//! The virtual desktop lamp's hardware edge (docs/simulator-lamp.md).
/// Tests that write a script and run it take this first: a script just
/// written can be refused as busy ("text file busy") when another test's
/// fork holds its write handle for a moment, which failed one of them about
/// one run in three.
#[cfg(test)]
pub(crate) static FAKE_PROGRAM: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub mod camera;
pub mod manage;
pub mod mic;
pub mod people;
pub mod pose;
pub mod pty;
pub mod ring;
pub mod servo;
pub mod stimuli;
pub mod voice;
pub mod warp;
