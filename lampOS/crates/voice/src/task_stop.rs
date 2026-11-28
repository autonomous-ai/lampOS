//! "Stop that task": the voice's half (`stop_agent_task`).
//!
//! The person asks the lamp to stop a piece of work handed over that is
//! still going ("stop that task", "cancel the poem one"). The model calls
//! [`STOP`] with the person's words for it (`which`) or its work id
//! (`task`). The voice finds the one meant among the work going now (the
//! tasks part's `tasks_now` record and each task's ask), and sends the
//! tasks part's `stop`:
//!
//! - request `{"op":"stop","guard":<as cancel>,"id":<task id>}`;
//! - answer `{"id":<id>,"stop":"stopped"|"unconfirmed"|"not_started"|"remote"|"cannot_stop"|"not_running"}`,
//!   at the top level and again in `data`; `busy` while the task is being
//!   handed over. The tasks part may take up to 32 s to answer: the lamp
//!   speaks within [`STOP_WAIT`], and the answer is read on by itself.
//!
//! "That one", or no words, means the only work going. Several going, and
//! words that don't single one out: the model is told to ask which one,
//! naming each by its own ask, shortened, never by its agent. None going:
//! nothing is running. What the lamp says is one short line for the
//! answer, from [`crate::skill_words`], said once: the model is told those
//! words and nothing else, so no claim is made the tasks part did not make.
//!
//! Work the person stopped ends with `stopped_by_person`. Its stop line
//! was said already, so the task news never tells it as stopped, and never
//! again ([`ended_quietly`], [`hand_over_ended_quietly`]). Work whose stop
//! was never confirmed and that then failed is told as a failure.
use crate::agent_tasks::{Place, ask, guard, label_of, nonce};
use crate::skill_words::{
    STOP_BUSY_LINE, STOP_CANNOT_STOP_LINE, STOP_COULD_NOT_LINE, STOP_NOT_RUNNING_LINE,
    STOP_NOT_STARTED_LINE, STOP_NOTHING_RUNNING_LINE, STOP_REMOTE_LINE, STOP_STOPPED_LINE,
    STOP_UNCONFIRMED_LINE,
};
use lamp_gemini::Tool;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

/// The model's function.
pub const STOP: &str = "stop_agent_task";
/// [`STOP`]'s parameter for the person's words for the work.
pub const WHICH: &str = "which";
/// [`STOP`]'s parameter for the work's id, as its hand-over answered it.
pub const TASK: &str = "task";
/// The reason, or code, a task the person stopped ends with (lamp-tasks).
pub const STOPPED_BY_PERSON: &str = "stopped_by_person";

/// The longest the person waits for the stop's answer before the lamp
/// speaks. The tasks part answers `stopped` only once the engine confirmed
/// it, and may take up to its `STOP_REQUEST_DEADLINE` (32 s): the voice
/// never holds the turn that long. Past this, the lamp says the stop was
/// asked and is not confirmed yet ([`STOP_UNCONFIRMED_LINE`]), and the
/// exchange goes on by itself on its own thread.
pub const STOP_WAIT: Duration = Duration::from_secs(3);
/// How long that thread reads the `stop` answer: past the tasks part's own
/// 32 s, so its reply is never cut off and a stop is never left half-asked.
pub const STOP_READ: Duration = Duration::from_secs(33);
/// Time between one try and the next while the tasks part is not sure.
const AGAIN_AFTER: Duration = Duration::from_millis(100);

/// The states of `tasks_now` in which work is still going and can be
/// stopped: at work, starting, waiting for the person, or handed over and
/// not looked at yet.
const GOING: [&str; 5] = [
    "working",
    "starting",
    "installing",
    "needs_input",
    "unknown",
];

/// Words that never single out a piece of work: "stop that one", "cancel
/// the task", and the agents' names (work is never named by its agent).
const NOT_NAMING: [&str; 40] = [
    "the", "that", "this", "these", "those", "one", "ones", "task", "tasks", "work", "job", "stop",
    "cancel", "please", "and", "for", "with", "what", "which", "thing", "you", "your", "doing",
    "running", "agent", "can", "could", "just", "about", "now", "it's", "claude", "code", "codex",
    "devin", "opencode", "hermes", "openclaw", "harness", "lamp",
];

/// The tasks the voice asked to stop lately, by store and id: their end is
/// never told as news ([`ended_quietly`]). At most this many are kept.
const MOST_KEPT: usize = 64;
static STOPPED_HERE: Mutex<VecDeque<(String, u64)>> = Mutex::new(VecDeque::new());

/// A piece of work going now: its id and its ask, shortened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Going {
    pub id: u64,
    pub label: String,
}

/// Which work a stop is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    /// No work is going.
    Nothing,
    /// This one.
    One(u64),
    /// The person must say which of these.
    AskWhich(Vec<String>),
}

