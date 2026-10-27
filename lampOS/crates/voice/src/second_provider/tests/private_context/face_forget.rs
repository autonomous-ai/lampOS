//! A successful face mutation may acknowledge only its own fixed public result.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const ACK: &str = "I've forgotten your face.";
const PRIVATE: &str = "PRIVATE FACE FORGET CONTEXT";
const PUBLISHER: [u8; 16] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
];

/// An explicit V3 publication at the original observation point. Reading or
/// acquiring a ticket never changes this capture origin or permission ceiling.
fn publish_seen(known: &std::path::Path, name: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::write(crate::speaking_faces::known_face_of(known), "f1").unwrap();
    let observed_us = lamp_ipc::monotonic_us();
    let mut source = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(crate::speaking_faces::known_speaker_of(known))
        .unwrap();
    write!(
        source,
        "{}",
        json!({"version":3,"camera_hold":1,"camera_began_us":1,
            "state":"seen","publisher":"0123456789abcdef0123456789abcdef","generation":1,
            "permission_until_us":observed_us.checked_add(1_500_000).unwrap(),
            "observed_us":observed_us,"name":name,"id":null,
            "boot":"0123456789abcdef","face":"f1"})
    )
    .unwrap();
}

struct ReceiptSource {
    authority: crate::camera_authority::fixture::Fixture,
    _directory: crate::process::SessionDirectory,
    known: std::path::PathBuf,
}
impl ReceiptSource {
    fn new() -> Self {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let known = directory.path.canonicalize().unwrap().join("known");
        std::fs::write(&known, "Fixture owner").unwrap();
        publish_seen(&known, "Fixture owner");
        let authority =
            crate::camera_authority::fixture::Fixture::start(known.clone(), PUBLISHER, 1);
        Self {
            authority,
            _directory: directory,
            known,
        }
    }
    fn owner(&self) -> RequestScope {
        RequestScope::looking_later(
            RequestKey {
                worker: lamp_turn_owner::BootId::new([29; 16]).unwrap(),
                session: 1,
                request: 2,
            },
            ScopeSnapshot::of("Fixture owner"),
            self.authority.cache(),
        )
    }
    fn prove(&self, lease: &crate::memory_scope::ScopeLease) {
        assert_eq!(self.authority.ready(lease), PermissionState::Ready);
        assert_eq!(
            super::super::super::worker::private_context_state(Some(&self.known), lease),
            PermissionState::Ready
        );
    }
}

struct FaceServices {
    script: Arc<Script>,
    spoken: Arc<Mutex<Vec<String>>>,
    lose_before_call: Option<(
        std::path::PathBuf,
        Arc<lamp_ipc::camera_authority::HoldWitness>,
    )>,
    blocked_private: Option<Arc<AtomicBool>>,
}
impl Services for FaceServices {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        self.script.listen()
    }
    fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
        let lose = self.lose_before_call.clone();
        self.script
            .chat(messages)
            .map(move |event| {
                if matches!(&event, Ok(ChatEvent::ToolCalls(_)))
                    && let Some((known, witness)) = &lose
                {
                    witness.revoke().unwrap();
                    publish_revoked(known);
                }
                event
            })
            .boxed()
    }
    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        self.spoken.lock().unwrap().push(text.clone());
        if text.contains("AUTHORIZED OLD FRAGMENT")
            && let Some(dropped) = &self.blocked_private
        {
            return Box::pin(BlockedPrivateSpeech(dropped.clone()));
        }
        self.script.speak(text)
    }
}
struct BlockedPrivateSpeech(Arc<AtomicBool>);
impl futures_util::Stream for BlockedPrivateSpeech {
    type Item = net::Result<Vec<i16>>;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Pending
    }
}
impl Drop for BlockedPrivateSpeech {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
struct Server(Option<tokio::task::JoinHandle<()>>);
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}
struct RestoreThread(Option<crate::thread::Thread>);
impl Drop for RestoreThread {
    fn drop(&mut self) {
        *crate::thread::THREAD.lock().unwrap() = self.0.take().unwrap();
    }
}

