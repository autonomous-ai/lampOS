//! Final Engine dispatch boundaries, with deterministic authority loss and real Services traps.
use super::*;
mod carried_speaker;
mod face_forget;
mod short_line;
use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};

fn admission(private: bool) -> RequestScope {
    RequestScope::new(
        RequestKey {
            worker: lamp_turn_owner::BootId::new([29; 16]).unwrap(),
            session: 1,
            request: 2,
        },
        if private {
            ScopeSnapshot::of("Fixture owner")
        } else {
            ScopeSnapshot::room()
        },
    )
}

struct Trapped {
    script: Arc<Script>,
    speeches: Arc<AtomicUsize>,
    expire_on_event: Option<Arc<AtomicBool>>,
}
impl Services for Trapped {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        self.script.listen()
    }
    fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
        let expire = self.expire_on_event.clone();
        self.script
            .chat(messages)
            .map(move |event| {
                if let Some(valid) = &expire {
                    valid.store(false, Ordering::Release);
                }
                event
            })
            .boxed()
    }
    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        self.speeches.fetch_add(1, Ordering::Relaxed);
        self.script.speak(text)
    }
}

fn guarded(
    script: Arc<Script>,
    required: bool,
    valid: Arc<AtomicBool>,
) -> (Engine, Arc<AtomicUsize>) {
    let (engine, _, _) = scripted(script.clone());
    let speeches = Arc::new(AtomicUsize::new(0));
    let mut engine = engine
        .announcing(false)
        .guarding_private_context(Arc::new(move |_| valid.load(Ordering::Acquire).into()));
    let shared = Arc::get_mut(&mut engine.shared).unwrap();
    shared.services = Arc::new(Trapped {
        script,
        speeches: speeches.clone(),
        expire_on_event: None,
    });
    shared.system = Arc::new(move || Told {
        text: "SYSTEM".into(),
        after: None,
        history: Vec::new(),
        requires_speaker: required,
        pending_private_context: false,
    });
    (engine, speeches)
}

