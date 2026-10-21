//! What the lamp remembers across days: a journal of each exchange it took
//! part in and what it was asked to keep, for each person apart, as text on
//! the lamp's own disk. See `docs/memory.md`.
//!
//! [`store`] is the files, [`protocol`] is what is asked of them and
//! answered, one JSON line each way, and [`server`] is the socket the lines
//! travel on.
pub mod days;
pub mod days_protocol;
pub mod forgot;
pub mod health;
pub mod health_counts;
pub mod protocol;
pub mod search;
pub mod server;
pub mod store;
pub mod tidy;

pub use protocol::{Request, Response, handle};
pub use server::Server;
pub use store::{Options, Person, Store};