/// The worker applies this same adapter only after a guarded IPC send. This
/// fixture consumes Engine output directly, then exercises that real adapter.
async fn face_turn(engine: &mut Engine, lease: &crate::memory_scope::ScopeLease) -> Vec<String> {
    let mut events = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("bounded face turn");
        super::super::super::worker::keep_admitted(&event, lease);
        let (label, last) = match event {
            ProviderOutput::Started { request, .. } => (format!("started:{request}"), false),
            ProviderOutput::Transcript {
                request: None,
                text,
                ..
            } => (format!("heard:{text}"), false),
            ProviderOutput::Transcript {
                request: Some(_),
                text,
                ..
            } if text.is_empty() => continue,
            ProviderOutput::Transcript {
                request: Some(_),
                text,
                ..
            } => (format!("said:{text}"), false),
            ProviderOutput::Audio {
                samples,
                source_frames,
                source_offset,
                ..
            } => {
                assert!(!samples.is_empty() && samples.len() <= PACKET_SAMPLES);
                assert_eq!((source_frames, source_offset), (samples.len(), 0));
                (format!("audio:{}", samples[0]), false)
            }
            ProviderOutput::GenerationComplete { .. } => ("generated".into(), false),
            ProviderOutput::TurnComplete { idle, .. } => {
                assert!(idle);
                ("complete".into(), true)
            }
            ProviderOutput::Stages { .. } => continue,
            other => panic!("unexpected face output: {other:?}"),
        };
        events.push(label);
        if last {
            return events;
        }
    }
}

fn publish_revoked(known: &std::path::Path) {
    let source: Value = serde_json::from_slice(
        &std::fs::read(crate::speaking_faces::known_speaker_of(known)).unwrap(),
    )
    .unwrap();
    // This terminal publication follows this fixture's explicit V3 Seen;
    // malformed or legacy inputs are never silently promoted to V3.
    assert_eq!(source["version"], 3);
    let observed_us = source["observed_us"].as_u64().unwrap();
    let permission_until_us = source["permission_until_us"].as_u64().unwrap();
    std::fs::write(
        crate::speaking_faces::known_speaker_of(known),
        json!({"version":3,"camera_hold":1,"camera_began_us":1,
            "observed_us":observed_us,"permission_until_us":permission_until_us,
            "state":"revoked","boot":"0123456789abcdef",
            "publisher":"0123456789abcdef0123456789abcdef","generation":2})
        .to_string(),
    )
    .unwrap();
}

