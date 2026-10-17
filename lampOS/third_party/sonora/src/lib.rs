#![doc = include_str!("../README.md")]

pub(crate) mod audio_buffer;
pub(crate) mod audio_converter;
pub(crate) mod audio_processing;
pub(crate) mod audio_processing_impl;
pub(crate) mod audio_samples_scaler;
pub(crate) mod capture_levels_adjuster;
pub(crate) mod config_selector;
pub(crate) mod echo_canceller3;
pub(crate) mod echo_detector;
pub(crate) mod gain_controller2;
pub(crate) mod input_volume_controller;
pub(crate) mod residual_echo_detector;
pub(crate) mod rms_level;
pub(crate) mod splitting_filter;
pub(crate) mod stream_config;
pub(crate) mod submodule_states;
pub(crate) mod swap_queue;

pub mod config;
pub mod high_pass_filter;
pub mod stats;
pub mod three_band_filter_bank;

pub use audio_processing::{AudioProcessing, AudioProcessingBuilder, Error};
pub use config::Config;
pub use stats::AudioProcessingStats;
pub use stream_config::StreamConfig;
