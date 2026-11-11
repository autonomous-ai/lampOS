//! `LAMP_SERVICE_REDO` (the connector records plan, PR 1d): a write redone
//! from its record fills every saved detail, applies only the change, and
//! still needs its read-back and a send; the records link both ways;
//! a sent mail becomes a follow-up, a created event is refused, someone
//! else is refused, and off nothing changes.
use super::*;
use crate::service_records::{Got, ended, expire, forgot, keep_send, on_for_test};
use crate::services::{
    NOTHING_WAITING, OWNER_ONLY, Pending, READ_BACK_NEXT, Turn, WRITE, answered, began_in, gate,
    said_in,
};
use lamp_gemini::ToolCall;
use std::time::{Duration, Instant};

const OWNER: &str = "p-0123456789abcdef";
const SOMEONE: &str = "p-fedcba9876543210";
const PARTY_BACK: &str = "Create an event on your calendar: \"Hamburger party\", Sunday 15 November from 7 PM to 10 PM. Shall I add it?";
const AT_EIGHT_BACK: &str = "Create an event on your calendar: \"Hamburger party\", Sunday 15 November from 8 PM to 10 PM. Shall I add it?";
const FOLLOW_UP_BACK: &str = "Send an email from me@home.test to octavia@home.test. Subject: \"Re: Dinner\". It says: \"Sorry, dinner is at 8 PM, not 7.\". Only the people named get it.";

fn call(args: Value) -> ToolCall {
    ToolCall {
        id: "c1".into(),
        name: WRITE.into(),
        args,
    }
}

fn party() -> Value {
    json!({
        "service": "google_calendar",
        "action": "create_event",
        "title": "Hamburger party",
        "start": "2026-11-15T19:00:00+07:00",
        "end": "2026-11-15T22:00:00+07:00",
        "calendar": "primary",
        "attendees": ["octavia@home.test"],
        "text": "Bring buns",
    })
}

fn mail() -> Value {
    json!({
        "service": "gmail",
        "action": "send_mail",
        "to": ["octavia@home.test"],
        "cc": [],
        "subject": "Dinner",
        "text": "Dinner is at 7 PM.",
    })
}

/// A turn heard at the lamp; the words only name the case (the model
/// decides whether a line confirmed).
fn turn<'a>(_words: &'a str, _previous: u64, owner: Option<&'a str>) -> Turn<'a> {
    Turn {
        heard: true,
        owner,
        sends: true,
        alike: Some(0.9),
        seen_owner: owner.is_some(),
        ..Turn::default()
    }
}

fn heard(words: &str, previous: u64) -> Turn<'_> {
    turn(words, previous, Some(OWNER))
}

fn fresh(redo: bool) {
    on_for_test(true);
    super::on_for_test(redo);
    forgot(None);
}

/// `args` prepared in `request` as `send_id`, read back with `read_back`
/// and the read-back said whole: the waiting write.
fn read_back(
    args: &Value,
    request: u64,
    send_id: &str,
    read_back: &str,
    now: Instant,
) -> Option<Pending> {
    let mut pending = None;
    let mut told = json!({"sent": false, "send_id": send_id, "read_back": read_back});
    answered(
        &mut pending,
        &call(args.clone()),
        request,
        &mut told,
        now,
        OWNER,
    );
    said_in(&mut pending, request, read_back, now);
    began_in(&mut pending, now + Duration::from_secs(12));
    pending
}

/// The hamburger party read back in request 10, then a line in 11 the lamp
/// answered with no send, so a send in 12 is refused: cancelled, as on the
/// tester's lamp. Its key.
fn cancelled_party(now: Instant) -> String {
    let mut pending = read_back(&party(), 10, "s1", PARTY_BACK, now);
    said_in(&mut pending, 11, "Okay, I won't add it.", now);
    let refused = gate(
        &mut pending,
        &call(json!({"service":"google_calendar","send_id":"s1","said":"send"})),
        12,
        &heard("go ahead", 11),
        now + Duration::from_secs(13),
    )
    .expect_err("answered with no send");
    assert_eq!(refused["error"], NOTHING_WAITING);
    let record = only(Status::Cancelled);
    assert_eq!(record.detail["title"], "Hamburger party");
    record.key
}

