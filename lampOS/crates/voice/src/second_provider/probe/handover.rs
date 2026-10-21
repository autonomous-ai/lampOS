//! The hand-over probe (`cargo run --example probe_handover`, #1722): which
//! function the second route's model calls when the person asks the lamp
//! to have one of two agents do something. On lamp-4ace, with Claude Code
//! (`office-claude`) and Codex (`office-codex`) read from the tasks part
//! (#2297), the played asks of a poem went to `not_for_me` or were written
//! by the lamp itself, though a hand-over was asked for by name.
//!
//! Each line is asked as a spoken turn's first round asks it
//! (`second_provider::Engine`): the system message the worker builds
//! ([`worker::system_message`]: the spoken character, the time and place,
//! and the tasks' session note, [`crate::agent_tasks::noted`]), then the
//! notes beside the words (the staff's, [`worker::staff_noting`], with
//! `LAMP_STAFF_NOTE=on`, and the call's, [`worker::call_note`]), then the
//! words. The functions are the voice's own and the actions part's, with
//! the tasks' ([`crate::agent_tasks::tools`], `start_agent_task` among
//! them), `not_for_me`, the time elsewhere and the look, as a spoken turn
//! declares them. It is asked of the chat the worker chooses
//! ([`worker::services`]: `second.json`, the unit's `LAMP_SECOND_*`, and on
//! the lamps the relay with its own credential).
//!
//! Left out, as a fresh conversation has none of them: the memory's kept
//! things, the thread and its exchanges, the room's facts, the skills'
//! lines and the web search's functions.
//!
//! The staff is held as the tasks part would answer with two of them
//! ([`STAFF`]); the tasks' switches are the environment's
//! (`LAMP_TASKS_DIR`, `LAMP_TASKS_WHO`, `LAMP_TASKS_ASKED`, …). Nothing is
//! sent to the tasks part. The wording asked is the one this build has.
//!
//! It prints counts and sizes only: never the character, the note, a key,
//! a URL or what the model said.
use super::super::net::{self, Chat, ChatEvent, Message};
use super::super::worker;
use std::{collections::BTreeMap, io, path::Path, sync::Arc};

/// lamp-4ace's two, as `docs/tasks.md` gives them: `(alias, engine)`.
pub const STAFF: [(&str, &str); 2] = [("office-claude", "claude"), ("office-codex", "codex")];

/// A played line and who should get it: the `who` a right call gives, and
/// whether leaving `who` out is right too (the default, `LAMP_TASKS_TARGET`).
pub struct Line {
    pub said: &'static str,
    pub who: &'static str,
}

/// The played test's three asks on lamp-4ace (#1722, the first builder at
/// 02:11).
pub const LINES: [Line; 3] = [
    Line {
        said: "Hey Lamp, ask Claude Code to write a two-line poem about the rain.",
        who: "Claude Code",
    },
    Line {
        said: "Hey Lamp, can you ask Claude Code to write a short poem about the sea?",
        who: "Claude Code",
    },
    Line {
        said: "Hey Lamp, ask Codex to write a haiku about coffee.",
        who: "Codex",
    },
];

/// What a request did with a line.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// `start_agent_task` with the right `who` (or none, for the default).
    Handed,
    /// `start_agent_task` with another `who`.
    HandedWrong(String),
    NotForMe,
    /// Another function first.
    Other(String),
    /// No call: the model answered itself.
    Itself,
    Failed,
}

impl Outcome {
    fn shown(&self) -> String {
        match self {
            Self::Handed => "start_agent_task".into(),
            Self::HandedWrong(who) => format!("start_agent_task(who={who})"),
            Self::NotForMe => crate::tools::NOT_FOR_ME.into(),
            Self::Other(name) => name.clone(),
            Self::Itself => "self".into(),
            Self::Failed => "failed".into(),
        }
    }
}

/// The outcome of a request's first call, for a line whose right `who` is
/// `right` and whose default is `default`.
pub fn judged(calls: &[net::ToolCall], right: &str, default: &str) -> Outcome {
    let Some(call) = calls.first() else {
        return Outcome::Itself;
    };
    if call.name == crate::tools::NOT_FOR_ME {
        return Outcome::NotForMe;
    }
    if call.name != crate::agent_tasks::START {
        return Outcome::Other(call.name.clone());
    }
    match call.args.get("who").and_then(|who| who.as_str()) {
        Some(who) if who == right => Outcome::Handed,
        None if right == default => Outcome::Handed,
        Some(who) => Outcome::HandedWrong(who.to_owned()),
        None => Outcome::HandedWrong(format!("(left out: {default})")),
    }
}

