//! #2810: restart from logical persisted prefixes of an ordinary self-switch.
//! These are filesystem/error-injection tests, not physical power-loss or
//! fsync durability proof. The updater stubs identify themselves and exit;
//! the real library performs the signed install, round, confirmation and back.
use super::*;
use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    panic::{AssertUnwindSafe, catch_unwind, panic_any, resume_unwind},
    process::{Child, Command, ExitStatus, Stdio},
    time::Instant,
};

#[path = "runnable_events.rs"]
mod runnable_events;

const OLD: &str = "#!/bin/sh\nprintf 'old-updater\\n'\n";
const NEW: &str = "#!/bin/sh\nprintf 'new-updater\\n'\n";

thread_local! {
    static EXECUTABLE_CHECK: Cell<usize> = const { Cell::new(0) };
}

/// Only the two immediate, builtin-only scripts above are run here. Reap
/// on both assertion failure and timeout; neither script creates children.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let started = Instant::now();
        let killed = self.0.kill();
        let reaped = self.0.wait();
        if std::thread::panicking() {
            // Do not panic again if the test runner's stderr is unavailable.
            let _ = writeln!(
                io::stderr().lock(),
                "updater fixture cleanup: pid={} kill={killed:?} reap={reaped:?} elapsed={:?}",
                self.0.id(),
                started.elapsed(),
            );
        }
    }
}

#[track_caller]
fn executable(path: &Path, script: &str) {
    let invocation = EXECUTABLE_CHECK.with(|count| {
        let next = count.get() + 1;
        count.set(next);
        next
    });
    let expected = if script == OLD {
        "old"
    } else {
        assert_eq!(script, NEW);
        "new"
    };
    let context = format!(
        "check={invocation} caller={} path={} expected={expected} sha256={}",
        std::panic::Location::caller(),
        path.display(),
        sha(script),
    );
    assert_eq!(fs::read(path).unwrap(), script.as_bytes());
    let metadata = fs::symlink_metadata(path).unwrap();
    assert!(metadata.is_file(), "the executable must be a regular file");
    assert_eq!(metadata.permissions().mode() & 0o777, 0o755);
    let _forking = forking();
    let mut child = Running(
        Command::new(path)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| {
                panic!("the selected updater must execute: {context}; {error}")
            }),
    );
    let started = Instant::now();
    let stdout = child.0.stdout.take().unwrap();
    let (status, output) = runnable_events::observe(&mut child.0, &stdout, started)
        .unwrap_or_else(|error| panic!("updater fixture observation failed: {context}; {error}"));
    assert_runnable_output(status, output, script, &context);
}

#[track_caller]
fn assert_runnable_output(status: ExitStatus, output: Vec<u8>, script: &str, context: &str) {
    assert!(status.success(), "{context}; {status}");
    let output = String::from_utf8(output).unwrap_or_else(|error| {
        panic!("updater fixture output is not UTF-8: {context}; {error:?}")
    });
    assert_eq!(
        output,
        if script == OLD {
            "old-updater\n"
        } else {
            assert_eq!(script, NEW);
            "new-updater\n"
        },
        "{context}",
    );
}

#[track_caller]
fn build(lamp: &Lamp, script: &str) {
    executable(
        &lamp
            .config
            .on(&unit::folder(own::FOLDER, &sha(script)))
            .join(own::BINARY),
        script,
    );
}

/// Check the selected executable before the expected link name: an unsafe
/// unlink/target mutation must fail because the selected file is absent.
#[track_caller]
fn boundary(lamp: &Lamp, current: &str) {
    executable(&lamp.config.on(own::CURRENT).join(own::BINARY), current);
    assert_eq!(lamp.own_build(), sha(current));
    // Even with new selected, the old build remains runnable for put-back.
    build(lamp, OLD);
    // Install's fixed fallback keeps its exact original bytes and mode.
    executable(&lamp.config.on(own::BACK), OLD);
}

#[track_caller]
fn prepared() -> Lamp {
    let mut lamp = Lamp::installed_with_updater(OLD);
    lamp.config.self_switch = true;
    lamp.systemd.self_recovery = Ok(());
    product_build_with_updater(&mut lamp, 2, NEW);
    boundary(&lamp, OLD);
    lamp
}

#[derive(Debug)]
struct Cut;

fn stop_at(lamp: &mut Lamp, line: &str) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        lamp.round_observing(&mut |said| {
            if said == line {
                panic_any(Cut);
            }
        })
    }));
    match outcome {
        Err(stopped) if stopped.is::<Cut>() => {}
        Err(other) => resume_unwind(other),
        Ok(other) => panic!("cut boundary was not reached: {other:?}; {:?}", lamp.said),
    }
    assert_eq!(lamp.state()["installed"]["sequence"], 2);
}

fn pending(lamp: &Lamp) -> own::Pending {
    let pending = lamp.pending().expect("handover must remain recoverable");
    assert_eq!(pending.version, 1);
    assert_eq!(pending.old, sha(OLD));
    assert_eq!(pending.new, sha(NEW));
    pending
}

