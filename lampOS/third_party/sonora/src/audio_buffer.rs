//! Central audio buffer for the audio processing pipeline.
//!
//! `AudioBuffer` stores audio data in a format suitable for the audio processing
//! module to operate on, handling resampling, downmixing, and band splitting.
//!
//! Ported from `modules/audio_processing/audio_buffer.h/cc`.

use sonora_common_audio::audio_util;
use sonora_common_audio::channel_buffer::ChannelBuffer;
use sonora_common_audio::push_sinc_resampler::PushSincResampler;

use crate::splitting_filter::SplittingFilter;
use crate::stream_config::StreamConfig;

fn num_bands_from_frames_per_channel(num_frames: usize) -> usize {
    match num_frames {
        320 => 2,
        480 => 3,
        _ => 1,
    }
}

/// Audio buffer for the processing pipeline.
///
/// Handles resampling between input/buffer/output rates, downmixing from
/// multi-channel to mono, and frequency band splitting for sub-band processing.
#[derive(Debug)]
pub(crate) struct AudioBuffer {
    input_num_frames: usize,
    input_num_channels: usize,
    buffer_num_frames: usize,
    buffer_num_channels: usize,
    output_num_frames: usize,

    num_channels: usize,
    num_bands: usize,
    num_split_frames: usize,

    data: ChannelBuffer<f32>,
    split_data: Option<ChannelBuffer<f32>>,
    splitting_filter: Option<SplittingFilter>,
    input_resamplers: Vec<PushSincResampler>,
    output_resamplers: Vec<PushSincResampler>,
    downmix_by_averaging: bool,
    channel_for_downmixing: usize,
}

impl AudioBuffer {
    /// Create a new audio buffer.
    ///
    /// - `input_rate`: sample rate of incoming audio
    /// - `input_num_channels`: number of input channels
    /// - `buffer_rate`: internal processing sample rate
    /// - `buffer_num_channels`: number of internal processing channels
    /// - `output_rate`: sample rate of outgoing audio
    pub(crate) fn new(
        input_rate: usize,
        input_num_channels: usize,
        buffer_rate: usize,
        buffer_num_channels: usize,
        output_rate: usize,
    ) -> Self {
        let input_num_frames = input_rate / 100;
        let buffer_num_frames = buffer_rate / 100;
        let output_num_frames = output_rate / 100;
        let num_bands = num_bands_from_frames_per_channel(buffer_num_frames);
        let num_split_frames = buffer_num_frames / num_bands;

        debug_assert!(input_num_frames > 0);
        debug_assert!(buffer_num_frames > 0);
        debug_assert!(output_num_frames > 0);
        debug_assert!(input_num_channels > 0);
        debug_assert!(buffer_num_channels > 0);
        debug_assert!(buffer_num_channels <= input_num_channels);

        let data = ChannelBuffer::new(buffer_num_frames, buffer_num_channels, 1);

        let input_resamplers = if input_num_frames != buffer_num_frames {
            (0..buffer_num_channels)
                .map(|_| PushSincResampler::new(input_num_frames, buffer_num_frames))
                .collect()
        } else {
            Vec::new()
        };

        let output_resamplers = if output_num_frames != buffer_num_frames {
            (0..buffer_num_channels)
                .map(|_| PushSincResampler::new(buffer_num_frames, output_num_frames))
                .collect()
        } else {
            Vec::new()
        };

        let (split_data, splitting_filter) = if num_bands > 1 {
            (
                Some(ChannelBuffer::new(
                    buffer_num_frames,
                    buffer_num_channels,
                    num_bands,
                )),
                Some(SplittingFilter::new(buffer_num_channels, num_bands)),
            )
        } else {
            (None, None)
        };

        Self {
            input_num_frames,
            input_num_channels,
            buffer_num_frames,
            buffer_num_channels,
            output_num_frames,
            num_channels: buffer_num_channels,
            num_bands,
            num_split_frames,
            data,
            split_data,
            splitting_filter,
            input_resamplers,
            output_resamplers,
            downmix_by_averaging: true,
            channel_for_downmixing: 0,
        }
    }

