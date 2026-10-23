//! #2849 (the first builder's yes, 23 Oct 09:37; the voice's
//! `LAMP_MEMORY_ROOM_SPEAKER`): a room line said by one known person keeps
//! their id beside it, in the `speakers` side table, and that person's
//! "forget everything" (`forget_said`) removes the room's lines they said
//! and nothing else. The id is never in anything the memory answers.
use lamp_memory::{
    Options, Person,
    days::{DATABASE, Days},
    days_protocol,
    store::LocalTime,
    tidy::{Category, Output, Proposal},
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

const NOW: u64 = 1_791_892_800_000;
const DAY: u64 = 86_400_000;
const MAI: &str = "p-0123456789abcdef";
const LAN: &str = "p-fedcba9876543210";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lamp-speakers-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        Self(root)
    }

    fn open(&self, keep_days: u32, now: u64) -> Days {
        Days::open(
            Options {
                dir: self.0.clone(),
                keep_days,
                local: LocalTime::Offset(0),
            },
            now,
        )
        .unwrap()
    }

    fn connection(&self) -> Connection {
        Connection::open(self.0.join(DATABASE)).unwrap()
    }

    /// The `speakers` rows, as (person, id, speaker) of their entries,
    /// by id; and how many name no entry at all (none, ever).
    fn speakers(&self) -> (Vec<(String, u64, String)>, u64) {
        let db = self.connection();
        let rows = db
            .prepare("SELECT e.person,e.id,s.speaker FROM speakers s JOIN entries e ON e.rid=s.rid ORDER BY e.person,e.id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let orphans = db
            .query_row(
                "SELECT count(*) FROM speakers WHERE rid NOT IN (SELECT rid FROM entries)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        (rows, orphans)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One request through the Days protocol: its answer and its log line.
fn ask(store: &mut Days, request: Value, now: u64) -> (Value, String) {
    let answer = days_protocol::answer(store, request.to_string().as_bytes(), now);
    (serde_json::from_str(&answer.line).unwrap(), answer.log)
}

fn id(store: &mut Days, request: Value) -> u64 {
    let (line, _) = ask(store, request.clone(), NOW);
    line["id"]
        .as_u64()
        .unwrap_or_else(|| panic!("{request}: {line}"))
}

fn room(id: u64, speaker: &str) -> (String, u64, String) {
    ("everyone".into(), id, speaker.into())
}

/// The room's observation from `sources`, by the observe pass.
fn observe(store: &mut Days, conversation: &str, sources: &[u64], text: &str) -> u64 {
    let everyone = Person::everyone();
    let job = store
        .conversation_end(&everyone, conversation, sources)
        .unwrap();
    let out = Output {
        observations: vec![Proposal {
            text: text.into(),
            category: Category::Preference,
            source_ids: sources.to_vec(),
            supersedes: None,
            on: None,
        }],
    };
    store.tidy_apply(&everyone, &job.job, &out).unwrap()[0]
}

fn exists(store: &Days, person: &str, id: u64) -> bool {
    store
        .record(&Person::new(person).unwrap(), id)
        .unwrap()
        .is_some()
}

/// `speaker` on `append`, `append_linked` and `remember` in the room's
/// scope: one row for each line written with one, none for a line without,
/// and none for a `once` that found the kept thing (nothing is written).
/// A speaker that is no known person's id, or on a line of anyone's but
/// the room's, is `bad_request`, and nothing is written.
#[test]
fn a_room_line_with_a_speaker_keeps_their_id_and_one_without_keeps_none() {
    let scratch = Scratch::new("written");
    let mut store = scratch.open(30, NOW);
    let a = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"The plants need water","said":"I'll remember","speaker":MAI}),
    );
    let b = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"Who is at the door?","said":"I can't see"}),
    );
    let c = id(
        &mut store,
        json!({"op":"append_linked","at_unix_ms":NOW,"heard":"And the fern?","said":"That too","source_ids":[],"speaker":MAI}),
    );
    let k = id(
        &mut store,
        json!({"op":"remember","person":"everyone","at_unix_ms":NOW,"text":"The plants need water","source_ids":[a],"speaker":MAI}),
    );
    let l = id(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":"Bread on Fridays","speaker":LAN}),
    );
    // `null` is no speaker.
    let n = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"Hello","speaker":null}),
    );
    // Found already: nothing written, so no row of Lan's on Mai's line.
    let (again, _) = ask(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":"the plants need water","once":true,"speaker":LAN}),
        NOW,
    );
    assert_eq!(again, json!({"ok":true,"id":k,"existing":true}));
    assert_eq!(
        scratch.speakers(),
        (
            vec![room(a, MAI), room(c, MAI), room(k, MAI), room(l, LAN)],
            0
        )
    );
    for unsaid in [b, n] {
        assert!(!scratch.speakers().0.iter().any(|(_, id, _)| *id == unsaid));
    }
    let status = ask(&mut store, json!({"op":"status","person":null}), NOW).0;
    for refused in [
        json!({"op":"append","person":MAI,"at_unix_ms":NOW,"heard":"mine","speaker":MAI}),
        json!({"op":"append","person":"r-0123456789abcdef","at_unix_ms":NOW,"heard":"x","speaker":MAI}),
        json!({"op":"remember","person":LAN,"at_unix_ms":NOW,"text":"theirs","speaker":LAN}),
        json!({"op":"append_linked","person":MAI,"at_unix_ms":NOW,"heard":"x","source_ids":[],"speaker":MAI}),
        json!({"op":"append","at_unix_ms":NOW,"heard":"x","speaker":"mai"}),
        json!({"op":"append","at_unix_ms":NOW,"heard":"x","speaker":"everyone"}),
        json!({"op":"append","at_unix_ms":NOW,"heard":"x","speaker":""}),
        json!({"op":"remember","at_unix_ms":NOW,"text":"x","speaker":"p-0123"}),
        json!({"op":"append","at_unix_ms":NOW,"heard":"x","speaker":7}),
    ] {
        let (line, _) = ask(&mut store, refused.clone(), NOW);
        assert_eq!(line["error"], "bad_request", "{refused}: {line}");
        assert_eq!(
            ask(&mut store, json!({"op":"status","person":null}), NOW).0,
            status,
            "{refused}"
        );
    }
    assert_eq!(scratch.speakers().0.len(), 4);
}

