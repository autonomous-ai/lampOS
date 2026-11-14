//! The server is full (#6543): a 503 with `Retry-After` pauses sending, as
//! V1 paused on it (`retry_after_s` in
//! `hal/drivers/sensing/perceptions/processors/utils.py`, PR #831): the
//! header's seconds, held between 2 and 30 s, a missing or date-form header
//! taken as 5 s, then jittered by ±20 %.
//!
//! While paused, nothing is sent. The HTTP routes (posture, later) pause
//! on it; activity's WebSocket never gets a 503, so its bursts have their
//! own [`Retreat`]. While either holds, `humans/doing` still goes out with
//! the sitting's timers, so the skills go on by their own clock. Pure
//! state machines: the caller passes the time and the jitter, so tests
//! need no clock and no randomness.

/// The pause when the header is missing or not a number of seconds.
pub const DEFAULT_S: f64 = 5.0;
/// The shortest pause.
pub const LEAST_S: f64 = 2.0;
/// The longest pause.
pub const MOST_S: f64 = 30.0;
/// The jitter, either way: ±20 %.
pub const JITTER: f64 = 0.2;

/// What came back from one send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply<'a> {
    /// Answered: no pause.
    Answered,
    /// 503, with the `Retry-After` header's value, if any.
    Busy { retry_after: Option<&'a str> },
}

/// How long to pause after a 503, in microseconds: V1's `retry_after_s`.
/// `jitter` is a draw from 0 to 1 (0.5 is no jitter); outside that it is
/// held to it.
pub fn pause_us(retry_after: Option<&str>, jitter: f64) -> u64 {
    let base = retry_after
        .and_then(|value| value.trim().parse::<f64>().ok())
        .unwrap_or(DEFAULT_S);
    // As Python's `min(hi, max(lo, nan))`: a NaN is the least.
    let held = if base.is_nan() {
        LEAST_S
    } else {
        base.clamp(LEAST_S, MOST_S)
    };
    let jitter = if jitter.is_nan() {
        0.5
    } else {
        jitter.clamp(0.0, 1.0)
    };
    let factor = 1.0 - JITTER + 2.0 * JITTER * jitter;
    (held * factor * 1_000_000.0).round() as u64
}

/// Whether sending is paused, and until when.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Backoff {
    /// Paused until this time (the caller's monotonic microseconds).
    until_us: Option<u64>,
    /// 503s since the last answer, for `diagnostics`.
    busy_in_a_row: u32,
}

impl Backoff {
    /// Whether a send may go now.
    pub fn may_send(&self, now_us: u64) -> bool {
        self.until_us.is_none_or(|until| now_us >= until)
    }

    /// When the pause ends, while paused at `now_us`.
    pub fn paused_until(&self, now_us: u64) -> Option<u64> {
        self.until_us.filter(|until| now_us < *until)
    }

    /// 503s since the last answer.
    pub fn busy_in_a_row(&self) -> u32 {
        self.busy_in_a_row
    }

    /// Take a send's reply at `now_us`, with a jitter draw from 0 to 1.
    pub fn take(&mut self, reply: &Reply<'_>, now_us: u64, jitter: f64) {
        match reply {
            Reply::Answered => {
                self.until_us = None;
                self.busy_in_a_row = 0;
            }
            Reply::Busy { retry_after } => {
                self.until_us = Some(now_us.saturating_add(pause_us(*retry_after, jitter)));
                self.busy_in_a_row = self.busy_in_a_row.saturating_add(1);
            }
        }
    }
}

/// Activity's bursts (#6543 phase 2) on a WebSocket, where the server
/// never sends a 503 or `Retry-After` (a busy frame gets `{"error":"busy"}`
/// and the socket stays open): after [`BUSY_RUN`] busy bursts in a row, or
/// any burst that failed (no connection, an HTTP refusal such as a 5xx, a
/// refused key exchange, a server error, no answer in time), the next burst
/// waits [`RETREAT_LEAST_S`], doubling each time to [`RETREAT_MOST_S`],
/// with ±20 % jitter, and the part says `warn` on `lamp/health` until a
/// burst is answered.
pub const BUSY_RUN: u32 = 3;
/// The first wait after a run of busy bursts or a failure.
pub const RETREAT_LEAST_S: u64 = 60;
/// The longest.
pub const RETREAT_MOST_S: u64 = 600;

/// How one burst ended, as the retreat takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// Answered for every frame sent.
    Answered,
    /// The server said busy: the burst dropped.
    Busy,
    /// No connection, refused, or no answer in time.
    Failed,
}

/// Whether bursts are held back after busy runs and failures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Retreat {
    busy_in_a_row: u32,
    /// Waits taken since the last answer: the next one doubles.
    steps: u32,
    until_us: Option<u64>,
}

impl Retreat {
    /// The wait for the `step`th retreat in a row (1 the first), before
    /// jitter, in seconds.
    pub fn wait_s(step: u32) -> u64 {
        let doubled = RETREAT_LEAST_S.saturating_mul(1u64 << step.saturating_sub(1).min(16));
        doubled.min(RETREAT_MOST_S)
    }

    /// Take a burst's end at `now_us`, with a jitter draw from 0 to 1.
    pub fn take(&mut self, ended: Ended, now_us: u64, jitter: f64) {
        let retreat = match ended {
            Ended::Answered => {
                *self = Self::default();
                return;
            }
            Ended::Busy => {
                self.busy_in_a_row = self.busy_in_a_row.saturating_add(1);
                self.busy_in_a_row >= BUSY_RUN
            }
            Ended::Failed => true,
        };
        if retreat {
            self.steps = self.steps.saturating_add(1);
            let jitter = if jitter.is_nan() {
                0.5
            } else {
                jitter.clamp(0.0, 1.0)
            };
            let factor = 1.0 - JITTER + 2.0 * JITTER * jitter;
            let wait = (Self::wait_s(self.steps) as f64 * factor * 1_000_000.0).round() as u64;
            self.until_us = Some(now_us.saturating_add(wait));
        }
    }

