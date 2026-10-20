//! How often a camera that did not open is tried again (#1941, Fifth's row 17
//! on lamp-52e6): after a mute its opens failed with `DeadlineExceeded` at
//! `StreamOn`, `SetInterval`, `RequestBuffers` or `Controls` every 5 s until a
//! reboot, a restart of lamp-eyes included. Each ioctl returned, only late, so
//! lamp-eyes was not hung: the device was. Such a refusal can also end by
//! itself after minutes (#763), so the camera keeps being tried, but after
//! half a minute of tries every 15 s instead of every 5, said once, to keep
//! its USB and the journal quieter. A stuck camera's USB device is reset, as
//! an unplug would (`lamp_camera::linux::reset_usb`): at the sixth failure,
//! then at most once in ten minutes while it stays stuck. Never a reboot.

use std::time::Duration;

/// A camera that did not open, or failed, is tried again after this.
pub const RETRY: Duration = Duration::from_secs(5);
/// After this many opens in a row that did not open, the camera is taken as
/// stuck: about half a minute of tries.
pub const STUCK_AFTER: u32 = 6;
/// A stuck camera is tried again after this: a refusal that ends by itself
/// is still seen within it.
pub const RETRY_STUCK: Duration = Duration::from_secs(15);

/// When to try again after `failed` opens in a row that did not open.
pub fn retry_after(failed: u32) -> Duration {
    if failed >= STUCK_AFTER {
        RETRY_STUCK
    } else {
        RETRY
    }
}

/// Whether the open that just failed, the `failed`th in a row, is the one at
/// which the camera is taken as stuck: said once, then not again until it
/// opens.
pub fn now_stuck(failed: u32) -> bool {
    failed == STUCK_AFTER
}

/// Tries at [`RETRY_STUCK`] between two resets of a camera that stays
/// stuck: ten minutes.
pub const RESET_EVERY: u32 = 40;

/// Whether the camera's USB device is reset after the `failed`th open in a
/// row that did not open: at [`STUCK_AFTER`], then every [`RESET_EVERY`]
/// tries after it.
pub fn reset_now(failed: u32) -> bool {
    failed >= STUCK_AFTER && (failed - STUCK_AFTER).is_multiple_of(RESET_EVERY)
}

/// The open-retry state of one camera: how many opens in a row did not open
/// and how often its USB device was reset. lamp-eyes asks it after each
/// open and does what it says.
#[derive(Debug, Default)]
pub struct Reopen {
    failed: u32,
    resets: u32,
    /// A reset fell due while seeing was not permitted: done at the first
    /// failure after it is again.
    owed: bool,
}

/// What to do after an open that did not open.
#[derive(Debug, PartialEq, Eq)]
pub struct Failed {
    /// The `failed`th in a row.
    pub failed: u32,
    /// Try again after this.
    pub retry_after: Duration,
    /// Say once that the camera seems stuck.
    pub stuck: bool,
    /// Reset its USB device now (and call [`Reopen::reset`] if it was):
    /// only while seeing is permitted, never while muted or the switch is
    /// closed (the reviewer's P3 on #2129).
    pub reset: bool,
}

impl Reopen {
    /// An open that did not open; `permitted`, whether seeing is permitted
    /// now, after it.
    pub fn failed(&mut self, permitted: bool) -> Failed {
        self.failed = self.failed.saturating_add(1);
        let due = self.owed || reset_now(self.failed);
        self.owed = due && !permitted;
        Failed {
            failed: self.failed,
            retry_after: retry_after(self.failed),
            stuck: now_stuck(self.failed),
            reset: due && permitted,
        }
    }

    /// The camera opened: the opens in a row that did not, when it had been
    /// taken as stuck, to say so.
    pub fn opened(&mut self) -> Option<u32> {
        let failed = std::mem::take(&mut self.failed);
        self.resets = 0;
        self.owed = false;
        (failed >= STUCK_AFTER).then_some(failed)
    }

    /// Its USB device was reset.
    pub fn reset(&mut self) {
        self.resets = self.resets.saturating_add(1);
    }

