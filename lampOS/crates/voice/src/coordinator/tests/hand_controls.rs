//! Local sockets and synthetic capture only. These tests measure the injected
//! coordinator-observation to speaker-control-send boundary, not bus delivery,
//! physical input detection, device playback, or acoustic silence.
use super::*;
use crate::activity::{ConfirmPolicy, START_BLOCKS};
use lamp_audio::ownership::PlaybackCursor;
use lamp_turn_end_wire::{Availability, Classifier, Outcome, Submit, Token};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[test]
fn hand_pats_report_only_accepted_metadata_without_inventing_a_turn() {
    let mut trace = SessionState::default();
    record_hand_pats(&mut trace, 0).unwrap();
    assert!(trace.is_empty());
    record_hand_pats(&mut trace, 2).unwrap();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0]["kind"], "hand_pat_observed");
    assert_eq!(trace[0]["count"], 2);
    assert_eq!(trace[0]["action"], "none");
    assert!(trace[0].get("turn").is_none());
}

fn received_authority(peer: &mut WorkerChannels) -> Snapshot {
    let Some(Control::Authority { snapshot }) = peer.control.receive().unwrap() else {
        panic!("speaker was not sent the authority revocation");
    };
    snapshot
}

fn pending(permit: OutputPermit, at_us: u64) -> PendingSpeaker {
    PendingSpeaker {
        origin: SpeechOrigin::Provider,
        sequence: 1,
        permit,
        accepted: 120,
        final_chunk: false,
        report_due_us: at_us + 100_000,
    }
}

#[test]
fn speaking_stop_sends_revocation_before_clock_receipt_and_drops_both_pcm_queues() {
    for muted in [false, true] {
        let directory = SessionDirectory::create().unwrap();
        let (mut speaker, mut peer) = pair(&directory, "speaker");
        let (mut owner, current, old_permit) = active_reply();
        owner
            .set_camera_permission(now(), Permission::Allowed)
            .unwrap();
        let old_owner = current.owner;
        let mut reply = Some(current);
        let mut queue = VecDeque::from([start_command(&reply), end_command(&reply)]);
        let observed_at_us = monotonic_us();
        let mut held = Some(pending(old_permit, observed_at_us));
        let mut admission = InputAdmission::default();
        let mut detector = TurnDetector::default();
        let mut clock_calls = 0;
        let cancellation = hand::cancel(
            &mut owner,
            &mut reply,
            &mut speaker.control,
            &mut held,
            &mut queue,
            &mut admission,
            &mut detector,
            muted,
            observed_at_us,
            || {
                clock_calls += 1;
                // Inspect the actual datagram inside the clock callback. The
                // receipt timestamp must be sampled after the successful send.
                let snapshot = received_authority(&mut peer);
                assert!(snapshot.owner().is_none());
                let at = MonoTime::from_micros(observed_at_us);
                let mut guard = BoundaryGuard::new(snapshot.boot(), at);
                guard.install(at, snapshot).unwrap();
                assert!(guard.check(at, old_permit, OutputKind::Speech).is_err());
                observed_at_us + 1_500
            },
        )
        .unwrap();
        assert_eq!(clock_calls, 1);
        assert_eq!(cancellation.speaker_told_at_us - observed_at_us, 1_500);
        assert!(cancellation.speaker_told_at_us - observed_at_us <= 2_000);
        assert_eq!(cancellation.cancelled_turn, Some(old_owner.turn()));
        assert!(reply.is_none() && held.is_none() && queue.is_empty());
        let cancelled = cancellation.reply.unwrap();
        assert_eq!(cancelled.owner, old_owner);
        assert!(cancelled.pcm.is_empty());
        assert_eq!(
            cancellation.snapshot.microphone_permission(),
            if muted {
                Permission::Denied
            } else {
                Permission::Allowed
            }
        );
        assert_eq!(
            cancellation.snapshot.camera_permission(),
            if muted {
                Permission::Denied
            } else {
                Permission::Allowed
            }
        );
        let mut snapshot = cancellation.snapshot;
        dispatch_speaker_chunk(
            &mut owner,
            &mut reply,
            &mut speaker.data,
            &mut held,
            &mut 0,
            &mut snapshot,
            &mut PlaybackMask::new(TurnTaking::VadOnly),
        )
        .unwrap();
        assert!(peer.data.receive::<SpeakerChunk>().unwrap().is_none());
    }
}

#[test]
fn thinking_and_idle_stop_are_local_and_repeated_stop_has_no_new_owner_to_cancel() {
    for idle in [false, true] {
        let directory = SessionDirectory::create().unwrap();
        let (mut speaker, mut peer) = pair(&directory, "speaker");
        let (mut owner, mut reply) = current_input();
        let turn = reply.as_ref().unwrap().owner;
        owner.input_ended(now(), turn).unwrap();
        reply.as_mut().unwrap().local_input_ended = true;
        if idle {
            owner.cancel_turn(now(), turn).unwrap();
            reply = None;
        }
        let mut admission = InputAdmission::default();
        let mut detector = TurnDetector::default();
        let mut generation = None;
        for repeated in [false, true] {
            let observed = monotonic_us();
            let cancellation = hand::cancel(
                &mut owner,
                &mut reply,
                &mut speaker.control,
                &mut None,
                &mut VecDeque::new(),
                &mut admission,
                &mut detector,
                false,
                observed,
                || observed,
            )
            .unwrap();
            let received = received_authority(&mut peer);
            assert_eq!(received.generation(), cancellation.snapshot.generation());
            assert!(received.owner().is_none());
            assert_eq!(received.microphone_permission(), Permission::Allowed);
            assert_eq!(cancellation.speaker_told_at_us - observed, 0);
            if idle || repeated {
                assert!(cancellation.cancelled_turn.is_none());
                assert!(cancellation.reply.is_none());
            } else {
                assert_eq!(cancellation.cancelled_turn, Some(turn.turn()));
                assert!(cancellation.reply.is_some());
            }
            if let Some(generation) = generation {
                assert_eq!(received.generation(), generation);
            }
            generation = Some(received.generation());
        }
    }
}

fn input_context(owner: &mut Controller, capture: CaptureLineage, at_us: u64) -> AdmissionContext {
    let at = MonoTime::from_micros(at_us);
    let until = MonoTime::from_micros(at_us + 100_000);
    owner
        .set_capture(at, CaptureState::RetainingUntil(until))
        .unwrap();
    owner
        .set_admission(at, AdmissionState::OpenUntil(until))
        .unwrap();
    AdmissionContext {
        capture,
        authority: owner.snapshot(at).unwrap(),
    }
}

fn audio(sequence: u64, captured_at_us: u64) -> ObservedAudio {
    ObservedAudio {
        sequence,
        captured_at_us,
        samples: [sequence as i16; 160],
    }
}

#[derive(Default)]
struct EndpointCalls {
    cancelled: AtomicUsize,
    submitted: AtomicUsize,
    polled: AtomicUsize,
}

