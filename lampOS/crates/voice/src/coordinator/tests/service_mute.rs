//! Synthetic child/socket supervision only; no hardware or provider connection.
use super::*;
use crate::service_privacy::PhysicalMute;

#[test]
fn cleanup_failure_cannot_leave_a_restartable_physical_mute() {
    let directory = SessionDirectory::create().unwrap();
    let (speaker, _speaker_peer) = Worker::sleeping_fixture(&directory.path, "speaker");
    let (capture, _capture_peer) = Worker::sleeping_fixture(&directory.path, "capture");
    let (provider, _provider_peer) = Worker::sleeping_fixture(&directory.path, "provider");
    let (privacy, _privacy_peer) = Worker::sleeping_fixture(&directory.path, "privacy");
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        privacy_controls: false,
        ring: None.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::VadOnly,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
    };
    let (mut controller, current, _) = active_reply();
    let mut reply = Some(current);
    let mut trace = SessionState::default();
    let error = finish_active_run(
        &mut rig,
        &mut controller,
        &mut reply,
        &mut trace,
        &mut None,
        Err(Box::new(PhysicalMute {
            observed_at_us: monotonic_us(),
        })),
    )
    .unwrap_err();
    assert!(
        trace
            .iter()
            .any(|row| row["kind"] == "worker_forced_shutdown")
    );
    assert!(
        error.downcast_ref::<PhysicalMute>().is_none(),
        "failed cleanup retained restart token: {error}"
    );
    assert!(error.to_string().contains("physical mute"));
    assert!(error.to_string().contains("forced termination"));
    let waits = std::cell::Cell::new(0);
    let mut failure = Some(error);
    let restarted = crate::coordinator::service::supervise(
        &mut trace,
        &mut None,
        &Termination::inactive(),
        |_, _| {
            waits.set(waits.get() + 1);
            Ok(crate::service_privacy::WaitOutcome::Allowed)
        },
        |_, _| Err(failure.take().expect("must never retry failed cleanup")),
    );
    assert!(restarted.is_err());
    assert_eq!(waits.get(), 1);
    assert!(reply.is_none());
    assert!(controller.snapshot(now()).unwrap().owner().is_none());
    for worker in [
        &mut rig.speaker,
        &mut rig.capture,
        &mut rig.provider,
        &mut rig.privacy,
    ] {
        assert!(worker.try_exit().unwrap().is_some());
    }
}

