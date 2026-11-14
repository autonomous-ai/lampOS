//! #7027 on the second route: while an agent task's read-back waits, a
//! whole plain yes, heard clear of the lamp's voice on a later request than
//! the prepare, starts the task in code before the chat round begins
//! (`crate::agent_tasks::yes_taken`). On lamp-9adf 2 of 5 such yeses were
//! lost: one to a chat round that never began ("chat round 1 had not begun
//! in 4500 ms; the turn ends unanswered"), one to a model that answered in
//! words with no call. A line that is more than a plain yes ("Yes, but make
//! it six fruits."), a typed one, or one begun over the lamp's voice goes
//! to the model as before, and the task still waits.
use super::*;
use crate::agent_tasks::{self as tasks, tests as fake};

/// The task said back, as the lamp said it in request 1.
const MOON_BACK: &str = "Claude Code will write a two-line poem about the moon on your office computer. Shall I start it?";

/// A task prepared in request 1 through a fake tasks part that answers
/// one status and one confirm after the prepare, read back whole in that
/// request: the place, what the tasks part was sent, and the guards held
/// while the test runs (the shared prepared task and the thread).
struct Prepared {
    place: tasks::Place,
    seen: std::sync::mpsc::Receiver<Value>,
    directory: std::path::PathBuf,
    _one: std::sync::MutexGuard<'static, ()>,
    _thread: crate::provider_worker::tests::OneThread,
}

impl Drop for Prepared {
    fn drop(&mut self) {
        fake::forget_pending();
        tasks::awaits_yes_here(None);
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// How long a test waits for its fake tasks part: a fixture's bound.
const PEER_WAIT: Duration = Duration::from_secs(10);

fn prepared(tag: &str, after: Vec<(&'static str, Value)>) -> Prepared {
    let one = fake::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let thread = crate::provider_worker::tests::one_thread();
    fake::forget_pending();
    let directory =
        std::env::temp_dir().join(format!("second-task-yes-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    let status = json!({"ok":true,"store_id":"s1","revision":0,"summary":{"session_id":"sess"}});
    let answers = [
        ("request.sock", status),
        (
            "request.sock",
            json!({"ok":true,"id":4,"prepared_digest":"moon","revision":1}),
        ),
    ]
    .into_iter()
    .chain(after)
    .collect();
    let seen = fake::tasks_part(&directory, answers);
    let flags = [tasks::FLAG.to_owned(), directory.display().to_string()];
    let place = tasks::take(&flags).unwrap().0.unwrap();
    let start = lamp_gemini::ToolCall {
        id: "c".into(),
        name: tasks::START.into(),
        args: json!({"task": "Write a two-line poem about the moon"}),
    };
    let (told, _) = tasks::run(&place, &start, 1, true, PEER_WAIT);
    assert_eq!(told["prepared"], json!(true), "{told}");
    {
        let now = Instant::now();
        let mut thread = crate::thread::THREAD.lock().unwrap();
        *thread = crate::thread::Thread::new();
        thread.person(
            "Ask Claude Code for a two-line poem about the moon",
            true,
            now,
        );
        thread.lamp(1, MOON_BACK, now);
    }
    // The read-back waits, as `awaits_yes` reads it on this test's thread.
    tasks::awaits_yes_here(Some(Instant::now()));
    Prepared {
        place,
        seen,
        directory,
        _one: one,
        _thread: thread,
    }
}

/// What the tasks part answers for a confirm: its status, then the confirm.
fn confirms() -> Vec<(&'static str, Value)> {
    vec![
        (
            "request.sock",
            json!({"ok":true,"store_id":"s1","revision":1,"summary":{"session_id":"sess"}}),
        ),
        (
            "voice.sock",
            json!({"ok":true,"id":4,"queued":true,"revision":2}),
        ),
    ]
}

/// One turn as it went.
struct Went {
    /// The calls the model's rounds ran, by name.
    ran: Vec<String>,
    /// Every message each chat round was asked with.
    asked: Vec<Vec<Message>>,
}

/// `input`'s line in request 2 (`heard`, spoken unless `written`), answered
/// with the model's `rounds` (or a chat that never begins), the yes and
/// the no taken in code wired as the worker wires them.
fn went(
    ready: &Prepared,
    heard: &str,
    input: impl FnOnce() -> Vec<ProviderInput>,
    rounds: Option<Vec<Vec<net::Result<ChatEvent>>>>,
) -> Went {
    let hold = rounds.is_none();
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new(rounds.unwrap_or_else(|| vec![vec![]]).into()),
        hold_chat: hold,
        ..Script::default()
    });
    let calls: Calls = Arc::default();
    let seen = calls.clone();
    let tools: ToolRunner = Arc::new(move |_, heard, name, args| {
        seen.lock().unwrap().push((name, args, heard));
        Box::pin(async { json!({"ok": true}) })
    });
    let (yes_place, no_place) = (ready.place.clone(), ready.place.clone());
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .taking_a_task_yes(Arc::new(move |request, words: &str| {
        tasks::yes_taken(&yes_place, request, words)
    }))
    .declining_a_task_waiting(Arc::new(move |request, words: &str| {
        tasks::declined(&no_place, request, words)
    }));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for input in input() {
                engine.submit(input);
            }
            turn_within(&mut engine, Duration::from_secs(10)).await
        });
    let ran = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(name, ..)| name.clone())
        .collect();
    let asked = script.asked.lock().unwrap().clone();
    Went { ran, asked }
}

