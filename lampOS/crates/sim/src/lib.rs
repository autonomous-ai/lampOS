//! The virtual desktop lamp's hardware edge (docs/simulator-lamp.md).
/// Tests that write a script and run it take this first: a script just
/// written can be refused as busy ("text file busy") when another test's
/// fork holds its write handle for a moment, which failed one of them about
/// one run in three.
#[cfg(test)]
pub(crate) static FAKE_PROGRAM: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The folder a test makes its own under: the system's temp folder, or `/tmp`
/// when that one is too long for the bus's sockets. A Unix socket's address
/// holds 103 bytes, and macOS's temp folder (`/var/folders/…/T/`, about 49
/// bytes) left the ring's and the voice's test sockets at 105 to 119.
#[cfg(test)]
pub(crate) fn test_root() -> std::path::PathBuf {
    let temp = std::env::temp_dir();
    if temp.as_os_str().len() > 24 {
        std::path::PathBuf::from("/tmp")
    } else {
        temp
    }
}

pub mod camera;
pub mod controls;
pub mod keys;
pub mod manage;
pub mod mic;
pub mod people;
pub mod pose;
pub mod pty;
pub mod real_voice;
pub mod ring;
pub mod servo;
pub mod stimuli;
pub mod voice;
pub mod warp;
