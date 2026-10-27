//! Real HTTP and kernel-ticket controls. No project gate has run this source.
use super::*;
use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) struct Authority {
    fixture: crate::camera_authority::fixture::Fixture,
    owner: RequestScope,
    _directory: crate::process::SessionDirectory,
    known: std::path::PathBuf,
    initial_source: Vec<u8>,
}
impl Authority {
    pub(crate) fn new() -> Self {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let known = directory.path.canonicalize().unwrap().join("known");
        std::fs::write(&known, "Fixture owner").unwrap();
        let now = lamp_ipc::monotonic_us();
        std::fs::write(
            crate::speaking_faces::known_speaker_of(&known),
            serde_json::json!({
                "version":3,"state":"seen","name":"Fixture owner","id":null,
                "boot":"0123456789abcdef","face":"f7",
                "publisher":"0123456789abcdef0123456789abcdef","generation":1,
                "camera_hold":1,"camera_began_us":1,"observed_us":now,
                "permission_until_us":now.checked_add(1_500_000).unwrap()
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
        let mut owner = RequestScope::looking_later(
            RequestKey {
                worker: lamp_turn_owner::BootId::new([73; 16]).unwrap(),
                session: 1,
                request: 7,
            },
            ScopeSnapshot::of("Fixture owner"),
            fixture.cache(),
        );
        let key = owner.lease().key();
        assert!(owner.speaker_recorder().unwrap().seal(
            key,
            &["0123456789abcdef/f7".into()],
            std::time::Instant::now() + std::time::Duration::from_secs(5)
        ));
        assert_eq!(fixture.ready(&owner.lease()), PermissionState::Ready);
        assert_eq!(
            crate::second_provider::worker::private_context_state(Some(&known), &owner.lease()),
            PermissionState::Ready
        );
        let initial_source =
            std::fs::read(crate::speaking_faces::known_speaker_of(&known)).unwrap();
        Self {
            fixture,
            _directory: directory,
            known,
            owner,
            initial_source,
        }
    }
    pub(crate) fn permission(&self) -> ChatPermission {
        let known = self.known.clone();
        let lease = self.owner.lease();
        Arc::new(move || {
            crate::second_provider::worker::private_context_state(Some(&known), &lease)
        })
    }
    pub(crate) fn revoke(&self) {
        let source = crate::speaking_faces::known_speaker_of(&self.known);
        let before = std::fs::read(&source).unwrap();
        self.fixture.witness.revoke().unwrap();
        assert_eq!(before, std::fs::read(source).unwrap());
        assert!(self.owner.lease().is_current());
        assert!(self.owner.lease().speaker().is_some());
    }
    fn restore_seen(&self) {
        // Restore the original observation/permission bytes, never retime it.
        std::fs::write(
            crate::speaking_faces::known_speaker_of(&self.known),
            &self.initial_source,
        )
        .unwrap();
    }
    fn checking(&self) {
        let source = crate::speaking_faces::known_speaker_of(&self.known);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();
        value["state"] = "checking".into();
        for field in ["name", "id", "face"] {
            value.as_object_mut().unwrap().remove(field);
        }
        std::fs::write(source, value.to_string()).unwrap();
    }
}

pub(crate) struct Http {
    pub(crate) url: String,
    pub(crate) bodies: Arc<Mutex<Vec<Vec<u8>>>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Http {
    /// One fixture server handles each connection in place; stop wins over an
    /// incomplete peer, and the whole task has a fixed outer bound.
    pub(crate) async fn start(first_status: u16) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let seen = bodies.clone();
        let (stop, mut ended) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let serving = async move {
                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut raw = Vec::new();
                    let body = loop {
                        assert!(raw.len() < 16384, "bounded local TTS body");
                        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                            let size: usize = head
                                .lines()
                                .find_map(|line| line.strip_prefix("content-length:"))
                                .unwrap()
                                .trim()
                                .parse()
                                .unwrap();
                            if raw.len() >= end + 4 + size {
                                break raw[end + 4..end + 4 + size].to_vec();
                            }
                        }
                        let mut bytes = [0; 1024];
                        let n = socket.read(&mut bytes).await.unwrap();
                        if n == 0 {
                            break Vec::new();
                        }
                        raw.extend_from_slice(&bytes[..n]);
                    };
                    if body.is_empty() {
                        continue;
                    }
                    let index = {
                        let mut all = seen.lock().unwrap();
                        all.push(body);
                        all.len()
                    };
                    let status = if index == 1 { first_status } else { 200 };
                    let pcm = if status == 200 {
                        vec![1u8, 0]
                    } else {
                        Vec::new()
                    };
                    let head = format!(
                        "HTTP/1.1 {status} fixture\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        pcm.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&pcm).await;
                }
            };
            tokio::select! {
                _=&mut ended=>{},
                result=tokio::time::timeout(std::time::Duration::from_secs(5),serving)=>{result.expect("bounded TTS fixture server");}
            }
        });
        Self {
            url,
            bodies,
            stop: Some(stop),
            task: Some(task),
        }
    }
    pub(crate) async fn finish(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.as_mut() {
            tokio::time::timeout(std::time::Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap();
            self.task.take();
        }
    }
}
impl Drop for Http {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
fn taken(url: &str) -> Taken {
    Taken {
        near: Arc::new(Near {
            routable: Routable {
                chat: false,
                speech: true,
                listening: false,
                gemini: false,
            },
            kept: Arc::new(routes::Kept::default()),
            url: String::new(),
            turn: Mutex::new((1, Routes::default())),
            chat: Mutex::new(None),
            tts: Mutex::new(None),
            hearing: Mutex::new(None),
            gemini_chat: Mutex::new(None),
            gemini_tts: Mutex::new(None),
        }),
        generation: 1,
        url: url.into(),
    }
}

#[tokio::test]
async fn real_tts_http_fallback_and_retry_recheck_kernel_authority_inside_one_poll() {
    for near in [false, true] {
        for revoke in [false, true] {
            // The raw + outer-guard row is the actual old composition control:
            // it counts a deferred HTTP send's first poll, not a service ctor.
            for per_send in [false, true] {
                let authority = Arc::new(Authority::new());
                let mut first = Http::start(500).await;
                let mut own = Http::start(200).await;
                let polls = Arc::new(AtomicUsize::new(0));
                let voice = Voice::Configured(
                    Arc::new(
                        Tts::with(
                            crate::second_config::TtsProvider::OpenAi,
                            &first.url,
                            "fixture",
                            None,
                        )
                        .unwrap()
                        .counting_send_polls(polls.clone()),
                    ),
                    "fixture",
                );
                let fallback = near.then(|| {
                    (
                        Voice::Configured(
                            Arc::new(
                                Tts::with(
                                    crate::second_config::TtsProvider::OpenAi,
                                    &own.url,
                                    "fixture",
                                    None,
                                )
                                .unwrap()
                                .counting_send_polls(polls.clone()),
                            ),
                            "fixture",
                        ),
                        taken(&first.url),
                        Service::Speech,
                    )
                });
                let attempts = Arc::new(AtomicUsize::new(0));
                let observed = attempts.clone();
                let close = authority.clone();
                let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    if observed.fetch_add(1, Ordering::AcqRel) == 0 && revoke {
                        close.revoke();
                    }
                });
                let permitted = authority.permission();
                let speech = in_one_voice_near_guarded(
                    voice,
                    fallback,
                    "PRIVATE_TTS_CANARY".into(),
                    None,
                    per_send.then(|| permitted.clone()),
                    Some(hook),
                );
                let mut speech = while_permitted(speech, permitted);
                let mut packets = 0;
                tokio::time::timeout(std::time::Duration::from_secs(3), async {
                    while let Some(packet) = speech.next().await {
                        packet.unwrap();
                        packets += 1;
                    }
                })
                .await
                .unwrap();
                first.finish().await;
                own.finish().await;
                assert!(
                    first.bodies.lock().unwrap()[0]
                        .windows(b"PRIVATE_TTS_CANARY".len())
                        .any(|w| w == b"PRIVATE_TTS_CANARY")
                );
                assert_eq!(
                    polls.load(Ordering::Acquire),
                    if revoke && per_send { 1 } else { 2 },
                    "near={near},revoke={revoke},per_send={per_send}"
                );
                if revoke && per_send {
                    assert_eq!(first.bodies.lock().unwrap().len(), 1);
                    assert!(own.bodies.lock().unwrap().is_empty());
                }
                if !revoke {
                    assert_eq!(packets, 1);
                    assert_eq!(first.bodies.lock().unwrap().len(), if near { 1 } else { 2 });
                    assert_eq!(own.bodies.lock().unwrap().len(), usize::from(near));
                }
                if revoke {
                    assert_eq!(packets, 0);
                }
            }
        }
    }
}

