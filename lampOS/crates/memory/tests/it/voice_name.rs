//! The forgotten name (the first builder's 15:1x on #1722; the owner told
//! lamp-4ace their name this week, and it was forgotten): a person tells the
//! lamp a made-up name, and later asks "what's my name?". Voice's own Days
//! client against the real store, as the second route uses it. For each way
//! the name may have been kept, and each moment it is asked, the table says
//! whether the session's note carries it (told to the model on every turn)
//! and whether a recall finds it (only if the model asks one).
//!
//! What this drives is memory: the store, its notes and its recall. What the
//! second route itself does with them is read from its code and named in
//! the table's last column: it asks a session's note once and keeps it
//! (`Sessions::frozen`), asks again for a new person, after something kept
//! with `remember` (#2540: `DaysClient::kept_since_note`, which the replay
//! asks as the route does), after ten quiet minutes with
//! `LAMP_MEMORY_REOPEN_QUIET`, or after a restart; and, without
//! `LAMP_SECOND_FACE_NAME` (#2539, off unless set), it never
//! tells the model the known face's name (the Live route does, with
//! `known_faces::note`), so a name kept only with `remember_my_face`, in the
//! eyes, is never told here.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::Days,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::remember::{DaysClient, RECALL, REMEMBER, person_id};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BUDGET: Duration = Duration::from_millis(1500);
const SESSION: Duration = Duration::from_secs(10);
/// A made-up name.
const NAME: &str = "Pippa";
static NEXT: AtomicU64 = AtomicU64::new(0);

/// The real store, each request and its answer traced.
struct Traced {
    store: Days,
    trace: Sender<Value>,
}

impl Memory for Traced {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        let answer = self.store.answer(line, now);
        let _ = self
            .trace
            .send(serde_json::from_slice(line).unwrap_or(Value::Null));
        answer
    }
}

/// One store's folder, served on its socket; [`Lamp::restart`] serves the
/// same folder again, as memory after the voice restarted.
struct Lamp {
    root: PathBuf,
    socket: PathBuf,
    now: u64,
    trace: Receiver<Value>,
    stop: Stopper,
    worker: Option<JoinHandle<()>>,
    /// Held for the fixture's life, so real-store voice fixtures take turns.
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Lamp {
    fn new() -> Self {
        let turn = crate::voice_store_turn();
        let root = std::env::temp_dir().join(format!(
            "lamp-vname-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let (socket, trace, stop, worker) = serve(&root, now, "a");
        Self {
            root,
            socket,
            now,
            trace,
            stop,
            worker: Some(worker),
            _turn: turn,
        }
    }

    /// The same store served again, on a socket of its own.
    fn restart(&mut self) {
        self.stop.stop();
        self.worker.take().unwrap().join().unwrap();
        let (socket, trace, stop, worker) = serve(&self.root, self.now + 60_000, "b");
        (self.socket, self.trace, self.stop) = (socket, trace, stop);
        self.worker = Some(worker);
    }

    /// Waits for the store to have taken a request with this op.
    fn took(&self, op: &str) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let line = self
                .trace
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if line["op"] == op {
                return;
            }
        }
    }
}

fn serve(root: &Path, now: u64, name: &str) -> (PathBuf, Receiver<Value>, Stopper, JoinHandle<()>) {
    let store = Days::open(
        Options {
            dir: root.join("store"),
            keep_days: 30,
            local: lamp_memory::store::LocalTime::Offset(0),
        },
        now,
    )
    .unwrap();
    let socket = root.join(format!("{name}.sock"));
    let server = Server::bind(&socket).unwrap();
    let stop = server.stopper();
    let (tx, trace) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut memory = Traced { store, trace: tx };
        server.run(&mut memory, &mut || now, &mut |_| {}).unwrap();
    });
    (socket, trace, stop, worker)
}

impl Drop for Lamp {
    fn drop(&mut self) {
        self.stop.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "voice-name-test".into(),
        name: name.into(),
        args,
    }
}

/// How the name was kept, if at all, in memory.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kept {
    /// `remember` with their face known: their own scope.
    Theirs,
    /// `remember` with no face known when they said it: the room's.
    Rooms,
    /// No `remember` (only `remember_my_face`, or nothing): the exchange is
    /// journalled, under their scope.
    Journal,
}

