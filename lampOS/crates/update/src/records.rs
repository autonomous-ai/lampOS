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
    collections::{BTreeMap, BTreeSet},
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
    /// A unit the switch adds (#3689): on the way back it is stopped and
    /// removed, with what it added. Absent from a journal written before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<Added>,
}

/// What a switch adds with a unit the lamp did not have, as the lamp names
/// each: all of it removed on the way back.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Added {
    /// The links that enable it, as `systemctl enable` writes them:
    /// `/etc/systemd/system/lampos.target.wants/lamp-x.service`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wants: Vec<String>,
    /// The `current` link made for a part new to the lamp:
    /// `/opt/lampOS/x/current`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
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
    /// The validated old startup order, projected onto every changed unit,
    /// including inactive ones. Absent in older journals; an empty list is
    /// present only when no units change. Saved before any switching write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_order: Option<Vec<String>>,
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

impl Journal {
    /// A saved order must cover the full journal exactly once, before
    /// recovery excludes units already restored. It is never guessed from
    /// a malformed list, nor replaced with the current installed graph.
    fn validate_rollback_order(&self) -> io::Result<()> {
        let Some(order) = &self.rollback_order else {
            return Ok(());
        };
        let units: BTreeSet<&str> = self.units.iter().map(|unit| unit.unit.as_str()).collect();
        let saved: BTreeSet<&str> = order.iter().map(String::as_str).collect();
        if units.len() != self.units.len()
            || saved.len() != order.len()
            || saved != units
            || units.iter().any(|name| {
                !(crate::manifest::is_unit_name(name) || crate::companion::kind(name).is_some())
                    || *name == crate::round::OWN_UNIT
            })
        {
            return Err(io::Error::other(
                "the saved rollback order must name every changed managed unit exactly once",
            ));
        }
        Ok(())
    }
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

