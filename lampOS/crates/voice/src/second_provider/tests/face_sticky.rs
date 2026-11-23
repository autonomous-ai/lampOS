//! The sticky identity plan (`docs/face-recognition-sticky-identity-plan.md`):
//! what a line from a name the eyes hold (`LAMP_FACE_STICKY`), or one held
//! with nobody in view (`LAMP_FACE_NO_FACE_HOLD`), may run.
use super::*;
use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};

fn key(request: u64) -> RequestKey {
    RequestKey {
        worker: lamp_turn_owner::BootId::new([61; 16]).unwrap(),
        session: 1,
        request,
    }
}

fn call(name: &str, args: Value) -> net::ToolCall {
    net::ToolCall {
        id: "c1".to_owned(),
        name: name.to_owned(),
        args,
        extra: None,
    }
}

/// (d): on a line from a held name, a forget, `forget_everything`, a face's
/// forget, a correction and every rule call are refused, the single-item
/// yes (#5627) included, by every runner; a plain keep runs. The lamp is
/// asked no question about it. The same line matched runs them as before.
#[test]
fn a_held_name_runs_no_forget_correction_or_rule() {
    let held = RequestScope::new(key(1), ScopeSnapshot::sticky_of("A"));
    let lease = held.lease();
    lease.mark_forget_agreed(&OneFaceForgetYes(()), 9);
    for (name, args) in [
        (crate::remember::FORGET, json!({"source_id": 9})),
        (crate::remember::FORGET, json!({"about": "the coffee"})),
        (crate::remember::FORGET_EVERYTHING, json!({})),
        (crate::known_faces::FORGET, json!({})),
        (crate::rules::KEEP_RULE, json!({"rule": "no news"})),
        (crate::rules::FORGET_RULE, json!({"id": 1})),
        (crate::rules::LIST_RULES, json!({})),
        (
            crate::remember::REMEMBER,
            json!({"text": "blue now", "replaces": 4}),
        ),
    ] {
        assert!(
            one_face_refused(Some(&lease), &call(name, args.clone())),
            "{name} {args}"
        );
    }
    assert!(!one_face_refused(
        Some(&lease),
        &call(crate::remember::REMEMBER, json!({"text": "I like tea"}))
    ));
    // Matched, not held: the agreed forget and a correction run as before.
    let seen = RequestScope::new(key(2), ScopeSnapshot::of("A"));
    let seen = seen.lease();
    for (name, args) in [
        (crate::remember::FORGET, json!({"source_id": 9})),
        (
            crate::remember::REMEMBER,
            json!({"text": "blue now", "replaces": 4}),
        ),
    ] {
        assert!(!one_face_refused(Some(&seen), &call(name, args)));
    }
}

/// A fresh observation of `name`, as the refresh reads the eyes' known file.
fn observation(name: &str) -> crate::known_faces::FaceObservation {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "face-sticky-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, name).unwrap();
    let observation = crate::known_faces::FaceObservation::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    observation
}

/// The plan's T1 (`LAMP_FACE_NO_FACE_HOLD`, the owner's call on #7933,
/// 20 Nov 01:55, overruling #7814's held-only condition): the person matched
/// and answered, then nobody in view for 25 s: a line reads, keeps and is
/// journalled as theirs, but every forget, correction and rule is refused
/// (d); `LAMP_FACE_GAP_HOLD`'s 12 s hold alone stays told-only, as before.
#[test]
fn with_the_no_face_hold_a_line_with_nobody_in_view_reads_and_keeps_but_forgets_nothing() {
    use crate::memory_scope::{FaceCache, Gap};
    let a = crate::remember::person_id("A");
    let cache = FaceCache::holding(Some(Gap::no_face()));
    let seen = observation("A");
    let until = seen.valid_until();
    assert!(cache.replace_seen(cache.begin_refresh(), Some(seen), Some(1)));
    cache.answered(&ScopeSnapshot::of("A"), until);
    assert!(cache.replace_seen(cache.begin_refresh(), None, Some(0)));
    let scope = cache.snapshot(until + std::time::Duration::from_secs(25));
    assert_eq!(scope.person(), Some(a.as_str()));
    let line = RequestScope::new(key(5), scope);
    let lease = line.lease();
    for name in [crate::remember::RECALL, crate::remember::REMEMBER] {
        assert!(!memory_refused(Some(&lease), name), "{name}");
    }
    assert!(withheld(Some(&lease)).is_empty());
    assert!(!held_by_gap(Some(&lease)));
    assert!(
        !worker::keep_out_scoped(&lease, false),
        "journalled as theirs"
    );
    for (name, args) in [
        (crate::remember::FORGET, json!({"about": "the coffee"})),
        (crate::remember::FORGET_EVERYTHING, json!({})),
        (crate::rules::KEEP_RULE, json!({"rule": "no news"})),
        (
            crate::remember::REMEMBER,
            json!({"text": "blue now", "replaces": 4}),
        ),
    ] {
        assert!(
            one_face_refused(Some(&lease), &call(name, args.clone())),
            "{name} {args}"
        );
    }
    // 30 s with no face: the room's (b).
    assert_eq!(
        cache
            .snapshot(until + crate::memory_scope::NO_FACE_SEEN)
            .person(),
        None
    );
    // The 12 s gap hold alone: held, told only, as before.
    let held = RequestScope::new(key(6), ScopeSnapshot::held_of("A"));
    let lease = held.lease();
    for name in [crate::remember::RECALL, crate::remember::REMEMBER] {
        assert!(memory_refused(Some(&lease), name), "{name}");
    }
    assert_eq!(withheld(Some(&lease)), MEMORY_CALLS);
}