#[track_caller]
fn switch(lamp: &mut Lamp) {
    assert_eq!(
        lamp.round(),
        Outcome::SelfSwitched {
            sequence: 2,
            from: sha(OLD),
            to: sha(NEW),
        },
        "{:?}",
        lamp.said
    );
    pending(lamp);
    boundary(lamp, NEW);
}

#[track_caller]
fn confirmed(lamp: &mut Lamp) {
    assert!(lamp.pending().is_none());
    assert!(own::refused(&lamp.records()).is_empty());
    boundary(lamp, NEW);
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::Nothing
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    boundary(lamp, NEW);
}

#[track_caller]
fn put_back(lamp: &mut Lamp) {
    assert!(matches!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::PutBack(_)
    ));
    assert!(lamp.pending().is_none());
    assert_eq!(own::refused(&lamp.records()), vec![sha(NEW)]);
    boundary(lamp, OLD);
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::Nothing
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    boundary(lamp, OLD);
}

fn back_error(lamp: &mut Lamp) -> String {
    let _no_fork = no_fork();
    own::back(
        &lamp.records(),
        &lamp.root,
        u64::MAX,
        &update_unit("failed", "failed"),
        &mut |_| {},
    )
    .expect_err("the occupied atomic-file name must interrupt put-back")
}

#[test]
fn cut_after_candidate_is_synced_keeps_old_runnable_and_reuses_candidate() {
    let mut lamp = prepared();
    let folder = unit::folder(own::FOLDER, &sha(NEW));
    stop_at(
        &mut lamp,
        &format!(
            "installed {folder} beside {}",
            unit::folder(own::FOLDER, &sha(OLD))
        ),
    );
    // The final rename/sync and state write finished, but begin did not.
    boundary(&lamp, OLD);
    build(&lamp, NEW);
    assert!(lamp.pending().is_none());
    assert!(
        lamp.state()["made"]
            .as_array()
            .unwrap()
            .contains(&folder.into())
    );

    // Removing only the remote candidate proves restart reuses the checked
    // local bytes; the signed manifest and ordinary parts remain available.
    let candidate = format!("https://channel.test/build-2/{}", own::BINARY);
    assert!(lamp.channel.files.remove(&candidate).is_some());
    switch(&mut lamp);
    assert_eq!(lamp.confirm(&sha(NEW)), own::Confirm::Confirmed);
    confirmed(&mut lamp);
}

#[test]
fn cut_after_pending_is_saved_keeps_old_runnable_and_recovers_before_next_round() {
    let mut lamp = prepared();
    let blocker = lamp
        .config
        .on(own::CURRENT)
        .with_file_name(".current.lamp-update");
    fs::create_dir(&blocker).unwrap();
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    boundary(&lamp, OLD);
    build(&lamp, NEW);
    let saved = pending(&lamp);
    assert!(
        lamp.said
            .iter()
            .any(|line| line.contains("cannot be pointed"))
    );
    assert!(matches!(lamp.round(), Outcome::Refused(why) if why.contains("not confirmed")));
    assert_eq!(lamp.pending(), Some(saved));
    boundary(&lamp, OLD);
    fs::remove_dir(&blocker).unwrap();
    put_back(&mut lamp);
}

#[test]
fn cut_after_current_moves_keeps_new_runnable_and_can_confirm_or_put_back() {
    // Both startup outcomes matter at this same prefix: a proven new
    // updater stays; one still unconfirmed at its deadline goes back.
    for proves_itself in [true, false] {
        let mut lamp = prepared();
        stop_at(
            &mut lamp,
            &format!(
                "lamp-update switches to its build {}; it must confirm itself within {} min",
                &sha(NEW)[..12],
                own::DEADLINE.as_secs() / 60
            ),
        );
        boundary(&lamp, NEW);
        let saved = pending(&lamp);
        if proves_itself {
            assert_eq!(lamp.confirm(&sha(NEW)), own::Confirm::Confirmed);
            confirmed(&mut lamp);
        } else {
            let late = saved.at_unix_ms + own::DEADLINE.as_millis() as u64;
            assert!(matches!(
                lamp.back(update_unit("active", "running"), late),
                own::Back::PutBack(_)
            ));
            assert!(lamp.pending().is_none());
            assert_eq!(own::refused(&lamp.records()), vec![sha(NEW)]);
            boundary(&lamp, OLD);
            assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
            boundary(&lamp, OLD);
        }
    }
}

#[test]
fn cut_after_confirmation_clears_pending_keeps_new_runnable_on_restart() {
    let mut lamp = prepared();
    switch(&mut lamp);
    // confirm returns only after removing self.json and syncing its folder.
    // Recreate World on the next calls, with no further handover state.
    assert_eq!(lamp.confirm(&sha(NEW)), own::Confirm::Confirmed);
    boundary(&lamp, NEW);
    confirmed(&mut lamp);
    assert_eq!(lamp.confirm(&sha(NEW)), own::Confirm::Nothing);
}