struct EndpointProbe(Arc<EndpointCalls>);
impl Classifier for EndpointProbe {
    fn submit(&mut self, _: Token, _: &[i16]) -> Submit {
        self.0.submitted.fetch_add(1, Ordering::SeqCst);
        Submit::Accepted
    }
    fn poll(&mut self) -> Option<Outcome> {
        self.0.polled.fetch_add(1, Ordering::SeqCst);
        None
    }
    fn availability(&self) -> Availability {
        Availability::Ready
    }
    fn cancel(&mut self) {
        self.0.cancelled.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn pending_and_streaming_speech_drain_to_silence_even_under_playback_suppression() {
    for streaming in [false, true] {
        for suppressed in [false, true] {
            let directory = SessionDirectory::create().unwrap();
            let (mut speaker, mut peer) = pair(&directory, "speaker");
            let (mut owner, mut reply) = current_input();
            let previous_owner = reply.as_ref().unwrap().owner;
            if !streaming {
                owner.cancel_turn(now(), previous_owner).unwrap();
                reply = None;
            }
            let capture = CaptureLineage {
                worker: new_boot().unwrap(),
                epoch: 1,
                dsp_epoch: 1,
                privacy_generation: owner.snapshot(now()).unwrap().microphone_generation(),
            };
            let calls = Arc::new(EndpointCalls::default());
            let mut detector = TurnDetector::default()
                .with_confirmation(
                    Box::new(EndpointProbe(Arc::clone(&calls))),
                    ConfirmPolicy::responsive(),
                )
                .unwrap();
            let mut admission = InputAdmission::default();
            let mut mask = PlaybackMask::new(if suppressed {
                TurnTaking::V1Baseline
            } else {
                TurnTaking::VadOnly
            });
            let base = monotonic_us() + 10_000;
            let mut first = None;
            for sequence in 1..=u64::from(START_BLOCKS) {
                let at = base + sequence * 10_000;
                let context = input_context(&mut owner, capture, at);
                let (update, ended) = observe_prospective_input(
                    &mut mask,
                    &mut detector,
                    &mut admission,
                    context,
                    audio(sequence, at),
                    at - 1_000,
                    0.99,
                    at,
                )
                .unwrap();
                assert!(!ended);
                if let AdmissionStep::Candidate(candidate) = update.step {
                    first = Some(candidate.id);
                    if streaming {
                        let accepted = admission
                            .decide(
                                Evidence {
                                    candidate: candidate.id,
                                    through_sequence: candidate.trigger_sequence,
                                    produced_at_us: at,
                                    verdict: Verdict::Accept(
                                        AcceptanceBasis::DirectedSessionVadOnly,
                                    ),
                                },
                                context,
                                at,
                            )
                            .unwrap();
                        assert!(matches!(accepted, Decision::Accepted(_)));
                    }
                }
            }
            assert!(first.is_some());
            let stopped_at = base + u64::from(START_BLOCKS) * 10_000 + 1;
            if suppressed {
                // Playback starts after this human candidate already exists.
                // Its real accepted interval remains until a later receipt.
                mask.dispatched(previous_owner, 1, stopped_at, 240).unwrap();
                mask.accepted(
                    previous_owner,
                    1,
                    PlaybackCursor { epoch: 1, frame: 0 },
                    PlaybackCursor {
                        epoch: 1,
                        frame: 240,
                    },
                    240,
                    stopped_at,
                    stopped_at,
                    stopped_at,
                )
                .unwrap();
            }
            let cancellations_before = calls.cancelled.load(Ordering::SeqCst);
            let polls_before = calls.polled.load(Ordering::SeqCst);
            hand::cancel(
                &mut owner,
                &mut reply,
                &mut speaker.control,
                &mut None,
                &mut VecDeque::new(),
                &mut admission,
                &mut detector,
                false,
                stopped_at,
                || stopped_at + 700,
            )
            .unwrap();
            assert!(received_authority(&mut peer).owner().is_none());
            assert!(admission.draining());
            assert!(calls.cancelled.load(Ordering::SeqCst) > cancellations_before);
            // Twenty more speech blocks, then the unchanged 60-block silence
            // boundary. No continuation, endpoint, or fresh candidate escapes.
            for sequence in 7..=86 {
                let at = base + sequence * 10_000;
                let context = input_context(&mut owner, capture, at);
                let (update, ended) = observe_prospective_input(
                    &mut mask,
                    &mut detector,
                    &mut admission,
                    context,
                    audio(sequence, at),
                    at - 1_000,
                    if sequence <= 26 { 0.99 } else { 0.0 },
                    at,
                )
                .unwrap();
                assert!(matches!(update.step, AdmissionStep::None));
                assert_eq!(ended, sequence == 86);
                assert_eq!(admission.draining(), sequence != 86);
                if suppressed {
                    assert_eq!(mask.frame(), Disposition::Suppress);
                }
            }
            assert_eq!(calls.submitted.load(Ordering::SeqCst), 0);
            assert_eq!(calls.polled.load(Ordering::SeqCst), polls_before);
            assert_eq!(detector.last_endpoint().unwrap().silence_ms, 600);
            let ended_at = base + 86 * 10_000;
            if suppressed {
                mask.discarded(previous_owner, ended_at, ended_at).unwrap();
            }
            // Wait beyond the speaker tail, then six genuinely new blocks.
            // The only resulting candidate has a new identity and later onset.
            for sequence in 87..=104 {
                let at = base + sequence * 10_000;
                let context = input_context(&mut owner, capture, at);
                let (update, ended) = observe_prospective_input(
                    &mut mask,
                    &mut detector,
                    &mut admission,
                    context,
                    audio(sequence, at),
                    at - 1_000,
                    if sequence >= 99 { 0.99 } else { 0.0 },
                    at,
                )
                .unwrap();
                assert!(!ended);
                if sequence == 104 {
                    let AdmissionStep::Candidate(candidate) = update.step else {
                        panic!("fresh utterance did not become a new candidate");
                    };
                    assert_ne!(Some(candidate.id), first);
                    assert_eq!(candidate.trigger_sequence, 104);
                    assert!(candidate.first_read_at_us > ended_at);
                } else {
                    assert!(matches!(update.step, AdmissionStep::None));
                }
            }
        }
    }
}

/// #891, the reviewer's 04:30 finding: a hand Stop ends a call's wait, in
/// either order. Stopped after the lamp's name armed the wait, or with a
/// sign of the name heard after the Stop, the abandoned utterance drains to
/// the ordinary 600 ms boundary, not the call's 2 s, and a fresh command
/// right after that boundary is a new candidate.
#[test]
fn a_hand_stop_ends_a_calls_wait_in_either_order_and_a_fresh_command_is_heard() {
    for sign_first in [true, false] {
        let directory = SessionDirectory::create().unwrap();
        let (mut speaker, mut peer) = pair(&directory, "speaker");
        let (mut owner, mut reply) = current_input();
        let previous_owner = reply.as_ref().unwrap().owner;
        owner.cancel_turn(now(), previous_owner).unwrap();
        reply = None;
        let capture = CaptureLineage {
            worker: new_boot().unwrap(),
            epoch: 1,
            dsp_epoch: 1,
            privacy_generation: owner.snapshot(now()).unwrap().microphone_generation(),
        };
        let mut detector = TurnDetector::default().with_call_wait(200).unwrap();
        let mut admission = InputAdmission::default();
        let mut mask = PlaybackMask::new(TurnTaking::VadOnly);
        let base = monotonic_us() + 10_000;
        let mut step = |owner: &mut Controller,
                        detector: &mut TurnDetector,
                        admission: &mut InputAdmission,
                        sequence: u64,
                        score: f32| {
            let at = base + sequence * 10_000;
            let context = input_context(owner, capture, at);
            observe_prospective_input(
                &mut mask,
                detector,
                admission,
                context,
                audio(sequence, at),
                at - 1_000,
                score,
                at,
            )
            .unwrap()
        };
        // "Hey Lamp," 0.35 s.
        let mut first = None;
        for sequence in 1..=35 {
            let (update, _) = step(&mut owner, &mut detector, &mut admission, sequence, 0.99);
            if let AdmissionStep::Candidate(candidate) = update.step {
                first = Some(candidate.id);
            }
        }
        assert!(first.is_some());
        let began_us = base + 10_000;
        if sign_first {
            assert!(detector.name_heard(began_us), "the call did not wait");
        }
        let stopped_at = base + 35 * 10_000 + 1;
        hand::cancel(
            &mut owner,
            &mut reply,
            &mut speaker.control,
            &mut None,
            &mut VecDeque::new(),
            &mut admission,
            &mut detector,
            false,
            stopped_at,
            || stopped_at + 700,
        )
        .unwrap();
        assert!(received_authority(&mut peer).owner().is_none());
        assert!(admission.draining());
        if !sign_first {
            // The sign comes 100 ms after the Stop, as the name's part
            // says a hearing once it falls.
            assert!(
                !detector.name_heard(began_us),
                "the Stop's utterance waited"
            );
        }
        // The ordinary 600 ms boundary ends it, not the call's 2 s.
        for sequence in 36..=95 {
            let (update, ended) = step(&mut owner, &mut detector, &mut admission, sequence, 0.0);
            assert!(matches!(update.step, AdmissionStep::None));
            assert_eq!(ended, sequence == 95, "{sign_first} {sequence}");
        }
        assert!(!admission.draining());
        assert_eq!(detector.last_endpoint().unwrap().silence_ms, 600);
        assert_eq!(detector.last_endpoint().unwrap().call_wait_ms, None);
        // A fresh command, after 200 ms of quiet: a new candidate.
        for sequence in 96..=115 {
            let (update, ended) = step(&mut owner, &mut detector, &mut admission, sequence, 0.0);
            assert!(!ended && matches!(update.step, AdmissionStep::None));
        }
        let mut fresh = None;
        for sequence in 116..=121 {
            let (update, _) = step(&mut owner, &mut detector, &mut admission, sequence, 0.99);
            if let AdmissionStep::Candidate(candidate) = update.step {
                fresh = Some(candidate);
            }
        }
        let fresh = fresh.expect("the fresh command was dropped");
        assert_ne!(Some(fresh.id), first);
        assert_eq!(fresh.trigger_sequence, 121);
    }
}

#[test]
fn idle_stop_fences_already_captured_audio_but_accepts_a_later_utterance() {
    let directory = SessionDirectory::create().unwrap();
    let (mut speaker, mut peer) = pair(&directory, "speaker");
    let (mut owner, current, _) = active_reply();
    owner.cancel_turn(now(), current.owner).unwrap();
    let capture = CaptureLineage {
        worker: new_boot().unwrap(),
        epoch: 1,
        dsp_epoch: 1,
        privacy_generation: owner.snapshot(now()).unwrap().microphone_generation(),
    };
    let mut detector = TurnDetector::default();
    let mut admission = InputAdmission::default();
    let mut mask = PlaybackMask::new(TurnTaking::VadOnly);
    let base = monotonic_us() + 1_000;
    let stopped_at = base + 20_000;
    let cancellation = hand::cancel(
        &mut owner,
        &mut None,
        &mut speaker.control,
        &mut None,
        &mut VecDeque::new(),
        &mut admission,
        &mut detector,
        false,
        stopped_at,
        || stopped_at,
    )
    .unwrap();
    assert!(cancellation.cancelled_turn.is_none());
    assert!(received_authority(&mut peer).owner().is_none());
    for sequence in 1..=26 {
        let captured = if sequence <= 20 {
            base + sequence * 1_000
        } else {
            stopped_at + (sequence - 20) * 10_000
        };
        let at = stopped_at.max(captured) + sequence;
        let context = input_context(&mut owner, capture, at);
        let (update, ended) = observe_prospective_input(
            &mut mask,
            &mut detector,
            &mut admission,
            context,
            audio(sequence, captured),
            captured,
            0.99,
            at,
        )
        .unwrap();
        assert!(!ended);
        if sequence < 26 {
            assert!(matches!(update.step, AdmissionStep::None));
        } else {
            let AdmissionStep::Candidate(candidate) = update.step else {
                panic!("fresh post-Stop speech was not admitted as a candidate");
            };
            assert_eq!(candidate.trigger_sequence, 26);
            assert!(candidate.first_read_at_us > stopped_at);
        }
    }
}

#[test]
fn a_hand_abandoned_pending_utterance_still_fails_on_physical_privacy_loss() {
    let directory = SessionDirectory::create().unwrap();
    let (mut speaker, mut peer) = pair(&directory, "speaker");
    let (mut owner, current, _) = active_reply();
    owner.cancel_turn(now(), current.owner).unwrap();
    let capture = CaptureLineage {
        worker: new_boot().unwrap(),
        epoch: 1,
        dsp_epoch: 1,
        privacy_generation: owner.snapshot(now()).unwrap().microphone_generation(),
    };
    let mut detector = TurnDetector::default();
    let mut admission = InputAdmission::default();
    let mut mask = PlaybackMask::new(TurnTaking::V1Baseline);
    let base = monotonic_us() + 1_000;
    for sequence in 1..=6 {
        let at = base + sequence * 10_000;
        let context = input_context(&mut owner, capture, at);
        let (update, _) = observe_prospective_input(
            &mut mask,
            &mut detector,
            &mut admission,
            context,
            audio(sequence, at),
            at - 1_000,
            0.99,
            at,
        )
        .unwrap();
        if sequence == 6 {
            assert!(matches!(update.step, AdmissionStep::Candidate(_)));
        }
    }
    let stopped_at = base + 60_001;
    hand::cancel(
        &mut owner,
        &mut None,
        &mut speaker.control,
        &mut None,
        &mut VecDeque::new(),
        &mut admission,
        &mut detector,
        false,
        stopped_at,
        || stopped_at,
    )
    .unwrap();
    assert!(received_authority(&mut peer).owner().is_none());
    assert!(admission.draining());
    let at = base + 70_000;
    owner
        .set_microphone_permission(MonoTime::from_micros(at), Permission::Denied)
        .unwrap();
    let context = AdmissionContext {
        capture,
        authority: owner.snapshot(MonoTime::from_micros(at)).unwrap(),
    };
    let error = observe_prospective_input(
        &mut mask,
        &mut detector,
        &mut admission,
        context,
        audio(7, at),
        at - 1_000,
        0.99,
        at,
    )
    .unwrap_err();
    assert!(error.to_string().contains("lost capture authority"));
    assert!(!admission.draining());
    assert!(!admission.owns_stream());
}

#[test]
fn conversation_sends_hand_stop_before_a_provider_fault_queued_in_the_same_tick() {
    use lamp_msgs::{
        ClockStamp, ControlGesture, ControlIntent, ControlSource, ControlsState, Gesture, HoldTier,
    };
    let directory = SessionDirectory::create().unwrap();
    let (speaker, mut speaker_peer, speaker_input) =
        Worker::cooperative_ready_fixture(&directory.path, "speaker");
    let (capture, capture_peer, capture_input) =
        Worker::cooperative_ready_fixture(&directory.path, "capture");
    let (provider, mut provider_peer, provider_input) =
        Worker::cooperative_ready_fixture(&directory.path, "provider");
    let (privacy, privacy_peer, privacy_input) =
        Worker::cooperative_ready_fixture(&directory.path, "privacy");
    let pids = [&speaker, &capture, &provider, &privacy].map(|worker| worker.fixture_pid());
    // Stepped from here, the owner made inside: on the host clock a stall
    // before the loop's first pass could end it on physical privacy's
    // freshness before the stop and the fault are seen (#1220). The peers
    // are read after the loop, on this thread; a step reads none of them,
    // and teardown takes each one's Stop here, on the same clock.
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(10)).unwrap();
    let teardown = Rc::new(RefCell::new(Vec::<SteppedPeer>::new()));
    let pumped = Rc::clone(&teardown);
    clock
        .set_pump(move |phase| {
            if let crate::scenario_clock::Phase::Shutdown { deadline } = phase {
                for peer in pumped.borrow_mut().iter_mut() {
                    peer.read(deadline)?;
                }
            }
            Ok(())
        })
        .unwrap();
    let mut rig = Rig {
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
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
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
        early_end_blocks: None,
    };
    let (mut owner, current, permit) = active_reply();
    let old_owner = current.owner;
    let mut reply = Some(current);
    let at_us = monotonic_us();
    let stamp = || ClockStamp {
        clock_id: "test-hand-clock".into(),
        at_us,
        sequence: 1,
    };
    let state = ControlsState {
        stamp: stamp(),
        session_id: "test-controls".into(),
        enabled: true,
        ready: true,
        fault: None,
        buttons: vec![],
        head_active: false,
        base_ready: false,
        base_held_us: None,
        base_tier: HoldTier::None,
        latest_stop: Some(ControlGesture {
            stamp: stamp(),
            session_id: "test-controls".into(),
            source: ControlSource::Button,
            name: "primary".into(),
            gesture: Gesture::Tap,
            intent: ControlIntent::Stop,
            duration_us: 100_000,
            released_us: at_us,
            enabled: true,
        }),
        latest_mute: None,
        event_sequence: 1,
        rejected_inputs: 0,
        privacy: None,
    };
    state.check().unwrap();
    let mut controls =
        Some(HandControls::with_test_state("test-hand-clock", at_us, state).unwrap());
    provider_peer
        .control
        .send(WorkerEvent::Fault {
            code: "synthetic same-tick provider fault".into(),
        })
        .unwrap();
    let mut trace = SessionState::default();
    let error = conversation_with_controls(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        &mut trace,
        StartupEvents::default(),
        &mut reply,
        &mut None,
        &Termination::inactive(),
        &FailureNotices::none(),
        &mut controls,
        &mut None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("same-tick provider fault"));
    // Read the real speaker socket before finish_active_run. No teardown
    // revocation can satisfy these assertions: the production loop itself
    // must send its cancellation before returning the already-queued fault.
    assert_eq!(
        received_authority(&mut speaker_peer).owner(),
        Some(old_owner)
    );
    let stopped = received_authority(&mut speaker_peer);
    assert!(stopped.owner().is_none());
    assert!(reply.is_none());
    let at = now();
    let mut guard = BoundaryGuard::new(stopped.boot(), at);
    guard.install(at, stopped).unwrap();
    assert!(guard.check(at, permit, OutputKind::Speech).is_err());
    assert!(trace.iter().any(|row| {
        row["kind"] == "turn_finished"
            && row["turn"] == old_owner.turn()
            && row["outcome"] == "hand_stop"
    }));

    // The mock peers close their own pipes on Stop. Every child has bounded
    // cooperative teardown, and Worker::drop still reaps on assertion failure.
    teardown.borrow_mut().extend(
        [
            (speaker_peer, speaker_input),
            (capture_peer, capture_input),
            (provider_peer, provider_input),
            (privacy_peer, privacy_input),
        ]
        .into_iter()
        .zip(pids)
        .map(|((peer, input), pid)| SteppedPeer {
            speaker: false,
            peer,
            input: Some(input),
            pid,
            keeping: false,
            stopped: false,
            told: Vec::new(),
            chunks: Vec::new(),
        }),
    );
    let error = finish_active_run(
        &mut rig,
        &mut owner,
        &mut reply,
        &mut trace,
        &mut None,
        Err(error),
    )
    .unwrap_err();
    assert!(error.to_string().contains("same-tick provider fault"));
    assert!(!error.to_string().contains("verified cleanup failed"));
    for peer in teardown.take() {
        assert!(peer.stopped, "mock peer did not receive Stop");
    }
    for worker in [
        &mut rig.speaker,
        &mut rig.capture,
        &mut rig.provider,
        &mut rig.privacy,
    ] {
        assert!(worker.try_exit().unwrap().is_some());
    }
    drop(rig);
    drop(clock);
    fs::remove_dir(&directory.path).unwrap();
}

#[test]
fn queued_provider_failure_and_full_trace_cannot_preempt_the_local_speaker_send() {
    let directory = SessionDirectory::create().unwrap();
    let (mut speaker, mut speaker_peer) = pair(&directory, "speaker");
    let (mut provider, mut provider_peer) = pair(&directory, "provider");
    provider_peer
        .control
        .send(WorkerEvent::Fault {
            code: "synthetic simultaneous provider link drop".into(),
        })
        .unwrap();
    let (mut owner, current, permit) = active_reply();
    let mut reply = Some(current);
    let mut trace = SessionState::default();
    trace.resize(crate::session_state::MAX_FINITE_EVENTS - 1, Value::Null);
    let observed = monotonic_us();
    let cancellation = hand::cancel(
        &mut owner,
        &mut reply,
        &mut speaker.control,
        &mut None,
        &mut VecDeque::new(),
        &mut InputAdmission::default(),
        &mut TurnDetector::default(),
        false,
        observed,
        || {
            let state = received_authority(&mut speaker_peer);
            assert!(state.owner().is_none());
            observed + 2_000
        },
    )
    .unwrap();
    assert_eq!(cancellation.speaker_told_at_us - observed, 2_000);
    assert!(
        record_cancelled_reply(
            cancellation.reply.unwrap(),
            &mut trace,
            "hand_stop",
            cancellation.speaker_told_at_us,
        )
        .is_err()
    );
    let error = service_provider_control(
        &mut provider.control,
        &mut CloudAvailability::default(),
        &mut 16,
        &mut trace,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("simultaneous provider link drop")
    );
    let at = MonoTime::from_micros(observed);
    let mut guard = BoundaryGuard::new(cancellation.snapshot.boot(), at);
    guard.install(at, cancellation.snapshot).unwrap();
    assert!(guard.check(at, permit, OutputKind::Speech).is_err());
    assert!(reply.is_none());
}

#[test]
fn a_regressed_measurement_clock_is_an_error_after_actual_revocation() {
    let directory = SessionDirectory::create().unwrap();
    let (mut speaker, mut peer) = pair(&directory, "speaker");
    let (mut owner, current, permit) = active_reply();
    let mut reply = Some(current);
    let observed = monotonic_us();
    let error = hand::cancel(
        &mut owner,
        &mut reply,
        &mut speaker.control,
        &mut None,
        &mut VecDeque::new(),
        &mut InputAdmission::default(),
        &mut TurnDetector::default(),
        false,
        observed,
        || observed - 1,
    )
    .err()
    .expect("a regressed clock cannot produce a latency receipt");
    assert!(error.to_string().contains("clock moved backwards"));
    let snapshot = received_authority(&mut peer);
    let at = MonoTime::from_micros(observed);
    let mut guard = BoundaryGuard::new(snapshot.boot(), at);
    guard.install(at, snapshot).unwrap();
    assert!(guard.check(at, permit, OutputKind::Speech).is_err());
    assert!(reply.is_none());
}

#[test]
fn cancelled_request_tails_are_filtered_without_swallowing_new_requests_or_link_events() {
    use crate::provider_worker::{FailedStage, Fault};
    for request in [3, 4, 5] {
        let events = [
            ProviderOutput::Started {
                request,
                session: 1,
                waiting_for_barrier: false,
            },
            ProviderOutput::Audio {
                request,
                sequence: 1,
                provider_event_at_us: 1,
                source_frames: 240,
                source_offset: 0,
                samples: vec![17; 240],
            },
            ProviderOutput::Transcript {
                request: Some(request),
                text: "Let me look".into(),
                finished: false,
            },
            ProviderOutput::GenerationComplete { request },
            ProviderOutput::Interrupted { request },
            ProviderOutput::TurnComplete {
                request,
                idle: true,
            },
            ProviderOutput::TurnFailed {
                request,
                stage: FailedStage::Responding,
                reason: Fault::Transport,
            },
            ProviderOutput::InputDiscarded {
                request,
                dropped_samples: 160,
                sent_samples: 160,
            },
            ProviderOutput::OwnershipAssumed {
                request: Some(request),
                superseded: Some(2),
            },
        ];
        for event in events {
            assert!(!hand::cancelled_output(&event, None));
            assert_eq!(hand::cancelled_output(&event, Some(4)), request <= 4);
        }
    }
    for event in [
        ProviderOutput::Ready { setup_us: 12 },
        ended_output(),
        ProviderOutput::Transcript {
            request: None,
            text: "session-scoped evidence".into(),
            finished: true,
        },
        ProviderOutput::OwnershipAssumed {
            request: None,
            superseded: Some(3),
        },
    ] {
        assert!(!hand::cancelled_output(&event, Some(4)));
    }
}

#[test]
#[ignore = "manual local-host speaker-control latency probe; no lamp or acoustic claim"]
fn local_hand_stop_dispatch_latency() {
    let mut elapsed_us = Vec::with_capacity(100);
    for _ in 0..100 {
        // Each iteration has fresh authority and independent socket endpoints.
        // All setup, allocation, verification, and teardown is outside the
        // measured boundary; the provider has no role in this measurement.
        let directory = SessionDirectory::create().unwrap();
        let (mut speaker, mut peer) = pair(&directory, "speaker");
        let (mut owner, current, permit) = active_reply();
        let mut reply = Some(current);
        let mut held = Some(pending(permit, monotonic_us()));
        let mut queue = VecDeque::from([start_command(&reply), end_command(&reply)]);
        let mut admission = InputAdmission::default();
        let mut detector = TurnDetector::default();
        let observed = monotonic_us();
        let cancellation = hand::cancel(
            &mut owner,
            &mut reply,
            &mut speaker.control,
            &mut held,
            &mut queue,
            &mut admission,
            &mut detector,
            false,
            observed,
            monotonic_us,
        )
        .unwrap();
        elapsed_us.push(cancellation.speaker_told_at_us - observed);
        assert!(received_authority(&mut peer).owner().is_none());
        assert!(reply.is_none() && held.is_none() && queue.is_empty());
    }
    let first_us = elapsed_us[0];
    let mut repeated = elapsed_us[1..].to_vec();
    repeated.sort_unstable();
    let count = repeated.len();
    println!(
        "{}",
        json!({
            "measurement":"local_hand_stop_dispatch",
            "host_os":std::env::consts::OS,
            "host_arch":std::env::consts::ARCH,
            "clock":"actual lamp_ipc::monotonic_us",
            "boundary":"coordinator observation immediately before hand::cancel to clock sampled after successful local speaker Authority datagram send",
            "excludes":["fixture setup and teardown", "bus delivery", "hand gesture detection", "speaker worker scheduling", "device playback", "acoustic silence"],
            "samples":elapsed_us.len(),
            "first_us":first_us,
            "repeated_samples":count,
            "repeated_median_us":repeated[(count - 1) / 2],
            "repeated_p95_order_statistic_us":repeated[(count * 95).div_ceil(100) - 1],
            "repeated_worst_us":repeated[count - 1],
            "worst_us":elapsed_us.into_iter().max(),
            "injected_clock_test_intervals_us":[0, 700, 1500, 2000],
            "injected_clock_target_us":2000,
            "lamp_measurement":false
        })
    );
}

/// The button's same-tick rig, run with `controls` and the owner and reply
/// `start` makes until a provider fault queued beside them ends the loop.
/// Returns the trace, the reply left and every authority the speaker was sent
/// before teardown.
fn run_until_fault(
    controls: impl FnOnce() -> HandControls,
    start: impl FnOnce() -> (Controller, Option<Reply>),
) -> (SessionState, Option<Reply>, Vec<Snapshot>) {
    run_until_error(controls, start, "same-tick provider fault")
}

/// As [`run_until_fault`], for a loop that ends with an error saying
/// `ended_by`: the provider's fault, or a mute that came first. Stepped: on
/// the host clock a stall before the loop's first pass could end it on
/// physical privacy's freshness before the controls are seen (#1220).
fn run_until_error(
    controls: impl FnOnce() -> HandControls,
    start: impl FnOnce() -> (Controller, Option<Reply>),
    ended_by: &str,
) -> (SessionState, Option<Reply>, Vec<Snapshot>) {
    let ran = run_loop_with(controls, start, true, Clocking::Stepped);
    assert!(ran.error.contains(ended_by), "{}", ran.error);
    (ran.trace, ran.left, ran.told)
}

/// What one run of the loop left.
struct Ran {
    error: String,
    /// When the loop ended, on the clock it ran on.
    ended_us: u64,
    trace: SessionState,
    left: Option<Reply>,
    told: Vec<Snapshot>,
    /// Every chunk the speaker was sent.
    chunks: Vec<SpeakerChunk>,
}

/// How the loop's time passes. On the host's clock, each fixture worker's
/// side is read on its own thread. Stepped, they are read on the test thread
/// before each two-millisecond step of the scenario clock, so a slow host,
/// such as an ARM64 emulator, cannot age a tap before the loop first looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clocking {
    Host,
    Stepped,
}

