//! #6547 (1b) on the second route: a spoken line whose own words name an
//! agent to do the work ("ask Hermes to …"), answered in round 1 with words
//! and no call, is caught in code. Those words are not said; the turn is
//! asked again once, told the person asked that agent and to call
//! `start_agent_task`, and the journal says so and what came of it. A line
//! that hands the work over, one naming no agent, a question about an agent
//! ("What is Hermes?"), and a request already asked again go on as before.
//!
//! #6547 C widens it: a round 1 answer that reads a task back with no call
//! ("Hermes will … Shall I start it?"), named agent or not, is asked again
//! too. Either is asked again only once the join window
//! ([`super::JOIN_WITHIN`]) has passed with no newer line begun, so a paused
//! ask is joined, never handed over half said; asked again and answered in
//! words once more, the plain line that it wasn't done is said, unless an
//! agent was named: then code calls `start_agent_task` itself with the
//! heard words, and the prepared task is said back for a yes (#6547
//! cause 3).
use super::*;

/// The lamp's engines and the computer's staff in these tests.
fn named(words: &str) -> Option<String> {
    crate::agent_tasks::named_for_work_in(
        words,
        &["hermes".to_owned(), "openclaw".to_owned()],
        &["Claude Code"],
    )
}

/// One turn as it went.
#[derive(Debug)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// The chat rounds asked.
    asked: usize,
    /// The calls run, by name.
    ran: Vec<String>,
    /// The calls' arguments, as run.
    args: Vec<Value>,
    /// Whether a round was asked with the named-agent note.
    told: bool,
    /// The journal's named lines.
    journal: Vec<String>,
    /// How long the turn took, by the test's paused clock.
    took: Duration,
}

/// One spoken turn: `heard` answered with round 1's `first` events and the
/// rounds `later`, the catch on. With `asked_before`, the request was
/// asked again once already.
fn went(
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
    asked_before: bool,
) -> Went {
    went_answered(heard, first, later, asked_before, json!({"ok": true}))
}

/// [`went`] with every call answered `answer`.
fn went_answered(
    heard: &str,
    first: Vec<net::Result<ChatEvent>>,
    later: Vec<Vec<net::Result<ChatEvent>>>,
    asked_before: bool,
    answer: Value,
) -> Went {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(std::iter::once(first).chain(later).collect()),
        ..Script::default()
    });
    let calls: Calls = Arc::default();
    let seen = calls.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        seen.lock().unwrap().push((name, args, heard));
        let answer = answer.clone();
        Box::pin(async move { answer })
    });
    let at = Instant::now();
    let engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(move || at),
    );
    let journal: Arc<Mutex<Vec<String>>> = Arc::default();
    let mut engine = engine
        .announcing(false)
        .catching_named_agents(Arc::new(named))
        .keeping_named_ask_journal(journal.clone());
    if asked_before {
        assert!(engine.shared.named_asked_again.first(1));
    }
    let (labels, took) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let began = tokio::time::Instant::now();
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            let labels = turn_within(&mut engine, Duration::from_secs(10)).await;
            (labels, began.elapsed())
        });
    let asked = script.asked.lock().unwrap();
    let note = crate::agent_tasks::named_no_start_note("Hermes");
    let notes = [
        note,
        crate::agent_tasks::read_back_no_start_note(true),
        crate::agent_tasks::read_back_no_start_note(false),
    ];
    let told = asked
        .iter()
        .flatten()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .any(|content| notes.iter().any(|note| note == content));
    let ran = calls.lock().unwrap().clone();
    Went {
        said: labels
            .into_iter()
            .filter(|label| label.starts_with("said:"))
            .collect(),
        asked: asked.len(),
        ran: ran.iter().map(|(name, ..)| name.clone()).collect(),
        args: ran.iter().map(|(_, args, _)| args.clone()).collect(),
        told,
        journal: journal.lock().unwrap().clone(),
        took,
    }
}

fn saying(words: &str) -> Vec<net::Result<ChatEvent>> {
    vec![text(words), Ok(ChatEvent::Done)]
}

fn starting() -> Vec<net::Result<ChatEvent>> {
    vec![
        Ok(ChatEvent::ToolCalls(vec![call(
            "c0",
            crate::agent_tasks::START,
            json!({"task": "Look up the weather"}),
        )])),
        Ok(ChatEvent::Done),
    ]
}

fn said(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| format!("said:{line}")).collect()
}

const ASKED_AGAIN: &str =
    "lamp-voice second: request 1: named agent, no start_agent_task: asked again";