async fn finish(
    engine: &mut Engine,
    owner: &RequestScope,
    notes_private: bool,
    prepared: Option<Prepared>,
    words: &str,
) {
    let shared = engine.shared.clone();
    let run = async {
        let answering = answer(
            &shared,
            (2, Some(owner.lease())),
            words.into(),
            Asker::Spoken,
            None,
            None,
            if notes_private {
                vec!["private hit".into()]
            } else {
                Vec::new()
            },
            notes_private,
            Times::new(Instant::now()),
            prepared,
            None,
        );
        tokio::pin!(answering);
        loop {
            tokio::select! {
                _ = &mut answering => break,
                event = engine.output.recv() => { assert!(event.is_some()); }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .expect("the answer is bounded");
}

#[tokio::test]
async fn expired_private_system_never_calls_chat_or_speech() {
    let script = Arc::new(Script::default());
    let (mut engine, speeches) = guarded(script.clone(), true, Arc::new(AtomicBool::new(false)));
    let owner = admission(true);
    finish(&mut engine, &owner, false, None, "question").await;
    assert!(script.asked.lock().unwrap().is_empty());
    assert!(engine.shared.lamp_said.lock().unwrap().1.is_empty());
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn private_hits_are_guarded_even_with_neutral_system() {
    let script = Arc::new(Script::default());
    let (mut engine, speeches) = guarded(script.clone(), false, Arc::new(AtomicBool::new(false)));
    let owner = admission(true);
    finish(&mut engine, &owner, true, None, "question").await;
    assert!(script.asked.lock().unwrap().is_empty());
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn private_held_forget_policy_expiry_sends_no_chat_or_tool() {
    let script = Arc::new(Script::default());
    let valid = Arc::new(AtomicBool::new(true));
    let (mut engine, speeches) = guarded(script.clone(), true, valid.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let shared = Arc::get_mut(&mut engine.shared).unwrap();
    shared.tools = Arc::new(move |_, _, _, _| {
        counted.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { json!({"ok":true}) })
    });
    shared.policy = Some(Arc::new(move |_, _| {
        let valid = valid.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            valid.store(false, Ordering::Release);
            Policed::Run
        })
    }));
    shared.held_forget.lock().unwrap().held = Some(HeldForget {
        call: call("held", crate::remember::FORGET, json!({"fact":"fixture"})),
        request: 1,
        asker: (1, crate::remember::person_id("Fixture owner")),
        at: Instant::now(),
        asked: true,
    });
    let owner = admission(true);
    finish(&mut engine, &owner, false, None, "yes").await;
    assert!(script.asked.lock().unwrap().is_empty());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn private_tool_completion_expiry_cannot_start_another_round() {
    let script = Arc::new(Script {
        chats: Mutex::new(VecDeque::from([vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "tool",
                "fixture_tool",
                json!({}),
            )])),
            Ok(ChatEvent::Done),
        ]])),
        ..Script::default()
    });
    let valid = Arc::new(AtomicBool::new(true));
    let (mut engine, speeches) = guarded(script.clone(), true, valid.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    Arc::get_mut(&mut engine.shared).unwrap().tools = Arc::new(move |_, _, _, _| {
        let valid = valid.clone();
        let counted = counted.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            counted.fetch_add(1, Ordering::Relaxed);
            valid.store(false, Ordering::Release);
            json!({"private":"fixture"})
        })
    });
    let owner = admission(true);
    finish(&mut engine, &owner, false, None, "question").await;
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(script.asked.lock().unwrap().len(), 1);
    assert!(engine.shared.lamp_said.lock().unwrap().1.is_empty());
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn expired_prepared_private_events_never_reach_speech() {
    let script = Arc::new(Script {
        chats: Mutex::new(VecDeque::from([vec![
            text("Private answer."),
            Ok(ChatEvent::Done),
        ]])),
        ..Script::default()
    });
    let valid = Arc::new(AtomicBool::new(true));
    let (mut engine, speeches) = guarded(script.clone(), true, valid.clone());
    let owner = admission(true);
    let prepared = Prepared::begin(
        &engine.shared,
        Some(&owner.lease()),
        "question".into(),
        &[],
        None,
        false,
    );
    tokio::task::yield_now().await;
    valid.store(false, Ordering::Release);
    finish(&mut engine, &owner, false, Some(prepared), "question").await;
    assert_eq!(script.asked.lock().unwrap().len(), 1);
    assert!(engine.shared.lamp_said.lock().unwrap().1.is_empty());
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn positive_private_and_public_room_contexts_still_dispatch() {
    for private in [false, true] {
        let script = Arc::new(Script {
            chats: Mutex::new(VecDeque::from([vec![
                text("Ordinary answer."),
                Ok(ChatEvent::Done),
            ]])),
            ..Script::default()
        });
        let (mut engine, speeches) =
            guarded(script.clone(), private, Arc::new(AtomicBool::new(private)));
        let owner = admission(private);
        finish(&mut engine, &owner, false, None, "question").await;
        assert_eq!(script.asked.lock().unwrap().len(), 1);
        assert!(
            engine
                .shared
                .lamp_said
                .lock()
                .unwrap()
                .1
                .contains("Ordinary answer.")
        );
        assert_eq!(speeches.load(Ordering::Relaxed), 1);
    }
}

/// Existing heed, echo and carry readers still see every packet's read time;
/// only positive speaker proof excludes empty packets, including after carry.
#[tokio::test]
async fn empty_packets_preserve_legacy_current_and_carried_read_bounds() {
    let script = Arc::new(Script::default());
    let (mut engine, _, _) = scripted(script);
    let current = |engine: &Engine| {
        *engine
            .turn
            .as_ref()
            .unwrap()
            .current_speaker_reads
            .lock()
            .unwrap()
    };
    engine.submit(start(1));
    let packet = |request, at, samples| ProviderInput::Audio {
        request,
        privacy_generation: 1,
        sequence: at,
        read_completed_at_us: at,
        samples,
    };
    engine.submit(packet(1, 10_000, Vec::new()));
    assert_eq!(
        *engine.turn.as_ref().unwrap().said.lock().unwrap(),
        (10_000, 10_000)
    );
    assert_eq!(
        *engine.turn.as_ref().unwrap().speaker_reads.lock().unwrap(),
        (0, 0)
    );
    assert_eq!(engine.kept.as_ref().unwrap().said_from, 10_000);
    assert_eq!(engine.kept.as_ref().unwrap().speaker_from, 0);
    assert_eq!(current(&engine), (0, 0));
    engine.submit(packet(1, 20_000, vec![1; 160]));
    engine.submit(packet(1, 30_000, Vec::new()));
    assert_eq!(
        *engine.turn.as_ref().unwrap().said.lock().unwrap(),
        (10_000, 30_000)
    );
    assert_eq!(
        *engine.turn.as_ref().unwrap().speaker_reads.lock().unwrap(),
        (20_000, 20_000)
    );
    assert_eq!(current(&engine), (20_000, 20_000));
    let mut next = start(2);
    if let ProviderInput::Start { continues, .. } = &mut next {
        *continues = true;
    }
    engine.submit(next);
    assert_eq!(
        *engine.turn.as_ref().unwrap().said.lock().unwrap(),
        (10_000, 10_000)
    );
    assert_eq!(
        *engine.turn.as_ref().unwrap().speaker_reads.lock().unwrap(),
        (20_000, 20_000)
    );
    assert_eq!(engine.kept.as_ref().unwrap().speaker_from, 20_000);
    assert_eq!(current(&engine), (0, 0));
    engine.submit(packet(2, 40_000, Vec::new()));
    assert_eq!(
        *engine.turn.as_ref().unwrap().said.lock().unwrap(),
        (10_000, 40_000)
    );
    assert_eq!(
        *engine.turn.as_ref().unwrap().speaker_reads.lock().unwrap(),
        (20_000, 20_000)
    );
    assert_eq!(current(&engine), (0, 0));
    engine.submit(packet(2, 50_000, vec![1; 160]));
    assert_eq!(current(&engine), (50_000, 50_000));
    assert_eq!(
        *engine.turn.as_ref().unwrap().said.lock().unwrap(),
        (10_000, 50_000)
    );
    assert_eq!(
        *engine.turn.as_ref().unwrap().speaker_reads.lock().unwrap(),
        (20_000, 50_000)
    );
}

#[tokio::test]
async fn authority_lost_while_chat_waits_sends_no_private_speech() {
    let script = Arc::new(Script {
        chats: Mutex::new(VecDeque::from([vec![
            text("Private answer."),
            Ok(ChatEvent::Done),
        ]])),
        ..Script::default()
    });
    let valid = Arc::new(AtomicBool::new(true));
    let (mut engine, speeches) = guarded(script.clone(), true, valid.clone());
    Arc::get_mut(&mut engine.shared).unwrap().services = Arc::new(Trapped {
        script: script.clone(),
        speeches: speeches.clone(),
        expire_on_event: Some(valid),
    });
    let owner = admission(true);
    finish(&mut engine, &owner, false, None, "question").await;
    assert_eq!(script.asked.lock().unwrap().len(), 1);
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
    assert!(engine.shared.lamp_said.lock().unwrap().1.is_empty());
}

#[tokio::test]
async fn private_authority_loss_completes_the_live_engine_turn_once() {
    let script = Arc::new(Script {
        heard: Mutex::new(VecDeque::from([Ok("Fixture question.".into())])),
        chats: Mutex::new(VecDeque::from([vec![
            text("Private fixture answer."),
            Ok(ChatEvent::Done),
        ]])),
        ..Script::default()
    });
    let valid = Arc::new(AtomicBool::new(true));
    let (mut engine, speeches) = guarded(script.clone(), true, valid.clone());
    Arc::get_mut(&mut engine.shared).unwrap().services = Arc::new(Trapped {
        script: script.clone(),
        speeches: speeches.clone(),
        // Deterministic expiry at the asynchronous chat boundary, while the
        // microphone/request owner remains live. No terminal is injected.
        expire_on_event: Some(valid.clone()),
    });
    let cache = Arc::new(crate::memory_scope::FaceCache::default());
    let runner: ScopedToolRunner = Arc::new(|_, _, _, _, _| {
        panic!("the fixture answer has no tool call");
    });
    engine = engine.with_memory_scope(
        lamp_turn_owner::BootId::new([29; 16]).unwrap(),
        cache.clone(),
        runner,
        None,
    );
    engine.memory_camera(true, 1);
    let path = std::env::temp_dir().join(format!(
        "private-terminal-known-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, "Fixture owner").unwrap();
    let observation = crate::known_faces::FaceObservation::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(cache.replace(cache.begin_refresh(), Some(observation)));
    engine.submit(start(2));
    let lease = engine.memory_lease().unwrap();
    assert_eq!(
        lease.scope().unwrap().person(),
        Some(crate::remember::person_id("Fixture owner").as_str())
    );
    engine.submit(audio(2, 1));
    engine.submit(end(2));

    assert_eq!(
        turn(&mut engine).await,
        ["started:2", "heard:Fixture question.", "complete"]
    );
    tokio::time::timeout(
        Duration::from_secs(2),
        &mut engine.turn.as_mut().unwrap().task,
    )
    .await
    .expect("the completed answer task exits")
    .unwrap();
    assert!(lease.is_current(), "proof loss is not owner revocation");
    assert!(!valid.load(Ordering::Acquire));
    assert_eq!(script.asked.lock().unwrap().len(), 1);
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
    assert!(engine.shared.in_flight.lock().unwrap().is_none());
    assert!(engine.shared.lamp_said.lock().unwrap().1.is_empty());
    assert!(engine.try_output().is_none(), "one terminal only");
}

#[test]
fn lost_private_authority_cannot_complete_a_revoked_or_different_request() {
    let valid = Arc::new(AtomicBool::new(true));
    let (engine, _) = guarded(Arc::new(Script::default()), true, valid.clone());
    let owner = admission(true);
    let guard = PrivateContext::new(&engine.shared, Some(&owner.lease()), true);
    *engine.shared.begun.lock().unwrap() = Some((2, 1));
    assert!(guard.current());
    assert!(!guard.lost_while_owned(&engine.shared, 2));
    valid.store(false, Ordering::Release);
    assert!(!guard.current());
    assert!(guard.lost_while_owned(&engine.shared, 2));
    assert!(!guard.lost_while_owned(&engine.shared, 3));
    for begun in [None, Some((3, 1)), Some((2, 2))] {
        *engine.shared.begun.lock().unwrap() = begun;
        assert!(!guard.lost_while_owned(&engine.shared, 2));
    }
    *engine.shared.begun.lock().unwrap() = Some((2, 1));
    owner.revoke();
    assert!(!guard.lost_while_owned(&engine.shared, 2));
}

#[tokio::test]
async fn cached_private_hit_cannot_be_downgraded_by_neutral_final_system() {
    let script = Arc::new(Script {
        chats: Mutex::new(VecDeque::from([vec![
            text("Private answer."),
            Ok(ChatEvent::Done),
        ]])),
        ..Script::default()
    });
    let valid = Arc::new(AtomicBool::new(true));
    let (mut engine, speeches) = guarded(script.clone(), false, valid.clone());
    let owner = admission(true);
    let prepared = Prepared::begin(
        &engine.shared,
        Some(&owner.lease()),
        "question".into(),
        &[],
        Some("private hit".into()),
        true,
    );
    tokio::task::yield_now().await;
    valid.store(false, Ordering::Release);
    finish(&mut engine, &owner, false, Some(prepared), "question").await;
    assert_eq!(script.asked.lock().unwrap().len(), 1);
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn private_output_rechecks_authority_after_queue_backpressure() {
    let valid = Arc::new(AtomicBool::new(true));
    let (mut engine, _) = guarded(Arc::new(Script::default()), true, valid.clone());
    let owner = admission(true);
    let guard = PrivateContext::new(&engine.shared, Some(&owner.lease()), true);
    let event = || ProviderOutput::Transcript {
        request: Some(2),
        text: "placeholder".into(),
        finished: false,
    };
    while engine.shared.out.try_send(event().into()).is_ok() {}
    let shared = engine.shared.clone();
    let pending = tokio::spawn(async move { shared.emit_private(&guard, event()).await });
    tokio::task::yield_now().await;
    assert!(!pending.is_finished());
    valid.store(false, Ordering::Release);
    engine.output.recv().await.unwrap();
    assert!(
        !tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap()
    );
    assert_eq!(engine.output.len(), OUTPUT_QUEUE - 1);
}

#[tokio::test]
async fn private_held_forget_tool_completion_expiry_sends_no_first_chat() {
    let script = Arc::new(Script::default());
    let valid = Arc::new(AtomicBool::new(true));
    let (mut engine, speeches) = guarded(script.clone(), true, valid.clone());
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let shared = Arc::get_mut(&mut engine.shared).unwrap();
    shared.tools = Arc::new(move |_, _, _, _| {
        let valid = valid.clone();
        let counted = counted.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            counted.fetch_add(1, Ordering::Relaxed);
            valid.store(false, Ordering::Release);
            json!({"ok":true})
        })
    });
    shared.held_forget.lock().unwrap().held = Some(HeldForget {
        call: call("held", crate::remember::FORGET, json!({"fact":"fixture"})),
        request: 1,
        asker: (1, crate::remember::person_id("Fixture owner")),
        at: Instant::now(),
        asked: true,
    });
    let owner = admission(true);
    finish(&mut engine, &owner, false, None, "yes").await;
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(script.asked.lock().unwrap().is_empty());
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
}

#[test]
fn lost_private_authority_cannot_resume_on_a_later_positive_read() {
    let valid = Arc::new(AtomicBool::new(true));
    let (engine, _) = guarded(Arc::new(Script::default()), true, valid.clone());
    let owner = admission(true);
    let guard = PrivateContext::new(&engine.shared, Some(&owner.lease()), true);
    assert!(guard.current());
    valid.store(false, Ordering::Release);
    assert!(!guard.current());
    valid.store(true, Ordering::Release);
    assert!(!guard.current());
}

#[tokio::test]
async fn buffered_private_output_expires_at_both_receiver_boundaries_without_blocking_public_output()
 {
    for waiting in [false, true] {
        let valid = Arc::new(AtomicBool::new(true));
        let (mut engine, _) = guarded(Arc::new(Script::default()), true, valid.clone());
        let owner = admission(true);
        let guard = PrivateContext::new(&engine.shared, Some(&owner.lease()), true);
        assert!(
            engine
                .shared
                .emit_private(
                    &guard,
                    ProviderOutput::Transcript {
                        request: Some(2),
                        text: "private fixture".into(),
                        finished: false,
                    }
                )
                .await
        );
        valid.store(false, Ordering::Release);
        assert!(
            engine
                .shared
                .emit(ProviderOutput::TurnComplete {
                    request: 2,
                    idle: true
                })
                .await
        );
        let next = if waiting {
            tokio::time::timeout(Duration::from_secs(2), engine.next())
                .await
                .expect("expired output does not block the receiver")
        } else {
            engine
                .try_output()
                .expect("the next public event remains available")
        };
        assert!(matches!(
            next,
            ProviderOutput::TurnComplete { request: 2, .. }
        ));
        assert!(engine.try_output().is_none());
    }
}

#[derive(Default)]
struct LazySpeech {
    initialised: Mutex<Vec<String>>,
    polled: Arc<Mutex<Vec<String>>>,
    release: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    began: Option<Arc<tokio::sync::Notify>>,
    after_wait: Arc<AtomicUsize>,
}
impl Services for LazySpeech {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        panic!("no capture in the speech-boundary fixture")
    }
    fn chat(&self, _: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
        stream::empty().boxed()
    }
    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        self.initialised.lock().unwrap().push(text.clone());
        let polled = self.polled.clone();
        let release = self.release.lock().unwrap().take();
        let began = self.began.clone();
        let after_wait = self.after_wait.clone();
        stream::once(async move {
            polled.lock().unwrap().push(text);
            if let Some(began) = began {
                began.notify_one();
            }
            if let Some(release) = release {
                release.await.expect("fixture releases the stream");
                after_wait.fetch_add(1, Ordering::Relaxed);
            }
            Ok(vec![1])
        })
        .boxed()
    }
}
fn speech_engine(services: Arc<LazySpeech>, valid: Arc<AtomicBool>) -> Engine {
    Engine::new(
        services,
        Told::default,
        Arc::new(|_, _, _, _| panic!("no tools in the speech-boundary fixture")),
    )
    .guarding_private_context(Arc::new(move |_| valid.load(Ordering::Acquire).into()))
}

