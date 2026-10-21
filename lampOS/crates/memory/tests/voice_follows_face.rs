//! What the caller said before the lamp knew their face follows them once it
//! does (#2176, "Remembers you"): Voice's Days client, with
//! `LAMP_MEMORY_FOLLOWS_FACE` switched on for this process, against the real
//! store on a socket. "My name is Bramble", then "remember my face" saved as
//! Bramble's, in the same voice: a new session's "what's my name?" finds it
//! in Bramble's own memory, theirs. A second voice's line between stays the
//! room's. All names and content are made up.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::Days,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::{
    follows_face,
    remember::{DaysClient, RECALL, person_id},
};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BUDGET: Duration = Duration::from_millis(1500);
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Traced<M> {
    store: M,
    trace: Sender<(Value, Value)>,
}

impl<M: Memory> Memory for Traced<M> {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        let answer = self.store.answer(line, now);
        let _ = self.trace.send((
            serde_json::from_slice(line).unwrap(),
            serde_json::from_str(&answer.line).unwrap(),
        ));
        answer
    }
}

struct Fixture {
    root: PathBuf,
    socket: PathBuf,
    now: u64,
    trace: Receiver<(Value, Value)>,
    stop: Stopper,
    worker: Option<JoinHandle<()>>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lamp-ff-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let store = Days::open(
            Options {
                dir: root.join("store"),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(0),
            },
            now,
        )
        .unwrap();
        let socket = root.join("m.sock");
        let server = Server::bind(&socket).unwrap();
        let stop = server.stopper();
        let (tx, trace) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut memory = Traced { store, trace: tx };
            server.run(&mut memory, &mut || now, &mut |_| {}).unwrap();
        });
        Self {
            root,
            socket,
            now,
            trace,
            stop,
            worker: Some(worker),
        }
    }

    /// Every op the store was asked, until (and with) the first that
    /// `last` matches.
    fn until(&self, last: impl Fn(&Value) -> bool) -> Vec<(Value, Value)> {
        let until = Instant::now() + Duration::from_secs(5);
        let mut seen = vec![];
        loop {
            let event = self
                .trace
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| panic!("the store saw only {seen:?}"));
            let done = last(&event.0);
            seen.push(event);
            if done {
                return seen;
            }
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.stop();
        self.worker.take().unwrap().join().unwrap();
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "voice-follows-face-test".into(),
        name: name.into(),
        args,
    }
}

/// Each text a recall told, with whether it was marked theirs.
fn found(told: &Value) -> Vec<(String, bool)> {
    told["found"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let text = ["asked_to_remember", "you_said"]
                .iter()
                .find_map(|field| item[*field].as_str())
                .unwrap();
            (text.to_owned(), item["theirs"] == true)
        })
        .collect()
}

/// The caller's voice, the same voice again (0.80, over 0.70), and another
/// (0.10, under 0.30): stand-ins for the second route's prints.
const BRAMBLE: [f32; 2] = [1.0, 0.0];
const BRAMBLE_AGAIN: [f32; 2] = [0.8, 0.6];
const SOMEONE_ELSE: [f32; 2] = [0.1, 0.995];

