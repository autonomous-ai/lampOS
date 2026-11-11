//! `LAMP_HISTORY_KEEPS_CALLS` on the second route: the functions a turn ran
//! are handed to the thread, so a later turn is told them with its words
//! (`worker::messages_told`). Only calls that ran and were not refused: a
//! decline, a call answered with an error or a keep not kept is not handed
//! on, and with the switch off nothing is.
use super::*;

/// What each call is told here: a light or volume change done, a failure,
/// a keep refused.
fn told(name: &str) -> Value {
    match name {
        "light_warmer" => {
            json!({"error": "I asked for it but could not confirm my light changed."})
        }
        "remember" => json!({"remembered": false, "note": "Not kept."}),
        // A write prepared: its read-back, the person's words in it.
        "service_write" => json!({
            "read_back": "Send an email from me@home.test to ann@b.test. Subject: \"Hi\". It says: \"Home by 7\". Shall I send it?",
            "send_id": "v17-1",
            "sent": false
        }),
        _ => json!({"ok": true}),
    }
}

/// One turn, `heard` asked with `rounds`, the calls handed on with the
/// switch `on`: what was handed on, as (request, name).
fn handed_on(
    on: bool,
    heard: &str,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> Vec<(u64, String)> {
    handed_on_with(on, heard, rounds)
        .into_iter()
        .map(|(request, name, _)| (request, name))
        .collect()
}

/// [`handed_on`], each with the arguments it is told with.
fn handed_on_with(
    on: bool,
    heard: &str,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> Vec<(u64, String, Value)> {
    handed_on_told(on, heard, rounds)
        .into_iter()
        .map(|(request, name, args, _)| (request, name, args))
        .collect()
}

/// [`handed_on_with`], each with what its result is told as
/// (`kept_result`).
fn handed_on_told(
    on: bool,
    heard: &str,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> Vec<(u64, String, Value, Option<&'static str>)> {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        let answer = told(&name);
        Box::pin(async move { answer })
    });
    type Kept = Vec<(u64, String, Value, Option<&'static str>)>;
    let kept: Arc<Mutex<Kept>> = Arc::default();
    let keep = kept.clone();
    let mut engine = Engine::with_clock(
        script,
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .keeping_calls_by(on.then(|| -> CallsKept {
        Arc::new(move |request, name: &str, args: &Value, result| {
            keep.lock()
                .unwrap()
                .push((request, name.to_owned(), args.clone(), result));
        })
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
    kept.lock().unwrap().clone()
}

fn calling(names: &[&str]) -> Vec<net::Result<ChatEvent>> {
    let calls = names
        .iter()
        .enumerate()
        .map(|(at, name)| call(&format!("c{at}"), name, json!({})))
        .collect();
    vec![Ok(ChatEvent::ToolCalls(calls)), Ok(ChatEvent::Done)]
}

fn saying(words: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(words), Ok(ChatEvent::Done)]
}

/// Lamp build 9699: "Lamp, you're a bit loud." The call that ran is handed
/// on with its request; off, nothing is.
#[test]
fn a_call_that_ran_is_handed_to_the_thread() {
    let rounds = || {
        vec![
            calling(&["volume_down"]),
            saying("Turning myself down a step."),
        ]
    };
    assert_eq!(
        handed_on(true, "Lamp, you're a bit loud.", rounds()),
        [(1, "volume_down".to_owned())]
    );
    assert!(handed_on(false, "Lamp, you're a bit loud.", rounds()).is_empty());
}

/// A call that failed, or a keep refused, ran but did nothing: never told
/// later as done. The one beside them that did is.
#[test]
fn a_call_that_did_nothing_is_not_handed_on() {
    assert_eq!(
        handed_on(
            true,
            "Warmer, and remember I like it warm.",
            vec![
                calling(&["light_warmer", "remember", "light_dimmer"]),
                saying("Done."),
            ],
        ),
        [(1, "light_dimmer".to_owned())]
    );
}

/// The decline is never run, so never handed on.
#[test]
fn a_decline_is_not_handed_on() {
    assert!(handed_on(true, "and then she said no", vec![calling(&[NOT_FOR_ME])]).is_empty());
}

/// #5045(a), lamp-52e6 at 9730: "Make the light red." answered "Done."
/// with no call, after earlier colour turns were told as words alone. A
/// light or volume call is handed on with its own arguments, so a later
/// turn is told the colour it set; the step beside it, which takes none,
/// with none.
#[test]
fn a_light_call_is_handed_on_with_its_arguments() {
    let rounds = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![
                call("c0", "light_colour", json!({"colour": "red"})),
                call("c1", "light_dimmer", json!({})),
            ])),
            Ok(ChatEvent::Done),
        ],
        saying("Red, and dimmer."),
    ];
    assert_eq!(
        handed_on_with(true, "Make it red and dimmer.", rounds),
        [
            (1, "light_colour".to_owned(), json!({"colour": "red"})),
            (1, "light_dimmer".to_owned(), json!({})),
        ]
    );
}

/// Any other function's call made with arguments, neither the light's,
/// the volume's nor the Schedule's, is never handed on: a keep's are the
/// person's own words, never told again as a call, and told bare they
/// would teach calling it bare (Fourteen's P3 on #5018). The step beside
/// it is.
#[test]
fn another_call_with_arguments_is_not_handed_on() {
    let rounds = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![
                call("c0", "remember", json!({"fact": "They like it warm."})),
                call("c1", "use_skill", json!({"name": "tea", "ask": "five"})),
                call("c2", "light_warmer", json!({})),
                call("c3", "light_dimmer", json!({})),
            ])),
            Ok(ChatEvent::Done),
        ],
        saying("Done."),
    ];
    assert_eq!(
        handed_on_with(
            true,
            "Remember I like it warm, the tea skill for five, dimmer.",
            rounds
        ),
        [(1, "light_dimmer".to_owned(), json!({}))]
    );
}

