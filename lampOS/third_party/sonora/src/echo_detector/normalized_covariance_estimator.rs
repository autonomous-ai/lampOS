//! Normalized covariance estimator using exponential smoothing.
//!
//! Estimates the Pearson product-moment correlation coefficient between two signals.
//!
//! Ported from `modules/audio_processing/echo_detector/normalized_covariance_estimator.h/cc`.

/// Adaptation speed parameter.
const ALPHA: f32 = 0.001;

/// Iteratively estimates the normalized covariance between two signals.
#[derive(Debug)]
pub(crate) struct NormalizedCovarianceEstimator {
    normalized_cross_correlation: f32,
    covariance: f32,
}

impl NormalizedCovarianceEstimator {
    pub(crate) fn new() -> Self {
        Self {
            normalized_cross_correlation: 0.0,
            covariance: 0.0,
        }
    }

    pub(crate) fn update(
        &mut self,
        x: f32,
        x_mean: f32,
        x_sigma: f32,
        y: f32,
        y_mean: f32,
        y_sigma: f32,
    ) {
        self.covariance = (1.0 - ALPHA) * self.covariance + ALPHA * (x - x_mean) * (y - y_mean);
        self.normalized_cross_correlation = self.covariance / (x_sigma * y_sigma + 0.0001);
        debug_assert!(self.covariance.is_finite());
        debug_assert!(self.normalized_cross_correlation.is_finite());
    }

    pub(crate) fn normalized_cross_correlation(&self) -> f32 {
        self.normalized_cross_correlation
    }

    pub(crate) fn clear(&mut self) {
        self.covariance = 0.0;
        self.normalized_cross_correlation = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_signal() {
        let mut est = NormalizedCovarianceEstimator::new();
        for _ in 0..10000 {
            est.update(1.0, 0.0, 1.0, 1.0, 0.0, 1.0);
            est.update(-1.0, 0.0, 1.0, -1.0, 0.0, 1.0);
        }
        assert!(
            (est.normalized_cross_correlation() - 1.0).abs() < 0.01,
            "expected ~1.0, got {}",
            est.normalized_cross_correlation(),
        );
        est.clear();
        assert_eq!(est.normalized_cross_correlation(), 0.0);
    }

    #[test]
    fn opposite_signal() {
        let mut est = NormalizedCovarianceEstimator::new();
        for _ in 0..10000 {
            est.update(1.0, 0.0, 1.0, -1.0, 0.0, 1.0);
            est.update(-1.0, 0.0, 1.0, 1.0, 0.0, 1.0);
        }
        assert!(
            (est.normalized_cross_correlation() + 1.0).abs() < 0.01,
            "expected ~-1.0, got {}",
            est.normalized_cross_correlation(),
        );
    }
}
