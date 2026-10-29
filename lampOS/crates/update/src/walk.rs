//! A round holds before it fetches while the setup walk is under way
//! (#3689): a lamp updates within the hour of its setup, and its person is
//! then being walked through onboarding by its first voice. The quiet gate
//! keeps a switch off a sentence, not off a walk waiting for someone to sit.
//!
//! Under way: setup is done and the voice's onboarding file has no outcome
//! for one of [`lamp_msgs::ONBOARDING_STEPS`]. No file, or one that can't be
//! read, is no walk: the 7071 image writes it at the first step's end, and
//! a lamp with onboarding off never does. Held at most [`MOST_HELD`] rounds
//! on a lamp, counted in the records and never reset, so a walk left half
//! done never holds updates for good, and the bound needs no clock (a new
//! lamp's clock may be set after setup ends).
use crate::{records, round::Config, setup_lifecycle};
use std::{fs, path::Path};

/// The voice's onboarding file (`LAMP_ONBOARDING_FILE` in the product unit).
pub const PROGRESS: &str = "/var/lib/lampOS/voice/onboarding/progress.json";
/// Rounds a lamp is held for a walk, in all: 2 hours at `serve`'s 900 s.
pub const MOST_HELD: u32 = 8;
/// In the records folder: the rounds held so far.
const HELD: &str = "walk-held";

/// Whether the setup walk is under way on the lamp under `config.root`.
pub fn under_way(config: &Config) -> bool {
    if !setup_lifecycle::done(&config.root) {
        return false;
    }
    let Ok(bytes) = fs::read(config.on(PROGRESS)) else {
        return false;
    };
    let Ok(file) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    let Some(steps) = file.get("steps").and_then(serde_json::Value::as_object) else {
        return false;
    };
    lamp_msgs::ONBOARDING_STEPS
        .iter()
        .any(|step| !steps.contains_key(*step))
}

/// One more round held, counted in `folder`: its number, or none once
/// [`MOST_HELD`] are spent or the count can't be kept, so the round goes on.
pub fn hold(folder: &Path) -> Option<u32> {
    let path = folder.join(HELD);
    let held = match fs::read_to_string(&path) {
        Ok(text) => text.trim().parse::<u32>().ok()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => return None,
    };
    if held >= MOST_HELD {
        return None;
    }
    let now = held + 1;
    records::write_whole(&path, format!("{now}\n").as_bytes(), 0o644).ok()?;
    Some(now)
}