/// Hermes named for the work, round 1 answering it in words: not said,
/// asked again once with the note; the call then runs and its round is
/// said. The journal says it was asked again and that the call came.
#[test]
fn named_with_no_start_is_asked_again_once() {
    let went = went(
        "Ask Hermes to look up the weather.",
        saying("It is sunny today."),
        vec![starting(), saying("Hermes is on it.")],
        false,
    );
    assert_eq!(went.said, said(&["Hermes is on it."]), "{went:?}");
    assert_eq!(went.asked, 3, "{went:?}");
    // Asked again only once the join window passed (#6547 C).
    assert!(went.took >= JOIN_WITHIN, "{went:?}");
    assert_eq!(went.ran, [crate::agent_tasks::START.to_owned()]);
    assert!(went.told, "{went:?}");
    assert_eq!(
        went.journal,
        [
            ASKED_AGAIN.to_owned(),
            format!("{ASKED_AGAIN}: start_agent_task called"),
        ]
    );
}

/// The journal's line for a hand-over made in code (#6547 cause 3).
fn handed_in_code(agent: &str) -> String {
    format!(
        "{ASKED_AGAIN}: still no call: the heard words are handed over in code to {agent}, said back for a yes"
    )
}

/// The poem read back from the prepared task.
const POEM_READ_BACK: &str =
    "I'll have Hermes on the lamp write a poem about tea in a file. Shall I start it?";

/// #6547 cause 3: Hermes named for the work, round 1 and the round asked
/// again both answered in words with no call. Neither is said; code calls
/// `start_agent_task` itself with the heard words as the task, so the work
/// is prepared as a model's call would be, nothing dispatched, and the
/// prepared task's read-back is said for the person's yes or cancel. No
/// "I didn't do that", and the journal says it was handed over in code.
#[test]
fn named_with_no_start_twice_is_handed_over_in_code_and_said_back() {
    const HEARD: &str = "Have Hermes write a poem about tea in a file.";
    assert_eq!(named(HEARD).as_deref(), Some("Hermes"));
    let went = went(
        HEARD,
        saying("Sure, here is a poem about tea."),
        vec![
            saying("Tea leaves dance in the cup."),
            saying(POEM_READ_BACK),
        ],
        false,
    );
    assert_eq!(
        went.said,
        [
            "said:I'll have Hermes on the lamp write a poem about tea in a file.",
            "said: Shall I start it?",
        ],
        "{went:?}"
    );
    // Prepared only: never `confirm_agent_task`, so nothing starts before
    // the person's yes.
    assert_eq!(went.ran, [crate::agent_tasks::START.to_owned()], "{went:?}");
    assert_eq!(went.args, [json!({"task": HEARD})], "{went:?}");
    assert_eq!(went.asked, 3, "{went:?}");
    assert!(went.told, "{went:?}");
    assert_eq!(
        went.journal,
        [ASKED_AGAIN.to_owned(), handed_in_code("Hermes")],
        "{went:?}"
    );
}