/// The eyes' known file, naming Bramble with their face's passing id `f1`
/// beside it, as the eyes write them once the face is saved; read by the
/// writer for whose face said the saving line. Written again at each save,
/// as the eyes keep writing them: the files are read as known for 3 s
/// after they are written, and a slow run's second save comes later.
fn eyes_know_bramble() {
    let dir = std::env::temp_dir().join(format!("lamp-ff-known-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let known = dir.join("known");
    fs::write(&known, "Bramble\n").unwrap();
    fs::write(dir.join("known-face"), "f1\n").unwrap();
    follows_face::reading_known(known);
}

/// "My name is Bramble" (request 1), a second voice's line (request 2) if
/// `between`, then "remember my face" (request 3), seen spoken by Bramble's
/// one face and saved as Bramble's in its turn; the conversation ends. Each
/// line is kept as the second route keeps it at its verdict. With
/// `printed`, the saving line has a print and "my name is Bramble" was said
/// off camera; without, the saving line was too short to print, and "my
/// name is Bramble" was seen spoken by the same one face.
fn conversation(
    fixture: &Fixture,
    client: &DaysClient,
    bramble: &str,
    between: bool,
    printed: bool,
) {
    let first_face = if printed {
        vec![]
    } else {
        vec!["b1/f1".into()]
    };
    follows_face::mark(1, Some(BRAMBLE.to_vec()), first_face);
    assert!(client.finished_line(
        1,
        fixture.now - 6_000,
        "my name is bramble",
        "nice to meet you, bramble",
        None
    ));
    if between {
        follows_face::mark(2, Some(SOMEONE_ELSE.to_vec()), vec![]);
        assert!(client.finished_line(
            2,
            fixture.now - 3_000,
            "my name is quill, by the way",
            "hello, quill",
            None
        ));
    }
    let saving_print = printed.then(|| BRAMBLE_AGAIN.to_vec());
    follows_face::mark(3, saving_print, vec!["b1/f1".into()]);
    eyes_know_bramble();
    assert!(client.face_saved(3, bramble));
    assert!(client.finished_line(
        3,
        fixture.now,
        "remember my face",
        "i will know your face now",
        None
    ));
    assert!(client.end_conversation());
    fixture.until(|line| line["op"] == "conversation_end" && line["person"] == bramble);
}

/// A new session's "what's my name?": recall in `person`'s scope.
fn whats_my_name(client: &DaysClient, person: Option<&str>) -> Vec<(String, bool)> {
    let (told, ok) = client.run_for(&call(RECALL, json!({"about":"name"})), BUDGET, person);
    assert!(ok, "{told}");
    if told["found"].as_array().is_none_or(Vec::is_empty) {
        return vec![];
    }
    let mut found = found(&told);
    found.sort();
    found
}

/// What `person`'s "what's my name?" finds that is marked theirs.
fn theirs(client: &DaysClient, person: &str) -> Vec<String> {
    whats_my_name(client, Some(person))
        .into_iter()
        .filter_map(|(text, theirs)| theirs.then_some(text))
        .collect()
}

#[test]
fn what_the_caller_said_before_their_face_was_saved_follows_them_and_no_one_elses() {
    assert!(follows_face::switch(true));
    eyes_know_bramble();
    let bramble = person_id("Bramble");

    // With the saving line printed, and without (too short to print, its
    // caller's print taken from "my name is Bramble" on the same face).
    for printed in [true, false] {
        // "My name is Bramble", then "remember my face": theirs.
        let fixture = Fixture::new();
        let client = DaysClient::new(fixture.socket.clone()).unwrap();
        conversation(&fixture, &client, &bramble, false, printed);
        assert_eq!(
            theirs(&client, &bramble),
            ["nice to meet you, bramble"],
            "printed={printed}"
        );
        assert!(whats_my_name(&client, None).is_empty());
        drop(client);
        drop(fixture);

        // A second voice's line between: it stays the room's.
        let fixture = Fixture::new();
        let client = DaysClient::new(fixture.socket.clone()).unwrap();
        conversation(&fixture, &client, &bramble, true, printed);
        assert_eq!(
            theirs(&client, &bramble),
            ["nice to meet you, bramble"],
            "printed={printed}"
        );
        assert_eq!(
            whats_my_name(&client, None),
            [("hello, quill".into(), false)]
        );
        drop(client);
        drop(fixture);
    }

    // The lines' prints are gone once their conversations ended.
    assert!(follows_face::LINES_HEARD.lock().unwrap().is_empty());
    let _ = fs::remove_dir_all(
        std::env::temp_dir().join(format!("lamp-ff-known-{}", std::process::id())),
    );
}
