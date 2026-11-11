//! `LAMP_SERVICE_RECORDS` (the connector records plan, PR 1a): each outcome
//! of a services call writes its record's status and reason once, the
//! #5853 receipts and the records are one, and a record waits for the
//! memory when it is down.
use super::*;
use crate::services::{
    self, CANCELLED, NOT_HEARD, NOT_READ_BACK, NOTHING_WAITING, OTHER_VOICE, Pending, READ, Turn,
    WRITE, answered, began_in, gate, said_in,
};
use lamp_gemini::ToolCall;
use serde_json::json;
use std::time::{Duration, Instant};

const OWNER: &str = "p-0123456789abcdef";
const READ_BACK: &str = "Create an event on your calendar: \"Hamburger party\", Sunday 15 November from 7 PM to 10 PM. Shall I add it?";

fn call(name: &str, args: Value) -> ToolCall {
    ToolCall {
        id: "c1".into(),
        name: name.into(),
        args,
    }
}

fn event() -> ToolCall {
    call(
        WRITE,
        json!({
            "service": "google_calendar",
            "action": "create_event",
            "title": "Hamburger party",
            "start": "2026-11-15T19:00:00+07:00",
            "end": "2026-11-15T22:00:00+07:00",
            "text": "Bring buns",
        }),
    )
}

fn send(id: &str) -> ToolCall {
    call(
        WRITE,
        json!({"service":"google_calendar","send_id":id,"said":"send"}),
    )
}

/// A turn heard at the lamp from the owner. Since the model decides
/// whether a line confirmed (the owner, 10 Nov), its words and the line
/// before are no longer the gate's: they only name the case here.
fn heard<'a>(_words: &'a str, _previous: u64) -> Turn<'a> {
    Turn {
        heard: true,
        owner: Some(OWNER),
        sends: true,
        alike: Some(0.9),
        seen_owner: true,
        ..Turn::default()
    }
}

/// The event prepared in request 10 and read back in full: the waiting
/// write, its record `prepared`.
fn prepared(now: Instant) -> Option<Pending> {
    let mut pending = None;
    let mut told = json!({"sent": false, "send_id": "s1", "read_back": READ_BACK});
    answered(&mut pending, &event(), 10, &mut told, now, OWNER);
    said_in(&mut pending, 10, READ_BACK, now);
    began_in(&mut pending, now + Duration::from_secs(12));
    pending
}

fn at(now: Instant) -> Instant {
    now + Duration::from_secs(13)
}

fn only() -> Record {
    let held = held();
    assert_eq!(held.len(), 1, "{held:?}");
    held[0].clone()
}

fn fresh() {
    on_for_test(true);
    forgot(None);
    kept::ledger(|ledger| ledger.receipts.clear());
}

/// The read-back: one record, `prepared`, with everything the event was
/// asked with (the owner's own write, in full), never the gate's fields.
#[test]
fn a_read_back_writes_a_prepared_record_with_every_detail() {
    fresh();
    let pending = prepared(Instant::now());
    assert!(pending.is_some());
    let record = only();
    assert_eq!(record.status, Status::Prepared);
    assert_eq!(record.person, OWNER);
    assert_eq!(record.service, "google_calendar");
    assert_eq!(record.action, "create_event");
    assert_eq!(record.detail["title"], "Hamburger party");
    assert_eq!(record.detail["start"], "2026-11-15T19:00:00+07:00");
    assert_eq!(record.detail["text"], "Bring buns");
    assert!(record.detail.get("service").is_none());
    assert_eq!(record.send_id.as_deref(), Some("s1"));
    let waiting = super::waiting();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].line()["status"], "prepared");
    assert_eq!(waiting[0].line()["op"], "service_record");
}

