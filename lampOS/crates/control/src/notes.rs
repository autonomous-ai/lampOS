//! A paired computer's own notes (docs/control.md, "Notes"): a small
//! key/value memory the lamp keeps for it, so an app's state outlives its
//! own restart and the lamp's. `notes.set` keeps a JSON value under a key,
//! `notes.get` reads it, `notes.delete` drops it and `notes.list` lists the
//! computer's own keys.
//!
//! The bounds are the lamp's, not a policy; a request past one is refused
//! whole, nothing partly kept:
//!
//! - **A key:** 1 to [`KEY_MOST_BYTES`] bytes of `A-Z a-z 0-9 . _ : -`.
//! - **A value:** any JSON, at most [`VALUE_MOST_BYTES`] serialized.
//! - **A computer's notes:** at most [`KEYS_MOST`] keys and
//!   [`TOTAL_MOST_BYTES`] of keys and values in all.
//! - **Isolated:** the computer is always the paired id from `hello`, never
//!   a field of the request, so no app can name another's keys.
//! - **Kept:** one file for each computer, `<folder>/<client>.json`, written
//!   whole (a temporary file, synced, then renamed over the old), so a crash
//!   leaves the old notes or the new, never half. A file that can't be read
//!   is set aside as `<client>.json.bad` and the notes start empty.
//! - **A revoke deletes them** ([`Notes::forget`]): nothing of a computer
//!   the owner let go stays on the lamp.
//!
//! The journal says counts and sizes, never a key or a value.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::protocol::{Error, ErrorCode};
use crate::session::Pending;

/// The folder in the state folder the notes are kept in.
pub const NOTES_FOLDER: &str = "notes";
/// A key's longest, in bytes.
pub const KEY_MOST_BYTES: usize = 64;
/// A value's largest, serialized.
pub const VALUE_MOST_BYTES: usize = 16 * 1024;
/// The most keys one computer keeps.
pub const KEYS_MOST: usize = 256;
/// The most bytes one computer's keys and values take in all.
pub const TOTAL_MOST_BYTES: usize = 256 * 1024;
/// The file's own format version.
const FILE_VERSION: u64 = 1;

/// What a write did at the door: nothing to wait for, or the disk write to
/// run off the door's lock (two fsyncs took 11 to 15 ms on lamp-75c9, 10 Nov:
/// too long to stall every other computer's request).
pub enum Step {
    Now(Value),
    Later(Pending),
}

impl std::fmt::Debug for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Now(answer) => write!(f, "Now({answer})"),
            Self::Later(_) => write!(f, "Later"),
        }
    }
}

impl Step {
    /// The answer, running the write here if there is one.
    pub fn wait(self) -> Result<Value, Error> {
        match self {
            Self::Now(answer) => Ok(answer),
            Self::Later(run) => run(),
        }
    }
}

/// What is shared between the door and the writes running off its lock.
#[derive(Default)]
struct Inner {
    /// Each computer's notes, as last kept on disk.
    kept: HashMap<String, BTreeMap<String, Value>>,
    /// The computers with a write being kept now: one at a time each, so
    /// files are written in the order the changes were made.
    writing: HashSet<String>,
    /// How many times each computer was forgotten: a write that finds it
    /// changed was let go while it ran, and keeps nothing.
    epoch: HashMap<String, u64>,
}