fn spoken_line() -> Vec<ProviderInput> {
    vec![start(2), audio(2, 1), end(2)]
}

/// The ops the tasks part was sent, waiting for at least `least` of them.
fn ops(ready: &Prepared, least: usize) -> Vec<String> {
    let mut ops: Vec<String> = Vec::new();
    while ops.len() < least {
        match ready.seen.recv_timeout(PEER_WAIT) {
            Ok(line) => ops.push(line["op"].as_str().unwrap().to_owned()),
            Err(_) => break,
        }
    }
    ops.extend(
        ready
            .seen
            .try_iter()
            .map(|line| line["op"].as_str().unwrap().to_owned()),
    );
    ops
}

fn told_yes(went: &Went) -> bool {
    went.asked
        .iter()
        .flatten()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .any(|content| content.contains("handed over") && content.contains(tasks::CONFIRM))
}

/// Shape A: "Yes." heard whole after the read-back, and the chat round
/// never begins, so the turn ends unanswered. The task is started all the
/// same, once, before the round was asked.
#[test]
fn a_plain_yes_starts_the_task_though_the_chat_round_never_begins() {
    let ready = prepared("a", confirms());
    let went = went(&ready, "Yes.", spoken_line, None);
    assert!(went.ran.is_empty(), "{:?}", went.ran);
    assert_eq!(
        ops(&ready, 4),
        ["status", "prepare", "status", "confirm"],
        "started once"
    );
    assert_eq!(fake::pending_id(), None, "nothing waits after it");
}

/// Shape B: "Yes." heard, and the model answers in words with no call. The
/// task is started in code, once, and the round is told so.
#[test]
fn a_plain_yes_starts_the_task_though_the_model_calls_nothing() {
    let ready = prepared("b", confirms());
    let went = went(
        &ready,
        "Yes.",
        spoken_line,
        Some(vec![vec![
            text("Sure, I'm giving that to Claude Code. I'll tell you when it's done."),
            Ok(ChatEvent::Done),
        ]]),
    );
    assert!(went.ran.is_empty(), "{:?}", went.ran);
    assert!(told_yes(&went), "the round is told it was handed over");
    assert_eq!(ops(&ready, 4), ["status", "prepare", "status", "confirm"]);
    assert_eq!(fake::pending_id(), None);
}

/// A plain "No." drops the task in code (#6582) before the round: nothing
/// is confirmed, the tasks part is asked to cancel it, and nothing waits.
#[test]
fn a_plain_no_drops_the_task_in_code() {
    let ready = prepared(
        "no",
        vec![
            (
                "request.sock",
                json!({"ok":true,"store_id":"s1","revision":1,"summary":{"session_id":"sess"}}),
            ),
            ("request.sock", json!({"ok":true,"revision":2})),
        ],
    );
    let went = went(&ready, "No.", spoken_line, None);
    assert!(went.ran.is_empty(), "{:?}", went.ran);
    assert_eq!(ops(&ready, 4), ["status", "prepare", "status", "cancel"]);
    assert_eq!(fake::pending_id(), None);
}

/// More than a plain yes goes to the model, as before: nothing is started
/// in code, and the task still waits for the model's confirm.
#[test]
fn a_yes_with_more_in_it_goes_to_the_model() {
    let ready = prepared("more", confirms());
    let went = went(
        &ready,
        "Yes, but make it six fruits.",
        spoken_line,
        Some(vec![vec![text("Six fruits it is."), Ok(ChatEvent::Done)]]),
    );
    assert!(!went.asked.is_empty(), "the model is asked");
    assert!(!told_yes(&went));
    assert_eq!(ops(&ready, 2), ["status", "prepare"], "nothing confirmed");
    assert_eq!(fake::pending_id(), Some(4));
}

/// A "Yes." begun over the lamp's own voice may be its own words heard
/// back: never taken in code, so the task still waits.
#[test]
fn a_yes_over_the_lamps_voice_is_not_taken_in_code() {
    let ready = prepared("over", confirms());
    let over = || {
        let mut began = start(2);
        if let ProviderInput::Start {
            over_the_lamps_voice,
            ..
        } = &mut began
        {
            *over_the_lamps_voice = true;
        }
        vec![began, audio(2, 1), end(2)]
    };
    let went = went(
        &ready,
        "Yes.",
        over,
        Some(vec![vec![text("Okay."), Ok(ChatEvent::Done)]]),
    );
    assert!(!told_yes(&went));
    assert_eq!(ops(&ready, 2), ["status", "prepare"], "nothing confirmed");
    assert_eq!(fake::pending_id(), Some(4));
}

/// A "Yes." typed in the app is the model's to judge, never taken in code.
#[test]
fn a_typed_yes_is_not_taken_in_code() {
    let ready = prepared("typed", confirms());
    let went = went(
        &ready,
        "",
        || vec![written(2, "Yes.")],
        Some(vec![vec![text("Okay."), Ok(ChatEvent::Done)]]),
    );
    assert!(!told_yes(&went));
    assert_eq!(ops(&ready, 2), ["status", "prepare"], "nothing confirmed");
    assert_eq!(fake::pending_id(), Some(4));
}
