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

/// The Core-verifier's GATE 23 FAIL on lamp-52e6 (stable 9980, 4 Nov), with
/// `LAMP_PERSON_SCOPE_ONE_FACE`: a room session, the one known face's keep
/// written to their scope, and the next line's answer, which says it, to the
/// room's log. Their "forget my favorite drink" takes the kept thing and
/// that room line, which cites it; another room line with the same word,
/// citing nothing of theirs, stays. A made-up name.
#[test]
fn a_one_face_forget_about_takes_the_room_line_that_says_their_keep() {
    assert!(switch_forget_about(true));
    const HENRY: &str = "p-927a0000000000aa";
    let fixture = Fixture::new("one-face");
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let _ = client.told_for_session_within(1, None, false, BUDGET);
    remember(
        &client,
        Some(HENRY),
        "Their favorite drink is jasmine cheese.",
    );
    // Line 2 is Henry's keep by the one-face rule, as its lease marks it;
    // line 3, a guest's, another voice heard, is no one's.
    lamp_voice::memory_scope::mark_one_face_line(2, Some(HENRY));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(client.finished_exchange_of(
        2,
        now,
        "What's my favorite stuff?",
        "Jasmine cheese, that's the favorite I have for you, Henry.",
        None
    ));
    assert!(client.finished_exchange_of(
        3,
        now + 1,
        "Do you have jasmine tea?",
        "I can't make tea.",
        None
    ));
    let rows = |person: &str| -> u64 {
        let db = rusqlite::Connection::open_with_flags(
            fixture.root.join("store").join(lamp_memory::days::DATABASE),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row(
            "SELECT count(*) FROM entries WHERE person=?1 AND (heard||' '||said||' '||text) LIKE '%jasmine%'",
            [person],
            |row| row.get(0),
        )
        .unwrap()
    };
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while rows("everyone") < 2 && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rows("everyone"), 2, "both room lines written");
    assert_eq!(rows(HENRY), 1, "his kept thing");
    let (told, ok) = client.run_for(
        &call(FORGET, json!({"about":"favorite drink"})),
        BUDGET,
        Some(HENRY),
    );
    assert!(ok && told["forgotten"] == true, "{told}");
    assert_eq!(rows(HENRY), 0, "his kept thing is gone");
    assert_eq!(rows("everyone"), 1, "only the guest's room line stays");
}

/// One request straight to the store, its answer.
fn ask(socket: &PathBuf, request: Value) -> Value {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
    stream.write_all(format!("{request}\n").as_bytes()).unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

/// El's scene on #5333: a line said and never kept, then "forget what I
/// said about my sister", which matches nothing kept. The night passes
/// over the line all the same; another person's line of the day is read.
#[test]
fn a_topic_forget_that_matched_nothing_keeps_the_night_off_that_days_lines() {
    assert!(switch_forget_about(true));
    let mut day = String::new();
    let fixture = Fixture::new_with("about-night", |store, now| {
        let ana = Person::new(ANA).unwrap();
        let binh = Person::new(BINH).unwrap();
        store
            .append(
                &ana,
                now - 1,
                "synthetic: my sister Lan just moved to Da Nang",
                "How exciting",
                &[],
            )
            .unwrap();
        store
            .append(
                &binh,
                now - 1,
                "synthetic: my brother lives in Hanoi",
                "Nice",
                &[],
            )
            .unwrap();
        day = lamp_memory::store::Day::at(now, &lamp_memory::store::LocalTime::Offset(0))
            .unwrap()
            .to_string();
    });
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let prepare = |person: &str| {
        ask(
            &fixture.socket,
            json!({"op":"nightly_prepare","person":person,"day":day}),
        )
    };
    let records = |answer: Value| answer["input"]["records"].as_array().map_or(0, Vec::len);
    let (told, ok) = client.run_for(&call(FORGET, json!({"about":"sister"})), BUDGET, Some(ANA));
    // The line itself goes too now (lamp-06c1, 11 Nov), and the night
    // still passes over the day.
    assert!(
        ok && told["forgotten"] == true && told["exchanges"] == 1,
        "{told}"
    );
    assert_eq!(prepare(ANA), json!({"ok":true,"reason":"nothing"}));
    assert_eq!(records(prepare(BINH)), 1);
}

/// Friday's step 6 on lamp-06c1 (11 Nov): a calendar event talked about
/// and moved, never kept, then "forget the recheck party". Recall found
/// the talk about it all the same, from the exchanges. Now those of theirs
/// that name it go; their other talk, and another person's about the same
/// party, stay. A made-up party.
#[test]
fn forget_about_takes_that_persons_talk_about_it_from_recall() {
    assert!(switch_forget_about(true));
    let fixture = Fixture::new_with("talk", |store, now| {
        let ana = Person::new(ANA).unwrap();
        let binh = Person::new(BINH).unwrap();
        for (heard, said) in [
            (
                "synthetic: put the Friday Recheck Party on my calendar for Sunday",
                "Done, the recheck party is on Sunday.",
            ),
            (
                "synthetic: move it to Monday at 7 PM",
                "Moved the recheck parties to Monday at 7 PM.",
            ),
            ("synthetic: my coffee is black", "Noted."),
        ] {
            store.append(&ana, now - 1, heard, said, &[]).unwrap();
        }
        store
            .append(
                &binh,
                now - 1,
                "synthetic: the recheck party sounds fun",
                "It does.",
                &[],
            )
            .unwrap();
    });
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let recall = |person: &str, about: &str| -> String {
        let (told, ok) =
            client.run_for(&call(RECALL, json!({"about":about})), BUDGET, Some(person));
        assert!(ok, "{told}");
        told.to_string()
    };
    assert!(recall(ANA, "recheck party").contains("Recheck Party"));

    let (told, ok) = client.run_for(
        &call(FORGET, json!({"about":"the recheck party"})),
        BUDGET,
        Some(ANA),
    );
    assert!(
        ok && told["forgotten"] == true && told["exchanges"] == 2,
        "{told}"
    );
    assert!(
        !told.to_string().to_lowercase().contains("sunday"),
        "{told}"
    );
    let after = recall(ANA, "recheck party");
    assert!(!after.to_lowercase().contains("recheck"), "{after}");
    assert!(recall(ANA, "coffee").contains("coffee is black"));
    assert!(recall(BINH, "recheck party").contains("sounds fun"));
}
