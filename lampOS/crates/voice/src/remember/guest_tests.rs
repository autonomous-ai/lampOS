//! #8639, the memory owner's calls of 27 Nov 2026 (17:42, 17:58): a guest's
//! words go to the room's memory, marked; the owner or an enrolled user can
//! ask what their guest said and is told it as the guest's; a guest's fact
//! is never told as the household's or the asker's. Off, nothing changes.
use super::*;
use crate::remember::{
    DaysClient, RECALL, days_answer, hits_told, opened_note, recall_tool, tools_with,
};
use lamp_gemini::ToolCall;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, BufReader, Write},
    os::unix::net::UnixListener,
    path::PathBuf,
    time::{Duration, Instant},
};

const NOW: u64 = 1_792_000_000_000;

/// W1's words, exactly as the memory owner approved them (27 Nov 17:58).
const W1: &str = " Items with said_by were said by a guest, someone the lamp did not know: tell them as the guest's, never as the asker's or the household's.";

/// The switch for this thread, put back when dropped.
struct Switch;

impl Switch {
    fn set(on: bool) -> Self {
        set_on_for_test(Some(on));
        Self
    }
}

impl Drop for Switch {
    fn drop(&mut self) {
        set_on_for_test(None);
    }
}

fn owner() -> String {
    crate::remember::person_id("A")
}

fn recall(args: Value) -> ToolCall {
    ToolCall {
        id: "c1".into(),
        name: RECALL.into(),
        args,
    }
}

/// A memory answering `requests` lines with `answer`, giving back those it
/// read, within 5 s in all.
fn memory(
    name: &str,
    requests: usize,
    answer: impl Fn(&Value) -> Value + Send + 'static,
) -> (PathBuf, std::thread::JoinHandle<Vec<Value>>) {
    const WITHIN: Duration = Duration::from_secs(5);
    let socket =
        std::env::temp_dir().join(format!("lamp-guest-{name}-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let served = std::thread::spawn(move || {
        let until = Instant::now() + WITHIN;
        let mut read = Vec::new();
        while read.len() < requests && Instant::now() < until {
            let mut stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(
                    until
                        .saturating_duration_since(Instant::now())
                        .max(Duration::from_millis(1)),
                ))
                .unwrap();
            let mut line = String::new();
            if BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .is_err()
            {
                break;
            }
            let request: Value = serde_json::from_str(&line).unwrap();
            stream
                .write_all(format!("{}\n", answer(&request)).as_bytes())
                .unwrap();
            read.push(request);
        }
        read
    });
    (socket, served)
}

