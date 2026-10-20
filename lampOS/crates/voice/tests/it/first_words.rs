//! A yield's first words (#828, `LAMP_FIRST_WORDS=kept`): the detector keeps
//! 1.6 s of pre-roll with each block's own score, and a start the duplex rule
//! marks as a yield begins 200 ms before the earliest speech-like block of
//! it, never past a block heard without echo evidence. Every other start,
//! and every start without the switch, keeps its 300 ms.
use lamp_voice::activity::{
    Activity, ObservedAudio, PRE_ROLL_BLOCKS, REACH_BACK_BLOCKS, REACH_LEAD_BLOCKS, START_BLOCKS,
    TurnDetector,
};

fn frame(sequence: u64) -> ObservedAudio {
    ObservedAudio {
        sequence,
        captured_at_us: sequence * 10_000,
        samples: [0; 160],
    }
}

/// 100 blocks the lamp's own, scored low; blocks 101 to 140 the person's
/// first words, which the mask gave the detector as nothing, though their
/// own score is speech; ten blocks of a breath; then the onset, marked a
/// yield on its last block when `yield_start`. Block `blind`, if any, was
/// heard without echo evidence. The sequence the start's audio begins at.
fn start(kept: bool, yield_start: bool, blind: Option<u64>, first_words: f32) -> u64 {
    let mut detector = TurnDetector::v1_baseline();
    detector.keep_first_words(kept);
    let mut sequence = 0;
    for n in 1..=150u64 {
        sequence += 1;
        let own = match n {
            _ if blind == Some(n) => None,
            101..=140 => Some(first_words),
            _ => Some(0.05),
        };
        assert!(matches!(
            detector.push_heard(frame(sequence), 0.0, own),
            Activity::Quiet
        ));
    }
    for n in 1..=START_BLOCKS {
        sequence += 1;
        if yield_start && n == START_BLOCKS {
            detector.reach_back();
        }
        if let Activity::Start(audio) = detector.push_heard(frame(sequence), 0.95, Some(0.95)) {
            assert!(audio.len() <= REACH_BACK_BLOCKS);
            assert!(
                audio
                    .windows(2)
                    .all(|pair| pair[1].sequence == pair[0].sequence + 1)
            );
            assert_eq!(audio.last().unwrap().sequence, sequence);
            return audio[0].sequence;
        }
    }
    panic!("the onset did not start an utterance");
}

#[test]
fn a_yields_start_reaches_back_to_the_first_words_only_with_them_kept() {
    let onset = 150 + START_BLOCKS as u64;
    let usual = onset + 1 - PRE_ROLL_BLOCKS as u64;
    assert_eq!(start(true, true, None, 0.9), 101 - REACH_LEAD_BLOCKS as u64);
    // Not a yield, or not kept: the usual 300 ms.
    assert_eq!(start(true, false, None, 0.9), usual);
    assert_eq!(start(false, true, None, 0.9), usual);
    assert_eq!(start(false, false, None, 0.9), usual);
    // Nothing speech-like before the onset: nothing more.
    assert_eq!(start(true, true, None, 0.2), usual);
    // A block heard without evidence is as far back as it may go.
    assert_eq!(start(true, true, Some(120), 0.9), 121);
    // Inside the usual 300 ms: as without the switch.
    assert_eq!(start(true, true, Some(130), 0.9), usual);
}

/// What is kept is at most 1.6 s: first words said longer before the onset
/// than that are not reached.
#[test]
fn first_words_are_reached_only_within_the_last_one_point_six_seconds() {
    let mut detector = TurnDetector::v1_baseline();
    detector.keep_first_words(true);
    let mut sequence = 0;
    // Speech-like for 20 blocks, then 200 blocks of the lamp alone.
    for n in 1..=220u64 {
        sequence += 1;
        let own = if n <= 20 { 0.9 } else { 0.05 };
        detector.push_heard(frame(sequence), 0.0, Some(own));
    }
    for n in 1..=START_BLOCKS {
        sequence += 1;
        if n == START_BLOCKS {
            detector.reach_back();
        }
        if let Activity::Start(audio) = detector.push_heard(frame(sequence), 0.95, Some(0.95)) {
            assert_eq!(audio[0].sequence, sequence + 1 - PRE_ROLL_BLOCKS as u64);
            return;
        }
    }
    panic!("the onset did not start an utterance");
}
