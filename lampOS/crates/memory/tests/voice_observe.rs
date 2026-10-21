//! #1693 (2): the observation pass through Voice's production Days client
//! against the real socket and store, with a scripted model in place of the
//! route's (`remember::observe_with`, as `second_provider::worker` gives it
//! DeepSeek). What it proposes is kept only as the memory's own checks allow;
//! a model that gives nothing closes the job with nothing kept. All content
//! is synthetic; no provider, credentials, hardware or network are involved.
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
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

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
        let root = std::env::temp_dir().join(format!("lamp-mo-{}", std::process::id()));
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
fn the_pass_keeps_what_the_model_proposes_and_closes_a_job_it_gave_nothing_for() {
    // The scripted model: a preference cited from the conversation's own
    // exchange; nothing (as a failure or a timeout gives) for "nothing".
    observe_with(Box::new(|input: &Value| {
        if input["records"][0]["heard"]
            .as_str()?
            .contains("synthetic nothing")
        {
            return None;
        }
        Some(json!({"observations":[{
            "text":"They take their coffee black",
            "category":"preference",
            "source_ids":[input["source_ids"][0]],
            "supersedes":null
        }]}))
    }));
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();

    assert!(client.finished_exchange(
        fixture.now,
        "synthetic: I always take my coffee black",
        "Noted"
    ));
    assert!(client.end_conversation());
    let (_, append) = fixture.next("append_linked");
    let (end, job) = fixture.next("conversation_end");
    // The frozen job comes back to be sent to the model.
    assert_eq!(end["submit"], false);
    assert!(job["input"]["job"].is_string(), "{job}");
    let (apply, kept) = fixture.next("tidy_apply");
    assert_eq!(apply["job"], job["input"]["job"]);
    assert_eq!(
        apply["observations"][0]["source_ids"],
        json!([append["id"]])
    );
    assert_eq!(kept["ok"], true, "{kept}");
    assert_eq!(kept["ids"].as_array().unwrap().len(), 1);
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE kind='observation' AND active=1"),
        1
    );

    // A model that gives nothing: the job is closed with nothing kept, so
    // jobs never wait out their day and fill the memory's queue.
    assert!(client.finished_exchange(fixture.now, "synthetic nothing to keep", "Fine"));
    assert!(client.end_conversation());
    let (apply, closed) = fixture.next("tidy_apply");
    assert_eq!(apply["observations"], json!([]));
    assert_eq!(closed, json!({"ok":true,"ids":[]}));
    assert_eq!(
        fixture.count("SELECT count(*) FROM jobs WHERE state='pending'"),
        0
    );
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE kind='observation'"),
        1
    );
}
