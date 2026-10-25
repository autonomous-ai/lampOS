//! `LAMP_PERSON_SCOPE_SEEN`: a line is in a known person's scope only when
//! their own face was seen speaking it, else the room's (reviewer-three's P1
//! and P2 on #3435, Second's late-keeper case at 09:24).
use super::*;
use crate::memory_scope::{FaceCache, KEEPER_WAIT, ScopeLease};
use crate::speaking_faces::{SpeakingFaces, known_face_of};
use std::sync::atomic::{AtomicBool, Ordering};

fn known_file(what: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "scope-seen-{what}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, "A").unwrap();
    std::fs::write(known_face_of(&path), "f1\n").unwrap();
    path
}

fn clean(path: &std::path::Path) {
    let _ = std::fs::remove_file(known_face_of(path));
    let _ = std::fs::remove_file(path);
}

/// The eyes while a line is said: the owner's face `f1` in view, speaking
/// it or silent, until `done`.
fn feed(
    eyes: Arc<SpeakingFaces>,
    f1_speaks: bool,
    done: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut frame = 0u64;
        while !done.load(Ordering::Acquire) {
            let faces = lamp_msgs::Faces {
                faces: vec![lamp_msgs::Face {
                    id: "f1".to_owned(),
                    xmin: 0.1,
                    ymin: 0.1,
                    xmax: 0.3,
                    ymax: 0.4,
                    confidence: 1.0,
                    yaw: None,
                    facing: None,
                    speaking: Some(lamp_msgs::Speaking {
                        state: if f1_speaks {
                            lamp_msgs::SpeakingState::Yes
                        } else {
                            lamp_msgs::SpeakingState::No
                        },
                        score: 0.9,
                        from: None,
                    }),
                }],
            };
            frame += 1;
            eyes.look(Instant::now(), Some((("eyes", frame), &faces)));
            std::thread::sleep(Duration::from_millis(2));
        }
    })
}

/// What one line admitted as the owner A's came to: the scope each memory
/// call ran in, the scope the line's lease ends in, and what its first
/// round was told.
struct Outcome {
    calls: Vec<Option<String>>,
    lease: Option<String>,
    told: Message,
}

async fn one_line(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    f1_speaks: bool,
    seen: bool,
) -> Outcome {
    let path = known_file(what);
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(words.to_owned())].into()),
        chats: Mutex::new(chats.into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let cache = Arc::new(FaceCache::default());
    let calls: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let leases: Arc<Mutex<Vec<ScopeLease>>> = Arc::default();
    let (seen_calls, seen_leases) = (calls.clone(), leases.clone());
    let runner: ScopedToolRunner = Arc::new(move |_, _, _, _, lease: ScopeLease| {
        seen_calls
            .lock()
            .unwrap()
            .push(lease.scope().unwrap().person().map(str::to_owned));
        Box::pin(async { json!({"ok":true}) })
    });
    let eyes = Arc::new(SpeakingFaces::default());
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([41; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|lease: &ScopeLease| {
                Told::from(format!("for {:?}", lease.scope().unwrap().person()))
            })),
        )
        .seeing_who_speaks(eyes.clone(), path.clone())
        .scoping_by_sight(seen)
        .remembering(Arc::new(move |_, _, lease: Option<&ScopeLease>| {
            seen_leases.lock().unwrap().extend(lease.cloned());
            None
        }))
        .announcing(false);
    engine.memory_camera(true, 1);
    cache.replace(
        cache.begin_refresh(),
        crate::known_faces::FaceObservation::read(&path),
    );
    let done = Arc::new(AtomicBool::new(false));
    let feeder = feed(eyes, f1_speaks, done.clone());
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    tokio::time::sleep(Duration::from_millis(40)).await;
    engine.submit(end(1));
    turn(&mut engine).await;
    done.store(true, Ordering::Release);
    feeder.join().unwrap();
    clean(&path);
    let lease = leases.lock().unwrap().first().and_then(|lease| {
        lease
            .scope()
            .and_then(|scope| scope.person().map(str::to_owned))
    });
    let told = script.asked.lock().unwrap()[0][0].clone();
    Outcome {
        calls: calls.lock().unwrap().clone(),
        lease,
        told,
    }
}

fn forget_everything() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "m1",
                crate::remember::FORGET_EVERYTHING,
                json!({}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Done."), Ok(ChatEvent::Done)],
    ]
}

const FORGET_ME: &str = "Lamp, forget everything about me.";

/// The owner's face in view and silent; an off-camera voice asks the lamp
/// to forget everything about "me". The forget runs in the room's scope,
/// where `forget_everything` refuses (`remember.rs`, nobody known), never
/// in the owner's: their memory is untouched (reviewer-three's P1).
#[tokio::test]
async fn an_off_camera_forget_everything_never_reaches_the_silent_owners_memory() {
    let got = one_line("forget", FORGET_ME, forget_everything(), false, true).await;
    assert_eq!(got.calls, [None], "the forget runs as the room's");
    assert_eq!(got.told, Message::system("for None"));
}

