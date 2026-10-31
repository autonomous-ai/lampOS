//! `lamp-msgs`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod activity;
mod arrival;
mod bus;
mod controls;
mod doc;
mod expression;
mod expression_rest;
mod logging;
mod presence;
mod privacy;
mod schedule;
mod skills;
mod tasks;
mod touch;
