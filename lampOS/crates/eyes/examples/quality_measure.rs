//! Generated-input microbenchmark, not camera/recognition/presence evidence.
//! Run with `cargo run --release -p lamp-eyes --example quality_measure`.
use lamp_eyes::{faces::Face, picture::Picture, quality};
use serde_json::json;
use std::{hint::black_box, time::Instant};

const REPEATS: usize = 500;
const BATCH: usize = 8;

fn percentile(samples: &[f64], quantile: f64) -> f64 {
    let mut values = samples.to_vec();
    values.sort_by(f64::total_cmp);
    let at = (values.len() - 1) as f64 * quantile;
    let low = at.floor() as usize;
    let high = at.ceil() as usize;
    values[low] + (values[high] - values[low]) * (at - low as f64)
}

fn main() {
    let mut results = Vec::new();
    for (width, height) in [(320usize, 240usize), (1280, 960), (4096, 4096)] {
        let rgb: Vec<u8> = (0..width * height * 3)
            .map(|i| ((i / 3 + i / (width * 3)) % 256) as u8)
            .collect();
        let picture = Picture::from_rgb(&rgb, width, height).unwrap();
        let scale = (lamp_eyes::picture::WIDTH as f32 / width as f32)
            .min(lamp_eyes::picture::HEIGHT as f32 / height as f32)
            .min(1.0);
        let native = lamp_eyes::picture::Frame {
            rgb,
            width,
            height,
            scale,
        };
        let (w, h) = (picture.filled.0 as f32, picture.filled.1 as f32);
        let face = Face {
            score: 0.9,
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            marks: [
                (w * 0.3, h * 0.3),
                (w * 0.7, h * 0.3),
                (w * 0.5, h * 0.5),
                (w * 0.35, h * 0.7),
                (w * 0.65, h * 0.7),
            ],
        };
        let start = Instant::now();
        let first = quality::measure(
            black_box(&face),
            black_box(&picture),
            Some(black_box(&native)),
        );
        let first_us = start.elapsed().as_secs_f64() * 1_000_000.0;
        let blur_samples = first.blur.unwrap().samples;
        black_box(first);
        let mut single = Vec::with_capacity(REPEATS);
        let mut batch = Vec::with_capacity(REPEATS);
        for _ in 0..REPEATS {
            let start = Instant::now();
            black_box(quality::measure(
                black_box(&face),
                black_box(&picture),
                Some(black_box(&native)),
            ));
            single.push(start.elapsed().as_secs_f64() * 1_000_000.0);
            let start = Instant::now();
            for _ in 0..BATCH {
                black_box(quality::measure(
                    black_box(&face),
                    black_box(&picture),
                    Some(black_box(&native)),
                ));
            }
            batch.push(start.elapsed().as_secs_f64() * 1_000_000.0);
        }
        results.push(json!({
            "native_size": [width, height], "working_size": picture.filled,
            "blur_samples": blur_samples, "first_call_us": first_us,
            "warm_samples": REPEATS,
            "one_face_p50_us": percentile(&single, 0.50),
            "one_face_p95_us": percentile(&single, 0.95),
            "one_face_max_us": percentile(&single, 1.0),
            "batch_faces": BATCH,
            "batch_p50_us": percentile(&batch, 0.50),
            "batch_p95_us": percentile(&batch, 0.95),
            "batch_max_us": percentile(&batch, 1.0),
            "single_samples_us": single,
            "batch_samples_us": batch
        }));
    }
    println!(
        "{}",
        json!({
            "schema": "face-quality-microbenchmark/1",
            "boundary": "Borrowed decoded frame and supplied face to measurements; decode, detector and allocation excluded",
            "generated_inputs": true, "physical_camera": false,
            "training": false, "results": results
        })
    );
}
