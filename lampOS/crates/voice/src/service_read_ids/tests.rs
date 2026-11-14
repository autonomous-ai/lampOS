use super::*;

const ANN: &str = "p-0123456789abcdef";
const BOB: &str = "p-fedcba9876543210";
/// The event lamp-06c1 could not cancel on 12 Nov (row X1).
const EVENT_ID: &str = "kctlh9nnt7045gm342qucliu0k";

// The note stays under the records note's budget.
const _: () = assert!(NOTE_BYTES < crate::service_records::told::NOTE_BYTES);

fn read_args() -> Value {
    json!({"service": "google_calendar", "what": "events", "from": "2026-11-13T00:00:00+07:00"})
}

/// A calendar read's answer as `lamp-services` gives it: Google's events,
/// with text no one may keep beside what the edit needs.
fn read_told() -> Value {
    json!({"quoted": [
        {
            "id": EVENT_ID,
            "status": "confirmed",
            "summary": "lamp train test 2",
            "description": "Door code 4417, bring the blue folder",
            "location": "12 Secret Street",
            "attendees": [{"email": "octavia@b.test", "comment": "running late"}],
            "start": {"dateTime": "2026-11-13T21:00:00+07:00"},
            "end": {"dateTime": "2026-11-13T22:00:00+07:00"},
        },
        {
            "id": "dentist0001",
            "summary": "Dentist",
            "start": {"date": "2026-11-14"},
            "end": {"date": "2026-11-15"},
        },
    ], "note": crate::services::DATA_NOTE})
}

fn cancel(id: &str) -> Value {
    json!({"service": "google_calendar", "action": "delete_event", "event_id": id})
}

/// The read in one request, "yes, cancel it" in the next: told the id, and
/// the write naming it goes on, its calendar filled in.
#[test]
fn a_kept_event_is_told_and_its_write_goes_on_the_next_request() {
    on_for_test(true);
    let now = Instant::now();
    read_answered(Some(ANN), &read_args(), &read_told(), now);
    let later = now + Duration::from_secs(20);
    let note = note_for(Some(ANN), later).expect("the asker is told");
    assert!(note.contains(&format!("event_id {EVENT_ID}")), "{note}");
    assert!(note.contains("«lamp train test 2»"), "{note}");
    assert!(
        note.contains("«lamp train test 2» · Fri 13 Nov 21:00–22:00 · event_id"),
        "{note}"
    );
    assert!(
        note.contains("«Dentist» · Sat 14 Nov–Sun 15 Nov · event_id dentist0001"),
        "{note}"
    );
    assert!(note.contains("event_id dentist0001"), "{note}");
    assert!(note.contains(RULE), "{note}");
    assert!(note.contains(crate::services::DATA_NOTE), "{note}");
    assert!(!note.contains("Give calendar"), "{note}");
    let mut args = cancel(EVENT_ID);
    assert_eq!(check_write(Some(ANN), &mut args, later), Ok(()));
    assert_eq!(args["calendar"], "primary");
    assert_eq!(args["event_id"], EVENT_ID);
    // A calendar the call gave is kept as it gave it.
    let mut args = json!({"service": "Google Calendar", "action": "update_event",
        "event_id": EVENT_ID, "calendar": "work", "start": "x", "end": "y"});
    assert_eq!(check_write(Some(ANN), &mut args, later), Ok(()));
    assert_eq!(args["calendar"], "work");
    // A read of another calendar keeps it, and tells it.
    let mut other = read_args();
    other["calendar"] = json!("family123");
    read_answered(Some(ANN), &other, &read_told(), later);
    let note = note_for(Some(ANN), later).unwrap();
    assert!(note.contains("Give calendar family123 with it."), "{note}");
    let mut args = cancel("dentist0001");
    assert_eq!(check_write(Some(ANN), &mut args, later), Ok(()));
    assert_eq!(args["calendar"], "family123");
}

