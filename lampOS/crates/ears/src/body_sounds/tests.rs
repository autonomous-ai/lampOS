use super::*;

const S: u64 = 1_000_000;

fn bars(refractory: u64) -> [(BodyClass, Option<Bar>); 5] {
    let bar = |bar| {
        Some(Bar {
            bar,
            refractory_us: refractory,
        })
    };
    [
        (BodyClass::Cough, bar(0.5)),
        (BodyClass::ThroatClear, bar(0.5)),
        (BodyClass::Sneeze, bar(0.6)),
        (BodyClass::Sniff, None),
        (BodyClass::Wheeze, None),
    ]
}

fn patch(sneeze: f32, cough: f32) -> SoundScores {
    SoundScores {
        sneeze,
        cough,
        ..SoundScores::default()
    }
}

const UNKNOWN: OwnVoiceEvidence = OwnVoiceEvidence::Unknown;

#[test]
fn the_lamp_ships_no_measured_bar_so_it_tells_nothing_yet() {
    let mut detector = Detector::new();
    let loud = SoundScores {
        cough: 1.0,
        sneeze: 1.0,
        throat_clear: 1.0,
        sniff: 1.0,
        wheeze: 1.0,
        ..SoundScores::default()
    };
    assert!(detector.feed(&loud, UNKNOWN, S).is_empty());
    assert!(BARS.iter().all(|(_, bar)| bar.is_none()));
}

#[test]
fn one_sneeze_across_three_patches_is_one_event() {
    let mut detector = Detector::with_bars(bars(S));
    let mut told = Vec::new();
    for (n, sneeze) in [0.1, 0.7, 0.9, 0.8, 0.2, 0.1].into_iter().enumerate() {
        told.extend(detector.feed(&patch(sneeze, 0.0), UNKNOWN, (n as u64 + 1) * S / 2));
    }
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].class, BodyClass::Sneeze);
    assert_eq!(told[0].score, 0.7, "the score on the rise");
    assert_eq!(told[0].at_us, S);
}

#[test]
fn a_cough_never_sets_off_a_sneeze_and_each_is_counted_apart() {
    let mut detector = Detector::with_bars(bars(S));
    let told = detector.feed(&patch(0.2, 0.9), UNKNOWN, S);
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].class, BodyClass::Cough);
    // A sneeze and a cough in the same patch are two events.
    let mut detector = Detector::with_bars(bars(S));
    let told = detector.feed(&patch(0.9, 0.9), UNKNOWN, S);
    assert_eq!(told.len(), 2);
}

#[test]
fn a_second_sound_needs_the_score_to_fall_and_the_refractory_to_pass() {
    let mut detector = Detector::with_bars(bars(2 * S));
    assert_eq!(detector.feed(&patch(0.9, 0.0), UNKNOWN, S).len(), 1);
    // Fell, but inside the refractory: nothing.
    assert!(
        detector
            .feed(&patch(0.0, 0.0), UNKNOWN, S + S / 2)
            .is_empty()
    );
    assert!(detector.feed(&patch(0.9, 0.0), UNKNOWN, 2 * S).is_empty());
    // Held high past the refractory with no fall: still the same sound.
    let mut detector = Detector::with_bars(bars(S));
    assert_eq!(detector.feed(&patch(0.9, 0.0), UNKNOWN, S).len(), 1);
    assert!(detector.feed(&patch(0.9, 0.0), UNKNOWN, 4 * S).is_empty());
    // Fell under 70% of the bar and the refractory passed: the next is a new one.
    assert!(detector.feed(&patch(0.1, 0.0), UNKNOWN, 5 * S).is_empty());
    assert_eq!(detector.feed(&patch(0.8, 0.0), UNKNOWN, 6 * S).len(), 1);
}

#[test]
fn a_score_just_under_its_bar_is_not_a_sound() {
    let mut detector = Detector::with_bars(bars(S));
    assert!(detector.feed(&patch(0.59, 0.49), UNKNOWN, S).is_empty());
    assert_eq!(detector.feed(&patch(0.6, 0.0), UNKNOWN, 2 * S).len(), 1);
}

#[test]
fn nothing_is_told_while_the_lamp_speaks_into_the_patch() {
    let mut detector = Detector::with_bars(bars(S));
    assert!(
        detector
            .feed(&patch(0.95, 0.95), OwnVoiceEvidence::Present, S)
            .is_empty()
    );
    // Its own voice did not use up the sound: a person's sneeze after it is told.
    assert_eq!(
        detector
            .feed(&patch(0.95, 0.0), OwnVoiceEvidence::Clear, 2 * S)
            .len(),
        1
    );
}

#[test]
fn a_class_with_no_bar_is_never_told_however_high() {
    let mut detector = Detector::with_bars(bars(S));
    let sniff = SoundScores {
        sniff: 1.0,
        wheeze: 1.0,
        ..SoundScores::default()
    };
    assert!(detector.feed(&sniff, UNKNOWN, S).is_empty());
}
