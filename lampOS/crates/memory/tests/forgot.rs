//! `memory/forgot` (the first builder, 10:5x on #2467): the real server on a
//! real socket, with a bus, tells each forget of everything kept under a
//! person after its answer, and nothing else.
use lamp_memory::{Options, Server, Store, forgot::ForgotOut};
use lamp_msgs::MemoryForgot;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    thread,
    time::Duration,
};

/// 2026-10-13 12:00 UTC: the time the server is told it is.
const NOON: u64 = 1_791_892_800_000;

/// A directory of this test's own, under `/tmp` so that the bus's sockets'
/// paths fit in a Unix socket's address, as the bus's own tests do.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = PathBuf::from(format!("/tmp/lm{}{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ask(client: &mut BufReader<UnixStream>, request: Value) -> Value {
    client
        .get_mut()
        .write_all(format!("{request}\n").as_bytes())
        .unwrap();
    let mut line = String::new();
    client.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn a_forget_of_everything_kept_under_a_person_is_told_on_the_bus_and_nothing_else() {
    let scratch = Scratch::new("f");
    let root = scratch.0.join("b");
    fs::create_dir_all(&root).unwrap();
    let mut told = lamp_bus::Bus::new(&root)
        .unwrap()
        .subscribe_typed(MemoryForgot::TOPIC, "t", MemoryForgot::TYPE)
        .unwrap();
    let socket = scratch.0.join("m.sock");
    let server = Server::bind(&socket)
        .unwrap()
        .telling_forgets(ForgotOut::on(&root).unwrap());
    let stopper = server.stopper();
    let mut store = Store::open(
        Options {
            dir: scratch.0.join("store"),
            keep_days: 30,
            local: lamp_memory::store::LocalTime::Offset(420),
        },
        NOON,
    )
    .unwrap();
    let serving = thread::spawn(move || {
        let mut now = NOON;
        server.run(
            &mut store,
            &mut || {
                now += 1;
                now
            },
            &mut |_| {},
        )
    });
    let stream = UnixStream::connect(&socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut client = BufReader::new(stream);
    let next = |told: &mut lamp_bus::Subscriber, wait: Duration| {
        told.recv(wait)
            .unwrap()
            .map(|message| message.parse::<MemoryForgot>().unwrap())
    };

    let answered = ask(
        &mut client,
        json!({"op":"forget_all","person":"p-0123456789abcdef"}),
    );
    assert_eq!(answered["ok"], true, "{answered}");
    assert_eq!(
        next(&mut told, Duration::from_secs(5)),
        Some(MemoryForgot {
            person: Some("p-0123456789abcdef".into())
        })
    );
    let answered = ask(&mut client, json!({"op":"forget_all","person":null}));
    assert_eq!(answered["ok"], true, "{answered}");
    assert_eq!(
        next(&mut told, Duration::from_secs(5)),
        Some(MemoryForgot { person: None })
    );
    // A refused forget, one of an id memory doesn't make (which could be a
    // name), a forget of the last thing, and a status: none told.
    for request in [
        json!({"op":"forget_all","person":"NOT AN ID"}),
        json!({"op":"forget_all","person":"mai"}),
        json!({"op":"forget_last","person":"p-0123456789abcdef"}),
        json!({"op":"status","person":null}),
    ] {
        ask(&mut client, request);
    }
    assert_eq!(next(&mut told, Duration::from_millis(300)), None);
    drop(client);
    stopper.stop();
    serving.join().unwrap().unwrap();
}
