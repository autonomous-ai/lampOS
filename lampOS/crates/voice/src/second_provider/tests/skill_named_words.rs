//! A heard line that plainly asks for an installed skill by its name has
//! that skill's function called in code as round 1 (#6772, from Seventh's
//! photo-booth row on #6413, lamp-52e6 at 12587): the pushed `photo-booth`
//! skill was on and offered, and "photo booth" was answered "I'm Lamp — I
//! don't have a camera, so I can't take photos…" with `calls=none`. The
//! call goes the way a model's round-1 call goes: the Policy's hold ends
//! the turn with nothing run, and a look its answer asks for is taken. A
//! line that only mentions a camera, naming no skill, is still the model's.
use super::*;

/// The installed skills' functions, as their tools file names them.
const ON_THE_LAMP: [&str; 2] = ["photo_booth", "desk_check"];

/// What the turn's runner answers the skill's call with.
fn booth_told() -> Value {
    json!({"ran": "photo_booth", "next": "Say: smile, the photo is taken."})
}

/// One turn as it went: every call the runner ran, by name and with its
/// arguments, every note the turn's first chat was asked with, every tool
/// result the model was given, any round, how many chats were asked, and
/// the turn's looks asked (by whether one was dropped) and words said.
struct Went {
    ran: Vec<(String, Value)>,
    notes: Vec<String>,
    results: Vec<String>,
    chats: usize,
    looks: Vec<Option<String>>,
    said: Vec<String>,
}

/// `heard` asked, the model's rounds `rounds`, with the heard words read
/// for a skill's name over [`ON_THE_LAMP`], reached through `use_skill`;
/// the photo booth answers `booth`, and `more` adds to the engine.
fn went_with(
    heard: &str,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
    booth: Value,
    more: impl FnOnce(Engine) -> Engine,
) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let ran: Calls = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        let answer = if crate::skill_named::function_of(&name, &args) == "photo_booth" {
            booth.clone()
        } else {
            json!({"next": "Say: done."})
        };
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async move { answer })
    });
    let engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .calling_skills_named(Arc::new(|heard: &str| {
        crate::skill_named::named(heard, &ON_THE_LAMP)
            .map(|function| crate::skill_named::call(true, function))
    }));
    let mut engine = more(engine);
    let (looks, said) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            let mut looks = Vec::new();
            let mut said = Vec::new();
            loop {
                let event = tokio::time::timeout(Duration::from_secs(5), engine.next())
                    .await
                    .expect("the turn ends");
                match event {
                    ProviderOutput::LookAsked { dropped, .. } => looks.push(dropped),
                    ProviderOutput::Transcript {
                        request: Some(_),
                        text,
                        ..
                    } if !text.is_empty() => said.push(text),
                    ProviderOutput::TurnComplete { .. } | ProviderOutput::TurnFailed { .. } => {
                        break;
                    }
                    _ => {}
                }
            }
            (looks, said)
        });
    let asked = script.asked.lock().unwrap().clone();
    let content = |message: &Message| {
        message
            .content
            .as_ref()
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let notes = asked
        .first()
        .map(|round| {
            round
                .iter()
                .filter(|message| message.role == "system")
                .filter_map(content)
                .filter(|note| note.starts_with("(For you to know"))
                .collect()
        })
        .unwrap_or_default();
    let results = asked
        .iter()
        .flatten()
        .filter(|message| message.role == "tool")
        .filter_map(content)
        .collect();
    let ran = ran
        .lock()
        .unwrap()
        .iter()
        .map(|(name, args, _)| (name.clone(), args.clone()))
        .collect();
    Went {
        ran,
        notes,
        results,
        chats: asked.len(),
        looks,
        said,
    }
}

/// [`went_with`] with the booth's plain answer and the engine as it is.
fn went(heard: &str, rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    went_with(heard, rounds, booth_told(), |engine| engine)
}

