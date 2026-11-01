//! Whether this lamp is signed out (`account/state`, `LAMP_SIGNED_OUT`,
//! `docs/cloud.md`, "Signed out"). `lamp-cloud` works it out from the ping's
//! answers and says it; the voice reads it to say the kept line and to stop
//! sending turns.
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What `lamp-cloud` knows of the lamp's login. A reader that finds it past
/// its lifespan, or none at all, goes on as before: `lamp-cloud` stopped, or
/// an older one, is never read as signed out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountState {
    pub signed_out: bool,
    /// How long this signed-out spell has lasted, in seconds; kept across a
    /// restart, so a restarted voice can tell a new spell from one already
    /// said. 0 while signed in.
    #[serde(default)]
    pub for_s: u64,
}

impl AccountState {
    pub const TYPE: &str = "lamp_msgs/AccountState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "account/state";

    /// The newest replaces the last. When it changes and every five seconds,
    /// and it means nothing after fifteen.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(5))
            .lasting(Duration::from_secs(15))
    }
}

/// The account's topic.
pub fn account_topics() -> Vec<(&'static str, Declaration)> {
    vec![(AccountState::TOPIC, AccountState::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_reads_without_its_later_field_and_is_declared_and_found() {
        assert_eq!(
            serde_json::from_str::<AccountState>(r#"{"signed_out":true}"#).unwrap(),
            AccountState {
                signed_out: true,
                for_s: 0
            }
        );
        assert!(crate::declaration_of(AccountState::TOPIC).is_some());
    }
}
