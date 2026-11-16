//! The burst period the cloud may set (#7401, `seen_clip`; the design
//! agreed on #7401 with the first builder's yes): lamp-cloud keeps the
//! `/ping` reply's `seen_clip` as [`FILE`], `{"every_s": N, "until_unix_s":
//! T}` in whole seconds, the way it keeps `region`, and removes it when the
//! reply leaves it out. This part reads it again every
//! [`crate::opt_in::READ_EVERY`] and never writes it.
//!
//! - **[`crate::activity::EVERY_US`] (a minute)** whenever it hasn't heard:
//!   no file, one that can't be read, over
//!   [`lamp_msgs::consent::MOST_BYTES`], not JSON of whole seconds, a value
//!   outside the range, or one at or past `until_unix_s`.
//! - **Never faster than the switch allows**: the range is from
//!   [`crate::activity::fastest_us`] (15 s with `LAMP_SEEN_BURST_15S=on`, a
//!   minute off) to a minute, so with the switch off it is a minute whatever
//!   the cloud says.
//! - **The heat pace multiplies it** ([`crate::heat::Pace::at_every`]).
use crate::activity::EVERY_US;
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;

/// Where lamp-cloud keeps the ping reply's `seen_clip`.
pub const FILE: &str = "/var/lib/lampos-cloud/seen-clip.json";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeenClip {
    every_s: u64,
    until_unix_s: u64,
}

/// The burst period `text` (the file's bytes, `None` when there is none or
/// it can't be read) gives at `now_unix_s`, never faster than `fastest_us`:
/// [`EVERY_US`] unless the cloud's value is valid, in force and in range.
pub fn every_us(text: Option<&[u8]>, now_unix_s: u64, fastest_us: u64) -> u64 {
    let Some(clip) = text
        .and_then(|text| serde_json::from_slice::<serde_json::Value>(text).ok())
        .filter(serde_json::Value::is_object)
        .and_then(|value| serde_json::from_value::<SeenClip>(value).ok())
    else {
        return EVERY_US;
    };
    let every_us = clip.every_s.saturating_mul(1_000_000);
    if now_unix_s >= clip.until_unix_s || every_us < fastest_us || every_us > EVERY_US {
        return EVERY_US;
    }
    every_us
}

/// The kept `seen_clip`, read again every [`crate::opt_in::READ_EVERY`].
pub struct ClipIn {
    path: PathBuf,
    fastest_us: u64,
    read: Option<Instant>,
    every_us: u64,
}

impl ClipIn {
    pub fn at(path: PathBuf, fastest_us: u64) -> Self {
        Self {
            path,
            fastest_us,
            read: None,
            every_us: EVERY_US,
        }
    }

    /// The burst period now, at `now_unix_s`. Never waits longer than a
    /// small file's read.
    pub fn every_us(&mut self, now_unix_s: u64) -> u64 {
        let now = Instant::now();
        if self
            .read
            .is_none_or(|at| now - at >= crate::opt_in::READ_EVERY)
        {
            self.read = Some(now);
            let text = read_most(&self.path);
            self.every_us = every_us(text.as_deref(), now_unix_s, self.fastest_us);
        }
        self.every_us
    }
}

fn read_most(path: &std::path::Path) -> Option<Vec<u8>> {
    let most = lamp_msgs::consent::MOST_BYTES;
    let mut text = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(most + 1)
        .read_to_end(&mut text)
        .ok()?;
    (text.len() as u64 <= most).then_some(text)
}

#[cfg(test)]
mod tests;