    /// Number of visible channels (may be less than buffer_num_channels).
    #[inline]
    pub(crate) fn num_channels(&self) -> usize {
        self.num_channels
    }

    /// Number of frames in the buffer (at buffer rate).
    #[inline]
    pub(crate) fn num_frames(&self) -> usize {
        self.buffer_num_frames
    }

    /// Number of frames per frequency band.
    #[inline]
    pub(crate) fn num_frames_per_band(&self) -> usize {
        self.num_split_frames
    }

    /// Number of frequency bands.
    #[inline]
    pub(crate) fn num_bands(&self) -> usize {
        self.num_bands
    }

    /// Get a slice for a specific channel (full-band).
    #[inline]
    pub(crate) fn channel(&self, channel: usize) -> &[f32] {
        self.data.bands(channel)
    }

    /// Get a mutable slice for a specific channel (full-band).
    #[inline]
    pub(crate) fn channel_mut(&mut self, channel: usize) -> &mut [f32] {
        self.data.bands_mut(channel)
    }

    /// Get split band data for a specific channel and band.
    pub(crate) fn split_band(&self, channel: usize, band: usize) -> &[f32] {
        if let Some(ref split) = self.split_data {
            split.channel(band, channel)
        } else {
            self.data.channel(band, channel)
        }
    }

    /// Get mutable split band data for a specific channel and band.
    pub(crate) fn split_band_mut(&mut self, channel: usize, band: usize) -> &mut [f32] {
        if let Some(ref mut split) = self.split_data {
            split.channel_mut(band, channel)
        } else {
            self.data.channel_mut(band, channel)
        }
    }

    /// Set downmixing mode to select a specific channel.
    pub(crate) fn set_downmixing_to_specific_channel(&mut self, channel: usize) {
        self.downmix_by_averaging = false;
        debug_assert!(self.input_num_channels > channel);
        self.channel_for_downmixing = channel.min(self.input_num_channels - 1);
    }

    /// Set downmixing mode to average all channels.
    pub(crate) fn set_downmixing_by_averaging(&mut self) {
        self.downmix_by_averaging = true;
    }

    /// Set the number of visible channels.
    pub(crate) fn set_num_channels(&mut self, num_channels: usize) {
        debug_assert!(self.buffer_num_channels >= num_channels);
        self.num_channels = num_channels;
        self.data.set_num_channels(num_channels);
        if let Some(ref mut split) = self.split_data {
            split.set_num_channels(num_channels);
        }
    }

    /// Restore the number of channels to the buffer's allocated count.
    pub(crate) fn restore_num_channels(&mut self) {
        self.num_channels = self.buffer_num_channels;
        self.data.set_num_channels(self.buffer_num_channels);
        if let Some(ref mut split) = self.split_data {
            split.set_num_channels(self.buffer_num_channels);
        }
    }

