//! #1693: forgetting that reaches everything, through Voice's production
//! Days client against the real socket and store, with
//! `LAMP_MEMORY_FORGET_ALL` on for this test process. All content is
//! synthetic; no provider, credentials, hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::{DATABASE, Days},
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
    tidy::{Category, Proposal},
};
use lamp_voice::remember::{
    Carry, DaysClient, FORGET, FORGET_EVERYTHING, RECALL, REMEMBER, named_forget_with,
    switch_forget_all, tools_with,
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
            "lamp-mv-forget-{}-{}",
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

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "c".into(),
        name: name.into(),
        args,
    }
}

fn remember(client: &DaysClient, text: &str) {
    let (told, ok) = client.run(&call(REMEMBER, json!({"text":text})), BUDGET);
    assert!(ok && told["remembered"] == true, "{told}");
}

/// The source_id recall gave for what it found about `about`.
fn recalled(client: &DaysClient, about: &str) -> Option<u64> {
    let (told, ok) = client.run(&call(RECALL, json!({"about":about})), BUDGET);
    assert!(ok, "{told}");
    told["found"].as_array()?.first()?["source_id"].as_u64()
}

/// "Forget that" after a recall: what recall gave, by its id, is gone with
/// what was linked to it, and something else kept stays. An id this session
/// was not shown is refused, and nothing is removed.
#[test]
fn forget_that_removes_what_recall_gave_and_nothing_else() {
    assert!(switch_forget_all(true));
    let fixture = Fixture::new(|_, _| {}, None);
    let client = fixture.client();
    remember(&client, "synthetic violet umbrella");
    remember(&client, "synthetic orange kettle");
    let violet = recalled(&client, "violet").expect("recall finds it");
    let before = fixture.count("entries");
    assert_eq!(
        client.run(&call(FORGET, json!({"source_id": violet + 1000})), BUDGET),
        (
            json!({"forgotten": false, "note": "That is not something recall gave me just now. Call recall, then forget it by its source_id."}),
            true
        )
    );
    assert_eq!(fixture.count("entries"), before);
    let (told, ok) = client.run(&call(FORGET, json!({"source_id": violet})), BUDGET);
    assert!(ok && told["forgotten"] == true, "{told}");
    assert_eq!(recalled(&client, "violet"), None);
    assert!(recalled(&client, "orange").is_some());
}

/// "Forget everything": for someone known, every entry kept of theirs is
/// gone, journal included; with nobody known it is refused and the room's
/// memory stays. The tool is offered with the switch, with `forget`'s
/// `source_id`.
#[test]
fn forget_everything_removes_all_of_someone_known_and_never_the_room_s() {
    assert!(switch_forget_all(true));
    let names: Vec<String> = tools_with(false)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert!(names.contains(&FORGET_EVERYTHING.to_owned()), "{names:?}");
    let forget = tools_with(false)
        .into_iter()
        .find(|tool| tool.name == FORGET)
        .unwrap();
    assert!(forget.parameters.unwrap()["properties"]["source_id"].is_object());
    let fixture = Fixture::new(|_, _| {}, None);
    let client = fixture.client();
    // Something the room keeps, which is the household's.
    remember(&client, "synthetic room clock");
    // Someone known, and what they asked to keep and said.
    const ANA: &str = "p-0123456789abcdef";
    let run = |name, args| client.run_for(&call(name, args), BUDGET, Some(ANA));
    let (told, ok) = run(REMEMBER, json!({"text":"synthetic teal lantern"}));
    assert!(ok && told["remembered"] == true, "{told}");
    assert!(client.finished_exchange_for(
        fixture.now,
        "what was it",
        "the synthetic teal lantern",
        Some(ANA)
    ));
    let (append, _) = fixture.next("append_linked");
    assert_eq!(append["person"], ANA);
    let before = fixture.count("entries");
    assert!(before >= 3);
    // With nobody known, everything is refused, and nothing goes: the room's
    // memory is the household's (the reviewer on #1723).
    let (told, ok) = client.run(&call(FORGET_EVERYTHING, json!({})), BUDGET);
    assert!(ok && told["forgotten"] == false, "{told}");
    assert_eq!(fixture.count("entries"), before);
    // For them, all of theirs goes, journal included, and the room's stays.
    let (told, ok) = run(FORGET_EVERYTHING, json!({}));
    assert!(
        ok && told["forgotten"] == true && told["everything"] == true,
        "{told}"
    );
    assert_eq!(fixture.count("entries"), 1);
    let (told, _) = run(RECALL, json!({"about":"teal"}));
    assert!(!told.to_string().contains("teal lantern"), "{told}");
    assert!(recalled(&client, "clock").is_some());
    // Nothing left of theirs: said so, not as a failure.
    let (told, ok) = run(FORGET_EVERYTHING, json!({}));
    assert!(ok && told["forgotten"] == false, "{told}");
}

