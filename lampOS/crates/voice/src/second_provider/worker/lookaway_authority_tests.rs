//! Explicit no-detection retains an already proved request, never a new grant.
use super::*;
use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
use crate::second_provider::{ChatEvent, Listener, PrivateContext, Services, net};
use futures_util::{
    StreamExt,
    future::BoxFuture,
    stream::{self, BoxStream},
};
use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, Controller, Permission};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const NAME: &str = "Fixture owner";
const BOOT: &str = "0123456789abcdef";
const PUBLISHER: &str = "0123456789abcdef0123456789abcdef";
const SPEAKER: &str = "0123456789abcdef/f7";
const CANARY: &str = "PRIVATE LOOKAWAY FACT";

struct Source {
    _directory: crate::process::SessionDirectory,
    known: PathBuf,
}
impl Source {
    fn new() -> Self {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let known = directory.path.join("known");
        std::fs::write(&known, NAME).unwrap();
        std::fs::write(crate::speaking_faces::known_face_of(&known), "f7").unwrap();
        let this = Self {
            _directory: directory,
            known,
        };
        this.publish(Self::seen());
        this
    }
    fn seen() -> Value {
        json!({"state":"seen","name":NAME,"id":null,"boot":BOOT,"face":"f7",
            "publisher":PUBLISHER,"generation":1})
    }
    fn absent() -> Value {
        json!({"state":"absent","boot":BOOT,"publisher":PUBLISHER,"generation":1})
    }
    fn publish(&self, value: Value) {
        publish(&self.known, &value);
    }
}
fn publish(known: &Path, value: &Value) {
    let source = crate::speaking_faces::known_speaker_of(known);
    let temporary = source.with_extension("new");
    std::fs::write(&temporary, value.to_string()).unwrap();
    std::fs::rename(temporary, source).unwrap();
}
fn owner(request: u64, deadline: Instant) -> RequestScope {
    let mut owner = RequestScope::new(
        RequestKey {
            worker: BootId::new([52; 16]).unwrap(),
            session: 1,
            request,
        },
        ScopeSnapshot::of(NAME),
    );
    let key = owner.lease().key();
    assert!(
        owner
            .speaker_recorder()
            .unwrap()
            .seal(key, &[SPEAKER.into()], deadline)
    );
    owner
}
fn current(source: &Source, owner: &RequestScope) -> bool {
    private_context_current(Some(&source.known), &owner.lease())
}

#[test]
fn explicit_absence_is_request_owned_and_never_grants_an_inherited_private_scope() {
    let source = Source::new();
    let original = owner(1, Instant::now() + Duration::from_secs(5));
    let began = Instant::now();
    assert!(current(&source, &original));
    let initial = began.elapsed();
    source.publish(Source::absent());
    let began = Instant::now();
    assert!(current(&source, &original));
    let continuation = began.elapsed();
    eprintln!(
        "private source host lookup: initial {} us, continuation {} us; bounded local file parse only, target under 5 ms, no device/acoustic claim",
        initial.as_micros(),
        continuation.as_micros()
    );
    assert!(!known_name_told(true, Some(&source.known), Some(&original.lease())).is_empty());
    let next = owner(2, Instant::now() + Duration::from_secs(5));
    assert!(
        !current(&source, &next),
        "new positive audio still needs its own Seen match"
    );
    assert!(
        !next.lease().private_context_lost(),
        "unproved stays neutral"
    );
    source.publish(Source::seen());
    assert!(current(&source, &next));
    source.publish(Source::absent());
    assert!(current(&source, &next));
    original.revoke();
    assert!(!current(&source, &original));
    assert!(
        current(&source, &next),
        "requests share no cached authorization"
    );
}

#[test]
fn explicit_absence_never_heals_loss_or_downgrades_errors_and_legacy_sources() {
    for case in [
        "revoked",
        "generation",
        "publisher",
        "boot",
        "face",
        "person",
        "missing",
        "malformed",
        "mixed",
        "stale",
        "future",
        "legacy",
    ] {
        let source = Source::new();
        let admitted = owner(1, Instant::now() + Duration::from_secs(5));
        assert!(current(&source, &admitted));
        let mut changed = Source::seen();
        match case {
            "revoked" => {
                changed = Source::absent();
                changed["state"] = json!("revoked");
            }
            "generation" => {
                changed = Source::absent();
                changed["generation"] = json!(2);
            }
            "publisher" => {
                changed = Source::absent();
                changed["publisher"] = json!("f".repeat(32));
            }
            "boot" => {
                changed = Source::absent();
                changed["boot"] = json!("fedcba9876543210");
            }
            "face" => {
                // A new track observed before completed recognition is a
                // denial, unlike an already re-recognized same-person return.
                let mut checking = Source::absent();
                checking["state"] = json!("checking");
                source.publish(checking);
                assert!(!current(&source, &admitted), "unrecognized returning track");
                changed["face"] = json!("f8");
            }
            "person" => changed["name"] = json!("Different fixture"),
            "malformed" => {
                changed = Source::absent();
                changed["generation"] = json!(0);
            }
            "mixed" => {
                changed = Source::absent();
                changed["name"] = json!(NAME);
            }
            "legacy" => {
                changed.as_object_mut().unwrap().remove("state");
                changed.as_object_mut().unwrap().remove("publisher");
                changed.as_object_mut().unwrap().remove("generation");
            }
            _ => (),
        }
        source.publish(changed);
        let path = crate::speaking_faces::known_speaker_of(&source.known);
        if case == "missing" {
            std::fs::remove_file(&path).unwrap();
        }
        if matches!(case, "stale" | "future") {
            let modified = if case == "stale" {
                SystemTime::now() - Duration::from_secs(4)
            } else {
                SystemTime::now() + Duration::from_secs(60)
            };
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(modified))
                .unwrap();
        }
        assert!(!current(&source, &admitted), "{case}");
        assert!(
            admitted.lease().is_current(),
            "read loss is not microphone revocation: {case}"
        );
        assert!(admitted.lease().private_context_lost(), "{case}");
        source.publish(Source::seen());
        assert!(!current(&source, &admitted), "restoration revived {case}");
    }
    let source = Source::new();
    let mut legacy = Source::seen();
    for field in ["state", "publisher", "generation"] {
        legacy.as_object_mut().unwrap().remove(field);
    }
    source.publish(legacy);
    let admitted = owner(3, Instant::now() + Duration::from_secs(5));
    assert!(current(&source, &admitted), "legacy positive compatibility");
    source.publish(Source::absent());
    assert!(
        !current(&source, &admitted),
        "legacy observation has no continuation token"
    );
}

