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
    authority: crate::camera_authority::fixture::Fixture,
    _directory: crate::process::SessionDirectory,
    known: PathBuf,
}
impl Source {
    fn new() -> Self {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let known = directory.path.canonicalize().unwrap().join("known");
        std::fs::write(&known, NAME).unwrap();
        std::fs::write(crate::speaking_faces::known_face_of(&known), "f7").unwrap();
        let authority = crate::camera_authority::fixture::Fixture::start(
            known.clone(),
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ],
            1,
        );
        let this = Self {
            authority,
            _directory: directory,
            known,
        };
        this.publish(Self::seen());
        this
    }
    fn seen() -> Value {
        let now = lamp_ipc::monotonic_us();
        json!({"version":3,"camera_hold":1,"camera_began_us":1,
            "state":"seen","name":NAME,"id":null,"boot":BOOT,"face":"f7",
            "publisher":PUBLISHER,"generation":1,
            "observed_us":now,"permission_until_us":now.checked_add(1_500_000).unwrap()})
    }
    fn absent() -> Value {
        let now = lamp_ipc::monotonic_us();
        json!({"version":3,"camera_hold":1,"camera_began_us":1,
            "state":"absent","boot":BOOT,"publisher":PUBLISHER,"generation":1,
            "observed_us":now,"permission_until_us":now.checked_add(1_500_000).unwrap()})
    }
    // Deliberately old wire for compatibility/parser-only controls. Runtime
    // private grants below use only the separately explicit V3 constructors.
    fn seen_v2() -> Value {
        let now = lamp_ipc::monotonic_us();
        json!({"state":"seen","name":NAME,"id":null,"boot":BOOT,"face":"f7",
            "publisher":PUBLISHER,"generation":1,
            "observed_us":now,"permission_until_us":now.checked_add(1_500_000).unwrap()})
    }
    fn absent_v2() -> Value {
        let now = lamp_ipc::monotonic_us();
        json!({"state":"absent","boot":BOOT,"publisher":PUBLISHER,"generation":1,
            "observed_us":now,"permission_until_us":now.checked_add(1_500_000).unwrap()})
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
fn owner(source: &Source, request: u64, deadline: Instant) -> RequestScope {
    let mut owner = RequestScope::looking_later(
        RequestKey {
            worker: BootId::new([52; 16]).unwrap(),
            session: 1,
            request,
        },
        ScopeSnapshot::of(NAME),
        source.authority.cache(),
    );
    let key = owner.lease().key();
    assert!(
        owner
            .speaker_recorder()
            .unwrap()
            .seal(key, &[SPEAKER.into()], deadline)
    );
    let _camera = source.authority.ready(&owner.lease());
    owner
}
fn current(source: &Source, owner: &RequestScope) -> bool {
    private_context_current(Some(&source.known), &owner.lease())
}

#[test]
fn explicit_absence_is_request_owned_and_never_grants_an_inherited_private_scope() {
    let source = Source::new();
    let original = owner(&source, 1, Instant::now() + Duration::from_secs(5));
    let began = Instant::now();
    assert!(current(&source, &original));
    let initial = began.elapsed();
    // A valid V3 file and original audio are insufficient without the actual
    // startup-installed client; this is not a JSON-only authority path.
    let mut no_client = RequestScope::new(
        RequestKey {
            worker: BootId::new([52; 16]).unwrap(),
            session: 1,
            request: 3,
        },
        ScopeSnapshot::of(NAME),
    );
    let key = no_client.lease().key();
    assert!(no_client.speaker_recorder().unwrap().seal(
        key,
        &[SPEAKER.into()],
        Instant::now() + Duration::from_secs(5),
    ));
    assert!(!current(&source, &no_client));
    assert!(no_client.lease().private_speaker().is_none());
    assert!(!no_client.lease().private_context_lost());
    source.publish(Source::seen_v2());
    let old_wire = owner(&source, 4, Instant::now() + Duration::from_secs(5));
    assert!(!current(&source, &old_wire), "V2 has no kernel authority");
    assert!(old_wire.lease().private_speaker().is_none());
    source.publish(Source::absent());
    let began = Instant::now();
    assert!(current(&source, &original));
    let continuation = began.elapsed();
    eprintln!(
        "private source host lookup: initial {} us, continuation {} us; bounded local source parse and warmed kernel witness check, target under 5 ms, no device/acoustic claim",
        initial.as_micros(),
        continuation.as_micros()
    );
    assert!(!known_name_told(true, Some(&source.known), Some(&original.lease())).is_empty());
    let next = owner(&source, 2, Instant::now() + Duration::from_secs(5));
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
        let admitted = owner(&source, 1, Instant::now() + Duration::from_secs(5));
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
                // An unresolved return pauses, then an unmatched recognition
                // revokes the epoch. The old request cannot survive that loss.
                let mut checking = Source::absent();
                checking["state"] = json!("checking");
                source.publish(checking);
                assert!(!current(&source, &admitted), "unrecognized returning track");
                assert!(!admitted.lease().private_context_lost());
                changed = Source::absent();
                changed["state"] = json!("revoked");
                changed["generation"] = json!(2);
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
                for field in ["version", "camera_hold", "camera_began_us"] {
                    changed.as_object_mut().unwrap().remove(field);
                }
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
    for field in [
        "version",
        "camera_hold",
        "camera_began_us",
        "state",
        "publisher",
        "generation",
        "permission_until_us",
        "observed_us",
    ] {
        legacy.as_object_mut().unwrap().remove(field);
    }
    source.publish(legacy);
    let admitted = owner(&source, 3, Instant::now() + Duration::from_secs(5));
    assert!(
        !current(&source, &admitted),
        "legacy lacks permission authority"
    );
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
    let admitted = owner(&source, 1, deadline);
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
        let cache = source.authority.cache();
        let warm = owner(&source, 900, Instant::now() + Duration::from_secs(5));
        assert_eq!(
            source.authority.ready(&warm.lease()),
            PermissionState::Ready
        );
        let services = Arc::new(Answer {
            known: source.known.clone(),
            next: if continued {
                Source::absent()
            } else {
                json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":"revoked","boot":BOOT,
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
            private_context_state(Some(&validate), lease)
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
    for mode in ["absent", "revoked", "missing", "ticket-revoked"] {
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
            private_context_state(Some(&known), lease)
        }));
        let owner = owner(&source, 2, Instant::now() + Duration::from_secs(5));
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
                json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":"revoked","boot":BOOT,"publisher":PUBLISHER,"generation":2}),
            ),
            "ticket-revoked" => {
                let path = crate::speaking_faces::known_speaker_of(&source.known);
                let unchanged = std::fs::read(&path).unwrap();
                source.authority.witness.revoke().unwrap();
                assert_eq!(std::fs::read(path).unwrap(), unchanged);
            }
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
    for case in [
        "lookaway",
        "revoked",
        "missing",
        "ticket-revoked",
        "public-room",
    ] {
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
            private_context_state(Some(&known), lease)
        }));
        let mut scope = RequestScope::looking_later(
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
            source.authority.cache(),
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
            assert_eq!(
                source.authority.ready(&scope.lease()),
                PermissionState::Ready
            );
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
                json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":"revoked","boot":BOOT,"publisher":PUBLISHER,"generation":2}),
            ),
            "ticket-revoked" => {
                let path = crate::speaking_faces::known_speaker_of(&source.known);
                let unchanged = std::fs::read(&path).unwrap();
                source.authority.witness.revoke().unwrap();
                assert_eq!(std::fs::read(path).unwrap(), unchanged);
            }
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
            crate::camera_authority::read(&source.known).is_none(),
            "{field}"
        );
    }
    for state in ["absent", "checking", "revoked"] {
        let mut value = Source::seen();
        value["state"] = json!(state);
        source.publish(value);
        assert!(
            crate::camera_authority::read(&source.known).is_none(),
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
            crate::camera_authority::read(&source.known).is_none(),
            "duplicate/unknown source field accepted"
        );
    }
    source.publish(Source::seen_v2());
    assert!(
        crate::speaking_faces::known_speaker(&source.known)
            .unwrap()
            .epoch
            .is_some()
    );
    source.publish(Source::absent_v2());
    assert!(
        crate::speaking_faces::known_speaker(&source.known).is_none(),
        "positive-only compatibility reader cannot expose retained identity"
    );
}