    /// Copy deinterleaved float data into the buffer.
    ///
    /// The input is in float [-1, 1] range and is converted to FloatS16 internally.
    pub(crate) fn copy_from_float(
        &mut self,
        stacked_data: &[&[f32]],
        stream_config: &StreamConfig,
    ) {
        debug_assert_eq!(stream_config.num_frames(), self.input_num_frames);
        debug_assert_eq!(
            stream_config.num_channels() as usize,
            self.input_num_channels
        );
        self.restore_num_channels();

        let downmix_needed = self.input_num_channels > 1 && self.num_channels == 1;
        let resampling_needed = self.input_num_frames != self.buffer_num_frames;

        if downmix_needed {
            let mut downmix = vec![0.0f32; self.input_num_frames];

            if self.downmix_by_averaging {
                let scale = 1.0 / self.input_num_channels as f32;
                for (i, d) in downmix.iter_mut().enumerate() {
                    let mut value = stacked_data[0][i];
                    for sd in &stacked_data[1..self.input_num_channels] {
                        value += sd[i];
                    }
                    *d = value * scale;
                }
            } else {
                downmix.copy_from_slice(
                    &stacked_data[self.channel_for_downmixing][..self.input_num_frames],
                );
            }

            let downmixed_data = if self.downmix_by_averaging {
                &downmix
            } else {
                &stacked_data[self.channel_for_downmixing][..self.input_num_frames]
            };

            if resampling_needed {
                let ch0 = self.data.bands_mut(0);
                self.input_resamplers[0].resample(downmixed_data, ch0);
                // Convert in-place.
                audio_util::float_to_float_s16_slice_inplace(ch0);
            } else {
                let ch0 = self.data.bands_mut(0);
                audio_util::float_to_float_s16_slice(downmixed_data, ch0);
            }
        } else if resampling_needed {
            for (i, sd) in stacked_data.iter().enumerate().take(self.num_channels) {
                let ch = self.data.bands_mut(i);
                self.input_resamplers[i].resample(&sd[..self.input_num_frames], ch);
                audio_util::float_to_float_s16_slice_inplace(ch);
            }
        } else {
            for (i, sd) in stacked_data.iter().enumerate().take(self.num_channels) {
                let ch = self.data.bands_mut(i);
                audio_util::float_to_float_s16_slice(&sd[..self.input_num_frames], ch);
            }
        }
    }

    /// Copy data from the buffer to deinterleaved float output.
    ///
    /// Data is converted from FloatS16 back to float [-1, 1] range.
    pub(crate) fn copy_to_float(
        &mut self,
        stream_config: &StreamConfig,
        stacked_data: &mut [&mut [f32]],
    ) {
        debug_assert_eq!(stream_config.num_frames(), self.output_num_frames);

        let resampling_needed = self.output_num_frames != self.buffer_num_frames;

        if resampling_needed {
            for (i, sd) in stacked_data.iter_mut().enumerate().take(self.num_channels) {
                // Convert from FloatS16 to float first, then resample.
                let ch = self.data.bands_mut(i);
                audio_util::float_s16_to_float_slice_inplace(ch);
                self.output_resamplers[i].resample(ch, &mut sd[..self.output_num_frames]);
            }
        } else {
            for (i, sd) in stacked_data.iter_mut().enumerate().take(self.num_channels) {
                let ch = self.data.bands(i);
                audio_util::float_s16_to_float_slice(ch, &mut sd[..self.output_num_frames]);
            }
        }

        // Copy channel 0 to any extra output channels.
        if self.num_channels < stream_config.num_channels() as usize {
            let (first, rest) = stacked_data.split_at_mut(1);
            for i in self.num_channels..stream_config.num_channels() as usize {
                rest[i - 1][..self.output_num_frames]
                    .copy_from_slice(&first[0][..self.output_num_frames]);
            }
        }
    }

    /// Copy data from this buffer to another AudioBuffer (with optional resampling).
    pub(crate) fn copy_to_buffer(&mut self, buffer: &mut Self) {
        debug_assert_eq!(buffer.num_frames(), self.output_num_frames);

        let resampling_needed = self.output_num_frames != self.buffer_num_frames;

        let buf_frames = buffer.num_frames();
        if resampling_needed {
            for i in 0..self.num_channels {
                let src = self.data.bands(i);
                let mut temp = vec![0.0f32; buf_frames];
                self.output_resamplers[i].resample(src, &mut temp);
                buffer.channel_mut(i)[..buf_frames].copy_from_slice(&temp);
            }
        } else {
            for i in 0..self.num_channels {
                let len = self.buffer_num_frames;
                buffer.channel_mut(i)[..len].copy_from_slice(&self.data.bands(i)[..len]);
            }
        }

        // Copy channel 0 to extra channels in destination.
        let num_frames = buffer.data.num_frames();
        let len = self.output_num_frames;
        for i in self.num_channels..buffer.num_channels() {
            let src_start = 0; // channel 0
            let dst_start = i * num_frames;
            buffer
                .data
                .data_mut()
                .copy_within(src_start..src_start + len, dst_start);
        }
    }

