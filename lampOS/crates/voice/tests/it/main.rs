//! `lamp-voice`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod admission;
mod call_wait;
mod camera_config;
mod camera_runtime;
mod capture_hold_worker;
mod diagnostics;
mod end_silence;
mod endpoint_confirmation;
mod expression_out;
mod first_control_message;
mod first_words;
mod fixture_provider;
mod hello_asks;
mod news_told;
mod privacy_controls;
mod privacy_pipeline;
mod privacy_state;
mod privacy_worker;
mod process;
mod progress_news;
mod provider_config;
mod reference_link;
mod reference_prime;
mod review_2026_10_14;
mod ring_worker;
mod speaker_lapse_worker;
mod typed_memory;
mod visual_options;
#[path = "core.rs"]
mod voice_core;
