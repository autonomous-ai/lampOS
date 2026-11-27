//! The official Light-ASD AVA frontend: unnormalised mono PCM16 at 16 kHz,
//! 25 ms rectangular windows every 10 ms, 26 mel filters and 13 MFCCs.
//! Matches python_speech_features 0.6, including the AVA loader's final
//! wrap padding. No microphone, resampler, labels or clock conversion.
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::sync::Arc;

pub const SAMPLE_RATE: usize = 16_000;
pub const COEFFICIENTS: usize = 13;
const WINDOW: usize = 400;
const STEP: usize = 160;
const FFT: usize = 512;
const FILTERS: usize = 26;

pub struct Mfcc {
    fft: Arc<dyn Fft<f64>>,
    filters: Vec<Vec<f64>>,
    dct: Vec<Vec<f64>>,
}

impl Default for Mfcc {
    fn default() -> Self {
        let mel = |hz: f64| 2595.0 * (1.0 + hz / 700.0).log10();
        let hz = |value: f64| 700.0 * (10.0_f64.powf(value / 2595.0) - 1.0);
        let bins: Vec<usize> = (0..FILTERS + 2)
            .map(|i| {
                (((FFT + 1) as f64 * hz(mel(8000.0) * i as f64 / (FILTERS + 1) as f64))
                    / SAMPLE_RATE as f64)
                    .floor() as usize
            })
            .collect();
        let mut filters = vec![vec![0.0; FFT / 2 + 1]; FILTERS];
        for (i, filter) in filters.iter_mut().enumerate() {
            for (k, value) in filter
                .iter_mut()
                .enumerate()
                .take(bins[i + 1])
                .skip(bins[i])
            {
                *value = (k - bins[i]) as f64 / (bins[i + 1] - bins[i]) as f64;
            }
            for (k, value) in filter
                .iter_mut()
                .enumerate()
                .take(bins[i + 2])
                .skip(bins[i + 1])
            {
                *value = (bins[i + 2] - k) as f64 / (bins[i + 2] - bins[i + 1]) as f64;
            }
        }
        let dct = (0..COEFFICIENTS)
            .map(|k| {
                let norm = if k == 0 {
                    (1.0 / FILTERS as f64).sqrt()
                } else {
                    (2.0 / FILTERS as f64).sqrt()
                };
                let lift = 1.0 + 11.0 * (std::f64::consts::PI * k as f64 / 22.0).sin();
                (0..FILTERS)
                    .map(|n| {
                        norm * lift
                            * (std::f64::consts::PI * k as f64 * (n as f64 + 0.5) / FILTERS as f64)
                                .cos()
                    })
                    .collect()
            })
            .collect();
        Self {
            fft: FftPlanner::new().plan_fft_forward(FFT),
            filters,
            dct,
        }
    }
}

