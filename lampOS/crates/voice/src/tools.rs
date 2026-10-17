//! What the model may do other than speak.
//!
//! A Live session can be told of functions and asks for one by name
//! (`lamp-gemini`, `SessionConfig::tools`). The voice declares two kinds and
//! runs neither itself:
//!
//! - **`not_for_me`**, its own. A session has had only one thing to do with
//!   what it hears: answer it. On lamp-4ace on 2026-10-13, of 23 new
//!   sessions that heard two people talking to each other, 7 spoke words to
//!   them and 3 more made a sound, whether or not the session had been told
//!   how the room was. With this function the model has something else to
//!   do with speech that is not for it, and when it does it, whatever sound
//!   the service still sends for that request is dropped before the speaker.
//!   Off unless the service's environment says `on`: what it does to those
//!   counts, and what it costs an ordinary answer, has not been measured.
//! - **The executive's**, read from the file it keeps in its directory
//!   (`tools.json`) each time a session opens, so that they appear when the
//!   executive starts and go when its directory does. A call is sent to its
//!   socket (`call.sock`) and waited for no longer than [`ASK_DEADLINE`];
//!   with the executive gone, late or unreadable the model is told so in
//!   words it can say, and the voice goes on. The voice never waits on the
//!   executive for anything else.
//!
//! - **`the_time`**, its own too, answered here at once from the lamp's
//!   clock. A session is told the time when it opens and may run for hours:
//!   on lamp-4ace on 2026-10-14 one opened at 10:48 told someone at 11:17
//!   that it was "roughly around ten forty-five". Off unless the service's
//!   environment says `on` ([`CLOCK_ENVIRONMENT`]).
//!
//! Every call is answered, whatever becomes of it: the service generates
//! nothing more for a request until its calls are.

use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[cfg(test)]
#[path = "tools/schedule_integration_tests.rs"]
mod schedule_integration_tests;

/// Where the coordinator reads whether the model may say a thing was not
/// for it. `on` only.
pub const ENVIRONMENT: &str = "LAMP_TOOLS";
/// The provider worker's flag for it.
pub const FLAG: &str = "--tools";
/// Where the coordinator reads the executive's directory: an absolute path.
pub const ACTIONS_ENVIRONMENT: &str = "LAMP_ACTIONS_DIR";
/// The provider worker's flag for it, followed by the path.
pub const ACTIONS_FLAG: &str = "--actions-dir";

/// The function the model calls instead of speaking.
pub const NOT_FOR_ME: &str = "not_for_me";
/// The function the model calls to know the time now.
pub const THE_TIME: &str = "the_time";
/// Where the coordinator reads whether the model may ask the time. `on`
/// only.
pub const CLOCK_ENVIRONMENT: &str = "LAMP_CLOCK";
/// The provider worker's flag for it.
pub const CLOCK_FLAG: &str = "--clock";
/// Only the exact value `on` permits the six Schedule tool names.
pub const SCHEDULE_TOOLS_ENVIRONMENT: &str = "LAMP_SCHEDULE_TOOLS";
pub const SCHEDULE_TOOLS_FLAG: &str = "--schedule-tools";
/// Independent permission for provider requests carrying a due reminder.
pub const SCHEDULE_SPEAK_ENVIRONMENT: &str = "LAMP_SCHEDULE_SPEAK";
pub const SCHEDULE_SPEAK_FLAG: &str = "--schedule-speak";
pub const SCHEDULE_NAMES: [&str; 6] = [
    "set_timer",
    "set_reminder",
    "repeat_reminder",
    "list_reminders",
    "cancel_reminder",
    "snooze_reminder",
];
/// The executive's list of functions, in its directory.
pub const CATALOG_FILE: &str = "tools.json";
/// The executive's socket, in its directory.
pub const CALL_SOCKET: &str = "call.sock";
/// The skills' folder in the executive's directory: `lamp-skills --tools-dir`
/// keeps its own `tools.json` and `call.sock` there, read and called the
/// same way, after the executive's.
pub const SKILLS_DIR: &str = "skills";
/// The longest a call waits for the executive, connecting included. A
/// person hears nothing while it waits.
pub const ASK_DEADLINE: Duration = Duration::from_millis(1_500);
/// The most the executive's list or one of its answers may be.
const MAX_CATALOG_BYTES: u64 = 65_536;
const MAX_ANSWER_BYTES: u64 = 8_192;

/// What the worker was told by its flags.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Settings {
    /// The model may call [`NOT_FOR_ME`].
    pub decline: bool,
    /// The executive's directory.
    pub exec: Option<PathBuf>,
    /// The memory's socket ([`crate::remember`]).
    pub memory: Option<PathBuf>,
    /// The model may call [`THE_TIME`].
    pub clock: bool,
    /// Work handed to an agent elsewhere ([`crate::agent_tasks`]).
    pub tasks: Option<crate::agent_tasks::Place>,
    /// Schedule tools may be declared and routed to Actions.
    pub schedule_tools: bool,
    /// A typed due-reminder request may be considered by the provider.
    pub schedule_speak: bool,
    /// The eyes' request socket, for remembering faces
    /// ([`crate::known_faces`]).
    pub who: Option<PathBuf>,
    /// A name is kept only if the person said it, and a face keeps one
    /// name (`LAMP_NAMES=checked`, [`crate::known_faces::said_by_them`]).
    pub names_checked: bool,
    /// The serving session's folder, for marking a bad moment
    /// ([`crate::marked`]).
    pub mark: Option<PathBuf>,
    /// The onboarding file, whose mailbox the model's reports go to
    /// ([`crate::onboarding`]).
    pub onboarding: Option<PathBuf>,
}

/// The provider worker's arguments for what the environment says.
pub fn arguments_from_environment() -> Vec<String> {
    let mut flags = arguments(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var(ACTIONS_ENVIRONMENT).ok().as_deref(),
        std::env::var(crate::remember::ENVIRONMENT).ok().as_deref(),
        std::env::var(CLOCK_ENVIRONMENT).ok().as_deref(),
    );
    flags.extend(crate::agent_tasks::arguments_from_environment());
    flags.extend(schedule_arguments(
        std::env::var(SCHEDULE_TOOLS_ENVIRONMENT).ok().as_deref(),
        std::env::var(SCHEDULE_SPEAK_ENVIRONMENT).ok().as_deref(),
    ));
    if let Some(socket) = std::env::var(crate::known_faces::ENVIRONMENT)
        .ok()
        .filter(|path| Path::new(path).is_absolute())
    {
        flags.push(crate::known_faces::FLAG.to_owned());
        flags.push(socket);
        flags.extend(crate::known_faces::names_arguments(
            std::env::var(crate::known_faces::NAMES_ENVIRONMENT)
                .ok()
                .as_deref(),
        ));
    }
    flags.extend(crate::marked::arguments_from_environment());
    if let Some(file) = crate::onboarding::switched_on(
        std::env::var(crate::onboarding::ENVIRONMENT)
            .ok()
            .as_deref(),
        std::env::var(crate::onboarding::FILE_ENVIRONMENT)
            .ok()
            .as_deref(),
    ) {
        flags.push(crate::onboarding::FLAG.to_owned());
        flags.push(file.display().to_string());
    }
    flags
}

pub(crate) fn schedule_arguments(tools: Option<&str>, speak: Option<&str>) -> Vec<String> {
    let mut flags = Vec::new();
    if tools == Some("on") {
        flags.push(SCHEDULE_TOOLS_FLAG.to_owned());
    }
    // Schedule speech by the parent's own rule: `on`, or `skill`.
    if crate::reminders::speaks(speak) {
        flags.push(SCHEDULE_SPEAK_FLAG.to_owned());
    }
    flags
}

pub fn is_schedule_tool(name: &str) -> bool {
    SCHEDULE_NAMES.contains(&name)
}