// Real V3 fixtures retain the original request audio and kernel hold.
// No test installs a permission override or changes global switches.
fn returning_seen(face: &str) -> Value {
    let mut source = Source::seen();
    source["face"] = json!(face);
    source["id"] = json!(lamp_msgs::known::person_id(NAME));
    source
}
fn returning_owner(source: &Source, request: u64, face: &str) -> RequestScope {
    let mut owner = RequestScope::looking_later(
        RequestKey {
            worker: BootId::new([54; 16]).unwrap(),
            session: 1,
            request,
        },
        ScopeSnapshot::of(NAME),
        source.authority.cache(),
    );
    let key = owner.lease().key();
    assert!(owner.speaker_recorder().unwrap().seal(
        key,
        &[format!("{BOOT}/{face}")],
        Instant::now() + Duration::from_secs(5),
    ));
    let _camera = source.authority.ready(&owner.lease());
    owner
}

#[test]
fn returning_track_changed_epoch_checking_and_unmatched_cancel_only_the_old_proved_request() {
    for state in ["checking", "revoked"] {
        let source = Source::new();
        source.publish(returning_seen("f3"));
        let old = returning_owner(&source, 1, "f3");
        assert!(current(&source, &old));
        let original = old.lease().private_speaker().unwrap().clone();
        source.publish(Source::absent());
        assert!(current(&source, &old));
        let mut denied = Source::absent();
        denied["state"] = json!(state);
        // Checking from a new epoch is terminal, unlike same-epoch return.
        denied["generation"] = json!(2);
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
        matched["generation"] = json!(2);
        source.publish(matched);
        assert!(
            !current(&source, &old),
            "completed recognition cannot revive observed denial"
        );
        assert_eq!(old.lease().speaker(), Some("0123456789abcdef/f3"));
        assert_eq!(old.lease().private_speaker(), Some(&original));
        let fresh = returning_owner(&source, 2, "f4");
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
        let old = returning_owner(&source, 1, "f3");
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
    for mode in ["returned", "checking-returned", "checking", "unmatched"] {
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
        let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let accepted_at_server = accepted.clone();
        let fallback_task = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(3),async {
                let (mut socket,_)=fallback.accept().await.unwrap();
                accepted_at_server.fetch_add(1, Ordering::SeqCst);
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
            private_context_state(Some(&known), lease)
        }));
        let owner = returning_owner(&source, 2, "f3");
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
            "checking" | "checking-returned" => {
                let mut checking = Source::absent();
                checking["state"] = json!("checking");
                if mode == "checking" {
                    checking["generation"] = json!(2);
                }
                source.publish(checking);
            }
            "unmatched" => source.publish(json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":"revoked","boot":BOOT,
                "publisher":PUBLISHER,"generation":2})),
            _ => unreachable!(),
        }
        assert!(owner.lease().is_current());
        release.send(()).unwrap();
        if mode == "checking-returned" {
            assert!(
                tokio::time::timeout(Duration::from_millis(80), prepared.events.recv())
                    .await
                    .is_err(),
                "same-epoch checking keeps the actual prepared stream pending"
            );
            assert_eq!(
                accepted.load(Ordering::SeqCst),
                0,
                "no fallback connection is opened during unresolved recognition"
            );
            assert!(bodies.lock().unwrap().is_empty());
            assert!(!owner.lease().private_context_lost());
            source.publish(returning_seen("f4"));
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while prepared.events.recv().await.is_some() {}
        })
        .await
        .expect("actual prepared HTTP stream finishes");
        first_task.await.unwrap();
        {
            let sent = bodies.lock().unwrap();
            assert_eq!(
                sent.len(),
                usize::from(matches!(mode, "returned" | "checking-returned")),
                "{mode}"
            );
            if matches!(mode, "returned" | "checking-returned") {
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
        if matches!(mode, "unmatched" | "checking") {
            recognized["generation"] = json!(2);
        }
        source.publish(recognized);
        assert_eq!(
            current(&source, &owner),
            matches!(mode, "returned" | "checking-returned"),
            "only completed matching recognition may resume the pending request"
        );
        let fresh = returning_owner(&source, 3, "f4");
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
            private_context_state(Some(&known), lease)
        }));
        let mut scope = RequestScope::looking_later(
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
            source.authority.cache(),
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
            assert_eq!(
                source.authority.ready(&scope.lease()),
                PermissionState::Ready
            );
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
                checking["generation"] = json!(2);
                source.publish(checking);
            }
            "unmatched" => source.publish(
                json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":"revoked","boot":BOOT,"publisher":PUBLISHER,"generation":2}),
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
    let unmatched = returning_owner(&source, 1, "f3");
    assert!(!current(&source, &unmatched));
    assert!(unmatched.lease().private_speaker().is_none());
    assert!(
        !unmatched.lease().private_context_lost(),
        "initial denial grants no continuation"
    );
    let matched = returning_owner(&source, 2, "f4");
    assert!(current(&source, &matched));

    let mut legacy = returning_seen("f3");
    for field in [
        "version",
        "camera_hold",
        "camera_began_us",
        "state",
        "publisher",
        "generation",
        "permission_until_us",
        "observed_us",
    ] {
        legacy.as_object_mut().unwrap().remove(field);
    }
    source.publish(legacy.clone());
    let original = returning_owner(&source, 3, "f3");
    assert!(
        !current(&source, &original),
        "legacy lacks permission authority"
    );
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
        crate::camera_authority::read(&source.known).map(|source| source.source),
        Some(crate::speaking_faces::KnownSpeakerSource::Checking(_))
    ));
    assert!(crate::speaking_faces::known_speaker(&source.known).is_none());
    let request = returning_owner(&source, 1, "f3");
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
            crate::camera_authority::read(&source.known)
                .map(|source| source.source)
                .is_none(),
            "{field}"
        );
    }
}

