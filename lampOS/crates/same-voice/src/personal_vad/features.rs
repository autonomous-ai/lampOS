//! The published Personal VAD v1 frontend. CAM++ uses different filter banks.
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::sync::Arc;

pub(super) const WINDOW: usize = 400;
pub(super) const HOP: usize = 160;
const FFT: usize = 512;
pub(super) const STATS: usize = 360;

pub(super) struct Frontend {
    fft: Arc<dyn Fft<f32>>,
    window: [f32; WINDOW],
    filters: Vec<(usize, Vec<f32>)>,
}

impl Frontend {
    pub(super) fn new() -> Self {
        let mut filters = Vec::with_capacity(120);
        for (count, low, high) in [(40, 125.0, 7500.0), (80, 80.0, 7600.0)] {
            let low = crate::mel(low);
            let step = (crate::mel(high) - low) / (count + 1) as f32;
            for band in 0..count {
                let center = low + (band + 1) as f32 * step;
                let weights: Vec<_> = (1..=FFT / 2)
                    .map(|k| {
                        let at = crate::mel(k as f32 * crate::RATE as f32 / FFT as f32);
                        (1.0 - ((at - center) / step).abs()).max(0.0)
                    })
                    .collect();
                let start = weights.iter().position(|&w| w > 0.0).unwrap_or(0);
                let end = weights
                    .iter()
                    .rposition(|&w| w > 0.0)
                    .map_or(start, |i| i + 1);
                filters.push((start + 1, weights[start..end].to_vec()));
            }
        }
        Self {
            fft: FftPlanner::new().plan_fft_forward(FFT),
            window: std::array::from_fn(|i| {
                0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / WINDOW as f32).cos()
            }),
            filters,
        }
    }

    /// TensorFlow's periodic Hann, 0.97 pre-emphasis, magnitude and log(x + 1e-6).
    pub(super) fn frames<'a>(&'a self, pcm: &'a [f32]) -> impl Iterator<Item = [f32; 120]> + 'a {
        let mut buffer = [Complex::default(); FFT];
        let mut magnitudes = [0f32; FFT / 2 + 1];
        let mut previous_frame_last = 0.0;
        pcm.windows(WINDOW).step_by(HOP).map(move |window| {
            let mut previous = previous_frame_last;
            previous_frame_last = window[HOP - 1];
            for (i, &sample) in window.iter().enumerate() {
                buffer[i] = Complex::new((sample - 0.97 * previous) * self.window[i], 0.0);
                previous = sample;
            }
            buffer[WINDOW..].fill(Complex::default());
            self.fft.process(&mut buffer);
            for (magnitude, x) in magnitudes.iter_mut().zip(buffer.iter()) {
                *magnitude = x.norm();
            }
            std::array::from_fn(|k| {
                let (start, weights) = &self.filters[k];
                let sum: f32 = magnitudes[*start..]
                    .iter()
                    .zip(weights)
                    .map(|(x, w)| x * w)
                    .sum();
                (1e-6 + sum).ln()
            })
        })
    }
}

/// Weighted sufficient statistics for one frame or a sliding collection of frames.
#[derive(Clone)]
pub(super) struct Moments {
    weight: f64,
    values: [f64; STATS],
}

impl Moments {
    pub(super) fn zero() -> Self {
        Self {
            weight: 0.0,
            values: [0.0; STATS],
        }
    }

    pub(super) fn frame(
        mel: &[f32; 120],
        previous: &[f32; 40],
        mean: &[f32],
        threshold: f32,
    ) -> Self {
        let energy = mel[2..34].iter().sum::<f32>() / 32.0;
        let weight =
            f64::from(0.05 + 1.0 / (1.0 + (-1.5 * (energy - threshold)).clamp(-15.0, 15.0).exp()));
        let mut frame = Self {
            weight,
            values: [0.0; STATS],
        };
        for i in 0..40 {
            let x = mel[i];
            let diff = x - mean[i];
            for (section, feature) in [
                x,
                x * x,
                diff.max(0.0),
                (-diff).max(0.0),
                (x - previous[i]).powi(2),
            ]
            .into_iter()
            .enumerate()
            {
                frame.values[40 * section + i] = weight * f64::from(feature);
            }
        }
        for (i, &x) in mel[40..].iter().enumerate() {
            frame.values[200 + i] = weight * f64::from(x);
            frame.values[280 + i] = weight * f64::from(x * x);
        }
        frame
    }

    pub(super) fn accumulate(&mut self, frame: &Self, sign: f64) {
        self.weight += sign * frame.weight;
        for (sum, x) in self.values.iter_mut().zip(&frame.values) {
            *sum += sign * x;
        }
    }

    pub(super) fn summary(&self) -> [f32; STATS] {
        let mut raw = self.values.map(|v| (v / self.weight.max(1e-4)) as f32);
        for i in 0..40 {
            raw[40 + i] = (raw[40 + i] - raw[i].powi(2)).max(1e-4).sqrt();
            raw[160 + i] = raw[160 + i].max(1e-4).sqrt();
        }
        for i in 0..80 {
            raw[280 + i] = (raw[280 + i] - raw[200 + i].powi(2)).max(1e-4).sqrt();
        }
        raw
    }
}
