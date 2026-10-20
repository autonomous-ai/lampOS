//! What lamp-update keeps, in its own folder (`/var/lib/lampos-update`):
//!
//! - `state.json`: the build the lamp runs and the one before, the builds
//!   refused after a failed switch (so the same build is not tried again),
//!   the folders it made (the only ones it ever removes), and the product
//!   settings as `install` last wrote them (a setting changed since is the
//!   lamp's own and is kept).
//! - `journal.json`: a switch under way. Written before any unit is
//!   changed and removed once the switch is over, so a switch cut short by
//!   a stop or a power cut is put back at the next start: the units, the
//!   links, and the product skills' files as they were.
//! - `previous/`: the units as they were before the last switch.
//! - `log.jsonl`: one line for each thing done or refused.
//! - `lock`: one lamp-update at a time.
//!
//! Every file is written whole: beside, synced, renamed, and the folder
//! synced. A state that cannot be read is kept aside and begun again, as
//! V1's updater did.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// The builds remembered as refused.
const MOST_REFUSED: usize = 32;
/// The log is begun again past this size, the old one kept once.
const MOST_LOG_BYTES: u64 = 1024 * 1024;

/// A build, by its sequence and commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    pub sequence: u64,
    pub commit: String,
}

/// A build that was switched to and put back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refusal {
    pub sequence: u64,
    pub commit: String,
    pub why: String,
    pub at_unix_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub installed: Option<Build>,
    pub previous: Option<Build>,
    pub refused: Vec<Refusal>,
    /// Build folders this program made, as the lamp names them.
    pub made: Vec<String>,
    /// Each product setting `install` wrote, or found already the build's
    /// default, by path: the SHA-256 of those bytes. A setting whose bytes
    /// no longer match was changed on the lamp and is never written over
    /// (docs/install.md). A key path may hold `install::SET_ASIDE` instead,
    /// no hash: written once, before records that were set aside. Absent
    /// from a state written before.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, String>,
    /// The SHA-256 of each product setting `install` is about to write,
    /// recorded before it writes: a run stopped between the write and its
    /// record leaves a file that is still install's (docs/install.md).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings_writing: BTreeMap<String, String>,
    /// `install` wrote what systemd has not yet been told of
    /// (`systemd-tmpfiles --create`, `systemctl enable`): the next install
    /// tells it, though nothing else has changed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub systemd_owed: bool,
    /// Each model path a round linked into the store, and the SHA-256 it
    /// names now; and as they were before the last switch. A store entry
    /// either names is kept (#1680). Absent from a state written before.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub models_before: BTreeMap<String, String>,
    /// Fields a later build wrote that this one does not know: kept as
    /// they were across a save, for that build to read again (#1505).
    #[serde(flatten)]
    pub later: BTreeMap<String, serde_json::Value>,
}

impl State {
    pub fn refusal(&self, sequence: u64) -> Option<&Refusal> {
        self.refused
            .iter()
            .find(|refusal| refusal.sequence == sequence)
    }

    pub fn refuse(&mut self, build: &Build, why: &str) {
        self.refused.push(Refusal {
            sequence: build.sequence,
            commit: build.commit.clone(),
            why: why.to_owned(),
            at_unix_ms: unix_ms(),
        });
        let extra = self.refused.len().saturating_sub(MOST_REFUSED);
        self.refused.drain(..extra);
    }
}

/// One unit of a switch under way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Switching {
    pub unit: String,
    /// It ran before the switch: it is started again on its way back.
    pub was_active: bool,
    /// A part started through its `current` link: the link, and the build
    /// it named before the switch, to name again on the way back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkBack>,
}

/// A `current` link as it was before a switch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkBack {
    /// As the lamp names it: `/opt/lampOS/voice/current`.
    pub path: String,
    /// The build it named: 64 hex.
    pub build: String,
}

/// The name of a link kept in `previous/`: `<unit>.link`, holding
/// `<link path> <build>`, for going back by hand (docs/updates.md).
pub const LINK_SUFFIX: &str = ".link";

/// A switch under way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub build: Build,
    pub units: Vec<Switching>,
    /// Each model path the switch moves, as it was (#1680): put back in
    /// the reverse order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<crate::models::ModelBack>,
    /// `lamp-update-apply` was asked to write the build's settings (#1680):
    /// they are put back with the rest.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub settings: bool,
    /// The product skills' files the switch writes, as they were before
    /// (#2014's P2): put back with the units. Absent from a journal written
    /// before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<SkillBack>,
}