fn booth_runs(went: &Went) -> usize {
    went.ran
        .iter()
        .filter(|(name, args)| crate::skill_named::function_of(name, args) == "photo_booth")
        .count()
}

fn said(line: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(line), Ok(ChatEvent::Done)]
}

fn calls(name: &str, args: Value) -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call("c1", name, args)])),
        Ok(ChatEvent::Done),
    ]
}

/// Seventh's line: the model would call nothing and say it has no camera.
/// The skill runs in code all the same, as round 1, through `use_skill`
/// with its function, and the one chat asked (round 2) is given its result
/// to word.
#[test]
fn photo_booth_by_name_runs_in_code_with_no_model_call() {
    for heard in [
        "photo booth",
        "Lamp, photo booth please.",
        "Run photo booth.",
    ] {
        let went = went(heard, vec![said("Smile, the photo is taken.")]);
        assert_eq!(booth_runs(&went), 1, "{heard}: {:?}", went.ran);
        assert_eq!(
            went.ran[0],
            (
                crate::use_skill::NAME.to_owned(),
                json!({"skill": "photo_booth"})
            ),
            "{heard}"
        );
        assert_eq!(went.chats, 1, "{heard}: round 1 asks no chat");
        assert!(
            went.notes
                .iter()
                .any(|note| note.contains("asked for photo booth by its name")),
            "{heard}: {:?}",
            went.notes
        );
        assert!(
            went.results
                .iter()
                .any(|told| told.contains("the photo is taken")),
            "{heard}: {:?}",
            went.results
        );
        assert_eq!(went.said, ["Smile, the photo is taken."], "{heard}");
    }
}

/// The model calls the skill itself after the words ran it: not run
/// twice, and it is given the first run's result.
#[test]
fn photo_booth_by_name_runs_once_when_the_model_calls_it_too() {
    let went = went(
        "photo booth",
        vec![
            calls(crate::use_skill::NAME, json!({"skill": "photo_booth"})),
            said("Smile, the photo is taken."),
        ],
    );
    assert_eq!(booth_runs(&went), 1, "{:?}", went.ran);
    assert_eq!(went.ran.len(), 1, "{:?}", went.ran);
    assert!(
        went.results
            .iter()
            .any(|told| told.contains("run for them already")),
        "{:?}",
        went.results
    );
}

/// Twelve's P1 1 and the Reviewer-lead's P2 on #6782: with the Policy
/// holding the turn (`LAMP_POLICY_HOLDS_ACTIONS`), a line that names a
/// skill runs nothing, no chat is asked, and the turn ends held, as a
/// model's held call ends it. With the Policy letting it run, it runs once.
#[test]
fn the_policys_hold_runs_nothing_for_a_skill_named() {
    let put: Arc<Mutex<Vec<String>>> = Arc::default();
    for held in [true, false] {
        let seen = put.clone();
        let went = went_with(
            "Lamp, photo booth please.",
            vec![said("Smile, the photo is taken.")],
            booth_told(),
            move |engine| {
                engine.holding_for_the_policy(Arc::new(move |_, name| {
                    seen.lock().unwrap().push(name);
                    Box::pin(async move { if held { Policed::Held } else { Policed::Run } })
                }))
            },
        );
        if held {
            assert!(went.ran.is_empty(), "{:?}", went.ran);
            assert_eq!(went.chats, 0, "held: no chat is asked");
            assert!(went.said.is_empty(), "{:?}", went.said);
        } else {
            assert_eq!(booth_runs(&went), 1, "{:?}", went.ran);
            assert_eq!(went.said, ["Smile, the photo is taken."]);
        }
    }
    assert_eq!(
        *put.lock().unwrap(),
        [crate::use_skill::NAME, crate::use_skill::NAME],
        "the call was put to the Policy each time"
    );
}