fn arguments(
    decline: Option<&str>,
    exec: Option<&str>,
    memory: Option<&str>,
    clock: Option<&str>,
) -> Vec<String> {
    let mut arguments = Vec::new();
    if decline == Some("on") {
        arguments.push(FLAG.to_owned());
    }
    if let Some(directory) = exec.filter(|path| Path::new(path).is_absolute()) {
        arguments.push(ACTIONS_FLAG.to_owned());
        arguments.push(directory.to_owned());
    }
    if let Some(socket) = memory.filter(|path| Path::new(path).is_absolute()) {
        arguments.push(crate::remember::FLAG.to_owned());
        arguments.push(socket.to_owned());
    }
    if clock == Some("on") {
        arguments.push(CLOCK_FLAG.to_owned());
    }
    arguments
}

/// Take a leading `--tools`, `--actions-dir PATH`, `--memory-socket PATH`
/// and `--clock` off a worker's flags.
pub fn take(flags: &[String]) -> std::io::Result<(Settings, &[String])> {
    let mut settings = Settings::default();
    let mut flags = flags;
    if let [flag, rest @ ..] = flags
        && flag == FLAG
    {
        settings.decline = true;
        flags = rest;
    }
    if let [flag, rest @ ..] = flags
        && flag == ACTIONS_FLAG
    {
        let [directory, rest @ ..] = rest else {
            return Err(std::io::Error::other("--actions-dir needs a path"));
        };
        if !Path::new(directory).is_absolute() {
            return Err(std::io::Error::other("--actions-dir must be absolute"));
        }
        settings.exec = Some(PathBuf::from(directory));
        flags = rest;
    }
    if let [flag, rest @ ..] = flags
        && flag == crate::remember::FLAG
    {
        let [socket, rest @ ..] = rest else {
            return Err(std::io::Error::other("--memory-socket needs a path"));
        };
        if !Path::new(socket).is_absolute() {
            return Err(std::io::Error::other("--memory-socket must be absolute"));
        }
        settings.memory = Some(PathBuf::from(socket));
        flags = rest;
    }
    if let [flag, rest @ ..] = flags
        && flag == CLOCK_FLAG
    {
        settings.clock = true;
        flags = rest;
    }
    let (tasks, mut flags) = crate::agent_tasks::take(flags)?;
    settings.tasks = tasks;
    if let [flag, rest @ ..] = flags
        && flag == SCHEDULE_TOOLS_FLAG
    {
        settings.schedule_tools = true;
        flags = rest;
    }
    if let [flag, rest @ ..] = flags
        && flag == SCHEDULE_SPEAK_FLAG
    {
        settings.schedule_speak = true;
        flags = rest;
    }
    if let [flag, rest @ ..] = flags
        && flag == crate::known_faces::FLAG
    {
        let [socket, rest @ ..] = rest else {
            return Err(std::io::Error::other("--who-socket needs a path"));
        };
        if !Path::new(socket).is_absolute() {
            return Err(std::io::Error::other("--who-socket must be absolute"));
        }
        settings.who = Some(PathBuf::from(socket));
        flags = rest;
        if let [flag, rest @ ..] = flags
            && flag == crate::known_faces::NAMES_FLAG
        {
            settings.names_checked = true;
            flags = rest;
        }
    }
    if let [flag, rest @ ..] = flags
        && flag == crate::marked::FLAG
    {
        let [folder, rest @ ..] = rest else {
            return Err(std::io::Error::other("--mark-weird needs a path"));
        };
        if !Path::new(folder).is_absolute() {
            return Err(std::io::Error::other("--mark-weird must be absolute"));
        }
        settings.mark = Some(PathBuf::from(folder));
        flags = rest;
    }
    if let [flag, rest @ ..] = flags
        && flag == crate::onboarding::FLAG
    {
        let [file, rest @ ..] = rest else {
            return Err(std::io::Error::other("--onboarding-file needs a path"));
        };
        if !Path::new(file).is_absolute() {
            return Err(std::io::Error::other("--onboarding-file must be absolute"));
        }
        settings.onboarding = Some(PathBuf::from(file));
        flags = rest;
    }
    Ok((settings, flags))
}

fn not_for_me() -> Tool {
    Tool {
        name: NOT_FOR_ME.into(),
        description: "Call this, and say nothing at all, when what you just heard was not said to you: \
                      people talking to each other, someone on a call or reading aloud, a television, \
                      a noise. It is how you stay out of a conversation that is not yours."
            .into(),
        parameters: None,
        // Declared as one nothing is said on account of. Declared as an
        // ordinary function, on lamp-4ace on 2026-10-13, its answer made
        // the model speak ("I'm sorry, I missed that. Could you please
        // repeat it?"), talk nonsense, or call it again eleven times.
        silent: true,
    }
}

fn the_time() -> Tool {
    Tool {
        name: THE_TIME.into(),
        description:
            "Call this whenever you need the time or the date now: when someone asks what \
                      time it is, what day it is, or how long until something. The time you were \
                      told when you woke may be hours old; this one is exact. Then say it as it \
                      gives it \"said aloud\", with its hour. Not for \"now\" or \"today\" in a \
                      question about something else, such as how you are feeling right now, and \
                      never say the time unless you were asked for it."
                .into(),
        parameters: None,
        silent: false,
    }
}

/// What the model is told when it asks the time: the lamp's clock now, in
/// words, that it has not been set (`LAMP_KERNEL_CLOCK`), or that it could
/// not be read.
pub fn time_now() -> Value {
    match crate::local_time::now_here() {
        Ok(Some(now)) => json!({ "now": now }),
        Ok(None) => json!({"error": "My clock could not be read just now."}),
        Err(unset) => json!({ "error": unset }),
    }
}

/// The functions a session opened now is told of: the voice's own if they
/// are switched on, then the executive's if its list is there and reads. A
/// name the voice uses itself is left out of the executive's.
pub fn declared(settings: &Settings) -> Vec<Tool> {
    let mut tools = Vec::new();
    if settings.decline {
        tools.push(not_for_me());
    }
    if settings.clock {
        tools.push(the_time());
    }
    if settings.memory.is_some() {
        tools.extend(crate::remember::tools());
    }
    if let Some(place) = &settings.tasks {
        tools.extend(crate::agent_tasks::tools(place));
    }
    if settings.who.is_some() {
        tools.extend(crate::known_faces::tools_checked(settings.names_checked));
    }
    if settings.mark.is_some() {
        tools.push(crate::marked::tool());
    }
    if settings.onboarding.is_some() {
        tools.extend(crate::onboarding::tools());
    }
    if let Some(directory) = &settings.exec {
        let room = lamp_gemini::MAX_TOOLS - tools.len();
        tools.extend(
            catalog(&directory.join(CATALOG_FILE))
                .into_iter()
                .filter(|tool| {
                    !voices_own(&tool.name)
                        && (settings.schedule_tools || !is_schedule_tool(&tool.name))
                })
                .take(room),
        );
        // The skills' after the executive's: a name already told stays its.
        let skills = catalog(&directory.join(SKILLS_DIR).join(CATALOG_FILE));
        let room = lamp_gemini::MAX_TOOLS - tools.len();
        let fresh: Vec<Tool> = skills
            .into_iter()
            .filter(|tool| {
                !voices_own(&tool.name)
                    && !is_schedule_tool(&tool.name)
                    && !tools.iter().any(|told| told.name == tool.name)
            })
            .take(room)
            .collect();
        tools.extend(fresh);
    }
    tools
}

/// A name the voice routes itself, whether or not it is switched on.
fn voices_own(name: &str) -> bool {
    name == NOT_FOR_ME
        || name == THE_TIME
        || crate::remember::is_tool(name)
        || crate::agent_tasks::is_tool(name)
        || crate::known_faces::is_tool(name)
        || crate::marked::is_tool(name)
        || crate::onboarding::is_tool(name)
}

/// Whether the skills run `name` and the executive does not.
fn a_skills(directory: &Path, name: &str) -> bool {
    let has = |path: PathBuf| catalog(&path).iter().any(|tool| tool.name == name);
    !has(directory.join(CATALOG_FILE)) && has(directory.join(SKILLS_DIR).join(CATALOG_FILE))
}

