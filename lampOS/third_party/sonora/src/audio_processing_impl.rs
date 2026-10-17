//! Core audio processing implementation.
//!
//! Ported from `AudioProcessingImpl` in
//! `modules/audio_processing/audio_processing_impl.h/cc`.
//!
//! This module coordinates all submodules (echo cancellation, noise
//! suppression, gain control, etc.) and manages their initialization,
//! configuration, and audio processing lifecycle.

use crate::audio_buffer::AudioBuffer;
use crate::audio_converter::AudioConverter;
use crate::capture_levels_adjuster::CaptureLevelsAdjuster;
use crate::config::{Config, DownmixMethod, NoiseSuppressionLevel, RuntimeSetting};
use crate::echo_canceller3::EchoCanceller3;
use crate::gain_controller2::{
    Agc2AdaptiveDigitalConfig, Agc2Config, Agc2InputVolumeControllerConfig, FixedDigitalConfig,
    GainController2,
};
use crate::high_pass_filter::HighPassFilter;
use crate::input_volume_controller::InputVolumeControllerConfig;
use crate::residual_echo_detector::ResidualEchoDetector;
use crate::rms_level::RmsLevel;
use crate::stats::AudioProcessingStats;
use crate::stream_config::StreamConfig;
use crate::submodule_states::SubmoduleStates;
use crate::swap_queue::SwapQueue;
use sonora_aec3::config::EchoCanceller3Config;
use sonora_ns::config::{NsConfig, SuppressionLevel};
use sonora_ns::noise_suppressor::NoiseSuppressor;
use std::collections::VecDeque;

/// Band split rate for sub-band processing (16 kHz).
const BAND_SPLIT_RATE: usize = 16000;

/// Maximum number of frames to buffer in render queues.
const MAX_NUM_FRAMES_TO_BUFFER: usize = 100;

/// Describes the four audio streams handled by the processing pipeline.
#[derive(Debug, Clone)]
pub(crate) struct ProcessingConfig {
    pub input_stream: StreamConfig,
    pub output_stream: StreamConfig,
    pub reverse_input_stream: StreamConfig,
    pub reverse_output_stream: StreamConfig,
}

impl Default for ProcessingConfig {
    fn default() -> Self {
        let default_stream = StreamConfig::new(16000, 1);
        Self {
            input_stream: default_stream,
            output_stream: default_stream,
            reverse_input_stream: default_stream,
            reverse_output_stream: default_stream,
        }
    }
}

/// Container for all optional submodule instances.
#[derive(Debug)]
struct Submodules {
    high_pass_filter: Option<HighPassFilter>,
    echo_controller: Option<EchoCanceller3>,
    noise_suppressors: Vec<NoiseSuppressor>,
    gain_controller2: Option<GainController2>,
    capture_levels_adjuster: Option<CaptureLevelsAdjuster>,
    echo_detector: Option<ResidualEchoDetector>,
}

#[derive(Debug)]
struct CaptureState {
    capture_audio: Option<AudioBuffer>,
    capture_fullband_audio: Option<AudioBuffer>,
    /// AEC3's linear output for the last capture frame, when exported
    /// (lampOS patch).
    linear_aec_output: Option<AudioBuffer>,
    capture_output_used: bool,
    capture_output_used_last_frame: bool,
    echo_path_gain_change: bool,
    prev_pre_adjustment_gain: f32,
    playout_volume: i32,
    prev_playout_volume: i32,
    stats: AudioProcessingStats,
    applied_input_volume: Option<i32>,
    applied_input_volume_changed: bool,
    recommended_input_volume: Option<i32>,
    was_stream_delay_set: bool,
    stream_delay_ms: i32,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            capture_audio: None,
            capture_fullband_audio: None,
            linear_aec_output: None,
            capture_output_used: true,
            capture_output_used_last_frame: true,
            echo_path_gain_change: false,
            prev_pre_adjustment_gain: -1.0,
            playout_volume: -1,
            prev_playout_volume: -1,
            stats: AudioProcessingStats::default(),
            applied_input_volume: None,
            applied_input_volume_changed: false,
            recommended_input_volume: None,
            was_stream_delay_set: false,
            stream_delay_ms: 0,
        }
    }
}

/// State for capture-side values not guarded by the capture lock.
#[derive(Debug)]
struct CaptureNonlocked {
    capture_processing_format: StreamConfig,
}

impl Default for CaptureNonlocked {
    fn default() -> Self {
        Self {
            capture_processing_format: StreamConfig::new(16000, 1),
        }
    }
}

#[derive(Debug, Default)]
struct RenderState {
    render_audio: Option<AudioBuffer>,
    render_converter: Option<AudioConverter>,
}

#[derive(Debug)]
struct FormatState {
    api_format: ProcessingConfig,
    render_processing_format: StreamConfig,
}

impl Default for FormatState {
    fn default() -> Self {
        Self {
            api_format: ProcessingConfig::default(),
            render_processing_format: StreamConfig::new(16000, 1),
        }
    }
}

/// The core audio processing engine.
///
/// Coordinates echo cancellation (AEC3), noise suppression (NS),
/// automatic gain control (AGC2), high-pass filtering, and level
/// adjustment across the capture and render audio paths.
#[derive(Debug)]
pub(crate) struct AudioProcessingImpl {
    config: Config,
    submodule_states: SubmoduleStates,
    submodules: Submodules,
    formats: FormatState,
    capture: CaptureState,
    capture_nonlocked: CaptureNonlocked,
    render: RenderState,
    capture_runtime_settings: VecDeque<RuntimeSetting>,
    render_runtime_settings: VecDeque<RuntimeSetting>,
    // Render queue for the residual echo detector.
    red_render_queue: Option<SwapQueue<Vec<f32>>>,
    red_render_queue_buffer: Vec<f32>,
    red_capture_queue_buffer: Vec<f32>,
    // RMS level metering.
    capture_input_rms: RmsLevel,
    capture_output_rms: RmsLevel,
    capture_rms_interval_counter: usize,
}

impl AudioProcessingImpl {
    /// Creates a new audio processing instance with the given configuration.
    pub(crate) fn with_config(config: Config) -> Self {
        let mut apm = Self {
            config,
            submodule_states: SubmoduleStates::new(),
            submodules: Submodules {
                high_pass_filter: None,
                echo_controller: None,
                noise_suppressors: Vec::new(),
                gain_controller2: None,
                capture_levels_adjuster: None,
                echo_detector: None,
            },
            formats: FormatState::default(),
            capture: CaptureState::default(),
            capture_nonlocked: CaptureNonlocked::default(),
            render: RenderState::default(),
            capture_runtime_settings: VecDeque::new(),
            render_runtime_settings: VecDeque::new(),
            red_render_queue: None,
            red_render_queue_buffer: Vec::new(),
            red_capture_queue_buffer: Vec::new(),
            capture_input_rms: RmsLevel::new(),
            capture_output_rms: RmsLevel::new(),
            capture_rms_interval_counter: 0,
        };
        apm.initialize();
        apm
    }

