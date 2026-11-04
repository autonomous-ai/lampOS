//! The Core-verifier's GATE 23 FAIL on lamp-52e6 (stable 9980, 4 Nov): with
//! `LAMP_PERSON_SCOPE_ONE_FACE`, a line from the one known face in view, not
//! seen speaking, is told and read as the room's, its keep theirs. Henry's
//! "my favorite drink is jasmine cheese" was kept as his; the next line's
//! answer, "Jasmine cheese — that's the favorite I have for you, Henry.", went
//! to the room's log. His "forget my favorite drink" took the kept thing, and
//! the room's line still held "jasmine": nothing linked it to what was kept,
//! since a line's sources were its own scope's only.
//!
//! Now the room's line cites the keeper's kept things it used (`keeper`,
//! `keeper_ids` on `append_linked`), and their forget reaches it: by id (what
//! `forget {about}` sends for each match), and by forget everything. Another
//! room line with the same word, citing nothing of theirs, stays. Through
//! the memory's own protocol on a scratch store; all content is synthetic.
use lamp_memory::{Options, days::Days, days_protocol};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
};

const NOW: u64 = 1_791_892_800_000;
const HENRY: &str = "p-927a0123456789ab";

struct Scratch {
    root: PathBuf,
    store: Days,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("lamp-forget-room-{}-{name}", std::process::id()));
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

    fn ask(&mut self, request: Value) -> Value {
        let line = serde_json::to_vec(&request).unwrap();
        serde_json::from_str(&days_protocol::answer(&mut self.store, &line, NOW + 60_000).line)
            .unwrap()
    }

    /// The scope's records holding `word`, as a search finds them.
    fn holding(&mut self, person: Option<&str>, word: &str) -> Vec<String> {
        let mut request = json!({"op":"search","query":word,"limit":20,"max_bytes":4000});
        if let Some(person) = person {
            request["person"] = json!(person);
        }
        let answer = self.ask(request);
        assert_eq!(answer["ok"], true, "{answer}");
        answer["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                format!(
                    "{} {} {}",
                    entry["heard"].as_str().unwrap_or(""),
                    entry["said"].as_str().unwrap_or(""),
                    entry["text"].as_str().unwrap_or("")
                )
                .trim()
                .to_owned()
            })
            .collect()
    }

    /// GATE 23's rows: Henry's keep as his own, the room's answer citing
    /// it, and a guest's room line with the same word citing nothing of
    /// his. The kept thing's id.
    fn henry_keeps_jasmine(&mut self) -> u64 {
        let kept = self.ask(json!({"op":"remember","person":HENRY,"at_unix_ms":NOW,
            "text":"Their favorite drink is jasmine cheese."}));
        assert_eq!(kept["ok"], true, "{kept}");
        let kept = kept["id"].as_u64().unwrap();
        let answer = self.ask(json!({"op":"append_linked","at_unix_ms":NOW + 1_000,
            "heard":"What's my favorite stuff?",
            "said":"Jasmine cheese — that's the favorite I have for you, Henry.",
            "source_ids":[],"keeper":HENRY,"keeper_ids":[kept]}));
        assert_eq!(answer["ok"], true, "{answer}");
        let guest = self.ask(json!({"op":"append_linked","at_unix_ms":NOW + 2_000,
            "heard":"Do you have jasmine tea?","said":"I can't make tea.","source_ids":[]}));
        assert_eq!(guest["ok"], true, "{guest}");
        assert_eq!(self.holding(None, "jasmine").len(), 2, "both room lines");
        assert_eq!(
            self.holding(Some(HENRY), "jasmine").len(),
            1,
            "his kept thing"
        );
        kept
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Henry's forget of the kept thing, by id (what his `forget {about}` sends
/// for it), selective or not: the kept thing and the room's line that cites
/// it go; the guest's room line stays.
#[test]
fn a_forget_of_a_one_face_keep_takes_the_room_line_that_cites_it() {
    for selective in [true, false] {
        let mut scratch = Scratch::new(&format!("by-id-{selective}"));
        let kept = scratch.henry_keeps_jasmine();
        let forgot =
            scratch.ask(json!({"op":"forget","person":HENRY,"id":kept,"selective":selective}));
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert_eq!(forgot["removed"], 2, "the kept thing and the room line");
        assert!(scratch.holding(Some(HENRY), "jasmine").is_empty());
        assert_eq!(
            scratch.holding(None, "jasmine"),
            vec!["Do you have jasmine tea? I can't make tea.".to_owned()],
            "only the guest's line, which cites nothing of his"
        );
    }
}

/// Henry's forget everything takes the room's line citing his keep too,
/// and no other room line; so does the app's removal of him.
#[test]
fn a_forget_everything_of_the_keeper_takes_the_room_line_that_cites_them() {
    for op in ["forget_all", "forget_person"] {
        let mut scratch = Scratch::new(op);
        scratch.henry_keeps_jasmine();
        let forgot = scratch.ask(json!({"op":op,"person":HENRY}));
        assert_eq!(forgot["ok"], true, "{forgot}");
        assert!(scratch.holding(Some(HENRY), "jasmine").is_empty(), "{op}");
        assert_eq!(
            scratch.holding(None, "jasmine").len(),
            1,
            "{op}: the guest's"
        );
    }
}

/// Thirteen's P2 on #5291: a room observation the observe pass made from
/// a line citing his keep (a preference with his kept words, sourced from
/// it) goes with his forget everything, as his selective forget of the keep
/// takes it; one made from the same line with none of his kept words stays.
#[test]
fn a_forget_everything_of_the_keeper_takes_a_room_observation_made_from_the_citing_line() {
    let mut scratch = Scratch::new("observed");
    let kept = scratch.henry_keeps_jasmine();
    let cited = scratch.ask(json!({"op":"append_linked","at_unix_ms":NOW + 3_000,
        "heard":"Jasmine cheese goes best with crackers","said":"Sounds tasty.",
        "source_ids":[],"keeper":HENRY,"keeper_ids":[kept]}));
    assert_eq!(cited["ok"], true, "{cited}");
    let cited = cited["id"].as_u64().unwrap();
    let ended =
        scratch.ask(json!({"op":"conversation_end","conversation":"c5291","source_ids":[cited]}));
    assert_eq!(ended["ok"], true, "{ended}");
    let applied = scratch.ask(json!({"op":"tidy_apply","job":ended["input"]["job"],
        "observations":[
            {"text":"Likes jasmine cheese with crackers.","category":"preference",
                "source_ids":[cited]},
            {"text":"Crackers go best in the room.","category":"preference",
                "source_ids":[cited]}]}));
    assert_eq!(applied["ok"], true, "{applied}");
    assert_eq!(
        applied["ids"].as_array().map(Vec::len),
        Some(2),
        "{applied}"
    );
    assert_eq!(
        scratch.holding(None, "likes"),
        vec!["Likes jasmine cheese with crackers.".to_owned()],
        "the room's observation, before"
    );
    let forgot = scratch.ask(json!({"op":"forget_all","person":HENRY}));
    assert_eq!(forgot["ok"], true, "{forgot}");
    assert!(
        scratch.holding(None, "likes").is_empty(),
        "the room's observation with his words"
    );
    assert_eq!(
        scratch.holding(None, "jasmine"),
        vec!["Do you have jasmine tea? I can't make tea.".to_owned()],
        "only the guest's line"
    );
    assert_eq!(
        scratch.holding(None, "room"),
        vec!["Crackers go best in the room.".to_owned()],
        "the room's observation without his words stays"
    );
}

/// A room line citing his keep, with none of its words, stays at a forget
/// of one kept thing by the selective rule, as his own would.
#[test]
fn a_room_line_citing_the_keep_without_its_words_stays_at_a_selective_forget() {
    let mut scratch = Scratch::new("no-words");
    let kept = scratch.henry_keeps_jasmine();
    let other = scratch.ask(json!({"op":"append_linked","at_unix_ms":NOW + 3_000,
        "heard":"Thanks","said":"Any time.","source_ids":[],"keeper":HENRY,"keeper_ids":[kept]}));
    assert_eq!(other["ok"], true, "{other}");
    scratch.ask(json!({"op":"forget","person":HENRY,"id":kept,"selective":true}));
    assert_eq!(scratch.holding(None, "thanks").len(), 1);
}

/// A keeper is only on a room line, only a known person's id, and their
/// ids are linked only when they are their kept things; nothing of theirs
/// is told in the room's record.
#[test]
fn a_keeper_is_checked_and_never_told_in_the_rooms_reads() {
    let mut scratch = Scratch::new("checked");
    let kept = scratch.henry_keeps_jasmine();
    for (line, why) in [
        (
            json!({"op":"append_linked","person":HENRY,"at_unix_ms":NOW,"heard":"hi",
                "source_ids":[],"keeper":HENRY,"keeper_ids":[kept]}),
            "a person's own line",
        ),
        (
            json!({"op":"append_linked","at_unix_ms":NOW,"heard":"hi","source_ids":[],
                "keeper":"everyone","keeper_ids":[kept]}),
            "the room is no keeper",
        ),
        (
            json!({"op":"append_linked","at_unix_ms":NOW,"heard":"hi","source_ids":[],
                "keeper_ids":[kept]}),
            "ids with no keeper",
        ),
        (
            json!({"op":"append_linked","at_unix_ms":NOW,"heard":"hi","source_ids":[],
                "keeper":HENRY,"keeper_ids":[kept, kept]}),
            "an id twice",
        ),
    ] {
        let answer = scratch.ask(line);
        assert_eq!(answer["error"], "bad_request", "{why}: {answer}");
    }
    // The room's record of the cited line tells no id of his.
    let room = scratch.ask(json!({"op":"search","query":"cheese","limit":20,"max_bytes":4000}));
    let entries = room["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{room}");
    assert!(
        entries[0]["source_ids"]
            .as_array()
            .is_none_or(|ids| ids.is_empty()),
        "{room}"
    );
    // An id that is not his kept thing is not linked, and the line is kept.
    let line = scratch.ask(json!({"op":"append_linked","at_unix_ms":NOW + 4_000,
        "heard":"jasmine again","source_ids":[],"keeper":HENRY,"keeper_ids":[999]}));
    assert_eq!(line["ok"], true, "{line}");
    scratch.ask(json!({"op":"forget","person":HENRY,"id":kept,"selective":true}));
    assert_eq!(
        scratch.holding(None, "jasmine").len(),
        2,
        "the guest's and the unlinked line"
    );
}
