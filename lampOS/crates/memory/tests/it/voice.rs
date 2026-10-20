//! Voice's production Days client against the real socket/store. All content
//! is synthetic; no provider, credentials, hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Person, Server,
    days::{DATABASE, Days},
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::remember::{DaysClient, FORGET, RECALL, REMEMBER, gone};
use rusqlite::{Connection, OpenFlags};
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

const DAY: u64 = 86_400_000;
/// A session's deadline where a test checks what is opened, not how fast:
/// each open commits with a full fsync, which a loaded disk can take past
/// the voice's 300 ms (#948). The held-store test keeps the voice's own.
const SESSION: Duration = Duration::from_secs(10);
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Traced {
    store: Days,
    trace: Sender<(Value, Value)>,
    hold: Option<Receiver<()>>,
}

impl Memory for Traced {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        if let Some(hold) = self.hold.take() {
            hold.recv_timeout(Duration::from_secs(5)).unwrap();
        }
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
    fn new(seed: impl FnOnce(&mut Days, u64), hold: Option<Receiver<()>>) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lamp-mv-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let mut store = Days::open(
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
        seed(&mut store, now);
        let socket = root.join("m.sock");
        let server = Server::bind(&socket).unwrap();
        let stop = server.stopper();
        let (tx, trace) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut memory = Traced {
                store,
                trace: tx,
                hold,
            };
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

    fn client(&self) -> DaysClient {
        DaysClient::new(self.socket.clone()).unwrap()
    }

    fn next(&self, op: &str) -> (Value, Value) {
        let until = Instant::now() + Duration::from_secs(5);
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

    fn count(&self, table: &str) -> u64 {
        assert!(matches!(table, "jobs" | "sessions" | "entries"));
        let db = Connection::open_with_flags(
            self.root.join("store").join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.stop();
        self.worker.take().unwrap().join().unwrap();
        fs::remove_dir_all(&self.root).unwrap();
    }
}

/// The room's snapshot for `session`, within [`SESSION`].
fn opened(client: &DaysClient, session: u64) -> Option<Value> {
    client
        .open_session_within(session, SESSION, None)
        .map(|(room, _)| room)
}

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "voice-memory-test".into(),
        name: name.into(),
        args,
    }
}

#[test]
fn ranked_voice_recall_finds_an_earlier_day_and_respects_a_named_day() {
    let fixture = Fixture::new(
        |store, now| {
            let person = Person::everyone();
            store
                .append(
                    &person,
                    now - 2 * DAY,
                    "orchid orchid orchid",
                    "the blue pot",
                    &[],
                )
                .unwrap();
            store
                .append(
                    &person,
                    now - DAY,
                    "orchid among many plants and garden notes",
                    "a long general conversation with little detail about flowers",
                    &[],
                )
                .unwrap();
            for n in 0..10 {
                store
                    .append(
                        &person,
                        now - 1000 + n,
                        "today we discussed a different topic",
                        "nothing about flowers",
                        &[],
                    )
                    .unwrap();
            }
        },
        None,
    );
    let client = fixture.client();
    let (told, ok) = client.run(
        &call(RECALL, json!({"about":"orchid"})),
        Duration::from_millis(1500),
    );
    assert!(ok, "{told}");
    assert_eq!(told["found"][0]["source_id"], 1);
    assert_eq!(told["found"][0]["you_said"], "the blue pot");
    assert_eq!(told["found"][0]["when"], "2 days ago");
    assert_eq!(fixture.next("search").0["query"], "orchid");
    let (only, ok) = client.run(
        &call(RECALL, json!({"about":"orchid","days_ago":2})),
        Duration::from_millis(1500),
    );
    assert!(ok);
    assert_eq!(only["found"].as_array().unwrap().len(), 1);
    assert_eq!(only["found"][0]["source_id"], 1);
    let (none, ok) = client.run(
        &call(RECALL, json!({"about":"orchid","days_ago":0})),
        Duration::from_millis(1500),
    );
    assert!(ok && none["found"].as_array().unwrap().is_empty());
    assert!(
        client
            .run(&call(RECALL, json!({})), Duration::from_millis(1500))
            .1
    );
    assert_eq!(fixture.next("recall").0["query"], Value::Null);
}

#[test]
fn a_voice_session_receives_a_frozen_note_with_provenance_then_a_new_snapshot() {
    let fixture = Fixture::new(
        |store, now| {
            store
                .remember(&Person::everyone(), now - DAY, "coffee black")
                .unwrap();
        },
        None,
    );
    let client = fixture.client();
    let first = opened(&client, 1).unwrap();
    assert_eq!(first["source_ids"], json!([1]));
    assert!(first["note"].as_str().unwrap().contains("coffee black"));
    assert_eq!(first["invalidated"], false);
    let (told, ok) = client.run(
        &call(REMEMBER, json!({"text":"tea plain"})),
        Duration::from_millis(1500),
    );
    assert!(ok && told["remembered"] == true);
    assert_eq!(opened(&client, 1).unwrap(), first);
    let second = opened(&client, 2).unwrap();
    assert_ne!(first["session"], second["session"]);
    assert!(second["note"].as_str().unwrap().contains("tea plain"));
    fixture.next("session_close");
    assert_eq!(fixture.count("sessions"), 1);
    let another = opened(&fixture.client(), 1).unwrap();
    assert_ne!(
        another["session"], first["session"],
        "process-local client nonce prevents ID reuse"
    );
}

#[test]
fn a_remembered_fact_links_later_journal_echoes_and_forgetting_removes_them() {
    let fixture = Fixture::new(|_, _| {}, None);
    let client = fixture.client();
    assert!(
        client
            .run(
                &call(REMEMBER, json!({"text":"synthetic violet secret"})),
                Duration::from_millis(1500)
            )
            .1
    );
    let (_, added) = fixture.next("remember");
    let id = added["id"].as_u64().unwrap();
    assert!(client.finished_exchange(
        fixture.now,
        "what was the colour",
        "the synthetic violet secret"
    ));
    let (append, answer) = fixture.next("append_linked");
    assert_eq!(append["source_ids"], json!([id]));
    assert_eq!(answer["ok"], true);
    assert_eq!(fixture.count("entries"), 2);
    assert_eq!(
        client.run(&call(FORGET, json!({})), Duration::from_millis(1500)),
        (json!({"forgotten":true}), true)
    );
    assert_eq!(fixture.count("entries"), 0);
    assert!(!client.finished_exchange(fixture.now, "late cached prose", "synthetic violet secret"));
    let (told, ok) = client.run(
        &call(RECALL, json!({"about":"violet"})),
        Duration::from_millis(1500),
    );
    assert!(ok && told["found"].as_array().unwrap().is_empty());
}

#[test]
fn repeated_real_conversation_ends_reach_memory_without_retaining_model_jobs() {
    let fixture = Fixture::new(|_, _| {}, None);
    let client = fixture.client();
    for n in 0..10 {
        let start = Instant::now();
        assert!(client.finished_exchange(
            fixture.now,
            &format!("synthetic question {n}"),
            "synthetic answer"
        ));
        assert!(client.end_conversation());
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "foreground queued work must not wait on durable storage"
        );
        let (_, append) = fixture.next("append_linked");
        let (end, receipt) = fixture.next("conversation_end");
        assert_eq!(end["source_ids"], json!([append["id"]]));
        assert_eq!(end["submit"], true);
        assert_eq!(
            receipt,
            json!({"ok":true,"submitted":false,"reason":"model_unconfigured"})
        );
    }
    assert_eq!(fixture.count("jobs"), 0);
    assert_eq!(fixture.count("entries"), 10);
}

