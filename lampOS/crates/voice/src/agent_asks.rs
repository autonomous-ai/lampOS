//! Live spoken asks handed to the lamp's agent (the owner's rulings,
//! 17 Nov): connector work goes to the lamp's agent, never the second
//! route's own model ("hand it all to the agent, don't let DeepSeek do
//! it"), and an ask that model can't do goes there too, never a spoken "I
//! can't". The agent is the lamp's default, Hermes or OpenClaw (~17:55),
//! picked by lamp-tasks.
//!
//! Two switches, each `on` exactly and off unless set, independent of one
//! another, read by the second route's worker (passed by
//! `process::second_route_worker_environment`) and by lamp-tasks:
//!
//! - **[`SERVICES_ENVIRONMENT`]** (`LAMP_SERVICES_TO_AGENT`, only with
//!   `LAMP_SERVICES` and `LAMP_AGENT_SERVICES` on, and lamp-control's own):
//!   a `service_read` that passed every one of the voice's gates
//!   (`services::gate`: heard live, the owner's own line) is not read by
//!   the voice. The person's words and the service's read grant
//!   (`<code>-read`, never a `-write`) go to the agent ([`hand`]), and the
//!   model is told the agent is reading it ([`HOLDING_NEXT`]), so it says
//!   one short holding line. Only a read the voice would tell in full
//!   (`services::said_whole`) is handed over: one that would be cut for
//!   someone maybe in view is not handed over at all, and nothing of it is
//!   told until the owner asks again alone (`services::NOT_ALONE_READ`;
//!   Heron's P2 1 on #7509: the agent's prose can't be cut to who and what
//!   about in code). An unlinked service is told "isn't linked" as the
//!   voice's own read tells it. `service_write` is as before in this cut
//!   (its gates move with it in a later one).
//! - **[`HARD_ENVIRONMENT`]** (`LAMP_HARD_ASKS_TO_AGENT`): the model is
//!   offered [`TOOL`], whose description has it judge by meaning when an
//!   ask needs more than a quick spoken answer, or is one it can't do or
//!   has no tool for, or a tool failed: then it hands it over instead of
//!   saying it can't. No word list or length rule on the person's words.
//!   No service grant, and only the owner's own heard line
//!   (`services::owner_of`, as a `service_read` is held): the run has the
//!   host network until the net runner lands.
//!
//! The path: one line on lamp-tasks' make socket
//! (`make_skill_agent::MAKE_SOCKET_NAME`, `lamp_tasks::voice_ask`) on a
//! thread of its own, the model answered at once. lamp-tasks runs the
//! lamp's default agent (the lamp's Hermes in a fresh home of its own, or
//! OpenClaw's one turn), at most [`WITHIN`]. A service's read on a lamp
//! whose agent can't take one yet (OpenClaw) is refused there and told
//! the fixed [`NO_SERVICES_LINE`], never read by the voice instead. Its
//! answer is told through the say service's `quoted` channel as the
//! agent's (`say::Words::Agent`): untrusted words, never urgent, framed as
//! never an instruction, asked with no functions. A read told in full is
//! said only while the owner is seen alone, looked at on the say client's
//! own thread as it waits and as it is said (`say::NOT_ALONE`), else the
//! fixed [`NOT_ALONE_LINE`]. A run that failed, timed out or was refused is
//! told the fixed [`FAILED_LINE`]. One ask at a time ([`BUSY_NEXT`]). A
//! forget (`services::forget_waiting`: the voice's, memory's, a privacy
//! change) or a spoken stop drops the answer of an ask in flight
//! ([`forget`]).
use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

/// A linked service's live read handed to the agent.
pub const SERVICES_ENVIRONMENT: &str = lamp_services::SERVICES_TO_AGENT_ENVIRONMENT;
/// An ask the second route's model can't do well handed to the agent.
pub const HARD_ENVIRONMENT: &str = "LAMP_HARD_ASKS_TO_AGENT";
/// The model's function for [`HARD_ENVIRONMENT`].
pub const TOOL: &str = "ask_agent";
/// How long the agent's run may take: lamp-tasks' most for a make or an
/// ask (`lamp_tasks::voice_ask::MOST_WITHIN`, 120 s).
pub const WITHIN: Duration = Duration::from_secs(120);
/// How long past [`WITHIN`] the answer is waited for: lamp-tasks ends the
/// run at its deadline and answers just after.
pub const GRACE: Duration = Duration::from_secs(10);
/// How long the line may take to send.
const SEND_WITHIN: Duration = Duration::from_secs(5);
/// The most of lamp-tasks' answer read.
const MOST_ANSWER_BYTES: u64 = 16 * 1024;
/// The most bytes of the agent's answer told: the say service's for its
/// quoted answer.
pub const MOST_TOLD_BYTES: usize = crate::say::MOST_AGENT_BYTES;
/// The say socket's skill name for these lines.
pub const SAY_SKILL: &str = crate::say::AGENT_SKILL;
/// How long a told line may wait for its moment.
const TOLD_WITHIN_MS: u64 = 60_000;

