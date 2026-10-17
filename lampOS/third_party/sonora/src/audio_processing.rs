//! Public audio processing API.
//!
//! Provides the user-facing [`AudioProcessing`] struct and
//! [`AudioProcessingBuilder`] for constructing configured instances.
//!
//! Ported from `AudioProcessing` / `AudioProcessingBuilderInterface` in
//! `api/audio/audio_processing.h`.

use std::error;
use std::fmt;

use sonora_common_audio::audio_util;

use crate::audio_processing_impl::AudioProcessingImpl;
use crate::config::{Config, PlayoutAudioDeviceInfo, RuntimeSetting};
use crate::stats::AudioProcessingStats;
use crate::stream_config::StreamConfig;
/// Errors returned by audio processing operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Sample rate is outside the supported range.
    InvalidSampleRate {
        /// The rejected sample rate.
        rate: u32,
    },
    /// Channel count is zero.
    InvalidChannelCount {
        /// The rejected channel count.
        count: u16,
    },
    /// Output channel count is incompatible with input channel count.
    ChannelMismatch {
        /// Input channel count.
        input: u16,
        /// Output channel count.
        output: u16,
    },
    /// A stream parameter (e.g. delay) was out of range and was clamped.
    StreamParameterClamped,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSampleRate { rate } => {
                write!(
                    f,
                    "invalid sample rate {rate} (must be {MIN_SAMPLE_RATE}..={MAX_SAMPLE_RATE})"
                )
            }
            Self::InvalidChannelCount { count } => {
                write!(f, "invalid channel count {count}")
            }
            Self::ChannelMismatch { input, output } => {
                write!(
                    f,
                    "channel mismatch: output ({output}) must be 1 or match input ({input})"
                )
            }
            Self::StreamParameterClamped => write!(f, "stream parameter was clamped"),
        }
    }
}

impl error::Error for Error {}
/// Maximum supported sample rate.
const MAX_SAMPLE_RATE: u32 = 384_000;

/// Minimum supported sample rate.
const MIN_SAMPLE_RATE: u32 = 8_000;

/// Result of validating a single audio format.
///
/// Mirrors C++ `AudioFormatValidity` in `audio_processing_impl.cc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormatValidity {
    /// Rate and channels are within supported bounds.
    ValidAndSupported,
    /// Rate is outside [8000, 384000] (interpretable but unsupported).
    ValidButUnsupportedRate,
    /// Zero channels (uninterpretable).
    InvalidChannels,
}

impl FormatValidity {
    /// Whether the format is interpretable (we can read/write its buffers).
    fn is_interpretable(self) -> bool {
        !matches!(self, Self::InvalidChannels)
    }

    /// Convert to the corresponding error code, or `None` for valid formats.
    fn to_error(self, config: &StreamConfig) -> Option<Error> {
        match self {
            Self::ValidAndSupported => None,
            Self::ValidButUnsupportedRate => Some(Error::InvalidSampleRate {
                rate: config.sample_rate_hz(),
            }),
            Self::InvalidChannels => Some(Error::InvalidChannelCount {
                count: config.num_channels(),
            }),
        }
    }
}

/// Validates a single stream config.
///
/// Mirrors C++ `ValidateAudioFormat`.
fn validate_audio_format(config: &StreamConfig) -> FormatValidity {
    if config.num_channels() == 0 {
        return FormatValidity::InvalidChannels;
    }
    let rate = config.sample_rate_hz();
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&rate) {
        return FormatValidity::ValidButUnsupportedRate;
    }
    FormatValidity::ValidAndSupported
}

/// What to do with the output buffer when an error is detected.
///
/// Mirrors C++ `ErrorOutputOption` in `audio_processing_impl.cc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorOutputOption {
    /// Don't touch the output buffer (output format is uninterpretable).
    DoNothing,
    /// Zero-fill the output buffer.
    Silence,
    /// Broadcast the first input channel to all output channels.
    CopyOfFirstChannel,
    /// Copy input to output exactly.
    ExactCopy,
}

/// Determines the error code and output filling strategy for a pair of
/// stream configs.
///
/// Returns `Ok(())` when both formats are valid and channels are compatible
/// (processing should proceed). Returns `Err((error, option))` when an
/// error is detected.
///
/// Mirrors C++ `ChooseErrorOutputOption`.
fn choose_error_output_option(
    input_config: &StreamConfig,
    output_config: &StreamConfig,
) -> Result<(), (Error, ErrorOutputOption)> {
    let input_validity = validate_audio_format(input_config);
    let output_validity = validate_audio_format(output_config);

    // Both valid and channels compatible → success.
    if input_validity == FormatValidity::ValidAndSupported
        && output_validity == FormatValidity::ValidAndSupported
    {
        let out_ch = output_config.num_channels();
        let in_ch = input_config.num_channels();
        if out_ch == 1 || out_ch == in_ch {
            return Ok(());
        }
    }

    // Determine error code: input error takes priority.
    let error = if let Some(e) = input_validity.to_error(input_config) {
        e
    } else if let Some(e) = output_validity.to_error(output_config) {
        e
    } else {
        // Both individually valid but channel mismatch.
        Error::ChannelMismatch {
            input: input_config.num_channels(),
            output: output_config.num_channels(),
        }
    };

    // Determine output option.
    let option = if !output_validity.is_interpretable() {
        // Can't write to uninterpretable output.
        ErrorOutputOption::DoNothing
    } else if !input_validity.is_interpretable() {
        // Can't read from uninterpretable input → silence.
        ErrorOutputOption::Silence
    } else if input_config.sample_rate_hz() != output_config.sample_rate_hz() {
        // Different rates → can't copy, write silence.
        ErrorOutputOption::Silence
    } else if input_config.num_channels() != output_config.num_channels() {
        // Same rate, different channels → broadcast first channel.
        ErrorOutputOption::CopyOfFirstChannel
    } else {
        // Same rate, same channels → exact copy.
        ErrorOutputOption::ExactCopy
    };

    Err((error, option))
}

