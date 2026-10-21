//! `lamp-actions`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod caps;
mod schedule;
mod server;