/// lamp-52e6 at 9928 (4 Nov 09:30): "set a timer for one minute" called
/// `set_timer`; five minutes later "Lamp, set a timer to one minute." made
/// no call and said "Timer set. It rings at 9:39 am.", the earlier reply
/// copied with a new time, and no timer existed: the earlier call, made
/// with arguments, was never kept, so the history showed a timer set in
/// words alone. A Schedule call is handed on with its own arguments, as a
/// light's is (#5045(a)); one past `thread::KEPT_ARGS_BYTES` is not kept,
/// since every Schedule function but `list_reminders` takes some, and
/// `list_reminders` with none is kept bare as before.
#[test]
fn a_schedule_call_is_handed_on_with_its_arguments() {
    let long = "x".repeat(crate::thread::KEPT_ARGS_BYTES);
    let rounds = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![
                call("c0", "set_timer", json!({"text": "Timer", "seconds": 60})),
                call("c1", "set_reminder", json!({"text": long, "time": "09:00"})),
                call("c2", "list_reminders", json!({})),
                call("c3", "cancel_reminder", json!({"text": "tea"})),
            ])),
            Ok(ChatEvent::Done),
        ],
        saying("Timer set. It rings at 9:34 am."),
    ];
    assert_eq!(
        handed_on_with(true, "set a timer for one minute", rounds),
        [
            (
                1,
                "set_timer".to_owned(),
                json!({"text": "Timer", "seconds": 60})
            ),
            (1, "list_reminders".to_owned(), json!({})),
            (1, "cancel_reminder".to_owned(), json!({"text": "tea"})),
        ]
    );
}

/// A light call whose arguments pass `thread::KEPT_ARGS_BYTES` as compact
/// JSON is never kept with them: one whose function takes none is kept
/// bare, one whose function takes some (`light_colour`) is not kept, as
/// told bare it would teach calling it bare. At the bound, it is kept
/// whole.
#[test]
fn a_light_call_past_the_arguments_bound_is_kept_bare_only_if_it_takes_none() {
    let over = |bytes: usize| {
        let padding = "x".repeat(bytes - r#"{"colour":""}"#.len());
        let args = json!({ "colour": padding });
        assert_eq!(args.to_string().len(), bytes);
        args
    };
    let at = over(crate::thread::KEPT_ARGS_BYTES);
    let past = over(crate::thread::KEPT_ARGS_BYTES + 1);
    let rounds = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![
                call("c0", "light_colour", past.clone()),
                call("c1", "light_brighter", past),
                call("c2", "volume_up", at.clone()),
            ])),
            Ok(ChatEvent::Done),
        ],
        saying("Done."),
    ];
    assert_eq!(
        handed_on_with(true, "Brighter and louder.", rounds),
        [
            (1, "light_brighter".to_owned(), json!({})),
            (1, "volume_up".to_owned(), at),
        ]
    );
}

/// An earlier turn as `worker::messages_told` gives it with one kept call.
fn kept_history() -> Told {
    let kept = net::ToolCall {
        id: "kept-1-0".into(),
        name: "volume_down".into(),
        args: json!({}),
        extra: None,
    };
    Told {
        text: "SYSTEM".into(),
        after: None,
        history: vec![
            Message::user("Lamp, you're a bit loud."),
            Message::assistant_calls(None, std::slice::from_ref(&kept)),
            Message::tool("kept-1-0", "(done earlier; its result was not kept)"),
            Message::assistant("Quieter."),
        ],
    }
}

