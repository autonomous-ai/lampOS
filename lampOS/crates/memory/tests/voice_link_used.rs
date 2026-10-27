//! #2176 finding 14: an exchange is linked only to the kept things its turn
//! used, through Voice's production Days client against the real socket and
//! store, with `LAMP_MEMORY_LINK_USED` and `LAMP_MEMORY_FORGET_ALL` on for
//! each test's own process ([`alone`]). Forgetting a kept thing takes the
//! talk that used it and the follow-ups after it in the session, never the
//! unrelated talk before it nor what was observed of that. All content is
//! synthetic; the model of the observation pass is scripted; no
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
    io::Read,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BUDGET: Duration = Duration::from_millis(1500);
/// Set in the process a test runs itself again in ([`alone`]): the
/// directory its parent made for it, removed by the parent even when the
/// child had to be killed.
const ALONE: &str = "LAMP_MEMORY_TEST_ALONE";
/// How long a test's own process may take before it is killed.
const ALONE_FOR: Duration = Duration::from_secs(90);
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
        let root = PathBuf::from(std::env::var_os(ALONE).unwrap())
            .join(format!("f{}", NEXT.fetch_add(1, Ordering::Relaxed)));
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

/// Whether this is the test's own process: if not, the test runs itself
/// again in one and says how that went. The switches, the scripted model,
/// the observation thread (bound to the first socket it is given) and the
/// count of forgets it checks a job against are all Voice's process-wide
/// statics, and no test can reset them: run side by side, one test's forget
/// closed the other's observation job unsent, and a forget's conversation
/// end could bind the thread to a socket already gone (the Tests audit on #3504).
fn alone(test: &str) -> bool {
    if std::env::var_os(ALONE).is_some() {
        return true;
    }
    let dir = std::env::temp_dir().join(format!(
        "lamp-mlu-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    DirBuilder::new().mode(0o700).create(&dir).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--test-threads=1"])
        .env(ALONE, &dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let read = |mut from: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = from.read_to_end(&mut text);
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let stdout = read(Box::new(child.stdout.take().unwrap()));
    let stderr = read(Box::new(child.stderr.take().unwrap()));
    let until = Instant::now() + ALONE_FOR;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // Exited or reaped: both pipes are at their end.
    let (stdout, stderr) = (stdout.join().unwrap(), stderr.join().unwrap());
    let _ = fs::remove_dir_all(&dir);
    assert!(
        status.is_some_and(|status| status.success()),
        "{status:?} (killed after {ALONE_FOR:?} if None)\n{stdout}{stderr}"
    );
    // `--exact` with a name that matches nothing exits 0 having run nothing.
    assert!(
        stdout.contains("test result: ok. 1 passed;"),
        "{test} did not run alone\n{stdout}{stderr}"
    );
    false
}

fn switches() {
    assert!(switch_forget_all(true));
    assert!(switch_link_used(true));
    // The scripted model, the same for every test (the first set stands):
    // one thread observed, cited from every exchange.
    observe_with(Box::new(|input: &Value| {
        Some(json!({"observations":[{
            // Words of both conversations' heard lines: an observation
            // rests on what the person said (finding M on #3504).
            "text":"Their sister's wedding is in June, they need a gift and ask what to keep",
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
/// pass's thread is the process's own, bound to the first socket, so the
/// follow-ups share this one. A fresh client recalls teal again,
/// and the follow-ups that say "it", with no word of it among them, go with
/// the turn they follow; so "forget teal" takes all three exchanges and
/// what was observed of them, as it did with the switch off.
#[test]
fn forgetting_a_kept_thing_leaves_the_talk_that_did_not_use_it() {
    if !alone("forgetting_a_kept_thing_leaves_the_talk_that_did_not_use_it") {
        return;
    }
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
    if !alone("the_talk_that_used_a_kept_thing_goes_with_it") {
        return;
    }
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
