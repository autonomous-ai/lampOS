//! How the lamp is. See `docs/health.md`.
//!
//! ROS gives every robot a `/diagnostics` stream, to which each part says
//! how it is, and one view of all of them put together. The lamp has the
//! same: each part publishes a `Diagnostic` on `diagnostics` once a second,
//! and this crate is the program that puts them together.
//!
//! - [`parts`]: every part's newest report, and how they become one
//!   `Health`. Arithmetic, and nothing else.
//! - [`board`]: the board's own numbers, read from the system's files, and
//!   the board's own report: when a number is too high.
//! - [`heat`]: how hot the board is for the work that can wait, ok, warm
//!   or hot with hysteresis, published as `board/heat` (#7879).
//! - [`reach`]: whether the internet answers while the voice's link is
//!   down, so a relay outage is not taken for no internet (#830).
//! - [`serve`]: the loop that listens on the bus and publishes
//!   `lamp/health`, `board/state` and `board/heat`.
//! - [`show`]: what `lamp-health` prints for a person, and the status it
//!   ends with.

// Nothing in this library prints. What the loop has to say about itself
// leaves it as a `serve::Note` and what a person is shown as text, and the
// program prints both.
#![deny(clippy::print_stderr, clippy::print_stdout, clippy::dbg_macro)]

pub mod board;
pub mod heat;
pub mod parts;
pub mod reach;
pub mod serve;
pub mod show;