#[tokio::test]
async fn repeated_explicit_absence_does_not_extend_the_original_audio_deadline() {
    let source = Source::new();
    let deadline = Instant::now() + Duration::from_millis(250);
    let admitted = owner(1, deadline);
    assert!(current(&source, &admitted));
    source.publish(Source::absent());
    assert!(current(&source, &admitted));
    tokio::time::timeout(
        Duration::from_secs(1),
        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)),
    )
    .await
    .unwrap();
    source.publish(Source::absent());
    assert!(!current(&source, &admitted));
    assert!(admitted.lease().private_context_lost());
    assert!(admitted.lease().is_current());
}

struct Heard;
impl Listener for Heard {
    fn send(&self, _: &[i16]) -> net::Result<()> {
        Ok(())
    }
    fn finish(self: Box<Self>, _: u64) -> BoxFuture<'static, net::Result<String>> {
        Box::pin(async { Ok("What do you remember about me?".into()) })
    }
}
struct Answer {
    known: PathBuf,
    next: Value,
    messages: Mutex<Vec<Vec<Message>>>,
    spoken: Mutex<Vec<String>>,
}
impl Services for Answer {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        Box::pin(async { Ok(Box::new(Heard) as Box<dyn Listener>) })
    }
    fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
        self.messages.lock().unwrap().push(messages);
        let path = self.known.clone();
        let next = self.next.clone();
        // The first actual inner poll changes the publication after chat
        // admission; both post-poll and all output boundaries must recheck.
        stream::iter([
            Ok(ChatEvent::Text("First response. ".into())),
            Ok(ChatEvent::Text("Final response.".into())),
            Ok(ChatEvent::Done),
        ])
        .enumerate()
        .map(move |(index, event)| {
            if index == 0 {
                publish(&path, &next);
            }
            event
        })
        .boxed()
    }
    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        self.spoken.lock().unwrap().push(text);
        stream::iter([Ok(vec![1; 160])]).boxed()
    }
}

#[tokio::test]
async fn actual_engine_sealed_microphone_request_finishes_through_explicit_lookaway() {
    use crate::speaking_faces::SpeakingFaces;
    for continued in [true, false] {
        let source = Source::new();
        let eyes = Arc::new(SpeakingFaces::default());
        let cache = Arc::new(crate::memory_scope::FaceCache::default());
        let services = Arc::new(Answer {
            known: source.known.clone(),
            next: if continued {
                Source::absent()
            } else {
                json!({"state":"revoked","boot":BOOT,
                "publisher":PUBLISHER,"generation":2})
            },
            messages: Mutex::new(Vec::new()),
            spoken: Mutex::new(Vec::new()),
        });
        let context = source.known.clone();
        let validate = source.known.clone();
        let mut engine = Engine::new(
            services.clone(),
            Told::default,
            Arc::new(|_, _, _, _| panic!("no unscoped tools")),
        )
        .with_memory_scope(
            BootId::new([53; 16]).unwrap(),
            cache.clone(),
            Arc::new(|_, _, _, _, _| panic!("no tools")),
            Some(Arc::new(move |lease| {
                let note = known_name_told(true, Some(&context), Some(lease));
                Told {
                    text: if note.is_empty() {
                        "neutral".into()
                    } else {
                        note.clone()
                    },
                    history: if note.is_empty() {
                        Vec::new()
                    } else {
                        vec![Message::user(CANARY)]
                    },
                    requires_speaker: !note.is_empty(),
                    ..Told::default()
                }
            })),
        )
        .guarding_private_context(Arc::new(move |lease| {
            private_context_current(Some(&validate), lease)
        }))
        .seeing_who_speaks(eyes.clone(), source.known.clone())
        .announcing(false);
        engine.memory_camera(true, 1);
        assert!(cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&source.known)
        ));
        let at = Instant::now() - Duration::from_secs(2);
        let at_us = monotonic_us().checked_sub(2_000_000).unwrap();
        for index in 0..10u64 {
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
                        state: lamp_msgs::SpeakingState::Yes,
                        score: 0.9,
                        from: None,
                    }),
                }],
            };
            eyes.look(
                at + Duration::from_millis(index * 100 + 50),
                Some(((BOOT, index + 1), &faces)),
            );
        }
        engine.submit(ProviderInput::Start {
            request: 1,
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
        for block in 1..=100 {
            engine.submit(ProviderInput::Audio {
                request: 1,
                privacy_generation: 1,
                sequence: block,
                read_completed_at_us: at_us + block * 10_000,
                samples: vec![1; lamp_audio::CAPTURE_SAMPLES],
            });
        }
        engine.submit(ProviderInput::End {
            request: 1,
            privacy_generation: 1,
            sound: false,
            cut_in: false,
        });
        let mut outputs = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let event = engine.next().await;
                assert!(!matches!(event, ProviderOutput::TurnFailed { .. }));
                let done = matches!(event, ProviderOutput::TurnComplete { .. });
                outputs.push(event);
                if done {
                    break;
                }
            }
        })
        .await
        .expect("actual Engine completes even after read denial");
        let messages = services.messages.lock().unwrap();
        assert_eq!(
            messages.len(),
            1,
            "actual sealed current piece grants one chat"
        );
        assert!(
            serde_json::to_string(&messages[0])
                .unwrap()
                .contains(CANARY)
        );
        let spoken = services.spoken.lock().unwrap().join(" ");
        if continued {
            assert!(spoken.contains("First response."));
            assert!(spoken.contains("Final response."));
        } else {
            assert!(
                spoken.is_empty(),
                "revocation before first text cannot speak private answer"
            );
        }
        assert_eq!(
            outputs
                .iter()
                .any(|e| matches!(e, ProviderOutput::Audio { .. })),
            continued
        );
    }
}

