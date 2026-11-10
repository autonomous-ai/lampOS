//! Made-skill asks, by voice and from the app, counted for `lamp-cloud`'s
//! opt-in health report (`docs/cloud.md`, "Skills, counted"; the owner's
//! ask on #4710, 3 Nov): a made skill never reaches the backend, so the
//! lamp says how many were asked for and what became of them, by its
//! [`Maker`]. Both makers are counted here, on the lamp that made them,
//! so the backend never counts the app's make asks itself and nothing is
//! counted twice (the owner, 3 Nov). Numbers only, kept in [`FILE`] beside
//! the made skills, in their root-only folder; no name, word or file of a
//! skill is in it.
//!
//! - `kept`, `on` and `left_off` are read from the made skills' own index
//!   ([`crate::made_skill::INDEX`]) when `lamp-cloud` asks ([`counts`]),
//!   under the maker its `from` says ([`Maker::of`]): a made skill counts
//!   once as kept, once as on when it went on (at once, with no spoken
//!   yes, since the owner's 10 Nov), and once as left off when it couldn't
//!   be put on (its `not_on`), or was asked about before that and never
//!   said yes to. Each is remembered by its SHA-256 in [`FILE`], so it is
//!   counted once.
//! - `refused`, an ask that made nothing, is never in the index: it is
//!   counted where it is told ([`refused`], the voice's turn or the app's
//!   make), on an atomic per maker, and added to [`FILE`] at most every
//!   [`FLUSH_EVERY`] by a thread of its own, or at the next ask in the same
//!   process. A voice turn never waits on a file.
//!
//! The four, per maker, are running totals; `lamp-cloud` makes a day's
//! counts from two readings. Every writer holds [`LOCK`], so the worker's flush, the
//! socket's read and [`forget`] never undo each other. When the person says
//! no to reports, the manage helper deletes [`FILE`] ([`forget`], asked by
//! `lamp-cloud`): the count begins again from nothing.
//!
//! Whether a made skill is the app's or the voice's is on its own card
//! (`made_by`, [`made_skill::Maker`]), where `lamp-skills` reads it: this
//! file and its answer name no package.
use crate::made_skill::{self, Maker};
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

/// Refused asks by voice in this process not yet in [`FILE`].
static REFUSED_VOICE: AtomicU64 = AtomicU64::new(0);
/// Refused asks from the app in this process not yet in [`FILE`].
static REFUSED_APP: AtomicU64 = AtomicU64::new(0);

/// `maker`'s refused asks not yet in [`FILE`].
fn refused_of(maker: Maker) -> &'static AtomicU64 {
    match maker {
        Maker::Voice => &REFUSED_VOICE,
        Maker::App => &REFUSED_APP,
    }
}
/// The folder this process's flush writes to, once it has begun.
static FLUSHING: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

/// One maker's four running totals.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Count {
    pub kept: u64,
    pub on: u64,
    pub refused: u64,
    pub left_off: u64,
}

impl Count {
    fn json(&self) -> Value {
        json!({"kept": self.kept, "on": self.on, "refused": self.refused, "left_off": self.left_off})
    }
}

/// [`FILE`]'s contents.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Totals {
    pub voice: Count,
    pub app: Count,
    /// The SHA-256s counted, so each is counted once; a package's SHA-256
    /// is one maker's alone.
    pub seen: Seen,
}