/// `args` prepared and sent: its record `done`. Its key.
fn done(args: &Value, send_id: &str, read_back_said: &str, now: Instant) -> String {
    let _ = read_back(args, 20, send_id, read_back_said, now);
    keep_send(
        Some(7),
        Some(send_id),
        Got::Answered(&json!({"sent": true, "id": "x1"})),
    );
    only(Status::Done).key
}

fn only(status: Status) -> Record {
    let held = super::super::held();
    let found: Vec<_> = held
        .iter()
        .filter(|record| record.status == status)
        .collect();
    assert_eq!(found.len(), 1, "{held:?}");
    found[0].clone()
}

fn by_key(key: &str) -> Record {
    super::super::held()
        .into_iter()
        .find(|record| record.key == key)
        .unwrap()
}

/// "Do it again at 8 PM" after the cancelled party: every saved detail is
/// filled in, only the start changed, the record's own service and action
/// kept, and no send yet.
#[test]
fn a_redo_of_a_cancelled_event_fills_every_detail_and_applies_the_one_change() {
    fresh(true);
    let now = Instant::now();
    let key = cancelled_party(now);
    let mut pending = None;
    let redo = call(json!({
        "service": "google_calendar",
        "action": "create_event",
        FIELD: key,
        "start": "2026-11-15T20:00:00+07:00",
    }));
    let passed = gate(
        &mut pending,
        &redo,
        12,
        &heard("do it again at 8 PM", 11),
        now,
    )
    .expect("the owner's redo is prepared");
    let args = &passed.args;
    assert_eq!(args["service"], "google_calendar");
    assert_eq!(args["action"], "create_event");
    assert_eq!(args["title"], "Hamburger party");
    assert_eq!(args["start"], "2026-11-15T20:00:00+07:00");
    assert_eq!(args["end"], "2026-11-15T22:00:00+07:00");
    assert_eq!(args["calendar"], "primary");
    assert_eq!(args["attendees"], json!(["octavia@home.test"]));
    assert_eq!(args["text"], "Bring buns");
    assert!(args.get("send_id").is_none());
    assert!(args.get("said").is_none(), "a prepare, never a send");
    assert!(pending.is_none(), "nothing waits before the read-back");
}

/// The redo goes through the same read-back and send: a line the lamp
/// answered with no send still cancels it, and only a send after the whole
/// read-back sends it.
#[test]
fn a_redo_still_needs_its_read_back_and_a_send() {
    for sent in [false, true] {
        fresh(true);
        let now = Instant::now();
        let key = cancelled_party(now);
        let mut pending = None;
        let redo = call(json!({
            "service": "google_calendar",
            "action": "create_event",
            FIELD: key,
            "start": "2026-11-15T20:00:00+07:00",
        }));
        let passed = gate(
            &mut pending,
            &redo,
            12,
            &heard("do it again at 8 PM", 11),
            now,
        )
        .unwrap();
        let mut told = json!({"sent": false, "send_id": "s2", "read_back": AT_EIGHT_BACK});
        answered(&mut pending, &passed, 12, &mut told, now, OWNER);
        assert_eq!(told["next"], READ_BACK_NEXT);
        assert!(told.get("follow_up").is_none());
        // A send in a line begun before the read-back was said is refused.
        let send = call(json!({"service":"google_calendar","send_id":"s2","said":"send"}));
        let mut early = pending.clone();
        began_in(&mut early, now + Duration::from_secs(1));
        assert!(
            gate(
                &mut early,
                &send,
                13,
                &heard("send it", 12),
                now + Duration::from_secs(1)
            )
            .is_err()
        );
        said_in(&mut pending, 12, AT_EIGHT_BACK, now);
        began_in(&mut pending, now + Duration::from_secs(12));
        let request = if sent {
            13
        } else {
            said_in(&mut pending, 13, "Okay, I won't add it.", now);
            14
        };
        let got = gate(
            &mut pending,
            &send,
            request,
            &heard("go ahead and send it", request - 1),
            now + Duration::from_secs(13),
        );
        assert_eq!(got.is_ok(), sent, "{sent}: {got:?}");
        if sent {
            assert_eq!(got.unwrap().args["said"], "send");
        }
    }
}

