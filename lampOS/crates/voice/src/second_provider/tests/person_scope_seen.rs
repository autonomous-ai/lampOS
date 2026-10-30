//! `LAMP_PERSON_SCOPE_SEEN`: a line is in a known person's scope only when
//! their own face was seen speaking it, else the room's (reviewer-three's P1
//! and P2 on #3435, Second's late-keeper case at 09:24).
use super::*;
use crate::memory_scope::{FaceCache, KEEPER_WAIT, ScopeLease};
use crate::speaking_faces::{SpeakingFaces, known_face_of};
use std::sync::atomic::{AtomicBool, Ordering};

/// The eyes' known file naming A with face `f1`, written by the eyes of
/// boot [`BOOT`] after their first frame (as `lamp-eyes` writes it), so
/// `known_with_boot` pairs it with that boot.
fn known_file(what: &str, eyes: &SpeakingFaces) -> std::path::PathBuf {
    eyes.look(
        Instant::now(),
        Some(((BOOT, 0), &lamp_msgs::Faces { faces: vec![] })),
    );
    // No tolerance at the boot's first frame (Fourth's P2 on #3038).
    std::thread::sleep(Duration::from_millis(5));
    let path = std::env::temp_dir().join(format!(
        "scope-seen-{what}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&path, "A").unwrap();
    std::fs::write(known_face_of(&path), "f1\n").unwrap();
    path
}

/// The eyes' boot the known file was written under.
const BOOT: &str = "eyes";

/// The worker's own setup (`worker::see_who_speaks`) with
/// `LAMP_SECOND_SPEAKING_FACE` on and `LAMP_PERSON_SCOPE_SEEN` as `seen`,
/// whose eyes' reader fails to start: a relative bus root, which
/// `lamp_bus::Bus::new` refuses (reviewer-two's P2, Second's P3 on #3437).
fn failed_reader(engine: Engine, known: &std::path::Path, seen: bool) -> Engine {
    assert!(engine.scopes_context());
    let (engine, eyes, reader) = super::super::worker::see_who_speaks(
        engine,
        Some(&known.to_path_buf()),
        Some(std::path::Path::new("relative-bus-root")),
        true,
        seen,
    );
    assert!(
        eyes.is_none() && reader.is_none(),
        "the reader never started"
    );
    engine
}

fn clean(path: &std::path::Path) {
    let _ = std::fs::remove_file(known_face_of(path));
    let _ = std::fs::remove_file(path);
}

/// The eyes of boot `boot` while a line is said: a face `f1` in view,
/// speaking it or silent, until `done`. Under another boot than [`BOOT`]
/// the eyes started again and their `f1` is a different face.
fn feed(
    eyes: Arc<SpeakingFaces>,
    f1_speaks: bool,
    boot: &'static str,
    done: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    feed_faces(eyes, vec![("f1", f1_speaks)], boot, done)
}

/// [`feed`] with `in_view` the faces in each look, by passing id, each
/// speaking it or silent; none, looks with no face in them.
fn feed_faces(
    eyes: Arc<SpeakingFaces>,
    in_view: Vec<(&'static str, bool)>,
    boot: &'static str,
    done: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut frame = 0u64;
        while !done.load(Ordering::Acquire) {
            let faces = lamp_msgs::Faces {
                faces: in_view
                    .iter()
                    .map(|&(id, speaks)| lamp_msgs::Face {
                        id: id.to_owned(),
                        xmin: 0.1,
                        ymin: 0.1,
                        xmax: 0.3,
                        ymax: 0.4,
                        confidence: 1.0,
                        yaw: None,
                        facing: None,
                        speaking: Some(lamp_msgs::Speaking {
                            state: if speaks {
                                lamp_msgs::SpeakingState::Yes
                            } else {
                                lamp_msgs::SpeakingState::No
                            },
                            score: 0.9,
                            from: None,
                        }),
                    })
                    .collect(),
            };
            frame += 1;
            eyes.look(Instant::now(), Some(((boot, frame), &faces)));
            std::thread::sleep(Duration::from_millis(2));
        }
    })
}

/// What one line admitted as the owner A's came to: the scope each memory
/// call ran in, the scope the line's lease ends in, and what its first
/// round was told.
/// A memory note asked: whose and on which words.
type Noted = (Option<String>, String);

struct Outcome {
    calls: Vec<Option<String>>,
    lease: Option<String>,
    told: Message,
    /// Each memory note asked, as it was asked.
    noted: Vec<Noted>,
    /// Whose context each chat was told, in the order asked.
    asked: Vec<Message>,
    /// Every output of the turn, as [`turn`]'s labels.
    labels: Vec<String>,
    /// Whether the line's lease ends held, told from the conversation as
    /// it stood ([`crate::memory_scope::ScopeSnapshot::held`]).
    held: bool,
}

async fn one_line(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    f1_speaks: bool,
    boot: &'static str,
    seen: bool,
) -> Outcome {
    one_line_eyes(what, words, chats, f1_speaks, boot, seen, true, None).await
}

/// [`one_line`], with `eyes_up` false set up by the worker with a reader
/// that fails to start ([`failed_reader`]): the engine never gets the
/// eyes.
#[allow(clippy::too_many_arguments)]
async fn one_line_eyes(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    f1_speaks: bool,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
) -> Outcome {
    let in_view = vec![("f1", f1_speaks)];
    one_line_with(
        what, words, chats, in_view, boot, seen, eyes_up, early, None, true,
    )
    .await
}

/// [`one_line_eyes`] with `in_view` the faces in each of the eyes' looks
/// ([`feed_faces`]) and `going_on` whether A is in a conversation going on
/// (`LAMP_CONVERSATION_CARRIES`, #4339); none, the switch off. With
/// `named` false the face cache names nobody, so the line is admitted as the
/// room's (lamp-06c1's turns with no face line, 29 Oct).
#[allow(clippy::too_many_arguments)]
async fn one_line_with(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    in_view: Vec<(&'static str, bool)>,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
    going_on: Option<bool>,
    named: bool,
) -> Outcome {
    let eyes = Arc::new(SpeakingFaces::default());
    let path = known_file(what, &eyes);
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(words.to_owned())].into()),
        chats: Mutex::new(chats.into()),
        early: Mutex::new(
            early
                .map(|early| vec![net::Early::Words(early.to_owned())])
                .into_iter()
                .collect(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    // `LAMP_SECOND_PREPARED`, with early words to prepare on.
    let engine = engine.preparing(early.is_some());
    let noted: Arc<Mutex<Vec<Noted>>> = Arc::default();
    let seen_noted = noted.clone();
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
    // The lease each context was told for: a held line asks memory nothing,
    // so it never reaches `remembering` below.
    let told_leases: Arc<Mutex<Vec<ScopeLease>>> = Arc::default();
    let seen_told = told_leases.clone();
    let engine = engine.with_memory_scope(
        lamp_turn_owner::BootId::new([41; 16]).unwrap(),
        cache.clone(),
        runner,
        Some(Arc::new(move |lease: &ScopeLease| {
            seen_told.lock().unwrap().push(lease.clone());
            Told::from(format!("for {:?}", lease.scope().unwrap().person()))
        })),
    );
    let engine = if eyes_up {
        engine
            .seeing_who_speaks(eyes.clone(), path.clone())
            .scoping_by_sight(seen)
    } else {
        failed_reader(engine, &path, seen)
    };
    let a = crate::remember::person_id("A");
    let engine = engine.carrying_conversations_by(going_on.map(
        |going_on| -> crate::second_provider::GoingOnWith {
            Arc::new(move |_| going_on.then(|| a.clone()))
        },
    ));
    let mut engine = engine
        .remembering(Arc::new(
            move |_, words: &str, lease: Option<&ScopeLease>| {
                seen_noted.lock().unwrap().push((
                    lease
                        .and_then(|lease| lease.scope())
                        .and_then(|scope| scope.person().map(str::to_owned)),
                    words.to_owned(),
                ));
                seen_leases.lock().unwrap().extend(lease.cloned());
                None
            },
        ))
        .announcing(false);
    engine.memory_camera(true, 1);
    if named {
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
    }
    let done = Arc::new(AtomicBool::new(false));
    let feeder = feed_faces(eyes, in_view, boot, done.clone());
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    tokio::time::sleep(Duration::from_millis(40)).await;
    engine.submit(end(1));
    let labels = turn(&mut engine).await;
    done.store(true, Ordering::Release);
    feeder.join().unwrap();
    clean(&path);
    // The line's lease: as its context was told, else as memory was asked.
    let first = told_leases
        .lock()
        .unwrap()
        .first()
        .cloned()
        .or_else(|| leases.lock().unwrap().first().cloned());
    let lease = first.as_ref().and_then(|lease| {
        lease
            .scope()
            .and_then(|scope| scope.person().map(str::to_owned))
    });
    let held = first
        .as_ref()
        .and_then(ScopeLease::scope)
        .is_some_and(crate::memory_scope::ScopeSnapshot::held);
    let asked: Vec<Message> = script
        .asked
        .lock()
        .unwrap()
        .iter()
        .map(|chat| chat[0].clone())
        .collect();
    Outcome {
        calls: calls.lock().unwrap().clone(),
        lease,
        told: asked[0].clone(),
        noted: noted.lock().unwrap().clone(),
        asked,
        labels,
        held,
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
    let got = one_line("forget", FORGET_ME, forget_everything(), false, BOOT, true).await;
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
        BOOT,
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
    let got = one_line("owner", FORGET_ME, forget_everything(), true, BOOT, true).await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a.clone()));
    assert_eq!(got.lease, Some(a.clone()));
    assert_eq!(
        got.told,
        Message::system(format!("for {:?}", Some(a.as_str())))
    );
}

/// Second's P1 on #3437: A's fresh files name `f1` from before the eyes
/// started again, and the new eyes' `f1`, a different face, says "forget
/// everything about me". The bare id matches, the boot does not: the line
/// is the room's. Off, it stays A's, as before.
#[tokio::test]
async fn a_reused_id_after_the_eyes_restart_is_not_the_known_persons_face() {
    let a = crate::remember::person_id("A");
    let got = one_line(
        "restart",
        FORGET_ME,
        forget_everything(),
        true,
        "restarted",
        true,
    )
    .await;
    assert_eq!(got.calls, [None], "the forget runs as the room's");
    assert_eq!(got.lease, None);
    assert_eq!(got.told, Message::system("for None"));
    let got = one_line(
        "restart-off",
        FORGET_ME,
        forget_everything(),
        true,
        "restarted",
        false,
    )
    .await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a));
}

