//! `lamp-expression`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
//! `tests/runtime.rs` stays a binary of its own: it runs that binary again
//! by a test's name, and its children share no process with other tests.
mod alive_engine;
mod alive_replay;
mod catalogue_playout;
mod catalogue_rest;
mod engine;
mod forward;
mod local_time;
mod program;
mod recent_playout;
mod recent_playout_replay;
mod recent_playout_runtime;
mod remark_engine;
mod replay;
mod rest_engine;
mod rest_replay;
mod rest_requests;
mod speaking_engine;
mod sudden_engine;