/// When "what's my name?" is asked, and whose note the second route has then.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Asked {
    /// A minute later, the same run of turns: the note asked before it, or
    /// asked again when something was kept since (#2540).
    SameRun,
    /// After ten quiet minutes, `LAMP_MEMORY_REOPEN_QUIET` off: as `SameRun`.
    QuietOff,
    /// After ten quiet minutes, the switch on: a new note, theirs.
    QuietOn,
    /// After a voice restart: a new note, theirs, from the store on disk.
    Restart,
    /// Their face gone, or a busy room (two people, or the face not the
    /// speaker's): a new note, the room's.
    RoomsNote,
}

/// Whether the note given for that moment, and a recall from the asker's
/// scope then, carry the name.
fn replay(kept: Kept, asked: Asked) -> (bool, bool) {
    let mut lamp = Lamp::new();
    let person = person_id(NAME);
    let mut client = DaysClient::new(lamp.socket.clone()).unwrap();
    // The run of turns they told it in: its note asked first, as the
    // second route asks it before the turn.
    let first = client.open_session_within(1, SESSION, Some(&person));
    assert!(first.is_some(), "no first session");
    match kept {
        Kept::Theirs | Kept::Rooms => {
            let scope = (kept == Kept::Theirs).then_some(person.as_str());
            let (told, ok) = client.run_for(
                &call(REMEMBER, json!({"text": format!("Their name is {NAME}.")})),
                BUDGET,
                scope,
            );
            assert!(ok && told["remembered"] == true, "{told}");
        }
        Kept::Journal => {
            assert!(client.finished_exchange_for(
                lamp.now - 1_000,
                &format!("My name is {NAME}."),
                &format!("Nice to meet you, {NAME}!"),
                Some(&person),
            ));
            lamp.took("append_linked");
        }
    }
    // The note the second route has when they ask, and whose scope asks.
    let (note, asker) = match asked {
        Asked::SameRun | Asked::QuietOff => {
            // Asked again, as the route asks it, when something was kept
            // since (#2540).
            let note = if client.kept_since_note() {
                client
                    .open_session_within(2, SESSION, Some(&person))
                    .unwrap()
            } else {
                first.unwrap()
            };
            (note, Some(person.clone()))
        }
        Asked::QuietOn => (
            client
                .open_session_within(2, SESSION, Some(&person))
                .unwrap(),
            Some(person.clone()),
        ),
        Asked::Restart => {
            drop(client);
            lamp.restart();
            client = DaysClient::new(lamp.socket.clone()).unwrap();
            (
                client
                    .open_session_within(1, SESSION, Some(&person))
                    .unwrap(),
                Some(person.clone()),
            )
        }
        Asked::RoomsNote => (client.open_session_within(2, SESSION, None).unwrap(), None),
    };
    let (room, theirs) = note;
    let in_note = [Some(&room), theirs.as_ref()]
        .into_iter()
        .flatten()
        .any(|note| note["note"].as_str().unwrap_or("").contains(NAME));
    let (recalled, ok) = client.run_for(
        &call(RECALL, json!({"about": "my name"})),
        BUDGET,
        asker.as_deref(),
    );
    assert!(ok, "{recalled}");
    (in_note, recalled.to_string().contains(NAME))
}

/// The table: each way the name was kept, against each moment it is asked.
/// A `false` in the note column with the model never asking a recall is a
/// name the lamp has lost at that moment.
#[test]
fn a_told_name_against_each_way_a_run_of_turns_ends() {
    use Asked::*;
    use Kept::*;
    let expected = [
        // (kept, asked, in the note, found by a recall)
        (Theirs, SameRun, true, true),
        (Theirs, QuietOff, true, true),
        (Theirs, QuietOn, true, true),
        (Theirs, Restart, true, true),
        (Theirs, RoomsNote, false, false),
        (Rooms, SameRun, true, true),
        (Rooms, QuietOff, true, true),
        (Rooms, QuietOn, true, true),
        (Rooms, Restart, true, true),
        (Rooms, RoomsNote, true, true),
        (Journal, SameRun, false, true),
        (Journal, QuietOff, false, true),
        (Journal, QuietOn, false, true),
        (Journal, Restart, false, true),
        (Journal, RoomsNote, false, false),
    ];
    let mut seen = Vec::new();
    for (kept, asked, _, _) in expected {
        let (note, recall) = replay(kept, asked);
        println!("NAME\t{kept:?}\t{asked:?}\tnote={note}\trecall={recall}");
        seen.push((kept, asked, note, recall));
    }
    assert_eq!(seen, expected);
}
