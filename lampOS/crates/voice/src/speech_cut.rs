//! Bounded speech-cut metadata carried on priority worker control channels.
//!
//! A cut identifies already generated audio; it grants no turn or playback
//! authority. Receivers validate the request/owner against their live state.
//! Provider and speaker watermarks are different sequence spaces and must not
//! be interchanged. Zero is a valid watermark before the first audio packet.
use serde::{Deserialize, Serialize};

/// Maximum wait for the coordinator's acknowledgement after the physical
/// speaker cut. A local control-path deadline, not a claim of acoustic silence.
pub(crate) const ACK_TIMEOUT_US: u64 = 250_000;

/// An admitted forget's inclusive provider Audio.sequence watermark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCut {
    pub request: u64,
    pub through: u64,
    /// Local monotonic admission time, for boundary measurements, not authority.
    pub admitted_at_us: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cut_is_fixed_size_and_preserves_zero_and_maximum_watermarks() {
        for through in [0, u64::MAX] {
            let cut = ProviderCut {
                request: 7,
                through,
                admitted_at_us: 100,
            };
            assert_eq!(
                serde_json::from_slice::<ProviderCut>(&serde_json::to_vec(&cut).unwrap()).unwrap(),
                cut
            );
        }
    }

    #[test]
    fn cut_rejects_missing_extra_negative_and_out_of_range_fields() {
        for bad in [
            json!({"request":7,"through":0}),
            json!({"request":7,"through":0,"admitted_at_us":100,"authority":true}),
            json!({"request":7,"through":-1,"admitted_at_us":100}),
            json!({"request":7,"through":0,"admitted_at_us":"100"}),
        ] {
            assert!(serde_json::from_value::<ProviderCut>(bad).is_err());
        }
        assert!(
            serde_json::from_str::<ProviderCut>(
                r#"{"request":7,"through":18446744073709551616,"admitted_at_us":100}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ProviderCut>(
                r#"{"request":7,"through":0,"through":1,"admitted_at_us":100}"#
            )
            .is_err()
        );
    }
}
