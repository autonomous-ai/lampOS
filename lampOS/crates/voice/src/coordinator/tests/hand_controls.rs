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
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        privacy_controls: false,
        capture_held: false,
        privacy_receipts: false,
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
        early_end_blocks: None,
        early_finalize: false,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: false,
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
        no_privacy_slide: false,
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
            capture: None,
            provider: false,
            provider_inputs: Vec::new(),
            privacy: false,
            privacy_said_us: None,
            source: None,
            authorities: Vec::new(),
            chunk_times: Vec::new(),
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
            ProviderOutput::HoldingAnswer {
                request,
                session: 1,
            },
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
    /// When each authority reached the capture's side, on the stepped
    /// clock (#2602). Empty on the host's clock.
    capture_authorities: Vec<u64>,
    /// When each speaker chunk reached the speaker's side (stepped only).
    chunk_times: Vec<u64>,
    /// When each authority reached the speaker's side (stepped only).
    speaker_authorities: Vec<u64>,
    /// When each authority reached the rig's camera's side, with
    /// `RIG_CAMERA` (stepped only).
    camera_authorities: Vec<u64>,
    /// Capture's permission/start/frame history for opted-in wake scenarios.
    capture: Option<SteppedCapture>,
    /// Commands actually received on the provider data socket in wake scenarios.
    provider_inputs: Vec<ProviderInput>,
    error: String,
    /// When the loop ended, on the clock it ran on.
    ended_us: u64,
    trace: SessionState,
    left: Option<Reply>,
    told: Vec<Snapshot>,
    /// Every chunk the speaker was sent.
    chunks: Vec<SpeakerChunk>,
    /// The loop's controls, as it left them.
    controls: Option<HandControls>,
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

thread_local! {
    /// Until when, on the scenario clock, the stepped privacy worker says
    /// the switch is open every 5 ms, so a loop runs past the privacy
    /// freshness that otherwise ends it at 50 ms. Unset, it says nothing,
    /// as before (#1362's hold tests).
    static PRIVACY_FRESH_UNTIL: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// Within this span, on the scenario clock, the privacy worker kept
    /// fresh says nothing: a gap in the switch's readings (#2602).
    static PRIVACY_SILENT: std::cell::Cell<Option<(u64, u64)>> = const { std::cell::Cell::new(None) };
    /// The loop's rig on the Controls privacy route, where a gap may be
    /// ridden (#2602). Unset, the direct route, as before.
    static RIG_PRIVACY_CONTROLS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// From this time, on the scenario clock, the privacy worker kept fresh
    /// says the switch is closed.
    static PRIVACY_CLOSED_FROM: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// The privacy worker fails once, 100 ms into its silent span (#2602).
    static PRIVACY_FAULT_IN_GAP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Exercise the bus worker's production expiry state machine, rather
    /// than a silent mock that cannot fail at its own freshness deadline.
    static PRIVACY_SOURCE_EXPIRY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The loop's rig with a camera of its own, whose side records each
    /// authority it is given (#2602, the first builder's P1 on #2623).
    static RIG_CAMERA: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Opt in to a capture peer that waits for Allowed and StartCapture, then
    /// returns fresh silent frames from this time. Other fixtures stay silent.
    static WAKE_CAPTURE_FROM: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    /// Before the first privacy reading, pause at the first time until the
    /// second. The initial closed-permission pass runs before this fault.
    static WAKE_PRIVACY_START_PAUSE: std::cell::Cell<Option<(u64, u64)>> = const { std::cell::Cell::new(None) };
    /// The next loop's onboarding walk, its storage loaded, and its file
    /// (#3747). Unset, no walk, as before.
    static RIG_ONBOARDING: RefCell<Option<(crate::onboarding::storage::ServiceStorage, PathBuf)>> =
        const { RefCell::new(None) };
}

struct SourcePeer {
    gap: crate::privacy_bus::SourceGap,
    gate: PrivacyGate,
    permitted: bool,
    granted: bool,
    failed: bool,
}

#[derive(Default)]
struct SteppedCapture {
    authority: Option<Snapshot>,
    granted_at_us: Option<u64>,
    started_at_us: Option<u64>,
    /// Acquisition time and microphone generation of each frame sent.
    frames: Vec<(u64, u64)>,
}

fn silent_capture_frame(at: u64, privacy_generation: u64, frame_sequence: u64) -> MicrophoneFrame {
    MicrophoneFrame {
        raw: None,
        linear: None,
        epoch: 1,
        dsp_epoch: 1,
        privacy_generation,
        frame_sequence,
        read_completed_at_us: at,
        probability: 0.0,
        echo: None,
        reference: Default::default(),
        samples: vec![0; 160],
        timing: crate::wire::CaptureTiming {
            first_read_started_at_us: at,
            last_read_started_at_us: at,
            successful_reads: 1,
            alsa_status_monotonic_us: None,
            status_observed_at_us: at,
            available_frames: 0,
            delayed_frames: 0,
            processing_completed_at_us: at,
            aec_queue_delay_ms: 0,
            reference: crate::wire::ReferenceTiming {
                playback_epoch: 1,
                clock_started_at_us: Some(at),
                ..crate::wire::ReferenceTiming::default()
            },
        },
    }
}

