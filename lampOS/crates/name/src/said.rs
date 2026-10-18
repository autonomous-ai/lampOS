//! When a run of scores is one hearing of the name.
//!
//! The models give a score every 80 ms, and a name that is said raises it
//! for several steps in a row. That is one hearing, said once, with the
//! highest score it reached. Nothing here is slow: a hearing is said when
//! the score falls again, or after [`LONGEST`] if it does not. Its first
//! sign is said as soon as the score reaches the bar ([`Hearings::rising`],
//! `name/rising`, #782): a person looks up at their name as it is said.
use lamp_msgs::{NameHeard, StopHeard};
use std::time::{Duration, Instant};

/// A hearing is said after this long even if the score has not fallen.
pub const LONGEST: Duration = Duration::from_millis(800);
/// After a hearing, scores are passed over for this long: the same call
/// is not heard twice.
pub const HOLD_OFF: Duration = Duration::from_secs(1);

pub struct Hearings {
    least: f32,
    /// A hearing going on: when it began and the highest score so far.
    open: Option<(Instant, f32)>,
    quiet_until: Option<Instant>,
    /// Whether the open hearing's first sign has been said.
    rising_said: bool,
}

impl Hearings {
    /// Scores of `least` or more are the name being heard.
    pub fn new(least: f32) -> Self {
        Self {
            least,
            open: None,
            quiet_until: None,
            rising_said: false,
        }
    }

    /// Whether a hearing is going on.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The first sign of a hearing that has just begun, once: the score it
    /// began with, and how long ago it reached the bar. Nothing when no
    /// hearing is open or its first sign has been said.
    pub fn rising(&mut self, now: Instant) -> Option<NameHeard> {
        let (began, score) = self.open?;
        if self.rising_said {
            return None;
        }
        self.rising_said = true;
        Some(NameHeard {
            score,
            ago_ms: u32::try_from(now.saturating_duration_since(began).as_millis())
                .unwrap_or(u32::MAX),
        })
    }

    fn close(&mut self, now: Instant) -> Option<NameHeard> {
        let (began, score) = self.open.take()?;
        self.quiet_until = Some(now + HOLD_OFF);
        Some(NameHeard {
            score,
            ago_ms: u32::try_from(now.saturating_duration_since(began).as_millis())
                .unwrap_or(u32::MAX),
        })
    }

    /// A score that came at `now`. A hearing, if one is over with it.
    pub fn score(&mut self, score: f32, now: Instant) -> Option<NameHeard> {
        if self.quiet_until.is_some_and(|until| now < until) {
            return None;
        }
        match (&mut self.open, score >= self.least) {
            (Some((began, highest)), true) => {
                *highest = highest.max(score.min(1.0));
                (now.saturating_duration_since(*began) >= LONGEST)
                    .then(|| self.close(now))
                    .flatten()
            }
            (Some(_), false) => self.close(now),
            (None, true) => {
                self.open = Some((now, score.min(1.0)));
                self.rising_said = false;
                None
            }
            (None, false) => None,
        }
    }

    /// The sound stopped coming, or some of it was lost: a hearing that
    /// is open is said as it stands, dated from when it began.
    pub fn cut(&mut self, now: Instant) -> Option<NameHeard> {
        self.close(now)
    }

    /// Time has passed with no score: a hearing that has gone on long
    /// enough, or whose sound stopped coming, is said.
    pub fn tick(&mut self, now: Instant) -> Option<NameHeard> {
        self.open
            .is_some_and(|(began, _)| now.saturating_duration_since(began) >= LONGEST)
            .then(|| self.close(now))
            .flatten()
    }
}

/// When a stop word is heard (#828, `docs/stop-word.md`): at the first step
/// the stop model's score reaches the bar, said at once, since a stop is
/// wanted now and not when the word is over; then nothing new for
/// [`HOLD_OFF`], so one word is one hearing.
pub struct Stops {
    least: f32,
    quiet_until: Option<Instant>,
}

impl Stops {
    /// Scores of `least` or more are a stop word being heard.
    pub fn new(least: f32) -> Self {
        Self {
            least,
            quiet_until: None,
        }
    }