    /// Copy interleaved i16 data into the buffer.
    pub(crate) fn copy_from_interleaved_i16(
        &mut self,
        interleaved_data: &[i16],
        stream_config: &StreamConfig,
    ) {
        debug_assert_eq!(
            stream_config.num_channels() as usize,
            self.input_num_channels
        );
        debug_assert_eq!(stream_config.num_frames(), self.input_num_frames);
        self.restore_num_channels();

        let resampling_required = self.input_num_frames != self.buffer_num_frames;

        if self.num_channels == 1 {
            if self.input_num_channels == 1 {
                // Mono to mono.
                if resampling_required {
                    let mut float_buffer = vec![0.0f32; self.input_num_frames];
                    audio_util::s16_to_float_s16_slice(
                        &interleaved_data[..self.input_num_frames],
                        &mut float_buffer,
                    );
                    let ch0 = self.data.bands_mut(0);
                    self.input_resamplers[0].resample(&float_buffer, ch0);
                } else {
                    let ch0 = self.data.bands_mut(0);
                    audio_util::s16_to_float_s16_slice(
                        &interleaved_data[..self.input_num_frames],
                        ch0,
                    );
                }
            } else {
                // Multi-channel to mono (downmix).
                let mut downmixed = vec![0.0f32; self.input_num_frames];

                if self.downmix_by_averaging {
                    for (j, sample) in downmixed.iter_mut().enumerate().take(self.input_num_frames)
                    {
                        let mut sum: i32 = 0;
                        for ch in 0..self.input_num_channels {
                            sum += interleaved_data[j * self.input_num_channels + ch] as i32;
                        }
                        *sample = sum as f32 / self.input_num_channels as f32;
                    }
                } else {
                    for j in 0..self.input_num_frames {
                        downmixed[j] = interleaved_data
                            [j * self.input_num_channels + self.channel_for_downmixing]
                            as f32;
                    }
                }

                if resampling_required {
                    let ch0 = self.data.bands_mut(0);
                    self.input_resamplers[0].resample(&downmixed, ch0);
                } else {
                    self.data.bands_mut(0)[..self.input_num_frames].copy_from_slice(&downmixed);
                }
            }
        } else {
            // Multi-channel, deinterleave.
            if resampling_required {
                let mut float_buffer = vec![0.0f32; self.input_num_frames];
                for i in 0..self.num_channels {
                    // Deinterleave channel i.
                    for (j, sample) in float_buffer
                        .iter_mut()
                        .enumerate()
                        .take(self.input_num_frames)
                    {
                        *sample = interleaved_data[j * self.input_num_channels + i] as f32;
                    }
                    let ch = self.data.bands_mut(i);
                    self.input_resamplers[i].resample(&float_buffer, ch);
                }
            } else {
                for i in 0..self.num_channels {
                    let ch = self.data.bands_mut(i);
                    for j in 0..self.input_num_frames {
                        ch[j] = interleaved_data[j * self.input_num_channels + i] as f32;
                    }
                }
            }
        }
    }

