//! #8639, the memory owner's calls of 27 Nov 2026 (17:42 and 17:58): a
//! guest's words go to the room's memory, marked as a guest's, so the owner
//! or an enrolled user can ask what their guest said; a guest's kept facts
//! expire after 30 days, as their exchanges do. The mark is a `guest` row in
//! the existing `speakers` table (the voice's `LAMP_MEMORY_GUEST_MARKED`
//! sends `"speaker":"guest"` on a guest's room line): no new column, table
//! or migration. Through the memory's own protocol on a scratch store; all
//! content is synthetic.
use lamp_memory::{Options, days::Days, days_protocol};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
};

const NOW: u64 = 1_791_892_800_000;
const DAY: u64 = 86_400_000;
const OWNER: &str = "p-0123456789abcdef";

struct Scratch {
    root: PathBuf,
    store: Days,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("lamp-guest-room-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let store = Days::open(
            Options {
                dir: root.clone(),
                keep_days: 30,
                local: lamp_memory::store::LocalTime::Offset(0),
            },
            NOW,
        )
        .unwrap();
        Self { root, store }
    }

    fn ask_at(&mut self, request: &Value, now: u64) -> Value {
        let line = serde_json::to_vec(request).unwrap();
        serde_json::from_str(&days_protocol::answer(&mut self.store, &line, now).line).unwrap()
    }

    fn ask(&mut self, request: &Value) -> Value {
        self.ask_at(request, NOW + 60_000)
    }

    /// A room exchange, a guest's when `guest`.
    fn said(&mut self, heard: &str, guest: bool) -> u64 {
        let mut line = json!({"op":"append_linked","at_unix_ms":NOW,"heard":heard,"said":"Nice.","source_ids":[]});
        if guest {
            line["speaker"] = json!("guest");
        }
        let answer = self.ask(&line);
        assert_eq!(answer["ok"], true, "{answer}");
        answer["id"].as_u64().unwrap()
    }

    /// A room kept fact, a guest's when `guest`.
    fn kept(&mut self, text: &str, guest: bool) -> u64 {
        let mut line = json!({"op":"remember","at_unix_ms":NOW,"text":text});
        if guest {
            line["speaker"] = json!("guest");
        }
        let answer = self.ask(&line);
        assert_eq!(answer["ok"], true, "{answer}");
        answer["id"].as_u64().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Each entry's id and its `said_by`, as an answer gives them.
fn said_by(answer: &Value) -> Vec<(u64, Option<String>)> {
    answer["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("{answer}"))
        .iter()
        .map(|entry| {
            (
                entry["id"].as_u64().unwrap(),
                entry["said_by"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

fn guest() -> Option<String> {
    Some("guest".to_owned())
}

/// A guest's room line is kept in the room's memory with its mark, and
/// every read of the room's tells it: a recall, a search and the kept list.
/// A household line has no mark. The mark is the room's alone: a guest
/// speaker on a person's own scope is refused, and `forget_said` never
/// takes it (it is no person's id).
#[test]
fn a_guest_line_is_marked_and_kept_in_the_room_memory() {
    let mut scratch = Scratch::new("marked");
    let household = scratch.said("We water the plants on Sunday", false);
    let theirs = scratch.said("The coffee here is lovely, I take mine with honey", true);
    let fact = scratch.kept("The guest's dog is called Biscuit", true);
    let recalled = scratch.ask(&json!({"op":"recall","recall_v2":true,"limit":6,"max_bytes":2400}));
    assert_eq!(recalled["ok"], true, "{recalled}");
    let marks = said_by(&recalled);
    assert!(marks.contains(&(household, None)), "{recalled}");
    assert!(marks.contains(&(theirs, guest())), "{recalled}");
    let found =
        scratch.ask(&json!({"op":"search","query":"coffee honey","limit":6,"max_bytes":2400}));
    assert_eq!(said_by(&found), [(theirs, guest())], "{found}");
    let listed = scratch.ask(&json!({"op":"kept","max_bytes":4096}));
    assert_eq!(said_by(&listed), [(fact, guest())], "{listed}");
    // The room's only.
    let refused = scratch.ask(&json!({"op":"append_linked","person":OWNER,"at_unix_ms":NOW,"heard":"hi","said":"hello","source_ids":[],"speaker":"guest"}));
    assert_eq!(refused["error"], "bad_request", "{refused}");
    let refused = scratch.ask(&json!({"op":"forget_said","speaker":"guest"}));
    assert_eq!(refused["error"], "bad_request", "{refused}");
    let again =
        scratch.ask(&json!({"op":"search","query":"coffee honey","limit":6,"max_bytes":2400}));
    assert_eq!(said_by(&again), [(theirs, guest())], "still kept: {again}");
}

/// "What did my guest say?": a recall with `guest` gives only the room's
/// guest lines and facts, each marked, within the same 6 entries; the
/// household's lines said later never crowd them out. A known person's own
/// scope is never filtered: theirs as before.
#[test]
fn a_guest_recall_gives_only_the_guests_lines() {
    let mut scratch = Scratch::new("filter");
    let theirs = scratch.said("The coffee here is lovely, I take mine with honey", true);
    let fact = scratch.kept("The guest is flying to Tokyo on Friday", true);
    for n in 0..8 {
        scratch.said(
            &format!("Household line number {n} about the plants"),
            false,
        );
    }
    let asked = scratch
        .ask(&json!({"op":"recall","recall_v2":true,"guest":true,"limit":6,"max_bytes":2400}));
    assert_eq!(asked["ok"], true, "{asked}");
    let mut marks = said_by(&asked);
    marks.sort();
    let mut expected = vec![(theirs, guest()), (fact, guest())];
    expected.sort();
    assert_eq!(marks, expected, "{asked}");
    // With words too.
    let asked = scratch.ask(
        &json!({"op":"search","query":"coffee plants","guest":true,"limit":6,"max_bytes":2400}),
    );
    assert_eq!(said_by(&asked), [(theirs, guest())], "{asked}");
    // Theirs unfiltered: the owner's own lines, as before.
    let own = scratch.ask(&json!({"op":"append_linked","person":OWNER,"at_unix_ms":NOW,"heard":"My own plants","said":"Noted","source_ids":[]}));
    let own = own["id"].as_u64().unwrap();
    let asked = scratch.ask(&json!({"op":"recall","person":OWNER,"recall_v2":true,"guest":true,"limit":6,"max_bytes":2400}));
    assert_eq!(said_by(&asked), [(own, None)], "{asked}");
}

/// A guest's kept fact expires at 30 days, as their exchanges do (the
/// memory owner's call, 27 Nov 17:58); the household's kept fact never
/// expires, as before.
#[test]
fn a_guests_kept_fact_expires_at_thirty_days() {
    let mut scratch = Scratch::new("expiry");
    let household = scratch.kept("The coffee here is made with oat milk and honey", false);
    let fact = scratch.kept("The guest's dog is called Biscuit", true);
    let line = scratch.said("I take my coffee black", true);
    let listed = |scratch: &mut Scratch, now: u64| -> Vec<u64> {
        let answer = scratch.ask_at(&json!({"op":"kept","max_bytes":4096}), now);
        said_by(&answer).into_iter().map(|(id, _)| id).collect()
    };
    let recalled = |scratch: &mut Scratch, now: u64| -> Vec<u64> {
        let answer = scratch.ask_at(
            &json!({"op":"recall","recall_v2":true,"limit":6,"max_bytes":2400}),
            now,
        );
        said_by(&answer).into_iter().map(|(id, _)| id).collect()
    };
    assert_eq!(listed(&mut scratch, NOW + 29 * DAY), [household, fact]);
    assert_eq!(recalled(&mut scratch, NOW + 29 * DAY), [line]);
    assert_eq!(
        listed(&mut scratch, NOW + 31 * DAY),
        [household],
        "the guest's fact goes with its exchanges"
    );
    assert!(recalled(&mut scratch, NOW + 31 * DAY).is_empty());
}

/// The session note's room snapshot names its guest facts by id
/// (`guest_ids`), so the voice tells each as "A guest said: …", never as
/// the household's. A note with none says nothing new.
#[test]
fn the_room_note_names_the_guests_facts() {
    let mut scratch = Scratch::new("note");
    let household = scratch.kept("The coffee here is made with oat milk and honey", false);
    let opened = scratch.ask(&json!({"op":"session_open","session":"s1","max_bytes":1200}));
    assert_eq!(opened["ok"], true, "{opened}");
    assert!(opened["snapshot"].get("guest_ids").is_none(), "{opened}");
    let fact = scratch.kept("The guest's dog is called Biscuit", true);
    let opened = scratch.ask(&json!({"op":"session_open","session":"s2","max_bytes":1200}));
    assert_eq!(opened["snapshot"]["guest_ids"], json!([fact]), "{opened}");
    assert!(
        opened["snapshot"]["source_ids"]
            .as_array()
            .unwrap()
            .contains(&json!(household))
    );
    // A person's own note never has them.
    let opened =
        scratch.ask(&json!({"op":"session_open","person":OWNER,"session":"s3","max_bytes":1200}));
    assert!(opened["snapshot"].get("guest_ids").is_none(), "{opened}");
}

/// PIN, green before and after: with no guest mark (the switch off in the
/// voice, which then never sends one), no answer says anything new.
#[test]
fn with_no_guest_mark_nothing_changes() {
    let mut scratch = Scratch::new("unmarked");
    scratch.said("The coffee here is lovely, I take mine with honey", false);
    scratch.kept("The coffee here is made with oat milk and honey", false);
    for request in [
        json!({"op":"recall","recall_v2":true,"limit":6,"max_bytes":2400}),
        json!({"op":"recall","limit":6,"max_bytes":2400}),
        json!({"op":"search","query":"coffee","limit":6,"max_bytes":2400}),
        json!({"op":"kept","max_bytes":4096}),
        json!({"op":"session_open","session":"s1","max_bytes":1200}),
    ] {
        let answer = scratch.ask(&request);
        assert_eq!(answer["ok"], true, "{answer}");
        let text = answer.to_string();
        assert!(
            !text.contains("said_by") && !text.contains("guest_ids"),
            "{request}: {answer}"
        );
    }
    // The household's kept fact never expires.
    let answer = scratch.ask_at(&json!({"op":"kept","max_bytes":4096}), NOW + 90 * DAY);
    assert_eq!(said_by(&answer).len(), 1, "{answer}");
}