    /// Whether a burst may begin at `now_us`.
    pub fn may_start(&self, now_us: u64) -> bool {
        self.until_us.is_none_or(|until| now_us >= until)
    }

    /// When the wait ends, if one was set; it stays set (for
    /// `diagnostics`) until a burst is answered.
    pub fn until_us(&self) -> Option<u64> {
        self.until_us
    }

    /// Whether to say `warn` on `lamp/health`: retreating, until a burst is
    /// answered.
    pub fn warn(&self) -> bool {
        self.steps > 0
    }

    pub fn busy_in_a_row(&self) -> u32 {
        self.busy_in_a_row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn the_header_is_held_and_jittered_as_v1s() {
        // V1's own table (hal/test/test_fire_hazard.py), at both ends of the
        // jitter and its middle.
        for (header, middle) in [
            (Some("2"), 2 * S),
            (Some("100"), 30 * S),
            (Some("0"), 2 * S),
            (Some("-5"), 2 * S),
            (Some("7.5"), 7_500_000),
            (Some(" 3 "), 3 * S),
            (Some("Wed, 21 Oct 2015 07:28:00 GMT"), 5 * S),
            (Some(""), 5 * S),
            (Some("NaN"), 2 * S),
            (Some("inf"), 30 * S),
            (None, 5 * S),
        ] {
            assert_eq!(pause_us(header, 0.5), middle, "{header:?}");
            assert_eq!(pause_us(header, 0.0), middle * 8 / 10, "{header:?} low");
            assert_eq!(pause_us(header, 1.0), middle * 12 / 10, "{header:?} high");
        }
        // A draw outside 0..1 is held to it.
        assert_eq!(pause_us(Some("10"), -1.0), 8 * S);
        assert_eq!(pause_us(Some("10"), 9.0), 12 * S);
        assert_eq!(pause_us(Some("10"), f64::NAN), 10 * S);
    }

    #[test]
    fn a_503_pauses_until_its_time_and_an_answer_resumes() {
        let mut backoff = Backoff::default();
        assert!(backoff.may_send(0));
        backoff.take(
            &Reply::Busy {
                retry_after: Some("2"),
            },
            10 * S,
            0.5,
        );
        assert!(!backoff.may_send(10 * S));
        assert!(!backoff.may_send(12 * S - 1));
        assert_eq!(backoff.paused_until(11 * S), Some(12 * S));
        assert!(backoff.may_send(12 * S));
        assert_eq!(backoff.paused_until(12 * S), None);
        assert_eq!(backoff.busy_in_a_row(), 1);
        // A second 503 pauses again from its own time.
        backoff.take(&Reply::Busy { retry_after: None }, 12 * S, 0.5);
        assert!(!backoff.may_send(16 * S));
        assert!(backoff.may_send(17 * S));
        assert_eq!(backoff.busy_in_a_row(), 2);
        backoff.take(&Reply::Answered, 17 * S, 0.5);
        assert!(backoff.may_send(17 * S));
        assert_eq!(backoff.busy_in_a_row(), 0);
    }

    #[test]
    fn the_pause_never_wraps_the_clock() {
        let mut backoff = Backoff::default();
        backoff.take(
            &Reply::Busy {
                retry_after: Some("30"),
            },
            u64::MAX - 1,
            1.0,
        );
        assert!(!backoff.may_send(u64::MAX - 1));
    }

    #[test]
    fn busy_bursts_retreat_after_three_and_failures_at_once() {
        let mut retreat = Retreat::default();
        retreat.take(Ended::Busy, 0, 0.5);
        retreat.take(Ended::Busy, 60 * S, 0.5);
        assert!(!retreat.warn(), "two busy bursts: no retreat yet");
        assert!(retreat.may_start(60 * S));
        retreat.take(Ended::Busy, 120 * S, 0.5);
        assert!(retreat.warn());
        assert!(!retreat.may_start(180 * S - 1));
        assert!(retreat.may_start(180 * S));
        // Busy again: the wait doubles.
        retreat.take(Ended::Busy, 180 * S, 0.5);
        assert_eq!(retreat.until_us(), Some(300 * S));
        // An answer ends it.
        retreat.take(Ended::Answered, 300 * S, 0.5);
        assert!(!retreat.warn());
        assert!(retreat.may_start(300 * S));
        assert_eq!(retreat.busy_in_a_row(), 0);
        // A failure retreats at once, jittered ±20 %.
        retreat.take(Ended::Failed, 0, 0.0);
        assert_eq!(retreat.until_us(), Some(48 * S));
        retreat.take(Ended::Failed, 0, 1.0);
        assert_eq!(retreat.until_us(), Some(144 * S));
    }

    #[test]
    fn the_retreat_doubles_from_a_minute_to_ten() {
        let waits: Vec<u64> = (1..=7).map(Retreat::wait_s).collect();
        assert_eq!(waits, [60, 120, 240, 480, 600, 600, 600]);
        assert_eq!(Retreat::wait_s(u32::MAX), 600);
        let mut retreat = Retreat::default();
        for _ in 0..100 {
            retreat.take(Ended::Failed, u64::MAX - 1, 1.0);
        }
        assert!(!retreat.may_start(u64::MAX - 1));
    }
}
