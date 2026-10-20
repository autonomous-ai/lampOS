use super::*;

struct Peers {
    speaker: WorkerChannels,
    capture: WorkerChannels,
    provider: WorkerChannels,
    privacy: WorkerChannels,
    ring: WorkerChannels,
}

fn fixture(directory: &SessionDirectory) -> (Rig, Peers) {
    fixture_with_command(directory, "exec sleep 60")
}

fn fixture_with_command(directory: &SessionDirectory, command: &str) -> (Rig, Peers) {
    let (speaker, speaker_peer) = Worker::command_fixture(&directory.path, "speaker", command);
    let (capture, capture_peer) = Worker::command_fixture(&directory.path, "capture", command);
    let (provider, provider_peer) = Worker::command_fixture(&directory.path, "provider", command);
    let (privacy, privacy_peer) = Worker::command_fixture(&directory.path, "privacy", command);
    let (ring, ring_peer) = Worker::command_fixture(&directory.path, "ring", command);
    (
        Rig {
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
            privacy_receipts: false,
            ring: Some(ring).into(),
            camera: None,
            motor: None,
            ring_channel_ceiling: Some(24),
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
        },
        Peers {
            speaker: speaker_peer,
            capture: capture_peer,
            provider: provider_peer,
            privacy: privacy_peer,
            ring: ring_peer,
        },
    )
}

fn drain_authority(peer: &mut WorkerChannels) -> Snapshot {
    let Some(Control::Authority { snapshot }) = peer.control.receive().unwrap() else {
        panic!("expected unchanged audio authority");
    };
    snapshot
}

#[test]
fn ring_failure_preserves_the_owner_and_dispatches_the_continuing_answer() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers) = fixture(&directory);
    let (mut controller, current, _) = super::tests::active_reply();
    let original = current.owner;
    let playback = current.playback;
    let mut reply = Some(current);
    let mut choreography = Some(RingChoreographer::new(24).unwrap());
    let mut trace = SessionState::default();
    let mut ring_trace = RingTrace::default();
    peers
        .ring
        .control
        .send(WorkerEvent::Fault {
            code: "test SPI failure".into(),
        })
        .unwrap();
    let failure_started = Instant::now();
    service_optional_ring(
        &mut rig,
        &mut choreography,
        &mut controller,
        &mut trace,
        &mut ring_trace,
    )
    .unwrap();
    let failure_elapsed = failure_started.elapsed();
    assert!(!rig.ring.is_active());
    assert!(rig.ring.is_some(), "disabled child remains owned");
    assert!(choreography.is_none());
    assert!(matches!(
        peers.ring.control.receive().unwrap(),
        Some(Control::Stop)
    ));
    assert_eq!(controller.snapshot(now()).unwrap().owner(), Some(original));
    assert_eq!(controller.snapshot(now()).unwrap().playback(), playback);
    let mut snapshot = controller.snapshot(now()).unwrap();
    rig.publish(snapshot).unwrap();
    rig.running().unwrap();
    for peer in [
        &mut peers.speaker,
        &mut peers.capture,
        &mut peers.provider,
        &mut peers.privacy,
    ] {
        assert_eq!(drain_authority(peer).owner(), Some(original));
    }
    assert!(
        peers.ring.control.receive::<Control>().unwrap().is_none(),
        "failed ring gets no authority renewal"
    );
    let mut pending = None;
    let mut sequence = 0;
    dispatch_speaker_chunk(
        &mut controller,
        &mut reply,
        &mut rig.speaker.channels.data,
        &mut pending,
        &mut sequence,
        &mut snapshot,
        &mut PlaybackMask::new(TurnTaking::VadOnly),
    )
    .unwrap();
    let chunk = peers
        .speaker
        .data
        .receive::<SpeakerChunk>()
        .unwrap()
        .unwrap();
    assert_eq!(chunk.permit.owner(), original);
    assert_eq!(chunk.samples, vec![123; 240]);
    assert_eq!(reply.as_ref().unwrap().playback, playback);
    assert_eq!(
        trace
            .iter()
            .filter(|e| e["kind"] == "ring_degraded")
            .count(),
        1
    );
    assert!(
        !trace
            .iter()
            .any(|e| e["kind"] == "reply_cancelled" || e["kind"] == "turn_finished")
    );
    let disabled_started = Instant::now();
    service_optional_ring(
        &mut rig,
        &mut choreography,
        &mut controller,
        &mut trace,
        &mut ring_trace,
    )
    .unwrap();
    eprintln!(
        "ring_degradation_host_observation first_fault_us={} disabled_tick_us={}",
        failure_elapsed.as_micros(),
        disabled_started.elapsed().as_micros()
    );

    assert!(
        peers.ring.control.receive::<Control>().unwrap().is_none(),
        "one stop, no retry or restart"
    );
}

