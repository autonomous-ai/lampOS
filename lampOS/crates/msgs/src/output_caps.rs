//! Optional, complete output restrictions from Actions. Source labels identify
//! entries; they are data, not authentication or actuator authority.
use crate::{ClockStamp, Invalid};
use lamp_bus::{Declaration, Kind, Message};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Duration};

pub const OUTPUT_CAPS_MAX_ENTRIES: usize = 8;
pub const OUTPUT_CAPS_MAX_BYTES: usize = 8 * 1024;
pub const OUTPUT_CAPS_FRESH_US: u64 = 3_000_000;
pub const OUTPUT_CAPS_MAX_DURATION_MS: u64 = 24 * 60 * 60 * 1_000;
/// An initialized wall-clock sanity range, not evidence of synchronization.
pub const OUTPUT_CAPS_MIN_UNIX_MS: u64 = 1_577_836_800_000; // 2020-01-01 UTC.
pub const OUTPUT_CAPS_MAX_UNIX_MS: u64 = 4_102_444_800_000; // 2100-01-01 UTC, excluded.

pub fn output_caps_clock_known(unix_ms: u64) -> bool {
    (OUTPUT_CAPS_MIN_UNIX_MS..OUTPUT_CAPS_MAX_UNIX_MS).contains(&unix_ms)
}

fn name(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

/// One temporary ceiling. All active entries combine by minimum, without a
/// priority contest. A cap can dim output, never request or authorize it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputCap {
    pub source: String,
    pub token: String,
    pub issued_unix_ms: u64,
    pub valid_until_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_channel_max: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_raw_max: Option<u8>,
}

impl OutputCap {
    pub fn check(&self) -> Result<(), Invalid> {
        if !name(&self.source)
            || !name(&self.token)
            || !output_caps_clock_known(self.issued_unix_ms)
            || !output_caps_clock_known(self.valid_until_unix_ms)
            || self.valid_until_unix_ms <= self.issued_unix_ms
            || self.valid_until_unix_ms - self.issued_unix_ms > OUTPUT_CAPS_MAX_DURATION_MS
            || self
                .light_channel_max
                .is_some_and(|limit| !(12..=120).contains(&limit))
            || self
                .volume_raw_max
                .is_some_and(|limit| !(1..=100).contains(&limit))
            || (self.light_channel_max.is_none() && self.volume_raw_max.is_none())
        {
            return Err(Invalid("invalid output cap".into()));
        }
        Ok(())
    }

    /// Unknown or regressed wall time cannot make a restriction expire.
    pub fn active_at(&self, now_unix_ms: u64) -> bool {
        !output_caps_clock_known(now_unix_ms) || now_unix_ms < self.valid_until_unix_ms
    }
}

/// The one complete Actions snapshot, including an empty list after release.
/// `revision` is persisted by Actions and does not reset with its bus session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputCaps {
    pub stamp: ClockStamp,
    pub session_id: String,
    pub revision: u64,
    pub entries: Vec<OutputCap>,
}

impl OutputCaps {
    pub const TYPE: &str = "lamp_msgs/OutputCaps";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "output/caps";
    pub const SOURCE: &str = "actions";

