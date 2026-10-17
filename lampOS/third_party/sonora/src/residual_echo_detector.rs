//! Residual echo detector using normalized covariance analysis.
//!
//! Ported from `modules/audio_processing/residual_echo_detector.h/cc`.

use crate::echo_detector::circular_buffer::CircularBuffer;
use crate::echo_detector::mean_variance_estimator::MeanVarianceEstimator;
use crate::echo_detector::moving_max::MovingMax;
use crate::echo_detector::normalized_covariance_estimator::NormalizedCovarianceEstimator;

const LOOKBACK_FRAMES: usize = 650;
const RENDER_BUFFER_SIZE: usize = 30;
const ALPHA: f32 = 0.001;
/// 10 seconds of data, updated every 10 ms.
const AGGREGATION_BUFFER_SIZE: usize = 10 * 100;

fn power(input: &[f32]) -> f32 {
    if input.is_empty() {
        return 0.0;
    }
    let sum: f32 = input.iter().map(|x| x * x).sum();
    sum / input.len() as f32
}

/// Echo detector metrics.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EchoDetectorMetrics {
    pub(crate) echo_likelihood: Option<f32>,
    pub(crate) echo_likelihood_recent_max: Option<f32>,
}

/// Residual echo detector that estimates echo likelihood from render/capture signals.
#[derive(Debug)]
pub(crate) struct ResidualEchoDetector {
    first_process_call: bool,
    render_buffer: CircularBuffer,
    frames_since_zero_buffer_size: usize,
    render_power: Vec<f32>,
    render_power_mean: Vec<f32>,
    render_power_std_dev: Vec<f32>,
    covariances: Vec<NormalizedCovarianceEstimator>,
    next_insertion_index: usize,
    render_statistics: MeanVarianceEstimator,
    capture_statistics: MeanVarianceEstimator,
    echo_likelihood: f32,
    reliability: f32,
    recent_likelihood_max: MovingMax,
    log_counter: i32,
}

impl ResidualEchoDetector {
    pub(crate) fn new() -> Self {
        Self {
            first_process_call: true,
            render_buffer: CircularBuffer::new(RENDER_BUFFER_SIZE),
            frames_since_zero_buffer_size: 0,
            render_power: vec![0.0; LOOKBACK_FRAMES],
            render_power_mean: vec![0.0; LOOKBACK_FRAMES],
            render_power_std_dev: vec![0.0; LOOKBACK_FRAMES],
            covariances: (0..LOOKBACK_FRAMES)
                .map(|_| NormalizedCovarianceEstimator::new())
                .collect(),
            next_insertion_index: 0,
            render_statistics: MeanVarianceEstimator::new(),
            capture_statistics: MeanVarianceEstimator::new(),
            echo_likelihood: 0.0,
            reliability: 0.0,
            recent_likelihood_max: MovingMax::new(AGGREGATION_BUFFER_SIZE),
            log_counter: 0,
        }
    }

    /// Analyze render (far-end) audio. Should be called while holding the render lock.
    pub(crate) fn analyze_render_audio(&mut self, render_audio: &[f32]) {
        if self.render_buffer.size() == 0 {
            self.frames_since_zero_buffer_size = 0;
        } else if self.frames_since_zero_buffer_size >= RENDER_BUFFER_SIZE {
            self.render_buffer.pop();
            self.frames_since_zero_buffer_size = 0;
        }
        self.frames_since_zero_buffer_size += 1;
        let pwr = power(render_audio);
        self.render_buffer.push(pwr);
    }

    /// Analyze capture (near-end) audio. Should be called while holding the capture lock.
    pub(crate) fn analyze_capture_audio(&mut self, capture_audio: &[f32]) {
        if self.first_process_call {
            self.render_buffer.clear();
            self.first_process_call = false;
        }

        let Some(buffered_render_power) = self.render_buffer.pop() else {
            return;
        };

        // Update render statistics and store in circular buffers.
        self.render_statistics.update(buffered_render_power);
        debug_assert!(self.next_insertion_index < LOOKBACK_FRAMES);
        self.render_power[self.next_insertion_index] = buffered_render_power;
        self.render_power_mean[self.next_insertion_index] = self.render_statistics.mean();
        self.render_power_std_dev[self.next_insertion_index] =
            self.render_statistics.std_deviation();

        // Get capture power and update capture statistics.
        let capture_power = power(capture_audio);
        self.capture_statistics.update(capture_power);
        let capture_mean = self.capture_statistics.mean();
        let capture_std_deviation = self.capture_statistics.std_deviation();

        // Update covariance values and determine echo likelihood.
        self.echo_likelihood = 0.0;
        let mut read_index = self.next_insertion_index;

        for delay in 0..self.covariances.len() {
            debug_assert!(read_index < self.render_power.len());
            self.covariances[delay].update(
                capture_power,
                capture_mean,
                capture_std_deviation,
                self.render_power[read_index],
                self.render_power_mean[read_index],
                self.render_power_std_dev[read_index],
            );
            read_index = if read_index > 0 {
                read_index - 1
            } else {
                LOOKBACK_FRAMES - 1
            };

            if self.covariances[delay].normalized_cross_correlation() > self.echo_likelihood {
                self.echo_likelihood = self.covariances[delay].normalized_cross_correlation();
            }
        }

        if self.echo_likelihood > 1.1 && self.log_counter < 5 {
            tracing::error!(
                echo_likelihood = self.echo_likelihood,
                reliability = self.reliability,
                "echo detector internal state: echo likelihood > 1.1"
            );
            self.log_counter += 1;
        }
        debug_assert!(self.echo_likelihood < 1.1);

        self.reliability = (1.0 - ALPHA) * self.reliability + ALPHA;
        self.echo_likelihood *= self.reliability;
        self.echo_likelihood = self.echo_likelihood.min(1.0);

        self.recent_likelihood_max.update(self.echo_likelihood);

        self.next_insertion_index = if self.next_insertion_index < (LOOKBACK_FRAMES - 1) {
            self.next_insertion_index + 1
        } else {
            0
        };
    }

