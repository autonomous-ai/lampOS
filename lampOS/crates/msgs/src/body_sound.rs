//! One body sound of the person near the lamp, heard (`ears/body`, #7685): a
//! cough, a throat cleared, a sneeze, a sniff, a wheeze. One message is one
//! sound, so a skill counts them ("four coughs in five minutes") with nothing
//! recorded. It carries no audio, no words and no one's name, and it stays on
//! the lamp: it is in no list the cloud, telemetry or the usage reports read,
//! and nothing keeps it (docs/ears.md, "Body sounds"). This type grants no
//! authority to react: the voice never answers a cough as if spoken to.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyClass {
    Cough,
    ThroatClear,
    Sneeze,
    Sniff,
    Wheeze,
}

impl BodyClass {
    pub const ALL: [BodyClass; 5] = [
        BodyClass::Cough,
        BodyClass::ThroatClear,
        BodyClass::Sneeze,
        BodyClass::Sniff,
        BodyClass::Wheeze,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BodyClass::Cough => "cough",
            BodyClass::ThroatClear => "throat_clear",
            BodyClass::Sneeze => "sneeze",
            BodyClass::Sniff => "sniff",
            BodyClass::Wheeze => "wheeze",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodySound {
    pub class: BodyClass,
    /// The model's score for it when it crossed its bar, 0 to 1.
    pub score: f32,
    /// When the window that held it ended, on the capture clock, in
    /// microseconds (never wall time).
    pub at_us: u64,
}

impl BodySound {
    pub const TYPE: &str = "lamp_msgs/BodySound";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "ears/body";

    /// Each is over when it has been sent; none is promised, none is kept.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if !self.score.is_finite() || !(0.0..=1.0).contains(&self.score) {
            return Err(Invalid(
                "body sound score must be finite and within [0,1]".into(),
            ));
        }
        if self.at_us == 0 {
            return Err(Invalid("body sound needs the time it was heard".into()));
        }
        Ok(())
    }
}

/// Body sounds are separate from the original topics and from the scores.
pub fn body_sound_topics() -> Vec<(&'static str, Declaration)> {
    vec![(BodySound::TOPIC, BodySound::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_sound_is_a_class_a_score_and_a_time_and_nothing_else() {
        let heard = BodySound {
            class: BodyClass::ThroatClear,
            score: 0.71,
            at_us: 5_000_000,
        };
        let text = serde_json::to_string(&heard).unwrap();
        assert_eq!(
            text,
            r#"{"class":"throat_clear","score":0.71,"at_us":5000000}"#
        );
        assert_eq!(serde_json::from_str::<BodySound>(&text).unwrap(), heard);
        assert_eq!(heard.check(), Ok(()));
        assert!(
            serde_json::from_str::<BodySound>(
                r#"{"class":"cough","score":0.7,"at_us":1,"audio":"x"}"#
            )
            .is_err(),
            "no field a sound could carry audio or words in"
        );
        for odd in [f32::NAN, 1.5, -0.1] {
            assert!(
                BodySound {
                    score: odd,
                    ..heard
                }
                .check()
                .is_err()
            );
        }
        assert!(BodySound { at_us: 0, ..heard }.check().is_err());
    }

    #[test]
    fn it_is_an_event_on_its_own_topic() {
        let declared = BodySound::declaration();
        assert_eq!(declared.kind, Kind::Event);
        assert_eq!(BodySound::TOPIC, "ears/body");
        assert!(!BodyClass::ALL.is_empty());
        assert_eq!(
            BodyClass::ALL.map(BodyClass::name),
            ["cough", "throat_clear", "sneeze", "sniff", "wheeze"]
        );
    }
}