/// Real V3 source and transferred kernel witness for the TTS boundary rows.
/// The private parent and worker stay alive until every speech task has joined.
struct SpeechAuthority {
    fixture: crate::camera_authority::fixture::Fixture,
    revoked: AtomicBool,
    _directory: crate::process::SessionDirectory,
    known: std::path::PathBuf,
}
impl SpeechAuthority {
    fn new() -> Self {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let known = directory.path.canonicalize().unwrap().join("known");
        std::fs::write(&known, "Fixture owner").unwrap();
        let observed = lamp_ipc::monotonic_us();
        std::fs::write(
            crate::speaking_faces::known_speaker_of(&known),
            json!({
                "version":3,"state":"seen","name":"Fixture owner","id":null,
                "boot":"0123456789abcdef","face":"f7",
                "publisher":"0123456789abcdef0123456789abcdef","generation":1,
                "camera_hold":1,"camera_began_us":1,"observed_us":observed,
                "permission_until_us":observed.checked_add(1_500_000).unwrap()
            })
            .to_string(),
        )
        .unwrap();
        let fixture = crate::camera_authority::fixture::Fixture::start(
            known.clone(),
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ],
            1,
        );
        Self {
            fixture,
            revoked: AtomicBool::new(false),
            _directory: directory,
            known,
        }
    }
    fn owner(&self, private: bool) -> RequestScope {
        let mut owner = RequestScope::looking_later(
            RequestKey {
                worker: lamp_turn_owner::BootId::new([29; 16]).unwrap(),
                session: 1,
                request: 2,
            },
            if private {
                ScopeSnapshot::of("Fixture owner")
            } else {
                ScopeSnapshot::room()
            },
            self.fixture.cache(),
        );
        if private {
            let key = owner.lease().key();
            assert!(owner.speaker_recorder().unwrap().seal(
                key,
                &["0123456789abcdef/f7".into()],
                Instant::now() + Duration::from_secs(5),
            ));
            assert_eq!(self.fixture.ready(&owner.lease()), PermissionState::Ready);
        }
        owner
    }
    fn engine(&self, services: Arc<LazySpeech>) -> Engine {
        let known = self.known.clone();
        Engine::new(
            services,
            Told::default,
            Arc::new(|_, _, _, _| panic!("no tools in the speech-boundary fixture")),
        )
        .guarding_private_context(Arc::new(move |lease| {
            crate::second_provider::worker::private_context_state(Some(&known), lease)
        }))
    }
    fn revoke(&self) {
        let source = crate::speaking_faces::known_speaker_of(&self.known);
        let unchanged = std::fs::read(&source).unwrap();
        // The original flag-based fixture could repeat the same revocation.
        // Record only an actual successful kernel shutdown; do not ask Darwin
        // to shutdown an already closed witness a second time.
        if !self.revoked.load(Ordering::Acquire) {
            self.fixture.witness.revoke().unwrap();
            self.revoked.store(true, Ordering::Release);
        }
        assert_eq!(
            std::fs::read(source).unwrap(),
            unchanged,
            "kernel revocation does not depend on a source-file rewrite"
        );
    }
}

async fn finish_speech(speech: &mut JoinSet<()>) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(done) = speech.join_next().await {
            done.unwrap();
        }
    })
    .await
    .expect("the bounded speech fixture stops");
}

#[tokio::test]
async fn private_words_waiting_for_announcement_capacity_are_never_retained_or_sent_to_tts() {
    let services = Arc::new(LazySpeech::default());
    let authority = SpeechAuthority::new();
    let engine = authority.engine(services.clone());
    let owner = authority.owner(true);
    let guard = PrivateContext::new(&engine.shared, Some(&owner.lease()), true);
    let (sentences, mut queued) = mpsc::channel(1);
    sentences
        .try_send(("occupied".into(), mpsc::channel(1).1))
        .unwrap();
    let mut speech = JoinSet::new();
    let mut queue = Queue {
        private_context: guard,
        shared: &engine.shared,
        speech: &mut speech,
        announcement: Some(mpsc::channel(1).1),
        sentences,
        started: false,
        request: 2,
        heard_at: Instant::now(),
        tags: None,
    };
    let pushing = queue.push("private fixture".into());
    tokio::pin!(pushing);
    assert!(
        std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(pushing.as_mut(), cx))
        })
        .await
        .is_pending()
    );
    authority.revoke();
    assert!(owner.lease().is_current());
    assert_eq!(queued.recv().await.unwrap().0, "occupied");
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), pushing)
            .await
            .unwrap(),
        Err(Stop::Gone)
    ));
    assert!(engine.shared.lamp_said.lock().unwrap().1.is_empty());
    assert!(services.initialised.lock().unwrap().is_empty());
    assert!(services.polled.lock().unwrap().is_empty());
}

