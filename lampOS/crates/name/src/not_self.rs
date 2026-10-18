//! A hearing that began while the lamp itself spoke is not said
//! (`--not-while-speaking on`, off unless given; #705, the bare name's
//! step 4).
//!
//! The lamp's own voice reaches its microphone, through the canceller, and
//! its name in the lamp's own words can be heard: on lamp-4ace on 18 Oct
//! (11:52) the installed model heard "There you go, I've made the light
//! warmer for you" at 0.990. Most readers of `name/heard` and `name/rising`
//! take any hearing: the voice marks the request in hand as called (a busy
//! room plays it, a policy hold is passed over), tells the model it was
//! called and starts its call's wait; only the body looks at whether the
//! lamp speaks, and as the message arrives. So the one place that knows
//! when a hearing began passes over one that began while `voice/state` said
//! `speaking`, or within [`TAIL`] after: a score reaches the bar a little
//! after the sound it is of, and the room rings on.
//!
//! What it costs: a person who calls the lamp by name while it speaks, or
//! right after, is not heard as calling it. Their words still reach the
//! voice, as a cut-in does, without the name's sign. Nothing known of the
//! voice (no `voice/state` for [`STALE`]) is not speaking: a hearing then
//! goes out as before.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// How long after the lamp stops speaking a hearing that begins is still
/// taken for its own: the scores' lag behind their sound (a step's window
/// ends after the sound it holds) and the room's ring.
pub const TAIL: Duration = Duration::from_millis(600);
/// `voice/state` is said when it changes and every second: unheard this
/// long, what it last said is no longer known.
pub const STALE: Duration = Duration::from_millis(2_500);
/// A hearing that began this soon after the lamp stopped speaking says how
/// soon, in the journal.
pub const NEAR: Duration = Duration::from_secs(3);
/// How far back the spans of speaking are kept: longer than a hearing
/// lasts before it is said (`said.rs`, 0.8 s) and its first sign.
const KEPT: Duration = Duration::from_secs(10);

/// When the lamp spoke, as `voice/state` said it.
#[derive(Debug, Default)]
pub struct Speaking {
    /// Each span of speaking: from, and until if it has ended.
    spans: VecDeque<(Instant, Option<Instant>)>,
    /// When `voice/state` was last heard.
    heard: Option<Instant>,
    /// The last hearing judged, by when it began, and whether it went out:
    /// its `name/heard` goes as its `name/rising` went.
    judged: Option<(Instant, bool)>,
    /// Hearings passed over, since the program started.
    pub passed_over: u64,
}

impl Speaking {
    /// `voice/state` said `speaking` (or not) at `at`.
    pub fn saw(&mut self, speaking: bool, at: Instant) {
        self.heard = Some(at);
        match (self.spans.back_mut(), speaking) {
            (Some((_, until @ None)), false) => *until = Some(at),
            (Some((_, None)), true) => {}
            (_, true) => self.spans.push_back((at, None)),
            (_, false) => {}
        }
        while self.spans.front().is_some_and(|(_, until)| {
            until.is_some_and(|until| at.saturating_duration_since(until) > KEPT)
        }) {
            self.spans.pop_front();
        }
    }

    /// `voice/state`'s publisher is gone: whatever it was saying is over.
    pub fn gone(&mut self, at: Instant) {
        self.saw(false, at);
    }

    /// Whether a hearing that began at `began` began while the lamp spoke,
    /// or within [`TAIL`] after, as known at `now`.
    pub fn covers(&self, began: Instant, now: Instant) -> bool {
        // A span still open when the state went unheard ends where it was
        // last heard of.
        let stale_from = self
            .heard
            .filter(|heard| now.saturating_duration_since(*heard) >= STALE);
        self.spans.iter().any(|(from, until)| {
            let until = until.or(stale_from);
            began >= *from && until.is_none_or(|until| began <= until + TAIL)
        })
    }

    /// Whether a hearing that began `ago` before `now` goes out. Its first
    /// sign and its final word are judged once, by the first of them: a
    /// hearing passed over at `name/rising` is passed over at `name/heard`
    /// too, and one let through is let through at both.
    pub fn lets_through(&mut self, ago: Duration, now: Instant) -> bool {
        let began = now.checked_sub(ago).unwrap_or(now);
        // The same hearing is dated from its first high score both times,
        // to within a millisecond's rounding.
        let same =
            |earlier: Instant| began.max(earlier) - began.min(earlier) <= Duration::from_millis(2);
        if let Some((earlier, through)) = self.judged
            && same(earlier)
        {
            return through;
        }
        let through = !self.covers(began, now);
        self.judged = Some((began, through));
        self.passed_over += u64::from(!through);
        through
    }