/// `forget_said {speaker}` removes the room's lines that speaker said, with
/// their versions, and answers how many went. Another speaker's lines, the
/// lines nobody was seen saying, and every person's own scope stay; a
/// speaker who said nothing removes nothing. Its log line is counts only.
#[test]
fn forget_said_removes_exactly_that_speakers_room_lines_and_their_versions() {
    let scratch = Scratch::new("forget-said");
    let mut store = scratch.open(30, NOW);
    let hers = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"My sister marries in June","said":"Lovely","speaker":MAI}),
    );
    let violet = id(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":"The door code is 1234","speaker":MAI}),
    );
    // Corrected later by a line nobody was seen saying: a version of hers.
    let (corrected, _) = ask(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW+1,"text":"The door code is 4321","replaces":violet,"shown":[violet]}),
        NOW + 1,
    );
    let corrected = corrected["id"].as_u64().unwrap();
    let lans = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"Bread on Fridays","said":"Noted","speaker":LAN}),
    );
    let nobodys = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"What time is it?","said":"Noon"}),
    );
    let her_own = id(
        &mut store,
        json!({"op":"remember","person":MAI,"at_unix_ms":NOW,"text":"Takes her tea green"}),
    );
    let (gone, log) = ask(&mut store, json!({"op":"forget_said","speaker":MAI}), NOW);
    assert_eq!(gone, json!({"ok":true,"removed":3}));
    assert_eq!(log, "forget_said person=everyone ok entries=0");
    for id in [hers, violet, corrected] {
        assert!(!exists(&store, "everyone", id), "{id}");
    }
    assert!(exists(&store, "everyone", lans));
    assert!(exists(&store, "everyone", nobodys));
    assert!(exists(&store, MAI, her_own));
    assert_eq!(scratch.speakers(), (vec![room(lans, LAN)], 0));
    // Again, or for someone who said nothing: nothing more.
    for speaker in [MAI, "p-1111111111111111"] {
        let (line, _) = ask(
            &mut store,
            json!({"op":"forget_said","speaker":speaker}),
            NOW,
        );
        assert_eq!(line, json!({"ok":true,"removed":0}), "{speaker}");
    }
    // The room named outright is the room.
    let (line, _) = ask(
        &mut store,
        json!({"op":"forget_said","person":"everyone","speaker":LAN}),
        NOW,
    );
    assert_eq!(line, json!({"ok":true,"removed":1}));
    assert!(exists(&store, "everyone", nobodys));
}

