//! The first face save at setup, refused only for want of the read-back's
//! yes, is held by the voice and run on that yes with no model call
//! (lamp-52e6, 5 Nov, #5391's check: "My name is Walter.", "Walter. Have I
//! got that right?", "Yes, that's my name." made no call, and the walk
//! ended with the name only).
use super::*;
use crate::known_faces::{REMEMBER, Said};
use crate::memory_scope::FaceCache;

/// What the model does on a turn: says a line, or calls the face save
/// under "Walter" and then says a line.
#[derive(Clone, Copy)]
enum Lamp {
    Says(&'static str),
    Saves(&'static str),
}

const READ_BACK: &str = "Walter. Have I got that right?";
/// Setup's face question, which the model asks after the name's yes.
const FACE_QUESTION: &str = "May I remember your face, so I know you next time?";
const NAME_LINE: (&str, Lamp) = ("My name is Walter.", Lamp::Saves(READ_BACK));

/// The eyes requests the voice sent: each save that ran, by its name.
async fn run(turns: &[(&'static str, Lamp)], two_faces_at: Option<usize>) -> Vec<Value> {
    run_with(turns, two_faces_at, None).await.0
}

/// As [`run`], with onboarding's "who you are" open (`true`) or past
/// (`false`), as the walk's file would say.
async fn run_at_setup(
    turns: &[(&'static str, Lamp)],
    two_faces_at: Option<usize>,
    open: bool,
) -> Vec<Value> {
    run_full(turns, two_faces_at, None, Some(open), false)
        .await
        .0
}

/// As [`run`], with the eyes refusing the save on the yes with `refused`,
/// and what each chat was asked.
async fn run_with(
    turns: &[(&'static str, Lamp)],
    two_faces_at: Option<usize>,
    refused: Option<&'static str>,
) -> (Vec<Value>, Vec<Vec<Message>>) {
    run_full(turns, two_faces_at, refused, None, false).await
}

/// As [`run_with`], with "who you are" open or not as `setup` says, else
/// as the environment's walk file says (none here), and with the eyes'
/// speaking read on (`LAMP_EYES_SPEAKING`) when `sight`.
async fn run_full(
    turns: &[(&'static str, Lamp)],
    two_faces_at: Option<usize>,
    refused: Option<&'static str>,
    setup: Option<bool>,
    sight: bool,
) -> (Vec<Value>, Vec<Vec<Message>>) {
    let (sent, asked, _) = run_timed(turns, two_faces_at, refused, setup, sight, None).await;
    (sent, asked)
}

/// As [`run_full`], with the eyes taking `slow` over each save and each
/// chat's first event coming a second late, and what the voice was asked
/// to say.
async fn run_timed(
    turns: &[(&'static str, Lamp)],
    two_faces_at: Option<usize>,
    refused: Option<&'static str>,
    setup: Option<bool>,
    sight: bool,
    slow: Option<Duration>,
) -> (Vec<Value>, Vec<Vec<Message>>, Vec<String>) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // Request numbers of their own: lines seen are the process's.
    let first = 9_000 + 100 * NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut chats = VecDeque::new();
    for (_, lamp) in turns {
        match lamp {
            Lamp::Says(line) => chats.push_back(vec![text(line), Ok(ChatEvent::Done)]),
            Lamp::Saves(line) => {
                chats.push_back(vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        REMEMBER,
                        json!({"name": "Walter"}),
                    )])),
                    Ok(ChatEvent::Done),
                ]);
                chats.push_back(vec![text(line), Ok(ChatEvent::Done)]);
            }
        }
    }
    let script = Arc::new(Script {
        heard: Mutex::new(
            turns
                .iter()
                .map(|(said, _)| Ok((*said).to_owned()))
                .collect(),
        ),
        chats: Mutex::new(chats),
        first_event_after: slow.map(|_| Duration::from_secs(1)),
        ..Script::default()
    });
    // What was said, as the thread's window gives it: the turn in hand and
    // the exchanges before it.
    let window = Arc::new(Mutex::new(Said::default()));
    let reading = window.clone();
    let sent: Arc<Mutex<Vec<Value>>> = Arc::default();
    let eyes = sent.clone();
    let judged = window.clone();
    // The eyes' check as a setup save makes it: kept only on a plain yes
    // to the read-back, else refused for it.
    let runner: ScopedToolRunner = Arc::new(move |_, _, name, args, _| {
        let said = judged.lock().unwrap().clone();
        let told = if name != REMEMBER {
            json!({"ok": true})
        } else if crate::known_faces::held_save_verdict(
            args["name"].as_str().unwrap_or_default(),
            &said,
        ) == crate::known_faces::HeldSaveVerdict::Runs
        {
            if let Some(why) = refused {
                return Box::pin(async move { json!({"error": why}) });
            }
            eyes.lock()
                .unwrap()
                .push(json!({"remember": args["name"], "one_face": true, "replace": false}));
            let told = json!({"ok": true, "remembered": args["name"]});
            if let Some(slow) = slow {
                return Box::pin(async move {
                    tokio::time::sleep(slow).await;
                    told
                });
            }
            told
        } else {
            json!({"error": crate::known_faces::SAY_IT_BACK})
        };
        Box::pin(async move { told })
    });
    let cache = Arc::new(FaceCache::default());
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([12; 16]).unwrap(),
            cache.clone(),
            runner,
            None,
        )
        .announcing(false)
        .reading_said_from(Arc::new(move || reading.lock().unwrap().clone()));
    if let Some(open) = setup {
        engine = engine.reading_setup_from(Arc::new(move || open));
    }
    if sight {
        let path = std::env::temp_dir().join(format!("held-save-{}-{first}", std::process::id()));
        engine = engine.confirming_forgets_by_sight(
            Arc::new(crate::speaking_faces::SpeakingFaces::default()),
            path,
        );
    }
    engine.memory_camera(true, 1);
    let mut before: Vec<(String, String)> = Vec::new();
    for (at, (said, lamp)) in turns.iter().enumerate() {
        let request = first + at as u64;
        *window.lock().unwrap() = Said {
            this: (*said).to_owned(),
            before: before[before.len().saturating_sub(2)..].to_vec(),
        };
        if two_faces_at == Some(at) {
            crate::speaking_faces::saw_line(crate::speaking_faces::LineSeen {
                request,
                speakers: vec!["f1".into(), "f2".into()],
                ..Default::default()
            });
        }
        cache.replace(cache.begin_refresh(), None);
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        engine.submit(end(request));
        turn_within(&mut engine, Duration::from_secs(10)).await;
        engine.retire_through(request);
        let lamp_said = match lamp {
            Lamp::Says(line) | Lamp::Saves(line) => line,
        };
        before.push(((*said).to_owned(), (*lamp_said).to_owned()));
    }
    let asked = script.asked.lock().unwrap().clone();
    let spoken = script.spoken.lock().unwrap().clone();
    (sent.lock().unwrap().clone(), asked, spoken)
}

fn kept() -> Vec<Value> {
    vec![json!({"remember": "Walter", "one_face": true, "replace": false})]
}

/// The notes for the model in the chat a turn asked.
fn notes(chat: &[Message]) -> Vec<&str> {
    chat.iter()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .filter(|content| content.starts_with("(For you to know"))
        .collect()
}

/// Whether a chat's notes have the model ask setup's face question.
fn asks_the_face(chat: &[Message]) -> bool {
    notes(chat).iter().any(|note| {
        note.contains("they said yes to their name, Walter") && note.contains(FACE_QUESTION)
    })
}

/// (a) and (b): the trace's order, the yes to the face with no model call
/// keeps the face once, the eyes asked as a model call would ask them. A
/// yes to the name alone keeps the save held and has the face question
/// asked first (the first builder's HOLD on #5700); a yes to the name that
/// asks for the face itself is the yes to it.
#[tokio::test]
async fn the_yes_to_the_face_keeps_the_held_face_with_no_call() {
    let (sent, asked) = run_with(
        &[
            NAME_LINE,
            ("Yes, that's my name.", Lamp::Says(FACE_QUESTION)),
        ],
        None,
        None,
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
    assert!(asks_the_face(asked.last().unwrap()), "{asked:?}");
    let sent = run(
        &[
            NAME_LINE,
            ("Yes, that's my name.", Lamp::Says(FACE_QUESTION)),
            ("Yes.", Lamp::Says("Got it.")),
        ],
        None,
    )
    .await;
    assert_eq!(sent, kept());
    let sent = run(
        &[
            NAME_LINE,
            (
                "Yes. That's right. Please remember my face.",
                Lamp::Says("Got it."),
            ),
        ],
        None,
    )
    .await;
    assert_eq!(sent, kept());
}

/// The held save's wait at the eyes (3.5 s here; 1.5-3.3 s on lamp-52e6)
/// does not count against round 1's start bound: the yes to the face is
/// answered, never with the down line (lamp-52e6, request 9).
#[tokio::test(start_paused = true)]
async fn a_slow_held_save_still_has_the_yes_answered() {
    let (sent, _, spoken) = run_timed(
        &[
            NAME_LINE,
            ("Yes, that's my name.", Lamp::Says(FACE_QUESTION)),
            ("Yes, please.", Lamp::Says("Got it.")),
        ],
        None,
        None,
        None,
        false,
        Some(Duration::from_millis(3_500)),
    )
    .await;
    assert_eq!(sent, kept());
    assert!(!spoken.iter().any(|line| line == DOWN_LINE), "{spoken:?}");
    assert!(spoken.iter().any(|line| line == "Got it."), "{spoken:?}");
}

/// A no, or anything but a plain yes, to the face question asked for the
/// held save drops it: nothing kept.
#[tokio::test]
async fn a_no_to_the_face_question_drops_the_held_save() {
    for answer in ["No, thanks.", "Not now.", "Why do you need my face?"] {
        let sent = run(
            &[
                NAME_LINE,
                ("Yes, that's my name.", Lamp::Says(FACE_QUESTION)),
                (answer, Lamp::Says("Okay.")),
                ("Yes.", Lamp::Says("Got it.")),
            ],
            None,
        )
        .await;
        assert_eq!(sent, Vec::<Value>::new(), "{answer}");
    }
}

/// Fourteen's HOLD on #5720: a plain yes to a face question the model
/// turned round keeps nothing, held or with no call at setup.
#[tokio::test]
async fn a_yes_to_a_face_question_turned_round_keeps_nothing() {
    for face in [
        "Would you rather I didn't remember your face?",
        "I won't remember your face unless you want me to. Is that okay?",
        "May I remember just your name, without your face?",
        "M\u{ec}nh kh\u{f4}ng nh\u{1edb} khu\u{f4}n m\u{1eb7}t b\u{1ea1}n nh\u{e9}?",
    ] {
        let sent = run(
            &[
                NAME_LINE,
                ("Yes, that's my name.", Lamp::Says(face)),
                ("Yes.", Lamp::Says("Got it.")),
            ],
            None,
        )
        .await;
        assert_eq!(sent, Vec::<Value>::new(), "held: {face}");
        let sent = run_at_setup(
            &[
                NO_CALL_NAME_LINE,
                ("Yes. That's my name.", Lamp::Says(face)),
                ("Yes.", Lamp::Says("Okay.")),
            ],
            None,
            true,
        )
        .await;
        assert_eq!(sent, Vec::<Value>::new(), "at setup: {face}");
    }
}

/// (c), (e) and a second face: a correction, a read-back folded into the
/// face question, or a second face seen speaking the yes keeps nothing,
/// and the hold is gone after it.
#[tokio::test]
async fn a_correction_a_face_question_or_a_second_face_keeps_nothing() {
    let sent = run(
        &[
            NAME_LINE,
            ("No, it's Walt.", Lamp::Says("Walt, right?")),
            ("Yes, that's my name.", Lamp::Says("Got it.")),
        ],
        None,
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
    let sent = run(
        &[
            (
                "My name is Walter.",
                Lamp::Saves("Nice to meet you, Walter! May I remember your face?"),
            ),
            ("Yes.", Lamp::Says("Got it.")),
        ],
        None,
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
    let sent = run(
        &[NAME_LINE, ("Yes, that's my name.", Lamp::Says("Got it."))],
        Some(1),
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
}

/// The lead's call of 9 Nov: with the eyes' speaking read on, the yes to
/// the face keeps it though no face was seen saying it (the owner may not
/// face the lamp), held or with no call at setup. Only a second face seen
/// speaking the yes refuses, and the model is told why.
#[tokio::test]
async fn with_the_speaking_read_an_unseen_yes_keeps_the_face() {
    let held = [
        NAME_LINE,
        ("Yes, that's my name.", Lamp::Says(FACE_QUESTION)),
        ("Yes.", Lamp::Says("Got it.")),
    ];
    let (sent, _) = run_full(&held, None, None, None, true).await;
    assert_eq!(sent, kept(), "held");
    let at_setup = [
        NO_CALL_NAME_LINE,
        ("Yes. That's my name.", Lamp::Says(FACE_QUESTION)),
        ("Yes.", Lamp::Says("Thanks, Walter.")),
    ];
    let (sent, _) = run_full(&at_setup, None, None, Some(true), true).await;
    assert_eq!(sent, kept(), "at setup");
    for (turns, setup) in [(&held, None), (&at_setup, Some(true))] {
        let (sent, asked) = run_full(turns, Some(2), None, setup, true).await;
        assert_eq!(sent, Vec::<Value>::new(), "two faces: {setup:?}");
        assert!(
            notes(asked.last().unwrap())
                .iter()
                .any(|note| note.contains("a second face seen at the yes")),
            "{asked:?}"
        );
    }
}

/// (d): the model also calls the save after the yes: kept exactly once.
#[tokio::test]
async fn a_model_call_after_the_held_save_keeps_it_once() {
    let sent = run(
        &[NAME_LINE, ("Yes, that's my name.", Lamp::Saves("Got it."))],
        None,
    )
    .await;
    assert_eq!(sent, kept());
}

/// The eyes keep nothing on the yes (no one face in view): the model is
/// told the face was not kept, and why, so it does not say it will know
/// them.
#[tokio::test]
async fn a_held_save_that_keeps_nothing_is_told_to_the_model() {
    let (sent, asked) = run_with(
        &[
            NAME_LINE,
            ("Yes, that's my name.", Lamp::Says(FACE_QUESTION)),
            ("Yes.", Lamp::Says("Got it.")),
        ],
        None,
        Some("no one face in view"),
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
    let last = asked.last().expect("the yes was asked of the model");
    let notes = notes(last);
    assert!(
        notes
            .iter()
            .any(|note| note.contains("could not be kept") && note.contains("no one face in view")),
        "{notes:?}"
    );
}

/// A plain yes whose held save is dropped all the same (here a second
/// face seen at the yes):
/// the model is told why, so it makes up no reason of its own (Fifth's
/// run B on #5415, lamp-52e6, 5 Nov).
#[tokio::test]
async fn a_dropped_held_save_after_a_plain_yes_is_told_to_the_model() {
    let (sent, asked) = run_with(
        &[NAME_LINE, ("Yes, that's my name.", Lamp::Says("Got it."))],
        Some(1),
        None,
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
    let last = asked.last().expect("the yes was asked of the model");
    assert!(
        last.iter()
            .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
            .any(|note| note.starts_with("(For you to know")
                && note.contains("could not be kept")
                && note.contains("a second face seen at the yes")),
        "{last:?}"
    );
}

/// #5684 (the Walter trace): the model said the name back with no call
/// at all, and made none on the yes either ("Now I'll take a quick look"
/// and `calls=none`), so the face wait closed "who you are" with the name
/// only. While the step is open, the voice has the model ask the face
/// question on that yes, and runs the save itself on the yes to that: the
/// face is kept once, and never on the name's yes alone (the first
/// builder's HOLD on #5700).
const NO_CALL_NAME_LINE: (&str, Lamp) = ("Name is Walter.", Lamp::Says(READ_BACK));
const NO_CALL_YES: (&str, Lamp) = (
    "Yes. That's my name.",
    Lamp::Says("Oh, good — Walter it is, then. Now I'll take a quick look so I know your face."),
);

/// (i) The yes to a read-back the model made with no call keeps no face:
/// the model is told to ask the face question. The yes to that keeps the
/// face, with the name said back at its start or its end.
#[tokio::test]
async fn at_setup_a_yes_to_the_read_back_with_no_call_asks_the_face_question() {
    let (sent, asked) = run_full(
        &[NO_CALL_NAME_LINE, NO_CALL_YES],
        None,
        None,
        Some(true),
        false,
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
    assert!(asks_the_face(asked.last().unwrap()), "{asked:?}");
    for read_back in [
        READ_BACK,
        "Walter, did I get that right?",
        "Did I get that right, Walter?",
    ] {
        let sent = run_at_setup(
            &[
                ("Name is Walter.", Lamp::Says(read_back)),
                ("Yes. That's my name.", Lamp::Says(FACE_QUESTION)),
                ("Yes.", Lamp::Says("Thanks, Walter.")),
            ],
            None,
            true,
        )
        .await;
        assert_eq!(sent, kept(), "{read_back}");
    }
}

/// (i') A no, or anything but a plain yes, to the face question keeps
/// nothing; nor does a yes to it with no yes to the name before it.
#[tokio::test]
async fn at_setup_no_yes_to_the_face_question_keeps_nothing_with_no_call() {
    for answer in ["No.", "No, don't.", "Not now.", "What will you do with it?"] {
        let sent = run_at_setup(
            &[
                NO_CALL_NAME_LINE,
                ("Yes. That's my name.", Lamp::Says(FACE_QUESTION)),
                (answer, Lamp::Says("Okay.")),
            ],
            None,
            true,
        )
        .await;
        assert_eq!(sent, Vec::<Value>::new(), "{answer}");
    }
    let sent = run_at_setup(
        &[
            NO_CALL_NAME_LINE,
            ("No, it's Walt.", Lamp::Says(FACE_QUESTION)),
            ("Yes.", Lamp::Says("Okay.")),
        ],
        None,
        true,
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
}

/// (ii) A correction, a question, the face question with the name folded
/// in, or a second face seen at the yes: nothing kept.
#[tokio::test]
async fn at_setup_no_yes_to_the_read_back_keeps_nothing_with_no_call() {
    for answer in [
        ("No, it's Walt.", Lamp::Says("Walt, right?")),
        (
            "Why do you need my name?",
            Lamp::Says("So I can greet you."),
        ),
    ] {
        let sent = run_at_setup(&[NO_CALL_NAME_LINE, answer], None, true).await;
        assert_eq!(sent, Vec::<Value>::new(), "{}", answer.0);
    }
    let sent = run_at_setup(
        &[
            (
                "Name is Walter.",
                Lamp::Says("Nice to meet you, Walter! May I remember your face?"),
            ),
            ("Yes.", Lamp::Says("Got it.")),
        ],
        None,
        true,
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
    let sent = run_at_setup(&[NO_CALL_NAME_LINE, NO_CALL_YES], Some(1), true).await;
    assert_eq!(sent, Vec::<Value>::new());
}

/// (iii) The model calls the save after all, on the yes or a turn later:
/// kept exactly once.
#[tokio::test]
async fn at_setup_a_model_call_after_the_yes_keeps_it_once() {
    let sent = run_at_setup(
        &[
            NO_CALL_NAME_LINE,
            ("Yes. That's my name.", Lamp::Saves("Got it.")),
        ],
        None,
        true,
    )
    .await;
    assert_eq!(sent, kept());
    let sent = run_at_setup(
        &[
            NO_CALL_NAME_LINE,
            NO_CALL_YES,
            ("Okay.", Lamp::Saves("All set.")),
        ],
        None,
        true,
    )
    .await;
    assert_eq!(sent, kept());
}

/// (iv) Past "who you are" (or with no walk), a yes to a name said back
/// runs nothing the model did not call.
#[tokio::test]
async fn past_setup_a_yes_to_the_read_back_with_no_call_keeps_nothing() {
    let (sent, asked) = run_full(
        &[NO_CALL_NAME_LINE, NO_CALL_YES],
        None,
        None,
        Some(false),
        false,
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
    assert!(!asks_the_face(asked.last().unwrap()), "{asked:?}");
    let sent = run_at_setup(
        &[
            NO_CALL_NAME_LINE,
            ("Yes. That's my name.", Lamp::Says(FACE_QUESTION)),
            ("Yes.", Lamp::Says("Okay.")),
        ],
        None,
        false,
    )
    .await;
    assert_eq!(sent, Vec::<Value>::new());
}
