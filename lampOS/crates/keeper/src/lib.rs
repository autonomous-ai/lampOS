//! The keeper (docs/work-split.md, task 27).
//!
//! [`Notify`] is what any part calls to tell systemd it is ready, alive
//! and stopping. [`Keeper`] is what `lamp-keeper` decides from the lamp's
//! health, [`units`] how it asks and commands systemd, and [`package`] the
//! keeper as the one service the old runtime's updater installs.
mod keep;
mod notify;
pub mod package;
pub mod profile;
pub mod units;

pub use keep::{Done, Keeper, NAME, PartState, Rules, STATE, VOICE, WINDOW};
pub use notify::Notify;
