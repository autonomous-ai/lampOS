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
    /// Every chat round, as asked.
    rounds: Vec<Vec<Message>>,
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
    one_line_saving(
        what, words, chats, in_view, boot, seen, eyes_up, early, going_on, named, false,
    )
    .await
}

/// [`one_line_with`] with `saves` as `LAMP_CONVERSATION_CARRIED_SAVES`
/// ([`Engine::saving_carried`]).
#[allow(clippy::too_many_arguments)]
async fn one_line_saving(
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
    saves: bool,
) -> Outcome {
    lines_saving(
        what,
        vec![(words.to_owned(), in_view, 0)],
        chats,
        boot,
        seen,
        eyes_up,
        early,
        going_on,
        named,
        saves,
        Voice::Off,
    )
    .await
}

/// `LAMP_PERSON_SCOPE_ONE_FACE` for [`lines_saving`]: off, or on with the
/// rule's record of another voice in the conversation.
#[derive(Clone, Copy, PartialEq)]
enum Voice {
    /// The switch off.
    Off,
    /// On, no other voice heard.
    One,
    /// On, another voice heard earlier in the conversation.
    AnotherBefore,
    /// On, another voice heard as this line was judged.
    AnotherInIt,
    /// On, the same-voice check not running: the rule judges the line, but
    /// nothing ever records another voice.
    Unchecked,
}