impl Totals {
    fn of(&mut self, maker: Maker) -> &mut Count {
        match maker {
            Maker::Voice => &mut self.voice,
            Maker::App => &mut self.app,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Seen {
    pub kept: Vec<String>,
    pub on: Vec<String>,
    pub left_off: Vec<String>,
}

/// One ask by `maker` that made nothing, counted: an atomic add, and the
/// flush begun for `directory` the first time, on a thread of its own.
pub fn refused(directory: &Path, maker: Maker) {
    refused_of(maker).fetch_add(1, Ordering::Relaxed);
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
    let refused = [Maker::Voice, Maker::App].map(|maker| {
        let count = refused_of(maker).swap(0, Ordering::Relaxed);
        let total = &mut totals.of(maker).refused;
        *total = total.saturating_add(count);
        (maker, count)
    });
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
            for (maker, count) in refused {
                refused_of(maker).fetch_add(count, Ordering::Relaxed);
            }
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
/// became of the made skills, added to [`FILE`], and told as
/// `{"made_by_voice": {"kept", "on", "refused", "left_off"}, "made_by_app":
/// {...}}`, the running totals per maker. Only numbers; never a name, a
/// word or a package.
pub fn counts(directory: &Path) -> Value {
    let entries = made_skill::entries(directory);
    let maker_of = |entry: &Value| Maker::of(entry["from"].as_str().unwrap_or_default());
    let sha_of = |entry: &Value| {
        entry["sha256"]
            .as_str()
            .filter(|sha256| sha256_ok(sha256))
            .map(str::to_owned)
    };
    let read = locked(directory, |totals| {
        for entry in &entries {
            let Some(sha256) = sha_of(entry) else {
                continue;
            };
            let Totals { voice, app, seen } = totals;
            let count = match maker_of(entry) {
                Maker::Voice => voice,
                Maker::App => app,
            };
            once(&mut count.kept, &mut seen.kept, &sha256);
            if entry["on"] == true {
                once(&mut count.on, &mut seen.on, &sha256);
            } else if entry.get("not_on").is_some()
                || entry["offered_at_unix_ms"].as_u64().is_some()
            {
                once(&mut count.left_off, &mut seen.left_off, &sha256);
            }
        }
        json!({"made_by_voice": totals.voice.json(), "made_by_app": totals.app.json()})
    });
    match read {
        Ok(made) => made,
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

    /// Each made skill counts under the maker its `from` says, the app's
    /// as well as the voice's, once each however often it is read.
    #[test]
    fn what_became_of_made_skills_is_counted_once_each_by_maker() {
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
            // An app's ask, made here or written there: the app's.
            entry(
                "local:app:9:1",
                'd',
                json!({"on": true, "offered_at_unix_ms": now - 9_000}),
            ),
            entry("written:app:8:1", 'f', json!({"on": true})),
            // Kept and couldn't be put on: left off.
            entry(
                "local:4",
                'e',
                json!({"goes_on_at_unix_ms": now - 1_000, "not_on": "the lamp's check didn't let it on"}),
            ),
            // Being put on now: not left off.
            entry("local:5", '9', json!({"goes_on_at_unix_ms": now - 1_000})),
        ];
        fs::write(
            dir.join(made_skill::INDEX),
            serde_json::to_vec(&entries).unwrap(),
        )
        .unwrap();
        let told = counts(&dir);
        let made = &told["made_by_voice"];
        assert_eq!(
            (&made["kept"], &made["on"], &made["left_off"]),
            (&json!(5), &json!(1), &json!(2)),
            "{told}"
        );
        let app = &told["made_by_app"];
        assert_eq!(
            (&app["kept"], &app["on"], &app["left_off"]),
            (&json!(2), &json!(2), &json!(0)),
            "{told}"
        );
        // Asked again: nothing counted twice, under either maker.
        let again = counts(&dir);
        for maker in ["made_by_voice", "made_by_app"] {
            for count in ["kept", "on", "left_off"] {
                assert_eq!(again[maker][count], told[maker][count], "{again}");
            }
        }
        let text = fs::read_to_string(dir.join(FILE)).unwrap() + &again.to_string();
        for secret in ["ann", "Ann", "22:00", "x.tar"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
    }

    #[test]
    fn a_refused_ask_is_counted_without_a_file_and_kept_at_the_next_read() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = folder("refused");
        // Two refused asks by voice and one from the app: counted in
        // memory alone, at once, each under its maker. Other tests'
        // refused asks may be counted meanwhile: at least these.
        refused(&dir, Maker::Voice);
        refused(&dir, Maker::Voice);
        refused(&dir, Maker::App);
        assert!(REFUSED_VOICE.load(Ordering::Relaxed) >= 2);
        assert!(REFUSED_APP.load(Ordering::Relaxed) >= 1);
        assert!(!dir.join(FILE).exists());
        let told = counts(&dir);
        assert!(
            told["made_by_voice"]["refused"].as_u64().unwrap() >= 2,
            "{told}"
        );
        assert!(
            told["made_by_app"]["refused"].as_u64().unwrap() >= 1,
            "{told}"
        );
        // A no forgets the file; the next read begins again.
        forget(&dir).unwrap();
        assert!(!dir.join(FILE).exists());
        forget(&dir).unwrap();
        forget(&dir.join("none")).unwrap();
    }
}
