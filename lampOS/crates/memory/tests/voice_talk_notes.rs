//! Talk notes (#1693's long-conversation plan, step 3,
//! `LAMP_CONVERSATION_SUMMARY`): at a conversation's end its running note
//! is kept in its own scope, the room's included, with its day; a recall
//! finds it, the asker's marked theirs and the room's told as the room's,
//! another person's never; a forget of the conversation takes it. Through
//! Voice's production Days client against the real socket and store, with
//! a scripted summary model. All content is synthetic; no provider,
//! credentials, hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::{DATABASE, Days},
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::{
    conversation_summary,
    remember::{DaysClient, FORGET_EVERYTHING, RECALL},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ANA: &str = "p-0123456789abcdef";
const MAI: &str = "p-00000000000000aa";
const BUDGET: Duration = Duration::from_millis(1500);

struct Traced {
    store: Days,
    trace: Sender<(Value, Value)>,
}

impl Memory for Traced {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        let answer = days_protocol::answer(&mut self.store, line, now);
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
        let root = std::env::temp_dir().join(format!("lamp-mtn-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let store = Days::open(
            Options {
                dir: root.join("store"),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(
                    lamp_voice::local_time::offset_here().unwrap_or(0) / 60,
                ),
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

    /// The next request with this op, and the memory's answer to it.
    fn next(&self, op: &str) -> (Value, Value) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let event = self
                .trace
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if event.0["op"] == op {
                return event;
            }
        }
    }

    fn count(&self, sql: &str) -> u64 {
        let db = Connection::open_with_flags(
            self.root.join("store").join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row(sql, [], |row| row.get(0)).unwrap()
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
        id: String::new(),
        name: name.into(),
        args,
    }
}

/// The scripted summary model: the previous note, and each line the person
/// said as a fact.
fn folding(input: &str) -> Option<String> {
    let job: Value = serde_json::from_str(input).ok()?;
    let mut note = job["previous"].as_str().unwrap_or_default().to_owned();
    if note.is_empty() {
        note = "Current activity: a rivers quiz\nKey facts:".into();
    }
    for line in job["exchanges"].as_str()?.lines() {
        if let Some(said) = line.strip_prefix("Person: ") {
            note.push_str(&format!("\n- {said}"));
        }
    }
    Some(note)
}

/// Folds `said` into the scope's running note, as turns leaving the view
/// would, and waits for the note.
fn noted(whose: Option<&str>, said: &str) {
    conversation_summary::fold(
        whose.map(str::to_owned),
        vec![(1, said.into(), "Mm.".into())],
    );
    let until = Instant::now() + Duration::from_secs(10);
    while !conversation_summary::for_scope(whose, Instant::now())
        .is_some_and(|note| note.contains(said))
    {
        assert!(Instant::now() < until, "the note was never made");
        std::thread::sleep(Duration::from_millis(5));
    }
}

const TEST: &str = "a_conversations_end_keeps_its_talk_note_in_its_scope_and_recall_finds_it";

#[test]
fn a_conversations_end_keeps_its_talk_note_in_its_scope_and_recall_finds_it() {
    // The switch is read from the environment, which this crate's tests may
    // not set in their own process: the test runs itself again with it set.
    if std::env::var(conversation_summary::ENVIRONMENT).as_deref() != Ok("on") {
        let ran = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env(conversation_summary::ENVIRONMENT, "on")
            .output()
            .unwrap();
        let (stdout, stderr) = (
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr),
        );
        assert!(ran.status.success(), "{}\n{stdout}{stderr}", ran.status);
        assert!(
            stdout.contains("test result: ok. 1 passed;"),
            "{TEST} did not run\n{stdout}{stderr}"
        );
        return;
    }
    assert!(lamp_voice::remember::switch_forget_all(true));
    conversation_summary::summarize_with(Box::new(folding));
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();

    // Ana's conversation: its note is kept as hers, dated today, linked to
    // its exchanges.
    noted(Some(ANA), "synthetic: I am planning a trip to Hue");
    assert!(client.finished_exchange_for(
        fixture.now,
        "synthetic: the quiz is about rivers",
        "Then the first river",
        Some(ANA)
    ));
    assert!(client.end_conversation());
    let (_, append) = fixture.next("append_linked");
    let (note, kept) = fixture.next("talk_note");
    assert_eq!(kept["ok"], true, "{kept}");
    assert_eq!(note["person"], ANA, "{note}");
    assert_eq!(note["source_ids"], json!([append["id"]]));
    assert!(
        note["text"].as_str().unwrap().contains("trip to Hue"),
        "{note}"
    );
    // The note is gone from the conversation once kept.
    assert_eq!(
        conversation_summary::for_scope(Some(ANA), Instant::now()),
        None
    );

    // The room's conversation: kept as the room's.
    noted(None, "synthetic: the office plant needs water");
    assert!(client.finished_exchange(fixture.now, "synthetic: who waters it?", "Someone here"));
    assert!(client.end_conversation());
    let (note, kept) = fixture.next("talk_note");
    assert_eq!(kept["ok"], true, "{kept}");
    assert!(note.get("person").is_none_or(Value::is_null), "{note}");

    // Ana's recall finds hers, marked hers, and the room's as the room's.
    let (told, ok) = client.run_for(
        &call(RECALL, json!({"about":"trip Hue"})),
        BUDGET,
        Some(ANA),
    );
    assert!(ok, "{told}");
    let found = told["found"].as_array().unwrap();
    let hers = found
        .iter()
        .find(|item| {
            item["talk_summary"]
                .as_str()
                .is_some_and(|text| text.contains("trip to Hue"))
        })
        .unwrap_or_else(|| panic!("{told}"));
    assert_eq!(hers["theirs"], true);
    assert_eq!(hers["about"], lamp_voice::remember::TALK_THEIRS);
    let day = hers["talked_on"].as_str().unwrap().to_owned();
    let (told, _) = client.run_for(
        &call(RECALL, json!({"about":"office plant"})),
        BUDGET,
        Some(ANA),
    );
    let room = told["found"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["talk_summary"]
                .as_str()
                .is_some_and(|text| text.contains("office plant"))
        })
        .unwrap_or_else(|| panic!("{told}"));
    assert!(room.get("theirs").is_none(), "{told}");
    assert_eq!(room["about"], lamp_voice::remember::TALK_ROOM);
    assert_eq!(room["talked_on"], day);
    // A new conversation of hers: "what did we talk about earlier?", in
    // its words or with none, is told her talk note (lamp-06c1, 30 Oct:
    // 7 kept, none found).
    assert!(client.finished_exchange_for(
        fixture.now,
        "synthetic: hello again",
        "Hi again",
        Some(ANA)
    ));
    for args in [json!({"about":"what did we talk about earlier"}), json!({})] {
        let (told, ok) = client.run_for(&call(RECALL, args.clone()), BUDGET, Some(ANA));
        assert!(ok, "{args}: {told}");
        assert!(
            told["found"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["theirs"] == true
                    && item["talk_summary"]
                        .as_str()
                        .is_some_and(|text| text.contains("trip to Hue"))),
            "{args}: {told}"
        );
    }
    // Another person never has hers; the room's own recall never has hers.
    for (person, query) in [(Some(MAI), "trip Hue"), (None, "trip Hue")] {
        let (told, _) = client.run_for(&call(RECALL, json!({"about":query})), BUDGET, person);
        assert!(
            !told.to_string().contains("trip to Hue"),
            "{person:?}: {told}"
        );
    }

    // Her forget_everything takes hers; the room's stays.
    let (told, ok) = client.run_for(&call(FORGET_EVERYTHING, json!({})), BUDGET, Some(ANA));
    assert!(ok, "{told}");
    assert_eq!(
        fixture.count(&format!(
            "SELECT count(*) FROM entries WHERE category='talk' AND person='{ANA}'"
        )),
        0
    );
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE category='talk' AND person='everyone'"),
        1
    );
}

