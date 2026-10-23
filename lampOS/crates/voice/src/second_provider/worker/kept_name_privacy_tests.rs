//! Real callback and memory RPC controls for request-owned private disclosure.
use super::*;
use crate::memory_scope::ScopeLease;

fn callback_telling(
    memory: &Memory,
    tell: impl Fn(Instant, Option<String>, String, Option<&ScopeLease>, bool) -> Told + Send + Sync,
) -> impl Fn(Option<&ScopeLease>) -> Told + Send + Sync {
    let sessions = Arc::new(Mutex::new(Sessions::default()));
    let known = memory.directory.join("known");
    let ahead =
        super::super::Ahead::reading(sessions.clone(), Some(known.clone()), memory.socket.clone());
    let context = memory_context(
        sessions,
        Some(memory.socket.clone()),
        Some(known),
        true,
        tell,
    );
    move |lease| {
        // Same lifecycle as the production Engine: Start admission, then its
        // off-loop final ready/read, then actual context/history composition.
        // This synchronous wrapper runs only on the test's calling thread.
        ahead.begin(lease, Instant::now());
        runtime().block_on(ahead.clone().ready(lease.cloned()));
        context(lease)
    }
}

fn callback(memory: &Memory, steady: bool) -> impl Fn(Option<&ScopeLease>) -> Told + Send + Sync {
    callback_telling(memory, move |_, person, note, _, history| {
        disclosure_message(
            steady,
            "Synthetic character and skills.",
            Some(" Synthetic time and facts."),
            &note,
            None,
            person.as_deref(),
            history,
        )
    })
}

fn history(person: Option<String>, marker: &str) {
    crate::thread::current(None, |_| ());
    let now = Instant::now();
    let mut thread = crate::thread::Thread::new();
    thread.at_lamp(person);
    thread.person(marker, true, now);
    thread.lamp(1, "Synthetic reply.", now);
    *crate::thread::THREAD.lock().unwrap() = thread;
}

fn proof(request: u64, faces: &[&str], until: Instant) -> RequestScope {
    let mut owner = unproved_scope(request, ScopeSnapshot::of(OWNER));
    let key = owner.lease().key();
    let _ = owner.speaker_recorder().unwrap().seal(
        key,
        &faces.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        until,
    );
    owner
}

#[test]
fn prepared_private_context_is_neutral_and_final_proof_discloses_both_layouts() {
    if !child(
        "privacy::prepared_private_context_is_neutral_and_final_proof_discloses_both_layouts",
        "privacy",
        0,
    ) {
        return;
    }
    let person = crate::remember::person_id(OWNER);
    let memory = Memory::new(Some(person.clone()));
    crate::remember::start(memory.socket.clone(), None);
    for steady in [false, true] {
        let callback = callback(&memory, steady);
        history(Some(person.clone()), "PRIVATE_HISTORY_MARKER");
        let mut owner = unproved_scope(8, ScopeSnapshot::of(OWNER));
        let lease = owner.lease();
        let before = memory.count("session_open");
        let early = callback(Some(&lease));
        assert!(early.history.is_empty());
        assert!(!early.requires_speaker);
        let early_payload = payload(early.clone());
        assert!(early_payload.contains("Synthetic character and skills."));
        assert!(early_payload.contains("Synthetic time and facts."));
        assert!(!early_payload.contains(OLD) && !early_payload.contains("PRIVATE_HISTORY_MARKER"));
        assert_eq!(
            memory.count("session_open"),
            before,
            "preparation does no memory I/O"
        );
        let key = lease.key();
        assert!(owner.speaker_recorder().unwrap().seal(
            key,
            &["0123456789abcdef/f7".into()],
            Instant::now() + Duration::from_secs(10)
        ));
        let final_told = callback(Some(&lease));
        assert!(!final_told.history.is_empty());
        assert!(final_told.requires_speaker);
        let final_payload = payload(final_told.clone());
        assert!(final_payload.contains(OLD) && final_payload.contains("PRIVATE_HISTORY_MARKER"));
        assert_eq!(final_told.after.is_some(), steady);
        assert_ne!(
            early.messages(),
            final_told.messages(),
            "final messages invalidate a neutral prepared chat"
        );
        lease.narrow_to_room();
        history(None, "ROOM_HISTORY_MARKER");
        let narrowed = payload(callback(Some(&lease)));
        assert!(!narrowed.contains(OLD) && !narrowed.contains("ROOM_HISTORY_MARKER"));
        owner.revoke();
        assert_eq!(callback(Some(&lease)), Told::default());
    }
}