/// An observation or kept thing made from the speaker's lines goes only
/// when every one of its sources is among them; one also made from someone
/// else's words, or from a line nobody was seen saying, stays, and so does
/// that other line: nothing climbs, no sibling goes (the #3127 shape). A
/// kept thing with a speaker of its own that isn't theirs stays though its
/// sources are all theirs; one with no sources at all is not "made from"
/// their lines and stays.
#[test]
fn a_thing_made_from_their_lines_goes_only_when_every_source_is_theirs() {
    let scratch = Scratch::new("mixed-sources");
    let mut store = scratch.open(30, NOW);
    let line = |store: &mut Days, heard: &str, speaker: Option<&str>| {
        let mut request = json!({"op":"append","at_unix_ms":NOW,"heard":heard,"said":"Mm"});
        if let Some(speaker) = speaker {
            request["speaker"] = json!(speaker);
        }
        id(store, request)
    };
    let m1 = line(&mut store, "I water the plants on Sunday", Some(MAI));
    let m2 = line(&mut store, "The fern too", Some(MAI));
    let l1 = line(&mut store, "And I feed the cat", Some(LAN));
    let u1 = line(&mut store, "Someone said the cat is hungry", None);
    let only_hers = observe(&mut store, "c1", &[m1, m2], "Plants are watered on Sunday");
    let with_lans = observe(
        &mut store,
        "c2",
        &[m1, l1],
        "Sunday is for plants and the cat",
    );
    let with_nobodys = observe(&mut store, "c3", &[m2, u1], "The fern and the cat");
    let kept_from_hers = id(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":"Water the plants on Sunday","source_ids":[m1]}),
    );
    let lans_from_hers = id(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":"The fern is watered too","source_ids":[m2],"speaker":LAN}),
    );
    let unsourced = id(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":"The cat is called Bo"}),
    );
    let (gone, _) = ask(&mut store, json!({"op":"forget_said","speaker":MAI}), NOW);
    assert_eq!(
        gone,
        json!({"ok":true,"removed":4}),
        "m1, m2 and two made only of them"
    );
    for id in [m1, m2, only_hers, kept_from_hers] {
        assert!(!exists(&store, "everyone", id), "{id} should go");
    }
    for id in [l1, u1, with_lans, with_nobodys, lans_from_hers, unsourced] {
        assert!(exists(&store, "everyone", id), "{id} should stay");
    }
    assert_eq!(
        scratch.speakers(),
        (vec![room(l1, LAN), room(lans_from_hers, LAN)], 0)
    );
}