/// Handles unsupported audio formats for float (deinterleaved) processing.
///
/// On error, fills the output buffer according to C++ semantics, then
/// returns the error. On success, returns `Ok(())` and processing should
/// proceed.
///
/// Mirrors C++ `HandleUnsupportedAudioFormats` (float variant).
fn handle_unsupported_formats_f32(
    src: &[&[f32]],
    input_config: &StreamConfig,
    output_config: &StreamConfig,
    dest: &mut [&mut [f32]],
) -> Result<(), Error> {
    let (error, option) = match choose_error_output_option(input_config, output_config) {
        Ok(()) => return Ok(()),
        Err(pair) => pair,
    };

    let out_frames = output_config.num_frames();
    let out_ch = dest.len();

    match option {
        ErrorOutputOption::DoNothing => {}
        ErrorOutputOption::Silence => {
            for ch_buf in dest.iter_mut().take(out_ch) {
                let len = ch_buf.len().min(out_frames);
                ch_buf[..len].fill(0.0);
            }
        }
        ErrorOutputOption::CopyOfFirstChannel => {
            if let Some(first_in) = src.first() {
                for ch_buf in dest.iter_mut().take(out_ch) {
                    let len = ch_buf.len().min(out_frames).min(first_in.len());
                    ch_buf[..len].copy_from_slice(&first_in[..len]);
                }
            }
        }
        ErrorOutputOption::ExactCopy => {
            for (out_ch_buf, in_ch_buf) in dest.iter_mut().zip(src.iter()) {
                let len = out_ch_buf.len().min(out_frames).min(in_ch_buf.len());
                out_ch_buf[..len].copy_from_slice(&in_ch_buf[..len]);
            }
        }
    }

    Err(error)
}

/// Handles unsupported audio formats for int16 (interleaved) processing.
///
/// On error, fills the output buffer according to C++ semantics, then
/// returns the error. On success, returns `Ok(())` and processing should
/// proceed.
///
/// Mirrors C++ `HandleUnsupportedAudioFormats` (int16 variant).
fn handle_unsupported_formats_i16(
    src: &[i16],
    input_config: &StreamConfig,
    output_config: &StreamConfig,
    dest: &mut [i16],
) -> Result<(), Error> {
    let (error, option) = match choose_error_output_option(input_config, output_config) {
        Ok(()) => return Ok(()),
        Err(pair) => pair,
    };

    let out_frames = output_config.num_frames();
    let out_channels = output_config.num_channels() as usize;
    let in_channels = input_config.num_channels() as usize;
    let out_samples = out_frames * out_channels;

    match option {
        ErrorOutputOption::DoNothing => {}
        ErrorOutputOption::Silence => {
            let len = dest.len().min(out_samples);
            dest[..len].fill(0);
        }
        ErrorOutputOption::CopyOfFirstChannel => {
            // Interleaved: extract first channel sample, broadcast to all
            // output channels.
            for i in 0..out_frames {
                let src_idx = i * in_channels;
                let sample = if src_idx < src.len() { src[src_idx] } else { 0 };
                for ch in 0..out_channels {
                    let dest_idx = i * out_channels + ch;
                    if dest_idx < dest.len() {
                        dest[dest_idx] = sample;
                    }
                }
            }
        }
        ErrorOutputOption::ExactCopy => {
            let len = dest.len().min(out_samples).min(src.len());
            dest[..len].copy_from_slice(&src[..len]);
        }
    }

    Err(error)
}
/// Builder for constructing an [`AudioProcessing`] instance.
///
/// # Example
/// ```
/// use sonora::{AudioProcessing, Config, StreamConfig};
/// use sonora::config::{EchoCanceller, NoiseSuppression};
///
/// let config = Config {
///     echo_canceller: Some(EchoCanceller::default()),
///     noise_suppression: Some(NoiseSuppression::default()),
///     ..Default::default()
/// };
///
/// let apm = AudioProcessing::builder()
///     .config(config)
///     .capture_config(StreamConfig::new(16000, 1))
///     .render_config(StreamConfig::new(16000, 1))
///     .build();
/// ```
#[derive(Debug)]
pub struct AudioProcessingBuilder {
    config: Config,
    capture_config: StreamConfig,
    render_config: StreamConfig,
    echo_detector: bool,
}

impl Default for AudioProcessingBuilder {
    fn default() -> Self {
        Self {
            config: Config::default(),
            capture_config: DEFAULT_STREAM_CONFIG,
            render_config: DEFAULT_STREAM_CONFIG,
            echo_detector: false,
        }
    }
}

/// Default stream config: 16 kHz mono.
const DEFAULT_STREAM_CONFIG: StreamConfig = StreamConfig::new(16000, 1);

impl AudioProcessingBuilder {
    /// Set the initial processing configuration.
    pub fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    /// Set the capture (near-end / microphone) stream format.
    ///
    /// Used as both input and output config for capture processing.
    pub fn capture_config(mut self, config: StreamConfig) -> Self {
        self.capture_config = config;
        self
    }

    /// Set the render (far-end / playback) stream format.
    ///
    /// Used as both input and output config for render processing.
    pub fn render_config(mut self, config: StreamConfig) -> Self {
        self.render_config = config;
        self
    }

    /// Enable the residual echo detector.
    ///
    /// The echo detector is opt-in (matching C++ `SetEchoDetector()`).
    /// It provides `residual_echo_likelihood` statistics.
    pub fn echo_detector(mut self, enabled: bool) -> Self {
        self.echo_detector = enabled;
        self
    }

    /// Build the [`AudioProcessing`] instance.
    pub fn build(self) -> AudioProcessing {
        let mut inner = AudioProcessingImpl::with_config(self.config);

        if self.echo_detector {
            inner.set_echo_detector();
        }

        use crate::audio_processing_impl::ProcessingConfig;
        let processing_config = ProcessingConfig {
            input_stream: self.capture_config,
            output_stream: self.capture_config,
            reverse_input_stream: self.render_config,
            reverse_output_stream: self.render_config,
        };
        inner.initialize_with_config(processing_config);

        AudioProcessing {
            inner,
            capture_config: self.capture_config,
            render_config: self.render_config,
            stream_delay_ms: 0,
            was_stream_delay_set: false,
        }
    }
}
/// Audio processing engine providing echo cancellation, noise suppression,
/// automatic gain control, and other audio processing capabilities.
///
/// The engine operates on two audio streams on a frame-by-frame basis.
/// Frames of the **capture** (near-end / microphone) stream, on which all
/// processing is applied, are passed to `process_capture_*`. Frames of the
/// **render** (far-end / playback) stream are passed to `process_render_*`.
/// The engine should be placed in the signal chain as close to the audio
/// hardware abstraction layer (HAL) as possible.
///
/// # Frame size
///
/// All processing operates on ~10 ms chunks of linear PCM audio.
/// Sample rates from 8 kHz to 384 kHz are accepted. The float interfaces
/// use deinterleaved data (`&[&[f32]]` per channel, range `[-1, 1]`), while
/// the i16 interfaces use interleaved data.
///
/// # Thread safety
///
/// `AudioProcessing` is `Send + Sync`. However, for lowest overhead, the
/// recommended usage pattern is:
/// - Call stream setters (`set_stream_analog_level`, `set_stream_delay_ms`)
///   and `process_capture_*` from the same thread.
/// - Call `apply_config` from any thread, but never concurrently with
///   `config()`.
///
/// # Usage
///
/// 1. Create an instance via [`AudioProcessing::builder()`] or
///    [`AudioProcessing::new()`].
/// 2. For each audio frame (~10 ms):
///    - Call [`process_render_f32()`](AudioProcessing::process_render_f32)
///      with the far-end (render/playback) audio.
///    - Call [`process_capture_f32()`](AudioProcessing::process_capture_f32)
///      with the near-end (capture/microphone) audio.
/// 3. Apply configuration changes via [`apply_config()`](AudioProcessing::apply_config).
///
/// Both f32 (deinterleaved) and i16 (interleaved) interfaces are provided.
#[derive(Debug)]
pub struct AudioProcessing {
    pub(crate) inner: AudioProcessingImpl,
    pub(crate) capture_config: StreamConfig,
    pub(crate) render_config: StreamConfig,
    stream_delay_ms: i32,
    was_stream_delay_set: bool,
}

