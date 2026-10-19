//! What the person at the lamp is doing, as far as their face shows it
//! (`room/activity`, #1678, `docs/presence.md`), and when they stood up and
//! left (`room/stood_up`). Read from the face's box and marks against the
//! person's own sitting, never against a fixed line: states and counts only,
//! no picture, no box and nobody's identity.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What the face at the desk shows, against this sitting's baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Nobody at the lamp (`room/presence`'s `someone: false`).
    Away,
    /// Someone at the desk, as they sat down, or before the baseline is in.
    AtDesk,
    /// The face bigger than it sat: closer to the lamp.
    LeaningIn,
    /// The face smaller than it sat, still turned to the lamp.
    LeaningBack,
    /// The face lower and the nose low between the eyes and mouth: reading,
    /// writing or a phone held low, not told apart.
    HeadDown,
    /// The face well lower than it sat, not looking down: the nearest
    /// honest name for slumped.
    SittingLow,
}

/// What the person at the lamp is doing, said by the eyes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomActivity {
    pub state: Activity,
    /// How long `state` has held, in whole seconds.
    pub for_s: u32,
    /// How long this sitting has lasted, from its first face, a gap under
    /// [`RoomActivity::SITTING_GAP_S`] forgiven; 0 while away.
    pub sitting_s: u32,
    /// Whether the sitting's baseline is in, so that the leaning and lower
    /// states can be said at all.
    pub baseline: bool,
}

impl RoomActivity {
    pub const TYPE: &str = "lamp_msgs/RoomActivity";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "room/activity";
    pub const EVERY: Duration = Duration::from_secs(1);
    pub const LASTS: Duration = Duration::from_secs(3);
    /// Looking away or leaning out of view for less than this does not end
    /// a sitting: Stretch break's `gap`.
    pub const SITTING_GAP_S: u32 = 120;

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Self::EVERY)
            .lasting(Self::LASTS)
    }

    /// Away has no sitting, and a leaning or lower state needs the baseline.
    pub fn check(&self) -> Result<(), Invalid> {
        if self.state == Activity::Away && self.sitting_s != 0 {
            return Err(Invalid(format!(
                "away, and sitting_s is {}: 0 while away",
                self.sitting_s
            )));
        }
        if !self.baseline && !matches!(self.state, Activity::Away | Activity::AtDesk) {
            return Err(Invalid(format!(
                "{:?} without the sitting's baseline: only away or at_desk",
                self.state
            )));
        }
        Ok(())
    }
}

/// The person at the desk stood up and left: the face rose and grew
/// smaller, then was lost. An event, for a reader that watches them go
/// (#1669); nothing is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoodUp {
    /// How long the sitting they left had lasted, in whole seconds.
    pub sitting_s: u32,
}

impl StoodUp {
    pub const TYPE: &str = "lamp_msgs/StoodUp";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "room/stood_up";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }
}

/// The activity topics, apart from the first topics.
pub fn activity_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (RoomActivity::TOPIC, RoomActivity::declaration()),
        (StoodUp::TOPIC, StoodUp::declaration()),
    ]
}
