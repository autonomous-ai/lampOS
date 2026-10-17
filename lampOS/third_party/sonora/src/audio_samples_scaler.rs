//! Audio samples scaler — applies gain with linear ramping.
//!
//! Ported from `modules/audio_processing/capture_levels_adjuster/audio_samples_scaler.h/cc`.

use crate::audio_buffer::AudioBuffer;

const MIN_FLOAT_S16_VALUE: f32 = -32768.0;
const MAX_FLOAT_S16_VALUE: f32 = 32767.0;

/// Applies a gain to audio samples with linear ramping over one frame.
#[derive(Debug)]
pub(crate) struct AudioSamplesScaler {
    previous_gain: f32,
    target_gain: f32,
    samples_per_channel: i32,
    one_by_samples_per_channel: f32,
}

impl AudioSamplesScaler {
    /// Create a new scaler with the given initial gain (applied immediately).
    pub(crate) fn new(initial_gain: f32) -> Self {
        Self {
            previous_gain: initial_gain,
            target_gain: initial_gain,
            samples_per_channel: -1,
            one_by_samples_per_channel: -1.0,
        }
    }

    /// Set the target gain. Changes take effect gradually over the next frame.
    pub(crate) fn set_gain(&mut self, gain: f32) {
        self.target_gain = gain;
    }

