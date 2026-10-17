//! Input volume controller for automatic microphone gain adjustment.
//!
//! Manages the recommended microphone input volume by analyzing captured audio
//! for clipping and adjusting volume based on speech level RMS error.
//!
//! Ported from `modules/audio_processing/agc2/input_volume_controller.h/cc`.

use crate::audio_buffer::AudioBuffer;
use sonora_agc2::clipping_predictor::{
    ClippingPredictor, ClippingPredictorConfig, ClippingPredictorMode, create_clipping_predictor,
};

/// Amount of error we tolerate in the microphone input volume (presumably due to
/// OS quantization) before we assume the user has manually adjusted the volume.
const VOLUME_QUANTIZATION_SLACK: i32 = 25;

const MAX_INPUT_VOLUME: i32 = 255;

/// Maximum absolute RMS error in dBFS.
const MAX_ABS_RMS_ERROR_DBFS: i32 = 15;

/// Maps input volumes (0..=255) to gains in dB.
const GAIN_MAP: [i32; 256] = [
    -56, -54, -52, -50, -48, -47, -45, -43, -42, -40, -38, -37, -35, -34, -33, -31, -30, -29, -27,
    -26, -25, -24, -23, -22, -20, -19, -18, -17, -16, -15, -14, -14, -13, -12, -11, -10, -9, -8,
    -8, -7, -6, -5, -5, -4, -3, -2, -2, -1, 0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9,
    9, 10, 10, 11, 11, 12, 12, 13, 13, 13, 14, 14, 15, 15, 15, 16, 16, 17, 17, 17, 18, 18, 18, 19,
    19, 19, 20, 20, 21, 21, 21, 22, 22, 22, 23, 23, 23, 24, 24, 24, 24, 25, 25, 25, 26, 26, 26, 27,
    27, 27, 28, 28, 28, 28, 29, 29, 29, 30, 30, 30, 30, 31, 31, 31, 32, 32, 32, 32, 33, 33, 33, 33,
    34, 34, 34, 35, 35, 35, 35, 36, 36, 36, 36, 37, 37, 37, 38, 38, 38, 38, 39, 39, 39, 39, 40, 40,
    40, 40, 41, 41, 41, 41, 42, 42, 42, 42, 43, 43, 43, 44, 44, 44, 44, 45, 45, 45, 45, 46, 46, 46,
    46, 47, 47, 47, 47, 48, 48, 48, 48, 49, 49, 49, 49, 50, 50, 50, 50, 51, 51, 51, 51, 52, 52, 52,
    52, 53, 53, 53, 53, 54, 54, 54, 54, 55, 55, 55, 55, 56, 56, 56, 56, 57, 57, 57, 57, 58, 58, 58,
    58, 59, 59, 59, 59, 60, 60, 60, 60, 61, 61, 61, 61, 62, 62, 62, 62, 63, 63, 63, 63, 64,
];

/// Returns an input volume in the [`min_input_volume`, `MAX_INPUT_VOLUME`] range
/// that reduces `gain_error_db`, which is a gain error estimated when
/// `input_volume` was applied, according to a fixed gain map.
fn compute_volume_update(gain_error_db: i32, input_volume: i32, min_input_volume: i32) -> i32 {
    debug_assert!(input_volume >= 0);
    debug_assert!(input_volume <= MAX_INPUT_VOLUME);
    if gain_error_db == 0 {
        return input_volume;
    }
    let mut new_volume = input_volume;
    if gain_error_db > 0 {
        while GAIN_MAP[new_volume as usize] - GAIN_MAP[input_volume as usize] < gain_error_db
            && new_volume < MAX_INPUT_VOLUME
        {
            new_volume += 1;
        }
    } else {
        while GAIN_MAP[new_volume as usize] - GAIN_MAP[input_volume as usize] > gain_error_db
            && new_volume > min_input_volume
        {
            new_volume -= 1;
        }
    }
    new_volume
}

/// Returns the proportion of samples in the buffer which are at full-scale
/// (and presumably clipped).
fn compute_clipped_ratio(audio: &AudioBuffer) -> f32 {
    let samples_per_channel = audio.num_frames();
    debug_assert!(samples_per_channel > 0);
    let mut num_clipped = 0;
    for ch in 0..audio.num_channels() {
        let channel = audio.channel(ch);
        let mut num_clipped_in_ch = 0;
        for &sample in &channel[..samples_per_channel] {
            if sample >= 32767.0 || sample <= -32768.0 {
                num_clipped_in_ch += 1;
            }
        }
        num_clipped = num_clipped.max(num_clipped_in_ch);
    }
    num_clipped as f32 / samples_per_channel as f32
}

/// Compares `speech_level_dbfs` to the [`target_range_min_dbfs`,
/// `target_range_max_dbfs`] range and returns the error to be compensated via
/// input volume adjustment.
fn get_speech_level_rms_error_db(
    speech_level_dbfs: f32,
    target_range_min_dbfs: i32,
    target_range_max_dbfs: i32,
) -> i32 {
    const MIN_SPEECH_LEVEL_DBFS: f32 = -90.0;
    const MAX_SPEECH_LEVEL_DBFS: f32 = 30.0;
    let speech_level_dbfs = speech_level_dbfs.clamp(MIN_SPEECH_LEVEL_DBFS, MAX_SPEECH_LEVEL_DBFS);

    if speech_level_dbfs > target_range_max_dbfs as f32 {
        (target_range_max_dbfs as f32 - speech_level_dbfs).round() as i32
    } else if speech_level_dbfs < target_range_min_dbfs as f32 {
        (target_range_min_dbfs as f32 - speech_level_dbfs).round() as i32
    } else {
        0
    }
}

/// Configuration for [`InputVolumeController`].
#[derive(Debug, Clone)]
pub(crate) struct InputVolumeControllerConfig {
    pub min_input_volume: i32,
    pub clipped_level_min: i32,
    pub clipped_level_step: i32,
    pub clipped_ratio_threshold: f32,
    pub clipped_wait_frames: i32,
    pub enable_clipping_predictor: bool,
    pub target_range_max_dbfs: i32,
    pub target_range_min_dbfs: i32,
    pub update_input_volume_wait_frames: i32,
    pub speech_probability_threshold: f32,
    pub speech_ratio_threshold: f32,
}

impl Default for InputVolumeControllerConfig {
    fn default() -> Self {
        Self {
            min_input_volume: 20,
            clipped_level_min: 70,
            clipped_level_step: 15,
            clipped_ratio_threshold: 0.1,
            clipped_wait_frames: 300,
            enable_clipping_predictor: false,
            target_range_max_dbfs: -18,
            target_range_min_dbfs: -30,
            update_input_volume_wait_frames: 0,
            speech_probability_threshold: 0.5,
            speech_ratio_threshold: 0.8,
        }
    }
}

