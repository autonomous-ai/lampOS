//! Submodule state tracking for the audio processing pipeline.
//!
//! Ported from `AudioProcessingImpl::SubmoduleStates` in
//! `modules/audio_processing/audio_processing_impl.h/cc`.
//!
//! Tracks which submodules are active and detects state changes that
//! require reinitialization.

/// Tracks the enabled/disabled state of each audio processing submodule.
#[derive(Debug)]
pub(crate) struct SubmoduleStates {
    high_pass_filter_enabled: bool,
    noise_suppressor_enabled: bool,
    gain_controller2_enabled: bool,
    gain_adjustment_enabled: bool,
    echo_controller_enabled: bool,
    first_update: bool,
}

impl SubmoduleStates {
    pub(crate) fn new() -> Self {
        Self {
            high_pass_filter_enabled: false,
            noise_suppressor_enabled: false,
            gain_controller2_enabled: false,
            gain_adjustment_enabled: false,
            echo_controller_enabled: false,
            first_update: true,
        }
    }

    /// Updates the submodule state and returns `true` if it has changed.
    pub(crate) fn update(
        &mut self,
        high_pass_filter_enabled: bool,
        noise_suppressor_enabled: bool,
        gain_controller2_enabled: bool,
        gain_adjustment_enabled: bool,
        echo_controller_enabled: bool,
    ) -> bool {
        let mut changed = false;
        changed |= high_pass_filter_enabled != self.high_pass_filter_enabled;
        changed |= noise_suppressor_enabled != self.noise_suppressor_enabled;
        changed |= gain_controller2_enabled != self.gain_controller2_enabled;
        changed |= gain_adjustment_enabled != self.gain_adjustment_enabled;
        changed |= echo_controller_enabled != self.echo_controller_enabled;

        if changed {
            self.high_pass_filter_enabled = high_pass_filter_enabled;
            self.noise_suppressor_enabled = noise_suppressor_enabled;
            self.gain_controller2_enabled = gain_controller2_enabled;
            self.gain_adjustment_enabled = gain_adjustment_enabled;
            self.echo_controller_enabled = echo_controller_enabled;
        }

        changed |= self.first_update;
        self.first_update = false;
        changed
    }

    /// Returns whether any capture multi-band processing submodules are active.
    pub(crate) fn capture_multi_band_sub_modules_active(&self) -> bool {
        self.capture_multi_band_processing_present()
    }

    /// Returns whether capture multi-band processing is present.
    pub(crate) fn capture_multi_band_processing_present(&self) -> bool {
        self.capture_multi_band_processing_active(true)
    }

    /// Returns whether capture multi-band processing is active.
    pub(crate) fn capture_multi_band_processing_active(&self, ec_processing_active: bool) -> bool {
        self.high_pass_filter_enabled
            || self.noise_suppressor_enabled
            || (self.echo_controller_enabled && ec_processing_active)
    }

    /// Returns whether capture full-band processing is active.
    pub(crate) fn capture_full_band_processing_active(&self) -> bool {
        self.gain_controller2_enabled || self.gain_adjustment_enabled
    }

    /// Returns whether render multi-band submodules are active.
    pub(crate) fn render_multi_band_sub_modules_active(&self) -> bool {
        self.echo_controller_enabled
    }

    /// Returns whether render output was modified by processing.
    /// Always false: the Rust port does not include render pre-processors.
    pub(crate) fn render_output_modified(&self) -> bool {
        false
    }

    /// Returns whether high-pass filtering is required.
    pub(crate) fn high_pass_filtering_required(&self) -> bool {
        self.high_pass_filter_enabled || self.noise_suppressor_enabled
    }

    /// Returns whether the echo controller is enabled.
    pub(crate) fn echo_controller_enabled(&self) -> bool {
        self.echo_controller_enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_update_always_returns_changed() {
        let mut states = SubmoduleStates::new();
        assert!(states.update(false, false, false, false, false));
    }

    #[test]
    fn second_update_no_change() {
        let mut states = SubmoduleStates::new();
        states.update(false, false, false, false, false);
        assert!(!states.update(false, false, false, false, false));
    }

    #[test]
    fn enabling_module_returns_changed() {
        let mut states = SubmoduleStates::new();
        states.update(false, false, false, false, false);
        assert!(states.update(true, false, false, false, false));
    }

    #[test]
    fn capture_multi_band_with_hpf() {
        let mut states = SubmoduleStates::new();
        states.update(true, false, false, false, false);
        assert!(states.capture_multi_band_processing_active(false));
    }

    #[test]
    fn capture_multi_band_with_echo_controller_inactive() {
        let mut states = SubmoduleStates::new();
        states.update(false, false, false, false, true);
        assert!(!states.capture_multi_band_processing_active(false));
        assert!(states.capture_multi_band_processing_active(true));
    }

    #[test]
    fn high_pass_filtering_required() {
        let mut states = SubmoduleStates::new();
        states.update(false, true, false, false, false);
        assert!(states.high_pass_filtering_required());
    }

    #[test]
    fn render_multi_band_with_echo_controller() {
        let mut states = SubmoduleStates::new();
        states.update(false, false, false, false, true);
        assert!(states.render_multi_band_sub_modules_active());
    }
}