async fn http_body(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut raw = Vec::new();
    loop {
        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            let end = end + 4;
            let length: usize = String::from_utf8_lossy(&raw[..end])
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|n| n.trim().parse().unwrap())
                })
                .expect("complete bounded request has content-length");
            assert!(length < 64 * 1024);
            if raw.len() >= end + length {
                return raw[end..end + length].to_vec();
            }
        }
        assert!(raw.len() < 64 * 1024 + 4096);
        let mut bytes = [0; 4096];
        let n = socket.read(&mut bytes).await.unwrap();
        assert!(n > 0, "complete request body arrives before response");
        raw.extend_from_slice(&bytes[..n]);
    }
}
#[tokio::test]
async fn actual_http_fallback_retains_only_explicit_same_epoch_lookaway() {
    use crate::second_provider::Prepared;
    use std::os::unix::fs::PermissionsExt;
    for mode in ["absent", "revoked", "missing"] {
        let source = Source::new();
        let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fallback = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let first_url = format!("http://{}", first.local_addr().unwrap());
        let fallback_url = format!("http://{}", fallback.local_addr().unwrap());
        let (arrived, arrival) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let first_task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(3),async {
                let (mut socket,_)=first.accept().await.unwrap();
                arrived.send(http_body(&mut socket).await).unwrap();
                released.await.unwrap();
                socket.write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await.unwrap();
            }).await.expect("first HTTP boundary is bounded");
        });
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let observed = bodies.clone();
        let fallback_task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(3),async {
                let (mut socket,_)=fallback.accept().await.unwrap();
                let body = http_body(&mut socket).await;
                observed.lock().unwrap().push(body);
                socket.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Finished.\"}}]}\n\ndata: [DONE]\n\n").await.unwrap();
            }).await.expect("fallback is bounded if attempted");
        });
        let credential = source._directory.path.join("key");
        std::fs::write(&credential, "local-fixture-key").unwrap();
        std::fs::set_permissions(&credential, std::fs::Permissions::from_mode(0o600)).unwrap();
        let path = source._directory.path.join("config.json");
        std::fs::write(
            &path,
            json!({"llm_base_url":"https://relay.example.invalid/api/v1/ai",
            "llm_model":"m","llm_credential_file":credential,"tts_provider":"elevenlabs"})
            .to_string(),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = crate::second_config::load_from_path(&path).unwrap();
        config.llm_base_url = first_url;
        let net = Net::new(config, Arc::new(Vec::new))
            .unwrap()
            .falling_back_to_the_relay(&fallback_url, Arc::new(|_| {}))
            .unwrap();
        let known = source.known.clone();
        let engine = Engine::new(
            Arc::new(net),
            || Told {
                text: CANARY.into(),
                requires_speaker: true,
                ..Told::default()
            },
            Arc::new(|_, _, _, _| panic!("no tools")),
        )
        .guarding_private_context(Arc::new(move |lease| {
            private_context_current(Some(&known), lease)
        }));
        let owner = owner(2, Instant::now() + Duration::from_secs(5));
        let mut prepared = Prepared::begin(
            &engine.shared,
            Some(&owner.lease()),
            "question".into(),
            &[],
            None,
            false,
        );
        let sent = tokio::time::timeout(Duration::from_secs(2), arrival)
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&sent).contains(CANARY));
        match mode {
            "absent" => source.publish(Source::absent()),
            "revoked" => source.publish(
                json!({"state":"revoked","boot":BOOT,"publisher":PUBLISHER,"generation":2}),
            ),
            "missing" => {
                std::fs::remove_file(crate::speaking_faces::known_speaker_of(&source.known))
                    .unwrap()
            }
            _ => unreachable!(),
        }
        assert!(owner.lease().is_current());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while prepared.events.recv().await.is_some() {}
        })
        .await
        .expect("actual prepared HTTP stream finishes");
        first_task.await.unwrap();
        {
            let sent = bodies.lock().unwrap();
            assert_eq!(sent.len(), usize::from(mode == "absent"), "{mode}");
            if mode == "absent" {
                assert!(String::from_utf8_lossy(&sent[0]).contains(CANARY));
            }
        }
        // Stream completion means any fallback request had to receive its
        // response, so a zero body count is a deterministic dispatch assertion.
        fallback_task.abort();
        let _ = fallback_task.await;
    }
}