struct DefaultSpeech {
    initialised: Arc<AtomicUsize>,
    polled: Arc<AtomicUsize>,
}
impl Services for DefaultSpeech {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        Box::pin(async { panic!("speech only") })
    }
    fn chat(&self, _: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
        stream::empty().boxed()
    }
    fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        self.initialised.fetch_add(1, Ordering::AcqRel);
        let polled = self.polled.clone();
        stream::once(async move {
            polled.fetch_add(1, Ordering::AcqRel);
            Ok(vec![1])
        })
        .boxed()
    }
}
#[tokio::test]
async fn default_guarded_speech_keeps_synchronous_construction_but_never_polls_pending_child() {
    for resume in [false, true] {
        let authority = Authority::new();
        authority.checking();
        let initialised = Arc::new(AtomicUsize::new(0));
        let polled = Arc::new(AtomicUsize::new(0));
        let services = DefaultSpeech {
            initialised: initialised.clone(),
            polled: polled.clone(),
        };
        let mut speech = services.speak_for_guarded(
            "private".into(),
            net::Turn {
                request: 7,
                heard_at: std::time::Instant::now(),
            },
            authority.permission(),
        );
        assert_eq!(initialised.load(Ordering::Acquire), 1);
        let mut next = Box::pin(speech.next());
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(std::future::Future::poll(
                next.as_mut(),
                cx
            )))
            .await
            .is_pending()
        );
        assert_eq!(polled.load(Ordering::Acquire), 0);
        if resume {
            authority.restore_seen();
        } else {
            authority.revoke();
        }
        let packet = tokio::time::timeout(std::time::Duration::from_secs(2), next)
            .await
            .unwrap();
        assert_eq!(packet.is_some(), resume);
        assert_eq!(polled.load(Ordering::Acquire), usize::from(resume));
        if let Some(packet) = packet {
            assert_eq!(packet.unwrap(), vec![1]);
        }
        assert!(speech.next().await.is_none());
    }
    deadline_controls().await;
}

