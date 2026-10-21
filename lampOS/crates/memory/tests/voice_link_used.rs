//! #2176 finding 14: an exchange is linked only to the kept things its turn
//! used, through Voice's production Days client against the real socket and
//! store, with `LAMP_MEMORY_LINK_USED` and `LAMP_MEMORY_FORGET_ALL` on for
//! this test process. Forgetting a kept thing takes the talk that used it
//! and the follow-ups after it in the session, never the unrelated talk
//! before it nor what was observed of that. All
//! content is synthetic; the model of the observation pass is scripted; no
//! provider, credentials, hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::{DATABASE, Days},
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::remember::{
    DaysClient, FORGET, RECALL, REMEMBER, observe_with, switch_forget_all, switch_link_used,
};
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

const BUDGET: Duration = Duration::from_millis(1500);
static NEXT: AtomicU64 = AtomicU64::new(0);

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
        let root = std::env::temp_dir().join(format!(
            "lamp-mlu-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
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

    /// Active entries of this kind.
    fn active(&self, kind: &str) -> u64 {
        assert!(matches!(kind, "kept" | "exchange" | "observation"));
        let db = Connection::open_with_flags(
            self.root.join("store").join(DATABASE),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row(
            "SELECT count(*) FROM entries WHERE kind=?1 AND active=1",
            [kind],
            |row| row.get(0),
        )
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

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "c".into(),
        name: name.into(),
        args,
    }
}

fn switches() {
    assert!(switch_forget_all(true));
    assert!(switch_link_used(true));
    // The scripted model, the same for every test (the first set stands):
    // one thread observed, cited from every exchange.
    observe_with(Box::new(|input: &Value| {
        Some(json!({"observations":[{
            "text":"Their sister's wedding is in June and they need a gift",
            "category":"thread",
            "source_ids":input["source_ids"],
            "supersedes":null
        }]}))
    }));
}

/// Teal kept in request 1's turn; its id.
fn keep_teal(fixture: &Fixture, client: &DaysClient) -> u64 {
    let (told, ok) = client.run_in(
        &call(REMEMBER, json!({"text":"My favourite colour is teal"})),
        BUDGET,
        None,
        1,
    );
    assert!(ok && told["remembered"] == true, "{told}");
    let (_, kept) = fixture.next("remember");
    kept["id"].as_u64().unwrap()
}

/// This exchange's append and the ids it was linked to.
fn appended(fixture: &Fixture) -> Value {
    let (append, answer) = fixture.next("append_linked");
    assert_eq!(answer["ok"], true, "{answer}");
    append["source_ids"].clone()
}

fn forget(client: &DaysClient, id: u64) {
    let (told, ok) = client.run(&call(FORGET, json!({"source_id":id})), BUDGET);
    assert!(ok && told["forgotten"] == true, "{told}");
}

/// The lamp-06c1 run's shape (21 Oct): teal kept, two exchanges that don't
/// use it, something observed of them, then teal forgotten. Teal is gone;
/// the two exchanges and the observation stay. Off, both exchanges were
/// linked to teal, and the forget took them and the observation.
///
/// Then the reviewer's P2 on #2511, on the same store: the observation
/// pass's thread is the process's own, bound to the first socket, so every
/// test that observes shares this one. A fresh client recalls teal again,
/// and the follow-ups that say "it", with no word of it among them, go with
/// the turn they follow; so "forget teal" takes all three exchanges and
/// what was observed of them, as it did with the switch off.
#[test]
fn forgetting_a_kept_thing_leaves_the_talk_that_did_not_use_it() {
    switches();
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let teal = keep_teal(&fixture, &client);
    assert!(client.finished_exchange_of(
        2,
        fixture.now,
        "my sister's wedding is in June",
        "How lovely, a June wedding.",
        None
    ));
    assert!(client.finished_exchange_of(
        3,
        fixture.now,
        "I need a gift for her",
        "A framed photo could be nice.",
        None
    ));
    assert_eq!(appended(&fixture), json!([]));
    assert_eq!(appended(&fixture), json!([]));
    assert!(client.end_conversation());
    let (_, applied) = fixture.next("tidy_apply");
    assert_eq!(applied["ok"], true, "{applied}");
    assert_eq!(fixture.active("observation"), 1);

    forget(&client, teal);
    assert_eq!(fixture.active("kept"), 0);
    assert_eq!(fixture.active("exchange"), 2);
    assert_eq!(fixture.active("observation"), 1);

    the_follow_ups_after_a_kept_thing_was_used_go_with_it(&fixture);
}

/// An exchange whose words answered from teal goes with it when it is
/// forgotten; one about something else before it stays.
#[test]
fn the_talk_that_used_a_kept_thing_goes_with_it() {
    switches();
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let teal = keep_teal(&fixture, &client);
    // Request 2 is about something else.
    assert!(client.finished_exchange_of(2, fixture.now, "the wedding is in June", "Lovely.", None));
    // Request 3 calls nothing, and answers from it.
    assert!(client.finished_exchange_of(
        3,
        fixture.now,
        "which colour should the curtains be?",
        "Teal would suit them.",
        None
    ));
    assert_eq!(appended(&fixture), json!([]));
    assert_eq!(appended(&fixture), json!([teal]));

    forget(&client, teal);
    assert_eq!(fixture.active("kept"), 0);
    assert_eq!(fixture.active("exchange"), 1);
}

/// The follow-ups after teal was used, on the store the test above left
/// (two exchanges and one observation active).
fn the_follow_ups_after_a_kept_thing_was_used_go_with_it(fixture: &Fixture) {
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let teal = keep_teal(fixture, &client);
    // Request 2 recalls it; its words share none of teal's.
    let (told, ok) = client.run_in(&call(RECALL, json!({"about":"teal"})), BUDGET, None, 2);
    assert!(ok, "{told}");
    assert_eq!(told["found"][0]["source_id"], teal, "{told}");
    assert!(client.finished_exchange_of(
        2,
        fixture.now,
        "what did I ask you to keep?",
        "You asked me to keep that one.",
        None
    ));
    assert!(client.finished_exchange_of(
        3,
        fixture.now,
        "why do I like it?",
        "You said it reminds you of the sea.",
        None
    ));
    assert!(client.finished_exchange_of(
        4,
        fixture.now,
        "tell me more about that",
        "Waves, salt and a long summer.",
        None
    ));
    for _ in 0..3 {
        assert_eq!(appended(fixture), json!([teal]));
    }
    assert!(client.end_conversation());
    let (_, applied) = fixture.next("tidy_apply");
    assert_eq!(applied["ok"], true, "{applied}");
    assert_eq!(fixture.active("exchange"), 5);
    assert_eq!(fixture.active("observation"), 2);

    forget(&client, teal);
    assert_eq!(fixture.active("kept"), 0);
    assert_eq!(fixture.active("exchange"), 2);
    assert_eq!(fixture.active("observation"), 1);
}
