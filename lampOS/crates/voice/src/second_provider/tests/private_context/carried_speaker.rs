//! Carried microphone input cannot lend an earlier speaker to a new piece.
use super::*;
use crate::speaking_faces::SpeakingFaces;

// A fresh private fixture namespace, canonicalized for the production no-link
// control-path contract. Its own files only are removed at teardown.
struct TicketDirectory {
    path: std::path::PathBuf,
    _owner: crate::process::SessionDirectory,
}
impl TicketDirectory {
    fn new() -> Self {
        let owner = crate::process::SessionDirectory::create().unwrap();
        Self {
            path: owner.path.canonicalize().unwrap(),
            _owner: owner,
        }
    }
}
impl Drop for TicketDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn warm_ticket(fixture: &crate::camera_authority::fixture::Fixture, name: &str, face: &str) {
    let mut owner = RequestScope::looking_later(
        RequestKey {
            worker: lamp_turn_owner::BootId::new([64; 16]).unwrap(),
            session: 1,
            request: 9000,
        },
        ScopeSnapshot::of(name),
        fixture.cache(),
    );
    let key = owner.lease().key();
    assert!(owner.speaker_recorder().unwrap().seal(
        key,
        &[format!("{BOOT}/{face}")],
        Instant::now() + Duration::from_secs(3)
    ));
    // Only the current hold's FD is cached. Target Engine requests still seal
    // and match their OWN current audio/identity; no KnownSpeaker pin transfers.
    assert_eq!(
        fixture.ready(&owner.lease()),
        crate::second_provider::PermissionState::Ready
    );
}

const NAME: &str = "Fixture owner";
const BOOT: &str = "0123456789abcdef";
const CANARY: &str = "PRIVATE CURRENT PIECE FACT";

struct PieceListener {
    words: String,
    finish_began: Option<Arc<tokio::sync::Notify>>,
    release: Option<tokio::sync::oneshot::Receiver<()>>,
    samples: Arc<AtomicUsize>,
}
impl Listener for PieceListener {
    fn send(&self, samples: &[i16]) -> net::Result<()> {
        self.samples.fetch_add(samples.len(), Ordering::Relaxed);
        Ok(())
    }
    fn finish(self: Box<Self>, _: u64) -> BoxFuture<'static, net::Result<String>> {
        Box::pin(async move {
            if let Some(began) = self.finish_began {
                began.notify_one();
            }
            if let Some(release) = self.release {
                let _ = release.await;
            }
            Ok(self.words)
        })
    }
}
struct Pieces {
    listeners: Mutex<VecDeque<PieceListener>>,
    messages: Mutex<Vec<Vec<Message>>>,
}
impl Services for Pieces {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        let listener = self
            .listeners
            .lock()
            .unwrap()
            .pop_front()
            .expect("exactly two admitted listeners");
        Box::pin(async move { Ok(Box::new(listener) as Box<dyn Listener>) })
    }
    fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
        self.messages.lock().unwrap().push(messages);
        stream::iter([text("Answered."), Ok(ChatEvent::Done)]).boxed()
    }
    fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        stream::iter([Ok(vec![1])]).boxed()
    }
}
fn face(id: &str, state: lamp_msgs::SpeakingState) -> lamp_msgs::Face {
    lamp_msgs::Face {
        id: id.into(),
        xmin: 0.1,
        ymin: 0.1,
        xmax: 0.3,
        ymax: 0.4,
        confidence: 1.0,
        yaw: None,
        facing: Some(true),
        speaking: Some(lamp_msgs::Speaking {
            state,
            score: 0.9,
            from: None,
        }),
    }
}
async fn completed(engine: &mut Engine) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match engine.next().await {
                ProviderOutput::TurnComplete { .. } => break,
                ProviderOutput::TurnFailed { .. } => panic!("the actual admitted turn must finish"),
                _ => (),
            }
        }
    })
    .await
    .expect("the scripted Engine turn finishes");
}
fn packet(request: u64, block: u64, start_us: u64) -> ProviderInput {
    ProviderInput::Audio {
        request,
        privacy_generation: 1,
        sequence: block,
        read_completed_at_us: start_us + block * 10_000,
        samples: vec![1; lamp_audio::CAPTURE_SAMPLES],
    }
}

