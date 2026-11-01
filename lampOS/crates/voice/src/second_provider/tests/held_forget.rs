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
    /// For each tool call, its request and what the eyes saw saying that
    /// request's line, read as it ran ([`crate::speaking_faces::line_seen`]).
    lines: Vec<(u64, Option<crate::speaking_faces::LineSeen>)>,
    /// For each forget run, by name, whether its request was marked as
    /// asking for everything when it ran
    /// ([`crate::memory_scope::ScopeLease::everything_marked`], #3564).
    marked: Vec<(String, bool)>,
}

async fn run(turns: &[Turn]) -> Ran {
    run_seen(turns, None).await
}

/// The face the eyes give each person at the lamp, beside the known file.
fn face_of(who: &str) -> &'static str {
    match who {
        "A" => "f1",
        "B" => "f2",
        _ => "f9",
    }
}

/// One turn's eyes: whom they see speaking, and whether the known face is
/// beside the known file.
type Sight = (&'static [&'static str], bool);

/// [`run`], with the eyes reading speaking (`LAMP_EYES_SPEAKING=on`,
/// #2377) when `sight` is given: for each turn, whom the eyes see speaking
/// while it is said, A (`f1`) and B (`f2`) both in view, and whether the
/// known person's face is beside the known file (a missing reading when
/// not).
async fn run_seen(turns: &[Turn], sight: Option<&[Sight]>) -> Ran {
    run_seen_from(turns, sight, 1).await
}

/// [`run_seen`], its first turn request `first`: lines seen are kept for
/// the whole process, so a test that reads them takes numbers of its own.
async fn run_seen_from(turns: &[Turn], sight: Option<&[Sight]>, first: u64) -> Ran {
    run_seen_from_result(turns, sight, first, json!({"ok": true})).await
}

