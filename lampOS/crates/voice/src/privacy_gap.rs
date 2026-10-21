//! Riding out a short gap in the privacy readings inside a conversation
//! (#2602's finding 3, the first builder's (B) at 17:4x; lamp-75c9, 21 Oct
//! 16:33:06). With `LAMP_PRIVACY_GAP_RIDE=on`, on the Controls route, a
//! lapse of the switch's freshness after the microphone was allowed no
//! longer ends the conversation at once:
//!
//! - from that moment no capture frame is taken (`GapRide::takes_frame`),
//!   the turn in flight is cut and the speaker told at once, nothing new
//!   begins, so the lamp says nothing, the eyes are told no, and the
//!   capture's and the camera's authority aren't renewed: their leases lapse
//!   within 250 ms (about 100 ms while a turn is under way) and capture
//!   closes the microphone as on a stall (`Discontinuity::AuthorityExpired`).
//!   A camera given none for 250 ms ends itself, so a longer gap costs it
//!   for the rest of the conversation;
//! - a fresh reading that says open, steady for the gate's 60 ms as the
//!   first grant needs, within [`RIDE_US`] renews it: capture reopens in the
//!   same generation with a new epoch, no frame read before the reopen is
//!   taken, the retention heartbeat counts from the reopen, and the
//!   conversation goes on with its history;
//! - a fresh reading that says closed is the physical mute, as before, and
//!   no fresh reading within [`RIDE_US`] ends the conversation as before;
//!   either end is marked (`privacy_gap` `ended`, with why).
//!
//! Capture's own bound on a closed microphone (`REOPEN_US`, 1.5 s) is past
//! [`RIDE_US`], so a ride ends before capture would. Off, every lapse ends
//! the conversation as it always has. Nothing here reads a clock or the
//! switch: the coordinator gives it times and what it knows.

/// The switch: only `on` rides a gap.
pub const SWITCH: &str = "LAMP_PRIVACY_GAP_RIDE";
/// The longest gap ridden. A Controls that stalls for longer is Controls'
/// publishing to fix, not a longer wait here (the first builder).
pub const RIDE_US: u64 = 1_000_000;

/// Whether the switch's value turns it on: only `on`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether the switch is on in this process's environment.
pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(SWITCH).ok().as_deref())
}

/// A conversation's gap, if one is being ridden.
#[derive(Debug, Default)]
pub struct GapRide {
    on: bool,
    since: Option<u64>,
}

impl GapRide {
    /// `on`: the switch, on the route that rides (the Controls route).
    pub fn new(on: bool) -> Self {
        Self { on, since: None }
    }

    /// Whether a lapse at `now_us` is ridden rather than the end: one about
    /// to begin while the microphone is `permitted`, or one ridden for less
    /// than [`RIDE_US`].
    pub fn rides(&self, permitted: bool, now_us: u64) -> bool {
        match self.since {
            Some(since) => now_us.saturating_sub(since) < RIDE_US,
            None => self.on && permitted,
        }
    }

    /// The gap begins at `now_us`. False when it can't be ridden ([`Self::rides`]).
    pub fn begin(&mut self, permitted: bool, now_us: u64) -> bool {
        if self.since.is_some() || !self.rides(permitted, now_us) {
            return false;
        }
        self.since = Some(now_us);
        true
    }

    /// A fresh reading said open at `now_us`: the gap is over, and how long
    /// it lasted. None when no gap was ridden.
    pub fn reopened(&mut self, now_us: u64) -> Option<u64> {
        self.since.take().map(|since| now_us.saturating_sub(since))
    }

    /// How long the gap ridden has lasted at `now_us`, if one is.
    pub fn lasted(&self, now_us: u64) -> Option<u64> {
        self.since.map(|since| now_us.saturating_sub(since))
    }

    /// Whether a capture frame read at `read_us` may be taken at `now_us`,
    /// with frames taken again only from `from_us` (the last gap's reopen):
    /// none while a gap is ridden, including those capture still reads
    /// inside its lease, and none read before the reopen, so nothing of the
    /// gap or of the epoch it closed is ever heard (the first builder on
    /// #2602, 17:4x).
    pub fn takes_frame(&self, read_us: u64, from_us: u64, now_us: u64) -> bool {
        self.lasted(now_us).is_none() && read_us >= from_us
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_turns_it_on() {
        assert!(enabled(Some("on")));
        for value in [None, Some("off"), Some("1"), Some("ON"), Some("")] {
            assert!(!enabled(value), "{value:?}");
        }
    }

    /// Off, nothing is ridden. On, a lapse with the microphone permitted
    /// begins a gap, ridden for less than a second and never longer; none
    /// begins before the microphone was ever permitted, and the gap's end
    /// says how long it lasted.
    #[test]
    fn a_gap_is_ridden_for_under_a_second_and_only_once_permitted() {
        let mut off = GapRide::new(false);
        assert!(!off.rides(true, 10));
        assert!(!off.begin(true, 10));
        let mut ride = GapRide::new(true);
        assert!(!ride.rides(false, 10), "never permitted: not ridden");
        assert!(!ride.begin(false, 10));
        assert!(ride.begin(true, 1_000));
        assert!(!ride.begin(true, 1_001), "one at a time");
        // Permitted no more once begun (denied), and still ridden.
        assert!(ride.rides(false, 1_000 + RIDE_US - 1));
        assert!(!ride.rides(false, 1_000 + RIDE_US), "a second is the most");
        assert_eq!(ride.lasted(1_250), Some(250));
        assert_eq!(ride.reopened(1_300), Some(300));
        assert_eq!(ride.reopened(1_400), None);
        assert_eq!(ride.lasted(1_400), None);
        // A later lapse is a gap of its own.
        assert!(ride.begin(true, 5_000));
    }

    /// No frame is taken while a gap is ridden, whenever it was read; after
    /// the reopen, none read before it.
    #[test]
    fn no_frame_of_the_gap_or_before_its_reopen_is_taken() {
        let mut ride = GapRide::new(true);
        assert!(ride.takes_frame(990, 0, 1_000), "no gap: taken");
        assert!(ride.begin(true, 1_000));
        for read in [900, 1_000, 1_050, 1_200] {
            assert!(!ride.takes_frame(read, 0, 1_200), "in the gap: {read}");
        }
        let reopened = 1_300;
        assert_eq!(ride.reopened(reopened), Some(300));
        assert!(!ride.takes_frame(1_250, reopened, 1_310), "read in the gap");
        assert!(
            !ride.takes_frame(1_299, reopened, 1_310),
            "read before the reopen"
        );
        assert!(ride.takes_frame(1_300, reopened, 1_310));
        assert!(ride.takes_frame(1_305, reopened, 1_310));
    }
}