impl AudioProcessing {
    /// Creates a new instance with default configuration (16 kHz mono).
    pub fn new() -> Self {
        Self::builder().build()
    }

    /// Returns a builder for constructing an instance with custom configuration.
    pub fn builder() -> AudioProcessingBuilder {
        AudioProcessingBuilder::default()
    }

    /// Applies a new processing configuration at runtime.
    ///
    /// Selectively reinitializes submodules as needed. May cause brief
    /// audio artifacts — prefer setting the initial config via the builder.
    pub fn apply_config(&mut self, config: Config) {
        self.inner.apply_config(config);
    }

    /// Returns the current processing configuration.
    pub fn config(&self) -> &Config {
        self.inner.config()
    }

    /// Reinitializes the processing pipeline with new stream configurations.
    ///
    /// This sets all four stream configs (capture input/output, render
    /// input/output) at once and reinitializes internal state accordingly.
    /// Typically used by the FFI layer; Rust callers should prefer the builder
    /// or the `_with_config` processing method variants.
    pub fn initialize(
        &mut self,
        input: StreamConfig,
        output: StreamConfig,
        reverse_input: StreamConfig,
        reverse_output: StreamConfig,
    ) {
        use crate::audio_processing_impl::ProcessingConfig;
        self.capture_config = input;
        self.render_config = reverse_input;
        self.inner.initialize_with_config(ProcessingConfig {
            input_stream: input,
            output_stream: output,
            reverse_input_stream: reverse_input,
            reverse_output_stream: reverse_output,
        });
    }

    /// Sets the capture pre-gain as a linear factor.
    ///
    /// Applied at the next `process_capture_*` call.
    pub fn set_capture_pre_gain(&mut self, gain: f32) {
        self.inner
            .set_runtime_setting(RuntimeSetting::CapturePreGain(gain));
    }

    /// Sets the capture post-gain as a linear factor.
    ///
    /// Applied at the next `process_capture_*` call.
    pub fn set_capture_post_gain(&mut self, gain: f32) {
        self.inner
            .set_runtime_setting(RuntimeSetting::CapturePostGain(gain));
    }

    /// Sets the fixed post-gain in dB (range `0.0..=90.0`).
    ///
    /// Applied at the next `process_capture_*` call.
    pub fn set_capture_fixed_post_gain(&mut self, gain_db: f32) {
        self.inner
            .set_runtime_setting(RuntimeSetting::CaptureFixedPostGain(gain_db));
    }

    /// Notifies that the playout (render) volume has changed.
    ///
    /// Applied at the next `process_capture_*` call.
    pub fn set_playout_volume(&mut self, volume: i32) {
        self.inner
            .set_runtime_setting(RuntimeSetting::PlayoutVolumeChange(volume));
    }

    /// Notifies that the playout (render) audio device has changed.
    ///
    /// Applied at the next `process_render_*` call.
    pub fn set_playout_audio_device(&mut self, id: i32, max_volume: i32) {
        self.inner
            .set_runtime_setting(RuntimeSetting::PlayoutAudioDeviceChange(
                PlayoutAudioDeviceInfo { id, max_volume },
            ));
    }

    /// Sets whether the capture output is being used.
    ///
    /// When `false`, some components may optimize by skipping work.
    /// Applied at the next `process_capture_*` call.
    pub fn set_capture_output_used(&mut self, used: bool) {
        self.inner
            .set_runtime_setting(RuntimeSetting::CaptureOutputUsed(used));
    }

    /// Returns current processing statistics.
    pub fn statistics(&self) -> &AudioProcessingStats {
        self.inner.get_statistics()
    }

    /// Sets the applied input volume (e.g. from the OS mixer).
    ///
    /// Must be called before [`process_capture_f32()`](Self::process_capture_f32) if the input
    /// volume controller is enabled. Value must be in range `[0, 255]`.
    pub fn set_stream_analog_level(&mut self, level: i32) {
        self.inner.set_applied_input_volume(level);
    }

    /// Returns the recommended analog level from AGC.
    ///
    /// Should be called after [`process_capture_f32()`](Self::process_capture_f32) to obtain the
    /// recommended new analog level for the audio HAL.
    pub fn recommended_stream_analog_level(&self) -> i32 {
        /// Default volume when neither recommended nor applied is available.
        /// Matches C++ `kFallBackInputVolume` in `audio_processing_impl.cc`.
        const FALLBACK_INPUT_VOLUME: i32 = 255;

        self.inner
            .recommended_input_volume()
            .or_else(|| self.inner.applied_input_volume())
            .unwrap_or(FALLBACK_INPUT_VOLUME)
    }

    /// Sets the delay in ms between render and capture.
    ///
    /// This must be called if echo cancellation is enabled. On the client-side
    /// the delay can be expressed as:
    ///
    /// ```text
    /// delay = (t_render - t_analyze) + (t_process - t_capture)
    /// ```
    ///
    /// where `t_analyze` is the time a frame is passed to `process_render_*`,
    /// `t_render` is the time its first sample is rendered by the audio hardware,
    /// `t_capture` is the time a frame's first sample is captured, and
    /// `t_process` is the time it is passed to `process_capture_*`.
    ///
    /// The delay is clamped to `[0, 500]`. Returns `Err(StreamParameterClamped)`
    /// if clamping was necessary (processing still proceeds).
    pub fn set_stream_delay_ms(&mut self, delay: i32) -> Result<(), Error> {
        self.was_stream_delay_set = true;
        let mut clamped = delay;
        let mut warning = false;
        if clamped < 0 {
            clamped = 0;
            warning = true;
        }
        if clamped > 500 {
            clamped = 500;
            warning = true;
        }
        self.stream_delay_ms = clamped;
        self.inner.set_stream_delay_ms(clamped);
        if warning {
            Err(Error::StreamParameterClamped)
        } else {
            Ok(())
        }
    }

