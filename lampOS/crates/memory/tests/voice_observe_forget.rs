//! Reviewer-two's P2 on #3450: a conversation's observation job waiting
//! behind a slow one, when the person then has everything forgotten, is
//! closed with nothing kept and never sent to the model; the slow one's
//! proposals, back after the forget, are not given either. Through Voice's
//! production Days client against the real socket and store, with a
//! scripted model that holds the first job until released. All content is
//! synthetic; no provider, credentials, hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::{DATABASE, Days},
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::remember::{DaysClient, FORGET_EVERYTHING, observe_with, switch_forget_all};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{
        Condvar, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BUDGET: Duration = Duration::from_secs(5);
const ANA: &str = "p-0123456789abcdef";

/// What the scripted model was sent, each job's first heard words.
static SENT: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Whether the first job is with the model, and whether it may answer.
static ENTERED: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
static RELEASED: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

fn set(flag: &(Mutex<bool>, Condvar)) {
    *flag.0.lock().unwrap() = true;
    flag.1.notify_all();
}

fn wait(flag: &(Mutex<bool>, Condvar)) {
    let (lock, ready) = flag;
    let (set, timeout) = ready
        .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(10), |set| !*set)
        .unwrap();
    assert!(*set && !timeout.timed_out());
}

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
        let root = std::env::temp_dir().join(format!("lamp-mof-{}", std::process::id()));
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
fn a_forget_closes_the_jobs_from_before_it_and_the_waiting_one_is_never_sent() {
    assert!(switch_forget_all(true));
    // The scripted model: each job's words noted as sent; the first held
    // until released, then a preference proposed from its exchange.
    observe_with(Box::new(|input: &Value| {
        let heard = input["records"][0]["heard"].as_str()?.to_owned();
        SENT.lock().unwrap().push(heard.clone());
        if heard.contains("synthetic first") {
            set(&ENTERED);
            wait(&RELEASED);
        }
        Some(json!({"observations":[{
            "text":"They keep a synthetic teal lantern",
            "category":"preference",
            "source_ids":[input["source_ids"][0]],
            "supersedes":null
        }]}))
    }));
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();

    // Ana's first conversation ends, and its job is with the model.
    assert!(client.finished_exchange_for(
        fixture.now,
        "synthetic first: I keep a teal lantern",
        "Noted",
        Some(ANA)
    ));
    assert!(client.end_conversation());
    let (_, first) = fixture.next("conversation_end");
    assert!(first["input"]["job"].is_string(), "{first}");
    wait(&ENTERED);
    // Her second ends while it is: its job waits behind it.
    assert!(client.finished_exchange_for(
        fixture.now,
        "synthetic canary: the second conversation",
        "Fine",
        Some(ANA)
    ));
    assert!(client.end_conversation());
    let (_, canary) = fixture.next("conversation_end");
    assert!(canary["input"]["job"].is_string(), "{canary}");

    // She has everything forgotten; then the model answers the first.
    let (told, ok) = client.run_for(
        &ToolCall {
            id: "c".into(),
            name: FORGET_EVERYTHING.into(),
            args: json!({}),
        },
        BUDGET,
        Some(ANA),
    );
    assert!(ok && told["forgotten"] == true, "{told}");
    set(&RELEASED);

    // Both jobs are closed with nothing kept, in order, and the waiting
    // one never reached the model. Each is closed, or already gone with
    // everything of hers (`stale`).
    for job in [&first, &canary] {
        let (apply, closed) = fixture.next("tidy_apply");
        assert_eq!(apply["job"], job["input"]["job"]);
        assert_eq!(apply["observations"], json!([]), "{apply}");
        assert!(
            closed["ok"] == true || closed["error"] == "stale",
            "{closed}"
        );
    }
    let sent = SENT.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("synthetic first"), "{sent:?}");
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE kind='observation'"),
        0
    );
    assert_eq!(
        fixture.count("SELECT count(*) FROM jobs WHERE state='pending'"),
        0
    );
}