    /// Copy data from the buffer to interleaved i16 output.
    pub(crate) fn copy_to_interleaved_i16(
        &mut self,
        stream_config: &StreamConfig,
        interleaved_data: &mut [i16],
    ) {
        let config_num_channels = stream_config.num_channels() as usize;
        debug_assert!(config_num_channels == self.num_channels || self.num_channels == 1);
        debug_assert_eq!(stream_config.num_frames(), self.output_num_frames);

        let resampling_required = self.buffer_num_frames != self.output_num_frames;

        if self.num_channels == 1 {
            let mut float_buffer = vec![0.0f32; self.output_num_frames];

            if resampling_required {
                let ch0 = self.data.bands(0);
                self.output_resamplers[0].resample(ch0, &mut float_buffer);
            } else {
                float_buffer.copy_from_slice(&self.data.bands(0)[..self.output_num_frames]);
            }

            if config_num_channels == 1 {
                for (dst, &src) in interleaved_data.iter_mut().zip(float_buffer.iter()) {
                    *dst = audio_util::float_s16_to_s16(src);
                }
            } else {
                let mut k = 0;
                for &fb in &float_buffer[..self.output_num_frames] {
                    let tmp = audio_util::float_s16_to_s16(fb);
                    for _ in 0..config_num_channels {
                        interleaved_data[k] = tmp;
                        k += 1;
                    }
                }
            }
        } else {
            if resampling_required {
                for i in 0..self.num_channels {
                    let mut float_buffer = vec![0.0f32; self.output_num_frames];
                    let ch = self.data.bands(i);
                    self.output_resamplers[i].resample(ch, &mut float_buffer);
                    for (k, sample) in float_buffer.iter().enumerate().take(self.output_num_frames)
                    {
                        interleaved_data[k * config_num_channels + i] =
                            audio_util::float_s16_to_s16(*sample);
                    }
                }
            } else {
                for i in 0..self.num_channels {
                    let ch = self.data.bands(i);
                    for j in 0..self.output_num_frames {
                        interleaved_data[j * config_num_channels + i] =
                            audio_util::float_s16_to_s16(ch[j]);
                    }
                }
            }

            // Copy extra channels from channel 0.
            for i in self.num_channels..config_num_channels {
                for j in 0..self.output_num_frames {
                    interleaved_data[j * config_num_channels + i] =
                        interleaved_data[j * config_num_channels + (i % self.num_channels)];
                }
            }
        }
    }

    /// Split the buffer data into frequency bands.
    pub(crate) fn split_into_frequency_bands(&mut self) {
        if let (Some(filter), Some(split)) = (&mut self.splitting_filter, &mut self.split_data) {
            filter.analysis(&self.data, split);
        }
    }

