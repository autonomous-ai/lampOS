use super::*;
use crate::task_news::News;
use std::io::{BufRead as _, Write as _};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The switch for this thread while it lives, then as it was.
struct On;
impl On {
    fn new(on: bool) -> Self {
        set_on_here(Some(on));
        Self
    }
}
impl Drop for On {
    fn drop(&mut self) {
        set_on_here(None);
    }
}

/// A task's finished news, as `task_news` makes it for the model.
fn news(task: u64) -> News {
    News {
        key: format!("0123456789abcdef0123456789abcdef:{task}:2"),
        note: "(Nobody has spoken. News from work you handed over for the person. Claude Code \
               just finished the task they gave it: \"write the weekly report\". What it came \
               back with: \"Done: the report is in reports/week-47.md\". Tell them now.)"
            .into(),
        until: None,
        progress: None,
        finished: None,
        also: Vec::new(),
    }
}

/// A lamp-tasks of the test's own on `socket`: each line it is sent is
/// kept, and it answers `answers` after `after`.
fn lamp_tasks(socket: &Path, answers: Vec<Value>, after: Duration) -> Arc<Mutex<Vec<Value>>> {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&asked);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut line = String::new();
            std::io::BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap();
            kept.lock()
                .unwrap()
                .push(serde_json::from_str(&line).unwrap());
            std::thread::sleep(after);
            for answer in &answers {
                if writeln!(&stream, "{answer}").is_err() {
                    break;
                }
            }
        }
    });
    asked
}

