//! Format conversion (remixing and resampling) for audio.
//!
//! Ported from `common_audio/audio_converter.h/cc`.
//!
//! Only simple remixing conversions are supported: downmix to mono
//! (`dst_channels == 1`) or upmix from mono (`src_channels == 1`).

use sonora_common_audio::channel_buffer::ChannelBuffer;
use sonora_common_audio::push_sinc_resampler::PushSincResampler;

/// Audio format converter supporting remixing and resampling.
///
/// The source and destination chunks have the same duration in time; specifying
/// the number of frames is equivalent to specifying the sample rates.
#[derive(Debug)]
pub(crate) struct AudioConverter {
    src_channels: usize,
    src_frames: usize,
    dst_channels: usize,
    dst_frames: usize,
    kind: ConverterKind,
}

#[derive(Debug)]
enum ConverterKind {
    /// Same channels, same frames — just copy.
    Copy,
    /// Mono → multichannel upmix.
    Upmix,
    /// Multichannel → mono downmix.
    Downmix,
    /// Same channels, different frames — resample.
    Resample { resamplers: Vec<PushSincResampler> },
    /// Composition of two converters with intermediate buffer.
    Composition {
        first: Box<AudioConverter>,
        second: Box<AudioConverter>,
        buffer: ChannelBuffer<f32>,
    },
}

impl AudioConverter {
    /// Creates a new `AudioConverter` for the given format.
    ///
    /// Only supports: same channel count, downmix to mono, or upmix from mono.
    pub(crate) fn new(
        src_channels: usize,
        src_frames: usize,
        dst_channels: usize,
        dst_frames: usize,
    ) -> Self {
        debug_assert!(
            dst_channels == src_channels || dst_channels == 1 || src_channels == 1,
            "Only same channels, downmix to mono, or upmix from mono supported"
        );

        let kind = if src_channels > dst_channels {
            // Downmix (+ optional resample).
            if src_frames != dst_frames {
                let first = Box::new(Self::new_with_kind(
                    src_channels,
                    src_frames,
                    dst_channels,
                    src_frames,
                    ConverterKind::Downmix,
                ));
                let resamplers = (0..dst_channels)
                    .map(|_| PushSincResampler::new(src_frames, dst_frames))
                    .collect();
                let second = Box::new(Self::new_with_kind(
                    dst_channels,
                    src_frames,
                    dst_channels,
                    dst_frames,
                    ConverterKind::Resample { resamplers },
                ));
                let buffer = ChannelBuffer::new_single_band(src_frames, dst_channels);
                ConverterKind::Composition {
                    first,
                    second,
                    buffer,
                }
            } else {
                ConverterKind::Downmix
            }
        } else if src_channels < dst_channels {
            // Upmix (+ optional resample).
            if src_frames != dst_frames {
                let resamplers = (0..src_channels)
                    .map(|_| PushSincResampler::new(src_frames, dst_frames))
                    .collect();
                let first = Box::new(Self::new_with_kind(
                    src_channels,
                    src_frames,
                    src_channels,
                    dst_frames,
                    ConverterKind::Resample { resamplers },
                ));
                let second = Box::new(Self::new_with_kind(
                    src_channels,
                    dst_frames,
                    dst_channels,
                    dst_frames,
                    ConverterKind::Upmix,
                ));
                let buffer = ChannelBuffer::new_single_band(dst_frames, src_channels);
                ConverterKind::Composition {
                    first,
                    second,
                    buffer,
                }
            } else {
                ConverterKind::Upmix
            }
        } else if src_frames != dst_frames {
            // Same channels, different frames — resample only.
            let resamplers = (0..src_channels)
                .map(|_| PushSincResampler::new(src_frames, dst_frames))
                .collect();
            ConverterKind::Resample { resamplers }
        } else {
            // Everything matches — just copy.
            ConverterKind::Copy
        };

        Self {
            src_channels,
            src_frames,
            dst_channels,
            dst_frames,
            kind,
        }
    }

    fn new_with_kind(
        src_channels: usize,
        src_frames: usize,
        dst_channels: usize,
        dst_frames: usize,
        kind: ConverterKind,
    ) -> Self {
        Self {
            src_channels,
            src_frames,
            dst_channels,
            dst_frames,
            kind,
        }
    }