/// A product skill's file as it was before a round wrote it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillBack {
    /// As the lamp names it: `/var/lib/lampOS/skills/dance/skill.toml`.
    pub path: String,
    /// Its text, or none: it was not there.
    #[serde(default)]
    pub before: Option<String>,
    /// The SHA-256 of what the round writes: a file that no longer holds
    /// it was changed on the lamp since, and is left.
    pub wrote: String,
    /// The folders made for it, the deepest first: removed again, if still
    /// empty, when it is put back as never there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub made: Vec<String>,
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// Writes `bytes` to `path` whole: beside, synced, renamed, folder synced.
pub fn write_whole(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("no file name"))?
        .to_string_lossy();
    let beside = path.with_file_name(format!(".{name}.lamp-update"));
    // One left beside by a run cut off is removed, a link as a link, and
    // the new one made afresh: never opened through a link or reused.
    match fs::remove_file(&beside) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .mode(mode)
            .open(&beside)?;
        // The mode is set, not only asked for at creation: the umask
        // narrows that.
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&beside, path)?;
    sync_folder(path.parent().unwrap_or(Path::new("/")))
}

/// Points the link at `path` to `target`, replaced whole: a new link made
/// beside under a temporary name, renamed over it, the folder synced. A
/// link that already names `target` is left as it is.
pub fn replace_link(path: &Path, target: &str) -> io::Result<()> {
    if fs::read_link(path).is_ok_and(|now| now == Path::new(target)) {
        return Ok(());
    }
    let beside = path.with_file_name(".current.lamp-update");
    if fs::symlink_metadata(&beside).is_ok() {
        fs::remove_file(&beside)?;
    }
    std::os::unix::fs::symlink(target, &beside)?;
    fs::rename(&beside, path)?;
    sync_parent(path)
}

pub fn sync_folder(folder: &Path) -> io::Result<()> {
    File::open(folder)?.sync_all()
}

/// Syncs the folder a path is in: its new name is then on the disk.
pub fn sync_parent(path: &Path) -> io::Result<()> {
    sync_folder(path.parent().unwrap_or(Path::new("/")))
}

/// The folder of records, made private if it is not there.
pub struct Records {
    folder: PathBuf,
}

/// Held while one lamp-update works; let go when dropped.
pub struct Lock {
    _file: File,
}

