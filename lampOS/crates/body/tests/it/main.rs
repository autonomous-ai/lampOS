//! `lamp-body`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod calibration;
mod control;
mod head_qualification;
mod protocol;
mod readback;
mod selection;
/// The scripted runtime `head_qualification` and `worker` drive, loaded once.
#[path = "../runtime/support.rs"]
mod support;
mod worker;
mod worker_config;
