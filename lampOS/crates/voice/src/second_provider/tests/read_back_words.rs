//! A heard line asking what a skill on the lamp uses, by its name, has the
//! skill read back in code before round 1 (V2 on #6413, Seventh's
//! row-6413c4 on lamp-52e6 at 12337): "what does desk check use?" was
//! answered by `use_skill` running the skill ("It just says done"), and
//! "Hey Lamp, what does desk check use?" by no call at all.
use super::*;

/// The made or pushed skills on the lamp, as written.
const ON_THE_LAMP: [&str; 1] = ["desk-check"];

/// What the turn's runner answers a read-back with, as the voice's own
/// `make_skill` does for a pushed skill's task waiting for its yes.
fn read_back_told() -> Value {
    json!({
        "skill": "desk-check",
        "task": true,
        "task_waits_for_yes": true,
        "approve_id": "t1",
        "next": "Say this whole: \"Desk check reads the desk's light.\" Then wait for their yes."
    })
}

/// One turn as it went: every call the runner ran, by name and with its
/// arguments, and every note round 1 was asked with.
struct Went {
    ran: Vec<(String, Value)>,
    notes: Vec<String>,
    /// Every tool result the model was given, any round.
    results: Vec<String>,
}

/// `heard` asked, the model's rounds `rounds`, with the heard words read
/// for a skill's read-back over [`ON_THE_LAMP`].
fn went(heard: &str, rounds: Vec<Vec<net::Result<ChatEvent>>>) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let ran: Calls = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        let answer = if name == crate::made_skill::MAKE {
            read_back_told()
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
    .reading_back_by_words(Arc::new(|heard: &str| {
        let names: Vec<String> = ON_THE_LAMP.iter().map(|name| (*name).to_owned()).collect();
        crate::make_skill_local::read_back_asked(heard, &names)
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

fn read_back_ran(went: &Went) -> bool {
    went.ran.iter().any(|(name, args)| {
        name == crate::made_skill::MAKE && args[crate::make_skill_local::READ_BACK] == "desk-check"
    })
}

fn use_skill_ran(went: &Went) -> bool {
    went.ran
        .iter()
        .any(|(name, _)| name == crate::use_skill::NAME)
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

/// use1: the model called `use_skill` for the skill. The read-back ran in
/// code first, with the skill's name; round 1 was told its result; the
/// model's `use_skill` of it was never run and was given the read-back.
#[test]
fn use1_reads_back_in_code_and_never_runs_the_skill() {
    let went = went(
        "what does desk check use?",
        vec![
            calls(crate::use_skill::NAME, json!({"skill": "desk_check"})),
            said("It reads the desk's light."),
        ],
    );
    assert!(read_back_ran(&went), "{:?}", went.ran);
    assert!(!use_skill_ran(&went), "{:?}", went.ran);
    assert_eq!(went.ran.len(), 1, "{:?}", went.ran);
    assert!(
        went.notes
            .iter()
            .any(|note| note.contains("uses, so it was read back")
                && note.contains("Desk check reads the desk's light.")),
        "{:?}",
        went.notes
    );
    assert!(
        went.results
            .iter()
            .any(|told| told.contains("read back already")),
        "{:?}",
        went.results
    );
}

/// use1b: with the lamp's name first, the model called nothing. The
/// read-back ran in code all the same, and round 1 was told what to say.
#[test]
fn use1b_reads_back_in_code_with_no_model_call() {
    let went = went(
        "Hey Lamp, what does desk check use?",
        vec![said("Desk check reads the desk's light.")],
    );
    assert!(read_back_ran(&went), "{:?}", went.ran);
    assert_eq!(went.ran.len(), 1, "{:?}", went.ran);
    assert!(
        went.notes
            .iter()
            .any(|note| note.contains("approve_id") && note.contains("t1")),
        "{:?}",
        went.notes
    );
}

/// A `make_skill` read-back of the same skill the model calls after it is
/// not run twice: no second wait for a yes.
#[test]
fn a_model_read_back_after_the_words_read_it_runs_once() {
    let went = went(
        "What can my desk check skill reach?",
        vec![
            calls(
                crate::made_skill::MAKE,
                json!({ crate::make_skill_local::READ_BACK: "desk check" }),
            ),
            said("Desk check reads the desk's light."),
        ],
    );
    assert_eq!(went.ran.len(), 1, "{:?}", went.ran);
    assert!(read_back_ran(&went), "{:?}", went.ran);
}

/// "use desk check", "run desk check" and "what time is it" read nothing
/// back: a `use_skill` the model calls runs as it did, and round 1 is told
/// nothing of a read-back. A line that asks and runs it runs it (the
/// Reviewer-lead's P1 on #6619): its `use_skill` is never replaced.
#[test]
fn asking_to_run_it_or_another_question_reads_nothing_back() {
    for heard in [
        "use desk check",
        "run desk check",
        "what does desk check use, and run it",
        "what does desk check need? ok run it",
        "what time is it? then run desk check, I need it",
    ] {
        let went = went(
            heard,
            vec![
                calls(crate::use_skill::NAME, json!({"skill": "desk-check"})),
                said("Done."),
            ],
        );
        assert!(!read_back_ran(&went), "{heard}: {:?}", went.ran);
        assert!(use_skill_ran(&went), "{heard}: {:?}", went.ran);
        assert!(
            !went.notes.iter().any(|note| note.contains("read back")),
            "{heard}: {:?}",
            went.notes
        );
    }
    let went = went("what time is it", vec![said("It's ten.")]);
    assert!(went.ran.is_empty(), "{:?}", went.ran);
    assert!(
        !went.notes.iter().any(|note| note.contains("read back")),
        "{:?}",
        went.notes
    );
}
