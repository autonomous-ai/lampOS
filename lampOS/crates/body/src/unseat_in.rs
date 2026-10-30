//! What lets a lamp the strain guard seated stand again (#4501,
//! `docs/body.md`, "The strain guard"): a person's deliberate hold of a head
//! pad or a button for a second, read as the first stand's yes is
//! (`first_stand::Touches`, from `controls/state`), or a health check
//! (`lamp/selftest`) started since it was seated. Looked at ten times a
//! second, and only while it is seated.

use crate::first_stand::{HOLD_US, Touches, touched};
use lamp_bus::Bus;
use lamp_msgs::{ControlsState, SelfTest};

const LOOK_EVERY_US: u64 = 100_000;

/// What has been seen since the lamp was seated.
#[derive(Debug, Default)]
pub struct Unseat {
    touches: Option<Touches>,
    seated_unix_s: Option<f64>,
}

impl Unseat {
    /// One look. `seated`: whether the body says it is; `touched`: the
    /// controls' touch (`first_stand::touched`); `checked`: when the newest
    /// health check started, in Unix seconds; `now_us` monotonic. What lets
    /// it stand, in words, once; nothing otherwise.
    pub fn look(
        &mut self,
        seated: bool,
        touched: Option<bool>,
        checked: Option<f64>,
        now_us: u64,
        now_unix_s: f64,
    ) -> Option<&'static str> {
        if !seated {
            *self = Self::default();
            return None;
        }
        let since = *self.seated_unix_s.get_or_insert(now_unix_s);
        let why = if checked.is_some_and(|started| started >= since) {
            "a health check started"
        } else if self
            .touches
            .get_or_insert_with(|| Touches::new(HOLD_US))
            .observe(touched, now_us)
        {
            "a hand held its head a second"
        } else {
            return None;
        };
        *self = Self::default();
        Some(why)
    }
}

/// The body's end of it on the bus.
pub struct UnseatIn {
    bus: Bus,
    unseat: Unseat,
    looked_us: Option<u64>,
}

impl UnseatIn {
    pub fn on(bus: Bus) -> Self {
        Self {
            bus,
            unseat: Unseat::default(),
            looked_us: None,
        }
    }

    /// What lets the lamp stand now, if `seated`. Never waits longer than
    /// two file reads.
    pub fn asked(&mut self, seated: bool, now_us: u64, now_unix_s: f64) -> Option<&'static str> {
        if !seated {
            return self.unseat.look(false, None, None, now_us, now_unix_s);
        }
        if self
            .looked_us
            .is_some_and(|at| now_us.saturating_sub(at) < LOOK_EVERY_US)
        {
            return None;
        }
        self.looked_us = Some(now_us);
        let touch = self
            .bus
            .latest(ControlsState::TOPIC)
            .ok()
            .flatten()
            .filter(|message| !message.expired)
            .and_then(|message| message.parse::<ControlsState>().ok())
            .and_then(|state| touched(&state));
        let checked = self
            .bus
            .latest(SelfTest::TOPIC)
            .ok()
            .flatten()
            .and_then(|message| message.parse::<SelfTest>().ok())
            .map(|run| run.started);
        self.unseat.look(true, touch, checked, now_us, now_unix_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deliberate_hold_or_a_later_health_check_lets_it_stand_once() {
        let mut unseat = Unseat::default();
        assert_eq!(unseat.look(false, Some(true), Some(2e9), 0, 1_000.0), None);
        // A hand already on when it was seated is no hold: seen off first.
        assert_eq!(unseat.look(true, Some(true), None, 0, 1_000.0), None);
        assert_eq!(
            unseat.look(true, Some(true), None, 2_000_000, 1_002.0),
            None
        );
        assert_eq!(
            unseat.look(true, Some(false), None, 2_100_000, 1_002.1),
            None
        );
        assert_eq!(
            unseat.look(true, Some(true), None, 2_200_000, 1_002.2),
            None
        );
        // An unknown state breaks a hold, as for the first stand's yes.
        assert_eq!(unseat.look(true, None, None, 2_600_000, 1_002.6), None);
        assert_eq!(
            unseat.look(true, Some(true), None, 2_700_000, 1_002.7),
            None
        );
        assert_eq!(
            unseat.look(true, Some(true), None, 3_700_000, 1_003.7),
            Some("a hand held its head a second")
        );
        // Once: the same hand still on asks nothing more.
        assert_eq!(
            unseat.look(true, Some(true), None, 5_000_000, 1_005.0),
            None
        );

        // A health check that started before it was seated is not one for it.
        let mut checked = Unseat::default();
        assert_eq!(checked.look(true, None, Some(999.0), 0, 1_000.0), None);
        assert_eq!(
            checked.look(true, None, Some(1_001.0), 100_000, 1_000.1),
            Some("a health check started")
        );
    }
}