#[tokio::test]
async fn proved_face_tools_retire_private_work_before_actual_scoped_mutation() {
    // Actual Engine -> production scoped runner -> tools dispatcher -> real
    // Eyes socket. The source revokes while the actual tool reply is blocked.
    for outcome in [
        "forgotten",
        "partial",
        "refused",
        "partial-refusal",
        "unknown",
        "remembered",
        "remember-unknown",
        "remember-known-as",
        "narrowed",
        "observed-lost",
        "mixed-queued",
        "remember-mixed-queued",
        "prior-policy-held",
    ] {
        let _one = crate::provider_worker::tests::one_thread();
        let _thread = RestoreThread(Some(std::mem::take(
            &mut *crate::thread::THREAD.lock().unwrap(),
        )));
        let directory = crate::process::SessionDirectory::create().unwrap();
        let parent = directory.path.canonicalize().unwrap();
        let socket = parent.join("who.sock");
        let known = crate::known_faces::known_file_of(&socket);
        std::fs::write(&known, "A").unwrap();
        let authority =
            crate::camera_authority::fixture::Fixture::start(known.clone(), PUBLISHER, 1);
        let remember = outcome.starts_with("remember");
        let mixed = outcome.ends_with("mixed-queued");
        let lost_before = outcome == "observed-lost";
        let tool = if remember {
            crate::known_faces::REMEMBER
        } else {
            crate::known_faces::FORGET
        };
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let server_known = known.clone();
        let server_witness = authority.witness.clone();
        let private_speech_dropped = Arc::new(AtomicBool::new(false));
        let server_dropped = private_speech_dropped.clone();
        let (revoked, revocation) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let mut server = Server(Some(tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(2), async {
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = tokio::io::BufReader::new(stream);
                let mut raw = Vec::new();
                loop {
                    assert!(raw.len() < 4096, "bounded actual tool request");
                    let mut byte = [0];
                    stream.read_exact(&mut byte).await.unwrap();
                    raw.push(byte[0]);
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                let wire: Value = serde_json::from_slice(&raw).unwrap();
                if remember {
                    let proof = &wire["remember_speaker"];
                    assert_eq!(wire.as_object().unwrap().len(), 1);
                    assert_eq!(proof["name"], "A");
                    assert_eq!(proof["face"], GAP_OWNER_SPEAKER);
                    assert_eq!(proof["replace"], false);
                    assert_eq!(proof["apart"], false);
                    assert!(
                        proof["began_us"].as_u64().unwrap() < proof["ended_us"].as_u64().unwrap()
                    );
                    assert_eq!(proof.as_object().unwrap().len(), 6);
                } else {
                    assert_eq!(wire, json!({"forget":"A"}));
                }
                if mixed {
                    assert!(
                        server_dropped.load(Ordering::Acquire),
                        "old TTS stream is dropped and joined BEFORE the face RPC"
                    );
                }
                server_witness.revoke().unwrap();
                publish_revoked(&server_known);
                revoked.send(()).unwrap();
                released.await.unwrap();
                let reply = match outcome {
                    "partial" => json!({"forgot":true,"unsearched":1}),
                    "refused" => json!({"forgot":false}),
                    "partial-refusal" => json!({"forgot":false,"unsearched":1}),
                    "unknown" | "remember-unknown" => {
                        json!({"error":"storage","known_as":"PRIVATE STORED NAME"})
                    }
                    "remember-known-as" => {
                        json!({"error":"known_as","known_as":"PRIVATE STORED NAME"})
                    }
                    "remembered" | "remember-mixed-queued" => {
                        json!({"remembered":"A","visits":true})
                    }
                    _ => json!({"forgot":true}),
                };
                stream
                    .get_mut()
                    .write_all(format!("{reply}\n").as_bytes())
                    .await
                    .unwrap();
            })
            .await
            .expect("bounded actual Eyes fixture exchange");
        })));
        let script = Arc::new(Script {
            heard: Mutex::new(VecDeque::from([Ok(if remember {
                "I'm A. Please remember my face.".into()
            } else {
                "Please forget my face.".into()
            })])),
            chats: Mutex::new(VecDeque::from([{
                let mut events = Vec::new();
                if mixed {
                    events.push(Ok(ChatEvent::Text("AUTHORIZED OLD FRAGMENT.".into())));
                }
                let mut calls = Vec::new();
                if outcome == "prior-policy-held" {
                    calls.push(call("held", "held_before_face", json!({})));
                }
                calls.push(call("face", tool, json!({"name":"A"})));
                if mixed {
                    calls.push(call("other", "pending_other_part", json!({})));
                }
                events.push(Ok(ChatEvent::ToolCalls(calls)));
                events.push(Ok(ChatEvent::Done));
                events
            }])),
            ..Script::default()
        });
        let spoken = Arc::new(Mutex::new(Vec::new()));
        let services = Arc::new(FaceServices {
            script: script.clone(),
            spoken: spoken.clone(),
            lose_before_call: lost_before.then(|| (known.clone(), authority.witness.clone())),
            blocked_private: mixed.then(|| private_speech_dropped.clone()),
        });
        let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
        let cache = authority.cache();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let journal = Arc::new(Mutex::new(Vec::new()));
        let tool_journal = journal.clone();
        // Named visit consent is offered only when Memory is configured.
        // These remembered-person rows never dispatch a Memory RPC: enrollment
        // uses Eyes, and face_saved applies only to a room's successful save.
        // Keep a real private endpoint and detect any unexpected dispatch after
        // the actual tool has joined, without arming forget-row side effects.
        let memory = remember.then(|| {
            let path = parent.join("memory.sock");
            let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
            listener.set_nonblocking(true).unwrap();
            (path, listener)
        });
        let settings = crate::tools::Settings {
            who: Some(socket.clone()),
            memory: memory.as_ref().map(|(path, _)| path.clone()),
            names_checked: true,
            remember_speaker: true,
            who_visits: true,
            ..crate::tools::Settings::default()
        };
        let crate::tools::Route::Faces(_, switches, _) = crate::tools::route(&settings, tool)
        else {
            panic!("actual face route");
        };
        assert_eq!(switches.visits, remember, "real Memory prerequisite");
        let declared = crate::tools::declared(&settings);
        for consent in [crate::known_faces::TELL, crate::known_faces::DONT_TELL] {
            assert_eq!(declared.iter().any(|tool| tool.name == consent), remember);
        }
        let production =
            super::super::super::worker::scoped_tool_runner(settings, None, None, false);
        let barrier = Arc::new(Mutex::new(Some((revocation, release))));
        let tool_known = known.clone();
        let runner: ScopedToolRunner = Arc::new(move |request, heard, name, args, lease| {
            assert_eq!(request, 1);
            assert!(heard);
            assert_eq!(name, tool);
            assert!(lease.is_current());
            assert_eq!(lease.speaker(), Some(GAP_OWNER_SPEAKER));
            assert!(
                lease.private_context_lost(),
                "retired BEFORE actual write dispatch"
            );
            count.fetch_add(1, Ordering::Relaxed);
            let operation = production(request, heard, name, args, lease.clone());
            let (revocation, release) = barrier.lock().unwrap().take().unwrap();
            let known = tool_known.clone();
            let journal = tool_journal.clone();
            Box::pin(async move {
                let observed = async {
                    tokio::time::timeout(Duration::from_secs(2), revocation)
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(
                        super::super::super::worker::private_context_state(Some(&known), &lease),
                        PermissionState::Lost
                    );
                    assert!(lease.is_current());
                    assert_eq!(lease.speaker(), Some(GAP_OWNER_SPEAKER));
                    assert!(
                        journal.lock().unwrap().is_empty(),
                        "no premature tool feedback while the actual RPC is blocked"
                    );
                    if outcome == "narrowed" {
                        lease.narrow_to_room();
                    }
                    release.send(()).unwrap();
                };
                let (told, ()) = tokio::join!(operation, observed);
                assert!(
                    journal.lock().unwrap().is_empty(),
                    "feedback follows the actual tool runner return"
                );
                told
            })
        });
        let dispatch_known = known.clone();
        let mut engine = Engine::new(
            services,
            Told::default,
            Arc::new(|_, _, _, _| panic!("scoped only")),
        )
        .with_memory_scope(
            lamp_turn_owner::BootId::new([69; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|_| Told {
                text: PRIVATE.into(),
                requires_speaker: true,
                ..Told::default()
            })),
        )
        .seeing_who_speaks(eyes.clone(), known.clone())
        .remembering_speaker(true, Some(eyes.clone()))
        .guarding_private_context(Arc::new(move |lease| {
            super::super::super::worker::private_context_state(Some(&dispatch_known), lease)
        }))
        .announcing(false)
        .noting_turn_tools(true)
        .keeping_turn_tools_journal(journal.clone());
        let policy_seen = Arc::new(Mutex::new(Vec::new()));
        if outcome == "prior-policy-held" {
            let seen = policy_seen.clone();
            engine = engine.holding_for_the_policy(Arc::new(move |request, name| {
                assert_eq!(request, 1);
                seen.lock().unwrap().push(name.clone());
                Box::pin(async move {
                    if name == "held_before_face" {
                        Policed::Held
                    } else {
                        Policed::Run
                    }
                })
            }));
        }
        engine.memory_camera(true, 1);
        publish_seen(&known, "A");
        assert!(cache.replace_seen(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&known),
            Some(1)
        ));
        engine.submit(start(1));
        let lease = engine.memory_lease().unwrap();
        assert!(lease.speaker().is_none());
        gap_owner_microphone(&mut engine, &eyes, 1);
        engine.submit(end(1));
        let events = face_turn(&mut engine, &lease).await;
        assert!(
            lease.camera_binding().is_some(),
            "private Engine context acquired the actual V3 kernel ticket"
        );
        tokio::time::timeout(
            Duration::from_secs(2),
            &mut engine.turn.as_mut().unwrap().task,
        )
        .await
        .unwrap()
        .unwrap();
        if lost_before {
            let task = server.0.take().unwrap();
            task.abort();
            assert!(
                tokio::time::timeout(Duration::from_secs(2), task)
                    .await
                    .unwrap()
                    .unwrap_err()
                    .is_cancelled()
            );
            assert_eq!(calls.load(Ordering::Relaxed), 0);
            assert!(spoken.lock().unwrap().is_empty());
        } else {
            tokio::time::timeout(Duration::from_secs(2), server.0.as_mut().unwrap())
                .await
                .unwrap()
                .unwrap();
            server.0.take();
            assert_eq!(calls.load(Ordering::Relaxed), 1);
        }
        if let Some((_, listener)) = &memory {
            match listener.accept() {
                Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock),
                Ok(_) => panic!("remembered-person face route unexpectedly dispatched Memory"),
            }
        }
        assert_eq!(
            journal.lock().unwrap().len(),
            1,
            "tool feedback is retained"
        );
        if !lost_before {
            assert!(journal.lock().unwrap()[0].contains(&format!("{tool}:1")));
        }
        assert_eq!(
            script.asked.lock().unwrap().len(),
            1,
            "never restart private chat"
        );
        assert!(
            script.asked.lock().unwrap()[0]
                .iter()
                .any(|message| message.words().is_some_and(|s| s.contains(PRIVATE)))
        );
        assert!(lease.private_context_lost());
        assert!(events.last().is_some_and(|event| event == "complete"));
        assert!(
            !events
                .iter()
                .any(|event| event.contains(PRIVATE) || event.contains("PRIVATE STORED NAME"))
        );
        if outcome == "prior-policy-held" {
            assert_eq!(*policy_seen.lock().unwrap(), ["held_before_face", tool]);
            assert!(journal.lock().unwrap()[0].contains("held_before_face:1"));
        }
        if matches!(outcome, "narrowed" | "observed-lost" | "prior-policy-held") {
            assert!(spoken.lock().unwrap().is_empty());
            assert!(
                !events
                    .iter()
                    .any(|event| event.starts_with("said:") || event.starts_with("audio:"))
            );
        } else {
            let lines = spoken.lock().unwrap();
            assert_eq!(lines.len(), if mixed { 2 } else { 1 });
            let line = lines.last().unwrap();
            match outcome {
                "forgotten" => assert_eq!(line, ACK),
                "mixed-queued" => {
                    assert_eq!(line, &format!("Please ask again for the other part. {ACK}"));
                    assert!(private_speech_dropped.load(Ordering::Acquire));
                    // Earlier words may have been authorized before the handoff;
                    // old packets cannot be polled or emitted after it.
                    let receipt = events
                        .iter()
                        .position(|event| event == &format!("said:{line}"))
                        .unwrap();
                    assert!(
                        !events[receipt..]
                            .iter()
                            .any(|event| event.contains("AUTHORIZED OLD FRAGMENT"))
                    );
                }
                "partial" => {
                    assert!(line.starts_with(ACK));
                    assert!(line.contains("kept apart"));
                    assert!(line.contains("factory reset"));
                }
                "refused" => assert_eq!(line, "I couldn't find a matching face to forget."),
                "partial-refusal" => {
                    assert!(line.starts_with("I couldn't find a matching face"));
                    assert!(line.contains("kept apart"));
                }
                "remembered" | "remember-mixed-queued" => {
                    if mixed {
                        assert!(line.starts_with("Please ask again for the other part. "));
                        assert!(private_speech_dropped.load(Ordering::Acquire));
                    }
                    assert!(line.contains("I've remembered your face."));
                    assert!(line.contains("how often you come by"));
                    assert!(line.contains("May I tell anyone who asks"));
                    assert!(line.contains("Unless you agree"));
                    assert!(line.ends_with('?'));
                    let thread = crate::thread::THREAD.lock().unwrap();
                    assert!(
                        thread.lamp_asked(2),
                        "actual public question reaches next-turn attention"
                    );
                    let recent = thread
                        .recall_for(lease.scope().unwrap().person(), Instant::now(), 8192)
                        .unwrap();
                    assert!(recent.contains(line.as_str()));
                    assert!(!recent.contains(PRIVATE));
                }
                _ => assert_eq!(
                    line,
                    "I couldn't confirm whether that face request was completed."
                ),
            }
            assert!(events.iter().any(|event| event == &format!("said:{line}")));
            assert!(events.iter().any(|event| event.starts_with("audio:")));
            let said = engine.shared.said.lock().unwrap();
            assert_eq!(said.1.last().unwrap().words.as_deref(), Some(line.as_str()));
            assert_eq!(said.1.last().unwrap().samples, 1500);
            assert_eq!(*engine.shared.answered_request.lock().unwrap(), Some(1));
            assert_eq!(engine.shared.lamp_said.lock().unwrap().1, *line);
        }
    }
}

