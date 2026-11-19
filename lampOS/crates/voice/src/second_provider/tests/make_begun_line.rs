//! lamp-9adf, 18 Nov 04:23–04:25 (#7554's two FAIL rows): after a make was
//! left waiting for its yes, three new make asks each got "I'll make it
//! now. This takes about a minute. I'm on it." with `calls=none`: the
//! lamp's own waiting line ([`crate::make_skill_agent::ONE_PATH_WAITING_LINE`]),
//! copied by the model from the history, and nothing made. A reply that
//! says one of the lamp's own make lines with no `make_skill` call in its
//! turn is not said; the round is asked again once with `make_skill`
//! offered, never forced ([`crate::make_skill_agent::BEGUN_AGAIN_NOTE`]):
//! the model judges whether the line asks for a skill (El's guard). A
//! second such reply is said as the plain line that it wasn't done; one in
//! its own words is said as it came. A make that calls `make_skill` says
//! its lines as before.
use super::*;

/// The model's copy of the lamp's own make line, as said on 9adf.
fn copied() -> String {
    format!(
        "I'll make it now. {}",
        crate::make_skill_agent::ONE_PATH_WAITING_LINE
    )
}

/// One written turn on the one run path, its history the 9adf shape: a
/// make ask answered with the lamp's own line and no call or result (a cut
/// turn's words), then `rounds` for a new make ask. Its said lines, the
/// messages each of its chats was asked, and the calls run.
async fn after_a_copied_line(
    request: u64,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> (Vec<String>, Vec<Vec<Message>>, Vec<String>) {
    let script = Arc::new(Script {
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let ran = Arc::new(Mutex::new(Vec::new()));
    let runs = ran.clone();
    let tools: ToolRunner = Arc::new(move |request, _, name, _| {
        runs.lock().unwrap().push(name.to_owned());
        Box::pin(async move {
            // The make's agent begins: the lamp's own waiting line is due.
            if name == crate::made_skill::MAKE {
                tokio::time::sleep(Duration::from_millis(20)).await;
                assert!(crate::make_skill_agent::waiting_due(request));
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
            json!({"ok": true})
        })
    });
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: vec![
                Message::user("Make me a skill that tells me my first calendar event."),
                Message::assistant(copied()),
            ],
        }),
        tools,
        Arc::new(Instant::now),
    )
    .waiting_on_one_path(Some(Duration::from_secs(60)));
    engine.submit(written(
        request,
        "Make me a skill called cat joke that tells me a cat joke.",
    ));
    let said = turn(&mut engine)
        .await
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .map(|label| label.replacen("said: ", "said:", 1))
        .collect();
    let asked = script.asked.lock().unwrap().clone();
    let ran = ran.lock().unwrap().clone();
    (said, asked, ran)
}

fn ends_with_the_note(messages: &[Message]) -> bool {
    messages.last().is_some_and(|last| {
        last.role == "system" && last.words() == Some(crate::make_skill_agent::BEGUN_AGAIN_NOTE)
    })
}

/// The 9adf shape: the copy is not said, and the round asked again ends
/// with the note that offers `make_skill`; its bare make call then says
/// the lamp's opening as the turn's first words (#7554), and the lamp's own
/// waiting line, each once.
#[tokio::test]
async fn a_copied_make_line_with_no_call_is_not_said_and_asked_again_with_make_skill() {
    let make = call(
        "c1",
        crate::made_skill::MAKE,
        json!({"request": "cat joke"}),
    );
    let (said, asked, ran) = after_a_copied_line(
        7_554_100,
        vec![
            vec![text(&copied()), Ok(ChatEvent::Done)],
            vec![Ok(ChatEvent::ToolCalls(vec![make])), Ok(ChatEvent::Done)],
            vec![text("Made cat joke."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    // The history carries the lamp's own line, with no call beside it.
    assert!(
        asked[0].iter().any(|message| message.role == "assistant"
            && message.words().is_some_and(
                |words| words.contains(crate::make_skill_agent::ONE_PATH_WAITING_LINE)
            )),
        "{:?}",
        asked[0]
            .iter()
            .map(|m| (m.role, m.words()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        said,
        [
            format!("said:{}", crate::make_skill_agent::MAKE_OPENING),
            format!("said:{}", crate::make_skill_agent::ONE_PATH_WAITING_LINE),
            "said:Made cat joke.".into(),
        ]
    );
    assert!(!ends_with_the_note(&asked[0]));
    assert!(ends_with_the_note(&asked[1]), "{:?}", asked[1].last());
    assert_eq!(ran, [crate::made_skill::MAKE]);
    // The copy is never kept as the lamp's words in the round asked again.
    assert!(!asked[1].iter().skip(asked[0].len()).any(|message| {
        message
            .words()
            .is_some_and(|words| words.contains(crate::make_skill_agent::ONE_PATH_WAITING_LINE))
    }));
}

/// Asked again, still no call: the plain line that it wasn't done, never
/// the lamp's make line.
#[tokio::test]
async fn a_copied_make_line_twice_is_said_not_done() {
    let (said, asked, ran) = after_a_copied_line(
        7_554_200,
        vec![
            vec![text(&copied()), Ok(ChatEvent::Done)],
            vec![text(&copied()), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        said,
        [format!("said:{}", crate::action_claim::NOT_DONE_LINE)]
    );
    assert_eq!(asked.len(), 2);
    assert!(ran.is_empty());
}

/// A make that calls `make_skill` says its words and the lamp's waiting
/// line as before, with the line in the history; so does a reply that
/// says no make line.
#[tokio::test]
async fn a_make_with_its_call_and_other_replies_are_said_as_before() {
    let make = call(
        "c1",
        crate::made_skill::MAKE,
        json!({"request": "cat joke"}),
    );
    let (said, asked, _) = after_a_copied_line(
        7_554_300,
        vec![
            vec![
                text("I'll make it now."),
                Ok(ChatEvent::ToolCalls(vec![make])),
                Ok(ChatEvent::Done),
            ],
            vec![text("Made cat joke."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        said,
        [
            "said:I'll make it now.".to_owned(),
            format!("said:{}", crate::make_skill_agent::ONE_PATH_WAITING_LINE),
            "said:Made cat joke.".into(),
        ]
    );
    assert!(asked.iter().all(|messages| !ends_with_the_note(messages)));
    let (said, _, ran) = after_a_copied_line(
        7_554_400,
        vec![vec![
            text("A cat joke skill? "),
            text("Sure. Tell me when it should run."),
            Ok(ChatEvent::Done),
        ]],
    )
    .await;
    assert_eq!(
        said,
        [
            "said:A cat joke skill?",
            "said:Sure.",
            "said:Tell me when it should run."
        ]
    );
    assert!(ran.is_empty());
}

/// The reader El's HOLD on #7554 (lamp-9adf, 18 Nov): a make asked right
/// after another, its history carrying the lamp's own line, called
/// `make_skill`, yet its opening was held to the round's end and its first
/// audio came at 8539 ms. Once the streamed round names a `make_skill`
/// call, the held opening is said at once: here the round never ends, nor
/// does the make run, and its first words are still said.
#[tokio::test]
async fn a_make_line_turn_that_calls_make_skill_has_its_first_audio_unheld() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            [vec![
                text("I'll make it now. "),
                Ok(ChatEvent::Calling(crate::made_skill::MAKE.to_owned())),
            ]]
            .into(),
        ),
        hold_chat: true,
        ..Script::default()
    });
    let ran = Arc::new(Mutex::new(Vec::new()));
    let runs = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        runs.lock().unwrap().push(name);
        Box::pin(async { json!({"ok": true}) })
    });
    let mut engine = Engine::with_clock(
        script,
        Arc::new(|| Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: vec![
                Message::user("Make me a skill that tells me my first calendar event."),
                Message::assistant(copied()),
            ],
        }),
        tools,
        Arc::new(Instant::now),
    )
    .waiting_on_one_path(Some(Duration::from_secs(60)));
    engine.submit(written(
        7_554_600,
        "Make me a skill called cat joke that tells me a cat joke.",
    ));
    let first = loop {
        match tokio::time::timeout(Duration::from_secs(3), engine.next())
            .await
            .expect("its opening is said before its round ends")
        {
            ProviderOutput::Transcript {
                request: Some(7_554_600),
                text,
                ..
            } if !text.is_empty() => break text,
            _ => {}
        }
    };
    assert_eq!(first, "I'll make it now.");
    assert!(ran.lock().unwrap().is_empty(), "the make has not run");
}

/// The lamp's own lines are matched whole, case and apostrophes aside;
/// at most two sentences are held for them.
#[test]
fn the_lamps_own_make_lines_are_matched_whole() {
    use crate::make_skill_agent::{BEGUN_LINES, opens_begun, says_begun};
    assert!(says_begun(&copied(), &BEGUN_LINES));
    assert!(says_begun(
        "i\u{2019}ll make it now. this takes about a minute.  I\u{2019}m on it",
        &BEGUN_LINES
    ));
    assert!(says_begun(
        crate::make_skill_agent::LONG_WAIT_LINE,
        &BEGUN_LINES
    ));
    assert!(!says_begun(
        "This takes about a minute to bake.",
        &BEGUN_LINES
    ));
    assert_eq!(opens_begun("I'll make it now.", false, &BEGUN_LINES), None);
    assert_eq!(
        opens_begun(
            "I'll make it now. This takes about a minute. I'm",
            false,
            &BEGUN_LINES
        ),
        None
    );
    assert_eq!(
        opens_begun("Sure. Tell me when. It", false, &BEGUN_LINES),
        Some(false)
    );
    assert_eq!(opens_begun("Sure.", true, &BEGUN_LINES), Some(false));
    assert_eq!(opens_begun(&copied(), false, &BEGUN_LINES), Some(true));
}

/// Asked again, the model judged the line no make ask: its own words are
/// said, nothing is made, and no make line.
#[tokio::test]
async fn a_copied_make_line_asked_again_may_be_answered_in_words() {
    let (said, asked, ran) = after_a_copied_line(
        7_554_500,
        vec![
            vec![text(&copied()), Ok(ChatEvent::Done)],
            vec![
                text("Which cat joke should it tell? "),
                text("A new one each time?"),
                Ok(ChatEvent::Done),
            ],
        ],
    )
    .await;
    assert_eq!(
        said,
        [
            "said:Which cat joke should it tell?",
            "said:A new one each time?"
        ]
    );
    assert!(ends_with_the_note(&asked[1]));
    assert!(ran.is_empty());
}

/// The note offers `make_skill` and leaves the model to judge: it names
/// the function as a word, so a narrowed request is given it, and says
/// what to do when the line asks for no skill.
#[test]
fn the_note_offers_make_skill_and_leaves_the_judgement_to_the_model() {
    let note = crate::make_skill_agent::BEGUN_AGAIN_NOTE;
    assert!(
        note.split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|word| word == crate::made_skill::MAKE)
    );
    assert!(note.contains("If it doesn't, answer it in your own words."));
}

/// One written make turn with `rounds`, its history `history`, the action
/// claims checked or not, on the one run path: its said lines in order.
/// The make's agent begins (`agent`), its waiting line due, or not; the
/// make answers after `make_takes`.
async fn a_make_turn(
    request: u64,
    history: Vec<Message>,
    (claims, agent, make_takes): (bool, bool, Duration),
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> Vec<String> {
    let script = Arc::new(Script {
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(move |request, _, name, _| {
        Box::pin(async move {
            if name == crate::made_skill::MAKE {
                if agent {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    assert!(crate::make_skill_agent::waiting_due(request));
                }
                tokio::time::sleep(make_takes).await;
            }
            json!({"ok": true})
        })
    });
    let mut engine = Engine::with_clock(
        script,
        Arc::new(move || Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: history.clone(),
        }),
        tools,
        Arc::new(Instant::now),
    )
    .waiting_on_one_path(Some(Duration::from_secs(60)))
    .checking_action_claims(claims);
    engine.submit(written(
        request,
        "Also make me a skill called cat joke that tells a joke about cats when I pat your head.",
    ));
    turn(&mut engine)
        .await
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .map(|label| label.replacen("said: ", "said:", 1))
        .collect()
}

/// Round 1 only a `make_skill` call with `args`, named as it streams with
/// its first argument, then its calls.
fn a_bare_call(args: Value) -> Vec<net::Result<ChatEvent>> {
    let first = args
        .as_object()
        .and_then(|args| args.keys().next().cloned())
        .unwrap_or_default();
    vec![
        Ok(ChatEvent::Calling(crate::made_skill::MAKE.to_owned())),
        Ok(ChatEvent::Argued(crate::made_skill::MAKE.to_owned(), first)),
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            crate::made_skill::MAKE,
            args,
        )])),
        Ok(ChatEvent::Done),
    ]
}

/// Round 1 only a `make_skill` call for a new skill.
fn a_bare_make() -> Vec<net::Result<ChatEvent>> {
    a_bare_call(json!({"request": "cat joke"}))
}

/// lamp-9adf, 18 Nov 11:12 (#7554, request 23): a make asked while another
/// ran was answered with a bare `make_skill` call, no words before it, and
/// the person heard nothing until round 2's reply, 8104 ms after their
/// words. The lamp's own opening ([`crate::make_skill_agent::MAKE_OPENING`])
/// is said as the call is named, once, before the make's answer: with a
/// make that answers fast, one whose agent begins (its waiting line after
/// the opening), with the action claims checked, and on a history that
/// carries the lamp's own make line.
#[tokio::test]
async fn a_bare_make_skill_call_has_the_lamps_opening_said_first_once() {
    let opening = format!("said:{}", crate::make_skill_agent::MAKE_OPENING);
    let waiting = format!("said:{}", crate::make_skill_agent::ONE_PATH_WAITING_LINE);
    let made = || vec![text("Made cat joke."), Ok(ChatEvent::Done)];
    let plain = || vec![Message::user("Make me a skill that hums at noon.")];
    let fast = (false, false, Duration::ZERO);
    assert_eq!(
        a_make_turn(7_554_700, plain(), fast, vec![a_bare_make(), made()]).await,
        [opening.clone(), "said:Made cat joke.".into()]
    );
    let slow = (false, true, Duration::from_millis(400));
    assert_eq!(
        a_make_turn(7_554_701, plain(), slow, vec![a_bare_make(), made()]).await,
        [
            opening.clone(),
            waiting.clone(),
            "said:Made cat joke.".into()
        ]
    );
    let checked = (true, false, Duration::from_millis(100));
    assert_eq!(
        a_make_turn(7_554_702, plain(), checked, vec![a_bare_make(), made()]).await,
        [opening.clone(), "said:Made cat joke.".into()]
    );
    let copied_history = vec![
        Message::user("Make me a skill that tells me my first calendar event."),
        Message::assistant(copied()),
    ];
    assert_eq!(
        a_make_turn(7_554_703, copied_history, slow, vec![a_bare_make(), made()]).await,
        [opening.clone(), waiting, "said:Made cat joke.".into()]
    );
    // With no stream event, its calls name it at the round's end.
    let no_stream_event = vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c1",
            crate::made_skill::MAKE,
            json!({"request": "cat joke"}),
        )])),
        Ok(ChatEvent::Done),
    ];
    assert_eq!(
        a_make_turn(7_554_704, plain(), fast, vec![no_stream_event, made()]).await,
        [opening, "said:Made cat joke.".into()]
    );
}

