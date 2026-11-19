//! A voice make whose route restarted while it was made (#7224, fault 5,
//! the first builder's 23:43 on 19 Nov): on lamp-c0a4 a service cycle
//! ended on `privacy_unreadable` during an OpenClaw make's 3–4 minutes, the
//! route started again in the same pid, and lamp-tasks' answer hit
//! `Broken pipe`. The person asked for a skill and never heard whether it
//! was made.
//!
//! With `LAMP_MAKE_SKILL_AGENT` on (a make that may go to the lamp's agent,
//! the long one), a voice make heard in its own turn carries a template
//! on its thread ([`on_this_thread`]): its request number, the model's
//! `make_skill` arguments, and who asked (the queue's
//! [`crate::make_queue::Asker`]: the person, the owner and the line
//! heard). It is written down only while lamp-tasks holds the make: at each
//! hand-over to the agent ([`handed_over`], the first and each repair),
//! at [`FILE`] in the voice's own runtime folder
//! (`RuntimeDirectory=lampos-voice`, 0700, kept across a restart, gone at
//! boot), 0600, at most [`MOST`]; and taken out the moment lamp-tasks'
//! answer is read ([`answered`]), before the voice checks or puts on
//! anything, so a restart then never asks the make again.
//!
//! Each hand-over carries the make's id ([`InFlight::id`], its request and
//! the time of its ask), the same on its repairs and on the make made again
//! after a restart: lamp-tasks keeps an outcome its voice was gone for by
//! that id, never by the rules, which a repair or a new day changes
//! (`lamp_tasks::skill_make::KeptOutcomes`).
//!
//! What a route that stopped mid-hand-over left there is read once as the
//! next route's worker starts, and the file removed ([`left_at`]): each
//! make younger than [`RESUMED_WITHIN`] is made again with no turn, as a
//! make that waited its turn is (`LAMP_MAKE_QUEUE`'s `run_queued`), its line
//! said through the say service once it answers. Its first hand-over asks
//! lamp-tasks the same make by its id: lamp-tasks gives it the outcome it
//! kept, a refusal too, or waits for the one still being made, with no run
//! of its own. One kept nowhere (lamp-tasks restarted too) is made anew.
//! The person's "forget everything" drops their makes written here
//! ([`forget_person`]); lamp-tasks' kept outcome, in its memory only, lasts
//! at most its 30 minutes.
use crate::make_queue::Asker;
use serde_json::{Value, json};
use std::cell::RefCell;
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
    /// The make's id, sent with each of its hand-overs: its request and the
    /// time of its ask, the same when it is made again after a restart.
    pub fn id(&self) -> String {
        format!("{}-{}", self.request, self.at_unix_ms)
    }

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

std::thread_local! {
    /// The voice make on this thread and where it is written down while
    /// lamp-tasks holds it ([`on_this_thread`]).
    static ON_THREAD: RefCell<Option<(PathBuf, InFlight)>> = const { RefCell::new(None) };
    /// It written down now: from a hand-over until its answer is read.
    static HELD: RefCell<Option<Held>> = const { RefCell::new(None) };
}

/// The voice make on this thread, until dropped (a panic too): nothing
/// held once it is.
pub struct OnThisThread(());

impl Drop for OnThisThread {
    fn drop(&mut self) {
        let held = HELD.with(|held| held.borrow_mut().take());
        drop(held);
        ON_THREAD.with(|on| on.borrow_mut().take());
    }
}

/// `make`, the voice make run on this thread, written down at [`FILE`]
/// while lamp-tasks holds it; none for any other make.
pub fn on_this_thread(make: Option<InFlight>) -> OnThisThread {
    on_this_thread_at(Path::new(FILE), make)
}

/// [`on_this_thread`] at `path`.
pub fn on_this_thread_at(path: &Path, make: Option<InFlight>) -> OnThisThread {
    let held = HELD.with(|held| held.borrow_mut().take());
    drop(held);
    ON_THREAD.with(|on| *on.borrow_mut() = make.map(|make| (path.to_owned(), make)));
    OnThisThread(())
}

/// A hand-over of this thread's voice make to lamp-tasks begins: it is
/// written down, and its id given for the line. None for a make with no
/// [`on_this_thread`].
pub fn handed_over() -> Option<String> {
    let (path, make) = ON_THREAD.with(|on| on.borrow().clone())?;
    let id = make.id();
    answered();
    let held = begin_at(&path, make);
    HELD.with(|kept| *kept.borrow_mut() = Some(held));
    Some(id)
}

/// lamp-tasks' answer to this thread's hand-over was read (or the
/// hand-over ended without one): taken out at once, before any check.
pub fn answered() {
    let held = HELD.with(|held| held.borrow_mut().take());
    drop(held);
}

/// The makes written down at `path` that `person` asked for, dropped (the
/// person's "forget everything"): never made again after a restart.
pub fn forget_person(path: &Path, person: &str) -> usize {
    let _writing = WRITING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut makes = read(path);
    let before = makes.len();
    makes.retain(|make| make.asker.speaker.as_deref() != Some(person));
    let dropped = before - makes.len();
    if dropped > 0 {
        write(path, &makes);
    }
    dropped
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

    /// Written down only while lamp-tasks holds the make: from each of its
    /// hand-overs (the first and each repair, one id) until its answer is
    /// read, before any check, so a restart then asks nothing again.
    #[test]
    fn a_make_is_written_down_only_while_lamp_tasks_holds_it() {
        let dir = Dir::new("held");
        let path = dir.path().join("make-in-flight.json");
        let on = on_this_thread_at(&path, Some(make(7, 1_000)));
        assert!(!path.exists(), "not before its hand-over");
        assert_eq!(handed_over().as_deref(), Some("7-1000"));
        assert_eq!(read(&path), vec![make(7, 1_000)]);
        answered();
        assert!(!path.exists(), "gone once its answer is read");
        // A repair's hand-over: the same id, written down again.
        assert_eq!(handed_over().as_deref(), Some("7-1000"));
        assert_eq!(read(&path).len(), 1);
        drop(on);
        assert!(!path.exists());
        // Any other make carries no id and writes nothing.
        let _none = on_this_thread_at(&path, None);
        assert_eq!(handed_over(), None);
        assert!(!path.exists());
    }

    /// The route stops during a repair round: the make is left, with the
    /// id its repairs carried, for the next worker.
    #[test]
    fn a_restart_during_a_repair_leaves_the_make_with_its_id() {
        let dir = Dir::new("repair");
        let path = dir.path().join("make-in-flight.json");
        let at = path.clone();
        let left = std::thread::spawn(move || {
            let on = on_this_thread_at(&at, Some(make(7, 1_000)));
            handed_over();
            answered();
            handed_over();
            // The worker ends here, mid-repair.
            std::mem::forget(on);
            left_at(&at, 2_000)
        })
        .join()
        .unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id(), "7-1000");
        assert!(!path.exists());
    }

    /// "Forget everything": the person's makes written down are dropped.
    #[test]
    fn forget_everything_drops_the_persons_makes() {
        let dir = Dir::new("forget");
        let path = dir.path().join("make-in-flight.json");
        std::mem::forget(begin_at(&path, make(7, 1_000)));
        let mut other = make(8, 1_100);
        other.asker.speaker = Some("Sam".into());
        std::mem::forget(begin_at(&path, other.clone()));
        assert_eq!(forget_person(&path, "Alex"), 1);
        assert_eq!(left_at(&path, 2_000), vec![other]);
    }
}
