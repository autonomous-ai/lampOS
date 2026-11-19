//! The yeses another part reads (#5500, `docs/onboarding.md`, "Face
//! expressions" and "Room sounds"): face expressions for lamp-eyes, on a
//! lamp with no privacy slide room sounds for lamp-ears, and with
//! `LAMP_SEEN_ACTIVITY=on` the owner's yes to seen activity for lamp-seen
//! (#6543), given by voice at any time, not at onboarding. Each is a file in
//! [`FOLDER`], present only while the yes stands:
//! `{"version":1,KEY:"yes","at_unix_s":N,"by":BY}`, `root:lampos` 0640.
//!
//! The voice writes one on a spoken yes (`"by":"voice"`), and
//! root's helper (`lamp-setup change`) on the owner's switch in the app
//! (`"by":"app"`, `consent.set` with `LAMP_APP_CONSENT=on`). One copy of the
//! names and the file's shape, read by `lamp-cloud`, the helper and the
//! voice. The readers (`lamp_eyes::seems`, `lamp_ears::classes`,
//! `lamp_seen::opt_in`, [`crate::Faces::for_skills`]) read only `KEY`, all
//! with one rule, [`allowed`] (#7767): a kept `"yes"` is on, a kept `"no"`
//! is off, a missing file is on only with [`NOTICING_BY_DEFAULT`] (read in
//! one place, [`noticing_by_default`]), and a file that can't be read is
//! off. A file from before `by` is the voice's.
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The switch on `lamp-cloud`: `on` exactly takes [`KINDS`] from the app.
pub const ENVIRONMENT: &str = "LAMP_APP_CONSENT";

/// The app's kinds: `consent.get` (`{}`) and `consent.set` (one key).
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
}

impl Kept {
    /// Given in the app.
    pub fn by_app(&self) -> bool {
        self.by.as_deref() == Some(BY_APP)
    }
}

/// The switch that reads a missing yes file as on (#7767): `on` exactly
/// makes the face expressions yes, the room sounds yes, seen activity and
/// health and crash reports default on. A kept `"no"` stays no and a file
/// that can't be read stays off either way; off, a missing file is no, as
/// before. Off unless set, in no unit (`docs/switches.md`).
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

/// `path`'s yes for `key` (#7767): one rule for a kept yes, a kept no and
/// the default.
///
/// - `KEY` exactly `"yes"`: a yes, with who gave it and when.
/// - No file at `path` (not found): `by_default`, as [`BY_DEFAULT`]'s yes
///   with no time, or no yes.
/// - `KEY` exactly `"no"` ([`no_bytes`]): no, whatever `by_default` says.
/// - Unreadable (any open error but not found, or a read error), over
///   [`MOST_BYTES`], not JSON, not an object, or an object with anything
///   else under `KEY`: no, the safe side.
pub fn read_or(path: &Path, key: &str, by_default: bool) -> Kept {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && by_default => {
            return Kept {
                yes: true,
                by: Some(BY_DEFAULT.to_owned()),
                at_unix_s: None,
                no: false,
                by_default: true,
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

/// A kept file's bytes, whole, with `KEY` as `answer`.
fn answer_bytes(key: &str, answer: &str, at_unix_s: u64, by: &str) -> Vec<u8> {
    let mut object = serde_json::Map::new();
    object.insert("version".into(), 1.into());
    object.insert(key.into(), answer.into());
    object.insert("at_unix_s".into(), at_unix_s.into());
    object.insert("by".into(), by.into());
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
    answer_bytes(key, "yes", at_unix_s, by)
}

/// A kept no's bytes, whole: `{"version":1,KEY:"no","at_unix_s":N,"by":BY}`
/// (#7767). Kept, not removed, so that [`NOTICING_BY_DEFAULT`] never turns
/// a person's no back on.
pub fn no_bytes(key: &str, at_unix_s: u64, by: &str) -> Vec<u8> {
    answer_bytes(key, "no", at_unix_s, by)
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
        // Missing: the default, only with the switch on.
        assert!(!allowed(&file, SEEN_ACTIVITY, false), "missing, off");
        assert!(allowed(&file, SEEN_ACTIVITY, true), "missing, on");
        assert_eq!(
            read_or(&file, SEEN_ACTIVITY, true),
            Kept {
                yes: true,
                by: Some(BY_DEFAULT.into()),
                at_unix_s: None,
                no: false,
                by_default: true,
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
}