/// #7679 (`LAMP_MAKE_QUEUE`): a bare make asked while another is under way
/// waits its turn, so the lamp's "I'll make it now" is never said before
/// it: the one line is the model's "I'll make the gold one next". Idle
/// again, the opening is said as before.
#[tokio::test]
async fn a_bare_make_while_another_is_made_says_no_opening_only_its_next_line() {
    let opening = format!("said:{}", crate::make_skill_agent::MAKE_OPENING);
    let next = || vec![text("I'll make the gold one next."), Ok(ChatEvent::Done)];
    let plain = || vec![Message::user("And another: check the gold price at six.")];
    let fast = (false, false, Duration::ZERO);
    crate::make_queue::clear_for_test();
    assert!(matches!(
        crate::make_queue::admit(
            7_679_100,
            crate::make_queue::Asker::default(),
            "bitcoin price".into(),
            Instant::now(),
            Box::new(|| crate::tools::ToolAnswer::plain(json!({}))),
        ),
        crate::make_queue::Admitted::Run
    ));
    assert_eq!(
        a_make_turn(7_679_101, plain(), fast, vec![a_bare_make(), next()]).await,
        ["said:I'll make the gold one next."]
    );
    crate::make_queue::clear_for_test();
    assert_eq!(
        a_make_turn(7_679_102, plain(), fast, vec![a_bare_make(), next()]).await,
        [opening, "said:I'll make the gold one next.".into()]
    );
}