#[test]
fn fixed_face_outcomes_keep_consent_and_partial_failure_without_raw_values() {
    let remember = crate::known_faces::REMEMBER;
    let forget = crate::known_faces::FORGET;
    for (note, visits, consent) in [
        (None, false, false),
        (
            Some(crate::known_faces::VISITS_LINE.to_owned()),
            true,
            false,
        ),
        (
            Some(crate::known_faces::ASK_TO_NAME.to_owned()),
            false,
            true,
        ),
        (
            Some(format!(
                "{} {}",
                crate::known_faces::VISITS_LINE,
                crate::known_faces::ASK_TO_NAME
            )),
            true,
            true,
        ),
    ] {
        let mut told = json!({"ok":true,"remembered":"PRIVATE STORED NAME"});
        if let Some(note) = note {
            told["also_say"] = json!(note);
        }
        let line = fixed_face_outcome(remember, &told);
        assert!(line.starts_with("I've remembered your face."));
        assert_eq!(line.contains("how often you come by"), visits);
        assert_eq!(line.contains("May I tell anyone who asks"), consent);
        assert!(!line.contains("PRIVATE STORED NAME"));
    }
    for memory in ["forgotten too", crate::remember::FACE_MEMORY_KEPT] {
        let told = crate::remember::face_and_memory(
            json!({"ok":true,"note":crate::known_faces::FORGOTTEN_SOME_KEPT_APART}),
            &if memory == "forgotten too" {
                json!({"forgotten":true})
            } else {
                json!({"error":"PRIVATE FAILURE"})
            },
        );
        let line = fixed_face_outcome(forget, &told);
        assert!(line.starts_with(ACK));
        assert!(line.contains("kept apart"));
        assert_eq!(
            line.contains("couldn't be forgotten just now"),
            memory == crate::remember::FACE_MEMORY_KEPT
        );
        assert_eq!(
            line.contains("I've also forgotten"),
            memory == "forgotten too"
        );
        assert!(!line.contains("PRIVATE FAILURE"));
    }
    for told in [
        json!({"ok":true}),
        json!({"ok":true,"remembered":null}),
        json!({"error":"PRIVATE ERROR","known_as":"PRIVATE NAME"}),
        json!({"ok":true,"remembered":"PRIVATE NAME","also_say":"PRIVATE INSTRUCTIONS"}),
    ] {
        assert_eq!(
            fixed_face_outcome(remember, &told),
            "I couldn't confirm whether that face request was completed."
        );
    }
}