/// The messages of a line's first round: the system message, the notes
/// beside the words, then the words.
pub fn messages(
    told: crate::second_provider::Told,
    staff: &crate::second_provider::StaffNote,
    said: &str,
) -> Vec<Message> {
    let mut messages = told.messages();
    let notes = staff(1, said).into_iter().chain(Some(
        worker::call_note(crate::attention::called_by_name_on(said, true)).to_owned(),
    ));
    messages.extend(notes.map(Message::system));
    messages.push(Message::user(said));
    messages
}

/// A request's first calls, and its words.
async fn asked(chat: &Chat, messages: &[Message]) -> net::Result<(Vec<net::ToolCall>, String)> {
    let mut stream = chat.send(messages).await?;
    let mut called = Vec::new();
    let mut words = String::new();
    while let Some(event) = stream.next().await? {
        match event {
            ChatEvent::ToolCalls(calls) if called.is_empty() => called = calls,
            ChatEvent::Text(text) => words.push_str(&text),
            _ => {}
        }
    }
    Ok((called, words))
}

/// What the played test b0152 asked before each hand-over: a long story,
/// told by the lamp itself (the first builder at 02:56 on #1722).
pub const STORY: &str = "Hey Lamp, tell me a story about a clever fox, at least eight sentences.";
/// With this `on`, each run first asks [`STORY`] as a line, and its real
/// answer is kept in the thread as the route keeps an exchange (the
/// person's words, then the lamp's), so each line after it is told it as
/// history ([`worker::system_message`]).
pub const HISTORY_ENVIRONMENT: &str = "PROBE_HANDOVER_HISTORY";

/// The thread holds one exchange: `said`, answered with `answer`.
pub fn keep_exchange(said: &str, answer: &str) {
    let now = std::time::Instant::now();
    if let Ok(mut thread) = crate::thread::THREAD.lock() {
        thread.forget();
        thread.person(said, true, now);
        thread.lamp(1, answer, now);
        thread.finished(1, now);
    }
}

