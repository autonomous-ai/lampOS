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
//!
//! A `self.json` that cannot be read at all (#1505, docs/updates.md) is
//! taken as a switch that failed, never as one confirmed. When the two
//! builds are known from elsewhere ([`known_builds`]) the link goes back
//! and the new build is refused, as back does; when they are not, nothing
//! is guessed: the file is set aside with a line in `log.jsonl`, the link
//! left, and the next round runs. Either way the file is renamed, never
//! deleted.
use crate::{
    judge::UnitState,
    records::{Records, replace_link, sync_folder, unix_ms, write_whole},
    round::{Config, Outcome, World, round},
    world::Systemd,
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
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
/// The builds put back once and tried once more (#2810): put back again,
/// one stays refused for good.
const RETRIED: &str = "self-retried";
/// The build a switch began to, and the boot it began on (#2810).
const BEGAN: &str = "self-began";
/// The build `lamp-update-back` last put back, and the boot it did so on.
const PUT_BACK: &str = "self-put-back";
/// The kernel's id of this boot, new at every start.
pub const BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";

/// Whether the switch is on, from its value.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Before a new handover, require install's independent fallback and its
/// running, persistently enabled timer. This only checks: it never installs
/// the fallback or changes systemd's state (#2700).
pub(crate) fn recovery_ready(root: &Path, systemd: &mut dyn Systemd) -> Result<(), String> {
    let metadata = fs::symlink_metadata(on_root(root, BACK))
        .map_err(|error| format!("independent recovery is not ready: {BACK}: {error}"))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(format!(
            "independent recovery is not ready: {BACK} is not a regular executable file"
        ));
    }
    systemd
        .self_recovery_ready()
        .map_err(|why| format!("independent recovery is not ready: {why}"))
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

/// `self.json`, read as a switch waits on it: `Err(None)` when it is there
/// but cannot be read as one (its JSON, or its four fields).
fn waiting(records: &Path) -> Result<Option<Pending>, Option<String>> {
    match pending(records) {
        Ok(found) => Ok(found),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => Err(None),
        Err(error) => Err(Some(format!("{PENDING}: {error}"))),
    }
}

/// When `self.json` was written, in unix ms: the start of its deadline
/// when its own `at_unix_ms` cannot be read.
fn written_unix_ms(records: &Path) -> Option<u64> {
    let at = fs::metadata(records.join(PENDING)).ok()?.modified().ok()?;
    Some(at.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// The builds an unreadable `self.json` named, from elsewhere: only when
/// `update/current` moved after the file was written (so it names the new
/// build) and `update/` holds exactly one other build with a lamp-update
/// in it (the old one). Otherwise None: nothing is guessed.
///
/// When that other build is already refused and the link's is not, a cut
/// came inside [`settle`] after the link went back: the link names the old
/// build, the put-back is done, and only the file is left to set aside.
pub fn known_builds(records: &Path, root: &Path) -> Option<Known> {
    let link = on_root(root, CURRENT);
    let written = fs::metadata(records.join(PENDING)).ok()?.modified().ok()?;
    let moved = fs::symlink_metadata(&link).ok()?.modified().ok()?;
    if moved < written {
        return None;
    }
    let named = fs::read_link(&link).ok()?.to_str()?.to_owned();
    if !is_build(&named) {
        return None;
    }
    let mut others = fs::read_dir(on_root(root, FOLDER))
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            (is_build(&name) && name != named && entry.path().join(BINARY).is_file())
                .then_some(name)
        });
    let other = others.next()?;
    if others.next().is_some() {
        return None;
    }
    let refused = refused(records);
    let is_refused = |build: &str| refused.iter().any(|known| known == build);
    match (is_refused(&other), is_refused(&named)) {
        (false, _) => Some(Known {
            old: other,
            new: named,
            done: false,
        }),
        (true, false) => Some(Known {
            old: named,
            new: other,
            done: true,
        }),
        (true, true) => None,
    }
}

/// Two builds an unreadable `self.json` is known to have named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Known {
    pub old: String,
    pub new: String,
    /// The link names `old` again and `new` is refused already.
    pub done: bool,
}

/// What became of a `self.json` that could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unreadable {
    /// Both builds known: the link names `old` again, `new` is refused.
    PutBack { old: String, new: String },
    /// Not known: the link is left as it is.
    SetAside,
}

