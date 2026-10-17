//! AEC3 config selector — chooses between mono and multichannel configurations.
//!
//! Ported from `modules/audio_processing/aec3/config_selector.h/cc`.

use sonora_aec3::config::EchoCanceller3Config;

/// Validates that the mono and the multichannel configs have compatible fields.
fn compatible_configs(
    mono_config: &EchoCanceller3Config,
    multichannel_config: &EchoCanceller3Config,
) -> bool {
    if mono_config.delay.fixed_capture_delay_samples
        != multichannel_config.delay.fixed_capture_delay_samples
    {
        return false;
    }
    if mono_config.filter.export_linear_aec_output
        != multichannel_config.filter.export_linear_aec_output
    {
        return false;
    }
    if mono_config.filter.high_pass_filter_echo_reference
        != multichannel_config.filter.high_pass_filter_echo_reference
    {
        return false;
    }
    if mono_config.multi_channel.detect_stereo_content
        != multichannel_config.multi_channel.detect_stereo_content
    {
        return false;
    }
    if mono_config
        .multi_channel
        .stereo_detection_timeout_threshold_seconds
        != multichannel_config
            .multi_channel
            .stereo_detection_timeout_threshold_seconds
    {
        return false;
    }
    true
}

/// Selects between mono and multichannel AEC3 configs.
#[derive(Debug)]
pub(crate) struct ConfigSelector {
    config: EchoCanceller3Config,
    multichannel_config: Option<EchoCanceller3Config>,
    use_multichannel: bool,
}

impl ConfigSelector {
    pub(crate) fn new(
        config: EchoCanceller3Config,
        multichannel_config: Option<EchoCanceller3Config>,
        num_render_input_channels: usize,
    ) -> Self {
        if let Some(ref mc) = multichannel_config {
            debug_assert!(compatible_configs(&config, mc));
        }

        let initial_multichannel =
            !config.multi_channel.detect_stereo_content && num_render_input_channels > 1;

        let use_multichannel = initial_multichannel && multichannel_config.is_some();

        Self {
            config,
            multichannel_config,
            use_multichannel,
        }
    }

    /// Updates the config selection based on the detection of multichannel
    /// content.
    pub(crate) fn update(&mut self, multichannel_content: bool) {
        self.use_multichannel = multichannel_content && self.multichannel_config.is_some();
    }

    /// Returns the currently active configuration.
    pub(crate) fn active_config(&self) -> &EchoCanceller3Config {
        if self.use_multichannel {
            self.multichannel_config
                .as_ref()
                .expect("multichannel config must exist when use_multichannel is true")
        } else {
            &self.config
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Parametrized tests over (num_channels, detect_stereo_content).
    // C++ uses: Combine(Values(1, 2, 8), Values(false, true))

    #[test]
    fn mono_config_selected_when_no_multichannel_config_present() {
        for &num_channels in &[1, 2, 8] {
            for &detect_stereo_content in &[false, true] {
                let mut config = EchoCanceller3Config::default();
                config.multi_channel.detect_stereo_content = detect_stereo_content;

                config.delay.default_delay += 1;
                let custom_delay = config.delay.default_delay;

                let mut cs = ConfigSelector::new(config, None, num_channels);
                assert_eq!(cs.active_config().delay.default_delay, custom_delay);

                cs.update(false);
                assert_eq!(cs.active_config().delay.default_delay, custom_delay);

                cs.update(true);
                assert_eq!(cs.active_config().delay.default_delay, custom_delay);
            }
        }
    }

    #[test]
    fn correct_initial_config_is_selected() {
        for &num_channels in &[1, 2, 8] {
            for &detect_stereo_content in &[false, true] {
                let mut config = EchoCanceller3Config::default();
                config.multi_channel.detect_stereo_content = detect_stereo_content;
                let mut multichannel_config = config.clone();

                config.delay.default_delay += 1;
                let custom_delay_config = config.delay.default_delay;
                multichannel_config.delay.default_delay += 2;
                let custom_delay_multichannel = multichannel_config.delay.default_delay;

                let cs = ConfigSelector::new(config, Some(multichannel_config), num_channels);

                if num_channels == 1 || detect_stereo_content {
                    assert_eq!(
                        cs.active_config().delay.default_delay,
                        custom_delay_config,
                        "num_channels={num_channels}, detect_stereo={detect_stereo_content}"
                    );
                } else {
                    assert_eq!(
                        cs.active_config().delay.default_delay,
                        custom_delay_multichannel,
                        "num_channels={num_channels}, detect_stereo={detect_stereo_content}"
                    );
                }
            }
        }
    }

    // Parametrized over num_channels only (detect_stereo_content = true).
    // C++: Values(1, 2, 8)

    #[test]
    fn correct_config_update_behavior() {
        for &num_channels in &[1, 2, 8] {
            let mut config = EchoCanceller3Config::default();
            config.multi_channel.detect_stereo_content = true;
            let mut multichannel_config = config.clone();

            config.delay.default_delay += 1;
            let custom_delay_config = config.delay.default_delay;
            multichannel_config.delay.default_delay += 2;
            let custom_delay_multichannel = multichannel_config.delay.default_delay;

            let mut cs = ConfigSelector::new(config, Some(multichannel_config), num_channels);

            cs.update(false);
            assert_eq!(
                cs.active_config().delay.default_delay,
                custom_delay_config,
                "num_channels={num_channels}"
            );

            if num_channels == 1 {
                cs.update(false);
                assert_eq!(
                    cs.active_config().delay.default_delay,
                    custom_delay_config,
                    "num_channels={num_channels}"
                );
            } else {
                cs.update(true);
                assert_eq!(
                    cs.active_config().delay.default_delay,
                    custom_delay_multichannel,
                    "num_channels={num_channels}"
                );
            }
        }
    }
}
