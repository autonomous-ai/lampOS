//! A heard line that plainly asks for an installed skill by its name has
//! that skill's function run in code before round 1 (#6772, from Seventh's
//! photo-booth row on #6413, lamp-52e6 at 12587): the pushed `photo-booth`
//! skill was on and offered, and "photo booth" was answered "I'm Lamp — I
//! don't have a camera, so I can't take photos…" with `calls=none`. A line
//! that only mentions a camera, naming no skill, is still the model's.
use super::*;

/// The installed skills' functions, as their tools file names them.
const ON_THE_LAMP: [&str; 2] = ["photo_booth", "desk_check"];

/// What the turn's runner answers the skill's call with.
fn booth_told() -> Value {
    json!({"ran": "photo_booth", "next": "Say: smile, the photo is taken."})
}

/// One turn as it went: every call the runner ran, by name and with its
/// arguments, every note round 1 was asked with, and every tool result the
/// model was given, any round.
struct Went {
    ran: Vec<(String, Value)>,
    notes: Vec<String>,
    results: Vec<String>,
}

/// `heard` asked, the model's rounds `rounds`, with the heard words read
/// for a skill's name over [`ON_THE_LAMP`], reached through `use_skill`.
fn went(heard: &str, rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let ran: Calls = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        let answer = if crate::skill_named::function_of(&name, &args) == "photo_booth" {
            booth_told()
        } else {
            json!({"next": "Say: done."})
        };
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
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
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            turn(&mut engine).await
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
    }
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
/// The skill runs in code all the same, through `use_skill` with its
/// function, and round 1 is told its result to word.
#[test]
fn photo_booth_by_name_runs_in_code_with_no_model_call() {
    for heard in [
        "photo booth",
        "Lamp, photo booth please.",
        "Run photo booth.",
    ] {
        let went = went(
            heard,
            vec![said(
                "I'm Lamp — I don't have a camera, so I can't take photos.",
            )],
        );
        assert_eq!(booth_runs(&went), 1, "{heard}: {:?}", went.ran);
        assert_eq!(
            went.ran[0],
            (
                crate::use_skill::NAME.to_owned(),
                json!({"skill": "photo_booth"})
            ),
            "{heard}"
        );
        assert!(
            went.notes
                .iter()
                .any(|note| note.contains("asked for photo booth by its name")
                    && note.contains("the photo is taken")),
            "{heard}: {:?}",
            went.notes
        );
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
