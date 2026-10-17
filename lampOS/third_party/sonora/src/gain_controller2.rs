//! Gain Controller 2 — orchestrates AGC2 subcomponents.
//!
//! Coordinates fixed gain, adaptive digital gain, input volume control,
//! VAD, noise/speech level estimation, saturation protection, and limiting.
//!
//! Ported from `modules/audio_processing/gain_controller2.h/cc`.

use crate::audio_buffer::AudioBuffer;
use crate::input_volume_controller::{InputVolumeController, InputVolumeControllerConfig};
use sonora_agc2::adaptive_digital_gain_controller::{AdaptiveDigitalGainController, FrameInfo};
use sonora_agc2::common::{
    ADJACENT_SPEECH_FRAMES_THRESHOLD, SATURATION_PROTECTOR_INITIAL_HEADROOM_DB,
    VAD_RESET_PERIOD_MS, db_to_ratio, float_s16_to_dbfs,
};
use sonora_agc2::gain_applier::GainApplier;
use sonora_agc2::limiter::Limiter;
use sonora_agc2::noise_level_estimator::NoiseLevelEstimator;
use sonora_agc2::saturation_protector::SaturationProtector;
use sonora_agc2::speech_level_estimator::{AdaptiveDigitalConfig, SpeechLevelEstimator};
use sonora_agc2::vad_wrapper::VoiceActivityDetectorWrapper;
use sonora_simd::detect_backend;

/// Configuration for the fixed digital controller.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FixedDigitalConfig {
    pub gain_db: f32,
}

impl Default for FixedDigitalConfig {
    fn default() -> Self {
        Self { gain_db: 0.0 }
    }
}

/// Configuration for GainController2's input volume controller.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Agc2InputVolumeControllerConfig {
    pub enabled: bool,
}

/// Top-level configuration for GainController2.
#[derive(Debug, Clone, Default)]
pub(crate) struct Agc2Config {
    pub fixed_digital: FixedDigitalConfig,
    pub adaptive_digital: Agc2AdaptiveDigitalConfig,
    pub input_volume_controller: Agc2InputVolumeControllerConfig,
}

/// Adaptive digital controller configuration (mirrors C++ API struct).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Agc2AdaptiveDigitalConfig {
    pub enabled: bool,
    pub headroom_db: f32,
    pub max_gain_db: f32,
    pub initial_gain_db: f32,
    pub max_gain_change_db_per_second: f32,
    pub max_output_noise_level_dbfs: f32,
}

impl Default for Agc2AdaptiveDigitalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            headroom_db: 5.0,
            max_gain_db: 50.0,
            initial_gain_db: 15.0,
            max_gain_change_db_per_second: 6.0,
            max_output_noise_level_dbfs: -50.0,
        }
    }
}

impl From<&Agc2AdaptiveDigitalConfig> for AdaptiveDigitalConfig {
    fn from(c: &Agc2AdaptiveDigitalConfig) -> Self {
        Self {
            headroom_db: c.headroom_db,
            max_gain_db: c.max_gain_db,
            initial_gain_db: c.initial_gain_db,
            max_gain_change_db_per_second: c.max_gain_change_db_per_second,
            max_output_noise_level_dbfs: c.max_output_noise_level_dbfs,
        }
    }
}

/// Peak and RMS audio levels in dBFS.
struct AudioLevels {
    peak_dbfs: f32,
    rms_dbfs: f32,
}

/// Computes the audio levels for the first channel.
fn compute_audio_levels(audio: &AudioBuffer) -> AudioLevels {
    let channel = audio.channel(0);
    let num_samples = audio.num_frames();
    let mut peak: f32 = 0.0;
    let mut rms: f32 = 0.0;
    for &x in &channel[..num_samples] {
        peak = peak.max(x.abs());
        rms += x * x;
    }
    AudioLevels {
        peak_dbfs: float_s16_to_dbfs(peak),
        rms_dbfs: float_s16_to_dbfs((rms / num_samples as f32).sqrt()),
    }
}

