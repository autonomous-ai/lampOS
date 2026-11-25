//! The yeses another part reads (#5500, `docs/onboarding.md`, "Face
//! expressions" and "Room sounds"): face expressions for lamp-eyes, on a
//! lamp with no privacy slide room sounds for lamp-ears, and with
//! `LAMP_SEEN_ACTIVITY=on` the owner's yes to seen activity for lamp-seen
//! (#6543), given by voice at any time, not at onboarding. Each is a file in
//! [`FOLDER`], present only while the yes stands:
//! `{"version":1,KEY:"yes","at_unix_s":N,"by":BY,"rev":REV}`, `root:lampos`
//! 0640. Every change to one is made holding the folder ([`hold`]), a fresh
//! revision kept first in [`STATE_FILE`], so the app's `consent.set` is a
//! compare-and-set on [`rev`].
//!
//! The voice writes one on a spoken yes (`"by":"voice"`), and
//! root's helper (`lamp-setup change`) on the owner's switch in the app
//! (`"by":"app"`, `consent.set` with `LAMP_APP_CONSENT=on`). One copy of the
//! names and the file's shape, read by `lamp-cloud`, the helper and the
//! voice. The readers (`lamp_eyes::seems`, `lamp_ears::classes`,
//! `lamp_seen::opt_in`, [`crate::Faces::for_skills`]) read only `KEY`, all
//! with one rule, [`allowed`] (#7767): a kept `"yes"` is on, a kept `"no"`
//! is off, a missing file is on only with [`NOTICING_BY_DEFAULT`] (read in
//! one place, [`noticing_by_default`]) once [`OLD_NOS_DONE`] is beside it,
//! and a file that can't be read is off. A file from before `by` is the
//! voice's.
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The switch on `lamp-cloud`: `on` exactly takes [`KINDS`] from the app.
pub const ENVIRONMENT: &str = "LAMP_APP_CONSENT";

/// The app's kinds: `consent.get` (`{}`) and `consent.set` (one key and
/// `if`, the revision the app last saw, [`rev`]).
pub const KINDS: [&str; 2] = ["consent.get", "consent.set"];

/// The folder both yeses are kept in (`deploy/product/lampos.conf`: 0750
/// `root:lampos`).
pub const FOLDER: &str = "/var/lib/lampOS/consent";

/// The face expressions yes's key, in the file and on the wire.
pub const FACE_EXPRESSIONS: &str = "face_expressions";
/// The room sounds yes's key, in the file and on the wire.
pub const ROOM_SOUNDS: &str = "room_sounds";

/// The owner's yes to seen activity's key (#6543), in the file and on the
/// wire: lamp-seen may look, now and then, at what they do at the desk.
pub const SEEN_ACTIVITY: &str = "seen_activity";

/// The keys, each with its file's name in [`FOLDER`].
pub const FILES: [(&str, &str); 3] = [
    (FACE_EXPRESSIONS, "face-expressions.json"),
    (ROOM_SOUNDS, "room-sounds.json"),
    (SEEN_ACTIVITY, "seen-activity.json"),
];

/// The file beside the yeses where each change to a yes ([`Held`], by root's
/// helper, the voice or the reset) keeps a fresh revision for its key
/// (`{"version":1,"revs":{KEY:{"wrote":REV}}}`), so `consent.set` applies
/// only against the revision the app last saw ([`rev`]). No key's file: no
/// reader takes it for a yes.
pub const STATE_FILE: &str = "app-revs.json";

/// The largest state file read.
pub const STATE_MOST_BYTES: u64 = 4096;

/// The longest revision, in bytes.
pub const REV_MOST: usize = 64;

/// A key's revision while no yes stands and the state is missing, can't be
/// read or names no revision for it, as `lamp-cloud` reads it: never equal
/// to what a holder finds, since [`Held::revs`] first puts a fresh boundary
/// in its place. There is no revision for "never changed": a root writer
/// at start (the voice's storage, the change helper) puts the boundary in
/// place, so a lamp's first `consent.get` gives a real one.
pub const REV_UNKNOWN: &str = "unknown";

/// Before the last revision of a key whose file holds no yes now
/// (`gone-REV`): taken back, or a yes that could not be written after its
/// state.
pub const GONE: &str = "gone-";

/// Before the time of a yes kept with no revision (`yes-AT_UNIX_S`): a yes
/// file from a build before revisions, the voice's or the app's.
pub const YES_UNREVISED: &str = "yes-";

/// Why a set's `if` is refused. Never holds what was sent.
pub const REV_REFUSED: &str =
    "consent.set takes if: the rev from the last consent.get or consent.set";

/// A revision's text: 1 to [`REV_MOST`] printable ASCII characters (`!` to
/// `~`).
fn fits(rev: &str) -> bool {
    (1..=REV_MOST).contains(&rev.len()) && rev.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// `consent.set`'s `if`, checked: the revision the app last saw for the
/// key ([`rev`]), as [`fits`] says. One copy of the rule, for `lamp-cloud`
/// and the helper.
pub fn fit_rev(rev: Option<&Value>) -> Result<&str, &'static str> {
    rev.and_then(Value::as_str)
        .filter(|rev| fits(rev))
        .ok_or(REV_REFUSED)
}

/// The helper's state file ([`STATE_FILE`]), read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Revs {
    /// Each key's revision kept last (a fresh boundary names every key).
    Kept(BTreeMap<String, String>),
    /// No state file.
    Missing,
    /// There but unreadable, over [`STATE_MOST_BYTES`] or not its shape.
    Unreadable,
}

impl Revs {
    /// The state file in `folder`.
    pub fn read(folder: &Path) -> Self {
        let file = match std::fs::File::open(folder.join(STATE_FILE)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Self::Missing;
            }
            Err(_) => return Self::Unreadable,
        };
        let mut text = Vec::new();
        if file
            .take(STATE_MOST_BYTES + 1)
            .read_to_end(&mut text)
            .is_err()
            || text.len() as u64 > STATE_MOST_BYTES
        {
            return Self::Unreadable;
        }
        let Ok(Value::Object(value)) = serde_json::from_slice::<Value>(&text) else {
            return Self::Unreadable;
        };
        let Some(Value::Object(revs)) = value.get("revs").filter(|_| value["version"] == 1) else {
            return Self::Unreadable;
        };
        let mut kept = BTreeMap::new();
        for (key, entry) in revs {
            match entry
                .get("wrote")
                .and_then(Value::as_str)
                .filter(|rev| fits(rev))
            {
                Some(rev) => kept.insert(key.clone(), rev.to_owned()),
                None => return Self::Unreadable,
            };
        }
        Self::Kept(kept)
    }

    /// The revision the helper wrote last for `key`, if any.
    pub fn wrote(&self, key: &str) -> Option<&str> {
        match self {
            Self::Kept(kept) => kept.get(key).map(String::as_str),
            Self::Missing | Self::Unreadable => None,
        }
    }
}

/// The state file's bytes, whole: `{"version":1,"revs":{KEY:{"wrote":REV}}}`.
pub fn state_bytes(wrote: &BTreeMap<String, String>) -> Vec<u8> {
    let revs: serde_json::Map<String, Value> = wrote
        .iter()
        .map(|(key, rev)| (key.clone(), serde_json::json!({ "wrote": rev })))
        .collect();
    serde_json::to_vec(&serde_json::json!({"version": 1, "revs": revs})).unwrap_or_default()
}

/// `key`'s revision now, in `folder` with `revs`, as `consent.get` says it
/// and the helper compares a set's `if` to it:
/// - its file holds a yes with `"rev": R` (written by the helper): `R`;
/// - a yes with no revision (from a build before them): `yes-AT_UNIX_S`
///   (`0` with no time);
/// - no yes, and the last change (or the boundary) kept `R`: `gone-R`;
/// - no yes, and the state is missing, can't be read or names no
///   revision for the key: `unknown`, which no set matches.
pub fn rev(folder: &Path, key: &str, revs: &Revs) -> String {
    let kept = file_in(folder, key)
        .map(|file| read(&file, key))
        .unwrap_or_default();
    rev_of(&kept, key, revs)
}