/// [`STOP`] as the model is given it.
pub fn tool() -> Tool {
    Tool {
        name: STOP.into(),
        description: "Stop a piece of work you handed over that is still going, when the \
                      person asks to stop or cancel it (\"stop that task\", \"cancel the poem \
                      one\", \"never mind the parser one\"). Never on your own, and never to \
                      stop your own speech. Say only what its answer tells you to, once."
            .into(),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                WHICH: {
                    "type": "string",
                    "description": "Their words for the work, if they named it (\"the poem one\"). Leave it out for \"that one\" or \"it\"."
                },
                TASK: {
                    "type": "string",
                    "description": "The work_id of the work, if its hand-over gave you one and they mean that work."
                }
            }
        })),
        silent: false,
    }
}

/// What the model is told to say: `line`, once, and nothing more.
fn say(line: &str) -> String {
    format!(
        "Say only this, once, in the conversation's language: \"{line}\" Add nothing else, and call nothing more."
    )
}

/// The line said for the tasks part's answer `outcome` to a stop; the
/// line for a refusal for any other.
pub(crate) fn line_for(outcome: &str) -> &'static str {
    match outcome {
        "stopped" => STOP_STOPPED_LINE,
        "unconfirmed" => STOP_UNCONFIRMED_LINE,
        "not_started" => STOP_NOT_STARTED_LINE,
        "remote" => STOP_REMOTE_LINE,
        "cannot_stop" => STOP_CANNOT_STOP_LINE,
        "not_running" => STOP_NOT_RUNNING_LINE,
        "busy" => STOP_BUSY_LINE,
        _ => STOP_COULD_NOT_LINE,
    }
}

/// What the model is told for the tasks part's answer `outcome`.
pub(crate) fn told(outcome: &str) -> Value {
    json!({"stop": outcome, "next": say(line_for(outcome))})
}

/// What the model is told when no work is going.
fn nothing_running() -> Value {
    json!({"stop": "nothing_running", "next": say(STOP_NOTHING_RUNNING_LINE)})
}

/// What the model is told when the person must say which work: each by
/// its own ask, never by its agent.
pub(crate) fn ask_which(labels: &[String]) -> Value {
    let named = labels
        .iter()
        .map(|label| format!("\"{label}\""))
        .collect::<Vec<_>>()
        .join(" or ");
    let next = if labels.len() == 1 {
        format!(
            "Ask them in one short question whether they mean the work {named}. Name it only by those words, never by its agent. Stop nothing until they say; then call stop_agent_task again."
        )
    } else {
        format!(
            "Ask them in one short question which one to stop: {named}. Name each only by those words, never by its agent. Stop nothing until they say; then call stop_agent_task again with which."
        )
    };
    json!({"which": labels, "next": next})
}

/// The content words of `text`: lowercase, three letters or more, none of
/// [`NOT_NAMING`].
fn content_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|word| word.trim_matches('\'').to_lowercase())
        .filter(|word| word.chars().count() >= 3 && !NOT_NAMING.contains(&word.as_str()))
        .collect()
}

/// How many of `said` words the work's `label` holds: a word alike, or one
/// beginning the other ("poem", "poems") from four letters.
fn score(said: &[String], label: &str) -> usize {
    let asked = content_words(label);
    said.iter()
        .filter(|word| {
            asked.iter().any(|known| {
                known == *word
                    || (known.len().min(word.len()) >= 4
                        && (known.starts_with(word.as_str()) || word.starts_with(known.as_str())))
            })
        })
        .count()
}

/// The work a stop is for, among `going`, from the person's words `which`.
pub(crate) fn choose(going: &[Going], which: Option<&str>) -> Choice {
    if going.is_empty() {
        return Choice::Nothing;
    }
    let said = content_words(which.unwrap_or_default());
    let labels = |chosen: &[&Going]| -> Vec<String> {
        chosen.iter().map(|work| work.label.clone()).collect()
    };
    if said.is_empty() {
        return match going {
            [only] => Choice::One(only.id),
            _ => Choice::AskWhich(labels(&going.iter().collect::<Vec<_>>())),
        };
    }
    let scored: Vec<(usize, &Going)> = going
        .iter()
        .map(|work| (score(&said, &work.label), work))
        .collect();
    let best = scored.iter().map(|(score, _)| *score).max().unwrap_or(0);
    if best == 0 {
        return Choice::AskWhich(labels(&going.iter().collect::<Vec<_>>()));
    }
    let top: Vec<&Going> = scored
        .iter()
        .filter(|(score, _)| *score == best)
        .map(|(_, work)| *work)
        .collect();
    match top.as_slice() {
        [only] => Choice::One(only.id),
        _ => Choice::AskWhich(labels(&top)),
    }
}

/// The ids of the tasks `record` (`tasks_now`) gives as going.
fn going_ids(record: &Value) -> Vec<u64> {
    record["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|task| {
            task["state"]
                .as_str()
                .is_some_and(|state| GOING.contains(&state))
        })
        .filter_map(|task| task["task"].as_u64())
        .collect()
}