/// The new record links back to the old one, the old one is marked
/// replaced by it, and both changes are written to the memory.
#[test]
fn the_new_record_links_back_and_the_old_one_is_marked_replaced() {
    fresh(true);
    let now = Instant::now();
    let old = cancelled_party(now);
    let mut pending = None;
    let redo = call(json!({
        "service": "google_calendar",
        "action": "create_event",
        FIELD: old,
        "start": "2026-11-15T20:00:00+07:00",
    }));
    let passed = gate(
        &mut pending,
        &redo,
        12,
        &heard("do it again at 8 PM", 11),
        now,
    )
    .unwrap();
    let mut told = json!({"sent": false, "send_id": "s2", "read_back": AT_EIGHT_BACK});
    answered(&mut pending, &passed, 12, &mut told, now, OWNER);
    let new = only(Status::Prepared);
    assert_eq!(new.links, vec![old.clone()]);
    assert!(
        new.detail.get(FIELD).is_none(),
        "the key is a link, not detail"
    );
    assert_eq!(new.detail["start"], "2026-11-15T20:00:00+07:00");
    assert_eq!(new.detail["title"], "Hamburger party");
    let replaced = by_key(&old);
    assert_eq!(replaced.status, Status::Cancelled, "a final status stays");
    assert_eq!(replaced.replaced_by, new.key);
    let lines: Vec<Value> = super::super::waiting().iter().map(Record::line).collect();
    assert!(
        lines
            .iter()
            .any(|line| line["key"] == new.key.as_str() && line["links"] == json!([old]))
    );
    assert!(
        lines
            .iter()
            .any(|line| line["key"] == old.as_str() && line["replaced_by"] == new.key.as_str())
    );
}

/// A failed, a timed-out and a refused write redo just as a cancelled one.
#[test]
fn a_failed_timed_out_or_refused_record_redoes() {
    for status in [Status::Failed, Status::TimedOut, Status::Refused] {
        fresh(true);
        let now = Instant::now();
        match status {
            Status::TimedOut => {
                let _ = read_back(&party(), 10, "s1", PARTY_BACK, now);
                expire(now + Duration::from_secs(121), Duration::from_secs(120));
            }
            _ => ended(
                Some(OWNER),
                WRITE,
                &party(),
                status,
                "the calendar said no",
                true,
            ),
        }
        let key = only(status).key;
        let mut pending = None;
        let redo = call(json!({
            "service": "google_calendar",
            "action": "create_event",
            FIELD: key,
        }));
        let passed = gate(&mut pending, &redo, 12, &heard("do it again", 11), now)
            .unwrap_or_else(|told| panic!("{status:?}: {told}"));
        assert_eq!(passed.args["title"], "Hamburger party", "{status:?}");
        assert_eq!(passed.args["start"], "2026-11-15T19:00:00+07:00");
        assert_eq!(passed.args["text"], "Bring buns");
    }
}