/// The model's own opening before its call is said once, and the lamp's
/// never beside it; a round with no call says no opening, nor does a
/// `make_skill` call that makes nothing new (a remove, a run, a yes: the
/// function says nothing first for those).
#[tokio::test]
async fn a_make_with_the_models_opening_says_it_once_and_no_call_says_none() {
    let made = || vec![text("Made cat joke."), Ok(ChatEvent::Done)];
    let plain = || vec![Message::user("Make me a skill that hums at noon.")];
    for (request, claims) in [(7_554_710, false), (7_554_711, true)] {
        let mut round = vec![text("Sure, I'll make it now. ")];
        round.extend(a_bare_make());
        assert_eq!(
            a_make_turn(
                request,
                plain(),
                (claims, false, Duration::ZERO),
                vec![round, made()]
            )
            .await,
            ["said:Sure, I'll make it now.", "said:Made cat joke."]
        );
    }
    assert_eq!(
        a_make_turn(
            7_554_712,
            plain(),
            (false, false, Duration::ZERO),
            vec![vec![
                text("Which cat joke should it tell?"),
                Ok(ChatEvent::Done)
            ]]
        )
        .await,
        ["said:Which cat joke should it tell?"]
    );
    for (request, args) in [
        (7_554_713, json!({crate::made_remove::REMOVE: "cat joke"})),
        (7_554_714, json!({crate::made_run::RUN: "cat joke"})),
        (7_554_715, json!({crate::made_list::LIST: true})),
    ] {
        let removed = vec![text("Remove cat joke?"), Ok(ChatEvent::Done)];
        assert_eq!(
            a_make_turn(
                request,
                plain(),
                (false, false, Duration::ZERO),
                vec![a_bare_call(args), removed]
            )
            .await,
            ["said:Remove cat joke?"]
        );
    }
}