#[tokio::test]
async fn private_speech_rechecks_before_first_poll_with_positive_room_and_public_controls() {
    for case in [
        "already-expired",
        "expired-private",
        "positive-private",
        "room",
        "announcement",
    ] {
        let services = Arc::new(LazySpeech::default());
        let authority = SpeechAuthority::new();
        let engine = authority.engine(services.clone());
        let owner = authority.owner(case != "room");
        let guard = (case != "announcement")
            .then(|| PrivateContext::new(&engine.shared, Some(&owner.lease()), case != "room"));
        let mut speech = JoinSet::new();
        if case == "already-expired" {
            authority.revoke();
            assert!(owner.lease().is_current());
        }
        let mut packets = start_speech(
            &engine.shared,
            &mut speech,
            "fixture".into(),
            net::Turn {
                request: 2,
                heard_at: Instant::now(),
            },
            guard,
        );
        assert_eq!(
            services.initialised.lock().unwrap().len(),
            usize::from(case != "already-expired"),
            "construction remains synchronous and ordered when authorised"
        );
        if case != "positive-private" {
            authority.revoke();
            assert!(owner.lease().is_current());
        }
        let packet = tokio::time::timeout(Duration::from_secs(2), packets.recv())
            .await
            .unwrap();
        let dispatched = !matches!(case, "already-expired" | "expired-private");
        assert_eq!(packet.is_some(), dispatched, "{case}");
        assert_eq!(
            services.polled.lock().unwrap().len(),
            usize::from(dispatched),
            "{case}: lazy dispatch"
        );
        finish_speech(&mut speech).await;
    }
}

#[tokio::test]
async fn private_speech_packet_returning_after_expiry_is_not_enqueued() {
    let (release, waiting) = tokio::sync::oneshot::channel();
    let began = Arc::new(tokio::sync::Notify::new());
    let services = Arc::new(LazySpeech {
        release: Mutex::new(Some(waiting)),
        began: Some(began.clone()),
        ..LazySpeech::default()
    });
    let authority = SpeechAuthority::new();
    let engine = authority.engine(services.clone());
    let owner = authority.owner(true);
    let guard = PrivateContext::new(&engine.shared, Some(&owner.lease()), true);
    let mut speech = JoinSet::new();
    let mut packets = start_speech(
        &engine.shared,
        &mut speech,
        "fixture".into(),
        net::Turn {
            request: 2,
            heard_at: Instant::now(),
        },
        Some(guard),
    );
    tokio::time::timeout(Duration::from_secs(2), began.notified())
        .await
        .unwrap();
    authority.revoke();
    assert!(owner.lease().is_current());
    release.send(()).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), packets.recv())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(*services.initialised.lock().unwrap(), ["fixture"]);
    assert_eq!(*services.polled.lock().unwrap(), ["fixture"]);
    assert_eq!(
        services.after_wait.load(Ordering::Relaxed),
        0,
        "expired proof prevents the inner future's resumed poll"
    );
    finish_speech(&mut speech).await;
}

#[tokio::test]
async fn private_speech_piece_initialisation_preserves_queue_call_order() {
    let services = Arc::new(LazySpeech::default());
    let engine = speech_engine(services.clone(), Arc::new(AtomicBool::new(true)));
    let owner = admission(true);
    let guard = PrivateContext::new(&engine.shared, Some(&owner.lease()), true);
    let mut speech = JoinSet::new();
    let mut pieces = Vec::new();
    for text in ["first", "second", "third"] {
        pieces.push(start_speech(
            &engine.shared,
            &mut speech,
            text.into(),
            net::Turn {
                request: 2,
                heard_at: Instant::now(),
            },
            Some(guard.clone()),
        ));
    }
    assert_eq!(
        *services.initialised.lock().unwrap(),
        ["first", "second", "third"]
    );
    for mut piece in pieces {
        assert!(
            tokio::time::timeout(Duration::from_secs(2), piece.recv())
                .await
                .unwrap()
                .unwrap()
                .is_ok()
        );
    }
    finish_speech(&mut speech).await;
}

#[tokio::test]
async fn private_words_expiring_between_announcement_check_and_retained_words_lock_are_not_kept() {
    let services = Arc::new(LazySpeech::default());
    let valid = Arc::new(AtomicBool::new(true));
    let armed = Arc::new(AtomicBool::new(false));
    let (release, wait_release) = std::sync::mpsc::sync_channel(1);
    let proof = valid.clone();
    let trigger = armed.clone();
    let engine = Engine::new(
        services.clone(),
        Told::default,
        Arc::new(|_, _, _, _| panic!("no tools")),
    )
    .guarding_private_context(Arc::new(move |_| {
        let permitted = proof.load(Ordering::Acquire);
        if trigger.swap(false, Ordering::AcqRel) {
            release.try_send(()).unwrap();
        }
        permitted.into()
    }));
    let owner = admission(true);
    let guard = PrivateContext::new(&engine.shared, Some(&owner.lease()), true);
    let (sentences, mut queued) = mpsc::channel(1);
    sentences
        .try_send(("occupied".into(), mpsc::channel(1).1))
        .unwrap();
    let mut speech = JoinSet::new();
    let mut queue = Queue {
        private_context: guard,
        shared: &engine.shared,
        speech: &mut speech,
        announcement: Some(mpsc::channel(1).1),
        sentences,
        started: false,
        request: 2,
        heard_at: Instant::now(),
        tags: None,
    };
    let pushing = queue.push("private fixture".into());
    tokio::pin!(pushing);
    assert!(
        std::future::poll_fn(|cx| {
            std::task::Poll::Ready(std::future::Future::poll(pushing.as_mut(), cx))
        })
        .await
        .is_pending()
    );
    // Hold the actual retention mutex. The next validation snapshots true
    // before releasing this thread; authority expires while that lock is held.
    let shared = engine.shared.clone();
    let (ready, locked) = std::sync::mpsc::sync_channel(1);
    let holder = std::thread::spawn(move || {
        let held = shared.lamp_said.lock().unwrap();
        ready.send(()).unwrap();
        wait_release
            .recv_timeout(Duration::from_secs(2))
            .expect("the post-announcement check releases the fixture");
        valid.store(false, Ordering::Release);
        drop(held);
    });
    locked
        .recv_timeout(Duration::from_secs(2))
        .expect("the fixture owns the retention mutex");
    armed.store(true, Ordering::Release);
    assert_eq!(queued.recv().await.unwrap().0, "occupied");
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), pushing)
            .await
            .unwrap(),
        Err(Stop::Gone)
    ));
    holder.join().unwrap();
    assert!(engine.shared.lamp_said.lock().unwrap().1.is_empty());
    assert!(services.initialised.lock().unwrap().is_empty());
}

#[tokio::test]
async fn event_note_private_provenance_survives_neutral_told_and_async_expiry() {
    const EVENT: &str = "private event fixture canary";
    for (case, private, initially_valid, expire_in_callback) in [
        ("private entry expired", true, false, false),
        ("private return expired", true, true, true),
        ("private positive", true, true, false),
        ("genuine room", false, false, true),
    ] {
        let script = Arc::new(Script {
            chats: Mutex::new(VecDeque::from([vec![
                text("A bounded answer."),
                Ok(ChatEvent::Done),
            ]])),
            ..Script::default()
        });
        let valid = Arc::new(AtomicBool::new(initially_valid));
        let (engine, speeches) = guarded(script.clone(), false, valid.clone());
        let reads = Arc::new(AtomicUsize::new(0));
        let counted = reads.clone();
        let mut engine = engine.asking_after_events(
            Arc::new(move |_, _, _| {
                counted.fetch_add(1, Ordering::Relaxed);
                if expire_in_callback {
                    valid.store(false, Ordering::Release);
                }
                Some(EVENT.into())
            }),
            Arc::new(|| None),
        );
        let owner = admission(private);
        finish(&mut engine, &owner, false, None, "hello").await;
        let entered = !private || initially_valid;
        assert_eq!(
            reads.load(Ordering::Relaxed),
            usize::from(entered),
            "{case}: callback entry"
        );
        let terminal = private && initially_valid && expire_in_callback;
        let asked = script.asked.lock().unwrap();
        assert_eq!(
            asked.len(),
            usize::from(!terminal),
            "{case}: actual Services sends"
        );
        let disclosed = asked
            .iter()
            .flatten()
            .any(|message| message.words().is_some_and(|words| words.contains(EVENT)));
        assert_eq!(disclosed, entered && !terminal, "{case}: note disclosure");
        if terminal {
            assert_eq!(speeches.load(Ordering::Relaxed), 0, "{case}");
            assert!(
                engine.shared.lamp_said.lock().unwrap().1.is_empty(),
                "{case}"
            );
        }
    }
}

