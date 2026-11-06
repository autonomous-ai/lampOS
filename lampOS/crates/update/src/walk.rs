//! A round holds before it fetches while the setup walk is under way
//! (#3689): a lamp updates within the hour of its setup, and its person is
//! then being walked through onboarding by its first voice. The quiet gate
//! keeps a switch off a sentence, not off a walk waiting for someone to sit.
//!
//! Looked at twice: before the fetch, and at each look of the switch's quiet
//! wait, so a walk that began while the round fetched or waited, in its
//! first step before its file, still holds the round before anything is
//! switched (Tests' P2 on #4455).
//!
//! Under way: setup is done and the voice's onboarding file has no outcome
//! for one of [`lamp_msgs::ONBOARDING_STEPS`], or has not been written yet
//! while the installed voice unit walks (`LAMP_ONBOARDING=on`): the 7071
//! image writes it only at the first step's end, so the walk's hello and
//! its wait for the person to sit have none (Tests' and Third's P2 on
//! #4455). The file is the one that unit names (`LAMP_ONBOARDING_FILE`).
//! One that can't be read, or isn't the walk's, is no walk, and so is no
//! file on a lamp whose voice doesn't walk. Held at most [`MOST_HELD`]
//! rounds on a lamp, counted in the records and never reset, so a walk
//! left half done never holds updates for good, and the bound needs no
//! clock (a new lamp's clock may be set after setup ends).
use crate::{
    records,
    round::{Config, VOICE_UNIT},
    setup_lifecycle, unit,
};
use std::{fs, path::Path};

/// The voice's onboarding file when its unit names none: the product
/// unit's `LAMP_ONBOARDING_FILE`, the same since the 7071 image.
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
    let voice = fs::read_to_string(config.on(&config.units).join(VOICE_UNIT)).unwrap_or_default();
    let walks = unit::environment(&voice, "LAMP_ONBOARDING").as_deref() == Some("on");
    let progress =
        unit::environment(&voice, "LAMP_ONBOARDING_FILE").unwrap_or_else(|| PROGRESS.to_owned());
    let bytes = match fs::read(config.on(&progress)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return walks,
        Err(_) => return false,
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

/// What the switch's quiet wait gives up with when a walk begins while it
/// waits (Tests' P2 on #4455): the round fetched, then waited, while the
/// walk's first step had no file yet.
pub const UNDER_WAY: &str = "the setup walk is under way";

/// The rounds held so far in `folder`, or none when the count can't be read.
fn count(folder: &Path) -> Option<u32> {
    match fs::read_to_string(folder.join(HELD)) {
        Ok(text) => text.trim().parse::<u32>().ok(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(0),
        Err(_) => None,
    }
}

/// Whether no round can be held any more: [`MOST_HELD`] spent, or a count
/// that can't be read. Then nothing need look at the walk.
pub fn spent(folder: &Path) -> bool {
    count(folder).is_none_or(|held| held >= MOST_HELD)
}

/// One more round held, counted in `folder`: its number, or none once
/// [`MOST_HELD`] are spent or the count can't be kept, so the round goes on.
pub fn hold(folder: &Path) -> Option<u32> {
    let held = count(folder)?;
    if held >= MOST_HELD {
        return None;
    }
    let now = held + 1;
    records::write_whole(&folder.join(HELD), format!("{now}\n").as_bytes(), 0o644).ok()?;
    Some(now)
}

/// The round's end for held round `round`.
pub fn held(round: u32) -> String {
    format!("{UNDER_WAY}: held, round {round} of {MOST_HELD}")
}
