//! A forget the model calls on a spoken turn whose own words don't ask for
//! one is held in the engine, and only a plain yes from the same person
//! within 60 s runs that call (the first builder on #1722, 00:35). #2324,
//! #2326 and #2331 let a yes confirm a forget by reading words, and each was
//! fooled; these are the cases that fooled them.
use super::*;
use crate::memory_scope::FaceCache;

/// What the model does on a turn: says a line, or calls a forget and then
/// says a line.
#[derive(Clone, Copy)]
enum Lamp {
    Says(&'static str),
    Calls(&'static str, &'static str),
}

impl Lamp {
    /// The forget it calls, if any.
    fn name(&self) -> &'static str {
        match self {
            Lamp::Says(_) => "",
            Lamp::Calls(name, _) => name,
        }
    }
}

/// One spoken turn: who is known at the lamp ("" for nobody), what they
/// say, what the model does, and the seconds since the turn before. Words
/// that begin with [`WROTE`] are a turn the lamp wrote itself, not heard.
type Turn = (&'static str, &'static str, Lamp, u64);

/// Marks a turn's words as the lamp's own written turn (a skill's say).
const WROTE: &str = "(the lamp wrote) ";

/// What the engine ran and what the model was told over `turns`.
struct Ran {
    /// Each forget run, by name and arguments.
    forgets: Vec<(String, Value)>,
    /// What each chat round was asked.
    asked: Vec<Vec<Message>>,
}

async fn run(turns: &[Turn]) -> Ran {
    let mut chats = VecDeque::new();
    for (who, _, lamp, _) in turns {
        match lamp {
            Lamp::Says(line) => chats.push_back(vec![text(line), Ok(ChatEvent::Done)]),
            Lamp::Calls(name, line) => {
                chats.push_back(vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        name,
                        json!({"name": who}),
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
                .filter(|(_, said, _, _)| !said.starts_with(WROTE))
                .map(|(_, said, _, _)| Ok((*said).to_owned()))
                .collect(),
        ),
        chats: Mutex::new(chats),
        ..Script::default()
    });
    let forgets: Calls = Arc::default();
    let seen = forgets.clone();
    let runner: ScopedToolRunner = Arc::new(move |_, heard, name, args, _| {
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async { json!({"ok": true}) })
    });
    let path = std::env::temp_dir().join(format!(
        "held-forget-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let cache = Arc::new(FaceCache::default());
    let (engine, _, now) = scripted(script.clone());
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([12; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|_: &crate::memory_scope::ScopeLease| {
                Told::from("SYSTEM".to_owned())
            })),
        )
        .announcing(false);
    engine.memory_camera(true, 1);
    for (at, (who, said, _, after)) in turns.iter().enumerate() {
        let request = at as u64 + 1;
        *now.lock().unwrap() += Duration::from_secs(*after);
        let seen = if who.is_empty() {
            None
        } else {
            std::fs::write(&path, who).unwrap();
            crate::known_faces::FaceObservation::read(&path)
        };
        cache.replace(cache.begin_refresh(), seen);
        if let Some(words) = said.strip_prefix(WROTE) {
            engine.submit(written(request, words));
        } else {
            engine.submit(start(request));
            engine.submit(audio(request, 1));
            engine.submit(end(request));
        }
        turn(&mut engine).await;
        engine.retire_through(request);
    }
    let _ = std::fs::remove_file(path);
    let forgets = forgets
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _, _)| crate::remember::is_forget(name))
        .map(|(name, args, _)| (name.clone(), args.clone()))
        .collect();
    let asked = script.asked.lock().unwrap().clone();
    Ran { forgets, asked }
}

const FACE: &str = crate::known_faces::FORGET;
const EVERYTHING: &str = crate::remember::FORGET_EVERYTHING;
const ASKS: Lamp = Lamp::Calls(FACE, "Shall I forget your face?");

