//! What lets a lamp the strain guard seated stand again (#4501,
//! `docs/body.md`, "The strain guard"): a person's deliberate hold of a head
//! pad or the primary button for a second (`first_stand::Touches`, from
//! `controls/touch_state`: the product's head pads are only there, never in
//! `controls/state`'s `head_active`, which needs the pins' `legacy_head`),
//! or a health check (`lamp/selftest`) started since it was seated. Looked
//! at ten times a second, and only while it is seated.

use crate::first_stand::{HOLD_US, Touches};
use lamp_bus::Bus;
use lamp_msgs::{SelfTest, TouchSource, TouchState};

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
            .latest(TouchState::TOPIC)
            .ok()
            .flatten()
            .filter(|message| !message.expired)
            .and_then(|message| message.parse::<TouchState>().ok())
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

/// Whether the head pads or the primary button are touched, from a valid,
/// enabled `controls/touch_state`: one of them touched, or a head hand held
/// through a pulse's short released gap (`held_us`, #711), is a touch; else
/// one known untouched is none; None when neither is known (the reset button
/// and the base strip are not asked).
pub fn touched(state: &TouchState) -> Option<bool> {
    if state.check().is_err() || !state.enabled {
        return None;
    }
    let mut known = None;
    for source in &state.sources {
        if matches!(source.source, TouchSource::Head | TouchSource::Primary) {
            match source.active {
                Some(true) => return Some(true),
                Some(false) if source.held_us.is_some() => return Some(true),
                Some(false) => known = Some(false),
                None => {}
            }
        }
    }
    known
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `controls/touch_state` as lamp-controls says it with the product's
    /// pins: the primary button and the head, each known or not.
    fn state(button: Option<bool>, head: Option<bool>) -> TouchState {
        let source = |kind: &str, active: Option<bool>| {
            let names: Vec<&str> = if active == Some(true) && kind == "head" {
                vec!["head"]
            } else {
                vec![]
            };
            serde_json::json!({
                "source": kind, "name": kind, "reader_session_id": "reader",
                "ready": active.is_some(), "fault": active.is_none().then_some("missing"),
                "observed_us": active.map(|_| 1000), "armed": active.is_some(),
                "active": active, "contact_id": null, "began_us": null, "held_us": null,
                "active_mask": active.map(u16::from), "active_names": names,
                "position": null, "toward_pad": null, "distance": 0.0, "speed": 0.0
            })
        };
        let state: TouchState = serde_json::from_value(serde_json::json!({
            "stamp": {"clock_id": "controls", "at_us": 1000, "sequence": 1},
            "session_id": "s1", "enabled": true,
            "sources": [source("primary", button), source("head", head)],
            "latest": [], "event_sequence": 0, "rejected_inputs": 0
        }))
        .unwrap();
        assert_eq!(state.check(), Ok(()), "a state lamp-controls could send");
        state
    }

    /// #4519 on lamp-52e6: the product's head pads are on
    /// `controls/touch_state` only, so a head hand must be read there.
    #[test]
    fn a_head_or_the_button_from_the_touch_state_is_a_touch() {
        assert_eq!(touched(&state(Some(false), Some(true))), Some(true));
        assert_eq!(touched(&state(Some(true), Some(false))), Some(true));
        assert_eq!(touched(&state(Some(false), Some(false))), Some(false));
        // A lost button does not hide the head, either way.
        assert_eq!(touched(&state(None, Some(true))), Some(true));
        assert_eq!(touched(&state(None, Some(false))), Some(false));
        assert_eq!(touched(&state(None, None)), None);
        let mut off = state(Some(false), Some(true));
        off.enabled = false;
        assert_eq!(touched(&off), None);
    }

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