#[test]
fn fixed_face_receipt_never_mints_from_prior_loss_or_outlives_its_owner() {
    for loss in [
        "guard", "lease", "pending", "revoked", "narrowed", "replaced",
    ] {
        let (engine, _, _) = scripted(Arc::new(Script::default()));
        let mut owner = admission(true);
        let lease = owner.lease();
        let recorder = owner.speaker_recorder().unwrap();
        assert!(recorder.seal(
            lease.key(),
            &[GAP_OWNER_SPEAKER.into()],
            Instant::now() + Duration::from_secs(2)
        ));
        *engine.shared.begun.lock().unwrap() = Some((2, 1));
        assert!(lease.private_context_permitted(true));
        let guard = PrivateContext::new(&engine.shared, Some(&lease), true);
        match loss {
            "guard" => guard.lost.store(true, Ordering::Release),
            "lease" => {
                lease.private_context_permitted(false);
            }
            "pending" => {
                lease.pin_private_speaker(crate::speaking_faces::KnownSpeaker {
                    name: "Fixture owner".into(),
                    person: crate::remember::person_id("Fixture owner"),
                    id: None,
                    face: GAP_OWNER_SPEAKER.into(),
                    epoch: Some(crate::speaking_faces::KnownSpeakerEpoch {
                        boot: "0123456789abcdef".into(),
                        publisher: "0123456789abcdef0123456789abcdef".into(),
                        generation: 1,
                    }),
                });
                assert!(lease.pause_private_context());
            }
            "revoked" => owner.revoke(),
            "narrowed" => lease.narrow_to_room(),
            "replaced" => {
                *engine.shared.begun.lock().unwrap() = Some((3, 1));
            }
            _ => unreachable!(),
        }
        assert!(
            guard.face_operation_receipt(&engine.shared, 2).is_none(),
            "{loss}"
        );
    }
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let source = ReceiptSource::new();
    let mut owner = source.owner();
    let lease = owner.lease();
    assert!(owner.speaker_recorder().unwrap().seal(
        lease.key(),
        &[GAP_OWNER_SPEAKER.into()],
        Instant::now() + Duration::from_secs(2)
    ));
    *engine.shared.begun.lock().unwrap() = Some((2, 1));
    source.prove(&lease);
    assert!(lease.private_context_permitted(true));
    let old = PrivateContext::new(&engine.shared, Some(&lease), true);
    let public = old.face_operation_receipt(&engine.shared, 2).unwrap();
    assert_eq!(old.state(), PermissionState::Lost);
    assert!(lease.private_context_lost());
    assert_eq!(public.state(), PermissionState::Ready);
    let evidence = crate::camera_authority::read(&source.known).unwrap();
    let source_file = crate::speaking_faces::known_speaker_of(&source.known);
    let original_source = std::fs::read(&source_file).unwrap();
    source.authority.witness.revoke().unwrap();
    assert_eq!(std::fs::read(&source_file).unwrap(), original_source);
    assert_eq!(
        lease.camera_binding().unwrap().state(&evidence),
        PermissionState::Lost,
        "the actual ticket closes while the original V3 source stays unchanged"
    );
    assert!(lease.is_current());
    assert_eq!(public.state(), PermissionState::Ready);
    publish_revoked(&source.known);
    assert_eq!(
        super::super::super::worker::private_context_state(Some(&source.known), &lease),
        PermissionState::Lost
    );
    assert_eq!(old.state(), PermissionState::Lost);
    assert!(lease.private_context_lost());
    assert_eq!(
        public.state(),
        PermissionState::Ready,
        "only the request-owned fixed public receipt survives source and ticket loss"
    );
    assert!(
        old.face_operation_receipt(&engine.shared, 2).is_none(),
        "one handoff only"
    );
    let queued = QueuedOutput {
        event: ProviderOutput::Transcript {
            request: Some(2),
            text: ACK.into(),
            finished: false,
        },
        private_context: Some(public),
    };
    owner.revoke();
    assert_eq!(
        queued.state(),
        PermissionState::Lost,
        "queued and IPC-facing fixed output stays request-owned"
    );
}

