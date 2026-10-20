//! A bare call ends nothing (#782), through the coordinator's own handling
//! of `name/rising` on a real bus in a temporary directory. The first sign
//! is published when `lamp-name` would send it on lamp-4ace: as its score
//! reaches the bar, 0.32 to 0.40 s before a 600 ms window ended "Hey Lamp,"
//! on 2026-10-15 (23:02 to 23:05). The final `name/heard` came 0.08 to
//! 0.19 s after that end, too late for the turn. The detector's own rule is
//! in `tests/call_wait.rs`.
use super::*;
use crate::activity::Activity;

/// When the first sign of the name is published: this many blocks into the
/// call's own speech, or into the quiet after it.
#[derive(Clone, Copy)]
enum Sign {
    Speaking(u16),
    Quiet(u16),
}

/// "Hey Lamp," (0.35 s of speech, or 0.7 s when the sign comes while it is
/// said), then quiet, with the first sign of the name published as `sign`
/// says. Blocks of quiet to the end, whether its record says the wait ran
/// out, and how many `call_wait` records.
fn hey_lamp_then_quiet(
    detector: &mut TurnDetector,
    root: &std::path::Path,
    sign: Sign,
) -> (u16, Option<u64>, usize) {
    let bus = lamp_bus::Bus::new(root).unwrap();
    let mut rising =
        crate::bus_out::NameIn::on_topic(root, lamp_msgs::NameHeard::RISING_TOPIC).unwrap();
    let mut says = bus
        .publisher_declared(
            lamp_msgs::NameHeard::RISING_TOPIC,
            &lamp_msgs::NameHeard::declaration(),
            "name",
        )
        .unwrap();
    let mut trace = SessionState::finite();
    let at = |sequence: u64| 1_000_000 + sequence * 10_000;
    let frame = |sequence: u64| ObservedAudio {
        sequence,
        captured_at_us: at(sequence),
        samples: [0; 160],
    };
    let publish = |says: &mut lamp_bus::Publisher| {
        says.publish_json(
            1,
            &lamp_msgs::NameHeard {
                score: 0.93,
                ago_ms: 0,
            },
        )
        .unwrap();
        // The datagram is in the voice's socket before it looks.
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut sequence = 0;
    let speaking = if matches!(sign, Sign::Speaking(_)) {
        70
    } else {
        35
    };
    for spoken in 1..=speaking {
        sequence += 1;
        if matches!(sign, Sign::Speaking(at) if at == spoken) {
            publish(&mut says);
        }
        hear_the_call(&mut rising, at(sequence), detector, Some(1), &mut trace).unwrap();
        detector.push(frame(sequence), 0.95);
    }
    for quiet in 1..=400u16 {
        sequence += 1;
        if matches!(sign, Sign::Quiet(at) if at == quiet) {
            publish(&mut says);
        }
        hear_the_call(&mut rising, at(sequence), detector, Some(1), &mut trace).unwrap();
        if matches!(detector.push(frame(sequence), 0.0), Activity::End(_)) {
            let waits = trace
                .to_vec()
                .iter()
                .filter(|record| record["kind"] == "call_wait")
                .count();
            return (
                quiet,
                detector.last_endpoint().and_then(|end| end.call_wait_ms),
                waits,
            );
        }
    }
    panic!("the utterance never ended");
}

fn root(name: &str) -> std::path::PathBuf {
    let root = std::path::PathBuf::from(format!("/tmp/lvc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn the_first_sign_of_the_name_makes_a_call_wait_for_the_rest() {
    let window = || TurnDetector::v1_baseline().with_end_silence(60).unwrap();
    // The first sign 0.23 s into the quiet, as on lamp-4ace: the call
    // waits its two seconds, and the record says so.
    let mut waiting = window().with_call_wait(200).unwrap();
    assert_eq!(
        hey_lamp_then_quiet(&mut waiting, &root("call-on"), Sign::Quiet(23)),
        (200, Some(2_000), 1)
    );
    // Without the wait the same sign changes nothing: the window ends it.
    let mut plain = window();
    assert_eq!(
        hey_lamp_then_quiet(&mut plain, &root("call-off"), Sign::Quiet(23)),
        (60, None, 0)
    );
    // A first sign after the window, as the final word came on the lamp,
    // is too late: the turn has ended.
    let mut late = window().with_call_wait(200).unwrap();
    assert_eq!(
        hey_lamp_then_quiet(&mut late, &root("call-late"), Sign::Quiet(68)),
        (60, None, 0)
    );
    // The first sign while the name is still being said (#891): the rest
    // of the call keeps the wait.
    let mut midway = window().with_call_wait(200).unwrap();
    assert_eq!(
        hey_lamp_then_quiet(&mut midway, &root("call-mid"), Sign::Speaking(35)),
        (200, Some(2_000), 1)
    );
}

/// Only an end that is a call's wait run out tells the model it was only
/// the name; every other end sends nothing more than it did. An offline
/// turn sends nothing, and a queue at its bound keeps its last slot for
/// the end.
#[test]
fn only_a_call_whose_wait_ran_out_is_told_to_the_model() {
    let end = |call_wait_ms| crate::activity::EndpointDecision {
        basis: "silence_fallback",
        silence_ms: 2_000,
        probability: None,
        fallback: None,
        verdict: None,
        call_wait_ms,
        short_asks: 0,
    };
    let (waited, other) = (end(Some(2_000)), end(None));
    let note = |decision, local_only, queued| call_note(decision, 7, local_only, 3, queued, 64);
    assert!(matches!(
        note(Some(&waited), false, 63),
        Some(ProviderInput::Called {
            request: 7,
            privacy_generation: 3
        })
    ));
    assert!(note(Some(&other), false, 0).is_none());
    assert!(note(None, false, 0).is_none());
    // A local-only turn's request never went out (#830).
    assert!(note(Some(&waited), true, 0).is_none());
    // At the bound, the end's own slot is not taken.
    assert!(note(Some(&waited), false, 64).is_none());
    let mut queue: VecDeque<ProviderInput> = (0..63)
        .map(|sequence| ProviderInput::Audio {
            request: 7,
            privacy_generation: 3,
            sequence,
            read_completed_at_us: 0,
            samples: Vec::new(),
        })
        .collect();
    let queued = queue.len();
    queue.extend(note(Some(&waited), false, queued));
    assert_eq!(queue.len(), 64);
    assert!(note(Some(&waited), false, queue.len()).is_none());
}
