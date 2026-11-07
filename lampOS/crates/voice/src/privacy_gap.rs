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
//!
//! With `LAMP_PRIVACY_RIDES_STALL=on` ([`STALL_SWITCH`], #5611) a lapse
//! noticed in a pass held up by the voice's own stall (more than
//! [`HELD_UP_PASS_US`] since the pass before, or since its own top) is
//! ridden the same way, with or without `LAMP_PRIVACY_GAP_RIDE`, for at most
//! [`STALL_RIDE_US`] counted from where the stall began; the reopen's 60 ms
//! steady time (`REOPEN_STABLE_US`) counts inside it, so a stall under about
//! 1.44 s is ridden and a longer one still ends. On lamp-06c1 on 6 Nov a 120 ms stall of
//! the whole voice during an SD log flush lapsed every lease, and the voice
//! ended and came back muted by the restart guard. Off, nothing changes.

/// The switch: only `on` rides a gap.
pub const SWITCH: &str = "LAMP_PRIVACY_GAP_RIDE";
/// The longest gap ridden. A Controls that stalls for longer is Controls'
/// publishing to fix, not a longer wait here (the first builder).
pub const RIDE_US: u64 = 1_000_000;
/// The switch for riding the voice's own stall (#5611): only `on`. It also
/// reaches the privacy worker and the second worker (`worker_command`).
pub const STALL_SWITCH: &str = "LAMP_PRIVACY_RIDES_STALL";
/// The longest stall ridden, from the pass before it: the bound the privacy
/// worker already rides a Denied lapse for, and the speaker's and the
/// second worker's (`DENIED_LAPSE_MOST_US`, `STALLED_CONTROL_US`). Past it
/// everything ends as before the switch.
pub const STALL_RIDE_US: u64 = 1_500_000;
/// A pass this long after the one before it, or running this long since its
/// own top, was held up by a stall of the voice. Passes take 2-5 ms; the
/// switch's reading is published at most every 20 ms, so it is up to 20 ms
/// old when a stall starts and 30 ms more makes it stale: the bound is 20
/// ms, not the 50 ms freshness (the first builder on #5633). A stall of
/// Controls alone, with the voice's passes on time, is still not ridden.
pub const HELD_UP_PASS_US: u64 = 20_000;

/// Whether the switch's value turns it on: only `on`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether the switch is on in this process's environment.
pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(SWITCH).ok().as_deref())
}

/// Whether [`STALL_SWITCH`] is on in this process's environment: only `on`.
pub fn rides_stall_from_environment() -> bool {
    enabled(std::env::var(STALL_SWITCH).ok().as_deref())
}

/// A conversation's gap, if one is being ridden.
#[derive(Debug)]
pub struct GapRide {
    on: bool,
    since: Option<u64>,
    /// [`STALL_SWITCH`]: a lapse seen in a held-up pass is ridden too.
    stall: bool,
    /// When the last pass began, and, if this one followed a stall, when
    /// the pass before it began ([`Self::pass`]).
    last_pass_us: Option<u64>,
    held_up_since: Option<u64>,
    /// The bound of the gap being ridden: [`RIDE_US`], or [`STALL_RIDE_US`]
    /// for one that began after a stall.
    most_us: u64,
}

impl Default for GapRide {
    fn default() -> Self {
        Self::new(false)
    }
}

impl GapRide {
    /// `on`: the switch, on the route that rides (the Controls route).
    pub fn new(on: bool) -> Self {
        Self {
            on,
            since: None,
            stall: false,
            last_pass_us: None,
            held_up_since: None,
            most_us: RIDE_US,
        }
    }

    /// With [`STALL_SWITCH`] on (#5611): a lapse seen in a pass that
    /// followed the voice's own stall is ridden, from the pass before it.
    pub fn riding_stalls(mut self, stall: bool) -> Self {
        self.stall = stall;
        self
    }

    /// A pass of the loop begins at `now_us`. With the stall switch on, one
    /// more than [`HELD_UP_PASS_US`] after the last followed a stall.
    pub fn pass(&mut self, now_us: u64) {
        self.held_up_since = self
            .last_pass_us
            .filter(|last| self.stall && now_us.saturating_sub(*last) > HELD_UP_PASS_US);
        self.last_pass_us = Some(now_us);
    }

