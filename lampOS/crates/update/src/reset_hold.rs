//! No switch while lamp-setup may be running a reset (#6266, step 5d). A reset
//! removes the setup marker, stops the lamp, wipes and starts setup again, or
//! reboots into it; a switch in that stretch restarts parts under a lamp that is
//! being emptied, and a round woken by the boot can find the lamp in it. So a
//! round holds, `NotQuiet` (it is not refused, and the next round tries again):
//!
//! - **while a reset's journal is there and the setup marker is not** (or the
//!   journal is new, [`JOURNAL_FRESH_S`]): [`JOURNAL`] is written first and removed
//!   only when the wipe has finished, and the marker is removed right after it.
//!   In a real reset the two are together for milliseconds, so a journal beside
//!   the marker that is not new is not a reset: a failed unlink of the marker
//!   (which `reset` reports and leaves the lamp running), or a journal that came
//!   by a restore. `lamp-setup serve` would remove it, but it never runs on a lamp
//!   whose marker is there; holding on it would stop every round for good, with
//!   nothing refused to alert anyone. Such a journal is said ([`stale_beside_marker`])
//!   and the round goes on;
//! - **in the first [`BOOT_GRACE_S`] seconds after the boot** (`/proc/uptime`),
//!   when setup's own recovery of a cut reset may still be running before it has
//!   written anything. An uptime that cannot be read holds nothing: a lamp with
//!   no `/proc/uptime` is never held for good.
//!
//! Looked at twice, as the setup walk is ([`crate::walk`]): before the fetch and
//! again after the quiet wait, before anything is written.
use crate::round::Config;
use std::{
    fs,
    time::{Duration, SystemTime},
};

/// lamp-setup's reset journal: there from the reset's start until its wipe is done
/// (`lamp_setup::reset::PRODUCT_JOURNAL`).
pub const JOURNAL: &str = "/var/lib/lampos-setup/reset.json";
/// A journal this young is a reset begun now, whatever the marker says.
pub const JOURNAL_FRESH_S: u64 = 60;
/// The kernel's uptime, whose first number is the seconds since the boot.
pub const UPTIME: &str = "/proc/uptime";
/// No switch this long after the boot.
pub const BOOT_GRACE_S: u64 = 180;

/// Why no switch may start now, or none: `forced` is the first update after
/// setup, which the boot's grace does not hold. The text is the round's end
/// (`NotQuiet`), said in the journal and never to the app's status.
pub fn hold(config: &Config, forced: bool) -> Option<String> {
    if journal_holds(config) {
        return Some(format!(
            "a reset of the lamp may be under way ({JOURNAL} is there): no switch until it has finished"
        ));
    }
    if forced {
        return None;
    }
    let uptime = fs::read_to_string(config.on(UPTIME)).ok()?;
    let seconds: f64 = uptime.split_whitespace().next()?.parse().ok()?;
    (seconds.is_finite() && seconds >= 0.0 && seconds < BOOT_GRACE_S as f64).then(|| {
        format!(
            "the lamp booted {} s ago: no switch in its first {BOOT_GRACE_S} s, while lamp-setup may be running a reset",
            seconds as u64
        )
    })
}

/// The journal's age, when it is there and its time is not in the future: a
/// clock that jumped, or a journal dated ahead, has no age to go by.
fn journal_age(config: &Config) -> Option<Option<Duration>> {
    let modified = fs::symlink_metadata(config.on(JOURNAL))
        .ok()?
        .modified()
        .ok()?;
    Some(SystemTime::now().duration_since(modified).ok())
}

/// A reset's journal holds the round: there with no setup marker (the reset
/// has begun and the lamp is being emptied), or there and new.
fn journal_holds(config: &Config) -> bool {
    match journal_age(config) {
        None => false,
        Some(age) => {
            !crate::setup_lifecycle::done(&config.root)
                || age.is_some_and(|age| age < Duration::from_secs(JOURNAL_FRESH_S))
        }
    }
}

/// A journal beside the setup marker that is not new: left by a failed unlink of
/// the marker or by a restore, not a reset. The round says so and goes on.
pub fn stale_beside_marker(config: &Config) -> bool {
    journal_age(config).is_some() && !journal_holds(config)
}
