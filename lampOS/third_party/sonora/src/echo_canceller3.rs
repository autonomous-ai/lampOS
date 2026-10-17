//! Echo Canceller 3 (AEC3) — top-level wrapper.
//!
//! Ported from `modules/audio_processing/aec3/echo_canceller3.h/cc`.
//!
//! Coordinates the render/capture pipeline by splitting 10 ms frames into
//! sub-frames, blocking them into 64-sample blocks, and forwarding to the
//! underlying `BlockProcessor`. A `SwapQueue` bridges the render and capture
//! paths.

use std::mem;

use sonora_aec3::api_call_jitter_metrics::ApiCallJitterMetrics;
use sonora_aec3::block::Block;
use sonora_aec3::block_framer::BlockFramer;
use sonora_aec3::block_processor::{BlockProcessor, BlockProcessorMetricsOutput};
use sonora_aec3::common::{
    MAX_NUM_BANDS, RENDER_TRANSFER_QUEUE_SIZE_FRAMES, SUB_FRAME_LENGTH, num_bands_for_rate,
    valid_full_band_rate,
};
use sonora_aec3::config::EchoCanceller3Config;
use sonora_aec3::frame_blocker::FrameBlocker;
use sonora_aec3::multi_channel_content_detector::MultiChannelContentDetector;

use crate::audio_buffer::AudioBuffer;
use crate::config_selector::ConfigSelector;
use crate::high_pass_filter::HighPassFilter;
use crate::swap_queue::SwapQueue;

/// Split-band size (samples per band in a 10 ms frame at the internal rate).
const SPLIT_BAND_SIZE: usize = 160;

// ---------------------------------------------------------------------------
// BlockDelayBuffer — fixed capture delay
// ---------------------------------------------------------------------------

/// Applies a fixed delay to the split-band audio in an `AudioBuffer`.
///
/// Ported from `modules/audio_processing/aec3/block_delay_buffer.h/cc`.
#[derive(Debug)]
struct BlockDelayBuffer {
    frame_length: usize,
    delay: usize,
    /// `buf[channel][band]` — circular delay line.
    buf: Vec<Vec<Vec<f32>>>,
    last_insert: usize,
}

impl BlockDelayBuffer {
    fn new(
        num_channels: usize,
        num_bands: usize,
        frame_length: usize,
        delay_samples: usize,
    ) -> Self {
        Self {
            frame_length,
            delay: delay_samples,
            buf: vec![vec![vec![0.0_f32; delay_samples]; num_bands]; num_channels],
            last_insert: 0,
        }
    }

    fn delay_signal(&mut self, frame: &mut AudioBuffer) {
        debug_assert_eq!(self.buf.len(), frame.num_channels());
        if self.delay == 0 {
            return;
        }

        let num_bands = self.buf[0].len();
        let num_channels = self.buf.len();
        let delay = self.delay;

        let mut i = self.last_insert;
        for ch in 0..num_channels {
            debug_assert_eq!(self.buf[ch].len(), frame.num_bands());
            debug_assert_eq!(self.buf[ch].len(), num_bands);

            for band in 0..num_bands {
                debug_assert_eq!(delay, self.buf[ch][band].len());
                i = self.last_insert;

                let frame_data = frame.split_band_mut(ch, band);
                let buf = &mut self.buf[ch][band];

                for sample in &mut frame_data[..self.frame_length] {
                    mem::swap(&mut buf[i], sample);
                    i = if i < delay - 1 { i + 1 } else { 0 };
                }
            }
        }

        self.last_insert = i;
    }
}

// ---------------------------------------------------------------------------
// RenderWriter — copies AudioBuffer into the render SwapQueue
// ---------------------------------------------------------------------------

/// Copies split-band render audio into the render transfer queue, optionally
/// applying a high-pass filter to the echo reference.
#[derive(Debug)]
struct RenderWriter {
    num_bands: usize,
    num_channels: usize,
    high_pass_filter: Option<HighPassFilter>,
    /// `render_queue_input_frame[band][channel]` — temporary storage.
    render_queue_input_frame: Vec<Vec<Vec<f32>>>,
}

impl RenderWriter {
    fn new(config: &EchoCanceller3Config, num_bands: usize, num_channels: usize) -> Self {
        let high_pass_filter = if config.filter.high_pass_filter_echo_reference {
            Some(HighPassFilter::new(16000, num_channels))
        } else {
            None
        };
        Self {
            num_bands,
            num_channels,
            high_pass_filter,
            render_queue_input_frame: vec![
                vec![vec![0.0_f32; SPLIT_BAND_SIZE]; num_channels];
                num_bands
            ],
        }
    }