/// The executive's list, or nothing: a missing file, one too large, one
/// that is not `{"tools":[...]}`, or one with a function the provider link
/// would refuse or a name twice. All or nothing, so that a session is
/// never opened with half of what the executive can do. A file there and
/// refused is said in the journal, with why (#799): no setting leaves the
/// lamp without the executive's functions and says nothing.
fn catalog(path: &Path) -> Vec<Tool> {
    read_catalog(path).unwrap_or_else(|why| {
        eprintln!("lamp-voice: the executive's functions are left out of the session: {why}");
        Vec::new()
    })
}

/// [`catalog`]'s list, empty with no file, or why a file there is refused:
/// names and sizes only, never a description's words.
fn read_catalog(path: &Path) -> Result<Vec<Tool>, String> {
    #[derive(serde::Deserialize)]
    struct Catalog {
        tools: Vec<Tool>,
    }
    let Ok(file) = std::fs::File::open(path) else {
        return Ok(Vec::new());
    };
    let mut text = String::new();
    file.take(MAX_CATALOG_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| format!("{} could not be read ({})", CATALOG_FILE, error.kind()))?;
    if text.len() as u64 > MAX_CATALOG_BYTES {
        return Err(format!("{CATALOG_FILE} is over {MAX_CATALOG_BYTES} bytes"));
    }
    let tools = serde_json::from_str::<Catalog>(&text)
        .map_err(|_| format!("{CATALOG_FILE} is not {{\"tools\":[...]}} of functions"))?
        .tools;
    // More functions than a session takes is no fault of the file: the
    // first that fit are told (`declared`), as before #1020 (Sixth on it).
    let (kept, left_out) = admitted_within(tools, usize::MAX);
    if left_out.is_empty() {
        Ok(kept)
    } else {
        Err(format!(
            "{CATALOG_FILE} has what the link would refuse: {}",
            left_out.join(", ")
        ))
    }
}

/// What to do with one call.
#[derive(Debug, Eq, PartialEq)]
pub enum Route {
    /// The model said the request was not for it: answer the call, and let
    /// none of the request's sound out.
    Decline,
    /// Ask the executive in this directory.
    Executive(PathBuf),
    /// A Schedule-only Actions exchange with bounded connect/read/write.
    Schedule(PathBuf),
    /// Ask the memory on this socket.
    Memory(PathBuf),
    /// The time: answered here, at once.
    Clock,
    /// Work for an agent elsewhere, through the tasks part.
    Tasks(crate::agent_tasks::Place),
    /// Remember or forget a face, through the eyes on this socket, with
    /// names checked or not.
    Faces(PathBuf, bool),
    /// Mark the last minute of this session for review.
    Mark(PathBuf),
    /// A step of the first meeting reported, through this onboarding file.
    Onboarding(PathBuf),
    /// Nobody here runs that.
    Nobody,
}

pub fn route(settings: &Settings, name: &str) -> Route {
    if is_schedule_tool(name) {
        return match (&settings.exec, settings.schedule_tools) {
            (Some(directory), true) => Route::Schedule(directory.clone()),
            _ => Route::Nobody,
        };
    }
    if let (Some(socket), true) = (&settings.memory, crate::remember::is_tool(name)) {
        return Route::Memory(socket.clone());
    }
    if crate::agent_tasks::is_tool(name) {
        return settings.tasks.clone().map_or(Route::Nobody, Route::Tasks);
    }
    if crate::known_faces::is_tool(name) {
        return settings.who.clone().map_or(Route::Nobody, |socket| {
            Route::Faces(socket, settings.names_checked)
        });
    }
    if crate::marked::is_tool(name) {
        return settings.mark.clone().map_or(Route::Nobody, Route::Mark);
    }
    if crate::onboarding::is_tool(name) {
        return settings
            .onboarding
            .clone()
            .map_or(Route::Nobody, Route::Onboarding);
    }
    match (&settings.exec, name) {
        (_, NOT_FOR_ME) if settings.decline => Route::Decline,
        (_, THE_TIME) if settings.clock => Route::Clock,
        (Some(directory), name)
            if name != NOT_FOR_ME
                && name != THE_TIME
                && !crate::remember::is_tool(name)
                && !crate::agent_tasks::is_tool(name) =>
        {
            if a_skills(directory, name) {
                Route::Executive(directory.join(SKILLS_DIR))
            } else {
                Route::Executive(directory.clone())
            }
        }
        _ => Route::Nobody,
    }
}

/// Whether a request with this call stays out of the memory's journal:
/// being asked to keep or forget something is not itself something to keep.
pub fn keeps_out(route: &Route, name: &str) -> bool {
    matches!(route, Route::Memory(_)) && name != crate::remember::RECALL
}

/// One call answered to the end, for a voice route that waits for each
/// call's answer before it goes on: the time from the lamp's clock, a
/// decline answered, a call nobody here runs told so, any other run by
/// [`run_call`]. Blocking: for a thread of its own. `not_for_me` itself is
/// the route's to act on before this.
pub fn answer(settings: &Settings, call: &ToolCall, request: u64, heard: bool) -> Value {
    match route(settings, &call.name) {
        Route::Clock => time_now(),
        Route::Decline => declined(),
        Route::Nobody => unavailable(),
        route => {
            if keeps_out(&route, &call.name) {
                crate::remember::keep_out(request);
            }
            let began = std::time::Instant::now();
            let (whom, (mut told, answered)) = run_call(&route, call, request, heard);
            // The second voice puts no news on its turns, so news asked about
            // there leaves its harness open (#1390); the key is never the
            // model's either way.
            if matches!(route, Route::Tasks(_)) {
                let _ = crate::agent_tasks::take_news_key(&mut told);
            }
            // Worded as the Live route's line, which is read on the lamp.
            eprintln!(
                "lamp-voice second: request {request} asked for {}; {whom} {} in {} ms; the answer was sent",
                call.name,
                if answered {
                    "answered"
                } else {
                    "did not answer"
                },
                began.elapsed().as_millis()
            );
            told
        }
    }
}

/// What the model is told when a call could not be run: words it can say.
pub fn unavailable() -> Value {
    json!({"error": "That part of me is not running right now, so I could not do it."})
}

/// What the model is told when it declines.
pub fn declined() -> Value {
    json!({"ok": true})
}

/// Run one call on the part its route names, waiting at most
/// [`ASK_DEADLINE`] where the part takes a deadline ([`crate::agent_tasks::DEADLINE`]
/// for the tasks part). Blocking: for a thread
/// of its own. Shared by every voice route's worker, so that a call is run
/// the same way whichever model asked. Returns who answered, in the words
/// the log uses, with what to tell the model and whether that part answered.
/// `heard` is false for a request the lamp wrote itself.
pub fn run_call(
    route: &Route,
    call: &ToolCall,
    request: u64,
    heard: bool,
) -> (&'static str, (Value, bool)) {
    let deadline = ASK_DEADLINE;
    match route {
        Route::Memory(socket) => ("the memory", crate::remember::run(socket, call, deadline)),
        Route::Executive(directory) => ("the executive", ask(directory, call, deadline)),
        Route::Tasks(place) => (
            "the tasks part",
            crate::agent_tasks::run(place, call, request, heard, crate::agent_tasks::DEADLINE),
        ),
        Route::Schedule(directory) => (
            "the schedule through the executive",
            ask_schedule(directory, call, deadline),
        ),
        Route::Faces(socket, checked) => {
            ("the eyes", crate::known_faces::run(socket, call, *checked))
        }
        Route::Mark(session) => ("the marks", crate::marked::run(session, call, heard)),
        Route::Onboarding(file) => ("the onboarding", crate::onboarding::run(file, call)),
        Route::Decline | Route::Clock | Route::Nobody => ("nobody", (unavailable(), false)),
    }
}