#[test]
fn unproved_mismatched_expired_and_legacy_private_requests_never_read_memory_or_history() {
    if !child(
        "privacy::unproved_mismatched_expired_and_legacy_private_requests_never_read_memory_or_history",
        "privacy",
        0,
    ) {
        return;
    }
    let memory = Memory::new(Some(crate::remember::person_id(OWNER)));
    crate::remember::start(memory.socket.clone(), None);
    let callback = callback(&memory, true);
    history(
        Some(crate::remember::person_id(OWNER)),
        "PRIVATE_HISTORY_MARKER",
    );
    let until = Instant::now() + Duration::from_secs(10);
    for (index, faces) in [
        vec![],
        vec!["0123456789abcdef/f7", "0123456789abcdef/f8"],
        vec!["0123456789abcdef/f8"],
        vec!["fedcba9876543210/f7"],
    ]
    .into_iter()
    .enumerate()
    {
        let owner = proof(index as u64 + 1, &faces, until);
        let told = callback(Some(&owner.lease()));
        assert!(told.history.is_empty());
        assert!(!payload(told).contains(OLD));
    }
    let expired = proof(10, &["0123456789abcdef/f7"], Instant::now());
    assert!(!payload(callback(Some(&expired.lease()))).contains(OLD));
    let valid = scope(11, ScopeSnapshot::of(OWNER));
    write_known(
        &memory.directory,
        "Another synthetic owner",
        "0123456789abcdef",
        "f7",
    );
    assert!(!payload(callback(Some(&valid.lease()))).contains(OLD));
    write_known(&memory.directory, OWNER, "0123456789abcdef", "f7");
    let known = crate::speaking_faces::known_speaker_of(&memory.directory.join("known"));
    std::fs::File::open(&known)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - Duration::from_secs(4))
        .unwrap();
    assert!(!payload(callback(Some(&valid.lease()))).contains(OLD));
    std::fs::remove_file(&known).unwrap();
    assert!(!payload(callback(Some(&valid.lease()))).contains(OLD));
    assert!(
        !payload(callback(None)).contains(OLD),
        "legacy sticky person is not speaker evidence"
    );
    assert_eq!(memory.count("session_open"), 0);
    assert_eq!(memory.count("kept"), 0);
    let held = unproved_scope(12, memory.held());
    assert_eq!(callback(Some(&held.lease())), Told::default());
}

#[test]
fn genuine_room_context_keeps_room_notes_and_history_without_speaker_proof() {
    if !child(
        "privacy::genuine_room_context_keeps_room_notes_and_history_without_speaker_proof",
        "privacy",
        0,
    ) {
        return;
    }
    let memory = Memory::new(None);
    crate::remember::start(memory.socket.clone(), None);
    history(None, "ROOM_HISTORY_MARKER");
    for steady in [false, true] {
        let callback = callback(&memory, steady);
        let room = unproved_scope(3, ScopeSnapshot::room());
        let told = callback(Some(&room.lease()));
        assert!(!told.requires_speaker);
        let text = payload(told);
        assert!(text.contains(OLD) && text.contains("ROOM_HISTORY_MARKER"));
    }
    let hits = super::super::memory_hits(
        Arc::new(Mutex::new(Sessions::default())),
        memory.socket.clone(),
        Some(memory.directory.join("known")),
    );
    let room = unproved_scope(3, ScopeSnapshot::room());
    assert!(
        hits(3, "synthetic name", Some(&room.lease()))
            .unwrap()
            .contains(OLD)
    );
    assert!(
        memory
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .all(|r| r["person"].is_null())
    );
}

#[test]
fn slow_note_read_rechecks_revocation_expiry_and_fresh_known_identity() {
    if !child(
        "privacy::slow_note_read_rechecks_revocation_expiry_and_fresh_known_identity",
        "privacy",
        0,
    ) {
        return;
    }
    let memory = Memory::new(Some(crate::remember::person_id(OWNER)));
    crate::remember::start(memory.socket.clone(), None);
    for kind in ["revoked", "expired", "other-boot", "wrong-person", "stale"] {
        write_known(&memory.directory, OWNER, "0123456789abcdef", "f7");
        let owner = Arc::new(proof(
            9,
            &["0123456789abcdef/f7"],
            Instant::now()
                + if kind == "expired" {
                    Duration::from_millis(100)
                } else {
                    Duration::from_secs(10)
                },
        ));
        let revoke = owner.clone();
        let directory = memory.directory.clone();
        *memory.before_reply.lock().unwrap() = Some(Arc::new(move |request| {
            if request["op"] != "session_open" {
                return;
            }
            match kind {
                "revoked" => revoke.revoke(),
                "expired" => std::thread::sleep(Duration::from_millis(110)),
                "other-boot" => write_known(&directory, OWNER, "fedcba9876543210", "f7"),
                "wrong-person" => write_known(
                    &directory,
                    "Another synthetic owner",
                    "0123456789abcdef",
                    "f7",
                ),
                "stale" => std::fs::File::open(crate::speaking_faces::known_speaker_of(
                    &directory.join("known"),
                ))
                .unwrap()
                .set_modified(std::time::SystemTime::now() - Duration::from_secs(4))
                .unwrap(),
                _ => unreachable!(),
            }
        }));
        let callback = callback(&memory, false);
        let told = callback(Some(&owner.lease()));
        if kind == "expired" {
            // A loaded host may expire before entry rather than in the RPC.
            // Either way neither private memory nor history may escape.
            assert!(told.history.is_empty());
            assert!(!payload(told).contains(OLD));
        } else {
            assert_eq!(told, Told::default(), "{kind}");
        }
        *memory.before_reply.lock().unwrap() = None;
    }
    // A cached note must not bypass a later proof check.
    write_known(&memory.directory, OWNER, "0123456789abcdef", "f7");
    let owner = scope(10, ScopeSnapshot::of(OWNER));
    let callback = callback(&memory, false);
    assert!(payload(callback(Some(&owner.lease()))).contains(OLD));
    let reads = memory.count("session_open");
    write_known(&memory.directory, OWNER, "fedcba9876543210", "f7");
    assert!(!payload(callback(Some(&owner.lease()))).contains(OLD));
    assert_eq!(memory.count("session_open"), reads);
}