/// The send's answer: `done` when it went, `failed` with the service's
/// error, `cancelled` when the services let it go; one change each, and a
/// later answer changes nothing.
#[test]
fn a_sends_answer_settles_its_record_once() {
    for (told, status, why) in [
        (
            json!({"sent": true, "service": "Google Calendar"}),
            Status::Done,
            "sent",
        ),
        (
            json!({"error": "not sent: the calendar refused it"}),
            Status::Failed,
            "not sent: the calendar refused it",
        ),
        (
            json!({"sent": false, "cancelled": true}),
            Status::Cancelled,
            "the service let it go",
        ),
    ] {
        fresh();
        let now = Instant::now();
        let mut pending = prepared(now);
        let passed = gate(
            &mut pending,
            &send("s1"),
            11,
            &heard("send it", 10),
            at(now),
        )
        .expect("a timely send passes");
        assert_eq!(passed.args["send_id"], "s1");
        keep_send(Some(7), Some("s1"), Got::Asked);
        assert_eq!(only().status, Status::Prepared, "asked is not done");
        keep_send(Some(7), Some("s1"), Got::Answered(&told));
        let record = only();
        assert_eq!(record.status, status, "{told}");
        assert_eq!(record.reason, why);
        // A second answer for it, or a late put: nothing changes.
        keep_send(
            Some(7),
            Some("s1"),
            Got::Answered(&json!({"error": "later"})),
        );
        assert_eq!(only().status, status);
        let changes: Vec<Record> = super::waiting();
        assert_eq!(changes.len(), 1, "one key, the latest change");
        assert_eq!(changes[0].status, status);
    }
}

/// A gate refusal of a send: `refused`, with the gate's reason, never the
/// model's words. A waiting write's send is anyone's yes, so the owner
/// check is not one of them.
#[test]
fn a_refused_send_is_refused_with_the_gates_reason() {
    let now = Instant::now();
    let cases: Vec<(&str, Turn, &str, &str)> = vec![
        (
            "not heard",
            Turn {
                heard: false,
                ..heard("send it", 10)
            },
            NOT_HEARD,
            "not heard live at the lamp",
        ),
        (
            "another voice",
            Turn {
                in_echo_tail: true,
                alike: Some(0.1),
                ..heard("send it", 10)
            },
            OTHER_VOICE,
            "another voice said send",
        ),
        (
            "another face",
            Turn {
                in_echo_tail: true,
                other_seen: true,
                ..heard("send it", 10)
            },
            OTHER_VOICE,
            "another face was seen saying send",
        ),
    ];
    for (case, turn, error, why) in cases {
        fresh();
        let mut pending = prepared(now);
        let refused = gate(&mut pending, &send("s1"), 11, &turn, at(now)).expect_err(case);
        assert_eq!(refused["error"], error, "{case}");
        let record = only();
        assert_eq!(record.status, Status::Refused, "{case}");
        assert_eq!(record.reason, why, "{case}");
    }
    // Not read back in full: a line began as the read-back started.
    fresh();
    let mut pending = None;
    let mut told = json!({"sent": false, "send_id": "s1", "read_back": READ_BACK});
    answered(&mut pending, &event(), 10, &mut told, now, OWNER);
    said_in(&mut pending, 10, READ_BACK, now);
    began_in(&mut pending, now);
    let refused = gate(
        &mut pending,
        &send("s1"),
        11,
        &heard("send it", 10),
        at(now),
    )
    .expect_err("not read back");
    assert_eq!(refused["error"], NOT_READ_BACK);
    assert_eq!(only().status, Status::Refused);
    assert_eq!(only().reason, "the message was not read back in full");
}