/// Ask the executive to run one call and wait at most `deadline` for its
/// answer, connecting included. Blocking: for a thread of its own. Always
/// returns what to tell the model; the second value says whether the
/// executive answered.
///
/// One line out, `{"op":"call","name":...,"args":{...}}`, and one line
/// back: `{"ok":true,"result":{...}}`, whose result goes to the model as it
/// is, or `{"ok":false,"error":"..."}`, whose words do.
pub fn ask(directory: &Path, call: &ToolCall, deadline: Duration) -> (Value, bool) {
    let line = json!({"op":"call","name":call.name,"args":call.args});
    let told = exchange(&directory.join(CALL_SOCKET), &line, deadline).and_then(|answer| {
        match answer.get("ok").and_then(Value::as_bool)? {
            true => Some(answer.get("result").cloned().unwrap_or_else(|| json!({}))),
            false => Some(json!({"error": answer.get("error").and_then(Value::as_str)?})),
        }
    });
    match told {
        // What the link would refuse to send is never offered to it: an
        // answer that is not sent leaves the service waiting on the call.
        Some(told) if sendable(&told) => (told, true),
        _ => (unavailable(), false),
    }
}

/// Only the new Schedule tools use this exchange. A lost or malformed reply
/// may follow a committed mutation; do not turn it into a claim of failure or
/// silently repeat the call under a new identity.
pub fn ask_schedule(directory: &Path, call: &ToolCall, deadline: Duration) -> (Value, bool) {
    if !is_schedule_tool(&call.name) {
        return (unavailable(), false);
    }
    let Some(until) = Instant::now().checked_add(deadline) else {
        return (unavailable(), false);
    };
    let line = json!({"op":"call","name":call.name,"args":call.args});
    match lamp_schedule::client::exchange(&directory.join(CALL_SOCKET), &line, until) {
        Ok(answer) => {
            let told = match answer.get("ok").and_then(Value::as_bool) {
                Some(true) => answer.get("result").cloned(),
                Some(false) => answer
                    .get("error")
                    .and_then(Value::as_str)
                    .map(|error| json!({"error":error})),
                None => None,
            };
            match told {
                Some(told) if sendable(&told) => (told, true),
                _ => (schedule_uncertain(), false),
            }
        }
        Err(error) if error.may_have_sent => (schedule_uncertain(), false),
        Err(_) => (unavailable(), false),
    }
}

fn schedule_uncertain() -> Value {
    json!({"error":"I could not confirm what happened to that reminder. It may have changed; please check the reminders before trying again."})
}

/// Whether the provider link will carry this to the model.
pub fn sendable(told: &Value) -> bool {
    told.to_string().len() + 16 <= lamp_gemini::MAX_TOOL_JSON_BYTES
}

/// One line of JSON to a local stream socket and one line back, inside
/// `deadline`, connecting included. `None` for a socket nobody answers on,
/// an answer that is late, cut off, too long or not JSON. Blocking: for a
/// thread of its own.
pub fn exchange(socket: &Path, line: &Value, deadline: Duration) -> Option<Value> {
    exchange_up_to(socket, line, deadline, MAX_ANSWER_BYTES)
}

/// [`exchange`] for a part whose answers can be longer than the executive's:
/// the tasks part's pages of tasks and an agent's answer (up to 16 KiB of it,
/// and 4 KiB of what it was asked). A page of eight tasks was 9,333 bytes on
/// lamp-4ace on 2026-10-15, and the 8 KiB bound made both the news and "how
/// is it going?" fail once the lamp had eight.
pub const MAX_TASKS_ANSWER_BYTES: u64 = 65_536;

pub fn exchange_up_to(socket: &Path, line: &Value, deadline: Duration, most: u64) -> Option<Value> {
    let started = Instant::now();
    let left = || {
        deadline
            .checked_sub(started.elapsed())
            .filter(|left| !left.is_zero())
    };
    // A local socket connects at once or not at all: there is no wait to
    // bound here that the deadlines below do not bound.
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_write_timeout(Some(left()?)).ok()?;
    let mut out = serde_json::to_vec(line).ok()?;
    out.push(b'\n');
    stream.write_all(&out).ok()?;
    let mut answer = Vec::new();
    let mut reader = BufReader::new(stream.try_clone().ok()?.take(most));
    loop {
        // The whole answer inside what is left, however it is cut up.
        stream.set_read_timeout(Some(left()?)).ok()?;
        let read = reader.read_until(b'\n', &mut answer).ok()?;
        if answer.last() == Some(&b'\n') {
            break;
        }
        if read == 0 {
            return None;
        }
    }
    serde_json::from_slice(&answer).ok()
}

/// The functions of `tools` the service's link takes, in their order, and
/// each it would refuse, left out, by name and why: a description or
/// parameters it would not take (the length of the description given), a
/// name given twice, or past [`lamp_gemini::MAX_TOOLS`]. One refused
/// function used to leave a session with none at all (`thread::equipped`);
/// on lamp-4ace on 2026-10-16 one description 12 bytes too long would have
/// done that, and nothing said so (#799). Names and lengths only:
/// descriptions are the voice's own fixed words or the executive's.
pub fn admitted(tools: Vec<Tool>) -> (Vec<Tool>, Vec<String>) {
    admitted_within(tools, lamp_gemini::MAX_TOOLS)
}

/// [`admitted`], keeping at most `most`.
fn admitted_within(tools: Vec<Tool>, most: usize) -> (Vec<Tool>, Vec<String>) {
    let mut kept: Vec<Tool> = Vec::new();
    let mut left_out = Vec::new();
    for tool in tools {
        let name = said_name(&tool.name);
        if tool.validate().is_err() {
            left_out.push(format!("{name} ({})", fault(&tool)));
        } else if kept.iter().any(|earlier| earlier.name == tool.name) {
            left_out.push(format!("{name} (a name given twice)"));
        } else if kept.len() == most {
            left_out.push(format!("{name} (past {most})"));
        } else {
            kept.push(tool);
        }
    }
    (kept, left_out)
}

/// A function's name as the journal may show it: one line, at most 48
/// characters, whatever a file gave.
fn said_name(name: &str) -> String {
    name.chars().take(48).flat_map(char::escape_debug).collect()
}