    /// Converts `src` to `dst`.
    ///
    /// `src` contains `src_channels` slices of `src_frames` samples each.
    /// `dst` contains `dst_channels` slices of `dst_frames` samples each.
    pub(crate) fn convert(&mut self, src: &[&[f32]], dst: &mut [&mut [f32]]) {
        debug_assert_eq!(src.len(), self.src_channels);
        debug_assert_eq!(dst.len(), self.dst_channels);
        for s in src.iter() {
            debug_assert_eq!(s.len(), self.src_frames);
        }
        for d in dst.iter() {
            debug_assert_eq!(d.len(), self.dst_frames);
        }

        match &mut self.kind {
            ConverterKind::Copy => {
                for (i, src_ch) in src.iter().enumerate() {
                    dst[i].copy_from_slice(src_ch);
                }
            }
            ConverterKind::Upmix => {
                for i in 0..self.dst_frames {
                    let value = src[0][i];
                    for dst_ch in dst.iter_mut() {
                        dst_ch[i] = value;
                    }
                }
            }
            ConverterKind::Downmix => {
                let scale = 1.0 / self.src_channels as f32;
                for i in 0..self.src_frames {
                    let mut sum = 0.0_f32;
                    for src_ch in src.iter() {
                        sum += src_ch[i];
                    }
                    dst[0][i] = sum * scale;
                }
            }
            ConverterKind::Resample { resamplers } => {
                for (i, resampler) in resamplers.iter_mut().enumerate() {
                    resampler.resample(src[i], dst[i]);
                }
            }
            ConverterKind::Composition {
                first,
                second,
                buffer,
            } => {
                let first_dst_channels = first.dst_channels;
                let first_dst_frames = first.dst_frames;
                let frames_per_ch = buffer.num_frames();

                // First stage: convert into intermediate buffer.
                {
                    let data = buffer.bands_mut(0);
                    let mut buf_slices: Vec<&mut [f32]> = Vec::with_capacity(first_dst_channels);
                    let mut remainder = &mut data[..];
                    for _ in 0..first_dst_channels {
                        let (chunk, rest) = remainder.split_at_mut(frames_per_ch);
                        buf_slices.push(&mut chunk[..first_dst_frames]);
                        remainder = rest;
                    }
                    first.convert(src, &mut buf_slices);
                }

                // Second stage: convert from intermediate buffer to dst.
                {
                    let data = buffer.bands(0);
                    let buf_refs: Vec<&[f32]> = (0..first_dst_channels)
                        .map(|ch| {
                            let start = ch * frames_per_ch;
                            &data[start..start + first_dst_frames]
                        })
                        .collect();
                    second.convert(&buf_refs, dst);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_buffer(data: &[f32], frames: usize) -> ChannelBuffer<f32> {
        let num_channels = data.len();
        let mut buf = ChannelBuffer::new_single_band(frames, num_channels);
        for (i, &scale) in data.iter().enumerate() {
            for j in 0..frames {
                buf.channel_mut(0, i)[j] = scale * j as f32;
            }
        }
        buf
    }

    fn compute_snr(
        ref_buf: &ChannelBuffer<f32>,
        test_buf: &ChannelBuffer<f32>,
        expected_delay: usize,
    ) -> f32 {
        assert_eq!(ref_buf.num_channels(), test_buf.num_channels());
        assert_eq!(ref_buf.num_frames(), test_buf.num_frames());

        let mut best_snr = 0.0_f32;
        let min_delay = expected_delay.saturating_sub(1);
        let max_delay = (expected_delay + 1).min(ref_buf.num_frames());

        for delay in min_delay..=max_delay {
            let mut mse = 0.0_f32;
            let mut variance = 0.0_f32;
            let mut mean = 0.0_f32;

            for i in 0..ref_buf.num_channels() {
                let ref_ch = ref_buf.channel(0, i);
                let test_ch = test_buf.channel(0, i);
                for j in 0..ref_buf.num_frames() - delay {
                    let error = ref_ch[j] - test_ch[j + delay];
                    mse += error * error;
                    variance += ref_ch[j] * ref_ch[j];
                    mean += ref_ch[j];
                }
            }

            let length = ref_buf.num_channels() * (ref_buf.num_frames() - delay);
            if length == 0 {
                continue;
            }
            let length_f = length as f32;
            mse /= length_f;
            variance /= length_f;
            mean /= length_f;
            variance -= mean * mean;
            let snr = if mse > 0.0 {
                10.0 * (variance / mse).log10()
            } else {
                100.0
            };
            if snr > best_snr {
                best_snr = snr;
            }
        }
        best_snr
    }

    fn run_audio_converter_test(
        src_channels: usize,
        src_sample_rate_hz: usize,
        dst_channels: usize,
        dst_sample_rate_hz: usize,
    ) {
        let src_left = 0.0002_f32;
        let src_right = 0.0001_f32;
        let resampling_factor = src_sample_rate_hz as f32 / dst_sample_rate_hz as f32;
        let dst_left = resampling_factor * src_left;
        let dst_right = resampling_factor * src_right;
        let dst_mono = (dst_left + dst_right) / 2.0;
        let src_frames = src_sample_rate_hz / 100;
        let dst_frames = dst_sample_rate_hz / 100;

        let mut src_data = vec![src_left];
        if src_channels == 2 {
            src_data.push(src_right);
        }
        let src_buffer = create_buffer(&src_data, src_frames);

        let ref_data = if dst_channels == 1 {
            if src_channels == 1 {
                vec![dst_left]
            } else {
                vec![dst_mono]
            }
        } else if src_channels == 1 {
            vec![dst_left, dst_left]
        } else {
            vec![dst_left, dst_right]
        };

        let mut dst_buffer = create_buffer(&vec![0.0; dst_channels], dst_frames);
        let ref_buffer = create_buffer(&ref_data, dst_frames);

        let delay_frames = if src_sample_rate_hz == dst_sample_rate_hz {
            0
        } else {
            (PushSincResampler::algorithmic_delay_seconds(src_sample_rate_hz as u32)
                * dst_sample_rate_hz as f32) as usize
        };

        let mut converter = AudioConverter::new(src_channels, src_frames, dst_channels, dst_frames);

        let src_refs: Vec<&[f32]> = (0..src_channels)
            .map(|ch| src_buffer.channel(0, ch))
            .collect();

        // Extract dst channels into temporary vecs, convert, then copy back.
        let mut dst_vecs: Vec<Vec<f32>> = (0..dst_channels)
            .map(|ch| dst_buffer.channel(0, ch).to_vec())
            .collect();
        {
            let mut dst_slices: Vec<&mut [f32]> =
                dst_vecs.iter_mut().map(|v| v.as_mut_slice()).collect();
            converter.convert(&src_refs, &mut dst_slices);
        }
        for (ch, vec) in dst_vecs.iter().enumerate() {
            dst_buffer.channel_mut(0, ch).copy_from_slice(vec);
        }

        let snr = compute_snr(&ref_buffer, &dst_buffer, delay_frames);
        assert!(
            snr > 43.0,
            "SNR={snr:.1} dB too low for ({src_channels}, {src_sample_rate_hz} Hz) -> ({dst_channels}, {dst_sample_rate_hz} Hz)"
        );
    }

    #[test]
    fn conversions_pass_snr_threshold() {
        let sample_rates = [8000, 11025, 16000, 22050, 32000, 44100, 48000];
        let channels = [1, 2];
        for &src_rate in &sample_rates {
            for &dst_rate in &sample_rates {
                for &src_ch in &channels {
                    for &dst_ch in &channels {
                        run_audio_converter_test(src_ch, src_rate, dst_ch, dst_rate);
                    }
                }
            }
        }
    }

    #[test]
    fn copy_preserves_signal() {
        let mut converter = AudioConverter::new(1, 160, 1, 160);
        let src = vec![1.0_f32; 160];
        let mut dst = vec![0.0_f32; 160];
        converter.convert(&[&src], &mut [&mut dst]);
        assert_eq!(src, dst);
    }

    #[test]
    fn upmix_duplicates_mono() {
        let mut converter = AudioConverter::new(1, 160, 2, 160);
        let src: Vec<f32> = (0..160).map(|i| i as f32).collect();
        let mut dst0 = vec![0.0_f32; 160];
        let mut dst1 = vec![0.0_f32; 160];
        converter.convert(&[&src], &mut [&mut dst0, &mut dst1]);
        assert_eq!(src, dst0);
        assert_eq!(src, dst1);
    }

    #[test]
    fn downmix_averages_channels() {
        let mut converter = AudioConverter::new(2, 160, 1, 160);
        let src0: Vec<f32> = (0..160).map(|i| i as f32 * 2.0).collect();
        let src1: Vec<f32> = (0..160).map(|i| i as f32).collect();
        let mut dst = vec![0.0_f32; 160];
        converter.convert(&[&src0, &src1], &mut [&mut dst]);
        for i in 0..160 {
            let expected = (src0[i] + src1[i]) / 2.0;
            assert!(
                (dst[i] - expected).abs() < 1e-6,
                "sample {i}: expected {expected}, got {}",
                dst[i]
            );
        }
    }
}