/// A sent mail is never changed: "change it" builds a follow-up to the
/// same people from it, `Re:` its subject, the new text the call's, and
/// the model is told to say plainly that a sent mail can't be changed.
/// The sent one's record keeps every word it had.
#[test]
fn a_done_mail_becomes_a_follow_up_and_is_never_edited() {
    fresh(true);
    let now = Instant::now();
    let sent = done(&mail(), "m1", "Send an email to octavia@home.test.", now);
    let before = by_key(&sent);
    let mut pending = None;
    // With no new text, the follow-up is asked for, nothing prepared.
    let bare = call(json!({"service":"gmail","action":"send_mail",FIELD: sent}));
    let told = gate(&mut pending, &bare, 21, &heard("change it", 20), now).unwrap_err();
    assert_eq!(told["error"], FOLLOW_UP_TEXT);
    let change = call(json!({
        "service": "gmail",
        "action": "send_mail",
        FIELD: sent,
        "text": "Sorry, dinner is at 8 PM, not 7.",
    }));
    let passed = gate(
        &mut pending,
        &change,
        21,
        &heard("change it to 8 PM", 20),
        now,
    )
    .unwrap();
    assert_eq!(passed.args["to"], json!(["octavia@home.test"]));
    assert_eq!(passed.args["subject"], "Re: Dinner");
    assert_eq!(passed.args["text"], "Sorry, dinner is at 8 PM, not 7.");
    assert!(passed.args.get("send_id").is_none(), "a new mail");
    let mut told = json!({"sent": false, "send_id": "m2", "read_back": FOLLOW_UP_BACK});
    answered(&mut pending, &passed, 21, &mut told, now, OWNER);
    assert_eq!(told["follow_up"], FOLLOW_UP_NOTE);
    assert!(FOLLOW_UP_NOTE.contains("a sent mail can't be changed"));
    let after = by_key(&sent);
    assert_eq!(after.status, Status::Done);
    assert_eq!(after.detail, before.detail, "the sent mail is never edited");
    assert_eq!(after.result_id, before.result_id);
    let follow_up = only(Status::Prepared);
    assert_eq!(after.replaced_by, follow_up.key);
    assert_eq!(follow_up.links, vec![sent]);
    // A subject the person gave is theirs.
    let mut pending = None;
    let named = call(json!({
        "service": "gmail", "action": "send_mail", FIELD: by_key(&follow_up.key).links[0],
        "text": "Sorry, 8 PM.", "subject": "Correction: dinner",
    }));
    let passed = gate(&mut pending, &named, 22, &heard("change it", 21), now).unwrap();
    assert_eq!(passed.args["subject"], "Correction: dinner");
}

/// A created event is not made again from its record: refused with a
/// note that moving or cancelling it is service_write's update_event or
/// delete_event action,
/// nothing prepared or linked.
#[test]
fn a_done_event_is_refused_here() {
    fresh(true);
    let now = Instant::now();
    let created = done(&party(), "s9", PARTY_BACK, now);
    let mut pending = None;
    let redo = call(json!({
        "service": "google_calendar",
        "action": "create_event",
        FIELD: created,
        "start": "2026-11-15T20:00:00+07:00",
    }));
    let told = gate(&mut pending, &redo, 21, &heard("make it again", 20), now).unwrap_err();
    assert_eq!(told["error"], EVENT_DONE);
    assert!(EVENT_DONE.contains(
        "call service_write with action update_event (or delete_event) and the record's event_id"
    ));
    // Never named as a function of its own (the 9adf fake writes).
    for told in [EVENT_DONE, DESCRIPTION] {
        for alone in [
            "call update_event",
            "call delete_event",
            "calling update_event",
        ] {
            assert!(!told.contains(alone), "{alone}: {told}");
        }
    }
    assert_eq!(super::super::held().len(), 1);
    assert!(by_key(&created).replaced_by.is_empty());
}

