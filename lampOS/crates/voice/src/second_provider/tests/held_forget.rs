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
    CallsWithStale(&'static str, &'static str),
    /// A `remember` of this text, then a line.
    Keeps(&'static str, &'static str),
    /// A `remember` of this text with a line in the same round, then the
    /// line again.
    KeepsAsking(&'static str, &'static str),
}

impl Lamp {
    /// The forget it calls, if any.
    fn name(&self) -> &'static str {
        match self {
            Lamp::Says(_) => "",
            Lamp::Calls(name, _) | Lamp::CallsWithStale(name, _) => name,
            Lamp::Keeps(..) | Lamp::KeepsAsking(..) => crate::remember::REMEMBER,
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
    second_audio_calls: usize,
    second_voice_prints: usize,
    second_voice_judgment: crate::same_voice::Confirmed,
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
    /// Every call run, by name and arguments, in order.
    calls: Vec<(String, Value)>,
    /// For each `remember` run, the words its scope was marked as a yes to
    /// a held keep about ([`crate::memory_scope::ScopeLease::released_keep`]).
    released: Vec<Option<String>>,
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
    run_switched(turns, sight, first, false, json!({"ok": true})).await
}

/// [`run_seen_from`], each forget answered with `result`.
async fn run_seen_from_result(
    turns: &[Turn],
    sight: Option<&[Sight]>,
    first: u64,
    result: Value,
) -> Ran {
    run_switched(turns, sight, first, false, result).await
}

/// [`run_seen_from_result`], a held face forget lasting through a spelling
/// question when `spelling` (`LAMP_NAMES_RECORD_TOLD`, #5141 row 7).
async fn run_switched(
    turns: &[Turn],
    sight: Option<&[Sight]>,
    first: u64,
    spelling: bool,
    result: Value,
) -> Ran {
    run_full(turns, sight, first, (spelling, false), result).await
}

/// [`run_switched`], with `LAMP_MEMORY_DOUBT_UNDOES` when `switches.1`
/// (C37): a `remember` is then answered as a new keep with id 391, and a
/// `forget` as forgotten.
async fn run_full(
    turns: &[Turn],
    sight: Option<&[Sight]>,
    first: u64,
    switches: (bool, bool),
    result: Value,
) -> Ran {
    run_full_with_audio(turns, sight, first, switches, result, false).await
}

/// The same actual Engine fixture through the optional spoken-audio runner.
async fn run_full_with_audio(
    turns: &[Turn],
    sight: Option<&[Sight]>,
    first: u64,
    (spelling, doubt_undoes): (bool, bool),
    result: Value,
    requested_audio: bool,
) -> Ran {
    use crate::speaking_faces::{SpeakingFaces, known_face_of};
    use std::sync::atomic::{AtomicBool, Ordering};
    let mut chats = VecDeque::new();
    for (who, _, lamp, _) in turns {
        match lamp {
            Lamp::Says(line) => chats.push_back(vec![text(line), Ok(ChatEvent::Done)]),
            Lamp::Keeps(kept, line) => {
                let calls = vec![call("c1", crate::remember::REMEMBER, json!({"text": kept}))];
                chats.push_back(vec![Ok(ChatEvent::ToolCalls(calls)), Ok(ChatEvent::Done)]);
                chats.push_back(vec![text(line), Ok(ChatEvent::Done)]);
            }
            Lamp::KeepsAsking(kept, line) => {
                let calls = vec![call("c1", crate::remember::REMEMBER, json!({"text": kept}))];
                chats.push_back(vec![
                    text(line),
                    Ok(ChatEvent::ToolCalls(calls)),
                    Ok(ChatEvent::Done),
                ]);
                chats.push_back(vec![text(line), Ok(ChatEvent::Done)]);
            }
            Lamp::Calls(name, line) | Lamp::CallsWithStale(name, line) => {
                let mut calls = vec![call("c1", name, json!({"name": who}))];
                if matches!(lamp, Lamp::CallsWithStale(_, _)) {
                    calls.push(call(
                        "stale",
                        "set_timer",
                        json!({"label": "old private context"}),
                    ));
                }
                chats.push_back(vec![Ok(ChatEvent::ToolCalls(calls)), Ok(ChatEvent::Done)]);
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
    let released = Arc::new(Mutex::new(Vec::new()));
    let releases = released.clone();
    let runner: ScopedToolRunner = Arc::new(move |request, heard, name, args, scope| {
        if name == crate::remember::REMEMBER {
            releases
                .lock()
                .unwrap()
                .push(scope.released_keep().map(str::to_owned));
        }
        if crate::remember::is_forget(&name) {
            marks
                .lock()
                .unwrap()
                .push((name.clone(), scope.everything_marked()));
        }
        let name_ran = name.clone();
        seen.lock().unwrap().push((name, args, heard));
        lines_seen
            .lock()
            .unwrap()
            .push((request, crate::speaking_faces::line_seen(request)));
        let result = match name_ran.as_str() {
            crate::remember::REMEMBER if doubt_undoes => json!({"remembered": true, "id": 391}),
            crate::remember::FORGET if doubt_undoes => json!({"forgotten": true}),
            _ => result.clone(),
        };
        Box::pin(async move { result })
    });
    let path = std::env::temp_dir().join(format!(
        "held-forget-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let cache = Arc::new(FaceCache::default());
    let (engine, _, now) = scripted(script.clone());
    let second_owner = Arc::new(crate::same_voice::OwnerVoice::new(0.45));
    // The cache keys are memory person IDs, not the display names used by
    // the scripted eyes. Match the admitted ScopeSnapshot's identity.
    let second_person = crate::remember::person_id("A");
    for request in 100..104 {
        assert!(second_owner.learn(&second_person, request, vec![1.0, 0.0, 0.0]));
    }
    let engine = engine.confirming_by_voice(super::super::VoiceConfirms {
        owner: second_owner.clone(),
        print_of: Arc::new(|_| None),
    });
    let eyes = Arc::new(SpeakingFaces::default());
    let mut engine = engine
        .with_memory_scope(
            lamp_turn_owner::BootId::new([12; 16]).unwrap(),
            cache.clone(),
            runner.clone(),
            Some(Arc::new(|_: &crate::memory_scope::ScopeLease| Told {
                text: "SYSTEM with old private context".into(),
                after: Some("old private context".into()),
                history: vec![Message::assistant("old private context")],
            })),
        )
        .announcing(false)
        .holding_forgets_through_spelling(spelling)
        .undoing_doubted_keeps(doubt_undoes)
        .asking_near_heard_words(NEAR_ASKS.with(|on| on.replace(false)));
    let audio_calls = Arc::new(AtomicUsize::new(0));
    if requested_audio {
        let seen = audio_calls.clone();
        engine = engine.requesting_audio(
            lamp_turn_owner::BootId::new([12; 16]).unwrap(),
            Arc::new(move |context, call, scope| {
                seen.fetch_add(1, Ordering::SeqCst);
                let running = runner(context.request, true, call.name, call.args, scope);
                Box::pin(async move { crate::tools::ToolAnswer::plain(running.await) })
            }),
        );
    }
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
    let calls = forgets
        .lock()
        .unwrap()
        .iter()
        .map(|(name, args, _)| (name.clone(), args.clone()))
        .collect();
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
    let released = released.lock().unwrap().clone();
    Ran {
        second_audio_calls: audio_calls.load(Ordering::SeqCst),
        second_voice_prints: second_owner.known(&second_person),
        second_voice_judgment: second_owner.judge(&second_person, &[1.0, 0.0, 0.0]),
        forgets,
        asked,
        lines,
        marked,
        calls,
        released,
    }
}

thread_local! {
    /// `LAMP_MEMORY_ASK_NEAR_HEARD` ([`Engine::asking_near_heard_words`])
    /// for the next [`run_full`] on this thread, once.
    static NEAR_ASKS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
            // Not "Okay, I'll keep it.": a bare keep is a keep claim now
            // (lamp-06c1, build 10226, 5 Nov), asked again with no call.
            &[ask, ("A", "No.", Lamp::Says("Okay, I won't forget it."), 5)],
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
    // Run, never held: its fixed outcome survives in the fresh request.
    let next = &ran.asked[2];
    assert_eq!(next.last(), Some(&Message::user("Yes.")));
    let encoded = serde_json::to_value(next).unwrap();
    let receipt = json!({"tool": FACE, "outcome": {"ok": true, "face_forgotten": true}});
    assert!(
        encoded[1]["content"]
            .as_str()
            .unwrap()
            .contains(&receipt.to_string())
    );
    let clean = serde_json::to_string(next).unwrap();
    assert!(!clean.contains("old private context"));
    assert!(!clean.contains("tool_call_id"));

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
                by_voice: false,
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

/// lamp-0c4e, 3 Nov (#5141 row 7): a face saved under a misheard name, and
/// the forget the model held for their yes was dropped when the lamp's
/// answer asked them to spell their name, so the wrong record stuck. With
/// `LAMP_NAMES_RECORD_TOLD` their spelling carries it once, and their plain
/// yes to the lamp's question about it then runs it; every other guard
/// stands. Off, it is dropped as before.
#[tokio::test]
async fn with_the_record_switch_a_held_face_forget_lasts_through_the_spelling_question() {
    let held = (
        "A",
        "That's not my name.",
        Lamp::Calls(FACE, "Sorry. How do you spell your name?"),
        0,
    );
    let spelled = (
        "A",
        "L, A, U, R, E, N.",
        Lamp::Says("Thanks. Shall I forget your face?"),
        5,
    );
    let yes = ("A", "Yes.", Lamp::Says("Done."), 5);
    let face = || vec![(FACE.to_owned(), json!({"name": "A"}))];
    // Each case: its turns, whether the switch is on, and the forgets run.
    type Case<'a> = (&'a [Turn], bool, Vec<(String, Value)>);
    let cases: &[Case] = &[
        (&[held, spelled, yes], true, face()),
        // Off: dropped at the spelling question, as before.
        (&[held, spelled, yes], false, Vec::new()),
        // Someone else spells, or their spelling's answer asks nothing
        // about the forget, or asks them to spell again: dropped.
        (
            &[held, ("B", "L, A, U, R, E, N.", spelled.2, 5), yes],
            true,
            Vec::new(),
        ),
        (
            &[
                held,
                ("A", "L, A, U, R, E, N.", Lamp::Says("Nice to meet you."), 5),
                yes,
            ],
            true,
            Vec::new(),
        ),
        (
            &[
                held,
                (
                    "A",
                    "L, A, U, R, E, N.",
                    Lamp::Says("Can you spell it again?"),
                    5,
                ),
                spelled,
                yes,
            ],
            true,
            Vec::new(),
        ),
        // A yes in place of the spelling runs nothing, and the minute still
        // counts from the held turn.
        (&[held, yes, yes], true, Vec::new()),
        (
            &[held, spelled, ("A", "Yes.", Lamp::Says("Done."), 56)],
            true,
            Vec::new(),
        ),
    ];
    for (at, (turns, on, want)) in cases.iter().enumerate() {
        let ran = run_switched(turns, None, 1, *on, json!({"ok": true})).await;
        assert_eq!(&ran.forgets, want, "case {at}");
    }
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

#[tokio::test]
async fn a_face_forgets_partial_cleanup_reaches_the_next_model_request() {
    let result = crate::remember::face_and_memory(
        json!({"ok": true}),
        &json!({
            "forgotten": true,
            "everything": false,
            "room_memory_result": "kept",
            "sound_consent_result": "uncertain",
            "face_expression_consent_result": "cleared",
            "private": "internal-memory canary",
        }),
    );
    let ran = run_seen_from_result(
        &[(
            "A",
            "Forget my face.",
            Lamp::Calls(FACE, "I could not confirm all related cleanup."),
            0,
        )],
        None,
        5_803_001,
        result,
    )
    .await;
    assert_eq!(ran.calls, [(FACE.to_owned(), json!({"name": "A"}))]);
    assert_eq!(ran.asked.len(), 2);
    let next = &ran.asked[1];
    assert_eq!(next.last(), Some(&Message::user("Forget my face.")));
    let clean = serde_json::to_string(next).unwrap();
    for canary in [
        "internal-memory canary",
        "old private context",
        "tool_call_id",
    ] {
        assert!(!clean.contains(canary), "{clean}");
    }
    let receipt = json!({"tool": FACE, "outcome": {
        "ok": true,
        "face_forgotten": true,
        "primary_memory_result": "forgotten",
        "everything": false,
        "room_memory_result": "kept",
        "sound_consent_result": "uncertain",
        "face_expression_consent_result": "cleared",
    }});
    let encoded = serde_json::to_value(next).unwrap();
    assert!(
        encoded[1]["content"]
            .as_str()
            .unwrap()
            .contains(&receipt.to_string())
    );
}

/// C37 (lamp-06c1, 5 Nov): round 1 was a bare `remember`, and round 2 said
/// "Was that 'rhinodulus' right? I want to be sure", the word already
/// kept. With `LAMP_MEMORY_DOUBT_UNDOES` that keep is forgotten by its id
/// at once and held for their yes, which keeps it again as called.
#[tokio::test]
async fn a_keep_the_next_round_doubts_is_taken_back_until_their_yes() {
    const REMEMBER: &str = crate::remember::REMEMBER;
    const FORGET: &str = crate::remember::FORGET;
    let doubted = Lamp::Calls(
        REMEMBER,
        "Kept. Was that 'rhinodulus' right? I want to be sure.",
    );
    let a: Sight = (&["A"], true);
    let kept_again = [
        (REMEMBER.to_owned(), json!({"name": "A"})),
        (FORGET.to_owned(), json!({"source_id": 391})),
        (REMEMBER.to_owned(), json!({"name": "A"})),
    ];
    let undone = &kept_again[..2];
    let kept = &kept_again[..1];
    let flower = "My favorite flower is a ranunculus.";
    /// The turns, their eyes, the switch, the calls run, and the case.
    type Case<'a> = (
        &'a [Turn],
        &'a [Sight],
        bool,
        &'a [(String, Value)],
        &'a str,
    );
    let cases: &[Case] = &[
        (
            &[
                ("A", flower, doubted, 0),
                ("A", "Yes.", Lamp::Says("Kept."), 5),
            ],
            &[a, a],
            true,
            &kept_again,
            "their yes keeps it again",
        ),
        (
            &[
                ("A", flower, doubted, 0),
                ("A", "No, ranunculus.", Lamp::Says("Ranunculus, got it."), 5),
            ],
            &[a, a],
            true,
            undone,
            "no yes: stays taken back",
        ),
        (
            &[
                ("A", flower, doubted, 0),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            &[a, (&["B"], true)],
            true,
            undone,
            "someone else's yes",
        ),
        (
            &[
                ("A", flower, doubted, 0),
                ("A", "Yes.", Lamp::Says("Okay."), 90),
            ],
            &[a, a],
            true,
            undone,
            "a yes too late",
        ),
        (
            &[
                ("A", flower, doubted, 0),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            &[a, a],
            false,
            kept,
            "switch off: as before",
        ),
        (
            &[
                (
                    "A",
                    flower,
                    Lamp::Calls(REMEMBER, "Kept: your favorite flower is the ranunculus."),
                    0,
                ),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            &[a, a],
            true,
            kept,
            "a plain say-back",
        ),
        (
            &[
                (
                    "A",
                    flower,
                    Lamp::Calls(
                        REMEMBER,
                        "Kept. Was that the day you went to Hanoi? Right, the trip.",
                    ),
                    0,
                ),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            &[a, a],
            true,
            kept,
            "a question about something else",
        ),
        (
            &[
                ("", flower, doubted, 0),
                ("", "Yes.", Lamp::Says("Okay."), 5),
            ],
            &[(&[], false), (&[], false)],
            true,
            &[(REMEMBER.to_owned(), json!({"name": ""}))],
            "nobody known: the room's keep stays",
        ),
    ];
    for (at, (turns, sight, on, calls, case)) in cases.iter().enumerate() {
        let ran = run_full(
            turns,
            Some(sight),
            9101 + 10 * at as u64,
            (false, *on),
            json!({"ok": true}),
        )
        .await;
        assert_eq!(ran.calls, *calls, "{case}");
    }
}

/// C37, the memory owner's call A (11 Nov). On lamp-06c1 on 10 Nov, twice,
/// "ranunculus" was heard as "renunctinus", kept in round 1, and round 2
/// asked "Kept as you said it … is that ranunculus?". That keep is taken
/// back at once, nothing of the misheard word stays kept, and their yes
/// keeps the word asked. A no keeps nothing; a reply asking nothing, or a
/// keep someone else made, is never taken back.
#[tokio::test]
async fn a_keep_its_next_round_offers_another_word_for_waits_for_their_yes_to_that_word() {
    const REMEMBER: &str = crate::remember::REMEMBER;
    const FORGET: &str = crate::remember::FORGET;
    const MISHEARD: &str = "Their favorite flower is a renunctinus.";
    const SAID: &str = "Remember that my favorite flower is a renunctinus.";
    let asks = Lamp::Keeps(MISHEARD, "Kept as you said it \u{2026} is that ranunculus?");
    let a: Sight = (&["A"], true);
    let b: Sight = (&["B"], true);
    let keep = |text: &str| (REMEMBER.to_owned(), json!({ "text": text }));
    let undo = (FORGET.to_owned(), json!({"source_id": 391}));
    let kept_misheard = vec![keep(MISHEARD)];
    let undone = vec![keep(MISHEARD), undo.clone()];
    let kept_asked = vec![
        keep(MISHEARD),
        undo,
        keep("Their favorite flower is a ranunculus."),
    ];
    /// The turns, their eyes, the calls run, and the case.
    type Case<'a> = (&'a [Turn], &'a [Sight], &'a [(String, Value)], &'a str);
    let cases: &[Case] = &[
        (
            &[("A", SAID, asks, 0)],
            &[a],
            &undone,
            "the 06c1 sequence: renunctinus is not left kept",
        ),
        (
            &[("A", SAID, asks, 0), ("A", "Yes.", Lamp::Says("Kept."), 5)],
            &[a, a],
            &kept_asked,
            "their yes keeps ranunculus",
        ),
        (
            &[
                ("A", SAID, asks, 0),
                ("A", "No.", Lamp::Says("Okay, how is it said?"), 5),
            ],
            &[a, a],
            &undone,
            "their no keeps nothing",
        ),
        (
            &[
                (
                    "A",
                    SAID,
                    Lamp::Keeps(MISHEARD, "Kept: your favorite flower is a renunctinus."),
                    0,
                ),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            &[a, a],
            &kept_misheard,
            "no confirm question: nothing undone",
        ),
        (
            &[
                ("B", SAID, Lamp::Keeps(MISHEARD, "Kept."), 0),
                (
                    "A",
                    "What's my favorite flower?",
                    Lamp::Says("Is that ranunculus?"),
                    5,
                ),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            &[b, a, a],
            &kept_misheard,
            "another person's keep is never undone",
        ),
    ];
    for (at, (turns, sight, calls, case)) in cases.iter().enumerate() {
        let ran = run_full(
            turns,
            Some(sight),
            9501 + 10 * at as u64,
            (false, true),
            json!({"ok": true}),
        )
        .await;
        assert_eq!(ran.calls, *calls, "{case}");
        // Their yes is a yes to the lamp's question about those words, so
        // the memory's route keeps them for a line that is only "Yes."
        // (`remember::heard::yes_alone`, `LAMP_MEMORY_KEEP_HEARD`).
        if *calls == kept_asked {
            assert_eq!(
                ran.released,
                [
                    None,
                    Some("Their favorite flower is a ranunculus.".to_owned())
                ],
                "{case}"
            );
        }
    }
}

/// The same question beside the keep, in its own round: the keep is not
/// run (#3504 finding N5), so nothing misheard is kept.
#[tokio::test]
async fn a_keep_beside_a_question_offering_another_word_is_not_run() {
    let ran = run_full(
        &[(
            "A",
            "Remember that my favorite flower is a renunctinus.",
            Lamp::KeepsAsking(
                "Their favorite flower is a renunctinus.",
                "Is that ranunculus?",
            ),
            0,
        )],
        Some(&[(&["A"], true)]),
        9601,
        (false, true),
        json!({"ok": true}),
    )
    .await;
    assert_eq!(ran.calls, []);
}

/// Without the eyes' sight no yes could keep it again, so nothing is taken
/// back.
#[tokio::test]
async fn a_doubted_keep_stays_without_the_eyes() {
    let ran = run_full(
        &[(
            "A",
            "My favorite flower is a ranunculus.",
            Lamp::Calls(
                crate::remember::REMEMBER,
                "Kept. Was that 'rhinodulus' right?",
            ),
            0,
        )],
        None,
        9301,
        (false, true),
        json!({"ok": true}),
    )
    .await;
    assert_eq!(
        ran.calls,
        [(crate::remember::REMEMBER.to_owned(), json!({"name": "A"}))]
    );
}

/// An admitted face forget may also empty personal memory. Neither its
/// next request nor later calls planned from the old context may reuse it.
#[tokio::test]
async fn face_forget_context_ordinary_discards_the_entire_next_request_and_batch() {
    for result in [
        json!({"ok": true, "memory": "forgotten too"}),
        json!({"ok": true, "everything": false, "memory": "private result canary"}),
        json!({"ok": false, "error": "private result canary"}),
    ] {
        let ran = run_seen_from_result(
            &[(
                "A",
                "Forget my face, then set a timer.",
                Lamp::CallsWithStale(FACE, "I will report only the confirmed outcome."),
                0,
            )],
            None,
            5_851_001,
            result,
        )
        .await;
        assert_eq!(ran.asked.len(), 2);
        assert!(
            serde_json::to_string(&ran.asked[0])
                .unwrap()
                .contains("old private context")
        );
        let clean = serde_json::to_string(&ran.asked[1]).unwrap();
        assert!(!clean.contains("old private context"), "{clean}");
        assert!(!clean.contains("private result canary"), "{clean}");
        assert!(!clean.contains("tool_call_id"), "{clean}");
        assert!(clean.contains("1 later calls"), "{clean}");
        assert_eq!(ran.calls, [(FACE.to_owned(), json!({"name": "A"}))]);
        assert_eq!(
            ran.asked[1].last(),
            Some(&Message::user("Forget my face, then set a timer."))
        );
    }
}

/// The same boundary applies before round one when a held face forget is
/// admitted by the person's yes, including a missing or partial receipt.
#[tokio::test]
async fn face_forget_context_held_yes_discards_the_entire_next_request() {
    for result in [
        json!({"ok": true, "memory": "forgotten too"}),
        json!({"ok": true, "everything": false, "memory": "private result canary"}),
        json!({"error": "private result canary"}),
    ] {
        let ran = run_seen_from_result(
            &[
                ("A", "What do you know about me?", ASKS, 0),
                (
                    "A",
                    "Yes.",
                    Lamp::Says("I will report only the confirmed outcome."),
                    5,
                ),
            ],
            None,
            5_851_101,
            result,
        )
        .await;
        assert_eq!(ran.calls, [(FACE.to_owned(), json!({"name": "A"}))]);
        let clean = serde_json::to_string(ran.asked.last().unwrap()).unwrap();
        assert!(!clean.contains("old private context"), "{clean}");
        assert!(!clean.contains("private result canary"), "{clean}");
        assert_eq!(
            ran.asked.last().unwrap().last(),
            Some(&Message::user("Yes."))
        );
    }
}

/// Cutting an admitted forget while its runner waits must not carry an
/// earlier recall round into the empty line's resumed model request.
async fn cut_during_forget_discards_recalled_round(name: &'static str, held: bool) {
    let question = if held {
        "What do you know about me?"
    } else if name == FACE {
        "Forget my face."
    } else {
        "Forget my favorite color."
    };
    let mut heard = vec![Ok(question.to_owned())];
    if held {
        heard.push(Ok("Yes.".to_owned()));
    }
    heard.push(Ok(String::new()));
    let mut chats = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "private-recall-call-canary",
                crate::remember::RECALL,
                json!({"query": "favorite color"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "forget-call",
                name,
                json!({"name": "A", "text": "favorite color"}),
            )])),
            Ok(ChatEvent::Done),
        ],
    ];
    if held {
        chats.push(vec![text("Shall I forget your face?"), Ok(ChatEvent::Done)]);
    }
    chats.push(vec![
        text("I will report only what is confirmed."),
        Ok(ChatEvent::Done),
    ]);
    let script = Arc::new(Script {
        heard: Mutex::new(heard.into()),
        chats: Mutex::new(chats.into()),
        ..Script::default()
    });
    let (begun, waiting) = std::sync::mpsc::channel();
    let runner: ToolRunner = Arc::new(move |_, _, called, _| {
        let begun = begun.clone();
        Box::pin(async move {
            if called == name {
                begun.send(()).unwrap();
                std::future::pending::<()>().await;
            }
            json!({"memories": ["private-recall-result-canary"]})
        })
    });
    let scoped_runner = runner.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        runner,
        Arc::new(Instant::now),
    )
    .announcing(false);
    if held {
        // A confirmation belongs to a known person's session; an
        // anonymous yes is correctly refused before the runner.
        let path = std::env::temp_dir().join(format!(
            "cut-held-forget-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, "A").unwrap();
        let cache = Arc::new(FaceCache::default());
        engine = engine.with_memory_scope(
            lamp_turn_owner::BootId::new([43; 16]).unwrap(),
            cache.clone(),
            Arc::new(move |request, heard, name, args, _| {
                scoped_runner(request, heard, name, args)
            }),
            None,
        );
        engine.memory_camera(true, 1);
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&path),
        );
        std::fs::remove_file(path).unwrap();
    }
    let request = 5_878_100;
    engine.submit(start(request));
    engine.submit(audio(request, 1));
    engine.submit(end(request));
    let forgetting = if held {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
                .await
                .expect("the face-forget question finishes");
            if matches!(event, ProviderOutput::TurnComplete { request: done, .. } if done == request)
            {
                break;
            }
        }
        engine.submit(start(request + 1));
        engine.submit(audio(request + 1, 1));
        engine.submit(end(request + 1));
        request + 1
    } else {
        request
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if waiting.try_recv().is_ok() {
                break;
            }
            let _ = tokio::time::timeout(Duration::from_millis(20), engine.next()).await;
        }
    })
    .await
    .expect("the admitted forget reaches its blocked runner");
    {
        let asked = script.asked.lock().unwrap();
        assert!(
            serde_json::to_string(&*asked)
                .unwrap()
                .contains("private-recall-result-canary")
        );
    }
    let resumed = forgetting + 1;
    engine.submit(start(resumed));
    engine.retire_through(forgetting);
    engine.submit(audio(resumed, 1));
    engine.submit(end(resumed));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("the empty line's resumed turn finishes");
        if matches!(event, ProviderOutput::TurnComplete { request: done, .. } if done == resumed) {
            break;
        }
    }
    let asked = script.asked.lock().unwrap();
    assert!(asked.len() >= if held { 4 } else { 3 });
    let next = serde_json::to_string(asked.last().unwrap()).unwrap();
    for canary in ["private-recall-call-canary", "private-recall-result-canary"] {
        assert!(
            !next.contains(canary),
            "{canary} survived cut during {name}: {next}"
        );
    }
}

#[tokio::test]
async fn cut_during_face_forget_drops_prior_recall_from_resumed_request() {
    cut_during_forget_discards_recalled_round(FACE, false).await;
}

#[tokio::test]
async fn cut_during_held_face_forget_drops_prior_recall_from_resumed_request() {
    cut_during_forget_discards_recalled_round(FACE, true).await;
}

#[tokio::test]
async fn cut_during_plain_forget_drops_prior_recall_from_resumed_request() {
    cut_during_forget_discards_recalled_round(crate::remember::FORGET, false).await;
}

/// C37 (lamp-06c1, 5 Nov, `LAMP_MEMORY_ASK_NEAR_HEARD`): "ranunculus" was
/// heard as "renunculus" and the model's `remember` wrote "ranunculus", so
/// `LAMP_MEMORY_KEEP_HEARD` kept the heard line while the lamp said the
/// other word. The keep is held and asked about instead: their yes keeps
/// the model's sentence, a no or nothing keeps nothing. A sentence of the
/// words heard, or a word near nothing heard, runs as before; so does every
/// keep with the switch off, or without the eyes.
#[tokio::test]
async fn a_keep_with_a_near_spelling_of_a_heard_word_waits_for_their_yes() {
    const REMEMBER: &str = crate::remember::REMEMBER;
    const SAID: &str = "Remember that my favourite flower is a renunculus.";
    const NEAR: &str = "Their favorite flower is a ranunculus.";
    const HEARD: &str = "My favourite flower is a renunculus.";
    const TULIP: &str = "Their favourite flower is a tulip.";
    const UNASKED: &str = "My favourite flower is a renunculus.";
    const ASKS_NEAR: Lamp = Lamp::Keeps(NEAR, "Is that ranunculus?");
    const A: Sight = (&["A"], true);
    let keep = |text: &str| vec![(REMEMBER.to_owned(), json!({ "text": text }))];
    /// The turns, their eyes, the switch, the calls run, and the case.
    type Case<'a> = (
        &'a [Turn],
        Option<&'a [Sight]>,
        bool,
        Vec<(String, Value)>,
        &'a str,
    );
    let cases: Vec<Case> = vec![
        (
            &[("A", SAID, ASKS_NEAR, 0)],
            Some(&[A]),
            true,
            vec![],
            "asked, not kept silently",
        ),
        (
            &[
                ("A", SAID, ASKS_NEAR, 0),
                ("A", "Yes.", Lamp::Says("Kept."), 5),
            ],
            Some(&[A, A]),
            true,
            keep(NEAR),
            "their yes keeps ranunculus",
        ),
        (
            &[
                ("A", SAID, ASKS_NEAR, 0),
                ("A", "No.", Lamp::Says("Okay, how is it said?"), 5),
            ],
            Some(&[A, A]),
            true,
            vec![],
            "their no keeps nothing",
        ),
        (
            &[
                ("A", SAID, Lamp::Keeps(NEAR, "What a lovely flower."), 0),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            Some(&[A, A]),
            true,
            vec![],
            "an answer that never asked: dropped, nothing kept",
        ),
        (
            &[("A", SAID, Lamp::Keeps(HEARD, "Kept."), 0)],
            Some(&[A]),
            true,
            keep(HEARD),
            "the words heard: kept at once",
        ),
        (
            &[("A", SAID, Lamp::Keeps(TULIP, "Kept."), 0)],
            Some(&[A]),
            true,
            keep(TULIP),
            "a word near nothing heard: as before (the heard line, in the memory's route)",
        ),
        (
            &[
                ("A", UNASKED, ASKS_NEAR, 0),
                ("A", "Yes.", Lamp::Says("Okay."), 5),
            ],
            Some(&[A, A]),
            true,
            keep(NEAR),
            "a keep the line didn't ask for: not held, keep_quietly's",
        ),
        (
            &[("A", SAID, ASKS_NEAR, 0)],
            Some(&[A]),
            false,
            keep(NEAR),
            "off: as before",
        ),
        (
            &[("A", SAID, ASKS_NEAR, 0)],
            None,
            true,
            keep(NEAR),
            "without the eyes: as before",
        ),
    ];
    for (at, (turns, sight, near, calls, case)) in cases.into_iter().enumerate() {
        NEAR_ASKS.with(|on| on.set(near));
        let ran = run_full(
            turns,
            sight,
            9701 + 10 * at as u64,
            (false, false),
            json!({"remembered": true}),
        )
        .await;
        assert_eq!(ran.calls, calls, "{case}");
        if case == "their yes keeps ranunculus" {
            // Their "Yes." is a yes to the lamp's question about the
            // model's sentence, which the memory's route keeps
            // (`remember::heard::released`).
            assert_eq!(ran.released, [Some(NEAR.to_owned())], "{case}");
        }
        if case.starts_with("a keep the line didn't ask for") {
            // Not held: the model is never told to ask, and their "Yes."
            // releases nothing. The call goes on to `keep_quietly::unheard`
            // (`LAMP_KEEP_QUIETLY`, on in the product), which refuses it:
            // "ranunculus" was not heard, so nothing is kept.
            assert_eq!(ran.released, [None], "{case}");
            assert!(
                !format!("{:?}", ran.asked).contains("Not kept yet"),
                "{case}"
            );
            assert!(
                crate::keep_quietly::refused(NEAR, Some(UNASKED), false),
                "{case}"
            );
        }
    }
}

/// Thirteen's row on #5627: a yes dropped as not the held forget's person
/// says why (no one known on the line, theirs in a later session, or
/// another person), each still opening "not the same person" so old greps
/// match, and never with an id.
#[test]
fn a_yes_not_the_held_persons_says_why() {
    let held: HeldAsker = (3, "p-a".to_owned());
    assert_eq!(not_theirs(Some(&held), &held), None);
    let cases = [
        (None, "not the same person: no one known on the line"),
        (
            Some((4, "p-a".to_owned())),
            "not the same person: theirs, in a later session",
        ),
        (
            Some((3, "p-b".to_owned())),
            "not the same person: another person",
        ),
    ];
    for (line, said) in cases {
        let why = not_theirs(line.as_ref(), &held).unwrap();
        assert_eq!(why, said);
        assert!(!why.contains("p-"), "no id is said: {why}");
    }
}

/// Friday's follow-up on #6990: a late one-face yes reads "too late" from
/// anyone on the line, another person or no one known included; only an
/// in-time yes says whose it is not.
#[test]
fn a_late_one_face_yes_reads_too_late_whoever_says_it() {
    let held: HeldAsker = (3, "p-a".to_owned());
    let lines = [
        None,
        Some(held.clone()),
        Some((4, "p-a".to_owned())),
        Some((3, "p-b".to_owned())),
    ];
    for line in &lines {
        assert_eq!(
            late_or_not_theirs(true, line.as_ref(), &held),
            Some("too late"),
            "{line:?}"
        );
        assert_eq!(
            late_or_not_theirs(false, line.as_ref(), &held),
            not_theirs(line.as_ref(), &held),
            "{line:?}"
        );
    }
}

#[tokio::test]
async fn second_admitted_forget_everything_drops_learned_confirmation_prints() {
    let ran = run_seen_from_result(
        &[
            (
                "A",
                "Forget everything about me.",
                Lamp::Calls(
                    EVERYTHING,
                    "Shall I forget everything I know about you, including your name?",
                ),
                0,
            ),
            ("A", "Yes.", Lamp::Says("Done, it's all gone."), 5),
        ],
        None,
        45001,
        json!({"forgotten": true, "everything": true, "face": "forgotten too"}),
    )
    .await;
    assert_eq!(ran.forgets.len(), 1, "forget must actually run");
    assert_eq!(ran.marked, vec![(EVERYTHING.to_owned(), true)]);
    assert_eq!(
        ran.second_voice_prints, 0,
        "after admitted forget: {:?}",
        ran.second_voice_judgment
    );
    assert_eq!(
        ran.second_voice_judgment,
        crate::same_voice::Confirmed::Unknown
    );
}

#[tokio::test]
async fn second_declined_forget_keeps_learned_confirmation_prints() {
    let ran = run(&[
        (
            "A",
            "Forget everything about me.",
            Lamp::Calls(
                EVERYTHING,
                "Shall I forget everything I know about you, including your name?",
            ),
            0,
        ),
        ("A", "No.", Lamp::Says("Okay, I won't forget it."), 5),
    ])
    .await;
    assert!(ran.forgets.is_empty());
    assert_eq!(ran.second_voice_prints, 4);
    assert!(matches!(
        ran.second_voice_judgment,
        crate::same_voice::Confirmed::Theirs(_)
    ));
}

#[tokio::test]
async fn confirmation_prints_survive_failed_or_other_persons_forget() {
    for (person, receipt) in [
        ("A", json!({"forgotten":false, "error":"unavailable"})),
        ("B", json!({"forgotten":true, "everything":true})),
    ] {
        let ran = run_seen_from_result(
            &[
                (
                    person,
                    "Forget everything about me.",
                    Lamp::Calls(
                        EVERYTHING,
                        "Shall I forget everything I know about you, including your name?",
                    ),
                    0,
                ),
                (person, "Yes.", Lamp::Says("Okay."), 5),
            ],
            None,
            45002,
            receipt,
        )
        .await;
        assert_eq!(ran.forgets.len(), 1);
        assert_eq!(ran.second_voice_prints, 4);
        assert!(matches!(
            ran.second_voice_judgment,
            crate::same_voice::Confirmed::Theirs(_)
        ));
    }
}

/// Exercise the Engine's actual run_tool target capture, not just its
/// cleanup helper. Receipts are synthetic; real eyes deletion is separate.
#[tokio::test]
async fn face_forget_receipts_clear_only_the_successfully_deleted_persons_voice() {
    for (at, (person, receipt, kept)) in [
        ("A", json!({"ok": true}), 0),
        ("A", json!({"error": "face deletion failed"}), 4),
        ("B", json!({"ok": true}), 4),
    ]
    .into_iter()
    .enumerate()
    {
        let ran = run_seen_from_result(
            &[(person, "Forget my face.", Lamp::Calls(FACE, "Okay."), 0)],
            None,
            45_100 + at as u64 * 10,
            receipt,
        )
        .await;
        assert_eq!(ran.forgets, [(FACE.to_owned(), json!({"name": person}))]);
        assert_eq!(ran.second_voice_prints, kept);
        if kept == 0 {
            assert_eq!(
                ran.second_voice_judgment,
                crate::same_voice::Confirmed::Unknown
            );
        } else {
            assert!(matches!(
                ran.second_voice_judgment,
                crate::same_voice::Confirmed::Theirs(_)
            ));
        }
    }
}

#[tokio::test]
async fn a_held_face_forget_clears_confirmation_voice_only_after_yes() {
    for (answer, runs, kept) in [("Yes.", 1, 0), ("No.", 0, 4)] {
        let ran = run_seen_from_result(
            &[
                ("A", "What's my name?", ASKS, 0),
                ("A", answer, Lamp::Says("Okay."), 5),
            ],
            None,
            45_200,
            json!({"ok": true}),
        )
        .await;
        assert_eq!(ran.forgets.len(), runs);
        assert_eq!(ran.second_voice_prints, kept);
    }
}

#[tokio::test]
async fn a_successful_face_forget_clears_confirmation_prints_through_the_audio_runner() {
    for requested_audio in [false, true] {
        let ran = run_full_with_audio(
            &[("A", "Forget my face.", Lamp::Calls(FACE, "Done."), 0)],
            None,
            46_100 + u64::from(requested_audio),
            (false, false),
            json!({"ok": true}),
            requested_audio,
        )
        .await;
        assert_eq!(ran.forgets.len(), 1);
        assert_eq!(ran.second_audio_calls, usize::from(requested_audio));
        assert_eq!(
            ran.second_voice_prints, 0,
            "requested audio: {requested_audio}"
        );
    }
}

#[tokio::test]
async fn the_audio_runner_preserves_confirmation_prints_after_failed_or_other_person_deletion() {
    for (index, who, result) in [
        (0, "A", json!({"ok": false, "error": "storage"})),
        (1, "A", json!({"ok": false})),
        (2, "B", json!({"ok": true})),
    ] {
        let ran = run_full_with_audio(
            &[(who, "Forget my face.", Lamp::Calls(FACE, "Okay."), 0)],
            None,
            46_120 + index,
            (false, false),
            result,
            true,
        )
        .await;
        assert_eq!(ran.forgets.len(), 1);
        assert_eq!(ran.second_audio_calls, 1);
        assert_eq!(ran.second_voice_prints, 4, "case {index}");
    }
}
