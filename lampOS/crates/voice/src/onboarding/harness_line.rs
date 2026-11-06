//! One sentence about Harness in the walk's closing (#5488): said once, in
//! the owner's language, and never when Harness is already signed in.
//!
//! Whether Harness is signed in is lamp-tasks' to say: its `status` answer
//! carries `signed_in` (`yes`, `offline` or `no`, as its journal's "Harness
//! signed in: …" line) once `serve --set-up-later` has read it. The walk
//! asks that on the tasks part's own request socket (`LAMP_TASKS_DIR`), off
//! the voice's loop, and says the sentence only on a plain `no`: with no tasks
//! part, no answer, or an older lamp-tasks that does not say, it is skipped
//! rather than claim something untrue. Not knowing is not kept: the next
//! closing reads again, and the read ends once the sentence is decided.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

/// `on` exactly, with `LAMP_ONBOARDING`; off, nothing changes.
pub const ENVIRONMENT: &str = "LAMP_ONBOARDING_HARNESS_LINE";

/// The one sentence about Harness, said at the end of the walk's closing,
/// inside its quoted line (`skill::PREFERENCES_DONE_HARNESS`,
/// `skill::PREFERENCES_DONE_TRIES_HARNESS`): one turn, never a note of its
/// own. In code, not the skill file: an older build refuses a new key.
/// The closings spell it out whole; their tests hold them to these words.
#[cfg(test)]
pub(crate) const SENTENCE: &str =
    "If you work on a computer, I can take on tasks for you: open Harness in the app to connect.";

/// How often the tasks part is asked while the voice runs.
const EVERY: Duration = Duration::from_secs(20);
/// How long one ask may take: a local socket answers at once or not at all.
const DEADLINE: Duration = Duration::from_secs(2);

/// What the walk knows of Harness's sign-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Linked {
    /// Signed in (`yes`, or `offline`: signed in, not renewable just now).
    Yes,
    /// Not signed in.
    No,
    /// Not known: no answer yet, or a tasks part that does not say.
    Unknown,
}

/// What became of the sentence, kept in the onboarding file so it is decided
/// once per setup: a redo, a restart or a later walk never says it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kept {
    /// Handed out to be said.
    Said,
    /// Skipped: Harness was signed in.
    SignedIn,
    /// Skipped: this voice has no tasks part (`LAMP_TASKS_DIR`).
    NoTasks,
    /// Not decided: the tasks part did not say whether Harness is signed
    /// in. Never kept since the #5488 review (a file from the build before
    /// may hold it): the next closing reads again.
    Unknown,
}

impl Kept {
    /// Decided for good: everything but [`Self::Unknown`], which leaves the
    /// next closing to read the sign-in again.
    pub(crate) fn is_final(self) -> bool {
        self != Self::Unknown
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Said => "said",
            Self::SignedIn => "signed_in",
            Self::NoTasks => "no_tasks",
            Self::Unknown => "unknown",
        }
    }
}

/// What the sentence comes to with `linked` read: said only on a plain `No`.
/// `None` for the read is a voice with no tasks part.
pub(crate) fn decide(linked: Option<Linked>) -> Kept {
    match linked {
        None => Kept::NoTasks,
        Some(Linked::No) => Kept::Said,
        Some(Linked::Yes) => Kept::SignedIn,
        Some(Linked::Unknown) => Kept::Unknown,
    }
}

/// Harness's sign-in in lamp-tasks' `status` answer.
pub(crate) fn linked_of(answer: Option<&Value>) -> Linked {
    let Some(answer) = answer.filter(|answer| answer.get("ok") == Some(&json!(true))) else {
        return Linked::Unknown;
    };
    match answer.get("signed_in").and_then(Value::as_str) {
        Some("yes" | "offline") => Linked::Yes,
        Some("no") => Linked::No,
        _ => Linked::Unknown,
    }
}

/// The last read, shared with the thread that reads it.
pub(crate) type Read = Arc<Mutex<Linked>>;