/// The real memory store behind a socket, for `requests` lines.
fn real_memory(
    name: &str,
    requests: usize,
) -> (PathBuf, PathBuf, std::thread::JoinHandle<Vec<Value>>) {
    use lamp_memory::{days::Days, days_protocol, store};
    let dir = std::env::temp_dir().join(format!("lamp-guest-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let now = || crate::remember::unix_ms();
    let store = std::sync::Mutex::new(
        Days::open(
            store::Options {
                dir: dir.clone(),
                keep_days: 30,
                local: store::LocalTime::Offset(0),
            },
            now(),
        )
        .unwrap(),
    );
    let (socket, served) = memory(name, requests, move |line| {
        let answer = days_protocol::answer(
            &mut store.lock().unwrap(),
            line.to_string().as_bytes(),
            now(),
        );
        serde_json::from_str(answer.line.trim()).unwrap()
    });
    (socket, dir, served)
}

/// A guest line is sent to the room's memory marked `"speaker":"guest"`; a
/// line admitted otherwise (nobody in view, a known face, a misread), a
/// person's own line and a request never read are not.
#[test]
fn a_guest_line_is_sent_marked_to_the_room_memory() {
    let _on = Switch::set(true);
    let (guest, other, theirs, unread) = (8_639_001, 8_639_002, 8_639_003, 8_639_004);
    mark_line(guest, true);
    mark_line(other, false);
    mark_line(theirs, true);
    let (socket, served) = memory("marked", 4, |_| json!({"ok":true,"id":7}));
    let client = DaysClient::with_idle(socket.clone(), Duration::from_secs(60)).unwrap();
    for (request, person) in [
        (guest, None),
        (other, None),
        (theirs, Some(owner())),
        (unread, None),
    ] {
        assert!(client.finished_exchange_of(
            request,
            NOW,
            "The coffee here is lovely",
            "Glad you like it",
            person.as_deref(),
        ));
    }
    let read = served.join().unwrap();
    assert_eq!(read.len(), 4, "{read:?}");
    let speakers: Vec<Option<&str>> = read
        .iter()
        .map(|line| line.get("speaker").and_then(Value::as_str))
        .collect();
    assert_eq!(speakers, [Some("guest"), None, None, None], "{read:?}");
    drop(client);
    std::fs::remove_file(&socket).unwrap();
}

/// A guest's kept fact (a `remember` on a guest line) is sent marked, so it
/// expires with the exchanges and is told as the guest's; one on a line
/// that is no guest's, or in a person's own scope, is not.
#[test]
fn a_guests_kept_fact_is_sent_marked() {
    let _on = Switch::set(true);
    let (guest, other) = (8_639_031, 8_639_032);
    mark_line(guest, true);
    mark_line(other, false);
    let (socket, served) = memory("kept", 12, |_| json!({"ok":true,"id":7}));
    let client = DaysClient::with_idle(socket.clone(), Duration::from_secs(60)).unwrap();
    let keep = ToolCall {
        id: "c1".into(),
        name: crate::remember::REMEMBER.into(),
        args: json!({"text":"The guest's dog is called Biscuit"}),
    };
    for (request, person) in [(guest, None), (other, None), (guest, Some(owner()))] {
        let _ = client.run_in(&keep, Duration::from_secs(1), person.as_deref(), request);
    }
    let read = served.join().unwrap();
    let speakers: Vec<Option<&str>> = read
        .iter()
        .filter(|line| line["op"] == "remember")
        .map(|line| line.get("speaker").and_then(Value::as_str))
        .collect();
    assert_eq!(speakers, [Some("guest"), None, None], "{read:?}");
    drop(client);
    std::fs::remove_file(&socket).unwrap();
}

/// "What did my guest say?", end to end against the real store: a guest's
/// line and a household line are kept in the room's memory; the owner's
/// recall with `guest` gets the guest's line only, told as the guest's
/// (`said_by`), never marked theirs, with W1's rule.
#[test]
fn the_owner_asking_what_their_guest_said_is_told_the_guests_line_as_the_guests() {
    let _on = Switch::set(true);
    let (guest, household) = (8_639_011, 8_639_012);
    mark_line(guest, true);
    mark_line(household, false);
    let (socket, dir, served) = real_memory("asked", 64);
    let client = DaysClient::with_idle(socket.clone(), Duration::from_secs(60)).unwrap();
    let now = crate::remember::unix_ms();
    assert!(client.finished_exchange_of(
        guest,
        now,
        "The coffee here is lovely, I take mine with honey",
        "Honey is a nice touch",
        None,
    ));
    assert!(client.finished_exchange_of(
        household,
        now + 1,
        "We water the plants on Sunday",
        "Noted",
        None,
    ));
    client.hand_ons.settle(Duration::from_secs(5));
    // The recall function asks for it (W2).
    let tools = super::described(vec![recall_tool(false)]);
    assert!(
        tools[0].parameters.as_ref().unwrap()["properties"]
            .get("guest")
            .is_some(),
        "recall takes guest"
    );
    assert_eq!(
        tools[0].description,
        recall_tool(false).description,
        "the description is as it was: 1,020 of its 1,024 bytes"
    );
    for person in [Some(owner()), None] {
        let (told, answered) = client.run_for(
            &recall(json!({"guest":true})),
            Duration::from_secs(3),
            person.as_deref(),
        );
        assert!(answered, "{told}");
        let found = told["found"].as_array().unwrap_or_else(|| panic!("{told}"));
        assert_eq!(found.len(), 1, "only the guest's: {told}");
        assert_eq!(found[0]["said_by"], "a guest", "{told}");
        assert!(
            found[0]["heard"].as_str().unwrap().contains("honey"),
            "{told}"
        );
        assert!(
            found[0].get("theirs").is_none(),
            "never the asker's: {told}"
        );
        assert!(told["scope"].as_str().unwrap().ends_with(W1), "{told}");
    }
    drop(client);
    let _ = served;
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A guest's fact is never told as the household's or the asker's: a recall
/// item says `said_by` (W1), the session note and the hits say "A guest
/// said: " (W3), and a household fact beside it is as before.
#[test]
fn a_guests_fact_is_never_told_as_the_households_or_the_askers() {
    let _on = Switch::set(true);
    let answer = json!({"ok":true,"entries":[
        {"id":7,"kind":"kept","at_unix_ms":NOW,"text":"The coffee here is made with oat milk and honey"},
        {"id":8,"kind":"kept","at_unix_ms":NOW,"text":"The guest's dog is called Biscuit","said_by":"guest"}
    ]});
    let (told, ids) = days_answer(&answer, NOW, None, None).unwrap();
    assert_eq!(ids, [7, 8]);
    assert!(told["found"][0].get("said_by").is_none(), "{told}");
    assert_eq!(told["found"][1]["said_by"], "a guest", "{told}");
    assert_eq!(
        told["scope"],
        format!("said in this room; speaker unknown{W1}")
    );
    // The session note: the room's snapshot names the guest's fact by id.
    let snapshot = json!({"session":"s","opened_unix_ms":NOW,"generation":1,"invalidated":false,
        "source_ids":[7,8],"guest_ids":[8],
        "note":json!({"scope":"said in this room; speaker unknown","untrusted":true,"facts":[
            {"id":7,"kind":"preference","text":"The coffee here is made with oat milk and honey"},
            {"id":8,"kind":"preference","text":"The guest's dog is called Biscuit"}]}).to_string()});
    for room_not_asker in [true, false] {
        let (note, _) = opened_note(room_not_asker, &snapshot, None);
        assert!(
            note.contains("A guest said: The guest's dog is called Biscuit"),
            "{note}"
        );
        assert!(!note.contains("A guest said: The coffee"), "{note}");
    }
    // The hits.
    let heard = "what is the dog called and how is the coffee made";
    let (hits, _, _) = hits_told(&answer, heard, NOW, None, false).unwrap();
    assert!(
        hits.contains("- A guest said: The guest's dog is called Biscuit"),
        "{hits}"
    );
    assert!(
        hits.contains("- The coffee here is made with oat milk and honey"),
        "{hits}"
    );
}

/// With the switch off nothing changes: no line is marked, no recall item
/// says `said_by`, the note and hits carry no "A guest said: ", the scope
/// is as before, and recall's function has no `guest`.
#[test]
fn with_the_switch_off_nothing_changes() {
    let _off = Switch::set(false);
    let request = 8_639_021;
    mark_line(request, true);
    assert!(!line(request));
    let (socket, served) = memory("off", 1, |_| json!({"ok":true,"id":7}));
    let client = DaysClient::with_idle(socket.clone(), Duration::from_secs(60)).unwrap();
    assert!(client.finished_exchange_of(request, NOW, "Hello there", "Hi", None));
    let read = served.join().unwrap();
    assert!(read[0].get("speaker").is_none(), "{read:?}");
    drop(client);
    std::fs::remove_file(&socket).unwrap();
    let answer = json!({"ok":true,"entries":[
        {"id":8,"kind":"kept","at_unix_ms":NOW,"text":"The guest's dog is called Biscuit","said_by":"guest"}
    ]});
    let (told, _) = days_answer(&answer, NOW, None, None).unwrap();
    let item = told["found"][0].as_object().unwrap();
    let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["asked_to_remember", "source_id", "when"], "{told}");
    assert_eq!(told["scope"], "said in this room; speaker unknown");
    let (hits, _, _) = hits_told(&answer, "the dog", NOW, None, false).unwrap();
    assert!(!hits.contains("A guest said"), "{hits}");
    for second in [false, true] {
        let tool = recall_tool(second);
        assert!(!tool.description.contains("guest"), "{}", tool.description);
        assert!(
            tool.parameters.as_ref().unwrap()["properties"]
                .get("guest")
                .is_none()
        );
    }
    for tool in tools_with(false) {
        assert!(!tool.description.contains("give guest true"));
    }
}

/// The recall function stays valid, within 1,024 bytes, with `guest` under
/// every other switch it can take.
#[test]
fn recall_with_guest_stays_a_valid_function() {
    let _on = Switch::set(true);
    for second in [false, true] {
        for replaces in [false, true] {
            let mut tools = super::described(vec![recall_tool(second)]);
            if replaces {
                tools = crate::remember::replaces_described(tools);
            }
            let recall = &tools[0];
            assert!(
                recall.validate().is_ok(),
                "second {second} replaces {replaces}: {} bytes",
                recall.description.len()
            );
            assert_eq!(
                recall.parameters.as_ref().unwrap()["properties"]["guest"]["description"],
                "true for only what a guest, someone the lamp did not know, said here."
            );
        }
    }
}
