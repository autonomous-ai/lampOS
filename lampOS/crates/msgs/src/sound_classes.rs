//! Semantic room-sound scores (#1669). This type grants no authority to react.
//! Capture, retention and final privacy checks remain the publisher's job.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const SOUND_RAW_AGE_US: u64 = 2_000_000;
pub const SOUND_RESULT_AGE_US: u64 = 2_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundUnavailable {
    WarmingUp,
    Privacy,
    DeviceGap,
    Overloaded,
    InvalidModel,
    InferenceFailed,
    OwnVoice,
    OwnVoiceUnknown,
}

/// Missing voice state is not evidence that the lamp was quiet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnVoiceEvidence {
    Clear,
    Present,
    Unknown,
}

fn zero(score: &f32) -> bool {
    *score == 0.0
}

/// Independent model scores; their sum need not be one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundScores {
    pub speech: f32,
    pub music: f32,
    pub laughter: f32,
    /// A door (the highest of YAMNet's Door, Sliding door, Slam and
    /// Knock), and someone walking (#2562, guard mode's arrivals by
    /// sound). Added to version 1: an older lamp-ears says neither, read
    /// as 0, which is no door and no footsteps.
    #[serde(default)]
    pub door: f32,
    #[serde(default)]
    pub footsteps: f32,
    /// The body sounds of a person near the lamp (#7685): a cough, a throat
    /// cleared, a sneeze, a sniff, a wheeze. Scored, and so written, ONLY
    /// with `LAMP_EARS_BODY_SOUNDS=on` in lamp-ears: with it off they are 0
    /// and left out, so the message is byte for byte what it was and a reader
    /// from before them (this struct refuses a field it does not know) reads
    /// it as ever. A reader that finds none reads 0. They are a person's own:
    /// never sent from the lamp, never kept (docs/ears.md, "Body sounds").
    #[serde(default, skip_serializing_if = "zero")]
    pub cough: f32,
    #[serde(default, skip_serializing_if = "zero")]
    pub throat_clear: f32,
    #[serde(default, skip_serializing_if = "zero")]
    pub sneeze: f32,
    #[serde(default, skip_serializing_if = "zero")]
    pub sniff: f32,
    #[serde(default, skip_serializing_if = "zero")]
    pub wheeze: f32,
}

/// The original source window and completion time, all on the capture clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundWindow {
    pub start_us: u64,
    pub end_us: u64,
    pub completed_us: u64,
    pub capture_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum SoundClasses {
    /// No scores can survive in an unavailable message.
    Unavailable { reason: SoundUnavailable },
    Scored {
        model_id: String,
        window: SoundWindow,
        scores: SoundScores,
        own_voice: OwnVoiceEvidence,
    },
}

