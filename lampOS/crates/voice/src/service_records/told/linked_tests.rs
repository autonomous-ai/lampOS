//! `LAMP_SERVICE_RECORDS_LINKED_TOLD` (the linked-service memory plan,
//! slice 8): at most 2 linked lines in the note, at most 3 linked records
//! of a recall's 5 with connector records never pushed out, and `BRIEF`
//! when someone else may be here.
use super::super::linked::{self, Kind, Linked};
use super::super::{Status, forgot, held, on_for_test as records_on};
use super::*;
use serde_json::json;

const OWNER: &str = "p-0123456789abcdef";
/// 10 Nov 2026, 15:40 at the tester's lamp (UTC+7).
const NOW: u64 = 1_794_300_000_000;
const ZONE: i32 = 7 * 60;
const HOUR: u64 = 3_600_000;

fn fresh(linked_told: bool) {
    records_on(true);
    on_for_test(true);
    linked_on_for_test(linked_told);
    forgot(None);
}

fn change(key: &str, kind: Kind, summary: &str, changed: u64) -> Linked {
    Linked {
        person: OWNER.into(),
        key: key.into(),
        kind,
        service: "harness".into(),
        action: "task".into(),
        status: Status::Done,
        source_ref: json!({"service":"lamp-tasks","id":format!("s1:{key}")}),
        summary: summary.into(),
        deed_ms: changed,
        asked_ms: changed,
        changed_ms: changed,
        links: Vec::new(),
    }
}

/// Three linked records within a day and one older: the note tells the
/// latest two, tagged with their keys, beside the connector record.
#[test]
fn the_note_tells_two_linked_lines_of_the_last_day() {
    fresh(true);
    super::super::kept::ledger(|ledger| {
        ledger.records.push_back(super::super::Record {
            key: "cal-1".into(),
            person: OWNER.into(),
            service: "google_calendar".into(),
            action: "create_event".into(),
            status: Status::Done,
            reason: String::new(),
            detail: json!({"title":"Hamburger party"}),
            result_id: String::new(),
            asked_ms: NOW - HOUR,
            changed_ms: NOW - HOUR,
            send_id: None,
            prepared_at: None,
            links: Vec::new(),
            replaced_by: String::new(),
        });
    });
    for (key, kind, summary, at) in [
        ("t-s1-1", Kind::Task, "Book the dentist", NOW - 3 * HOUR),
        (
            "r-4-set",
            Kind::Reminder,
            "Take the pills at 9",
            NOW - 2 * HOUR,
        ),
        ("t-s1-2", Kind::Task, "Order printer paper", NOW - HOUR),
        ("t-s1-0", Kind::Task, "Old one", NOW - 25 * HOUR),
    ] {
        assert_ne!(
            linked::put(change(key, kind, summary, at)),
            linked::Put::Off
        );
    }
    let note = note_now(Some(OWNER), true, NOW, ZONE).expect("a note");
    let tagged: Vec<&str> = note
        .lines()
        .filter(|line| line.starts_with("- ["))
        .collect();
    assert_eq!(tagged.len(), LINKED_SHOWN, "{note}");
    assert!(tagged[0].contains("[task] Order printer paper"), "{note}");
    assert!(tagged[0].contains("key t-s1-2"), "{note}");
    assert!(
        tagged[1].contains("[reminder] Take the pills at 9"),
        "{note}"
    );
    assert!(!note.contains("Book the dentist"), "{note}");
    assert!(!note.contains("Old one"), "{note}");
    assert!(note.contains("Hamburger party"), "{note}");
    assert!(note.contains(LINKED_RULE), "{note}");
    // Linked records never enter the connector ledger.
    assert_eq!(held().len(), 1);
}

/// Someone else may be here: the linked lines say only kind, status and
/// when, no summary or key, with `BRIEF`.
#[test]
fn someone_else_here_hears_only_the_kind_and_status() {
    fresh(true);
    linked::put(change("t-s1-1", Kind::Task, "Book the dentist", NOW - HOUR));
    let note = note_now(Some(OWNER), false, NOW, ZONE).expect("a note");
    assert!(note.contains(BRIEF), "{note}");
    assert!(note.contains("- [task] · done · today"), "{note}");
    assert!(!note.contains("dentist"), "{note}");
    assert!(!note.contains("t-s1-1"), "{note}");
}

/// A forget everything, or about a thing the summary matches, takes the
/// linked lines.
#[test]
fn forgets_take_the_linked_lines() {
    fresh(true);
    linked::put(change("t-s1-1", Kind::Task, "Book the dentist", NOW - HOUR));
    linked::put(change(
        "t-s1-2",
        Kind::Task,
        "Order printer paper",
        NOW - HOUR,
    ));
    super::super::unsent_about(OWNER, &[vec!["dentist".into()]]);
    let note = note_now(Some(OWNER), true, NOW, ZONE).expect("a note");
    assert!(!note.contains("dentist"), "{note}");
    assert!(note.contains("printer paper"), "{note}");
    forgot(None);
    assert_eq!(note_now(Some(OWNER), true, NOW, ZONE), None);
}

fn listed(key: &str, kind: &str, status: &str) -> Value {
    json!({
        "key": key, "service": if kind == "voice" { "google_calendar" } else { "harness" },
        "action": if kind == "voice" { "create_event" } else { "task" },
        "status": status, "reason": "", "detail": {"title": format!("Party {key}")},
        "result_id": "", "asked_unix_ms": NOW - HOUR, "changed_unix_ms": NOW - HOUR,
        "links": [], "replaced_by": "", "source_kind": kind,
        "source_ref": if kind == "voice" { json!({}) } else { json!({"service":"lamp-tasks","id":format!("s1:{key}")}) },
        "summary": if kind == "voice" { String::new() } else { format!("Task {key}") },
        "deed_unix_ms": NOW - HOUR,
    })
}