    /// Initialize/reset the detector.
    pub(crate) fn initialize(&mut self) {
        self.render_buffer.clear();
        self.render_power.fill(0.0);
        self.render_power_mean.fill(0.0);
        self.render_power_std_dev.fill(0.0);
        self.render_statistics.clear();
        self.capture_statistics.clear();
        self.recent_likelihood_max.clear();
        for cov in &mut self.covariances {
            cov.clear();
        }
        self.echo_likelihood = 0.0;
        self.next_insertion_index = 0;
        self.reliability = 0.0;
    }

    /// Get current echo detection metrics.
    pub(crate) fn get_metrics(&self) -> EchoDetectorMetrics {
        EchoDetectorMetrics {
            echo_likelihood: Some(self.echo_likelihood),
            echo_likelihood_recent_max: Some(self.recent_likelihood_max.max()),
        }
    }

    /// Set reliability directly (for testing only).
    #[cfg(test)]
    fn set_reliability_for_test(&mut self, value: f32) {
        self.reliability = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo() {
        let mut echo_detector = ResidualEchoDetector::new();
        echo_detector.set_reliability_for_test(1.0);
        let ones = vec![1.0f32; 160];
        let zeros = vec![0.0f32; 160];

        for i in 0..1000 {
            if i % 20 == 0 {
                echo_detector.analyze_render_audio(&ones);
                echo_detector.analyze_capture_audio(&zeros);
            } else if i % 20 == 10 {
                echo_detector.analyze_render_audio(&zeros);
                echo_detector.analyze_capture_audio(&ones);
            } else {
                echo_detector.analyze_render_audio(&zeros);
                echo_detector.analyze_capture_audio(&zeros);
            }
        }

        let metrics = echo_detector.get_metrics();
        assert!(metrics.echo_likelihood.is_some());
        let likelihood = metrics.echo_likelihood.unwrap();
        assert!(
            (likelihood - 1.0).abs() < 0.01,
            "expected near 1.0, got {likelihood}",
        );
    }

    #[test]
    fn no_echo() {
        let mut echo_detector = ResidualEchoDetector::new();
        echo_detector.set_reliability_for_test(1.0);
        let ones = vec![1.0f32; 160];
        let zeros = vec![0.0f32; 160];

        for i in 0..1000 {
            if i % 20 == 0 {
                echo_detector.analyze_render_audio(&ones);
            } else {
                echo_detector.analyze_render_audio(&zeros);
            }
            echo_detector.analyze_capture_audio(&zeros);
        }

        let metrics = echo_detector.get_metrics();
        assert!(metrics.echo_likelihood.is_some());
        let likelihood = metrics.echo_likelihood.unwrap();
        assert!(
            likelihood.abs() < 0.01,
            "expected near 0.0, got {likelihood}",
        );
    }

    #[test]
    fn echo_with_render_clock_drift() {
        let mut echo_detector = ResidualEchoDetector::new();
        echo_detector.set_reliability_for_test(1.0);
        let ones = vec![1.0f32; 160];
        let zeros = vec![0.0f32; 160];

        for i in 0..1000 {
            if i % 20 == 0 {
                echo_detector.analyze_render_audio(&ones);
                echo_detector.analyze_capture_audio(&zeros);
            } else if i % 20 == 10 {
                echo_detector.analyze_render_audio(&zeros);
                echo_detector.analyze_capture_audio(&ones);
            } else {
                echo_detector.analyze_render_audio(&zeros);
                echo_detector.analyze_capture_audio(&zeros);
            }
            if i % 100 == 0 {
                echo_detector.analyze_render_audio(&zeros);
            }
        }

        let metrics = echo_detector.get_metrics();
        assert!(metrics.echo_likelihood.is_some());
        let likelihood = metrics.echo_likelihood.unwrap();
        assert!(likelihood > 0.75, "expected > 0.75, got {likelihood}",);
    }

    #[test]
    fn echo_with_capture_clock_drift() {
        let mut echo_detector = ResidualEchoDetector::new();
        echo_detector.set_reliability_for_test(1.0);
        let ones = vec![1.0f32; 160];
        let zeros = vec![0.0f32; 160];

        for i in 0..1000 {
            if i % 20 == 0 {
                echo_detector.analyze_render_audio(&ones);
                echo_detector.analyze_capture_audio(&zeros);
            } else if i % 20 == 10 {
                echo_detector.analyze_render_audio(&zeros);
                echo_detector.analyze_capture_audio(&ones);
            } else {
                echo_detector.analyze_render_audio(&zeros);
                echo_detector.analyze_capture_audio(&zeros);
            }
            if i % 100 == 0 {
                echo_detector.analyze_capture_audio(&zeros);
            }
        }

        let metrics = echo_detector.get_metrics();
        assert!(metrics.echo_likelihood.is_some());
        let likelihood = metrics.echo_likelihood.unwrap();
        assert!(
            (likelihood - 1.0).abs() < 0.01,
            "expected near 1.0, got {likelihood}",
        );
    }
}
