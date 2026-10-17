//! Online mean and variance estimator using exponential smoothing.
//!
//! Ported from `modules/audio_processing/echo_detector/mean_variance_estimator.h/cc`.

/// Adaptation speed parameter.
const ALPHA: f32 = 0.001;

/// Iteratively estimates the mean and variance of a signal.
#[derive(Debug)]
pub(crate) struct MeanVarianceEstimator {
    mean: f32,
    variance: f32,
}

impl MeanVarianceEstimator {
    pub(crate) fn new() -> Self {
        Self {
            mean: 0.0,
            variance: 0.0,
        }
    }

    pub(crate) fn update(&mut self, value: f32) {
        self.mean = (1.0 - ALPHA) * self.mean + ALPHA * value;
        self.variance =
            (1.0 - ALPHA) * self.variance + ALPHA * (value - self.mean) * (value - self.mean);
        debug_assert!(self.mean.is_finite());
        debug_assert!(self.variance.is_finite());
    }

    pub(crate) fn std_deviation(&self) -> f32 {
        debug_assert!(self.variance >= 0.0);
        self.variance.sqrt()
    }

    pub(crate) fn mean(&self) -> f32 {
        self.mean
    }

    pub(crate) fn clear(&mut self) {
        self.mean = 0.0;
        self.variance = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_two_values() {
        let mut est = MeanVarianceEstimator::new();
        est.update(3.0);
        est.update(5.0);
        assert!(est.mean() > 0.0);
        assert!(est.std_deviation() > 0.0);
        est.clear();
        assert_eq!(est.mean(), 0.0);
        assert_eq!(est.std_deviation(), 0.0);
    }

    #[test]
    fn insert_zeroes() {
        let mut est = MeanVarianceEstimator::new();
        for _ in 0..20000 {
            est.update(0.0);
        }
        assert_eq!(est.mean(), 0.0);
        assert_eq!(est.std_deviation(), 0.0);
    }

    #[test]
    fn constant_value() {
        let mut est = MeanVarianceEstimator::new();
        for _ in 0..20000 {
            est.update(3.0);
        }
        assert!((est.mean() - 3.0).abs() < 0.01);
        assert!(est.std_deviation() < 0.01);
    }

    #[test]
    fn alternating_value() {
        let mut est = MeanVarianceEstimator::new();
        for _ in 0..20000 {
            est.update(1.0);
            est.update(-1.0);
        }
        assert!(est.mean().abs() < 0.01);
        assert!((est.std_deviation() - 1.0).abs() < 0.01);
    }
}
