//! Health and crash reports to the company, opt-in at setup (#1683,
//! `docs/cloud.md`, "Health and crash reports"). `lamp-cloud` sends them; on the bus there is only
//! the way to turn them off and whether they are on. Off unless
//! `LAMP_REPORTS=on`.
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A part on the lamp asks for reports to stop (`reports/request`): the
/// voice, when the person says "stop sending reports". Anyone on the lamp
/// can write to the bus, so `lamp-cloud` takes only `share: false` from it:
/// the bus can turn reports off, never on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportsRequest {
    pub share: bool,
}

impl ReportsRequest {
    pub const TYPE: &str = "lamp_msgs/ReportsRequest";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "reports/request";

    /// An ask, kept by nobody.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }
}

/// Whether reports are being sent now (`reports/state`), as `lamp-cloud`
/// holds it: the switch on, the person's yes, and how many wait to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportsState {
    /// True while the person's yes stands and the switch is on.
    pub share: bool,
    /// Reports waiting to be sent.
    pub queued: u32,
}

impl ReportsState {
    pub const TYPE: &str = "lamp_msgs/ReportsState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "reports/state";

    /// The newest replaces the last. When it changes and every five
    /// seconds, and it means nothing after fifteen: a `lamp-cloud` that has
    /// stopped sends nothing.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(5))
            .lasting(Duration::from_secs(15))
    }
}

/// The reports' two topics.
pub fn reports_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (ReportsRequest::TOPIC, ReportsRequest::declaration()),
        (ReportsState::TOPIC, ReportsState::declaration()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reports_topics_are_declared_and_found() {
        assert_eq!(
            serde_json::to_string(&ReportsRequest { share: false }).unwrap(),
            r#"{"share":false}"#
        );
        assert_eq!(
            serde_json::to_string(&ReportsState {
                share: true,
                queued: 2
            })
            .unwrap(),
            r#"{"share":true,"queued":2}"#
        );
        for (topic, declaration) in reports_topics() {
            assert_eq!(crate::declaration_of(topic), Some(declaration));
        }
    }
}
