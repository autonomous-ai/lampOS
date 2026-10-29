//! The part that hears the lamp's name. See `docs/name.md`.
//!
//! The service that writes down what the lamp hears almost never writes
//! "Lamp": it writes "Milan", "Hey man", "Amen", or leaves the name out.
//! So the lamp cannot tell from that text that it was called. This part
//! listens to the sound itself, for the lamp's name and nothing else.
//!
//! - [`hear`]: three small models run every 80 ms over what the
//!   microphone hears, and give a score for "the name was just said".
//! - [`quiet`]: with `--quiet-below`, the models rest while the last
//!   2.03 s held no sound, and give the same scores when it comes.
//! - [`said`]: when a run of scores is one hearing, and what is said of it.
//! - [`not_self`]: with `--not-while-speaking on`, a hearing that began
//!   while the lamp itself spoke is not said.
//! - [`rescue`]: with `--rescue-model`, a second name model rescues a call
//!   the installed one half heard.
//!
//! It keeps no sound and writes none down: samples are held for about a
//! fifth of a second, until they are the numbers the models work on, and
//! of those the last two seconds' worth are kept. With `--quiet-below` the
//! newest 2.11 s of samples are also held in memory, to start the models
//! on when a sound comes.

// Nothing in this library prints: the program does.
#![deny(clippy::print_stderr, clippy::print_stdout, clippy::dbg_macro)]

pub mod hear;
pub mod moving;
pub mod not_self;
pub mod quiet;
pub mod rescue;
pub mod said;
