//! Stepped local ownership/quiet boundaries, not acoustic measurements.
use super::*;

fn provider_boot() -> BootId {
    BootId::new([1; 16]).unwrap()
}

struct Test {
    owner: Controller,
    reply: Option<Reply>,
    queue: VecDeque<ProviderInput>,
    trace: SessionState,
    pending: holding::Continuation,
    admission: InputAdmission,
    playback: PlaybackMask,
}

impl Test {
    fn new(enabled: bool) -> Self {
        let (mut owner, mut reply, _) = active_reply();
        let request = reply.owner.turn();
        let mut pending = holding::Continuation::new(enabled);
        pending.offer(request, reply.cloud.session, provider_boot());
        // Finish the fixture's accepted PCM and provider response through
        // the same owner boundaries the live loop joins before scheduling.
        owner
            .playback_ended(now(), reply.playback.take().unwrap())
            .unwrap();
        reply.pcm.clear();
        reply.cloud.complete(true).unwrap();
        reply.provider_idle = true;
        reply.generation_done = true;
        assert!(reply.settled(&None));
        owner.complete_turn(now(), reply.owner).unwrap();
        pending.settled(request, true, monotonic_us());
        Self {
            owner,
            reply: None,
            queue: VecDeque::new(),
            trace: SessionState::default(),
            pending,
            admission: InputAdmission::default(),
            playback: PlaybackMask::new(TurnTaking::V1Baseline),
        }
    }

    fn start(
        &mut self,
        age_us: u64,
        capture_age: Option<u64>,
        session: Option<u64>,
        speaker_pending: bool,
        draining: bool,
    ) -> bool {
        let at = now();
        let until = at.checked_add(INPUT_LEASE_US).unwrap();
        if self.owner.snapshot(at).unwrap().microphone_permission() == Permission::Allowed {
            self.owner
                .set_capture(at, CaptureState::RetainingUntil(until))
                .unwrap();
            self.owner
                .set_admission(at, AdmissionState::OpenUntil(until))
                .unwrap();
        }
        let snapshot = self.owner.snapshot(at).unwrap();
        holding::start(
            &mut self.pending,
            holding::Gates {
                at_us: at.as_micros(),
                cycle_started_us: 0,
                last_speech_us: at.as_micros() - age_us,
                last_capture_us: capture_age.map(|age| at.as_micros() - age),
                session,
                worker: provider_boot(),
                snapshot: &snapshot,
                admission: &self.admission,
                playback: &self.playback,
                speaker_pending,
                submission_draining: draining,
            },
            &mut self.owner,
            &mut self.reply,
            &mut self.queue,
            PlayoutStart::default(),
            &mut self.trace,
            |_| Ok(()),
        )
        .unwrap()
        .is_some()
    }
}

fn clock() -> crate::scenario_clock::Scope {
    let clock = crate::scenario_clock::Scope::enter(Duration::from_secs(10)).unwrap();
    let mut first = true;
    clock
        .set_pump(move |phase| {
            if first && matches!(phase, crate::scenario_clock::Phase::Run) {
                first = false;
                crate::scenario_clock::elapse_for_fault(Duration::from_secs(2))?;
            }
            Ok(())
        })
        .unwrap();
    crate::scenario_clock::step(crate::scenario_clock::Phase::Run).unwrap();
    clock
}

#[test]
fn holding_continuation_waits_a_full_second_then_queues_one_note_on_the_same_session() {
    let _clock = clock();
    let mut test = Test::new(true);
    for quiet_us in [0, 999_999] {
        assert!(!test.start(quiet_us, Some(0), Some(1), false, false));
        assert!(test.queue.is_empty());
    }
    let eligible_at = monotonic_us();
    assert!(test.start(1_000_000, Some(0), Some(1), false, false));
    assert_eq!(
        monotonic_us(),
        eligible_at,
        "no timer or blocking wait added"
    );
    assert!(matches!(test.queue.front(), Some(ProviderInput::Written {
        request, session: 1, text, look: false, ..
    }) if *request == test.reply.as_ref().unwrap().owner.turn()
        && text == crate::holding_answer::NOTE));
    assert_eq!(test.queue.len(), 1);
    assert!(test.reply.as_ref().unwrap().cloud.same_session_only);
    let next = test.reply.as_ref().unwrap().owner.turn();
    test.pending.offer(next, 1, provider_boot());
    test.pending.settled(next, true, monotonic_us());
    assert!(
        test.pending
            .take(monotonic_us(), None, Some(1), provider_boot(), true)
            .is_none()
    );
    assert_eq!(
        test.trace
            .iter()
            .filter(|e| e["kind"] == "holding_answer_nudged")
            .count(),
        1
    );
}

#[test]
fn holding_continuation_off_keeps_queue_and_trace_unchanged() {
    let _clock = clock();
    let mut test = Test::new(false);
    assert!(!test.start(1_000_000, Some(0), Some(1), false, false));
    assert!(test.reply.is_none() && test.queue.is_empty() && test.trace.is_empty());
}