    /// A switch under way, if any. Unreadable legacy JSON is `InvalidData`
    /// and set aside. A present but invalid saved order is a recovery
    /// failure instead: the journal and restoration prefix must stay put.
    pub fn journal(&self) -> io::Result<Option<Journal>> {
        match fs::read(self.folder.join("journal.json")) {
            Ok(bytes) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                let has_order = value.get("rollback_order").is_some();
                if has_order && !value["rollback_order"].is_array() {
                    return Err(io::Error::other("the saved rollback order is not an array"));
                }
                // Decode the original bytes, not Value: duplicate order
                // properties must be refused rather than keeping the last.
                let journal: Journal = serde_json::from_slice(&bytes).map_err(|error| {
                    if has_order {
                        io::Error::other(error)
                    } else {
                        io::Error::new(io::ErrorKind::InvalidData, error)
                    }
                })?;
                journal.validate_rollback_order()?;
                Ok(Some(journal))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn save_journal(&self, journal: &Journal) -> io::Result<()> {
        journal.validate_rollback_order()?;
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
            rollback_order: Some(vec![
                "lamp-body.service".into(),
                "lamp-voice.service".into(),
            ]),
            units: vec![
                Switching {
                    unit: "lamp-voice.service".into(),
                    was_active: true,
                    link: None,
                    added: None,
                },
                Switching {
                    unit: "lamp-body.service".into(),
                    was_active: true,
                    link: Some(LinkBack {
                        path: "/opt/lampOS/body/current".into(),
                        build: "b".repeat(64),
                    }),
                    added: None,
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

    /// lamp-update's own records as today's build writes them: every field
    /// of the state and of a switch under way filled in.
    fn todays_records() -> (super::State, super::Journal) {
        use super::{Build, Journal, LinkBack, Refusal, SkillBack, State, Switching};
        use crate::models::ModelBack;
        let state = State {
            installed: Some(Build {
                sequence: 4576,
                commit: "4f5e62507".into(),
            }),
            previous: Some(Build {
                sequence: 4570,
                commit: "d90cc8b8e".into(),
            }),
            refused: vec![Refusal {
                sequence: 4573,
                commit: "c902a95cf".into(),
                why: "lamp-voice.service was not well within a minute".into(),
                at_unix_ms: 1_760_000_000_000,
            }],
            made: vec!["/opt/lampOS/voice/".to_owned() + &"a".repeat(64)],
            settings: [("/etc/lampOS/voice/second.json".to_owned(), "b".repeat(64))].into(),
            settings_writing: [("/etc/lampOS/records.json".to_owned(), "c".repeat(64))].into(),
            systemd_owed: true,
            models: [(
                "/opt/lampOS/eyes/models/face.onnx".to_owned(),
                "d".repeat(64),
            )]
            .into(),
            models_before: [(
                "/opt/lampOS/eyes/models/face.onnx".to_owned(),
                "e".repeat(64),
            )]
            .into(),
            later: Default::default(),
        };
        let journal = Journal {
            build: Build {
                sequence: 4576,
                commit: "4f5e62507".into(),
            },
            rollback_order: Some(vec!["lamp-voice.service".into()]),
            units: vec![Switching {
                unit: "lamp-voice.service".into(),
                was_active: true,
                link: Some(LinkBack {
                    path: "/opt/lampOS/voice/current".into(),
                    build: "f".repeat(64),
                }),
                added: None,
            }],
            models: vec![ModelBack {
                path: "/opt/lampOS/eyes/models/face.onnx".into(),
                sha256: Some("e".repeat(64)),
                file: false,
            }],
            settings: true,
            skills: vec![SkillBack {
                path: "/var/lib/lampOS/skills/dance/skill.toml".into(),
                before: Some("name = \"dance\"\n".into()),
                wrote: "9".repeat(64),
                made: Vec::new(),
            }],
        };
        (state, journal)
    }

    /// Writes [`todays_records`] as this build saves them into the folder in
    /// `LAMP_UPDATE_RECORDS_FIXTURE`, as `state.json` and `journal.json`.
    /// Run by hand when either's shape changes, keeping the ones kept.
    #[test]
    #[ignore = "writes records fixtures: run by hand with LAMP_UPDATE_RECORDS_FIXTURE set"]
    fn write_records_fixtures() {
        let folder =
            std::env::var_os("LAMP_UPDATE_RECORDS_FIXTURE").expect("LAMP_UPDATE_RECORDS_FIXTURE");
        let records = super::Records::new(folder.into());
        let (state, journal) = todays_records();
        records.save_state(&state).unwrap();
        records.save_journal(&journal).unwrap();
    }

    /// The first thing a kept record holds that `read` doesn't: a key of an
    /// object missing or different, by its path, an array of another length, or
    /// another value. A key `read` has beyond the kept one is a later build's
    /// addition, and passes: a kept day's test is never edited (#1505; the
    /// reviewer on #2486).
    fn missing(kept: &serde_json::Value, read: &serde_json::Value, path: &str) -> Option<String> {
        use serde_json::Value;
        match (kept, read) {
            (Value::Object(kept), Value::Object(read)) => kept.iter().find_map(|(key, value)| {
                let path = format!("{path}.{key}");
                match read.get(key) {
                    Some(got) => missing(value, got, &path),
                    None => Some(format!("{path}: not read")),
                }
            }),
            (Value::Array(kept), Value::Array(read)) if kept.len() == read.len() => kept
                .iter()
                .zip(read)
                .enumerate()
                .find_map(|(i, (value, got))| missing(value, got, &format!("{path}[{i}]"))),
            _ if kept == read => None,
            _ => Some(format!("{path}: kept {kept}, read {read}")),
        }
    }

    /// lamp-update's records kept as fixtures, each day's as its build saved
    /// them ([`write_records_fixtures`]): every later lamp-update reads each
    /// with every field as it was, so an update never loses what the one
    /// before it knew of the lamp, and a switch it began can still be put
    /// back (the build row "Updates cover the whole lamp, V3 to V5").
    #[test]
    fn each_kept_state_and_journal_reads_as_saved() {
        let kept: [(&str, &[u8], &[u8]); 2] = [
            (
                "2026-10-21",
                include_bytes!("../tests/fixtures/stores/state-2026-10-21.json"),
                include_bytes!("../tests/fixtures/stores/journal-2026-10-21.json"),
            ),
            (
                "2026-10-23",
                include_bytes!("../tests/fixtures/stores/state-2026-10-23.json"),
                include_bytes!("../tests/fixtures/stores/journal-2026-10-23.json"),
            ),
        ];
        for (day, state_bytes, journal_bytes) in kept {
            let folder = tempfile::tempdir().unwrap();
            std::fs::write(folder.path().join("state.json"), state_bytes).unwrap();
            std::fs::write(folder.path().join("journal.json"), journal_bytes).unwrap();
            let records = super::Records::new(folder.path().to_owned());
            let mut said = Vec::new();
            let read = records
                .state(&mut |line: &str| said.push(line.to_owned()))
                .unwrap_or_else(|error| panic!("{day}: {error}"));
            assert!(said.is_empty(), "{day}: {said:?}");
            assert_eq!(
                missing(
                    &serde_json::from_slice(state_bytes).unwrap(),
                    &serde_json::to_value(&read).unwrap(),
                    "state",
                ),
                None,
                "{day}"
            );
            let under_way = records
                .journal()
                .unwrap_or_else(|error| panic!("{day}: {error}"))
                .unwrap_or_else(|| panic!("{day}: no switch under way"));
            assert_eq!(
                missing(
                    &serde_json::from_slice(journal_bytes).unwrap(),
                    &serde_json::to_value(&under_way).unwrap(),
                    "journal",
                ),
                None,
                "{day}"
            );
        }
    }
}
