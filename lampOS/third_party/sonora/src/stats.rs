//! Audio processing statistics.
//!
//! Ported from `AudioProcessingStats` in
//! `api/audio/audio_processing_statistics.h`.

/// Statistics from the audio processing pipeline.
///
/// All fields are `Option` — `None` indicates the statistic is unavailable
/// (e.g. because the relevant component is disabled).
#[derive(Debug, Clone, Default)]
pub struct AudioProcessingStats {
    /// Echo Return Loss in dB: `ERL = 10 log10(P_far / P_echo)`.
    pub echo_return_loss: Option<f64>,
    /// Echo Return Loss Enhancement in dB: `ERLE = 10 log10(P_echo / P_out)`.
    pub echo_return_loss_enhancement: Option<f64>,
    /// Fraction of time that the AEC linear filter is divergent, in a 1-second
    /// non-overlapping aggregation window.
    pub divergent_filter_fraction: Option<f64>,
    /// Median delay estimate in milliseconds.
    ///
    /// The delay metrics are aggregated until the first call to
    /// [`AudioProcessing::statistics()`](crate::AudioProcessing::statistics)
    /// and afterwards aggregated and updated every second.
    pub delay_median_ms: Option<i32>,
    /// Standard deviation of the delay estimate in milliseconds.
    ///
    /// Uses the same aggregation window as [`delay_median_ms`](Self::delay_median_ms).
    pub delay_standard_deviation_ms: Option<i32>,
    /// Residual echo detector likelihood in `[0.0, 1.0]`.
    pub residual_echo_likelihood: Option<f64>,
    /// Maximum residual echo likelihood from the last time period.
    pub residual_echo_likelihood_recent_max: Option<f64>,
    /// Instantaneous delay estimate from the AEC in milliseconds.
    ///
    /// This is the value at the time of the call to
    /// [`AudioProcessing::statistics()`](crate::AudioProcessing::statistics),
    /// not an aggregated value.
    pub delay_ms: Option<i32>,
}