/// [`rev`], for `key`'s file already read as `kept`.
pub fn rev_of(kept: &Kept, key: &str, revs: &Revs) -> String {
    // A yes nobody kept (no file, read as on by default, #7767) is no
    // file's yes: its revision is the state's, as for any key with none.
    if kept.yes && !kept.by_default {
        return match kept.rev.clone().filter(|rev| fits(rev)) {
            Some(rev) => rev,
            None => format!("{YES_UNREVISED}{}", kept.at_unix_s.unwrap_or(0)),
        };
    }
    match revs.wrote(key) {
        Some(wrote) => format!("{GONE}{wrote}"),
        None => REV_UNKNOWN.into(),
    }
}

/// How many times [`snapshot`] reads before it gives up on a state that
/// keeps changing.
pub const SNAPSHOT_TRIES: usize = 5;

/// Each of `keys` as the lamp keeps it now, its yes and its revision read
/// together, for a reader that holds nothing (`lamp-cloud`): the state is
/// read before and after the files, and all read again while it changed
/// in between. Every change keeps a fresh revision in the state before it
/// touches a file ([`Held`]), so a yes and a revision from one unchanged
/// state go together; one that never settles is said `unknown`, which no
/// set matches. The yes is [`read_or`]'s with `by_default` (a missing file
/// is on with [`NOTICING_BY_DEFAULT`] once the old nos are kept, #7767);
/// the revision is the same either way.
pub fn snapshot(folder: &Path, keys: &[&str], by_default: bool) -> Vec<(String, bool, String)> {
    let mut said = Vec::new();
    for _ in 0..SNAPSHOT_TRIES {
        let revs = Revs::read(folder);
        said = keys
            .iter()
            .map(|key| {
                let kept = file_in(folder, key)
                    .map(|file| read_or(&file, key, by_default))
                    .unwrap_or_default();
                ((*key).to_owned(), kept.yes, rev_of(&kept, key, &revs))
            })
            .collect();
        if Revs::read(folder) == revs {
            return said;
        }
    }
    for (_, _, rev) in &mut said {
        *rev = REV_UNKNOWN.into();
    }
    said
}

/// A fresh revision: 128 bits from the OS (`/dev/urandom`), as 32 hex
/// characters.
pub fn fresh_rev() -> std::io::Result<String> {
    let mut bits = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bits)?;
    Ok(bits.iter().map(|b| format!("{b:02x}")).collect())
}

/// The consent folder held (`flock`, exclusive, on the folder itself) while
/// a change to a yes is made, given back when dropped. Every change the lamp
/// makes to a yes file is made holding it, through [`Held::keep_yes`] or
/// [`Held::take_back`], so each writes a fresh revision into the state
/// first: a revision the app saw before any change never matches after it,
/// even when the key reads as it did (a yes and a "no" at the lamp between).
#[derive(Debug)]
pub struct Held {
    folder: PathBuf,
    _lock: std::fs::File,
}

/// [`Held`] on `folder`, waiting for whoever holds it.
pub fn hold(folder: &Path) -> std::io::Result<Held> {
    let lock = std::fs::File::open(folder)?;
    lock.lock()?;
    Ok(Held {
        folder: folder.to_path_buf(),
        _lock: lock,
    })
}

impl Held {
    /// The folder held.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The state, read; one that is missing, can't be read or names no
    /// revision for a key ([`FILES`]) is put right at once, durably, before
    /// anything is compared or said: a fresh revision for every key it
    /// lacks (every key, when missing or unreadable). So every revision
    /// handed out before reads stale (it fails closed), and the next read
    /// hands out a new one. A state that can't be put right fails.
    pub fn revs(&self) -> std::io::Result<BTreeMap<String, String>> {
        let mut kept = match Revs::read(&self.folder) {
            Revs::Kept(kept) => kept,
            Revs::Missing | Revs::Unreadable => BTreeMap::new(),
        };
        if FILES.iter().all(|(key, _)| kept.contains_key(*key)) {
            return Ok(kept);
        }
        for (key, _) in FILES {
            if !kept.contains_key(key) {
                kept.insert(key.to_owned(), fresh_rev()?);
            }
        }
        keep_in_folder(&self.folder.join(STATE_FILE), &state_bytes(&kept))?;
        Ok(kept)
    }

    /// `key`'s revision now ([`rev`]), the state put right first.
    pub fn rev(&self, key: &str) -> std::io::Result<String> {
        let revs = Revs::Kept(self.revs()?);
        Ok(rev(&self.folder, key, &revs))
    }

    /// A fresh revision for `key`, kept in the state, whole and in place,
    /// durably (the folder synced), before anything else changes: for a
    /// change that writes nothing else (the reset's, a redo over a kept no).
    pub fn bump(&self, key: &str) -> std::io::Result<String> {
        let fresh = self.stage_bump(key)?;
        std::fs::File::open(&self.folder)?.sync_all()?;
        Ok(fresh)
    }

    /// [`Held::bump`], the state renamed into place but not yet synced: the
    /// change made after it in the same folder (a yes or a no written, a
    /// file removed) syncs the folder once for both (#8704: a held change
    /// then costs what it did before revisions, not two syncs more, so the
    /// user's forget keeps its deadline). Holding the folder, every holder
    /// and reader sees the new state before the change, so a stale app yes
    /// is refused whatever the disk has; on a crash before the sync, a
    /// state lost or torn reads missing or unreadable, which is a fresh
    /// boundary (every older revision stale), and the folder's one sync
    /// keeps the state's rename with the change after it.
    ///
    /// A state that is missing, can't be read or lacks a key gets its fresh
    /// boundary ([`Held::revs`]) in the same write.
    fn stage_bump(&self, key: &str) -> std::io::Result<String> {
        let fresh = fresh_rev()?;
        self.stage_rev(key, &fresh)?;
        Ok(fresh)
    }

    /// [`Held::stage_bump`] with the fresh revision given.
    fn stage_rev(&self, key: &str, fresh: &str) -> std::io::Result<()> {
        let mut wrote = match Revs::read(&self.folder) {
            Revs::Kept(kept) => kept,
            Revs::Missing | Revs::Unreadable => BTreeMap::new(),
        };
        for (known, _) in FILES {
            if !wrote.contains_key(known) {
                wrote.insert(known.to_owned(), fresh_rev()?);
            }
        }
        wrote.insert(key.to_owned(), fresh.to_owned());
        keep_staged(
            &self.folder.join(STATE_FILE),
            &state_bytes(&wrote),
            folder_group(&self.folder)?,
        )
    }

    /// A "no"'s fresh revision for `key`, tried before or after the no is
    /// made, never in its way (the Reviewer-lead's P2 on #8704): `gone-REV`
    /// once kept, or, when the state can't be written (a full disk, a
    /// folder in the temporary's place), [`REV_UNKNOWN`] with one journal
    /// line, the no made all the same. Holding the folder, an app set then
    /// compares against what the state holds, so its old revision is stale
    /// as long as the state names any change since; with no state it is
    /// stale anyway ([`Held::revs`] puts a fresh boundary first).
    fn no_rev(&self, key: &str) -> String {
        match fresh_rev().and_then(|fresh| self.stage_rev(key, &fresh).map(|()| fresh)) {
            Ok(fresh) => format!("{GONE}{fresh}"),
            Err(error) => {
                eprintln!(
                    "lamp-msgs: consent: the {key} no is made, but its revision could not be kept: {}",
                    error.kind()
                );
                REV_UNKNOWN.into()
            }
        }
    }

    /// `key`'s yes kept, given `at_unix_s` by `by`: a fresh revision in the
    /// state first ([`Held::bump`]), then the yes with it
    /// ([`bytes_with_rev`]), whole and in place. Its revision, now `key`'s.
    /// A yes that fails after the state leaves `key` reading `gone-REV`:
    /// every revision from before is stale.
    pub fn keep_yes(&self, key: &str, at_unix_s: u64, by: &str) -> std::io::Result<String> {
        let path =
            file_in(&self.folder, key).ok_or_else(|| std::io::Error::other("no such yes"))?;
        let group = folder_group(&self.folder)?;
        self.keep_yes_at(&path, key, at_unix_s, by, group)
    }

    /// [`Held::keep_yes`] at `path` in the folder held (the voice's own
    /// name for it), 0640 and `group`'s, or 0600 with none.
    pub fn keep_yes_at(
        &self,
        path: &Path,
        key: &str,
        at_unix_s: u64,
        by: &str,
        group: Option<u32>,
    ) -> std::io::Result<String> {
        self.within(path)?;
        let fresh = self.stage_bump(key)?;
        keep_with_group(path, &bytes_with_rev(key, at_unix_s, by, &fresh), group)?;
        Ok(fresh)
    }

