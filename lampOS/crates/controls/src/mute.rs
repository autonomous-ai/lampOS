//! A software inhibit belongs to the input/output owner, not to a GPIO reader.
//! Disk I/O here is explicit and must run outside the owner's urgent stop path.
use lamp_msgs::MuteReason;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

const MAX_BYTES: u64 = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SavePoint {
    PendingWritten,
    ReplacementLinked,
    TargetRenamed,
    TargetSynced,
}

/// Durable software preference with a boot-scoped request watermark. Neither
/// an owner restart nor a new machine boot clears mute. Physical privacy is a
/// separate authority; this record never grants permission to open a worker.
/// On a lamp with no slide (`LAMP_PRIVACY_CONTROLS=no-slide`) it is the only
/// thing that closes the microphones, so the voice needs it there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MuteRecord {
    pub version: u32,
    pub clock_id: String,
    pub muted: bool,
    /// Mute requests at or before this time have already been considered.
    /// An explicit clear advances this watermark so retained requests do not
    /// undo it after an owner restart.
    pub through_us: u64,
    /// Why, while muted (#711): `sleep` when a hand held on the head asked
    /// for it, and a head tap may then clear it. Absent is the mute's
    /// original meaning, and a record without it reads as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<MuteReason>,
}

/// Whether `bytes` are a mute record of some build: a JSON object with a
/// whole-number `version` from 1 on. Anything else is corrupt, not newer.
fn from_a_build(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| value.get("version").and_then(serde_json::Value::as_u64))
        .is_some_and(|version| version >= 1)
}

fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

impl MuteRecord {
    pub fn new(clock_id: &str) -> io::Result<Self> {
        let out = Self {
            version: 1,
            clock_id: clock_id.to_owned(),
            muted: false,
            through_us: 0,
            reason: None,
        };
        out.check()?;
        Ok(out)
    }

    pub fn check(&self) -> io::Result<()> {
        if self.version != 1
            || self.clock_id.is_empty()
            || self.clock_id.len() > 128
            || self.clock_id.chars().any(char::is_control)
        {
            return Err(invalid("invalid mute record version or clock identity"));
        }
        if !self.muted && self.reason.is_some() {
            return Err(invalid("an unmuted record has no mute reason"));
        }
        Ok(())
    }

    /// A one-way request, with its original observation time. Call only after
    /// validating the Controls publisher, session and enclosing fresh state.
    /// A hold's mute is a person's privacy: it takes over a sleep, so a head
    /// tap can no longer clear it.
    pub fn request_mute(&mut self, at_us: u64) -> bool {
        if at_us <= self.through_us {
            return false;
        }
        self.through_us = at_us;
        self.muted = true;
        self.reason = None;
        true
    }

    /// Sleep (#711): the same one-way mute, with its reason. A lamp already
    /// muted keeps its mute and its reason, so sleep never weakens privacy.
    /// Call only after validating the head's hold.
    pub fn request_sleep(&mut self, at_us: u64) -> bool {
        if self.muted || at_us <= self.through_us {
            return false;
        }
        self.through_us = at_us;
        self.muted = true;
        self.reason = Some(MuteReason::Sleep);
        true
    }

    /// Muted because the lamp was put to sleep.
    pub fn asleep(&self) -> bool {
        self.muted && self.reason == Some(MuteReason::Sleep)
    }

    /// For a separate, explicit user action at the owner. This never grants
    /// capture or output permission, resets a privacy gate, or proves readiness.
    /// Persist the result before acknowledging the clear or opening anything.
    pub fn clear_after_explicit_user_request(&mut self, now_us: u64) -> io::Result<()> {
        if now_us < self.through_us {
            return Err(invalid(
                "cannot clear mute with a clock older than its watermark",
            ));
        }
        self.through_us = now_us;
        self.muted = false;
        self.reason = None;
        Ok(())
    }