    /// Installs the residual echo detector (opt-in).
    ///
    /// Must be called before `initialize()`.
    pub(crate) fn set_echo_detector(&mut self) {
        self.submodules.echo_detector = Some(ResidualEchoDetector::new());
    }

    /// Initializes or reinitializes the processing pipeline with
    /// the current format configuration.
    pub(crate) fn initialize(&mut self) {
        self.initialize_locked();
    }

    /// Initializes the pipeline with new stream configurations.
    pub(crate) fn initialize_with_config(&mut self, processing_config: ProcessingConfig) {
        self.initialize_locked_with_config(processing_config);
    }

    /// Applies a new configuration, selectively reinitializing submodules.
    pub(crate) fn apply_config(&mut self, config: Config) {
        // lampOS patch: exporting the linear output is set up with AEC3.
        let export = |c: &Config| {
            c.echo_canceller
                .as_ref()
                .map(|ec| ec.export_linear_aec_output)
        };
        let aec_config_changed = self.config.echo_canceller.is_some()
            != config.echo_canceller.is_some()
            || export(&self.config) != export(&config);

        let agc2_config_changed = self.config.gain_controller2 != config.gain_controller2;

        let ns_config_changed = {
            let old_enabled = self.config.noise_suppression.is_some();
            let new_enabled = config.noise_suppression.is_some();
            let old_level = self.config.noise_suppression.as_ref().map(|ns| ns.level);
            let new_level = config.noise_suppression.as_ref().map(|ns| ns.level);
            old_enabled != new_enabled || old_level != new_level
        };

        let pre_amplifier_config_changed = {
            let old = &self.config.pre_amplifier;
            let new = &config.pre_amplifier;
            match (old, new) {
                (None, None) => false,
                (Some(_), None) | (None, Some(_)) => true,
                (Some(a), Some(b)) => a.fixed_gain_factor != b.fixed_gain_factor,
            }
        };

        let gain_adjustment_config_changed =
            self.config.capture_level_adjustment != config.capture_level_adjustment;

        let pipeline_config_changed = self.config.pipeline.multi_channel_render
            != config.pipeline.multi_channel_render
            || self.config.pipeline.multi_channel_capture != config.pipeline.multi_channel_capture
            || self.config.pipeline.maximum_internal_processing_rate
                != config.pipeline.maximum_internal_processing_rate
            || self.config.pipeline.capture_downmix_method
                != config.pipeline.capture_downmix_method;

        self.config = config;

        if aec_config_changed {
            self.initialize_echo_controller();
        }

        if ns_config_changed {
            self.initialize_noise_suppressor();
        }

        self.initialize_high_pass_filter(false);

        if agc2_config_changed {
            if !GainController2::validate(&self.agc2_config_from_api()) {
                tracing::error!("Invalid GainController2 config; using default");
                self.config.gain_controller2 = None;
            }
            self.initialize_gain_controller2();
        }

        if pre_amplifier_config_changed || gain_adjustment_config_changed {
            self.initialize_capture_levels_adjuster();
        }

        let reinitialization_needed =
            self.update_active_submodule_states() || pipeline_config_changed;
        if reinitialization_needed {
            let api_format = self.formats.api_format.clone();
            self.initialize_locked_with_config(api_format);
        }
    }

    /// Enqueues a runtime setting for the capture path.
    pub(crate) fn set_runtime_setting(&mut self, setting: RuntimeSetting) {
        self.capture_runtime_settings.push_back(setting);
    }

    /// Returns current processing statistics.
    pub(crate) fn get_statistics(&self) -> &AudioProcessingStats {
        &self.capture.stats
    }

    /// AEC3's linear output for the last capture frame, in FloatS16 scale,
    /// when it is exported (lampOS patch).
    pub(crate) fn linear_aec_output(&self) -> Option<&AudioBuffer> {
        self.capture.linear_aec_output.as_ref()
    }

    /// Sets the applied input volume (e.g. from the OS mixer).
    pub(crate) fn set_applied_input_volume(&mut self, volume: i32) {
        self.capture.applied_input_volume_changed = self
            .capture
            .applied_input_volume
            .is_some_and(|prev| prev != volume);
        self.capture.applied_input_volume = Some(volume);
    }

    /// Returns the recommended input volume from AGC2.
    pub(crate) fn recommended_input_volume(&self) -> Option<i32> {
        self.capture.recommended_input_volume
    }

    /// Returns the applied input volume (as set by the caller).
    pub(crate) fn applied_input_volume(&self) -> Option<i32> {
        self.capture.applied_input_volume
    }

    /// Sets the stream delay in ms (forwarded from the public API).
    pub(crate) fn set_stream_delay_ms(&mut self, delay_ms: i32) {
        self.capture.was_stream_delay_set = true;
        self.capture.stream_delay_ms = delay_ms;
    }

    /// Processes a capture audio frame.
    ///
    /// `src` and `dest` are deinterleaved float channel slices, one per channel.
    pub(crate) fn process_stream(
        &mut self,
        src: &[&[f32]],
        input_config: &StreamConfig,
        output_config: &StreamConfig,
        dest: &mut [&mut [f32]],
    ) {
        self.maybe_initialize_capture(input_config, output_config);

        let capture_audio = self.capture.capture_audio.as_mut().unwrap();
        capture_audio.copy_from_float(src, input_config);
        if let Some(fullband) = &mut self.capture.capture_fullband_audio {
            fullband.copy_from_float(src, input_config);
        }

        self.process_capture_stream_locked();

        if let Some(ref mut fullband) = self.capture.capture_fullband_audio {
            fullband.copy_to_float(output_config, dest);
        } else {
            self.capture
                .capture_audio
                .as_mut()
                .unwrap()
                .copy_to_float(output_config, dest);
        }
    }

    /// Processes a reverse (render / far-end) audio frame.
    ///
    /// `src` and `dest` are deinterleaved float channel slices, one per channel.
    pub(crate) fn process_reverse_stream(
        &mut self,
        src: &[&[f32]],
        input_config: &StreamConfig,
        output_config: &StreamConfig,
        dest: &mut [&mut [f32]],
    ) {
        self.maybe_initialize_render(input_config, output_config);
        self.analyze_reverse_stream_locked(src, input_config);

        // Only read from the AudioBuffer if render processing actually modified
        // the data (multi-band or full-band processing). The echo controller
        // reads from the AudioBuffer for analysis but doesn't modify the render
        // output — so we must NOT use render_multi_band_sub_modules_active()
        // here (which includes echo_controller_enabled).
        if self.submodule_states.render_output_modified() {
            let render_audio = self.render.render_audio.as_mut().unwrap();
            render_audio.copy_to_float(&self.formats.api_format.reverse_output_stream, dest);
        } else if self.formats.api_format.reverse_input_stream
            != self.formats.api_format.reverse_output_stream
        {
            if let Some(converter) = &mut self.render.render_converter {
                converter.convert(src, dest);
            }
        } else {
            // Copy input to output directly when no processing is needed.
            for (out_ch, in_ch) in dest.iter_mut().zip(src.iter()) {
                let len = out_ch.len().min(in_ch.len());
                out_ch[..len].copy_from_slice(&in_ch[..len]);
            }
        }
    }

