//! The text logs and the eMMC log copy emptied at a reset (#7771, the first
//! builder's four conditions at 19:05). orangepi-ramlog keeps `/var/log` on
//! a RAM disk and its copy on the eMMC in `/var/log.hdd`, and syncs the RAM
//! disk back into the copy; rsyslog's text logs and the archived journals
//! there name what the voice heard and did. So both are emptied: in each
//! of [`LOG_FOLDERS`], the [`TEXT_LOGS`] are truncated in place, never
//! removed, so a writer holding one open (rsyslog) goes on writing to it,
//! and their rotated copies (`syslog.1`, `syslog.2.gz`) are removed; in the
//! copy, each folder of [`COPY_JOURNAL`] is emptied and kept. The live
//! journal (`/var/log/journal`) is the reset's [`super::FORGET`]'s, and no
//! service is restarted: files only (a journald restart broke every
//! service's log pipe on a lamp, 15 Oct).

use super::{empty, is_dir, open_dir};
use rustix::{
    fd::OwnedFd,
    fs::{AtFlags, Mode, OFlags},
};
use std::{
    ffi::{CString, OsStr},
    io,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

/// The eMMC copy first, then the RAM disk it is synced from.
pub const LOG_FOLDERS: [&str; 2] = ["/var/log.hdd", "/var/log"];

/// The copy's archived journals, one folder a machine id.
pub const COPY_JOURNAL: &str = "/var/log.hdd/journal";

/// rsyslog's logs that carry the lamp's services' lines.
pub const TEXT_LOGS: [&str; 4] = ["syslog", "user.log", "kern.log", "auth.log"];

/// What a reset does to one log entry, as [`plan_logs`] lists it and
/// [`forget_logs`] does it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogStep {
    /// A log emptied in place, kept with its inode.
    Truncate(PathBuf),
    /// A rotated copy, a link or a stray file removed.
    Remove(PathBuf),
    /// A journal folder emptied and kept.
    Empty(PathBuf),
}

impl std::fmt::Display for LogStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogStep::Truncate(path) => write!(f, "truncate {}", path.display()),
            LogStep::Remove(path) => write!(f, "remove {}", path.display()),
            LogStep::Empty(path) => write!(f, "empty {}", path.display()),
        }
    }
}

/// The logs under `root` (`/` on a lamp) forgotten, as the module says;
/// how many entries were truncated (those not empty), removed or emptied
/// (those that held anything). A folder that is missing, or reached through
/// a link, is passed over; nothing is followed through a link.
pub fn forget_logs(root: &Path) -> io::Result<usize> {
    walk(root, true, &mut Vec::new())
}

/// What [`forget_logs`] would do under `root`, one step an entry, changing
/// nothing (`lamp-setup forget-logs --dry-run`): paths only, never a line.
pub fn plan_logs(root: &Path) -> io::Result<Vec<LogStep>> {
    let mut steps = Vec::new();
    walk(root, false, &mut steps)?;
    Ok(steps)
}

fn open_under(root: &Path, folder: &str) -> io::Result<Option<(PathBuf, OwnedFd)>> {
    let path = root.join(folder.trim_start_matches('/'));
    match open_dir(&path) {
        Ok(dir) => Ok(Some((path, dir))),
        Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

fn names(dir: &OwnedFd) -> io::Result<Vec<CString>> {
    let mut names = Vec::new();
    for entry in rustix::fs::Dir::read_from(dir)? {
        let name = entry?.file_name().to_owned();
        if !matches!(name.as_bytes(), b"." | b"..") {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn walk(root: &Path, act: bool, steps: &mut Vec<LogStep>) -> io::Result<usize> {
    if !root.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the reset's logs' root must be absolute",
        ));
    }
    let mut forgotten = 0;
    for folder in LOG_FOLDERS {
        let Some((path, dir)) = open_under(root, folder)? else {
            continue;
        };
        forgotten += text_logs(&path, &dir, act, steps)?;
    }
    if let Some((path, dir)) = open_under(root, COPY_JOURNAL)? {
        forgotten += journals(&path, &dir, act, steps)?;
    }
    Ok(forgotten)
}

/// Whether `name` is one of [`TEXT_LOGS`] (`Some(true)`), a rotated copy of
/// one (`Some(false)`), or neither.
fn text_log(name: &[u8]) -> Option<bool> {
    TEXT_LOGS.iter().find_map(|log| {
        let rest = name.strip_prefix(log.as_bytes())?;
        if rest.is_empty() {
            Some(true)
        } else if rest.starts_with(b".") {
            Some(false)
        } else {
            None
        }
    })
}

fn text_logs(
    folder: &Path,
    dir: &OwnedFd,
    act: bool,
    steps: &mut Vec<LogStep>,
) -> io::Result<usize> {
    let mut forgotten = 0;
    for name in names(dir)? {
        let Some(live) = text_log(name.as_bytes()) else {
            continue;
        };
        let path = folder.join(OsStr::from_bytes(name.as_bytes()));
        let stat = match rustix::fs::statat(dir, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => continue,
            Err(error) => return Err(error.into()),
        };
        let kind = rustix::fs::FileType::from_raw_mode(stat.st_mode);
        if is_dir(&stat) {
            continue;
        }
        if live && kind == rustix::fs::FileType::RegularFile {
            steps.push(LogStep::Truncate(path));
            if act && stat.st_size > 0 {
                let flags = OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NOCTTY;
                let file = rustix::fs::openat(dir, name.as_c_str(), flags, Mode::empty())?;
                rustix::fs::ftruncate(&file, 0)?;
                rustix::fs::fsync(&file)?;
            }
            forgotten += usize::from(stat.st_size > 0);
        } else {
            steps.push(LogStep::Remove(path));
            if act {
                rustix::fs::unlinkat(dir, name.as_c_str(), AtFlags::empty())?;
            }
            forgotten += 1;
        }
    }
    if act && forgotten > 0 {
        rustix::fs::fsync(dir)?;
    }
    Ok(forgotten)
}

fn journals(
    folder: &Path,
    dir: &OwnedFd,
    act: bool,
    steps: &mut Vec<LogStep>,
) -> io::Result<usize> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut forgotten = 0;
    for name in names(dir)? {
        let path = folder.join(OsStr::from_bytes(name.as_bytes()));
        let stat = match rustix::fs::statat(dir, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => continue,
            Err(error) => return Err(error.into()),
        };
        if is_dir(&stat) {
            let child = rustix::fs::openat(dir, name.as_c_str(), flags, Mode::empty())?;
            let held = rustix::fs::Dir::read_from(&child)?
                .filter_map(Result::ok)
                .any(|entry| !matches!(entry.file_name().to_bytes(), b"." | b".."));
            steps.push(LogStep::Empty(path));
            if act && held {
                empty(&child)?;
                rustix::fs::fsync(&child)?;
            }
            forgotten += usize::from(held);
        } else {
            steps.push(LogStep::Remove(path));
            if act {
                rustix::fs::unlinkat(dir, name.as_c_str(), AtFlags::empty())?;
            }
            forgotten += 1;
        }
    }
    if act && forgotten > 0 {
        rustix::fs::fsync(dir)?;
    }
    Ok(forgotten)
}
