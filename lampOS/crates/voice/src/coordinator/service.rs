//! The outer service owns no audio. Only a verified physical/software mute, or
//! on the Controls route a switch that cannot be read (closed, #795), may
//! create another runtime; faults and cleanup failures never become retries.
use super::*;
use crate::service_privacy::{PhysicalMute, SoftwareMute, WaitOutcome, is_unavailable};

pub(super) fn supervise(
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    wait: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<WaitOutcome>,
    active: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<()>,
) -> Result<()> {
    let progress = crate::onboarding::switched_on(
        std::env::var(crate::onboarding::ENVIRONMENT)
            .ok()
            .as_deref(),
        std::env::var(crate::onboarding::FILE_ENVIRONMENT)
            .ok()
            .as_deref(),
    );
    let paths = progress.map(|progress| crate::onboarding::storage::Paths {
        checkpoint: progress
            .parent()
            .expect("absolute onboarding path")
            .join("privacy-pending.json"),
        mailbox: crate::onboarding::mailbox_of(&progress),
        progress,
        // Fourth's consumer reads this fixed path (#753); only setup writes it.
        near_call: PathBuf::from(lamp_near_call::FILE),
        room_sounds: crate::onboarding::ROOM_SOUNDS_PATH.into(),
        room_sounds_group: room_sounds_group(),
    });
    supervise_with_installed_skill(
        trace,
        cues,
        termination,
        paths.as_ref(),
        Path::new(crate::skill_words::INSTALLED),
        wait,
        active,
    )
}

fn supervise_with_installed_skill(
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    paths: Option<&crate::onboarding::storage::Paths>,
    skills: &Path,
    wait: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<WaitOutcome>,
    active: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<()>,
) -> Result<()> {
    // Skill-only installs do not restart Voice. A new service invocation is
    // the activation boundary; never read or reload skill data inside a cycle.
    trace.onboarding_definition =
        paths.and_then(|_| match crate::onboarding::skill::read_installed(skills) {
            Ok(definition) => {
                eprintln!("lamp-voice: onboarding skill loaded for this service invocation");
                Some(std::sync::Arc::new(definition))
            }
            Err(_) => {
                eprintln!(
                    "lamp-voice: onboarding skill unavailable; walk disabled until Voice restarts"
                );
                None
            }
        });
    supervise_with_checkpoint(trace, cues, termination, paths, wait, active)
}

/// [`crate::onboarding::ROOM_SOUNDS_GROUP`]'s id. Not known: the yes is
/// root's alone (0600), which lamp-ears reads as no, said once here.
fn room_sounds_group() -> Option<u32> {
    let name = crate::onboarding::ROOM_SOUNDS_GROUP;
    let id = std::fs::read_to_string("/etc/group")
        .ok()
        .and_then(|groups| crate::say::group_id(&groups, name));
    if id.is_none() {
        eprintln!("lamp-voice: the group {name:?} is not known; a room sounds yes is root's alone");
    }
    id
}

pub(super) fn storage_failed(trace: &mut SessionState, error: impl std::fmt::Display) {
    if !trace.onboarding_privacy_disabled {
        let _ = record(
            trace,
            json!({"kind":"onboarding_privacy_incomplete","at_us":monotonic_us(),
            "reason":"storage_failed","error":error.to_string()}),
        );
    }
    let epoch = trace
        .onboarding_storage
        .as_ref()
        .and_then(|store| store.client().snapshot().ok())
        .map_or(u64::MAX, |snapshot| snapshot.epoch);
    trace.onboarding_storage_failed_epoch = Some(
        trace
            .onboarding_storage_failed_epoch
            .map_or(epoch, |prior| prior.max(epoch)),
    );
    trace.onboarding_privacy_disabled = true;
}
pub(super) fn poll_storage(trace: &mut SessionState) {
    let result = trace
        .onboarding_storage
        .as_ref()
        .map(|store| store.client().snapshot());
    match result {
        Some(Ok(snapshot)) if snapshot.failed.is_some() => {
            storage_failed(trace, snapshot.failed.unwrap())
        }
        Some(Ok(snapshot))
            if snapshot.ready
                && snapshot.failed.is_none()
                && trace
                    .onboarding_storage_failed_epoch
                    .is_some_and(|failed| snapshot.epoch > failed) =>
        {
            trace.onboarding_privacy_disabled = false;
            trace.onboarding_storage_failed_epoch = None;
        }
        Some(Err(error)) => storage_failed(trace, error),
        _ => {}
    }
}
fn discard_onboarding_checkpoint(trace: &mut SessionState, _checkpoint: Option<&Path>, why: &str) {
    if let Some(storage) = trace.onboarding_storage.as_ref() {
        let result = storage.client().fence(None);
        match result {
            Ok(tag) => {
                let _ = record(
                    trace,
                    json!({"kind":"onboarding_privacy_checkpoint","state":"queued",
                "operation":"discarded","boundary":why,"epoch":tag.epoch,"revision":tag.revision,"at_us":monotonic_us()}),
                );
            }
            Err(error) => storage_failed(trace, error),
        }
    }
}

