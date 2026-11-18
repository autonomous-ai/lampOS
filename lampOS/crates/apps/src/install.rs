//! One install or update, in one order. The owner's yes is used up first, and
//! nothing is written before it; then the companion skills are placed with
//! the apps lock let go; then, under the apps lock, the app is laid beside
//! any older version, the `current` link is switched in one rename, and the
//! unit is (re)started unless the owner had stopped it.
//!
//! A failure is cleaned as the sweep cleans a cut install (the linked version
//! is never removed unless this install linked it and was fresh); a cut with no undo (a crash, the
//! helper's time limit) leaves the `installing` mark, and the start-up sweep
//! finishes the job ([`crate::sweep`]).
use crate::{
    effects::{Effects, Verb},
    entry::entry_env,
    facts::{check, unit_of},
    layout::Layout,
    lock::AppsLock,
    name::{net_unit, plain_unit},
    package::{AppPackage, PackageReader, paths_ok, sha256_hex},
    remove::{Companions, remove_entry},
    state::{Mark, clear, has, set},
    sweep::clean_install,
    yes::{Op, Yes, YesKey},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::{
        fs::symlink,
        fs::{DirBuilderExt, OpenOptionsExt},
    },
    path::Path,
};

/// What an app that is being installed or removed is told.
pub const BUSY_APP: &str = "this app is being changed";
/// What an install of the version already installed is told.
pub const ALREADY: &str = "this version is already installed";
/// The helper's own record of a version, in its folder: written by the helper
/// from the verified manifest, never from a package file (a package may not
/// have a file of this name).
pub const RECORD: &str = "lamp-apps.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub version: String,
    pub grants: Vec<String>,
    pub net: bool,
    /// Absent in a record an earlier build wrote: no logs.
    #[serde(default)]
    pub logs: bool,
    pub sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Installed {
    pub name: String,
    pub version: String,
    /// The unit instance that runs it.
    pub unit: String,
    /// Whether it was started now: not when the owner had stopped it.
    pub started: bool,
    /// An update of an app that was there.
    pub updated: bool,
}

/// The installed version's record, if the app is installed.
pub fn installed_record(layout: &Layout, name: &str) -> Option<Record> {
    let text = fs::read_to_string(layout.code.join(name).join("current").join(RECORD)).ok()?;
    serde_json::from_str(&text).ok()
}

fn triple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

fn sorted(grants: &[String]) -> Vec<String> {
    let mut grants = grants.to_vec();
    grants.sort();
    grants.dedup();
    grants
}

