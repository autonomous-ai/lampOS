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
    /// The nose low between the eyes and mouth: reading, writing or a phone
    /// held low, not told apart. Leaning back and sitting low were tried at
    /// lamp-75c9 and could not be seen from its place: they come back only
    /// if a lamp in front of a person shows them.
    HeadDown,
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
    /// Whether the sitting's baseline is in, so that leaning in and head
    /// down can be said at all.
    pub baseline: bool,
    /// This sitting's yawns, as the eyes count them (`LAMP_EYES_YAWN`,
    /// #3709); 0 while away, or when they are not counted. Left out at 0,
    /// so an eyes that counts none says what it said before.
    #[serde(default, skip_serializing_if = "no_yawns")]
    pub yawns: u32,
}

fn no_yawns(yawns: &u32) -> bool {
    *yawns == 0
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

    /// Away has no sitting, and leaning in or head down needs the baseline.
    pub fn check(&self) -> Result<(), Invalid> {
        if self.state == Activity::Away && self.sitting_s != 0 {
            return Err(Invalid(format!(
                "away, and sitting_s is {}: 0 while away",
                self.sitting_s
            )));
        }
        if self.state == Activity::Away && self.yawns != 0 {
            return Err(Invalid(format!(
                "away, and yawns is {}: 0 while away",
                self.yawns
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
    /// The side of the camera's picture they left by, from where their face
    /// was last seen; none when it was near the middle (added 2026-10-20,
    /// optional, for watching them go). A turn toward it is the turn the
    /// gaze would make to a face seen there (`docs/body.md`, `toward`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
}

/// A side of the camera's picture, as it is filled: left is its left edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
}

impl StoodUp {
    pub const TYPE: &str = "lamp_msgs/StoodUp";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "room/stood_up";
    /// A departure means nothing this long after it: a reader that keeps the
    /// newest value (lamp-skills' triggers) lets it go, so the next
    /// departure starts anew.
    pub const LASTS: Duration = Duration::from_secs(5);

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).lasting(Self::LASTS)
    }
}

/// The activity topics, apart from the first topics.
pub fn activity_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (RoomActivity::TOPIC, RoomActivity::declaration()),
        (StoodUp::TOPIC, StoodUp::declaration()),
    ]
}