/// `forget_said` is the room's alone: a person's scope, a rules scope or
/// every person is `bad_person`; a speaker that is no known person's id, or
/// none, is `bad_request`. Nothing is removed by any of them.
#[test]
fn forget_said_is_for_the_room_and_one_known_persons_id_only() {
    let scratch = Scratch::new("forget-said-refused");
    let mut store = scratch.open(30, NOW);
    let hers = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"Mine","speaker":MAI}),
    );
    for (refused, error) in [
        (
            json!({"op":"forget_said","person":MAI,"speaker":MAI}),
            "bad_person",
        ),
        (
            json!({"op":"forget_said","person":null,"speaker":MAI}),
            "bad_person",
        ),
        (
            json!({"op":"forget_said","person":"r-0123456789abcdef","speaker":MAI}),
            "bad_person",
        ),
        (json!({"op":"forget_said","speaker":"mai"}), "bad_request"),
        (
            json!({"op":"forget_said","speaker":"everyone"}),
            "bad_request",
        ),
        (json!({"op":"forget_said"}), "bad_request"),
        (
            json!({"op":"forget_said","speaker":MAI,"id":1}),
            "bad_request",
        ),
    ] {
        let (line, _) = ask(&mut store, refused.clone(), NOW);
        assert_eq!(line["error"], error, "{refused}: {line}");
        assert!(exists(&store, "everyone", hers), "{refused}");
    }
}

/// A speakers row goes with its entry, whatever removes it: expiry, a
/// forget by id, `forget_last`, the room's `forget_all`, or its line being
/// adopted by someone known (then it is theirs, no longer the room's).
#[test]
fn a_speakers_row_goes_with_its_entry() {
    let scratch = Scratch::new("rows-go");
    let mut store = scratch.open(1, NOW);
    let said = |store: &mut Days, heard: &str, at: u64| {
        id(
            store,
            json!({"op":"append","at_unix_ms":at,"heard":heard,"speaker":MAI}),
        )
    };
    let old = said(&mut store, "Old words", NOW);
    let forgotten = said(&mut store, "Forget me", NOW + 2 * DAY);
    let adopted = said(&mut store, "Adopt me", NOW + 2 * DAY);
    let last = said(&mut store, "The last", NOW + 2 * DAY);
    store.maintain(NOW + 2 * DAY).unwrap();
    assert!(!exists(&store, "everyone", old), "expired");
    assert_eq!(scratch.speakers().0.len(), 3);
    store.forget(&Person::everyone(), forgotten).unwrap();
    assert_eq!(scratch.speakers().0.len(), 2);
    let moved = store
        .adopt(&Person::everyone(), &Person::new(MAI).unwrap(), &[adopted])
        .unwrap();
    assert_eq!(moved.len(), 1);
    assert_eq!(scratch.speakers(), (vec![room(last, MAI)], 0));
    let (line, _) = ask(&mut store, json!({"op":"forget_last"}), NOW + 2 * DAY);
    assert_eq!(line["removed"]["id"], last, "{line}");
    assert_eq!(scratch.speakers(), (vec![], 0));
    said(&mut store, "Once more", NOW + 2 * DAY);
    store.forget_all(Some(&Person::everyone())).unwrap();
    assert_eq!(scratch.speakers(), (vec![], 0));
}

/// The one rule of #2849: who said a room line is read only by
/// `forget_said`. It is in no answer the memory gives, the session note,
/// what is kept, recall, search with history (a follow-up is never the
/// room's), the observe
/// pass's input or status (no id and no `speaker` field), nor in any log
/// line, a write's or a forget's.
#[test]
fn a_rooms_speaker_is_never_in_what_the_memory_answers() {
    let scratch = Scratch::new("never-told");
    let mut store = scratch.open(30, NOW);
    let a = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"The plants need water","said":"Noted","speaker":MAI}),
    );
    id(
        &mut store,
        json!({"op":"append_linked","at_unix_ms":NOW,"heard":"The plants again","said":"Yes","source_ids":[a],"speaker":MAI}),
    );
    id(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":"The plants need water","source_ids":[a],"speaker":MAI}),
    );
    observe(&mut store, "c", &[a], "The plants need water on Sunday");
    let mut lines = vec![];
    for request in [
        json!({"op":"session_open","session":"s1","max_bytes":1200}),
        json!({"op":"kept","observations":true}),
        json!({"op":"recall"}),
        json!({"op":"recall","query":"plants"}),
        json!({"op":"recall","query":"plants","recall_v2":true}),
        json!({"op":"search","query":"plants","history":true}),
        json!({"op":"conversation_end","conversation":"c9","source_ids":[a]}),
        json!({"op":"status"}),
        json!({"op":"status","person":null}),
        json!({"op":"forget_said","speaker":MAI}),
    ] {
        let answer = days_protocol::answer(&mut store, request.to_string().as_bytes(), NOW);
        lines.push((request, answer.line, answer.log));
    }
    for (request, line, log) in &lines {
        let answer: Value = serde_json::from_str(line).unwrap();
        assert_eq!(answer["ok"], true, "{request}: {line}");
        for said in [line, log] {
            assert!(!said.contains(MAI), "{request}: {said}");
            assert!(!said.contains("\"speaker\""), "{request}: {said}");
        }
    }
    // The reads found the lines: the test reads what was there.
    assert!(lines[3].1.contains("plants"), "{}", lines[3].1);
}