/// A fixture worker's side read on the stepped clock, as the thread in
/// [`run_loop_with`] reads it on the host's.
struct SteppedPeer {
    speaker: bool,
    capture: Option<SteppedCapture>,
    provider: bool,
    provider_inputs: Vec<ProviderInput>,
    /// The privacy worker's side, which [`PRIVACY_FRESH_UNTIL`] may keep
    /// fresh, and when it last said the switch open.
    privacy: bool,
    privacy_said_us: Option<u64>,
    source: Option<SourcePeer>,
    /// When each authority reached this side, on the scenario clock.
    authorities: Vec<u64>,
    /// When each speaker chunk reached this side.
    chunk_times: Vec<u64>,
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
                    self.authorities.push(monotonic_us());
                    self.told.push(snapshot)
                }
                Control::Authority { snapshot } => {
                    if let Some(capture) = &mut self.capture
                        && WAKE_CAPTURE_FROM.with(std::cell::Cell::get).is_some()
                    {
                        if snapshot.microphone_permission() == Permission::Allowed {
                            capture.granted_at_us.get_or_insert(monotonic_us());
                        }
                        capture.authority = Some(snapshot);
                    }
                    if let Some(source) = &mut self.source {
                        source.permitted = snapshot.microphone_permission() == Permission::Allowed;
                    }
                    if self.keeping {
                        self.authorities.push(monotonic_us());
                    }
                }
                Control::StartCapture => {
                    if let Some(capture) = &mut self.capture
                        && WAKE_CAPTURE_FROM.with(std::cell::Cell::get).is_some()
                    {
                        capture.started_at_us.get_or_insert(monotonic_us());
                    }
                }
                Control::ProviderOutputCapacity { .. } => {}
                other => panic!("unexpected command to a head-touch fixture worker: {other:?}"),
            }
        }
        if self.privacy
            && self.keeping
            && let Some((pause_at_us, resume_at_us)) =
                WAKE_PRIVACY_START_PAUSE.with(std::cell::Cell::get)
        {
            let at_us = monotonic_us();
            if at_us < pause_at_us {
                return Ok(());
            }
            WAKE_PRIVACY_START_PAUSE.with(|pause| pause.set(None));
            // All peers have drained their initial authority before the
            // pause. The Open below is acquired after it, never backdated.
            crate::scenario_clock::elapse_for_fault(Duration::from_micros(
                resume_at_us.saturating_sub(at_us),
            ))?;
        }
        let now_us = monotonic_us();
        if self.keeping
            && let Some(from_us) = WAKE_CAPTURE_FROM.with(std::cell::Cell::get)
        {
            if let Some(capture) = &mut self.capture
                && capture.started_at_us.is_some()
                && let Some(authority) = capture.authority
                && authority.microphone_permission() == Permission::Allowed
                && authority.issued_at().as_micros() <= now_us
                && now_us < authority.expires_at().as_micros()
                && now_us >= from_us
                && capture
                    .frames
                    .last()
                    .is_none_or(|(at, _)| now_us >= at + 10_000)
            {
                let generation = authority.microphone_generation();
                if capture.frames.is_empty() {
                    let mut started = super::capture_started(NoiseSuppression::On);
                    if let WorkerEvent::CaptureStarted {
                        privacy_generation, ..
                    } = &mut started
                    {
                        *privacy_generation = generation;
                    }
                    self.peer.control.send(started)?;
                }
                self.peer.data.send(silent_capture_frame(
                    now_us,
                    generation,
                    capture.frames.len() as u64 + 1,
                ))?;
                capture.frames.push((now_us, generation));
            }
            if self.provider {
                while let Some(input) = self.peer.data.receive::<ProviderInput>()? {
                    self.provider_inputs.push(input);
                }
            }
        }
        if self.privacy
            && self.source.as_ref().is_none_or(|source| !source.failed)
            && PRIVACY_FRESH_UNTIL
                .with(std::cell::Cell::get)
                .is_some_and(|until| now_us < until)
            && !PRIVACY_SILENT
                .with(std::cell::Cell::get)
                .is_some_and(|(from, to)| (from..to).contains(&now_us))
            && self
                .privacy_said_us
                .is_none_or(|said| now_us >= said + 5_000)
        {
            let muted = PRIVACY_CLOSED_FROM
                .with(std::cell::Cell::get)
                .is_some_and(|from| now_us >= from);
            if let Some(source) = &mut self.source {
                if let Some(event) = source
                    .gap
                    .before_observation(
                        self.privacy_said_us,
                        source.permitted && source.granted,
                        now_us,
                    )
                    .map_err(io::Error::other)?
                {
                    source.gate.close();
                    self.peer.control.send(event)?;
                }
                source.gate.observe(now_us, now(), now(), muted);
                source.granted |= source.permitted && source.gate.allowed(now());
            }
            self.privacy_said_us = Some(now_us);
            self.peer.control.send(WorkerEvent::Privacy {
                receipt: None,
                acquired_at_us: now_us,
                muted,
            })?;
        }
        if let Some(source) = &mut self.source
            && !source.failed
        {
            let expired = source.gap.check(now_us).and_then(|()| {
                if let Some(last) = self.privacy_said_us
                    && now_us.saturating_sub(last) >= crate::privacy::PRIVACY_FRESH_US
                {
                    return source.gap.missing(
                        last + crate::privacy::PRIVACY_FRESH_US,
                        source.permitted && source.granted,
                        now_us,
                    );
                }
                if matches!(
                    source.gate.state(now()),
                    PrivacyState::Allowed | PrivacyState::Muted
                ) {
                    source.gap.recovered();
                }
                Ok(None)
            });
            match expired {
                Ok(Some(event)) => {
                    source.gate.close();
                    self.peer.control.send(event)?;
                }
                Ok(None) => {}
                Err(error) => {
                    source.failed = true;
                    self.peer.control.send(WorkerEvent::Fault {
                        code: format!("{}{error}", crate::privacy_bus::UNREADABLE),
                    })?;
                }
            }
        }
        if self.privacy
            && PRIVACY_SILENT
                .with(std::cell::Cell::get)
                .is_some_and(|(from, _)| now_us >= from + 100_000)
            && PRIVACY_FAULT_IN_GAP.with(|fault| fault.replace(false))
        {
            self.peer.control.send(WorkerEvent::Fault {
                code: "privacy worker fault (test)".into(),
            })?;
        }
        if self.speaker {
            while let Some(chunk) = self.peer.data.receive::<SpeakerChunk>().unwrap() {
                if self.keeping {
                    self.chunk_times.push(monotonic_us());
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
    run_loop_ringed(controls, start, fault_first, clocking, false, false)
}

/// As [`run_loop_with`], and with a ring worker when `ringed`, so the
/// trace holds the frames the ring was asked to draw (`ring_requested`).
/// The ring's side answers none: a frame only has to be asked for. With
/// `capture_frame` (stepped only), the capture's side has given one frame
/// before the loop first reads it.
fn run_loop_ringed(
    controls: impl FnOnce() -> HandControls,
    start: impl FnOnce() -> (Controller, Option<Reply>),
    fault_first: bool,
    clocking: Clocking,
    ringed: bool,
    capture_frame: bool,
) -> Ran {
    assert!(!capture_frame || clocking == Clocking::Stepped);
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
    // A camera of the rig's own, only when a test asks (#2602): last of the
    // sides, so the others keep their places.
    let (camera, camera_peer) = if RIG_CAMERA.with(std::cell::Cell::get) {
        let (worker, peer, input) = fixture(&directory.path, "camera");
        pids.push(worker.fixture_pid());
        let source = VisualSource {
            socket: directory.path.join("camera.frames"),
            camera: worker.boot,
            source: "desk".into(),
            width: 320,
            height: 240,
            max_frame_bytes: 128 * 1024,
        };
        (Some((worker, source)), Some((false, peer, input)))
    } else {
        (None, None)
    };
    // Every child has crossed its real readiness barrier. Enter the scenario
    // before any runtime message is queued or any observer samples its clock:
    // the first provider fault and the camera must share the loop's epoch too.
    let mut scope = (clocking == Clocking::Stepped).then(|| {
        crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap()
    });
    let camera = camera.map(|(worker, source)| {
        // Opening for the whole run: the side never answers a start.
        OptionalCamera::new(worker, source).opening_until(u64::MAX)
    });
    let mut rig = Rig {
        expression_out: None,
        second: None,
        microphone: crate::options::Microphone::Mic2,
        end_silence_blocks: None,
        faint_level: None,
        speaker,
        capture,
        provider,
        privacy,
        privacy_controls: RIG_PRIVACY_CONTROLS.with(std::cell::Cell::get),
        capture_held: false,
        privacy_receipts: false,
        ring: ring.into(),
        camera,
        motor: None,
        ring_channel_ceiling,
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
        early_end_blocks: None,
        early_finalize: false,
        open_name: false,
        call_takes_next: false,
        unfinished_wait: false,
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
    .chain(camera_peer)
    .collect::<Vec<_>>();
    let (mut threads, mut stepped) = (Vec::new(), None);
    if let Some(clock) = scope.as_ref() {
        let shared = Rc::new(RefCell::new(
            peers
                .into_iter()
                .zip(pids)
                .enumerate()
                .map(|(index, ((speaker, peer, input), pid))| SteppedPeer {
                    speaker,
                    capture: (index == 1).then(SteppedCapture::default),
                    provider: index == 2,
                    provider_inputs: Vec::new(),
                    // The fourth side: speaker, capture, provider, privacy.
                    privacy: index == 3,
                    privacy_said_us: None,
                    source: (index == 3 && PRIVACY_SOURCE_EXPIRY.with(std::cell::Cell::get)).then(
                        || SourcePeer {
                            gap: crate::privacy_bus::SourceGap::new(
                                TEST_PRIVACY_GAP_RIDE.with(std::cell::Cell::get),
                            ),
                            gate: PrivacyGate::default(),
                            permitted: false,
                            granted: false,
                            failed: false,
                        },
                    ),
                    authorities: Vec::new(),
                    chunk_times: Vec::new(),
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
        stepped = Some(shared);
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
                                other => panic!(
                                    "unexpected command to a head-touch fixture worker: {other:?}"
                                ),
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
    if let Some((storage, file)) = RIG_ONBOARDING.with(|walk| walk.borrow_mut().take()) {
        trace.onboarding_storage = Some(storage);
        trace.onboarding_test_file = Some(file);
    }
    // Made last, just before the loop first polls it: a touch is fresh for
    // 100 ms only. Stepped, the owner and its reply are made here too, inside
    // the scope, so starting the children cannot spend the owner's windows
    // (the reviewer and the seventh on #1207).
    let (mut owner, mut reply) = start();
    let mut controls = Some(controls());
    let mut startup = StartupEvents::default();
    if WAKE_CAPTURE_FROM.with(std::cell::Cell::get).is_some() {
        stepped
            .as_ref()
            .expect("wake scenario uses stepped peers")
            .borrow_mut()[2]
            .peer
            .control
            .send(WorkerEvent::ProviderLink {
                link: super::ready_link(1, 1),
            })
            .unwrap();
        startup
            .poll(&mut rig.provider.channels, "provider")
            .unwrap();
        assert_eq!(startup.cloud.ready_session(), Some(1));
    }
    if capture_frame && let Some(shared) = &stepped {
        let at = monotonic_us();
        // The peers are in the order made above: the capture's is second.
        shared.borrow_mut()[1]
            .peer
            .data
            .send(silent_capture_frame(
                at,
                owner.snapshot(now()).unwrap().microphone_generation(),
                1,
            ))
            .unwrap();
    }
    let error = conversation_with_controls(
        &mut rig,
        &mut owner,
        Lifetime::Serve,
        &mut trace,
        startup,
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
    let (mut capture_authorities, mut chunk_times) = (Vec::new(), Vec::new());
    let (mut camera_authorities, mut speaker_authorities) = (Vec::new(), Vec::new());
    let (mut capture, mut provider_inputs) = (None, Vec::new());
    let with_camera = RIG_CAMERA.with(std::cell::Cell::get);
    for peer in threads {
        let (peer, peer_told, peer_chunks) = peer.join().unwrap();
        drop(peer);
        told.extend(peer_told);
        chunks.extend(peer_chunks);
    }
    let stepped_peers = stepped.map(|shared| shared.take()).unwrap_or_default();
    let last = stepped_peers.len().saturating_sub(1);
    for (index, peer) in stepped_peers.into_iter().enumerate() {
        assert!(peer.stopped, "mock peer did not receive Stop");
        // In the order made: speaker, capture, provider, privacy, the ring
        // if any, the camera if any.
        if index == 0 {
            speaker_authorities = peer.authorities.clone();
        }
        if index == 1 {
            capture_authorities = peer.authorities.clone();
            capture = peer.capture;
        }
        if with_camera && index == last {
            camera_authorities = peer.authorities.clone();
        }
        chunk_times.extend(peer.chunk_times);
        told.extend(peer.told);
        chunks.extend(peer.chunks);
        provider_inputs.extend(peer.provider_inputs);
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
        capture_authorities,
        chunk_times,
        speaker_authorities,
        camera_authorities,
        capture,
        provider_inputs,
        error: message,
        ended_us,
        trace,
        left,
        told,
        chunks,
        controls,
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

/// The first builder's 21:37 on #705, for #1592: a frame the capture gives
/// marks the run heard where the conversation reads it (`heard_a_frame`), so
/// its fault keeps the guard. The same run with no frame releases it.
#[test]
fn a_frame_the_conversation_reads_marks_the_run_heard() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!("lv-heard-loop-{}", monotonic_us()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    for frame in [true, false] {
        let guard = || {
            let mut controls =
                HandControls::with_test_guard("test-hand-clock", monotonic_us(), &record).unwrap();
            controls.arm_active().unwrap();
            controls
        };
        let mut ran = run_loop_ringed(
            guard,
            || (quiet_owner(), None),
            false,
            Clocking::Stepped,
            false,
            frame,
        );
        let released = ran
            .controls
            .as_mut()
            .unwrap()
            .release_never_listened()
            .unwrap();
        assert_eq!(released, !frame, "frame {frame}: {}", ran.error);
    }
    let _ = fs::remove_dir_all(&directory);
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
    answer_muted_tap(
        Some(&mut face),
        &Observation::default(),
        false,
        false,
        &mut trace,
    )
    .unwrap();
    answer_muted_tap(Some(&mut face), &tap, true, false, &mut trace).unwrap();
    assert_eq!(faces(&trace), 0, "no tap, or the tap that wakes it");
    answer_muted_tap(Some(&mut face), &tap, false, true, &mut trace).unwrap();
    assert_eq!(
        (faces(&trace), starts.get()),
        (0, 0),
        "not over a hand's sleep's face (#1362)"
    );
    answer_muted_tap(Some(&mut face), &tap, false, false, &mut trace).unwrap();
    answer_muted_tap(Some(&mut face), &tap, false, false, &mut trace).unwrap();
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
        false,
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
    let _clock =
        crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap();
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
    // A tap counts only while its head was read within `TOUCH_INPUT_TTL_US`
    // (100 ms) of the poll that takes it, and `head_tap()` stamps its tap
    // 420 ms back, before the controls are built. So this finds 0 where
    // the first poll comes late: in a debug build under qemu (the ARM64
    // cross image), and natively with a 40 ms hold after `head_tap()` (the
    // first builder on #705, 18 Oct). Native release and debug runs pass.
    assert_eq!(
        faces_for(PrivacyState::Muted),
        1,
        "the slide closed, the mute open (0 if the first poll came over \
         100 ms after the tap's last reading, as under qemu: see above)"
    );
    assert_eq!(faces_for(PrivacyState::Allowed), 0, "the slide open");
}

/// #1362: the muted wait's own closures, for a lamp a hand put to sleep,
/// as the privacy wait calls them. The sleep's face begins at the first
/// look, said once in the trace. With the privacy slide's route, the slide
/// read closed leaves the ring black and closed by the time the wait goes
/// on, so the slide's indication, started after it, is the one writer; no
/// face begins again in that wait, even once the slide opens.
#[test]
fn the_muted_wait_shows_a_hands_sleep_and_gives_the_ring_to_the_closed_slide() {
    use crate::privacy::PrivacyState;
    use crate::ring_asleep::{Asleep, SleepFace, TapFace};
    use crate::service_privacy::WaitOutcome;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    /// A ring that says whether a face has it open.
    struct Opened(Arc<AtomicBool>);
    impl lamp_ring::FrameSink for Opened {
        fn write_frame(&mut self, _: &lamp_ring::EncodedFrame) -> io::Result<()> {
            Ok(())
        }
    }
    impl Drop for Opened {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    let directory =
        std::env::temp_dir().join(format!("lamp-voice-sleep-face-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let controls = RefCell::new(Some(
        HandControls::with_test_asleep("test-hand-clock", monotonic_us(), &record).unwrap(),
    ));
    let reminders = RefCell::new(None);
    let open = Arc::new(AtomicBool::new(false));
    let starts = std::cell::Cell::new(0);
    // A face that opens the ring at once, so the test sees it open.
    let face = SleepFace::new(
        || {
            starts.set(starts.get() + 1);
            let flag = open.clone();
            Some(Asleep::tapped_for(
                move || {
                    flag.store(true, Ordering::SeqCst);
                    lamp_ring::asleep::AsleepRing::new(Opened(flag))
                },
                lamp_ring::ChannelCeiling::new(24).unwrap(),
                Duration::from_secs(30),
            ))
        },
        true,
    );
    let mut trace = SessionState::default();
    let mut closed_when_told = None;
    let outcome = muted_wait_pulsing(
        &mut trace,
        &mut None,
        &controls,
        &reminders,
        None::<TapFace<fn() -> Option<Asleep>>>,
        Some(face),
        None,
        None,
        None,
        |on_state, gate| {
            let boot = crate::process::new_boot().unwrap();
            on_state(PrivacyState::Allowed, boot, 1)?;
            for _ in 0..3 {
                gate()?;
            }
            let until = Instant::now() + Duration::from_secs(2);
            while !open.load(Ordering::SeqCst) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(open.load(Ordering::SeqCst), "the sleep's face has the ring");
            on_state(PrivacyState::Muted, boot, 2)?;
            closed_when_told = Some(!open.load(Ordering::SeqCst));
            for _ in 0..3 {
                gate()?;
            }
            on_state(PrivacyState::Allowed, boot, 3)?;
            gate()?;
            Ok(WaitOutcome::Stopped)
        },
    )
    .unwrap();
    assert_eq!(outcome, WaitOutcome::Stopped);
    assert_eq!(
        closed_when_told,
        Some(true),
        "closed before the slide's own"
    );
    let faces = trace
        .iter()
        .filter(|row| row["kind"] == "head_sleep_face")
        .count();
    assert_eq!((faces, starts.get()), (1, 1));
    // A plain mute, the person's or the app's, is no hand's sleep: no face
    // (the reviewer's P3 on #2460).
    let controls = RefCell::new(Some(
        HandControls::with_test_muted(
            "test-hand-clock",
            monotonic_us(),
            &directory.join("muted.json"),
        )
        .unwrap(),
    ));
    let plain_starts = std::cell::Cell::new(0);
    let face = SleepFace::new(
        || {
            plain_starts.set(plain_starts.get() + 1);
            None
        },
        true,
    );
    let mut trace = SessionState::default();
    let outcome = muted_wait_pulsing(
        &mut trace,
        &mut None,
        &controls,
        &reminders,
        None::<TapFace<fn() -> Option<Asleep>>>,
        Some(face),
        None,
        None,
        None,
        |on_state, gate| {
            on_state(
                PrivacyState::Allowed,
                crate::process::new_boot().unwrap(),
                1,
            )?;
            for _ in 0..3 {
                assert!(!gate()?, "still muted");
            }
            Ok(WaitOutcome::Stopped)
        },
    )
    .unwrap();
    assert_eq!(outcome, WaitOutcome::Stopped);
    assert_eq!(plain_starts.get(), 0);
    assert!(trace.iter().all(|row| row["kind"] != "head_sleep_face"));
    let _ = fs::remove_dir_all(&directory);
    // Shown, and yielding to a closing slide only on the route, where the
    // slide's indication is the ring's other writer.
    for (head_touch, asleep_on, privacy_controls, shown) in [
        (true, false, false, Some(false)),
        (true, false, true, Some(true)),
        (true, true, true, Some(true)),
        (true, true, false, None),
        (false, false, false, None),
        (false, false, true, None),
    ] {
        assert_eq!(
            sleep_face_allowed(head_touch, asleep_on, privacy_controls),
            shown,
            "{head_touch} {asleep_on} {privacy_controls}"
        );
    }
}

/// The first builder on #2506 (#1396): while the Controls route reads the
/// switch again because each fresh worker ends at its start, the retry
/// tells the wait the switch reads unavailable, and once that has lasted
/// `WAIT_UNREADABLE_AFTER_US` the health says the microphones are off until
/// it can be read, so the wait shows outside the journal.
#[test]
fn the_muted_wait_says_the_switch_cannot_be_read_while_workers_end_at_start() {
    let _clock =
        crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap();
    use crate::privacy::PrivacyState;
    use crate::service_privacy::WaitOutcome;
    use lamp_msgs::{Diagnostic, Level};
    let root = std::path::PathBuf::from(format!("/tmp/lvw-unread-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let mut hears = bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
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
            // As `wait_selected`'s retry says it, since a while ago.
            let since = monotonic_us() - crate::bus_out::WAIT_UNREADABLE_AFTER_US - 100_000;
            on_state(
                PrivacyState::Unavailable,
                crate::process::new_boot().unwrap(),
                since,
            )?;
            gate()?;
            Ok(WaitOutcome::Stopped)
        },
    )
    .unwrap();
    assert_eq!(outcome, WaitOutcome::Stopped);
    let mut said = Vec::new();
    while let Some(message) = hears.recv(std::time::Duration::from_millis(50)).unwrap() {
        let report = message.parse::<Diagnostic>().unwrap();
        said.push((report.level, report.message));
    }
    assert!(
        said.contains(&(
            Level::Warn,
            "the microphones are off until the privacy switch can be read".to_owned()
        )),
        "{said:?}"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// The reviewer-lead's P3 on #2549: the light a hand's sleep keeps on is
/// read from the bus the voice has; with none, or nothing said there, it is
/// none, and the sleep shows the asleep face.
#[test]
fn a_hands_sleep_takes_the_persons_light_from_the_bus() {
    assert_eq!(sleep_light(None), None);
    let root = std::env::temp_dir().join(format!("lv-sleep-light-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = lamp_bus::Bus::new(&root).unwrap();
    assert_eq!(sleep_light(Some(&bus)), None, "nothing said");
    let mut ring = bus
        .publisher_declared(
            lamp_msgs::Light::TOPICS[1],
            &lamp_msgs::Light::declaration(),
            crate::ring_light::SOURCE,
        )
        .unwrap();
    ring.publish_json(
        1,
        &lamp_msgs::Light {
            on: true,
            colour: "warm".into(),
            level: 3,
        },
    )
    .unwrap();
    assert_eq!(
        sleep_light(Some(&bus)),
        crate::ring_face::Lit::of(true, "warm", 3)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The loop on the Controls privacy route with the switch read open every
/// 5 ms for `fresh_us`, silent in `silent` (from the loop's start), closed
/// from `closed_from` if given, and `LAMP_PRIVACY_GAP_RIDE` `ride`, from a
/// reply under way (#2602). What it left, and when it began.
fn gap_run(ride: bool, fresh_us: u64, silent: (u64, u64), closed_from: Option<u64>) -> (Ran, u64) {
    let began = std::cell::Cell::new(0);
    // Read as the rig and the loop are made, before the controls are.
    RIG_PRIVACY_CONTROLS.with(|on| on.set(true));
    TEST_PRIVACY_GAP_RIDE.with(|on| on.set(ride));
    let ran = run_loop_with(
        // A tap long gone: nothing the controls ask.
        || {
            let now = monotonic_us();
            began.set(now);
            PRIVACY_FRESH_UNTIL.with(|until| until.set(Some(now + fresh_us)));
            PRIVACY_SILENT.with(|span| span.set(Some((now + silent.0, now + silent.1))));
            PRIVACY_CLOSED_FROM.with(|from| from.set(closed_from.map(|at| now + at)));
            HandControls::with_test_tap("test-hand-clock", now - 10_000_000).unwrap()
        },
        || {
            let (owner, current, _) = active_reply();
            (owner, Some(current))
        },
        false,
        Clocking::Stepped,
    );
    RIG_PRIVACY_CONTROLS.with(|on| on.set(false));
    TEST_PRIVACY_GAP_RIDE.with(|on| on.set(false));
    PRIVACY_FRESH_UNTIL.with(|until| until.set(None));
    PRIVACY_SILENT.with(|span| span.set(None));
    PRIVACY_CLOSED_FROM.with(|from| from.set(None));
    (ran, began.get())
}

/// A privacy gap's mark in the trace: its time and how long it lasted.
fn gap_marks(ran: &Ran, state: &str) -> Vec<(u64, Option<u64>)> {
    ran.trace
        .iter()
        .filter(|row| row["kind"] == "privacy_gap" && row["state"] == state)
        .map(|row| (row["at_us"].as_u64().unwrap(), row["lasted_us"].as_u64()))
        .collect()
}

/// Eighth's 75c9 finding on #2602: the real worker's expiry rule must feed
/// the ride instead of ending this conversation before recovery is seen.
#[test]
fn the_workers_own_expiry_rides_a_sixty_millisecond_silence_in_one_conversation() {
    PRIVACY_SOURCE_EXPIRY.with(|on| on.set(true));
    let (ran, began) = gap_run(true, 900_000, (300_000, 360_000), None);
    PRIVACY_SOURCE_EXPIRY.with(|on| on.set(false));
    let started = gap_marks(&ran, "began");
    let reopened = gap_marks(&ran, "reopened");
    assert_eq!(started.len(), 2, "{}", ran.error);
    assert_eq!(reopened.len(), 1, "{}", ran.error);
    let gap_at = started[0].0;
    let open_at = reopened[0].0;
    assert!((began + 340_000..began + 360_000).contains(&gap_at));
    assert!((began + 420_000..began + 440_000).contains(&open_at));
    assert!(ran.left.is_none(), "the cancelled reply was not resumed");
    assert!(
        ran.chunk_times
            .iter()
            .all(|at| !(gap_at..open_at).contains(at))
    );
    assert!(
        ran.capture_authorities
            .iter()
            .all(|at| !(gap_at..open_at).contains(at))
    );
    assert!(ran.capture_authorities.iter().any(|at| *at >= open_at));
    assert_eq!(gap_marks(&ran, "ended").len(), 1);
    assert!(
        ran.error.contains("privacy switch unreadable"),
        "{}",
        ran.error
    );
    assert!(
        ran.ended_us
            <= began + 900_000 + crate::privacy::PRIVACY_FRESH_US + crate::privacy_gap::RIDE_US
    );
}

#[test]
fn without_the_ride_the_workers_own_expiry_still_ends_the_conversation() {
    PRIVACY_SOURCE_EXPIRY.with(|on| on.set(true));
    let (ran, began) = gap_run(false, 900_000, (300_000, 360_000), None);
    PRIVACY_SOURCE_EXPIRY.with(|on| on.set(false));
    assert!(gap_marks(&ran, "began").is_empty());
    assert!(gap_marks(&ran, "reopened").is_empty());
    assert!(ran.ended_us < began + 360_000, "{}", ran.error);
}

/// #2602 (finding 3, the first builder's (B) at 17:4x;
/// `LAMP_PRIVACY_GAP_RIDE=on`): a 300 ms gap in the switch's readings with
/// the switch unchanged doesn't end the conversation. From the lapse the
/// reply under way is cut and not resumed, the capture's authority isn't
/// renewed (its lease lapses), and nothing is said; a fresh reading that
/// says open renews it and the conversation goes on. A later gap that
/// outlasts a second ends it as before.
#[test]
fn a_short_gap_in_the_switchs_readings_is_ridden_and_the_conversation_goes_on() {
    let (ran, began) = gap_run(true, 1_500_000, (300_000, 600_000), None);
    let started = gap_marks(&ran, "began");
    let reopened = gap_marks(&ran, "reopened");
    let ended = gap_marks(&ran, "ended");
    assert_eq!(started.len(), 2, "two gaps: {:?}", started);
    assert_eq!(reopened.len(), 1, "{reopened:?}");
    let (gap_at, _) = started[0];
    let (open_at, lasted) = reopened[0];
    // Noticed within the 50 ms freshness of the last reading at 300 ms, and
    // open again once the readings after 600 ms are steady: the gate's
    // 60 ms, as for the first grant.
    assert!(
        (began + 300_000..began + 400_000).contains(&gap_at),
        "{}",
        gap_at - began
    );
    assert!(
        (began + 660_000..began + 700_000).contains(&open_at),
        "{}",
        open_at - began
    );
    assert_eq!(lasted, Some(open_at - gap_at));
    // The reply under way was cut there, and nothing was said in the gap.
    assert!(
        ran.trace
            .iter()
            .any(|row| row["kind"] == "turn_finished" && row["outcome"] == "privacy_gap"),
        "the reply cut"
    );
    assert!(ran.left.is_none(), "not resumed");
    // The speaker was told in the pass the gap began, not the next one.
    assert!(
        ran.speaker_authorities.contains(&gap_at),
        "the speaker told late: {:?}",
        ran.speaker_authorities
            .iter()
            .filter(|at| **at >= gap_at)
            .map(|at| at - gap_at)
            .take(3)
            .collect::<Vec<_>>()
    );
    assert!(
        ran.chunk_times
            .iter()
            .all(|at| !(gap_at..open_at).contains(at))
    );
    // The capture's lease lapsed: no authority reached it from the gap's
    // start to its reopen, and renewal came back with the fresh reading.
    assert!(
        ran.capture_authorities
            .iter()
            .all(|at| !(gap_at..open_at).contains(at)),
        "renewed in the gap: {:?}",
        ran.capture_authorities
            .iter()
            .filter(|at| (gap_at..open_at).contains(at))
            .map(|at| at - gap_at)
            .collect::<Vec<_>>()
    );
    assert!(ran.capture_authorities.iter().any(|at| *at >= open_at));
    // The second gap, after the readings stop at 1.5 s, lasted its second
    // and ended the conversation as before.
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert!(
        ended[0].1.unwrap() >= crate::privacy_gap::RIDE_US,
        "{ended:?}"
    );
    assert!(
        ran.trace
            .iter()
            .any(|row| row["kind"] == "privacy_gap" && row["why"] == "no fresh reading")
    );
    assert!(ran.error.contains("freshness lost"), "{}", ran.error);
}

/// #2602, the first builder's P1s on #2623: with the rig's camera and the
/// eyes' link both there, a ridden gap gives neither a yes. The eyes are
/// told no as it begins and hear no yes until it reopens; the camera gets
/// no authority from its start to its reopen; the loop gives both it again
/// after.
#[test]
fn a_ridden_gap_gives_the_camera_and_the_eyes_nothing_until_it_reopens() {
    let said = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let nobody = std::path::PathBuf::from(format!("/tmp/lvw-eyes-{}.sock", std::process::id()));
    TEST_EYES.with(|eyes| {
        *eyes.borrow_mut() = Some(
            crate::body_link::EyesLink::to(nobody)
                .unwrap()
                .logging(said.clone()),
        )
    });
    RIG_CAMERA.with(|on| on.set(true));
    let (ran, _) = gap_run(true, 1_500_000, (300_000, 600_000), None);
    RIG_CAMERA.with(|on| on.set(false));
    TEST_EYES.with(|eyes| eyes.borrow_mut().take());
    let (gap_at, _) = gap_marks(&ran, "began")[0];
    let (open_at, _) = gap_marks(&ran, "reopened")[0];
    let said = said.lock().unwrap().clone();
    let in_gap = |at: &u64| (gap_at..open_at).contains(at);
    assert!(
        said.iter().any(|(yes, at)| *yes && *at < gap_at),
        "{said:?}"
    );
    assert!(
        said.iter().any(|(yes, at)| !*yes && *at == gap_at),
        "no at the gap's start: {said:?}"
    );
    assert!(
        said.iter().all(|(yes, at)| !*yes || !in_gap(at)),
        "a yes in the gap: {said:?}"
    );
    assert!(
        said.iter().any(|(yes, at)| *yes && *at >= open_at),
        "{said:?}"
    );
    assert!(
        ran.camera_authorities.iter().any(|at| *at < gap_at),
        "the camera was given it before: {:?}",
        ran.camera_authorities
    );
    assert!(
        ran.camera_authorities.iter().all(|at| !in_gap(at)),
        "the camera in the gap: {:?}",
        ran.camera_authorities
            .iter()
            .filter(|at| in_gap(at))
            .map(|at| at - gap_at)
            .collect::<Vec<_>>()
    );
    // Renewed again after the reopen (a real camera given none for 250 ms
    // has ended itself by then: `WORKER_INITIAL_AUTHORITY_US`).
    assert!(ran.camera_authorities.iter().any(|at| *at >= open_at));
    assert!(
        !ran.trace
            .iter()
            .any(|row| row["kind"] == "camera_unavailable"),
        "the camera stayed up until the gap"
    );
}

/// #2602 (the first builder's P3s on #2623, 19:3x): the privacy worker
/// failing in a ridden gap ends it as every exit in a gap does, marked
/// `ended` with why.
#[test]
fn a_privacy_worker_failing_in_a_ridden_gap_marks_its_end() {
    PRIVACY_FAULT_IN_GAP.with(|fault| fault.set(true));
    let (ran, _) = gap_run(true, 1_500_000, (300_000, 600_000), None);
    PRIVACY_FAULT_IN_GAP.with(|fault| fault.set(false));
    assert_eq!(gap_marks(&ran, "began").len(), 1);
    assert!(gap_marks(&ran, "reopened").is_empty());
    assert_eq!(gap_marks(&ran, "ended").len(), 1, "{}", ran.error);
    assert!(
        ran.trace
            .iter()
            .any(|row| row["kind"] == "privacy_gap" && row["why"] == "privacy worker failed")
    );
    assert!(
        ran.error.contains("privacy worker fault (test)"),
        "{}",
        ran.error
    );
}

/// #2602 (the first builder's P3 on #2623): the microphone's retention
/// heartbeat counts from a ridden gap's reopen, so a reopen near the gap's
/// second leaves capture its whole 1.5 s to come back.
#[test]
fn the_retention_heartbeat_counts_from_a_gaps_reopen() {
    let last = 1_000_000;
    let reopened = last + 950_000;
    assert!(!retention_expired(None, reopened, u64::MAX));
    assert!(!retention_expired(Some(last), 0, last + CAPTURE_GAP_US - 1));
    assert!(retention_expired(Some(last), 0, last + CAPTURE_GAP_US));
    assert!(!retention_expired(
        Some(last),
        reopened,
        last + CAPTURE_GAP_US
    ));
    assert!(!retention_expired(
        Some(last),
        reopened,
        reopened + CAPTURE_GAP_US - 1
    ));
    assert!(retention_expired(
        Some(last),
        reopened,
        reopened + CAPTURE_GAP_US
    ));
    // A frame retained after the reopen counts from itself.
    let later = reopened + 10_000;
    assert!(!retention_expired(
        Some(later),
        reopened,
        later + CAPTURE_GAP_US - 1
    ));
}

/// #2602: off, the same gap ends the conversation at its lapse, as always.
#[test]
fn without_the_switch_a_gap_in_the_readings_ends_the_conversation_as_before() {
    let (ran, began) = gap_run(false, 1_500_000, (300_000, 600_000), None);
    assert!(gap_marks(&ran, "began").is_empty());
    assert!(ran.error.contains("freshness lost"), "{}", ran.error);
    assert!(
        (began + 300_000..began + 400_000).contains(&ran.ended_us),
        "{}",
        ran.ended_us - began
    );
}

/// #2602: a fresh reading that says closed in a ridden gap is the physical
/// mute, as before: never reopened.
#[test]
fn a_closed_switch_read_in_a_ridden_gap_is_the_mute() {
    let (ran, _) = gap_run(true, 1_500_000, (300_000, 500_000), Some(500_000));
    assert_eq!(gap_marks(&ran, "began").len(), 1);
    assert!(gap_marks(&ran, "reopened").is_empty());
    // Its end is marked as every exit in a gap is.
    assert_eq!(gap_marks(&ran, "ended").len(), 1);
    assert!(
        ran.trace
            .iter()
            .any(|row| row["kind"] == "privacy_gap" && row["why"] == "read closed")
    );
    assert!(
        ran.trace.iter().any(|row| row["kind"] == "physical_mute"),
        "the mute: {}",
        ran.error
    );
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
        false,
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
    let refused = run_loop_ringed(
        head_tap,
        still_thinking,
        false,
        Clocking::Stepped,
        true,
        false,
    );
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

/// A hand on the head `held_us` in, lifted at `lifted_us`, run through the
/// loop with the privacy switch kept fresh for `fresh_us` (#1362): what the
/// loop left, and the scenario time Controls' sleep would come at if the
/// hand stays.
fn held_run(held_us: u64, lifted_us: u64, fresh_us: u64) -> (Ran, u64) {
    use std::os::unix::fs::PermissionsExt;
    // One folder each: the stepped tests share their scenario clock's start.
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("lv-sleep-breath-{}-{run}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let sleeps_at = std::cell::Cell::new(0);
    let ran = run_loop_with(
        || {
            let now = monotonic_us();
            sleeps_at.set(now + crate::head_cue::SLEEP_HOLD_US - held_us);
            PRIVACY_FRESH_UNTIL.with(|until| until.set((fresh_us > 0).then_some(now + fresh_us)));
            HandControls::with_test_hold("test-hand-clock", now, held_us, lifted_us, &record)
                .unwrap()
        },
        || (quiet_owner(), None),
        false,
        Clocking::Stepped,
    );
    PRIVACY_FRESH_UNTIL.with(|until| until.set(None));
    let _ = fs::remove_dir_all(&directory);
    (ran, sleeps_at.get())
}

/// The sleep's end of a held run, checked where Controls put it: the
/// conversation ends `head_sleep` within 4 ms of Controls' time, not one
/// step later for anything that plays.
fn slept_on_time(ran: &Ran, sleeps_at: u64) {
    assert!(ran.error.contains("software mute"), "{}", ran.error);
    let cancel = ran
        .trace
        .iter()
        .find(|row| row["kind"] == "hand_control_cancel")
        .expect("the sleep ends it");
    assert_eq!(cancel["reason"], "head_sleep");
    let at = cancel["at_us"].as_u64().unwrap();
    assert!(
        (sleeps_at..sleeps_at + 4_000).contains(&at),
        "slept {} us after Controls' time",
        at as i64 - sleeps_at as i64
    );
}

/// #1362 (the first builder on #1362, 21 Oct 10:35): a hand kept on the
/// head into the hold's last second gets the falling breath, once, as a
/// turn of its own; and the breath never moves the sleep. Begun 2.15 s in,
/// it still plays at three seconds, and Controls' sleep then ends the
/// conversation on the pass that reaches it, the breath cut there, so the
/// microphones close when they would without it.
#[test]
fn a_hand_kept_on_the_head_sighs_in_its_last_second_and_sleeps_on_time() {
    let (ran, sleeps_at) = held_run(2_150_000, 3_200_000, 1_500_000);
    let breaths: Vec<_> = ran
        .trace
        .iter()
        .filter(|row| row["kind"] == "head_sleep_breath")
        .collect();
    assert_eq!(breaths.len(), 1, "once a hold: {breaths:?}");
    assert_eq!(breaths[0]["plays"], true, "{}", breaths[0]);
    let held = breaths[0]["held_us"].as_u64().unwrap();
    assert!(crate::head_cue::sighs_at(held), "{held}");
    let admitted = ran
        .trace
        .iter()
        .find(|row| row["kind"] == "input_admitted")
        .expect("its own turn");
    assert_eq!(admitted["admission_basis"], "head_sleep_breath");
    let breath = crate::head_cue::breath(crate::head_cue::DBFS);
    let first = ran.chunks.first().expect("the breath reached the speaker");
    assert_eq!(first.samples, breath[..240]);
    slept_on_time(&ran, sleeps_at);
    assert!(
        ran.trace
            .iter()
            .any(|row| row["kind"] == "turn_finished" && row["outcome"] == "head_sleep"),
        "the breath's turn ended by the sleep"
    );
}

/// #1362 (the reviewer's P3 on #2487): a hand first seen too late in the
/// hold for most of the breath to fit gets none, never one cut to a tick,
/// and the sleep comes on time.
#[test]
fn a_hand_seen_late_in_the_hold_gets_no_breath_and_sleeps_on_time() {
    let (ran, sleeps_at) = held_run(2_970_000, 3_200_000, 0);
    assert!(
        ran.trace
            .iter()
            .all(|row| row["kind"] != "head_sleep_breath"),
        "no breath"
    );
    assert!(ran.chunks.is_empty(), "nothing played");
    slept_on_time(&ran, sleeps_at);
}

/// #1362 (the reviewer's P3 on #2487): a hand lifted between two seconds
/// and three gets its sigh and no sleep: one breath, no mute, and the
/// conversation goes on until the test lets the switch go stale.
#[test]
fn a_hand_lifted_after_its_breath_sighs_and_stays_awake() {
    // The switch kept fresh past where the sleep would have come.
    let (ran, sleeps_at) = held_run(2_050_000, 2_500_000, 1_200_000);
    let breaths: Vec<_> = ran
        .trace
        .iter()
        .filter(|row| row["kind"] == "head_sleep_breath")
        .collect();
    assert_eq!(breaths.len(), 1, "{breaths:?}");
    assert_eq!(breaths[0]["plays"], true, "{}", breaths[0]);
    assert!(
        ran.trace
            .iter()
            .all(|row| row["kind"] != "hand_control_cancel"),
        "no sleep"
    );
    assert!(
        ran.error.contains("privacy") && !ran.error.contains("software mute"),
        "ended only by the stale switch: {}",
        ran.error
    );
    assert!(ran.ended_us > sleeps_at, "ran past the sleep's time");
}

/// An onboarding walk for the next loop (#3747) whose kept progress has
/// each of `ended` skipped: its storage started and loaded. The folder,
/// for the caller to remove.
fn walk_for_the_next_loop(ended: &[&str]) -> PathBuf {
    use crate::onboarding::{
        Kept, Outcome, Progress,
        storage::{Paths, ServiceStorage},
    };
    static WALKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let walk = WALKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("lv-held-walk-{}-{walk}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let file = directory.join("progress.json");
    let mut progress = Progress::default();
    for step in ended {
        progress.steps.insert(
            (*step).into(),
            Kept {
                outcome: Outcome::Skipped,
                at_unix_s: 1,
                framing: None,
                offered_at_unix_s: None,
                later_offers: 0,
            },
        );
    }
    fs::write(&file, serde_json::to_vec(&progress).unwrap()).unwrap();
    let storage = ServiceStorage::start(Paths {
        progress: file.clone(),
        checkpoint: directory.join("privacy.json"),
        mailbox: directory.join("mailbox"),
        near_call: directory.join("near-call.json"),
        room_sounds: directory.join("room-sounds.json"),
        room_sounds_group: None,
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !storage.client().snapshot().unwrap().ready {
        assert!(Instant::now() < deadline, "onboarding storage never loaded");
        std::thread::yield_now();
    }
    RIG_ONBOARDING.with(|walk| *walk.borrow_mut() = Some((storage, file)));
    directory
}

/// #3747 (the lead on #1711): while the onboarding walk is under way, a
/// fresh one waiting to begin or one between two steps, a hand kept on the
/// head from 2.15 s in to 3.5 s, past Controls' sleep at three, gets no
/// falling breath (its decision says why) and asks no sleep: nothing
/// played, the conversation going on until the test lets the switch go
/// stale. With every step ended, the same hold sighs and sleeps on time.
#[test]
fn a_hold_while_onboarding_is_under_way_gets_no_breath_and_never_sleeps() {
    for (label, ended) in [
        ("waiting to begin", &[][..]),
        ("between steps", &["where_you_sit"][..]),
    ] {
        let directory = walk_for_the_next_loop(ended);
        let (ran, sleeps_at) = held_run(2_150_000, 3_500_000, 1_800_000);
        let _ = fs::remove_dir_all(&directory);
        let breaths: Vec<_> = ran
            .trace
            .iter()
            .filter(|row| row["kind"] == "head_sleep_breath")
            .collect();
        assert_eq!(breaths.len(), 1, "{label}: {breaths:?}");
        assert_eq!(breaths[0]["plays"], false, "{label}: {}", breaths[0]);
        assert_eq!(breaths[0]["why"], "onboarding_under_way", "{label}");
        assert!(ran.chunks.is_empty(), "{label}: nothing played");
        assert!(
            ran.trace.iter().all(|row| {
                !(row["kind"] == "hand_control_cancel" && row["reason"] == "head_sleep")
            }),
            "{label}: no sleep"
        );
        assert!(
            ran.error.contains("privacy") && !ran.error.contains("software mute"),
            "{label}: ended only by the stale switch: {}",
            ran.error
        );
        assert!(
            ran.ended_us > sleeps_at,
            "{label}: ran past the sleep's time"
        );
    }
    let directory = walk_for_the_next_loop(&crate::onboarding::Step::IDS);
    let (ran, sleeps_at) = held_run(2_150_000, 3_200_000, 1_500_000);
    let _ = fs::remove_dir_all(&directory);
    let breaths: Vec<_> = ran
        .trace
        .iter()
        .filter(|row| row["kind"] == "head_sleep_breath")
        .collect();
    assert_eq!(breaths.len(), 1, "walk over: {breaths:?}");
    assert_eq!(breaths[0]["plays"], true, "walk over: {}", breaths[0]);
    slept_on_time(&ran, sleeps_at);
}

/// #1362: woken from a hand's sleep by the person, the conversation the
/// wake opened says the rising "hm?" first, once, as a turn of its own,
/// however lately it last hummed.
#[test]
fn woken_from_a_hands_sleep_the_lamp_says_hm_first() {
    let ran = run_loop_with(
        || HandControls::with_test_woken("test-hand-clock", monotonic_us() - 60_000).unwrap(),
        || (quiet_owner(), None),
        false,
        Clocking::Stepped,
    );
    let cues: Vec<_> = ran
        .trace
        .iter()
        .filter(|row| row["kind"] == "head_wake_cue")
        .collect();
    assert_eq!(cues.len(), 1, "once: {cues:?}");
    assert_eq!(cues[0]["plays"], true, "{}", cues[0]);
    let admitted = ran
        .trace
        .iter()
        .find(|row| row["kind"] == "input_admitted")
        .expect("its own turn");
    assert_eq!(admitted["admission_basis"], "head_wake_cue");
    assert!(admitted["provider_session"].is_null());
    let hum = crate::head_cue::hum(crate::head_cue::DBFS);
    let first = ran.chunks.first().expect("the hum reached the speaker");
    assert_eq!(first.samples, hum[..240]);
}

/// Wake with the real initial Unknown permission and privacy stabilization.
/// Only this fixture opts into capture: after Allowed and StartCapture, its
/// first fresh frame is withheld until `capture_after_us` from loop setup.
fn wake_run(
    capture_after_us: u64,
    closed_after_us: Option<u64>,
    controls: impl FnOnce(u64) -> HandControls,
) -> (Ran, u64) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            WAKE_CAPTURE_FROM.with(|from| from.set(None));
            WAKE_PRIVACY_START_PAUSE.with(|pause| pause.set(None));
            PRIVACY_FRESH_UNTIL.with(|until| until.set(None));
            PRIVACY_CLOSED_FROM.with(|from| from.set(None));
            PRIVACY_SILENT.with(|span| span.set(None));
            PRIVACY_FAULT_IN_GAP.with(|fault| fault.set(false));
        }
    }
    let _reset = Reset;
    let began = std::cell::Cell::new(0);
    let ran = run_loop_with(
        || {
            let at = monotonic_us();
            began.set(at);
            WAKE_CAPTURE_FROM.with(|from| from.set(Some(at + capture_after_us)));
            PRIVACY_FRESH_UNTIL.with(|until| until.set(Some(at + 2_000_000)));
            PRIVACY_CLOSED_FROM.with(|from| from.set(closed_after_us.map(|after| at + after)));
            controls(at)
        },
        || (Controller::new(new_boot().unwrap(), now()), None),
        false,
        Clocking::Stepped,
    );
    let (ready_index, ready) = ran
        .trace
        .iter()
        .enumerate()
        .find(|(_, row)| row["kind"] == "provider_ready")
        .expect("provider readiness consumed");
    assert_eq!(ready["link"]["session"], 1);
    let ready_at = ready["coordinator_observed_at_us"].as_u64().unwrap();
    assert_eq!(ready_at, began.get());
    for (index, cue) in ran
        .trace
        .iter()
        .enumerate()
        .filter(|(_, row)| row["kind"] == "head_wake_cue")
    {
        assert!(ready_index < index);
        assert!(ready_at <= cue["at_us"].as_u64().unwrap());
    }
    (ran, began.get())
}

fn pending_wake(at_us: u64) -> HandControls {
    HandControls::with_test_woken("test-hand-clock", at_us).unwrap()
}

fn no_wake_sound(ran: &Ran) {
    assert!(ran.chunks.is_empty(), "no sound sent to the speaker");
    assert!(ran.provider_inputs.is_empty(), "no provider request");
    assert!(
        ran.trace.iter().all(|row| {
            !(row["kind"] == "head_wake_cue" && row["plays"] == true
                || row["kind"] == "input_admitted" && row["admission_basis"] == "head_wake_cue")
        }),
        "no wake turn admitted"
    );
}

/// A suppressed pending wake has one terminal decision, including exits
/// that return before the ordinary cue block can run.
fn refused_wake(ran: &Ran, why: &str) -> u64 {
    no_wake_sound(ran);
    let cues: Vec<_> = ran
        .trace
        .iter()
        .filter(|row| row["kind"] == "head_wake_cue")
        .collect();
    assert_eq!(
        cues.len(),
        1,
        "one terminal decision: {cues:?}; {}",
        ran.error
    );
    assert_eq!(cues[0]["plays"], false, "{}", cues[0]);
    assert_eq!(cues[0]["why"], why, "{}", cues[0]);
    cues[0]["at_us"].as_u64().unwrap()
}

/// A saved hand sleep can be cleared by the button with head touch off.
/// Without any configured hum, consuming that wake creates no cue decision.
#[test]
fn a_wake_without_a_hum_leaves_no_cue_record() {
    let mut controls_sender = None;
    let (mut ran, began) = wake_run(0, Some(100_000), |at| {
        let (mut controls, sender) = HandControls::with_test_sender("test-hand-clock", at).unwrap();
        controls_sender = Some(sender);
        assert!(!controls.head_touch());
        controls.set_woken_for_test(at);
        controls
    });
    assert!(
        controls_sender.is_some(),
        "the Controls channel stayed open"
    );
    no_wake_sound(&ran);
    let capture = ran.capture.as_ref().expect("capture peer");
    assert!(capture.granted_at_us.is_some(), "permission became ready");
    assert!(capture.started_at_us.is_some(), "capture started");
    assert!(!capture.frames.is_empty(), "capture produced fresh frames");
    assert!(
        (100_000..2_000_000).contains(&(ran.ended_us - began)),
        "the loop ended before marker expiry: {} us, {}",
        ran.ended_us - began,
        ran.error
    );
    assert!(ran.trace.iter().any(|row| row["kind"] == "physical_mute"));
    assert_eq!(
        ran.controls.as_mut().unwrap().take_woken(ran.ended_us),
        None,
        "the marker was consumed before its two-second freshness expired"
    );
    assert!(
        ran.trace.iter().all(|row| row["kind"] != "head_wake_cue"),
        "an unconfigured hum has no decision row: {:?}",
        ran.trace
            .iter()
            .filter(|row| row["kind"] == "head_wake_cue")
            .collect::<Vec<_>>()
    );
}

/// #2487: the first pass has no microphone permission. Keep the wake pending
/// through the physical switch's real 60 ms stabilization, then require a
/// retained frame from the new microphone generation before saying its hm.
#[test]
fn a_wake_waits_for_microphone_permission_and_fresh_capture_before_one_hm() {
    let (ran, began) = wake_run(0, None, pending_wake);
    let initial = ran
        .told
        .first()
        .expect("initial closed authority published");
    assert_eq!(initial.microphone_permission(), Permission::Unknown);
    assert!(!initial.listening_ready(initial.issued_at()));
    let capture = ran.capture.as_ref().unwrap();
    let granted = capture.granted_at_us.expect("microphone opened");
    let started = capture.started_at_us.expect("capture explicitly started");
    let (retained, generation) = *capture.frames.first().expect("fresh capture returned");
    assert!(granted >= began + crate::privacy::REOPEN_STABLE_US);
    assert!(started >= granted && retained >= started);
    assert!(generation > initial.microphone_generation());
    assert!(capture.frames.iter().all(|(_, given)| *given == generation));

    let cues: Vec<_> = ran
        .trace
        .iter()
        .filter(|row| row["kind"] == "head_wake_cue")
        .collect();
    assert_eq!(cues.len(), 1, "one decision: {cues:?}; {}", ran.error);
    assert_eq!(cues[0]["plays"], true, "{}", cues[0]);
    assert_eq!(cues[0]["acquired_at_us"], began);
    let decided = cues[0]["at_us"].as_u64().unwrap();
    assert!(decided >= retained);
    let admissions: Vec<_> = ran
        .trace
        .iter()
        .filter(|row| row["kind"] == "input_admitted")
        .collect();
    assert_eq!(admissions.len(), 1, "one local turn: {admissions:?}");
    assert_eq!(admissions[0]["admission_basis"], "head_wake_cue");
    assert!(admissions[0]["provider_session"].is_null());
    assert!(ran.provider_inputs.is_empty(), "no provider data sent");
    let first = ran.chunks.first().expect("the hm reached the speaker");
    assert_eq!(
        first.samples,
        crate::head_cue::hum(crate::head_cue::DBFS)[..240]
    );
    let dispatched = *ran.chunk_times.first().unwrap();
    assert!(dispatched >= decided);
    // This is scenario time to the local socket, not ALSA/acoustic latency.
    eprintln!(
        "wake fixture: permission={} us, capture={} us, decision={} us, first speaker send={} us",
        granted - began,
        retained - began,
        decided - began,
        dispatched - began
    );
    assert!(dispatched - began <= 500_000, "{} us", dispatched - began);
}

/// #3021: one pass waits with Unknown permission, then the coordinator is
/// stalled from 2 ms to the original 1.5 s deadline. Its first Open reading
/// is acquired after that stall and is still stabilizing at the next pass.
/// This does not extend the one-second startup-unavailable grace: no loop
/// passes run during the pause, and its next reading is genuinely fresh.
#[test]
fn a_wake_expires_while_permission_is_unavailable_and_later_open_cannot_revive_it() {
    let within = crate::head_cue::WAKE_WITHIN_US;
    let (ran, began) = wake_run(0, None, |at| {
        WAKE_PRIVACY_START_PAUSE.with(|pause| pause.set(Some((at + 2_000, at + within))));
        pending_wake(at)
    });
    let decided = refused_wake(&ran, "wake_window_expired");
    assert_eq!(decided, began + within + 2_000);
    let before_decision: Vec<_> = ran
        .told
        .iter()
        .filter(|state| state.issued_at().as_micros() <= decided)
        .collect();
    assert!(!before_decision.is_empty());
    assert!(
        before_decision
            .iter()
            .all(|state| state.microphone_permission() == Permission::Unknown),
        "permission was never allowed before the expiry decision"
    );
    assert!(ran.trace.iter().all(|row| row["kind"] != "physical_mute"));
    let capture = ran.capture.as_ref().unwrap();
    let granted = capture.granted_at_us.expect("fresh Open later stabilized");
    assert!(granted >= began + within + crate::privacy::REOPEN_STABLE_US);
    assert!(granted > decided);
    let started = capture.started_at_us.expect("capture later started");
    assert!(started >= granted);
    let first_frame = capture
        .frames
        .first()
        .expect("fresh capture later arrived")
        .0;
    assert!(first_frame >= started && ran.ended_us > first_frame);
}

/// A slide closed throughout, closed during stabilization, or closed after
/// permission while capture is still unavailable cancels the pending cue.
#[test]
fn a_wake_never_speaks_with_privacy_closed_or_closed_while_waiting() {
    for closed_after_us in [0, 40_000, 100_000] {
        let (mut ran, began) = wake_run(500_000, Some(closed_after_us), pending_wake);
        assert_eq!(refused_wake(&ran, "conversation_ended"), ran.ended_us);
        let muted = ran
            .trace
            .iter()
            .find(|row| row["kind"] == "physical_mute")
            .expect("the physical switch ends this conversation");
        assert!(muted["at_us"].as_u64().unwrap() >= began + closed_after_us);
        let capture = ran.capture.as_ref().unwrap();
        assert!(capture.frames.is_empty());
        if closed_after_us < crate::privacy::REOPEN_STABLE_US {
            assert!(capture.granted_at_us.is_none());
            assert!(capture.started_at_us.is_none());
            assert!(
                ran.told
                    .iter()
                    .all(|state| state.microphone_permission() != Permission::Allowed)
            );
        } else {
            assert!(capture.granted_at_us.is_some());
            assert!(capture.started_at_us.is_some());
        }
        assert!(
            ran.controls
                .as_mut()
                .unwrap()
                .take_woken(ran.ended_us)
                .is_none()
        );
    }
}

/// A sleep queued while the cue waits for permission still wins on its own
/// pass. The old wake cannot survive this conversation's cancellation.
#[test]
fn returning_to_sleep_cancels_a_wake_waiting_for_permission() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!(
        "lv-pending-wake-sleep-{}-{}",
        std::process::id(),
        lamp_ipc::monotonic_us()
    ));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let record = directory.join("mute.json");
    let (mut ran, began) = wake_run(0, None, |at| {
        let mut controls =
            HandControls::with_test_hold("test-hand-clock", at, 2_970_000, 3_200_000, &record)
                .unwrap();
        controls.set_woken_for_test(at);
        controls
    });
    slept_on_time(&ran, began + 30_000);
    assert_eq!(refused_wake(&ran, "conversation_ended"), ran.ended_us);
    let capture = ran.capture.as_ref().unwrap();
    assert!(capture.granted_at_us.is_none() && capture.frames.is_empty());
    assert!(
        ran.controls
            .as_mut()
            .unwrap()
            .take_woken(ran.ended_us)
            .is_none()
    );
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn a_privacy_worker_fault_reports_a_pending_wake_once() {
    let (ran, began) = wake_run(0, None, |at| {
        // The existing worker-fault seam fires 100 ms into this gap: 20 ms
        // after startup here, before the no-reading freshness bound.
        PRIVACY_SILENT.with(|span| span.set(Some((at - 80_000, at + 200_000))));
        PRIVACY_FAULT_IN_GAP.with(|fault| fault.set(true));
        pending_wake(at)
    });
    assert!(
        ran.error.contains("privacy worker fault (test)"),
        "{}",
        ran.error
    );
    assert_eq!(refused_wake(&ran, "conversation_ended"), ran.ended_us);
    assert!((began + 20_000..began + 50_000).contains(&ran.ended_us));
    let capture = ran.capture.as_ref().unwrap();
    assert!(capture.granted_at_us.is_none() && capture.frames.is_empty());
}

/// Capture becoming ready near the bound can speak. A first frame at the
/// deadline or later cannot revive the wake even though capture subsequently
/// stays fresh. The pure gate tests pin readiness at the exact microsecond.
#[test]
fn a_wakes_capture_readiness_must_arrive_before_its_original_deadline() {
    let within = crate::head_cue::WAKE_WITHIN_US;
    for (capture_after_us, plays) in [
        (within - 20_000, true),
        (within, false),
        (within + 20_000, false),
    ] {
        let (ran, began) = wake_run(capture_after_us, None, pending_wake);
        let capture = ran.capture.as_ref().unwrap();
        assert_eq!(capture.frames.first().unwrap().0, began + capture_after_us);
        let cues: Vec<_> = ran
            .trace
            .iter()
            .filter(|row| row["kind"] == "head_wake_cue")
            .collect();
        assert_eq!(cues.len(), 1, "{cues:?}; {}", ran.error);
        assert_eq!(cues[0]["plays"], plays, "{}", cues[0]);
        let decided = cues[0]["at_us"].as_u64().unwrap();
        if plays {
            assert!(decided >= began + capture_after_us);
            assert!(decided < began + within);
            assert!(!ran.chunks.is_empty());
        } else {
            assert_eq!(decided, began + within);
            assert_eq!(cues[0]["why"], "wake_window_expired");
            no_wake_sound(&ran);
            assert!(ran.ended_us > began + capture_after_us);
        }
        assert!(ran.provider_inputs.is_empty());
    }
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

/// #2233: with `LAMP_CAMERA_FOLLOWS_EYES=on`, a voice whose camera is the
/// eyes' permits it with the microphone, and a mute by hand denies it with
/// the microphone, as the eyes are told no at once.
#[test]
fn the_eyes_camera_follows_the_microphone_and_a_mute_denies_it() {
    let directory = SessionDirectory::create().unwrap();
    let (mut speaker, mut peer) = pair(&directory, "speaker");
    let (mut owner, mut reply) = current_input();
    super::super::eyes_look_with_the_microphone(&mut owner, true, false).unwrap();
    assert!(owner.snapshot(now()).unwrap().camera_allowed(now()));
    let observed = monotonic_us();
    hand::cancel(
        &mut owner,
        &mut reply,
        &mut speaker.control,
        &mut None,
        &mut VecDeque::new(),
        &mut InputAdmission::default(),
        &mut TurnDetector::default(),
        true,
        observed,
        || observed,
    )
    .unwrap();
    let received = received_authority(&mut peer);
    assert_eq!(received.camera_permission(), Permission::Denied);
    assert_eq!(received.microphone_permission(), Permission::Denied);
}
