use lamp_audio::{
    CAPTURE_SAMPLES, EchoProcessor, RENDER_SAMPLES, float_to_pcm,
    qualification::{benchmark, synthetic_echo_block},
};

#[test]
fn pcm_extremes_saturate_without_sign_wrap() {
    assert_eq!(float_to_pcm(1.0), i16::MAX);
    assert_eq!(float_to_pcm(-1.0), i16::MIN);
    assert_eq!(float_to_pcm(2.0), i16::MAX);
    assert_eq!(float_to_pcm(-2.0), i16::MIN);
}

#[test]
fn silence_stays_silent_at_native_provider_and_capture_rates() {
    let mut processor = EchoProcessor::new(true);
    for _ in 0..100 {
        processor.render(&[0; RENDER_SAMPLES]).unwrap();
        assert_eq!(
            processor.capture(&[0; CAPTURE_SAMPLES], 0).unwrap(),
            [0; CAPTURE_SAMPLES]
        );
    }
}

#[test]
fn impossible_delay_is_rejected_without_poisoning_next_frame() {
    let mut processor = EchoProcessor::new(true);
    assert!(processor.capture(&[0; CAPTURE_SAMPLES], 501).is_err());
    processor.render(&[0; RENDER_SAMPLES]).unwrap();
    assert_eq!(
        processor.capture(&[0; CAPTURE_SAMPLES], 0).unwrap(),
        [0; CAPTURE_SAMPLES]
    );
}

#[test]
fn alignment_observation_preserves_pcm_and_reset_discards_the_old_value() {
    for noise_suppression in [false, true] {
        let mut observed = EchoProcessor::new(noise_suppression);
        let mut control = EchoProcessor::new(noise_suppression);
        assert_eq!(observed.internal_alignment_ms(), None);
        for i in 0..250 {
            let (render, capture) = synthetic_echo_block(i);
            observed.render(&render).unwrap();
            control.render(&render).unwrap();
            let before = observed.internal_alignment_ms();
            assert_eq!(observed.internal_alignment_ms(), before);
            assert_eq!(
                observed.capture(&capture, 18).unwrap(),
                control.capture(&capture, 18).unwrap(),
            );
            let alignment = observed.internal_alignment_ms();
            assert!(alignment.is_some());
            assert_eq!(observed.internal_alignment_ms(), alignment);
        }
        observed.reset();
        assert_eq!(observed.internal_alignment_ms(), None);
    }
}

#[test]
fn reset_forgets_previous_reference_and_matches_a_fresh_processor() {
    let mut processor = EchoProcessor::new(true);
    for i in 0..100 {
        let (render, capture) = synthetic_echo_block(i);
        processor.render(&render).unwrap();
        processor.capture(&capture, 70).unwrap();
    }
    processor.reset();
    let mut fresh = EchoProcessor::new(true);
    for i in 100..150 {
        let (render, capture) = synthetic_echo_block(i);
        processor.render(&render).unwrap();
        fresh.render(&render).unwrap();
        assert_eq!(
            processor.capture(&capture, 70).unwrap(),
            fresh.capture(&capture, 70).unwrap()
        );
    }
}

#[test]
fn echo_fixture_is_attenuated_after_warmup_without_erasing_all_near_end() {
    let report = benchmark(500).unwrap();
    assert!(report.synthetic_echo_reduction_db >= 6.0, "{report:?}");
    // This is a signal-integrity floor, not a claim of natural double-talk quality.
    assert!(
        report.near_end_with_silent_render_output_input_rms_ratio > 0.02,
        "{report:?}"
    );
}

#[test]
fn benchmark_resource_bounds_are_explicit() {
    assert!(benchmark(499).is_err());
    assert!(benchmark(60001).is_err());
}

