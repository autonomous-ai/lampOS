//! What the person at the desk was last seen doing, and how they sat, from
//! pictures sent to be labelled (#6543, cloud perception): `humans/doing`,
//! `humans/doing_seen` and `humans/posture`. Labels and numbers only: no
//! picture, no box, no raw label and nobody's identity. Only this sitting:
//! the timers begin again on `room/stood_up`, so no history is kept.
//!
//! Their producer is `lamp-seen` (bus name `lamp_seen`), coming with
//! #6543's phases 2 and 3; until then nothing publishes them.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// What the person was last seen doing, as a coarse kind: never the raw
/// label ("eating burger").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Doing {
    Drinking,
    Eating,
    /// A stretch (V1's `break` bucket: stretching an arm or a leg).
    Break,
    /// Nothing seen yet this sitting.
    #[serde(rename = "none")]
    Nothing,
}

/// The sitting's timers: how long the person has not been seen drinking,
/// eating or taking a break, and the last thing seen.
///
/// Every timer is always present. When nothing has been seen this sitting
/// (or frames do not go out: the opt-in off, the server busy, the slide
/// shut), it counts from the desk arrival, so it is the sitting's length:
/// a skill's `[[while]] above` on it still holds, and the skill runs on the
/// clock as it did without these topics. A missing field would make
/// `above` never hold (#6543, the optional-field blocker). Read a timer as
/// "not seen doing it for at least this long".
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HumanDoing {
    /// Seconds since a drink was seen, or since the desk arrival.
    pub drank_s_ago: u32,
    /// Seconds since eating was seen, or since the desk arrival.
    pub ate_s_ago: u32,
    /// Seconds since a stretch was seen, or since the desk arrival.
    pub break_s_ago: u32,
    /// The last thing seen this sitting; `none` before anything.
    pub last: Doing,
    /// How sure the read behind `last` was, 0 to 1; 0 with `none`.
    pub confidence: f32,
}

impl HumanDoing {
    pub const TYPE: &str = "lamp_msgs/HumanDoing";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "humans/doing";
    /// Said every 30 s while someone sits at the desk.
    pub const EVERY: Duration = Duration::from_secs(30);
    /// A value means nothing this long after it was said.
    pub const LASTS: Duration = Duration::from_secs(90);

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Self::EVERY)
            .lasting(Self::LASTS)
    }

    /// The confidence is from 0 to 1, and 0 when nothing was seen.
    pub fn check(&self) -> Result<(), Invalid> {
        share("confidence", self.confidence)?;
        if self.last == Doing::Nothing && self.confidence != 0.0 {
            return Err(Invalid(format!(
                "last is none, and confidence is {}: 0 with none",
                self.confidence
            )));
        }
        Ok(())
    }
}

/// One activity newly seen: an event, once each time, for Policy and logs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DoingSeen {
    /// What was seen; never `none`.
    pub what: Doing,
    /// How sure the read was, 0 to 1.
    pub confidence: f32,
}

impl DoingSeen {
    pub const TYPE: &str = "lamp_msgs/DoingSeen";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "humans/doing_seen";
    /// A sighting means nothing this long after it: a reader that keeps the
    /// newest value (lamp-skills) lets it go, as `room/stood_up`.
    pub const LASTS: Duration = Duration::from_secs(5);

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event).lasting(Self::LASTS)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.what == Doing::Nothing {
            return Err(Invalid("what is none: a sighting is of something".into()));
        }
        share("confidence", self.confidence)
    }
}

/// The body region that most often scored badly in the window. `wrist` is
/// left out: the server's RULA fixes it at 1, so it never counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostureRegion {
    Neck,
    Trunk,
    UpperArm,
    LowerArm,
    /// No region stood out, or no bad sample yet.
    #[serde(rename = "none")]
    Nothing,
}

/// How the person has sat over the current window: its length so far, the
/// share of its samples read as bad, and the region that was most often
/// bad. `window_s` is always in a value said, 0 at the window's start, so a
/// skill's `[[while]] above` on it is never blocked by a missing field.
///
/// Not said while the window holds no sample (frames not going out: the
/// opt-in off, the slide shut, the server busy, no body found, a privacy or
/// sleep wipe), so the topic runs out and a skill holding on it with
/// `unknown = "holds_stale"` runs as it did. A `bad_share` of 0 is said only
/// over samples, never for none.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HumanPosture {
    /// How long the window has run, in whole seconds, at most an hour.
    pub window_s: u32,
    /// The share of the window's samples read as bad, 0 to 1.
    pub bad_share: f32,
    pub region: PostureRegion,
}

impl HumanPosture {
    pub const TYPE: &str = "lamp_msgs/HumanPosture";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "humans/posture";
    /// Said every 30 s while someone sits at the desk and the window holds
    /// a sample.
    pub const EVERY: Duration = Duration::from_secs(30);
    /// A value means nothing this long after it was said.
    pub const LASTS: Duration = Duration::from_secs(90);
    /// The longest window said.
    pub const LONGEST_WINDOW_S: u32 = 3600;

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Self::EVERY)
            .lasting(Self::LASTS)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.window_s > Self::LONGEST_WINDOW_S {
            return Err(Invalid(format!(
                "window_s is {}: at most {}",
                self.window_s,
                Self::LONGEST_WINDOW_S
            )));
        }
        share("bad_share", self.bad_share)
    }
}

