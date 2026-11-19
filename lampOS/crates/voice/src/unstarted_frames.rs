//! Capture frames that come before the speaker's clock has started again
//! (#7224, fault 4). A held turn whose capture frames stop is cut before
//! its lease lapses (#4332), and both audio workers are revoked with it:
//! capture reopens its microphone under the fresh authority, its sequence
//! counting from 1 again, while the speaker re-primes on the same authority,
//! which clears its clock's start until `ClockStarted`. At first start the
//! microphone waits for that start; a reopen did not, so a reopened
//! microphone's first frame could go out before it, and the coordinator,
//! whose check guards the boot barrier, ended the voice with exit 1
//! ("capture frame preceded certified running reference clock").
//!
//! With `LAMP_VOICE_UNSTARTED_FRAME_DROP=on` (on in the product unit) the
//! capture worker sends no frame after a reopen until the current playback
//! epoch's clock has started ([`ReopenHold`]): it waits up to
//! [`WAIT_US`], and past that drops and counts frames until the clock
//! comes. The person loses at most that sound, never the process. A clock
//! that still has not come [`RECOVER_US`] after the reopen is recovered
//! once: capture closes the microphone as a gap and reopens it under the
//! next fresh authority, as after a stall, while the speaker re-primes on
//! its own for as long as its clock has not started; if that brings no
//! started clock within another [`RECOVER_US`], capture ends with the
//! error the coordinator ended the voice with before, so the voice exits
//! and systemd restarts it. Never silent for good. The
//! coordinator, as a backstop, ends the voice on a frame that fails that
//! check only at the boot barrier, before the first certified start;
//! anywhere else the frame is a gap's dropped frame, said once per reopen
//! ([`UnstartedFrames`]). Off, exactly as before.

use crate::wire::ReferenceTiming;

/// The switch, read by the voice and passed to its capture worker.
pub const ENVIRONMENT: &str = "LAMP_VOICE_UNSTARTED_FRAME_DROP";

/// How long a reopen waits for the clock before its frames are counted as
/// dropped past the wait.
pub const WAIT_US: u64 = 500_000;

/// How long after a reopen a clock that has not started is recovered by a
/// fresh reopen, and how long after that reopen it ends capture. Both are
/// inside the coordinator's own 1.5 s retention heartbeat counted from the
/// last kept frame (`coordinator::CAPTURE_GAP_US`), which ends the voice
/// first if neither has.
pub const RECOVER_US: u64 = 800_000;

/// The error the coordinator's check ended the voice with, kept for the
/// exit that a failed recovery ends in.
pub const UNSTARTED_ERROR: &str = "capture frame preceded certified running reference clock";

/// What the capture worker does about a clock that has not come.
#[derive(Debug, PartialEq, Eq)]
pub enum Recovery {
    /// Close the microphone as a gap and reopen it under the next fresh
    /// authority; with the journal line.
    Reopen(String),
    /// End capture with [`UNSTARTED_ERROR`]; with the journal line.
    Exit(String),
}

