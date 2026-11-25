//! Private correlation for built-in lines from an admitted spoken tool call.
//! It identifies worker-owned history scope; it grants no tool or turn authority.
use crate::Invalid;
use serde::{Deserialize, Serialize};

/// Original same-host dispatch facts, supplied by Voice rather than model args.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixedLineOrigin {
    pub worker_boot: String,
    pub provider_session: u64,
    pub request: u64,
    pub call_id: String,
    pub route_epoch: u64,
    pub microphone_generation: u64,
    pub camera_generation: u64,
    pub issued_us: u64,
}

impl FixedLineOrigin {
    pub fn check(&self) -> Result<(), Invalid> {
        let bounded = |value: &str, most: usize| {
            !value.is_empty() && value.len() <= most && !value.chars().any(char::is_control)
        };
        if !bounded(&self.worker_boot, 128)
            || !bounded(&self.call_id, 256)
            || self.provider_session == 0
            || self.request == 0
            || self.route_epoch == 0
            || self.issued_us == 0
            || self.issued_us.checked_add(600_000_000).is_none()
        {
            return Err(Invalid("invalid fixed line origin".into()));
        }
        Ok(())
    }

    /// Changing time or privacy counters cannot mint another call identity.
    pub fn same_call(&self, other: &Self) -> bool {
        self.worker_boot == other.worker_boot
            && self.provider_session == other.provider_session
            && self.request == other.request
            && self.call_id == other.call_id
            && self.route_epoch == other.route_epoch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> FixedLineOrigin {
        FixedLineOrigin {
            worker_boot: "worker-1".into(),
            provider_session: 2,
            request: 3,
            call_id: "call-4".into(),
            route_epoch: 5,
            microphone_generation: 0,
            camera_generation: 0,
            issued_us: 6,
        }
    }

    #[test]
    fn fixed_line_origin_has_strict_bounded_identity() {
        let valid = origin();
        valid.check().unwrap();
        let mut bad = valid.clone();
        for text in [String::new(), "a".repeat(129), "worker\n".into()] {
            bad.worker_boot = text;
            assert!(bad.check().is_err());
        }
        for text in [String::new(), "a".repeat(257), "call\0".into()] {
            bad = valid.clone();
            bad.call_id = text;
            assert!(bad.check().is_err());
        }
        for field in ["provider_session", "request", "route_epoch", "issued_us"] {
            let mut json = serde_json::to_value(&valid).unwrap();
            json[field] = 0.into();
            assert!(
                serde_json::from_value::<FixedLineOrigin>(json)
                    .unwrap()
                    .check()
                    .is_err()
            );
        }
        bad = valid.clone();
        bad.issued_us = u64::MAX;
        assert!(bad.check().is_err());
        let mut json = serde_json::to_value(&valid).unwrap();
        json["person"] = "not permitted".into();
        assert!(serde_json::from_value::<FixedLineOrigin>(json).is_err());
    }

    #[test]
    fn fixed_line_origin_identity_does_not_renew_on_time_or_privacy() {
        let first = origin();
        let mut later = first.clone();
        later.issued_us += 1;
        later.microphone_generation += 1;
        later.camera_generation += 1;
        assert!(first.same_call(&later));
        later.route_epoch += 1;
        assert!(!first.same_call(&later));
        later = first.clone();
        later.call_id.push('x');
        assert!(!first.same_call(&later));
    }
}