/// A fixture worker's side read on the stepped clock, as the thread in
/// [`run_loop_with`] reads it on the host's.
struct SteppedPeer {
    speaker: bool,
    peer: WorkerChannels,
    input: Option<std::process::ChildStdin>,
    pid: u32,
    keeping: bool,
    stopped: bool,
    told: Vec<Snapshot>,
    chunks: Vec<SpeakerChunk>,
}

impl SteppedPeer {
    fn read(&mut self, deadline: Instant) -> io::Result<()> {
        if self.stopped {
            return Ok(());
        }
        while let Some(message) = self.peer.control.receive::<Control>().unwrap() {
            match message {
                Control::Stop => {
                    self.stopped = true;
                    // The child exits once its input closes, and its exit is
                    // waited for here, bounded by the scope's own watchdog,
                    // not the cleanup's real deadline, which a busy host can
                    // use up before this step runs (the reviewer, 07:07 on
                    // #705). The cleanup that follows finds it gone.
                    drop(self.input.take());
                    let bound = crate::scenario_clock::host_deadline().unwrap_or(deadline);
                    return crate::scenario_clock::wait_child_exited(self.pid, bound);
                }
                Control::Authority { snapshot } if self.speaker && self.keeping => {
                    self.told.push(snapshot)
                }
                Control::Authority { .. } => {}
                _ => panic!("unexpected command to a head-touch fixture worker"),
            }
        }
        if self.speaker {
            while let Some(chunk) = self.peer.data.receive::<SpeakerChunk>().unwrap() {
                if self.keeping {
                    self.chunks.push(chunk);
                }
            }
        }
        Ok(())
    }
}

