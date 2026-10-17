//! Stream configuration for audio processing.
//!
//! Ported from `api/audio/audio_processing.h` (StreamConfig class).

/// Configuration describing an audio stream's properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamConfig {
    sample_rate_hz: u32,
    num_channels: u16,
}

impl StreamConfig {
    /// Create a new stream configuration.
    pub const fn new(sample_rate_hz: u32, num_channels: u16) -> Self {
        Self {
            sample_rate_hz,
            num_channels,
        }
    }

    /// The sampling rate in Hz.
    #[inline]
    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    /// The number of channels.
    #[inline]
    pub fn num_channels(&self) -> u16 {
        self.num_channels
    }

    /// The number of frames per 10ms chunk.
    #[inline]
    pub fn num_frames(&self) -> usize {
        self.sample_rate_hz as usize / 100
    }

    /// Total number of samples (channels × frames).
    #[inline]
    pub fn num_samples(&self) -> usize {
        self.num_channels as usize * self.num_frames()
    }
}
