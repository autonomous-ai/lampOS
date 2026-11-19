//! A voice make whose route restarted while it was made (#7224, fault 5,
//! the first builder's 23:43 on 19 Nov): on lamp-c0a4 a service cycle
//! ended on `privacy_unreadable` during an OpenClaw make's 3–4 minutes, the
//! route started again in the same pid, and lamp-tasks' answer hit
//! `Broken pipe`. The person asked for a skill and never heard whether it
//! was made.
//!
//! With `LAMP_MAKE_SKILL_AGENT` on (a make that may go to the lamp's agent,
//! the long one), a voice make heard in its own turn is written down as it
//! begins ([`begin`]): its request number, the model's `make_skill`
//! arguments, and who asked (the queue's [`crate::make_queue::Asker`]: the
//! person, the owner and the line heard), at [`FILE`] in the voice's own
//! runtime folder (`RuntimeDirectory=lampos-voice`, 0700, kept across a
//! restart, gone at boot), 0600, at most [`MOST`]. It is taken out again as
//! the make ends, however it ends ([`Held`]).
//!
//! What a route that stopped mid-make left there is read once as the next
//! route's worker starts, and the file removed ([`left_at`]): each make
//! younger than [`RESUMED_WITHIN`] is made again with no turn, as a make
//! that waited its turn is (`LAMP_MAKE_QUEUE`'s `run_queued`), its line
//! said through the say service once. Its hand-over to the agent asks
//! lamp-tasks the same make, by the same rules: lamp-tasks gives it the
//! outcome it kept when the old route was gone, or waits for the one still
//! being made (`lamp_tasks::skill_make::KeptOutcomes`), never a second
//! run. One kept nowhere (lamp-tasks restarted too) is made anew.
use crate::make_queue::Asker;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Where a voice make under way is written down for a restart of its route.
pub const FILE: &str = "/run/lampos-voice/make-in-flight.json";
/// How long after its ask a make left by a route that stopped is made
/// again: lamp-tasks' own `KEPT_FOR`.
pub const RESUMED_WITHIN: Duration = Duration::from_secs(30 * 60);
/// The most makes written down at once.
pub const MOST: usize = 2;

/// One voice make under way, as written down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InFlight {
    /// The request it was asked in.
    pub request: u64,
    /// The model's `make_skill` arguments.
    pub args: Value,
    /// Who asked, and the line heard for it.
    pub asker: Asker,
    /// When it was asked, in Unix milliseconds.
    pub at_unix_ms: u64,
}

impl InFlight {
    fn to_json(&self) -> Value {
        json!({
            "request": self.request,
            "args": self.args,
            "speaker": self.asker.speaker,
            "owner": self.asker.owner,
            "heard": self.asker.heard,
            "at_unix_ms": self.at_unix_ms,
        })
    }

    fn of_json(value: &Value) -> Option<Self> {
        let text = |key: &str| value[key].as_str().map(str::to_owned);
        Some(Self {
            request: value["request"].as_u64()?,
            args: value.get("args").filter(|args| args.is_object())?.clone(),
            asker: Asker {
                speaker: text("speaker"),
                owner: text("owner"),
                heard: text("heard"),
            },
            at_unix_ms: value["at_unix_ms"].as_u64()?,
        })
    }
}

/// Every change to the file, one at a time in this process.
static WRITING: Mutex<()> = Mutex::new(());

fn read(path: &Path) -> Vec<InFlight> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|kept| {
            kept["makes"]
                .as_array()
                .map(|makes| makes.iter().filter_map(InFlight::of_json).collect())
        })
        .unwrap_or_default()
}

fn write(path: &Path, makes: &[InFlight]) {
    if makes.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let kept = json!({ "makes": makes.iter().map(InFlight::to_json).collect::<Vec<_>>() });
    if let Err(error) = crate::thread::write_file(path, &kept.to_string()) {
        eprintln!("lamp-voice: a make under way isn't kept for a restart of its route: {error}");
    }
}

