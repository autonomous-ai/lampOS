//! RMS level computation following RFC 6465.
//!
//! Ported from `modules/audio_processing/rms_level.h/cc`.

// Constants and computation functions used by test-only methods
// (average, average_and_peak). Production code only uses
// analyze_float + reset.
#[cfg(test)]
const MAX_SQUARED_LEVEL: f32 = 32768.0 * 32768.0;
#[cfg(test)]
const MIN_LEVEL: f32 = 1.995_262_3e-13;
#[cfg(test)]
const MIN_LEVEL_DB: i32 = 127;
#[cfg(test)]
const INAUDIBLE_BUT_NOT_MUTED: i32 = 126;

#[cfg(test)]
fn compute_rms(mean_square: f32) -> i32 {
    if mean_square <= MIN_LEVEL * MAX_SQUARED_LEVEL {
        return MIN_LEVEL_DB;
    }
    let mean_square_norm = mean_square / MAX_SQUARED_LEVEL;
    debug_assert!(mean_square_norm > MIN_LEVEL);
    let rms = 10.0 * mean_square_norm.log10();
    debug_assert!(rms <= 0.0);
    (-rms + 0.5) as i32
}

/// Average and peak RMS levels.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Levels {
    pub(crate) average: i32,
    pub(crate) peak: i32,
}

/// Computes RMS level in dBFS following RFC 6465.
#[derive(Debug)]
pub(crate) struct RmsLevel {
    sum_square: f32,
    sample_count: usize,
    max_sum_square: f32,
    block_size: Option<usize>,
}