/// A short folder of the test's own under `/tmp` (a socket's path is
/// short), removed when it is dropped.
struct Dir(PathBuf);
impl Dir {
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir() -> Dir {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = PathBuf::from(format!(
        "/tmp/na{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

/// Each tick's step until it is no longer a wait, at most 5 s: the clock
/// the steps are given is `at`, so the 10 s never passes here.
fn settled(wordings: &mut Wordings, news: &News, at: Instant) -> Step {
    for _ in 0..500 {
        let step = wordings.step(Some(news), at, true);
        if step != Step::Wait {
            return step;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Step::Wait
}

fn answered(words: &str) -> Vec<Value> {
    vec![
        json!({"ok": true, "started": true}),
        json!({"ok": true, "answer": words}),
    ]
}

/// What is handed to the agent is the note the model is given today, word
/// for word, after the lamp's brief, and nothing else of the news: no key,
/// no grant, no time past the 10 s.
#[test]
fn the_content_handed_is_the_models_note_word_for_word() {
    let _on = On::new(true);
    let dir = dir();
    let socket = dir.path().join("make.sock");
    let asked = lamp_tasks(
        &socket,
        answered("Your weekly report is done."),
        Duration::ZERO,
    );
    let mut wordings = Wordings::at(Some(socket));
    let news = news(7);
    let now = Instant::now();
    assert_eq!(wordings.step(Some(&news), now, true), Step::Wait);
    assert_eq!(
        settled(&mut wordings, &news, now),
        Step::Line("Your weekly report is done.".into())
    );
    let asked = asked.lock().unwrap();
    assert_eq!(asked.len(), 1);
    let line = &asked[0];
    assert_eq!(line["op"], "voice_ask");
    assert_eq!(line["grants"], json!([]));
    assert_eq!(line["within_ms"], json!(10_000));
    let ask = line["ask"].as_str().unwrap();
    assert_eq!(ask.strip_prefix(BRIEF), Some(news.note.as_str()));
    assert!(!ask.contains(&news.key));
    assert_eq!(line["news"], true);
    assert_eq!(line.as_object().unwrap().len(), 5);
}

/// The agent's line in time: said in the note's place, as its words, and
/// only for the very news it was worded for; the next news is worded anew.
#[test]
fn the_agents_line_in_time_is_said_in_the_notes_place() {
    let _on = On::new(true);
    let dir = dir();
    let socket = dir.path().join("make.sock");
    let _asked = lamp_tasks(
        &socket,
        answered("Oh, Claude Code just finished your report."),
        Duration::ZERO,
    );
    let mut wordings = Wordings::at(Some(socket));
    let news = news(7);
    let now = Instant::now();
    assert!(matches!(settled(&mut wordings, &news, now), Step::Line(_)));
    // In time, it stays the news's while the news waits for its moment.
    assert!(matches!(
        wordings.step(Some(&news), now + Duration::from_secs(60), true),
        Step::Line(_)
    ));
    let turn = wordings.take_line(&news).unwrap();
    assert!(
        turn.contains("«Oh, Claude Code just finished your report.»"),
        "{turn}"
    );
    assert_eq!(
        turn,
        spoken_turn("Oh, Claude Code just finished your report.")
    );
    // Taken once.
    assert_eq!(wordings.take_line(&news), None);
    // Told together with another task's: no longer the note it worded, so
    // the note is told.
    let socket = dir.path().join("again.sock");
    let _asked = lamp_tasks(&socket, answered("Your report is done."), Duration::ZERO);
    let mut wordings = Wordings::at(Some(socket));
    assert!(matches!(settled(&mut wordings, &news, now), Step::Line(_)));
    let mut together = news.clone();
    together.note.push_str(" And another.");
    assert_eq!(wordings.take_line(&together), None);
}

/// A slow agent: the news waits for its line at most 10 s from when it
/// was handed over, then the model's note is told at once, as today, and
/// a line that comes after is never said.
#[test]
fn a_slow_agent_past_10_s_tells_the_note_at_once() {
    let _on = On::new(true);
    let dir = dir();
    let socket = dir.path().join("make.sock");
    let _asked = lamp_tasks(&socket, answered("Too late."), Duration::from_secs(2));
    let mut wordings = Wordings::at(Some(socket));
    let news = news(7);
    let began = Instant::now();
    assert_eq!(wordings.step(Some(&news), began, true), Step::Wait);
    assert_eq!(
        wordings.step(
            Some(&news),
            began + FALLBACK_WITHIN - Duration::from_millis(1),
            true
        ),
        Step::Wait
    );
    assert_eq!(
        wordings.step(Some(&news), began + FALLBACK_WITHIN, true),
        Step::Own(LATE.into())
    );
    // Its line comes later: dropped.
    std::thread::sleep(Duration::from_millis(2_500));
    assert_eq!(
        wordings.step(Some(&news), began + FALLBACK_WITHIN, true),
        Step::Own(LATE.into())
    );
    assert_eq!(wordings.take_line(&news), None);
}

/// The agent down, or lamp-tasks not reached, or refusing (its switch off,
/// busy): the note at once, never a wait of 10 s.
#[test]
fn the_agent_down_tells_the_note_at_once() {
    let _on = On::new(true);
    let dir = dir();
    let news = news(7);
    let now = Instant::now();
    // No lamp-tasks at all.
    let mut wordings = Wordings::at(Some(dir.path().join("none.sock")));
    assert!(matches!(settled(&mut wordings, &news, now), Step::Own(_)));
    // No tasks part on this lamp.
    let mut wordings = Wordings::at(None);
    assert!(matches!(
        wordings.step(Some(&news), now, true),
        Step::Own(_)
    ));
    // Refused.
    let socket = dir.path().join("make.sock");
    let _asked = lamp_tasks(
        &socket,
        vec![json!({"ok": false, "error": "off"})],
        Duration::ZERO,
    );
    let mut wordings = Wordings::at(Some(socket));
    assert!(matches!(settled(&mut wordings, &news, now), Step::Own(_)));
    assert_eq!(wordings.take_line(&news), None);
}

/// `-`, an empty answer, more than one short line, or one for the owner
/// alone: the note at once.
#[test]
fn a_dash_or_empty_answer_tells_the_note_at_once() {
    let _on = On::new(true);
    let news = news(7);
    for (words, owner_alone) in [
        ("-", false),
        (" - ", false),
        ("**", false),
        ("Done. It is in the file. You can read it. Bye.", false),
        (&*"report ".repeat(40), false),
        ("Your report is done.", true),
    ] {
        let dir = dir();
        let socket = dir.path().join("make.sock");
        let mut answer = json!({"ok": true, "answer": words});
        if owner_alone {
            answer["owner_alone"] = json!(true);
        }
        let _asked = lamp_tasks(&socket, vec![answer], Duration::ZERO);
        let mut wordings = Wordings::at(Some(socket));
        let step = settled(&mut wordings, &news, Instant::now());
        assert!(matches!(step, Step::Own(_)), "{words:?}: {step:?}");
        assert_eq!(wordings.take_line(&news), None, "{words:?}");
    }
    // An answer with no words at all.
    let dir = dir();
    let socket = dir.path().join("make.sock");
    let _asked = lamp_tasks(
        &socket,
        vec![json!({"ok": true, "answer": ""})],
        Duration::ZERO,
    );
    let mut wordings = Wordings::at(Some(socket));
    assert!(matches!(
        settled(&mut wordings, &news, Instant::now()),
        Step::Own(_)
    ));
}

/// Off (no `news` in `LAMP_CORE_VIA_AGENT`): nothing is handed over and the
/// note is told as before. On, only a task's settled news goes: never a
/// sign-in's code, progress, or a note too long for an ask.
#[test]
fn off_or_other_news_hands_nothing_over() {
    let dir = dir();
    let socket = dir.path().join("make.sock");
    let asked = lamp_tasks(&socket, answered("Hi."), Duration::ZERO);
    let news = news(7);
    let now = Instant::now();
    {
        let _off = On::new(false);
        let mut wordings = Wordings::at(Some(socket.clone()));
        assert_eq!(wordings.step(Some(&news), now, true), Step::Tell);
        assert_eq!(wordings.take_line(&news), None);
    }
    let _on = On::new(true);
    let mut wordings = Wordings::at(Some(socket));
    let mut sign_in = news.clone();
    sign_in.key = "sign-in-3-code".into();
    assert!(crate::sign_in::is_news(&sign_in.key));
    assert_eq!(wordings.step(Some(&sign_in), now, true), Step::Tell);
    let mut progress = news.clone();
    progress.key = "0123456789abcdef0123456789abcdef:7:progress-1-2".into();
    assert_eq!(wordings.step(Some(&progress), now, true), Step::Tell);
    let mut long = news.clone();
    long.note = "word ".repeat(500);
    assert!(matches!(
        wordings.step(Some(&long), now, true),
        Step::Own(_)
    ));
    assert_eq!(wordings.step(None, now, true), Step::Tell);
    std::thread::sleep(Duration::from_millis(200));
    assert!(asked.lock().unwrap().is_empty());
}

/// The switch's value: `on`, or a list naming `news`.
#[test]
fn the_switch_turns_on_news_by_name() {
    assert!(turns_on(Some("on"), GROUP));
    assert!(turns_on(Some("pilot,news"), GROUP));
    assert!(turns_on(Some(" news "), GROUP));
    assert!(!turns_on(Some("pilot,nudges"), GROUP));
    assert!(!turns_on(Some("newsy"), GROUP));
    assert!(!turns_on(None, GROUP));
}

/// The journal says what was told and why, never the agent's words.
#[test]
fn the_journal_never_has_the_agents_words() {
    let line = Step::Line("Your secret report is done.".into());
    let said = journal(&line, true).unwrap();
    assert!(!said.contains("secret"), "{said}");
    assert!(
        journal(&line, false)
            .unwrap()
            .contains("not the one it worded")
    );
    assert!(
        journal(&Step::Own(LATE.into()), false)
            .unwrap()
            .contains(LATE)
    );
    assert_eq!(journal(&Step::Tell, false), None);
    assert_eq!(journal(&Step::Wait, false), None);
}

/// A person's ask came while the news was worded: lamp-tasks cancelled the
/// news ask (`lamp_tasks::voice_ask`), and the note is told, at once or
/// after their answer, as news waits today.
#[test]
fn a_persons_ask_first_tells_the_note() {
    let _on = On::new(true);
    let dir = dir();
    let socket = dir.path().join("make.sock");
    let _asked = lamp_tasks(
        &socket,
        vec![
            json!({"ok": true, "started": true}),
            json!({"ok": false, "error": "cancelled"}),
        ],
        Duration::ZERO,
    );
    let mut wordings = Wordings::at(Some(socket));
    let news = news(7);
    assert_eq!(
        settled(&mut wordings, &news, Instant::now()),
        Step::Own(CANCELLED.into())
    );
    assert_eq!(wordings.take_line(&news), None);
}

/// No wording begins while a conversation is going on (a turn open, or one
/// in the last 15 s): the news is told as before, and worded only once the
/// room is quiet; one already worded goes on.
#[test]
fn no_wording_starts_mid_conversation() {
    let _on = On::new(true);
    let dir = dir();
    let socket = dir.path().join("make.sock");
    let asked = lamp_tasks(&socket, answered("Your report is done."), Duration::ZERO);
    let mut wordings = Wordings::at(Some(socket));
    let news = news(7);
    let now = Instant::now();
    assert_eq!(wordings.step(Some(&news), now, false), Step::Tell);
    std::thread::sleep(Duration::from_millis(200));
    assert!(asked.lock().unwrap().is_empty());
    assert_eq!(wordings.step(Some(&news), now, true), Step::Wait);
    // Worded already: a conversation that starts doesn't stop it.
    let mut step = Step::Wait;
    for _ in 0..500 {
        step = wordings.step(Some(&news), now, false);
        if step != Step::Wait {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(step, Step::Line("Your report is done.".into()));
    assert_eq!(asked.lock().unwrap().len(), 1);
    // The conversation state, as the coordinator reads it.
    assert!(in_conversation(true, 100_000_000, 0));
    assert!(in_conversation(false, 10_000_000, 0));
    assert!(!in_conversation(false, 15_000_000, 0));
}
