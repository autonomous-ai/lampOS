//! #2176: `forget {about}`, through Voice's production Days client against
//! the real socket and store, with `LAMP_MEMORY_FORGET_ABOUT` on for this
//! test process. All content is synthetic; no provider, credentials,
//! hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Server,
    days::Days,
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
    store::Person,
    tidy::{Category, Output, Proposal},
};
use lamp_voice::remember::{DaysClient, FORGET, RECALL, REMEMBER, switch_forget_about};
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
const BINH: &str = "p-0000000000000b0b";

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
        Self::new_with(name, |_, _| {})
    }

    fn new_with(name: &str, seed: impl FnOnce(&mut Days, u64)) -> Self {
        let root = std::env::temp_dir().join(format!("lamp-mfa-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let mut store = Days::open(
            Options {
                dir: root.join("store"),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(0),
            },
            now,
        )
        .unwrap();
        seed(&mut store, now);
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

/// An unknown guest can remove a personal fact that an older observation
/// pass saved under the room's scope. The room session must have seen that
/// observation; an unrelated room fact survives.
#[test]
fn an_unknown_guests_room_observation_is_forgettable() {
    assert!(switch_forget_about(true));
    let fixture = Fixture::new_with("guest-observation", |store, now| {
        let room = Person::everyone();
        let evidence = store
            .append(&room, now - 1, "I'm allergic to nuts", "I see", &[])
            .unwrap();
        let job = store
            .conversation_end(&room, &format!("c{now}"), &[evidence])
            .unwrap();
        store
            .tidy_apply(
                &room,
                &job.job,
                &Output {
                    observations: vec![Proposal {
                        text: "Allergic to nuts".into(),
                        category: Category::Preference,
                        source_ids: vec![evidence],
                        supersedes: None,
                        on: None,
                        replaces: None,
                    }],
                },
            )
            .unwrap();
        store
            .remember(&room, now, "The plant needs water on Fridays")
            .unwrap();
    });
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let note = client.told_for_session_within(1, None, false, BUDGET);
    assert!(note.contains("Allergic to nuts"), "{note}");
    let (told, ok) = client.run_for(&call(FORGET, json!({"about":"allergy"})), BUDGET, None);
    assert!(
        ok && told["forgotten"] == true && told["count"] == 1,
        "{told}"
    );
    let db = rusqlite::Connection::open_with_flags(
        fixture.root.join("store").join(lamp_memory::days::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let allergy_rows: u64 = db
        .query_row(
            "SELECT count(*) FROM entries WHERE person='everyone' AND (text LIKE '%Allerg%' OR heard LIKE '%allerg%')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(allergy_rows, 0);
    drop(db);
    assert_eq!(
        kept(&client, None, "plant"),
        ["The plant needs water on Fridays"]
    );
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

fn remember(client: &DaysClient, person: Option<&str>, text: &str) {
    let (told, ok) = client.run_for(&call(REMEMBER, json!({"text":text})), BUDGET, person);
    assert!(ok && told["remembered"] == true, "{told}");
}

/// What recall finds kept of `person` about `about`, as texts.
fn kept(client: &DaysClient, person: Option<&str>, about: &str) -> Vec<String> {
    let (told, ok) = client.run_for(&call(RECALL, json!({"about":about})), BUDGET, person);
    assert!(ok, "{told}");
    told["found"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| person.is_none() || item["theirs"] == true)
        .filter_map(|item| Some(item["asked_to_remember"].as_str()?.to_owned()))
        .collect()
}

/// "Forget what I said about my bicycle": both bicycle things go, coffee
/// stays; a word nothing is about forgets nothing; and the same words for
/// one person never reach another's.
#[test]
fn forget_about_removes_that_persons_kept_things_about_it_and_nothing_else() {
    assert!(switch_forget_about(true));
    let fixture = Fixture::new("about");
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    remember(&client, Some(ANA), "synthetic: my bicycle is red");
    remember(&client, Some(ANA), "synthetic: my bicycle is blue now");
    remember(&client, Some(ANA), "synthetic: my coffee is black");
    remember(&client, Some(BINH), "synthetic: my bicycle is green");

    let (told, ok) = client.run_for(&call(FORGET, json!({"about":"piano"})), BUDGET, Some(ANA));
    assert!(ok && told["forgotten"] == false, "{told}");
    assert_eq!(kept(&client, Some(ANA), "bicycle").len(), 2);

    let (told, ok) = client.run_for(&call(FORGET, json!({"about":"bicycle"})), BUDGET, Some(ANA));
    assert!(
        ok && told["forgotten"] == true && told["count"] == 2,
        "{told}"
    );
    assert!(kept(&client, Some(ANA), "bicycle").is_empty());
    assert_eq!(
        kept(&client, Some(ANA), "coffee"),
        ["synthetic: my coffee is black"]
    );
    assert_eq!(
        kept(&client, Some(BINH), "bicycle"),
        ["synthetic: my bicycle is green"]
    );
}

/// With nobody known, the room's kept things this conversation kept go and
/// one it never had stays (as the reviewer guards the room's on #1723), and
/// a kept thing written without accents goes for words with them.
#[test]
fn forget_about_in_the_room_reaches_only_this_conversations_kept_things() {
    assert!(switch_forget_about(true));
    let fixture = Fixture::new("room");
    let earlier = DaysClient::new(fixture.socket.clone()).unwrap();
    remember(&earlier, None, "synthetic: the bicycle pump is in the shed");
    drop(earlier);
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    remember(&client, None, "synthetic: xe dap mau do o ngoai hien");

    let (told, ok) = client.run_for(&call(FORGET, json!({"about":"xe đạp"})), BUDGET, None);
    assert!(ok && told["count"] == 1, "{told}");
    let (told, ok) = client.run_for(&call(FORGET, json!({"about":"bicycle"})), BUDGET, None);
    assert!(
        ok && told["forgotten"] == false && told["left"] == 1,
        "{told}"
    );
    assert_eq!(
        kept(&client, None, "bicycle"),
        ["synthetic: the bicycle pump is in the shed"]
    );
}
