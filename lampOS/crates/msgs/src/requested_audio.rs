//! Private Skills/Voice authorization for person-requested local audio.
//! These are socket receipts, never model arguments or bus authority claims.
use crate::Invalid;
use serde::{Deserialize, Serialize};

pub const REQUESTED_AUDIO_START_US: u64 = 10_000_000;
pub const REQUESTED_AUDIO_PLAY_US: u64 = 60_000_000;
pub const REQUESTED_AUDIO_LEASE_US: u64 = 500_000;
pub const REQUESTED_AUDIO_RATE: u32 = 24_000;
pub const REQUESTED_AUDIO_MOST_FRAMES: u32 = REQUESTED_AUDIO_RATE * 60;

fn bounded(value: &str, most: usize) -> bool {
    !value.is_empty() && value.len() <= most && !value.chars().any(char::is_control)
}

/// Captured by Voice once, at the original spoken tool dispatch. No field
/// comes from model arguments. Monotonic times are on the same host clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedAudioContext {
    pub worker_boot: String,
    pub provider_session: u64,
    pub request: u64,
    pub privacy_generation: u64,
    pub call_id: String,
    pub issued_us: u64,
}

impl RequestedAudioContext {
    pub fn check(&self) -> Result<(), Invalid> {
        if !bounded(&self.worker_boot, 128)
            || !bounded(&self.call_id, 256)
            || self.provider_session == 0
            || self.request == 0
            || self.issued_us == 0
            || self
                .issued_us
                .checked_add(REQUESTED_AUDIO_START_US + REQUESTED_AUDIO_PLAY_US)
                .is_none()
        {
            return Err(Invalid("invalid requested audio context".into()));
        }
        Ok(())
    }

    /// A timestamp change cannot turn a repeated call into a new call.
    pub fn same_call(&self, other: &Self) -> bool {
        self.worker_boot == other.worker_boot
            && self.provider_session == other.provider_session
            && self.request == other.request
            && self.call_id == other.call_id
    }
}

/// One immutable checked asset and the original, nonrenewable time bounds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedAudioOffer {
    pub skills_boot: String,
    pub definition_generation: u64,
    pub offer_id: u64,
    pub context: RequestedAudioContext,
    pub skill: String,
    pub asset: String,
    pub sha256: String,
    pub frames: u32,
    pub issued_us: u64,
    pub start_before_us: u64,
    pub finish_before_us: u64,
}

impl RequestedAudioOffer {
    pub fn check(&self) -> Result<(), Invalid> {
        self.context.check()?;
        if !bounded(&self.skills_boot, 128)
            || self.definition_generation == 0
            || self.offer_id == 0
            || !(1..=64).contains(&self.skill.len())
            || !self
                .skill
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !bounded(&self.asset, 4096)
            || !std::path::Path::new(&self.asset).is_absolute()
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !(1..=REQUESTED_AUDIO_MOST_FRAMES).contains(&self.frames)
            || self.issued_us != self.context.issued_us
            || self.issued_us.checked_add(REQUESTED_AUDIO_START_US) != Some(self.start_before_us)
            || self.finish_before_us <= self.start_before_us
            || self.finish_before_us > self.start_before_us.saturating_add(REQUESTED_AUDIO_PLAY_US)
        {
            return Err(Invalid("invalid requested audio offer".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedAudioPhase {
    Start,
    Renew,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedAudioOutcome {
    Completed,
    Cancelled,
}

/// Private operations on Skills' existing call.sock. Unknown operations,
/// phases and outcomes are refused, including in otherwise valid receipts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestedAudioOperation {
    AudioAuthorize {
        offer: RequestedAudioOffer,
        phase: RequestedAudioPhase,
    },
    AudioFinish {
        offer: RequestedAudioOffer,
        outcome: RequestedAudioOutcome,
    },
}

impl RequestedAudioOperation {
    pub fn check(&self) -> Result<(), Invalid> {
        match self {
            Self::AudioAuthorize { offer, .. } | Self::AudioFinish { offer, .. } => offer.check(),
        }
    }
}

/// Permission for at most 500 ms, bounded again by the original finish.
/// Voice compares this identity with its offer and checks both clocks at
/// every dispatch; a lease is not evidence of audible completion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestedAudioLease {
    pub skills_boot: String,
    pub definition_generation: u64,
    pub offer_id: u64,
    pub until_us: u64,
}

impl RequestedAudioLease {
    pub fn check(&self, offer: &RequestedAudioOffer, now_us: u64) -> Result<(), Invalid> {
        offer.check()?;
        if self.skills_boot != offer.skills_boot
            || self.definition_generation != offer.definition_generation
            || self.offer_id != offer.offer_id
            || now_us < offer.issued_us
            || self.until_us <= now_us
            || self.until_us > now_us.saturating_add(REQUESTED_AUDIO_LEASE_US)
            || self.until_us > offer.finish_before_us
        {
            return Err(Invalid("invalid requested audio lease".into()));
        }
        Ok(())
    }
}