fn supervise_with_checkpoint(
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    paths: Option<&crate::onboarding::storage::Paths>,
    wait: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<WaitOutcome>,
    active: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<()>,
) -> Result<()> {
    if let Some(paths) = paths {
        if trace.onboarding_storage.is_some() {
            return Err(io::Error::other("onboarding storage owner already exists").into());
        }
        match crate::onboarding::storage::ServiceStorage::start(paths.clone()) {
            Ok(storage) => trace.onboarding_storage = Some(storage),
            Err(error) => storage_failed(trace, error),
        }
    }
    let result = run_service_loop(
        trace,
        cues,
        termination,
        paths.map(|paths| paths.checkpoint.as_path()),
        wait,
        active,
    );
    let shutdown = if let Some(storage) = trace.onboarding_storage.as_mut() {
        match storage.finish(Duration::from_millis(500)) {
            Ok(crate::onboarding::storage::StorageShutdown::Finished) => Ok(()),
            Ok(crate::onboarding::storage::StorageShutdown::Pending) => Err(io::Error::other(
                "onboarding storage still finishing; no replacement service in this process",
            )),
            Err(error) => Err(error),
        }
    } else {
        Ok(())
    };
    crate::service_privacy::finish(result, shutdown)
}
fn run_service_loop(
    trace: &mut SessionState,
    cues: &mut Option<CueSink>,
    termination: &Termination,
    checkpoint: Option<&Path>,
    mut wait: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<WaitOutcome>,
    mut active: impl FnMut(&mut SessionState, &mut Option<CueSink>) -> Result<()>,
) -> Result<()> {
    let mut cycle = 0u64;
    loop {
        poll_storage(trace);
        if termination.signal_name().is_some() {
            return Ok(());
        }
        record(
            trace,
            json!({"kind":"service_waiting_for_privacy","at_us":monotonic_us(),"next_cycle":cycle.checked_add(1),"audio_workers_open":false}),
        )?;
        flush_cues(trace, cues);
        match wait(trace, cues)? {
            WaitOutcome::Stopped => return Ok(()),
            WaitOutcome::Allowed => {}
        }
        if termination.signal_name().is_some() {
            return Ok(());
        }
        poll_storage(trace);
        cycle = cycle
            .checked_add(1)
            .ok_or_else(|| io::Error::other("service cycle overflow"))?;
        // The previous rig has been closed and reaped. New boots cannot settle
        // old commands. One logger/queue remains for the whole service invocation.
        trace.receipts = Default::default();
        record(
            trace,
            json!({"kind":"service_cycle_start","at_us":monotonic_us(),"cycle":cycle,"fresh_runtime_required":true,"context_continuity_claimed":false}),
        )?;
        match active(trace, cues) {
            Ok(()) => return Ok(()),
            Err(error)
                if error.downcast_ref::<PhysicalMute>().is_some()
                    || error.downcast_ref::<SoftwareMute>().is_some() =>
            {
                // This branch is reachable only after verified runtime cleanup:
                // service_privacy::finish strips the mute token on cleanup fault.
                // Persist optional onboarding evidence here, never ahead of revoke.
                if let Some(checkpoint) = error
                    .downcast_ref::<PhysicalMute>()
                    .and_then(|mute| mute.onboarding.as_ref())
                    && let Some(storage) = trace.onboarding_storage.as_ref()
                {
                    let operation = if checkpoint.is_discard() {
                        "discarded"
                    } else {
                        "closed"
                    };
                    match storage.client().persist_closed(checkpoint.clone()) {
                        Ok(crate::onboarding::storage::Submit::Queued(tag)) => {
                            let _ = record(
                                trace,
                                json!({"kind":"onboarding_privacy_checkpoint","state":"queued",
                                    "operation":operation,"epoch":tag.epoch,"revision":tag.revision,
                                    "at_us":monotonic_us(),"cleanup_verified":true,"audio_workers_open":false}),
                            );
                        }
                        Ok(_) => storage_failed(
                            trace,
                            "onboarding checkpoint not accepted by storage owner",
                        ),
                        Err(error) => storage_failed(trace, error),
                    }
                }
                if error.downcast_ref::<SoftwareMute>().is_some() {
                    discard_onboarding_checkpoint(
                        trace,
                        checkpoint,
                        "verified_software_mute_cleanup",
                    );
                }
                let (reason, observed_at_us) =
                    if let Some(mute) = error.downcast_ref::<SoftwareMute>() {
                        let reason = if mute.asleep {
                            "head_sleep"
                        } else {
                            "software_muted"
                        };
                        (reason, mute.observed_at_us)
                    } else {
                        (
                            "physical_muted",
                            error
                                .downcast_ref::<PhysicalMute>()
                                .expect("matched a physical or software mute")
                                .observed_at_us,
                        )
                    };
                record(
                    trace,
                    json!({"kind":"service_cycle_muted","at_us":monotonic_us(),"cycle":cycle,"reason":reason,"observed_at_us":observed_at_us,"cleanup_verified":true}),
                )?;
                flush_cues(trace, cues);
            }
            // The switch could not be read: the microphones are closed and
            // the wait reads it again. A failed cleanup is never this.
            Err(error) if is_unavailable(&*error) => {
                discard_onboarding_checkpoint(
                    trace,
                    checkpoint,
                    "verified_privacy_unavailable_cleanup",
                );
                eprintln!("lamp-voice: microphones closed until the switch can be read: {error}");
                record(
                    trace,
                    json!({"kind":"service_cycle_muted","at_us":monotonic_us(),"cycle":cycle,"reason":"privacy_unreadable","why":error.to_string(),"cleanup_verified":true}),
                )?;
                flush_cues(trace, cues);
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn test_paths(checkpoint: &Path) -> crate::onboarding::storage::Paths {
        crate::onboarding::storage::Paths {
            progress: checkpoint.with_file_name("progress.json"),
            mailbox: checkpoint.with_file_name("progress.told"),
            near_call: checkpoint.with_file_name("near-call.json"),
            room_sounds: checkpoint.with_file_name("room-sounds.json"),
            room_sounds_group: None,
            checkpoint: checkpoint.to_path_buf(),
        }
    }
    /// Until the storage is idle, its last outcome taken in. Idle is read
    /// first and the outcome after it: the other order missed a failure
    /// that finished between the two snapshots, so the flag it sets was
    /// never set (#1569 row 55).
    fn storage_idle(trace: &mut SessionState) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let done = trace
                .onboarding_storage
                .as_ref()
                .is_none_or(|store| !store.client().snapshot().unwrap().busy);
            poll_storage(trace);
            if done {
                break;
            }
            assert!(Instant::now() < until, "storage fixture timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn pending_privacy_checkpoint(path: PathBuf) -> crate::onboarding::privacy::Checkpoint {
        use crate::onboarding::privacy::{Acquisition, Attempt, Checkpoint, Epoch, Invitation};
        use lamp_msgs::{ClockStamp, PrivacySwitch};
        let epoch = Epoch {
            device_boot: "host-boot".into(),
            clock_id: "host-clock".into(),
        };
        let sample = |sequence, at_us, muted| Acquisition {
            controls_session: "0123456789abcdef".into(),
            physical: PrivacySwitch {
                stamp: ClockStamp {
                    clock_id: "host-clock".into(),
                    at_us,
                    sequence,
                },
                reader_session_id: "reader".into(),
                muted: Some(muted),
                fault: None,
            },
        };
        let mut attempt = Attempt::arm(
            epoch,
            Invitation {
                voice_boot: BootId::new([1; 16]).unwrap(),
                turn: 1,
                delivered_at_us: 100,
            },
            sample(1, 100, false),
            100,
        )
        .unwrap();
        attempt.observe_close(sample(2, 200, true), 200).unwrap();
        Checkpoint::new(attempt, path)
    }

    #[test]
    fn onboarding_skill_snapshot_survives_mute_reopen_and_recovers_only_in_a_new_service() {
        use crate::onboarding::{Framing, Onboarding};
        let directory = SessionDirectory::create().unwrap();
        let skills = directory.path.join("skills");
        let installed = skills.join("onboarding/skill.toml");
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        let shipped = include_str!("../../../../skills/onboarding/skill.toml");
        let changed = shipped.replace("Say hello in a few warm words", "Say a short hello");
        let expected = crate::onboarding::skill::parse(shipped)
            .unwrap()
            .note(crate::onboarding::Step::WhereYouSit, false)
            .to_owned();
        let revised = crate::onboarding::skill::parse(&changed)
            .unwrap()
            .note(crate::onboarding::Step::WhereYouSit, false)
            .to_owned();
        let face = Some(Framing {
            size: 0.2,
            x: 0.0,
            y: 0.0,
        });
        for initially_present in [true, false] {
            if installed.exists() {
                fs::remove_file(&installed).unwrap();
            }
            if initially_present {
                fs::write(&installed, shipped).unwrap();
            }
            let paths = test_paths(&directory.path.join(if initially_present {
                "present.json"
            } else {
                "missing.json"
            }));
            let cycles = Cell::new(0);
            let mut trace = SessionState::default();
            let began = Instant::now();
            let mut startup = None;
            supervise_with_installed_skill(
                &mut trace,
                &mut None,
                &Termination::inactive(),
                Some(&paths),
                &skills,
                |trace, _| {
                    storage_idle(trace);
                    Ok(WaitOutcome::Allowed)
                },
                |trace, _| {
                    startup.get_or_insert_with(|| began.elapsed());
                    cycles.set(cycles.get() + 1);
                    assert!(
                        trace.onboarding_storage.is_some(),
                        "skill failure must not disable physical privacy storage"
                    );
                    assert_eq!(trace.onboarding_definition.is_some(), initially_present);
                    if let Some(definition) = &trace.onboarding_definition {
                        let mut walk = Onboarding::with_definition(
                            paths.progress.clone(),
                            trace.onboarding_storage.as_ref().unwrap().client(),
                            definition.clone(),
                        );
                        walk.drain_storage_for_test(4_000_000);
                        assert_eq!(
                            walk.tick(4_000_000, 4_000_000, true, false, face, true),
                            Some(expected.clone())
                        );
                    }
                    if cycles.get() == 1 {
                        fs::write(&installed, &changed).unwrap();
                        Err(Box::new(SoftwareMute {
                            observed_at_us: 5_000_000,
                            asleep: false,
                        }))
                    } else {
                        // Updates and recovery during mute are not read on reopen.
                        Ok(())
                    }
                },
            )
            .unwrap();
            assert_eq!(cycles.get(), 2);
            assert_eq!(
                trace
                    .iter()
                    .filter(|row| row["kind"] == "service_cycle_start")
                    .count(),
                2
            );
            let mut fresh = SessionState::default();
            supervise_with_installed_skill(
                &mut fresh,
                &mut None,
                &Termination::inactive(),
                Some(&paths),
                &skills,
                |trace, _| {
                    storage_idle(trace);
                    Ok(WaitOutcome::Allowed)
                },
                |trace, _| {
                    let definition = trace.onboarding_definition.as_ref().unwrap();
                    let mut walk = Onboarding::with_definition(
                        paths.progress.clone(),
                        trace.onboarding_storage.as_ref().unwrap().client(),
                        definition.clone(),
                    );
                    walk.drain_storage_for_test(4_000_000);
                    assert_eq!(
                        walk.tick(4_000_000, 4_000_000, true, false, face, true),
                        Some(revised.clone())
                    );
                    Ok(())
                },
            )
            .unwrap();
            eprintln!(
                "onboarding service timing: initially_present={initially_present} startup_to_active_us={}",
                startup.unwrap().as_micros()
            );
        }
    }

    #[test]
    fn physical_checkpoint_is_persisted_only_after_verified_cleanup() {
        for verified in [false, true] {
            let directory = SessionDirectory::create().unwrap();
            let path = directory.path.join("privacy-attempt.json");
            let waits = Cell::new(0);
            let mut checkpoint = Some(pending_privacy_checkpoint(path.clone()));
            let mut trace = SessionState::default();
            let result = supervise_with_checkpoint(
                &mut trace,
                &mut None,
                &Termination::inactive(),
                Some(&test_paths(&path)),
                |trace, _| {
                    storage_idle(trace);
                    waits.set(waits.get() + 1);
                    if waits.get() == 1 {
                        assert!(
                            !path.exists(),
                            "close observation performed I/O before cleanup"
                        );
                        Ok(WaitOutcome::Allowed)
                    } else {
                        assert!(path.is_file());
                        Ok(WaitOutcome::Stopped)
                    }
                },
                |_, _| {
                    let mute = PhysicalMute {
                        observed_at_us: 200,
                        onboarding: checkpoint.take(),
                    };
                    crate::service_privacy::finish(
                        Err(Box::new(mute)),
                        if verified {
                            Ok(())
                        } else {
                            Err(io::Error::other("fixture cleanup failed"))
                        },
                    )
                },
            );
            assert_eq!(result.is_ok(), verified);
            assert_eq!(path.is_file(), verified);
            assert_eq!(waits.get(), if verified { 2 } else { 1 });
            assert_eq!(
                trace
                    .iter()
                    .filter(|row| row["kind"] == "onboarding_privacy_checkpoint")
                    .count(),
                usize::from(verified)
            );
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn checkpoint_storage_failure_keeps_the_normal_no_audio_privacy_wait() {
        let directory = SessionDirectory::create().unwrap();
        let path = directory.path.join("missing-parent/pending.json");
        let mut checkpoint = Some(pending_privacy_checkpoint(path.clone()));
        let waits = Cell::new(0);
        let mut trace = SessionState::default();
        let result = supervise_with_checkpoint(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            Some(&test_paths(&path)),
            |trace, _| {
                storage_idle(trace);
                waits.set(waits.get() + 1);
                Ok(if waits.get() == 1 {
                    WaitOutcome::Allowed
                } else {
                    WaitOutcome::Stopped
                })
            },
            |_, _| {
                Err(Box::new(PhysicalMute {
                    observed_at_us: 200,
                    onboarding: checkpoint.take(),
                }))
            },
        );
        assert!(result.is_ok());
        assert_eq!(waits.get(), 2);
        let event = trace
            .iter()
            .find(|row| row["kind"] == "onboarding_privacy_incomplete")
            .unwrap();
        assert!(trace.onboarding_privacy_disabled);
        assert!(event["error"].is_string());
        assert_eq!(
            trace
                .iter()
                .filter(|row| row["kind"] == "service_cycle_start")
                .count(),
            1
        );
    }

    #[test]
    fn software_mute_and_unreadable_cycles_discard_old_closed_proof() {
        use crate::onboarding::privacy::{Attempt, Epoch, Phase};
        for unreadable in [false, true] {
            let directory = SessionDirectory::create().unwrap();
            let path = directory.path.join("pending.json");
            let epoch = Epoch {
                device_boot: "host-boot".into(),
                clock_id: "host-clock".into(),
            };
            let waits = Cell::new(0);
            let cycles = Cell::new(0);
            let mut candidate = Some(pending_privacy_checkpoint(path.clone()));
            let mut trace = SessionState::default();
            supervise_with_checkpoint(
                &mut trace,
                &mut None,
                &Termination::inactive(),
                Some(&test_paths(&path)),
                |trace, _| {
                    storage_idle(trace);
                    waits.set(waits.get() + 1);
                    match waits.get() {
                        1 => assert!(!path.exists()),
                        2 => assert_eq!(
                            Attempt::load(&path, &epoch, 300).unwrap().unwrap().phase(),
                            Phase::Closed
                        ),
                        _ => assert!(
                            Attempt::load(&path, &epoch, 400).unwrap().is_none(),
                            "later open cannot recover Done from invalidated proof"
                        ),
                    }
                    Ok(if waits.get() < 3 {
                        WaitOutcome::Allowed
                    } else {
                        WaitOutcome::Stopped
                    })
                },
                |_, _| {
                    cycles.set(cycles.get() + 1);
                    if cycles.get() == 1 {
                        Err(Box::new(PhysicalMute {
                            observed_at_us: 200,
                            onboarding: candidate.take(),
                        }))
                    } else if unreadable {
                        Err(crate::service_privacy::unavailable(
                            "fixture physical source lost",
                        ))
                    } else {
                        Err(Box::new(SoftwareMute {
                            observed_at_us: 300,
                            asleep: false,
                        }))
                    }
                },
            )
            .unwrap();
            assert_eq!(cycles.get(), 2);
            assert_eq!(waits.get(), 3);
            assert!(!trace.onboarding_privacy_disabled);
        }
    }

    #[test]
    fn explicit_discard_survives_physical_close_before_idle_maintenance() {
        use crate::onboarding::privacy::{Attempt, Checkpoint, Epoch, Phase};
        let directory = SessionDirectory::create().unwrap();
        let path = directory.path.join("pending.json");
        let epoch = Epoch {
            device_boot: "host-boot".into(),
            clock_id: "host-clock".into(),
        };
        let mut first = Some(pending_privacy_checkpoint(path.clone()));
        let waits = Cell::new(0);
        let cycles = Cell::new(0);
        let mut trace = SessionState::default();
        supervise_with_checkpoint(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            Some(&test_paths(&path)),
            |trace, _| {
                storage_idle(trace);
                waits.set(waits.get() + 1);
                if waits.get() == 2 {
                    assert_eq!(
                        Attempt::load(&path, &epoch, 300).unwrap().unwrap().phase(),
                        Phase::Closed
                    );
                } else if waits.get() == 3 {
                    assert!(Attempt::load(&path, &epoch, 400).unwrap().is_none());
                }
                Ok(if waits.get() < 3 {
                    WaitOutcome::Allowed
                } else {
                    WaitOutcome::Stopped
                })
            },
            |_, _| {
                cycles.set(cycles.get() + 1);
                // The wrapper explicitly requests discard when pause/skip already
                // invalidated an attempt before another physical close.
                let onboarding = if cycles.get() == 1 {
                    first.take()
                } else {
                    Some(Checkpoint::discard(path.clone()))
                };
                Err(Box::new(PhysicalMute {
                    observed_at_us: 200,
                    onboarding,
                }))
            },
        )
        .unwrap();
        let states: Vec<_> = trace
            .iter()
            .filter(|row| row["kind"] == "onboarding_privacy_checkpoint")
            .map(|row| row["operation"].clone())
            .collect();
        assert_eq!(states, [json!("closed"), json!("discarded")]);
        assert!(!trace.onboarding_privacy_disabled);
    }

    #[test]
    fn a_new_service_discards_closed_proof_before_its_first_wait() {
        let directory = SessionDirectory::create().unwrap();
        let path = directory.path.join("pending.json");
        pending_privacy_checkpoint(path.clone())
            .persist_after_cleanup()
            .unwrap();
        assert!(path.is_file());
        let mut trace = SessionState::default();
        supervise_with_checkpoint(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            Some(&test_paths(&path)),
            |trace, _| {
                storage_idle(trace);
                assert!(!path.exists());
                Ok(WaitOutcome::Stopped)
            },
            |_, _| panic!("no active runtime requested"),
        )
        .unwrap();
        assert!(!trace.onboarding_privacy_disabled);
    }

    #[test]
    fn failed_invalidation_disables_only_this_services_onboarding() {
        let directory = SessionDirectory::create().unwrap();
        let path = directory.path.join("pending.json");
        fs::create_dir(&path).unwrap();
        let mut failed = SessionState::default();
        supervise_with_checkpoint(
            &mut failed,
            &mut None,
            &Termination::inactive(),
            Some(&test_paths(&path)),
            |trace, _| {
                storage_idle(trace);
                assert!(trace.onboarding_privacy_disabled);
                Ok(WaitOutcome::Stopped)
            },
            |_, _| panic!("no active runtime requested"),
        )
        .unwrap();
        assert!(failed.iter().any(|row| row["reason"] == "storage_failed"));
        let mut fresh = SessionState::default();
        supervise_with_checkpoint(
            &mut fresh,
            &mut None,
            &Termination::inactive(),
            None,
            |trace, _| {
                assert!(!trace.onboarding_privacy_disabled);
                Ok(WaitOutcome::Stopped)
            },
            |_, _| panic!("no active runtime requested"),
        )
        .unwrap();
        fs::remove_dir(path).unwrap();
    }

    #[test]
    fn old_ready_snapshot_cannot_clear_a_failure_until_a_new_barrier_is_acknowledged() {
        let directory = SessionDirectory::create().unwrap();
        let paths = test_paths(&directory.path.join("pending.json"));
        let mut trace = SessionState::default();
        trace.onboarding_storage =
            Some(crate::onboarding::storage::ServiceStorage::start(paths).unwrap());
        storage_idle(&mut trace);
        storage_failed(&mut trace, "fixture submission failure");
        poll_storage(&mut trace);
        assert!(trace.onboarding_privacy_disabled);
        let client = trace.onboarding_storage.as_ref().unwrap().client();
        client
            .commit(crate::onboarding::Progress::default(), false)
            .unwrap();
        storage_idle(&mut trace);
        assert!(
            trace.onboarding_privacy_disabled,
            "ordinary same-epoch completion is not recovery"
        );
        client
            .fence(Some(crate::onboarding::Progress::default()))
            .unwrap();
        storage_idle(&mut trace);
        assert!(!trace.onboarding_privacy_disabled);
        assert_eq!(trace.onboarding_storage_failed_epoch, None);
        trace
            .onboarding_storage
            .as_mut()
            .unwrap()
            .finish(Duration::from_secs(2))
            .unwrap();
    }

    /// The first builder on #795 (13:50): with the Controls privacy route
    /// on, the red breath is the hardware switch's and the asleep face stays
    /// off, so the two never write the ring at once.
    #[test]
    fn the_asleep_face_stays_off_with_the_controls_privacy_route() {
        assert!(super::super::asleep_face_allowed(true, false));
        assert!(!super::super::asleep_face_allowed(true, true));
        assert!(!super::super::asleep_face_allowed(false, false));
        assert!(!super::super::asleep_face_allowed(false, true));
    }

    /// A lamp with no slide (`LAMP_PRIVACY_CONTROLS=no-slide`): its wait
    /// paints no physical-mute indication, so its ring is as with the route
    /// off: the asleep face, the tap face and the reminder pulse all show.
    /// With a slide (`on`) they stay off, as before.
    #[test]
    fn the_ring_of_a_lamp_with_no_slide_is_as_with_the_route_off() {
        use super::super::{
            asleep_face_allowed, privacy_ring, pulse_without_face, tap_face_allowed,
        };
        use crate::privacy_bus::Route;
        assert!(!privacy_ring(None));
        assert!(privacy_ring(Some(Route::Slide)));
        assert!(!privacy_ring(Some(Route::NoSlide)));
        let no_slide = privacy_ring(Some(Route::NoSlide));
        assert!(asleep_face_allowed(true, no_slide));
        assert!(tap_face_allowed(true, false, no_slide));
        assert_eq!(pulse_without_face(true, true, true, no_slide), None);
        let slide = privacy_ring(Some(Route::Slide));
        assert!(!asleep_face_allowed(true, slide));
        assert!(!tap_face_allowed(true, false, slide));
        assert!(pulse_without_face(true, true, true, slide).is_some());
    }

    #[test]
    fn verified_mutes_repeat_the_privacy_wait_and_record_their_actual_source() {
        // Physical, software, and software because the head put it to
        // sleep (#711).
        for (software, asleep) in [(false, false), (true, false), (true, true)] {
            let mut trace = SessionState::default();
            let waits = Cell::new(0);
            let active = Cell::new(0);
            supervise(
                &mut trace,
                &mut None,
                &Termination::inactive(),
                |_, _| {
                    waits.set(waits.get() + 1);
                    assert_eq!(waits.get(), active.get() + 1);
                    Ok(WaitOutcome::Allowed)
                },
                |_, _| {
                    active.set(active.get() + 1);
                    if active.get() == 1 {
                        let mute: Box<dyn std::error::Error + Send + Sync> = if software {
                            Box::new(SoftwareMute {
                                observed_at_us: 71,
                                asleep,
                            })
                        } else {
                            Box::new(PhysicalMute {
                                observed_at_us: 71,
                                onboarding: None,
                            })
                        };
                        crate::service_privacy::finish(Err(mute), Ok(()))
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap();
            assert_eq!((waits.get(), active.get()), (2, 2));
            let muted: Vec<_> = trace
                .iter()
                .filter(|row| row["kind"] == "service_cycle_muted")
                .collect();
            assert_eq!(muted.len(), 1);
            assert_eq!(
                muted[0]["reason"],
                match (software, asleep) {
                    (true, true) => "head_sleep",
                    (true, false) => "software_muted",
                    _ => "physical_muted",
                }
            );
            assert_eq!(muted[0]["observed_at_us"], 71);
            assert_eq!(muted[0]["cleanup_verified"], true);
        }
    }

    /// lamp-0c89's update round, 21 Oct 13:13 (#1396): Controls swapped
    /// mid-session. The conversation ends on its lapsed freshness, and the
    /// next wait's privacy worker ends at its start, before the new
    /// Controls reads the switch. The voice stays, the microphones closed,
    /// reads the switch again, and opens on the next fresh read.
    #[test]
    fn a_controls_swap_mid_session_closes_the_microphones_and_never_ends_the_voice() {
        let mut ended = Some(a_privacy_worker_ended("exit 0"));
        let mut trace = SessionState::default();
        let reads = Cell::new(0);
        let active = Cell::new(0);
        supervise(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            |_, _| {
                crate::service_privacy_controls::unreadable_retried(
                    &Termination::inactive(),
                    Duration::from_millis(1),
                    || {
                        reads.set(reads.get() + 1);
                        // The second wait's first read: its worker ended.
                        let error = if reads.get() == 2 { ended.take() } else { None };
                        match error {
                            Some(error) => crate::service_privacy_controls::started(Err(error))
                                .map(|_| WaitOutcome::Allowed),
                            None => Ok(WaitOutcome::Allowed),
                        }
                    },
                    |_| Ok(()),
                )
            },
            |_, _| {
                active.set(active.get() + 1);
                if active.get() == 1 {
                    crate::service_privacy::finish(
                        Err(crate::service_privacy::unavailable(
                            "service privacy freshness lost before next observation",
                        )),
                        Ok(()),
                    )
                } else {
                    Ok(())
                }
            },
        )
        .unwrap();
        // Three reads: the first wait, the ended worker, the fresh one; and
        // the conversation again after it.
        assert_eq!((reads.get(), active.get()), (3, 2));
        assert!(ended.is_none(), "the ended worker was read");
        let muted: Vec<_> = trace
            .iter()
            .filter(|row| row["kind"] == "service_cycle_muted")
            .collect();
        assert_eq!(muted.len(), 1);
        assert_eq!(muted[0]["reason"], "privacy_unreadable");
    }

    use crate::service_privacy_controls::tests::a_privacy_worker_ended;

    /// Seventh on #1396: the conversation's own privacy worker ending
    /// cleanly at its start, on the Controls route, ends the conversation as
    /// an unreadable switch, so the voice goes back to the wait with the
    /// microphones closed and doesn't exit. On the direct GPIO route, for a
    /// worker that crashed and for another start failure, the error is as
    /// it was (the reviewer-lead's P2 on #2506).
    #[test]
    fn a_conversations_privacy_worker_ended_at_its_start_goes_back_to_the_wait() {
        let started = |error, controls| privacy_started(Err(error), controls).err().unwrap();
        assert!(is_unavailable(&started(
            a_privacy_worker_ended("exit 0"),
            true
        )));
        assert!(!is_unavailable(&started(
            a_privacy_worker_ended("exit 0"),
            false
        )));
        assert!(!is_unavailable(&started(
            a_privacy_worker_ended("exit 101"),
            true
        )));
        assert!(!is_unavailable(&started(
            a_privacy_worker_ended("kill -SEGV $$"),
            true
        )));
        let timed_out = io::Error::new(io::ErrorKind::TimedOut, "worker readiness timed out");
        assert!(!is_unavailable(&started(timed_out, true)));
        let mut trace = SessionState::default();
        let waits = Cell::new(0);
        let active = Cell::new(0);
        let mut ended = Some(a_privacy_worker_ended("exit 0"));
        supervise(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            |_, _| {
                waits.set(waits.get() + 1);
                Ok(WaitOutcome::Allowed)
            },
            |_, _| {
                active.set(active.get() + 1);
                match ended.take() {
                    Some(error) => Err(started(error, true).into()),
                    None => Ok(()),
                }
            },
        )
        .unwrap();
        assert_eq!((waits.get(), active.get()), (2, 2));
        assert!(trace.iter().any(
            |row| row["kind"] == "service_cycle_muted" && row["reason"] == "privacy_unreadable"
        ));
    }

    /// #795: on the Controls route a switch that cannot be read closes the
    /// cycle; the service waits and reads again instead of ending, and says
    /// why. With its cleanup failed it still ends.
    #[test]
    fn an_unreadable_switch_repeats_the_wait_unless_its_cleanup_failed() {
        let mut trace = SessionState::default();
        let waits = Cell::new(0);
        let active = Cell::new(0);
        supervise(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            |_, _| {
                waits.set(waits.get() + 1);
                Ok(WaitOutcome::Allowed)
            },
            |_, _| {
                active.set(active.get() + 1);
                if active.get() == 1 {
                    crate::service_privacy::finish(
                        Err(crate::service_privacy::unavailable(
                            "physical privacy closed or lost freshness",
                        )),
                        Ok(()),
                    )
                } else {
                    Ok(())
                }
            },
        )
        .unwrap();
        assert_eq!((waits.get(), active.get()), (2, 2));
        let muted: Vec<_> = trace
            .iter()
            .filter(|row| row["kind"] == "service_cycle_muted")
            .collect();
        assert_eq!(muted.len(), 1);
        assert_eq!(muted[0]["reason"], "privacy_unreadable");
        let error = supervise(
            &mut SessionState::default(),
            &mut None,
            &Termination::inactive(),
            |_, _| Ok(WaitOutcome::Allowed),
            |_, _| {
                crate::service_privacy::finish(
                    Err(crate::service_privacy::unavailable("no switch")),
                    Err(io::Error::other("capture still open")),
                )
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("verified cleanup failed"));
    }

    #[test]
    fn software_mute_with_failed_cleanup_never_starts_a_second_wait() {
        let mut trace = SessionState::default();
        let waits = Cell::new(0);
        let active = Cell::new(0);
        let error = supervise(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            |_, _| {
                waits.set(waits.get() + 1);
                assert_eq!(waits.get(), 1, "failed cleanup must not be retried");
                Ok(WaitOutcome::Allowed)
            },
            |_, _| {
                active.set(active.get() + 1);
                crate::service_privacy::finish(
                    Err(Box::new(SoftwareMute {
                        observed_at_us: 73,
                        asleep: false,
                    })),
                    Err(io::Error::other("synthetic cleanup failure")),
                )
            },
        )
        .unwrap_err();
        assert_eq!((waits.get(), active.get()), (1, 1));
        assert!(error.downcast_ref::<SoftwareMute>().is_none());
        assert!(error.to_string().contains("synthetic cleanup failure"));
        assert!(!trace.iter().any(|row| row["kind"] == "service_cycle_muted"));
    }
}
