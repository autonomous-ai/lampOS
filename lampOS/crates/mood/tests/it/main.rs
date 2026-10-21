//! `lamp-mood`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod engine;
mod input;
mod program;
mod replay;
mod runtime;
mod timing;
