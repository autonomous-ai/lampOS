//! Capture levels adjuster — pre and post gain with emulated analog mic gain.
//!
//! Ported from `modules/audio_processing/capture_levels_adjuster/capture_levels_adjuster.h/cc`.

use crate::audio_buffer::AudioBuffer;
use crate::audio_samples_scaler::AudioSamplesScaler;

const MIN_ANALOG_MIC_GAIN_LEVEL: i32 = 0;
const MAX_ANALOG_MIC_GAIN_LEVEL: i32 = 255;

fn compute_level_based_gain(emulated_analog_mic_gain_level: i32) -> f32 {
    debug_assert!(emulated_analog_mic_gain_level >= MIN_ANALOG_MIC_GAIN_LEVEL);
    debug_assert!(emulated_analog_mic_gain_level <= MAX_ANALOG_MIC_GAIN_LEVEL);
    emulated_analog_mic_gain_level as f32 / MAX_ANALOG_MIC_GAIN_LEVEL as f32
}

fn compute_pre_gain(
    pre_gain: f32,
    emulated_analog_mic_gain_level: i32,
    emulated_analog_mic_gain_enabled: bool,
) -> f32 {
    if emulated_analog_mic_gain_enabled {
        pre_gain * compute_level_based_gain(emulated_analog_mic_gain_level)
    } else {
        pre_gain
    }
}

/// Adjusts capture signal levels with pre-gain, emulated analog mic gain, and post-gain.
#[derive(Debug)]
pub(crate) struct CaptureLevelsAdjuster {
    emulated_analog_mic_gain_enabled: bool,
    emulated_analog_mic_gain_level: i32,
    pre_gain: f32,
    pre_adjustment_gain: f32,
    pre_scaler: AudioSamplesScaler,
    post_scaler: AudioSamplesScaler,
}

impl CaptureLevelsAdjuster {
    pub(crate) fn new(
        emulated_analog_mic_gain_enabled: bool,
        emulated_analog_mic_gain_level: i32,
        pre_gain: f32,
        post_gain: f32,
    ) -> Self {
        let pre_adjustment_gain = compute_pre_gain(
            pre_gain,
            emulated_analog_mic_gain_level,
            emulated_analog_mic_gain_enabled,
        );
        Self {
            emulated_analog_mic_gain_enabled,
            emulated_analog_mic_gain_level,
            pre_gain,
            pre_adjustment_gain,
            pre_scaler: AudioSamplesScaler::new(pre_adjustment_gain),
            post_scaler: AudioSamplesScaler::new(post_gain),
        }
    }

    pub(crate) fn apply_pre_level_adjustment(&mut self, audio_buffer: &mut AudioBuffer) {
        self.pre_scaler.process(audio_buffer);
    }

    pub(crate) fn apply_post_level_adjustment(&mut self, audio_buffer: &mut AudioBuffer) {
        self.post_scaler.process(audio_buffer);
    }

    pub(crate) fn set_pre_gain(&mut self, pre_gain: f32) {
        self.pre_gain = pre_gain;
        self.update_pre_adjustment_gain();
    }

    pub(crate) fn get_pre_adjustment_gain(&self) -> f32 {
        self.pre_adjustment_gain
    }

    pub(crate) fn set_post_gain(&mut self, post_gain: f32) {
        self.post_scaler.set_gain(post_gain);
    }

    pub(crate) fn set_analog_mic_gain_level(&mut self, level: i32) {
        debug_assert!(level >= MIN_ANALOG_MIC_GAIN_LEVEL);
        debug_assert!(level <= MAX_ANALOG_MIC_GAIN_LEVEL);
        let clamped = level.clamp(MIN_ANALOG_MIC_GAIN_LEVEL, MAX_ANALOG_MIC_GAIN_LEVEL);
        self.emulated_analog_mic_gain_level = clamped;
        self.update_pre_adjustment_gain();
    }

    pub(crate) fn get_analog_mic_gain_level(&self) -> i32 {
        self.emulated_analog_mic_gain_level
    }

