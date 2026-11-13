//! The energy level the character keeps, read from `character/energy`
//! (#6893, `crate::energy`). Read only with `LAMP_BODY_ENERGY=on`, from the
//! topic's kept value at most once a second. A level outside 1 to 5 or a
//! message that does not read is not taken; a publisher gone keeps the last
//! level taken, not 1. Off, or nothing said yet: level 1, today.
//!
//! Only `level` is read from `{"level": 1..5, "chosen": bool}`; the typed
//! message (`lamp_msgs`) is added beside the publisher.

use lamp_bus::{Bus, Message};
use serde::Deserialize;
use std::time::{Duration, Instant};

/// The switch, the body's alone: off unless exactly `on`.
pub const SWITCH: &str = "LAMP_BODY_ENERGY";
pub const TOPIC: &str = "character/energy";
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// Whether the switch's value turns it on: only `on`.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

pub fn enabled_from_environment() -> bool {
    enabled(std::env::var(SWITCH).ok().as_deref())
}

#[derive(Deserialize)]
struct Said {
    level: u8,
}

/// The level `message` says, if it says one from 1 to 5.
pub fn level_in(message: &Message) -> Option<u8> {
    message
        .parse::<Said>()
        .ok()
        .map(|said| said.level)
        .filter(|level| (1..=5).contains(level))
}

/// The body's end of `character/energy`.
pub struct EnergyIn {
    bus: Bus,
    looked: Option<Instant>,
    level: u8,
}

impl EnergyIn {
    pub fn on(bus: Bus) -> Self {
        Self {
            bus,
            looked: None,
            level: 1,
        }
    }

    /// The level last said. Never waits longer than a file read.
    pub fn level(&mut self) -> u8 {
        let now = Instant::now();
        if self.looked.is_none_or(|at| now - at >= LOOK_EVERY) {
            self.looked = Some(now);
            if let Some(level) = self
                .bus
                .latest(TOPIC)
                .ok()
                .flatten()
                .as_ref()
                .and_then(level_in)
            {
                self.level = level;
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::Kind;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn root() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lamp-body-energy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn only_on_turns_it_on() {
        assert!(enabled(Some("on")));
        for value in [None, Some("off"), Some("1"), Some("ON"), Some("")] {
            assert!(!enabled(value), "{value:?}");
        }
    }

    /// Nothing said is 1; a level said is taken; one outside 1 to 5 or
    /// unreadable leaves the last one taken.
    #[test]
    fn a_said_level_is_taken_and_a_bad_one_keeps_the_last() {
        let root = root();
        let bus = Bus::new(&root).unwrap();
        let mut energy = EnergyIn::on(Bus::new(&root).unwrap());
        assert_eq!(energy.level(), 1);
        let mut out = bus.publisher(TOPIC, Kind::Latest, "cloud").unwrap();
        let mut say = |value: serde_json::Value| {
            out.publish_json(1, &value).unwrap();
            energy.looked = None;
            energy.level()
        };
        assert_eq!(say(serde_json::json!({"level": 4, "chosen": true})), 4);
        for bad in [
            serde_json::json!({"level": 0}),
            serde_json::json!({"level": 6}),
            serde_json::json!({"chosen": true}),
        ] {
            assert_eq!(say(bad), 4);
        }
        assert_eq!(say(serde_json::json!({"level": 1, "chosen": false})), 1);
        let _ = std::fs::remove_dir_all(root);
    }
}