/// The tester's lamp, 11 Nov: a saved draft "changed" was saved again
/// beside it. A saved draft's record keeps its id in Drafts (the save's
/// `draft_id`) as its result id, for `update_draft` to name; a redo of it
/// is refused with that way, nothing prepared or linked.
#[test]
fn a_saved_draft_keeps_its_id_and_is_never_saved_again_from_its_record() {
    fresh(true);
    let now = Instant::now();
    let draft = json!({
        "service": "gmail",
        "action": "save_draft",
        "to": ["octavia@home.test"],
        "subject": "Party",
        "text": "Party on Saturday at 7.",
    });
    let back = "Save a draft to octavia@home.test, in the Gmail Drafts of me@home.test. Subject: \"Party\". It says: \"Party on Saturday at 7.\". Nothing is sent.";
    let _ = read_back(&draft, 20, "s8", back, now);
    keep_send(
        Some(8),
        Some("s8"),
        Got::Answered(
            &json!({"sent": false, "saved": true, "service": "Gmail", "draft_id": "draft12"}),
        ),
    );
    let saved = only(Status::Done);
    assert_eq!(saved.result_id, "draft12");
    assert_eq!(saved.reason, "saved as a draft");
    let mut pending = None;
    let redo = call(json!({
        "service": "gmail",
        "action": "save_draft",
        FIELD: saved.key,
        "text": "Party on Saturday at 7, at 12 Elm St. Wear something casual.",
    }));
    let told = gate(&mut pending, &redo, 21, &heard("add my address", 20), now).unwrap_err();
    assert_eq!(told["error"], DRAFT_DONE);
    assert!(DRAFT_DONE.contains("update_draft or delete_draft with its draft_id"));
    assert_eq!(super::super::held().len(), 1);
    assert!(by_key(&saved.key).replaced_by.is_empty());
}

/// Only the owner redoes: another line is refused by the services' owner
/// check, and another person's id finds none of the owner's records.
#[test]
fn someone_other_than_the_owner_is_refused() {
    fresh(true);
    let now = Instant::now();
    let key = cancelled_party(now);
    let redo = call(json!({
        "service": "google_calendar",
        "action": "create_event",
        FIELD: key,
        "start": "2026-11-15T20:00:00+07:00",
    }));
    let mut pending = None;
    let told = gate(
        &mut pending,
        &redo,
        12,
        &turn("do it again at 8 PM", 11, None),
        now,
    )
    .expect_err("not the owner");
    assert_eq!(told["error"], OWNER_ONLY);
    let mut args = redo.args.clone();
    assert_eq!(fill(SOMEONE, &mut args), Err(NO_RECORD));
    assert_eq!(args, redo.args);
    // Nor is the memory asked for anyone else's line.
    let mut asked = false;
    bring_with(None, &json!({FIELD: "v1-9"}), |_| {
        asked = true;
        None
    });
    assert!(!asked);
    let held = super::super::held();
    assert_eq!(held.len(), 1);
    assert!(held[0].replaced_by.is_empty());
}

