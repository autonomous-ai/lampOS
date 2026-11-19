//! The board's heat (#7879, `board/heat`, `lamp_msgs::BoardHeat`, said by
//! lamp-health) and the pace it sets for the work nobody waits for here:
//! activity's bursts and posture's frames.
//!
//! - **Read only with `LAMP_HEAT_BACKOFF=on`** ([`SWITCH`], read once at
//!   start), from the topic's kept value at most once a second, as
//!   lamp-body reads `room/presence`: never waiting longer than a file read,
//!   and with no subscriber name. Off: ok, always, and the pace is today's.
//! - **A value past its life** (`BoardHeat::LASTS`), from the future, past
//!   its own life on the bus or not valid is ok: full speed.
//! - **The pace** ([`Pace::at`]): at warm the next burst is spaced
//!   [`crate::activity::EVERY_US`]×2, at hot ×4; posture's frame every
//!   30 s at ok and warm, one a minute at hot (never paused).
//! - **Unchanged at every level:** the frames' spacing within a burst
//!   ([`crate::activity::SPACING_US`]), a burst under way (it finishes),
//!   the opt-in read every 2 s and every gate closing (a stop, a forget,
//!   the privacy, sleep), so no yes or stop is ever missed.
use lamp_bus::{Bus, Message};
use lamp_msgs::{BoardHeat, HeatLevel};
use std::time::{Duration, Instant};

/// The switch, shared with lamp-eyes and lamp-body: off unless exactly `on`.
pub const SWITCH: &str = "LAMP_HEAT_BACKOFF";
pub const TOPIC: &str = BoardHeat::TOPIC;
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// Whether the switch's value turns it on: only `on`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(SWITCH).ok().as_deref())
}

/// The level `message` says, read at `now_us` (this boot's monotonic
/// clock): ok unless it is fresh and valid.
pub fn level_in(message: &Message, now_us: u64) -> HeatLevel {
    if message.expired
        || message.header.at_us > now_us
        || now_us - message.header.at_us > BoardHeat::LASTS.as_micros() as u64
    {
        return HeatLevel::Ok;
    }
    message
        .parse::<BoardHeat>()
        .ok()
        .filter(|said| said.check().is_ok())
        .map_or(HeatLevel::Ok, |said| said.level)
}

/// How often the bursts and posture's frames go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pace {
    /// From one burst's start to the next's.
    pub burst_every_us: u64,
    /// Between two posture frames.
    pub posture_every_us: u64,
}

impl Pace {
    /// Today's pace: a burst a minute, a posture frame every 30 s.
    pub const FULL: Pace = Pace {
        burst_every_us: crate::activity::EVERY_US,
        posture_every_us: crate::pose::EVERY_US,
    };

    /// The pace at `level`; with the switch off, [`Pace::FULL`] always.
    pub fn at(level: HeatLevel, on: bool) -> Pace {
        if !on {
            return Self::FULL;
        }
        match level {
            HeatLevel::Ok => Self::FULL,
            HeatLevel::Warm => Pace {
                burst_every_us: crate::activity::EVERY_US * 2,
                posture_every_us: crate::pose::EVERY_US,
            },
            HeatLevel::Hot => Pace {
                burst_every_us: crate::activity::EVERY_US * 4,
                posture_every_us: crate::pose::EVERY_US * 2,
            },
        }
    }

    /// This pace while a held sitting is open (`hold.rs`): posture keeps
    /// today's 30 s, since a hold ends with no answer after
    /// [`crate::hold::FIRST_ANSWER_US`] and counts two far answers in a row
    /// to end on someone else (Heron's HOLD on #7927); bursts keep this
    /// pace. With no hold open, this pace as it is.
    pub fn holding(self, held: bool) -> Pace {
        if held {
            Pace {
                posture_every_us: Self::FULL.posture_every_us,
                ..self
            }
        } else {
            self
        }
    }

    /// The journal's line for a change to this pace at `level`, without
    /// the `lamp-seen: ` prefix.
    pub fn line(self, level: HeatLevel) -> String {
        let bursts = format!("bursts every {} s", self.burst_every_us / 1_000_000);
        if self.posture_every_us == Self::FULL.posture_every_us {
            format!("board {}: {bursts}", level.as_str())
        } else {
            format!("board {}: {bursts}, posture one a minute", level.as_str())
        }
    }
}

/// This part's end of `board/heat`.
pub struct HeatIn {
    bus: Bus,
    looked: Option<Instant>,
    level: HeatLevel,
}

impl HeatIn {
    pub fn on(bus: Bus) -> Self {
        Self {
            bus,
            looked: None,
            level: HeatLevel::Ok,
        }
    }

    /// The board's heat, as last read. Never waits longer than a file read.
    pub fn level(&mut self) -> HeatLevel {
        let now = Instant::now();
        if self.looked.is_none_or(|at| now - at >= LOOK_EVERY) {
            self.looked = Some(now);
            let now_us = lamp_bus::monotonic_us();
            self.level = self
                .bus
                .latest(TOPIC)
                .ok()
                .flatten()
                .map_or(HeatLevel::Ok, |message| level_in(&message, now_us));
        }
        self.level
    }
}

#[cfg(test)]
mod tests;