impl RmsLevel {
    pub(crate) fn new() -> Self {
        Self {
            sum_square: 0.0,
            sample_count: 0,
            max_sum_square: 0.0,
            block_size: None,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.sum_square = 0.0;
        self.sample_count = 0;
        self.max_sum_square = 0.0;
        self.block_size = None;
    }

    /// Analyze a block of i16 samples.
    #[cfg(test)]
    pub(crate) fn analyze_i16(&mut self, data: &[i16]) {
        if data.is_empty() {
            return;
        }
        self.check_block_size(data.len());

        let sum_square: f32 = data.iter().map(|&s| (s as f32) * (s as f32)).sum();
        debug_assert!(sum_square >= 0.0);
        self.sum_square += sum_square;
        self.sample_count += data.len();
        self.max_sum_square = self.max_sum_square.max(sum_square);
    }

    /// Analyze a block of float samples (FloatS16 range).
    pub(crate) fn analyze_float(&mut self, data: &[f32]) {
        if data.is_empty() {
            return;
        }
        self.check_block_size(data.len());

        let mut sum_square = 0.0f32;
        for &sample in data {
            let tmp = sample.clamp(-32768.0, 32767.0) as i16;
            sum_square += (tmp as f32) * (tmp as f32);
        }
        debug_assert!(sum_square >= 0.0);
        self.sum_square += sum_square;
        self.sample_count += data.len();
        self.max_sum_square = self.max_sum_square.max(sum_square);
    }

    /// Record muted samples (all zeros).
    #[cfg(test)]
    pub(crate) fn analyze_muted(&mut self, length: usize) {
        self.check_block_size(length);
        self.sample_count += length;
    }

    /// Compute average RMS and reset.
    #[cfg(test)]
    pub(crate) fn average(&mut self) -> i32 {
        let have_samples = self.sample_count != 0;
        let mut rms = if have_samples {
            compute_rms(self.sum_square / self.sample_count as f32)
        } else {
            MIN_LEVEL_DB
        };

        if have_samples && rms == MIN_LEVEL_DB && self.sum_square != 0.0 {
            rms = INAUDIBLE_BUT_NOT_MUTED;
        }

        self.reset();
        rms
    }

    /// Compute average and peak RMS levels and reset.
    #[cfg(test)]
    pub(crate) fn average_and_peak(&mut self) -> Levels {
        let levels = if self.sample_count == 0 {
            Levels {
                average: MIN_LEVEL_DB,
                peak: MIN_LEVEL_DB,
            }
        } else {
            let block_size = self
                .block_size
                .expect("block_size should be set when sample_count > 0");
            Levels {
                average: compute_rms(self.sum_square / self.sample_count as f32),
                peak: compute_rms(self.max_sum_square / block_size as f32),
            }
        };
        self.reset();
        levels
    }

    fn check_block_size(&mut self, block_size: usize) {
        if self.block_size != Some(block_size) {
            self.reset();
            self.block_size = Some(block_size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE_HZ: usize = 48000;
    const BLOCK_SIZE_SAMPLES: usize = SAMPLE_RATE_HZ / 100;

    fn run_test_i16(input: &[i16]) -> RmsLevel {
        let mut level = RmsLevel::new();
        let mut n = 0;
        while n + BLOCK_SIZE_SAMPLES <= input.len() {
            level.analyze_i16(&input[n..n + BLOCK_SIZE_SAMPLES]);
            n += BLOCK_SIZE_SAMPLES;
        }
        level
    }

    fn run_test_float(input: &[f32]) -> RmsLevel {
        let mut level = RmsLevel::new();
        let mut n = 0;
        while n + BLOCK_SIZE_SAMPLES <= input.len() {
            level.analyze_float(&input[n..n + BLOCK_SIZE_SAMPLES]);
            n += BLOCK_SIZE_SAMPLES;
        }
        level
    }

    fn create_i16_sinusoid(frequency_hz: i32, amplitude: i32, num_samples: usize) -> Vec<i16> {
        use std::f64::consts::PI;

        (0..num_samples)
            .map(|n| {
                let val = amplitude as f64
                    * (2.0 * PI * n as f64 * frequency_hz as f64 / SAMPLE_RATE_HZ as f64).sin();
                val.clamp(i16::MIN as f64, i16::MAX as f64) as i16
            })
            .collect()
    }

    fn create_float_sinusoid(frequency_hz: i32, amplitude: i32, num_samples: usize) -> Vec<f32> {
        let x16 = create_i16_sinusoid(frequency_hz, amplitude, num_samples);
        x16.iter().map(|&s| s as f32).collect()
    }

    #[test]
    fn verify_identity_between_float_and_fix() {
        let x_f = create_float_sinusoid(1000, i16::MAX as i32, SAMPLE_RATE_HZ);
        let x_i = create_float_sinusoid(1000, i16::MAX as i32, SAMPLE_RATE_HZ);
        let mut level_f = run_test_float(&x_f);
        let mut level_i = run_test_float(&x_i);
        let avg_i = level_i.average();
        let avg_f = level_f.average();
        assert_eq!(3, avg_i);
        assert_eq!(avg_f, avg_i);
    }

    #[test]
    fn run_1000hz_full_scale() {
        let x = create_i16_sinusoid(1000, i16::MAX as i32, SAMPLE_RATE_HZ);
        let mut level = run_test_i16(&x);
        assert_eq!(3, level.average());
    }

    #[test]
    fn run_1000hz_full_scale_average_and_peak() {
        let x = create_i16_sinusoid(1000, i16::MAX as i32, SAMPLE_RATE_HZ);
        let mut level = run_test_i16(&x);
        let stats = level.average_and_peak();
        assert_eq!(3, stats.average);
        assert_eq!(3, stats.peak);
    }

    #[test]
    fn run_1000hz_half_scale() {
        let x = create_i16_sinusoid(1000, i16::MAX as i32 / 2, SAMPLE_RATE_HZ);
        let mut level = run_test_i16(&x);
        assert_eq!(9, level.average());
    }

    #[test]
    fn run_zeros() {
        let x = vec![0i16; SAMPLE_RATE_HZ];
        let mut level = run_test_i16(&x);
        assert_eq!(127, level.average());
    }

    #[test]
    fn run_zeros_average_and_peak() {
        let x = vec![0i16; SAMPLE_RATE_HZ];
        let mut level = run_test_i16(&x);
        let stats = level.average_and_peak();
        assert_eq!(127, stats.average);
        assert_eq!(127, stats.peak);
    }

    #[test]
    fn no_samples() {
        let mut level = RmsLevel::new();
        assert_eq!(127, level.average());
    }

    #[test]
    fn no_samples_average_and_peak() {
        let mut level = RmsLevel::new();
        let stats = level.average_and_peak();
        assert_eq!(127, stats.average);
        assert_eq!(127, stats.peak);
    }

    #[test]
    fn poll_twice() {
        let x = create_i16_sinusoid(1000, i16::MAX as i32, SAMPLE_RATE_HZ);
        let mut level = run_test_i16(&x);
        level.average();
        assert_eq!(127, level.average());
    }

    #[test]
    fn reset_test() {
        let x = create_i16_sinusoid(1000, i16::MAX as i32, SAMPLE_RATE_HZ);
        let mut level = run_test_i16(&x);
        level.reset();
        assert_eq!(127, level.average());
    }

    #[test]
    fn process_muted() {
        let x = create_i16_sinusoid(1000, i16::MAX as i32, SAMPLE_RATE_HZ);
        let mut level = run_test_i16(&x);
        let blocks_per_second = SAMPLE_RATE_HZ / BLOCK_SIZE_SAMPLES;
        for _ in 0..blocks_per_second {
            level.analyze_muted(BLOCK_SIZE_SAMPLES);
        }
        assert_eq!(6, level.average());
    }

    #[test]
    fn only_digital_silence_is_127() {
        let mut test_buffer = vec![0i16; SAMPLE_RATE_HZ];
        let mut level = run_test_i16(&test_buffer);
        assert_eq!(127, level.average());

        test_buffer[0] = 1;
        level = run_test_i16(&test_buffer);
        assert!(level.average() < 127);
    }

    #[test]
    fn run_half_scale_and_insert_full_scale() {
        let half_scale = create_i16_sinusoid(1000, i16::MAX as i32 / 2, SAMPLE_RATE_HZ);
        let full_scale = create_i16_sinusoid(1000, i16::MAX as i32, SAMPLE_RATE_HZ / 100);
        let mut x = half_scale.clone();
        x.extend_from_slice(&full_scale);
        x.extend_from_slice(&half_scale);
        assert_eq!(2 * SAMPLE_RATE_HZ + SAMPLE_RATE_HZ / 100, x.len());
        let mut level = run_test_i16(&x);
        let stats = level.average_and_peak();
        assert_eq!(9, stats.average);
        assert_eq!(3, stats.peak);
    }

    #[test]
    fn reset_on_block_size_change() {
        let x = create_i16_sinusoid(1000, i16::MAX as i32, SAMPLE_RATE_HZ);
        let mut level = run_test_i16(&x);
        let y = create_i16_sinusoid(1000, i16::MAX as i32 / 2, BLOCK_SIZE_SAMPLES * 2);
        level.analyze_i16(&y);
        let stats = level.average_and_peak();
        assert_eq!(9, stats.average);
        assert_eq!(9, stats.peak);
    }
}
