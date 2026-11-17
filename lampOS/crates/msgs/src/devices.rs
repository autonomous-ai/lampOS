//! The owner's desk and Hydrate bottles, reached through the cloud (#7470):
//! `devices/ask`, `devices/answer` and `devices/event`. The voice puts a
//! question or a command on `devices/ask`; `lamp-cloud` sends it up on the
//! lamp's own cloud link (`devices.list.get`, `devices.state.get`,
//! `devices.command`), and puts the cloud's reply on `devices/answer`, each
//! with the `req_id` it was asked by. A device's own news (`hydrate.sip`,
//! `desk.height`) comes down as `devices/event`.
//!
//! The lamp holds no token and no address of any device: the cloud checks
//! that the lamp's owner owns the device, and says `refused` when not. These
//! types carry the closed set the lamp may say (a height, a stop), never a free word for the cloud.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// The lowest and highest height a desk is asked to go to, in centimetres
/// (the cloud answers `invalid_value` outside them).
pub const DESK_CM: std::ops::RangeInclusive<u16> = 60..=132;
/// The most bytes in an id (`req_id`, `device_id`).
pub const DEVICE_ID_BYTES: usize = 64;
/// The most devices one list carries.
pub const MOST_DEVICES: usize = 32;
/// The most bytes of an event's `data`, as JSON.
pub const EVENT_DATA_BYTES: usize = 1024;

/// What the lamp asks about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "what", rename_all = "snake_case")]
pub enum DeviceAsk {
    /// The owner's desks and Hydrates this lamp may use.
    List,
    /// One device's reading.
    State { device_id: String },
    /// One command to a desk.
    Command {
        device_id: String,
        command: DeviceCommand,
    },
}

/// What a desk is told: a height, or to stop. (The cloud's
/// `memory_position` is not here: nothing on the lamp asks for it yet.)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum DeviceCommand {
    GotoHeight {
        cm: u16,
    },
    /// Accepted at any time, with no value: sent ahead of everything else.
    Stop,
}

impl DeviceCommand {
    /// The cloud's word for the command.
    pub fn name(&self) -> &'static str {
        match self {
            Self::GotoHeight { .. } => "goto_height",
            Self::Stop => "stop",
        }
    }

    fn check(&self) -> Result<(), Invalid> {
        match self {
            Self::GotoHeight { cm } if !DESK_CM.contains(cm) => Err(Invalid(format!(
                "a desk goes to {}..={} cm",
                DESK_CM.start(),
                DESK_CM.end()
            ))),
            _ => Ok(()),
        }
    }
}

/// One question or command, by the id its answer comes back under.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicesAsk {
    pub req_id: String,
    #[serde(flatten)]
    pub ask: DeviceAsk,
}

impl DevicesAsk {
    pub const TYPE: &str = "lamp_msgs/DevicesAsk";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "devices/ask";
    /// An ask not taken off the bus this soon is not sent: a desk is never
    /// moved by a request that waited.
    pub const LASTS: Duration = Duration::from_secs(10);

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).lasting(Self::LASTS)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        check_id("req_id", &self.req_id)?;
        match &self.ask {
            DeviceAsk::List => Ok(()),
            DeviceAsk::State { device_id } => check_id("device_id", device_id),
            DeviceAsk::Command { device_id, command } => {
                check_id("device_id", device_id)?;
                command.check()
            }
        }
    }
}

/// How the cloud answered a command or a read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    /// A command: handed to the desk, not that it moved. A read: answered.
    Ok,
    /// Not the owner's device.
    Refused,
    /// Anything else the cloud said, or a word the lamp does not know.
    Error,
}

/// Why the lamp has no answer from the cloud.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoAnswer {
    /// The cloud did not answer in time.
    Timeout,
    /// The lamp is not connected: nothing was sent.
    Away,
    /// Not a device the cloud's list gave: nothing was sent.
    Unlisted,
    /// Too many asks waiting or too many in a minute: nothing was sent.
    Busy,
    /// A command that was still waiting when a stop came: never sent.
    Cancelled,
}

/// One device the owner has, as the cloud lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRow {
    pub device_id: String,
    pub device_type: DeviceType,
    /// The owner's name for it, one line.
    pub name: String,
    /// What it takes (`goto_height`, `memory_position`, `stop`), as the
    /// cloud lists them.
    pub commands: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    Desk,
    Hydrate,
}

