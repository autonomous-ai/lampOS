//! An Apple Health read on the second route (the first builder's P2s on
//! #7900): the lamp's fixed words (`services::HEALTH_SAY`) are said as they
//! are, the chat model is told only that they were said, and the thread and
//! session records keep [`crate::services::HEALTH_SHOWN`], never a value.
use super::*;

const LINE: &str = "Apple Health, as of 7:30 am: you slept 6 hours and 52 minutes last night.";
const ROUND_TWO: &str = "And the light is warmer.";

fn health_told(name: &str) -> Value {
    match name {
        crate::services::READ => json!({"ok": true, crate::services::HEALTH_SAY: LINE}),
        _ => json!({"ok": true}),
    }
}

/// One turn on "How did I sleep?" whose round 1 calls `names`, each told
/// [`health_told`]: its labels, the chats asked, and what the voice said.
fn health_turn(names: &[&str]) -> (Vec<String>, Vec<Vec<Message>>, Vec<String>) {
    let calls = names
        .iter()
        .enumerate()
        .map(|(at, name)| {
            let args = if *name == crate::services::READ {
                json!({"service": "apple_health", "what": "sleep"})
            } else {
                json!({})
            };
            call(&format!("c{at}"), name, args)
        })
        .collect();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("How did I sleep last night?".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![Ok(ChatEvent::ToolCalls(calls)), Ok(ChatEvent::Done)],
                vec![text(ROUND_TWO), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        let answer = health_told(&name);
        Box::pin(async move { answer })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false);
    let labels = tokio::runtime::Builder::new_current_thread()
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
    let spoken = script.spoken.lock().unwrap().clone();
    (labels, asked, spoken)
}

/// A read alone: its fixed words go to the voice as they are, the
/// transcript says only [`crate::services::HEALTH_SHOWN`], and no round 2
/// is asked, so no model ever sees a value.
#[test]
fn a_health_read_is_said_in_fixed_words_with_no_model_round() {
    let (labels, asked, spoken) = health_turn(&[crate::services::READ]);
    assert_eq!(spoken, vec![LINE.to_owned()], "{labels:?}");
    let said: Vec<&str> = labels
        .iter()
        .filter_map(|label| label.strip_prefix("said:"))
        .map(str::trim)
        .collect();
    assert_eq!(said, vec![crate::services::HEALTH_SHOWN], "{labels:?}");
    assert!(
        !labels.iter().any(|label| label.contains("6 hours")),
        "{labels:?}"
    );
    assert_eq!(asked.len(), 1, "no round 2: {asked:?}");
    assert_eq!(labels.last().map(String::as_str), Some("complete"));
}

/// Beside another call, round 2 is asked for the rest, and is told only
/// that the health answer was said, never its words.
#[test]
fn beside_another_call_round_two_is_never_told_the_values() {
    let (labels, asked, spoken) = health_turn(&[crate::services::READ, "set_light"]);
    assert_eq!(spoken.first().map(String::as_str), Some(LINE), "{labels:?}");
    assert_eq!(asked.len(), 2, "{labels:?}");
    let second = format!("{:?}", asked[1]);
    assert!(!second.contains("6 hours"), "{second}");
    assert!(second.contains("[an Apple Health read]"), "{second}");
    assert!(
        !labels.iter().any(|label| label.contains("6 hours")),
        "{labels:?}"
    );
}