#[test]
fn a_held_real_store_costs_only_the_note_and_call_deadlines_and_cleans_a_late_note() {
    let (release, hold) = mpsc::channel();
    let fixture = Fixture::new(
        |store, now| {
            store
                .remember(&Person::everyone(), now, "coffee black")
                .unwrap();
        },
        Some(hold),
    );
    let client = fixture.client();
    let start = Instant::now();
    assert!(client.open_session(1, Duration::from_secs(10)).is_none());
    let note_elapsed = start.elapsed();
    assert!(
        note_elapsed >= Duration::from_millis(280) && note_elapsed < Duration::from_millis(450),
        "{note_elapsed:?}"
    );
    let start = Instant::now();
    assert_eq!(
        client.run(
            &call(RECALL, json!({"about":"coffee"})),
            Duration::from_secs(10)
        ),
        (gone(), false)
    );
    let call_elapsed = start.elapsed();
    assert!(
        call_elapsed >= Duration::from_millis(1400) && call_elapsed < Duration::from_millis(1650),
        "{call_elapsed:?}"
    );
    release.send(()).unwrap();
    fixture.next("session_open");
    fixture.next("session_close");
    assert_eq!(fixture.count("sessions"), 0);
    assert!(opened(&client, 2).is_some());
}

#[test]
fn a_missing_store_never_becomes_an_empty_or_successful_memory_answer() {
    let root = std::env::temp_dir().join(format!("lamp-mv-absent-{}", std::process::id()));
    assert!(!root.exists());
    let client = DaysClient::new(root).unwrap();
    let start = Instant::now();
    assert!(client.open_session(1, Duration::from_millis(300)).is_none());
    assert_eq!(
        client.run(
            &call(RECALL, json!({"about":"coffee"})),
            Duration::from_millis(1500)
        ),
        (gone(), false)
    );
    assert!(start.elapsed() < Duration::from_millis(300));
}