/// Twelve's P1 2 on #6782: a skill run by its name whose answer asks for a
/// look (`"look": true`, lamp-skills' `told` for a skill's `look` step)
/// has its look asked of the coordinator, as after a model's call, and the
/// lamp says it will look; no chat is asked before the picture.
#[test]
fn a_skill_named_whose_answer_asks_a_look_takes_it() {
    let went = went_with(
        "photo booth",
        vec![said("Nothing to see.")],
        json!({
            "next": "Say only \"Let me take a look.\" now, in those words. When the picture comes: say cheese.",
            "look": true
        }),
        |engine| engine.looking_for_skills(Arc::new(|name| name == crate::use_skill::NAME)),
    );
    assert_eq!(booth_runs(&went), 1, "{:?}", went.ran);
    assert_eq!(went.looks, [None], "the look is asked, not dropped");
    assert_eq!(went.said, [LOOKING]);
    assert_eq!(went.chats, 0, "the picture's turn words it");
}

/// A line that only mentions a camera names no skill: nothing runs in
/// code, the model is asked as before, and a call it makes runs as it
/// did.
#[test]
fn a_camera_line_naming_no_skill_goes_to_the_model() {
    let camera = went(
        "is there a camera in here?",
        vec![said("Yes, I can look with my eye.")],
    );
    assert!(camera.ran.is_empty(), "{:?}", camera.ran);
    assert!(
        !camera.notes.iter().any(|note| note.contains("by its name")),
        "{:?}",
        camera.notes
    );
    let question = went(
        "what does photo booth use?",
        vec![
            calls(crate::use_skill::NAME, json!({"skill": "photo_booth"})),
            said("It uses the camera and the light."),
        ],
    );
    assert_eq!(booth_runs(&question), 1, "{:?}", question.ran);
    assert!(
        !question
            .notes
            .iter()
            .any(|note| note.contains("by its name")),
        "{:?}",
        question.notes
    );
}

/// Twelve's P2 4 on #6782: "wait" beside the skill's name may be a person
/// stopping, so the line is the model's and nothing runs in code.
#[test]
fn a_wait_beside_a_skills_name_goes_to_the_model() {
    for heard in ["Photo booth, wait.", "wait, photo booth"] {
        let went = went(heard, vec![said("Sure, take your time.")]);
        assert!(went.ran.is_empty(), "{heard}: {:?}", went.ran);
        assert_eq!(went.chats, 1, "{heard}");
        assert!(
            !went.notes.iter().any(|note| note.contains("by its name")),
            "{heard}: {:?}",
            went.notes
        );
    }
}

