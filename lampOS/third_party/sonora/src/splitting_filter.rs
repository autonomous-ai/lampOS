//! Band-splitting filter for 2-band (32 kHz) and 3-band (48 kHz) operation.
//!
//! Splits a fullband audio frame into sub-band frames using QMF filter banks:
//! - **2-band** (32 kHz → 2 × 160 samples): allpass-based QMF from SPL library
//! - **3-band** (48 kHz → 3 × 160 samples): FIR filter bank with DCT modulation
//!
//! Ported from `modules/audio_processing/splitting_filter.h/cc` and
//! `common_audio/signal_processing/splitting_filter.c`.

use sonora_common_audio::channel_buffer::ChannelBuffer;

use crate::three_band_filter_bank::ThreeBandFilterBank;

const SAMPLES_PER_BAND: usize = 160;
const TWO_BAND_FILTER_SAMPLES_PER_FRAME: usize = 320;

// ---------------------------------------------------------------------------
// 2-band QMF filter (ported from common_audio/signal_processing/splitting_filter.c)
// ---------------------------------------------------------------------------

const ALL_PASS_FILTER_1: [f32; 3] = [0.097_930_91, 0.564_300_54, 0.873_733_5];
const ALL_PASS_FILTER_2: [f32; 3] = [0.325_515_75, 0.748_626_7, 0.961_456_3];

/// State size for one allpass QMF filter chain (3 cascaded first-order sections,
/// each needing 2 state values = 6 total).
const QMF_STATE_SIZE: usize = 6;

/// Three cascaded first-order allpass sections.
///
/// Each section implements:
///   y[n] = x[n-1] + a * (x[n] - y[n-1])
///
/// The three sections alternate between two scratch buffers to avoid allocation.
fn allpass_qmf(
    in_data: &mut [f32; SAMPLES_PER_BAND],
    out_data: &mut [f32; SAMPLES_PER_BAND],
    coefficients: &[f32; 3],
    state: &mut [f32; QMF_STATE_SIZE],
) {
    let data_length = SAMPLES_PER_BAND;

    // First cascade: in_data → out_data
    let diff = in_data[0] - state[1];
    out_data[0] = state[0] + coefficients[0] * diff;
    for k in 1..data_length {
        let diff = in_data[k] - out_data[k - 1];
        out_data[k] = in_data[k - 1] + coefficients[0] * diff;
    }
    state[0] = in_data[data_length - 1];
    state[1] = out_data[data_length - 1];

    // Second cascade: out_data → in_data
    let diff = out_data[0] - state[3];
    in_data[0] = state[2] + coefficients[1] * diff;
    for k in 1..data_length {
        let diff = out_data[k] - in_data[k - 1];
        in_data[k] = out_data[k - 1] + coefficients[1] * diff;
    }
    state[2] = out_data[data_length - 1];
    state[3] = in_data[data_length - 1];

    // Third cascade: in_data → out_data
    let diff = in_data[0] - state[5];
    out_data[0] = state[4] + coefficients[2] * diff;
    for k in 1..data_length {
        let diff = in_data[k] - out_data[k - 1];
        out_data[k] = in_data[k - 1] + coefficients[2] * diff;
    }
    state[4] = in_data[data_length - 1];
    state[5] = out_data[data_length - 1];
}

/// Split a fullband signal into low-band and high-band using QMF analysis.
fn analysis_qmf(
    in_data: &[f32; TWO_BAND_FILTER_SAMPLES_PER_FRAME],
    low_band: &mut [f32; SAMPLES_PER_BAND],
    high_band: &mut [f32; SAMPLES_PER_BAND],
    filter_state1: &mut [f32; QMF_STATE_SIZE],
    filter_state2: &mut [f32; QMF_STATE_SIZE],
) {
    // Split even and odd samples (stack arrays, matching C++).
    let mut half_in1 = [0.0f32; SAMPLES_PER_BAND];
    let mut half_in2 = [0.0f32; SAMPLES_PER_BAND];
    for i in 0..SAMPLES_PER_BAND {
        half_in2[i] = in_data[2 * i];
        half_in1[i] = in_data[2 * i + 1];
    }

    // Allpass filter even and odd samples independently.
    let mut filter1 = [0.0f32; SAMPLES_PER_BAND];
    let mut filter2 = [0.0f32; SAMPLES_PER_BAND];
    allpass_qmf(
        &mut half_in1,
        &mut filter1,
        &ALL_PASS_FILTER_1,
        filter_state1,
    );
    allpass_qmf(
        &mut half_in2,
        &mut filter2,
        &ALL_PASS_FILTER_2,
        filter_state2,
    );

    // Sum and difference to get low and high bands.
    for i in 0..SAMPLES_PER_BAND {
        low_band[i] = (filter1[i] + filter2[i]) * 0.5;
        high_band[i] = (filter1[i] - filter2[i]) * 0.5;
    }
}