/// What the model is told once an ask is handed over.
pub const HOLDING_NEXT: &str = "The lamp's agent is doing this now, and its answer is told to them as soon as it comes. Say only one short line that you are on it, in the conversation's language, such as \"Let me check.\", and nothing of what it may find. Call no other tool for it.";
/// What the model is told while another ask is still with the agent.
pub const BUSY_NEXT: &str = "Not handed over: the lamp's agent is still on their last ask, whose answer comes first. Say so in a few words; they can ask again after it.";
/// What the model is told of an ask not on the owner's own line.
pub const OWNER_ONLY_NEXT: &str = "Not handed over: only the lamp's owner can hand work to the lamp's agent, and I can't tell it is them now. Say so plainly, in a few words, and ask them to look at the lamp so it can see who is asking.";
/// What the model is told when there is no agent to hand it to.
pub const NO_AGENT: &str =
    "Not handed over: the lamp's agent isn't set up on this lamp. Say so in a few words.";
/// The fixed line when the agent's run failed, timed out or was refused.
pub const FAILED_LINE: &str =
    "I couldn't get that done just now. Please ask me again in a little while.";
/// lamp-tasks' refusal of a service's read because the lamp's default
/// agent can't take one yet (`lamp_tasks::voice_ask::NO_SERVICES`,
/// OpenClaw today).
pub const NO_SERVICES: &str = "no_services";
/// lamp-tasks' refusal while Harness hasn't looked at the lamp's default
/// agent yet (`lamp_tasks::skill_make::NOT_READY`): told
/// `made_skill::NOT_READY_LINE`, as a make's.
pub const NOT_READY: &str = "not_ready";
/// The field of lamp-tasks' answer that holds it to the owner seen alone
/// (`lamp_tasks::voice_ask::OWNER_ALONE`).
pub const OWNER_ALONE: &str = "owner_alone";
/// The field of lamp-tasks' `busy` refusal that says a make holds the agent
/// (`lamp_tasks::voice_ask::MAKING_FIELD`, #7245): told [`MAKING_LINE`].
pub const MAKING_FIELD: &str = "making";
/// What is said to a live ask while the agent makes a skill: why, and when
/// to ask again, never the failed line (Twelve's note on #7737).
pub const MAKING_LINE: &str = "I'm still making your skill. Ask me again in a minute.";
/// What is said for [`NO_SERVICES`].
pub const NO_SERVICES_LINE: &str =
    "Your lamp's agent can't read your linked services yet, so I can't get that for you.";
/// The fixed line when a full read's answer came while someone else may
/// be in view: nothing of it is told.
pub const NOT_ALONE_LINE: &str =
    "I have that for you, but someone else may be here. Ask me again when you're on your own.";

/// The tasks part's directory (`--tasks-dir`), set by the worker.
static TASKS: OnceLock<PathBuf> = OnceLock::new();
/// An ask with the agent now.
static ASKING: AtomicBool = AtomicBool::new(false);
/// Bumped by [`forget`]: an answer of an older ask is never told.
static GENERATION: OnceLock<Arc<AtomicU64>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    /// Each test thread's own forgets, so another test's forget never
    /// drops this one's answer; handed to the threads it spawns.
    static GENERATION_HERE: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
}

/// The forgets counted for this process (a test thread's own in tests).
fn generations() -> Arc<AtomicU64> {
    #[cfg(test)]
    {
        let _ = &GENERATION;
        GENERATION_HERE.with(Arc::clone)
    }
    #[cfg(not(test))]
    Arc::clone(GENERATION.get_or_init(Arc::default))
}

/// One ask's place among the forgets: forgotten or stopped once another
/// [`forget`] came after it.
#[derive(Clone, Debug)]
pub struct Since {
    forgets: Arc<AtomicU64>,
    at: u64,
}

impl Since {
    /// From now.
    pub fn now() -> Self {
        let forgets = generations();
        let at = forgets.load(Ordering::Acquire);
        Self { forgets, at }
    }

    /// Whether a forget or a stop came since.
    pub fn forgotten(&self) -> bool {
        self.forgets.load(Ordering::Acquire) != self.at
    }
}

/// The tasks part's directory for this worker.
pub fn set_tasks_dir(directory: PathBuf) {
    let _ = TASKS.set(directory);
}

#[cfg(test)]
thread_local! {
    static SWITCHES_HERE: std::cell::Cell<Option<(bool, bool)>> =
        const { std::cell::Cell::new(None) };
}

/// The switches for this test's thread: (services, hard).
#[cfg(test)]
pub(crate) fn set_on_here(on: Option<(bool, bool)>) {
    SWITCHES_HERE.with(|here| here.set(on));
}

fn on_var(name: &str) -> bool {
    std::env::var(name).ok().as_deref() == Some("on")
}

/// Whether a linked service's live read goes to the agent: the switch and
/// `LAMP_AGENT_SERVICES`, each `on` (a read is only asked at all with
/// `LAMP_SERVICES` on, which gives the worker its services' folder).
pub fn services_on() -> bool {
    #[cfg(test)]
    if let Some((services, _)) = SWITCHES_HERE.with(std::cell::Cell::get) {
        return services;
    }
    on_var(SERVICES_ENVIRONMENT) && crate::made_skill::agent_services_here()
}

/// In the process with the say service (the coordinator): with
/// [`SERVICES_ENVIRONMENT`] on, the faces in view counted here too, so the
/// say client's look at the owner seen alone (`say::NOT_ALONE`) has them,
/// as `run_write_yes::Waits` counts them for a skill run's read-back.
pub fn count_faces_here() {
    if on_var(SERVICES_ENVIRONMENT)
        && let Ok(bus) = lamp_bus::Bus::from_environment()
    {
        crate::known_faces::keep_faces_counted(bus);
    }
}