/// The asks of tasks `ids` (`get_task`'s `text`) on `socket`, all asked at
/// once, each within `deadline`: the stop's wait is one exchange, never
/// one per task going.
fn asks_of(socket: &std::path::Path, ids: &[u64], deadline: Duration) -> BTreeMap<u64, String> {
    std::thread::scope(|scope| {
        let asked: Vec<_> = ids
            .iter()
            .map(|&id| {
                let handle = scope.spawn(move || {
                    ask(socket, &json!({"op":"get_task","id":id}), deadline)
                        .filter(|got| got.get("ok") == Some(&json!(true)))
                        .and_then(|got| got["text"].as_str().map(str::to_owned))
                });
                (id, handle)
            })
            .collect();
        asked
            .into_iter()
            .filter_map(|(id, handle)| Some((id, handle.join().ok()??)))
            .collect()
    })
}

/// The work going now, from the tasks part's `tasks_now` `record`, each
/// with its ask as `asked_of` gives it. A make handed to the lamp's agent is
/// not the person's work.
pub(crate) fn going_in(record: &Value, asked_of: impl Fn(u64) -> Option<String>) -> Vec<Going> {
    record["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|task| {
            task["state"]
                .as_str()
                .is_some_and(|state| GOING.contains(&state))
        })
        .filter_map(|task| {
            let id = task["task"].as_u64()?;
            let asked = asked_of(id).unwrap_or_default();
            if crate::made_skill::is_brief(&asked) {
                return None;
            }
            let label = label_of(&asked);
            Some(Going {
                id,
                label: if label.is_empty() {
                    "the work you handed over".to_owned()
                } else {
                    label
                },
            })
        })
        .collect()
}

/// Run [`STOP`] at `place`, within `deadline` for the reads; the stop's
/// answer is waited for at most [`STOP_WAIT`] ([`stop_aside`]). Whether
/// the tasks part answered.
///
/// On the way to the stop: `tasks_now`, then every going task's ask at
/// once ([`asks_of`]), then the stop, guarded from `tasks_now`'s own reply
/// (a stop needs no fresh revision, so no `status` read before it). A work
/// id from the model counts only when it is one of the work going: never a
/// make brief or work already over.
pub fn run(place: &Place, args: &Value, request: u64, deadline: Duration) -> (Value, bool) {
    let started = Instant::now();
    let left = || deadline.saturating_sub(started.elapsed());
    let socket = place.directory.join("request.sock");
    let named = match args.get(TASK) {
        Some(Value::String(said)) => said.trim().parse::<u64>().ok(),
        Some(Value::Number(said)) => said.as_u64(),
        _ => None,
    };
    let Some(record) = ask(&socket, &json!({"op":"tasks_now"}), left()) else {
        return (unreached(place), false);
    };
    if record.get("ok") != Some(&json!(true)) {
        eprintln!("lamp-voice: tasks: request {request}: {STOP} could not read the work going");
        return (told("refused"), true);
    }
    let asks = asks_of(&socket, &going_ids(&record), left());
    let going = going_in(&record, |id| asks.get(&id).cloned());
    let which = args.get(WHICH).and_then(Value::as_str);
    let choice = match named {
        Some(id) if going.iter().any(|work| work.id == id) => Choice::One(id),
        // Not one of the work going: the person is asked about what is.
        Some(id) => {
            eprintln!("lamp-voice: tasks: request {request}: {STOP}: task {id} is not work going");
            match choose(&going, which) {
                Choice::One(only) if content_words(which.unwrap_or_default()).is_empty() => {
                    Choice::AskWhich(
                        going
                            .iter()
                            .filter(|work| work.id == only)
                            .map(|work| work.label.clone())
                            .collect(),
                    )
                }
                choice => choice,
            }
        }
        None => choose(&going, which),
    };
    let id = match choice {
        Choice::Nothing => return (nothing_running(), true),
        Choice::AskWhich(labels) => {
            eprintln!(
                "lamp-voice: tasks: request {request}: {STOP}: {} pieces of work going, asked which",
                labels.len()
            );
            return (ask_which(&labels), true);
        }
        Choice::One(id) => id,
    };
    let store = record["store_id"].as_str().unwrap_or_default().to_owned();
    let Some(guard) = guard(&record, format!("voice-{request}-stop-{}", nonce())) else {
        return (told("refused"), true);
    };
    let line = json!({"op":"stop","guard":guard,"id":id});
    let outcome = stop_aside(socket, line, STOP_WAIT, move |outcome| {
        // Said or not, a stop asked is kept, so its end is never news.
        if matches!(outcome, "stopped" | "unconfirmed" | "not_started") {
            keep_stopped(&store, id);
        }
        eprintln!("lamp-voice: tasks: request {request}: {STOP}: task {id}: {outcome}");
    });
    (told(&outcome), true)
}