    /// Processes a capture audio frame (int16, interleaved).
    pub(crate) fn process_stream_i16(
        &mut self,
        src: &[i16],
        input_config: &StreamConfig,
        output_config: &StreamConfig,
        dest: &mut [i16],
    ) {
        self.maybe_initialize_capture(input_config, output_config);

        let capture_audio = self.capture.capture_audio.as_mut().unwrap();
        capture_audio.copy_from_interleaved_i16(src, input_config);
        if let Some(fullband) = &mut self.capture.capture_fullband_audio {
            fullband.copy_from_interleaved_i16(src, input_config);
        }

        self.process_capture_stream_locked();

        // Only copy through AudioBuffer when processing actually happened.
        // When all components are disabled, the int16 data is left as-is
        // (matching C++ behavior where src == dest is common).
        if self
            .submodule_states
            .capture_multi_band_processing_present()
            || self.submodule_states.capture_full_band_processing_active()
        {
            if let Some(ref mut fullband) = self.capture.capture_fullband_audio {
                fullband.copy_to_interleaved_i16(output_config, dest);
            } else {
                self.capture
                    .capture_audio
                    .as_mut()
                    .unwrap()
                    .copy_to_interleaved_i16(output_config, dest);
            }
        } else {
            // No processing — copy input to output directly.
            let len = dest.len().min(src.len());
            dest[..len].copy_from_slice(&src[..len]);
        }
    }

    /// Processes a reverse (render / far-end) audio frame (int16, interleaved).
    pub(crate) fn process_reverse_stream_i16(
        &mut self,
        src: &[i16],
        input_config: &StreamConfig,
        output_config: &StreamConfig,
        dest: &mut [i16],
    ) {
        self.maybe_initialize_render(input_config, output_config);

        let render_audio = self.render.render_audio.as_mut().unwrap();
        render_audio.copy_from_interleaved_i16(src, input_config);
        self.process_render_stream_locked();

        if self.submodule_states.render_output_modified() {
            let render_audio = self.render.render_audio.as_mut().unwrap();
            render_audio
                .copy_to_interleaved_i16(&self.formats.api_format.reverse_output_stream, dest);
        } else {
            // Copy src to dest directly (no processing needed).
            let len = dest.len().min(src.len());
            dest[..len].copy_from_slice(&src[..len]);
        }
    }