#[tokio::test]
async fn a_yes_with_no_held_call_forgets_nothing() {
    // The cases that fooled #2324, #2326 and #2331: the model calls a forget
    // on the yes, which only holds it.
    let cases: &[&[Turn]] = &[
        &[
            (
                "A",
                "Clear the timer.",
                Lamp::Says("Want me to set a reminder?"),
                0,
            ),
            ("A", "Yes.", ASKS, 5),
        ],
        &[
            (
                "A",
                "I keep forgetting my keys.",
                Lamp::Says("Did you forget them at work?"),
                0,
            ),
            ("A", "Yes.", ASKS, 5),
        ],
        &[
            (
                "A",
                "I keep forgetting my keys.",
                Lamp::Says("Remember everything I said about the trip?"),
                0,
            ),
            ("A", "Yes.", ASKS, 5),
        ],
        &[
            (
                "A",
                "Don't forget my keys.",
                Lamp::Says("Shall I remind you not to forget them?"),
                0,
            ),
            ("A", "Yes.", ASKS, 5),
        ],
        &[
            (
                "A",
                "Tôi hay quên chìa khoá.",
                Lamp::Says("Bạn có quên chìa khoá không?"),
                0,
            ),
            ("A", "Có.", ASKS, 5),
        ],
        &[
            (
                "A",
                "Don't forget my keys.",
                Lamp::Says("Want me to set a reminder?"),
                0,
            ),
            ("A", "OK, what's my name?", ASKS, 5),
        ],
        &[
            ("A", "Rất vui được làm quen.", Lamp::Says("Bạn tên gì?"), 0),
            ("A", "Vâng.", ASKS, 5),
        ],
    ];
    for turns in cases {
        let ran = run(turns).await;
        assert_eq!(ran.forgets, [], "{}", turns[0].1);
    }
}

