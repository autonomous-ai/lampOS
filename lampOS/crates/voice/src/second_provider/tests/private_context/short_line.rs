//! Two actual current-piece observations, never an inherited speaker alone.
use super::*;
use crate::speaking_faces::SpeakingFaces;
use lamp_msgs::SpeakingState::{No, Unknown, Yes};

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

const CANARY: &str = "PRIVATE SHORT PIECE HISTORY";
const BOOT: &str = "0123456789abcdef";

fn frame(people: &[(&str, lamp_msgs::SpeakingState)]) -> lamp_msgs::Faces {
    lamp_msgs::Faces {
        faces: people
            .iter()
            .map(|(id, state)| lamp_msgs::Face {
                id: (*id).into(),
                xmin: 0.1,
                ymin: 0.1,
                xmax: 0.3,
                ymax: 0.4,
                confidence: 1.0,
                yaw: None,
                facing: Some(true),
                speaking: Some(lamp_msgs::Speaking {
                    state: *state,
                    score: 0.9,
                    from: None,
                }),
            })
            .collect(),
    }
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
async fn a_short_current_piece_requires_two_unambiguous_positive_frames() {
    struct RestoreThread(Option<crate::thread::Thread>);
    impl Drop for RestoreThread {
        fn drop(&mut self) {
            *crate::thread::THREAD.lock().unwrap() = self.0.take().unwrap();
        }
    }
    for (case, carry) in [
        ("two", false),
        ("one", false),
        ("duplicate", false),
        ("none", false),
        ("no", false),
        ("unknown", false),
        ("other", false),
        ("second face", false),
        ("missing source", false),
        ("two", true),
        ("none", true),
        ("one", true),
        ("unknown", true),
        ("other", true),
        ("carried only", true),
    ] {
        let _one = crate::provider_worker::tests::one_thread();
        let _thread = RestoreThread(Some(std::mem::take(
            &mut *crate::thread::THREAD.lock().unwrap(),
        )));
        let directory = TicketDirectory::new();
        let known = directory.path.join("known");
        std::fs::write(&known, "A").unwrap();
        let eyes = Arc::new(SpeakingFaces::default());
        let fixture = crate::camera_authority::fixture::Fixture::start(
            known.clone(),
            [
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ],
            1,
        );
        let cache = fixture.cache();
        let heard = if carry {
            vec![Ok("Lamp.".into()), Ok("What is my name?".into())]
        } else {
            vec![Ok("What is my name?".into())]
        };
        let script = Arc::new(Script {
            heard: Mutex::new(heard.into()),
            chats: Mutex::new(VecDeque::from([
                vec![text("Answered."), Ok(ChatEvent::Done)],
                vec![text("Answered."), Ok(ChatEvent::Done)],
            ])),
            ..Script::default()
        });
        let reads = Arc::new(Mutex::new(Vec::new()));
        let read = reads.clone();
        let context_known = known.clone();
        let dispatch_known = known.clone();
        let mut engine = Engine::new(
            script.clone(),
            Told::default,
            Arc::new(|_, _, _, _| panic!("no unscoped tool in the short-line control")),
        )
        .with_memory_scope(
            lamp_turn_owner::BootId::new([43; 16]).unwrap(),
            cache.clone(),
            Arc::new(|_, _, _, _, _| panic!("no tool in the short-line control")),
            Some(Arc::new(move |lease| {
                let note = crate::second_provider::worker::known_name_told(
                    true,
                    Some(&context_known),
                    Some(lease),
                );
                let private = !note.is_empty();
                if private {
                    read.lock().unwrap().push(lease.key().request);
                }
                Told {
                    text: if private { note } else { "neutral".into() },
                    history: if private {
                        vec![Message::user(CANARY), Message::assistant("Kept.")]
                    } else {
                        Vec::new()
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
        .announcing(false);
        engine.memory_camera(true, 1);
        gap_owner_observation(&known);
        // This existing positive-only fixture publication explicitly becomes
        // V3, preserving its original observed_us and permission_until_us.
        let path = crate::speaking_faces::known_speaker_of(&known);
        let mut source: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        source["version"] = 3.into();
        source["camera_hold"] = 1.into();
        source["camera_began_us"] = 1.into();
        std::fs::write(path, source.to_string()).unwrap();
        warm_ticket(&fixture, "A", "f1");
        assert!(cache.replace_seen(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&known),
            Some(1)
        ));
        // Host timestamps are controlled, not a real camera/microphone replay.
        // The current half-second contains only the distinct looks at 100/350 ms.
        let anchor = Instant::now() - Duration::from_secs(6);
        let anchor_us = monotonic_us().checked_sub(6_000_000).unwrap();
        if carry {
            engine.submit(start(1));
            for look in 0..40u64 {
                eyes.look(
                    anchor + Duration::from_millis(look * 100 + 50),
                    Some(((BOOT, look + 1), &frame(&[("f1", Yes)]))),
                );
            }
            for block in 1..=400 {
                engine.submit(packet(1, block, anchor_us));
            }
            engine.submit(end(1));
            turn(&mut engine).await;
            assert!(crate::attention::only_the_name(
                &engine.shared.heard_last.lock().unwrap().as_ref().unwrap().1
            ));
            assert_eq!(engine.kept.as_ref().unwrap().samples.len(), KEPT_SAMPLES);
        }
        let before = script.asked.lock().unwrap().len();
        let request = if carry { 2 } else { 1 };
        let mut input = start(request);
        if let ProviderInput::Start { continues, .. } = &mut input {
            *continues = carry;
        }
        engine.submit(input);
        let lease = engine.memory_lease().unwrap();
        assert_eq!(
            lease.scope().unwrap().person(),
            Some(crate::remember::person_id("A").as_str())
        );
        assert!(lease.speaker().is_none());
        assert_eq!(
            *engine
                .turn
                .as_ref()
                .unwrap()
                .current_speaker_reads
                .lock()
                .unwrap(),
            (0, 0)
        );
        if carry {
            assert_eq!(
                engine
                    .turn
                    .as_ref()
                    .unwrap()
                    .speaker_reads
                    .lock()
                    .unwrap()
                    .0,
                anchor_us + 10_000
            );
        }
        let current = anchor + Duration::from_secs(4);
        for (index, millis) in [100, 350].into_iter().enumerate() {
            if case == "none" || case == "carried only" || (case == "one" && index == 1) {
                continue;
            }
            let people = if index == 1 {
                match case {
                    "no" => vec![("f1", No)],
                    "unknown" => vec![("f1", Unknown)],
                    "other" => vec![("f2", Yes)],
                    "second face" => vec![("f1", Yes), ("f2", Unknown)],
                    _ => vec![("f1", Yes)],
                }
            } else {
                vec![("f1", Yes)]
            };
            let sequence = if case == "duplicate" {
                100
            } else {
                100 + index as u64
            };
            eyes.look(
                current + Duration::from_millis(millis),
                Some(((BOOT, sequence), &frame(&people))),
            );
        }
        if carry && case != "other" {
            assert_eq!(
                eyes.positive_speaker(anchor, current + Duration::from_millis(500)),
                [GAP_OWNER_SPEAKER],
                "the carried majority really passes before the current-piece guard: {case}"
            );
        }
        if case == "missing source" {
            std::fs::remove_file(crate::speaking_faces::known_speaker_of(&known)).unwrap();
        }
        if case != "carried only" {
            for block in 1..=50 {
                engine.submit(packet(request, block, anchor_us + 4_000_000));
            }
        }
        engine.submit(end(request));
        let events = turn(&mut engine).await;
        assert_eq!(
            events.last().map(String::as_str),
            Some("complete"),
            "{case}/{carry}"
        );
        let proof = case == "two" || case == "missing source";
        assert_eq!(
            lease.speaker(),
            proof.then_some(GAP_OWNER_SPEAKER),
            "{case}/{carry}"
        );
        let expected = case == "two";
        let asked = script.asked.lock().unwrap();
        assert_eq!(
            asked.len(),
            before + 1,
            "neutral answers remain available: {case}/{carry}"
        );
        let messages = asked.last().unwrap();
        for words in [CANARY, "The person at the lamp now is A"] {
            assert_eq!(
                messages
                    .iter()
                    .any(|message| message.words().is_some_and(|text| text.contains(words))),
                expected,
                "{case}/{carry}: {words}"
            );
        }
        assert_eq!(
            reads.lock().unwrap().contains(&request),
            expected,
            "private callback entry: {case}/{carry}"
        );
        assert!(
            lease.is_current(),
            "denial does not revoke the admitted owner"
        );
    }
}
