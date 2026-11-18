//! `LAMP_LIMIT_LOCAL_ASKS` (#7346, done-when 4): at the usage limit, a
//! plain light or volume ask is done with no model and answered with the
//! acting part's own line; anything else is the limit as before.
use super::*;

/// A line at the limit that is no plain ask is journaled by its word count
/// alone: none of its words reach the journal.
#[test]
fn an_unmatched_line_is_journaled_by_its_word_count_only() {
    let line = limit_local_unmatched_line(7, "Hey Lamp, what's the weather like?");
    assert_eq!(
        line,
        "lamp-voice second: request 7: limit_local: no plain ask matched (6 words)"
    );
    assert!(!line.to_lowercase().contains("weather"));
}

/// What lamp-actions answers (`lamp_actions::Outcome::line_for`, with
/// `LAMP_SHORT_DONE`): a done step with its line, a refusal for the
/// cooler step.
fn told(name: &str) -> Value {
    let short = |say: &str| json!({"result": "done", "next": crate::second_provider::probe::tools::SHORT_NEXT, "say": say});
    match name {
        "light_brighter" => short("Brighter."),
        "volume_down" => short("Quieter."),
        "light_colour" => short("Done."),
        "light_cooler" => {
            json!({"error": "I asked for it but could not confirm my light changed."})
        }
        _ => json!({"ok": true}),
    }
}

/// An engine with the down line, the switch `on`, the given heard lines and
/// chats, in the order asked (a line done with no chat asks none), and its
/// tools taking `tool_takes` each; its lines rendered ahead.
async fn engine(
    on: bool,
    heard: &[&str],
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    tool_takes: Duration,
) -> (Engine, Arc<Script>, Calls) {
    engine_going_on(on, heard, chats, tool_takes, &[]).await
}