/// The loop with these controls, ended by a provider fault queued before
/// it starts (`fault_first`), or else by itself a few passes in: the
/// fixture's privacy worker says nothing, so physical privacy becomes
/// unavailable.
fn run_loop(
    controls: impl FnOnce() -> HandControls,
    owner: Controller,
    reply: Option<Reply>,
    fault_first: bool,
) -> Ran {
    run_loop_with(
        controls,
        move || (owner, reply),
        fault_first,
        Clocking::Host,
    )
}

/// As [`run_loop`], with the loop's time passing as `clocking` says, and the
/// owner and its reply made by `start` just before the controls.
fn run_loop_with(
    controls: impl FnOnce() -> HandControls,
    start: impl FnOnce() -> (Controller, Option<Reply>),
    fault_first: bool,
    clocking: Clocking,
) -> Ran {
    run_loop_ringed(controls, start, fault_first, clocking, false)
}

/// As [`run_loop_with`], and with a ring worker when `ringed`, so the
/// trace holds the frames the ring was asked to draw (`ring_requested`).
/// The ring's side answers none: a frame only has to be asked for.
fn run_loop_ringed(
    controls: impl FnOnce() -> HandControls,
    start: impl FnOnce() -> (Controller, Option<Reply>),
    fault_first: bool,
    clocking: Clocking,
    ringed: bool,
) -> Ran {
    let directory = SessionDirectory::create().unwrap();
    // The stepped clock cannot rely on the scheduler running each child
    // before logical time advances: its fixture's echo proves it ready.
    let fixture = match clocking {
        Clocking::Host => Worker::cooperative_fixture,
        Clocking::Stepped => Worker::cooperative_ready_fixture,
    };
    let (speaker, speaker_peer, speaker_input) = fixture(&directory.path, "speaker");
    let (capture, capture_peer, capture_input) = fixture(&directory.path, "capture");
    let (provider, provider_peer, provider_input) = fixture(&directory.path, "provider");
    let (privacy, privacy_peer, privacy_input) = fixture(&directory.path, "privacy");
    let mut pids = vec![
        speaker.fixture_pid(),
        capture.fixture_pid(),
        provider.fixture_pid(),
        privacy.fixture_pid(),
    ];
    let ring = ringed.then(|| fixture(&directory.path, "ring"));
    let ring_channel_ceiling = ring.as_ref().map(|_| 24);
    let (ring, ring_peer) = match ring {
        Some((worker, peer, input)) => {
            pids.push(worker.fixture_pid());
            (Some(worker), Some((false, peer, input)))
        }
        None => (None, None),
    };
    let mut rig = Rig {
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        privacy_controls: false,
        ring: ring.into(),
        camera: None,
        motor: None,
        ring_channel_ceiling,
        diagnostic_directory: None,
        noise_suppression: NoiseSuppression::On,
        turn_taking: TurnTaking::VadOnly,
        playout_start: PlayoutStart::default(),
        endpoint: None,
        call_wait_blocks: None,
        cut_keeps_name: false,
        asker_first: crate::asker_first::Mode::Off,
        follows_on: false,
        paused: Box::new(|| false),
        early_end_blocks: None,
    };
    // Every worker's side is read while the loop runs, as a worker reads
    // it: a message is stale 100 ms after it is sent. The speaker's keeps
    // what it is told and sent until the loop has ended.
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    let mut provider_peer = provider_peer;
    if fault_first {
        provider_peer
            .control
            .send(WorkerEvent::Fault {
                code: "synthetic same-tick provider fault".into(),
            })
            .unwrap();
    }
    let running = Arc::new(AtomicBool::new(true));
    let drained = Arc::new(AtomicUsize::new(0));
    let peers = [
        (true, speaker_peer, speaker_input),
        (false, capture_peer, capture_input),
        (false, provider_peer, provider_input),
        (false, privacy_peer, privacy_input),
    ]
    .into_iter()
    .chain(ring_peer)
    .collect::<Vec<_>>();
    let (mut threads, mut stepped, mut scope) = (Vec::new(), None, None);
    if clocking == Clocking::Stepped {
        // Every child has crossed its readiness barrier: the loop, its
        // controls and these peers share the scenario clock from here.
        let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(10)).unwrap();
        let shared = Rc::new(RefCell::new(
            peers
                .into_iter()
                .zip(pids)
                .map(|((speaker, peer, input), pid)| SteppedPeer {
                    speaker,
                    peer,
                    input: Some(input),
                    pid,
                    keeping: true,
                    stopped: false,
                    told: Vec::new(),
                    chunks: Vec::new(),
                })
                .collect::<Vec<_>>(),
        ));
        let pumped = Rc::clone(&shared);
        clock
            .set_pump(move |phase| {
                // A real child's exit is not simulated: one deadline bounds
                // the children a step stops, and shutdown brings its own.
                let deadline = match phase {
                    crate::scenario_clock::Phase::Run => {
                        Instant::now() + Duration::from_millis(300)
                    }
                    crate::scenario_clock::Phase::Shutdown { deadline } => deadline,
                };
                for peer in pumped.borrow_mut().iter_mut() {
                    peer.read(deadline)?;
                }
                Ok(())
            })
            .unwrap();
        (stepped, scope) = (Some(shared), Some(clock));
    } else {
        threads = peers
            .into_iter()
            .map(|(speaker, mut peer, input)| {
                let (running, drained) = (Arc::clone(&running), Arc::clone(&drained));
                std::thread::spawn(move || {
                    let (mut told, mut chunks) = (Vec::new(), Vec::new());
                    let mut keeping = true;
                    let deadline = Instant::now() + Duration::from_secs(10);
                    loop {
                        let ended = !running.load(Ordering::Acquire);
                        while let Some(message) = peer.control.receive::<Control>().unwrap() {
                            match message {
                                Control::Stop => {
                                    drop(input);
                                    return (peer, told, chunks);
                                }
                                Control::Authority { snapshot } if speaker && keeping => {
                                    told.push(snapshot)
                                }
                                Control::Authority { .. } => {}
                                _ => panic!("unexpected command to a head-touch fixture worker"),
                            }
                        }
                        if speaker {
                            while let Some(chunk) = peer.data.receive::<SpeakerChunk>().unwrap() {
                                if keeping {
                                    chunks.push(chunk);
                                }
                            }
                        }
                        // Everything sent before the loop ended has been read now.
                        if ended && keeping {
                            keeping = false;
                            drained.fetch_add(1, Ordering::AcqRel);
                        }
                        assert!(Instant::now() < deadline, "mock peer did not receive Stop");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                })
            })
            .collect::<Vec<_>>();
    }
    let mut trace = SessionState::default();
    // Made last, just before the loop first polls it: a touch is fresh for
    // 100 ms only. Stepped, the owner and its reply are made here too, inside
    // the scope, so starting the children cannot spend the owner's windows
    // (the reviewer and the seventh on #1207).
    let (mut owner, mut reply) = start();
    let mut controls = Some(controls());
    let error = conversation_with_controls(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        &mut trace,
        StartupEvents::default(),
        &mut reply,
        &mut None,
        &Termination::inactive(),
        &FailureNotices::none(),
        &mut controls,
        &mut None,
    )
    .unwrap_err();
    let ended_us = monotonic_us();
    let message = error.to_string();
    running.store(false, Ordering::Release);
    while drained.load(Ordering::Acquire) < threads.len() {
        std::thread::sleep(Duration::from_millis(1));
    }
    // Everything sent before the loop ended is read now, as the threads do.
    if let Some(shared) = &stepped {
        for peer in shared.borrow_mut().iter_mut() {
            peer.read(Instant::now() + Duration::from_millis(300))
                .unwrap();
            peer.keeping = false;
        }
    }
    let left = reply.take();
    let _ = finish_active_run(
        &mut rig,
        &mut owner,
        &mut None,
        &mut trace,
        &mut None,
        Err(error),
    );
    let (mut told, mut chunks) = (Vec::new(), Vec::new());
    for peer in threads {
        let (peer, peer_told, peer_chunks) = peer.join().unwrap();
        drop(peer);
        told.extend(peer_told);
        chunks.extend(peer_chunks);
    }
    for peer in stepped.map(|shared| shared.take()).unwrap_or_default() {
        assert!(peer.stopped, "mock peer did not receive Stop");
        told.extend(peer.told);
        chunks.extend(peer.chunks);
    }
    for worker in [
        &mut rig.speaker,
        &mut rig.capture,
        &mut rig.provider,
        &mut rig.privacy,
    ] {
        assert!(worker.try_exit().unwrap().is_some());
    }
    drop(rig);
    drop(scope.take());
    fs::remove_dir(&directory.path).unwrap();
    Ran {
        error: message,
        ended_us,
        trace,
        left,
        told,
        chunks,
    }
}