/// #6547 cause 3's control: Hermes named and round 1 answered with the
/// lamp's own tool (none run), then the round asked again with nothing:
/// handed over in code the same way.
#[test]
fn named_answered_by_an_own_tool_then_nothing_is_handed_over_in_code() {
    const HEARD: &str = "Hermes, look up the weather in Hanoi.";
    let went = went(
        HEARD,
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "w0",
                "weather",
                json!({"place": "Hanoi"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![
            saying("It is sunny in Hanoi."),
            saying("Hermes will look it up. Shall I start it?"),
        ],
        false,
    );
    assert_eq!(went.ran, [crate::agent_tasks::START.to_owned()], "{went:?}");
    assert_eq!(went.args, [json!({"task": HEARD})], "{went:?}");
    assert_eq!(
        went.journal.last(),
        Some(&handed_in_code("Hermes")),
        "{went:?}"
    );
}

/// #6547 cause 3's control: a computer agent named with none paired. The
/// code-made call carries the heard words only, so the tasks part refuses
/// it from those words as it refuses a model's (#6786): nothing prepared,
/// and the refusal is what the next round says.
#[test]
fn a_computer_agent_named_with_none_paired_is_refused_as_before() {
    const HEARD: &str = "Ask Claude Code to write a poem about tea in a file.";
    const REFUSED_SAID: &str = "No computer is paired with the lamp.";
    assert_eq!(named(HEARD).as_deref(), Some("Claude Code"));
    let went = went_answered(
        HEARD,
        saying("Sure, here is a poem about tea."),
        vec![saying("Tea leaves dance in the cup."), saying(REFUSED_SAID)],
        false,
        json!({"error": crate::agent_tasks::NO_COMPUTER_PAIRED}),
    );
    assert_eq!(went.said, said(&[REFUSED_SAID]), "{went:?}");
    assert_eq!(went.ran, [crate::agent_tasks::START.to_owned()], "{went:?}");
    // No place or who of the code's own: the tasks part reads them from
    // the heard words, as for the model's call.
    assert_eq!(went.args, [json!({"task": HEARD})], "{went:?}");
    assert_eq!(went.journal.last(), Some(&handed_in_code("Claude Code")));
}

/// #6547 cause 3's control: the round asked again calls
/// `start_agent_task` itself: only the model's call runs, with the
/// model's task, and none is made in code.
#[test]
fn named_with_start_when_asked_again_makes_none_in_code() {
    let went = went(
        "Have Hermes write a poem about tea in a file.",
        saying("Sure, here is a poem about tea."),
        vec![starting(), saying("Hermes is on it.")],
        false,
    );
    assert_eq!(went.ran, [crate::agent_tasks::START.to_owned()], "{went:?}");
    assert_eq!(
        went.args,
        [json!({"task": "Look up the weather"})],
        "{went:?}"
    );
    assert_eq!(
        went.journal,
        [
            ASKED_AGAIN.to_owned(),
            format!("{ASKED_AGAIN}: start_agent_task called"),
        ]
    );
}

/// #6547 cause 3's control: no agent named, a read-back twice with no
/// call: nothing is made in code; the plain line that it wasn't done is
/// said, as before.
#[test]
fn not_named_no_start_twice_makes_none_in_code() {
    let went = went(
        "Write a poem about tea in a file.",
        saying(READ_BACK),
        vec![saying(READ_BACK)],
        false,
    );
    assert!(went.ran.is_empty(), "{went:?}");
    assert_eq!(
        went.said
            .last()
            .map(|line| line.trim_start_matches("said:").trim()),
        Some(crate::action_claim::NOT_DONE_LINE),
        "{went:?}"
    );
    assert_eq!(
        went.journal.last(),
        Some(&format!(
            "{READ_BACK_AGAIN}: still no call, said it wasn't done"
        ))
    );
}

/// Hermes named and round 1 hands the work over: untouched, nothing asked
/// again, nothing journalled.
#[test]
fn named_with_start_is_untouched() {
    let went = went(
        "Ask Hermes to look up the weather.",
        starting(),
        vec![saying("Hermes is on it.")],
        false,
    );
    assert_eq!(went.said, said(&["Hermes is on it."]), "{went:?}");
    assert_eq!(went.asked, 2);
    assert_eq!(went.ran, [crate::agent_tasks::START.to_owned()]);
    assert!(!went.told);
    assert!(went.journal.is_empty(), "{:?}", went.journal);
}

/// No agent named, and a question about one ("What is Hermes?"): answered
/// in words as it came, in one round.
#[test]
fn not_named_and_a_question_about_an_agent_are_untouched() {
    for heard in [
        "What's the weather today?",
        "What is Hermes?",
        "Is OpenClaw's update out?",
        // Only called by name, asking no work (#6547 hold, Thirteen's P2
        // on #6572): round 1's answer is said as normal.
        "Hermes, what time is it?",
        "Hermes, how are you?",
        "Hey Hermes, tell me a joke.",
        "Codex, what can you do?",
    ] {
        assert_eq!(named(heard), None, "{heard}");
        let went = went(heard, saying("It is sunny today."), vec![], false);
        assert_eq!(went.said, said(&["It is sunny today."]), "{heard}");
        assert_eq!((went.asked, went.ran.len()), (1, 0), "{heard}");
        assert!(!went.told, "{heard}");
        assert!(went.journal.is_empty(), "{heard} {:?}", went.journal);
    }
}

/// The same request asked again before (a second time in one request):
/// not asked again, said as it came, and the journal says why.
#[test]
fn a_request_asked_again_before_is_not_asked_again() {
    let went = went(
        "Ask Hermes to look up the weather.",
        saying("It is sunny today."),
        vec![],
        true,
    );
    assert_eq!(went.said, said(&["It is sunny today."]), "{went:?}");
    assert_eq!((went.asked, went.ran.len()), (1, 0));
    assert!(!went.told);
    assert_eq!(
        went.journal,
        [format!("{ASKED_AGAIN} before, said as it came")]
    );
}

/// A read-back of the work with no call.
const READ_BACK: &str =
    "I'll have Hermes on the lamp make a file called beach.md. Shall I start it?";

/// [`READ_BACK`] as said, a label a sentence.
const READ_BACK_SAID: [&str; 2] = [
    "said:I'll have Hermes on the lamp make a file called beach.md.",
    "said: Shall I start it?",
];

const READ_BACK_AGAIN: &str =
    "lamp-voice second: request 1: read back, no start_agent_task: asked again";

/// #6547 C: no agent named, and round 1 reads the task back in words with
/// no call: said as it came (the person hears it and may say yes), and
/// once the join window passed with no newer line, asked again once; the
/// call then runs, and the journal says so.
#[test]
fn a_words_only_read_back_is_asked_again_for_the_call() {
    let went = went(
        "Make me a file called beach.md.",
        saying(READ_BACK),
        vec![starting(), saying("")],
        false,
    );
    // Said as it streamed, its closing question too, before the wait.
    assert_eq!(went.said, READ_BACK_SAID, "{went:?}");
    assert_eq!(went.ran, [crate::agent_tasks::START.to_owned()]);
    assert!(went.told, "{went:?}");
    assert!(went.took >= JOIN_WITHIN, "{went:?}");
    assert_eq!(
        went.journal,
        [
            READ_BACK_AGAIN.to_owned(),
            format!("{READ_BACK_AGAIN}: start_agent_task called"),
        ]
    );
}

/// #6547 C: asked again and still answered in words only: the second
/// read-back is not said, the plain line is, and nothing loops.
#[test]
fn a_read_back_twice_says_it_was_not_done_and_never_loops() {
    let went = went(
        "Make me a file called beach.md.",
        saying(READ_BACK),
        vec![saying(READ_BACK)],
        false,
    );
    let mut lines = READ_BACK_SAID.to_vec();
    lines.push("said: Sorry, I didn't do that. Could you ask me again?");
    assert_eq!(went.said, lines, "{went:?}");
    assert_eq!(
        lines[2].trim_start_matches("said:").trim(),
        crate::action_claim::NOT_DONE_LINE
    );
    assert_eq!((went.asked, went.ran.len()), (2, 0), "{went:?}");
    assert_eq!(
        went.journal,
        [
            READ_BACK_AGAIN.to_owned(),
            format!("{READ_BACK_AGAIN}: still no call, said it wasn't done"),
        ]
    );
}

/// #6547 C: an answer that asks something else ("Shall I start a timer?")
/// is no read-back: said in one round, at once, nothing asked again.
#[test]
fn an_answer_that_is_no_read_back_is_untouched() {
    let went = went(
        "I'm going to cook some pasta.",
        saying("Shall I start a timer for ten minutes?"),
        vec![],
        false,
    );
    assert_eq!(
        went.said,
        said(&["Shall I start a timer for ten minutes?"]),
        "{went:?}"
    );
    assert_eq!((went.asked, went.ran.len()), (1, 0), "{went:?}");
    assert!(went.journal.is_empty(), "{:?}", went.journal);
    assert!(went.took < JOIN_WITHIN, "{went:?}");
}

/// W3's P1 on #6572, fixed by #6547 C: a paused ask whose first half
/// stands alone ("Ask Hermes on the lamp to make a file called beach.md.")
/// and whose second half begins 1.5 s after it, within the join window.
/// The first half is never asked again nor handed over; the two are asked
/// as one line, and only that line's START runs.
#[test]
fn a_paused_ask_is_joined_and_never_asked_again_half_said() {
    const FIRST: &str = "Ask Hermes on the lamp to make a file called beach.md.";
    const SECOND: &str = "And write a poem about the sea in it.";
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(FIRST.to_owned()), Ok(SECOND.to_owned())].into()),
        chats: Mutex::new(
            [
                saying("Sure, a file called beach.md."),
                starting(),
                saying("Hermes is on it."),
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, calls, now) = scripted(script.clone());
    let journal: Arc<Mutex<Vec<String>>> = Arc::default();
    let mut engine = engine
        .announcing(false)
        .catching_named_agents(Arc::new(named))
        .keeping_named_ask_journal(journal.clone())
        .joining_cut_words(Arc::new(|_, _| Joined::Same("one face seen speaking both")))
        .unsaying_joined(Arc::new(|_| {}));
    let gap = Duration::from_millis(1_500);
    let labels = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let mut labels: Vec<String> = Vec::new();
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            let until = tokio::time::Instant::now() + gap;
            while let Ok(event) = tokio::time::timeout_at(until, engine.next()).await {
                labels.push(format!("{event:?}"));
            }
            // No call ran in the first half's join window.
            assert!(calls.lock().unwrap().is_empty());
            *now.lock().unwrap() += gap;
            engine.submit(start(2));
            engine.retire_through(1);
            engine.submit(audio(2, 1));
            engine.submit(end(2));
            loop {
                let event = tokio::time::timeout(Duration::from_secs(10), engine.next())
                    .await
                    .expect("request 2's turn ends");
                let done = matches!(event, ProviderOutput::TurnComplete { request: 2, .. });
                labels.push(format!("{event:?}"));
                if done {
                    break;
                }
            }
            labels
        });
    let asked = script.asked.lock().unwrap().clone();
    let joined = Message::user(format!("{FIRST} {SECOND}"));
    assert!(
        asked.iter().any(|round| round.last() == Some(&joined)),
        "{asked:?}"
    );
    let note = crate::agent_tasks::named_no_start_note("Hermes");
    assert!(
        !asked
            .iter()
            .flatten()
            .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
            .any(|content| content == note),
        "{asked:?}"
    );
    let ran = calls.lock().unwrap().clone();
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(ran[0].0, crate::agent_tasks::START);
    assert!(
        !journal
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.ends_with(": asked again")),
        "{:?} {labels:?}",
        journal.lock().unwrap()
    );
}

