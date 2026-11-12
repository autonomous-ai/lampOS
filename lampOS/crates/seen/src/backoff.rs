//! The server is full (#6543): a 503 with `Retry-After` pauses sending, as
//! V1 paused on it (`retry_after_s` in
//! `hal/drivers/sensing/perceptions/processors/utils.py`, PR #831): the
//! header's seconds, held between 2 and 30 s, a missing or date-form header
//! taken as 5 s, then jittered by ±20 %.
//!
//! While paused, nothing is sent and nothing is published: no label goes
//! out, so the skills go on by their own clock and presence rules, as they
//! do with this part off. A pure state machine: the caller passes the time
//! and the jitter, so tests need no clock and no randomness.

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
}
