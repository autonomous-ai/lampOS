//! How hot the board is for the work that can wait (`board/heat`, #7879,
//! `docs/health.md`, "The board's heat"): ok, warm or hot, with the zone it
//! was read from. Said by lamp-health from each reading of the board, so
//! that the parts whose work nobody waits for can ease off before the board
//! slows itself down.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How hot the board is, as lamp-health works it out with hysteresis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeatLevel {
    /// Full speed.
    Ok,
    /// Near the point where the board slows itself down: work that can
    /// wait eases off.
    Warm,
    /// At that point, or the board already slowing itself down: work that
    /// can wait eases off further.
    Hot,
}

impl HeatLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warm => "warm",
            Self::Hot => "hot",
        }
    }
}

/// The board's heat now. A reader that has none, or one that has run out,
/// or one that does not pass [`BoardHeat::check`], takes the board for ok:
/// full speed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardHeat {
    pub level: HeatLevel,
    /// The processor zone it was read from, the hotter of them, by its
    /// name in `board/state` (`cpub_thermal_zone`).
    pub zone: String,
    /// The zone's temperature, in degrees Celsius.
    pub c: f32,
    /// Where the zone's board slows itself down: its second passive trip
    /// point, or 90 °C where it names none.
    pub trip_c: f32,
}

impl BoardHeat {
    pub const TYPE: &str = "lamp_msgs/BoardHeat";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "board/heat";
    /// Said with each reading of the board.
    pub const EVERY: Duration = Duration::from_secs(5);
    /// A value means nothing this long after it was said, as `board/state`.
    pub const LASTS: Duration = Duration::from_secs(15);
    /// No zone's name is longer, as lamp-health reads them.
    pub const MOST_ZONE_BYTES: usize = 48;

    /// The newest replaces the one before, every five seconds, each good
    /// for fifteen.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Self::EVERY)
            .lasting(Self::LASTS)
    }

    /// A zone is named, and both temperatures are numbers, the trip point
    /// above freezing.
    pub fn check(&self) -> Result<(), Invalid> {
        if self.zone.is_empty() || self.zone.len() > Self::MOST_ZONE_BYTES {
            return Err(Invalid(format!(
                "the zone's name is {} bytes: from 1 to {}",
                self.zone.len(),
                Self::MOST_ZONE_BYTES
            )));
        }
        if !self.c.is_finite() {
            return Err(Invalid(format!("c is {}", self.c)));
        }
        if !self.trip_c.is_finite() || self.trip_c <= 0.0 {
            return Err(Invalid(format!("trip_c is {}: above 0", self.trip_c)));
        }
        Ok(())
    }
}

/// The board's heat, apart from the first topics.
pub fn heat_topics() -> Vec<(&'static str, Declaration)> {
    vec![(BoardHeat::TOPIC, BoardHeat::declaration())]
}
