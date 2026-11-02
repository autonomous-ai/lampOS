//! Immutable, verified audio identity for the central model accounting boundary.
use crate::{Result, hash, invalid, speech};
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};

#[derive(Clone, Serialize)]
pub struct Input {
    wav_sha256: String,
    pcm_sha256: String,
    samples: u64,
    #[serde(skip)]
    pcm: Arc<[u8]>,
}
impl fmt::Debug for Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedAudio")
            .field("wav_sha256", &self.wav_sha256)
            .field("pcm_sha256", &self.pcm_sha256)
            .field("samples", &self.samples)
            .finish()
    }
}
impl Input {
    pub fn from_wav(wav: &[u8]) -> Result<Self> {
        let bytes = speech::pcm(wav)?;
        Ok(Self {
            wav_sha256: hash(wav),
            pcm_sha256: hash(bytes),
            samples: bytes.len() as u64 / 2,
            pcm: bytes.into(),
        })
    }
    pub fn pcm(&self) -> &[u8] {
        &self.pcm
    }
    pub fn samples(&self) -> u64 {
        self.samples
    }
    pub fn pcm_sha256(&self) -> &str {
        &self.pcm_sha256
    }
    pub fn wav_sha256(&self) -> &str {
        &self.wav_sha256
    }
    /// Round the verified sample duration up to one millisecond for reservation.
    /// This small bound covers decimal metadata rounding without a billed block.
    pub fn reserve_micros(&self) -> u64 {
        self.samples.div_ceil(16) * 1000
    }
    pub fn validate(&self) -> Result<()> {
        if self.samples == 0
            || self.samples > 16_000 * 120
            || self.pcm.len() as u64 != self.samples * 2
        {
            return Err(invalid("invalid verified PCM duration"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub audio_micros: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prices {
    pub source: String,
    pub date: String,
    pub microusd_per_minute: u64,
}
impl Prices {
    pub fn validate(&self) -> Result<()> {
        if self.source.is_empty() || self.date.is_empty() || self.microusd_per_minute == 0 {
            return Err(invalid("dated nonzero speech pricing required"));
        }
        Ok(())
    }
    pub fn charge(&self, micros: u64) -> Result<u64> {
        let amount =
            (u128::from(micros) * u128::from(self.microusd_per_minute)).div_ceil(60_000_000);
        u64::try_from(amount).map_err(|_| invalid("speech charge overflow"))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub pcm_sha256: String,
    pub samples_sent: u64,
    pub transcript_sha256: String,
    pub confidence: Option<f64>,
    pub model_revision: Option<String>,
    pub model_uuid: Option<String>,
    pub duration_usage: Option<Usage>,
}