    fn update_pre_adjustment_gain(&mut self) {
        self.pre_adjustment_gain = compute_pre_gain(
            self.pre_gain,
            self.emulated_analog_mic_gain_level,
            self.emulated_analog_mic_gain_enabled,
        );
        self.pre_scaler.set_gain(self.pre_adjustment_gain);
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

    fn compute_expected_pre_gain(
        emulated_enabled: bool,
        emulated_level: i32,
        pre_gain: f32,
    ) -> f32 {
        if !emulated_enabled {
            return pre_gain;
        }
        pre_gain * emulated_level.min(255) as f32 / 255.0
    }

    fn compute_expected_post_gain(
        emulated_enabled: bool,
        emulated_level: i32,
        pre_gain: f32,
        post_gain: f32,
    ) -> f32 {
        post_gain * compute_expected_pre_gain(emulated_enabled, emulated_level, pre_gain)
    }

    const NUM_FRAMES_TO_PROCESS: usize = 10;
    const MIN_FLOAT_S16_VALUE: f32 = -32768.0;
    const MAX_FLOAT_S16_VALUE: f32 = 32767.0;

    fn test_params() -> Vec<(usize, usize, bool, i32, f32, f32)> {
        let mut params = Vec::new();
        for &rate in &[16000, 32000, 48000] {
            for &channels in &[1, 2, 4] {
                for &enabled in &[false, true] {
                    for &level in &[21, 255] {
                        for &pre in &[0.1, 1.0, 4.0] {
                            for &post in &[0.1, 1.0, 4.0] {
                                params.push((rate, channels, enabled, level, pre, post));
                            }
                        }
                    }
                }
            }
        }
        params
    }

    #[test]
    fn initial_gain_is_instantly_achieved() {
        for (sample_rate_hz, num_channels, emulated_enabled, emulated_level, pre_gain, post_gain) in
            test_params()
        {
            let mut adjuster =
                CaptureLevelsAdjuster::new(emulated_enabled, emulated_level, pre_gain, post_gain);

            let mut audio_buffer = AudioBuffer::new(
                sample_rate_hz,
                num_channels,
                sample_rate_hz,
                num_channels,
                sample_rate_hz,
            );

            let expected_pre =
                compute_expected_pre_gain(emulated_enabled, emulated_level, pre_gain);
            let expected_post =
                compute_expected_post_gain(emulated_enabled, emulated_level, pre_gain, post_gain);

            for _ in 0..NUM_FRAMES_TO_PROCESS {
                populate_buffer(&mut audio_buffer);
                adjuster.apply_pre_level_adjustment(&mut audio_buffer);

                assert!(
                    (adjuster.get_pre_adjustment_gain() - expected_pre).abs() < 1e-6,
                    "pre adjustment gain mismatch",
                );

                let num_fr = audio_buffer.num_frames();
                for ch in 0..num_channels {
                    let expected_val = (expected_pre * sample_value_for_channel(ch))
                        .clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
                    let data = audio_buffer.channel(ch);
                    for &sample in &data[..num_fr] {
                        assert!(
                            (sample - expected_val).abs() < 1e-3,
                            "rate={sample_rate_hz}, ch={ch}, pre: expected {expected_val}, got {sample}",
                        );
                    }
                }

                adjuster.apply_post_level_adjustment(&mut audio_buffer);
                let num_fr = audio_buffer.num_frames();
                for ch in 0..num_channels {
                    let expected_val = (expected_post * sample_value_for_channel(ch))
                        .clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
                    let data = audio_buffer.channel(ch);
                    for &sample in &data[..num_fr] {
                        assert!(
                            (sample - expected_val).abs() < 1e-2,
                            "rate={sample_rate_hz}, ch={ch}, post: expected {expected_val}, got {sample}",
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn new_gains_are_achieved() {
        for (sample_rate_hz, num_channels, emulated_enabled, emulated_level, pre_gain, post_gain) in
            test_params()
        {
            let lower_emulated_level = emulated_level;
            let lower_pre_gain = pre_gain;
            let lower_post_gain = post_gain;
            let higher_emulated_level = (lower_emulated_level * 2).min(255);
            let higher_pre_gain = lower_pre_gain * 2.0;
            let higher_post_gain = lower_post_gain * 2.0;

            let mut adjuster = CaptureLevelsAdjuster::new(
                emulated_enabled,
                lower_emulated_level,
                lower_pre_gain,
                lower_post_gain,
            );

            let mut audio_buffer = AudioBuffer::new(
                sample_rate_hz,
                num_channels,
                sample_rate_hz,
                num_channels,
                sample_rate_hz,
            );

            let expected_pre =
                compute_expected_pre_gain(emulated_enabled, higher_emulated_level, higher_pre_gain);
            let expected_post = compute_expected_post_gain(
                emulated_enabled,
                higher_emulated_level,
                higher_pre_gain,
                higher_post_gain,
            );

            adjuster.set_pre_gain(higher_pre_gain);
            adjuster.set_post_gain(higher_post_gain);
            adjuster.set_analog_mic_gain_level(higher_emulated_level);

            // First frame: transition frame.
            populate_buffer(&mut audio_buffer);
            adjuster.apply_pre_level_adjustment(&mut audio_buffer);
            adjuster.apply_post_level_adjustment(&mut audio_buffer);
            assert_eq!(adjuster.get_analog_mic_gain_level(), higher_emulated_level);

            // Subsequent frames: should be at stable new gain.
            for _ in 1..NUM_FRAMES_TO_PROCESS {
                populate_buffer(&mut audio_buffer);
                adjuster.apply_pre_level_adjustment(&mut audio_buffer);

                assert!(
                    (adjuster.get_pre_adjustment_gain() - expected_pre).abs() < 1e-6,
                    "pre adjustment gain mismatch",
                );

                let num_fr = audio_buffer.num_frames();
                for ch in 0..num_channels {
                    let expected_val = (expected_pre * sample_value_for_channel(ch))
                        .clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
                    let data = audio_buffer.channel(ch);
                    for &sample in &data[..num_fr] {
                        assert!(
                            (sample - expected_val).abs() < 1e-3,
                            "rate={sample_rate_hz}, ch={ch}, pre: expected {expected_val}, got {sample}",
                        );
                    }
                }

                adjuster.apply_post_level_adjustment(&mut audio_buffer);
                let num_fr = audio_buffer.num_frames();
                for ch in 0..num_channels {
                    let expected_val = (expected_post * sample_value_for_channel(ch))
                        .clamp(MIN_FLOAT_S16_VALUE, MAX_FLOAT_S16_VALUE);
                    let data = audio_buffer.channel(ch);
                    for &sample in &data[..num_fr] {
                        assert!(
                            (sample - expected_val).abs() < 1e-2,
                            "rate={sample_rate_hz}, ch={ch}, post: expected {expected_val}, got {sample}",
                        );
                    }
                }

                assert_eq!(adjuster.get_analog_mic_gain_level(), higher_emulated_level);
            }
        }
    }
}
