//! A pre-registered, fictional-month memory evaluation. No hardware access.
pub mod arm_server;
pub mod arms;
pub mod conversation;
pub mod corpus;
pub mod files;
pub mod ledger;
pub mod model;
pub mod model_worker;
pub mod partial;
pub mod plumbing;
pub mod protocol;
pub mod runner;
pub mod score;
pub mod session;
pub mod smoke;
pub mod speech;
pub mod transport;

use sha2::{Digest, Sha256};
use std::{fmt, io};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    Invalid(String),
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "benchmark I/O: {e}"),
            Self::Json(e) => write!(f, "benchmark JSON: {e}"),
            Self::Invalid(e) => f.write_str(e),
            Self::Unsupported(e) => write!(f, "unsupported: {e}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