impl Mfcc {
    /// Exactly one or two seconds. Full windows produce 99/199 feature
    /// rows, then repeat their first row to reach 4 rows per video frame,
    /// just as the official AVA loader does. This is not zero padding.
    pub fn extract(&self, pcm: &[i16], video_frames: usize) -> Result<Vec<f32>, &'static str> {
        if !matches!(video_frames, 25 | 50) || pcm.len() != video_frames * 640 {
            return Err("MFCC needs exactly 1 or 2 seconds of mono PCM16 at 16 kHz");
        }
        let mut emphasized = Vec::with_capacity(pcm.len());
        emphasized.push(f64::from(pcm[0]));
        for pair in pcm.windows(2) {
            emphasized.push(f64::from(pair[1]) - 0.97 * f64::from(pair[0]));
        }
        let count = 1 + (pcm.len() - WINDOW).div_ceil(STEP);
        let mut features = Vec::with_capacity(count * COEFFICIENTS);
        let mut input = vec![Complex::new(0.0, 0.0); FFT];
        let mut scratch = vec![Complex::default(); self.fft.get_inplace_scratch_len()];
        for start in (0..count).map(|n| n * STEP) {
            input.fill(Complex::default());
            for (n, value) in input.iter_mut().enumerate().take(WINDOW) {
                value.re = emphasized.get(start + n).copied().unwrap_or(0.0);
            }
            self.fft.process_with_scratch(&mut input, &mut scratch);
            let power: Vec<f64> = input[..=FFT / 2]
                .iter()
                .map(|value| value.norm_sqr() / FFT as f64)
                .collect();
            let nonzero_log = |value: f64| {
                if value == 0.0 {
                    f64::EPSILON.ln()
                } else {
                    value.ln()
                }
            };
            let energy = nonzero_log(power.iter().sum::<f64>());
            let banks: Vec<f64> = self
                .filters
                .iter()
                .map(|filter| {
                    nonzero_log(
                        filter
                            .iter()
                            .zip(&power)
                            .map(|(weight, value)| weight * value)
                            .sum::<f64>(),
                    )
                })
                .collect();
            for (k, weights) in self.dct.iter().enumerate() {
                let value = if k == 0 {
                    energy
                } else {
                    weights.iter().zip(&banks).map(|(w, x)| w * x).sum()
                };
                features.push(value as f32);
            }
        }
        let wanted = 4 * video_frames * COEFFICIENTS;
        let original = features.len();
        for index in original..wanted {
            features.push(features[(index - original) % original]);
        }
        Ok(features)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent golden values from the unchanged official dataLoader's
    /// load_audio with python_speech_features 0.6, checkpoint source
    /// ed38c232de5efe0261dbd68627c0ade7cdfe14eb. Synthetic PCM only.
    #[test]
    fn matches_the_official_frontend_at_start_middle_and_padded_end() {
        let fixtures: [(usize, usize, [f64; 13]); 5] = [
            (
                25,
                0,
                [
                    17.89239311,
                    -15.15007401,
                    -8.55025578,
                    -11.90336704,
                    -9.62010670,
                    -9.92119789,
                    -9.16670322,
                    -10.76351070,
                    -11.69807243,
                    -14.40094376,
                    -16.10317802,
                    -16.52005959,
                    -13.32702446,
                ],
            ),
            (
                25,
                50,
                [
                    17.84504128,
                    -16.41470337,
                    -10.78337955,
                    -15.26541042,
                    -14.01470470,
                    -15.05330563,
                    -14.45758724,
                    -15.64343929,
                    -15.46476746,
                    -16.61289787,
                    -16.65855598,
                    -15.76533985,
                    -11.87628841,
                ],
            ),
            (
                25,
                98,
                [
                    17.68974113,
                    -14.81356430,
                    -8.23941898,
                    -11.84656811,
                    -9.84526539,
                    -10.38220978,
                    -9.60474682,
                    -10.87338352,
                    -10.99485111,
                    -12.66612339,
                    -13.28916264,
                    -12.91468430,
                    -9.62934685,
                ],
            ),
            (
                50,
                100,
                [
                    17.85811615,
                    -14.68438530,
                    -7.88330841,
                    -11.11237621,
                    -8.74443150,
                    -8.83248806,
                    -7.70460701,
                    -8.82145596,
                    -9.22787476,
                    -11.36658859,
                    -12.60259914,
                    -12.67704201,
                    -9.39413548,
                ],
            ),
            (
                50,
                198,
                [
                    17.69466400,
                    -15.61934853,
                    -9.51019764,
                    -13.51445198,
                    -11.81347084,
                    -12.43039703,
                    -11.49077797,
                    -12.47133636,
                    -12.39776421,
                    -13.87273598,
                    -14.47091675,
                    -14.10332489,
                    -10.62558079,
                ],
            ),
        ];
        let extractor = Mfcc::default();
        for (frames, row, expected) in fixtures {
            let pcm: Vec<i16> = (0..frames * 640)
                .map(|i| ((i * 73) % 4096) as i16 - 2048)
                .collect();
            let actual = extractor.extract(&pcm, frames).unwrap();
            for (i, value) in expected.into_iter().enumerate() {
                assert!(
                    (f64::from(actual[row * COEFFICIENTS + i]) - value).abs() < 2e-5,
                    "frames={frames} row={row} coefficient={i}"
                );
            }
        }
    }

    #[test]
    fn silence_is_finite_and_keeps_the_log_energy_floor() {
        let output = Mfcc::default().extract(&vec![0; SAMPLE_RATE], 25).unwrap();
        assert_eq!(output.len(), 100 * COEFFICIENTS);
        assert!(output.iter().all(|value| value.is_finite()));
        assert!((output[0] - f64::EPSILON.ln() as f32).abs() < 1e-5);
    }

    #[test]
    fn final_row_wraps_the_first_and_invalid_durations_are_refused() {
        let extractor = Mfcc::default();
        let pcm: Vec<i16> = (0..SAMPLE_RATE)
            .map(|i| ((i * 73) % 4096) as i16 - 2048)
            .collect();
        let output = extractor.extract(&pcm, 25).unwrap();
        assert_eq!(&output[..COEFFICIENTS], &output[99 * COEFFICIENTS..]);
        assert!(extractor.extract(&pcm[..100], 25).is_err());
        assert!(extractor.extract(&pcm, 24).is_err());
    }
}