    /// `key`'s no kept (#7767), given `at_unix_s` by `by`: an explicit
    /// "no" where a missing file would read as on ([`NOTICING_BY_DEFAULT`]).
    /// A fresh revision tried in the state first, never in the no's way
    /// (`Held::no_rev`), then the no with it ([`no_bytes_with_rev`]), whole
    /// and in place, so `key` reads `gone-REV` and every revision from
    /// before is stale, the same as [`Held::take_back`]. Its revision then,
    /// `gone-REV`, or [`REV_UNKNOWN`] when the state could not be written.
    pub fn keep_no(&self, key: &str, at_unix_s: u64, by: &str) -> std::io::Result<String> {
        let path =
            file_in(&self.folder, key).ok_or_else(|| std::io::Error::other("no such yes"))?;
        let group = folder_group(&self.folder)?;
        self.keep_no_at(&path, key, at_unix_s, by, group)
    }

    /// [`Held::keep_no`] at `path` in the folder held (the voice's own name
    /// for it), 0640 and `group`'s, or 0600 with none.
    pub fn keep_no_at(
        &self,
        path: &Path,
        key: &str,
        at_unix_s: u64,
        by: &str,
        group: Option<u32>,
    ) -> std::io::Result<String> {
        self.within(path)?;
        // The revision tried first, never in the no's way; the no's own
        // write then syncs the folder for both.
        let rev = self.no_rev(key);
        let bytes = match rev.strip_prefix(GONE) {
            Some(fresh) => no_bytes_with_rev(key, at_unix_s, by, fresh),
            None => no_bytes(key, at_unix_s, by),
        };
        keep_with_group(path, &bytes, group)?;
        Ok(rev)
    }

    /// `path` is a file in the folder held.
    fn within(&self, path: &Path) -> std::io::Result<()> {
        if path.parent() == Some(self.folder.as_path()) {
            Ok(())
        } else {
            Err(std::io::Error::other("a yes outside the folder held"))
        }
    }

    /// `key`'s yes taken back, whoever gave it: the file removed first,
    /// whatever the state then does, then a fresh revision in the state,
    /// the folder synced once for both. `key`'s revision then, `gone-REV`,
    /// or [`REV_UNKNOWN`] when the state could not be written (the yes is
    /// gone all the same).
    pub fn take_back(&self, key: &str) -> std::io::Result<String> {
        let path =
            file_in(&self.folder, key).ok_or_else(|| std::io::Error::other("no such yes"))?;
        self.take_back_at(&path, key)
    }

    /// [`Held::take_back`] at `path` in the folder held (the voice's own
    /// name for it). An explicit "no": the revision is renewed even with no
    /// yes there, so a set made against the revision before it never
    /// applies after.
    pub fn take_back_at(&self, path: &Path, key: &str) -> std::io::Result<String> {
        self.withdraw_at(path, key).map_err(|refused| refused.error)
    }

    /// [`Held::take_back_at`], and when it fails, the bytes the file held
    /// as it was read holding the folder (none with no file, or one that
    /// could not be read): the one yes a retry may remove
    /// ([`Held::take_back_if`]), never a newer one written since.
    pub fn withdraw_at(&self, path: &Path, key: &str) -> Result<String, Refused> {
        self.within(path)
            .map_err(|error| Refused { error, held: None })?;
        let held = read_whole(path);
        let refused = |error| Refused {
            error,
            held: held.clone(),
        };
        // The yes removed first, whatever the revision then does (the
        // Reviewer-lead's P2 on #8704); then the fresh revision, also with
        // no file there (an explicit "no" still renews), and the folder
        // synced once for both.
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(refused(error)),
        }
        let rev = self.no_rev(key);
        std::fs::File::open(&self.folder)
            .and_then(|folder| folder.sync_all())
            .map_err(refused)?;
        Ok(rev)
    }

    /// A take-back retried: `path`'s yes withdrawn ([`Held::withdraw_at`])
    /// only while it still holds `held`, the bytes it held when the first
    /// one failed. `Ok(true)` once done or no longer owed (the file gone or
    /// holding something else: a newer yes stands); `Ok(false)` while it
    /// still fails.
    pub fn take_back_if(&self, path: &Path, key: &str, held: &[u8]) -> std::io::Result<bool> {
        match read_whole(path) {
            Some(bytes) if bytes == held => Ok(self.withdraw_at(path, key).is_ok()),
            _ => Ok(true),
        }
    }
}

/// A take-back that failed: why, and what the file held as it was read
/// holding the folder.
#[derive(Debug)]
pub struct Refused {
    pub error: std::io::Error,
    pub held: Option<Vec<u8>>,
}

/// `path`'s bytes, whole, at most [`MOST_BYTES`]; none when missing,
/// unreadable or longer.
fn read_whole(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MOST_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MOST_BYTES).then_some(bytes)
}

/// `path`'s yes for `key` taken back, whoever gave it, through its folder
/// held ([`Held::take_back_at`]): an explicit "no" (a forget of
/// everything), so the revision is renewed even with no file there. A
/// folder that is not there holds no yes and no revision, and nothing
/// changes.
pub fn take_back_at(path: &Path, key: &str) -> std::io::Result<()> {
    withdraw_at(path, key).map_err(|refused| refused.error)
}

/// [`take_back_at`], with what the file held when it failed
/// ([`Held::withdraw_at`]).
pub fn withdraw_at(path: &Path, key: &str) -> Result<(), Refused> {
    let folder = path.parent().ok_or_else(|| Refused {
        error: std::io::Error::other("a yes has no folder"),
        held: None,
    })?;
    let held = match hold(folder) {
        Ok(held) => held,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Refused { error, held: None }),
    };
    held.withdraw_at(path, key).map(|_| ())
}

/// `key`'s yes kept at `path`, given `at_unix_s` by `by`, through its
/// folder held ([`Held::keep_yes_at`]), 0640 and `group`'s (0600 with
/// none): for a writer that holds nothing yet (the voice's seen activity
/// yes). A writer that holds the mark's lock ([`lock_mark`]) takes it
/// first, then the folder, never the other way round.
pub fn keep_yes_holding(
    path: &Path,
    key: &str,
    at_unix_s: u64,
    by: &str,
    group: Option<u32>,
) -> std::io::Result<String> {
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("a yes has no folder"))?;
    hold(folder)?.keep_yes_at(path, key, at_unix_s, by, group)
}

/// [`keep_yes_holding`] for a no ([`Held::keep_no_at`]).
pub fn keep_no_holding(
    path: &Path,
    key: &str,
    at_unix_s: u64,
    by: &str,
    group: Option<u32>,
) -> std::io::Result<String> {
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("a yes has no folder"))?;
    hold(folder)?.keep_no_at(path, key, at_unix_s, by, group)
}

/// A fresh revision for `key`, its file in `path`'s folder left as it is
/// ([`Held::bump`], the folder held): a change at the lamp that writes
/// nothing (a redo over a kept no) still makes every older revision stale.
pub fn renew_holding(path: &Path, key: &str) -> std::io::Result<String> {
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("a yes has no folder"))?;
    hold(folder)?.bump(key)
}

/// `bytes` kept at `path` in the consent folder, whole and in place at
/// once (a temporary, `NAME.change-next`, written and synced, renamed over
/// it, the folder synced): 0640 and the folder's group (`lampos` on a lamp,
/// which the eyes and the ears are in), or 0600 where the folder's group is
/// root's.
pub fn keep_in_folder(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("a yes has no folder"))?;
    keep_with_group(path, bytes, folder_group(folder)?)
}

/// `folder`'s group, none where it is root's.
fn folder_group(folder: &Path) -> std::io::Result<Option<u32>> {
    use std::os::unix::fs::MetadataExt;
    Ok(Some(std::fs::metadata(folder)?.gid()).filter(|group| *group != 0))
}

/// `bytes` written beside `path` and renamed over it, 0640 and `group`'s
/// (0600 with none), with no sync: only for the state, before a change in
/// the same folder that syncs it ([`Held::stage_bump`]).
fn keep_staged(path: &Path, bytes: &[u8], group: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let next = path.with_extension("change-next");
    let _ = std::fs::remove_file(&next);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&next)?;
        let mode = if group.is_some() { 0o640 } else { 0o600 };
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        if group.is_some() {
            std::os::unix::fs::fchown(&file, None, group)?;
        }
        std::io::Write::write_all(&mut file, bytes)?;
        std::fs::rename(&next, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&next);
    }
    result
}

