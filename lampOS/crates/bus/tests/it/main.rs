//! `lamp-bus`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod bus;
mod cli;
mod clock_step;
mod logs;
