//! Content-free conversation state sent from one provider worker to its parent.
//! A mirror belongs to that worker's boot, not to an individual model session.
use serde::{Deserialize, Serialize};
use std::{io, time::Instant};

const WINDOW_US: u64 = crate::thread::GOING_ON_FOR.as_micros() as u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub observed_at_us: u64,
    pub until_us: Option<u64>,
}

/// A publication and its exact source key. Acknowledge this token only after
/// successfully sending its snapshot; an old token cannot acknowledge a newer one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pending {
    pub snapshot: Snapshot,
    deadline: Option<Instant>,
}

#[derive(Default)]
pub struct Source {
    sent: Option<Option<Instant>>,
    pending: Option<Pending>,
}

impl Source {
    /// Keep a stable conversion across retries. A changed key replaces unsent
    /// state; returning to the last sent key cancels the unsent change.
    pub fn peek(
        &mut self,
        deadline: Option<Instant>,
        now: Instant,
        observed_at_us: u64,
    ) -> Option<Pending> {
        if self.sent == Some(deadline) {
            self.pending = None;
            return None;
        }
        if self
            .pending
            .is_some_and(|pending| pending.deadline == deadline)
        {
            return self.pending;
        }
        // A future-dated source exchange cannot grant more than the normal
        // conversation window. Real exchange timestamps are at or before now.
        // Preserve the inclusive deadline: equality is still active.
        let until_us = deadline.and_then(|until| {
            until.checked_duration_since(now).map(|left| {
                let left_us = left.as_micros().min(u128::from(WINDOW_US)) as u64;
                observed_at_us.saturating_add(left_us)
            })
        });
        let pending = Pending {
            snapshot: Snapshot {
                observed_at_us,
                until_us,
            },
            deadline,
        };
        self.pending = Some(pending);
        Some(pending)
    }

    pub fn sent(&mut self, pending: Pending) {
        if self.pending == Some(pending) {
            self.sent = Some(pending.deadline);
            self.pending = None;
        }
    }
}

/// Parent-local state for one worker. Construct a fresh mirror on replacement;
/// ordinary model-session changes leave it intact. The transport checks boot and
/// sequence before this additional clock and lifetime validation.
#[derive(Default, Debug)]
pub struct Mirror {
    observed_at_us: Option<u64>,
    until_us: Option<u64>,
}

impl Mirror {
    pub fn observe(&mut self, snapshot: Snapshot, now_us: u64) -> io::Result<()> {
        if snapshot.observed_at_us > now_us
            || self
                .observed_at_us
                .is_some_and(|at| snapshot.observed_at_us < at)
            || snapshot.until_us.is_some_and(|until| {
                until < snapshot.observed_at_us
                    || until.saturating_sub(snapshot.observed_at_us) > WINDOW_US
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid conversation deadline",
            ));
        }
        self.observed_at_us = Some(snapshot.observed_at_us);
        self.until_us = snapshot.until_us;
        Ok(())
    }

    pub fn going_on(&self, now_us: u64) -> bool {
        self.until_us.is_some_and(|until| now_us <= until)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn initial_empty_state_is_sent_once_and_retries_do_not_move_deadlines() {
        let now = Instant::now();
        let mut source = Source::default();
        let empty = source.peek(None, now, 10).unwrap();
        assert_eq!(empty.snapshot.until_us, None);
        assert_eq!(
            source.peek(None, now + Duration::from_secs(1), 1_000_010),
            Some(empty)
        );
        source.sent(empty);
        assert_eq!(source.peek(None, now, 10), None);
        let deadline = now + Duration::from_secs(2);
        let active = source.peek(Some(deadline), now, 10).unwrap();
        assert_eq!(active.snapshot.until_us, Some(2_000_010));
        assert_eq!(
            source.peek(Some(deadline), deadline, 2_000_010),
            Some(active)
        );
        source.sent(active);
        assert_eq!(
            source.peek(Some(deadline), deadline + Duration::from_secs(1), 3_000_010),
            None
        );
    }

    #[test]
    fn backpressure_coalesces_changes_and_stale_ack_cannot_clear_new_state() {
        let now = Instant::now();
        let mut source = Source::default();
        let empty = source.peek(None, now, 10).unwrap();
        source.sent(empty);
        let old = source
            .peek(Some(now + Duration::from_secs(2)), now, 10)
            .unwrap();
        let new = source
            .peek(Some(now + Duration::from_secs(3)), now, 10)
            .unwrap();
        source.sent(old);
        assert_eq!(source.peek(new.deadline, now, 10), Some(new));
        assert_eq!(source.peek(None, now, 10), None);
        source.sent(new);
        assert_eq!(source.peek(None, now, 10), None);
        let active = source.peek(old.deadline, now, 10).unwrap();
        source.sent(active);
        let cleared = source.peek(None, now, 11).unwrap();
        assert_eq!(cleared.snapshot.until_us, None);
    }

    #[test]
    fn expiry_is_inclusive_and_delivery_delay_never_extends_it() {
        let now = Instant::now();
        let mut source = Source::default();
        let pending = source
            .peek(Some(now + Duration::from_micros(20)), now, 100)
            .unwrap();
        let mut mirror = Mirror::default();
        mirror.observe(pending.snapshot, 119).unwrap();
        assert!(mirror.going_on(120));
        assert!(!mirror.going_on(121));
        mirror.observe(pending.snapshot, 200).unwrap();
        assert!(!mirror.going_on(200));
        let mut source = Source::default();
        assert_eq!(
            source.peek(Some(now), now, 100).unwrap().snapshot.until_us,
            Some(100)
        );
        let expired = source
            .peek(Some(now - Duration::from_nanos(1)), now, 100)
            .unwrap();
        assert_eq!(expired.snapshot.until_us, None);
    }

    #[test]
    fn invalid_clock_and_lifetime_leave_the_mirror_unchanged() {
        let mut mirror = Mirror::default();
        mirror
            .observe(
                Snapshot {
                    observed_at_us: 100,
                    until_us: Some(200),
                },
                100,
            )
            .unwrap();
        for snapshot in [
            Snapshot {
                observed_at_us: 102,
                until_us: None,
            },
            Snapshot {
                observed_at_us: 99,
                until_us: None,
            },
            Snapshot {
                observed_at_us: 100,
                until_us: Some(99),
            },
            Snapshot {
                observed_at_us: 100,
                until_us: Some(101 + WINDOW_US),
            },
        ] {
            assert!(mirror.observe(snapshot, 101).is_err());
            assert!(mirror.going_on(150));
        }
        mirror
            .observe(
                Snapshot {
                    observed_at_us: 101,
                    until_us: None,
                },
                101,
            )
            .unwrap();
        assert!(!mirror.going_on(150));
        assert!(!Mirror::default().going_on(150));
    }

    #[test]
    fn future_source_is_bounded_and_wire_rejects_unknown_content() {
        let now = Instant::now();
        let pending = Source::default()
            .peek(Some(now + Duration::from_secs(90)), now, 100)
            .unwrap();
        assert_eq!(pending.snapshot.until_us, Some(100 + WINDOW_US));
        Mirror::default().observe(pending.snapshot, 100).unwrap();
        assert!(
            serde_json::from_str::<Snapshot>(
                r#"{"observed_at_us":100,"until_us":null,"words":"private"}"#
            )
            .is_err()
        );
    }
}
