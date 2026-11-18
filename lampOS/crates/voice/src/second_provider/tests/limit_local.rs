//! `LAMP_LIMIT_LOCAL_ASKS` (#7346, done-when 4): at the usage limit, a
//! plain light or volume ask is done with no model and answered with the
//! acting part's own line, and "what time is it?" is said from the lamp's
//! clock (#7642); anything else is the limit as before.
use super::*;
use crate::second_provider::limit_local as pieces;

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
    // The time's pieces are rendered last ([`pieces::time_pieces`]).
    let last_piece = pieces::time_pieces().last().unwrap();
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
            || (on
                && ["Brighter.", LIMIT_TIMER_DONE, LIMIT_REMINDER_DUE, last_piece]
                    .iter()
                    .any(|line| engine.shared.fixed_clips.kept(line).is_none()))
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
            "Tell me a joke.",
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

/// #7642: only the service's own usage-limit word keys the local asks, with
/// no two-minute guess. Plain 429s that last [`LIMIT_AFTER`] still say the
/// limit line (#7346), but a plain ask then is not done without the model,
/// and the next line asks the chat.
#[tokio::test]
async fn plain_429s_that_last_do_not_key_the_local_asks() {
    let plain = || vec![Err(net::Error::Status(429))];
    let (mut engine, script, ran) = engine(
        true,
        &[
            "What time is it?",
            "Make it brighter.",
            "Make it brighter.",
            "Make it brighter.",
        ],
        vec![answered(), plain(), plain(), answered()],
        Duration::ZERO,
    )
    .await;
    sent_with_down(&mut engine, 1).await;
    sent_with_down(&mut engine, 2).await;
    tokio::time::pause();
    tokio::time::advance(LIMIT_AFTER + Duration::from_secs(10)).await;
    tokio::time::resume();
    let lasted = sent_with_down(&mut engine, 3).await;
    assert!(
        lasted.contains(&format!("audio:{}", DOWN_LINE_LIMIT.len())),
        "429s past the bound still say the limit: {lasted:?}"
    );
    assert!(names(&ran).is_empty(), "nothing run: {lasted:?}");
    let next = sent_with_down(&mut engine, 4).await;
    assert!(next.contains(&"complete".to_owned()), "{next:?}");
    assert!(names(&ran).is_empty(), "the model's: {next:?}");
    assert_eq!(script.asked.lock().unwrap().len(), 4, "its chat was asked");
}

/// The audio a request's turn played, whole, and how it ended.
async fn played(engine: &mut Engine, request: u64) -> (Vec<i16>, Vec<String>) {
    engine.submit(start(request));
    engine.submit(audio(request, 1));
    engine.submit(end(request));
    let (mut samples, mut events) = (Vec::new(), Vec::new());
    loop {
        match engine.next().await {
            ProviderOutput::Audio { samples: part, .. } => samples.extend(part),
            ProviderOutput::DownLineSaid { request: of } if of == request => {
                events.push("down".to_owned());
            }
            ProviderOutput::TurnFailed { .. } => {
                events.push("failed".to_owned());
                return (samples, events);
            }
            ProviderOutput::TurnComplete { .. } => {
                events.push("complete".to_owned());
                return (samples, events);
            }
            _ => {}
        }
    }
}

/// The clip the time `(hour, minute)` is said in, from its pieces as the
/// fake voice renders them (1,500 samples of each piece's length).
fn time_clip((hour, minute): (i64, i64)) -> Vec<i16> {
    pieces::time_said(hour, minute)
        .into_iter()
        .flat_map(|piece| vec![piece.len() as i16; 1_500])
        .collect()
}

/// #7642: at the usage limit, "what time is it?" is answered from the
/// lamp's own clock, said from pieces rendered at start (the relay renders
/// no new speech at the limit, #7412), with no chat asked and no function
/// run; the run's first refused ask too, in place of the limit line.
#[tokio::test]
async fn at_the_limit_the_time_is_said_from_its_pieces() {
    let (mut engine, script, ran) = engine(
        true,
        &[
            "Make it brighter.",
            "What time is it?",
            "Lamp, what's the time?",
        ],
        vec![answered(), limited()],
        Duration::ZERO,
    )
    .await;
    sent_with_down(&mut engine, 1).await;
    for request in [2, 3] {
        let before = crate::local_time::clock_here().expect("the host has a clock");
        let (samples, events) = played(&mut engine, request).await;
        let after = crate::local_time::clock_here().unwrap();
        assert_eq!(events, ["down", "failed"], "request {request}");
        assert!(
            samples == time_clip(before) || samples == time_clip(after),
            "request {request}: {} samples, not the time",
            samples.len()
        );
    }
    assert!(names(&ran).is_empty(), "no function for the time");
    assert_eq!(script.asked.lock().unwrap().len(), 2, "no chat at the limit");
}

