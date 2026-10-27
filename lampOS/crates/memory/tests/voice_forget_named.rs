//! #3113: a `forget` that names a thing, through Voice's production Days
//! client and its guard (`named_forget_with`) against the real socket and
//! store, on the product's switches: `LAMP_MEMORY_FORGET_ABOUT` and
//! `LAMP_MEMORY_FORGET_ALL` off in this test process. All content is
//! synthetic; no provider, credentials, hardware or network are involved.
use lamp_gemini::ToolCall;
use lamp_memory::{
    Options, Person, Server,
    days::Days,
    days_protocol,
    protocol::Answer,
    server::{Memory, Stopper},
    tidy::{Category, Output, Proposal},
};
use lamp_voice::remember::{
    Carry, DaysClient, FORGET, REMEMBER, forget_about_on, forget_all_on, named_forget_with,
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
const MAI: &str = "p-0000000000000a0a";
const AN: &str = "p-0000000000000b0b";

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
    /// The real store with, before the socket opens, `noticed`: each a
    /// scope and an observation kept of it by the observe pass's own
    /// `tidy_apply`, from one exchange of synthetic words.
    fn new(name: &str, noticed: &[(Option<&str>, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!("lamp-mfn-{}-{name}", std::process::id()));
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
        for (index, (person, text)) in noticed.iter().enumerate() {
            let person = Person::new(person.unwrap_or(Person::EVERYONE)).unwrap();
            let heard = store
                .append(
                    &person,
                    now,
                    "synthetic: a line",
                    "synthetic: an answer",
                    &[],
                )
                .unwrap();
            let input = store
                .conversation_end(&person, &format!("c{index}"), &[heard])
                .unwrap();
            let output = Output {
                observations: vec![Proposal {
                    text: (*text).into(),
                    category: Category::Preference,
                    source_ids: vec![heard],
                    supersedes: None,
                    replaces: None,
                    on: None,
                }],
            };
            store.tidy_apply(&person, &input.job, &output).unwrap();
        }
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

/// A keep as the voice runs it: through the guard, which a keep resets.
fn remember(client: &DaysClient, person: Option<&str>, text: &str, carry: &Carry) {
    let keep = call(REMEMBER, json!({"text":text}));
    assert!(named_forget_with(&keep, None, person, carry).is_none());
    let (told, ok) = client.run_for(&keep, BUDGET, person);
    assert!(ok && told["remembered"] == true, "{told}");
}

/// What `person`'s scope keeps and was noticed of, as texts, from the
/// store's own listing.
fn listed(socket: &std::path::Path, person: Option<&str>) -> Vec<String> {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
    let mut line = json!({"op":"kept","observations":true,"max_bytes":16384});
    if let Some(person) = person {
        line["person"] = json!(person);
    }
    writeln!(stream, "{line}").unwrap();
    let mut answer = String::new();
    BufReader::new(stream).read_line(&mut answer).unwrap();
    let answer: Value = serde_json::from_str(&answer).unwrap();
    let mut texts: Vec<String> = answer["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["text"].as_str().unwrap().to_owned())
        .collect();
    texts.sort();
    texts
}

/// The person's turn as the voice runs it: the guard on the model's call
/// with what was heard, then the client in their scope.
fn forget(client: &DaysClient, said: &str, person: Option<&str>, carry: &Carry) -> Value {
    let model = call(FORGET, json!({}));
    let named = named_forget_with(&model, Some(said), person, carry);
    let (told, ok) = client.run_for(named.as_ref().unwrap_or(&model), BUDGET, person);
    assert!(ok, "{told}");
    told
}

/// Twelve's run on c0a4: the colour noticed, then the wedding kept, then
/// "forget my favourite colour" removes the colour and keeps the wedding;
/// An's colour and the room's stay. Then "forget that" right after a keep
/// removes that keep. In English and in Vietnamese.
#[test]
fn a_forget_that_names_a_thing_removes_only_it_and_never_the_newest_kept_line() {
    assert!(!forget_about_on() && !forget_all_on());
    for (name, colour, wedding, said, after, gone) in [
        (
            "en",
            "synthetic: favourite colour is violet",
            "synthetic: sister's wedding is on Saturday",
            "Lamp, please forget my favourite colour.",
            "synthetic: keys are in the blue bowl",
            "Forget that.",
        ),
        (
            "vi",
            "synthetic: màu yêu thích là màu tím",
            "synthetic: đám cưới chị gái vào thứ bảy",
            "quên màu yêu thích của tôi",
            "synthetic: chìa khoá ở trong bát xanh",
            "quên cái đó đi",
        ),
    ] {
        let fixture = Fixture::new(
            name,
            &[(Some(MAI), colour), (Some(AN), colour), (None, colour)],
        );
        let client = DaysClient::new(fixture.socket.clone()).unwrap();
        let carry = Carry::default();
        remember(&client, Some(MAI), wedding, &carry);
        let mut both = [colour, wedding];
        both.sort();
        assert_eq!(listed(&fixture.socket, Some(MAI)), both, "{name}");

        let told = forget(&client, said, Some(MAI), &carry);
        assert!(
            told["forgotten"] == true && told["count"] == 1,
            "{name}: {told}"
        );
        assert_eq!(listed(&fixture.socket, Some(MAI)), [wedding], "{name}");
        assert_eq!(listed(&fixture.socket, Some(AN)), [colour], "{name}");
        assert_eq!(listed(&fixture.socket, None), [colour], "{name}");

        // Asked again: nothing about it is left, so nothing goes, and the
        // wedding, the newest kept line, stays.
        let told = forget(&client, said, Some(MAI), &carry);
        assert_eq!(told["forgotten"], false, "{name}: {told}");
        assert!(
            told["note"]
                .as_str()
                .unwrap()
                .contains("I couldn't find anything about"),
            "{name}: {told}"
        );
        assert_eq!(listed(&fixture.socket, Some(MAI)), [wedding], "{name}");

        remember(&client, Some(MAI), after, &carry);
        let told = forget(&client, gone, Some(MAI), &carry);
        assert_eq!(told["forgotten"], true, "{name}: {told}");
        assert_eq!(listed(&fixture.socket, Some(MAI)), [wedding], "{name}");
    }
}

/// With nobody known, a named forget is the room's alone, of what this
/// conversation had: a person's matching thing is never reached, and the
/// room's colour noticed in an earlier conversation stays.
#[test]
fn a_named_forget_in_the_room_never_reaches_a_persons_or_an_earlier_conversations() {
    let fixture = Fixture::new(
        "room",
        &[
            (Some(MAI), "synthetic: favourite colour is violet"),
            (None, "synthetic: favourite colour is green"),
        ],
    );
    let client = DaysClient::new(fixture.socket.clone()).unwrap();
    let told = forget(
        &client,
        "forget the favourite colour",
        None,
        &Carry::default(),
    );
    assert_eq!(told["forgotten"], false, "{told}");
    assert_eq!(
        listed(&fixture.socket, None),
        ["synthetic: favourite colour is green"]
    );
    assert_eq!(
        listed(&fixture.socket, Some(MAI)),
        ["synthetic: favourite colour is violet"]
    );
}