/// Every way a waiting write is dropped is `cancelled` with that reason:
/// another call, more lines since the read-back, the lamp answering
/// another request, the lamp answering another request, a new write, and
/// the lamp answering the line after the read-back with no send; past the
/// two minutes, `timed_out`.
#[test]
fn every_dropped_waiting_write_is_cancelled_with_its_reason() {
    let now = Instant::now();
    // Another call in the next request.
    fresh();
    let mut pending = prepared(now);
    let read = call(READ, json!({"service":"gmail","what":"recent"}));
    gate(&mut pending, &read, 11, &heard("what's new?", 10), at(now)).unwrap();
    assert!(pending.is_none());
    assert_eq!(only().status, Status::Cancelled);
    assert_eq!(only().reason, "another request came before a send");
    // The lamp answered two other requests.
    fresh();
    let mut pending = prepared(now);
    said_in(&mut pending, 11, "Sure.", now);
    said_in(&mut pending, 12, "Okay.", now);
    assert!(pending.is_none());
    assert_eq!(only().status, Status::Cancelled);
    assert_eq!(
        only().reason,
        "the lamp answered another request before a send"
    );
    // A new write in the same request replaces it.
    fresh();
    let mut pending = prepared(now);
    let _ = gate(&mut pending, &event(), 10, &heard("make it 8", 9), at(now));
    let first = held()[0].clone();
    assert_eq!(first.status, Status::Cancelled);
    assert_eq!(first.reason, "a new write replaced it");
    // The lamp answered the line after the read-back with no send ("Okay,
    // cancelled."): a send after it never goes, and the record says why.
    fresh();
    let mut pending = prepared(now);
    said_in(&mut pending, 11, "Okay, cancelled.", now);
    let refused = gate(
        &mut pending,
        &send("s1"),
        12,
        &heard("send it", 11),
        at(now),
    )
    .expect_err("answered with no send");
    assert_eq!(refused["error"], NOTHING_WAITING);
    assert_eq!(only().status, Status::Cancelled);
    assert_eq!(
        only().reason,
        "the lamp answered after the read-back with no send"
    );
    // A send past the two minutes: timed out.
    fresh();
    let mut pending = prepared(now);
    let late = now + services::PENDING_FOR + Duration::from_secs(1);
    let _ = gate(&mut pending, &send("s1"), 11, &heard("send it", 10), late);
    assert_eq!(only().status, Status::TimedOut);
}

/// Two minutes with nothing said: the writer marks the record `timed_out`
/// on its own, the waiting write itself left to the gate.
#[test]
fn a_write_waiting_past_two_minutes_times_out() {
    fresh();
    let now = Instant::now();
    let pending = prepared(now);
    expire(now + Duration::from_secs(60), services::PENDING_FOR);
    assert_eq!(only().status, Status::Prepared);
    expire(
        now + services::PENDING_FOR + Duration::from_secs(1),
        services::PENDING_FOR,
    );
    assert_eq!(only().status, Status::TimedOut);
    assert_eq!(
        only().reason,
        "no send was heard within two minutes of the read-back"
    );
    assert!(pending.is_some(), "the gate's own write is untouched");
}

/// A read keeps its query and of what it found the titles, senders, dates
/// and links, and a short excerpt only past the services' security check;
/// nothing found is `no_result`; an error is `failed`.
#[test]
fn a_read_keeps_metadata_and_a_masked_excerpt_or_no_result() {
    fresh();
    let args = json!({"service":"gmail","what":"search","query":"party"});
    let told = json!({"source":"Gmail","quoted":[
        {"id":"m1","from":"Octavia <octavia@example.com>","subject":"Hamburger party","date":"Tue, 10 Nov 2026 15:30:00 +0700","text":"Bring the buns on Sunday please, and the pickles too, I will bring the rest of the food and drinks for everyone at the party."},
        {"id":"m2","from":"Bank","subject":"Your statement","date":"Tue, 10 Nov 2026 09:00:00 +0700","text":"Use code 482913 to sign in"},
        "a security mail, left out",
    ]});
    read(OWNER, &args, &told);
    let record = only();
    assert_eq!(record.status, Status::Done);
    assert_eq!(record.service, "gmail");
    assert_eq!(record.action, "search");
    assert_eq!(record.detail["query"], "party");
    assert_eq!(record.detail["count"], 3);
    assert_eq!(record.detail["left_out"], 1);
    let found = record.detail["found"].as_array().unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0]["title"], "Hamburger party");
    assert_eq!(found[0]["from"], "Octavia <octavia@example.com>");
    let excerpt = found[0]["excerpt"].as_str().unwrap();
    assert!(excerpt.len() <= EXCERPT_BYTES && excerpt.starts_with("Bring the buns"));
    assert!(found[1].get("excerpt").is_none(), "{}", found[1]);
    assert!(!record.detail.to_string().contains("482913"));
    // Drive "Sunday" found nothing.
    fresh();
    let args = json!({"service":"google_drive","what":"files","query":"Sunday"});
    read(OWNER, &args, &json!({"source":"Google Drive","quoted":[]}));
    assert_eq!(only().status, Status::NoResult);
    assert_eq!(only().detail["query"], "Sunday");
    // An error.
    fresh();
    read(OWNER, &args, &json!({"error":"Google Drive is not linked"}));
    assert_eq!(only().status, Status::Failed);
    assert_eq!(only().reason, "Google Drive is not linked");
}