#[test]
fn queued_private_event_callback_rechecks_proof_before_read() {
    const EVENT: &str = "queued private event fixture canary";
    tokio::runtime::Builder::new_current_thread()
        .max_blocking_threads(1)
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            // Occupy only this runtime's sole blocking worker. No process-wide
            // pool saturation, sleeps or scheduler-speed assumptions are needed.
            let (entered, worker_entered) = tokio::sync::oneshot::channel();
            let (release, worker_release) = std::sync::mpsc::sync_channel(1);
            let occupied = tokio::task::spawn_blocking(move || {
                entered.send(()).unwrap();
                worker_release
                    .recv_timeout(Duration::from_secs(2))
                    .expect("the queued callback control releases the worker");
            });
            worker_entered.await.unwrap();
            let script = Arc::new(Script {
                chats: Mutex::new(VecDeque::from([vec![
                    text("A neutral answer."),
                    Ok(ChatEvent::Done),
                ]])),
                ..Script::default()
            });
            let valid = Arc::new(AtomicBool::new(true));
            let (engine, _) = guarded(script.clone(), false, valid.clone());
            let checks = Arc::new(AtomicUsize::new(0));
            let counted_checks = checks.clone();
            let check_valid = valid.clone();
            let (scheduled, scheduling_check) = tokio::sync::oneshot::channel();
            let first_check = Mutex::new(Some(scheduled));
            let reads = Arc::new(AtomicUsize::new(0));
            let counted_reads = reads.clone();
            let mut engine = engine
                .guarding_private_context(Arc::new(move |_| {
                    let current = check_valid.load(Ordering::Acquire);
                    if counted_checks.fetch_add(1, Ordering::Relaxed) == 0 {
                        assert!(current, "proof is valid at the scheduling guard");
                        first_check.lock().unwrap().take().unwrap().send(()).unwrap();
                    }
                    current.into()
                }))
                .asking_after_events(
                    Arc::new(move |_, _, _| {
                        counted_reads.fetch_add(1, Ordering::Relaxed);
                        Some(EVENT.into())
                    }),
                    Arc::new(|| None),
                );
            let owner = admission(true);
            {
                let answering = finish(&mut engine, &owner, false, None, "hello");
                tokio::pin!(answering);
                tokio::time::timeout(Duration::from_secs(2), async {
                    tokio::select! {
                        checked = scheduling_check => checked.unwrap(),
                        _ = &mut answering => panic!("the callback must wait behind the occupied worker"),
                    }
                }).await.expect("the actual event_line reached its scheduling guard");
                // The current-thread executor has polled event_line through
                // spawn_blocking's await; its read is queued, not yet started.
                assert_eq!(checks.load(Ordering::Relaxed), 1);
                assert_eq!(reads.load(Ordering::Relaxed), 0);
                valid.store(false, Ordering::Release);
                release.send(()).unwrap();
                answering.await;
            }
            occupied.await.unwrap();
            assert_eq!(checks.load(Ordering::Relaxed), 2, "entry rechecks after queueing");
            assert_eq!(reads.load(Ordering::Relaxed), 0, "expired proof permits no private read");
            let asked = script.asked.lock().unwrap();
            assert_eq!(asked.len(), 1, "neutral context remains answerable");
            assert!(!asked.iter().flatten().any(|message|
                message.words().is_some_and(|words| words.contains(EVENT))));
        });
}

/// Real Engine admission/current microphone proof, then the actual worker
/// validator observes loss and restoration before any Told private tag exists.
#[tokio::test]
async fn lost_request_before_told_keeps_public_completion_without_private_services() {
    struct RestoreThread(Option<crate::thread::Thread>);
    impl Drop for RestoreThread {
        fn drop(&mut self) {
            *crate::thread::THREAD.lock().unwrap() = self.0.take().unwrap();
        }
    }
    for lose in [false, true] {
        let _one = crate::provider_worker::tests::one_thread();
        let _thread = RestoreThread(Some(std::mem::take(
            &mut *crate::thread::THREAD.lock().unwrap(),
        )));
        let directory = crate::process::SessionDirectory::create().unwrap();
        let known = directory.path.canonicalize().unwrap().join("known");
        std::fs::write(&known, "A").unwrap();
        let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
        let authority = crate::camera_authority::fixture::Fixture::start(
            known.clone(),
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ],
            1,
        );
        let authority = Arc::new(authority);
        let cache = Arc::new(crate::memory_scope::FaceCache::default());
        assert!(cache.set_camera_client(authority.client.clone()));
        let script = Arc::new(Script {
            heard: Mutex::new(VecDeque::from([Ok("What did I keep?".into())])),
            chats: Mutex::new(VecDeque::from([vec![
                text("A bounded answer."),
                Ok(ChatEvent::Done),
            ]])),
            ..Script::default()
        });
        let speeches = Arc::new(AtomicUsize::new(0));
        let services = Arc::new(Trapped {
            script: script.clone(),
            speeches: speeches.clone(),
            expire_on_event: None,
        });
        let contexts = Arc::new(AtomicUsize::new(0));
        let counted = contexts.clone();
        let dispatch_known = known.clone();
        let note_known = known.clone();
        let note_authority = authority.clone();
        let mut engine = Engine::new(
            services,
            Told::default,
            Arc::new(|_, _, _, _| panic!("no unscoped tool in the disclosure control")),
        )
        .with_memory_scope(
            lamp_turn_owner::BootId::new([9; 16]).unwrap(),
            cache.clone(),
            Arc::new(|_, _, _, _, _| panic!("no tool in the disclosure control")),
            Some(Arc::new(move |_| {
                counted.fetch_add(1, Ordering::Relaxed);
                Told {
                    text: "PRIVATE PRE-TOLD CANARY".into(),
                    requires_speaker: true,
                    pending_private_context: false,
                    ..Told::default()
                }
            })),
        )
        .seeing_who_speaks(eyes.clone(), known.clone())
        .guarding_private_context(Arc::new(move |lease| {
            super::super::worker::private_context_state(Some(&dispatch_known), lease)
        }))
        .asking_the_note_ahead(NoteAhead {
            begin: Arc::new(|_| {}),
            ready: Arc::new(move |lease| {
                let known = note_known.clone();
                let authority = note_authority.clone();
                Box::pin(async move {
                    let lease = lease.expect("a request-owned final note admission");
                    // Acquire the real bounded ticket after the original
                    // Engine audio seal, before the existing worker check.
                    assert_eq!(authority.ready(&lease), PermissionState::Ready);
                    assert_eq!(lease.speaker(), Some(GAP_OWNER_SPEAKER));
                    assert!(super::super::worker::private_context_current(
                        Some(&known),
                        &lease
                    ));
                    if lose {
                        std::fs::remove_file(crate::speaking_faces::known_speaker_of(&known))
                            .unwrap();
                        assert!(!super::super::worker::private_context_current(
                            Some(&known),
                            &lease
                        ));
                        assert!(lease.private_context_lost());
                        assert!(lease.is_current());
                        assert_eq!(lease.speaker(), Some(GAP_OWNER_SPEAKER));
                        // The same data returning cannot restore this request.
                        gap_owner_observation_v3(&known);
                        assert!(!super::super::worker::private_context_current(
                            Some(&known),
                            &lease
                        ));
                    }
                })
            }),
            forget: Arc::new(|| {}),
        })
        .announcing(false);
        engine.memory_camera(true, 1);
        gap_owner_observation_v3(&known);
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
        let events = turn(&mut engine).await;
        assert_eq!(
            events.last().map(String::as_str),
            Some("complete"),
            "still-owned request needs its public terminal, lose={lose}"
        );
        assert!(lease.is_current());
        assert_eq!(lease.private_context_lost(), lose);
        assert_eq!(
            contexts.load(Ordering::Relaxed),
            usize::from(!lose),
            "lost disclosure is noticed before Told can tag or compose private context"
        );
        assert_eq!(script.asked.lock().unwrap().len(), usize::from(!lose));
        assert_eq!(speeches.load(Ordering::Relaxed), usize::from(!lose));
        if lose {
            assert!(engine.shared.lamp_said.lock().unwrap().1.is_empty());
            assert!(!events.iter().any(|event| event.starts_with("audio:")));
        } else {
            assert!(
                script
                    .asked
                    .lock()
                    .unwrap()
                    .iter()
                    .flatten()
                    .any(|message| message
                        .words()
                        .is_some_and(|words| words.contains("PRIVATE PRE-TOLD CANARY")))
            );
        }
    }
}

