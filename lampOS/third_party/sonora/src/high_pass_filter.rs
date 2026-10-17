//! High-pass filter for removing DC and low-frequency noise.
//!
//! Ported from `modules/audio_processing/high_pass_filter.h/cc`.

use sonora_common_audio::cascaded_biquad_filter::{BiQuadCoefficients, CascadedBiQuadFilter};

use crate::audio_buffer::AudioBuffer;

const HIGH_PASS_FILTER_COEFFICIENTS_16KHZ: [BiQuadCoefficients; 3] = [
    BiQuadCoefficients {
        b: [
            0.8773539420715290582_f32,
            -1.754683920749088077_f32,
            0.8773539420715289472_f32,
        ],
        a: [-1.881687317862849707_f32, 0.8880584644559580410_f32],
    },
    BiQuadCoefficients {
        b: [1.0, -1.999810143464515022_f32, 1.0],
        a: [-1.976035417167170793_f32, 0.9779708644868606582_f32],
    },
    BiQuadCoefficients {
        b: [1.0, -1.999669231394235469_f32, 1.0],
        a: [-1.994265767864654482_f32, 0.9954861594635392441_f32],
    },
];

const HIGH_PASS_FILTER_COEFFICIENTS_32KHZ: [BiQuadCoefficients; 3] = [
    BiQuadCoefficients {
        b: [
            0.9102055685511306615_f32,
            -1.820404922871161624_f32,
            0.9102055685511306615_f32,
        ],
        a: [-1.940710875829138482_f32, 0.9423512845457852061_f32],
    },
    BiQuadCoefficients {
        b: [1.0, -1.999952541587768806_f32, 1.0],
        a: [-1.988434609801665420_f32, 0.9889212529819323416_f32],
    },
    BiQuadCoefficients {
        b: [1.0, -1.999917315632020021_f32, 1.0],
        a: [-1.997434723613889629_f32, 0.9977401885079651978_f32],
    },
];

const HIGH_PASS_FILTER_COEFFICIENTS_48KHZ: [BiQuadCoefficients; 3] = [
    BiQuadCoefficients {
        b: [
            0.9213790163564168_f32,
            -1.8427552370064049_f32,
            0.9213790163564168_f32,
        ],
        a: [-1.9604500061078971_f32, 0.9611862979079667_f32],
    },
    BiQuadCoefficients {
        b: [1.0, -1.9999789078432082_f32, 1.0],
        a: [-1.9923834169149972_f32, 0.9926001112941157_f32],
    },
    BiQuadCoefficients {
        b: [1.0, -1.9999632520325810_f32, 1.0],
        a: [-1.9983570340145236_f32, 0.9984928491805198_f32],
    },
];

fn choose_coefficients(sample_rate_hz: i32) -> &'static [BiQuadCoefficients; 3] {
    match sample_rate_hz {
        16000 => &HIGH_PASS_FILTER_COEFFICIENTS_16KHZ,
        32000 => &HIGH_PASS_FILTER_COEFFICIENTS_32KHZ,
        48000 => &HIGH_PASS_FILTER_COEFFICIENTS_48KHZ,
        _ => panic!("unsupported sample rate: {sample_rate_hz}; expected 16000, 32000, or 48000"),
    }
}

/// Per-channel high-pass filter using cascaded biquad sections.
#[derive(Debug)]
pub struct HighPassFilter {
    sample_rate_hz: i32,
    filters: Vec<CascadedBiQuadFilter>,
}

impl HighPassFilter {
    pub fn new(sample_rate_hz: i32, num_channels: usize) -> Self {
        let coefficients = choose_coefficients(sample_rate_hz);
        let filters = (0..num_channels)
            .map(|_| CascadedBiQuadFilter::new(coefficients))
            .collect();
        Self {
            sample_rate_hz,
            filters,
        }
    }

    /// Process audio through the high-pass filter using an AudioBuffer.
    pub(crate) fn process(&mut self, audio: &mut AudioBuffer, use_split_band_data: bool) {
        debug_assert_eq!(self.filters.len(), audio.num_channels());
        if use_split_band_data {
            for k in 0..audio.num_channels() {
                let data = audio.split_band_mut(k, 0);
                self.filters[k].process_in_place(data);
            }
        } else {
            for k in 0..audio.num_channels() {
                let data = audio.channel_mut(k);
                self.filters[k].process_in_place(data);
            }
        }
    }