#[test]
fn cold_report_starts_at_first_capture_and_keeps_reset_near_end_separate() {
    let report = benchmark(500).unwrap();
    let mut echo = EchoProcessor::new(true);
    let mut near = EchoProcessor::new(true);
    let mut input_energy = 0.0;
    let mut echo_energy = 0.0;
    let mut near_energy = 0.0;
    for block in 0..25 {
        let (render, capture) = synthetic_echo_block(block);
        echo.render(&render).unwrap();
        near.render(&[0; RENDER_SAMPLES]).unwrap();
        let echo_output = echo.capture(&capture, 70).unwrap();
        let near_output = near.capture(&capture, 0).unwrap();
        input_energy += capture
            .iter()
            .map(|&sample| f64::from(sample).powi(2))
            .sum::<f64>();
        echo_energy += echo_output
            .iter()
            .map(|&sample| f64::from(sample).powi(2))
            .sum::<f64>();
        near_energy += near_output
            .iter()
            .map(|&sample| f64::from(sample).powi(2))
            .sum::<f64>();
    }
    let first_echo = &report.cold_start_echo[0];
    let first_near = &report.cold_start_near_end_with_silent_render[0];
    assert_eq!(first_echo.frames, 25 * CAPTURE_SAMPLES);
    assert_eq!(first_echo.input_rms_pcm, (input_energy / 4000.0).sqrt());
    assert_eq!(first_echo.output_rms_pcm, (echo_energy / 4000.0).sqrt());
    assert_eq!(first_near.output_rms_pcm, (near_energy / 4000.0).sqrt());
    assert_ne!(first_echo.output_rms_pcm, first_near.output_rms_pcm);
}

#[test]
fn the_linear_output_is_off_by_default_and_on_changes_no_processed_block() {
    for noise_suppression in [false, true] {
        let mut plain = EchoProcessor::new(noise_suppression);
        let mut linear = EchoProcessor::new(noise_suppression).with_linear_output();
        assert_eq!(linear.linear(), None);
        for i in 0..400 {
            let (render, capture) = synthetic_echo_block(i);
            plain.render(&render).unwrap();
            linear.render(&render).unwrap();
            assert_eq!(
                plain.capture(&capture, 18).unwrap(),
                linear.capture(&capture, 18).unwrap(),
                "block {i}, noise suppression {noise_suppression}"
            );
            assert_eq!(plain.linear(), None);
            assert!(linear.linear().is_some());
        }
        assert!(linear.capture(&[0; CAPTURE_SAMPLES], 501).is_err());
        assert_eq!(linear.linear(), None);
        linear.reset();
        plain.reset();
        assert_eq!(linear.linear(), None);
        for i in 400..450 {
            let (render, capture) = synthetic_echo_block(i);
            plain.render(&render).unwrap();
            linear.render(&render).unwrap();
            assert_eq!(
                plain.capture(&capture, 18).unwrap(),
                linear.capture(&capture, 18).unwrap()
            );
            assert!(linear.linear().is_some());
        }
    }
}

fn energy(samples: &[i16]) -> f64 {
    samples.iter().map(|&x| f64::from(x).powi(2)).sum()
}

/// Normalized correlation of `a` with `b` delayed by `lag` samples.
fn correlation(a: &[i16], b: &[i16], lag: usize) -> f64 {
    let (a, b) = (&a[..a.len() - lag], &b[lag..]);
    let product: f64 = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| f64::from(x) * f64::from(y))
        .sum();
    product / (energy(a) * energy(b)).sqrt()
}

#[test]
fn the_linear_output_removes_a_linear_echo_and_passes_speech_over_silence() {
    let mut echo = EchoProcessor::new(false).with_linear_output();
    let mut near = EchoProcessor::new(false).with_linear_output();
    let (mut microphone, mut echo_linear) = (Vec::new(), Vec::new());
    let (mut near_linear, mut near_output) = (Vec::new(), Vec::new());
    for i in 0..600 {
        let (render, capture) = synthetic_echo_block(i);
        echo.render(&render).unwrap();
        echo.capture(&capture, 18).unwrap();
        // The same sound as speech with the lamp silent.
        near.render(&[0; RENDER_SAMPLES]).unwrap();
        let output = near.capture(&capture, 0).unwrap();
        if i >= 300 {
            microphone.extend(capture);
            echo_linear.extend(*echo.linear().unwrap());
            near_linear.extend(*near.linear().unwrap());
            near_output.extend(output);
        }
    }
    // 36.5 dB on this Mac (x86_64), 2026-10-17.
    let removed = 10.0 * (energy(&microphone) / energy(&echo_linear)).log10();
    assert!(removed > 20.0, "{removed} dB");
    // 0.53 dB less, after the high-pass filter.
    let kept = 10.0 * (energy(&near_linear) / energy(&microphone)).log10();
    assert!(kept.abs() < 1.5, "{kept} dB");
    // The linear output is one 64-sample block behind the microphone and
    // one ahead of the processed output, which the suppressor delays.
    assert!(correlation(&microphone, &near_linear, 64) > 0.7);
    assert!(correlation(&microphone, &near_linear, 0) < 0.3);
    assert!(correlation(&near_linear, &near_output, 64) > 0.99);
}