/// The tasks part's `stop` answer `answer` as one outcome: `stop` read at
/// the top level, else inside `data`. One it doesn't say, or a refusal, is
/// `refused`; `busy` (the task is being handed over right now) is its own;
/// a store not yet sure is `unconfirmed`. None (no answer in time) is
/// `unconfirmed`: the stop was sent.
pub(crate) fn outcome_of(answer: Option<&Value>) -> String {
    let Some(answer) = answer else {
        return "unconfirmed".to_owned();
    };
    let outcome = if answer.get("ok") == Some(&json!(true)) {
        answer
            .get("stop")
            .or_else(|| answer.get("data").and_then(|data| data.get("stop")))
            .and_then(Value::as_str)
            .unwrap_or("refused")
    } else {
        match answer["error"].as_str() {
            Some("busy") => "busy",
            Some("uncertain" | "timeout") => "unconfirmed",
            _ => "refused",
        }
    };
    if line_for(outcome) == STOP_COULD_NOT_LINE {
        "refused".to_owned()
    } else {
        outcome.to_owned()
    }
}

/// Send `line` (a `stop`) on a thread of its own, which reads its answer
/// for up to [`STOP_READ`] and asks again while the store is not sure
/// (`uncertain`, `timeout`), never on `busy`. A `conflict` (a tasks part
/// from before a stop needed no fresh revision, its guard stale) is asked
/// once more with a fresh guard from `status`. `done` gets the outcome
/// there. The outcome comes back here if it came within `wait`, else
/// `unconfirmed` at once: the voice's turn is never held for the engine.
fn stop_aside(
    socket: std::path::PathBuf,
    line: Value,
    wait: Duration,
    done: impl FnOnce(&str) + Send + 'static,
) -> String {
    let (sent, answered) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-task-stop".into())
        .spawn(move || {
            let started = Instant::now();
            let left = || STOP_READ.saturating_sub(started.elapsed());
            let mut line = line;
            let mut fresh = false;
            let answer = loop {
                let answer = ask(&socket, &line, left());
                let conflict = answer
                    .as_ref()
                    .is_some_and(|answer| answer["error"] == "conflict");
                if conflict && !fresh {
                    fresh = true;
                    let renewed =
                        ask(&socket, &json!({"op":"status"}), left()).and_then(|status| {
                            let id = line["guard"]["request_id"].as_str().unwrap_or("voice-stop");
                            guard(&status, format!("{id}-again"))
                        });
                    if let Some(renewed) = renewed {
                        line["guard"] = renewed;
                        continue;
                    }
                }
                let again = answer
                    .as_ref()
                    .and_then(|answer| answer["error"].as_str())
                    .is_some_and(|code| matches!(code, "uncertain" | "timeout"));
                if !again || left() <= AGAIN_AFTER {
                    break answer;
                }
                std::thread::sleep(AGAIN_AFTER);
            };
            let outcome = outcome_of(answer.as_ref());
            done(&outcome);
            let _ = sent.send(outcome);
        });
    if spawned.is_err() {
        return "refused".to_owned();
    }
    answered
        .recv_timeout(wait)
        .unwrap_or_else(|_| "unconfirmed".to_owned())
}

/// What the model is told when the tasks part did not answer: not set up
/// only while the file it needs is missing, as `check_agent_task` says.
fn unreached(place: &Place) -> Value {
    match &place.needs {
        Some(needs) if !needs.exists() => json!({"error": crate::agent_tasks::NOT_SET_UP}),
        _ => crate::tools::unavailable(),
    }
}

/// Keep task `id` of `store` as one the voice asked to stop.
pub(crate) fn keep_stopped(store: &str, id: u64) {
    if let Ok(mut kept) = STOPPED_HERE.lock() {
        if kept
            .iter()
            .any(|(known, kept_id)| known == store && *kept_id == id)
        {
            return;
        }
        if kept.len() >= MOST_KEPT {
            kept.pop_front();
        }
        kept.push_back((store.to_owned(), id));
    }
}

/// Whether the voice asked to stop task `id` of `store`.
fn stopped_here(store: &str, id: u64) -> bool {
    STOPPED_HERE.lock().is_ok_and(|kept| {
        kept.iter()
            .any(|(known, kept_id)| known == store && *kept_id == id)
    })
}

/// Whether `value` says it ended for [`STOPPED_BY_PERSON`]: its `code` or
/// `reason`, or the end's words.
fn says_stopped_by_person(value: &Value) -> bool {
    ["code", "reason", "answer"]
        .iter()
        .any(|key| value[*key].as_str().map(str::trim) == Some(STOPPED_BY_PERSON))
}

/// Whether the settled look `data` at task `id` of `store` is the end of
/// a stop the person asked for: news never tells it, or again. That is a
/// task ended `stopped_by_person`, or one this voice asked to stop that
/// ended `stopped`. Work the voice asked to stop that failed anyway (its
/// stop unconfirmed) is told as a failure, and work that finished anyway
/// as done.
pub fn ended_quietly(store: &str, id: u64, data: &Value) -> bool {
    // Only a stop: a stop never confirmed may not have landed, and a task
    // that then failed for its own reasons is told as any failure.
    says_stopped_by_person(data) || (data["state"] == "stopped" && stopped_here(store, id))
}