#[test]
fn a_timed_out_mutation_may_commit_but_cannot_seed_unlinked_journal_history() {
    for name in [REMEMBER, FORGET] {
        let (release, hold) = mpsc::channel();
        let fixture = Fixture::new(
            |store, now| {
                store
                    .remember(&Person::everyone(), now, "existing synthetic memory")
                    .unwrap();
            },
            Some(hold),
        );
        let client = fixture.client();
        // The caller gives up while the write is held. Its deadline covers
        // sending the line on a loaded host: with 50 ms a client held up
        // before it wrote gave up without sending, so nothing was held
        // (#1280). The hold, not the deadline, decides that it times out.
        assert_eq!(
            client.run(
                &call(name, json!({"text":"late synthetic memory"})),
                Duration::from_secs(1)
            ),
            (gone(), false)
        );
        assert!(!client.finished_exchange(
            fixture.now,
            "after an uncertain mutation",
            "a cached memory"
        ));
        release.send(()).unwrap();
        let (_, answer) = fixture.next(if name == REMEMBER {
            "remember"
        } else {
            "forget_last"
        });
        assert_eq!(
            answer["ok"], true,
            "the write really did commit after the caller gave up"
        );
        assert!(!client.finished_exchange(
            fixture.now,
            "even after the late receipt",
            "a cached memory"
        ));
        assert_eq!(
            fixture.count("entries"),
            if name == REMEMBER { 2 } else { 0 }
        );
    }
}

#[test]
fn deletion_invalidates_a_frozen_note_and_linked_writes_never_fall_back_to_plain_append() {
    let fixture = Fixture::new(
        |store, now| {
            store
                .remember(&Person::everyone(), now, "synthetic private fact")
                .unwrap();
        },
        None,
    );
    let client = fixture.client();
    assert!(opened(&client, 1).is_some());
    let deleted = lamp_voice::tools::exchange(
        &fixture.socket,
        &json!({"op":"forget_last","kind":"kept"}),
        Duration::from_millis(1500),
    )
    .unwrap();
    assert!(deleted["removed"].is_object());
    assert!(client.finished_exchange(fixture.now, "late echo", "synthetic private fact"));
    let (request, answer) = fixture.next("append_linked");
    assert_eq!(request["source_ids"], json!([1]));
    assert_eq!(answer["ok"], false);
    assert_eq!(fixture.count("entries"), 0);
    assert!(opened(&client, 1).is_none());
    // The server has replied to that request only after processing the failed
    // linked write; a plain-append retry would have made an entry by now.
    assert_eq!(fixture.count("entries"), 0);
}