#[tokio::test]
async fn a_held_forget_runs_only_on_a_plain_yes_from_them_within_a_minute() {
    let held = ("A", "What's my name?", ASKS, 0);
    let cases: &[(&[Turn], usize)] = &[
        // That person's plain yes runs exactly the call held.
        (
            &[
                held,
                ("A", "Yes.", Lamp::Says("Done, I've forgotten it."), 10),
            ],
            1,
        ),
        (&[held, ("A", "Yes.", Lamp::Says("Done."), 60)], 1),
        // Another person's yes, or nobody's known.
        (&[held, ("B", "Yes.", Lamp::Says("Okay."), 5)], 0),
        (&[held, ("", "Yes.", Lamp::Says("Okay."), 5)], 0),
        // Too late.
        (&[held, ("A", "Yes.", Lamp::Says("Okay."), 61)], 0),
        // Not a plain yes.
        (
            &[held, ("A", "OK, what's my name?", Lamp::Says("Kim."), 5)],
            0,
        ),
        (&[held, ("A", "No.", Lamp::Says("Okay."), 5)], 0),
        (&[held, ("A", "Yes?", Lamp::Says("Shall I?"), 5)], 0),
        // Something else first: the hold is dropped.
        (
            &[
                held,
                ("A", "Set a timer for five minutes.", Lamp::Says("Done."), 5),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            0,
        ),
        // A second yes after it ran runs nothing more.
        (
            &[
                held,
                ("A", "Yes.", Lamp::Says("Done."), 5),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            1,
        ),
        // Nobody known: nothing is held, so their yes runs nothing.
        (
            &[
                ("", "What's my name?", ASKS, 0),
                ("", "Yes.", Lamp::Says("Okay."), 5),
            ],
            0,
        ),
    ];
    for (turns, runs) in cases {
        let ran = run(turns).await;
        let want: Vec<(String, Value)> = (0..*runs)
            .map(|_| (FACE.to_owned(), json!({"name": "A"})))
            .collect();
        assert_eq!(
            ran.forgets,
            want,
            "{:?}",
            turns.iter().map(|turn| turn.1).collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn a_forget_asked_for_in_words_runs_at_once() {
    for (said, name) in [
        ("Forget everything about me.", EVERYTHING),
        ("xóa khuôn mặt tôi", FACE),
        ("delete my face", FACE),
    ] {
        let ran = run(&[("A", said, Lamp::Calls(name, "Done."), 0)]).await;
        assert_eq!(
            ran.forgets,
            [(name.to_owned(), json!({"name": "A"}))],
            "{said}"
        );
    }
}

/// The forget functions say what the engine does (the reviewer on #2334):
/// one called unasked is held, the model asks once, and their yes runs it,
/// with the model told it ran and not to call it again.
#[tokio::test]
async fn the_forget_descriptions_and_the_hold_agree() {
    let face = lamp_gemini::Tool {
        name: FACE.into(),
        description: "Forget their face.".into(),
        parameters: None,
        silent: false,
    };
    let everything = lamp_gemini::Tool {
        name: EVERYTHING.into(),
        description: "Ask them to say \"forget everything\"; a yes alone forgets nothing.".into(),
        parameters: None,
        silent: false,
    };
    let other = lamp_gemini::Tool {
        name: "remember".into(),
        description: "Keep it.".into(),
        parameters: None,
        silent: false,
    };
    let told = crate::remember::held_forgets_described(vec![face, everything, other]);
    assert!(
        told[0]
            .description
            .ends_with(crate::remember::HELD_DESCRIBED)
    );
    assert!(
        told[1]
            .description
            .ends_with(crate::remember::HELD_DESCRIBED)
    );
    assert!(!told[1].description.contains("a yes alone forgets nothing"));
    assert_eq!(told[2].description, "Keep it.");
    assert!(told.iter().all(|tool| tool.description.len() <= 1024));

    let ran = run(&[
        (
            "A",
            "Who am I?",
            Lamp::Calls(EVERYTHING, "Shall I forget everything about you?"),
            0,
        ),
        ("A", "Yes.", Lamp::Says("Done, it's all gone."), 10),
    ])
    .await;
    assert_eq!(ran.forgets, [(EVERYTHING.to_owned(), json!({"name": "A"}))]);
    // Round 2 of the first turn was told it is held, to ask once.
    let held = ran.asked[1].last().cloned().unwrap();
    assert_eq!(
        held,
        Message::tool(
            "c1",
            crate::remember::held_forget_told(EVERYTHING).to_string()
        )
    );
    // The yes turn was told it ran.
    let yes = &ran.asked[2];
    assert!(
        yes.iter()
            .any(|message| format!("{message:?}").contains("has run")),
        "the yes turn is told the forget ran"
    );
}

/// On the turn a held forget ran, a forget the model calls again is not
/// run nor held: it is told it ran, so the lamp doesn't ask again.
#[tokio::test]
async fn a_forget_called_again_on_the_yes_is_not_held_again() {
    let ran = run(&[
        ("A", "What's my name?", ASKS, 0),
        ("A", "Yes.", Lamp::Calls(FACE, "Done."), 5),
        ("A", "Yes.", Lamp::Says("Okay."), 5),
    ])
    .await;
    assert_eq!(ran.forgets, [(FACE.to_owned(), json!({"name": "A"}))]);
    let told = ran.asked[3].last().cloned().unwrap();
    assert_eq!(
        told,
        Message::tool(
            "c1",
            json!({"error": crate::remember::HELD_FORGET_RAN}).to_string()
        )
    );
}

/// The held turn's answer must ask them about the forget, as a question
/// that names one, said whole (Fourth's P2 on #2340): a yes to anything
/// else, to a statement, or after an answer cut short runs nothing.
#[tokio::test]
async fn a_held_forget_stands_only_on_a_question_about_it() {
    let yes = ("A", "Yes.", Lamp::Says("Okay."), 5);
    let cases: &[(Lamp, usize)] = &[
        // Fourth's case: the model asked something else.
        (Lamp::Calls(FACE, "Want me to set a reminder?"), 0),
        // It names the forget, but asks nothing.
        (Lamp::Calls(FACE, "I can forget your face."), 0),
        // It names the forget, and asks about something else.
        (
            Lamp::Calls(FACE, "I can forget your face. Want me to set a reminder?"),
            0,
        ),
        // The reviewer's 05:19 on #2340: a forget word, but not the face.
        (Lamp::Calls(FACE, "Shall I delete that reminder too?"), 0),
        (Lamp::Calls(FACE, "Do you want me to clear the timer?"), 0),
        // Cut: the question went out, the rest of the answer failed.
        (Lamp::Calls(FACE, "Shall I forget your face? FAIL now."), 0),
        // A question about it.
        (Lamp::Calls(FACE, "Shall I forget your face?"), 1),
        (
            Lamp::Calls(FACE, "Okay. Do you want me to forget your face?"),
            1,
        ),
        (Lamp::Calls(FACE, "Mình xóa khuôn mặt bạn nhé?"), 1),
    ];
    for (lamp, runs) in cases {
        let ran = run(&[("A", "What's my name?", *lamp, 0), yes]).await;
        let want: Vec<(String, Value)> = (0..*runs)
            .map(|_| (FACE.to_owned(), json!({"name": "A"})))
            .collect();
        let Lamp::Calls(_, line) = lamp else {
            unreachable!()
        };
        assert_eq!(ran.forgets, want, "{line}");
    }
}

/// Their own turn just before asked in words, so the call on their yes runs
/// at once, with no second question (Fourth's P3 on #2340).
#[tokio::test]
async fn a_forget_they_asked_for_just_before_runs_on_the_call() {
    let ran = run(&[
        (
            "A",
            "Forget everything about me.",
            Lamp::Says("Everything, really?"),
            0,
        ),
        (
            "A",
            "Yes.",
            Lamp::Calls(EVERYTHING, "Done, it's all gone."),
            5,
        ),
    ])
    .await;
    assert_eq!(ran.forgets, [(EVERYTHING.to_owned(), json!({"name": "A"}))]);
    // Run, never held: the model was given the forget's own result.
    let told = ran.asked[2].last().cloned().unwrap();
    assert_eq!(told, Message::tool("c1", json!({"ok": true}).to_string()));
}

/// Only the same person's own spoken words just before, in the same
/// session and within 60 s; never the lamp's words, its answer or a turn
/// it wrote.
#[tokio::test]
async fn only_their_own_ask_just_before_counts() {
    let asked = (
        "A",
        "Forget everything about me.",
        Lamp::Says("Everything, really?"),
        0,
    );
    let calls = Lamp::Calls(EVERYTHING, "Okay.");
    let cases: &[(&[Turn], usize)] = &[
        (&[asked, ("A", "Yes.", calls, 60)], 1),
        // Too late, another person, nobody known.
        (&[asked, ("A", "Yes.", calls, 61)], 0),
        (&[asked, ("B", "Yes.", calls, 5)], 0),
        (&[asked, ("", "Yes.", calls, 5)], 0),
        (
            &[
                ("", "Forget everything about me.", Lamp::Says("Really?"), 0),
                ("", "Yes.", calls, 5),
            ],
            0,
        ),
        // Not the turn just before.
        (
            &[
                asked,
                ("A", "What time is it?", Lamp::Says("Ten."), 5),
                ("A", "Yes.", calls, 5),
            ],
            0,
        ),
        // The lamp's own answer names it: their words didn't ask.
        (
            &[
                (
                    "A",
                    "Who am I?",
                    Lamp::Says("Shall I forget everything about you?"),
                    0,
                ),
                ("A", "Yes.", calls, 5),
            ],
            0,
        ),
        // A turn the lamp wrote asks it: not theirs.
        (
            &[
                (
                    "A",
                    "(the lamp wrote) Forget everything about me.",
                    Lamp::Says("Okay."),
                    0,
                ),
                ("A", "Yes.", calls, 5),
            ],
            0,
        ),
        // A turn the lamp wrote between their ask and their yes.
        (
            &[
                asked,
                (
                    "A",
                    "(the lamp wrote) Your timer is done.",
                    Lamp::Says("Your timer is done."),
                    5,
                ),
                ("A", "Yes.", calls, 5),
            ],
            0,
        ),
    ];
    for (turns, runs) in cases {
        let ran = run(turns).await;
        let want: Vec<(String, Value)> = (0..*runs)
            .map(|_| (EVERYTHING.to_owned(), json!({"name": "A"})))
            .collect();
        assert_eq!(
            ran.forgets,
            want,
            "{:?}",
            turns.iter().map(|turn| turn.1).collect::<Vec<_>>()
        );
    }
}

/// Their ask carries to the next turn only for the forget it names
/// ("everything", "tất cả"; "face", "khuôn mặt"); a plain `forget` of a
/// kept thing never does, and always goes through the held question.
#[tokio::test]
async fn an_ask_just_before_carries_only_for_the_forget_it_names() {
    let then = |said: &'static str, name: &'static str| -> [Turn; 2] {
        [
            ("A", said, Lamp::Says("Really?"), 0),
            ("A", "Yes.", Lamp::Calls(name, "Okay."), 5),
        ]
    };
    let cases: &[([Turn; 2], usize)] = &[
        (then("Forget my face.", FACE), 1),
        (then("Xóa khuôn mặt của tôi đi.", FACE), 1),
        (then("Xóa tất cả về tôi.", EVERYTHING), 1),
        // Named something else.
        (then("Forget everything about me.", FACE), 0),
        (then("Forget my face.", EVERYTHING), 0),
        // A plain forget is never run early.
        (then("Forget that.", crate::remember::FORGET), 0),
        (
            then("Forget everything about me.", crate::remember::FORGET),
            0,
        ),
    ];
    for (turns, runs) in cases {
        let ran = run(turns).await;
        assert_eq!(
            ran.forgets.len(),
            *runs,
            "{} -> {}",
            turns[0].1,
            turns[1].2.name()
        );
    }
}

/// "Don't forget my keys." asks by the matcher's words but names no face:
/// a `forget_my_face` called after it is held for the question, not run.
#[tokio::test]
async fn a_face_forget_after_dont_forget_my_keys_is_held() {
    let ran = run(&[
        (
            "A",
            "Don't forget my keys.",
            Lamp::Says("Shall I remind you not to forget them?"),
            0,
        ),
        ("A", "Yes.", ASKS, 5),
    ])
    .await;
    assert_eq!(ran.forgets, []);
    let told = ran.asked[2].last().cloned().unwrap();
    assert_eq!(
        told,
        Message::tool("c1", crate::remember::held_forget_told(FACE).to_string())
    );
}

/// The question must name the forget that is held (Fourth's 05:11 P3 on
/// #2340): the face or everything by name, and a plain `forget` with the
/// lamp as the one who would forget. A question with only a forget word in
/// it, or about something else, drops it.
#[tokio::test]
async fn a_held_forget_stands_only_on_a_question_that_names_it() {
    const KEPT: &str = crate::remember::FORGET;
    let yes = ("A", "Yes.", Lamp::Says("Okay."), 5);
    let cases: &[(Lamp, usize)] = &[
        (Lamp::Calls(FACE, "Did you forget your keys?"), 0),
        (Lamp::Calls(FACE, "Should I clear the timer?"), 0),
        (Lamp::Calls(FACE, "Want me to remove the reminder?"), 0),
        (Lamp::Calls(FACE, "Shall I forget everything about you?"), 0),
        (Lamp::Calls(EVERYTHING, "Shall I forget your face?"), 0),
        (Lamp::Calls(KEPT, "Did you forget it?"), 0),
        (Lamp::Calls(KEPT, "Did you forget your face?"), 0),
        // The real ones.
        (Lamp::Calls(FACE, "Shall I forget your face?"), 1),
        (
            Lamp::Calls(EVERYTHING, "Shall I forget everything about you?"),
            1,
        ),
        (Lamp::Calls(EVERYTHING, "Mình xóa tất cả về bạn nhé?"), 1),
        (Lamp::Calls(KEPT, "Should I forget that?"), 1),
        (
            Lamp::Calls(KEPT, "Do you want me to delete what you told me?"),
            1,
        ),
        (Lamp::Calls(KEPT, "Tôi có nên quên điều đó không?"), 1),
    ];
    for (lamp, runs) in cases {
        let Lamp::Calls(name, line) = lamp else {
            unreachable!()
        };
        let ran = run(&[("A", "What's my name?", *lamp, 0), yes]).await;
        let want: Vec<(String, Value)> = (0..*runs)
            .map(|_| ((*name).to_owned(), json!({"name": "A"})))
            .collect();
        assert_eq!(ran.forgets, want, "{name}: {line}");
    }
}
