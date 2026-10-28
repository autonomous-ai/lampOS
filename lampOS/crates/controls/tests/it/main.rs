//! `lamp-controls`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
//! `tests/strip_worker.rs` stays a binary of its own: it runs that binary again
//! by a test's name, and its children share no process with other tests.
mod base;
mod engine;
mod head_discovery;
mod head_pet;
mod head_rest;
mod head_sleep;
mod hold_clear;
mod kept_stores;
mod mpr121;
mod mute;
mod no_slide;
mod privacy;
mod privacy_from;
mod privacy_program;
mod program;
mod receiver;
mod replay;
mod runtime;
mod strip;
mod touch;
mod touch_calibrate;
mod touch_from;
mod touch_pins;
mod touch_program;
mod touch_receiver;
mod touch_replay;
mod touch_runtime;