/// Off (either switch), the call goes on exactly as asked, nothing filled
/// or linked, and `service_write` is offered as before.
#[test]
fn nothing_changes_with_the_switch_off() {
    for (records, redo_on) in [(true, false), (false, true)] {
        fresh(redo_on);
        on_for_test(true);
        let now = Instant::now();
        let key = cancelled_party(now);
        on_for_test(records);
        let redo = call(json!({
            "service": "google_calendar",
            "action": "create_event",
            FIELD: key,
            "start": "2026-11-15T20:00:00+07:00",
        }));
        let mut pending = None;
        let passed = gate(
            &mut pending,
            &redo,
            12,
            &heard("do it again at 8 PM", 11),
            now,
        )
        .unwrap();
        assert_eq!(passed.args, redo.args, "{records} {redo_on}");
        let mut told = json!({"sent": false, "send_id": "s2", "read_back": AT_EIGHT_BACK});
        answered(&mut pending, &passed, 12, &mut told, now, OWNER);
        assert!(told.get("follow_up").is_none());
        on_for_test(true);
        let held = super::super::held();
        assert!(held.iter().all(|record| record.links.is_empty()));
        assert!(held.iter().all(|record| record.replaced_by.is_empty()));
        on_for_test(records);
        let dir = std::env::temp_dir().join(format!(
            "lamp-services-redo-{}-{records}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        lamp_services::tools::write(&dir).unwrap();
        let offered = crate::services::tools(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        let write = offered.iter().find(|tool| tool.name == WRITE).unwrap();
        assert!(write.parameters.as_ref().unwrap()["properties"][FIELD].is_null());
    }
}

/// On, `service_write` offers the field, within the link's most.
#[test]
fn on_the_field_is_offered() {
    fresh(true);
    let dir = std::env::temp_dir().join(format!("lamp-services-redo-on-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    lamp_services::tools::write(&dir).unwrap();
    let offered = crate::services::tools(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
    let write = offered.iter().find(|tool| tool.name == WRITE).unwrap();
    let parameters = write.parameters.as_ref().unwrap();
    assert_eq!(parameters["properties"][FIELD]["description"], DESCRIPTION);
    assert!(parameters.to_string().len() <= lamp_gemini::MAX_TOOL_JSON_BYTES);
}

/// A record made before the voice started is read from the memory for the
/// owner's line, then redone the same way.
#[test]
fn a_record_from_before_a_restart_is_brought_from_the_memory() {
    fresh(true);
    let now = Instant::now();
    let row = json!({
        "key": "v1762763400000-3", "service": "google_calendar", "action": "create_event",
        "status": "cancelled", "reason": "the lamp answered after the read-back with no send", "detail": party(),
        "result_id": "", "asked_unix_ms": 1, "changed_unix_ms": 2, "links": [], "replaced_by": "",
    });
    let mut detail = row["detail"].clone();
    detail.as_object_mut().unwrap().remove("service");
    detail.as_object_mut().unwrap().remove("action");
    let mut row = row;
    row["detail"] = detail;
    let args = json!({"service":"google_calendar","action":"create_event",FIELD:"v1762763400000-3","start":"2026-11-15T20:00:00+07:00"});
    let mut asked = None;
    bring_with(Some(OWNER), &args, |line| {
        asked = Some(line.clone());
        Some(json!({"ok": true, "records": [row]}))
    });
    let asked = asked.expect("the memory is asked");
    assert_eq!(asked["op"], "service_records");
    assert_eq!(asked["person"], OWNER);
    let mut pending = None;
    let passed = gate(
        &mut pending,
        &call(args),
        12,
        &heard("do it again at 8 PM", 11),
        now,
    )
    .unwrap();
    assert_eq!(passed.args["title"], "Hamburger party");
    assert_eq!(passed.args["start"], "2026-11-15T20:00:00+07:00");
    // Held now: not asked again.
    let mut again = false;
    bring_with(Some(OWNER), &json!({FIELD:"v1762763400000-3"}), |_| {
        again = true;
        None
    });
    assert!(!again);
}

/// A replay of the tester's lamp, 10 Nov: the hamburger party read back,
/// a line answered with no send cancelled it, then "do it again at 8 PM" names only the
/// record and the new time. Everything else comes from the record, the
/// read-back is given and a send sends it, and the records link.
#[test]
fn the_ten_november_do_it_again_at_8_pm_needs_no_repeated_details() {
    fresh(true);
    let now = Instant::now();
    let key = cancelled_party(now);
    let mut pending = None;
    let redo = call(json!({
        "service": "google_calendar",
        "action": "create_event",
        FIELD: key,
        "start": "2026-11-15T20:00:00+07:00",
    }));
    let passed = gate(
        &mut pending,
        &redo,
        12,
        &heard("do it again at 8 PM", 11),
        now,
    )
    .unwrap();
    for (field, kept) in party().as_object().unwrap() {
        if field != "start" {
            assert_eq!(&passed.args[field], kept, "{field}");
        }
    }
    let mut told = json!({"sent": false, "send_id": "s2", "read_back": AT_EIGHT_BACK});
    answered(&mut pending, &passed, 12, &mut told, now, OWNER);
    said_in(&mut pending, 12, AT_EIGHT_BACK, now);
    began_in(&mut pending, now + Duration::from_secs(12));
    let sent = gate(
        &mut pending,
        &call(json!({"service":"google_calendar","send_id":"s2","said":"send"})),
        13,
        &heard("go ahead and send it", 12),
        now + Duration::from_secs(13),
    )
    .expect("a send after the read-back");
    keep_send(
        Some(8),
        Some("s2"),
        Got::Answered(&json!({"sent": true, "id": "ev1"})),
    );
    assert_eq!(sent.args["said"], "send");
    let made = only(Status::Done);
    assert_eq!(made.detail["start"], "2026-11-15T20:00:00+07:00");
    assert_eq!(made.links, vec![key.clone()]);
    assert_eq!(by_key(&key).replaced_by, made.key);
    assert_eq!(by_key(&key).status, Status::Cancelled);
}

/// lamp-06c1, 11 Nov (requests 131 and 137): "move it to 8 PM" and
/// "cancel it" after the lamp made the event. `from_record` with
/// update_event or delete_event names the event by the record's
/// `result_id`, Google's id: never the key. Only what the call gave is
/// laid on, the record's calendar kept, and it still needs its read-back
/// and a send.
#[test]
fn a_made_event_is_moved_or_cancelled_by_its_records_event_id() {
    for action in ["update_event", "delete_event"] {
        fresh(true);
        let now = Instant::now();
        let mut family = party();
        family["calendar"] = json!("family@group.calendar.google.com");
        let _ = read_back(&family, 20, "s9", PARTY_BACK, now);
        keep_send(
            Some(7),
            Some("s9"),
            Got::Answered(&json!({"sent": true, "event_id": "mbsiaoesblr6nec4tfh4flsbng"})),
        );
        let created = only(Status::Done).key;
        let mut asked = json!({
            "service": "google_calendar",
            "action": action,
            FIELD: created,
        });
        if action == "update_event" {
            asked["start"] = json!("2026-11-15T20:00:00+07:00");
            asked["end"] = json!("2026-11-15T22:00:00+07:00");
        }
        let mut pending = None;
        let passed = gate(
            &mut pending,
            &call(asked),
            21,
            &heard("move it to 8 PM", 20),
            now,
        )
        .expect("the owner's edit is prepared");
        let args = &passed.args;
        assert_eq!(args["action"], action, "{args}");
        assert_eq!(args["event_id"], "mbsiaoesblr6nec4tfh4flsbng", "{args}");
        assert_ne!(args["event_id"], json!(created));
        assert_eq!(args["calendar"], "family@group.calendar.google.com");
        // Nothing of the create's but its event and calendar.
        assert!(args.get("title").is_none(), "{args}");
        assert!(args.get("attendees").is_none(), "{args}");
        if action == "update_event" {
            assert_eq!(args["start"], "2026-11-15T20:00:00+07:00");
        } else {
            assert!(args.get("start").is_none(), "{args}");
        }
        assert!(args.get("said").is_none(), "a prepare, never a send");
        assert!(pending.is_none(), "nothing waits before the read-back");
    }
}

/// A made event's record with no `result_id` names no event: refused
/// with a plain reason, nothing prepared.
#[test]
fn a_made_event_with_no_event_id_is_refused() {
    fresh(true);
    let now = Instant::now();
    let created = done(&party(), "s9", PARTY_BACK, now);
    kept::ledger(|ledger| {
        for record in &mut ledger.records {
            record.result_id.clear();
        }
    });
    for action in ["update_event", "delete_event"] {
        let mut pending = None;
        let told = gate(
            &mut pending,
            &call(json!({"service":"google_calendar","action":action,FIELD: created})),
            21,
            &heard("cancel it", 20),
            now,
        )
        .unwrap_err();
        assert_eq!(told["error"], NO_EVENT_ID, "{action}");
        assert!(pending.is_none());
    }
    assert_eq!(super::super::held().len(), 1);
}