/// Per-channel input volume controller.
#[derive(Debug)]
pub(crate) struct MonoInputVolumeController {
    min_input_volume: i32,
    min_input_volume_after_clipping: i32,
    max_input_volume: i32,
    last_recommended_input_volume: i32,
    capture_output_used: bool,
    check_volume_on_next_process: bool,
    startup: bool,
    recommended_input_volume: i32,
    update_input_volume_wait_frames: i32,
    frames_since_update_input_volume: i32,
    speech_frames_since_update_input_volume: i32,
    is_first_frame: bool,
    speech_probability_threshold: f32,
    speech_ratio_threshold: f32,
}

impl MonoInputVolumeController {
    pub(crate) fn new(
        min_input_volume_after_clipping: i32,
        min_input_volume: i32,
        update_input_volume_wait_frames: i32,
        speech_probability_threshold: f32,
        speech_ratio_threshold: f32,
    ) -> Self {
        debug_assert!((0..=255).contains(&min_input_volume));
        debug_assert!((0..=255).contains(&min_input_volume_after_clipping));
        Self {
            min_input_volume,
            min_input_volume_after_clipping,
            max_input_volume: MAX_INPUT_VOLUME,
            last_recommended_input_volume: 0,
            capture_output_used: true,
            check_volume_on_next_process: true,
            startup: true,
            recommended_input_volume: 0,
            update_input_volume_wait_frames: update_input_volume_wait_frames.max(1),
            frames_since_update_input_volume: 0,
            speech_frames_since_update_input_volume: 0,
            is_first_frame: true,
            speech_probability_threshold,
            speech_ratio_threshold,
        }
    }

    pub(crate) fn initialize(&mut self) {
        self.max_input_volume = MAX_INPUT_VOLUME;
        self.capture_output_used = true;
        self.check_volume_on_next_process = true;
        self.frames_since_update_input_volume = 0;
        self.speech_frames_since_update_input_volume = 0;
        self.is_first_frame = true;
    }

    pub(crate) fn handle_capture_output_used_change(&mut self, capture_output_used: bool) {
        if self.capture_output_used == capture_output_used {
            return;
        }
        self.capture_output_used = capture_output_used;
        if capture_output_used {
            self.check_volume_on_next_process = true;
        }
    }

    /// Sets the current input volume.
    pub(crate) fn set_stream_analog_level(&mut self, input_volume: i32) {
        self.recommended_input_volume = input_volume;
    }

    /// Lowers the recommended input volume in response to clipping.
    pub(crate) fn handle_clipping(&mut self, clipped_level_step: i32) {
        debug_assert!(clipped_level_step > 0);
        self.set_max_level(
            self.min_input_volume_after_clipping
                .max(self.max_input_volume - clipped_level_step),
        );
        if self.last_recommended_input_volume > self.min_input_volume_after_clipping {
            self.set_input_volume(
                self.min_input_volume_after_clipping
                    .max(self.last_recommended_input_volume - clipped_level_step),
            );
            self.frames_since_update_input_volume = 0;
            self.speech_frames_since_update_input_volume = 0;
            self.is_first_frame = false;
        }
    }

    /// Adjusts the recommended input volume upwards/downwards depending on
    /// the result of `handle_clipping()` and on `rms_error_db`.
    pub(crate) fn process(&mut self, rms_error_db: Option<f32>, speech_probability: f32) {
        if self.check_volume_on_next_process {
            self.check_volume_on_next_process = false;
            self.check_volume_and_reset();
        }

        // Count frames with a high speech probability as speech.
        if speech_probability >= self.speech_probability_threshold {
            self.speech_frames_since_update_input_volume += 1;
        }

        self.frames_since_update_input_volume += 1;
        if self.frames_since_update_input_volume >= self.update_input_volume_wait_frames {
            let speech_ratio = self.speech_frames_since_update_input_volume as f32
                / self.update_input_volume_wait_frames as f32;

            self.frames_since_update_input_volume = 0;
            self.speech_frames_since_update_input_volume = 0;

            if !self.is_first_frame
                && speech_ratio >= self.speech_ratio_threshold
                && let Some(rms_error) = rms_error_db
            {
                // Convert from f32 to i32, rounding. The C++ code passes int directly,
                // but some callers (tests) pass float.
                self.update_input_volume(rms_error as i32);
            }
        }

        self.is_first_frame = false;
    }

    /// Returns the recommended input volume.
    pub(crate) fn recommended_analog_level(&self) -> i32 {
        self.recommended_input_volume
    }

    pub(crate) fn min_input_volume_after_clipping(&self) -> i32 {
        self.min_input_volume_after_clipping
    }

    fn set_input_volume(&mut self, new_volume: i32) {
        let applied_input_volume = self.recommended_input_volume;
        if applied_input_volume == 0 {
            return;
        }
        if !(0..=MAX_INPUT_VOLUME).contains(&applied_input_volume) {
            return;
        }

        // Detect manual input volume adjustments.
        if applied_input_volume > self.last_recommended_input_volume + VOLUME_QUANTIZATION_SLACK
            || applied_input_volume < self.last_recommended_input_volume - VOLUME_QUANTIZATION_SLACK
        {
            self.last_recommended_input_volume = applied_input_volume;
            if self.last_recommended_input_volume > self.max_input_volume {
                self.set_max_level(self.last_recommended_input_volume);
            }
            self.frames_since_update_input_volume = 0;
            self.speech_frames_since_update_input_volume = 0;
            self.is_first_frame = false;
            return;
        }

        let new_volume = new_volume.min(self.max_input_volume);
        if new_volume == self.last_recommended_input_volume {
            return;
        }

        self.recommended_input_volume = new_volume;
        self.last_recommended_input_volume = new_volume;
    }

    fn set_max_level(&mut self, input_volume: i32) {
        debug_assert!(input_volume >= self.min_input_volume_after_clipping);
        self.max_input_volume = input_volume;
    }

    fn check_volume_and_reset(&mut self) -> i32 {
        let input_volume = self.recommended_input_volume;
        if input_volume == 0 && !self.startup {
            return 0;
        }
        if !(0..=MAX_INPUT_VOLUME).contains(&input_volume) {
            return -1;
        }

        let input_volume = if input_volume < self.min_input_volume {
            self.recommended_input_volume = self.min_input_volume;
            self.min_input_volume
        } else {
            input_volume
        };

        self.last_recommended_input_volume = input_volume;
        self.startup = false;
        self.frames_since_update_input_volume = 0;
        self.speech_frames_since_update_input_volume = 0;
        self.is_first_frame = true;

        0
    }

    fn update_input_volume(&mut self, rms_error_db: i32) {
        let rms_error_db = rms_error_db.clamp(-MAX_ABS_RMS_ERROR_DBFS, MAX_ABS_RMS_ERROR_DBFS);
        if rms_error_db == 0 {
            return;
        }
        self.set_input_volume(compute_volume_update(
            rms_error_db,
            self.last_recommended_input_volume,
            self.min_input_volume,
        ));
    }
}