/// T9 (`LAMP_CARRIED_JOURNAL`, its own switch, Friday's P2 on #7933): a
/// carried line read as its person's (their own face the one face in view
/// through it, `LAMP_CONVERSATION_CARRIED_MEMORY`) is journalled in their
/// record, and the journal says so with counts only; off, kept out as
/// before. A held line is kept out either way.
#[test]
fn with_the_carried_journal_a_carried_line_read_as_theirs_is_journalled() {
    let a = crate::remember::person_id("A");
    let line = worker::carried_journalled_line(7, 3);
    assert_eq!(
        line,
        "lamp-voice second: request 7: a carried line read as its person's is journalled in their record (3 so far, LAMP_CARRIED_JOURNAL)"
    );
    for (request, sticky) in [(7_814_001, true), (7_814_002, false)] {
        let carried = RequestScope::new(key(request), ScopeSnapshot::room());
        let lease = carried.lease();
        assert!(lease.carry_conversation(&a));
        assert!(lease.read_carried());
        assert!(lease.reads_carried());
        assert_eq!(worker::keep_out_scoped(&lease, sticky), !sticky);
        assert_eq!(crate::remember::kept_out(request), !sticky, "{sticky}");
    }
    let held = RequestScope::new(key(7_814_003), ScopeSnapshot::held_of("A"));
    assert!(worker::keep_out_scoped(&held.lease(), true));
    assert!(crate::remember::kept_out(7_814_003));
}

/// #7814: a carried line journalled as its person's is logged once per
/// request, not once per output event: one answer's many events count 1, a
/// second request counts 2.
#[test]
fn a_carried_line_journalled_as_theirs_is_logged_once_per_request() {
    use std::sync::atomic::AtomicU64;
    let last = AtomicU64::new(u64::MAX);
    let count = AtomicU64::new(0);
    let logged: Vec<u64> = (0..49)
        .filter_map(|_| worker::carried_journalled_once(&last, &count, 7))
        .collect();
    assert_eq!(logged, [1]);
    assert_eq!(worker::carried_journalled_once(&last, &count, 8), Some(2));
    assert_eq!(worker::carried_journalled_once(&last, &count, 8), None);
    assert!(!worker::first_for_request(&last, 8));
    assert!(worker::first_for_request(&last, 9));
}

/// Friday's P1 on #7933 (20 Nov 03:16): the eyes write a T7 rematch (0.31
/// within ten minutes of a match) with the held mark, as a held check. On
/// the voice, its line runs no forget, and the owner's private read-back
/// sees no owner in view, so `owner_sees_alone_now` is false; a real match
/// (no mark) is the owner in view, as before.
#[test]
fn a_rematch_written_held_runs_no_forget_and_is_never_the_owner_alone() {
    let directory = std::env::temp_dir().join(format!("rematch-held-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let socket = directory.join("who.sock");
    let known = crate::known_faces::known_file_of(&socket);
    let owners = [crate::known_faces::memory_person("A")];
    // The eyes' rematch: the name and the mark.
    std::fs::write(&known, "A\n").unwrap();
    std::fs::write(crate::known_faces::known_held_of(&known), b"").unwrap();
    let held = crate::known_faces::known_for_owner_checks_in(true, Some(&socket));
    assert_eq!(held, None, "held: no known name for the owner's checks");
    let in_view = crate::services::owner_known(held, &owners);
    assert!(!crate::services::owner_sees_alone(false, in_view, || true));
    // Its line, held: a forget is refused.
    let line = RequestScope::new(key(9), ScopeSnapshot::sticky_of("A"));
    let lease = line.lease();
    assert!(one_face_refused(
        Some(&lease),
        &call(crate::remember::FORGET, json!({"about": "the coffee"}))
    ));
    // A real match: the eyes clear the mark; the owner in view, as before.
    std::fs::remove_file(crate::known_faces::known_held_of(&known)).unwrap();
    let matched = crate::known_faces::known_for_owner_checks_in(true, Some(&socket));
    assert!(crate::services::owner_known(matched, &owners));
    std::fs::remove_dir_all(&directory).unwrap();
}