    /// How often its USB device was reset since it last opened.
    pub fn resets(&self) -> u32 {
        self.resets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake camera that refuses `StreamOn` for its first `refusals` opens,
    /// or until its USB device is reset when `reset_cures`; driven as
    /// lamp-eyes drives the real one, on a clock of its own. Gives when it
    /// opened, the opens tried, what was said and the resets.
    fn drive(refusals: u32, reset_cures: bool) -> (Duration, u32, Vec<String>, u32) {
        drive_muted(refusals, reset_cures, &[])
    }

    /// As [`drive`], with seeing not permitted after the opens in `muted`.
    fn drive_muted(
        refusals: u32,
        reset_cures: bool,
        muted: &[u32],
    ) -> (Duration, u32, Vec<String>, u32) {
        let mut reopen = Reopen::default();
        let mut at = Duration::ZERO;
        let mut tried = 0;
        let mut said = Vec::new();
        let mut cured = false;
        loop {
            tried += 1;
            if cured || tried > refusals {
                let resets = reopen.resets();
                if let Some(failed) = reopen.opened() {
                    said.push(format!("open again after {failed}"));
                }
                assert_eq!(reopen.resets(), 0, "counted per stuck spell");
                return (at, tried, said, resets);
            }
            let failed = reopen.failed(!muted.contains(&tried));
            if failed.stuck {
                said.push(format!("stuck at {}", failed.failed));
            }
            if failed.reset {
                reopen.reset();
                said.push(format!("reset at {}", failed.failed));
                cured |= reset_cures;
            }
            at += failed.retry_after;
            assert!(tried < 1_000, "never opened");
        }
    }

    /// A camera that refuses `StreamOn` a few times opens on the next try,
    /// 5 s apart, with nothing said and no reset.
    #[test]
    fn a_few_refusals_are_tried_every_five_seconds_with_nothing_said() {
        assert_eq!(drive(0, false), (Duration::ZERO, 1, vec![], 0));
        assert_eq!(drive(3, false), (Duration::from_secs(15), 4, vec![], 0));
        assert_eq!(drive(5, false), (Duration::from_secs(25), 6, vec![], 0));
    }

    /// Fifth's 52e6 (#2126): a camera stuck after a mute that a reset cures
    /// opens on the try after the first reset, 40 s after the first refusal,
    /// with no reboot; stuck and reset said once each, and "open again".
    #[test]
    fn a_stuck_camera_that_a_reset_cures_opens_on_the_try_after_it() {
        let (at, tried, said, resets) = drive(u32::MAX, true);
        assert_eq!((at, tried, resets), (Duration::from_secs(25 + 15), 7, 1));
        assert_eq!(said, ["stuck at 6", "reset at 6", "open again after 6"]);
    }

    /// A camera a reset does not cure, which ends by itself after 50
    /// refusals (#763's minutes): every 15 s once stuck, a reset at the 6th
    /// and the 46th only, said stuck once, then "open again".
    #[test]
    fn a_camera_that_stays_stuck_is_reset_at_most_every_ten_minutes_until_it_opens() {
        let (at, tried, said, resets) = drive(50, false);
        assert_eq!(tried, 51);
        assert_eq!(at, Duration::from_secs(5 * 5 + 15 * 45));
        assert_eq!(resets, 2);
        assert_eq!(
            said,
            [
                "stuck at 6",
                "reset at 6",
                "reset at 46",
                "open again after 50"
            ]
        );
    }

    /// Five seconds for the first tries, then every 15 s; said once, at
    /// the sixth failure in a row, and not again while it stays stuck.
    #[test]
    fn a_camera_failing_in_a_row_is_tried_less_often_and_said_stuck_once() {
        for failed in 0..STUCK_AFTER {
            assert_eq!(retry_after(failed), RETRY, "{failed}");
            assert!(!now_stuck(failed), "{failed}");
        }
        assert!(now_stuck(STUCK_AFTER));
        for failed in STUCK_AFTER..STUCK_AFTER + 100 {
            assert_eq!(retry_after(failed), RETRY_STUCK, "{failed}");
        }
        assert!(
            (STUCK_AFTER + 1..u32::MAX)
                .step_by(997)
                .all(|failed| !now_stuck(failed))
        );
        assert_eq!(retry_after(u32::MAX), RETRY_STUCK);
    }

    /// A camera that stays stuck is reset at the sixth failure, then once
    /// every 40 tries (ten minutes at 15 s): never more often, never before
    /// it is taken as stuck.
    #[test]
    fn a_stuck_camera_is_reset_at_once_then_at_most_every_ten_minutes() {
        let resets: Vec<u32> = (0..200).filter(|&failed| reset_now(failed)).collect();
        assert_eq!(resets, [6, 46, 86, 126, 166]);
        assert_eq!(
            RETRY_STUCK * RESET_EVERY,
            Duration::from_secs(600),
            "ten minutes between resets"
        );
    }

    /// The reviewer's P3 on #2129: a mute that lands during the sixth open
    /// puts the reset off; it is done at the first failure after seeing is
    /// permitted again, never while muted.
    #[test]
    fn a_reset_due_while_muted_waits_for_the_first_failure_after_it() {
        let (_, tried, said, resets) = drive_muted(u32::MAX, true, &[6, 7]);
        assert_eq!((tried, resets), (9, 1));
        assert_eq!(said, ["stuck at 6", "reset at 8", "open again after 8"]);
    }
}