    /// Freshness controls admission of a snapshot, not the lifetime of its
    /// restrictions. Consumers retain admitted caps after a missing heartbeat.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(1))
            .lasting(Duration::from_secs(3))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        self.stamp.check()?;
        if self.stamp.sequence == 0
            || self.stamp.at_us == 0
            || self.session_id.len() != 16
            || !self
                .session_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.entries.len() > OUTPUT_CAPS_MAX_ENTRIES
        {
            return Err(Invalid("invalid output caps snapshot".into()));
        }
        let mut sources = BTreeSet::new();
        for entry in &self.entries {
            entry.check()?;
            if !sources.insert(&entry.source) {
                return Err(Invalid("duplicate output cap source".into()));
            }
        }
        Ok(())
    }

    /// Validate a fresh envelope on this host boot. Consumers also compare
    /// persisted revision and session sequence with their last accepted state.
    /// Unknown wall time leaves the same-host monotonic freshness check in
    /// force, so an explicit newer snapshot may still release a restriction.
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
            || header.expires_ms != Some(OUTPUT_CAPS_FRESH_US / 1_000)
            || message.payload.len() > OUTPUT_CAPS_MAX_BYTES
            || (output_caps_clock_known(now_unix_ms)
                && (message.expired
                    || header.expired_at(now_unix_ms)
                    || header.at_unix_ms > now_unix_ms.saturating_add(lamp_bus::EXPIRY_GRACE_MS)))
        {
            return Err(Invalid("invalid output caps envelope".into()));
        }
        let value: Self = message
            .parse()
            .map_err(|error| Invalid(error.to_string()))?;
        value.check()?;
        if value.session_id != header.boot
            || value.stamp.clock_id != clock_id
            || value.stamp.sequence != header.seq
            || value.stamp.at_us != header.at_us
            || value.stamp.at_us > now_us
            || now_us - value.stamp.at_us >= OUTPUT_CAPS_FRESH_US
        {
            return Err(Invalid("stale, foreign or mismatched output caps".into()));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap() -> OutputCap {
        OutputCap {
            source: "daily-rest".into(),
            token: "entry-1".into(),
            issued_unix_ms: OUTPUT_CAPS_MIN_UNIX_MS,
            valid_until_unix_ms: OUTPUT_CAPS_MIN_UNIX_MS + OUTPUT_CAPS_MAX_DURATION_MS,
            light_channel_max: Some(12),
            volume_raw_max: Some(1),
        }
    }

    fn snapshot() -> OutputCaps {
        OutputCaps {
            stamp: ClockStamp {
                clock_id: "host-boot".into(),
                at_us: 100,
                sequence: 1,
            },
            session_id: "0123456789abcdef".into(),
            revision: 0,
            entries: vec![cap()],
        }
    }

    #[test]
    fn cap_limits_deadlines_names_and_source_count_are_checked() {
        assert!(snapshot().check().is_ok());
        for change in [
            |cap: &mut OutputCap| cap.light_channel_max = Some(11),
            |cap: &mut OutputCap| cap.light_channel_max = Some(121),
            |cap: &mut OutputCap| cap.volume_raw_max = Some(0),
            |cap: &mut OutputCap| cap.volume_raw_max = Some(101),
            |cap: &mut OutputCap| cap.valid_until_unix_ms += 1,
            |cap: &mut OutputCap| cap.valid_until_unix_ms = cap.issued_unix_ms,
            |cap: &mut OutputCap| cap.issued_unix_ms = 0,
            |cap: &mut OutputCap| cap.source = "UPPER".into(),
            |cap: &mut OutputCap| cap.token = "x".repeat(65),
            |cap: &mut OutputCap| {
                cap.light_channel_max = None;
                cap.volume_raw_max = None;
            },
        ] {
            let mut value = cap();
            change(&mut value);
            assert!(value.check().is_err());
        }
        let mut value = snapshot();
        value.entries.push(cap());
        assert!(value.check().is_err());
        value.entries = (0..OUTPUT_CAPS_MAX_ENTRIES)
            .map(|i| OutputCap {
                source: format!("cap-{i}"),
                ..cap()
            })
            .collect();
        assert!(value.check().is_ok());
        value.entries.push(OutputCap {
            source: "one-more".into(),
            ..cap()
        });
        assert!(value.check().is_err());
    }

    #[test]
    fn output_cap_inputs_refuse_unknown_fields_and_invalid_scalars() {
        let mut json = serde_json::to_value(snapshot()).unwrap();
        json["entries"][0]["future_mode"] = "guess".into();
        assert!(serde_json::from_value::<OutputCaps>(json).is_err());
        let mut json = serde_json::to_value(snapshot()).unwrap();
        json["mode"] = "unknown".into();
        assert!(serde_json::from_value::<OutputCaps>(json).is_err());
        let mut json = serde_json::to_value(snapshot()).unwrap();
        json["entries"][0]["volume_raw_max"] = 256.into();
        assert!(serde_json::from_value::<OutputCaps>(json).is_err());
    }

    #[test]
    fn restrictions_expire_only_with_a_plausible_wall_clock() {
        let cap = cap();
        assert!(cap.active_at(0));
        assert!(cap.active_at(u64::MAX));
        assert!(cap.active_at(cap.issued_unix_ms - 1));
        assert!(cap.active_at(cap.valid_until_unix_ms - 1));
        assert!(!cap.active_at(cap.valid_until_unix_ms));
    }
}
