//! A long held trigger kept across a short restart of lamp-skills
//! (`docs/skills.md`, "A hold kept across a restart").
//!
//! A trigger's `for` is counted in memory from when it began to hold
//! (`since_us`, on the host's monotonic clock, which runs on through a
//! restart of the program and begins again only at a reboot). Each update
//! restarts lamp-skills, so without this a 50- or 90-minute sitting began
//! again at every update (a product lamp, build 11756: restarted at 11:03,
//! 11:03 and 11:39, and its water nudge rarely came).
//!
//! The holds are written to one small file in the program's runtime folder
//! (`--tools-dir`, `/run/lampos-skills`: tmpfs, gone at a reboot, kept
//! through a restart by the unit's `RuntimeDirectoryPreserve=restart`): the
//! boot, when it was written, and each hold's skill, trigger, a fingerprint
//! of the trigger, when it began and was last seen holding. A hold is taken
//! back at the start only on the same boot, for the same skill and the same
//! trigger, written within that trigger's own `gap` of now; the engine then
//! wants the next message to hold, or drops it ([`crate::Engine`]).
use crate::Trigger;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// The file in the runtime folder (`--tools-dir`) that keeps the holds.
pub const FILE: &str = "holds.json";
/// Only a trigger whose `for` is at least this long is kept: a shorter one
/// loses little to a restart of a few seconds.
pub const KEPT_FROM_US: u64 = 60_000_000;
/// While a hold is kept, it is written again at most this often, so the
/// file says it was still held within this long; at once when a hold
/// begins, ends or changes otherwise.
pub const WRITE_EVERY_US: u64 = 30_000_000;
const VERSION: u32 = 1;
/// The most holds kept, and the largest file read.
const MOST_HOLDS: usize = 256;
const MOST_BYTES: u64 = 64 * 1024;

/// One trigger's hold, as kept.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hold {
    pub skill: String,
    /// The trigger's place in the skill.
    pub trigger: usize,
    /// [`fingerprint`] of the trigger: an edited one does not inherit.
    pub fingerprint: String,
    pub since_us: u64,
    pub last_held_us: u64,
    /// This hold's chance was had (once a sitting).
    #[serde(default)]
    pub spent: bool,
    /// The trigger's last run, for its cooldown.
    #[serde(default)]
    pub ran_us: Option<u64>,
    /// Since when this hold's chance has waited for its `[[while]]`.
    #[serde(default)]
    pub waiting_us: Option<u64>,
}