    /// Returns a reference to the current configuration.
    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    /// The main capture processing pipeline.
    fn process_capture_stream_locked(&mut self) {
        self.empty_queued_render_audio();
        self.handle_capture_runtime_settings();

        let capture_buffer = self.capture.capture_audio.as_mut().unwrap();

        // Phase 1: Full-band high-pass filter (before splitting).
        if self
            .config
            .high_pass_filter
            .as_ref()
            .is_some_and(|hpf| hpf.apply_in_full_band)
            && let Some(hpf) = &mut self.submodules.high_pass_filter
        {
            hpf.process(capture_buffer, false);
        }

        // Phase 1b: Pre-level adjustment.
        if let Some(adjuster) = &mut self.submodules.capture_levels_adjuster {
            let emulation_enabled = self
                .config
                .capture_level_adjustment
                .as_ref()
                .is_some_and(|cla| cla.analog_mic_gain_emulation.is_some());
            if emulation_enabled {
                // Feed the emulated analog gain level as the applied input
                // volume.
                let level = adjuster.get_analog_mic_gain_level();
                self.capture.applied_input_volume = Some(level);
            }
            adjuster.apply_pre_level_adjustment(capture_buffer);
        }

        // Input RMS metering (after pre-level adjustment).
        self.capture_input_rms
            .analyze_float(capture_buffer.channel(0));
        self.capture_rms_interval_counter += 1;
        let log_rms = self.capture_rms_interval_counter >= 1000;
        if log_rms {
            self.capture_rms_interval_counter = 0;
            // Reset counters; in C++ these feed Chrome histograms.
            self.capture_input_rms.reset();
        }

        // Phase 2: Echo path gain change detection.
        if self.submodules.echo_controller.is_some() {
            self.capture.echo_path_gain_change = self.capture.applied_input_volume_changed;

            if let Some(adjuster) = &self.submodules.capture_levels_adjuster {
                let pre_adjustment_gain = adjuster.get_pre_adjustment_gain();
                self.capture.echo_path_gain_change = self.capture.echo_path_gain_change
                    || (self.capture.prev_pre_adjustment_gain != pre_adjustment_gain
                        && self.capture.prev_pre_adjustment_gain >= 0.0);
                self.capture.prev_pre_adjustment_gain = pre_adjustment_gain;
            }

            self.capture.echo_path_gain_change = self.capture.echo_path_gain_change
                || (self.capture.prev_playout_volume != self.capture.playout_volume
                    && self.capture.prev_playout_volume >= 0);
            self.capture.prev_playout_volume = self.capture.playout_volume;
        }

        // Phase 2b: Echo controller capture analysis.
        if let Some(ec) = &mut self.submodules.echo_controller {
            ec.analyze_capture(capture_buffer);
        }

        // Phase 2c: AGC2 input volume analysis.
        if let Some(gc2) = &mut self.submodules.gain_controller2
            && self
                .config
                .gain_controller2
                .as_ref()
                .is_some_and(|gc| gc.input_volume_controller)
            && let Some(vol) = self.capture.applied_input_volume
        {
            gc2.analyze(vol, capture_buffer);
        }

        // Phase 3: Frequency band splitting.
        let capture_rate = self
            .capture_nonlocked
            .capture_processing_format
            .sample_rate_hz();
        if self
            .submodule_states
            .capture_multi_band_sub_modules_active()
            && sample_rate_supports_multi_band(capture_rate)
        {
            let capture_buffer = self.capture.capture_audio.as_mut().unwrap();
            capture_buffer.split_into_frequency_bands();
        }

        // Phase 4: Down-mix to mono for echo controller.
        let multi_channel_capture = self.config.pipeline.multi_channel_capture;
        if self.submodules.echo_controller.is_some() && !multi_channel_capture {
            let capture_buffer = self.capture.capture_audio.as_mut().unwrap();
            capture_buffer.set_num_channels(1);
        }

        // Phase 5: Split-band high-pass filter.
        if !self
            .config
            .high_pass_filter
            .as_ref()
            .is_some_and(|hpf| hpf.apply_in_full_band)
            && let Some(hpf) = &mut self.submodules.high_pass_filter
        {
            let capture_buffer = self.capture.capture_audio.as_mut().unwrap();
            hpf.process(capture_buffer, true);
        }

        // Phase 6: Noise suppression analysis (before echo cancellation).
        if !self
            .config
            .noise_suppression
            .as_ref()
            .is_some_and(|ns| ns.analyze_linear_aec_output_when_available)
        {
            for (ch, ns) in self.submodules.noise_suppressors.iter_mut().enumerate() {
                let capture_buffer = self.capture.capture_audio.as_ref().unwrap();
                let band0 = capture_buffer.split_band(ch, 0);

                if let Ok(frame) = <&[f32; 160]>::try_from(band0) {
                    ns.analyze(frame);
                }
            }
        }

        // Phase 7: Echo control processing.
        if let Some(ec) = &mut self.submodules.echo_controller {
            if self.capture.was_stream_delay_set {
                ec.set_audio_buffer_delay(self.capture.stream_delay_ms);
            }
            let echo_path_gain_change = self.capture.echo_path_gain_change;
            let capture_buffer = self.capture.capture_audio.as_mut().unwrap();
            let linear_output = self.capture.linear_aec_output.as_mut();
            ec.process_capture(capture_buffer, linear_output, echo_path_gain_change);
        }

        // Phase 7b: NS analysis on linear AEC output (if configured).
        if self
            .config
            .noise_suppression
            .as_ref()
            .is_some_and(|ns| ns.analyze_linear_aec_output_when_available)
        {
            for (ch, ns) in self.submodules.noise_suppressors.iter_mut().enumerate() {
                let capture_buffer = self.capture.capture_audio.as_ref().unwrap();
                let band0 = capture_buffer.split_band(ch, 0);
                if let Ok(frame) = <&[f32; 160]>::try_from(band0) {
                    ns.analyze(frame);
                }
            }
        }

        // Phase 7c: Noise suppression processing.
        // Process band 0 for each channel (updates Wiener filter + computes upper band gain).
        for (ch, ns) in self.submodules.noise_suppressors.iter_mut().enumerate() {
            let capture_buffer = self.capture.capture_audio.as_mut().unwrap();
            let band0 = capture_buffer.split_band_mut(ch, 0);
            if let Ok(frame) = <&mut [f32; 160]>::try_from(band0) {
                ns.process(frame);
            }
        }

        // Process upper bands if multi-band (delay compensation + gain + clamp).
        if !self.submodules.noise_suppressors.is_empty() {
            let num_bands = self.capture.capture_audio.as_ref().unwrap().num_bands();
            if num_bands > 1 {
                // Aggregate upper band gain across channels (take minimum).
                let upper_band_gain = self
                    .submodules
                    .noise_suppressors
                    .iter()
                    .map(|ns| ns.upper_band_gain())
                    .fold(f32::MAX, f32::min);

                for (ch, ns) in self.submodules.noise_suppressors.iter_mut().enumerate() {
                    // Delay and apply gain to each upper band.
                    for b in 1..num_bands {
                        let capture_buffer = self.capture.capture_audio.as_mut().unwrap();
                        let band_data = capture_buffer.split_band_mut(ch, b);
                        if let Ok(frame) = <&mut [f32; 160]>::try_from(band_data) {
                            ns.process_upper_band(frame, b - 1, upper_band_gain);
                        }
                    }

                    // Clamp all bands (band 0 already clamped by process(),
                    // but C++ clamps all bands after upper band processing).
                    for b in 0..num_bands {
                        let capture_buffer = self.capture.capture_audio.as_mut().unwrap();
                        let band_data = capture_buffer.split_band_mut(ch, b);
                        if let Ok(frame) = <&mut [f32; 160]>::try_from(band_data) {
                            NoiseSuppressor::clamp_frame(frame);
                        }
                    }
                }
            }
        }

        // Phase 8: Frequency band merging.
        if self
            .submodule_states
            .capture_multi_band_processing_present()
            && sample_rate_supports_multi_band(capture_rate)
        {
            let capture_buffer = self.capture.capture_audio.as_mut().unwrap();
            capture_buffer.merge_frequency_bands();
        }

        // Phase 9: Full-band post-processing.
        if self.capture.capture_output_used {
            // Copy to fullband buffer if needed.
            if let Some(fullband) = self.capture.capture_fullband_audio.as_mut() {
                let ec_active = self
                    .submodules
                    .echo_controller
                    .as_ref()
                    .is_some_and(|ec| ec.active_processing());
                if self
                    .submodule_states
                    .capture_multi_band_processing_active(ec_active)
                {
                    // Copy the multi-band processed audio to the fullband buffer.
                    // Temporarily take capture_audio to avoid double &mut borrow.
                    let mut capture = self.capture.capture_audio.take().unwrap();
                    capture.copy_to_buffer(fullband);
                    self.capture.capture_audio = Some(capture);
                }
            }

            // Residual echo detector analysis.
            let capture_buffer = self
                .capture
                .capture_fullband_audio
                .as_ref()
                .or(self.capture.capture_audio.as_ref())
                .unwrap();
            if let Some(red) = &mut self.submodules.echo_detector {
                let ch0 = capture_buffer.channel(0);
                red.analyze_capture_audio(ch0);
            }

            // GainController2 processing.
            let input_volume_changed = self.capture.applied_input_volume_changed;
            let capture_buffer = self
                .capture
                .capture_fullband_audio
                .as_mut()
                .or(self.capture.capture_audio.as_mut())
                .unwrap();
            if let Some(gc2) = &mut self.submodules.gain_controller2 {
                gc2.process(input_volume_changed, capture_buffer);
            }

            // Output RMS metering (after GC2).
            {
                let capture_buffer = self
                    .capture
                    .capture_fullband_audio
                    .as_ref()
                    .or(self.capture.capture_audio.as_ref())
                    .unwrap();
                self.capture_output_rms
                    .analyze_float(capture_buffer.channel(0));
            }
            if log_rms {
                self.capture_output_rms.reset();
            }

            // Echo detector stats.
            if let Some(red) = &self.submodules.echo_detector {
                let metrics = red.get_metrics();
                self.capture.stats.residual_echo_likelihood =
                    metrics.echo_likelihood.map(f64::from);
                self.capture.stats.residual_echo_likelihood_recent_max =
                    metrics.echo_likelihood_recent_max.map(f64::from);
            }
        }

        // Phase 10: Echo controller statistics.
        if let Some(ec) = &self.submodules.echo_controller {
            let metrics = ec.get_metrics();
            self.capture.stats.echo_return_loss = Some(metrics.echo_return_loss);
            self.capture.stats.echo_return_loss_enhancement =
                Some(metrics.echo_return_loss_enhancement);
            self.capture.stats.delay_ms = Some(metrics.delay_ms);
        }

        // Phase 11: Update recommended input volume.
        self.update_recommended_input_volume();

        // Phase 12: Post-level adjustment.
        let capture_buffer = self
            .capture
            .capture_fullband_audio
            .as_mut()
            .or(self.capture.capture_audio.as_mut())
            .unwrap();
        if let Some(adjuster) = &mut self.submodules.capture_levels_adjuster {
            adjuster.apply_post_level_adjustment(capture_buffer);

            let emulation_enabled = self
                .config
                .capture_level_adjustment
                .as_ref()
                .is_some_and(|cla| cla.analog_mic_gain_emulation.is_some());
            if emulation_enabled && let Some(vol) = self.capture.recommended_input_volume {
                adjuster.set_analog_mic_gain_level(vol);
            }
        }

        // Phase 13: Mute click avoidance.
        if !self.capture.capture_output_used_last_frame && self.capture.capture_output_used {
            let capture_buffer = self
                .capture
                .capture_fullband_audio
                .as_mut()
                .or(self.capture.capture_audio.as_mut())
                .unwrap();
            for ch in 0..capture_buffer.num_channels() {
                let channel = capture_buffer.channel_mut(ch);
                channel.fill(0.0);
            }
        }
        self.capture.capture_output_used_last_frame = self.capture.capture_output_used;
    }

