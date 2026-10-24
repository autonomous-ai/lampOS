//! Real callback and memory RPC controls for request-owned private disclosure.
use super::*;
use crate::memory_scope::ScopeLease;
use crate::second_provider::worker::{known_name_told, with_typed_name_for_lease};

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
        context(lease, super::super::Chosen::ByLease)
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

/// A proof for A never labels sticky B's data as A's, or as Room data.
#[test]
fn by_presence_context_never_adopts_a_different_sticky_person() {
    if !child(
        "privacy::by_presence_context_never_adopts_a_different_sticky_person",
        "privacy",
        0,
    ) {
        return;
    }
    let a = crate::remember::person_id(OWNER);
    let b = crate::remember::person_id("Other fixture owner");
    let memory = Memory::new(Some(a.clone()));
    let known = memory.directory.join("known");
    let sessions = Arc::new(Mutex::new(Sessions::default()));
    sessions
        .lock()
        .unwrap()
        .presence
        .seen(Some(b.clone()), Instant::now());
    sessions.lock().unwrap().note = Some("STICKY_B_NOTE".into());
    history(Some(b), "STICKY_B_HISTORY");
    // The atomic source identifies A, while the legacy presence is still B.
    std::fs::remove_file(&known).unwrap();
    let context = memory_context(
        sessions.clone(),
        Some(memory.socket.clone()),
        Some(known.clone()),
        true,
        |_, person, note, _, allowed| {
            disclosure_message(
                false,
                "Public character.",
                None,
                &note,
                None,
                person.as_deref(),
                allowed,
            )
        },
    );
    let owner = proof(
        71,
        &["0123456789abcdef/f7"],
        Instant::now() + Duration::from_secs(10),
    );
    let room = unproved_scope(72, ScopeSnapshot::room());
    for lease in [owner.lease(), room.lease()] {
        let told = context(Some(&lease), super::super::Chosen::ByPresence);
        assert!(
            told.history.is_empty(),
            "sticky history must not be read under another selection"
        );
        assert!(
            !told.requires_speaker,
            "neutral output must contain no private text"
        );
        let text = payload(told);
        assert!(text.contains("Public character."));
        assert!(!text.contains("STICKY_B_NOTE") && !text.contains("STICKY_B_HISTORY"));
    }
    assert_eq!(
        memory.count("session_open"),
        0,
        "composition opens no wrong-person snapshot"
    );
    // The same actual callback still composes A once presence and history are
    // A's and A has the same valid request-owned positive proof.
    write_known(&memory.directory, OWNER, "0123456789abcdef", "f7");
    {
        // A new observation alone intentionally leaves B counted until the
        // presence handoff settles. This positive fixture starts after that
        // handoff; the preceding negatives retain the actual sticky B.
        let mut sessions = sessions.lock().unwrap();
        sessions.presence = Default::default();
        assert_eq!(
            sessions.presence.seen(Some(a.clone()), Instant::now()),
            Some(a.as_str())
        );
    }
    history(Some(a), "CURRENT_A_HISTORY");
    for mode in [
        super::super::Chosen::ByPresence,
        super::super::Chosen::ByLease,
    ] {
        let told = context(Some(&owner.lease()), mode);
        assert!(told.requires_speaker);
        let text = payload(told);
        assert!(text.contains("CURRENT_A_HISTORY"));
        assert!(!text.contains("STICKY_B_HISTORY") && !text.contains("STICKY_B_NOTE"));
    }
    assert_eq!(memory.count("session_open"), 0);
}

