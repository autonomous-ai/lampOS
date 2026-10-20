//! A bare call ends nothing (#782): with a call wait, an utterance that is
//! only the lamp's name waits for the rest; every other utterance, and
//! every utterance without it, ends as before.
use lamp_voice::activity::{Activity, CALL_SPEECH_BLOCKS, ObservedAudio, TurnDetector};

fn frame(sequence: u64) -> ObservedAudio {
    ObservedAudio {
        sequence,
        captured_at_us: sequence * 10_000,
        samples: [0; 160],
    }
}

/// `speech` blocks of speech, then the name heard as it began (if `named`),
/// then quiet: how many quiet blocks end it, and its record.
fn quiet_blocks_to_end(mut detector: TurnDetector, speech: u64, named: bool) -> (u16, Option<u64>) {
    let mut sequence = 0;
    for _ in 0..speech {
        sequence += 1;
        detector.push(frame(sequence), 0.95);
    }
    if named {
        detector.name_heard(10_000);
    }
    for quiet in 1..=400 {
        sequence += 1;
        if matches!(detector.push(frame(sequence), 0.0), Activity::End(_)) {
            let waited = detector.last_endpoint().and_then(|end| end.call_wait_ms);
            return (quiet, waited);
        }
    }
    panic!("the utterance never ended");
}

fn lamp() -> TurnDetector {
    TurnDetector::v1_baseline().with_end_silence(60).unwrap()
}

#[test]
fn only_the_name_waits_for_the_rest_and_says_so_when_the_wait_runs_out() {
    let waiting = || lamp().with_call_wait(200).unwrap();
    // "Hey Lamp," (0.7 s), named: two seconds, not the window.
    assert_eq!(quiet_blocks_to_end(waiting(), 70, true), (200, Some(2_000)));
    // Not named, or more said than a call: the window, as always.
    assert_eq!(quiet_blocks_to_end(waiting(), 70, false), (60, None));
    let long = u64::from(CALL_SPEECH_BLOCKS) + 30;
    assert_eq!(quiet_blocks_to_end(waiting(), long, true), (60, None));
    // Without the wait, the name changes nothing.
    assert_eq!(quiet_blocks_to_end(lamp(), 70, true), (60, None));
}

#[test]
fn the_rest_of_the_call_is_the_same_utterance_and_ends_as_any_other() {
    let mut detector = lamp().with_call_wait(200).unwrap();
    let mut sequence = 0;
    let mut starts = 0;
    let mut push = |detector: &mut TurnDetector, probability: f32| {
        sequence += 1;
        let activity = detector.push(frame(sequence), probability);
        if matches!(activity, Activity::Start(_)) {
            starts += 1;
        }
        activity
    };
    for _ in 0..70 {
        push(&mut detector, 0.95);
    }
    assert!(detector.name_heard(10_000));
    // A second, not ending at the 600 ms window, then the question.
    for _ in 0..100 {
        assert!(matches!(push(&mut detector, 0.0), Activity::Continue(_)));
    }
    for _ in 0..150 {
        assert!(matches!(push(&mut detector, 0.95), Activity::Continue(_)));
    }
    // The question's own pause ends it at the window, and only once.
    let mut ended = None;
    for quiet in 1..=100 {
        if matches!(push(&mut detector, 0.0), Activity::End(_)) {
            ended = Some(quiet);
            break;
        }
    }
    assert_eq!(ended, Some(60));
    assert_eq!(detector.last_endpoint().unwrap().call_wait_ms, None);
    assert_eq!(starts, 1);
    // A second hearing in the same utterance does not wait again.
    assert!(!detector.name_heard(10_000));
}

/// #891: the first sign comes while the name is still being said. The
/// rest of the call keeps the wait; it ends when the rest comes after a
/// pause, or when more is said than a call.
#[test]
fn a_first_sign_while_the_name_is_said_still_waits() {
    let mut sequence = 0;
    let mut push = |detector: &mut TurnDetector, probability: f32| {
        sequence += 1;
        detector.push(frame(sequence), probability)
    };
    let quiet_to_end =
        |detector: &mut TurnDetector, push: &mut dyn FnMut(&mut TurnDetector, f32) -> Activity| {
            for quiet in 1..=400u16 {
                if matches!(push(detector, 0.0), Activity::End(_)) {
                    return (
                        quiet,
                        detector.last_endpoint().and_then(|end| end.call_wait_ms),
                    );
                }
            }
            panic!("the utterance never ended");
        };
    // "Hey Lamp," 0.7 s, the sign at 0.35 s with a short gap after it.
    let mut detector = lamp().with_call_wait(200).unwrap();
    for _ in 0..35 {
        push(&mut detector, 0.95);
    }
    assert!(detector.name_heard(10_000));
    for _ in 0..5 {
        push(&mut detector, 0.0);
    }
    for _ in 0..30 {
        push(&mut detector, 0.95);
    }
    assert_eq!(quiet_to_end(&mut detector, &mut push), (200, Some(2_000)));
}

/// #891: a first sign while the name is said, and then more and more said
/// without a pause: more than a call, ended at the window as any other.
#[test]
fn speech_that_goes_on_past_a_call_is_not_one() {
    let mut long = lamp().with_call_wait(200).unwrap();
    let mut sequence = 0;
    for _ in 0..35 {
        sequence += 1;
        long.push(frame(sequence), 0.95);
    }
    assert!(long.name_heard(10_000));
    for _ in 0..u64::from(CALL_SPEECH_BLOCKS) {
        sequence += 1;
        long.push(frame(sequence), 0.95);
    }
    for quiet in 1..=400u16 {
        sequence += 1;
        if matches!(long.push(frame(sequence), 0.0), Activity::End(_)) {
            assert_eq!(quiet, 60);
            assert_eq!(long.last_endpoint().unwrap().call_wait_ms, None);
            return;
        }
    }
    panic!("the utterance never ended");
}

#[test]
fn a_call_wait_must_be_longer_than_the_window_and_at_most_three_seconds() {
    assert!(lamp().with_call_wait(60).is_err());
    assert!(lamp().with_call_wait(301).is_err());
    assert_eq!(
        lamp().with_call_wait(300).unwrap().call_wait_blocks(),
        Some(300)
    );
    // A name heard long before the utterance began is not its own.
    let mut detector = lamp().with_call_wait(200).unwrap();
    for sequence in 1..=200 {
        detector.push(frame(sequence), 0.0);
    }
    for sequence in 201..=270 {
        detector.push(frame(sequence), 0.95);
    }
    assert!(!detector.name_heard(10_000));
}
