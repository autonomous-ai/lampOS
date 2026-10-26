//! The add-ons' judge (#2530, `docs/addons.md`): each proposal a partner's
//! add-on makes on `addon/proposal` is kept or dropped here, by rules, before
//! anything the lamp shows changes. A proposal is never an order.
//!
//! This crate holds the rules alone: no bus, no clock, no file. What the lamp
//! is doing is told to it as [`Situation`], read from the bus by the part that
//! runs it, with each reading's age; the time is given with every question.
//! So every rule is tested here as a plain function, and a replay is the same
//! code as the lamp.
pub mod carry;
pub mod machine;
pub mod rules;
pub mod runtime;
pub mod situation;
pub mod stream;

pub use rules::{Drop, Judge, Reading, Situation, Verdict};
pub use situation::Watcher;