/// A make written down while it is made: taken out when dropped.
pub struct Held {
    path: PathBuf,
    request: u64,
}

impl Drop for Held {
    fn drop(&mut self) {
        let _writing = WRITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut makes = read(&self.path);
        makes.retain(|make| make.request != self.request);
        write(&self.path, &makes);
    }
}

/// `make` written down at `path` until the [`Held`] is dropped, the newest
/// [`MOST`] kept.
pub fn begin_at(path: &Path, make: InFlight) -> Held {
    let _writing = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut makes = read(path);
    let request = make.request;
    makes.retain(|kept| kept.request != request);
    makes.push(make);
    let over = makes.len().saturating_sub(MOST);
    makes.drain(..over);
    write(path, &makes);
    Held {
        path: path.to_owned(),
        request,
    }
}

/// `make` written down at [`FILE`] with `LAMP_MAKE_SKILL_AGENT` on (a make
/// that may wait minutes for the agent); none otherwise.
pub fn begin(make: InFlight) -> Option<Held> {
    crate::make_skill_agent::on().then(|| begin_at(Path::new(FILE), make))
}

/// The makes a route that stopped left at `path`, read once and the file
/// removed: those younger than [`RESUMED_WITHIN`] at `now_unix_ms`, oldest
/// first.
pub fn left_at(path: &Path, now_unix_ms: u64) -> Vec<InFlight> {
    let _writing = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut makes = read(path);
    let _ = std::fs::remove_file(path);
    makes.retain(|make| {
        now_unix_ms.saturating_sub(make.at_unix_ms) <= RESUMED_WITHIN.as_millis() as u64
    });
    makes.sort_by_key(|make| make.at_unix_ms);
    makes
}

/// Now, in Unix milliseconds.
pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the test's own, removed when dropped.
    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("make-resume-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn make(request: u64, at_unix_ms: u64) -> InFlight {
        InFlight {
            request,
            args: json!({"request": "check the gold price every evening at six"}),
            asker: Asker {
                speaker: Some("Alex".into()),
                owner: Some("alex".into()),
                heard: Some("make me a gold price skill".into()),
            },
            at_unix_ms,
        }
    }

    /// The route restarts during a make: its [`Held`] is never dropped,
    /// so the make is left for the next worker, read once, whole.
    #[test]
    fn a_make_left_by_a_route_that_stopped_is_read_once() {
        let dir = Dir::new("read_once");
        let path = dir.path().join("make-in-flight.json");
        let held = begin_at(&path, make(7, 1_000));
        // The worker ends with the make under way.
        std::mem::forget(held);
        assert_eq!(left_at(&path, 2_000), vec![make(7, 1_000)]);
        assert!(!path.exists());
        assert_eq!(left_at(&path, 2_000), Vec::new(), "read once");
        use std::os::unix::fs::PermissionsExt;
        std::mem::forget(begin_at(&path, make(8, 1_000)));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// A make that ended is taken out: nothing is made again.
    #[test]
    fn a_make_that_ended_leaves_nothing() {
        let dir = Dir::new("ended");
        let path = dir.path().join("make-in-flight.json");
        let first = begin_at(&path, make(7, 1_000));
        let second = begin_at(&path, make(9, 1_100));
        drop(first);
        assert_eq!(read(&path), vec![make(9, 1_100)]);
        drop(second);
        assert!(!path.exists());
        assert_eq!(left_at(&path, 2_000), Vec::new());
    }

    /// Past [`RESUMED_WITHIN`] a make left is dropped; at most [`MOST`]
    /// are written down, the oldest going.
    #[test]
    fn old_makes_are_dropped_and_few_are_kept() {
        let dir = Dir::new("old");
        let path = dir.path().join("make-in-flight.json");
        for request in 1..=3 {
            std::mem::forget(begin_at(&path, make(request, request * 1_000)));
        }
        let within = RESUMED_WITHIN.as_millis() as u64;
        let left = left_at(&path, 2_000 + within + 1);
        assert_eq!(left, vec![make(3, 3_000)]);
    }
}