/// The lamp's opening is its first audio as the call is named: here the
/// round never ends, nor does the make run.
#[tokio::test]
async fn a_bare_make_skill_call_has_its_first_audio_before_the_round_ends() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            [vec![
                Ok(ChatEvent::Calling(crate::made_skill::MAKE.to_owned())),
                Ok(ChatEvent::Argued(
                    crate::made_skill::MAKE.to_owned(),
                    crate::make_skill_local::REQUEST.to_owned(),
                )),
            ]]
            .into(),
        ),
        hold_chat: true,
        ..Script::default()
    });
    let ran = Arc::new(Mutex::new(Vec::new()));
    let runs = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        runs.lock().unwrap().push(name);
        Box::pin(async { json!({"ok": true}) })
    });
    let mut engine = Engine::with_clock(
        script,
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    );
    engine.submit(written(
        7_554_720,
        "Also make me a skill called cat joke that tells a joke about cats.",
    ));
    let first = loop {
        match tokio::time::timeout(Duration::from_secs(3), engine.next())
            .await
            .expect("the lamp's opening is said before the round ends")
        {
            ProviderOutput::Transcript {
                request: Some(7_554_720),
                text,
                ..
            } if !text.is_empty() => break text,
            _ => {}
        }
    };
    assert_eq!(first, crate::make_skill_agent::MAKE_OPENING);
    assert!(ran.lock().unwrap().is_empty(), "the make has not run");
}

/// Round 1 only a `make_skill` call with `args`, its first argument
/// streamed as `first` (`None`: no stream event), then its calls.
fn a_streamed_call(first: Option<&str>, args: Value) -> Vec<net::Result<ChatEvent>> {
    let mut round = Vec::new();
    if let Some(first) = first {
        round.push(Ok(ChatEvent::Calling(crate::made_skill::MAKE.to_owned())));
        round.push(Ok(ChatEvent::Argued(
            crate::made_skill::MAKE.to_owned(),
            first.to_owned(),
        )));
    }
    round.push(Ok(ChatEvent::ToolCalls(vec![call(
        "c1",
        crate::made_skill::MAKE,
        args,
    )])));
    round.push(Ok(ChatEvent::Done));
    round
}

