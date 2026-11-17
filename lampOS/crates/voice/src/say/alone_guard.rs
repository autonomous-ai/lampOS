//! A say held to the owner seen alone (the lamp's agent's answer to a read
//! told in full, `crate::agent_asks`; Heron's P2 2 on #7509). Checked on the
//! say client's own thread as the line is read, as a skill's task's yes is
//! (`crate::skill_task_yes::asked_aloud_here`), and again on that thread
//! while the line waits and while it is said: the voice reads only the
//! atomic deadline, at the moment it takes the turn and as it speaks
//! (`Ticket::cut_why`). Once the owner is not seen alone, the say never
//! regains it, even if they are again a moment later.
use super::unplug_guard::Guard;

/// Why such a say is refused or cut: the owner not seen alone (nor the
/// private room) as it was read, taken or said.
pub const NOT_ALONE: &str = "owner_not_alone";
/// How often the client's thread looks again, at most: each look reads the
/// eyes' known file.
const LOOK_EVERY_US: u64 = 250_000;
/// How long one look holds: a stalled client thread never keeps a say
/// allowed past it.
const FRESH_US: u64 = 600_000;

pub(super) struct Monitor {
    guard: Guard,
    alone: fn() -> bool,
    looked_us: u64,
    failed: bool,
}

impl Monitor {
    /// Looked at once, now: refused [`NOT_ALONE`] unless `alone` holds.
    pub(super) fn new(alone: fn() -> bool, now_us: u64) -> Result<Self, &'static str> {
        if !alone() {
            return Err(NOT_ALONE);
        }
        let guard = Guard::new(NOT_ALONE);
        guard.set(now_us.saturating_add(FRESH_US));
        Ok(Self {
            guard,
            alone,
            looked_us: now_us,
            failed: false,
        })
    }

    pub(super) fn guard(&self) -> Guard {
        self.guard.clone()
    }

    /// Looked at again, at most each [`LOOK_EVERY_US`].
    pub(super) fn refresh(&mut self, now_us: u64) {
        if self.failed {
            return;
        }
        if !self.guard.valid(now_us) {
            self.invalidate();
            return;
        }
        if now_us.saturating_sub(self.looked_us) < LOOK_EVERY_US {
            return;
        }
        self.looked_us = now_us;
        if (self.alone)() {
            self.guard.set(now_us.saturating_add(FRESH_US));
        } else {
            self.invalidate();
        }
    }

    fn invalidate(&mut self) {
        self.failed = true;
        self.guard.set(0);
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    static ALONE: AtomicBool = AtomicBool::new(true);

    fn alone() -> bool {
        ALONE.load(Ordering::Acquire)
    }

    /// Refused at once when not alone; held while alone, looked at again
    /// only each 250 ms; lost for good once someone else is seen, and when
    /// its thread stalls past one look's hold.
    #[test]
    fn the_owner_alone_is_looked_at_as_it_waits_and_never_regained() {
        ALONE.store(false, Ordering::Release);
        assert_eq!(Monitor::new(alone, 1_000).err(), Some(NOT_ALONE));
        ALONE.store(true, Ordering::Release);
        let mut monitor = Monitor::new(alone, 1_000).unwrap();
        let guard = monitor.guard();
        assert!(guard.valid(1_000));
        assert_eq!(guard.why(), NOT_ALONE);
        monitor.refresh(300_000);
        assert!(guard.valid(800_000));
        // Someone walks in: a look within 250 ms of the last one doesn't
        // see it yet, the next one does, for good.
        ALONE.store(false, Ordering::Release);
        monitor.refresh(400_000);
        assert!(guard.valid(400_000));
        monitor.refresh(600_000);
        assert!(!guard.valid(600_000));
        ALONE.store(true, Ordering::Release);
        monitor.refresh(900_000);
        assert!(!guard.valid(900_000));
        // A thread that stops looking: no longer valid past one hold.
        let stalled = Monitor::new(alone, 1_000).unwrap();
        assert!(!stalled.guard().valid(1_000 + FRESH_US));
        // Dropped with its client: never valid again.
        let dropped = stalled.guard();
        drop(stalled);
        assert!(!dropped.valid(1_001));
    }
}
