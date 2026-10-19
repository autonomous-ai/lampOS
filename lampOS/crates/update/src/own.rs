//! lamp-update switches itself, and puts itself back (#1680,
//! docs/updates.md). Off unless `LAMP_UPDATE_SELF=on`.
//!
//! 1. Once a round's parts run a build whose `lamp-update` is not the one
//!    `update/current` names, the round installs it beside, writes
//!    `self.json` (the build named before and the new one) and moves the
//!    link; `serve` then `exec`s the new binary in its own place.
//! 2. The new binary confirms itself only when it has read the channel's
//!    manifest and checked its signature, a dry round: then `self.json`
//!    goes. Until then no round acts.
//! 3. `lamp-update-back`, at a fixed path that only `install` writes and no
//!    round can, puts the link back when `self.json` is still there and
//!    lamp-update has failed (`OnFailure=`) or [`DEADLINE`] has passed (its
//!    timer). The binary is refused from then on, and lamp-update started
//!    again.
//!
//! `self.json` is read by a lamp-update-back as old as the lamp's install,
//! so it is a contract for good: its four fields keep their meaning, a new
//! version may add fields, and back reads any version and ignores what it
//! does not know.
use crate::{
    judge::UnitState,
    records::{Records, replace_link, sync_folder, unix_ms, write_whole},
    round::{Config, Outcome, World, round},
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

/// The switch, in lamp-update.service's environment: `on`, or off.
pub const SWITCH: &str = "LAMP_UPDATE_SELF";
/// lamp-update-back: written by `install` alone, outside what
/// lamp-update.service may write.
pub const BACK: &str = "/usr/local/lib/lampos/lamp-update-back";
/// lamp-update's own builds, and the link its unit starts it through.
pub const FOLDER: &str = "/opt/lampOS/update/";
pub const CURRENT: &str = "/opt/lampOS/update/current";
pub const BINARY: &str = "lamp-update";
/// How long a new lamp-update has to confirm itself.
pub const DEADLINE: Duration = Duration::from_secs(600);
/// The version of `self.json` this program writes.
pub const VERSION: u32 = 1;
const PENDING: &str = "self.json";
const REFUSED: &str = "self-refused";

/// Whether the switch is on, from its value.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// `self.json`: a switch of lamp-update's own waiting to be confirmed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub version: u32,
    /// The build `update/current` named before: 64 hex.
    pub old: String,
    /// The build it names now.
    pub new: String,
    /// When the link moved, on the wall clock.
    pub at_unix_ms: u64,
}