/// The refusal for a version that is not newer than the one installed.
fn newer_than(have: &Option<Record>, version: &str) -> Result<(), String> {
    if let Some(have) = have {
        match triple(version).cmp(&triple(&have.version)) {
            std::cmp::Ordering::Equal => return Err(ALREADY.to_owned()),
            std::cmp::Ordering::Less => {
                return Err(format!(
                    "{version} is older than the {} installed",
                    have.version
                ));
            }
            std::cmp::Ordering::Greater => {}
        }
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn make_dirs(path: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Lays the version folder beside the others, as a temporary folder first.
fn lay(app_dir: &Path, package: &AppPackage, record: &Record, sha: &str) -> Result<(), String> {
    let _ = sha;
    let version = &package.app.version;
    let tmp = app_dir.join(format!(".tmp-{}-{version}", std::process::id()));
    remove_entry(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    make_dirs(&tmp)?;
    for (path, bytes) in &package.files {
        let dest = tmp.join(path);
        if let Some(parent) = dest.parent() {
            make_dirs(parent)?;
        }
        write_file(&dest, bytes)?;
    }
    write_file(
        &tmp.join("entry.env"),
        entry_env(&package.app.entry)?.as_bytes(),
    )?;
    let record = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    write_file(&tmp.join(RECORD), &record)?;
    let done = app_dir.join(version);
    remove_entry(&done).map_err(|e| format!("{}: {e}", done.display()))?;
    fs::rename(&tmp, &done).map_err(|e| format!("{}: {e}", done.display()))
}

/// `current` -> the version, in one rename.
fn switch(app_dir: &Path, version: &str) -> Result<(), String> {
    let fresh = app_dir.join(".current.new");
    remove_entry(&fresh).map_err(|e| e.to_string())?;
    symlink(version, &fresh).map_err(|e| format!("{}: {e}", fresh.display()))?;
    fs::rename(&fresh, app_dir.join("current")).map_err(|e| format!("current: {e}"))
}

/// The apps lock is held for what is done here, and for nothing else.
fn lay_and_start(
    layout: &Layout,
    fx: &mut dyn Effects,
    package: &AppPackage,
    sha: &str,
    before: &Option<Record>,
    switched: &mut bool,
) -> Result<Installed, String> {
    let name = &package.app.name;
    let _held = AppsLock::take(&layout.state, layout.lock_wait)?;
    // The state may have moved while the skills were being placed: a remove
    // of the same app may have run in the gap, or another install finished.
    if has(&layout.state, name, Mark::Removing) {
        return Err(BUSY_APP.to_owned());
    }
    let now = installed_record(layout, name);
    if now != *before {
        newer_than(&now, &package.app.version)?;
    }
    let app_dir = layout.code.join(name);
    make_dirs(&app_dir)?;
    let record = Record {
        version: package.app.version.clone(),
        grants: sorted(&package.app.grants),
        net: package.app.net,
        logs: package.app.logs,
        sha256: sha.to_owned(),
    };
    lay(&app_dir, package, &record, sha)?;
    switch(&app_dir, &package.app.version)?;
    *switched = true;
    let unit = unit_of(&package.app);
    if let Some(old) = before
        && old.net != package.app.net
    {
        // Never two variants of one app at once.
        let other = if old.net {
            net_unit(name)
        } else {
            plain_unit(name)
        };
        fx.systemctl(Verb::Stop, &other)?;
    }
    let started = !has(&layout.state, name, Mark::Stopped);
    if started {
        fx.systemctl(Verb::Restart, &unit)?;
    }
    clear(&layout.state, name, Mark::Installing).map_err(|e| format!("a mark: {e}"))?;
    Ok(Installed {
        name: name.clone(),
        version: package.app.version.clone(),
        unit,
        started,
        updated: before.is_some(),
    })
}

/// Installs or updates from a staged package's bytes (read by
/// [`crate::staging::read_staged`]); `build` is this lamp's build sequence.
pub fn install(
    layout: &Layout,
    fx: &mut dyn Effects,
    skills: &mut dyn Companions,
    yes: &mut dyn Yes,
    reader: &dyn PackageReader,
    bytes: &[u8],
    build: Option<u64>,
) -> Result<Installed, String> {
    let package = reader.read(bytes)?;
    check(&package.app, build)?;
    paths_ok(&package)?;
    let name = package.app.name.clone();
    let before = installed_record(layout, &name);
    if before.is_none() && skills.taken_by_skill(&name) {
        return Err(format!("a skill is already called {name}"));
    }
    newer_than(&before, &package.app.version)?;
    let sha = sha256_hex(bytes);
    let key = YesKey {
        op: if before.is_some() {
            Op::Update
        } else {
            Op::Install
        },
        name: name.clone(),
        version: package.app.version.clone(),
        sha256: sha.clone(),
        grants: match &before {
            None => sorted(&package.app.grants),
            Some(have) => sorted(&package.app.grants)
                .into_iter()
                .filter(|grant| !have.grants.contains(grant))
                .collect(),
        },
        // `net` and `logs` are manifest booleans outside the grants but as
        // much a permission: an install says them as the manifest does, an
        // update only what it newly turns on (the FB's condition, #7415).
        net: package.app.net && before.as_ref().is_none_or(|have| !have.net),
        logs: package.app.logs && before.as_ref().is_none_or(|have| !have.logs),
    };
    // The yes is used up under the lock, so a busy lamp does not spend it,
    // and nothing is written before it.
    {
        let _held = AppsLock::take(&layout.state, layout.lock_wait)?;
        // An app being installed or removed is refused before the yes is spent.
        if has(&layout.state, &name, Mark::Removing) || has(&layout.state, &name, Mark::Installing)
        {
            return Err(BUSY_APP.to_owned());
        }
        yes.take(&key).map_err(|why| format!("no yes: {why}"))?;
        if before.is_none() {
            // A stopped mark left by an earlier remove must not make a fresh install start stopped.
            clear(&layout.state, &name, Mark::Stopped).map_err(|e| format!("a mark: {e}"))?;
        }
        set(&layout.state, &name, Mark::Installing).map_err(|e| format!("a mark: {e}"))?;
    }
    let mut switched = false;
    let result = skills
        .place(&name, &package.skills)
        .and_then(|()| lay_and_start(layout, fx, &package, &sha, &before, &mut switched));
    if result.is_err() {
        // Only an install that put its own version in as `current` may take
        // the whole app out again, and only if it was fresh.
        undo(layout, skills, &name, before.is_none() && switched);
    }
    result
}

/// A failed install or update is cleaned as the sweep cleans a cut one
/// ([`clean_install`]): temporary and other version folders go and the
/// version that `current` links is kept. Only a fresh install that itself
/// linked its version (`ours`) takes the whole app out again. `before` was
/// read without the lock, so it never decides this alone: a failed fresh
/// install must not remove another install of the same name that finished
/// meanwhile.
fn undo(layout: &Layout, skills: &mut dyn Companions, name: &str, ours: bool) {
    if ours {
        if let Ok(_held) = AppsLock::take(&layout.state, layout.lock_wait) {
            let _ = remove_entry(&layout.code.join(name));
        }
        let _ = skills.remove_for(name);
        if let Ok(_held) = AppsLock::take(&layout.state, layout.lock_wait) {
            let _ = clear(&layout.state, name, Mark::Installing);
        }
    } else {
        let _ = clean_install(layout, skills, name);
    }
}