/// Multi-channel input volume controller.
///
/// Coordinates per-channel [`MonoInputVolumeController`] instances and handles
/// clipping detection/prediction at the multi-channel level.
#[derive(Debug)]
pub(crate) struct InputVolumeController {
    num_capture_channels: usize,
    min_input_volume: i32,
    capture_output_used: bool,
    pub(crate) clipped_level_step: i32,
    pub(crate) clipped_ratio_threshold: f32,
    pub(crate) clipped_wait_frames: i32,
    clipping_predictor: Option<ClippingPredictor>,
    use_clipping_predictor_step: bool,
    frames_since_clipped: i32,
    clipping_rate_log_counter: i32,
    clipping_rate_log: f32,
    target_range_max_dbfs: i32,
    target_range_min_dbfs: i32,
    channel_controllers: Vec<MonoInputVolumeController>,
    channel_controlling_gain: usize,
    recommended_input_volume: i32,
    applied_input_volume: Option<i32>,
}

impl InputVolumeController {
    pub(crate) fn new(num_capture_channels: usize, config: &InputVolumeControllerConfig) -> Self {
        let clipping_predictor_config = ClippingPredictorConfig {
            enabled: config.enable_clipping_predictor,
            mode: ClippingPredictorMode::ClippingEvent,
            ..Default::default()
        };
        let clipping_predictor =
            create_clipping_predictor(num_capture_channels, &clipping_predictor_config);
        let use_clipping_predictor_step =
            clipping_predictor.is_some() && clipping_predictor_config.enabled;

        let mut channel_controllers = Vec::with_capacity(num_capture_channels);
        for _ in 0..num_capture_channels {
            channel_controllers.push(MonoInputVolumeController::new(
                config.clipped_level_min,
                config.min_input_volume,
                config.update_input_volume_wait_frames,
                config.speech_probability_threshold,
                config.speech_ratio_threshold,
            ));
        }

        debug_assert!(!channel_controllers.is_empty());
        debug_assert!(config.clipped_level_step > 0);
        debug_assert!(config.clipped_level_step <= 255);
        debug_assert!(config.clipped_ratio_threshold > 0.0);
        debug_assert!(config.clipped_ratio_threshold < 1.0);
        debug_assert!(config.clipped_wait_frames > 0);

        Self {
            num_capture_channels,
            min_input_volume: config.min_input_volume,
            capture_output_used: true,
            clipped_level_step: config.clipped_level_step,
            clipped_ratio_threshold: config.clipped_ratio_threshold,
            clipped_wait_frames: config.clipped_wait_frames,
            clipping_predictor,
            use_clipping_predictor_step,
            frames_since_clipped: config.clipped_wait_frames,
            clipping_rate_log_counter: 0,
            clipping_rate_log: 0.0,
            target_range_max_dbfs: config.target_range_max_dbfs,
            target_range_min_dbfs: config.target_range_min_dbfs,
            channel_controllers,
            channel_controlling_gain: 0,
            recommended_input_volume: 0,
            applied_input_volume: None,
        }
    }

    pub(crate) fn initialize(&mut self) {
        for controller in &mut self.channel_controllers {
            controller.initialize();
        }
        self.capture_output_used = true;
        self.aggregate_channel_levels();
        self.clipping_rate_log = 0.0;
        self.clipping_rate_log_counter = 0;
        self.applied_input_volume = None;
    }

    pub(crate) fn analyze_input_audio(
        &mut self,
        applied_input_volume: i32,
        audio_buffer: &AudioBuffer,
    ) {
        debug_assert!((0..=255).contains(&applied_input_volume));

        self.set_applied_input_volume(applied_input_volume);

        debug_assert_eq!(audio_buffer.num_channels(), self.channel_controllers.len());

        self.aggregate_channel_levels();
        if !self.capture_output_used {
            return;
        }

        if let Some(predictor) = &mut self.clipping_predictor {
            let channel_slices: Vec<&[f32]> = (0..self.num_capture_channels)
                .map(|ch| audio_buffer.channel(ch))
                .collect();
            predictor.analyze(&channel_slices);
        }

        // Check for clipped samples.
        let clipped_ratio = compute_clipped_ratio(audio_buffer);
        self.clipping_rate_log = self.clipping_rate_log.max(clipped_ratio);
        self.clipping_rate_log_counter += 1;
        const NUM_FRAMES_IN_30_SECONDS: i32 = 3000;
        if self.clipping_rate_log_counter == NUM_FRAMES_IN_30_SECONDS {
            self.clipping_rate_log = 0.0;
            self.clipping_rate_log_counter = 0;
        }

        if self.frames_since_clipped < self.clipped_wait_frames {
            self.frames_since_clipped += 1;
            return;
        }

        let clipping_detected = clipped_ratio > self.clipped_ratio_threshold;
        let mut clipping_predicted = false;
        let mut predicted_step = 0;
        if let Some(predictor) = &self.clipping_predictor {
            for channel in 0..self.num_capture_channels {
                let step = predictor.estimate_clipped_level_step(
                    channel,
                    self.recommended_input_volume,
                    self.clipped_level_step,
                    self.channel_controllers[channel].min_input_volume_after_clipping(),
                    MAX_INPUT_VOLUME,
                );
                if let Some(s) = step {
                    predicted_step = predicted_step.max(s);
                    clipping_predicted = true;
                }
            }
        }

        let mut step = self.clipped_level_step;
        if clipping_predicted {
            predicted_step = predicted_step.max(self.clipped_level_step);
            if self.use_clipping_predictor_step {
                step = predicted_step;
            }
        }

        if clipping_detected || (clipping_predicted && self.use_clipping_predictor_step) {
            for controller in &mut self.channel_controllers {
                controller.handle_clipping(step);
            }
            self.frames_since_clipped = 0;
            if let Some(predictor) = &mut self.clipping_predictor {
                predictor.reset();
            }
        }

        self.aggregate_channel_levels();
    }

    pub(crate) fn recommend_input_volume(
        &mut self,
        speech_probability: f32,
        speech_level_dbfs: Option<f32>,
    ) -> Option<i32> {
        self.applied_input_volume?;

        self.aggregate_channel_levels();
        let _volume_after_clipping_handling = self.recommended_input_volume;

        if !self.capture_output_used {
            return self.applied_input_volume;
        }

        let rms_error_db = speech_level_dbfs.map(|level| {
            get_speech_level_rms_error_db(
                level,
                self.target_range_min_dbfs,
                self.target_range_max_dbfs,
            ) as f32
        });

        for controller in &mut self.channel_controllers {
            controller.process(rms_error_db, speech_probability);
        }

        self.aggregate_channel_levels();
        self.applied_input_volume = None;
        Some(self.recommended_input_volume)
    }