/// The short action line keeps the original private request's proof across
/// both the tool result and the empty model round. These are actual Engine
/// turns with accepted microphone frames and the real known-speaker validator.
#[tokio::test]
async fn short_done_after_a_private_tool_keeps_fresh_speaker_permission() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Lose {
        Never,
        ToolResult,
        EmptyRound,
    }
    struct ShortServices {
        script: Arc<Script>,
        known: std::path::PathBuf,
        lose: Lose,
        rounds: AtomicUsize,
        losses: Arc<AtomicUsize>,
        spoken: Arc<Mutex<Vec<String>>>,
    }
    impl Services for ShortServices {
        fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
            self.script.listen()
        }
        fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
            let round = self.rounds.fetch_add(1, Ordering::Relaxed);
            let remove = self.lose == Lose::EmptyRound && round == 1;
            let known = self.known.clone();
            let losses = self.losses.clone();
            self.script
                .chat(messages)
                .map(move |event| {
                    if remove && matches!(&event, Ok(ChatEvent::Done)) {
                        // The second send was permitted. Its empty answer
                        // arrives only after the fresh source has disappeared.
                        std::fs::remove_file(crate::speaking_faces::known_speaker_of(&known))
                            .unwrap();
                        losses.fetch_add(1, Ordering::Relaxed);
                    }
                    event
                })
                .boxed()
        }
        fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
            self.spoken.lock().unwrap().push(text.clone());
            self.script.speak(text)
        }
    }
    struct RestoreThread(Option<crate::thread::Thread>);
    impl Drop for RestoreThread {
        fn drop(&mut self) {
            *crate::thread::THREAD.lock().unwrap() = self.0.take().unwrap();
        }
    }
    const LINE: &str = "Brighter.";
    const RESULT: &str = "PRIVATE ACTION RECEIPT";
    for lose in [Lose::Never, Lose::ToolResult, Lose::EmptyRound] {
        let _one = crate::provider_worker::tests::one_thread();
        let _thread = RestoreThread(Some(std::mem::take(
            &mut *crate::thread::THREAD.lock().unwrap(),
        )));
        let directory = crate::process::SessionDirectory::create().unwrap();
        let known = directory.path.canonicalize().unwrap().join("known");
        std::fs::write(&known, "A").unwrap();
        let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
        let authority = crate::camera_authority::fixture::Fixture::start(
            known.clone(),
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ],
            1,
        );
        let cache = Arc::new(crate::memory_scope::FaceCache::default());
        assert!(cache.set_camera_client(authority.client.clone()));
        let script = Arc::new(Script {
            heard: Mutex::new(VecDeque::from([Ok("Make it brighter.".into())])),
            chats: Mutex::new(VecDeque::from([
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "light",
                        "light_brighter",
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![Ok(ChatEvent::Done)],
            ])),
            ..Script::default()
        });
        let spoken = Arc::new(Mutex::new(Vec::new()));
        let losses = Arc::new(AtomicUsize::new(0));
        let services = Arc::new(ShortServices {
            script: script.clone(),
            known: known.clone(),
            lose,
            rounds: AtomicUsize::new(0),
            losses: losses.clone(),
            spoken: spoken.clone(),
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let tool_known = known.clone();
        let tool_losses = losses.clone();
        let runner: ScopedToolRunner = Arc::new(move |request, heard, name, _, lease| {
            assert_eq!(request, 1);
            assert!(heard);
            assert_eq!(name, "light_brighter");
            assert_eq!(lease.speaker(), Some(GAP_OWNER_SPEAKER));
            assert!(super::super::worker::private_context_current(
                Some(&tool_known),
                &lease
            ));
            counted.fetch_add(1, Ordering::Relaxed);
            let answer = json!({
                "result": RESULT,
                "next": crate::second_provider::probe::tools::SHORT_NEXT,
                "say": LINE,
            });
            if lose == Lose::ToolResult {
                // The actual scoped call has produced its result. Permission
                // is lost before the Engine can use it for another send.
                std::fs::remove_file(crate::speaking_faces::known_speaker_of(&tool_known)).unwrap();
                tool_losses.fetch_add(1, Ordering::Relaxed);
            }
            Box::pin(async move { answer })
        });
        let dispatch_known = known.clone();
        let mut engine = Engine::new(
            services.clone(),
            Told::default,
            Arc::new(|_, _, _, _| panic!("the scoped runner must own the action")),
        )
        .with_memory_scope(
            lamp_turn_owner::BootId::new([9; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|_| Told {
                text: "PRIVATE SHORT-DONE CONTEXT".into(),
                requires_speaker: true,
                pending_private_context: false,
                ..Told::default()
            })),
        )
        .seeing_who_speaks(eyes.clone(), known.clone())
        .guarding_private_context(Arc::new(move |lease| {
            super::super::worker::private_context_state(Some(&dispatch_known), lease)
        }))
        .announcing(false)
        .answering_short(true);
        engine.memory_camera(true, 1);
        gap_owner_observation_v3(&known);
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
        let events = turn(&mut engine).await;
        tokio::time::timeout(
            Duration::from_secs(2),
            &mut engine.turn.as_mut().unwrap().task,
        )
        .await
        .expect("the actual Engine task completes")
        .unwrap();
        assert_eq!(
            events.iter().filter(|event| *event == "complete").count(),
            1,
            "{lose:?}"
        );
        assert!(
            engine.try_output().is_none(),
            "one public terminal: {lose:?}"
        );
        assert!(lease.is_current(), "proof loss is not owner revocation");
        assert_eq!(lease.speaker(), Some(GAP_OWNER_SPEAKER));
        assert_eq!(
            lease.private_context_lost(),
            lose != Lose::Never,
            "{lose:?}"
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1, "{lose:?}");
        assert_eq!(
            losses.load(Ordering::Relaxed),
            usize::from(lose != Lose::Never)
        );
        let asked = script.asked.lock().unwrap();
        let expected_rounds = if lose == Lose::ToolResult { 1 } else { 2 };
        assert_eq!(
            asked.len(),
            expected_rounds,
            "no post-loss backend send: {lose:?}"
        );
        assert_eq!(services.rounds.load(Ordering::Relaxed), expected_rounds);
        assert!(
            asked[0].iter().any(|message| message
                .words()
                .is_some_and(|words| words.contains("PRIVATE SHORT-DONE CONTEXT"))),
            "the actual initial request was private"
        );
        assert!(
            !asked[0]
                .iter()
                .any(|message| message.words().is_some_and(|words| words.contains(RESULT)))
        );
        if expected_rounds == 2 {
            assert!(
                asked[1]
                    .iter()
                    .any(|message| message.words().is_some_and(|words| words.contains(RESULT))),
                "the permitted second round received the actual tool result"
            );
            assert!(
                asked[1].iter().any(|message| message
                    .words()
                    .is_some_and(|words| words.contains(&short_done_note(LINE)))),
                "this was the short-done extra round"
            );
        }
        if lose == Lose::Never {
            assert_eq!(*spoken.lock().unwrap(), [LINE.to_owned()]);
            assert_eq!(
                events
                    .iter()
                    .filter(|event| *event == &format!("said:{LINE}"))
                    .count(),
                1
            );
            assert!(events.iter().any(|event| event.starts_with("audio:")));
            assert_eq!(engine.shared.lamp_said.lock().unwrap().1.trim(), LINE);
        } else {
            assert!(spoken.lock().unwrap().is_empty(), "no TTS: {lose:?}");
            assert!(
                !events
                    .iter()
                    .any(|event| event.starts_with("said:") || event.starts_with("audio:")),
                "no private action output: {lose:?}"
            );
            assert!(
                engine.shared.lamp_said.lock().unwrap().1.is_empty(),
                "no retained action line: {lose:?}"
            );
        }
    }
}