    /// How long after the lamp last stopped speaking a hearing that began
    /// `ago` before `now` began: zero inside a span, none if no span ended
    /// within [`NEAR`] before it. For the journal, so a lamp's own hearings
    /// say how far [`TAIL`] has to reach (the reviewer on #1486).
    pub fn after_speech(&self, ago: Duration, now: Instant) -> Option<Duration> {
        let began = now.checked_sub(ago).unwrap_or(now);
        let stale_from = self
            .heard
            .filter(|heard| now.saturating_duration_since(*heard) >= STALE);
        let (_, until) = self.spans.iter().rev().find(|(from, _)| *from <= began)?;
        match until.or(stale_from) {
            None => Some(Duration::ZERO),
            Some(until) if began <= until => Some(Duration::ZERO),
            Some(until) => Some(began - until).filter(|after| *after <= NEAR),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The two cases asked for on #705: a hearing that began inside the
    /// lamp's reply is passed over, both its first sign and its final word;
    /// one that began just after the reply and its tail goes out.
    #[test]
    fn a_hearing_inside_a_reply_is_passed_over_and_one_after_it_is_not() {
        let start = Instant::now();
        let mut speaking = Speaking::default();
        speaking.saw(false, start);
        speaking.saw(true, start + ms(1_000));
        speaking.saw(false, start + ms(4_000));
        // Inside the reply: the name in the lamp's own words at 2.5 s, its
        // first sign at 2.9 s and its final word at 3.3 s.
        assert!(!speaking.lets_through(ms(400), start + ms(2_900)));
        assert!(!speaking.lets_through(ms(800), start + ms(3_300)));
        assert_eq!(speaking.passed_over, 1);
        // Just after the reply and its tail: a person calling at 4.7 s.
        assert!(speaking.lets_through(ms(300), start + ms(5_000)));
        assert!(speaking.lets_through(ms(700), start + ms(5_400)));
        // Within the tail is still the lamp's own.
        assert!(speaking.covers(start + ms(4_500), start + ms(5_000)));
        assert!(!speaking.covers(start + ms(4_601), start + ms(5_000)));
        // Before the reply began, too, it goes out.
        assert!(!speaking.covers(start + ms(900), start + ms(5_000)));
        assert_eq!(speaking.passed_over, 1);
    }

    /// While it still speaks, every hearing is its own; a voice unheard for
    /// 2.5 s, or gone, is not speaking any more, so hearings go out again.
    #[test]
    fn a_reply_still_going_covers_and_a_silent_or_gone_voice_does_not() {
        let start = Instant::now();
        let mut speaking = Speaking::default();
        // Nothing known: hearings go out.
        assert!(speaking.lets_through(ms(100), start));
        speaking.saw(true, start + ms(1_000));
        speaking.saw(true, start + ms(2_000));
        assert!(!speaking.lets_through(ms(200), start + ms(2_500)));
        // Unheard since 2 s: from 4.5 s on, it ended at 2 s.
        assert!(speaking.covers(start + ms(2_400), start + ms(4_500)));
        assert!(!speaking.covers(start + ms(3_000), start + ms(4_500)));
        // Gone.
        let mut gone = Speaking::default();
        gone.saw(true, start);
        gone.gone(start + ms(500));
        assert!(!gone.covers(start + ms(1_200), start + ms(1_300)));
    }

    /// The reviewer on #1486: a hearing let through at its first sign is let
    /// through at its final word, even when the voice is heard speaking
    /// again in between (after 3 s unheard).
    #[test]
    fn a_hearing_let_through_at_its_first_sign_is_let_through_at_its_last() {
        let start = Instant::now();
        let mut speaking = Speaking::default();
        speaking.saw(true, start);
        // Unheard 3 s: the reply counts as ended where it was last heard.
        assert!(speaking.lets_through(ms(0), start + ms(3_000)));
        speaking.saw(true, start + ms(3_100));
        // The same hearing's final word, 0.4 s on.
        assert!(speaking.lets_through(ms(400), start + ms(3_400)));
        assert_eq!(speaking.passed_over, 0);
    }

    /// How long after the lamp stopped a hearing began, for the journal.
    #[test]
    fn a_hearing_says_how_long_after_the_lamps_speech_it_began() {
        let start = Instant::now();
        let mut speaking = Speaking::default();
        speaking.saw(true, start);
        speaking.saw(false, start + ms(1_000));
        let now = start + ms(5_000);
        assert_eq!(speaking.after_speech(ms(4_500), now), Some(Duration::ZERO));
        assert_eq!(speaking.after_speech(ms(3_700), now), Some(ms(300)));
        assert_eq!(speaking.after_speech(ms(500), now), None);
        assert_eq!(Speaking::default().after_speech(ms(0), now), None);
    }

    /// Spans older than ten seconds are let go.
    #[test]
    fn old_spans_are_let_go() {
        let start = Instant::now();
        let mut speaking = Speaking::default();
        for second in 0..30u64 {
            speaking.saw(second % 2 == 0, start + ms(second * 1_000));
        }
        assert!(speaking.spans.len() <= 6, "{}", speaking.spans.len());
    }
}
