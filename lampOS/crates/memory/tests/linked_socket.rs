//! `linked.sock` (the linked-service memory plan, slice 1b; `docs/memory-days.md`
//! "Linked records"): the real server on real sockets with the Days store.
//! The socket is 0660 for its group and takes `linked_record` only; memory
//! keeps a record for the owner of the task row it names and drops the rest,
//! counting each drop; a writer that goes quiet is dropped within
//! `LINKED_PATIENCE`, so it never holds the main socket long.
use lamp_memory::{
    Options, Server,
    days::Days,
    server::{LINKED_PATIENCE, LinkedDrops},
};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// 2026-10-13 12:00 UTC: the time the server is told it is.
const NOON: u64 = 1_791_892_800_000;
const P1: &str = "p-0123456789abcdef";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = PathBuf::from(format!("/tmp/lls{}{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        DirBuilder::new().mode(0o700).create(&dir).unwrap();
        DirBuilder::new()
            .mode(0o700)
            .create(dir.join("store"))
            .unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A running server with its main socket and `linked.sock`.
struct Lamp {
    socket: PathBuf,
    linked: PathBuf,
    gid: u32,
    drops: Arc<LinkedDrops>,
    stopper: lamp_memory::server::Stopper,
    serving: Option<JoinHandle<()>>,
}

impl Lamp {
    fn start(scratch: &Scratch) -> Self {
        let (socket, linked) = (scratch.path("memory.sock"), scratch.path("linked.sock"));
        let gid = rustix::process::getegid().as_raw();
        let mut store = Days::open(
            Options {
                dir: scratch.path("store"),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(0),
            },
            NOON,
        )
        .unwrap();
        let server = Server::bind(&socket)
            .unwrap()
            .linked_socket(&linked, gid)
            .unwrap();
        let drops = server.linked_drops();
        let stopper = server.stopper();
        let serving = thread::spawn(move || {
            let mut now = NOON;
            server
                .run(
                    &mut store,
                    &mut || {
                        now += 1;
                        now
                    },
                    &mut |_| {},
                )
                .unwrap();
        });
        Self {
            socket,
            linked,
            gid,
            drops,
            stopper,
            serving: Some(serving),
        }
    }

    fn ask(&self, request: Value) -> Value {
        ask_on(&self.socket, &request.to_string())
    }

    fn ask_linked(&self, request: Value) -> Value {
        ask_on(&self.linked, &request.to_string())
    }

    fn drops(&self) -> [u64; 4] {
        [
            self.drops.no_owner.load(Ordering::Relaxed),
            self.drops.forgotten.load(Ordering::Relaxed),
            self.drops.refused.load(Ordering::Relaxed),
            self.drops.late.load(Ordering::Relaxed),
        ]
    }

    fn stop(mut self) {
        self.stopper.stop();
        self.serving.take().unwrap().join().unwrap();
    }
}

fn ask_on(socket: &Path, line: &str) -> Value {
    let stream = UnixStream::connect(socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut client = BufReader::new(stream);
    client
        .get_mut()
        .write_all(format!("{line}\n").as_bytes())
        .unwrap();
    let mut answer = String::new();
    client.read_line(&mut answer).unwrap();
    serde_json::from_str(&answer).unwrap_or_else(|_| panic!("{line}: {answer}"))
}

/// The voice's task row for P1's task `t-tasks-7`, run as `run-1`.
fn task_row(at: u64) -> Value {
    json!({
        "op": "service_record", "person": P1, "key": "t-tasks-7",
        "service": "lamp_tasks", "action": "task_settled", "status": "done",
        "asked_unix_ms": at, "changed_unix_ms": at,
        "source_kind": "task",
        "source_ref": {"service": "lamp-tasks", "id": "7", "session": "run-1"},
        "summary": "Book the dentist", "deed_unix_ms": at,
    })
}

fn agent_read(task: &str, n: u64, deed: u64) -> Value {
    json!({
        "op": "linked_record", "source_kind": "agent", "task": task,
        "run": "run-1", "n": n, "action": "search_mail", "status": "done",
        "deed_unix_ms": deed, "summary": "Searched mail for the dentist",
        "source_ref": {"service": "gmail", "id": "res-1", "session": "run-1"},
    })
}

fn keys(lamp: &Lamp) -> Vec<String> {
    let listed = lamp.ask(json!({"op":"service_records","person":P1}));
    assert_eq!(listed["ok"], true, "{listed}");
    let mut keys: Vec<String> = listed["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["key"].as_str().unwrap().to_owned())
        .collect();
    keys.sort();
    keys
}

/// The socket is mode 0660 and its group's; it takes `linked_record` and
/// nothing else, one request a connection; the main socket is unchanged.
#[test]
fn the_linked_socket_is_its_groups_and_takes_linked_record_only() {
    let scratch = Scratch::new("only");
    let lamp = Lamp::start(&scratch);
    let meta = fs::metadata(&lamp.linked).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o660);
    assert_eq!(meta.gid(), lamp.gid);
    assert_eq!(
        fs::metadata(&lamp.socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
    for request in [
        json!({"op":"status","person":P1}),
        json!({"op":"service_records","person":P1}),
        json!({"op":"forget_person","person":P1}),
        json!({"op":"forget_all","person":null}),
        task_row(NOON),
    ] {
        let answer = lamp.ask_linked(request.clone());
        assert_eq!(answer["error"], "not_allowed", "{request} {answer}");
    }
    assert!(keys(&lamp).is_empty());
    // One request a connection: after its answer the connection ends.
    let mut stream = UnixStream::connect(&lamp.linked).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let two = format!(
        "{}\n{}\n",
        agent_read("t-tasks-7", 0, NOON),
        agent_read("t-tasks-7", 1, NOON)
    );
    stream.write_all(two.as_bytes()).unwrap();
    let mut all = String::new();
    stream.read_to_string(&mut all).unwrap();
    assert_eq!(all.lines().count(), 1, "{all}");
    assert_eq!(lamp.ask(json!({"op":"status","person":P1}))["ok"], true);
    lamp.stop();
}

/// Memory keeps a record only for the owner of the task row it names, and
/// counts each one it drops by why: no owner, a forget that wins, any
/// other refusal.
#[test]
fn a_linked_record_is_kept_for_its_owner_and_each_drop_is_counted() {
    let scratch = Scratch::new("owner");
    let lamp = Lamp::start(&scratch);
    let none = lamp.ask_linked(agent_read("t-tasks-7", 0, NOON));
    assert_eq!(none["error"], "no_owner", "{none}");
    assert_eq!(lamp.drops(), [1, 0, 0, 0]);
    assert_eq!(lamp.ask(task_row(NOON))["ok"], true);
    let kept = lamp.ask_linked(agent_read("t-tasks-7", 0, NOON));
    assert_eq!(kept["ok"], true, "{kept}");
    assert_eq!(keys(&lamp), ["a-run-1-0", "t-tasks-7"]);
    // Forget everything, then the task row again: a deed from before the
    // forget is still the forget's.
    assert_eq!(lamp.ask(json!({"op":"forget_all","person":P1}))["ok"], true);
    assert_eq!(lamp.ask(task_row(NOON + 1_000))["ok"], true);
    let early = lamp.ask_linked(agent_read("t-tasks-7", 1, NOON - 1_000));
    assert_eq!(early["error"], "forgotten", "{early}");
    let mut kindless = agent_read("t-tasks-7", 2, NOON + 1_000);
    kindless.as_object_mut().unwrap().remove("source_kind");
    assert_eq!(lamp.ask_linked(kindless)["error"], "bad_kind");
    let mut task_kind = agent_read("t-tasks-7", 3, NOON + 1_000);
    task_kind["source_kind"] = json!("task");
    assert_eq!(lamp.ask_linked(task_kind)["error"], "bad_kind");
    assert_eq!(lamp.drops(), [1, 1, 2, 0]);
    assert_eq!(keys(&lamp), ["t-tasks-7"]);
    lamp.stop();
}

/// A writer that connects and says nothing is dropped within
/// `LINKED_PATIENCE` and counted late: the main socket's next client is
/// answered long before the main socket's own two seconds.
#[test]
fn a_quiet_linked_writer_is_dropped_soon_and_counted() {
    let scratch = Scratch::new("quiet");
    let lamp = Lamp::start(&scratch);
    let quiet = UnixStream::connect(&lamp.linked).unwrap();
    // Let the server take the quiet one before the main socket's client.
    thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    assert_eq!(lamp.ask(json!({"op":"status","person":P1}))["ok"], true);
    let waited = started.elapsed();
    assert!(
        waited < LINKED_PATIENCE + Duration::from_millis(750),
        "{waited:?}"
    );
    let begun = Instant::now();
    while lamp.drops()[3] == 0 && begun.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(lamp.drops(), [0, 0, 0, 1]);
    drop(quiet);
    lamp.stop();
}

/// Off by default: a server bound without `linked_socket` has no
/// `linked.sock`, and the switch's name is the documented one.
#[test]
fn the_linked_socket_is_off_unless_opened() {
    let scratch = Scratch::new("off");
    let server = Server::bind(&scratch.path("memory.sock")).unwrap();
    assert!(!scratch.path("linked.sock").exists());
    assert_eq!(
        lamp_memory::server::LINKED_SWITCH,
        "LAMP_MEMORY_LINKED_SOCKET"
    );
    assert_eq!(lamp_memory::server::LINKED_GROUP, "lampos-linked");
    drop(server);
}