    /// Analyzes a reverse (render) stream.
    fn analyze_reverse_stream_locked(&mut self, src: &[&[f32]], input_config: &StreamConfig) {
        let render_audio = self.render.render_audio.as_mut().unwrap();
        render_audio.copy_from_float(src, input_config);
        self.process_render_stream_locked();
    }

    /// The render processing pipeline.
    fn process_render_stream_locked(&mut self) {
        self.handle_render_runtime_settings();

        // Queue non-banded render audio for the echo detector (uses shared ref).
        {
            let render_buffer = self.render.render_audio.as_ref().unwrap();
            if self.submodules.echo_detector.is_some() {
                let ch0 = render_buffer.channel(0);
                self.red_render_queue_buffer.clear();
                self.red_render_queue_buffer.extend_from_slice(ch0);
            }
        }
        self.queue_render_buffer_into_red();

        // Frequency band splitting.
        let render_rate = self.formats.render_processing_format.sample_rate_hz();
        if self.submodule_states.render_multi_band_sub_modules_active()
            && sample_rate_supports_multi_band(render_rate)
        {
            let render_buffer = self.render.render_audio.as_mut().unwrap();
            render_buffer.split_into_frequency_bands();
        }

        // Echo controller render analysis.
        if let Some(ec) = &mut self.submodules.echo_controller {
            let render_buffer = self.render.render_audio.as_ref().unwrap();
            ec.analyze_render(render_buffer);
        }

        // Frequency band merging.
        if self.submodule_states.render_multi_band_sub_modules_active()
            && sample_rate_supports_multi_band(render_rate)
        {
            let render_buffer = self.render.render_audio.as_mut().unwrap();
            render_buffer.merge_frequency_bands();
        }
    }

    /// Inserts the pre-filled `red_render_queue_buffer` into the render queue.
    /// The buffer must already be filled with render channel 0 data.
    fn queue_render_buffer_into_red(&mut self) {
        if self.submodules.echo_detector.is_none() {
            return;
        }

        let mut needs_flush = false;
        if let Some(queue) = &mut self.red_render_queue
            && !queue.insert(&mut self.red_render_queue_buffer)
        {
            needs_flush = true;
        }

        if needs_flush {
            // Queue was full — flush then retry.
            self.empty_queued_render_audio_inner();
            if let Some(queue) = &mut self.red_render_queue {
                let _ = queue.insert(&mut self.red_render_queue_buffer);
            }
        }
    }

    fn empty_queued_render_audio(&mut self) {
        self.empty_queued_render_audio_inner();
    }

    fn empty_queued_render_audio_inner(&mut self) {
        if let (Some(queue), Some(red)) = (
            &mut self.red_render_queue,
            &mut self.submodules.echo_detector,
        ) {
            while queue.remove(&mut self.red_capture_queue_buffer) {
                red.analyze_render_audio(&self.red_capture_queue_buffer);
            }
        }
    }

    fn handle_capture_runtime_settings(&mut self) {
        while let Some(setting) = self.capture_runtime_settings.pop_front() {
            match setting {
                RuntimeSetting::CapturePreGain(value) => {
                    let has_pre_amp = self.config.pre_amplifier.is_some();
                    let has_cla = self.config.capture_level_adjustment.is_some();
                    if has_pre_amp || has_cla {
                        if let Some(ref mut pre_amp) = self.config.pre_amplifier {
                            pre_amp.fixed_gain_factor = value;
                        } else if let Some(ref mut cla) = self.config.capture_level_adjustment {
                            cla.pre_gain_factor = value;
                        }

                        let mut gain = 1.0_f32;
                        if let Some(ref pre_amp) = self.config.pre_amplifier {
                            gain *= pre_amp.fixed_gain_factor;
                        }
                        if let Some(ref cla) = self.config.capture_level_adjustment {
                            gain *= cla.pre_gain_factor;
                        }

                        if let Some(adjuster) = &mut self.submodules.capture_levels_adjuster {
                            adjuster.set_pre_gain(gain);
                        }
                    }
                }
                RuntimeSetting::CapturePostGain(value) => {
                    if let Some(ref mut cla) = self.config.capture_level_adjustment {
                        cla.post_gain_factor = value;
                        if let Some(adjuster) = &mut self.submodules.capture_levels_adjuster {
                            adjuster.set_post_gain(cla.post_gain_factor);
                        }
                    }
                }
                RuntimeSetting::CaptureFixedPostGain(value) => {
                    if let Some(gc2) = &mut self.submodules.gain_controller2 {
                        if let Some(ref mut gc2_config) = self.config.gain_controller2 {
                            gc2_config.fixed_digital.gain_db = value;
                        }
                        gc2.set_fixed_gain_db(value);
                    }
                }
                RuntimeSetting::PlayoutVolumeChange(value) => {
                    self.capture.playout_volume = value;
                }
                RuntimeSetting::CaptureOutputUsed(value) => {
                    self.capture.capture_output_used = value;
                    if let Some(ec) = &mut self.submodules.echo_controller {
                        ec.set_capture_output_usage(value);
                    }
                    if let Some(gc2) = &mut self.submodules.gain_controller2 {
                        gc2.set_capture_output_used(value);
                    }
                }
                RuntimeSetting::PlayoutAudioDeviceChange(_info) => {
                    // No render pre-processor in the Rust port.
                }
            }
        }
    }

    fn handle_render_runtime_settings(&mut self) {
        while let Some(setting) = self.render_runtime_settings.pop_front() {
            match setting {
                RuntimeSetting::PlayoutAudioDeviceChange(info) => {
                    tracing::trace!(
                        id = info.id,
                        max_volume = info.max_volume,
                        "playout device changed"
                    );
                }
                RuntimeSetting::PlayoutVolumeChange(_) => {
                    // No render pre-processor in the Rust port.
                }
                _ => {}
            }
        }
    }

    fn maybe_initialize_capture(
        &mut self,
        input_config: &StreamConfig,
        output_config: &StreamConfig,
    ) {
        let mut reinitialization_required = self.update_active_submodule_states();

        if self.formats.api_format.input_stream != *input_config {
            reinitialization_required = true;
        }
        if self.formats.api_format.output_stream != *output_config {
            reinitialization_required = true;
        }

        if reinitialization_required {
            let mut processing_config = self.formats.api_format.clone();
            processing_config.input_stream = *input_config;
            processing_config.output_stream = *output_config;
            self.initialize_locked_with_config(processing_config);
        }
    }