/// The time's pieces: the hour with its lead, the minutes as a clock is
/// read, and a.m. or p.m.; every one of them rendered at start.
#[test]
fn the_time_is_said_from_pieces_rendered_at_start() {
    let cases: &[((i64, i64), &[&str])] = &[
        ((0, 0), &["It's twelve", "a.m."]),
        ((7, 5), &["It's seven", "oh", "five", "a.m."]),
        ((12, 15), &["It's twelve", "fifteen", "p.m."]),
        ((19, 52), &["It's seven", "fifty", "two", "p.m."]),
        ((23, 40), &["It's eleven", "forty", "p.m."]),
    ];
    for ((hour, minute), want) in cases {
        assert_eq!(&pieces::time_said(*hour, *minute), want);
    }
    let rendered: Vec<&str> = pieces::time_pieces().collect();
    for hour in 0..24 {
        for minute in 0..60 {
            for piece in pieces::time_said(hour, minute) {
                assert!(rendered.contains(&piece), "{hour}:{minute}: {piece}");
            }
        }
    }
}

/// A due timer (`timer`) or reminder, said by this route.
fn due(request: u64, timer: bool) -> ProviderInput {
    ProviderInput::Reminder {
        request,
        session: 1,
        privacy_generation: 1,
        text: "Tea".into(),
        late: false,
        late_by_ms: None,
        also: Vec::new(),
        again: false,
        ids: Vec::new(),
        timer,
    }
}

/// #7642 (V2's ruling A): at the known limit a due timer is rung with
/// "Your timer is done." and a reminder with "You have a reminder.", from
/// their clips, with no chat asked, and each turn ends answered with that
/// audio, so the coordinator completes it as said (no ring again, and no
/// attempt lost to the no-audio retries); told as the lamp's own words, so
/// it ends no refusal.
#[tokio::test]
async fn at_the_known_limit_a_due_timer_or_reminder_is_rung_with_its_own_line() {
    let (engine, script, _) = engine(
        true,
        &["Lamp, what's the weather?"],
        vec![limited()],
        Duration::ZERO,
    )
    .await;
    let mut engine = engine.saying_reminders(true);
    let refused = sent_with_down(&mut engine, 1).await;
    assert!(
        refused.contains(&format!("audio:{}", DOWN_LINE_LIMIT.len())),
        "{refused:?}"
    );
    for (request, timer, line) in [
        (2, true, LIMIT_TIMER_DONE),
        (3, false, LIMIT_REMINDER_DUE),
    ] {
        engine.submit(due(request, timer));
        let rung = turn(&mut engine).await;
        assert!(rung.contains(&format!("audio:{}", line.len())), "{rung:?}");
        assert!(rung.contains(&format!("own_words:{request}")), "{rung:?}");
        assert_eq!(rung.last().unwrap(), "complete", "{rung:?}");
    }
    assert_eq!(script.asked.lock().unwrap().len(), 1, "no chat asked");
}

/// #7642: the limit not known yet, a due timer's own chat refused with the
/// relay's `usage_limit` is rung with its line all the same, ending
/// answered. With the switch off it is as before: nothing said, the turn
/// failed, for the consumer to offer again.
#[tokio::test]
async fn a_due_timer_refused_by_the_limit_is_rung_and_without_the_switch_is_not() {
    for on in [true, false] {
        let (engine, script, _) = engine(on, &[], vec![limited()], Duration::ZERO).await;
        let mut engine = engine.saying_reminders(true);
        engine.submit(due(1, true));
        let rung = turn(&mut engine).await;
        let ended = rung.last().cloned().unwrap_or_default();
        if on {
            assert!(
                rung.contains(&format!("audio:{}", LIMIT_TIMER_DONE.len())),
                "{rung:?}"
            );
            assert_eq!(ended, "complete", "{rung:?}");
        } else {
            assert!(
                !rung.iter().any(|label| label.starts_with("audio:")),
                "{rung:?}"
            );
            assert!(ended.starts_with("failed:"), "{rung:?}");
        }
        assert_eq!(script.asked.lock().unwrap().len(), 1);
    }
}