/// Thirteen's P2 on #6073: each kept field is held to the excerpt's rule.
/// A subject that is only a code, or a code with "to sign in", and a reset
/// link with a token are never stored; a plain subject and a Drive
/// `webViewLink` still are.
#[test]
fn a_reads_title_and_link_with_a_secrets_shape_are_never_stored() {
    fresh();
    let args = json!({"service":"gmail","what":"search","query":"code"});
    let told = json!({"source":"Gmail","quoted":[
        {"id":"m1","from":"Shop","subject":"F7K2Q9","date":"Tue, 10 Nov 2026 15:30:00 +0700"},
        {"id":"m2","from":"Shop","subject":"Ref 4X9QJ2 to sign in"},
        {"id":"m3","from":"Shop","subject":"Your account","link":"https://shop.example.com/reset?token=abcdefghijklmnopqrstuvwxyz0123"},
        {"id":"m4","from":"Shop","subject":"Your order","link":"https://shop.example.com/orders?id=1&code=77aa"},
        {"id":"m5","from":"bob42@x.com","subject":"Lunch"},
        {"id":"m6","from":"Bob <bob42@x.com>","subject":"Lunch again"},
        {"id":"m7","from":"Team 7Q4K2X <noreply@x.com>","subject":"Hello"},
    ]});
    read(OWNER, &args, &told);
    let detail = only().detail.to_string();
    for secret in ["F7K2Q9", "4X9QJ2", "token=", "code=77aa", "7Q4K2X"] {
        assert!(!detail.contains(secret), "{secret} kept: {detail}");
    }
    let found = only().detail["found"].as_array().unwrap().clone();
    assert_eq!(found[2]["title"], "Your account", "{detail}");
    assert_eq!(found[3]["title"], "Your order", "{detail}");
    // A sender is checked by its display name only (as `masked_from`): a
    // bare address with digits, or one in `<…>`, is kept.
    assert_eq!(found[4]["from"], "bob42@x.com", "{detail}");
    assert_eq!(found[5]["from"], "Bob <bob42@x.com>", "{detail}");
    assert!(found[6].get("from").is_none(), "{detail}");
    assert_eq!(found[6]["title"], "Hello", "{detail}");
    fresh();
    let args = json!({"service":"google_drive","what":"files","query":"trip"});
    let link = "https://docs.google.com/document/d/1AbCdEfGhIjKlMnOpQrStUvWxYz0123456789/edit?usp=drivesdk";
    let told = json!({"source":"Google Drive","quoted":[
        {"id":"1AbCdEfGhIjKlMnOpQrStUvWxYz0123456789","name":"Trip plan","webViewLink":link},
    ]});
    read(OWNER, &args, &told);
    let found = &only().detail["found"][0];
    assert_eq!(found["title"], "Trip plan");
    assert_eq!(found["link"], link);
}