/// Settles an unreadable `self.json`, the records' lock held: the switch
/// failed. The file is kept under a new name and a line says what was done.
fn settle(records: &Path, root: &Path, say: &mut dyn FnMut(&str)) -> Result<Unreadable, String> {
    let held = Records::new(records.to_owned());
    let known = known_builds(records, root);
    // Refused before the link moves: a cut between the two leaves the
    // failed build refused, so the next start never takes the old build,
    // which the link names again, for the new one (the reviewer on #2075).
    if let Some(Known {
        old,
        new,
        done: false,
    }) = &known
    {
        refuse(records, root, new)?;
        replace_link(&on_root(root, CURRENT), old)
            .map_err(|error| format!("{CURRENT}: {error}"))?;
    }
    let aside = held
        .set_aside(PENDING)
        .map_err(|error| format!("{PENDING}: {error}"))?;
    let (event, line, settled) = match known {
        Some(Known { old, new, .. }) => (
            "self-put-back",
            format!(
                "{PENDING} could not be read: lamp-update's build {} is taken as not confirmed, {CURRENT} names {} again, and the file is kept as {}",
                &new[..12],
                &old[..12],
                aside.display()
            ),
            Unreadable::PutBack { old, new },
        ),
        None => (
            "self-set-aside",
            format!(
                "{PENDING} could not be read and the builds it names are not known from elsewhere: kept as {}, {CURRENT} left as it is",
                aside.display()
            ),
            Unreadable::SetAside,
        ),
    };
    say(&line);
    held.log(event, &line);
    Ok(settled)
}

fn refuse(records: &Path, root: &Path, build: &str) -> Result<(), String> {
    // The boot it is put back on, first: the retry rule reads it beside
    // the boot its switch began on (Tests' P2 on #4131). Not known, it is
    // not noted, and the build is never tried again.
    if let Some(boot) = boot_id(root) {
        write_whole(
            &records.join(PUT_BACK),
            format!("{build} {boot}\n").as_bytes(),
            0o600,
        )
        .map_err(|error| format!("{PUT_BACK}: {error}"))?;
    }
    let mut refused = refused(records);
    if !refused.iter().any(|known| known == build) {
        refused.push(build.to_owned());
    }
    write_whole(
        &records.join(REFUSED),
        format!("{}\n", refused.join("\n")).as_bytes(),
        0o600,
    )
    .map_err(|error| format!("{REFUSED}: {error}"))
}

fn end(records: &Path) -> io::Result<()> {
    match fs::remove_file(records.join(PENDING)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => sync_folder(records),
    }
}

/// The binaries put back, by SHA-256: never switched to again, unless
/// once more ([`refused_for_good`]).
pub fn refused(records: &Path) -> Vec<String> {
    builds_in(&records.join(REFUSED))
}

/// The binaries put back once and tried once more ([`retry`]).
pub fn retried(records: &Path) -> Vec<String> {
    builds_in(&records.join(RETRIED))
}

fn builds_in(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| is_build(line))
        .map(str::to_owned)
        .collect()
}

/// Whether a build put back is never taken again (#2810). A put-back says
/// only that the switch was not confirmed in time. A power cut or a reboot
/// in its ten minutes reads the same as a build that can't run, and on a
/// lamp with no clock of its own the jump of its clock at the next boot can
/// pass the deadline before the confirming round runs. So a build whose
/// switch began on one boot and was put back on another is tried once
/// more. One put back on the boot it began on, or put back a second time,
/// or by a `lamp-update-back` that notes no boot (one older than this
/// rule), stays refused.
pub fn refused_for_good(records: &Path, build: &str) -> bool {
    refused(records).iter().any(|known| known == build)
        && (retried(records).iter().any(|known| known == build)
            || !put_back_on_another_boot(records, build))
}

/// This boot's id under `root`, when the kernel gives one.
pub fn boot_id(root: &Path) -> Option<String> {
    let id = fs::read_to_string(on_root(root, BOOT_ID)).ok()?;
    let id = id.trim();
    (!id.is_empty() && id.len() <= 64).then(|| id.to_owned())
}

/// Noted as a switch to `build` begins: the boot it begins on. Nothing is
/// noted when the boot is not known, and then no retry is ever made.
pub(crate) fn note_began(records: &Path, root: &Path, build: &str) -> io::Result<()> {
    let Some(boot) = boot_id(root) else {
        return Ok(());
    };
    write_whole(
        &records.join(BEGAN),
        format!("{build} {boot}\n").as_bytes(),
        0o600,
    )
}

fn put_back_on_another_boot(records: &Path, build: &str) -> bool {
    let noted = |file: &str| {
        let text = fs::read_to_string(records.join(file)).ok()?;
        let (noted, boot) = text.trim().split_once(' ')?;
        (noted == build).then(|| boot.to_owned())
    };
    matches!((noted(BEGAN), noted(PUT_BACK)), (Some(began), Some(back)) if began != back)
}