/// Reviewer-two's P2 on #3437: the eyes' reader never started (the bus
/// would not open), so nothing reads who spoke. With the switch that is no
/// face seen speaking it: the line is the room's. Off, it stays A's.
#[tokio::test]
async fn with_no_eyes_to_read_a_line_is_the_rooms() {
    let a = crate::remember::person_id("A");
    let got = one_line_eyes(
        "blind",
        FORGET_ME,
        forget_everything(),
        true,
        BOOT,
        true,
        false,
        None,
    )
    .await;
    assert_eq!(got.calls, [None], "the forget runs as the room's");
    assert_eq!(got.lease, None);
    let got = one_line_eyes(
        "blind-off",
        FORGET_ME,
        forget_everything(),
        true,
        BOOT,
        false,
        false,
        None,
    )
    .await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a));
}

const TOKYO: &str = "I'm flying to Tokyo next week.";
/// The scripted voice's audio label for the reply.
const AUDIO: &str = "audio:17";
const TOKYO_EARLY: &str = "i'm flying to tokyo next week";

/// A chat that answers in words, enough for a prepared round and another.
fn answers() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![text("Have a good trip."), Ok(ChatEvent::Done)],
        vec![text("Have a good trip."), Ok(ChatEvent::Done)],
    ]
}

/// The scripted reply said once, its audio played, and the turn complete,
/// not failed.
fn spoken(got: &Outcome, why: &str) {
    let said: Vec<&String> = got
        .labels
        .iter()
        .filter(|label| label.starts_with("said:"))
        .collect();
    assert_eq!(said, ["said:Have a good trip."], "{why}: {:?}", got.labels);
    assert!(
        got.labels.iter().any(|label| label == AUDIO),
        "{why}: its audio, {:?}",
        got.labels
    );
    assert_eq!(
        got.labels.last().map(String::as_str),
        Some("complete"),
        "{why}: {:?}",
        got.labels
    );
}

