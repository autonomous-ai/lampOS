//! Bounded local evidence for the fleet experience evaluations (#8793).
//!
//! The pure recorder counts received observations, not every intended call or
//! correct answer. Internal turn identities stay in its local checkpoint;
//! aggregate snapshots contain only fixed categories, counts and build metadata.
//! Storage is an explicit separate operation, never performed by `observe`.
//!
//! This library does not install a process, decode the proposed version-2 voice
//! record, enable a producer, schedule a check or send a report. See
//! `docs/evals.md` for the integration gates and measurement boundaries.
#![deny(clippy::print_stderr, clippy::print_stdout, clippy::dbg_macro)]

pub mod core;
pub mod model;
pub mod persistence;
pub mod store;

pub use core::{Error, Ingest, Recorder};
pub use model::{Build, Contract, Observation, Origin, Snapshot, SourceTime, Terminal};