/// The tasks part's request socket, with an absolute `LAMP_TASKS_DIR`;
/// `None` with no tasks part. Read only with the switch on.
pub(crate) fn socket_from_environment() -> Option<PathBuf> {
    let directory = std::env::var(crate::agent_tasks::ENVIRONMENT).ok()?;
    let directory = Path::new(&directory);
    if !directory.is_absolute() {
        return None;
    }
    Some(directory.join("request.sock"))
}

/// A read kept fresh by a thread of its own, every [`EVERY`], until the
/// walk lets the read go (the walk drops it once the sentence is decided,
/// #5488 review): the thread then ends at its next wake.
pub(crate) fn start(socket: PathBuf) -> Read {
    let read = Arc::new(Mutex::new(Linked::Unknown));
    let weak: Weak<Mutex<Linked>> = Arc::downgrade(&read);
    let spawned = std::thread::Builder::new()
        .name("onboarding-harness".into())
        .spawn(move || {
            let mut last = None;
            loop {
                let answer = crate::tools::exchange_up_to(
                    &socket,
                    &json!({"op":"status"}),
                    DEADLINE,
                    crate::tools::MAX_TASKS_ANSWER_BYTES,
                );
                let linked = linked_of(answer.as_ref());
                let Some(read) = weak.upgrade() else {
                    return;
                };
                if let Ok(mut kept) = read.lock() {
                    *kept = linked;
                }
                drop(read);
                if last != Some(linked) {
                    eprintln!("lamp-voice: onboarding: Harness signed in, as the tasks part says: {linked:?}");
                    last = Some(linked);
                }
                std::thread::sleep(EVERY);
            }
        });
    if let Err(error) = spawned {
        eprintln!("lamp-voice: onboarding: Harness's sign-in not read: {error}");
    }
    read
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_no_says_the_sentence() {
        let answer = |signed_in: Value| json!({"ok":true,"summary":{},"signed_in":signed_in});
        assert_eq!(linked_of(Some(&answer(json!("no")))), Linked::No);
        assert_eq!(linked_of(Some(&answer(json!("yes")))), Linked::Yes);
        assert_eq!(linked_of(Some(&answer(json!("offline")))), Linked::Yes);
        assert_eq!(linked_of(Some(&answer(json!("maybe")))), Linked::Unknown);
        // An older lamp-tasks, or one that has not read it yet, says nothing.
        assert_eq!(linked_of(Some(&json!({"ok":true}))), Linked::Unknown);
        assert_eq!(
            linked_of(Some(&json!({"ok":false,"signed_in":"no"}))),
            Linked::Unknown
        );
        assert_eq!(linked_of(None), Linked::Unknown);
        assert_eq!(decide(Some(Linked::No)), Kept::Said);
        assert_eq!(decide(Some(Linked::Yes)), Kept::SignedIn);
        assert_eq!(decide(Some(Linked::Unknown)), Kept::Unknown);
        assert_eq!(decide(None), Kept::NoTasks);
        assert!(!Kept::Unknown.is_final());
        assert!(Kept::Said.is_final() && Kept::SignedIn.is_final() && Kept::NoTasks.is_final());
    }

    /// The thread reads the tasks part's own answer, and a socket nobody
    /// serves leaves it unknown.
    #[test]
    fn the_read_follows_the_tasks_parts_status() {
        use std::io::{BufRead, BufReader, Write};
        let directory = std::env::temp_dir().join(format!(
            "harness-line-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("request.sock");
        let nobody = start(directory.join("nobody.sock"));
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let served = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap()["op"],
                "status"
            );
            (&stream)
                .write_all(b"{\"ok\":true,\"signed_in\":\"no\"}\n")
                .unwrap();
        });
        let read = start(socket);
        served.join().unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while *read.lock().unwrap() != Linked::No && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*read.lock().unwrap(), Linked::No);
        assert_eq!(*nobody.lock().unwrap(), Linked::Unknown);
    }
}