// Exercise both clock fences with the same real held authority: an unpolled
// child cannot start late, and an already returned value cannot be released late.
async fn deadline_controls() {
    for completed in [false, true] {
        for continuous in [false, true] {
            let authority = Authority::new();
            if !completed {
                authority.checking();
            }
            let permission = authority.permission();
            let polls = Arc::new(AtomicUsize::new(0));
            let counted = polls.clone();
            let until = tokio::time::Instant::now() + std::time::Duration::from_millis(10);
            let operation = async {
                counted.fetch_add(1, Ordering::AcqRel);
                if completed {
                    authority.checking();
                }
                1usize
            };
            let mut held = Box::pin(permitted_until(operation, Some(&permission), until));
            assert!(
                std::future::poll_fn(|cx| std::task::Poll::Ready(std::future::Future::poll(
                    held.as_mut(),
                    cx
                )))
                .await
                .is_pending()
            );
            assert_eq!(polls.load(Ordering::Acquire), usize::from(completed));
            tokio::time::sleep_until(until).await;
            if continuous {
                assert_eq!(
                    permission(),
                    PermissionState::Pending,
                    "source remains actual Checking at expiry"
                );
                let terminal = std::future::poll_fn(|cx| {
                    std::task::Poll::Ready(std::future::Future::poll(held.as_mut(), cx))
                })
                .await;
                assert!(
                    matches!(terminal, std::task::Poll::Ready(None)),
                    "deadline stays visible while authority remains Pending"
                );
                assert_eq!(
                    permission(),
                    PermissionState::Pending,
                    "clock expiry does not revoke or resume request authority"
                );
                assert_eq!(polls.load(Ordering::Acquire), usize::from(completed));
                assert!(!authority.owner.lease().private_context_lost());
                authority.restore_seen();
                assert_eq!(permission(), PermissionState::Ready);
            } else {
                authority.restore_seen();
                assert_eq!(permission(), PermissionState::Ready);
                assert!(
                    tokio::time::timeout(std::time::Duration::from_secs(2), held)
                        .await
                        .unwrap()
                        .is_none()
                );
                assert_eq!(polls.load(Ordering::Acquire), usize::from(completed));
                assert!(
                    !authority.owner.lease().private_context_lost(),
                    "operation expiry does not revoke an independent current request"
                );
            }
        }
    }
    // The default Services outer stream must not hide its own Pending clock.
    let authority = Authority::new();
    authority.checking();
    let initialised = Arc::new(AtomicUsize::new(0));
    let polled = Arc::new(AtomicUsize::new(0));
    let services = DefaultSpeech {
        initialised: initialised.clone(),
        polled: polled.clone(),
    };
    let until = tokio::time::Instant::now() + std::time::Duration::from_millis(10);
    let mut stream = while_permitted_clock(
        services.speak("private".into()),
        authority.permission(),
        Some(until),
    );
    let mut next = Box::pin(stream.next());
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(std::future::Future::poll(
            next.as_mut(),
            cx
        )))
        .await
        .is_pending()
    );
    tokio::time::sleep_until(until).await;
    assert_eq!(authority.permission()(), PermissionState::Pending);
    let terminal = std::future::poll_fn(|cx| {
        std::task::Poll::Ready(std::future::Future::poll(next.as_mut(), cx))
    })
    .await;
    assert!(matches!(terminal, std::task::Poll::Ready(None)));
    assert_eq!(initialised.load(Ordering::Acquire), 1);
    assert_eq!(polled.load(Ordering::Acquire), 0);
    assert!(!authority.owner.lease().private_context_lost());
}

