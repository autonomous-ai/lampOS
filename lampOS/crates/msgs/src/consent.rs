//! The two yeses another part reads (#5500, `docs/onboarding.md`, "Face
//! expressions" and "Room sounds"): face expressions for lamp-eyes and, on a
//! lamp with no privacy slide, room sounds for lamp-ears. Each is a file in
//! [`FOLDER`], present only while the yes stands:
//! `{"version":1,KEY:"yes","at_unix_s":N,"by":BY,"rev":REV}`, `root:lampos`
//! 0640. Every change to one is made holding the folder ([`hold`]), a fresh
//! revision kept first in [`STATE_FILE`], so the app's `consent.set` is a
//! compare-and-set on [`rev`].
//!
//! The voice's onboarding writes one on a spoken yes (`"by":"voice"`), and
//! root's helper (`lamp-setup change`) on the owner's switch in the app
//! (`"by":"app"`, `consent.set` with `LAMP_APP_CONSENT=on`). One copy of the
//! names and the file's shape, read by `lamp-cloud`, the helper and the
//! voice. The readers (`lamp_eyes::seems`, `lamp_ears::classes`) read only
//! `KEY` and keep their own copy of the rule; a file from before `by` is the
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

/// The keys, each with its file's name in [`FOLDER`].
pub const FILES: [(&str, &str); 2] = [
    (FACE_EXPRESSIONS, "face-expressions.json"),
    (ROOM_SOUNDS, "room-sounds.json"),
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

/// Before the time of a yes kept with no revision (`yes-AT_UNIX_S`): the
/// voice's spoken yes at onboarding, or an app yes from before revisions.
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
/// - a yes with no revision (the voice's): `yes-AT_UNIX_S` (`0` with no
///   time);
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
    if kept.yes {
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
/// set matches.
pub fn snapshot(folder: &Path, keys: &[&str]) -> Vec<(String, bool, String)> {
    let mut said = Vec::new();
    for _ in 0..SNAPSHOT_TRIES {
        let revs = Revs::read(folder);
        said = keys
            .iter()
            .map(|key| {
                let kept = file_in(folder, key)
                    .map(|file| read(&file, key))
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
    /// before anything else changes.
    pub fn bump(&self, key: &str) -> std::io::Result<String> {
        let mut wrote = self.revs()?;
        let fresh = fresh_rev()?;
        wrote.insert(key.to_owned(), fresh.clone());
        keep_in_folder(&self.folder.join(STATE_FILE), &state_bytes(&wrote))?;
        Ok(fresh)
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
        let fresh = self.bump(key)?;
        keep_with_group(path, &bytes_with_rev(key, at_unix_s, by, &fresh), group)?;
        Ok(fresh)
    }

    /// `path` is a file in the folder held.
    fn within(&self, path: &Path) -> std::io::Result<()> {
        if path.parent() == Some(self.folder.as_path()) {
            Ok(())
        } else {
            Err(std::io::Error::other("a yes outside the folder held"))
        }
    }

    /// `key`'s yes taken back, whoever gave it: a fresh revision in the
    /// state first, then the file removed, durably. `key`'s revision then,
    /// `gone-REV`.
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
        let fresh = self.bump(key).map_err(refused)?;
        remove_in_folder(path).map_err(refused)?;
        Ok(format!("{GONE}{fresh}"))
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
    /// The helper's revision of it ([`rev`]), if written with one.
    pub rev: Option<String>,
}

impl Kept {
    /// Given in the app.
    pub fn by_app(&self) -> bool {
        self.by.as_deref() == Some(BY_APP)
    }
}

/// `path`'s yes for `key`. Missing, unreadable, over [`MOST_BYTES`], not
/// JSON or not an object is no yes, with nothing else said.
pub fn read(path: &Path, key: &str) -> Kept {
    let Ok(file) = std::fs::File::open(path) else {
        return Kept::default();
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
    Kept {
        yes: value.get(key).and_then(Value::as_str) == Some("yes"),
        by: value.get("by").and_then(Value::as_str).map(str::to_owned),
        at_unix_s: value.get("at_unix_s").and_then(Value::as_u64),
        rev: value.get("rev").and_then(Value::as_str).map(str::to_owned),
    }
}

/// A yes file's bytes, whole: `{"version":1,KEY:"yes","at_unix_s":N,"by":BY}`.
pub fn bytes(key: &str, at_unix_s: u64, by: &str) -> Vec<u8> {
    bytes_with(key, at_unix_s, by, None)
}

/// A yes file's bytes as the helper writes them, with its revision:
/// `{"version":1,KEY:"yes","at_unix_s":N,"by":"app","rev":REV}`. The readers
/// read only `KEY`, so the revision changes nothing they read.
pub fn bytes_with_rev(key: &str, at_unix_s: u64, by: &str, rev: &str) -> Vec<u8> {
    bytes_with(key, at_unix_s, by, Some(rev))
}

fn bytes_with(key: &str, at_unix_s: u64, by: &str, rev: Option<&str>) -> Vec<u8> {
    let mut object = serde_json::Map::new();
    object.insert("version".into(), 1.into());
    object.insert(key.into(), "yes".into());
    object.insert("at_unix_s".into(), at_unix_s.into());
    object.insert("by".into(), by.into());
    if let Some(rev) = rev {
        object.insert("rev".into(), rev.into());
    }
    serde_json::to_vec(&Value::Object(object)).unwrap_or_default()
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
        let _ = std::fs::remove_dir_all(folder);
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