    /// Since when the voice was held up, if it was, at `now_us`: before
    /// this pass began, from the pass before it, or inside this pass, from
    /// its top, as a page fault in running code holds it (#5633). Only
    /// within [`STALL_RIDE_US`] of that start.
    fn stalled_since(&self, now_us: u64) -> Option<u64> {
        let inside = self
            .last_pass_us
            .filter(|top| self.stall && now_us.saturating_sub(*top) > HELD_UP_PASS_US);
        self.held_up_since
            .or(inside)
            .filter(|since| now_us.saturating_sub(*since) < STALL_RIDE_US)
    }

    /// Whether the voice was held up, before this pass or inside it, still
    /// within [`STALL_RIDE_US`] at `now_us`.
    pub fn held_up(&self, now_us: u64) -> bool {
        self.stalled_since(now_us).is_some()
    }

    /// The bound of the gap ridden now, or of one about to begin.
    pub fn most_us(&self) -> u64 {
        self.most_us
    }

    /// Whether a lapse at `now_us` is ridden rather than the end: one about
    /// to begin while the microphone is `permitted`, or one ridden for less
    /// than its bound ([`RIDE_US`], or [`STALL_RIDE_US`] after a stall).
    pub fn rides(&self, permitted: bool, now_us: u64) -> bool {
        match self.since {
            Some(since) => now_us.saturating_sub(since) < self.most_us,
            None => (self.on || self.held_up(now_us)) && permitted,
        }
    }

    /// The gap begins at `now_us`, or, after a stall, at the pass before
    /// it. False when it can't be ridden ([`Self::rides`]).
    pub fn begin(&mut self, permitted: bool, now_us: u64) -> bool {
        if self.since.is_some() || !self.rides(permitted, now_us) {
            return false;
        }
        let stalled = self.stalled_since(now_us);
        self.since = Some(stalled.map_or(now_us, |since| since.min(now_us)));
        self.most_us = if stalled.is_some() {
            STALL_RIDE_US
        } else {
            RIDE_US
        };
        true
    }

    /// Whether the gap ridden now began after a stall of the voice.
    pub fn after_a_stall(&self) -> bool {
        self.since.is_some() && self.most_us == STALL_RIDE_US
    }

    /// A worker noticed the same gap earlier. Its acquisition-based start
    /// may shorten the remaining budget, never renew it.
    pub fn noticed_since(&mut self, since_us: u64) {
        if let Some(since) = &mut self.since {
            *since = (*since).min(since_us);
        }
    }