/// A head touch whose newest sample is 15 ms old: never in the future, which
/// the receiver refuses, and well inside its 100 ms freshness.
fn head_controls() -> HandControls {
    let touched = monotonic_us() - 45_000;
    let (state, events) = crate::hand_controls::test_head_touch("test-hand-clock", touched);
    assert_eq!(events.len(), 1, "one qualified head touch");
    HandControls::with_test_touch("test-hand-clock", touched - 600_000, state, events).unwrap()
}

#[test]
fn a_head_touch_stops_the_lamp_while_it_says_its_answer() {
    // #711: a hand on its head while it talks means "stop", as the button.
    let turn = std::cell::Cell::new(None);
    let talking = || {
        let (owner, current, _) = active_reply();
        turn.set(Some(current.owner.turn()));
        (owner, Some(current))
    };
    let (trace, left, told) = run_until_fault(head_controls, talking);
    let turn = turn.get().unwrap();
    assert!(left.is_none(), "the answer was dropped");
    assert!(told.last().is_some_and(|stopped| stopped.owner().is_none()));
    let cancel = trace
        .iter()
        .find(|row| row["kind"] == "hand_control_cancel")
        .expect("a cancellation");
    assert_eq!(cancel["reason"], "head_stop");
    assert!(cancel["acquired_at_us"].as_u64().is_some());
    assert!(trace.iter().any(|row| {
        row["kind"] == "turn_finished" && row["turn"] == turn && row["outcome"] == "head_stop"
    }));
}

