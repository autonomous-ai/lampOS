//! `lamp-memory`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
//! Five files stay binaries of their own:
//! - `tests/health_control.rs` runs its binary again by a test's name, and its
//!   children share no process with other tests;
//! - `tests/voice_early_words.rs` sets `early_words::switch`, which the first
//!   setting in a process fixes for the rest of it;
//! - `tests/voice_forget.rs`, `tests/voice_forget_about.rs` and
//!   `tests/voice_observe.rs` set Voice's once-a-process memory hooks: the
//!   forget switches (`switch_forget_all`, `switch_forget_about`) and the
//!   observation pass's model (`observe_with`), which then hold for every
//!   other test in the process (#1569 row 58).
/// The health control fixture (which loads the child protocol from `src` as
/// its `protocol`) and the child schedule, each loaded once for every module
/// that uses them; `tests/health_control.rs` includes the same fixture.
#[path = "../support/health_control.rs"]
pub mod health_control;
pub use health_control::protocol as fixture_protocol;
mod binary;
mod days;
mod days_recall;
mod days_timing;
mod fixture_child;
#[path = "../../src/fixture_schedule.rs"]
pub mod fixture_schedule;
mod health;
mod health_timing;
mod kept;
mod retrieval;
mod server;
mod timing;
mod voice;
mod voice_name;
mod voice_people;
mod voice_submit;
mod voice_timing;
