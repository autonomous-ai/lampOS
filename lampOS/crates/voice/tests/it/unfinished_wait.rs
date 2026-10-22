//! A line whose words so far end unfinished waits longer before its end
//! (#3026, `LAMP_SECOND_UNFINISHED_WAIT`): the detector holds a due end for
//! at most `WAIT_BLOCKS` more of quiet while it is told so; a finished line,
//! and a detector told nothing, end as before.
use lamp_voice::activity::{Activity, ObservedAudio, TurnDetector};
use lamp_voice::unfinished_words::{Trailing, WAIT_BLOCKS, trailing};

fn frame(sequence: u64) -> ObservedAudio {
    ObservedAudio {
        sequence,
        captured_at_us: sequence * 10_000,
        samples: [0; 160],
    }
}

/// The lamp's 600 ms silence window.
fn lamp() -> TurnDetector {
    TurnDetector::v1_baseline().with_end_silence(60).unwrap()
}

struct Line {
    detector: TurnDetector,
    sequence: u64,
    starts: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            detector: lamp(),
            sequence: 0,
            starts: 0,
        }
    }
    fn push(&mut self, probability: f32) -> Activity {
        self.sequence += 1;
        let activity = self.detector.push(frame(self.sequence), probability);
        if matches!(activity, Activity::Start(_)) {
            self.starts += 1;
        }
        activity
    }
    fn speak(&mut self, blocks: u16) {
        for _ in 0..blocks {
            assert!(!matches!(self.push(0.95), Activity::End(_)));
        }
    }
    /// Quiet blocks until the line ends, if it does within `most`.
    fn quiet_to_end(&mut self, most: u16) -> Option<u16> {
        (1..=most).find(|_| matches!(self.push(0.0), Activity::End(_)))
    }
}

#[test]
fn unfinished_words_wait_and_end_within_the_bound() {
    let mut line = Line::new();
    line.speak(150);
    // "Lamp, I'm thinking about"
    line.detector
        .words_end(trailing("Lamp, I'm thinking about"));
    let ended = line.quiet_to_end(400).unwrap();
    assert_eq!(ended, 60 + WAIT_BLOCKS);
    let end = line.detector.last_endpoint().unwrap();
    assert_eq!(end.unfinished_wait_ms, Some(u64::from(WAIT_BLOCKS) * 10));
    let waited = line.detector.take_unfinished_wait().unwrap();
    assert_eq!(waited.class, Trailing::Joining);
    assert!(!waited.went_on);
    assert_eq!(line.detector.take_unfinished_wait(), None);
}

#[test]
fn a_finished_question_does_not_wait() {
    let mut line = Line::new();
    line.speak(150);
    line.detector
        .words_end(trailing("Lamp, what's the weather in Hanoi?"));
    assert_eq!(line.quiet_to_end(400), Some(60));
    assert_eq!(
        line.detector.last_endpoint().unwrap().unfinished_wait_ms,
        None
    );
    assert_eq!(line.detector.take_unfinished_wait(), None);
}

#[test]
fn a_person_who_stopped_after_um_is_answered_at_the_bound() {
    let mut line = Line::new();
    line.speak(120);
    line.detector.words_end(trailing("Lamp, can you, um"));
    // Never past the window and the wait: the bound.
    assert_eq!(line.quiet_to_end(1_000), Some(60 + WAIT_BLOCKS));
    let waited = line.detector.take_unfinished_wait().unwrap();
    assert_eq!(waited.class, Trailing::Filler);
    assert_eq!(waited.waited_ms, u64::from(WAIT_BLOCKS) * 10);
}

#[test]
fn speech_in_the_wait_goes_on_with_the_same_line() {
    let mut line = Line::new();
    line.speak(150);
    line.detector
        .words_end(trailing("Lamp, I'm thinking about"));
    // The pause: past the window, within the wait.
    for _ in 0..100 {
        assert!(matches!(line.push(0.0), Activity::Continue(_)));
    }
    line.speak(150);
    let waited = line.detector.take_unfinished_wait().unwrap();
    assert!(waited.went_on);
    assert_eq!(waited.waited_ms, 400);
    // "… going for a run tomorrow morning, is that a good idea?"
    line.detector.words_end(trailing(
        "going for a run tomorrow morning, is that a good idea?",
    ));
    assert_eq!(line.quiet_to_end(400), Some(60));
    assert_eq!(line.starts, 1);
}

#[test]
fn finished_words_told_in_the_wait_end_the_line_at_once() {
    let mut line = Line::new();
    line.speak(150);
    line.detector.words_end(trailing("Turn on the"));
    for _ in 0..80 {
        assert!(matches!(line.push(0.0), Activity::Continue(_)));
    }
    // The service's later words: "Turn on the light".
    line.detector.words_end(trailing("Turn on the light"));
    assert!(matches!(line.push(0.0), Activity::End(_)));
    assert_eq!(
        line.detector.last_endpoint().unwrap().unfinished_wait_ms,
        Some(210)
    );
}

#[test]
fn without_words_told_the_line_ends_as_before() {
    let mut line = Line::new();
    line.speak(150);
    assert_eq!(line.quiet_to_end(400), Some(60));
    assert_eq!(
        line.detector.last_endpoint().unwrap().unfinished_wait_ms,
        None
    );
    // Words told with no line in hand hold nothing of the next one.
    line.detector.words_end(Some(Trailing::Filler));
    line.speak(150);
    assert_eq!(line.quiet_to_end(400), Some(60));
}
