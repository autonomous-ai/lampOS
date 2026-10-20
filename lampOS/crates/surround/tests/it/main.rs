//! `lamp-surround`'s integration tests, one module a file, built as one binary so
//! the crate is linked once for them, not once a file (#1806).
mod atomic;
mod cli;
mod config;
mod configuration;
mod http_limit;
mod network;
mod operator;
mod owner;
mod release;
mod reset;
mod runtime;
mod selection;
mod setup;
mod startup;
/// The release fixtures, shared by `release`, `selection` and `worker`.
#[path = "../../fixtures/releases/support.rs"]
pub mod support;
mod tls_provider;
mod update;
mod worker;
