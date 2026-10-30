//! #1693, step 2: a talk with no quiet gap past the memory's source bound
//! (`SOURCE_IDS`, 64 exchanges) is observed in pieces, never skipped: at the
//! bound the part said so far is ended and observed as a conversation's end
//! is, and the talk goes on in a new piece. No exchange is named in two
//! pieces, and a forget said during the talk still closes every piece's job
//! unsent (the N13 forget count, reviewer-two's P2 on #3450). Through Voice's production Days client against the real
//! socket and store, with a scripted model. One test: the forget count and
//! the model are this process's. All content is synthetic; no provider,
//! credentials, hardware or network are involved.
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
    cell::RefCell,
    collections::{BTreeSet, VecDeque},
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
const BEN: &str = "p-fedcba9876543210";
/// One talk's exchanges, past the 64 one piece may name.
const EXCHANGES: u64 = 70;

/// The source ids of each job the scripted model was sent.
static SENT: Mutex<Vec<Vec<u64>>> = Mutex::new(Vec::new());

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
    held: RefCell<VecDeque<(Value, Value)>>,
    stop: Stopper,
    worker: Option<JoinHandle<()>>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lamp-moo-{}", std::process::id()));
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
            held: RefCell::default(),
            stop,
            worker: Some(worker),
        }
    }

    /// The next request with this op, and the memory's answer to it.
    fn next(&self, op: &str) -> (Value, Value) {
        self.next_of(&[op])
    }

    /// The next request with any of these ops, and the memory's answer;
    /// the others seen on the way are kept for a later call, in order.
    fn next_of(&self, ops: &[&str]) -> (Value, Value) {
        let wanted = |event: &(Value, Value)| ops.iter().any(|op| event.0["op"] == *op);
        let mut held = self.held.borrow_mut();
        if let Some(at) = held.iter().position(wanted) {
            return held.remove(at).unwrap();
        }
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let event = self
                .trace
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if wanted(&event) {
                return event;
            }
            held.push_back(event);
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

/// One talk of [`EXCHANGES`] for `person`, each line written before the
/// next is said, with its `forget_after`th line's place forgotten by name
/// right after it (`forget {"about":...}`, which leaves the journal
/// writing); then its end. Gives each piece's end and the memory's answer.
fn talk(
    fixture: &Fixture,
    client: &DaysClient,
    person: &str,
    forget_after: Option<u64>,
) -> Vec<(Value, Value)> {
    let mut ends = Vec::new();
    for n in 1..=EXCHANGES {
        assert!(client.finished_exchange_for(
            fixture.now,
            &if forget_after == Some(n) {
                format!("synthetic line {n}: I'm flying to Tokyo next week")
            } else {
                format!("synthetic line {n}: a plain remark")
            },
            "Fine",
            Some(person)
        ));
        // The line is written; at the bound, the piece before it ended
        // first.
        loop {
            let event = fixture.next_of(&["append_linked", "conversation_end"]);
            if event.0["op"] == "conversation_end" {
                ends.push(event);
            } else {
                assert_eq!(event.1["ok"], true, "{event:?}");
                break;
            }
        }
        if forget_after == Some(n) {
            let (told, ok) = client.run_for(
                &ToolCall {
                    id: "f".into(),
                    name: FORGET.into(),
                    args: json!({"about":"Tokyo"}),
                },
                BUDGET,
                Some(person),
            );
            assert!(ok, "{told}");
        }
    }
    assert!(client.end_conversation());
    ends.push(fixture.next("conversation_end"));
    ends
}

#[test]
fn a_talk_past_the_source_bound_is_observed_in_pieces_and_a_forget_still_stops_them() {
    switch_forget_about(true);
    observe_with(Box::new(|input: &Value| {
        let ids = input["source_ids"]
            .as_array()?
            .iter()
            .filter_map(Value::as_u64)
            .collect();
        SENT.lock().unwrap().push(ids);
        None
    }));
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();

    // Seventy exchanges with no gap: two pieces, each ended and observed,
    // the first of the 64 before the bound, and no exchange in both.
    let ends = talk(&fixture, &client, ANA, None);
    assert_eq!(ends.len(), 2, "{ends:?}");
    let mut named = BTreeSet::new();
    for ((end, job), size) in ends.iter().zip([64, 6]) {
        assert_eq!(end["person"], ANA);
        assert!(job["input"]["job"].is_string(), "{job}");
        let ids = end["source_ids"].as_array().unwrap();
        assert_eq!(ids.len(), size, "{end}");
        for id in ids {
            assert!(named.insert(id.as_u64().unwrap()), "{id} twice: {ends:?}");
        }
    }
    assert_ne!(ends[0].0["conversation"], ends[1].0["conversation"]);
    for (_, job) in &ends {
        let (apply, closed) = fixture.next("tidy_apply");
        assert_eq!(apply["job"], job["input"]["job"]);
        assert_eq!(closed["ok"], true, "{closed}");
    }
    let sent = SENT.lock().unwrap().clone();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent.concat().len(), EXCHANGES as usize);

    // The same talk with a named forget said after its 30th line, in the
    // first piece: both pieces end, none named twice, and both jobs are
    // closed unsent, the later piece's too, as the whole talk's was.
    SENT.lock().unwrap().clear();
    let ends = talk(&fixture, &client, BEN, Some(30));
    assert_eq!(ends.len(), 2, "{ends:?}");
    let ids = |end: &Value| -> BTreeSet<u64> {
        end["source_ids"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_u64)
            .collect()
    };
    let (first, rest) = (ids(&ends[0].0), ids(&ends[1].0));
    assert_eq!((first.len(), rest.len()), (64, 6));
    assert!(first.is_disjoint(&rest));
    for (_, job) in &ends {
        let job = &job["input"]["job"];
        assert!(job.is_string(), "{ends:?}");
        let (apply, closed) = fixture.next("tidy_apply");
        assert_eq!(&apply["job"], job);
        assert_eq!(apply["observations"], json!([]), "{apply}");
        assert!(
            closed["ok"] == true || closed["error"] == "stale",
            "{closed}"
        );
    }
    assert!(SENT.lock().unwrap().is_empty());
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE kind='observation'"),
        0
    );
    assert_eq!(
        fixture.count("SELECT count(*) FROM jobs WHERE state='pending'"),
        0
    );
}