    /// Load before starting capture/provider/output workers. Corruption and I/O
    /// failure are errors, never an unmuted default. Missing means first use.
    /// A record from a newer build reads as muted ([`Self::load_noting`]).
    ///
    /// Loading writes: it settles the markers a cut save leaves, removing or
    /// renaming them and, for an unfinished write, saving a muted record
    /// (`recover_markers`). So only the record's one owner may call it, at
    /// its start, while no save of its own is in flight; another process's
    /// save in flight would read as unfinished and be overwritten as muted.
    pub fn load(path: &Path, clock_id: &str, now_us: u64) -> io::Result<Self> {
        Self::load_noting(path, clock_id, now_us).map(|(record, _)| record)
    }

    /// [`Self::load`], which writes as it does (one owner, at its start), with
    /// a line to say when the record was written by a
    /// newer build (#1505, harm 1): a mute record with a `version` that this
    /// build cannot read, a field it does not know or a `reason` it does not
    /// know. Such a record never stops the voice starting and never reads as
    /// unmuted: it reads as muted for privacy, the mute only a fresh hold of
    /// the button or the base clears (a hold on the head asks for sleep,
    /// which a muted lamp ignores), whatever it said. What is not a mute record at all (not JSON,
    /// no `version`) is still an error.
    pub fn load_noting(
        path: &Path,
        clock_id: &str,
        now_us: u64,
    ) -> io::Result<(Self, Option<String>)> {
        if let Some((muted, note)) = Self::recover_markers(path, clock_id)? {
            return Ok((muted, Some(note)));
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
                    return Err(invalid("mute state must be a private regular file"));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok((Self::new(clock_id)?, None));
            }
            Err(error) => return Err(error),
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("mute state exceeds 4096 bytes"));
        }
        let state = match serde_json::from_slice::<Self>(&bytes) {
            Ok(state) if state.version <= 1 => state,
            read => {
                let why = match read {
                    Ok(state) => format!("version {}", state.version),
                    Err(error) => error.to_string(),
                };
                if !from_a_build(&bytes) {
                    return Err(invalid(why));
                }
                let mut muted = Self::new(clock_id)?;
                muted.muted = true;
                return Ok((
                    muted,
                    Some(format!(
                        "the mute record at {} was written by a newer build, or is one this build cannot read ({why}): read as muted, until a fresh hold of the button or the base clears it",
                        path.display()
                    )),
                ));
            }
        };
        state.check()?;
        if state.clock_id != clock_id {
            // Monotonic timestamps cannot cross machine boots, but the user's
            // preference does. An old boot's watermark must not block a fresh
            // local hold, and rebasing must never turn a saved mute into clear.
            let mut rebased = Self::new(clock_id)?;
            rebased.muted = state.muted;
            rebased.reason = state.reason;
            return Ok((rebased, None));
        }
        if state.through_us > now_us {
            return Err(invalid("mute state watermark is in the future"));
        }
        Ok((state, None))
    }

    /// Settle the `.pending` and `.replacement` markers a save leaves while it
    /// runs, before the record is read. None: no marker, or a finished write.
    /// Some(note): an unfinished one, which the caller reads as muted.
    ///
    /// A marker sharing the record's own inode (`dev` and `ino`) is a
    /// finished write: the record is the save's synced new file, and only the
    /// last, unsynced unlink was lost, as a power cut a few seconds after a
    /// save does (lamp-06c1, 29 Oct). The markers are removed and the record
    /// read as usual.
    ///
    /// Any other marker means the write may not have finished, and which
    /// state won is unknown, so the record reads as muted, said once. It used
    /// to refuse the start until an operator came; a voice that never starts
    /// again after an unplug is worse than a mute a hold clears (#4332). The
    /// markers are moved aside to `.recovered` names, so the next save, the
    /// person's hold that clears the mute among them, can claim its fixed
    /// sibling names again. Only the latest is kept on purpose: a second cut's
    /// markers overwrite the first's `.recovered` evidence, so a lamp cut many
    /// times holds one pair, not a growing pile in a private state folder.
    ///
    /// The muted record is made durable before the markers move: it is
    /// written to `.<name>.recovering`, synced, renamed over the record and
    /// the directory synced. Only then are the markers set aside. A second
    /// cut before that rename leaves the markers, so the next load reads
    /// muted again; a cut after it leaves a muted record. Moving the markers
    /// first would leave, after a second cut, the old record with no marker,
    /// unmuted if the cut write was the person's mute (Reviewer-two on #4378).
    fn recover_markers(path: &Path, clock_id: &str) -> io::Result<Option<(Self, String)>> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Ok(None);
        };
        let name = name.to_string_lossy();
        // A `.recovering` is only ever this function's own muted copy, never
        // a record anyone chose. One left behind means a cut before its
        // rename landed, and then the markers it guards are still in place
        // (they move only after the rename is synced), so the muted branch
        // below runs again and writes it afresh. Removing it loses nothing,
        // and it would otherwise refuse the `create_new` below.
        let recovering = parent.join(format!(".{name}.recovering"));
        match fs::remove_file(&recovering) {
            Ok(()) => File::open(parent)?.sync_all()?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut markers = Vec::new();
        for suffix in ["pending", "replacement"] {
            let marker = parent.join(format!(".{name}.{suffix}"));
            match fs::symlink_metadata(&marker) {
                Ok(metadata) => markers.push((marker, metadata)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        if markers.is_empty() {
            return Ok(None);
        }
        let record = match fs::symlink_metadata(path) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let finished = record.as_ref().is_some_and(|record| {
            record.is_file()
                && markers
                    .iter()
                    .all(|(_, marker)| marker.dev() == record.dev() && marker.ino() == record.ino())
        });
        let directory = File::open(parent)?;
        if finished {
            for (marker, _) in &markers {
                fs::remove_file(marker)?;
            }
            directory.sync_all()?;
            return Ok(None);
        }
        let mut muted = Self::new(clock_id)?;
        muted.muted = true;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&recovering)?;
        serde_json::to_writer(&mut file, &muted)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&recovering, path)?;
        directory.sync_all()?;
        let mut kept = Vec::new();
        for (marker, _) in &markers {
            let aside = marker.with_file_name(format!(
                "{}.recovered",
                marker.file_name().unwrap().to_string_lossy()
            ));
            fs::rename(marker, &aside)?;
            kept.push(aside.display().to_string());
        }
        directory.sync_all()?;
        let note = format!(
            "the mute record at {} had an unfinished write: read as muted, until a fresh hold of the button or the base clears it (kept as {})",
            path.display(),
            kept.join(", ")
        );
        Ok(Some((muted, note)))
    }

    /// Commit a private replacement while retaining a recovery marker until
    /// the target and its directory are synced. The owner supplies a trusted
    /// local directory and serializes access. Failures retain live inhibition.
    /// Marker removal is the final operation: a crash may resurrect that
    /// marker, and the next load settles it (`recover_markers`): its own
    /// inode reads as this finished write, anything else as muted.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        self.save_with(path, |_| Ok(()))
    }

    fn save_with(
        &self,
        path: &Path,
        mut after: impl FnMut(SavePoint) -> io::Result<()>,
    ) -> io::Result<()> {
        self.check()?;
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(invalid("mute state needs an absolute file path"));
        }
        let parent = path.parent().ok_or_else(|| invalid("missing parent"))?;
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(invalid("mute state directory must be private (0700)"));
        }
        if let Ok(metadata) = fs::symlink_metadata(path)
            && !metadata.is_file()
        {
            return Err(invalid("mute state target is not a regular file"));
        }
        // A fixed sibling deliberately refuses another outstanding writer;
        // never overwrite or follow a pre-existing temporary path.
        let temp = parent.join(format!(
            ".{}.pending",
            path.file_name().unwrap().to_string_lossy()
        ));
        let replacement = parent.join(format!(
            ".{}.replacement",
            path.file_name().unwrap().to_string_lossy()
        ));
        match fs::symlink_metadata(&replacement) {
            Ok(_) => return Err(invalid("an interrupted mute replacement requires recovery")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let directory = File::open(parent)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        serde_json::to_writer(&mut file, self)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        after(SavePoint::PendingWritten)?;
        directory.sync_all()?;
        // The original inode stays at .pending even after replacement rename,
        // so the next load can tell a finished write (.pending is the record's
        // own inode) from an unfinished one (any other). A rename that lands
        // although the directory sync after it fails returns an error here,
        // and the live latch keeps its inhibition, but a restarted owner reads
        // the renamed record as that finished write. All names share one
        // directory.
        fs::hard_link(&temp, &replacement)?;
        after(SavePoint::ReplacementLinked)?;
        fs::rename(&replacement, path)?;
        after(SavePoint::TargetRenamed)?;
        directory.sync_all()?;
        after(SavePoint::TargetSynced)?;
        // The target is already durable. The unlink's own sync is best
        // effort: an error is ignored, so the save still succeeds. It matters
        // because the lamp images mount the root ext4 with `commit=600`, and an
        // unsynced unlink can stay unjournaled for up to ten minutes, until
        // some other fsync commits it; lamp-06c1's unplug about 4 minutes after
        // a quiet save brought the marker back (29 Oct). If it is lost anyway,
        // the next load reads the resurrected marker as this finished write
        // (`recover_markers`), and the markers it moves aside keep only the
        // latest as evidence.
        fs::remove_file(&temp)?;
        let _ = directory.sync_all();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Root(PathBuf);

    impl Root {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "controls-save-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            Self(root)
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// #1505: a record as a later build writes it, with a field this build
    /// doesn't know, is read, as muted, and the voice starts on it (Tests'
    /// forward-read test, made plain by `el/mute-forward`).
    #[test]
    fn a_record_from_a_later_build_is_read_and_stays_muted() {
        let root = Root::new();
        let path = root.0.join("mute.json");
        let mut record = MuteRecord::new("boot").unwrap();
        record.request_mute(10);
        record.save(&path).unwrap();
        let mut later: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        later["a_field_from_a_later_build"] = serde_json::json!(true);
        fs::write(&path, serde_json::to_vec(&later).unwrap()).unwrap();
        let read = MuteRecord::load(&path, "boot", 30).unwrap();
        assert!(read.muted);
    }

    /// A clear cut at each point of its save: the next load never fails, and
    /// a save after it works, so the voice starts and a person's hold can
    /// still clear the mute. Cut before the rename, the old muted record
    /// stands and the start is muted; cut after it, the record is the
    /// finished clear and the markers are its own inode (#4332, lamp-06c1).
    #[test]
    fn a_save_cut_at_any_point_never_stops_the_next_start() {
        for point in [
            SavePoint::PendingWritten,
            SavePoint::ReplacementLinked,
            SavePoint::TargetRenamed,
            SavePoint::TargetSynced,
        ] {
            let root = Root::new();
            let path = root.0.join("mute.json");
            let mut record = MuteRecord::new("boot").unwrap();
            record.request_mute(10);
            record.save(&path).unwrap();
            record.clear_after_explicit_user_request(20).unwrap();
            assert!(
                record
                    .save_with(&path, |at| {
                        if at == point {
                            Err(io::Error::other("injected persistence failure"))
                        } else {
                            Ok(())
                        }
                    })
                    .is_err()
            );
            assert!(root.0.join(".mute.json.pending").exists());
            let finished = matches!(point, SavePoint::TargetRenamed | SavePoint::TargetSynced);
            let (mut loaded, note) = MuteRecord::load_noting(&path, "boot", 30).unwrap();
            assert_eq!(loaded.muted, !finished);
            assert_eq!(note.is_some(), !finished);
            assert!(!root.0.join(".mute.json.pending").exists());
            assert!(!root.0.join(".mute.json.replacement").exists());
            assert_eq!(
                root.0.join(".mute.json.pending.recovered").exists(),
                !finished
            );
            loaded.save(&path).unwrap();
            loaded.clear_after_explicit_user_request(40).unwrap();
            loaded.save(&path).unwrap();
            assert!(!MuteRecord::load(&path, "boot", 50).unwrap().muted);
        }
    }

    /// The lamp-06c1 files of 29 Oct: the record and `.pending` one inode,
    /// with `.replacement` too on another cut, read as the record they are.
    #[test]
    fn a_finished_write_with_its_marker_left_reads_as_the_record() {
        for with_replacement in [false, true] {
            let root = Root::new();
            let path = root.0.join("mute.json");
            let mut record = MuteRecord::new("boot").unwrap();
            record.request_mute(10);
            record.save(&path).unwrap();
            fs::hard_link(&path, root.0.join(".mute.json.pending")).unwrap();
            if with_replacement {
                fs::hard_link(&path, root.0.join(".mute.json.replacement")).unwrap();
            }
            let (loaded, note) = MuteRecord::load_noting(&path, "boot", 30).unwrap();
            assert_eq!(loaded, record);
            assert!(note.is_none());
            for leftover in [".mute.json.pending", ".mute.json.replacement"] {
                assert!(!root.0.join(leftover).exists());
                assert!(!root.0.join(format!("{leftover}.recovered")).exists());
            }
            record.clear_after_explicit_user_request(20).unwrap();
            record.save(&path).unwrap();
        }
    }

    /// Reviewer-two's probe on #4378: a person's mute cut before its rename,
    /// then a second cut before the voice's own save. The first load already
    /// made the muted record durable, so the second still reads muted, never
    /// the old unmuted record with its markers gone.
    #[test]
    fn a_second_cut_before_the_voices_save_still_reads_muted() {
        let root = Root::new();
        let path = root.0.join("mute.json");
        let mut record = MuteRecord::new("boot").unwrap();
        record.save(&path).unwrap();
        record.request_mute(10);
        assert!(
            record
                .save_with(&path, |at| {
                    if at == SavePoint::PendingWritten {
                        Err(io::Error::other("injected cut"))
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        let (first, note) = MuteRecord::load_noting(&path, "boot", 30).unwrap();
        assert!(first.muted);
        assert!(note.is_some());
        // No save between: the next boot's load.
        let (second, note) = MuteRecord::load_noting(&path, "boot", 40).unwrap();
        assert!(second.muted);
        assert!(note.is_none());
        assert!(!root.0.join(".mute.json.recovering").exists());
    }

    /// A cut between writing `.recovering` and its rename leaves it beside
    /// the markers it guards: the next load reads muted again and the
    /// leftover never stops it. Alone, beside a plain record, it is dropped
    /// and the record read as it is.
    #[test]
    fn a_leftover_recovering_copy_never_stops_a_load_or_a_save() {
        let root = Root::new();
        let path = root.0.join("mute.json");
        let mut record = MuteRecord::new("boot").unwrap();
        record.save(&path).unwrap();
        fs::write(root.0.join(".mute.json.pending"), "cut").unwrap();
        fs::write(root.0.join(".mute.json.recovering"), "half").unwrap();
        let (loaded, note) = MuteRecord::load_noting(&path, "boot", 30).unwrap();
        assert!(loaded.muted);
        assert!(note.is_some());
        assert!(!root.0.join(".mute.json.recovering").exists());
        assert!(MuteRecord::load(&path, "boot", 31).unwrap().muted);

        record.clear_after_explicit_user_request(40).unwrap();
        record.save(&path).unwrap();
        fs::write(root.0.join(".mute.json.recovering"), "half").unwrap();
        let (loaded, note) = MuteRecord::load_noting(&path, "boot", 50).unwrap();
        assert!(!loaded.muted);
        assert!(note.is_none());
        assert!(!root.0.join(".mute.json.recovering").exists());
        record.save(&path).unwrap();
    }

    /// The first save cut before its rename: a `.pending` and no record yet.
    /// Which state it was writing is unknown, so the start is muted, never
    /// the first use's unmuted default.
    #[test]
    fn a_pending_marker_with_no_record_reads_as_muted() {
        let root = Root::new();
        let path = root.0.join("mute.json");
        fs::write(root.0.join(".mute.json.pending"), "cut").unwrap();
        let (loaded, note) = MuteRecord::load_noting(&path, "boot", 30).unwrap();
        assert!(loaded.muted);
        assert!(note.unwrap().contains("unfinished write"));
        loaded.save(&path).unwrap();
        assert!(MuteRecord::load(&path, "boot", 40).unwrap().muted);
    }
}