async fn run_seen_from_result(
    turns: &[Turn],
    sight: Option<&[Sight]>,
    first: u64,
    result: Value,
) -> Ran {
    use crate::speaking_faces::{SpeakingFaces, known_face_of};
    use std::sync::atomic::{AtomicBool, Ordering};
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
    let lines = Arc::new(Mutex::new(Vec::new()));
    let lines_seen = lines.clone();
    let marked = Arc::new(Mutex::new(Vec::new()));
    let marks = marked.clone();
    let runner: ScopedToolRunner = Arc::new(move |request, heard, name, args, scope| {
        if crate::remember::is_forget(&name) {
            marks
                .lock()
                .unwrap()
                .push((name.clone(), scope.everything_marked()));
        }
        seen.lock().unwrap().push((name, args, heard));
        lines_seen
            .lock()
            .unwrap()
            .push((request, crate::speaking_faces::line_seen(request)));
        let result = result.clone();
        Box::pin(async move { result })
    });
    let path = std::env::temp_dir().join(format!(
        "held-forget-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let cache = Arc::new(FaceCache::default());
    let (engine, _, now) = scripted(script.clone());
    let eyes = Arc::new(SpeakingFaces::default());
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([12; 16]).unwrap(),
            cache.clone(),
            runner,
            Some(Arc::new(|_: &crate::memory_scope::ScopeLease| Told {
                text: "SYSTEM with old private context".into(),
                after: Some("old private context".into()),
                history: vec![Message::assistant("old private context")],
            })),
        )
        .announcing(false);
    if sight.is_some() {
        engine = engine.confirming_forgets_by_sight(eyes.clone(), path.clone());
    }
    engine.memory_camera(true, 1);
    // The eyes publishing before the first turn: a known face's file is
    // tied to the boot whose frames were read before it was written.
    eyes.look(
        Instant::now(),
        Some((("eyes", 0), &lamp_msgs::Faces { faces: Vec::new() })),
    );
    // Strictly after it, beyond a coarse file clock's tick.
    std::thread::sleep(Duration::from_millis(20));
    let mut frame = 0u64;
    for (at, (who, said, _, after)) in turns.iter().enumerate() {
        let request = at as u64 + first;
        *now.lock().unwrap() += Duration::from_secs(*after);
        let (speaking, face_beside) = sight.map_or((&[][..], false), |sight| sight[at]);
        let _ = std::fs::remove_file(known_face_of(&path));
        let seen = if who.is_empty() {
            None
        } else {
            std::fs::write(&path, who).unwrap();
            if face_beside {
                std::fs::write(known_face_of(&path), format!("{}\n", face_of(who))).unwrap();
            }
            crate::known_faces::FaceObservation::read(&path)
        };
        cache.replace(cache.begin_refresh(), seen);
        if let Some(words) = said.strip_prefix(WROTE) {
            engine.submit(written(request, words));
        } else {
            // The eyes, while the line is said: A and B in view, those in
            // `speaking` read speaking.
            let done = Arc::new(AtomicBool::new(false));
            let feeder = {
                let (eyes, done) = (eyes.clone(), done.clone());
                let speaking: Vec<&'static str> = speaking.iter().map(|who| face_of(who)).collect();
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
            if sight.is_some() {
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
            engine.submit(end(request));
            turn(&mut engine).await;
            done.store(true, Ordering::Release);
            frame = feeder.join().unwrap();
            engine.retire_through(request);
            continue;
        }
        turn(&mut engine).await;
        engine.retire_through(request);
    }
    let _ = std::fs::remove_file(known_face_of(&path));
    let _ = std::fs::remove_file(path);
    let forgets = forgets
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _, _)| crate::remember::is_forget(name))
        .map(|(name, args, _)| (name.clone(), args.clone()))
        .collect();
    let asked = script.asked.lock().unwrap().clone();
    let lines = lines.lock().unwrap().clone();
    let marked = marked.lock().unwrap().clone();
    Ran {
        forgets,
        asked,
        lines,
        marked,
    }
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
    // Not `forget_everything`, which always asks first (#3504, N3).
    for (said, name) in [("xóa khuôn mặt tôi", FACE), ("delete my face", FACE)] {
        let ran = run(&[("A", said, Lamp::Calls(name, "Done."), 0)]).await;
        assert_eq!(
            ran.forgets,
            [(name.to_owned(), json!({"name": "A"}))],
            "{said}"
        );
    }
}

/// lamp-06c1, 26 Oct: a `forget_everything` called for words that ask to
/// forget one thing ("forget that"), or say not to forget everything, is
/// refused, never run and never held for the yes that follows; also after
/// a follow-up the lamp offered (Reviewer-two's composed negative on
/// #3564). Each case's positive stays: the explicit ask runs at once.
#[tokio::test]
async fn a_bare_forget_never_runs_or_holds_forget_everything() {
    let follow_up = (
        "A",
        "How did the graduation go?",
        Lamp::Says("Shall I ask you about it on Saturday?"),
        0,
    );
    for said in [
        "Forget that.",
        "Delete that one.",
        "Quên chuyện đó đi.",
        "Don't forget everything, just that.",
        "Đừng xóa tất cả, chỉ cái đó.",
    ] {
        for before in [None, Some(follow_up)] {
            let mut turns: Vec<Turn> = before.into_iter().collect();
            turns.push(("A", said, Lamp::Calls(EVERYTHING, "Which thing?"), 5));
            turns.push(("A", "Yes.", Lamp::Says("Okay."), 5));
            let ran = run(&turns).await;
            assert!(ran.forgets.is_empty(), "{said}");
            assert!(
                ran.asked
                    .iter()
                    .any(|messages| messages.iter().any(|message| {
                        format!("{message:?}").contains("not everything about them")
                    })),
                "{said}"
            );
        }
    }
    // The explicit ask is held and asked about (#3504, N3); their yes runs it.
    let asks = Lamp::Calls(EVERYTHING, "Shall I forget everything about you?");
    for said in ["Forget everything about me.", "Xóa tất cả về tôi."] {
        let ran = run(&[follow_up, ("A", said, asks, 5)]).await;
        assert!(ran.forgets.is_empty(), "{said}");
        let ran = run(&[
            follow_up,
            ("A", said, asks, 5),
            ("A", "Yes.", Lamp::Says("Done."), 5),
        ])
        .await;
        assert_eq!(
            ran.forgets,
            [(EVERYTHING.to_owned(), json!({"name": "A"}))],
            "{said}"
        );
    }
}

/// #3564 on #3608's guard: a `forget_everything` the guard runs marks its
/// request as asking for everything, and only then is the face forgotten
/// too (`remember::forget_everything_face`): their yes to one held, whether
/// their words asked for it or not (#3504, N3: a spoken `forget_everything`
/// is always held first), never an ask just before. A forget of one thing,
/// a face forget, and a later request never carry the mark; a bare "forget
/// that" never runs `forget_everything` at all (Reviewer-two's P2s, 01:12
/// and 02:20).
#[tokio::test]
async fn only_an_ask_the_guard_showed_marks_everything_for_the_face() {
    // A forget run, by name, and whether its request was marked.
    type Marked = (&'static str, bool);
    let held = (
        "A",
        "Who am I?",
        Lamp::Calls(EVERYTHING, "Shall I forget everything about you?"),
        0,
    );
    let marked = |runs: &[Marked]| -> Vec<(String, bool)> {
        runs.iter()
            .map(|(name, marked)| ((*name).to_owned(), *marked))
            .collect()
    };
    let cases: &[(&[Turn], &[Marked])] = &[
        // Their words now: held all the same (N3), and their yes runs it.
        (
            &[
                (
                    "A",
                    "Forget everything about me.",
                    Lamp::Calls(EVERYTHING, "Shall I forget everything about you?"),
                    0,
                ),
                ("A", "Yes.", Lamp::Says("Done."), 5),
            ],
            &[(EVERYTHING, true)],
        ),
        // Their own ask just before, the call at their yes: held, not run
        // (N3, the reviewer-lead's P2 on #3706), so nothing marked.
        (
            &[
                (
                    "A",
                    "Forget everything about me.",
                    Lamp::Says("Everything, really?"),
                    0,
                ),
                ("A", "Yes.", Lamp::Calls(EVERYTHING, "Done."), 5),
            ],
            &[],
        ),
        // Their yes to one held.
        (
            &[held, ("A", "Yes.", Lamp::Says("Done."), 5)],
            &[(EVERYTHING, true)],
        ),
        // A face forget, asked for in words: not everything.
        (
            &[("A", "delete my face", Lamp::Calls(FACE, "Done."), 0)],
            &[(FACE, false)],
        ),
        // The mark is the request's own: the next one starts without it.
        (
            &[
                (
                    "A",
                    "Forget everything about me.",
                    Lamp::Calls(EVERYTHING, "Shall I forget everything about you?"),
                    0,
                ),
                ("A", "Yes.", Lamp::Says("Done."), 5),
                ("A", "delete my face", Lamp::Calls(FACE, "Done."), 10),
            ],
            &[(EVERYTHING, true), (FACE, false)],
        ),
        // A bare forget: `forget_everything` refused, nothing run, nothing
        // marked.
        (
            &[
                (
                    "A",
                    "Forget that.",
                    Lamp::Calls(EVERYTHING, "Which thing?"),
                    0,
                ),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            &[],
        ),
    ];
    for (turns, runs) in cases {
        let ran = run(turns).await;
        assert_eq!(
            ran.marked,
            marked(runs),
            "{:?}",
            turns.iter().map(|turn| turn.1).collect::<Vec<_>>()
        );
    }
}

/// #3504, N3 (the person's decision, 27 Oct): on lamp-06c1 speech to text
/// heard "except my name" as "accept my name", and one person's whole
/// memory went. A spoken `forget_everything` never runs on the turn that
/// asks for it: the lamp asks once, and only the same person's plain yes on
/// the next turn runs it. A no, another person's yes, another line or
/// nothing at all forgets nothing.
#[tokio::test]
async fn a_spoken_forget_everything_always_asks_first() {
    let asks = Lamp::Calls(
        EVERYTHING,
        "Shall I forget everything I know about you, including your name?",
    );
    let ask = ("A", "Forget everything about me.", asks, 0);
    let misheard = ("A", "Forget everything about me. Accept my name.", asks, 0);
    let cases: &[(&[Turn], usize)] = &[
        // Asked: nothing deleted on the turn itself.
        (&[ask], 0),
        (&[misheard], 0),
        // Their plain yes next runs it, once.
        (
            &[ask, ("A", "Yes.", Lamp::Says("Done, it's all gone."), 5)],
            1,
        ),
        (&[misheard, ("A", "Vâng.", Lamp::Says("Xong."), 5)], 1),
        // A no, anything else, another person's yes, or nobody's.
        (
            &[ask, ("A", "No.", Lamp::Says("Okay, I'll keep it."), 5)],
            0,
        ),
        (&[ask, ("A", "What time is it?", Lamp::Says("Noon."), 5)], 0),
        (&[ask, ("B", "Yes.", Lamp::Says("Okay."), 5)], 0),
        (&[ask, ("", "Yes.", Lamp::Says("Okay."), 5)], 0),
        // Asking again in words asks again; it does not run.
        (&[ask, ("A", "Forget everything about me.", asks, 5)], 0),
        // A yes too late.
        (&[ask, ("A", "Yes.", Lamp::Says("Okay."), 61)], 0),
        // Called again on the yes: run once, never twice.
        (
            &[ask, ("A", "Yes.", Lamp::Calls(EVERYTHING, "Done."), 5)],
            1,
        ),
    ];
    for (turns, runs) in cases {
        let ran = run(turns).await;
        assert_eq!(
            ran.forgets.len(),
            *runs,
            "{:?}",
            turns
                .iter()
                .map(|turn| (turn.0, turn.1))
                .collect::<Vec<_>>()
        );
    }
    // The model is told to ask, naming the name as well.
    let ran = run(&[ask]).await;
    assert_eq!(
        ran.asked[1].last().cloned().unwrap(),
        Message::tool(
            "c1",
            json!({"held": crate::remember::FORGET_EVERYTHING_ASKS}).to_string()
        )
    );
    assert!(crate::remember::asks_about_forgetting(
        "Shall I forget everything I know about you, including your name?",
        EVERYTHING
    ));
}

/// What asks for everything, by its words alone, clause by clause, the
/// last one deciding (Reviewer-two's HOLD on #3608).
#[test]
fn only_unnegated_words_ask_to_forget_everything() {
    for said in [
        "Forget everything about me.",
        "Please erase everything you know about me",
        "Don't keep anything, forget everything.",
        "No, forget everything about me.",
        "xóa tất cả về tôi",
        "Quên tất cả đi",
    ] {
        assert!(crate::remember::asks_to_forget_everything(said), "{said}");
    }
    for said in [
        "Forget that.",
        "Don't forget everything.",
        "Never delete everything I said.",
        "Don't get rid of everything.",
        "Forget everything. Actually, don't forget everything.",
        "Forget everything. Actually, don't.",
        "Don't forget everything; forget that one.",
        "Forget everything but my name.",
        "Forget everything, except my name.",
        "Forget everything, but keep my name",
        "Forget everything, apart from my name",
        "Forget everything and don't forget everything.",
        // Conservative, the lead's decision on #3608: "not just that"
        // asks once too.
        "Forget everything, not just that.",
        "Xóa tất cả, trừ tên tôi.",
        "Forget everything, keep only my birthday.",
        "Forget everything except only my birthday.",
        "Forget everything except just my birthday.",
        "Xóa tất cả, chỉ giữ ngày sinh của tôi.",
        "Xóa tất cả, không chỉ cái đó.",
        "Forget everything, not just yet.",
        "Forget everything about me, not just now.",
        "Forget everything, not just like that, ask me first.",
        "Erase everything, not just yet, maybe tomorrow.",
        "Forget everything I told you. Not just now though.",
        "Should I have you forget everything? Not just yet.",
        "Forget everything. Cancel that.",
        "Forget everything. Stop.",
        "Forget everything. Hold on.",
        "Forget everything. Hang on.",
        "Everything is fine.",
        "Đừng quên tất cả.",
        "Không xóa tất cả.",
        "Xóa tất cả. Thôi, đừng.",
    ] {
        assert!(!crate::remember::asks_to_forget_everything(said), "{said}");
    }
    assert_eq!(
        crate::remember::everything_asked("Don't forget my keys."),
        None
    );
    assert_eq!(crate::remember::everything_asked("What's the time?"), None);
    assert_eq!(
        crate::remember::everything_asked("Forget that."),
        Some(false)
    );
}

/// Their words now outweigh what came before: a prior "forget everything"
/// then "don't forget everything", and a held `forget_everything` met with
/// a refusal, then a yes, run nothing; the held yes alone still runs.
#[tokio::test]
async fn a_refusal_now_outweighs_an_ask_before_and_a_held_yes() {
    let held = (
        "A",
        "Who am I?",
        Lamp::Calls(EVERYTHING, "Shall I forget everything about you?"),
        0,
    );
    let cases: &[(&[Turn], usize)] = &[
        (
            &[
                (
                    "A",
                    "Forget everything about me.",
                    Lamp::Says("Are you sure?"),
                    0,
                ),
                (
                    "A",
                    "Don't forget everything.",
                    Lamp::Calls(EVERYTHING, "Okay."),
                    5,
                ),
            ],
            0,
        ),
        (
            &[
                held,
                ("A", "No, don't forget everything.", Lamp::Says("Okay."), 5),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            0,
        ),
        (
            &[
                held,
                (
                    "A",
                    "Yes. No wait, don't forget everything.",
                    Lamp::Calls(EVERYTHING, "Okay."),
                    5,
                ),
            ],
            0,
        ),
        (&[held, ("A", "Yes.", Lamp::Says("Done."), 5)], 1),
    ];
    // Something kept back or taken back after "everything", in a later
    // clause, with a wrong All and a yes after it (Reviewer-two, 01:59).
    let saving: Vec<[Turn; 2]> = [
        "Forget everything, except my name.",
        "Forget everything, but keep my name",
        "Forget everything, apart from my name",
        "Forget everything and don't forget everything.",
        "Forget everything, keep only my birthday.",
        "Forget everything except only my birthday.",
        "Forget everything except just my birthday.",
        "Xóa tất cả, chỉ giữ ngày sinh của tôi.",
        "Forget everything, not just that.",
        "Forget everything, not just yet.",
        "Forget everything about me, not just now.",
        "Forget everything, not just like that, ask me first.",
        "Erase everything, not just yet, maybe tomorrow.",
        "Forget everything I told you. Not just now though.",
        "Should I have you forget everything? Not just yet.",
        "Forget everything. Cancel that.",
    ]
    .into_iter()
    .map(|said| {
        [
            ("A", said, Lamp::Calls(EVERYTHING, "Okay."), 0),
            ("A", "Yes.", Lamp::Says("Okay."), 5),
        ]
    })
    .collect();
    let cases: Vec<(&[Turn], usize)> = cases
        .iter()
        .copied()
        .chain(saving.iter().map(|turns| (&turns[..], 0)))
        .collect();
    for (turns, runs) in cases {
        let ran = run(turns).await;
        assert_eq!(
            ran.forgets.len(),
            runs,
            "{:?}",
            turns.iter().map(|turn| turn.1).collect::<Vec<_>>()
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
        !told[1]
            .description
            .contains(crate::remember::HELD_DESCRIBED)
    );
    assert!(told[1].description.contains("only when the person asks"));
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
        !serde_json::to_string(yes)
            .unwrap()
            .contains("old private context")
    );
    assert_eq!(yes.last(), Some(&Message::user("Yes.")));
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
/// at once, with no second question (Fourth's P3 on #2340). Not for
/// `forget_everything` (#3504, N3; the reviewer-lead's P2 on #3706): a yes
/// read there has no check that their face said it, so the call is held and
/// asked about, and only their yes to the hold runs it.
#[tokio::test]
async fn a_forget_they_asked_for_just_before_runs_on_the_call() {
    let ran = run(&[
        ("A", "Forget my face.", Lamp::Says("Your face, really?"), 0),
        ("A", "Yes.", Lamp::Calls(FACE, "Done, it's gone."), 5),
    ])
    .await;
    assert_eq!(ran.forgets, [(FACE.to_owned(), json!({"name": "A"}))]);
    // Run, never held: the model was given the forget's own result.
    let told = ran.asked[2].last().cloned().unwrap();
    assert_eq!(told, Message::tool("c1", json!({"ok": true}).to_string()));

    let asked = (
        "A",
        "Forget everything about me.",
        Lamp::Says("Everything, really?"),
        0,
    );
    let held = (
        "A",
        "Yes.",
        Lamp::Calls(EVERYTHING, "Shall I forget everything about you?"),
        5,
    );
    let ran = run(&[asked, held]).await;
    assert!(ran.forgets.is_empty());
    assert_eq!(
        ran.asked[2].last().cloned().unwrap(),
        Message::tool(
            "c1",
            json!({"held": crate::remember::FORGET_EVERYTHING_ASKS}).to_string()
        )
    );
    let ran = run(&[asked, held, ("A", "Yes.", Lamp::Says("Done."), 5)]).await;
    assert_eq!(ran.forgets, [(EVERYTHING.to_owned(), json!({"name": "A"}))]);
}

/// The reviewer-lead's P2 on #3706: A asks in words, the model asks in
/// prose, and a yes comes while A's face holds the scope. Where the eyes
/// read speaking, only A's face seen saying the yes to the hold runs the
/// wipe; someone else's yes under A's face forgets nothing.
#[tokio::test]
async fn a_wipe_after_a_prose_question_needs_their_face_seen_saying_yes() {
    let turns: &[Turn] = &[
        (
            "A",
            "Forget everything about me.",
            Lamp::Says("Everything, really?"),
            0,
        ),
        (
            "A",
            "Yes.",
            Lamp::Calls(EVERYTHING, "Shall I forget everything about you?"),
            5,
        ),
        ("A", "Yes.", Lamp::Says("Okay."), 5),
    ];
    let a: Sight = (&["A"], true);
    let cases: &[(&[Sight], usize, &str)] = &[
        // Someone else says each yes while A's face is at the lamp.
        (&[a, (&["B"], true), (&["B"], true)], 0, "B's yes"),
        (&[a, (&[], true), (&[], true)], 0, "nobody seen"),
        // A seen saying the yes to the hold.
        (&[a, (&["B"], true), a], 1, "A's yes to the hold"),
        (&[a, a, a], 1, "A throughout"),
    ];
    for (sight, runs, case) in cases {
        let ran = run_seen(turns, Some(sight)).await;
        assert_eq!(ran.forgets.len(), *runs, "{case}");
    }
}

/// Only the same person's own spoken words just before, in the same
/// session and within 60 s; never the lamp's words, its answer or a turn
/// it wrote. (With `forget_my_face`: `forget_everything` never runs this
/// way, #3504, N3.)
#[tokio::test]
async fn only_their_own_ask_just_before_counts() {
    let asked = ("A", "Forget my face.", Lamp::Says("Everything, really?"), 0);
    let calls = Lamp::Calls(FACE, "Okay.");
    let cases: &[(&[Turn], usize)] = &[
        (&[asked, ("A", "Yes.", calls, 60)], 1),
        // Too late, another person, nobody known.
        (&[asked, ("A", "Yes.", calls, 61)], 0),
        (&[asked, ("B", "Yes.", calls, 5)], 0),
        (&[asked, ("", "Yes.", calls, 5)], 0),
        (
            &[
                ("", "Forget my face.", Lamp::Says("Really?"), 0),
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
                ("A", "Who am I?", Lamp::Says("Shall I forget your face?"), 0),
                ("A", "Yes.", calls, 5),
            ],
            0,
        ),
        // A turn the lamp wrote asks it: not theirs.
        (
            &[
                (
                    "A",
                    "(the lamp wrote) Forget my face.",
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
        // Everything is held and asked about, never run this way (N3).
        (then("Xóa tất cả về tôi.", EVERYTHING), 0),
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

/// #2377, the reviewer's P3 1 on #2340: where the eyes read speaking
/// (`LAMP_EYES_SPEAKING=on`), a held forget's yes runs it only when the
/// held person's face is seen saying it, and no other face. Nobody seen
/// speaking, another face speaking while A is at the lamp, both, or no
/// known face beside the known file (a missing reading) runs nothing, and
/// the model is told once that the yes did not count. A's own face seen
/// saying it runs exactly the call.
#[tokio::test]
async fn where_the_eyes_read_speaking_only_their_face_saying_yes_runs_it() {
    let held = ("A", "What's my name?", ASKS, 0);
    let yes = ("A", "Yes.", Lamp::Says("Okay."), 5);
    let quiet: Sight = (&[], true);
    let cases: &[(&[Sight], usize, &str)] = &[
        (&[quiet, (&["A"], true)], 1, "A seen saying it"),
        (&[quiet, (&[], true)], 0, "nobody seen speaking"),
        (
            &[quiet, (&["B"], true)],
            0,
            "B seen speaking while A is at the lamp",
        ),
        (
            &[quiet, (&["A", "B"], true)],
            0,
            "A and B both seen speaking",
        ),
        (
            &[quiet, (&["A"], false)],
            0,
            "no known face beside the known file",
        ),
    ];
    for (sight, runs, case) in cases {
        let ran = run_seen(&[held, yes], Some(sight)).await;
        let want: Vec<(String, Value)> = (0..*runs)
            .map(|_| (FACE.to_owned(), json!({"name": "A"})))
            .collect();
        assert_eq!(ran.forgets, want, "{case}");
        let told = Message::system(crate::remember::unseen_yes_told(FACE));
        let told_on_yes = ran.asked.last().unwrap().contains(&told);
        assert_eq!(told_on_yes, *runs == 0, "{case}");
    }
}

/// #2377: after a yes that did not count, the one asked for in words,
/// "Yes, forget my face.", runs at once (the cost of a missed reading);
/// a second plain yes runs nothing, with nothing held.
#[tokio::test]
async fn after_an_unseen_yes_their_words_run_it_and_a_yes_does_not() {
    let held = ("A", "What's my name?", ASKS, 0);
    let unseen = (
        "A",
        "Yes.",
        Lamp::Says("Say \"Yes, forget my face.\" if you want that."),
        5,
    );
    let ran = run_seen(
        &[
            held,
            unseen,
            ("A", "Yes, forget my face.", Lamp::Calls(FACE, "Done."), 5),
        ],
        Some(&[(&[], true), (&[], true), (&[], true)]),
    )
    .await;
    assert_eq!(ran.forgets, [(FACE.to_owned(), json!({"name": "A"}))]);
    let ran = run_seen(
        &[held, unseen, ("A", "Yes.", Lamp::Says("Okay."), 5)],
        Some(&[(&[], true), (&[], true), (&["A"], true)]),
    )
    .await;
    assert_eq!(ran.forgets, []);
}

/// #2377: without `LAMP_EYES_SPEAKING`, #2340's hold is as it was: A's
/// plain yes runs it while their face is at the lamp, whoever the eyes see
/// speaking, and nothing extra is told.
#[tokio::test]
async fn without_the_eyes_switch_a_held_forget_is_as_before() {
    let held = ("A", "What's my name?", ASKS, 0);
    let ran = run(&[held, ("A", "Yes.", Lamp::Says("Done."), 5)]).await;
    assert_eq!(ran.forgets, [(FACE.to_owned(), json!({"name": "A"}))]);
    let told = Message::system(crate::remember::unseen_yes_told(FACE));
    assert!(ran.asked.iter().all(|asked| !asked.contains(&told)));
    let ran = run(&[held, ("B", "Yes.", Lamp::Says("Okay."), 5)]).await;
    assert_eq!(ran.forgets, []);
}

/// #3037: a line asking to forget, with the eyes reading speaking, records
/// at its end who the eyes saw saying it and the known person with their
/// face, each id with the eyes' boot, for that request's calls
/// (`speaking_faces::line_seen`). Without the eyes' switch nothing is
/// recorded. Request numbers of this test's own: the lines seen are the
/// process's.
#[tokio::test]
async fn a_lines_end_records_who_was_seen_saying_it() {
    use crate::speaking_faces::LineSeen;
    let asks = ("A", "Forget my face.", Lamp::Calls(FACE, "Done."), 0);
    let seen = |request, speaker: &str| {
        (
            request,
            Some(LineSeen {
                request,
                speakers: vec![format!("eyes/{speaker}")],
                known: Some(("A".to_owned(), "eyes/f1".to_owned())),
                enrollment: None,
            }),
        )
    };
    let ran = run_seen_from(&[asks], Some(&[(&["A"], true)]), 3_037_001).await;
    assert_eq!(ran.lines, [seen(3_037_001, "f1")]);
    let ran = run_seen_from(&[asks], Some(&[(&["B"], true)]), 3_037_002).await;
    assert_eq!(ran.lines, [seen(3_037_002, "f2")]);
    let ran = run_seen_from(&[asks], None, 3_037_003).await;
    assert_eq!(ran.lines, [(3_037_003, None)]);
}

#[tokio::test]
async fn a_held_memory_forgets_uncertain_result_cannot_copy_old_context() {
    let ran = run_seen_from_result(
        &[
            (
                "A",
                "What do you remember?",
                Lamp::Calls(EVERYTHING, "Shall I forget everything about you?"),
                0,
            ),
            ("A", "Yes.", Lamp::Says("I could not confirm that."), 5),
        ],
        None,
        938001,
        json!({"error": "old private context"}),
    )
    .await;
    assert_eq!(ran.forgets.len(), 1);
    let yes = ran.asked.last().unwrap();
    let encoded = serde_json::to_string(yes).unwrap();
    assert!(!encoded.contains("old private context"), "{encoded}");
    assert!(encoded.contains("uncertain"));
    assert_eq!(yes.last(), Some(&Message::user("Yes.")));
}