/// #1693: something remembered goes with the conversation it came up in.
/// The earlier turn where it was said is kept with it (the voice sends the
/// conversation's journalled exchanges with `remember`), so "forget that"
/// removes it too; an earlier conversation about something else stays.
#[test]
fn forget_that_removes_the_conversation_a_kept_thing_came_up_in() {
    assert!(switch_forget_all(true));
    let fixture = Fixture::new(|_, _| {}, None);
    let client = fixture.client();
    // An earlier conversation, about something else.
    assert!(client.finished_exchange(fixture.now, "synthetic: what's the weather", "Sunny"));
    let (_, weather) = fixture.next("append_linked");
    assert!(client.end_conversation());
    fixture.next("conversation_end");
    // This conversation: said, then asked to be kept.
    assert!(client.finished_exchange(
        fixture.now,
        "synthetic: my violet umbrella is in the car",
        "Good to know"
    ));
    let (_, said) = fixture.next("append_linked");
    // The writer takes the store's answer a moment after the store gives
    // it; on a lamp the next turn is seconds later.
    std::thread::sleep(Duration::from_millis(200));
    remember(&client, "synthetic violet umbrella in the car");
    let (keep, _) = fixture.next("remember");
    assert_eq!(keep["source_ids"], json!([said["id"]]));
    let violet = recalled(&client, "violet").expect("recall finds it");
    let (told, ok) = client.run(&call(FORGET, json!({"source_id": violet})), BUDGET);
    assert!(ok && told["forgotten"] == true, "{told}");
    // The kept thing and the turn it came from are gone; the weather stays.
    assert_eq!(fixture.count("entries"), 1);
    assert_eq!(recalled(&client, "umbrella"), None);
    assert!(recalled(&client, "weather").is_some(), "{weather}");
}

/// Second's P2 on #2003: after "forget that" removed a kept thing with its
/// conversation, the next "remember …" in that conversation is kept, not
/// refused for the forgotten turns' ids; the same after "forget everything"
/// for someone known.
#[test]
fn after_forgetting_the_next_thing_remembered_is_kept() {
    assert!(switch_forget_all(true));
    let fixture = Fixture::new(|_, _| {}, None);
    let client = fixture.client();
    assert!(client.finished_exchange(
        fixture.now,
        "synthetic: my violet umbrella is in the car",
        "Good to know"
    ));
    fixture.next("append_linked");
    // The writer takes the store's answer a moment after it is given.
    std::thread::sleep(Duration::from_millis(200));
    remember(&client, "synthetic violet umbrella in the car");
    let violet = recalled(&client, "violet").expect("recall finds it");
    let (told, ok) = client.run(&call(FORGET, json!({"source_id": violet})), BUDGET);
    assert!(ok && told["forgotten"] == true, "{told}");
    // Something else, in the same conversation: kept.
    remember(&client, "synthetic orange kettle");
    assert!(recalled(&client, "kettle").is_some());

    // Someone known: their turn, a thing kept with it, then everything.
    const ANA: &str = "p-0123456789abcdef";
    let fresh = fixture.client();
    assert!(fresh.finished_exchange_for(
        fixture.now,
        "synthetic: my bike is the blue one",
        "Noted",
        Some(ANA)
    ));
    fixture.next("append_linked");
    std::thread::sleep(Duration::from_millis(200));
    let run_fresh = |name, args| fresh.run_for(&call(name, args), BUDGET, Some(ANA));
    let (told, ok) = run_fresh(REMEMBER, json!({"text":"synthetic blue bike"}));
    assert!(ok && told["remembered"] == true, "{told}");
    let (told, ok) = run_fresh(FORGET_EVERYTHING, json!({}));
    assert!(ok && told["everything"] == true, "{told}");
    let (told, ok) = run_fresh(REMEMBER, json!({"text":"synthetic green scarf"}));
    assert!(ok && told["remembered"] == true, "{told}");
}