#[test]
fn observed_disclosure_loss_wins_the_atomic_fixed_receipt_claim() {
    for _ in 0..32 {
        let owner = admission(true);
        let lease = owner.lease();
        assert!(
            !lease.retire_private_for_face_receipt(),
            "unproved cannot transfer"
        );
        assert!(lease.private_context_permitted(true));
        let gate = std::sync::Barrier::new(2);
        std::thread::scope(|threads| {
            let losing = lease.clone();
            let gate_ref = &gate;
            let loss = threads.spawn(move || {
                assert!(!losing.private_context_permitted(false));
                gate_ref.wait();
            });
            gate.wait();
            assert!(
                !lease.retire_private_for_face_receipt(),
                "observed loss is never revived"
            );
            loss.join().unwrap();
        });
        assert!(lease.private_context_lost());
        assert!(lease.is_current());
    }
}

#[tokio::test]
async fn queued_fixed_face_receipt_keeps_the_original_audio_expiry() {
    let (engine, _, _) = scripted(Arc::new(Script::default()));
    let source = ReceiptSource::new();
    let mut owner = source.owner();
    let lease = owner.lease();
    let until = Instant::now() + Duration::from_secs(2);
    assert!(owner.speaker_recorder().unwrap().seal(
        lease.key(),
        &[GAP_OWNER_SPEAKER.into()],
        until
    ));
    *engine.shared.begun.lock().unwrap() = Some((2, 1));
    source.prove(&lease);
    assert!(lease.private_context_permitted(true));
    let private = PrivateContext::new(&engine.shared, Some(&lease), true);
    let public = private.face_operation_receipt(&engine.shared, 2).unwrap();
    let queued = QueuedOutput {
        event: ProviderOutput::Transcript {
            request: Some(2),
            text: ACK.into(),
            finished: false,
        },
        private_context: Some(public),
    };
    assert_eq!(queued.state(), PermissionState::Ready);
    tokio::time::timeout(
        Duration::from_secs(3),
        tokio::time::sleep_until(tokio::time::Instant::from_std(until)),
    )
    .await
    .unwrap();
    assert_eq!(queued.state(), PermissionState::Lost);
    assert_eq!(private.state(), PermissionState::Lost);
    assert!(lease.private_context_lost());
    assert!(lease.is_current());
}
