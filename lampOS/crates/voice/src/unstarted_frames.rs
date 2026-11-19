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

/// The reason of the `CaptureGap` capture sends when its reopen hold
/// closes the microphone for a clock that has not come.
pub const CLOCK_NOT_STARTED: &str = "ClockNotStarted";

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

/// The switch on for every capture worker started in this process, in
/// place of the environment: the capture worker's own tests over a scripted
/// microphone (`tests/it/capture_hold_worker.rs`), which run it on a thread
/// and may not set the environment. Never set outside them.
#[doc(hidden)]
pub static ON_FOR_TESTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The switch as this process's environment sets it.
pub fn from_environment() -> bool {
    ON_FOR_TESTS.load(std::sync::atomic::Ordering::Relaxed)
        || switched_on(std::env::var(ENVIRONMENT).ok().as_deref())
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
    /// The last reopen's wait, for its line: from the reopen to the clock's
    /// start, in ms.
    waited_ms: Option<u64>,
    /// The last reopen, until a clock started after it: a re-prime soon
    /// after it is held as the reopen is.
    last_reopen_at: Option<u64>,
}

impl ReopenHold {
    pub fn new(on: bool) -> Self {
        Self {
            on,
            ..Self::default()
        }
    }

    /// The microphone reopened after a gap, at `at_us`. A reopen during a
    /// wait starts step 1's clock again; once the recovery's reopen has
    /// been made, a further reopen leaves step 2's clock where it is, so
    /// the hold ends within [`RECOVER_US`] of the recovery's reopen
    /// however many stalls come in it. `recovering` is cleared only by a
    /// started clock.
    pub fn reopened(&mut self, at_us: u64) {
        if !self.on {
            return;
        }
        self.last_reopen_at = Some(at_us);
        if !self.recovering || self.waiting_since.is_none() {
            self.waiting_since = Some(at_us);
            self.overran = false;
        }
    }

    /// The speaker re-primed at `at_us`. Within [`RECOVER_US`] of a reopen
    /// whose clock has not yet started after it, the re-prime is the
    /// reopen's, and its blocks are held as the reopen's are, its bounds
    /// counted from the reopen. Otherwise nothing: a re-prime at a
    /// cancelled answer keeps the two blocks before the start notice.
    pub fn primed(&mut self, at_us: u64) {
        if self.waiting_since.is_none()
            && let Some(reopen) = self.last_reopen_at
            && at_us.saturating_sub(reopen) < RECOVER_US
        {
            self.waiting_since = Some(reopen);
            self.overran = false;
        }
    }

    /// Whether blocks are being held back for the clock.
    pub fn waiting(&self) -> bool {
        self.waiting_since.is_some()
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
        if reference.playback_epoch != 0
            && let Some(clock) = reference.clock_started_at_us
        {
            self.waiting_since = None;
            self.recovering = false;
            self.waited_ms = Some(clock.saturating_sub(since) / 1_000);
            if self.last_reopen_at.is_some_and(|reopen| clock >= reopen) {
                self.last_reopen_at = None;
            }
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
        let waited_ms = self.waited_ms.take()?;
        let (held, past) = (
            std::mem::take(&mut self.held),
            std::mem::take(&mut self.past_wait),
        );
        Some(format!(
            "lamp-voice: capture: the speaker's clock started {waited_ms} ms after the reopen; {} blocks read before it were dropped, {past} of them past the {} ms wait ({ENVIRONMENT}=on)",
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
        assert!(
            line.contains("started 25 ms after the reopen; 3 blocks "),
            "{line}"
        );
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
        assert!(
            end.contains("started 500 ms after the reopen; 3 blocks "),
            "{end}"
        );
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
        hold.primed(1);
        assert!(!hold.waiting());
        assert_eq!(hold.due(&started(2, None), 100 * RECOVER_US), None);
    }

    /// A reopen with the clock running sends at once; its line says the
    /// clock waited for nothing, so every reopen's wait is journalled.
    #[test]
    fn a_reopen_with_the_clock_running_waits_for_nothing() {
        let mut hold = ReopenHold::new(true);
        hold.reopened(5_000);
        assert!(hold.takes(&started(1, Some(1)), 5_000));
        let line = hold.line().unwrap();
        assert!(
            line.contains("started 0 ms after the reopen; 0 blocks"),
            "{line}"
        );
        assert_eq!(hold.line(), None);
    }

    /// Twelve's P3b on #8011: a reopen during the wait starts step 1 again;
    /// after the recovery's reopen a further one leaves step 2's clock, so
    /// the total stays bounded; only a started clock clears it.
    #[test]
    fn a_reopen_in_the_wait_restarts_step_1_but_not_step_2() {
        let mut hold = ReopenHold::new(true);
        let unstarted = started(2, None);
        hold.reopened(0);
        hold.reopened(500_000);
        assert_eq!(
            hold.due(&unstarted, RECOVER_US),
            None,
            "step 1 from the new reopen"
        );
        assert!(matches!(
            hold.due(&unstarted, 500_000 + RECOVER_US),
            Some(Recovery::Reopen(_))
        ));
        let recovery = 2 * RECOVER_US;
        hold.reopened(recovery);
        hold.reopened(recovery + 400_000);
        assert!(
            matches!(
                hold.due(&unstarted, recovery + RECOVER_US),
                Some(Recovery::Exit(_))
            ),
            "step 2 counts from the recovery's reopen"
        );
    }

    /// A re-prime right after a reopen whose first block went with the old
    /// clock is held as the reopen is; one long after it, or with no
    /// reopen, holds nothing.
    #[test]
    fn a_reprime_soon_after_a_reopen_is_held() {
        let mut hold = ReopenHold::new(true);
        hold.primed(10);
        assert!(!hold.waiting(), "no reopen");
        hold.reopened(1_000);
        assert!(
            hold.takes(&started(1, Some(1)), 1_000),
            "the old clock runs"
        );
        hold.primed(30_000);
        assert!(hold.waiting());
        assert!(!hold.takes(&started(2, None), 40_000));
        assert!(hold.takes(&started(2, Some(300_000)), 300_000));
        assert!(
            hold.line()
                .unwrap()
                .contains("started 299 ms after the reopen; 1 blocks")
        );
        // That clock started after the reopen: a later re-prime is not it.
        hold.primed(320_000);
        assert!(!hold.waiting());
        let mut late = ReopenHold::new(true);
        late.reopened(0);
        assert!(late.takes(&started(1, Some(1)), 0));
        late.primed(RECOVER_US);
        assert!(!late.waiting());
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