    /// Returns the current stream delay in ms.
    pub fn stream_delay_ms(&self) -> i32 {
        self.stream_delay_ms
    }

    /// The internal capture processing sample rate.
    pub fn proc_sample_rate_hz(&self) -> usize {
        self.inner.proc_sample_rate_hz()
    }

    /// Copies AEC3's linear output for the last processed capture frame
    /// into `output`, one array per channel the echo canceller processes
    /// (one unless `pipeline.multi_channel_capture`), with values in
    /// `[-1.0, 1.0]` as for [`process_capture_f32`](Self::process_capture_f32).
    ///
    /// This is the capture signal less the echo that AEC3's adaptive linear
    /// filter predicts, before its residual echo suppressor and comfort
    /// noise and before noise suppression or gain control. It is at 16 kHz
    /// (the lowest split band), 160 samples per 10 ms frame, whatever the
    /// stream rates. Before AEC3 has received render audio it does not
    /// process capture, and this keeps its previous (initially zero) value,
    /// as in WebRTC.
    ///
    /// Returns `false`, leaving `output` unchanged, unless the echo
    /// canceller is enabled with
    /// [`EchoCanceller::export_linear_aec_output`](crate::config::EchoCanceller::export_linear_aec_output)
    /// and `output` has one array per channel. Ported from
    /// `AudioProcessingImpl::GetLinearAecOutput` (lampOS patch).
    pub fn linear_aec_output(&self, output: &mut [[f32; 160]]) -> bool {
        let Some(linear) = self.inner.linear_aec_output() else {
            return false;
        };
        if linear.num_channels() != output.len() || linear.num_frames() != 160 {
            return false;
        }
        for (channel, target) in output.iter_mut().enumerate() {
            audio_util::float_s16_to_float_slice(linear.channel(channel), target);
        }
        true
    }

    /// Processes a capture audio frame (float, deinterleaved).
    ///
    /// Uses the stream config set via the builder. Each element of `src` /
    /// `dest` is one channel. Values in `[-1.0, 1.0]`.
    pub fn process_capture_f32(
        &mut self,
        src: &[&[f32]],
        dest: &mut [&mut [f32]],
    ) -> Result<(), Error> {
        let cfg = self.capture_config;
        self.process_capture_f32_with_config(src, &cfg, &cfg, dest)
    }

    /// Processes a capture audio frame with explicit per-call configs.
    ///
    /// Use this when input and output formats differ (e.g. resampling).
    pub fn process_capture_f32_with_config(
        &mut self,
        src: &[&[f32]],
        input_config: &StreamConfig,
        output_config: &StreamConfig,
        dest: &mut [&mut [f32]],
    ) -> Result<(), Error> {
        handle_unsupported_formats_f32(src, input_config, output_config, dest)?;
        self.inner
            .process_stream(src, input_config, output_config, dest);
        Ok(())
    }

    /// Processes a render (far-end / playback) audio frame (float, deinterleaved).
    ///
    /// Uses the stream config set via the builder.
    pub fn process_render_f32(
        &mut self,
        src: &[&[f32]],
        dest: &mut [&mut [f32]],
    ) -> Result<(), Error> {
        let cfg = self.render_config;
        self.process_render_f32_with_config(src, &cfg, &cfg, dest)
    }

    /// Processes a render audio frame with explicit per-call configs.
    pub fn process_render_f32_with_config(
        &mut self,
        src: &[&[f32]],
        input_config: &StreamConfig,
        output_config: &StreamConfig,
        dest: &mut [&mut [f32]],
    ) -> Result<(), Error> {
        handle_unsupported_formats_f32(src, input_config, output_config, dest)?;
        self.inner
            .process_reverse_stream(src, input_config, output_config, dest);
        Ok(())
    }

    /// Processes a capture audio frame (int16, interleaved).
    ///
    /// Uses the stream config set via the builder.
    pub fn process_capture_i16(&mut self, src: &[i16], dest: &mut [i16]) -> Result<(), Error> {
        let cfg = self.capture_config;
        self.process_capture_i16_with_config(src, &cfg, &cfg, dest)
    }

    /// Processes a capture audio frame (int16) with explicit per-call configs.
    pub fn process_capture_i16_with_config(
        &mut self,
        src: &[i16],
        input_config: &StreamConfig,
        output_config: &StreamConfig,
        dest: &mut [i16],
    ) -> Result<(), Error> {
        handle_unsupported_formats_i16(src, input_config, output_config, dest)?;
        self.inner
            .process_stream_i16(src, input_config, output_config, dest);
        Ok(())
    }

    /// Processes a render (far-end / playback) audio frame (int16, interleaved).
    ///
    /// Uses the stream config set via the builder.
    pub fn process_render_i16(&mut self, src: &[i16], dest: &mut [i16]) -> Result<(), Error> {
        let cfg = self.render_config;
        self.process_render_i16_with_config(src, &cfg, &cfg, dest)
    }

    /// Processes a render audio frame (int16) with explicit per-call configs.
    pub fn process_render_i16_with_config(
        &mut self,
        src: &[i16],
        input_config: &StreamConfig,
        output_config: &StreamConfig,
        dest: &mut [i16],
    ) -> Result<(), Error> {
        handle_unsupported_formats_i16(src, input_config, output_config, dest)?;
        self.inner
            .process_reverse_stream_i16(src, input_config, output_config, dest);
        Ok(())
    }
}

