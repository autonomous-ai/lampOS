//! The real server on a real socket, in a thread of the test, asked what
//! another process on the lamp would ask it.
use lamp_memory::{Options, Person, Server, Store, protocol::MOST_LINE_BYTES, server::Stopper};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::Shutdown,
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

/// 2026-10-13 12:00 UTC: the time the server is told it is.
const NOON: u64 = 1_791_892_800_000;

/// A directory of this test's own. Its name is short: a socket's path has
/// to fit in about a hundred bytes.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lm-{}-s-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Running {
    socket: PathBuf,
    stopper: Stopper,
    thread: JoinHandle<io::Result<()>>,
    logged: Arc<Mutex<Vec<String>>>,
}

fn open(scratch: &Scratch) -> Store {
    let options = Options {
        dir: scratch.0.join("store"),
        keep_days: 30,
        local: lamp_memory::store::LocalTime::Offset(420),
    };
    Store::open(options, NOON).unwrap()
}

/// Bind, open the store and serve in a thread. The clock is the test's: a
/// millisecond past noon for each request.
fn start(scratch: &Scratch, patience: Duration) -> Running {
    let socket = scratch.0.join("m.sock");
    let server = Server::bind(&socket).unwrap().patience(patience);
    let mut store = open(scratch);
    let stopper = server.stopper();
    let logged = Arc::new(Mutex::new(Vec::new()));
    let log_to = Arc::clone(&logged);
    let thread = thread::spawn(move || {
        let mut now = NOON;
        let mut clock = || {
            now += 1;
            now
        };
        let mut log = |line: &str| log_to.lock().unwrap().push(line.to_owned());
        server.run(&mut store, &mut clock, &mut log)
    });
    Running {
        socket,
        stopper,
        thread,
        logged,
    }
}

impl Running {
    fn stop(self) -> Vec<String> {
        self.stopper.stop();
        self.thread.join().unwrap().unwrap();
        Arc::try_unwrap(self.logged).unwrap().into_inner().unwrap()
    }
}

struct Client(BufReader<UnixStream>);

impl Client {
    fn connect(socket: &Path) -> Self {
        let stream = UnixStream::connect(socket).unwrap();
        // Only so that a server that never answers fails the test and does
        // not hang it.
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        Self(BufReader::new(stream))
    }

    fn send(&mut self, bytes: &[u8]) {
        self.0.get_mut().write_all(bytes).unwrap();
    }

    fn answer(&mut self) -> Value {
        let mut line = String::new();
        self.0.read_line(&mut line).unwrap();
        assert!(line.ends_with('\n'), "{line:?}");
        serde_json::from_str(&line).unwrap()
    }

    fn ask(&mut self, request: Value) -> Value {
        self.send(format!("{request}\n").as_bytes());
        self.answer()
    }

