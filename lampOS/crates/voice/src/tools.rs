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
    collections::BTreeSet,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[cfg(test)]
#[path = "tools/schedule_integration_tests.rs"]
mod schedule_integration_tests;

#[path = "tools/schedule_asked.rs"]
pub mod schedule_asked;

/// Where the coordinator reads whether the model may say a thing was not
/// for it. `on` only.
pub const ENVIRONMENT: &str = "LAMP_TOOLS";
/// The provider worker's flag for it.
pub const FLAG: &str = "--tools";
/// Where the coordinator reads the executive's directory: an absolute path.
pub const ACTIONS_ENVIRONMENT: &str = "LAMP_ACTIONS_DIR";
/// The provider worker's flag for it, followed by the path.
pub const ACTIONS_FLAG: &str = "--actions-dir";
/// Where the coordinator reads the skills' own folder, `lamp-skills
/// --tools-dir`: an absolute path.
pub const SKILLS_ENVIRONMENT: &str = "LAMP_SKILLS_DIR";
/// The provider worker's flag for it, followed by the path.
pub const SKILLS_FLAG: &str = "--skills-dir";

/// The function the model calls instead of speaking.
pub const NOT_FOR_ME: &str = "not_for_me";
/// The function the model calls to know the time now.
pub const THE_TIME: &str = "the_time";
/// Where the coordinator reads whether the model may ask the time. `on`
/// only.
pub const CLOCK_ENVIRONMENT: &str = "LAMP_CLOCK";
/// The provider worker's flag for it.
pub const CLOCK_FLAG: &str = "--clock";
/// Where the coordinator reads whether [`THE_TIME`] takes a place
/// (`LAMP_TIME_PLACES`, #1547): `on` only, and only with [`CLOCK_ENVIRONMENT`].
pub const PLACES_ENVIRONMENT: &str = "LAMP_TIME_PLACES";
/// The provider worker's flag for it, right after [`CLOCK_FLAG`].
pub const PLACES_FLAG: &str = "--time-places";
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
/// The skills' folder in the executive's directory, where `lamp-skills
/// --tools-dir` kept its `tools.json` and `call.sock` before they had a
/// folder of their own ([`Settings::skills`]): systemd removed it with the
/// executive's at each of its restarts, and the skills, sandboxed, could
/// not make it again (lamp-4ace, 19 Oct 04:33). Still read and called,
/// after the skills' own, for one release, so a voice and skills from
/// different builds still meet ([`skills_folders`]).
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
    /// The skills' own folder (`lamp-skills --tools-dir`): their
    /// `tools.json` and `call.sock`, read and called as the executive's
    /// are, after it.
    pub skills: Option<PathBuf>,
    /// The memory's socket ([`crate::remember`]).
    pub memory: Option<PathBuf>,
    /// The model may call [`THE_TIME`].
    pub clock: bool,
    /// [`THE_TIME`] takes a place, and its answer says whose time it is
    /// (`--time-places`, #1547). Only with `clock`.
    pub places: bool,
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
    /// Save only the face seen speaking this exact request.
    pub remember_speaker: bool,
    /// The person at the lamp may say yes or no to being named in Who was
    /// here (`LAMP_WHO_WAS_HERE_NAMES=on`, [`crate::known_faces::TELL`]).
    pub who_visits: bool,
    /// A face added from a photo is asked about, and its person's yes or
    /// no told to the eyes (`LAMP_PHOTO_FACES=on`, [`crate::photo_faces`]).
    pub photo_faces: bool,
    /// The serving session's folder, for marking a bad moment
    /// ([`crate::marked`]).
    pub mark: Option<PathBuf>,
    /// The onboarding file, whose mailbox the model's reports go to
    /// ([`crate::onboarding`]).
    pub onboarding: Option<PathBuf>,
    /// The skills are reached through one function, `use_skill`
    /// ([`crate::use_skill`], `LAMP_USE_SKILL=on`), not one each.
    pub use_skill: bool,
    /// The person may stop health and crash reports by voice
    /// ([`crate::reports_stop`], `LAMP_REPORTS=on`, #1683).
    pub reports_stop: bool,
    /// Typed local recording offers for an explicitly spoken request.
    pub requested_audio: bool,
    /// The person may put the lamp to sleep by voice
    /// ([`crate::sleep_asked`], `LAMP_SLEEP_ASKED=on`, #3748).
    pub sleep_asked: bool,
}

/// The provider worker's arguments for what the environment says.
pub fn arguments_from_environment() -> Vec<String> {
    let mut flags = arguments(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var(ACTIONS_ENVIRONMENT).ok().as_deref(),
        std::env::var(SKILLS_ENVIRONMENT).ok().as_deref(),
        std::env::var(crate::remember::ENVIRONMENT).ok().as_deref(),
        std::env::var(CLOCK_ENVIRONMENT).ok().as_deref(),
        std::env::var(PLACES_ENVIRONMENT).ok().as_deref(),
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
        if crate::remember_speaker::switched_on(
            std::env::var(crate::remember_speaker::ENVIRONMENT)
                .ok()
                .as_deref(),
        ) {
            flags.push(crate::remember_speaker::FLAG.to_owned());
        }
        flags.extend(crate::known_faces::visits_arguments(
            std::env::var(crate::known_faces::VISITS_ENVIRONMENT)
                .ok()
                .as_deref(),
        ));
        if crate::photo_faces::on() {
            flags.push(crate::photo_faces::FLAG.to_owned());
        }
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
    flags.extend(crate::use_skill::arguments(
        std::env::var(crate::use_skill::ENVIRONMENT).ok().as_deref(),
    ));
    flags.extend(crate::reports_stop::arguments(
        std::env::var(crate::reports_stop::ENVIRONMENT)
            .ok()
            .as_deref(),
    ));
    flags.extend(crate::requested_audio::arguments_from_environment());
    flags.extend(crate::sleep_asked::arguments(
        std::env::var(crate::sleep_asked::ENVIRONMENT)
            .ok()
            .as_deref(),
    ));
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
    skills: Option<&str>,
    memory: Option<&str>,
    clock: Option<&str>,
    places: Option<&str>,
) -> Vec<String> {
    let mut arguments = Vec::new();
    if decline == Some("on") {
        arguments.push(FLAG.to_owned());
    }
    if let Some(directory) = exec.filter(|path| Path::new(path).is_absolute()) {
        arguments.push(ACTIONS_FLAG.to_owned());
        arguments.push(directory.to_owned());
    }
    if let Some(folder) = skills.filter(|path| Path::new(path).is_absolute()) {
        arguments.push(SKILLS_FLAG.to_owned());
        arguments.push(folder.to_owned());
    }
    if let Some(socket) = memory.filter(|path| Path::new(path).is_absolute()) {
        arguments.push(crate::remember::FLAG.to_owned());
        arguments.push(socket.to_owned());
    }
    if clock == Some("on") {
        arguments.push(CLOCK_FLAG.to_owned());
        if places == Some("on") {
            arguments.push(PLACES_FLAG.to_owned());
        }
    }
    arguments
}

/// Take a leading `--tools`, `--actions-dir PATH`, `--skills-dir PATH`,
/// `--memory-socket PATH` and `--clock` off a worker's flags.
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
        && flag == SKILLS_FLAG
    {
        let [folder, rest @ ..] = rest else {
            return Err(std::io::Error::other("--skills-dir needs a path"));
        };
        if !Path::new(folder).is_absolute() {
            return Err(std::io::Error::other("--skills-dir must be absolute"));
        }
        settings.skills = Some(PathBuf::from(folder));
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
        if let [flag, rest @ ..] = flags
            && flag == PLACES_FLAG
        {
            settings.places = true;
            flags = rest;
        }
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
        if let [flag, rest @ ..] = flags
            && flag == crate::remember_speaker::FLAG
        {
            settings.remember_speaker = true;
            flags = rest;
        }
        if let [flag, rest @ ..] = flags
            && flag == crate::known_faces::VISITS_FLAG
        {
            settings.who_visits = true;
            flags = rest;
        }
        if let [flag, rest @ ..] = flags
            && flag == crate::photo_faces::FLAG
        {
            settings.photo_faces = true;
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
    if let [flag, rest @ ..] = flags
        && flag == crate::use_skill::FLAG
    {
        settings.use_skill = true;
        flags = rest;
    }
    if let [flag, rest @ ..] = flags
        && flag == crate::reports_stop::FLAG
    {
        settings.reports_stop = true;
        flags = rest;
    }
    if let [flag, rest @ ..] = flags
        && flag == crate::requested_audio::FLAG
    {
        settings.requested_audio = true;
        flags = rest;
    }
    if let [flag, rest @ ..] = flags
        && flag == crate::sleep_asked::FLAG
    {
        settings.sleep_asked = true;
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

fn the_time(places: bool) -> Tool {
    if places {
        return Tool {
            name: THE_TIME.into(),
            description: "Call this whenever you need the time or the date: when someone asks what time \
                          it is, what day it is, or how long until something. For the time somewhere \
                          else, such as \"what time is it in London\", give the place as its time \
                          zone; the time you know is only the time here. Then say it as it gives it \
                          \"said aloud\", with its hour, and for another place the place it names. \
                          Not for \"now\" or \
                          \"today\" in a question about something else, and never say the time unless \
                          you were asked for it."
                .into(),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "place": {
                        "type": "string",
                        "description": "The time zone of the place asked about, as its IANA name: Europe/London, America/New_York, Asia/Tokyo. Leave it out for the time here."
                    }
                }
            })),
            silent: false,
        };
    }
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
/// not be read. With `places` (`LAMP_TIME_PLACES`, #1547) and a `place`
/// in the call's arguments, that place's time and the lamp's beside it, or
/// that the place is not known: never the lamp's own as the place's.
pub fn time_now_for(places: bool, args: &Value) -> Value {
    match args.get("place") {
        // A place given as anything but words is no place: never the lamp's
        // own time in its stead (Jarvis on #1575).
        Some(place) if places && !place.is_string() && !place.is_null() => {
            json!({ "error": crate::local_time::PLACE_UNKNOWN })
        }
        Some(Value::String(place)) if places && !place.trim().is_empty() => {
            match crate::local_time::now_in(place) {
                Ok(Some(now)) => json!({ "now": now }),
                Ok(None) => json!({"error": "My clock could not be read just now."}),
                Err(why) => json!({ "error": why }),
            }
        }
        // With the switch every answer says whose time it is, the lamp's
        // own too (the reviewer on #1589).
        _ if places => match crate::local_time::now_here_named() {
            Ok(Some(now)) => json!({ "now": now }),
            Ok(None) => json!({"error": "My clock could not be read just now."}),
            Err(unset) => json!({ "error": unset }),
        },
        _ => time_now(),
    }
}

/// [`time_now_for`] with no place: the lamp's own time.
pub fn time_now() -> Value {
    match crate::local_time::now_here() {
        Ok(Some(now)) => json!({ "now": now }),
        Ok(None) => json!({"error": "My clock could not be read just now."}),
        Err(unset) => json!({ "error": unset }),
    }
}

/// The functions a session opened now is told of: the voice's own if they
/// are switched on, then the executive's if its list is there and reads,
/// then the skills', each its own or, with `use_skill`, all through that one
/// ([`crate::use_skill`], #1668). A name the voice uses itself is left out
/// of the executive's.
///
/// Past [`lamp_gemini::MAX_TOOLS`] the bound cuts by [`fit`]: only the
/// skills' functions that are not a core skill's, the last first.
pub fn declared(settings: &Settings) -> Vec<Tool> {
    let (tools, cut, core) = uncut(settings);
    fit(tools, &cut, &core)
}

/// [`declared`]'s functions before the bound, the order it cuts them in,
/// and the core skills' functions, never cut (the owner's rule, on #2614):
/// the cut is `reports_state` with `LAMP_REPORTS=on` (reading reports'
/// state is rare; `reports_stop` stays), then the skills' functions that
/// are not a core skill's ([`core_skills`]), the last first. With
/// `LAMP_USE_SKILL=on`, `use_skill` carries every core skill, so it is a
/// core name (the first builder on #2644).
pub fn uncut(settings: &Settings) -> (Vec<Tool>, Vec<String>, BTreeSet<String>) {
    uncut_within(settings, lamp_gemini::MAX_TOOLS)
}

/// [`uncut`] with the executive's functions taken up to `bound`, not
/// Live's: the second route's own ([`crate::second_provider::MAX_TOOLS`]).
/// Taken up to Live's 32 there, each function more of the voice's own
/// pushed one of the lamp's own actions out of every chat request, with
/// room to spare (25 Oct: `LAMP_MEMORY_FORGET_ALL`'s `forget_everything`
/// and `LAMP_REPORTS`' two left `snooze_reminder` out).
pub fn uncut_within(
    settings: &Settings,
    bound: usize,
) -> (Vec<Tool>, Vec<String>, BTreeSet<String>) {
    let mut tools = core_tools_within(settings, bound);
    let skills = skills_after(settings, &tools);
    let mut core = core_skills(settings);
    let mut cut: Vec<String> = Vec::new();
    if settings.reports_stop {
        cut.push(crate::reports_stop::STATE_NAME.to_owned());
    }
    if settings.use_skill {
        tools.extend(crate::use_skill::function(&skills));
        core.insert(crate::use_skill::NAME.to_owned());
        return (tools, cut, core);
    }
    cut.extend(
        skills
            .iter()
            .rev()
            .filter(|tool| !core.contains(&tool.name))
            .map(|tool| tool.name.clone()),
    );
    tools.extend(skills);
    (tools, cut, core)
}

/// `tools` within [`lamp_gemini::MAX_TOOLS`], cut only by `cut`: each
/// named there left out in its order until the rest fit, and none other.
/// When nothing in `cut` is left and the rest are still past the bound,
/// the error is the list as it then is: the lamp's own and its core
/// skills' functions alone are over (the owner's rule, on #2614: at the
/// bound a core skill's function is never the one cut).
pub fn within_bound(tools: Vec<Tool>, cut: &[String]) -> Result<Vec<Tool>, Vec<Tool>> {
    within(tools, cut, lamp_gemini::MAX_TOOLS)
}

/// [`within_bound`] past `bound` functions, not Live's: the second route's
/// own ([`crate::second_provider::MAX_TOOLS`]).
pub fn within(mut tools: Vec<Tool>, cut: &[String], bound: usize) -> Result<Vec<Tool>, Vec<Tool>> {
    for name in cut {
        if tools.len() <= bound {
            break;
        }
        tools.retain(|tool| tool.name != *name);
    }
    if tools.len() <= bound {
        Ok(tools)
    } else {
        Err(tools)
    }
}

/// [`within_bound`]; when it fails, a session still opens: the last
/// functions that are not a core skill's (`core`) are left out until the
/// rest fit, and named on an error line, never silently. Whatever is cut
/// is said once, on a plain line naming each ([`said_cut`]).
pub fn fit(tools: Vec<Tool>, cut: &[String], core: &BTreeSet<String>) -> Vec<Tool> {
    fit_within(tools, cut, core, lamp_gemini::MAX_TOOLS)
}

/// [`fit`] past `bound` functions, not Live's: the second route's own
/// ([`crate::second_provider::MAX_TOOLS`]).
pub fn fit_within(
    tools: Vec<Tool>,
    cut: &[String],
    core: &BTreeSet<String>,
    bound: usize,
) -> Vec<Tool> {
    let before: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
    let kept = past_bound(tools, cut, core, bound);
    let left: Vec<String> = before
        .iter()
        .filter(|name| !kept.iter().any(|tool| tool.name == **name))
        .map(|name| said_name(name))
        .collect();
    if let Some(line) = said_cut(&left, bound) {
        eprintln!("{line}");
    }
    kept
}

/// The line naming the functions the bound cut, or `None` when nothing is
/// cut or this same cut was said last: the second route fits its
/// functions on each request, and the first builder on #2644 asked for
/// the cut said once, at start.
// ponytail: one process-wide last cut; a cut that changes and changes
// back is said again, which is then a start of its own.
fn said_cut(left: &[String], bound: usize) -> Option<String> {
    static LAST: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if left.is_empty() || *last == left {
        return None;
    }
    *last = left.to_vec();
    Some(format!(
        "lamp-voice: past the bound of {}, left out: {}",
        bound,
        left.join(", ")
    ))
}

fn past_bound(
    tools: Vec<Tool>,
    cut: &[String],
    core: &BTreeSet<String>,
    bound: usize,
) -> Vec<Tool> {
    within(tools, cut, bound).unwrap_or_else(|mut over| {
        let mut past = Vec::new();
        while over.len() > bound {
            let last = over
                .iter()
                .rposition(|tool| !core.contains(&tool.name))
                .unwrap_or(over.len() - 1);
            past.push(said_name(&over.remove(last).name));
        }
        eprintln!(
            "lamp-voice: error: the lamp's own and its core skills' functions alone are past the bound of {}; left out: {}",
            bound,
            past.join(", ")
        );
        over
    })
}

/// The skills' functions as their tools file says them, those a session
/// may be told: no name the voice or the executive already has, nor a
/// Schedule name. What `use_skill` reaches ([`crate::use_skill`]).
pub fn skill_tools(settings: &Settings) -> Vec<Tool> {
    skills_after(settings, &core_tools(settings))
}

fn skills_after(settings: &Settings, told: &[Tool]) -> Vec<Tool> {
    // The skills' after the executive's: a name already told stays its, and
    // a name the skills' own folder gives is its ([`skills_folders`]).
    let mut skills: Vec<Tool> = Vec::new();
    for folder in skills_folders(settings) {
        for tool in catalog(&folder.join(CATALOG_FILE)) {
            if !voices_own(&tool.name)
                && !is_schedule_tool(&tool.name)
                && !told.iter().any(|earlier| earlier.name == tool.name)
                && !skills.iter().any(|earlier| earlier.name == tool.name)
            {
                skills.push(tool);
            }
        }
    }
    skills
}

/// The voice's own functions and the executive's.
fn core_tools(settings: &Settings) -> Vec<Tool> {
    core_tools_within(settings, lamp_gemini::MAX_TOOLS)
}

/// [`core_tools`] with the executive's taken up to `bound` ([`uncut_within`]).
fn core_tools_within(settings: &Settings, bound: usize) -> Vec<Tool> {
    let mut tools = Vec::new();
    if settings.decline {
        tools.push(not_for_me());
    }
    if settings.clock {
        tools.push(the_time(settings.places));
    }
    if settings.memory.is_some() {
        tools.extend(crate::remember::tools());
    }
    if let Some(place) = &settings.tasks {
        tools.extend(crate::agent_tasks::tools(place));
    }
    if settings.who.is_some() {
        tools.extend(crate::known_faces::tools_for(face_switches(settings)));
        // A face added from a photo, asked about at the lamp (#1968).
        if settings.photo_faces {
            tools.extend(crate::photo_faces::tools());
        }
    }
    if settings.mark.is_some() {
        tools.push(crate::marked::tool());
    }
    if settings.onboarding.is_some() {
        tools.extend(crate::onboarding::tools());
    }
    // A system function, never a skill (#1683): before the executive's and
    // the skills', so it is never the one cut past 32; a skill cut for it is
    // named on the setup line (#1706).
    if settings.reports_stop {
        tools.push(crate::reports_stop::tool());
        tools.push(crate::reports_stop::state_tool());
    }
    if settings.sleep_asked {
        tools.push(crate::sleep_asked::tool());
    }
    if let Some(directory) = &settings.exec {
        let room = bound.saturating_sub(tools.len());
        tools.extend(
            catalog(&directory.join(CATALOG_FILE))
                .into_iter()
                .filter(|tool| {
                    !voices_own(&tool.name)
                        && (settings.schedule_tools || !is_schedule_tool(&tool.name))
                })
                .take(room),
        );
    }
    tools
}

/// Where the skills' functions are read and called, in order: their own
/// folder ([`Settings::skills`]), then the old one in the executive's
/// directory ([`SKILLS_DIR`]), for one release, and only while their own
/// has no socket: after the update, until actions restarts, the old one
/// keeps the old lamp-skills' catalog and a socket nobody answers (the
/// reviewer on #1656). A name the first gives is its.
pub fn skills_folders(settings: &Settings) -> Vec<PathBuf> {
    let own_live = settings
        .skills
        .as_ref()
        .is_some_and(|own| own.join(CALL_SOCKET).exists());
    every_skills_folder(settings)
        .into_iter()
        .filter(|folder| !own_live || settings.skills.as_ref() == Some(folder))
        .collect()
}

/// Both of the skills' folders, their own first, whether or not each is
/// read now: what is looked at for a change, and what counts as a skills'
/// route.
pub fn every_skills_folder(settings: &Settings) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = settings.skills.iter().cloned().collect();
    if let Some(directory) = &settings.exec {
        let old = directory.join(SKILLS_DIR);
        if !folders.contains(&old) {
            folders.push(old);
        }
    }
    folders
}