#[test]
fn actual_hits_rpc_requires_positive_current_matching_proof_and_rechecks_its_reply() {
    if !child(
        "privacy::actual_hits_rpc_requires_positive_current_matching_proof_and_rechecks_its_reply",
        "privacy",
        0,
    ) {
        return;
    }
    let memory = Memory::new(Some(crate::remember::person_id(OWNER)));
    crate::remember::start(memory.socket.clone(), None);
    let hits = super::super::memory_hits(
        Arc::new(Mutex::new(Sessions::default())),
        memory.socket.clone(),
        Some(memory.directory.join("known")),
    );
    let unproved = unproved_scope(1, ScopeSnapshot::of(OWNER));
    assert_eq!(hits(1, "synthetic name", Some(&unproved.lease())), None);
    assert_eq!(hits(1, "synthetic name", None), None);
    assert_eq!(memory.count("kept"), 0);
    let owner = Arc::new(scope(2, ScopeSnapshot::of(OWNER)));
    assert!(
        hits(2, "synthetic name", Some(&owner.lease()))
            .unwrap()
            .contains(OLD)
    );
    assert_eq!(hits(999, "synthetic name", Some(&owner.lease())), None);
    let reads = memory.count("kept");
    let revoke = owner.clone();
    *memory.before_reply.lock().unwrap() = Some(Arc::new(move |_| revoke.revoke()));
    assert_eq!(hits(2, "synthetic name", Some(&owner.lease())), None);
    assert_eq!(
        memory.count("kept"),
        reads + 1,
        "a real response was suppressed after revocation"
    );
    *memory.before_reply.lock().unwrap() = None;
    for kind in ["expired", "other-boot", "wrong-person", "stale"] {
        write_known(&memory.directory, OWNER, "0123456789abcdef", "f7");
        let owner = proof(
            4,
            &["0123456789abcdef/f7"],
            Instant::now()
                + if kind == "expired" {
                    Duration::from_millis(15)
                } else {
                    Duration::from_secs(10)
                },
        );
        let directory = memory.directory.clone();
        *memory.before_reply.lock().unwrap() = Some(Arc::new(move |_| match kind {
            "expired" => std::thread::sleep(Duration::from_millis(20)),
            "other-boot" => write_known(&directory, OWNER, "fedcba9876543210", "f7"),
            "wrong-person" => write_known(
                &directory,
                "Another synthetic owner",
                "0123456789abcdef",
                "f7",
            ),
            "stale" => std::fs::File::open(crate::speaking_faces::known_speaker_of(
                &directory.join("known"),
            ))
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(4))
            .unwrap(),
            _ => unreachable!(),
        }));
        assert_eq!(
            hits(4, "synthetic name", Some(&owner.lease())),
            None,
            "{kind}"
        );
        *memory.before_reply.lock().unwrap() = None;
    }
    write_known(&memory.directory, OWNER, "0123456789abcdef", "f7");
    let reads = memory.count("kept");
    let narrowed = scope(3, ScopeSnapshot::of(OWNER));
    narrowed.lease().narrow_to_room();
    assert_eq!(hits(3, "synthetic name", Some(&narrowed.lease())), None);
    assert_eq!(memory.count("kept"), reads);
}

#[test]
fn identity_changed_during_history_composition_discards_the_private_told() {
    if !child(
        "privacy::identity_changed_during_history_composition_discards_the_private_told",
        "privacy",
        0,
    ) {
        return;
    }
    let person = crate::remember::person_id(OWNER);
    let memory = Memory::new(Some(person.clone()));
    crate::remember::start(memory.socket.clone(), None);
    history(Some(person), "PRIVATE_HISTORY_MARKER");
    let directory = memory.directory.clone();
    let callback = callback_telling(&memory, move |_, person, note, _, allow_history| {
        let told = disclosure_message(
            false,
            "Synthetic character.",
            None,
            &note,
            None,
            person.as_deref(),
            allow_history,
        );
        let text = payload(told.clone());
        assert!(text.contains(OLD) && text.contains("PRIVATE_HISTORY_MARKER"));
        // Invalidate only the atomic observation after actual composition;
        // the lease itself remains current with its original positive proof.
        write_known(&directory, OWNER, "fedcba9876543210", "f7");
        told
    });
    let owner = scope(9, ScopeSnapshot::of(OWNER));
    assert_eq!(callback(Some(&owner.lease())), Told::default());
    assert!(owner.lease().speaker().is_some());
    assert_eq!(memory.count("session_open"), 2);
}
