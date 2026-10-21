//! `lamp-cloud`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod app_contract;
mod report_contract;
mod ring;
mod setup;
mod skills_contract;
mod switch;
mod texts;
