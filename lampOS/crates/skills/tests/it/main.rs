//! `lamp-skills`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod call_wait;
mod daily_engine;
mod made;
mod package;
mod routine_service;
mod skills;
mod store;
