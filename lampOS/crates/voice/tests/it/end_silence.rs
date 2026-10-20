//! The end-of-speech wait is the caller's explicit choice, within bounds.
use lamp_voice::activity::{Activity, ObservedAudio, TurnDetector};

fn frame(sequence: u64) -> ObservedAudio {
    ObservedAudio {
        sequence,
        captured_at_us: sequence * 10_000,
        samples: [0; 160],
    }
}

/// Six speech blocks start an utterance; returns how many quiet blocks end it.
fn quiet_blocks_to_end(mut detector: TurnDetector) -> u16 {
    let mut sequence = 0;
    let mut started = false;
    for _ in 0..6 {
        sequence += 1;
        started |= matches!(detector.push(frame(sequence), 0.95), Activity::Start(_));
    }
    assert!(started);
    for quiet in 1..=400 {
        sequence += 1;
        if matches!(detector.push(frame(sequence), 0.0), Activity::End(_)) {
            return quiet;
        }
    }
    panic!("the utterance never ended");
}

#[test]
fn each_policy_keeps_its_own_wait_when_none_is_asked_for() {
    assert_eq!(TurnDetector::v1_baseline().endpoint_silence_ms(), 1_200);
    assert_eq!(TurnDetector::default().endpoint_silence_ms(), 600);
    assert_eq!(quiet_blocks_to_end(TurnDetector::v1_baseline()), 120);
    assert_eq!(quiet_blocks_to_end(TurnDetector::default()), 60);
}

#[test]
fn an_utterance_ends_after_exactly_the_wait_that_was_asked_for() {
    for blocks in [30, 70, 120, 200] {
        let detector = TurnDetector::v1_baseline()
            .with_end_silence(blocks)
            .unwrap();
        assert_eq!(detector.endpoint_silence_ms(), u64::from(blocks) * 10);
        assert_eq!(quiet_blocks_to_end(detector), blocks);
    }
}

#[test]
fn a_pause_shorter_than_the_wait_does_not_end_the_utterance() {
    let mut detector = TurnDetector::v1_baseline().with_end_silence(70).unwrap();
    let mut sequence = 0;
    for _ in 0..6 {
        sequence += 1;
        detector.push(frame(sequence), 0.95);
    }
    // 690 ms of quiet, one block of speech, then the full wait again.
    for _ in 0..69 {
        sequence += 1;
        assert!(matches!(
            detector.push(frame(sequence), 0.0),
            Activity::Continue(_)
        ));
    }
    sequence += 1;
    assert!(matches!(
        detector.push(frame(sequence), 0.95),
        Activity::Continue(_)
    ));
    for quiet in 1..=70 {
        sequence += 1;
        let ended = matches!(detector.push(frame(sequence), 0.0), Activity::End(_));
        assert_eq!(ended, quiet == 70, "at quiet block {quiet}");
    }
}

#[test]
fn a_wait_outside_300_to_2000_ms_is_refused() {
    assert!(TurnDetector::v1_baseline().with_end_silence(29).is_err());
    assert!(TurnDetector::v1_baseline().with_end_silence(201).is_err());
    assert!(TurnDetector::v1_baseline().with_end_silence(0).is_err());
}