#[tokio::test]
async fn returned_private_event_waits_without_rereading_then_resumes_or_ends() {
    const EVENT: &str = "returned private event pending canary";
    for resume in [true, false] {
        let script = Arc::new(Script {
            chats: Mutex::new(VecDeque::from([vec![
                text("A bounded answer."),
                Ok(ChatEvent::Done),
            ]])),
            ..Script::default()
        });
        let (engine, speeches) = guarded(script.clone(), false, Arc::new(AtomicBool::new(true)));
        // The callback returns an actual private value while its exact
        // request is Pending. This is not a callback that was never entered.
        let state = Arc::new(std::sync::atomic::AtomicU8::new(0));
        let checked_state = state.clone();
        let callback_state = state.clone();
        let (noticed, observed_pending) = tokio::sync::oneshot::channel();
        let noticed = Mutex::new(Some(noticed));
        let reads = Arc::new(AtomicUsize::new(0));
        let counted_reads = reads.clone();
        let mut engine = engine
            .guarding_private_context(Arc::new(move |_| {
                match checked_state.load(Ordering::Acquire) {
                    0 => PermissionState::Ready,
                    1 => {
                        if let Some(noticed) = noticed.lock().unwrap().take() {
                            noticed.send(()).unwrap();
                        }
                        PermissionState::Pending
                    }
                    _ => {
                        // The source can be Ready again immediately after the
                        // observed loss; that must not revive this answer.
                        checked_state.store(0, Ordering::Release);
                        PermissionState::Lost
                    }
                }
            }))
            .asking_after_events(
                Arc::new(move |_, _, _| {
                    counted_reads.fetch_add(1, Ordering::Relaxed);
                    callback_state.store(1, Ordering::Release);
                    Some(EVENT.into())
                }),
                Arc::new(|| None),
            );
        let owner = admission(true);
        let shared = engine.shared.clone();
        // A still-owned turn receives its public terminal even after losing
        // private disclosure permission; loss does not retire its microphone.
        *shared.begun.lock().unwrap() = Some((2, 1));
        let answering = answer(
            &shared,
            (2, Some(owner.lease())),
            "hello".into(),
            Asker::Spoken,
            None,
            None,
            Vec::new(),
            false,
            Times::new(Instant::now()),
            None,
            None,
        );
        tokio::pin!(answering);
        tokio::time::timeout(Duration::from_secs(2), async {
            tokio::select! {
                noticed = observed_pending => noticed.unwrap(),
                _ = &mut answering => panic!("the returned note must wait for authority"),
            }
        })
        .await
        .expect("the real answer observes Pending after its callback returns");
        assert_eq!(reads.load(Ordering::Relaxed), 1);
        assert!(script.asked.lock().unwrap().is_empty());
        assert_eq!(speeches.load(Ordering::Relaxed), 0);
        assert!(shared.lamp_said.lock().unwrap().1.is_empty());
        assert!(
            std::future::poll_fn(|cx| {
                std::task::Poll::Ready(std::future::Future::poll(answering.as_mut(), cx))
            })
            .await
            .is_pending(),
            "another poll cannot bypass Pending"
        );
        assert!(script.asked.lock().unwrap().is_empty());
        state.store(if resume { 0 } else { 2 }, Ordering::Release);
        let mut events = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::select! {
                    _ = &mut answering => break,
                    event = engine.output.recv() => events.push(event.unwrap().event),
                }
            }
        })
        .await
        .expect("return or loss finishes the answer within its original ownership");
        while let Ok(event) = engine.output.try_recv() {
            events.push(event.event);
        }
        assert_eq!(
            reads.load(Ordering::Relaxed),
            1,
            "do not repeat the event decision"
        );
        let asked = script.asked.lock().unwrap();
        assert_eq!(asked.len(), usize::from(resume), "backend sends: {resume}");
        let mentions = asked
            .iter()
            .flatten()
            .filter(|message| message.words().is_some_and(|words| words.contains(EVENT)))
            .count();
        assert_eq!(
            mentions,
            usize::from(resume),
            "the retained note arrives once"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProviderOutput::TurnComplete { request: 2, .. }))
                .count(),
            1,
            "one public terminal for the still-owned request: {resume}"
        );
        assert!(owner.lease().is_current());
        if !resume {
            assert_eq!(speeches.load(Ordering::Relaxed), 0);
            assert!(shared.lamp_said.lock().unwrap().1.is_empty());
            assert!(!events.iter().any(|event| matches!(
                event,
                ProviderOutput::Audio { .. }
                    | ProviderOutput::Transcript { .. }
                    | ProviderOutput::GenerationComplete { .. }
            )));
        }
    }
}

