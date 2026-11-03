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
