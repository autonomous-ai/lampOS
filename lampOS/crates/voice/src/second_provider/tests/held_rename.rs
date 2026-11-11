//! A face rename the lamp asked about is held for the person's yes and run
//! by the voice with no model call (`LAMP_NAMES_RENAME_YES`, #5485): on
//! lamp-29e7, 6 Nov, every yes to "Do you want me to know you as A
//! instead?" came to nothing, and the lamp then said it would use A. Made-up
//! names: Tove is the name the face is kept under, Marisol the new one.
use super::*;
use crate::known_faces::{REMEMBER, Said};
use crate::memory_scope::FaceCache;
use crate::speaking_faces::{SpeakingFaces, known_face_of};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// What the model does on a turn: says a line, or calls the face save
/// with these arguments and then says a line.
#[derive(Clone)]
enum Lamp {
    Says(&'static str),
    Saves(Value, &'static str),
    /// Calls this tool with these arguments, then says a line.
    Calls(&'static str, Value, &'static str),
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
    /// How many memory forgets the runner finished.
    forgot: usize,
}

/// How the turn at this index is cut once its memory forget has finished
/// in the runner and waits there to return (Reviewer-three on #5792).
#[derive(Clone, Copy, PartialEq)]
enum Cut {
    /// Nothing cut: every turn runs through.
    None,
    /// Its scope is revoked, then the forget's answer is let go: the turn
    /// returns at the scope check after the await.
    Revoked(usize),
    /// The turn is stopped, dropping its future mid-await.
    Dropped(usize),
}

/// The turns, with the switch on or off. The runner answers a save as the
/// face check would: a save with no `replace` is known as Tove; a
/// `replace` is sent to the eyes only on a yes by its guards
/// (`known_faces::rename_agreed`), else refused for want of it.
async fn run(turns: &[Turn], on: bool) -> Ran {
    run_cut(turns, on, Cut::None).await
}

/// [`run`], with one turn cut ([`Cut`]) while its forget waits to return.
async fn run_cut(turns: &[Turn], on: bool, cut: Cut) -> Ran {
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
            Lamp::Calls(name, args, line) => {
                chats.push_back(vec![
                    Ok(ChatEvent::ToolCalls(vec![call("c1", name, args.clone())])),
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
    let forgot = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(tokio::sync::Notify::new());
    let (forgets, released) = (forgot.clone(), release.clone());
    let runner: ScopedToolRunner = Arc::new(move |_, _, name, args, _| {
        // A forget is done here; with a cut, its answer waits to be let go.
        let held = crate::remember::is_forget(&name) && cut != Cut::None;
        if crate::remember::is_forget(&name) {
            forgets.fetch_add(1, Ordering::SeqCst);
        }
        let released = released.clone();
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
        Box::pin(async move {
            if held {
                released.notified().await;
            }
            told
        })
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
        match cut {
            Cut::Revoked(index) | Cut::Dropped(index) if index == at => {
                tokio::time::timeout(Duration::from_secs(5), async {
                    while forgot.load(Ordering::SeqCst) == 0 {
                        tokio::select! {
                            _ = engine.next() => {}
                            () = tokio::time::sleep(Duration::from_millis(5)) => {}
                        }
                    }
                })
                .await
                .expect("the forget reached the runner");
                if matches!(cut, Cut::Revoked(_)) {
                    engine
                        .turn
                        .as_ref()
                        .unwrap()
                        .memory_scope
                        .as_ref()
                        .unwrap()
                        .revoke();
                    release.notify_one();
                    tokio::time::timeout(
                        Duration::from_secs(5),
                        &mut engine.turn.as_mut().unwrap().task,
                    )
                    .await
                    .expect("the revoked turn returns")
                    .unwrap();
                } else {
                    // Dropped mid-await: its answer is never let go.
                    engine.stop_through(request);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                engine.retire_through(request);
                engine.urgent.clear();
                while engine.output.try_recv().is_ok() {}
            }
            _ => {
                turn(&mut engine).await;
            }
        }
        done.store(true, Ordering::Release);
        frame = feeder.join().unwrap();
        engine.retire_through(request);
        let lamp_said = match lamp {
            Lamp::Says(line) | Lamp::Saves(_, line) | Lamp::Calls(_, _, line) => line,
        };
        before.push(((*said).to_owned(), (*lamp_said).to_owned()));
    }
    let _ = std::fs::remove_file(known_face_of(&path));
    let _ = std::fs::remove_file(&path);
    let asked = script.asked.lock().unwrap().clone();
    let sent = sent.lock().unwrap().clone();
    Ran {
        sent,
        asked,
        forgot: forgot.load(Ordering::SeqCst),
    }
}

/// Every message of a chat that names Marisol, whole as sent.
fn marisol_in(asked: &[Message]) -> Vec<String> {
    asked
        .iter()
        .map(|message| serde_json::to_string(message).unwrap())
        .filter(|message| message.contains("Marisol"))
        .collect()
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

/// The Reviewer-lead on #5586: an asker's turn that can't be proven theirs
/// (two faces in view and none seen speaking, or two seen speaking, theirs
/// one of them) is still told no rename has run, so the lamp can't say
/// "That's what I'll use." (lamp-29e7, 6 Nov), but without the old name,
/// which can't be shown to whoever is speaking.
#[tokio::test]
async fn an_unproven_askers_turn_is_told_no_rename_ran_without_the_old_name() {
    for (line, speaking) in [
        ("Okay. Can you repeat my name again?", &[][..]),
        ("Okay. Can you repeat my name again?", &["f1", "f2"][..]),
        ("What's my name, Lamp?", &[][..]),
    ] {
        let turns: [Turn; 2] = [ASKS, (line, Lamp::Says("Sure."), speaking)];
        let ran = run(&turns, true).await;
        assert_eq!(ran.sent, Vec::<Value>::new(), "{line} {speaking:?}");
        let notes = last_notes(&ran);
        assert!(
            notes.iter().any(|note| note.contains("no rename has run")
                && note.contains("Marisol")
                && note.contains("Never say the name was changed")),
            "{line} {speaking:?}: {notes:?}"
        );
        assert!(
            !notes.iter().any(|note| note.contains("Tove")),
            "{line} {speaking:?}: {notes:?}"
        );
    }
}

/// `LAMP_NAMES_RENAME_ASKED` into `LAMP_NAMES_RENAME_YES` (#5485): a line
/// the face known as Tove says alone at the lamp, giving another name with
/// no "remember", is forced to call the face save
/// (`worker::rename_asks`); that call's `known_as` answer is held, and
/// their yes renames once, while a no renames nothing.
#[tokio::test]
async fn a_rename_the_trigger_asks_is_held_and_a_yes_renames_and_a_no_does_not() {
    const GIVES: &str = "Call me Marisol.";
    let root = std::env::temp_dir().join(format!("lvr-asked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = lamp_bus::Bus::new(&root).unwrap();
    let mut eyes = bus
        .publisher_declared(
            lamp_msgs::Faces::TOPIC,
            &lamp_msgs::Faces::declaration(),
            "eyes",
        )
        .unwrap();
    let face = lamp_msgs::Face {
        id: "f1".into(),
        xmin: 0.4,
        ymin: 0.0,
        xmax: 0.7,
        ymax: 0.3,
        confidence: 0.9,
        yaw: None,
        facing: None,
        speaking: None,
        follows: None,
    };
    eyes.publish_json(
        crate::scenario_clock::monotonic_us(),
        &lamp_msgs::Faces { faces: vec![face] },
    )
    .unwrap();
    let known = crate::known_faces::known_file_of(&root.join("who.sock"));
    std::fs::write(&known, "Tove\n").unwrap();
    let declared = crate::known_faces::tools_checked(true);
    let chat = crate::second_provider::net::Chat::deepseek(
        "deepseek-chat",
        "k",
        Arc::new(move || declared.clone()),
        false,
    )
    .unwrap();
    chat.forcing(crate::second_provider::worker::rename_asks(
        known,
        lamp_bus::Bus::new(&root).unwrap(),
    ));
    let line = [Message::system("You are a lamp."), Message::user(GIVES)];
    assert_eq!(
        chat.wire(&line).2.get("tool_choice").cloned(),
        Some(json!({"type": "function", "function": {"name": REMEMBER}}))
    );
    std::fs::remove_dir_all(root).unwrap();
    // The forced call, run through the held rename.
    let asks: Turn = (GIVES, Lamp::Saves(Value::Null, ASKED), &["f1"]);
    let yes: [Turn; 2] = [asks.clone(), ("Yes, please.", Lamp::Says("Okay."), &["f1"])];
    assert_eq!(run(&yes, true).await.sent, renamed());
    let no: [Turn; 2] = [
        asks,
        ("No, keep Tove.", Lamp::Says("Okay, Tove it is."), &["f1"]),
    ];
    assert_eq!(run(&no, true).await.sent, Vec::<Value>::new());
}

/// The rename asked, dropped by an unrelated turn, and the model's forget
/// of the proposed name held and asked about; their yes runs it, and the
/// session's next turn asks their name.
fn held_forget() -> [Turn; 4] {
    [
        ASKS,
        (
            "I no longer want that new name.",
            Lamp::Calls(
                crate::remember::FORGET,
                json!({"about": "Marisol"}),
                "Shall I forget that proposed name?",
            ),
            &["f1"],
        ),
        ("Yes.", Lamp::Says("Forgotten."), &["f1"]),
        ("What's my name, Lamp?", Lamp::Says("Tove."), &["f1"]),
    ]
}

/// The rename asked, then a turn whose own words ask to forget the
/// proposed name: the model's forget runs on that call, and the session's
/// next turn asks their name.
fn called_forget() -> [Turn; 3] {
    [
        ASKS,
        (
            "Please forget the name Marisol.",
            Lamp::Calls(
                crate::remember::FORGET,
                json!({"about": "Marisol"}),
                "Forgotten.",
            ),
            &["f1"],
        ),
        ("What's my name, Lamp?", Lamp::Says("Tove."), &["f1"]),
    ]
}

/// The rename asked, then "Forget my face.": the model's
/// `forget_my_face` runs, and the session's next turn says hello (Jarvis on
/// #5792).
fn face_forget() -> [Turn; 3] {
    [
        ASKS,
        (
            "Forget my face.",
            Lamp::Calls(crate::known_faces::FORGET, json!({}), "Done."),
            &["f1"],
        ),
        ("Hello, Lamp.", Lamp::Says("Hello."), &["f1"]),
    ]
}

/// The session's next turn, after the forget: its whole prompt never names
/// the proposal.
fn next_never_told(ran: &Ran, why: &str) {
    assert_eq!(ran.sent, Vec::<Value>::new(), "{why}");
    assert_eq!(ran.forgot, 1, "{why}: the forget ran once");
    let next = ran.asked.last().expect("the next turn was asked");
    assert!(
        next.iter().any(|message| {
            matches!(
                message.content.as_ref().and_then(Value::as_str),
                Some("What's my name, Lamp?" | "Hello, Lamp.")
            )
        }),
        "{why}: the last chat is the next turn's: {next:?}"
    );
    assert_eq!(marisol_in(next), Vec::<String>::new(), "{why}, next turn");
}

/// Reviewer-three on #5586: the proposed name a held memory forget clears
/// is never told again. Neither that yes's chat (started clean at the
/// forget) nor the session's next turn names the proposal, in any part of
/// its prompt.
#[tokio::test]
async fn a_forgotten_proposed_name_is_never_told_again() {
    for on in [false, true] {
        let ran = run(&held_forget(), on).await;
        // The yes's chat: started clean at the forget that ran.
        let yes_chat = ran
            .asked
            .iter()
            .find(|asked| {
                asked
                    .iter()
                    .any(|message| message.content.as_ref().and_then(Value::as_str) == Some("Yes."))
            })
            .expect("the yes was asked");
        assert!(
            yes_chat.iter().any(|message| {
                message
                    .content
                    .as_ref()
                    .and_then(Value::as_str)
                    .is_some_and(|content| content.starts_with("A memory forget call has run"))
            }),
            "on {on}: the held forget did not run: {yes_chat:?}"
        );
        assert_eq!(
            marisol_in(yes_chat),
            Vec::<String>::new(),
            "on {on}, yes turn"
        );
        next_never_told(&ran, &format!("on {on}"));
        let ran = run(&called_forget(), on).await;
        next_never_told(&ran, &format!("on {on}, called"));
    }
}

/// Jarvis on #5792: "Forget my face." after the rename was asked ends it
/// too; the next turn is never told the proposal.
#[tokio::test]
async fn a_forgotten_face_ends_the_asked_rename() {
    for on in [false, true] {
        let ran = run(&face_forget(), on).await;
        next_never_told(&ran, &format!("on {on}, forget_my_face"));
    }
}

/// Reviewer-three on #5792: a forget admitted and finished in the runner,
/// whose turn is then revoked before its answer returns, or dropped while
/// it waits, still ends the asked rename: the session's next turn is never
/// told the proposal. Their yes to a held forget, and the model's own call.
#[tokio::test]
async fn a_forget_cut_at_its_return_still_ends_the_asked_rename() {
    for on in [false, true] {
        for (turns, at, how) in [
            (held_forget().to_vec(), 2, "held"),
            (called_forget().to_vec(), 1, "called"),
            (face_forget().to_vec(), 1, "forget_my_face"),
        ] {
            let ran = run_cut(&turns, on, Cut::Revoked(at)).await;
            next_never_told(&ran, &format!("on {on}, {how}, revoked at its return"));
            let ran = run_cut(&turns, on, Cut::Dropped(at)).await;
            next_never_told(&ran, &format!("on {on}, {how}, dropped mid-await"));
        }
    }
}

/// Jarvis's remaining P3 on #5792, owned by the face-context boundary:
/// the already-composed note is discarded on this turn, not only the next.
#[tokio::test]
async fn face_forget_context_drops_a_pending_rename_from_the_same_turn() {
    for on in [true, false] {
        let turns = face_forget();
        let ran = run(&turns[..2], on).await;
        assert_eq!(ran.forgot, 1, "on {on}");
        assert_eq!(ran.sent, Vec::<Value>::new(), "on {on}");
        assert_eq!(ran.asked.len(), 4, "on {on}");
        if on {
            assert!(
                !marisol_in(&ran.asked[2]).is_empty(),
                "the initial face-forget request had the cached proposal"
            );
        }
        let next = ran.asked.last().unwrap();
        assert_eq!(marisol_in(next), Vec::<String>::new(), "on {on}, same turn");
        assert_eq!(next.last(), Some(&Message::user("Forget my face.")));
        assert!(!serde_json::to_string(next).unwrap().contains("Tove"));
        assert!(
            serde_json::to_string(next)
                .unwrap()
                .contains("A face forget call has run")
        );
    }
}
