//! A hearing that began while the body moved needs a higher bar
//! (`--moving-least SCORE`, or `LAMP_NAME_MOVING_LEAST=on` for 0.99; off
//! unless given; #4208).
//!
//! The lamp's own motors reach its microphone, and its name can be heard in
//! them: on lamp-a0ae on 29 Oct (02:13 to 02:19), with nobody in the room,
//! `heard its name` came at 0.945 to 0.978 while it played `greeting`,
//! `dance_groove`, `dance_hype` and stood up for a `nod`, and each false wake
//! ended the gesture and suspended a computer's body lease. So while the body
//! plays a gesture (`body/gesture` said `began`, and not yet `over` or
//! `dropped`), a hearing that began within [`TAIL`] before it began, or after
//! it ended, or between, goes out only at the higher bar. A person who calls
//! the lamp mid-dance is still heard, at that bar; one whose call scores under
//! it (a usual call scores 0.95 to 0.98) is not heard as calling it, for as
//! long as the gesture plays, up to [`LONGEST`].
//!
//! The body's own small shifts at rest and its `scanning` say `began` and
//! `over` on `body/gesture` too ([`AT_REST`]); they are no gesture a person
//! asked for or would call one, so they leave the bar as it is.
//!
//! `body/gesture` is news once, never repeated: a `began` whose end was never
//! heard stops counting [`LONGEST`] after it, the longest a clip plays and
//! more, so a lost message never leaves the bar raised.
use std::time::{Duration, Instant};

/// After a gesture ends, its motors' sound and a score that comes a little
/// after the sound it is of.
pub const TAIL: Duration = Duration::from_millis(800);
/// The longest a gesture is taken to play without its end heard: the
/// longest clip (`dance_waltz`, 9.6 s) and more.
pub const LONGEST: Duration = Duration::from_secs(15);
/// `LAMP_NAME_MOVING_LEAST=on`'s bar.
pub const DEFAULT_MOVING_LEAST: f32 = 0.99;
/// What lamp-body plays of its own at rest, and says on `body/gesture` as it
/// does a gesture: its shifts at rest (`lamp_body::idle::MOVES`) and its look
/// around (`scanning`, which the control schema doesn't offer either).
pub const AT_REST: [&str; 6] = [
    "idle",
    "tilt_left",
    "tilt_right",
    "look_up",
    "dip",
    "scanning",
];

/// Whether a `body/gesture` of this name raises the bar: not one of
/// [`AT_REST`].
pub fn counts(name: &str) -> bool {
    !AT_REST.contains(&name)
}

/// When the body moved, as `body/gesture` said it.
#[derive(Debug, Default)]
pub struct Moving {
    /// The gesture playing now: when it began.
    since: Option<Instant>,
    /// The last one's span, began to ended.
    last: Option<(Instant, Instant)>,
    /// Hearings passed over for it, for the minute's line
    /// (`passed_over_moving_in_all`).
    pub passed_over: u64,
}

impl Moving {
    /// A gesture began.
    pub fn began(&mut self, at: Instant) {
        if let Some(since) = self.since.take() {
            self.last = Some((since, at));
        }
        self.since = Some(at);
    }

    /// The gesture playing ended, or was dropped.
    pub fn ended(&mut self, at: Instant) {
        if let Some(since) = self.since.take() {
            self.last = Some((since, at));
        }
    }

    /// Whether a hearing that began at `began` came while the body moved, or
    /// within [`TAIL`] before or after, as known at `now`.
    pub fn covers(&self, began: Instant, now: Instant) -> bool {
        let playing = self.since.is_some_and(|since| {
            now.saturating_duration_since(since) < LONGEST && began + TAIL >= since
        });
        let after = self
            .last
            .is_some_and(|(since, until)| began + TAIL >= since && began <= until + TAIL);
        playing || after
    }