/// Copies channel data out of an AudioBuffer for use with `&mut [&mut [f32]]` APIs.
fn extract_channels(audio: &AudioBuffer) -> Vec<Vec<f32>> {
    (0..audio.num_channels())
        .map(|ch| audio.channel(ch).to_vec())
        .collect()
}

/// Writes channel data back into an AudioBuffer.
fn write_back_channels(audio: &mut AudioBuffer, channel_data: &[Vec<f32>]) {
    let num_frames = audio.num_frames();
    for (ch, data) in channel_data.iter().enumerate() {
        audio.channel_mut(ch)[..num_frames].copy_from_slice(&data[..num_frames]);
    }
}

/// Gain Controller 2 aims to automatically adjust levels by acting on the
/// microphone gain and/or applying digital gain.
#[derive(Debug)]
pub(crate) struct GainController2 {
    fixed_gain_applier: GainApplier,
    noise_level_estimator: Option<NoiseLevelEstimator>,
    vad: Option<VoiceActivityDetectorWrapper>,
    speech_level_estimator: Option<SpeechLevelEstimator>,
    input_volume_controller: Option<InputVolumeController>,
    saturation_protector: Option<SaturationProtector>,
    adaptive_digital_controller: Option<AdaptiveDigitalGainController>,
    limiter: Limiter,
    recommended_input_volume: Option<i32>,
}

impl GainController2 {
    /// Creates a new GainController2.
    ///
    /// If `use_internal_vad` is true, an internal voice activity detector is
    /// used for digital adaptive gain.
    pub(crate) fn new(
        config: &Agc2Config,
        input_volume_controller_config: &InputVolumeControllerConfig,
        sample_rate_hz: usize,
        num_channels: usize,
        use_internal_vad: bool,
    ) -> Self {
        debug_assert!(Self::validate(config));
        let samples_per_channel = sample_rate_hz / 100;

        let mut speech_level_estimator = None;
        let mut vad = None;

        if config.input_volume_controller.enabled || config.adaptive_digital.enabled {
            let adaptive_config = AdaptiveDigitalConfig::from(&config.adaptive_digital);
            speech_level_estimator = Some(SpeechLevelEstimator::new(
                &adaptive_config,
                ADJACENT_SPEECH_FRAMES_THRESHOLD,
            ));
            if use_internal_vad {
                let backend = detect_backend();
                vad = Some(VoiceActivityDetectorWrapper::with_reset_period(
                    VAD_RESET_PERIOD_MS,
                    backend,
                    sample_rate_hz as i32,
                ));
            }
        }

        let mut input_volume_controller = None;
        if config.input_volume_controller.enabled {
            let mut ivc = InputVolumeController::new(num_channels, input_volume_controller_config);
            ivc.initialize();
            input_volume_controller = Some(ivc);
        }

        let mut noise_level_estimator = None;
        let mut saturation_protector = None;
        let mut adaptive_digital_controller = None;
        if config.adaptive_digital.enabled {
            noise_level_estimator = Some(NoiseLevelEstimator::default());
            saturation_protector = Some(SaturationProtector::new(
                SATURATION_PROTECTOR_INITIAL_HEADROOM_DB,
                ADJACENT_SPEECH_FRAMES_THRESHOLD,
            ));
            let adaptive_config = AdaptiveDigitalConfig::from(&config.adaptive_digital);
            adaptive_digital_controller = Some(AdaptiveDigitalGainController::new(
                adaptive_config,
                ADJACENT_SPEECH_FRAMES_THRESHOLD,
            ));
        }

        Self {
            fixed_gain_applier: GainApplier::new(false, db_to_ratio(config.fixed_digital.gain_db)),
            noise_level_estimator,
            vad,
            speech_level_estimator,
            input_volume_controller,
            saturation_protector,
            adaptive_digital_controller,
            limiter: Limiter::new(samples_per_channel),
            recommended_input_volume: None,
        }
    }

    /// Sets the fixed digital gain.
    pub(crate) fn set_fixed_gain_db(&mut self, gain_db: f32) {
        let gain_factor = db_to_ratio(gain_db);
        if self.fixed_gain_applier.get_gain_factor() != gain_factor {
            self.limiter.reset();
        }
        self.fixed_gain_applier.set_gain_factor(gain_factor);
    }