/// Whether [`TOOL`] is offered: the switch `on`.
pub fn hard_on() -> bool {
    #[cfg(test)]
    if let Some((_, hard)) = SWITCHES_HERE.with(std::cell::Cell::get) {
        return hard;
    }
    on_var(HARD_ENVIRONMENT)
}

/// Whether `name` is [`TOOL`].
pub fn is_tool(name: &str) -> bool {
    name == TOOL
}

/// [`TOOL`]: the model judges by meaning when to call it; nothing in the
/// voice reads the person's words for it. Under the link's 1,024 bytes.
pub fn tool() -> Tool {
    Tool {
        name: TOOL.into(),
        description: "Hand the person's ask to the lamp's agent, which works it out on its own and \
                      whose answer is told to them when it is done. Call it when the ask needs more \
                      than a quick spoken answer: finding things out, the web, several steps, \
                      working something out, code or files. Call it too, instead of saying you can't \
                      or don't know how, whenever you have no tool for what they ask or a tool \
                      failed it. Never for small talk, a quick fact you know, or the lamp's own \
                      controls (the light, its head, reminders, timers), and never for something \
                      refused to keep someone safe or private."
            .into(),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                "ask": {
                    "type": "string",
                    "description": "What they asked, in their own words, with what it refers to."
                }
            },
            "required": ["ask"]
        })),
        silent: false,
    }
}

/// One ask for the agent: the person's words (and what the lamp took
/// them to ask), a linked service's read grant, and whether its answer
/// is held to the owner seen alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asked {
    pub ask: String,
    pub grants: Vec<String>,
    /// A read told in full: said only while the owner is seen alone (or
    /// in the private room) as it is said (`say::Words::Agent`).
    pub owner_alone: bool,
}

/// What is told of an ask once its run ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Told {
    /// The agent's answer, one line, at most [`MOST_TOLD_BYTES`]: told as
    /// quoted, held to the owner seen alone with `owner_alone`.
    Answer { words: String, owner_alone: bool },
    /// A fixed line of the lamp's own ([`FAILED_LINE`],
    /// [`NOT_ALONE_LINE`]), as a note in the conversation's language.
    Line(&'static str),
}

/// The line lamp-tasks takes (`lamp_tasks::voice_ask::AskLine`).
pub fn line(asked: &Asked) -> Value {
    json!({
        "op": "voice_ask",
        "ask": asked.ask,
        "grants": asked.grants,
        "within_ms": WITHIN.as_millis() as u64,
    })
}