fn kinds(told: &Value) -> Vec<String> {
    told["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["kind"].as_str().unwrap_or("voice").to_owned())
        .collect()
}

/// A recall is told at most 3 linked records of its 5, and a connector
/// record is never pushed out by them: the memory is asked for 10, at
/// most 3 of them linked.
#[test]
fn a_recall_tells_at_most_three_linked_of_five() {
    fresh(true);
    let asked = about_request(OWNER, "the party");
    assert_eq!(asked["limit"], RECALL_ASKED);
    assert_eq!(asked["linked_most"], RECALL_LINKED_MOST);
    let mut items: Vec<Value> = (0..6)
        .map(|n| listed(&format!("t-s1-{n}"), "task", "done"))
        .collect();
    items.push(listed("cal-1", "voice", "done"));
    items.push(listed("cal-2", "voice", "cancelled"));
    let answer = json!({"ok":true,"records":items});
    let told = recall_told(&answer, true, NOW, ZONE, 8_000).expect("found");
    let told_kinds = kinds(&told);
    assert_eq!(told_kinds.len(), 5, "{told}");
    assert_eq!(told_kinds.iter().filter(|kind| *kind == "voice").count(), 2);
    assert_eq!(told_kinds.iter().filter(|kind| *kind == "task").count(), 3);
    assert_eq!(told["records"][0]["key"], "t-s1-0");
    assert!(
        told["records"][0]["summary"]
            .as_str()
            .unwrap()
            .starts_with("[task] Task t-s1-0")
    );
    // Five connector records and two linked: the five, no linked one.
    let mut items: Vec<Value> = (0..2)
        .map(|n| listed(&format!("t-s1-{n}"), "task", "done"))
        .collect();
    items.extend((0..5).map(|n| listed(&format!("cal-{n}"), "voice", "done")));
    let told = recall_told(&json!({"ok":true,"records":items}), true, NOW, ZONE, 8_000).unwrap();
    assert_eq!(kinds(&told), vec!["voice"; 5], "{told}");
    // Three connector records and six linked: the three and two linked.
    let mut items: Vec<Value> = (0..6)
        .map(|n| listed(&format!("t-s1-{n}"), "task", "done"))
        .collect();
    items.extend((0..3).map(|n| listed(&format!("cal-{n}"), "voice", "done")));
    let told = recall_told(&json!({"ok":true,"records":items}), true, NOW, ZONE, 8_000).unwrap();
    let told_kinds = kinds(&told);
    assert_eq!(told_kinds.iter().filter(|kind| *kind == "voice").count(), 3);
    assert_eq!(told_kinds.iter().filter(|kind| *kind == "task").count(), 2);
}

/// A linked record changed exactly a day ago is still told in the note;
/// a millisecond older, not.
#[test]
fn a_linked_line_of_exactly_a_day_is_told() {
    fresh(true);
    linked::put(change(
        "t-s1-1",
        Kind::Task,
        "Book the dentist",
        NOW - LINKED_WITHIN_MS,
    ));
    linked::put(change(
        "t-s1-2",
        Kind::Task,
        "Order printer paper",
        NOW - LINKED_WITHIN_MS - 1,
    ));
    let note = note_now(Some(OWNER), true, NOW, ZONE).expect("a note");
    assert!(note.contains("Book the dentist"), "{note}");
    assert!(!note.contains("printer paper"), "{note}");
}

/// Someone else may be here: a linked record a recall finds is told by its
/// kind, status and when only, with `BRIEF`.
#[test]
fn a_recall_with_someone_else_here_is_brief() {
    fresh(true);
    let answer = json!({"ok":true,"records":[listed("t-s1-1", "task", "done")]});
    let told = recall_told(&answer, false, NOW, ZONE, 8_000).expect("found");
    assert_eq!(told["others_here"], BRIEF);
    let record = &told["records"][0];
    assert!(record.get("key").is_none(), "{told}");
    assert!(!told.to_string().contains("Task t-s1-1"), "{told}");
}

/// After a start, the memory's linked records go to the note's list, not
/// the connector ledger.
#[test]
fn after_a_start_linked_records_go_to_the_note() {
    fresh(true);
    let answer = json!({"ok":true,"records":[
        listed("t-s1-1", "task", "done"),
        listed("cal-1", "voice", "done"),
    ]});
    merge(OWNER, &answer, NOW, super::super::forgets());
    assert_eq!(held().len(), 1);
    assert_eq!(held()[0].key, "cal-1");
    assert_eq!(linked::recent_of(OWNER).len(), 1);
    let note = note_now(Some(OWNER), true, NOW, ZONE).unwrap();
    assert!(note.contains("[task] Task t-s1-1"), "{note}");
}

/// Off: no linked line, a recall asks 5 and tells what it found as
/// before, and the memory's list is merged as before.
#[test]
fn off_changes_nothing() {
    fresh(false);
    linked::put(change("t-s1-1", Kind::Task, "Book the dentist", NOW - HOUR));
    assert_eq!(note_now(Some(OWNER), true, NOW, ZONE), None);
    let asked = about_request(OWNER, "the party");
    assert_eq!(
        asked,
        json!({"op":"service_records","person":OWNER,"about":"the party","limit":RECALL_MOST})
    );
    let answer = json!({"ok":true,"records":[listed("t-s1-1", "task", "done")]});
    let told = recall_told(&answer, true, NOW, ZONE, 8_000).unwrap();
    assert!(told["records"][0].get("kind").is_none(), "{told}");
    merge(OWNER, &answer, NOW, super::super::forgets());
    assert_eq!(held().len(), 1);
}