    pub(crate) fn handle_capture_output_used_change(&mut self, capture_output_used: bool) {
        for controller in &mut self.channel_controllers {
            controller.handle_capture_output_used_change(capture_output_used);
        }
        self.capture_output_used = capture_output_used;
    }

    #[cfg(test)]
    pub(crate) fn recommended_input_volume(&self) -> i32 {
        self.recommended_input_volume
    }

    fn set_applied_input_volume(&mut self, input_volume: i32) {
        self.applied_input_volume = Some(input_volume);
        for controller in &mut self.channel_controllers {
            controller.set_stream_analog_level(input_volume);
        }
        self.aggregate_channel_levels();
    }

    fn aggregate_channel_levels(&mut self) {
        let mut new_recommended = self.channel_controllers[0].recommended_analog_level();
        self.channel_controlling_gain = 0;
        for (ch, controller) in self.channel_controllers.iter().enumerate().skip(1) {
            let volume = controller.recommended_analog_level();
            if volume < new_recommended {
                new_recommended = volume;
                self.channel_controlling_gain = ch;
            }
        }

        // Enforce the minimum input volume when a recommendation is made.
        if let Some(applied) = self.applied_input_volume
            && applied > 0
        {
            new_recommended = new_recommended.max(self.min_input_volume);
        }

        self.recommended_input_volume = new_recommended;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIGH_SPEECH_PROBABILITY: f32 = 0.7;
    const LOW_SPEECH_PROBABILITY: f32 = 0.1;
    const SPEECH_RATIO_THRESHOLD: f32 = 0.8;

    /// Runs the MonoInputVolumeController processing sequence following the API
    /// contract. Returns the updated recommended input volume.
    fn update_recommended_input_volume(
        mono_controller: &mut MonoInputVolumeController,
        applied_input_volume: i32,
        speech_probability: f32,
        rms_error_dbfs: Option<f32>,
    ) -> i32 {
        mono_controller.set_stream_analog_level(applied_input_volume);
        assert_eq!(
            mono_controller.recommended_analog_level(),
            applied_input_volume
        );
        mono_controller.process(rms_error_dbfs, speech_probability);
        mono_controller.recommended_analog_level()
    }

    #[test]
    fn check_handle_clipping_lowers_volume() {
        let initial_input_volume = 100;
        let input_volume_step = 29;
        let mut mono_controller = MonoInputVolumeController::new(
            70, // clipped_level_min
            32, // min_mic_level
            3,  // update_input_volume_wait_frames
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller.initialize();

        update_recommended_input_volume(
            &mut mono_controller,
            initial_input_volume,
            LOW_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        mono_controller.handle_clipping(input_volume_step);

        assert_eq!(
            mono_controller.recommended_analog_level(),
            initial_input_volume - input_volume_step
        );
    }

    #[test]
    fn check_process_negative_rms_error_decreases_input_volume() {
        let initial_input_volume = 100;
        let mut mono_controller = MonoInputVolumeController::new(
            64,
            32,
            3,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller.initialize();

        let mut volume = update_recommended_input_volume(
            &mut mono_controller,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        volume = update_recommended_input_volume(
            &mut mono_controller,
            volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        volume = update_recommended_input_volume(
            &mut mono_controller,
            volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert!(volume < initial_input_volume);
    }

    #[test]
    fn check_process_positive_rms_error_increases_input_volume() {
        let initial_input_volume = 100;
        let mut mono_controller = MonoInputVolumeController::new(
            64,
            32,
            3,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller.initialize();

        let mut volume = update_recommended_input_volume(
            &mut mono_controller,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(10.0),
        );
        volume = update_recommended_input_volume(
            &mut mono_controller,
            volume,
            HIGH_SPEECH_PROBABILITY,
            Some(10.0),
        );
        volume = update_recommended_input_volume(
            &mut mono_controller,
            volume,
            HIGH_SPEECH_PROBABILITY,
            Some(10.0),
        );

        assert!(volume > initial_input_volume);
    }

    #[test]
    fn check_process_negative_rms_error_decreases_input_volume_with_limit() {
        let initial_input_volume = 100;
        let mut mono_controller_1 = MonoInputVolumeController::new(
            64,
            32,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        let mut mono_controller_2 = MonoInputVolumeController::new(
            64,
            32,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        let mut mono_controller_3 = MonoInputVolumeController::new(64, 32, 2, 0.7, 0.8);
        mono_controller_1.initialize();
        mono_controller_2.initialize();
        mono_controller_3.initialize();

        // Process RMS errors in the range [-kMaxResidualGainChange, kMaxResidualGainChange].
        let mut volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-14.0),
        );
        volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            volume_1,
            HIGH_SPEECH_PROBABILITY,
            Some(-14.0),
        );
        // Process RMS errors outside the range.
        let mut volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-15.0),
        );
        let mut volume_3 = update_recommended_input_volume(
            &mut mono_controller_3,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-30.0),
        );
        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            HIGH_SPEECH_PROBABILITY,
            Some(-15.0),
        );
        volume_3 = update_recommended_input_volume(
            &mut mono_controller_3,
            volume_3,
            HIGH_SPEECH_PROBABILITY,
            Some(-30.0),
        );

        assert!(volume_1 < initial_input_volume);
        assert!(volume_2 < volume_1);
        assert_eq!(volume_2, volume_3);
    }

    #[test]
    fn check_process_positive_rms_error_increases_input_volume_with_limit() {
        let initial_input_volume = 100;
        let mut mono_controller_1 = MonoInputVolumeController::new(
            64,
            32,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        let mut mono_controller_2 = MonoInputVolumeController::new(
            64,
            32,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        let mut mono_controller_3 = MonoInputVolumeController::new(
            64,
            32,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller_1.initialize();
        mono_controller_2.initialize();
        mono_controller_3.initialize();

        // Process RMS errors in the range.
        let mut volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(14.0),
        );
        volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            volume_1,
            HIGH_SPEECH_PROBABILITY,
            Some(14.0),
        );
        // Process RMS errors outside the range.
        let mut volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(15.0),
        );
        let mut volume_3 = update_recommended_input_volume(
            &mut mono_controller_3,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(30.0),
        );
        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            HIGH_SPEECH_PROBABILITY,
            Some(15.0),
        );
        volume_3 = update_recommended_input_volume(
            &mut mono_controller_3,
            volume_3,
            HIGH_SPEECH_PROBABILITY,
            Some(30.0),
        );

        assert!(volume_1 > initial_input_volume);
        assert!(volume_2 > volume_1);
        assert_eq!(volume_2, volume_3);
    }

    #[test]
    fn check_process_rms_error_decreases_input_volume_repeatedly() {
        let initial_input_volume = 100;
        let mut mono_controller = MonoInputVolumeController::new(
            64,
            32,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller.initialize();

        let mut volume_before = update_recommended_input_volume(
            &mut mono_controller,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        volume_before = update_recommended_input_volume(
            &mut mono_controller,
            volume_before,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert!(volume_before < initial_input_volume);

        let mut volume_after = update_recommended_input_volume(
            &mut mono_controller,
            volume_before,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        volume_after = update_recommended_input_volume(
            &mut mono_controller,
            volume_after,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert!(volume_after < volume_before);
    }

    #[test]
    fn check_process_positive_rms_error_increases_input_volume_repeatedly() {
        let initial_input_volume = 100;
        let mut mono_controller = MonoInputVolumeController::new(
            64,
            32,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller.initialize();

        let mut volume_before = update_recommended_input_volume(
            &mut mono_controller,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(10.0),
        );
        volume_before = update_recommended_input_volume(
            &mut mono_controller,
            volume_before,
            HIGH_SPEECH_PROBABILITY,
            Some(10.0),
        );

        assert!(volume_before > initial_input_volume);

        let mut volume_after = update_recommended_input_volume(
            &mut mono_controller,
            volume_before,
            HIGH_SPEECH_PROBABILITY,
            Some(10.0),
        );
        volume_after = update_recommended_input_volume(
            &mut mono_controller,
            volume_after,
            HIGH_SPEECH_PROBABILITY,
            Some(10.0),
        );

        assert!(volume_after > volume_before);
    }

    #[test]
    fn check_clipped_level_min_is_effective() {
        let initial_input_volume = 100;
        let clipped_level_min = 70;
        let mut mono_controller_1 = MonoInputVolumeController::new(
            clipped_level_min,
            84,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        let mut mono_controller_2 = MonoInputVolumeController::new(
            clipped_level_min,
            84,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller_1.initialize();
        mono_controller_2.initialize();

        // Process one frame to reset the state for `handle_clipping()`.
        assert_eq!(
            update_recommended_input_volume(
                &mut mono_controller_1,
                initial_input_volume,
                LOW_SPEECH_PROBABILITY,
                Some(-10.0),
            ),
            initial_input_volume
        );
        assert_eq!(
            update_recommended_input_volume(
                &mut mono_controller_2,
                initial_input_volume,
                LOW_SPEECH_PROBABILITY,
                Some(-10.0),
            ),
            initial_input_volume
        );

        mono_controller_1.handle_clipping(29);
        mono_controller_2.handle_clipping(31);

        assert_eq!(
            mono_controller_2.recommended_analog_level(),
            clipped_level_min
        );
        assert!(
            mono_controller_2.recommended_analog_level()
                < mono_controller_1.recommended_analog_level()
        );
    }

    #[test]
    fn check_min_mic_level_is_effective() {
        let initial_input_volume = 100;
        let min_mic_level = 64;
        let mut mono_controller_1 = MonoInputVolumeController::new(
            64,
            min_mic_level,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        let mut mono_controller_2 = MonoInputVolumeController::new(
            64,
            min_mic_level,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller_1.initialize();
        mono_controller_2.initialize();

        let mut volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        let mut volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert_eq!(volume_1, initial_input_volume);
        assert_eq!(volume_2, initial_input_volume);

        volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            volume_1,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            HIGH_SPEECH_PROBABILITY,
            Some(-30.0),
        );

        assert!(volume_1 < initial_input_volume);
        assert!(volume_2 < volume_1);
        assert_eq!(volume_2, min_mic_level);
    }

    #[test]
    fn check_update_input_volume_wait_frames_is_effective() {
        let initial_input_volume = 100;
        let mut mono_controller_1 = MonoInputVolumeController::new(
            64,
            84,
            1,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        let mut mono_controller_2 = MonoInputVolumeController::new(
            64,
            84,
            3,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller_1.initialize();
        mono_controller_2.initialize();

        let mut volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        let mut volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert_eq!(volume_1, initial_input_volume);
        assert_eq!(volume_2, initial_input_volume);

        volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            volume_1,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert!(volume_1 < initial_input_volume);
        assert_eq!(volume_2, initial_input_volume);

        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert!(volume_2 < initial_input_volume);
    }

    #[test]
    fn check_speech_probability_threshold_is_effective() {
        let initial_input_volume = 100;
        let speech_probability_threshold = 0.8_f32;
        let mut mono_controller_1 = MonoInputVolumeController::new(
            64,
            84,
            2,
            speech_probability_threshold,
            SPEECH_RATIO_THRESHOLD,
        );
        let mut mono_controller_2 = MonoInputVolumeController::new(
            64,
            84,
            2,
            speech_probability_threshold,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller_1.initialize();
        mono_controller_2.initialize();

        let mut volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            initial_input_volume,
            speech_probability_threshold,
            Some(-10.0),
        );
        let mut volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            initial_input_volume,
            speech_probability_threshold,
            Some(-10.0),
        );

        assert_eq!(volume_1, initial_input_volume);
        assert_eq!(volume_2, initial_input_volume);

        volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            volume_1,
            speech_probability_threshold - 0.1,
            Some(-10.0),
        );
        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            speech_probability_threshold,
            Some(-10.0),
        );

        assert_eq!(volume_1, initial_input_volume);
        assert!(volume_2 < volume_1);
    }

    #[test]
    fn check_speech_ratio_threshold_is_effective() {
        let initial_input_volume = 100;
        let mut mono_controller_1 =
            MonoInputVolumeController::new(64, 84, 4, HIGH_SPEECH_PROBABILITY, 0.75);
        let mut mono_controller_2 =
            MonoInputVolumeController::new(64, 84, 4, HIGH_SPEECH_PROBABILITY, 0.75);
        mono_controller_1.initialize();
        mono_controller_2.initialize();

        let mut volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        let mut volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            volume_1,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            volume_1,
            LOW_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            LOW_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert_eq!(volume_1, initial_input_volume);
        assert_eq!(volume_2, initial_input_volume);

        volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            volume_1,
            LOW_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert_eq!(volume_1, initial_input_volume);
        assert!(volume_2 < volume_1);
    }

    #[test]
    fn check_process_empty_rms_error_does_not_lower_volume() {
        let initial_input_volume = 100;
        let mut mono_controller_1 = MonoInputVolumeController::new(
            64,
            84,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        let mut mono_controller_2 = MonoInputVolumeController::new(
            64,
            84,
            2,
            HIGH_SPEECH_PROBABILITY,
            SPEECH_RATIO_THRESHOLD,
        );
        mono_controller_1.initialize();
        mono_controller_2.initialize();

        let mut volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );
        let mut volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            initial_input_volume,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert_eq!(volume_1, initial_input_volume);
        assert_eq!(volume_2, initial_input_volume);

        volume_1 = update_recommended_input_volume(
            &mut mono_controller_1,
            volume_1,
            HIGH_SPEECH_PROBABILITY,
            None,
        );
        volume_2 = update_recommended_input_volume(
            &mut mono_controller_2,
            volume_2,
            HIGH_SPEECH_PROBABILITY,
            Some(-10.0),
        );

        assert_eq!(volume_1, initial_input_volume);
        assert!(volume_2 < volume_1);
    }

    // --- InputVolumeController tests ---

    const SAMPLE_RATE_HZ: usize = 32000;
    const NUM_CHANNELS: usize = 1;
    const DEFAULT_INITIAL_INPUT_VOLUME: i32 = 128;
    const CLIPPED_MIN: i32 = 165;
    const ABOVE_CLIPPED_THRESHOLD: f32 = 0.2;
    const CLIPPED_LEVEL_STEP: i32 = 15;
    const CLIPPED_RATIO_THRESHOLD: f32 = 0.1;
    const CLIPPED_WAIT_FRAMES: i32 = 300;
    const SPEECH_LEVEL: f32 = -25.0;

    const MAX_SAMPLE: f32 = i16::MAX as f32;

    fn get_test_config() -> InputVolumeControllerConfig {
        InputVolumeControllerConfig {
            clipped_level_min: CLIPPED_MIN,
            clipped_level_step: CLIPPED_LEVEL_STEP,
            clipped_ratio_threshold: CLIPPED_RATIO_THRESHOLD,
            clipped_wait_frames: CLIPPED_WAIT_FRAMES,
            enable_clipping_predictor: false,
            target_range_max_dbfs: -18,
            target_range_min_dbfs: -30,
            update_input_volume_wait_frames: 0,
            speech_probability_threshold: 0.5,
            speech_ratio_threshold: 1.0,
            ..Default::default()
        }
    }

    fn write_audio_buffer_samples(
        samples_value: f32,
        clipped_ratio: f32,
        audio_buffer: &mut AudioBuffer,
    ) {
        let num_channels = audio_buffer.num_channels();
        let num_samples = audio_buffer.num_frames();
        let num_clipping_samples = (clipped_ratio * num_samples as f32) as usize;
        for ch in 0..num_channels {
            let channel = audio_buffer.channel_mut(ch);
            for sample in &mut channel[..num_clipping_samples] {
                *sample = 32767.0;
            }
            for sample in &mut channel[num_clipping_samples..num_samples] {
                *sample = samples_value;
            }
        }
    }

    fn write_alternating_audio_buffer_samples(samples_value: f32, audio_buffer: &mut AudioBuffer) {
        let num_channels = audio_buffer.num_channels();
        let num_frames = audio_buffer.num_frames();
        for ch in 0..num_channels {
            let channel = audio_buffer.channel_mut(ch);
            for i in (0..num_frames).step_by(2) {
                channel[i] = samples_value;
                if i + 1 < num_frames {
                    channel[i + 1] = 0.0;
                }
            }
        }
    }

    struct TestHelper {
        audio_buffer: AudioBuffer,
        controller: InputVolumeController,
    }

    impl TestHelper {
        fn new(config: InputVolumeControllerConfig) -> Self {
            let mut audio_buffer = AudioBuffer::new(
                SAMPLE_RATE_HZ,
                NUM_CHANNELS,
                SAMPLE_RATE_HZ,
                NUM_CHANNELS,
                SAMPLE_RATE_HZ,
            );
            let mut controller = InputVolumeController::new(1, &config);
            controller.initialize();
            write_audio_buffer_samples(0.0, 0.0, &mut audio_buffer);
            Self {
                audio_buffer,
                controller,
            }
        }

        fn call_agc_sequence(
            &mut self,
            applied_input_volume: i32,
            speech_probability: f32,
            speech_level_dbfs: f32,
            num_calls: i32,
        ) -> Option<i32> {
            debug_assert!(num_calls >= 1);
            let mut volume = Some(applied_input_volume);
            for _ in 0..num_calls {
                self.controller.analyze_input_audio(
                    volume.unwrap_or(applied_input_volume),
                    &self.audio_buffer,
                );
                volume = self
                    .controller
                    .recommend_input_volume(speech_probability, Some(speech_level_dbfs));
                if let Some(v) = volume {
                    assert_eq!(v, self.controller.recommended_input_volume());
                }
            }
            volume
        }

        fn call_recommend_input_volume(
            &mut self,
            num_calls: i32,
            initial_volume: i32,
            speech_probability: f32,
            speech_level_dbfs: f32,
        ) -> i32 {
            write_alternating_audio_buffer_samples(0.1 * MAX_SAMPLE, &mut self.audio_buffer);
            let mut volume = initial_volume;
            for _ in 0..num_calls {
                self.controller
                    .analyze_input_audio(volume, &self.audio_buffer);
                let recommended = self
                    .controller
                    .recommend_input_volume(speech_probability, Some(speech_level_dbfs));
                assert!(recommended.is_some());
                volume = recommended.unwrap();
            }
            volume
        }

        fn call_analyze_input_audio(&mut self, num_calls: i32, clipped_ratio: f32) {
            write_audio_buffer_samples(0.0, clipped_ratio, &mut self.audio_buffer);
            for _ in 0..num_calls {
                self.controller.analyze_input_audio(
                    self.controller.recommended_input_volume(),
                    &self.audio_buffer,
                );
            }
        }
    }

    #[test]
    fn startup_min_volume_configuration_respected_when_applied_input_volume_above_min() {
        for min_input_volume in [12, 20] {
            let mut helper = TestHelper::new(InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            });

            assert_eq!(helper.call_agc_sequence(128, 0.9, -80.0, 1).unwrap(), 128);
        }
    }

    #[test]
    fn startup_min_volume_configuration_respected_when_applied_input_volume_maybe_below_min() {
        for min_input_volume in [12, 20] {
            let mut helper = TestHelper::new(InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            });

            assert!(helper.call_agc_sequence(10, 0.9, -80.0, 1).unwrap() >= 10);
        }
    }

    #[test]
    fn startup_min_volume_respected_when_applied_volume_non_zero() {
        for min_input_volume in [12, 20] {
            let mut helper = TestHelper::new(InputVolumeControllerConfig {
                min_input_volume,
                target_range_min_dbfs: -30,
                update_input_volume_wait_frames: 1,
                speech_probability_threshold: 0.5,
                speech_ratio_threshold: 0.5,
                ..get_test_config()
            });

            let volume = helper.call_agc_sequence(1, 0.9, -80.0, 1).unwrap();
            assert_eq!(volume, min_input_volume);
        }
    }

    #[test]
    fn min_volume_repeatedly_respected_when_applied_volume_non_zero() {
        for min_input_volume in [12, 20] {
            let mut helper = TestHelper::new(InputVolumeControllerConfig {
                min_input_volume,
                target_range_min_dbfs: -30,
                update_input_volume_wait_frames: 1,
                speech_probability_threshold: 0.5,
                speech_ratio_threshold: 0.5,
                ..get_test_config()
            });

            for _ in 0..100 {
                let volume = helper.call_agc_sequence(1, 0.9, -80.0, 1).unwrap();
                assert!(volume >= min_input_volume);
            }
        }
    }

    #[test]
    fn startup_min_volume_respected_once_when_applied_volume_zero() {
        for min_input_volume in [12, 20] {
            let mut helper = TestHelper::new(InputVolumeControllerConfig {
                min_input_volume,
                target_range_min_dbfs: -30,
                update_input_volume_wait_frames: 1,
                speech_probability_threshold: 0.5,
                speech_ratio_threshold: 0.5,
                ..get_test_config()
            });

            let volume = helper.call_agc_sequence(0, 0.9, -80.0, 1).unwrap();
            assert_eq!(volume, min_input_volume);

            // No change of volume regardless; applied volume is zero.
            let volume = helper.call_agc_sequence(0, 0.9, -80.0, 1).unwrap();
            assert_eq!(volume, 0);
        }
    }

    #[test]
    fn mic_volume_response_to_rms_error() {
        for min_input_volume in [12, 20] {
            let mut config = get_test_config();
            config.min_input_volume = min_input_volume;
            let mut helper = TestHelper::new(config);
            let mut volume = helper
                .call_agc_sequence(
                    DEFAULT_INITIAL_INPUT_VOLUME,
                    HIGH_SPEECH_PROBABILITY,
                    SPEECH_LEVEL,
                    1,
                )
                .unwrap();

            // Inside the digital gain's window; no change of volume.
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -23.0);
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -28.0);

            // Above the digital gain's window; volume should be increased.
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -29.0);
            assert_eq!(volume, 128);

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -38.0);
            assert_eq!(volume, 156);

            // Inside the digital gain's window; no change of volume.
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -23.0);
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -18.0);

            // Below the digital gain's window; volume should be decreased.
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -17.0);
            assert_eq!(volume, 155);

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -17.0);
            assert_eq!(volume, 151);

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -9.0);
            assert_eq!(volume, 119);
        }
    }

    #[test]
    fn mic_volume_is_limited() {
        for min_input_volume in [12, 20] {
            let mut config = get_test_config();
            config.min_input_volume = min_input_volume;
            let mut helper = TestHelper::new(config);
            let mut volume = helper
                .call_agc_sequence(
                    DEFAULT_INITIAL_INPUT_VOLUME,
                    HIGH_SPEECH_PROBABILITY,
                    SPEECH_LEVEL,
                    1,
                )
                .unwrap();

            // Maximum upwards change is limited.
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -48.0);
            assert_eq!(volume, 183);

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -48.0);
            assert_eq!(volume, 243);

            // Won't go higher than the maximum.
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -48.0);
            assert_eq!(volume, 255);

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, -17.0);
            assert_eq!(volume, 254);

            // Maximum downwards change is limited.
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, 22.0);
            assert_eq!(volume, 194);

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, 22.0);
            assert_eq!(volume, 137);

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, 22.0);
            assert_eq!(volume, 88);

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, 22.0);
            assert_eq!(volume, 54);

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, 22.0);
            assert_eq!(volume, 33);

            // Won't go lower than the minimum.
            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, 22.0);
            assert_eq!(volume, 18_i32.max(min_input_volume));

            volume = helper.call_recommend_input_volume(1, volume, HIGH_SPEECH_PROBABILITY, 22.0);
            assert_eq!(volume, 12_i32.max(min_input_volume));
        }
    }

    #[test]
    fn no_action_while_muted() {
        for min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            };
            let mut helper_1 = TestHelper::new(config.clone());
            let mut helper_2 = TestHelper::new(config);

            let mut volume_1 = helper_1
                .call_agc_sequence(255, HIGH_SPEECH_PROBABILITY, SPEECH_LEVEL, 1)
                .unwrap();
            let mut volume_2 = helper_2
                .call_agc_sequence(255, HIGH_SPEECH_PROBABILITY, SPEECH_LEVEL, 1)
                .unwrap();

            assert_eq!(volume_1, 255);
            assert_eq!(volume_2, 255);

            helper_2.controller.handle_capture_output_used_change(false);

            write_alternating_audio_buffer_samples(MAX_SAMPLE, &mut helper_1.audio_buffer);
            write_alternating_audio_buffer_samples(MAX_SAMPLE, &mut helper_2.audio_buffer);

            volume_1 = helper_1
                .call_agc_sequence(volume_1, HIGH_SPEECH_PROBABILITY, SPEECH_LEVEL, 1)
                .unwrap();
            volume_2 = helper_2
                .call_agc_sequence(volume_2, HIGH_SPEECH_PROBABILITY, SPEECH_LEVEL, 1)
                .unwrap();

            assert!(volume_1 < 255);
            assert_eq!(volume_2, 255);
        }
    }

    #[test]
    fn unmuting_checks_volume_without_raising() {
        for min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            };
            let mut helper = TestHelper::new(config);
            helper.call_agc_sequence(
                DEFAULT_INITIAL_INPUT_VOLUME,
                HIGH_SPEECH_PROBABILITY,
                SPEECH_LEVEL,
                1,
            );

            helper.controller.handle_capture_output_used_change(false);
            helper.controller.handle_capture_output_used_change(true);

            let input_volume = 127;
            assert_eq!(
                helper.call_recommend_input_volume(
                    1,
                    input_volume,
                    HIGH_SPEECH_PROBABILITY,
                    SPEECH_LEVEL,
                ),
                input_volume
            );
        }
    }

    #[test]
    fn unmuting_raises_too_low_volume() {
        for min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            };
            let mut helper = TestHelper::new(config);
            helper.call_agc_sequence(
                DEFAULT_INITIAL_INPUT_VOLUME,
                HIGH_SPEECH_PROBABILITY,
                SPEECH_LEVEL,
                1,
            );

            helper.controller.handle_capture_output_used_change(false);
            helper.controller.handle_capture_output_used_change(true);

            let input_volume = 11;
            assert_eq!(
                helper.call_recommend_input_volume(
                    1,
                    input_volume,
                    HIGH_SPEECH_PROBABILITY,
                    SPEECH_LEVEL,
                ),
                min_input_volume
            );
        }
    }

    #[test]
    fn no_clipping_has_no_impact() {
        for min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            };
            let mut helper = TestHelper::new(config);
            helper.call_agc_sequence(
                DEFAULT_INITIAL_INPUT_VOLUME,
                HIGH_SPEECH_PROBABILITY,
                SPEECH_LEVEL,
                1,
            );

            helper.call_analyze_input_audio(100, 0.0);
            assert_eq!(helper.controller.recommended_input_volume(), 128);
        }
    }

    #[test]
    fn clipping_under_threshold_has_no_impact() {
        for min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            };
            let mut helper = TestHelper::new(config);
            helper.call_agc_sequence(
                DEFAULT_INITIAL_INPUT_VOLUME,
                HIGH_SPEECH_PROBABILITY,
                SPEECH_LEVEL,
                1,
            );

            helper.call_analyze_input_audio(1, 0.099);
            assert_eq!(helper.controller.recommended_input_volume(), 128);
        }
    }

    #[test]
    fn clipping_lowers_volume() {
        for min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            };
            let mut helper = TestHelper::new(config);
            helper.call_agc_sequence(255, HIGH_SPEECH_PROBABILITY, SPEECH_LEVEL, 1);

            helper.call_analyze_input_audio(1, 0.2);
            assert_eq!(helper.controller.recommended_input_volume(), 240);
        }
    }

    #[test]
    fn waiting_period_between_clipping_checks() {
        for min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            };
            let mut helper = TestHelper::new(config);
            helper.call_agc_sequence(255, HIGH_SPEECH_PROBABILITY, SPEECH_LEVEL, 1);

            helper.call_analyze_input_audio(1, ABOVE_CLIPPED_THRESHOLD);
            assert_eq!(helper.controller.recommended_input_volume(), 240);

            helper.call_analyze_input_audio(300, ABOVE_CLIPPED_THRESHOLD);
            assert_eq!(helper.controller.recommended_input_volume(), 240);

            helper.call_analyze_input_audio(1, ABOVE_CLIPPED_THRESHOLD);
            assert_eq!(helper.controller.recommended_input_volume(), 225);
        }
    }

    #[test]
    fn clipping_lowering_is_limited() {
        for min_input_volume in [12, 20] {
            let mut config = get_test_config();
            config.min_input_volume = min_input_volume;
            let mut helper = TestHelper::new(config);
            helper.call_agc_sequence(180, HIGH_SPEECH_PROBABILITY, SPEECH_LEVEL, 1);

            helper.call_analyze_input_audio(1, ABOVE_CLIPPED_THRESHOLD);
            assert_eq!(helper.controller.recommended_input_volume(), CLIPPED_MIN);

            helper.call_analyze_input_audio(1000, ABOVE_CLIPPED_THRESHOLD);
            assert_eq!(helper.controller.recommended_input_volume(), CLIPPED_MIN);
        }
    }

    #[test]
    fn clipping_parameters_verified() {
        for _min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                clipped_level_step: CLIPPED_LEVEL_STEP,
                clipped_ratio_threshold: CLIPPED_RATIO_THRESHOLD,
                clipped_wait_frames: CLIPPED_WAIT_FRAMES,
                ..Default::default()
            };
            let mut controller = InputVolumeController::new(1, &config);
            controller.initialize();
            assert_eq!(controller.clipped_level_step, CLIPPED_LEVEL_STEP);
            assert_eq!(controller.clipped_ratio_threshold, CLIPPED_RATIO_THRESHOLD);
            assert_eq!(controller.clipped_wait_frames, CLIPPED_WAIT_FRAMES);

            let config_custom = InputVolumeControllerConfig {
                clipped_level_step: 10,
                clipped_ratio_threshold: 0.2,
                clipped_wait_frames: 50,
                ..Default::default()
            };
            let mut controller_custom = InputVolumeController::new(1, &config_custom);
            controller_custom.initialize();
            assert_eq!(controller_custom.clipped_level_step, 10);
            assert_eq!(controller_custom.clipped_ratio_threshold, 0.2);
            assert_eq!(controller_custom.clipped_wait_frames, 50);
        }
    }

    #[test]
    fn disable_clipping_predictor_disables_clipping_predictor() {
        for _min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                clipped_level_step: CLIPPED_LEVEL_STEP,
                clipped_ratio_threshold: CLIPPED_RATIO_THRESHOLD,
                clipped_wait_frames: CLIPPED_WAIT_FRAMES,
                enable_clipping_predictor: false,
                ..Default::default()
            };
            let mut controller = InputVolumeController::new(1, &config);
            controller.initialize();

            assert!(controller.clipping_predictor.is_none());
            assert!(!controller.use_clipping_predictor_step);
        }
    }

    #[test]
    fn enable_clipping_predictor_enables_clipping_predictor() {
        for _min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                clipped_level_step: CLIPPED_LEVEL_STEP,
                clipped_ratio_threshold: CLIPPED_RATIO_THRESHOLD,
                clipped_wait_frames: CLIPPED_WAIT_FRAMES,
                enable_clipping_predictor: true,
                ..Default::default()
            };
            let mut controller = InputVolumeController::new(1, &config);
            controller.initialize();

            assert!(controller.clipping_predictor.is_some());
            assert!(controller.use_clipping_predictor_step);
        }
    }

    #[test]
    fn takes_no_action_on_zero_mic_volume() {
        for min_input_volume in [12, 20] {
            let config = InputVolumeControllerConfig {
                min_input_volume,
                ..get_test_config()
            };
            let mut helper = TestHelper::new(config);
            helper.call_agc_sequence(
                DEFAULT_INITIAL_INPUT_VOLUME,
                HIGH_SPEECH_PROBABILITY,
                SPEECH_LEVEL,
                1,
            );

            assert_eq!(
                helper.call_recommend_input_volume(10, 0, HIGH_SPEECH_PROBABILITY, -48.0),
                0
            );
        }
    }

    #[test]
    fn clipping_does_not_pull_low_volume_back_up() {
        for min_input_volume in [12, 20] {
            let mut config = get_test_config();
            config.min_input_volume = min_input_volume;
            let mut helper = TestHelper::new(config);
            helper.call_agc_sequence(80, HIGH_SPEECH_PROBABILITY, SPEECH_LEVEL, 1);

            let initial_volume = helper.controller.recommended_input_volume();
            helper.call_analyze_input_audio(1, ABOVE_CLIPPED_THRESHOLD);
            assert_eq!(helper.controller.recommended_input_volume(), initial_volume);
        }
    }

    #[test]
    fn min_input_volume_enforced_with_clipping_when_above_clipped_level_min() {
        let mut helper = TestHelper::new(InputVolumeControllerConfig {
            min_input_volume: 80,
            clipped_level_min: 70,
            ..get_test_config()
        });

        write_audio_buffer_samples(4000.0, 0.8, &mut helper.audio_buffer);
        let num_calls = 800;
        helper.call_agc_sequence(100, LOW_SPEECH_PROBABILITY, -18.0, num_calls);

        assert_eq!(helper.controller.recommended_input_volume(), 80);
    }

    #[test]
    fn clipped_level_min_enforced_with_clipping_when_above_min_input_volume() {
        let mut helper = TestHelper::new(InputVolumeControllerConfig {
            min_input_volume: 70,
            clipped_level_min: 80,
            ..get_test_config()
        });

        write_audio_buffer_samples(4000.0, 0.8, &mut helper.audio_buffer);
        let num_calls = 800;
        helper.call_agc_sequence(100, LOW_SPEECH_PROBABILITY, -18.0, num_calls);

        assert_eq!(helper.controller.recommended_input_volume(), 80);
    }
}
