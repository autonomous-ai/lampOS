//! #7679 (`LAMP_MAKE_QUEUE`): a make that waited its turn runs with no turn
//! once the one before it is done. Its line goes through the say service,
//! once, as a cut make's does, and the queue is told once it was said.
use super::run_queued_with;
use crate::make_skill_local::tests::{chat_at, good, keeping, manage_helper, service};
use crate::tools::ToolAnswer;
use std::{sync::mpsc, time::Duration};

const REQUEST: &str = "make me a skill that dims the light at 10 pm";
const NOW: u64 = 1_800_000_000_000;
const LINE: &str = "I made evening dim, and it's on: when the lamp goes quiet, between 22:00 and 22:30, it dims the light a step, at most once every 12 hours, using its light and whether it is talking.";

/// The queued gold make, with the lamp's model and manage helper of the
/// test's own: made on the queue's thread, its line said, then ended.
#[test]
fn a_queued_make_says_its_line_through_the_say_service_once_then_frees_the_slot() {
    let (root, keeping) = keeping("queued-made-line");
    let (url, served) = service(vec![(200, good())]);
    let asked = manage_helper(&keeping.manage);
    let work: crate::make_queue::Work = Box::new(move || {
        let told = crate::make_skill_local::make_with(
            0,
            None,
            REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        ToolAnswer::plain(told)
    });
    let (say, said) = mpsc::channel();
    let (end, ended) = mpsc::channel();
    let (order, ordered) = mpsc::channel();
    let said_order = order.clone();
    run_queued_with(
        4,
        work,
        move |line| {
            said_order.send("said").unwrap();
            say.send(line).unwrap();
        },
        move || {
            order.send("ended").unwrap();
            end.send(()).unwrap();
        },
    );
    let line = said.recv_timeout(Duration::from_secs(20));
    assert!(
        line.as_deref().is_ok_and(|l| l.starts_with(LINE)),
        "{line:?}"
    );
    assert!(ended.recv_timeout(Duration::from_secs(5)).is_ok(), "ended");
    assert_eq!(ordered.recv().unwrap(), "said");
    assert_eq!(ordered.recv().unwrap(), "ended");
    // Said once.
    assert!(said.recv_timeout(Duration::from_millis(300)).is_err());
    assert_eq!(served.join().unwrap(), 1);
    assert!(asked.join().unwrap()["install_made"].is_string());
    let _ = std::fs::remove_dir_all(&root);
}

/// A make with nothing to say (its work panicked, or the brief refused it
/// before any line): the slot is let go all the same, never held for good.
#[test]
fn a_queued_make_that_says_nothing_still_frees_the_slot() {
    let (end, ended) = mpsc::channel();
    let (say, said) = mpsc::channel::<String>();
    run_queued_with(
        5,
        Box::new(|| ToolAnswer::plain(serde_json::json!({}))),
        move |line| say.send(line).unwrap(),
        move || end.send(()).unwrap(),
    );
    assert!(ended.recv_timeout(Duration::from_secs(5)).is_ok());
    assert!(said.recv_timeout(Duration::from_millis(300)).is_err());
    let (end, ended) = mpsc::channel();
    run_queued_with(
        6,
        Box::new(|| panic!("the make's thread failed")),
        |_| {},
        move || end.send(()).unwrap(),
    );
    assert!(
        ended.recv_timeout(Duration::from_secs(5)).is_ok(),
        "a panic too"
    );
}

/// A waiting make that turned on a core skill already doing it (#7763)
/// made no line of its own: the line its answer gives is said, once, then
/// the slot let go. Without it the person, told "I'll make it next", would
/// hear nothing while a skill went on.
#[test]
fn a_queued_make_that_turned_on_a_core_skill_says_its_line() {
    const ON: &str = "Weather is on now. Say turn off weather to undo it.";
    let (say, said) = mpsc::channel();
    let (end, ended) = mpsc::channel();
    run_queued_with(
        7,
        Box::new(|| {
            ToolAnswer::plain(serde_json::json!({
                "turned_on": "weather",
                "next": format!("Say only this line, once, in the conversation's language: \"{ON}\" Call nothing."),
            }))
        }),
        move |line| say.send(line).unwrap(),
        move || end.send(()).unwrap(),
    );
    assert_eq!(said.recv_timeout(Duration::from_secs(5)).as_deref(), Ok(ON));
    assert!(ended.recv_timeout(Duration::from_secs(5)).is_ok());
    assert!(said.recv_timeout(Duration::from_millis(300)).is_err());
}

/// A waiting make that failed is always said: the person was told it comes
/// next, and every line of theirs since is a going on, which silences a
/// cut make's failure (lamp-9adf, 17 Nov 18:10), never a queued one's.
#[test]
fn a_queued_make_that_failed_is_said_though_the_person_went_on() {
    const FAILED: &str = "I couldn't make the gold one.";
    let (say, said) = mpsc::channel();
    let (end, ended) = mpsc::channel();
    run_queued_with(
        8,
        Box::new(|| {
            crate::make_skill_local::set_said_outcome(
                FAILED.to_owned(),
                crate::make_skill_local::Outcome::Failed,
            );
            ToolAnswer::plain(serde_json::json!({"error": "not_made"}))
        }),
        move |line| say.send(line).unwrap(),
        move || end.send(()).unwrap(),
    );
    assert_eq!(
        said.recv_timeout(Duration::from_secs(5)).as_deref(),
        Ok(FAILED)
    );
    assert!(ended.recv_timeout(Duration::from_secs(5)).is_ok());
}

/// A waiting make taken from the queue as its thread takes it, asked by
/// the owner with `asked` as its line: `work` wraps the real run path.
fn queued_entry(request: u64, work: crate::make_queue::Work) -> crate::make_queue::Entry {
    use crate::make_queue::{Admitted, Asker, admit, clear_for_test, said, step};
    clear_for_test();
    crate::skill_write_yes::clear_for_test();
    let now = std::time::Instant::now();
    let idle = || ToolAnswer::plain(serde_json::json!({}));
    assert!(matches!(
        admit(
            request - 1,
            Asker::default(),
            "first".into(),
            now,
            Box::new(idle)
        ),
        Admitted::Run
    ));
    let asker = Asker {
        speaker: Some("ann".into()),
        owner: Some("ann".into()),
        heard: Some("check the gold price every evening at six".into()),
    };
    assert!(matches!(
        admit(request, asker, "gold price".into(), now, work),
        Admitted::Queued { .. }
    ));
    said(request - 1);
    step(now, false, false).run.expect("the gold one runs")
}

/// The pre-push read's P1: a waiting make that failed with no line of its
/// own (a brief refused, the part not running) says one failed line, by
/// its label, through the real run path (`Entry::run` and
/// `answer_keeping_news`, heard, no lease), held to the owner and line kept
/// at its ask; then the slot is let go.
#[test]
fn a_queued_make_that_failed_with_no_line_says_one_through_the_real_path() {
    if crate::person::private_room() {
        return;
    }
    let make = |request: &str| lamp_gemini::ToolCall {
        id: "c1".into(),
        name: crate::made_skill::MAKE.into(),
        args: serde_json::json!({ "request": request, "label": "gold price" }),
    };
    let on = crate::tools::Settings {
        make_skill: true,
        ..crate::tools::Settings::default()
    };
    let off = crate::tools::Settings::default();
    let too_long = "gold ".repeat(crate::made_skill::MOST_REQUEST_CHARS);
    for (request, settings, call, line) in [
        (
            7_679_201,
            on,
            make(&too_long),
            "I couldn't make gold price: that is too long for me to make as one skill: please say it more shortly.",
        ),
        (
            7_679_203,
            off,
            make("check the gold price every evening at six"),
            "I couldn't make gold price: that part of me is not running right now, so I could not do it.",
        ),
    ] {
        let real = super::queued_work(settings, call, request);
        let kept = std::sync::Arc::new(std::sync::Mutex::new(None));
        let seen = kept.clone();
        let entry = queued_entry(
            request,
            Box::new(move || {
                *seen.lock().unwrap() = Some(crate::make_skill_local::kept_for_test());
                real()
            }),
        );
        let (say, said) = mpsc::channel();
        let (end, ended) = mpsc::channel();
        run_queued_with(
            request,
            Box::new(move || entry.run()),
            move |line| say.send(line).unwrap(),
            move || end.send(()).unwrap(),
        );
        assert_eq!(
            said.recv_timeout(Duration::from_secs(10)).as_deref(),
            Ok(line),
            "{request}"
        );
        assert!(ended.recv_timeout(Duration::from_secs(5)).is_ok());
        assert!(
            said.recv_timeout(Duration::from_millis(300)).is_err(),
            "once"
        );
        assert_eq!(
            kept.lock().unwrap().clone(),
            Some((
                Some("ann".to_owned()),
                Some("check the gold price every evening at six".to_owned())
            ))
        );
    }
}

/// The unclear ask with `LAMP_MAKE_NO_ASKS` (`make_no_asks::NOT_ACTED`),
/// whose real path needs the making model: its answer, run as a waiting
/// make, says one failed line.
#[test]
fn a_queued_make_not_acted_on_says_one_failed_line() {
    let entry = queued_entry(
        7_679_211,
        Box::new(|| {
            ToolAnswer::plain(serde_json::json!({
                "error": crate::make_no_asks::NOT_ACTED,
                "next": crate::make_no_asks::NOT_ACTED_NEXT
            }))
        }),
    );
    let (say, said) = mpsc::channel();
    let (end, ended) = mpsc::channel();
    run_queued_with(
        7_679_211,
        Box::new(move || entry.run()),
        move |line| say.send(line).unwrap(),
        move || end.send(()).unwrap(),
    );
    assert_eq!(
        said.recv_timeout(Duration::from_secs(5)).as_deref(),
        Ok("I couldn't make gold price: it wasn't clear what to make.")
    );
    assert!(ended.recv_timeout(Duration::from_secs(5)).is_ok());
    assert!(
        said.recv_timeout(Duration::from_millis(300)).is_err(),
        "once"
    );
}