/// One line with `LAMP_SECOND_PREPARED` and early words.
async fn prepared_line(what: &str, f1_speaks: bool, seen: bool) -> Outcome {
    one_line_eyes(
        what,
        TOKYO,
        answers(),
        f1_speaks,
        BOOT,
        seen,
        true,
        Some(TOKYO_EARLY),
    )
    .await
}

/// Second's P2 on #3437: with `LAMP_SECOND_PREPARED`, the early words
/// would ask memory and a chat told the owner's context before the line's
/// end shows their face never spoke it. With the switch nothing is asked
/// for them early: memory and the one chat are the room's, on the final
/// words, and the room's answer comes.
#[tokio::test]
async fn an_off_camera_lines_early_words_ask_nothing_for_the_silent_owner() {
    let got = prepared_line("early-silent", false, true).await;
    assert_eq!(
        got.noted,
        [(None, TOKYO.to_owned())],
        "memory, the room's, on the final words"
    );
    assert_eq!(
        got.asked,
        [Message::system("for None")],
        "one chat, the room's"
    );
    assert_eq!(got.lease, None);
    spoken(&got, "the room's answer comes");
}

/// The owner seen speaking: not prepared early either, and their answer is
/// asked on the final words in their scope.
#[tokio::test]
async fn the_owner_seen_speaking_is_answered_as_theirs_on_the_final_words() {
    let a = crate::remember::person_id("A");
    let got = prepared_line("early-owner", true, true).await;
    assert_eq!(got.noted, [(Some(a.clone()), TOKYO.to_owned())]);
    assert_eq!(
        got.asked,
        [Message::system(format!("for {:?}", Some(a.as_str())))]
    );
    spoken(&got, "answered as theirs");
}