#[tokio::test]
async fn returning_track_completed_recognition_never_extends_original_audio_deadline() {
    let source = Source::new();
    source.publish(returning_seen("f3"));
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut request = RequestScope::looking_later(
        RequestKey {
            worker: BootId::new([55; 16]).unwrap(),
            session: 1,
            request: 1,
        },
        ScopeSnapshot::of(NAME),
        source.authority.cache(),
    );
    let key = request.lease().key();
    assert!(request.speaker_recorder().unwrap().seal(
        key,
        &["0123456789abcdef/f3".into()],
        deadline,
    ));
    assert_eq!(
        source.authority.ready(&request.lease()),
        PermissionState::Ready
    );
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

// Matching Checking denies disclosure while preserving only
// the already-pinned request. No new API, permission override or global switch.
#[test]
fn checking_return_never_arms_and_only_same_subject_can_finish_the_wait() {
    let source = Source::new();
    source.publish(returning_seen("f3"));
    let old = returning_owner(&source, 41, "f3");
    assert!(current(&source, &old));
    let grant = old.lease().private_speaker().unwrap().clone();
    source.publish(Source::absent());
    assert!(current(&source, &old));
    let mut checking = Source::absent();
    checking["state"] = json!("checking");
    source.publish(checking);
    assert!(
        !current(&source, &old),
        "no disclosure during unresolved recognition"
    );
    assert!(
        !old.lease().private_context_lost(),
        "matching Checking preserves a bounded pending grant"
    );
    let new = returning_owner(&source, 42, "f4");
    assert!(
        !current(&source, &new),
        "Checking cannot bind new audio to a person"
    );
    assert!(new.lease().private_speaker().is_none());
    assert!(
        !new.lease().private_context_lost(),
        "unproved remains neutral"
    );
    source.publish(returning_seen("f4"));
    assert!(
        current(&source, &old),
        "completed same-person match resumes old request"
    );
    assert_eq!(old.lease().private_speaker(), Some(&grant));
    assert_eq!(old.lease().speaker(), Some("0123456789abcdef/f3"));
    assert!(
        current(&source, &new),
        "a new request needs its own matching positive proof"
    );
}

#[test]
fn checking_wait_never_hides_terminal_source_or_identity_loss() {
    for case in [
        "missing",
        "revoked",
        "other-person",
        "same-name-other-id",
        "boot",
        "publisher",
        "generation",
        "malformed",
        "privacy-forget",
    ] {
        let source = Source::new();
        source.publish(returning_seen("f3"));
        let old = returning_owner(&source, 43, "f3");
        assert!(current(&source, &old));
        source.publish(Source::absent());
        assert!(current(&source, &old));
        let mut checking = Source::absent();
        checking["state"] = json!("checking");
        source.publish(checking.clone());
        assert!(!current(&source, &old));
        assert!(
            !old.lease().private_context_lost(),
            "{case}: initial wait is not loss"
        );
        match case {
            "missing" => {
                std::fs::remove_file(crate::speaking_faces::known_speaker_of(&source.known))
                    .unwrap()
            }
            "revoked" | "privacy-forget" => source.publish(
                json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":"revoked", "boot":BOOT, "publisher":PUBLISHER, "generation":2}),
            ),
            "other-person" => {
                let mut seen = returning_seen("f4");
                seen["name"] = json!("Other fixture");
                source.publish(seen);
            }
            "same-name-other-id" => {
                let mut seen = returning_seen("f4");
                seen["id"] = json!(lamp_msgs::known::person_id("Other fixture"));
                source.publish(seen);
            }
            "boot" => {
                checking["boot"] = json!("fedcba9876543210");
                source.publish(checking);
            }
            "publisher" => {
                checking["publisher"] = json!("fedcba9876543210fedcba9876543210");
                source.publish(checking);
            }
            "generation" => {
                checking["generation"] = json!(2);
                source.publish(checking);
            }
            "malformed" => {
                checking["name"] = json!(NAME);
                source.publish(checking);
            }
            _ => unreachable!(),
        }
        assert!(!current(&source, &old), "{case}: no private authority");
        assert!(
            old.lease().private_context_lost(),
            "{case}: terminal failure is irreversible"
        );
        let mut recognized = returning_seen("f4");
        recognized["generation"] = json!(2);
        source.publish(recognized);
        assert!(!current(&source, &old), "{case}: no old-request revival");
        let new = returning_owner(&source, 44, "f4");
        assert!(
            current(&source, &new),
            "{case}: a fresh recognized ask may start"
        );
    }
}

#[tokio::test]
async fn checking_wait_ends_at_the_original_audio_deadline() {
    let source = Source::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    let old = owner(&source, 45, deadline);
    assert!(current(&source, &old));
    let grant = old.lease().private_speaker().unwrap().clone();
    source.publish(Source::absent());
    assert!(current(&source, &old));
    let mut checking = Source::absent();
    checking["state"] = json!("checking");
    source.publish(checking);
    assert!(!current(&source, &old));
    assert!(!old.lease().private_context_lost());
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::time::sleep_until(deadline.into()).await;
        assert!(
            !current(&source, &old),
            "original sealed audio deadline ends the wait"
        );
        assert!(old.lease().private_context_lost());
        source.publish(Source::seen());
        assert!(
            !current(&source, &old),
            "fresh publication cannot retime an expired proof"
        );
    })
    .await
    .expect("one bounded original-deadline wait");
    assert_eq!(old.lease().private_speaker(), Some(&grant));
    assert!(
        old.lease().is_current(),
        "disclosure expiry is not microphone revocation"
    );
}