#[test]
fn interrupted_put_back_keeps_a_runnable_current_and_finishes_on_restart() {
    let mut lamp = prepared();
    switch(&mut lamp);
    let saved = pending(&lamp);

    // Failure before atomic replacement must leave the selected new build
    // runnable. In particular, it must not be deleted before old is linked.
    let link_blocker = lamp
        .config
        .on(own::CURRENT)
        .with_file_name(".current.lamp-update");
    fs::create_dir(&link_blocker).unwrap();
    assert!(back_error(&mut lamp).contains(own::CURRENT));
    boundary(&lamp, NEW);
    assert_eq!(lamp.pending(), Some(saved.clone()));
    fs::remove_dir(&link_blocker).unwrap();

    // Now the old link is restored, then recording the refusal fails. The
    // pending record must survive until that remaining write can finish.
    let refusal_blocker = lamp.records().join(".self-refused.lamp-update");
    fs::create_dir(&refusal_blocker).unwrap();
    assert!(back_error(&mut lamp).contains("self-refused"));
    boundary(&lamp, OLD);
    assert_eq!(lamp.pending(), Some(saved));
    assert!(own::refused(&lamp.records()).is_empty());
    fs::remove_dir(&refusal_blocker).unwrap();
    put_back(&mut lamp);

    // A separate logical-prefix reconstruction covers the remaining gap:
    // old restored and new refused, but self.json not yet removed. This
    // does not interrupt back at that point: let the real back write its
    // result, then restore only the exact pending bytes captured beforehand.
    let mut lamp = prepared();
    switch(&mut lamp);
    let saved = pending(&lamp);
    let pending_path = lamp.records().join("self.json");
    let pending_bytes = fs::read(&pending_path).unwrap();
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::PutBack(saved.clone())
    );
    assert!(lamp.pending().is_none());
    boundary(&lamp, OLD);
    let refusal_path = lamp.records().join("self-refused");
    let refused_bytes = fs::read(&refusal_path).unwrap();
    assert_eq!(refused_bytes, format!("{}\n", sha(NEW)).as_bytes());
    let completed_files = lamp.files();

    lamp_update::records::write_whole(&pending_path, &pending_bytes, 0o600).unwrap();
    let mut reconstructed_files = completed_files.clone();
    reconstructed_files.insert(
        "var/lib/lampos-update/self.json".into(),
        pending_bytes.clone(),
    );
    assert_eq!(lamp.files(), reconstructed_files);
    assert_eq!(fs::read(&pending_path).unwrap(), pending_bytes);
    assert_eq!(lamp.pending(), Some(saved.clone()));
    assert_eq!(own::refused(&lamp.records()), vec![sha(NEW)]);
    boundary(&lamp, OLD);

    assert_eq!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::PutBack(saved)
    );
    assert!(lamp.pending().is_none());
    assert_eq!(fs::read(&refusal_path).unwrap(), refused_bytes);
    // Every file as the completed put-back left it, but the log: each
    // put-back is a round's end of its own (#1683's update alerts), so the
    // finished one adds its line.
    let log = "var/lib/lampos-update/log.jsonl";
    let without_log = |mut files: BTreeMap<String, Vec<u8>>| {
        files.remove(log);
        files
    };
    assert_eq!(
        without_log(lamp.files()),
        without_log(completed_files.clone())
    );
    assert!(lamp.files()[log].starts_with(&completed_files[log]));
    let told = lamp_update::status::last_in_records(&lamp.records()).unwrap();
    assert_eq!(told.outcome, "self-put-back");
    boundary(&lamp, OLD);
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    assert_eq!(fs::read(&refusal_path).unwrap(), refused_bytes);
    boundary(&lamp, OLD);
}

#[test]
fn without_self_opt_in_parts_update_but_no_handover_prefix_is_written() {
    for value in [None, Some("off"), Some("ON"), Some("yes"), Some(" on")] {
        assert!(!own::on(value));
    }
    let mut lamp = prepared();
    lamp.config.self_switch = own::on(None);
    assert_eq!(lamp.round(), Outcome::Switched(2), "{:?}", lamp.said);
    boundary(&lamp, OLD);
    assert!(lamp.pending().is_none());
    assert!(own::refused(&lamp.records()).is_empty());
    assert!(
        !lamp
            .config
            .on(&unit::folder(own::FOLDER, &sha(NEW)))
            .exists()
    );
    assert_eq!(lamp.systemd.self_recovery_queries, 0);
    assert_eq!(lamp.confirm(&sha(OLD)), own::Confirm::Nothing);
    assert_eq!(
        lamp.back(update_unit("failed", "failed"), u64::MAX),
        own::Back::Nothing
    );
    assert_eq!(lamp.round(), Outcome::UpToDate(2), "{:?}", lamp.said);
    boundary(&lamp, OLD);
}