/// Thirteen's line 1 on #6782's row (lamp-c0a4, 13:18): "Hey, Lamp.
/// Dance.", the speech to text's own full stop after the name, goes down
/// the name's wait (`LAMP_SECOND_NAME_CARRIES`, "only the lamp's name: its
/// answer waits … for the line after it", then "no line came after its
/// name: answered as a call"). The words that wait answers are read for a
/// skill's name all the same, so an installed skill that takes nothing
/// runs in code: in one line with the coordinator's `Called` or without
/// it, and said as the line after a bare "Hey, Lamp.". The lamp's own
/// `dance` is a core skill with a choice and a flag, so on a lamp it stays
/// the model's: here an installed `dance` taking nothing stands for any.
#[tokio::test(start_paused = true)]
async fn a_skill_named_on_the_names_wait_runs_in_code() {
    for (lines, called) in [
        (&["Hey, Lamp. Dance."][..], true),
        (&["Hey, Lamp. Dance."][..], false),
        (&["Hey, Lamp.", "Dance."][..], true),
        (&["Hey, Lamp.", "Dance."][..], false),
    ] {
        let script = Arc::new(Script {
            heard: Mutex::new(lines.iter().map(|line| Ok((*line).to_owned())).collect()),
            chats: Mutex::new(
                lines
                    .iter()
                    .map(|_| vec![text("Here I go."), Ok(ChatEvent::Done)])
                    .collect(),
            ),
            ..Script::default()
        });
        let (engine, ran, now) = scripted(script.clone());
        let mut engine = engine
            .announcing(false)
            .carrying_the_name(true)
            .calling_skills_named(Arc::new(|heard: &str| {
                crate::skill_named::named(heard, &["dance", "photo_booth"])
                    .map(|function| crate::skill_named::call(true, function))
            }));
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        if called {
            engine.submit(ProviderInput::Called {
                request: 1,
                privacy_generation: 1,
            });
        }
        engine.submit(end(1));
        if lines.len() > 1 {
            let after = Duration::from_millis(1_200);
            tokio::time::sleep(after).await;
            *now.lock().unwrap() += after;
            engine.submit(start(2));
            engine.submit(audio(2, 1));
            engine.submit(end(2));
        }
        let heard = turn_within(&mut engine, Duration::from_secs(5)).await;
        assert_eq!(
            heard.last().unwrap(),
            "complete",
            "{lines:?} {called}: {heard:?}"
        );
        let ran: Vec<(String, Value)> = ran
            .lock()
            .unwrap()
            .iter()
            .map(|(name, args, _)| (name.clone(), args.clone()))
            .collect();
        assert_eq!(
            ran,
            [(crate::use_skill::NAME.to_owned(), json!({"skill": "dance"}))],
            "{lines:?} {called}: run in code by its name"
        );
        let asked = script.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 1, "{lines:?} {called}: round 1 asks no chat");
        assert!(
            asked[0].iter().any(|message| message.role == "system"
                && message
                    .content
                    .as_ref()
                    .and_then(Value::as_str)
                    .is_some_and(|note| note.contains("asked for dance by its name"))),
            "{lines:?} {called}: {:?}",
            asked[0]
        );
    }
}

/// A skills folder of the test's own, with the pushed `photo-booth` and the
/// Store's `relaxing-music` in its tools file and their `asks` beside them,
/// as lamp-skills writes them from each `skill.toml`'s `asks` (#6772).
fn asks_folder() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let base = if std::path::Path::new("/private/tmp").is_dir() {
        std::path::PathBuf::from("/private/tmp")
    } else {
        std::env::temp_dir()
    };
    let dir = base.join(format!(
        "lvs-asks-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let tools = json!({
        "tools": [
            {"name": "photo_booth", "description": "Take a photo."},
            {"name": "play_relaxing_music", "description": "Play a quiet instrumental piece."}
        ],
        "asks": {
            "photo_booth": ["take a photo", "take a picture"],
            "play_relaxing_music": ["relaxing music", "play me something to relax"]
        }
    });
    std::fs::write(dir.join(crate::tools::CATALOG_FILE), tools.to_string()).unwrap();
    dir
}

/// `heard` asked as [`went`] asks it, with the heard words read over the
/// skills of [`asks_folder`], their names and their ask phrases, as the
/// voice reads them on the lamp (`skill_named::Heard`).
fn went_asking(heard: &str, rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    let dir = asks_folder();
    let reads = Arc::new(crate::skill_named::Heard::new(crate::tools::Settings {
        skills: Some(dir.clone()),
        use_skill: true,
        ..Default::default()
    }));
    let went = went_with(heard, rounds, booth_told(), move |engine| {
        engine.calling_skills_named(Arc::new(move |heard: &str| reads.heard(heard)))
    });
    std::fs::remove_dir_all(dir).ok();
    went
}