/// Whether the hand-over of list `item` of `store` ended because the
/// person stopped it: never told as a hand-over that failed.
pub fn hand_over_ended_quietly(store: &str, item: &Value) -> bool {
    says_stopped_by_person(&item["dispatch"])
        || item["id"]
            .as_u64()
            .is_some_and(|id| stopped_here(store, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_tasks::tests::tasks_part;
    use std::path::Path;

    const ENGINE_NAMES: [&str; 8] = [
        "Claude", "Codex", "Devin", "OpenCode", "Hermes", "OpenClaw", "Harness", "engine",
    ];

    fn going(id: u64, label: &str) -> Going {
        Going {
            id,
            label: label.into(),
        }
    }

    fn place(directory: &Path) -> Place {
        let flags: Vec<String> = ["--tasks-dir".to_owned(), directory.display().to_string()].into();
        crate::agent_tasks::take(&flags).unwrap().0.unwrap()
    }

    /// "Stop that task" with one piece of work going: that one, whatever
    /// words name nothing in particular.
    #[test]
    fn stop_one_running_task_is_that_task() {
        let only = [going(9, "Fix the parser")];
        for which in [None, Some("that one"), Some("stop it"), Some("the task")] {
            assert_eq!(choose(&only, which), Choice::One(9), "{which:?}");
        }
        // Its words single out one of several.
        let two = [
            going(9, "Fix the parser"),
            going(7, "Write a short poem about tea"),
        ];
        assert_eq!(choose(&two, Some("the poem one")), Choice::One(7));
        assert_eq!(choose(&two, Some("cancel the parser")), Choice::One(9));
        assert_eq!(choose(&[], None), Choice::Nothing);
    }

    /// Several going and no words that single one out: ask which, each by
    /// its own words, never by its agent.
    #[test]
    fn stop_several_running_asks_which_by_their_words() {
        let two = [
            going(9, "Fix the parser"),
            going(7, "Write a short poem about tea"),
        ];
        for which in [
            None,
            Some("that one"),
            Some("the Claude one"),
            Some("the garden"),
        ] {
            let Choice::AskWhich(labels) = choose(&two, which) else {
                panic!("{which:?}: not asked which");
            };
            assert_eq!(labels, ["Fix the parser", "Write a short poem about tea"]);
            let told = ask_which(&labels);
            let next = told["next"].as_str().unwrap();
            assert!(next.contains("\"Fix the parser\""), "{next}");
            assert!(next.contains("\"Write a short poem about tea\""), "{next}");
            assert!(next.contains("never by its agent"), "{next}");
            for engine in ENGINE_NAMES {
                assert!(!next.contains(engine), "{engine}: {next}");
            }
        }
    }

    /// Each answer of the tasks part's `stop` is its one line, and the
    /// model is told to say only that, once.
    #[test]
    fn stop_each_outcome_maps_to_its_one_line() {
        for (outcome, line) in [
            ("stopped", "Okay, I stopped it."),
            (
                "unconfirmed",
                "I asked it to stop, but I couldn't confirm it yet.",
            ),
            ("not_started", "Okay, I won't start it."),
            (
                "remote",
                "That one is running on your computer, and I can't stop it from here yet.",
            ),
            ("not_running", "That one had already finished."),
            (
                "cannot_stop",
                "Sorry, I can't stop that one right now. It'll keep going until it's done.",
            ),
            (
                "busy",
                "It's just getting started. Ask me again in a moment and I'll stop it.",
            ),
            ("something_new", "I couldn't stop it just now."),
        ] {
            assert_eq!(line_for(outcome), line, "{outcome}");
            let told = told(outcome);
            let next = told["next"].as_str().unwrap();
            assert!(next.contains(&format!("\"{line}\"")), "{next}");
            assert!(next.contains("Say only this, once"), "{next}");
            assert_eq!(next.matches(line).count(), 1, "{next}");
        }
    }

    /// No line the lamp says names an agent or a technical word.
    #[test]
    fn stop_no_line_names_an_engine() {
        let lines = [
            STOP_STOPPED_LINE,
            STOP_UNCONFIRMED_LINE,
            STOP_NOT_STARTED_LINE,
            STOP_REMOTE_LINE,
            STOP_CANNOT_STOP_LINE,
            STOP_NOT_RUNNING_LINE,
            STOP_NOTHING_RUNNING_LINE,
            STOP_COULD_NOT_LINE,
            STOP_BUSY_LINE,
        ];
        for line in lines {
            for word in
                ENGINE_NAMES
                    .iter()
                    .chain(&["task", "engine", "agent", "stopped_by_person", "_"])
            {
                assert!(
                    !line.to_lowercase().contains(&word.to_lowercase()),
                    "{word}: {line}"
                );
            }
        }
    }

    /// End to end against a fake tasks part: one piece of work going, a
    /// stop sent for it with its guard, and the answer said; several
    /// going, nothing sent; none, nothing running.
    #[test]
    fn stop_against_the_tasks_part_sends_stop_for_the_one_going() {
        let directory = std::env::temp_dir().join(format!("task-stop-one-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let seen = tasks_part(
            &directory,
            vec![
                (
                    "request.sock",
                    json!({"ok":true,"store_id":"s-stop","revision":4,"tasks":[
                        {"task":9,"agent":"claude","target":"office","state":"working"},
                        {"task":7,"agent":"codex","target":"office","state":"idle"}]}),
                ),
                (
                    "request.sock",
                    json!({"ok":true,"text":"Fix the parser","target":{"engine":"claude"}}),
                ),
                (
                    "request.sock",
                    json!({"ok":true,"data":{"id":9,"stop":"stopped"}}),
                ),
            ],
        );
        let (answered, ok) = run(&place(&directory), &json!({}), 5, Duration::from_secs(10));
        assert!(ok, "{answered}");
        assert_eq!(answered["stop"], "stopped");
        let sent: Vec<Value> = seen.try_iter().collect();
        assert_eq!(sent[0], json!({"op":"tasks_now"}));
        assert_eq!(sent[1], json!({"op":"get_task","id":9}));
        // The guard is `tasks_now`'s own: no `status` read before the stop.
        assert_eq!(sent[2]["op"], "stop");
        assert_eq!(sent[2]["id"], 9);
        assert_eq!(sent[2]["guard"]["store_id"], "s-stop");
        assert_eq!(sent[2]["guard"]["revision"], 4);
        // Its end is never news.
        assert!(ended_quietly(
            "s-stop",
            9,
            &json!({"state":"stopped","answer":""})
        ));
        std::fs::remove_dir_all(&directory).unwrap();

        let directory = std::env::temp_dir().join(format!("task-stop-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let _seen = tasks_part(
            &directory,
            vec![(
                "request.sock",
                json!({"ok":true,"tasks":[{"task":7,"agent":"codex","target":"office","state":"idle"}]}),
            )],
        );
        let (answered, _) = run(&place(&directory), &json!({}), 6, Duration::from_secs(10));
        assert_eq!(answered["stop"], "nothing_running");
        assert!(
            answered["next"]
                .as_str()
                .unwrap()
                .contains(STOP_NOTHING_RUNNING_LINE)
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// The tasks part's answer: `stop` at the top level first, else in
    /// `data`; `busy` (being handed over) is its own line, never stopped;
    /// a store not sure, or no answer in time, is not confirmed.
    #[test]
    fn stop_answer_reads_the_top_level_then_data_and_busy_is_never_stopped() {
        let both = json!({"ok":true,"id":9,"stop":"not_running","data":{"id":9,"stop":"stopped"}});
        assert_eq!(outcome_of(Some(&both)), "not_running");
        let in_data = json!({"ok":true,"data":{"id":9,"stop":"remote"}});
        assert_eq!(outcome_of(Some(&in_data)), "remote");
        let busy = json!({"ok":false,"error":"busy"});
        assert_eq!(outcome_of(Some(&busy)), "busy");
        assert_ne!(line_for("busy"), STOP_STOPPED_LINE);
        let unsure = json!({"ok":false,"error":"uncertain"});
        assert_eq!(outcome_of(Some(&unsure)), "unconfirmed");
        assert_eq!(outcome_of(None), "unconfirmed");
        let refused = json!({"ok":false,"error":"bad_request"});
        assert_eq!(outcome_of(Some(&refused)), "refused");
        let unknown = json!({"ok":true,"stop":"exploded"});
        assert_eq!(outcome_of(Some(&unknown)), "refused");
    }

    /// The tasks part may take 32 s to answer a stop: its answer is read for
    /// at least 33 s, on a thread of its own, and the lamp speaks within
    /// the wait, saying the stop is not confirmed yet; the late answer is
    /// still read whole.
    #[test]
    fn stop_never_holds_the_turn_for_a_slow_engine() {
        assert!(STOP_READ >= Duration::from_secs(33));
        assert!(STOP_WAIT < STOP_READ);
        let directory = std::env::temp_dir().join(format!("task-stop-slow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("request.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            let mut stream = listener.incoming().next().unwrap().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap();
            std::thread::sleep(Duration::from_millis(800));
            writeln!(stream, "{}", json!({"ok":true,"id":4,"stop":"stopped"})).unwrap();
        });
        let (late, heard) = std::sync::mpsc::channel();
        let started = Instant::now();
        let said = stop_aside(
            socket,
            json!({"op":"stop","id":4}),
            Duration::from_millis(100),
            move |outcome| late.send(outcome.to_owned()).unwrap(),
        );
        assert!(
            started.elapsed() < Duration::from_millis(600),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(said, "unconfirmed");
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)).unwrap(),
            "stopped"
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// A stop the person asked for ends quietly: its end is never told as
    /// a stop or a failure; an ordinary stop still is, and work stopped
    /// here that finished anyway is told as done.
    #[test]
    fn stop_news_stays_silent_on_stopped_by_person() {
        let by_person = json!({"state":"failed","code":STOPPED_BY_PERSON,"answer":""});
        assert!(ended_quietly("s1", 41, &by_person));
        let by_reason = json!({"state":"stopped","reason":STOPPED_BY_PERSON});
        assert!(ended_quietly("s1", 41, &by_reason));
        let ordinary = json!({"state":"stopped","answer":"I could not reach the repository."});
        assert!(!ended_quietly("s1", 42, &ordinary));
        keep_stopped("s1", 43);
        assert!(ended_quietly("s1", 43, &ordinary));
        assert!(!ended_quietly("s2", 43, &ordinary));
        assert!(!ended_quietly(
            "s1",
            43,
            &json!({"state":"idle","answer":"Done."})
        ));
        assert!(hand_over_ended_quietly(
            "s1",
            &json!({"id":50,"dispatch":{"state":"not_sent","code":STOPPED_BY_PERSON}})
        ));
        assert!(!hand_over_ended_quietly(
            "s1",
            &json!({"id":51,"dispatch":{"state":"not_sent","code":"expired"}})
        ));
    }

    /// A stop the engine never confirmed, then the task really failed: that
    /// failure is told as any failure, never dropped. Only a task that ended
    /// stopped stays quiet; one that finished is told as done.
    #[test]
    fn stop_a_failure_after_an_unconfirmed_stop_is_told() {
        keep_stopped("s-unconfirmed", 44);
        let failed = json!({"state":"failed","answer":"I could not reach the repository."});
        assert!(!ended_quietly("s-unconfirmed", 44, &failed));
        assert!(ended_quietly(
            "s-unconfirmed",
            44,
            &json!({"state":"stopped","answer":""})
        ));
        assert!(!ended_quietly(
            "s-unconfirmed",
            44,
            &json!({"state":"idle","answer":"Done."})
        ));
    }

    /// A fake tasks part answering each line by its op, each connection on
    /// a thread of its own (so asks sent at once are answered at once):
    /// `tasks_now` with `record`, `get_task` with `asks` after `slow`,
    /// `status` with the store, and each `stop` with the next of `stops`.
    fn by_op(
        directory: &Path,
        record: Value,
        asks: Vec<(u64, &'static str)>,
        slow: Duration,
        stops: Vec<Value>,
    ) -> std::sync::mpsc::Receiver<(Value, Instant)> {
        use std::io::{BufRead, Write};
        let listener =
            std::os::unix::net::UnixListener::bind(directory.join("request.sock")).unwrap();
        let (sent, seen) = std::sync::mpsc::channel();
        let stops = std::sync::Arc::new(Mutex::new(VecDeque::from(stops)));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let (record, asks, stops, sent) =
                    (record.clone(), asks.clone(), stops.clone(), sent.clone());
                std::thread::spawn(move || {
                    let mut line = String::new();
                    std::io::BufReader::new(&stream)
                        .read_line(&mut line)
                        .unwrap();
                    let asked: Value = serde_json::from_str(&line).unwrap();
                    let _ = sent.send((asked.clone(), Instant::now()));
                    let answer = match asked["op"].as_str().unwrap() {
                        "tasks_now" => record,
                        "get_task" => {
                            std::thread::sleep(slow);
                            asks.iter()
                                .find(|(id, _)| Some(*id) == asked["id"].as_u64())
                                .map_or(
                                    json!({"ok":false,"error":"not_found"}),
                                    |(_, text)| json!({"ok":true,"text":text}),
                                )
                        }
                        "status" => json!({"ok":true,"store_id":"s-by-op","revision":9}),
                        "stop" => stops
                            .lock()
                            .unwrap()
                            .pop_front()
                            .unwrap_or(json!({"ok":false,"error":"bad_request"})),
                        _ => json!({"ok":false,"error":"bad_request"}),
                    };
                    let _ = writeln!(stream, "{answer}");
                });
            }
        });
        seen
    }

    fn fresh(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// A work id from the model stops only work in the voice's own going
    /// set: never a make brief, never work already over. Named so, the
    /// person is asked about the work that is going; nothing is stopped.
    #[test]
    fn stop_a_named_id_only_among_the_work_going() {
        let record = json!({"ok":true,"store_id":"s-by-op","revision":3,"tasks":[
            {"task":9,"agent":"openclaw","target":"lamp","state":"working"},
            {"task":7,"agent":"claude","target":"office","state":"working"},
            {"task":5,"agent":"codex","target":"office","state":"idle"}]});
        let asks = vec![
            (9, "Write one lamp skill (lampOS lamp-skills): a timer"),
            (7, "Fix the parser"),
            (5, "Write a short poem about tea"),
        ];
        for named in ["9", "5", "404"] {
            let directory = fresh(&format!("task-stop-named-{named}"));
            let seen = by_op(
                &directory,
                record.clone(),
                asks.clone(),
                Duration::ZERO,
                vec![],
            );
            let (answered, ok) = run(
                &place(&directory),
                &json!({"task": named}),
                8,
                Duration::from_secs(5),
            );
            assert!(ok, "{answered}");
            assert_eq!(
                answered["which"],
                json!(["Fix the parser"]),
                "{named}: {answered}"
            );
            let ops: Vec<Value> = seen
                .try_iter()
                .map(|(line, _)| line["op"].clone())
                .collect();
            assert!(!ops.contains(&json!("stop")), "{named}: {ops:?}");
            std::fs::remove_dir_all(&directory).unwrap();
        }
        // The one going, named: stopped, its guard from `tasks_now`'s reply.
        let directory = fresh("task-stop-named-7");
        let seen = by_op(
            &directory,
            record,
            asks,
            Duration::ZERO,
            vec![json!({"ok":true,"id":7,"stop":"stopped"})],
        );
        let (answered, _) = run(
            &place(&directory),
            &json!({"task":7}),
            8,
            Duration::from_secs(5),
        );
        assert_eq!(answered["stop"], "stopped", "{answered}");
        let sent: Vec<Value> = seen.try_iter().map(|(line, _)| line).collect();
        let stop = sent.iter().find(|line| line["op"] == "stop").unwrap();
        assert_eq!(stop["id"], 7);
        assert_eq!(stop["guard"]["store_id"], "s-by-op");
        assert_eq!(stop["guard"]["revision"], 3);
        assert!(!sent.iter().any(|line| line["op"] == "status"), "{sent:?}");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// Latency: the asks of the work going are read at once, not one after
    /// another, and the guard comes from `tasks_now`'s own reply, so the
    /// stop leaves within one `get_task`'s time of the record, whatever the
    /// number of tasks going.
    #[test]
    fn stop_reads_the_asks_of_the_work_going_at_once() {
        let slow = Duration::from_millis(150);
        let record = json!({"ok":true,"store_id":"s-by-op","revision":3,"tasks":[
            {"task":1,"agent":"claude","target":"office","state":"working"},
            {"task":2,"agent":"codex","target":"office","state":"working"},
            {"task":3,"agent":"hermes","target":"lamp","state":"working"}]});
        let directory = fresh("task-stop-at-once");
        let seen = by_op(
            &directory,
            record,
            vec![
                (1, "Fix the parser"),
                (2, "Write a poem about tea"),
                (3, "Plan my trip"),
            ],
            slow,
            vec![json!({"ok":true,"id":2,"stop":"stopped"})],
        );
        let started = Instant::now();
        let (answered, _) = run(
            &place(&directory),
            &json!({"which":"the poem one"}),
            8,
            Duration::from_secs(5),
        );
        let took = started.elapsed();
        assert_eq!(answered["stop"], "stopped", "{answered}");
        let sent: Vec<(Value, Instant)> = seen.try_iter().collect();
        let (_, stop_at) = sent.iter().find(|(line, _)| line["op"] == "stop").unwrap();
        let to_stop = stop_at.duration_since(started);
        eprintln!("task_stop latency: to the stop {to_stop:?}, whole call {took:?}");
        assert!(
            to_stop < slow * 2,
            "the asks were read one by one: {to_stop:?}"
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// A tasks part from before a stop needed no fresh revision answers a
    /// stale guard `conflict`: the voice reads `status` and asks once more,
    /// with a fresh guard and request id, and says what that answers.
    #[test]
    fn stop_a_conflict_is_asked_again_once_with_a_fresh_guard() {
        let record = json!({"ok":true,"store_id":"s-by-op","revision":3,"tasks":[
            {"task":7,"agent":"claude","target":"office","state":"working"}]});
        let directory = fresh("task-stop-conflict");
        let seen = by_op(
            &directory,
            record,
            vec![(7, "Fix the parser")],
            Duration::ZERO,
            vec![
                json!({"ok":false,"error":"conflict"}),
                json!({"ok":true,"id":7,"stop":"stopped"}),
                json!({"ok":false,"error":"conflict"}),
            ],
        );
        let (answered, _) = run(&place(&directory), &json!({}), 8, Duration::from_secs(5));
        assert_eq!(answered["stop"], "stopped", "{answered}");
        let sent: Vec<Value> = seen.try_iter().map(|(line, _)| line).collect();
        let stops: Vec<&Value> = sent.iter().filter(|line| line["op"] == "stop").collect();
        assert_eq!(stops.len(), 2, "{sent:?}");
        assert_eq!(stops[0]["guard"]["revision"], 3);
        assert_eq!(stops[1]["guard"]["revision"], 9);
        assert_ne!(
            stops[0]["guard"]["request_id"],
            stops[1]["guard"]["request_id"]
        );
        assert!(sent.iter().any(|line| line["op"] == "status"));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
