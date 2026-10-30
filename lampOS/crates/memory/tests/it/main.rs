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
mod days_speakers;
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
mod voice_note_ahead;
mod voice_people;
mod voice_submit;
mod voice_timing;

/// Real-store voice fixtures (`voice`, `voice_people`, `voice_name`,
/// `voice_note_ahead`) one at a time: each write is several full flushes
/// (F_FULLFSYNC) on a Mac, and beside one another a follow-up forget ran out
/// the voice's own 1.5 s cap (#1569).
static VOICE_STORE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// This fixture's turn; a test that panicked while holding it passes it on.
pub fn voice_store_turn() -> std::sync::MutexGuard<'static, ()> {
    VOICE_STORE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