const NEEDS_FACE_TEST: &str = "with_needs_face_a_room_conversation_keeps_no_talk_note";

/// Reviewer-three's P1 on #4479: with `LAMP_MEMORY_PERSONAL_NEEDS_FACE=on`
/// (in the product), a conversation nobody known was seen in keeps nothing,
/// no talk note either, while a known person's still keeps theirs. With the
/// switch off the room's is kept ([`TEST`] above).
#[test]
fn with_needs_face_a_room_conversation_keeps_no_talk_note() {
    let needs_face = lamp_voice::remember::needs_face::ENVIRONMENT;
    if std::env::var(conversation_summary::ENVIRONMENT).as_deref() != Ok("on") {
        let ran = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NEEDS_FACE_TEST, "--nocapture"])
            .env(conversation_summary::ENVIRONMENT, "on")
            .env(needs_face, "on")
            .output()
            .unwrap();
        let (stdout, stderr) = (
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr),
        );
        assert!(ran.status.success(), "{}\n{stdout}{stderr}", ran.status);
        assert!(
            stdout.contains("test result: ok. 1 passed;"),
            "{NEEDS_FACE_TEST} did not run\n{stdout}{stderr}"
        );
        return;
    }
    assert_eq!(std::env::var(needs_face).as_deref(), Ok("on"));
    conversation_summary::summarize_with(Box::new(folding));
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    // The room's conversation: no talk note.
    noted(None, "synthetic: my sister's surgery is on Friday");
    assert!(client.finished_exchange(fixture.now, "synthetic: at the city hospital", "I see"));
    assert!(client.end_conversation());
    let _ = fixture.next("conversation_end");
    // Ana's next: hers is kept, so the room's end was done by then.
    noted(Some(ANA), "synthetic: I am planning a trip to Hue");
    assert!(client.finished_exchange_for(
        fixture.now,
        "synthetic: the quiz is about rivers",
        "Then the first river",
        Some(ANA)
    ));
    assert!(client.end_conversation());
    let (note, kept) = fixture.next("talk_note");
    assert_eq!(kept["ok"], true, "{kept}");
    assert_eq!(note["person"], ANA, "{note}");
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE category='talk' AND person='everyone'"),
        0
    );
    let (told, _) = client.run_for(&call(RECALL, json!({"about":"surgery"})), BUDGET, None);
    assert!(!told.to_string().contains("surgery"), "{told}");
}