#[tokio::test]
async fn explicit_lookaway_retains_credited_output_only_for_same_request_epoch() {
    const NOW: u64 = 10_000_000;
    for case in ["lookaway", "revoked", "missing", "public-room"] {
        let source = Source::new();
        let directory = crate::process::SessionDirectory::create().unwrap();
        let boot = BootId::new([37; 16]).unwrap();
        let worker_boot = BootId::new([38; 16]).unwrap();
        let mut parent =
            WorkerChannels::bind(&directory.path, "second", boot, worker_boot, false).unwrap();
        let mut worker =
            WorkerChannels::bind(&directory.path, "second", boot, worker_boot, true).unwrap();
        parent.connect(&directory.path, "second", false).unwrap();
        worker.connect(&directory.path, "second", true).unwrap();
        let at = MonoTime::from_micros(NOW);
        let mut owner = Controller::new(boot, at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        owner
            .set_camera_permission(at, Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(NOW + 100_000);
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        let request = owner.admit(at, AdmittedInput::NewTurn).unwrap().turn();
        let before = owner.snapshot(at).unwrap();
        let known = source.known.clone();
        let mut engine = Engine::new(
            Arc::new(Answer {
                known: known.clone(),
                next: Source::absent(),
                messages: Mutex::new(Vec::new()),
                spoken: Mutex::new(Vec::new()),
            }),
            Told::default,
            Arc::new(|_, _, _, _| panic!("no tools in the IPC fixture")),
        )
        .guarding_private_context(Arc::new(move |lease| {
            private_context_current(Some(&known), lease)
        }));
        let mut scope = RequestScope::new(
            RequestKey {
                worker: worker_boot,
                session: 1,
                request,
            },
            if case == "public-room" {
                ScopeSnapshot::room()
            } else {
                ScopeSnapshot::of(NAME)
            },
        );
        // Use the actual sealed request proof and fresh source matcher,
        // never a manually toggled permission callback.
        let deadline = Instant::now() + Duration::from_secs(5);
        if case != "public-room" {
            let key = scope.lease().key();
            assert!(
                scope
                    .speaker_recorder()
                    .unwrap()
                    .seal(key, &[SPEAKER.into()], deadline)
            );
            assert_eq!(scope.lease().speaker(), Some(SPEAKER));
        }
        let guard =
            PrivateContext::new(&engine.shared, Some(&scope.lease()), case != "public-room");
        let mut gate = Gate::new(boot, NOW);
        gate.install(NOW, before, &mut engine).unwrap();
        // Spend both initial credits through the actual worker/parent sockets.
        for _ in 0..2 {
            let public = ProviderOutput::GenerationComplete { request }.into();
            assert!(
                gate.send_or_discard(&engine, NOW, &public, |event| worker
                    .data
                    .send_with(event, || NOW))
                    .unwrap()
            );
            assert!(
                parent
                    .data
                    .receive_with::<ProviderOutput>(|| NOW)
                    .unwrap()
                    .is_some()
            );
        }
        assert!(!gate.flow.can_send());
        assert!(
            engine
                .shared
                .emit_private(
                    &guard,
                    ProviderOutput::Transcript {
                        request: Some(request),
                        text: "fixture response".into(),
                        finished: false
                    }
                )
                .await
        );
        let pending = next_output(&mut engine).unwrap();
        let mut blocked_attempts = 0;
        assert!(
            !gate
                .send_or_discard(&engine, NOW, &pending, |event| {
                    blocked_attempts += 1;
                    worker.data.send_with(event, || NOW)
                })
                .unwrap()
        );
        assert_eq!(blocked_attempts, 0, "no credit means no IPC attempt");
        assert!(
            parent
                .data
                .receive_with::<ProviderOutput>(|| NOW)
                .unwrap()
                .is_none()
        );
        // The same pending entry crosses an actual scheduling boundary without
        // credit. The public control authority and microphone generation stay.
        tokio::task::yield_now().await;
        match case {
            "lookaway" => source.publish(Source::absent()),
            "revoked" => source.publish(
                json!({"state":"revoked","boot":BOOT,"publisher":PUBLISHER,"generation":2}),
            ),
            "missing" => {
                std::fs::remove_file(crate::speaking_faces::known_speaker_of(&source.known))
                    .unwrap()
            }
            "public-room" => (),
            _ => unreachable!(),
        }
        let retry = NOW + 1_000;
        gate.fresh(retry).unwrap();
        gate.may_send(retry, &pending.event).unwrap();
        let after = gate.guard.state().unwrap();
        assert_eq!(before.owner(), after.owner());
        assert_eq!(
            before.microphone_generation(),
            after.microphone_generation()
        );
        assert!(scope.lease().is_current());
        gate.flow.grant(3).unwrap();
        let mut attempts = 0;
        assert!(
            gate.send_or_discard(&engine, retry, &pending, |event| {
                attempts += 1;
                worker.data.send_with(event, || retry)
            })
            .unwrap()
        );
        let delivered = matches!(case, "lookaway" | "public-room");
        assert_eq!(
            attempts,
            usize::from(delivered),
            "revocation or missing source causes no IPC attempt"
        );
        assert_eq!(
            matches!(
                parent
                    .data
                    .receive_with::<ProviderOutput>(|| retry)
                    .unwrap(),
                Some(ProviderOutput::Transcript { .. })
            ),
            delivered,
            "{case}"
        );
        assert_eq!(
            gate.flow.can_send(),
            !delivered,
            "discard spends no credit: {case}"
        );
        // A discarded private event cannot hold up the public terminal event.
        gate.flow.grant(4).unwrap();
        let done = ProviderOutput::TurnComplete {
            request,
            idle: true,
        }
        .into();
        assert!(
            gate.send_or_discard(&engine, retry, &done, |event| worker
                .data
                .send_with(event, || retry))
                .unwrap()
        );
        assert!(
            matches!(parent.data.receive_with::<ProviderOutput>(||retry).unwrap(),Some(ProviderOutput::TurnComplete {request:got,..}) if got==request)
        );
    }
}

#[test]
fn explicit_source_envelopes_reject_malformed_or_mixed_continuation_tokens() {
    let source = Source::new();
    for (field, value) in [
        ("publisher", json!("")),
        ("publisher", json!("A".repeat(32))),
        ("publisher", json!("0".repeat(31))),
        ("generation", json!(0)),
        ("generation", json!(-1)),
        ("generation", json!("1")),
        ("boot", json!("")),
        ("boot", json!("0123456789abcdeF")),
        ("state", json!("unknown")),
        ("face", json!("f7")),
        ("name", json!(NAME)),
        ("id", Value::Null),
    ] {
        let mut value_source = Source::absent();
        value_source[field] = value;
        source.publish(value_source);
        assert!(
            crate::speaking_faces::known_speaker_source(&source.known).is_none(),
            "{field}"
        );
    }
    for state in ["absent", "checking", "revoked"] {
        let mut value = Source::seen();
        value["state"] = json!(state);
        source.publish(value);
        assert!(
            crate::speaking_faces::known_speaker_source(&source.known).is_none(),
            "{state}"
        );
    }
    for value in [
        Source::seen().to_string().replacen(
            "\"state\":\"seen\"",
            "\"state\":\"seen\",\"state\":\"absent\"",
            1,
        ),
        Source::seen().to_string().replacen(
            "\"name\":",
            "\"name\":\"Injected fixture\",\"name\":",
            1,
        ),
        Source::absent().to_string().replacen(
            "\"generation\":1",
            "\"generation\":1,\"generation\":2",
            1,
        ),
        Source::seen()
            .to_string()
            .replace("\"seen\"", "\"unknown\""),
    ] {
        std::fs::write(
            crate::speaking_faces::known_speaker_of(&source.known),
            value,
        )
        .unwrap();
        assert!(
            crate::speaking_faces::known_speaker_source(&source.known).is_none(),
            "duplicate/unknown source field accepted"
        );
    }
    source.publish(Source::seen());
    assert!(
        crate::speaking_faces::known_speaker(&source.known)
            .unwrap()
            .epoch
            .is_some()
    );
    source.publish(Source::absent());
    assert!(
        crate::speaking_faces::known_speaker(&source.known).is_none(),
        "positive-only compatibility reader cannot expose retained identity"
    );
}

// These JSON-only fixtures compile before the consumer learns the checking
// state. No test installs a permission override or changes global switches.
fn returning_seen(face: &str) -> Value {
    let mut source = Source::seen();
    source["face"] = json!(face);
    source["id"] = json!(lamp_msgs::known::person_id(NAME));
    source
}
fn returning_owner(request: u64, face: &str) -> RequestScope {
    let mut owner = RequestScope::new(
        RequestKey {
            worker: BootId::new([54; 16]).unwrap(),
            session: 1,
            request,
        },
        ScopeSnapshot::of(NAME),
    );
    let key = owner.lease().key();
    assert!(owner.speaker_recorder().unwrap().seal(
        key,
        &[format!("{BOOT}/{face}")],
        Instant::now() + Duration::from_secs(5),
    ));
    owner
}

#[test]
fn returning_track_checking_and_unmatched_cancel_only_the_old_proved_request() {
    for state in ["checking", "revoked"] {
        let source = Source::new();
        source.publish(returning_seen("f3"));
        let old = returning_owner(1, "f3");
        assert!(current(&source, &old));
        let original = old.lease().private_speaker().unwrap().clone();
        source.publish(Source::absent());
        assert!(current(&source, &old));
        let mut denied = Source::absent();
        denied["state"] = json!(state);
        if state == "revoked" {
            denied["generation"] = json!(2);
        }
        source.publish(denied);
        assert!(
            !current(&source, &old),
            "{state} must deny immediately, never pause"
        );
        assert!(old.lease().private_context_lost());
        assert!(
            old.lease().is_current(),
            "disclosure denial is not a microphone revoke"
        );
        let mut matched = returning_seen("f4");
        if state == "revoked" {
            matched["generation"] = json!(2);
        }
        source.publish(matched);
        assert!(
            !current(&source, &old),
            "completed recognition cannot revive observed denial"
        );
        assert_eq!(old.lease().speaker(), Some("0123456789abcdef/f3"));
        assert_eq!(old.lease().private_speaker(), Some(&original));
        let fresh = returning_owner(2, "f4");
        assert!(
            current(&source, &fresh),
            "fresh own proof may use completed re-recognition"
        );
        assert!(!known_name_told(true, Some(&source.known), Some(&fresh.lease())).is_empty());
        assert!(known_name_told(true, Some(&source.known), Some(&old.lease())).is_empty());
        assert_eq!(fresh.lease().speaker(), Some("0123456789abcdef/f4"));
        assert_eq!(
            fresh.lease().scope().unwrap().person(),
            Some(lamp_msgs::known::person_id(NAME).as_str())
        );
    }
}

#[test]
fn returning_track_same_name_never_erases_changed_stable_identity() {
    for case in ["different person", "same name different id"] {
        let source = Source::new();
        source.publish(returning_seen("f3"));
        let old = returning_owner(1, "f3");
        assert!(current(&source, &old));
        let original = old.lease().private_speaker().unwrap().clone();
        let mut other = returning_seen("f3");
        if case == "different person" {
            other["name"] = json!("Other fixture");
        } else {
            other["id"] = json!(format!("{}-2", lamp_msgs::known::person_id(NAME)));
        }
        source.publish(other);
        assert!(
            !current(&source, &old),
            "{case}; raw stable id matters even with name-keyed memory"
        );
        assert!(old.lease().private_context_lost(), "{case}");
        source.publish(returning_seen("f3"));
        assert!(
            !current(&source, &old),
            "restoration after {case} cannot revive old request"
        );
        assert_eq!(old.lease().private_speaker(), Some(&original));
    }
}

#[tokio::test]
async fn returning_track_completed_recognition_keeps_original_grant_at_actual_http_fallback() {
    use crate::second_provider::Prepared;
    use std::os::unix::fs::PermissionsExt;
    for mode in ["returned", "checking", "unmatched"] {
        let source = Source::new();
        source.publish(returning_seen("f3"));
        let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fallback = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let first_url = format!("http://{}", first.local_addr().unwrap());
        let fallback_url = format!("http://{}", fallback.local_addr().unwrap());
        let (arrived, arrival) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let first_task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(3),async {
                let (mut socket,_)=first.accept().await.unwrap();
                arrived.send(http_body(&mut socket).await).unwrap();
                released.await.unwrap();
                socket.write_all(b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await.unwrap();
            }).await.expect("first HTTP boundary is bounded");
        });
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let observed = bodies.clone();
        let fallback_task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(3),async {
                let (mut socket,_)=fallback.accept().await.unwrap();
                let body = http_body(&mut socket).await;
                observed.lock().unwrap().push(body);
                socket.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Finished.\"}}]}\n\ndata: [DONE]\n\n").await.unwrap();
            }).await.expect("fallback is bounded if attempted");
        });
        let credential = source._directory.path.join("key");
        std::fs::write(&credential, "local-fixture-key").unwrap();
        std::fs::set_permissions(&credential, std::fs::Permissions::from_mode(0o600)).unwrap();
        let path = source._directory.path.join("config.json");
        std::fs::write(
            &path,
            json!({"llm_base_url":"https://relay.example.invalid/api/v1/ai",
            "llm_model":"m","llm_credential_file":credential,"tts_provider":"elevenlabs"})
            .to_string(),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut config = crate::second_config::load_from_path(&path).unwrap();
        config.llm_base_url = first_url;
        let net = Net::new(config, Arc::new(Vec::new))
            .unwrap()
            .falling_back_to_the_relay(&fallback_url, Arc::new(|_| {}))
            .unwrap();
        let known = source.known.clone();
        let engine = Engine::new(
            Arc::new(net),
            || Told {
                text: CANARY.into(),
                requires_speaker: true,
                ..Told::default()
            },
            Arc::new(|_, _, _, _| panic!("no tools")),
        )
        .guarding_private_context(Arc::new(move |lease| {
            private_context_current(Some(&known), lease)
        }));
        let owner = returning_owner(2, "f3");
        let original_grant_before = owner.lease().private_speaker().cloned();
        assert!(original_grant_before.is_none());
        let mut prepared = Prepared::begin(
            &engine.shared,
            Some(&owner.lease()),
            "question".into(),
            &[],
            None,
            false,
        );
        let sent = tokio::time::timeout(Duration::from_secs(2), arrival)
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&sent).contains(CANARY));
        let original_grant = owner.lease().private_speaker().unwrap().clone();
        assert_eq!(original_grant.face, "0123456789abcdef/f3");
        source.publish(Source::absent());
        assert!(current(&source, &owner));
        match mode {
            "returned" => source.publish(returning_seen("f4")),
            "checking" => {
                let mut checking = Source::absent();
                checking["state"] = json!("checking");
                source.publish(checking);
            }
            "unmatched" => source.publish(json!({"state":"revoked","boot":BOOT,
                "publisher":PUBLISHER,"generation":2})),
            _ => unreachable!(),
        }
        assert!(owner.lease().is_current());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while prepared.events.recv().await.is_some() {}
        })
        .await
        .expect("actual prepared HTTP stream finishes");
        first_task.await.unwrap();
        {
            let sent = bodies.lock().unwrap();
            assert_eq!(sent.len(), usize::from(mode == "returned"), "{mode}");
            if mode == "returned" {
                assert!(String::from_utf8_lossy(&sent[0]).contains(CANARY));
            }
        }
        assert_eq!(owner.lease().speaker(), Some("0123456789abcdef/f3"));
        assert_eq!(owner.lease().private_speaker(), Some(&original_grant));
        assert_eq!(
            owner.lease().scope().unwrap().person(),
            Some(lamp_msgs::known::person_id(NAME).as_str())
        );
        let mut recognized = returning_seen("f4");
        if mode == "unmatched" {
            recognized["generation"] = json!(2);
        }
        source.publish(recognized);
        assert_eq!(
            current(&source, &owner),
            mode == "returned",
            "same request cannot revive"
        );
        let fresh = returning_owner(3, "f4");
        assert!(
            current(&source, &fresh),
            "fresh proved ask may use recognized return"
        );
        // Stream completion means any fallback request had to receive its
        // response, so a zero body count is a deterministic dispatch assertion.
        fallback_task.abort();
        let _ = fallback_task.await;
    }
}