/// The first builder's ruling on #6059 (13 Nov 10:15, and V2's ask at
/// 12:5x): a skill's own ask phrases in skill.toml, matched whole and
/// called in code. Seventh's two lines on #6413 (lamp-52e6, 12587) call
/// the pushed `photo-booth`, and Sixth's two on #5749 (row-5749j) the
/// `relaxing-music` skill, each as round 1, with no model call.
#[test]
fn a_skills_own_ask_phrases_call_it_in_code() {
    for (heard, function) in [
        ("take a photo", "photo_booth"),
        ("photo booth", "photo_booth"),
        (
            "Hey Lamp, play me something to relax.",
            "play_relaxing_music",
        ),
        (
            "Lamp, can you play some relaxing music?",
            "play_relaxing_music",
        ),
    ] {
        let went = went_asking(heard, vec![said("Here you go.")]);
        assert_eq!(
            went.ran,
            [(
                crate::use_skill::NAME.to_owned(),
                json!({ "skill": function })
            )],
            "{heard}"
        );
        assert_eq!(went.chats, 1, "{heard}: round 1 asks no chat");
    }
}

/// A line that only asks about a camera is none of the skills' asks: the
/// model is asked, and nothing runs in code.
#[test]
fn a_camera_question_is_not_an_ask() {
    for heard in [
        "is there a camera?",
        "don't take a photo",
        "Take a photo, wait.",
    ] {
        let went = went_asking(heard, vec![said("Yes, I have one.")]);
        assert!(went.ran.is_empty(), "{heard}: {:?}", went.ran);
        assert_eq!(went.chats, 1, "{heard}");
    }
}

/// A skill's own ask said with the lamp's name, as the name's wait
/// (`LAMP_SECOND_NAME_CARRIES`) answers it, as for its name (Thirteen's
/// line 1 on #6782's row): in one line, the speech to text's full stop
/// after the name, with the coordinator's `Called` or without it, and as
/// the line after a bare "Hey, Lamp.". Each calls the pushed photo booth in
/// code as round 1, with no model call.
#[tokio::test(start_paused = true)]
async fn a_skills_ask_on_the_names_wait_calls_it_in_code() {
    for (lines, called) in [
        (&["Hey, Lamp. Take a photo."][..], true),
        (&["Hey, Lamp. Take a photo."][..], false),
        (&["Hey, Lamp.", "Take a photo."][..], true),
        (&["Hey, Lamp.", "Take a photo."][..], false),
    ] {
        let dir = asks_folder();
        let reads = Arc::new(crate::skill_named::Heard::new(crate::tools::Settings {
            skills: Some(dir.clone()),
            use_skill: true,
            ..Default::default()
        }));
        let script = Arc::new(Script {
            heard: Mutex::new(lines.iter().map(|line| Ok((*line).to_owned())).collect()),
            chats: Mutex::new(
                lines
                    .iter()
                    .map(|_| vec![text("Smile."), Ok(ChatEvent::Done)])
                    .collect(),
            ),
            ..Script::default()
        });
        let (engine, ran, now) = scripted(script.clone());
        let mut engine = engine
            .announcing(false)
            .carrying_the_name(true)
            .calling_skills_named(Arc::new(move |heard: &str| reads.heard(heard)));
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        if called {
            engine.submit(ProviderInput::Called {
                request: 1,
                privacy_generation: 1,
            });
        }
        engine.submit(end(1));
        if lines.len() > 1 {
            let after = Duration::from_millis(1_200);
            tokio::time::sleep(after).await;
            *now.lock().unwrap() += after;
            engine.submit(start(2));
            engine.submit(audio(2, 1));
            engine.submit(end(2));
        }
        let heard = turn_within(&mut engine, Duration::from_secs(5)).await;
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(
            heard.last().unwrap(),
            "complete",
            "{lines:?} {called}: {heard:?}"
        );
        let ran: Vec<(String, Value)> = ran
            .lock()
            .unwrap()
            .iter()
            .map(|(name, args, _)| (name.clone(), args.clone()))
            .collect();
        assert_eq!(
            ran,
            [(
                crate::use_skill::NAME.to_owned(),
                json!({"skill": "photo_booth"})
            )],
            "{lines:?} {called}: called in code by its ask"
        );
        assert_eq!(
            script.asked.lock().unwrap().len(),
            1,
            "{lines:?} {called}: round 1 asks no chat"
        );
    }
}