    fn maybe_initialize_render(
        &mut self,
        input_config: &StreamConfig,
        output_config: &StreamConfig,
    ) {
        let mut processing_config = self.formats.api_format.clone();
        processing_config.reverse_input_stream = *input_config;
        processing_config.reverse_output_stream = *output_config;

        if processing_config.input_stream == self.formats.api_format.input_stream
            && processing_config.output_stream == self.formats.api_format.output_stream
            && processing_config.reverse_input_stream
                == self.formats.api_format.reverse_input_stream
            && processing_config.reverse_output_stream
                == self.formats.api_format.reverse_output_stream
        {
            return;
        }

        self.initialize_locked_with_config(processing_config);
    }

    fn update_recommended_input_volume(&mut self) {
        if self.capture.applied_input_volume.is_none() {
            self.capture.recommended_input_volume = None;
            return;
        }

        if let Some(gc2) = &self.submodules.gain_controller2
            && self
                .config
                .gain_controller2
                .as_ref()
                .is_some_and(|gc| gc.input_volume_controller)
        {
            self.capture.recommended_input_volume = gc2.recommended_input_volume();
            return;
        }

        self.capture.recommended_input_volume = self.capture.applied_input_volume;
    }

    /// The capture processing sample rate.
    pub(crate) fn proc_sample_rate_hz(&self) -> usize {
        self.capture_nonlocked
            .capture_processing_format
            .sample_rate_hz() as usize
    }

    /// The fullband capture sample rate.
    fn proc_fullband_sample_rate_hz(&self) -> usize {
        match &self.capture.capture_fullband_audio {
            Some(buf) => buf.num_frames() * 100,
            None => self.proc_sample_rate_hz(),
        }
    }

    /// The number of reverse (render) channels.
    fn num_reverse_channels(&self) -> usize {
        self.formats.render_processing_format.num_channels() as usize
    }

    /// The number of processing channels (may be 1 if AEC3 forces mono).
    fn num_proc_channels(&self) -> usize {
        let multi_channel_capture = self.config.pipeline.multi_channel_capture;
        if self.submodule_states.echo_controller_enabled() && !multi_channel_capture {
            return 1;
        }
        self.num_output_channels()
    }

    /// The number of output channels.
    fn num_output_channels(&self) -> usize {
        self.formats.api_format.output_stream.num_channels() as usize
    }

    fn initialize_locked(&mut self) {
        self.update_active_submodule_states();

        let render_audiobuffer_sample_rate_hz =
            if self.formats.api_format.reverse_output_stream.num_frames() == 0 {
                self.formats.render_processing_format.sample_rate_hz() as usize
            } else {
                self.formats
                    .api_format
                    .reverse_output_stream
                    .sample_rate_hz() as usize
            };

        // Set up render audio buffer.
        if self.formats.api_format.reverse_input_stream.num_channels() > 0 {
            self.render.render_audio = Some(AudioBuffer::new(
                self.formats
                    .api_format
                    .reverse_input_stream
                    .sample_rate_hz() as usize,
                self.formats.api_format.reverse_input_stream.num_channels() as usize,
                self.formats.render_processing_format.sample_rate_hz() as usize,
                self.formats.render_processing_format.num_channels() as usize,
                render_audiobuffer_sample_rate_hz,
            ));
            if self.formats.api_format.reverse_input_stream
                != self.formats.api_format.reverse_output_stream
            {
                self.render.render_converter = Some(AudioConverter::new(
                    self.formats.api_format.reverse_input_stream.num_channels() as usize,
                    self.formats.api_format.reverse_input_stream.num_frames(),
                    self.formats.api_format.reverse_output_stream.num_channels() as usize,
                    self.formats.api_format.reverse_output_stream.num_frames(),
                ));
            } else {
                self.render.render_converter = None;
            }
        } else {
            self.render.render_audio = None;
            self.render.render_converter = None;
        }

        // Set up capture audio buffer.
        let capture_processing_rate = self
            .capture_nonlocked
            .capture_processing_format
            .sample_rate_hz() as usize;
        let input_rate = self.formats.api_format.input_stream.sample_rate_hz() as usize;
        let input_channels = self.formats.api_format.input_stream.num_channels() as usize;
        let output_rate = self.formats.api_format.output_stream.sample_rate_hz() as usize;
        let output_channels = self.formats.api_format.output_stream.num_channels() as usize;

        self.capture.capture_audio = Some(AudioBuffer::new(
            input_rate,
            input_channels,
            capture_processing_rate,
            output_channels,
            output_rate,
        ));
        set_downmix_method(
            self.capture.capture_audio.as_mut().unwrap(),
            self.config.pipeline.capture_downmix_method,
        );

        // Set up fullband capture audio buffer if needed.
        if capture_processing_rate < output_rate && output_rate == 48000 {
            self.capture.capture_fullband_audio = Some(AudioBuffer::new(
                input_rate,
                input_channels,
                output_rate,
                output_channels,
                output_rate,
            ));
            set_downmix_method(
                self.capture.capture_fullband_audio.as_mut().unwrap(),
                self.config.pipeline.capture_downmix_method,
            );
        } else {
            self.capture.capture_fullband_audio = None;
        }

        self.allocate_render_queue();

        // Initialize all submodules.
        self.initialize_high_pass_filter(true);
        self.initialize_residual_echo_detector();
        self.initialize_echo_controller();
        self.initialize_gain_controller2();
        self.initialize_noise_suppressor();
        self.initialize_capture_levels_adjuster();
    }

    fn initialize_locked_with_config(&mut self, config: ProcessingConfig) {
        self.update_active_submodule_states();

        self.formats.api_format = config;

        // Choose maximum processing rate.
        let max_splitting_rate = self
            .config
            .pipeline
            .maximum_internal_processing_rate
            .as_hz();

        // Determine capture processing rate.
        let min_capture_rate = self
            .formats
            .api_format
            .input_stream
            .sample_rate_hz()
            .min(self.formats.api_format.output_stream.sample_rate_hz());
        let band_splitting_required = self
            .submodule_states
            .capture_multi_band_sub_modules_active()
            || self.submodule_states.render_multi_band_sub_modules_active();
        let capture_processing_rate = suitable_process_rate(
            min_capture_rate,
            max_splitting_rate,
            band_splitting_required,
        );

        self.capture_nonlocked.capture_processing_format =
            StreamConfig::new(capture_processing_rate, 1);

        // Determine render processing rate.
        let render_processing_rate = if !self.submodule_states.echo_controller_enabled() {
            let min_render_rate = self
                .formats
                .api_format
                .reverse_input_stream
                .sample_rate_hz()
                .min(
                    self.formats
                        .api_format
                        .reverse_output_stream
                        .sample_rate_hz(),
                );
            suitable_process_rate(min_render_rate, max_splitting_rate, band_splitting_required)
        } else {
            capture_processing_rate
        };

        // Setup render processing format.
        if self.submodule_states.render_multi_band_sub_modules_active() {
            let multi_channel_render = self.config.pipeline.multi_channel_render;
            let render_processing_num_channels = if multi_channel_render {
                self.formats.api_format.reverse_input_stream.num_channels()
            } else {
                1
            };
            self.formats.render_processing_format =
                StreamConfig::new(render_processing_rate, render_processing_num_channels);
        } else {
            self.formats.render_processing_format = StreamConfig::new(
                self.formats
                    .api_format
                    .reverse_input_stream
                    .sample_rate_hz(),
                self.formats.api_format.reverse_input_stream.num_channels(),
            );
        }

        self.initialize_locked();
    }