/// V2's P1 on 12587 (Fifth's row-6547, "hermes"): Hermes named for the
/// work and round 1 calls the lamp's own `weather` instead of handing it
/// over. The weather call is not run, its words are not said, and the turn
/// is asked again once with the note; the hand-over then runs.
#[test]
fn named_answered_by_an_own_tool_runs_none_and_is_asked_again_once() {
    let went = went(
        "Hermes, look up the weather in Hanoi.",
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "w0",
                "weather",
                json!({"place": "Hanoi"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![starting(), saying("Hermes is on it.")],
        false,
    );
    assert_eq!(went.said, said(&["Hermes is on it."]), "{went:?}");
    assert_eq!(went.ran, [crate::agent_tasks::START.to_owned()], "{went:?}");
    assert!(went.told, "{went:?}");
    // Asked again only once the join window passed, as the calls-empty
    // catch (the Reviewer-lead's P3 on #6786).
    assert!(went.took >= JOIN_WITHIN, "{went:?}");
    assert_eq!(
        went.journal,
        [
            "lamp-voice second: request 1: named agent, called weather not start_agent_task: none run, asked again".to_owned(),
            format!("{ASKED_AGAIN}: start_agent_task called"),
        ]
    );
}

/// No agent named: round 1's own tool runs as before, nothing asked again.
#[test]
fn an_own_tool_with_no_agent_named_runs_as_before() {
    let went = went(
        "Look up the weather in Hanoi.",
        vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "w0",
                "weather",
                json!({"place": "Hanoi"}),
            )])),
            Ok(ChatEvent::Done),
        ],
        vec![saying("It is sunny in Hanoi.")],
        false,
    );
    assert_eq!(went.ran, ["weather".to_owned()], "{went:?}");
    assert!(!went.told, "{went:?}");
    assert!(went.journal.is_empty(), "{:?}", went.journal);
}

/// The Reviewer-lead's P1 on #6786: the agent named beside an agent-task
/// tool. "Hermes, go ahead and run it" answered with `confirm_agent_task`,
/// and "ask Hermes to check on my task" with `check_agent_task`: each runs
/// as called, nothing is asked again, and no new task is started.
#[test]
fn an_agent_task_tool_with_the_agent_named_runs_as_called() {
    for (heard, tool) in [
        ("Hermes, go ahead and run it.", crate::agent_tasks::CONFIRM),
        ("Ask Hermes to check on my task.", crate::agent_tasks::CHECK),
        (
            "Hermes, check what my tasks are doing.",
            crate::agent_tasks::NOW,
        ),
    ] {
        let went = went(
            heard,
            vec![
                Ok(ChatEvent::ToolCalls(vec![call("t0", tool, json!({}))])),
                Ok(ChatEvent::Done),
            ],
            vec![saying("Done.")],
            false,
        );
        assert_eq!(went.ran, [tool.to_owned()], "{heard}: {went:?}");
        assert!(!went.told, "{heard}: {went:?}");
        assert!(went.journal.is_empty(), "{heard}: {:?}", went.journal);
    }
}
