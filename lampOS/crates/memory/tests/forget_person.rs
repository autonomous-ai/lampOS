//! The app's removal of a person (`forget_person`, `docs/memory.md`
//! "Forgetting a person"): the real server on real sockets, the Days store
//! and the legacy one. Nothing is left for the person or their rules scope,
//! nobody else's is touched, the narrow socket takes that one request and
//! nothing else, and a removal that was done is told on `memory/forgot`.
use lamp_memory::{
    Options, Server, Store,
    days::{DATABASE, Days},
    forgot::ForgotOut,
    server::Memory,
};
use lamp_msgs::MemoryForgot;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    io::{BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::Duration,
};

/// 2026-10-13 12:00 UTC: the time the server is told it is.
const NOON: u64 = 1_791_892_800_000;
const P1: &str = "p-0123456789abcdef";
const R1: &str = "r-0123456789abcdef";
const P2: &str = "p-fedcba9876543210";
const R2: &str = "r-fedcba9876543210";
const ROOM: &str = "everyone";

/// A directory of this test's own, under `/tmp` so that the bus's sockets'
/// paths fit in a Unix socket's address.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = PathBuf::from(format!("/tmp/lfp{}{name}", std::process::id()));
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

    fn options(&self) -> Options {
        Options {
            dir: self.path("store"),
            keep_days: 30,
            local: lamp_memory::store::LocalTime::Offset(0),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A group this process is in, by its name in `/etc/group`, for the
/// narrow socket. None where the host's group file names none of them.
fn own_group() -> Option<(String, u32)> {
    let mine: Vec<u32> = std::iter::once(rustix::process::getegid().as_raw())
        .chain(
            rustix::process::getgroups()
                .unwrap_or_default()
                .into_iter()
                .map(|gid| gid.as_raw()),
        )
        .collect();
    fs::read_to_string("/etc/group")
        .ok()?
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            let gid: u32 = fields.get(2)?.parse().ok()?;
            (mine.contains(&gid) && !fields[0].is_empty()).then(|| (fields[0].to_owned(), gid))
        })
}

/// A running server with both sockets and a bus, serving `store`.
struct Lamp {
    socket: PathBuf,
    narrow: PathBuf,
    stopper: lamp_memory::server::Stopper,
    serving: Option<JoinHandle<()>>,
}

