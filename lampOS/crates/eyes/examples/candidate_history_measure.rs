//! Generated-data mechanics timing, not camera or presence evidence.
use lamp_eyes::candidate_history::{
    CameraInterval, Candidate, Capture, History, Limits, MAX_TRACKS,
};
use serde_json::json;
use std::{hint::black_box, time::Instant};

fn percentile(samples: &[f64], q: f64) -> f64 {
    let mut values = samples.to_vec();
    values.sort_by(f64::total_cmp);
    let at = (values.len() - 1) as f64 * q;
    let lower = at.floor() as usize;
    let upper = at.ceil() as usize;
    values[lower] + (values[upper] - values[lower]) * (at - lower as f64)
}

fn main() {
    // Authored timing controls, not production defaults or learned limits.
    let mut history = History::new(
        1,
        Limits {
            window_us: 10_000_000,
            maximum_gap_us: 5_000_000,
            maximum_frame_age_us: 1_000_000,
        },
    )
    .unwrap();
    let mut samples = Vec::with_capacity(1001);
    for sequence in 0..1001 {
        let candidates: [Candidate; MAX_TRACKS] = std::array::from_fn(|index| Candidate {
            track_id: index as u32,
            centre: [index as f32 / 10.0 + (sequence % 3) as f32 * 0.01, 0.5],
            usable: true,
        });
        let capture = Capture {
            epoch: 1,
            sequence,
            captured_us: sequence * 250_000,
        };
        let start = Instant::now();
        let result = history.observe(
            black_box(capture),
            black_box(capture.captured_us),
            CameraInterval::KnownStill,
            black_box(&candidates),
        );
        black_box(result.unwrap());
        samples.push(start.elapsed().as_secs_f64() * 1_000_000.0);
    }
    let repeated = &samples[1..];
    println!(
        "{}",
        json!({
            "schema": "candidate-history-mechanics/1",
            "boundary": "Validated upstream ids and normalized candidate frame to bounded history output; caller quality and interval validation excluded",
            "generated_inputs": true, "tracks_per_frame": MAX_TRACKS,
            "first_frame_us": samples[0], "later_frames": repeated.len(),
            "p50_us": percentile(repeated, 0.5),
            "p95_us": percentile(repeated, 0.95),
            "max_us": percentile(repeated, 1.0),
            "samples_us": samples,
            "physical_presence_measured": false, "device_access": false
        })
    );
}