/// `probe_handover PROVIDER_CONFIG SECOND_CONFIG [RUNS] [LABEL]`: each line
/// asked `runs` times, the counts printed with `label` (the wording this
/// build has: A, B or C).
pub async fn run(
    provider_config: &Path,
    second_config: &Path,
    runs: u32,
    label: &str,
) -> io::Result<()> {
    let flags = crate::agent_tasks::arguments_from_environment();
    let (Some(mut place), _) = crate::agent_tasks::take(&flags)? else {
        return Err(io::Error::other(
            "no tasks part in the environment (LAMP_TASKS_DIR): the hand-over isn't declared",
        ));
    };
    if !(place.who && place.asked) {
        return Err(io::Error::other(
            "the probe asks with LAMP_TASKS_WHO=on and LAMP_TASKS_ASKED=on, as lamp-4ace has them",
        ));
    }
    place.staff_note = crate::staff_note::take(&crate::staff_note::arguments_from_environment()).0;
    crate::agent_tasks::hold_staff(&STAFF);
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = worker::spoken_character(&provider.instruction(false)?, true, true);
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    let route =
        worker::RouteFlags::from_values(|name| std::env::var_os(name)).map_err(io::Error::other)?;
    let actions = std::env::var_os(crate::tools::ACTIONS_ENVIRONMENT)
        .filter(|dir| !dir.is_empty())
        .map(std::path::PathBuf::from);
    let mut settings = super::cache::settings();
    settings.exec = actions;
    settings.tasks = Some(place.clone());
    let places = settings.clock && settings.places;
    let declared = worker::with_look(worker::time_elsewhere(
        crate::tools::declared(&settings),
        places,
    ));
    let start = declared
        .iter()
        .find(|tool| tool.name == crate::agent_tasks::START)
        .map(|tool| tool.description.len());
    let count = declared.len();
    let declare: net::Declare = Arc::new(move || declared.clone());
    let credentials = std::env::var_os(crate::second_config::CREDENTIALS_ENV);
    let chat =
        worker::services(config, &provider, route, credentials.as_deref(), declare)?.chat_client();
    let time = crate::agent_tasks::noted(
        crate::local_time::told_asked_here(settings.clock, places),
        Some(&place),
    );
    let note = crate::agent_tasks::session_note(&place).map_or(0, |note| note.len());
    let history = std::env::var(HISTORY_ENVIRONMENT).is_ok_and(|value| value == "on");
    let forced = crate::agent_tasks::named_ask_on();
    if forced {
        chat.forcing(worker::named_asks(place.clone()));
    }
    let staff = worker::staff_noting(place);
    let default = "Claude Code";
    println!(
        "handover {label}: {count} functions, start_agent_task's description {} B, the note {note} B, staff_note={}, history={history}, named_ask={forced}, {} lines, {runs} runs each",
        start.map_or("absent".to_owned(), |len| len.to_string()),
        crate::staff_note::take(&crate::staff_note::arguments_from_environment()).0,
        LINES.len()
    );
    let mut counts: BTreeMap<usize, BTreeMap<Outcome, u32>> = BTreeMap::new();
    let mut stories = (0, 0);
    for _ in 0..runs {
        if history {
            let told = worker::system_message(&character, time.as_deref(), "", None, None);
            match asked(&chat, &messages(told, &staff, STORY)).await {
                Ok((calls, words)) => {
                    stories.0 += u32::from(calls.is_empty());
                    stories.1 += words.split_whitespace().count();
                    keep_exchange(STORY, words.trim());
                }
                Err(error) => {
                    println!("handover {label} story failed: {error}");
                    keep_exchange(STORY, "");
                }
            }
        }
        for (at, line) in LINES.iter().enumerate() {
            let told = worker::system_message(&character, time.as_deref(), "", None, None);
            let outcome = match asked(&chat, &messages(told, &staff, line.said)).await {
                Ok((calls, _)) => judged(&calls, line.who, default),
                Err(error) => {
                    println!("handover {label} line={} failed: {error}", at + 1);
                    Outcome::Failed
                }
            };
            *counts.entry(at).or_default().entry(outcome).or_default() += 1;
        }
    }
    if history {
        println!(
            "handover {label} stories: {}/{runs} told in words with no call, {} words on average",
            stories.0,
            stories.1 / runs.max(1) as usize
        );
    }
    let mut total = 0;
    for (at, line) in LINES.iter().enumerate() {
        let called = counts.remove(&at).unwrap_or_default();
        let right = called.get(&Outcome::Handed).copied().unwrap_or_default();
        total += right;
        let shown: Vec<String> = called
            .iter()
            .map(|(outcome, n)| format!("{}={n}", outcome.shown()))
            .collect();
        println!(
            "handover {label} line={} who={} right={right}/{runs}: {}",
            at + 1,
            line.who,
            shown.join(" ")
        );
    }
    println!(
        "handover {label} total right={total}/{}",
        runs * LINES.len() as u32
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> net::ToolCall {
        net::ToolCall {
            id: "1".into(),
            name: name.into(),
            args,
            extra: None,
        }
    }

    /// The story kept as the route keeps an exchange is told as history:
    /// the person's words, then the lamp's, after the system message.
    /// The thread is the process's one: it starts fresh, as the probe's
    /// own process does, since `keep_exchange` keeps the exchange as
    /// whoever an earlier test left at the lamp, and a forget it left
    /// pending would drop it before it is told. It is put back after.
    #[test]
    fn the_story_is_told_as_the_exchange_before_the_line() {
        let _one = crate::provider_worker::tests::one_thread();
        let before = std::mem::take(
            &mut *crate::thread::THREAD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        crate::thread::THREAD.clear_poison();
        crate::remember::clear_forgotten_thread(&mut crate::thread::Thread::new());
        keep_exchange(STORY, "Once there was a fox.");
        let told = worker::system_message("You are a lamp.", None, "", None, None);
        let history: Vec<(&str, Option<String>)> = told
            .history
            .iter()
            .map(|message| (message.role, message.words().map(str::to_owned)))
            .collect();
        *crate::thread::THREAD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = before;
        assert_eq!(
            history,
            vec![
                ("user", Some(STORY.to_owned())),
                ("assistant", Some("Once there was a fox.".to_owned()))
            ]
        );
    }

    #[test]
    fn a_line_is_judged_on_its_first_call_and_who_it_names() {
        let start = crate::agent_tasks::START;
        assert_eq!(judged(&[], "Codex", "Claude Code"), Outcome::Itself);
        assert_eq!(
            judged(
                &[call(crate::tools::NOT_FOR_ME, json!({}))],
                "Codex",
                "Claude Code"
            ),
            Outcome::NotForMe
        );
        assert_eq!(
            judged(
                &[call(start, json!({"task": "a haiku", "who": "Codex"}))],
                "Codex",
                "Claude Code"
            ),
            Outcome::Handed
        );
        assert_eq!(
            judged(
                &[call(start, json!({"task": "a poem"}))],
                "Claude Code",
                "Claude Code"
            ),
            Outcome::Handed
        );
        assert_eq!(
            judged(
                &[call(start, json!({"task": "a haiku"}))],
                "Codex",
                "Claude Code"
            ),
            Outcome::HandedWrong("(left out: Claude Code)".into())
        );
        assert_eq!(
            judged(&[call("the_time", json!({}))], "Codex", "Claude Code"),
            Outcome::Other("the_time".into())
        );
    }
}
