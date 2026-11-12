//! #6547 (1b) on the second route: a spoken line whose own words name an
//! agent to do the work ("ask Hermes to …"), answered in round 1 with words
//! and no call, is caught in code. Those words are not said; the turn is
//! asked again once, told the person asked that agent and to call
//! `start_agent_task`, and the journal says so and what came of it. A line
//! that hands the work over, one naming no agent, a question about an agent
//! ("What is Hermes?"), and a request already asked again go on as before.
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
    /// Whether a round was asked with the named-agent note.
    told: bool,
    /// The journal's named lines.
    journal: Vec<String>,
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
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(std::iter::once(first).chain(later).collect()),
        ..Script::default()
    });
    let (engine, calls, _) = scripted(script.clone());
    let journal: Arc<Mutex<Vec<String>>> = Arc::default();
    let mut engine = engine
        .announcing(false)
        .catching_named_agents(Arc::new(named))
        .keeping_named_ask_journal(journal.clone());
    if asked_before {
        assert!(engine.shared.named_asked_again.first(1));
    }
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
    let asked = script.asked.lock().unwrap();
    let note = crate::agent_tasks::named_no_start_note("Hermes");
    let told = asked
        .iter()
        .flatten()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .any(|content| content == note);
    Went {
        said: labels
            .into_iter()
            .filter(|label| label.starts_with("said:"))
            .collect(),
        asked: asked.len(),
        ran: calls
            .lock()
            .unwrap()
            .iter()
            .map(|(name, ..)| name.clone())
            .collect(),
        told,
        journal: journal.lock().unwrap().clone(),
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

/// Asked again and answered in words once more: said as it came, never a
/// third round, and the journal says no call came.
#[test]
fn named_with_no_start_twice_is_said_the_second_time() {
    let went = went(
        "Hermes, look up the weather.",
        saying("It is sunny today."),
        vec![saying("It is still sunny.")],
        false,
    );
    assert_eq!(went.said, said(&["It is still sunny."]), "{went:?}");
    assert_eq!((went.asked, went.ran.len()), (2, 0), "{went:?}");
    assert!(went.told);
    assert_eq!(
        went.journal,
        [
            ASKED_AGAIN.to_owned(),
            format!("{ASKED_AGAIN}: still no call, said as it came"),
        ]
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