/// Off, the early words prepare as before: memory asked on them for the
/// owner, and the prepared round stands.
#[tokio::test]
async fn without_the_switch_the_early_words_still_prepare() {
    let a = crate::remember::person_id("A");
    let got = prepared_line("early-off", false, false).await;
    assert_eq!(got.noted, [(Some(a.clone()), TOKYO_EARLY.to_owned())]);
    assert_eq!(
        got.asked,
        [Message::system(format!("for {:?}", Some(a.as_str())))]
    );
}

/// Off, nothing changes: the silent owner's line stays theirs, as on
/// lamp-voice before the switch.
#[tokio::test]
async fn without_the_switch_an_off_camera_line_stays_the_silent_owners() {
    let a = crate::remember::person_id("A");
    let got = one_line("off", FORGET_ME, forget_everything(), false, BOOT, false).await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a.clone()));
    assert_eq!(got.lease, Some(a.clone()));
    let got = one_line(
        "owner-off",
        FORGET_ME,
        forget_everything(),
        true,
        BOOT,
        false,
    )
    .await;
    // A's own: held for their yes, never run on the ask (#3504, N3).
    assert!(got.calls.is_empty(), "held for A's yes");
    assert_eq!(got.lease, Some(a));
}

/// Second's case at 09:24 on #3435: after a restart the cache is cold, so
/// the line is admitted as the room's and looks later. The owner's face is
/// in view and silent from the line's start, their identity arrives within
/// the keeper bound, and an off-camera voice asks for a keep: the late
/// identity alone gives no keeper with the switch, and does without it.
#[tokio::test]
async fn a_late_looks_keeper_needs_the_owners_own_face_seen_speaking() {
    let a = crate::remember::person_id("A");
    for (what, f1_speaks, boot, seen, eyes_up, expected) in [
        ("silent, on", false, BOOT, true, true, None),
        ("silent, off", false, BOOT, false, true, Some(a.clone())),
        ("speaking, on", true, BOOT, true, true, Some(a.clone())),
        // Second's P1 on #3437: the eyes started again and their new `f1`,
        // a different face, says it.
        ("restarted, on", true, "restarted", true, true, None),
        (
            "restarted, off",
            true,
            "restarted",
            false,
            true,
            Some(a.clone()),
        ),
        // Reviewer-two's P2 on #3437: the eyes' reader never started.
        ("no eyes, on", true, BOOT, true, false, None),
        ("no eyes, off", true, BOOT, false, false, Some(a.clone())),
    ] {
        let eyes = Arc::new(SpeakingFaces::default());
        let path = known_file("late", &eyes);
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
        let engine = engine.with_memory_scope(
            lamp_turn_owner::BootId::new([42; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|lease: &ScopeLease| {
                Told::from(format!("for {:?}", lease.scope().unwrap().person()))
            })),
        );
        let engine = if eyes_up {
            engine
                .seeing_who_speaks(eyes.clone(), path.clone())
                .scoping_by_sight(seen)
        } else {
            failed_reader(engine, &path, seen)
        };
        let mut engine = engine.announcing(false);
        engine.memory_camera(true, 1);
        // Cold: the voice restarted a second ago and nobody is named yet.
        cache.cold_since(Instant::now() - Duration::from_secs(1));
        assert!(cache.is_cold(Instant::now()), "{what}: cold");
        let done = Arc::new(AtomicBool::new(false));
        let feeder = feed(eyes, f1_speaks, boot, done.clone());
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

/// #4339, lamp-3e83: A's line no face was seen speaking, A's own face in
/// view and silent, in A's conversation going on (`LAMP_CONVERSATION_CARRIES`):
/// told from A's conversation as it stood, a held scope, not the room's,
/// so the chat keeps the talk it was part of. With no face in view at all
/// from the start, it is the room's: an unseen line carries only within
/// `CARRY_UNSEEN_WITHIN` of their face in view (the Reviewer-lead's P2 on
/// #4479).
#[tokio::test]
async fn an_unseen_line_in_their_conversation_going_on_keeps_that_conversation() {
    let a = crate::remember::person_id("A");
    for (what, in_view) in [("carried", vec![("f1", false)])] {
        let chats = vec![vec![text("Floral, then."), Ok(ChatEvent::Done)]];
        let got = one_line_with(
            what,
            "The floral one, I think.",
            chats,
            in_view,
            BOOT,
            true,
            true,
            None,
            Some(true),
            true,
        )
        .await;
        assert_eq!(
            got.lease,
            Some(a.clone()),
            "{what}: A's conversation, not the room's"
        );
        assert!(got.held, "{what}: told from the conversation as it stood");
        assert_eq!(
            got.told,
            Message::system(format!("for {:?}", Some(a.as_str()))),
            "{what}"
        );
    }
}

/// #4339: carried, a memory call on that line runs for no one, as a
/// gap-held line's does: an off-camera "forget everything about me" reaches
/// nobody's memory (reviewer-three's P1 on #3435 holds).
#[tokio::test]
async fn a_carried_lines_memory_calls_run_for_no_one() {
    let got = one_line_with(
        "carried-forget",
        FORGET_ME,
        forget_everything(),
        vec![("f1", false)],
        BOOT,
        true,
        true,
        None,
        Some(true),
        true,
    )
    .await;
    assert!(got.calls.is_empty(), "no memory call runs: {:?}", got.calls);
    assert!(got.held);
}

/// #4339: a line no face was seen speaking stays the room's, as before,
/// when A isn't in a conversation going on (a 45 s quiet, or someone
/// else's exchange between), with the switch off, with a face not A's in
/// view, and with another face seen speaking it.
#[tokio::test]
async fn without_their_conversation_or_with_another_face_an_unseen_line_is_the_rooms() {
    for (what, in_view, going_on) in [
        ("quiet", vec![("f1", false)], Some(false)),
        ("off", vec![("f1", false)], None),
        ("empty-from-start", vec![], Some(true)),
        ("other-face", vec![("f2", false)], Some(true)),
        ("two-faces", vec![("f1", false), ("f2", false)], Some(true)),
        (
            "other-speaks",
            vec![("f1", false), ("f2", true)],
            Some(true),
        ),
    ] {
        let chats = vec![vec![text("Okay."), Ok(ChatEvent::Done)]];
        let got = one_line_with(
            what,
            "The floral one, I think.",
            chats,
            in_view,
            BOOT,
            true,
            true,
            None,
            going_on,
            true,
        )
        .await;
        assert_eq!(got.lease, None, "{what}: the room's");
        assert!(!got.held, "{what}");
        assert_eq!(got.told, Message::system("for None"), "{what}");
    }
}

/// #4339: A's own face seen speaking it is A's as ever, with memory, not
/// merely carried.
#[tokio::test]
async fn their_face_seen_speaking_is_theirs_not_carried() {
    let a = crate::remember::person_id("A");
    let chats = vec![vec![text("Floral."), Ok(ChatEvent::Done)]];
    let got = one_line_with(
        "own",
        "The floral one, I think.",
        chats,
        vec![("f1", true)],
        BOOT,
        true,
        true,
        None,
        Some(true),
        true,
    )
    .await;
    assert_eq!(got.lease, Some(a));
    assert!(!got.held, "seen speaking: theirs, with memory");
}

/// #4339, lamp-06c1's turns with no face line (29 Oct): a line admitted as
/// the room's, the face cache naming nobody, in A's conversation going on,
/// with only A's face in view and none seen speaking it: carried as A's
/// conversation, held. Not in one, with an empty view from the start, or
/// with another face in view or speaking it, it stays the room's.
#[tokio::test]
async fn a_rooms_line_in_their_conversation_going_on_is_carried_as_it() {
    let a = crate::remember::person_id("A");
    for (what, in_view, going_on, carried) in [
        ("room-own", vec![("f1", false)], Some(true), true),
        ("room-none", vec![], Some(true), false),
        ("room-quiet", vec![("f1", false)], Some(false), false),
        ("room-off", vec![("f1", false)], None, false),
        ("room-other", vec![("f2", false)], Some(true), false),
        ("room-other-speaks", vec![("f2", true)], Some(true), false),
    ] {
        let chats = vec![vec![text("Okay."), Ok(ChatEvent::Done)]];
        let got = one_line_with(
            what,
            "The floral one, I think.",
            chats,
            in_view,
            BOOT,
            true,
            true,
            None,
            going_on,
            false,
        )
        .await;
        if carried {
            assert_eq!(got.lease, Some(a.clone()), "{what}: A's conversation");
            assert!(got.held, "{what}: held, never memory's");
        } else {
            assert_eq!(got.lease, None, "{what}: the room's");
            assert!(!got.held, "{what}");
        }
    }
}
