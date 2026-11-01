//! An experimental, isolated owner memory. No voice, bus, device or background task.
//!
//! Prepare a bounded sleep input, release the store while the model runs, then
//! apply its proposal atomically. Forget and corrections invalidate old inputs.
//!
//! The library has no probe-copy or snapshot-restore API, including in bench
//! builds. Only the development executable can create disposable probe stores.
//!
//! ```compile_fail,E0599
//! use lamp_memory_tiers::Store;
//! let store = Store::in_memory("fictional-owner").unwrap();
//! store.bench_fork().unwrap();
//! ```
//!
//! ```compile_fail,E0599
//! use lamp_memory_tiers::Store;
//! let mut store = Store::in_memory("fictional-owner").unwrap();
//! store.restore_snapshot("old.sqlite").unwrap();
//! ```
//!
//! ```compile_fail,E0624
//! use lamp_memory_tiers::Store;
//! let connection = rusqlite::Connection::open_in_memory().unwrap();
//! Store::from_connection(connection, "fictional-owner", [0; 32]).unwrap();
//! ```
pub mod model;
mod policy;
mod recall;
mod schema;
mod store;
mod types;

pub use policy::Arm;
pub use recall::*;
pub use store::{Store, TOPIC_PROMPT, TopicInput, TopicProposal, TopicRange, TopicReport};
pub use types::*;

#[derive(Debug)]
pub enum Error {
    Database(rusqlite::Error),
    Io(std::io::Error),
    Invalid(&'static str),
    Stale,
    Budget,
    Timeout,
    Model(&'static str),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "memory database: {e}"),
            Self::Io(e) => write!(f, "memory file: {e}"),
            Self::Invalid(e) => write!(f, "memory validation: {e}"),
            Self::Stale => f.write_str("memory input invalidated; prepare it again"),
            Self::Budget => f.write_str("memory model budget exhausted"),
            Self::Timeout => f.write_str("memory model deadline elapsed"),
            Self::Model(e) => write!(f, "memory model: {e}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