/// #1191's review (#1178): a ring held 150 ms in a turn, through the real
/// service. The ring stays active with no Stop, its choreography holds the
/// frame and asks no other until that frame's late receipt, then a new frame
/// goes out and the answer goes on. The fourth hold in ten minutes disables
/// the ring, as any ring fault did before.
#[test]
fn a_ring_held_up_mid_turn_stays_on_and_the_fourth_hold_disables_it() {
    use crate::ring_wire::{RingFeedback, RingFrame, RingRejection};

    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers) = fixture(&directory);
    let (mut controller, current, _) = super::tests::active_reply();
    let original = current.owner;
    let mut reply = Some(current);
    let mut choreography = Some(RingChoreographer::new(24).unwrap());
    let mut trace = SessionState::default();
    let mut ring_trace = RingTrace::default();
    let renew = |controller: &mut Controller| {
        let until = MonoTime::from_micros(monotonic_us() + 500_000);
        controller
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
    };
    for hold in 1..=4 {
        renew(&mut controller);
        service_optional_ring(
            &mut rig,
            &mut choreography,
            &mut controller,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap();
        let frame = peers
            .ring
            .data
            .receive::<RingFrame>()
            .unwrap()
            .unwrap_or_else(|| panic!("hold {hold}: a frame goes out"));
        // The ring is held 150 ms; the coordinator renews the turn meanwhile.
        for _ in 0..3 {
            std::thread::sleep(Duration::from_millis(50));
            renew(&mut controller);
        }
        service_optional_ring(
            &mut rig,
            &mut choreography,
            &mut controller,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap();
        let mut stopped = false;
        while let Some(control) = peers.ring.control.receive::<Control>().unwrap() {
            stopped |= matches!(control, Control::Stop);
        }
        if hold == 4 {
            assert!(
                !rig.ring.is_active() && stopped,
                "the fourth hold disables it"
            );
            assert!(choreography.is_none());
            break;
        }
        assert!(
            rig.ring.is_active() && !stopped,
            "hold {hold}: the ring stays on"
        );
        // Held: no other frame is asked for.
        service_optional_ring(
            &mut rig,
            &mut choreography,
            &mut controller,
            &mut trace,
            &mut ring_trace,
        )
        .unwrap();
        assert!(peers.ring.data.receive::<RingFrame>().unwrap().is_none());
        // The answer goes on meanwhile.
        if hold == 1 {
            let mut snapshot = controller.snapshot(now()).unwrap();
            let (mut pending, mut sequence) = (None, 0);
            dispatch_speaker_chunk(
                &mut controller,
                &mut reply,
                &mut rig.speaker.channels.data,
                &mut pending,
                &mut sequence,
                &mut snapshot,
                &mut PlaybackMask::new(TurnTaking::VadOnly),
            )
            .unwrap();
            let chunk = peers
                .speaker
                .data
                .receive::<SpeakerChunk>()
                .unwrap()
                .unwrap();
            assert_eq!(chunk.permit.owner(), original);
        }
        // The held frame's late receipt: the worker refuses a frame so old.
        peers
            .ring
            .control
            .send(WorkerEvent::Ring {
                report: RingFeedback::Rejected {
                    owner: frame.permit.owner(),
                    phase: frame.phase,
                    requested_at_us: frame.requested_at_us,
                    reason: RingRejection::ExpiredRequest,
                },
            })
            .unwrap();
    }
    assert_eq!(
        trace
            .iter()
            .filter(|event| event["kind"] == "ring_resynced")
            .count(),
        3
    );
}

#[test]
fn a_closed_ring_channel_does_not_create_input_readiness_or_stop_audio_authority() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, peers) = fixture(&directory);
    let Peers {
        mut speaker,
        mut capture,
        mut provider,
        mut privacy,
        ring,
    } = peers;
    drop(ring);
    let mut controller = Controller::new(new_boot().unwrap(), now());
    let snapshot = controller.snapshot(now()).unwrap();
    assert!(!snapshot.listening_ready(now()));
    rig.publish(snapshot).unwrap();
    assert!(!rig.ring.is_active());
    let mut choreography = Some(RingChoreographer::new(24).unwrap());
    let mut trace = SessionState::default();
    service_optional_ring(
        &mut rig,
        &mut choreography,
        &mut controller,
        &mut trace,
        &mut RingTrace::default(),
    )
    .unwrap();
    assert!(choreography.is_none());
    for peer in [&mut speaker, &mut capture, &mut provider, &mut privacy] {
        assert!(!drain_authority(peer).listening_ready(now()));
    }
    assert!(!controller.snapshot(now()).unwrap().listening_ready(now()));
    assert!(
        !trace
            .iter()
            .any(|e| e["kind"] == "listening_ready" || e["kind"] == "ring_requested")
    );
    assert_eq!(trace[0]["kind"], "ring_degraded");
    assert_eq!(trace[0]["light_available"], false);
    assert!(
        rig.ring.cleanup_error().is_some(),
        "failed stop must remain a cleanup failure"
    );
}