/// Another person's line is told none of them, and a write of theirs
/// naming one is refused; nobody known keeps nothing.
#[test]
fn another_persons_request_never_sees_or_uses_them() {
    on_for_test(true);
    let now = Instant::now();
    read_answered(Some(ANN), &read_args(), &read_told(), now);
    assert_eq!(note_for(Some(BOB), now), None);
    assert_eq!(note_for(None, now), None);
    let mut args = cancel(EVENT_ID);
    let refused = check_write(Some(BOB), &mut args, now).unwrap_err();
    assert_eq!(refused["error"], NOT_THEIRS);
    assert!(args.get("calendar").is_none());
    // Bob's own read is his; Ann's stays hers.
    read_answered(
        Some(BOB),
        &read_args(),
        &json!({"quoted": [
            {"id": "bobs0001", "summary": "Gym", "start": {"date": "2026-11-14"}}
        ]}),
        now,
    );
    assert!(note_for(Some(BOB), now).unwrap().contains("bobs0001"));
    assert!(!note_for(Some(BOB), now).unwrap().contains(EVENT_ID));
    assert!(!note_for(Some(ANN), now).unwrap().contains("bobs0001"));
    assert_eq!(
        check_write(Some(ANN), &mut cancel("bobs0001"), now).unwrap_err()["error"],
        NOT_THEIRS
    );
    assert_eq!(check_write(Some(ANN), &mut cancel(EVENT_ID), now), Ok(()));
    // Nobody known: nothing kept.
    forget();
    read_answered(None, &read_args(), &read_told(), now);
    read_answered(Some(""), &read_args(), &read_told(), now);
    assert!(kept::with(|kept| kept.is_empty()));
}

/// Past [`KEPT_FOR`], or once the conversation is closed: told nothing, and
/// a write naming one is told to read again; the titles and times are gone.
/// A forget drops all of it.
#[test]
fn an_expired_read_is_refused_with_read_again() {
    on_for_test(true);
    let now = Instant::now();
    read_answered(Some(ANN), &read_args(), &read_told(), now);
    let still = now + KEPT_FOR;
    assert!(note_for(Some(ANN), still).is_some());
    let late = still + Duration::from_secs(1);
    assert_eq!(note_for(Some(ANN), late), None);
    let refused = check_write(Some(ANN), &mut cancel(EVENT_ID), late).unwrap_err();
    assert_eq!(refused["error"], EXPIRED);
    assert!(EXPIRED.contains("service_read"), "it says to read again");
    kept::with(|kept| {
        for event in &kept[0].events {
            assert!(event.title.is_empty() && event.start.is_empty() && event.end.is_empty());
        }
    });
    // Closed: the same at once.
    read_answered(Some(ANN), &read_args(), &read_told(), now);
    assert!(note_for(Some(ANN), now).is_some());
    conversation_ended();
    assert_eq!(note_for(Some(ANN), now), None);
    assert_eq!(
        check_write(Some(ANN), &mut cancel(EVENT_ID), now).unwrap_err()["error"],
        EXPIRED
    );
    // Another read keeps afresh.
    read_answered(Some(ANN), &read_args(), &read_told(), now);
    assert_eq!(check_write(Some(ANN), &mut cancel(EVENT_ID), now), Ok(()));
    // A forget: nothing kept, the id goes on as any other would.
    forget();
    assert_eq!(note_for(Some(ANN), now), None);
    assert_eq!(check_write(Some(ANN), &mut cancel(EVENT_ID), now), Ok(()));
}

/// Off (the default): nothing is kept, told or refused, as before.
#[test]
fn off_keeps_tells_and_refuses_nothing() {
    on_for_test(false);
    let now = Instant::now();
    read_answered(Some(ANN), &read_args(), &read_told(), now);
    assert!(kept::with(|kept| kept.is_empty()));
    assert_eq!(note_for(Some(ANN), now), None);
    let mut args = cancel(EVENT_ID);
    assert_eq!(check_write(Some(BOB), &mut args, now), Ok(()));
    assert_eq!(args, cancel(EVENT_ID));
}