    /// The stop model's score for a step: a hearing at the step it first
    /// reaches the bar, outside the hold-off of the last one.
    pub fn score(&mut self, score: f32, now: Instant) -> Option<StopHeard> {
        if score < self.least || self.quiet_until.is_some_and(|until| now < until) {
            return None;
        }
        self.quiet_until = Some(now + HOLD_OFF);
        Some(StopHeard { score, ago_ms: 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// #782: the first sign as the score reaches the bar, once, before the
    /// hearing itself, which is said when the score falls as before.
    #[test]
    fn a_hearing_is_first_signed_as_it_begins_and_said_when_it_falls() {
        let start = Instant::now();
        let mut hearings = Hearings::new(0.9);
        assert_eq!(hearings.score(0.2, start), None);
        assert_eq!(hearings.rising(start), None);
        let opened = start + ms(80);
        assert!(!hearings.is_open());
        assert_eq!(hearings.score(0.93, opened), None);
        assert!(hearings.is_open());
        let first = hearings.rising(opened).unwrap();
        assert_eq!((first.score, first.ago_ms), (0.93, 0));
        assert_eq!(hearings.rising(opened + ms(80)), None);
        // It goes on rising, then falls: one hearing, at its highest, dated
        // from the first sign.
        assert_eq!(hearings.score(1.0, opened + ms(80)), None);
        let said = hearings.score(0.1, opened + ms(480)).unwrap();
        assert_eq!((said.score, said.ago_ms), (1.0, 480));
        // Held off for a second, then a new hearing has its own first sign.
        assert_eq!(hearings.score(0.95, opened + ms(800)), None);
        assert!(!hearings.is_open(), "a score held off opens nothing");
        assert_eq!(hearings.rising(opened + ms(800)), None);
        let again = opened + ms(1_600);
        assert_eq!(hearings.score(0.95, again), None);
        assert_eq!(hearings.rising(again).map(|sign| sign.score), Some(0.95));
    }

    #[test]
    fn a_run_of_high_scores_is_one_hearing_said_when_the_score_falls() {
        let start = Instant::now();
        let mut hearings = Hearings::new(0.9);
        // Talk, then the name over four steps, then talk.
        let scores = [0.01, 0.2, 0.93, 0.999, 0.97, 0.95, 0.3, 0.02];
        let mut said = Vec::new();
        for (step, score) in scores.into_iter().enumerate() {
            if let Some(heard) = hearings.score(score, start + ms(80 * step as u64)) {
                said.push((step, heard));
            }
        }
        // Once, at the step the score fell, with the highest it reached
        // and how long ago it began.
        assert_eq!(said.len(), 1);
        let (step, heard) = said[0];
        assert_eq!(step, 6);
        assert_eq!(heard.score, 0.999);
        assert_eq!(heard.ago_ms, 320);
        assert_eq!(heard.check(), Ok(()));
    }

    #[test]
    fn a_score_that_stays_high_is_said_after_its_longest_and_not_again_for_a_second() {
        let start = Instant::now();
        let mut hearings = Hearings::new(0.9);
        let mut said = Vec::new();
        // Three seconds of nothing but high scores, 80 ms apart.
        for step in 0..38u64 {
            if let Some(heard) = hearings.score(0.95, start + ms(80 * step)) {
                said.push((step, heard.ago_ms));
            }
        }
        // At 0.8 s, then nothing for a second, then a second hearing that
        // begins at 1.84 s and is said at 2.64 s.
        assert_eq!(said, [(10, 800), (33, 800)]);
    }

    /// Sound lost in the middle of a hearing: said at once, and dated from
    /// its first high score, not 0.8 s before it.
    #[test]
    fn a_hearing_cut_short_is_said_then_and_dated_from_when_it_began() {
        let start = Instant::now();
        let mut hearings = Hearings::new(0.9);
        assert_eq!(hearings.cut(start), None);
        assert_eq!(hearings.score(0.97, start + ms(80)), None);
        let heard = hearings.cut(start + ms(240)).unwrap();
        assert_eq!((heard.score, heard.ago_ms), (0.97, 160));
        // Said once, and the same call is not heard again for a second.
        assert_eq!(hearings.cut(start + ms(300)), None);
        assert_eq!(
            hearings.score(1.0, start + ms(240) + HOLD_OFF - ms(1)),
            None
        );
        assert_eq!(hearings.tick(start + ms(5_000)), None);
    }

    #[test]
    fn a_stop_word_is_said_at_the_step_it_reaches_the_bar_and_once_a_second() {
        let start = Instant::now();
        let mut stops = Stops::new(0.9);
        assert_eq!(stops.score(0.89, start), None);
        let heard = stops.score(0.93, start + ms(80)).unwrap();
        assert_eq!((heard.score, heard.ago_ms), (0.93, 0));
        // The same word, still over the bar: not said again.
        assert_eq!(stops.score(0.99, start + ms(160)), None);
        assert_eq!(stops.score(0.99, start + ms(80) + HOLD_OFF - ms(1)), None);
        // A second later, another word is another hearing.
        let again = stops.score(0.95, start + ms(80) + HOLD_OFF).unwrap();
        assert_eq!(again.score, 0.95);
    }

    #[test]
    fn a_hearing_whose_sound_stops_coming_is_said_by_the_clock() {
        let start = Instant::now();
        let mut hearings = Hearings::new(0.9);
        assert_eq!(hearings.score(0.96, start), None);
        assert_eq!(hearings.tick(start + ms(400)), None);
        let heard = hearings.tick(start + ms(800)).unwrap();
        assert_eq!((heard.score, heard.ago_ms), (0.96, 800));
        // Nothing is open, so the clock says nothing more.
        assert_eq!(hearings.tick(start + ms(5_000)), None);
        // And a score under the least never opens one.
        assert_eq!(hearings.score(0.89, start + ms(6_000)), None);
        assert_eq!(hearings.tick(start + ms(9_000)), None);
    }
}