    fn update_active_submodule_states(&mut self) -> bool {
        let need_echo_controller = self.config.echo_canceller.is_some();
        let gain_adjustment =
            self.config.pre_amplifier.is_some() || self.config.capture_level_adjustment.is_some();
        self.submodule_states.update(
            self.config.high_pass_filter.is_some(),
            !self.submodules.noise_suppressors.is_empty(),
            self.config.gain_controller2.is_some(),
            gain_adjustment,
            need_echo_controller,
        )
    }

    fn initialize_high_pass_filter(&mut self, forced_reset: bool) {
        let hpf_needed_by_aec = self
            .config
            .echo_canceller
            .as_ref()
            .is_some_and(|ec| ec.enforce_high_pass_filtering);
        if self.submodule_states.high_pass_filtering_required() || hpf_needed_by_aec {
            let use_full_band = self
                .config
                .high_pass_filter
                .as_ref()
                .is_some_and(|hpf| hpf.apply_in_full_band);
            let rate = if use_full_band {
                self.proc_fullband_sample_rate_hz() as i32
            } else {
                BAND_SPLIT_RATE as i32
            };
            let num_channels = if use_full_band {
                self.num_output_channels()
            } else {
                self.num_proc_channels()
            };

            let need_reset = match &self.submodules.high_pass_filter {
                Some(hpf) => {
                    rate != hpf.sample_rate_hz()
                        || forced_reset
                        || num_channels != hpf.num_channels()
                }
                None => true,
            };

            if need_reset {
                self.submodules.high_pass_filter = Some(HighPassFilter::new(rate, num_channels));
            }
        } else {
            self.submodules.high_pass_filter = None;
        }
    }

    fn initialize_echo_controller(&mut self) {
        self.submodules.echo_controller = None;
        self.capture.linear_aec_output = None;

        let Some(ec) = &self.config.echo_canceller else {
            return;
        };

        let sample_rate_hz = self.proc_sample_rate_hz();
        let num_render_channels = self.num_reverse_channels();
        let num_capture_channels = self.num_proc_channels();

        let mut config = EchoCanceller3Config::default();
        config.echo_removal_control.transparent_mode = ec.transparent_mode;
        let mut multichannel_config = EchoCanceller3Config::create_default_multichannel_config();
        // lampOS patch: AEC3 requires both configs to agree on this.
        config.filter.export_linear_aec_output = ec.export_linear_aec_output;
        multichannel_config.filter.export_linear_aec_output = ec.export_linear_aec_output;
        let multichannel_config = Some(multichannel_config);
        if ec.export_linear_aec_output {
            // The linear output is the lowest split band: 16 kHz.
            self.capture.linear_aec_output = Some(AudioBuffer::new(
                BAND_SPLIT_RATE,
                num_capture_channels,
                BAND_SPLIT_RATE,
                num_capture_channels,
                BAND_SPLIT_RATE,
            ));
        }

        self.submodules.echo_controller = Some(EchoCanceller3::new(
            config,
            multichannel_config,
            sample_rate_hz,
            num_render_channels,
            num_capture_channels,
        ));
    }

    fn initialize_noise_suppressor(&mut self) {
        self.submodules.noise_suppressors.clear();

        let Some(ns_config) = &self.config.noise_suppression else {
            return;
        };

        let level = map_ns_level(ns_config.level);
        let num_channels = self.num_proc_channels();
        let num_bands = self.proc_sample_rate_hz() / 16000;
        let ns_config = NsConfig {
            target_level: level,
        };

        for _ in 0..num_channels {
            self.submodules
                .noise_suppressors
                .push(NoiseSuppressor::new_with_bands(ns_config, num_bands));
        }
    }

    fn initialize_gain_controller2(&mut self) {
        if self.config.gain_controller2.is_none() {
            self.submodules.gain_controller2 = None;
            return;
        }

        let agc2_config = self.agc2_config_from_api();
        let ivc_config = InputVolumeControllerConfig::default();
        let sample_rate_hz = self.proc_fullband_sample_rate_hz();
        let num_channels = self.num_output_channels();

        self.submodules.gain_controller2 = Some(GainController2::new(
            &agc2_config,
            &ivc_config,
            sample_rate_hz,
            num_channels,
            true, // use_internal_vad
        ));

        if let Some(gc2) = &mut self.submodules.gain_controller2 {
            gc2.set_capture_output_used(self.capture.capture_output_used);
        }
    }

    fn initialize_capture_levels_adjuster(&mut self) {
        if self.config.pre_amplifier.is_some() || self.config.capture_level_adjustment.is_some() {
            let mut pre_gain = 1.0_f32;
            if let Some(ref pre_amp) = self.config.pre_amplifier {
                pre_gain *= pre_amp.fixed_gain_factor;
            }
            if let Some(ref cla) = self.config.capture_level_adjustment {
                pre_gain *= cla.pre_gain_factor;
            }

            let (emulation_enabled, initial_level, post_gain) = if let Some(ref cla) =
                self.config.capture_level_adjustment
            {
                let (emu_enabled, level) = if let Some(ref amge) = cla.analog_mic_gain_emulation {
                    (true, i32::from(amge.initial_level))
                } else {
                    (false, 255)
                };
                (emu_enabled, level, cla.post_gain_factor)
            } else {
                (false, 255, 1.0)
            };

            self.submodules.capture_levels_adjuster = Some(CaptureLevelsAdjuster::new(
                emulation_enabled,
                initial_level,
                pre_gain,
                post_gain,
            ));
        } else {
            self.submodules.capture_levels_adjuster = None;
        }
    }

    fn initialize_residual_echo_detector(&mut self) {
        if let Some(red) = &mut self.submodules.echo_detector {
            red.initialize();
        }
    }

    fn allocate_render_queue(&mut self) {
        if self.submodules.echo_detector.is_some() {
            let max_frames = self
                .formats
                .api_format
                .reverse_input_stream
                .num_frames()
                .max(1);
            if self.red_render_queue.is_none() || self.red_render_queue_buffer.len() < max_frames {
                let prototype = vec![0.0_f32; max_frames];
                self.red_render_queue = Some(SwapQueue::with_prototype(
                    MAX_NUM_FRAMES_TO_BUFFER,
                    prototype,
                ));
                self.red_render_queue_buffer.resize(max_frames, 0.0);
                self.red_capture_queue_buffer.resize(max_frames, 0.0);
            } else if let Some(queue) = &mut self.red_render_queue {
                queue.clear();
            }
        }
    }