/// What of `tool` the link would refuse, found by the link's own check
/// with each part in turn made plain, so that its rules are not written
/// twice.
fn fault(tool: &Tool) -> String {
    let passes = |change: &dyn Fn(&mut Tool)| {
        let mut changed = tool.clone();
        change(&mut changed);
        changed.validate().is_ok()
    };
    if passes(&|tool| tool.description = "a".into()) {
        format!("{} bytes described", tool.description.len())
    } else if passes(&|tool| tool.name = "a".into()) {
        "a name the link does not take".into()
    } else if passes(&|tool| tool.parameters = None) {
        "parameters the link does not take".into()
    } else {
        "more than one part the link does not take".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #799: a function the link would refuse is left out and named; the
    /// rest are kept, and the link takes them.
    #[test]
    fn a_function_the_link_refuses_is_left_out_by_name_and_the_rest_are_kept() {
        let tool = |name: &str, bytes: usize| Tool {
            name: name.into(),
            description: "x".repeat(bytes),
            parameters: None,
            silent: false,
        };
        let link = || {
            lamp_gemini::SessionConfig::new(
                lamp_gemini::GOOGLE_ENDPOINT,
                lamp_gemini::Credential::api_key("test-only").unwrap(),
            )
            .unwrap()
        };
        let all = vec![
            tool("not_for_me", 9),
            tool("start_agent_task", 1_036),
            tool("confirm_agent_task", 1_024),
            tool("not_for_me", 9),
        ];
        assert!(
            link().tools(all.clone()).is_err(),
            "the whole list is refused"
        );
        let (kept, left_out) = admitted(all);
        let names: Vec<&str> = kept.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names, ["not_for_me", "confirm_agent_task"]);
        assert_eq!(
            left_out,
            [
                "start_agent_task (1036 bytes described)",
                "not_for_me (a name given twice)"
            ]
        );
        assert!(link().tools(kept).is_ok());
        let many: Vec<Tool> = (0..=lamp_gemini::MAX_TOOLS)
            .map(|index| tool(&format!("f{index}"), 9))
            .collect();
        let (kept, left_out) = admitted(many);
        assert_eq!(
            (kept.len(), left_out),
            (32, vec!["f32 (past 32)".to_owned()])
        );
        assert_eq!(admitted(Vec::new()), (Vec::new(), Vec::new()));
        // What is at fault, by the link's own check; a name on one line.
        let mut odd = tool("light_on", 9);
        odd.parameters = Some(json!(["not", "an", "object"]));
        let mut both = tool("Bad\nName", 2_000);
        both.parameters = Some(json!([]));
        let (_, left_out) = admitted(vec![odd, tool("Bad\nName", 9), both]);
        assert_eq!(
            left_out,
            [
                "light_on (parameters the link does not take)",
                "Bad\\nName (a name the link does not take)",
                "Bad\\nName (more than one part the link does not take)"
            ]
        );
    }

    /// #799: with every switch of the voice's own on, the name signal
    /// included, none of its functions is one the link would refuse.
    #[test]
    fn with_every_switch_on_the_voices_own_functions_are_a_list_the_link_takes() {
        let settings = Settings {
            decline: true,
            exec: None,
            memory: Some("/run/lampos-memory/memory.sock".into()),
            clock: true,
            tasks: Some(crate::agent_tasks::Place {
                directory: "/run/lampos-tasks".into(),
                target: crate::agent_tasks::DEFAULT_TARGET.into(),
                who: true,
                lead_in: true,
                asked: true,
                needs: None,
                staff_note: false,
            }),
            schedule_tools: true,
            schedule_speak: true,
            who: Some("/run/lampos-eyes/request.sock".into()),
            // The checked `remember_my_face`, with `replace` (#1057).
            names_checked: true,
            mark: Some("/var/lib/lampos-voice/session".into()),
            onboarding: Some("/var/lib/lampos-voice/onboarding.json".into()),
        };
        let mut tools = declared(&settings);
        tools.push(crate::attention::name_tool());
        let count = tools.len();
        let (kept, left_out) = admitted(tools);
        assert!(left_out.is_empty(), "{left_out:?}");
        assert_eq!(kept.len(), count);
    }
    use std::os::unix::net::UnixListener;

    fn directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lamp-tools-{name}-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn schedule_directory(name: &str) -> PathBuf {
        let base = if Path::new("/private/tmp").is_dir() {
            PathBuf::from("/private/tmp")
        } else {
            std::env::temp_dir()
        };
        let path = base.join(format!(
            "lvs-{name}-{}-{:x}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn schedule_flags_are_exact_independent_opt_ins_and_leave_old_arguments_unchanged() {
        let old = arguments(Some("on"), Some("/run/actions"), None, Some("on"));
        for value in [
            None,
            Some("off"),
            Some("ON"),
            Some("true"),
            Some(" on"),
            Some(""),
        ] {
            let mut flags = old.clone();
            flags.extend(schedule_arguments(value, value));
            assert_eq!(flags, old);
            let (settings, rest) = take(&flags).unwrap();
            assert!(rest.is_empty());
            assert!(!settings.schedule_tools && !settings.schedule_speak);
        }
        for (tools, speak) in [(true, false), (false, true), (true, true)] {
            let mut flags = old.clone();
            flags.extend(schedule_arguments(
                tools.then_some("on"),
                speak.then_some("on"),
            ));
            flags.push("remainder".into());
            let (settings, rest) = take(&flags).unwrap();
            assert_eq!(
                (settings.schedule_tools, settings.schedule_speak),
                (tools, speak)
            );
            assert_eq!(rest, ["remainder"]);
            assert!(settings.decline && settings.clock);
            assert_eq!(settings.exec, Some("/run/actions".into()));
        }
    }

    /// The reviewer's P1 on #1038: `LAMP_SCHEDULE_SPEAK=skill` is Schedule
    /// speech on for the parent (`reminders::speaks`), so the provider
    /// worker gets its flag too. The flag came only for `on`, and under
    /// `skill` the worker refused every reminder, so none was said. Exactly
    /// `on` and `skill`, as the parent reads them.
    #[test]
    fn schedule_speech_by_skill_gives_the_worker_its_flag() {
        let old = arguments(Some("on"), Some("/run/actions"), None, Some("on"));
        for (speak, on) in [
            (Some("on"), true),
            (Some("skill"), true),
            (Some("SKILL"), false),
            (Some("skills"), false),
            (Some(" skill"), false),
            (None, false),
        ] {
            let mut flags = old.clone();
            flags.extend(schedule_arguments(None, speak));
            let (settings, rest) = take(&flags).unwrap();
            assert!(rest.is_empty());
            assert_eq!(settings.schedule_speak, on, "{speak:?}");
            assert_eq!(crate::reminders::speaks(speak), on, "{speak:?}");
        }
    }

    #[test]
    fn without_the_onboarding_switch_nothing_changes() {
        let old: Vec<String> = arguments(Some("on"), None, None, Some("on"));
        let (off, rest) = take(&old).unwrap();
        assert!(rest.is_empty());
        assert_eq!(off.onboarding, None);
        let names = |settings: &Settings| {
            declared(settings)
                .into_iter()
                .map(|tool| tool.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&off), [NOT_FOR_ME, THE_TIME]);
        for name in [crate::onboarding::STEP, crate::onboarding::REDO] {
            assert_eq!(route(&off, name), Route::Nobody);
        }
        let mut flags = old.clone();
        flags.extend([
            crate::onboarding::FLAG.into(),
            "/var/lib/lampos-voice/onboarding.json".into(),
        ]);
        let (on, rest) = take(&flags).unwrap();
        assert!(rest.is_empty());
        let file = PathBuf::from("/var/lib/lampos-voice/onboarding.json");
        assert_eq!(on.onboarding, Some(file.clone()));
        assert_eq!(
            names(&on),
            [
                NOT_FOR_ME,
                THE_TIME,
                crate::onboarding::STEP,
                crate::onboarding::REDO
            ]
        );
        assert_eq!(route(&on, crate::onboarding::STEP), Route::Onboarding(file));
        flags.pop();
        flags.push("onboarding.json".into());
        assert!(take(&flags).is_err());
    }

    #[test]
    fn tasks_then_schedule_flags_keep_both_owners_and_their_declarations() {
        let dir = schedule_directory("tasks-schedule");
        let mut entries = vec![json!({"name":"light_on","description":"Old tool"})];
        entries.extend(
            [
                crate::agent_tasks::START,
                crate::agent_tasks::CONFIRM,
                crate::agent_tasks::CHECK,
            ]
            .into_iter()
            .chain(SCHEDULE_NAMES)
            .map(|name| json!({"name":name,"description":"Executive catalog entry"})),
        );
        std::fs::write(dir.join(CATALOG_FILE), json!({"tools":entries}).to_string()).unwrap();
        let mut flags = arguments(
            Some("on"),
            Some(dir.to_str().unwrap()),
            Some("/run/lampos-memory/memory.sock"),
            Some("on"),
        );
        flags.extend([
            crate::agent_tasks::FLAG.into(),
            "/run/lampos-tasks".into(),
            crate::agent_tasks::TARGET_FLAG.into(),
            "workstation".into(),
        ]);
        flags.extend(schedule_arguments(Some("on"), Some("on")));
        flags.extend([
            crate::known_faces::FLAG.into(),
            "/run/lampos-eyes/who.sock".into(),
        ]);
        flags.extend(["--session".into(), "7".into()]);
        let (settings, rest) = take(&flags).unwrap();
        let place = crate::agent_tasks::Place {
            directory: "/run/lampos-tasks".into(),
            target: "workstation".into(),
            who: false,
            lead_in: false,
            asked: false,
            needs: None,
            staff_note: false,
        };
        assert_eq!(
            settings,
            Settings {
                decline: true,
                exec: Some(dir.clone()),
                memory: Some("/run/lampos-memory/memory.sock".into()),
                clock: true,
                tasks: Some(place.clone()),
                schedule_tools: true,
                schedule_speak: true,
                who: Some("/run/lampos-eyes/who.sock".into()),
                names_checked: false,
                mark: None,
                onboarding: None,
            }
        );
        assert_eq!(rest, ["--session", "7"]);
        let declared_names = |settings: &Settings| {
            declared(settings)
                .into_iter()
                .map(|tool| tool.name)
                .collect::<Vec<_>>()
        };
        let mut expected = vec![
            NOT_FOR_ME,
            THE_TIME,
            "remember",
            "recall",
            "forget",
            crate::agent_tasks::START,
            crate::agent_tasks::CONFIRM,
            crate::agent_tasks::CHECK,
            crate::known_faces::REMEMBER,
            crate::known_faces::FORGET,
            "light_on",
        ];
        expected.extend(SCHEDULE_NAMES);
        assert_eq!(declared_names(&settings), expected);
        for tool in crate::agent_tasks::tools(&place) {
            let declared = declared(&settings)
                .into_iter()
                .find(|entry| entry.name == tool.name)
                .unwrap();
            assert_eq!(declared.description, tool.description);
            assert_eq!(declared.parameters, tool.parameters);
            assert_eq!(route(&settings, &tool.name), Route::Tasks(place.clone()));
        }
        for name in SCHEDULE_NAMES {
            assert_eq!(route(&settings, name), Route::Schedule(dir.clone()));
        }
        assert_eq!(route(&settings, "light_on"), Route::Executive(dir.clone()));
        let eyes = PathBuf::from("/run/lampos-eyes/who.sock");
        for name in [crate::known_faces::REMEMBER, crate::known_faces::FORGET] {
            assert_eq!(route(&settings, name), Route::Faces(eyes.clone(), false));
            let blind = Settings {
                who: None,
                ..settings.clone()
            };
            assert_eq!(route(&blind, name), Route::Nobody);
            assert!(declared(&blind).iter().all(|tool| tool.name != name));
        }
        let schedule_off = Settings {
            schedule_tools: false,
            ..settings.clone()
        };
        assert_eq!(declared_names(&schedule_off), expected[..11]);
        for name in SCHEDULE_NAMES {
            assert_eq!(route(&schedule_off, name), Route::Nobody);
        }
        assert_eq!(
            route(&schedule_off, crate::agent_tasks::START),
            Route::Tasks(place)
        );
        let tasks_off = Settings {
            tasks: None,
            ..settings
        };
        assert!(
            declared_names(&tasks_off)
                .iter()
                .all(|name| !crate::agent_tasks::is_tool(name))
        );
        for name in [
            crate::agent_tasks::START,
            crate::agent_tasks::CONFIRM,
            crate::agent_tasks::CHECK,
        ] {
            assert_eq!(route(&tasks_off, name), Route::Nobody);
        }
        for name in SCHEDULE_NAMES {
            assert_eq!(route(&tasks_off, name), Route::Schedule(dir.clone()));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_stale_or_malicious_schedule_catalog_cannot_declare_or_route_when_off() {
        let dir = schedule_directory("schedule-catalog");
        let mut entries = vec![json!({"name":"light_on","description":"Old tool"})];
        entries.extend(
            SCHEDULE_NAMES
                .iter()
                .map(|name| json!({"name":name,"description":"Stale tool"})),
        );
        std::fs::write(dir.join(CATALOG_FILE), json!({"tools":entries}).to_string()).unwrap();
        let off = Settings {
            exec: Some(dir.clone()),
            ..Settings::default()
        };
        assert_eq!(
            declared(&off)
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            ["light_on"]
        );
        for name in SCHEDULE_NAMES {
            assert_eq!(route(&off, name), Route::Nobody);
            assert_eq!(
                route(
                    &Settings {
                        schedule_speak: true,
                        ..off.clone()
                    },
                    name
                ),
                Route::Nobody
            );
        }
        assert_eq!(route(&off, "light_on"), Route::Executive(dir.clone()));
        let on = Settings {
            schedule_tools: true,
            ..off
        };
        assert_eq!(declared(&on).len(), 7);
        for name in SCHEDULE_NAMES {
            assert_eq!(route(&on, name), Route::Schedule(dir.clone()));
            assert_eq!(
                route(
                    &Settings {
                        exec: None,
                        ..on.clone()
                    },
                    name
                ),
                Route::Nobody
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn schedule_exchange_keeps_the_call_envelope_and_committed_result() {
        let dir = schedule_directory("schedule-answer");
        let served = executive(
            &dir,
            b"{\"ok\":true,\"result\":{\"id\":7,\"state\":\"scheduled\"}}\n",
            Duration::ZERO,
        );
        let request = ToolCall {
            id: "provider-call-1".into(),
            name: "set_timer".into(),
            args: json!({"text":"Tea","seconds":60}),
        };
        assert_eq!(
            ask_schedule(&dir, &request, ASK_DEADLINE),
            (json!({"id":7,"state":"scheduled"}), true)
        );
        assert_eq!(
            served.join().unwrap(),
            json!({"op":"call","name":"set_timer","args":{"text":"Tea","seconds":60}})
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lost_or_unsendable_schedule_results_are_unknown_and_are_never_retried() {
        for (name, reply) in [
            ("lost", b"".as_slice()),
            ("malformed", b"{\"ok\":true}\n".as_slice()),
        ] {
            let dir = schedule_directory(name);
            let served = executive(&dir, reply, Duration::ZERO);
            let request = ToolCall {
                id: "1".into(),
                name: "cancel_reminder".into(),
                args: json!({"id":7}),
            };
            assert_eq!(
                ask_schedule(&dir, &request, ASK_DEADLINE),
                (schedule_uncertain(), false)
            );
            assert_eq!(served.join().unwrap()["name"], "cancel_reminder");
            std::fs::remove_dir_all(dir).unwrap();
        }
        let dir = schedule_directory("too-large");
        let listener = UnixListener::bind(dir.join(CALL_SOCKET)).unwrap();
        let served = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let answer =
                json!({"ok":true,"result":{"text":"x".repeat(lamp_gemini::MAX_TOOL_JSON_BYTES)}});
            writeln!(stream, "{answer}").unwrap();
        });
        assert_eq!(
            ask_schedule(&dir, &call("list_reminders"), ASK_DEADLINE),
            (schedule_uncertain(), false)
        );
        served.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: name.into(),
            args: json!({"colour":"blue"}),
        }
    }
    /// An executive that reads one request and answers what it is given,
    /// after a wait. Returns the request it read.
    fn executive(
        directory: &Path,
        answer: &'static [u8],
        wait: Duration,
    ) -> std::thread::JoinHandle<Value> {
        let listener = UnixListener::bind(directory.join(CALL_SOCKET)).unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            std::thread::sleep(wait);
            let _ = stream.write_all(answer);
            serde_json::from_str(&line).unwrap()
        })
    }

    #[test]
    fn with_a_memory_its_three_functions_are_declared_and_go_to_it_and_to_nobody_without() {
        let dir = directory("memory");
        // An executive that offers a function of the memory's name does
        // not get it.
        std::fs::write(
            dir.join(CATALOG_FILE),
            r#"{"tools":[{"name":"remember","description":"x"},{"name":"light_on","description":"x"}]}"#,
        )
        .unwrap();
        let with = Settings {
            decline: true,
            exec: Some(dir.clone()),
            memory: Some("/run/lampos-memory/memory.sock".into()),
            clock: false,
            ..Default::default()
        };
        let names: Vec<String> = declared(&with).into_iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            [NOT_FOR_ME, "remember", "recall", "forget", "light_on"]
        );
        for name in ["remember", "recall", "forget"] {
            assert_eq!(
                route(&with, name),
                Route::Memory("/run/lampos-memory/memory.sock".into())
            );
        }
        assert_eq!(route(&with, "light_on"), Route::Executive(dir.clone()));
        let without = Settings {
            memory: None,
            ..with
        };
        let names: Vec<String> = declared(&without)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(names, [NOT_FOR_ME, "light_on"]);
        // The executive is never asked to remember: it is not its work.
        assert_eq!(route(&without, "remember"), Route::Nobody);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_on_and_an_absolute_path_become_flags_and_they_are_taken_from_the_front() {
        assert!(arguments(None, None, None, None).is_empty());
        assert!(
            arguments(
                Some("yes"),
                Some("relative/dir"),
                Some("memory.sock"),
                Some("yes")
            )
            .is_empty()
        );
        let all = arguments(
            Some("on"),
            Some("/run/lampos-actions"),
            Some("/run/lampos-memory/memory.sock"),
            None,
        );
        let memory = crate::remember::FLAG;
        assert_eq!(
            all,
            [
                FLAG,
                ACTIONS_FLAG,
                "/run/lampos-actions",
                memory,
                "/run/lampos-memory/memory.sock"
            ]
        );
        let mut flags = all.clone();
        flags.push("rest".into());
        let (settings, rest) = take(&flags).unwrap();
        assert_eq!(
            settings,
            Settings {
                decline: true,
                exec: Some("/run/lampos-actions".into()),
                memory: Some("/run/lampos-memory/memory.sock".into()),
                clock: false,
                ..Default::default()
            }
        );
        assert_eq!(rest, ["rest"]);
        let (settings, rest) = take(&flags[1..]).unwrap();
        assert!(!settings.decline && settings.exec.is_some() && settings.memory.is_some());
        assert_eq!(rest, ["rest"]);
        let (settings, rest) = take(&flags[3..]).unwrap();
        assert!(settings.exec.is_none() && settings.memory.is_some());
        assert_eq!(rest, ["rest"]);
        let (settings, rest) = take(&flags[5..]).unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(rest, ["rest"]);
        assert!(take(&[ACTIONS_FLAG.to_owned()]).is_err());
        assert!(take(&[ACTIONS_FLAG.to_owned(), "relative".to_owned()]).is_err());
        assert!(take(&[memory.to_owned()]).is_err());
        assert!(take(&[memory.to_owned(), "relative".to_owned()]).is_err());
    }

    #[test]
    fn a_session_is_told_of_the_voices_own_function_and_of_a_whole_readable_catalog_or_none_of_it()
    {
        assert!(declared(&Settings::default()).is_empty());
        let own = Settings {
            decline: true,
            exec: None,
            memory: None,
            clock: false,
            ..Default::default()
        };
        let told = declared(&own);
        assert_eq!(told.len(), 1);
        assert_eq!(told[0].name, NOT_FOR_ME);
        // What the link will be given must be something it accepts.
        assert!(told[0].validate().is_ok());

        let dir = directory("catalog");
        let with = Settings {
            decline: true,
            exec: Some(dir.clone()),
            memory: None,
            clock: false,
            ..Default::default()
        };
        // No file yet: the executive has not started.
        assert_eq!(declared(&with).len(), 1);
        let write = |text: &str| std::fs::write(dir.join(CATALOG_FILE), text).unwrap();
        write(
            r#"{"tools":[{"name":"light_on","description":"Turn the light on."},
            {"name":"light_colour","description":"Set the colour.","parameters":{"type":"object","properties":{"colour":{"type":"string","enum":["red","blue"]}},"required":["colour"]}},
            {"name":"not_for_me","description":"A name the voice keeps."}]}"#,
        );
        let names: Vec<String> = declared(&with).into_iter().map(|tool| tool.name).collect();
        assert_eq!(names, [NOT_FOR_ME, "light_on", "light_colour"]);
        // The executive's alone, when the voice's own is off.
        let alone = Settings {
            decline: false,
            exec: Some(dir.clone()),
            memory: None,
            clock: false,
            ..Default::default()
        };
        assert_eq!(declared(&alone).len(), 2);
        // One function the link would refuse, a name twice, a file that is
        // not the shape or not JSON, a file too large: none of it is told.
        for unsound in [
            r#"{"tools":[{"name":"light_on","description":"ok"},{"name":"Light Off","description":"bad name"}]}"#,
            r#"{"tools":[{"name":"light_on","description":"a"},{"name":"light_on","description":"b"}]}"#,
            r#"{"tools":[{"name":"light_on","description":"a","extra":1}]}"#,
            r#"[{"name":"light_on","description":"a"}]"#,
            "not json",
        ] {
            write(unsound);
            assert_eq!(declared(&with).len(), 1, "{unsound}");
        }
        // And each is said, with why (#799).
        let refused = |text: &str| {
            write(text);
            read_catalog(&dir.join(CATALOG_FILE)).unwrap_err()
        };
        assert_eq!(
            refused(
                r#"{"tools":[{"name":"light_on","description":"ok"},{"name":"Light Off","description":"bad name"}]}"#
            ),
            "tools.json has what the link would refuse: Light Off (a name the link does not take)"
        );
        assert_eq!(
            refused(
                r#"{"tools":[{"name":"light_on","description":"a"},{"name":"light_on","description":"b"}]}"#
            ),
            "tools.json has what the link would refuse: light_on (a name given twice)"
        );
        assert_eq!(
            refused("not json"),
            "tools.json is not {\"tools\":[...]} of functions"
        );
        write(&format!(
            r#"{{"tools":[{{"name":"light_on","description":"a"}}],"pad":"{}"}}"#,
            "p".repeat(MAX_CATALOG_BYTES as usize)
        ));
        assert_eq!(declared(&with).len(), 1);
        assert_eq!(
            read_catalog(&dir.join(CATALOG_FILE)).unwrap_err(),
            format!("tools.json is over {MAX_CATALOG_BYTES} bytes")
        );
        // More than a session takes is no fault of the file: it is read
        // whole, and the first that fit are told (Sixth on #1020).
        let many: Vec<serde_json::Value> = (0..40)
            .map(|index| json!({"name":format!("f{index}"),"description":"a"}))
            .collect();
        write(&json!({"tools":many}).to_string());
        assert_eq!(
            read_catalog(&dir.join(CATALOG_FILE)).map(|tools| tools.len()),
            Ok(40)
        );
        let names: Vec<String> = declared(&with).into_iter().map(|tool| tool.name).collect();
        assert_eq!(names.len(), lamp_gemini::MAX_TOOLS);
        assert_eq!(
            [&names[0], &names[1], &names[31]],
            [NOT_FOR_ME, "f0", "f30"]
        );
        // No file: no executive, nothing to say.
        std::fs::remove_file(dir.join(CATALOG_FILE)).unwrap();
        assert_eq!(read_catalog(&dir.join(CATALOG_FILE)), Ok(Vec::new()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_call_goes_to_whoever_runs_it_and_to_nobody_when_that_is_switched_off() {
        let dir = PathBuf::from("/run/lampos-actions");
        let all = Settings {
            decline: true,
            exec: Some(dir.clone()),
            memory: None,
            clock: false,
            ..Default::default()
        };
        assert_eq!(route(&all, NOT_FOR_ME), Route::Decline);
        assert_eq!(route(&all, "light_on"), Route::Executive(dir.clone()));
        let own = Settings {
            decline: true,
            exec: None,
            memory: None,
            clock: false,
            ..Default::default()
        };
        assert_eq!(route(&own, "light_on"), Route::Nobody);
        // Not declared, so not meant: it is never handed to the executive.
        let exec = Settings {
            decline: false,
            exec: Some(dir),
            memory: None,
            clock: false,
            ..Default::default()
        };
        assert_eq!(route(&exec, NOT_FOR_ME), Route::Nobody);
        assert_eq!(route(&Settings::default(), "light_on"), Route::Nobody);
    }

    #[test]
    fn work_for_an_agent_is_declared_and_routed_only_with_its_directory() {
        let place = crate::agent_tasks::Place {
            directory: "/run/lampos-tasks".into(),
            target: "office".into(),
            who: false,
            lead_in: false,
            asked: false,
            needs: None,
            staff_note: false,
        };
        let with = Settings {
            tasks: Some(place.clone()),
            ..Settings::default()
        };
        let names: Vec<String> = declared(&with).into_iter().map(|tool| tool.name).collect();
        assert_eq!(
            names,
            [
                crate::agent_tasks::START,
                crate::agent_tasks::CONFIRM,
                crate::agent_tasks::CHECK
            ]
        );
        assert_eq!(
            route(&with, crate::agent_tasks::CONFIRM),
            Route::Tasks(place)
        );
        assert_eq!(
            route(&Settings::default(), crate::agent_tasks::START),
            Route::Nobody
        );
        assert!(declared(&Settings::default()).is_empty());
    }

    #[test]
    fn the_time_is_declared_and_answered_here_only_when_switched_on() {
        assert_eq!(arguments(None, None, None, Some("on")), [CLOCK_FLAG]);
        let flags = [CLOCK_FLAG.to_owned(), "rest".to_owned()];
        let (settings, rest) = take(&flags).unwrap();
        assert!(settings.clock && !settings.decline && settings.exec.is_none());
        assert_eq!(rest, ["rest"]);
        let on = Settings {
            clock: true,
            ..Settings::default()
        };
        let names = |settings: &Settings| {
            declared(settings)
                .into_iter()
                .map(|tool| tool.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&on), [THE_TIME]);
        assert!(names(&Settings::default()).is_empty());
        assert_eq!(route(&on, THE_TIME), Route::Clock);
        // Off, it is nobody's, and never the executive's either.
        let exec = Settings {
            exec: Some("/run/lampos-actions".into()),
            ..Settings::default()
        };
        assert_eq!(route(&exec, THE_TIME), Route::Nobody);
        assert_eq!(route(&Settings::default(), THE_TIME), Route::Nobody);
        // Spoken, not silent: the model is to say what it is told.
        assert!(!the_time().silent);
        let told = time_now();
        assert!(told["now"].is_string() || told["error"].is_string());
    }

    /// #803: marking a bad moment is declared, and routed, only when the
    /// worker was given the session's folder; never the executive's.
    #[test]
    fn marking_for_review_is_declared_and_routed_only_with_its_flag() {
        let flags = [
            crate::marked::FLAG.to_owned(),
            "/var/lib/lampOS/voice/sessions/abc".to_owned(),
            "rest".to_owned(),
        ];
        let (settings, rest) = take(&flags).unwrap();
        assert_eq!(
            settings.mark.as_deref(),
            Some(Path::new("/var/lib/lampOS/voice/sessions/abc"))
        );
        assert_eq!(rest, ["rest"]);
        assert!(take(&[crate::marked::FLAG.to_owned(), "relative".to_owned()]).is_err());
        let names = |settings: &Settings| {
            declared(settings)
                .into_iter()
                .map(|tool| tool.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&settings), [crate::marked::MARK]);
        assert!(names(&Settings::default()).is_empty());
        assert_eq!(
            route(&settings, crate::marked::MARK),
            Route::Mark("/var/lib/lampOS/voice/sessions/abc".into())
        );
        let exec = Settings {
            exec: Some("/run/lampos-actions".into()),
            ..Settings::default()
        };
        assert_eq!(route(&exec, crate::marked::MARK), Route::Nobody);
        assert!(crate::marked::tool().validate().is_ok());
    }

    #[test]
    fn the_skills_tools_are_told_after_the_executives_and_go_to_the_skills() {
        let dir = directory("skills");
        let settings = Settings {
            exec: Some(dir.clone()),
            ..Default::default()
        };
        // Nothing written by the skills: nothing changes.
        std::fs::write(
            dir.join(CATALOG_FILE),
            r#"{"tools":[{"name":"light_on","description":"Turn the light on."}]}"#,
        )
        .unwrap();
        assert_eq!(declared(&settings).len(), 1);
        assert_eq!(
            route(&settings, "check_my_work"),
            Route::Executive(dir.clone())
        );
        std::fs::create_dir(dir.join(SKILLS_DIR)).unwrap();
        std::fs::write(
            dir.join(SKILLS_DIR).join(CATALOG_FILE),
            r#"{"tools":[{"name":"check_my_work","description":"When someone asks the lamp to check their work."},
            {"name":"light_on","description":"A name the executive has."},
            {"name":"not_for_me","description":"A name the voice keeps."}]}"#,
        )
        .unwrap();
        let names: Vec<String> = declared(&settings)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(names, ["light_on", "check_my_work"]);
        assert_eq!(
            route(&settings, "check_my_work"),
            Route::Executive(dir.join(SKILLS_DIR))
        );
        assert_eq!(route(&settings, "light_on"), Route::Executive(dir.clone()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_executive_is_asked_in_one_line_and_its_result_or_its_words_go_to_the_model() {
        let dir = directory("ask");
        let served = executive(
            &dir,
            b"{\"ok\":true,\"result\":{\"light\":\"blue\"}}\n",
            Duration::ZERO,
        );
        let (told, answered) = ask(&dir, &call("light_colour"), ASK_DEADLINE);
        assert_eq!(told, json!({"light":"blue"}));
        assert!(answered);
        assert_eq!(
            served.join().unwrap(),
            json!({"op":"call","name":"light_colour","args":{"colour":"blue"}})
        );
        std::fs::remove_file(dir.join(CALL_SOCKET)).unwrap();

        let served = executive(
            &dir,
            b"{\"ok\":false,\"error\":\"The light is already at its brightest.\"}\n",
            Duration::ZERO,
        );
        let (told, answered) = ask(&dir, &call("light_brighter"), ASK_DEADLINE);
        assert_eq!(
            told,
            json!({"error":"The light is already at its brightest."})
        );
        assert!(answered);
        served.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The executive gone, late, cut off or talking nonsense: the model is
    /// told in words it can say, inside the deadline, and nothing else
    /// happens to the voice.
    #[test]
    fn an_executive_that_is_gone_late_or_unreadable_costs_one_deadline_and_an_honest_answer() {
        let dir = directory("gone");
        // No socket at all.
        let started = Instant::now();
        assert_eq!(
            ask(&dir, &call("light_on"), ASK_DEADLINE),
            (unavailable(), false)
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        // A socket file nobody listens on.
        drop(UnixListener::bind(dir.join(CALL_SOCKET)).unwrap());
        assert_eq!(
            ask(&dir, &call("light_on"), ASK_DEADLINE),
            (unavailable(), false)
        );
        std::fs::remove_file(dir.join(CALL_SOCKET)).unwrap();
        // One that answers after the deadline.
        let deadline = Duration::from_millis(200);
        let served = executive(
            &dir,
            b"{\"ok\":true,\"result\":{}}\n",
            Duration::from_millis(600),
        );
        let started = Instant::now();
        assert_eq!(
            ask(&dir, &call("light_on"), deadline),
            (unavailable(), false)
        );
        let waited = started.elapsed();
        assert!(
            waited >= deadline && waited < Duration::from_millis(500),
            "{waited:?}"
        );
        served.join().unwrap();
        std::fs::remove_file(dir.join(CALL_SOCKET)).unwrap();
        // An answer larger than a session will carry to the model.
        let large: &'static [u8] = Box::leak(
            format!(
                "{{\"ok\":true,\"result\":{{\"pad\":\"{}\"}}}}\n",
                "p".repeat(lamp_gemini::MAX_TOOL_JSON_BYTES)
            )
            .into_bytes()
            .into_boxed_slice(),
        );
        let served = executive(&dir, large, Duration::ZERO);
        assert_eq!(
            ask(&dir, &call("light_on"), ASK_DEADLINE),
            (unavailable(), false)
        );
        served.join().unwrap();
        std::fs::remove_file(dir.join(CALL_SOCKET)).unwrap();
        // One that hangs up without a whole line, and three that answer
        // something that is not an answer.
        for nonsense in [
            b"{\"ok\":true".as_slice(),
            b"hello\n".as_slice(),
            b"{\"result\":{}}\n".as_slice(),
            b"{\"ok\":false}\n".as_slice(),
        ] {
            let served = executive(&dir, nonsense, Duration::ZERO);
            assert_eq!(
                ask(&dir, &call("light_on"), ASK_DEADLINE),
                (unavailable(), false),
                "{}",
                String::from_utf8_lossy(nonsense)
            );
            served.join().unwrap();
            std::fs::remove_file(dir.join(CALL_SOCKET)).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