struct Consumer(Option<tokio::task::JoinHandle<Vec<Vec<i16>>>>);
impl Consumer {
    async fn finish(&mut self) -> Vec<Vec<i16>> {
        let packets =
            tokio::time::timeout(std::time::Duration::from_secs(2), self.0.as_mut().unwrap())
                .await
                .unwrap()
                .unwrap();
        self.0.take();
        packets
    }
}
impl Drop for Consumer {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

#[tokio::test]
async fn completed_real_tts_response_waits_once_for_checking_then_resumes_or_drops() {
    for resume in [false, true] {
        let authority = Arc::new(Authority::new());
        let mut http = Http::start(200).await;
        let sends = Arc::new(AtomicUsize::new(0));
        let voice = Voice::Configured(
            Arc::new(
                Tts::with(
                    crate::second_config::TtsProvider::OpenAi,
                    &http.url,
                    "fixture",
                    None,
                )
                .unwrap()
                .counting_send_polls(sends.clone()),
            ),
            "fixture",
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let checking = authority.clone();
        let (noticed, notice) = tokio::sync::oneshot::channel();
        let noticed = Mutex::new(Some(noticed));
        let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            counted.fetch_add(1, Ordering::AcqRel);
            checking.checking();
            if let Some(noticed) = noticed.lock().unwrap().take() {
                let _ = noticed.send(());
            }
        });
        let permission = authority.permission();
        let mut speech = in_one_voice_near_guarded(
            voice,
            None,
            "PRIVATE_PENDING_TTS".into(),
            None,
            Some(permission.clone()),
            Some(hook),
        );
        let mut consuming = Consumer(Some(tokio::spawn(async move {
            let mut packets = Vec::new();
            while let Some(packet) = speech.next().await {
                packets.push(packet.unwrap());
            }
            packets
        })));
        tokio::time::timeout(std::time::Duration::from_secs(2), notice)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(permission(), PermissionState::Pending);
        assert_eq!(sends.load(Ordering::Acquire), 1);
        assert_eq!(http.bodies.lock().unwrap().len(), 1);
        if resume {
            authority.restore_seen();
        } else {
            authority.revoke();
        }
        let packets = consuming.finish().await;
        assert_eq!(packets.len(), usize::from(resume));
        assert_eq!(sends.load(Ordering::Acquire), 1);
        assert_eq!(calls.load(Ordering::Acquire), 1);
        http.finish().await;
    }
}

#[tokio::test]
async fn gemini_tts_text_post_requires_the_same_live_kernel_authority() {
    for revoke in [false, true] {
        let authority = Authority::new();
        let mut http = Http::start(200).await;
        let tts =
            GeminiTts::new(&http.url, "fixture-model", "fixture-key", "FixtureVoice").unwrap();
        let permission = authority.permission();
        if revoke {
            authority.revoke();
        }
        let speech = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            tts.speak_permitted("PRIVATE_GEMINI_CANARY", Some(&permission)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(speech.is_some(), !revoke);
        http.finish().await;
        assert_eq!(http.bodies.lock().unwrap().len(), usize::from(!revoke));
        if !revoke {
            let value: serde_json::Value =
                serde_json::from_slice(&http.bodies.lock().unwrap()[0]).unwrap();
            assert_eq!(
                value["contents"][0]["parts"][0]["text"],
                "PRIVATE_GEMINI_CANARY"
            );
        }
    }
}
