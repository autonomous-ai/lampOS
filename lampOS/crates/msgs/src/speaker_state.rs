//! The sole mixer owner's observation and irreversible audio-revocation counter.
use crate::{ClockStamp, Invalid};
use lamp_bus::{Declaration, Kind, Message};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const SPEAKER_STATE_FRESH_US: u64 = 3_000_000;

/// A passive observation, never a request to change the mixer. Unknown volume
/// cannot authorize sound. Consumers bind an admitted item to the host clock,
/// publisher session and mute generation; a later unmute does not revive it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakerState {
    pub stamp: ClockStamp,
    /// The Bus publisher's sixteen-hex-character boot, not the host boot.
    pub session_id: String,
    /// Advances before mixer I/O for every valid explicit mute intent, and
    /// when a confirmed zero or loss of a known observation retires old audio.
    pub mute_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_raw: Option<u8>,
    /// Logical mute intent or a confirmed raw level of zero. `false` with
    /// unknown volume is not evidence that the speaker is available.
    pub muted: bool,
}

impl SpeakerState {
    pub const TYPE: &str = "lamp_msgs/SpeakerState";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "speaker/state";
    pub const SOURCE: &str = "actions";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(1))
            .lasting(Duration::from_secs(3))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        if self.stamp.sequence == 0
            || self.session_id.len() != 16
            || !self.session_id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.volume_raw.is_some_and(|raw| raw > 100)
            || (self.volume_raw == Some(0) && !self.muted)
        {
            return Err(Invalid("invalid speaker state".into()));
        }
        Ok(())
    }

    pub fn fresh_at(&self, clock_id: &str, now_us: u64) -> bool {
        self.check().is_ok()
            && self.stamp.clock_id == clock_id
            && now_us
                .checked_sub(self.stamp.at_us)
                .is_some_and(|age| age < SPEAKER_STATE_FRESH_US)
    }

    /// Validate identity and freshness without I/O. The consumer also fences
    /// session changes, generation/sequence regressions and observation loss.
    pub fn from_message(
        message: &Message,
        clock_id: &str,
        now_us: u64,
        now_unix_ms: u64,
    ) -> Result<Self, Invalid> {
        let header = &message.header;
        if header.topic != Self::TOPIC
            || header.source != Self::SOURCE
            || header.type_name.as_deref() != Some(Self::TYPE)
            || header.kind != Kind::Latest
            || header.enc != lamp_bus::JSON
            || header.seq == 0
            || header.expires_ms != Some(SPEAKER_STATE_FRESH_US / 1_000)
            || message.expired
            || header.expired_at(now_unix_ms)
            || header.at_unix_ms > now_unix_ms.saturating_add(lamp_bus::EXPIRY_GRACE_MS)
        {
            return Err(Invalid("invalid speaker state envelope".into()));
        }
        let value: Self = message
            .parse()
            .map_err(|error| Invalid(error.to_string()))?;
        if value.session_id != header.boot
            || value.stamp.sequence != header.seq
            || value.stamp.at_us != header.at_us
            || !value.fresh_at(clock_id, now_us)
        {
            return Err(Invalid("stale, foreign or mismatched speaker state".into()));
        }
        Ok(value)
    }
}

/// Kept outside the baseline catalogue so a disabled runtime creates nothing.
pub fn speaker_state_topics() -> Vec<(&'static str, Declaration)> {
    vec![(SpeakerState::TOPIC, SpeakerState::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SpeakerState {
        SpeakerState {
            stamp: ClockStamp {
                clock_id: "host-boot".into(),
                at_us: 10,
                sequence: 1,
            },
            session_id: "1234567890abcdef".into(),
            mute_generation: 0,
            volume_raw: Some(40),
            muted: false,
        }
    }

    #[test]
    fn additive_fields_are_tolerated_but_known_fields_are_validated() {
        let mut value = serde_json::to_value(state()).unwrap();
        value["future"] = serde_json::json!({"detail":true});
        assert_eq!(
            serde_json::from_value::<SpeakerState>(value.clone()).unwrap(),
            state()
        );
        for field in ["muted", "mute_generation", "stamp", "session_id"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<SpeakerState>(missing).is_err());
        }
        value["volume_raw"] = serde_json::json!(101);
        assert!(
            serde_json::from_value::<SpeakerState>(value)
                .unwrap()
                .check()
                .is_err()
        );
        let mut unknown = state();
        unknown.volume_raw = None;
        assert!(unknown.check().is_ok());
        unknown.volume_raw = Some(0);
        assert!(unknown.check().is_err());
        unknown.muted = true;
        assert!(unknown.check().is_ok());
        unknown.volume_raw = Some(40); // Logical mute survives hardware failure.
        assert!(unknown.check().is_ok());
    }

    #[test]
    fn host_and_session_identity_and_exact_monotonic_expiry_are_checked() {
        let state = state();
        let mut message = Message {
            header: lamp_bus::Header {
                topic: SpeakerState::TOPIC.into(),
                kind: Kind::Latest,
                enc: lamp_bus::JSON.into(),
                type_name: Some(SpeakerState::TYPE.into()),
                source: SpeakerState::SOURCE.into(),
                boot: state.session_id.clone(),
                seq: 1,
                at_us: 10,
                at_unix_ms: 1000,
                expires_ms: Some(3000),
                clock: None,
            },
            payload: serde_json::to_vec(&state).unwrap(),
            gap: 0,
            publisher_restarted: false,
            expired: false,
        };
        assert!(SpeakerState::from_message(&message, "host-boot", 10, 1000).is_ok());
        assert!(SpeakerState::from_message(&message, "host-boot", 3_000_009, 1000).is_ok());
        assert!(SpeakerState::from_message(&message, "host-boot", 3_000_010, 1000).is_err());
        assert!(SpeakerState::from_message(&message, "host-boot", 9, 1000).is_err());
        assert!(SpeakerState::from_message(&message, "another-boot", 10, 1000).is_err());
        message.header.boot = "fedcba0987654321".into();
        assert!(SpeakerState::from_message(&message, "host-boot", 10, 1000).is_err());
        message.header.boot = state.session_id;
        message.header.seq += 1;
        assert!(SpeakerState::from_message(&message, "host-boot", 10, 1000).is_err());
    }
}