#[test]
fn audio_authority_failure_is_still_terminal_even_if_ring_is_disabled() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers) = fixture(&directory);
    rig.ring.disable("test ring fault");
    drop(peers.capture);
    let mut controller = Controller::new(new_boot().unwrap(), now());
    // A child another test forks as `peers.capture` drops holds a copy of
    // its socket until its exec (CLOEXEC), so for a moment the capture
    // worker is still there: sent to, or full (Ten on #705, 18 Oct).
    // Publish until a send is refused as gone, for at most 5 s, reading
    // what the other workers were sent so that none of them fills, and it
    // must be the capture worker that is gone.
    let gone = |error: &io::Error| {
        matches!(
            error.kind(),
            io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionRefused
                | io::ErrorKind::NotConnected
                | io::ErrorKind::BrokenPipe
        )
    };
    let until = std::time::Instant::now() + Duration::from_secs(5);
    let refused = loop {
        match rig.publish(controller.snapshot(now()).unwrap()) {
            Err(error) if gone(&error) => break error,
            _ => {
                assert!(
                    std::time::Instant::now() < until,
                    "the capture worker was not gone after 5 s"
                );
                for peer in [&mut peers.speaker, &mut peers.privacy, &mut peers.provider] {
                    while let Ok(Some(_)) = peer.control.receive::<Control>() {}
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    };
    assert!(refused.to_string().starts_with("capture."), "{refused}");
}

#[test]
fn a_ring_fault_during_pre_activation_does_not_activate_input() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers) = fixture(&directory);
    let mut controller = Controller::new(new_boot().unwrap(), now());
    peers
        .ring
        .control
        .send(WorkerEvent::Fault {
            code: "initial black write failed".into(),
        })
        .unwrap();
    rig.ring
        .startup_tick(Some(controller.snapshot(now()).unwrap()));
    assert!(!rig.ring.is_active());
    assert!(!controller.snapshot(now()).unwrap().listening_ready(now()));
    assert!(matches!(
        peers.ring.control.receive().unwrap(),
        Some(Control::Stop)
    ));
    assert!(peers.ring.control.receive::<Control>().unwrap().is_none());
    let mut trace = SessionState::default();
    rig.ring.poll_cleanup(&mut trace).unwrap();
    assert_eq!(trace[0]["kind"], "ring_degraded");
}

#[test]
fn a_dead_optional_child_does_not_mask_a_dead_audio_child() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, _peers) = fixture(&directory);
    let error = rig
        .ring
        .active_mut()
        .unwrap()
        .terminate_and_reap(io::ErrorKind::Other, "test exit".into());
    assert!(error.to_string().contains("reaped"));
    rig.running().unwrap();
    assert!(!rig.ring.is_active());
    let _ = rig
        .capture
        .terminate_and_reap(io::ErrorKind::Other, "test exit".into());
    assert!(rig.running().is_err());
}

