//! Whether the lamp can reach the internet now (`lamp/online`, #830,
//! `docs/offline.md`). Worked out from the voice's link to its speech
//! service, as it reports it on `diagnostics`; with `lamp-health
//! --reach-probe on`, while that link is down, from a probe of the internet
//! itself (`from: "reach"`), so it can be online with the link down, and
//! absent while the probe has not yet answered. With `--reach-idle on` too,
//! the probe asks with the link up, and the internet not answering is
//! offline from `reach` with the link still ready (#1844).
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Online {
    /// True while a link that needs the internet works, false while it has
    /// been down past its grace, absent when no such link is reporting. With
    /// the probe, while the link is down: whether the internet answered, and
    /// absent until the first probe of the outage ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
    /// Who said so: the part whose link it is, e.g. `voice`, or `reach` for
    /// the probe. Empty when none did.
    pub from: String,
    /// How long it has been down, in whole seconds, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down_for_s: Option<u64>,
}

impl Online {
    pub const TYPE: &str = "lamp_msgs/Online";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "lamp/online";

    /// The newest replaces the last. When it changes, and once a second.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest).every(Duration::from_secs(1))
    }

    /// A part said so when there is an answer, and only a lamp that is
    /// offline has been down for a while.
    pub fn check(&self) -> Result<(), Invalid> {
        if self.online.is_some() == self.from.is_empty() {
            return Err(Invalid(
                "online needs the part it is from, and only then".into(),
            ));
        }
        if self.down_for_s.is_some() && self.online != Some(false) {
            return Err(Invalid(
                "down_for_s is only for a lamp that is offline".into(),
            ));
        }
        Ok(())
    }
}

/// The reachability topic, separate from the original seventeen.
pub fn online_topics() -> Vec<(&'static str, Declaration)> {
    vec![(Online::TOPIC, Online::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_says_who_said_so_and_how_long_it_has_been_down() {
        let up = Online {
            online: Some(true),
            from: "voice".into(),
            down_for_s: None,
        };
        assert!(up.check().is_ok());
        assert_eq!(
            serde_json::to_string(&up).unwrap(),
            r#"{"online":true,"from":"voice"}"#
        );
        let down = Online {
            online: Some(false),
            from: "voice".into(),
            down_for_s: Some(12),
        };
        assert!(down.check().is_ok());
        let unknown = Online {
            online: None,
            from: String::new(),
            down_for_s: None,
        };
        assert!(unknown.check().is_ok());
        assert_eq!(serde_json::to_string(&unknown).unwrap(), r#"{"from":""}"#);
        for wrong in [
            Online {
                from: String::new(),
                ..up.clone()
            },
            Online {
                online: None,
                ..up.clone()
            },
            Online {
                down_for_s: Some(1),
                ..up
            },
        ] {
            assert!(wrong.check().is_err());
        }
        assert_eq!(declaration_of(Online::TOPIC), Some(Online::declaration()));
    }

    use crate::declaration_of;
}
