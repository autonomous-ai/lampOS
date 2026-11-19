//! #7976, lamp-9adf, 19 Nov 18:34 (`LAMP_MAKE_NO_ASKS=on`): "Lamp, make a
//! skill called tea reminder: every day at five in the afternoon, glow
//! green and say time for tea." came as two lines, cut at the colon.
//! Request 16 began 0.76 s after request 15 ended and cut it short before
//! any of its answer; the join refused them ("no voice check, and no one
//! face was seen speaking both; no face in view at request 15's end, 1 face
//! in view at request 16's, in a room not known to be quiet"), so request
//! 16 was asked alone, and its first round called `make_skill`'s turn off
//! of `time-for-tea`, a skill made with them minutes before: "Time for tea
//! is off". A split make is still one make, and its second half never acts
//! on another skill by itself (the first builder's ruling): with the
//! switch, the `make_skill` calls acting on a skill in the first round of a
//! line that goes on from words cut short just before it, not joined to
//! them and not told apart from them, are not run, and the model is told
//! those words to judge whether the two are one ask
//! ([`crate::make_no_asks::split_told`]).
use super::*;

const FIRST: &str = "Lamp, make a skill called tea reminder";
const SECOND: &str = "every day at five in the afternoon, glow green and say time for tea.";
const WHOLE: &str =
    "tea reminder: every day at five in the afternoon, glow green and say time for tea";

/// What the join said on 9adf: nothing to tell whether one person said both.
const NOTHING_TO_TELL: &str = "no voice check, and no one face was seen speaking both; no face in view at request 1's end, 1 face in view at request 2's, in a room not known to be quiet";

fn nothing_to_tell() -> Joined {
    Joined::Not(NOTHING_TO_TELL.to_owned())
}

/// Request 1 says `first`, its answer never played; request 2 says
/// `second` `gap_ms` later, cutting it short, the join's verdict `verdict`
/// ([`nothing_to_tell`] on 9adf), and its rounds are `rounds`. With `on`,
/// `LAMP_MAKE_NO_ASKS`. The calls run (name and arguments) and what each
/// chat was asked.
async fn split(
    on: bool,
    (first, second): (&'static str, &'static str),
    gap_ms: u64,
    verdict: Joined,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> (Vec<(String, Value)>, Vec<Vec<Message>>) {
    crate::make_no_asks::set_here(Some(on));
    let (release, hold) = tokio::sync::oneshot::channel();
    let mut chats = vec![vec![text("Sure."), Ok(ChatEvent::Done)]];
    chats.extend(rounds);
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(first.to_owned()), Ok(second.to_owned())].into()),
        chats: Mutex::new(chats.into()),
        speech_hold: Mutex::new(Some(("Sure.", hold))),
        ..Script::default()
    });
    let ran = Arc::new(Mutex::new(Vec::new()));
    let runs = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, args| {
        runs.lock().unwrap().push((name, args));
        Box::pin(async { json!({"ok": true}) })
    });
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: Vec::new(),
        }),
        tools,
        Arc::new(move || *clock.lock().unwrap()),
    )
    .announcing(false)
    .joining_cut_words(Arc::new(move |_, _| verdict.clone()));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 1's text comes");
        if matches!(&event, ProviderOutput::Transcript { request: Some(1), text, .. } if text.trim() == "Sure.")
        {
            break;
        }
    }
    while tokio::time::timeout(Duration::from_millis(100), engine.next())
        .await
        .is_ok()
    {}
    *now.lock().unwrap() += Duration::from_millis(gap_ms);
    engine.submit(start(2));
    engine.retire_through(1);
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), engine.next())
            .await
            .expect("request 2's turn ends");
        if matches!(event, ProviderOutput::TurnComplete { request: 2, .. }) {
            break;
        }
    }
    drop(release);
    crate::make_no_asks::set_here(None);
    let ran = ran.lock().unwrap().clone();
    let asked = script.asked.lock().unwrap().clone();
    (ran, asked)
}

fn turn_off() -> ToolCall {
    call(
        "c1",
        crate::made_skill::MAKE,
        json!({ crate::made_turn::TURN_OFF: "time for tea" }),
    )
}

fn make_whole(id: &str) -> ToolCall {
    call(id, crate::made_skill::MAKE, json!({ "request": WHOLE }))
}

/// The one run of `make_skill` with `args`.
fn ran_once(args: Value) -> [(String, Value); 1] {
    [(crate::made_skill::MAKE.to_owned(), args)]
}

/// What call `id` was told, in the first chat that holds its answer.
fn held_told(asked: &[Vec<Message>], id: &str) -> String {
    asked
        .iter()
        .find_map(|chat| {
            chat.iter().find(|message| {
                message.role == "tool" && message.tool_call_id.as_deref() == Some(id)
            })
        })
        .and_then(Message::words)
        .unwrap_or_default()
        .to_owned()
}

/// The 9adf shape: the second half's turn off is not run; the model, told
/// the first half, makes one skill with both, and nothing is turned off.
#[tokio::test]
async fn a_make_cut_at_its_colon_makes_one_skill_and_turns_nothing_off() {
    let (ran, asked) = split(
        true,
        (FIRST, SECOND),
        760,
        nothing_to_tell(),
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![turn_off()])),
                Ok(ChatEvent::Done),
            ],
            vec![
                Ok(ChatEvent::ToolCalls(vec![make_whole("c2")])),
                Ok(ChatEvent::Done),
            ],
            vec![text("Made tea reminder."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        ran,
        ran_once(json!({ "request": WHOLE })),
        "only the one make runs, no turn off"
    );
    // Request 2's second round was told the cut words, to judge.
    let told = held_told(&asked, "c1");
    assert!(told.contains(FIRST), "{told}");
    assert!(
        told.contains(crate::make_no_asks::SPLIT_NEXT),
        "the model is told to judge: {told}"
    );
}

