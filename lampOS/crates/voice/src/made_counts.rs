//! Made-skill asks by voice, counted for `lamp-cloud`'s opt-in health
//! report (`docs/cloud.md`, "Skills, counted"; the owner's ask on #4710,
//! 3 Nov): a skill made by voice never reaches the backend, so the lamp
//! says how many were asked for and what became of them. Numbers only,
//! kept in [`FILE`] beside the made skills, in their root-only folder; no
//! name, word or file of a skill is in it.
//!
//! - `kept`, `on` and `left_off` are read from the made skills' own index
//!   ([`crate::made_skill::INDEX`]) when `lamp-cloud` asks ([`counts`]): a
//!   skill made by voice (its `from` is not the app's, [`is_app`]) counts
//!   once as kept, once as on when a spoken yes put it on, and once as
//!   left off when it was asked about and is no longer the offer a yes
//!   would put on. Each is remembered by its SHA-256 in [`FILE`], so it is
//!   counted once.
//! - `refused`, an ask that made nothing, is never in the index: it is
//!   counted where it is told ([`refused`]), on an atomic, and added to
//!   [`FILE`] at most every [`FLUSH_EVERY`] by a thread of its own, or at
//!   the next ask in the same process. A voice turn never waits on a file.
//!
//! The four are running totals; `lamp-cloud` makes a day's counts from two
//! readings. Every writer holds [`LOCK`], so the worker's flush, the
//! socket's read and [`forget`] never undo each other. When the person says
//! no to reports, the manage helper deletes [`FILE`] ([`forget`], asked by
//! `lamp-cloud`): the count begins again from nothing.
//!
//! Whether a made skill is the app's or the voice's is on its own card
//! (`made_by`, [`made_skill::Maker`]), where `lamp-skills` reads it: this
//! file and its answer name no package.
use crate::made_skill;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

/// The totals, beside the made skills' index.
pub const FILE: &str = "counts.json";
/// What every writer of [`FILE`] holds across its read and write.
const LOCK: &str = ".counts.lock";
/// The most often a process adds its refused asks to [`FILE`].
pub const FLUSH_EVERY: Duration = Duration::from_secs(60);
/// The SHA-256s remembered for each count, the newest: more than the made
/// skills ever kept at once ([`made_skill::KEPT_MOST`]).
const MOST_SEEN: usize = 32;

/// Refused asks in this process not yet in [`FILE`].
static REFUSED: AtomicU64 = AtomicU64::new(0);
/// The folder this process's flush writes to, once it has begun.
static FLUSHING: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

/// [`FILE`]'s contents.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Totals {
    pub kept: u64,
    pub on: u64,
    pub refused: u64,
    pub left_off: u64,
    /// The SHA-256s counted, so each is counted once.
    pub seen: Seen,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Seen {
    pub kept: Vec<String>,
    pub on: Vec<String>,
    pub left_off: Vec<String>,
}

/// Whether a made skill's `from` is an app's ask (`local:app:…`, or a
/// written one's `written:app:…`): [`made_skill::Maker::of`], as its card
/// says it.
pub fn is_app(from: &str) -> bool {
    made_skill::Maker::of(from) == made_skill::Maker::App
}

/// One ask by voice that made nothing, counted: an atomic add, and the
/// flush begun for `directory` the first time, on a thread of its own.
pub fn refused(directory: &Path) {
    REFUSED.fetch_add(1, Ordering::Relaxed);
    let flushing = FLUSHING.get_or_init(|| Mutex::new(None));
    let mut place = flushing.lock().unwrap_or_else(|e| e.into_inner());
    if place.is_some() {
        return;
    }
    let directory = directory.to_path_buf();
    *place = Some(directory.clone());
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-made-counts".into())
        .spawn(move || {
            loop {
                std::thread::sleep(FLUSH_EVERY);
                if let Err(error) = locked(&directory, |_| ()) {
                    eprintln!("lamp-voice: made-skill counts not kept: {error}");
                }
            }
        });
    if spawned.is_err() {
        // Tried again at the next refused ask.
        *place = None;
    }
}

/// The person said no to reports: [`FILE`] deleted under [`LOCK`] (by the
/// manage helper, root, as the voice is). Refused asks counted in a running
/// voice's memory and not yet written (at most [`FLUSH_EVERY`]'s) begin the
/// next file; none is ever sent without a yes, and a yes's first reading
/// only begins the count. A folder that isn't there has nothing to forget.
pub fn forget(directory: &Path) -> io::Result<()> {
    if !directory.is_dir() {
        return Ok(());
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(LOCK))?;
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive)?;
    match fs::remove_file(directory.join(FILE)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// [`FILE`] read under [`LOCK`], this process's refused asks added, let
/// `change` change it, and written whole beside it and moved over it.
fn locked<R>(directory: &Path, change: impl FnOnce(&mut Totals) -> R) -> io::Result<R> {
    made_skill::ensure_private(directory)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(LOCK))?;
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive)?;
    let mut totals: Totals = fs::read(directory.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let before = totals.clone();
    let refused = REFUSED.swap(0, Ordering::Relaxed);
    totals.refused = totals.refused.saturating_add(refused);
    let told = change(&mut totals);
    if totals != before {
        let written = (|| {
            let new = directory.join(format!(".{FILE}.new"));
            let mut file = fs::File::create(&new)?;
            file.write_all(&serde_json::to_vec(&totals).map_err(io::Error::other)?)?;
            file.sync_all()?;
            fs::rename(&new, directory.join(FILE))
        })();
        if let Err(error) = written {
            REFUSED.fetch_add(refused, Ordering::Relaxed);
            return Err(error);
        }
    }
    Ok(told)
}

