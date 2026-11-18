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

fn patch(sneeze: f32, cough: f32) -> BodyScores {
    BodyScores {
        sneeze,
        cough,
        ..BodyScores::default()
    }
}

const CLEAR: OwnVoiceEvidence = OwnVoiceEvidence::Clear;

#[test]
fn the_lamp_ships_no_measured_bar_so_it_tells_nothing_yet() {
    let mut detector = Detector::new();
    let loud = BodyScores {
        cough: 1.0,
        sneeze: 1.0,
        throat_clear: 1.0,
        sniff: 1.0,
        wheeze: 1.0,
    };
    assert!(detector.feed(&loud, CLEAR, S).is_empty());
    assert!(BARS.iter().all(|(_, bar)| bar.is_none()));
}

#[test]
fn one_sneeze_across_three_patches_is_one_event() {
    let mut detector = Detector::with_bars(bars(S));
    let mut told = Vec::new();
    for (n, sneeze) in [0.1, 0.7, 0.9, 0.8, 0.2, 0.1].into_iter().enumerate() {
        told.extend(detector.feed(&patch(sneeze, 0.0), CLEAR, (n as u64 + 1) * S / 2));
    }
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].class, BodyClass::Sneeze);
    assert_eq!(told[0].score, 0.7, "the score on the rise");
    assert_eq!(told[0].at_us, S);
}

#[test]
fn a_cough_never_sets_off_a_sneeze_and_each_is_counted_apart() {
    let mut detector = Detector::with_bars(bars(S));
    let told = detector.feed(&patch(0.2, 0.9), CLEAR, S);
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].class, BodyClass::Cough);
    // A sneeze and a cough in the same patch are two events.
    let mut detector = Detector::with_bars(bars(S));
    let told = detector.feed(&patch(0.9, 0.9), CLEAR, S);
    assert_eq!(told.len(), 2);
}

#[test]
fn a_second_sound_needs_the_score_to_fall_and_the_refractory_to_pass() {
    let mut detector = Detector::with_bars(bars(2 * S));
    assert_eq!(detector.feed(&patch(0.9, 0.0), CLEAR, S).len(), 1);
    // Fell, but inside the refractory: nothing.
    assert!(detector.feed(&patch(0.0, 0.0), CLEAR, S + S / 2).is_empty());
    assert!(detector.feed(&patch(0.9, 0.0), CLEAR, 2 * S).is_empty());
    // Held high past the refractory with no fall: still the same sound.
    let mut detector = Detector::with_bars(bars(S));
    assert_eq!(detector.feed(&patch(0.9, 0.0), CLEAR, S).len(), 1);
    assert!(detector.feed(&patch(0.9, 0.0), CLEAR, 4 * S).is_empty());
    // Fell under 70% of the bar and the refractory passed: the next is a new one.
    assert!(detector.feed(&patch(0.1, 0.0), CLEAR, 5 * S).is_empty());
    assert_eq!(detector.feed(&patch(0.8, 0.0), CLEAR, 6 * S).len(), 1);
}

#[test]
fn a_score_just_under_its_bar_is_not_a_sound() {
    let mut detector = Detector::with_bars(bars(S));
    assert!(detector.feed(&patch(0.59, 0.49), CLEAR, S).is_empty());
    assert_eq!(detector.feed(&patch(0.6, 0.0), CLEAR, 2 * S).len(), 1);
}

#[test]
fn nothing_is_told_while_the_lamp_speaks_into_the_patch() {
    let mut detector = Detector::with_bars(bars(S));
    assert!(
        detector
            .feed(&patch(0.95, 0.95), OwnVoiceEvidence::Present, S)
            .is_empty()
    );
    // Its own voice did not use up the person's next sneeze, after a fall.
    assert!(detector.feed(&patch(0.0, 0.0), CLEAR, S + S / 2).is_empty());
    assert_eq!(detector.feed(&patch(0.95, 0.0), CLEAR, 2 * S).len(), 1);
}

#[test]
fn a_class_with_no_bar_is_never_told_however_high() {
    let mut detector = Detector::with_bars(bars(S));
    let sniff = BodyScores {
        sniff: 1.0,
        wheeze: 1.0,
        ..BodyScores::default()
    };
    assert!(detector.feed(&sniff, CLEAR, S).is_empty());
}

#[test]
fn a_stale_or_unknown_voice_state_counts_as_speaking() {
    let mut detector = Detector::with_bars(bars(S));
    assert!(
        detector
            .feed(&patch(0.95, 0.95), OwnVoiceEvidence::Unknown, S)
            .is_empty()
    );
}

#[test]
fn the_tail_of_a_sound_under_the_lamps_voice_is_not_told_after_it() {
    let mut detector = Detector::with_bars(bars(S));
    assert!(
        detector
            .feed(&patch(0.95, 0.0), OwnVoiceEvidence::Present, S)
            .is_empty()
    );
    // Still high once the voice has ended: the same sound, not a new one.
    assert!(detector.feed(&patch(0.9, 0.0), CLEAR, 2 * S).is_empty());
    // It falls, and the next one is told.
    assert!(detector.feed(&patch(0.1, 0.0), CLEAR, 3 * S).is_empty());
    assert_eq!(detector.feed(&patch(0.9, 0.0), CLEAR, 4 * S).len(), 1);
}

#[test]
fn a_class_is_re_armed_under_seventy_percent_of_its_bar_and_not_before() {
    // Sneeze bar 0.6: 70% is 0.42.
    let mut detector = Detector::with_bars(bars(0));
    assert_eq!(detector.feed(&patch(0.9, 0.0), CLEAR, S).len(), 1);
    // Fell to 0.43, still over 70%: not re-armed, the next rise is the same sound.
    assert!(detector.feed(&patch(0.43, 0.0), CLEAR, 2 * S).is_empty());
    assert!(detector.feed(&patch(0.9, 0.0), CLEAR, 3 * S).is_empty());
    // Fell to 0.41, under 70%: re-armed, the next rise is a new sound.
    assert!(detector.feed(&patch(0.41, 0.0), CLEAR, 4 * S).is_empty());
    assert_eq!(detector.feed(&patch(0.9, 0.0), CLEAR, 5 * S).len(), 1);
}

#[test]
fn a_rise_held_back_by_the_refractory_is_never_told_later_in_the_same_sound() {
    let mut detector = Detector::with_bars(bars(3 * S));
    assert_eq!(detector.feed(&patch(0.9, 0.0), CLEAR, S).len(), 1);
    // Fell and rose again inside the refractory: dropped.
    assert!(detector.feed(&patch(0.1, 0.0), CLEAR, 2 * S).is_empty());
    assert!(detector.feed(&patch(0.9, 0.0), CLEAR, 3 * S).is_empty());
    // The refractory has passed but the score has not fallen: still that sound.
    assert!(detector.feed(&patch(0.9, 0.0), CLEAR, 5 * S).is_empty());
    // Only a fall and a new rise is told.
    assert!(detector.feed(&patch(0.1, 0.0), CLEAR, 6 * S).is_empty());
    assert_eq!(detector.feed(&patch(0.9, 0.0), CLEAR, 7 * S).len(), 1);
}