/// Merge low-band and high-band into a fullband signal using QMF synthesis.
fn synthesis_qmf(
    low_band: &[f32; SAMPLES_PER_BAND],
    high_band: &[f32; SAMPLES_PER_BAND],
    out_data: &mut [f32; TWO_BAND_FILTER_SAMPLES_PER_FRAME],
    filter_state1: &mut [f32; QMF_STATE_SIZE],
    filter_state2: &mut [f32; QMF_STATE_SIZE],
) {
    // Sum and difference channels (stack arrays, matching C++).
    let mut half_in1 = [0.0f32; SAMPLES_PER_BAND];
    let mut half_in2 = [0.0f32; SAMPLES_PER_BAND];
    for i in 0..SAMPLES_PER_BAND {
        half_in1[i] = low_band[i] + high_band[i];
        half_in2[i] = low_band[i] - high_band[i];
    }

    // Allpass filter.
    let mut filter1 = [0.0f32; SAMPLES_PER_BAND];
    let mut filter2 = [0.0f32; SAMPLES_PER_BAND];
    allpass_qmf(
        &mut half_in1,
        &mut filter1,
        &ALL_PASS_FILTER_2,
        filter_state1,
    );
    allpass_qmf(
        &mut half_in2,
        &mut filter2,
        &ALL_PASS_FILTER_1,
        filter_state2,
    );

    // Interleave with saturation (matching C++ int16 range clamp).
    for i in 0..SAMPLES_PER_BAND {
        out_data[2 * i] = filter2[i].clamp(-32768.0, 32767.0);
        out_data[2 * i + 1] = filter1[i].clamp(-32768.0, 32767.0);
    }
}

// ---------------------------------------------------------------------------
// TwoBandsStates
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct TwoBandsStates {
    analysis_state1: [f32; QMF_STATE_SIZE],
    analysis_state2: [f32; QMF_STATE_SIZE],
    synthesis_state1: [f32; QMF_STATE_SIZE],
    synthesis_state2: [f32; QMF_STATE_SIZE],
}

impl TwoBandsStates {
    fn new() -> Self {
        Self {
            analysis_state1: [0.0; QMF_STATE_SIZE],
            analysis_state2: [0.0; QMF_STATE_SIZE],
            synthesis_state1: [0.0; QMF_STATE_SIZE],
            synthesis_state2: [0.0; QMF_STATE_SIZE],
        }
    }
}

// ---------------------------------------------------------------------------
// SplittingFilter
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum FilterState {
    TwoBand(Vec<TwoBandsStates>),
    ThreeBand(Vec<ThreeBandFilterBank>),
}

/// Band-splitting filter supporting 2-band and 3-band operation.
///
/// For each block, call [`analysis`] to split into bands, then [`synthesis`]
/// to merge them back.
#[derive(Debug)]
pub(crate) struct SplittingFilter {
    num_bands: usize,
    state: FilterState,
}

impl SplittingFilter {
    /// Create a new splitting filter.
    ///
    /// - `num_channels`: number of audio channels
    /// - `num_bands`: 2 (for 32 kHz) or 3 (for 48 kHz)
    pub(crate) fn new(num_channels: usize, num_bands: usize) -> Self {
        assert!(num_bands == 2 || num_bands == 3, "num_bands must be 2 or 3");
        let state = match num_bands {
            2 => FilterState::TwoBand((0..num_channels).map(|_| TwoBandsStates::new()).collect()),
            3 => FilterState::ThreeBand(
                (0..num_channels)
                    .map(|_| ThreeBandFilterBank::new())
                    .collect(),
            ),
            _ => unreachable!(),
        };
        Self { num_bands, state }
    }

