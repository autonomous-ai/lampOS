//! `lamp-schedule`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
//! Two files stay binaries of their own:
//! - `tests/storage_children.rs` runs its binary again by a test's name, and
//!   its children share no process with other tests;
//! - `tests/forward.rs`: in one process its tests reopen a store another of
//!   them may still hold, while one starts `lamp-schedule` (#1569 row 53).
mod calendar;
mod client;
mod delivery_ledger;
mod engine;
mod protocol;
mod runtime;
mod runtime_failures;
mod storage;