/// A package's SHA-256, as the made skills' index keeps it: 64 lowercase hex.
fn sha256_ok(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `sha256` counted in `total` once, remembered in `seen`.
fn once(total: &mut u64, seen: &mut Vec<String>, sha256: &str) {
    if seen.iter().any(|known| known == sha256) {
        return;
    }
    *total = total.saturating_add(1);
    seen.push(sha256.to_owned());
    while seen.len() > MOST_SEEN {
        seen.remove(0);
    }
}

/// The socket's `counts` (`crate::app_make`): the index read for what
/// became of the skills made by voice, added to [`FILE`], and told as
/// `{"made_by_voice": {"kept", "on", "refused", "left_off"}}`, the running
/// totals. Only numbers; never a name, a word or a package.
pub fn counts(directory: &Path, now_unix_ms: u64) -> Value {
    let entries = made_skill::entries(directory);
    let waiting = made_skill::waiting(directory, now_unix_ms).map(|(_, sha256)| sha256);
    let from_app = |entry: &Value| entry["from"].as_str().is_some_and(is_app);
    let sha_of = |entry: &Value| {
        entry["sha256"]
            .as_str()
            .filter(|sha256| sha256_ok(sha256))
            .map(str::to_owned)
    };
    let read = locked(directory, |totals| {
        for entry in entries.iter().filter(|entry| !from_app(entry)) {
            let Some(sha256) = sha_of(entry) else {
                continue;
            };
            once(&mut totals.kept, &mut totals.seen.kept, &sha256);
            if entry["on"] == true {
                once(&mut totals.on, &mut totals.seen.on, &sha256);
            } else if entry["offered_at_unix_ms"].as_u64().is_some()
                && waiting.as_deref() != Some(sha256.as_str())
            {
                once(&mut totals.left_off, &mut totals.seen.left_off, &sha256);
            }
        }
        json!({
            "kept": totals.kept,
            "on": totals.on,
            "refused": totals.refused,
            "left_off": totals.left_off,
        })
    });
    match read {
        Ok(made) => json!({"made_by_voice": made}),
        Err(error) => {
            eprintln!("lamp-voice: made-skill counts not read: {error}");
            json!({"refused": "the counts can't be read now"})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The refused count is the process's: one test at a time reads it.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("made-counts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(from: &str, sha: char, extra: Value) -> Value {
        let mut entry = json!({
            "from": from,
            "name": "ann-secret-skill",
            "description": "Dims for Ann at 22:00",
            "sha256": sha.to_string().repeat(64),
            "package": "x.tar",
            "at_unix_ms": 1u64,
        });
        for (key, value) in extra.as_object().unwrap() {
            entry[key] = value.clone();
        }
        entry
    }

    #[test]
    fn what_became_of_skills_made_by_voice_is_counted_once_each() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = folder("once");
        let now = 1_800_000_000_000u64;
        let entries = [
            entry("local:1", 'a', json!({})),
            entry(
                "local:2",
                'b',
                json!({"on": true, "offered_at_unix_ms": now - 5_000}),
            ),
            entry("local:3", 'c', json!({"offered_at_unix_ms": now - 600_000})),
            // An app's ask, made here or written there: never in the
            // voice's counts.
            entry(
                "local:app:9:1",
                'd',
                json!({"on": true, "offered_at_unix_ms": now - 9_000}),
            ),
            entry("written:app:8:1", 'f', json!({"on": true})),
            // The offer a yes would put on now: not left off yet.
            entry("local:4", 'e', json!({"offered_at_unix_ms": now - 1_000})),
        ];
        fs::write(
            dir.join(made_skill::INDEX),
            serde_json::to_vec(&entries).unwrap(),
        )
        .unwrap();
        let told = counts(&dir, now);
        let made = &told["made_by_voice"];
        assert_eq!(
            (&made["kept"], &made["on"], &made["left_off"]),
            (&json!(4), &json!(1), &json!(1)),
            "{told}"
        );
        assert_eq!(told.get("app"), None, "{told}");
        // Asked again: nothing counted twice.
        let again = counts(&dir, now);
        for count in ["kept", "on", "left_off"] {
            assert_eq!(again["made_by_voice"][count], made[count], "{again}");
        }
        // The offer's time ends: left off once more.
        let later = counts(&dir, now + 200_000);
        assert_eq!(later["made_by_voice"]["left_off"], 2, "{later}");
        let text = fs::read_to_string(dir.join(FILE)).unwrap() + &later.to_string();
        for secret in ["ann", "Ann", "22:00", "x.tar"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
    }

    #[test]
    fn a_refused_ask_is_counted_without_a_file_and_kept_at_the_next_read() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = folder("refused");
        // Two refused asks: counted in memory alone, at once.
        // Other tests' refused asks may be counted meanwhile: at least two.
        refused(&dir);
        refused(&dir);
        assert!(REFUSED.load(Ordering::Relaxed) >= 2);
        assert!(!dir.join(FILE).exists());
        let told = counts(&dir, 1);
        assert!(
            told["made_by_voice"]["refused"].as_u64().unwrap() >= 2,
            "{told}"
        );
        // A no forgets the file; the next read begins again.
        forget(&dir).unwrap();
        assert!(!dir.join(FILE).exists());
        forget(&dir).unwrap();
        forget(&dir.join("none")).unwrap();
    }
}