#[tokio::test]
async fn returning_track_completed_recognition_keeps_original_grant_at_credited_ipc() {
    const NOW: u64 = 10_000_000;
    for case in ["returned", "checking", "unmatched", "public-room"] {
        let source = Source::new();
        source.publish(returning_seen("f3"));
        let directory = crate::process::SessionDirectory::create().unwrap();
        let boot = BootId::new([37; 16]).unwrap();
        let worker_boot = BootId::new([38; 16]).unwrap();
        let mut parent =
            WorkerChannels::bind(&directory.path, "second", boot, worker_boot, false).unwrap();
        let mut worker =
            WorkerChannels::bind(&directory.path, "second", boot, worker_boot, true).unwrap();
        parent.connect(&directory.path, "second", false).unwrap();
        worker.connect(&directory.path, "second", true).unwrap();
        let at = MonoTime::from_micros(NOW);
        let mut owner = Controller::new(boot, at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        owner
            .set_camera_permission(at, Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(NOW + 100_000);
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        let request = owner.admit(at, AdmittedInput::NewTurn).unwrap().turn();
        let before = owner.snapshot(at).unwrap();
        let known = source.known.clone();
        let mut engine = Engine::new(
            Arc::new(Answer {
                known: known.clone(),
                next: Source::absent(),
                messages: Mutex::new(Vec::new()),
                spoken: Mutex::new(Vec::new()),
            }),
            Told::default,
            Arc::new(|_, _, _, _| panic!("no tools in the IPC fixture")),
        )
        .guarding_private_context(Arc::new(move |lease| {
            private_context_current(Some(&known), lease)
        }));
        let mut scope = RequestScope::new(
            RequestKey {
                worker: worker_boot,
                session: 1,
                request,
            },
            if case == "public-room" {
                ScopeSnapshot::room()
            } else {
                ScopeSnapshot::of(NAME)
            },
        );
        // Use the actual sealed request proof and fresh source matcher,
        // never a manually toggled permission callback.
        let deadline = Instant::now() + Duration::from_secs(5);
        if case != "public-room" {
            let key = scope.lease().key();
            assert!(scope.speaker_recorder().unwrap().seal(
                key,
                &["0123456789abcdef/f3".into()],
                deadline
            ));
            assert_eq!(scope.lease().speaker(), Some("0123456789abcdef/f3"));
        }
        let guard =
            PrivateContext::new(&engine.shared, Some(&scope.lease()), case != "public-room");
        let mut gate = Gate::new(boot, NOW);
        gate.install(NOW, before, &mut engine).unwrap();
        // Spend both initial credits through the actual worker/parent sockets.
        for _ in 0..2 {
            let public = ProviderOutput::GenerationComplete { request }.into();
            assert!(
                gate.send_or_discard(&engine, NOW, &public, |event| worker
                    .data
                    .send_with(event, || NOW))
                    .unwrap()
            );
            assert!(
                parent
                    .data
                    .receive_with::<ProviderOutput>(|| NOW)
                    .unwrap()
                    .is_some()
            );
        }
        assert!(!gate.flow.can_send());
        assert!(
            engine
                .shared
                .emit_private(
                    &guard,
                    ProviderOutput::Transcript {
                        request: Some(request),
                        text: "fixture response".into(),
                        finished: false
                    }
                )
                .await
        );
        let pending = next_output(&mut engine).unwrap();
        let mut blocked_attempts = 0;
        assert!(
            !gate
                .send_or_discard(&engine, NOW, &pending, |event| {
                    blocked_attempts += 1;
                    worker.data.send_with(event, || NOW)
                })
                .unwrap()
        );
        assert_eq!(blocked_attempts, 0, "no credit means no IPC attempt");
        assert!(
            parent
                .data
                .receive_with::<ProviderOutput>(|| NOW)
                .unwrap()
                .is_none()
        );
        // The same pending entry crosses an actual scheduling boundary without
        // credit. The public control authority and microphone generation stay.
        tokio::task::yield_now().await;
        if case != "public-room" {
            source.publish(Source::absent());
            assert!(private_context_current(Some(&source.known), &scope.lease()));
            assert_eq!(
                scope.lease().private_speaker().unwrap().face,
                "0123456789abcdef/f3"
            );
        }
        match case {
            "returned" => source.publish(returning_seen("f4")),
            "checking" => {
                let mut checking = Source::absent();
                checking["state"] = json!("checking");
                source.publish(checking);
            }
            "unmatched" => source.publish(
                json!({"state":"revoked","boot":BOOT,"publisher":PUBLISHER,"generation":2}),
            ),
            "public-room" => (),
            _ => unreachable!(),
        }
        let retry = NOW + 1_000;
        gate.fresh(retry).unwrap();
        gate.may_send(retry, &pending.event).unwrap();
        let after = gate.guard.state().unwrap();
        assert_eq!(before.owner(), after.owner());
        assert_eq!(
            before.microphone_generation(),
            after.microphone_generation()
        );
        assert!(scope.lease().is_current());
        gate.flow.grant(3).unwrap();
        let mut attempts = 0;
        assert!(
            gate.send_or_discard(&engine, retry, &pending, |event| {
                attempts += 1;
                worker.data.send_with(event, || retry)
            })
            .unwrap()
        );
        let delivered = matches!(case, "returned" | "public-room");
        assert_eq!(
            attempts,
            usize::from(delivered),
            "revocation or missing source causes no IPC attempt"
        );
        assert_eq!(
            matches!(
                parent
                    .data
                    .receive_with::<ProviderOutput>(|| retry)
                    .unwrap(),
                Some(ProviderOutput::Transcript { .. })
            ),
            delivered,
            "{case}"
        );
        assert_eq!(
            gate.flow.can_send(),
            !delivered,
            "discard spends no credit: {case}"
        );
        // A discarded private event cannot hold up the public terminal event.
        gate.flow.grant(4).unwrap();
        let done = ProviderOutput::TurnComplete {
            request,
            idle: true,
        }
        .into();
        assert!(
            gate.send_or_discard(&engine, retry, &done, |event| worker
                .data
                .send_with(event, || retry))
                .unwrap()
        );
        assert!(
            matches!(parent.data.receive_with::<ProviderOutput>(||retry).unwrap(),Some(ProviderOutput::TurnComplete {request:got,..}) if got==request)
        );
    }
}

#[test]
fn returning_track_unbound_and_legacy_requests_still_need_the_original_audio_face() {
    let source = Source::new();
    source.publish(returning_seen("f4"));
    let unmatched = returning_owner(1, "f3");
    assert!(!current(&source, &unmatched));
    assert!(unmatched.lease().private_speaker().is_none());
    assert!(
        !unmatched.lease().private_context_lost(),
        "initial denial grants no continuation"
    );
    let matched = returning_owner(2, "f4");
    assert!(current(&source, &matched));

    let mut legacy = returning_seen("f3");
    for field in ["state", "publisher", "generation"] {
        legacy.as_object_mut().unwrap().remove(field);
    }
    source.publish(legacy.clone());
    let original = returning_owner(3, "f3");
    assert!(current(&source, &original));
    assert!(
        original.lease().private_speaker().is_none(),
        "legacy cannot pin continuity"
    );
    legacy["face"] = json!("f4");
    source.publish(legacy.clone());
    assert!(
        !current(&source, &original),
        "legacy still requires the exact original face"
    );
    legacy["face"] = json!("f3");
    source.publish(legacy);
    assert!(
        !current(&source, &original),
        "legacy disclosure denial cannot revive"
    );
}

#[test]
fn returning_track_checking_envelope_has_no_identity_and_cannot_arm_continuation() {
    let source = Source::new();
    let mut checking = Source::absent();
    checking["state"] = json!("checking");
    source.publish(checking.clone());
    assert!(matches!(
        crate::speaking_faces::known_speaker_source(&source.known),
        Some(crate::speaking_faces::KnownSpeakerSource::Checking(_))
    ));
    assert!(crate::speaking_faces::known_speaker(&source.known).is_none());
    let request = returning_owner(1, "f3");
    assert!(!current(&source, &request));
    assert!(request.lease().private_speaker().is_none());
    assert!(!request.lease().private_context_lost());
    for (field, value) in [
        ("name", json!(NAME)),
        ("id", Value::Null),
        ("face", json!("f3")),
        ("generation", json!(0)),
        ("publisher", json!("F".repeat(32))),
        ("boot", json!("0123456789abcdeF")),
    ] {
        let mut bad = checking.clone();
        bad[field] = value;
        source.publish(bad);
        assert!(
            crate::speaking_faces::known_speaker_source(&source.known).is_none(),
            "{field}"
        );
    }
}

#[tokio::test]
async fn returning_track_completed_recognition_never_extends_original_audio_deadline() {
    let source = Source::new();
    source.publish(returning_seen("f3"));
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut request = RequestScope::new(
        RequestKey {
            worker: BootId::new([55; 16]).unwrap(),
            session: 1,
            request: 1,
        },
        ScopeSnapshot::of(NAME),
    );
    let key = request.lease().key();
    assert!(request.speaker_recorder().unwrap().seal(
        key,
        &["0123456789abcdef/f3".into()],
        deadline,
    ));
    assert!(current(&source, &request));
    let grant = request.lease().private_speaker().unwrap().clone();
    source.publish(Source::absent());
    assert!(current(&source, &request));
    source.publish(returning_seen("f4"));
    assert!(current(&source, &request));
    assert_eq!(request.lease().speaker(), Some("0123456789abcdef/f3"));
    assert_eq!(request.lease().private_speaker(), Some(&grant));
    tokio::time::timeout(
        Duration::from_secs(3),
        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)),
    )
    .await
    .unwrap();
    source.publish(returning_seen("f4"));
    assert!(!current(&source, &request));
    assert!(request.lease().is_current());
    assert!(request.lease().speaker().is_none());
    assert!(request.lease().private_context_lost());
    assert_eq!(request.lease().private_speaker(), Some(&grant));
}