/// On only for exactly `on`.
pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The switch as this process's environment sets it.
pub fn from_environment() -> bool {
    switched_on(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether a frame's reference timing says the clock it was cancelled
/// against has not started: no playback epoch yet, or the first frame of a
/// microphone epoch with no clock start. The coordinator's check.
pub fn unstarted(frame_sequence: u64, reference: &ReferenceTiming) -> bool {
    reference.playback_epoch == 0
        || (frame_sequence == 1 && reference.clock_started_at_us.is_none())
}

/// The capture worker's side: after a reopen, frames are held back until
/// the current playback epoch's clock has started.
#[derive(Debug, Default)]
pub struct ReopenHold {
    on: bool,
    /// Since when a reopen has waited for the clock.
    waiting_since: Option<u64>,
    /// Frames held back inside the wait, and past it, since the last line.
    held: u64,
    past_wait: u64,
    /// The wait's bound was passed and said.
    overran: bool,
    /// The recovery's reopen has been made: the next bound ends capture.
    recovering: bool,
}

impl ReopenHold {
    pub fn new(on: bool) -> Self {
        Self {
            on,
            ..Self::default()
        }
    }

    /// The microphone reopened after a gap, at `at_us`.
    pub fn reopened(&mut self, at_us: u64) {
        if self.on && self.waiting_since.is_none() {
            self.waiting_since = Some(at_us);
            self.overran = false;
        }
    }

    /// Each pass, whether the clock that a reopen waits for has come, and
    /// if not, what its bounds call for. Once a recovery's reopen is asked
    /// for, the wait starts again at that reopen.
    pub fn due(&mut self, reference: &ReferenceTiming, at_us: u64) -> Option<Recovery> {
        let since = self.waiting_since?;
        if reference.playback_epoch != 0 && reference.clock_started_at_us.is_some() {
            return None;
        }
        let waited = at_us.saturating_sub(since);
        if waited < RECOVER_US {
            return None;
        }
        let dropped = std::mem::take(&mut self.held) + std::mem::take(&mut self.past_wait);
        if self.recovering {
            return Some(Recovery::Exit(format!(
                "lamp-voice: capture: the speaker's clock has not started {} ms after the recovery's reopen, {dropped} blocks dropped since; recovery step 2: capture ends ({ENVIRONMENT}=on)",
                waited / 1_000
            )));
        }
        self.recovering = true;
        self.waiting_since = None;
        self.overran = false;
        Some(Recovery::Reopen(format!(
            "lamp-voice: capture: the speaker's clock has not started {} ms after a reopen, {dropped} blocks dropped; recovery step 1: the microphone is reopened under a fresh authority ({ENVIRONMENT}=on)",
            waited / 1_000
        )))
    }

    /// Whether a frame cancelled against `reference` may be sent at `at_us`.
    /// After a reopen, one before the clock has started is dropped and
    /// counted: inside the wait as held, past it as dropped.
    pub fn takes(&mut self, reference: &ReferenceTiming, at_us: u64) -> bool {
        let Some(since) = self.waiting_since else {
            return true;
        };
        if reference.playback_epoch != 0 && reference.clock_started_at_us.is_some() {
            self.waiting_since = None;
            self.recovering = false;
            return true;
        }
        if at_us.saturating_sub(since) < WAIT_US {
            self.held += 1;
        } else {
            self.past_wait += 1;
        }
        false
    }

    /// A journal line, counts only: once when the wait passes its bound,
    /// and once when the clock has come after frames were held.
    pub fn line(&mut self) -> Option<String> {
        if self.waiting_since.is_some() {
            if self.past_wait > 0 && !self.overran {
                self.overran = true;
                return Some(format!(
                    "lamp-voice: capture: the speaker's clock has not started {} ms after a reopen; {} blocks held, dropping until it does ({ENVIRONMENT}=on)",
                    WAIT_US / 1_000,
                    self.held
                ));
            }
            return None;
        }
        if self.held == 0 && self.past_wait == 0 {
            return None;
        }
        let (held, past) = (
            std::mem::take(&mut self.held),
            std::mem::take(&mut self.past_wait),
        );
        Some(format!(
            "lamp-voice: capture: {} blocks read after a reopen before the speaker's clock started again were dropped, {past} of them past the {} ms wait ({ENVIRONMENT}=on)",
            held + past,
            WAIT_US / 1_000
        ))
    }
}

/// What the coordinator does with a frame that failed only the
/// unstarted-clock check.
#[derive(Debug, PartialEq, Eq)]
pub enum Unstarted {
    /// End the voice, as before: the switch is off, or this is the boot
    /// barrier.
    Exit,
    /// Drop it as a gap's frame; with the line to journal, once per reopen.
    Drop(Option<String>),
}

/// The coordinator's side, a backstop to [`ReopenHold`].
#[derive(Debug, Default)]
pub struct UnstartedFrames {
    on: bool,
    /// A frame has passed the check: the first certified start is behind.
    certified: bool,
    /// The microphone epoch last said.
    said_epoch: Option<u64>,
}

impl UnstartedFrames {
    pub fn new(on: bool) -> Self {
        Self {
            on,
            ..Self::default()
        }
    }

    /// A frame passed the check.
    pub fn passed(&mut self) {
        self.certified = true;
    }

    /// A frame of microphone epoch `epoch` failed only the unstarted-clock
    /// check.
    pub fn failed(&mut self, epoch: u64) -> Unstarted {
        if !self.on || !self.certified {
            return Unstarted::Exit;
        }
        if self.said_epoch == Some(epoch) {
            return Unstarted::Drop(None);
        }
        self.said_epoch = Some(epoch);
        Unstarted::Drop(Some(format!(
            "lamp-voice: a capture frame of microphone epoch {epoch} came before the speaker's clock had started again; dropped as a gap ({ENVIRONMENT}=on)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(epoch: u64, at: Option<u64>) -> ReferenceTiming {
        ReferenceTiming {
            playback_epoch: epoch,
            clock_started_at_us: at,
            ..ReferenceTiming::default()
        }
    }

    #[test]
    fn on_only_for_exactly_on() {
        assert!(switched_on(Some("on")));
        for value in [
            None,
            Some(""),
            Some("off"),
            Some("ON"),
            Some("1"),
            Some("on "),
        ] {
            assert!(!switched_on(value), "{value:?}");
        }
    }

    #[test]
    fn the_check_is_the_coordinators() {
        assert!(unstarted(5, &started(0, Some(1))));
        assert!(unstarted(1, &started(2, None)));
        assert!(!unstarted(2, &started(2, None)));
        assert!(!unstarted(1, &started(2, Some(1))));
    }

    /// A re-prime during a reopen: sequence 1 and what follows it, read
    /// before `ClockStarted`, are held back and counted, and go once the
    /// clock has started; one line with the count.
    #[test]
    fn after_a_reopen_frames_wait_for_the_clock_and_are_counted() {
        let mut hold = ReopenHold::new(true);
        // Before any reopen nothing is held, even with no start.
        assert!(hold.takes(&started(1, None), 0));
        hold.reopened(1_000_000);
        // The speaker re-primed: epoch 2, its clock not started.
        for at in [1_000_000, 1_010_000, 1_020_000] {
            assert!(!hold.takes(&started(2, None), at));
            assert_eq!(hold.line(), None, "said only once the wait ends");
        }
        assert!(hold.takes(&started(2, Some(1_025_000)), 1_030_000));
        let line = hold.line().unwrap();
        assert!(line.contains(": 3 blocks "), "{line}");
        assert!(line.contains("0 of them past"), "{line}");
        assert!(
            line.contains("LAMP_VOICE_UNSTARTED_FRAME_DROP=on"),
            "{line}"
        );
        assert_eq!(hold.line(), None);
        // Once started, a later re-prime with no reopen holds nothing back.
        assert!(hold.takes(&started(3, None), 1_040_000));
    }

    /// The 500 ms bound: past it the frames are still dropped and counted
    /// until the clock comes, said once at the bound and once at the end.
    #[test]
    fn past_the_wait_frames_are_dropped_and_counted_until_the_clock() {
        let mut hold = ReopenHold::new(true);
        hold.reopened(0);
        assert!(!hold.takes(&started(2, None), WAIT_US - 1));
        assert_eq!(hold.line(), None);
        assert!(!hold.takes(&started(2, None), WAIT_US));
        let bound = hold.line().unwrap();
        assert!(
            bound.contains("not started 500 ms after a reopen; 1 blocks held"),
            "{bound}"
        );
        assert!(!hold.takes(&started(2, None), WAIT_US + 10_000));
        assert_eq!(hold.line(), None, "the bound is said once");
        assert!(hold.takes(&started(2, Some(WAIT_US)), WAIT_US + 20_000));
        let end = hold.line().unwrap();
        assert!(end.contains(": 3 blocks "), "{end}");
        assert!(end.contains("2 of them past the 500 ms wait"), "{end}");
    }

    /// A clock that never starts after a reopen: recovered by a fresh
    /// reopen at the bound, said once, and if the clock still has not come
    /// within another bound, capture ends with the old error. It never ends
    /// in silence for good.
    #[test]
    fn a_clock_that_never_starts_is_recovered_then_ends_capture() {
        let mut hold = ReopenHold::new(true);
        let unstarted = started(2, None);
        hold.reopened(0);
        assert_eq!(hold.due(&unstarted, RECOVER_US - 1), None);
        assert!(!hold.takes(&unstarted, 100_000));
        let Some(Recovery::Reopen(line)) = hold.due(&unstarted, RECOVER_US) else {
            panic!("step 1 at the bound");
        };
        assert!(
            line.contains("800 ms after a reopen, 1 blocks dropped; recovery step 1"),
            "{line}"
        );
        // Nothing more until the recovery's reopen is made.
        assert_eq!(hold.due(&unstarted, 10 * RECOVER_US), None);
        assert!(
            hold.takes(&unstarted, 10 * RECOVER_US),
            "closed: nothing to hold"
        );
        let reopened = 2 * RECOVER_US;
        hold.reopened(reopened);
        assert_eq!(hold.due(&unstarted, reopened + RECOVER_US - 1), None);
        let Some(Recovery::Exit(line)) = hold.due(&unstarted, reopened + RECOVER_US) else {
            panic!("step 2 ends capture");
        };
        assert!(line.contains("recovery step 2"), "{line}");
        assert_eq!(
            UNSTARTED_ERROR,
            "capture frame preceded certified running reference clock"
        );
    }

    /// A recovery whose reopen brings the clock goes on, and the next
    /// reopen starts again at step 1.
    #[test]
    fn a_recovery_that_brings_the_clock_goes_on() {
        let mut hold = ReopenHold::new(true);
        hold.reopened(0);
        assert!(matches!(
            hold.due(&started(2, None), RECOVER_US),
            Some(Recovery::Reopen(_))
        ));
        hold.reopened(RECOVER_US + 10_000);
        assert_eq!(hold.due(&started(3, Some(1)), 3 * RECOVER_US), None);
        assert!(hold.takes(&started(3, Some(1)), 3 * RECOVER_US));
        hold.reopened(4 * RECOVER_US);
        assert!(matches!(
            hold.due(&started(4, None), 5 * RECOVER_US),
            Some(Recovery::Reopen(_))
        ));
    }

    /// Off, nothing is ever recovered.
    #[test]
    fn off_nothing_is_recovered() {
        let mut hold = ReopenHold::new(false);
        hold.reopened(0);
        assert_eq!(hold.due(&started(2, None), 100 * RECOVER_US), None);
    }

    /// A reopen with the clock running sends at once.
    #[test]
    fn a_reopen_with_the_clock_running_waits_for_nothing() {
        let mut hold = ReopenHold::new(true);
        hold.reopened(0);
        assert!(hold.takes(&started(1, Some(1)), 0));
        assert_eq!(hold.line(), None);
    }

    /// Off, a reopen holds nothing back: today's behaviour.
    #[test]
    fn off_a_reopen_holds_nothing_back() {
        let mut hold = ReopenHold::new(false);
        hold.reopened(0);
        assert!(hold.takes(&started(2, None), 0));
        assert!(hold.takes(&started(0, None), WAIT_US));
        assert_eq!(hold.line(), None);
    }

    /// Mid-run, an unstarted frame is a gap's drop, said once per reopen,
    /// however many come: there is no count that ends the voice.
    #[test]
    fn mid_run_unstarted_frames_are_dropped_and_said_once_per_reopen() {
        let mut frames = UnstartedFrames::new(true);
        frames.passed();
        let Unstarted::Drop(Some(line)) = frames.failed(2) else {
            panic!("the first of epoch 2 is said");
        };
        assert!(line.contains("microphone epoch 2"), "{line}");
        for _ in 0..100 {
            assert_eq!(frames.failed(2), Unstarted::Drop(None));
        }
        frames.passed();
        assert!(matches!(frames.failed(3), Unstarted::Drop(Some(_))));
    }

    /// Off, the exit is kept.
    #[test]
    fn off_the_exit_is_kept() {
        let mut frames = UnstartedFrames::new(false);
        frames.passed();
        assert_eq!(frames.failed(2), Unstarted::Exit);
    }

    /// Before the first certified start, the boot barrier's exit is
    /// unchanged.
    #[test]
    fn at_boot_the_exit_is_unchanged() {
        let mut frames = UnstartedFrames::new(true);
        assert_eq!(frames.failed(1), Unstarted::Exit);
    }
}
