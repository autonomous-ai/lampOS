//! #7415 piece 6 on the second route: a final line that is a whole trigger of
//! a running app (`LAMP_APP_TRIGGERS`, [`crate::app_triggers`]) is offered to
//! that app, and when the app takes it the request ends there, in code: no
//! chat round, no words from the lamp, the app speaks for itself. A line the
//! app does not take, or no hook at all (the switch off, no app running),
//! goes to the model exactly as before.
use super::*;

/// One spoken line, `heard`, in request 2, and every message the model's chat
/// round was asked with, if it was asked at all. `take` is the line-taker the
/// worker would wire: `None` is the switch off.
fn went(heard: &str, take: Option<AppLine>) -> (Vec<Vec<Message>>, Vec<String>) {
    went_with(heard, take, None, false).0
}

/// As [`went`], the conversation's note for the line (`CONVERSATION_ALONE`,
/// `FOLLOW_UP_NOTE`) if any, and whether an earlier line's words were in
/// flight under its request; also whether anything of it is still in flight
/// when the turn ends.
fn went_with(
    heard: &str,
    take: Option<AppLine>,
    note: Option<&'static str>,
    in_flight: bool,
) -> ((Vec<Vec<Message>>, Vec<String>), bool) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(vec![vec![text("Sure, starting it."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(move |_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false);
    if let Some(take) = take {
        engine = engine.taking_app_lines(take);
    }
    if let Some(note) = note {
        engine = engine.noting_the_conversation_alone(Arc::new(move |_| Some(note)));
    }
    if in_flight {
        put_in_flight(
            &engine.shared,
            Carried {
                request: 2,
                parts: Vec::new(),
                at: Instant::now(),
                said_by: 2,
                unchecked: Vec::new(),
                memory_write_owner: WriteOwner::default(),
                rounds: Vec::new(),
                rounds_epoch: 0,
                earlier: Vec::new(),
            },
        );
    }
    let outputs = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for input in [start(2), audio(2, 1), end(2)] {
                engine.submit(input);
            }
            turn_within(&mut engine, Duration::from_secs(10)).await
        });
    let asked = script.asked.lock().unwrap().clone();
    let kept = engine.shared.in_flight.lock().unwrap().is_some();
    ((asked, outputs), kept)
}

/// A line-taker that takes `trigger` whole and records what it was offered.
fn taker(trigger: &'static str, offered: Arc<Mutex<Vec<(u64, String)>>>) -> AppLine {
    Arc::new(move |request, words: &str| {
        offered.lock().unwrap().push((request, words.to_owned()));
        crate::app_triggers::matched(
            &[lamp_msgs::AppsTriggers {
                app: "store-test".into(),
                triggers: vec![trigger.into()],
            }],
            words,
        )
        .is_some()
    })
}

#[test]
fn a_line_the_app_takes_ends_the_request_with_no_chat_round() {
    let offered: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let (asked, _) = went(
        "Start the store test.",
        Some(taker("start the store test", offered.clone())),
    );
    assert_eq!(
        *offered.lock().unwrap(),
        [(2, "Start the store test.".to_owned())],
        "offered once, in its request"
    );
    assert!(asked.is_empty(), "the model is not asked: {asked:?}");
}

#[test]
fn a_line_no_app_takes_goes_to_the_model_as_before() {
    let offered: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let (asked, _) = went(
        "What time is it?",
        Some(taker("start the store test", offered.clone())),
    );
    assert_eq!(offered.lock().unwrap().len(), 1, "offered, and not taken");
    assert!(!asked.is_empty(), "the model is asked");
}

#[test]
fn with_no_line_taker_a_trigger_goes_to_the_model_as_before() {
    let (asked, _) = went("Start the store test.", None);
    assert!(!asked.is_empty(), "the switch off: the model is asked");
}

#[test]
fn a_line_let_in_only_as_the_conversation_going_on_is_never_offered_to_an_app() {
    // Nobody said the name or turned to the lamp, and it is not the caller:
    // another person's or the TV's words must not start the owner's app.
    let offered: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let ((asked, _), _) = went_with(
        "Start the store test.",
        Some(taker("start the store test", offered.clone())),
        Some(worker::CONVERSATION_ALONE),
        false,
    );
    assert!(offered.lock().unwrap().is_empty(), "never offered");
    assert!(!asked.is_empty(), "the model judges it (not_for_me)");
}

#[test]
fn the_callers_own_follow_up_is_offered_to_an_app() {
    let offered: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let ((asked, _), _) = went_with(
        "Start the store test.",
        Some(taker("start the store test", offered.clone())),
        Some(worker::FOLLOW_UP_NOTE),
        false,
    );
    assert_eq!(offered.lock().unwrap().len(), 1);
    assert!(asked.is_empty(), "taken: the model is not asked");
}

#[test]
fn a_taken_line_ends_its_turn_with_nothing_of_it_left_in_flight() {
    // Words an earlier cut line joined to it would be answered by the next line.
    let offered = Arc::default();
    let (_, in_flight) = went_with(
        "Start the store test.",
        Some(taker("start the store test", offered)),
        None,
        true,
    );
    assert!(!in_flight, "its turn is over: nothing of it is carried");
}

/// The hook never runs while the lamp waits on an answer: a "yes go ahead"
/// trigger of an installed app must not take the person's yes to a task read
/// back (Heron's P2 on #7736). `began_clear` too: a line begun over the lamp's
/// own voice is never offered.
#[test]
fn a_line_begun_over_the_lamps_voice_is_never_offered_to_an_app() {
    let offered: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Start the store test.".to_owned())].into()),
        chats: Mutex::new(vec![vec![text("Sure."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(move |_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .taking_app_lines(taker("start the store test", offered.clone()));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut began = start(2);
            if let ProviderInput::Start {
                over_the_lamps_voice,
                ..
            } = &mut began
            {
                *over_the_lamps_voice = true;
            }
            for input in [began, audio(2, 1), end(2)] {
                engine.submit(input);
            }
            turn_within(&mut engine, Duration::from_secs(10)).await
        });
    assert!(offered.lock().unwrap().is_empty(), "never offered");
    assert!(
        !script.asked.lock().unwrap().is_empty(),
        "the model is asked"
    );
}

#[test]
fn a_line_is_not_offered_while_a_held_forget_or_a_task_yes_waits() {
    use crate::agent_tasks;
    // A task read back waits for its yes (the same clock the task-yes uses).
    agent_tasks::awaits_yes_here(Some(Instant::now()));
    let offered: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let ((asked, _), _) = went_with(
        "Start the store test.",
        Some(taker("start the store test", offered.clone())),
        None,
        false,
    );
    agent_tasks::awaits_yes_here(None);
    assert!(offered.lock().unwrap().is_empty(), "never offered");
    assert!(!asked.is_empty(), "the model answers the person's line");
    // And with nothing waiting it is offered, as before.
    let offered: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let (asked, _) = went(
        "Start the store test.",
        Some(taker("start the store test", offered.clone())),
    );
    assert_eq!(offered.lock().unwrap().len(), 1);
    assert!(asked.is_empty());
}

/// What the app was told: the request, the app and the trigger.
type Told3 = Vec<(u64, String, String)>;

/// `start_app` (the first builder at 19:49 on #7415): a line that is no bare
/// trigger is for the model, which is offered the running apps' triggers as
/// one function. A line, `heard`, where the model's first round calls
/// `start_app` for `english-practice`; the apps' side is `takes`. Gives the
/// rounds the model was asked, the outputs, and what the app was told.
fn went_starting_an_app(
    heard: &str,
    takes: bool,
    note: Option<&'static str>,
    awaiting_a_yes: bool,
) -> (Vec<Vec<Message>>, Vec<String>, Told3) {
    struct Apps {
        takes: bool,
        told: Arc<Mutex<Vec<(u64, String, String)>>>,
    }
    impl crate::app_triggers::Apps for Apps {
        fn running(&self) -> Vec<lamp_msgs::AppsTriggers> {
            vec![lamp_msgs::AppsTriggers {
                app: "english-practice".into(),
                triggers: vec!["start english practice".into()],
            }]
        }
        fn take(&self, request: u64, app: &str, trigger: &str) -> bool {
            self.told
                .lock()
                .unwrap()
                .push((request, app.to_owned(), trigger.to_owned()));
            self.takes
        }
    }
    let told: Arc<Mutex<Vec<(u64, String, String)>>> = Arc::default();
    let apps: Arc<dyn crate::app_triggers::Apps> = Arc::new(Apps {
        takes,
        told: told.clone(),
    });
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        crate::app_triggers::START_APP,
                        json!({"app": "english-practice", "trigger": "start english practice"}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Sorry, it did not start."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    // The worker's own answer to the call: checked against the apps running.
    let runner: ToolRunner = Arc::new(move |request, _, name, args| {
        let apps = apps.clone();
        Box::pin(async move {
            assert_eq!(name, crate::app_triggers::START_APP);
            crate::app_triggers::start_app_answer(&*apps, request, &args)
        })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        runner,
        Arc::new(Instant::now),
    )
    .announcing(false);
    if let Some(note) = note {
        engine = engine.noting_the_conversation_alone(Arc::new(move |_| Some(note)));
    }
    if awaiting_a_yes {
        crate::agent_tasks::awaits_yes_here(Some(Instant::now()));
    }
    let outputs = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for input in [start(2), audio(2, 1), end(2)] {
                engine.submit(input);
            }
            turn_within(&mut engine, Duration::from_secs(10)).await
        });
    crate::agent_tasks::awaits_yes_here(None);
    let asked = script.asked.lock().unwrap().clone();
    let told = told.lock().unwrap().clone();
    (asked, outputs, told)
}

/// "can you start english practice please" and a line in another language
/// are no fast path; the model, which judges meaning, calls `start_app`. The
/// app takes it: the request ends there, with no second round and no words
/// from the lamp.
#[test]
fn a_line_that_is_no_bare_trigger_reaches_the_app_through_start_app() {
    for heard in [
        "can you start english practice please",
        "Lamp ơi, bắt đầu tập nhé",
    ] {
        // Not the fast path: the bare-trigger matcher does not take it.
        assert_eq!(
            crate::app_triggers::matched(
                &[lamp_msgs::AppsTriggers {
                    app: "english-practice".into(),
                    triggers: vec!["start english practice".into()],
                }],
                heard
            ),
            None,
            "{heard}"
        );
        let (asked, outputs, told) = went_starting_an_app(heard, true, None, false);
        assert_eq!(
            asked.len(),
            1,
            "no second round after a taken call: {asked:?}"
        );
        assert_eq!(
            told,
            [(
                2,
                "english-practice".to_owned(),
                "start english practice".to_owned()
            )],
            "{heard}"
        );
        assert!(
            !outputs
                .iter()
                .any(|label| label.starts_with("audio") || label.starts_with("words")),
            "no words from the lamp: {outputs:?}"
        );
        assert!(
            outputs.iter().any(|label| label == "complete"),
            "the turn ends: {outputs:?}"
        );
    }
}

/// An app that does not take the call: the model is told so and answers as
/// it would, in a second round.
#[test]
fn a_start_app_the_app_does_not_take_goes_back_to_the_model() {
    let (asked, _, told) =
        went_starting_an_app("can you start english practice please", false, None, false);
    assert_eq!(told.len(), 1, "the app was told once");
    assert_eq!(
        asked.len(),
        2,
        "the model answers in a second round: {asked:?}"
    );
}

/// Heron's P2 on #7996: a line let in only as the conversation going on (no
/// name, not turned to the lamp, not the caller) is another person's or the
/// TV's: the model may call `start_app` for it, but nothing is sent to the
/// app, as the fast path refuses it. The same while the lamp waits on a yes.
#[test]
fn a_start_app_for_a_line_not_said_to_the_lamp_or_over_an_open_yes_starts_nothing() {
    let (asked, _, told) = went_starting_an_app(
        "can you start english practice please",
        true,
        Some(worker::CONVERSATION_ALONE),
        false,
    );
    assert!(told.is_empty(), "the TV's line started the app: {told:?}");
    assert_eq!(asked.len(), 2, "the model was told and answers: {asked:?}");
    let (asked, _, told) =
        went_starting_an_app("can you start english practice please", true, None, true);
    assert!(told.is_empty(), "an open yes started the app: {told:?}");
    assert_eq!(asked.len(), 2, "{asked:?}");
}