#[test]
fn a_lamp_put_to_sleep_from_the_head_ends_its_conversation_as_a_mute_named_head_sleep() {
    // #711: sleep is the software mute with its reason; the answer goes as
    // for any mute, and the trace says why.
    let record = std::env::temp_dir().join("never-written-sleep.json");
    let (trace, left, _) = run_until_error(
        || HandControls::with_test_asleep("test-hand-clock", monotonic_us(), &record).unwrap(),
        || {
            let (owner, current, _) = active_reply();
            (owner, Some(current))
        },
        "software mute",
    );
    assert!(left.is_none(), "the answer was dropped");
    let cancel = trace
        .iter()
        .find(|row| row["kind"] == "hand_control_cancel")
        .expect("a cancellation");
    assert_eq!(cancel["reason"], "head_sleep");
    assert!(!record.exists());
}

/// A lamp at rest: microphone allowed, listening, no turn.
fn quiet_owner() -> Controller {
    let mut owner = Controller::new(new_boot().unwrap(), now());
    owner
        .set_microphone_permission(now(), Permission::Allowed)
        .unwrap();
    let until = MonoTime::from_micros(monotonic_us() + 500_000);
    owner
        .set_capture(now(), CaptureState::RetainingUntil(until))
        .unwrap();
    owner
        .set_admission(now(), AdmissionState::OpenUntil(until))
        .unwrap();
    owner
}