/// Whether `route` asks the skills, in one of their folders
/// ([`every_skills_folder`]), not the executive.
pub fn is_skills(settings: &Settings, route: &Route) -> bool {
    matches!(route, Route::Executive(folder) if every_skills_folder(settings).contains(folder))
}

/// A name the voice routes itself, whether or not it is switched on.
fn voices_own(name: &str) -> bool {
    name == NOT_FOR_ME
        || name == THE_TIME
        || crate::remember::is_tool(name)
        || crate::agent_tasks::is_tool(name)
        || crate::known_faces::is_tool(name)
        || crate::photo_faces::is_tool(name)
        || crate::marked::is_tool(name)
        || crate::onboarding::is_tool(name)
        || name == crate::use_skill::NAME
        || crate::reports_stop::is_tool(name)
        || crate::reports_stop::is_state_tool(name)
        || crate::sleep_asked::is_tool(name)
}

/// The skills' folder that runs `name`, when the executive does not.
fn skills_for(settings: &Settings, name: &str) -> Option<PathBuf> {
    let has = |path: PathBuf| catalog(&path).iter().any(|tool| tool.name == name);
    if let Some(directory) = &settings.exec
        && has(directory.join(CATALOG_FILE))
    {
        return None;
    }
    skills_folders(settings)
        .into_iter()
        .find(|folder| has(folder.join(CATALOG_FILE)))
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
    /// Ask the memory on this socket; with the eyes' socket and switches,
    /// a known person's "forget everything about me" forgets their face too
    /// ([`crate::remember::forget_everything_face`]).
    Memory(PathBuf, Option<(PathBuf, crate::known_faces::FaceSwitches)>),
    /// The time: answered here, at once.
    Clock,
    /// Work for an agent elsewhere, through the tasks part.
    Tasks(crate::agent_tasks::Place),
    /// Remember or forget a face, or say yes or no to being named, through
    /// the eyes on this socket, as switched; with the onboarding file, if
    /// any, which a face saved tells (#3740).
    Faces(PathBuf, crate::known_faces::FaceSwitches, Option<PathBuf>),
    /// Mark the last minute of this session for review.
    Mark(PathBuf),
    /// A step of the first meeting reported, through this onboarding file.
    Onboarding(PathBuf),
    /// Health and crash reports stopped, through `lamp-cloud` on the bus;
    /// `true` when the line's answer is held, as `lamp-cloud` keeps it
    /// (#1880).
    ReportsStop(bool),
    /// Whether reports are sent, and who stopped them and when, read from
    /// `lamp-cloud`'s `reports/state`: answered here, at once.
    ReportsState,
    /// A skill named in a call of `use_skill` ([`crate::use_skill`]), with
    /// the settings that find it: made the skill's own call when it is run.
    UseSkill(Box<Settings>),
    /// The person asked the lamp to sleep ([`crate::sleep_asked`]):
    /// answered here, at once, with the line to say, or not now while
    /// onboarding is under way.
    Sleep,
    /// Nobody here runs that.
    Nobody,
}

/// The executive's calls for the Autonomous app alone,
/// `lamp_actions::APP_CALLS` (a test holds the two lists equal; the voice
/// does not link `lamp-actions`).
pub const APP_CALLS: [&str; 2] = ["volume_level", "volume_to"];

/// The face functions' switches. The yes and no to being named need the
/// memory too: only with it is a line's scope a known person's (the face
/// cache), so without it every yes would be refused while still asked for.
fn face_switches(settings: &Settings) -> crate::known_faces::FaceSwitches {
    crate::known_faces::FaceSwitches {
        checked: settings.names_checked,
        remember_speaker: settings.remember_speaker,
        visits: settings.who_visits && settings.memory.is_some(),
    }
}