/// The ask's words for the agent: the person's line as heard in
/// `request`, and what the model took them to ask (`model`), at most
/// 2,000 bytes, one line.
pub fn ask_words(heard: Option<&str>, model: Option<&str>) -> String {
    let plain = |text: &str| {
        text.split(|c: char| c.is_whitespace() || c.is_control())
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let heard = heard.map(plain).filter(|text| !text.is_empty());
    let model = model.map(plain).filter(|text| !text.is_empty());
    let words = match (heard, model) {
        (Some(heard), Some(model)) if heard != model => {
            format!("{heard} (the lamp took this to mean: {model})")
        }
        (Some(heard), _) => heard,
        (None, Some(model)) => model,
        (None, None) => String::new(),
    };
    cut_at(&words, 2_000).to_owned()
}

/// `text` cut to at most `most` bytes at a character's end.
fn cut_at(text: &str, most: usize) -> &str {
    if text.len() <= most {
        return text;
    }
    let mut end = most;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Hands `asked` to the agent on a thread of its own and answers the model
/// at once: [`HOLDING_NEXT`], or why not.
pub fn hand(asked: Asked) -> Value {
    let Some(directory) = TASKS.get().cloned() else {
        return json!({ "error": NO_AGENT });
    };
    hand_at(
        directory.join(crate::make_skill_agent::MAKE_SOCKET_NAME),
        asked,
        tell,
    )
}

/// [`hand`] at `socket`, its answer told through `told`.
pub fn hand_at(socket: PathBuf, asked: Asked, told: fn(Told, Since)) -> Value {
    if asked.ask.trim().is_empty() {
        return json!({ "error": NO_AGENT });
    }
    if ASKING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        eprintln!(
            "lamp-voice: an ask for the lamp's agent: one is with it already; not handed over"
        );
        return json!({ "error": BUSY_NEXT });
    }
    let since = Since::now();
    let grants = asked.grants.len();
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-agent-ask".into())
        .spawn(move || {
            let outcome = exchange(&socket, &line(&asked), WITHIN + GRACE);
            ASKING.store(false, Ordering::Release);
            if since.forgotten() {
                eprintln!("lamp-voice: an ask for the lamp's agent: its answer dropped, untold: forgotten or stopped since");
                return;
            }
            // Whether the owner is still alone is the say client's to look
            // at, as the answer waits and as it is said (`say::NOT_ALONE`).
            let told_now = match outcome {
                Ok(answer) => {
                    eprintln!(
                        "lamp-voice: an ask for the lamp's agent: answered, {} bytes{}",
                        answer.words.len(),
                        if answer.owner_alone {
                            ", held to the owner seen alone"
                        } else {
                            ""
                        }
                    );
                    // Held to the owner alone when the ask asked it (a full
                    // read), or when lamp-tasks says so (an answer from the
                    // owner's own OpenClaw, AppOnly's P2 on #7509).
                    Told::Answer {
                        words: one_line(&answer.words),
                        owner_alone: asked.owner_alone || answer.owner_alone,
                    }
                }
                Err(why) => {
                    eprintln!("lamp-voice: an ask for the lamp's agent: not answered: {why}");
                    if why == format!("refused {NO_SERVICES}") {
                        Told::Line(NO_SERVICES_LINE)
                    } else if why == format!("refused {NOT_READY}") {
                        Told::Line(crate::made_skill::NOT_READY_LINE)
                    } else if why == format!("refused {MAKING_FIELD}") {
                        Told::Line(MAKING_LINE)
                    } else {
                        Told::Line(FAILED_LINE)
                    }
                }
            };
            told(told_now, since);
        });
    if spawned.is_err() {
        ASKING.store(false, Ordering::Release);
        return json!({ "error": NO_AGENT });
    }
    eprintln!("lamp-voice: an ask handed to the lamp's agent ({grants} service grant(s))");
    json!({ "handed_to_agent": true, "next": HOLDING_NEXT })
}

/// The agent's `answer` as the say service takes a quoted line: one
/// line, no control characters, at most [`MOST_TOLD_BYTES`].
pub fn one_line(answer: &str) -> String {
    let line = answer
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    cut_at(&line, MOST_TOLD_BYTES).to_owned()
}

/// Waits, at most 5 s, for an ask in flight to end.
#[cfg(test)]
pub(crate) fn wait_free() {
    for _ in 0..500 {
        if !ASKING.load(Ordering::Acquire) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// One test at a time of those that hand an ask over: they share [`ASKING`].
#[cfg(test)]
pub(crate) static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A forget, a stop or a privacy close: an ask in flight is never told.
/// Called by `services::forget_waiting` (the thread's forget, memory's
/// `memory/forgot`, a privacy change) and by a spoken stop
/// (`second_provider`, `LAMP_SPOKEN_STOP_ENDS`).
pub fn forget() {
    generations().fetch_add(1, Ordering::AcqRel);
}

/// How many forgets so far: tests only.
#[cfg(test)]
pub(crate) fn forgets() -> u64 {
    generations().load(Ordering::Acquire)
}

/// lamp-tasks' answer to an ask: the agent's words, and whether lamp-tasks
/// asks that they be said only to the owner seen alone
/// (`lamp_tasks::voice_ask::OWNER_ALONE`: an answer from the owner's own
/// OpenClaw, whose memory and sessions it has).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answered {
    pub words: String,
    pub owner_alone: bool,
}

/// One ask's line on lamp-tasks' socket and its answer, read until
/// `within`: the agent's answer, or why there is none.
pub fn exchange(socket: &Path, line: &Value, within: Duration) -> Result<Answered, String> {
    let mut stream = UnixStream::connect(socket)
        .map_err(|error| format!("lamp-tasks' socket is not reachable: {error}"))?;
    stream
        .set_write_timeout(Some(SEND_WITHIN))
        .and_then(|()| stream.set_read_timeout(Some(within)))
        .and_then(|()| writeln!(stream, "{line}"))
        .map_err(|error| format!("the ask was not sent: {error}"))?;
    let mut lines = BufReader::new(std::io::Read::take(stream, MOST_ANSWER_BYTES));
    loop {
        let mut text = String::new();
        match lines.read_line(&mut text) {
            Ok(0) => return Err("lamp-tasks gave no answer".into()),
            Ok(_) => {}
            Err(error) => return Err(format!("no answer in time: {}", error.kind())),
        }
        let answer: Value =
            serde_json::from_str(&text).map_err(|_| "lamp-tasks' answer can't be read")?;
        if answer["ok"] == true && answer["started"] == true {
            continue;
        }
        if answer["ok"] == true {
            return answer["answer"]
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(|words| Answered {
                    words: words.to_owned(),
                    owner_alone: answer[OWNER_ALONE] == true,
                })
                .ok_or_else(|| "lamp-tasks' answer has no words".into());
        }
        // A `busy` that a make holds is told apart (#7245): its own line.
        if answer[MAKING_FIELD] == true {
            return Err(format!("refused {MAKING_FIELD}"));
        }
        return Err(format!(
            "refused {}",
            answer["error"].as_str().unwrap_or("?")
        ));
    }
}

/// A fixed `line` told in the language of the conversation.
pub fn in_their_language(line: &str) -> String {
    format!(
        "{}Tell the person this, in a few words, in the language of the conversation: \"{line}\"",
        crate::made_skill::ALREADY_DONE
    )
}

/// The say service's ask for `told`: the agent's answer as its `quoted`
/// line ([`SAY_SKILL`]), never urgent, held to the owner seen alone with
/// `owner_alone`; a fixed line as a note, as the lamp's own.
pub fn say_line(told: &Told) -> Value {
    match told {
        Told::Answer { words, owner_alone } => json!({
            "skill": SAY_SKILL,
            "quoted": words,
            "owner_alone": owner_alone,
            "priority": crate::say::NORMAL_PRIORITY,
            "expires_in_ms": TOLD_WITHIN_MS,
        }),
        Told::Line(line) => json!({
            "skill": SAY_SKILL,
            "note": in_their_language(line),
            "priority": crate::app_make::LINE_PRIORITY,
            "expires_in_ms": TOLD_WITHIN_MS,
        }),
    }
}

/// Whether the say service refused or cut a say because the owner was no
/// longer seen alone ([`crate::say::NOT_ALONE`]).
pub fn not_alone(outcome: &crate::say::Outcome) -> bool {
    match outcome {
        crate::say::Outcome::Refused(why) | crate::say::Outcome::Dropped(why) => {
            why.contains(crate::say::NOT_ALONE)
        }
        _ => false,
    }
}

/// How often a say waiting for its moment looks for a forget or a stop.
const CANCEL_LOOK: Duration = Duration::from_millis(200);
/// How long past its wait a say's outcome may still come: begun at the
/// end of its wait, said whole.
const SAID_WITHIN: Duration = Duration::from_secs(120);

/// One ask of the say service at `socket`, read until its outcome. A
/// forget or a stop `since` its ask ([`forget`]) cancels it on its own
/// connection (`say::CANCEL_LINE`), as it waits or as it is said. None when
/// the service isn't there or goes quiet.
fn say_at(socket: &Path, ask: &Value, since: &Since) -> Option<crate::say::Outcome> {
    use crate::say::Outcome;
    let Ok(mut stream) = UnixStream::connect(socket) else {
        eprintln!(
            "lamp-voice: an ask for the lamp's agent: the say service is not there; not told"
        );
        return None;
    };
    stream.set_write_timeout(Some(SEND_WITHIN)).ok()?;
    writeln!(stream, "{ask}").ok()?;
    stream.set_read_timeout(Some(CANCEL_LOOK)).ok()?;
    let mut lines = BufReader::new(stream.try_clone().ok()?);
    let until = std::time::Instant::now() + Duration::from_millis(TOLD_WITHIN_MS) + SAID_WITHIN;
    let mut cancelled = false;
    let mut line = String::new();
    while std::time::Instant::now() < until {
        match lines.read_line(&mut line) {
            Ok(0) => return None,
            Ok(_) => {
                let told: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                line.clear();
                let why = || told["why"].as_str().unwrap_or_default().to_owned();
                return Some(match told["outcome"].as_str() {
                    // Begun: read on for its end.
                    None => continue,
                    Some("spoken") => Outcome::Spoken,
                    Some("dropped") => Outcome::Dropped(why()),
                    Some("expired") => Outcome::Expired,
                    Some(other) => Outcome::Refused(format!("{other} {}", why()).trim().to_owned()),
                });
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if !cancelled && since.forgotten() {
                    cancelled = true;
                    eprintln!(
                        "lamp-voice: an ask for the lamp's agent: forgotten or stopped as its answer waited: cancelled"
                    );
                    let _ = writeln!(stream, "{}", crate::say::CANCEL_LINE);
                }
            }
            Err(_) => return None,
        }
    }
    None
}

/// `told` asked of the say service at `socket`: its outcome. An answer
/// the owner was not alone for is followed by [`NOT_ALONE_LINE`], unless
/// forgotten or stopped `since` its ask.
fn tell_at(socket: &Path, told: Told, since: &Since) -> Option<crate::say::Outcome> {
    let outcome = say_at(socket, &say_line(&told), since);
    eprintln!("lamp-voice: an ask for the lamp's agent: told: {outcome:?}");
    if matches!(told, Told::Answer { .. })
        && outcome.as_ref().is_some_and(not_alone)
        && !since.forgotten()
    {
        eprintln!(
            "lamp-voice: an ask for the lamp's agent: answered, but someone else may be here: not told"
        );
        return tell_at(socket, Told::Line(NOT_ALONE_LINE), since);
    }
    outcome
}

/// `told` through the lamp's say service ([`tell_at`]), for the ask
/// made `since`.
fn tell(told: Told, since: Since) {
    let _ = tell_at(Path::new(crate::say::DEFAULT_SOCKET), told, &since);
}

/// [`TOOL`]'s call from a line `heard` live in `request`, its memory
/// scope `lease`: handed over only for the owner's own line, as a
/// `service_read` is held (`services::owner_of`; Heron's P2 6 on #7509:
/// the run has the host network).
pub fn run(
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    if !heard {
        return json!({ "error": crate::services::NOT_HEARD });
    }
    if crate::services::owner_of(lease).is_none() {
        eprintln!(
            "lamp-voice: request {request}: an ask for the lamp's agent not on the owner's own line: not handed over"
        );
        return json!({ "error": OWNER_ONLY_NEXT });
    }
    let said = crate::thread::THREAD
        .lock()
        .ok()
        .and_then(|thread| thread.heard_for(request, std::time::Instant::now()));
    let ask = ask_words(
        said.as_deref(),
        call.args.get("ask").and_then(Value::as_str),
    );
    hand(Asked {
        ask,
        ..Asked::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::Mutex;
    use std::sync::mpsc;

    static TOLD: Mutex<Option<mpsc::Sender<Told>>> = Mutex::new(None);
    use super::ONE;

    /// A folder of its own, removed with it.
    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "va-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn told(told: Told, _since: Since) {
        if let Some(sender) = TOLD.lock().unwrap().as_ref() {
            let _ = sender.send(told);
        }
    }

    /// lamp-tasks as a test has it: takes one line, answers with `answers`.
    fn tasks(answers: Vec<Value>) -> (Dir, PathBuf, mpsc::Receiver<Value>) {
        let dir = Dir::new();
        let socket = dir.path().join("skill-make.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (sent, seen) = mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            sent.send(serde_json::from_str::<Value>(&line).unwrap())
                .unwrap();
            let mut stream = stream;
            std::thread::sleep(Duration::from_millis(100));
            for answer in answers {
                writeln!(stream, "{answer}").unwrap();
            }
        });
        (dir, socket, seen)
    }

    fn listen() -> mpsc::Receiver<Told> {
        let (sender, notes) = mpsc::channel();
        *TOLD.lock().unwrap() = Some(sender);
        notes
    }

    #[test]
    fn a_read_is_handed_with_its_grant_held_then_its_answer_is_told() {
        let _one = ONE.lock().unwrap_or_else(|p| p.into_inner());
        wait_free();
        let notes = listen();
        let (_dir, socket, seen) = tasks(vec![
            json!({"ok": true, "started": true}),
            json!({"ok": true, "answer": "Two new mails, from Ana and the bank."}),
        ]);
        let asked = Asked {
            ask: "read me my latest emails".into(),
            grants: vec!["gmail-read".into()],
            owner_alone: true,
        };
        let answer = hand_at(socket, asked, told);
        assert_eq!(
            answer,
            json!({"handed_to_agent": true, "next": HOLDING_NEXT})
        );
        let line = seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(line["op"], "voice_ask");
        assert_eq!(line["ask"], "read me my latest emails");
        assert_eq!(line["grants"], json!(["gmail-read"]));
        // No brief read is ever handed over (Heron's P2 1 on #7509).
        assert!(line.get("brief").is_none(), "{line}");
        assert_eq!(line["within_ms"], 120_000);
        // Told as the agent's quoted answer, held to the owner alone.
        assert_eq!(
            notes.recv_timeout(Duration::from_secs(5)).unwrap(),
            Told::Answer {
                words: "Two new mails, from Ana and the bank.".into(),
                owner_alone: true,
            }
        );
    }

    #[test]
    fn a_failed_refused_or_unanswered_run_says_the_fixed_line() {
        let _one = ONE.lock().unwrap_or_else(|p| p.into_inner());
        wait_free();
        let notes = listen();
        for answers in [
            vec![json!({"ok": false, "error": "run_failed", "message": "deadline"})],
            vec![json!({"ok": true, "started": true})],
            vec![json!({"ok": false, "error": "busy", "message": "one at a time"})],
        ] {
            let (_dir, socket, _seen) = tasks(answers);
            let answer = hand_at(
                socket,
                Asked {
                    ask: "plan my trip".into(),
                    ..Asked::default()
                },
                told,
            );
            assert_eq!(answer["handed_to_agent"], true);
            let note = notes.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(note, Told::Line(FAILED_LINE));
            // The one place is free again once told.
            for _ in 0..100 {
                if !ASKING.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        // A read the lamp's default agent (OpenClaw) can't take yet: its
        // own plain line, never the voice's own read.
        let (_dir, socket, _seen) = tasks(vec![json!({
            "ok": false,
            "error": NO_SERVICES,
            "message": "can't be held off the network"
        })]);
        let answer = hand_at(
            socket,
            Asked {
                ask: "read me my mail".into(),
                grants: vec!["gmail-read".into()],
                ..Asked::default()
            },
            told,
        );
        assert_eq!(answer["handed_to_agent"], true);
        assert_eq!(
            notes.recv_timeout(Duration::from_secs(5)).unwrap(),
            Told::Line(NO_SERVICES_LINE)
        );
        wait_free();
        // Harness not looked yet at the lamp's default agent: the lamp's
        // not-ready line, as a make's (AppOnly's P3 on #7509).
        let (_dir, socket, _seen) = tasks(vec![json!({
            "ok": false,
            "error": NOT_READY,
            "message": "not ready"
        })]);
        hand_at(
            socket,
            Asked {
                ask: "x".into(),
                ..Asked::default()
            },
            told,
        );
        assert_eq!(
            notes.recv_timeout(Duration::from_secs(5)).unwrap(),
            Told::Line(crate::made_skill::NOT_READY_LINE)
        );
        wait_free();
        // A live ask while the agent makes a skill (#7245, Twelve's note on
        // #7737): lamp-tasks' `busy` with `making` is told why, not the
        // failed line; a `busy` without it stays the failed line (above).
        let (_dir, socket, _seen) = tasks(vec![json!({
            "ok": false,
            "error": "busy",
            "message": "the lamp's agent is making a skill; one is taken at a time",
            MAKING_FIELD: true
        })]);
        hand_at(
            socket,
            Asked {
                ask: "what's the weather".into(),
                ..Asked::default()
            },
            told,
        );
        assert_eq!(
            notes.recv_timeout(Duration::from_secs(5)).unwrap(),
            Told::Line(MAKING_LINE)
        );
        assert!(!MAKING_LINE.contains("couldn't"));
        wait_free();
        // No socket at all: the fixed line too.
        let gone = std::env::temp_dir().join("va-none/skill-make.sock");
        hand_at(
            gone,
            Asked {
                ask: "x".into(),
                ..Asked::default()
            },
            told,
        );
        assert_eq!(
            notes.recv_timeout(Duration::from_secs(5)).unwrap(),
            Told::Line(FAILED_LINE)
        );
    }

    #[test]
    fn a_timeout_is_a_failure_and_a_second_ask_waits_for_the_first() {
        let _one = ONE.lock().unwrap_or_else(|p| p.into_inner());
        wait_free();
        let dir = Dir::new();
        let socket = dir.path().join("skill-make.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        // Nothing answers: the read times out.
        assert!(
            exchange(
                &socket,
                &json!({"op": "voice_ask"}),
                Duration::from_millis(200)
            )
            .unwrap_err()
            .starts_with("no answer in time")
        );
        ASKING.store(true, Ordering::Release);
        let busy = hand_at(
            socket,
            Asked {
                ask: "x".into(),
                ..Asked::default()
            },
            told,
        );
        ASKING.store(false, Ordering::Release);
        assert_eq!(busy, json!({"error": BUSY_NEXT}));
    }

    #[test]
    fn a_forget_drops_an_answer_in_flight() {
        let _one = ONE.lock().unwrap_or_else(|p| p.into_inner());
        wait_free();
        let notes = listen();
        let (_dir, socket, seen) = tasks(vec![json!({"ok": true, "answer": "Ana wrote: lunch?"})]);
        // The thread reads its line before the forget; its answer is dropped.
        let (gate, open) = mpsc::channel::<()>();
        let forgets = generations();
        std::thread::spawn(move || {
            let _ = seen.recv_timeout(Duration::from_secs(5));
            forgets.fetch_add(1, Ordering::AcqRel);
            let _ = gate.send(());
        });
        hand_at(
            socket,
            Asked {
                ask: "x".into(),
                ..Asked::default()
            },
            told,
        );
        open.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(notes.recv_timeout(Duration::from_millis(300)).is_err());
    }

    /// The say service as a test has it: answers each ask, in order, with
    /// its outcome line, after the asker's cancel when `None`; sends back
    /// each ask and each cancel it read.
    fn say_service(outcomes: Vec<Option<Value>>) -> (Dir, PathBuf, mpsc::Receiver<Value>) {
        let dir = Dir::new();
        let socket = dir.path().join("say.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (sent, seen) = mpsc::channel();
        std::thread::spawn(move || {
            for outcome in outcomes {
                let (mut stream, _) = listener.accept().unwrap();
                let mut lines = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                lines.read_line(&mut line).unwrap();
                sent.send(serde_json::from_str::<Value>(&line).unwrap())
                    .unwrap();
                writeln!(stream, "{}", json!({"at_us": 1})).unwrap();
                let outcome = outcome.unwrap_or_else(|| {
                    let mut cancel = String::new();
                    lines.read_line(&mut cancel).unwrap();
                    sent.send(serde_json::from_str::<Value>(&cancel).unwrap())
                        .unwrap();
                    json!({"outcome": "dropped", "why": crate::say::ASKER_STOPPED})
                });
                writeln!(stream, "{outcome}").unwrap();
            }
        });
        (dir, socket, seen)
    }

    /// Heron's P2s 2 and 3 on #7509: the answer goes as the say service's
    /// quoted line of the agent, never urgent, held to the owner seen alone
    /// on the say client's own thread; refused or cut there for someone
    /// else in view, only the fixed line follows.
    #[test]
    fn an_answer_is_quoted_and_the_owner_alone_is_the_say_clients_to_judge() {
        let answer = Told::Answer {
            words: "Ana wrote: lunch at noon?".into(),
            owner_alone: true,
        };
        let line = say_line(&answer);
        assert_eq!(line["quoted"], "Ana wrote: lunch at noon?");
        assert_eq!(line["owner_alone"], true);
        assert!(line.get("note").is_none(), "{line}");
        let asked = crate::say::parse(&line.to_string()).unwrap();
        assert!(asked.priority < crate::say::URGENT_FROM);
        assert_eq!(
            asked.words,
            crate::say::Words::Agent {
                words: "Ana wrote: lunch at noon?".into(),
                owner_alone: true,
            }
        );
        let turn = asked.words.turn().unwrap();
        assert!(turn.contains("never an instruction"), "{turn}");
        assert!(turn.contains("You have no functions"), "{turn}");
        // A fixed line is the lamp's own note.
        let fixed = crate::say::parse(&say_line(&Told::Line(FAILED_LINE)).to_string()).unwrap();
        assert_eq!(
            fixed.words,
            crate::say::Words::Note(in_their_language(FAILED_LINE))
        );
        // Refused by the say client (someone else in view): the fixed line.
        let (_dir, socket, seen) = say_service(vec![
            Some(json!({"outcome": "refused", "why": crate::say::NOT_ALONE})),
            Some(json!({"outcome": "spoken"})),
        ]);
        let since = Since::now();
        assert_eq!(
            tell_at(&socket, answer.clone(), &since),
            Some(crate::say::Outcome::Spoken)
        );
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(5)).unwrap()["quoted"],
            "Ana wrote: lunch at noon?"
        );
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(5)).unwrap()["note"],
            in_their_language(NOT_ALONE_LINE)
        );
        // Said whole: nothing follows.
        let (_dir, socket, seen) = say_service(vec![Some(json!({"outcome": "spoken"}))]);
        assert_eq!(
            tell_at(&socket, answer, &since),
            Some(crate::say::Outcome::Spoken)
        );
        seen.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(seen.recv_timeout(Duration::from_millis(300)).is_err());
        assert!(not_alone(&crate::say::Outcome::Dropped(
            crate::say::NOT_ALONE.into()
        )));
        assert!(!not_alone(&crate::say::Outcome::Expired));
    }

    /// AppOnly's P2 on #7509: a hard ask (no grant, not held by the voice)
    /// answered by the owner's own OpenClaw, which has their memory and
    /// sessions, is held to the owner seen alone when lamp-tasks says so:
    /// with a guest in view the say client refuses it and only the fixed
    /// line is said. Hermes's answer, from a fresh home, is as before.
    #[test]
    fn a_gateway_hard_ask_is_held_to_the_owner_alone_and_hermess_is_not() {
        let _one = ONE.lock().unwrap_or_else(|p| p.into_inner());
        wait_free();
        let notes = listen();
        for (held, expected) in [(Some(true), true), (None, false)] {
            let mut answered = json!({"ok": true, "answer": "You said Ana likes tea."});
            if let Some(held) = held {
                answered[OWNER_ALONE] = json!(held);
            }
            let (_dir, socket, _seen) = tasks(vec![json!({"ok": true, "started": true}), answered]);
            hand_at(
                socket,
                Asked {
                    ask: "what did I tell you about Ana".into(),
                    ..Asked::default()
                },
                told,
            );
            assert_eq!(
                notes.recv_timeout(Duration::from_secs(5)).unwrap(),
                Told::Answer {
                    words: "You said Ana likes tea.".into(),
                    owner_alone: expected,
                }
            );
            wait_free();
        }
        // A guest in view: the say client refuses the held answer, and only
        // the fixed line is said.
        let held = Told::Answer {
            words: "You said Ana likes tea.".into(),
            owner_alone: true,
        };
        let (_dir, socket, seen) = say_service(vec![
            Some(json!({"outcome": "refused", "why": crate::say::NOT_ALONE})),
            Some(json!({"outcome": "spoken"})),
        ]);
        tell_at(&socket, held, &Since::now());
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(5)).unwrap()["owner_alone"],
            true
        );
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(5)).unwrap()["note"],
            in_their_language(NOT_ALONE_LINE)
        );
    }

    /// Heron's P2 4 on #7509: a forget while the answer waits for its
    /// moment cancels it on its own connection, and nothing follows.
    #[test]
    fn a_forget_while_the_answer_waits_cancels_it() {
        let (_dir, socket, seen) = say_service(vec![None]);
        let since = Since::now();
        let teller = std::thread::spawn(move || {
            tell_at(
                &socket,
                Told::Answer {
                    words: "x".into(),
                    owner_alone: false,
                },
                &since,
            )
        });
        seen.recv_timeout(Duration::from_secs(5)).unwrap();
        forget();
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(5)).unwrap(),
            json!({"cancel": true})
        );
        assert_eq!(
            teller.join().unwrap(),
            Some(crate::say::Outcome::Dropped(
                crate::say::ASKER_STOPPED.into()
            ))
        );
    }

    #[test]
    fn the_tool_fits_the_link_and_its_words_are_the_persons() {
        assert!(tool().description.len() < 1_024);
        assert!(serde_json::to_string(&tool().parameters).unwrap().len() < 1_024);
        assert_eq!(ask_words(Some(" read  my\nmail "), None), "read my mail");
        assert_eq!(
            ask_words(Some("do that again"), Some("check the weather in Hue")),
            "do that again (the lamp took this to mean: check the weather in Hue)"
        );
        assert_eq!(ask_words(None, Some("x")), "x");
        assert_eq!(ask_words(Some(&"é".repeat(1_500)), None).len(), 2_000);
        assert_eq!(
            one_line(" One\n\tmail,\u{7} from Ann. "),
            "One mail, from Ann."
        );
        assert_eq!(one_line(&"é".repeat(1_000)).len(), MOST_TOLD_BYTES);
        assert!(in_their_language(FAILED_LINE).len() <= crate::say::MOST_WORDS_BYTES);
        assert!(in_their_language(NOT_ALONE_LINE).len() <= crate::say::MOST_WORDS_BYTES);
    }

    /// lamp-tasks' and lamp-control's names are these.
    #[test]
    fn the_switch_names_are_the_tasks_parts() {
        assert_eq!(SERVICES_ENVIRONMENT, lamp_tasks::voice_ask::SERVICES_SWITCH);
        assert_eq!(HARD_ENVIRONMENT, lamp_tasks::voice_ask::HARD_SWITCH);
        assert_eq!(lamp_services::VOICE_ASK_SKILL, lamp_tasks::voice_ask::SKILL);
        assert_eq!(NO_SERVICES, lamp_tasks::voice_ask::NO_SERVICES);
        assert_eq!(NOT_READY, lamp_tasks::skill_make::NOT_READY);
        assert_eq!(OWNER_ALONE, lamp_tasks::voice_ask::OWNER_ALONE);
        assert_eq!(
            crate::made_skill::NOT_READY_LINE,
            lamp_tasks::skill_make::NOT_READY_LINE
        );
        assert_eq!(crate::say::AGENT_SKILL, lamp_services::VOICE_ASK_SKILL);
        assert_eq!(
            line(&Asked::default())["within_ms"].as_u64().unwrap(),
            lamp_tasks::voice_ask::MOST_WITHIN.as_millis() as u64
        );
    }
}