/// A store made before `speakers` (the build before #2849) gets the table
/// on open, its lines have no speaker, and a new line keeps one.
#[test]
fn a_store_from_before_speakers_opens_and_gets_the_table() {
    let scratch = Scratch::new("before");
    let mut store = scratch.open(30, NOW);
    let old = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"From before"}),
    );
    drop(store);
    scratch
        .connection()
        .execute_batch("DROP INDEX speakers_by; DROP TABLE speakers;")
        .unwrap();
    let mut store = scratch.open(30, NOW);
    assert_eq!(scratch.speakers(), (vec![], 0));
    let new = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"From now","speaker":MAI}),
    );
    assert_eq!(scratch.speakers(), (vec![room(new, MAI)], 0));
    let version: u64 = scratch
        .connection()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 1, "no version bump");
    let (line, _) = ask(&mut store, json!({"op":"forget_said","speaker":MAI}), NOW);
    assert_eq!(line, json!({"ok":true,"removed":1}));
    assert!(exists(&store, "everyone", old));
}

/// The build before #2849 opening a store this one wrote: `user_version`
/// is still 1, so it is not set aside; its own schema (this one's without
/// the `speakers` block) runs on it and changes nothing; what it held is
/// all there; and a removal it makes takes the line's speakers row with it
/// (the foreign key's cascade), so no row is left naming a line forgotten.
#[test]
fn the_build_before_opens_a_store_with_speakers_and_keeps_what_it_held() {
    let schema = include_str!("../../src/days.sql");
    let (before, ours) = schema
        .split_once("-- #2849 (the first builder's yes")
        .expect("the speakers block is the schema's last");
    assert!(ours.contains("CREATE TABLE IF NOT EXISTS speakers"));
    assert!(!before.contains("speakers"));
    let scratch = Scratch::new("build-before");
    let mut store = scratch.open(30, NOW);
    let hers = id(
        &mut store,
        json!({"op":"append","at_unix_ms":NOW,"heard":"The plants","speaker":MAI}),
    );
    let kept = id(
        &mut store,
        json!({"op":"remember","at_unix_ms":NOW,"text":"Water them Sunday","speaker":MAI}),
    );
    drop(store);
    // The older build's open: the version read first, then its schema.
    let db = scratch.connection();
    let version: u64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert!(version <= 1, "an older build would set aside {version}");
    db.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    db.execute_batch(before).unwrap();
    let entries: u64 = db
        .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))
        .unwrap();
    assert_eq!(entries, 2);
    // Its forget of the line, as its `erase` deletes.
    db.execute(
        "DELETE FROM entries WHERE person='everyone' AND id=?",
        [hers],
    )
    .unwrap();
    drop(db);
    assert_eq!(scratch.speakers(), (vec![room(kept, MAI)], 0));
    // And this build reads it back as it was.
    let mut store = scratch.open(30, NOW);
    assert!(exists(&store, "everyone", kept));
    let (line, _) = ask(&mut store, json!({"op":"forget_said","speaker":MAI}), NOW);
    assert_eq!(line, json!({"ok":true,"removed":1}));
}