#[test]
fn holding_continuation_requires_fresh_capture_and_no_pending_audio_or_input() {
    let _clock = clock();
    for (capture, speaker, draining) in [
        (None, false, false),
        (Some(INPUT_LEASE_US + 1), false, false),
        (Some(0), true, false),
        (Some(0), false, true),
    ] {
        let mut test = Test::new(true);
        assert!(!test.start(1_000_000, capture, Some(1), speaker, draining));
        assert!(test.queue.is_empty());
        assert!(test.start(1_000_000, Some(0), Some(1), false, false));
    }
    let mut test = Test::new(true);
    let snapshot = test.owner.snapshot(now()).unwrap();
    let capture = crate::admission::CaptureLineage {
        worker: new_boot().unwrap(),
        epoch: 1,
        dsp_epoch: 1,
        privacy_generation: snapshot.microphone_generation(),
    };
    let update = test
        .admission
        .observe(
            crate::activity::Activity::Start(vec![crate::activity::ObservedAudio {
                sequence: 1,
                captured_at_us: monotonic_us(),
                samples: [1; lamp_audio::CAPTURE_SAMPLES],
            }]),
            crate::admission::Context {
                capture,
                authority: snapshot,
            },
            monotonic_us(),
        )
        .unwrap();
    assert!(matches!(update.step, crate::admission::Step::Candidate(_)));
    assert!(!test.admission.owns_stream());
    assert!(!test.start(1_000_000, Some(0), Some(1), false, false));
}

#[test]
fn holding_continuation_never_moves_to_a_new_session_or_crosses_mute() {
    let _clock = clock();
    for session in [None, Some(2)] {
        let mut test = Test::new(true);
        assert!(!test.start(1_000_000, Some(0), session, false, false));
        assert!(!test.start(1_000_000, Some(0), Some(1), false, false));
        assert!(test.queue.is_empty());
    }
    let mut test = Test::new(true);
    test.owner
        .set_microphone_permission(now(), Permission::Denied)
        .unwrap();
    assert!(!test.start(1_000_000, Some(0), Some(1), false, false));
    assert!(test.queue.is_empty());
}

#[test]
fn holding_classification_requires_the_current_confirmed_session() {
    let _clock = clock();
    let (_, reply, _) = active_reply();
    let request = reply.owner.turn();
    let mut reply = Some(reply);
    let mut cloud = CloudAvailability::default();
    assert!(
        observe_provider_output(
            &mut cloud,
            &mut reply,
            &ProviderOutput::HoldingAnswer {
                request,
                session: 2
            }
        )
        .is_err()
    );
    observe_provider_output(
        &mut cloud,
        &mut reply,
        &ProviderOutput::HoldingAnswer {
            request,
            session: 1,
        },
    )
    .unwrap();
    assert!(hand::cancelled_output(
        &ProviderOutput::HoldingAnswer {
            request,
            session: 1
        },
        Some(request)
    ));
}

#[test]
fn holding_continuation_counts_speech_even_when_admission_suppresses_it() {
    let _clock = clock();
    let mut test = Test::new(true);
    let mut frame = MicrophoneFrame {
        epoch: 1,
        dsp_epoch: 1,
        privacy_generation: 1,
        frame_sequence: 1,
        read_completed_at_us: monotonic_us(),
        timing: CaptureTiming {
            first_read_started_at_us: monotonic_us(),
            last_read_started_at_us: monotonic_us(),
            successful_reads: 1,
            alsa_status_monotonic_us: None,
            status_observed_at_us: monotonic_us(),
            available_frames: 0,
            delayed_frames: 0,
            processing_completed_at_us: monotonic_us(),
            aec_queue_delay_ms: 0,
            reference: Default::default(),
        },
        probability: 0.8,
        echo: None,
        reference: Default::default(),
        samples: vec![0; lamp_audio::CAPTURE_SAMPLES],
        raw: None,
        linear: None,
    };
    // Busy-room admission can ignore this block, but the original frame
    // still says speech was heard while the holding reply was ending.
    test.pending.observe_capture(&frame);
    assert!(!test.start(1_000_000, Some(0), Some(1), false, false));
    for _ in 0..499 {
        crate::scenario_clock::step(crate::scenario_clock::Phase::Run).unwrap();
    }
    // A non-speech block or older timestamp cannot move the last word back.
    frame.read_completed_at_us -= 2_000_000;
    test.pending.observe_capture(&frame);
    frame.read_completed_at_us = monotonic_us();
    for probability in [0.49, f32::NAN] {
        frame.probability = probability;
        test.pending.observe_capture(&frame);
    }
    assert!(!test.start(1_000_000, Some(0), Some(1), false, false));
    crate::scenario_clock::step(crate::scenario_clock::Phase::Run).unwrap();
    assert!(test.start(1_000_000, Some(0), Some(1), false, false));
}

#[test]
fn holding_classification_cannot_extend_a_turn_the_lamp_started_itself() {
    let _clock = clock();
    let (_, mut reply, _) = active_reply();
    let request = reply.owner.turn();
    let mut state = holding::Continuation::new(true);
    reply.input_sequence = 0;
    state.observed(request, 1, provider_boot(), Some(&reply));
    state.settled(request, true, monotonic_us());
    assert!(
        state
            .take(monotonic_us(), None, Some(1), provider_boot(), true)
            .is_none()
    );
    reply.input_sequence = 1;
    state.observed(request, 1, provider_boot(), Some(&reply));
    state.settled(request, true, monotonic_us());
    assert!(
        state
            .take(monotonic_us(), None, Some(1), provider_boot(), true)
            .is_some()
    );
}
