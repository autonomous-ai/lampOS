use super::*;
use std::fs as stdfs;

#[cfg(target_os = "linux")]
thread_local! {
    static FIXTURE_ROOT: std::cell::RefCell<Option<(PathBuf, File)>> = const {
        std::cell::RefCell::new(None)
    };
}

/// Only explicitly scoped reader fixtures replace the filesystem root. The
/// original ancestor, owner, mode, ACL and component checks still run at and
/// below this pinned private root. These cases do not test host `/` or `/tmp`
/// ancestry; every unscoped security fixture still uses the original `/`.
#[cfg(target_os = "linux")]
pub(super) fn fixture_root<'a>(file: &mut File, path: &'a Path) -> io::Result<&'a Path> {
    FIXTURE_ROOT.with(|slot| {
        let slot = slot.borrow();
        let Some((root, pinned)) = slot.as_ref() else {
            return Ok(path);
        };
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid("fixture path escaped its owned root"))?;
        let current: File = fs::open(root, flags(), Mode::empty())?.into();
        same(&current, pinned)?;
        *file = pinned.try_clone()?;
        Ok(relative)
    })
}

#[cfg(target_os = "linux")]
struct FixtureRoot {
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(target_os = "linux")]
impl FixtureRoot {
    fn enter(root: &Path) -> Self {
        let pinned: File = fs::open(root, flags(), Mode::empty()).unwrap().into();
        FIXTURE_ROOT.with(|slot| {
            let mut slot = slot.borrow_mut();
            assert!(slot.is_none(), "fixture root scopes must not nest");
            *slot = Some((root.to_owned(), pinned));
        });
        Self {
            _thread: std::marker::PhantomData,
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for FixtureRoot {
    fn drop(&mut self) {
        FIXTURE_ROOT.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

#[test]
fn local_session_reader_group_is_exact_unique_and_bounded() {
    let parse = |text: &str| group_from("peek-readers", text.as_bytes());
    assert_eq!(parse("other:x:2:\npeek-readers:x:71:\n").unwrap(), 71);
    for text in [
        "peek-readers-other:x:71:\n",
        "peek-readers:x:71:\npeek-readers:x:72:\n",
        "peek-readers:x:4294967295:\n",
        "peek-readers:x:no:\n",
        "peek-readers:x:71:extra:field\n",
    ] {
        assert!(parse(text).is_err());
    }
    for name in [
        "",
        "71",
        "../peek",
        "peek readers",
        "peek\nreaders",
        "-peek",
    ] {
        assert!(group_from(name, "peek-readers:x:71:".as_bytes()).is_err());
    }
    assert!(group_from(&"p".repeat(129), &b""[..]).is_err());
    assert!(group_from("peek-readers", &b"\xff"[..]).is_err());
    assert!(
        group_from(
            "peek-readers",
            &vec![b'x'; MAX_GROUP_BYTES as usize + 1][..]
        )
        .is_err()
    );
}

#[test]
fn finite_and_unset_service_never_resolve_a_reader_group() {
    assert!(
        requested(
            false,
            Path::new("/not-used"),
            || panic!("finite read environment"),
            |_| panic!("finite lookup")
        )
        .is_none()
    );
    assert!(
        requested(
            true,
            Path::new("/not-used"),
            || None,
            |_| panic!("unset lookup")
        )
        .is_none()
    );
}

#[test]
fn group_lookup_failure_retains_private_logging_without_writer_failure() {
    let dir = crate::process::SessionDirectory::create().unwrap();
    let reader = requested(
        true,
        &dir.path,
        || Some("missing-group".into()),
        |_| Err(invalid("missing")),
    )
    .unwrap();
    let mut writer =
        crate::service_trace::ServiceTrace::start_with_reader(&dir.path, reader).unwrap();
    writer.submit(&serde_json::json!({"kind":"synthetic"}));
    let bounded = writer.finish(crate::service_trace::FINISH_WAIT);
    assert!(bounded.finish_attempted);
    let report = writer.observe_completion_for_test(&bounded, std::time::Duration::from_secs(5));
    assert!(
        report.acknowledged && report.degraded && !report.writer_failed,
        "bounded: {bounded:?}; completed: {report:?}"
    );
    assert_private(&dir.path);
    stdfs::remove_dir_all(dir.path.join("metadata")).unwrap();
}

fn assert_private(session: &Path) {
    assert_eq!(stdfs::metadata(session).unwrap().mode() & 0o7777, 0o700);
    assert_eq!(
        stdfs::metadata(session.join("metadata")).unwrap().mode() & 0o7777,
        0o700
    );
    for name in [
        "events-0.jsonl",
        "events-1.jsonl",
        "events-2.jsonl",
        "events-3.jsonl",
        "health.json",
    ] {
        assert_eq!(
            stdfs::metadata(session.join("metadata").join(name))
                .unwrap()
                .mode()
                & 0o7777,
            0o600
        );
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn unsupported_acl_verification_degrades_to_private_metadata() {
    let dir = crate::process::SessionDirectory::create().unwrap();
    let reader = Reader::prepare(&dir.path, rustix::process::getegid().as_raw());
    assert!(reader.is_err());
    let mut writer =
        crate::service_trace::ServiceTrace::start_with_reader(&dir.path, reader).unwrap();
    let bounded = writer.finish(crate::service_trace::FINISH_WAIT);
    assert!(bounded.finish_attempted);
    let report = writer.observe_completion_for_test(&bounded, std::time::Duration::from_secs(5));
    assert!(
        report.acknowledged && report.degraded && !report.writer_failed,
        "bounded: {bounded:?}; completed: {report:?}"
    );
    assert_private(&dir.path);
    stdfs::remove_dir_all(dir.path.join("metadata")).unwrap();
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::fs::{OpenOptions, Permissions};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        voice: PathBuf,
        sessions: PathBuf,
        output: PathBuf,
        gid: u32,
    }
    impl Fixture {
        fn new() -> Self {
            Self::under(&std::env::temp_dir().canonicalize().unwrap())
        }
        fn under(parent: &Path) -> Self {
            let root = parent.join(format!(
                "voice-session-reader-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            stdfs::create_dir(&root).unwrap();
            stdfs::set_permissions(&root, Permissions::from_mode(0o700)).unwrap();
            let voice = root.join("voice");
            let sessions = voice.join("sessions");
            stdfs::create_dir(&voice).unwrap();
            stdfs::create_dir(&sessions).unwrap();
            stdfs::set_permissions(&voice, Permissions::from_mode(0o710)).unwrap();
            stdfs::set_permissions(&sessions, Permissions::from_mode(0o750)).unwrap();
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join("sentinel"))
                .unwrap();
            Self {
                output: sessions.join("0123456789abcdef0123456789abcdef"),
                root,
                voice,
                sessions,
                gid: rustix::process::getegid().as_raw(),
            }
        }
        fn prepare(&self) -> Reader {
            Reader::prepare(&self.output, self.gid).unwrap()
        }
        fn create(&self) {
            stdfs::create_dir(&self.output).unwrap();
            stdfs::set_permissions(&self.output, Permissions::from_mode(0o700)).unwrap();
        }
        fn files(&self) -> (Vec<File>, File) {
            self.create();
            let metadata = self.output.join("metadata");
            stdfs::create_dir(&metadata).unwrap();
            stdfs::set_permissions(&metadata, Permissions::from_mode(0o700)).unwrap();
            let open = |name: &str| {
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(metadata.join(name))
                    .unwrap()
            };
            (
                (0..4).map(|n| open(&format!("events-{n}.jsonl"))).collect(),
                open("health.json"),
            )
        }
        fn sentinel(&self) {
            assert_eq!(
                stdfs::metadata(self.root.join("sentinel")).unwrap().mode() & 0o7777,
                0o600
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            stdfs::remove_dir_all(&self.root).unwrap();
        }
    }

    struct FixtureMode {
        path: PathBuf,
        original: Permissions,
    }
    impl FixtureMode {
        fn change(path: &Path, mode: u32) -> Self {
            let metadata = stdfs::symlink_metadata(path).unwrap();
            assert!(metadata.is_dir());
            let original = metadata.permissions();
            stdfs::set_permissions(path, Permissions::from_mode(mode)).unwrap();
            Self {
                path: path.to_owned(),
                original,
            }
        }
    }
    impl Drop for FixtureMode {
        fn drop(&mut self) {
            stdfs::set_permissions(&self.path, self.original.clone()).unwrap();
        }
    }

    #[test]
    fn only_the_new_session_is_shared_after_one_pinned_lookup() {
        for iteration in 0..2 {
            let fixture = Fixture::new();
            // This case checks the new session below its owned root.
            let _root = FixtureRoot::enter(&fixture.root);
            let old = fixture.sessions.join("old");
            stdfs::create_dir(&old).unwrap();
            stdfs::set_permissions(&old, Permissions::from_mode(0o700)).unwrap();
            let old_meta = stdfs::metadata(&old).unwrap();
            let lookups = std::cell::Cell::new(0);
            let began = std::time::Instant::now();
            let reader = requested(
                true,
                &fixture.output,
                || Some("peek-readers".into()),
                |name| {
                    lookups.set(lookups.get() + 1);
                    group_from(
                        name,
                        format!("peek-readers:x:{}:\n", fixture.gid).as_bytes(),
                    )
                },
            )
            .unwrap()
            .unwrap();
            fixture.create();
            let mut writer =
                crate::service_trace::ServiceTrace::start_with_reader(&fixture.output, Ok(reader))
                    .unwrap();
            eprintln!(
                "session reader setup iteration={} elapsed_us={} target_us=50000",
                iteration,
                began.elapsed().as_micros()
            );
            writer.submit(&serde_json::json!({"kind":"synthetic"}));
            let bounded = writer.finish(crate::service_trace::FINISH_WAIT);
            assert!(bounded.finish_attempted);
            let report =
                writer.observe_completion_for_test(&bounded, std::time::Duration::from_secs(5));
            assert!(
                report.acknowledged && !report.degraded,
                "bounded: {bounded:?}; completed: {report:?}"
            );
            assert_eq!(lookups.get(), 1);
            for path in [&fixture.output, &fixture.output.join("metadata")] {
                let meta = stdfs::metadata(path).unwrap();
                assert_eq!(meta.mode() & 0o7777, 0o750);
                assert_eq!(meta.gid(), fixture.gid);
            }
            for n in 0..4 {
                let m = stdfs::metadata(fixture.output.join(format!("metadata/events-{n}.jsonl")))
                    .unwrap();
                assert_eq!(m.mode() & 0o7777, 0o640);
                assert_eq!(m.gid(), fixture.gid);
            }
            assert_eq!(
                stdfs::metadata(fixture.output.join("metadata/health.json"))
                    .unwrap()
                    .mode()
                    & 0o7777,
                0o600
            );
            let after = stdfs::metadata(old).unwrap();
            assert_eq!(
                (after.ino(), after.gid(), after.mode()),
                (old_meta.ino(), old_meta.gid(), old_meta.mode())
            );
            fixture.sentinel();
        }
    }

    #[test]
    fn an_owned_fixture_root_ignores_only_changes_above_its_boundary() {
        for (pin_fixture, change_voice, shared) in [
            (false, false, false),
            (true, false, true),
            (true, true, false),
        ] {
            // Even while the controlled ancestor is writable, this top-level
            // private directory keeps the whole experiment inaccessible.
            let private = Fixture::new();
            let ancestor = Fixture::under(&private.root);
            let fixture = Fixture::under(&ancestor.root);
            let boundary = if pin_fixture {
                &fixture.root
            } else {
                &ancestor.root
            };
            let _root = FixtureRoot::enter(boundary);
            let reader = fixture.prepare();
            let (events, health) = fixture.files();
            // Admission preceded this change. Only a boundary below this
            // ancestor may exclude it from the grant's repeated checks.
            let _ancestor_mode = FixtureMode::change(&ancestor.root, 0o777);
            let _voice_mode = change_voice.then(|| FixtureMode::change(&fixture.voice, 0o700));
            assert_eq!(
                reader.grant(&fixture.output, &events, &health).unwrap(),
                shared
            );
            for path in [&fixture.output, &fixture.output.join("metadata")] {
                let metadata = stdfs::metadata(path).unwrap();
                assert_eq!(metadata.mode() & 0o7777, if shared { 0o750 } else { 0o700 });
                assert_eq!(metadata.gid(), fixture.gid);
            }
            for event in &events {
                let metadata = event.metadata().unwrap();
                assert_eq!(metadata.mode() & 0o7777, if shared { 0o640 } else { 0o600 });
                assert_eq!(metadata.gid(), fixture.gid);
            }
            assert_eq!(health.metadata().unwrap().mode() & 0o7777, 0o600);
            fixture.sentinel();
            ancestor.sentinel();
            private.sentinel();
        }
    }

    /// Explicitly invoked in an isolated root Linux container; ordinary mode
    /// assertions do not substitute for this cross-identity DAC proof.
    #[test]
    #[ignore = "requires isolated Linux root and setpriv; run explicitly with --ignored"]
    fn dedicated_reader_can_only_read_new_events_under_dropped_identity() {
        assert_eq!(
            rustix::process::geteuid().as_raw(),
            0,
            "requires container root"
        );
        let fixture_with_reader_group = || {
            let mut fixture = Fixture::new();
            fixture.gid = 65533;
            for path in [&fixture.voice, &fixture.sessions] {
                fs::fchown(
                    File::open(path).unwrap(),
                    None,
                    Some(rustix::process::Gid::from_raw(fixture.gid)),
                )
                .unwrap();
            }
            fixture
        };
        // Root can assign a genuinely different group without creating an
        // account. Verify every object's original group, not only event modes.
        for fail in 0..13 {
            let fixture = fixture_with_reader_group();
            let reader = fixture.prepare();
            let (events, health) = fixture.files();
            let session = File::open(&fixture.output).unwrap();
            let metadata = File::open(fixture.output.join("metadata")).unwrap();
            let original_files: Vec<_> = [&session, &metadata]
                .into_iter()
                .chain(events.iter())
                .chain(std::iter::once(&health))
                .collect();
            for file in &original_files {
                fs::fchown(*file, None, Some(rustix::process::Gid::from_raw(0))).unwrap();
            }
            let originals: Vec<_> = original_files
                .iter()
                .map(|file| file.metadata().unwrap().gid())
                .collect();
            assert!(originals.iter().all(|gid| *gid != fixture.gid));
            let observed_changed_gid = std::cell::Cell::new(false);
            let shared = reader
                .grant_with(&fixture.output, &events, &health, |rollback, step| {
                    if !rollback && step == fail {
                        if step > 0 {
                            assert_eq!(events[0].metadata().unwrap().gid(), fixture.gid);
                            observed_changed_gid.set(true);
                        }
                        Err(io::Error::other("injected distinct-group failure"))
                    } else {
                        Ok(())
                    }
                })
                .unwrap();
            assert!(!shared, "distinct-group failure {fail}");
            assert_eq!(observed_changed_gid.get(), fail > 0);
            assert_private(&fixture.output);
            assert_eq!(
                original_files
                    .iter()
                    .map(|file| file.metadata().unwrap().gid())
                    .collect::<Vec<_>>(),
                originals,
                "session, metadata, events and health must restore original GIDs"
            );
            fixture.sentinel();
        }
        let fixture = fixture_with_reader_group();
        stdfs::set_permissions(&fixture.root, Permissions::from_mode(0o711)).unwrap();
        let onboarding = fixture.voice.join("onboarding");
        stdfs::create_dir(&onboarding).unwrap();
        stdfs::set_permissions(&onboarding, Permissions::from_mode(0o700)).unwrap();
        let progress = onboarding.join("progress.json");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&progress)
            .unwrap();
        let reader = fixture.prepare();
        fixture.create();
        let mut writer =
            crate::service_trace::ServiceTrace::start_with_reader(&fixture.output, Ok(reader))
                .unwrap();
        writer
            .submit(&serde_json::json!({"kind":"transcript","text":"DAC_SECRET_CANARY","turn":1}));
        writer.submit(&serde_json::json!({"kind":"answer_stages","turn":1,"words_ms":11,"chat_first_ms":22,"first_answer_audio_ms":33,"private":{"path":"DAC_SECRET_CANARY"}}));
        writer
            .submit(&serde_json::json!({"kind":"DAC_SECRET_CANARY","outcome":"DAC_SECRET_CANARY"}));
        let bounded = writer.finish(crate::service_trace::FINISH_WAIT);
        assert!(!bounded.writer_failed, "bounded finish: {bounded:?}");
        assert_eq!(bounded.degraded, !bounded.acknowledged);
        let completed =
            writer.observe_completion_for_test(&bounded, std::time::Duration::from_secs(5));
        assert!(completed.acknowledged && !completed.degraded && !completed.writer_failed);
        let event = fixture.output.join("metadata/events-0.jsonl");
        let health = fixture.output.join("metadata/health.json");
        let sentinel = fixture.root.join("sentinel");
        let reader_script = r#"
set -eu
for segment in "$5"/events-0.jsonl "$5"/events-1.jsonl "$5"/events-2.jsonl "$5"/events-3.jsonl; do
    cat "$segment" >/dev/null
    if grep -q 'DAC_SECRET_CANARY' "$segment"; then exit 16; fi
done
grep -q '"words_ms":11' "$1"
for private in "$2" "$3" "$4"; do
    if cat "$private" >/dev/null 2>&1; then exit 11; fi
done
if (printf forbidden >> "$1") 2>/dev/null; then exit 12; fi
if mv "$1" "$1.moved" 2>/dev/null; then exit 13; fi
if rm "$1" 2>/dev/null; then exit 14; fi
"#;
        let run = |gid: &str, script: &str| {
            std::process::Command::new("setpriv")
                .args([
                    "--reuid",
                    "65534",
                    "--regid",
                    gid,
                    "--clear-groups",
                    "--",
                    "/bin/sh",
                    "-c",
                    script,
                    "reader-fixture",
                ])
                .args([
                    &event,
                    &health,
                    &progress,
                    &sentinel,
                    &fixture.output.join("metadata"),
                ])
                .output()
                .unwrap()
        };
        let reader = run("65533", reader_script);
        assert!(
            reader.status.success(),
            "reader DAC failure: {:?}",
            reader.status
        );
        assert!(
            run("65532", r#"if cat "$1" >/dev/null 2>&1; then exit 15; fi"#)
                .status
                .success()
        );
        assert!(event.exists());
        assert_eq!(stdfs::metadata(&health).unwrap().mode() & 0o7777, 0o600);
        fixture.sentinel();
    }

    #[test]
    fn unprepared_or_wrong_group_parent_never_grants_or_changes_ancestors() {
        for role in [
            "private-parent",
            "private-root",
            "wrong-group",
            "old-session",
            "linked-parent",
        ] {
            let fixture = Fixture::new();
            match role {
                "private-parent" => {
                    stdfs::set_permissions(&fixture.voice, Permissions::from_mode(0o700)).unwrap()
                }
                "private-root" => {
                    stdfs::set_permissions(&fixture.sessions, Permissions::from_mode(0o700))
                        .unwrap()
                }
                "old-session" => fixture.create(),
                "linked-parent" => {
                    let moved = fixture.root.join("actual");
                    stdfs::rename(&fixture.voice, &moved).unwrap();
                    std::os::unix::fs::symlink(moved, &fixture.voice).unwrap();
                }
                _ => {}
            }
            let gid = if role == "wrong-group" {
                fixture.gid.wrapping_add(1)
            } else {
                fixture.gid
            };
            assert!(Reader::prepare(&fixture.output, gid).is_err(), "{role}");
            fixture.sentinel();
        }
    }

    #[test]
    fn readable_or_linked_voice_siblings_deny_preparation_and_late_grant() {
        for mode in [0o644, 0o640, 0o660] {
            let fixture = Fixture::new();
            let path = fixture.voice.join("near-call.json");
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(&path)
                .unwrap();
            stdfs::set_permissions(&path, Permissions::from_mode(mode)).unwrap();
            assert!(Reader::prepare(&fixture.output, fixture.gid).is_err());
            assert_eq!(stdfs::metadata(&path).unwrap().mode() & 0o7777, mode);
        }
        let fixture = Fixture::new();
        let reader = fixture.prepare();
        let path = fixture.voice.join("loose-state");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&path)
            .unwrap();
        stdfs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
        let (events, health) = fixture.files();
        assert!(!reader.grant(&fixture.output, &events, &health).unwrap());
        assert_private(&fixture.output);
        assert_eq!(stdfs::metadata(&path).unwrap().mode() & 0o7777, 0o644);
        stdfs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(fixture.root.join("sentinel"), &path).unwrap();
        assert!(Reader::prepare(&fixture.sessions.join("next"), fixture.gid).is_err());
        fixture.sentinel();
    }

    #[test]
    fn every_grant_failure_restores_private_modes_and_original_groups() {
        for fail in 0..13 {
            let fixture = Fixture::new();
            let reader = fixture.prepare();
            let (events, health) = fixture.files();
            let original: Vec<_> = events.iter().map(|f| f.metadata().unwrap().gid()).collect();
            let shared = reader
                .grant_with(&fixture.output, &events, &health, |rollback, step| {
                    if !rollback && step == fail {
                        Err(io::Error::other("injected"))
                    } else {
                        Ok(())
                    }
                })
                .unwrap();
            assert!(!shared, "failure {fail}");
            assert_private(&fixture.output);
            assert_eq!(
                events
                    .iter()
                    .map(|f| f.metadata().unwrap().gid())
                    .collect::<Vec<_>>(),
                original
            );
            fixture.sentinel();
        }
    }

    #[test]
    fn failed_private_rollback_disables_only_metadata() {
        let fixture = Fixture::new();
        let _root = FixtureRoot::enter(&fixture.root);
        let mut reader = fixture.prepare();
        reader.failure = Some((12, Some(0)));
        let observation = std::sync::Arc::new(std::sync::Mutex::new(FailureObservation::default()));
        reader.failure_observation = Some(observation.clone());
        fixture.create();
        let mut state =
            crate::session_state::SessionState::service_with_reader(&fixture.output, Ok(reader));
        assert!(state.continuous());
        state
            .record(serde_json::json!({"kind":"synthetic"}))
            .unwrap();
        state.flush(&mut None, 1);
        let report = state.finish_logging().unwrap();
        let observation = observation.lock().unwrap().clone();
        eprintln!(
            "private rollback fixture: grant_hit={} rollback_hit={} apply_error={:?}; report={report:?}",
            observation.injected_grant_hit,
            observation.injected_rollback_hit,
            observation.apply_error,
        );
        assert!(
            observation.injected_grant_hit,
            "grant step 12 was not exercised: {observation:?}; report={report:?}"
        );
        assert!(
            observation.injected_rollback_hit,
            "rollback step 0 was not exercised: {observation:?}; report={report:?}"
        );
        assert!(report.initialization_failed, "{report:?}; {observation:?}");
        assert!(report.degraded, "{report:?}; {observation:?}");
        assert_eq!(report.dropped_events, 1, "{report:?}; {observation:?}");
        assert_eq!(
            stdfs::metadata(fixture.output.join("metadata/events-0.jsonl"))
                .unwrap()
                .len(),
            0
        );
        fixture.sentinel();
    }

    #[test]
    fn an_earlier_reader_refusal_keeps_a_verified_private_writer() {
        let fixture = Fixture::new();
        let _root = FixtureRoot::enter(&fixture.root);
        let mut reader = fixture.prepare();
        reader.failure = Some((12, Some(0)));
        let observation = std::sync::Arc::new(std::sync::Mutex::new(FailureObservation::default()));
        reader.failure_observation = Some(observation.clone());
        fixture.create();
        // A deterministic, real privacy refusal before any grant checkpoint.
        let loose = fixture.voice.join("loose-state");
        stdfs::write(&loose, b"synthetic").unwrap();
        stdfs::set_permissions(&loose, Permissions::from_mode(0o644)).unwrap();
        let mut state =
            crate::session_state::SessionState::service_with_reader(&fixture.output, Ok(reader));
        assert!(state.continuous());
        state
            .record(serde_json::json!({"kind":"synthetic"}))
            .unwrap();
        state.flush(&mut None, 1);
        let bounded = state.finish_logging().unwrap();
        let observation = observation.lock().unwrap().clone();
        eprintln!("early private reader refusal: {observation:?}; bounded={bounded:?}");
        assert!(!observation.injected_grant_hit, "{observation:?}");
        assert!(observation.injected_rollback_hit, "{observation:?}");
        assert!(
            observation
                .apply_error
                .as_deref()
                .is_some_and(|error| error.starts_with("untrusted session record")),
            "{observation:?}"
        );
        assert!(!bounded.initialization_failed, "{bounded:?}");
        assert!(bounded.degraded && !bounded.writer_failed, "{bounded:?}");
        assert!(bounded.finish_attempted, "{bounded:?}");
        let report = state
            .writer_for_test()
            .observe_completion_for_test(&bounded, std::time::Duration::from_secs(5));
        eprintln!("early private reader completion: {report:?}; bounded={bounded:?}");
        assert!(
            !report.initialization_failed && report.degraded,
            "{report:?}"
        );
        assert!(report.acknowledged && !report.writer_failed, "{report:?}");
        assert_eq!(report.dropped_events, 0, "{report:?}");
        assert_eq!(report.written_events, 1, "{report:?}");
        assert_private(&fixture.output);
        fixture.sentinel();
    }

    #[test]
    fn a_fixture_root_retains_ancestor_mode_acl_and_path_checks() {
        let fixture = Fixture::new();
        let _root = FixtureRoot::enter(&fixture.root);
        let escaped = Reader::prepare(Path::new("/outside/voice/sessions/new"), fixture.gid)
            .err()
            .expect("an escaped fixture path must be refused");
        assert_eq!(escaped.to_string(), "fixture path escaped its owned root");
        stdfs::set_permissions(&fixture.root, Permissions::from_mode(0o777)).unwrap();
        let unsafe_mode = Reader::prepare(&fixture.output, fixture.gid)
            .err()
            .expect("an unsafe fixture ancestor must be refused");
        assert_eq!(unsafe_mode.to_string(), "untrusted session ancestor");
        stdfs::set_permissions(&fixture.root, Permissions::from_mode(0o700)).unwrap();
        acl(&fixture.root, false);
        let with_acl = Reader::prepare(&fixture.output, fixture.gid)
            .err()
            .expect("a fixture ancestor with an ACL must be refused");
        assert_eq!(
            with_acl.to_string(),
            "session reader ACL cannot be verified absent"
        );
        fixture.sentinel();
    }

    fn acl(path: &Path, default: bool) {
        let file = File::open(path).unwrap();
        let mut bytes = 2u32.to_le_bytes().to_vec();
        for (tag, permission, id) in [
            (1u16, 7u16, u32::MAX),
            (2, 4, rustix::process::geteuid().as_raw().wrapping_add(1)),
            (4, 5, u32::MAX),
            (16, 5, u32::MAX),
            (32, 0, u32::MAX),
        ] {
            bytes.extend(tag.to_le_bytes());
            bytes.extend(permission.to_le_bytes());
            bytes.extend(id.to_le_bytes());
        }
        fs::fsetxattr(
            &file,
            if default {
                "system.posix_acl_default"
            } else {
                "system.posix_acl_access"
            },
            &bytes,
            fs::XattrFlags::empty(),
        )
        .unwrap();
    }
    #[test]
    fn access_and_default_acl_are_refused_without_stripping_or_widening_them() {
        for default in [false, true] {
            let fixture = Fixture::new();
            acl(&fixture.sessions, default);
            assert!(Reader::prepare(&fixture.output, fixture.gid).is_err());
            let file = File::open(&fixture.sessions).unwrap();
            assert!(
                fs::fgetxattr(
                    &file,
                    if default {
                        "system.posix_acl_default"
                    } else {
                        "system.posix_acl_access"
                    },
                    &mut [0u8; 128]
                )
                .is_ok()
            );
            fixture.sentinel();
        }
    }
    #[test]
    fn replaced_record_never_changes_an_outside_sentinel() {
        let fixture = Fixture::new();
        let _root = FixtureRoot::enter(&fixture.root);
        let reader = fixture.prepare();
        let (events, health) = fixture.files();
        let target = fixture.output.join("metadata/events-0.jsonl");
        stdfs::remove_file(&target).unwrap();
        std::os::unix::fs::symlink(fixture.root.join("sentinel"), &target).unwrap();
        assert!(reader.grant(&fixture.output, &events, &health).is_err());
        fixture.sentinel();
    }
    #[test]
    fn shared_session_writer_projects_every_segment_without_private_values() {
        use serde_json::{Value, json};
        const SECRET: &str = "READER_SECRET_CANARY";
        let fixture = Fixture::new();
        let _root = FixtureRoot::enter(&fixture.root);
        let reader = fixture.prepare();
        fixture.create();
        let mut state = crate::service_trace::small_segments(1024, || {
            crate::session_state::SessionState::service_with_reader(&fixture.output, Ok(reader))
        });
        let rows = [
            json!({"kind":"input_admitted","turn":7,"at_us":100,"admission_basis":"local_speech",
                "owner":{"boot":SECRET},"person":SECRET,"path":SECRET}),
            json!({"kind":"answer_stages","turn":7,"at_us":200,"decided_at_us":100,
                "words_ms":13,"cue_ms":null,"chat_first_ms":29,"first_answer_audio_ms":47,
                "words":SECRET,"nested":{"identity":SECRET},"unexpected":[SECRET]}),
            json!({"kind":"answer_first_audio","turn":7,"at_us":300,"after_decision_ms":51,"text":SECRET}),
            json!({"kind":"turn_finished","turn":7,"at_us":400,"outcome":"audio_written_unscored",
                "playback_gaps":2,"private_detail":SECRET}),
            json!({"kind":"transcript","turn":7,"at_us":500,"text":SECRET,"finished":true,
                "input_correlation":SECRET}),
            json!({"kind":SECRET,"turn":7,"at_us":600,"outcome":SECRET,"anything":{"name":SECRET}}),
            json!({"kind":"turn_finished","turn":SECRET,"at_us":SECRET,"outcome":SECRET,
                "playback_gaps":{"value":SECRET},"text":SECRET}),
        ];
        let began = std::time::Instant::now();
        for _ in 0..8 {
            for row in &rows {
                state.record(row.clone()).unwrap();
            }
            state.flush(&mut None, 1);
        }
        let enqueue = began.elapsed();
        let bounded = state.finish_logging().unwrap();
        assert!(!bounded.writer_failed, "bounded finish: {bounded:?}");
        assert!(bounded.finish_attempted);
        assert_eq!(bounded.degraded, !bounded.acknowledged);
        let report = state
            .writer_for_test()
            .observe_completion_for_test(&bounded, std::time::Duration::from_secs(5));
        assert!(report.acknowledged && !report.degraded && !report.writer_failed);
        assert_eq!((report.written_events, report.dropped_events), (56, 0));
        assert!(report.rotations >= 4, "all four segments must rotate");
        let mut all = Vec::new();
        for n in 0..4 {
            let path = fixture.output.join(format!("metadata/events-{n}.jsonl"));
            let bytes = stdfs::read(&path).unwrap();
            assert!(!bytes.is_empty(), "segment {n} was really written");
            assert!(bytes.len() <= 1024, "projected records stay bounded");
            assert!(
                !String::from_utf8_lossy(&bytes).contains(SECRET),
                "segment {n} leaks a private value"
            );
            assert_eq!(stdfs::metadata(&path).unwrap().mode() & 0o7777, 0o640);
            for line in bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                let row: Value = serde_json::from_slice(line).unwrap();
                let object = row.as_object().unwrap();
                if row["kind"] == "service_segment" {
                    continue;
                }
                assert_eq!(row["shared_metadata"], 1);
                let numeric = [
                    "service_sequence",
                    "turn",
                    "at_us",
                    "decided_at_us",
                    "words_ms",
                    "cue_ms",
                    "chat_first_ms",
                    "first_answer_audio_ms",
                    "after_decision_ms",
                    "playback_gaps",
                ];
                for (key, value) in object {
                    if numeric.contains(&key.as_str()) {
                        assert!(value.is_u64());
                    } else {
                        assert!(
                            ["kind", "shared_metadata", "admission_basis", "outcome"]
                                .contains(&key.as_str()),
                            "unexpected key {key}"
                        );
                    }
                    assert!(
                        !value.is_object() && !value.is_array(),
                        "no private nested field"
                    );
                }
                all.push(row);
            }
        }
        let stages = all
            .iter()
            .find(|row| row["kind"] == "answer_stages")
            .expect("retained stage row");
        assert_eq!(
            (
                stages["words_ms"].as_u64(),
                stages["chat_first_ms"].as_u64(),
                stages["first_answer_audio_ms"].as_u64()
            ),
            (Some(13), Some(29), Some(47))
        );
        assert!(
            all.iter()
                .any(|row| row["kind"] == "input_admitted"
                    && row["admission_basis"] == "local_speech")
        );
        assert!(
            all.iter()
                .any(|row| row["kind"] == "answer_first_audio" && row["after_decision_ms"] == 51)
        );
        assert!(all.iter().any(|row| row["kind"] == "turn_finished"
            && row["outcome"] == "audio_written_unscored"
            && row["playback_gaps"] == 2));
        assert!(all.iter().any(|row| row["kind"] == "service_event"));
        assert!(all.iter().any(|row| row["kind"] == "turn_finished"
            && row.get("outcome").is_none()
            && row.get("turn").is_none()));
        let health = fixture.output.join("metadata/health.json");
        assert_eq!(stdfs::metadata(&health).unwrap().mode() & 0o7777, 0o600);
        let health: Value = serde_json::from_slice(&stdfs::read(health).unwrap()).unwrap();
        assert_eq!(health["health"]["written_events"], 56);
        eprintln!(
            "shared metadata enqueue: events=56 total_us={} per_event_us={} target_per_event_us=1000; host only, no device claim",
            enqueue.as_micros(),
            enqueue.as_micros() / 56
        );
    }

    #[test]
    fn a_verified_failed_grant_keeps_private_original_records() {
        let fixture = Fixture::new();
        let mut reader = fixture.prepare();
        reader.failure = Some((12, None));
        fixture.create();
        let mut state =
            crate::session_state::SessionState::service_with_reader(&fixture.output, Ok(reader));
        let original =
            serde_json::json!({"kind":"transcript","text":"PRIVATE_ROLLBACK_CONTROL","turn":8});
        state.record(original.clone()).unwrap();
        state.flush(&mut None, 1);
        let bounded = state.finish_logging().unwrap();
        assert!(!bounded.writer_failed, "bounded finish: {bounded:?}");
        assert!(bounded.finish_attempted);
        assert!(bounded.degraded);
        let report = state
            .writer_for_test()
            .observe_completion_for_test(&bounded, std::time::Duration::from_secs(5));
        assert!(report.acknowledged && report.degraded && !report.writer_failed);
        assert_private(&fixture.output);
        let bytes = stdfs::read_to_string(fixture.output.join("metadata/events-0.jsonl")).unwrap();
        let saved: serde_json::Value = serde_json::from_str(bytes.lines().nth(1).unwrap()).unwrap();
        let mut expected = original;
        expected["service_sequence"] = serde_json::json!(1);
        assert_eq!(
            saved, expected,
            "failed sharing keeps the private trace contract"
        );
    }
}