/// The stream and the whole calls read one rule for a new make: the
/// call's arguments name its `request`, whatever else it carries. The
/// same call gets the same opening on either path; a stream whose first
/// argument was another's keeps its decision when its whole calls name a
/// `request` after it.
#[tokio::test]
async fn the_stream_and_the_whole_calls_agree_on_a_new_make() {
    let opening = format!("said:{}", crate::make_skill_agent::MAKE_OPENING);
    let plain = || vec![Message::user("Make me a skill that hums at noon.")];
    let next = || vec![text("Done."), Ok(ChatEvent::Done)];
    let fast = (false, false, Duration::ZERO);
    let request = crate::make_skill_local::REQUEST;
    let beside = json!({"request": "cat joke", "approve_id": "a1"});
    let removed = json!({crate::made_remove::REMOVE: "cat joke"});
    for (at, (first, args, opened)) in [
        (Some(request), json!({"request": "cat joke"}), true),
        (None, json!({"request": "cat joke"}), true),
        (Some(request), beside.clone(), true),
        (None, beside.clone(), true),
        (Some(crate::made_remove::REMOVE), removed.clone(), false),
        (None, removed, false),
        (Some(crate::skill_write_yes::APPROVE_ID), beside, false),
    ]
    .into_iter()
    .enumerate()
    {
        let said = a_make_turn(
            7_554_730 + at as u64,
            plain(),
            fast,
            vec![a_streamed_call(first, args.clone()), next()],
        )
        .await;
        let expected: Vec<String> = if opened {
            vec![opening.clone(), "said:Done.".into()]
        } else {
            vec!["said:Done.".into()]
        };
        assert_eq!(said, expected, "{first:?} {args}");
    }
}

/// #7911 (Twelve's HOLD on #7926): the schema lists `given_name` before
/// `request`, so a named make may stream it first. It is a new make's
/// argument too: the opening is said once, as for a `request` first,
/// on the stream and on the whole calls alike.
#[tokio::test]
async fn a_make_whose_first_argument_is_its_given_name_is_opened() {
    let opening = format!("said:{}", crate::make_skill_agent::MAKE_OPENING);
    let plain = || {
        vec![Message::user(
            "Make a skill called evening rate that hums at noon.",
        )]
    };
    let next = || vec![text("Done."), Ok(ChatEvent::Done)];
    let fast = (false, false, Duration::ZERO);
    let named = json!({
        crate::make_skill_local::GIVEN_NAME: "evening rate",
        "request": "hum at noon"
    });
    for (at, first) in [Some(crate::make_skill_local::GIVEN_NAME), None]
        .into_iter()
        .enumerate()
    {
        let said = a_make_turn(
            7_911_730 + at as u64,
            plain(),
            fast,
            vec![a_streamed_call(first, named.clone()), next()],
        )
        .await;
        assert_eq!(
            said,
            vec![opening.clone(), "said:Done.".into()],
            "{first:?}"
        );
    }
    // `given_name` first decides nothing: the next argument does.
    assert_eq!(
        crate::make_skill_local::new_make_argument(crate::make_skill_local::GIVEN_NAME),
        None
    );
    assert_eq!(
        crate::make_skill_local::new_make_argument(crate::make_skill_local::REQUEST),
        Some(true)
    );
    assert_eq!(
        crate::make_skill_local::new_make_argument(crate::made_remove::REMOVE),
        Some(false)
    );
}

/// #7911 (Heron's HOLD on #7926): `given_name` sorts first in the schema,
/// so it streams first in any call that carries it. A remove with it, or
/// it alone with no `request`, is no new make: nothing is said first, on
/// the stream and on the whole calls alike.
#[tokio::test]
async fn a_given_name_without_a_request_opens_no_make() {
    let plain = || vec![Message::user("Remove the one called evening rate.")];
    let next = || vec![text("Done."), Ok(ChatEvent::Done)];
    let fast = (false, false, Duration::ZERO);
    let given = crate::make_skill_local::GIVEN_NAME;
    let removed = json!({given: "evening rate", crate::made_remove::REMOVE: "evening rate"});
    let alone = json!({given: "evening rate"});
    for (at, (first, args)) in [
        (Some(given), removed.clone()),
        (None, removed),
        (Some(given), alone.clone()),
        (None, alone),
    ]
    .into_iter()
    .enumerate()
    {
        let said = a_make_turn(
            7_911_740 + at as u64,
            plain(),
            fast,
            vec![a_streamed_call(first, args.clone()), next()],
        )
        .await;
        assert_eq!(said, vec!["said:Done.".to_owned()], "{first:?} {args}");
    }
}