/// The computers' notes: where they're kept, and what is read of them.
pub struct Notes {
    folder: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

impl Notes {
    /// The notes kept under `folder` (the state folder's [`NOTES_FOLDER`]).
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            inner: Arc::default(),
        }
    }

    /// Where `client`'s notes are kept, whether or not any are.
    pub fn path(folder: &Path, client: &str) -> Option<PathBuf> {
        crate::sounds::safe_client(client).then(|| folder.join(format!("{client}.json")))
    }

    /// `notes.get`: the value under `key`, if any.
    pub fn get(&self, client: &str, key: &str) -> Result<Option<Value>, Error> {
        valid_key(key)?;
        let mut inner = self.inner.lock().expect("notes");
        Ok(self.room(&mut inner, client)?.get(key).cloned())
    }

    /// `notes.set`: `value` kept under `key`, over one of the same key. The
    /// checks are made now; the write is the step's, off the door's lock, and
    /// the answer (`key`, and the bytes the value takes) comes when it is
    /// kept. Refused as busy while this computer's last write is being kept.
    pub fn set(&self, client: &str, key: &str, value: &Value) -> Result<Step, Error> {
        valid_key(key)?;
        let bytes = serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len());
        if bytes > VALUE_MOST_BYTES {
            return Err(Error::new(
                ErrorCode::BadRequest,
                format!("value: {bytes} bytes, over the {VALUE_MOST_BYTES} one note may take"),
            ));
        }
        let mut inner = self.inner.lock().expect("notes");
        let room = self.room(&mut inner, client)?.clone();
        let before = room.get(key).map(|old| entry_bytes(key, old));
        let (keys, total) = (
            room.len() + usize::from(before.is_none()),
            held(&room) - before.unwrap_or(0) + key.len() + bytes,
        );
        if keys > KEYS_MOST || total > TOTAL_MOST_BYTES {
            return Err(Error::new(
                ErrorCode::Refused,
                format!(
                    "this computer's notes are full: it keeps {KEYS_MOST} keys at most and {TOTAL_MOST_BYTES} bytes in all (now {} keys, {} bytes)",
                    room.len(),
                    held(&room)
                ),
            ));
        }
        let mut after = room;
        after.insert(key.to_owned(), value.clone());
        let answer = json!({"key": key, "bytes": bytes});
        self.keep(&mut inner, client, after, answer)
    }

    /// `notes.delete`: the key dropped; `deleted` says whether it was there.
    /// A key not there is answered at once, nothing written.
    pub fn delete(&self, client: &str, key: &str) -> Result<Step, Error> {
        valid_key(key)?;
        let mut inner = self.inner.lock().expect("notes");
        // Busy first: a key absent only because this computer's own set of
        // it is still being kept would answer "deleted: false" and leave the
        // key there when the set lands (eighth's P2 on banana/store).
        if inner.writing.contains(client) {
            return Err(busy());
        }
        let mut after = self.room(&mut inner, client)?.clone();
        if after.remove(key).is_none() {
            return Ok(Step::Now(json!({"key": key, "deleted": false})));
        }
        self.keep(
            &mut inner,
            client,
            after,
            json!({"key": key, "deleted": true}),
        )
    }

    /// `notes.list`: `client`'s keys that start with `prefix`, sorted, each
    /// with the bytes of its value, and what the whole holds against the caps.
    pub fn list(&self, client: &str, prefix: &str) -> Result<Value, Error> {
        if prefix.len() > KEY_MOST_BYTES || !prefix.bytes().all(key_byte) {
            return Err(bad_key("prefix"));
        }
        let mut inner = self.inner.lock().expect("notes");
        let room = self.room(&mut inner, client)?;
        let keys: Vec<Value> = room
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, value)| json!({"key": key, "bytes": value_bytes(value)}))
            .collect();
        Ok(json!({
            "keys": keys,
            "count": keys.len(),
            "total": room.len(),
            "bytes": held(room),
            "keys_max": KEYS_MOST,
            "bytes_max": TOTAL_MOST_BYTES,
        }))
    }

    /// `client` revoked: every note of it deleted, in memory and on disk, and
    /// a write being kept for it keeps nothing. Said in the journal if a file
    /// can't be removed.
    pub fn forget(&self, client: &str) {
        let mut inner = self.inner.lock().expect("notes");
        inner.kept.remove(client);
        *inner.epoch.entry(client.to_owned()).or_default() += 1;
        remove_files(&self.folder, client);
    }

    /// `client`'s notes, read from their file the first time they are asked
    /// for.
    fn room<'a>(
        &self,
        inner: &'a mut Inner,
        client: &str,
    ) -> Result<&'a mut BTreeMap<String, Value>, Error> {
        let Some(file) = Self::path(&self.folder, client) else {
            return Err(Error::new(
                ErrorCode::Refused,
                "this computer's id can't name a file",
            ));
        };
        if !inner.kept.contains_key(client) {
            inner.kept.insert(client.to_owned(), read_file(&file));
        }
        Ok(inner.kept.get_mut(client).expect("just read"))
    }

    /// The write of `after` as `client`'s notes, to run off the door's lock:
    /// kept on disk first, in memory once it is, so a failed write changes
    /// nothing and a read in between sees the notes as they were.
    fn keep(
        &self,
        inner: &mut Inner,
        client: &str,
        after: BTreeMap<String, Value>,
        answer: Value,
    ) -> Result<Step, Error> {
        if !inner.writing.insert(client.to_owned()) {
            return Err(busy());
        }
        let (folder, shared, client) = (self.folder.clone(), self.inner.clone(), client.to_owned());
        let epoch = inner.epoch.get(&client).copied().unwrap_or(0);
        // Cleared when the write ends however it does: finished, failed,
        // panicked, or never run (eighth's P3 on banana/store).
        let writing = Writing {
            shared: shared.clone(),
            client: client.clone(),
        };
        Ok(Step::Later(Box::new(move || {
            let _writing = writing;
            let written = guarded(|| save(&folder, &client, &after));
            let mut inner = shared.lock().expect("notes");
            if inner.epoch.get(&client).copied().unwrap_or(0) != epoch {
                // Let go while it was written: not a file of it stays, nor
                // a room read in the meantime.
                inner.kept.remove(&client);
                drop(inner);
                remove_files(&folder, &client);
                return Err(Error::new(
                    ErrorCode::Refused,
                    "this computer was let go before its note was kept",
                ));
            }
            written?;
            inner.kept.insert(client, after);
            Ok(answer)
        })))
    }
}

