//! Reviewer-three's and reviewer-two's P2 on `208e376a3` (#3450): a named
//! forget said during a conversation closes that conversation's
//! observation job unsent. Ana says she is flying to Tokyo, asks for it to
//! be forgotten (`forget {"about":"Tokyo"}`, the named path that returns
//! before the rest), and the conversation ends: the model is never sent it
//! and nothing of it is kept. Read at the conversation's end, the forget
//! was already counted, and a named forget was never counted; the
//! conversation's count is now taken at its first exchange and every forget
//! counts first. Through Voice's production Days client against the real
//! socket and store, with a scripted model. All content is synthetic; no
//! provider, credentials, hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::{DATABASE, Days},
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::remember::{DaysClient, FORGET, observe_with, switch_forget_about};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{
        Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BUDGET: Duration = Duration::from_secs(5);
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
        let root = std::env::temp_dir().join(format!("lamp-mon-{}", std::process::id()));
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

#[test]
fn a_named_forget_said_in_a_conversation_closes_its_job_unsent() {
    switch_forget_about(true);
    observe_with(Box::new(|input: &Value| {
        SENT.lock()
            .unwrap()
            .push(input["records"][0]["heard"].as_str()?.to_owned());
        Some(json!({"observations":[{
            "text":"Ana is flying to Tokyo next week for a job interview",
            "category":"thread",
            "source_ids":[input["source_ids"][0]],
            "supersedes":null
        }]}))
    }));
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();

    assert!(client.finished_exchange_for(
        fixture.now,
        "synthetic: I'm flying to Tokyo next week for a job interview",
        "Good luck with it",
        Some(ANA)
    ));
    let (told, ok) = client.run_for(
        &ToolCall {
            id: "c".into(),
            name: FORGET.into(),
            args: json!({"about":"Tokyo"}),
        },
        BUDGET,
        Some(ANA),
    );
    assert!(ok, "{told}");
    assert!(client.end_conversation());
    let (_, end) = fixture.next("conversation_end");
    assert!(end["input"]["job"].is_string(), "{end}");
    let (apply, closed) = fixture.next("tidy_apply");
    assert_eq!(apply["job"], end["input"]["job"]);
    assert_eq!(apply["observations"], json!([]), "{apply}");
    assert!(
        closed["ok"] == true || closed["error"] == "stale",
        "{closed}"
    );
    assert!(
        SENT.lock().unwrap().is_empty(),
        "{:?}",
        SENT.lock().unwrap()
    );
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE kind='observation'"),
        0
    );
}
