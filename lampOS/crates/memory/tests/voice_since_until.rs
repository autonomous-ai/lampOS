//! #2510 take #4: since and until for a corrected fact, through Voice's
//! production Days client against the real socket and store, with
//! `LAMP_MEMORY_REPLACES` (and `LAMP_MEMORY_FORGET_ALL`, to forget by id)
//! on for this test process. History only when the person asks about their
//! own; never the room's. All content is synthetic; no provider,
//! credentials, hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::Days,
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
};
use lamp_voice::remember::{
    DaysClient, FORGET, RECALL, REMEMBER, switch_forget_all, switch_replaces,
};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    thread::JoinHandle,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const BUDGET: Duration = Duration::from_millis(1500);
const ANA: &str = "p-0000000000000a0a";
const VIOLET: &str = "synthetic: my favourite colour is violet";
const GREEN: &str = "synthetic: my favourite colour is green";

struct Plain(Days);

impl Memory for Plain {
    fn answer(&mut self, line: &[u8], now: u64) -> Answer {
        days_protocol::answer(&mut self.0, line, now)
    }
}

struct Fixture {
    root: PathBuf,
    socket: PathBuf,
    stop: Stopper,
    worker: Option<JoinHandle<()>>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("lamp-msu-{}-{name}", std::process::id()));
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
                local: lamp_memory::store::LocalTime::Offset(0),
            },
            now,
        )
        .unwrap();
        let socket = root.join("m.sock");
        let server = Server::bind(&socket).unwrap();
        let stop = server.stopper();
        let worker = std::thread::spawn(move || {
            server
                .run(&mut Plain(store), &mut || now, &mut |_| {})
                .unwrap();
        });
        Self {
            root,
            socket,
            stop,
            worker: Some(worker),
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
        id: "c".into(),
        name: name.into(),
        args,
    }
}

/// Violet kept, then corrected to green by violet's id: green's id.
fn violet_then_green(client: &DaysClient, person: Option<&str>) -> (u64, u64) {
    let (told, ok) = client.run_for(&call(REMEMBER, json!({"text":VIOLET})), BUDGET, person);
    assert!(ok && told["remembered"] == true, "{told}");
    let violet = found(client, person, false)[0]["source_id"]
        .as_u64()
        .unwrap();
    let (told, ok) = client.run_for(
        &call(REMEMBER, json!({"text":GREEN,"replaces":violet})),
        BUDGET,
        person,
    );
    assert!(ok && told["replaced"] == violet, "{told}");
    let green = found(client, person, false)[0]["source_id"]
        .as_u64()
        .unwrap();
    (violet, green)
}

/// What recall about "colour" finds kept, with or without history.
fn found(client: &DaysClient, person: Option<&str>, history: bool) -> Vec<Value> {
    let mut args = json!({"about":"colour"});
    if history {
        args["history"] = json!(true);
    }
    let (told, ok) = client.run_for(&call(RECALL, args), BUDGET, person);
    assert!(ok, "{told}");
    told["found"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item.get("asked_to_remember").is_some())
        .cloned()
        .collect()
}

/// Violet, then green by `replaces`: recall gives green since today; asked
/// about their own past, both, violet until today; forgetting green removes
/// both.
#[test]
fn recall_says_since_and_gives_history_with_until_only_when_asked() {
    switch_replaces(true);
    switch_forget_all(true);
    let fixture = Fixture::new("own");
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let (violet, green) = violet_then_green(&client, Some(ANA));

    let now = found(&client, Some(ANA), false);
    assert_eq!(now.len(), 1, "{now:?}");
    assert_eq!(now[0]["asked_to_remember"], GREEN);
    assert_eq!(now[0]["since"], "since today");
    assert!(now[0].get("until").is_none(), "{now:?}");

    let then = found(&client, Some(ANA), true);
    assert_eq!(then.len(), 2, "{then:?}");
    for item in &then {
        assert_eq!(item["since"], "since today");
        match item["source_id"].as_u64().unwrap() {
            id if id == violet => {
                assert_eq!(item["asked_to_remember"], VIOLET);
                assert_eq!(item["until"], "until today", "{then:?}");
            }
            id => {
                assert_eq!(id, green);
                assert!(item.get("until").is_none(), "{then:?}");
            }
        }
    }

    let (told, ok) = client.run_for(&call(FORGET, json!({"source_id":green})), BUDGET, Some(ANA));
    assert!(ok && told["forgotten"] == true, "{told}");
    assert!(found(&client, Some(ANA), true).is_empty());
}

/// Finding L on #3504: with nobody known, history asked for gives the
/// room's own corrected version too, with until when; someone known asking
/// for theirs still never gets the room's replaced version.
#[test]
fn the_rooms_scope_gives_its_own_history_only_to_its_own_recall() {
    switch_replaces(true);
    switch_forget_all(true);
    let fixture = Fixture::new("room");
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let (violet, green) = violet_then_green(&client, None);

    let items = found(&client, None, true);
    assert_eq!(items.len(), 2, "{items:?}");
    assert!(
        items
            .iter()
            .any(|item| item["source_id"] == green && item.get("until").is_none())
    );
    assert!(
        items
            .iter()
            .any(|item| item["source_id"] == violet && item.get("until").is_some())
    );
    let items = found(&client, Some(ANA), true);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["source_id"], green);
    assert!(items[0].get("until").is_none(), "{items:?}");
}
