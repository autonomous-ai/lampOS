//! The policy: the one part that decides what the lamp does. See
//! `docs/policy.md`.
//!
//! The lamp's design has several decision models, each a judge that
//! answers typed questions with probabilities, and one policy that
//! combines what they say and makes the single choice. This crate is that
//! part's first version. It makes one decision, "is this said to the
//! lamp?", and makes it in shadow mode: the decision is published on the
//! bus and written to a log, and nothing acts on it.
//!
//! - [`decide`]: the one way of asking any decision model, and the first
//!   model behind it. It knows nothing of lamps.
//! - [`attention`]: the decision itself. What a model is shown and asked,
//!   and how its answers become a choice. Arithmetic, and nothing else.
//! - [`gather`]: the pieces of what was heard put together into whole
//!   utterances, and what the lamp itself last said.
//! - [`record`]: the decisions log.
//! - [`outcome`]: what the lamp did after a decision, and how long before
//!   its first sound the decision was known.
//! - [`progress`]: the second decision, "say this progress now?", about
//!   the work the lamp handed over (`lamp-policy --progress`).
//! - [`hello`]: "say hello now?", when someone comes back and sits down
//!   (`lamp-policy --hello`).
//! - [`moment`]: "is this a good moment to start this itself?", for a
//!   skill's ask of news or anything else unprompted (`lamp-policy --gate`).
//! - [`rules`]: what the people at the lamp asked it about speaking first
//!   (#813), obeyed by the progress question (`lamp-policy --rules`).
//! - [`policy`]: the loop that listens on the bus and does the rest.
//! - [`replay`]: the logged decisions asked again of other models
//!   (`lamp-policy-replay`, #871), for comparing them, with no words out.

// Nothing in this library prints. What it has to say about itself leaves
// it as a `policy::Note`, which cannot hold what was heard or said, and
// the program prints those. A line printed from in here could: it is an
// error, so that the check that every change passes refuses it.
#![deny(clippy::print_stderr, clippy::print_stdout, clippy::dbg_macro)]

pub mod attention;
pub mod decide;
pub mod gather;
pub mod hedge;
pub mod hello;
pub mod moment;
pub mod outcome;
pub mod pictures;
pub mod policy;
pub mod progress;
pub mod record;
pub mod replay;
pub mod rules;