impl Records {
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    fn make(&self) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(self.folder.join("previous"))
    }

    /// One lamp-update at a time: a second is told, and does nothing.
    pub fn lock(&self) -> io::Result<Lock> {
        self.make()?;
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(self.folder.join("lock"))?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
            |_| io::Error::new(io::ErrorKind::WouldBlock, "another lamp-update is at work"),
        )?;
        Ok(Lock { _file: file })
    }

    /// The state. One that cannot be read is moved aside, said, and begun
    /// again, with the key paths marked `install::SET_ASIDE`: an update must
    /// not stop for good because of its own file.
    pub fn state(&self, say: &mut dyn FnMut(&str)) -> io::Result<State> {
        let path = self.folder.join("state.json");
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(state) => Ok(state),
                Err(error) => {
                    let aside = path.with_file_name(format!("state.json.corrupt-{}", unix_ms()));
                    fs::rename(&path, &aside)?;
                    say(&format!(
                        "the state could not be read ({error}): kept as {} and begun again",
                        aside.display()
                    ));
                    // What install wrote before is not known now: each key
                    // path is recorded as once written, so whichever reader
                    // saves first, a round as much as install or prepare,
                    // keeps a key a reset removes from coming back from V1's
                    // config (the reviewer on #1263).
                    let mut state = State::default();
                    for path in crate::install::KEY_PATHS {
                        state
                            .settings
                            .insert(path.to_owned(), crate::install::SET_ASIDE.to_owned());
                    }
                    Ok(state)
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(State::default()),
            Err(error) => Err(error),
        }
    }

    /// The state, read only: for a look that changes nothing.
    pub fn peek_state(&self) -> State {
        fs::read(self.folder.join("state.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save_state(&self, state: &State) -> io::Result<()> {
        self.make()?;
        let bytes = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
        write_whole(&self.folder.join("state.json"), &bytes, 0o600)
    }

    /// One of its files that cannot be read, renamed `<name>.corrupt-<ms>`
    /// beside it, never deleted: where it went.
    pub fn set_aside(&self, name: &str) -> io::Result<PathBuf> {
        let aside = self.folder.join(format!("{name}.corrupt-{}", unix_ms()));
        fs::rename(self.folder.join(name), &aside)?;
        sync_folder(&self.folder)?;
        Ok(aside)
    }

    /// A switch under way, if any. One that cannot be read is
    /// `InvalidData`: the round sets it aside.
    pub fn journal(&self) -> io::Result<Option<Journal>> {
        match fs::read(self.folder.join("journal.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn save_journal(&self, journal: &Journal) -> io::Result<()> {
        self.make()?;
        let bytes = serde_json::to_vec_pretty(journal).map_err(io::Error::other)?;
        write_whole(&self.folder.join("journal.json"), &bytes, 0o600)
    }

    pub fn clear_journal(&self) -> io::Result<()> {
        match fs::remove_file(self.folder.join("journal.json")) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => sync_folder(&self.folder),
        }
    }

    /// The units as they are before a switch, replacing the ones kept.
    pub fn keep_previous(&self, units: &[(String, String)]) -> io::Result<()> {
        self.make()?;
        let folder = self.folder.join("previous");
        for entry in fs::read_dir(&folder)? {
            fs::remove_file(entry?.path())?;
        }
        for (unit, text) in units {
            write_whole(&folder.join(unit), text.as_bytes(), 0o600)?;
        }
        Ok(())
    }

    pub fn previous(&self, unit: &str) -> io::Result<String> {
        fs::read_to_string(self.folder.join("previous").join(unit))
    }

    /// Every unit kept from before the last switch.
    pub fn previous_units(&self) -> Vec<String> {
        fs::read_dir(self.folder.join("previous"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().ends_with(LINK_SUFFIX))
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .collect()
    }

    /// Every `current` link kept from before the last switch: its path and
    /// the build it named, from `previous/<unit>.link` (one line each).
    pub fn previous_links(&self) -> Vec<LinkBack> {
        fs::read_dir(self.folder.join("previous"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(LINK_SUFFIX))
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|text| {
                let (path, build) = text.trim().split_once(' ')?;
                Some(LinkBack {
                    path: path.to_owned(),
                    build: build.to_owned(),
                })
            })
            .collect()
    }

    /// One line in the log: what happened, and a sentence. Best effort: a
    /// full disk must not stop an update going back.
    pub fn log(&self, event: &str, line: &str) {
        self.log_entry(serde_json::json!({"at_unix_ms": unix_ms(), "event": event, "line": line}));
    }

    /// A round's end in the log, marked as one and with the build it was
    /// about: what `update/status` says as the last result, read back when
    /// lamp-update starts again (#2194, `crate::status`).
    pub fn log_round(&self, event: &str, line: &str, sequence: Option<u64>) {
        self.log_entry(serde_json::json!({
            "at_unix_ms": unix_ms(),
            "event": event,
            "line": line,
            "round": true,
            "sequence": sequence,
        }));
    }

    /// The last round's end the log holds, newest file first: its event,
    /// build and time. Lines a round did not write are passed over.
    pub fn last_round(&self) -> Option<(String, Option<u64>, u64)> {
        for name in ["log.jsonl", "log.jsonl.1"] {
            let Ok(text) = fs::read_to_string(self.folder.join(name)) else {
                continue;
            };
            let found = text.lines().rev().find_map(|line| {
                let entry: serde_json::Value = serde_json::from_str(line).ok()?;
                (entry["round"] == true).then(|| {
                    (
                        entry["event"].as_str().unwrap_or_default().to_owned(),
                        entry["sequence"].as_u64(),
                        entry["at_unix_ms"].as_u64().unwrap_or(0),
                    )
                })
            });
            if found.is_some() {
                return found;
            }
        }
        None
    }

    fn log_entry(&self, entry: serde_json::Value) {
        let _ = self.make();
        let path = self.folder.join("log.jsonl");
        if fs::metadata(&path).is_ok_and(|found| found.len() > MOST_LOG_BYTES) {
            let _ = fs::rename(&path, self.folder.join("log.jsonl.1"));
        }
        if let Ok(mut file) = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&path)
        {
            let _ = writeln!(file, "{entry}");
        }
    }
}

#[cfg(test)]
mod tests {
    /// The temp file beside is never opened through a link left there.
    #[test]
    fn a_link_left_beside_is_replaced_and_never_written_through() {
        let folder = tempfile::tempdir().unwrap();
        let outside = folder.path().join("outside");
        fs::write(&outside, "untouched").unwrap();
        let path = folder.path().join("state.json");
        std::os::unix::fs::symlink(&outside, folder.path().join(".state.json.lamp-update"))
            .unwrap();
        write_whole(&path, b"new", 0o600).unwrap();
        assert_eq!(fs::read_to_string(&outside).unwrap(), "untouched");
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert!(
            !fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    use super::*;

    /// A state written before `settings` was kept reads as one with none,
    /// and a state with none is written as before.
    #[test]
    fn a_state_from_before_the_settings_reads_and_writes_as_before() {
        let before =
            r#"{"installed":{"sequence":188,"commit":"a"},"previous":null,"refused":[],"made":[]}"#;
        let state: State = serde_json::from_str(before).unwrap();
        assert!(state.settings.is_empty());
        assert_eq!(state.installed.as_ref().unwrap().sequence, 188);
        assert_eq!(serde_json::to_string(&state).unwrap(), before);
        let mut kept = state;
        kept.settings
            .insert("/etc/lampOS/a.json".into(), "0".repeat(64));
        let again: State = serde_json::from_str(&serde_json::to_string(&kept).unwrap()).unwrap();
        assert_eq!(again, kept);
    }

    #[test]
    fn state_journal_and_previous_units_are_kept() {
        let folder = tempfile::tempdir().unwrap();
        let records = Records::new(folder.path().join("records"));
        let mut said = Vec::new();
        assert_eq!(
            records
                .state(&mut |line| said.push(line.to_owned()))
                .unwrap(),
            State::default()
        );

        let build = Build {
            sequence: 3,
            commit: "c".repeat(40),
        };
        let mut state = State {
            installed: Some(build.clone()),
            ..State::default()
        };
        for _ in 0..40 {
            state.refuse(&build, "why");
        }
        assert_eq!(state.refused.len(), MOST_REFUSED);
        records.save_state(&state).unwrap();
        assert_eq!(records.state(&mut |_| {}).unwrap(), state);
        assert_eq!(records.peek_state(), state);

        let journal = Journal {
            build,
            units: vec![
                Switching {
                    unit: "lamp-voice.service".into(),
                    was_active: true,
                    link: None,
                },
                Switching {
                    unit: "lamp-body.service".into(),
                    was_active: true,
                    link: Some(LinkBack {
                        path: "/opt/lampOS/body/current".into(),
                        build: "b".repeat(64),
                    }),
                },
            ],
            skills: vec![SkillBack {
                path: "/var/lib/lampOS/skills/dance/skill.toml".into(),
                before: None,
                wrote: "c".repeat(64),
                made: vec!["/var/lib/lampOS/skills/dance".into()],
            }],
            models: Vec::new(),
            settings: false,
        };
        records.save_journal(&journal).unwrap();
        assert_eq!(records.journal().unwrap(), Some(journal));
        records.clear_journal().unwrap();
        // A journal written before links were kept reads as one without a link.
        fs::write(
            records.folder().join("journal.json"),
            r#"{"build":{"sequence":7,"commit":"c"},"units":[{"unit":"lamp-voice.service","was_active":true}]}"#,
        )
        .unwrap();
        assert_eq!(records.journal().unwrap().unwrap().units[0].link, None);
        records.clear_journal().unwrap();
        assert_eq!(records.journal().unwrap(), None);

        records
            .keep_previous(&[("lamp-voice.service".into(), "one".into())])
            .unwrap();
        records
            .keep_previous(&[("lamp-body.service".into(), "two".into())])
            .unwrap();
        assert_eq!(records.previous_units(), vec!["two".to_owned()]);
        assert!(records.previous("lamp-voice.service").is_err());

        records.log("checked", "a line");
        let log = fs::read_to_string(records.folder().join("log.jsonl")).unwrap();
        assert!(log.contains("\"event\":\"checked\""));
    }

    /// #1505: a state a later build wrote, with a field this one does not
    /// know, reads, and keeps that field across a save.
    #[test]
    fn a_state_from_a_later_build_keeps_its_fields_across_a_save() {
        let folder = tempfile::tempdir().unwrap();
        let records = Records::new(folder.path().to_owned());
        fs::write(
            folder.path().join("state.json"),
            br#"{"installed":null,"previous":null,"refused":[],"made":[],"pinned":{"to":7}}"#,
        )
        .unwrap();
        let mut state = records.state(&mut |_| {}).unwrap();
        state.made.push("/opt/lampOS/voice/x/".into());
        records.save_state(&state).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(folder.path().join("state.json")).unwrap()).unwrap();
        assert_eq!(saved["pinned"], serde_json::json!({"to": 7}));
        assert_eq!(saved["made"][0], "/opt/lampOS/voice/x/");
    }

    #[test]
    fn a_state_that_cannot_be_read_is_kept_aside() {
        let folder = tempfile::tempdir().unwrap();
        let records = Records::new(folder.path().to_owned());
        fs::write(folder.path().join("state.json"), b"{not json").unwrap();
        let mut said = Vec::new();
        // Begun again, with only the key paths marked as once written (the
        // reviewer on #1263).
        let mut begun = State::default();
        for path in crate::install::KEY_PATHS {
            begun
                .settings
                .insert(path.to_owned(), crate::install::SET_ASIDE.to_owned());
        }
        assert_eq!(
            records
                .state(&mut |line| said.push(line.to_owned()))
                .unwrap(),
            begun
        );
        assert_eq!(said.len(), 1);
        assert!(fs::read_dir(folder.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("state.json.corrupt-")
        }));
    }

    #[test]
    fn one_at_a_time() {
        let folder = tempfile::tempdir().unwrap();
        let records = Records::new(folder.path().to_owned());
        let held = records.lock().unwrap();
        let other = Records::new(folder.path().to_owned());
        assert!(other.lock().is_err());
        drop(held);
        assert!(other.lock().is_ok());
    }
}