    /// Whether a hearing said now, begun `ago` before at `score`, goes out
    /// with this bar.
    pub fn lets_through(&mut self, score: f32, least: f32, ago: Duration, now: Instant) -> bool {
        let began = now.checked_sub(ago).unwrap_or(now);
        if score >= least || !self.covers(began, now) {
            return true;
        }
        self.passed_over += 1;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEAST: f32 = 0.99;

    #[test]
    fn a_hearing_while_a_gesture_plays_needs_the_higher_bar() {
        let start = Instant::now();
        let mut moving = Moving::default();
        moving.began(start);
        let now = start + Duration::from_secs(3);
        // a0ae's false wakes: 0.945 and 0.962, mid-dance.
        assert!(!moving.lets_through(0.945, LEAST, Duration::from_millis(80), now));
        assert!(!moving.lets_through(0.962, LEAST, Duration::from_millis(80), now));
        // A person calling it mid-dance, at the bar: heard.
        assert!(moving.lets_through(0.995, LEAST, Duration::from_millis(80), now));
        assert_eq!(moving.passed_over, 2);
    }

    #[test]
    fn after_the_tail_the_usual_bar_is_back() {
        let start = Instant::now();
        let mut moving = Moving::default();
        moving.began(start);
        moving.ended(start + Duration::from_secs(5));
        let in_tail = start + Duration::from_secs(5) + Duration::from_millis(500);
        assert!(!moving.lets_through(0.95, LEAST, Duration::from_millis(80), in_tail));
        let later = start + Duration::from_secs(7);
        assert!(moving.lets_through(0.95, LEAST, Duration::from_millis(80), later));
    }

    #[test]
    fn a_hearing_begun_before_the_gesture_began_goes_as_before() {
        let start = Instant::now();
        let mut moving = Moving::default();
        moving.began(start + Duration::from_secs(2));
        // Said after the gesture began, but begun a second before it.
        let now = start + Duration::from_secs(2) + Duration::from_millis(100);
        assert!(moving.lets_through(0.95, LEAST, Duration::from_millis(1_100), now));
    }

    #[test]
    fn a_hearing_begun_just_before_the_gesture_began_needs_the_higher_bar() {
        let start = Instant::now();
        let mut moving = Moving::default();
        moving.began(start + Duration::from_secs(2));
        // Begun 0.5 s before the gesture, inside the tail before it.
        let now = start + Duration::from_secs(2) + Duration::from_millis(100);
        assert!(!moving.lets_through(0.95, LEAST, Duration::from_millis(600), now));
    }

    #[test]
    fn a_gesture_that_replaces_another_keeps_the_bar_up_from_the_first() {
        let start = Instant::now();
        let mut moving = Moving::default();
        moving.began(start);
        // The second began before the first was said over.
        moving.began(start + Duration::from_secs(3));
        let now = start + Duration::from_secs(4);
        // Begun in the first: still covered, by the first's span.
        assert!(!moving.lets_through(0.95, LEAST, Duration::from_millis(2_500), now));
        // Begun in the second.
        assert!(!moving.lets_through(0.95, LEAST, Duration::from_millis(80), now));
        moving.ended(start + Duration::from_secs(6));
        let later = start + Duration::from_secs(8);
        assert!(moving.lets_through(0.95, LEAST, Duration::from_millis(80), later));
    }

    #[test]
    fn the_shifts_at_rest_and_scanning_leave_the_bar_and_a_gesture_raises_it() {
        for name in AT_REST {
            assert!(!counts(name), "{name}");
        }
        for name in [
            "dance_groove",
            "dance_hype",
            "greeting",
            "nod",
            "dance_waltz",
        ] {
            assert!(counts(name), "{name}");
        }
        // Every shift the body makes at rest is in the list, and nothing else
        // but `scanning`.
        let shifts: Vec<&str> = lamp_body::idle::MOVES
            .iter()
            .map(|shift| shift.name)
            .collect();
        let mut listed: Vec<&str> = AT_REST
            .iter()
            .copied()
            .filter(|name| *name != "scanning")
            .collect();
        listed.sort_unstable();
        let mut sorted = shifts.clone();
        sorted.sort_unstable();
        assert_eq!(listed, sorted);
        assert!(lamp_body::clips::named("scanning").is_some());
    }

    #[test]
    fn a_begun_never_ended_stops_counting_after_the_longest_clip() {
        let start = Instant::now();
        let mut moving = Moving::default();
        moving.began(start);
        let now = start + LONGEST + Duration::from_secs(1);
        assert!(moving.lets_through(0.95, LEAST, Duration::from_millis(80), now));
    }

    #[test]
    fn nothing_moved_lets_every_hearing_through() {
        let mut moving = Moving::default();
        let now = Instant::now();
        assert!(moving.lets_through(0.91, LEAST, Duration::from_millis(80), now));
        assert_eq!(moving.passed_over, 0);
    }
}