/// [`keep_in_folder`], 0640 and `group`'s, or 0600 with none.
pub fn keep_with_group(path: &Path, bytes: &[u8], group: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("a yes has no folder"))?;
    let next = path.with_extension("change-next");
    let _ = std::fs::remove_file(&next);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&next)?;
        let mode = if group.is_some() { 0o640 } else { 0o600 };
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        if group.is_some() {
            std::os::unix::fs::fchown(&file, None, group)?;
        }
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
        std::fs::rename(&next, path)?;
        std::fs::File::open(folder)?.sync_all()
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&next);
    }
    result
}

/// `path` removed, durably (the folder synced); one not there is removed
/// already.
pub fn remove_in_folder(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// `by` for a yes given in the app.
pub const BY_APP: &str = "app";
/// `by` for a yes spoken at onboarding.
pub const BY_VOICE: &str = "voice";

/// The largest file read, as the readers' own bound.
pub const MOST_BYTES: u64 = 4096;

/// `key`'s file in `folder`, or none for a key that is neither.
pub fn file_in(folder: &Path, key: &str) -> Option<PathBuf> {
    FILES
        .iter()
        .find(|(known, _)| *known == key)
        .map(|(_, name)| folder.join(name))
}

/// What a kept yes file says, read as the readers read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    /// `KEY` is exactly `"yes"`.
    pub yes: bool,
    /// Who gave it: `"app"`, `"voice"`, or none (a file from before `by`).
    pub by: Option<String>,
    /// When, in Unix seconds, if said.
    pub at_unix_s: Option<u64>,
    /// `KEY` is exactly `"no"` ([`no_bytes`], #7767): a person's kept no, as
    /// against a file that can't be read.
    pub no: bool,
    /// No file at all, read as on with [`NOTICING_BY_DEFAULT`] (#7767): set
    /// only there, never from a file's text, so a file holding
    /// `"by":"default"` is no default's yes (Twelve's P3 on #7928).
    pub by_default: bool,
    /// The helper's revision of it ([`rev`]), if written with one.
    pub rev: Option<String>,
}

impl Kept {
    /// Given in the app.
    pub fn by_app(&self) -> bool {
        self.by.as_deref() == Some(BY_APP)
    }
}

/// The switch that reads a missing yes file as on (#7767): `on` exactly
/// makes the face expressions yes, the room sounds yes and seen activity
/// default on. A kept `"no"` stays no and a file that can't be read stays
/// off either way; off, a missing file is no, as before. Off unless set, in
/// no unit (`docs/switches.md`). Health and crash reports are not under it:
/// [`reports_by_default`].
pub const NOTICING_BY_DEFAULT: &str = "LAMP_NOTICING_BY_DEFAULT";

/// `by` for a yes nobody gave: no file is kept and
/// [`NOTICING_BY_DEFAULT`] is on. Never written.
pub const BY_DEFAULT: &str = "default";

/// Whether `value` turns [`NOTICING_BY_DEFAULT`] on: `on` exactly.
pub fn by_default_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// [`NOTICING_BY_DEFAULT`] as this process's environment says it: the one
/// place the switch is read, so every reader takes the same default.
pub fn noticing_by_default() -> bool {
    by_default_on(std::env::var(NOTICING_BY_DEFAULT).ok().as_deref())
}

/// Health and crash reports with no record of a person's answer: on. The
/// product owner's call of 25 Nov 2026: health and crash reports are on
/// unless a person kept a no (at setup, or later in the app). Apart from
/// [`noticing_by_default`], which desk pictures, face expressions and room
/// sounds keep. Read by lamp-cloud's reports and setup's status.
pub fn reports_by_default() -> bool {
    true
}

/// The done mark of the voice's migration of earlier nos (#7767, the first
/// builder's c7002724376 and c7003588081), in the consent folder beside the
/// files: written once, with [`NOTICING_BY_DEFAULT`] on, after every no a
/// person gave before the default is kept as one (`lamp-voice`'s
/// `onboarding/storage.rs`, `keep_old_nos`). Until it is there, a missing
/// file is no to every reader ([`read_or`]), so a migration that never ran
/// or failed half way leaves the lamp off, never on.
pub const OLD_NOS_DONE: &str = "noticing-old-nos.done";

/// Whether [`OLD_NOS_DONE`] is in `folder` (there at all, never followed).
pub fn old_nos_done(folder: &Path) -> bool {
    std::fs::symlink_metadata(folder.join(OLD_NOS_DONE)).is_ok()
}

/// `path`'s yes for `key` (#7767): one rule for a kept yes, a kept no and
/// the default.
///
/// - `KEY` exactly `"yes"`: a yes, with who gave it and when.
/// - No file at `path` (not found): `by_default` once [`OLD_NOS_DONE`] is
///   beside it ([`old_nos_done`]), as [`BY_DEFAULT`]'s yes with no time;
///   with no done mark, or `by_default` false, no yes.
/// - `KEY` exactly `"no"` ([`no_bytes`]): no, whatever `by_default` says.
/// - Unreadable (any open error but not found, or a read error), over
///   [`MOST_BYTES`], not JSON, not an object, or an object with anything
///   else under `KEY`: no, the safe side.
pub fn read_or(path: &Path, key: &str, by_default: bool) -> Kept {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && by_default
                && path.parent().is_some_and(old_nos_done) =>
        {
            return Kept {
                yes: true,
                by: Some(BY_DEFAULT.to_owned()),
                at_unix_s: None,
                no: false,
                by_default: true,
                rev: None,
            };
        }
        Err(_) => return Kept::default(),
    };
    let mut text = String::new();
    if file.take(MOST_BYTES + 1).read_to_string(&mut text).is_err()
        || text.len() as u64 > MOST_BYTES
    {
        return Kept::default();
    }
    let Ok(Value::Object(value)) = serde_json::from_str::<Value>(&text) else {
        return Kept::default();
    };
    let answer = value.get(key).and_then(Value::as_str);
    Kept {
        yes: answer == Some("yes"),
        by: value.get("by").and_then(Value::as_str).map(str::to_owned),
        at_unix_s: value.get("at_unix_s").and_then(Value::as_u64),
        no: answer == Some("no"),
        by_default: false,
        rev: value.get("rev").and_then(Value::as_str).map(str::to_owned),
    }
}

/// `path`'s yes for `key` with no default: missing, unreadable, over
/// [`MOST_BYTES`], not JSON or not an object is no yes, with nothing else
/// said ([`read_or`] with `by_default` false).
pub fn read(path: &Path, key: &str) -> Kept {
    read_or(path, key, false)
}

/// Whether `key` is allowed at `path` ([`read_or`]'s rule): a kept `"yes"`
/// is, a kept `"no"` is not, a missing file is `by_default`, and a file
/// that can't be read is not. Readers pass [`noticing_by_default`].
pub fn allowed(path: &Path, key: &str, by_default: bool) -> bool {
    read_or(path, key, by_default).yes
}

/// Whether `path` holds a person's kept no for `key` (`KEY` exactly `"no"`,
/// [`no_bytes`], #7767), which with [`NOTICING_BY_DEFAULT`] on nothing but
/// a yes or a factory reset replaces.
pub fn kept_no(path: &Path, key: &str) -> bool {
    read(path, key).no
}

/// A yes file's bytes as a holder writes them, with its revision:
/// `{"version":1,KEY:"yes","at_unix_s":N,"by":BY,"rev":REV}`. The readers
/// read only `KEY`, so the revision changes nothing they read.
pub fn bytes_with_rev(key: &str, at_unix_s: u64, by: &str, rev: &str) -> Vec<u8> {
    answer_bytes(key, "yes", at_unix_s, by, Some(rev))
}

/// A kept no's bytes as a holder writes them, with its revision (#7767):
/// `{"version":1,KEY:"no","at_unix_s":N,"by":BY,"rev":REV}`. The revision
/// is the one its holder kept first in the state, so the key reads
/// `gone-REV` ([`rev`]) either way.
pub fn no_bytes_with_rev(key: &str, at_unix_s: u64, by: &str, rev: &str) -> Vec<u8> {
    answer_bytes(key, "no", at_unix_s, by, Some(rev))
}