    /// Recombine frequency bands into full-band signal.
    pub(crate) fn merge_frequency_bands(&mut self) {
        if let (Some(filter), Some(split)) = (&mut self.splitting_filter, &mut self.split_data) {
            filter.synthesis(split, &mut self.data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_without_resampling() {
        let mut ab1 = AudioBuffer::new(32000, 2, 32000, 2, 32000);
        let mut ab2 = AudioBuffer::new(32000, 2, 32000, 2, 32000);

        // Fill first buffer.
        let num_ch = ab1.num_channels();
        let num_fr = ab1.num_frames();
        for ch in 0..num_ch {
            let channel = ab1.channel_mut(ch);
            for (i, sample) in channel.iter_mut().enumerate().take(num_fr) {
                *sample = (i + ch) as f32;
            }
        }

        // Copy to second buffer.
        ab1.copy_to_buffer(&mut ab2);

        // Verify content.
        for ch in 0..ab2.num_channels() {
            let channel = ab2.channel(ch);
            for (i, &sample) in channel.iter().enumerate() {
                assert_eq!(sample, (i + ch) as f32);
            }
        }
    }

    #[test]
    fn copy_with_resampling() {
        let mut ab1 = AudioBuffer::new(32000, 2, 32000, 2, 48000);
        let mut ab2 = AudioBuffer::new(48000, 2, 48000, 2, 48000);

        use std::f32::consts::PI;

        let mut energy_ab1 = 0.0f32;
        let num_ch = ab1.num_channels();
        let num_fr = ab1.num_frames();

        // Put a sine and compute energy of first buffer.
        for ch in 0..num_ch {
            let channel = ab1.channel_mut(ch);
            for (i, sample) in channel.iter_mut().enumerate().take(num_fr) {
                *sample = (2.0 * PI * 100.0 / 32000.0 * i as f32).sin();
                energy_ab1 += *sample * *sample;
            }
        }

        // Copy to second buffer (resamples 32k → 48k).
        ab1.copy_to_buffer(&mut ab2);

        // Compute energy of second buffer.
        let mut energy_ab2 = 0.0f32;
        for ch in 0..ab2.num_channels() {
            let channel = ab2.channel(ch);
            for &sample in channel.iter() {
                energy_ab2 += sample * sample;
            }
        }

        // Verify energies match (accounting for rate difference).
        let expected = energy_ab2 * 32000.0 / 48000.0;
        assert!(
            (energy_ab1 - expected).abs() < 0.01 * energy_ab1,
            "energy mismatch: ab1={energy_ab1}, ab2_scaled={expected}",
        );
    }

    #[test]
    fn band_splitting() {
        let mut ab = AudioBuffer::new(48000, 1, 48000, 1, 48000);
        assert_eq!(ab.num_bands(), 3);
        assert_eq!(ab.num_frames_per_band(), 160);

        // Fill with a signal.
        let channel = ab.channel_mut(0);
        for (i, sample) in channel.iter_mut().enumerate() {
            *sample = (i as f32 / 48.0).sin() * 8192.0;
        }

        ab.split_into_frequency_bands();

        // Band 0 should have some energy.
        let band0 = ab.split_band(0, 0);
        let energy: f32 = band0.iter().map(|x| x * x).sum();
        assert!(energy > 0.0, "band 0 should have energy after splitting");

        ab.merge_frequency_bands();
    }

    #[test]
    fn single_band_no_splitting_filter() {
        let ab = AudioBuffer::new(16000, 1, 16000, 1, 16000);
        assert_eq!(ab.num_bands(), 1);
        assert_eq!(ab.num_frames(), 160);
        assert_eq!(ab.num_frames_per_band(), 160);
    }

    #[test]
    fn two_band_32khz() {
        let ab = AudioBuffer::new(32000, 1, 32000, 1, 32000);
        assert_eq!(ab.num_bands(), 2);
        assert_eq!(ab.num_frames(), 320);
        assert_eq!(ab.num_frames_per_band(), 160);
    }

    #[test]
    fn downmix_by_averaging() {
        let mut ab = AudioBuffer::new(16000, 2, 16000, 1, 16000);
        ab.set_downmixing_by_averaging();

        let ch0 = vec![0.25f32; 160];
        let ch1 = vec![0.75f32; 160];
        let channels: Vec<&[f32]> = vec![&ch0, &ch1];
        let config = StreamConfig::new(16000, 2);

        ab.copy_from_float(&channels, &config);

        // After downmixing: (0.25 + 0.75) / 2 = 0.5 → FloatS16: 0.5 * 32768 = 16384
        let channel = ab.channel(0);
        for &s in channel.iter() {
            let expected = 0.5 * 32768.0;
            assert!((s - expected).abs() < 1.0, "expected ~{expected}, got {s}",);
        }
    }

    #[test]
    fn downmix_specific_channel() {
        let mut ab = AudioBuffer::new(16000, 2, 16000, 1, 16000);
        ab.set_downmixing_to_specific_channel(1);

        let ch0 = vec![0.25f32; 160];
        let ch1 = vec![0.75f32; 160];
        let channels: Vec<&[f32]> = vec![&ch0, &ch1];
        let config = StreamConfig::new(16000, 2);

        ab.copy_from_float(&channels, &config);

        // After selecting channel 1: 0.75 → FloatS16: 0.75 * 32768 = 24576
        let channel = ab.channel(0);
        for &s in channel.iter() {
            let expected = 0.75 * 32768.0;
            assert!((s - expected).abs() < 1.0, "expected ~{expected}, got {s}",);
        }
    }
}