#[test]
fn early_confirmed_ring_cleanup_is_not_falsified_by_the_later_global_stop() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers) = fixture_with_command(&directory, "exit 0");
    rig.ring.disable("presentation exceeded budget");
    assert!(matches!(
        peers.ring.control.receive().unwrap(),
        Some(Control::Stop)
    ));
    let requested = rig.ring.stop_requested_at(0);
    let finished = monotonic_us();
    peers
        .ring
        .control
        .send(WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::Shutdown,
                write_started_at_us: requested,
                write_finished_at_us: finished,
            },
        })
        .unwrap();
    let mut trace = SessionState::default();
    rig.ring.poll_cleanup(&mut trace).unwrap();
    assert!(rig.ring.blank_confirmed());
    assert!(rig.ring.cleanup_error().is_none());
    // All children here actually exit zero. The black-write receipt above is a
    // synthetic peer claim, validated through the real boot/sequence channel.
    rig.shutdown(&mut trace).unwrap();
    assert_eq!(
        trace
            .iter()
            .filter(|e| e["kind"] == "ring_degraded_blank" && e["valid"] == true)
            .count(),
        1
    );
    assert!(!rig.ring.is_active());
}

#[test]
fn clean_process_exit_alone_never_proves_a_black_ring_or_successful_shutdown() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, _peers) = fixture_with_command(&directory, "exit 0");
    rig.ring.disable("test presentation failure");
    let mut trace = SessionState::default();
    rig.ring.poll_cleanup(&mut trace).unwrap();
    let error = rig.shutdown(&mut trace).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("no verified black-write receipt")
    );
    assert!(!rig.ring.blank_confirmed());
    assert!(
        !trace
            .iter()
            .any(|e| e["kind"] == "ring_degraded_blank" && e["valid"] == true)
    );
}

#[test]
fn blanking_before_disable_and_late_ready_cannot_repair_cleanup_evidence() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers) = fixture_with_command(&directory, "exit 0");
    rig.ring.disable("test presentation failure");
    let requested = rig.ring.stop_requested_at(0);
    peers
        .ring
        .control
        .send(WorkerEvent::Ring {
            report: RingFeedback::Blanked {
                reason: RingBlankReason::Shutdown,
                write_started_at_us: requested - 1,
                write_finished_at_us: requested,
            },
        })
        .unwrap();
    peers.ring.control.send(WorkerEvent::Ready).unwrap();
    let mut trace = SessionState::default();
    rig.ring.poll_cleanup(&mut trace).unwrap();
    assert!(!rig.ring.blank_confirmed());
    assert!(rig.ring.cleanup_error().is_some());
    assert!(!trace.iter().any(|e| e["kind"] == "listening_ready"));
    assert!(rig.shutdown(&mut trace).is_err());
}

#[test]
fn service_metadata_pressure_cannot_turn_optional_ring_failure_into_audio_failure() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers) = fixture(&directory);
    let (mut controller, reply, _) = super::tests::active_reply();
    let original = reply.owner;
    let (writer, entered, release) = crate::service_trace::ServiceTrace::stalled();
    let mut trace = SessionState::with_writer(writer);
    record(&mut trace, json!({"kind":"block_writer"})).unwrap();
    flush_cues(&mut trace, &mut None);
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    for n in 0..300 {
        record(&mut trace, json!({"kind":"fill_service_batch","n":n})).unwrap();
    }
    peers
        .ring
        .control
        .send(WorkerEvent::Fault {
            code: "synthetic SPI failure with full metadata batch".into(),
        })
        .unwrap();
    service_optional_ring(
        &mut rig,
        &mut Some(RingChoreographer::new(24).unwrap()),
        &mut controller,
        &mut trace,
        &mut RingTrace::default(),
    )
    .unwrap();
    assert!(!rig.ring.is_active());
    assert_eq!(controller.snapshot(now()).unwrap().owner(), Some(original));
    assert!(matches!(
        peers.ring.control.receive().unwrap(),
        Some(Control::Stop)
    ));
    assert_eq!(
        trace.len(),
        256,
        "optional cleanup cannot bypass the service cap"
    );
    controller
        .set_microphone_permission(now(), Permission::Denied)
        .unwrap();
    assert!(controller.snapshot(now()).unwrap().owner().is_none());
    flush_cues(&mut trace, &mut None);
    release.send(()).unwrap();
    let report = trace.finish_logging().unwrap();
    assert!(report.acknowledged && report.degraded && report.dropped_events > 0);
}