#[tokio::test]
async fn carried_old_speech_requires_positive_evidence_on_the_actual_current_piece() {
    use lamp_msgs::SpeakingState::{No, Yes};
    for old_words in ["missing", "name-only"] {
        for current in ["unseen", "same", "different", "restart"] {
            let directory = TicketDirectory::new();
            let known = directory.path.join("known");
            let eyes = Arc::new(SpeakingFaces::default());
            // Controlled original host-read timestamps, not real-time playback
            // or an acoustic measurement: 4 s old input, followed by 1 s new.
            let anchor = Instant::now();
            let start = anchor - Duration::from_secs(6);
            let start_us = monotonic_us().checked_sub(6_000_000).unwrap();
            for look in 0..50u64 {
                let newer = look >= 40;
                let faces = lamp_msgs::Faces {
                    faces: if newer && current == "different" {
                        vec![face("f7", No), face("f8", Yes)]
                    } else {
                        vec![face(
                            "f7",
                            if !newer || current != "unseen" {
                                Yes
                            } else {
                                No
                            },
                        )]
                    },
                };
                let boot = if newer && current == "restart" {
                    "fedcba9876543210"
                } else {
                    BOOT
                };
                eyes.look(
                    start + Duration::from_millis(look * 100 + 50),
                    Some(((boot, look + 1), &faces)),
                );
            }
            assert_eq!(
                eyes.positive_speaker(start, start + Duration::from_secs(5)),
                if matches!(current, "unseen" | "same") {
                    vec![format!("{BOOT}/f7")]
                } else {
                    Vec::new()
                },
                "the unseen-speaker negative really satisfies the old combined majority"
            );
            std::fs::write(&known, NAME).unwrap();
            std::fs::write(crate::speaking_faces::known_face_of(&known), "f7").unwrap();
            std::fs::write(
                crate::speaking_faces::known_speaker_of(&known),
                json!({"version":3,"camera_hold":1,"camera_began_us":1,"state":"seen","publisher":"0123456789abcdef0123456789abcdef","generation":1,
            "observed_us":lamp_ipc::monotonic_us(),
            "permission_until_us":lamp_ipc::monotonic_us().checked_add(1_500_000).unwrap(),"name":NAME,"id":null,"boot":BOOT,"face":"f7"}).to_string(),
            )
            .unwrap();
            let fixture = crate::camera_authority::fixture::Fixture::start(
                known.clone(),
                [
                    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
                    0xab, 0xcd, 0xef,
                ],
                1,
            );
            warm_ticket(&fixture, NAME, "f7");
            let cache = fixture.cache();
            let first_samples = Arc::new(AtomicUsize::new(0));
            let second_samples = Arc::new(AtomicUsize::new(0));
            let finish_began = Arc::new(tokio::sync::Notify::new());
            let (hold, waiting) = tokio::sync::oneshot::channel();
            let services = Arc::new(Pieces {
                listeners: Mutex::new(VecDeque::from([
                    PieceListener {
                        words: "Lamp.".into(),
                        finish_began: Some(finish_began.clone()),
                        release: (old_words == "missing").then_some(waiting),
                        samples: first_samples.clone(),
                    },
                    PieceListener {
                        words: "What do you remember about me?".into(),
                        finish_began: None,
                        release: None,
                        samples: second_samples.clone(),
                    },
                ])),
                messages: Mutex::new(Vec::new()),
            });
            let context_known = known.clone();
            let dispatch_known = known.clone();
            let private_reads = Arc::new(Mutex::new(Vec::new()));
            let read = private_reads.clone();
            let mut engine = Engine::new(
                services.clone(),
                Told::default,
                Arc::new(|_, _, _, _| panic!("no unscoped tools")),
            )
            .with_memory_scope(
                lamp_turn_owner::BootId::new([41; 16]).unwrap(),
                cache.clone(),
                Arc::new(|_, _, _, _, _| panic!("no tools in the proof regression")),
                Some(Arc::new(move |lease| {
                    let private = crate::second_provider::worker::private_context_current(
                        Some(&context_known),
                        lease,
                    );
                    if private {
                        read.lock().unwrap().push(lease.key().request);
                    }
                    Told {
                        text: if private {
                            CANARY.into()
                        } else {
                            "neutral".into()
                        },
                        requires_speaker: private,
                        ..Told::default()
                    }
                })),
            )
            .guarding_private_context(Arc::new(move |lease| {
                crate::second_provider::worker::private_context_state(Some(&dispatch_known), lease)
            }))
            .seeing_who_speaks(eyes, known.clone())
            .announcing(false);
            engine.memory_camera(true, 1);
            assert!(cache.replace(
                cache.begin_refresh(),
                crate::known_faces::FaceObservation::read(&known)
            ));
            engine.submit(start_input(1));
            for block in 1..=400 {
                engine.submit(packet(1, block, start_us));
            }
            engine.submit(end(1));
            if old_words == "missing" {
                tokio::time::timeout(Duration::from_secs(2), finish_began.notified())
                    .await
                    .expect("old transcription is actually pending");
                assert!(engine.shared.heard_last.lock().unwrap().is_none());
            } else {
                completed(&mut engine).await;
                assert!(crate::attention::only_the_name(
                    &engine.shared.heard_last.lock().unwrap().as_ref().unwrap().1
                ));
            }
            assert_eq!(first_samples.load(Ordering::Relaxed), 4 * 16_000);
            let old_from = engine.kept.as_ref().unwrap().speaker_from;
            assert_eq!(old_from, start_us + 10_000);
            assert_eq!(
                engine.kept.as_ref().unwrap().samples.len(),
                KEPT_SAMPLES,
                "older audio was actually evicted"
            );
            let before = services.messages.lock().unwrap().len();
            let mut next = start_input(2);
            if let ProviderInput::Start { continues, .. } = &mut next {
                *continues = true;
            }
            engine.submit(next);
            assert_eq!(
                engine
                    .turn
                    .as_ref()
                    .unwrap()
                    .speaker_reads
                    .lock()
                    .unwrap()
                    .0,
                old_from,
                "the combined carry really happened"
            );
            assert_eq!(
                *engine
                    .turn
                    .as_ref()
                    .unwrap()
                    .current_speaker_reads
                    .lock()
                    .unwrap(),
                (0, 0),
                "current proof is never seeded from carry"
            );
            for block in 1..=100 {
                engine.submit(packet(2, block, start_us + 4_000_000));
            }
            engine.submit(end(2));
            completed(&mut engine).await;
            let lease = engine.memory_lease().unwrap();
            assert_eq!(
                lease.scope().unwrap().person(),
                Some(crate::remember::person_id(NAME).as_str()),
                "Old remains the fresh admitted person"
            );
            let expected = current == "same";
            assert_eq!(
                lease.speaker(),
                expected.then_some("0123456789abcdef/f7"),
                "{old_words}/{current}"
            );
            let messages = services.messages.lock().unwrap();
            assert_eq!(messages.len(), before + 1);
            assert_eq!(
                messages
                    .last()
                    .unwrap()
                    .iter()
                    .any(|message| message.words().is_some_and(|words| words.contains(CANARY))),
                expected,
                "{old_words}/{current}: actual Services dispatch"
            );
            assert_eq!(
                private_reads.lock().unwrap().contains(&2),
                expected,
                "no private read for the unseen new speaker"
            );
            let expected_samples = 16_000
                + if old_words == "missing" {
                    KEPT_SAMPLES
                } else {
                    0
                };
            assert_eq!(
                second_samples.load(Ordering::Relaxed),
                expected_samples,
                "carry's normal audio behavior stays"
            );
            assert!(
                crate::camera_authority::read(&known).is_some(),
                "the original known face is still fresh"
            );
            drop(hold);
        }
    }
}
fn start_input(request: u64) -> ProviderInput {
    start(request)
}

