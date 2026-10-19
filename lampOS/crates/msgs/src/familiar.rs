//! How familiar the remembered person at the lamp is (`humans/familiar`,
//! #1670, `docs/eyes.md`): only a level, from how often they come by, for
//! someone who asked the lamp to remember their face. No name, no face, no
//! picture and no time of anyone's visit.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How familiar: `new` until they come by often, `regular` about once a
/// week kept up, `close` about three days a week.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FamiliarLevel {
    New,
    Regular,
    Close,
}

/// The remembered person the eyes recognise at the lamp now, as a level, or
/// `None`: nobody near, someone not remembered, or not recognised. Said
/// only with `lamp-eyes --visits`, while the eyes look. A reader treats no
/// fresh value as not known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanFamiliar {
    pub level: Option<FamiliarLevel>,
}

impl HumanFamiliar {
    pub const TYPE: &str = "lamp_msgs/HumanFamiliar";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "humans/familiar";
    /// Said at each recognition, every two seconds while the eyes look.
    pub const EVERY: Duration = Duration::from_secs(2);
    /// A value means nothing this long after it was said.
    pub const LASTS: Duration = Duration::from_secs(5);

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Self::EVERY)
            .lasting(Self::LASTS)
    }

    /// Nothing to check: a level or none.
    pub fn check(&self) -> Result<(), Invalid> {
        Ok(())
    }
}

/// The familiarity topic, apart from the first topics.
pub fn familiar_topics() -> Vec<(&'static str, Declaration)> {
    vec![(HumanFamiliar::TOPIC, HumanFamiliar::declaration())]
}