/// Only the id, calendar, title and start and end of each event are kept:
/// never a description, a place, a guest or anything else the read said;
/// at most [`KEPT_MOST`]; and only a calendar `events` read with no error.
#[test]
fn only_the_id_title_and_time_are_kept() {
    on_for_test(true);
    let now = Instant::now();
    read_answered(Some(ANN), &read_args(), &read_told(), now);
    let held = kept::with(|kept| kept[0].events.clone());
    assert_eq!(
        held[0],
        Event {
            id: EVENT_ID.into(),
            calendar: "primary".into(),
            title: "lamp train test 2".into(),
            start: "2026-11-13T21:00:00+07:00".into(),
            end: "2026-11-13T22:00:00+07:00".into(),
        }
    );
    assert_eq!(held[1].start, "2026-11-14");
    // A title with a code's shape is never kept; its event is, by its id.
    let coded = events_of(
        &json!({"quoted": [{"id": "code0001", "summary": "Your code is 482913"}]}),
        "primary",
    );
    assert_eq!(coded[0].id, "code0001");
    assert!(coded[0].title.is_empty(), "{coded:?}");
    let all = format!("{held:?}");
    for secret in [
        "Door code",
        "4417",
        "Secret Street",
        "octavia",
        "running late",
    ] {
        assert!(!all.contains(secret), "{secret} kept: {all}");
    }
    let note = note_for(Some(ANN), now).unwrap();
    for secret in [
        "Door code",
        "4417",
        "Secret Street",
        "octavia",
        "running late",
    ] {
        assert!(!note.contains(secret), "{secret} told: {note}");
    }
    // Ten found: the first five kept. An item with no id, or an id that is
    // no id in a path, is not.
    let many: Vec<Value> = (0..10)
        .map(|n| json!({"id": format!("ev{n}"), "summary": format!("E{n}")}))
        .chain([json!({"summary": "no id"}), json!({"id": "a/b"})])
        .collect();
    let events = events_of(&json!({"quoted": many}), "primary");
    let ids: Vec<&str> = events.iter().map(|event| event.id.as_str()).collect();
    assert_eq!(ids, ["ev0", "ev1", "ev2", "ev3", "ev4"]);
    assert_eq!(KEPT_MOST, 5);
    // A Gmail read, a calendar read that failed, or another calendar read
    // keeps nothing and leaves what was kept.
    forget();
    for (args, told) in [
        (json!({"service": "gmail", "what": "recent"}), read_told()),
        (
            read_args(),
            json!({"error": "busy", "quoted": read_told()["quoted"]}),
        ),
        (
            json!({"service": "google_calendar", "what": "get", "path": "x"}),
            read_told(),
        ),
    ] {
        read_answered(Some(ANN), &args, &told, now);
    }
    assert!(kept::with(|kept| kept.is_empty()));
    // A read that found nothing drops what was kept: no stale ids.
    read_answered(Some(ANN), &read_args(), &read_told(), now);
    read_answered(Some(ANN), &read_args(), &json!({"quoted": []}), now);
    assert_eq!(note_for(Some(ANN), now), None);
}

/// The note fits [`NOTE_BYTES`] with five events with long titles in
/// Vietnamese, Google-length ids and another calendar; with ids at
/// Google's 1,024-byte bound, the lines that do not fit are left out, and
/// it still fits.
#[test]
fn the_note_fits_its_bytes() {
    on_for_test(true);
    let now = Instant::now();
    let title = "Họp với đội thiết kế về đèn mới và lịch thử nghiệm tuần sau nữa";
    let items: Vec<Value> = (0..KEPT_MOST)
        .map(|n| {
            json!({"id": format!("{n}{}", &"kctlh9nnt7045gm342qucliu0k_20261113T140000Z"[1..]),
                "summary": title,
                "start": {"dateTime": "2026-11-13T21:00:00+07:00"},
                "end": {"dateTime": "2026-11-13T22:00:00+07:00"}})
        })
        .collect();
    let mut args = read_args();
    args["calendar"] = json!("c_0123456789abcdef0123456789abcdef@group.calendar.google.com");
    read_answered(Some(ANN), &args, &json!({"quoted": items}), now);
    let note = note_for(Some(ANN), now).unwrap();
    assert!(note.len() <= NOTE_BYTES, "{} bytes: {note}", note.len());
    assert_eq!(
        note.matches("\n- ").count(),
        KEPT_MOST,
        "all five told: {note}"
    );
    let huge: Vec<Value> = (0..KEPT_MOST)
        .map(|n| json!({"id": format!("{n}{}", "x".repeat(1_023)), "summary": "Long"}))
        .collect();
    read_answered(Some(ANN), &read_args(), &json!({"quoted": huge}), now);
    assert_eq!(note_for(Some(ANN), now), None, "no line fits, none told");
    let mut mixed: Vec<Value> = vec![json!({"id": "short01", "summary": "Short"})];
    mixed.push(json!({"id": "y".repeat(1_024), "summary": "Long"}));
    read_answered(Some(ANN), &read_args(), &json!({"quoted": mixed}), now);
    let note = note_for(Some(ANN), now).unwrap();
    assert!(note.len() <= NOTE_BYTES);
    assert!(note.contains("short01") && !note.contains("yyyy"), "{note}");
}