    /// Process audio through the high-pass filter using raw channel vectors.
    pub fn process_channels(&mut self, audio: &mut [Vec<f32>]) {
        debug_assert_eq!(self.filters.len(), audio.len());
        for (k, channel) in audio.iter_mut().enumerate() {
            self.filters[k].process_in_place(channel);
        }
    }

    pub(crate) fn sample_rate_hz(&self) -> i32 {
        self.sample_rate_hz
    }

    pub(crate) fn num_channels(&self) -> usize {
        self.filters.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream_config::StreamConfig;

    fn process_one_frame_as_audio_buffer(
        frame_input: &[f32],
        stream_config: &StreamConfig,
        hpf: &mut HighPassFilter,
    ) -> Vec<f32> {
        let rate = stream_config.sample_rate_hz() as usize;
        let ch_count = stream_config.num_channels() as usize;
        let mut audio_buffer = AudioBuffer::new(rate, ch_count, rate, ch_count, rate);

        // CopyVectorToAudioBuffer: deinterleave into per-channel data.
        let frames = stream_config.num_frames();
        for ch in 0..ch_count {
            let channel = audio_buffer.channel_mut(ch);
            for i in 0..frames {
                channel[i] = frame_input[i * ch_count + ch];
            }
        }

        hpf.process(&mut audio_buffer, false);

        // ExtractVectorFromAudioBuffer: read back per-channel data.
        let mut output = vec![0.0f32; frames * ch_count];
        for ch in 0..ch_count {
            let channel = audio_buffer.channel(ch);
            for i in 0..frames {
                output[ch * frames + i] = channel[i];
            }
        }
        output
    }

    fn process_one_frame_as_vector(
        frame_input: &[f32],
        stream_config: &StreamConfig,
        hpf: &mut HighPassFilter,
    ) -> Vec<f32> {
        let frames = stream_config.num_frames();
        let ch_count = stream_config.num_channels() as usize;

        let mut process_vector: Vec<Vec<f32>> = vec![vec![0.0; frames]; ch_count];
        for k in 0..frames {
            for ch in 0..ch_count {
                process_vector[ch][k] = frame_input[k * ch_count + ch];
            }
        }

        hpf.process_channels(&mut process_vector);

        process_vector[0].clone()
    }

    fn run_bitexactness_test(
        num_channels: u16,
        use_audio_buffer_interface: bool,
        input: &[f32],
        reference: &[f32],
    ) {
        let stream_config = StreamConfig::new(16000, num_channels);
        let mut hpf = HighPassFilter::new(16000, num_channels as usize);

        let frames = stream_config.num_frames();
        let nch = num_channels as usize;
        let num_frames_to_process = input.len() / (frames * nch);
        let mut output = Vec::new();

        for frame_no in 0..num_frames_to_process {
            let start = frames * nch * frame_no;
            let end = frames * nch * (frame_no + 1);
            let frame_input = &input[start..end];

            output = if use_audio_buffer_interface {
                process_one_frame_as_audio_buffer(frame_input, &stream_config, &mut hpf)
            } else {
                process_one_frame_as_vector(frame_input, &stream_config, &mut hpf)
            };
        }

        let reference_frame_length = reference.len() / nch;
        let mut output_to_verify = Vec::new();
        for channel_no in 0..nch {
            let start = channel_no * frames;
            let end = start + reference_frame_length;
            output_to_verify.extend_from_slice(&output[start..end]);
        }

        let element_error_bound = 1.0 / 32768.0;
        for (i, (o, r)) in output_to_verify.iter().zip(reference.iter()).enumerate() {
            assert!(
                (o - r).abs() <= element_error_bound,
                "mismatch at index {i}: output={o}, reference={r}, diff={}",
                (o - r).abs(),
            );
        }
    }

    #[test]
    fn mono_initial() {
        #[rustfmt::skip]
        let reference_input: Vec<f32> = vec![
            0.150254, 0.512488, -0.631245, 0.240938, 0.089080, -0.365440,
            -0.121169, 0.095748, 1.000000, 0.773932, -0.377232, 0.848124,
            0.202718, -0.017621, 0.199738, -0.057279, -0.034693, 0.416303,
            0.393761, 0.396041, 0.187653, -0.337438, 0.200436, 0.455577,
            0.136624, 0.289150, 0.203131, -0.084798, 0.082124, -0.220010,
            0.248266, -0.320554, -0.298701, -0.226218, -0.822794, 0.401962,
            0.090876, -0.210968, 0.382936, -0.478291, -0.028572, -0.067474,
            0.089204, 0.087430, -0.241695, -0.008398, -0.046076, 0.175416,
            0.305518, 0.309992, -0.241352, 0.021618, -0.339291, -0.311173,
            -0.001914, 0.428301, -0.215087, 0.103784, -0.063041, 0.312250,
            -0.304344, 0.009098, 0.154406, 0.307571, 0.431537, 0.024014,
            -0.416832, -0.207440, -0.296664, 0.656846, -0.172033, 0.209054,
            -0.053772, 0.248326, -0.213741, -0.391871, -0.397490, 0.136428,
            -0.049568, -0.054788, 0.396633, 0.081485, 0.055279, 0.443690,
            -0.224812, 0.194675, 0.233369, -0.068107, 0.060270, -0.325801,
            -0.320801, 0.029308, 0.201837, 0.722528, -0.186366, 0.052351,
            -0.023053, -0.540192, -0.122671, -0.501532, 0.234847, -0.248165,
            0.027971, -0.152171, 0.084820, -0.167764, 0.136923, 0.206619,
            0.478395, -0.054249, -0.597574, -0.234627, 0.378548, -0.299619,
            0.268543, 0.034666, 0.401492, -0.547983, -0.055248, -0.337538,
            0.812657, 0.230611, 0.385360, -0.295713, -0.130957, -0.076143,
            0.306960, -0.077653, 0.196049, -0.573390, -0.098885, -0.230155,
            -0.440716, 0.141956, 0.078802, 0.009356, -0.372703, 0.315083,
            0.097859, -0.083575, 0.006397, -0.073216, -0.489105, -0.079827,
            -0.232329, -0.273644, -0.323162, -0.149105, -0.559646, 0.269458,
            0.145333, -0.005597, -0.009717, -0.223051, 0.284676, -0.037228,
            -0.199679, 0.377651, -0.062813, -0.164607,
        ];
        #[rustfmt::skip]
        let reference: Vec<f32> = vec![
            0.131826, 0.430194, -0.638357, 0.213868,
            0.049683, -0.358489, -0.094094, 0.111697,
            0.891429, 0.563210, -0.539361, 0.598238,
        ];

        for use_audio_buffer_interface in [true, false] {
            run_bitexactness_test(1, use_audio_buffer_interface, &reference_input, &reference);
        }
    }

    #[test]
    fn mono_converged() {
        #[rustfmt::skip]
        let reference_input: Vec<f32> = vec![
            0.150254, 0.512488, -0.631245, 0.240938, 0.089080, -0.365440,
            -0.121169, 0.095748, 1.000000, 0.773932, -0.377232, 0.848124,
            0.202718, -0.017621, 0.199738, -0.057279, -0.034693, 0.416303,
            0.393761, 0.396041, 0.187653, -0.337438, 0.200436, 0.455577,
            0.136624, 0.289150, 0.203131, -0.084798, 0.082124, -0.220010,
            0.248266, -0.320554, -0.298701, -0.226218, -0.822794, 0.401962,
            0.090876, -0.210968, 0.382936, -0.478291, -0.028572, -0.067474,
            0.089204, 0.087430, -0.241695, -0.008398, -0.046076, 0.175416,
            0.305518, 0.309992, -0.241352, 0.021618, -0.339291, -0.311173,
            -0.001914, 0.428301, -0.215087, 0.103784, -0.063041, 0.312250,
            -0.304344, 0.009098, 0.154406, 0.307571, 0.431537, 0.024014,
            -0.416832, -0.207440, -0.296664, 0.656846, -0.172033, 0.209054,
            -0.053772, 0.248326, -0.213741, -0.391871, -0.397490, 0.136428,
            -0.049568, -0.054788, 0.396633, 0.081485, 0.055279, 0.443690,
            -0.224812, 0.194675, 0.233369, -0.068107, 0.060270, -0.325801,
            -0.320801, 0.029308, 0.201837, 0.722528, -0.186366, 0.052351,
            -0.023053, -0.540192, -0.122671, -0.501532, 0.234847, -0.248165,
            0.027971, -0.152171, 0.084820, -0.167764, 0.136923, 0.206619,
            0.478395, -0.054249, -0.597574, -0.234627, 0.378548, -0.299619,
            0.268543, 0.034666, 0.401492, -0.547983, -0.055248, -0.337538,
            0.812657, 0.230611, 0.385360, -0.295713, -0.130957, -0.076143,
            0.306960, -0.077653, 0.196049, -0.573390, -0.098885, -0.230155,
            -0.440716, 0.141956, 0.078802, 0.009356, -0.372703, 0.315083,
            0.097859, -0.083575, 0.006397, -0.073216, -0.489105, -0.079827,
            -0.232329, -0.273644, -0.323162, -0.149105, -0.559646, 0.269458,
            0.145333, -0.005597, -0.009717, -0.223051, 0.284676, -0.037228,
            -0.199679, 0.377651, -0.062813, -0.164607, -0.082091, -0.236957,
            -0.313025, 0.705903, 0.462637, 0.085942, -0.351308, -0.241859,
            -0.049333, 0.221165, -0.372235, -0.651092, -0.404957, 0.093201,
            0.109366, 0.126224, -0.036409, 0.051333, -0.133063, 0.240896,
            -0.380532, 0.127160, -0.237176, -0.093586, 0.154478, 0.290379,
            -0.312329, 0.352297, 0.184480, -0.018965, -0.054555, -0.060811,
            -0.084705, 0.006440, 0.014333, 0.230847, 0.426721, 0.130481,
            -0.058605, 0.174712, 0.051204, -0.287773, 0.265265, 0.085810,
            0.037775, 0.143988, 0.073051, -0.263103, -0.045366, -0.040816,
            -0.148673, 0.470072, -0.244727, -0.135204, -0.198973, -0.328139,
            -0.053722, -0.076590, 0.427586, -0.069591, -0.297399, 0.448094,
            0.345037, -0.064170, -0.420903, -0.124253, -0.043578, 0.077149,
            -0.072983, 0.123916, 0.109517, -0.349508, -0.264912, -0.207106,
            -0.141912, -0.089586, 0.003485, -0.846518, -0.127715, 0.347208,
            -0.298095, 0.260935, 0.097899, -0.008106, 0.050987, -0.437362,
            -0.023625, 0.448230, 0.027484, 0.011562, -0.205167, -0.008611,
            0.064930, 0.119156, -0.104183, -0.066078, 0.565530, -0.631108,
            0.623029, 0.094334, 0.279472, -0.465059, -0.164888, -0.077706,
            0.118130, -0.466746, 0.131800, -0.338936, 0.018497, 0.182304,
            0.091398, 0.302547, 0.281153, -0.181899, 0.071836, -0.263911,
            -0.369380, 0.258447, 0.000014, -0.015347, 0.254619, 0.166159,
            0.097865, 0.349389, 0.259834, 0.067003, -0.192925, -0.182080,
            0.333139, -0.450434, -0.006836, -0.544615, 0.285183, 0.240811,
            0.000325, -0.019796, -0.694804, 0.162411, -0.612686, -0.648134,
            0.022338, -0.265058, 0.114993, 0.189185, 0.239697, -0.193148,
            0.125581, 0.028122, 0.230849, 0.149832, 0.250919, -0.036871,
            -0.041136, 0.281627, -0.593466, -0.141009, -0.355074, -0.106915,
            0.181276, 0.230753, -0.283631, -0.131643, 0.038292, -0.081563,
            0.084345, 0.111763, -0.259882, -0.049416, -0.595824, 0.320077,
            -0.175802, -0.336422, -0.070966, -0.399242, -0.005829, -0.156680,
            0.608591, 0.318150, -0.697767, 0.123331, -0.390716, -0.071276,
            0.045943, 0.208958, -0.076304, 0.440505, -0.134400, 0.091525,
            0.185763, 0.023806, 0.246186, 0.090323, -0.219133, -0.504520,
            0.519393, -0.168939, 0.028884, 0.157380, 0.031745, -0.252830,
            -0.130705, -0.034901, 0.413302, -0.240559, 0.219279, 0.086246,
            -0.065353, -0.295376, -0.079405, -0.024226, -0.410629, 0.053706,
            -0.229794, -0.026336, 0.093956, -0.252810, -0.080555, 0.097827,
            -0.513040, 0.289508, 0.677527, 0.268109, -0.088244, 0.119781,
            -0.289511, 0.524778, 0.262884, 0.220028, -0.244767, 0.089411,
            -0.156018, -0.087030, -0.159292, -0.286646, -0.253953, -0.058657,
            -0.474756, 0.169797, -0.032919, 0.195384, 0.075355, 0.138131,
            -0.414465, -0.285118, -0.124915, 0.030645, 0.315431, -0.081032,
            0.352546, 0.132860, 0.328112, 0.035476, -0.183550, -0.413984,
            0.043452, 0.228748, -0.081765, -0.151125, -0.086251, -0.306448,
            -0.137774, -0.050508, 0.012811, -0.017824, 0.170841, 0.030549,
            0.506935, 0.087197, 0.504274, -0.202080, 0.147146, -0.072728,
            0.167713, 0.165977, -0.610894, -0.370849, -0.402698, 0.112297,
            0.410855, -0.091330, 0.227008, 0.152454, -0.293884, 0.111074,
            -0.210121, 0.423728, -0.009101, 0.457188, -0.118785, 0.164720,
            -0.017547, -0.565046, -0.274461, 0.171169, -0.015338, -0.312635,
            -0.175044, 0.069729, -0.277504, 0.272454, -0.179049, 0.505495,
            -0.301774, 0.055664, -0.425058, -0.202222, -0.165787, 0.112155,
            0.263284, 0.083972, -0.104256, 0.227892, 0.223253, 0.033592,
            0.159638, 0.115358, -0.275811, 0.212265, -0.183658, -0.168768,
        ];

        #[rustfmt::skip]
        let reference: Vec<f32> = vec![
            -0.232532, -0.066054, 0.094511, -0.021924,
            0.128477, 0.135562, -0.210019, 0.004422,
            -0.474193, 0.400765, -0.085890, -0.211323,
        ];

        for use_audio_buffer_interface in [true, false] {
            run_bitexactness_test(1, use_audio_buffer_interface, &reference_input, &reference);
        }
    }

    fn dc_signal_attenuation(sample_rate: f32) -> f32 {
        let num_channels = 1;
        let mut hpf = HighPassFilter::new(sample_rate as i32, num_channels);
        let len = sample_rate as usize / 10;
        let mut audio_data = vec![vec![0.0f32; len]; 1];

        let max_dc_level = 32767.0f32;
        let mut energy_before_filtering = 0.0f32;
        let mut energy_after_filtering = 0.0f32;

        for _ in 0..2 {
            energy_before_filtering = 0.0;
            for sample in audio_data[0].iter_mut() {
                *sample = max_dc_level;
                energy_before_filtering += *sample * *sample;
            }

            hpf.process_channels(&mut audio_data);
            energy_after_filtering = 0.0;
            for &sample in &audio_data[0] {
                energy_after_filtering += sample * sample;
            }
        }

        10.0 * (energy_before_filtering / energy_after_filtering).log10()
    }

    #[test]
    fn dc_signal_attenuation_16() {
        assert!(dc_signal_attenuation(16000.0) >= 47.3);
    }

    #[test]
    fn dc_signal_attenuation_32() {
        assert!(dc_signal_attenuation(32000.0) >= 47.3);
    }

    #[test]
    fn dc_signal_attenuation_48() {
        assert!(dc_signal_attenuation(48000.0) >= 47.3);
    }
}
