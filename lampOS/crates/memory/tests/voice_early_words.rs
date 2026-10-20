//! What was said before a lone face was known follows that person (#2176,
//! 5a): Voice's Days client, with `LAMP_MEMORY_EARLY_WORDS` switched on for
//! this process, against the real store on a socket. A room exchange and a
//! thing kept, then the one face known as A, with no other face seen: both
//! are A's in the store and in A's recall. With a second face seen in the
//! conversation, or the conversation ended between, nothing moves. All
//! content is synthetic.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::Days,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::{
    early_words,
    remember::{DaysClient, RECALL, REMEMBER, person_id},
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
            "lamp-ew-{}-{}",
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
        id: "voice-early-words-test".into(),
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

/// The eyes for the last `ms`, a look every 400 ms up to now: `faces` in
/// each (track ids `f0`, `f1`, ...), the last one known as `named`.
fn looked(ms: u64, faces: impl Fn(u64) -> usize, named: Option<&str>) {
    let now = Instant::now();
    let mut seen = early_words::SEEN.lock().unwrap();
    *seen = early_words::Seen::default();
    for back in (0..=ms / 400).rev() {
        let at = now - Duration::from_millis(back * 400);
        let who = (back == 0).then(|| named.map(str::to_owned)).flatten();
        let ids = (0..faces(back)).map(|n| Some(format!("f{n}"))).collect();
        seen.look(Some((ids, who)), at);
    }
}

/// The room's conversation: a thing kept and an exchange, then the
/// exchange of A's once known, after `between` (the eyes, an end).
fn conversation(
    fixture: &Fixture,
    client: &DaysClient,
    a: &str,
    between: impl FnOnce(),
) -> Vec<(Value, Value)> {
    looked(10_000, |_| 1, None);
    assert!(client.finished_exchange_for(
        fixture.now - 3_000,
        "my sister marries in june",
        "lovely, a june wedding",
        None
    ));
    // Kept after it, so the room's ids are 1 and 2 in that order.
    fixture.until(|line| line["op"] == "append_linked");
    let (told, ok) = client.run_for(
        &call(REMEMBER, json!({"text":"their sister marries in june"})),
        BUDGET,
        None,
    );
    assert!(ok && told["remembered"] == true, "{told}");
    between();
    assert!(client.finished_exchange_for(
        fixture.now,
        "what is on saturday",
        "the market",
        Some(a)
    ));
    fixture.until(|line| line["op"] == "append_linked" && line["person"] == a)
}

fn recall(client: &DaysClient, person: Option<&str>) -> Vec<(String, bool)> {
    let (told, ok) = client.run_for(
        &call(RECALL, json!({"about":"sister june wedding"})),
        BUDGET,
        person,
    );
    assert!(ok, "{told}");
    let mut found = found(&told);
    found.sort();
    found
}

#[test]
fn the_rooms_early_words_follow_a_lone_face_once_known_and_never_with_another() {
    assert!(early_words::switch(true));
    let a = person_id("Ann");

    // One face all along, then known as Ann: both are hers.
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let seen = conversation(&fixture, &client, &a, || looked(10_000, |_| 1, Some(&a)));
    let adopted: Vec<_> = seen
        .iter()
        .filter(|(line, _)| line["op"] == "adopt")
        .collect();
    assert_eq!(adopted.len(), 1, "{seen:?}");
    assert_eq!(adopted[0].0, json!({"op":"adopt","to":a,"ids":[1,2]}));
    assert_eq!(adopted[0].1["moved"], 2);
    assert_eq!(
        recall(&client, Some(&a)),
        [
            ("lovely, a june wedding".into(), true),
            ("their sister marries in june".into(), true)
        ]
    );
    assert!(recall(&client, None).is_empty());
    drop(client);
    drop(fixture);

    // A second face in one look of the conversation: nothing moves.
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let seen = conversation(&fixture, &client, &a, || {
        looked(10_000, |back| if back == 3 { 2 } else { 1 }, Some(&a))
    });
    assert!(
        seen.iter().all(|(line, _)| line["op"] != "adopt"),
        "{seen:?}"
    );
    assert!(seen.iter().any(|(line, _)| line["op"] == "conversation_end"
        && line.get("person").is_none()
        && line["source_ids"] == json!([1])));
    assert_eq!(recall(&client, None).len(), 2);
    assert!(recall(&client, Some(&a)).iter().all(|(_, theirs)| !theirs));
    drop(client);
    drop(fixture);

    // The room's conversation ended between: nothing moves.
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let seen = conversation(&fixture, &client, &a, || {
        assert!(client.end_conversation());
        looked(10_000, |_| 1, Some(&a));
    });
    assert!(
        seen.iter().all(|(line, _)| line["op"] != "adopt"),
        "{seen:?}"
    );
    assert_eq!(recall(&client, None).len(), 2);
    drop(client);
    drop(fixture);
}