    /// Apply gain to the audio buffer with linear ramping and S16 clamping.
    pub(crate) fn process(&mut self, audio_buffer: &mut AudioBuffer) {
        let num_frames = audio_buffer.num_frames();

        if num_frames as i32 != self.samples_per_channel {
            debug_assert!(num_frames > 0);
            self.samples_per_channel = num_frames as i32;
            self.one_by_samples_per_channel = 1.0 / self.samples_per_channel as f32;
        }

        if self.target_gain == 1.0 && self.previous_gain == self.target_gain {
            return;
        }

        let num_channels = audio_buffer.num_channels();

        if self.previous_gain == self.target_gain {
            // Apply constant gain.
            let gain = self.previous_gain;
            for channel in 0..num_channels {
                let data = audio_buffer.channel_mut(channel);
                for sample in data[..num_frames].iter_mut() {
                    *sample *= gain;
                }
            }
        } else {
            let increment =
                (self.target_gain - self.previous_gain) * self.one_by_samples_per_channel;

            if increment > 0.0 {
                // Apply increasing gain.
                for channel in 0..num_channels {
                    let mut gain = self.previous_gain;
                    let data = audio_buffer.channel_mut(channel);
                    for sample in data[..num_frames].iter_mut() {
                        gain = (gain + increment).min(self.target_gain);
                        *sample *= gain;
                    }
                }
            } else {
                // Apply decreasing gain.
                for channel in 0..num_channels {
                    let mut gain = self.previous_gain;
                    let data = audio_buffer.channel_mut(channel);
                    for sample in data[..num_frames].iter_mut() {
                        gain = (gain + increment).max(self.target_gain);
                        *sample *= gain;
                    }
                }
            }
        }
        self.previous_gain = self.target_gain;

        // Clamp to S16 range.
        for channel in 0..num_channels {
            let data = audio_buffer.channel_mut(channel);
            for sample in data[..num_frames].iter_mut() {
                *sample = sample.clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_value_for_channel(channel: usize) -> f32 {
        100.0 + channel as f32
    }

    fn populate_buffer(audio_buffer: &mut AudioBuffer) {
        let num_ch = audio_buffer.num_channels();
        let num_fr = audio_buffer.num_frames();
        for ch in 0..num_ch {
            let value = sample_value_for_channel(ch);
            let data = audio_buffer.channel_mut(ch);
            for sample in data[..num_fr].iter_mut() {
                *sample = value;
            }
        }
    }

    const NUM_FRAMES_TO_PROCESS: usize = 10;

    fn test_params() -> Vec<(usize, usize, f32)> {
        let mut params = Vec::new();
        for &rate in &[16000, 32000, 48000] {
            for &channels in &[1, 2, 4] {
                for &gain in &[0.1, 1.0, 2.0, 4.0] {
                    params.push((rate, channels, gain));
                }
            }
        }
        params
    }

    #[test]
    fn initial_gain_is_respected() {
        for (sample_rate_hz, num_channels, initial_gain) in test_params() {
            let mut scaler = AudioSamplesScaler::new(initial_gain);
            let mut audio_buffer = AudioBuffer::new(
                sample_rate_hz,
                num_channels,
                sample_rate_hz,
                num_channels,
                sample_rate_hz,
            );

            for _ in 0..NUM_FRAMES_TO_PROCESS {
                populate_buffer(&mut audio_buffer);
                scaler.process(&mut audio_buffer);
                let num_fr = audio_buffer.num_frames();
                for ch in 0..num_channels {
                    let expected = initial_gain * sample_value_for_channel(ch);
                    let clamped = expected.clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
                    let data = audio_buffer.channel(ch);
                    for &sample in &data[..num_fr] {
                        assert!(
                            (sample - clamped).abs() < 1e-4,
                            "rate={sample_rate_hz}, ch={ch}, gain={initial_gain}: expected {clamped}, got {sample}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn verify_gain_adjustment() {
        for (sample_rate_hz, num_channels, higher_gain) in test_params() {
            let lower_gain = higher_gain / 2.0;
            let mut scaler = AudioSamplesScaler::new(lower_gain);
            let mut audio_buffer = AudioBuffer::new(
                sample_rate_hz,
                num_channels,
                sample_rate_hz,
                num_channels,
                sample_rate_hz,
            );

            // Allow initial lower gain to take effect.
            populate_buffer(&mut audio_buffer);
            scaler.process(&mut audio_buffer);

            // Set higher gain.
            scaler.set_gain(higher_gain);

            // Gain should ramp up gradually over one frame.
            populate_buffer(&mut audio_buffer);
            scaler.process(&mut audio_buffer);
            let num_fr = audio_buffer.num_frames();
            for ch in 0..num_channels {
                let target = higher_gain * sample_value_for_channel(ch);
                let clamped_target = target.clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
                let data = audio_buffer.channel(ch);
                for i in 0..num_fr - 1 {
                    assert!(
                        data[i] <= clamped_target + 1e-4,
                        "rate={sample_rate_hz}, ch={ch}: sample[{i}]={} should be <= {clamped_target}",
                        data[i],
                    );
                    assert!(
                        data[i] <= data[i + 1] + 1e-4,
                        "rate={sample_rate_hz}, ch={ch}: samples should be non-decreasing",
                    );
                }
                assert!(
                    data[num_fr - 1] <= clamped_target + 1e-4,
                    "last sample should be <= target",
                );
            }

            // After transition, gain should be stable at higher_gain.
            for _ in 0..NUM_FRAMES_TO_PROCESS {
                populate_buffer(&mut audio_buffer);
                scaler.process(&mut audio_buffer);
                let num_fr = audio_buffer.num_frames();
                for ch in 0..num_channels {
                    let expected = higher_gain * sample_value_for_channel(ch);
                    let clamped = expected.clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
                    let data = audio_buffer.channel(ch);
                    for &sample in &data[..num_fr] {
                        assert!(
                            (sample - clamped).abs() < 1e-4,
                            "rate={sample_rate_hz}, ch={ch}: expected stable {clamped}, got {sample}",
                        );
                    }
                }
            }

            // Set lower gain.
            scaler.set_gain(lower_gain);

            // Gain should ramp down gradually.
            populate_buffer(&mut audio_buffer);
            scaler.process(&mut audio_buffer);
            let num_fr = audio_buffer.num_frames();
            for ch in 0..num_channels {
                let target = lower_gain * sample_value_for_channel(ch);
                let clamped_target = target.clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
                let data = audio_buffer.channel(ch);
                for i in 0..num_fr - 1 {
                    assert!(
                        data[i] >= clamped_target - 1e-4,
                        "rate={sample_rate_hz}, ch={ch}: sample[{i}]={} should be >= {clamped_target}",
                        data[i],
                    );
                    assert!(
                        data[i] >= data[i + 1] - 1e-4,
                        "rate={sample_rate_hz}, ch={ch}: samples should be non-increasing",
                    );
                }
                assert!(
                    data[num_fr - 1] >= clamped_target - 1e-4,
                    "last sample should be >= target",
                );
            }

            // After transition, gain should be stable at lower_gain.
            for _ in 0..NUM_FRAMES_TO_PROCESS {
                populate_buffer(&mut audio_buffer);
                scaler.process(&mut audio_buffer);
                let num_fr = audio_buffer.num_frames();
                for ch in 0..num_channels {
                    let expected = lower_gain * sample_value_for_channel(ch);
                    let clamped = expected.clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
                    let data = audio_buffer.channel(ch);
                    for &sample in &data[..num_fr] {
                        assert!(
                            (sample - clamped).abs() < 1e-4,
                            "rate={sample_rate_hz}, ch={ch}: expected stable {clamped}, got {sample}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn upwards_clamping() {
        let sample_rate_hz = 48000;
        let num_channels = 1;
        let gain = 10.0f32;
        let max_clamped = MAX_FLOAT_S16_VALUE;

        let mut scaler = AudioSamplesScaler::new(gain);
        let mut audio_buffer = AudioBuffer::new(
            sample_rate_hz,
            num_channels,
            sample_rate_hz,
            num_channels,
            sample_rate_hz,
        );

        let num_fr = audio_buffer.num_frames();
        for _ in 0..NUM_FRAMES_TO_PROCESS {
            // Fill with values near max that when multiplied by gain exceed max.
            for ch in 0..num_channels {
                let value = max_clamped - num_channels as f32 + 1.0 + ch as f32;
                let data = audio_buffer.channel_mut(ch);
                for sample in data[..num_fr].iter_mut() {
                    *sample = value;
                }
            }

            scaler.process(&mut audio_buffer);
            for ch in 0..num_channels {
                let data = audio_buffer.channel(ch);
                for &sample in &data[..num_fr] {
                    assert!(
                        (sample - max_clamped).abs() < 1e-4,
                        "expected {max_clamped}, got {sample}",
                    );
                }
            }
        }
    }

    #[test]
    fn downwards_clamping() {
        let sample_rate_hz = 48000;
        let num_channels = 1;
        let gain = 10.0f32;
        let min_clamped = MIN_FLOAT_S16_VALUE;

        let mut scaler = AudioSamplesScaler::new(gain);
        let mut audio_buffer = AudioBuffer::new(
            sample_rate_hz,
            num_channels,
            sample_rate_hz,
            num_channels,
            sample_rate_hz,
        );

        let num_fr = audio_buffer.num_frames();
        for _ in 0..NUM_FRAMES_TO_PROCESS {
            for ch in 0..num_channels {
                let value = min_clamped + num_channels as f32 - 1.0 + ch as f32;
                let data = audio_buffer.channel_mut(ch);
                for sample in data[..num_fr].iter_mut() {
                    *sample = value;
                }
            }

            scaler.process(&mut audio_buffer);
            for ch in 0..num_channels {
                let data = audio_buffer.channel(ch);
                for &sample in &data[..num_fr] {
                    assert!(
                        (sample - min_clamped).abs() < 1e-4,
                        "expected {min_clamped}, got {sample}",
                    );
                }
            }
        }
    }
}