/// A build put back once, taken again: written to `self-retried` first,
/// then taken out of `self-refused`, so a cut between the two leaves it
/// refused for good rather than tried for ever. When `lamp-update-back`
/// puts it back again it writes it to `self-refused` again, as it always
/// has. Nothing for a build not refused.
pub(crate) fn retry(records: &Path, build: &str) -> io::Result<bool> {
    let refused = refused(records);
    if !refused.iter().any(|known| known == build) {
        return Ok(false);
    }
    let mut retried = retried(records);
    if !retried.iter().any(|known| known == build) {
        retried.push(build.to_owned());
        write_whole(
            &records.join(RETRIED),
            format!("{}\n", retried.join("\n")).as_bytes(),
            0o600,
        )?;
    }
    let left: Vec<String> = refused.into_iter().filter(|known| known != build).collect();
    let text = if left.is_empty() {
        String::new()
    } else {
        format!("{}\n", left.join("\n"))
    };
    write_whole(&records.join(REFUSED), text.as_bytes(), 0o600)?;
    Ok(true)
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
    /// `self.json` could not be read: settled as a switch that failed.
    /// After [`Unreadable::PutBack`] the caller starts the old build.
    Unreadable(Unreadable),
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
    let pending = match waiting(&folder) {
        Ok(None) => return Ok(Confirm::Nothing),
        Ok(Some(pending)) => pending,
        Err(Some(why)) => return Err(why),
        Err(None) => return confirm_unreadable(config, world),
    };
    if running != Some(pending.new.as_str()) || !link_names(&config.root, &pending.new) {
        return Ok(Confirm::NotOwn);
    }
    let dry = Config {
        apply: false,
        ..config.clone()
    };
    // Only what this updater is switched to apply counts: a build that
    // waits for the models switch was read and checked all the same.
    match round(&dry, world) {
        Outcome::UpToDate(_) | Outcome::Would(_) | Outcome::WaitsForModels(_) => {}
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

/// An unreadable `self.json`, seen by the updater that runs: when the
/// builds are known it is put back at once; when they are not, the file is
/// set aside only after the dry round a confirmation needs.
fn confirm_unreadable(config: &Config, world: &mut World) -> Result<Confirm, String> {
    let folder = config.on(&config.records);
    if known_builds(&folder, &config.root).is_none() {
        let dry = Config {
            apply: false,
            ..config.clone()
        };
        match round(&dry, world) {
            Outcome::UpToDate(_) | Outcome::Would(_) | Outcome::WaitsForModels(_) => {}
            other => return Ok(Confirm::NotProven(other.to_string())),
        }
    }
    let records = Records::new(folder.clone());
    let _lock = match records.lock() {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(Confirm::Busy),
        Err(error) => return Err(error.to_string()),
    };
    match waiting(&folder) {
        Ok(None) => Ok(Confirm::Nothing),
        Ok(Some(_)) => Ok(Confirm::NotOwn),
        Err(Some(why)) => Err(why),
        Err(None) => settle(&folder, &config.root, world.say).map(Confirm::Unreadable),
    }
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
    /// `self.json` could not be read: settled as a switch that failed.
    Unreadable(Unreadable),
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
    if matches!(waiting(records), Ok(None)) {
        return Ok(Back::Nothing);
    }
    let held = Records::new(records.to_owned());
    let _lock = match held.lock() {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(Back::Busy),
        Err(error) => return Err(error.to_string()),
    };
    let running = lamp_update.active == "active" && lamp_update.sub == "running";
    let late = |at: u64| now_unix_ms >= at.saturating_add(DEADLINE.as_millis() as u64);
    let pending = match waiting(records) {
        Ok(None) => return Ok(Back::Nothing),
        Ok(Some(pending)) => pending,
        Err(Some(why)) => return Err(why),
        // Its deadline runs from when the file was written.
        Err(None) => {
            if running && !late(written_unix_ms(records).unwrap_or(0)) {
                return Ok(Back::Waiting);
            }
            return settle(records, root, say).map(Back::Unreadable);
        }
    };
    let late = late(pending.at_unix_ms);
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
    refuse(records, root, &pending.new)?;
    end(records).map_err(|error| format!("{PENDING}: {error}"))?;
    say(&format!(
        "lamp-update's build {} did not confirm itself ({}): {CURRENT} names {} again",
        &pending.new[..12],
        if running { "too late" } else { "not running" },
        &pending.old[..12]
    ));
    Ok(Back::PutBack(pending))
}
