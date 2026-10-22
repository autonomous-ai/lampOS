//! A line whose words so far end unfinished waits longer before its end
//! (#3026, `LAMP_SECOND_UNFINISHED_WAIT`): the detector holds a due end for
//! at most `WAIT_BLOCKS` more of quiet while it is told so; a finished line,
//! and a detector told nothing, end as before.
use lamp_turn_end_wire::{Availability, Classifier, Outcome, Submit, Token, Verdict};
use lamp_voice::activity::{Activity, ConfirmPolicy, ObservedAudio, TurnDetector};
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
    /// With the end-of-turn model, as `--endpoint smart-turn-fast` runs it
    /// (the responsive profile on the 600 ms window), every verdict
    /// `probability`.
    fn smart_turn(probability: f32) -> Self {
        Self {
            detector: lamp()
                .with_confirmation(
                    Box::new(Fixed(probability, None)),
                    ConfirmPolicy::responsive(),
                )
                .unwrap(),
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

/// An end-of-turn model whose every verdict is the same.
struct Fixed(f32, Option<Token>);

impl Classifier for Fixed {
    fn submit(&mut self, token: Token, _pcm: &[i16]) -> Submit {
        self.1 = Some(token);
        Submit::Accepted
    }
    fn poll(&mut self) -> Option<Outcome> {
        self.1.take().map(|token| Outcome {
            token,
            worker: 1,
            request: 1,
            result: Ok(Verdict {
                probability: self.0,
                ..Verdict::default()
            }),
        })
    }
    fn availability(&self) -> Availability {
        Availability::Ready
    }
    fn cancel(&mut self) {
        self.1 = None;
    }
}

/// El on #3045 (P2): with the end-of-turn model on, a person who stopped
/// after "um" is answered at the model's incomplete wait (1.5 s), never
/// that plus the hold; an uncertain verdict's end, on the window, is held
/// to the window plus the hold at most.
#[test]
fn with_smart_turn_a_stop_after_um_is_answered_within_the_bound() {
    let incomplete = ConfirmPolicy::responsive().incomplete_blocks;
    let mut line = Line::smart_turn(0.001);
    line.speak(150);
    line.detector.words_end(trailing("Lamp, can you, um"));
    assert_eq!(line.quiet_to_end(1_000), Some(incomplete));
    let end = line.detector.last_endpoint().unwrap();
    assert_eq!(end.basis, "model_incomplete");
    assert_eq!(end.unfinished_wait_ms, None);
    assert_eq!(line.detector.take_unfinished_wait(), None);

    let mut line = Line::smart_turn(0.5);
    line.speak(150);
    line.detector.words_end(trailing("Lamp, can you, um"));
    assert_eq!(line.quiet_to_end(1_000), Some(60 + WAIT_BLOCKS));
    let end = line.detector.last_endpoint().unwrap();
    assert_eq!(end.basis, "model_uncertain");
    assert_eq!(end.quiet_ms(), u64::from(60 + WAIT_BLOCKS) * 10);
}

/// El on #3045 (P3): the hold is the line's. A noise block lets a hold go
/// at 1.0 s; the next due end on the same words is held only for the 0.3 s
/// left. A line whose noise came 10 ms before its hold ran out is held
/// 10 ms more at its next end: 1.3 s in all, never another 1.3 s.
#[test]
fn a_lines_holds_add_up_to_the_bound() {
    let mut line = Line::new();
    line.speak(150);
    line.detector
        .words_end(trailing("Lamp, I'm thinking about"));
    for _ in 0..60 + 100 {
        assert!(matches!(line.push(0.0), Activity::Continue(_)));
    }
    // A chair, a TV: one block scored as speech.
    line.speak(1);
    assert_eq!(
        line.detector.take_unfinished_wait().unwrap().waited_ms,
        1_000
    );
    assert_eq!(line.quiet_to_end(400), Some(60 + WAIT_BLOCKS - 100));
    assert_eq!(
        line.detector.last_endpoint().unwrap().unfinished_wait_ms,
        Some(u64::from(WAIT_BLOCKS - 100) * 10)
    );

    // Another line, whose hold is all but taken by one pause.
    let mut line = Line::new();
    line.speak(150);
    line.detector
        .words_end(trailing("Lamp, I'm thinking about"));
    for _ in 0..60 + WAIT_BLOCKS - 1 {
        assert!(matches!(line.push(0.0), Activity::Continue(_)));
    }
    line.speak(1);
    assert_eq!(line.quiet_to_end(400), Some(60 + 1));
    assert_eq!(
        line.detector.last_endpoint().unwrap().unfinished_wait_ms,
        Some(10)
    );
    assert_eq!(line.starts, 1);
}