#[tokio::test]
async fn checking_return_holds_real_credited_ipc_until_completed_recognition() {
    const NOW: u64 = 10_000_000;
    for case in ["returned", "unmatched", "public-room"] {
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
            private_context_state(Some(&known), lease)
        }));
        let mut scope = RequestScope::looking_later(
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
            source.authority.cache(),
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
            assert_eq!(
                source.authority.ready(&scope.lease()),
                PermissionState::Ready
            );
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
        gate.flow.grant(3).unwrap();
        if case != "public-room" {
            let mut checking = Source::absent();
            checking["state"] = json!("checking");
            source.publish(checking);
            let mut attempts = 0;
            assert!(
                !gate
                    .send_or_discard(&engine, NOW, &pending, |event| {
                        attempts += 1;
                        worker.data.send_with(event, || NOW)
                    })
                    .unwrap(),
                "matching in-flight Checking retains this pending event"
            );
            assert_eq!(attempts, 0, "Checking sends no private IPC");
            assert!(
                parent
                    .data
                    .receive_with::<ProviderOutput>(|| NOW)
                    .unwrap()
                    .is_none()
            );
            assert!(gate.flow.can_send(), "Checking spends no credit");
            assert!(
                !scope.lease().private_context_lost(),
                "Checking is not terminal loss"
            );
            assert!(scope.lease().is_current());
        }
        match case {
            "returned" => source.publish(returning_seen("f4")),
            "unmatched" => source.publish(
                json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":"revoked","boot":BOOT,"publisher":PUBLISHER,"generation":2}),
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
fn event_selection_keeps_checking_distinct_from_room_and_lost() {
    let _one = crate::provider_worker::tests::one_thread();
    struct RestoreThread(Option<crate::thread::Thread>);
    impl Drop for RestoreThread {
        fn drop(&mut self) {
            *crate::thread::THREAD
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = self.0.take().unwrap();
        }
    }
    let _restore = RestoreThread(Some(std::mem::take(
        &mut *crate::thread::THREAD.lock().unwrap(),
    )));
    let source = Source::new();
    let admitted = owner(&source, 1, Instant::now() + Duration::from_secs(5));
    let sessions = Mutex::new(Sessions::default());
    let select = |request, lease: &crate::memory_scope::ScopeLease| {
        memory_whose(
            request,
            Some(lease),
            &sessions,
            Some(&source.known),
            Instant::now(),
        )
    };
    assert_eq!(
        select(1, &admitted.lease()),
        Ok(Some(crate::remember::person_id(NAME)))
    );
    let grant = admitted.lease().private_speaker().unwrap().clone();
    let mut checking = Source::absent();
    checking["state"] = json!("checking");
    source.publish(checking);
    assert_eq!(select(1, &admitted.lease()), Err(MemoryUnselected::Pending));
    assert!(!admitted.lease().private_context_lost());
    source.publish(Source::seen());
    assert_eq!(
        select(1, &admitted.lease()),
        Ok(Some(crate::remember::person_id(NAME)))
    );
    assert_eq!(admitted.lease().private_speaker(), Some(&grant));
    assert_eq!(select(2, &admitted.lease()), Err(MemoryUnselected::Lost));
    admitted.revoke();
    assert_eq!(select(1, &admitted.lease()), Err(MemoryUnselected::Lost));
    let room = RequestScope::new(
        RequestKey {
            worker: BootId::new([52; 16]).unwrap(),
            session: 1,
            request: 3,
        },
        ScopeSnapshot::room(),
    );
    assert_eq!(select(3, &room.lease()), Ok(None));
    assert_eq!(
        memory_whose(
            4,
            None,
            &Mutex::new(Sessions::default()),
            None,
            Instant::now()
        ),
        Ok(None)
    );
    // An initial Checking observation has no pinned grant to pause or lose.
    // It suppresses the event while leaving the old neutral answer available.
    let unproved = owner(&source, 5, Instant::now() + Duration::from_secs(5));
    let mut checking = Source::absent();
    checking["state"] = json!("checking");
    source.publish(checking);
    assert_eq!(
        select(5, &unproved.lease()),
        Err(MemoryUnselected::Unavailable)
    );
    assert!(!unproved.lease().private_context_lost());
    assert!(unproved.lease().private_speaker().is_none());
    // A legacy unleased sticky session never gains private/room disclosure,
    // but lack of proof must not cancel its otherwise neutral answer.
    source.publish(Source::seen());
    let sticky = Mutex::new(Sessions::default());
    assert_eq!(
        memory_whose(6, None, &sticky, Some(&source.known), Instant::now()),
        Err(MemoryUnselected::Unavailable)
    );
    assert_eq!(
        hits_whose(6, None, &sticky, Some(&source.known), Instant::now()),
        None
    );
}

/// The same bounded source parser and first-grant validator used at final
/// dispatch, with only the monotonic clock injected. No sleep or lamp claim.
#[test]
fn permission_expiry_blocks_first_private_grant_at_the_final_boundary() {
    let source = Source::new();
    let mut seen = Source::seen_v2();
    seen["permission_until_us"] = json!(2_500_000u64);
    seen["observed_us"] = json!(2_400_000u64);
    source.publish(seen.clone());
    let read = |now| {
        crate::speaking_faces::read_known_speaker_source_at(
            || std::fs::File::open(crate::speaking_faces::known_speaker_of(&source.known)),
            std::time::SystemTime::now,
            || now,
            true,
        )
    };
    let fresh = owner(&source, 901, Instant::now() + Duration::from_secs(5));
    assert_eq!(
        private_speaker_permission_from(&fresh.lease(), || read(2_499_999)).0,
        PermissionState::Ready
    );
    let first = owner(&source, 902, Instant::now() + Duration::from_secs(5));
    assert!(first.lease().private_speaker().is_none());
    assert_ne!(
        private_speaker_permission_from(&first.lease(), || read(2_500_000)).0,
        PermissionState::Ready
    );
    assert!(
        first.lease().private_speaker().is_none(),
        "expired Seen must not mint a first grant"
    );
    assert_eq!(
        private_speaker_permission_from(&fresh.lease(), || read(2_500_000)).0,
        PermissionState::Lost
    );
    seen["permission_until_us"] = json!(4_000_000u64);
    source.publish(seen);
    assert_eq!(
        private_speaker_permission_from(&fresh.lease(), || read(2_500_001)).0,
        PermissionState::Lost
    );
    // Old typed producers and legacy sources remain parseable by the explicit
    // compatibility helper, but cannot authorize private dispatch.
    let mut old = Source::seen_v2();
    for field in ["permission_until_us", "observed_us"] {
        old.as_object_mut().unwrap().remove(field);
    }
    source.publish(old.clone());
    assert!(crate::speaking_faces::known_speaker(&source.known).is_some());
    assert!(read(2_400_001).is_none());
    let mut legacy = old;
    for field in [
        "version",
        "camera_hold",
        "camera_began_us",
        "state",
        "publisher",
        "generation",
        "permission_until_us",
        "observed_us",
    ] {
        legacy.as_object_mut().unwrap().remove(field);
    }
    source.publish(legacy);
    assert!(crate::speaking_faces::known_speaker(&source.known).is_some());
    assert!(read(2_400_001).is_none());
    let legacy_owner = owner(&source, 903, Instant::now() + Duration::from_secs(5));
    assert_ne!(
        private_speaker_permission_from(&legacy_owner.lease(), || {
            crate::speaking_faces::known_speaker(&source.known)
                .map(crate::speaking_faces::KnownSpeakerSource::Seen)
        })
        .0,
        PermissionState::Ready,
        "even the permission validator cannot grant an unversioned source"
    );
}

#[test]
fn permission_reader_checks_completion_and_all_nonterminal_states() {
    let source = Source::new();
    for state in ["seen", "absent", "checking"] {
        let mut value = if state == "seen" {
            Source::seen_v2()
        } else {
            Source::absent_v2()
        };
        value["state"] = json!(state);
        value["permission_until_us"] = json!(2_500_000u64);
        value["observed_us"] = json!(2_400_000u64);
        source.publish(value);
        let mut clocks = [2_499_999, 2_500_000].into_iter();
        assert!(
            crate::speaking_faces::read_known_speaker_source_at(
                || std::fs::File::open(crate::speaking_faces::known_speaker_of(&source.known)),
                std::time::SystemTime::now,
                || clocks.next().unwrap(),
                true,
            )
            .is_none(),
            "{state}: completion crosses permission equality"
        );
    }
    // Continuous permission renewal does not make a three-second-old camera
    // capture fresh after a slow recognition completion or mtime rewrite.
    for (observed, until, now) in [
        (1_000_000, 5_500_000, 4_000_000),
        (0, 2_500_000, 2_400_000),
        (2_500_001, 3_000_000, 2_500_000),
        (2_400_000, u64::MAX, 2_500_000),
    ] {
        let mut value = Source::seen_v2();
        value["observed_us"] = json!(observed);
        value["permission_until_us"] = json!(until);
        source.publish(value);
        assert!(
            crate::speaking_faces::read_known_speaker_source_at(
                || std::fs::File::open(crate::speaking_faces::known_speaker_of(&source.known)),
                std::time::SystemTime::now,
                || now,
                true,
            )
            .is_none()
        );
    }
}

#[test]
fn renewed_permission_cannot_refresh_an_old_absent_or_checking_observation() {
    for state in ["absent", "checking"] {
        for expired in [false, true] {
            let source = Source::new();
            let request = owner(&source, 904, Instant::now() + Duration::from_secs(5));
            let read = |now| {
                crate::speaking_faces::read_known_speaker_source_at(
                    || std::fs::File::open(crate::speaking_faces::known_speaker_of(&source.known)),
                    std::time::SystemTime::now,
                    || now,
                    true,
                )
            };
            let mut seen = Source::seen_v2();
            seen["observed_us"] = json!(1_000_000u64);
            seen["permission_until_us"] = json!(2_500_000u64);
            source.publish(seen);
            assert_eq!(
                private_speaker_permission_from(&request.lease(), || read(1_100_000)).0,
                PermissionState::Ready
            );

            // No intermediate read observes expiry. A renewed permission and
            // fresh file mtime cannot refresh the captured observation itself.
            let mut observation = Source::absent_v2();
            observation["state"] = json!(state);
            observation["observed_us"] = json!(if expired { 1_000_000u64 } else { 3_900_000 });
            observation["permission_until_us"] = json!(5_500_000u64);
            source.publish(observation.clone());
            let expected = if expired {
                PermissionState::Lost
            } else if state == "checking" {
                PermissionState::Pending
            } else {
                PermissionState::Ready
            };
            assert_eq!(
                private_speaker_permission_from(&request.lease(), || read(4_000_000)).0,
                expected,
                "{state}, expired={expired}"
            );
            if expired {
                observation["observed_us"] = json!(3_900_000u64);
                source.publish(observation);
                assert_eq!(
                    private_speaker_permission_from(&request.lease(), || read(4_000_001)).0,
                    PermissionState::Lost,
                    "later refresh cannot revive a lost private request"
                );
            }
        }
    }
}