/// One line said: its words, the faces in the eyes' looks while it is said
/// ([`feed_faces`]), and the seconds since the line before.
type Line = (String, Vec<(&'static str, bool)>, u64);

/// [`one_line_saving`] over `lines`, said one after another, requests 1,
/// 2, … With `saves` the eyes also judge a held call's yes
/// ([`Engine::confirming_forgets_by_sight`]), as on the lamp.
#[allow(clippy::too_many_arguments)]
async fn lines_saving(
    what: &str,
    lines: Vec<Line>,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
    going_on: Option<bool>,
    named: bool,
    saves: bool,
    voice: Voice,
) -> Outcome {
    lines_past_silent(
        what, lines, chats, boot, seen, eyes_up, early, going_on, named, saves, None, voice,
    )
    .await
}

/// [`lines_saving`] with `past_silent` as `LAMP_CONVERSATION_CARRIES_PAST_SILENT`
/// ([`Engine::carrying_past_silent_where_heard`], the rule keeping its sign):
/// whether the same-voice check runs, and whether the conversation has shown
/// another voice, asked each time; none, the switch off.
#[allow(clippy::too_many_arguments)]
async fn lines_past_silent(
    what: &str,
    lines: Vec<Line>,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    boot: &'static str,
    seen: bool,
    eyes_up: bool,
    early: Option<&str>,
    going_on: Option<bool>,
    named: bool,
    saves: bool,
    past_silent: Option<(bool, crate::second_provider::AnotherVoice)>,
    voice: Voice,
) -> Outcome {
    let eyes = Arc::new(SpeakingFaces::default());
    let path = known_file(what, &eyes);
    let script = Arc::new(Script {
        heard: Mutex::new(lines.iter().map(|(words, ..)| Ok(words.clone())).collect()),
        chats: Mutex::new(chats.into()),
        early: Mutex::new(
            early
                .map(|early| vec![net::Early::Words(early.to_owned())])
                .into_iter()
                .collect(),
        ),
        ..Script::default()
    });
    let (engine, _, now) = scripted(script.clone());
    // `LAMP_SECOND_PREPARED`, with early words to prepare on.
    let engine = engine.preparing(early.is_some());
    let noted: Arc<Mutex<Vec<Noted>>> = Arc::default();
    let seen_noted = noted.clone();
    let cache = Arc::new(FaceCache::default());
    let calls: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let leases: Arc<Mutex<Vec<ScopeLease>>> = Arc::default();
    let (seen_calls, seen_leases) = (calls.clone(), leases.clone());
    let runner: ScopedToolRunner = Arc::new(move |_, _, name: String, _, lease: ScopeLease| {
        // Whose scope the call runs in, as the memory's route picks it: a
        // `remember` on a one-face line is its person's
        // (`ScopeLease::kept_by_one_face`), all else the lease's scope.
        seen_calls.lock().unwrap().push(
            (name == crate::remember::REMEMBER)
                .then(|| lease.kept_by_one_face())
                .flatten()
                .or_else(|| lease.scope().unwrap().person().map(str::to_owned)),
        );
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
    let engine = if voice == Voice::Off {
        engine
    } else {
        let another = Arc::new(AtomicBool::new(voice == Voice::AnotherBefore));
        let heard = another.clone();
        let engine = engine.scoping_by_one_face(another);
        if voice == Voice::AnotherInIt {
            // The rule's judging of this line hears another voice.
            engine.heeding(Arc::new(move |_, _, _, _, _| {
                heard.store(true, Ordering::Release);
                None
            }))
        } else if voice == Voice::Unchecked {
            // The rule judges this line with no voice print to hear by.
            engine.heeding(Arc::new(|_, _, _, _, _| None))
        } else {
            engine
        }
    };
    let a = crate::remember::person_id("A");
    let engine = engine.carrying_conversations_by(going_on.map(
        |going_on| -> crate::second_provider::GoingOnWith {
            Arc::new(move |_| going_on.then(|| a.clone()))
        },
    ));
    let mut engine = engine.saving_carried(saves);
    if let Some((hears, another)) = past_silent {
        engine = engine.carrying_past_silent_where_heard(true, hears, another);
    }
    if saves {
        engine = engine.confirming_forgets_by_sight(eyes.clone(), path.clone());
    }
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
    let mut labels = Vec::new();
    for (at, (_, in_view, after)) in lines.into_iter().enumerate() {
        let request = at as u64 + 1;
        // The line before is over; the last stays current, for its lease.
        if request > 1 {
            engine.retire_through(request - 1);
        }
        *now.lock().unwrap() += Duration::from_secs(after);
        let done = Arc::new(AtomicBool::new(false));
        let feeder = feed_faces(eyes.clone(), in_view, boot, done.clone());
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        tokio::time::sleep(Duration::from_millis(40)).await;
        engine.submit(end(request));
        labels.extend(turn(&mut engine).await);
        done.store(true, Ordering::Release);
        feeder.join().unwrap();
    }
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
    let rounds = script.asked.lock().unwrap().clone();
    let asked: Vec<Message> = rounds.iter().map(|chat| chat[0].clone()).collect();
    Outcome {
        calls: calls.lock().unwrap().clone(),
        lease,
        told: asked[0].clone(),
        noted: noted.lock().unwrap().clone(),
        asked,
        labels,
        held,
        rounds,
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

/// One line of A's conversation going on, the faces `in_view`, with
/// `LAMP_CONVERSATION_CARRIES_PAST_SILENT` and `another` as the
/// conversation's sign of another voice, the same-voice check running.
async fn past_silent_line(
    what: &str,
    in_view: Vec<(&'static str, bool)>,
    named: bool,
    another: crate::second_provider::AnotherVoice,
) -> Outcome {
    past_silent_line_hearing(what, in_view, named, true, another).await
}

/// [`past_silent_line`] with `hears` as whether the same-voice check runs.
async fn past_silent_line_hearing(
    what: &str,
    in_view: Vec<(&'static str, bool)>,
    named: bool,
    hears: bool,
    another: crate::second_provider::AnotherVoice,
) -> Outcome {
    lines_past_silent(
        what,
        vec![("The floral one, I think.".to_owned(), in_view, 0)],
        vec![vec![text("Floral, then."), Ok(ChatEvent::Done)]],
        BOOT,
        true,
        true,
        None,
        Some(true),
        named,
        false,
        Some((hears, another)),
        Voice::Off,
    )
    .await
}

/// #4339, lamp-3e83 on 30 Oct (`LAMP_CONVERSATION_CARRIES_PAST_SILENT`): the
/// known face in all 30 looks of a line and none with it alone, so a line no
/// face was seen speaking went to the room's. A silent second face doesn't
/// count: carried as A's conversation, held, whether admitted as A's or the
/// room's. A second face seen speaking it, another voice in the
/// conversation, or no face of A's in view from the start, and it is the
/// room's, as before.
#[tokio::test]
async fn a_silent_second_face_does_not_keep_their_conversations_line_the_rooms() {
    let a = crate::remember::person_id("A");
    let quiet: crate::second_provider::AnotherVoice = Arc::new(|| false);
    let voice: crate::second_provider::AnotherVoice = Arc::new(|| true);
    for (what, in_view, named, another, carried) in [
        (
            "silent-guest",
            vec![("f1", false), ("f2", false)],
            true,
            &quiet,
            true,
        ),
        (
            "silent-guests",
            vec![("f1", false), ("f2", false), ("f3", false)],
            true,
            &quiet,
            true,
        ),
        (
            "silent-guest-room",
            vec![("f1", false), ("f2", false)],
            false,
            &quiet,
            true,
        ),
        ("alone", vec![("f1", false)], true, &quiet, true),
        (
            "guest-speaks",
            vec![("f1", false), ("f2", true)],
            true,
            &quiet,
            false,
        ),
        (
            "guest-speaks-room",
            vec![("f1", false), ("f2", true)],
            false,
            &quiet,
            false,
        ),
        (
            "another-voice",
            vec![("f1", false), ("f2", false)],
            true,
            &voice,
            false,
        ),
        (
            "another-voice-alone",
            vec![("f1", false)],
            true,
            &voice,
            false,
        ),
        (
            "another-voice-room",
            vec![("f1", false)],
            false,
            &voice,
            false,
        ),
        ("guest-only", vec![("f2", false)], true, &quiet, false),
        ("empty-from-start", vec![], true, &quiet, false),
    ] {
        let got = past_silent_line(what, in_view, named, another.clone()).await;
        if carried {
            assert_eq!(got.lease, Some(a.clone()), "{what}: A's conversation");
            assert!(got.held, "{what}: held, never memory's");
        } else {
            assert_eq!(got.lease, None, "{what}: the room's");
            assert!(!got.held, "{what}");
            assert_eq!(got.told, Message::system("for None"), "{what}");
        }
    }
}

/// #4339 (`LAMP_CONVERSATION_CARRIES_PAST_SILENT`): a line carried past a
/// silent face is judged again once the rule has heard its voice; another
/// voice shown by then, and it is the room's before anything is told.
#[tokio::test]
async fn a_line_carried_past_a_silent_face_is_the_rooms_once_another_voice_shows() {
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = asked.clone();
    // No other voice as the line is heard; one once the rule has judged it.
    let another: crate::second_provider::AnotherVoice =
        Arc::new(move || counted.fetch_add(1, Ordering::AcqRel) > 0);
    let got = past_silent_line(
        "late-voice",
        vec![("f1", false), ("f2", false)],
        true,
        another,
    )
    .await;
    assert!(
        asked.load(Ordering::Acquire) >= 2,
        "asked again after the rule"
    );
    assert_eq!(got.lease, None, "the room's");
    assert_eq!(got.told, Message::system("for None"));
}

/// The Reviewer-lead's P2 on #4610: with the switch on but the same-voice
/// check not running (no printer started, a standby beside Live, none of
/// its switches on), nothing could show another voice, so a silent second
/// face still keeps the line the room's, as before the switch; alone, the
/// line is carried as before. The rule keeping no sign is the same.
#[tokio::test]
async fn without_the_same_voice_check_a_silent_second_face_still_blocks_the_carry() {
    let a = crate::remember::person_id("A");
    let quiet: crate::second_provider::AnotherVoice = Arc::new(|| false);
    let got = past_silent_line_hearing(
        "silent-guest-unheard",
        vec![("f1", false), ("f2", false)],
        true,
        false,
        quiet.clone(),
    )
    .await;
    assert_eq!(got.lease, None, "the room's");
    assert!(!got.held);
    assert_eq!(got.told, Message::system("for None"));
    let got =
        past_silent_line_hearing("alone-unheard", vec![("f1", false)], true, false, quiet).await;
    assert_eq!(got.lease, Some(a), "carried as before the switch");
    assert!(got.held);
    assert!(crate::second_provider::past_silent_inactive(true, false).is_some());
    assert!(crate::second_provider::past_silent_inactive(false, true).is_some());
    assert!(crate::second_provider::past_silent_inactive(false, false).is_some());
    assert!(crate::second_provider::past_silent_inactive(true, true).is_none());
}

/// The Reviewer-lead's P3 on #4610: a line the rule skipped (held, empty,
/// its own echo) is judged again on its own result, not on the sign the
/// line before left: unknown, so not carried, and the stale sign never
/// asked. A line the rule judged reads the sign; no rule, the sign alone.
#[test]
fn a_line_the_rule_skipped_is_not_carried_on_the_line_befores_sign() {
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = asked.clone();
    let quiet: crate::second_provider::AnotherVoice = Arc::new(move || {
        counted.fetch_add(1, Ordering::AcqRel);
        false
    });
    let voice: crate::second_provider::AnotherVoice = Arc::new(|| true);
    let undone = crate::second_provider::carried_past_silent_undone;
    assert!(undone(true, false, &quiet).is_some(), "skipped: the room's");
    assert_eq!(
        asked.load(Ordering::Acquire),
        0,
        "the stale sign never read"
    );
    assert!(
        undone(true, true, &quiet).is_none(),
        "judged, no voice: carried"
    );
    assert!(
        undone(true, true, &voice).is_some(),
        "judged, another voice"
    );
    assert!(
        undone(false, false, &quiet).is_none(),
        "no rule: the sign alone"
    );
    assert!(undone(false, false, &voice).is_some());
}

/// #4339 with the new switch off: a silent second face keeps the line the
/// room's, as #4479 shipped it.
#[tokio::test]
async fn without_past_silent_a_silent_second_face_keeps_the_line_the_rooms() {
    let got = one_line_with(
        "silent-guest-off",
        "The floral one, I think.",
        vec![vec![text("Okay."), Ok(ChatEvent::Done)]],
        vec![("f1", false), ("f2", false)],
        BOOT,
        true,
        true,
        None,
        Some(true),
        true,
    )
    .await;
    assert_eq!(got.lease, None, "the room's");
    assert!(!got.held);
}

/// A `remember` of "We booked the Pine Hill Hotel" and its answer.
fn keep_the_hotel() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "k1",
                crate::remember::REMEMBER,
                json!({"text": "They booked the Pine Hill Hotel."}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Got it."), Ok(ChatEvent::Done)],
    ]
}

const THE_HOTEL: &str = "We booked the Pine Hill Hotel.";

/// #4339, approved 30 Oct (`LAMP_CONVERSATION_CARRIED_SAVES`), with the
/// first builder's call on #4508: A's line no face was seen speaking, in
/// A's conversation going on, keeps nothing at once, its own line or one
/// admitted as the room's alike, the scope still held. With the switch its
/// keep is held for A's yes, and the model is told it is not kept yet and
/// to ask; off, it runs for no one, as before.
#[tokio::test]
async fn a_carried_lines_keep_is_held_never_written_at_once() {
    let a = crate::remember::person_id("A");
    for (what, named, saves) in [
        ("save-own", true, true),
        ("save-room", false, true),
        ("save-own-off", true, false),
        ("save-room-off", false, false),
    ] {
        let got = one_line_saving(
            what,
            THE_HOTEL,
            keep_the_hotel(),
            vec![("f1", false)],
            BOOT,
            true,
            true,
            None,
            Some(true),
            named,
            saves,
        )
        .await;
        assert_eq!(got.calls, [], "{what}: nothing written");
        assert_eq!(got.lease, Some(a.clone()), "{what}: A's conversation");
        assert!(got.held, "{what}: still held, no reads");
        assert!(
            got.noted.is_empty(),
            "{what}: no note read: {:?}",
            got.noted
        );
        let told = if saves {
            crate::remember::held_keep_told().to_string()
        } else {
            json!({ "error": HELD_MEMORY_CALL }).to_string()
        };
        assert!(
            got.rounds[1].contains(&Message::tool("k1".to_owned(), told)),
            "{what}: {:?}",
            got.rounds[1]
        );
    }
}

/// A held keep's case: its name, its lines, whether the lamp asks, and
/// the scopes the keep is written in.
type HeldCase = (&'static str, Vec<Line>, bool, Vec<Option<String>>);

/// A carried `remember` and the lamp asking about it.
fn keep_the_hotel_asking() -> Vec<Vec<net::Result<ChatEvent>>> {
    let mut chats = keep_the_hotel();
    chats[1] = vec![text("Want me to keep that for you?"), Ok(ChatEvent::Done)];
    chats
}

/// The first builder's call on #4508, with Reviewer-three's 14:46: a
/// carried line's held keep is written in A's scope only on A's plain yes,
/// their face seen saying it, and only after the lamp asked. A's face in
/// view and silent through the line, then through a yes (a guest's off
/// camera), keeps nothing; so does a yes after the conversation ended
/// (past `HELD_FORGET_FOR`), another face seen saying it, a forget between,
/// or a reply that never asked. Never another scope.
#[tokio::test]
async fn a_held_carried_keep_is_written_only_on_their_seen_yes() {
    let a = crate::remember::person_id("A");
    let quiet = || vec![("f1", false)];
    let yes = |in_view, after| ("Yes.".to_owned(), in_view, after);
    let hotel = || (THE_HOTEL.to_owned(), vec![("f1", false)], 0);
    let okay = || vec![text("Okay."), Ok(ChatEvent::Done)];
    let cases: Vec<HeldCase> = vec![
        (
            "held-seen-yes",
            vec![hotel(), yes(vec![("f1", true)], 5)],
            true,
            vec![Some(a.clone())],
        ),
        (
            "held-quiet-yes",
            vec![hotel(), yes(quiet(), 5)],
            true,
            vec![],
        ),
        (
            "held-too-late",
            vec![hotel(), yes(vec![("f1", true)], 61)],
            true,
            vec![],
        ),
        (
            "held-other-speaks",
            vec![hotel(), yes(vec![("f1", false), ("f2", true)], 5)],
            true,
            vec![],
        ),
        (
            "held-forget-between",
            vec![
                hotel(),
                ("Forget it.".to_owned(), vec![("f1", true)], 5),
                yes(vec![("f1", true)], 5),
            ],
            true,
            vec![],
        ),
        (
            "held-never-asked",
            vec![hotel(), yes(vec![("f1", true)], 5)],
            false,
            vec![],
        ),
    ];
    for (what, lines, asks, expected) in cases {
        let mut chats = if asks {
            keep_the_hotel_asking()
        } else {
            keep_the_hotel()
        };
        for _ in 1..lines.len() {
            chats.push(okay());
        }
        let got = lines_saving(
            what,
            lines,
            chats,
            BOOT,
            true,
            true,
            None,
            Some(true),
            true,
            true,
            Voice::Off,
        )
        .await;
        assert_eq!(got.calls, expected, "{what}");
    }
}

/// #4339 with `LAMP_CONVERSATION_CARRIED_SAVES`: a carried line still
/// cannot forget, one thing or everything; only `remember` runs.
#[tokio::test]
async fn a_carried_line_that_saves_still_cannot_forget() {
    let forget_one = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "f1",
                crate::remember::FORGET,
                json!({"text": "the hotel"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Done."), Ok(ChatEvent::Done)],
    ];
    for (what, words, chats) in [
        ("saves-forget-all", FORGET_ME, forget_everything()),
        ("saves-forget-one", "Forget the hotel.", forget_one),
    ] {
        let got = one_line_saving(
            what,
            words,
            chats,
            vec![("f1", false)],
            BOOT,
            true,
            true,
            None,
            Some(true),
            true,
            true,
        )
        .await;
        assert!(
            got.calls.is_empty(),
            "{what}: no forget runs: {:?}",
            got.calls
        );
        assert!(got.held, "{what}");
    }
}

/// #4339 with `LAMP_CONVERSATION_CARRIED_SAVES`: a line not carried keeps
/// as before, never in A's scope: another face in view, another face seen
/// speaking it, no conversation going on, or no face of A's in view through
/// it or within `CARRY_UNSEEN_WITHIN` before it (an empty view from the
/// start), and it is the room's.
#[tokio::test]
async fn a_line_not_carried_never_saves_into_their_scope() {
    for (what, in_view, going_on) in [
        ("saves-empty-from-start", vec![], Some(true)),
        ("saves-other-face", vec![("f2", false)], Some(true)),
        (
            "saves-two-faces",
            vec![("f1", false), ("f2", false)],
            Some(true),
        ),
        (
            "saves-other-speaks",
            vec![("f1", false), ("f2", true)],
            Some(true),
        ),
        ("saves-quiet", vec![("f1", false)], Some(false)),
    ] {
        let got = one_line_saving(
            what,
            THE_HOTEL,
            keep_the_hotel(),
            in_view,
            BOOT,
            true,
            true,
            None,
            going_on,
            true,
            true,
        )
        .await;
        assert_eq!(got.calls, [None], "{what}: the room's keep, never A's");
        assert_eq!(got.lease, None, "{what}");
        assert!(!got.held, "{what}");
    }
}

/// #4339 with `LAMP_CONVERSATION_CARRIED_SAVES` and the first builder's
/// call on #4508: a carried line is kept out of the journal whether or not
/// it may save, so nothing said off camera is journalled as the carried
/// person's; only a keep they say yes to, seen, is written. Its scope stays
/// held, so its exchange is marked held in the thread (#4479's
/// `Thread::held_for`) and never given to the summary or a talk note.
#[test]
fn a_carried_line_is_kept_out_of_the_journal_whether_or_not_it_saves() {
    use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
    // A person of this test's own: the thread is shared by the crate's tests.
    let a = crate::remember::person_id("Carried Saves A");
    for (request, saves) in [(4_339_101_u64, true), (4_339_102, false)] {
        let scope = RequestScope::new(
            RequestKey {
                worker: lamp_turn_owner::BootId::new([43; 16]).unwrap(),
                session: 1,
                request,
            },
            ScopeSnapshot::room(),
        );
        let lease = scope.lease();
        assert!(lease.carry_conversation(&a));
        if saves {
            assert!(lease.save_carried());
        }
        assert_eq!(lease.saves_carried(), saves.then_some(a.as_str()));
        super::super::worker::keep_admitted(
            &ProviderOutput::HoldingAnswer {
                request,
                session: 1,
            },
            &lease,
        );
        assert!(crate::remember::kept_out(request), "{request}");
        assert!(lease.scope().unwrap().held(), "{request}: still held");
        super::super::worker::keep_admitted(
            &ProviderOutput::Transcript {
                request: Some(request),
                text: "Got it, the Pine Hill Hotel.".to_owned(),
                finished: true,
            },
            &lease,
        );
        let mut thread = crate::thread::THREAD.lock().unwrap();
        let later = Instant::now() + Duration::from_secs(1);
        let given = thread.summary_rest(Some(&a), later);
        assert!(
            given.iter().all(|(given, ..)| *given != request),
            "{request}: never in a talk note: {given:?}"
        );
    }
}

/// A chat that keeps what was said, then answers.
fn remember_tokyo() -> Vec<Vec<net::Result<ChatEvent>>> {
    vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "m1",
                crate::remember::REMEMBER,
                json!({"text": "Flying to Tokyo next week."}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![text("Noted. Have a good trip."), Ok(ChatEvent::Done)],
    ]
}

async fn one_face_line(
    what: &str,
    words: &str,
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    in_view: Vec<(&'static str, bool)>,
    going_on: Option<bool>,
    voice: Voice,
) -> Outcome {
    lines_saving(
        what,
        vec![(words.to_owned(), in_view, 0)],
        chats,
        BOOT,
        true,
        true,
        None,
        going_on,
        true,
        false,
        voice,
    )
    .await
}

/// The launch testers' item 1 on #3997 (`LAMP_PERSON_SCOPE_ONE_FACE`), as
/// on lamp-220c: A's known face the one face in every look, never read
/// speaking, and no other voice. Narrowed to option 2 of reviewer-three's
/// P1 on #4584: A's keep lands in A's scope, but the line is told and read
/// as the room's (no A note, context or thread). In A's conversation going
/// on, the carry wins, as without the rule: held, never memory's.
#[tokio::test]
async fn one_known_face_and_one_voice_keep_their_keep_but_read_the_rooms_unseen_speaking() {
    let a = crate::remember::person_id("A");
    let got = one_face_line(
        "one-face",
        TOKYO,
        remember_tokyo(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert_eq!(got.lease, None, "told and read as the room's");
    assert!(!got.held);
    assert_eq!(got.calls, [Some(a.clone())], "the keep lands in A's scope");
    assert_eq!(got.told, Message::system("for None"), "no A context");
    assert!(
        got.noted.iter().all(|(person, _)| person.is_none()),
        "no A note: {:?}",
        got.noted
    );
    let got = one_face_line(
        "one-face-carried",
        TOKYO,
        vec![vec![text("Okay."), Ok(ChatEvent::Done)]],
        vec![("f1", false)],
        Some(true),
        Voice::One,
    )
    .await;
    assert_eq!(got.lease, Some(a));
    assert!(got.held, "carried, as without the rule");
}

/// The first builder's option 1 on #3997 (22:12): the rule does not wait on
/// the same-voice check. Without it (no printer, or a standby beside Live)
/// no voice is ever heard as another, and a one-face line's keep still lands
/// in A's scope while the line is told and read as the room's.
#[tokio::test]
async fn without_the_same_voice_check_a_one_face_keep_is_theirs_and_its_reads_the_rooms() {
    let got = one_face_line(
        "one-face-unchecked",
        TOKYO,
        remember_tokyo(),
        vec![("f1", false)],
        None,
        Voice::Unchecked,
    )
    .await;
    assert_eq!(
        got.calls,
        [Some(crate::remember::person_id("A"))],
        "the keep lands in A's scope"
    );
    assert_eq!(got.lease, None, "told and read as the room's");
    assert!(!got.held);
    assert_eq!(got.told, Message::system("for None"), "no A context");
    assert!(
        got.noted.iter().all(|(person, _)| person.is_none()),
        "no A note: {:?}",
        got.noted
    );
}

/// Reviewer-three's P1 on #4584: A sits silent, the one face in view; a
/// guest off camera begins the conversation (the first voice, so no other
/// voice is heard) and asks about A's memory. Nothing of A's is read or
/// told: the recall runs in the room's scope, the turn is told the room's
/// context and no A note; with the switch off, the same.
#[tokio::test]
async fn a_guest_off_camera_asking_about_a_silent_one_face_reads_nothing_of_theirs() {
    for voice in [Voice::One, Voice::Off] {
        let got = one_face_line(
            "guest-asks",
            "Lamp, what did she say about her doctor?",
            vec![
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "r1",
                        crate::remember::RECALL,
                        json!({"query": "doctor"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("I don't have anything on that."), Ok(ChatEvent::Done)],
            ],
            vec![("f1", false)],
            None,
            voice,
        )
        .await;
        let what = if voice == Voice::One { "on" } else { "off" };
        assert_eq!(got.calls, [None], "{what}: the recall is the room's");
        assert_eq!(got.lease, None, "{what}");
        assert_eq!(
            got.told,
            Message::system("for None"),
            "{what}: no A context"
        );
        assert!(
            got.noted.iter().all(|(person, _)| person.is_none()),
            "{what}: no A note: {:?}",
            got.noted
        );
    }
}

/// A second face, a face the eyes don't know as A's, no face at all, another
/// voice before it or in it, or the switch off: as today, the room's (or
/// carried, in A's conversation going on, with no other face).
#[tokio::test]
async fn a_second_face_an_unknown_face_no_face_or_another_voice_is_as_before() {
    for (what, in_view, voice) in [
        (
            "second-face",
            vec![("f1", false), ("f2", false)],
            Voice::One,
        ),
        ("unknown-face", vec![("f2", false)], Voice::One),
        ("no-face", vec![], Voice::One),
        ("voice-before", vec![("f1", false)], Voice::AnotherBefore),
        ("voice-in-it", vec![("f1", false)], Voice::AnotherInIt),
        ("off", vec![("f1", false)], Voice::Off),
    ] {
        let got = one_face_line(what, TOKYO, remember_tokyo(), in_view, None, voice).await;
        assert_eq!(got.lease, None, "{what}: the room's");
        assert!(!got.held, "{what}");
        assert_eq!(got.calls, [None], "{what}: the keep is the room's");
        // The chat is asked only after the line is judged, so another
        // voice heard as it was judged is told the room's too.
        assert_eq!(got.told, Message::system("for None"), "{what}");
    }
    // Another voice, in A's conversation going on: carried, as before.
    let a = crate::remember::person_id("A");
    let got = one_face_line(
        "voice-carried",
        TOKYO,
        vec![vec![text("Okay."), Ok(ChatEvent::Done)]],
        vec![("f1", false)],
        Some(true),
        Voice::AnotherBefore,
    )
    .await;
    assert_eq!(got.lease, Some(a));
    assert!(got.held, "carried, never memory's");
}

/// "Forget everything about me" on a line whose keep is theirs by one face
/// alone never runs in their scope: the line is the room's, as without the
/// rule, and a yes is never one either
/// ([`a_yes_on_a_line_theirs_by_one_face_alone_runs_no_held_forget`]).
#[tokio::test]
async fn forget_everything_on_a_one_face_line_never_runs_in_their_scope() {
    let a = crate::remember::person_id("A");
    let got = one_face_line(
        "one-face-forget",
        FORGET_ME,
        forget_everything(),
        vec![("f1", false)],
        None,
        Voice::One,
    )
    .await;
    assert!(
        !got.calls.contains(&Some(a)),
        "never run in A's scope: {:?}",
        got.calls
    );
    assert_eq!(got.lease, None, "the room's");
}

/// A held forget's yes on a line theirs only by their face alone in view
/// (`ScopeLease::by_one_face`) runs nothing, whatever else the yes passes:
/// a forget is never relaxed by `LAMP_PERSON_SCOPE_ONE_FACE`. The same yes
/// on a line not so taken runs it.
#[tokio::test]
async fn a_yes_on_a_line_theirs_by_one_face_alone_runs_no_held_forget() {
    let script = Arc::new(Script::default());
    let (engine, _, _) = scripted(script);
    let asker = (7, crate::remember::person_id("A"));
    for (by_one_face, runs) in [(true, false), (false, true)] {
        let now = Instant::now();
        if let Ok(mut hold) = engine.shared.held_forget.lock() {
            hold.held = Some(HeldForget {
                call: call("f1", crate::remember::FORGET_EVERYTHING, json!({})),
                request: 1,
                asker: asker.clone(),
                at: now,
                asked: true,
            });
        }
        let taken = take_held_forget(
            &engine.shared,
            2,
            true,
            "Yes.",
            Some(&asker),
            by_one_face,
            now,
        );
        match taken {
            Taken::Runs(_) => assert!(runs, "by one face: no yes"),
            Taken::Unseen(_) => assert!(!runs, "not by one face: runs"),
            Taken::Nothing => panic!("dropped for another reason"),
        }
    }
}