    fn insert(
        &mut self,
        input: &AudioBuffer,
        render_transfer_queue: &mut SwapQueue<Vec<Vec<Vec<f32>>>>,
    ) {
        debug_assert_eq!(SPLIT_BAND_SIZE, input.num_frames_per_band());
        debug_assert_eq!(self.num_bands, input.num_bands());
        debug_assert_eq!(self.num_channels, input.num_channels());

        // Copy from AudioBuffer into the temporary frame.
        copy_buffer_into_frame(
            input,
            self.num_bands,
            self.num_channels,
            &mut self.render_queue_input_frame,
        );

        // Optionally apply high-pass filter to band 0.
        if let Some(hpf) = &mut self.high_pass_filter {
            hpf.process_channels(&mut self.render_queue_input_frame[0]);
        }

        let _ = render_transfer_queue.insert(&mut self.render_queue_input_frame);
    }
}

// ---------------------------------------------------------------------------
// Helper: copy AudioBuffer → frame (Vec<Vec<Vec<f32>>>)
// ---------------------------------------------------------------------------

fn copy_buffer_into_frame(
    buffer: &AudioBuffer,
    num_bands: usize,
    num_channels: usize,
    frame: &mut [Vec<Vec<f32>>],
) {
    debug_assert_eq!(num_bands, frame.len());
    debug_assert_eq!(num_channels, frame[0].len());
    debug_assert_eq!(SPLIT_BAND_SIZE, frame[0][0].len());
    for (band, frame_band) in frame.iter_mut().enumerate().take(num_bands) {
        for (channel, frame_ch) in frame_band.iter_mut().enumerate().take(num_channels) {
            let src = buffer.split_band(channel, band);
            frame_ch[..SPLIT_BAND_SIZE].copy_from_slice(&src[..SPLIT_BAND_SIZE]);
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: detect microphone saturation
// ---------------------------------------------------------------------------

fn detect_saturation(y: &[f32]) -> bool {
    y.iter().any(|&v| v >= 32700.0 || v <= -32700.0)
}

// ---------------------------------------------------------------------------
// Sub-frame fill helpers
// ---------------------------------------------------------------------------

/// Fills a sub-frame view from an `AudioBuffer`.
///
/// `sub_frame_view[band][channel]` will be populated with 80 samples starting
/// at `sub_frame_index * SUB_FRAME_LENGTH` within each split band.
fn fill_sub_frame_from_audio_buffer(
    frame: &AudioBuffer,
    sub_frame_index: usize,
    sub_frame_view: &mut [Vec<Vec<f32>>],
) {
    debug_assert!(sub_frame_index <= 1);
    debug_assert_eq!(frame.num_bands(), sub_frame_view.len());
    debug_assert_eq!(frame.num_channels(), sub_frame_view[0].len());
    let offset = sub_frame_index * SUB_FRAME_LENGTH;
    for (band, sfv_band) in sub_frame_view.iter_mut().enumerate() {
        for (channel, sfv_ch) in sfv_band.iter_mut().enumerate() {
            let src = frame.split_band(channel, band);
            sfv_ch.copy_from_slice(&src[offset..offset + SUB_FRAME_LENGTH]);
        }
    }
}

/// Writes a sub-frame view back into an `AudioBuffer`.
fn write_sub_frame_to_audio_buffer(
    sub_frame_view: &[Vec<Vec<f32>>],
    sub_frame_index: usize,
    frame: &mut AudioBuffer,
) {
    let offset = sub_frame_index * SUB_FRAME_LENGTH;
    for (band, sfv_band) in sub_frame_view.iter().enumerate() {
        for (channel, sfv_ch) in sfv_band.iter().enumerate() {
            let dst = frame.split_band_mut(channel, band);
            dst[offset..offset + SUB_FRAME_LENGTH].copy_from_slice(sfv_ch);
        }
    }
}

/// Fills a sub-frame view from a raw render frame, with optional downmixing.
///
/// `render_frame[band][channel]` contains SPLIT_BAND_SIZE samples.
fn fill_sub_frame_from_render_frame(
    proper_downmix_needed: bool,
    render_frame: &mut [Vec<Vec<f32>>],
    sub_frame_index: usize,
    sub_frame_view: &mut [Vec<Vec<f32>>],
) {
    debug_assert!(sub_frame_index <= 1);
    debug_assert_eq!(render_frame.len(), sub_frame_view.len());

    let frame_num_channels = render_frame[0].len();
    let sub_frame_num_channels = sub_frame_view[0].len();
    let offset = sub_frame_index * SUB_FRAME_LENGTH;

    if frame_num_channels > sub_frame_num_channels {
        debug_assert_eq!(sub_frame_num_channels, 1);
        if proper_downmix_needed {
            // Average all channels into channel 0.
            for band in render_frame.iter_mut() {
                for ch in 1..frame_num_channels {
                    for k in 0..SUB_FRAME_LENGTH {
                        band[0][offset + k] += band[ch][offset + k];
                    }
                }
                let scale = 1.0 / frame_num_channels as f32;
                for k in 0..SUB_FRAME_LENGTH {
                    band[0][offset + k] *= scale;
                }
            }
        }
        for band in 0..render_frame.len() {
            sub_frame_view[band][0]
                .copy_from_slice(&render_frame[band][0][offset..offset + SUB_FRAME_LENGTH]);
        }
    } else {
        debug_assert_eq!(frame_num_channels, sub_frame_num_channels);
        for band in 0..render_frame.len() {
            for channel in 0..frame_num_channels {
                sub_frame_view[band][channel].copy_from_slice(
                    &render_frame[band][channel][offset..offset + SUB_FRAME_LENGTH],
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// EchoCanceller3
// ---------------------------------------------------------------------------

/// Top-level echo canceller coordinating render/capture processing.
#[derive(Debug)]
pub(crate) struct EchoCanceller3 {
    sample_rate_hz: usize,
    num_bands: usize,
    num_render_input_channels: usize,
    num_render_channels_to_aec: usize,
    num_capture_channels: usize,

    config_selector: ConfigSelector,
    multichannel_content_detector: MultiChannelContentDetector,

    // Render path.
    render_writer: RenderWriter,
    render_transfer_queue: SwapQueue<Vec<Vec<Vec<f32>>>>,
    render_queue_output_frame: Vec<Vec<Vec<f32>>>,
    render_blocker: FrameBlocker,
    render_block: Block,
    render_sub_frame_view: Vec<Vec<Vec<f32>>>,

    // Capture path.
    output_framer: BlockFramer,
    capture_blocker: FrameBlocker,
    capture_block: Block,
    capture_sub_frame_view: Vec<Vec<Vec<f32>>>,

    // Optional linear output.
    linear_output_framer: Option<BlockFramer>,
    linear_output_block: Option<Block>,
    linear_output_sub_frame_view: Option<Vec<Vec<Vec<f32>>>>,

    // Optional capture delay.
    block_delay_buffer: Option<BlockDelayBuffer>,

    // Core processor.
    block_processor: BlockProcessor,

    // State.
    saturated_microphone_signal: bool,
    api_call_metrics: ApiCallJitterMetrics,
}

impl EchoCanceller3 {
    pub(crate) fn new(
        config: EchoCanceller3Config,
        multichannel_config: Option<EchoCanceller3Config>,
        sample_rate_hz: usize,
        num_render_channels: usize,
        num_capture_channels: usize,
    ) -> Self {
        debug_assert!(valid_full_band_rate(sample_rate_hz));

        let num_bands = num_bands_for_rate(sample_rate_hz);
        debug_assert!(num_bands <= MAX_NUM_BANDS);

        let config_selector =
            ConfigSelector::new(config.clone(), multichannel_config, num_render_channels);
        let active = config_selector.active_config();

        let multichannel_content_detector = MultiChannelContentDetector::new(
            active.multi_channel.detect_stereo_content,
            num_render_channels,
            active.multi_channel.stereo_detection_threshold,
            active
                .multi_channel
                .stereo_detection_timeout_threshold_seconds,
            active.multi_channel.stereo_detection_hysteresis_seconds,
        );

        let block_delay_buffer = if active.delay.fixed_capture_delay_samples > 0 {
            Some(BlockDelayBuffer::new(
                num_capture_channels,
                num_bands,
                SPLIT_BAND_SIZE,
                active.delay.fixed_capture_delay_samples,
            ))
        } else {
            None
        };

        let render_writer = RenderWriter::new(active, num_bands, num_render_channels);

        let render_transfer_queue = SwapQueue::with_prototype(
            RENDER_TRANSFER_QUEUE_SIZE_FRAMES,
            vec![vec![vec![0.0_f32; SPLIT_BAND_SIZE]; num_render_channels]; num_bands],
        );

        let render_queue_output_frame =
            vec![vec![vec![0.0_f32; SPLIT_BAND_SIZE]; num_render_channels]; num_bands];

        let output_framer = BlockFramer::new(num_bands, num_capture_channels);
        let capture_blocker = FrameBlocker::new(num_bands, num_capture_channels);

        let (linear_output_framer, linear_output_block, linear_output_sub_frame_view) =
            if active.filter.export_linear_aec_output {
                (
                    Some(BlockFramer::new(1, num_capture_channels)),
                    Some(Block::new(1, num_capture_channels)),
                    Some(vec![
                        vec![
                            vec![0.0_f32; SUB_FRAME_LENGTH];
                            num_capture_channels
                        ];
                        1
                    ]),
                )
            } else {
                (None, None, None)
            };

        // Initial render channels is 1 (mono) until stereo content is detected.
        let num_render_channels_to_aec = 1;
        let render_blocker = FrameBlocker::new(num_bands, num_render_channels_to_aec);
        let render_block = Block::new(num_bands, num_render_channels_to_aec);
        let render_sub_frame_view =
            vec![vec![vec![0.0_f32; SUB_FRAME_LENGTH]; num_render_channels_to_aec]; num_bands];

        let block_processor = BlockProcessor::new(
            config_selector.active_config(),
            sample_rate_hz,
            num_render_channels_to_aec,
            num_capture_channels,
        );

        let capture_block = Block::new(num_bands, num_capture_channels);
        let capture_sub_frame_view =
            vec![vec![vec![0.0_f32; SUB_FRAME_LENGTH]; num_capture_channels]; num_bands];

        Self {
            sample_rate_hz,
            num_bands,
            num_render_input_channels: num_render_channels,
            num_render_channels_to_aec,
            num_capture_channels,
            config_selector,
            multichannel_content_detector,
            render_writer,
            render_transfer_queue,
            render_queue_output_frame,
            render_blocker,
            render_block,
            render_sub_frame_view,
            output_framer,
            capture_blocker,
            capture_block,
            capture_sub_frame_view,
            linear_output_framer,
            linear_output_block,
            linear_output_sub_frame_view,
            block_delay_buffer,
            block_processor,
            saturated_microphone_signal: false,
            api_call_metrics: ApiCallJitterMetrics::new(),
        }
    }

    /// Re-initializes internal state (called when stereo content changes).
    fn initialize(&mut self) {
        self.num_render_channels_to_aec = if self
            .multichannel_content_detector
            .is_proper_multi_channel_content_detected()
        {
            self.num_render_input_channels
        } else {
            1
        };

        self.config_selector.update(
            self.multichannel_content_detector
                .is_proper_multi_channel_content_detected(),
        );

        self.render_block
            .set_num_channels(self.num_render_channels_to_aec);

        self.render_blocker = FrameBlocker::new(self.num_bands, self.num_render_channels_to_aec);

        self.block_processor = BlockProcessor::new(
            self.config_selector.active_config(),
            self.sample_rate_hz,
            self.num_render_channels_to_aec,
            self.num_capture_channels,
        );

        self.render_sub_frame_view =
            vec![
                vec![vec![0.0_f32; SUB_FRAME_LENGTH]; self.num_render_channels_to_aec];
                self.num_bands
            ];
    }

    /// Stores an internal copy of the split-band render signal.
    pub(crate) fn analyze_render(&mut self, render: &AudioBuffer) {
        debug_assert_eq!(render.num_channels(), self.num_render_input_channels);
        self.render_writer
            .insert(render, &mut self.render_transfer_queue);
    }

    /// Detects signal saturation in the full-band capture signal.
    pub(crate) fn analyze_capture(&mut self, capture: &AudioBuffer) {
        self.saturated_microphone_signal = false;
        for channel in 0..capture.num_channels() {
            if detect_saturation(capture.channel(channel)) {
                self.saturated_microphone_signal = true;
                break;
            }
        }
    }

    /// Processes the split-band capture signal to remove echo.
    pub(crate) fn process_capture(
        &mut self,
        capture: &mut AudioBuffer,
        mut linear_output: Option<&mut AudioBuffer>,
        level_change: bool,
    ) {
        debug_assert_eq!(self.num_bands, capture.num_bands());
        debug_assert_eq!(SPLIT_BAND_SIZE, capture.num_frames_per_band());
        debug_assert_eq!(capture.num_channels(), self.num_capture_channels);

        if linear_output.is_some() && self.linear_output_framer.is_none() {
            tracing::error!(
                "Trying to retrieve the linear AEC output without properly configuring AEC3."
            );
            return;
        }

        // Report capture call in the metrics.
        self.api_call_metrics.report_capture_call();

        // Optionally delay the capture signal.
        if self
            .config_selector
            .active_config()
            .delay
            .fixed_capture_delay_samples
            > 0
            && let Some(delay_buf) = &mut self.block_delay_buffer
        {
            delay_buf.delay_signal(capture);
        }

        self.empty_render_queue();

        let aec_reference_is_downmixed_stereo = self
            .multichannel_content_detector
            .is_temporary_multi_channel_content_detected();

        // Process two sub-frames (0 and 1).
        for sub_frame_idx in 0..2 {
            self.process_capture_sub_frame(
                capture,
                linear_output.is_some(),
                level_change,
                aec_reference_is_downmixed_stereo,
                sub_frame_idx,
            );
            // In C++ the linear sub-frame view points into `linear_output`;
            // here it is an owned copy, so write it out (lampOS patch).
            if let (Some(linear), Some(view)) = (
                linear_output.as_deref_mut(),
                self.linear_output_sub_frame_view.as_ref(),
            ) {
                write_sub_frame_to_audio_buffer(view, sub_frame_idx, linear);
            }
        }

        // Process any remaining block from the capture blocker.
        self.process_remaining_capture(
            level_change,
            aec_reference_is_downmixed_stereo,
            linear_output.is_some(),
        );

        // Write sub-frame views back to capture AudioBuffer.
        // (The sub-frame processing already wrote through capture_sub_frame_view,
        //  but we need to write the modified sub-frame data back.)
    }

    /// Processes one capture sub-frame.
    fn process_capture_sub_frame(
        &mut self,
        capture: &mut AudioBuffer,
        has_linear_output: bool,
        level_change: bool,
        aec_reference_is_downmixed_stereo: bool,
        sub_frame_index: usize,
    ) {
        // Fill capture sub-frame view from AudioBuffer.
        fill_sub_frame_from_audio_buffer(
            capture,
            sub_frame_index,
            &mut self.capture_sub_frame_view,
        );

        // Convert sub-frame slices to the format FrameBlocker expects.
        let sub_frame_refs: Vec<Vec<&[f32]>> = self
            .capture_sub_frame_view
            .iter()
            .map(|band| band.iter().map(|ch| ch.as_slice()).collect())
            .collect();

        self.capture_blocker
            .insert_sub_frame_and_extract_block(&sub_frame_refs, &mut self.capture_block);

        // Process through block processor.
        let echo_path_gain_change = level_change || aec_reference_is_downmixed_stereo;
        if has_linear_output {
            self.block_processor.process_capture(
                echo_path_gain_change,
                self.saturated_microphone_signal,
                self.linear_output_block.as_mut(),
                &mut self.capture_block,
            );
        } else {
            self.block_processor.process_capture(
                echo_path_gain_change,
                self.saturated_microphone_signal,
                None,
                &mut self.capture_block,
            );
        }

        // Extract sub-frame from output framer.
        self.output_framer.insert_block_and_extract_sub_frame(
            &self.capture_block,
            &mut self.capture_sub_frame_view,
        );

        // Write the processed sub-frame back to the AudioBuffer.
        write_sub_frame_to_audio_buffer(&self.capture_sub_frame_view, sub_frame_index, capture);

        // Handle linear output framer.
        if has_linear_output
            && let (Some(lo_framer), Some(lo_block), Some(lo_view)) = (
                &mut self.linear_output_framer,
                &self.linear_output_block,
                &mut self.linear_output_sub_frame_view,
            )
        {
            lo_framer.insert_block_and_extract_sub_frame(lo_block, lo_view);
        }
    }

    /// Processes any remaining block in the capture blocker.
    fn process_remaining_capture(
        &mut self,
        level_change: bool,
        aec_reference_is_downmixed_stereo: bool,
        has_linear_output: bool,
    ) {
        if !self.capture_blocker.is_block_available() {
            return;
        }

        self.capture_blocker.extract_block(&mut self.capture_block);

        let echo_path_gain_change = level_change || aec_reference_is_downmixed_stereo;
        if has_linear_output {
            self.block_processor.process_capture(
                echo_path_gain_change,
                self.saturated_microphone_signal,
                self.linear_output_block.as_mut(),
                &mut self.capture_block,
            );
        } else {
            self.block_processor.process_capture(
                echo_path_gain_change,
                self.saturated_microphone_signal,
                None,
                &mut self.capture_block,
            );
        }

        self.output_framer.insert_block(&self.capture_block);

        if has_linear_output
            && let (Some(lo_framer), Some(lo_block)) =
                (&mut self.linear_output_framer, &self.linear_output_block)
        {
            lo_framer.insert_block(lo_block);
        }
    }

    /// Drains all render frames from the transfer queue.
    fn empty_render_queue(&mut self) {
        while self
            .render_transfer_queue
            .remove(&mut self.render_queue_output_frame)
        {
            // Report render call in the metrics.
            self.api_call_metrics.report_render_call();

            if self
                .multichannel_content_detector
                .update_detection(&self.render_queue_output_frame)
            {
                // Reinitialize AEC when proper stereo is detected.
                self.initialize();
            }

            let proper_downmix_needed = self
                .multichannel_content_detector
                .is_temporary_multi_channel_content_detected();

            // Buffer two sub-frames from the render frame.
            for sub_frame_idx in 0..2 {
                fill_sub_frame_from_render_frame(
                    proper_downmix_needed,
                    &mut self.render_queue_output_frame,
                    sub_frame_idx,
                    &mut self.render_sub_frame_view,
                );

                let sub_frame_refs: Vec<Vec<&[f32]>> = self
                    .render_sub_frame_view
                    .iter()
                    .map(|band| band.iter().map(|ch| ch.as_slice()).collect())
                    .collect();

                self.render_blocker
                    .insert_sub_frame_and_extract_block(&sub_frame_refs, &mut self.render_block);
                self.block_processor.buffer_render(&self.render_block);
            }

            // Buffer any remaining block.
            if self.render_blocker.is_block_available() {
                self.render_blocker.extract_block(&mut self.render_block);
                self.block_processor.buffer_render(&self.render_block);
            }
        }
    }

    /// Collects current metrics from the echo canceller.
    pub(crate) fn get_metrics(&self) -> BlockProcessorMetricsOutput {
        self.block_processor.get_metrics()
    }

    /// Provides an optional external estimate of the audio buffer delay.
    pub(crate) fn set_audio_buffer_delay(&mut self, delay_ms: i32) {
        self.block_processor.set_audio_buffer_delay(delay_ms);
    }

    /// Specifies whether the capture output will be used.
    pub(crate) fn set_capture_output_usage(&mut self, capture_output_used: bool) {
        self.block_processor
            .set_capture_output_usage(capture_output_used);
    }

    /// Returns whether echo cancellation is actively processing.
    pub(crate) fn active_processing(&self) -> bool {
        true
    }

    /// Returns whether stereo render processing is active (for testing).
    #[cfg(test)]
    fn stereo_render_processing_active_for_testing(&self) -> bool {
        self.multichannel_content_detector
            .is_proper_multi_channel_content_detected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_audio_buffer(sample_rate_hz: usize, num_channels: usize) -> AudioBuffer {
        AudioBuffer::new(
            sample_rate_hz,
            num_channels,
            sample_rate_hz,
            num_channels,
            sample_rate_hz,
        )
    }

    #[test]
    fn construct_default_config_16khz() {
        let config = EchoCanceller3Config::default();
        let ec3 = EchoCanceller3::new(config, None, 16000, 1, 1);
        assert_eq!(ec3.num_bands, 1);
        assert_eq!(ec3.num_render_input_channels, 1);
        assert_eq!(ec3.num_capture_channels, 1);
        assert!(ec3.active_processing());
    }

    #[test]
    fn construct_default_config_48khz() {
        let config = EchoCanceller3Config::default();
        let ec3 = EchoCanceller3::new(config, None, 48000, 1, 1);
        assert_eq!(ec3.num_bands, 3);
    }

    #[test]
    fn construct_stereo() {
        let config = EchoCanceller3Config::default();
        let ec3 = EchoCanceller3::new(config, None, 48000, 2, 2);
        assert_eq!(ec3.num_render_input_channels, 2);
        assert_eq!(ec3.num_capture_channels, 2);
        // Initially mono until stereo content is detected.
        assert_eq!(ec3.num_render_channels_to_aec, 1);
    }

    #[test]
    fn analyze_and_process_mono_16khz() {
        let config = EchoCanceller3Config::default();
        let mut ec3 = EchoCanceller3::new(config, None, 16000, 1, 1);

        let render = create_audio_buffer(16000, 1);
        let mut capture = create_audio_buffer(16000, 1);

        // Run a few frames.
        for _ in 0..10 {
            ec3.analyze_render(&render);
            ec3.analyze_capture(&capture);
            ec3.process_capture(&mut capture, None, false);
        }
    }

    #[test]
    fn analyze_and_process_mono_48khz() {
        let config = EchoCanceller3Config::default();
        let mut ec3 = EchoCanceller3::new(config, None, 48000, 1, 1);

        let render = create_audio_buffer(48000, 1);
        let mut capture = create_audio_buffer(48000, 1);

        for _ in 0..10 {
            ec3.analyze_render(&render);
            ec3.analyze_capture(&capture);
            ec3.process_capture(&mut capture, None, false);
        }
    }

    #[test]
    fn saturation_detection() {
        let config = EchoCanceller3Config::default();
        let mut ec3 = EchoCanceller3::new(config, None, 16000, 1, 1);

        let mut capture = create_audio_buffer(16000, 1);

        // No saturation with normal signal.
        ec3.analyze_capture(&capture);
        assert!(!ec3.saturated_microphone_signal);

        // Saturated signal.
        let data = capture.channel_mut(0);
        data[0] = 32700.0;
        ec3.analyze_capture(&capture);
        assert!(ec3.saturated_microphone_signal);
    }

    #[test]
    fn stereo_content_detection() {
        let mut config = EchoCanceller3Config::default();
        config.multi_channel.detect_stereo_content = true;
        let ec3 = EchoCanceller3::new(config, None, 16000, 2, 1);

        assert!(!ec3.stereo_render_processing_active_for_testing());
    }

    #[test]
    fn get_metrics_returns_valid() {
        let config = EchoCanceller3Config::default();
        let ec3 = EchoCanceller3::new(config, None, 16000, 1, 1);
        let _metrics = ec3.get_metrics();
    }

    #[test]
    fn set_audio_buffer_delay() {
        let config = EchoCanceller3Config::default();
        let mut ec3 = EchoCanceller3::new(config, None, 16000, 1, 1);
        ec3.set_audio_buffer_delay(50);
    }

    #[test]
    fn set_capture_output_usage() {
        let config = EchoCanceller3Config::default();
        let mut ec3 = EchoCanceller3::new(config, None, 16000, 1, 1);
        ec3.set_capture_output_usage(false);
        ec3.set_capture_output_usage(true);
    }

    #[test]
    fn block_delay_buffer_basic() {
        let mut buf = BlockDelayBuffer::new(1, 1, 160, 10);
        let mut audio = create_audio_buffer(16000, 1);

        // Fill with known pattern.
        for i in 0..160 {
            audio.split_band_mut(0, 0)[i] = (i + 1) as f32;
        }

        buf.delay_signal(&mut audio);

        // First 10 samples should be zeros (the delay).
        for i in 0..10 {
            assert_eq!(
                audio.split_band(0, 0)[i],
                0.0,
                "sample {i} should be delayed"
            );
        }
        // Samples 10..160 should be the original 1..151.
        for i in 10..160 {
            assert_eq!(
                audio.split_band(0, 0)[i],
                (i - 9) as f32,
                "sample {i} mismatch"
            );
        }
    }

    #[test]
    fn block_delay_buffer_zero_delay() {
        let mut buf = BlockDelayBuffer::new(1, 1, 160, 0);
        let mut audio = create_audio_buffer(16000, 1);

        for i in 0..160 {
            audio.split_band_mut(0, 0)[i] = (i + 1) as f32;
        }

        buf.delay_signal(&mut audio);

        // No delay — signal should be unchanged.
        for i in 0..160 {
            assert_eq!(audio.split_band(0, 0)[i], (i + 1) as f32);
        }
    }
}
