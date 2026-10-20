//! `lamp-update`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
//! Three files stay binaries of their own:
//! - `tests/functions.rs` runs its binary again by a test's name;
//! - `tests/install.rs` and `tests/round.rs` take lamp-update's lock, an
//!   `flock`, and a process another test in the same binary starts holds a
//!   copy of it until its exec (`round.rs`'s note on `FORKS`).
mod ci_paths;
mod image_journal;
mod image_ramlog;