#[test]
fn typed_setup_name_uses_final_speaker_proof_and_marks_disclosure() {
    if !child(
        "privacy::typed_setup_name_uses_final_speaker_proof_and_marks_disclosure",
        "privacy",
        0,
    ) {
        return;
    }
    let person = crate::remember::person_id(OWNER);
    let memory = Memory::new(Some(person.clone()));
    let known = memory.directory.join("known");
    let bus_root = std::path::PathBuf::from(format!("/tmp/lkn-privacy-{}", std::process::id()));
    std::fs::create_dir(&bus_root).unwrap();
    let bus = lamp_bus::Bus::new(&bus_root).unwrap();
    let mut eyes = bus
        .publisher_declared(
            lamp_msgs::Faces::TOPIC,
            &lamp_msgs::Faces::declaration(),
            "eyes",
        )
        .unwrap();
    eyes.publish_json(
        crate::scenario_clock::monotonic_us(),
        &lamp_msgs::Faces {
            faces: vec![lamp_msgs::Face {
                id: "f7".into(),
                xmin: 0.4,
                ymin: 0.0,
                xmax: 0.7,
                ymax: 0.3,
                confidence: 0.9,
                yaw: None,
                facing: None,
                speaking: None,
            }],
        },
    )
    .unwrap();
    history(Some(person), "");
    let context = memory_context(
        Arc::new(Mutex::new(Sessions::default())),
        None,
        Some(known.clone()),
        true,
        move |_, person, note, lease, allowed| {
            let (character, private) = with_typed_name_for_lease(
                "Public character.".into(),
                Some(OWNER),
                Some(&bus),
                Some(&known),
                lease,
            );
            let mut told = disclosure_message(
                false,
                &character,
                None,
                &note,
                None,
                person.as_deref(),
                allowed,
            );
            told.requires_speaker = private;
            told
        },
    );
    let unproved = unproved_scope(73, ScopeSnapshot::of(OWNER));
    let held = unproved_scope(74, ScopeSnapshot::held_of(OWNER));
    let owner = proof(
        75,
        &["0123456789abcdef/f7"],
        Instant::now() + Duration::from_secs(10),
    );
    for mode in [
        super::super::Chosen::ByPresence,
        super::super::Chosen::ByLease,
    ] {
        for lease in [None, Some(unproved.lease()), Some(held.lease())] {
            let told = context(lease.as_ref(), mode);
            assert!(!told.requires_speaker);
            assert!(
                !payload(told).contains(OWNER),
                "no name from unsealed, held or absent authority"
            );
        }
        let told = context(Some(&owner.lease()), mode);
        assert!(
            told.requires_speaker,
            "typed-name-only private text needs the final guard"
        );
        assert!(payload(told).contains(OWNER));
    }
    let narrowed = proof(
        76,
        &["0123456789abcdef/f7"],
        Instant::now() + Duration::from_secs(10),
    );
    narrowed.lease().narrow_to_room();
    let revoked = proof(
        77,
        &["0123456789abcdef/f7"],
        Instant::now() + Duration::from_secs(10),
    );
    revoked.revoke();
    let deadline = Instant::now() + Duration::from_millis(100);
    let expired = proof(78, &["0123456789abcdef/f7"], deadline);
    runtime().block_on(async {
        tokio::time::sleep_until(deadline.into()).await;
    });
    for mode in [
        super::super::Chosen::ByPresence,
        super::super::Chosen::ByLease,
    ] {
        for lease in [narrowed.lease(), revoked.lease(), expired.lease()] {
            assert!(!payload(context(Some(&lease), mode)).contains(OWNER));
        }
    }
    // A changed known subject invalidates the already admitted owner; putting
    // the old record back cannot revive that request or its typed name.
    write_known(
        &memory.directory,
        "Other fixture owner",
        "0123456789abcdef",
        "f7",
    );
    assert!(!payload(context(Some(&owner.lease()), super::super::Chosen::ByLease)).contains(OWNER));
    write_known(&memory.directory, OWNER, "0123456789abcdef", "f7");
    assert!(
        !payload(context(
            Some(&owner.lease()),
            super::super::Chosen::ByPresence
        ))
        .contains(OWNER)
    );
    assert!(owner.lease().is_current());
    assert!(owner.lease().private_context_lost());
    drop(eyes);
    std::fs::remove_dir_all(bus_root).unwrap();
}

#[test]
fn missing_private_source_is_reported_once_without_granting_context() {
    if !child(
        "privacy::missing_private_source_is_reported_once_without_granting_context",
        "privacy",
        0,
    ) {
        return;
    }
    let memory = Memory::new(Some(crate::remember::person_id(OWNER)));
    let known = memory.directory.join("known");
    std::fs::remove_file(crate::speaking_faces::known_speaker_of(&known)).unwrap();
    let owner = proof(
        79,
        &["0123456789abcdef/f7"],
        Instant::now() + Duration::from_secs(10),
    );
    for _ in 0..2 {
        assert!(known_name_told(true, Some(&known), Some(&owner.lease())).is_empty());
    }
    assert!(owner.lease().private_speaker().is_none());
    assert_eq!(memory.count("session_open"), 0);
}