    /// Updates the input volume controller about whether the capture output is
    /// used or not.
    pub(crate) fn set_capture_output_used(&mut self, capture_output_used: bool) {
        if let Some(ref mut ivc) = self.input_volume_controller {
            ivc.handle_capture_output_used_change(capture_output_used);
        }
    }

    /// Analyzes `audio_buffer` before `process()` is called so that the
    /// analysis can be performed before digital processing operations take
    /// place (e.g., echo cancellation). The analysis consists of input
    /// clipping detection and prediction (if enabled).
    pub(crate) fn analyze(&mut self, applied_input_volume: i32, audio_buffer: &AudioBuffer) {
        self.recommended_input_volume = None;
        debug_assert!((0..=255).contains(&applied_input_volume));
        if let Some(ref mut ivc) = self.input_volume_controller {
            ivc.analyze_input_audio(applied_input_volume, audio_buffer);
        }
    }

    /// Updates the recommended input volume, applies the adaptive digital and
    /// the fixed digital gains, and runs a limiter on `audio`.
    pub(crate) fn process(&mut self, input_volume_changed: bool, audio: &mut AudioBuffer) {
        self.recommended_input_volume = None;

        if input_volume_changed {
            if let Some(ref mut sle) = self.speech_level_estimator {
                sle.reset();
            }
            if let Some(ref mut sp) = self.saturation_protector {
                sp.reset();
            }
        }

        // Compute speech probability.
        let speech_probability = if let Some(ref mut vad) = self.vad {
            let first_channel = audio.channel(0);
            vad.analyze(first_channel)
        } else {
            0.0
        };

        // Compute audio levels from the first channel.
        let audio_levels = compute_audio_levels(audio);

        // Noise level estimation.
        let noise_rms_dbfs = if let Some(ref mut nle) = self.noise_level_estimator {
            let num_ch = audio.num_channels();
            let channels: Vec<&[f32]> = (0..num_ch).map(|ch| audio.channel(ch)).collect();
            Some(nle.analyze(&channels))
        } else {
            None
        };

        // Speech level estimation.
        let speech_level = if let Some(ref mut sle) = self.speech_level_estimator {
            sle.update(audio_levels.rms_dbfs, speech_probability);
            Some((sle.is_confident(), sle.level_dbfs()))
        } else {
            None
        };

        // Update the recommended input volume.
        if let (Some(ivc), Some((is_confident, rms_dbfs))) =
            (&mut self.input_volume_controller, speech_level)
        {
            let speech_level_opt = if is_confident { Some(rms_dbfs) } else { None };
            self.recommended_input_volume =
                ivc.recommend_input_volume(speech_probability, speech_level_opt);
        }

        // Adaptive digital controller.
        if let Some(ref mut adc) = self.adaptive_digital_controller {
            let sp = self
                .saturation_protector
                .as_mut()
                .expect("saturation protector must exist when adaptive digital controller exists");
            let (is_confident, rms_dbfs) = speech_level
                .expect("speech level must exist when adaptive digital controller exists");
            sp.analyze(speech_probability, audio_levels.peak_dbfs, rms_dbfs);
            let headroom_db = sp.headroom_db();
            let limiter_envelope_dbfs = float_s16_to_dbfs(self.limiter.last_audio_level());
            let noise = noise_rms_dbfs
                .expect("noise level must exist when adaptive digital controller exists");

            let info = FrameInfo {
                speech_probability,
                speech_level_dbfs: rms_dbfs,
                speech_level_reliable: is_confident,
                noise_rms_dbfs: noise,
                headroom_db,
                limiter_envelope_dbfs,
            };

            // Build mutable channel slices for the adaptive controller.
            let mut channel_data = extract_channels(audio);
            let mut channel_slices: Vec<&mut [f32]> =
                channel_data.iter_mut().map(|v| v.as_mut_slice()).collect();
            adc.process(&info, &mut channel_slices);
            write_back_channels(audio, &channel_data);
        }

        // Fixed gain.
        {
            let mut channel_data = extract_channels(audio);
            let mut channel_slices: Vec<&mut [f32]> =
                channel_data.iter_mut().map(|v| v.as_mut_slice()).collect();
            self.fixed_gain_applier.apply_gain(&mut channel_slices);
            write_back_channels(audio, &channel_data);
        }

        // Limiter.
        {
            let mut channel_data = extract_channels(audio);
            let mut channel_slices: Vec<&mut [f32]> =
                channel_data.iter_mut().map(|v| v.as_mut_slice()).collect();
            self.limiter.process(&mut channel_slices);
            write_back_channels(audio, &channel_data);
        }
    }

