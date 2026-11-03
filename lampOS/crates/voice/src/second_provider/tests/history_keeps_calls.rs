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
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        let answer = told(&name);
        Box::pin(async move { answer })
    });
    let kept: Arc<Mutex<Vec<(u64, String)>>> = Arc::default();
    let keep = kept.clone();
    let mut engine = Engine::with_clock(
        script,
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .keeping_calls_by(on.then(|| -> CallsKept {
        Arc::new(move |request, name: &str| {
            keep.lock().unwrap().push((request, name.to_owned()));
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

/// A call made with arguments is never handed on: a kept call is told with
/// none, and would teach calling it bare (Fourteen's P3 on #5018). The
/// step beside it, which takes none, is.
#[test]
fn a_call_with_arguments_is_not_handed_on() {
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
        handed_on(true, "Make it red and dimmer.", rounds),
        [(1, "light_dimmer".to_owned())]
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
