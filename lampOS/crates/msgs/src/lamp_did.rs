//! Every outward action the lamp takes, told with its reason (`lamp/did`,
//! #7757): so "why did you do that?" and "why did you do that earlier?" can be
//! answered by looking it up. One message is one action. It carries who
//! decided, what it was, a short fact in plain words as the reason, when, and
//! how it went. It carries NO word a person said, ever: a reason is what the
//! lamp measured or decided ("sat 47 min, no drink seen", "its name was
//! heard"), as numbers and named states. It stays on the lamp: the voice keeps
//! the last hour in memory and nothing else reads it.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};

/// What the lamp did outwardly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DidWhat {
    Spoke,
    Moved,
    Lit,
    StartedTask,
}

/// How it went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DidOutcome {
    Done,
    Refused,
    Cut,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LampDid {
    /// The part, and the skill or rule that decided: "lamp-skills:sip-of-water".
    /// A skill's name is the one name here, as `skills/events` already carries
    /// it (a skill a person made may be named anything).
    pub who: String,
    pub what: DidWhat,
    /// A short fact in plain words. Never a word a person said.
    pub why: String,
    /// When, on the capture clock, in microseconds (never wall time).
    pub at_us: u64,
    pub outcome: DidOutcome,
}

impl LampDid {
    pub const TYPE: &str = "lamp_msgs/LampDid";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "lamp/did";
    pub const MAX_WHO: usize = 96;
    pub const MAX_WHY: usize = 120;

    /// Each is over when it has been sent; the voice keeps the last hour.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        let plain = |text: &str, most: usize| {
            !text.trim().is_empty()
                && text.chars().count() <= most
                && !text.chars().any(|c| c.is_control())
        };
        if !plain(&self.who, Self::MAX_WHO) {
            return Err(Invalid("a did names who decided, on one short line".into()));
        }
        if !plain(&self.why, Self::MAX_WHY) {
            return Err(Invalid(
                "a did gives its reason in at most 120 characters on one line".into(),
            ));
        }
        if self.at_us == 0 {
            return Err(Invalid("a did needs the time it happened".into()));
        }
        Ok(())
    }
}

pub fn lamp_did_topics() -> Vec<(&'static str, Declaration)> {
    vec![(LampDid::TOPIC, LampDid::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn did() -> LampDid {
        LampDid {
            who: "lamp-skills:sip-of-water".into(),
            what: DidWhat::Spoke,
            why: "sat 47 min, no drink seen".into(),
            at_us: 5_000_000,
            outcome: DidOutcome::Done,
        }
    }

    #[test]
    fn a_did_is_who_what_why_when_and_how_and_nothing_else() {
        let heard = did();
        heard.check().unwrap();
        let json = serde_json::to_string(&heard).unwrap();
        assert_eq!(
            json,
            r#"{"who":"lamp-skills:sip-of-water","what":"spoke","why":"sat 47 min, no drink seen","at_us":5000000,"outcome":"done"}"#
        );
        assert_eq!(serde_json::from_str::<LampDid>(&json).unwrap(), heard);
        let extra = json.replace("}", r#","words":"hello"}"#);
        assert!(
            serde_json::from_str::<LampDid>(&extra).is_err(),
            "no field for words"
        );
    }

    #[test]
    fn a_did_without_a_short_plain_reason_or_a_time_is_refused() {
        for bad in [
            LampDid {
                why: "".into(),
                ..did()
            },
            LampDid {
                why: "x".repeat(121),
                ..did()
            },
            LampDid {
                why: "two\nlines".into(),
                ..did()
            },
            LampDid {
                who: " ".into(),
                ..did()
            },
            LampDid {
                who: "w".repeat(97),
                ..did()
            },
            LampDid { at_us: 0, ..did() },
        ] {
            assert!(bad.check().is_err(), "{bad:?}");
        }
        assert!(
            LampDid {
                why: "x".repeat(120),
                ..did()
            }
            .check()
            .is_ok()
        );
    }

    #[test]
    fn it_is_an_event_on_lamp_did() {
        assert_eq!(LampDid::TOPIC, "lamp/did");
        assert_eq!(LampDid::declaration().kind, Kind::Event);
    }
}