/// #7554's P3: the opening is one of the lamp's fixed lines, said in the
/// owner's language and from its clip rendered ahead as the worker
/// starts, so its first audio never waits on the voice: here the voice
/// never answers it.
#[tokio::test]
async fn the_make_opening_is_said_in_the_owners_language_from_its_clip() {
    use crate::language::{Language, fixed_in};
    let english = crate::make_skill_agent::MAKE_OPENING;
    let words = fixed_in(Some(Language::Japanese), english);
    assert_ne!(words, english);
    assert!(crate::language::is_fixed(english));
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                a_bare_make(),
                vec![text("Made cat joke."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .in_language(Some(Language::Japanese))
    .rendering_make_opening(true);
    engine.render_lines();
    tokio::time::timeout(Duration::from_secs(2), async {
        while engine.shared.fixed_clips.kept(english).is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the opening is rendered ahead");
    let (_release, stuck) = tokio::sync::oneshot::channel::<()>();
    *script.speech_hold.lock().unwrap() = Some((words, stuck));
    engine.submit(written(
        7_554_740,
        "Also make me a skill called cat joke that tells a joke about cats.",
    ));
    let out = turn(&mut engine).await;
    let said: Vec<String> = out
        .iter()
        .filter(|o| o.starts_with("said:"))
        .map(|o| o.replacen("said: ", "said:", 1))
        .collect();
    assert_eq!(
        said,
        [format!("said:{words}"), "said:Made cat joke.".into()]
    );
    let at = out
        .iter()
        .position(|o| *o == format!("said:{words}"))
        .unwrap();
    assert_eq!(out[at + 1], format!("audio:{}", words.len()), "{out:?}");
}

/// One written make turn after three makes whose turns opened with the
/// make's opening, as the history held them before it was kept out:
/// `rounds`; its said lines, the messages each chat was asked, and the
/// calls run.
async fn after_three_makes(
    request: u64,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> (Vec<String>, Vec<Vec<Message>>, Vec<String>) {
    let script = Arc::new(Script {
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let ran = Arc::new(Mutex::new(Vec::new()));
    let runs = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        runs.lock().unwrap().push(name.to_owned());
        Box::pin(async { json!({"ok": true}) })
    });
    let opening = crate::make_skill_agent::MAKE_OPENING;
    let history = vec![
        Message::user("Make me a skill that dims the light at ten."),
        Message::assistant(format!("{opening} Made night dim.")),
        Message::user("Make me a skill called cat joke for when I pat your head."),
        Message::assistant(format!("{opening} Made cat joke.")),
        Message::user("Make me a skill that hums at noon."),
        Message::assistant(format!("{opening} Made noon hum.")),
    ];
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(move || Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: history.clone(),
        }),
        tools,
        Arc::new(Instant::now),
    );
    engine.submit(written(
        request,
        "Also make me a skill called dog joke that tells a joke about dogs.",
    ));
    let said = turn(&mut engine)
        .await
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .map(|label| label.replacen("said: ", "said:", 1))
        .collect();
    let asked = script.asked.lock().unwrap().clone();
    let ran = ran.lock().unwrap().clone();
    (said, asked, ran)
}

/// lamp-9adf, 18 Nov (#7554's ROW at 98e76debb0, requests 28, 30 and 33):
/// after three makes, new make asks were answered "I'll make it now.
/// Done — … is on the lamp" with no call, and nothing was made. A reply
/// that begins with the make's opening with no `make_skill` call in its
/// turn is not said, and asked again once with `make_skill` offered; its
/// make then says the lamp's opening and its answer, nothing false. Asked
/// again and still no call, the plain line that it wasn't done.
#[tokio::test]
async fn a_make_claimed_done_after_the_opening_with_no_call_is_asked_again() {
    let opening = crate::make_skill_agent::MAKE_OPENING;
    let claim = format!("{opening} Done — dog joke is on the lamp: pat my head and it tells one.");
    let (said, asked, ran) = after_three_makes(
        7_554_750,
        vec![
            vec![text(&claim), Ok(ChatEvent::Done)],
            a_bare_make(),
            vec![text("Made dog joke."), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        said,
        [format!("said:{opening}"), "said:Made dog joke.".into()]
    );
    assert!(!ends_with_the_note(&asked[0]));
    assert!(ends_with_the_note(&asked[1]), "{:?}", asked[1].last());
    assert_eq!(ran, [crate::made_skill::MAKE]);
    // The claim is never kept as the lamp's words in the round asked again.
    assert!(!asked[1].iter().skip(asked[0].len()).any(|message| {
        message
            .words()
            .is_some_and(|words| words.contains("is on the lamp"))
    }));
    let (said, asked, ran) = after_three_makes(
        7_554_751,
        vec![
            vec![text(&claim), Ok(ChatEvent::Done)],
            vec![text(&claim), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        said,
        [format!("said:{}", crate::action_claim::NOT_DONE_LINE)]
    );
    assert_eq!(asked.len(), 2);
    assert!(ran.is_empty());
}

/// The opening is read only at a reply's start: a reply that opens with
/// other words is said as it streams, the opening later in it or not.
#[test]
fn only_a_reply_that_opens_with_the_make_opening_is_held_for_it() {
    use crate::make_skill_agent::{is_opening, opens_with_opening};
    let opening = crate::make_skill_agent::MAKE_OPENING;
    assert_eq!(opens_with_opening("I'll", false), None);
    assert_eq!(opens_with_opening("I\u{2019}ll make it", false), None);
    assert_eq!(opens_with_opening(opening, false), Some(true));
    assert_eq!(
        opens_with_opening(&format!("{opening} Done — it's on."), false),
        Some(true)
    );
    assert_eq!(opens_with_opening("I'll check.", false), Some(false));
    assert_eq!(
        opens_with_opening("Sure, I'll make it now.", false),
        Some(false)
    );
    assert_eq!(opens_with_opening("I'll", true), Some(false));
    assert_eq!(opens_with_opening("", false), None);
    let japanese = crate::language::fixed_in(Some(crate::language::Language::Japanese), opening);
    assert_eq!(
        opens_with_opening(&format!("{japanese}完成！"), false),
        Some(true)
    );
    assert!(is_opening(opening) && is_opening(" i'll make it now") && is_opening(japanese));
    assert!(!is_opening("I'll make it now. Made cat joke.") && !is_opening("Made."));
}

/// lamp-9adf, 18 Nov (#7554's ROW at 2e864a0, request 94): the reply, word
/// for word, with `calls=none`; no `cat-joke-on-pat-3` was ever made.
const CLAIMED_IN_OWN_WORDS: &str = "I'll make that now. Cat joke on pat 3 clashes with cat joke \
     on pat, joke on pat and tell joke on pat… Replace them all, keep them all so they take \
     turns, or cancel?";

/// The 9adf shape: the make opening in the model's own words, then a clash
/// for a skill never made, with no call. Not said, asked again once with
/// `make_skill` offered; still no call, the plain line that it wasn't done.
#[tokio::test]
async fn a_make_and_clash_claimed_in_the_models_own_words_are_never_said() {
    let (said, asked, ran) = after_three_makes(
        7_554_960,
        vec![
            vec![text(CLAIMED_IN_OWN_WORDS), Ok(ChatEvent::Done)],
            vec![text(CLAIMED_IN_OWN_WORDS), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        said,
        [format!("said:{}", crate::action_claim::NOT_DONE_LINE)]
    );
    assert_eq!(asked.len(), 2);
    assert!(ends_with_the_note(&asked[1]), "{:?}", asked[1].last());
    assert!(ran.is_empty());
}

/// One written turn of `line` whose history is `history`, with `rounds`:
/// its said lines and the messages each of its chats was asked.
async fn after_history(
    request: u64,
    history: Vec<Message>,
    line: &str,
    rounds: Vec<Vec<net::Result<ChatEvent>>>,
) -> (Vec<String>, Vec<Vec<Message>>) {
    let script = Arc::new(Script {
        chats: Mutex::new(rounds.into()),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(move || Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: history.clone(),
        }),
        tools,
        Arc::new(Instant::now),
    );
    engine.submit(written(request, line));
    let said = turn(&mut engine)
        .await
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .map(|label| label.replacen("said: ", "said:", 1))
        .collect();
    let asked = script.asked.lock().unwrap().clone();
    (said, asked)
}

/// The history of 9adf's requests 92 and 93: the lamp asked a clash's
/// choice, and the person cancelled it.
fn after_a_clash() -> Vec<Message> {
    vec![
        Message::user("Make me a skill that tells a joke when I pat your head."),
        Message::assistant(
            "Joke on pat 2 would start at the same moment as cat joke on pat, joke on pat and \
             tell joke on pat, which are on the lamp, and they all use the light and the head. \
             Shall I replace them all, keep them all (they may take turns), or cancel?",
        ),
        Message::user("Never mind."),
        Message::assistant("Okay, cancelled. Nothing new is on."),
    ]
}

/// The clash alone, in the model's words, copied from a history that asked
/// one, with no call, just after that clash ended: not said, asked again;
/// an ordinary reply on such a turn is said as it came.
#[tokio::test]
async fn a_clash_claimed_with_no_call_is_not_said_and_other_replies_are() {
    let claim = CLAIMED_IN_OWN_WORDS
        .strip_prefix("I'll make that now. ")
        .unwrap();
    // 9adf's request 93 cancelled the clash 56 s before request 94.
    crate::made_clash::set_ended(Some(Instant::now() - Duration::from_secs(56)));
    let (said, asked) = after_history(
        7_554_970,
        after_a_clash(),
        "Make me a skill that tells a cat joke when I pat your head.",
        vec![
            vec![text(claim), Ok(ChatEvent::Done)],
            vec![text(claim), Ok(ChatEvent::Done)],
        ],
    )
    .await;
    assert_eq!(
        said,
        [format!("said:{}", crate::action_claim::NOT_DONE_LINE)]
    );
    assert_eq!(asked.len(), 2);
    assert!(ends_with_the_note(&asked[1]), "{:?}", asked[1].last());
    let (said, asked) = after_history(
        7_554_971,
        after_a_clash(),
        "Tell me a cat joke.",
        vec![vec![
            text("Why did the cat sit on the computer? To keep an eye on the mouse."),
            Ok(ChatEvent::Done),
        ]],
    )
    .await;
    assert_eq!(
        said.concat(),
        "said:Why did the cat sit on the computer?said:To keep an eye on the mouse.",
        "{said:?}"
    );
    assert_eq!(asked.len(), 1);
    crate::made_clash::set_ended(None);
}

/// The 40 ms between a streamed reply's pieces, as a word comes.
const PIECE_GAP: Duration = Duration::from_millis(40);

/// A written turn whose history asked a clash's choice, its reply streamed a
/// word every [`PIECE_GAP`]; a clash ended `ended` ago, or none: the time
/// from the ask to its first audio, on the test's clock.
async fn first_audio_after_a_clash(request: u64, ended: Option<Duration>, reply: &str) -> Duration {
    crate::made_clash::set_ended(ended.map(|ago| Instant::now() - ago));
    let cut: Vec<&str> = reply.split_inclusive(' ').collect();
    let script = Arc::new(Script {
        chats: Mutex::new(
            [cut.iter()
                .map(|piece| text(piece))
                .chain([Ok(ChatEvent::Done)])
                .collect()]
            .into(),
        ),
        piece_gap: Some(PIECE_GAP),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"ok": true}) }));
    let mut engine = Engine::with_clock(
        script,
        Arc::new(|| Told {
            text: "SYSTEM".to_owned(),
            after: None,
            history: after_a_clash(),
        }),
        tools,
        Arc::new(Instant::now),
    );
    engine.submit(written(request, "Tell me a cat joke."));
    let asked = tokio::time::Instant::now();
    let first = loop {
        match engine.next().await {
            ProviderOutput::Audio { samples, .. } if !samples.is_empty() => {
                break asked.elapsed();
            }
            ProviderOutput::TurnComplete { .. } | ProviderOutput::TurnFailed { .. } => {
                panic!("{reply}: nothing said")
            }
            _ => {}
        }
    };
    crate::made_clash::set_ended(None);
    first
}

/// App Only's P2 on #7554: the clash hold was on every turn whose history
/// carried a clash's question, for the rest of the conversation, each
/// reply's first audio waiting two sentences. It is now only on a turn
/// within `made_clash::COPIED_WITHIN` of a clash's end; a later turn, or
/// one with no clash ended, says its first sentence as it streams. Timed on
/// the test's clock, a word every 40 ms; shown with `--nocapture`.
#[tokio::test(start_paused = true)]
async fn a_turn_long_after_a_clash_ended_is_not_held_for_it() {
    let past = crate::made_clash::COPIED_WITHIN + Duration::from_secs(1);
    for reply in [
        "Why did the cat sit on the computer? To keep an eye on the mouse.",
        "Sure. Cats nap about sixteen hours a day, and they dream as we do.",
    ] {
        let held = first_audio_after_a_clash(7_554_980, Some(Duration::from_secs(56)), reply).await;
        let later = first_audio_after_a_clash(7_554_981, Some(past), reply).await;
        let none = first_audio_after_a_clash(7_554_982, None, reply).await;
        eprintln!(
            "#7554 clash hold, first audio: 56 s after a clash ended (held, as every such turn was at 824969b) {} ms; {} s after (not held) {} ms; no clash ended {} ms: {reply}",
            held.as_millis(),
            past.as_secs(),
            later.as_millis(),
            none.as_millis()
        );
        assert_eq!(later, none, "{reply}");
        assert!(held > later, "{reply}");
    }
}

/// The opening's family, read at the reply's start only, and the clash
/// choice by its structure, a claim only when no clash waits.
#[test]
fn the_make_opening_and_clash_choice_are_read_by_their_structure() {
    use crate::make_skill_agent::{clash_claimed, opens_with_make_line, opens_with_opening};
    for said in [
        "I'll make that now.",
        "I\u{2019}ll make that now. Cat joke on pat 3 clashes …",
        "Making it now!",
        "Let me make that one for you.",
        "I'm making the skill right now.",
        "I'll make it now.",
    ] {
        assert_eq!(opens_with_make_line(said, false), Some(true), "{said}");
    }
    for said in [
        "I'll make it brighter.",
        "I'll make that now?",
        "I'll check.",
        "Sure, I'll make that now.",
        "Making tea is easy.",
        "The opening I'll make that now.",
        // App Only's P3 on #7554: more follows that isn't the make's.
        "I'll make it now, then set the light.",
        "I'll make that now, then dim the light.",
    ] {
        assert_eq!(opens_with_make_line(said, false), Some(false), "{said}");
        assert_eq!(opens_with_make_line(said, true), Some(false), "{said}");
    }
    assert_eq!(
        opens_with_opening("I'll make it now, then set the light.", true),
        Some(false)
    );
    assert_eq!(opens_with_opening("I'll make it now", false), None);
    assert_eq!(opens_with_opening("I'll make it now", true), Some(true));
    // A bare "make it" may be about anything else (and is the opening's
    // start while it streams).
    assert_eq!(opens_with_make_line("I'll make it.", true), Some(false));
    assert_eq!(opens_with_make_line("I'll make that. ", false), Some(false));
    assert_eq!(opens_with_make_line("I'll make th", false), None);
    assert_eq!(opens_with_make_line("I'll make that now", false), None);
    assert_eq!(opens_with_make_line("I'll make that now", true), Some(true));
    let claim = CLAIMED_IN_OWN_WORDS;
    let lamps = after_a_clash()[1].words().unwrap().to_owned();
    for asked in [claim, lamps.as_str()] {
        assert_eq!(clash_claimed(asked, false, || false), Some(true), "{asked}");
        assert_eq!(clash_claimed(asked, true, || true), Some(false), "{asked}");
    }
    for none in ["Shall I cancel it?", "Keep it, replace it, or cancel?"] {
        assert_eq!(clash_claimed(none, true, || false), Some(false), "{none}");
    }
    assert_eq!(
        clash_claimed("Cat joke on pat 3 clashes", false, || false),
        None
    );
    assert_eq!(
        clash_claimed("Sure. Here is one. Then", false, || false),
        Some(false)
    );
    assert_eq!(clash_claimed("Why not?", true, || false), Some(false));
}
