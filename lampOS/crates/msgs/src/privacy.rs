//! Voice's short capture lease. Controls observations never grant permission.
use crate::{ClockStamp, Invalid};
use lamp_bus::{Declaration, Kind, Message};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const MICROPHONE_PRIVACY_FRESH_US: u64 = 50_000;

/// Permission for an independent capture owner, renewed only by Voice while
/// both its controller and the original physical observation remain valid
/// (on a lamp with no slide, the Controls publication saying so).
/// Expiry is on the named kernel boot's CLOCK_MONOTONIC, not wall time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MicrophonePrivacy {
    pub stamp: ClockStamp,
    /// The Bus publisher's sixteen-hex-character boot, not the controller boot.
    pub session_id: String,
    /// The private IPC controller's 32-hex-character boot.
    pub controller_boot: String,
    pub microphone_generation: u64,
    /// Original switch acquisition; zero when physical state is unknown.
    pub observed_us: u64,
    pub expires_at_us: u64,
    pub permitted: bool,
    /// Unknown is neither permission nor evidence of a deliberate user mute.
    pub physical_muted: Option<bool>,
    /// A lamp with no slide (`LAMP_PRIVACY_CONTROLS=no-slide`): Controls said
    /// so, `observed_us` is that Controls publication, and `physical_muted`
    /// stays unknown. Only Voice's controller (its software mute) closes it.
    /// Absent, and so left out, on a slide lamp, whose lease is byte for
    /// byte as before. An Ears built before this field ignores it and finds
    /// a no-slide lease invalid (a permit with unknown physical state, or an
    /// acquisition with no level), so it stays closed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_slide: bool,
}

impl MicrophonePrivacy {
    pub const TYPE: &str = "lamp_msgs/MicrophonePrivacy";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/privacy";

    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_millis(10))
            .lasting(Duration::from_millis(50))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        let hex = |text: &str, length| {
            text.len() == length && text.bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        if !hex(&self.session_id, 16)
            || !hex(&self.controller_boot, 32)
            || self.controller_boot.bytes().all(|byte| byte == b'0')
            || self.stamp.sequence == 0
            || self.microphone_generation == 0
            || self.expires_at_us <= self.stamp.at_us
            || self.expires_at_us - self.stamp.at_us > MICROPHONE_PRIVACY_FRESH_US
            // A permit needs a switch read open, or a lamp with no slide,
            // whose physical state is never inferred.
            || (self.permitted
                && self.physical_muted != Some(false)
                && !(self.no_slide && self.physical_muted.is_none()))
            || (self.no_slide && self.physical_muted.is_some())
        {
            return Err(Invalid("invalid microphone privacy lease".into()));
        }
        // A switch reading, or on a lamp with no slide the Controls
        // publication that said so, is fenced the same way.
        if self.physical_muted.is_some() || self.no_slide {
            if self.observed_us == 0
                || self.observed_us > self.stamp.at_us
                || self.stamp.at_us - self.observed_us >= MICROPHONE_PRIVACY_FRESH_US
                || self.expires_at_us.saturating_sub(self.observed_us) > MICROPHONE_PRIVACY_FRESH_US
            {
                return Err(Invalid(
                    "privacy lease outlives its switch acquisition".into(),
                ));
            }
        } else if self.observed_us != 0 {
            return Err(Invalid("unknown privacy has no acquisition".into()));
        }
        Ok(())
    }

    pub fn fresh_at(&self, clock_id: &str, now_us: u64) -> bool {
        self.check().is_ok()
            && self.stamp.clock_id == clock_id
            && self.stamp.at_us <= now_us
            && now_us < self.expires_at_us
    }

    /// Validate the complete typed envelope without doing I/O. Consumers must
    /// additionally fence gaps, publisher changes, sequence/time regressions
    /// and queue loss before accepting permission from this value.
    pub fn from_message(
        message: &Message,
        clock_id: &str,
        now_us: u64,
        now_unix_ms: u64,
    ) -> Result<Self, Invalid> {
        let header = &message.header;
        if header.topic != Self::TOPIC
            || header.source != "voice"
            || header.type_name.as_deref() != Some(Self::TYPE)
            || header.kind != Kind::Latest
            || header.enc != lamp_bus::JSON
            || header.seq == 0
            || header.expires_ms != Some(50)
            || message.expired
            || header.expired_at(now_unix_ms)
            || header.at_unix_ms > now_unix_ms.saturating_add(lamp_bus::EXPIRY_GRACE_MS)
        {
            return Err(Invalid("invalid microphone privacy envelope".into()));
        }
        let value: Self = message
            .parse()
            .map_err(|error| Invalid(error.to_string()))?;
        if value.session_id != header.boot
            || value.stamp.at_us != header.at_us
            || value.stamp.sequence != header.seq
            || !value.fresh_at(clock_id, now_us)
        {
            return Err(Invalid(
                "stale, foreign or mismatched microphone privacy".into(),
            ));
        }
        Ok(value)
    }
}

/// Kept separate from the original topic catalogue so the disabled runtime
/// creates no new topics or declarations.
pub fn privacy_topics() -> Vec<(&'static str, Declaration)> {
    vec![(MicrophonePrivacy::TOPIC, MicrophonePrivacy::declaration())]
}