/// One request on the store's socket, as the observe pass's own client
/// sends it, and the store's answer.
fn raw(socket: &std::path::Path, request: Value) -> Value {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
    writeln!(stream, "{request}").unwrap();
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).unwrap();
    serde_json::from_str(&answer).unwrap()
}

/// What `person` keeps and was noticed of, as texts.
fn listed(socket: &std::path::Path, person: &str) -> Vec<String> {
    let answer = raw(
        socket,
        json!({"op":"kept","person":person,"observations":true,"max_bytes":16384}),
    );
    let mut texts: Vec<String> = answer["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["text"].as_str().unwrap().to_owned())
        .collect();
    texts.sort();
    texts
}

/// #3113, Twelve's c0a4 run with `LAMP_MEMORY_FORGET_ALL` on (the third reviewer's
/// P1 on #3127): in one conversation the colour is said, the wedding is
/// kept (with that conversation's exchange as its source), and the observe
/// pass notices the colour from the same exchange. "Forget my favourite
/// colour" removes the colour and the exchange it was noticed from; the
/// wedding stays.
#[test]
fn forget_my_favourite_colour_leaves_the_wedding_kept_in_the_same_conversation() {
    assert!(switch_forget_all(true));
    const MAI: &str = "p-0000000000000a0a";
    let fixture = Fixture::new(|_, _| {}, None);
    let client = fixture.client();
    assert!(client.finished_exchange_for(
        fixture.now,
        "synthetic: my favourite colour is violet",
        "Lovely",
        Some(MAI)
    ));
    let (_, said) = fixture.next("append_linked");
    let said = said["id"].as_u64().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let wedding = "synthetic: sister's wedding is on Saturday";
    let (told, ok) = client.run_for(&call(REMEMBER, json!({"text":wedding})), BUDGET, Some(MAI));
    assert!(ok && told["remembered"] == true, "{told}");
    let (keep, _) = fixture.next("remember");
    assert_eq!(keep["source_ids"], json!([said]), "FORGET_ALL's sources");
    // The observe pass: the colour, noticed from the same exchange.
    let ended = raw(
        &fixture.socket,
        json!({"op":"conversation_end","person":MAI,"conversation":"c0a4","source_ids":[said]}),
    );
    let colour = "synthetic: favourite colour is violet";
    let noticed = Proposal {
        text: colour.into(),
        category: Category::Preference,
        source_ids: vec![said],
        supersedes: None,
        replaces: None,
        on: None,
    };
    let applied = raw(
        &fixture.socket,
        json!({"op":"tidy_apply","person":MAI,"job":ended["input"]["job"],"observations":[noticed]}),
    );
    assert_eq!(applied["ok"], true, "{applied}");
    let mut both = vec![colour.to_owned(), wedding.to_owned()];
    both.sort();
    assert_eq!(listed(&fixture.socket, MAI), both);

    let model = call(FORGET, json!({}));
    let carry = Carry::default();
    let named = named_forget_with(
        &model,
        Some("Lamp, please forget my favourite colour."),
        Some(MAI),
        &carry,
    )
    .expect("a named forget");
    let (told, ok) = client.run_for(&named, BUDGET, Some(MAI));
    assert!(
        ok && told["forgotten"] == true && told["count"] == 1,
        "{told}"
    );
    assert_eq!(listed(&fixture.socket, MAI), [wedding], "the wedding stays");
    let (told, ok) = client.run_for(&call(RECALL, json!({"about":"colour"})), BUDGET, Some(MAI));
    assert!(ok && !told.to_string().contains("violet"), "{told}");
}
