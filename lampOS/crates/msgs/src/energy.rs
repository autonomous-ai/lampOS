//! How lively the owner's lamp moves (`character/energy`, #6893,
//! `docs/chosen-voice.md`, "Energy"): one level kept with the character
//! words, said by `lamp-cloud` for the body. Level 1 is the lamp as it
//! always moved.
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The level kept now. None said, or none kept, is level 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Energy {
    /// 1 (calm, as today) to 5 (lively).
    pub level: u8,
    /// Whether the owner picked it; false when worked out from the words.
    pub chosen: bool,
}

impl Default for Energy {
    fn default() -> Self {
        Self {
            level: 1,
            chosen: false,
        }
    }
}

impl Energy {
    pub const TYPE: &str = "lamp_msgs/Energy";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "character/energy";

    /// The newest replaces the last, every five seconds. A setting, not a
    /// reading: it never goes stale, so a reader keeps the last it heard.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest).every(Duration::from_secs(5))
    }
}

/// The energy's topic.
pub fn energy_topics() -> Vec<(&'static str, Declaration)> {
    vec![(Energy::TOPIC, Energy::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_is_declared_and_found() {
        assert_eq!(
            serde_json::to_string(&Energy::default()).unwrap(),
            r#"{"level":1,"chosen":false}"#
        );
        assert!(crate::declaration_of(Energy::TOPIC).is_some());
    }
}