    /// A fresh reading said open at `now_us`: the gap is over, and how long
    /// it lasted. None when no gap was ridden or its deadline has passed;
    /// an expired gap stays active so the caller must end it, never reopen.
    pub fn reopened(&mut self, now_us: u64) -> Option<u64> {
        if self
            .lasted(now_us)
            .is_some_and(|lasted| lasted >= self.most_us)
        {
            return None;
        }
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

    #[test]
    fn a_workers_earlier_expiry_shortens_but_never_renews_the_ride() {
        let mut ride = GapRide::new(true);
        assert!(ride.begin(true, 100_000));
        ride.noticed_since(90_000);
        ride.noticed_since(110_000);
        assert_eq!(ride.lasted(120_000), Some(30_000));
        assert!(ride.rides(true, 90_000 + RIDE_US - 1));
        assert!(!ride.rides(true, 90_000 + RIDE_US));
    }

    #[test]
    fn recovery_at_the_deadline_does_not_use_an_earlier_successful_check() {
        for late in [0, 1, 10_000] {
            let mut ride = GapRide::new(true);
            assert!(ride.begin(true, 100));
            assert!(ride.rides(true, 100 + RIDE_US - 1));
            assert_eq!(ride.reopened(100 + RIDE_US + late), None);
            assert_eq!(ride.lasted(100 + RIDE_US + late), Some(RIDE_US + late));
        }
    }

    /// #5611: off, a held-up pass rides nothing, as before. On, a lapse seen
    /// in a pass more than [`HELD_UP_PASS_US`] (20 ms) after the last is
    /// ridden from that last pass for at most 1.5 s, and only once the
    /// microphone was permitted; one seen in an ordinary pass is not, and
    /// one past the bound ends.
    #[test]
    fn a_lapse_after_a_held_up_pass_is_ridden_from_before_the_stall_for_1_5_s() {
        let mut off = GapRide::new(false);
        off.pass(1_000_000);
        off.pass(1_300_000);
        assert!(!off.held_up(1_300_000));
        assert!(!off.rides(true, 1_300_000));
        let mut ride = GapRide::new(false).riding_stalls(true);
        ride.pass(1_000_000);
        ride.pass(1_002_000);
        assert!(!ride.rides(true, 1_002_000), "no stall: not ridden");
        ride.pass(1_302_000);
        assert!(ride.held_up(1_302_000));
        assert!(!ride.rides(false, 1_302_000), "never permitted");
        assert!(ride.begin(true, 1_302_000));
        assert!(ride.after_a_stall());
        assert_eq!(ride.lasted(1_302_000), Some(300_000));
        assert!(ride.rides(true, 1_002_000 + STALL_RIDE_US - 1));
        assert!(!ride.rides(true, 1_002_000 + STALL_RIDE_US));
        assert_eq!(ride.reopened(1_400_000), Some(398_000));
        // A stall past the bound is not ridden at all.
        let mut hung = GapRide::new(false).riding_stalls(true);
        hung.pass(1_000_000);
        hung.pass(1_000_000 + 1_800_000);
        assert!(!hung.rides(true, 1_000_000 + 1_800_000));
        assert!(!hung.begin(true, 1_000_000 + 1_800_000));
    }

    /// The first builder's P2 on #5633: a stall inside a pass, after its
    /// top, as a page fault in running code is, is seen by the same pass.
    /// Ridden from that pass's top; a pass of ordinary length is not.
    #[test]
    fn a_stall_inside_a_pass_is_ridden_from_the_passs_top() {
        let mut ride = GapRide::new(false).riding_stalls(true);
        ride.pass(1_000_000);
        ride.pass(1_002_000);
        assert!(!ride.rides(true, 1_007_000), "an ordinary 5 ms pass");
        assert!(ride.held_up(1_122_000), "120 ms into the pass");
        assert!(ride.rides(true, 1_122_000));
        assert!(ride.begin(true, 1_122_000));
        assert!(ride.after_a_stall());
        assert_eq!(ride.lasted(1_122_000), Some(120_000));
        assert!(ride.rides(true, 1_002_000 + STALL_RIDE_US - 1));
        assert!(!ride.rides(true, 1_002_000 + STALL_RIDE_US));
        // Past the bound inside the pass: not ridden, as before.
        let mut hung = GapRide::new(false).riding_stalls(true);
        hung.pass(1_000_000);
        hung.pass(1_002_000);
        assert!(!hung.rides(true, 1_002_000 + 1_800_000));
        // Off: nothing, inside a pass or between.
        let mut off = GapRide::new(false);
        off.pass(1_000_000);
        off.pass(1_002_000);
        assert!(!off.rides(true, 1_122_000));
    }

    /// The reading is up to ~20 ms old when a stall starts, so 30-50 ms
    /// makes it stale: a 35 ms stall counts as held up, between passes and
    /// inside one, and the bound is 20 ms (#5633, the first builder).
    #[test]
    fn a_35_ms_stall_counts_as_held_up() {
        assert_eq!(HELD_UP_PASS_US, 20_000);
        let mut between = GapRide::new(false).riding_stalls(true);
        between.pass(1_000_000);
        between.pass(1_035_000);
        assert!(between.rides(true, 1_035_000), "between passes");
        let mut inside = GapRide::new(false).riding_stalls(true);
        inside.pass(1_000_000);
        inside.pass(1_002_000);
        assert!(inside.rides(true, 1_037_000), "inside a pass");
        let mut ordinary = GapRide::new(false).riding_stalls(true);
        ordinary.pass(1_000_000);
        ordinary.pass(1_005_000);
        assert!(!ordinary.rides(true, 1_019_000));
    }

    #[test]
    fn only_on_turns_the_stall_ride_on() {
        assert_eq!(STALL_SWITCH, "LAMP_PRIVACY_RIDES_STALL");
        assert!(enabled(Some("on")));
        for value in [None, Some("off"), Some("1"), Some("ON"), Some("on ")] {
            assert!(!enabled(value), "{value:?}");
        }
    }
}