    /// Validates the configuration.
    pub(crate) fn validate(config: &Agc2Config) -> bool {
        let fixed = &config.fixed_digital;
        let adaptive = &config.adaptive_digital;
        fixed.gain_db >= 0.0
            && fixed.gain_db < 50.0
            && adaptive.headroom_db >= 0.0
            && adaptive.max_gain_db > 0.0
            && adaptive.initial_gain_db >= 0.0
            && adaptive.max_gain_change_db_per_second > 0.0
            && adaptive.max_output_noise_level_dbfs <= 0.0
    }

    /// Returns the recommended input volume, if available.
    pub(crate) fn recommended_input_volume(&self) -> Option<i32> {
        self.recommended_input_volume
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonora_agc2::common::LIMITER_MAX_INPUT_LEVEL_DB_FS;

    /// Sets all samples in `ab` to `value`.
    fn set_audio_buffer_samples(value: f32, ab: &mut AudioBuffer) {
        for ch in 0..ab.num_channels() {
            let channel = ab.channel_mut(ch);
            for sample in channel.iter_mut() {
                *sample = value;
            }
        }
    }

    fn run_agc2_with_constant_input(
        agc2: &mut GainController2,
        input_level: f32,
        num_frames: usize,
        sample_rate_hz: usize,
        num_channels: usize,
        applied_initial_volume: i32,
    ) -> f32 {
        let num_samples = sample_rate_hz / 100;
        let mut ab = AudioBuffer::new(
            sample_rate_hz,
            num_channels,
            sample_rate_hz,
            num_channels,
            sample_rate_hz,
        );

        for _ in 0..num_frames + 1 {
            set_audio_buffer_samples(input_level, &mut ab);
            let applied_volume = agc2
                .recommended_input_volume()
                .unwrap_or(applied_initial_volume);
            agc2.analyze(applied_volume, &ab);
            agc2.process(false, &mut ab);
        }

        ab.channel(0)[num_samples - 1]
    }

    fn create_agc2_fixed_digital_mode(
        fixed_gain_db: f32,
        sample_rate_hz: usize,
    ) -> GainController2 {
        let config = Agc2Config {
            fixed_digital: FixedDigitalConfig {
                gain_db: fixed_gain_db,
            },
            adaptive_digital: Agc2AdaptiveDigitalConfig {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(GainController2::validate(&config));
        GainController2::new(
            &config,
            &InputVolumeControllerConfig::default(),
            sample_rate_hz,
            1,
            true,
        )
    }

    const TEST_INPUT_VOLUME_CONTROLLER_CONFIG: InputVolumeControllerConfig =
        InputVolumeControllerConfig {
            min_input_volume: 20,
            clipped_level_min: 20,
            clipped_level_step: 30,
            clipped_ratio_threshold: 0.4,
            clipped_wait_frames: 50,
            enable_clipping_predictor: true,
            target_range_max_dbfs: -6,
            target_range_min_dbfs: -70,
            update_input_volume_wait_frames: 100,
            speech_probability_threshold: 0.9,
            speech_ratio_threshold: 1.0,
        };

    // --- Validation tests ---

    #[test]
    fn check_default_config() {
        let config = Agc2Config::default();
        assert!(GainController2::validate(&config));
    }

    #[test]
    fn check_fixed_digital_config() {
        let mut config = Agc2Config::default();
        // Attenuation is not allowed.
        config.fixed_digital.gain_db = -5.0;
        assert!(!GainController2::validate(&config));
        // No gain is allowed.
        config.fixed_digital.gain_db = 0.0;
        assert!(GainController2::validate(&config));
        // Positive gain is allowed.
        config.fixed_digital.gain_db = 15.0;
        assert!(GainController2::validate(&config));
    }

    #[test]
    fn check_headroom_db() {
        let mut config = Agc2Config::default();
        config.adaptive_digital.headroom_db = -1.0;
        assert!(!GainController2::validate(&config));
        config.adaptive_digital.headroom_db = 0.0;
        assert!(GainController2::validate(&config));
        config.adaptive_digital.headroom_db = 5.0;
        assert!(GainController2::validate(&config));
    }

    #[test]
    fn check_max_gain_db() {
        let mut config = Agc2Config::default();
        config.adaptive_digital.max_gain_db = -1.0;
        assert!(!GainController2::validate(&config));
        config.adaptive_digital.max_gain_db = 0.0;
        assert!(!GainController2::validate(&config));
        config.adaptive_digital.max_gain_db = 5.0;
        assert!(GainController2::validate(&config));
    }

    #[test]
    fn check_initial_gain_db() {
        let mut config = Agc2Config::default();
        config.adaptive_digital.initial_gain_db = -1.0;
        assert!(!GainController2::validate(&config));
        config.adaptive_digital.initial_gain_db = 0.0;
        assert!(GainController2::validate(&config));
        config.adaptive_digital.initial_gain_db = 5.0;
        assert!(GainController2::validate(&config));
    }

    #[test]
    fn check_adaptive_digital_max_gain_change_speed_config() {
        let mut config = Agc2Config::default();
        config.adaptive_digital.max_gain_change_db_per_second = -1.0;
        assert!(!GainController2::validate(&config));
        config.adaptive_digital.max_gain_change_db_per_second = 0.0;
        assert!(!GainController2::validate(&config));
        config.adaptive_digital.max_gain_change_db_per_second = 5.0;
        assert!(GainController2::validate(&config));
    }

    #[test]
    fn check_adaptive_digital_max_output_noise_level_config() {
        let mut config = Agc2Config::default();
        config.adaptive_digital.max_output_noise_level_dbfs = 5.0;
        assert!(!GainController2::validate(&config));
        config.adaptive_digital.max_output_noise_level_dbfs = 0.0;
        assert!(GainController2::validate(&config));
        config.adaptive_digital.max_output_noise_level_dbfs = -5.0;
        assert!(GainController2::validate(&config));
    }

    // --- Input volume controller tests ---

    #[test]
    fn check_recommended_input_volume_when_input_volume_controller_not_enabled() {
        let high_input_level = 32767.0_f32;
        let low_input_level = 1000.0_f32;
        let initial_input_volume = 100;
        let num_channels = 2;
        let num_frames = 5;
        let sample_rate_hz = 16000;

        let config = Agc2Config {
            input_volume_controller: Agc2InputVolumeControllerConfig { enabled: false },
            ..Default::default()
        };

        let mut gain_controller = GainController2::new(
            &config,
            &InputVolumeControllerConfig::default(),
            sample_rate_hz,
            num_channels,
            true,
        );

        assert!(gain_controller.recommended_input_volume().is_none());

        run_agc2_with_constant_input(
            &mut gain_controller,
            low_input_level,
            num_frames,
            sample_rate_hz,
            num_channels,
            initial_input_volume,
        );
        assert!(gain_controller.recommended_input_volume().is_none());

        run_agc2_with_constant_input(
            &mut gain_controller,
            high_input_level,
            num_frames,
            sample_rate_hz,
            num_channels,
            initial_input_volume,
        );
        assert!(gain_controller.recommended_input_volume().is_none());
    }

    #[test]
    fn check_recommended_input_volume_when_not_enabled_and_specific_config() {
        let high_input_level = 32767.0_f32;
        let low_input_level = 1000.0_f32;
        let initial_input_volume = 100;
        let num_channels = 2;
        let num_frames = 5;
        let sample_rate_hz = 16000;

        let config = Agc2Config {
            input_volume_controller: Agc2InputVolumeControllerConfig { enabled: false },
            ..Default::default()
        };

        let mut gain_controller = GainController2::new(
            &config,
            &TEST_INPUT_VOLUME_CONTROLLER_CONFIG,
            sample_rate_hz,
            num_channels,
            true,
        );

        assert!(gain_controller.recommended_input_volume().is_none());

        run_agc2_with_constant_input(
            &mut gain_controller,
            low_input_level,
            num_frames,
            sample_rate_hz,
            num_channels,
            initial_input_volume,
        );
        assert!(gain_controller.recommended_input_volume().is_none());

        run_agc2_with_constant_input(
            &mut gain_controller,
            high_input_level,
            num_frames,
            sample_rate_hz,
            num_channels,
            initial_input_volume,
        );
        assert!(gain_controller.recommended_input_volume().is_none());
    }

    #[test]
    fn check_recommended_input_volume_when_input_volume_controller_enabled() {
        let high_input_level = 32767.0_f32;
        let low_input_level = 1000.0_f32;
        let initial_input_volume = 100;
        let num_channels = 2;
        let num_frames = 5;
        let sample_rate_hz = 16000;

        let config = Agc2Config {
            input_volume_controller: Agc2InputVolumeControllerConfig { enabled: true },
            adaptive_digital: Agc2AdaptiveDigitalConfig {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut gain_controller = GainController2::new(
            &config,
            &InputVolumeControllerConfig::default(),
            sample_rate_hz,
            num_channels,
            true,
        );

        assert!(gain_controller.recommended_input_volume().is_none());

        run_agc2_with_constant_input(
            &mut gain_controller,
            low_input_level,
            num_frames,
            sample_rate_hz,
            num_channels,
            initial_input_volume,
        );
        assert!(gain_controller.recommended_input_volume().is_some());

        run_agc2_with_constant_input(
            &mut gain_controller,
            high_input_level,
            num_frames,
            sample_rate_hz,
            num_channels,
            initial_input_volume,
        );
        assert!(gain_controller.recommended_input_volume().is_some());
    }

    #[test]
    fn check_recommended_input_volume_when_enabled_and_specific_config() {
        let high_input_level = 32767.0_f32;
        let low_input_level = 1000.0_f32;
        let initial_input_volume = 100;
        let num_channels = 2;
        let num_frames = 5;
        let sample_rate_hz = 16000;

        let config = Agc2Config {
            input_volume_controller: Agc2InputVolumeControllerConfig { enabled: true },
            adaptive_digital: Agc2AdaptiveDigitalConfig {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut gain_controller = GainController2::new(
            &config,
            &TEST_INPUT_VOLUME_CONTROLLER_CONFIG,
            sample_rate_hz,
            num_channels,
            true,
        );

        assert!(gain_controller.recommended_input_volume().is_none());

        run_agc2_with_constant_input(
            &mut gain_controller,
            low_input_level,
            num_frames,
            sample_rate_hz,
            num_channels,
            initial_input_volume,
        );
        assert!(gain_controller.recommended_input_volume().is_some());

        run_agc2_with_constant_input(
            &mut gain_controller,
            high_input_level,
            num_frames,
            sample_rate_hz,
            num_channels,
            initial_input_volume,
        );
        assert!(gain_controller.recommended_input_volume().is_some());
    }

    // --- Construction test ---

    #[test]
    fn apply_default_config() {
        let config = Agc2Config::default();
        let gain_controller = GainController2::new(
            &config,
            &InputVolumeControllerConfig::default(),
            16000,
            2,
            true,
        );
        // Just check it constructs without panicking.
        let _ = gain_controller;
    }

    // --- Fixed digital tests ---

    #[test]
    fn gain_should_change_on_set_gain() {
        let input_level = 1000.0_f32;
        let num_frames = 5;
        let sample_rate_hz = 8000;
        let gain_0_db = 0.0_f32;
        let gain_20_db = 20.0_f32;

        let mut agc2_fixed = create_agc2_fixed_digital_mode(gain_0_db, sample_rate_hz);

        // Signal level is unchanged with 0 dB gain.
        let out = run_agc2_with_constant_input(
            &mut agc2_fixed,
            input_level,
            num_frames,
            sample_rate_hz,
            1,
            0,
        );
        assert!(
            (out - input_level).abs() < 0.01,
            "expected ~{input_level}, got {out}"
        );

        // +20 dB should increase signal by a factor of 10.
        agc2_fixed.set_fixed_gain_db(gain_20_db);
        let out = run_agc2_with_constant_input(
            &mut agc2_fixed,
            input_level,
            num_frames,
            sample_rate_hz,
            1,
            0,
        );
        assert!(
            (out - input_level * 10.0).abs() < 0.01,
            "expected ~{}, got {out}",
            input_level * 10.0
        );
    }

    #[test]
    fn change_fixed_gain_should_be_fast_and_time_invariant() {
        let num_frames = 5;
        let input_level = 1000.0_f32;
        let sample_rate_hz = 8000;
        let gain_db_low = 0.0_f32;
        let gain_db_high = 25.0_f32;

        let mut agc2_fixed = create_agc2_fixed_digital_mode(gain_db_low, sample_rate_hz);

        // Start with a lower gain.
        let output_level_pre = run_agc2_with_constant_input(
            &mut agc2_fixed,
            input_level,
            num_frames,
            sample_rate_hz,
            1,
            0,
        );

        // Increase gain.
        agc2_fixed.set_fixed_gain_db(gain_db_high);
        let _ = run_agc2_with_constant_input(
            &mut agc2_fixed,
            input_level,
            num_frames,
            sample_rate_hz,
            1,
            0,
        );

        // Back to the lower gain.
        agc2_fixed.set_fixed_gain_db(gain_db_low);
        let output_level_post = run_agc2_with_constant_input(
            &mut agc2_fixed,
            input_level,
            num_frames,
            sample_rate_hz,
            1,
            0,
        );

        assert_eq!(output_level_pre, output_level_post);
    }

    #[test]
    fn check_saturation_behavior_with_limiter() {
        // LinSpace equivalent: generate evenly-spaced values.
        fn lin_space(l: f64, r: f64, num_points: usize) -> Vec<f64> {
            if num_points == 1 {
                return vec![l];
            }
            let step = (r - l) / (num_points - 1) as f64;
            (0..num_points).map(|i| l + step * i as f64).collect()
        }

        let test_cases: Vec<(f64, f64, usize, bool)> = vec![
            // When gain < kLimiterMaxInputLevelDbFs, no saturation.
            (0.1, LIMITER_MAX_INPUT_LEVEL_DB_FS - 0.01, 8000, false),
            (0.1, LIMITER_MAX_INPUT_LEVEL_DB_FS - 0.01, 48000, false),
            // When gain > kLimiterMaxInputLevelDbFs, saturation.
            (LIMITER_MAX_INPUT_LEVEL_DB_FS + 0.01, 10.0, 8000, true),
            (LIMITER_MAX_INPUT_LEVEL_DB_FS + 0.01, 10.0, 48000, true),
        ];

        for (gain_db_min, gain_db_max, sample_rate_hz, saturation_expected) in test_cases {
            for gain_db in lin_space(gain_db_min, gain_db_max, 10) {
                let mut agc2_fixed = create_agc2_fixed_digital_mode(gain_db as f32, sample_rate_hz);
                let processed_sample =
                    run_agc2_with_constant_input(&mut agc2_fixed, 32767.0, 5, sample_rate_hz, 1, 0);
                if saturation_expected {
                    assert!(
                        (processed_sample - 32767.0).abs() < 0.01,
                        "gain_db={gain_db}, rate={sample_rate_hz}: expected saturation at 32767, got {processed_sample}"
                    );
                } else {
                    assert!(
                        processed_sample < 32767.0,
                        "gain_db={gain_db}, rate={sample_rate_hz}: expected no saturation, got {processed_sample}"
                    );
                }
            }
        }
    }
}