#[tokio::test]
async fn days_event_lock_pending_is_retried_without_replaying_its_decision() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum End {
        Greet,
        Request,
        Lost,
        Revoked,
    }
    for end in [End::Greet, End::Request, End::Lost, End::Revoked] {
        let fixture = Arc::new(crate::remember::event_read_fixture(&format!(
            "engine-event-lock-{end:?}"
        )));
        assert_eq!(fixture.decision(), None);
        let script = Arc::new(Script {
            chats: Mutex::new(VecDeque::from([vec![
                text("A bounded answer."),
                Ok(ChatEvent::Done),
            ]])),
            ..Script::default()
        });
        let valid = Arc::new(AtomicBool::new(true));
        let (engine, speeches) = guarded(script.clone(), false, valid.clone());
        let owner = admission(true);
        let lease = owner.lease();
        let reads = Arc::new(AtomicUsize::new(0));
        let checks = Arc::new(AtomicUsize::new(0));
        let callback_reads = reads.clone();
        let permission_checks = checks.clone();
        let (before_lock, waiting_for_lock) = tokio::sync::oneshot::channel();
        let before_lock = Mutex::new(Some(before_lock));
        let permission_valid = valid.clone();
        let permitted: crate::remember::NotePermission = Arc::new(move || {
            let check = permission_checks.fetch_add(1, Ordering::Relaxed);
            if check == 0 {
                before_lock
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send(())
                    .unwrap();
                return PermissionState::Ready;
            }
            if !lease.is_current() {
                return PermissionState::Lost;
            }
            if check == 1 {
                if end == End::Lost {
                    permission_valid.store(false, Ordering::Release);
                    return PermissionState::Lost;
                }
                // Checking was observed under the real Days mutex, but Seen
                // is already Ready by the time the callback returns. Only an
                // explicit result can preserve this unfinished selection.
                return PermissionState::Pending;
            }
            PermissionState::Ready
        });
        let callback_fixture = fixture.clone();
        let mut engine = engine.asking_after_events_permitted(
            Arc::new(move |_, heard, _, until| {
                callback_reads.fetch_add(1, Ordering::Relaxed);
                let authority = permitted.clone();
                let bounded: crate::remember::NotePermission = Arc::new(move || {
                    if tokio::time::Instant::now() >= until {
                        PermissionState::Lost
                    } else {
                        authority()
                    }
                });
                callback_fixture.read(heard, &bounded)
            }),
            Arc::new(|| None),
        );
        let held_fixture = fixture.clone();
        let (locked, has_lock) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let holder = std::thread::spawn(move || {
            held_fixture.with_context_held(|| {
                locked.send(()).unwrap();
                released
                    .recv_timeout(Duration::from_secs(3))
                    .expect("release the real Days mutex");
            });
        });
        has_lock.await.unwrap();
        let shared = engine.shared.clone();
        *shared.begun.lock().unwrap() = Some((2, 1));
        let words = if end == End::Request {
            "What time is it?"
        } else {
            "Good evening!"
        };
        let answering = answer(
            &shared,
            (2, Some(owner.lease())),
            words.into(),
            Asker::Spoken,
            None,
            None,
            Vec::new(),
            false,
            Times::new(Instant::now()),
            None,
            None,
        );
        tokio::pin!(answering);
        tokio::time::timeout(Duration::from_secs(2), async {
            tokio::select! {
                ready = waiting_for_lock => ready.unwrap(),
                _ = &mut answering => panic!("the actual event read must reach its first permission fence"),
            }
        }).await.unwrap();
        assert!(
            script.asked.lock().unwrap().is_empty(),
            "no send while Days is locked"
        );
        assert_eq!(reads.load(Ordering::Relaxed), 1);
        if end == End::Revoked {
            owner.revoke();
        }
        release.send(()).unwrap();
        let mut events = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::select! {
                    _ = &mut answering => break,
                    event = engine.output.recv() => events.push(event.unwrap().event),
                }
            }
        })
        .await
        .expect("Pending releases the blocking worker and completes or cancels");
        holder.join().unwrap();
        while let Ok(event) = engine.output.try_recv() {
            events.push(event.event);
        }
        let ready = matches!(end, End::Greet | End::Request);
        assert_eq!(
            reads.load(Ordering::Relaxed),
            if ready { 2 } else { 1 },
            "{end:?}"
        );
        assert_eq!(
            checks.load(Ordering::Relaxed),
            if ready { 4 } else { 2 },
            "{end:?}"
        );
        assert_eq!(
            fixture.decision(),
            match end {
                End::Greet => Some("greeted"),
                End::Request => Some("withheld"),
                _ => None,
            },
            "{end:?}"
        );
        assert_eq!(
            fixture.taken_back(),
            usize::from(end == End::Request),
            "one job at most: {end:?}"
        );
        let asked = script.asked.lock().unwrap();
        assert_eq!(asked.len(), usize::from(ready), "{end:?}");
        assert_eq!(
            asked
                .iter()
                .flatten()
                .filter(|message| message
                    .words()
                    .is_some_and(|words| words.contains(crate::remember::EVENT_TURN_LINE)))
                .count(),
            usize::from(end == End::Greet),
            "{end:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProviderOutput::TurnComplete { request: 2, .. }))
                .count(),
            usize::from(end != End::Revoked),
            "{end:?}"
        );
        if !ready {
            assert_eq!(speeches.load(Ordering::Relaxed), 0);
            assert!(shared.lamp_said.lock().unwrap().1.is_empty());
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CeilingPendingAt {
    Entry,
    AfterNote,
    AfterEvent,
}

async fn pending_private_answer_keeps_its_ceiling(at: CeilingPendingAt, late_ready: bool) {
    let script = Arc::new(Script::default());
    let (engine, speeches) = guarded(script.clone(), false, Arc::new(AtomicBool::new(true)));
    let state = Arc::new(std::sync::atomic::AtomicU8::new(u8::from(
        at == CeilingPendingAt::Entry,
    )));
    let checked = state.clone();
    let (noticed, observed_pending) = tokio::sync::oneshot::channel();
    let noticed = Mutex::new(Some(noticed));
    let note_reads = Arc::new(AtomicUsize::new(0));
    let note_count = note_reads.clone();
    let note_state = state.clone();
    let event_reads = Arc::new(AtomicUsize::new(0));
    let event_count = event_reads.clone();
    let event_state = state.clone();
    let event_deadline = Arc::new(Mutex::new(None));
    let recorded_deadline = event_deadline.clone();
    let mut engine = engine
        .guarding_private_context(Arc::new(move |_| {
            if checked.load(Ordering::Acquire) == 1 {
                if let Some(noticed) = noticed.lock().unwrap().take() {
                    let _ = noticed.send(());
                }
                PermissionState::Pending
            } else {
                PermissionState::Ready
            }
        }))
        .asking_the_note_ahead(NoteAhead {
            begin: Arc::new(|_| {}),
            ready: Arc::new(move |_| {
                let count = note_count.clone();
                let state = note_state.clone();
                Box::pin(async move {
                    count.fetch_add(1, Ordering::Relaxed);
                    if at == CeilingPendingAt::AfterNote {
                        state.store(1, Ordering::Release);
                    }
                })
            }),
            forget: Arc::new(|| {}),
        })
        .asking_after_events_permitted(
            Arc::new(move |_, _, _, until| {
                event_count.fetch_add(1, Ordering::Relaxed);
                *recorded_deadline.lock().unwrap() = Some(until);
                event_state.store(1, Ordering::Release);
                crate::remember::EventNote::Ready(Some(
                    "private returned event ceiling canary".into(),
                ))
            }),
            Arc::new(|| None),
        );
    let owner = admission(true);
    let shared = engine.shared.clone();
    *shared.begun.lock().unwrap() = Some((2, 1));
    let private_note = at != CeilingPendingAt::AfterEvent;
    let answering = answer(
        &shared,
        (2, Some(owner.lease())),
        "hello".into(),
        Asker::Spoken,
        None,
        None,
        if private_note {
            vec!["private input ceiling canary".into()]
        } else {
            Vec::new()
        },
        private_note,
        Times::new(Instant::now()),
        None,
        None,
    );
    tokio::pin!(answering);
    // Keep the real clock running until the actual callback has returned
    // through spawn_blocking and its result's post-return guard is observed.
    // A signal sent from inside that callback alone would be insufficient.
    tokio::time::timeout(Duration::from_secs(2), async {
        tokio::select! {
            observed = observed_pending => observed.unwrap(),
            _ = &mut answering => panic!("the actual answer must reach its private Pending boundary: {at:?}"),
        }
    }).await.expect("observe the actual blocked answer before pausing time");
    assert_eq!(
        note_reads.load(Ordering::Relaxed),
        usize::from(at != CeilingPendingAt::Entry)
    );
    assert_eq!(
        event_reads.load(Ordering::Relaxed),
        usize::from(at == CeilingPendingAt::AfterEvent)
    );
    assert!(script.asked.lock().unwrap().is_empty());
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
    // No std::Instant proof expiry is simulated: this is the Engine's
    // independent Tokio first-response ceiling with an injected validator.
    tokio::time::pause();
    let until = event_deadline
        .lock()
        .unwrap()
        .unwrap_or_else(|| tokio::time::Instant::now() + BEGIN_WITHIN);
    let remaining = until.saturating_duration_since(tokio::time::Instant::now());
    tokio::time::advance(remaining + Duration::from_millis(1)).await;
    if late_ready {
        // Restore after the deadline but BEFORE the answer's next poll: a
        // Ready child can otherwise beat timeout_at's already-expired timer.
        state.store(0, Ordering::Release);
    }
    let mut events = Vec::new();
    let completed = tokio::time::timeout(Duration::from_millis(1), async {
        loop {
            tokio::select! {
                _ = &mut answering => break,
                event = engine.output.recv() => events.push(event.unwrap().event),
            }
        }
    })
    .await;
    assert!(
        completed.is_ok(),
        "Pending must end by the original response ceiling, not wait for the separate speaker expiry: {at:?}"
    );
    while let Ok(event) = engine.output.try_recv() {
        events.push(event.event);
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                ProviderOutput::TurnComplete {
                    request: 2,
                    idle: true
                }
            ))
            .count(),
        1,
        "one public terminal: {at:?}"
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        ProviderOutput::Audio { .. }
            | ProviderOutput::Transcript { .. }
            | ProviderOutput::GenerationComplete { .. }
    )));
    assert!(script.asked.lock().unwrap().is_empty());
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
    assert!(shared.lamp_said.lock().unwrap().1.is_empty());
    assert!(
        owner.lease().is_current(),
        "a response timeout is not microphone retirement"
    );
    // A later matching Seen cannot restart the completed answer or replay
    // the completed event decision. No new request is manufactured here.
    state.store(0, Ordering::Release);
    tokio::task::yield_now().await;
    assert!(engine.output.try_recv().is_err());
    assert!(script.asked.lock().unwrap().is_empty());
    assert_eq!(speeches.load(Ordering::Relaxed), 0);
    assert_eq!(
        event_reads.load(Ordering::Relaxed),
        usize::from(at == CeilingPendingAt::AfterEvent)
    );
    assert_eq!(
        note_reads.load(Ordering::Relaxed),
        usize::from(at != CeilingPendingAt::Entry)
    );
    tokio::time::resume();
}

#[tokio::test]
async fn returned_private_event_pending_obeys_the_original_response_ceiling() {
    for late_ready in [false, true] {
        pending_private_answer_keeps_its_ceiling(CeilingPendingAt::AfterEvent, late_ready).await;
    }
}

#[tokio::test]
async fn private_pending_before_and_after_note_ready_keeps_the_original_response_ceiling() {
    for at in [CeilingPendingAt::Entry, CeilingPendingAt::AfterNote] {
        for late_ready in [false, true] {
            pending_private_answer_keeps_its_ceiling(at, late_ready).await;
        }
    }
}
