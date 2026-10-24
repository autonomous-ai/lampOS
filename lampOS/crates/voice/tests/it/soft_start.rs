//! A line's sound begins a moment before its first word (`LAMP_SOFT_START=on`,
//! with `LAMP_FIRST_WORDS=kept`): an ordinary start keeps 300 ms more than the
//! usual 300 ms, never past a block heard while the lamp was sounding, and
//! never later than the usual start. A yield's start and a start without the
//! switch are unchanged.
use lamp_voice::activity::{
    Activity, ObservedAudio, PRE_ROLL_BLOCKS, SOFT_START_LEAD_BLOCKS, START_BLOCKS, TurnDetector,
};

fn frame(sequence: u64) -> ObservedAudio {
    ObservedAudio {
        sequence,
        captured_at_us: sequence * 10_000,
        samples: [0; 160],
    }
}

/// 120 blocks of quiet heard with the lamp silent, then `before`, whether each
/// block was heard with the lamp silent, the last just ahead of the onset; then
/// the onset, six blocks over the start's bar, heard with the lamp silent as
/// `onset_silent` says, marked a yield on its last block when `yield_start`.
/// The sequence the start's audio begins at, and the onset's first.
fn start(
    soft: bool,
    kept: bool,
    before: &[bool],
    onset_silent: bool,
    yield_start: bool,
) -> (u64, u64) {
    let mut detector = TurnDetector::v1_baseline();
    detector.keep_first_words(kept);
    detector.keep_soft_starts(soft);
    let mut sequence = 0;
    for silent in std::iter::repeat_n(true, 120).chain(before.iter().copied()) {
        sequence += 1;
        detector.lamp_silent(silent);
        assert!(matches!(
            detector.push_heard(frame(sequence), 0.0, Some(0.0)),
            Activity::Quiet
        ));
    }
    let onset = sequence + 1;
    for n in 1..=START_BLOCKS {
        sequence += 1;
        if yield_start && n == START_BLOCKS {
            detector.reach_back();
        }
        detector.lamp_silent(onset_silent);
        if let Activity::Start(audio) = detector.push_heard(frame(sequence), 0.95, Some(0.95)) {
            assert!(!audio.is_empty());
            assert!(
                audio
                    .windows(2)
                    .all(|pair| pair[1].sequence == pair[0].sequence + 1)
            );
            assert_eq!(audio.last().unwrap().sequence, sequence);
            return (audio[0].sequence, onset);
        }
    }
    panic!("the onset did not start an utterance");
}

/// The usual start for an onset: 300 ms back from its sixth block.
fn usual(onset: u64) -> u64 {
    onset + START_BLOCKS as u64 - PRE_ROLL_BLOCKS as u64
}

/// "what" on lamp-4ace: started 278 ms after the word began, so the usual
/// 300 ms began 12 ms before it, and the word was lost. With the switch the
/// line's sound begins 300 ms earlier still; without it, or without first
/// words kept, as before.
#[test]
fn a_soft_start_keeps_300_ms_more_only_with_the_switch_and_first_words() {
    let (begins, onset) = start(true, true, &[true; 40], true, false);
    assert_eq!(begins, usual(onset) - SOFT_START_LEAD_BLOCKS as u64);
    for (soft, kept) in [(false, true), (true, false), (false, false)] {
        let (begins, onset) = start(soft, kept, &[true; 40], true, false);
        assert_eq!(begins, usual(onset), "soft {soft} kept {kept}");
    }
}

/// A block heard while the lamp was sounding, 10 blocks before the usual
/// start: the line begins after it, never in the lamp's own voice.
#[test]
fn a_soft_start_never_reaches_into_the_lamps_own_sound() {
    let mut before = [true; 40];
    before[40 - 24 - 10] = false;
    let (begins, onset) = start(true, true, &before, true, false);
    assert_eq!(begins, usual(onset) - 9);
}

/// The reviewer's P2 on #3392: every block, the onset's own among them,
/// heard while the lamp was sounding (a reply not yet over): the usual start,
/// with its audio, never one with none.
#[test]
fn a_start_heard_all_with_the_lamp_sounding_begins_as_usual() {
    let (begins, onset) = start(true, true, &[false; 40], false, false);
    assert_eq!(begins, usual(onset));
    // Silent until the onset, the onset not: still the usual start.
    let (begins, onset) = start(true, true, &[true; 40], false, false);
    assert_eq!(begins, usual(onset));
}

/// A soft start never reaches before what the detector keeps: with first
/// words kept that is 1.6 s, so 300 ms more is always there; and it is the
/// same 300 ms more after a short quiet as after a long one.
#[test]
fn a_soft_start_keeps_the_same_300_ms_after_any_quiet() {
    for quiet in [24 + SOFT_START_LEAD_BLOCKS, 60, 200] {
        let (begins, onset) = start(true, true, &vec![true; quiet], true, false);
        assert_eq!(
            begins,
            usual(onset) - SOFT_START_LEAD_BLOCKS as u64,
            "{quiet}"
        );
    }
}

/// A yield's start reaches back by its own scores (here all 0): the usual
/// start, as without the switch.
#[test]
fn a_yields_start_is_not_a_soft_start() {
    let (begins, onset) = start(true, true, &[true; 40], true, true);
    assert_eq!(begins, usual(onset));
}