/// #5853, one source: a send cut in on (its turn's lease no longer
/// current) still goes to its answer, which sets the receipt the cut-in
/// turn reads and the record together; the receipt says sent, the record
/// done, once, and neither is written again by anyone else.
#[test]
fn a_cut_in_send_is_told_and_recorded_once() {
    fresh();
    let now = Instant::now();
    let mut pending = prepared(now);
    gate(
        &mut pending,
        &send("s1"),
        11,
        &heard("send it", 10),
        at(now),
    )
    .unwrap();
    keep_send(Some(42), Some("s1"), Got::Asked);
    // The cut-in turn looks while the send waits: asked, not sent.
    assert_eq!(services::send_state(42), Some(services::SendState::Asked));
    assert_eq!(only().status, Status::Prepared);
    // The send's own call gets its answer.
    let sent = json!({"sent": true, "service": "Google Calendar"});
    keep_send(Some(42), Some("s1"), Got::Answered(&sent));
    assert_eq!(
        services::send_state(42),
        Some(services::SendState::Told(sent.clone()))
    );
    assert_eq!(only().status, Status::Done);
    // Told again (a retry of the same try), it stays sent and done, one
    // change waiting.
    keep_send(
        Some(42),
        Some("s1"),
        Got::Refused(&json!({"error": CANCELLED})),
    );
    assert_eq!(
        services::send_state(42),
        Some(services::SendState::Told(sent))
    );
    assert_eq!(only().status, Status::Done);
    assert_eq!(super::waiting().len(), 1);
    assert_eq!(super::waiting()[0].status, Status::Done);
}

/// A slow service: no answer within the bound is `failed`, saying it is
/// not known whether it went, never `done`; the receipt stays asked, so a
/// cut-in turn says it could not confirm it, as before.
#[test]
fn a_send_with_no_answer_in_time_is_not_known_never_done() {
    fresh();
    let now = Instant::now();
    let mut pending = prepared(now);
    gate(
        &mut pending,
        &send("s1"),
        11,
        &heard("send it", 10),
        at(now),
    )
    .unwrap();
    keep_send(Some(5), Some("s1"), Got::Asked);
    keep_send(Some(5), Some("s1"), Got::Unanswered);
    assert_eq!(services::send_state(5), Some(services::SendState::Asked));
    let record = only();
    assert_eq!(record.status, Status::Failed);
    assert!(record.reason.contains("not known"), "{}", record.reason);
}

/// Off: no record is kept or waits, and the receipts are kept as before.
#[test]
fn off_keeps_no_record_and_the_receipts_as_before() {
    fresh();
    on_for_test(false);
    let now = Instant::now();
    let mut pending = prepared(now);
    gate(
        &mut pending,
        &send("s1"),
        11,
        &heard("send it", 10),
        at(now),
    )
    .unwrap();
    keep_send(Some(9), Some("s1"), Got::Asked);
    keep_send(Some(9), Some("s1"), Got::Answered(&json!({"sent": true})));
    read(
        OWNER,
        &json!({"service":"gmail","what":"recent"}),
        &json!({"quoted":[]}),
    );
    assert!(held().is_empty());
    assert!(super::waiting().is_empty());
    assert_eq!(
        services::send_state(9),
        Some(services::SendState::Told(json!({"sent": true})))
    );
}

/// Memory down: the change waits and is tried again, said once in the
/// journal; a newer change of the same record replaces it; a refusal (an
/// older memory) lets it go; the queue holds at most [`OUTBOX_MOST`].
#[test]
fn memory_down_queues_records_and_tries_again() {
    fresh();
    let now = Instant::now();
    let mut pending = prepared(now);
    assert_eq!(save_next(|_| None), Saved::Down);
    assert_eq!(super::waiting().len(), 1);
    assert_eq!(save_next(|_| None), Saved::Down);
    // Settled meanwhile: still one change waiting, the latest.
    said_in(&mut pending, 11, "Okay, cancelled.", now);
    gate(
        &mut pending,
        &send("s1"),
        12,
        &heard("send it", 11),
        at(now),
    )
    .unwrap_err();
    assert_eq!(super::waiting().len(), 1);
    assert_eq!(super::waiting()[0].status, Status::Cancelled);
    let mut sent = Vec::new();
    assert_eq!(
        save_next(|line| {
            sent.push(line.clone());
            Some(json!({"ok": true, "status": "cancelled"}))
        }),
        Saved::Done
    );
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["status"], "cancelled");
    assert!(super::waiting().is_empty());
    assert_eq!(save_next(|_| panic!("nothing waits")), Saved::Nothing);
    // An older memory refuses the op: let go, never tried for ever.
    read(
        OWNER,
        &json!({"service":"gmail","what":"recent"}),
        &json!({"quoted":[]}),
    );
    assert_eq!(
        save_next(|_| Some(json!({"ok": false, "error": "unknown_op"}))),
        Saved::Done
    );
    assert!(super::waiting().is_empty());
    // Bounded.
    for _ in 0..OUTBOX_MOST + 5 {
        read(
            OWNER,
            &json!({"service":"gmail","what":"recent"}),
            &json!({"quoted":[]}),
        );
    }
    assert_eq!(super::waiting().len(), OUTBOX_MOST);
}