/// An off-camera colleague's "I'm flying to Tokyo next week" with the owner
/// silent in view: told as the room's and its lease the room's, which is
/// the scope its exchange is kept and observed under
/// (`FinishedExchange::whose`), never the owner's (reviewer-three's P2).
#[tokio::test]
async fn an_off_camera_line_is_not_observed_or_kept_as_the_silent_owners() {
    let chats = vec![vec![text("Have a good trip."), Ok(ChatEvent::Done)]];
    let got = one_line(
        "tokyo",
        "I'm flying to Tokyo next week.",
        chats,
        false,
        true,
    )
    .await;
    assert_eq!(got.lease, None, "the line's lease is the room's");
    assert_eq!(got.told, Message::system("for None"));
}

/// The owner's own face seen speaking the line keeps it theirs.
#[tokio::test]
async fn the_owners_own_face_seen_speaking_keeps_their_scope() {
    let a = crate::remember::person_id("A");
    let got = one_line("owner", FORGET_ME, forget_everything(), true, true).await;
    assert_eq!(got.calls, [Some(a.clone())]);
    assert_eq!(got.lease, Some(a.clone()));
    assert_eq!(
        got.told,
        Message::system(format!("for {:?}", Some(a.as_str())))
    );
}

/// Off, nothing changes: the silent owner's line stays theirs, as on
/// lamp-voice before the switch.
#[tokio::test]
async fn without_the_switch_an_off_camera_line_stays_the_silent_owners() {
    let a = crate::remember::person_id("A");
    let got = one_line("off", FORGET_ME, forget_everything(), false, false).await;
    assert_eq!(got.calls, [Some(a.clone())]);
    assert_eq!(got.lease, Some(a.clone()));
    let got = one_line("owner-off", FORGET_ME, forget_everything(), true, false).await;
    assert_eq!(got.calls, [Some(a)]);
}

/// Second's case at 09:24 on #3435: after a restart the cache is cold, so
/// the line is admitted as the room's and looks later. The owner's face is
/// in view and silent from the line's start, their identity arrives within
/// the keeper bound, and an off-camera voice asks for a keep: the late
/// identity alone gives no keeper with the switch, and does without it.
#[tokio::test]
async fn a_late_looks_keeper_needs_the_owners_own_face_seen_speaking() {
    let a = crate::remember::person_id("A");
    for (what, f1_speaks, seen, expected) in [
        ("silent, on", false, true, None),
        ("silent, off", false, false, Some(a.clone())),
        ("speaking, on", true, true, Some(a.clone())),
    ] {
        let path = known_file("late");
        let script = Arc::new(Script {
            heard: Mutex::new(
                [Ok(
                    "Lamp, remember my sister's wedding is Saturday.".to_owned()
                )]
                .into(),
            ),
            chats: Mutex::new(
                [
                    vec![
                        Ok(ChatEvent::ToolCalls(vec![call(
                            "m1",
                            "remember",
                            json!({"text":"sister's wedding is Saturday"}),
                        )])),
                        Ok(ChatEvent::Done),
                    ],
                    vec![text("Got it."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let (engine, _, _) = scripted(script);
        let cache = Arc::new(FaceCache::default());
        let keepers: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
        let kept = keepers.clone();
        let runner: ScopedToolRunner = Arc::new(move |_, _, _, _, lease: ScopeLease| {
            let kept = kept.clone();
            Box::pin(async move {
                let keeper = tokio::task::spawn_blocking(move || lease.keeper_within(KEEPER_WAIT))
                    .await
                    .unwrap();
                kept.lock().unwrap().push(keeper);
                json!({"ok":true})
            })
        });
        let eyes = Arc::new(SpeakingFaces::default());
        let mut engine = engine
            .with_memory_scope(
                lamp_turn_owner::BootId::new([42; 16]).unwrap(),
                cache.clone(),
                runner,
                Some(Arc::new(|lease: &ScopeLease| {
                    Told::from(format!("for {:?}", lease.scope().unwrap().person()))
                })),
            )
            .seeing_who_speaks(eyes.clone(), path.clone())
            .scoping_by_sight(seen)
            .announcing(false);
        engine.memory_camera(true, 1);
        // Cold: the voice restarted a second ago and nobody is named yet.
        cache.cold_since(Instant::now() - Duration::from_secs(1));
        assert!(cache.is_cold(Instant::now()), "{what}: cold");
        let done = Arc::new(AtomicBool::new(false));
        let feeder = feed(eyes, f1_speaks, done.clone());
        engine.submit(start(1));
        let lease = engine
            .turn
            .as_ref()
            .unwrap()
            .memory_scope
            .as_ref()
            .unwrap()
            .lease();
        assert_eq!(lease.scope().unwrap().person(), None, "{what}: the room's");
        assert!(lease.looks_later(), "{what}");
        // The eyes name A while the line is said.
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
        engine.submit(audio(1, 1));
        tokio::time::sleep(Duration::from_millis(40)).await;
        engine.submit(end(1));
        turn(&mut engine).await;
        done.store(true, Ordering::Release);
        feeder.join().unwrap();
        clean(&path);
        assert_eq!(*keepers.lock().unwrap(), [expected], "{what}");
    }
}