/// A computer's write being kept: clears its mark when it is dropped.
struct Writing {
    shared: Arc<Mutex<Inner>>,
    client: String,
}

impl Drop for Writing {
    fn drop(&mut self) {
        // A write that panicked poisoned the lock; the mark still goes.
        let mut inner = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.writing.remove(&self.client);
    }
}

fn busy() -> Error {
    Error::new(
        ErrorCode::Busy,
        "this computer's last note is still being kept",
    )
}

/// The notes, the unreadable file set aside from them (`.bad`) and a write cut
/// short by a power cut (`.tmp`): nothing of a let-go computer stays.
fn remove_files(folder: &Path, client: &str) {
    let Some(file) = Notes::path(folder, client) else {
        return;
    };
    for file in [
        file.clone(),
        file.with_extension("json.bad"),
        file.with_extension("json.tmp"),
    ] {
        match std::fs::remove_file(&file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                eprintln!("lamp-control: removing a revoked computer's notes: {error}");
            }
            _ => {}
        }
    }
}

/// A write step run so that a panic in it is an answer, not a lost one: the
/// asker gets `refused`, and the journal's stderr says that it panicked, never
/// what the note held (nine's P3 on #6022).
fn guarded(step: impl FnOnce() -> Result<(), Error>) -> Result<(), Error> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(step)).unwrap_or_else(|_| {
        eprintln!("lamp-control: notes: a write panicked");
        Err(Error::new(
            ErrorCode::Refused,
            "the lamp could not keep the note",
        ))
    })
}

/// `client`'s notes written whole: a temporary file, synced, renamed over the
/// old.
fn save(folder: &Path, client: &str, room: &BTreeMap<String, Value>) -> Result<(), Error> {
    let failed = |error: std::io::Error| {
        eprintln!("lamp-control: notes: {error}");
        Error::new(ErrorCode::Refused, "the lamp could not keep the note")
    };
    let file = Notes::path(folder, client).expect("a safe id");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)
        .map_err(failed)?;
    let text = serde_json::to_vec(&json!({"v": FILE_VERSION, "keys": room}))
        .map_err(|error| failed(error.into()))?;
    let temporary = file.with_extension("json.tmp");
    let written = (|| {
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        out.write_all(&text)?;
        out.sync_all()?;
        std::fs::rename(&temporary, &file)
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(failed(error));
    }
    // The rename itself is kept past a power cut once its folder is.
    if let Ok(folder) = std::fs::File::open(folder) {
        let _ = folder.sync_all();
    }
    Ok(())
}