#[test]
fn optional_ring_incomplete_cleanup_blocks_physical_mute_reopen() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, _peers) = fixture_with_command(&directory, "exit 0");
    rig.ring.disable("synthetic presentation failure");
    let (mut controller, reply, _) = super::tests::active_reply();
    let mut reply = Some(reply);
    let mut trace = SessionState::default();
    let error = finish_active_run(
        &mut rig,
        &mut controller,
        &mut reply,
        &mut trace,
        &mut None,
        Err(Box::new(PhysicalMute {
            onboarding: None,
            observed_at_us: monotonic_us(),
        })),
    )
    .unwrap_err();
    assert!(error.downcast_ref::<PhysicalMute>().is_none());
    assert!(error.to_string().contains("black-write receipt"));
    let waits = std::cell::Cell::new(0);
    let mut failure = Some(error);
    let result = service::supervise(
        &mut trace,
        &mut None,
        &Termination::inactive(),
        |_, _| {
            waits.set(waits.get() + 1);
            Ok(service_privacy::WaitOutcome::Allowed)
        },
        |_, _| Err(failure.take().expect("failed cleanup must not restart")),
    );
    assert!(result.is_err());
    assert_eq!(waits.get(), 1);
    assert!(reply.is_none());
}

#[test]
fn mandatory_worker_faults_remain_terminal_during_optional_spawn() {
    use crate::process::ring_startup_tests::ChildFixture;
    for role in ["provider", "capture", "speaker"] {
        let fixture = ChildFixture::new();
        let (mut required, mut peer) = Worker::sleeping_fixture(&fixture.directory.path, role);
        let mut owner = Controller::new(new_boot().unwrap(), now());
        let mut startup = StartupEvents::default();
        let mut last = 0;
        let termination = Termination::inactive();
        let mut sent = false;
        let error = OptionalRing::spawn(
            &fixture.executable,
            &fixture.directory.path,
            new_boot().unwrap(),
            &[],
            || {
                if !sent && fixture.identity().is_some() {
                    peer.control.send(WorkerEvent::Fault {
                        code: format!("mandatory {role} failure"),
                    })?;
                    sent = true;
                }
                pulse(
                    &mut owner,
                    &mut [&mut required],
                    &mut last,
                    &mut startup,
                    &termination,
                )
            },
        )
        .err()
        .expect("required fault cannot become optional unavailability");
        assert_eq!(error.to_string(), format!("mandatory {role} failure"));
        assert!(!owner.snapshot(now()).unwrap().listening_ready(now()));
        required.check_running().unwrap();
        fixture.assert_reaped();
        assert!(!fixture.directory.path.join("ring").exists());
    }
}

#[test]
fn unavailable_startup_keeps_answer_cancellation_and_privacy_authority() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, mut peers) = fixture(&directory);
    rig.ring = OptionalRing::spawn(
        &directory.path.join("absent"),
        &directory.path,
        new_boot().unwrap(),
        &[],
        || Ok(()),
    )
    .unwrap();
    let (mut owner, reply, _) = super::tests::active_reply();
    let original = reply.owner;
    let mut reply = Some(reply);
    let mut trace = SessionState::default();
    rig.running().unwrap();
    let snapshot = owner.snapshot(now()).unwrap();
    rig.publish(snapshot).unwrap();
    for peer in [
        &mut peers.speaker,
        &mut peers.capture,
        &mut peers.provider,
        &mut peers.privacy,
    ] {
        assert_eq!(drain_authority(peer).owner(), Some(original));
    }
    cancel_reply(&mut owner, &mut reply, &mut trace, "user_interrupted").unwrap();
    assert!(reply.is_none());
    assert_ne!(owner.snapshot(now()).unwrap().owner(), Some(original));
    owner
        .set_microphone_permission(now(), Permission::Denied)
        .unwrap();
    owner
        .set_camera_permission(now(), Permission::Denied)
        .unwrap();
    let closed = owner.snapshot(now()).unwrap();
    rig.publish(closed).unwrap();
    for peer in [
        &mut peers.speaker,
        &mut peers.capture,
        &mut peers.provider,
        &mut peers.privacy,
    ] {
        let received = drain_authority(peer);
        assert!(!received.listening_ready(now()));
        assert_eq!(received.owner(), closed.owner());
    }
    let mut choreography = None;
    service_optional_ring(
        &mut rig,
        &mut choreography,
        &mut owner,
        &mut trace,
        &mut RingTrace::default(),
    )
    .unwrap();
    assert!(choreography.is_none());
    assert!(
        !trace
            .iter()
            .any(|e| e["kind"] == "ring_requested" || e["kind"] == "listening_ready")
    );
}