/// A kept file's bytes, whole, with `KEY` as `answer` and, if given, the
/// revision it was kept with.
fn answer_bytes(key: &str, answer: &str, at_unix_s: u64, by: &str, rev: Option<&str>) -> Vec<u8> {
    let mut object = serde_json::Map::new();
    object.insert("version".into(), 1.into());
    object.insert(key.into(), answer.into());
    object.insert("at_unix_s".into(), at_unix_s.into());
    object.insert("by".into(), by.into());
    if let Some(rev) = rev {
        object.insert("rev".into(), rev.into());
    }
    serde_json::to_vec(&Value::Object(object)).unwrap_or_default()
}

/// The app's desk noticing mark (#6543, `docs/cloud.md`, "Desk noticing
/// from the app"), beside the yes in [`FOLDER`]:
/// `{"version":1,"after_unix_ms":N}`. A `desk_noticing.set` yes is kept only
/// with a `request_id` (the app's send time in Unix ms) above `N`, so a
/// resent or late yes never lands after a later "no". Root's helper raises
/// it to each app request's id it takes; the voice's stop and any forget
/// raise it to the lamp's own now. Written whole and renamed into place,
/// in the yes's mode, only under [`MarkLock`]; only the helper reads it.
/// The reset's plan removes it with the yes.
pub const SEEN_ACTIVITY_MARK: &str = "seen-activity-mark.json";

/// The lock beside the mark (Twelve's P2 on #7551): an empty file, held
/// with `flock` by every writer of the yes or the mark across its check and
/// write, so a "no" (the voice's stop, the app's off, a forget) and an app
/// yes never interleave. Kept: it holds nothing.
pub const SEEN_ACTIVITY_LOCK: &str = "seen-activity-mark.lock";

/// How far past the lamp's own now an app's `request_id` may be (Twelve's
/// P2 on #7551): a day, generous for a phone's clock. One beyond it (an app
/// sending microseconds, a phone years ahead) is no send time: a yes is
/// refused, and an off raises the mark only to the lamp's now, so the mark
/// never runs ahead of the clock and refuses every later yes.
pub const REQUEST_AHEAD_MOST_MS: u64 = 24 * 60 * 60 * 1000;

/// The mark's file in `folder`.
pub fn mark_in(folder: &Path) -> PathBuf {
    folder.join(SEEN_ACTIVITY_MARK)
}

/// Opened as root opens a file another may have placed: no link followed,
/// never blocked on a FIFO.
fn careful() -> i32 {
    use rustix::fs::OFlags;
    (OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC).bits() as i32
}

/// The mark at `path`: 0 when there is none; `None` when it is there but
/// unreadable, not a plain file or not the mark's shape, so a yes is
/// refused, the safe side. Opened with `O_NOFOLLOW | O_NONBLOCK`, as the
/// writer is careful to be.
pub fn read_mark(path: &Path) -> Option<u64> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(careful())
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(0),
        Err(_) => return None,
    };
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    if file.take(MOST_BYTES + 1).read_to_string(&mut text).is_err()
        || text.len() as u64 > MOST_BYTES
    {
        return None;
    }
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|value| value.get("after_unix_ms").and_then(Value::as_u64))
}

/// Now, in Unix milliseconds, on the lamp's clock.
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// An app's `request_id` past [`REQUEST_AHEAD_MOST_MS`] of the lamp's now.
pub fn too_far_ahead(request_id: u64) -> bool {
    request_id > now_unix_ms().saturating_add(REQUEST_AHEAD_MOST_MS)
}

/// The mark's lock in a consent folder, held until dropped: whoever holds
/// it reads the mark, raises it, and keeps or removes the yes beside it
/// with no other writer between. Another open of the same lock waits, in
/// this process too, so a holder raises through [`MarkLock::raise`], never
/// [`raise_mark`].
#[derive(Debug)]
pub struct MarkLock {
    _held: std::fs::File,
    mark: PathBuf,
}

/// Take the mark's lock in `folder`, waiting for any other holder: the
/// lock file made if missing (`O_NOFOLLOW`, never a link or a FIFO), in the
/// yes's mode, 0640 and the folder's group, or 0600 where the folder is
/// root's.
pub fn lock_mark(folder: &Path) -> std::io::Result<MarkLock> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let group = Some(std::fs::metadata(folder)?.gid()).filter(|group| *group != 0);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .mode(if group.is_some() { 0o640 } else { 0o600 })
        .custom_flags(careful())
        .open(folder.join(SEEN_ACTIVITY_LOCK))?;
    let made = file.metadata()?;
    if !made.is_file() {
        return Err(std::io::Error::other("the mark's lock is not a plain file"));
    }
    if group.is_some_and(|group| made.gid() != group) {
        std::os::unix::fs::fchown(&file, None, group)?;
    }
    file.lock()?;
    Ok(MarkLock {
        _held: file,
        mark: mark_in(folder),
    })
}

impl MarkLock {
    /// The mark, as [`read_mark`] reads it.
    pub fn read(&self) -> Option<u64> {
        read_mark(&self.mark)
    }

    /// Raise the mark to `at_ms`: read then written under this lock, never
    /// lowered (one unreadable is replaced), written whole beside it and
    /// renamed into place, in the lock's mode. What it holds after.
    pub fn raise(&self, at_ms: u64) -> std::io::Result<u64> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let path = &self.mark;
        let folder = path
            .parent()
            .ok_or_else(|| std::io::Error::other("the mark has no folder"))?;
        let read = read_mark(path);
        let mark = read.unwrap_or(0).max(at_ms);
        if read == Some(mark) {
            return Ok(mark);
        }
        let group = Some(std::fs::metadata(folder)?.gid()).filter(|group| *group != 0);
        // One name, written only under the lock.
        let next = path.with_extension("mark-next");
        let _ = std::fs::remove_file(&next);
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&next)?;
            let mode = if group.is_some() { 0o640 } else { 0o600 };
            file.set_permissions(std::fs::Permissions::from_mode(mode))?;
            if group.is_some() {
                std::os::unix::fs::fchown(&file, None, group)?;
            }
            let text = serde_json::json!({"version": 1, "after_unix_ms": mark}).to_string();
            std::io::Write::write_all(&mut file, text.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&next, path)?;
            std::fs::File::open(folder)?.sync_all()
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&next);
        }
        result.map(|()| mark)
    }
}

/// Raise the mark at `path` to `at_ms` under its lock ([`lock_mark`] in its
/// folder, taken and let go here): [`MarkLock::raise`]. What it holds after.
pub fn raise_mark(path: &Path, at_ms: u64) -> std::io::Result<u64> {
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("the mark has no folder"))?;
    lock_mark(folder)?.raise(at_ms)
}

/// A yes file's bytes, whole: `{"version":1,KEY:"yes","at_unix_s":N,"by":BY}`.
pub fn bytes(key: &str, at_unix_s: u64, by: &str) -> Vec<u8> {
    answer_bytes(key, "yes", at_unix_s, by, None)
}

