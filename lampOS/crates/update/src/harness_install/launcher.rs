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
use rustix::{
    fs::{self as rfs, AtFlags, Mode, OFlags},
    io::Errno,
};
use std::{
    fs,
    io::{self, Read, Write},
    os::{
        fd::OwnedFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
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

/// The folders between Harness's home and its launcher, and the launcher's
/// own name: [`LAUNCHER`] in parts.
const BIN: [&str; 2] = [".local", "bin"];
const NAME: &str = "harness";

/// The most of a launcher read or pinned: 64 KiB. Harness's own launcher is a
/// few hundred bytes (a shebang, maybe two exports, one `exec` line), so a
/// bigger one is not Harness's; it is left as it is, so a launcher made huge
/// cannot make root read it all into memory (a security read's P3 on #5647).
pub(crate) const MAX_LAUNCHER: u64 = 64 * 1024;

/// The launcher at `launcher` (`<home>/`[`LAUNCHER`]) written in the pinned
/// shape with `env_file`, whole: through a file beside it, with its mode
/// (executable) and owner (Harness's user, not root), then renamed over it.
/// One already pinned is not written; a missing one is not made. An error,
/// with the launcher unchanged, when it is not a plain file, has a second
/// hard link, is not owned by its bin folder's owner, is over
/// [`MAX_LAUNCHER`], has no `exec` line, or cannot be written.
///
/// This runs as root in a folder Harness's user owns, so nothing there is
/// opened through a link (the Reviewer-lead on #5624): `.local` and `bin`
/// are opened without following one, and the launcher is opened, checked
/// (fstat), read and its mode and owner taken through that one open file;
/// the temporary file is made new (`O_CREAT|O_EXCL|O_NOFOLLOW`) in that
/// folder after any name left there is unlinked, written and given its mode
/// and owner through its own open file, and renamed in the same folder.
pub fn pin(launcher: &Path, env_file: &str) -> Result<Pinned, String> {
    let said = |error: &dyn std::fmt::Display| format!("{}: {error}", launcher.display());
    let home = launcher
        .ancestors()
        .nth(3)
        .filter(|_| launcher.ends_with(LAUNCHER))
        .ok_or_else(|| said(&format!("not at <home>/{LAUNCHER}")))?;
    let dir = match bin_folder(home) {
        Ok(dir) => dir,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Pinned::Missing),
        Err(error) => return Err(said(&error)),
    };
    let file = match rfs::openat(
        &dir,
        NAME,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fs::File::from(fd),
        Err(Errno::NOENT) => return Ok(Pinned::Missing),
        Err(Errno::LOOP) => {
            return Err(said(
                &"a link, left as it is (its target not read or written)",
            ));
        }
        Err(error) => return Err(said(&io::Error::from(error))),
    };
    let metadata = file.metadata().map_err(|error| said(&error))?;
    if !metadata.is_file() {
        return Err(said(&"not a plain file"));
    }
    let bin_uid = rfs::fstat(&dir)
        .map_err(|error| said(&io::Error::from(error)))?
        .st_uid;
    if let Some(why) = refused(metadata.nlink(), metadata.uid(), bin_uid) {
        return Err(said(&why));
    }
    if let Some(why) = too_big(metadata.len()) {
        return Err(said(&why));
    }
    // Bounded again while reading, for a launcher that grew after the fstat.
    let mut text = String::new();
    let read = (&file)
        .take(MAX_LAUNCHER + 1)
        .read_to_string(&mut text)
        .map_err(|error| said(&error))?;
    if let Some(why) = too_big(read as u64) {
        return Err(said(&why));
    }
    drop(file);
    let pinned = pinned_launcher(&text, env_file).ok_or_else(|| said(&"no exec line"))?;
    if pinned == text {
        return Ok(Pinned::Kept);
    }
    let written = write_next(&dir, pinned.as_bytes(), &metadata)
        .and_then(|()| rfs::renameat(&dir, NEXT, &dir, NAME).map_err(io::Error::from));
    if let Err(error) = written {
        let _ = rfs::unlinkat(&dir, NEXT, AtFlags::empty());
        return Err(said(&error));
    }
    Ok(Pinned::Written)
}

/// Why a plain-file launcher is not pinned (two security reads' P3 on
/// #5624): a second hard link (another name for the same file, which the
/// rename would part from it) or an owner other than its bin folder's
/// (Harness's user). `None` when it may be pinned.
pub(crate) fn refused(nlink: u64, uid: u32, bin_uid: u32) -> Option<String> {
    if nlink != 1 {
        return Some(format!("hard-linked ({nlink} names), left as it is"));
    }
    (uid != bin_uid)
        .then(|| format!("owned by uid {uid}, not the bin folder's owner {bin_uid}, left as it is"))
}

/// Why a launcher of `bytes` is not read or pinned: over [`MAX_LAUNCHER`].
fn too_big(bytes: u64) -> Option<String> {
    (bytes > MAX_LAUNCHER).then(|| format!("{bytes} bytes, over the 64 KiB cap, left as it is"))
}

/// `<home>/.local/bin`, opened as a folder, neither `.local` nor `bin`
/// through a link. `home` itself is root's path to Harness's home.
fn bin_folder(home: &Path) -> io::Result<OwnedFd> {
    let mut dir = rfs::open(
        home,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    for part in BIN {
        dir = rfs::openat(
            &dir,
            part,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| match error {
            Errno::LOOP | Errno::NOTDIR => {
                io::Error::other(format!("{part} is not a plain folder (a link?)"))
            }
            error => io::Error::from(error),
        })?;
    }
    Ok(dir)
}

/// [`NEXT`] in `dir`, made new (any name there unlinked first, never
/// followed), holding `bytes`, with `was`'s owner and mode (executable).
fn write_next(dir: &OwnedFd, bytes: &[u8], was: &fs::Metadata) -> io::Result<()> {
    let mut mode = was.mode() & 0o7777;
    if mode & 0o100 == 0 {
        mode = 0o755;
    }
    match rfs::unlinkat(dir, NEXT, AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => {}
        Err(error) => return Err(error.into()),
    }
    let fd = rfs::openat(
        dir,
        NEXT,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?;
    let mut file = fs::File::from(fd);
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
