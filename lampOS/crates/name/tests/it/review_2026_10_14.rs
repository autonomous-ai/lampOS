//! Review of what changed on the night of 2026-10-14
//! (`docs/voice-review-2026-10-14.md`): tests only.
//!
//! When a run of scores is one hearing of the name (`lamp_name::said`),
//! with the clock passed in.
//!
//! The tests whose names begin `review_` show a fault and fail today. The
//! ones that begin `review_sound_` are edges that were tried and held.
use lamp_name::said::{HOLD_OFF, Hearings, LONGEST};
use std::time::{Duration, Instant};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The models' last score is not held to 0..1 anywhere: `name.onnx` is
/// ours and `Hearer::load` checks only that silence scores in range. The
/// first score of a hearing is held to 1 when it opens one; a higher score
/// later in the same hearing is not, and the hearing is said with it. The
/// message then breaks its own type's rule (`NameHeard::check`: "score is
/// ...: from 0 to 1"), which the voice does not check and other readers of
/// `name/heard` may.
#[test]
fn review_a_hearing_is_never_said_with_a_score_over_one() {
    let start = Instant::now();
    let mut hearings = Hearings::new(0.9);
    assert_eq!(hearings.score(0.95, start), None);
    assert_eq!(hearings.score(1.02, start + ms(80)), None);
    let heard = hearings
        .score(0.1, start + ms(160))
        .expect("the hearing is said when the score falls");
    assert_eq!(
        heard.check(),
        Ok(()),
        "said with a score of {}",
        heard.score
    );
}

/// One call is one hearing however long its score stays up, the next is
/// heard after the hold-off, and a score that is not a number opens
/// nothing. Its date is from the score that opened it.
#[test]
fn review_sound_one_call_is_one_hearing_dated_from_its_first_high_score() {
    let start = Instant::now();
    let mut hearings = Hearings::new(0.9);
    assert_eq!(hearings.score(f32::NAN, start), None);
    assert_eq!(hearings.score(0.97, start + ms(80)), None);
    let heard = hearings
        .tick(start + ms(80) + LONGEST)
        .expect("said by the clock");
    assert_eq!(heard.ago_ms, 800);
    // Within the hold-off, nothing; just after it, a new hearing opens.
    let after = start + ms(80) + LONGEST;
    assert_eq!(hearings.score(1.0, after + HOLD_OFF - ms(1)), None);
    assert_eq!(hearings.score(1.0, after + HOLD_OFF), None);
    let next = hearings
        .score(0.2, after + HOLD_OFF + ms(80))
        .expect("the second call");
    assert_eq!((next.score, next.ago_ms), (1.0, 80));
}
