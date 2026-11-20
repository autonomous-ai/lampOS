use super::*;

fn sine(rate: u32, hertz: f64, seconds: f64, amplitude: f64) -> Vec<f32> {
    (0..(f64::from(rate) * seconds) as usize)
        .map(|n| {
            (amplitude * (2.0 * std::f64::consts::PI * hertz * n as f64 / f64::from(rate)).sin())
                as f32
        })
        .collect()
}

fn rms(samples: &[i16]) -> f64 {
    let power: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
    (power / samples.len() as f64).sqrt()
}

fn convert(rate: u32, input: &[f32], block: usize) -> Vec<i16> {
    let mut resampler = Resampler::new(rate).unwrap();
    let mut out = Vec::new();
    for part in input.chunks(block) {
        resampler.push(part, &mut out);
    }
    resampler.finish(&mut out);
    out
}

#[test]
fn the_output_is_24_khz_long_for_44_1_and_48_khz_input() {
    for rate in [44_100, 48_000, 32_000, 22_050, 16_000, 8_000] {
        let out = convert(rate, &sine(rate, 440.0, 2.0, 0.5), 1_152);
        let expected = 2 * OUT_RATE as usize;
        assert!(
            out.len().abs_diff(expected) <= 4,
            "{rate}: {} not near {expected}",
            out.len()
        );
    }
}

#[test]
fn a_tone_keeps_its_level_and_its_pitch() {
    for rate in [44_100, 48_000] {
        let out = convert(rate, &sine(rate, 1_000.0, 1.0, 0.5), 1_000);
        // Past the start's silence: a half-amplitude sine is 0.5/sqrt(2) RMS.
        let steady = &out[1_000..out.len() - 1_000];
        let expected = 0.5 / 2f64.sqrt() * 32_767.0;
        assert!(
            (rms(steady) - expected).abs() < expected * 0.02,
            "{rate}: {} vs {expected}",
            rms(steady)
        );
        // 1 kHz crosses zero upward 1,000 times a second.
        let crossings = steady
            .windows(2)
            .filter(|pair| pair[0] < 0 && pair[1] >= 0)
            .count();
        let per_second = crossings as f64 * 24_000.0 / steady.len() as f64;
        assert!(
            (per_second - 1_000.0).abs() < 15.0,
            "{rate}: {per_second} Hz"
        );
    }
}

#[test]
fn a_tone_above_the_output_band_is_cut_and_not_folded_back() {
    // 20 kHz at 48 kHz is past 12 kHz, the output's Nyquist: it must go,
    // not come back as a 4 kHz alias.
    let out = convert(48_000, &sine(48_000, 20_000.0, 1.0, 0.5), 960);
    let steady = &out[1_000..out.len() - 1_000];
    assert!(rms(steady) < 0.02 * 32_767.0, "{}", rms(steady));
}

#[test]
fn block_sizes_make_no_difference() {
    let input = sine(44_100, 700.0, 0.5, 0.4);
    let whole = convert(44_100, &input, input.len());
    for block in [1, 7, 576, 1_152, 4_410] {
        let parts = convert(44_100, &input, block);
        assert_eq!(parts, whole, "block {block}");
    }
}

#[test]
fn at_24_khz_it_only_converts_and_never_exceeds_full_scale() {
    let input = [0.0f32, 0.5, -0.5, 1.0, -1.0, 2.0, -2.0];
    let out = convert(24_000, &input, 3);
    assert_eq!(out, [0, 16_384, -16_384, 32_767, -32_767, 32_767, -32_767]);
}

#[test]
fn a_rate_that_cannot_be_played_has_no_converter() {
    assert!(Resampler::new(0).is_none());
    assert!(Resampler::new(192_001).is_none());
    assert!(Resampler::new(192_000).is_some());
}
