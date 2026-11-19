//! The board's heat for OpenClaw's install (#7879; docs/install.md, "Gives
//! way while it runs"): with `LAMP_HEAT_BACKOFF=on` in lamp-update's own
//! unit, the install watch ([`crate::hermes_schedule`]) pauses OpenClaw's
//! install in Harness's one-shot while lamp-health's `board/heat` reads
//! hot, as it does while someone talks (the first builder, 10:42 on
//! #7879: npm at a full core is what a hot lamp can't afford).
//!
//! - **Read only with the switch on** ([`SWITCH`], read once at the
//!   watch's start), from the topic's kept value at most once a second,
//!   as lamp-seen's `heat::HeatIn` does: never waiting longer than a file
//!   read, and with no subscriber name.
//! - **Not hot:** warm or ok, a value past its life (`BoardHeat::LASTS`),
//!   from the future, past its own life on the bus or not valid, and no
//!   bus. So a stale reading never holds an install.
//! - **Only OpenClaw's turn** ([`holds`]): Hermes's install in the same
//!   one-shot runs on.
use lamp_bus::{Bus, Message};
use lamp_msgs::{BoardHeat, HeatLevel};
use std::time::{Duration, Instant};

/// The switch, shared with lamp-seen, lamp-eyes and lamp-body: off unless
/// exactly `on`.
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

/// Whether the board's `level` holds the install now: only hot, only with
/// the switch `on`, and only on OpenClaw's turn (`openclaw_turn`).
pub fn holds(level: HeatLevel, on: bool, openclaw_turn: bool) -> bool {
    on && openclaw_turn && level == HeatLevel::Hot
}

/// lamp-update's end of `board/heat`.
pub struct HeatIn {
    bus: Option<Bus>,
    looked: Option<Instant>,
    level: HeatLevel,
}

impl HeatIn {
    /// On `bus`; none (it could not be opened) is never hot.
    pub fn on(bus: Option<Bus>) -> Self {
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
                .as_ref()
                .and_then(|bus| bus.latest(TOPIC).ok().flatten())
                .map_or(HeatLevel::Ok, |message| level_in(&message, now_us));
        }
        self.level
    }
}

#[cfg(test)]
mod tests;