/// Heron's P2 on #7990: two acting calls in the held round, the turn off
/// and a make of the second half alone, run neither; only the round after
/// it, told the cut words, makes the one skill.
#[tokio::test]
async fn every_acting_call_of_the_held_round_is_held() {
    let half = call("c1b", crate::made_skill::MAKE, json!({ "request": SECOND }));
    let (ran, asked) = split(
        true,
        (FIRST, SECOND),
        760,
        nothing_to_tell(),
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![turn_off(), half])),
                Ok(ChatEvent::Done),
            ],
            vec![
                Ok(ChatEvent::ToolCalls(vec![make_whole("c2")])),
                Ok(ChatEvent::Done),
            ],
            vec![text("Made tea reminder."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        ran,
        ran_once(json!({ "request": WHOLE })),
        "neither call of the held round runs, then the one make"
    );
    for id in ["c1", "c1b"] {
        let told = held_told(&asked, id);
        assert!(
            told.contains(crate::make_no_asks::SPLIT_NEXT),
            "{id}: {told}"
        );
    }
}

/// The model, told, still judges the line asks the turn off itself: its
/// call in the next round runs; never held twice.
#[tokio::test]
async fn a_turn_off_called_again_once_told_runs() {
    let again = call(
        "c2",
        crate::made_skill::MAKE,
        json!({ crate::made_turn::TURN_OFF: "time for tea" }),
    );
    let (ran, _) = split(
        true,
        (FIRST, "And turn off my time for tea skill."),
        760,
        nothing_to_tell(),
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![turn_off()])),
                Ok(ChatEvent::Done),
            ],
            vec![Ok(ChatEvent::ToolCalls(vec![again])), Ok(ChatEvent::Done)],
            vec![text("Time for tea is off."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        ran,
        ran_once(json!({ crate::made_turn::TURN_OFF: "time for tea" }))
    );
}

/// Heron's P2 on #7990: a held new make that is its own ask ("what time is
/// it", then "make a skill that glows blue at nine") is told to be called
/// again as it was, and that call runs.
#[tokio::test]
async fn a_held_make_of_its_own_is_told_to_call_again_and_runs() {
    const BLUE: &str = "a skill that glows blue at nine";
    let make = |id: &str| call(id, crate::made_skill::MAKE, json!({ "request": BLUE }));
    let (ran, asked) = split(
        true,
        ("What time is it?", "make a skill that glows blue at nine"),
        1_000,
        nothing_to_tell(),
        vec![
            vec![
                Ok(ChatEvent::ToolCalls(vec![make("c1")])),
                Ok(ChatEvent::Done),
            ],
            vec![
                Ok(ChatEvent::ToolCalls(vec![make("c2")])),
                Ok(ChatEvent::Done),
            ],
            vec![text("Made blue at nine."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(ran, ran_once(json!({ "request": BLUE })));
    let told = held_told(&asked, "c1");
    assert!(
        told.contains("If it is its own ask, make the call again as it was."),
        "{told}"
    );
}

/// No extra round where nothing was cut just before it, or where the join
/// told the lines apart: past the join's window, joined, with the switch
/// off, or another person by the voice prints or two speaking faces
/// (Heron's P2 on #7990), the turn off runs in the first round as before.
#[tokio::test]
async fn a_line_not_going_on_from_cut_words_runs_its_call_at_once() {
    for (on, gap_ms, verdict) in [
        (true, 4_000, nothing_to_tell()),
        (true, 760, Joined::Same("the two lines' voices")),
        (false, 760, nothing_to_tell()),
        (true, 760, Joined::Not(format!("{TWO_VOICES} 0.12)"))),
        (true, 760, Joined::Not(TWO_FACES_SPEAKING.to_owned())),
    ] {
        let case = format!("on {on}, {gap_ms} ms, {verdict:?}");
        let (ran, asked) = split(
            on,
            (FIRST, SECOND),
            gap_ms,
            verdict,
            vec![
                vec![
                    Ok(ChatEvent::ToolCalls(vec![turn_off()])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Time for tea is off."), Ok(ChatEvent::Done)],
            ],
        )
        .await;
        assert_eq!(
            ran,
            ran_once(json!({ crate::made_turn::TURN_OFF: "time for tea" })),
            "{case}"
        );
        let told = held_told(&asked, "c1");
        assert!(
            !told.contains(crate::make_no_asks::SPLIT_NEXT),
            "{case}: {told}"
        );
    }
}

/// Only a voice print or two speaking faces tell two lines apart; a want
/// of either, or a likeness between the bounds, does not.
#[test]
fn only_a_print_or_two_speaking_faces_tell_the_lines_apart() {
    assert!(Joined::Not(format!("{TWO_VOICES} 0.12)")).told_apart());
    assert!(Joined::Not(TWO_FACES_SPEAKING.to_owned()).told_apart());
    assert!(!nothing_to_tell().told_apart());
    assert!(
        !Joined::Not(
            "a likeness between the bars (cosine 0.50), and no one face was seen speaking both; \
             1 face in view at request 1's end"
                .to_owned()
        )
        .told_apart()
    );
    assert!(!Joined::Same("the two lines' voices").told_apart());
}
