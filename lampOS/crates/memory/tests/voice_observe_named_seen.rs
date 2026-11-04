//! Heron's P2 on #5284: the observation pass's name evidence, made where it
//! is made. Mai's conversation has two exchanges: in the first her own one
//! face was seen saying "my name is Mai"; in the second another known
//! person's face (Bao's) was seen saying "call me Connie". The writer hands
//! the pass only the first as seen spoken by Mai, so of the scripted model's
//! two name proposals only "Their name is Mai." is given to the memory.
//! Through Voice's production Days client against the real socket and
//! store, with a scripted model. All content is synthetic; no provider,
//! credentials, hardware or network are involved.
use lamp_memory::{
    Options, Server,
    days::{DATABASE, Days},
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::{
    known_faces::memory_person,
    remember::{DaysClient, observe_with},
    speaking_faces::{LineSeen, saw_line},
};
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
        let root = std::env::temp_dir().join(format!("lamp-mns-{}", std::process::id()));
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

fn seen(request: u64, speaker: &str, known: (&str, &str)) -> LineSeen {
    LineSeen {
        request,
        speakers: vec![speaker.to_owned()],
        known: Some((known.0.to_owned(), known.1.to_owned())),
        ..LineSeen::default()
    }
}

#[test]
fn the_writer_hands_the_pass_only_the_exchanges_seen_spoken_by_their_person() {
    // The scripted model: each exchange's self-introduced name, as the
    // person's own, cited from that exchange.
    observe_with(Box::new(|input: &Value| {
        let observations: Vec<Value> = input["records"]
            .as_array()?
            .iter()
            .filter_map(|record| {
                let heard = record["heard"].as_str()?;
                let name = ["Mai", "Connie"]
                    .into_iter()
                    .find(|name| heard.contains(name))?;
                Some(json!({
                    "text": format!("Their name is {name}."),
                    "category": "preference",
                    "source_ids": [record["id"]],
                    "supersedes": null
                }))
            })
            .collect();
        Some(json!({ "observations": observations }))
    }));
    let fixture = Fixture::new();
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let mai = memory_person("Mai");

    // Her own one face seen saying the first line; another known person's
    // face (Bao's) seen saying the second.
    let request = 5_284_100;
    saw_line(seen(request, "b1/f1", ("Mai", "b1/f1")));
    saw_line(seen(request + 1, "b1/f2", ("Bao", "b1/f2")));
    assert!(client.finished_line(
        request,
        fixture.now,
        "synthetic: hello, my name is Mai",
        "Nice to meet you, Mai",
        Some(&mai)
    ));
    assert!(client.finished_line(
        request + 1,
        fixture.now,
        "synthetic: and you can call me Connie",
        "Sure, Connie",
        Some(&mai)
    ));
    assert!(client.end_conversation());
    let (first, _) = fixture.next("append_linked");
    let (second, _) = fixture.next("append_linked");
    assert_eq!(first["person"], mai.as_str(), "{first}");
    assert_eq!(second["person"], mai.as_str(), "{second}");
    let (end, job) = fixture.next("conversation_end");
    assert_eq!(end["submit"], false);
    assert_eq!(
        job["input"]["records"].as_array().map(Vec::len),
        Some(2),
        "{job}"
    );
    let (apply, kept) = fixture.next("tidy_apply");
    assert_eq!(apply["job"], job["input"]["job"]);
    assert_eq!(apply["person"], mai.as_str(), "{apply}");
    let texts: Vec<&str> = apply["observations"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|observation| observation["text"].as_str())
        .collect();
    assert_eq!(texts, ["Their name is Mai."], "{apply}");
    assert_eq!(kept["ok"], true, "{kept}");
    assert_eq!(kept["ids"].as_array().unwrap().len(), 1, "{kept}");
    assert_eq!(
        fixture.count("SELECT count(*) FROM entries WHERE kind='observation' AND active=1"),
        1
    );
}
