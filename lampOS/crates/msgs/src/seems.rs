//! How the person at the lamp seems from their face (`humans/seems`, #1671,
//! `docs/eyes.md`): a kind and a confidence, for measuring how often the
//! read is wrong before anything acts on it. No name, no face, no picture.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// `positive` (happy), `negative` (low, cross, afraid), or `unsure`: no
/// kind held most of the read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeemsKind {
    Positive,
    Negative,
    Unsure,
}

/// Which reader said it: FER+ (`lamp-eyes --seems`, for the measure on our
/// own lamps only) or Google's Blendshape V2 (`--seems-blendshapes`, the
/// product's). Both may run side by side for the measure (the first
/// builder, 20 Oct 09:51). A message without `from` was said by FER+, the
/// only reader before this field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeemsReader {
    #[default]
    FerPlus,
    Blendshapes,
}

/// One read of the near face. Said only with `lamp-eyes --seems` or
/// `--seems-blendshapes`, while a face is near. A reader treats no fresh
/// value as not known; with both readers on, a value is either's, so a
/// reader that wants one takes the stream, not `latest`, and looks at
/// `from`: on a look where both read, `latest` holds only the second.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HumanSeems {
    pub kind: SeemsKind,
    /// The model's share for the kind, 0 to 1; for `unsure`, the larger of
    /// the positive and negative shares.
    pub confidence: f32,
    #[serde(default)]
    pub from: SeemsReader,
}

impl HumanSeems {
    pub const TYPE: &str = "lamp_msgs/HumanSeems";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "humans/seems";
    /// Said at each read, at most every two seconds while a face is near.
    pub const EVERY: Duration = Duration::from_secs(2);
    /// A value means nothing this long after it was said.
    pub const LASTS: Duration = Duration::from_secs(5);

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Self::EVERY)
            .lasting(Self::LASTS)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if (0.0..=1.0).contains(&self.confidence) {
            Ok(())
        } else {
            Err(Invalid("confidence is from 0 to 1".into()))
        }
    }
}

/// The face-read topic, apart from the first topics.
pub fn seems_topics() -> Vec<(&'static str, Declaration)> {
    vec![(HumanSeems::TOPIC, HumanSeems::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_is_a_kind_and_a_confidence_only() {
        let read = HumanSeems {
            kind: SeemsKind::Negative,
            confidence: 0.7,
            from: SeemsReader::Blendshapes,
        };
        let json = serde_json::to_string(&read).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"negative","confidence":0.7,"from":"blendshapes"}"#
        );
        assert_eq!(serde_json::from_str::<HumanSeems>(&json).unwrap(), read);
        assert!(read.check().is_ok());
        for confidence in [-0.1, 1.1, f32::NAN] {
            assert!(HumanSeems { confidence, ..read }.check().is_err());
        }
    }

    /// A message from before `from` reads as FER+'s, the one reader then,
    /// so every consumer keeps working (the first builder, 09:51).
    #[test]
    fn a_read_without_from_is_fer_plus() {
        let read: HumanSeems =
            serde_json::from_str(r#"{"kind":"positive","confidence":0.9}"#).unwrap();
        assert_eq!(read.from, SeemsReader::FerPlus);
        let json = serde_json::to_string(&read).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"positive","confidence":0.9,"from":"fer_plus"}"#
        );
        assert!(
            serde_json::from_str::<HumanSeems>(
                r#"{"kind":"positive","confidence":0.9,"from":"other"}"#
            )
            .is_err()
        );
    }
}
