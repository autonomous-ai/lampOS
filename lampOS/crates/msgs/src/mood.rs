//! The creature's temporary inner state. These values describe manner only;
//! they never grant microphone, camera, speech, motion or listening authority.
use crate::{ClockStamp, Invalid};
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const MOOD_STATE_PERIOD_US: u64 = 1_000_000;
pub const MOOD_STATE_TTL_US: u64 = 2_500_000;
pub const MOOD_MAX_FACE_ID_BYTES: usize = 128;

/// A single, temporary focus. A face is scoped to its publisher instance,
/// never a person's identity. Sound carries no invented sound direction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttentionTarget {
    Face { publisher_boot: String, id: String },
    Sound,
    Touch,
}

impl AttentionTarget {
    pub fn check(&self) -> Result<(), Invalid> {
        if let Self::Face { publisher_boot, id } = self {
            if publisher_boot.len() != 16
                || !publisher_boot
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(Invalid(
                    "face publisher boot must be 16 lowercase hex characters".into(),
                ));
            }
            if id.is_empty()
                || id.len() > MOOD_MAX_FACE_ID_BYTES
                || id.chars().any(char::is_control)
            {
                return Err(Invalid(
                    "face id must have 1..128 bytes without controls".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Recognition is deliberately unavailable. Passing face tracks are not
/// evidence that the lamp knows someone, even after repeated observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Familiarity {
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoodState {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub awake: f32,
    pub energy: f32,
    pub warmth: f32,
    /// A metadata sleep latch, not proof that microphones or outputs closed.
    pub asleep: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<AttentionTarget>,
    pub familiarity: Familiarity,
    /// How familiar the remembered person at the lamp is, while the eyes
    /// say they are there (#1670, `humans/familiar`): a level only, no name.
    /// Optional, beside `familiarity: unknown`, which stays for v1 readers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub familiar: Option<crate::FamiliarLevel>,
}

impl MoodState {
    pub const TYPE: &str = "lamp_msgs/MoodState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "mood/state";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_micros(MOOD_STATE_PERIOD_US))
            .lasting(Duration::from_micros(MOOD_STATE_TTL_US))
    }

    /// Shape only: consumers also check current boot, publisher session,
    /// monotonic ordering, expiry and their own independent output authority.
    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        if self.stamp.sequence == 0
            || self.session_id.is_empty()
            || self.session_id.len() > 128
            || self.session_id.chars().any(char::is_control)
        {
            return Err(Invalid(
                "mood needs a positive sequence and a bounded session identity".into(),
            ));
        }
        for (name, value) in [
            ("awake", self.awake),
            ("energy", self.energy),
            ("warmth", self.warmth),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(Invalid(format!(
                    "mood {name} must be finite and from 0 to 1"
                )));
            }
        }
        if self.asleep && (self.awake > 0.25 || self.attention.is_some()) {
            return Err(Invalid(
                "sleeping mood cannot claim high awake or an attention target".into(),
            ));
        }
        if let Some(attention) = &self.attention {
            attention.check()?;
        }
        Ok(())
    }
}

/// Optional Mood contract, kept separate from the original first topics.
pub fn mood_topics() -> Vec<(&'static str, Declaration)> {
    vec![(MoodState::TOPIC, MoodState::declaration())]
}