impl Hold {
    /// The same but for when it was last seen holding, which moves at each
    /// message and is written only every [`WRITE_EVERY_US`].
    fn same_but_seen(&self, other: &Self) -> bool {
        Self {
            last_held_us: 0,
            ..self.clone()
        } == Self {
            last_held_us: 0,
            ..other.clone()
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Kept {
    version: u32,
    boot: String,
    written_us: u64,
    holds: Vec<Hold>,
}

/// The holds read at the start: when the file was written, and what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restored {
    pub read_us: u64,
    pub written_us: u64,
    pub holds: Vec<Hold>,
}

/// A trigger's fingerprint: the SHA-256 of what the program read of it,
/// so any edit to it, or to how it is read, begins its hold again.
pub fn fingerprint(trigger: &Trigger) -> String {
    Sha256::digest(format!("{trigger:?}").as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Where the holds are kept, on which boot, and what was last written.
pub struct Keeper {
    file: PathBuf,
    boot: String,
    written: Option<(u64, Vec<Hold>)>,
    faulted: bool,
}

impl Keeper {
    pub fn new(file: PathBuf, boot: String) -> Self {
        Self {
            file,
            boot,
            written: None,
            faulted: false,
        }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The holds kept on this boot, if the file is there, is this boot's,
    /// and was not written after `now_us` (a test's skipped clock is not
    /// carried into a new process). A file of another boot, or one that
    /// can't be read, is none.
    pub fn read(&self, now_us: u64) -> io::Result<Option<Restored>> {
        let mut text = String::new();
        match fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&self.file)
        {
            Ok(file) => file.take(MOST_BYTES).read_to_string(&mut text)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let kept: Kept = serde_json::from_str(&text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if kept.version != VERSION
            || kept.boot != self.boot
            || kept.written_us > now_us
            || kept.holds.len() > MOST_HOLDS
        {
            return Ok(None);
        }
        Ok(Some(Restored {
            read_us: now_us,
            written_us: kept.written_us,
            holds: kept.holds,
        }))
    }

    /// `holds` written at `now_us` when they differ from the last written
    /// but for when each was last seen holding, or, while any is kept,
    /// [`WRITE_EVERY_US`] after the last write. The first failure is said.
    pub fn keep(&mut self, mut holds: Vec<Hold>, now_us: u64) {
        holds.truncate(MOST_HOLDS);
        let due = match &self.written {
            None => true,
            Some((at, before)) => {
                before.len() != holds.len()
                    || before
                        .iter()
                        .zip(&holds)
                        .any(|(before, now)| !before.same_but_seen(now))
                    || (!holds.is_empty() && now_us.saturating_sub(*at) >= WRITE_EVERY_US)
            }
        };
        if !due {
            return;
        }
        let kept = Kept {
            version: VERSION,
            boot: self.boot.clone(),
            written_us: now_us,
            holds,
        };
        match write(&self.file, &kept) {
            Ok(()) => self.faulted = false,
            Err(error) if !self.faulted => {
                self.faulted = true;
                eprintln!(
                    "lamp-skills: the holds not kept in {}: {error}",
                    self.file.display()
                );
            }
            Err(_) => {}
        }
        // Tried again at the next change or the next cadence, not at once.
        self.written = Some((now_us, kept.holds));
    }
}

/// The file written whole: beside it, made new (never through a link, a
/// file a cut-off run left there removed first), then moved over it, as
/// [`crate::keep_names`] writes. Not synced: it is on tmpfs, which a power
/// cut empties anyway.
fn write(file: &Path, kept: &Kept) -> io::Result<()> {
    let text = serde_json::to_vec(kept).map_err(io::Error::other)?;
    let new = file.with_extension("new");
    match fs::symlink_metadata(&new) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(io::Error::other(format!(
                "{} is a link: refused",
                new.display()
            )));
        }
        Ok(_) => fs::remove_file(&new)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(&new)?;
    out.write_all(&text)?;
    drop(out);
    fs::rename(&new, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hold(since_us: u64, last_held_us: u64) -> Hold {
        Hold {
            skill: "sip".into(),
            trigger: 0,
            fingerprint: "f".into(),
            since_us,
            last_held_us,
            spent: false,
            ran_us: None,
            waiting_us: None,
        }
    }

    /// Written at a change at once, and only every 30 s while it only goes
    /// on holding; read back on its boot only, never one written later.
    #[test]
    fn written_at_a_change_and_on_a_cadence_and_read_on_its_boot() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let mut keeper = Keeper::new(file.clone(), "boot-a".into());
        keeper.keep(vec![hold(10, 10)], 10);
        let first = fs::read(&file).unwrap();
        keeper.keep(vec![hold(10, 20_000_000)], 20_000_000);
        assert_eq!(fs::read(&file).unwrap(), first, "only seen again: not due");
        keeper.keep(vec![hold(10, 31_000_000)], 31_000_000);
        let read = keeper.read(32_000_000).unwrap().unwrap();
        assert_eq!(read.written_us, 31_000_000);
        assert_eq!(read.holds, vec![hold(10, 31_000_000)]);
        keeper.keep(vec![hold(40_000_000, 40_000_000)], 40_000_000);
        assert_eq!(
            keeper.read(41_000_000).unwrap().unwrap().written_us,
            40_000_000
        );
        assert!(keeper.read(39_000_000).unwrap().is_none(), "written later");
        let other = Keeper::new(file.clone(), "boot-b".into());
        assert!(other.read(50_000_000).unwrap().is_none(), "another boot");
        keeper.keep(Vec::new(), 41_000_000);
        assert!(keeper.read(42_000_000).unwrap().unwrap().holds.is_empty());
        fs::write(&file, "not json").unwrap();
        assert!(keeper.read(42_000_000).is_err());
        assert!(!dir.path().join("holds.new").exists());
    }
}