    /// The server has closed the connection, and sent nothing first.
    fn closed(&mut self) -> bool {
        let mut rest = Vec::new();
        matches!(self.0.read_to_end(&mut rest), Ok(0))
    }
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn the_server_answers_over_its_socket_and_a_bad_line_does_not_end_the_connection() {
    let scratch = Scratch::new("talk");
    let running = start(&scratch, Duration::from_secs(2));
    let mut client = Client::connect(&running.socket);
    let added = client.ask(
        json!({"op":"append","at_unix_ms":NOON,"heard":"where are the tulip bulbs","said":"In the shed."}),
    );
    assert_eq!(added, json!({"ok":true,"id":1}));

    // It has answered, so it is ready and says so beside its socket.
    let ready = scratch.0.join("ready");
    assert_eq!(fs::read_to_string(&ready).unwrap(), "ready\n");
    assert_eq!(mode(&ready), 0o600);
    assert!(
        fs::metadata(&running.socket)
            .unwrap()
            .file_type()
            .is_socket()
    );
    assert_eq!(mode(&running.socket), 0o600);
    assert_eq!(scratch.names(), ["m.sock", "ready", "store"]);
    assert_eq!(mode(&scratch.0.join("store")), 0o700);

    // A line that is not JSON, then a good one on the same connection.
    client.send(b"remember the tulip bulbs\n");
    let refused = client.answer();
    assert_eq!(
        (&refused["ok"], &refused["error"]),
        (&json!(false), &json!("bad_json"))
    );
    let kept = client.ask(
        json!({"op":"remember","at_unix_ms":NOON + 5,"text":"tulip bulbs go in before the frost"}),
    );
    assert_eq!(kept, json!({"ok":true,"id":2}));
    // An op there is not, a person there cannot be, and on it goes.
    assert_eq!(client.ask(json!({"op":"recollect"}))["error"], "unknown_op");
    assert_eq!(
        client.ask(json!({"op":"recall","person":"../x"}))["error"],
        "bad_person"
    );
    assert_eq!(
        client.ask(json!({"op":"append","at_unix_ms":NOON}))["error"],
        "empty"
    );

    // A line over 64 KiB is refused whole, wherever it stops, and the next
    // one is read from its own beginning.
    let long = json!({"op":"remember","at_unix_ms":NOON,"text":"tulip ".repeat(12_000)});
    assert!(long.to_string().len() > MOST_LINE_BYTES);
    assert_eq!(client.ask(long)["error"], "too_long");
    client.send(&[b'x'; 300_000]);
    client.send(b"\n");
    assert_eq!(client.answer()["error"], "too_long");
    let mut most = br#"{"op":"recall","query":"TULIP"}"#.to_vec();
    most.resize(MOST_LINE_BYTES, b' ');
    client.send(&most);
    client.send(b"\n");
    let found = client.answer();
    assert_eq!(
        found,
        json!({"ok":true,"entries":[
            {"kind":"exchange","id":1,"at_unix_ms":NOON,"heard":"where are the tulip bulbs","said":"In the shed."},
            {"kind":"kept","id":2,"at_unix_ms":NOON + 5,"text":"tulip bulbs go in before the frost"}
        ]})
    );
    most.push(b' ');
    client.send(&most);
    client.send(b"\n");
    assert_eq!(client.answer()["error"], "too_long");
    drop(client);

    // Another client: two requests sent at once are answered in order.
    let mut client = Client::connect(&running.socket);
    client.send(b"{\"op\":\"kept\"}\n{\"op\":\"recall\",\"day\":\"2026-10-13\"}\n");
    assert_eq!(client.answer()["entries"][0]["id"], 2);
    assert_eq!(client.answer()["entries"][0]["id"], 1);
    let status = client.ask(json!({"op":"status"}));
    let on_disk: u64 = fs::read_dir(scratch.0.join("store/everyone"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    assert_eq!(status["people"][0]["bytes"], on_disk);
    assert_eq!(status["people"][0]["person"], "everyone");
    assert_eq!(
        (
            &status["people"][0]["exchanges"],
            &status["people"][0]["kept"]
        ),
        (&json!(1), &json!(1))
    );
    assert_eq!(status["people"][0]["newest_day"], "2026-10-13");
    drop(client);

    // A last line without its end, from a client that then stops writing.
    let mut client = Client::connect(&running.socket);
    client.send(b"{\"op\":\"forget_last\"}");
    client.0.get_mut().shutdown(Shutdown::Write).unwrap();
    assert_eq!(
        client.answer()["removed"],
        json!({"kind":"kept","id":2,"at_unix_ms":NOON + 5})
    );
    assert!(client.closed());

    // What is on the disk is what was answered for.
    let journal = scratch.0.join("store/everyone/journal-2026-10-13.jsonl");
    assert_eq!(
        fs::read_to_string(journal).unwrap(),
        format!(
            "{{\"id\":1,\"at_unix_ms\":{NOON},\"heard\":\"where are the tulip bulbs\",\"said\":\"In the shed.\"}}\n"
        )
    );
    assert!(!scratch.0.join("store/everyone/kept.jsonl").exists());
    assert!(!scratch.0.join("x").exists());

    // Stopped in good order, it takes its socket and its word away.
    let logged = running.stop();
    assert_eq!(scratch.names(), ["store"]);
    assert_eq!(
        logged,
        [
            "append person=everyone ok id=1",
            "? refused error=bad_json",
            "remember person=everyone ok id=2",
            "? refused error=unknown_op",
            "recall person=? refused error=bad_person",
            "append person=everyone refused error=empty",
            "? refused error=too_long",
            "? refused error=too_long",
            "recall person=everyone ok entries=2 bytes=71",
            "? refused error=too_long",
            "kept person=everyone ok entries=1 bytes=34",
            "recall person=everyone ok entries=1 bytes=37",
            &format!(
                "status person=* ok people=1 exchanges=1 kept=1 bytes={on_disk} skipped_lines=0"
            ),
            "forget_last person=everyone ok removed=1 kind=kept id=2",
        ]
    );
    // Not a word of what was said or kept is in the log.
    assert!(
        logged
            .iter()
            .all(|line| !line.to_lowercase().contains("tulip"))
    );
}

#[test]
fn a_client_that_says_nothing_or_half_a_request_is_dropped_and_the_next_is_served() {
    let scratch = Scratch::new("quiet");
    // Enough kept beforehand that the answer listing it all is far larger
    // than a socket holds.
    let mut store = open(&scratch);
    for n in 0..150 {
        let fact = format!("{n:0>300}");
        store.remember(&Person::everyone(), NOON, &fact).unwrap();
    }
    drop(store);
    // No client below is ever late itself: each has sent what it is going
    // to send before the server turns to it.
    let running = start(&scratch, Duration::from_millis(200));
    // Three clients at once. The first says nothing; the second stops in
    // the middle of a request; the third asks, and is kept waiting by the
    // two before it for no longer than the server's patience with each.
    let mut silent = Client::connect(&running.socket);
    let mut half = Client::connect(&running.socket);
    half.send(b"{\"op\":\"remember\",\"at_unix_ms\":1,\"text\":\"a half-said secr");
    let mut third = Client::connect(&running.socket);
    let added = third.ask(json!({"op":"remember","at_unix_ms":NOON,"text":"whole"}));
    assert_eq!(added, json!({"ok":true,"id":151}));
    drop(third);
    assert!(silent.closed());
    assert!(half.closed());
    // One that asks, is answered and then stays without closing is dropped
    // the same way, and nothing of the half request was kept.
    let mut lingering = Client::connect(&running.socket);
    let kept = lingering.ask(json!({"op":"kept","max_bytes":400}));
    assert_eq!(kept["entries"].as_array().unwrap().len(), 2);
    assert_eq!(kept["entries"][1]["text"], "whole");
    assert!(lingering.closed());
    // One that asks for answers and never reads them does not hold the
    // server either.
    let mut deaf = Client::connect(&running.socket);
    for _ in 0..40 {
        deaf.send(b"{\"op\":\"kept\",\"max_bytes\":1000000}\n");
    }
    let mut after = Client::connect(&running.socket);
    assert_eq!(after.ask(json!({"op":"status"}))["people"][0]["kept"], 151);
    drop(after);
    drop(deaf);

    let logged = running.stop();
    let dropped = |what: &str| logged.iter().filter(|line| line.contains(what)).count();
    assert_eq!(dropped("did not finish its request in time"), 1);
    assert_eq!(dropped("did not take its answer in time"), 1);
    assert!(logged.iter().all(|line| !line.contains("secr")));
}

#[test]
fn a_socket_nobody_answers_on_is_replaced_and_anything_else_at_the_path_is_left() {
    let scratch = Scratch::new("stale");
    let socket = scratch.0.join("m.sock");
    // As a killed process leaves them: a socket file with nobody behind
    // it, and the word that it is ready.
    drop(UnixListener::bind(&socket).unwrap());
    fs::write(scratch.0.join("ready"), "ready\n").unwrap();
    assert!(UnixStream::connect(&socket).is_err());
    let running = start(&scratch, Duration::from_secs(2));
    let mut client = Client::connect(&running.socket);
    assert_eq!(
        client.ask(json!({"op":"status"})),
        json!({"ok":true,"people":[]})
    );
    drop(client);
    assert_eq!(mode(&socket), 0o600);
    assert_eq!(scratch.names(), ["m.sock", "ready", "store"]);

    // A second server on a socket that is answered does not take it, and
    // the first goes on answering.
    let second = Server::bind(&socket).err().unwrap();
    assert_eq!(second.kind(), io::ErrorKind::AddrInUse);
    assert_eq!(
        fs::read_to_string(scratch.0.join("ready")).unwrap(),
        "ready\n"
    );
    let mut client = Client::connect(&running.socket);
    assert_eq!(
        client.ask(json!({"op":"status"})),
        json!({"ok":true,"people":[]})
    );
    drop(client);
    running.stop();
    assert_eq!(scratch.names(), ["store"]);

    // Between binding and serving there is a socket and no word yet.
    let waiting = Server::bind(&socket).unwrap();
    assert_eq!(scratch.names(), ["m.sock", "store"]);
    drop(waiting);
    assert_eq!(scratch.names(), ["store"]);

    // Something at the path that is not a socket is not this server's to
    // remove.
    let file = scratch.0.join("notes");
    fs::write(&file, "an operator's file").unwrap();
    assert_eq!(
        Server::bind(&file).err().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "an operator's file");
    assert_eq!(
        Server::bind(&scratch.0.join("store")).err().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert!(Server::bind(&scratch.0.join("missing/m.sock")).is_err());
    assert!(Server::bind(&scratch.0.join("n".repeat(200))).is_err());
    assert!(Server::bind(Path::new("m.sock")).is_err());
    assert_eq!(scratch.names(), ["notes", "store"]);
}
