//! Reviewer-three's P2 on #3450: with `LAMP_MEMORY_PERSONAL_NEEDS_FACE`, a
//! conversation nobody known was seen in (a guest's, or the owner's off
//! camera) is not observed: its job is closed with nothing kept and never
//! sent to the model, so "Someone is flying to Tokyo next week" is not kept
//! as the room's for every later session. A known person's conversation is
//! still observed and kept as theirs. Through Voice's production Days client
//! against the real socket and store, with a scripted model. All content is
//! synthetic; no provider, credentials, hardware or network are involved.
use lamp_memory::{
    Options, Server,
    days::{DATABASE, Days},
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::remember::{DaysClient, observe_with};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::Command,
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const ANA: &str = "p-0123456789abcdef";

/// What the scripted model was sent, each job's first heard words.
static SENT: Mutex<Vec<String>> = Mutex::new(Vec::new());

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
        let root = std::env::temp_dir().join(format!("lamp-mor-{}", std::process::id()));
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

const TEST: &str = "with_the_switch_a_room_conversation_is_not_observed_and_a_known_persons_is";

#[test]
fn with_the_switch_a_room_conversation_is_not_observed_and_a_known_persons_is() {
    // The switch is read from the environment, which this crate's tests may
    // not set in their own process: the test runs itself again with it set.
    if std::env::var("LAMP_MEMORY_PERSONAL_NEEDS_FACE").as_deref() != Ok("on") {
        let ran = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env("LAMP_MEMORY_PERSONAL_NEEDS_FACE", "on")
            .output()
            .unwrap();
        let (stdout, stderr) = (
            String::from_utf8_lossy(&ran.stdout),
            String::from_utf8_lossy(&ran.stderr),
        );
        assert!(ran.status.success(), "{}\n{stdout}{stderr}", ran.status);
        // `--exact` with a name that matches nothing exits 0 having run
        // nothing.
        assert!(
            stdout.contains("test result: ok. 1 passed;"),
            "{TEST} did not run\n{stdout}{stderr}"
        );
        return;
    }
    // The scripted model proposes, in words no pronoun list catches, what
    // reviewer-three's run kept as the room's.
    observe_with(Box::new(|input: &Value| {
        SENT.lock()
            .unwrap()
            .push(input["records"][0]["heard"].as_str()?.to_owned());
        Some(json!({"observations":[{
            "text":"Someone is flying to Tokyo next week.",
            "category":"thread",
            "source_ids":[input["source_ids"][0]],
            "supersedes":null
        }]}))
    }));
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();

    // Nobody known: the conversation's job is closed unsent, nothing kept.
    assert!(client.finished_exchange(
        fixture.now,
        "synthetic room: I'm flying to Tokyo next week",
        "Have a good trip"
    ));
    assert!(client.end_conversation());
    let (_, room) = fixture.next("conversation_end");
    let (apply, closed) = fixture.next("tidy_apply");
    assert_eq!(apply["job"], room["input"]["job"]);
    assert_eq!(apply["observations"], json!([]), "{apply}");
    assert_eq!(closed, json!({"ok":true,"ids":[]}));
    assert!(SENT.lock().unwrap().is_empty());
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE kind='observation'"),
        0
    );

    // Someone known: observed, and kept as theirs.
    assert!(client.finished_exchange_for(
        fixture.now,
        "synthetic known: I'm flying to Tokyo next week",
        "Have a good trip",
        Some(ANA)
    ));
    assert!(client.end_conversation());
    let (apply, kept) = fixture.next("tidy_apply");
    assert_eq!(apply["person"], ANA, "{apply}");
    assert_eq!(kept["ok"], true, "{kept}");
    assert_eq!(kept["ids"].as_array().unwrap().len(), 1, "{kept}");
    assert_eq!(SENT.lock().unwrap().len(), 1);
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE kind='observation' AND active=1"),
        1
    );
    assert_eq!(
        fixture.count("SELECT count(*) FROM jobs WHERE state='pending'"),
        0
    );
}