    /// Split fullband data into sub-bands.
    pub(crate) fn analysis(&mut self, data: &ChannelBuffer<f32>, bands: &mut ChannelBuffer<f32>) {
        debug_assert_eq!(self.num_bands, bands.num_bands());
        debug_assert_eq!(data.num_channels(), bands.num_channels());
        debug_assert_eq!(
            data.num_frames(),
            bands.num_frames_per_band() * bands.num_bands()
        );
        match &mut self.state {
            FilterState::TwoBand(states) => {
                Self::two_bands_analysis(states, data, bands);
            }
            FilterState::ThreeBand(banks) => {
                Self::three_bands_analysis(banks, data, bands);
            }
        }
    }

    /// Merge sub-bands back into fullband data.
    pub(crate) fn synthesis(&mut self, bands: &ChannelBuffer<f32>, data: &mut ChannelBuffer<f32>) {
        debug_assert_eq!(self.num_bands, bands.num_bands());
        debug_assert_eq!(data.num_channels(), bands.num_channels());
        debug_assert_eq!(
            data.num_frames(),
            bands.num_frames_per_band() * bands.num_bands()
        );
        match &mut self.state {
            FilterState::TwoBand(states) => {
                Self::two_bands_synthesis(states, bands, data);
            }
            FilterState::ThreeBand(banks) => {
                Self::three_bands_synthesis(banks, bands, data);
            }
        }
    }

    fn two_bands_analysis(
        states: &mut [TwoBandsStates],
        data: &ChannelBuffer<f32>,
        bands: &mut ChannelBuffer<f32>,
    ) {
        debug_assert_eq!(states.len(), data.num_channels());
        debug_assert_eq!(data.num_frames(), TWO_BAND_FILTER_SAMPLES_PER_FRAME);

        for (i, state) in states.iter_mut().enumerate() {
            let mut low_band = [0.0f32; SAMPLES_PER_BAND];
            let mut high_band = [0.0f32; SAMPLES_PER_BAND];

            // C++ uses channels(0)[i] with num_frames() length, which reads
            // the full channel data across all bands (320 samples).
            let in_data: &[f32; TWO_BAND_FILTER_SAMPLES_PER_FRAME] =
                data.bands(i).try_into().unwrap();
            analysis_qmf(
                in_data,
                &mut low_band,
                &mut high_band,
                &mut state.analysis_state1,
                &mut state.analysis_state2,
            );

            bands.channel_mut(0, i).copy_from_slice(&low_band);
            bands.channel_mut(1, i).copy_from_slice(&high_band);
        }
    }

    fn two_bands_synthesis(
        states: &mut [TwoBandsStates],
        bands: &ChannelBuffer<f32>,
        data: &mut ChannelBuffer<f32>,
    ) {
        debug_assert!(data.num_channels() <= states.len());
        debug_assert_eq!(data.num_frames(), TWO_BAND_FILTER_SAMPLES_PER_FRAME);

        for (i, state) in states.iter_mut().enumerate().take(data.num_channels()) {
            let mut low_band = [0.0f32; SAMPLES_PER_BAND];
            let mut high_band = [0.0f32; SAMPLES_PER_BAND];
            low_band.copy_from_slice(bands.channel(0, i));
            high_band.copy_from_slice(bands.channel(1, i));

            // C++ writes to channels(0)[i] with full num_frames() length.
            let out_data: &mut [f32; TWO_BAND_FILTER_SAMPLES_PER_FRAME] =
                data.bands_mut(i).try_into().unwrap();
            synthesis_qmf(
                &low_band,
                &high_band,
                out_data,
                &mut state.synthesis_state1,
                &mut state.synthesis_state2,
            );
        }
    }

