//! Harness's launcher written in Harness's own pinned shape after an
//! install, when lamp-tasks holds Harness's updates (#5493; docs/tasks.md,
//! "Harness held at its pinned version"). Off unless
//! `LAMP_HARNESS_HOLD_UPDATES=on` is in lamp-tasks' own unit, as systemd has
//! it loaded (`systemctl show lamp-tasks.service --property=Environment`).
//!
//! From Harness's maintainers: self-update is read only from the process's
//! environment (`ADAPTER_UPDATE_DISABLE`), an env file only when
//! `HARNESS_ENV_FILE` is set there, and `harness logout` or `start` restarts
//! the daemon from the caller's environment. Harness's installer with
//! `--no-updates` exports the hold before the launcher's `exec` line, and
//! Harness's own updates rewrite only that line, so the preamble stays. A
//! Harness command run by hand then holds its updates too. lamp-tasks writes
//! the same at its start, for lamps installed before this
//! (crates/tasks/src/hold.rs, `pin_at_start`).
use crate::install::{HARNESS_HOME, Runner};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// The switch, in lamp-tasks' own unit: `on` exactly.
pub const SWITCH: &str = "LAMP_HARNESS_HOLD_UPDATES";
/// The variable Harness's daemon holds its updates by.
const DISABLE: &str = "ADAPTER_UPDATE_DISABLE";
/// The variable naming the one file Harness loads.
const ENV_FILE: &str = "HARNESS_ENV_FILE";
/// Harness's launcher, under Harness's home.
pub const LAUNCHER: &str = ".local/bin/harness";
/// The file lamp-tasks writes the hold in, in Harness's home.
pub const HOLD_FILE: &str = "harness-hold.env";
/// The temporary file the launcher is written through, beside it.
const NEXT: &str = ".harness.lampos-pin";

/// Whether lamp-tasks' unit, as systemd has it loaded, sets the switch
/// `on`: one `systemctl show` through `run`, the seam a test replaces.
/// `Err` (taken as off) when systemctl fails.
pub fn switch_on(run: &mut Runner) -> Result<bool, String> {
    let argv = [
        "systemctl",
        "show",
        "lamp-tasks.service",
        "--property=Environment",
        "--value",
    ]
    .map(str::to_owned);
    let environment = run(&argv)?;
    Ok(environment
        .split_whitespace()
        .any(|assignment| assignment == format!("{SWITCH}=on")))
}

/// `text`, a Harness launcher, in Harness's pinned shape: every
/// `export ADAPTER_UPDATE_DISABLE=` and `export HARNESS_ENV_FILE=` line
/// before its first `exec` line taken out, then `export
/// ADAPTER_UPDATE_DISABLE=true` and `export HARNESS_ENV_FILE=<env_file>`
/// put just before it. The `exec` line and every other line are kept as
/// they are, so a launcher already pinned comes back byte for byte. `None`
/// when there is no `exec` line. The same text as lamp-tasks' own.
pub fn pinned_launcher(text: &str, env_file: &str) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let exec = lines
        .iter()
        .position(|line| line.trim_start().starts_with("exec "))?;
    let mut pinned = String::with_capacity(text.len() + 96);
    for line in &lines[..exec] {
        let line_text = line.trim();
        if line_text.starts_with(&format!("export {DISABLE}="))
            || line_text.starts_with(&format!("export {ENV_FILE}="))
        {
            continue;
        }
        pinned.push_str(line);
    }
    pinned.push_str(&format!("export {DISABLE}=true\n"));
    pinned.push_str(&format!("export {ENV_FILE}={env_file}\n"));
    for line in &lines[exec..] {
        pinned.push_str(line);
    }
    Some(pinned)
}

/// What [`pin`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Pinned {
    /// Written in the pinned shape.
    Written,
    /// Already in it: nothing written.
    Kept,
    /// No launcher there: nothing written, none made.
    Missing,
}

/// The launcher at `launcher` written in the pinned shape with `env_file`,
/// whole: through a file beside it, with its mode (executable) and owner
/// (Harness's user, not root), then renamed over it. One already pinned is
/// not written; a missing one is not made. An error, with the launcher
/// unchanged, when it is not a plain file, has no `exec` line, or cannot be
/// written.
pub fn pin(launcher: &Path, env_file: &str) -> Result<Pinned, String> {
    let said = |error: &dyn std::fmt::Display| format!("{}: {error}", launcher.display());
    let metadata = match fs::symlink_metadata(launcher) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Pinned::Missing),
        Err(error) => return Err(said(&error)),
    };
    if !metadata.is_file() {
        return Err(said(&"not a plain file"));
    }
    let text = fs::read_to_string(launcher).map_err(|error| said(&error))?;
    let pinned = pinned_launcher(&text, env_file).ok_or_else(|| said(&"no exec line"))?;
    if pinned == text {
        return Ok(Pinned::Kept);
    }
    let next = launcher.with_file_name(NEXT);
    let written =
        write_next(&next, pinned.as_bytes(), &metadata).and_then(|()| fs::rename(&next, launcher));
    if let Err(error) = written {
        let _ = fs::remove_file(&next);
        return Err(said(&error));
    }
    Ok(Pinned::Written)
}

fn write_next(next: &Path, bytes: &[u8], was: &fs::Metadata) -> io::Result<()> {
    let mut mode = was.mode() & 0o7777;
    if mode & 0o100 == 0 {
        mode = 0o755;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(next)?;
    file.write_all(bytes)?;
    let now = file.metadata()?;
    if (now.uid(), now.gid()) != (was.uid(), was.gid()) {
        std::os::unix::fs::fchown(&file, Some(was.uid()), Some(was.gid()))?;
    }
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.sync_all()
}

/// After an install: with the switch on in lamp-tasks' unit, Harness's
/// launcher under `root` pinned to `<HARNESS_HOME>/harness-hold.env`, and
/// one line said; with it off, or lamp-tasks' unit unreadable (taken as
/// off), the launcher not looked at or written, and one line said.
pub fn after_install(root: &Path, run: &mut Runner, say: &mut dyn FnMut(&str)) {
    match switch_on(run) {
        Ok(true) => {}
        Ok(false) => {
            say(&format!(
                "{SWITCH} is not on in lamp-tasks.service: Harness's launcher left as it is"
            ));
            return;
        }
        Err(why) => {
            say(&format!(
                "lamp-tasks.service's environment not read ({why}): {SWITCH} taken as off, Harness's launcher left as it is"
            ));
            return;
        }
    }
    let home = root.join(HARNESS_HOME.trim_start_matches('/'));
    let launcher = home.join(LAUNCHER);
    let env_file = format!("{HARNESS_HOME}/{HOLD_FILE}");
    match pin(&launcher, &env_file) {
        Ok(Pinned::Written) => say(&format!(
            "Harness's launcher pinned ({DISABLE}=true and {ENV_FILE}={env_file} exported before its exec line)"
        )),
        Ok(Pinned::Kept) => say("Harness's launcher already pinned"),
        Ok(Pinned::Missing) => say(&format!(
            "no Harness launcher at {HARNESS_HOME}/{LAUNCHER}: left alone"
        )),
        Err(why) => say(&format!("Harness's launcher not pinned: {why}")),
    }
}