/// [`engine`], with the requests in `going_on` let out only as the
/// conversation going on (told `worker::CONVERSATION_ALONE`).
async fn engine_going_on(
    on: bool,
    heard: &[&str],
    chats: Vec<Vec<net::Result<ChatEvent>>>,
    tool_takes: Duration,
    going_on: &[u64],
) -> (Engine, Arc<Script>, Calls) {
    let going_on = going_on.to_vec();
    let script = Arc::new(Script {
        heard: Mutex::new(heard.iter().map(|words| Ok((*words).to_owned())).collect()),
        chats: Mutex::new(chats.into()),
        ..Script::default()
    });
    let ran: Calls = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        let answer = told(&name);
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async move {
            tokio::time::sleep(tool_takes).await;
            answer
        })
    });
    let engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .saying_when_down(true)
    .doing_at_the_limit(on)
    .noting_the_conversation_alone(Arc::new(move |request| {
        going_on
            .contains(&request)
            .then_some(worker::CONVERSATION_ALONE)
    }));
    engine.render_lines();
    tokio::time::timeout(Duration::from_secs(5), async {
        while engine
            .shared
            .down
            .as_ref()
            .unwrap()
            .limit
            .lock()
            .unwrap()
            .is_none()
            || (on && engine.shared.fixed_clips.kept("Brighter.").is_none())
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the lines are rendered ahead");
    (engine, script, ran)
}

fn answered() -> Vec<net::Result<ChatEvent>> {
    vec![text("It is noon."), Ok(ChatEvent::Done)]
}

fn limited() -> Vec<net::Result<ChatEvent>> {
    vec![Err(net::Error::UsageLimit(None))]
}

fn names(ran: &Calls) -> Vec<String> {
    ran.lock()
        .unwrap()
        .iter()
        .map(|(name, _, _)| name.clone())
        .collect()
}

/// At the limit: the first refused ask, plain, is done and answered with
/// its own line in place of the limit line; the next is done with no chat
/// asked at all. A line that is no plain ask hears the limit as before,
/// with no chat asked either (Heron on #7612), and an ask its function
/// refused hears it too, the function run once. [`LIMIT_TRY_AGAIN`] after
/// the last refusal, a line asks the chat again.
#[tokio::test]
async fn at_the_limit_a_plain_ask_is_done_without_the_model() {
    let (mut engine, script, ran) = engine(
        true,
        &[
            "What time is it?",
            "Make it brighter.",
            "Quieter, please.",
            "What time is it?",
            "Lamp, a bit cooler.",
            "Lamp, what's the weather?",
            "Make it brighter.",
        ],
        // From "Quieter, please." on, no chat is asked until a minute on.
        vec![answered(), limited(), answered(), answered()],
        Duration::ZERO,
    )
    .await;
    let brighter = format!("audio:{}", "Brighter.".len());
    let quieter = format!("audio:{}", "Quieter.".len());
    let limit = format!("audio:{}", DOWN_LINE_LIMIT.len());
    let quiet = ["down".to_owned(), "failed".to_owned()];
    // Only `clip`'s audio, then the quiet end: no limit line, no other.
    let only = |turn: &[String], clip: &str| {
        turn.ends_with(&quiet)
            && !turn[..turn.len() - 2].is_empty()
            && turn[..turn.len() - 2].iter().all(|event| event == clip)
    };

    sent_with_down(&mut engine, 1).await;
    let first = sent_with_down(&mut engine, 2).await;
    assert!(only(&first, &brighter), "{first:?}");
    assert_eq!(names(&ran), ["light_brighter"]);
    assert_eq!(script.asked.lock().unwrap().len(), 2, "its chat was asked");

    let second = sent_with_down(&mut engine, 3).await;
    assert!(only(&second, &quieter), "{second:?}");
    assert_eq!(names(&ran), ["light_brighter", "volume_down"]);
    assert_eq!(
        script.asked.lock().unwrap().len(),
        2,
        "already at the limit: no chat asked"
    );

    // No plain ask: the limit line, the first of the run, as before, and
    // no chat asked, as it would only be refused.
    let other = sent_with_down(&mut engine, 4).await;
    assert!(other.contains(&limit), "{other:?}");
    assert_eq!(names(&ran).len(), 2, "nothing run: {other:?}");
    assert_eq!(script.asked.lock().unwrap().len(), 2, "no chat: {other:?}");

    // Refused by its function: the limit's own way (here quiet, named
    // within 5 min), and the function run once, not again after the chat.
    let refused = sent_with_down(&mut engine, 5).await;
    assert_eq!(refused, quiet, "{refused:?}");
    assert_eq!(
        names(&ran),
        ["light_brighter", "volume_down", "light_cooler"]
    );

    assert_eq!(
        script.asked.lock().unwrap().len(),
        2,
        "no chat: {refused:?}"
    );

    // A minute on, a line asks the chat again; it answers, the limit is
    // over, and the model is asked.
    tokio::time::pause();
    tokio::time::advance(LIMIT_TRY_AGAIN).await;
    tokio::time::resume();
    let back = sent_with_down(&mut engine, 6).await;
    assert!(back.contains(&"complete".to_owned()), "{back:?}");
    let after = sent_with_down(&mut engine, 7).await;
    assert!(after.contains(&"complete".to_owned()), "{after:?}");
    assert_eq!(names(&ran).len(), 3, "asked of the model: {after:?}");
    assert_eq!(script.asked.lock().unwrap().len(), 4);
}

/// At the limit, a line let out only as the conversation going on is not
/// acted on without the model, which alone could judge it not for the
/// lamp (Heron on #7612: "Louder." from the TV): refused, it hears the
/// limit, and at the known limit nothing, with no chat asked. Named, the
/// same words are done.
#[tokio::test]
async fn at_the_limit_only_a_line_said_to_the_lamp_is_done() {
    let (mut engine, script, ran) = engine_going_on(
        true,
        &["What time is it?", "Louder.", "Louder.", "Lamp, louder."],
        vec![answered(), limited()],
        Duration::ZERO,
        &[2, 3, 4],
    )
    .await;
    sent_with_down(&mut engine, 1).await;
    let refused = sent_with_down(&mut engine, 2).await;
    assert!(
        refused.contains(&format!("audio:{}", DOWN_LINE_LIMIT.len())),
        "{refused:?}"
    );
    let known = sent_with_down(&mut engine, 3).await;
    assert_eq!(known, ["down", "failed"]);
    assert!(names(&ran).is_empty(), "not said to the lamp: nothing run");
    assert_eq!(
        script.asked.lock().unwrap().len(),
        2,
        "no chat at the limit"
    );
    let named = sent_with_down(&mut engine, 4).await;
    assert!(named.ends_with(&["down".to_owned(), "failed".to_owned()]));
    assert_eq!(names(&ran), ["volume_up"]);
    assert_eq!(script.asked.lock().unwrap().len(), 2);
}

/// With the switch off the limit is as it was: the plain ask hears the
/// limit line and nothing is run.
#[tokio::test]
async fn with_the_switch_off_the_limit_is_as_before() {
    let (mut engine, _, ran) = engine(
        false,
        &["What time is it?", "Make it brighter.", "Quieter, please."],
        vec![answered(), limited(), limited()],
        Duration::ZERO,
    )
    .await;
    sent_with_down(&mut engine, 1).await;
    let first = sent_with_down(&mut engine, 2).await;
    assert!(
        first.contains(&format!("audio:{}", DOWN_LINE_LIMIT.len())),
        "{first:?}"
    );
    let second = sent_with_down(&mut engine, 3).await;
    assert_eq!(second, ["down", "failed"]);
    assert!(names(&ran).is_empty());
}

/// Not at the limit (a 503, a plain answer): a plain ask is the model's,
/// whatever the switch.
#[tokio::test]
async fn only_at_the_limit() {
    let (mut engine, _, ran) = engine(
        true,
        &["Make it brighter.", "Make it brighter."],
        vec![vec![Err(net::Error::Status(503))], answered()],
        Duration::ZERO,
    )
    .await;
    let down = sent_with_down(&mut engine, 1).await;
    assert!(
        down.contains(&format!("audio:{}", DOWN_LINE.len())),
        "{down:?}"
    );
    sent_with_down(&mut engine, 2).await;
    assert!(names(&ran).is_empty());
}

/// The latency (#7346: within about 1 s of the line's end): from the line's
/// end to its line's first audio on the wall clock, with the speech to text
/// and the chat's refusal answering at once and lamp-actions taking 300 ms,
/// as a light step's confirm on a lamp may. What is left of the budget is
/// for the speech to text and, for the run's first ask only, the relay's
/// 429.
#[tokio::test]
async fn a_plain_ask_at_the_limit_is_answered_within_the_budget() {
    let (mut engine, _, _) = engine(
        true,
        &["What time is it?", "Make it brighter.", "Make it brighter."],
        vec![answered(), limited()],
        Duration::from_millis(300),
    )
    .await;
    sent_with_down(&mut engine, 1).await;
    for request in [2, 3] {
        engine.submit(start(request));
        engine.submit(audio(request, 1));
        let ended = std::time::Instant::now();
        engine.submit(end(request));
        let took = loop {
            if let ProviderOutput::Audio { .. } = engine.next().await {
                break ended.elapsed();
            }
        };
        assert!(
            took < Duration::from_millis(700),
            "request {request}: {} ms",
            took.as_millis()
        );
        while !matches!(engine.next().await, ProviderOutput::TurnFailed { .. }) {}
    }
}