pub fn route(settings: &Settings, name: &str) -> Route {
    // The Autonomous app's two volume calls on the executive's socket
    // (`lamp-cloud`, #1396) are never the model's: one it names anyway is
    // run by nobody, so it never sets the volume (the reviewer on #1418).
    if APP_CALLS.contains(&name) {
        return Route::Nobody;
    }
    if settings.use_skill && name == crate::use_skill::NAME {
        return Route::UseSkill(Box::new(settings.clone()));
    }
    if is_schedule_tool(name) {
        return match (&settings.exec, settings.schedule_tools) {
            (Some(directory), true) => Route::Schedule(directory.clone()),
            _ => Route::Nobody,
        };
    }
    if let (Some(socket), true) = (&settings.memory, crate::remember::is_tool(name)) {
        return Route::Memory(
            socket.clone(),
            settings
                .who
                .clone()
                .map(|who| (who, face_switches(settings))),
        );
    }
    if crate::agent_tasks::is_tool(name) {
        return settings.tasks.clone().map_or(Route::Nobody, Route::Tasks);
    }
    if crate::known_faces::is_tool(name) || crate::photo_faces::is_tool(name) {
        return settings.who.clone().map_or(Route::Nobody, |socket| {
            Route::Faces(socket, face_switches(settings), settings.onboarding.clone())
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
    if crate::reports_stop::is_tool(name) {
        return if settings.reports_stop {
            Route::ReportsStop(false)
        } else {
            Route::Nobody
        };
    }
    if crate::sleep_asked::is_tool(name) {
        return if settings.sleep_asked {
            Route::Sleep
        } else {
            Route::Nobody
        };
    }
    if crate::reports_stop::is_state_tool(name) {
        return if settings.reports_stop {
            Route::ReportsState
        } else {
            Route::Nobody
        };
    }
    match (&settings.exec, name) {
        (_, NOT_FOR_ME) if settings.decline => Route::Decline,
        (_, THE_TIME) if settings.clock => Route::Clock,
        (exec, name)
            if name != NOT_FOR_ME
                && name != THE_TIME
                && !crate::remember::is_tool(name)
                && !crate::agent_tasks::is_tool(name) =>
        {
            match skills_for(settings, name) {
                Some(folder) => Route::Executive(folder),
                None => exec.clone().map_or(Route::Nobody, Route::Executive),
            }
        }
        _ => Route::Nobody,
    }
}

/// Whether a request with this call stays out of the memory's journal:
/// being asked to keep or forget something is not itself something to keep.
pub fn keeps_out(route: &Route, name: &str) -> bool {
    matches!(route, Route::Memory(..)) && name != crate::remember::RECALL
}

/// One call answered to the end, for a voice route that waits for each
/// call's answer before it goes on: the time from the lamp's clock, a
/// decline answered, a call nobody here runs told so, any other run by
/// [`run_call`]. Blocking: for a thread of its own. `not_for_me` itself is
/// the route's to act on before this.
pub fn answer(settings: &Settings, call: &ToolCall, request: u64, heard: bool) -> Value {
    answer_inner(settings, call, request, heard, None, None, false).told
}

/// A spoken request's immutable memory identity, never the current face.
pub fn answer_scoped(
    settings: &Settings,
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: &crate::memory_scope::ScopeLease,
) -> Value {
    answer_inner(settings, call, request, heard, None, Some(lease), false).told
}

/// [`answer`], or [`answer_scoped`] with a `lease`, for the second voice's
/// spoken turns: a `check_agent_task` answer keeps its news key
/// ([`crate::agent_tasks::NEWS_KEY`]), which the second route takes out of
/// what its model is told and puts on the turn as `NewsAnswered`, as the
/// Live worker does (#1390; the first builder's 20:15 on #1722).
pub fn answer_keeping_news(
    settings: &Settings,
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    answer_inner(settings, call, request, heard, None, lease, true).told
}

/// A local recording offer is private transport data, never model context.
pub(crate) struct ToolAnswer {
    pub told: Value,
    pub requested_audio: Option<lamp_msgs::RequestedAudioOffer>,
    /// The person asked the lamp to sleep, and the model was told the line
    /// to say first ([`crate::sleep_asked`]): the route tells the
    /// coordinator, which asks the sleep once that line has played.
    pub sleep: bool,
}

impl ToolAnswer {
    pub(crate) fn plain(told: Value) -> Self {
        Self {
            told,
            requested_audio: None,
            sleep: false,
        }
    }
}

/// Only a provider's accepted microphone turn supplies this context. The
/// legacy `answer` path, including Messages, cannot request a recording offer.
#[cfg(test)]
pub(crate) fn answer_spoken(
    settings: &Settings,
    call: &ToolCall,
    context: &lamp_msgs::RequestedAudioContext,
) -> ToolAnswer {
    answer_inner(
        settings,
        call,
        context.request,
        true,
        Some(context),
        None,
        false,
    )
}

/// The spoken audio capability and immutable memory lease belong to the same
/// admitted worker/session/request. Neither can replace the other's guards.
#[cfg(test)]
pub(crate) fn answer_spoken_scoped(
    settings: &Settings,
    call: &ToolCall,
    context: &lamp_msgs::RequestedAudioContext,
    lease: &crate::memory_scope::ScopeLease,
) -> ToolAnswer {
    answer_inner(
        settings,
        call,
        context.request,
        true,
        Some(context),
        Some(lease),
        false,
    )
}

/// The second route also carries private task news on its turn. Its Engine
/// removes that key before the model reads `told`; the audio offer is separate.
pub(crate) fn answer_spoken_scoped_keeping_news(
    settings: &Settings,
    call: &ToolCall,
    context: &lamp_msgs::RequestedAudioContext,
    lease: &crate::memory_scope::ScopeLease,
) -> ToolAnswer {
    answer_inner(
        settings,
        call,
        context.request,
        true,
        Some(context),
        Some(lease),
        true,
    )
}

fn answer_inner(
    settings: &Settings,
    call: &ToolCall,
    request: u64,
    heard: bool,
    context: Option<&lamp_msgs::RequestedAudioContext>,
    lease: Option<&crate::memory_scope::ScopeLease>,
    keep_news: bool,
) -> ToolAnswer {
    if !spoken_scope_current(lease, request, context) {
        return ToolAnswer::plain(unavailable());
    }
    let answer = match route(settings, &call.name) {
        Route::Clock => ToolAnswer::plain(time_now_for(settings.places, &call.args)),
        Route::ReportsState => ToolAnswer::plain(crate::reports_stop::state_now(
            &mut crate::reports_stop::Bus::of_the_lamp(),
        )),
        Route::Decline => ToolAnswer::plain(declined()),
        Route::Sleep => {
            let onboarding = crate::sleep_asked::during_onboarding(request);
            ToolAnswer {
                sleep: !onboarding,
                ..ToolAnswer::plain(crate::sleep_asked::told(onboarding))
            }
        }
        Route::Nobody => ToolAnswer::plain(unavailable()),
        route => {
            if keeps_out(&route, &call.name) {
                crate::remember::keep_out(request);
            }
            let began = std::time::Instant::now();
            let (whom, mut answer, answered) =
                run_spoken_call_inner(settings, &route, call, request, heard, context, lease);
            // Kept only for the second voice's spoken turn, which takes it
            // out before its model reads the answer; the key is never a
            // model's (#1390).
            if !keep_news && matches!(route, Route::Tasks(_)) {
                let _ = crate::agent_tasks::take_news_key(&mut answer.told);
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
            answer
        }
    };
    if spoken_scope_current(lease, request, context) {
        answer
    } else {
        ToolAnswer::plain(unavailable())
    }
}

/// What the model is told when a call about faces comes from anything but
/// the person at the lamp.
pub const FACES_IN_PERSON: &str =
    "Faces are kept and forgotten only when the person asks at the lamp, in person.";

/// [`answer`] for a typed conversation, a text from the Autonomous app
/// (#1673): nobody at the lamp is the one asking, so nothing is run as
/// theirs. The memory is the room's, never the person's in view, and a call
/// about faces is refused, as for every request not heard at the lamp (the
/// reviewer on #1713).
pub fn answer_typed(settings: &Settings, call: &ToolCall, request: u64) -> Value {
    let route = route(settings, &call.name);
    // A skill that moves the body is never run for a text: a lamp that
    // moves for a text nobody in the room sent looks wrong (#1673, the first
    // builder, 19 Oct). Light, volume and timers stay the owner's to ask for.
    let skill = match &route {
        Route::Executive(_) if is_skills(settings, &route) => Some(call.name.clone()),
        Route::UseSkill(_) => {
            let skills = skill_tools(settings);
            crate::use_skill::translate(call, crate::use_skill::offered(&skills))
                .ok()
                .map(|skill| skill.name)
        }
        _ => None,
    };
    if let Some(skill) = skill
        && moving_skills(settings).contains(&skill)
    {
        eprintln!(
            "lamp-voice second: text request {request} asked for {skill}, which moves the lamp: refused"
        );
        return json!({"error": MOVES_ONLY_AT_THE_LAMP});
    }
    match route {
        Route::Memory(socket, _) => {
            if crate::remember::is_forget(&call.name) {
                crate::remember_speaker::forget();
            }
            let began = std::time::Instant::now();
            // A text names no day the lamp heard: a `remember`'s `on` goes (#1693).
            let call = crate::remember::on::call_as_heard(call, None);
            let (told, answered) = crate::remember::run_for(&socket, &call, ASK_DEADLINE, None);
            eprintln!(
                "lamp-voice second: text request {request} asked for {}; the room's memory {} in {} ms; the answer was sent",
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
        _ => answer(settings, call, request, false),
    }
}

/// What the model is told when a text asks for a skill that moves the lamp.
pub const MOVES_ONLY_AT_THE_LAMP: &str =
    "That one moves me, so I do it only when someone asks me at the lamp.";

/// The skills' tools whose skill asks for the body, as each skills folder's
/// tools file names them beside its tools (`body`, `lamp_skills::tools_file`):
/// what a text never runs ([`answer_typed`]). An older file names none.
pub fn moving_skills(settings: &Settings) -> BTreeSet<String> {
    named_beside(settings, "body")
}

/// The skills' tools of core skills, as each skills folder's tools file
/// names them beside its tools (`core`, `lamp_skills::tools_file`): never
/// the ones the bound cuts ([`uncut`]). An older file names none.
pub fn core_skills(settings: &Settings) -> BTreeSet<String> {
    named_beside(settings, "core")
}

/// A search's note and walk on the second route, as its skill's file says
/// them (`[search]`'s `note`, `walk` and `looked`, and its choice's
/// `default`) and its skills folder's tools file gives them beside its
/// tools (`searches`, `lamp_skills::tools_file`).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct SearchWalk {
    /// What the model is told, once a search, of a look it left.
    pub note: String,
    /// The places the walk goes on to after the note, in order.
    #[serde(default)]
    pub walk: Vec<String>,
    /// By place, where a walked look's turn says it looked.
    #[serde(default)]
    pub looked: std::collections::BTreeMap<String, String>,
    /// The place a call naming none looks at.
    #[serde(default)]
    pub default: Option<String>,
}

/// The note and walk of the search `tool` starts, from the first skills
/// folder whose tools file gives them ([`SearchWalk`]): none from an older
/// file, or for a tool with no search.
pub fn search_walk(settings: &Settings, tool: &str) -> Option<SearchWalk> {
    tools_files(settings)
        .into_iter()
        .find_map(|file| serde_json::from_value(file["searches"][tool].clone()).ok())
}

/// The names each skills folder's tools file lists under `key` beside its
/// tools.
fn named_beside(settings: &Settings, key: &str) -> BTreeSet<String> {
    let mut named = BTreeSet::new();
    for file in tools_files(settings) {
        if let Some(names) = file[key].as_array() {
            named.extend(
                names
                    .iter()
                    .filter_map(|name| name.as_str().map(str::to_owned)),
            );
        }
    }
    named
}

/// Each skills folder's tools file that can be read, in order.
fn tools_files(settings: &Settings) -> Vec<Value> {
    let mut files = Vec::new();
    for folder in skills_folders(settings) {
        let Ok(file) = std::fs::File::open(folder.join(CATALOG_FILE)) else {
            continue;
        };
        let mut text = String::new();
        if file
            .take(MAX_CATALOG_BYTES + 1)
            .read_to_string(&mut text)
            .is_err()
            || text.len() as u64 > MAX_CATALOG_BYTES
        {
            continue;
        }
        if let Ok(file) = serde_json::from_str::<Value>(&text) {
            files.push(file);
        }
    }
    files
}

/// The context is forwarded only through a verified Skills socket. Translating
/// `use_skill` preserves the original call identity and its original deadline.
#[cfg(test)]
pub(crate) fn run_spoken_call(
    settings: &Settings,
    route: &Route,
    call: &ToolCall,
    request: u64,
    heard: bool,
    context: Option<&lamp_msgs::RequestedAudioContext>,
) -> (&'static str, ToolAnswer, bool) {
    run_spoken_call_inner(settings, route, call, request, heard, context, None)
}

pub(crate) fn run_spoken_call_scoped(
    settings: &Settings,
    route: &Route,
    call: &ToolCall,
    request: u64,
    heard: bool,
    context: Option<&lamp_msgs::RequestedAudioContext>,
    lease: &crate::memory_scope::ScopeLease,
) -> (&'static str, ToolAnswer, bool) {
    run_spoken_call_inner(settings, route, call, request, heard, context, Some(lease))
}

fn run_spoken_call_inner(
    settings: &Settings,
    route: &Route,
    call: &ToolCall,
    request: u64,
    heard: bool,
    context: Option<&lamp_msgs::RequestedAudioContext>,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> (&'static str, ToolAnswer, bool) {
    if !spoken_scope_current(lease, request, context) {
        return ("nobody", ToolAnswer::plain(unavailable()), false);
    }
    let context = context.filter(|context| {
        settings.requested_audio
            && heard
            && context.request == request
            && context.call_id == call.id
            && context.check().is_ok()
    });
    let result = if let Route::UseSkill(settings) = route {
        let skills = skill_tools(settings);
        match crate::use_skill::translate(call, crate::use_skill::offered(&skills)) {
            Ok(skill) => match self::route(settings, &skill.name) {
                found @ Route::Executive(_) if is_skills(settings, &found) => {
                    run_spoken_call_inner(settings, &found, &skill, request, heard, context, lease)
                }
                _ => ("nobody", ToolAnswer::plain(unavailable()), false),
            },
            Err(told) => (crate::use_skill::NAME, ToolAnswer::plain(told), true),
        }
    } else if let Route::Executive(directory) = route
        && is_skills(settings, route)
    {
        let (answer, answered) = ask_with_context(directory, call, ASK_DEADLINE, context);
        ("the executive", answer, answered)
    } else {
        let (whom, (mut told, answered)) = run_call_with_scope(route, call, request, heard, lease);
        // A similarly named field from another service is never authority and
        // must not leak its private path or ticket into the model's conversation.
        take_requested_audio(&mut told, None);
        (whom, ToolAnswer::plain(told), answered)
    };
    // This also fences the direct Skills reply and every use_skill return.
    // A revoked reply must lose the private offer as well as the model text.
    if spoken_scope_current(lease, request, context) {
        result
    } else {
        (result.0, ToolAnswer::plain(unavailable()), false)
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
    run_call_with_scope(route, call, request, heard, None)
}

pub fn run_call_scoped(
    route: &Route,
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: &crate::memory_scope::ScopeLease,
) -> (&'static str, (Value, bool)) {
    run_call_with_scope(route, call, request, heard, Some(lease))
}

fn scope_current(lease: Option<&crate::memory_scope::ScopeLease>, request: u64) -> bool {
    lease.is_none_or(|lease| lease.key().request == request && lease.is_current())
}

fn spoken_scope_current(
    lease: Option<&crate::memory_scope::ScopeLease>,
    request: u64,
    context: Option<&lamp_msgs::RequestedAudioContext>,
) -> bool {
    scope_current(lease, request)
        && lease.zip(context).is_none_or(|(lease, context)| {
            let key = lease.key();
            key.request == context.request
                && key.session == context.provider_session
                && crate::process::boot_text(key.worker) == context.worker_boot
        })
}

fn run_call_with_scope(
    route: &Route,
    call: &ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> (&'static str, (Value, bool)) {
    if !scope_current(lease, request) {
        return ("nobody", (unavailable(), false));
    }
    let deadline = ASK_DEADLINE;
    let result = match route {
        Route::Memory(socket, faces) => {
            if crate::remember::is_forget(&call.name) {
                crate::remember_speaker::forget();
            }
            // A `forget` whose heard line names what to forget is that
            // thing's, of the asker's own scope, never the last kept thing's
            // (#3113), on every switch.
            let said = (heard && call.name == crate::remember::FORGET)
                .then(|| {
                    crate::thread::THREAD
                        .lock()
                        .ok()?
                        .heard_for(request, std::time::Instant::now())
                })
                .flatten();
            let named = crate::remember::named_forget(call, said.as_deref(), lease);
            let call = named.as_ref().unwrap_or(call);
            // With `LAMP_MEMORY_KEEP_HEARD`, a `remember` whose sentence has
            // words the line did not keeps that line instead (#2176).
            // With `LAMP_MEMORY_EVENTS`, a `remember`'s `on` is kept only
            // when the words heard in its line name a day (#1693).
            // A line that is only a yes, with no offer to keep before it,
            // keeps nothing; a yes to the lamp's offer is held against the
            // line the offer was about (#3504, N6).
            if let Some(told) = crate::remember::heard::yes_alone(call, request, heard, lease) {
                return ("the memory", (told, true));
            }
            // The words also give a day the model left out (#3987).
            let words = (heard && call.name == crate::remember::REMEMBER)
                .then(|| crate::remember::heard::words_for(request, lease))
                .flatten();
            let model = call;
            let dated = crate::remember::on::call_as_heard(call, words.as_deref());
            let call = dated.as_ref();
            let as_heard = crate::remember::heard::call_as_heard(call, request, heard, lease);
            let call = as_heard.as_ref().map_or(call, |(call, _)| call);
            // With `LAMP_MEMORY_REPLACES`, a `remember`'s journal line counts
            // its words against those heard in its line (#2510).
            let heard_words = (heard
                && model.name == crate::remember::REMEMBER
                && crate::remember::replaces_on())
            .then(|| crate::remember::heard::words_for(request, lease))
            .flatten();
            let heard_kept = crate::remember::heard::on().then_some(as_heard.is_some());
            let (told, answered) = crate::remember::noted::remember_noted(
                socket,
                model,
                heard_words.as_deref(),
                heard_kept,
                lease,
                || match lease {
                    // A keep in a line admitted as the room's before the
                    // eyes named anyone goes to the keeper recorded at the
                    // line's end: the known face named by then plus 300 ms,
                    // none other seen speaking it (#2807); the room's else.
                    Some(lease) if model.name == crate::remember::REMEMBER => {
                        let started = std::time::Instant::now();
                        match lease.keeper_within(crate::memory_scope::KEEPER_WAIT) {
                            Some(person) => {
                                eprintln!(
                                    "lamp-voice: request {request}: a keep in a line admitted as the room's goes to the known face the eyes named by its end ({} ms)",
                                    started.elapsed().as_millis()
                                );
                                crate::remember::run_scoped_for(
                                    socket,
                                    call,
                                    deadline.saturating_sub(started.elapsed()),
                                    lease,
                                    &person,
                                )
                            }
                            None => crate::remember::run_scoped(
                                socket,
                                call,
                                deadline.saturating_sub(started.elapsed()),
                                lease,
                            ),
                        }
                    }
                    Some(lease) => crate::remember::run_scoped(socket, call, deadline, lease),
                    None => crate::remember::run_of(socket, call, deadline, request),
                },
            );
            let told = crate::remember::heard::answered(
                told,
                model,
                as_heard.as_ref(),
                crate::remember::replaces_on(),
            );
            // "Forget everything about me" said at the lamp by the known
            // person whose line it is: their face, and the name it keeps,
            // go too, as "forget my face" empties their memory (#2850). Only
            // for a request the second route's guard marked as asking for
            // everything (#3564): Live and texts never mark one, so there
            // the face is kept.
            let (told, answered) = match (faces, lease) {
                (Some((who, switches)), Some(lease))
                    if heard && model.name == crate::remember::FORGET_EVERYTHING =>
                {
                    crate::remember::forget_everything_face((told, answered), who, *switches, lease)
                }
                _ => (told, answered),
            };
            ("the memory", (told, answered))
        }
        Route::Executive(directory) => ("the executive", ask(directory, call, deadline)),
        Route::Tasks(place) => (
            "the tasks part",
            crate::agent_tasks::run(place, call, request, heard, crate::agent_tasks::DEADLINE),
        ),
        Route::Schedule(directory) => (
            "the schedule through the executive",
            ask_schedule(directory, call, deadline),
        ),
        // A face is the person's at the lamp: only words they said there
        // may keep or forget one, never a request the lamp wrote or a text
        // from the app (the reviewer on #1713), as for marks.
        Route::Faces(..) if !heard => ("the eyes", (json!({"error": FACES_IN_PERSON}), false)),
        Route::Faces(socket, ..) if crate::photo_faces::is_tool(&call.name) => {
            ("the eyes", crate::photo_faces::run(socket, call))
        }
        // A yes to being named is taken only from the line's own admitted
        // scope, the named person's (#1997): none, revoked or narrowed to
        // the room is nobody's.
        Route::Faces(socket, switches, onboarding) => {
            let person = lease
                .and_then(|lease| lease.scope())
                .and_then(|scope| scope.person());
            // "Forget my face": nothing heard or saved before it follows
            // anyone (`LAMP_MEMORY_FOLLOWS_FACE`), whatever its outcome.
            if call.name == crate::known_faces::FORGET {
                crate::follows_face::forgot();
                if switches.remember_speaker {
                    crate::remember_speaker::forget();
                }
            }
            // Who the eyes saw saying this line: a room's "forget my face"
            // forgets only a face seen saying it (El's P2 on #3018).
            let line = ((call.name == crate::known_faces::FORGET && person.is_none())
                || (call.name == crate::known_faces::REMEMBER && switches.remember_speaker))
                .then(|| crate::speaking_faces::line_seen(request))
                .flatten();
            let told = crate::known_faces::run_scoped(
                socket,
                call,
                *switches,
                person,
                line.as_ref(),
                request,
                lease,
            );
            // "Forget my face" by the known person whose line it is: their
            // memory goes with it (`LAMP_MEMORY_FORGET_ALL`, #2850).
            let told = match lease {
                Some(lease) if call.name == crate::known_faces::FORGET && person.is_some() => {
                    crate::remember::forget_face_memory(told, lease, deadline)
                }
                _ => told,
            };
            // A face saved by this tool: the face cache goes cold at once
            // (#2807, `FaceCache::face_remembered`), without waiting for the
            // refresh to see the eyes' known file change.
            if call.name == crate::known_faces::REMEMBER
                && told.0["ok"] == true
                && let Some(lease) = lease
            {
                lease.face_remembered();
            }
            // A face saved while the walk leads "who you are" ends that step
            // (#3740), through the mailbox `setup_step` writes.
            if call.name == crate::known_faces::REMEMBER
                && told.0["ok"] == true
                && let Some(file) = onboarding
            {
                crate::onboarding::face_saved(file);
            }
            // A face saved in a line of the room's: the caller's own lines
            // before it follow them (`LAMP_MEMORY_FOLLOWS_FACE`, #2176).
            if call.name == crate::known_faces::REMEMBER
                && person.is_none()
                && told.0["ok"] == true
                && let Some(name) = told.0["remembered"].as_str()
            {
                crate::remember::face_saved(request, &crate::known_faces::memory_person(name));
            }
            ("the eyes", told)
        }
        Route::Mark(session) => ("the marks", crate::marked::run(session, call, heard)),
        Route::Onboarding(file) => ("the onboarding", crate::onboarding::run(file, call)),
        Route::ReportsStop(held) => (
            "the reports",
            crate::reports_stop::run(
                &mut crate::reports_stop::Bus::of_the_lamp(),
                heard,
                *held,
                crate::reports_stop::ANSWER_WITHIN,
            ),
        ),
        Route::UseSkill(settings) => {
            let skills = skill_tools(settings);
            match crate::use_skill::translate(call, crate::use_skill::offered(&skills)) {
                // Only ever a skills' socket, in either folder (the reviewer
                // on #1696, #1656).
                Ok(skill) => match crate::tools::route(settings, &skill.name) {
                    found @ Route::Executive(_) if is_skills(settings, &found) => {
                        run_call_with_scope(&found, &skill, request, heard, lease)
                    }
                    _ => ("nobody", (unavailable(), false)),
                },
                // Refused by the voice itself, no skill asked: the journal
                // says so (the reviewer on #1706).
                Err(told) => (crate::use_skill::NAME, (told, true)),
            }
        }
        Route::Decline | Route::Clock | Route::ReportsState | Route::Sleep | Route::Nobody => {
            ("nobody", (unavailable(), false))
        }
    };
    if scope_current(lease, request) {
        result
    } else {
        (result.0, (unavailable(), false))
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
    let (answer, answered) = ask_with_context(directory, call, deadline, None);
    (answer.told, answered)
}

fn ask_with_context(
    directory: &Path,
    call: &ToolCall,
    deadline: Duration,
    context: Option<&lamp_msgs::RequestedAudioContext>,
) -> (ToolAnswer, bool) {
    let mut line = json!({"op":"call","name":call.name,"args":call.args});
    if let Some(context) = context {
        line["requested_audio_context"] = json!(context);
    }
    let told = exchange(&directory.join(CALL_SOCKET), &line, deadline).and_then(|answer| {
        match answer.get("ok").and_then(Value::as_bool)? {
            true => Some(answer.get("result").cloned().unwrap_or_else(|| json!({}))),
            false => Some(json!({"error": answer.get("error").and_then(Value::as_str)?})),
        }
    });
    match told {
        // What the link would refuse to send is never offered to it: an
        // answer that is not sent leaves the service waiting on the call.
        Some(mut told) => {
            let requested_audio = take_requested_audio(&mut told, context);
            if sendable(&told) {
                (
                    ToolAnswer {
                        told,
                        requested_audio,
                        sleep: false,
                    },
                    true,
                )
            } else {
                (ToolAnswer::plain(unavailable()), false)
            }
        }
        None => (ToolAnswer::plain(unavailable()), false),
    }
}

fn take_requested_audio(
    told: &mut Value,
    context: Option<&lamp_msgs::RequestedAudioContext>,
) -> Option<lamp_msgs::RequestedAudioOffer> {
    let private = told.as_object_mut()?.remove("requested_audio")?;
    let expected = context?;
    let offer: lamp_msgs::RequestedAudioOffer = serde_json::from_value(private).ok()?;
    if offer.check().is_err() || &offer.context != expected {
        return None;
    }
    // An offer is not the coordinator's acceptance or an audible handoff.
    // Both voice routes receive the same truthful preparation boundary.
    let note = told["next"].as_str().unwrap_or_default();
    told["next"] = json!(format!(
        "{note} The recording has only been offered for preparation; playback has not started and may still be refused. Confirm only that you will try to play it. Do not say it is playing, has started, or is ready."
    ));
    Some(offer)
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
    // A local socket connects at once or not at all: there is no wait to
    // bound here that the deadlines below do not bound.
    exchange_on(UnixStream::connect(socket).ok()?, line, deadline, most)
}

/// [`exchange`], told apart: `None` for a socket that can't be connected
/// to (none there, or nobody listening), `Some(None)` for one connected to
/// that gave no answer in time.
pub fn reached(socket: &Path, line: &Value, deadline: Duration) -> Option<Option<Value>> {
    let stream = UnixStream::connect(socket).ok()?;
    Some(exchange_on(stream, line, deadline, MAX_ANSWER_BYTES))
}

fn exchange_on(
    mut stream: UnixStream,
    line: &Value,
    deadline: Duration,
    most: u64,
) -> Option<Value> {
    let started = Instant::now();
    let left = || {
        deadline
            .checked_sub(started.elapsed())
            .filter(|left| !left.is_zero())
    };
    stream.set_write_timeout(Some(left()?)).ok()?;
    let mut out = serde_json::to_vec(line).ok()?;
    out.push(b'\n');
    stream.write_all(&out).ok()?;
    read_answer(&stream, most, left)
}

/// One line of JSON read back on `stream`, at most `most` bytes, inside
/// what `left` gives.
fn read_answer(
    stream: &UnixStream,
    most: u64,
    left: impl Fn() -> Option<Duration>,
) -> Option<Value> {
    let mut answer = Vec::new();
    let mut reader = BufReader::new(stream.try_clone().ok()?.take(most));
    loop {
        // The whole answer inside what is left, however it is cut up. On
        // macOS a socket whose peer has already answered and closed refuses a
        // timeout (EINVAL), though its answer waits to be read; its reads
        // can't block then, so it is read as it is (#1569). Linux takes it.
        if let Err(error) = stream.set_read_timeout(Some(left()?))
            && error.raw_os_error() != Some(rustix::io::Errno::INVAL.raw_os_error())
        {
            return None;
        }
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

    #[test]
    fn scoped_dispatch_rejects_wrong_request_and_revoked_scope_before_routing() {
        let owner = crate::memory_scope::RequestScope::new(
            crate::memory_scope::RequestKey {
                worker: lamp_turn_owner::BootId::new([6; 16]).unwrap(),
                session: 1,
                request: 1,
            },
            crate::memory_scope::ScopeSnapshot::room(),
        );
        let lease = owner.lease();
        let call = ToolCall {
            id: "scope".into(),
            name: crate::remember::RECALL.into(),
            args: json!({}),
        };
        let route = Route::Memory(PathBuf::from("/missing-memory.sock"), None);
        assert_eq!(
            run_call_scoped(&route, &call, 2, true, &lease),
            ("nobody", (unavailable(), false))
        );
        owner.revoke();
        assert_eq!(
            run_call_scoped(&route, &call, 1, true, &lease),
            ("nobody", (unavailable(), false))
        );
    }

    /// The reviewer on #1713: a face is kept or forgotten only on words
    /// heard at the lamp. An unheard request, a text from the app or one the
    /// lamp wrote, never reaches the eyes; a heard one does.
    #[test]
    fn a_face_is_kept_or_forgotten_only_on_words_heard_at_the_lamp() {
        let dir = std::env::temp_dir().join(format!("tools-faces-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("eyes.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let route = Route::Faces(socket.clone(), Default::default(), None);
        for name in [crate::known_faces::REMEMBER, crate::known_faces::FORGET] {
            let call = ToolCall {
                id: String::new(),
                name: name.into(),
                args: json!({"name": "Sam"}),
            };
            let (whom, (told, answered)) = run_call(&route, &call, 1, false);
            assert_eq!(whom, "the eyes");
            assert_eq!(told["error"], FACES_IN_PERSON, "{name}");
            assert!(!answered);
            assert!(listener.accept().is_err(), "{name} reached the eyes");
        }
        // Heard, on a room's line: Sam's own face must have been seen saying
        // it (the first builder on #2602, El's P2 on #3018); then it goes to
        // the eyes (here nobody answers, which is told). Sam at the lamp with
        // no line seen, or another face seen saying it, is refused.
        let call = ToolCall {
            id: String::new(),
            name: crate::known_faces::FORGET.into(),
            args: json!({"name": "Sam"}),
        };
        let (_, (told, answered)) = run_call(&route, &call, 2, true);
        assert_eq!(told["error"], crate::known_faces::NOT_THEIRS);
        assert!(answered);
        let known = crate::known_faces::known_file_of(&socket);
        std::fs::write(&known, "Sam\n").unwrap();
        std::fs::write(crate::speaking_faces::known_face_of(&known), "f1\n").unwrap();
        let (_, (told, _)) = run_call(&route, &call, 2, true);
        assert_eq!(told["error"], crate::known_faces::NOT_THEIRS);
        // Request numbers of this test's own: the lines seen are the
        // process's.
        let request = 3_018_001;
        let seen = |request, speaker: &str| {
            crate::speaking_faces::saw_line(crate::speaking_faces::LineSeen {
                request,
                speakers: vec![speaker.to_owned()],
                known: crate::speaking_faces::known_with_face(&known),
                enrollment: None,
            })
        };
        seen(request, "f2");
        let (_, (told, _)) = run_call(&route, &call, request, true);
        assert_eq!(told["error"], crate::known_faces::NOT_THEIRS);
        assert!(
            listener.accept().is_err(),
            "a forget not said by that face reached the eyes"
        );
        seen(request + 1, "f1");
        // Each call reads its own request's line, not the newest: the
        // first line is still another face's (El on #3038).
        let (_, (told, _)) = run_call(&route, &call, request, true);
        assert_eq!(told["error"], crate::known_faces::NOT_THEIRS);
        assert!(
            listener.accept().is_err(),
            "a call read a later request's line"
        );
        let asking = std::thread::spawn(move || run_call(&route, &call, request + 1, true));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let reached = loop {
            if let Ok((stream, _)) = listener.accept() {
                break Some(stream);
            }
            if std::time::Instant::now() > deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(reached.is_some(), "a heard forget never reached the eyes");
        drop(reached);
        let (_, (told, _)) = asking.join().unwrap();
        assert_ne!(told["error"], FACES_IN_PERSON);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A face the eyes saved tells the onboarding walk (#3740), through the
    /// mailbox `setup_step` writes; a refused one tells it nothing.
    #[test]
    fn a_face_saved_tells_the_onboarding_walk() {
        let dir = std::env::temp_dir().join(format!("tools-face-saved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("eyes.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let answer = if line.contains("Sam") {
                    "{\"remembered\":\"Sam\"}\n"
                } else {
                    "{\"error\":\"busy\"}\n"
                };
                stream.write_all(answer.as_bytes()).unwrap();
            }
        });
        let file = dir.join("onboarding.json");
        let route = Route::Faces(socket.clone(), Default::default(), Some(file.clone()));
        let remember = |name: &str| ToolCall {
            id: String::new(),
            name: crate::known_faces::REMEMBER.into(),
            args: json!({"name": name}),
        };
        let mailbox =
            || std::fs::read_to_string(crate::onboarding::mailbox_of(&file)).unwrap_or_default();
        let (_, (told, _)) = run_call(&route, &remember("Alex"), 1, true);
        assert_ne!(told["ok"], true, "{told}");
        assert_eq!(mailbox(), "", "a refused save tells nothing");
        let (_, (told, _)) = run_call(&route, &remember("Sam"), 2, true);
        assert_eq!(told["ok"], true, "{told}");
        let lines: Vec<Value> = mailbox()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            lines,
            [json!({"step": "who_you_are", "outcome": "done", "by": "remember_my_face"})]
        );
        // With no onboarding file, a save writes nothing anywhere.
        let none = Route::Faces(socket.clone(), Default::default(), None);
        let _ = run_call(&none, &remember("Sam"), 3, true);
        assert_eq!(mailbox().lines().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1997 (Third's guard): a yes to being named reaches the eyes only
    /// from the named person's own admitted scope; the room's, another
    /// person's, one narrowed to the room (#2008) or none is refused with
    /// nothing sent. A no is sent from any scope. Neither is run unheard, nor
    /// with the switch off.
    #[test]
    fn a_yes_to_being_named_is_taken_only_from_the_named_persons_own_scope() {
        use crate::known_faces::{DONT_TELL, FaceSwitches, NOT_SURE_WHOSE_YES, TELL};
        use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
        let dir = std::env::temp_dir().join(format!("tools-visits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("eyes.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let heard = sent.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let answer = if line.contains("visits_yes") {
                    "{\"visits\":\"yes\"}\n"
                } else {
                    "{\"visits\":\"no\"}\n"
                };
                heard.lock().unwrap().push(line.trim().to_owned());
                stream.write_all(answer.as_bytes()).unwrap();
            }
        });
        let on = FaceSwitches {
            checked: false,
            visits: true,
            remember_speaker: false,
        };
        let route = Route::Faces(socket.clone(), on, None);
        let call = |name: &str| ToolCall {
            id: String::new(),
            name: name.into(),
            args: json!({"name": "Sam"}),
        };
        let scoped = |route: &Route, name: &str, scope: ScopeSnapshot, narrowed: bool| -> Value {
            let owner = RequestScope::new(
                RequestKey {
                    worker: lamp_turn_owner::BootId::new([7; 16]).unwrap(),
                    session: 1,
                    request: 1,
                },
                scope,
            );
            let lease = owner.lease();
            if narrowed {
                lease.narrow_to_room();
            }
            run_call_scoped(route, &call(name), 1, true, &lease).1.0
        };
        let taken = std::mem::take::<Vec<String>>;
        // Sam's own scope: the yes reaches the eyes.
        let told = scoped(&route, TELL, ScopeSnapshot::of("Sam"), false);
        assert_eq!(told["ok"], true, "{told}");
        assert_eq!(
            taken(&mut sent.lock().unwrap()),
            [r#"{"visits_yes":"Sam"}"#]
        );
        // Anyone else's, or nobody's: refused, nothing sent.
        for (scope, narrowed) in [
            (ScopeSnapshot::room(), false),
            (ScopeSnapshot::of("Alex"), false),
            (ScopeSnapshot::of("Sam"), true),
        ] {
            let told = scoped(&route, TELL, scope.clone(), narrowed);
            assert_eq!(told["error"], NOT_SURE_WHOSE_YES, "{scope:?} {narrowed}");
        }
        assert_eq!(
            run_call(&route, &call(TELL), 1, true).1.0["error"],
            NOT_SURE_WHOSE_YES
        );
        assert!(sent.lock().unwrap().is_empty());
        // A no from the room's scope is sent.
        let told = scoped(&route, DONT_TELL, ScopeSnapshot::room(), false);
        assert_eq!(told["ok"], true, "{told}");
        assert_eq!(taken(&mut sent.lock().unwrap()), [r#"{"visits_no":"Sam"}"#]);
        // Unheard (a text, a line the lamp wrote), or with the switch off:
        // neither reaches the eyes.
        for name in [TELL, DONT_TELL] {
            let unheard = run_call(&route, &call(name), 1, false).1.0;
            assert_eq!(unheard["error"], FACES_IN_PERSON, "{name}");
            let off = Route::Faces(socket.clone(), FaceSwitches::default(), None);
            assert_eq!(
                scoped(&off, name, ScopeSnapshot::of("Sam"), false),
                unavailable()
            );
        }
        assert!(sent.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// #1997: the switch is the worker's `--who-visits`, after the names',
    /// and only then, with the memory (whose face cache gives a line its
    /// person), are the yes and the no offered and routed.
    #[test]
    fn the_yes_and_no_to_being_named_are_offered_only_with_the_switch() {
        let flags = |rest: &[&str]| {
            let mut flags = vec![
                crate::remember::FLAG,
                "/run/lampos-memory/memory.sock",
                crate::known_faces::FLAG,
                "/run/lampos-eyes/who.sock",
            ];
            flags.extend(rest);
            flags.into_iter().map(String::from).collect::<Vec<_>>()
        };
        let names = |settings: &Settings| {
            declared(settings)
                .into_iter()
                .map(|tool| tool.name)
                .collect::<Vec<_>>()
        };
        let visits = [crate::known_faces::TELL, crate::known_faces::DONT_TELL];
        let on = flags(&[
            crate::known_faces::NAMES_FLAG,
            crate::known_faces::VISITS_FLAG,
        ]);
        let (settings, rest) = take(&on).unwrap();
        assert!(settings.who_visits && settings.names_checked && rest.is_empty());
        for name in visits {
            assert!(names(&settings).iter().any(|told| told == name), "{name}");
            assert!(matches!(
                route(&settings, name),
                Route::Faces(_, switches, _) if switches.visits && switches.checked
            ));
        }
        // Without the memory, no line is a known person's: neither is offered,
        // and a call reaches the eyes as switched off.
        let forgetful = Settings {
            memory: None,
            ..settings.clone()
        };
        for name in visits {
            assert!(names(&forgetful).iter().all(|told| told != name), "{name}");
            assert!(matches!(
                route(&forgetful, name),
                Route::Faces(_, switches, _) if !switches.visits
            ));
        }
        let (settings, _) = take(&flags(&[])).unwrap();
        assert!(!settings.who_visits);
        for name in visits {
            assert!(names(&settings).iter().all(|told| told != name), "{name}");
        }
        assert_eq!(
            crate::known_faces::visits_arguments(Some("on")),
            [crate::known_faces::VISITS_FLAG]
        );
        assert!(crate::known_faces::visits_arguments(Some("yes")).is_empty());
        assert!(crate::known_faces::visits_arguments(None).is_empty());
    }

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
            sleep_asked: false,
            decline: true,
            use_skill: false,
            requested_audio: true,
            exec: None,
            skills: None,
            memory: Some("/run/lampos-memory/memory.sock".into()),
            clock: true,
            places: true,
            tasks: Some(crate::agent_tasks::Place {
                directory: "/run/lampos-tasks".into(),
                target: crate::agent_tasks::DEFAULT_TARGET.into(),
                who: true,
                lead_in: true,
                asked: true,
                accounts: false,
                machine_names: false,
                needs: None,
                staff_note: false,
                make_skill: false,
                named_only: false,
                correct: false,
                sign_in: false,
            }),
            schedule_tools: true,
            schedule_speak: true,
            who: Some("/run/lampos-eyes/request.sock".into()),
            // The checked `remember_my_face`, with `replace` (#1057).
            names_checked: true,
            remember_speaker: true,
            who_visits: false,
            photo_faces: false,
            mark: Some("/var/lib/lampos-voice/session".into()),
            onboarding: Some("/var/lib/lampos-voice/onboarding.json".into()),
            reports_stop: true,
        };
        let mut tools = declared(&settings);
        tools.push(crate::attention::name_tool());
        let count = tools.len();
        let (kept, left_out) = admitted(tools);
        assert!(left_out.is_empty(), "{left_out:?}");
        assert_eq!(kept.len(), count);
    }
    use std::os::unix::net::UnixListener;

    /// #1604, the reviewer's P3: an answer whose sender has already closed
    /// is read. macOS refuses that socket a read timeout (EINVAL), though the
    /// answer is there (#1569). Half a line, then gone, is still nothing.
    #[test]
    fn an_answer_whose_sender_already_closed_is_read() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let left = || {
            deadline
                .checked_duration_since(Instant::now())
                .filter(|left| !left.is_zero())
        };
        let (stream, mut sender) = UnixStream::pair().unwrap();
        sender.write_all(b"{\"ok\":true}\n").unwrap();
        drop(sender);
        assert_eq!(read_answer(&stream, 64, left), Some(json!({"ok": true})));
        let (stream, mut sender) = UnixStream::pair().unwrap();
        sender.write_all(b"{\"ok\"").unwrap();
        drop(sender);
        assert_eq!(read_answer(&stream, 64, left), None);
    }

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
        let old = arguments(
            Some("on"),
            Some("/run/actions"),
            None,
            None,
            Some("on"),
            None,
        );
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
        let old = arguments(
            Some("on"),
            Some("/run/actions"),
            None,
            None,
            Some("on"),
            None,
        );
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
        let old: Vec<String> = arguments(Some("on"), None, None, None, Some("on"), None);
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
            None,
            Some("/run/lampos-memory/memory.sock"),
            Some("on"),
            None,
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
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
            make_skill: false,
            named_only: false,
            correct: false,
            sign_in: false,
        };
        assert_eq!(
            settings,
            Settings {
                sleep_asked: false,
                decline: true,
                use_skill: false,
                requested_audio: false,
                exec: Some(dir.clone()),
                skills: None,
                memory: Some("/run/lampos-memory/memory.sock".into()),
                clock: true,
                places: false,
                tasks: Some(place.clone()),
                schedule_tools: true,
                schedule_speak: true,
                who: Some("/run/lampos-eyes/who.sock".into()),
                names_checked: false,
                remember_speaker: false,
                who_visits: false,
                photo_faces: false,
                mark: None,
                onboarding: None,
                reports_stop: false,
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
        // The app's two volume calls are never the model's: run by nobody,
        // with the executive there (the reviewer on #1418). They are the
        // executive's own list, both ways (the reviewer on #1458).
        assert_eq!(lamp_actions::APP_CALLS.as_slice(), APP_CALLS.as_slice());
        for name in APP_CALLS {
            assert!(lamp_actions::is_app_call(name), "{name}");
        }
        for offered in lamp_actions::catalog(true, true) {
            assert!(!lamp_actions::is_app_call(offered.name), "{}", offered.name);
        }
        for name in APP_CALLS {
            assert_eq!(route(&settings, name), Route::Nobody, "{name}");
            assert!(declared(&settings).iter().all(|tool| tool.name != name));
        }
        assert_eq!(route(&settings, "volume_up"), Route::Executive(dir.clone()));
        let eyes = PathBuf::from("/run/lampos-eyes/who.sock");
        for name in [crate::known_faces::REMEMBER, crate::known_faces::FORGET] {
            assert_eq!(
                route(&settings, name),
                Route::Faces(eyes.clone(), Default::default(), None)
            );
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
    #[test]
    fn reports_and_requested_audio_flags_are_independent_and_leave_the_session_untouched() {
        let untouched = ["--session".to_owned(), "7".to_owned()];
        for use_skill in [false, true] {
            for reports_stop in [false, true] {
                for requested_audio in [false, true] {
                    let mut flags = crate::use_skill::arguments(use_skill.then_some("on"));
                    flags.extend(crate::reports_stop::arguments(reports_stop.then_some("on")));
                    if requested_audio {
                        flags.push(crate::requested_audio::FLAG.to_owned());
                    }
                    flags.extend(untouched.clone());
                    let (settings, rest) = take(&flags).unwrap();
                    assert_eq!(settings.use_skill, use_skill, "{flags:?}");
                    assert_eq!(settings.reports_stop, reports_stop, "{flags:?}");
                    assert_eq!(settings.requested_audio, requested_audio, "{flags:?}");
                    assert_eq!(rest, untouched, "{flags:?}");
                }
            }
        }
    }

    #[test]
    fn requested_audio_preserves_reports_stop_route_and_its_spoken_only_guard() {
        let settings = Settings {
            reports_stop: true,
            requested_audio: true,
            ..Default::default()
        };
        let call = call(crate::reports_stop::NAME);
        let context = audio_context();
        let route = route(&settings, &call.name);
        assert_eq!(route, Route::ReportsStop(false));
        // Even a supplied recording context cannot make a written request
        // count as words the person said or reroute this system function.
        let (whom, answer, answered) = run_spoken_call(
            &settings,
            &route,
            &call,
            context.request,
            false,
            Some(&context),
        );
        assert_eq!(whom, "the reports");
        assert!(!answered);
        assert!(answer.requested_audio.is_none());
        assert_eq!(
            answer.told,
            json!({"error": "Only a person can stop the reports."})
        );
        assert_eq!(
            answer_typed(&settings, &call, crate::messages::REQUESTS_FROM + 1),
            answer.told
        );
    }

    fn audio_context() -> lamp_msgs::RequestedAudioContext {
        lamp_msgs::RequestedAudioContext {
            worker_boot: "a".repeat(32),
            provider_session: 3,
            request: 7,
            privacy_generation: 5,
            call_id: "c1".into(),
            issued_us: 1_000_000,
        }
    }

    fn audio_offer(context: lamp_msgs::RequestedAudioContext) -> lamp_msgs::RequestedAudioOffer {
        lamp_msgs::RequestedAudioOffer {
            skills_boot: "skills-test".into(),
            definition_generation: 2,
            offer_id: 1,
            skill: "relaxing-music".into(),
            asset: "/local/relaxing.wav".into(),
            sha256: "a".repeat(64),
            frames: 24_000,
            issued_us: context.issued_us,
            start_before_us: context.issued_us + lamp_msgs::REQUESTED_AUDIO_START_US,
            finish_before_us: context.issued_us + lamp_msgs::REQUESTED_AUDIO_START_US + 1_000_000,
            context,
        }
    }

    fn audio_socket(directory: &Path, offer: Value) -> std::thread::JoinHandle<Value> {
        let listener = UnixListener::bind(directory.join(CALL_SOCKET)).unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            writeln!(
                stream,
                "{}",
                json!({"ok":true,"result":{
                    "next":"Say one short confirmation.","requested_audio":offer,
                }})
            )
            .unwrap();
            serde_json::from_str(&line).unwrap()
        })
    }

    fn audio_scope(
        snapshot: crate::memory_scope::ScopeSnapshot,
    ) -> (
        std::sync::Arc<crate::memory_scope::RequestScope>,
        lamp_msgs::RequestedAudioContext,
    ) {
        let worker = lamp_turn_owner::BootId::new([0xaa; 16]).unwrap();
        let mut context = audio_context();
        context.worker_boot = crate::process::boot_text(worker);
        let owner = crate::memory_scope::RequestScope::new(
            crate::memory_scope::RequestKey {
                worker,
                session: context.provider_session,
                request: context.request,
            },
            snapshot,
        );
        (std::sync::Arc::new(owner), context)
    }

    fn scoped_audio_setup(wrapped: bool) -> (PathBuf, Settings, ToolCall) {
        let dir = schedule_directory("audio-scope");
        std::fs::write(
            dir.join(CATALOG_FILE),
            r#"{"tools":[{"name":"play_relaxing_music","description":"A requested recording."}]}"#,
        )
        .unwrap();
        let settings = Settings {
            skills: Some(dir.clone()),
            requested_audio: true,
            use_skill: wrapped,
            ..Default::default()
        };
        let call = ToolCall {
            id: "c1".into(),
            name: if wrapped {
                crate::use_skill::NAME
            } else {
                "play_relaxing_music"
            }
            .into(),
            args: if wrapped {
                json!({"skill":"play_relaxing_music"})
            } else {
                json!({})
            },
        };
        (dir, settings, call)
    }

    fn scoped_reply(
        socket: &Path,
        owner: std::sync::Arc<crate::memory_scope::RequestScope>,
        revoke: bool,
        reply: Value,
    ) -> std::thread::JoinHandle<Value> {
        let listener = UnixListener::bind(socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        std::thread::spawn(move || {
            use rustix::event::{PollFd, PollFlags, Timespec, poll};
            let mut waiting = [PollFd::new(&listener, PollFlags::IN)];
            assert_eq!(
                poll(
                    &mut waiting,
                    Some(&Timespec {
                        tv_sec: 2,
                        tv_nsec: 0,
                    }),
                )
                .unwrap(),
                1,
                "the valid scoped call never reached its socket"
            );
            let (mut stream, _) = listener.accept().unwrap();
            // Accepted sockets inherit nonblocking mode on macOS. Wait for
            // the complete request under the same bounded fixture deadline.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let asked = serde_json::from_str(&line).unwrap();
            // The RPC was dispatched already. Only its reply/offer is
            // withheld; this does not claim to undo a completed mutation.
            if revoke {
                owner.revoke();
            }
            let written = writeln!(stream, "{reply}");
            if !revoke {
                written.unwrap();
            }
            asked
        })
    }

    #[test]
    fn scoped_audio_direct_and_wrapped_replies_require_a_live_lease() {
        for wrapped in [false, true] {
            for revoke in [false, true] {
                let (dir, settings, call) = scoped_audio_setup(wrapped);
                let (owner, context) = audio_scope(crate::memory_scope::ScopeSnapshot::room());
                let lease = owner.lease();
                let offer = audio_offer(context.clone());
                let served = scoped_reply(
                    &dir.join(CALL_SOCKET),
                    owner.clone(),
                    revoke,
                    json!({"ok":true,"result":{
                        "next":"Say one short confirmation.","requested_audio":&offer,
                    }}),
                );
                let answer = answer_spoken_scoped(&settings, &call, &context, &lease);
                if revoke {
                    assert_eq!(answer.told, unavailable());
                    assert!(answer.requested_audio.is_none());
                    assert!(!lease.is_current());
                } else {
                    let note = answer.told["next"].as_str().unwrap();
                    assert!(note.starts_with("Say one short confirmation."));
                    assert!(note.contains("playback has not started and may still be refused"));
                    assert!(note.contains("Confirm only that you will try to play it."));
                    assert!(note.contains("Do not say it is playing, has started, or is ready."));
                    assert_eq!(answer.told.as_object().unwrap().len(), 1);
                    assert!(!note.contains("relaxing.wav"));
                    assert_eq!(answer.requested_audio, Some(offer));
                    assert!(lease.is_current());
                }
                assert_eq!(
                    served.join().unwrap(),
                    json!({"op":"call","name":"play_relaxing_music","args":{},
                        "requested_audio_context":context}),
                    "translation must retain the original call id, request and time bound"
                );
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }

    #[test]
    fn scoped_audio_rejects_revocation_and_mixed_request_keys_before_dispatch() {
        for wrapped in [false, true] {
            let (dir, settings, call) = scoped_audio_setup(wrapped);
            let listener = UnixListener::bind(dir.join(CALL_SOCKET)).unwrap();
            listener.set_nonblocking(true).unwrap();
            for change in ["revoked", "request", "session", "worker"] {
                let (owner, mut context) = audio_scope(crate::memory_scope::ScopeSnapshot::room());
                let lease = owner.lease();
                match change {
                    "revoked" => owner.revoke(),
                    "request" => context.request += 1,
                    "session" => context.provider_session += 1,
                    "worker" => context.worker_boot = "b".repeat(32),
                    _ => unreachable!(),
                }
                let (whom, answer, answered) = run_spoken_call_scoped(
                    &settings,
                    &route(&settings, &call.name),
                    &call,
                    lease.key().request,
                    true,
                    Some(&context),
                    &lease,
                );
                assert_eq!(whom, "nobody", "{change}");
                assert!(!answered, "{change}");
                assert_eq!(answer.told, unavailable(), "{change}");
                assert!(answer.requested_audio.is_none(), "{change}");
                assert_eq!(
                    listener.accept().unwrap_err().kind(),
                    std::io::ErrorKind::WouldBlock,
                    "{change} reached Skills"
                );
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn scoped_memory_with_audio_context_keeps_admitted_identity_and_suppresses_late_reply() {
        for revoke in [false, true] {
            let dir = schedule_directory("memory-audio-scope");
            let file = dir.join("face");
            let cache = crate::memory_scope::FaceCache::default();
            std::fs::write(&file, "speaker-first").unwrap();
            let token = cache.begin_refresh();
            assert!(cache.replace(token, crate::known_faces::FaceObservation::read(&file)));
            let (owner, context) = audio_scope(cache.snapshot(Instant::now()));
            let lease = owner.lease();
            let admitted = crate::remember::person_id("speaker-first");
            assert_eq!(lease.scope().unwrap().person(), Some(admitted.as_str()));
            std::fs::write(&file, "speaker-next").unwrap();
            let token = cache.begin_refresh();
            assert!(cache.replace(token, crate::known_faces::FaceObservation::read(&file)));
            assert_eq!(
                cache.snapshot(Instant::now()).person(),
                Some(crate::remember::person_id("speaker-next").as_str())
            );
            let socket = dir.join("memory.sock");
            let served = scoped_reply(&socket, owner.clone(), revoke, json!({"ok":true,"id":1}));
            let settings = Settings {
                memory: Some(socket),
                requested_audio: true,
                ..Default::default()
            };
            let call = ToolCall {
                id: context.call_id.clone(),
                name: crate::remember::REMEMBER.into(),
                args: json!({"text":"A fixture preference."}),
            };
            let (whom, answer, answered) = run_spoken_call_scoped(
                &settings,
                &route(&settings, &call.name),
                &call,
                context.request,
                true,
                Some(&context),
                &lease,
            );
            assert_eq!(whom, "the memory");
            assert_eq!(answered, !revoke);
            assert_eq!(
                answer.told,
                if revoke {
                    unavailable()
                } else {
                    json!({"remembered":true,"kept_for":crate::remember::KEPT_FOR_THEM})
                }
            );
            assert!(answer.requested_audio.is_none());
            let asked = served.join().unwrap();
            assert_eq!(asked["person"], admitted);
            assert!(asked.get("requested_audio_context").is_none());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn requested_audio_uses_the_skills_socket_directly_and_through_use_skill() {
        for wrapped in [false, true] {
            let dir = schedule_directory(if wrapped {
                "audio-wrapped"
            } else {
                "audio-direct"
            });
            std::fs::write(
                dir.join(CATALOG_FILE),
                json!({"tools":[
                    {"name":"play_relaxing_music", "description":"An explicitly requested recording."},
                    {"name":"dance", "description":"Dance."}
                ],"body":["dance"]})
                .to_string(),
            )
            .unwrap();
            let settings = Settings {
                skills: Some(dir.clone()),
                requested_audio: true,
                use_skill: wrapped,
                ..Default::default()
            };
            let call = ToolCall {
                id: "c1".into(),
                name: if wrapped {
                    crate::use_skill::NAME
                } else {
                    "play_relaxing_music"
                }
                .into(),
                args: if wrapped {
                    json!({"skill":"play_relaxing_music"})
                } else {
                    json!({})
                },
            };
            let context = audio_context();
            let offer = audio_offer(context.clone());
            let server = audio_socket(&dir, json!(&offer));
            let answer = answer_spoken(&settings, &call, &context);
            assert_eq!(answer.requested_audio, Some(offer));
            let note = answer.told["next"].as_str().unwrap();
            assert!(note.starts_with("Say one short confirmation."));
            assert!(note.contains("playback has not started and may still be refused"));
            assert!(note.contains("Confirm only that you will try to play it."));
            assert!(note.contains("Do not say it is playing, has started, or is ready."));
            assert_eq!(answer.told.as_object().unwrap().len(), 1);
            assert!(!note.contains("relaxing.wav"));
            assert_eq!(
                server.join().unwrap(),
                json!({"op":"call","name":"play_relaxing_music",
                "args":{},"requested_audio_context":context})
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn requested_audio_is_private_and_unavailable_to_legacy_off_and_non_skills_routes() {
        for mode in [
            "legacy-heard",
            "legacy-written",
            "typed",
            "typed-wrapped",
            "off",
            "executive",
            "wrong-call",
        ] {
            let dir = schedule_directory(&format!("audio-{mode}"));
            std::fs::write(
                dir.join(CATALOG_FILE),
                json!({"tools":[
                    {"name":"play_relaxing_music","description":"A recording."},
                    {"name":"dance","description":"Dance."}
                ],"body":["dance"]})
                .to_string(),
            )
            .unwrap();
            let mut settings = Settings {
                requested_audio: mode != "off",
                use_skill: mode == "typed-wrapped",
                ..Default::default()
            };
            if mode == "executive" {
                settings.exec = Some(dir.clone());
            } else {
                settings.skills = Some(dir.clone());
            }
            let mut context = audio_context();
            if matches!(mode, "typed" | "typed-wrapped") {
                context.request = crate::messages::REQUESTS_FROM + 1;
            }
            let server = audio_socket(&dir, json!(audio_offer(context.clone())));
            if mode == "wrong-call" {
                context.call_id = "different-call".into();
            }
            let call = if mode == "typed-wrapped" {
                ToolCall {
                    id: "c1".into(),
                    name: crate::use_skill::NAME.into(),
                    args: json!({"skill":"play_relaxing_music"}),
                }
            } else {
                ToolCall {
                    id: "c1".into(),
                    name: "play_relaxing_music".into(),
                    args: json!({}),
                }
            };
            let answer = match mode {
                "legacy-heard" | "legacy-written" => {
                    ToolAnswer::plain(answer(&settings, &call, 7, mode == "legacy-heard"))
                }
                "typed" | "typed-wrapped" => {
                    ToolAnswer::plain(answer_typed(&settings, &call, context.request))
                }
                _ => answer_spoken(&settings, &call, &context),
            };
            assert!(answer.requested_audio.is_none(), "{mode}");
            assert_eq!(answer.told, json!({"next":"Say one short confirmation."}));
            assert!(
                server
                    .join()
                    .unwrap()
                    .get("requested_audio_context")
                    .is_none(),
                "{mode}"
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn requested_audio_rejects_changed_binding_and_unknown_offer_fields() {
        let context = audio_context();
        let offer = audio_offer(context.clone());
        for field in [
            "worker_boot",
            "provider_session",
            "request",
            "privacy_generation",
            "call_id",
            "issued_us",
        ] {
            let mut changed = json!(&offer);
            changed["context"][field] = match field {
                "worker_boot" | "call_id" => json!("another"),
                _ => json!(99),
            };
            let mut told = json!({"next":"Confirm.","requested_audio":changed});
            assert!(
                take_requested_audio(&mut told, Some(&context)).is_none(),
                "{field}"
            );
            assert_eq!(told, json!({"next":"Confirm."}));
        }
        let mut private = json!(offer);
        private["future_permission"] = json!(true);
        let mut told = json!({"requested_audio":private});
        assert!(take_requested_audio(&mut told, Some(&context)).is_none());
        assert_eq!(told, json!({}));
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
                Route::Memory("/run/lampos-memory/memory.sock".into(), None)
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
        assert!(arguments(None, None, None, None, None, None).is_empty());
        assert!(
            arguments(
                Some("yes"),
                Some("relative/dir"),
                Some("relative/skills"),
                Some("memory.sock"),
                Some("yes"),
                Some("on")
            )
            .is_empty()
        );
        let all = arguments(
            Some("on"),
            Some("/run/lampos-actions"),
            Some("/run/lampos-skills"),
            Some("/run/lampos-memory/memory.sock"),
            None,
            Some("on"),
        );
        let memory = crate::remember::FLAG;
        assert_eq!(
            all,
            [
                FLAG,
                ACTIONS_FLAG,
                "/run/lampos-actions",
                SKILLS_FLAG,
                "/run/lampos-skills",
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
                skills: Some("/run/lampos-skills".into()),
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
        assert!(settings.exec.is_none() && settings.skills.is_some() && settings.memory.is_some());
        assert_eq!(rest, ["rest"]);
        let (settings, rest) = take(&flags[5..]).unwrap();
        assert!(settings.skills.is_none() && settings.memory.is_some());
        assert_eq!(rest, ["rest"]);
        let (settings, rest) = take(&flags[7..]).unwrap();
        assert_eq!(settings, Settings::default());
        assert_eq!(rest, ["rest"]);
        assert!(take(&[ACTIONS_FLAG.to_owned()]).is_err());
        assert!(take(&[ACTIONS_FLAG.to_owned(), "relative".to_owned()]).is_err());
        assert!(take(&[SKILLS_FLAG.to_owned()]).is_err());
        assert!(take(&[SKILLS_FLAG.to_owned(), "relative".to_owned()]).is_err());
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
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
            make_skill: false,
            named_only: false,
            correct: false,
            sign_in: false,
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
        assert_eq!(
            arguments(None, None, None, None, Some("on"), None),
            [CLOCK_FLAG]
        );
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
        assert!(!the_time(false).silent);
        let told = time_now();
        assert!(told["now"].is_string() || told["error"].is_string());
    }

    /// #1547, `LAMP_TIME_PLACES`: only with the clock, right after its
    /// flag. With it, the function takes a place and a place this lamp has
    /// no zone for is said to be unknown, never given the lamp's own time;
    /// without it, a place in the call is the lamp's own time as before.
    #[test]
    fn the_time_takes_a_place_only_with_its_switch() {
        assert_eq!(
            arguments(None, None, None, None, Some("on"), Some("on")),
            [CLOCK_FLAG, PLACES_FLAG]
        );
        assert!(arguments(None, None, None, None, None, Some("on")).is_empty());
        assert_eq!(
            arguments(None, None, None, None, Some("on"), Some("yes")),
            [CLOCK_FLAG]
        );
        let flags = [
            CLOCK_FLAG.to_owned(),
            PLACES_FLAG.to_owned(),
            "rest".to_owned(),
        ];
        let (settings, rest) = take(&flags).unwrap();
        assert!(settings.clock && settings.places);
        assert_eq!(rest, ["rest"]);
        // Out of its place it is not taken.
        let (settings, rest) = take(&flags[1..]).unwrap();
        assert!(!settings.clock && !settings.places);
        assert_eq!(rest[0], PLACES_FLAG);
        let with = |places: bool| Settings {
            clock: true,
            places,
            ..Settings::default()
        };
        let declared_place = |settings: &Settings| {
            declared(settings)
                .into_iter()
                .find(|tool| tool.name == THE_TIME)
                .unwrap()
                .parameters
                .and_then(|parameters| parameters["properties"].get("place").cloned())
        };
        assert!(declared_place(&with(true)).is_some());
        assert!(declared_place(&with(false)).is_none());
        let nowhere = json!({"place": "Middle_Earth/Rivendell"});
        assert_eq!(
            time_now_for(true, &nowhere),
            json!({"error": crate::local_time::PLACE_UNKNOWN})
        );
        assert_eq!(
            time_now_for(true, &json!({"place": "../../etc/passwd"})),
            json!({"error": crate::local_time::PLACE_UNKNOWN})
        );
        let unknown = json!({"error": crate::local_time::PLACE_UNKNOWN});
        assert_ne!(time_now_for(false, &nowhere), unknown);
        assert_ne!(time_now_for(true, &json!({})), unknown);
        // A place that is not words is not the lamp's own time either.
        for odd in [
            json!({"city": "London"}),
            json!(51),
            json!(["Europe/London"]),
        ] {
            assert_eq!(time_now_for(true, &json!({"place": odd})), unknown, "{odd}");
        }
        assert_ne!(time_now_for(true, &json!({"place": null})), unknown);
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

    /// The executive's seventeen and three skills, as on a product lamp
    /// with Desk tutor installed (#1668).
    fn many_skills(name: &str) -> (PathBuf, Settings) {
        let dir = directory(name);
        let executive: Vec<Value> = (0..25)
            .map(|n| json!({"name": format!("light_{n}"), "description": "A light."}))
            .collect();
        std::fs::write(
            dir.join(CATALOG_FILE),
            json!({"tools": executive}).to_string(),
        )
        .unwrap();
        std::fs::create_dir(dir.join(SKILLS_DIR)).unwrap();
        std::fs::write(
            dir.join(SKILLS_DIR).join(CATALOG_FILE),
            json!({"tools":[
                {"name":"dance","description":"When someone asks the lamp to dance.",
                 "parameters":{"type":"object","properties":{
                    "style":{"type":"string","enum":["calm","upbeat","jazz"],"description":"The mood."},
                    "long":{"type":"boolean","description":"A long one."}}}},
                {"name":"check_my_work","description":"Check their work."},
                {"name":"show_verdict","description":"After check_my_work.",
                 "parameters":{"type":"object","properties":{
                    "verdict":{"type":"string","enum":["right","wrong"],"description":"Always one."}}}}]})
            .to_string(),
        )
        .unwrap();
        // Thirty before the skills, as on a product lamp: the voice's own
        // five here, and the executive's twenty-five standing in for the rest.
        let settings = Settings {
            decline: true,
            exec: Some(dir.clone()),
            clock: true,
            memory: Some("/run/lampos-memory/memory.sock".into()),
            ..Default::default()
        };
        (dir, settings)
    }

    /// The first builder on #2644: past 32, `use_skill` is a core name and
    /// never the one cut, so `dance` stays reachable through it; with
    /// `LAMP_REPORTS=on` the cut takes `reports_state` before any of the
    /// lamp's own actions, and `reports_stop` stays.
    #[test]
    fn past_the_bound_use_skill_and_the_lamps_actions_stay() {
        let (dir, mut settings) = many_skills("never-cut");
        let skills = dir.join(SKILLS_DIR).join(CATALOG_FILE);
        let mut catalog: Value =
            serde_json::from_str(&std::fs::read_to_string(&skills).unwrap()).unwrap();
        catalog["core"] = json!(["dance"]);
        std::fs::write(&skills, catalog.to_string()).unwrap();
        let names =
            |tools: Vec<Tool>| -> Vec<String> { tools.into_iter().map(|tool| tool.name).collect() };
        // Two more of the lamp's own: 32 before `use_skill`.
        let lights: Vec<Value> = (0..27)
            .map(|n| json!({"name": format!("light_{n}"), "description": "A light."}))
            .collect();
        std::fs::write(dir.join(CATALOG_FILE), json!({"tools": lights}).to_string()).unwrap();
        settings.use_skill = true;
        let told = names(declared(&settings));
        assert_eq!(told.len(), lamp_gemini::MAX_TOOLS);
        assert!(
            told.iter().any(|name| name == crate::use_skill::NAME),
            "{told:?}"
        );
        assert!(!told.iter().any(|name| name == "light_26"), "{told:?}");
        assert_eq!(
            route(&settings, crate::use_skill::NAME),
            Route::UseSkill(Box::new(settings.clone()))
        );
        // Reports on: the voice's seven and twenty-five actions are 32, and
        // three skills more; the cut is `reports_state`, then the two that
        // are not core, and every action and `dance` stay.
        let lights: Vec<Value> = (0..25)
            .map(|n| json!({"name": format!("light_{n}"), "description": "A light."}))
            .collect();
        std::fs::write(dir.join(CATALOG_FILE), json!({"tools": lights}).to_string()).unwrap();
        settings.use_skill = false;
        settings.reports_stop = true;
        let told = names(declared(&settings));
        assert_eq!(told.len(), lamp_gemini::MAX_TOOLS);
        for kept in ["light_24", "dance", crate::reports_stop::NAME] {
            assert!(told.iter().any(|name| name == kept), "{kept}: {told:?}");
        }
        for cut in [
            crate::reports_stop::STATE_NAME,
            "check_my_work",
            "show_verdict",
        ] {
            assert!(!told.iter().any(|name| name == cut), "{cut}: {told:?}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #1668: past the 32, one skill was dropped without a word. With
    /// `use_skill` a session is given that one function for every skill,
    /// and none of the skills' own; its instruction lists them.
    #[test]
    fn with_use_skill_every_skill_is_reached_through_one_function() {
        let (dir, mut settings) = many_skills("use-skill");
        let core = core_tools(&settings).len();
        assert_eq!(core, 30);
        // Without it, Desk tutor's verdict is cut, silently but for the
        // setup line (the reviewer on #1696).
        let off = declared(&settings);
        assert_eq!(off.len(), 32);
        assert!(off.iter().any(|tool| tool.name == "check_my_work"));
        assert!(!off.iter().any(|tool| tool.name == "show_verdict"));
        let said = crate::session_tools::setup_line(&off, &settings);
        assert!(
            said.ends_with(
                "; skills not given: show_verdict (past 32: LAMP_USE_SKILL=on reaches them)"
            ),
            "{said}"
        );
        settings.use_skill = true;
        let on = declared(&settings);
        assert_eq!(on.len(), core + 1);
        let names: Vec<&str> = on.iter().map(|tool| tool.name.as_str()).collect();
        assert!(names.contains(&crate::use_skill::NAME), "{names:?}");
        assert!(!names.contains(&"dance") && !names.contains(&"check_my_work"));
        assert_eq!(
            skill_tools(&settings)
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            ["dance", "check_my_work", "show_verdict"]
        );
        let said = crate::session_tools::setup_line(&on, &settings);
        assert!(said.ends_with("; 3 skills through use_skill"), "{said}");
        let (kept, left_out) = admitted(on);
        assert!(left_out.is_empty(), "{left_out:?}");
        assert_eq!(kept.len(), core + 1);
        assert_eq!(
            route(&settings, crate::use_skill::NAME),
            Route::UseSkill(Box::new(settings.clone()))
        );
        // Without the switch, use_skill is nobody's, and a catalog's
        // function of that name is never told: the name is the voice's,
        // switch or not (the reviewer on #1696).
        settings.use_skill = false;
        assert_eq!(
            route(&settings, crate::use_skill::NAME),
            Route::Executive(dir.clone())
        );
        let named: Vec<Value> = (0..25)
            .map(|n| json!({"name": format!("light_{n}"), "description": "A light."}))
            .chain([json!({"name": crate::use_skill::NAME, "description": "Not this."})])
            .collect();
        std::fs::write(dir.join(CATALOG_FILE), json!({"tools": named}).to_string()).unwrap();
        assert!(
            !declared(&settings)
                .iter()
                .any(|tool| tool.name == crate::use_skill::NAME)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The reviewer on #1696: the worker's flag sets the switch, after the
    /// rest of the tools' flags, and leaves what follows.
    #[test]
    fn the_use_skill_flag_sets_the_switch() {
        let plain = ["rest".to_owned()];
        let (settings, rest) = take(&plain).unwrap();
        assert!(!settings.use_skill);
        assert_eq!(rest, ["rest"]);
        let mut flags = arguments(Some("on"), None, None, None, None, None);
        flags.extend(crate::use_skill::arguments(Some("on")));
        flags.push("rest".into());
        let (settings, rest) = take(&flags).unwrap();
        assert!(settings.decline && settings.use_skill);
        assert_eq!(rest, ["rest"]);
    }

    /// #1683: `reports_stop` is the voice's own, declared and routed only
    /// with its switch, taken after `use_skill`'s flag, and never a catalog's.
    #[test]
    fn stopping_reports_is_declared_and_routed_only_with_its_switch() {
        let plain = Settings::default();
        assert!(
            declared(&plain)
                .iter()
                .all(|tool| tool.name != crate::reports_stop::NAME
                    && tool.name != crate::reports_stop::STATE_NAME)
        );
        assert_eq!(route(&plain, crate::reports_stop::NAME), Route::Nobody);
        assert_eq!(
            route(&plain, crate::reports_stop::STATE_NAME),
            Route::Nobody
        );
        let mut flags = crate::use_skill::arguments(Some("on"));
        flags.extend(crate::reports_stop::arguments(Some("on")));
        flags.push("rest".into());
        let (on, rest) = take(&flags).unwrap();
        assert!(on.use_skill && on.reports_stop);
        assert_eq!(rest, ["rest"]);
        let names: Vec<String> = declared(&on).into_iter().map(|tool| tool.name).collect();
        assert!(
            names.contains(&crate::reports_stop::NAME.to_owned()),
            "{names:?}"
        );
        assert!(
            names.contains(&crate::reports_stop::STATE_NAME.to_owned()),
            "{names:?}"
        );
        assert_eq!(
            route(&on, crate::reports_stop::NAME),
            Route::ReportsStop(false)
        );
        assert_eq!(
            route(&on, crate::reports_stop::STATE_NAME),
            Route::ReportsState
        );
        assert!(voices_own(crate::reports_stop::NAME));
        assert!(voices_own(crate::reports_stop::STATE_NAME));
        // Off, the flag is never given.
        assert!(crate::reports_stop::arguments(None).is_empty());
    }

    /// 25 Oct: the second route's chat request takes the executive's
    /// functions up to its own bound, so with more of the voice's own (here
    /// reports' two and the faces') every one of the lamp's actions is still
    /// offered there; a Live session takes them up to its 32, as before.
    #[test]
    fn the_chat_route_keeps_every_action_past_lives_bound() {
        let (_dir, mut settings) = many_skills("chat-own-bound");
        settings.reports_stop = true;
        settings.who = Some("/run/lampos-eyes/who.sock".into());
        let names =
            |tools: Vec<Tool>| -> Vec<String> { tools.into_iter().map(|tool| tool.name).collect() };
        let live = names(core_tools(&settings));
        assert_eq!(live.len(), lamp_gemini::MAX_TOOLS, "{live:?}");
        assert!(!live.iter().any(|name| name == "light_24"), "{live:?}");
        let chat = names(crate::second_provider::worker::chat_tools(
            &settings,
            &[],
            false,
        ));
        assert!(
            (0..25).all(|n| chat.contains(&format!("light_{n}"))),
            "{chat:?}"
        );
        assert!(chat.len() <= crate::second_provider::MAX_TOOLS);
    }

    /// The reviewer's P3 on #1906: `LAMP_REPORTS=on` declares two more,
    /// `reports_stop` and `reports_state`, before the executive's and the
    /// skills'; past 32 the cut is `reports_state`, then the last skills'
    /// functions, and the setup line names the skills'.
    #[test]
    fn reports_add_two_functions_and_the_last_skills_are_cut_past_32() {
        let (dir, mut settings) = many_skills("reports-count");
        // Off: the voice's own five and the executive's twenty-five, and two
        // of the three skills' functions; show_verdict is past 32.
        assert_eq!(core_tools(&settings).len(), 30);
        let off: Vec<String> = declared(&settings)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(off.len(), 32);
        assert_eq!(&off[30..], ["dance", "check_my_work"]);
        // On: two more of the voice's own; the cut takes `reports_state`
        // first, then the last skill's that fitted (the first builder on
        // #2644); `reports_stop` and the executive's all stay.
        settings.reports_stop = true;
        assert_eq!(core_tools(&settings).len(), 32);
        let on = declared(&settings);
        assert_eq!(on.len(), 32);
        let names: Vec<&str> = on.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names[5], crate::reports_stop::NAME);
        assert!(!names.contains(&crate::reports_stop::STATE_NAME));
        assert!((0..25).all(|n| names.contains(&format!("light_{n}").as_str())));
        let cut: Vec<&str> = off
            .iter()
            .map(String::as_str)
            .filter(|name| !names.contains(name))
            .collect();
        assert_eq!(cut, ["check_my_work"]);
        let said = crate::session_tools::setup_line(&on, &settings);
        assert!(
            said.ends_with(
                "; skills not given: check_my_work, show_verdict (past 32: LAMP_USE_SKILL=on reaches them)"
            ),
            "{said}"
        );
        let (kept, left_out) = admitted(on);
        assert!(left_out.is_empty(), "{left_out:?}");
        assert_eq!(kept.len(), 32);
        // With use_skill as well, it is the 33rd, and a core name: the cut
        // is `reports_state` again, and `use_skill` stays.
        settings.use_skill = true;
        let (kept, left_out) = admitted(declared(&settings));
        assert_eq!(kept.len(), 32);
        assert!(left_out.is_empty(), "{left_out:?}");
        assert!(kept.iter().any(|tool| tool.name == crate::use_skill::NAME));
        assert!(
            kept.iter()
                .all(|tool| tool.name != crate::reports_stop::STATE_NAME)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #1880 on the second route (`LAMP_VOICE_ROUTE=second`), whose runner
    /// is [`answer`]: `reports_state` is answered there at once, from the
    /// bus's state, as on the Live route, and only for its own call; off,
    /// it is nobody's. Asked by a text, it is answered the same.
    #[test]
    fn reports_state_is_answered_on_the_second_route_only_when_called() {
        let call = ToolCall {
            id: "c".into(),
            name: crate::reports_stop::STATE_NAME.into(),
            args: json!({}),
        };
        assert_eq!(answer(&Settings::default(), &call, 7, true), unavailable());
        let on = Settings {
            reports_stop: true,
            ..Default::default()
        };
        // Whatever the bus says now (a test of the Live worker may have put
        // its own state there): one of the state's answers, never "not
        // running" as a call nobody here runs is told.
        let told = answer(&on, &call, 7, true);
        assert_ne!(told, unavailable());
        assert!(
            told.get("say").is_some()
                || told
                    == json!({"error": "The part of me that sends reports is not running, so nothing is being sent now."}),
            "{told}"
        );
        let texted = answer_typed(&on, &call, crate::messages::REQUESTS_FROM + 1);
        assert_ne!(texted, unavailable());
    }

    /// #3748: `go_to_sleep` is declared and routed only with
    /// `LAMP_SLEEP_ASKED=on`, its flag is taken last, and it is answered at
    /// once with the line to say, or not now for a request begun while
    /// onboarding was under way, on either route.
    #[test]
    fn go_to_sleep_is_the_voices_own_and_says_its_line_or_not_now() {
        let untouched = ["--session".to_owned(), "7".to_owned()];
        let mut flags = crate::requested_audio::arguments_from_environment();
        flags.extend(crate::sleep_asked::arguments(Some("on")));
        flags.extend(untouched.clone());
        let (on, rest) = take(&flags).unwrap();
        assert!(on.sleep_asked);
        assert_eq!(rest, untouched);
        let (off, _) = take(&untouched).unwrap();
        assert!(!off.sleep_asked);
        let named = |settings: &Settings| {
            core_tools(settings)
                .iter()
                .any(|tool| tool.name == crate::sleep_asked::NAME)
        };
        assert!(named(&on) && !named(&off));
        assert!(voices_own(crate::sleep_asked::NAME));
        let call = call(crate::sleep_asked::NAME);
        assert_eq!(route(&on, &call.name), Route::Sleep);
        assert_eq!(route(&off, &call.name), Route::Nobody);
        assert_eq!(answer(&off, &call, 7, true), unavailable());
        // Asked: the line, and the route is told a sleep was asked.
        let asked = answer_inner(&on, &call, 70_001, true, None, None, true);
        assert_eq!(asked.told, crate::sleep_asked::told(false));
        assert!(asked.sleep);
        // Begun while onboarding was under way: not now, and no sleep.
        crate::sleep_asked::mark_onboarding(70_002);
        let walking = answer_inner(&on, &call, 70_002, true, None, None, true);
        assert_eq!(walking.told, crate::sleep_asked::told(true));
        assert!(!walking.sleep);
    }

    /// The reviewer on #1706: the setup line's two notes with `use_skill`,
    /// the skills past its bound and its lines named alone or left out.
    #[test]
    fn the_setup_line_says_what_use_skill_cut() {
        let dir = directory("cut");
        std::fs::create_dir(dir.join(SKILLS_DIR)).unwrap();
        // Within the catalog's own 64 KB, and long enough that the lines fill.
        let long = "x".repeat(300);
        let skills: Vec<Value> = (0..120)
            .map(
                |n| json!({"name": format!("{n:0>48}").replacen('0', "a", 1), "description": long}),
            )
            .collect();
        std::fs::write(
            dir.join(SKILLS_DIR).join(CATALOG_FILE),
            json!({"tools": skills}).to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.join(CATALOG_FILE),
            r#"{"tools":[{"name":"light_on","description":"Light."}]}"#,
        )
        .unwrap();
        let settings = Settings {
            exec: Some(dir.clone()),
            use_skill: true,
            ..Default::default()
        };
        let all = skill_tools(&settings);
        let offered = crate::use_skill::offered(&all).len();
        assert!(offered < 120);
        let said = crate::session_tools::setup_line(&declared(&settings), &settings);
        assert!(
            said.contains(&format!("; {offered} skills through use_skill")),
            "{said}"
        );
        assert!(said.contains(" (past use_skill's bound)"), "{said}");
        let (named, left) = crate::use_skill::cut(crate::use_skill::offered(&all));
        assert!(!named.is_empty() && !left.is_empty());
        assert!(
            said.ends_with(&format!(
                "; use_skill's lines: {} named alone, {} left out",
                named.len(),
                left.len()
            )),
            "{said}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A call of `use_skill` reaches the skill's own socket as the skill's
    /// own call, its choice and yes-or-no under the skill's own names, and
    /// the skill's answer goes to the model: on the cascade's path
    /// ([`answer`]) as on the Live worker's ([`run_call`]). One naming no
    /// skill is told so, and nobody is asked.
    #[test]
    fn a_use_skill_call_is_asked_of_its_skill_as_its_own() {
        let (dir, mut settings) = many_skills("u");
        settings.use_skill = true;
        let skills = dir.join(SKILLS_DIR);
        let served = executive(
            &skills,
            b"{\"ok\":true,\"result\":{\"next\":\"Dance.\"}}\n",
            Duration::ZERO,
        );
        let call = ToolCall {
            id: "c1".into(),
            name: crate::use_skill::NAME.into(),
            args: json!({"skill":"dance","choice":"jazz","flag":true}),
        };
        assert_eq!(answer(&settings, &call, 7, true), json!({"next":"Dance."}));
        assert_eq!(
            served.join().unwrap(),
            json!({"op":"call","name":"dance","args":{"style":"jazz","long":true}})
        );
        std::fs::remove_file(skills.join(CALL_SOCKET)).unwrap();
        let served = executive(&skills, b"{\"ok\":true,\"result\":{}}\n", Duration::ZERO);
        let check = ToolCall {
            args: json!({"skill":"check_my_work"}),
            ..call.clone()
        };
        let (whom, (_, answered)) = run_call(&route(&settings, &check.name), &check, 7, true);
        assert_eq!(whom, "the executive");
        assert!(answered);
        assert_eq!(
            served.join().unwrap(),
            json!({"op":"call","name":"check_my_work","args":{}})
        );
        std::fs::remove_file(skills.join(CALL_SOCKET)).unwrap();
        // No skill of that name: told, and no socket is asked.
        let juggle = ToolCall {
            args: json!({"skill":"juggle"}),
            ..call
        };
        let told = answer(&settings, &juggle, 7, true);
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .starts_with("There is no skill named juggle."),
            "{told}"
        );
        // Told by the voice itself: the journal names use_skill, which
        // answered, not the skills (the reviewer on #1706).
        let (whom, (_, answered)) = run_call(&route(&settings, &juggle.name), &juggle, 7, true);
        assert_eq!((whom, answered), (crate::use_skill::NAME, true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #1668's cost on the host: a call through `use_skill` against the
    /// same call made itself, each answered by a stub skill on its socket,
    /// medians of 300. What it adds in the voice, not the model's turn.
    #[test]
    #[ignore = "host-only timing probe; run alone with --ignored --exact --nocapture"]
    fn use_skill_cost() {
        use std::io::{BufRead, Write};
        let (dir, mut settings) = many_skills("c");
        settings.use_skill = true;
        let listener = UnixListener::bind(dir.join(SKILLS_DIR).join(CALL_SOCKET)).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                std::io::BufReader::new(&stream)
                    .read_line(&mut line)
                    .unwrap();
                (&stream)
                    .write_all(b"{\"ok\":true,\"result\":{}}\n")
                    .unwrap();
            }
        });
        let median = |call: &ToolCall| {
            let mut took: Vec<Duration> = (0..300)
                .map(|_| {
                    let began = Instant::now();
                    let _ = answer(&settings, call, 1, true);
                    began.elapsed()
                })
                .collect();
            took.sort();
            took[150]
        };
        let itself = ToolCall {
            id: String::new(),
            name: "dance".into(),
            args: json!({"style":"jazz"}),
        };
        let through = ToolCall {
            id: String::new(),
            name: crate::use_skill::NAME.into(),
            args: json!({"skill":"dance","choice":"jazz"}),
        };
        let (a, b) = (median(&itself), median(&through));
        println!("use_skill_cost: itself {a:?}, through use_skill {b:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #1673 (the first builder, 19 Oct): a text never runs a skill that asks
    /// for the body, by its own name or through `use_skill`, and its socket
    /// is never asked. A text runs one that doesn't, and a spoken call runs
    /// the moving one as before, with requested audio switched off or on.
    #[test]
    fn a_searchs_note_and_walk_are_read_from_the_tools_file() {
        let dir = directory("sw");
        let own = dir.join("own");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(
            own.join(CATALOG_FILE),
            r#"{"tools":[{"name":"look_for","description":"Look."}],
            "searches":{"look_for":{"note":"Look on.","walk":["left","desk"],
            "looked":{"left":"to the left","desk":"at the desk"},"default":"desk"}}}"#,
        )
        .unwrap();
        let settings = Settings {
            skills: Some(own),
            ..Default::default()
        };
        let walk = search_walk(&settings, "look_for").unwrap();
        assert_eq!(walk.note, "Look on.");
        assert_eq!(walk.walk, ["left", "desk"]);
        assert_eq!(walk.looked["left"], "to the left");
        assert_eq!(walk.default.as_deref(), Some("desk"));
        // A tool with no search, or an older file, gives none.
        assert_eq!(search_walk(&settings, "glow"), None);
        assert_eq!(search_walk(&Settings::default(), "look_for"), None);
    }

    #[test]
    fn a_text_never_runs_a_skill_that_moves_the_body() {
        for requested_audio in [false, true] {
            // Short names: a socket's whole path has to fit in 103 bytes.
            let dir = directory("mv");
            let own = dir.join("own");
            std::fs::create_dir_all(&own).unwrap();
            std::fs::write(
                own.join(CATALOG_FILE),
                r#"{"tools":[{"name":"dance","description":"Dance."},
            {"name":"glow","description":"Glow."}],"body":["dance"]}"#,
            )
            .unwrap();
            let mut settings = Settings {
                skills: Some(own.clone()),
                requested_audio,
                ..Default::default()
            };
            assert_eq!(
                moving_skills(&settings),
                std::collections::BTreeSet::from(["dance".to_owned()])
            );
            let named = |name: &str| ToolCall {
                id: "c".into(),
                name: name.into(),
                args: json!({}),
            };
            let text = crate::messages::REQUESTS_FROM + 1;
            let refused = json!({"error": MOVES_ONLY_AT_THE_LAMP});
            // The skills' socket answers one call: the first that reaches it.
            let served = executive(
                &own,
                b"{\"ok\":true,\"result\":{\"next\":\"Done.\"}}\n",
                Duration::ZERO,
            );
            assert_eq!(answer_typed(&settings, &named("dance"), text), refused);
            assert_eq!(
                answer_typed(&settings, &named("glow"), text),
                json!({"next":"Done."})
            );
            assert_eq!(served.join().unwrap()["name"], "glow");
            settings.use_skill = true;
            let through = ToolCall {
                id: "c".into(),
                name: crate::use_skill::NAME.into(),
                args: json!({"skill":"dance"}),
            };
            // Keep the same live peer for the wrapped refusal and subsequent
            // spoken call: only the latter may consume its one socket request.
            std::fs::remove_file(own.join(CALL_SOCKET)).unwrap();
            let served = executive(
                &own,
                b"{\"ok\":true,\"result\":{\"next\":\"Done.\"}}\n",
                Duration::ZERO,
            );
            assert_eq!(answer_typed(&settings, &through, text), refused);
            let mut context = audio_context();
            context.call_id = through.id.clone();
            if requested_audio {
                let answer = answer_spoken(&settings, &through, &context);
                assert_eq!(answer.told, json!({"next":"Done."}));
                assert!(answer.requested_audio.is_none());
            } else {
                assert_eq!(
                    answer(&settings, &through, 9, true),
                    json!({"next":"Done."})
                );
            }
            let asked = served.join().unwrap();
            assert_eq!(asked["name"], "dance");
            assert_eq!(
                asked.get("requested_audio_context"),
                requested_audio.then(|| json!(&context)).as_ref()
            );
            // An older tools file names none.
            std::fs::write(
                own.join(CATALOG_FILE),
                r#"{"tools":[{"name":"dance","description":"Dance."}]}"#,
            )
            .unwrap();
            assert!(moving_skills(&settings).is_empty());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    /// #1696 with #1656: a `use_skill` call to a skill in the skills' own
    /// folder reaches that folder's socket, as one in the old folder does.
    #[test]
    fn a_use_skill_call_reaches_a_skill_in_the_own_folder() {
        // Short names: a socket's whole path has to fit in 103 bytes.
        let dir = directory("uo");
        let own = dir.join("own");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(
            own.join(CATALOG_FILE),
            r#"{"tools":[{"name":"dance","description":"Dance."}]}"#,
        )
        .unwrap();
        let settings = Settings {
            skills: Some(own.clone()),
            use_skill: true,
            ..Default::default()
        };
        let served = executive(
            &own,
            b"{\"ok\":true,\"result\":{\"next\":\"Dance.\"}}\n",
            Duration::ZERO,
        );
        let call = ToolCall {
            id: "c1".into(),
            name: crate::use_skill::NAME.into(),
            args: json!({"skill":"dance"}),
        };
        assert_eq!(answer(&settings, &call, 7, true), json!({"next":"Dance."}));
        assert_eq!(
            served.join().unwrap(),
            json!({"op":"call","name":"dance","args":{}})
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The skills' own folder (`--skills-dir`), outside the executive's,
    /// which systemd removes with it (lamp-4ace, 19 Oct 04:33): read and
    /// called after the executive's, before the old one inside it, which is
    /// still read for one release; a name the own folder gives is its.
    #[test]
    fn the_skills_own_folder_is_read_and_called_before_the_old_one() {
        // Short names: a socket's whole path has to fit in 103 bytes.
        let dir = directory("own");
        let own = dir.join("own");
        let exec = dir.join("actions");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::create_dir_all(exec.join(SKILLS_DIR)).unwrap();
        std::fs::write(
            exec.join(CATALOG_FILE),
            r#"{"tools":[{"name":"light_on","description":"Turn the light on."}]}"#,
        )
        .unwrap();
        std::fs::write(
            own.join(CATALOG_FILE),
            r#"{"tools":[{"name":"dance","description":"Dance."},
            {"name":"light_on","description":"A name the executive has."}]}"#,
        )
        .unwrap();
        std::fs::write(
            exec.join(SKILLS_DIR).join(CATALOG_FILE),
            r#"{"tools":[{"name":"dance","description":"An older skills' dance."},
            {"name":"check_my_work","description":"Only in the old folder."}]}"#,
        )
        .unwrap();
        let settings = Settings {
            exec: Some(exec.clone()),
            skills: Some(own.clone()),
            ..Default::default()
        };
        let told: Vec<(String, String)> = declared(&settings)
            .into_iter()
            .map(|tool| (tool.name, tool.description))
            .collect();
        assert_eq!(
            told,
            [
                ("light_on".into(), "Turn the light on.".into()),
                ("dance".into(), "Dance.".into()),
                ("check_my_work".into(), "Only in the old folder.".into()),
            ]
        );
        assert_eq!(route(&settings, "dance"), Route::Executive(own.clone()));
        assert_eq!(
            route(&settings, "check_my_work"),
            Route::Executive(exec.join(SKILLS_DIR))
        );
        assert_eq!(route(&settings, "light_on"), Route::Executive(exec.clone()));
        assert!(is_skills(&settings, &route(&settings, "dance")));
        assert!(is_skills(&settings, &route(&settings, "check_my_work")));
        assert!(!is_skills(&settings, &route(&settings, "light_on")));
        // Their own folder live (its socket there): the old one, with the
        // catalog an older lamp-skills left and its dead socket, is not read
        // (the reviewer on #1656); a call of a name only it gave goes to the
        // executive, which refuses it.
        let live = std::os::unix::net::UnixListener::bind(own.join(CALL_SOCKET)).unwrap();
        let names: Vec<String> = declared(&settings)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(names, ["light_on", "dance"]);
        assert_eq!(
            route(&settings, "check_my_work"),
            Route::Executive(exec.clone())
        );
        assert!(is_skills(
            &settings,
            &Route::Executive(exec.join(SKILLS_DIR))
        ));
        drop(live);
        // The skills alone, with no executive: read and called all the same.
        let alone = Settings {
            skills: Some(own.clone()),
            ..Default::default()
        };
        let names: Vec<String> = declared(&alone).into_iter().map(|tool| tool.name).collect();
        assert_eq!(names, ["dance", "light_on"]);
        assert_eq!(route(&alone, "dance"), Route::Executive(own.clone()));
        assert_eq!(route(&alone, "volume_up"), Route::Nobody);
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

    /// One `remember` answered through the memory's route for a line
    /// admitted as `admitted` with a late look at `cache`, `fill` run on the
    /// way: what the memory was asked.
    fn kept_by_late_look(
        name: &str,
        cache: std::sync::Arc<crate::memory_scope::FaceCache>,
        fill: impl FnOnce() + Send + 'static,
    ) -> Value {
        let socket =
            std::env::temp_dir().join(format!("keep-late-{name}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let served = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            stream.write_all(b"{\"ok\":true,\"id\":1}\n").unwrap();
            serde_json::from_str::<Value>(&line).unwrap()
        });
        let request = 2_807;
        let owner = crate::memory_scope::RequestScope::looking_later(
            crate::memory_scope::RequestKey {
                worker: lamp_turn_owner::BootId::new([0x28; 16]).unwrap(),
                session: 1,
                request,
            },
            cache.snapshot(std::time::Instant::now()),
            cache,
        );
        fill();
        // The line ends now: its keeper is the known face named by then
        // plus `KEEPER_WAIT`, as the second voice records it.
        let lease = owner.lease();
        lease.record_keeper(lease.face_at_end(std::time::Instant::now()));
        let settings = Settings {
            memory: Some(socket.clone()),
            ..Default::default()
        };
        let call = ToolCall {
            id: "k1".into(),
            name: crate::remember::REMEMBER.into(),
            args: json!({"text":"Her sister's wedding is this Saturday."}),
        };
        let told = answer_scoped(&settings, &call, request, false, &owner.lease());
        assert_eq!(told["remembered"], true, "{told}");
        let asked = served.join().unwrap();
        std::fs::remove_file(socket).unwrap();
        asked
    }

    /// #2807 on lamp-c0a4: right after a voice restart, a face just
    /// remembered said a dated event in the session's first line, which was
    /// admitted before the eyes named them, as the room's; its keep went to
    /// `everyone`. The keep now goes to the face the eyes name by the line's
    /// end, or within 300 ms past it, never the room.
    #[test]
    fn the_restarts_first_line_from_a_remembered_face_is_kept_as_theirs() {
        let ann = crate::remember::person_id("Ann");
        let face = |name: &str| {
            let path =
                std::env::temp_dir().join(format!("keep-late-face-{name}-{}", std::process::id()));
            std::fs::write(&path, "Ann").unwrap();
            let seen = crate::known_faces::FaceObservation::read(&path);
            std::fs::remove_file(path).unwrap();
            seen
        };
        // Named between the line's start and its end.
        let cache = std::sync::Arc::new(crate::memory_scope::FaceCache::default());
        let filled = cache.clone();
        let seen = face("before");
        let asked = kept_by_late_look("before", cache, move || {
            filled.replace(filled.begin_refresh(), seen);
        });
        assert_eq!(asked["person"], json!(ann), "{asked}");
        // Named only within the wait past the line's end.
        let cache = std::sync::Arc::new(crate::memory_scope::FaceCache::default());
        let filled = cache.clone();
        let seen = face("during");
        let asked = kept_by_late_look("during", cache, move || {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(80));
                filled.replace(filled.begin_refresh(), seen);
            });
        });
        assert_eq!(asked["person"], json!(ann), "{asked}");
    }

    /// A genuinely unknown speaker's keep (nobody known by the end of the
    /// wait) keeps the documented room rule: "said in this room; speaker
    /// unknown", with no person, as before.
    #[test]
    fn an_unknown_speakers_keep_is_the_rooms_as_documented() {
        let cache = std::sync::Arc::new(crate::memory_scope::FaceCache::default());
        let started = std::time::Instant::now();
        let asked = kept_by_late_look("nobody", cache, || {});
        assert!(asked.get("person").is_none(), "{asked}");
        assert!(started.elapsed() < Duration::from_millis(1_200), "bounded");
    }
}