impl Default for AudioProcessing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AdaptiveDigital, EchoCanceller, GainController2, HighPassFilter, NoiseSuppression,
        NoiseSuppressionLevel, Pipeline, PreAmplifier,
    };

    const fn send<T: Send>() {}
    const _: () = send::<AudioProcessing>();

    const fn sync<T: Sync>() {}
    const _: () = sync::<AudioProcessing>();

    #[test]
    fn builder_creates_default_instance() {
        let apm = AudioProcessing::builder().build();
        assert_eq!(apm.stream_delay_ms(), 0);
        // Fallback: no recommended, no applied → 255 (C++ kFallBackInputVolume).
        assert_eq!(apm.recommended_stream_analog_level(), 255);
    }

    #[test]
    fn builder_with_config() {
        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            noise_suppression: Some(NoiseSuppression::default()),
            ..Default::default()
        };
        let _apm = AudioProcessing::builder().config(config).build();
    }

    #[test]
    fn set_stream_delay_clamps_negative() {
        let mut apm = AudioProcessing::new();
        let result = apm.set_stream_delay_ms(-10);
        assert_eq!(result, Err(Error::StreamParameterClamped));
        assert_eq!(apm.stream_delay_ms(), 0);
    }

    #[test]
    fn set_stream_delay_clamps_high() {
        let mut apm = AudioProcessing::new();
        let result = apm.set_stream_delay_ms(600);
        assert_eq!(result, Err(Error::StreamParameterClamped));
        assert_eq!(apm.stream_delay_ms(), 500);
    }

    #[test]
    fn set_stream_delay_valid_range() {
        let mut apm = AudioProcessing::new();
        assert!(apm.set_stream_delay_ms(50).is_ok());
        assert_eq!(apm.stream_delay_ms(), 50);
    }

    #[test]
    fn process_capture_f32_with_config_validates_bad_rate() {
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(100, 1); // Bad rate
        let output_config = StreamConfig::new(16000, 1);
        let src_data = [0.0f32; 1];
        let src: &[&[f32]] = &[&src_data];
        let mut dest_data = [0.0f32; 160];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
        let result = apm.process_capture_f32_with_config(src, &input_config, &output_config, dest);
        assert_eq!(result, Err(Error::InvalidSampleRate { rate: 100 }));
    }

    #[test]
    fn process_capture_f32_with_config_validates_bad_channels() {
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(16000, 0); // Bad channels
        let output_config = StreamConfig::new(16000, 1);
        let src: &[&[f32]] = &[];
        let mut dest_data = [0.0f32; 160];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
        let result = apm.process_capture_f32_with_config(src, &input_config, &output_config, dest);
        assert_eq!(result, Err(Error::InvalidChannelCount { count: 0 }));
    }

    #[test]
    fn process_capture_f32_with_config_validates_channel_mismatch() {
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(16000, 2);
        let output_config = StreamConfig::new(16000, 3); // Not 1 and not 2
        let src_data = [0.0f32; 160];
        let src: &[&[f32]] = &[&src_data, &src_data];
        let mut dest0 = [0.0f32; 160];
        let mut dest1 = [0.0f32; 160];
        let mut dest2 = [0.0f32; 160];
        let dest: &mut [&mut [f32]] = &mut [&mut dest0, &mut dest1, &mut dest2];
        let result = apm.process_capture_f32_with_config(src, &input_config, &output_config, dest);
        assert_eq!(
            result,
            Err(Error::ChannelMismatch {
                input: 2,
                output: 3
            })
        );
    }

    #[test]
    fn process_capture_i16_with_config_accepts_non_native_rate() {
        // Non-native rates like 44100 Hz are accepted (matching C++ behavior).
        // The AudioBuffer handles internal resampling.
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(44100, 1);
        let output_config = StreamConfig::new(44100, 1);
        let src = [0i16; 441];
        let mut dest = [0i16; 441];
        let result =
            apm.process_capture_i16_with_config(&src, &input_config, &output_config, &mut dest);
        assert!(result.is_ok());
    }

    #[test]
    fn process_capture_f32_with_config_silence_passthrough() {
        let mut apm = AudioProcessing::new();
        let config = StreamConfig::new(16000, 1);
        let src_data = [0.0f32; 160];
        let src: &[&[f32]] = &[&src_data];
        let mut dest_data = [1.0f32; 160]; // Fill with non-zero to verify overwrite
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
        let result = apm.process_capture_f32_with_config(src, &config, &config, dest);
        assert!(result.is_ok());
        // With all defaults (no processing enabled), silence in → silence out.
        for &sample in dest[0].iter() {
            assert!(sample.abs() < 1e-6, "expected silence, got {sample}",);
        }
    }

    #[test]
    fn process_capture_i16_with_config_silence_passthrough() {
        let mut apm = AudioProcessing::new();
        let config = StreamConfig::new(16000, 1);
        let src = [0i16; 160];
        let mut dest = [100i16; 160]; // Fill with non-zero to verify overwrite
        let result = apm.process_capture_i16_with_config(&src, &config, &config, &mut dest);
        assert!(result.is_ok());
        for &sample in dest.iter() {
            assert_eq!(sample, 0, "expected silence");
        }
    }

    #[test]
    fn process_render_f32_with_config_passthrough() {
        let mut apm = AudioProcessing::new();
        let config = StreamConfig::new(16000, 1);
        let src_data = [0.5f32; 160];
        let src: &[&[f32]] = &[&src_data];
        let mut dest_data = [0.0f32; 160];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
        let result = apm.process_render_f32_with_config(src, &config, &config, dest);
        assert!(result.is_ok());
    }

    #[test]
    fn error_display() {
        assert_eq!(
            format!("{}", Error::InvalidSampleRate { rate: 100 }),
            "invalid sample rate 100 (must be 8000..=384000)"
        );
        assert_eq!(
            format!("{}", Error::InvalidChannelCount { count: 0 }),
            "invalid channel count 0"
        );
        assert_eq!(
            format!(
                "{}",
                Error::ChannelMismatch {
                    input: 2,
                    output: 3
                }
            ),
            "channel mismatch: output (3) must be 1 or match input (2)"
        );
        assert_eq!(
            format!("{}", Error::StreamParameterClamped),
            "stream parameter was clamped"
        );
    }

    #[test]
    fn format_handling_f32_error_and_silence_rate_mismatch() {
        // When input rate is unsupported (< 8000), output gets silence.
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(7900, 1);
        let output_config = StreamConfig::new(16000, 1);
        let src_data = [1.0f32; 79]; // 7900 / 100
        let src: &[&[f32]] = &[&src_data];
        let mut dest_data = [42.0f32; 160];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
        let result = apm.process_capture_f32_with_config(src, &input_config, &output_config, dest);
        assert_eq!(result, Err(Error::InvalidSampleRate { rate: 7900 }));
        // Output should be filled with silence.
        for &sample in dest[0].iter() {
            assert_eq!(sample, 0.0, "expected silence");
        }
    }

    #[test]
    fn format_handling_f32_error_and_copy_of_first_channel() {
        // When channel counts differ but rates match, output gets
        // broadcast of first input channel.
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(16000, 3);
        let output_config = StreamConfig::new(16000, 2);
        let ch0 = [0.5f32; 160];
        let ch1 = [0.3f32; 160];
        let ch2 = [0.1f32; 160];
        let src: &[&[f32]] = &[&ch0, &ch1, &ch2];
        let mut dest0 = [42.0f32; 160];
        let mut dest1 = [42.0f32; 160];
        let dest: &mut [&mut [f32]] = &mut [&mut dest0, &mut dest1];
        let result = apm.process_capture_f32_with_config(src, &input_config, &output_config, dest);
        assert_eq!(
            result,
            Err(Error::ChannelMismatch {
                input: 3,
                output: 2
            })
        );
        // Both output channels should be a copy of input channel 0.
        for (i, (&d0, &d1)) in dest[0].iter().zip(dest[1].iter()).enumerate().take(160) {
            assert_eq!(d0, 0.5, "dest[0][{i}] should be ch0 value");
            assert_eq!(d1, 0.5, "dest[1][{i}] should be ch0 value");
        }
    }

    #[test]
    fn format_handling_f32_error_and_exact_copy() {
        // When both formats are unsupported but match exactly, output
        // gets an exact copy of input.
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(7900, 1);
        let output_config = StreamConfig::new(7900, 1);
        let src_data: Vec<f32> = (0..79).map(|i| i as f32 * 0.01).collect();
        let src: &[&[f32]] = &[&src_data];
        let mut dest_data = [42.0f32; 79];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
        let result = apm.process_capture_f32_with_config(src, &input_config, &output_config, dest);
        assert_eq!(result, Err(Error::InvalidSampleRate { rate: 7900 }));
        for i in 0..79 {
            assert_eq!(dest[0][i], src_data[i], "sample {i} should be exact copy");
        }
    }

    #[test]
    fn format_handling_i16_error_and_silence() {
        // When input rate is unsupported and rates differ, output gets silence.
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(7900, 1);
        let output_config = StreamConfig::new(16000, 1);
        let src = [100i16; 79];
        let mut dest = [42i16; 160];
        let result =
            apm.process_capture_i16_with_config(&src, &input_config, &output_config, &mut dest);
        assert_eq!(result, Err(Error::InvalidSampleRate { rate: 7900 }));
        for &sample in dest.iter() {
            assert_eq!(sample, 0, "expected silence");
        }
    }

    #[test]
    fn format_handling_i16_error_and_broadcast_first_channel() {
        // When channel counts differ but rates match (interleaved),
        // output gets broadcast of first input channel.
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(16000, 3);
        let output_config = StreamConfig::new(16000, 2);
        // Interleaved: [ch0_f0, ch1_f0, ch2_f0, ch0_f1, ch1_f1, ch2_f1, ...]
        let mut src = vec![0i16; 160 * 3];
        for i in 0..160 {
            src[i * 3] = (i * 3) as i16; // ch0
            src[i * 3 + 1] = 1000; // ch1
            src[i * 3 + 2] = 2000; // ch2
        }
        let mut dest = vec![42i16; 160 * 2];
        let result =
            apm.process_capture_i16_with_config(&src, &input_config, &output_config, &mut dest);
        assert_eq!(
            result,
            Err(Error::ChannelMismatch {
                input: 3,
                output: 2
            })
        );
        // Each output frame should have ch0 value broadcast to both channels.
        for i in 0..160 {
            let expected = (i * 3) as i16;
            assert_eq!(
                dest[i * 2],
                expected,
                "dest[{}] should be ch0 frame {i}",
                i * 2
            );
            assert_eq!(
                dest[i * 2 + 1],
                expected,
                "dest[{}] should be ch0 frame {i}",
                i * 2 + 1
            );
        }
    }

    #[test]
    fn format_handling_i16_error_and_exact_copy() {
        // When both formats are unsupported but match exactly.
        let mut apm = AudioProcessing::new();
        let input_config = StreamConfig::new(7900, 1);
        let output_config = StreamConfig::new(7900, 1);
        let src: Vec<i16> = (0..79).map(|i| i as i16).collect();
        let mut dest = vec![42i16; 79];
        let result =
            apm.process_capture_i16_with_config(&src, &input_config, &output_config, &mut dest);
        assert_eq!(result, Err(Error::InvalidSampleRate { rate: 7900 }));
        for i in 0..79 {
            assert_eq!(dest[i], src[i], "sample {i} should be exact copy");
        }
    }

    #[test]
    fn format_handling_valid_formats_proceed() {
        // Valid formats should not trigger error handling.
        let input_config = StreamConfig::new(16000, 2);
        let output_config = StreamConfig::new(16000, 1);
        let result = choose_error_output_option(&input_config, &output_config);
        assert!(result.is_ok());
    }

    #[test]
    fn format_handling_zero_channels_is_invalid() {
        let config = StreamConfig::new(16000, 0);
        assert_eq!(
            validate_audio_format(&config),
            FormatValidity::InvalidChannels
        );
    }

    #[test]
    fn format_handling_unsupported_rate() {
        let config = StreamConfig::new(7900, 1);
        assert_eq!(
            validate_audio_format(&config),
            FormatValidity::ValidButUnsupportedRate
        );
    }

    /// Helper: generate a ~10ms frame of sawtooth at the given rate.
    fn sawtooth_frame(sample_rate_hz: usize, num_channels: usize) -> Vec<f32> {
        let num_frames = sample_rate_hz / 100;
        let mut data = vec![0.0f32; num_frames * num_channels];
        for i in 0..num_frames {
            let sample = ((i % 100) as f32 / 100.0) * 2.0 - 1.0;
            for ch in 0..num_channels {
                data[i * num_channels + ch] = sample * 0.5;
            }
        }
        data
    }

    /// Helper: deinterleave a multichannel buffer into per-channel slices.
    fn deinterleave(data: &[f32], num_channels: usize) -> Vec<Vec<f32>> {
        let num_frames = data.len() / num_channels;
        let mut channels = vec![vec![0.0f32; num_frames]; num_channels];
        for i in 0..num_frames {
            for ch in 0..num_channels {
                channels[ch][i] = data[i * num_channels + ch];
            }
        }
        channels
    }

    #[test]
    fn no_processing_when_all_disabled_float() {
        // Matches C++ test: NoProcessingWhenAllComponentsDisabledFloat.
        // With all components disabled, ProcessStream copies input to output.
        let mut apm = AudioProcessing::new();
        let config = StreamConfig::new(16000, 1);
        let num_frames = 160;
        let src_data: Vec<f32> = (0..num_frames)
            .map(|i| (i as f32 / num_frames as f32) * 2.0 - 1.0)
            .collect();
        let src: &[&[f32]] = &[&src_data];
        let mut dest_data = vec![0.0f32; num_frames];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];

        apm.process_capture_f32_with_config(src, &config, &config, dest)
            .unwrap();

        for i in 0..num_frames {
            assert_eq!(
                src_data[i], dest[0][i],
                "sample {i} mismatch: src={}, dest={}",
                src_data[i], dest[0][i]
            );
        }
    }

    #[test]
    fn no_processing_when_all_disabled_float_reverse() {
        let mut apm = AudioProcessing::new();
        let config = StreamConfig::new(16000, 1);
        let num_frames = 160;
        let src_data: Vec<f32> = (0..num_frames)
            .map(|i| (i as f32 / num_frames as f32) * 2.0 - 1.0)
            .collect();
        let src: &[&[f32]] = &[&src_data];
        let mut dest_data = vec![0.0f32; num_frames];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];

        apm.process_render_f32_with_config(src, &config, &config, dest)
            .unwrap();

        for i in 0..num_frames {
            assert_eq!(src_data[i], dest[0][i], "sample {i} mismatch");
        }
    }

    #[test]
    fn no_processing_when_all_disabled_i16() {
        // Matches C++ test: NoProcessingWhenAllComponentsDisabledInt.
        let mut apm = AudioProcessing::new();
        for &rate in &[8000u32, 16000, 32000, 48000] {
            let config = StreamConfig::new(rate, 1);
            let num_frames = config.num_frames();
            let src: Vec<i16> = (0..num_frames)
                .map(|i| ((i as i32 * 200) % 30000 - 15000) as i16)
                .collect();
            let mut dest = vec![0i16; num_frames];

            apm.process_capture_i16_with_config(&src, &config, &config, &mut dest)
                .unwrap();

            for i in 0..num_frames {
                assert_eq!(src[i], dest[i], "rate={rate} sample {i} mismatch");
            }
        }
    }

    #[test]
    fn all_processing_disabled_by_default() {
        // Matches C++ test: AllProcessingDisabledByDefault.
        let apm = AudioProcessing::new();
        let config = apm.config();
        assert!(config.echo_canceller.is_none());
        assert!(config.high_pass_filter.is_none());
        assert!(config.noise_suppression.is_none());
        assert!(config.gain_controller2.is_none());
    }

    #[test]
    fn echo_canceller_processes_multiple_frames() {
        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        };
        let mut apm = AudioProcessing::builder().config(config).build();
        let stream = StreamConfig::new(16000, 1);
        let num_frames = 160;

        // Process 50 frames (500 ms) of render + capture.
        for _ in 0..50 {
            let render = vec![0.1f32; num_frames];
            let render_src: &[&[f32]] = &[&render];
            let mut render_dest = vec![0.0f32; num_frames];
            let render_dest: &mut [&mut [f32]] = &mut [&mut render_dest];
            apm.process_render_f32_with_config(render_src, &stream, &stream, render_dest)
                .unwrap();

            let capture = vec![0.05f32; num_frames];
            let capture_src: &[&[f32]] = &[&capture];
            let mut capture_dest = vec![0.0f32; num_frames];
            let capture_dest: &mut [&mut [f32]] = &mut [&mut capture_dest];
            apm.process_capture_f32_with_config(capture_src, &stream, &stream, capture_dest)
                .unwrap();
        }
    }

    #[test]
    fn noise_suppression_processes_multiple_frames() {
        let config = Config {
            noise_suppression: Some(NoiseSuppression {
                level: NoiseSuppressionLevel::High,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut apm = AudioProcessing::builder().config(config).build();
        let stream = StreamConfig::new(16000, 1);
        let num_frames = 160;

        for _ in 0..50 {
            let capture = sawtooth_frame(16000, 1);
            let channels = deinterleave(&capture, 1);
            let src: Vec<&[f32]> = channels.iter().map(|c| c.as_slice()).collect();
            let mut dest_data = vec![0.0f32; num_frames];
            let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
            apm.process_capture_f32_with_config(&src, &stream, &stream, dest)
                .unwrap();
        }
    }

    #[test]
    fn gain_controller2_processes_multiple_frames() {
        let config = Config {
            gain_controller2: Some(GainController2 {
                adaptive_digital: Some(AdaptiveDigital::default()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut apm = AudioProcessing::builder().config(config).build();
        let stream = StreamConfig::new(16000, 1);
        let num_frames = 160;

        for _ in 0..50 {
            let capture = sawtooth_frame(16000, 1);
            let channels = deinterleave(&capture, 1);
            let src: Vec<&[f32]> = channels.iter().map(|c| c.as_slice()).collect();
            let mut dest_data = vec![0.0f32; num_frames];
            let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
            apm.process_capture_f32_with_config(&src, &stream, &stream, dest)
                .unwrap();
        }
    }

    #[test]
    fn high_pass_filter_processes_multiple_frames() {
        let config = Config {
            high_pass_filter: Some(HighPassFilter::default()),
            ..Default::default()
        };
        let mut apm = AudioProcessing::builder().config(config).build();
        let stream = StreamConfig::new(16000, 1);
        let num_frames = 160;

        for _ in 0..50 {
            let capture = sawtooth_frame(16000, 1);
            let channels = deinterleave(&capture, 1);
            let src: Vec<&[f32]> = channels.iter().map(|c| c.as_slice()).collect();
            let mut dest_data = vec![0.0f32; num_frames];
            let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
            apm.process_capture_f32_with_config(&src, &stream, &stream, dest)
                .unwrap();
        }
    }

    #[test]
    fn all_components_enabled_processes_multiple_frames() {
        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            noise_suppression: Some(NoiseSuppression::default()),
            high_pass_filter: Some(HighPassFilter::default()),
            gain_controller2: Some(GainController2 {
                adaptive_digital: Some(AdaptiveDigital::default()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut apm = AudioProcessing::builder().config(config).build();
        let stream = StreamConfig::new(16000, 1);
        let num_frames = 160;

        for _ in 0..100 {
            // Render.
            let render = sawtooth_frame(16000, 1);
            let render_ch = deinterleave(&render, 1);
            let render_src: Vec<&[f32]> = render_ch.iter().map(|c| c.as_slice()).collect();
            let mut render_dest = vec![0.0f32; num_frames];
            let render_dest: &mut [&mut [f32]] = &mut [&mut render_dest];
            apm.process_render_f32_with_config(&render_src, &stream, &stream, render_dest)
                .unwrap();

            // Capture.
            let capture = sawtooth_frame(16000, 1);
            let capture_ch = deinterleave(&capture, 1);
            let capture_src: Vec<&[f32]> = capture_ch.iter().map(|c| c.as_slice()).collect();
            let mut capture_dest = vec![0.0f32; num_frames];
            let capture_dest: &mut [&mut [f32]] = &mut [&mut capture_dest];
            apm.process_capture_f32_with_config(&capture_src, &stream, &stream, capture_dest)
                .unwrap();
        }
    }

    #[test]
    fn config_change_mid_stream() {
        let mut apm = AudioProcessing::new();
        let stream = StreamConfig::new(16000, 1);
        let num_frames = 160;
        let silence = vec![0.0f32; num_frames];
        let src: &[&[f32]] = &[&silence];
        let mut dest_data = vec![0.0f32; num_frames];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];

        // Process a few frames with default config.
        for _ in 0..10 {
            apm.process_capture_f32_with_config(src, &stream, &stream, dest)
                .unwrap();
        }

        // Enable echo canceller mid-stream.
        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            ..Default::default()
        };
        apm.apply_config(config);

        // Process more frames — should not crash.
        for _ in 0..10 {
            apm.process_capture_f32_with_config(src, &stream, &stream, dest)
                .unwrap();
        }

        // Enable noise suppression mid-stream.
        let mut config = apm.config().clone();
        config.noise_suppression = Some(NoiseSuppression::default());
        apm.apply_config(config);

        for _ in 0..10 {
            apm.process_capture_f32_with_config(src, &stream, &stream, dest)
                .unwrap();
        }
    }

    #[test]
    fn sample_rate_change_mid_stream() {
        let mut apm = AudioProcessing::new();

        // Process at 16 kHz.
        let stream_16k = StreamConfig::new(16000, 1);
        let src_16k = vec![0.0f32; 160];
        let src: &[&[f32]] = &[&src_16k];
        let mut dest_16k = vec![0.0f32; 160];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_16k];
        for _ in 0..5 {
            apm.process_capture_f32_with_config(src, &stream_16k, &stream_16k, dest)
                .unwrap();
        }

        // Switch to 48 kHz — should reinitialize automatically.
        let stream_48k = StreamConfig::new(48000, 1);
        let src_48k = vec![0.0f32; 480];
        let src: &[&[f32]] = &[&src_48k];
        let mut dest_48k = vec![0.0f32; 480];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_48k];
        for _ in 0..5 {
            apm.process_capture_f32_with_config(src, &stream_48k, &stream_48k, dest)
                .unwrap();
        }
    }

    #[test]
    fn pre_amplifier_applies_gain() {
        // Matches C++ test: PreAmplifier (simplified).
        let config = Config {
            pre_amplifier: Some(PreAmplifier {
                fixed_gain_factor: 2.0,
            }),
            ..Default::default()
        };
        let mut apm = AudioProcessing::builder().config(config).build();

        let stream = StreamConfig::new(16000, 1);
        let num_frames = 160;

        // Run enough frames for the filter to settle.
        for _ in 0..20 {
            let src_data: Vec<f32> = (0..num_frames)
                .map(|i| ((i % 3) as f32 - 1.0) * 0.3)
                .collect();
            let src: &[&[f32]] = &[&src_data];
            let mut dest_data = vec![0.0f32; num_frames];
            let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
            apm.process_capture_f32_with_config(src, &stream, &stream, dest)
                .unwrap();
        }

        // Process one more frame and check that the output has roughly 2x power.
        let src_data: Vec<f32> = (0..num_frames)
            .map(|i| ((i % 3) as f32 - 1.0) * 0.3)
            .collect();
        let input_power: f32 = src_data.iter().map(|&s| s * s).sum::<f32>() / num_frames as f32;

        let src: &[&[f32]] = &[&src_data];
        let mut dest_data = vec![0.0f32; num_frames];
        let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
        apm.process_capture_f32_with_config(src, &stream, &stream, dest)
            .unwrap();

        let output_power: f32 = dest[0].iter().map(|&s| s * s).sum::<f32>() / num_frames as f32;

        // 2x gain → 4x power. Allow some tolerance for filter settling.
        assert!(
            output_power > input_power * 3.0,
            "expected ~4x power, got input={input_power}, output={output_power}",
        );
    }

    #[test]
    fn runtime_setting_capture_pre_gain() {
        let config = Config {
            pre_amplifier: Some(PreAmplifier {
                fixed_gain_factor: 1.0,
            }),
            ..Default::default()
        };
        let mut apm = AudioProcessing::builder().config(config).build();

        // Change gain via runtime setting.
        apm.set_capture_pre_gain(2.0);

        let stream = StreamConfig::new(16000, 1);
        let num_frames = 160;

        // Process enough frames for the setting to take effect.
        for _ in 0..20 {
            let src_data = vec![0.1f32; num_frames];
            let src: &[&[f32]] = &[&src_data];
            let mut dest_data = vec![0.0f32; num_frames];
            let dest: &mut [&mut [f32]] = &mut [&mut dest_data];
            apm.process_capture_f32_with_config(src, &stream, &stream, dest)
                .unwrap();
        }

        // Verify the config was updated.
        let config = apm.config();
        assert_eq!(
            config.pre_amplifier.as_ref().unwrap().fixed_gain_factor,
            2.0
        );
    }

    #[test]
    fn stereo_processing_does_not_crash() {
        let config = Config {
            echo_canceller: Some(EchoCanceller::default()),
            pipeline: Pipeline {
                multi_channel_render: true,
                multi_channel_capture: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut apm = AudioProcessing::builder().config(config).build();

        let stream = StreamConfig::new(16000, 2);
        let num_frames = 160;

        for _ in 0..20 {
            // Render (stereo).
            let render_l = vec![0.1f32; num_frames];
            let render_r = vec![0.05f32; num_frames];
            let render_src: &[&[f32]] = &[&render_l, &render_r];
            let mut render_dest_l = vec![0.0f32; num_frames];
            let mut render_dest_r = vec![0.0f32; num_frames];
            let render_dest: &mut [&mut [f32]] = &mut [&mut render_dest_l, &mut render_dest_r];
            apm.process_render_f32_with_config(render_src, &stream, &stream, render_dest)
                .unwrap();

            // Capture (stereo).
            let capture_l = vec![0.05f32; num_frames];
            let capture_r = vec![0.02f32; num_frames];
            let capture_src: &[&[f32]] = &[&capture_l, &capture_r];
            let mut capture_dest_l = vec![0.0f32; num_frames];
            let mut capture_dest_r = vec![0.0f32; num_frames];
            let capture_dest: &mut [&mut [f32]] = &mut [&mut capture_dest_l, &mut capture_dest_r];
            apm.process_capture_f32_with_config(capture_src, &stream, &stream, capture_dest)
                .unwrap();
        }
    }
}
