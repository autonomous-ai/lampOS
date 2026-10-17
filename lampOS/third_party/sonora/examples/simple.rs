//! Minimal echo cancellation demo.
//!
//! Creates synthetic stereo render and capture signals, processes them through
//! the AEC pipeline, and verifies that the capture signal was modified.
//!
//! ```sh
//! cargo run -p sonora --example simple
//! ```

use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};

fn main() {
    let sample_rate_hz = 48_000;
    let num_channels = 2;
    let stream_config = StreamConfig::new(sample_rate_hz, num_channels);
    let num_frames = stream_config.num_frames();

    let config = Config {
        echo_canceller: Some(EchoCanceller::default()),
        ..Default::default()
    };

    let mut apm = AudioProcessing::builder()
        .config(config)
        .capture_config(stream_config)
        .render_config(stream_config)
        .build();

    // Generate synthetic stereo signals that simulate a microphone picking up
    // speaker output (the render signal leaks into the capture signal).
    let (render_ch0, render_ch1, capture_ch0, capture_ch1) = sample_stereo_frames(num_frames);

    // Process the render frame (tell the AEC what is being played through speakers).
    let mut render_out_ch0 = vec![0.0f32; num_frames];
    let mut render_out_ch1 = vec![0.0f32; num_frames];
    apm.process_render_f32(
        &[&render_ch0, &render_ch1],
        &mut [&mut render_out_ch0, &mut render_out_ch1],
    )
    .unwrap();

    assert_eq!(
        render_ch0, render_out_ch0,
        "render channel 0 should not be modified"
    );
    assert_eq!(
        render_ch1, render_out_ch1,
        "render channel 1 should not be modified"
    );

    // Process the capture frame (apply echo cancellation to microphone input).
    let mut capture_out_ch0 = vec![0.0f32; num_frames];
    let mut capture_out_ch1 = vec![0.0f32; num_frames];
    apm.process_capture_f32(
        &[&capture_ch0, &capture_ch1],
        &mut [&mut capture_out_ch0, &mut capture_out_ch1],
    )
    .unwrap();

    assert_ne!(
        capture_ch0, capture_out_ch0,
        "echo cancellation should have modified capture channel 0"
    );
    assert_ne!(
        capture_ch1, capture_out_ch1,
        "echo cancellation should have modified capture channel 1"
    );

    println!("Successfully processed render and capture frames through sonora AEC.");
}

/// Generate example stereo frames that simulate a microphone (capture) picking
/// up the speaker (render) output.
fn sample_stereo_frames(num_frames: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
    let mut render_ch0 = vec![0.0f32; num_frames];
    let mut render_ch1 = vec![0.0f32; num_frames];
    let mut capture_ch0 = vec![0.0f32; num_frames];
    let mut capture_ch1 = vec![0.0f32; num_frames];

    for i in 0..num_frames {
        render_ch0[i] = (i as f32 / 40.0).cos() * 0.4;
        render_ch1[i] = (i as f32 / 40.0).cos() * 0.2;
        capture_ch0[i] = (i as f32 / 20.0).sin() * 0.4 + render_ch0[i] * 0.2;
        capture_ch1[i] = (i as f32 / 20.0).sin() * 0.2 + render_ch1[i] * 0.2;
    }

    (render_ch0, render_ch1, capture_ch0, capture_ch1)
}
