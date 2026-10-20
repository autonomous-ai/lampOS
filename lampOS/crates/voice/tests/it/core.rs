use lamp_turn_owner::{BootId, MonoTime};
use lamp_voice::{
    activity::{Activity, ObservedAudio, TurnDetector},
    privacy::PrivacyGate,
    wire::{Control, Envelope, ReceiveOrder, decode, encode},
};

fn time(t: u64) -> MonoTime {
    MonoTime::from_micros(t)
}
fn audio(sequence: u64) -> ObservedAudio {
    ObservedAudio {
        sequence,
        captured_at_us: sequence * 10_000,
        samples: [sequence as i16; 160],
    }
}

#[test]
fn empty_control_commands_keep_wire_compatibility_but_reject_unknown_fields() {
    for command in [Control::Stop, Control::StartCapture] {
        let valid = encode(&command).unwrap();
        assert_eq!(encode(&decode::<Control>(&valid).unwrap()).unwrap(), valid);
        let mut with_extra: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        with_extra["unexpected"] = true.into();
        assert!(decode::<Control>(&serde_json::to_vec(&with_extra).unwrap()).is_err());
    }
    assert!(decode::<Control>(br#"{"kind":"stop","kind":"start_capture"}"#).is_err());
    let capacity = br#"{"kind":"provider_output_capacity","through":2}"#;
    assert!(matches!(
        decode::<Control>(capacity).unwrap(),
        Control::ProviderOutputCapacity { through: 2 }
    ));
    for invalid in [
        br#"{"kind":"provider_output_capacity"}"#.as_slice(),
        br#"{"kind":"provider_output_capacity","through":-1}"#.as_slice(),
        br#"{"kind":"provider_output_capacity","through":2,"owner":1}"#.as_slice(),
    ] {
        assert!(decode::<Control>(invalid).is_err());
    }
}

#[test]
fn mute_is_immediate_reopening_stable_and_missing_samples_close() {
    let mut gate = PrivacyGate::default();
    for sequence in 1..=7 {
        let t = time(sequence * 10_000);
        assert_eq!(gate.observe(sequence, t, t, false), sequence == 7);
    }
    assert!(!gate.allowed(time(120_000)));
    assert!(!gate.observe(8, time(130_000), time(130_000), false));
    assert!(!gate.observe(9, time(140_000), time(140_000), true));
    assert!(!gate.allowed(time(140_001)));
}

#[test]
fn privacy_reorder_future_and_stale_fail_closed_without_rolling_back_highwater() {
    let mut gate = PrivacyGate::default();
    for seq in 1..=7 {
        let t = time(seq * 10_000);
        gate.observe(seq, t, t, false);
    }
    assert!(gate.allowed(time(70_000)));
    assert!(!gate.observe(6, time(60_000), time(71_000), false));
    assert!(!gate.observe(8, time(100_000), time(72_000), false));
    assert!(!gate.observe(8, time(80_000), time(180_000), false));
    assert!(!gate.observe(8, time(180_000), time(180_000), false));
}

#[test]
fn retained_prefix_and_endpoint_include_every_active_frame_once() {
    let mut detector = TurnDetector::default();
    for seq in 1..=25 {
        assert!(matches!(detector.push(audio(seq), 0.0), Activity::Quiet));
    }
    for seq in 26..=30 {
        assert!(matches!(detector.push(audio(seq), 0.95), Activity::Quiet));
    }
    let Activity::Start(prefix) = detector.push(audio(31), 0.95) else {
        panic!("speech start");
    };
    assert_eq!(prefix.len(), 30);
    assert_eq!(prefix.first().unwrap().sequence, 2);
    assert_eq!(prefix.last().unwrap().sequence, 31);
    for seq in 32..=90 {
        assert!(matches!(
            detector.push(audio(seq), 0.1),
            Activity::Continue(_)
        ));
    }
    assert!(matches!(detector.push(audio(91), 0.1), Activity::End(_)));
    assert!(matches!(detector.push(audio(92), 0.1), Activity::Quiet));
}

#[test]
fn missing_audio_or_invalid_probability_is_failure_not_a_silent_endpoint() {
    let mut detector = TurnDetector::default();
    for seq in 1..=6 {
        detector.push(audio(seq), 0.99);
    }
    assert!(matches!(detector.push(audio(8), 0.99), Activity::Fault(_)));
    assert!(matches!(
        detector.push(audio(9), f32::NAN),
        Activity::Fault(_)
    ));
    assert!(matches!(detector.push(audio(10), 1.1), Activity::Fault(_)));
}

#[test]
fn dsp_boundary_discards_inactive_prefix_but_preserves_active_words_and_order() {
    let mut detector = TurnDetector::default();
    for seq in 1..=5 {
        assert!(matches!(detector.push(audio(seq), 0.99), Activity::Quiet));
    }
    detector.discard_inactive_prefix();
    for seq in 6..=10 {
        assert!(matches!(detector.push(audio(seq), 0.99), Activity::Quiet));
    }
    let Activity::Start(prefix) = detector.push(audio(11), 0.99) else {
        panic!("new DSP prefix should start after six new blocks");
    };
    assert_eq!(prefix.first().unwrap().sequence, 6);
    assert_eq!(prefix.len(), 6);
    detector.discard_inactive_prefix();
    assert!(matches!(
        detector.push(audio(12), 0.99),
        Activity::Continue(_)
    ));
    detector.discard_inactive_prefix();
    assert!(matches!(detector.push(audio(14), 0.99), Activity::Fault(_)));
}

#[test]
fn ipc_replay_or_other_worker_cannot_replace_fresh_observation() {
    let boot = BootId::new([1; 16]).unwrap();
    let mut order = ReceiveOrder::new(boot);
    let packet = Envelope {
        boot,
        sequence: 3,
        sent_at_us: 100,
        payload: (),
    };
    order.accept(&packet, 101, 50).unwrap();
    assert!(order.accept(&packet, 102, 50).is_err());
    let other = Envelope {
        boot: BootId::new([2; 16]).unwrap(),
        sequence: 4,
        sent_at_us: 103,
        payload: (),
    };
    assert!(order.accept(&other, 104, 50).is_err());
    assert!(decode::<Envelope<()>>(&vec![b' '; 8193]).is_err());
}

#[test]
fn baseline_keeps_a_mid_question_low_score_pause_in_one_input() {
    // Synthetic reproduction of the recorded fifth question: speech, a 700 ms
    // detector dropout, then the rest of that same question. No PCM inference.
    let scores = [
        vec![0.1; 25],
        vec![0.95; 20],
        vec![0.1; 70],
        vec![0.95; 30],
        vec![0.1; 120],
    ]
    .concat();
    for (mut detector, expected_starts) in [
        (TurnDetector::default(), 2),
        (TurnDetector::v1_baseline(), 1),
    ] {
        let mut starts = 0;
        let mut ends = 0;
        let mut retained = Vec::new();
        for (i, score) in scores.iter().enumerate() {
            match detector.push(audio(i as u64 + 1), *score) {
                Activity::Start(prefix) => {
                    starts += 1;
                    retained.extend(prefix.iter().map(|frame| frame.sequence));
                }
                Activity::Continue(frame) => retained.push(frame.sequence),
                Activity::End(frame) => {
                    ends += 1;
                    retained.push(frame.sequence);
                }
                Activity::Quiet => {}
                Activity::Fault(reason) => panic!("{reason}"),
            }
        }
        assert_eq!(starts, expected_starts);
        assert_eq!(ends, expected_starts);
        if expected_starts == 1 {
            assert_eq!(
                retained,
                (2..=265).collect::<Vec<_>>(),
                "every prefix and active frame must be retained exactly once"
            );
        }
    }
}

#[test]
fn baseline_short_reply_has_a_finite_endpoint_and_reset_keeps_policy() {
    let mut detector = TurnDetector::v1_baseline();
    assert_eq!(TurnDetector::default().endpoint_silence_ms(), 600);
    for _ in 0..2 {
        detector.reset();
        assert_eq!(detector.endpoint_silence_ms(), 1_200);
        // A brief synthetic yes/no needs only the original six-block onset.
        for seq in 1..6 {
            assert!(matches!(detector.push(audio(seq), 0.95), Activity::Quiet));
        }
        assert!(matches!(detector.push(audio(6), 0.95), Activity::Start(_)));
        for seq in 7..126 {
            assert!(matches!(
                detector.push(audio(seq), 0.1),
                Activity::Continue(_)
            ));
        }
        assert!(matches!(detector.push(audio(126), 0.1), Activity::End(_)));
    }
}

#[test]
fn baseline_does_not_claim_to_recover_an_arbitrarily_quiet_onset() {
    let mut detector = TurnDetector::v1_baseline();
    // Deliberately slow synthetic onset: the prefix is still only 300 ms.
    for seq in 1..=50 {
        assert!(matches!(detector.push(audio(seq), 0.6), Activity::Quiet));
    }
    for seq in 51..56 {
        assert!(matches!(detector.push(audio(seq), 0.95), Activity::Quiet));
    }
    let Activity::Start(prefix) = detector.push(audio(56), 0.95) else {
        panic!("onset");
    };
    assert_eq!(prefix.first().unwrap().sequence, 27);
    assert_eq!(prefix.last().unwrap().sequence, 56);
    assert_eq!(prefix.len(), 30);
    // Continuity faults must stay faults even with the longer silence window.
    assert!(matches!(detector.push(audio(58), 0.1), Activity::Fault(_)));
    assert_eq!(detector.endpoint_silence_ms(), 1_200);
}