/// A tap whose last step is 20 ms old when the loop first polls it.
fn head_tap() -> HandControls {
    HandControls::with_test_tap("test-hand-clock", monotonic_us() - 420_000).unwrap()
}

/// #711 (First on #711, 17 Oct 10:18): a tap on the head while the lamp
/// waits out a mute gets the asleep face for about a second, said in the
/// trace; none without a tap, none for the tap that wakes it, and one at a
/// time. Only where that face is the muted look: head touch on, not the
/// Controls privacy route, and not shown all along by `LAMP_RING_ASLEEP`.
#[test]
fn a_tap_while_muted_gets_the_asleep_face_once() {
    use crate::hand_controls::Observation;
    use crate::ring_asleep::{Asleep, TapFace};
    let starts = std::cell::Cell::new(0);
    let mut face = TapFace::new(|| {
        starts.set(starts.get() + 1);
        Some(Asleep::tapped_with(
            || lamp_ring::asleep::AsleepRing::new(DiscardFrames),
            lamp_ring::ChannelCeiling::new(24).unwrap(),
        ))
    });
    let tap = Observation {
        head_tap_at_us: Some(1),
        ..Observation::default()
    };
    let mut trace = SessionState::default();
    let faces = |trace: &SessionState| {
        trace
            .iter()
            .filter(|row| row["kind"] == "head_tap_muted_face")
            .count()
    };
    answer_muted_tap(Some(&mut face), &Observation::default(), false, &mut trace).unwrap();
    answer_muted_tap(Some(&mut face), &tap, true, &mut trace).unwrap();
    assert_eq!(faces(&trace), 0, "no tap, or the tap that wakes it");
    answer_muted_tap(Some(&mut face), &tap, false, &mut trace).unwrap();
    answer_muted_tap(Some(&mut face), &tap, false, &mut trace).unwrap();
    assert_eq!((faces(&trace), starts.get()), (1, 1), "one at a time");
    face.stop();
    // The slide closed with the software mute open is still muted: the tap
    // gets its face (the reviewer on #1288).
    use crate::privacy::PrivacyState;
    for (allowed, slide, hears) in [
        (true, None, true),
        (true, Some(PrivacyState::Allowed), true),
        (true, Some(PrivacyState::Muted), false),
        (false, Some(PrivacyState::Allowed), false),
        (false, None, false),
    ] {
        assert_eq!(may_hear(allowed, slide), hears, "{allowed} {slide:?}");
    }
    let mut face = TapFace::new(|| {
        Some(Asleep::tapped_with(
            || lamp_ring::asleep::AsleepRing::new(DiscardFrames),
            lamp_ring::ChannelCeiling::new(24).unwrap(),
        ))
    });
    let mut slid = SessionState::default();
    answer_muted_tap(
        Some(&mut face),
        &tap,
        may_hear(true, Some(PrivacyState::Muted)),
        &mut slid,
    )
    .unwrap();
    assert_eq!(faces(&slid), 1, "the slide closed, the mute open");
    face.stop();
    for (head_touch, asleep_on, privacy_controls, shown) in [
        (true, false, false, true),
        (false, false, false, false),
        (true, true, false, false),
        (true, false, true, false),
    ] {
        assert_eq!(
            tap_face_allowed(head_touch, asleep_on, privacy_controls),
            shown,
            "{head_touch} {asleep_on} {privacy_controls}"
        );
    }
}

/// The reviewer on #1288 (P3): the muted wait's own closures, run through
/// `muted_wait` with a waiter that says the slide's state and then looks,
/// as the privacy wait does. With the software mute open, a tap while the
/// slide is closed gets the asleep face; with the slide open, the same tap
/// is the one that lets it hear, and gets none.
#[test]
fn the_muted_waits_own_closures_count_the_slide_for_a_taps_face() {
    use crate::privacy::PrivacyState;
    use crate::ring_asleep::{Asleep, TapFace};
    use crate::service_privacy::WaitOutcome;
    let faces_for = |said: PrivacyState| {
        let controls = RefCell::new(Some(head_tap()));
        let reminders = RefCell::new(None);
        let face = TapFace::new(|| {
            Some(Asleep::tapped_with(
                || lamp_ring::asleep::AsleepRing::new(DiscardFrames),
                lamp_ring::ChannelCeiling::new(24).unwrap(),
            ))
        });
        let mut trace = SessionState::default();
        let outcome = muted_wait(
            &mut trace,
            &mut None,
            &controls,
            &reminders,
            Some(face),
            None,
            |on_state, gate| {
                on_state(said, crate::process::new_boot().unwrap(), 1)?;
                for _ in 0..3 {
                    gate()?;
                }
                Ok(WaitOutcome::Allowed)
            },
        )
        .unwrap();
        assert_eq!(outcome, WaitOutcome::Allowed);
        trace
            .iter()
            .filter(|row| row["kind"] == "head_tap_muted_face")
            .count()
    };
    assert_eq!(
        faces_for(PrivacyState::Muted),
        1,
        "the slide closed, the mute open"
    );
    assert_eq!(faces_for(PrivacyState::Allowed), 0, "the slide open");
}