/// A key's or a prefix's byte.
fn key_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
}

fn bad_key(what: &str) -> Error {
    Error::new(
        ErrorCode::BadRequest,
        format!("{what}: up to {KEY_MOST_BYTES} bytes of letters, digits and . _ : -"),
    )
}

fn valid_key(key: &str) -> Result<(), Error> {
    if key.is_empty() || key.len() > KEY_MOST_BYTES || !key.bytes().all(key_byte) {
        return Err(bad_key("key"));
    }
    Ok(())
}

fn value_bytes(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(0, |bytes| bytes.len())
}

fn entry_bytes(key: &str, value: &Value) -> usize {
    key.len() + value_bytes(value)
}

/// What a computer's keys and values take in all.
fn held(room: &BTreeMap<String, Value>) -> usize {
    room.iter()
        .map(|(key, value)| entry_bytes(key, value))
        .sum()
}

/// A computer's notes from their file: none if there is no file, and none
/// (the file set aside as `.bad`, said without a word of it) if it can't be
/// read or is another version's.
fn read_file(file: &Path) -> BTreeMap<String, Value> {
    let Ok(bytes) = std::fs::read(file) else {
        return BTreeMap::new();
    };
    let parsed: Option<BTreeMap<String, Value>> = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .filter(|whole| whole["v"] == FILE_VERSION)
        .and_then(|whole| serde_json::from_value(whole["keys"].clone()).ok())
        .filter(|room: &BTreeMap<String, Value>| room.keys().all(|key| valid_key(key).is_ok()));
    parsed.unwrap_or_else(|| {
        let aside = file.with_extension("json.bad");
        eprintln!("lamp-control: a computer's notes file can't be read; set aside");
        let _ = std::fs::rename(file, aside);
        BTreeMap::new()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);
    impl Dir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("lamp-control-notes-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The writes as the tests want them: done, and the answer read.
    trait Done {
        fn put(&self, client: &str, key: &str, value: &Value) -> Result<usize, Error>;
        fn del(&self, client: &str, key: &str) -> Result<bool, Error>;
    }
    impl Done for Notes {
        fn put(&self, client: &str, key: &str, value: &Value) -> Result<usize, Error> {
            let answer = self.set(client, key, value)?.wait()?;
            Ok(answer["bytes"].as_u64().expect("bytes") as usize)
        }
        fn del(&self, client: &str, key: &str) -> Result<bool, Error> {
            Ok(self.delete(client, key)?.wait()?["deleted"] == true)
        }
    }

    const A: &str = "c_aaaa";
    const B: &str = "c_bbbb";

    fn refused(error: Error) -> String {
        assert_eq!(error.code, ErrorCode::Refused, "{}", error.why);
        error.why
    }

    #[test]
    fn a_note_is_set_read_listed_and_deleted() {
        let dir = Dir::new("basic");
        let notes = Notes::new(dir.0.clone());
        assert_eq!(notes.get(A, "mood").unwrap(), None);
        assert_eq!(
            notes.put(A, "mood", &json!({"calm": [1, 2, 3]})).unwrap(),
            16
        );
        assert_eq!(
            notes.get(A, "mood").unwrap(),
            Some(json!({"calm": [1, 2, 3]}))
        );
        // Over a key of the same name: one key, the new value.
        notes.put(A, "mood", &json!("bright")).unwrap();
        notes.put(A, "mode", &json!(7)).unwrap();
        notes.put(A, "other", &json!(null)).unwrap();
        assert_eq!(notes.get(A, "mood").unwrap(), Some(json!("bright")));
        let listed = notes.list(A, "mo").unwrap();
        assert_eq!(
            listed["keys"],
            json!([{"key": "mode", "bytes": 1}, {"key": "mood", "bytes": 8}])
        );
        assert_eq!(listed["count"], 2);
        assert_eq!(listed["total"], 3);
        assert_eq!(listed["keys_max"], KEYS_MOST);
        assert_eq!(listed["bytes_max"], TOTAL_MOST_BYTES);
        // The whole, not the prefix: 3 keys, their keys' and values' bytes.
        let all = notes.list(A, "").unwrap();
        assert_eq!(all["count"], 3);
        assert_eq!(all["bytes"], (4 + 8) + (4 + 1) + (5 + 4));
        assert!(notes.del(A, "mood").unwrap());
        assert!(!notes.del(A, "mood").unwrap());
        assert_eq!(notes.get(A, "mood").unwrap(), None);
    }

    #[test]
    fn one_computer_never_sees_anothers_notes() {
        let dir = Dir::new("isolated");
        let notes = Notes::new(dir.0.clone());
        notes.put(A, "secret", &json!("a")).unwrap();
        notes.put(B, "secret", &json!("b")).unwrap();
        assert_eq!(notes.get(A, "secret").unwrap(), Some(json!("a")));
        assert_eq!(notes.get(B, "secret").unwrap(), Some(json!("b")));
        notes.del(A, "secret").unwrap();
        assert_eq!(notes.get(B, "secret").unwrap(), Some(json!("b")));
        assert_eq!(notes.list(A, "").unwrap()["count"], 0);
        // An id that could name a path is refused, never joined to one.
        for bad in ["", "../x", "a/b", "c_\u{e9}"] {
            let error = notes.put(bad, "k", &json!(1)).unwrap_err();
            assert!(refused(error).contains("id"), "{bad:?}");
            assert!(notes.get(bad, "k").is_err(), "{bad:?}");
        }
    }

    #[test]
    fn keys_and_values_are_held_to_their_bounds_and_nothing_is_half_kept() {
        let dir = Dir::new("bounds");
        let notes = Notes::new(dir.0.clone());
        for key in [
            "",
            "has space",
            "slash/key",
            "dot\u{e9}",
            &"k".repeat(KEY_MOST_BYTES + 1),
        ] {
            let error = notes.put(A, key, &json!(1)).unwrap_err();
            assert_eq!(error.code, ErrorCode::BadRequest, "{key:?}");
        }
        notes
            .put(A, &"k".repeat(KEY_MOST_BYTES), &json!(1))
            .unwrap();
        notes.put(A, "a.b_c:d-E9", &json!(1)).unwrap();
        // A value at its largest is kept, one byte more is refused.
        let fits = json!("x".repeat(VALUE_MOST_BYTES - 2));
        assert_eq!(notes.put(A, "big", &fits).unwrap(), VALUE_MOST_BYTES);
        let over = json!("x".repeat(VALUE_MOST_BYTES - 1));
        // The value past its bound is a request that can't be kept, as a bad
        // key is: `bad_request` (the lead's P3 on banana/store); a full store
        // is `refused`.
        let error = notes.put(A, "big", &over).unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert!(error.why.contains("16384"), "{}", error.why);
        assert_eq!(notes.get(A, "big").unwrap(), Some(fits));
        // A bad prefix is a bad request, not an empty list.
        assert!(notes.list(A, "no/slash").is_err());
    }

    #[test]
    fn a_computer_keeps_at_most_so_many_keys_and_bytes_and_replacing_counts_the_difference() {
        let dir = Dir::new("caps");
        let notes = Notes::new(dir.0.clone());
        for at in 0..KEYS_MOST {
            notes.put(A, &format!("k{at:04}"), &json!(at)).unwrap();
        }
        let why = refused(notes.put(A, "one-more", &json!(1)).unwrap_err());
        assert!(why.contains("full") && why.contains("256 keys"), "{why}");
        assert_eq!(notes.get(A, "one-more").unwrap(), None);
        // A key already there can be replaced.
        notes.put(A, "k0000", &json!("changed")).unwrap();
        // Another computer has its own room.
        notes.put(B, "one-more", &json!(1)).unwrap();
        // Bytes: fill a computer's room with large values.
        let notes = Notes::new(dir.0.join("bytes"));
        let value = json!("y".repeat(VALUE_MOST_BYTES - 2));
        let fits = TOTAL_MOST_BYTES / VALUE_MOST_BYTES;
        for at in 0..fits - 1 {
            notes.put(A, &format!("v{at}"), &value).unwrap();
        }
        let why = refused(notes.put(A, "last", &value).unwrap_err());
        assert!(why.contains("full") && why.contains("bytes"), "{why}");
        // Replacing one with a smaller value always fits; with one the same
        // size it fits too (the difference is nothing).
        notes.put(A, "v0", &value).unwrap();
        notes.put(A, "v0", &json!("small")).unwrap();
    }

    #[test]
    fn notes_are_kept_across_a_restart_and_a_revoke_deletes_them() {
        let dir = Dir::new("restart");
        {
            let notes = Notes::new(dir.0.clone());
            notes.put(A, "name", &json!("lamp")).unwrap();
            notes.put(B, "name", &json!("other")).unwrap();
            notes.del(A, "gone").unwrap();
        }
        let file = Notes::path(&dir.0, A).unwrap();
        assert!(file.is_file());
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let notes = Notes::new(dir.0.clone());
        assert_eq!(notes.get(A, "name").unwrap(), Some(json!("lamp")));
        notes.forget(A);
        assert!(!file.exists());
        assert_eq!(notes.get(A, "name").unwrap(), None);
        assert_eq!(notes.get(B, "name").unwrap(), Some(json!("other")));
        // And after another restart too.
        let notes = Notes::new(dir.0.clone());
        assert_eq!(notes.get(A, "name").unwrap(), None);
        // Forgetting a computer with none is nothing.
        notes.forget("c_never");
        // A revoke takes the file set aside as unreadable and a write left
        // half-done by a power cut with it: not a word of a let-go computer
        // stays (nine's P2).
        let notes = Notes::new(dir.0.clone());
        let c = Notes::path(&dir.0, "c_cccc").unwrap();
        std::fs::write(&c, b"{not json").unwrap();
        std::fs::write(c.with_extension("json.tmp"), b"half a write").unwrap();
        assert_eq!(notes.get("c_cccc", "k").unwrap(), None);
        assert!(c.with_extension("json.bad").is_file() && c.with_extension("json.tmp").is_file());
        notes.forget("c_cccc");
        for left in ["json", "json.bad", "json.tmp"] {
            assert!(!c.with_extension(left).exists(), "{left}");
        }
        // No temporary file is left behind.
        let left: Vec<_> = std::fs::read_dir(&dir.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(left, [format!("{B}.json")]);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_set_aside_and_the_notes_start_empty() {
        let dir = Dir::new("bad");
        std::fs::create_dir_all(&dir.0).unwrap();
        let file = Notes::path(&dir.0, A).unwrap();
        std::fs::write(&file, b"{not json").unwrap();
        let notes = Notes::new(dir.0.clone());
        assert_eq!(notes.get(A, "k").unwrap(), None);
        assert!(dir.0.join(format!("{A}.json.bad")).is_file());
        notes.put(A, "k", &json!(1)).unwrap();
        let again = Notes::new(dir.0.clone());
        assert_eq!(again.get(A, "k").unwrap(), Some(json!(1)));
        // A file of another version is set aside too, not guessed at.
        std::fs::write(&file, br#"{"v":2,"keys":{"k":1}}"#).unwrap();
        let other = Notes::new(dir.0.clone());
        assert_eq!(other.get(A, "k").unwrap(), None);
    }

    #[test]
    fn a_failed_write_changes_nothing() {
        let dir = Dir::new("unwritable");
        std::fs::create_dir_all(&dir.0).unwrap();
        // The folder is a file, so nothing can be written under it.
        let blocked = dir.0.join("notes");
        std::fs::write(&blocked, b"in the way").unwrap();
        let notes = Notes::new(blocked);
        let why = refused(notes.put(A, "k", &json!(1)).unwrap_err());
        assert!(why.contains("could not keep"), "{why}");
        assert_eq!(notes.get(A, "k").unwrap(), None);
        assert_eq!(notes.list(A, "").unwrap()["count"], 0);
    }

    #[test]
    fn a_second_write_of_the_same_computer_waits_while_one_is_being_kept() {
        let dir = Dir::new("flight");
        let notes = Notes::new(dir.0.clone());
        let first = notes.set(A, "one", &json!(1)).unwrap();
        // Nothing is kept until the write runs: a read sees the notes as
        // they were, and another write of this computer is busy.
        assert_eq!(notes.get(A, "one").unwrap(), None);
        let busy = notes.set(A, "two", &json!(2)).unwrap_err();
        assert_eq!(busy.code, ErrorCode::Busy);
        // A delete of the same key must not answer "not there" while the set
        // of it is still being kept (eighth's P2): busy too.
        assert_eq!(notes.delete(A, "one").unwrap_err().code, ErrorCode::Busy);
        // Another computer's write is not held up by it.
        notes.put(B, "one", &json!("b")).unwrap();
        assert_eq!(first.wait().unwrap(), json!({"key": "one", "bytes": 1}));
        assert_eq!(notes.get(A, "one").unwrap(), Some(json!(1)));
        notes.put(A, "two", &json!(2)).unwrap();
        // A write's file is what memory says after a restart.
        let again = Notes::new(dir.0.clone());
        assert_eq!(again.list(A, "").unwrap()["count"], 2);
        assert_eq!(again.get(B, "one").unwrap(), Some(json!("b")));
    }

    #[test]
    fn a_computer_let_go_while_its_note_is_being_kept_leaves_nothing() {
        let dir = Dir::new("let-go");
        let notes = Notes::new(dir.0.clone());
        notes.put(A, "old", &json!(1)).unwrap();
        notes.put(B, "mine", &json!(1)).unwrap();
        let write = notes.set(A, "new", &json!(2)).unwrap();
        notes.forget(A);
        let error = write.wait().unwrap_err();
        assert!(error.why.contains("let go"), "{}", error.why);
        assert!(!Notes::path(&dir.0, A).unwrap().exists());
        assert!(!dir.0.join(format!("{A}.json.tmp")).exists());
        assert_eq!(notes.get(A, "old").unwrap(), None);
        assert_eq!(notes.get(A, "new").unwrap(), None);
        assert_eq!(notes.get(B, "mine").unwrap(), Some(json!(1)));
    }

    #[test]
    fn a_write_that_fails_lets_the_next_one_in() {
        let dir = Dir::new("failed-flight");
        std::fs::create_dir_all(&dir.0).unwrap();
        let blocked = dir.0.join("notes");
        std::fs::write(&blocked, b"in the way").unwrap();
        let notes = Notes::new(blocked.clone());
        let first = notes.set(A, "k", &json!(1)).unwrap();
        assert!(first.wait().is_err());
        // Not busy any more: the next try is made, and fails the same way.
        let second = notes.set(A, "k", &json!(1)).unwrap();
        assert!(refused(second.wait().unwrap_err()).contains("could not keep"));
        std::fs::remove_file(&blocked).unwrap();
        notes.put(A, "k", &json!(1)).unwrap();
        assert_eq!(notes.get(A, "k").unwrap(), Some(json!(1)));
    }

    #[test]
    fn a_write_never_run_or_dropped_clears_its_mark() {
        let dir = Dir::new("dropped");
        let notes = Notes::new(dir.0.clone());
        let step = notes.set(A, "k", &json!(1)).unwrap();
        assert_eq!(
            notes.set(A, "j", &json!(1)).unwrap_err().code,
            ErrorCode::Busy
        );
        // Never run (the connection went, a thread did not start): the mark
        // goes with the step, so this computer is not busy for ever.
        drop(step);
        notes.put(A, "j", &json!(1)).unwrap();
        assert_eq!(notes.get(A, "k").unwrap(), None);
        assert_eq!(notes.get(A, "j").unwrap(), Some(json!(1)));
    }

    #[test]
    fn a_write_that_panics_answers_refused_and_says_nothing_of_the_note() {
        let error = guarded(|| panic!("secret-value-in-the-panic")).unwrap_err();
        assert_eq!(error.code, ErrorCode::Refused);
        assert!(!format!("{error:?}").contains("secret-value"));
        assert_eq!(guarded(|| Ok(())), Ok(()));
    }
}
