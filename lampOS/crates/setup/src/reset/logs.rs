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
//!
//! Best effort, unlike the engines' homes (#8671): a log that can't be
//! read, truncated or removed (`EACCES`, `EPERM`, `EROFS`) is said with its
//! path and error, counted, and passed over; the reset goes on, so a stray
//! log never leaves the person's data half wiped.

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

/// The RAM disk first, then its eMMC copy (the Reviewer-lead's P3 on
/// #8671): an orangepi-ramlog sync between the two then copies no old line
/// of the RAM disk back into the copy.
pub const LOG_FOLDERS: [&str; 2] = ["/var/log", "/var/log.hdd"];

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

/// What [`forget_logs_saying`] did: entries truncated (those not empty),
/// removed or emptied (those that held anything), and those that could not
/// be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogsForgotten {
    pub emptied: usize,
    pub failed: usize,
}

/// The logs under `root` (`/` on a lamp) forgotten, as the module says,
/// each failure said on stderr; how many entries were emptied. Only a
/// relative `root` is an error.
pub fn forget_logs(root: &Path) -> io::Result<usize> {
    forget_logs_saying(root, &mut |line| eprintln!("lamp-setup: {line}")).map(|done| done.emptied)
}

/// [`forget_logs`], each failure given to `say` as `reset: a log could
/// not be emptied: <path>: <error>`. A folder that is missing, or reached
/// through a link, is passed over; nothing is followed through a link.
pub fn forget_logs_saying(root: &Path, say: &mut dyn FnMut(&str)) -> io::Result<LogsForgotten> {
    let mut walk = Walk {
        act: true,
        steps: Vec::new(),
        done: LogsForgotten::default(),
        say,
    };
    walk.all(root)?;
    Ok(walk.done)
}

/// What [`forget_logs`] would do under `root`, one step an entry, changing
/// nothing (`lamp-setup forget-logs --dry-run`): paths only, never a line.
/// A folder that can't be read is said on stderr and passed over.
pub fn plan_logs(root: &Path) -> io::Result<Vec<LogStep>> {
    let mut say = |line: &str| eprintln!("lamp-setup: {line}");
    let mut walk = Walk {
        act: false,
        steps: Vec::new(),
        done: LogsForgotten::default(),
        say: &mut say,
    };
    walk.all(root)?;
    Ok(walk.steps)
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

struct Walk<'a> {
    act: bool,
    steps: Vec<LogStep>,
    done: LogsForgotten,
    say: &'a mut dyn FnMut(&str),
}

impl Walk<'_> {
    fn failed(&mut self, path: &Path, error: impl std::fmt::Display) {
        (self.say)(&format!(
            "reset: a log could not be emptied: {}: {error}",
            path.display()
        ));
        self.done.failed += 1;
    }

    fn all(&mut self, root: &Path) -> io::Result<()> {
        if !root.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the reset's logs' root must be absolute",
            ));
        }
        for folder in LOG_FOLDERS {
            if let Some((path, dir)) = self.open(root, folder) {
                self.text_logs(&path, &dir);
            }
        }
        if let Some((path, dir)) = self.open(root, COPY_JOURNAL) {
            self.journals(&path, &dir);
        }
        Ok(())
    }

    fn open(&mut self, root: &Path, folder: &str) -> Option<(PathBuf, OwnedFd)> {
        let path = root.join(folder.trim_start_matches('/'));
        match open_dir(&path) {
            Ok(dir) => Some((path, dir)),
            Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR | rustix::io::Errno::LOOP) => {
                None
            }
            Err(error) => {
                self.failed(&path, error);
                None
            }
        }
    }

    fn names(&mut self, folder: &Path, dir: &OwnedFd) -> Vec<CString> {
        names(dir).unwrap_or_else(|error| {
            self.failed(folder, error);
            Vec::new()
        })
    }

    fn stat(&mut self, dir: &OwnedFd, name: &CString, path: &Path) -> Option<rustix::fs::Stat> {
        match rustix::fs::statat(dir, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => Some(stat),
            Err(rustix::io::Errno::NOENT) => None,
            Err(error) => {
                self.failed(path, error);
                None
            }
        }
    }

    fn remove(&mut self, dir: &OwnedFd, name: &CString, path: PathBuf) -> bool {
        self.steps.push(LogStep::Remove(path.clone()));
        if !self.act {
            return true;
        }
        match rustix::fs::unlinkat(dir, name.as_c_str(), AtFlags::empty()) {
            Ok(()) => {
                self.done.emptied += 1;
                true
            }
            Err(error) => {
                self.failed(&path, error);
                false
            }
        }
    }

    fn sync(&mut self, dir: &OwnedFd, path: &Path) {
        if let Err(error) = rustix::fs::fsync(dir) {
            self.failed(path, error);
        }
    }

    fn text_logs(&mut self, folder: &Path, dir: &OwnedFd) {
        let mut changed = false;
        for name in self.names(folder, dir) {
            let Some(live) = text_log(name.as_bytes()) else {
                continue;
            };
            let path = folder.join(OsStr::from_bytes(name.as_bytes()));
            let Some(stat) = self.stat(dir, &name, &path) else {
                continue;
            };
            if is_dir(&stat) {
                continue;
            }
            let regular = rustix::fs::FileType::from_raw_mode(stat.st_mode)
                == rustix::fs::FileType::RegularFile;
            if live && regular {
                self.steps.push(LogStep::Truncate(path.clone()));
                if self.act && stat.st_size > 0 {
                    match truncate(dir, &name) {
                        Ok(()) => self.done.emptied += 1,
                        Err(error) => self.failed(&path, error),
                    }
                }
            } else {
                changed |= self.remove(dir, &name, path);
            }
        }
        if self.act && changed {
            self.sync(dir, folder);
        }
    }

    fn journals(&mut self, folder: &Path, dir: &OwnedFd) {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut changed = false;
        for name in self.names(folder, dir) {
            let path = folder.join(OsStr::from_bytes(name.as_bytes()));
            let Some(stat) = self.stat(dir, &name, &path) else {
                continue;
            };
            if !is_dir(&stat) {
                changed |= self.remove(dir, &name, path);
                continue;
            }
            self.steps.push(LogStep::Empty(path.clone()));
            let child = match rustix::fs::openat(dir, name.as_c_str(), flags, Mode::empty()) {
                Ok(child) => child,
                Err(error) => {
                    self.failed(&path, error);
                    continue;
                }
            };
            let held = rustix::fs::Dir::read_from(&child).is_ok_and(|entries| {
                entries
                    .filter_map(Result::ok)
                    .any(|entry| !matches!(entry.file_name().to_bytes(), b"." | b".."))
            });
            if self.act && held {
                match empty(&child)
                    .and_then(|()| rustix::fs::fsync(&child).map_err(io::Error::from))
                {
                    Ok(()) => self.done.emptied += 1,
                    Err(error) => self.failed(&path, error),
                }
            }
        }
        if self.act && changed {
            self.sync(dir, folder);
        }
    }
}

fn truncate(dir: &OwnedFd, name: &CString) -> io::Result<()> {
    let flags = OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NOCTTY;
    let file = rustix::fs::openat(dir, name.as_c_str(), flags, Mode::empty())?;
    rustix::fs::ftruncate(&file, 0)?;
    rustix::fs::fsync(&file)?;
    Ok(())
}
