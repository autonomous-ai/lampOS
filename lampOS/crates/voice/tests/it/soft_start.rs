//! A line's sound begins a moment before its first word (`LAMP_SOFT_START=on`,
//! with `LAMP_FIRST_WORDS=kept`): an ordinary start keeps 200 ms more than the
//! usual 300 ms, or begins 200 ms before the earliest block of the
//! speech-like run it starts on, by its score before the faint floor, if that
//! is earlier; at most 500 ms more, only through gaps under 100 ms, and never
//! past a block heard while the lamp was sounding. A yield's start and a start
//! without the switch are unchanged.
use lamp_voice::activity::{
    Activity, ObservedAudio, PRE_ROLL_BLOCKS, REACH_LEAD_BLOCKS, SOFT_START_BLOCKS,
    SOFT_START_LEAD_BLOCKS, START_BLOCKS, TurnDetector,
};

fn frame(sequence: u64) -> ObservedAudio {
    ObservedAudio {
        sequence,
        captured_at_us: sequence * 10_000,
        samples: [0; 160],
    }
}

/// 120 blocks of quiet; then `before`: each block's score before the floor,
/// the last of them just ahead of the onset, all given to the detector as
/// nothing (under the floor); then the onset, six blocks over the start's
/// bar. The sequence the start's audio begins at, and the onset's first.
fn start(soft: bool, kept: bool, before: &[Option<f32>], yield_start: bool) -> (u64, u64) {
    let mut detector = TurnDetector::v1_baseline();
    detector.keep_first_words(kept);
    detector.keep_soft_starts(soft);
    let mut sequence = 0;
    let quiet = std::iter::repeat_n(Some(0.05), 120);
    for unfloored in quiet.chain(before.iter().copied()) {
        sequence += 1;
        detector.unfloored(unfloored);
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
        detector.unfloored(Some(0.95));
        if let Activity::Start(audio) = detector.push_heard(frame(sequence), 0.95, Some(0.95)) {
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

/// A soft "what" before a louder "time": 40 blocks scored 0.5 before the
/// floor, all under it, run into the onset.
fn soft_word() -> Vec<Option<f32>> {
    vec![Some(0.5); 40]
}

#[test]
fn a_soft_first_word_is_kept_only_with_the_switch_and_first_words() {
    let (begins, onset) = start(true, true, &soft_word(), false);
    // The run's first block is 40 before the onset; 200 ms before it.
    assert_eq!(begins, onset - 40 - REACH_LEAD_BLOCKS as u64);
    assert!(begins < usual(onset));
    // Without the switch, or without first words kept: the usual 300 ms.
    for (soft, kept) in [(false, true), (true, false), (false, false)] {
        let (begins, onset) = start(soft, kept, &soft_word(), false);
        assert_eq!(begins, usual(onset), "soft {soft} kept {kept}");
    }
}

#[test]
fn a_soft_start_reaches_at_most_half_a_second_before_the_usual_one() {
    // A run of 200 blocks before the onset: talk going on all along.
    let (begins, onset) = start(true, true, &vec![Some(0.6); 200], false);
    assert_eq!(begins, usual(onset) - SOFT_START_BLOCKS as u64);
}

#[test]
fn a_pause_of_a_tenth_of_a_second_ends_what_a_soft_start_reaches() {
    // "Lamp," 20 blocks, a 120 ms pause scored quiet, then the soft word of
    // 30 blocks: the name, inside the half second, is not reached.
    let mut before = vec![Some(0.9); 20];
    before.extend(std::iter::repeat_n(Some(0.1), 12));
    before.extend(vec![Some(0.5); 30]);
    let (begins, onset) = start(true, true, &before, false);
    assert_eq!(begins, onset - 30 - REACH_LEAD_BLOCKS as u64);
    // A gap of 9 quiet blocks inside the word is gone through; 10 is a pause.
    for (gap, reached) in [(9, 49), (10, 30)] {
        let mut before = vec![Some(0.5); 10];
        before.extend(std::iter::repeat_n(Some(0.1), gap));
        before.extend(vec![Some(0.5); 30]);
        let (begins, onset) = start(true, true, &before, false);
        assert_eq!(
            begins,
            onset - reached - REACH_LEAD_BLOCKS as u64,
            "gap {gap}"
        );
    }
}

/// "what" on lamp-4ace: the word's first block at the usual start, so the
/// line's sound began at it, and the word was lost. It gets its 200 ms.
#[test]
fn a_word_begun_at_the_usual_start_gets_its_200_ms_before() {
    let (begins, onset) = start(true, true, &vec![Some(0.5); 24], false);
    assert_eq!(usual(onset), onset - 24);
    assert_eq!(begins, onset - 24 - REACH_LEAD_BLOCKS as u64);
    // Said 200 ms or more inside the usual start already: 200 ms more all
    // the same.
    let (begins, onset) = start(true, true, &vec![Some(0.5); 4], false);
    assert_eq!(begins, usual(onset) - SOFT_START_LEAD_BLOCKS as u64);
}

#[test]
fn a_soft_start_never_reaches_into_the_lamps_own_sound() {
    // The block 30 before the onset was heard while the lamp sounded.
    let mut before = soft_word();
    before[10] = None;
    let (begins, onset) = start(true, true, &before, false);
    assert_eq!(begins, onset - 29);
}

/// A word the detector's scores are slow on: nothing speech-like before the
/// onset, and still 200 ms more than the usual 300 ms.
#[test]
fn every_soft_start_keeps_200_ms_more_than_the_usual() {
    let (begins, onset) = start(true, true, &vec![Some(0.2); 40], false);
    assert_eq!(begins, usual(onset) - SOFT_START_LEAD_BLOCKS as u64);
    // And with the lamp sounding 10 blocks before the usual start, from
    // after that block.
    let mut before = vec![Some(0.2); 40];
    before[40 - 24 - 10] = None;
    let (begins, onset) = start(true, true, &before, false);
    assert_eq!(begins, usual(onset) - 9);
}

#[test]
fn a_yields_start_is_not_a_soft_start() {
    // A yield reaches back by its own scores, which here are all 0: the
    // usual start, as without the switch.
    let (begins, onset) = start(true, true, &soft_word(), true);
    assert_eq!(begins, usual(onset));
}