use crate::service_privacy::{
    self, WaitOutcome,
    tests::{Script, mock},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

/// A fixture worker's side read on the stepped clock in [`active_mute`]: the
/// privacy peer says once, at the first authority, that the switch is muted
/// (stale when `invalid`).
struct MutePeer {
    channels: WorkerChannels,
    input: Option<std::process::ChildStdin>,
    pid: u32,
    privacy: bool,
    sent: bool,
    denied: bool,
    stopped: bool,
}

impl MutePeer {
    fn read(&mut self, invalid: bool, deadline: Instant) -> io::Result<()> {
        if self.stopped {
            return Ok(());
        }
        while let Some(message) = self.channels.control.receive::<Control>().unwrap() {
            match message {
                Control::Authority { snapshot } => {
                    self.denied |= snapshot.microphone_permission() == Permission::Denied;
                    if self.privacy && !self.sent {
                        let acquired_at_us = if invalid {
                            monotonic_us() - crate::privacy::PRIVACY_FRESH_US
                        } else {
                            monotonic_us()
                        };
                        self.channels
                            .control
                            .send(WorkerEvent::Privacy {
                                acquired_at_us,
                                muted: true,
                            })
                            .unwrap();
                        self.sent = true;
                    }
                }
                Control::Stop => {
                    self.stopped = true;
                    // The child exits once its input closes, and its exit is
                    // waited for here, bounded by the scope's own watchdog,
                    // so the cleanup that follows finds it gone.
                    drop(self.input.take());
                    let bound = crate::scenario_clock::host_deadline().unwrap_or(deadline);
                    return crate::scenario_clock::wait_child_exited(self.pid, bound);
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn active_mute(trace: &mut SessionState, invalid: bool) -> Result<()> {
    let directory = SessionDirectory::create().unwrap();
    // The stepped clock cannot rely on the scheduler running each child
    // before logical time advances: its fixture's echo proves it ready.
    let (speaker, speaker_peer, speaker_input) =
        Worker::cooperative_ready_fixture(&directory.path, "speaker");
    let (capture, capture_peer, capture_input) =
        Worker::cooperative_ready_fixture(&directory.path, "capture");
    let (provider, provider_peer, provider_input) =
        Worker::cooperative_ready_fixture(&directory.path, "provider");
    let (privacy, privacy_peer, privacy_input) =
        Worker::cooperative_ready_fixture(&directory.path, "privacy");
    let pids = [&speaker, &capture, &provider, &privacy].map(|worker| worker.fixture_pid());
    // Stepped from here, the owner made inside: each peer is read on this
    // thread before each step, as its thread read it on the host clock, and
    // a Stop closes its child's input and waits for its exit there. On the
    // host clock a starved peer thread could miss the 300 ms in which a
    // stopped worker must exit (#1232).
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(10)).unwrap();
    let peers = Rc::new(RefCell::new(
        [
            (speaker_peer, speaker_input, false),
            (capture_peer, capture_input, false),
            (provider_peer, provider_input, false),
            (privacy_peer, privacy_input, true),
        ]
        .into_iter()
        .zip(pids)
        .map(|((channels, input, privacy), pid)| MutePeer {
            channels,
            input: Some(input),
            pid,
            privacy,
            sent: false,
            denied: false,
            stopped: false,
        })
        .collect::<Vec<_>>(),
    ));
    let pumped = Rc::clone(&peers);
    clock
        .set_pump(move |phase| {
            // A real child's exit is not simulated: one deadline bounds the
            // children a step stops, and shutdown brings its own.
            let deadline = match phase {
                crate::scenario_clock::Phase::Run => Instant::now() + Duration::from_millis(300),
                crate::scenario_clock::Phase::Shutdown { deadline } => deadline,
            };
            for peer in pumped.borrow_mut().iter_mut() {
                peer.read(invalid, deadline)?;
            }
            Ok(())
        })
        .unwrap();
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        privacy_controls: false,
        ring: None.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::VadOnly,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
    };
    let (mut controller, current, permit) = active_reply();
    let mut reply = Some(current);
    let result = conversation(
        &mut rig,
        &mut controller,
        Lifetime::Serve,
        trace,
        StartupEvents::default(),
        &mut reply,
        &mut None,
        &Termination::inactive(),
        &FailureNotices::none(),
    );
    assert_eq!(
        result
            .as_ref()
            .unwrap_err()
            .downcast_ref::<PhysicalMute>()
            .is_some(),
        !invalid
    );
    let result = finish_active_run(
        &mut rig,
        &mut controller,
        &mut reply,
        trace,
        &mut None,
        result,
    );
    assert!(reply.is_none());
    let snapshot = controller.snapshot(now()).unwrap();
    let mut guard = BoundaryGuard::new(snapshot.boot(), now());
    guard.install(now(), snapshot).unwrap();
    assert!(guard.check(now(), permit, OutputKind::Speech).is_err());
    for peer in peers.take() {
        assert!(peer.stopped, "mock peer did not receive Stop");
        assert!(peer.denied);
    }
    for worker in [
        &mut rig.capture,
        &mut rig.speaker,
        &mut rig.provider,
        &mut rig.privacy,
    ] {
        assert!(worker.try_exit().unwrap().is_some());
    }
    drop(rig);
    drop(clock);
    fs::remove_dir(&directory.path).unwrap();
    result
}

#[test]
fn active_physical_mute_cancels_owned_reply_then_reaps_without_completion() {
    let mut trace = SessionState::default();
    let error = active_mute(&mut trace, false).unwrap_err();
    assert!(error.downcast_ref::<PhysicalMute>().is_some());
    assert!(
        trace
            .iter()
            .any(|row| row["kind"] == "turn_finished" && row["outcome"] == "privacy_muted")
    );
    assert!(
        !trace
            .iter()
            .any(|row| row["outcome"] == "audio_written_unscored")
    );
}

#[test]
fn stale_switch_during_active_run_is_never_a_restartable_mute() {
    let mut trace = SessionState::default();
    let error = active_mute(&mut trace, true).unwrap_err();
    assert!(error.downcast_ref::<PhysicalMute>().is_none());
    assert!(!trace.iter().any(|row| row["kind"] == "physical_mute"));
    assert!(trace.iter().any(|row| row["outcome"] == "runtime_failed"));
}

#[test]
fn repeated_verified_mute_cycles_keep_one_logger_and_never_open_audio_while_waiting() {
    let directory = SessionDirectory::create().unwrap();
    let output = directory.path.join("output");
    fs::create_dir(&output).unwrap();
    let mut trace = SessionState::service(&output);
    let active_count = Cell::new(0);
    let wait_count = Cell::new(0);
    let active_open = Cell::new(false);
    let mut boots = Vec::new();
    crate::coordinator::service::supervise(
        &mut trace,
        &mut None,
        &Termination::inactive(),
        |_, _| {
            assert!(!active_open.get());
            if wait_count.get() == 3 {
                return Ok(WaitOutcome::Stopped);
            }
            let scratch = SessionDirectory::create().unwrap();
            let seen = Arc::new(AtomicBool::new(false));
            let (worker, peer) = mock(&scratch.path, Script::Unmute, seen.clone());
            let boot = new_boot().unwrap();
            assert!(!boots.contains(&boot));
            boots.push(boot);
            let result = service_privacy::wait_worker(
                worker,
                boot,
                &Termination::inactive(),
                |state, _, _| {
                    assert!(!active_open.get());
                    assert_eq!(active_count.get(), wait_count.get());
                    if state == PrivacyState::Allowed {
                        assert!(seen.load(Ordering::SeqCst));
                    }
                    Ok(())
                },
            );
            assert!(peer.join().unwrap().received_stop);
            fs::remove_dir(&scratch.path).unwrap();
            wait_count.set(wait_count.get() + 1);
            result
        },
        |trace, _| {
            assert_eq!(active_count.get() + 1, wait_count.get());
            active_open.set(true);
            let result = active_mute(trace, false);
            active_open.set(false);
            active_count.set(active_count.get() + 1);
            result
        },
    )
    .unwrap();
    assert_eq!(active_count.get(), 3);
    assert_eq!(wait_count.get(), 3);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
    assert!(trace.finish_logging().unwrap().acknowledged);
    assert_eq!(fs::read_dir(output.join("metadata")).unwrap().count(), 5);
    fs::remove_dir_all(&output).unwrap();
}

#[test]
fn unavailable_startup_or_failed_cleanup_never_calls_next_runtime() {
    for cleanup in [false, true] {
        let mut trace = SessionState::default();
        let opened = Cell::new(0);
        let result = crate::coordinator::service::supervise(
            &mut trace,
            &mut None,
            &Termination::inactive(),
            |_, _| {
                if cleanup {
                    service_privacy::finish(
                        Ok(WaitOutcome::Allowed),
                        Err(io::Error::other("forced termination")),
                    )
                } else {
                    Err(io::Error::other("privacy unavailable").into())
                }
            },
            |_, _| {
                opened.set(opened.get() + 1);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(opened.get(), 0);
    }
}

/// One active rig's shutdown after the cycle ended as an unreadable switch,
/// with the privacy worker having queued `fault` (if any) and ended by
/// itself: cleanly, or killed.
/// With `aged`, the queued fault has aged out before the cycle closes, as
/// after a stall: its receive is an error, not the fault.
fn shutdown_after_unreadable(
    privacy_controls: bool,
    fault: Option<&str>,
    killed: bool,
    aged: bool,
) -> Result<()> {
    let directory = SessionDirectory::create().unwrap();
    let mut peers = Vec::new();
    let mut fixture = |role: &str| {
        let (mut worker, mut channels, input) = Worker::cooperative_fixture(&directory.path, role);
        worker.end_on_stop(input);
        peers.push(thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !matches!(
                channels.control.receive::<Control>(),
                Ok(Some(Control::Stop))
            ) {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            channels
        }));
        worker
    };
    let (speaker, capture, provider) =
        (fixture("speaker"), fixture("capture"), fixture("provider"));
    // The privacy worker said why and ended before any Stop.
    let (mut privacy, mut privacy_channels, privacy_input) =
        Worker::cooperative_fixture(&directory.path, "privacy");
    if let Some(code) = fault {
        privacy_channels
            .control
            .send(WorkerEvent::Fault { code: code.into() })
            .unwrap();
    }
    if killed {
        let pid = rustix::process::Pid::from_raw(privacy.fixture_pid() as i32).unwrap();
        rustix::process::kill_process(pid, rustix::process::Signal::KILL).unwrap();
    }
    drop(privacy_input);
    let deadline = Instant::now() + Duration::from_secs(3);
    while privacy.try_exit().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    if aged {
        thread::sleep(Duration::from_millis(1_700));
    }
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        early_end_blocks: None,
        early_finalize: false,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        privacy_controls,
        ring: None.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling: None,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::VadOnly,
        playout_start: PlayoutStart::default(),
        fixture_at_ms: None,
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
    };
    let (mut controller, _, _) = active_reply();
    let result = finish_active_run(
        &mut rig,
        &mut controller,
        &mut None,
        &mut SessionState::default(),
        &mut None,
        Err(crate::service_privacy::unavailable(
            "service privacy freshness lost before next observation",
        )),
    );
    for peer in peers {
        let _ = peer.join();
    }
    drop(privacy_channels);
    drop(rig);
    let _ = fs::remove_dir_all(&directory.path);
    result
}

/// The reviewer on #795 (15:50): on the Controls route the privacy worker
/// may end with its unreadable fault still queued when the cycle closes.
/// That fault, with a clean exit, keeps the cycle an unreadable switch for
/// the service to wait out, never a failed shutdown that ends the voice. A
/// killed worker, another fault, or the direct route still fail cleanup.
#[test]
fn a_queued_unreadable_fault_with_a_clean_exit_is_no_failed_shutdown() {
    let unreadable = format!(
        "{}physical privacy source expired",
        crate::privacy_bus::UNREADABLE
    );
    let error = |result: Result<()>| result.unwrap_err();
    let kept = error(shutdown_after_unreadable(
        true,
        Some(&unreadable),
        false,
        false,
    ));
    assert!(
        crate::service_privacy::is_unavailable(&*kept),
        "expected an unreadable switch, got: {kept}"
    );
    let kept = error(shutdown_after_unreadable(true, None, false, false));
    assert!(crate::service_privacy::is_unavailable(&*kept), "{kept}");
    // The reviewer on #795 (01:45): the fault aged out before the cycle
    // closed, so its receive is an error; with the worker's clean exit that
    // is no failed shutdown either.
    let kept = error(shutdown_after_unreadable(
        true,
        Some(&unreadable),
        false,
        true,
    ));
    assert!(crate::service_privacy::is_unavailable(&*kept), "{kept}");
    for (controls, fault, killed) in [
        (true, Some(unreadable.as_str()), true),
        (true, Some("privacy controller lease expired"), false),
        (false, Some(unreadable.as_str()), false),
    ] {
        let failed = error(shutdown_after_unreadable(controls, fault, killed, false));
        assert!(
            !crate::service_privacy::is_unavailable(&*failed),
            "{controls} {fault:?} {killed}: {failed}"
        );
        assert!(
            failed.to_string().contains("verified cleanup failed"),
            "{failed}"
        );
    }
}