/// A kept no's bytes, whole: `{"version":1,KEY:"no","at_unix_s":N,"by":BY}`
/// (#7767). Kept, not removed, so that [`NOTICING_BY_DEFAULT`] never turns
/// a person's no back on.
pub fn no_bytes(key: &str, at_unix_s: u64, by: &str) -> Vec<u8> {
    answer_bytes(key, "no", at_unix_s, by, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_yes_is_read_with_who_gave_it() {
        let folder = std::env::temp_dir().join(format!("msgs-consent-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = file_in(&folder, FACE_EXPRESSIONS).unwrap();
        assert_eq!(read(&file, FACE_EXPRESSIONS), Kept::default(), "missing");
        std::fs::write(&file, bytes(FACE_EXPRESSIONS, 7, BY_APP)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            r#"{"at_unix_s":7,"by":"app","face_expressions":"yes","version":1}"#
        );
        let kept = read(&file, FACE_EXPRESSIONS);
        assert!(kept.yes && kept.by_app() && kept.at_unix_s == Some(7));
        // A file from before `by` is a yes nobody named: the voice's.
        std::fs::write(
            &file,
            r#"{"version":1,"face_expressions":"yes","at_unix_s":3}"#,
        )
        .unwrap();
        let kept = read(&file, FACE_EXPRESSIONS);
        assert!(kept.yes && !kept.by_app() && kept.by.is_none());
        // Another key's yes, a "no", JSON that is no object: no yes.
        assert!(!read(&file, ROOM_SOUNDS).yes);
        for text in [r#"{"face_expressions":"no"}"#, "[1]", "yes"] {
            std::fs::write(&file, text).unwrap();
            assert!(!read(&file, FACE_EXPRESSIONS).yes, "{text}");
        }
        std::fs::write(&file, vec![b' '; MOST_BYTES as usize + 1]).unwrap();
        assert!(!read(&file, FACE_EXPRESSIONS).yes, "too long");
        assert_eq!(file_in(&folder, "camera"), None);
        assert_eq!(
            file_in(&folder, SEEN_ACTIVITY),
            Some(folder.join("seen-activity.json"))
        );
        let _ = std::fs::remove_dir_all(folder);
    }

    /// #6543: the desk noticing mark is 0 when missing, refused when
    /// garbled, only ever raised, and replaces a garbled one.
    #[test]
    fn the_desk_noticing_mark_only_rises() {
        let folder = std::env::temp_dir().join(format!("msgs-mark-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let mark = mark_in(&folder);
        assert_eq!(read_mark(&mark), Some(0));
        assert_eq!(raise_mark(&mark, 20).unwrap(), 20);
        assert_eq!(
            std::fs::read_to_string(&mark).unwrap(),
            r#"{"after_unix_ms":20,"version":1}"#
        );
        assert_eq!(raise_mark(&mark, 10).unwrap(), 20);
        assert_eq!(read_mark(&mark), Some(20));
        assert_eq!(raise_mark(&mark, 30).unwrap(), 30);
        std::fs::write(&mark, "not a mark").unwrap();
        assert_eq!(read_mark(&mark), None);
        assert_eq!(raise_mark(&mark, 5).unwrap(), 5);
        assert_eq!(read_mark(&mark), Some(5));
        assert!(now_unix_ms() > 1_700_000_000_000);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Twelve's P3 on #7551: the mark is read as it is written, no link
    /// followed and no FIFO waited on; either refuses a yes. The lock is
    /// made in the yes's mode and refuses a link in its place.
    #[test]
    fn the_mark_and_its_lock_follow_no_link_and_wait_on_no_fifo() {
        use std::os::unix::fs::PermissionsExt;
        let folder = std::env::temp_dir().join(format!("msgs-careful-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let mark = mark_in(&folder);
        let elsewhere = folder.join("elsewhere.json");
        std::fs::write(&elsewhere, r#"{"version":1,"after_unix_ms":1}"#).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &mark).unwrap();
        assert_eq!(read_mark(&mark), None);
        std::fs::remove_file(&mark).unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&mark)
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(read_mark(&mark), None);
        std::fs::remove_file(&mark).unwrap();
        let lock = folder.join(SEEN_ACTIVITY_LOCK);
        std::os::unix::fs::symlink(&elsewhere, &lock).unwrap();
        assert!(lock_mark(&folder).is_err());
        std::fs::remove_file(&lock).unwrap();
        let held = lock_mark(&folder).unwrap();
        let group = std::os::unix::fs::MetadataExt::gid(&std::fs::metadata(&folder).unwrap());
        let mode = std::fs::metadata(&lock).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, if group == 0 { 0o600 } else { 0o640 });
        assert_eq!(held.raise(4).unwrap(), 4);
        assert_eq!(held.read(), Some(4));
        drop(held);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Twelve's P2 on #7551: a raise waits for the lock's holder, so the
    /// holder's check and write are never split; raises from many writers at
    /// once leave the highest.
    #[test]
    fn a_raise_waits_for_the_locks_holder() {
        let folder = std::env::temp_dir().join(format!("msgs-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let mark = mark_in(&folder);
        let held = lock_mark(&folder).unwrap();
        let waiting = {
            let mark = mark.clone();
            std::thread::spawn(move || raise_mark(&mark, 50).unwrap())
        };
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!waiting.is_finished());
        assert_eq!(held.read(), Some(0));
        assert_eq!(held.raise(70).unwrap(), 70);
        drop(held);
        assert_eq!(waiting.join().unwrap(), 70);
        let writers: Vec<_> = (1..=8_u64)
            .map(|writer| {
                let mark = mark.clone();
                std::thread::spawn(move || {
                    for step in 0..20 {
                        raise_mark(&mark, 100 + writer * 20 + step).unwrap();
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        assert_eq!(read_mark(&mark), Some(100 + 8 * 20 + 19));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// #6543: the app's desk noticing yes is the voice's seen activity yes
    /// byte for byte but for `by`.
    #[test]
    fn the_apps_seen_activity_yes_is_the_voices_but_for_by() {
        let app = String::from_utf8(bytes(SEEN_ACTIVITY, 11, BY_APP)).unwrap();
        let voice = String::from_utf8(bytes(SEEN_ACTIVITY, 11, BY_VOICE)).unwrap();
        assert_eq!(
            voice,
            r#"{"at_unix_s":11,"by":"voice","seen_activity":"yes","version":1}"#
        );
        assert_eq!(app, voice.replace(r#""by":"voice""#, r#""by":"app""#));
    }

    /// #7767: a kept no stays no with the switch on; a missing file is the
    /// default only with it on; a file that can't be read is off either
    /// way; with it off, every answer is [`read`]'s, as before.
    #[test]
    fn a_kept_no_stays_no_and_missing_is_the_default_only_with_the_switch() {
        let folder =
            std::env::temp_dir().join(format!("msgs-consent-default-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = file_in(&folder, SEEN_ACTIVITY).unwrap();
        // Missing: the default, only with the switch on, and only once the
        // migration's done mark is there (c7003588081).
        assert!(!allowed(&file, SEEN_ACTIVITY, false), "missing, off");
        assert!(!allowed(&file, SEEN_ACTIVITY, true), "missing, on, no mark");
        std::fs::write(folder.join(OLD_NOS_DONE), b"{}").unwrap();
        assert!(!allowed(&file, SEEN_ACTIVITY, false), "missing, off, mark");
        assert!(allowed(&file, SEEN_ACTIVITY, true), "missing, on");
        assert_eq!(
            read_or(&file, SEEN_ACTIVITY, true),
            Kept {
                yes: true,
                by: Some(BY_DEFAULT.into()),
                at_unix_s: None,
                no: false,
                by_default: true,
                rev: None,
            }
        );
        assert_eq!(read(&file, SEEN_ACTIVITY), Kept::default());
        // A kept no: no either way.
        std::fs::write(&file, no_bytes(SEEN_ACTIVITY, 9, BY_VOICE)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            r#"{"at_unix_s":9,"by":"voice","seen_activity":"no","version":1}"#
        );
        for by_default in [false, true] {
            assert!(
                !allowed(&file, SEEN_ACTIVITY, by_default),
                "no, {by_default}"
            );
        }
        // A kept yes: yes either way.
        std::fs::write(&file, bytes(SEEN_ACTIVITY, 9, BY_APP)).unwrap();
        for by_default in [false, true] {
            assert!(
                allowed(&file, SEEN_ACTIVITY, by_default),
                "yes, {by_default}"
            );
        }
        // Bad files: off either way, and as `read` says with the switch off.
        let bad: [&[u8]; 6] = [
            b"not json",
            b"[1]",
            b"\"yes\"",
            br#"{"version":1}"#,
            br#"{"seen_activity":true}"#,
            br#"{"seen_activity":"YES"}"#,
        ];
        for text in bad {
            std::fs::write(&file, text).unwrap();
            for by_default in [false, true] {
                assert!(
                    !allowed(&file, SEEN_ACTIVITY, by_default),
                    "{} {by_default}",
                    String::from_utf8_lossy(text)
                );
            }
            assert_eq!(
                read_or(&file, SEEN_ACTIVITY, false),
                read(&file, SEEN_ACTIVITY)
            );
        }
        std::fs::write(&file, vec![b' '; MOST_BYTES as usize + 1]).unwrap();
        assert!(!allowed(&file, SEEN_ACTIVITY, true), "too long");
        // There but unreadable as a file: a folder in its place.
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        assert!(!allowed(&file, SEEN_ACTIVITY, true), "a folder");
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Twelve's HOLD and P3 3 on #7928: `no` is a kept no only, never a
    /// file that can't be read; `by_default` is set only for no file with
    /// the switch on, never from a file's text.
    #[test]
    fn a_kept_no_and_the_default_are_told_apart_from_a_files_text() {
        let folder = std::env::temp_dir().join(format!("msgs-consent-no-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = file_in(&folder, FACE_EXPRESSIONS).unwrap();
        assert!(!kept_no(&file, FACE_EXPRESSIONS), "missing");
        assert!(
            !read_or(&file, FACE_EXPRESSIONS, true).by_default,
            "no mark"
        );
        std::fs::write(folder.join(OLD_NOS_DONE), b"{}").unwrap();
        assert!(read_or(&file, FACE_EXPRESSIONS, true).by_default);
        assert!(!read_or(&file, FACE_EXPRESSIONS, false).by_default);
        std::fs::write(&file, no_bytes(FACE_EXPRESSIONS, 9, BY_VOICE)).unwrap();
        assert!(kept_no(&file, FACE_EXPRESSIONS));
        assert!(!read_or(&file, FACE_EXPRESSIONS, true).by_default);
        for text in [
            &b"not json"[..],
            br#"{"face_expressions":"NO"}"#,
            br#"{"face_expressions":"yes","by":"default"}"#,
        ] {
            std::fs::write(&file, text).unwrap();
            assert!(!kept_no(&file, FACE_EXPRESSIONS));
            for by_default in [false, true] {
                assert!(!read_or(&file, FACE_EXPRESSIONS, by_default).by_default);
            }
        }
        let _ = std::fs::remove_dir_all(folder);
    }

    /// The first builder's c7003588081 on #7767: with the switch on and no
    /// done mark, a missing face-expressions, room-sounds or seen-activity
    /// file is no, a mark in another folder too; with the mark beside it,
    /// the default, and a kept no still no.
    #[test]
    fn a_missing_file_is_the_default_only_once_the_old_nos_are_done() {
        let folder = std::env::temp_dir().join(format!("msgs-consent-done-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("other")).unwrap();
        std::fs::write(folder.join("other").join(OLD_NOS_DONE), b"{}").unwrap();
        for (key, _) in FILES {
            let file = file_in(&folder, key).unwrap();
            assert!(!allowed(&file, key, true), "{key}, no mark");
            assert_eq!(read_or(&file, key, true), Kept::default(), "{key}");
        }
        std::fs::write(folder.join(OLD_NOS_DONE), b"{}").unwrap();
        assert!(old_nos_done(&folder));
        for (key, _) in FILES {
            let file = file_in(&folder, key).unwrap();
            assert!(allowed(&file, key, true), "{key}, mark");
            assert!(!allowed(&file, key, false), "{key}, mark, off");
            std::fs::write(&file, no_bytes(key, 9, BY_VOICE)).unwrap();
            assert!(!allowed(&file, key, true), "{key}, a kept no");
        }
        let _ = std::fs::remove_dir_all(folder);
    }

    /// The product owner's call of 25 Nov 2026: health and crash reports
    /// are on with no record, whatever [`NOTICING_BY_DEFAULT`] says. This
    /// test binary runs this test again with the switch `on`, `off` and
    /// unset; each child checks the switch reached it, and the default.
    #[test]
    fn reports_are_on_by_default_whatever_the_noticing_switch_says() {
        const NAME: &str =
            "consent::tests::reports_are_on_by_default_whatever_the_noticing_switch_says";
        const CHILD: &str = "LAMP_MSGS_TEST_REPORTS_DEFAULT_SWITCH";
        if let Ok(switch) = std::env::var(CHILD) {
            assert_eq!(noticing_by_default(), switch == "on", "{switch:?}");
            assert!(reports_by_default(), "{switch:?}");
            return;
        }
        assert!(reports_by_default());
        for switch in [Some("on"), Some("off"), None] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child
                .args([NAME, "--exact", "--test-threads", "1"])
                .env(CHILD, switch.unwrap_or_default());
            match switch {
                Some(value) => child.env(NOTICING_BY_DEFAULT, value),
                None => child.env_remove(NOTICING_BY_DEFAULT),
            };
            let out = child.output().unwrap();
            let said = String::from_utf8_lossy(&out.stdout);
            // Passed, and ran this one test, not none.
            assert!(
                out.status.success() && said.contains("1 passed"),
                "{switch:?}: {out:?}"
            );
        }
    }

    #[test]
    fn the_switch_is_on_only_for_on_exactly() {
        assert!(by_default_on(Some("on")));
        for value in [
            None,
            Some(""),
            Some("ON"),
            Some("on "),
            Some("yes"),
            Some("1"),
        ] {
            assert!(!by_default_on(value), "{value:?}");
        }
    }

    /// The helper's state file sits beside the yeses but is no key's file:
    /// a folder holding only it reads no yes, even when it names a key; a
    /// yes written with a revision reads as a yes, its revision with it.
    #[test]
    fn a_yes_with_a_rev_is_a_yes_and_the_state_file_is_none() {
        let folder = std::env::temp_dir().join(format!("msgs-consent-revs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(STATE_FILE),
            r#"{"version":1,"revs":{"face_expressions":{"wrote":"a"}},"face_expressions":"yes","room_sounds":"yes"}"#,
        )
        .unwrap();
        assert!(FILES.iter().all(|(_, name)| *name != STATE_FILE));
        for (key, _) in FILES {
            let file = file_in(&folder, key).unwrap();
            assert_ne!(file, folder.join(STATE_FILE));
            assert!(!read(&file, key).yes, "{key}");
        }
        let file = file_in(&folder, FACE_EXPRESSIONS).unwrap();
        std::fs::write(&file, bytes_with_rev(FACE_EXPRESSIONS, 7, BY_APP, "r1")).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            r#"{"at_unix_s":7,"by":"app","face_expressions":"yes","rev":"r1","version":1}"#
        );
        let kept = read(&file, FACE_EXPRESSIONS);
        assert!(kept.yes && kept.by_app() && kept.rev.as_deref() == Some("r1"));
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Each case of [`rev`], and the state file's reading.
    #[test]
    fn a_keys_rev_follows_its_file_and_the_helpers_state() {
        let folder = std::env::temp_dir().join(format!("msgs-consent-rev-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let face = file_in(&folder, FACE_EXPRESSIONS).unwrap();
        let revs = || Revs::read(&folder);
        // No state: `unknown`, never a revision one could match.
        assert_eq!(revs(), Revs::Missing, "no file");
        assert_eq!(rev(&folder, FACE_EXPRESSIONS, &revs()), REV_UNKNOWN);
        // The voice's yes, with and without a time.
        std::fs::write(&face, bytes(FACE_EXPRESSIONS, 5, BY_VOICE)).unwrap();
        assert_eq!(rev(&folder, FACE_EXPRESSIONS, &revs()), "yes-5");
        std::fs::write(&face, r#"{"face_expressions":"yes"}"#).unwrap();
        assert_eq!(rev(&folder, FACE_EXPRESSIONS, &revs()), "yes-0");
        // The helper's.
        let wrote = BTreeMap::from([(FACE_EXPRESSIONS.to_owned(), "r2".to_owned())]);
        std::fs::write(folder.join(STATE_FILE), state_bytes(&wrote)).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join(STATE_FILE)).unwrap(),
            r#"{"revs":{"face_expressions":{"wrote":"r2"}},"version":1}"#
        );
        assert_eq!(revs(), Revs::Kept(wrote));
        std::fs::write(&face, bytes_with_rev(FACE_EXPRESSIONS, 9, BY_APP, "r2")).unwrap();
        assert_eq!(rev(&folder, FACE_EXPRESSIONS, &revs()), "r2");
        assert_eq!(rev(&folder, ROOM_SOUNDS, &revs()), REV_UNKNOWN, "not named");
        // Taken back at the lamp.
        std::fs::remove_file(&face).unwrap();
        assert_eq!(rev(&folder, FACE_EXPRESSIONS, &revs()), "gone-r2");
        // A state not of its shape: unknown while no yes, the yes's own
        // revision while one stands.
        for text in [
            "not json".to_owned(),
            "[]".to_owned(),
            r#"{"version":2,"revs":{}}"#.to_owned(),
            r#"{"version":1}"#.to_owned(),
            r#"{"version":1,"revs":{"face_expressions":{"wrote":""}}}"#.to_owned(),
            r#"{"version":1,"revs":{"face_expressions":"r2"}}"#.to_owned(),
            format!(
                r#"{{"version":1,"revs":{{}},"pad":"{}"}}"#,
                "x".repeat(4096)
            ),
        ] {
            std::fs::write(folder.join(STATE_FILE), &text).unwrap();
            assert_eq!(revs(), Revs::Unreadable, "{text:.40}");
            assert_eq!(rev(&folder, FACE_EXPRESSIONS, &revs()), REV_UNKNOWN);
            std::fs::write(&face, bytes_with_rev(FACE_EXPRESSIONS, 9, BY_APP, "r3")).unwrap();
            assert_eq!(rev(&folder, FACE_EXPRESSIONS, &revs()), "r3");
            std::fs::remove_file(&face).unwrap();
        }
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Every change through [`Held`] keeps a fresh revision first: a "no",
    /// a yes and a "no" again at the lamp never bring back a revision the
    /// app saw after the first "no" (the privacy reviewer's ABA); a state
    /// that can't be read is put back as a fresh boundary.
    #[test]
    fn every_change_through_the_held_folder_is_a_new_rev() {
        let folder = std::env::temp_dir().join(format!("msgs-consent-held-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let held = hold(&folder).unwrap();
        assert!(!folder.join(STATE_FILE).exists(), "holding writes nothing");
        // A missing state: a fresh boundary for every key, kept before
        // anything is said.
        let first = held.rev(FACE_EXPRESSIONS).unwrap();
        assert!(first.starts_with(GONE), "{first}");
        assert_eq!(rev(&folder, FACE_EXPRESSIONS, &Revs::read(&folder)), first);
        assert!(rev(&folder, ROOM_SOUNDS, &Revs::read(&folder)).starts_with(GONE));
        // An explicit "no" with no yes there renews it all the same.
        let no = held.take_back(FACE_EXPRESSIONS).unwrap();
        assert_ne!(no, first);
        assert_eq!(held.rev(FACE_EXPRESSIONS).unwrap(), no);
        let r1 = held.keep_yes(FACE_EXPRESSIONS, 5, BY_APP).unwrap();
        assert_eq!(r1.len(), 32);
        assert!(r1.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(held.rev(FACE_EXPRESSIONS).unwrap(), r1);
        let face = file_in(&folder, FACE_EXPRESSIONS).unwrap();
        assert!(read(&face, FACE_EXPRESSIONS).yes);
        let seen = held.take_back(FACE_EXPRESSIONS).unwrap();
        assert_eq!(held.rev(FACE_EXPRESSIONS).unwrap(), seen);
        assert!(seen.starts_with(GONE) && !face.exists());
        held.keep_yes(FACE_EXPRESSIONS, 6, BY_VOICE).unwrap();
        let again = held.take_back(FACE_EXPRESSIONS).unwrap();
        assert_ne!(again, seen);
        assert_eq!(rev(&folder, FACE_EXPRESSIONS, &Revs::read(&folder)), again);
        // Not its shape: a fresh boundary for every key.
        std::fs::write(folder.join(STATE_FILE), "garbage").unwrap();
        assert_eq!(
            rev(&folder, FACE_EXPRESSIONS, &Revs::read(&folder)),
            REV_UNKNOWN
        );
        let face_now = held.rev(FACE_EXPRESSIONS).unwrap();
        let room_now = held.rev(ROOM_SOUNDS).unwrap();
        assert!(face_now.starts_with(GONE) && room_now.starts_with(GONE));
        assert_ne!(face_now, again);
        assert_eq!(rev(&folder, ROOM_SOUNDS, &Revs::read(&folder)), room_now);
        // Removed: the same, never `none` again.
        std::fs::remove_file(folder.join(STATE_FILE)).unwrap();
        assert_eq!(
            rev(&folder, FACE_EXPRESSIONS, &Revs::read(&folder)),
            REV_UNKNOWN
        );
        let after = held.rev(FACE_EXPRESSIONS).unwrap();
        assert!(after.starts_with(GONE) && after != face_now);
        drop(held);
        let _ = std::fs::remove_dir_all(folder);
    }

    /// A take-back that fails says what the file held, read holding the
    /// folder; its retry removes only that yes, never a newer one.
    #[test]
    fn a_failed_take_back_says_what_it_withdrew_and_its_retry_spares_a_newer_yes() {
        use std::os::unix::fs::PermissionsExt;
        let folder =
            std::env::temp_dir().join(format!("msgs-consent-withdraw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let held = hold(&folder).unwrap();
        held.keep_yes(FACE_EXPRESSIONS, 5, BY_APP).unwrap();
        let face = file_in(&folder, FACE_EXPRESSIONS).unwrap();
        let old = std::fs::read(&face).unwrap();
        let mode = |mode| {
            std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode)).unwrap()
        };
        mode(0o500);
        if std::fs::write(folder.join("probe"), b"").is_ok() {
            // Root writes into a read-only folder: nothing fails here.
            mode(0o700);
            let _ = std::fs::remove_dir_all(folder);
            return;
        }
        let refused = held.withdraw_at(&face, FACE_EXPRESSIONS).unwrap_err();
        mode(0o700);
        assert_eq!(refused.held.as_deref(), Some(old.as_slice()));
        assert!(face.exists());
        // A newer yes given before the retry: it stands.
        held.keep_yes(FACE_EXPRESSIONS, 6, BY_APP).unwrap();
        assert!(held.take_back_if(&face, FACE_EXPRESSIONS, &old).unwrap());
        assert!(read(&face, FACE_EXPRESSIONS).yes);
        // The yes it withdrew, still there: removed.
        std::fs::write(&face, &old).unwrap();
        assert!(held.take_back_if(&face, FACE_EXPRESSIONS, &old).unwrap());
        assert!(!face.exists());
        drop(held);
        let _ = std::fs::remove_dir_all(folder);
    }

    /// The Reviewer-lead's P2 on #8704: a "no" never waits on its revision.
    /// With the state's temporary blocked (a folder in its place), a
    /// take-back still removes the yes and a kept no is still kept, each
    /// said `unknown`; a yes, whose revision is written before it, is
    /// refused and nothing is kept.
    #[test]
    fn a_no_is_made_when_its_revision_cannot_be_kept() {
        let folder =
            std::env::temp_dir().join(format!("msgs-consent-blocked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let held = hold(&folder).unwrap();
        held.keep_yes(FACE_EXPRESSIONS, 5, BY_APP).unwrap();
        held.keep_yes(ROOM_SOUNDS, 5, BY_VOICE).unwrap();
        let before = held.rev(FACE_EXPRESSIONS).unwrap();
        std::fs::create_dir(folder.join("app-revs.change-next")).unwrap();
        let face = file_in(&folder, FACE_EXPRESSIONS).unwrap();
        assert_eq!(held.take_back(FACE_EXPRESSIONS).unwrap(), REV_UNKNOWN);
        assert!(!face.exists());
        assert_ne!(rev(&folder, FACE_EXPRESSIONS, &Revs::read(&folder)), before);
        let room = file_in(&folder, ROOM_SOUNDS).unwrap();
        assert_eq!(held.keep_no(ROOM_SOUNDS, 6, BY_VOICE).unwrap(), REV_UNKNOWN);
        assert!(kept_no(&room, ROOM_SOUNDS));
        assert!(held.keep_yes(FACE_EXPRESSIONS, 7, BY_APP).is_err());
        assert!(!face.exists());
        // The free function the voice's take-backs use, the same.
        std::fs::write(&face, bytes(FACE_EXPRESSIONS, 8, BY_VOICE)).unwrap();
        drop(held);
        take_back_at(&face, FACE_EXPRESSIONS).unwrap();
        assert!(!face.exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_rev_is_1_to_64_printable_ascii_characters() {
        let longest = "a".repeat(64);
        for rev in ["a", "!~", "gone-3f9c1a2b", "none", longest.as_str()] {
            assert_eq!(fit_rev(Some(&Value::from(rev))), Ok(rev));
        }
        for rev in [
            None,
            Some(Value::Null),
            Some(Value::from(7)),
            Some(Value::from("")),
            Some(Value::from("a".repeat(65))),
            Some(Value::from("a b")),
            Some(Value::from("tap\n")),
            Some(Value::from("tap\u{7f}")),
            Some(Value::from("café")),
        ] {
            assert_eq!(fit_rev(rev.as_ref()), Err(REV_REFUSED), "{rev:?}");
        }
    }
}