/// The Reviewer-lead's P2 on #5018: a chat that refuses round 1 (a 400,
/// say a call given back with no thought signature) with kept calls in the
/// history is asked once more without them, and its answer is said; the
/// earlier turn's words stay. With no kept calls, the same 400 ends the
/// turn as before, asked once.
#[tokio::test]
async fn a_chat_that_refuses_kept_calls_is_asked_again_without_them() {
    for kept in [true, false] {
        let script = Arc::new(Script {
            heard: Mutex::new([Ok("Lamp, quieter again.".into())].into()),
            chats: Mutex::new(
                [
                    vec![Err(net::Error::Status(400))],
                    vec![text("Sure."), Ok(ChatEvent::Done)],
                ]
                .into(),
            ),
            ..Script::default()
        });
        let system: System = if kept {
            Arc::new(kept_history)
        } else {
            Arc::new(|| "SYSTEM".to_owned().into())
        };
        let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
        let mut engine = Engine::with_clock(script.clone(), system, tools, Arc::new(Instant::now))
            .announcing(false);
        engine.submit(start(1));
        engine.submit(audio(1, 1));
        engine.submit(end(1));
        let labels = turn(&mut engine).await;
        let asked = script.asked.lock().unwrap().clone();
        if kept {
            assert_eq!(asked.len(), 2, "{labels:?}");
            assert!(
                labels.iter().any(|label| label == "said:Sure."),
                "{labels:?}"
            );
            let first = format!("{:?}", asked[0]);
            let again = format!("{:?}", asked[1]);
            assert!(first.contains("kept-1-0"), "{first}");
            assert!(!again.contains("kept-1-0"), "{again}");
            assert!(again.contains("Quieter."), "{again}");
            assert_eq!(
                asked[1].last(),
                asked[0].last(),
                "the line asked stays last"
            );
            assert_eq!(asked[1].len() + 2, asked[0].len());
        } else {
            assert_eq!(asked.len(), 1, "{labels:?}");
            assert!(
                !labels.iter().any(|label| label == "said:Sure."),
                "{labels:?}"
            );
        }
    }
}

/// lamp-06c1 at 11913 (11 Nov): create, move, cancel, then the same again
/// in one conversation. Each first write called `service_write`; each
/// second was read back by the model itself with no call, the history
/// showing every earlier write as words alone. A prepared write is handed
/// on with only its service, action and send id, its result its status:
/// never a recipient, a subject, a body or the person's "said".
#[test]
fn a_service_write_is_handed_on_with_no_ones_words() {
    let asked = json!({
        "service": "gmail",
        "action": "send_mail",
        "to": ["ann@b.test"],
        "subject": "Hi",
        "text": "Home by 7",
        "said": "send a mail to Ann",
        "event_id": "7ckf9",
    });
    let rounds = vec![
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c0",
                "service_write",
                asked,
            )])),
            Ok(ChatEvent::Done),
        ],
        saying("Send an email from me@home.test to ann@b.test. Shall I send it?"),
    ];
    let handed = handed_on_told(true, "Send a mail to Ann saying home by 7.", rounds);
    assert_eq!(
        handed,
        [(
            1,
            "service_write".to_owned(),
            json!({"service": "gmail", "action": "send_mail"}),
            Some(WRITE_PREPARED),
        )]
    );
    let told = handed[0].2.to_string();
    for private in ["ann@b.test", "Hi", "Home by 7", "Ann", "7ckf9"] {
        assert!(!told.contains(private), "{private} kept in {told}");
    }
}

/// A service write's kept arguments ([`kept_args`]) and its status
/// ([`kept_result`]): a send's yes keeps its send id, a field that is not a
/// string is left out, and a call that is not a write keeps its plain
/// result.
#[test]
fn a_service_writes_status_and_fields_are_kept() {
    assert_eq!(
        kept_args(
            "service_write",
            &json!({"service": "google_calendar", "send_id": "v1-2", "said": "yes", "event": {"title": "x"}}),
        ),
        Some(json!({"service": "google_calendar", "send_id": "v1-2"}))
    );
    assert_eq!(
        kept_args(
            "service_write",
            &json!({"service": "x".repeat(crate::thread::KEPT_ARGS_BYTES)})
        ),
        None
    );
    assert_eq!(
        kept_result(
            "service_write",
            &json!({"read_back": "Add it?", "send_id": "v1"})
        ),
        Some(WRITE_PREPARED)
    );
    for done in [json!({"sent": true}), json!({"sent": false, "saved": true})] {
        assert_eq!(
            kept_result("service_write", &done),
            Some(WRITE_DONE),
            "{done}"
        );
    }
    assert_eq!(
        kept_result("service_write", &json!({"sent": false, "cancelled": true})),
        Some(WRITE_NOT_DONE)
    );
    assert_eq!(kept_result("set_timer", &json!({"ok": true})), None);
    for line in [WRITE_PREPARED, WRITE_DONE, WRITE_NOT_DONE] {
        assert!(line.starts_with('(') && line.ends_with(')'), "{line}");
    }
}