/// A device's reading: only the fields its kind has, `None` for one the
/// cloud has not got (a desk that has not reported a height says `null`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
    /// A desk's height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_cm: Option<u16>,
    /// A Hydrate's day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub today_ml: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sips_today: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_ml: Option<u32>,
}

/// What came back, or why nothing did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "snake_case")]
pub enum DeviceAnswer {
    List {
        devices: Vec<DeviceRow>,
    },
    State {
        device_id: String,
        status: DeviceStatus,
        state: DeviceState,
    },
    Command {
        device_id: String,
        status: DeviceStatus,
        /// The cloud's own word (`not_allowed`, `invalid_value`, …), for the
        /// journal: never said aloud.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// No answer: nothing the cloud said.
    None {
        why: NoAnswer,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicesAnswer {
    pub req_id: String,
    #[serde(flatten)]
    pub answer: DeviceAnswer,
}

impl DevicesAnswer {
    pub const TYPE: &str = "lamp_msgs/DevicesAnswer";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "devices/answer";
    /// An answer nobody took this long after is for a question long given up.
    pub const LASTS: Duration = Duration::from_secs(30);

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).lasting(Self::LASTS)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        check_id("req_id", &self.req_id)?;
        match &self.answer {
            DeviceAnswer::List { devices } => {
                if devices.len() > MOST_DEVICES {
                    return Err(Invalid(format!(
                        "a list has at most {MOST_DEVICES} devices"
                    )));
                }
                for device in devices {
                    check_id("device_id", &device.device_id)?;
                    check_line(&device.name)?;
                }
                Ok(())
            }
            DeviceAnswer::State { device_id, .. } | DeviceAnswer::Command { device_id, .. } => {
                check_id("device_id", device_id)
            }
            DeviceAnswer::None { .. } => Ok(()),
        }
    }
}

/// A device's own news: `hydrate.sip` `{ml, today_ml, sips_today}`,
/// `desk.height` `{height_cm}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevicesEvent {
    pub device_id: String,
    pub device_type: DeviceType,
    /// `hydrate.sip`, `desk.height`: letters, digits, `.`, `_`.
    pub event: String,
    /// When the cloud says it happened, as it said it.
    pub at: String,
    pub data: Value,
}

impl DevicesEvent {
    pub const TYPE: &str = "lamp_msgs/DevicesEvent";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "devices/event";
    /// News older than this is not news.
    pub const LASTS: Duration = Duration::from_secs(60);

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).lasting(Self::LASTS)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        check_id("device_id", &self.device_id)?;
        if self.event.is_empty()
            || self.event.len() > 32
            || !self
                .event
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_'))
        {
            return Err(Invalid("event is 1..32 letters, digits, '.' or '_'".into()));
        }
        check_line(&self.at)?;
        if self.data.to_string().len() > EVENT_DATA_BYTES {
            return Err(Invalid(format!("data is at most {EVENT_DATA_BYTES} bytes")));
        }
        Ok(())
    }
}

fn check_id(what: &str, id: &str) -> Result<(), Invalid> {
    if id.is_empty()
        || id.len() > DEVICE_ID_BYTES
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
    {
        return Err(Invalid(format!(
            "{what} is 1..{DEVICE_ID_BYTES} letters, digits, '-', '_', '.' or ':'"
        )));
    }
    Ok(())
}

fn check_line(text: &str) -> Result<(), Invalid> {
    if text.len() > 128 || text.chars().any(char::is_control) {
        return Err(Invalid(
            "a name or time is one line of at most 128 bytes".into(),
        ));
    }
    Ok(())
}