impl SoundClasses {
    pub const TYPE: &str = "lamp_msgs/SoundClasses";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "ears/classes";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(500))
            .lasting(Duration::from_millis(1500))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        let Self::Scored {
            model_id,
            window,
            scores,
            ..
        } = self
        else {
            return Ok(());
        };
        if model_id.is_empty()
            || model_id.len() > 128
            || !model_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
        {
            return Err(Invalid(
                "sound model id is a bounded identifier, not a path".into(),
            ));
        }
        for (name, score) in [
            ("speech", scores.speech),
            ("music", scores.music),
            ("laughter", scores.laughter),
            ("door", scores.door),
            ("footsteps", scores.footsteps),
            ("cough", scores.cough),
            ("throat_clear", scores.throat_clear),
            ("sneeze", scores.sneeze),
            ("sniff", scores.sniff),
            ("wheeze", scores.wheeze),
        ] {
            if !score.is_finite() || !(0.0..=1.0).contains(&score) {
                return Err(Invalid(format!(
                    "{name} score must be finite and within [0,1]"
                )));
            }
        }
        if window.start_us >= window.end_us
            || window.end_us > window.completed_us
            || window.completed_us.saturating_sub(window.start_us) > SOUND_RAW_AGE_US
        {
            return Err(Invalid(
                "sound window, queue and inference must share the two-second raw age bound".into(),
            ));
        }
        Ok(())
    }

    /// Structural/source-age checks only. A caller must still verify clock,
    /// epoch, current privacy authority and final behaviour gates.
    pub fn check_at(&self, now_us: u64) -> Result<(), Invalid> {
        self.check()?;
        if let Self::Scored { window, .. } = self
            && (window.completed_us > now_us
                || now_us.saturating_sub(window.end_us) > SOUND_RESULT_AGE_US)
        {
            return Err(Invalid(
                "sound result is future-dated or too old at publication".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scored() -> SoundClasses {
        SoundClasses::Scored {
            model_id: "yamnet-fixture-v1".into(),
            window: SoundWindow {
                start_us: 1_000_000,
                end_us: 2_000_000,
                completed_us: 2_100_000,
                capture_epoch: 7,
            },
            scores: SoundScores {
                speech: 0.9,
                music: 0.9,
                laughter: 0.2,
                door: 0.1,
                footsteps: 0.0,
                cough: 0.0,
                throat_clear: 0.0,
                sneeze: 0.0,
                sniff: 0.0,
                wheeze: 0.0,
            },
            own_voice: OwnVoiceEvidence::Unknown,
        }
    }

    #[test]
    fn scores_are_multilabel_and_missing_own_voice_is_not_clear() {
        let value = scored();
        assert!(value.check_at(2_100_000).is_ok());
        let mut json = serde_json::to_value(&value).unwrap();
        assert_eq!(json["own_voice"], "unknown");
        assert_eq!(
            serde_json::from_value::<SoundClasses>(json.clone()).unwrap(),
            value
        );
        json.as_object_mut().unwrap().remove("own_voice");
        assert!(serde_json::from_value::<SoundClasses>(json).is_err());
    }

    /// #2562: an older lamp-ears says no door or footsteps; read as none.
    /// #7685: the body sounds are written only when they are scored, so a
    /// lamp with the switch off sends the message it always did (and a reader
    /// from before them, which refuses a field it does not know, reads it).
    #[test]
    fn body_scores_off_leave_the_message_byte_for_byte_as_it_was() {
        let before =
            "{\"speech\":0.9,\"music\":0.9,\"laughter\":0.2,\"door\":0.1,\"footsteps\":0.0}";
        let SoundClasses::Scored { scores, .. } = scored() else {
            unreachable!()
        };
        assert_eq!(serde_json::to_string(&scores).unwrap(), before);
        // From before: no body field, read as 0.
        let old: SoundScores = serde_json::from_str(before).unwrap();
        assert_eq!(
            (
                old.cough,
                old.throat_clear,
                old.sneeze,
                old.sniff,
                old.wheeze
            ),
            (0.0, 0.0, 0.0, 0.0, 0.0)
        );
        // On: written, and read back.
        let on = SoundScores {
            sneeze: 0.8,
            cough: 0.4,
            ..scores
        };
        let text = serde_json::to_string(&on).unwrap();
        assert!(
            text.contains("\"cough\":0.4") && text.contains("\"sneeze\":0.8"),
            "{text}"
        );
        assert!(
            !text.contains("sniff") && !text.contains("wheeze") && !text.contains("throat_clear"),
            "a zero is left out: {text}"
        );
        assert_eq!(serde_json::from_str::<SoundScores>(&text).unwrap(), on);
    }

    #[test]
    fn a_body_score_outside_zero_to_one_or_not_finite_is_refused() {
        for bad in [-0.1, 1.5, f32::NAN] {
            let SoundClasses::Scored {
                model_id,
                window,
                scores,
                own_voice,
            } = scored()
            else {
                unreachable!()
            };
            let odd = SoundClasses::Scored {
                model_id,
                window,
                scores: SoundScores {
                    sneeze: bad,
                    ..scores
                },
                own_voice,
            };
            assert!(odd.check().is_err(), "{bad}");
        }
    }

    #[test]
    fn scores_without_a_door_or_footsteps_read_as_none() {
        let json = serde_json::json!({"speech": 0.1, "music": 0.2, "laughter": 0.0});
        let scores: SoundScores = serde_json::from_value(json).unwrap();
        assert_eq!((scores.door, scores.footsteps), (0.0, 0.0));
    }

    #[test]
    fn unavailable_cannot_carry_old_scores_or_a_source_window() {
        let value = SoundClasses::Unavailable {
            reason: SoundUnavailable::Privacy,
        };
        let mut json = serde_json::to_value(&value).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"status":"unavailable","reason":"privacy"})
        );
        assert!(value.check_at(u64::MAX).is_ok());
        json["scores"] = serde_json::json!({"speech":0,"music":1,"laughter":0});
        assert!(serde_json::from_value::<SoundClasses>(json).is_err());
    }

    #[test]
    fn old_or_reversed_windows_and_nonfinite_scores_are_refused() {
        for (start, end, completed, now) in [
            (2, 1, 3, 3),
            (1, 1, 3, 3),
            (1, 3, 2, 3),
            (1, 1_000_001, 2_000_002, 2_000_002),
            (1, 1_000_001, 1_100_001, 1_100_000),
            (1, 1_000_001, 1_100_001, 3_000_002),
        ] {
            let mut value = scored();
            if let SoundClasses::Scored { window, .. } = &mut value {
                *window = SoundWindow {
                    start_us: start,
                    end_us: end,
                    completed_us: completed,
                    capture_epoch: 7,
                };
            }
            assert!(value.check_at(now).is_err());
        }
        for score in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            let mut value = scored();
            if let SoundClasses::Scored { scores, .. } = &mut value {
                scores.music = score;
            }
            assert!(value.check().is_err());
            let mut value = scored();
            if let SoundClasses::Scored { scores, .. } = &mut value {
                scores.door = score;
            }
            assert!(value.check().is_err());
        }
        let mut value = scored();
        if let SoundClasses::Scored { model_id, .. } = &mut value {
            *model_id = "/a/model.onnx".into();
        }
        assert!(value.check().is_err());
    }

    #[test]
    fn classification_has_its_own_expiring_latest_topic() {
        let declaration = SoundClasses::declaration();
        assert_eq!(declaration.kind, Kind::Latest);
        assert_eq!(declaration.period(), Some(Duration::from_millis(500)));
        assert_eq!(declaration.lifespan(), Some(Duration::from_millis(1500)));
        assert_eq!(
            crate::declaration_of(SoundClasses::TOPIC),
            Some(declaration)
        );
        assert_eq!(crate::first_topics().len(), 17);
        assert_ne!(SoundClasses::TOPIC, crate::RoomSound::TOPIC);
        assert_ne!(SoundClasses::TOPIC, crate::SuddenSound::TOPIC);
    }
}
