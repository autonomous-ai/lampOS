//! A face rename the lamp asked about is held for the person's yes and run
//! by the voice with no model call (`LAMP_NAMES_RENAME_YES`, #5485): on
//! lamp-29e7, 6 Nov, every yes to "Do you want me to know you as A
//! instead?" came to nothing, and the lamp then said it would use A. Made-up
//! names: Tove is the name the face is kept under, Marisol the new one.
use super::*;
use crate::known_faces::{REMEMBER, Said};
use crate::memory_scope::FaceCache;
use crate::speaking_faces::{SpeakingFaces, known_face_of};
use std::sync::atomic::{AtomicBool, Ordering};

/// What the model does on a turn: says a line, or calls the face save
/// with these arguments and then says a line.
#[derive(Clone)]
enum Lamp {
    Says(&'static str),
    Saves(Value, &'static str),
}

/// One spoken turn: what they say, what the model does, and whose faces
/// the eyes see speaking it (`f1` is Tove's, `f2` someone else's).
type Turn = (&'static str, Lamp, &'static [&'static str]);

const ASKED: &str = "I know this face as Tove. Do you want me to know you as Marisol instead?";
const ASKS: (&str, Lamp, &[&str]) = (
    "Can you call me Marisol instead?",
    Lamp::Saves(Value::Null, ASKED),
    &["f1"],
);
const OWNER_TYPED: &str = "Owner Typed";
/// A guard of the runner's own, as the real one refuses a rename to the
/// owner's typed name.
const TYPED_REFUSED: &str = "I cannot safely replace a saved name from this request, so I have not changed anyone's saved face.";

struct Ran {
    /// What the eyes were sent, each rename that ran.
    sent: Vec<Value>,
    /// What each chat round was asked.
    asked: Vec<Vec<Message>>,
}

/// The turns, with the switch on or off. The runner answers a save as the
/// face check would: a save with no `replace` is known as Tove; a
/// `replace` is sent to the eyes only on a yes by its guards
/// (`known_faces::rename_agreed`), else refused for want of it.
async fn run(turns: &[Turn], on: bool) -> Ran {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    // Request numbers of their own: lines seen are the process's.
    let first = 19_000 + 100 * NEXT.fetch_add(1, Ordering::Relaxed);
    let mut chats = VecDeque::new();
    for (_, lamp, _) in turns {
        match lamp {
            Lamp::Says(line) => chats.push_back(vec![text(line), Ok(ChatEvent::Done)]),
            Lamp::Saves(args, line) => {
                let args = if args.is_null() {
                    json!({"name": "Marisol"})
                } else {
                    args.clone()
                };
                chats.push_back(vec![
                    Ok(ChatEvent::ToolCalls(vec![call("c1", REMEMBER, args)])),
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
                .map(|(said, _, _)| Ok((*said).to_owned()))
                .collect(),
        ),
        chats: Mutex::new(chats),
        ..Script::default()
    });
    let window = Arc::new(Mutex::new(Said::default()));
    let reading = window.clone();
    let judged = window.clone();
    let sent: Arc<Mutex<Vec<Value>>> = Arc::default();
    let eyes_sent = sent.clone();
    let runner: ScopedToolRunner = Arc::new(move |_, _, name, args, _| {
        let said = judged.lock().unwrap().clone();
        let new_name = args["name"].as_str().unwrap_or_default().to_owned();
        let told = if name != REMEMBER {
            json!({"ok": true})
        } else if args["replace"] != true {
            crate::known_faces::told_known_as("Tove")
        } else if new_name == OWNER_TYPED {
            json!({"error": TYPED_REFUSED})
        } else if crate::known_faces::rename_agreed(&new_name, &said) {
            eyes_sent
                .lock()
                .unwrap()
                .push(json!({"remember": new_name, "one_face": true, "replace": true}));
            json!({"ok": true, "remembered": new_name})
        } else {
            json!({"error": crate::known_faces::NOT_AGREED})
        };
        Box::pin(async move { told })
    });
    let path = std::env::temp_dir().join(format!("held-rename-{}-{first}", std::process::id()));
    let cache = Arc::new(FaceCache::default());
    let (engine, _, _) = scripted(script.clone());
    let eyes = Arc::new(SpeakingFaces::default());
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([12; 16]).unwrap(),
            cache.clone(),
            runner,
            None,
        )
        .announcing(false)
        .reading_said_from(Arc::new(move || reading.lock().unwrap().clone()))
        .confirming_forgets_by_sight(eyes.clone(), path.clone())
        .holding_renames_for_yes(on);
    engine.memory_camera(true, 1);
    eyes.look(
        Instant::now(),
        Some((("eyes", 0), &lamp_msgs::Faces { faces: Vec::new() })),
    );
    std::thread::sleep(Duration::from_millis(20));
    let mut frame = 0u64;
    let mut before: Vec<(String, String)> = Vec::new();
    for (at, (said, lamp, speaking)) in turns.iter().enumerate() {
        let request = first + at as u64;
        *window.lock().unwrap() = Said {
            this: (*said).to_owned(),
            before: before[before.len().saturating_sub(2)..].to_vec(),
        };
        std::fs::write(&path, "Tove").unwrap();
        std::fs::write(known_face_of(&path), "f1\n").unwrap();
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
        // The eyes while the line is said: f1 and f2 in view, those in
        // `speaking` read speaking.
        let done = Arc::new(AtomicBool::new(false));
        let feeder = {
            let (eyes, done) = (eyes.clone(), done.clone());
            let speaking: Vec<&'static str> = speaking.to_vec();
            std::thread::spawn(move || {
                while !done.load(Ordering::Acquire) {
                    let faces = lamp_msgs::Faces {
                        faces: ["f1", "f2"]
                            .map(|id| lamp_msgs::Face {
                                id: id.to_owned(),
                                xmin: 0.1,
                                ymin: 0.1,
                                xmax: 0.3,
                                ymax: 0.4,
                                confidence: 1.0,
                                yaw: None,
                                facing: None,
                                speaking: Some(lamp_msgs::Speaking {
                                    state: if speaking.contains(&id) {
                                        lamp_msgs::SpeakingState::Yes
                                    } else {
                                        lamp_msgs::SpeakingState::No
                                    },
                                    score: 0.9,
                                    from: None,
                                }),
                                follows: None,
                            })
                            .into(),
                    };
                    frame += 1;
                    eyes.look(Instant::now(), Some((("eyes", frame), &faces)));
                    std::thread::sleep(Duration::from_millis(2));
                }
                frame
            })
        };
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        tokio::time::sleep(Duration::from_millis(40)).await;
        engine.submit(end(request));
        turn(&mut engine).await;
        done.store(true, Ordering::Release);
        frame = feeder.join().unwrap();
        engine.retire_through(request);
        let lamp_said = match lamp {
            Lamp::Says(line) | Lamp::Saves(_, line) => line,
        };
        before.push(((*said).to_owned(), (*lamp_said).to_owned()));
    }
    let _ = std::fs::remove_file(known_face_of(&path));
    let _ = std::fs::remove_file(&path);
    let asked = script.asked.lock().unwrap().clone();
    let sent = sent.lock().unwrap().clone();
    Ran { sent, asked }
}

fn renamed() -> Vec<Value> {
    vec![json!({"remember": "Marisol", "one_face": true, "replace": true})]
}

/// The notes the last chat round was told.
fn last_notes(ran: &Ran) -> Vec<String> {
    ran.asked
        .last()
        .expect("asked")
        .iter()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .filter(|content| content.starts_with("(For you to know"))
        .map(str::to_owned)
        .collect()
}

/// The 29e7 yeses, replayed: the rename asked by the eyes' `known_as`
/// (request 18), or by a `replace` before the yes (request 61, when the
/// model then made no call), runs on the yes with no model call, once, and
/// the model is told it is done.
#[tokio::test]
async fn the_29e7_yeses_run_the_held_rename_with_no_call() {
    let request_18: [Turn; 2] = [
        ASKS,
        (
            "Yes. please forgot the name Tove and Marisol, and this is my faith the face of Marisol.",
            Lamp::Says("Got it."),
            &["f1"],
        ),
    ];
    let ran = run(&request_18, true).await;
    assert_eq!(ran.sent, renamed());
    assert!(
        last_notes(&ran)
            .iter()
            .any(|note| note.contains("renamed now")),
        "{:?}",
        last_notes(&ran)
    );
    let request_61: [Turn; 2] = [
        (
            "Okay. I want to change my name Can you call me Marisol instead?",
            Lamp::Saves(
                json!({"name": "Marisol", "replace": true}),
                "Marisol instead of Tove, is that right?",
            ),
            &["f1"],
        ),
        (
            "Okay. Can you repeat my name again?",
            Lamp::Says("Marisol."),
            &["f1"],
        ),
    ];
    assert_eq!(run(&request_61, true).await.sent, renamed());
}

/// The model also calls the rename after the yes: renamed exactly once.
#[tokio::test]
async fn a_model_call_after_the_held_rename_renames_once() {
    let turns: [Turn; 2] = [
        ASKS,
        (
            "Yes, please.",
            Lamp::Saves(json!({"name": "Marisol", "replace": true}), "Done."),
            &["f1"],
        ),
    ];
    assert_eq!(run(&turns, true).await.sent, renamed());
}

/// With the switch off, the yes is as today: no hold, so the voice runs
/// nothing, and no note is told.
#[tokio::test]
async fn with_the_switch_off_nothing_is_held_or_told() {
    let turns: [Turn; 2] = [
        ASKS,
        (
            "Okay. Can you repeat my name again?",
            Lamp::Says("Marisol."),
            &["f1"],
        ),
    ];
    let ran = run(&turns, false).await;
    assert_eq!(ran.sent, Vec::<Value>::new());
    assert!(last_notes(&ran).is_empty(), "{:?}", last_notes(&ran));
}

/// A yes another face is seen saying renames nothing, and the model is told
/// it did not count and nothing was renamed.
#[tokio::test]
async fn a_yes_seen_from_a_second_face_is_dropped() {
    let turns: [Turn; 2] = [ASKS, ("Yes, please.", Lamp::Says("Okay."), &["f1", "f2"])];
    let ran = run(&turns, true).await;
    assert_eq!(ran.sent, Vec::<Value>::new());
    let notes = last_notes(&ran);
    assert!(
        notes
            .iter()
            .any(|note| note.contains("did not count") && note.contains("another face")),
        "{notes:?}"
    );
}

/// The runner's own guards still apply to the held run: a rename to the
/// owner's typed name is refused there, and the model is told nothing was
/// renamed.
#[tokio::test]
async fn the_owners_typed_name_is_still_refused() {
    let turns: [Turn; 2] = [
        (
            "Can you call me Owner Typed instead?",
            Lamp::Saves(
                json!({"name": OWNER_TYPED}),
                "I know this face as Tove. Do you want me to know you as Owner Typed instead?",
            ),
            &["f1"],
        ),
        ("Yes, please.", Lamp::Says("Okay."), &["f1"]),
    ];
    let ran = run(&turns, true).await;
    assert_eq!(ran.sent, Vec::<Value>::new());
    let notes = last_notes(&ran);
    assert!(
        notes
            .iter()
            .any(|note| note.contains("was not renamed") && note.contains(TYPED_REFUSED)),
        "{notes:?}"
    );
}

/// No claim the lamp can't back (the 29e7 end: "That's what I'll use"):
/// after a no, each later turn of the session is told the face is still
/// Tove's and nothing was renamed; once the rename runs, it is no longer
/// told.
#[tokio::test]
async fn an_unrun_rename_is_never_claimed() {
    let turns: [Turn; 3] = [
        ASKS,
        ("No, not now.", Lamp::Says("Okay."), &["f1"]),
        ("What's my name, Lamp?", Lamp::Says("Tove."), &["f1"]),
    ];
    let ran = run(&turns, true).await;
    assert_eq!(ran.sent, Vec::<Value>::new());
    let notes = last_notes(&ran);
    assert!(
        notes.iter().any(|note| note.contains("still saved as Tove")
            && note.contains("nothing was renamed")
            && note.contains("Never say the name was changed")),
        "{notes:?}"
    );
    let turns: [Turn; 3] = [
        ASKS,
        ("Yes.", Lamp::Says("Got it."), &["f1"]),
        ("What's my name, Lamp?", Lamp::Says("Marisol."), &["f1"]),
    ];
    let ran = run(&turns, true).await;
    assert_eq!(ran.sent, renamed());
    assert!(
        !last_notes(&ran)
            .iter()
            .any(|note| note.contains("no rename has run")),
        "{:?}",
        last_notes(&ran)
    );
}

/// The note that nothing was renamed is the asker's alone: a turn another
/// face is seen saying is not told the old name, and the asker's own turn
/// after it still is.
#[tokio::test]
async fn an_unrun_rename_is_told_only_to_its_asker() {
    let turns: [Turn; 2] = [
        ASKS,
        (
            "What's my name, Lamp?",
            Lamp::Says("I'm not sure."),
            &["f2"],
        ),
    ];
    let ran = run(&turns, true).await;
    assert!(
        !last_notes(&ran)
            .iter()
            .any(|note| note.contains("no rename has run") || note.contains("Tove")),
        "{:?}",
        last_notes(&ran)
    );
    let turns: [Turn; 3] = [
        ASKS,
        (
            "What's my name, Lamp?",
            Lamp::Says("I'm not sure."),
            &["f2"],
        ),
        ("And what's mine?", Lamp::Says("Tove."), &["f1"]),
    ];
    let ran = run(&turns, true).await;
    assert!(
        last_notes(&ran)
            .iter()
            .any(|note| note.contains("no rename has run") && note.contains("still saved as Tove")),
        "{:?}",
        last_notes(&ran)
    );
}