/// Forget everything: the person's records and waiting changes go; the
/// receipts stay, outcome only. A forget about a thing takes the records
/// that name it.
#[test]
fn a_forget_takes_the_records_held_and_waiting() {
    fresh();
    let now = Instant::now();
    let mut pending = prepared(now);
    gate(
        &mut pending,
        &send("s1"),
        11,
        &heard("send it", 10),
        at(now),
    )
    .unwrap();
    keep_send(Some(3), Some("s1"), Got::Answered(&json!({"sent": true})));
    forgot(Some(OWNER));
    assert!(held().is_empty());
    assert!(super::waiting().is_empty());
    assert_eq!(
        services::send_state(3),
        Some(services::SendState::Told(json!({"sent": true})))
    );
    // About the party: that one only.
    fresh();
    let _ = prepared(now);
    read(
        OWNER,
        &json!({"service":"google_drive","what":"files","query":"Sunday"}),
        &json!({"quoted":[]}),
    );
    forgot_about(OWNER, &[vec!["parties".to_owned()]]);
    let held = held();
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].service, "google_drive");
    assert_eq!(super::waiting().len(), 1);
}

/// A replay of the tester's lamp, 10 Nov 15:30-15:58: Drive searched for
/// "Sunday" found nothing; the hamburger party event read back, then "go
/// ahead" the lamp answered with no send, then another request. The Drive search ends
/// `no_result`, the event `cancelled` with why, and nothing says done.
#[test]
fn the_ten_november_session_ends_cancelled_and_no_result() {
    fresh();
    let now = Instant::now();
    let drive = call(
        READ,
        json!({"service":"google_drive","what":"files","query":"Sunday"}),
    );
    let mut pending = None;
    gate(
        &mut pending,
        &drive,
        3,
        &heard("search my drive for Sunday", 2),
        now,
    )
    .unwrap();
    let mut told = json!({"source":"Google Drive","quoted":[]});
    answered(&mut pending, &drive, 3, &mut told, now, OWNER);
    read(OWNER, &drive.args, &told);
    let mut pending = prepared(now);
    // "go ahead" (no "send"): the lamp answered it with no send.
    said_in(&mut pending, 11, "Okay, I won't send it.", now);
    let refused = gate(
        &mut pending,
        &send("s1"),
        12,
        &heard("go ahead", 11),
        at(now),
    )
    .expect_err("answered with no send");
    assert_eq!(refused["error"], NOTHING_WAITING);
    // Later, "did you do it?" and a new read: nothing changes the record.
    let _ = gate(
        &mut pending,
        &drive,
        13,
        &heard("did you do it?", 12),
        at(now),
    );
    let held = held();
    assert_eq!(held.len(), 2, "{held:?}");
    assert_eq!(held[0].service, "google_drive");
    assert_eq!(held[0].status, Status::NoResult);
    assert_eq!(held[0].detail["query"], "Sunday");
    assert_eq!(held[1].service, "google_calendar");
    assert_eq!(held[1].status, Status::Cancelled);
    assert_eq!(
        held[1].reason,
        "the lamp answered after the read-back with no send"
    );
    assert_eq!(held[1].detail["title"], "Hamburger party");
    assert!(held.iter().all(|record| record.status != Status::Done));
}
