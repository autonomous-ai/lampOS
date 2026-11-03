//! `strength` on replies (the memory owner's Tiers-ideas plan, 3 Nov, P2;
//! adapted from memory-tiers): with the request's `strength`, a search's,
//! a recall's and a session note's lines that are a plan or a maybe say so;
//! without it, every answer is byte for byte what it was.
use lamp_memory::{Options, days::Days, days_protocol, store::LocalTime};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const NOW: u64 = 1_791_892_800_000;
const PERSON: &str = "p-0123456789abcdef";
const DECOY: &str = "Maybe I will learn pottery; I have not decided or booked a class.";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lamp-strength-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }

    fn open(&self) -> Days {
        Days::open(
            Options {
                dir: self.0.clone(),
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            NOW,
        )
        .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn line(store: &mut Days, request: &Value) -> String {
    days_protocol::answer(store, &serde_json::to_vec(request).unwrap(), NOW).line
}

fn send(store: &mut Days, request: Value) -> Value {
    let answer: Value = serde_json::from_str(&line(store, &request)).unwrap();
    assert_eq!(answer["ok"], true, "{request} {answer}");
    answer
}

/// A maybe heard, a plan kept and a pottery line as said; their ids.
fn pottery(store: &mut Days) -> (u64, u64, u64) {
    let maybe = send(
        store,
        json!({"op":"append","person":PERSON,"at_unix_ms":NOW-3_000,"heard":DECOY,"said":"That sounds fun."}),
    );
    let plan = send(
        store,
        json!({"op":"remember","person":PERSON,"at_unix_ms":NOW-2_000,"text":"Will visit the pottery fair on Sunday"}),
    );
    let said = send(
        store,
        json!({"op":"remember","person":PERSON,"at_unix_ms":NOW-1_000,"text":"Learned pottery at school"}),
    );
    let id = |answer: &Value| answer["id"].as_u64().unwrap();
    (id(&maybe), id(&plan), id(&said))
}

fn strength_of(answer: &Value, list: &str, id: u64) -> Option<String> {
    let entry = answer[list]
        .as_array()
        .unwrap_or_else(|| panic!("{answer}"))
        .iter()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("{id} not in {answer}"));
    entry
        .get("strength")
        .map(|label| label.as_str().unwrap().to_owned())
}

fn note(answer: &Value) -> Value {
    serde_json::from_str(answer["snapshot"]["note"].as_str().unwrap()).unwrap()
}

#[test]
fn maybe_line_carries_strength() {
    let scratch = Scratch::new("maybe");
    let mut store = scratch.open();
    let (maybe, plan, said) = pottery(&mut store);
    for request in [
        json!({"op":"search","person":PERSON,"query":"pottery","strength":true}),
        json!({"op":"recall","person":PERSON,"query":"pottery","strength":true}),
        json!({"op":"recall","person":PERSON,"query":"pottery","recall_v2":true,"strength":true}),
    ] {
        let found = send(&mut store, request.clone());
        assert_eq!(
            found["entries"].as_array().unwrap().len(),
            3,
            "{request} {found}"
        );
        assert_eq!(
            strength_of(&found, "entries", maybe).as_deref(),
            Some("maybe"),
            "{request}"
        );
        assert_eq!(
            strength_of(&found, "entries", plan).as_deref(),
            Some("plan"),
            "{request}"
        );
        assert_eq!(strength_of(&found, "entries", said), None, "{request}");
    }
    let opened = send(
        &mut store,
        json!({"op":"session_open","person":PERSON,"session":"s-1","strength":true}),
    );
    let quoted = note(&opened);
    assert_eq!(
        strength_of(&quoted, "facts", plan).as_deref(),
        Some("plan"),
        "{quoted}"
    );
    assert_eq!(strength_of(&quoted, "facts", said), None, "{quoted}");
}

/// An exchange is read by what was heard: the lamp's own "maybe" never
/// labels the person's line.
#[test]
fn the_lamps_words_never_label_an_exchange() {
    let scratch = Scratch::new("lamp-words");
    let mut store = scratch.open();
    let id = send(
        &mut store,
        json!({"op":"append","person":PERSON,"at_unix_ms":NOW,"heard":"My tea is jasmine","said":"Maybe you will like peppermint too."}),
    )["id"]
        .as_u64()
        .unwrap();
    let found = send(
        &mut store,
        json!({"op":"search","person":PERSON,"query":"jasmine tea","strength":true}),
    );
    assert_eq!(strength_of(&found, "entries", id), None, "{found}");
}

/// Off, nothing changes: no `strength` in any answer, a recall that names
/// `strength:false` (now the new operation) answers byte for byte as the
/// legacy one, and so do a search and a note's reopening.
#[test]
fn off_is_byte_identical() {
    let scratch = Scratch::new("off");
    let mut store = scratch.open();
    pottery(&mut store);
    for (bare, off) in [
        (
            json!({"op":"search","person":PERSON,"query":"pottery"}),
            json!({"op":"search","person":PERSON,"query":"pottery","strength":false}),
        ),
        (
            json!({"op":"recall","person":PERSON,"query":"pottery"}),
            json!({"op":"recall","person":PERSON,"query":"pottery","strength":false}),
        ),
        (
            json!({"op":"recall","person":PERSON,"query":"pottery","recall_v2":true}),
            json!({"op":"recall","person":PERSON,"query":"pottery","recall_v2":true,"strength":false}),
        ),
        (
            json!({"op":"session_open","person":PERSON,"session":"s-1"}),
            json!({"op":"session_open","person":PERSON,"session":"s-1","strength":false}),
        ),
    ] {
        let (bare_line, off_line) = (line(&mut store, &bare), line(&mut store, &off));
        assert_eq!(bare_line, off_line, "{bare}");
        assert!(!bare_line.contains("strength"), "{bare_line}");
        assert!(bare_line.contains("\"ok\":true"), "{bare_line}");
    }
}

/// On, the frozen note is the same note, labelled, within the same cap: a
/// labelled note that would pass it leaves out its last facts and their
/// ids, never a fact without its label.
#[test]
fn a_labelled_note_stays_within_its_cap() {
    let scratch = Scratch::new("cap");
    let mut store = scratch.open();
    for n in 0..40u64 {
        send(
            &mut store,
            json!({"op":"remember","person":PERSON,"at_unix_ms":NOW-40_000+n*1_000,"text":format!("Maybe I will try class {n}")}),
        );
    }
    let off = send(
        &mut store,
        json!({"op":"session_open","person":PERSON,"session":"s-1"}),
    );
    let on = send(
        &mut store,
        json!({"op":"session_open","person":PERSON,"session":"s-1","strength":true}),
    );
    let (told, labelled) = (note(&off), note(&on));
    let text = on["snapshot"]["note"].as_str().unwrap();
    assert!(text.len() <= 1_200, "{} bytes", text.len());
    let facts = labelled["facts"].as_array().unwrap();
    assert!(!facts.is_empty());
    assert!(facts.len() < told["facts"].as_array().unwrap().len());
    assert!(
        facts.iter().all(|fact| fact["strength"] == "maybe"),
        "{labelled}"
    );
    let ids: Vec<&Value> = facts.iter().map(|fact| &fact["id"]).collect();
    let source_ids: Vec<&Value> = on["snapshot"]["source_ids"]
        .as_array()
        .unwrap()
        .iter()
        .collect();
    assert_eq!(ids, source_ids);
    // Every fact it gives is the frozen note's, in its order.
    let frozen: Vec<&Value> = told["facts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| &f["id"])
        .collect();
    assert_eq!(&frozen[..ids.len()], &ids[..]);
    // The same at every open.
    let again = send(
        &mut store,
        json!({"op":"session_open","person":PERSON,"session":"s-1","strength":true}),
    );
    assert_eq!(again, on);
}