fn is_build(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The switch waiting to be confirmed, if any.
pub fn pending(records: &Path) -> io::Result<Option<Pending>> {
    match fs::read(records.join(PENDING)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Written by the round, before the link moves.
pub(crate) fn begin(records: &Path, old: &str, new: &str) -> io::Result<()> {
    let pending = Pending {
        version: VERSION,
        old: old.to_owned(),
        new: new.to_owned(),
        at_unix_ms: unix_ms(),
    };
    let bytes = serde_json::to_vec_pretty(&pending).map_err(io::Error::other)?;
    write_whole(&records.join(PENDING), &bytes, 0o600)
}

fn end(records: &Path) -> io::Result<()> {
    match fs::remove_file(records.join(PENDING)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => sync_folder(records),
    }
}

/// The binaries put back, by SHA-256: never switched to again.
pub fn refused(records: &Path) -> Vec<String> {
    fs::read_to_string(records.join(REFUSED))
        .unwrap_or_default()
        .lines()
        .filter(|line| is_build(line))
        .map(str::to_owned)
        .collect()
}

fn link_names(root: &Path, build: &str) -> bool {
    fs::read_link(on_root(root, CURRENT)).is_ok_and(|now| now == Path::new(build))
}

fn on_root(root: &Path, path: &str) -> PathBuf {
    root.join(path.trim_start_matches('/'))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// No switch waits.
    Nothing,
    /// This program is not the build the switch waits on.
    NotOwn,
    /// The dry round did not check a manifest: why. Tried again later.
    NotProven(String),
    /// Another lamp-update holds the records' lock.
    Busy,
    Confirmed,
}

/// Phase 2: the new lamp-update, running from build `running`, confirms
/// itself once a dry round (nothing applied) has fetched the channel's
/// manifest and checked its signature.
pub fn confirm(
    config: &Config,
    world: &mut World,
    running: Option<&str>,
) -> Result<Confirm, String> {
    let folder = config.on(&config.records);
    let failed = |error: io::Error| format!("{PENDING}: {error}");
    let Some(pending) = pending(&folder).map_err(failed)? else {
        return Ok(Confirm::Nothing);
    };
    if running != Some(pending.new.as_str()) || !link_names(&config.root, &pending.new) {
        return Ok(Confirm::NotOwn);
    }
    let dry = Config {
        apply: false,
        ..config.clone()
    };
    match round(&dry, world) {
        Outcome::UpToDate(_) | Outcome::Would(_) => {}
        other => return Ok(Confirm::NotProven(other.to_string())),
    }
    let records = Records::new(folder.clone());
    let _lock = match records.lock() {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(Confirm::Busy),
        Err(error) => return Err(error.to_string()),
    };
    records
        .state(world.say)
        .map_err(|error| format!("the state cannot be read: {error}"))?;
    if pending_now(&folder)?.is_none_or(|now| now.new != pending.new) {
        return Ok(Confirm::NotOwn);
    }
    end(&folder).map_err(failed)?;
    records.log(
        "self-confirmed",
        &format!("lamp-update runs its build {}", &pending.new[..12]),
    );
    Ok(Confirm::Confirmed)
}

fn pending_now(folder: &Path) -> Result<Option<Pending>, String> {
    pending(folder).map_err(|error| format!("{PENDING}: {error}"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Back {
    /// No switch waits.
    Nothing,
    /// Another lamp-update holds the records' lock: the timer comes again.
    Busy,
    /// lamp-update runs and its deadline has not passed.
    Waiting,
    /// The link names the old build again, and the new one is refused.
    PutBack(Pending),
}

/// lamp-update-back: the link back, the new binary refused; the caller
/// starts lamp-update again. Nothing more, since the copy a lamp has is as
/// old as its install. `now_unix_ms` is the wall clock, `lamp_update`
/// what systemd says of lamp-update.service.
pub fn back(
    records: &Path,
    root: &Path,
    now_unix_ms: u64,
    lamp_update: &UnitState,
    say: &mut dyn FnMut(&str),
) -> Result<Back, String> {
    if pending_now(records)?.is_none() {
        return Ok(Back::Nothing);
    }
    let held = Records::new(records.to_owned());
    let _lock = match held.lock() {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(Back::Busy),
        Err(error) => return Err(error.to_string()),
    };
    let Some(pending) = pending_now(records)? else {
        return Ok(Back::Nothing);
    };
    let running = lamp_update.active == "active" && lamp_update.sub == "running";
    let late = now_unix_ms
        >= pending
            .at_unix_ms
            .saturating_add(DEADLINE.as_millis() as u64);
    if running && !late {
        return Ok(Back::Waiting);
    }
    if !is_build(&pending.old) || !is_build(&pending.new) {
        return Err(format!("{PENDING} does not name two builds"));
    }
    if link_names(root, &pending.new) {
        replace_link(&on_root(root, CURRENT), &pending.old)
            .map_err(|error| format!("{CURRENT}: {error}"))?;
    }
    let mut refused = refused(records);
    if !refused.contains(&pending.new) {
        refused.push(pending.new.clone());
    }
    write_whole(
        &records.join(REFUSED),
        format!("{}\n", refused.join("\n")).as_bytes(),
        0o600,
    )
    .map_err(|error| format!("{REFUSED}: {error}"))?;
    end(records).map_err(|error| format!("{PENDING}: {error}"))?;
    say(&format!(
        "lamp-update's build {} did not confirm itself ({}): {CURRENT} names {} again",
        &pending.new[..12],
        if running { "too late" } else { "not running" },
        &pending.old[..12]
    ));
    Ok(Back::PutBack(pending))
}