    /// Builds an `Agc2Config` from the public API config.
    fn agc2_config_from_api(&self) -> Agc2Config {
        match &self.config.gain_controller2 {
            Some(gc2) => Agc2Config {
                fixed_digital: FixedDigitalConfig {
                    gain_db: gc2.fixed_digital.gain_db,
                },
                adaptive_digital: match &gc2.adaptive_digital {
                    Some(ad) => Agc2AdaptiveDigitalConfig {
                        enabled: true,
                        headroom_db: ad.headroom_db,
                        max_gain_db: ad.max_gain_db,
                        initial_gain_db: ad.initial_gain_db,
                        max_gain_change_db_per_second: ad.max_gain_change_db_per_second,
                        max_output_noise_level_dbfs: ad.max_output_noise_level_dbfs,
                    },
                    None => Agc2AdaptiveDigitalConfig {
                        enabled: false,
                        ..Default::default()
                    },
                },
                input_volume_controller: Agc2InputVolumeControllerConfig {
                    enabled: gc2.input_volume_controller,
                },
            },
            None => Agc2Config::default(),
        }
    }
}

/// Choose a suitable internal processing rate.
fn suitable_process_rate(
    minimum_rate: u32,
    max_splitting_rate: u32,
    band_splitting_required: bool,
) -> u32 {
    let uppermost_native_rate = if band_splitting_required {
        max_splitting_rate
    } else {
        48000
    };
    for rate in [16000u32, 32000, 48000] {
        if rate >= uppermost_native_rate {
            return uppermost_native_rate;
        }
        if rate >= minimum_rate {
            return rate;
        }
    }
    uppermost_native_rate
}

/// Configure the downmix method on an AudioBuffer.
fn set_downmix_method(buffer: &mut AudioBuffer, method: DownmixMethod) {
    match method {
        DownmixMethod::AverageChannels => buffer.set_downmixing_by_averaging(),
        DownmixMethod::UseFirstChannel => buffer.set_downmixing_to_specific_channel(0),
    }
}

/// Map the public NS level to the internal suppression level.
fn map_ns_level(level: NoiseSuppressionLevel) -> SuppressionLevel {
    match level {
        NoiseSuppressionLevel::Low => SuppressionLevel::K6dB,
        NoiseSuppressionLevel::Moderate => SuppressionLevel::K12dB,
        NoiseSuppressionLevel::High => SuppressionLevel::K18dB,
        NoiseSuppressionLevel::VeryHigh => SuppressionLevel::K21dB,
    }
}

/// Returns `true` if the sample rate supports multi-band (sub-band) processing.
///
/// Multi-band splitting is only meaningful above the band-split rate (16 kHz).
fn sample_rate_supports_multi_band(sample_rate_hz: u32) -> bool {
    sample_rate_hz > BAND_SPLIT_RATE as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CaptureLevelAdjustment, EchoCanceller, GainController2, NoiseSuppression};

    #[test]
    fn suitable_process_rate_basic() {
        assert_eq!(suitable_process_rate(8000, 32000, true), 16000);
        assert_eq!(suitable_process_rate(16000, 32000, true), 16000);
        assert_eq!(suitable_process_rate(24000, 32000, true), 32000);
        assert_eq!(suitable_process_rate(44100, 32000, true), 32000);
    }

    #[test]
    fn suitable_process_rate_no_splitting() {
        assert_eq!(suitable_process_rate(8000, 32000, false), 16000);
        assert_eq!(suitable_process_rate(44100, 32000, false), 48000);
    }

    #[test]
    fn suitable_process_rate_48k_max() {
        assert_eq!(suitable_process_rate(8000, 48000, true), 16000);
        assert_eq!(suitable_process_rate(44100, 48000, true), 48000);
    }

    #[test]
    fn create_default() {
        let apm = AudioProcessingImpl::with_config(Config::default());
        assert!(apm.capture.capture_audio.is_some());
        assert!(apm.submodules.echo_controller.is_none());
        assert!(apm.submodules.noise_suppressors.is_empty());
        assert!(apm.submodules.gain_controller2.is_none());
        assert!(apm.submodules.capture_levels_adjuster.is_none());
    }

    #[test]
    fn create_with_echo_canceller() {
        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        };
        let apm = AudioProcessingImpl::with_config(config);
        assert!(apm.submodules.echo_controller.is_some());
    }

    #[test]
    fn create_with_noise_suppression() {
        let config = Config {
            noise_suppression: Some(NoiseSuppression::default()),
            ..Default::default()
        };
        let apm = AudioProcessingImpl::with_config(config);
        assert_eq!(apm.submodules.noise_suppressors.len(), 1);
    }

    #[test]
    fn create_with_gain_controller2() {
        let config = Config {
            gain_controller2: Some(GainController2::default()),
            ..Default::default()
        };
        let apm = AudioProcessingImpl::with_config(config);
        assert!(apm.submodules.gain_controller2.is_some());
    }

    #[test]
    fn create_with_capture_level_adjustment() {
        let config = Config {
            capture_level_adjustment: Some(CaptureLevelAdjustment {
                pre_gain_factor: 2.0,
                post_gain_factor: 0.5,
                ..Default::default()
            }),
            ..Default::default()
        };
        let apm = AudioProcessingImpl::with_config(config);
        assert!(apm.submodules.capture_levels_adjuster.is_some());
    }

    #[test]
    fn apply_config_enables_echo_canceller() {
        let mut apm = AudioProcessingImpl::with_config(Config::default());
        assert!(apm.submodules.echo_controller.is_none());

        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        };
        apm.apply_config(config);
        assert!(apm.submodules.echo_controller.is_some());
    }

    #[test]
    fn apply_config_disables_noise_suppression() {
        let config = Config {
            noise_suppression: Some(NoiseSuppression::default()),
            ..Default::default()
        };
        let mut apm = AudioProcessingImpl::with_config(config);
        assert!(!apm.submodules.noise_suppressors.is_empty());

        let config2 = Config::default();
        apm.apply_config(config2);
        assert!(apm.submodules.noise_suppressors.is_empty());
    }

    #[test]
    fn high_pass_filter_enforced_by_aec() {
        let config = Config {
            echo_canceller: Some(EchoCanceller {
                enforce_high_pass_filtering: true,
                ..Default::default()
            }),
            // high_pass_filter is None (disabled)
            ..Default::default()
        };
        let apm = AudioProcessingImpl::with_config(config);
        assert!(apm.submodules.high_pass_filter.is_some());
    }

    #[test]
    fn statistics_default() {
        let apm = AudioProcessingImpl::with_config(Config::default());
        let stats = apm.get_statistics();
        assert!(stats.echo_return_loss.is_none());
        assert!(stats.delay_ms.is_none());
    }

    #[test]
    fn initialize_with_different_rates() {
        let mut apm = AudioProcessingImpl::with_config(Config::default());
        let config = ProcessingConfig {
            input_stream: StreamConfig::new(48000, 2),
            output_stream: StreamConfig::new(48000, 2),
            reverse_input_stream: StreamConfig::new(48000, 2),
            reverse_output_stream: StreamConfig::new(48000, 2),
        };
        apm.initialize_with_config(config);
        assert!(apm.capture.capture_audio.is_some());
        assert!(apm.render.render_audio.is_some());
    }
}