/// Requested enrollment and private disclosure share the captured line, but
/// retain their different positive-evidence bars and independent cancellation.
#[tokio::test]
async fn enrollment_and_private_disclosure_keep_independent_current_line_proofs() {
    use lamp_msgs::SpeakingState::Yes;
    for (case, looks, competitor, private, enrollment) in [
        ("both", 4, false, true, true),
        ("two disclosure looks", 2, false, true, false),
        ("minority speaker", 4, true, false, false),
    ] {
        let directory = TicketDirectory::new();
        let known = directory.path.join("known");
        std::fs::write(&known, NAME).unwrap();
        std::fs::write(crate::speaking_faces::known_face_of(&known), "f7").unwrap();
        std::fs::write(
            crate::speaking_faces::known_speaker_of(&known),
            json!({
                "version":3,"camera_hold":1,"camera_began_us":1,"state":"seen","publisher":"11111111111111111111111111111111","generation":1,
                "observed_us":lamp_ipc::monotonic_us(),
                "permission_until_us":lamp_ipc::monotonic_us().checked_add(1_500_000).unwrap(),
                "name":NAME,"id":null,"boot":BOOT,"face":"f7"
            })
            .to_string(),
        )
        .unwrap();
        let eyes = Arc::new(SpeakingFaces::default());
        let at = Instant::now();
        let now_us = monotonic_us();
        let first_us = now_us.checked_sub(1_000_000).unwrap();
        for look in 0..looks {
            let mut faces = vec![face("f7", Yes)];
            if competitor && look == 1 {
                faces.push(face("f8", Yes));
            }
            eyes.look(
                at - Duration::from_millis(900 - look * 100),
                Some(((BOOT, look + 1), &lamp_msgs::Faces { faces })),
            );
        }
        let words = "Please remember this face";
        let services = Arc::new(Pieces {
            listeners: Mutex::new(VecDeque::from([PieceListener {
                words: words.into(),
                finish_began: None,
                release: None,
                samples: Arc::new(AtomicUsize::new(0)),
            }])),
            messages: Mutex::new(Vec::new()),
        });
        let fixture =
            crate::camera_authority::fixture::Fixture::start(known.clone(), [0x11; 16], 1);
        warm_ticket(&fixture, NAME, "f7");
        let cache = fixture.cache();
        let context_known = known.clone();
        let dispatch_known = known.clone();
        let mut engine = Engine::new(
            services.clone(),
            Told::default,
            Arc::new(|_, _, _, _| panic!("no unscoped tools")),
        )
        .with_memory_scope(
            lamp_turn_owner::BootId::new([43; 16]).unwrap(),
            cache.clone(),
            Arc::new(|_, _, _, _, _| panic!("no tool is called in the composition control")),
            Some(Arc::new(move |lease| {
                let private = crate::second_provider::worker::private_context_current(
                    Some(&context_known),
                    lease,
                );
                Told {
                    text: if private {
                        CANARY.into()
                    } else {
                        "neutral".into()
                    },
                    requires_speaker: private,
                    ..Told::default()
                }
            })),
        )
        .guarding_private_context(Arc::new(move |lease| {
            crate::second_provider::worker::private_context_state(Some(&dispatch_known), lease)
        }))
        .seeing_who_speaks(eyes.clone(), known.clone())
        .remembering_speaker(true, Some(eyes))
        .announcing(false);
        engine.memory_camera(true, 1);
        assert!(cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&known)
        ));
        let request = 3_306_401
            + if looks == 2 {
                1
            } else if competitor {
                2
            } else {
                0
            };
        engine.submit(start_input(request));
        for block in 1..=50 {
            engine.submit(packet(request, block, first_us));
        }
        engine.submit(end(request));
        completed(&mut engine).await;
        let chats = services.messages.lock().unwrap();
        assert_eq!(
            chats
                .iter()
                .flatten()
                .any(|message| message.words().is_some_and(|words| words.contains(CANARY))),
            private,
            "{case}"
        );
        drop(chats);
        let line =
            crate::speaking_faces::line_seen(request).expect("the actual line was published");
        assert_eq!(line.enrollment.is_some(), enrollment, "{case}");
        if let Some(proof) = line.enrollment {
            assert_eq!(proof.face, format!("{BOOT}/f7"));
            assert_eq!(proof.heard, words);
            assert_eq!(
                (proof.began_us, proof.ended_us),
                (first_us, first_us + 500_000)
            );
            assert!(proof.is_current(monotonic_us()));
            engine.stop_through(request);
            assert!(!proof.is_current(monotonic_us()));
        }
    }
}