/// #1371, and the reviewer and Jarvis on #1376: the muted wait's own
/// closures, around the real privacy wait and its mock GPIO worker (closed
/// for its first readings, then open), say the voice's health while it
/// waits. Before the first reading, "reading the privacy switch", never a
/// warning that it cannot be read; closed and while it steadies, "the
/// privacy switch"; open, an empty report as the wait ends. All well.
#[test]
fn the_muted_wait_says_the_microphones_are_off_by_the_switch() {
    use crate::service_privacy::tests::{Script, mock};
    use crate::service_privacy::{WaitOutcome, wait_worker_with_gate};
    use lamp_msgs::{Diagnostic, Level};
    use std::sync::{Arc, atomic::AtomicBool};
    let root = std::path::PathBuf::from(format!("/tmp/lvw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let mut hears = bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let directory = crate::process::SessionDirectory::create().unwrap();
    let (worker, peer) = mock(
        &directory.path,
        Script::Unmute,
        Arc::new(AtomicBool::new(false)),
    );
    let controls = RefCell::new(None);
    let reminders = RefCell::new(None);
    let mut trace = SessionState::default();
    let outcome = muted_wait(
        &mut trace,
        &mut None,
        &controls,
        &reminders,
        None::<crate::ring_asleep::TapFace<fn() -> Option<crate::ring_asleep::Asleep>>>,
        Some(crate::bus_out::WaitHealth::on(&bus).unwrap()),
        |on_state, gate| {
            wait_worker_with_gate(
                worker,
                crate::process::new_boot().unwrap(),
                &Termination::inactive(),
                on_state,
                gate,
            )
        },
    )
    .unwrap();
    assert_eq!(outcome, WaitOutcome::Allowed);
    drop(peer.join().unwrap().channels);
    let _ = std::fs::remove_dir(&directory.path);
    let mut said = Vec::new();
    while let Some(message) = hears.recv(std::time::Duration::from_millis(50)).unwrap() {
        let report = message.parse::<Diagnostic>().unwrap();
        said.push((report.level, report.message));
    }
    let off = |why: &str| (Level::Ok, format!("the microphones are off ({why})"));
    assert_eq!(
        said,
        [
            off("reading the privacy switch"),
            off("the privacy switch"),
            (Level::Ok, String::new()),
        ]
    );
    let _ = std::fs::remove_dir_all(root);
}

/// A ring that takes every frame and keeps none.
struct DiscardFrames;
impl lamp_ring::FrameSink for DiscardFrames {
    fn write_frame(&mut self, _: &lamp_ring::EncodedFrame) -> io::Result<()> {
        Ok(())
    }
}

/// The reviewer on #1244: the ring's face after a head tap follows the
/// voice's hum, so only the hum's own frames may be marked `listening`. A
/// tap that hums: the first frame the ring is asked for is. A tap refused
/// while the lamp still thinks: frames go out for its turn, and none is.
#[test]
fn only_the_hums_own_ring_frames_are_marked_listening() {
    let listening = |ran: &Ran| -> Vec<bool> {
        ran.trace
            .iter()
            .filter(|row| row["kind"] == "ring_requested")
            .map(|row| row["frame"]["listening"] == true)
            .collect()
    };
    let hummed = run_loop_ringed(
        head_tap,
        || (quiet_owner(), None),
        false,
        Clocking::Stepped,
        true,
    );
    let cue = hummed
        .trace
        .iter()
        .find(|row| row["kind"] == "head_tap_cue")
        .expect("the tap is judged");
    assert_eq!(cue["plays"], true, "{cue}");
    let frames = listening(&hummed);
    assert_eq!(frames.first(), Some(&true), "{frames:?}");
    let still_thinking = || {
        let (owner, mut current, _) = active_reply();
        current.had_audio = false;
        current.playback = None;
        (owner, Some(current))
    };
    let refused = run_loop_ringed(head_tap, still_thinking, false, Clocking::Stepped, true);
    let cue = refused
        .trace
        .iter()
        .find(|row| row["kind"] == "head_tap_cue")
        .expect("the tap is judged");
    assert_eq!(cue["plays"], false, "{cue}");
    let frames = listening(&refused);
    assert!(!frames.is_empty(), "the thinking turn's frames go out");
    assert!(frames.iter().all(|listening| !listening), "{frames:?}");
}

/// The reviewer on #1222: nothing pinned that a privacy worker that never
/// says anything ends the loop once its freshness (`PRIVACY_FRESH_US`) has
/// passed, and not before. Stepped, so the time the loop ran is its own:
/// a host stall neither ends it early nor keeps it going.
#[test]
fn a_silent_privacy_worker_ends_the_loop_once_its_freshness_has_passed() {
    let started = std::cell::Cell::new(0);
    let ran = run_loop_with(
        head_tap,
        || {
            started.set(monotonic_us());
            (quiet_owner(), None)
        },
        false,
        Clocking::Stepped,
    );
    assert!(
        ran.error
            .contains("physical privacy unavailable; not a user mute"),
        "{}",
        ran.error
    );
    let ran_us = ran.ended_us - started.get();
    let fresh = crate::privacy::PRIVACY_FRESH_US;
    assert!((fresh..fresh + 10_000).contains(&ran_us), "ran {ran_us} us");
}

#[test]
fn a_tap_on_a_quiet_lamps_head_plays_its_hm_as_a_turn_of_its_own() {
    // #711: a soft "hm?", through the speaker path, nothing to the provider.
    let ran = run_loop_with(head_tap, || (quiet_owner(), None), false, Clocking::Stepped);
    let cue = ran
        .trace
        .iter()
        .find(|row| row["kind"] == "head_tap_cue")
        .expect("the tap is judged");
    assert_eq!(cue["plays"], true, "{cue}");
    assert!(cue["acquired_at_us"].as_u64() < cue["at_us"].as_u64());
    let admitted = ran
        .trace
        .iter()
        .find(|row| row["kind"] == "input_admitted")
        .expect("its own turn");
    assert_eq!(admitted["admission_basis"], "head_tap_cue");
    assert!(admitted["provider_session"].is_null());
    let hum = crate::head_cue::hum(crate::head_cue::DBFS);
    let first = ran.chunks.first().expect("the hum reached the speaker");
    assert_eq!(first.samples, hum[..240]);
    assert!(
        ran.trace
            .iter()
            .all(|row| row["kind"] != "provider_request_started")
    );
}

#[test]
fn a_tap_while_the_lamp_is_still_thinking_keeps_its_answer_and_does_not_hum() {
    // Still thinking: the tap is noted, the answer kept, no hum.
    let still_thinking = || {
        let (owner, mut current, _) = active_reply();
        current.had_audio = false;
        current.playback = None;
        (owner, Some(current))
    };
    let ran = run_loop_with(head_tap, still_thinking, false, Clocking::Stepped);
    let cue = ran
        .trace
        .iter()
        .find(|row| row["kind"] == "head_tap_cue")
        .expect("the tap is judged");
    assert_eq!(
        (cue["plays"].clone(), cue["why"].clone()),
        (false.into(), "not_quiet".into())
    );
    assert!(ran.left.is_some(), "the answer is kept");
    assert!(
        ran.chunks
            .iter()
            .all(|chunk| chunk.samples[..] != crate::head_cue::hum(crate::head_cue::DBFS)[..240])
    );
}

#[test]
#[ignore = "manual host probe: the hum's decision to its first chunk sent to the speaker; no lamp claim"]
fn head_tap_cue_decision_to_speaker_dispatch_latency() {
    let mut elapsed_us = Vec::new();
    for _ in 0..50 {
        let ran = run_loop(head_tap, quiet_owner(), None, false);
        let at = |kind: &str| {
            ran.trace
                .iter()
                .find(|row| row["kind"] == kind)
                .and_then(|row| row["at_us"].as_u64())
                .unwrap()
        };
        elapsed_us.push(at("head_tap_cue_attempted") - at("head_tap_cue"));
    }
    elapsed_us.sort_unstable();
    let count = elapsed_us.len();
    println!(
        "{}",
        json!({
            "measurement":"head_tap_cue_decision_to_speaker_dispatch",
            "host_os":std::env::consts::OS,
            "host_arch":std::env::consts::ARCH,
            "boundary":"the coordinator's head_tap_cue decision to its first chunk reserved for the speaker transport",
            "excludes":["the hand on the pad", "Controls' qualification and release", "the bus and the coordinator's poll (head_touch_bus_to_poll_latency)", "the speaker worker and ALSA (head_tap_cue_first_write on a lamp)", "acoustic onset"],
            "samples":count,
            "median_us":elapsed_us[(count - 1) / 2],
            "p95_us":elapsed_us[(count * 95).div_ceil(100) - 1],
            "worst_us":elapsed_us[count - 1],
            "lamp_measurement":false
        })
    );
}

#[test]
fn a_head_touch_while_the_lamp_is_still_thinking_asks_nothing() {
    // Its answer has no sound yet: a pat on the head must not throw it away.
    let still_thinking = || {
        let (owner, mut current, _) = active_reply();
        current.had_audio = false;
        current.playback = None;
        (owner, Some(current))
    };
    let (trace, left, _) = run_until_fault(head_controls, still_thinking);
    assert!(left.is_some(), "the answer is kept");
    assert!(!trace.iter().any(|row| row["kind"] == "hand_control_cancel"));
    assert!(trace.iter().any(|row| row["kind"] == "head_touch_noticed"));
}