/// The two optional private additions share the actual admitted line's proof,
/// even when a silent visible person supplied its inherited memory scope.
#[test]
fn private_name_and_hits_share_the_same_admitted_speaker_at_http_dispatch() {
    if !child(
        "privacy::private_name_and_hits_share_the_same_admitted_speaker_at_http_dispatch",
        "privacy-defaults",
        0,
    ) {
        return;
    }
    use crate::second_provider::{
        ChatFunctions, ChatPermission, Engine, Listener, Net, Services,
        net::{self, ChatEvent, Message},
        services::tests::{answered, config_at},
    };
    use crate::{provider_worker::ProviderInput, speaking_faces::SpeakingFaces};
    use futures_util::{future::BoxFuture, stream::BoxStream};
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const BOOT: &str = "0123456789abcdef";
    const FACE: &str = "0123456789abcdef/f7";
    const QUERY: &str = "Lamp, what is my synthetic name?";
    const PUBLIC: &str = "PUBLIC DEFAULTS CONTEXT";
    const MAX_HEAD: usize = 8_192;
    const MAX_BODY: usize = 65_536;

    struct Heard {
        samples: Arc<AtomicUsize>,
    }
    impl Listener for Heard {
        fn send(&self, samples: &[i16]) -> net::Result<()> {
            self.samples.fetch_add(samples.len(), Ordering::Relaxed);
            Ok(())
        }
        fn finish(self: Box<Self>, _: u64) -> BoxFuture<'static, net::Result<String>> {
            Box::pin(async { Ok(QUERY.to_owned()) })
        }
    }
    struct HttpChat {
        net: Net,
        samples: Arc<AtomicUsize>,
        chats: AtomicUsize,
    }
    impl Services for HttpChat {
        fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
            let listener = Heard {
                samples: self.samples.clone(),
            };
            Box::pin(async { Ok(Box::new(listener) as Box<dyn Listener>) })
        }
        fn chat(&self, _: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
            panic!("the combined fixture must retain the guarded HTTP path")
        }
        fn chat_guarded(
            &self,
            messages: Vec<Message>,
            functions: ChatFunctions,
            permitted: ChatPermission,
        ) -> BoxStream<'static, net::Result<ChatEvent>> {
            self.chats.fetch_add(1, Ordering::Relaxed);
            self.net.chat_guarded(messages, functions, permitted)
        }
        fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
            panic!("the Done-only HTTP reply contains no sentence")
        }
    }
    struct Capture(Option<tokio::task::JoinHandle<(Value, Instant)>>);
    impl Drop for Capture {
        fn drop(&mut self) {
            if let Some(task) = &self.0 {
                task.abort();
            }
        }
    }
    struct ConfigFolder(PathBuf);
    impl Drop for ConfigFolder {
        fn drop(&mut self) {
            if self.0.exists()
                && let Err(error) = std::fs::remove_dir_all(&self.0)
            {
                eprintln!("combined fixture config cleanup failed: {error}");
            }
        }
    }

    assert!(crate::remember::days_from_environment());
    assert!(crate::remember::hits_on());
    assert!(crate::second_provider::worker::scoped_context_from_environment());
    assert!(crate::speaking_faces::eyes_from_environment());
    let face_name = std::env::var(crate::second_provider::FACE_NAME_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    assert!(face_name);
    let _one = crate::provider_worker::tests::one_thread();
    let person = crate::remember::person_id(OWNER);
    let memory = Memory::new(Some(person.clone()));
    crate::remember::start(memory.socket.clone(), None);

    runtime().block_on(async {
        for (case, positive, request) in [("silent", false, 1), ("speaking", true, 2)] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let until = tokio::time::Instant::now() + WAIT;
            let key = format!("fixture-private-defaults-{case}-{}", std::process::id());
            let capture_key = key.clone();
            let mut capture = Capture(Some(tokio::spawn(async move {
                tokio::time::timeout_at(until, async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut raw = Vec::new();
                    let end = loop {
                        if let Some(end) = raw.windows(4).position(|part| part == b"\r\n\r\n") {
                            break end + 4;
                        }
                        assert!(raw.len() < MAX_HEAD, "bounded HTTP head");
                        let mut chunk = [0; 1024];
                        let n = socket.read(&mut chunk).await.unwrap();
                        assert!(n > 0, "complete HTTP head");
                        raw.extend_from_slice(&chunk[..n]);
                    };
                    assert!(end <= MAX_HEAD);
                    let head = std::str::from_utf8(&raw[..end]).unwrap();
                    assert!(head.starts_with("POST "));
                    assert!(head.contains(&capture_key), "only this fixture's request counts");
                    let mut lengths = head.lines().filter_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    });
                    let length = lengths.next().expect("HTTP body length");
                    assert!(length <= MAX_BODY);
                    assert!(lengths.next().is_none(), "one HTTP body length");
                    while raw.len() - end < length {
                        let mut chunk = [0; 4096];
                        let n = socket.read(&mut chunk).await.unwrap();
                        assert!(n > 0, "complete HTTP body");
                        raw.extend_from_slice(&chunk[..n]);
                        assert!(raw.len() <= MAX_HEAD + MAX_BODY);
                    }
                    let sent: Value = serde_json::from_slice(&raw[end..end + length]).unwrap();
                    let body_parsed_at = Instant::now();
                    let (head, body) = answered("").unwrap();
                    socket.write_all(head.as_bytes()).await.unwrap();
                    socket.write_all(&body).await.unwrap();
                    (sent, body_parsed_at)
                })
                .await
                .expect("the combined fixture receives a complete HTTP request")
            })));
            let config_name = format!("combined-private-{case}");
            let config_folder = std::env::temp_dir().join(format!(
                "lamp-voice-routes-{config_name}-{}",
                std::process::id()
            ));
            assert!(!config_folder.exists(), "fresh owned config directory");
            let config_dir = ConfigFolder(config_folder);
            let services = Arc::new(HttpChat {
                net: Net::new(config_at(&config_name, &key, &url, &url), Arc::new(Vec::new))
                    .unwrap(),
                samples: Arc::default(),
                chats: AtomicUsize::new(0),
            });
            let known = memory.directory.join("known");
            let context_known = known.clone();
            let dispatch_known = known.clone();
            let cache = Arc::new(FaceCache::default());
            let eyes = Arc::new(SpeakingFaces::default());
            let hits = super::super::memory_hits(
                Arc::new(Mutex::new(Sessions::default())),
                memory.socket.clone(),
                Some(known.clone()),
            );
            let mut engine = Engine::new(
                services.clone(),
                Told::default,
                Arc::new(|_, _, _, _| Box::pin(async { panic!("no tools in fixture") })),
            )
            .with_memory_scope(
                lamp_turn_owner::BootId::new([47; 16]).unwrap(),
                cache.clone(),
                Arc::new(|_, _, _, _, _| Box::pin(async { panic!("no scoped tools") })),
                Some(Arc::new(move |lease| {
                    let name = known_name_told(face_name, Some(&context_known), Some(lease));
                    Told {
                        requires_speaker: !name.is_empty(),
                        text: format!("{PUBLIC}{name}"),
                        ..Told::default()
                    }
                })),
            )
            .guarding_private_context(Arc::new(move |lease| {
                super::super::private_context_state(Some(&dispatch_known), lease)
            }))
            .seeing_who_speaks(eyes.clone(), known.clone())
            .remembering(hits)
            .announcing(false);
            engine.memory_camera(true, 1);
            // Both cases have the same genuinely fresh visible-person scope.
            // The negative differs only in the absence of positive speech.
            write_known(&memory.directory, OWNER, BOOT, "f7");
            std::fs::write(crate::speaking_faces::known_face_of(&known), "f7").unwrap();
            assert!(cache.replace_seen(
                cache.begin_refresh(),
                crate::known_faces::FaceObservation::read(&known),
                Some(1)
            ));
            engine.submit(ProviderInput::Start {
                request,
                session: 1,
                privacy_generation: 1,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
                heard_of_cut: None,
            });
            let lease = engine.memory_lease().unwrap();
            assert_eq!(lease.scope().unwrap().person(), Some(person.as_str()));
            assert!(lease.speaker().is_none());
            // As in the current-piece controls, observations and microphone
            // read stamps share one host-clock anchor, wholly before now.
            let at = Instant::now() - Duration::from_secs(1);
            let at_us = lamp_ipc::monotonic_us().checked_sub(1_000_000).unwrap();
            for (index, millis) in [100, 250, 400].into_iter().enumerate() {
                let faces = lamp_msgs::Faces {
                    faces: vec![lamp_msgs::Face {
                        id: "f7".into(),
                        xmin: 0.1,
                        ymin: 0.1,
                        xmax: 0.3,
                        ymax: 0.4,
                        confidence: 1.0,
                        yaw: None,
                        facing: Some(true),
                        speaking: Some(lamp_msgs::Speaking {
                            state: if positive {
                                lamp_msgs::SpeakingState::Yes
                            } else {
                                lamp_msgs::SpeakingState::No
                            },
                            score: 0.9,
                            from: None,
                        }),
                    }],
                };
                eyes.look(
                    at + Duration::from_millis(millis),
                    Some(((BOOT, index as u64 + 1), &faces)),
                );
            }
            for sequence in 1..=50 {
                engine.submit(ProviderInput::Audio {
                    request,
                    privacy_generation: 1,
                    sequence,
                    read_completed_at_us: at_us + sequence * 10_000,
                    samples: vec![1; lamp_audio::CAPTURE_SAMPLES],
                });
            }
            let before = memory.count("kept");
            let began = Instant::now();
            engine.submit(ProviderInput::End {
                request,
                privacy_generation: 1,
                sound: false,
                cut_in: false,
            });
            tokio::time::timeout_at(until, async {
                loop {
                    match engine.next().await {
                        crate::provider_worker::ProviderOutput::TurnComplete {
                            request: ended,
                            idle,
                        } => {
                            assert_eq!(ended, request);
                            assert!(idle);
                            break;
                        }
                        crate::provider_worker::ProviderOutput::TurnFailed { .. } => {
                            panic!("the admitted request must still complete")
                        }
                        _ => {}
                    }
                }
            })
            .await
            .expect("the combined request completes within the fixture bound");
            assert_eq!(
                memory.count("kept") - before,
                usize::from(positive),
                "private I/O is checked even if final HTTP dispatch was denied: {case}"
            );
            let (sent, body_parsed_at) =
                tokio::time::timeout_at(until, capture.0.as_mut().unwrap())
                .await
                .expect("HTTP capture finishes")
                .unwrap();
            drop(capture.0.take());
            let messages = sent["messages"].as_array().expect("actual HTTP messages");
            let payload = serde_json::to_string(messages).unwrap();
            assert!(payload.contains(PUBLIC) && payload.contains(QUERY), "{case}");
            assert_eq!(payload.contains(OWNER), positive, "name: {case}");
            assert_eq!(payload.contains(OLD), positive, "hits: {case}");
            assert_eq!(services.chats.load(Ordering::Relaxed), 1, "{case}");
            assert_eq!(
                services.samples.load(Ordering::Relaxed),
                50 * lamp_audio::CAPTURE_SAMPLES,
                "current synthetic microphone packets: {case}"
            );
            assert_eq!(lease.scope().unwrap().person(), Some(person.as_str()));
            assert_eq!(lease.speaker(), positive.then_some(FACE), "{case}");
            let requests = memory.state.lock().unwrap().requests.clone();
            let reads: Vec<_> = requests.iter().filter(|read| read["op"] == "kept").collect();
            assert_eq!(reads.len() - before, usize::from(positive), "{case}");
            assert!(reads.iter().all(|read| read["person"] == person));
            assert_eq!(memory.count("session_open"), 0, "no other note path");
            eprintln!(
                "callback check: combined_private_defaults case={case} end_to_http_us={} kept_reads={}",
                body_parsed_at.saturating_duration_since(began).as_micros(),
                reads.len() - before
            );
            drop(engine);
            drop(services);
            std::fs::remove_dir_all(&config_dir.0).unwrap();
            assert!(!config_dir.0.exists());
        }
    });
}