#[test]
fn failed_optional_ring_does_not_hide_mandatory_privacy_startup_failure() {
    use crate::process::ring_startup_tests::ChildFixture;
    let fixture = ChildFixture::new();
    let mut ring = OptionalRing::spawn(
        &fixture.directory.path.join("absent"),
        &fixture.directory.path,
        new_boot().unwrap(),
        &[],
        || Ok(()),
    )
    .unwrap();
    let mut peer = None;
    let error = Worker::spawn_with_tick(
        &fixture.executable,
        &fixture.directory.path,
        "privacy",
        new_boot().unwrap(),
        &[],
        || {
            ring.startup_tick(None);
            if peer.is_none()
                && let Some((parent, boot, _)) = fixture.identity()
            {
                let mut privacy =
                    WorkerChannels::bind(&fixture.directory.path, "privacy", parent, boot, true)?;
                privacy.connect(&fixture.directory.path, "privacy", true)?;
                privacy.control.send(WorkerEvent::Fault {
                    code: "fake physical privacy unavailable".into(),
                })?;
                peer = Some(privacy);
            }
            Ok(())
        },
    )
    .err()
    .expect("privacy Ready failure stays terminal");
    assert!(
        error
            .to_string()
            .contains("unexpected worker startup message")
    );
    assert!(!ring.is_active());
    fixture.assert_reaped();
}

/// lamp-52e6, 19 Oct 21:47 (#1620): a ring that had already ended on its own
/// mid-session (degraded, its exit observed) is seen again by the service
/// stop's first pass, a millisecond after `service_stop_requested`. The stop
/// still fails: nothing confirms the strip went black. Its record says the
/// exit came before the stop, so it is not read as the stop killing the ring.
#[test]
fn a_ring_that_ended_mid_session_fails_the_stop_and_its_exit_is_dated_before_it() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, _peers) = fixture_with_command(&directory, "exit 0");
    let ring_directory = SessionDirectory::create().unwrap();
    let (mut ring, _ring_peer) = Worker::command_fixture(&ring_directory.path, "ring", "exit 1");
    crate::process::ring_startup_tests::await_exit(&mut ring);
    rig.ring = Some(ring).into();
    // The loop's own check finds it gone and disables it; audio goes on.
    rig.ring.check_running();
    assert!(!rig.ring.is_active());
    let mut trace = SessionState::default();
    rig.ring.poll_cleanup(&mut trace).unwrap();
    assert!(
        trace
            .iter()
            .any(|e| e["kind"] == "ring_degraded_exit" && e["success"] == false)
    );
    let error = rig.shutdown(&mut trace).unwrap_err().to_string();
    assert!(error.contains("no verified black-write receipt"), "{error}");
    assert!(error.contains("exited abnormally"), "{error}");
    let exits: Vec<_> = trace
        .iter()
        .filter(|e| e["kind"] == "worker_exit_failed" && e["worker"] == "ring")
        .collect();
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0]["exited_before_stop"], true, "{}", exits[0]);
}

/// The other side of the same check: a ring the voice still held live at
/// the stop is not dated before it, however it ends.
#[test]
fn a_ring_live_at_the_stop_is_not_dated_before_it() {
    let directory = SessionDirectory::create().unwrap();
    let (mut rig, _peers) = fixture_with_command(&directory, "exit 0");
    let ring_directory = SessionDirectory::create().unwrap();
    let (ring, _ring_peer) = Worker::command_fixture(&ring_directory.path, "ring", "exit 1");
    rig.ring = Some(ring).into();
    let mut trace = SessionState::default();
    let error = rig.shutdown(&mut trace).unwrap_err().to_string();
    assert!(error.contains("no verified black-write receipt"), "{error}");
    let exit = trace
        .iter()
        .find(|e| e["kind"] == "worker_exit_failed" && e["worker"] == "ring")
        .expect("ring exit recorded");
    assert_eq!(exit["exited_before_stop"], false, "{exit}");
}