fn share(name: &str, value: f32) -> Result<(), Invalid> {
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(Invalid(format!("{name} is from 0 to 1")))
    }
}

/// The cloud-perception topics, apart from the first topics.
pub fn doing_topics() -> Vec<(&'static str, Declaration)> {
    vec![
        (HumanDoing::TOPIC, HumanDoing::declaration()),
        (DoingSeen::TOPIC, DoingSeen::declaration()),
        (HumanPosture::TOPIC, HumanPosture::declaration()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    /// `text` with one field taken out is not a `T`.
    fn needs<T: serde::de::DeserializeOwned>(text: &str, field: &str) {
        let mut without: Value = serde_json::from_str(text).unwrap();
        assert!(without.as_object_mut().unwrap().remove(field).is_some());
        let error = serde_json::from_value::<T>(without).err().expect("refused");
        assert!(error.to_string().contains(field), "{error}");
    }

    /// Written as `text`, read back as itself, and the same with a field a
    /// later version adds.
    fn round_trip<T>(value: &T, text: &str)
    where
        T: Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        assert_eq!(serde_json::to_string(value).unwrap(), text);
        assert_eq!(&serde_json::from_str::<T>(text).unwrap(), value);
        let mut later: Value = serde_json::from_str(text).unwrap();
        later["added_later"] = json!(1);
        assert_eq!(&serde_json::from_value::<T>(later).unwrap(), value);
    }

    #[test]
    fn doing_is_three_timers_always_said_a_last_and_a_confidence() {
        let doing = HumanDoing {
            drank_s_ago: 1200,
            ate_s_ago: 5400,
            break_s_ago: 5400,
            last: Doing::Drinking,
            confidence: 0.75,
        };
        let text = r#"{"drank_s_ago":1200,"ate_s_ago":5400,"break_s_ago":5400,"last":"drinking","confidence":0.75}"#;
        round_trip(&doing, text);
        doing.check().unwrap();
        for field in [
            "drank_s_ago",
            "ate_s_ago",
            "break_s_ago",
            "last",
            "confidence",
        ] {
            needs::<HumanDoing>(text, field);
        }
        // Nothing seen: the timers are the sitting's length, confidence 0.
        let nothing = HumanDoing {
            drank_s_ago: 600,
            ate_s_ago: 600,
            break_s_ago: 600,
            last: Doing::Nothing,
            confidence: 0.0,
        };
        round_trip(
            &nothing,
            r#"{"drank_s_ago":600,"ate_s_ago":600,"break_s_ago":600,"last":"none","confidence":0.0}"#,
        );
        nothing.check().unwrap();
        assert!(
            HumanDoing {
                confidence: 0.5,
                ..nothing
            }
            .check()
            .is_err()
        );
        for confidence in [-0.1, 1.1, f32::NAN] {
            assert!(
                HumanDoing {
                    confidence,
                    ..doing
                }
                .check()
                .is_err()
            );
        }
        // Never a raw label.
        assert!(
            serde_json::from_str::<HumanDoing>(&text.replace("drinking", "eating burger")).is_err()
        );
    }

    #[test]
    fn a_sighting_is_of_something() {
        let seen = DoingSeen {
            what: Doing::Break,
            confidence: 0.6,
        };
        let text = r#"{"what":"break","confidence":0.6}"#;
        round_trip(&seen, text);
        seen.check().unwrap();
        needs::<DoingSeen>(text, "what");
        needs::<DoingSeen>(text, "confidence");
        assert!(
            DoingSeen {
                what: Doing::Nothing,
                ..seen
            }
            .check()
            .is_err()
        );
        assert!(
            DoingSeen {
                confidence: 2.0,
                ..seen
            }
            .check()
            .is_err()
        );
    }

    #[test]
    fn posture_is_a_window_a_share_and_a_region() {
        let posture = HumanPosture {
            window_s: 600,
            bad_share: 0.625,
            region: PostureRegion::UpperArm,
        };
        let text = r#"{"window_s":600,"bad_share":0.625,"region":"upper_arm"}"#;
        round_trip(&posture, text);
        posture.check().unwrap();
        for field in ["window_s", "bad_share", "region"] {
            needs::<HumanPosture>(text, field);
        }
        for region in ["neck", "trunk", "upper_arm", "lower_arm", "none"] {
            let text = format!(r#"{{"window_s":0,"bad_share":0.0,"region":"{region}"}}"#);
            assert!(
                serde_json::from_str::<HumanPosture>(&text).is_ok(),
                "{region}"
            );
        }
        // The wrist is fixed at 1 server-side, so it is never a region.
        assert!(serde_json::from_str::<HumanPosture>(&text.replace("upper_arm", "wrist")).is_err());
        assert!(
            HumanPosture {
                window_s: 3601,
                ..posture
            }
            .check()
            .is_err()
        );
        assert!(
            HumanPosture {
                bad_share: -0.1,
                ..posture
            }
            .check()
            .is_err()
        );
    }
}