    fn three_bands_analysis(
        banks: &mut [ThreeBandFilterBank],
        data: &ChannelBuffer<f32>,
        bands: &mut ChannelBuffer<f32>,
    ) {
        use crate::three_band_filter_bank::{FULL_BAND_SIZE, NUM_BANDS, SPLIT_BAND_SIZE};
        debug_assert_eq!(banks.len(), data.num_channels());
        debug_assert_eq!(data.num_frames(), FULL_BAND_SIZE);
        debug_assert_eq!(bands.num_frames(), FULL_BAND_SIZE);
        debug_assert_eq!(bands.num_bands(), NUM_BANDS);
        debug_assert_eq!(bands.num_frames_per_band(), SPLIT_BAND_SIZE);

        for (i, bank) in banks.iter_mut().enumerate() {
            // C++ uses channels_view()[i] which gives the full 480 samples.
            let input: &[f32; FULL_BAND_SIZE] = data.bands(i).try_into().unwrap();
            let mut output = [[0.0f32; SPLIT_BAND_SIZE]; NUM_BANDS];
            bank.analysis(input, &mut output);
            for (band, out) in output.iter().enumerate() {
                bands.channel_mut(band, i).copy_from_slice(out);
            }
        }
    }

    fn three_bands_synthesis(
        banks: &mut [ThreeBandFilterBank],
        bands: &ChannelBuffer<f32>,
        data: &mut ChannelBuffer<f32>,
    ) {
        use crate::three_band_filter_bank::{FULL_BAND_SIZE, NUM_BANDS, SPLIT_BAND_SIZE};
        debug_assert!(data.num_channels() <= banks.len());
        debug_assert_eq!(data.num_frames(), FULL_BAND_SIZE);
        debug_assert_eq!(bands.num_frames(), FULL_BAND_SIZE);
        debug_assert_eq!(bands.num_bands(), NUM_BANDS);
        debug_assert_eq!(bands.num_frames_per_band(), SPLIT_BAND_SIZE);

        for (i, bank) in banks.iter_mut().enumerate().take(data.num_channels()) {
            let mut input = [[0.0f32; SPLIT_BAND_SIZE]; NUM_BANDS];
            for (band, inp) in input.iter_mut().enumerate() {
                inp.copy_from_slice(bands.channel(band, i));
            }
            let output: &mut [f32; FULL_BAND_SIZE] = data.bands_mut(i).try_into().unwrap();
            bank.synthesis(&input, output);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::PI;

    use super::*;

    // -----------------------------------------------------------------------
    // QMF filter unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn qmf_analysis_splits_signal() {
        let mut state1 = [0.0f32; QMF_STATE_SIZE];
        let mut state2 = [0.0f32; QMF_STATE_SIZE];

        // Generate a low-frequency sine (500 Hz at 32 kHz = well within low band).
        let mut input = [0.0f32; TWO_BAND_FILTER_SAMPLES_PER_FRAME];
        for (i, sample) in input.iter_mut().enumerate() {
            *sample = (2.0 * PI * 500.0 * i as f32 / 32000.0).sin();
        }

        let mut low_band = [0.0f32; SAMPLES_PER_BAND];
        let mut high_band = [0.0f32; SAMPLES_PER_BAND];

        analysis_qmf(
            &input,
            &mut low_band,
            &mut high_band,
            &mut state1,
            &mut state2,
        );

        let low_energy: f32 = low_band.iter().map(|x| x * x).sum();
        let high_energy: f32 = high_band.iter().map(|x| x * x).sum();

        // Low-frequency signal should appear mostly in low band.
        assert!(
            low_energy > high_energy * 10.0,
            "low_energy={low_energy}, high_energy={high_energy}",
        );
    }

    #[test]
    fn qmf_synthesis_reconstructs() {
        let mut a_state1 = [0.0f32; QMF_STATE_SIZE];
        let mut a_state2 = [0.0f32; QMF_STATE_SIZE];
        let mut s_state1 = [0.0f32; QMF_STATE_SIZE];
        let mut s_state2 = [0.0f32; QMF_STATE_SIZE];

        // Process multiple frames to let filter settle.
        let num_frames = 10;
        let mut last_input = [0.0f32; TWO_BAND_FILTER_SAMPLES_PER_FRAME];
        let mut last_output = [0.0f32; TWO_BAND_FILTER_SAMPLES_PER_FRAME];

        for frame in 0..num_frames {
            let mut input = [0.0f32; TWO_BAND_FILTER_SAMPLES_PER_FRAME];
            for (i, sample) in input.iter_mut().enumerate() {
                let t = (frame * TWO_BAND_FILTER_SAMPLES_PER_FRAME + i) as f32 / 32000.0;
                *sample = (2.0 * PI * 1000.0 * t).sin() * 1000.0;
            }

            let mut low_band = [0.0f32; SAMPLES_PER_BAND];
            let mut high_band = [0.0f32; SAMPLES_PER_BAND];
            analysis_qmf(
                &input,
                &mut low_band,
                &mut high_band,
                &mut a_state1,
                &mut a_state2,
            );

            let mut output = [0.0f32; TWO_BAND_FILTER_SAMPLES_PER_FRAME];
            synthesis_qmf(
                &low_band,
                &high_band,
                &mut output,
                &mut s_state1,
                &mut s_state2,
            );

            last_input = input;
            last_output = output;
        }

        // After settling, output should have significant energy relative to input.
        let input_energy: f32 = last_input.iter().map(|x| x * x).sum();
        let output_energy: f32 = last_output.iter().map(|x| x * x).sum();
        assert!(
            output_energy > input_energy * 0.5,
            "roundtrip should preserve energy: input={input_energy}, output={output_energy}",
        );
    }

    // -----------------------------------------------------------------------
    // SplittingFilter — 3-band test (matches C++ upstream test)
    // -----------------------------------------------------------------------

    #[test]
    fn splits_into_three_bands_and_reconstructs() {
        let channels = 1;
        let sample_rate_hz = 48000;
        let num_bands = 3;
        let frequencies_hz = [1000, 12000, 18000];
        let amplitude = 8192.0f32;
        let chunks = 8;
        let samples_per_48khz_channel = 480;
        let samples_per_16khz_channel = 160;

        let mut splitting_filter = SplittingFilter::new(channels, num_bands);
        let mut in_data = ChannelBuffer::<f32>::new(samples_per_48khz_channel, channels, num_bands);
        let mut bands = ChannelBuffer::<f32>::new(samples_per_48khz_channel, channels, num_bands);
        let mut out_data =
            ChannelBuffer::<f32>::new(samples_per_48khz_channel, channels, num_bands);

        for i in 0..chunks {
            // Input signal generation.
            let mut is_present = [false; 3];
            // Zero the input channel (C++ uses channels()[0] = full channel).
            for s in in_data.bands_mut(0).iter_mut() {
                *s = 0.0;
            }
            for (j, &freq) in frequencies_hz.iter().enumerate() {
                is_present[j] = (i & (1 << j)) != 0;
                let amp = if is_present[j] { amplitude } else { 0.0 };
                let mut addition = vec![0.0f32; samples_per_48khz_channel];
                for (k, a) in addition.iter_mut().enumerate() {
                    *a = amp
                        * (2.0 * PI * freq as f32 * (i * samples_per_48khz_channel + k) as f32
                            / sample_rate_hz as f32)
                            .sin();
                }
                let ch = in_data.bands_mut(0);
                for (c, a) in ch.iter_mut().zip(addition.iter()) {
                    *c += a;
                }
            }

            // Three-band splitting filter.
            splitting_filter.analysis(&in_data, &mut bands);

            // Energy calculation.
            for (j, &present) in is_present.iter().enumerate().take(num_bands) {
                let mut energy = 0.0f32;
                let band_data = bands.channel(j, 0);
                for s in &band_data[..samples_per_16khz_channel] {
                    energy += s * s;
                }
                energy /= samples_per_16khz_channel as f32;
                if present {
                    assert!(
                        energy > amplitude * amplitude / 4.0,
                        "chunk {i}, band {j}: expected present, energy={energy}",
                    );
                } else {
                    assert!(
                        energy < amplitude * amplitude / 4.0,
                        "chunk {i}, band {j}: expected absent, energy={energy}",
                    );
                }
            }

            // Three-band merge.
            splitting_filter.synthesis(&bands, &mut out_data);

            // Delay and cross correlation estimation.
            let mut xcorr = 0.0f32;
            let in_ch = in_data.bands(0);
            let out_ch = out_data.bands(0);
            for delay in 0..samples_per_48khz_channel {
                let mut tmpcorr = 0.0f32;
                for j in delay..samples_per_48khz_channel {
                    tmpcorr += in_ch[j - delay] * out_ch[j];
                }
                tmpcorr /= samples_per_48khz_channel as f32;
                if tmpcorr > xcorr {
                    xcorr = tmpcorr;
                }
            }

            // High cross correlation check.
            let any_present = is_present.iter().any(|&p| p);
            if any_present {
                assert!(
                    xcorr > amplitude * amplitude / 4.0,
                    "chunk {i}: cross-correlation too low: {xcorr}",
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // SplittingFilter — 2-band test
    // -----------------------------------------------------------------------

    #[test]
    fn two_band_analysis_and_synthesis() {
        let channels = 1;
        let num_bands = 2;
        let num_frames = 320;
        let chunks = 10;
        let amplitude = 4096.0f32;

        let mut splitting_filter = SplittingFilter::new(channels, num_bands);

        for chunk in 0..chunks {
            let mut in_data = ChannelBuffer::<f32>::new(num_frames, channels, num_bands);
            let mut bands = ChannelBuffer::<f32>::new(num_frames, channels, num_bands);
            let mut out_data = ChannelBuffer::<f32>::new(num_frames, channels, num_bands);

            // Generate a low-frequency sine (500 Hz at 32 kHz).
            let ch = in_data.bands_mut(0);
            for (k, sample) in ch.iter_mut().enumerate().take(num_frames) {
                let t = (chunk * num_frames + k) as f32 / 32000.0;
                *sample = amplitude * (2.0 * PI * 500.0 * t).sin();
            }

            splitting_filter.analysis(&in_data, &mut bands);

            // Low-frequency signal should be mostly in band 0.
            let low_energy: f32 = bands.channel(0, 0).iter().map(|x| x * x).sum();
            let high_energy: f32 = bands.channel(1, 0).iter().map(|x| x * x).sum();
            if chunk >= 2 {
                assert!(
                    low_energy > high_energy * 5.0,
                    "chunk {chunk}: low={low_energy}, high={high_energy}",
                );
            }

            splitting_filter.synthesis(&bands, &mut out_data);
        }
    }

    #[test]
    fn two_band_zero_input() {
        let mut filter = SplittingFilter::new(1, 2);
        let in_data = ChannelBuffer::<f32>::new(320, 1, 2);
        let mut bands = ChannelBuffer::<f32>::new(320, 1, 2);

        filter.analysis(&in_data, &mut bands);

        for &s in bands.data() {
            assert_eq!(s, 0.0);
        }
    }

    #[test]
    fn three_band_zero_input() {
        let mut filter = SplittingFilter::new(1, 3);
        let in_data = ChannelBuffer::<f32>::new(480, 1, 3);
        let mut bands = ChannelBuffer::<f32>::new(480, 1, 3);

        filter.analysis(&in_data, &mut bands);

        for &s in bands.data() {
            assert_eq!(s, 0.0);
        }
    }

    #[test]
    fn multi_channel_two_band() {
        let channels = 4;
        let mut filter = SplittingFilter::new(channels, 2);
        let mut in_data = ChannelBuffer::<f32>::new(320, channels, 2);
        let mut bands = ChannelBuffer::<f32>::new(320, channels, 2);

        // Put different signals in each channel.
        for ch in 0..channels {
            let data = in_data.bands_mut(ch);
            for (k, d) in data.iter_mut().enumerate().take(320) {
                *d = (ch as f32 + 1.0) * (k as f32 / 320.0);
            }
        }

        filter.analysis(&in_data, &mut bands);

        // Each channel should have non-zero output.
        for ch in 0..channels {
            let energy: f32 = bands.channel(0, ch).iter().map(|x| x * x).sum();
            assert!(energy > 0.0, "channel {ch} should have non-zero output");
        }
    }

    #[test]
    fn multi_channel_three_band() {
        let channels = 2;
        let mut filter = SplittingFilter::new(channels, 3);
        let mut in_data = ChannelBuffer::<f32>::new(480, channels, 3);
        let mut bands = ChannelBuffer::<f32>::new(480, channels, 3);

        // Put different signals in each channel.
        for ch in 0..channels {
            let data = in_data.bands_mut(ch);
            for (k, d) in data.iter_mut().enumerate().take(480) {
                *d = (ch as f32 + 1.0) * (2.0 * PI * 1000.0 * k as f32 / 48000.0).sin();
            }
        }

        filter.analysis(&in_data, &mut bands);

        for ch in 0..channels {
            let energy: f32 = bands.channel(0, ch).iter().map(|x| x * x).sum();
            assert!(
                energy > 0.0,
                "channel {ch} band 0 should have non-zero output"
            );
        }
    }
}