/// The devices topics (#7470).
pub fn devices_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (DevicesAsk::TOPIC, DevicesAsk::declaration()),
        (DevicesAnswer::TOPIC, DevicesAnswer::declaration()),
        (DevicesEvent::TOPIC, DevicesEvent::declaration()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(command: DeviceCommand) -> DevicesAsk {
        DevicesAsk {
            req_id: "r1".into(),
            ask: DeviceAsk::Command {
                device_id: "desk-1".into(),
                command,
            },
        }
    }

    #[test]
    fn a_desk_goes_only_where_it_can_() {
        for cm in [60, 110, 132] {
            assert_eq!(command(DeviceCommand::GotoHeight { cm }).check(), Ok(()));
        }
        for cm in [0, 59, 133, 1100] {
            assert!(
                command(DeviceCommand::GotoHeight { cm }).check().is_err(),
                "{cm}"
            );
        }
        assert_eq!(command(DeviceCommand::Stop).check(), Ok(()));
    }

    #[test]
    fn an_ask_has_an_id_and_an_id_has_no_odd_bytes() {
        let list = |req_id: &str| DevicesAsk {
            req_id: req_id.into(),
            ask: DeviceAsk::List,
        };
        assert_eq!(list("r-1").check(), Ok(()));
        for bad in ["", "a b", "two\nlines", &"a".repeat(DEVICE_ID_BYTES + 1)] {
            assert!(list(bad).check().is_err(), "{bad:?}");
        }
        let state = DevicesAsk {
            req_id: "r".into(),
            ask: DeviceAsk::State {
                device_id: "x y".into(),
            },
        };
        assert!(state.check().is_err());
    }

    #[test]
    fn the_wire_form_is_flat_and_stable() {
        let json = serde_json::to_string(&command(DeviceCommand::GotoHeight { cm: 110 })).unwrap();
        assert_eq!(
            json,
            r#"{"req_id":"r1","what":"command","device_id":"desk-1","command":{"command":"goto_height","cm":110}}"#
        );
        let back: DevicesAsk = serde_json::from_str(&json).unwrap();
        assert_eq!(back, command(DeviceCommand::GotoHeight { cm: 110 }));
        assert_eq!(
            serde_json::to_string(&DevicesAsk {
                req_id: "r".into(),
                ask: DeviceAsk::List
            })
            .unwrap(),
            r#"{"req_id":"r","what":"list"}"#
        );
    }

    #[test]
    fn an_answer_says_what_came_back_or_why_nothing_did() {
        let state = DevicesAnswer {
            req_id: "r".into(),
            answer: DeviceAnswer::State {
                device_id: "desk-1".into(),
                status: DeviceStatus::Ok,
                state: DeviceState {
                    online: Some(false),
                    ..DeviceState::default()
                },
            },
        };
        assert_eq!(state.check(), Ok(()));
        let json = serde_json::to_string(&state).unwrap();
        assert!(
            !json.contains("height_cm"),
            "no height is left out, not made up: {json}"
        );
        assert_eq!(serde_json::from_str::<DevicesAnswer>(&json).unwrap(), state);
        for why in [
            NoAnswer::Timeout,
            NoAnswer::Away,
            NoAnswer::Unlisted,
            NoAnswer::Busy,
        ] {
            let none = DevicesAnswer {
                req_id: "r".into(),
                answer: DeviceAnswer::None { why },
            };
            let json = serde_json::to_string(&none).unwrap();
            assert_eq!(serde_json::from_str::<DevicesAnswer>(&json).unwrap(), none);
        }
        let many = DevicesAnswer {
            req_id: "r".into(),
            answer: DeviceAnswer::List {
                devices: (0..=MOST_DEVICES)
                    .map(|n| DeviceRow {
                        device_id: format!("d{n}"),
                        device_type: DeviceType::Hydrate,
                        name: "Bottle".into(),
                        commands: vec![],
                    })
                    .collect(),
            },
        };
        assert!(many.check().is_err());
    }

    #[test]
    fn an_event_is_small_and_named_plainly() {
        let event = |name: &str, data: Value| DevicesEvent {
            device_id: "h-1".into(),
            device_type: DeviceType::Hydrate,
            event: name.into(),
            at: "2026-11-18T03:00:00Z".into(),
            data,
        };
        let sip = serde_json::json!({"ml": 30, "today_ml": 250, "sips_today": 6});
        assert_eq!(event("hydrate.sip", sip.clone()).check(), Ok(()));
        assert!(event("", sip.clone()).check().is_err());
        assert!(event("a b", sip).check().is_err());
        let big = serde_json::json!({"x": "a".repeat(EVENT_DATA_BYTES)});
        assert!(event("hydrate.sip", big).check().is_err());
    }

    #[test]
    fn every_topic_is_declared_and_asks_are_not_kept_long() {
        assert_eq!(devices_topics().len(), 3);
        assert_eq!(
            DevicesAsk::declaration().lifespan(),
            Some(DevicesAsk::LASTS)
        );
        assert!(DevicesAsk::LASTS <= Duration::from_secs(10));
    }
}