impl Lamp {
    fn start<M: Memory + Send + 'static>(scratch: &Scratch, mut store: M) -> Self {
        let (socket, narrow) = (scratch.path("m.sock"), scratch.path("f.sock"));
        let gid = own_group().map_or_else(|| rustix::process::getegid().as_raw(), |(_, gid)| gid);
        let server = Server::bind(&socket)
            .unwrap()
            .forget_socket(&narrow, gid)
            .unwrap()
            .telling_forgets(ForgotOut::on(&scratch.path("b")).unwrap());
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
            narrow,
            stopper,
            serving: Some(serving),
        }
    }

    fn ask(&self, request: Value) -> Value {
        ask_on(&self.socket, &request.to_string())
    }

    fn ask_narrow(&self, line: &str) -> Value {
        ask_on(&self.narrow, line)
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

fn ok(answer: Value) -> Value {
    assert_eq!(answer["ok"], true, "{answer}");
    answer
}

/// One person's own exchanges and kept things, a rule in their rules scope
/// and, with Days, a session note and a tidy job: `word` in each.
fn seed(lamp: &Lamp, person: &str, rules: &str, word: &str, days: bool) {
    let id = ok(
        lamp.ask(json!({"op":"append","person":person,"at_unix_ms":NOON,
        "heard":format!("tell me about the {word}"),"said":format!("the {word} swims")})),
    )["id"]
        .clone();
    ok(
        lamp.ask(json!({"op":"remember","person":person,"at_unix_ms":NOON,
        "text":format!("likes the {word}")})),
    );
    ok(
        lamp.ask(json!({"op":"remember","person":rules,"at_unix_ms":NOON,
        "text":format!("[quiet] no {word} talk")})),
    );
    if days {
        ok(lamp.ask(json!({"op":"session_open","person":person,"session":"s1"})));
        ok(lamp.ask(json!({"op":"conversation_end","person":person,
            "conversation":"c1","source_ids":[id]})));
    }
}

/// Everything a person's memory gives back, to compare before and after.
fn whole(lamp: &Lamp, person: &str, word: &str, days: bool) -> Value {
    let mut seen = json!({
        "status": ok(lamp.ask(json!({"op":"status","person":person}))),
        "kept": ok(lamp.ask(json!({"op":"kept","person":person}))),
        "recall": ok(lamp.ask(json!({"op":"recall","person":person}))),
        "query": ok(lamp.ask(json!({"op":"recall","person":person,"query":word}))),
    });
    if days {
        seen["search"] =
            ok(lamp.ask(json!({"op":"search","person":person,"query":word,"history":true})));
    }
    seen
}

/// Nothing a person's memory gives back has anything in it.
fn nothing_for(lamp: &Lamp, person: &str, word: &str, days: bool) {
    let seen = whole(lamp, person, word, days);
    for status in seen["status"]["people"].as_array().unwrap() {
        assert_eq!(status["exchanges"], 0, "{seen}");
        assert_eq!(status["kept"], 0, "{seen}");
        assert_eq!(status["bytes"], 0, "{seen}");
    }
    for kind in ["kept", "recall", "query"] {
        assert_eq!(seen[kind]["entries"], json!([]), "{kind}: {seen}");
    }
    if days {
        assert_eq!(seen["search"]["entries"], json!([]), "{seen}");
    }
    assert!(!seen.to_string().contains(word), "{seen}");
}

fn told(subscriber: &mut lamp_bus::Subscriber, wait: Duration) -> Option<MemoryForgot> {
    subscriber
        .recv(wait)
        .unwrap()
        .map(|message| message.parse::<MemoryForgot>().unwrap())
}

fn removes_one_person_entirely(days: bool) {
    let scratch = Scratch::new(if days { "d" } else { "l" });
    let root = scratch.path("b");
    fs::create_dir_all(&root).unwrap();
    let mut bus = lamp_bus::Bus::new(&root)
        .unwrap()
        .subscribe_typed(MemoryForgot::TOPIC, "t", MemoryForgot::TYPE)
        .unwrap();
    let lamp = if days {
        Lamp::start(&scratch, Days::open(scratch.options(), NOON).unwrap())
    } else {
        Lamp::start(&scratch, Store::open(scratch.options(), NOON).unwrap())
    };
    seed(&lamp, P1, R1, "zebrafish", days);
    seed(&lamp, P2, R2, "kingfisher", days);
    seed(&lamp, ROOM, "r-everyone", "lighthouse", days);
    let others = [
        (P2, "kingfisher"),
        (R2, "kingfisher"),
        (ROOM, "lighthouse"),
        ("r-everyone", "lighthouse"),
    ];
    let before: Vec<Value> = others
        .iter()
        .map(|(person, word)| whole(&lamp, person, word, days))
        .collect();
    // Something there to forget, and the rules scope with it.
    assert_ne!(
        whole(&lamp, P1, "zebrafish", days)["kept"]["entries"],
        json!([])
    );

    let answer = lamp.ask_narrow(&json!({"op":"forget_person","person":P1}).to_string());
    assert_eq!(answer, json!({"ok":true,"removed":2,"rules":1}));
    assert_eq!(
        told(&mut bus, Duration::from_secs(5)),
        Some(MemoryForgot {
            person: Some(P1.into())
        })
    );

    nothing_for(&lamp, P1, "zebrafish", days);
    nothing_for(&lamp, R1, "zebrafish", days);
    let after: Vec<Value> = others
        .iter()
        .map(|(person, word)| whole(&lamp, person, word, days))
        .collect();
    assert_eq!(before, after, "another's memory changed");

    // Asked again: nothing left, still told. A refused one is not told.
    assert_eq!(
        lamp.ask(json!({"op":"forget_person","person":P1})),
        json!({"ok":true,"removed":0,"rules":0})
    );
    assert!(told(&mut bus, Duration::from_secs(5)).is_some());
    for refused in [
        json!({"op":"forget_person","person":R2}),
        json!({"op":"forget_person","person":ROOM}),
        json!({"op":"forget_person","person":null}),
    ] {
        assert_eq!(
            lamp.ask(refused.clone())["error"],
            "bad_person",
            "{refused}"
        );
    }
    assert_eq!(told(&mut bus, Duration::from_millis(300)), None);
    lamp.stop();

    if days {
        // The rows themselves, with the server gone: none of P1's or their
        // rules', and P2's and the room's still there.
        let db = Connection::open(scratch.path("store").join(DATABASE)).unwrap();
        let count = |sql: &str, people: [&str; 2]| -> u64 {
            db.query_row(sql, people, |row| row.get(0)).unwrap()
        };
        {
            let people = [P1, R1];
            assert_eq!(
                count(
                    "SELECT count(*) FROM entries WHERE person IN (?1,?2)",
                    people
                ),
                0
            );
            assert_eq!(
                count("SELECT count(*) FROM jobs WHERE person IN (?1,?2)", people),
                0
            );
            assert_eq!(
                count(
                    "SELECT count(*) FROM sessions WHERE person IN (?1,?2) AND (note!='' OR ids!='[]' OR invalidated=0)",
                    people
                ),
                0
            );
            assert_eq!(
                count(
                    "SELECT count(*) FROM follow_ups WHERE person IN (?1,?2)",
                    people
                ),
                0
            );
        }
        let matching = |word: &str| -> u64 {
            db.query_row(
                "SELECT count(*) FROM search WHERE search MATCH ?1",
                [word],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(matching("zebrafish"), 0);
        assert!(matching("kingfisher") > 0);
        assert!(matching("lighthouse") > 0);
        assert!(
            count(
                "SELECT count(*) FROM entries WHERE person IN (?1,?2)",
                [P2, R2]
            ) > 0
        );
        assert!(
            count(
                "SELECT count(*) FROM jobs WHERE person IN (?1,?2)",
                [P2, R2]
            ) > 0
        );
    } else {
        // The legacy store's directories: P1's and their rules' are gone.
        let store = scratch.path("store");
        assert!(!store.join(P1).exists());
        assert!(!store.join(R1).exists());
        assert!(store.join(P2).exists());
        assert!(store.join(R2).exists());
        assert!(store.join(ROOM).exists());
    }
}

#[test]
fn forget_person_leaves_nothing_of_one_person_in_days_and_nobody_elses_is_touched() {
    removes_one_person_entirely(true);
}

#[test]
fn forget_person_leaves_nothing_of_one_person_in_the_legacy_store_and_nobody_elses_is_touched() {
    removes_one_person_entirely(false);
}

#[test]
fn the_narrow_socket_takes_forget_person_only_and_refuses_the_rest_untouched() {
    let scratch = Scratch::new("n");
    let lamp = Lamp::start(&scratch, Days::open(scratch.options(), NOON).unwrap());
    // The main socket stays its owner's; the narrow one is for the group.
    let main = fs::metadata(&lamp.socket).unwrap();
    let narrow = fs::metadata(&lamp.narrow).unwrap();
    assert_eq!(main.permissions().mode() & 0o777, 0o600);
    assert_eq!(narrow.permissions().mode() & 0o777, 0o660);
    if let Some((_, gid)) = own_group() {
        assert_eq!(narrow.gid(), gid);
    }
    seed(&lamp, P1, R1, "zebrafish", true);
    seed(&lamp, P2, R2, "kingfisher", true);
    let everything = |lamp: &Lamp| {
        json!([
            ok(lamp.ask(json!({"op":"status","person":null}))),
            whole(lamp, P1, "zebrafish", true),
            whole(lamp, R1, "zebrafish", true),
            whole(lamp, P2, "kingfisher", true),
        ])
    };
    let before = everything(&lamp);
    for (line, error) in [
        (
            json!({"op":"recall","person":P1}).to_string(),
            "not_allowed",
        ),
        (
            json!({"op":"search","person":P1,"query":"zebrafish"}).to_string(),
            "not_allowed",
        ),
        (json!({"op":"kept","person":P1}).to_string(), "not_allowed"),
        (
            json!({"op":"status","person":null}).to_string(),
            "not_allowed",
        ),
        (
            json!({"op":"forget_all","person":null}).to_string(),
            "not_allowed",
        ),
        (
            json!({"op":"forget_all","person":P1}).to_string(),
            "not_allowed",
        ),
        (
            json!({"op":"forget_person","person":P1,"also":true}).to_string(),
            "not_allowed",
        ),
        (
            json!({"op":"forget_person","person":R1}).to_string(),
            "bad_person",
        ),
        (
            json!({"op":"forget_person","person":"Mai"}).to_string(),
            "bad_person",
        ),
        ("forget_person p-0123456789abcdef".into(), "not_allowed"),
    ] {
        let answer = lamp.ask_narrow(&line);
        assert_eq!(answer["ok"], false, "{line}: {answer}");
        assert_eq!(answer["error"], error, "{line}: {answer}");
        assert!(answer.get("entries").is_none() && answer.get("people").is_none());
    }
    assert_eq!(everything(&lamp), before, "a refusal changed the store");
    // The one request it takes.
    assert_eq!(
        lamp.ask_narrow(&json!({"op":"forget_person","person":P1}).to_string()),
        json!({"ok":true,"removed":2,"rules":1})
    );
    nothing_for(&lamp, P1, "zebrafish", true);
    assert_eq!(
        whole(&lamp, P2, "kingfisher", true),
        before[3],
        "another's memory changed"
    );
    lamp.stop();
    assert!(!scratch.path("f.sock").exists());
    assert!(!scratch.path("m.sock").exists());
}
