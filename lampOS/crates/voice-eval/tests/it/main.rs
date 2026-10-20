//! `lamp-voice-eval`'s integration tests, one module a file, built as one binary
//! so the crate is linked once for them, not once a file (#1806).
mod acceptance_boundaries;
mod answer_review;
mod audit_final_dispatch;
mod audit_importer;
mod audit_runner_dispatch;
mod canary_plan;
/// Shared helpers, loaded once for every module that uses them.
mod common;
mod cost;
mod endpoint;
mod evaluator;
mod fake_runtime;
mod final_dispatch;
mod next_turn_ready;
mod one_breath;
mod physical_loopback;
mod pinned_copies;
mod playback_occurrences;
mod provider_failed;
mod recording_evidence;
mod recording_import;
mod relay_clock;
mod relay_hardening;
mod report_validity;
mod serve_plan;
mod serve_relay;
mod serve_session;
mod switches;
mod target_audio;
mod waiting_cuts;
