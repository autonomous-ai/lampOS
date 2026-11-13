//! A skill made by voice (#1685, "An owner makes their own skill"), off
//! unless `LAMP_MAKE_SKILL=on`: "Lamp, make me a skill that dims the light
//! at 10 pm". The model calls [`MAKE`] with the person's request, and the
//! lamp's own model writes it ([`crate::make_skill_local`]): the [`brief`],
//! fixed words that carry the skill format and what a made skill may use,
//! then the request and nothing else of the conversation. A computer's
//! agent never makes one (the first builder, 3 Nov, on the Queue).
//!
//! When the model's answer comes back, its two files are taken from it ([`files`]), held to
//! what a made skill may use ([`capped`]), marked `made: here` and by its
//! [`Maker`] (`made_by: voice` or `made_by: app`), and checked
//! and packed by `lamp-skills pack` (#1698), the checker an install uses.
//! The package is kept in the voice's own folder ([`DIRECTORY`], 0700, at
//! most [`KEPT_MOST`]), with its SHA-256 and what it asks to use. Once
//! kept, it goes on at once, with no spoken yes (the owner, 10 Nov: a made
//! skill goes on with no spoken yes; the app's MAKE IT and the voice ask
//! are the ask, #5977): [`put_on`] marks it to go on ([`goes_on`]) and asks
//! `lamp-skills`' manage helper for that hash alone, which installs only
//! those bytes, held again to the cap, and only for a hash marked so
//! within [`GOES_ON_WITHIN`]. The lamp then says [`on_line`], or
//! [`not_on_line`] with why it isn't on.
//!
//! With [`SCRIPTS_ENVIRONMENT`] on (#6307, off unless set), the model may
//! also write the skill's `scripts/`, run by one `task` step and granted
//! only its `grants`: held to their bounds, packed with the two files,
//! said back with what the code may use, and put on only on the person's
//! yes to that package's SHA-256 ([`put_on_agreed`]), never at once.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

/// Where the coordinator reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_MAKE_SKILL";
/// The function the model calls for a skill to be made.
pub const MAKE: &str = "make_skill";
/// The most of the person's request carried in the brief: the brief and it
/// fit in [`MOST_BRIEF_BYTES`] (480 since the owner found 180 too short in
/// the app, 6 Nov).
pub const MOST_REQUEST_BYTES: usize = 480;
/// At most this many made skills are kept; the oldest not on is let go,
/// and one on never is.
pub const KEPT_MOST: usize = 5;
/// Where the product keeps them: the voice's own, 0700.
pub const DIRECTORY: &str = "/var/lib/lampOS/voice/made-skills";
/// The program that checks and packs them: `lamp-skills`, as the product
/// installs it.
pub const PROGRAM: &str = "/opt/lampOS/skills/current/lamp-skills";
/// The topics a made skill's first triggers read: whether the voice is
/// talking, whether someone is at the lamp, whether a face is in view and
/// someone stood up from the desk (`lamp_skills::PRESENCE_TOPICS`,
/// presence alone: its trigger reads no field of the faces, so never who),
/// and the local clock (`lamp_skills::LOCAL_CLOCK_TOPIC`, which lamp-skills
/// makes itself). No longer a cap (#6066): a made skill may read any topic
/// lamp-skills takes, and [`plainly`] says the triggers it knows
/// (`SAYABLE_TOPICS`).
pub const TOPICS: [&str; 5] = [
    "voice/state",
    "room/presence",
    FACES_TOPIC,
    STOOD_UP_TOPIC,
    CLOCK_TOPIC,
];
/// The faces in view, `humans/faces/tracked`.
const FACES_TOPIC: &str = lamp_msgs::Faces::TOPIC;
/// The eyes' word that the person at the desk stood up to leave.
const STOOD_UP_TOPIC: &str = lamp_msgs::StoodUp::TOPIC;
/// lamp-skills' own local clock, `lamp_skills::LOCAL_CLOCK_TOPIC` written
/// out: this crate can't name it, lamp-skills depends on this one. Held
/// equal to it by lamp-skills' own test (`tests/it/made.rs`).
pub const CLOCK_TOPIC: &str = "clock/local";
/// How long after a kept skill is marked to go on ([`goes_on`]) the manage
/// helper still puts it on, by its own clock: the voice asks the helper
/// right after marking it, so a kept SHA-256 is never put on later by any
/// path. No person's yes is waited for (the owner, 10 Nov).
pub const GOES_ON_WITHIN: Duration = Duration::from_secs(30);
/// The manage helper's socket, `lamp_skills::package::MANAGE_SOCKET`
/// (lamp-skills' tests hold the two the same): root's, which puts a made
/// skill on by its SHA-256 alone.
pub const MANAGE_SOCKET: &str = "/run/lampos-skills-manage/manage.sock";
/// The longest the helper is waited for: its own 4 s for the skills
/// folder, and the install, while the person waits for the answer.
const PUT_ON_WAIT: Duration = Duration::from_secs(8);
/// The longest `lamp-skills pack` may take.
const PACK_WITHIN: Duration = Duration::from_secs(20);
/// The most of either file taken from an answer.
const MOST_FILE_BYTES: usize = 8 * 1024;
/// The most of a description said: what it does, how often it starts on its
/// own (#6066), and what was left out.
const MOST_DESCRIPTION_CHARS: usize = 640;
const _: () = assert!(
    MOST_PLAIN_CHARS + MOST_MANNERS_CHARS + MOST_CANNOT_CHARS + 40 <= MOST_DESCRIPTION_CHARS
);
/// How an answer begins that makes nothing: what was asked is more than a
/// made skill can sense or do.
pub const CANNOT: &str = "cannot:";
/// The most of what the model left out, said with what it does.
const MOST_CANNOT_CHARS: usize = 120;
/// The most of the model's `cannot:` reason, said to the person as it is:
/// room for one short sentence and what to ask instead (#5977: on
/// lamp-74ff, 10 Nov, a reason cut at 120 characters ended "beyond s…" and
/// never said what to ask; the owner: "phải nói rõ tại sao không được").
const MOST_CANNOT_REASON_CHARS: usize = 240;
/// How a line outside the blocks begins that names what was asked and
/// isn't in the skill ("left out: dimming slowly; it dims in one step"):
/// said after what it does, so the person hears what was left out too.
pub const LEFT_OUT: &str = "left out:";
/// The file that keeps what was made, in [`DIRECTORY`].
pub const INDEX: &str = "made.json";

/// The brief's first words: a task that begins with them is a skill to make.
pub const BRIEF_HEAD: &str = "Write one lamp skill (lampOS lamp-skills)";
/// What comes before the person's request in the brief.
const REQUEST_MARK: &str = "Request: ";

/// The switch for a made skill that reads a linked service to its owner
/// alone (#5288, the first builder's ruling there, 4 Nov 20:5x): here, in
/// `lamp-skills`, its manage helper and `lamp-services`. `on` only; off,
/// the making model is told a skill can't use a service at all
/// (`make_skill_local`'s system prompt) and the cap refuses one.
pub const READS_ENVIRONMENT: &str = "LAMP_SKILL_READS";

/// Whether a made skill may read a linked service in this process.
pub fn reads_on() -> bool {
    std::env::var(READS_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The switch for a made skill that sends or writes on its own (#6066, the
/// owner's lift of rule (b), 10 Nov): here, in `lamp-skills`, its manage
/// helper and `lamp-services`. `on` only; off, the making model is told a
/// skill can't send or write, and the cap says so ([`SENDS_ON_ITS_OWN`]).
pub const WRITES_ENVIRONMENT: &str = lamp_services::skill_writes::ENVIRONMENT;

#[cfg(test)]
thread_local! {
    /// [`WRITES_ENVIRONMENT`] as a test of this thread sets it.
    static WRITES_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether a made skill may send or write in this process.
pub fn writes_on() -> bool {
    #[cfg(test)]
    if let Some(on) = WRITES_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(WRITES_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// [`writes_on`] for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_writes_here(on: Option<bool>) {
    WRITES_HERE.with(|here| here.set(on));
}

/// The switch that loosens a skill made here (the owner's ask on the Queue,
/// #6059, 11 Nov: a made skill is the person's own): here and in
/// `lamp-skills`' manage helper (which holds it again to [`capped`]). `on`
/// only; off unless set, in no product unit. On, for a skill made here alone
/// (never one added from a file, from the Store or installed):
///
/// - a `[[do]]` step, or a key of a step, `lamp-skills pack` refuses is left
///   out, not refused, and told ([`loosened`]); never a trigger's key;
/// - no `most_a_day` or shared `[budget]` is added unless the request asks
///   for limits ([`limits_asked`]), its `[rest]` stays, and `[quiet]` only
///   for a skill that says something or hums ([`mannered_with`]);
/// - a say's cooldown is at least [`LEAST_LOOSE_SAY_COOLDOWN_SECS`], not an
///   hour;
/// - a skill that neither speaks nor hums has a cooldown of at least a
///   minute, never raised to an hour ([`LEAST_LOOSE_COOLDOWN_SECS`]), and a
///   repeat the request asks for ("every five minutes while ...",
///   [`repeat_asked`]) runs while its trigger holds: no `rearm`, its
///   cooldown the asked interval ([`say_paced_with`]);
/// - a step [`plainly`] can't say is said in general words, never a send,
///   the camera or the microphone ([`named_exactly`]).
///
/// A write's read-back and yes, the sends (3 a skill, 10 across all skills
/// a day, in `lamp-services`), privacy refusals and the code grants stay
/// as they are.
pub const LOOSE_ENVIRONMENT: &str = "LAMP_MADE_SKILL_LOOSE";

thread_local! {
    /// [`LOOSE_ENVIRONMENT`] as a test of this thread sets it.
    static LOOSE_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether a made skill's checks are loosened in this process.
pub fn loose_on() -> bool {
    if let Some(on) = LOOSE_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(LOOSE_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// [`loose_on`] for the calling thread alone, for tests (this crate's and
/// `lamp-skills`' own against the real check); `None` reads the
/// environment again. The product never calls it.
#[doc(hidden)]
pub fn set_loose_here(on: Option<bool>) {
    LOOSE_HERE.with(|here| here.set(on));
}

/// The least cooldown of a made skill that speaks with [`LOOSE_ENVIRONMENT`]
/// on: a minute, as often as the person asked, never every moment.
pub const LEAST_LOOSE_SAY_COOLDOWN_SECS: u64 = 60;

/// The least cooldown of a made skill that neither speaks nor hums with
/// [`LOOSE_ENVIRONMENT`] on, and of a repeat it asked for: a minute, as a
/// getting up's ([`LEAST_STOOD_UP_COOLDOWN_SECS`]); never the hour the
/// making model copies from its example ([`say_paced_with`]).
pub const LEAST_LOOSE_COOLDOWN_SECS: u64 = LEAST_STOOD_UP_COOLDOWN_SECS;

/// The least cooldown of a made skill that speaks, as [`loose_on`] has it.
fn least_say_cooldown_secs() -> u64 {
    if loose_on() {
        LEAST_LOOSE_SAY_COOLDOWN_SECS
    } else {
        LEAST_SAY_COOLDOWN_SECS
    }
}

/// What the lamp can't do yet with [`WRITES_ENVIRONMENT`] off, said as
/// [`cant_yet`]: "I can't do that yet: sending or writing on its own".
pub const SENDS_ON_ITS_OWN: &str = "sending or writing on its own";

/// How the say-back of a skill that sends or writes ends (#6066): that it
/// waits for the person's agreement, never on a word for them to repeat,
/// so its own last words heard back are no yes. It has no "yes" in it: a
/// line made only of the say-back's words, begun in its echo tail, is held
/// while the yes is awaited (`second_provider::own_echo::yes_echo`), and a
/// person's quick "Yes." must not be one.
pub const WAITS_FOR_YES: &str = " It waits for you to agree before it sends anything.";

/// The switch for a made skill that carries code of its own (#6307, #6071
/// PR 4, the first builder's 12:45 condition 4: model-written scripts last,
/// behind a switch of their own, off). `on` only. Off, the making model is
/// never told a skill may carry code, and any `scripts/` file in its answer,
/// a `task` step or a `grants` list is refused as what the lamp can't do yet
/// ([`CODE_OF_ITS_OWN`]). On, the model may write a skill's `scripts/`
/// within [`MOST_SCRIPTS`], [`MOST_SCRIPT_BYTES`] and [`MOST_SCRIPTS_BYTES`],
/// run by one `task = { task, run }` step (#6305's `do.task`) and granted
/// only its `grants`; its say-back names the grants and that it runs its own
/// code, and it goes on only on the person's yes to that package's SHA-256
/// (the whole folder, its scripts with it), never at once.
pub const SCRIPTS_ENVIRONMENT: &str = "LAMP_MADE_SCRIPTS";

#[cfg(test)]
thread_local! {
    /// [`SCRIPTS_ENVIRONMENT`] as a test of this thread sets it.
    static SCRIPTS_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether a made skill may carry code of its own in this process.
pub fn scripts_on() -> bool {
    #[cfg(test)]
    if let Some(on) = SCRIPTS_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(SCRIPTS_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// [`scripts_on`] for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_scripts_here(on: Option<bool>) {
    SCRIPTS_HERE.with(|here| here.set(on));
}

/// The switch for a made skill that uses a linked service (mail, a
/// calendar, GitHub, ...) through the lamp's agent (the owner's ask, 12
/// Nov): `on` exactly, off unless set. On, with [`SCRIPTS_ENVIRONMENT`] on
/// too (a made `task` step is held to its cap), the making model is told to
/// make such a skill as one `do.task` for the lamp's agent (Hermes, which
/// reaches the linked services through its `lamp-services` MCP tools
/// `services_connected`, `service_read` and `service_write`; a send goes
/// back to the voice for the owner's spoken yes), never as a `do.read` or
/// `do.write` step. Off, the make is as before. Skills already made with a
/// `do.read` or `do.write` load and run as they did: nothing migrates.
pub const AGENT_SERVICES_ENVIRONMENT: &str = "LAMP_AGENT_SERVICES";

/// Whether [`AGENT_SERVICES_ENVIRONMENT`]'s value `var` turns it on: `on`
/// exactly.
pub fn agent_services_on(var: Option<&str>) -> bool {
    var == Some("on")
}

#[cfg(test)]
thread_local! {
    /// [`AGENT_SERVICES_ENVIRONMENT`] as a test of this thread sets it.
    static AGENT_SERVICES_HERE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// [`agent_services_on`] as this process has it.
pub fn agent_services_here() -> bool {
    #[cfg(test)]
    if let Some(on) = AGENT_SERVICES_HERE.with(std::cell::Cell::get) {
        return on;
    }
    agent_services_on(std::env::var(AGENT_SERVICES_ENVIRONMENT).ok().as_deref())
}

/// [`agent_services_here`] for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_agent_services_here(on: Option<bool>) {
    AGENT_SERVICES_HERE.with(|here| here.set(on));
}

/// The linked services a service task's run may be granted, by their
/// catalog code (the first builder's ruling on #6432): its grants are
/// `<code>-read` and `<code>-write`, as `lamp_control::protocol::SERVICE_CODES`
/// names them (the same list; lamp-tasks' grant check follows it).
pub const SERVICE_GRANT_CODES: [&str; 14] = [
    "gmail",
    "google_calendar",
    "google_drive",
    "figma",
    "github",
    "linear",
    "asana",
    "ahrefs",
    "facebook",
    "slack",
    "mondaydotcom",
    "hubspot",
    "amplitude",
    "notion",
];

/// A service task's grant (`gmail-read`, `github-write`): its service's
/// code and whether it writes; `None` for any other name.
pub fn service_grant(grant: &str) -> Option<(&str, bool)> {
    let (code, write) = grant
        .strip_suffix("-read")
        .map(|code| (code, false))
        .or_else(|| grant.strip_suffix("-write").map(|code| (code, true)))?;
    SERVICE_GRANT_CODES.contains(&code).then_some((code, write))
}

/// The switch that puts a made skill with code of its own (#6307,
/// [`SCRIPTS_ENVIRONMENT`]) on at once, by voice or from the app, with no
/// spoken yes (the owner's ask: a made skill goes on at once, and the lamp
/// says whether it worked, with no confirming). `on` only; off unless set,
/// in no product unit. On, it goes on through its yes's own path
/// (`make_skill_local::code_approve`: that package by its SHA-256, then its
/// task's yes recorded), its say-back says it is on and what its code may
/// use ([`code_said`], with no [`RUNS_AFTER_YES`]), and nothing waits. Off,
/// it waits for the spoken yes as before.
pub const CODE_AT_ONCE_ENVIRONMENT: &str = "LAMP_MADE_CODE_ON_AT_ONCE";

#[cfg(test)]
thread_local! {
    /// [`CODE_AT_ONCE_ENVIRONMENT`] as a test of this thread sets it.
    static CODE_AT_ONCE_HERE: std::cell::Cell<Option<bool>> =
        const { std::cell::Cell::new(None) };
}

/// Whether a made skill with code of its own goes on at once in this
/// process ([`CODE_AT_ONCE_ENVIRONMENT`]); only with [`scripts_on`].
pub fn code_on_at_once() -> bool {
    if !scripts_on() {
        return false;
    }
    #[cfg(test)]
    if let Some(on) = CODE_AT_ONCE_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(CODE_AT_ONCE_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// [`code_on_at_once`] for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_code_at_once_here(on: Option<bool>) {
    CODE_AT_ONCE_HERE.with(|here| here.set(on));
}

/// The grants a made skill's code may hold and still go on at once with
/// [`CODE_AT_ONCE_ENVIRONMENT`] (the first builder's ruling on #6059,
/// 19:14, and his 19:44 line: "sound" is the lamp's own sounds and voice):
/// its light, its voice and sounds (`speak`) and its head and arm; a timer
/// is a trigger and needs no grant. Any other grant keeps the spoken yes
/// ([`CODE_YES_GRANTS`] among them).
pub const CODE_AT_ONCE_GRANTS: [&str; 3] = ["light", "speak", "body"];

/// The grants that always keep a made skill's code to its spoken yes, the
/// switch on or off (the same ruling): the camera, the microphone and
/// `sound`, which is hearing, not playing (lamp-control's grant: "can hear
/// kinds of sound, never words"; hearing keeps its yes, the 19:44 line). A
/// send (a write, or messages to the phone) keeps it too
/// ([`code_goes_on_at_once`]).
pub const CODE_YES_GRANTS: [&str; 3] = ["camera", "microphone", "sound"];

/// Whether code granted `grants` may go on at once: the switch on and
/// every grant one of [`CODE_AT_ONCE_GRANTS`] (none at all is one).
pub fn grants_go_on_at_once(grants: &[String]) -> bool {
    code_on_at_once()
        && grants
            .iter()
            .all(|grant| CODE_AT_ONCE_GRANTS.contains(&grant.as_str()))
}

/// Whether the made skill `made`, with code of its own, goes on at once
/// ([`CODE_AT_ONCE_ENVIRONMENT`]): its grants within
/// [`CODE_AT_ONCE_GRANTS`] ([`grants_go_on_at_once`]), and no send, no
/// camera, no microphone and no knowing who is at the lamp (`who`,
/// `known_faces`: identity is the camera's privacy, the first builder at
/// 20:16 on #6659) in what it uses. lamp-control has no grant for who is
/// there (`look` is "never who"), so these are its `uses`. Anything else
/// waits for the spoken yes, exactly as with the switch off.
pub fn code_goes_on_at_once(made: &Made) -> bool {
    made.uses.code
        && made.uses.writes.is_empty()
        && !made.uses.messages
        && !made.uses.camera
        && !made.uses.listens()
        && !made.uses.who
        && !made.uses.known_faces
        && grants_go_on_at_once(&made.uses.grants)
}

/// What the lamp adds of a skill with code of its own put on at once
/// ([`code_goes_on_at_once`]) whose task wasn't kept (a skill's tasks off, or
/// its folder's grants not those said): its code runs nowhere yet.
pub const CODE_RUNS_NOWHERE_YET: &str = " But this lamp doesn't run its code yet.";

/// What the lamp can't do yet with [`SCRIPTS_ENVIRONMENT`] off, said as
/// [`cant_yet`]: "I can't do that yet: running code of its own".
pub const CODE_OF_ITS_OWN: &str = "running code of its own";
/// The folder a made skill's code is kept in, inside its own.
pub const SCRIPTS_FOLDER: &str = "scripts/";
/// The most files a made skill's code may have.
pub const MOST_SCRIPTS: usize = 4;
/// The most of one of them, as of either of its other two files.
pub const MOST_SCRIPT_BYTES: usize = MOST_FILE_BYTES;
/// The most of them all together.
pub const MOST_SCRIPTS_BYTES: usize = 16 * 1024;
/// The kinds of text file its code may have, by their ending; the one its
/// `task` step runs is Python, as the lamp's SDK is first (#6304).
const SCRIPT_ENDINGS: [&str; 4] = ["py", "json", "txt", "md"];
/// The ending of the file a `task` step runs.
const RUNNABLE_ENDING: &str = ".py";
/// The most of a script's path past [`SCRIPTS_FOLDER`], in bytes.
const MOST_SCRIPT_PATH_BYTES: usize = 64;
/// The most of a `task` step's own words, said back whole.
pub const MOST_TASK_CHARS: usize = 120;

/// What a made skill's code may be granted: the grants a `do.task`'s script
/// may hold (`crate::skill_task_yes::GRANTS`, #6305, the same names in the
/// same order, a test holds), each with what this say-back's sentence says
/// of it ("that code may use the camera"). `skills` is left out: code a
/// model wrote never puts other skills on the lamp, which would then run
/// with no yes of their own ([`SKILLS_GRANT`]).
pub const GRANTS: [(&str, &str); 12] = [
    ("body", "its head and arm"),
    ("light", "its light"),
    ("speak", "its voice and sounds"),
    ("look", "whether someone is there, never who"),
    ("hear", "its own phrases when it hears them"),
    ("sound", "the kinds of sound it hears"),
    ("air", "the air sensor"),
    ("device", "the lamp's processor, memory and temperature"),
    ("camera", "the camera"),
    ("microphone", "the microphone"),
    ("notes", "small notes of its own on the lamp"),
    ("speech", "everything the lamp says out loud"),
];

/// The grant a made skill's code never has ([`GRANTS`]), and what the lamp
/// can't do yet when it is asked for.
const SKILLS_GRANT: (&str, &str) = ("skills", "code that puts other skills on the lamp");

/// Why a made skill's code is refused for its form, the person's reason;
/// the repair is told [`SCRIPTS_ONLY`] with what was wrong.
pub const SCRIPTS_NOT_HELD: &str = "its code isn't in a form a made skill may carry; try asking \
                                    for something simpler";

/// What a repair is told of a made skill's code ([`SCRIPTS_NOT_HELD`]).
const SCRIPTS_ONLY: &str = "Code is at most 4 more fenced blocks, each named ```scripts/NAME \
                            (letters, digits, - _ and ., ending .py, .json, .txt or .md), plain \
                            text, at most 8 KiB each and 16 KiB in all; it runs only by exactly \
                            one [[do]] step task = { task = \"what it does\", run = \
                            \"scripts/main.py\" }, naming one of its .py files; grants = [ ... ] \
                            at the top lists only what the code uses";

/// How the say-back of a skill with code of its own ends, as
/// [`WAITS_FOR_YES`] does: it goes on only once the person agrees, never
/// on a word for them to repeat, with no "yes" in it.
pub const RUNS_AFTER_YES: &str = " It waits for you to agree before it runs any code.";

/// A calendar's step, given whole: its events of a day.
pub const CALENDAR_READ: &str = "read = { service = \"google_calendar\", what = \"events\", \
     day = \"today\" }, or day = \"tomorrow\" when tomorrow is asked";

/// Mail's step, given whole: the latest mail.
pub const GMAIL_READ: &str = "read = { service = \"gmail\", what = \"recent\" }";

/// Drive's step, given whole: its files come newest first, and `query` is
/// only words to search for. A lamp, 5 Nov: "read my latest Google Drive
/// files" was made from Drive's listed reads with `query = "order by
/// modifiedTime desc"`, which matched no file (lamp-skills now refuses an
/// order, `service_read::QUERY_NOT_AN_ORDER`). A file by `id` is left out:
/// a request said aloud names none. The note after the step names no key
/// of its own: on 10300 (lamp-9adf, 5 Nov 12:59) "(newest first; …)" right
/// after the braces came back twice as one more key in them, refused as
/// `unknown field` at the key after `what`.
pub const DRIVE_READ: &str = "read = { service = \"google_drive\", what = \"files\" }, \
     no other key: files come newest first; query is search words";

/// Each linked service a made skill may read, as one line for the making
/// model: its name, its code and the one step that reads it (a calendar's,
/// mail's or Drive's given whole, any other's reads listed to choose from,
/// never a whole text or a free-form read). From `lamp-services`' own
/// catalogue, so a service with reads added there is offered here too.
/// Which one a request asks for, and whether it asks for a read at all,
/// is the model's to judge (the owner, 10 Nov: decisions go to the model,
/// not word lists); the cap ([`capped`]) still holds the skill to one
/// readable service, read only.
pub fn read_steps() -> Vec<String> {
    lamp_services::reads::WHATS
        .iter()
        .map(|(code, _)| *code)
        .filter(|code| lamp_services::spoken::not_readable(code).is_none())
        .map(|code| {
            let step = match code {
                "google_calendar" => CALENDAR_READ.to_owned(),
                "gmail" => GMAIL_READ.to_owned(),
                "google_drive" => DRIVE_READ.to_owned(),
                _ => format!(
                    "read = {{ service = \"{code}\", what = one of {}, its keys }}",
                    lamp_services::spoken::whats(code).join("; ")
                ),
            };
            format!("{} ({code}): {step}", service_name(code))
        })
        .collect()
}

/// The linked services a made skill can't read, by name, each once: the
/// making model answers a request for one with `cannot:`.
pub fn unreadable_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for service in lamp_services::catalog::SERVICES {
        if lamp_services::spoken::not_readable(service.code).is_some()
            && !names.contains(&service.name)
        {
            names.push(service.name);
        }
    }
    names
}

/// The brief: the format of a skill a lamp can make, what it may use, and
/// the person's request, within [`MOST_BRIEF_BYTES`]. Nothing of
/// the conversation but the request: the model passes it, in the person's
/// words.
pub fn brief(request: &str) -> Result<String, &'static str> {
    brief_with(request, reads_on())
}

/// [`brief`], with a made skill's reads on or off ([`READS_ENVIRONMENT`]).
/// One brief for every request: whether it asks for the light and the
/// head, for a read of one linked service, or for something no made skill
/// does is the making model's to judge (the owner, 10 Nov: decisions go to
/// the model, not word lists), by the rules in its system prompt
/// (`make_skill_local`), which answers what it can't make with
/// [`CANNOT`]. On, the brief adds a few words pointing to a read ("as
/// above": the system prompt, which says its form and each service's step,
/// [`read_steps`]), so the longest brief stays within [`MOST_BRIEF_BYTES`].
/// It no longer says "write no files": no maker has had tools to write
/// with since the lamp's own model became the only one (3 Nov). Only an empty or too long
/// request is refused here, before the model is asked.
pub fn brief_with(request: &str, reads: bool) -> Result<String, &'static str> {
    brief_for(request, reads, agent_services_here() && scripts_on())
}

/// [`brief_with`], with a linked service made as a task for the lamp's
/// agent or not (`agent`, [`AGENT_SERVICES_ENVIRONMENT`] with
/// [`SCRIPTS_ENVIRONMENT`]): on, the brief points to that task ("as
/// above": the system prompt says its form) in place of a read or a write.
pub fn brief_for(request: &str, reads: bool, agent: bool) -> Result<String, &'static str> {
    let request = request.split_whitespace().collect::<Vec<_>>().join(" ");
    if request.is_empty() {
        return Err("Say what the skill should do.");
    }
    if request.len() > MOST_REQUEST_BYTES {
        return Err("That is too long to ask for as one skill: say it more shortly.");
    }
    // A read or a write as the system prompt says them (`make_skill_local`),
    // a write's words only the request's own (#6066).
    let read = match (reads, writes_on()) {
        _ if agent => "",
        (true, true) => " Or a read/write as above.",
        (true, false) => " Or one read, as above.",
        (false, true) => " Or one write, as above.",
        (false, false) => "",
    };
    // Code of its own as the system prompt says it (#6307), only with its
    // switch: off, the brief never offers it.
    // With a service made as a task for the lamp's agent, one line points
    // to both, within [`MOST_BRIEF_BYTES`].
    let code = if agent {
        " Or a task, as above."
    } else if scripts_on() {
        " Or code, as above."
    } else {
        ""
    };
    Ok(format!(
        "{BRIEF_HEAD} for the request. Reply with two fenced blocks: \
         ```skill.toml, ```SKILL.md. skill.toml: \
         kind = \"installable\", version = \"1.0.0\", no name key; [permissions] \
         light/body/speak = true, its topic, any other, as used; one [trigger] as listed \
         (coming to the desk is room/presence), with for, cooldown, rearm = \"ends\", \
         during = {{ from = \"HH:MM\", to = \"HH:MM\" }} if asked; \
         [ask] policy = \"nudge\" or listed; [[do]] only steps asked: light = \
         {{ colour, level 1-5, for }}, body = {{ gesture }}, say = {{ text }} (exact \
         words, asker's language) or cue = \"hum\".{read}{code} SKILL.md: \
         front matter name (lowercase, hyphens), description. \
         {REQUEST_MARK}{request}"
    ))
}

/// Whether words are a [`brief`]: an app's ask recorded (`crate::app_make`),
/// or a task handed over before the lamp made skills itself, whose answer
/// is never told.
pub fn is_brief(task: &str) -> bool {
    task.starts_with(BRIEF_HEAD)
}

/// The person's request in a [`brief`].
pub fn request_of(task: &str) -> &str {
    task.rsplit_once(REQUEST_MARK)
        .map_or("", |(_, request)| request)
}

/// What a repair's brief says before the check's lines (#2813 piece 2).
pub const REPAIR_MARK: &str = "Your last answer did not pass the lamp's check, which said: ";
/// What it asks after them.
const REPAIR_ASK: &str = " Answer again with both blocks, fixed. ";
/// The check's lines a repair carries, at most, in bytes: with the longest
/// request the brief stays within 2,000 bytes (the first
/// builder on #3126, 23 Oct 13:07). 560 since the brief says what each
/// trigger means (lamp-74ff, 10 Nov: "come back to my desk"); 735 since a
/// made skill has six triggers ([`SAYABLE_TRIGGERS`]), the brief shorter
/// by as much, its triggers and gestures told in the system prompt
/// (`make_skill_local`): the two still within the tasks part's 2,000.
pub const MOST_REPAIR_BYTES: usize = 735;
/// The longest a [`brief`] is with the longest request, any service's
/// read step included: no longer half the tasks part's 2,000 bytes, but
/// small enough that its repair, with the [`REPAIR_MARK`], the check's
/// [`MOST_REPAIR_BYTES`] and the ask, still fits that bound.
pub const MOST_BRIEF_BYTES: usize = 1_160;
const _: () = assert!(
    MOST_BRIEF_BYTES + REPAIR_MARK.len() + MOST_REPAIR_BYTES + REPAIR_ASK.len()
        <= crate::agent_tasks::MOST_TASK_BYTES
);

/// The brief for a repair (#2813 piece 2): when the lamp's check refuses
/// a made skill, the model is asked once more, with the whole [`brief`]
/// again and the check's `lines` ([`check_lines`]) before the request.
/// Those lines are the model's own skill read back to it, never the
/// conversation.
pub fn repair_brief(request: &str, lines: &str) -> Result<String, &'static str> {
    let brief = brief(request)?;
    let (head, request) = brief
        .rsplit_once(REQUEST_MARK)
        .expect("a brief ends with its request");
    Ok(format!(
        "{head}{REPAIR_MARK}{}{REPAIR_ASK}{REQUEST_MARK}{request}",
        cut_bytes(lines, MOST_REPAIR_BYTES)
    ))
}

/// Why a made skill is not kept, in the lamp's words for the journal (the
/// person hears [`plain_reason`]'s, or the model's own `cannot:`
/// sentence); and when the lamp's check refused it, the check's lines for
/// a repair ([`check_lines`]), never said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub String, pub Option<String>);

impl Refused {
    fn new(why: impl Into<String>) -> Self {
        Self(why.into(), None)
    }
}

/// The two files of a made skill, as the model wrote them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Files {
    pub toml: String,
    pub card: String,
    /// What was asked and isn't in it, as the model's [`LEFT_OUT`] line
    /// outside the blocks said it: told with what it does.
    pub left_out: Option<String>,
    /// Its code, with [`SCRIPTS_ENVIRONMENT`] on: each file's path, under
    /// [`SCRIPTS_FOLDER`], and its text, in the order written. None off.
    pub scripts: Vec<(String, String)>,
}

/// Whether `path` is a made skill's script as it may be written: under
/// [`SCRIPTS_FOLDER`], one or two names deep, each name of ASCII letters,
/// digits, `-`, `_` and `.`, never beginning with a dot (so never `..`), and
/// the file ending as one of [`SCRIPT_ENDINGS`].
fn script_path_ok(path: &str) -> bool {
    let Some(rest) = path.strip_prefix(SCRIPTS_FOLDER) else {
        return false;
    };
    let names: Vec<&str> = rest.split('/').collect();
    let named = |name: &&str| {
        !name.is_empty()
            && !name.starts_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    rest.len() <= MOST_SCRIPT_PATH_BYTES
        && (1..=2).contains(&names.len())
        && names.iter().all(named)
        && rest
            .rsplit_once('.')
            .is_some_and(|(_, ending)| SCRIPT_ENDINGS.contains(&ending))
}

/// Whether `text` is a made skill's script as it may be kept: plain text,
/// no control character but a tab or a line's end, and no format character
/// ([`is_format`]), which could make what is shown differ from what runs.
fn script_text_ok(text: &str) -> bool {
    text.chars()
        .all(|c| (!c.is_control() || c == '\n' || c == '\t') && !is_format(c))
}

/// A made skill's code refused for its form: [`SCRIPTS_NOT_HELD`] said, and
/// `what` with [`SCRIPTS_ONLY`] told the repair; `what`, a fixed phrase,
/// journaled.
fn scripts_not_held(what: &str) -> Refused {
    eprintln!("lamp-voice: a made skill's code isn't held: {what}");
    Refused(
        SCRIPTS_NOT_HELD.into(),
        Some(format!("The lamp refused its code: {what}. {SCRIPTS_ONLY}")),
    )
}

/// What follows `word` at the start of `line`, in any case.
fn after_word<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    line.get(..word.len())
        .filter(|head| head.eq_ignore_ascii_case(word))
        .map(|_| &line[word.len()..])
}

/// Whether the model's answer is its word that no made skill can do what
/// was asked ([`CANNOT`], and no fenced block): its reason is the model's
/// own words, which may echo the person's, so it is told to the person and
/// never journaled ([`MODELS_CANNOT_JOURNALED`]; Second's HOLD on #5985).
pub fn says_cannot(answer: &str) -> bool {
    after_word(answer.trim_start(), CANNOT).is_some() && !answer.contains("```")
}

/// What the journal says in place of the model's own `cannot:` reason.
pub const MODELS_CANNOT_JOURNALED: &str =
    "the lamp's model said no made skill can do it (its reason told, not journaled)";

/// Why a make failed, as the person hears it and the app shows it (the
/// owner, 10 Nov: "when making fails I hear no reason from the lamp, nor in
/// the app"): plain words and what to try, never a file's or a part's
/// name. What exactly was refused is the journal's and the repair's.
pub const MODEL_CUT_OFF: &str =
    "the lamp's model stopped before it finished writing it; try asking again";
/// An answer that isn't a skill's two files.
pub const NOT_A_SKILLS_FORM: &str =
    "the lamp's model didn't write it in a skill's form; try asking again";
/// A `skill.toml` that isn't TOML ([`unreadable`]).
pub const NOT_READABLE: &str =
    "the lamp's model wrote it in a form the lamp can't read; try asking again";
/// Refused by `lamp-skills pack` ([`pack`]).
pub const NOT_PASSED: &str = "it didn't pass the lamp's own check of what a skill may do; try \
                              again, maybe more simply";

/// The lamp's model gave no answer.
pub const MODEL_SILENT: &str = "the lamp's model didn't answer";
/// The lamp's model gave back the maker's own example.
pub const EXAMPLE_GIVEN_BACK_WHY: &str =
    "the lamp's model gave back its own example, not what was asked; try asking again";
/// The lamp couldn't write, check or keep what was made: its own fault,
/// never the person's ask.
pub const NOT_KEPT_HERE: &str = "the lamp couldn't keep it";
pub const NOT_CHECKED: &str = "the lamp couldn't check it";
pub const NOT_CHECKED_IN_TIME: &str = "the lamp couldn't check it in time";

/// What the person hears of a make the lamp's check refused after the
/// repairs were used up, or that couldn't be said plainly (the owner, 11
/// Nov: makes failed "with reasons nobody understands"): one thing to ask,
/// never the check's words.
pub const NOT_MADE_TRY: &str =
    "I couldn't make that one. Try asking for one thing, like 'say hello when I sit down'.";
/// What the person hears when the lamp couldn't keep what it made.
pub const NOT_KEPT_TRY: &str = "Something went wrong keeping it. Please try again.";

/// The reasons [`plain_reason`] gives as they are: the model's silence or
/// cut, said plainly with what to try.
const PLAIN_REASONS: [&str; 2] = [MODEL_CUT_OFF, MODEL_SILENT];

/// Why a make failed, as the person hears it and the app shows it: the
/// lamp's own reason `why` (never the model's `cannot:` sentence, which
/// is already the person's words and goes as it is) in everyday words.
/// What it can't do yet ([`CANT_YET`]), that they already have it
/// ([`ALREADY_HAVE`]), the model's silence or cut and a name with no free
/// one left go as they are; the lamp failing to write, check or keep it is
/// [`NOT_KEPT_TRY`]; any other (the check's refusal, a skill that couldn't
/// be said plainly, a form the lamp can't read, each with its repairs used
/// up) is [`NOT_MADE_TRY`]. The detailed reason stays in the journal.
pub fn plain_reason(why: &str) -> String {
    let why = why.trim();
    if why.starts_with(CANT_YET)
        || why.starts_with(ALREADY_HAVE)
        || why.starts_with(NAME_TAKEN_HEAD)
        || [NOT_MADE_TRY, NOT_KEPT_TRY].contains(&why)
        || PLAIN_REASONS.contains(&why)
    {
        return why.to_owned();
    }
    if [NOT_KEPT_HERE, NOT_CHECKED, NOT_CHECKED_IN_TIME].contains(&why) {
        return NOT_KEPT_TRY.to_owned();
    }
    NOT_MADE_TRY.to_owned()
}

/// How the reason begins when no free name is left ([`free_name`]).
const NAME_TAKEN_HEAD: &str = "a skill called ";

/// What the lamp says of a skill it couldn't make, by voice or from the
/// app: a whole line of its own ([`CANT_YET`], [`ALREADY_HAVE`],
/// [`NOT_MADE_TRY`], [`NOT_KEPT_TRY`]) as it is, or "I couldn't make
/// that:" and why, ending as a sentence (#5977). `why` is
/// [`plain_reason`]'s, or the model's own `cannot:` sentence with what to
/// ask instead.
pub fn not_made_line(why: &str) -> String {
    let why = why.trim();
    // What it can't do yet is said as itself (#6066), as are the lines
    // whole already.
    if why.starts_with(CANT_YET)
        || why.starts_with(ALREADY_HAVE)
        || [NOT_MADE_TRY, NOT_KEPT_TRY].contains(&why)
    {
        return match why.ends_with(['.', '!', '?']) {
            true => why.to_owned(),
            false => format!("{why}."),
        };
    }
    if why.ends_with(['.', '!', '?']) {
        format!("{NOT_MADE_HEAD} {why}")
    } else {
        format!("{NOT_MADE_HEAD} {why}.")
    }
}

/// How [`not_made_line`] begins.
pub const NOT_MADE_HEAD: &str = "I couldn't make that:";

/// The two files of the model's answer: exactly one fenced block named
/// `skill.toml` and one named `SKILL.md` (its info string's last word, in
/// any case, or its language, `toml` or `md`; a bare fence by what it
/// holds, [`fenced`]), and no other fenced block but, with
/// [`SCRIPTS_ENVIRONMENT`] on, its code (`scripts/NAME`, [`script_path_ok`],
/// plain text, within [`MOST_SCRIPTS`] and [`MOST_SCRIPTS_BYTES`]); each at
/// most [`MOST_FILE_BYTES`]. A block left open is the model cut off
/// ([`MODEL_CUT_OFF`]): the lamp's own maker closes one left open at the
/// end of an answer that ended whole before it comes here
/// ([`open_fence`]).
pub fn files(answer: &str) -> Result<Files, Refused> {
    // The model's word that no made skill can do what was asked (it can't
    // hear words said, say anything, or keep time by itself): its reason,
    // one sentence to the person with what to ask instead, is the refusal
    // as it is, the app showing it and the voice saying it (#5977), never
    // stood in for by something else and never cut mid-word.
    if says_cannot(answer)
        && let Some(why) = after_word(answer.trim_start(), CANNOT)
    {
        let why = cut_sentences(why, MOST_CANNOT_REASON_CHARS);
        if !why.is_empty() {
            return Err(Refused::new(why));
        }
        return Err(Refused::new("a skill can't do that"));
    }
    let mut toml = None;
    let mut card = None;
    let mut scripts: Vec<(String, String)> = Vec::new();
    let fenced = fenced(answer);
    for block in fenced.blocks {
        if block.open {
            return Err(Refused::new(MODEL_CUT_OFF));
        }
        if block.body.len() > MOST_FILE_BYTES {
            return Err(Refused::new(
                "the lamp's model wrote more than a skill can hold; try asking for something \
                 simpler",
            ));
        }
        // A file of its code (#6307): only with its switch, and only
        // within its bounds. Off, what the lamp can't do yet, never
        // repaired away.
        if let Named::Script(name) = &block.named {
            if !scripts_on() {
                eprintln!("lamp-voice: a made skill's answer carries code, and its switch is off");
                return Err(Refused::new(cant_yet(CODE_OF_ITS_OWN)));
            }
            if !script_path_ok(name) {
                return Err(scripts_not_held("a file's name or ending"));
            }
            if !script_text_ok(&block.body) {
                return Err(scripts_not_held("a file that isn't plain text"));
            }
            if scripts.iter().any(|(path, _)| path == name) {
                return Err(not_a_skills_form("one of its files twice"));
            }
            scripts.push((name.clone(), block.body));
            if scripts.len() > MOST_SCRIPTS {
                return Err(scripts_not_held("more files than a made skill may carry"));
            }
            if scripts.iter().map(|(_, text)| text.len()).sum::<usize>() > MOST_SCRIPTS_BYTES {
                return Err(scripts_not_held("more code than a made skill may carry"));
            }
            continue;
        }
        let slot = match block.part() {
            Some(Part::Toml) => &mut toml,
            Some(Part::Card) => &mut card,
            None => return Err(not_a_skills_form("a file a made skill can't have")),
        };
        if slot.replace(block.body).is_some() {
            return Err(not_a_skills_form("one of its files twice"));
        }
    }
    let left_out = fenced.left_out;
    match (toml, card) {
        (Some(toml), Some(card)) => Ok(Files {
            toml,
            card,
            left_out,
            scripts,
        }),
        _ => Err(not_a_skills_form("not both of a skill's files")),
    }
}

/// An answer not in a skill's form: said plainly ([`NOT_A_SKILLS_FORM`]),
/// and how, a fixed phrase, journaled.
fn not_a_skills_form(how: &str) -> Refused {
    eprintln!("lamp-voice: a made skill's answer isn't a skill's form: {how}");
    Refused::new(NOT_A_SKILLS_FORM)
}

/// The fence that closes what the answer left open at its end, a line of
/// its own for each block still open; none when every block it opened is
/// closed (a product lamp on 11798, 11 Nov: an answer that ended whole,
/// its last fence never written, was taken for a cut stream).
pub fn open_fence(answer: &str) -> Option<String> {
    fenced(answer).closing
}

/// Which of a made skill's two files a fenced block is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Toml,
    Card,
}

/// A fence's name, from its info string: one of the two files, a file of
/// its code (#6307, `scripts/…` as written, never lowercased), none, or
/// another.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Named {
    Part(Part),
    Script(String),
    Bare,
    Other,
}

/// A fenced block of the model's answer, as it was written.
struct Block {
    named: Named,
    body: String,
    /// The answer ended before its fence closed.
    open: bool,
}

impl Block {
    /// The file it is: as named, or for a fence with no name, `SKILL.md`
    /// when it begins with front matter and `skill.toml` otherwise.
    fn part(&self) -> Option<Part> {
        match &self.named {
            Named::Part(part) => Some(*part),
            Named::Bare if self.body.trim_start().starts_with("---") => Some(Part::Card),
            Named::Bare => Some(Part::Toml),
            Named::Script(_) | Named::Other => None,
        }
    }
}

/// The answer's fenced blocks, in order, its `left out:` line, and the
/// fence that closes what it left open ([`open_fence`]).
struct Fenced {
    blocks: Vec<Block>,
    left_out: Option<String>,
    closing: Option<String>,
}

/// A fence line, indented or not: its mark (a backtick or a tilde), how
/// many (three or more), and its info string, trimmed.
fn fence(line: &str) -> Option<(u8, usize, &str)> {
    let line = line.trim();
    let mark = *line.as_bytes().first()?;
    if mark != b'`' && mark != b'~' {
        return None;
    }
    let run = line.bytes().take_while(|&b| b == mark).count();
    (run >= 3).then(|| (mark, run, line[run..].trim()))
}

/// A fence's name and whether it names a file whole (`skill.toml`,
/// `SKILL.md`) rather than its language (`toml`, `md`, `markdown`): its
/// info string's last word, in any case, a `title=` or quotes and a
/// trailing colon taken off.
fn fence_named(info: &str) -> (Named, bool) {
    let word = info.split_whitespace().last().unwrap_or_default();
    let word = word.rsplit_once('=').map_or(word, |(_, value)| value);
    let word = word
        .trim_matches(|c| matches!(c, '"' | '\'' | '*' | '`'))
        .trim_end_matches(':');
    // A file of its code (#6307), its path as written: held to its form
    // where it is read ([`files`]).
    if word.starts_with(SCRIPTS_FOLDER) {
        return (Named::Script(word.to_owned()), true);
    }
    let word = word.to_ascii_lowercase();
    match word.as_str() {
        "" => (Named::Bare, false),
        "skill.toml" => (Named::Part(Part::Toml), true),
        "skill.md" => (Named::Part(Part::Card), true),
        "toml" => (Named::Part(Part::Toml), false),
        "md" | "markdown" => (Named::Part(Part::Card), false),
        _ => (Named::Other, false),
    }
}

/// The answer read as fenced blocks. A block closes on a fence of its own
/// mark, at least as long, with no info string, or on one at the end of
/// its last line (`At ten it dims.```"`). A fence naming the other file
/// whole (`SKILL.md` in `skill.toml`), or any named fence in `skill.toml`,
/// closes it and opens that one; in `SKILL.md` another named fence opens a
/// block within it, closed by the next bare fence, kept as its words.
fn fenced(answer: &str) -> Fenced {
    let mut blocks = Vec::new();
    let mut left_out = None;
    // The block open now: its mark, its run, and how many blocks within
    // it are open.
    let mut open: Option<(Block, u8, usize, usize)> = None;
    let mut lines = answer.lines();
    let mut again = None;
    while let Some(line) = again.take().or_else(|| lines.next()) {
        let Some((mut block, mark, run, mut depth)) = open.take() else {
            match fence(line) {
                Some((mark, run, info)) => {
                    let block = Block {
                        named: fence_named(info).0,
                        body: String::new(),
                        open: true,
                    };
                    open = Some((block, mark, run, 0));
                }
                None => {
                    if let Some(what) = after_word(line.trim_start(), LEFT_OUT)
                        && !what.trim().is_empty()
                    {
                        left_out = Some(cut_chars(what, MOST_CANNOT_CHARS));
                    }
                }
            }
            continue;
        };
        let mut closed = false;
        match fence(line) {
            Some((_, _, "")) if depth > 0 => {
                depth -= 1;
                block.body.push_str(line);
                block.body.push('\n');
            }
            Some((this, length, "")) if this == mark && length >= run => closed = true,
            Some((_, _, info)) if !info.is_empty() && depth == 0 => {
                let (name, whole) = fence_named(info);
                // A file of its code always begins a block of its own, so
                // it is refused with its switch off, never kept as words.
                let other_file = (whole && matches!(name, Named::Part(_)) && name != block.named)
                    || matches!(name, Named::Script(_));
                if other_file || block.named == Named::Part(Part::Toml) {
                    closed = true;
                    again = Some(line);
                } else {
                    depth += 1;
                    block.body.push_str(line);
                    block.body.push('\n');
                }
            }
            Some((_, _, info)) if !info.is_empty() => {
                depth += 1;
                block.body.push_str(line);
                block.body.push('\n');
            }
            _ => {
                let end = String::from_utf8(vec![mark; run]).unwrap_or_default();
                match line.trim_end().strip_suffix(end.as_str()) {
                    Some(before) if depth == 0 && !before.trim().is_empty() => {
                        block.body.push_str(before);
                        block.body.push('\n');
                        closed = true;
                    }
                    _ => {
                        block.body.push_str(line);
                        block.body.push('\n');
                    }
                }
            }
        }
        if closed {
            block.open = false;
            blocks.push(block);
        } else {
            open = Some((block, mark, run, depth));
        }
    }
    let closing = match open {
        // A bare fence alone at the very end (a closing fence written
        // twice) opens nothing.
        Some((block, ..)) if block.named == Named::Bare && block.body.trim().is_empty() => None,
        Some((block, mark, run, depth)) => {
            blocks.push(block);
            let end = String::from_utf8(vec![mark; run]).unwrap_or_default();
            Some(vec![end; depth + 1].join("\n"))
        }
        None => None,
    };
    Fenced {
        blocks,
        left_out,
        closing,
    }
}

/// What a made skill asks to use: within [`capped`]'s bounds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Uses {
    pub light: bool,
    pub body: bool,
    pub topics: Vec<String>,
    /// Its voice: only a skill written in the app ([`Written`]), whose
    /// call's answer is the person's instructions for the model; one that
    /// reads a linked service to its owner ([`Uses::reads`]); or one whose
    /// one `say` step is its exact words, said as written (#6062,
    /// [`say_text`]).
    pub speak: bool,
    /// The linked service it reads to the owner alone, by its code, with
    /// [`READS_ENVIRONMENT`] on (#5288): never one it writes to.
    pub reads: Vec<String>,
    /// It may send a short text to the person's phone (`messages`): an
    /// outbound step, named when it is said back (#6066).
    pub messages: bool,
    /// It may see the camera's picture (`camera`, or format 1's `look`):
    /// named when it is said back (#6066).
    pub camera: bool,
    /// It may know whether a face is one the lamp knows (`known_faces`).
    pub known_faces: bool,
    /// Its note is told who is at the lamp (`who`).
    pub who: bool,
    /// The actions part's functions it may call by name (`actions`).
    pub actions: Vec<String>,
    /// It may set light and volume ceilings (`output_limits`).
    pub output_limits: bool,
    /// The linked services it writes to, by code, with
    /// [`WRITES_ENVIRONMENT`] on (#6066): each write waits for the
    /// person's yes.
    pub writes: Vec<String>,
    /// Where its writes go, every recipient or place, for the say-back and
    /// the app's list ("sends to …").
    pub sends_to: Vec<String>,
    /// It runs code of its own (#6307, with [`SCRIPTS_ENVIRONMENT`] on): a
    /// `task` step and its `scripts/`, put on only on the person's yes.
    pub code: bool,
    /// What its code may use on the lamp, its `grants` ([`GRANTS`]), each
    /// named when it is said back.
    pub grants: Vec<String>,
}

/// The topics that are what the microphone hears: words, its name, the
/// sounds' classes. A skill reading one is said back as listening (#6066).
const MICROPHONE_TOPICS: [&str; 3] = ["voice/heard", "name/heard", "ears/classes"];

impl Uses {
    /// In plain words: "its light and its head", "whether someone is at the
    /// desk".
    pub fn spoken(&self) -> String {
        let mut parts = Vec::new();
        if self.speak {
            parts.push("its voice".to_owned());
        }
        if !self.reads.is_empty() {
            let names: Vec<&str> = self.reads.iter().map(|code| service_name(code)).collect();
            parts.push(format!("reading your {}", names.join(" and ")));
        }
        if self.light {
            parts.push("its light".to_owned());
        }
        if self.body {
            parts.push("its head and arm".to_owned());
        }
        if self.camera {
            parts.push("the camera".to_owned());
        }
        if self.listens() {
            parts.push("the microphone".to_owned());
        }
        if self.messages {
            parts.push("messages to your phone".to_owned());
        }
        if !self.writes.is_empty() {
            let names: Vec<&str> = self.writes.iter().map(|code| service_name(code)).collect();
            parts.push(format!(
                "your {}, sending to {}",
                names.join(" and "),
                self.sends_to.join(", ")
            ));
        }
        if self.known_faces {
            parts.push("whether it knows a face".to_owned());
        }
        if self.who {
            parts.push("who is at the lamp".to_owned());
        }
        if self.output_limits {
            parts.push("limits on its light and volume".to_owned());
        }
        if self.code {
            parts.push(match grants_plainly(&self.grants) {
                Some(grants) => format!("code of its own that may use {grants}"),
                None => "code of its own".to_owned(),
            });
        }
        for action in &self.actions {
            parts.push(format!("its action {}", named(action)));
        }
        for topic in &self.topics {
            parts.push(
                match topic.as_str() {
                    "voice/state" => "whether it is talking",
                    "room/presence" => "whether someone is at the desk",
                    FACES_TOPIC => "whether it sees a face",
                    STOOD_UP_TOPIC => "someone getting up from the desk",
                    CLOCK_TOPIC => "the time of day",
                    other => other,
                }
                .to_owned(),
            );
        }
        match parts.as_slice() {
            [] => "nothing".to_owned(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        }
    }

    /// Whether it reads what the microphone hears ([`MICROPHONE_TOPICS`]).
    pub fn listens(&self) -> bool {
        self.topics
            .iter()
            .any(|topic| MICROPHONE_TOPICS.contains(&topic.as_str()))
    }

    fn json(&self) -> Value {
        let mut uses = json!({"light": self.light, "body": self.body, "topics": self.topics, "speak": self.speak});
        // One read as its code, as an entry kept before #6066 reads; more
        // as a list (Thirteen's P3 on #6103).
        match self.reads.as_slice() {
            [] => {}
            [service] => uses["reads"] = json!(service),
            services => uses["reads"] = json!(services),
        }
        // The grants beyond the light, the body and the voice, only when
        // given, so an entry kept before #6066 reads the same.
        for (key, given) in [
            ("messages", self.messages),
            ("camera", self.camera),
            ("known_faces", self.known_faces),
            ("who", self.who),
            ("output_limits", self.output_limits),
        ] {
            if given {
                uses[key] = json!(true);
            }
        }
        if !self.actions.is_empty() {
            uses["actions"] = json!(self.actions);
        }
        // Where it sends, for the app's list ("sends to …", #6066).
        if !self.writes.is_empty() {
            uses["writes"] = json!(self.writes);
            uses["sends_to"] = json!(self.sends_to);
        }
        // Its code and what it may use (#6307), only when it has code.
        if self.code {
            uses["code"] = json!(true);
            uses["grants"] = json!(self.grants);
        }
        uses
    }

    /// What [`Uses::json`] wrote, read back.
    fn of_json(uses: &Value) -> Self {
        let strings = |key: &str| -> Vec<String> {
            uses[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        };
        Self {
            light: uses["light"] == true,
            body: uses["body"] == true,
            topics: strings("topics"),
            speak: uses["speak"] == true,
            reads: match uses["reads"].as_str() {
                Some(service) => vec![service.to_owned()],
                None => strings("reads"),
            },
            messages: uses["messages"] == true,
            camera: uses["camera"] == true,
            known_faces: uses["known_faces"] == true,
            who: uses["who"] == true,
            actions: strings("actions"),
            output_limits: uses["output_limits"] == true,
            writes: strings("writes"),
            sends_to: strings("sends_to"),
            code: uses["code"] == true,
            grants: strings("grants"),
        }
    }
}

/// A made skill's code's grants in plain words ([`GRANTS`]), "the camera
/// and its light"; none for no grant. A name not in [`GRANTS`] is said as
/// its name, though [`capped`] never keeps one.
fn grants_plainly(grants: &[String]) -> Option<String> {
    let said: Vec<String> = grants
        .iter()
        .map(|grant| {
            if let Some((code, write)) = service_grant(grant) {
                let service = code.replace('_', " ");
                return if write {
                    format!("sending with your {service}, each send only on your yes")
                } else {
                    format!("reading your {service}")
                };
            }
            GRANTS
                .iter()
                .find(|(name, _)| name == grant)
                .map_or_else(|| named(grant).to_owned(), |(_, said)| (*said).to_owned())
        })
        .collect();
    (!said.is_empty()).then(|| and_list(&said))
}

/// What the say-back of a skill with code of its own says of it (#6307),
/// after its manners: who wrote its code and what it may use, then
/// [`RUNS_AFTER_YES`]. What must be heard whole before the person's yes is
/// taken (`crate::skill_write_yes`). Code that goes on at once
/// ([`grants_go_on_at_once`]) has no wait, so it ends on what it may use.
pub fn code_said(grants: &[String]) -> String {
    code_said_as(grants, grants_go_on_at_once(grants))
}

/// [`code_said`], `at_once` whether it goes on at once
/// ([`code_goes_on_at_once`]): then with no [`RUNS_AFTER_YES`].
pub fn code_said_as(grants: &[String], at_once: bool) -> String {
    let uses = grants_plainly(grants).unwrap_or_else(|| "nothing of the lamp's".to_owned());
    let waits = if at_once { "" } else { RUNS_AFTER_YES };
    format!(" The lamp's model wrote its code, and that code may use {uses}.{waits}")
}

/// A linked service a made skill may read, as the person hears it.
fn service_name(code: &str) -> &str {
    lamp_services::spoken::name(code)
}

/// The linked services a made skill reads that aren't linked now: for the
/// app's finished make, `[{"code", "name"}]` (its `needs_link`), and the
/// voice's [`needs_link_ask`]. `linked` gives the codes linked now
/// (`crate::services::linked_codes`), asked only for a skill that reads
/// one. Either Gmail code is linked by the other, as `lamp-services`
/// reads it. Empty when nothing is missing; `None` when it can't be told,
/// and nothing is said of it.
pub fn needs_link(uses: &Uses, linked: impl FnOnce() -> Option<Vec<String>>) -> Option<Vec<Value>> {
    if uses.reads.is_empty() {
        return Some(Vec::new());
    }
    let linked = linked()?;
    // Every read not linked now, each asked for (Thirteen's P3 on #6103).
    let needs = uses
        .reads
        .iter()
        .filter(|code| {
            let alike = |linked: &str| {
                linked == code.as_str()
                    || matches!(
                        (code.as_str(), linked),
                        ("gmail", "gmail-app") | ("gmail-app", "gmail")
                    )
            };
            !linked.iter().any(|linked| alike(linked))
        })
        .map(|code| json!({"code": code, "name": service_name(code)}))
        .collect();
    Some(needs)
}

/// What the voice's model is asked to tell of [`needs_link`]'s services,
/// in its own words and the conversation's language, never a fixed
/// English sentence the lamp says as it is (the owner, 10 Nov: "đồng bộ
/// luôn", as the `cannot:` reason is the model's own): that the skill
/// starts once they are linked in the app, under Skills, then Connectors.
/// The services themselves go with it as `needs_link`. None with none.
pub fn needs_link_ask(needs: &[Value]) -> Option<String> {
    let names: Vec<&str> = needs
        .iter()
        .filter_map(|need| need["name"].as_str())
        .collect();
    if names.is_empty() {
        return None;
    }
    let (names, are) = (
        names.join(" and "),
        if names.len() == 1 { "is" } else { "are" },
    );
    Some(format!(
        "Then tell the person, in your own words and in the language of the conversation, \
         that the skill will start once {names} {are} linked in the app, under Skills, then \
         Connectors (needs_link)."
    ))
}

/// A made skill's `skill.toml` taken as one may be written: installable,
/// with a version, and no function for the model (`tool`): it runs by
/// itself. Since #6066 (the first builder's rule: a skill a person makes on
/// their own lamp may do whatever the lamp can do) its `[permissions]` and
/// `[ask]` are whatever `lamp-skills` takes for an installable skill: the
/// light, the body, the voice, any topic, messages, the camera, known
/// faces, actions, and with [`READS_ENVIRONMENT`] on a linked service's
/// reads. What it may declare and do is `lamp-skills pack`'s check after,
/// which refuses a core grant, a write to a service or a step its runtime
/// doesn't have. Only #6062's say stays this cap's: one say step, its
/// words. What it does is then said back whole ([`plainly`]), any outbound
/// step, the camera and the microphone named.
pub fn capped(text: &str) -> Result<(String, Uses), Refused> {
    capped_with(text, reads_on())
}

/// [`capped`], with a made skill's reads on or off ([`READS_ENVIRONMENT`]).
pub fn capped_with(text: &str, reads: bool) -> Result<(String, Uses), Refused> {
    let table: toml::Table = toml::from_str(text).map_err(unreadable)?;
    let held = if table.contains_key("tool") {
        Err(Refused::new(
            "it would run only when called on, and a made skill runs by itself; try saying when \
             it should run, like 'when I come to my desk'",
        ))
    } else {
        held(&table, false, reads)
    };
    // Its lines go back to the model as the check's do (the owner, 10 Nov:
    // "re-run it yourself until it works"): the lamp's own reason, and
    // what a made skill may have.
    held.map_err(|Refused(why, lines)| {
        // What the lamp can't do yet is said, never repaired away.
        if why.starts_with(CANT_YET) {
            return Refused(why, None);
        }
        // A write's own repair lines (#6066), its check's words.
        if let Some(lines) = lines {
            return Refused(why, Some(lines));
        }
        let read = if reads { CAPPED_OR_READ } else { "" };
        let lines = format!("The lamp refused it: {why}. {CAPPED_ONLY}{read}");
        Refused(why, Some(lines))
    })
}

/// What a repair is told when [`capped`] refused.
const CAPPED_ONLY: &str = "kind = \"installable\", version = \"1.0.0\", no [tool]; \
                           [permissions] as its steps and trigger use them: light, body, speak \
                           (speak for a say step, cue = \"hum\" or a read), topics its \
                           trigger's, any other a skill may declare; a say is one \
                           say = { text = \"the exact words\" }, in the request's language";

/// The most of a made skill's say text ([`say_text`]): one or two short
/// sentences, "Welcome back!", said back whole within [`MOST_PLAIN_CHARS`].
pub const MOST_SAY_TEXT_CHARS: usize = 120;

/// The least cooldown of a made skill that speaks (#6062, the readers' P2a):
/// at most once an hour. [`paced`] sets it when it is missing or shorter.
pub const LEAST_SAY_COOLDOWN_SECS: u64 = 3_600;

/// When a made skill that speaks on the lamp's resting may run (#6062):
/// never at night, 22:00 to 07:00, when nobody may be there to hear it.
/// [`paced`] sets its `during` within these hours.
pub const SAY_HOURS: (&str, &str) = ("07:00", "22:00");

/// Why a say step that isn't its exact words is refused, the person's
/// reason and the repair's.
const SAY_NOT_TEXT: &str = "what it would say isn't its exact words; try saying the words \
                            themselves, like 'Welcome back!'";

/// A Unicode format character (general category Cf): zero-width marks,
/// bidirectional controls and the like (U+200B, U+202E), which
/// [`char::is_control`] doesn't cover (App Only's P2b on #6062).
fn is_format(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// A made skill's `say` step's words (#6062), as [`plainly`] says them: a
/// table with `text` alone, a string of at most [`MOST_SAY_TEXT_CHARS`],
/// one or two sentences, with no double quotes, braces or guillemets (so
/// it reads back quoted, whole), no web address, and no control or format
/// character. The voice says them as written, framed as quoted with no
/// functions (`say::Words::Made`), so they can't instruct anything. A bare
/// note, `{ note }`, a list or anything else is none, so refused.
pub fn say_text(say: &toml::Value) -> Option<String> {
    let table = say.as_table().filter(|table| table.len() == 1)?;
    let text = table.get("text")?.as_str()?.trim();
    let lower = text.to_lowercase();
    let sentences = text
        .split(['.', '!', '?'])
        .filter(|part| !part.trim().is_empty())
        .count();
    let plain = !text.chars().any(|c| {
        c.is_control()
            || is_format(c)
            || matches!(
                c,
                '"' | '“' | '”' | '«' | '»' | '{' | '}' | '`' | '\u{2028}' | '\u{2029}'
            )
    });
    let web = ["://", "www.", "http"]
        .iter()
        .any(|mark| lower.contains(mark));
    (plain && !web && (1..=2).contains(&sentences) && text.chars().count() <= MOST_SAY_TEXT_CHARS)
        .then(|| text.to_owned())
}

/// The least `absent_for` of a made skill on a face coming into view
/// (#6070, the Reviewer-lead's P2): a face lost for a frame and seen again
/// is the same person still there, never someone new to greet (CLAUDE.md
/// rule 8). [`paced`] sets it, `"1m"`, when it is missing or shorter,
/// whether or not the skill speaks.
pub const LEAST_FACE_ABSENT_SECS: u64 = 60;

/// The least cooldown of a made skill on someone getting up (#6070): the
/// eyes say a departure again each time the sitter's face is lost rising
/// after being seen, so someone bobbing out of the picture and back would
/// run it each time. [`paced`] sets it, `"1m"`, when it is missing or
/// shorter; one that speaks has [`LEAST_SAY_COOLDOWN_SECS`].
pub const LEAST_STOOD_UP_COOLDOWN_SECS: u64 = 60;

/// The least cooldown of a made skill on stuffy air (#6066): the CO2 level
/// hovers about its line as a window opens and shuts, so a skill on it
/// would run each crossing. [`paced`] sets it, `"1h"`, when it is missing or
/// shorter, as for a say; the built-in stuffy air waits for the air to
/// clear (its `rearm`), which a made skill doesn't write.
pub const LEAST_AIR_COOLDOWN_SECS: u64 = 3_600;

/// A trigger's cooldown in seconds, when it is one [`duration`].
fn cooldown_secs(trigger: &toml::Table) -> Option<u64> {
    duration_secs(trigger, "cooldown")
}

/// A trigger's `key` (`cooldown`, `absent_for`) in seconds, when it is one
/// [`duration`].
fn duration_secs(trigger: &toml::Table, key: &str) -> Option<u64> {
    let (count, unit) = duration(trigger.get(key)?.as_str()?)?;
    // Too long to count is unreadable, so it is paced like a missing one.
    count.checked_mul(match unit {
        "second" => 1,
        "minute" => 60,
        _ => 3_600,
    })
}

/// Whether a trigger's `during` lies within [`SAY_HOURS`], never across
/// midnight.
fn in_say_hours(during: &toml::Value) -> bool {
    let hour = |key: &str| {
        during
            .get(key)
            .and_then(|hour| hour.as_str())
            .and_then(minutes)
    };
    match (
        hour("from"),
        hour("to"),
        minutes(SAY_HOURS.0),
        minutes(SAY_HOURS.1),
    ) {
        (Some(from), Some(to), Some(day_from), Some(day_to)) => {
            day_from <= from && from < to && to <= day_to
        }
        _ => false,
    }
}

/// `"HH:MM"` in minutes from midnight.
fn minutes(hour: &str) -> Option<u32> {
    let (hours, mins) = hour.split_once(':')?;
    let (hours, mins): (u32, u32) = (hours.parse().ok()?, mins.parse().ok()?);
    (hours < 24 && mins < 60 && hour.len() == 5).then_some(hours * 60 + mins)
}

/// A trigger's `during` within [`SAY_HOURS`]: itself when it is, the part of
/// it within them when there is one (a window across midnight: its first
/// such part), the hours themselves when it has none; `None` when none of
/// it is (a window all night): a say then can't be made safe.
fn day_window(trigger: &toml::Table) -> Option<(u32, u32)> {
    let (day_from, day_to) = (minutes(SAY_HOURS.0)?, minutes(SAY_HOURS.1)?);
    let Some(during) = trigger.get("during") else {
        return Some((day_from, day_to));
    };
    let hour = |key: &str| {
        during
            .get(key)
            .and_then(|hour| hour.as_str())
            .and_then(minutes)
    };
    let (from, to) = (hour("from")?, hour("to")?);
    let parts = if from < to {
        vec![(from, to)]
    } else {
        vec![(from, 24 * 60), (0, to)]
    };
    parts
        .into_iter()
        .map(|(from, to)| (from.max(day_from), to.min(day_to)))
        .find(|(from, to)| from < to)
}

fn hhmm(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Whether a made skill's table has a `say` step, or its hum (`cue`),
/// paced as a say is.
fn says_aloud(table: &toml::Table) -> bool {
    table
        .get("do")
        .and_then(|steps| steps.as_array())
        .is_some_and(|steps| {
            steps
                .iter()
                .any(|step| step.get("say").is_some() || step.get("cue").is_some())
        })
}

/// A made skill's `skill.toml` with its manners set rather than refused
/// (#6062, #6066's rule (a), the owner's: lifting a limit makes things
/// simpler, never adds a refusal): a skill with a say step has a `cooldown`
/// of at least an hour ([`LEAST_SAY_COOLDOWN_SECS`]), set to `"1h"` when
/// missing, shorter or unreadable; one that speaks on the lamp's resting
/// (`voice/state`) runs only within [`SAY_HOURS`], its `during` set to them
/// or to its part within them. Whether or not it speaks (#6070), one on a
/// face coming into view runs `when = "appears"` (set when missing or
/// `"present"`) after seeing none for at least a minute, its `absent_for`
/// set to `"1m"` ([`LEAST_FACE_ABSENT_SECS`]) when missing, shorter or
/// unreadable; one on someone getting up has a `cooldown` of at least a
/// minute ([`LEAST_STOOD_UP_COOLDOWN_SECS`]). Its `[trigger]` table's lines
/// are changed in place, so what is checked, said plainly and packed is the
/// same text. Anything else, or a skill that can't be read, is given back
/// as it came. Then every skill that starts itself is given the runtime's
/// manners ([`mannered`], #6066 (a)): both the floors and the manners apply.
/// First the fields that can't apply are taken out ([`tidied`]), and the
/// topics its trigger and a look at a face read are put in its
/// `[permissions] topics` when the model left them out ([`topic_granted`]).
pub fn paced(text: &str) -> String {
    paced_asked(text, &[])
}

/// [`paced`], with the times the request named ([`asked_windows`]): a
/// `during` within one runs as said, with no quiet hours.
pub fn paced_asked(text: &str, asked: &[(u32, u32)]) -> String {
    paced_with(text, asked, false)
}

/// [`paced_asked`], with whether the request asked for limits in its own
/// words ([`limits_asked`]), which [`mannered_with`] reads with
/// [`LOOSE_ENVIRONMENT`] on.
pub fn paced_with(text: &str, asked: &[(u32, u32)], limits: bool) -> String {
    paced_for(text, asked, limits, None)
}

/// [`paced_with`], with the repeat the request asked for, in seconds
/// ([`repeat_asked`]): with [`LOOSE_ENVIRONMENT`] on, a skill asked to
/// repeat "every five minutes while I am at my desk" runs again while its
/// trigger holds, its `rearm` taken out and its `cooldown` the asked
/// interval, a minute at least ([`say_paced_with`]).
pub fn paced_for(text: &str, asked: &[(u32, u32)], limits: bool, repeat: Option<u64>) -> String {
    mannered_with(
        &say_paced_with(&topic_granted(&tidied(text)), repeat),
        asked,
        limits,
    )
}

/// The interval a request asks a skill to repeat at, in seconds, in the
/// person's own words: "every five minutes", "every 2 hours", "every
/// minute", "every half hour", "mỗi 10 phút", "once an hour while I'm
/// here". A unit is read after a number or a number word, or alone at a
/// phrase's end or before "while" or "when"; "every second" alone is no
/// repeat (a skill never runs every second), nor "every second time",
/// "every other time", "every few minutes", or "once an hour" without
/// "while" or "when" (a limit, [`limits_asked`]). `None` when it asks for
/// no repeat ("every time I sit down", "every day at 22:00"). Read with
/// [`LOOSE_ENVIRONMENT`] on alone ([`say_paced_with`]).
pub fn repeat_asked(request: &str) -> Option<u64> {
    const WORDS: [(&str, u64); 21] = [
        ("a", 1),
        ("an", 1),
        ("one", 1),
        ("two", 2),
        ("three", 3),
        ("four", 4),
        ("five", 5),
        ("six", 6),
        ("seven", 7),
        ("eight", 8),
        ("nine", 9),
        ("ten", 10),
        ("eleven", 11),
        ("twelve", 12),
        ("fifteen", 15),
        ("twenty", 20),
        ("thirty", 30),
        ("forty", 40),
        ("forty-five", 45),
        ("fifty", 50),
        ("sixty", 60),
    ];
    let unit = |word: &str| -> Option<u64> {
        match word {
            "second" | "seconds" | "sec" | "secs" | "giây" => Some(1),
            "minute" | "minutes" | "min" | "mins" | "phút" => Some(60),
            "hour" | "hours" | "hr" | "hrs" | "giờ" | "tiếng" => Some(3_600),
            _ => None,
        }
    };
    let count = |word: &str| -> Option<u64> {
        word.parse::<u64>()
            .ok()
            .filter(|count| *count > 0)
            .or_else(|| {
                WORDS
                    .iter()
                    .find(|(name, _)| *name == word)
                    .map(|(_, count)| *count)
            })
    };
    // A punctuation mark is a word of its own, ",", so a phrase's end is
    // read: "every hour, say stretch".
    let lower: String = request
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_punctuation() && c != '-' {
                " , ".to_owned()
            } else {
                c.to_string()
            }
        })
        .collect();
    let words: Vec<&str> = lower.split_whitespace().collect();
    // Where a bare unit ends its phrase: "every minute while I'm here".
    let ends = |word: &str| matches!(word, "" | "," | "while" | "when" | "khi");
    words.iter().enumerate().find_map(|(at, word)| {
        let next = |by: usize| words.get(at + by).copied().unwrap_or_default();
        // "once an hour while I'm at my desk"; "once an hour" alone is a
        // limit ([`limits_asked`]), never a repeat.
        if *word == "once" {
            return unit(next(2))
                .filter(|unit| *unit > 1)
                .filter(|_| matches!(next(1), "a" | "an"))
                .filter(|_| matches!(next(3), "while" | "when" | "khi"));
        }
        if !matches!(*word, "every" | "mỗi" | "cứ") {
            return None;
        }
        // "every minute", "mỗi tiếng"; never "every second", an ordinal
        // ("every second time") or a skill run each second.
        if let Some(unit) = unit(next(1)) {
            return (unit > 1 && ends(next(2))).then_some(unit);
        }
        // "every half hour", "every half an hour".
        if next(1) == "half" {
            return [next(2), next(3)]
                .into_iter()
                .any(|word| unit(word) == Some(3_600))
                .then_some(1_800);
        }
        // "every five minutes", "every 10 min", "mỗi 10 phút".
        count(next(1))?.checked_mul(unit(next(2))?)
    })
}

/// Whether a request asks, in its own words, for a limit on how often a
/// skill runs or when it keeps quiet ("at most twice a day", "not after
/// 10 pm", "no more than once an hour"): with [`LOOSE_ENVIRONMENT`] on, only
/// then are the lamp's manners set ([`mannered_with`]).
pub fn limits_asked(request: &str) -> bool {
    let request = format!(" {} ", request.to_lowercase());
    const LIMITS: [&str; 19] = [
        "at most",
        "no more than",
        "not more than",
        "only once",
        " limit",
        "maximum",
        " max ",
        "times a day",
        "times per day",
        "once a day",
        "twice a day",
        "a day at most",
        "per hour",
        "per day",
        "quiet hours",
        "not too often",
        "don't bother me",
        "rest if",
        "once an hour",
    ];
    LIMITS.iter().any(|limit| request.contains(limit))
}

/// A made skill's `skill.toml` with the fields that can't apply taken out,
/// as the lamp's own model wrote them on lamp-74ff, 11 Nov 09:48-09:50
/// ("nod when I stand up" failed all three repairs twice): a body step's
/// `for` beside a `gesture` with no `mood` (a gesture plays once; only a
/// mood is held, and `lamp-skills` refuses "a body mood goes with its for,
/// and a for with a mood"); a `for` or `absent_for` on a `room/stood_up`
/// trigger, one message a getting up that never lasts, which [`plainly`]
/// can't say, and on any trigger one of nothing ("0s"), which
/// `lamp-skills` reads as none and [`plainly`] can't say (lamp-9adf, 11
/// Nov 10:01: "say hello when I come back to my desk" refused on
/// "room/presence, its for"); and `speak = true` in `[permissions]` with
/// no say step, no hum and no read, which [`capped`] refuses ("it asks for
/// its voice but never says anything"); and a `when` `lamp-skills`
/// doesn't know on a `room/stood_up` or `controls/pet` trigger, one
/// message each, which none reads the same (lamp-0c89, 11 Nov 10:35: "nod
/// when I stand up" refused on "room/stood_up, its when"); and a blink's
/// gap, a light step that turns it `out` with a `for` beside a step that
/// lights it, taken out whole ([`blink_gap`], lamp-9adf, 11 Nov 20:5x).
/// Each only removes: the skill keeps every step,
/// trigger and grant it uses, and gains nothing. Each removal is a line or
/// an entry of a one-line table taken out, taken only when the text then
/// reads as the same skill without that one field; a skill written
/// otherwise is given back as it came, for the check to say why. One
/// journal line for each.
pub fn tidied(text: &str) -> String {
    let Ok(mut table) = toml::from_str::<toml::Table>(text) else {
        return text.to_owned();
    };
    let mut text = text.to_owned();
    fn tidy(
        text: &mut String,
        table: &mut toml::Table,
        parent: &dyn Fn(&mut toml::Table) -> Option<&mut toml::Table>,
        key: &str,
        what: String,
    ) {
        if let Some((tidied, expected)) = field_taken_out(text, table, parent, key) {
            eprintln!("lamp-voice: a made skill tidied: {what}");
            *text = tidied;
            *table = expected;
        }
    }
    // (a) A gesture's `for`, with no mood to hold.
    let steps = table
        .get("do")
        .and_then(toml::Value::as_array)
        .map_or(0, Vec::len);
    for index in 0..steps {
        let gesture_for = table["do"][index]
            .get("body")
            .and_then(toml::Value::as_table)
            .is_some_and(|body| {
                body.contains_key("gesture")
                    && body.contains_key("for")
                    && !body.contains_key("mood")
            });
        if gesture_for {
            tidy(
                &mut text,
                &mut table,
                &|table| {
                    table
                        .get_mut("do")?
                        .as_array_mut()?
                        .get_mut(index)?
                        .get_mut("body")?
                        .as_table_mut()
                },
                "for",
                format!("[[do]] step {}, a body's for beside its gesture", index + 1),
            );
        }
    }
    // (b) A getting up's `for` and `absent_for`, and on any trigger a
    // `for` or `absent_for` of nothing, which `lamp-skills` reads as none
    // (its `optional`: no duration and "0s" are both 0).
    for key in ["for", "absent_for"] {
        let Some(trigger) = table.get("trigger").and_then(toml::Value::as_table) else {
            break;
        };
        let stood_up = trigger.get("topic").and_then(toml::Value::as_str) == Some(STOOD_UP_TOPIC);
        let nothing = trigger
            .get(key)
            .and_then(toml::Value::as_str)
            .and_then(whole_ms)
            == Some(0);
        if trigger.contains_key(key) && (stood_up || nothing) {
            let topic = trigger
                .get("topic")
                .and_then(toml::Value::as_str)
                .map_or("another", named)
                .to_owned();
            let why = if stood_up { "" } else { " of nothing" };
            tidy(
                &mut text,
                &mut table,
                &|table| table.get_mut("trigger")?.as_table_mut(),
                key,
                format!("trigger topic {topic}, its {key}{why}"),
            );
        }
    }
    // (c) A `when` `lamp-skills` doesn't know on a getting up or a pat,
    // one message each: none is the same trigger, "present", which it
    // reads as every message (lamp-0c89, 11 Nov 10:35: "nod when I stand
    // up" refused on "room/stood_up, its when").
    if let Some(trigger) = table.get("trigger").and_then(toml::Value::as_table)
        && let Some(topic @ (STOOD_UP_TOPIC | PET_TOPIC)) =
            trigger.get("topic").and_then(toml::Value::as_str)
        && trigger.get("when").is_some_and(|when| {
            !when
                .as_str()
                .is_some_and(|when| KNOWN_WHENS.contains(&when))
        })
    {
        let what = format!("trigger topic {topic}, a when lamp-skills doesn't know");
        tidy(
            &mut text,
            &mut table,
            &|table| table.get_mut("trigger")?.as_table_mut(),
            "when",
            what,
        );
    }
    // (e) A blink's gap: a light step that turns it `out` with a `for`
    // beside a step that lights it (lamp-9adf, 11 Nov 20:5x: "blink the
    // light green twice when I arrive" failed all three repairs on "out
    // takes no colour, dim, level, for, pattern or no_brighter"). The gap
    // can't be played, so the whole step is taken out and the lit step
    // plays once; [`keep_asked`] says the blinking was left out.
    while let Some(index) = table
        .get("do")
        .and_then(toml::Value::as_array)
        .filter(|steps| steps.iter().any(lights))
        .and_then(|steps| steps.iter().position(blink_gap))
    {
        let Some((tidied, expected)) = step_taken_out(&text, &table, index) else {
            break;
        };
        eprintln!(
            "lamp-voice: a made skill tidied: [[do]] step {}, a blink's gap: light out with more \
             than over and until",
            index + 1
        );
        text = tidied;
        table = expected;
    }
    // (d) Its voice asked with nothing to say: no say step, no sound and
    // no read, as [`capped`] holds it.
    let steps = || {
        table
            .get("do")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
    };
    let speaks = steps().any(|step| {
        ["say", "cue", "sound"]
            .iter()
            .any(|key| step.get(key).is_some())
    });
    let permissions = table.get("permissions").and_then(toml::Value::as_table);
    let reads = permissions
        .and_then(|permissions| permissions.get("services"))
        .and_then(toml::Value::as_table)
        .is_some_and(|services| services.values().any(|how| how.as_str() == Some("read")));
    let voice = permissions
        .and_then(|permissions| permissions.get("speak"))
        .and_then(toml::Value::as_bool)
        == Some(true);
    if voice && !speaks && !reads {
        tidy(
            &mut text,
            &mut table,
            &|table| table.get_mut("permissions")?.as_table_mut(),
            "speak",
            "[permissions] speak, with no say step, hum or read".to_owned(),
        );
    }
    text
}

/// Whether a `[[do]]` step is a blink's gap ([`tidied`]): a step of a light
/// alone that turns it `out` with more beside it than `over` and `until`,
/// which `lamp-skills` refuses ("out takes ... only over and until").
fn blink_gap(step: &toml::Value) -> bool {
    step.as_table()
        .filter(|step| step.len() == 1)
        .and_then(|step| step.get("light"))
        .and_then(toml::Value::as_table)
        .is_some_and(|light| {
            light.get("out").and_then(toml::Value::as_bool) == Some(true)
                && light
                    .keys()
                    .any(|key| !matches!(key.as_str(), "out" | "over" | "until"))
        })
}

/// Whether a `[[do]]` step lights the light: a light step that isn't `out`.
fn lights(step: &toml::Value) -> bool {
    step.get("light")
        .and_then(toml::Value::as_table)
        .is_some_and(|light| light.get("out").and_then(toml::Value::as_bool) != Some(true))
}

/// Whether a made skill has a blink's gap beside a lit step, which
/// [`tidied`] takes out: told as left out, never dropped without a word.
fn blink_gapped(text: &str) -> bool {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return false;
    };
    table
        .get("do")
        .and_then(toml::Value::as_array)
        .is_some_and(|steps| steps.iter().any(lights) && steps.iter().any(blink_gap))
}

/// `text` with its `index`th `[[do]]` step taken out, and the table it then
/// reads as, if that step is written under a `[[do]]` header of its own:
/// the header and every line to the next header but its own `[do.…]`
/// tables, or to the end ([`tidied`]'s blink gap, and a whole step
/// [`loosened`] leaves out). Any other form, or a cut that reads
/// as anything else, is `None`.
fn step_taken_out(text: &str, table: &toml::Table, index: usize) -> Option<(String, toml::Table)> {
    let mut expected = table.clone();
    let steps = expected.get_mut("do")?.as_array_mut()?;
    if index >= steps.len() {
        return None;
    }
    steps.remove(index);
    let (mut start, mut seen, mut from, mut to) = (0, 0, None, text.len());
    for line in text.split_inclusive('\n') {
        let head = line.trim_start();
        if head.starts_with('[') && !head.starts_with("[do.") {
            if from.is_some() {
                to = start;
                break;
            }
            let rest = head.strip_prefix("[[do]]").map(str::trim_start);
            if rest.is_some_and(|rest| rest.is_empty() || rest.starts_with('#')) {
                if seen == index {
                    from = Some(start);
                }
                seen += 1;
            }
        }
        start += line.len();
    }
    let from = from?;
    let tidied = if to == text.len() {
        // The last step: the blank lines before it go with it.
        format!("{}\n", text[..from].trim_end())
    } else {
        format!("{}{}", &text[..from], &text[to..])
    };
    toml::from_str::<toml::Table>(&tidied)
        .is_ok_and(|read| read == expected)
        .then_some((tidied, expected))
}

/// `text` with `parent`'s `key` taken out, and the table it then reads as,
/// if one line or one entry of a one-line table taken out makes it read as
/// `table` without that field and nothing else ([`tidied`]).
fn field_taken_out(
    text: &str,
    table: &toml::Table,
    parent: &dyn Fn(&mut toml::Table) -> Option<&mut toml::Table>,
    key: &str,
) -> Option<(String, toml::Table)> {
    let mut expected = table.clone();
    parent(&mut expected)?.remove(key)?;
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        for (from, to) in entries_of(line, key) {
            let tidied = format!("{}{}", &text[..start + from], &text[start + to..]);
            if toml::from_str::<toml::Table>(&tidied).is_ok_and(|read| read == expected) {
                return Some((tidied, expected));
            }
        }
        start += line.len();
    }
    None
}

/// Where in `line` each `key = value` entry is, as the span to take out to
/// drop it: the whole line when the entry is the line, else the entry with
/// one comma beside it, in a one-line table. Strings are skipped whole, and
/// a value's own tables and lists with it; what this misreads, the read
/// back in [`field_taken_out`] refuses.
fn entries_of(line: &str, key: &str) -> Vec<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut spans = Vec::new();
    for (at, _) in line.match_indices(key) {
        let before = line[..at].trim_end();
        if !(before.is_empty() || before.ends_with('{') || before.ends_with(',')) {
            continue;
        }
        let after = &line[at + key.len()..];
        let Some(value) = after.trim_start().strip_prefix('=') else {
            continue;
        };
        let value_at = line.len() - value.len();
        let (mut depth, mut quote, mut escaped) = (0_i32, None::<u8>, false);
        let mut end = line.len();
        for (offset, &byte) in bytes[value_at..].iter().enumerate() {
            if let Some(open) = quote {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' && open == b'"' {
                    escaped = true;
                } else if byte == open {
                    quote = None;
                }
                continue;
            }
            match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'{' | b'[' => depth += 1,
                b'}' | b']' if depth > 0 => depth -= 1,
                b',' | b'}' | b'#' | b'\n' | b'\r' if depth == 0 => {
                    end = value_at + offset;
                    break;
                }
                _ => {}
            }
        }
        let rest = &line[end..];
        if before.is_empty() && (rest.trim().is_empty() || rest.trim_start().starts_with('#')) {
            // The entry is the line.
            spans.push((0, line.len()));
        } else if let Some(after_comma) = rest.strip_prefix(',') {
            // Its comma after it, and the space before the next.
            let next = after_comma.len() - after_comma.trim_start_matches([' ', '\t']).len();
            spans.push((at, end + 1 + next));
        } else if rest.starts_with('}') && before.ends_with(',') {
            // The last entry: the comma before it.
            let value_end = line[..end].trim_end().len();
            spans.push((before.len() - 1, value_end));
        }
    }
    spans
}

/// A made skill's `skill.toml` with the topics its own parts read in its
/// `[permissions] topics`, added when the model left them out: its
/// `[trigger]`'s topic (lamp-9adf, 11 Nov 05:36: "dims the light at 10 pm"
/// written on `clock/local` with no such grant, which `lamp-skills`
/// refuses: a skill reads only the topics it is given), and the faces in
/// view ([`FACES_TOPIC`]) for a step's look at a face, `body = { look_at =
/// "face" }` (lamp-9adf, 11 Nov 17:0x: "when someone sits down, look at
/// them and say hi" failed all three repairs on "a look at a face reads
/// humans/faces/tracked"). Its trigger and its look read those topics
/// whatever the grant says, so adding them gives the skill nothing its
/// parts didn't already name, and the say-back says them ([`plainly`]: the
/// trigger, and "turns to the face it sees"). No other grant is added: a
/// look still needs `body = true`, a real grant the person hears, which
/// the check asks the model for. Added as a line in `[permissions]`, the
/// topics written last in a one-line `topics` or as one of its own, so
/// what is checked, said and packed is one text; a skill written otherwise
/// (its grants inline or dotted, its topics over several lines), or one
/// that then doesn't read as the same skill with those topics more, is
/// given back as it came, and the check says why.
pub fn topic_granted(text: &str) -> String {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return text.to_owned();
    };
    let trigger = table
        .get("trigger")
        .and_then(|trigger| trigger.get("topic"))
        .and_then(toml::Value::as_str);
    // A look at a face, as `lamp-skills` reads one: a `[[do]]` step's
    // `body` with `look_at = "face"`.
    let looks = table
        .get("do")
        .and_then(toml::Value::as_array)
        .is_some_and(|steps| {
            steps.iter().any(|step| {
                step.get("body")
                    .and_then(|body| body.get("look_at"))
                    .and_then(toml::Value::as_str)
                    == Some("face")
            })
        });
    let permissions = table.get("permissions");
    let granted: Vec<toml::Value> = match permissions.map(|granted| granted.get("topics")) {
        None | Some(None) => Vec::new(),
        Some(Some(toml::Value::Array(topics))) => topics.clone(),
        Some(Some(_)) => return text.to_owned(),
    };
    let mut topics = granted;
    let mut added = false;
    for topic in trigger.into_iter().chain(looks.then_some(FACES_TOPIC)) {
        if !topics.iter().any(|given| given.as_str() == Some(topic)) {
            topics.push(toml::Value::String(topic.to_owned()));
            added = true;
        }
    }
    if !added {
        return text.to_owned();
    }
    // Each a basic string, as JSON writes one: the round trip below holds
    // the text to the same topics.
    let Some(quoted) = topics
        .iter()
        .map(|topic| topic.as_str().map(|topic| Value::from(topic).to_string()))
        .collect::<Option<Vec<_>>>()
    else {
        return text.to_owned();
    };
    let line = format!("topics = [{}]", quoted.join(", "));
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    match lines.iter().position(|line| line.trim() == "[permissions]") {
        Some(head) => {
            let end = lines[head + 1..]
                .iter()
                .position(|line| line.trim_start().starts_with('['))
                .map_or(lines.len(), |at| head + 1 + at);
            let key_of = |line: &str| line.split_once('=').map(|(key, _)| key.trim().to_owned());
            match (head + 1..end).find(|&at| key_of(&lines[at]).as_deref() == Some("topics")) {
                Some(at) => lines[at] = line,
                None => lines.insert(head + 1, line),
            }
        }
        None if permissions.is_none() => {
            let Some(trigger) = lines.iter().position(|line| line.trim() == "[trigger]") else {
                return text.to_owned();
            };
            lines.splice(trigger..trigger, [format!("[permissions]\n{line}\n")]);
        }
        None => return text.to_owned(),
    }
    let mut granted_text = lines.join("\n");
    if text.ends_with('\n') {
        granted_text.push('\n');
    }
    // Only the same skill with that one topic more is taken.
    let mut expected = table.clone();
    if let toml::Value::Table(permissions) = expected
        .entry("permissions")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
    {
        permissions.insert("topics".to_owned(), toml::Value::Array(topics));
    }
    match toml::from_str::<toml::Table>(&granted_text) {
        Ok(granted) if granted == expected => granted_text,
        _ => text.to_owned(),
    }
}

/// The manners a made skill that starts itself is given by the lamp, never
/// refused (#6066 (a), the first builder's review, 20:14): the built-in
/// nudges' own (`skills/sip-of-water`): at most [`MANNERS_MOST_A_DAY`] runs a
/// day, never in [`MANNERS_QUIET`]'s hours, one an hour in the nudges'
/// `[budget]` group ([`MANNERS_GROUP`]) and a `[rest]` until morning after
/// [`MANNERS_RESTS_AFTER`] runs nobody noticed.
pub const MANNERS_MOST_A_DAY: u32 = 3;
/// The quiet hours a made skill is given: the nudges' night.
pub const MANNERS_QUIET: (&str, &str) = ("22:00", "08:00");
/// The `[budget]` group a made skill joins, one run an hour with it: the
/// wellbeing nudges', so a made skill never adds to how often the lamp
/// speaks up unasked.
pub const MANNERS_GROUP: &str = "wellbeing";
/// Ignored this many runs in a row, a made skill rests until morning.
pub const MANNERS_RESTS_AFTER: u32 = 2;
/// The runs an hour a made skill's [`MANNERS_GROUP`] `[budget]` gives it
/// when it names none: one, the nudges' own.
pub const MANNERS_PER_HOUR: u32 = 1;

/// A made skill that starts itself (a `[trigger]` table, no `tool`) with the
/// runtime's manners set where it has none of its own (#6066 (a)):
/// `most_a_day`, `[quiet]` (left out when its trigger names a time of its
/// own, `during`, which is the person's ask), `[budget]` and `[rest]`. One
/// it has, the person's ask for more, is kept, and [`plainly`] says it.
/// Added as lines, so what is checked, said and packed is one text; a skill
/// that can't be read, or doesn't start itself, is given back as it came.
pub fn mannered(text: &str) -> String {
    mannered_asked(text, &[])
}

/// [`mannered`], with the times the request named ([`keeps_quiet`]).
pub fn mannered_asked(text: &str, asked: &[(u32, u32)]) -> String {
    mannered_with(text, asked, false)
}

/// [`mannered_asked`], with whether the request asked for limits
/// ([`limits_asked`]). With [`LOOSE_ENVIRONMENT`] on and no limit asked, a
/// made skill is given none the person didn't ask for: no `most_a_day` or
/// shared `[budget]` (any the model wrote alone taken out), and `[quiet]`
/// only when it says something or hums ([`says_aloud`]), as
/// [`keeps_quiet`] holds it; its `[rest]` stays, so a skill ignored twice
/// rests (the first builder's yes on #6059, 22:44). Asked for, or off, the
/// manners are set as before.
pub fn mannered_with(text: &str, asked: &[(u32, u32)], limits: bool) -> String {
    let set_limits = limits || !loose_on();
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return text.to_owned();
    };
    let Some(trigger) = table.get("trigger").and_then(toml::Value::as_table) else {
        return text.to_owned();
    };
    if table.contains_key("tool") {
        return text.to_owned();
    }
    // Each manner the lamp's, bounded (Thirteen's and App Only's P2 on
    // #6103): what the skill wrote is clamped, never refused, so the
    // model can't choose a skill that speaks up more than the nudges do.
    let integer = |key: &str, of: Option<&toml::Value>| {
        of.and_then(|table| table.get(key))
            .and_then(toml::Value::as_integer)
    };
    let most_a_day = table
        .get("most_a_day")
        .and_then(toml::Value::as_integer)
        .unwrap_or(MANNERS_MOST_A_DAY.into())
        .clamp(1, MANNERS_MOST_A_DAY_AT_MOST.into());
    let per_hour = integer("per_hour", table.get("budget"))
        .unwrap_or(MANNERS_PER_HOUR.into())
        .clamp(1, MANNERS_PER_HOUR_AT_MOST.into());
    let after_ignored = integer("after_ignored", table.get("rest"))
        .unwrap_or(MANNERS_RESTS_AFTER.into())
        .clamp(1, MANNERS_RESTS_AFTER_AT_MOST.into());
    // The night stays quiet unless the skill runs only within the day, or
    // at a time the person asked for ([`within_day`]).
    // Loosened with no limit asked, only a skill that speaks keeps the
    // night quiet ([`LOOSE_ENVIRONMENT`]).
    let quiet = keeps_quiet(trigger, asked) && (set_limits || says_aloud(&table));
    // A grant no step uses is dropped (App Only's P2 on #6103): no step of
    // a made skill sends a message or takes the camera's picture, so
    // neither is kept, and the say-back names only what a step does.
    let unused = ["messages", "camera", "look"];
    let mut kept = Vec::new();
    let mut part: Option<String> = None;
    // Inside a multi-line string a line is text, never a header or a key
    // (Thirteen's shape 3 on #6103: a description line `[rest]`).
    let mut in_text = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let quotes = line.matches("\"\"\"").count() + line.matches("'''").count();
        if in_text {
            in_text = quotes % 2 == 0;
            let manner = matches!(part.as_deref(), Some("quiet" | "budget" | "rest"));
            if !manner {
                kept.push(line);
            }
            continue;
        }
        if trimmed.starts_with('[') {
            part = Some(
                trimmed
                    .trim_matches(|c| c == '[' || c == ']')
                    .trim()
                    .to_owned(),
            );
        }
        let key = trimmed
            .split_once('=')
            .map(|(key, _)| key.trim().trim_matches('"'));
        // A dotted key (`quiet.from`, `permissions.messages`), by its parts
        // (Thirteen's shape 2).
        let (head, rest) = match key {
            Some(key) => match key.split_once('.') {
                Some((head, rest)) => (Some(head.trim().trim_matches('"')), Some(rest.trim())),
                None => (Some(key), None),
            },
            None => (None, None),
        };
        let dropped = match (part.as_deref(), head, rest) {
            (Some("quiet" | "budget" | "rest"), _, _) => true,
            (None, Some("most_a_day" | "quiet" | "budget" | "rest"), _) => true,
            (None, Some("permissions"), Some(grant)) => unused.contains(&grant.trim_matches('"')),
            (Some("permissions"), Some(key), _) => unused.contains(&key),
            _ => false,
        };
        in_text = quotes % 2 == 1;
        if !dropped {
            kept.push(line);
        }
    }
    let mut mannered = if set_limits {
        format!(
            "most_a_day = {most_a_day}\n{}\n",
            kept.join("\n").trim_end()
        )
    } else {
        format!("{}\n", kept.join("\n").trim_end())
    };
    if quiet {
        let (from, to) = MANNERS_QUIET;
        mannered.push_str(&format!("\n[quiet]\nfrom = \"{from}\"\nto = \"{to}\"\n"));
    }
    if set_limits {
        mannered.push_str(&format!(
            "\n[budget]\ngroup = \"{MANNERS_GROUP}\"\nper_hour = {per_hour}\n"
        ));
    }
    // A skill ignored twice rests till morning, loosened too (the first
    // builder's yes on #6059, 22:44: `[rest]` stays).
    mannered.push_str(&format!("\n[rest]\nafter_ignored = {after_ignored}\n"));
    // Taken when it reads as the same skill with its manners as set; a
    // skill written otherwise (a manner inline) is given back as it came,
    // and a manner past its bound is then unsaid ([`manners_plainly`]), so
    // the repair writes it plainly.
    let unasked = |set: &toml::Table| {
        !["most_a_day", "budget"]
            .iter()
            .any(|key| set.contains_key(*key))
    };
    let as_set = |set: &toml::Table| {
        if !set_limits {
            return unasked(set)
                && integer("after_ignored", set.get("rest")) == Some(after_ignored)
                && set.contains_key("quiet") == quiet
                && !set
                    .get("permissions")
                    .and_then(toml::Value::as_table)
                    .is_some_and(|granted| unused.iter().any(|key| granted.contains_key(*key)));
        }
        set.get("most_a_day").and_then(toml::Value::as_integer) == Some(most_a_day)
            && integer("per_hour", set.get("budget")) == Some(per_hour)
            && set
                .get("budget")
                .and_then(|budget| budget.get("group"))
                .and_then(toml::Value::as_str)
                == Some(MANNERS_GROUP)
            && integer("after_ignored", set.get("rest")) == Some(after_ignored)
            && set.contains_key("quiet") == quiet
            && !set
                .get("permissions")
                .and_then(toml::Value::as_table)
                .is_some_and(|granted| unused.iter().any(|key| granted.contains_key(*key)))
    };
    match toml::from_str::<toml::Table>(&mannered) {
        Ok(set) if as_set(&set) => mannered,
        _ => text.to_owned(),
    }
}

/// Whether a made skill carries the lamp's manners as [`mannered`] sets
/// them (Thirteen's and App Only's P2 on #6103, option 2): one that starts
/// itself (a `[trigger]`, no `tool`) has all four at the lamp's values,
/// `most_a_day` 1 to 6, `[budget]` the nudges' group with 1 or 2 an hour,
/// `[rest]` after 1 to 3, and `[quiet]` 22:00-08:00 unless its time runs as
/// said ([`within_day`]), and no grant no step uses (`messages`, `camera`,
/// `look`). Whatever shape the model wrote, inline or dotted, the table is
/// what is read. Anything else is true.
fn manners_kept(table: &toml::Table, asked: &[(u32, u32)]) -> bool {
    let Some(trigger) = table.get("trigger").and_then(toml::Value::as_table) else {
        return true;
    };
    if table.contains_key("tool") {
        return true;
    }
    let only = |part: &str, keys: &[&str]| {
        table
            .get(part)
            .and_then(toml::Value::as_table)
            .is_some_and(|part| part.keys().all(|key| keys.contains(&key.as_str())))
    };
    let integer = |part: &str, key: &str| {
        table
            .get(part)
            .and_then(|part| part.get(key))
            .and_then(toml::Value::as_integer)
    };
    let text = |part: &str, key: &str| {
        table
            .get(part)
            .and_then(|part| part.get(key))
            .and_then(toml::Value::as_str)
    };
    let in_range = |value: Option<i64>, most: u32| {
        value.is_some_and(|value| (1..=i64::from(most)).contains(&value))
    };
    let night = only("quiet", &["from", "to"])
        && text("quiet", "from") == Some(MANNERS_QUIET.0)
        && text("quiet", "to") == Some(MANNERS_QUIET.1);
    let quiet_kept = match keeps_quiet(trigger, asked) {
        false => !table.contains_key("quiet") || night,
        true => night,
    };
    let granted = table
        .get("permissions")
        .and_then(toml::Value::as_table)
        .is_some_and(|granted| {
            ["messages", "camera", "look"]
                .iter()
                .any(|key| granted.contains_key(*key))
        });
    in_range(
        table.get("most_a_day").and_then(toml::Value::as_integer),
        MANNERS_MOST_A_DAY_AT_MOST,
    ) && only("budget", &["group", "per_hour"])
        && text("budget", "group") == Some(MANNERS_GROUP)
        && in_range(integer("budget", "per_hour"), MANNERS_PER_HOUR_AT_MOST)
        && only("rest", &["after_ignored"])
        && in_range(
            integer("rest", "after_ignored"),
            MANNERS_RESTS_AFTER_AT_MOST,
        )
        && quiet_kept
        && !granted
}

/// What [`keep`] says of a paced skill: [`plainly`], once its manners are
/// as the lamp sets them ([`manners_kept`]); a skill the lamp couldn't
/// set them on is never kept unmannered, but unsaid ("parts manners"), so
/// the repair writes it plainly.
fn said_mannered(text: &str, asked: &[(u32, u32)]) -> Result<String, Refused> {
    let table: toml::Table = toml::from_str(text).map_err(unreadable)?;
    // Loosened, the manners are the person's ([`mannered_with`]): whatever
    // it carries is said, never refused ([`LOOSE_ENVIRONMENT`]).
    if !loose_on() && !manners_kept(&table, asked) {
        let unsaid = Unsaid::of("parts manners".to_owned());
        eprintln!(
            "lamp-voice: a made skill can't be said plainly: {}",
            unsaid.part
        );
        return Err(Refused(UNSAID.into(), Some(unsaid.repair(reads_on()))));
    }
    plainly_as(text, reads_on(), loose_on())
}

/// The most runs a day a made skill may ask for (Thirteen's bound on #6103).
pub const MANNERS_MOST_A_DAY_AT_MOST: u32 = 6;
/// The most runs an hour its group may give it.
pub const MANNERS_PER_HOUR_AT_MOST: u32 = 2;
/// The most runs nobody noticed before it rests.
pub const MANNERS_RESTS_AFTER_AT_MOST: u32 = 3;

/// The day a made skill may run in with no quiet hours: 08:00 to 22:00,
/// [`MANNERS_QUIET`]'s other side.
const DAY: (u32, u32) = (8 * 60, 22 * 60);

/// The longest `during` that runs at the night's hours with no quiet hours
/// over it: a time asked for in the person's own words ([`asked_windows`],
/// "dim the light at ten tonight"), at most 3 hours long. A window the
/// model chose alone, or a wider one, keeps the night quiet (Thirteen's
/// HOLD follow-up on #6103, 21:51).
const ASKED_TIME_MINUTES: u32 = 3 * 60;

/// How long after a single time the person named a window may run: "at
/// ten" is 22:00 to 23:00.
const ASKED_AT_MINUTES: u32 = 60;

/// Minutes in a day.
const DAY_MINUTES: u32 = 24 * 60;

/// A trigger's `during`, as minutes of the day, from and to.
fn during_minutes(during: &toml::Value) -> Option<(u32, u32)> {
    let at = |key: &str| {
        let (hours, minutes) = during.get(key)?.as_str()?.split_once(':')?;
        let (hours, minutes): (u32, u32) = (hours.parse().ok()?, minutes.parse().ok()?);
        (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
    };
    Some((at("from")?, at("to")?))
}

/// Whether a trigger's `during` lies wholly within [`DAY`], never across
/// midnight: it needs no quiet hours.
fn within_day(during: &toml::Value) -> bool {
    matches!(during_minutes(during), Some((from, to)) if DAY.0 <= from && from < to && to <= DAY.1)
}

/// Whether a trigger's `during` runs at a time the person asked for in
/// their own words: at most [`ASKED_TIME_MINUTES`] long and wholly within
/// one of `asked` ([`asked_windows`]), across midnight too.
fn runs_as_asked(during: &toml::Value, asked: &[(u32, u32)]) -> bool {
    let Some((from, to)) = during_minutes(during) else {
        return false;
    };
    let after = |at: u32, start: u32| (at + DAY_MINUTES - start) % DAY_MINUTES;
    let long = after(to, from);
    long > 0
        && long <= ASKED_TIME_MINUTES
        && asked.iter().any(|&(start, end)| {
            let (at_from, at_to, window) =
                (after(from, start), after(to, start), after(end, start));
            at_from < at_to && at_to <= window
        })
}

/// Whether a made skill keeps the night quiet: unless its `during` lies
/// within the day, or runs at a time the person asked for.
fn keeps_quiet(trigger: &toml::Table, asked: &[(u32, u32)]) -> bool {
    !trigger
        .get("during")
        .is_some_and(|during| within_day(during) || runs_as_asked(during, asked))
}

/// The times of day a request names in the person's own words, as windows
/// of minutes (Thirteen's HOLD follow-up on #6103): "from 22:00 to 22:30"
/// is that window; a single time, "at 10 pm", "at ten tonight", "lúc 10 giờ
/// tối", is it and the hour after ([`ASKED_AT_MINUTES`]). A bare hour with
/// no morning or evening, "at ten", is either. Only these let a made
/// skill's `during` run at night without quiet hours ([`mannered`]).
pub fn asked_windows(request: &str) -> Vec<(u32, u32)> {
    const WORDS: [&str; 12] = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
        "twelve",
    ];
    const LATER: [&str; 10] = [
        "pm",
        "p.m.",
        "p.m",
        "tonight",
        "evening",
        "night",
        "tối",
        "đêm",
        "chiều",
        "afternoon",
    ];
    const EARLIER: [&str; 5] = ["am", "a.m.", "a.m", "morning", "sáng"];
    let lower = request.to_lowercase();
    let tokens: Vec<&str> = lower
        .split(|c: char| c.is_whitespace() || ",;!?()".contains(c))
        .filter(|token| !token.is_empty())
        .collect();
    // Each time named, with where it was, as the minutes it may be.
    let mut times: Vec<(usize, Vec<u32>)> = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let token = token.trim_end_matches('.');
        // "22:00", "10:30pm", "10pm", "10h", "ten".
        let (digits, suffix) = token.split_at(
            token
                .find(|c: char| !(c.is_ascii_digit() || c == ':'))
                .unwrap_or(token.len()),
        );
        let (hour, minute) = match digits.split_once(':') {
            Some((hour, minute)) => (hour.parse::<u32>().ok(), minute.parse::<u32>().ok()),
            None => (digits.parse::<u32>().ok(), Some(0)),
        };
        let (hour, minute, suffix) = match (hour, minute) {
            (Some(hour), Some(minute)) if !digits.is_empty() => (hour, minute, suffix),
            _ => match WORDS.iter().position(|word| *word == token) {
                Some(index) => (index as u32 + 1, 0, ""),
                None => continue,
            },
        };
        if hour > 23 || minute > 59 || !matches!(suffix, "" | "pm" | "am" | "h" | "giờ") {
            continue;
        }
        let near: Vec<&str> = std::iter::once(suffix)
            .chain(tokens.iter().skip(at + 1).take(3).copied())
            .collect();
        let later = near.iter().any(|word| LATER.contains(word));
        let earlier = near.iter().any(|word| EARLIER.contains(word));
        let clock = digits.contains(':') || hour > 12;
        let hours: Vec<u32> = match (clock, later, earlier) {
            (true, _, _) if hour > 12 || !later => vec![hour],
            (_, true, _) => vec![if hour < 12 { hour + 12 } else { hour }],
            (_, _, true) => vec![if hour == 12 { 0 } else { hour }],
            // A bare hour, said as a time ("at ten"): morning or evening.
            _ if at > 0
                && ["at", "lúc", "from", "to", "until", "till", "đến"]
                    .contains(&tokens[at - 1]) =>
            {
                vec![hour % 12, hour % 12 + 12]
            }
            _ => continue,
        };
        times.push((
            at,
            hours.into_iter().map(|hour| hour * 60 + minute).collect(),
        ));
    }
    let mut windows = Vec::new();
    let mut used = vec![false; times.len()];
    for index in 0..times.len() {
        if used[index] {
            continue;
        }
        let (at, starts) = &times[index];
        // "from A to B": the next time named within a few words.
        let until = times.get(index + 1).filter(|(next, _)| {
            next - at <= 4
                && tokens[at + 1..*next]
                    .iter()
                    .any(|word| ["to", "until", "till", "đến", "-"].contains(word))
        });
        match until {
            Some((_, ends)) => {
                used[index + 1] = true;
                for &start in starts {
                    for &end in ends {
                        windows.push((start, end));
                    }
                }
            }
            None => {
                for &start in starts {
                    windows.push((start, (start + ASKED_AT_MINUTES) % DAY_MINUTES));
                }
            }
        }
    }
    windows
}

/// [`paced`]'s limits on speaking (#6062).
#[cfg(test)]
fn say_paced(text: &str) -> String {
    say_paced_with(text, None)
}

/// A cooldown of `secs` as a skill writes it: `"2h"`, `"5m"`, `"90s"`.
fn cooldown_line(secs: u64) -> String {
    if secs.is_multiple_of(3_600) {
        format!("cooldown = \"{}h\"", secs / 3_600)
    } else if secs.is_multiple_of(60) {
        format!("cooldown = \"{}m\"", secs / 60)
    } else {
        format!("cooldown = \"{secs}s\"")
    }
}

/// [`say_paced`], with the repeat the request asked for
/// ([`repeat_asked`]), read with [`LOOSE_ENVIRONMENT`] on alone. On (the
/// first builder's ruling on the lamp-9adf makes of 12 Nov 15:13-15:16):
///
/// - a skill that neither speaks nor hums has a cooldown of at least a
///   minute ([`LEAST_LOOSE_COOLDOWN_SECS`]): a missing or shorter one is
///   raised to it, and a longer one the model wrote is kept, never lowered
///   ("the first time I come in each morning" keeps its `"12h"`); the air
///   keeps its hour, a time of day runs once at its minute, and a touch on
///   the lamp (any `controls/…` topic, a pat on the head too) answers each
///   deliberate touch, so none of them is changed;
/// - a repeat asked ("every five minutes while I am at my desk") runs
///   while its trigger holds: its `rearm` is taken out and its `cooldown`
///   is the asked interval, a minute at least (the air's an hour); never
///   on a time of day or a face coming into view, which run once each time;
/// - with no repeat asked, a held state with no `rearm` ("when I'm at my
///   desk") gains `rearm = "ends"`: once each time it holds.
///
/// Its `[rest]` stays either way ([`mannered_with`]).
fn say_paced_with(text: &str, repeat: Option<u64>) -> String {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return text.to_owned();
    };
    let Some(trigger) = table.get("trigger").and_then(|trigger| trigger.as_table()) else {
        return text.to_owned();
    };
    let topic = trigger.get("topic").and_then(|topic| topic.as_str());
    let says = says_aloud(&table);
    let loose = loose_on();
    let mut set: Vec<(&str, String)> = Vec::new();
    let mut taken: Vec<&str> = Vec::new();
    let repeat =
        repeat.filter(|_| loose && ![Some(CLOCK_TOPIC), Some(FACES_TOPIC)].contains(&topic));
    // The air keeps its hour whatever it does: its level hovers about its
    // line. A say elsewhere, with [`LOOSE_ENVIRONMENT`] on, as often as
    // asked, at most once a minute; and so anything else on, but a time of
    // day, which runs once at its minute, and a touch, which answers each
    // deliberate touch.
    let touch = topic.is_some_and(|topic| topic.starts_with("controls/"));
    let least = if says && loose && topic != Some(AIR_TOPIC) {
        Some((LEAST_LOOSE_SAY_COOLDOWN_SECS, "1m"))
    } else if says {
        Some((LEAST_SAY_COOLDOWN_SECS, "1h"))
    } else if topic == Some(AIR_TOPIC) {
        Some((LEAST_AIR_COOLDOWN_SECS, "1h"))
    } else if topic == Some(STOOD_UP_TOPIC) {
        Some((LEAST_STOOD_UP_COOLDOWN_SECS, "1m"))
    } else if loose && topic != Some(CLOCK_TOPIC) && !touch {
        Some((LEAST_LOOSE_COOLDOWN_SECS, "1m"))
    } else {
        None
    };
    if let Some(every) = repeat {
        let least = if topic == Some(AIR_TOPIC) {
            LEAST_AIR_COOLDOWN_SECS
        } else {
            LEAST_LOOSE_COOLDOWN_SECS
        };
        let every = every.max(least);
        if cooldown_secs(trigger) != Some(every) {
            set.push(("cooldown", cooldown_line(every)));
        }
        if trigger.contains_key("rearm") {
            taken.push("rearm");
        }
    } else if let Some((least, line)) = least
        && cooldown_secs(trigger).is_none_or(|secs| secs < least)
    {
        // Only ever raised to the floor: a longer cooldown the model wrote
        // is the person's ("the first time each morning", `"12h"`).
        set.push(("cooldown", format!("cooldown = \"{line}\"")));
    }
    // A held state ("when I'm at my desk", `room/presence` someone true)
    // with no `rearm` and no repeat asked would run on each message while
    // it holds, or each minute with the floor: once each time it holds
    // (`rearm = "ends"`, the first builder's ruling for "when I'm
    // present"). Never on a time of day, a face or a getting up, each run
    // once already, nor the air, which keeps its hour.
    if loose
        && repeat.is_none()
        && !trigger.contains_key("rearm")
        && trigger.get("when").and_then(toml::Value::as_str) != Some("appears")
        && topic.is_some_and(|topic| {
            ![CLOCK_TOPIC, FACES_TOPIC, STOOD_UP_TOPIC, AIR_TOPIC].contains(&topic)
        })
    {
        set.push(("rearm", "rearm = \"ends\"".to_owned()));
    }
    // A face coming into view: a frame without one and the face again is
    // still the one there, never a new arrival (#6070, CLAUDE.md rule 8).
    // `lamp-skills` counts `absent_for` only with `when = "appears"`.
    if topic == Some(FACES_TOPIC) {
        if matches!(
            trigger.get("when").map(toml::Value::as_str),
            None | Some(Some("present"))
        ) {
            set.push(("when", "when = \"appears\"".to_owned()));
        }
        if duration_secs(trigger, "absent_for").is_none_or(|secs| secs < LEAST_FACE_ABSENT_SECS) {
            set.push(("absent_for", "absent_for = \"1m\"".to_owned()));
        }
    }
    if says
        && topic == Some("voice/state")
        && let Some((from, to)) = day_window(trigger)
    {
        let line = format!(
            "during = {{ from = \"{}\", to = \"{}\" }}",
            hhmm(from),
            hhmm(to)
        );
        let same = trigger.get("during").is_some_and(|during| {
            during.get("from").and_then(|v| v.as_str()) == Some(hhmm(from).as_str())
                && during.get("to").and_then(|v| v.as_str()) == Some(hhmm(to).as_str())
        });
        if !same {
            set.push(("during", line));
        }
    }
    if set.is_empty() && taken.is_empty() {
        return text.to_owned();
    }
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let Some(head) = lines.iter().position(|line| line.trim() == "[trigger]") else {
        return text.to_owned();
    };
    let end_of = |lines: &[String]| {
        lines[head + 1..]
            .iter()
            .position(|line| line.trim_start().starts_with('['))
            .map_or(lines.len(), |at| head + 1 + at)
    };
    let key_of = |line: &str| line.split_once('=').map(|(key, _)| key.trim().to_owned());
    // A key taken out (a repeat's `rearm`), its one line, before any is set.
    for key in taken {
        let end = end_of(&lines);
        if let Some(at) = (head + 1..end).find(|&at| key_of(&lines[at]).as_deref() == Some(key)) {
            lines.remove(at);
        }
    }
    let end = end_of(&lines);
    for (key, line) in set.into_iter().rev() {
        match (head + 1..end).find(|&at| key_of(&lines[at]).as_deref() == Some(key)) {
            Some(at) => lines[at] = line,
            None => lines.insert(head + 1, line),
        }
    }
    let mut paced = lines.join("\n");
    if text.ends_with('\n') {
        paced.push('\n');
    }
    // Only a change that still reads as the same skill is taken.
    match toml::from_str::<toml::Table>(&paced) {
        Ok(_) => paced,
        Err(_) => text.to_owned(),
    }
}

/// The sounds a made skill may play, by name, each as [`plainly`] says it:
/// only what the lamp ships. No skill sound ships as a file (`sounds/`), so
/// a made skill carries no audio and names no path: its one sound is the
/// voice's built-in cue, `cue = "hum"`, which `lamp-skills` takes in a
/// skill without a tool and the voice makes at its fixed quiet level.
const SOUNDS: [(&str, &str); 1] = [("hum", "hums softly")];

/// Why a sound step the lamp doesn't have is refused: another cue, a
/// `sound` file, a path.
const SOUND_NOT_SHIPPED: &str = "I can't do that yet: a sound other than its soft hum";

/// How the lamp answers what it can't do yet (#6066, the first builder's rule):
/// "I can't do that yet:" and what, never a refusal by a rule. Each one is
/// build work.
pub const CANT_YET: &str = "I can't do that yet:";

/// The `cannot:` sentences the making model's system prompt itself asks
/// for (`make_skill_local`'s `system_with`, `services`, `write_rules` and
/// `event_times`), kept here beside the brief so they can't drift (a test
/// holds each to the prompt): a DeepSeek `cannot:` with one of them is said
/// as it is and never handed to the lamp's agent (`LAMP_MAKE_SKILL_AGENT`,
/// the Reviewer-lead on #6439).
pub const BRIEFS_OWN_REFUSALS: &[&str] = &[
    "hear what is said",
    "know who someone is",
    "sending a message",
    "Ask me for a reminder instead",
    "sending what you didn't say word for word",
    "adding an event with no day and time",
    "adding a calendar event while my clock isn't set",
    "You can ask me for it yourself any time",
    SENDS_ON_ITS_OWN,
    "Not possible yet: a skill can't",
    "deleting or changing what is in your services",
];

/// Whether the model's `cannot:` reason `why` is one of the brief's own
/// refusals ([`BRIEFS_OWN_REFUSALS`]), in any case, a verb's "-ing" said
/// as the prompt says it ("hearing what is said" for "hear what is said").
pub fn briefs_own_refusal(why: &str) -> bool {
    let plain = |text: &str| {
        text.to_lowercase()
            .replace("hearing ", "hear ")
            .replace("knowing ", "know ")
            .replace("sending ", "send ")
    };
    let why = plain(why);
    BRIEFS_OWN_REFUSALS
        .iter()
        .any(|refusal| why.contains(&plain(refusal)))
}

/// [`CANT_YET`] with what: "I can't do that yet: sending a message".
pub fn cant_yet(what: &str) -> String {
    format!("{CANT_YET} {what}")
}

/// What a repair is told when a hum sits beside a say or another hum:
/// `lamp-skills` plays one line or sound a run, so the extra hum goes.
const HUM_DROPPED: &str = "the lamp plays one line or hum a run, so leave out the extra cue = \
                           \"hum\" step, keeping its say or one hum";

/// A `cue` step's sound in plain words ([`SOUNDS`]), or none.
fn sound_plainly(cue: &toml::Value) -> Option<&'static str> {
    let name = cue.as_str()?;
    SOUNDS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, said)| *said)
}

/// What [`CAPPED_ONLY`] goes on to say with reads on.
const CAPPED_OR_READ: &str = "; or, for a read of one linked service, the [permissions] your \
                              instructions give for it";

/// What a skill added from a file says of itself in its `SKILL.md`, beside
/// `made: here` (`crate::upload_skill`): the manage helper holds it to
/// [`capped_added`] and never lets it take another skill's place.
pub const ADDED_FROM_FILE: &str = "added: file";

/// What the lamp says of a skill added from a file before what it does:
/// never said as the Store's.
pub const ADDED_LABEL: &str = "Added from a file, not from the Skills Store";

/// A skill added from a file (`crate::upload_skill`, `LAMP_SKILL_UPLOAD`)
/// held to the made skills' cap, with two differences: it never starts on
/// its own (no trigger, no `[ask]`), so it must have a function for the
/// model (`tool`), the only way it runs; and the model may say its note
/// (`speak`), its own answer in its own turn, as any tool's is.
pub fn capped_added(text: &str) -> Result<(String, Uses), Refused> {
    let table: toml::Table =
        toml::from_str(text).map_err(|_| Refused::new("its skill.toml can't be read"))?;
    if table.contains_key("trigger") || table.contains_key("while") {
        return Err(Refused::new(
            "it starts on its own, which a skill added from a file can't: it runs only when asked",
        ));
    }
    if table.contains_key("ask") {
        return Err(Refused::new(
            "it asks to be started by itself, which a skill added from a file never is",
        ));
    }
    if !table.get("tool").is_some_and(toml::Value::is_str) {
        return Err(Refused::new(
            "it has no tool to be asked for by, so it could never run",
        ));
    }
    held(&table, true, false)
}

/// What a skill made or added here is told of a `do.task` step or `grants`
/// (#6305): a script's task comes only from the Store or the owner's own
/// computer for now.
pub const SCRIPT_NOT_YET: &str = "a skill made on the lamp can't hand a task to a script yet";

/// Whether a `skill.toml`'s table has `grants` or a `[[do]]` step with a
/// `task` (`lamp_skills::task`, #6305).
fn carries_task(table: &toml::Table) -> bool {
    table.contains_key("grants")
        || table
            .get("do")
            .and_then(toml::Value::as_array)
            .is_some_and(|steps| {
                steps.iter().any(|step| {
                    step.as_table()
                        .is_some_and(|step| step.contains_key("task"))
                })
            })
}

/// [`capped`]'s check of a table, past its tool: `speak` taken only for a
/// skill added from a file ([`capped_added`]), or with `reads` on for one
/// that reads a linked service.
fn held(table: &toml::Table, added: bool, reads: bool) -> Result<(String, Uses), Refused> {
    match table.get("kind").map(|kind| kind.as_str()) {
        None | Some(Some("installable")) => {}
        _ => {
            return Err(Refused::new(
                "it isn't a kind of skill the lamp can make (only an installable one)",
            ));
        }
    }
    let Some(version) = table.get("version").and_then(|version| version.as_str()) else {
        return Err(Refused::new("it's missing its version number"));
    };
    // A script's task and its grants (#6305) only come from the Store or the
    // owner's own computer: a skill added here carries none, and one made
    // here only with its own switch on (#6307, [`code_held`] below).
    if carries_task(table) && added {
        return Err(Refused::new(SCRIPT_NOT_YET));
    }
    // A skill added from a file keeps the cap from before #6066 (App
    // Only's P2 on #6103): the lift is for a skill made on the lamp only.
    if added {
        return added_held(table, version);
    }
    // Its `[ask]`, as `lamp-skills` takes any (#6066): a nudge, a hello,
    // news or a reminder, each said back ([`plainly`]).
    let mut uses = Uses::default();
    if let Some(permissions) = table.get("permissions") {
        let Some(permissions) = permissions.as_table() else {
            return Err(Refused::new("the lamp couldn't tell what it would use"));
        };
        // Format 1's `look` gives the picture too, as `camera` does
        // (`lamp_skills::Permissions::sees_picture`).
        let format_2 = table.get("format").and_then(toml::Value::as_integer) == Some(2);
        let strings = |list: &toml::Value| -> Option<Vec<String>> {
            list.as_array()?
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect()
        };
        let unclear = || Refused::new("the lamp couldn't tell what it would use");
        for (key, value) in permissions {
            let on = value.as_bool();
            match key.as_str() {
                "light" => uses.light = on.ok_or_else(unclear)?,
                "body" => uses.body = on.ok_or_else(unclear)?,
                "topics" => {
                    for topic in strings(value).ok_or_else(unclear)? {
                        if !uses.topics.contains(&topic) {
                            uses.topics.push(topic);
                        }
                    }
                }
                "actions" => uses.actions = strings(value).ok_or_else(unclear)?,
                "speak" if added => {}
                // Its voice: a read of a linked service said aloud
                // (#5288), or its own `say` step (#6055), held below to a
                // skill that has one.
                "speak" => uses.speak = on.ok_or_else(unclear)?,
                "messages" => uses.messages = on.ok_or_else(unclear)?,
                "camera" => uses.camera |= on.ok_or_else(unclear)?,
                "look" => uses.camera |= on.ok_or_else(unclear)? && !format_2,
                "known_faces" => uses.known_faces = on.ok_or_else(unclear)?,
                "who" => uses.who = on.ok_or_else(unclear)?,
                "output_limits" => uses.output_limits = on.ok_or_else(unclear)?,
                // A linked service: a read (#5288), or with its switch a
                // write (#6066), as lamp-skills' check takes them. Off, the
                // lamp can't yet.
                "services" => {
                    let services = value.as_table().ok_or_else(unclear)?;
                    for (code, how) in services {
                        match how.as_str() {
                            Some("read") if reads => uses.reads.push(code.clone()),
                            Some("read") => {
                                return Err(Refused::new(cant_yet("reading your linked services")));
                            }
                            Some("write") if !writes_on() => {
                                return Err(Refused::new(cant_yet(SENDS_ON_ITS_OWN)));
                            }
                            Some("write") if !lamp_services::skill_writes::writable(code) => {
                                return Err(Refused::new(cant_yet(&format!(
                                    "writing to your {} on its own",
                                    service_name(code)
                                ))));
                            }
                            Some("write") => uses.writes.push(code.clone()),
                            _ => {
                                return Err(Refused::new(cant_yet("writing to a linked service")));
                            }
                        }
                    }
                }
                // Any other key is lamp-skills' check's to take or refuse:
                // a core grant, an add-on's, or a word it doesn't know.
                _ => {}
            }
        }
    }
    // Its voice only for what it reads to the owner, or for its own say
    // step (#6062): its exact words, said as written with no functions
    // ([`say_text`]), never a note, a list or what the voice keeps.
    if !added {
        let steps = || {
            table
                .get("do")
                .and_then(|steps| steps.as_array())
                .into_iter()
                .flatten()
        };
        let says: Vec<&toml::Value> = steps().filter_map(|step| step.get("say")).collect();
        if says.iter().any(|say| say_text(say).is_none()) {
            return Err(Refused::new(SAY_NOT_TEXT));
        }
        // Code of its own (#6307): a `task` step and its grants, only with
        // their switch; off, what the lamp can't do yet.
        let tasks: Vec<&toml::Value> = steps().filter_map(|step| step.get("task")).collect();
        if !tasks.is_empty() || table.contains_key("grants") {
            if !scripts_on() {
                return Err(Refused::new(cant_yet(CODE_OF_ITS_OWN)));
            }
            uses.grants = code_held(table, &tasks, &uses)?;
            uses.code = true;
        }
        // A step the lamp can't do yet in a made skill, said so before the
        // check refuses it (#6066): a send or a write, a move of its own, a
        // sound other than the hum ([`SOUNDS`]: a made skill carries no
        // audio).
        if let Some(what) = steps().find_map(step_cant_yet) {
            return Err(Refused::new(match what {
                OTHER_SOUND => SOUND_NOT_SHIPPED.to_owned(),
                what => cant_yet(what),
            }));
        }
        // A write (#6066): fixed whole in the file, one of the kinds
        // lamp-services sends, its service granted as a write; where it
        // goes kept for the say-back and the app's list. Refused for a
        // repair with the check's own words, never said or journaled with
        // a value.
        for write in steps().filter_map(|step| step.get("write")) {
            let content = write_content(write).ok_or_else(|| {
                Refused(
                    WRITE_NOT_FIXED.into(),
                    Some(format!(
                        "The lamp refused it: {WRITE_NOT_FIXED}. {WRITE_ONLY}"
                    )),
                )
            })?;
            if let Err(why) = lamp_services::skill_writes::check(&content) {
                return Err(Refused(
                    WRITE_NOT_FIXED.into(),
                    Some(format!("The lamp refused it: {why}. {WRITE_ONLY}")),
                ));
            }
            let code = content["service"].as_str().unwrap_or_default();
            if !uses.writes.iter().any(|granted| granted == code) {
                return Err(Refused(
                    WRITE_NOT_FIXED.into(),
                    Some(format!(
                        "The lamp refused it: its write needs [permissions] services = {{ {code} = \"write\" }}. {WRITE_ONLY}"
                    )),
                ));
            }
            for to in write_reaches(&content) {
                if !uses.sends_to.contains(&to) {
                    uses.sends_to.push(to);
                }
            }
        }
        let cues: Vec<&toml::Value> = steps().filter_map(|step| step.get("cue")).collect();
        // One say or sound a run, as `lamp-skills` takes it.
        if says.len() > 1 {
            return Err(Refused::new(
                "it would say more than one line, and a made skill says one short line",
            ));
        }
        // A hum beside its line, or a second hum: the repair drops it
        // quietly, the line kept (the owner: lifting limits makes it
        // simpler for the person, never a scolding).
        if says.len() + cues.len() > 1 {
            return Err(Refused::new(HUM_DROPPED));
        }
        if (!says.is_empty() || !cues.is_empty()) && !uses.speak {
            return Err(Refused::new(
                "it would say something without asking for its voice (speak = true)",
            ));
        }
        if uses.speak && says.is_empty() && cues.is_empty() && uses.reads.is_empty() {
            return Err(Refused::new(
                "it asks for its voice but never says anything; a made skill asks for its voice \
                 only with a say step or its hum",
            ));
        }
        // Paced as [`paced`] sets it (#6062): at most once an hour, and on
        // the lamp's resting never at night. Only a skill not paced so,
        // such as one whose window is all night, gets here.
        let trigger = table.get("trigger").and_then(|trigger| trigger.as_table());
        if (!says.is_empty() || !cues.is_empty())
            && let Some(trigger) = trigger
        {
            // A minute with [`LOOSE_ENVIRONMENT`] on: as often as asked.
            let least = least_say_cooldown_secs();
            if cooldown_secs(trigger).is_none_or(|secs| secs < least) {
                return Err(Refused::new(if least < LEAST_SAY_COOLDOWN_SECS {
                    "it would speak more than once a minute; a made skill that speaks has \
                     cooldown = \"1m\" or longer"
                } else {
                    "it would speak more than once an hour; a made skill that speaks has \
                     cooldown = \"1h\" or longer"
                }));
            }
            let resting =
                trigger.get("topic").and_then(|topic| topic.as_str()) == Some("voice/state");
            if resting && !trigger.get("during").is_some_and(in_say_hours) {
                return Err(Refused::new(
                    "it would speak when the lamp is resting at night, maybe to an empty room; \
                     drop its say step, or have it run between 07:00 and 22:00",
                ));
            }
        }
    }
    Ok((version.to_owned(), uses))
}

/// A made skill's code as its `skill.toml` names it (#6307, with
/// [`SCRIPTS_ENVIRONMENT`] on): exactly one `[[do]]` step that is a `task`
/// alone, `task = { task = "what it does", run = "scripts/main.py" }`
/// (#6305's `do.task`), its words plain and at most [`MOST_TASK_CHARS`],
/// its `run` a Python file under [`SCRIPTS_FOLDER`]; and a `grants` list of
/// [`GRANTS`] (none when it has no list), each once. No write beside it:
/// the person's yes is to its code, not to a send. Its grants, or why not.
fn code_held(
    table: &toml::Table,
    tasks: &[&toml::Value],
    uses: &Uses,
) -> Result<Vec<String>, Refused> {
    if !uses.writes.is_empty() {
        return Err(Refused::new(cant_yet(
            "running code of its own and sending on its own in one skill",
        )));
    }
    let [task] = tasks else {
        return Err(scripts_not_held(if tasks.is_empty() {
            "grants with no task step to run its code"
        } else {
            "more than one task step"
        }));
    };
    let alone = table
        .get("do")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|step| step.get("task").is_some())
        .all(|step| step.as_table().is_some_and(|step| step.len() == 1));
    if !alone {
        return Err(scripts_not_held("a task step with another kind beside it"));
    }
    let Some(task) = task.as_table() else {
        return Err(scripts_not_held("a task step that isn't a table"));
    };
    if task.keys().any(|key| key != "task" && key != "run") {
        return Err(scripts_not_held(
            "a task step with a key other than task and run",
        ));
    }
    let words = task.get("task").and_then(toml::Value::as_str).unwrap_or("");
    let plain = !words.trim().is_empty()
        && words.chars().count() <= MOST_TASK_CHARS
        && words.chars().all(|c| !c.is_control() && !is_format(c));
    if !plain {
        return Err(scripts_not_held(
            "a task's words, empty, too long or not plain",
        ));
    }
    let runs = task
        .get("run")
        .and_then(toml::Value::as_str)
        .is_some_and(|run| script_path_ok(run) && run.ends_with(RUNNABLE_ENDING));
    if !runs {
        return Err(scripts_not_held(
            "a task's run, not a .py file under scripts/",
        ));
    }
    let mut grants: Vec<String> = Vec::new();
    if let Some(list) = table.get("grants") {
        let Some(list) = list.as_array() else {
            return Err(scripts_not_held("grants that aren't a list"));
        };
        for grant in list {
            let Some(grant) = grant.as_str() else {
                return Err(scripts_not_held("a grant that isn't a name"));
            };
            if grant == SKILLS_GRANT.0 {
                return Err(Refused::new(cant_yet(SKILLS_GRANT.1)));
            }
            // A linked service's grant, only for a service task for the
            // lamp's agent (`LAMP_AGENT_SERVICES`, #6432's ruling).
            let service = agent_services_here() && service_grant(grant).is_some();
            if !service && !GRANTS.iter().any(|(name, _)| *name == grant) {
                eprintln!(
                    "lamp-voice: a made skill's code asks for a grant the lamp doesn't have: {}",
                    named(grant)
                );
                let names: Vec<&str> = GRANTS.iter().map(|(name, _)| *name).collect();
                return Err(Refused(
                    SCRIPTS_NOT_HELD.into(),
                    Some(format!(
                        "The lamp refused its code: a grant it doesn't have. Grants are only {}. \
                         {SCRIPTS_ONLY}",
                        names.join(", ")
                    )),
                ));
            }
            if !grants.iter().any(|kept| kept == grant) {
                grants.push(grant.to_owned());
            }
        }
    }
    Ok(grants)
}

/// The `run` of a made skill's `task` step: the file its code starts at.
pub fn task_run(toml: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(toml).ok()?;
    table
        .get("do")?
        .as_array()?
        .iter()
        .find_map(|step| step.get("task")?.get("run")?.as_str().map(str::to_owned))
}

/// A made skill's code held to its `skill.toml` (#6307): code only with a
/// `task` step, a `task` step only with code, and its `run` one of the
/// files written.
fn code_matches(files: &Files, uses: &Uses) -> Result<(), Refused> {
    match (uses.code, files.scripts.is_empty()) {
        (false, true) => Ok(()),
        (false, false) => Err(scripts_not_held("code with no task step to run it")),
        (true, true) => Err(scripts_not_held("a task step with no code written")),
        (true, false) => {
            let run = task_run(&files.toml).unwrap_or_default();
            if files.scripts.iter().any(|(path, _)| *path == run) {
                Ok(())
            } else {
                Err(scripts_not_held("a task's run that names no file written"))
            }
        }
    }
}

/// The Python a made skill's scripts are parsed with as it is made (#6749
/// E): the lamp's own. A script is never run here: [`PARSES`] reads it from
/// stdin alone and hands it to Python's own parser.
pub const PYTHON: &str = "/usr/bin/python3";
/// What [`PYTHON`] runs, with `-I` (no environment, user site or working
/// folder on its path): the bytes on its stdin parsed, never compiled to
/// run, nor run. Bytes, not text, so the parse never hangs on the
/// locale's encoding: Python reads a script's source as UTF-8.
const PARSES: &str = "import ast,sys; ast.parse(sys.stdin.buffer.read())";
/// The longest one script's parse is waited for: Python starts and parses
/// 8 KiB in well under it on the lamp.
const PARSE_WITHIN: Duration = Duration::from_secs(2);
/// How often [`parsed`] looks whether Python has ended.
const PARSE_POLL: Duration = Duration::from_millis(5);
/// The most of Python's words read after a parse that failed.
const MOST_PARSE_WORDS: u64 = 4 * 1024;
/// The most of a parse error's own words told the repair, in characters.
const MOST_PARSE_ERROR_CHARS: usize = 80;
/// The most of the script's line told the repair, in characters.
const MOST_PARSE_LINE_CHARS: usize = 80;
// A script is written to Python's stdin whole before its end is awaited:
// within any pipe's buffer, so the write never waits on Python.
const _: () = assert!(MOST_SCRIPT_BYTES <= 16 * 1024);

/// Why a made skill is refused when a script it wrote isn't Python that
/// parses (#6749 E), the lamp's reason; the person hears [`NOT_MADE_TRY`]
/// once its repairs are used up, and the repair is told where it broke.
pub const SCRIPT_DOESNT_PARSE: &str = "its code isn't Python that parses";

/// How a made skill's code reaches the lamp, told the making model
/// (`make_skill_local`) and a repair of code that didn't parse: the lamp's
/// own `lampos` SDK on the lamp (`sdk/python`), as a run's script imports
/// it, its calls named as they are, so no module or keyword is guessed
/// (lamp-9adf, 13 Nov: `from lamp import light` and
/// `light.set_light(..., for="59s")`).
pub const SCRIPT_SDK: &str = "import asyncio; from lampos import Lamp; async def main(): lamp = \
     await Lamp.local(), then only await lamp.device() (a dict; temperature_c maps each sensor to \
     degrees C; grant device), await lamp.air() (air), await lamp.light(colour, level, for_ms) (a \
     light step's colour, level 1-5, for_ms in ms; light), await lamp.say(text) (speak), await \
     lamp.gesture(name) (body), await lamp.close(); last, asyncio.run(main())";

#[cfg(test)]
thread_local! {
    /// [`PYTHON`] as a test of this thread sets it.
    static PYTHON_HERE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// The Python [`scripts_parse`] parses with: [`PYTHON`], or a test's.
fn python() -> PathBuf {
    #[cfg(test)]
    if let Some(python) = PYTHON_HERE.with(|here| here.borrow().clone()) {
        return python;
    }
    PathBuf::from(PYTHON)
}

/// [`python`] for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_python_here(python: Option<PathBuf>) {
    PYTHON_HERE.with(|here| *here.borrow_mut() = python);
}

/// A Python this machine runs, set for this test's thread ([`python`]):
/// [`PYTHON`] when it runs, else `python3` on the PATH; none when neither
/// does, and the test that needs one says it was skipped.
#[cfg(test)]
pub(crate) fn a_python_here() -> Option<PathBuf> {
    let found = [PathBuf::from(PYTHON), PathBuf::from("python3")]
        .into_iter()
        .find(|python| {
            Command::new(python)
                .args(["-I", "-c", "pass"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        });
    set_python_here(found.clone());
    found
}

/// What [`parsed`] found of one script.
#[derive(Debug)]
enum Parse {
    /// Python's parser took it.
    Parses,
    /// Python's parser refused it: the line it named, and its error ("SyntaxError:
    /// invalid syntax").
    Not { line: Option<usize>, error: String },
    /// Python couldn't tell: not started, not ended within its time, or
    /// ended with no parse error. Why, for the journal.
    Unchecked(String),
}

/// `text` parsed by `python` ([`PARSES`]), on its stdin alone, with an
/// empty environment from `/`, waited for at most `within`, then killed.
fn parsed(python: &Path, text: &str, within: Duration) -> Parse {
    let child = Command::new(python)
        .args(["-I", "-c", PARSES])
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(why) => return Parse::Unchecked(format!("python not started: {why}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A Python that ended before reading it is told by its end below.
        let _ = stdin.write_all(text.as_bytes());
    }
    let began = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if began.elapsed() < within => std::thread::sleep(PARSE_POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Parse::Unchecked(format!("python not ended within {within:?}"));
            }
        }
    };
    if status.success() {
        return Parse::Parses;
    }
    let mut words = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        let _ = stderr.take(MOST_PARSE_WORDS).read_to_end(&mut words);
    }
    let words = String::from_utf8_lossy(&words);
    // Python's own traceback names the parsed source `<unknown>`, and its
    // last line is the error: anything else is Python failing, not the
    // script.
    let error = words
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .filter(|line| {
            line.split_once(':')
                .map_or(*line, |(kind, _)| kind)
                .ends_with("Error")
        });
    let Some(error) = error else {
        return Parse::Unchecked(format!("python ended {status} with no parse error"));
    };
    let line = words
        .lines()
        .filter_map(|line| line.trim().strip_prefix("File \"<unknown>\", line "))
        .filter_map(|rest| {
            rest.split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        })
        .next_back();
    Parse::Not {
        line,
        error: cut_chars(error, MOST_PARSE_ERROR_CHARS),
    }
}

/// Each `.py` file of a made skill's code parsed as Python (#6749 E), before
/// its check and its say-back: one that doesn't parse is refused for a
/// repair, told the file, the line, Python's error and the line as written,
/// and the lamp's calls ([`SCRIPT_SDK`]). Nothing of it is run. A parse
/// Python can't tell ([`Parse::Unchecked`]) is journaled and leaves the
/// script as it is: the person's yes still stands before it runs.
fn scripts_parse(scripts: &[(String, String)]) -> Result<(), Refused> {
    let python = python();
    for (path, text) in scripts
        .iter()
        .filter(|(path, _)| path.ends_with(RUNNABLE_ENDING))
    {
        match parsed(&python, text, PARSE_WITHIN) {
            Parse::Parses => {}
            Parse::Unchecked(why) => {
                eprintln!("lamp-voice: a made skill's {path} was not parsed: {why}");
            }
            Parse::Not { line, error } => {
                let kind = error
                    .split_once(':')
                    .map_or(error.as_str(), |(kind, _)| kind);
                let at = line.map_or(String::new(), |line| format!(" at line {line}"));
                eprintln!("lamp-voice: a made skill's code doesn't parse: {path}{at} ({kind})");
                let shown = line
                    .and_then(|line| text.lines().nth(line.checked_sub(1)?))
                    .map(str::trim)
                    .filter(|shown| !shown.is_empty())
                    .map_or(String::new(), |shown| {
                        format!(": {}", cut_chars(shown, MOST_PARSE_LINE_CHARS))
                    });
                return Err(Refused(
                    SCRIPT_DOESNT_PARSE.into(),
                    Some(format!(
                        "The lamp refused its code: {path} isn't Python that parses{at}, \
                         {error}{shown}. Reach the lamp only so: {SCRIPT_SDK}."
                    )),
                ));
            }
        }
    }
    Ok(())
}

/// [`held`] for a skill added from a file ([`capped_added`]): the cap
/// before #6066, unchanged (App Only's P2 on #6103): the light, the head,
/// [`TOPICS`] and its note (`speak`), nothing more.
fn added_held(table: &toml::Table, version: &str) -> Result<(String, Uses), Refused> {
    let mut uses = Uses::default();
    if let Some(permissions) = table.get("permissions") {
        let Some(permissions) = permissions.as_table() else {
            return Err(Refused::new("the lamp couldn't tell what it would use"));
        };
        for (key, value) in permissions {
            match (key.as_str(), value) {
                ("light", toml::Value::Boolean(on)) => uses.light = *on,
                ("body", toml::Value::Boolean(on)) => uses.body = *on,
                ("topics", toml::Value::Array(topics)) => {
                    for topic in topics {
                        match topic.as_str() {
                            Some(topic) if TOPICS.contains(&topic) => {
                                if !uses.topics.iter().any(|kept| kept == topic) {
                                    uses.topics.push(topic.to_owned());
                                }
                            }
                            _ => {
                                return Err(Refused::new(
                                    "it asks to read more than a made skill may",
                                ));
                            }
                        }
                    }
                }
                ("actions", toml::Value::Array(actions)) if actions.is_empty() => {}
                ("speak", toml::Value::Boolean(_)) => {}
                (
                    "look" | "camera" | "messages" | "known_faces" | "who" | "actions" | "services"
                    | "output_limits",
                    _,
                ) => {
                    return Err(Refused::new(
                        "it asks to use more than the light and the head, which a made skill can't",
                    ));
                }
                _ => return Err(Refused::new("the lamp couldn't tell what it would use")),
            }
        }
    }
    Ok((version.to_owned(), uses))
}

/// A `skill.toml` that isn't TOML at all (lamp-9adf, 3 Nov: voice and app
/// makes both refused so, and nothing said why): the parser's words are
/// said in the journal and go back to the model once, as the check's lines
/// do, so a slip of syntax is fixed rather than the make refused outright.
fn unreadable(error: toml::de::Error) -> Refused {
    let lines: Vec<String> = error
        .to_string()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "|")
        .map(str::to_owned)
        .collect();
    // The journal has where and what, never the quoted line: that is the
    // model's own text, which only the repair carries (as `check_reason`).
    eprintln!(
        "lamp-voice: a made skill's skill.toml can't be read: {}",
        match lines.as_slice() {
            [] => String::new(),
            [only] => only.clone(),
            [first, .., last] => format!("{first}: {last}"),
        }
    );
    let lines = lines.join(" / ");
    Refused(
        NOT_READABLE.into(),
        Some(format!("skill.toml is not TOML: {lines}")),
    )
}

/// What a made skill does, in plain words written from its `skill.toml`:
/// when it runs and what it does then. This, never the model's own
/// description, is the card's and what the lamp says before it asks to put
/// it on (Mobile on #4710, 3 Nov: lamp-0c89 kept "when someone says good
/// night, dim the light slowly over a minute" as any quiet minute from
/// 22:00 to 22:30, under a description that said what was asked). A
/// trigger or a step not put in plain words here is refused, so nothing is
/// kept that the person can't be told.
pub fn plainly(text: &str) -> Result<String, Refused> {
    plainly_with(text, reads_on())
}

/// [`plainly`], with a made skill's reads on or off ([`READS_ENVIRONMENT`]):
/// on, a skill whose one step reads a linked service is said with the
/// service, what it reads and the condition it is read under, "…, when
/// you're alone with the lamp, it reads you tomorrow's Google Calendar: …"
/// (the first builder's ruling on #5288). Off, such a step is unsaid.
pub fn plainly_with(text: &str, reads: bool) -> Result<String, Refused> {
    plainly_as(text, reads, false)
}

/// [`plainly_with`] for a skill made here, `general` with
/// [`LOOSE_ENVIRONMENT`] on: a part it can't say plainly is said in general
/// words ("changes the light", "moves its head", "says something"), never
/// refused; what the lamp can't do yet, and a read or a write, still are.
pub fn plainly_as(text: &str, reads: bool, general: bool) -> Result<String, Refused> {
    let table: toml::Table = toml::from_str(text).map_err(unreadable)?;
    said_plainly(&table, reads, general).map_err(|unsaid| {
        // The part's keys and topic, never a value nor the model's words.
        eprintln!(
            "lamp-voice: a made skill can't be said plainly: {}",
            unsaid.part
        );
        // What the lamp can't do yet is said so, with no repair (#6066).
        if let Some(what) = &unsaid.cant {
            return Refused(cant_yet(what), None);
        }
        // Its lines go back to the model once, as the check's do, naming
        // what couldn't be said and how to write it so it can.
        Refused(UNSAID.into(), Some(unsaid.repair(reads)))
    })
}

/// What [`plainly`] couldn't say of a skill: the part, by its keys and its
/// trigger's topic alone (never a value, which may be the model's words or
/// the person's), for the journal and the repair; and whether it is the
/// trigger, so the repair lists the triggers that can be said.
struct Unsaid {
    part: String,
    trigger: bool,
    /// How to write the part so it is said, told the repair after the part
    /// whole, never cut with its keys ([`step_hint`]).
    hint: String,
    /// What the lamp can't do yet, said as [`cant_yet`] and never repaired
    /// (#6066).
    cant: Option<String>,
}

impl Unsaid {
    fn of(part: impl Into<String>) -> Self {
        Self {
            part: part.into(),
            trigger: false,
            hint: String::new(),
            cant: None,
        }
    }

    fn trigger(part: impl Into<String>) -> Self {
        Self {
            part: part.into(),
            trigger: true,
            hint: String::new(),
            cant: None,
        }
    }

    fn cant(what: impl Into<String>) -> Self {
        let what = what.into();
        Self {
            part: format!("can't yet: {}", named(&what)),
            trigger: false,
            hint: String::new(),
            cant: Some(what),
        }
    }

    /// The lines a repair is told: what couldn't be said, and how to write
    /// it so it can (on lamp-74ff, 10 Nov 17:39, an app make of "glow green
    /// when I come back to my desk" was refused twice, the repair told only
    /// the keys a trigger may have, never that its topic was the trouble).
    fn repair(&self, reads: bool) -> String {
        let fix = match (self.trigger, reads) {
            (true, _) => SAYABLE_TRIGGERS.to_owned(),
            (false, false) => PLAIN_ONLY.to_owned(),
            (false, true) => format!("{PLAIN_ONLY}{PLAIN_OR_READ}"),
        };
        format!(
            "{UNSAID_PART}{}{}. {fix}",
            cut_bytes(&self.part, MOST_UNSAID_PART_BYTES),
            self.hint
        )
    }
}

/// The lamp's reason when [`plainly`] refused, journaled; the repair is
/// told its own lines ([`Unsaid::repair`]), and the person, the repairs
/// used up, hears [`NOT_MADE_TRY`] ([`plain_reason`]; the owner, 11 Nov:
/// reasons "nobody understands").
pub const UNSAID: &str = "when it would run and what it would do didn't come out in plain \
                          words; try saying when it should run, like 'when I come to my desk', \
                          and what it should do, like 'glow green'";

/// What a repair is told after a say step [`plainly`] couldn't say.
const SAY_TEXT_PART: &str =
    ": say = { text = \"the exact words\" }, one or two short sentences, no quotes";

/// What a repair is told after a sound step [`plainly`] couldn't say.
const SOUND_PART: &str = ": its one sound is cue = \"hum\"";

/// What a repair is told after a dim or `relative` light step [`plainly`]
/// couldn't say: each as `lamp_skills::light_call` takes it (lamp-9adf, 11
/// Nov 05:34: "dim the light a step at ten" written as a `relative` with
/// `until` failed all three repairs on the check's own words).
const LIGHT_PART: &str = ": relative = { dimmer = 1 }, for = \"10m\" takes only for, at most \
                          10m; dim = 1 to 4 takes only until = \"ends\"";

/// What a repair is told after a body step [`plainly`] couldn't say: each
/// as `lamp-skills` takes it (lamp-74ff, 11 Nov 09:48: a nod written with a
/// `for` failed all three repairs, twice, and `lamp-skills` refused it: "a
/// body mood goes with its for, and a for with a mood").
const BODY_PART: &str = ": body = { gesture = \"nod\" } takes no for; only mood = \"reading\" \
                         takes for, at most 2m";

/// What a repair is told after a step [`plainly`] couldn't say with more
/// than one kind in it (lamp-9adf and lamp-74ff, 11 Nov 10:01: "say hello
/// when I come back to my desk" written as one step with a gesture, a
/// light and a say, told only how to write a say, never repaired).
const ONE_KIND_PART: &str = ": one kind a [[do]] step: a light, a body and a say each a step \
                             of its own";

/// What a repair is told after a light step whose colour isn't one the
/// light has (`lamp_msgs::Light::COLOURS`, the list `lamp-skills`' check
/// holds it to).
fn colour_part() -> String {
    format!(
        ": colour is one of {}",
        lamp_msgs::Light::COLOURS.join(", ")
    )
}

/// The most a step's hints come to ([`step_hint`]), so a repair's lines
/// keep within [`MOST_REPAIR_BYTES`].
const MOST_HINT_BYTES: usize = 200;

/// How to write a `[[do]]` step [`plainly`] couldn't say, for the repair:
/// each kind in it judged on its own ([`step_plainly`]), and a hint for
/// each that fails, by what failed (a say's form, the hum, a dim, a colour
/// not in the light's list, a body's gesture or mood), then, for a step
/// with more than one kind, that each is a step of its own (lamp-9adf, 11
/// Nov 10:01: a say beside a light of an unknown colour was told only the
/// say's form, which was fine). One hint is said as it is; several after
/// one colon, by semicolons, at most [`MOST_HINT_BYTES`].
fn step_hint(step: &toml::Value) -> String {
    let Some(step) = step.as_table() else {
        return String::new();
    };
    let mut hints: Vec<String> = Vec::new();
    for (key, value) in step {
        let alone = toml::Value::Table(toml::Table::from_iter([(key.clone(), value.clone())]));
        if step_plainly(&alone).is_some() {
            continue;
        }
        let hint = match key.as_str() {
            "say" => SAY_TEXT_PART.to_owned(),
            "cue" | "sound" => SOUND_PART.to_owned(),
            "body" => BODY_PART.to_owned(),
            "light" => light_hint(value),
            _ => String::new(),
        };
        if !hint.is_empty() && !hints.contains(&hint) {
            hints.push(hint);
        }
    }
    if step.len() > 1 {
        hints.push(ONE_KIND_PART.to_owned());
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut bytes = 0;
    for hint in &hints {
        let hint = hint.strip_prefix(": ").unwrap_or(hint);
        if bytes + hint.len() + 2 <= MOST_HINT_BYTES {
            bytes += hint.len() + 2;
            kept.push(hint);
        }
    }
    if kept.is_empty() {
        return String::new();
    }
    format!(": {}", kept.join("; "))
}

/// How to write a light step [`plainly`] couldn't say: a dim or a
/// `relative` as `lamp_skills::light_call` takes it ([`LIGHT_PART`]), or
/// a colour the light has ([`colour_part`]); else nothing more than its
/// keys.
fn light_hint(light: &toml::Value) -> String {
    let Some(light) = light.as_table() else {
        return String::new();
    };
    if light.contains_key("relative") || light.contains_key("dim") {
        return LIGHT_PART.to_owned();
    }
    match light.get("colour").map(toml::Value::as_str) {
        Some(Some(colour)) if lamp_msgs::Light::COLOURS.contains(&colour) => String::new(),
        Some(_) => colour_part(),
        None => String::new(),
    }
}

/// Whether a repair's `lines` carry a step's hint for what `lamp-skills`'
/// check would refuse alike: a dim or `relative`, a body or a colour, with
/// any other hint of the same step beside it (a step of more than one
/// kind, lamp-9adf, 11 Nov 10:01), so one repair is told all of it. Then
/// they go to the repair before the check's own words ([`keep_asked`]). A
/// step of more than one kind alone, which the check takes, still goes
/// to the check first.
fn hinted_first(lines: &str) -> bool {
    [LIGHT_PART, BODY_PART]
        .iter()
        .any(|hint| lines.contains(hint.strip_prefix(": ").unwrap_or(hint)))
        || lines.contains(colour_part().trim_start_matches(": "))
}

/// What a repair is told before the part [`plainly`] couldn't say.
const UNSAID_PART: &str = "Can't be said plainly: ";

/// The most of the part named in a repair: its keys may be many.
const MOST_UNSAID_PART_BYTES: usize = 110;

/// A key or a topic as the journal and a repair name it: a plain name, or
/// "another" for anything else (a key may be any text the model wrote).
fn named(text: &str) -> &str {
    let plain = (1..=32).contains(&text.len())
        && text
            .chars()
            .all(|letter| letter.is_ascii_alphanumeric() || "/_-.".contains(letter));
    if plain { text } else { "another" }
}

/// Keys as named ([`named`]), at most six.
fn keys_named<'a>(keys: impl Iterator<Item = &'a String>) -> String {
    let keys: Vec<&str> = keys.map(|key| named(key)).collect();
    let mut said = keys[..keys.len().min(6)].join(", ");
    if keys.len() > 6 {
        said.push_str(" and more");
    }
    said
}

/// A `[[do]]` step's keys, a table's own keys after it: "light (dim, for)".
fn step_named(step: &toml::Value) -> String {
    let Some(step) = step.as_table() else {
        return "not a table".to_owned();
    };
    step.iter()
        .map(|(key, value)| match value.as_table() {
            Some(inner) => format!("{} ({})", named(key), keys_named(inner.keys())),
            None => named(key).to_owned(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// How a made skill's trigger is said when it can't be said plainly, with
/// [`LOOSE_ENVIRONMENT`] on ([`plainly_as`]).
const GENERAL_TRIGGER: &str = "When its moment comes";

/// What is said, loosened, of a made skill's parts beside its trigger and
/// steps that can't be said plainly (a `[[while]]`).
const GENERAL_CONDITIONS: &str = ", under conditions of its own";

/// The keys of a part that sends or writes, or uses the camera or the
/// microphone: never said in general words, even loosened (the first
/// builder's yes on #6059, 22:44); one that can't be said exactly is
/// refused as before ([`named_exactly`]).
const EXACT_KEYS: [&str; 20] = [
    "write",
    "read",
    "text",
    "message",
    "messages",
    "send",
    "mail",
    "email",
    "post",
    "notify",
    "look",
    "look_at",
    "camera",
    "photo",
    "picture",
    "listen",
    "hear",
    "heard",
    "record",
    "microphone",
];

/// Whether a part of a made skill must be named exactly, never in general
/// words ([`EXACT_KEYS`]): a send or a write, the camera or the
/// microphone, by its keys at any depth (a say's own `text` aside) or a
/// topic the eyes or the ears give (`humans/…`, `room/…`, `…/heard`).
fn named_exactly(part: &toml::Value) -> bool {
    match part {
        toml::Value::Table(table) => table.iter().any(|(key, value)| {
            let say_text = key == "text" && value.is_str();
            (!say_text && EXACT_KEYS.contains(&key.as_str())) || named_exactly(value)
        }),
        toml::Value::Array(items) => items.iter().any(named_exactly),
        toml::Value::String(text) => {
            text.starts_with("humans/")
                || text.starts_with("room/")
                || text.ends_with("/heard")
                || text.starts_with("speech")
        }
        _ => false,
    }
}

/// A `[[do]]` step in general words, with [`LOOSE_ENVIRONMENT`] on, by what
/// it uses: "changes the light", "moves its head", "says something"; a step
/// of more than one, each joined. Never a send or a write, the camera or
/// the microphone ([`named_exactly`]), which are said exactly or refused;
/// none for those.
fn general_step(step: &toml::Value) -> Option<String> {
    if named_exactly(step) {
        return None;
    }
    let step = step.as_table()?;
    let mut words: Vec<&str> = Vec::new();
    for (keys, said) in [
        (&["light"][..], "changes the light"),
        (&["body", "stand"][..], "moves its head"),
        (&["say"][..], "says something"),
        (&["cue", "sound"][..], "plays a sound"),
    ] {
        if keys.iter().any(|key| step.contains_key(*key)) {
            words.push(said);
        }
    }
    Some(match words.as_slice() {
        [] => "does something".to_owned(),
        words => words.join(" and "),
    })
}

/// A made skill's say-back in general words alone, with
/// [`LOOSE_ENVIRONMENT`] on, when it was too long said plainly: its trigger
/// as said (or [`GENERAL_TRIGGER`]), each step by [`general_step`], what it
/// may use.
fn general_said(table: &toml::Table, steps: &[toml::Value]) -> String {
    let trigger = trigger_plainly(table.get("trigger"))
        .map(|(said, _)| said)
        .unwrap_or_else(|_| GENERAL_TRIGGER.to_owned());
    let mut does: Vec<String> = Vec::new();
    for words in steps.iter().filter_map(general_step) {
        if does.last() != Some(&words) {
            does.push(words);
        }
    }
    format!(
        "{trigger}, it {}{}.",
        does.join(", then "),
        reaches_plainly(table)
    )
}

/// [`plainly_with`]'s words, or the part it couldn't say; `general`, with
/// [`LOOSE_ENVIRONMENT`] on, says in general words what it can't say
/// plainly ([`plainly_as`]).
fn said_plainly(table: &toml::Table, reads: bool, general: bool) -> Result<String, Unsaid> {
    // A skill written in the app ([`Written`]): a function whose answer is
    // the person's own instructions, said as that and nothing more. Only its
    // exact shape; another with a function, added from a file, is said below
    // (Twelve's P2 on #5072).
    if let Some(said) = written_plainly(table) {
        return Ok(said);
    }
    // Only what is said below, or changes nothing that runs: a `[[while]]`
    // or any other key would run otherwise than the words (Thirteen's P2 on
    // #5042). The manners are said since #6066 ([`manners_plainly`]).
    let parts = [
        "kind",
        "version",
        "needs",
        "format",
        "permissions",
        "trigger",
        "ask",
        "do",
        "tool",
        "most_a_day",
        "active",
        "quiet",
        "budget",
        "rest",
    ];
    // Its code's grants, said below, only with their switch (#6307).
    let code = scripts_on();
    let has_task = table
        .get("do")
        .and_then(toml::Value::as_array)
        .is_some_and(|steps| steps.iter().any(|step| step.get("task").is_some()));
    if !code && (has_task || table.contains_key("grants")) {
        return Err(Unsaid::cant(CODE_OF_ITS_OWN));
    }
    let extras: Vec<&String> = table
        .keys()
        .filter(|key| {
            let said_below = parts.contains(&key.as_str()) || (code && key.as_str() == "grants");
            !said_below
        })
        .collect();
    let extras = (!extras.is_empty()).then(|| format!("parts {}", keys_named(extras.into_iter())));
    let steps_of = |table: &toml::Table| {
        table
            .get("do")
            .and_then(|steps| steps.as_array())
            .filter(|steps| !steps.is_empty())
            .cloned()
            .ok_or_else(|| Unsaid::of("no [[do]] step"))
    };
    let unsaid_step = |index: usize, step: &toml::Value| Unsaid {
        hint: step_hint(step),
        ..Unsaid::of(format!("[[do]] step {}, {}", index + 1, step_named(step)))
    };
    let too_long = || Unsaid::of("what it does, too long for one breath");
    // A skill added from a file (`crate::upload_skill`): started by its
    // function alone, when the person asks, never on its own. A made skill
    // never gets here with a tool: [`capped`] refuses it first.
    if table.contains_key("tool") {
        if let Some(extras) = extras {
            return Err(Unsaid::of(extras));
        }
        if table.contains_key("trigger") || table.contains_key("ask") {
            return Err(Unsaid::of("a function's skill with a [trigger] or [ask]"));
        }
        let steps = steps_of(table)?
            .iter()
            .enumerate()
            .map(|(index, step)| {
                note_plainly(step)
                    .or_else(|| step_plainly(step))
                    .ok_or_else(|| unsaid_step(index, step))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let said = format!(
            "Only when you ask for it, it {}{}, and never on its own.",
            steps.join(", then "),
            reaches_plainly(table)
        );
        if said.chars().count() > MOST_PLAIN_CHARS {
            return Err(too_long());
        }
        return Ok(said);
    }
    // A trigger on a topic no saying here knows: the lamp can't start a
    // made skill on it yet (#6066), said so, never repaired away.
    if let Some(topic) = table
        .get("trigger")
        .and_then(|trigger| trigger.get("topic"))
        .and_then(toml::Value::as_str)
        && !SAYABLE_TOPICS.contains(&topic)
        && let Some(what) = topic_cant_yet(topic)
    {
        return Err(Unsaid::cant(what));
    }
    // A face by whose it is: `lamp-skills`' trigger sees only that a face
    // is in view (`lamp_msgs::Faces` carries no name), so starting on one
    // person is what the lamp can't do yet (#6066), said so.
    if let Some(trigger) = table.get("trigger").and_then(toml::Value::as_table)
        && trigger.get("topic").and_then(toml::Value::as_str) == Some(FACES_TOPIC)
        && ["name", "known", "who", "person", "face", "names"]
            .iter()
            .any(|key| trigger.contains_key(*key))
    {
        return Err(Unsaid::cant(ONE_PERSON));
    }
    // The trigger first, any part beside it named with it: a skill shaped
    // like a built-in one (a [[trigger]] on the faces, a [[while]]) is told
    // both in its one repair.
    // Loosened ([`LOOSE_ENVIRONMENT`]), a trigger it can't say is said in
    // general words, its own table kept for its cooldown.
    let no_trigger = toml::Table::new();
    let (mut said, trigger) = match trigger_plainly(table.get("trigger")) {
        Ok(said) => said,
        // Never one on the camera or the microphone ([`named_exactly`]).
        Err(part) if general && !table.get("trigger").is_some_and(named_exactly) => {
            eprintln!("lamp-voice: a made skill said in general words: {part}");
            (
                GENERAL_TRIGGER.to_owned(),
                table
                    .get("trigger")
                    .and_then(toml::Value::as_table)
                    .unwrap_or(&no_trigger),
            )
        }
        Err(part) => {
            return Err(Unsaid::trigger(match extras {
                Some(extras) => format!("{part}; {extras}"),
                None => part,
            }));
        }
    };
    // Loosened, parts beside it are named as conditions of its own.
    let mut conditions = "";
    if let Some(extras) = extras {
        // Never a part on the camera or the microphone ([`named_exactly`]).
        let exact = table
            .iter()
            .filter(|(key, _)| !parts.contains(&key.as_str()))
            .any(|(_, part)| named_exactly(part));
        if !general || exact {
            return Err(Unsaid::of(extras));
        }
        eprintln!("lamp-voice: a made skill said in general words: {extras}");
        conditions = GENERAL_CONDITIONS;
    }
    let steps = steps_of(table)?;
    // A write to a linked service (#6066): its skill's one step, said
    // whole, every recipient and every word it sends.
    let mut writes = false;
    // A read of a linked service (#5288): its skill's one step, said with
    // the gate it waits for.
    let mut reading = false;
    if let [step] = steps.as_slice()
        && let Some(read) = read_plainly(step).filter(|_| reads)
    {
        reading = true;
        said.push_str(&format!(", when you're alone with the lamp, it {read}"));
    } else if let [step] = steps.as_slice()
        && let Some(write) = write_plainly(step).filter(|_| writes_on())
    {
        writes = true;
        said.push_str(&format!(", it {write}"));
    } else {
        let steps = steps
            .iter()
            .enumerate()
            .map(|(index, step)| {
                match step_plainly(step).or_else(|| task_plainly(step).filter(|_| code)) {
                    Some(words) => Ok(words),
                    None => match step_cant_yet(step) {
                        Some(what) => Err(Unsaid::cant(what)),
                        // Loosened, a step it can't say in its own words is
                        // said in general ones (never a read or a write:
                        // [`general_step`]).
                        None if general => {
                            let unsaid = unsaid_step(index, step);
                            let words =
                                general_step(step).ok_or_else(|| unsaid_step(index, step))?;
                            eprintln!(
                                "lamp-voice: a made skill said in general words: {}",
                                unsaid.part
                            );
                            Ok(words)
                        }
                        None => Err(unsaid_step(index, step)),
                    },
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        said.push_str(&format!(", it {}", steps.join(", then ")));
    }
    said.push_str(conditions);
    if let Some(cooldown) = trigger.get("cooldown") {
        match cooldown.as_str().and_then(duration) {
            Some((1, unit)) => said.push_str(&format!(", at most once {}", one(unit))),
            Some((count, unit)) => {
                said.push_str(&format!(", at most once every {count} {unit}s"));
            }
            // Loosened, a cooldown it can't say is left to the check.
            None if general => {}
            None => {
                return Err(Unsaid::trigger(format!(
                    "{}, its cooldown",
                    trigger_named(trigger)
                )));
            }
        }
    }
    match ask_plainly(table) {
        Ok(ask) => said.push_str(ask),
        Err(_) if general => {}
        Err(unsaid) => return Err(unsaid),
    }
    // The camera named once: a look step already says it.
    let reach = reaches_plainly(table);
    match said.contains("with the camera") {
        true => said.push_str(&reach.replace(", using the camera", "")),
        false => said.push_str(&reach),
    }
    said.push('.');
    // Code of its own (#6307): its task's words and what its code may use
    // said whole, never cut.
    let runs_code = code && has_task;
    // Said whole or not at all: never cut before a step or the cooldown. A
    // write's words are the request's own, so at most its length again.
    // Loosened, one too long is said in general words, every step one
    // ([`general_said`]), and is cut where a description is, never refused.
    let most = if writes || runs_code {
        MOST_WRITE_PLAIN_CHARS
    } else {
        MOST_PLAIN_CHARS
    };
    if said.chars().count() > most {
        if !general || writes || reading || runs_code || steps.iter().any(named_exactly) {
            return Err(too_long());
        }
        eprintln!("lamp-voice: a made skill said in general words: what it does, too long");
        said = general_said(table, &steps);
    }
    // That it starts on its own, and how often (#6066 (a)): a sentence of
    // its own, at most [`MOST_MANNERS_CHARS`], beside what it does.
    // Loosened, manners in a shape it can't say are told generally.
    said.push_str(&match manners_plainly(table) {
        Ok(manners) => manners,
        Err(_) if general => format!("{MANNERS_BEGIN}, within limits of its own."),
        Err(unsaid) => return Err(unsaid),
    });
    // And that it sends nothing until the person's yes, ending on no word
    // for them to repeat (#6066).
    if writes {
        said.push_str(WAITS_FOR_YES);
    }
    // And who wrote its code, what it may use, and that it runs nothing
    // until the person's yes (#6307).
    if runs_code {
        let grants: Vec<String> = table
            .get("grants")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|grant| grant.as_str().map(str::to_owned))
            .collect();
        // Its wait said unless it goes on at once: grants within
        // [`CODE_AT_ONCE_GRANTS`] and nothing it reaches beside them
        // (the camera, the microphone, who is in view, a message).
        let at_once = grants_go_on_at_once(&grants) && !writes && reaches_plainly(table).is_empty();
        said.push_str(&code_said_as(&grants, at_once));
    }
    Ok(said)
}

/// A made skill's `task` step in plain words (#6307): "runs its own code
/// to “take a photo”", its words as written, at most [`MOST_TASK_CHARS`];
/// none for any other step, or one not in that form.
fn task_plainly(step: &toml::Value) -> Option<String> {
    let step = step.as_table().filter(|step| step.len() == 1)?;
    let task = step.get("task")?.as_table()?;
    let words = task.get("task")?.as_str()?.trim();
    task.get("run")?.as_str()?;
    let words = words.trim_end_matches(['.', '!', '?']);
    (!words.is_empty()
        && words.chars().count() <= MOST_TASK_CHARS
        && words.chars().all(|c| !c.is_control() && !is_format(c)))
    .then(|| format!("runs its own code to “{words}”"))
}

/// The most [`manners_plainly`] says: each part at its longest.
const MOST_MANNERS_CHARS: usize = 220;

/// That a made skill starts on its own, and how often, as [`plainly`] says
/// it after what it does (#6066 (a)): " It starts on its own, at most 3
/// times a day, one an hour with the lamp's other nudges, never from 22:00
/// to 08:00, and rests till morning if ignored twice." Each of
/// `most_a_day`, `[active]`, `[quiet]`, `[budget]` and `[rest]` it has is
/// said, as `lamp-skills` takes it; one in another shape is unsaid.
fn manners_plainly(table: &toml::Table) -> Result<String, Unsaid> {
    let unsaid = |part: &str| Unsaid::of(format!("parts {part}"));
    let mut parts = Vec::new();
    if let Some(most) = table.get("most_a_day") {
        parts.push(
            match most.as_integer().ok_or_else(|| unsaid("most_a_day"))? {
                1 => "once a day".to_owned(),
                // Past [`mannered`]'s bound only in a shape it couldn't
                // set: unsaid, so the repair writes it plainly.
                most @ 2..=6 => format!("at most {most} times a day"),
                _ => return Err(unsaid("most_a_day")),
            },
        );
    }
    if let Some(budget) = table.get("budget") {
        let group = budget.get("group").and_then(toml::Value::as_str);
        let per_hour = budget.get("per_hour").and_then(toml::Value::as_integer);
        let others = budget
            .as_table()
            .is_some_and(|budget| budget.keys().all(|key| key == "group" || key == "per_hour"));
        parts.push(match (group, per_hour, others) {
            (Some(MANNERS_GROUP), Some(1), true) => {
                "one an hour with the lamp's other nudges".to_owned()
            }
            (Some(MANNERS_GROUP), Some(2), true) => {
                "two an hour with the lamp's other nudges".to_owned()
            }
            _ => return Err(unsaid("budget")),
        });
    }
    let hours = |part: &str| -> Result<Option<(String, String)>, Unsaid> {
        let Some(hours) = table.get(part) else {
            return Ok(None);
        };
        let at = |key: &str| {
            hours
                .get(key)
                .and_then(toml::Value::as_str)
                .filter(|at| {
                    at.len() == 5
                        && at.as_bytes()[2] == b':'
                        && at.replacen(':', "", 1).chars().all(|c| c.is_ascii_digit())
                })
                .map(str::to_owned)
        };
        match (at("from"), at("to")) {
            (Some(from), Some(to)) => Ok(Some((from, to))),
            _ => Err(unsaid(part)),
        }
    };
    if let Some((from, to)) = hours("active")? {
        parts.push(format!("only from {from} to {to}"));
    }
    if let Some((from, to)) = hours("quiet")? {
        parts.push(format!("never from {from} to {to}"));
    }
    if let Some(rest) = table.get("rest") {
        let ignored = rest
            .get("after_ignored")
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| unsaid("rest"))?;
        parts.push(match ignored {
            1 => "rests till morning if ignored once".to_owned(),
            2 => "rests till morning if ignored twice".to_owned(),
            3 => "rests till morning if ignored 3 times".to_owned(),
            _ => return Err(unsaid("rest")),
        });
    }
    let said = match parts.as_slice() {
        [] => format!("{MANNERS_BEGIN}."),
        [one] => format!("{MANNERS_BEGIN}, {one}."),
        [rest @ .., last] => format!("{MANNERS_BEGIN}, {}, and {last}.", rest.join(", ")),
    };
    if said.chars().count() > MOST_MANNERS_CHARS {
        return Err(unsaid("of its manners, too long"));
    }
    Ok(said)
}

/// The trigger topics [`plainly`] can say (#6066): [`TOPICS`], a pat on
/// the head (`controls/pet`), the air (`air/state`) and what someone at the
/// desk is doing (`room/activity`). A trigger on any other is what the lamp
/// can't start a made skill on yet ([`topic_cant_yet`]).
const SAYABLE_TOPICS: [&str; 8] = [
    "voice/state",
    "room/presence",
    FACES_TOPIC,
    STOOD_UP_TOPIC,
    CLOCK_TOPIC,
    PET_TOPIC,
    AIR_TOPIC,
    ACTIVITY_TOPIC,
];
/// A hand on the head's pads (`lamp_msgs` `controls/pet`).
const PET_TOPIC: &str = "controls/pet";
/// The air at the desk.
const AIR_TOPIC: &str = "air/state";
/// What someone at the desk is doing, as the eyes see it.
const ACTIVITY_TOPIC: &str = "room/activity";

/// What the lamp can't start a made skill on yet, by a real topic of the
/// lamp's that no saying here knows, in plain words: "starting on words it
/// hears". Any other topic is none: a slip the repair fixes.
/// What a made skill on one person's face can't do yet: the trigger sees
/// a face, never whose.
const ONE_PERSON: &str = "starting when it sees one particular person";

fn topic_cant_yet(topic: &str) -> Option<&'static str> {
    Some(match topic {
        "voice/heard" | "name/heard" => "starting on words it hears",
        "ears/classes" => "starting on a sound it hears",
        "schedule/due" => "starting when a reminder comes due",
        "tasks/status" => "starting on the news of your work",
        "controls/touch" | "controls/state" => "starting on a touch",
        "mood/state" => "starting on its own mood",
        "lamp/health" => "starting on how its parts are",
        "lamp/pause" => "starting on a pause",
        _ => return None,
    })
}

/// What a `[[do]]` step asks that the lamp can't do yet in a made skill
/// (#6066), in plain words: a message or a write sent out (no step sends
/// one: a text is a watch's, and a service is only read), a move of its own
/// or a sound it doesn't ship (a made skill carries no file but its two).
fn step_cant_yet(step: &toml::Value) -> Option<&'static str> {
    let step = step.as_table()?;
    // A fixed write to a linked service, only with its switch (#6066).
    if step.contains_key("write") && step.len() == 1 {
        return (!writes_on()).then_some(SENDS_ON_ITS_OWN);
    }
    // Code of its own, only with its switch (#6307).
    if step.contains_key("task") && step.len() == 1 {
        return (!scripts_on()).then_some(CODE_OF_ITS_OWN);
    }
    let sends = [
        "text", "message", "messages", "send", "mail", "email", "post", "write", "notify",
    ];
    if step.keys().any(|key| sends.contains(&key.as_str())) {
        return Some("sending a message or writing to a linked service");
    }
    if step.get("body").and_then(|body| body.get("move")).is_some() {
        return Some("a move of its own");
    }
    let other_cue = step
        .get("cue")
        .is_some_and(|cue| sound_plainly(cue).is_none());
    (other_cue || step.contains_key("sound")).then_some(OTHER_SOUND)
}

/// A sound a made skill can't play yet: one it would carry.
const OTHER_SOUND: &str = "a sound other than its soft hum";

/// How a made skill offers itself, its `[ask] policy`, as [`plainly`] adds
/// it after the steps: nothing for a nudge or none, ", as a hello", ", as a
/// reminder", ", as news", ", as a warning" (#6066). Another is unsaid.
fn ask_plainly(table: &toml::Table) -> Result<&'static str, Unsaid> {
    let Some(ask) = table.get("ask") else {
        return Ok("");
    };
    match ask.get("policy").and_then(toml::Value::as_str) {
        Some("nudge") => Ok(""),
        Some("hello") => Ok(", as a hello"),
        Some("reminder") => Ok(", as a reminder"),
        Some("news") => Ok(", as news"),
        Some("warning") => Ok(", as a warning"),
        _ => Err(Unsaid::of(
            "[ask] policy: nudge, hello, reminder, news or warning",
        )),
    }
}

/// What a made skill reaches beyond the lamp, or sees and hears, named
/// after what it does (#6066, the first builder's rule: said back before it goes
/// on): ", using the camera", ", listening with the microphone", ", and it
/// may send a message to your phone". From its `[permissions]`, so a grant
/// is named whether or not a step uses it.
fn reaches_plainly(table: &toml::Table) -> String {
    let Some(permissions) = table.get("permissions").and_then(toml::Value::as_table) else {
        return String::new();
    };
    let given = |key: &str| permissions.get(key).and_then(toml::Value::as_bool) == Some(true);
    let format_2 = table.get("format").and_then(toml::Value::as_integer) == Some(2);
    let listens = permissions
        .get("topics")
        .and_then(toml::Value::as_array)
        .is_some_and(|topics| {
            topics.iter().any(|topic| {
                topic
                    .as_str()
                    .is_some_and(|topic| MICROPHONE_TOPICS.contains(&topic))
            })
        });
    let mut said = String::new();
    if given("camera") || (given("look") && !format_2) {
        said.push_str(", using the camera");
    }
    if listens {
        said.push_str(", listening with the microphone");
    }
    // Known faces or who is at the lamp (Thirteen's P3 on #6103).
    if given("known_faces") || given("who") {
        said.push_str(", knowing who is in view");
    }
    if given("messages") {
        said.push_str(", and it may send a message to your phone");
    }
    said
}

/// A trigger as the journal and a repair name it: its topic alone.
fn trigger_named(trigger: &toml::Table) -> String {
    match trigger.get("topic").and_then(|topic| topic.as_str()) {
        Some(topic) => format!("trigger topic {}", named(topic)),
        None => "trigger with no topic".to_owned(),
    }
}

/// When a skill's one `[trigger]` runs, in plain words, and the trigger;
/// or the part that can't be said, by its topic and keys ([`Unsaid`]).
/// What a repair is told of a `when` [`trigger_plainly`] can't say: the
/// ones `lamp-skills` knows that it says, or none.
const WHEN_PART: &str = "when, which is \"appears\", \"present\" or left out";

/// The `when`s `lamp-skills` knows on a trigger (`lamp_skills::When`):
/// any other is refused by its check.
const KNOWN_WHENS: [&str; 3] = ["appears", "present", "changes"];

/// Six triggers are said: someone comes to the desk (or comes back, after
/// it was empty a while), the desk is left, the lamp goes quiet, a face
/// comes into view (after none, or none for a while), someone at the desk
/// gets up, and a time of day, each day or on named days of the week; each
/// with its `for`, `rearm`, `during` and `cooldown` (said after the steps)
/// as lamp-skills counts them for that topic.
fn trigger_plainly(trigger: Option<&toml::Value>) -> Result<(String, &toml::Table), String> {
    // The keys a trigger said may have; any other is named.
    let unknown = |trigger: &toml::Table| {
        let known = [
            "topic",
            "field",
            "equals",
            "for",
            "cooldown",
            "rearm",
            "during",
            "when",
            "absent_for",
            "above",
            "days",
        ];
        let unknown: Vec<&String> = trigger
            .keys()
            .filter(|key| !known.contains(&key.as_str()))
            .collect();
        (!unknown.is_empty()).then(|| format!(", keys {}", keys_named(unknown.into_iter())))
    };
    let trigger = match trigger {
        None => return Err("no [trigger]".to_owned()),
        Some(toml::Value::Table(trigger)) => trigger,
        // [[trigger]], as the built-in skills write theirs: a list.
        Some(toml::Value::Array(triggers)) => {
            return Err(match triggers.first().and_then(|first| first.as_table()) {
                Some(first) => format!(
                    "[[trigger]] written as a list, its first {}{}",
                    trigger_named(first),
                    unknown(first).unwrap_or_default()
                ),
                None => "[[trigger]] written as a list".to_owned(),
            });
        }
        Some(_) => return Err("[trigger] that isn't a table".to_owned()),
    };
    let named = trigger_named(trigger);
    if let Some(keys) = unknown(trigger) {
        return Err(format!("{named}{keys}"));
    }
    let text_of = |key: &str| trigger.get(key).and_then(|value| value.as_str());
    let its = |what: &str| format!("{named}, its {what}");
    let lasting = match trigger.get("for") {
        Some(lasting) => Some(
            lasting
                .as_str()
                .and_then(spell)
                .ok_or_else(|| its("for, like \"5m\": s, m or h, above 0"))?,
        ),
        None => None,
    };
    // `when = "appears"`: once each time it comes to hold, as
    // `rearm = "ends"` runs it, so said in the same words; and only with
    // it does `lamp-skills` count `absent_for`, the while it didn't hold
    // before (`lamp_skills::Trigger::absent_for_us`).
    let appears = match trigger.get("when").map(toml::Value::as_str) {
        None | Some(Some("present")) => false,
        Some(Some("appears")) => true,
        Some(_) => return Err(its(WHEN_PART)),
    };
    let absent = match trigger.get("absent_for") {
        Some(absent) => Some(
            absent
                .as_str()
                .and_then(spell)
                .filter(|_| appears)
                .ok_or_else(|| its("absent_for, which goes with when = \"appears\""))?,
        ),
        None => None,
    };
    // `rearm = "ends"` (or `"leaves"`, the same): once each time it holds.
    let rearmed = matches!(
        trigger.get("rearm").map(toml::Value::as_str),
        Some(Some("ends" | "leaves"))
    );
    if trigger.contains_key("rearm") && !rearmed {
        return Err(its("rearm"));
    }
    let someone = |equals: bool| {
        (
            Some("room/presence"),
            Some("someone"),
            Some(toml::Value::Boolean(equals)),
        )
    };
    let topic = (
        text_of("topic"),
        text_of("field"),
        trigger.get("equals").cloned(),
    );
    // `above` only on someone standing up, its one test.
    if trigger.contains_key("above") && ![Some(STOOD_UP_TOPIC), Some(AIR_TOPIC)].contains(&topic.0)
    {
        return Err(its("above"));
    }
    // `days` only on a time of day, its one window's days (#6066).
    if trigger.contains_key("days") && topic.0 != Some(CLOCK_TOPIC) {
        return Err(its("days, only on a time of day"));
    }
    let above = trigger.get("above").and_then(|line| match line {
        toml::Value::Integer(line) => Some(*line as f64),
        toml::Value::Float(line) => Some(*line),
        _ => None,
    });
    let mut said = match (lasting, absent) {
        // Each day at a time (lamp-skills' own clock, `clock/local`): the
        // built-in daily triggers' shape, known local time held once
        // (`rearm`) in a window of one minute, so it runs once at that
        // minute; on its `days` alone when it names them (#6066, as
        // lamp-skills reads them, [`weekdays`]); never a window's while.
        (lasting, absent) if topic.0 == Some(CLOCK_TOPIC) => {
            let at = (topic.1 == Some("known")
                && topic.2 == Some(toml::Value::Boolean(true))
                && lasting.is_none()
                && absent.is_none()
                && rearmed
                && !appears)
                .then(|| trigger.get("during").and_then(one_minute))
                .flatten()
                .ok_or_else(|| {
                    its("test, rearm and during: known = true, rearm = \"ends\", during of one minute")
                })?;
            let every = match trigger.get("days") {
                Some(days) => weekdays(days)
                    .map(every_day_of)
                    .map_err(|why| its(&format!("days: {why}")))?,
                None => "Every day".to_owned(),
            };
            return Ok((format!("{every} at {at}"), trigger));
        }
        // A face comes into view (`humans/faces/tracked` with no field
        // test: any face, never whose), once each time (`appears`, or
        // `rearm`), after none, or none for `absent_for`, which
        // lamp-skills counts only with `appears`.
        (lasting, absent) if topic.0 == Some(FACES_TOPIC) => {
            if topic.1.is_some() || topic.2.is_some() {
                return Err(its(
                    "field and equals: a face in view has no test, never who",
                ));
            }
            if !appears && !rearmed {
                return Err(its("when, which is \"appears\""));
            }
            let none = match absent {
                Some(absent) => format!("after seeing none for {absent}"),
                None => "after seeing none".to_owned(),
            };
            match lasting {
                Some(lasting) => format!("When the lamp has seen a face for {lasting}, {none}"),
                None => format!("When the lamp sees a face, {none}"),
            }
        }
        // Someone at the desk gets up to leave (`room/stood_up`, one
        // message a departure, lasting 5 s): every departure, as Watch you
        // leave tests it (`sitting_s` above 0), never held a while.
        (None, None) if topic.0 == Some(STOOD_UP_TOPIC) => {
            if topic.1 != Some("sitting_s") || topic.2.is_some() || above != Some(0.0) {
                return Err(its("field and above: sitting_s above 0"));
            }
            "When someone sitting at the desk gets up".to_owned()
        }
        (_, _) if topic.0 == Some(STOOD_UP_TOPIC) => {
            return Err(its("for or absent_for, which a getting up never lasts"));
        }
        // A pat on the head (#6066): Controls says `ended = false` while a
        // hand is on the pads, so once a pet with `rearm`.
        (None, None) if topic.0 == Some(PET_TOPIC) => {
            if topic.1 != Some("ended") || topic.2 != Some(toml::Value::Boolean(false)) {
                return Err(its("field and equals: ended, equals false"));
            }
            "When someone pats its head".to_owned()
        }
        // Stuffy air (#6066): CO2 above a line, as Stuffy air reads it.
        (lasting, None) if topic.0 == Some(AIR_TOPIC) => {
            let line = above
                .filter(|line| topic.1 == Some("co2_ppm") && topic.2.is_none() && *line > 0.0)
                .ok_or_else(|| its("field and above: co2_ppm above a level"))?;
            let line = line as u64;
            match lasting {
                Some(lasting) => {
                    format!("When the air has been stuffy, CO2 over {line} ppm, for {lasting}")
                }
                None => format!("When the air gets stuffy, CO2 over {line} ppm"),
            }
        }
        // What someone at the desk is doing, as the eyes see it (#6066).
        (lasting, None) if topic.0 == Some(ACTIVITY_TOPIC) => {
            let doing = match (topic.1, topic.2.as_ref().and_then(toml::Value::as_str)) {
                (Some("state"), Some("head_down")) => "someone is head down at the desk",
                (Some("state"), Some("at_desk")) => "someone is at the desk, looking up",
                (Some("state"), Some("leaning_in")) => "someone leans in close",
                _ => {
                    return Err(its(
                        "field and equals: state, head_down, at_desk or leaning_in",
                    ));
                }
            };
            match lasting {
                Some(lasting) => format!("When {doing} for {lasting}"),
                None => format!("When {doing}"),
            }
        }
        (lasting, None)
            if topic.0 == Some("voice/state")
                && topic.1 == Some("state")
                && topic.2.as_ref().and_then(toml::Value::as_str) == Some("resting") =>
        {
            match lasting {
                Some(lasting) => format!("When the lamp has been quiet for {lasting}"),
                None => "When the lamp goes quiet".to_owned(),
            }
        }
        // Back after the desk was empty a while: the built-in welcome
        // back's own `absent_for`, on the desk's presence.
        (lasting, Some(absent)) if topic == someone(true) => match lasting {
            Some(lasting) => format!(
                "When someone has been back at the desk for {lasting}, after it was empty for \
                 {absent}"
            ),
            None => format!("When someone comes back to the desk after it was empty for {absent}"),
        },
        (lasting, None) if topic == someone(true) => match lasting {
            Some(lasting) => format!("When someone has been at the desk for {lasting}"),
            None => "When someone comes to the desk".to_owned(),
        },
        (lasting, None) if topic == someone(false) => match lasting {
            Some(lasting) => format!("When the desk has been empty for {lasting}"),
            None => "When the desk is left".to_owned(),
        },
        (_, Some(_)) => return Err(its("absent_for, only for someone coming back")),
        _ => return Err(its("field and equals")),
    };
    if let Some(during) = trigger.get("during") {
        let hour = |key: &str| during.get(key).and_then(|hour| hour.as_str());
        let (Some(from), Some(to)) = (hour("from"), hour("to")) else {
            return Err(its("during"));
        };
        said.push_str(&format!(", between {from} and {to}"));
    }
    Ok((said, trigger))
}

/// A `during` of one minute, `{ from = "22:00", to = "22:01" }`: its
/// `from`, as lamp-skills reads a time of day (`HH:MM`, 00:00 to 23:59).
fn one_minute(during: &toml::Value) -> Option<String> {
    let during = during.as_table().filter(|during| during.len() == 2)?;
    let minute = |key: &str| {
        let text = during.get(key)?.as_str()?;
        let (hours, minutes) = text.split_once(':')?;
        let digits = |part: &str| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit());
        if !digits(hours) || !digits(minutes) {
            return None;
        }
        let (hours, minutes): (u32, u32) = (hours.parse().ok()?, minutes.parse().ok()?);
        (hours < 24 && minutes < 60).then_some((text, hours * 60 + minutes))
    };
    let ((from, start), (_, end)) = (minute("from")?, minute("to")?);
    (end == (start + 1) % (24 * 60)).then(|| from.to_owned())
}

/// The names a clock trigger's `days` takes, Monday first, as lamp-skills
/// reads them (`lamp_skills::daily::Weekdays`, #6066), and each day said.
const DAY_NAMES: [(&str, &str); 7] = [
    ("mon", "Monday"),
    ("tue", "Tuesday"),
    ("wed", "Wednesday"),
    ("thu", "Thursday"),
    ("fri", "Friday"),
    ("sat", "Saturday"),
    ("sun", "Sunday"),
];

/// A trigger's `days`, a bit a day, Monday the lowest, held as
/// lamp-skills holds them (`Weekdays::parse`, #6066): one name or a list
/// of `mon` to `sun`, `weekdays` (Monday to Friday) and `weekend`
/// (Saturday and Sunday); a day named twice is the same day; an empty
/// list, any other name or a value that isn't one is refused, in words a
/// repair is told.
fn weekdays(days: &toml::Value) -> Result<u8, String> {
    let names: Vec<&toml::Value> = match days {
        toml::Value::Array(names) => names.iter().collect(),
        name => vec![name],
    };
    if names.is_empty() {
        return Err("it names no day".to_owned());
    }
    let mut bits = 0u8;
    for name in names {
        let name = name.as_str().unwrap_or_default();
        bits |= match name {
            "weekdays" => 0b001_1111,
            "weekend" => 0b110_0000,
            _ => match DAY_NAMES.iter().position(|(day, _)| *day == name) {
                Some(index) => 1 << index,
                None => {
                    return Err(format!(
                        "{} is not mon to sun, weekdays or weekend",
                        named(name)
                    ));
                }
            },
        };
    }
    Ok(bits)
}

/// [`weekdays`]' days in plain words: "Every day", "Every weekday",
/// "Every Saturday and Sunday", "Every Wednesday and Friday".
fn every_day_of(bits: u8) -> String {
    match bits {
        0b111_1111 => return "Every day".to_owned(),
        0b001_1111 => return "Every weekday".to_owned(),
        _ => {}
    }
    let days: Vec<&str> = DAY_NAMES
        .iter()
        .enumerate()
        .filter(|(index, _)| bits & (1 << index) != 0)
        .map(|(_, (_, day))| *day)
        .collect();
    match days.split_last() {
        Some((last, [])) => format!("Every {last}"),
        Some((last, rest)) => format!("Every {} and {last}", rest.join(", ")),
        None => "Every day".to_owned(),
    }
}

/// The longest [`plainly`] says: one breath, and with [`left_out`]'s
/// words still within [`MOST_DESCRIPTION_CHARS`], so nothing is cut. 240
/// since #6066, room for an outbound or camera step named after the steps.
const MOST_PLAIN_CHARS: usize = 240;

/// The longest [`plainly`] says of a skill that writes (#6066): its write
/// said whole, every recipient and every word, which are the request's own
/// ([`MOST_REQUEST_BYTES`]) and may be said twice over (a subject and a
/// text), with when it runs. Never cut: longer is unsaid.
const MOST_WRITE_PLAIN_CHARS: usize = 1_400;

/// The triggers [`plainly`] says, each with what it means: in
/// [`SAYABLE_TRIGGERS`] and [`TRIGGER_MEANINGS`] alike.
macro_rules! trigger_meanings {
    () => {
        "each as topic, field, test: someone comes or comes back to the desk: \
         \"room/presence\", \"someone\", equals true (back after a while away: \
         when = \"appears\", absent_for = \"5m\"); the desk left: equals false; the lamp goes \
         quiet: \"voice/state\", \"state\", equals \"resting\"; a face comes into view: \
         \"humans/faces/tracked\", no field, when = \"appears\"; someone gets up: \
         \"room/stood_up\", \"sitting_s\", above = 0; every day at 22:00: \"clock/local\", \
         \"known\", equals true, rearm = \"ends\", during = { from = \"22:00\", to = \"22:01\" }."
    };
}

/// What a repair is told when [`plainly`] refused a part other than the
/// trigger.
const PLAIN_ONLY: &str = "Write only kind, version, needs, format, permissions, one [trigger], \
                          [ask] and [[do]], and at most a few [[do]] steps, each one light \
                          (colour, dim, out or relative), one body (a listed gesture, \
                          look_at = \"face\"), one say = { text = \"the exact words\" } or \
                          cue = \"hum\" (with speak = true), as the request names them";

/// The triggers [`plainly`] says beyond [`trigger_meanings`]' (#6066): in
/// [`TRIGGER_MEANINGS`] for the making model, and named by
/// [`SAYABLE_TRIGGERS`] for a repair, which has no room for them whole.
macro_rules! more_trigger_meanings {
    () => {
        " Also: a pat on the head: \"controls/pet\", \"ended\", equals false, rearm = \"ends\"; \
         stuffy air: \"air/state\", \"co2_ppm\", above = 1200; someone head down, at the desk \
         or leaning in: \"room/activity\", \"state\", equals \"head_down\", \"at_desk\" or \
         \"leaning_in\"."
    };
}

/// What a repair is told when [`plainly`] refused the trigger: the ones it
/// says, each with what it means, so a request to "come back to my desk"
/// is written on the desk's presence, not on the faces as the built-in
/// welcome back's is (lamp-74ff, 10 Nov 17:39). `make_skill_local`'s
/// system prompt tells the making model the same before it writes, with
/// the pat, the air and the activity whole ([`TRIGGER_MEANINGS`]).
pub const SAYABLE_TRIGGERS: &str = concat!(
    "One [trigger] table, one of: ",
    trigger_meanings!(),
    " Beside it only for, cooldown, rearm = \"ends\", during, days."
);

/// The triggers [`plainly`] says, each with what it means, for the making
/// model's system prompt (`make_skill_local`), and a time's days (#6066),
/// which a repair is told only when they are its trouble ([`weekdays`]'
/// own words name every day), so its lines keep within
/// [`MOST_REPAIR_BYTES`].
pub const TRIGGER_MEANINGS: &str = concat!(
    trigger_meanings!(),
    " On some days only, add days = [\"wed\", \"fri\"] (mon to sun, \"weekdays\", \
     \"weekend\").",
    more_trigger_meanings!()
);

/// The steps, `[ask]` and grants a made skill may have beyond the light,
/// gesture, say and hum the brief names (#6066), for the making model's
/// system prompt (`make_skill_local`): each one [`plainly`] says, a dim and
/// a `relative` light as `lamp_skills::light_call` takes them (lamp-9adf,
/// 11 Nov 05:34: "dim the light a step at ten" was written as a `relative`
/// with `until`, and once with a `for` in words, and failed all three
/// repairs); a gesture with no `for`, a getting up with no `for` or
/// `absent_for`, and `speak` only with something said (lamp-74ff, 11 Nov
/// 09:48: "nod when I stand up" written with all three failed every
/// repair, twice; [`tidied`] takes them out too).
pub const MORE_STEPS: &str = "A light step may also be { dim = 1 } (dim = 1 to 4, beside it \
     only until = \"ends\"), { out = true }, { relative = { dimmer = 1 }, for = \"10m\" } (a \
     step dimmer, or warmer = 1, for a while: beside it only for, at most 10m) or a colour \
     with pattern = { kind = \"breathe\" } (or candle, pulse). Every for is a whole number \
     and ms, s, m or h, like \"30s\" or \"10m\", never words. A body step may be \
     { gesture = \"nod\" } with no for (a gesture plays once), look_at = \"face\", or \
     mood = \"reading\" with for = \"1m\" (at most 2m): only a mood takes a for. A getting \
     up (room/stood_up) takes no for or absent_for. speak = true only with a say or cue \
     step, or a read. Each [[do]] step is one kind, a light, a body, a say or the hum, each a \
     step of its own: saying hello is one say step. [ask] policy is \"nudge\", or \"hello\", \"reminder\" or \"news\" as the \
     request means it. Any [permissions] a skill may have are fine as its steps use them: the \
     lamp says each back to the person before it goes on.";

/// What [`PLAIN_ONLY`] goes on to say with reads on: a skill that reads a
/// linked service keeps its one read step. On lamp-9adf, 5 Nov 13:26, an
/// app make of "Read me my latest email when I sit down at my desk." was
/// refused twice: the repair, told only lights and gestures, took the
/// mail read away.
const PLAIN_OR_READ: &str = "; or, for a read of a linked service, only one [[do]] step: that \
                             service's read step exactly as your instructions list it, and \
                             nothing else";

/// A `room/stood_up` trigger's `for` or `absent_for` of more than nothing,
/// spelled ("10 minutes"): what [`tidied`] takes out, since a getting up
/// never lasts. None for one of nothing, taken out without a word.
fn dropped_wait(text: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(text).ok()?;
    let trigger = table.get("trigger")?.as_table()?;
    if trigger.get("topic")?.as_str()? != STOOD_UP_TOPIC {
        return None;
    }
    ["for", "absent_for"].iter().find_map(|key| {
        let wait = trigger.get(*key)?.as_str()?;
        (whole_ms(wait)? > 0).then(|| spell(wait)).flatten()
    })
}

/// What the say-back adds of a getting up's wait taken out
/// ([`dropped_wait`]): "nod when I've been up 10 minutes" is told it nods
/// on every getting up.
fn cant_wait(wait: &str) -> String {
    format!("It can't wait {wait} for that: it runs each time someone gets up.")
}

/// The model's [`LEFT_OUT`] words in a fixed frame, as its own note: no
/// quotes, and one clause, so they can't read as another sentence of what
/// the skill does (Thirteen's P2 on #5042).
fn left_out(what: &str) -> String {
    let clause: String = what
        .chars()
        .take_while(|letter| !matches!(letter, '.' | '!' | '?' | ';' | ':' | '\n'))
        .filter(|letter| !matches!(letter, '"' | '\'' | '“' | '”' | '‘' | '’' | '`'))
        .collect();
    let clause = clause.trim().trim_end_matches(',');
    format!("The maker notes what it couldn't do: {clause}.")
}

/// A function's note (`say = "…"`, the model's own answer to its call) in
/// plain words: a skill added from a file's only way to speak.
fn note_plainly(step: &toml::Value) -> Option<String> {
    let step = step.as_table().filter(|step| step.len() == 1)?;
    step.get("say")?
        .as_str()
        .filter(|note| !note.trim().is_empty())
        .map(|_| "answers in its own words, as its file tells it".to_owned())
}

/// A read step (#5288) in plain words: the service, what it reads and what
/// is said of it, as `lamp-skills` takes the step
/// (`lamp_skills::service_read`). Any other shape is none, so unsaid.
fn read_plainly(step: &toml::Value) -> Option<String> {
    let step = step.as_table().filter(|step| step.len() == 1)?;
    let read = step.get("read")?.as_table()?;
    let text = |key: &str| read.get(key).and_then(|value| value.as_str());
    let (service, what) = (text("service")?, text("what")?);
    // A fixed read of a service with one: never a whole text or a
    // free-form read (Thirteen's P2 on #5303).
    if lamp_services::spoken::skill_read_refused(service, what).is_some() {
        return None;
    }
    let events = service == "google_calendar" && what == "events";
    match (events, text("day")) {
        (true, Some(day @ ("today" | "tomorrow"))) if read.len() == 3 => {
            Some(lamp_services::spoken::plainly(service, what, Some(day)))
        }
        (false, None) if !read.contains_key("day") => {
            Some(lamp_services::spoken::plainly(service, what, None))
        }
        _ => None,
    }
}

/// What the person is told of a write step the lamp can't send: not fixed
/// whole, not one of the kinds a made skill writes, or its service not
/// granted as a write. Never a value.
pub const WRITE_NOT_FIXED: &str = "what it would send isn't something the lamp can send by \
                                   itself; try saying exactly what to send, and to whom";

/// What the person is told of a write whose words aren't their own, or
/// whose recipient or place they didn't name (#6066).
pub const WRITE_NOT_SAID: &str = "what it would send, or where, isn't in your own words; try \
                                  saying the exact words to send and the address, like 'mail \
                                  mai@example.com saying I'm leaving now'";

/// What a repair is told of a write step.
const WRITE_ONLY: &str = "A write is its skill's only [[do]] step, write = { service = \"CODE\", \
                          action = \"ACTION\", ... } with only that kind's keys and every value \
                          written out, with [permissions] services = { CODE = \"write\" }";

/// What a repair is told of a write not from the request's own words.
const WRITE_FROM_REQUEST: &str = "Every subject, text and title is the request's own words, \
                                  copied exactly, never made up, read or remembered; every to, \
                                  cc and attendee an address the request says, or the person's \
                                  own";

/// A `write = { … }` table as the content `lamp-services` checks and
/// hashes: its keys, `service` and `action` among them, as JSON.
fn write_content(write: &toml::Value) -> Option<Value> {
    write.as_table()?;
    serde_json::to_value(write).ok().filter(Value::is_object)
}

/// The addresses a write's `key` lists: a list, or a comma-separated text.
fn addresses_of(content: &Value, key: &str) -> Vec<String> {
    match content.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(|one| one.trim().to_owned())
            .filter(|one| !one.is_empty())
            .collect(),
        Some(Value::String(text)) => text
            .split(',')
            .map(|one| one.trim().to_owned())
            .filter(|one| !one.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Every recipient of a write, or the place it goes when it has none: a
/// repository, a team, a project, a file, your calendar.
fn write_reaches(content: &Value) -> Vec<String> {
    let mut people = addresses_of(content, "to");
    people.extend(addresses_of(content, "cc"));
    people.extend(addresses_of(content, "attendees"));
    if people.is_empty() {
        let place = lamp_services::skill_writes::sent_to(content);
        if !place.is_empty() {
            people.push(place);
        }
    }
    people
}

/// Items said as one: "a", "a and b", "a, b and c".
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Said of every GitHub write before its yes, so the yes covers who may
/// see it: the repository's visibility is read only at the approval
/// (`lamp_services::skill_writes`), which says again "anyone can see" for
/// a public one.
pub const SEEN_WITH_THE_REPO: &str = "anyone who can see that repo can see it";

/// A write step (#6066) in plain words, whole: where it goes, every
/// recipient, and every text quoted in full, as `lamp-skills` sends it
/// (`lamp_skills::service_write`). None for a write the lamp can't send.
fn write_plainly(step: &toml::Value) -> Option<String> {
    let step = step.as_table().filter(|step| step.len() == 1)?;
    write_said(&write_content(step.get("write")?)?)
}

/// A write's content (as [`writes_of`] keeps it) in the say-back's plain
/// words: what the lamp must have said whole before its yes is taken
/// (`crate::skill_write_yes`, App Only's P1 on #6142).
pub fn write_said(content: &Value) -> Option<String> {
    lamp_services::skill_writes::check(content).ok()?;
    let text = |key: &str| content.get(key).and_then(Value::as_str);
    let quoted = |key: &str| text(key).map(|text| format!("“{text}”"));
    let saying = quoted("text")
        .map(|text| format!(", saying {text}"))
        .unwrap_or_default();
    let service = text("service")?;
    let name = service_name(service);
    Some(match (service, text("action")?) {
        (_, "send_mail") => {
            let mut said = format!(
                "mails {} from your {name}",
                and_list(&addresses_of(content, "to"))
            );
            let cc = addresses_of(content, "cc");
            if !cc.is_empty() {
                said.push_str(&format!(", copying {}", and_list(&cc)));
            }
            if let Some(subject) = quoted("subject") {
                said.push_str(&format!(", subject {subject}"));
            }
            said + &saying
        }
        ("google_calendar", _) => {
            let mut said = format!(
                "adds {} to your {name}",
                quoted("title").unwrap_or_else(|| "an event".into())
            );
            if let (Some(start), Some(end)) = (text("start"), text("end")) {
                said.push_str(&format!(" from {start} to {end}"));
            }
            let guests = addresses_of(content, "attendees");
            if !guests.is_empty() {
                said.push_str(&format!(", inviting {}", and_list(&guests)));
            }
            said + &saying
        }
        ("github", "comment") => format!(
            "comments on issue {} on {} on GitHub{saying}; {SEEN_WITH_THE_REPO}",
            content.get("number")?,
            text("repo")?
        ),
        ("github", _) => format!(
            "opens an issue {} on {} on GitHub{saying}; {SEEN_WITH_THE_REPO}",
            quoted("title")?,
            text("repo")?
        ),
        ("linear", _) => format!(
            "opens a Linear issue {} for team {}{saying}",
            quoted("title")?,
            text("team_id")?
        ),
        ("asana", _) => format!(
            "adds an Asana task {} to {}{saying}",
            quoted("title")?,
            match (text("project"), text("workspace")) {
                (Some(project), _) => format!("project {project}"),
                (None, Some(workspace)) => format!("workspace {workspace}"),
                (None, None) => return None,
            }
        ),
        (_, "comment") => format!("comments on Figma file {}{saying}", text("id")?),
        _ => return None,
    })
}

/// Words as a write's check compares them: lower case, curly apostrophes
/// made plain, split at anything but a letter, a digit or an apostrophe,
/// apostrophes at a word's ends dropped (a quote's marks).
fn words_of(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['’', '‘', '`'], "'")
        .split(|letter: char| !(letter.is_alphanumeric() || letter == '\''))
        .map(|word| word.trim_matches('\'').to_owned())
        .filter(|word| !word.is_empty())
        .collect()
}

/// Whether `part`'s words are said, one after another, in `whole`'s.
fn said_in(part: &[String], whole: &[String]) -> bool {
    !part.is_empty() && whole.windows(part.len()).any(|window| window == part)
}

/// Whether the address `address` is said in `request`: as written, or
/// spoken ("mai at example dot com").
fn address_said(address: &str, request: &str) -> bool {
    let address = address.trim().to_lowercase();
    if address.is_empty() {
        return false;
    }
    if request
        .to_lowercase()
        .split(|letter: char| letter.is_whitespace() || "<>()[],;\"'“”".contains(letter))
        .any(|word| word.trim_end_matches(['.', '!', '?', ':']) == address)
    {
        return true;
    }
    let spoken = address.replace('@', " at ").replace('.', " dot ");
    said_in(&words_of(&spoken), &words_of(request))
}

/// Whether `code`'s linked account is `account`'s service: the same code,
/// or either Gmail for the other.
fn same_service(code: &str, linked: &str) -> bool {
    code == linked
        || matches!(
            (code, linked),
            ("gmail", "gmail-app") | ("gmail-app", "gmail")
        )
}

/// A made skill's writes held to the request it was made from (#6066, the
/// first builder's conditions of 21:44): every subject, text and title is
/// the request's own words, copied (case, spacing and punctuation aside, a
/// quoted span included), never from a read, the web or memory; every
/// recipient (`to`, `cc`, `attendees`) is the linked account's own address
/// (`own`: each linked service's code and account) or an address the
/// request says; every place (a repository, a team, a project, a
/// workspace, a file, an issue's number, a calendar other than the
/// primary) is named in the request. Refused for a repair, its lines
/// naming the rule and the person's own addresses, never a value of the
/// request; the person hears [`WRITE_NOT_SAID`]. A skill with no write
/// passes.
pub fn write_from_request(
    text: &str,
    request: &str,
    own: &[(String, String)],
) -> Result<(), Refused> {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return Ok(());
    };
    let heard = words_of(request);
    let refused = |what: &str| {
        eprintln!("lamp-voice: a made skill's write isn't the request's own: its {what}");
        let mine: Vec<&str> = own
            .iter()
            .map(|(_, account)| account.as_str())
            .filter(|account| account.contains('@'))
            .collect();
        let mine = match mine.as_slice() {
            [] => String::new(),
            mine => format!(" (the person's own: {})", mine.join(", ")),
        };
        Err(Refused(
            WRITE_NOT_SAID.into(),
            Some(format!(
                "The lamp refused it: its {what} isn't from the request. {WRITE_FROM_REQUEST}{mine}."
            )),
        ))
    };
    let writes = table
        .get("do")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|step| step.get("write"))
        .filter_map(write_content);
    for content in writes {
        let code = content["service"].as_str().unwrap_or_default();
        for key in ["subject", "text", "title"] {
            if let Some(field) = content.get(key).and_then(Value::as_str)
                && !said_in(&words_of(field), &heard)
            {
                return refused(key);
            }
        }
        for key in ["repo", "team_id", "project", "workspace", "id", "calendar"] {
            let Some(place) = content.get(key).and_then(Value::as_str) else {
                continue;
            };
            if key == "calendar" && place == "primary" {
                continue;
            }
            if !said_in(&words_of(place), &heard) {
                return refused(key);
            }
        }
        if let Some(number) = content.get("number")
            && !heard.contains(&number.to_string())
        {
            return refused("number");
        }
        for key in ["to", "cc", "attendees"] {
            for address in addresses_of(&content, key) {
                let mine = own.iter().any(|(linked, account)| {
                    same_service(code, linked) && account.eq_ignore_ascii_case(address.trim())
                });
                if !mine && !address_said(&address, request) {
                    return refused(key);
                }
            }
        }
    }
    Ok(())
}

/// One `[[do]]` step in plain words: a light, a gesture, a say or the hum,
/// as the brief names them. A say is its exact words, quoted whole, so no
/// comma in them blurs into what is said after (the readers' P3 on #6062):
/// "says “Welcome back!”".
fn step_plainly(step: &toml::Value) -> Option<String> {
    let step = step.as_table().filter(|step| step.len() == 1)?;
    if let Some(say) = step.get("say") {
        return say_text(say).map(|text| format!("says “{text}”"));
    }
    if let Some(cue) = step.get("cue") {
        return sound_plainly(cue).map(str::to_owned);
    }
    // The camera's look and a stand (#6066): said, and left to
    // lamp-skills' check, which takes either only in a skill a person asks
    // for by its function.
    if let Some(look) = step.get("look") {
        return look
            .as_bool()?
            .then(|| "takes a look with the camera".to_owned());
    }
    if let Some(stand) = step.get("stand") {
        return Some(format!(
            "stands at {} percent of its height",
            stand.as_integer()?
        ));
    }
    if let Some(light) = step.get("light").and_then(|light| light.as_table()) {
        return light_plainly(light);
    }
    body_plainly(step.get("body")?.as_table()?)
}

/// How long a light is held when its trigger's end gives it back
/// (`until = "ends"`): " while that lasts".
fn until_plainly(light: &toml::Table) -> Option<&'static str> {
    match light.get("until") {
        None => Some(""),
        Some(until) if until.as_str() == Some("ends") => Some(" while that lasts"),
        Some(_) => None,
    }
}

/// A `light` step in plain words: a colour at a level for a while, how it
/// moves, a dim, the light out, or a shade warmer or dimmer than the
/// person's own (#6066), each as `lamp-skills` makes it. Any other key is
/// none, so unsaid.
fn light_plainly(light: &toml::Table) -> Option<String> {
    let until = until_plainly(light)?;
    let only = |known: &[&str]| light.keys().all(|key| known.contains(&key.as_str()));
    if let Some(dim) = light.get("dim") {
        if !only(&["dim", "until"]) {
            return None;
        }
        // `lamp_skills::light_call`'s dim: 1 to 4 steps, with only
        // `until = "ends"` beside it.
        return match dim.as_integer()? {
            1 => Some(format!("dims the light a step{until}")),
            steps @ 2..=4 => Some(format!("dims the light {steps} steps{until}")),
            _ => None,
        };
    }
    if let Some(out) = light.get("out") {
        if !only(&["out", "over", "until"]) || out.as_bool() != Some(true) {
            return None;
        }
        let over = match light.get("over") {
            Some(over) => format!(" slowly over {}", spell(over.as_str()?)?),
            None => String::new(),
        };
        return Some(format!("turns the light off{over}{until}"));
    }
    if let Some(relative) = light.get("relative").and_then(toml::Value::as_table) {
        return relative_plainly(light, relative);
    }
    if !only(&["colour", "level", "for", "pattern", "until", "no_brighter"]) {
        return None;
    }
    // A colour the light has and a level 1 to 5, as
    // `lamp_skills::light_call` takes them (lamp-9adf, 11 Nov 10:01: a
    // colour not in its list said here and refused by the check).
    let mut said = match light.get("colour") {
        Some(colour) => {
            let colour = colour.as_str()?;
            lamp_msgs::Light::COLOURS.contains(&colour).then_some(())?;
            format!("glows {colour}")
        }
        None => "turns the light".to_owned(),
    };
    if let Some(level) = light.get("level") {
        let level = level.as_integer()?;
        (1..=5).contains(&level).then_some(())?;
        said.push_str(&format!(" at level {level}"));
    }
    if let Some(lasting) = light.get("for") {
        said.push_str(&format!(" for {}", spell(lasting.as_str()?)?));
    }
    said.push_str(until);
    if let Some(pattern) = light.get("pattern") {
        said.push_str(match pattern.get("kind").and_then(toml::Value::as_str)? {
            "breathe" => ", breathing",
            "candle" => ", flickering like a candle",
            "pulse" => ", pulsing",
            _ => return None,
        });
    }
    if light.get("no_brighter").and_then(toml::Value::as_bool) == Some(true) {
        said.push_str(", never brighter than your light");
    }
    Some(said)
}

/// A light step `relative` to the person's light (#6066) in plain words,
/// only as `lamp_skills::light_call` takes it (lamp-9adf, 11 Nov 05:34: a
/// made "dim the light a step at ten" written with `until` passed here and
/// failed the check three times): beside it only a `for` of at most
/// [`RELATIVE_MOST_MS`], which it needs, and a breathe pattern with no
/// `low`; `warmer` 0 or 1 and `dimmer` 0 to 4, not both 0; and
/// `warm_at_dimmest = true` only with `warmer = 1`. Anything else is none,
/// so unsaid, and the repair is told how to write it ([`LIGHT_PART`]).
fn relative_plainly(light: &toml::Table, relative: &toml::Table) -> Option<String> {
    if light
        .keys()
        .any(|key| !["relative", "for", "pattern"].contains(&key.as_str()))
        || relative
            .keys()
            .any(|key| !["warmer", "dimmer", "warm_at_dimmest"].contains(&key.as_str()))
    {
        return None;
    }
    let steps = |key: &str| match relative.get(key) {
        None => Some(0),
        Some(steps) => steps.as_integer(),
    };
    let (warmer, dimmer) = (steps("warmer")?, steps("dimmer")?);
    if !(0..=1).contains(&warmer) || !(0..=4).contains(&dimmer) || warmer + dimmer == 0 {
        return None;
    }
    if let Some(warm) = relative.get("warm_at_dimmest")
        && (warm.as_bool() != Some(true) || warmer != 1)
    {
        return None;
    }
    let lasting = light.get("for")?.as_str()?;
    if whole_ms(lasting)? > RELATIVE_MOST_MS {
        return None;
    }
    let breathing = match light.get("pattern") {
        None => "",
        Some(pattern) => {
            let pattern = pattern.as_table()?;
            if pattern
                .keys()
                .any(|key| !["kind", "period"].contains(&key.as_str()))
                || pattern.get("kind")?.as_str()? != "breathe"
            {
                return None;
            }
            if let Some(period) = pattern.get("period") {
                let ms = u32::try_from(whole_ms(period.as_str()?)?).ok()?;
                if !lamp_msgs::LightPattern::BREATHE_MS.contains(&ms) {
                    return None;
                }
            }
            ", breathing"
        }
    };
    let mut parts = Vec::new();
    if warmer > 0 {
        parts.push("a shade warmer".to_owned());
    }
    match dimmer {
        0 => {}
        1 => parts.push("a step dimmer".to_owned()),
        steps => parts.push(format!("{steps} steps dimmer")),
    }
    Some(format!(
        "makes your light {} for {}{breathing}",
        parts.join(" and "),
        spell(lasting)?
    ))
}

/// The longest a `relative` light is held: `lamp_skills::light_call`'s
/// "a light's for is at most 10m".
const RELATIVE_MOST_MS: u64 = 10 * 60 * 1_000;

/// A skill's duration in whole milliseconds, as `lamp_skills::duration_us`
/// reads it: a whole number of ms, s, m or h.
fn whole_ms(text: &str) -> Option<u64> {
    let digits = text.trim_end_matches(|letter: char| letter.is_ascii_alphabetic());
    let per_unit: u64 = match &text[digits.len()..] {
        "ms" => 1,
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        _ => return None,
    };
    digits.parse::<u64>().ok()?.checked_mul(per_unit)
}

/// A `body` step in plain words: a gesture by name ([`GESTURES`]), a turn
/// to the face in view, and the reading pose held a while (#6066), each as
/// `lamp-skills` makes them, in its order. Any other key is none.
fn body_plainly(body: &toml::Table) -> Option<String> {
    if body
        .keys()
        .any(|key| !["gesture", "look_at", "mood", "for"].contains(&key.as_str()))
    {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(look_at) = body.get("look_at") {
        (look_at.as_str()? == "face").then_some(())?;
        parts.push("turns to the face it sees".to_owned());
    }
    if let Some(gesture) = body.get("gesture") {
        parts.push(gesture_said(gesture.as_str()?)?.to_owned());
    }
    // As `lamp-skills` takes them: a mood with its for, more than 0 and at
    // most 2m, and a for only with a mood (lamp-74ff, 11 Nov 09:48: a nod
    // with a for, which it refuses).
    match (body.get("mood"), body.get("for")) {
        (None, None) => {}
        (Some(mood), Some(lasting)) if mood.as_str() == Some("reading") => {
            let lasting = lasting.as_str()?;
            if !(1..=MOOD_MOST_MS).contains(&whole_ms(lasting)?) {
                return None;
            }
            parts.push(format!("holds its reading pose for {}", spell(lasting)?));
        }
        _ => return None,
    }
    (!parts.is_empty()).then(|| parts.join(" and "))
}

/// The longest a mood is held: `lamp-skills`' "a mood's for is more than 0
/// and at most 2m".
const MOOD_MOST_MS: u64 = 2 * 60 * 1_000;

/// Each gesture a made skill may name, one a step, as the person hears it:
/// the body's clips that `lamp_skills::gesture_known` takes (all of
/// `lamp_body::clips::ALL` but `idle`, the body's own), said by what each
/// does (docs/body.md's tables). A clip with no saying here is refused, so
/// one added to the body is never played unsaid; a test holds every clip
/// to one.
const GESTURES: [(&str, &str); 20] = [
    ("nod", "nods"),
    ("headshake", "shakes its head"),
    ("greeting", "lifts its head and arm in a greeting"),
    ("laugh", "tips its head back in a laugh"),
    ("shy", "dips its head shyly"),
    ("stretching", "stretches its arm up"),
    ("acknowledge", "gives a quick bow"),
    ("curious", "looks up, curious"),
    ("happy", "wiggles happily"),
    ("caring", "droops slowly and gently"),
    ("surprised", "recoils in surprise"),
    ("confused", "dips and turns, puzzled"),
    ("sleepy", "droops sleepily"),
    ("excited", "bounces up twice, excited"),
    ("scanning", "looks all round the room"),
    ("dance_chill", "sways in a slow dance"),
    ("dance_groove", "bounces in a groove dance"),
    ("dance_hype", "dances fast with its whole body"),
    ("dance_waltz", "leans in a slow waltz"),
    ("dance_jazz", "bobs its head in a jazz dance"),
];

/// A gesture as the person hears it, if a made skill may name it: one of
/// the body's clips but `idle` (`lamp_skills::gesture_known`, which this
/// crate can't call: lamp-skills depends on it), with a saying.
fn gesture_said(name: &str) -> Option<&'static str> {
    if name == "idle" || lamp_body::clips::named(name).is_none() {
        return None;
    }
    GESTURES
        .iter()
        .find(|(clip, _)| *clip == name)
        .map(|(_, said)| *said)
}

/// The gestures a made skill may name, for the making model:
/// "nod, headshake, …".
pub fn gesture_names() -> String {
    GESTURES
        .iter()
        .filter(|(name, _)| gesture_said(name).is_some())
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A skill's duration, `"20s"`, `"1m"`, `"12h"`: its count and its unit.
fn duration(text: &str) -> Option<(u64, &'static str)> {
    let unit = match text.chars().last()? {
        's' => "second",
        'm' => "minute",
        'h' => "hour",
        _ => return None,
    };
    let count: u64 = text[..text.len() - 1].parse().ok()?;
    (count > 0).then_some((count, unit))
}

/// `"a minute"`, `"20 seconds"`.
fn spell(text: &str) -> Option<String> {
    let (count, unit) = duration(text)?;
    Some(match count {
        1 => one(unit).to_owned(),
        _ => format!("{count} {unit}s"),
    })
}

fn one(unit: &str) -> &'static str {
    match unit {
        "second" => "a second",
        "minute" => "a minute",
        _ => "an hour",
    }
}

/// Who asked for a made skill, written on its card as `made_by: <key>`
/// beside `made: here` ([`MADE_BY`]), so `lamp-skills` counts its runs by
/// maker from the card alone (`lamp_skills::usage`, the opt-in health
/// report). An enum only: never a person's name or a word of the ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Maker {
    /// Asked for by voice (`make_skill`).
    Voice,
    /// Asked for in the owner's app (`crate::app_make`).
    App,
}

/// The card's key for [`Maker`], in `SKILL.md`'s front matter.
pub const MADE_BY: &str = "made_by";

impl Maker {
    /// The maker of a skill kept as made from `from`: the app's when its
    /// second part is `app` (`local:app:<ms>:1`, an app's ask made here; a
    /// written one's `written:app:…` too), the voice's otherwise
    /// (`local:<ms>`).
    pub fn of(from: &str) -> Self {
        match from.split(':').nth(1) {
            Some("app") => Self::App,
            _ => Self::Voice,
        }
    }

    /// As the card says it.
    pub fn key(self) -> &'static str {
        match self {
            Self::Voice => "voice",
            Self::App => "app",
        }
    }
}

/// A `SKILL.md`'s name, and the card marked `made: here` (`lamp-cloud`'s
/// "Made on this lamp" group) and `made_by: <maker>` ([`Maker`]) with
/// `description` ([`plainly`]'s) in place of the one written. A `made` or
/// `made_by` line the model wrote is dropped: only the lamp says them.
fn card(text: &str, description: &str, maker: Maker) -> Result<(String, String), Refused> {
    card_asked(text, description, maker, false)
}

/// The front matter line, after `made_by`, of a made skill whose request
/// asked for limits ([`limits_asked`]): with [`LOOSE_ENVIRONMENT`] on,
/// `lamp-skills` never ignores the lamp's default limits on it as it
/// loads, since the person asked for them (`lamp_skills::made_by_ask`).
pub const LIMITS_ASKED: &str = "limits: asked";

/// [`card`], with [`LIMITS_ASKED`] after its maker when the request asked
/// for limits (`limits`). A `limits` line the model wrote is dropped, as a
/// `made` one is.
fn card_asked(
    text: &str,
    description: &str,
    maker: Maker,
    limits: bool,
) -> Result<(String, String), Refused> {
    let asked = if limits {
        format!("\n{LIMITS_ASKED}")
    } else {
        String::new()
    };
    card_marked(
        text,
        description,
        &format!("made: here\n{MADE_BY}: {}{asked}", maker.key()),
    )
}

/// [`card`] with `marks`, the front matter's last lines: `made: here` and
/// its maker, [`LIMITS_ASKED`] when asked, and for a skill added from a
/// file [`ADDED_FROM_FILE`] too. Any such line the file had is taken out
/// first.
fn card_marked(text: &str, description: &str, marks: &str) -> Result<(String, String), Refused> {
    let unread = || Refused::new("it has no name and description");
    let rest = text.strip_prefix("---\n").ok_or_else(unread)?;
    let (front, body) = rest.split_once("\n---").ok_or_else(unread)?;
    let field = |key: &str| {
        front.lines().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(':'))
                .map(|value| value.trim().trim_matches('"').to_owned())
        })
    };
    let name = field("name").ok_or_else(unread)?;
    if field("description").is_none_or(|written| written.is_empty()) {
        return Err(unread());
    }
    if !(1..=64).contains(&name.len())
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(Refused::new("its name isn't a skill's name"));
    }
    // The written description goes whole, its continuation lines too
    // (indented, YAML's): none of the model's words stay in the package.
    let mut kept: Vec<&str> = Vec::new();
    let mut in_description = false;
    for line in front.lines() {
        let continued = in_description && line.starts_with([' ', '\t']);
        in_description = continued || line.starts_with("description:");
        let marker = line
            .split_once(':')
            .is_some_and(|(key, _)| matches!(key.trim(), "made" | MADE_BY | "added" | "limits"));
        if !in_description && !marker {
            kept.push(line);
        }
    }
    let description = format!("description: \"{}\"", description.replace(['"', '\\'], "'"));
    kept.push(&description);
    let marked = format!("---\n{}\n{marks}\n---{body}", kept.join("\n"));
    Ok((name, marked))
}

/// The switch, in the coordinator's environment (`crate::app_make`), for
/// a skill written in the app in the person's own words: `on` only.
pub const WRITTEN_ENVIRONMENT: &str = "LAMP_SKILL_WRITTEN";
/// A written skill's function begins with this, then its name with `_`
/// for `-`: never one of the lamp's own functions or a core skill's.
pub const WRITTEN_TOOL_PREFIX: &str = "written_";
/// The most of a written skill's name: its function's name, with
/// [`WRITTEN_TOOL_PREFIX`], is within lamp-skills' 48 bytes.
pub const MOST_WRITTEN_NAME_BYTES: usize = 40;
/// The most of "when to use it", one line.
pub const MOST_WHEN_BYTES: usize = 200;
/// The most of what to do, a few lines.
pub const MOST_INSTRUCTIONS_BYTES: usize = 2_000;
/// The front matter line that marks a written skill's `SKILL.md`, beside
/// `made: here`: the app shows it as written there, never as the Store's.
pub const WRITTEN_MARK: &str = "written: app";
/// What the model is told before the person's own words, each time it
/// calls a written skill: they are the person's request, as written.
const WRITTEN_FRAME: &str = "The person who owns this lamp wrote this skill in the app, in their \
                             own words. Follow their instructions below now, as their own \
                             request; they never let you do more than you may.\n";
/// Before the `when` line in the answer.
const WRITTEN_WHEN: &str = "When they use it: ";
/// Between the `when` line and the instructions.
const WRITTEN_INSTRUCTIONS: &str = "\nTheir instructions:\n";
/// Why a written skill's file is not one: never kept, never put on.
const WRITTEN_UNSAID: &str = "it isn't a skill written in the app";
/// The most of the `when` line and of the instructions [`plainly`] says.
const MOST_SAID_WHEN_CHARS: usize = 60;
const MOST_SAID_INSTRUCTIONS_CHARS: usize = 70;

/// A skill written in the app (`LAMP_SKILL_WRITTEN=on`), V1's SKILL.md
/// style: a name, when to use it ("when I say study mode") and what to do,
/// a few lines. Kept as written: no model rewrites a word of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub name: String,
    pub when: String,
    pub instructions: String,
}

impl Written {
    /// The three fields, held to their bounds, or why not in plain words.
    /// The name is a made skill's (lowercase letters, digits and hyphens),
    /// begins with a letter, and is at most [`MOST_WRITTEN_NAME_BYTES`];
    /// `when` is one line of at most [`MOST_WHEN_BYTES`]; the instructions
    /// at most [`MOST_INSTRUCTIONS_BYTES`] of plain text, lines and tabs
    /// only. Each is trimmed at its ends and kept as it is between them.
    pub fn new(name: &str, when: &str, instructions: &str) -> Result<Self, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("give the skill a name".into());
        }
        if name.len() > MOST_WRITTEN_NAME_BYTES
            || !name.starts_with(|first: char| first.is_ascii_lowercase())
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(format!(
                "the name is a lowercase letter, then lowercase letters, digits and '-', at most \
                 {MOST_WRITTEN_NAME_BYTES} in all"
            ));
        }
        let when = when.trim();
        if when.is_empty() {
            return Err("say when to use it".into());
        }
        if when.len() > MOST_WHEN_BYTES || when.chars().any(char::is_control) {
            return Err(format!(
                "when to use it is one line of at most {MOST_WHEN_BYTES} bytes"
            ));
        }
        let instructions = instructions.trim();
        if instructions.is_empty() {
            return Err("say what the lamp should do".into());
        }
        if instructions.len() > MOST_INSTRUCTIONS_BYTES
            || instructions
                .chars()
                .any(|letter| letter.is_control() && !matches!(letter, '\n' | '\t'))
        {
            return Err(format!(
                "what the lamp should do is plain text of at most {MOST_INSTRUCTIONS_BYTES} bytes"
            ));
        }
        Ok(Self {
            name: name.to_owned(),
            when: when.to_owned(),
            instructions: instructions.to_owned(),
        })
    }

    /// Its function's name: [`WRITTEN_TOOL_PREFIX`] and the name, `_` for
    /// `-`.
    pub fn tool(&self) -> String {
        format!("{WRITTEN_TOOL_PREFIX}{}", self.name.replace('-', "_"))
    }

    /// What the model is told when it calls it: [`WRITTEN_FRAME`], the
    /// `when` line and the instructions, word for word.
    fn note(&self) -> String {
        format!(
            "{WRITTEN_FRAME}{WRITTEN_WHEN}{}{WRITTEN_INSTRUCTIONS}{}",
            self.when, self.instructions
        )
    }

    /// Its `skill.toml`: an installable skill whose only part is a
    /// function for the model, with no trigger (it never starts by
    /// itself), that may only answer the call with a note: the
    /// instructions, as lamp-skills gives a tool's `say` back to the model
    /// (`{"next": NOTE}`). No light, no body, no topic, no camera, no code.
    pub fn toml(&self) -> String {
        format!(
            "kind = \"installable\"\nversion = \"1.0.0\"\ntool = {}\n\n[permissions]\nspeak = \
             true\n\n[[do]]\nsay = {}\n",
            toml_string(&self.tool()),
            toml_string(&self.note())
        )
    }

    /// Its `SKILL.md`: the name, `description` ([`plainly`]'s, which names
    /// the `when` line and is what the model is told of when to call it),
    /// `made: here` and [`WRITTEN_MARK`]; the instructions, as written,
    /// are its body.
    fn card(&self, description: &str) -> String {
        format!(
            "---\nname: {}\ndescription: \"{}\"\nmade: here\n{WRITTEN_MARK}\n---\n{}\n",
            self.name,
            description.replace(['"', '\\'], "'"),
            self.instructions
        )
    }
}

/// `text` as a TOML basic string: `"`, `\`, a new line and a tab escaped.
/// [`Written::new`] lets no other control character in.
fn toml_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for letter in text.chars() {
        match letter {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\t' => quoted.push_str("\\t"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// A written skill's `skill.toml`, read back: its `when` line and its
/// instructions, if it is exactly the shape [`Written::toml`] writes
/// (the kind, a version, a function of [`WRITTEN_TOOL_PREFIX`],
/// `[permissions]` with `speak` alone, and one step, the note); else none.
fn written_of(table: &toml::Table) -> Option<(String, String)> {
    let keys = ["kind", "version", "tool", "permissions", "do"];
    if table.len() != keys.len() || keys.iter().any(|key| !table.contains_key(*key)) {
        return None;
    }
    if table["kind"].as_str() != Some("installable") || table["version"].as_str().is_none() {
        return None;
    }
    table["tool"]
        .as_str()
        .filter(|tool| tool.starts_with(WRITTEN_TOOL_PREFIX))?;
    let permissions = table["permissions"].as_table()?;
    if permissions.len() != 1 || permissions.get("speak") != Some(&toml::Value::Boolean(true)) {
        return None;
    }
    let [step] = table["do"].as_array()?.as_slice() else {
        return None;
    };
    let step = step.as_table().filter(|step| step.len() == 1)?;
    let (when, instructions) = step
        .get("say")?
        .as_str()?
        .strip_prefix(WRITTEN_FRAME)?
        .strip_prefix(WRITTEN_WHEN)?
        .split_once(WRITTEN_INSTRUCTIONS)?;
    // Held to the same bounds as when written: none other is kept.
    let held = Written::new("x", when, instructions).ok()?;
    (held.when == when && held.instructions == instructions)
        .then(|| (when.to_owned(), instructions.to_owned()))
}

/// A written skill's `skill.toml` held to what one may use: its version,
/// and its voice alone. Anything else is refused.
pub fn written_capped(text: &str) -> Result<(String, Uses), Refused> {
    let table: toml::Table =
        toml::from_str(text).map_err(|_| Refused::new("its skill.toml can't be read"))?;
    written_of(&table).ok_or_else(|| Refused::new(WRITTEN_UNSAID))?;
    let version = table["version"].as_str().unwrap_or_default().to_owned();
    Ok((
        version,
        Uses {
            speak: true,
            ..Uses::default()
        },
    ))
}

/// The cap a made skill is held to again as it is put on (lamp-skills'
/// manage helper, `install_made`): a written skill's ([`written_capped`])
/// when it gives the model a function, else a made one's ([`capped`]).
pub fn put_on_cap(text: &str) -> Result<(String, Uses), Refused> {
    let has_tool =
        toml::from_str::<toml::Table>(text).is_ok_and(|table| table.contains_key("tool"));
    if has_tool {
        written_capped(text)
    } else {
        capped(text)
    }
}

/// A made package's other files held again as it is put on (lamp-skills'
/// manage helper, `install_made`; the Reviewer-lead's P3 on #6355), beside
/// [`put_on_cap`]'s `uses` for its `skill.toml`: every file but its two is
/// its code, as [`files`] took it (a script's name and ending, plain text,
/// at most [`MOST_SCRIPT_BYTES`] each, [`MOST_SCRIPTS`] and
/// [`MOST_SCRIPTS_BYTES`] in all), only with [`SCRIPTS_ENVIRONMENT`] on, and
/// matched to its `task` step ([`code_matches`]).
pub fn put_on_scripts<'a>(
    toml: &str,
    uses: &Uses,
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<(), Refused> {
    let mut scripts: Vec<(String, String)> = Vec::new();
    for (path, bytes) in files {
        if path == "skill.toml" || path == "SKILL.md" {
            continue;
        }
        if !script_path_ok(path) {
            return Err(scripts_not_held("a file's name or ending"));
        }
        let Some(text) = std::str::from_utf8(bytes)
            .ok()
            .filter(|text| script_text_ok(text))
        else {
            return Err(scripts_not_held("a file that isn't plain text"));
        };
        if bytes.len() > MOST_SCRIPT_BYTES {
            return Err(scripts_not_held("more code than a made skill may carry"));
        }
        scripts.push((path.to_owned(), text.to_owned()));
    }
    if !scripts.is_empty() && !scripts_on() {
        return Err(Refused::new(cant_yet(CODE_OF_ITS_OWN)));
    }
    if scripts.len() > MOST_SCRIPTS {
        return Err(scripts_not_held("more files than a made skill may carry"));
    }
    if scripts.iter().map(|(_, text)| text.len()).sum::<usize>() > MOST_SCRIPTS_BYTES {
        return Err(scripts_not_held("more code than a made skill may carry"));
    }
    let files = Files {
        toml: toml.to_owned(),
        card: String::new(),
        left_out: None,
        scripts,
    };
    code_matches(&files, uses)
}

/// What a written skill does, in plain words: "When you ask for it (when I
/// say study mode), the lamp follows the instructions you wrote: Dim the
/// light…". Its `when` and its instructions are cut short, never left out.
fn written_plainly(table: &toml::Table) -> Option<String> {
    let (when, instructions) = written_of(table)?;
    let when = cut_words(&when, MOST_SAID_WHEN_CHARS);
    let short = cut_words(&instructions, MOST_SAID_INSTRUCTIONS_CHARS);
    let end = if short.ends_with(['.', '!', '?', '…']) {
        ""
    } else {
        "."
    };
    Some(format!(
        "When you ask for it ({when}), the lamp follows the instructions you wrote: {short}{end}"
    ))
}

/// Keep the skill `written` in the app, from the ask `from`: its two files
/// written here from the person's words ([`Written::toml`],
/// [`Written::card`]), held to [`written_capped`], checked and packed by
/// `lamp-skills pack`, and kept as a made skill is, at most [`KEPT_MOST`].
/// A name any skill on the lamp has (in `installed`, pushed beside it, or
/// kept here and on) gives the first free one ([`free_name`]); a kept one
/// of that name not on is replaced. Kept once:
/// asked again, the kept one is given back. Nothing is put on here: the
/// caller puts it on next ([`put_on`]).
pub fn keep_written(
    written: &Written,
    from: &str,
    keeping: &Keeping,
    installed: &Path,
) -> Result<Made, Refused> {
    let unkept = |why: io::Error| {
        eprintln!("lamp-voice: a written skill was not kept: {why}");
        Refused::new(NOT_KEPT_HERE)
    };
    ensure_private(&keeping.directory).map_err(unkept)?;
    drop_unfinished(&keeping.directory);
    if let Some(made) = kept(&keeping.directory, from) {
        return Ok(made);
    }
    // A name a skill on the lamp has: the first free one, or "you already
    // have that skill" for the person's own of the same words ([`free_name`]).
    let free = free_name(
        keeping,
        installed,
        &written.name,
        &written.toml(),
        MOST_WRITTEN_NAME_BYTES,
    )?;
    let written = &Written {
        name: free,
        ..written.clone()
    };
    let name = &written.name;
    let toml = written.toml();
    let (version, uses) = written_capped(&toml)?;
    let said = plainly(&toml)?;
    let card = written.card(&said);
    let made = packed_and_kept(
        keeping,
        from,
        Card {
            toml: &toml,
            name: name.clone(),
            marked: card,
            version,
            uses,
            scripts: &[],
        },
        Ok(said),
        false,
    )?;
    // A kept one of that name not on the lamp gives way to the person's
    // newer words (Seventh's P2 on #5072): the new one goes on in its place.
    if let Err(why) = forget_others(&keeping.directory, name, from) {
        eprintln!("lamp-voice: an older kept skill of that name stays: {why}");
    }
    Ok(made)
}

/// Where pushed skills are, beside `installed` (lamp-skills' `PUSHED_DIR`).
pub(crate) fn pushed(installed: &Path) -> PathBuf {
    installed.with_file_name("skills-dev")
}

/// [`pushed`], for a made skill's clash (`crate::made_clash`).
pub fn pushed_folder(installed: &Path) -> PathBuf {
    pushed(installed)
}

/// The made skill called `name` taken off the lamp (`crate::made_clash`'s
/// replace, once the manage helper removed it): dropped from the index,
/// its package with it, so it is never told as on again.
pub fn forget_named(directory: &Path, name: &str) {
    if let Err(error) = forget_others(directory, name, "") {
        eprintln!("lamp-voice: a replaced made skill wasn't dropped from the index: {error}");
    }
}

/// The made skill `sha256`, kept and not on, dropped from the index, its
/// package with it unless another entry names it (`crate::made_clash`'s
/// cancel, or a clash never answered): never told as made and off.
pub fn forget_unput(directory: &Path, sha256: &str) {
    let (gone, entries): (Vec<Value>, Vec<Value>) = index(directory)
        .into_iter()
        .partition(|entry| entry["sha256"] == sha256 && entry["on"] != true);
    if gone.is_empty() {
        return;
    }
    if let Err(error) = write_index(directory, &entries) {
        eprintln!("lamp-voice: a made skill not put on wasn't dropped from the index: {error}");
        return;
    }
    drop_packages(directory, gone, &entries);
}

/// Drop from the index every kept skill called `name` but the one from
/// `from`, their packages with them.
fn forget_others(directory: &Path, name: &str, from: &str) -> io::Result<()> {
    let (gone, entries): (Vec<Value>, Vec<Value>) = index(directory)
        .into_iter()
        .partition(|entry| entry["name"] == name && entry["from"] != from);
    if gone.is_empty() {
        return Ok(());
    }
    write_index(directory, &entries)?;
    drop_packages(directory, gone, &entries);
    Ok(())
}

/// The packages of the index entries `gone`, each unless one of `entries`,
/// still kept, names it.
fn drop_packages(directory: &Path, gone: Vec<Value>, entries: &[Value]) {
    for entry in gone {
        // The same words make the same package: kept while one still names it.
        if let Some(package) = entry["package"].as_str().filter(|package| {
            !package.is_empty()
                && !package.contains('/')
                && !package.starts_with('.')
                && !entries.iter().any(|kept| kept["package"] == *package)
        }) {
            let _ = fs::remove_file(directory.join(package));
        }
    }
}

/// Where made skills are kept, the program that checks them, and the
/// manage helper's socket that puts a kept one on ([`put_on`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keeping {
    pub directory: PathBuf,
    pub program: PathBuf,
    pub manage: PathBuf,
    /// The lamp's installed skills (`crate::skill_words::INSTALLED`): a
    /// made skill's name is never one of theirs ([`free_name`]).
    pub installed: PathBuf,
}

/// Whether the switch is on in this process's environment: the
/// coordinator's, which systemd gives it, and the second worker's, which
/// is given it (`process::worker_command`).
pub fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

impl Keeping {
    /// The product's, with the switch on in this process's environment
    /// ([`from_environment`]).
    pub fn from_environment() -> Option<Self> {
        Self::when(from_environment())
    }

    /// The product's, if `on`.
    pub fn when(on: bool) -> Option<Self> {
        on.then(|| Self {
            directory: PathBuf::from(DIRECTORY),
            program: PathBuf::from(PROGRAM),
            manage: PathBuf::from(MANAGE_SOCKET),
            installed: PathBuf::from(crate::skill_words::INSTALLED),
        })
    }
}

/// A made skill, checked and kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    pub name: String,
    pub version: String,
    /// The package's SHA-256, lowercase hex: what the manage helper puts
    /// on, those bytes alone.
    pub sha256: String,
    pub description: String,
    pub uses: Uses,
    pub package: PathBuf,
}

/// Take a made skill from the model's `answer` (`cut`: the answer was cut
/// before it came here), hold it to the cap, check and pack it with
/// `lamp-skills pack`, and keep the package, at most [`KEPT_MOST`]. The
/// answer made as `from` (`local:…`) is kept once: asked again,
/// the kept one is given back.
pub fn keep(answer: &str, cut: bool, from: &str, keeping: &Keeping) -> Result<Made, Refused> {
    keep_asked(answer, cut, from, keeping, &Asked::default())
}

/// The person's request a skill is made from, as [`keep_asked`] holds it
/// to: its words, and each linked service's code and account (a write's
/// own address, #6066).
#[derive(Clone, Debug, Default)]
pub struct Asked<'a> {
    pub request: &'a str,
    pub own: Vec<(String, String)>,
    /// The line heard for a make by voice, the person's own words: a
    /// write's words and recipients are held to it, never to the model's
    /// `request` argument, which may carry a read's (App Only's P1 on
    /// #6142). `None` for the app's typed ask, held to `request`.
    pub heard: Option<&'a str>,
}

/// [`keep`], with the person's request: the times of day it named
/// ([`asked_windows`]), so a `during` within one runs as said, at night
/// too, with no quiet hours and any other keeps the night quiet; and a
/// write held to its words ([`write_from_request`], #6066).
pub fn keep_asked(
    answer: &str,
    cut: bool,
    from: &str,
    keeping: &Keeping,
    request: &Asked<'_>,
) -> Result<Made, Refused> {
    let asked = &asked_windows(request.request)[..];
    let unkept = |why: io::Error| {
        eprintln!("lamp-voice: a made skill was not kept: {why}");
        Refused::new(NOT_KEPT_HERE)
    };
    ensure_private(&keeping.directory).map_err(unkept)?;
    drop_unfinished(&keeping.directory);
    if let Some(made) = kept(&keeping.directory, from) {
        return Ok(made);
    }
    if cut {
        return Err(Refused::new(MODEL_CUT_OFF));
    }
    let mut files = files(answer)?;
    // A getting up's wait, which [`tidied`] takes out: said with what it
    // does, never dropped without a word (App Only's P2 on #6249).
    let waited = dropped_wait(&files.toml);
    let blinked = blink_gapped(&files.toml);
    // A say's pace set, never refused (#6062): the text checked, said and
    // packed is the paced one.
    // A repeat in the request's words, or the heard line's (read with
    // [`LOOSE_ENVIRONMENT`] on alone).
    let repeat = repeat_asked(request.request).or_else(|| request.heard.and_then(repeat_asked));
    let limits = limits_asked(request.request);
    files.toml = paced_for(&files.toml, asked, limits, repeat);
    let waited = waited.filter(|_| dropped_wait(&files.toml).is_none());
    // A blink's gap, which [`tidied`] takes out: the blinking said as left
    // out, with whatever else the model left out.
    if blinked
        && !blink_gapped(&files.toml)
        && !files
            .left_out
            .as_deref()
            .is_some_and(|what| what.to_lowercase().contains("blink"))
    {
        files.left_out = Some(match files.left_out.take() {
            Some(what) => cut_chars(&format!("blinking, {what}"), MOST_CANNOT_CHARS),
            None => "blinking".to_owned(),
        });
    }
    let (_, uses) = capped(&files.toml)?;
    // Its code only with a task step to run it, and that step's file one
    // it wrote (#6307).
    code_matches(&files, &uses)?;
    // Each .py it wrote Python that parses, before its check and its
    // say-back (#6749 E): one that doesn't goes to a repair.
    scripts_parse(&files.scripts)?;
    // A write's words and recipients are the request's own (#6066): the
    // heard line's, for a make by voice.
    if !uses.writes.is_empty() {
        let words = request.heard.unwrap_or(request.request);
        write_from_request(&files.toml, words, &request.own)?;
    }
    let ending = Ending {
        files: &files,
        waited: waited.as_deref(),
        asked,
        limits,
        from,
        keeping,
    };
    let made = ending.checked_and_kept(&files.toml, &[]);
    // Loosened ([`LOOSE_ENVIRONMENT`]): what the check refused is left out
    // and told, before any repair, and refused only when nothing is left.
    // Never a skill that writes: its content is said whole for its yes.
    match &made {
        Err(Refused(why, Some(_)))
            if why == NOT_PASSED
                && loose_on()
                && uses.writes.is_empty()
                && files.scripts.is_empty() =>
        {
            match loosened(&files.toml, &files.card, from, keeping) {
                Some((toml, out)) => ending.checked_and_kept(&toml, &out),
                None => made,
            }
        }
        _ => made,
    }
}

/// What [`keep_asked`] ends with, for the answer's `skill.toml` or the one
/// [`loosened`] gives.
struct Ending<'a> {
    files: &'a Files,
    waited: Option<&'a str>,
    asked: &'a [(u32, u32)],
    /// Whether the request asked for limits ([`limits_asked`]), marked on
    /// its card ([`LIMITS_ASKED`]).
    limits: bool,
    from: &'a str,
    keeping: &'a Keeping,
}

impl Ending<'_> {
    /// `toml` held to the cap, said plainly with what was `left` out
    /// ([`left_out_said`]), checked, packed and kept.
    fn checked_and_kept(&self, toml: &str, left: &[String]) -> Result<Made, Refused> {
        let Self {
            files,
            waited,
            asked,
            limits,
            from,
            keeping,
        } = *self;
        let (version, uses) = capped(toml)?;
        // Said only once the check has passed: its lines, which name the
        // field, go to a repair before these do.
        let said = said_mannered(toml, asked).map(|does| {
            let does = match waited {
                Some(wait) => format!("{does} {}", cant_wait(wait)),
                None => does,
            };
            let does = match &files.left_out {
                Some(what) => format!("{does} {}", left_out(what)),
                None => does,
            };
            match left {
                [] => does,
                left => format!("{does} {}", left_out_said(left)),
            }
        });
        // A dim or `relative` light [`plainly`] refused is one the check
        // refuses too, as it says them alike: its plain hint ([`LIGHT_PART`])
        // goes to the repair first, not the check's own words (lamp-9adf, 11
        // Nov 05:34: three repairs told only "relative takes only for and a
        // breathe pattern" never wrote one the check took).
        // A body step's alike (`BODY_PART`, lamp-74ff, 11 Nov 09:48), a
        // colour's and a step with more than one kind (lamp-9adf, 11 Nov
        // 10:01): [`hinted_first`].
        if let Err(refused) = &said
            && refused.1.as_deref().is_some_and(hinted_first)
        {
            return Err(refused.clone());
        }
        let (name, marked) = card_asked(
            &files.card,
            said.as_deref().unwrap_or_default(),
            Maker::of(from),
            limits,
        )?;
        // A name a skill on the lamp has already (a factory reset keeps the
        // lamp's own, welcome-back among them): the first free one, or "you
        // already have that skill" for the person's own of the same words.
        let free = free_name(
            keeping,
            &keeping.installed,
            &name,
            &made_words(toml, &files.scripts),
            MOST_NAME_BYTES,
        )?;
        let marked = if free == name {
            marked
        } else {
            renamed_card(&marked, &free)
        };
        packed_and_kept(
            keeping,
            from,
            Card {
                toml,
                name: free,
                marked,
                version,
                uses,
                scripts: &files.scripts,
            },
            said,
            false,
        )
    }
}

/// The most checks [`loosened`] runs for one skill.
const MOST_LOOSE_TRIALS: usize = 32;
/// The most time [`loosened`] takes, its checks together: well within a
/// make's, and far shorter than one repair by the model.
const LOOSE_WITHIN: Duration = Duration::from_secs(6);
/// How often [`loosened`] looks whether a check has ended.
const LOOSE_POLL: Duration = Duration::from_millis(5);

/// One part [`loosened`] may leave out of a made skill: a key of a step or
/// a whole step, never its last step, a read or a write. Never a key of its
/// trigger, nor a key inside a step that sends or writes, or uses the
/// camera or the microphone ([`named_exactly`]): what the check refuses
/// there stays refused, as before (Heron's P2 on #6437).
/// Keys that say when a part runs, never taken out alone by [`loosened`]
/// (the Reviewer-lead's P3 on #6437): a step's guards (`on`, `if_choice`,
/// `after`) and every trigger key, wherever they are written. Whole steps
/// may still go.
const GUARD_KEYS: [&str; 12] = [
    "on",
    "if_choice",
    "after",
    "equals",
    "above",
    "below",
    "field",
    "days",
    "when",
    "cooldown",
    "rearm",
    "during",
];

#[derive(Clone, Debug, PartialEq, Eq)]
enum Loose {
    /// A key of a step's kind: `light = { out = true, for = "1s" }`'s `for`.
    KindKey {
        step: usize,
        kind: String,
        key: String,
    },
    /// One kind of a step with more than one: `{ light = …, body = … }`'s
    /// `body`.
    StepKind { step: usize, kind: String },
    /// A whole `[[do]]` step.
    Step(usize),
}

impl Loose {
    /// Every part of `table` that may be left out, keys first, then whole
    /// steps.
    fn of(table: &toml::Table) -> Vec<Self> {
        let steps: &[toml::Value] = table
            .get("do")
            .and_then(toml::Value::as_array)
            .map_or(&[], Vec::as_slice);
        let open = |step: &toml::Value| {
            step.as_table()
                .filter(|step| !step.contains_key("write") && !step.contains_key("read"))
                .cloned()
        };
        let mut parts = Vec::new();
        for (index, whole) in steps.iter().enumerate() {
            let Some(step) = open(whole) else {
                continue;
            };
            // Its keys only when nothing in it must be named exactly: a
            // look's `for` never goes alone. A guard never goes alone
            // either ([`GUARD_KEYS`]): the step would run where it didn't.
            let kinds: Vec<&String> = step
                .keys()
                .filter(|kind| !GUARD_KEYS.contains(&kind.as_str()))
                .collect();
            if !named_exactly(whole) {
                for kind in &kinds {
                    let keys = step[kind.as_str()]
                        .as_table()
                        .into_iter()
                        .flat_map(toml::Table::keys)
                        .filter(|key| !GUARD_KEYS.contains(&key.as_str()));
                    for key in keys {
                        parts.push(Self::KindKey {
                            step: index,
                            kind: (*kind).clone(),
                            key: key.clone(),
                        });
                    }
                }
            }
            if kinds.len() > 1 {
                for kind in kinds {
                    parts.push(Self::StepKind {
                        step: index,
                        kind: kind.clone(),
                    });
                }
            }
        }
        if steps.len() > 1 {
            for (index, step) in steps.iter().enumerate() {
                if open(step).is_some() {
                    parts.push(Self::Step(index));
                }
            }
        }
        parts
    }

    /// Whether the check's `words` name this part: its key, or for a whole
    /// step one of its kinds.
    fn named_in(&self, words: &str, table: &toml::Table) -> bool {
        match self {
            Self::KindKey { key, .. } => word_in(words, key),
            Self::StepKind { kind, .. } => word_in(words, kind),
            Self::Step(index) => step_at(table, *index)
                .and_then(toml::Value::as_table)
                .is_some_and(|step| step.keys().any(|kind| word_in(words, kind))),
        }
    }

    /// `text` with this part taken out, and the table it then reads as.
    fn taken_out(&self, text: &str, table: &toml::Table) -> Option<(String, toml::Table)> {
        match self {
            Self::KindKey { step, kind, key } => field_taken_out(
                text,
                table,
                &|table| {
                    table
                        .get_mut("do")?
                        .as_array_mut()?
                        .get_mut(*step)?
                        .get_mut(kind.as_str())?
                        .as_table_mut()
                },
                key,
            ),
            Self::StepKind { step, kind } => field_taken_out(
                text,
                table,
                &|table| {
                    table
                        .get_mut("do")?
                        .as_array_mut()?
                        .get_mut(*step)?
                        .as_table_mut()
                },
                kind,
            ),
            Self::Step(step) => step_taken_out(text, table, *step),
        }
    }

    /// The part for the journal: its keys and place, never a value.
    fn journaled(&self) -> String {
        match self {
            Self::KindKey { step, kind, key } => {
                format!("[[do]] step {}, its {kind}'s {key}", step + 1)
            }
            Self::StepKind { step, kind } => format!("[[do]] step {}, its {kind}", step + 1),
            Self::Step(step) => format!("[[do]] step {}", step + 1),
        }
    }

    /// The part in plain words, for "Left out: …" ([`left_out_said`]),
    /// from `table` as it was before it was taken out.
    fn said(&self, table: &toml::Table) -> String {
        // General words, or its own for a send, the camera or the
        // microphone ([`named_exactly`]).
        let does = |step: Option<&toml::Value>| {
            step.and_then(|step| general_step(step).or_else(|| step_plainly(step)))
                .unwrap_or_else(|| "does something".to_owned())
        };
        match self {
            Self::KindKey { step, key, .. } => format!(
                "{} in a step that {}",
                key_said(key),
                does(step_at(table, *step))
            ),
            Self::StepKind { step, kind } => {
                let kind_alone = step_at(table, *step)
                    .and_then(|step| step.get(kind.as_str()))
                    .map(|value| {
                        let mut alone = toml::Table::new();
                        alone.insert(kind.clone(), value.clone());
                        toml::Value::Table(alone)
                    });
                format!("that one step also {}", does(kind_alone.as_ref()))
            }
            Self::Step(step) => format!("a step that {}", does(step_at(table, *step))),
        }
    }
}

/// A step's key in plain words, for [`Loose::said`].
fn key_said(key: &str) -> String {
    match key {
        "for" => "how long it lasts".to_owned(),
        "over" => "how slowly it changes".to_owned(),
        "until" => "when it ends".to_owned(),
        "level" => "its brightness".to_owned(),
        "dim" => "its dimming".to_owned(),
        "out" => "turning the light off".to_owned(),
        key => format!("its {}", key.replace('_', " ")),
    }
}

/// The `index`th `[[do]]` step of `table`.
fn step_at(table: &toml::Table, index: usize) -> Option<&toml::Value> {
    table.get("do")?.as_array()?.get(index)
}

/// Whether `words` name `key` as a word of its own: "for" in "out takes no
/// colour, dim, level, for, pattern", never in "before".
fn word_in(words: &str, key: &str) -> bool {
    let inside = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    words.match_indices(key).any(|(at, _)| {
        !inside(words[..at].chars().next_back()) && !inside(words[at + key.len()..].chars().next())
    })
}

/// What [`keep_asked`] adds to the say-back of a skill [`loosened`]: "Left
/// out: how long it lasts in a step that changes the light."
fn left_out_said(left: &[String]) -> String {
    format!("Left out: {}.", and_list(left))
}

/// The check's words, comparable from one trial to the next: the folder
/// replaced, a TOML error's quoted source and every digit (its line and
/// column) left out.
fn check_words(words: &str, folder: &Path) -> String {
    words
        .replace(&*folder.to_string_lossy(), "<folder>")
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('|')
                && !line.starts_with(|c: char| c.is_ascii_digit())
        })
        .map(|line| {
            line.chars()
                .filter(|c| !c.is_ascii_digit())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

/// A made skill's `skill.toml` the check refused, with what it refuses left
/// out (the owner's ask on the Queue, #6059, with [`LOOSE_ENVIRONMENT`]
/// on): driven by `lamp-skills pack`'s own refusal, as #6425 takes out a
/// blink's gap. Each part that may go ([`Loose::of`]: a key of a step, then
/// a whole step; never a trigger's key) is tried out of the skill, those the
/// check's words name first. One the check then passes is taken, and the
/// skill with it; one the check's words name whose taking changes its
/// refusal is taken and the next refusal followed. Each is told in plain
/// words, and journaled by its keys alone. None when nothing that may go
/// lets it pass, within [`MOST_LOOSE_TRIALS`] and [`LOOSE_WITHIN`]: the
/// refusal stands, for a repair. The cap ([`capped`]) holds each try.
fn loosened(
    toml: &str,
    card_text: &str,
    from: &str,
    keeping: &Keeping,
) -> Option<(String, Vec<String>)> {
    let began = Instant::now();
    let (name, marked) = card(card_text, "A skill made here.", Maker::of(from)).ok()?;
    static LOOSENING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let work = keeping.directory.join(format!(
        ".loosening-{}-{}",
        std::process::id(),
        LOOSENING.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&work);
    let folder = work.join("skill").join(&name);
    let out = work.join("out");
    let ready =
        fs::create_dir_all(&folder).and_then(|()| fs::write(folder.join("SKILL.md"), &marked));
    // Some(None): passed; Some(Some(words)): refused with them; None: not
    // checked.
    let trial = |text: &str| -> Option<Option<String>> {
        fs::write(folder.join("skill.toml"), text).ok()?;
        let _ = fs::remove_dir_all(&out);
        fs::create_dir_all(&out).ok()?;
        match run_pack(&keeping.program, &folder, &out, LOOSE_POLL) {
            Ok(None) => Some(None),
            Ok(Some((_, words))) => Some(Some(check_words(&words, &folder))),
            Err(_) => None,
        }
    };
    let loosened = ready.ok().and_then(|()| {
        let mut text = toml.to_owned();
        let mut table: toml::Table = toml::from_str(toml).ok()?;
        let mut words = trial(&text)??;
        let mut trials = 1;
        let mut left = Vec::new();
        loop {
            let parts = Loose::of(&table);
            let (named, others): (Vec<Loose>, Vec<Loose>) = parts
                .into_iter()
                .partition(|part| part.named_in(&words, &table));
            let mut moved = false;
            for (part, named) in named
                .into_iter()
                .map(|part| (part, true))
                .chain(others.into_iter().map(|part| (part, false)))
            {
                if trials >= MOST_LOOSE_TRIALS || began.elapsed() > LOOSE_WITHIN {
                    return None;
                }
                let Some((cut, _)) = part.taken_out(&text, &table) else {
                    continue;
                };
                // Its voice asked with nothing left to say is taken out
                // too ([`tidied`]), as the cap holds it.
                let cut = tidied(&cut);
                let Ok(cut_table) = toml::from_str::<toml::Table>(&cut) else {
                    continue;
                };
                if capped(&cut).is_err() {
                    continue;
                }
                trials += 1;
                let next = trial(&cut)?;
                if next.as_ref().is_some_and(|next| !named || *next == words) {
                    continue;
                }
                eprintln!(
                    "lamp-voice: a made skill loosened: left out {}",
                    part.journaled()
                );
                left.push(part.said(&table));
                (text, table) = (cut, cut_table);
                match next {
                    None => return Some((text, left)),
                    Some(next) => words = next,
                }
                moved = true;
                break;
            }
            if !moved {
                return None;
            }
        }
    });
    let _ = fs::remove_dir_all(&work);
    loosened
}

/// The most of a made skill's name, as `lamp-skills` holds a skill's.
const MOST_NAME_BYTES: usize = 64;

/// A made skill's words as [`free_name`] and [`remember`] know them: its
/// `skill.toml`, and with code of its own (#6307) each script's path and
/// text after it, so a changed script is never "you already have that
/// skill" but another skill.
fn made_words(toml: &str, scripts: &[(String, String)]) -> String {
    let mut words = toml.to_owned();
    for (path, text) in scripts {
        words.push_str(&format!("\n```{path}\n{text}```\n"));
    }
    words
}

/// What the person hears when the skill asked for is their own made one,
/// on the lamp already with the same `skill.toml` ([`free_name`]): never a
/// twin of it.
pub const ALREADY_HAVE: &str = "You already have that skill:";

/// [`ALREADY_HAVE`] with the skill's spoken name, and that it is on.
pub fn already_have_line(name: &str) -> String {
    format!("{ALREADY_HAVE} {}. It's on.", spoken_name(name))
}

/// Whether a skill called `name` is on the lamp already: installed in
/// `installed`, pushed beside it, or kept here and put on. One kept here
/// and not on is no skill on the lamp: a newer one of its name takes its
/// place, as [`remember`] and [`forget_others`] keep it (Seventh's P2 on
/// #5072).
fn name_taken(keeping: &Keeping, installed: &Path, name: &str) -> bool {
    installed.join(name).exists()
        || pushed(installed).join(name).exists()
        || index(&keeping.directory)
            .iter()
            .any(|entry| entry["name"] == name && entry["on"] == true)
}

/// Whether the skill called `name` on the lamp is the person's own made
/// one with the same `toml`: installed here with `made: here` and that
/// `skill.toml`, or kept here, on, with that `skill.toml`'s SHA-256.
fn same_made_on(keeping: &Keeping, installed: &Path, name: &str, toml: &str) -> bool {
    let there = installed.join(name);
    let installed_same = fs::read_to_string(there.join("SKILL.md"))
        .is_ok_and(|card| card.lines().any(|line| line.trim() == "made: here"))
        && fs::read_to_string(there.join("skill.toml")).is_ok_and(|kept| kept == toml);
    let sha256 = hex(&Sha256::digest(toml.as_bytes()));
    installed_same
        || index(&keeping.directory).iter().any(|entry| {
            entry["name"] == name && entry["on"] == true && entry["toml_sha256"] == sha256
        })
}

/// A made skill's name, free on the lamp (owner, 11 Nov: makes failed
/// again and again on "a skill called welcome back is already on the
/// lamp", a factory reset keeping the lamp's own skills): `name` when no
/// skill on the lamp has it ([`name_taken`]); else, when the one that has
/// it is the person's own made skill of the same `toml`, "you already have
/// that skill" ([`already_have_line`]), never a twin; else the first of
/// `name-2`, `name-3`, … that is free (or, of the same words, already
/// had), its stem cut so it is at most `most` bytes and a skill's name.
fn free_name(
    keeping: &Keeping,
    installed: &Path,
    name: &str,
    toml: &str,
    most: usize,
) -> Result<String, Refused> {
    if !name_taken(keeping, installed, name) {
        return Ok(name.to_owned());
    }
    if same_made_on(keeping, installed, name, toml) {
        eprintln!("lamp-voice: a made skill is the person's own {name}, on already");
        return Err(Refused::new(already_have_line(name)));
    }
    for number in 2..=MOST_NAME_TRIES {
        let suffix = format!("-{number}");
        let stem = cut_bytes(name, most.saturating_sub(suffix.len())).trim_end_matches('-');
        if stem.is_empty() {
            break;
        }
        let free = format!("{stem}{suffix}");
        if !name_taken(keeping, installed, &free) {
            eprintln!("lamp-voice: a made skill's name {name} is taken: made as {free}");
            return Ok(free);
        }
        // One made before under a number, of the same words.
        if same_made_on(keeping, installed, &free, toml) {
            eprintln!("lamp-voice: a made skill is the person's own {free}, on already");
            return Err(Refused::new(already_have_line(&free)));
        }
    }
    Err(Refused::new(format!(
        "{NAME_TAKEN_HEAD}{} is already on the lamp",
        spoken_name(name)
    )))
}

/// The most numbers [`free_name`] tries after a taken name.
const MOST_NAME_TRIES: u32 = 99;

/// A marked `SKILL.md` ([`card_marked`]) with its front matter's `name:`
/// line saying `name`.
fn renamed_card(card: &str, name: &str) -> String {
    let mut done = false;
    card.split_inclusive('\n')
        .map(|line| {
            if !done && line.starts_with("name:") {
                done = true;
                format!("name: {name}\n")
            } else {
                line.to_owned()
            }
        })
        .collect()
}

/// A skill's two files as they are packed: its `skill.toml` as written,
/// its name, its `SKILL.md` marked ([`card_marked`]), its version and
/// what it may use.
struct Card<'a> {
    toml: &'a str,
    name: String,
    marked: String,
    version: String,
    uses: Uses,
    /// Its code, under [`SCRIPTS_FOLDER`] (#6307): none but a made skill's.
    scripts: &'a [(String, String)],
}

/// A skill added from a file (`crate::upload_skill`, `LAMP_SKILL_UPLOAD`):
/// its two `files` (a bare `SKILL.md` already shaped as a written skill)
/// held to [`capped_added`], said by [`plainly`], marked `made: here` and
/// [`ADDED_FROM_FILE`], checked and packed by `lamp-skills pack` and kept
/// beside the made skills from `from`, with them at most [`KEPT_MOST`].
/// Its card's description is what [`plainly`] says, the label, and when to
/// ask for it in the file's own words, so the model knows when to call it;
/// what the lamp says aloud is [`plainly`]'s alone.
pub fn keep_added(files: &Files, from: &str, keeping: &Keeping) -> Result<Made, Refused> {
    let unkept = |why: io::Error| {
        eprintln!("lamp-voice: a skill added from a file was not kept: {why}");
        Refused::new(NOT_KEPT_HERE)
    };
    ensure_private(&keeping.directory).map_err(unkept)?;
    drop_unfinished(&keeping.directory);
    if let Some(made) = kept(&keeping.directory, from) {
        return Ok(made);
    }
    if files.toml.len() > MOST_FILE_BYTES || files.card.len() > MOST_FILE_BYTES {
        return Err(Refused::new("one of its files is too long"));
    }
    let (version, uses) = capped_added(&files.toml)?;
    let does = plainly(&files.toml)?;
    let when = written_description(&files.card).unwrap_or_default();
    let card_description = cut_bytes(
        &format!("{does} {ADDED_LABEL}. When to use it: {when}"),
        MOST_ADDED_DESCRIPTION_BYTES,
    )
    .to_owned();
    let (name, marked) = card_marked(
        &files.card,
        &card_description,
        &format!(
            "made: here\n{MADE_BY}: {}\n{ADDED_FROM_FILE}",
            Maker::App.key()
        ),
    )?;
    packed_and_kept(
        keeping,
        from,
        Card {
            toml: &files.toml,
            name,
            marked,
            version,
            uses,
            scripts: &[],
        },
        Ok(does),
        true,
    )
}

/// The most of an added skill's card description: a tool's is at most
/// 1,024 bytes (`lamp-skills`' check), with room for its quotes.
const MOST_ADDED_DESCRIPTION_BYTES: usize = 900;

/// A `SKILL.md`'s own `description:`, on one line, with no control
/// characters.
pub fn written_description(card: &str) -> Option<String> {
    let front = card.strip_prefix("---\n")?.split_once("\n---")?.0;
    let line = front
        .lines()
        .find_map(|line| line.strip_prefix("description:"))?;
    let line: String = line
        .trim()
        .trim_matches('"')
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = line.trim().to_owned();
    (!line.is_empty()).then_some(line)
}

/// A `SKILL.md`'s `name:`, as the card's own check reads it.
pub fn card_name(card: &str) -> Option<String> {
    let front = card.strip_prefix("---\n")?.split_once("\n---")?.0;
    front
        .lines()
        .find_map(|line| line.strip_prefix("name:"))
        .map(|name| name.trim().trim_matches('"').to_owned())
        .filter(|name| !name.is_empty())
}

/// The names of the skills kept here, made or added, on or not.
pub fn kept_names(directory: &Path) -> Vec<String> {
    index(directory)
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect()
}

/// `card` written to a folder of this call's own, checked and packed by
/// `lamp-skills pack`, and kept from `from` once `said` is; `added` is kept
/// in the index as `"added": "file"`. What [`keep`], [`keep_added`] and
/// [`keep_written`] share.
fn packed_and_kept(
    keeping: &Keeping,
    from: &str,
    card: Card,
    said: Result<String, Refused>,
    added: bool,
) -> Result<Made, Refused> {
    let unkept = |why: io::Error| {
        eprintln!("lamp-voice: a made skill was not kept: {why}");
        Refused::new(NOT_KEPT_HERE)
    };
    let Card {
        toml,
        name,
        marked,
        version,
        uses,
        scripts,
    } = card;
    // A folder of this call's own: two keeps at once in one process (two
    // readers of the same news, as #3662's test runs them) never share
    // its check's words.
    static MAKING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let work = keeping.directory.join(format!(
        ".making-{}-{}",
        std::process::id(),
        MAKING.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&work);
    let folder = work.join("skill").join(&name);
    let out = work.join("out");
    let made = (|| {
        fs::create_dir_all(&folder)?;
        fs::create_dir_all(&out)?;
        fs::write(folder.join("skill.toml"), toml)?;
        fs::write(folder.join("SKILL.md"), &marked)?;
        // Its code beside them (#6307), each path held to
        // [`script_path_ok`], so always inside its folder: packed with
        // them, so the package's SHA-256, what the person's yes is to,
        // covers every byte of it.
        for (path, text) in scripts {
            if !script_path_ok(path) {
                return Err(io::Error::other("a script's path outside its folder"));
            }
            let file = folder.join(path);
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(file, text)?;
        }
        Ok::<_, io::Error>(())
    })()
    .map_err(unkept);
    let packed = made.and_then(|()| pack(&keeping.program, &folder, &out, &name));
    let result = packed.and(said).and_then(|description| {
        let package = format!("{name}-{version}.tar");
        let bytes = fs::read(out.join(&package)).map_err(unkept)?;
        let sha256 = hex(&Sha256::digest(&bytes));
        let kept_as = keeping
            .directory
            .join(format!("{name}-{version}-{}.tar", &sha256[..12]));
        // Written whole beside it, and moved in only once the index names
        // it: a crash leaves a `.new` the next keep deletes, never a
        // package nothing names (the reviewer on #2011).
        let new = keeping
            .directory
            .join(format!(".{name}-{version}-{}.tar.new", &sha256[..12]));
        fs::write(&new, &bytes).map_err(unkept)?;
        let made = Made {
            name,
            version,
            sha256,
            // A write's say-back is never cut (#6066): every recipient
            // and word is said, bounded by [`MOST_WRITE_PLAIN_CHARS`]; nor
            // one with code of its own, its grants and its wait (#6307).
            description: if uses.writes.is_empty() && !uses.code {
                cut_chars(&description, MOST_DESCRIPTION_CHARS)
            } else {
                description
            },
            uses,
            package: kept_as,
        };
        let toml_sha256 = hex(&Sha256::digest(made_words(toml, scripts).as_bytes()));
        remember(&keeping.directory, from, &made, added, &toml_sha256).map_err(unkept)?;
        // A write's exact contents, kept beside it for the person's yes
        // (#6066, [`writes_of`]): the voice's own folder, as the package.
        let writes = writes_in(toml);
        if !writes.is_empty() {
            note_writes(&keeping.directory, &made.sha256, &writes).map_err(unkept)?;
        }
        // What it starts on and uses, for a clash with a skill on the lamp
        // (#6593, `crate::made_clash`).
        note_clash(
            &keeping.directory,
            &made.sha256,
            crate::made_clash::signature(toml),
        )
        .map_err(unkept)?;
        fs::rename(&new, &made.package).map_err(unkept)?;
        Ok(made)
    });
    let _ = fs::remove_dir_all(&work);
    result
}

/// `lamp-skills pack --out OUT FOLDER`: the checks a load makes, its
/// permissions among them, and the package read back as an install would.
/// Its words go to a file beside OUT, in the private folder, so a long
/// answer never blocks it.
fn pack(program: &Path, folder: &Path, out: &Path, name: &str) -> Result<(), Refused> {
    let Some((code, words)) = run_pack(program, folder, out, Duration::from_millis(50))? else {
        return Ok(());
    };
    eprintln!(
        "lamp-voice: a made skill did not pass lamp-skills' check (exit {}): {}",
        code.map_or("none".to_owned(), |code| code.to_string()),
        check_reason(&words, folder, name)
    );
    Err(Refused(
        NOT_PASSED.into(),
        Some(check_lines(&words, folder)),
    ))
}

/// [`pack`]'s run, journaling nothing, looked at every `poll`: none when it
/// passed, else its exit code and its words, at most [`MOST_CHECK_BYTES`].
fn run_pack(
    program: &Path,
    folder: &Path,
    out: &Path,
    poll: Duration,
) -> Result<Option<(Option<i32>, String)>, Refused> {
    let said = out.with_file_name("check.err");
    let stderr = fs::File::create(&said).map_err(|why| {
        eprintln!("lamp-voice: a made skill was not checked: {why}");
        Refused::new(NOT_CHECKED)
    })?;
    let mut child = Command::new(program)
        .arg("pack")
        .arg("--out")
        .arg(out)
        .arg(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .map_err(|why| {
            eprintln!("lamp-voice: a made skill was not checked: {why}");
            Refused::new(NOT_CHECKED)
        })?;
    let began = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if began.elapsed() < PACK_WITHIN => {
                std::thread::sleep(poll);
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Refused::new(NOT_CHECKED_IN_TIME));
            }
        }
    };
    if status.success() {
        return Ok(None);
    }
    let mut words = Vec::new();
    if let Ok(file) = fs::File::open(&said) {
        let _ = file.take(MOST_CHECK_BYTES).read_to_end(&mut words);
    }
    Ok(Some((
        status.code(),
        String::from_utf8_lossy(&words).into_owned(),
    )))
}

/// The check's `said` for a repair's brief: every line, trimmed and joined
/// with " / ", the skill's folder (the lamp's own path) replaced, cut to
/// [`MOST_REPAIR_BYTES`]. Unlike [`check_reason`] its quotes are kept: they
/// are the model's own words going back to it.
fn check_lines(said: &str, folder: &Path) -> String {
    let said = said.replace(&*folder.to_string_lossy(), "<folder>");
    let lines: Vec<&str> = said
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    cut_bytes(&lines.join(" / "), MOST_REPAIR_BYTES).to_owned()
}

/// The check's answer read, at most.
const MOST_CHECK_BYTES: u64 = 8 * 1024;
/// The reason logged, at most, in characters.
const MOST_REASON_CHARS: usize = 200;

/// The check's first line, for the journal (Mobile's #2813 rerun on
/// lamp-0c89, 23 Oct, gave only `exit 1`). lamp-skills' words can quote the
/// skill's, which come from the model and the person's request (the
/// reviewer on #2011), so anything in quotes, the skill's own name and its
/// folder are left out, and the line is cut short.
fn check_reason(said: &str, folder: &Path, name: &str) -> String {
    let mut lines = said.lines().map(str::trim).filter(|line| !line.is_empty());
    let Some(line) = lines.next() else {
        return "it said nothing".into();
    };
    let line = line.strip_prefix("lamp-skills: ").unwrap_or(line);
    // A TOML parse error says where on its first line and why after the
    // quoted source ("unknown field `name`", Mobile's #2813 rerun): the
    // first line that is not the source or its marks is added.
    let because = if line.contains("TOML parse error") {
        lines.find(|line| !line.starts_with('|') && !line.starts_with(|c: char| c.is_ascii_digit()))
    } else {
        None
    };
    let line = match because {
        Some(because) => format!("{line}: {because}"),
        None => line.to_owned(),
    };
    let line = line.replace(&*folder.to_string_lossy(), "<folder>");
    let chars: Vec<char> = line.chars().collect();
    let word = |at: Option<&char>| at.is_some_and(|c| c.is_alphanumeric());
    let mut kept = String::new();
    let mut open: Option<char> = None;
    for (at, &c) in chars.iter().enumerate() {
        let before = at.checked_sub(1).and_then(|at| chars.get(at));
        // An apostrophe in a word ("skill's", "don't") neither opens nor
        // closes a quote.
        let in_word = word(before) && word(chars.get(at + 1));
        // A quote escaped inside a value printed with `{:?}` ("say \"hi\"")
        // neither closes nor opens one (the reviewer-lead's P3 on #3225).
        let escaped = before == Some(&'\\');
        match open {
            Some(quote)
                if !in_word && !escaped && c == if quote == '“' { '”' } else { quote } =>
            {
                kept.push('…');
                kept.push(c);
                open = None;
            }
            Some(_) => {}
            None if !word(before) && matches!(c, '"' | '\'' | '`' | '“') => {
                kept.push(c);
                open = Some(c);
            }
            None => kept.push(c),
        }
    }
    // A quote never closed hides the rest of the line.
    if open.is_some() {
        kept.push('…');
    }
    let kept = if name.is_empty() {
        kept
    } else {
        kept.replace(name, "<skill>")
    };
    cut_chars(&kept, MOST_REASON_CHARS)
}

/// The folder, made 0700 if it is not there; one already there is made
/// 0700 too.
pub(crate) fn ensure_private(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
}

/// A package a crash left half moved in (`.NAME.tar.new`), deleted.
/// How old a half-moved package must be to be a crash's: older than any
/// keep takes from writing it to moving it in, its check included. Two
/// readers keep the same answers, so a younger one may be another keep's,
/// named in the index a moment before it is moved in (Fourteen's P2 on
/// #3662).
const UNFINISHED_AFTER: Duration = Duration::from_secs(3 * PACK_WITHIN.as_secs());

fn drop_unfinished(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > UNFINISHED_AFTER);
        if old
            && name
                .to_str()
                .is_some_and(|name| name.starts_with('.') && name.ends_with(".tar.new"))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// What is kept, oldest first.
/// The made skills' index as kept, for the app's asks
/// (`crate::app_make`): read only.
pub fn entries(directory: &Path) -> Vec<Value> {
    index(directory)
}

fn index(directory: &Path) -> Vec<Value> {
    fs::read(directory.join(INDEX))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
}

/// The made skill kept as made from `from`, if it is still kept.
fn kept(directory: &Path, from: &str) -> Option<Made> {
    index(directory)
        .into_iter()
        .find(|entry| entry["from"] == from)
        .and_then(|entry| made_of(directory, &entry))
}

/// An index entry as the [`Made`] it keeps, its package still there.
fn made_of(directory: &Path, entry: &Value) -> Option<Made> {
    let package = directory.join(entry["package"].as_str()?);
    package.is_file().then(|| Made {
        name: entry["name"].as_str().unwrap_or_default().to_owned(),
        version: entry["version"].as_str().unwrap_or_default().to_owned(),
        sha256: entry["sha256"].as_str().unwrap_or_default().to_owned(),
        description: entry["description"].as_str().unwrap_or_default().to_owned(),
        uses: Uses::of_json(&entry["uses"]),
        package,
    })
}

/// The kept skill on the lamp whose name is `asked` ([`name_asked`]), to
/// read it back (`make_skill_local::read_back`); none when no skill kept
/// here and put on has that name.
pub fn kept_on_named(directory: &Path, asked: &str) -> Option<Made> {
    let asked = name_asked(asked);
    index(directory)
        .into_iter()
        .filter(|entry| entry["on"] == true)
        .find(|entry| entry["name"].as_str() == Some(asked.as_str()))
        .and_then(|entry| made_of(directory, &entry))
}

/// The names of the kept skills on the lamp.
pub fn kept_on_names(directory: &Path) -> Vec<String> {
    index(directory)
        .iter()
        .filter(|entry| entry["on"] == true)
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect()
}

/// A skill's name as the person or the model gives it, as a kept skill's
/// name is written: lower case, each run of anything but a letter or a
/// digit one dash, no "my" before it or "skill" after it ("my Leaving
/// mail skill" is `leaving-mail`).
pub fn name_asked(asked: &str) -> String {
    let words: Vec<String> = asked
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut words = words.as_slice();
    if let [first, rest @ ..] = words
        && first == "my"
        && !rest.is_empty()
    {
        words = rest;
    }
    if let [rest @ .., last] = words
        && last == "skill"
        && !rest.is_empty()
    {
        words = rest;
    }
    words.join("-")
}

/// A written name (`lamp-test-email-2`, as [`name_asked`] writes one) with
/// each number said in words written as digits, the same on both sides of
/// a match: speech gives "lamp test email two" for a skill named with a
/// `2` (lamp-9adf, 12 Nov, #6223's rows). Zero to nineteen, the tens to
/// ninety, and a tens word with a unit after it as one number ("twenty
/// one" is `21`); any other word is left as it is.
pub fn name_digits(name: &str) -> String {
    const UNITS: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 8] = [
        "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    let unit = |word: &str| UNITS.iter().position(|said| *said == word);
    let tens = |word: &str| {
        TENS.iter()
            .position(|said| *said == word)
            .map(|at| 20 + 10 * at)
    };
    let words: Vec<&str> = name.split('-').collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut at = 0;
    while at < words.len() {
        let word = words[at];
        if let Some(tens) = tens(word) {
            match words.get(at + 1).and_then(|next| unit(next)) {
                Some(one) if (1..=9).contains(&one) => {
                    out.push((tens + one).to_string());
                    at += 2;
                }
                _ => {
                    out.push(tens.to_string());
                    at += 1;
                }
            }
        } else {
            out.push(unit(word).map_or_else(|| word.to_owned(), |number| number.to_string()));
            at += 1;
        }
    }
    out.join("-")
}

/// Keep `made` in the index, as made from `from`, and let the oldest
/// go past [`KEPT_MOST`], their packages with them. Written whole beside
/// it and moved over it.
///
/// One entry a skill: the same bytes, or the same name not on, are
/// replaced, its `on` kept, never listed twice (lamp-9adf, 3 Nov 16:15: a
/// skill made twice listed twice pushed out a skill that was on).
/// Eviction lets go the oldest that is not on, never the one just kept;
/// with every other one on, all are kept, past the cap. A package another
/// entry still names stays. `toml_sha256` is its `skill.toml`'s (with its
/// code, [`made_words`]), so a make of the same words is known as the
/// person's own ([`free_name`]).
fn remember(
    directory: &Path,
    from: &str,
    made: &Made,
    added: bool,
    toml_sha256: &str,
) -> io::Result<()> {
    let mut entries = index(directory);
    let mut entry = json!({
        "from": from,
        "name": made.name,
        "version": made.version,
        "sha256": made.sha256,
        "toml_sha256": toml_sha256,
        "description": made.description,
        "uses": made.uses.json(),
        "package": made.package.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
        "at_unix_ms": SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64),
    });
    let mut gone = Vec::new();
    entries.retain(|old| {
        let same = old["sha256"] == made.sha256.as_str()
            || (old["name"] == made.name.as_str() && old["on"] != true);
        if same {
            // The same bytes on the lamp stay on.
            if old["on"] == true {
                entry["on"] = json!(true);
            }
            gone.push(old.clone());
        }
        !same
    });
    if added {
        entry["added"] = json!("file");
    }
    entries.push(entry);
    while entries.len() > KEPT_MOST {
        // The newest, just kept, is never let go.
        let newest = entries.len() - 1;
        let Some(oldest) = entries[..newest]
            .iter()
            .position(|entry| entry["on"] != true)
        else {
            break;
        };
        gone.push(entries.remove(oldest));
    }
    for old in gone {
        if let Some(package) = old["package"]
            .as_str()
            .filter(|name| !name.is_empty() && !name.contains('/') && !name.starts_with('.'))
            .filter(|name| entries.iter().all(|entry| entry["package"] != *name))
        {
            let _ = fs::remove_file(directory.join(package));
        }
    }
    write_index(directory, &entries)
}

/// The write steps' exact contents of a `skill.toml`, as `lamp-skills`
/// sends them and `lamp-services` hashes them (#6066).
pub fn writes_in(toml: &str) -> Vec<Value> {
    let Ok(table) = toml::from_str::<toml::Table>(toml) else {
        return Vec::new();
    };
    table
        .get("do")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|step| step.get("write"))
        .filter_map(write_content)
        .collect()
}

/// Keep the writes of the kept skill `sha256` beside it in the index.
fn note_writes(directory: &Path, sha256: &str, writes: &[Value]) -> io::Result<()> {
    let mut entries = index(directory);
    for entry in &mut entries {
        if entry["sha256"] == sha256 {
            entry["writes"] = json!(writes);
        }
    }
    write_index(directory, &entries)
}

/// What the made skill `sha256` starts on and uses, kept as `"clash"`
/// beside it (`crate::made_clash::signature`).
fn note_clash(directory: &Path, sha256: &str, signature: Value) -> io::Result<()> {
    let mut entries = index(directory);
    for entry in &mut entries {
        if entry["sha256"] == sha256 {
            entry["clash"] = signature.clone();
        }
    }
    write_index(directory, &entries)
}

/// The writes kept with the made skill `sha256` ([`note_writes`]): what
/// the person's yes approves. None for a skill that writes nothing.
pub fn writes_of(directory: &Path, sha256: &str) -> Vec<Value> {
    index(directory)
        .into_iter()
        .find(|entry| entry["sha256"] == sha256)
        .and_then(|entry| entry["writes"].as_array().cloned())
        .unwrap_or_default()
}

/// The index written whole beside it and moved over it.
fn write_index(directory: &Path, entries: &[Value]) -> io::Result<()> {
    let new = directory.join(format!(".{INDEX}.new"));
    let mut file = fs::File::create(&new)?;
    file.write_all(&serde_json::to_vec(entries).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&new, directory.join(INDEX))
}

/// Mark the kept skill `sha256` to go on at `now_unix_ms`, as
/// `goes_on_at_unix_ms` beside its package: the narrow "put on as made"
/// entry the manage helper holds `install_made` to ([`going_on`]), written
/// by the voice alone, right before it asks the helper. No yes is faked:
/// a made skill goes on with no spoken yes (the owner, 10 Nov: the app's
/// MAKE IT and the voice ask are the ask, #5977). Whether one was marked.
pub fn goes_on(directory: &Path, sha256: &str, now_unix_ms: u64) -> io::Result<bool> {
    let mut entries = index(directory);
    let mut found = false;
    for entry in &mut entries {
        if entry["sha256"] == sha256 {
            entry["goes_on_at_unix_ms"] = json!(now_unix_ms);
            if let Some(entry) = entry.as_object_mut() {
                entry.remove("not_on");
            }
            found = true;
        }
    }
    if found {
        write_index(directory, &entries)?;
    }
    Ok(found)
}

/// The name of the kept skill `sha256` if it was marked to go on
/// ([`goes_on`]) within [`GOES_ON_WITHIN`] before `now_unix_ms` and is not
/// on yet: what lamp-skills' manage helper holds `install_made` to, by its
/// own clock. A kept SHA-256 never marked, marked too long ago, marked
/// after `now_unix_ms` (a clock gone back) or on already is none.
pub fn going_on(directory: &Path, sha256: &str, now_unix_ms: u64) -> Option<String> {
    let within = GOES_ON_WITHIN.as_millis() as u64;
    index(directory)
        .into_iter()
        .find(|entry| entry["sha256"] == sha256)
        .filter(|entry| entry["on"] != true)
        .filter(|entry| {
            entry["goes_on_at_unix_ms"]
                .as_u64()
                .is_some_and(|at| at <= now_unix_ms && now_unix_ms - at <= within)
        })
        .map(|entry| entry["name"].as_str().unwrap_or_default().to_owned())
}

/// Why the kept skill `sha256` didn't go on, in plain words, kept beside it
/// as `not_on`: the app's and the counts' "not on" ([`crate::app_make`],
/// [`crate::made_counts`]).
fn not_on(directory: &Path, sha256: &str, why: &str) {
    let mut entries = index(directory);
    let mut found = false;
    for entry in &mut entries {
        if entry["sha256"] == sha256 && entry["on"] != true {
            entry["not_on"] = json!(why);
            found = true;
        }
    }
    if found && let Err(error) = write_index(directory, &entries) {
        eprintln!("lamp-voice: a made skill not on, and why not kept: {error}");
    }
}

/// What the lamp says once a made skill is on (the owner, 10 Nov: no
/// "Shall I put it on the lamp?" any more): its name, then what it does as
/// its description says and what it uses, so the person hears what went on
/// with no yes asked (App Only's P1 on #5985, the 11:47 ruling's
/// read-back). No "say take it off" (the owner, 10 Nov: the person
/// removes it in the app when they want).
pub fn on_line(made: &Made) -> String {
    let name = spoken_name(&made.name);
    let does = made.description.trim().trim_end_matches(['.', '!', '?']);
    let mut letters = does.chars();
    let does: String = match (letters.next(), letters.clone().next()) {
        // "Dims the light" said after a colon as "dims the light", never
        // a word all in capitals ("LED").
        (Some(first), second) if !second.is_some_and(char::is_uppercase) => {
            first.to_lowercase().chain(letters).collect()
        }
        _ => does.to_owned(),
    };
    // One that sends or writes is said whole, its every recipient and word
    // in its description, and ends as that does: it waits for the
    // person's yes (#6066), never on a word for them to repeat.
    // One with code of its own alike (#6307): its grants and its wait;
    // put on at once ([`code_goes_on_at_once`]), its grants and that it's on.
    if code_goes_on_at_once(made) {
        let mut said = format!("I made {name}, and it's on: {does}");
        if !said.ends_with(['.', '!', '?']) {
            said.push('.');
        }
        return said;
    }
    if !made.uses.writes.is_empty() || made.uses.code {
        let mut said = format!("I made {name}: {does}");
        if !said.ends_with(['.', '!', '?']) {
            said.push('.');
        }
        return said;
    }
    if does.is_empty() {
        return format!("I made {name}, and it's on.");
    }
    let uses = made.uses.spoken();
    let using = if uses == "nothing" {
        String::new()
    } else {
        format!(", using {uses}")
    };
    // What it uses goes with what it does, before how often it starts on
    // its own (#6066 (a)), its own sentence.
    match does.split_once(MANNERS_BEGIN) {
        Some((what, manners)) => format!(
            "I made {name}, and it's on: {}{using}. {}{manners}.",
            what.trim_end_matches('.'),
            MANNERS_BEGIN.trim_start()
        ),
        None => format!("I made {name}, and it's on: {does}{using}."),
    }
}

/// What the lamp says when a made skill that sends or writes, already
/// approved for what it sends, is read back by voice
/// (`make_skill_local::read_back`): its say-back as [`on_line`] has it,
/// not ending on [`WAITS_FOR_YES`], then [`APPROVED_ALREADY`]. Nothing is
/// asked.
pub fn approved_line(made: &Made) -> String {
    let line = on_line(made);
    let line = line
        .strip_suffix(WAITS_FOR_YES.trim_start())
        .unwrap_or(&line)
        .trim_end();
    let line = line.strip_prefix("I made ").unwrap_or(line);
    format!("Your {line} {APPROVED_ALREADY}")
}

/// How [`approved_line`] ends.
pub const APPROVED_ALREADY: &str = "You agreed to it, so it sends as it says.";

/// What the lamp says of a made skill that sends or writes when the turn
/// it was made in was cut (App Only's P2 on #6142): said through the say
/// service, its say-back never told the voice's model with its
/// `approve_id`, so no yes the person gives later can approve it
/// (`crate::skill_write_yes`). Its name and that it waits for the yes, in
/// [`WAITS_FOR_YES`]'s words, then that what it sends wasn't said, so it
/// sends nothing yet: never "it's on" alone, and no word to repeat.
pub fn cut_write_line(made: &Made) -> String {
    format!(
        "I made {}.{WAITS_FOR_YES} {CUT_WRITE_UNSAID}",
        spoken_name(&made.name)
    )
}

/// How [`cut_write_line`] ends.
pub const CUT_WRITE_UNSAID: &str =
    "I couldn't tell you just now what it sends, so it sends nothing yet.";

/// What the lamp says of a made skill with code of its own when the turn
/// it was made in was cut (#6307), as [`cut_write_line`] for a write: its
/// say-back never reached the person with its `approve_id`, so no yes can
/// put it on. Its name, [`RUNS_AFTER_YES`], then [`CUT_CODE_UNSAID`].
pub fn cut_code_line(made: &Made) -> String {
    format!(
        "I made {}.{RUNS_AFTER_YES} {CUT_CODE_UNSAID}",
        spoken_name(&made.name)
    )
}

/// How [`cut_code_line`] ends.
pub const CUT_CODE_UNSAID: &str =
    "I couldn't tell you just now what its code may use, so it isn't on.";

/// Why a skill with code of its own is never put on at once ([`put_on`],
/// #6307): the app's MAKE IT comes before its grants are said, and its yes
/// is taken by voice only, after its say-back ([`put_on_agreed`]), which
/// the lamp says of an app's make too ([`code_asked`]).
pub const CODE_BY_VOICE: &str = "a skill that runs its own code goes on only once you agree to it out loud at the lamp, after it says what that code may use";

/// The kept skill `sha256`, with code of its own and made from the app,
/// said back aloud at `now_unix_ms` (#6307, `crate::app_make`): kept as
/// asked, `code_asked_at_unix_ms`, so the voice's second route takes its
/// yes once that say-back is said ([`code_asked_in`]).
pub fn code_asked(directory: &Path, sha256: &str, now_unix_ms: u64) {
    let mut entries = index(directory);
    let mut found = false;
    for entry in &mut entries {
        if entry["sha256"] == sha256 && entry["on"] != true && entry["uses"]["code"] == true {
            entry["code_asked_at_unix_ms"] = json!(now_unix_ms);
            found = true;
        }
    }
    if found && let Err(error) = write_index(directory, &entries) {
        eprintln!("lamp-voice: a made skill with code of its own: its ask not kept: {error}");
    }
}

/// The newest kept skill with code of its own asked about from the app
/// ([`code_asked`]) within `within_ms` before `now_unix_ms` and not on:
/// whose say-back, once said, waits for the person's yes.
pub fn code_asked_in(directory: &Path, now_unix_ms: u64, within_ms: u64) -> Option<Made> {
    index(directory)
        .into_iter()
        .filter(|entry| entry["on"] != true && entry["uses"]["code"] == true)
        .filter_map(|entry| {
            let at = entry["code_asked_at_unix_ms"].as_u64()?;
            (at <= now_unix_ms && now_unix_ms - at <= within_ms).then_some((at, entry))
        })
        .max_by_key(|(at, _)| *at)
        .and_then(|(_, entry)| made_of(directory, &entry))
}

/// Put the kept skill with code of its own whose SHA-256 is `sha256` on the
/// lamp, on the person's yes to its say-back (#6307,
/// `crate::skill_write_yes`): as [`put_on`], by that SHA-256 alone, so only
/// the very package said back, every byte of its code with it, goes on; a
/// changed script is another package and asks again. Refused for a kept
/// skill with no code: this path is its yes alone. Its name once on, or why
/// not, in plain words.
pub fn put_on_agreed(keeping: &Keeping, sha256: &str) -> Result<String, String> {
    let code = index(&keeping.directory)
        .into_iter()
        .find(|entry| entry["sha256"] == sha256)
        .is_some_and(|entry| entry["uses"]["code"] == true);
    if !code {
        return Err("the lamp no longer has it".into());
    }
    let now_unix_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    put_on_within(
        &keeping.directory,
        &keeping.manage,
        sha256,
        now_unix_ms,
        PUT_ON_WAIT,
    )
}

/// How [`manners_plainly`]'s sentence begins.
const MANNERS_BEGIN: &str = " It starts on its own";

/// What the voice's model is told to say of a made skill: `line`'s
/// content in the language of the conversation, every part of it (what it
/// does, when, and what it uses or reads: with no spoken yes, the only
/// read-back the person hears; Thirteen's and App Only's P2 on #6032),
/// the skill's name and any service's name kept as written and nothing
/// added (the owner, 10 Nov:
/// "đồng bộ luôn"; a fixed English line said as it is, "Say exactly",
/// was English to a person speaking Vietnamese). The model is the voice
/// itself, whose character already speaks the owner's language
/// (`crate::language::instruction`). Used by the voice's tool answer and
/// by the say service's note for an app's or a file's skill.
pub fn in_their_language(line: &str) -> String {
    format!(
        "{ALREADY_DONE}Tell the person this, in the language of the conversation, every part \
         of it, leaving nothing out: what the skill does, when, and what it uses or reads. Keep \
         the skill's name and any service's name as written, and add nothing else: \"{line}\""
    )
}

/// What a note of a make's line begins with: it has happened already, so
/// the model only speaks. On lamp-74ff, 10 Nov 17:43, the say service's
/// note of an app's made skill was taken by the model as an ask to make
/// it, and `make_skill` called (refused, no one spoke) in place of the line.
pub const ALREADY_DONE: &str = "This has happened already and nothing is left to do: call no \
                                tool, make_skill least of all; only speak. ";

/// What the voice's model is told to say of a skill it couldn't make:
/// `line` ([`not_made_line`]) in the language of the conversation, the
/// reason and what to try whole.
pub fn not_made_in_their_language(line: &str) -> String {
    format!(
        "{ALREADY_DONE}Tell the person this, in the language of the conversation, every part \
         of it, leaving nothing out: that the skill wasn't made, why, and what to try. Add \
         nothing else: \"{line}\""
    )
}

/// What the lamp says when a kept skill couldn't be put on: why, in the
/// plain words [`put_on`] gives.
pub fn not_on_line(why: &str) -> String {
    format!("I made it but couldn't put it on: {why}.")
}

/// A skill's name as it is said: its dashes as spaces.
pub fn spoken_name(name: &str) -> String {
    name.replace('-', " ")
}

/// Put the made skill `made`, just kept and checked, on the lamp at once
/// (the owner, 10 Nov: a made skill goes on with no spoken yes; the app's
/// MAKE IT and the voice ask are the ask, #5977): marked to go on by the lamp's
/// own clock ([`goes_on`]), then asked of the manage helper at
/// `keeping.manage` by its SHA-256 alone, which installs only those bytes,
/// held again to the cap ([`put_on_within`]). On; or why not, in plain
/// words, kept beside it.
pub fn put_on(keeping: &Keeping, made: &Made) -> Result<(), String> {
    // One with code of its own never goes on at once (#6307): only on the
    // person's yes to its say-back ([`put_on_agreed`]), so here, where no
    // yes is (the app's MAKE IT), it is left off and why kept beside it.
    if made.uses.code {
        eprintln!(
            "lamp-voice: a made skill with code of its own was not put on at once: {}",
            made.name
        );
        not_on(&keeping.directory, &made.sha256, CODE_BY_VOICE);
        return Err(CODE_BY_VOICE.into());
    }
    let now_unix_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    put_on_within(
        &keeping.directory,
        &keeping.manage,
        &made.sha256,
        now_unix_ms,
        PUT_ON_WAIT,
    )
    .map(|_| ())
}

/// [`put_on`] of the kept skill `sha256` in `directory`, marked at
/// `now_unix_ms`, through the helper at `socket`, waiting at most `wait`
/// for it: its name once on, or why not, in plain words, kept beside it
/// as `not_on`. One on already is on.
pub fn put_on_within(
    directory: &Path,
    socket: &Path,
    sha256: &str,
    now_unix_ms: u64,
    wait: Duration,
) -> Result<String, String> {
    let Some(entry) = index(directory)
        .into_iter()
        .find(|entry| entry["sha256"] == sha256)
    else {
        return Err("the lamp no longer has it".into());
    };
    let name = entry["name"].as_str().unwrap_or_default().to_owned();
    if entry["on"] == true {
        return Ok(name);
    }
    let refused = |why: String| {
        eprintln!("lamp-voice: a made skill was not put on: {name}: {why}");
        not_on(directory, sha256, &why);
        Err(why)
    };
    if let Err(error) = goes_on(directory, sha256, now_unix_ms) {
        eprintln!("lamp-voice: a made skill: its mark to go on not kept: {error}");
        return refused("the lamp couldn't keep it".into());
    }
    // A helper not there (no socket, or nobody listening) never puts it on:
    // only one asked and silent may still (the reviewer's P3 on #2424).
    let Some(answer) = crate::tools::reached(socket, &json!({ "install_made": sha256 }), wait)
    else {
        return refused("the lamp's skills couldn't be reached".into());
    };
    match answer.as_ref().map(|answer| answer.get("done").is_some()) {
        // No answer in time: the helper may still put it on.
        None => refused("the lamp didn't say in time whether it is on".into()),
        Some(true) => {
            let mut entries = index(directory);
            for entry in &mut entries {
                if entry["sha256"] == sha256 {
                    entry["on"] = json!(true);
                    if let Some(entry) = entry.as_object_mut() {
                        entry.remove("not_on");
                    }
                }
            }
            if let Err(error) = write_index(directory, &entries) {
                eprintln!("lamp-voice: a made skill is on, but not marked so: {error}");
            }
            eprintln!("lamp-voice: a made skill is on the lamp: {name}, sha256 {sha256}");
            Ok(name)
        }
        Some(false) => {
            let why = answer
                .as_ref()
                .and_then(|answer| answer["refused"].as_str())
                .unwrap_or("");
            // One they put on another way has its name (#2414): said, not
            // replaced.
            if why.contains("put on another way") || why.contains("never takes its place") {
                return refused(format!(
                    "a skill called {} is already on the lamp",
                    spoken_name(&name)
                ));
            }
            eprintln!("lamp-voice: the skills helper refused a made skill: {why}");
            refused("the lamp's check didn't let it on".into())
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `text`'s first `most` bytes, cut on a character.
fn cut_bytes(text: &str, most: usize) -> &str {
    let mut at = most.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    &text[..at]
}

/// [`cut_chars`], cut back to a whole word when it cuts.
fn cut_words(text: &str, most: usize) -> String {
    let cut = cut_chars(text, most);
    match cut.strip_suffix('…') {
        Some(head) => match head.rsplit_once(' ') {
            Some((words, _)) if !words.is_empty() => {
                format!("{}…", words.trim_end_matches([',', ';', ':']))
            }
            _ => cut,
        },
        None => cut,
    }
}

/// `text` on one line, at most `most` characters, cut only where a
/// sentence ends, or else where a word ends, never mid-word and never with
/// "…": what is left ends as a sentence (#5977).
fn cut_sentences(text: &str, most: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some((at, _)) = line.char_indices().nth(most) else {
        return line;
    };
    let head = &line[..at];
    // A sentence ends at '.', '!' or '?' followed by a space (or at the cut,
    // when the next character is a space).
    let next_is_space = line[at..].starts_with(' ');
    let ends = head
        .char_indices()
        .rev()
        .filter(|&(index, c)| {
            matches!(c, '.' | '!' | '?')
                && (head[index + c.len_utf8()..].starts_with(' ')
                    || (index + c.len_utf8() == head.len() && next_is_space))
        })
        .map(|(index, c)| index + c.len_utf8())
        .next();
    if let Some(end) = ends {
        return head[..end].to_owned();
    }
    let words = if next_is_space {
        head
    } else {
        head.rsplit_once(' ').map_or("", |(words, _)| words)
    };
    let words = words.trim_end_matches([',', ';', ':', '-', ' ']);
    if words.is_empty() {
        String::new()
    } else {
        format!("{words}.")
    }
}

fn cut_chars(text: &str, most: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(most) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a made skill does, as [`plainly`] says it, before how often it
    /// starts on its own.
    fn does(said: &str) -> &str {
        said.split(MANNERS_BEGIN).next().unwrap_or(said)
    }

    const TOML: &str = r#"kind = "installable"
version = "1.0.0"
needs = ["light"]

[permissions]
topics = ["voice/state"]
light = true

[trigger]
topic = "voice/state"
field = "state"
equals = "resting"
cooldown = "12h"
during = { from = "22:00", to = "22:30" }

[ask]
policy = "nudge"

[[do]]
light = { dim = 1 }
"#;

    const CARD: &str = "---\nname: evening-dim\ndescription: Dims the light a shade at 10 pm.\n---\n\n# Evening dim\n\nAt ten it dims.\n";

    fn answer(toml: &str, card: &str) -> String {
        format!("Here it is.\n\n```skill.toml\n{toml}```\n\n```markdown SKILL.md\n{card}```\n")
    }

    /// The brief and the longest request fit in [`MOST_BRIEF_BYTES`], so a
    /// repair with the check's lines still fits the tasks part's 2,000;
    /// it carries the request and nothing else of the conversation, and a
    /// longer request is refused, never cut.
    #[test]
    fn the_brief_fits_the_hand_over_and_carries_only_the_request() {
        let request = "x".repeat(MOST_REQUEST_BYTES);
        let longest = brief(&request).unwrap();
        assert!(longest.len() <= MOST_BRIEF_BYTES, "{}", longest.len());
        assert!(is_brief(&longest));
        assert_eq!(request_of(&longest), request);
        let asked = brief("make me a skill  that dims\nthe light at 10 pm").unwrap();
        assert!(asked.ends_with("Request: make me a skill that dims the light at 10 pm"));
        // A time of day is the trigger's own window (#2813's rerun on
        // lamp-0c89: a 23.5 h [quiet] around 22:00, as the brief once said),
        // shown as its form with no time in it: a brief that showed 22:00 to
        // 22:30, a nod and a "Hi!" had a hello skill made with that window, a
        // glow and a nod nobody asked for (Fifth's row on #6249, 11 Nov).
        assert!(
            asked.contains("during = { from = \"HH:MM\", to = \"HH:MM\" }"),
            "{asked}"
        );
        let unasked = brief("nod when I stand up").unwrap();
        for value in ["22:", "\"nod\"", "Hi!"] {
            assert!(!unasked.contains(value), "{value}: {unasked}");
        }
        assert!(unasked.contains("only steps asked"), "{unasked}");
        assert!(!asked.contains("from its end to its start"), "{asked}");
        // The name is SKILL.md's alone: a `name =` in skill.toml fails
        // lamp-skills' check as an unknown field (Mobile on lamp-0c89, #2813).
        assert!(asked.contains("no name key"), "{asked}");
        assert!(brief(&"x".repeat(MOST_REQUEST_BYTES + 1)).is_err());
        assert!(brief("  ").is_err());
        assert!(!is_brief("Write a poem about lamps"));
    }

    /// `LAMP_AGENT_SERVICES` (the owner's ask, 12 Nov): on, the brief of a
    /// read or a send points to a service task for the lamp's agent in place
    /// of a read or a write; off, as before; the longest fits either way.
    #[test]
    fn a_services_brief_points_to_the_agents_task_only_with_its_switch() {
        set_writes_here(Some(true));
        for request in [
            "every morning read my new mail",
            "when I leave, email x@example.com that I'm out",
        ] {
            for reads in [true, false] {
                let on = brief_for(request, reads, true).unwrap();
                assert!(is_brief(&on) && request_of(&on) == request, "{on}");
                assert!(on.contains(" Or a task, as above."), "{on}");
                assert!(
                    !on.contains("read/write") && !on.contains("one write"),
                    "{on}"
                );
                let off = brief_for(request, reads, false).unwrap();
                assert!(!off.contains("Or a task"), "{off}");
                let said = if reads {
                    " Or a read/write as above."
                } else {
                    " Or one write, as above."
                };
                assert!(off.contains(said), "{off}");
            }
        }
        set_scripts_here(Some(true));
        let longest = brief_for(&"x".repeat(MOST_REQUEST_BYTES), true, true).unwrap();
        assert!(longest.len() <= MOST_BRIEF_BYTES, "{}", longest.len());
        set_scripts_here(None);
        set_writes_here(None);
    }

    /// The service grant codes are lamp-services' catalog codes, a
    /// service's second code (`gmail-app`, `figma-api`) folded to its
    /// first, in the catalog's order.
    #[test]
    fn the_service_grant_codes_are_the_catalogs() {
        let codes: Vec<&str> = lamp_services::catalog::SERVICES
            .iter()
            .map(|service| service.code)
            .collect();
        let firsts: Vec<&str> = codes
            .iter()
            .copied()
            .filter(|code| !code.contains('-'))
            .collect();
        assert_eq!(firsts, SERVICE_GRANT_CODES);
        for second in codes.iter().filter(|code| code.contains('-')) {
            let first = second.split('-').next().unwrap();
            assert!(SERVICE_GRANT_CODES.contains(&first), "{second}");
        }
    }

    /// #6432's ruling: a service task's grants are `<code>-read` and
    /// `<code>-write` of the catalog's codes, kept by the cap only with
    /// `LAMP_AGENT_SERVICES` on, and said back in plain words.
    #[test]
    fn a_service_tasks_grants_are_held_only_with_its_switch_and_said_back() {
        assert_eq!(service_grant("gmail-read"), Some(("gmail", false)));
        assert_eq!(
            service_grant("google_calendar-write"),
            Some(("google_calendar", true))
        );
        for no in [
            "gmail",
            "gmail-app-read",
            "-read",
            "drive-read",
            "github-read-write",
        ] {
            assert_eq!(service_grant(no), None, "{no}");
        }
        set_scripts_here(Some(true));
        let toml = CODE_TOML.replace(
            "grants = [\"camera\", \"light\"]",
            "grants = [\"gmail-read\"]",
        );
        set_agent_services_here(Some(false));
        assert!(capped(&toml).is_err());
        set_agent_services_here(Some(true));
        let (_, uses) = capped(&toml).unwrap();
        assert_eq!(uses.grants, ["gmail-read"]);
        assert!(code_said(&uses.grants).contains("reading your gmail"));
        assert!(
            code_said(&["gmail-write".to_owned()])
                .contains("sending with your gmail, each send only on your yes")
        );
        set_agent_services_here(None);
        set_scripts_here(None);
    }

    /// The owner, 10 Nov: whether a request asks for a linked service, a
    /// write or two services is the making model's to judge, never a word
    /// list's here. Every request, with reads on or off, is briefed with
    /// the request as it is said; with reads on the brief adds one line
    /// pointing to a read, and the longest fits [`MOST_BRIEF_BYTES`] either way.
    #[test]
    fn every_request_is_briefed_and_the_model_judges_it() {
        for request in [
            "read my mail every morning",
            "Glow blue when I get an E-mail",
            "every evening at nine, read me tomorrow's calendar",
            "glow red when Slack has a message",
            "read me my Notion plans",
            "send my calendar to the team",
            "read my calendar and my mail",
            "tomorrow book a meeting on my calendar",
            "read me my latest Facebook post",
            "dim the light at 10 pm",
        ] {
            for reads in [true, false] {
                let brief = brief_with(request, reads).unwrap();
                assert!(is_brief(&brief) && request_of(&brief) == request, "{brief}");
                assert_eq!(brief.contains(" Or one read, as above."), reads, "{brief}");
            }
        }
        let request = "x".repeat(MOST_REQUEST_BYTES);
        for reads in [true, false] {
            let longest = brief_with(&request, reads).unwrap();
            assert!(longest.len() <= MOST_BRIEF_BYTES, "{}", longest.len());
        }
        // With a made skill's writes on (#6066), a write pointed to too,
        // still within the bound.
        set_writes_here(Some(true));
        for (reads, said) in [
            (true, " Or a read/write as above."),
            (false, " Or one write, as above."),
        ] {
            let longest = brief_with(&request, reads).unwrap();
            assert!(longest.contains(said), "{longest}");
            assert!(longest.len() <= MOST_BRIEF_BYTES, "{}", longest.len());
        }
        set_writes_here(None);
    }

    /// #5288: the making model is shown each readable service's one step,
    /// from `lamp-services`' catalogue: a calendar's, mail's and Drive's
    /// whole, any other's fixed reads (never a query, a path or a whole
    /// text); a service with nothing a skill reads is only named, for its
    /// `cannot:`.
    #[test]
    fn the_read_steps_are_the_catalogues_readable_services() {
        let steps = read_steps();
        let step = |code: &str| {
            steps
                .iter()
                .find(|line| line.contains(&format!("({code}):")))
                .unwrap_or_else(|| panic!("{code}: {steps:?}"))
                .clone()
        };
        assert_eq!(
            step("google_calendar"),
            format!("Google Calendar (google_calendar): {CALENDAR_READ}")
        );
        assert!(CALENDAR_READ.contains("day = \"today\" }, or day = \"tomorrow\""));
        assert_eq!(step("gmail"), format!("Gmail (gmail): {GMAIL_READ}"));
        assert_eq!(
            step("google_drive"),
            format!("Google Drive (google_drive): {DRIVE_READ}")
        );
        // A lamp, 5 Nov: "read my latest Google Drive files" was made with
        // query = "order by modifiedTime desc", which matched no file.
        // Drive's step is given whole: newest first, query only search words.
        assert!(
            DRIVE_READ.contains("what = \"files\" }, no other key")
                && DRIVE_READ.contains("newest first")
                && DRIVE_READ.contains("query is search words")
        );
        // On 10300 the note's "(newest first; …)" right after the braces
        // came back as one more key in them, twice.
        assert!(!DRIVE_READ.contains('(') && !DRIVE_READ.contains("newest_first"));
        // Thirteen's P2 on #5303: fixed reads only, no query, path or call.
        let github = step("github");
        assert!(
            github.contains("notifications; issues {repo}; pulls {repo}; repos, its keys"),
            "{github}"
        );
        for line in &steps {
            assert!(
                !line.contains("graphql") && !line.contains("text {id}"),
                "{line}"
            );
        }
        for code in ["notion", "slack", "ahrefs", "hubspot", "gmail-app"] {
            assert!(
                !steps
                    .iter()
                    .any(|line| line.contains(&format!("({code}):"))),
                "{code}"
            );
        }
        let unreadable = unreadable_names();
        for name in ["Notion", "Slack", "Ahrefs", "HubSpot"] {
            assert!(unreadable.contains(&name), "{name}: {unreadable:?}");
        }
        assert!(!unreadable.contains(&"Google Calendar"));
    }

    /// A made skill that reads (#5288).
    const READ_TOML: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n[permissions]\n\
        speak = true\nservices = { google_calendar = \"read\" }\ntopics = [\"voice/state\"]\n\
        [trigger]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n\
        during = { from = \"21:00\", to = \"21:30\" }\n[ask]\npolicy = \"nudge\"\n\
        [[do]]\nread = { service = \"google_calendar\", what = \"events\", day = \"tomorrow\" }\n";

    /// #5288: its cap takes one linked service, as a read, and its voice
    /// for saying it, only with the switch on; what it does is said with the
    /// service, what it reads and the condition it reads under.
    #[test]
    fn a_read_skill_is_held_to_one_service_read_and_said_with_its_condition() {
        let (_, uses) = capped_with(READ_TOML, true).unwrap();
        assert_eq!(uses.reads, ["google_calendar"]);
        assert!(uses.speak);
        assert_eq!(
            uses.spoken(),
            "its voice, reading your Google Calendar and whether it is talking"
        );
        assert_eq!(uses.json()["reads"], "google_calendar");
        assert!(capped_with(READ_TOML, false).is_err());
        // A write, with LAMP_SKILL_WRITES off, is what the lamp can't do
        // yet (#6066); a second read, a service with nothing to read or
        // none listed is lamp-skills' check's to refuse
        // (`tests/it/made.rs`), no longer this cap's.
        set_writes_here(Some(false));
        let write = READ_TOML.replace("google_calendar = \"read\"", "google_calendar = \"write\"");
        assert_eq!(
            capped_with(&write, true).unwrap_err().0,
            cant_yet(SENDS_ON_ITS_OWN)
        );
        set_writes_here(None);
        assert_eq!(
            capped_with(READ_TOML, false).unwrap_err().0,
            cant_yet("reading your linked services")
        );
        for to in [
            "google_calendar = \"read\", gmail = \"read\"",
            "\"gmail-app\" = \"read\"",
        ] {
            let wider = READ_TOML.replace("google_calendar = \"read\"", to);
            assert!(capped_with(&wider, true).is_ok(), "{to}");
        }
        // Any other service with reads: a Drive list, said by name.
        let drive = READ_TOML
            .replace("google_calendar = \"read\"", "google_drive = \"read\"")
            .replace(
                "service = \"google_calendar\", what = \"events\", day = \"tomorrow\"",
                "service = \"google_drive\", what = \"files\", query = \"taxes\"",
            );
        let (_, uses) = capped_with(&drive, true).unwrap();
        assert_eq!(uses.reads, ["google_drive"]);
        assert!(uses.spoken().contains("reading your Google Drive"));
        let said = plainly_with(&drive, true).unwrap();
        assert!(
            does(&said).ends_with(
                "it reads you Google Drive's files: how many, then each name, never the text."
            ),
            "{said}"
        );
        // A whole text, or a what the service doesn't list, is never said.
        for what in ["what = \"text\", id = \"f\"", "what = \"nothing\""] {
            let wrong = drive.replace("what = \"files\", query = \"taxes\"", what);
            assert!(plainly_with(&wrong, true).is_err(), "{what}");
        }
        // Nor a free-form read: a query, a path, an MCP call (#5303's P2).
        for (service, step) in [
            (
                "github",
                "service = \"github\", what = \"query\", graphql = \"{ viewer { name: bio } }\"",
            ),
            (
                "ahrefs",
                "service = \"ahrefs\", what = \"get\", path = \"/v3/x\"",
            ),
            (
                "slack",
                "service = \"slack\", what = \"call\", tool = \"read\"",
            ),
        ] {
            let free = READ_TOML
                .replace(
                    "google_calendar = \"read\"",
                    &format!("{service} = \"read\""),
                )
                .replace(
                    "service = \"google_calendar\", what = \"events\", day = \"tomorrow\"",
                    step,
                );
            assert!(plainly_with(&free, true).is_err(), "{service}");
        }
        assert_eq!(
            plainly_with(READ_TOML, true).unwrap(),
            "When the lamp goes quiet, between 21:00 and 21:30, when you're alone with the lamp, \
             it reads you tomorrow's Google Calendar: how many events, then each time and title. It starts on its own."
        );
        assert!(plainly_with(READ_TOML, false).is_err());
        let mail = READ_TOML
            .replace("google_calendar = \"read\"", "gmail = \"read\"")
            .replace(
                "service = \"google_calendar\", what = \"events\", day = \"tomorrow\"",
                "service = \"gmail\", what = \"recent\"",
            );
        let said = plainly_with(&mail, true).unwrap();
        assert!(
            does(&said).ends_with("each sender and subject, never the text."),
            "{said}"
        );
        // The longest trigger's words still fit, said whole.
        let longest = mail
            .replace(
                "topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n",
                "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\nfor = \"10m\"\n",
            )
            .replace("\"voice/state\"]", "\"room/presence\"]");
        assert!(plainly_with(&longest, true).unwrap().chars().count() <= MOST_PLAIN_CHARS);
    }

    /// The answer's two files, each its own fenced block; another file, one
    /// twice, one missing, one left open or one too long refuses it.
    #[test]
    fn an_answer_gives_exactly_its_two_files() {
        let files = files(&answer(TOML, CARD)).unwrap();
        assert_eq!((files.toml.as_str(), files.card.as_str()), (TOML, CARD));
        let extra = format!("{}```moves/x.json\n{{}}\n```\n", answer(TOML, CARD));
        assert_eq!(files_err(&extra), NOT_A_SKILLS_FORM);
        let twice = format!("{}```skill.toml\n{TOML}```\n", answer(TOML, CARD));
        assert_eq!(files_err(&twice), NOT_A_SKILLS_FORM);
        assert_eq!(
            files_err(&format!("```skill.toml\n{TOML}```\n")),
            NOT_A_SKILLS_FORM
        );
        assert_eq!(files_err(&format!("```skill.toml\n{TOML}")), MODEL_CUT_OFF);
        let long = format!(
            "```skill.toml\n{}\n```\n```SKILL.md\n{CARD}```\n",
            "#".repeat(9_000)
        );
        assert!(files_err(&long).contains("more than a skill can hold"));
    }

    fn files_err(answer: &str) -> String {
        files(answer).unwrap_err().0
    }

    /// The fences the model writes (a product lamp on 11798, 11 Nov): its
    /// language for a name, any case, a space, a title, trailing spaces,
    /// tildes, longer runs, indented, bare, a closing fence at the end of
    /// a line, SKILL.md opened before skill.toml closed, a block within
    /// SKILL.md, a closing fence written twice: each read as the two files.
    #[test]
    fn the_fences_the_model_writes_give_the_two_files() {
        let two = |open_toml: &str, close_toml: &str, open_card: &str, close_card: &str| {
            format!("{open_toml}\n{TOML}{close_toml}\n\n{open_card}\n{CARD}{close_card}\n")
        };
        for answer in [
            two("```toml", "```", "```md", "```"),
            two("``` toml", "```", "``` markdown", "```"),
            two("```Skill.TOML  ", "```  ", "```skill.md", "```"),
            two("```toml title=\"skill.toml\"", "```", "```SKILL.md:", "```"),
            two("~~~toml", "~~~", "~~~SKILL.md", "~~~"),
            two("````skill.toml", "````", "````SKILL.md", "````"),
            two("  ```skill.toml", "  ```", "  ```SKILL.md", "  ```"),
            two("```", "```", "```", "```"),
            format!("```skill.toml\n{TOML}\n```SKILL.md\n{CARD}```\n"),
            format!("```skill.toml\n{TOML}```\n```SKILL.md\n{CARD}```\n```\n"),
        ] {
            let files = files(&answer).unwrap_or_else(|why| panic!("{why:?}\n{answer}"));
            assert_eq!(files.toml.trim_end(), TOML.trim_end(), "{answer}");
            assert_eq!(files.card.trim_end(), CARD.trim_end(), "{answer}");
            assert_eq!(open_fence(&answer), None, "{answer}");
        }
        // A closing fence at the end of the last line.
        let inline = format!(
            "```skill.toml\n{TOML}```\n```SKILL.md\n{}```\n",
            CARD.trim_end()
        );
        let got = files(&inline).unwrap();
        assert_eq!(got.card.trim_end(), CARD.trim_end());
        // A block within SKILL.md is its words.
        let card = format!("{CARD}\n```bash\necho hi\n```\n");
        let nested = format!("```skill.toml\n{TOML}```\n```SKILL.md\n{card}```\n");
        let got = files(&nested).unwrap();
        assert_eq!(got.card, card);
        assert_eq!(open_fence(&nested), None);
        // Left open at the end: the fence that closes it, of its own mark
        // and run, a line for each block open.
        let open = format!("```skill.toml\n{TOML}```\n~~~~SKILL.md\n{CARD}");
        assert_eq!(open_fence(&open).as_deref(), Some("~~~~"));
        assert_eq!(files_err(&open), MODEL_CUT_OFF);
        let closed = format!("{}\n~~~~\n", open.trim_end());
        assert_eq!(files(&closed).unwrap().card.trim_end(), CARD.trim_end());
        let within = format!("```skill.toml\n{TOML}```\n```SKILL.md\n{CARD}```bash\necho hi\n");
        assert_eq!(open_fence(&within).as_deref(), Some("```\n```"));
        // Another file still refuses it.
        let json = format!("{}```json\n{{}}\n```\n", answer(TOML, CARD));
        assert_eq!(files_err(&json), NOT_A_SKILLS_FORM);
    }

    /// A skill.toml that isn't TOML is refused with the parser's words for
    /// the repair: where it broke and the line, never a bare refusal.
    #[test]
    fn a_skill_toml_that_isnt_toml_carries_the_parsers_words() {
        let broken = TOML.replace("[permissions]", "[permissions");
        for refused in [
            capped(&broken),
            plainly(&broken).map(|said| (said, Uses::default())),
        ] {
            let Refused(why, lines) = refused.unwrap_err();
            assert_eq!(why, NOT_READABLE);
            let lines = lines.expect("lines for a repair");
            assert!(lines.starts_with("skill.toml is not TOML: "), "{lines}");
            assert!(lines.contains("line "), "{lines}");
            assert!(!lines.contains('\n'), "{lines}");
        }
    }

    /// The coordinator's cap (19 Oct) lifted on #6066: whatever a skill can
    /// declare is kept, and lamp-skills' check judges it; only a function
    /// for the model, a core kind and a missing version are refused here,
    /// and a write to a service is what the lamp can't do yet.
    #[test]
    fn a_made_skill_may_use_whatever_a_skill_can_declare() {
        let (version, uses) = capped(TOML).unwrap();
        assert_eq!(version, "1.0.0");
        assert_eq!(
            uses,
            Uses {
                light: true,
                body: false,
                topics: vec!["voice/state".into()],
                speak: false,
                reads: Vec::new(),
                ..Uses::default()
            }
        );
        assert_eq!(uses.spoken(), "its light and whether it is talking");
        // Each grant a skill can declare is kept, and named.
        for (wider, spoken) in [
            ("look = true", "the camera"),
            ("camera = true", "the camera"),
            ("messages = true", "messages to your phone"),
            ("known_faces = true", "whether it knows a face"),
            ("who = true", "who is at the lamp"),
            ("actions = [\"volume_up\"]", "its action volume_up"),
            ("output_limits = true", "limits on its light and volume"),
        ] {
            let text = TOML.replace("light = true\n", &format!("light = true\n{wider}\n"));
            let (_, uses) = capped(&text).unwrap_or_else(|why| panic!("{wider}: {why:?}"));
            assert!(uses.spoken().contains(spoken), "{wider}: {}", uses.spoken());
            assert_eq!(Uses::of_json(&uses.json()), uses, "{wider}");
        }
        // Format 2's look is presence alone, never the picture.
        let presence_look = TOML
            .replace("light = true\n", "light = true\nlook = true\n")
            .replace("version = \"1.0.0\"\n", "version = \"1.0.0\"\nformat = 2\n");
        assert!(!capped(&presence_look).unwrap().1.camera);
        // A key no skill has, or a core grant, is lamp-skills' check's.
        let frobs = TOML.replace("light = true\n", "light = true\nfrobs = true\n");
        assert!(capped(&frobs).is_ok());
        let unclear = TOML.replace("light = true\n", "light = \"yes\"\n");
        assert!(capped(&unclear).unwrap_err().0.contains("couldn't tell"));
        // Any topic: presence and the clock, and what is heard, named as
        // the microphone.
        let presence = TOML.replace(
            "[\"voice/state\"]",
            "[\"humans/faces/tracked\", \"room/stood_up\", \"clock/local\"]",
        );
        assert_eq!(
            capped(&presence).unwrap().1.spoken(),
            "its light, whether it sees a face, someone getting up from the desk and the time \
             of day"
        );
        for wider in ["air/state", "ears/classes", "name/heard", "voice/heard"] {
            let text = TOML.replace("[\"voice/state\"]", &format!("[\"{wider}\"]"));
            let (_, uses) = capped(&text).unwrap_or_else(|why| panic!("{wider}: {why:?}"));
            assert_eq!(uses.listens(), wider != "air/state", "{wider}");
        }
        // A write to a service, with LAMP_SKILL_WRITES off, is what the
        // lamp can't do yet, never repaired away.
        set_writes_here(Some(false));
        let write = TOML.replace(
            "light = true\n",
            "light = true\nservices = { gmail = \"write\" }\n",
        );
        assert_eq!(
            capped_with(&write, true).unwrap_err(),
            Refused(cant_yet(SENDS_ON_ITS_OWN), None)
        );
        set_writes_here(None);
        let tool = format!("tool = \"dim_now\"\n{TOML}");
        assert!(capped(&tool).unwrap_err().0.contains("runs by itself"));
        let core = TOML.replace("installable", "core");
        assert!(capped(&core).unwrap_err().0.contains("installable"));
        // Any [ask]: a hello, news or a reminder, said back.
        for (policy, said) in [
            ("reminder", ", as a reminder."),
            ("hello", ", as a hello."),
            ("news", ", as news."),
        ] {
            let text = TOML.replace("policy = \"nudge\"", &format!("policy = \"{policy}\""));
            assert!(capped(&text).is_ok(), "{policy}");
            assert!(does(&plainly(&text).unwrap()).ends_with(said), "{policy}");
        }
        let unversioned = TOML.replace("version = \"1.0.0\"\n", "");
        assert!(capped(&unversioned).unwrap_err().0.contains("version"));
        assert!(capped("not toml [").is_err());
        let presence = TOML
            .replace("[\"voice/state\"]", "[\"room/presence\", \"voice/state\"]")
            .replace("light = true\n", "light = true\nbody = true\n");
        assert_eq!(
            capped(&presence).unwrap().1.spoken(),
            "its light, its head and arm, whether someone is at the desk and whether it is talking"
        );
    }

    /// #6055, the app's own example: "Say hello when I sit down at my
    /// desk", a say step on the desk's presence, its voice asked for.
    const HELLO: &str = r#"kind = "installable"
version = "1.0.0"

[permissions]
speak = true
topics = ["room/presence"]

[trigger]
topic = "room/presence"
field = "someone"
equals = true
for = "5s"
cooldown = "1h"
rearm = "ends"

[ask]
policy = "nudge"

[[do]]
say = { text = "Welcome back!" }
"#;

    /// #6062: a made skill may say its exact words, a say step alone or with
    /// a light and a gesture, said back plainly with the words quoted whole.
    #[test]
    fn a_made_skill_may_say_its_exact_words() {
        for reads in [false, true] {
            let (_, uses) = capped_with(HELLO, reads).unwrap();
            assert!(uses.speak && !uses.light && uses.reads.is_empty());
            assert_eq!(
                uses.spoken(),
                "its voice and whether someone is at the desk"
            );
            assert_eq!(
                plainly_with(HELLO, reads).unwrap(),
                "When someone has been at the desk for 5 seconds, it says “Welcome back!”, at \
                 most once an hour. It starts on its own."
            );
        }
        // Two short sentences, a comma and "it's", in any language, are
        // words said as written: the comma stays inside the quotes (P3).
        for words in [
            "Welcome back, it's good to see you. Ready?",
            "Chào mừng bạn trở lại!",
        ] {
            let text = HELLO.replace("Welcome back!", words);
            capped(&text).unwrap();
            assert!(
                plainly(&text)
                    .unwrap()
                    .contains(&format!("it says “{words}”, at most once an hour.")),
                "{words}"
            );
        }
        let both = HELLO
            .replace(
                "speak = true\n",
                "speak = true\nlight = true\nbody = true\n",
            )
            .replace(
                "[[do]]\n",
                "[[do]]\nlight = { colour = \"warm\", level = 3, for = \"20s\" }\n\n\
                 [[do]]\nbody = { gesture = \"nod\" }\n\n[[do]]\n",
            );
        capped(&both).unwrap();
        assert_eq!(
            plainly(&both).unwrap(),
            "When someone has been at the desk for 5 seconds, it glows warm at level 3 for 20 \
             seconds, then nods, then says “Welcome back!”, at most once an hour. It starts on its own."
        );
    }

    /// #6062: a say that isn't exact words (a bare note, `{ note }`, a list),
    /// or words too long, three sentences, quoted, with a web address, a
    /// control or a format character, is refused, plainly and with lines for
    /// a repair; a say with no voice asked for, or a voice with nothing to
    /// say, too.
    #[test]
    fn a_say_that_isnt_exact_plain_words_is_refused() {
        for say in [
            "\"greet them warmly\"".to_owned(),
            "{ note = \"greet them warmly\" }".to_owned(),
            "{ text = [\"Hi.\", \"Hello.\"] }".to_owned(),
            "{ text = \"Hi.\", priority = 9 }".to_owned(),
            "{ from = \"tasks\" }".to_owned(),
            format!("{{ text = \"{}\" }}", "word ".repeat(25)),
            "{ text = \"Hi. Then. Tell them the news.\" }".to_owned(),
            "{ text = \"Say \\\"hello there\\\"\" }".to_owned(),
            "{ text = \"See https://example.com now\" }".to_owned(),
            "{ text = \"See www.example.com\" }".to_owned(),
            "{ text = \"Hi {name}\" }".to_owned(),
            "{ text = \"Hi\\nsecond line\" }".to_owned(),
            "{ text = \"Hi\\u200B there\" }".to_owned(),
            "{ text = \"Hi \\u202Eereht\" }".to_owned(),
            "{ text = \"   \" }".to_owned(),
        ] {
            let text = HELLO.replace("{ text = \"Welcome back!\" }", &say);
            match capped(&text) {
                Err(Refused(why, Some(lines))) => {
                    assert_eq!(why, SAY_NOT_TEXT, "{say}");
                    assert!(lines.contains("speak for a say step"), "{lines}");
                    assert!(lines.contains("the exact words"), "{lines}");
                }
                other => panic!("{say}: {other:?}"),
            }
            match plainly_with(&text, false) {
                Err(Refused(why, Some(lines))) => {
                    assert_eq!(why, UNSAID);
                    assert!(lines.contains(SAY_TEXT_PART), "{lines}");
                    assert!(lines.contains("one say = { text"), "{lines}");
                    assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
                }
                other => panic!("{say}: {other:?}"),
            }
        }
        // 120 characters is kept, 121 is not.
        let most = HELLO.replace("Welcome back!", &"é".repeat(MOST_SAY_TEXT_CHARS));
        assert!(capped(&most).is_ok());
        let over = HELLO.replace("Welcome back!", &"é".repeat(MOST_SAY_TEXT_CHARS + 1));
        assert_eq!(capped(&over).unwrap_err().0, SAY_NOT_TEXT);
        let twice = format!("{HELLO}\n[[do]]\nsay = {{ text = \"How are you?\" }}\n");
        assert!(capped(&twice).unwrap_err().0.contains("more than one line"));
        let unvoiced = HELLO.replace("speak = true\n", "");
        assert!(
            capped(&unvoiced)
                .unwrap_err()
                .0
                .contains("without asking for its voice")
        );
        let silent = HELLO.replace(
            "say = { text = \"Welcome back!\" }",
            "body = { gesture = \"nod\" }",
        );
        assert!(
            capped(&silent)
                .unwrap_err()
                .0
                .contains("never says anything")
        );
        // Beside a say, any grant (#6066): kept, and named when said back.
        for (wider, named) in [
            ("camera = true", ", using the camera."),
            (
                "messages = true",
                ", and it may send a message to your phone.",
            ),
        ] {
            let text = HELLO.replace("speak = true\n", &format!("speak = true\n{wider}\n"));
            assert!(capped(&text).is_ok(), "{wider}");
            assert!(does(&plainly(&text).unwrap()).ends_with(named), "{wider}");
        }
    }

    /// #6062, P2a and the owner's rule (a lifted limit is set, never a new
    /// refusal): a say skill's missing or short cooldown is set to an hour,
    /// a longer one kept; on the lamp's resting it runs only within
    /// 07:00-22:00, its window set or narrowed to them. Only a window all
    /// night can't be made safe: the cap refuses it, asking the repair to
    /// drop the say.
    #[test]
    fn a_say_skills_pace_is_set_not_refused() {
        for (cooldown, kept) in [
            ("", "1h"),
            ("cooldown = \"20m\"\n", "1h"),
            ("cooldown = \"5s\"\n", "1h"),
            ("cooldown = \"1h30m\"\n", "1h"),
            ("cooldown = \"3h\"\n", "3h"),
            ("cooldown = \"90m\"\n", "90m"),
            // Too long to count in seconds: unreadable, so an hour (#6070).
            ("cooldown = \"18446744073709551615h\"\n", "1h"),
            ("cooldown = \"18446744073709551615m\"\n", "1h"),
        ] {
            let text = HELLO.replace("cooldown = \"1h\"\n", cooldown);
            let paced = paced(&text);
            let table: toml::Table = toml::from_str(&paced).unwrap();
            assert_eq!(
                table["trigger"]["cooldown"].as_str(),
                Some(kept),
                "{cooldown}"
            );
            capped(&paced).unwrap();
            // Unpaced, the cap refuses it, never says it plainly as shorter.
            if kept == "1h" && !cooldown.is_empty() {
                assert!(
                    capped(&text)
                        .unwrap_err()
                        .0
                        .contains("more than once an hour")
                );
            }
        }
        assert!(
            does(&plainly(&paced(&HELLO.replace("cooldown = \"1h\"\n", ""))).unwrap())
                .ends_with("at most once an hour.")
        );
        // A skill that never speaks is given back as it came.
        assert_eq!(say_paced(TOML), TOML);
        assert_eq!(say_paced(HELLO), HELLO);
        let resting = HELLO
            .replace("topics = [\"room/presence\"]", "topics = [\"voice/state\"]")
            .replace(
                "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\nfor = \"5s\"\n",
                "topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n",
            )
            .replace("rearm = \"ends\"\n", "");
        assert!(capped(&resting).unwrap_err().0.contains("resting at night"));
        for (during, window) in [
            ("", Some(("07:00", "22:00"))),
            (
                "during = { from = \"09:00\", to = \"10:00\" }\n",
                Some(("09:00", "10:00")),
            ),
            (
                "during = { from = \"21:00\", to = \"23:30\" }\n",
                Some(("21:00", "22:00")),
            ),
            (
                "during = { from = \"20:00\", to = \"08:00\" }\n",
                Some(("20:00", "22:00")),
            ),
            ("during = { from = \"23:00\", to = \"06:00\" }\n", None),
        ] {
            let text = resting.replace(
                "cooldown = \"1h\"\n",
                &format!("cooldown = \"1h\"\n{during}"),
            );
            let paced = paced(&text);
            match window {
                Some((from, to)) => {
                    let table: toml::Table = toml::from_str(&paced).unwrap();
                    let during = &table["trigger"]["during"];
                    assert_eq!(
                        (during["from"].as_str(), during["to"].as_str()),
                        (Some(from), Some(to)),
                        "{text}"
                    );
                    capped(&paced).unwrap();
                    // The night kept quiet unless it runs within 08-22.
                    let night = if from < "08:00" {
                        "never from 22:00 to 08:00, "
                    } else {
                        ""
                    };
                    assert_eq!(
                        plainly(&paced).unwrap(),
                        format!(
                            "When the lamp goes quiet, between {from} and {to}, it says \
                             “Welcome back!”, at most once an hour. It starts on its own, at \
                             most 3 times a day, one an hour with the lamp's other nudges, \
                             {night}and rests till morning if ignored twice."
                        )
                    );
                }
                None => {
                    assert_eq!(
                        say_paced(&text),
                        text,
                        "an all-night window is left for the cap"
                    );
                    let refused = capped(&paced).unwrap_err();
                    assert!(refused.0.contains("drop its say step"), "{}", refused.0);
                }
            }
        }
    }

    /// A made skill may play a sound: only the lamp's own soft hum, the
    /// voice's built-in cue, alone or with a light and a gesture, said
    /// back plainly.
    #[test]
    fn a_made_skill_may_play_the_lamps_soft_hum() {
        let hum = HELLO.replace("say = { text = \"Welcome back!\" }", "cue = \"hum\"");
        for reads in [false, true] {
            let (_, uses) = capped_with(&hum, reads).unwrap();
            assert!(uses.speak && !uses.light && uses.reads.is_empty());
            assert_eq!(
                plainly_with(&hum, reads).unwrap(),
                "When someone has been at the desk for 5 seconds, it hums softly, at most once \
                 an hour. It starts on its own."
            );
        }
        let both = hum
            .replace(
                "speak = true\n",
                "speak = true\nlight = true\nbody = true\n",
            )
            .replace(
                "[[do]]\n",
                "[[do]]\nlight = { colour = \"warm\", level = 3, for = \"20s\" }\n\n\
                 [[do]]\nbody = { gesture = \"nod\" }\n\n[[do]]\n",
            );
        capped(&both).unwrap();
        assert_eq!(
            plainly(&both).unwrap(),
            "When someone has been at the desk for 5 seconds, it glows warm at level 3 for 20 \
             seconds, then nods, then hums softly, at most once an hour. It starts on its own."
        );
        // Paced as a say is (#6062): a missing cooldown is set to an hour.
        let unpaced = hum.replace("cooldown = \"1h\"\n", "");
        assert!(paced(&unpaced).contains("cooldown = \"1h\""));
        capped(&paced(&unpaced)).unwrap();
        // Too long to count is paced to an hour, never wrapped (#6070).
        for unit in ["h", "m"] {
            let huge = hum.replace(
                "cooldown = \"1h\"",
                &format!("cooldown = \"18446744073709551615{unit}\""),
            );
            let table: toml::Table = toml::from_str(&paced(&huge)).unwrap();
            assert_eq!(table["trigger"]["cooldown"].as_str(), Some("1h"));
            capped(&paced(&huge)).unwrap();
        }
    }

    /// A sound the lamp doesn't ship, by name, path or file, is what it
    /// can't do yet (#6066), said so with no repair; one sound and no say
    /// beside it, as `lamp-skills` takes it, and its voice asked for.
    #[test]
    fn a_sound_the_lamp_doesnt_ship_is_refused() {
        assert_eq!(SOUND_NOT_SHIPPED, cant_yet(OTHER_SOUND));
        assert_eq!(
            not_made_line(SOUND_NOT_SHIPPED),
            "I can't do that yet: a sound other than its soft hum."
        );
        for step in [
            "cue = \"chime\"",
            "cue = \"Hum\"",
            "cue = \"hum.wav\"",
            "cue = \"sounds/hum.wav\"",
            "cue = \"/opt/lampOS/voice/notices/wifi-lost.en-US.wav\"",
            "cue = { name = \"hum\" }",
            "sound = \"hum\"",
            "sound = \"../hum.wav\"",
        ] {
            let text = HELLO.replace("say = { text = \"Welcome back!\" }", step);
            let cant = Refused(SOUND_NOT_SHIPPED.into(), None);
            assert_eq!(capped(&text).unwrap_err(), cant, "{step}");
            assert_eq!(plainly_with(&text, false).unwrap_err(), cant, "{step}");
        }
        let hum = HELLO.replace("say = { text = \"Welcome back!\" }", "cue = \"hum\"");
        // A hum beside its line, or a second hum: the repair is told to
        // leave the extra hum out, never a scolding.
        for twice in ["cue = \"hum\"", "say = { text = \"How are you?\" }"] {
            let text = format!("{hum}\n[[do]]\n{twice}\n");
            match capped(&text) {
                Err(Refused(why, Some(lines))) => {
                    assert_eq!(why, HUM_DROPPED, "{twice}");
                    assert!(lines.contains("leave out the extra cue"), "{lines}");
                    assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
                }
                other => panic!("{twice}: {other:?}"),
            }
        }
        let unvoiced = hum.replace("speak = true\n", "");
        assert!(
            capped(&unvoiced)
                .unwrap_err()
                .0
                .contains("without asking for its voice")
        );
    }

    /// The card's name, `made: here` and its maker in its front matter,
    /// once each, and the description given in place of the one written;
    /// a card without them, or with a name that is not a skill's, is
    /// refused. A maker the model wrote is not kept.
    #[test]
    fn the_card_is_marked_made_here_and_by_whom() {
        let said = "When the lamp goes quiet, it dims the light a step.";
        let (name, marked) = card(CARD, said, Maker::Voice).unwrap();
        assert_eq!(name, "evening-dim");
        assert_eq!(marked.matches("made: here").count(), 1);
        assert_eq!(marked.matches("made_by: voice\n").count(), 1);
        assert!(marked.starts_with("---\nname: evening-dim\n"));
        assert!(marked.ends_with("At ten it dims.\n"));
        assert_eq!(marked.matches("description:").count(), 1);
        assert!(marked.contains(&format!("description: \"{said}\"\n")));
        assert!(!marked.contains("10 pm"));
        let quoted = card(CARD, "glows \"red\"", Maker::Voice).unwrap().1;
        assert!(quoted.contains("description: \"glows 'red'\"\n"));
        let again = card(
            &CARD.replace("name:", "made: there\nmade_by: voice\nname:"),
            said,
            Maker::App,
        )
        .unwrap()
        .1;
        assert_eq!(again.matches("made:").count(), 1);
        assert_eq!(again.matches("made_by:").count(), 1);
        assert!(again.contains("\nmade: here\nmade_by: app\n---"), "{again}");
        // `limits: asked` is the lamp's to write alone, after the maker.
        let written = CARD.replace("name:", "limits: asked\nname:");
        let unasked = card(&written, said, Maker::Voice).unwrap().1;
        assert!(!unasked.contains("limits:"), "{unasked}");
        let asked = card_asked(&written, said, Maker::Voice, true).unwrap().1;
        assert_eq!(asked.matches("limits:").count(), 1);
        assert!(
            asked.contains(&format!("\nmade_by: voice\n{LIMITS_ASKED}\n---")),
            "{asked}"
        );
        assert!(card("# No front matter\n", said, Maker::Voice).is_err());
        assert!(
            card(
                &CARD.replace("evening-dim", "Evening Dim"),
                said,
                Maker::Voice
            )
            .is_err()
        );
        assert!(
            card(
                &CARD.replace("description: Dims the light a shade at 10 pm.\n", ""),
                said,
                Maker::Voice
            )
            .is_err()
        );
    }

    /// The maker is the app's for an app's ask, made here or written there,
    /// and the voice's for any other.
    #[test]
    fn the_maker_is_told_by_where_the_skill_was_made_from() {
        assert_eq!(Maker::of("local:app:9:1"), Maker::App);
        assert_eq!(Maker::of("written:app:9:1"), Maker::App);
        assert_eq!(Maker::of("local:1800000000000"), Maker::Voice);
        assert_eq!(Maker::of("local:application"), Maker::Voice);
        assert_eq!(Maker::of("s:1:2"), Maker::Voice);
    }

    /// What a made skill does is said from its `skill.toml`, not the
    /// model's words: lamp-0c89's "good night" skill (Mobile on #4710,
    /// 3 Nov) is said as the quiet minute it is.
    #[test]
    fn what_it_does_is_said_from_its_toml() {
        let good_night = TOML.replace(
            "cooldown = \"12h\"\n",
            "for = \"1m\"\ncooldown = \"1h\"\nrearm = \"ends\"\n",
        );
        assert_eq!(
            plainly(&good_night).unwrap(),
            "When the lamp has been quiet for a minute, between 22:00 and 22:30, it dims the \
             light a step, at most once an hour. It starts on its own."
        );
        assert_eq!(
            plainly(TOML).unwrap(),
            "When the lamp goes quiet, between 22:00 and 22:30, it dims the light a step, at \
             most once every 12 hours. It starts on its own."
        );
        let back = r#"[trigger]
topic = "room/presence"
field = "someone"
equals = true
for = "5s"
cooldown = "1h"

[[do]]
light = { colour = "cyan", level = 3, for = "20s" }

[[do]]
body = { gesture = "nod" }
"#;
        assert_eq!(
            plainly(back).unwrap(),
            "When someone has been at the desk for 5 seconds, it glows cyan at level 3 for 20 \
             seconds, then nods, at most once an hour. It starts on its own."
        );
        let left = back
            .replace(
                "equals = true\nfor = \"5s\"\n",
                "equals = false\nfor = \"10m\"\n",
            )
            .replace("gesture = \"nod\"", "gesture = \"curious\"");
        assert!(plainly(&left).unwrap().starts_with(
            "When the desk has been empty for 10 minutes, it glows cyan at level 3 for 20 \
             seconds, then looks up, curious,"
        ));
    }

    /// A trigger or a step it can't say is refused, with lines for a
    /// repair that name the part, by its keys and topic alone: the person
    /// is never asked about something they weren't told.
    #[test]
    fn what_it_cant_say_is_refused_with_lines_for_a_repair() {
        let refused = |toml: &str, part: &str, trigger: bool| match plainly_with(toml, false) {
            Err(Refused(why, Some(lines))) => {
                assert_eq!(why, UNSAID);
                let fix = if trigger {
                    SAYABLE_TRIGGERS
                } else {
                    PLAIN_ONLY
                };
                assert_eq!(lines, format!("{UNSAID_PART}{part}. {fix}"));
                assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
            }
            other => panic!("{other:?}"),
        };
        let voice = "trigger topic voice/state";
        refused(
            &TOML.replace("cooldown", "once_per_day = true\ncooldown"),
            &format!("{voice}, keys once_per_day"),
            true,
        );
        refused(
            &TOML.replace("equals = \"resting\"", "equals = \"talking\""),
            &format!("{voice}, its field and equals"),
            true,
        );
        refused(
            &TOML.replace("voice/state\"\nfield", "room/activity\"\nfield"),
            "trigger topic room/activity, its field and equals: state, head_down, at_desk or leaning_in",
            true,
        );
        // A real topic of the lamp's that no saying knows is what it can't
        // start a made skill on yet (#6066): said so, with no repair.
        assert_eq!(
            plainly_with(
                &TOML.replace("voice/state\"\nfield", "voice/heard\"\nfield"),
                false
            )
            .unwrap_err(),
            Refused(cant_yet("starting on words it hears"), None)
        );
        refused(
            &TOML.replace("{ dim = 1 }", "{ dim = 1, for = \"1m\" }"),
            &format!("[[do]] step 1, light (dim, for){LIGHT_PART}"),
            false,
        );
        refused(
            &TOML.replace("light = { dim = 1 }", "say = \"good night\""),
            &format!("[[do]] step 1, say{SAY_TEXT_PART}"),
            false,
        );
        refused(
            &TOML.replace("[[do]]\nlight = { dim = 1 }\n", ""),
            "no [[do]] step",
            false,
        );
        refused(
            &TOML.replace("cooldown = \"12h\"", "cooldown = \"1h30m\""),
            &format!("{voice}, its cooldown"),
            true,
        );
        refused(
            &TOML.replace(
                "during = { from = \"22:00\", to = \"22:30\" }",
                "during = {}",
            ),
            &format!("{voice}, its during"),
            true,
        );

        // Thirteen's and Mobile's P2s on #5042: a part that would run
        // otherwise than the words, and more steps than one breath says,
        // never cut.
        refused(
            &format!(
                "{TOML}\n[[while]]\ntopic = \"room/presence\"\nfield = \"someone\"\nequals = true\n"
            ),
            "parts while",
            false,
        );
        // The manners a person asked for are said since #6066 (a); one in
        // another shape is unsaid.
        assert!(
            plainly(&format!(
                "{TOML}\n[quiet]\nfrom = \"21:00\"\nto = \"08:00\"\n"
            ))
            .unwrap()
            .ends_with(" It starts on its own, never from 21:00 to 08:00.")
        );
        assert!(
            plainly(&TOML.replace("version = ", "most_a_day = 1\nversion = "))
                .unwrap()
                .ends_with(" It starts on its own, once a day.")
        );
        refused(
            &format!("{TOML}\n[quiet]\nfrom = \"9 pm\"\nto = \"08:00\"\n"),
            "parts quiet",
            false,
        );
        refused(
            &TOML.replace("version = ", "most_a_day = \"3\"\nversion = "),
            "parts most_a_day",
            false,
        );
        refused(
            &TOML.replace("version = ", "enabled = false\nversion = "),
            "parts enabled",
            false,
        );
        refused(
            &TOML.replace("[trigger]", "[[trigger]]"),
            "[[trigger]] written as a list, its first trigger topic voice/state",
            true,
        );
        let many = "\n[[do]]\nlight = { colour = \"warm\", level = 2, for = \"20s\" }\n".repeat(6);
        refused(
            &format!("{TOML}{many}\n[[do]]\nbody = {{ gesture = \"nod\" }}\n"),
            "what it does, too long for one breath",
            false,
        );
        // A key the model wrote that is no plain name is never repeated.
        refused(
            &TOML.replace("cooldown", "\"say \\\"hi\\\"\" = 1\ncooldown"),
            &format!("{voice}, keys another"),
            true,
        );
    }

    /// lamp-74ff, 10 Nov 17:39: an app make of "glow green when I come back
    /// to my desk" written as the built-in welcome back writes its trigger
    /// (a [[trigger]] on the faces, a [[while]]) was refused twice, its
    /// repair told only the keys a trigger may have. Now its repair names
    /// the faces' topic and the keys, and lists the triggers it can have,
    /// room/presence for someone coming back; the person is told plainly,
    /// with what to try.
    #[test]
    fn a_welcome_back_shaped_trigger_is_named_to_its_repair_with_room_presence() {
        let back = r#"kind = "installable"
version = "1.0.0"
needs = ["light"]

[permissions]
topics = ["room/presence"]
light = true

[[trigger]]
topic = "humans/faces/tracked"
when = "appears"
near = 0.14
absent_for = "5m"
for = "3s"
gap = "3s"
cooldown = "20m"

[[while]]
topic = "voice/state"
field = "state"
equals = "resting"

[ask]
policy = "nudge"

[[do]]
light = { colour = "green", level = 3, for = "20s" }
"#;
        let Err(Refused(why, Some(lines))) = plainly_with(back, false) else {
            panic!("a trigger on the faces is unsaid");
        };
        assert_eq!(why, UNSAID);
        assert!(why.contains("when I come to my desk"), "{why}");
        assert!(
            lines.starts_with(
                "Can't be said plainly: [[trigger]] written as a list, its first trigger topic \
                 humans/faces/tracked, keys gap, near; parts while. One [trigger] table"
            ),
            "{lines}"
        );
        assert!(lines.contains(SAYABLE_TRIGGERS), "{lines}");
        assert!(
            lines.contains(
                "someone comes or comes back to the desk: \"room/presence\", \"someone\", \
                 equals true"
            ),
            "{lines}"
        );
        assert!(lines.contains("the lamp goes quiet: \"voice/state\""));
        assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
        // One [trigger] table, on the faces: the topic and its keys named.
        let one = back.replace("[[trigger]]", "[trigger]").replace(
            "[[while]]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n",
            "",
        );
        let Err(Refused(_, Some(lines))) = plainly_with(&one, false) else {
            panic!("a trigger on the faces is unsaid");
        };
        assert!(
            lines.starts_with(
                "Can't be said plainly: trigger topic humans/faces/tracked, keys gap, near."
            ),
            "{lines}"
        );
        // The repair goes back whole within the tasks part's bound.
        let repair = repair_brief(&"x".repeat(MOST_REQUEST_BYTES), &lines).unwrap();
        assert!(repair.contains(&lines), "{repair}");
        assert!(repair.len() <= crate::agent_tasks::MOST_TASK_BYTES);
        // Written as the repair says, it is said.
        let fixed = one.replace(
            "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\nnear = 0.14\n",
            "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\nwhen = \"appears\"\n",
        )
        .replace("gap = \"3s\"\n", "");
        assert_eq!(
            plainly_with(&fixed, false).unwrap(),
            "When someone has been back at the desk for 3 seconds, after it was empty for 5 \
             minutes, it glows green at level 3 for 20 seconds, at most once every 20 minutes. It starts on its own."
        );
    }

    /// A made skill with `trigger` and one `step`, reading presence and the
    /// clock, its light and its body.
    /// Thirteen's and App Only's P2 on 355d255189: a self-starting skill
    /// the lamp couldn't set its manners on is never kept unmannered. An
    /// inline `permissions` with `messages`, dotted `quiet.*` keys and a
    /// description line `[rest]` each come out mannered, or unsaid for a
    /// repair; none is said with a send.
    #[test]
    fn a_skill_is_never_kept_without_its_manners_whatever_its_shape() {
        let pet = "topic = \"controls/pet\"\nfield = \"ended\"\nequals = false\nrearm = \"ends\"\n";
        let nod = made_with(pet, "body = { gesture = \"nod\" }");
        let version = "version = \"1.0.0\"\n";
        let inline = nod
            .replace(
                "[permissions]\nlight = true\nbody = true\n",
                "permissions = { light = true, body = true, messages = true, topics = [] }\n",
            )
            .replace(
                "topics = [\"humans/faces/tracked\", \"room/stood_up\", \"clock/local\"]\n",
                "",
            );
        let shapes = [
            ("inline permissions", inline),
            (
                "dotted quiet",
                nod.replace(
                    version,
                    &format!("{version}quiet.from = \"03:00\"\nquiet.to = \"03:01\"\n"),
                ),
            ),
            (
                "dotted grant",
                nod.replace(
                    "[permissions]\nlight = true\nbody = true\ntopics = [\"humans/faces/tracked\", \
                     \"room/stood_up\", \"clock/local\"]\n",
                    "permissions.light = true\npermissions.body = true\n\
                     permissions.messages = true\n",
                ),
            ),
            (
                "a description line [rest]",
                nod.replace(
                    version,
                    &format!(
                        "{version}description = \"\"\"Nods.\n[rest]\nafter_ignored = 9\n\"\"\"\n"
                    ),
                ),
            ),
        ];
        for (shape, text) in shapes {
            let table: toml::Table =
                toml::from_str(&text).unwrap_or_else(|error| panic!("{shape}: {error}"));
            if shape == "dotted grant" {
                assert_eq!(table["permissions"]["messages"].as_bool(), Some(true));
            }
            let paced = paced(&text);
            let set: toml::Table = toml::from_str(&paced).unwrap();
            // Only the inline table is left to the repair: the lamp sets
            // the manners on the others.
            assert_eq!(
                manners_kept(&set, &[]),
                shape != "inline permissions",
                "{shape}: {paced}"
            );
            match said_mannered(&paced, &[]) {
                Ok(said) => {
                    assert!(manners_kept(&set, &[]), "{shape}: {paced}");
                    assert!(!said.contains("send a message"), "{shape}: {said}");
                    assert!(
                        said.contains("never from 22:00 to 08:00"),
                        "{shape}: {said}"
                    );
                }
                // Unsaid for a repair: its manners, or a part plainly
                // can't say (a description).
                Err(Refused(why, Some(lines))) => {
                    assert_eq!(why, UNSAID, "{shape}");
                    let part = match shape {
                        "inline permissions" => "parts manners",
                        "a description line [rest]" => "parts description",
                        _ => panic!("{shape}: {lines}"),
                    };
                    assert!(lines.contains(part), "{shape}: {lines}");
                }
                other => panic!("{shape}: {other:?}"),
            }
        }
        // Unpaced, a self-starting skill has none: never said, repaired.
        assert!(matches!(
            said_mannered(&nod, &[]),
            Err(Refused(why, Some(_))) if why == UNSAID
        ));
        // The evening dim, a time asked for, needs no quiet hours.
        assert!(said_mannered(&paced(TOML), &[]).is_ok());
    }

    fn made_with(trigger: &str, step: &str) -> String {
        format!(
            "kind = \"installable\"\nversion = \"1.0.0\"\n\n[permissions]\nlight = true\n\
             body = true\ntopics = [\"humans/faces/tracked\", \"room/stood_up\", \
             \"clock/local\"]\n\n[trigger]\n{trigger}\n[ask]\npolicy = \"nudge\"\n\n\
             [[do]]\n{step}\n"
        )
    }

    /// #6066: each step and trigger kind a skill can declare is kept and
    /// said back: a pat, stuffy air and what someone at the desk is doing;
    /// a light's dim, out, shade and pattern, a turn to the face, the
    /// reading pose, the camera's look and a stand; the camera and an
    /// outbound grant named.
    #[test]
    fn every_kind_a_skill_can_declare_is_kept_and_said() {
        let kept_and_said = |trigger: &str, step: &str, said: &str| {
            let text = made_with(trigger, step);
            assert!(capped_with(&text, false).is_ok(), "{text}");
            assert_eq!(
                plainly_with(&text, false).unwrap(),
                format!("{said} It starts on its own."),
                "{step}"
            );
        };
        let pet = "topic = \"controls/pet\"\nfield = \"ended\"\nequals = false\nrearm = \"ends\"\n";
        let air = "topic = \"air/state\"\nfield = \"co2_ppm\"\nabove = 1200\nfor = \"10m\"\n";
        let down = "topic = \"room/activity\"\nfield = \"state\"\nequals = \"head_down\"\n\
                    for = \"45m\"\n";
        for (trigger, step, said) in [
            (
                pet,
                "body = { gesture = \"happy\" }",
                "When someone pats its head, it wiggles happily.",
            ),
            (
                air,
                "light = { dim = 1, until = \"ends\" }",
                "When the air has been stuffy, CO2 over 1200 ppm, for 10 minutes, it dims the \
                 light a step while that lasts.",
            ),
            (
                down,
                "light = { out = true, over = \"5s\" }",
                "When someone is head down at the desk for 45 minutes, it turns the light off \
                 slowly over 5 seconds.",
            ),
            (
                pet,
                "light = { relative = { warmer = 1, dimmer = 2 }, for = \"1m\" }",
                "When someone pats its head, it makes your light a shade warmer and 2 steps \
                 dimmer for a minute.",
            ),
            (
                pet,
                "light = { colour = \"orange\", level = 2, for = \"20s\", pattern = { kind = \
                 \"breathe\", period = \"6s\" } }",
                "When someone pats its head, it glows orange at level 2 for 20 seconds, \
                 breathing.",
            ),
            (
                pet,
                "body = { look_at = \"face\", gesture = \"nod\" }",
                "When someone pats its head, it turns to the face it sees and nods.",
            ),
            (
                pet,
                "body = { mood = \"reading\", for = \"1m\" }",
                "When someone pats its head, it holds its reading pose for a minute.",
            ),
            (
                pet,
                "look = true",
                "When someone pats its head, it takes a look with the camera.",
            ),
            (
                pet,
                "stand = 60",
                "When someone pats its head, it stands at 60 percent of its height.",
            ),
        ] {
            kept_and_said(trigger, step, said);
        }
        // The camera and an outbound grant, named in the said-back line.
        let camera = made_with(pet, "body = { gesture = \"nod\" }")
            .replace("body = true\n", "body = true\ncamera = true\n");
        assert!(capped_with(&camera, false).unwrap().1.camera);
        assert_eq!(
            plainly_with(&camera, false).unwrap(),
            "When someone pats its head, it nods, using the camera. It starts on its own."
        );
        let looks = camera.replace("body = { gesture = \"nod\" }", "look = true");
        assert_eq!(
            plainly_with(&looks, false).unwrap(),
            "When someone pats its head, it takes a look with the camera. It starts on its own."
        );
        let sends = made_with(pet, "body = { gesture = \"nod\" }")
            .replace("body = true\n", "body = true\nmessages = true\n");
        assert!(capped_with(&sends, false).unwrap().1.messages);
        assert_eq!(
            plainly_with(&sends, false).unwrap(),
            "When someone pats its head, it nods, and it may send a message to your phone. It starts \
             on its own."
        );
        // A step no runtime has yet: a send, a move of its own. Said as
        // what the lamp can't do yet, before the check, never repaired.
        for (step, what) in [
            (
                "text = \"I'm home\"",
                "sending a message or writing to a linked service",
            ),
            ("body = { move = \"spin\" }", "a move of its own"),
        ] {
            let text = made_with(pet, step);
            let cant = Refused(cant_yet(what), None);
            assert_eq!(capped_with(&text, false).unwrap_err(), cant, "{step}");
            assert_eq!(plainly_with(&text, false).unwrap_err(), cant, "{step}");
            assert_eq!(not_made_line(&cant.0), format!("{CANT_YET} {what}."));
        }
    }

    /// #6066 (a), the first builder's review: a made skill that starts
    /// itself is given the nudges' manners, never refused for lacking
    /// them; the say-back names that it starts on its own and how often. A
    /// grant no step uses is dropped (App Only's P2 on #6103). A skill a
    /// tool starts is left as it came.
    #[test]
    fn a_self_starting_made_skill_is_given_manners_not_refused() {
        let pet = "topic = \"controls/pet\"\nfield = \"ended\"\nequals = false\nrearm = \"ends\"\n";
        let wide = made_with(pet, "body = { gesture = \"nod\" }").replace(
            "body = true\n",
            "body = true\ncamera = true\nmessages = true\nknown_faces = true\n",
        );
        let mannered = paced(&wide);
        let table: toml::Table = toml::from_str(&mannered).unwrap();
        assert_eq!(table["most_a_day"].as_integer(), Some(3));
        assert_eq!(table["quiet"]["from"].as_str(), Some("22:00"));
        assert_eq!(table["quiet"]["to"].as_str(), Some("08:00"));
        assert_eq!(table["budget"]["group"].as_str(), Some(MANNERS_GROUP));
        assert_eq!(table["budget"]["per_hour"].as_integer(), Some(1));
        assert_eq!(table["rest"]["after_ignored"].as_integer(), Some(2));
        // No step sends or takes the picture: both grants dropped, not
        // refused; knowing who is in view said (Thirteen's P3).
        let granted = table["permissions"].as_table().unwrap();
        assert!(!granted.contains_key("camera") && !granted.contains_key("messages"));
        let (_, uses) = capped_with(&mannered, false).unwrap();
        assert!(!uses.camera && !uses.messages);
        assert_eq!(
            plainly_with(&mannered, false).unwrap(),
            "When someone pats its head, it nods, knowing who is in view. It starts on its own, \
             at most 3 times a day, one an hour with the lamp's other nudges, never from 22:00 \
             to 08:00, and rests till morning if ignored twice."
        );
        // Set once: pacing again changes nothing.
        assert_eq!(paced(&mannered), mannered);
        // Started by its tool alone, it isn't started by the lamp.
        let tool = "tool = \"dim_now\"\n[[do]]\nlight = { dim = 1 }\n";
        assert_eq!(super::mannered(tool), tool);
    }

    /// Thirteen's and App Only's P2 on #6103: each manner the model writes
    /// is clamped to its bound, never refused, and said as set.
    #[test]
    fn a_made_skills_manners_are_clamped_to_their_bounds() {
        let pet = "topic = \"controls/pet\"\nfield = \"ended\"\nequals = false\nrearm = \"ends\"\n";
        let nod = made_with(pet, "body = { gesture = \"nod\" }");
        let set = |head: &str, tail: &str| {
            let text = paced(&format!("{head}{nod}{tail}"));
            capped_with(&text, false).unwrap();
            (
                toml::from_str::<toml::Table>(&text).unwrap(),
                plainly_with(&text, false).unwrap(),
            )
        };
        // most_a_day: 1 to 6.
        for (asked, kept, said) in [
            (50, 6, "at most 6 times a day"),
            (6, 6, "at most 6 times a day"),
            (0, 1, "once a day"),
            (-3, 1, "once a day"),
        ] {
            let (table, plain) = set(&format!("most_a_day = {asked}\n"), "");
            assert_eq!(table["most_a_day"].as_integer(), Some(kept), "{asked}");
            assert!(plain.contains(said), "{plain}");
        }
        // [budget]: always the nudges' group, 1 or 2 an hour.
        for (asked, kept, said) in [
            (
                "group = \"mine\"\nper_hour = 30\n",
                2,
                "two an hour with the lamp's other nudges",
            ),
            (
                "group = \"wellbeing\"\nper_hour = 0\n",
                1,
                "one an hour with the lamp's other nudges",
            ),
            (
                "group = \"mine\"\nper_hour = 1\ngap = \"5m\"\n",
                1,
                "one an hour",
            ),
        ] {
            let (table, plain) = set("", &format!("\n[budget]\n{asked}"));
            assert_eq!(table["budget"]["group"].as_str(), Some(MANNERS_GROUP));
            assert_eq!(
                table["budget"]["per_hour"].as_integer(),
                Some(kept),
                "{asked}"
            );
            assert!(plain.contains(said), "{plain}");
        }
        // [rest] after_ignored: 1 to 3, 0 or less is 1.
        for (asked, kept, said) in [
            (1000, 3, "ignored 3 times"),
            (0, 1, "ignored once"),
            (-1, 1, "ignored once"),
        ] {
            let (table, plain) = set("", &format!("\n[rest]\nafter_ignored = {asked}\n"));
            assert_eq!(
                table["rest"]["after_ignored"].as_integer(),
                Some(kept),
                "{asked}"
            );
            assert!(plain.contains(said), "{plain}");
        }
        // [quiet] the nudges' night, whatever was written.
        let (table, plain) = set("", "\n[quiet]\nfrom = \"03:00\"\nto = \"04:00\"\n");
        assert_eq!(table["quiet"]["from"].as_str(), Some("22:00"));
        assert!(plain.contains("never from 22:00 to 08:00"), "{plain}");
    }

    /// #6066: a made skill on stuffy air is kept with the nudges' manners
    /// and an hour's cooldown at least, set not refused; one on a person's
    /// face by whose it is is what the lamp can't do yet (the trigger sees
    /// a face, never whose), said so with no repair.
    #[test]
    fn stuffy_air_is_paced_and_one_persons_face_is_cant_yet() {
        let air = "topic = \"air/state\"\nfield = \"co2_ppm\"\nabove = 1200\nfor = \"10m\"\n";
        for (cooldown, kept) in [
            ("", "1h"),
            ("cooldown = \"5m\"\n", "1h"),
            ("cooldown = \"3h\"\n", "3h"),
        ] {
            let text = paced(&made_with(
                &format!("{air}{cooldown}"),
                "light = { colour = \"blue\", level = 2, for = \"6s\" }",
            ));
            let table: toml::Table = toml::from_str(&text).unwrap();
            assert_eq!(
                table["trigger"]["cooldown"].as_str(),
                Some(kept),
                "{cooldown}"
            );
            assert_eq!(table["budget"]["group"].as_str(), Some(MANNERS_GROUP));
            capped_with(&text, false).unwrap();
            let said = plainly_with(&text, false).unwrap();
            assert!(
                said.contains("It starts on its own, at most 3 times a day"),
                "{said}"
            );
        }
        for key in ["name = \"Mai\"", "known = \"Mai\"", "who = \"Mai\""] {
            let text = paced(&made_with(
                &format!("topic = \"humans/faces/tracked\"\nwhen = \"appears\"\n{key}\n"),
                "cue = \"hum\"",
            ));
            assert_eq!(
                plainly_with(&text, false).unwrap_err(),
                Refused(cant_yet(ONE_PERSON), None),
                "{key}"
            );
        }
        assert_eq!(
            not_made_line(&cant_yet(ONE_PERSON)),
            "I can't do that yet: starting when it sees one particular person."
        );
    }

    /// Thirteen's P2 on #6103 and his 21:51 follow-up: the night stays quiet
    /// unless the trigger runs only within 08:00-22:00, or at a time the
    /// request's own words name (3 hours at most, within it). A window the
    /// model chose alone, or an all-day one, keeps it.
    #[test]
    fn the_night_stays_quiet_unless_its_window_is_within_the_day() {
        let pet = "topic = \"controls/pet\"\nfield = \"ended\"\nequals = false\nrearm = \"ends\"\n";
        let quiet = |request: &str, during: &str| {
            let text = paced_asked(
                &made_with(
                    &format!("{pet}during = {{ {during} }}\n"),
                    "body = { gesture = \"nod\" }",
                ),
                &asked_windows(request),
            );
            let table: toml::Table = toml::from_str(&text).unwrap();
            assert!(manners_kept(&table, &asked_windows(request)), "{text}");
            table.contains_key("quiet")
        };
        let no_time = "make a skill that nods when someone pats its head";
        // Within the day, asked or not: no quiet hours.
        assert!(!quiet(no_time, "from = \"08:00\", to = \"22:00\""));
        assert!(!quiet(no_time, "from = \"12:00\", to = \"13:00\""));
        // A night window the model chose alone keeps them.
        assert!(quiet(no_time, "from = \"01:00\", to = \"03:00\""));
        assert!(quiet(no_time, "from = \"22:00\", to = \"22:30\""));
        assert!(quiet(no_time, "from = \"00:00\", to = \"23:59\""));
        assert!(quiet(no_time, "from = \"07:00\", to = \"09:00\""));
        // A time the person named runs as said.
        for request in [
            "dim the light at ten tonight",
            "dim the light at 10 pm",
            "dim the light at 22:00",
            "dim the light from 22:00 to 22:30",
            "giảm đèn lúc 10 giờ tối",
            "dim the light at ten",
        ] {
            assert!(
                !quiet(request, "from = \"22:00\", to = \"22:30\""),
                "{request}"
            );
            // Not at another hour of the night, nor wider than asked.
            assert!(
                quiet(request, "from = \"01:00\", to = \"03:00\""),
                "{request}"
            );
            assert!(
                quiet(request, "from = \"00:00\", to = \"23:59\""),
                "{request}"
            );
        }
        assert!(!quiet(
            "nod from 23:30 to 01:00",
            "from = \"23:30\", to = \"01:00\""
        ));
        assert!(quiet(
            "nod from 22:00 to 01:01",
            "from = \"22:00\", to = \"01:01\""
        ));
        assert!(quiet("dim at 10 am", "from = \"22:00\", to = \"22:30\""));
    }

    /// The times a request names, as windows of minutes ([`asked_windows`]).
    #[test]
    fn the_times_a_request_names_are_read_from_its_words() {
        let ten_pm = (22 * 60, 23 * 60);
        assert_eq!(asked_windows("dim the light at 10 pm"), [ten_pm]);
        assert_eq!(asked_windows("dim the light at 10pm"), [ten_pm]);
        assert_eq!(asked_windows("dim the light at ten tonight"), [ten_pm]);
        assert_eq!(asked_windows("giảm đèn lúc 10 giờ tối"), [ten_pm]);
        assert_eq!(asked_windows("at 22:00 dim it"), [ten_pm]);
        assert_eq!(
            asked_windows("from 22:00 to 22:30"),
            [(22 * 60, 22 * 60 + 30)]
        );
        assert_eq!(asked_windows("at ten"), [(10 * 60, 11 * 60), ten_pm]);
        assert_eq!(asked_windows("at 7 am"), [(7 * 60, 8 * 60)]);
        assert!(asked_windows("make a skill that nods when someone pats its head").is_empty());
        assert!(asked_windows("glow when two people come").is_empty());
    }

    /// A skill added from a file keeps the cap from before #6066 (App
    /// Only's P2 on #6103), and every service a made skill reads is asked
    /// to be linked (Thirteen's P3).
    #[test]
    fn a_skill_from_a_file_keeps_the_old_cap_and_every_read_is_asked_for() {
        let tool = "kind = \"installable\"\nversion = \"1.0.0\"\ntool = \"glow_back\"\n\
                    [permissions]\nlight = true\n[[do]]\nlight = { colour = \"cyan\" }\n";
        assert!(capped_added(tool).is_ok());
        for wider in [
            "camera = true",
            "messages = true",
            "known_faces = true",
            "who = true",
        ] {
            let text = tool.replace("light = true\n", &format!("light = true\n{wider}\n"));
            assert!(
                capped_added(&text)
                    .unwrap_err()
                    .0
                    .contains("more than the light and the head"),
                "{wider}"
            );
        }
        let both = READ_TOML.replace(
            "google_calendar = \"read\"",
            "google_calendar = \"read\", google_drive = \"read\"",
        );
        let (_, uses) = capped_with(&both, true).unwrap();
        assert_eq!(uses.reads.len(), 2);
        assert_eq!(Uses::of_json(&uses.json()), uses);
        let needs = needs_link(&uses, || Some(vec!["google_drive".into()])).unwrap();
        assert_eq!(needs.len(), 1);
        assert_eq!(needs[0]["code"], "google_calendar");
        let needs = needs_link(&uses, || Some(Vec::new())).unwrap();
        assert_eq!(needs.len(), 2);
    }

    /// A face coming into view (never whose), someone at the desk getting
    /// up, and a time each day are said as lamp-skills runs them, and kept.
    #[test]
    fn a_face_a_getting_up_and_a_time_each_day_are_said() {
        let kept_and_said = |trigger: &str, step: &str, said: &str| {
            let text = made_with(trigger, step);
            assert!(capped_with(&text, false).is_ok(), "{text}");
            assert_eq!(
                plainly_with(&text, false).unwrap(),
                format!("{said} It starts on its own.")
            );
        };
        let face = "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\n";
        kept_and_said(
            &format!("{face}cooldown = \"1h\"\n"),
            "body = { gesture = \"greeting\" }",
            "When the lamp sees a face, after seeing none, it lifts its head and arm in a \
             greeting, at most once an hour.",
        );
        kept_and_said(
            &format!("{face}for = \"3s\"\nabsent_for = \"10m\"\n"),
            "light = { colour = \"warm\", level = 2, for = \"20s\" }",
            "When the lamp has seen a face for 3 seconds, after seeing none for 10 minutes, it \
             glows warm at level 2 for 20 seconds.",
        );
        kept_and_said(
            "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\nwhen = \"appears\"\n",
            "body = { gesture = \"headshake\" }",
            "When someone sitting at the desk gets up, it shakes its head.",
        );
        let clock = "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n";
        kept_and_said(
            &format!("{clock}during = {{ from = \"22:00\", to = \"22:01\" }}\n"),
            "light = { dim = 1 }",
            "Every day at 22:00, it dims the light a step.",
        );
        kept_and_said(
            &format!(
                "{clock}during = {{ from = \"23:59\", to = \"00:00\" }}\ncooldown = \"12h\"\n"
            ),
            "body = { gesture = \"sleepy\" }",
            "Every day at 23:59, it droops sleepily, at most once every 12 hours.",
        );
    }

    /// #6066: a time on named days of the week, as lamp-skills reads
    /// `days`, is kept and said with its days ("every Wednesday and Friday
    /// at 3:17 in the morning", refused on lamp-9adf staging 11625).
    #[test]
    fn a_time_on_named_weekdays_is_kept_and_said_with_its_days() {
        let clock = "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
                     during = { from = \"03:17\", to = \"03:18\" }\n";
        for (days, every) in [
            ("[\"wed\", \"fri\"]", "Every Wednesday and Friday"),
            ("[\"fri\", \"wed\", \"fri\"]", "Every Wednesday and Friday"),
            ("\"sun\"", "Every Sunday"),
            (
                "[\"mon\", \"wed\", \"sat\"]",
                "Every Monday, Wednesday and Saturday",
            ),
            ("\"weekdays\"", "Every weekday"),
            ("[\"weekend\"]", "Every Saturday and Sunday"),
            ("[\"weekdays\", \"weekend\"]", "Every day"),
            ("[\"weekdays\", \"mon\"]", "Every weekday"),
        ] {
            let text = made_with(&format!("{clock}days = {days}\n"), "light = { dim = 1 }");
            assert!(capped_with(&text, false).is_ok(), "{text}");
            assert!(
                paced(&text).contains(&format!("\ndays = {days}\n")),
                "{days}"
            );
            assert_eq!(
                plainly_with(&text, false).unwrap(),
                format!("{every} at 03:17, it dims the light a step. It starts on its own."),
                "{days}"
            );
        }
        // #6103's quiet rule holds as for a time each day: a weekday time
        // the request names runs as said, a night one the model chose alone
        // keeps 22:00-08:00 quiet, one within the day needs none.
        for (request, during, kept_quiet) in [
            (
                "every Wednesday and Friday at 3:17 in the morning, dim the light",
                "03:17",
                false,
            ),
            (
                "on Wednesdays and Fridays, dim the light at night",
                "03:17",
                true,
            ),
            ("on Wednesdays and Fridays, dim the light", "09:00", false),
            (
                "every Wednesday and Friday at 10 pm, dim the light",
                "22:00",
                false,
            ),
        ] {
            let asked = asked_windows(request);
            let quiet = |days: &str| {
                let minute = one_minute_after(during);
                let text = paced_asked(
                    &made_with(
                        &format!(
                            "topic = \"clock/local\"\nfield = \"known\"\nequals = true\n\
                             rearm = \"ends\"\nduring = {{ from = \"{during}\", to = \
                             \"{minute}\" }}\n{days}"
                        ),
                        "light = { dim = 1 }",
                    ),
                    &asked,
                );
                let table: toml::Table = toml::from_str(&text).unwrap();
                assert!(manners_kept(&table, &asked), "{text}");
                assert!(said_mannered(&text, &asked).is_ok(), "{text}");
                table.contains_key("quiet")
            };
            assert_eq!(quiet(""), kept_quiet, "{request}");
            assert_eq!(
                quiet("days = [\"wed\", \"fri\"]\n"),
                kept_quiet,
                "{request}"
            );
        }
        // One that speaks, as the voice makes it: its hour and its manners
        // set, said with its days.
        let request = "every Wednesday and Friday at 3:17 in the morning, say time to take out \
                       the bins";
        let asked = asked_windows(request);
        let text = paced_asked(
            &made_with(
                &format!("{clock}days = [\"wed\", \"fri\"]\n"),
                "say = { text = \"Time to take out the bins.\" }",
            )
            .replace("body = true\n", "body = true\nspeak = true\n"),
            &asked,
        );
        assert!(capped_with(&text, false).is_ok(), "{text}");
        let said = said_mannered(&text, &asked).unwrap();
        assert!(
            said.starts_with(
                "Every Wednesday and Friday at 03:17, it says “Time to take out the bins.”, at \
                 most once an hour."
            ),
            "{said}"
        );
    }

    /// The minute after `HH:MM`, as `HH:MM`.
    fn one_minute_after(at: &str) -> String {
        let minutes = minutes(at).unwrap();
        hhmm((minutes + 1) % (24 * 60))
    }

    /// #6070, the Reviewer-lead's P2, by #6066's rule (a): a made skill on a
    /// face coming into view, a gesture or a light alone, runs only after
    /// seeing none for a minute, set and never refused: missing or shorter
    /// is set to `"1m"`, a longer one kept; `rearm` alone or `"present"`
    /// becomes `when = "appears"`, which `absent_for` needs in lamp-skills.
    /// One on someone getting up has a cooldown of a minute or more; one
    /// that speaks keeps its hour.
    #[test]
    fn a_face_skill_waits_a_minute_away_and_a_getting_up_a_minute() {
        let face = "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\n";
        let greeting = "body = { gesture = \"greeting\" }";
        for (absent, kept) in [
            ("", "1m"),
            ("absent_for = \"10s\"\n", "1m"),
            ("absent_for = \"59s\"\n", "1m"),
            ("absent_for = \"soon\"\n", "1m"),
            ("absent_for = \"18446744073709551615h\"\n", "1m"),
            ("absent_for = \"60s\"\n", "60s"),
            ("absent_for = \"5m\"\n", "5m"),
        ] {
            let text = made_with(&format!("{face}{absent}"), greeting);
            let set = paced(&text);
            let table: toml::Table = toml::from_str(&set).unwrap();
            assert_eq!(
                table["trigger"]["absent_for"].as_str(),
                Some(kept),
                "{absent}"
            );
            assert_eq!(table["trigger"]["when"].as_str(), Some("appears"));
            // No cooldown is added: the minute away is its manners.
            assert!(table["trigger"].get("cooldown").is_none(), "{set}");
            capped_with(&set, false).unwrap();
            assert_eq!(paced(&set), set, "paced once is paced");
        }
        assert_eq!(
            plainly_with(&paced(&made_with(face, greeting)), false).unwrap(),
            "When the lamp sees a face, after seeing none for a minute, it lifts its head and \
             arm in a greeting. It starts on its own, at most 3 times a day, one an hour with \
             the lamp's other nudges, never from 22:00 to 08:00, and rests till morning if \
             ignored twice."
        );
        // `rearm` alone, or `"present"` beside it: appears, so the minute
        // counts.
        for when in [
            "rearm = \"ends\"\n",
            "when = \"present\"\nrearm = \"ends\"\n",
        ] {
            let text = made_with(
                &format!("topic = \"humans/faces/tracked\"\n{when}"),
                greeting,
            );
            let table: toml::Table = toml::from_str(&paced(&text)).unwrap();
            assert_eq!(table["trigger"]["when"].as_str(), Some("appears"), "{when}");
            assert_eq!(
                table["trigger"]["absent_for"].as_str(),
                Some("1m"),
                "{when}"
            );
            capped_with(&paced(&text), false).unwrap();
        }
        // A face skill that speaks gets both: the hour and the minute away.
        let hello = made_with(face, "say = { text = \"Hello again!\" }")
            .replace("body = true\n", "speak = true\n");
        let table: toml::Table = toml::from_str(&paced(&hello)).unwrap();
        assert_eq!(table["trigger"]["cooldown"].as_str(), Some("1h"));
        assert_eq!(table["trigger"]["absent_for"].as_str(), Some("1m"));
        // Someone getting up: a cooldown of a minute, a longer one kept.
        let stood = "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n";
        for (cooldown, kept) in [
            ("", "1m"),
            ("cooldown = \"20s\"\n", "1m"),
            ("cooldown = \"10m\"\n", "10m"),
        ] {
            let text = made_with(
                &format!("{stood}{cooldown}"),
                "body = { gesture = \"headshake\" }",
            );
            let table: toml::Table = toml::from_str(&paced(&text)).unwrap();
            assert_eq!(
                table["trigger"]["cooldown"].as_str(),
                Some(kept),
                "{cooldown}"
            );
            assert!(table["trigger"].get("absent_for").is_none());
            capped_with(&paced(&text), false).unwrap();
        }
        // Other triggers, silent, keep their own floors: only the manners
        // are set, the night kept quiet: no request named the time.
        let clock = made_with(
            "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
             during = { from = \"22:00\", to = \"22:01\" }\n",
            "light = { dim = 1 }",
        );
        assert_eq!(say_paced(&clock), clock);
        assert!(paced(&clock).contains("[quiet]"));
    }

    /// What lamp-skills would run otherwise than said, or that reads who
    /// someone is, is refused: a face by a field or a list (a name, a known
    /// face), a face held while in view, a getting up held a while, a time
    /// over a window, a day of the week lamp-skills doesn't name, or days
    /// on anything but a time.
    #[test]
    fn a_face_by_who_a_window_or_an_unknown_day_is_refused() {
        let part =
            |trigger: &str| match plainly_with(&made_with(trigger, "light = { dim = 1 }"), false) {
                Err(Refused(why, Some(lines))) => {
                    assert_eq!(why, UNSAID);
                    assert!(lines.contains(SAYABLE_TRIGGERS), "{lines}");
                    assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
                    lines
                }
                other => panic!("{trigger}: {other:?}"),
            };
        let faces = "Can't be said plainly: trigger topic humans/faces/tracked";
        let face = "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\n";
        assert!(
            part(&format!("{face}field = \"name\"\nequals = \"Alex\"\n")).starts_with(&format!(
                "{faces}, its field and equals: a face in view has no test, never who."
            ))
        );
        assert!(
            part(&format!("{face}each = \"faces\"\nkey = \"known\"\n"))
                .starts_with(&format!("{faces}, keys each, key."))
        );
        assert!(part(&format!("{face}near = 0.2\n")).starts_with(&format!("{faces}, keys near.")));
        assert!(
            part("topic = \"humans/faces/tracked\"\nfor = \"5s\"\n")
                .starts_with(&format!("{faces}, its when"))
        );
        // Known faces may be declared (#6066), but no trigger here reads
        // who: a face in view is said as any face.
        let known = made_with(face, "light = { dim = 1 }")
            .replace("body = true\n", "body = true\nknown_faces = true\n");
        assert!(capped_with(&known, false).unwrap().1.known_faces);
        let stood = "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n";
        assert!(part(&format!("{stood}for = \"10s\"\n")).contains("its for or absent_for"));
        assert!(part(&stood.replace("above = 0", "above = 600")).contains("its field and above"));
        let clock = "topic = \"clock/local\"\nfield = \"known\"\nequals = true\n";
        let at = "during = { from = \"22:00\", to = \"22:01\" }\n";
        let clocked = "Can't be said plainly: trigger topic clock/local";
        for trigger in [
            format!("{clock}{at}"),
            format!("{clock}rearm = \"ends\"\nduring = {{ from = \"22:00\", to = \"22:30\" }}\n"),
            format!("{clock}rearm = \"ends\"\nduring = {{ from = \"9:00\", to = \"9:01\" }}\n"),
            format!("{clock}rearm = \"ends\"\nfor = \"1m\"\n{at}"),
            format!("{clock}rearm = \"ends\"\n"),
        ] {
            assert!(
                part(&trigger).starts_with(&format!("{clocked}, its test, rearm and during")),
                "{trigger}"
            );
        }
        // `days` as lamp-skills reads them (#6066), and only on a time.
        for (days, why) in [
            (
                "[\"wednesday\"]",
                "wednesday is not mon to sun, weekdays or weekend.",
            ),
            ("[\"Wed\"]", "Wed is not mon to sun"),
            ("\"weekly\"", "weekly is not mon to sun"),
            ("[]", "it names no day."),
            ("[\"mon\", 3]", "another is not mon to sun"),
            ("[\"mon\", \"a b\"]", "another is not mon to sun"),
        ] {
            let said = part(&format!("{clock}rearm = \"ends\"\n{at}days = {days}\n"));
            assert!(
                said.starts_with(&format!("{clocked}, its days: {why}")),
                "{days}: {said}"
            );
        }
        assert!(
            part(
                "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\n\
                 days = [\"mon\"]\n"
            )
            .contains("its days, only on a time of day")
        );
        assert!(
            part("topic = \"voice/state\"\nfield = \"state\"\nabove = 0\n").contains("its above")
        );
    }

    /// Every gesture lamp-skills plays for a skill (the body's clips but
    /// `idle`) has a saying, one a step; a name the body hasn't, `idle`, or
    /// a table of clips is refused.
    #[test]
    fn every_gesture_of_the_body_is_said_and_no_other() {
        let clips: Vec<&str> = lamp_body::clips::ALL
            .iter()
            .map(|clip| clip.name)
            .filter(|name| *name != "idle")
            .collect();
        for name in &clips {
            assert!(gesture_said(name).is_some(), "{name} has no saying");
        }
        assert_eq!(gesture_names(), clips.join(", "));
        let presence = "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\n";
        let said = |gesture: &str| {
            plainly_with(
                &made_with(presence, &format!("body = {{ gesture = \"{gesture}\" }}")),
                false,
            )
        };
        for (gesture, saying) in [
            ("dance_waltz", "leans in a slow waltz"),
            ("laugh", "tips its head back in a laugh"),
            ("scanning", "looks all round the room"),
            ("nod", "nods"),
        ] {
            assert_eq!(
                said(gesture).unwrap(),
                format!("When someone comes to the desk, it {saying}. It starts on its own.")
            );
        }
        for refused in ["moonwalk", "idle", "dip", "tilt_left", "Nod"] {
            match said(refused) {
                Err(Refused(why, Some(lines))) => {
                    assert_eq!(why, UNSAID);
                    assert!(lines.starts_with("Can't be said plainly: [[do]] step 1, body"));
                    assert!(lines.contains("a listed gesture"), "{lines}");
                }
                other => panic!("{refused}: {other:?}"),
            }
        }
        let two = made_with(presence, "body = { gesture = \"nod\", move = \"wave\" }");
        assert!(plainly_with(&two, false).is_err());
        let table = made_with(presence, "body = { gesture = { calm = [\"nod\"] } }");
        assert!(plainly_with(&table, false).is_err());
    }

    /// `lamp-skills` counts a trigger's `absent_for` only with
    /// `when = "appears"`: on someone coming to the desk it is said as
    /// coming back; anywhere else, or without "appears", it is unsaid.
    #[test]
    fn someone_coming_back_after_a_while_away_is_said() {
        let back = |rest: &str| {
            format!(
                "[trigger]\ntopic = \"room/presence\"\nfield = \"someone\"\nequals = true\n\
                 {rest}rearm = \"ends\"\n\n[[do]]\nlight = {{ colour = \"green\", level = 2, \
                 for = \"20s\" }}\n"
            )
        };
        assert_eq!(
            plainly(&back("when = \"appears\"\nabsent_for = \"10m\"\n")).unwrap(),
            "When someone comes back to the desk after it was empty for 10 minutes, it glows \
             green at level 2 for 20 seconds. It starts on its own."
        );
        assert_eq!(
            plainly(&back("when = \"appears\"\n")).unwrap(),
            "When someone comes to the desk, it glows green at level 2 for 20 seconds. It starts on its own."
        );
        assert_eq!(
            plainly(&back("when = \"present\"\nfor = \"5s\"\n")).unwrap(),
            "When someone has been at the desk for 5 seconds, it glows green at level 2 for 20 \
             seconds. It starts on its own."
        );
        let part = |toml: &str| match plainly_with(toml, false) {
            Err(Refused(_, Some(lines))) => lines,
            other => panic!("{other:?}"),
        };
        let presence = "Can't be said plainly: trigger topic room/presence";
        assert!(part(&back("absent_for = \"10m\"\n")).starts_with(&format!(
            "{presence}, its absent_for, which goes with when = \"appears\"."
        )));
        assert!(
            part(&back("when = \"changes\"\n"))
                .starts_with(&format!("{presence}, its {WHEN_PART}."))
        );
        assert!(
            part(&back("when = \"appears\"\nabsent_for = \"soon\"\n"))
                .starts_with(&format!("{presence}, its absent_for"))
        );
        let left = back("when = \"appears\"\nabsent_for = \"10m\"\n")
            .replace("equals = true", "equals = false");
        assert!(part(&left).starts_with(&format!(
            "{presence}, its absent_for, only for someone coming back."
        )));
    }

    /// The making model's instructions and the brief say what "come back
    /// to my desk" is, so the first answer is on the desk's presence.
    #[test]
    fn the_brief_and_the_triggers_say_what_coming_back_to_the_desk_is() {
        let brief = brief("glow green when I come back to my desk").unwrap();
        assert!(
            brief.contains("one [trigger] as listed (coming to the desk is room/presence)"),
            "{brief}"
        );
        assert!(TRIGGER_MEANINGS.contains("someone comes or comes back to the desk"));
        assert!(TRIGGER_MEANINGS.contains("when = \"appears\", absent_for = \"5m\""));
        assert!(SAYABLE_TRIGGERS.contains(trigger_meanings!()));
        assert!(TRIGGER_MEANINGS.contains("\"controls/pet\", \"ended\", equals false"));
        assert!(TRIGGER_MEANINGS.contains(
            "}. On some days only, add days = [\"wed\", \"fri\"] (mon to sun, \"weekdays\", \
             \"weekend\"). Also: a pat"
        ));
    }

    /// The person hears why a make failed in plain words, with what to try:
    /// no file's, part's or program's name.
    #[test]
    fn the_reasons_a_person_hears_are_plain() {
        for why in [
            UNSAID,
            MODEL_CUT_OFF,
            NOT_A_SKILLS_FORM,
            NOT_READABLE,
            NOT_PASSED,
        ] {
            for jargon in [
                "skill.toml",
                "SKILL.md",
                "lamp-skills",
                "[trigger]",
                "plainly",
            ] {
                assert!(!why.contains(jargon), "{why}");
            }
            assert!(why.contains("try"), "{why}");
        }
        assert_eq!(
            not_made_line(UNSAID),
            "I couldn't make that: when it would run and what it would do didn't come out in \
             plain words; try saying when it should run, like 'when I come to my desk', and \
             what it should do, like 'glow green'."
        );
        assert_eq!(
            not_made_line("A skill can't hear words."),
            "I couldn't make that: A skill can't hear words."
        );
        let note = not_made_in_their_language(&not_made_line(NOT_PASSED));
        assert!(note.starts_with(ALREADY_DONE), "{note}");
        assert!(note.contains("call no tool"), "{note}");
        assert!(in_their_language("I made x, and it's on.").starts_with(ALREADY_DONE));
    }

    /// With reads on, a read refused as unsaid goes back with lines that
    /// keep the service's read step, so the repair doesn't take the read away
    /// (lamp-9adf, 5 Nov 13:26: an app make of a mail read, refused twice).
    #[test]
    fn with_reads_on_the_repair_lines_keep_the_read_step() {
        let mail = READ_TOML
            .replace("google_calendar = \"read\"", "gmail = \"read\"")
            .replace(
                "service = \"google_calendar\", what = \"events\", day = \"tomorrow\"",
                "service = \"gmail\", what = \"recent\", day = \"today\"",
            );
        let Err(Refused(why, Some(lines))) = plainly_with(&mail, true) else {
            panic!("a mail read with a day is unsaid");
        };
        assert_eq!(why, UNSAID);
        assert_eq!(
            lines,
            format!(
                "{UNSAID_PART}[[do]] step 1, read (day, service, what). \
                 {PLAIN_ONLY}{PLAIN_OR_READ}"
            )
        );
        assert!(
            lines.contains("that service's read step exactly as your instructions list it"),
            "{lines}"
        );
        assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
        let repair = repair_brief(&"x".repeat(MOST_REQUEST_BYTES), &lines).unwrap();
        assert!(repair.contains(&lines), "{repair}");
        assert!(repair.len() <= crate::agent_tasks::MOST_TASK_BYTES);
        let Err(Refused(_, Some(off))) = plainly_with(&mail, false) else {
            panic!("a read is unsaid with reads off");
        };
        assert!(off.ends_with(&format!(". {PLAIN_ONLY}")), "{off}");
    }

    /// A made skill never reaches a connected service ([`crate::services`]):
    /// a step that names one is never said plainly, and a function of its
    /// own is refused, so it can't read, still less write.
    #[test]
    fn a_made_skill_never_reaches_a_connected_service() {
        for step in [
            "service_write = { service = \"gmail\", action = \"send\" }",
            "service_read = { service = \"gmail\", what = \"recent\" }",
            "call = { name = \"service_write\" }",
        ] {
            let toml = TOML.replace("light = { dim = 1 }", step);
            assert!(plainly(&toml).is_err(), "{step}");
        }
        let with_tool = format!("{TOML}\n[tool]\nname = \"service_write\"\n");
        assert!(capped(&with_tool).is_err());
    }

    /// The model's `cannot:` is said as a skill that can't do that, and
    /// nothing is kept; a `cannot:` with blocks is read as blocks.
    #[test]
    fn a_cannot_answer_is_said_and_makes_nothing() {
        let Err(Refused(why, None)) =
            files("cannot: a skill can't hear words like \"good night\".")
        else {
            panic!("a cannot answer is refused with no repair");
        };
        assert_eq!(why, "a skill can't hear words like \"good night\".");
        let long = format!("cannot: {}", "x ".repeat(200));
        assert!(files(&long).unwrap_err().0.chars().count() <= MOST_CANNOT_REASON_CHARS + 1);
        let both = format!("cannot: no\n{}", answer(TOML, CARD));
        assert!(files(&both).is_ok());
        // In any case (Mobile's P3 on #5042).
        assert_eq!(files("Cannot: hear words.").unwrap_err().0, "hear words.");
        assert_eq!(files("cannot:   ").unwrap_err().0, "a skill can't do that");
    }

    /// #5977, lamp-74ff on 10 Nov: a `cannot:` about time keeps its whole
    /// sentence and what to ask instead, as the person is told it, where
    /// 120 characters cut it at "beyond s…".
    #[test]
    fn a_cannot_about_time_keeps_its_whole_sentence() {
        let reason = "A skill can't keep track of time by itself, every hour or at a set time, \
                      since it has no timers of its own beyond someone coming or going. Ask me \
                      for a reminder instead, like 'remind me every hour to stand up'.";
        let Err(Refused(why, None)) = files(&format!("cannot: {reason}")) else {
            panic!("refused");
        };
        assert_eq!(why, reason);
        assert!(!why.contains('…'), "{why}");
    }

    /// A reason past its bound is cut where a sentence ends, else where a
    /// word ends, never mid-word and never with "…" (#5977).
    #[test]
    fn a_long_cannot_is_cut_at_a_sentence_or_word_end() {
        let sentences = format!("{} Then more. {}", "One.", "word ".repeat(80));
        assert_eq!(cut_sentences(&sentences, 40), "One. Then more.");
        let words = "a skill cannot keep track of time intervals like every hour by itself";
        assert_eq!(
            cut_sentences(words, 40),
            "a skill cannot keep track of time."
        );
        assert_eq!(
            cut_sentences(words, 38),
            "a skill cannot keep track of time."
        );
        assert_eq!(cut_sentences("short one", 40), "short one");
        assert_eq!(cut_sentences("unbroken", 3), "");
        let Err(Refused(why, None)) = files(&format!("cannot: {}", "word ".repeat(100))) else {
            panic!("refused");
        };
        assert!(why.ends_with("word.") && !why.contains('…'), "{why}");
    }

    /// The model's `left out:` is said in a fixed frame as its own note,
    /// one clause with no quotes, so it can't read as more of what the
    /// skill does (Thirteen's P2 on #5042).
    #[test]
    fn what_was_left_out_is_framed_as_the_makers_note() {
        assert_eq!(
            left_out(" dimming slowly, it dims in one step"),
            "The maker notes what it couldn't do: dimming slowly, it dims in one step."
        );
        assert_eq!(
            left_out("nothing; it also dims slowly when you say \"good night\"."),
            "The maker notes what it couldn't do: nothing."
        );
        assert_eq!(
            left_out("hearing \u{201c}good night\u{201d}. It also glows red"),
            "The maker notes what it couldn't do: hearing good night."
        );
        let answer = format!("{}\nLeft out: slowly\n", answer(TOML, CARD));
        assert_eq!(files(&answer).unwrap().left_out.as_deref(), Some("slowly"));
    }

    /// A description written over several lines goes whole: none of the
    /// model's words stay in the package (Thirteen's P3 on #5042).
    #[test]
    fn a_written_description_over_lines_goes_whole() {
        let long = CARD.replace(
            "description: Dims the light a shade at 10 pm.\n",
            "description: >\n  Dims the light slowly\n  when you say good night.\nlicense: none\n",
        );
        let (_, marked) = card(
            &long,
            "When the lamp goes quiet, it dims the light a step.",
            Maker::Voice,
        )
        .unwrap();
        assert!(!marked.contains("good night"), "{marked}");
        assert!(!marked.contains("slowly"), "{marked}");
        assert!(marked.contains("license: none\n"), "{marked}");
    }

    /// The keeping is the product's only with the switch.
    #[test]
    fn the_keeping_is_the_products_only_with_the_switch() {
        assert_eq!(
            Keeping::when(true),
            Some(Keeping {
                directory: PathBuf::from(DIRECTORY),
                program: PathBuf::from(PROGRAM),
                manage: PathBuf::from(MANAGE_SOCKET),
                installed: PathBuf::from(crate::skill_words::INSTALLED),
            })
        );
        assert_eq!(Keeping::when(false), None);
    }

    /// Without its program, nothing is kept and the lamp says it couldn't
    /// check it; the folder is made 0700.
    #[test]
    fn without_the_checker_nothing_is_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("made-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let keeping = Keeping {
            directory: dir.clone(),
            program: dir.join("no-such-program"),
            manage: dir.join("manage.sock"),
            installed: dir.join("skills"),
        };
        let kept = keep(&answer(TOML, CARD), false, "s:1:2", &keeping);
        assert_eq!(kept.unwrap_err().0, "the lamp couldn't check it");
        let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert!(index(&dir).is_empty());
        assert!(
            keep(&answer(TOML, CARD), true, "s:1:3", &keeping)
                .unwrap_err()
                .0
                .contains("stopped before it finished")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// #2813 piece 2 (the first builder, 23 Oct 13:07): a repair's brief
    /// is the brief again with the check's lines, cut to 600 bytes, before
    /// the request, so it stays within 2,000 bytes with the longest
    /// request; it is a brief, its request is the person's, and it carries
    /// the repair's mark.
    #[test]
    fn a_repairs_brief_carries_the_checks_lines_and_still_fits() {
        let request = "x".repeat(MOST_REQUEST_BYTES);
        let lines = "y".repeat(5_000);
        let repair = repair_brief(&request, &lines).unwrap();
        assert!(
            repair.len() <= crate::agent_tasks::MOST_TASK_BYTES,
            "{}",
            repair.len()
        );
        assert!(repair.contains(&"y".repeat(MOST_REPAIR_BYTES)));
        assert!(!repair.contains(&"y".repeat(MOST_REPAIR_BYTES + 1)));
        assert!(is_brief(&repair) && repair.contains(REPAIR_MARK));
        assert_eq!(request_of(&repair), request);
        // Cut on a character, never inside one.
        let wide = repair_brief("dim it", &"é".repeat(400)).unwrap();
        assert!(wide.contains(&"é".repeat(MOST_REPAIR_BYTES / 2)));
        assert!(!wide.contains(&"é".repeat(MOST_REPAIR_BYTES / 2 + 1)));
        assert_eq!(repair_brief("", "x"), Err("Say what the skill should do."));
    }

    /// A made skill the check refuses comes back with the check's lines for
    /// a repair, its folder replaced, every line kept; a refusal that is no
    /// check's carries none.
    #[test]
    fn a_failed_check_gives_its_lines_with_the_folder_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("made-check-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let program = dir.join("pack");
        fs::write(
            &program,
            "#!/bin/sh\necho \"lamp-skills: not packed: $4/skill.toml: TOML parse error at line 3\" >&2\necho '  |' >&2\necho 'unknown field `name`' >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let keeping = Keeping {
            directory: dir.join("made"),
            program,
            manage: dir.join("manage.sock"),
            installed: dir.join("skills"),
        };
        let refused = keep(&answer(TOML, CARD), false, "s:1:2", &keeping).unwrap_err();
        assert_eq!(refused.0, NOT_PASSED);
        let lines = refused.1.expect("the check's lines");
        assert_eq!(
            lines,
            "lamp-skills: not packed: <folder>/skill.toml: TOML parse error at line 3 / | / unknown field `name`"
        );
        assert!(!lines.contains(".making-"), "{lines}");
        assert_eq!(
            check_lines(&"z".repeat(900), Path::new("/f")).len(),
            MOST_REPAIR_BYTES
        );
        let cut = keep(&answer(TOML, CARD), true, "s:1:3", &keeping).unwrap_err();
        assert!(cut.1.is_none(), "{cut:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The check's first line goes to the journal with the skill's words
    /// left out: quoted text, its name and its folder (Mobile's #2813 rerun:
    /// only `exit 1` was logged).
    #[test]
    fn the_checks_reason_is_logged_without_the_skills_words() {
        let folder = Path::new("/var/lib/made/.making-7/skill/evening-dim");
        let reason = |said: &str| check_reason(said, folder, "evening-dim");
        assert_eq!(
            reason(
                "lamp-skills: not packed: /var/lib/made/.making-7/skill/evening-dim/skill.toml: \
                 [[do]] light: unknown key \"glow\" (expected colour, level, for or dim)\n\
                 more lines\n"
            ),
            "not packed: <folder>/skill.toml: [[do]] light: unknown key \"…\" (expected colour, level, for or dim)"
        );
        // Its name, an apostrophe in a word, single quotes, backticks and
        // curly quotes.
        assert_eq!(
            reason("not packed: evening-dim's skill.toml says no version: a package carries one"),
            "not packed: <skill>'s skill.toml says no version: a package carries one"
        );
        assert_eq!(
            reason("gesture 'wave to the cat' isn't one of `nod`, “curious”"),
            "gesture '…' isn't one of `…`, “…”"
        );
        // An apostrophe inside a quote doesn't end it early.
        assert_eq!(reason("said 'don't wake me' at 7"), "said '…' at 7");
        // A TOML parse error gives its why after the quoted source, which is
        // left out.
        assert_eq!(
            reason(
                "lamp-skills: not packed: skill.toml: TOML parse error at line 1, column 1\n  |\n\
                 1 | name = \"evening-dim\"\n  | ^^^^\nunknown field `name`, expected one of `kind`\n"
            ),
            "not packed: skill.toml: TOML parse error at line 1, column 1: unknown field `…`, expected one of `…`"
        );
        // Nor does one escaped in a `{:?}` value.
        assert_eq!(
            reason(r#"unknown say "call \"home\" now" in [[do]]"#),
            r#"unknown say "…" in [[do]]"#
        );
        // A quote never closed hides the rest.
        assert_eq!(reason("bad value \"call home at"), "bad value \"…");
        assert_eq!(reason("\n  \n"), "it said nothing");
        let long = reason(&"word ".repeat(100));
        assert!(long.chars().count() <= MOST_REASON_CHARS + 1, "{long}");
        assert!(long.ends_with('…'));
    }

    /// A check that says a lot and fails is answered "doesn't pass" at
    /// once: its words go to a file, so a full pipe never holds it.
    #[test]
    fn a_check_that_says_a_lot_still_ends() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("made-loud");
        let program = dir.join("lamp-skills");
        fs::write(
            &program,
            "#!/bin/sh\nhead -c 200000 /dev/zero | tr '\\0' x >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let keeping = Keeping {
            directory: dir.join("made"),
            program,
            manage: dir.join("manage.sock"),
            installed: dir.join("skills"),
        };
        let began = Instant::now();
        let kept = keep(&answer(TOML, CARD), false, "s:1:4", &keeping);
        assert_eq!(kept.unwrap_err().0, NOT_PASSED);
        assert!(began.elapsed() < PACK_WITHIN, "{:?}", began.elapsed());
        assert!(index(&keeping.directory).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A made skill `name` whose package's bytes are `sha`'s, its package
    /// in `directory` as `keep` moves it in.
    fn made_as(directory: &Path, name: &str, sha: char) -> Made {
        let sha256 = sha.to_string().repeat(64);
        let package = directory.join(format!("{name}-1.0.0-{}.tar", &sha256[..12]));
        fs::write(&package, format!("tar of {name}")).unwrap();
        Made {
            name: name.into(),
            version: "1.0.0".into(),
            sha256,
            description: "d".into(),
            uses: Uses {
                light: true,
                body: false,
                topics: vec!["room/presence".into()],
                speak: false,
                reads: Vec::new(),
                ..Uses::default()
            },
            package,
        }
    }

    /// An index entry of `name`, its package `package` there, on or not.
    fn entry_of(directory: &Path, name: &str, sha: char, package: &str, on: bool) -> Value {
        fs::write(directory.join(package), format!("tar of {name}")).unwrap();
        let mut entry = json!({"name": name, "sha256": sha.to_string().repeat(64),
            "package": package, "from": format!("local:{sha}"), "at_unix_ms": 1});
        if on {
            entry["on"] = json!(true);
        }
        entry
    }

    fn names(directory: &Path) -> Vec<String> {
        index(directory)
            .iter()
            .map(|entry| entry["name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// The same bytes kept twice are one entry, the newest ask's, still
    /// found by its `from`; one on stays on. The same name not on, with
    /// other bytes, takes the old one's place, its package with it; the
    /// same name on stays beside the new one.
    #[test]
    fn a_skill_kept_twice_is_one_entry() {
        let dir = scratch("made-once");
        let made = made_as(&dir, "water", 'a');
        remember(&dir, "local:1", &made, false, "").unwrap();
        remember(&dir, "local:2", &made, false, "").unwrap();
        assert_eq!(names(&dir), ["water"]);
        assert_eq!(kept(&dir, "local:2").unwrap().sha256, made.sha256);
        assert!(kept(&dir, "local:1").is_none());
        assert!(made.package.is_file());
        let other = made_as(&dir, "water", 'b');
        remember(&dir, "local:3", &other, false, "").unwrap();
        assert_eq!(names(&dir), ["water"]);
        assert_eq!(index(&dir)[0]["sha256"], other.sha256);
        assert!(!made.package.exists() && other.package.is_file());
        // On, its bytes kept again stay on; another of its name sits beside.
        let mut entries = index(&dir);
        entries[0]["on"] = json!(true);
        write_index(&dir, &entries).unwrap();
        remember(&dir, "local:4", &other, false, "").unwrap();
        assert_eq!(index(&dir).len(), 1);
        assert_eq!(index(&dir)[0]["on"], true);
        let third = made_as(&dir, "water", 'c');
        remember(&dir, "local:5", &third, false, "").unwrap();
        assert_eq!(names(&dir), ["water", "water"]);
        assert!(other.package.is_file() && third.package.is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Past [`KEPT_MOST`], the oldest not on is let go, and a package
    /// another entry still names stays. With every other one on, all stay.
    #[test]
    fn letting_go_skips_skills_on_and_packages_still_named() {
        let dir = scratch("made-evict");
        write_index(
            &dir,
            &[
                entry_of(&dir, "on-one", '1', "on-one.tar", true),
                entry_of(&dir, "shared-a", '2', "shared.tar", false),
                entry_of(&dir, "three", '3', "three.tar", false),
                entry_of(&dir, "shared-b", '4', "shared.tar", false),
                entry_of(&dir, "five", '5', "five.tar", false),
            ],
        )
        .unwrap();
        remember(&dir, "local:6", &made_as(&dir, "six", '6'), false, "").unwrap();
        assert_eq!(names(&dir), ["on-one", "three", "shared-b", "five", "six"]);
        assert!(dir.join("on-one.tar").is_file());
        assert!(dir.join("shared.tar").is_file());
        remember(&dir, "local:7", &made_as(&dir, "seven", '7'), false, "").unwrap();
        assert!(!dir.join("three.tar").exists());
        // All on: kept, past the cap.
        let mut entries = index(&dir);
        for entry in &mut entries {
            entry["on"] = json!(true);
        }
        write_index(&dir, &entries).unwrap();
        let eighth = made_as(&dir, "eight", '8');
        remember(&dir, "local:8", &eighth, false, "").unwrap();
        assert_eq!(index(&dir).len(), KEPT_MOST + 1);
        assert!(eighth.package.is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    /// lamp-9adf, 3 Nov 16:15: with `glow-yellow-on-return` on, the same
    /// skill kept again and again, and others after it, never let its
    /// package go.
    #[test]
    fn a_skill_on_keeps_its_package_however_often_others_are_kept() {
        let dir = scratch("made-9adf");
        write_index(
            &dir,
            &[
                entry_of(&dir, "glow-yellow-on-return", 'y', "glow-yellow.tar", true),
                entry_of(&dir, "evening-dim", 'e', "evening-dim.tar", false),
                entry_of(&dir, "morning-glow", 'm', "morning-glow.tar", false),
                entry_of(&dir, "nod-hello", 'n', "nod-hello.tar", false),
            ],
        )
        .unwrap();
        let water = made_as(&dir, "water-on-return", 'w');
        for at in 0..4 {
            remember(&dir, &format!("local:{at}"), &water, false, "").unwrap();
        }
        for (at, sha) in ['p', 'q', 'r'].into_iter().enumerate() {
            remember(
                &dir,
                &format!("app:{at}"),
                &made_as(&dir, &format!("more-{at}"), sha),
                false,
                "",
            )
            .unwrap();
        }
        let kept = index(&dir);
        assert!(kept.len() <= KEPT_MOST, "{kept:?}");
        assert_eq!(kept[0]["name"], "glow-yellow-on-return");
        assert_eq!(kept[0]["on"], true);
        assert!(dir.join("glow-yellow.tar").is_file());
        assert_eq!(
            kept.iter()
                .filter(|entry| entry["name"] == "water-on-return")
                .count(),
            1,
            "{kept:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A fresh folder of the test's own, for its process.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An index of two made skills, as `keep` writes it.
    fn two_kept(directory: &Path) {
        fs::create_dir_all(directory).unwrap();
        write_index(
            directory,
            &[
                json!({"name": "evening-dim", "sha256": "a".repeat(64), "package": "evening-dim.tar", "from": "s1:7:1"}),
                json!({"name": "morning-glow", "sha256": "b".repeat(64), "package": "morning-glow.tar", "from": "s1:8:1"}),
            ],
        )
        .unwrap();
    }

    /// A helper that answers `answer` to one line, and gives back what it was
    /// asked.
    fn helper(socket: &Path, answer: &'static str) -> std::thread::JoinHandle<String> {
        use std::io::{BufRead, BufReader};
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            writeln!(&stream, "{answer}").unwrap();
            line
        })
    }

    /// A package left half moved in by a crash is deleted, but never one
    /// another keep is moving in right now: only one older than any keep
    /// takes (Fourteen's P2 on #3662: two readers keep the same answers).
    #[test]
    fn only_a_package_left_long_ago_is_dropped_as_unfinished() {
        let dir = std::env::temp_dir().join(format!("made-unfinished-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let fresh = dir.join(".evening-dim-1.0.0-abc.tar.new");
        let left = dir.join(".evening-dim-1.0.0-def.tar.new");
        fs::write(&fresh, b"x").unwrap();
        fs::write(&left, b"x").unwrap();
        fs::File::options()
            .write(true)
            .open(&left)
            .unwrap()
            .set_modified(SystemTime::now() - UNFINISHED_AFTER - Duration::from_secs(1))
            .unwrap();
        drop_unfinished(&dir);
        assert!(fresh.exists(), "another keep's package, moving in now");
        assert!(!left.exists(), "a crash's, long ago");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A kept skill goes on at once, with no yes (the owner, 10 Nov): the
    /// helper is asked for its SHA-256 alone, it is marked to go on and
    /// then on, and once on, asking again asks the helper nothing.
    #[test]
    fn a_kept_skill_goes_on_at_once_by_its_sha256_with_no_yes() {
        let dir = scratch("mk-on");
        let made = dir.join("made");
        two_kept(&made);
        let socket = dir.join("manage.sock");
        let asked = helper(&socket, r#"{"done": "installed evening-dim"}"#);
        let wait = Duration::from_secs(5);
        let on = put_on_within(&made, &socket, &"a".repeat(64), 2_000, wait);
        assert_eq!(on.as_deref(), Ok("evening-dim"));
        let asked: Value = serde_json::from_str(&asked.join().unwrap()).unwrap();
        assert_eq!(asked, json!({"install_made": "a".repeat(64)}));
        let entries = index(&made);
        assert_eq!(entries[0]["on"], true);
        assert_eq!(entries[0]["goes_on_at_unix_ms"], 2_000);
        assert!(
            entries[1].get("goes_on_at_unix_ms").is_none(),
            "{entries:?}"
        );
        assert_eq!(going_on(&made, &"a".repeat(64), 2_000), None, "on now");
        // On already: on, and no helper is asked (none listens now).
        let again = put_on_within(&made, &socket, &"a".repeat(64), 3_000, wait);
        assert_eq!(again.as_deref(), Ok("evening-dim"));
        let line = on_line(&Made {
            name: "evening-dim".into(),
            version: "1.0.0".into(),
            sha256: "a".repeat(64),
            description: String::new(),
            uses: Uses::default(),
            package: PathBuf::new(),
        });
        assert_eq!(line, "I made evening dim, and it's on.");
        // What it does and uses is said with it (App Only's P1 on #5985).
        let line = on_line(&Made {
            name: "evening-dim".into(),
            version: "1.0.0".into(),
            sha256: "a".repeat(64),
            description: "Dims the light at 10 pm.".into(),
            uses: Uses {
                light: true,
                ..Uses::default()
            },
            package: PathBuf::new(),
        });
        assert_eq!(
            line,
            "I made evening dim, and it's on: dims the light at 10 pm, using its light."
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The manage helper's own rule: a kept skill goes on only within
    /// [`GOES_ON_WITHIN`] of its mark, by the helper's clock, never one
    /// never marked, marked too long ago, marked after now, or on already.
    #[test]
    fn only_a_fresh_mark_lets_a_kept_skill_go_on() {
        let dir = scratch("mk-going");
        let made = dir.join("made");
        two_kept(&made);
        let (a, b) = ("a".repeat(64), "b".repeat(64));
        assert_eq!(going_on(&made, &a, 1_000), None, "never marked");
        assert!(goes_on(&made, &a, 1_000).unwrap());
        assert!(!goes_on(&made, &"c".repeat(64), 1_000).unwrap(), "not kept");
        let within = GOES_ON_WITHIN.as_millis() as u64;
        assert_eq!(going_on(&made, &a, 1_000).as_deref(), Some("evening-dim"));
        assert_eq!(
            going_on(&made, &a, 1_000 + within).as_deref(),
            Some("evening-dim")
        );
        assert_eq!(going_on(&made, &a, 1_001 + within), None, "too long ago");
        assert_eq!(going_on(&made, &a, 999), None, "a clock gone back");
        assert_eq!(going_on(&made, &b, 1_000), None, "another, never marked");
        let mut entries = index(&made);
        entries[0]["on"] = json!(true);
        write_index(&made, &entries).unwrap();
        assert_eq!(going_on(&made, &a, 1_000), None, "on already");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A kept skill that couldn't be put on stays off, and why is said in
    /// plain words and kept beside it (`not_on`): no helper, a helper
    /// silent, a refusal, and a skill of that name put on another way.
    #[test]
    fn a_kept_skill_not_put_on_stays_off_and_says_why() {
        let dir = scratch("mk-not-on");
        let made = dir.join("made");
        two_kept(&made);
        let a = "a".repeat(64);
        let wait = Duration::from_secs(5);
        let nobody = put_on_within(&made, &dir.join("nobody.sock"), &a, 2_000, wait);
        assert_eq!(
            nobody,
            Err("the lamp's skills couldn't be reached".to_owned())
        );
        assert_eq!(index(&made)[0]["not_on"], nobody.unwrap_err().as_str());
        assert_ne!(index(&made)[0]["on"], true);
        // A helper asked and silent may still put it on.
        let socket = dir.join("silent.sock");
        let _listening = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let silent = put_on_within(&made, &socket, &a, 2_000, Duration::from_millis(200));
        assert_eq!(
            silent,
            Err("the lamp didn't say in time whether it is on".to_owned())
        );
        let socket = dir.join("manage.sock");
        let asked = helper(
            &socket,
            r#"{"refused": "not installed: its SHA-256 has changed"}"#,
        );
        let refused = put_on_within(&made, &socket, &a, 2_000, wait);
        asked.join().unwrap();
        assert_eq!(refused, Err("the lamp's check didn't let it on".to_owned()));
        let socket = dir.join("in-the-way.sock");
        let asked = helper(
            &socket,
            r#"{"refused": "not installed: evening-dim is already on the lamp, put on another way, and a skill made here doesn't take its place"}"#,
        );
        let in_the_way = put_on_within(&made, &socket, &a, 2_000, wait);
        asked.join().unwrap();
        let why = in_the_way.unwrap_err();
        assert_eq!(why, "a skill called evening dim is already on the lamp");
        assert_eq!(
            not_on_line(&why),
            "I made it but couldn't put it on: a skill called evening dim is already on the lamp."
        );
        assert_eq!(index(&made)[0]["not_on"], why.as_str());
        assert_ne!(index(&made)[0]["on"], true);
        let gone = put_on_within(&made, &socket, &"c".repeat(64), 2_000, wait);
        assert_eq!(gone, Err("the lamp no longer has it".to_owned()));
        let _ = fs::remove_dir_all(&dir);
    }

    /// Now, as the index's times are kept.
    fn now_ms() -> u64 {
        crate::app_make::now_unix_ms()
    }

    const INSTRUCTIONS: &str = "Dim the light to level 2.\nSay \"focus time\" once, then stay quiet \
                                for 25 minutes.\n\tNo music.";

    fn study_mode() -> Written {
        Written::new("study-mode", "when I say study mode", INSTRUCTIONS).unwrap()
    }

    /// A skill written in the app (`LAMP_SKILL_WRITTEN`): its skill.toml
    /// is a function with no trigger, never started by itself, whose
    /// answer is the person's words as written; its card is marked made
    /// here and written in the app, the instructions its body; and it is
    /// said plainly as what it does.
    #[test]
    fn a_written_skill_is_a_function_with_the_text_as_written_and_no_trigger() {
        let written = study_mode();
        let text = written.toml();
        let table: toml::Table = toml::from_str(&text).unwrap();
        assert!(!table.contains_key("trigger") && !table.contains_key("ask"));
        assert_eq!(table["tool"].as_str(), Some("written_study_mode"));
        assert_eq!(table["kind"].as_str(), Some("installable"));
        assert_eq!(
            table["permissions"]
                .as_table()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["speak"]
        );
        let say = table["do"][0]["say"].as_str().unwrap();
        assert!(say.starts_with(WRITTEN_FRAME), "{say}");
        assert!(
            say.ends_with(&format!("\nTheir instructions:\n{INSTRUCTIONS}")),
            "{say}"
        );
        assert!(
            say.contains("When they use it: when I say study mode\n"),
            "{say}"
        );
        let (version, uses) = written_capped(&text).unwrap();
        assert_eq!(version, "1.0.0");
        assert_eq!(uses.spoken(), "its voice");
        assert_eq!(put_on_cap(&text).unwrap(), (version, uses));
        // The made skills' own cap never takes one: a model's answer can't
        // be kept as one.
        assert!(capped(&text).unwrap_err().0.contains("runs by itself"));
        let said = plainly(&text).unwrap();
        assert_eq!(
            said,
            "When you ask for it (when I say study mode), the lamp follows the instructions you \
             wrote: Dim the light to level 2. Say \"focus time\" once, then stay quiet for…"
        );
        assert!(said.chars().count() <= MOST_PLAIN_CHARS, "{said}");
        let card = written.card(&said);
        assert!(card.starts_with("---\nname: study-mode\ndescription: \"When you ask for it"));
        assert!(card.contains("Say 'focus time' once"), "{card}");
        assert!(card.contains("\nmade: here\nwritten: app\n---\n"), "{card}");
        assert!(card.ends_with(&format!("---\n{INSTRUCTIONS}\n")), "{card}");
    }

    /// Kept as a made skill is, with what it does said plainly; keeping
    /// alone marks nothing to go on and puts nothing into the skills
    /// folder: [`put_on`] does, next.
    #[test]
    fn a_written_skill_is_kept_and_nothing_goes_on_by_keeping_alone() {
        let (root, keeping) = crate::make_skill_local::tests::keeping("written-kept");
        let installed = root.join("skills");
        fs::create_dir_all(&installed).unwrap();
        let made = keep_written(&study_mode(), "written:app:1:1", &keeping, &installed).unwrap();
        assert_eq!(made.name, "study-mode");
        assert!(
            made.description
                .starts_with("When you ask for it (when I say study mode)")
        );
        assert_eq!(made.uses.spoken(), "its voice");
        assert_eq!(index(&keeping.directory)[0]["uses"]["speak"], true);
        assert_eq!(going_on(&keeping.directory, &made.sha256, now_ms()), None);
        assert_eq!(fs::read_dir(&installed).unwrap().count(), 0);
        // The same ask again: the kept one.
        let again = keep_written(&study_mode(), "written:app:1:1", &keeping, &installed).unwrap();
        assert_eq!(again, made);
        // Marked to go on: only then may the helper put it on.
        goes_on(&keeping.directory, &made.sha256, now_ms()).unwrap();
        assert_eq!(
            going_on(&keeping.directory, &made.sha256, now_ms()),
            Some("study-mode".to_owned())
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// A name any skill on the lamp has, installed or pushed, gets the first
    /// free one. A kept one of that name not on, marked to go on or
    /// not, is replaced by the new one, its package and its mark with it,
    /// so a fix sent after it goes at once (Seventh's P2 on #5072).
    #[test]
    fn a_written_skill_takes_no_name_a_skill_has() {
        let (root, keeping) = crate::make_skill_local::tests::keeping("written-taken");
        let installed = root.join("skills");
        for on_lamp in [
            installed.join("study-mode"),
            root.join("skills-dev/study-mode"),
        ] {
            fs::create_dir_all(&on_lamp).unwrap();
            let made =
                keep_written(&study_mode(), "written:app:1:1", &keeping, &installed).unwrap();
            assert_eq!(made.name, "study-mode-2");
            fs::remove_dir_all(&on_lamp).unwrap();
        }
        assert_eq!(index(&keeping.directory).len(), 1);
        write_index(&keeping.directory, &[]).unwrap();
        // Kept and not yet asked about: the next takes its place.
        keep_written(&study_mode(), "written:app:2:1", &keeping, &installed).unwrap();
        let first = keep_written(&study_mode(), "written:app:3:1", &keeping, &installed).unwrap();
        let entries = index(&keeping.directory);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["from"], "written:app:3:1");
        // Marked to go on and not on (the helper refused it): a fix goes at
        // once, and the old mark is gone with it.
        goes_on(&keeping.directory, &first.sha256, now_ms()).unwrap();
        assert!(going_on(&keeping.directory, &first.sha256, now_ms()).is_some());
        let fixed = keep_written(&study_mode(), "written:app:3:2", &keeping, &installed).unwrap();
        assert_eq!(index(&keeping.directory).len(), 1);
        assert_eq!(index(&keeping.directory)[0]["from"], "written:app:3:2");
        // Its mark ran out, not on: the new one takes its place too.
        let long_ago = now_ms() - GOES_ON_WITHIN.as_millis() as u64 - 1_000;
        goes_on(&keeping.directory, &fixed.sha256, long_ago).unwrap();
        let first = fixed;
        // An older one of that name, said no to, with a package of its own.
        let older = keeping.directory.join("study-mode-0.9.0-000000000000.tar");
        fs::write(&older, b"older").unwrap();
        let mut entries = index(&keeping.directory);
        entries.insert(
            0,
            json!({"from": "written:app:0:1", "name": "study-mode", "package": "study-mode-0.9.0-000000000000.tar", "sha256": "0", "goes_on_at_unix_ms": long_ago}),
        );
        write_index(&keeping.directory, &entries).unwrap();
        let rewritten =
            Written::new("study-mode", "when I say study mode", "Dim the light.").unwrap();
        let second = keep_written(&rewritten, "written:app:4:1", &keeping, &installed).unwrap();
        let entries = index(&keeping.directory);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["from"], "written:app:4:1");
        assert!(!older.exists());
        // The test's packer packs the same bytes whatever the words: one
        // package stands for both, and it stays while one names it.
        assert_eq!(second.package, first.package);
        assert!(second.package.exists());
        goes_on(&keeping.directory, &second.sha256, long_ago).unwrap();
        let third = keep_written(&rewritten, "written:app:5:1", &keeping, &installed).unwrap();
        assert_eq!(third.package, second.package);
        assert!(third.package.exists());
        assert_eq!(index(&keeping.directory).len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    /// Each field within its bound, as plain text: empty, too long, a name
    /// that is not a skill's, a `when` of two lines or a control character
    /// in the instructions is refused in plain words.
    #[test]
    fn a_written_skill_is_held_to_its_bounds() {
        let most = |bytes: usize| "a".repeat(bytes);
        assert!(Written::new("a", &most(MOST_WHEN_BYTES), &most(MOST_INSTRUCTIONS_BYTES)).is_ok());
        assert!(Written::new(&most(MOST_WRITTEN_NAME_BYTES), "x", "y").is_ok());
        for (name, when, instructions, why) in [
            ("", "x", "y", "name"),
            ("  ", "x", "y", "name"),
            ("Study", "x", "y", "lowercase"),
            ("1study", "x", "y", "lowercase"),
            ("study_mode", "x", "y", "lowercase"),
            (&most(MOST_WRITTEN_NAME_BYTES + 1), "x", "y", "lowercase"),
            ("study", "", "y", "when"),
            ("study", "one\ntwo", "y", "one line"),
            ("study", &most(MOST_WHEN_BYTES + 1), "y", "one line"),
            ("study", "x", " ", "what the lamp should do"),
            (
                "study",
                "x",
                &most(MOST_INSTRUCTIONS_BYTES + 1),
                "plain text",
            ),
            ("study", "x", "a\u{7}b", "plain text"),
        ] {
            let refused = Written::new(name, when, instructions).unwrap_err();
            assert!(refused.contains(why), "{name:?} {when:?}: {refused}");
        }
    }

    /// Anything more than the written shape is refused, as it is kept and
    /// as it is put on: a light, a trigger, a function not a written one's
    /// or words not framed as the person's.
    #[test]
    fn a_written_skill_may_use_nothing_but_its_answer() {
        let text = study_mode().toml();
        for wider in [
            text.replace("speak = true", "speak = true\nlight = true"),
            format!(
                "{text}\n[trigger]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n"
            ),
            text.replace("written_study_mode", "study_mode"),
            text.replace("The person who owns this lamp", "The lamp's maker"),
            format!("{text}\n[[do]]\nlight = {{ dim = 1 }}\n"),
            text.replace("kind = \"installable\"", "kind = \"core\""),
        ] {
            assert!(written_capped(&wider).is_err(), "{wider}");
            assert!(put_on_cap(&wider).is_err(), "{wider}");
        }
    }

    /// What a made skill needs linked: the service it reads, by code and
    /// name, unless it is linked now (either Gmail by the other); nothing
    /// for a skill that reads none, never asking; `None` when what is
    /// linked can't be told.
    #[test]
    fn a_made_skill_needs_linked_only_the_service_it_reads_not_linked() {
        let reads = |code: &str| Uses {
            speak: true,
            reads: vec![code.to_owned()],
            ..Default::default()
        };
        let linked = |codes: &[&str]| Some(codes.iter().map(|code| (*code).to_owned()).collect());
        assert_eq!(
            needs_link(&reads("gmail"), || linked(&[])),
            Some(vec![json!({"code": "gmail", "name": "Gmail"})])
        );
        assert_eq!(
            needs_link(&reads("gmail"), || linked(&["notion"])),
            Some(vec![json!({"code": "gmail", "name": "Gmail"})])
        );
        assert_eq!(
            needs_link(&reads("gmail"), || linked(&["gmail"])),
            Some(vec![])
        );
        assert_eq!(
            needs_link(&reads("gmail"), || linked(&["gmail-app"])),
            Some(vec![])
        );
        assert_eq!(
            needs_link(&reads("google_calendar"), || linked(&["gmail"])).unwrap()[0]["name"],
            "Google Calendar"
        );
        assert_eq!(needs_link(&reads("gmail"), || None), None);
        assert_eq!(
            needs_link(&Uses::default(), || panic!("never asked")),
            Some(vec![])
        );
        // The owner, 10 Nov ("đồng bộ luôn"): the model is asked to tell
        // it in its own words and the conversation's language, never given
        // an English sentence to say as it is.
        let ask = needs_link_ask(&[json!({"code": "gmail", "name": "Gmail"})]).unwrap();
        assert!(
            ask.contains("in your own words and in the language of the conversation")
                && ask.contains(
                    "the skill will start once Gmail is linked in the app, under \
                                 Skills, then Connectors"
                ),
            "{ask}"
        );
        assert!(!ask.contains("Say exactly") && !ask.contains("isn't linked yet"));
        // Thirteen's and App Only's P2 on #6032: every part is told, the
        // uses and reads too, the only read-back with no spoken yes.
        let line = on_line(&Made {
            name: "morning-mail".into(),
            version: "1.0.0".into(),
            sha256: "a".repeat(64),
            description: "Reads your latest mail at eight.".into(),
            uses: Uses {
                speak: true,
                reads: vec!["gmail".into()],
                ..Default::default()
            },
            package: "morning-mail.tar".into(),
        });
        assert!(line.contains("reading your Gmail"), "{line}");
        let told = in_their_language(&line);
        assert!(
            told.starts_with(&format!(
                "{ALREADY_DONE}Tell the person this, in the language of the conversation"
            )) && told.contains("every part of it, leaving nothing out")
                && told.contains("what the skill does, when, and what it uses or reads")
                && told.contains("Keep the skill's name and any service's name as written")
                && told.contains("add nothing else")
                && told.ends_with(&format!(": \"{line}\"")),
            "{told}"
        );
        assert_eq!(needs_link_ask(&[]), None);
    }

    /// A made skill that mails, as the making model writes it (#6066).
    pub(crate) const WRITE_TOML: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\
        [permissions]\ntopics = [\"room/stood_up\"]\nservices = { gmail = \"write\" }\n\
        [trigger]\ntopic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n\
        cooldown = \"1h\"\n[ask]\npolicy = \"nudge\"\n[[do]]\n\
        write = { service = \"gmail\", action = \"send_mail\", to = [\"mai@example.com\"], \
        subject = \"Leaving\", text = \"I'm leaving now.\" }\n";

    /// The request it is made from.
    pub(crate) const WRITE_REQUEST: &str = "make a skill that mails mai@example.com with the \
        subject Leaving, saying \"I'm leaving now.\" when I get up from my desk";

    /// Item 1 (#6066): one write step, its service granted as a write, for
    /// the kinds lamp-services sends, only with LAMP_SKILL_WRITES on; off,
    /// "I can't do that yet: sending or writing on its own", never repaired.
    #[test]
    fn a_write_is_carried_only_with_its_switch() {
        set_writes_here(Some(false));
        assert_eq!(
            capped_with(WRITE_TOML, false).unwrap_err(),
            Refused(cant_yet(SENDS_ON_ITS_OWN), None)
        );
        assert_eq!(
            not_made_line(&cant_yet(SENDS_ON_ITS_OWN)),
            "I can't do that yet: sending or writing on its own."
        );
        // A write step with no permission, the switch off: said the same.
        let bare = WRITE_TOML.replace("services = { gmail = \"write\" }\n", "");
        assert_eq!(
            capped_with(&bare, false).unwrap_err(),
            Refused(cant_yet(SENDS_ON_ITS_OWN), None)
        );
        set_writes_here(Some(true));
        let (_, uses) = capped_with(WRITE_TOML, false).unwrap();
        assert_eq!(uses.writes, ["gmail"]);
        assert_eq!(uses.sends_to, ["mai@example.com"]);
        assert!(!uses.speak);
        assert_eq!(uses.json()["sends_to"], json!(["mai@example.com"]));
        assert_eq!(Uses::of_json(&uses.json()), uses);
        assert!(
            uses.spoken()
                .contains("your Gmail, sending to mai@example.com"),
            "{}",
            uses.spoken()
        );
        assert_eq!(writes_in(WRITE_TOML)[0]["to"], json!(["mai@example.com"]));
        // A service no skill writes to, a public post: can't yet.
        let page = WRITE_TOML.replace("gmail = \"write\"", "facebook = \"write\"");
        assert!(
            capped_with(&page, false)
                .unwrap_err()
                .0
                .starts_with(CANT_YET)
        );
        // Its write ungranted, or not one the lamp sends: repaired.
        let ungranted = bare.clone();
        let refused = capped_with(&ungranted, false).unwrap_err();
        assert_eq!(refused.0, WRITE_NOT_FIXED);
        assert!(
            refused
                .1
                .unwrap()
                .contains("services = { gmail = \"write\" }")
        );
        let unknown = WRITE_TOML.replace("subject = \"Leaving\"", "title = \"Leaving\"");
        let refused = capped_with(&unknown, false).unwrap_err();
        assert_eq!(refused.0, WRITE_NOT_FIXED);
        assert!(refused.1.unwrap().contains(WRITE_ONLY));
        set_writes_here(None);
    }

    /// Item 4: the say-back says where it goes, every recipient, the whole
    /// text quoted, when and how often, and that it waits for a yes, ending
    /// on no word to repeat; the lamp's line for it says the same, never
    /// "it's on".
    #[test]
    fn a_writes_say_back_is_whole_and_ends_on_its_wait() {
        set_writes_here(Some(true));
        let said = plainly(&mannered(WRITE_TOML)).unwrap();
        for part in [
            "When someone sitting at the desk gets up",
            "it mails mai@example.com from your Gmail, subject “Leaving”, saying “I'm leaving now.”",
            "at most once an hour",
            "It starts on its own",
        ] {
            assert!(said.contains(part), "{part}: {said}");
        }
        assert!(said.ends_with(WAITS_FOR_YES), "{said}");
        assert!(!said.to_lowercase().contains("yes"), "{said}");
        let made = Made {
            name: "leaving-mail".into(),
            version: "1.0.0".into(),
            sha256: "0".repeat(64),
            description: said.clone(),
            uses: capped_with(WRITE_TOML, false).unwrap().1,
            package: PathBuf::from("leaving-mail.tar"),
        };
        let line = on_line(&made);
        assert!(
            line.starts_with("I made leaving mail: when someone"),
            "{line}"
        );
        assert!(!line.contains("it's on"), "{line}");
        assert!(line.ends_with(WAITS_FOR_YES.trim()), "{line}");
        // Off, the write is unsaid as what the lamp can't do yet.
        set_writes_here(Some(false));
        assert_eq!(
            plainly(WRITE_TOML).unwrap_err().0,
            cant_yet(SENDS_ON_ITS_OWN)
        );
        set_writes_here(None);
    }

    /// A GitHub write's say-back names its repository and who can see what
    /// it posts there before the yes, an issue and a comment alike, so the
    /// yes covers a public destination.
    #[test]
    fn a_github_writes_say_back_says_who_can_see_it_before_the_yes() {
        set_writes_here(Some(true));
        for (write, said_where) in [
            (
                "write = { service = \"github\", action = \"create_issue\", repo = \"mai/notes\", \
                 title = \"Left\", text = \"I left.\" }",
                "opens an issue “Left” on mai/notes on GitHub, saying “I left.”; ",
            ),
            (
                "write = { service = \"github\", action = \"comment\", repo = \"mai/notes\", \
                 number = 7, text = \"I left.\" }",
                "comments on issue 7 on mai/notes on GitHub, saying “I left.”; ",
            ),
        ] {
            let toml = WRITE_TOML
                .replace("gmail = \"write\"", "github = \"write\"")
                .replace(WRITE_TOML.lines().last().unwrap(), write);
            let said = plainly(&mannered(&toml)).unwrap();
            assert!(
                said.contains(&format!("{said_where}{SEEN_WITH_THE_REPO}")),
                "{said}"
            );
            assert!(said.ends_with(WAITS_FOR_YES), "{said}");
        }
        set_writes_here(None);
    }

    /// Items 2 and 3: every subject, text and title is in the request's own
    /// words (case, spacing and quotes aside); every recipient the linked
    /// account's own or an address the request says; every place named in
    /// it. Anything else goes back for repair, never naming a value.
    #[test]
    fn a_writes_words_and_recipients_are_the_requests_own() {
        let none: [(String, String); 0] = [];
        assert_eq!(write_from_request(WRITE_TOML, WRITE_REQUEST, &none), Ok(()));
        // Case, spacing, punctuation and a quoted span.
        assert_eq!(
            write_from_request(
                WRITE_TOML,
                "when I get up, MAIL mai@example.com:  subject 'leaving',  “i’m   LEAVING now”",
                &none
            ),
            Ok(())
        );
        // A spoken address.
        assert_eq!(
            write_from_request(
                WRITE_TOML,
                "mail mai at example dot com, subject leaving: I'm leaving now",
                &none
            ),
            Ok(())
        );
        let refused = |toml: &str, request: &str, own: &[(String, String)]| {
            let Refused(why, lines) = write_from_request(toml, request, own).unwrap_err();
            assert_eq!(why, WRITE_NOT_SAID);
            let lines = lines.unwrap();
            assert!(lines.contains(WRITE_FROM_REQUEST), "{lines}");
            assert!(!lines.contains("mai@example.com"), "{lines}");
            lines
        };
        // Words the model added, or took from elsewhere.
        let added = WRITE_TOML.replace("I'm leaving now.", "I'm leaving now, see you tomorrow.");
        assert!(refused(&added, WRITE_REQUEST, &none).contains("its text"));
        let subject = WRITE_TOML.replace("subject = \"Leaving\"", "subject = \"Update\"");
        assert!(refused(&subject, WRITE_REQUEST, &none).contains("its subject"));
        // A recipient the request didn't say: refused; the person's own
        // linked address: taken, and named to the repair.
        let boss = WRITE_TOML.replace("mai@example.com", "boss@work.test");
        let mine = [("gmail-app".to_owned(), "me@home.test".to_owned())];
        assert!(refused(&boss, WRITE_REQUEST, &mine).contains("me@home.test"));
        let me = WRITE_TOML.replace("mai@example.com", "me@home.test");
        assert_eq!(
            write_from_request(
                &me,
                "mail me saying I'm leaving now, subject leaving",
                &mine
            ),
            Ok(())
        );
        // A contact named without an address is no address.
        let named = "mail Mai saying I'm leaving now, subject leaving";
        refused(WRITE_TOML, named, &none);
        // A GitHub repository only as the request names it.
        let issue = "kind = \"installable\"\nversion = \"1.0.0\"\n[permissions]\n\
            services = { github = \"write\" }\n[trigger]\ntopic = \"clock/local\"\n\
            field = \"known\"\nequals = true\n[[do]]\nwrite = { service = \"github\", \
            action = \"create_issue\", repo = \"acme/site\", title = \"Standup\", \
            text = \"Standup notes\" }\n";
        assert_eq!(
            write_from_request(
                issue,
                "open an issue Standup in acme/site saying Standup notes",
                &none
            ),
            Ok(())
        );
        assert!(
            refused(issue, "open an issue Standup saying Standup notes", &none)
                .contains("its repo")
        );
    }

    /// lamp-9adf, 11 Nov 05:34: "every night at ten, dim the light a step"
    /// was written as a `relative` with `until`, which `plainly` said and
    /// `lamp-skills` refused three times with its own words. A dim or a
    /// `relative` light is now said only as `lamp_skills::light_call` takes
    /// it, and refused otherwise with a plain hint for the repair.
    #[test]
    fn a_dim_step_is_said_only_as_lamp_skills_takes_it() {
        let clock = "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
                     during = { from = \"22:00\", to = \"22:01\" }\n";
        for step in [
            "light = { relative = { dimmer = 1 }, until = \"ends\" }",
            "light = { relative = { dimmer = 1 }, for = \"10m\", until = \"ends\" }",
            // No for: a relative light is a glow, given back.
            "light = { relative = { dimmer = 1 } }",
            // Past the check's 10 minutes, or not a duration at all.
            "light = { relative = { dimmer = 1 }, for = \"30m\" }",
            "light = { relative = { dimmer = 1 }, for = \"all night\" }",
            // Past warmer's 0 or 1 and dimmer's 0 to 4, or both 0.
            "light = { relative = { warmer = 2 }, for = \"1m\" }",
            "light = { relative = { dimmer = 5 }, for = \"1m\" }",
            "light = { relative = { warmer = 0, dimmer = 0 }, for = \"1m\" }",
            "light = { relative = { dimmer = 1, warm_at_dimmest = true }, for = \"1m\" }",
            "light = { relative = { warmer = 1, warm_at_dimmest = false }, for = \"1m\" }",
            // Its colour and level are the person's.
            "light = { relative = { dimmer = 1 }, for = \"1m\", level = 2 }",
            // Only a breath with no low, of 4 to 12 seconds.
            "light = { relative = { dimmer = 1 }, for = \"1m\", pattern = { kind = \"candle\" } }",
            "light = { relative = { dimmer = 1 }, for = \"1m\", pattern = { kind = \"breathe\", \
             low = 1 } }",
            "light = { relative = { dimmer = 1 }, for = \"1m\", pattern = { kind = \"breathe\", \
             period = \"20s\" } }",
            // A dim is 1 to 4 steps, with only until beside it.
            "light = { dim = 5 }",
            "light = { dim = 1, for = \"10m\" }",
        ] {
            let text = made_with(clock, step);
            let Err(Refused(why, Some(lines))) = plainly_with(&text, false) else {
                panic!("{step}: said");
            };
            assert_eq!(why, UNSAID, "{step}");
            assert!(
                lines.starts_with(&format!("{UNSAID_PART}[[do]] step 1, light ("))
                    && lines.ends_with(&format!("{LIGHT_PART}. {PLAIN_ONLY}")),
                "{step}: {lines}"
            );
            assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
        }
        let Err(Refused(_, Some(lines))) = plainly_with(
            &made_with(
                clock,
                "light = { relative = { dimmer = 1 }, until = \"ends\" }",
            ),
            false,
        ) else {
            panic!("said");
        };
        assert_eq!(
            lines,
            format!(
                "{UNSAID_PART}[[do]] step 1, light (relative, until){LIGHT_PART}. {PLAIN_ONLY}"
            )
        );
        // A colour's refusal is not told of dims.
        let Err(Refused(_, Some(lines))) = plainly_with(
            &made_with(clock, "light = { colour = \"amber\", over = \"1m\" }"),
            false,
        ) else {
            panic!("said");
        };
        assert!(!lines.contains(LIGHT_PART), "{lines}");
        // As the check takes them: said.
        for (step, said) in [
            (
                "light = { relative = { dimmer = 1 }, for = \"10m\" }",
                "Every day at 22:00, it makes your light a step dimmer for 10 minutes.",
            ),
            (
                "light = { relative = { warmer = 1, warm_at_dimmest = true }, for = \"30s\", \
                 pattern = { kind = \"breathe\", period = \"6s\" } }",
                "Every day at 22:00, it makes your light a shade warmer for 30 seconds, \
                 breathing.",
            ),
            (
                "light = { dim = 4, until = \"ends\" }",
                "Every day at 22:00, it dims the light 4 steps while that lasts.",
            ),
        ] {
            let text = made_with(clock, step);
            assert!(capped_with(&text, false).is_ok(), "{text}");
            assert_eq!(
                plainly_with(&text, false).unwrap(),
                format!("{said} It starts on its own."),
                "{step}"
            );
        }
    }

    /// lamp-9adf, 11 Nov 05:36: a made skill on the clock written with no
    /// `clock/local` in its topics, which `lamp-skills` refuses: its
    /// trigger's topic is put in its topics, in whatever form the grant
    /// has, and a skill written otherwise is given back as it came.
    #[test]
    fn a_made_skills_trigger_topic_is_granted() {
        let clock = "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
                     during = { from = \"22:00\", to = \"22:01\" }\n";
        let topics = |text: &str| -> Vec<String> {
            let table: toml::Table = toml::from_str(text).unwrap();
            table["permissions"]["topics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|topic| topic.as_str().unwrap().to_owned())
                .collect()
        };
        let made = made_with(clock, "light = { dim = 1 }");
        // Granted already: as it came.
        assert_eq!(topic_granted(&made), made);
        // Left out of a one-line list: added last.
        let left_out = made.replace(", \"clock/local\"]", "]");
        assert_eq!(
            topics(&topic_granted(&left_out)),
            ["humans/faces/tracked", "room/stood_up", "clock/local"]
        );
        // No topics at all: a line of its own in [permissions].
        let none = made.replace(
            "topics = [\"humans/faces/tracked\", \"room/stood_up\", \"clock/local\"]\n",
            "",
        );
        assert_eq!(topics(&topic_granted(&none)), ["clock/local"]);
        // No [permissions]: one, before the trigger.
        let bare = format!(
            "kind = \"installable\"\nversion = \"1.0.0\"\n\n[trigger]\n{clock}\n[[do]]\nlight = \
             {{ dim = 1 }}\n"
        );
        let granted = topic_granted(&bare);
        assert_eq!(topics(&granted), ["clock/local"]);
        assert!(granted.contains("[permissions]\ntopics = [\"clock/local\"]\n\n[trigger]"));
        // Paced, and so kept and said, with it.
        assert_eq!(
            topics(&paced(&left_out)),
            ["humans/faces/tracked", "room/stood_up", "clock/local"]
        );
        assert!(
            capped_with(&paced(&left_out), false)
                .unwrap()
                .1
                .topics
                .contains(&"clock/local".to_owned())
        );
        // Grants written inline, or topics over several lines: as it came,
        // for the check to say why.
        let inline = made.replace(
            "[permissions]\nlight = true\nbody = true\ntopics = [\"humans/faces/tracked\", \
             \"room/stood_up\", \"clock/local\"]\n",
            "permissions = { light = true }\n",
        );
        assert!(
            inline.contains("permissions = { light = true }"),
            "{inline}"
        );
        assert_eq!(topic_granted(&inline), inline);
        let lines = left_out.replace(
            "topics = [\"humans/faces/tracked\", \"room/stood_up\"]",
            "topics = [\n  \"humans/faces/tracked\",\n  \"room/stood_up\",\n]",
        );
        assert_eq!(topic_granted(&lines), lines);
        // Not a skill, or no trigger: as it came.
        assert_eq!(topic_granted("not = toml = at all"), "not = toml = at all");
        assert_eq!(
            topic_granted(&TOML.replace("[trigger]", "[when]")),
            TOML.replace("[trigger]", "[when]")
        );
    }

    /// lamp-9adf, 11 Nov 17:0x: "when someone sits down, look at them and
    /// say hi" failed all three repairs, its look at a face written with no
    /// `humans/faces/tracked` in its topics, which `lamp-skills` refuses:
    /// that topic, the one its look reads, is put in its topics, beside its
    /// trigger's when that is left out too; no other grant is added, and a
    /// skill written otherwise is given back as it came.
    #[test]
    fn a_made_looks_faces_topic_is_granted() {
        let presence = "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\n";
        let look = "body = { look_at = \"face\" }";
        let topics = |text: &str| -> Vec<String> {
            let table: toml::Table = toml::from_str(text).unwrap();
            table["permissions"]["topics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|topic| topic.as_str().unwrap().to_owned())
                .collect()
        };
        let all = "topics = [\"humans/faces/tracked\", \"room/stood_up\", \"clock/local\"]";
        // Its trigger's topic granted, the faces left out: added last.
        let made = made_with(presence, look).replace(all, "topics = [\"room/presence\"]");
        let granted = topic_granted(&made);
        assert_eq!(topics(&granted), ["room/presence", "humans/faces/tracked"]);
        // Nothing else changed: the same skill with that one topic more.
        assert_eq!(
            granted.replace(", \"humans/faces/tracked\"]", "]"),
            made,
            "{granted}"
        );
        // Granted already: as it came.
        let given = made_with(presence, look).replace(
            all,
            "topics = [\"room/presence\", \"humans/faces/tracked\"]",
        );
        assert_eq!(topic_granted(&given), given);
        // Its trigger's topic left out too: both added, the trigger's first.
        let neither = made_with(presence, look).replace(&format!("{all}\n"), "");
        assert_eq!(
            topics(&topic_granted(&neither)),
            ["room/presence", "humans/faces/tracked"]
        );
        // A look in a later step, as a table of its own: found.
        let later = made_with(presence, "say = { text = \"Hi!\" }")
            .replace(all, "topics = [\"room/presence\"]")
            + "\n[[do]]\n[do.body]\nlook_at = \"face\"\n";
        assert_eq!(
            topics(&topic_granted(&later)),
            ["room/presence", "humans/faces/tracked"]
        );
        // No body grant is added: a look still needs the person to hear it.
        let bodiless = made.replace("body = true\n", "");
        let granted = topic_granted(&bodiless);
        assert!(!granted.contains("body = true"), "{granted}");
        assert_eq!(topics(&granted), ["room/presence", "humans/faces/tracked"]);
        // No look, or a look at something else: nothing added.
        let nod = made_with(presence, "body = { gesture = \"nod\" }")
            .replace(all, "topics = [\"room/presence\"]");
        assert_eq!(topic_granted(&nod), nod);
        let elsewhere = made.replace("look_at = \"face\"", "look_at = \"door\"");
        assert_eq!(topic_granted(&elsewhere), elsewhere);
        // Grants inline, or topics over several lines: as it came.
        let inline = made.replace(
            "[permissions]\nlight = true\nbody = true\ntopics = [\"room/presence\"]\n",
            "permissions = { body = true, topics = [\"room/presence\"] }\n",
        );
        assert!(inline.contains("permissions = {"), "{inline}");
        assert_eq!(topic_granted(&inline), inline);
        let lines = made.replace(
            "topics = [\"room/presence\"]",
            "topics = [\n  \"room/presence\",\n]",
        );
        assert_eq!(topic_granted(&lines), lines);
        // Paced with it, and so passed by the cap and said.
        let paced_made = paced(&made);
        assert_eq!(
            topics(&paced_made),
            ["room/presence", "humans/faces/tracked"]
        );
        assert!(
            capped_with(&paced_made, false)
                .unwrap()
                .1
                .topics
                .contains(&"humans/faces/tracked".to_owned())
        );
        assert!(
            plainly_with(&paced_made, false)
                .unwrap()
                .contains("turns to the face it sees"),
            "{paced_made}"
        );
    }

    /// lamp-74ff, 11 Nov 09:48-09:50: "nod when I stand up", made from the
    /// app, failed all three repairs twice on a nod written with a `for`:
    /// that `for` is taken out, in any form it is written as one line or
    /// one entry, and a gesture beside a mood, which takes it, is not.
    #[test]
    fn a_made_gestures_for_is_tidied_and_a_moods_kept() {
        let stood = "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n";
        let nod = made_with(stood, "body = { gesture = \"nod\" }");
        for step in [
            "body = { gesture = \"nod\", for = \"2s\" }",
            "body = { for = \"2s\", gesture = \"nod\" }",
            "body = {gesture=\"nod\",for=\"2s\"}",
        ] {
            let tidy = tidied(&made_with(stood, step));
            assert_eq!(
                toml::from_str::<toml::Table>(&tidy).unwrap(),
                toml::from_str::<toml::Table>(&nod).unwrap(),
                "{step}"
            );
            assert!(!tidy.contains("for ="), "{tidy}");
        }
        assert_eq!(
            tidied(&made_with(
                stood,
                "body = { gesture = \"nod\", for = \"2s\" }"
            )),
            nod
        );
        // As a table of its own: its line.
        assert_eq!(
            tidied(&made_with(
                stood,
                "[do.body]\ngesture = \"nod\"\nfor = \"2s\""
            )),
            made_with(stood, "[do.body]\ngesture = \"nod\"")
        );
        // A mood takes its for, with a gesture or not: untouched.
        for step in [
            "body = { gesture = \"nod\", mood = \"reading\", for = \"1m\" }",
            "body = { mood = \"reading\", for = \"1m\" }",
            "body = { gesture = \"nod\" }",
        ] {
            let text = made_with(stood, step);
            assert_eq!(tidied(&text), text, "{step}");
        }
        // Written dotted: as it came, for the check to say why.
        let dotted = made_with(stood, "body.gesture = \"nod\"\nbody.for = \"2s\"");
        assert_eq!(tidied(&dotted), dotted);
        assert_eq!(tidied("not = toml = at all"), "not = toml = at all");
        // Paced, so checked and said, it nods.
        let paced_nod = paced(&made_with(
            stood,
            "body = { gesture = \"nod\", for = \"2s\" }",
        ));
        assert!(capped_with(&paced_nod, false).is_ok(), "{paced_nod}");
        assert!(
            plainly_with(&paced_nod, false)
                .unwrap()
                .starts_with("When someone sitting at the desk gets up, it nods"),
            "{paced_nod}"
        );
    }

    /// The same make's trigger: `room/stood_up` with a `for` or an
    /// `absent_for`, which a getting up never lasts. Both are taken out;
    /// a face's `absent_for`, which it counts, is not.
    #[test]
    fn a_made_getting_ups_for_and_absent_for_are_tidied() {
        let stood = "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n";
        let step = "body = { gesture = \"nod\" }";
        let nod = made_with(stood, step);
        for held in [
            "for = \"10s\"\n",
            "absent_for = \"1m\"\n",
            "for = \"10s\"\nabsent_for = \"1m\"\n",
        ] {
            let text = made_with(&format!("{stood}{held}"), step);
            assert!(plainly_with(&text, false).is_err(), "{held}");
            assert_eq!(tidied(&text), nod, "{held}");
            assert!(plainly_with(&paced(&text), false).is_ok(), "{held}");
        }
        let face = made_with(
            "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\nabsent_for = \"5m\"\n",
            step,
        );
        assert_eq!(tidied(&face), face);
    }

    /// The same make's grant: `speak = true` with nothing said, which the
    /// cap refuses ("it asks for its voice but never says anything"), is
    /// taken out; beside a say, a hum or a read it is kept.
    #[test]
    fn a_made_skills_voice_asked_with_nothing_said_is_tidied() {
        let stood = "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n";
        let speaking = |step: &str| {
            made_with(stood, step).replace("body = true\n", "body = true\nspeak = true\n")
        };
        let nod = speaking("body = { gesture = \"nod\" }");
        assert!(capped_with(&nod, false).is_err());
        assert_eq!(
            tidied(&nod),
            made_with(stood, "body = { gesture = \"nod\" }")
        );
        assert!(capped_with(&paced(&nod), false).is_ok());
        for step in [
            "say = { text = \"Off you go.\" }",
            "cue = \"hum\"",
            "body = { gesture = \"nod\" }\n\n[[do]]\ncue = \"hum\"",
        ] {
            let text = speaking(step);
            assert_eq!(tidied(&text), text, "{step}");
        }
        let reads = nod.replace(
            "speak = true\n",
            "speak = true\nservices = { gmail = \"read\" }\n",
        );
        assert_eq!(tidied(&reads), reads);
        // All three at once, as lamp-74ff's model wrote them.
        let all = speaking("body = { gesture = \"nod\", for = \"2s\" }")
            .replace(stood, &format!("{stood}for = \"5s\"\n"));
        assert_eq!(
            tidied(&all),
            made_with(stood, "body = { gesture = \"nod\" }")
        );
    }

    /// lamp-9adf, 11 Nov 20:5x: "blink the light green twice when I
    /// arrive" was written as a pulse and an `out` step with a `for`, the
    /// blink's gap, which `lamp-skills` refuses; all three repairs failed.
    /// The gap is taken out whole and the pulse kept; an `out` alone, or
    /// with only `over` and `until`, is kept, and so is a gap with no lit
    /// step beside it, for the check to say why.
    #[test]
    fn a_made_blinks_gap_is_taken_out_and_a_plain_out_kept() {
        let face = "topic = \"humans/faces/tracked\"\n";
        let pulse = "light = { colour = \"green\", level = 3, for = \"2s\", pattern = { kind = \"pulse\" } }";
        let gap = "\n\n[[do]]\nlight = { out = true, for = \"500ms\" }";
        let blink = made_with(face, &format!("{pulse}{gap}"));
        let kept = made_with(face, pulse);
        assert!(blink_gapped(&blink) && !blink_gapped(&kept));
        assert_eq!(tidied(&blink), kept);
        // Twice over, as "twice" may be written: both gaps go.
        let twice = made_with(face, &format!("{pulse}{gap}\n\n[[do]]\n{pulse}{gap}"));
        assert_eq!(
            tidied(&twice),
            made_with(face, &format!("{pulse}\n\n[[do]]\n{pulse}"))
        );
        // The gap first, with a table of its own: taken out up to the pulse.
        let first = made_with(
            face,
            &format!("[do.light]\nout = true\nfor = \"1s\"\n\n[[do]]\n{pulse}"),
        );
        assert_eq!(tidied(&first), kept);
        // Paced, so checked and said, it pulses green.
        let paced_blink = paced(&blink);
        assert!(capped_with(&paced_blink, false).is_ok(), "{paced_blink}");
        assert!(!paced_blink.contains("out = true"), "{paced_blink}");
        // A plain out, with its fade or not, and a gap alone: untouched.
        for step in [
            format!("{pulse}\n\n[[do]]\nlight = {{ out = true }}"),
            format!(
                "{pulse}\n\n[[do]]\nlight = {{ out = true, over = \"30s\", until = \"ends\" }}"
            ),
            "light = { out = true, for = \"500ms\" }".to_owned(),
            "light = { out = true, for = \"500ms\" }\n\n[[do]]\ncue = \"hum\"".to_owned(),
        ] {
            let text = made_with(face, &step);
            assert!(!blink_gapped(&text), "{step}");
            assert_eq!(tidied(&text), text, "{step}");
        }
        // Written inline: as it came, for the check to say why.
        let inline = format!(
            "do = [{{ {pulse} }}, {{ light = {{ out = true, for = \"1s\" }} }}]\n{}",
            kept.replace(&format!("\n\n[[do]]\n{pulse}\n"), "\n")
        );
        assert!(blink_gapped(&inline), "{inline}");
        assert_eq!(tidied(&inline), inline);
    }

    /// A body step [`plainly`] can't say is refused with how to write one,
    /// as `lamp-skills` takes it: a gesture with no for, a mood's for more
    /// than 0 and at most 2m.
    #[test]
    fn a_body_step_is_said_only_as_lamp_skills_takes_it() {
        let stood = "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n";
        for (step, keys) in [
            ("body = { gesture = \"nod\", for = \"2s\" }", "for, gesture"),
            ("body = { mood = \"reading\", for = \"3m\" }", "for, mood"),
            ("body = { mood = \"reading\", for = \"0s\" }", "for, mood"),
            ("body = { mood = \"reading\" }", "mood"),
        ] {
            let Err(Refused(why, Some(lines))) = plainly_with(&made_with(stood, step), false)
            else {
                panic!("{step}: said");
            };
            assert_eq!(why, UNSAID, "{step}");
            assert_eq!(
                lines,
                format!("{UNSAID_PART}[[do]] step 1, body ({keys}){BODY_PART}. {PLAIN_ONLY}"),
                "{step}"
            );
            assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
        }
        assert!(
            plainly_with(
                &made_with(stood, "body = { mood = \"reading\", for = \"2m\" }"),
                false
            )
            .is_ok()
        );
    }

    /// lamp-9adf and lamp-74ff, 11 Nov 10:01: "say hello when I come back
    /// to my desk" written as one step with a gesture, a light of a colour
    /// the light hasn't and a fine say, the repair told only the say's form.
    /// Each kind is judged on its own: the colour is named with the
    /// light's list, the say isn't, and the step is told to be split.
    #[test]
    fn a_repair_is_told_the_part_of_a_step_that_failed() {
        let back = "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\n\
                    when = \"appears\"\nabsent_for = \"5m\"\n";
        let step = "body = { gesture = \"greeting\" }\nlight = { colour = \"gold\", level = 3, \
                    for = \"20s\" }\nsay = { text = \"Welcome back!\" }";
        let Err(Refused(why, Some(lines))) = plainly_with(&made_with(back, step), false) else {
            panic!("said");
        };
        assert_eq!(why, UNSAID);
        assert!(
            lines.contains(
                ": colour is one of yellow, red, green, blue, cyan, purple, orange, pink, white, \
                 warm; one kind a [[do]] step"
            ),
            "{lines}"
        );
        assert!(!lines.contains(SAY_TEXT_PART), "{lines}");
        assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
        assert!(hinted_first(&lines));
        // Every kind fine, but together: told only to split them.
        let together = step.replace("\"gold\"", "\"warm\"");
        let Err(Refused(_, Some(lines))) = plainly_with(&made_with(back, &together), false) else {
            panic!("said");
        };
        assert!(
            lines.contains(&format!("say (text){ONE_KIND_PART}. ")),
            "{lines}"
        );
        // One step a kind: said.
        let apart = together
            .replace("\nlight", "\n\n[[do]]\nlight")
            .replace("\nsay", "\n\n[[do]]\nsay");
        assert!(
            plainly_with(&made_with(back, &apart), false).is_ok(),
            "{apart}"
        );
        // A light alone of a colour it hasn't, or a level past 5: refused,
        // the colour named.
        for light in [
            "light = { colour = \"gold\" }",
            "light = { colour = \"warm\", level = 9 }",
        ] {
            let Err(Refused(_, Some(lines))) = plainly_with(&made_with(back, light), false) else {
                panic!("{light}: said");
            };
            assert!(lines.contains("[[do]] step 1, light ("), "{lines}");
        }
        let Err(Refused(_, Some(lines))) =
            plainly_with(&made_with(back, "light = { colour = \"gold\" }"), false)
        else {
            panic!("said");
        };
        assert!(
            lines.contains(&format!("light (colour){}. ", colour_part())),
            "{lines}"
        );
        // A bad say alone is still told the say's form.
        let Err(Refused(_, Some(lines))) = plainly_with(&made_with(back, "say = \"hello\""), false)
        else {
            panic!("said");
        };
        assert!(lines.contains(&format!("say{SAY_TEXT_PART}. ")), "{lines}");
    }

    /// The same make's trigger, on the desk's presence: a `for` of nothing
    /// is taken out, on any trigger, as `lamp-skills` reads it as none; a
    /// `for` that holds is kept; one in words is refused with its form.
    #[test]
    fn a_triggers_for_of_nothing_is_tidied_and_one_that_holds_kept() {
        let come = "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\n";
        let step = "body = { gesture = \"greeting\" }";
        let plain = made_with(come, step);
        for nothing in ["for = \"0s\"\n", "for = \"0m\"\n", "for = \"0ms\"\n"] {
            let text = made_with(&format!("{come}{nothing}"), step);
            assert!(plainly_with(&text, false).is_err(), "{nothing}");
            assert_eq!(tidied(&text), plain, "{nothing}");
        }
        let face = "topic = \"humans/faces/tracked\"\nwhen = \"appears\"\n";
        assert_eq!(
            tidied(&made_with(&format!("{face}absent_for = \"0s\"\n"), step)),
            made_with(face, step)
        );
        let held = made_with(&format!("{come}for = \"5s\"\n"), step);
        assert_eq!(tidied(&held), held);
        let words = made_with(&format!("{come}for = \"5 seconds\"\n"), step);
        assert_eq!(tidied(&words), words);
        let Err(Refused(_, Some(lines))) = plainly_with(&words, false) else {
            panic!("said");
        };
        assert!(
            lines.contains("room/presence, its for, like \"5m\": s, m or h, above 0"),
            "{lines}"
        );
        assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
    }

    /// lamp-0c89, 11 Nov 10:35: "nod when I stand up" written with a
    /// `when` lamp-skills doesn't know on its `room/stood_up` trigger. It
    /// is taken out, so the nod is said; a known one stays, and on a topic
    /// whose `when` means something the repair is told the ones it may be.
    #[test]
    fn a_getting_ups_unknown_when_is_tidied_and_another_topics_named() {
        let stood = "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n";
        let step = "body = { gesture = \"nod\" }";
        let nod = made_with(stood, step);
        for when in ["when = \"stands\"\n", "when = \"up\"\n", "when = 1\n"] {
            let text = made_with(&format!("{stood}{when}"), step);
            assert!(plainly_with(&text, false).is_err(), "{when}");
            assert_eq!(tidied(&text), nod, "{when}");
            assert!(plainly_with(&paced(&text), false).is_ok(), "{when}");
        }
        let known = made_with(&format!("{stood}when = \"appears\"\n"), step);
        assert_eq!(tidied(&known), known);
        let presence = made_with(
            "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\nwhen = \"arrives\"\n",
            step,
        );
        assert_eq!(tidied(&presence), presence);
        let Err(Refused(_, Some(lines))) = plainly_with(&presence, false) else {
            panic!("refused with lines");
        };
        assert!(
            lines.contains("its when, which is \"appears\", \"present\" or left out"),
            "{lines}"
        );
    }

    /// A folder of installed skills, `name` among them: the lamp's own
    /// (`made` false) or one made here, with `toml`.
    fn installed_skill(installed: &Path, name: &str, made: bool, toml: &str) {
        let folder = installed.join(name);
        fs::create_dir_all(&folder).unwrap();
        let mark = if made { "made: here\n" } else { "" };
        fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: d\n{mark}---\nbody\n"),
        )
        .unwrap();
        fs::write(folder.join("skill.toml"), toml).unwrap();
    }

    fn name_keeping(dir: &Path) -> Keeping {
        Keeping {
            directory: dir.join("made"),
            program: dir.join("no-such-program"),
            manage: dir.join("manage.sock"),
            installed: dir.join("skills"),
        }
    }

    /// The owner, 11 Nov: a factory reset keeps the lamp's own skills, so
    /// "say hello when I come back", named welcome-back, failed every time.
    /// A name the lamp's own skill has gets the first free one; a pushed
    /// one's is taken too; a long name is cut to fit with its number.
    #[test]
    fn a_built_in_skills_name_gets_the_first_free_one() {
        let dir = scratch("made-name-free");
        let keeping = name_keeping(&dir);
        let installed = &keeping.installed;
        installed_skill(installed, "welcome-back", false, "kind = \"core\"\n");
        let free = |name: &str, most| free_name(&keeping, installed, name, TOML, most);
        assert_eq!(free("evening-dim", 64).unwrap(), "evening-dim");
        assert_eq!(free("welcome-back", 64).unwrap(), "welcome-back-2");
        installed_skill(installed, "welcome-back-2", false, "kind = \"core\"\n");
        fs::create_dir_all(pushed(installed).join("welcome-back-3")).unwrap();
        assert_eq!(free("welcome-back", 64).unwrap(), "welcome-back-4");
        let long = format!("{}-ab", "a".repeat(60));
        installed_skill(installed, &long, false, "kind = \"core\"\n");
        let cut = free(&long, 64).unwrap();
        assert_eq!(cut, format!("{}-a-2", "a".repeat(60)));
        assert!(cut.len() <= 64);
        // A written skill's cap, its stem's trailing dash dropped.
        let written = format!("{}-b", "w".repeat(37));
        installed_skill(installed, &written, false, "kind = \"core\"\n");
        let cut = free(&written, MOST_WRITTEN_NAME_BYTES).unwrap();
        assert_eq!(cut, format!("{}-2", "w".repeat(37)));
        // The card says the new name.
        let card = "---\nname: welcome-back\ndescription: \"d\"\nmade: here\n---\nname: body\n";
        assert_eq!(
            renamed_card(card, "welcome-back-2"),
            "---\nname: welcome-back-2\ndescription: \"d\"\nmade: here\n---\nname: body\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The person's own made skill, on the lamp with the same skill.toml,
    /// is never made twice: "You already have that skill", said and shown
    /// as it is. Another made skill of that name, other words, gets `-2`;
    /// one kept and not on gives way, as before.
    #[test]
    fn the_persons_own_same_skill_is_already_had_and_another_gets_a_number() {
        let dir = scratch("made-name-own");
        let keeping = name_keeping(&dir);
        let installed = &keeping.installed;
        installed_skill(installed, "evening-dim", true, TOML);
        let free = |toml: &str| free_name(&keeping, installed, "evening-dim", toml, 64);
        let Err(Refused(why, None)) = free(TOML) else {
            panic!("already had");
        };
        assert_eq!(why, "You already have that skill: evening dim. It's on.");
        assert_eq!(plain_reason(&why), why);
        assert_eq!(not_made_line(&why), why);
        let other = TOML.replace("dim = 1", "dim = 2");
        assert_eq!(free(&other).unwrap(), "evening-dim-2");
        // The lamp's own of that name and the same words is no made one.
        installed_skill(installed, "light-out", false, TOML);
        assert_eq!(
            free_name(&keeping, installed, "light-out", TOML, 64).unwrap(),
            "light-out-2"
        );
        // Kept here and on, by its skill.toml's SHA-256 alone.
        fs::create_dir_all(&keeping.directory).unwrap();
        let sha = hex(&Sha256::digest(other.as_bytes()));
        let entries = vec![
            json!({"name": "bedtime", "on": true, "toml_sha256": sha}),
            json!({"name": "unused", "on": false, "toml_sha256": sha}),
        ];
        write_index(&keeping.directory, &entries).unwrap();
        let Err(Refused(why, None)) = free_name(&keeping, installed, "bedtime", &other, 64) else {
            panic!("already had");
        };
        assert!(why.starts_with(ALREADY_HAVE), "{why}");
        assert_eq!(
            free_name(&keeping, installed, "bedtime", TOML, 64).unwrap(),
            "bedtime-2"
        );
        assert_eq!(
            free_name(&keeping, installed, "unused", TOML, 64).unwrap(),
            "unused"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The owner, 11 Nov: makes failed "with reasons nobody understands".
    /// Every reason the lamp's own check gives is said in everyday words:
    /// none says the check's internals, and each mapped reason has its line.
    /// The model's own `cannot:` sentence is said as it is.
    #[test]
    fn a_failed_make_is_told_in_plain_words() {
        let internal = [
            UNSAID.to_owned(),
            NOT_PASSED.to_owned(),
            NOT_READABLE.to_owned(),
            NOT_A_SKILLS_FORM.to_owned(),
            SAY_NOT_TEXT.to_owned(),
            HUM_DROPPED.to_owned(),
            WRITE_NOT_FIXED.to_owned(),
            WRITTEN_UNSAID.to_owned(),
            EXAMPLE_GIVEN_BACK_WHY.to_owned(),
            "it would say something without asking for its voice (speak = true)".to_owned(),
            "Can't be said plainly: [[do]] step 1, body (gesture, for)".to_owned(),
            "trigger topic room/stood_up, its when".to_owned(),
            "not packed: lamp-skills refused skill.toml".to_owned(),
        ];
        let forbidden = [
            "[[",
            "toml",
            "plainly",
            "packed",
            "kept",
            "lamp-skills",
            "room/",
            "/",
            "=",
        ];
        for why in &internal {
            let line = not_made_line(&plain_reason(why));
            assert_eq!(line, NOT_MADE_TRY, "{why}");
        }
        for why in [NOT_KEPT_HERE, NOT_CHECKED, NOT_CHECKED_IN_TIME] {
            assert_eq!(not_made_line(&plain_reason(why)), NOT_KEPT_TRY, "{why}");
        }
        let cant = cant_yet("sending a message or writing to a linked service");
        assert_eq!(
            not_made_line(&plain_reason(&cant)),
            "I can't do that yet: sending a message or writing to a linked service."
        );
        assert_eq!(
            not_made_line(&plain_reason(MODEL_SILENT)),
            "I couldn't make that: the lamp's model didn't answer."
        );
        // The model's own words, never mapped where it said `cannot:`.
        assert_eq!(
            not_made_line("A skill can't hear words; ask for one that runs when you sit down."),
            "I couldn't make that: A skill can't hear words; ask for one that runs when you sit \
             down."
        );
        let mut lines: Vec<String> = internal
            .iter()
            .chain([NOT_KEPT_HERE.to_owned(), MODEL_CUT_OFF.to_owned()].iter())
            .map(|why| not_made_line(&plain_reason(why)))
            .collect();
        lines.push(not_made_line(&plain_reason(&cant)));
        lines.push(already_have_line("welcome-back-2"));
        for line in &lines {
            for word in forbidden {
                assert!(!line.contains(word), "{line:?} says {word:?}");
            }
        }
    }

    /// App Only's P2 on #6249: "nod when I've been up 10 minutes" written
    /// with a `for` on its getting up, which a getting up never lasts. It
    /// is taken out and the say-back says so; one of nothing is taken out
    /// without a word.
    #[test]
    fn a_getting_ups_wait_taken_out_is_said() {
        let stood = "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n";
        let step = "body = { gesture = \"nod\" }";
        let held = made_with(&format!("{stood}for = \"10m\"\n"), step);
        assert_eq!(dropped_wait(&held).as_deref(), Some("10 minutes"));
        assert_eq!(dropped_wait(&tidied(&held)), None);
        assert_eq!(
            cant_wait("10 minutes"),
            "It can't wait 10 minutes for that: it runs each time someone gets up."
        );
        let absent = made_with(&format!("{stood}absent_for = \"1m\"\n"), step);
        assert_eq!(dropped_wait(&absent).as_deref(), Some("a minute"));
        for nothing in ["for = \"0s\"\n", "absent_for = \"0m\"\n"] {
            let text = made_with(&format!("{stood}{nothing}"), step);
            assert_eq!(dropped_wait(&text), None, "{nothing}");
        }
        let face = made_with("topic = \"humans/faces/tracked\"\nfor = \"10m\"\n", step);
        assert_eq!(dropped_wait(&face), None);
    }

    /// A made skill with code of its own (#6307): a photo booth when
    /// someone comes to the desk.
    const CODE_TOML: &str = r#"kind = "installable"
version = "1.0.0"
grants = ["camera", "light"]

[permissions]
topics = ["room/presence"]

[trigger]
topic = "room/presence"
field = "someone"
equals = true
for = "5s"
cooldown = "1h"
rearm = "ends"

[ask]
policy = "nudge"

[[do]]
task = { task = "take a photo and send it to the app", run = "scripts/main.py" }
"#;

    const CODE_CARD: &str = "---\nname: photo-booth\ndescription: Takes a photo when I sit down.\n---\n\n# Photo booth\n";

    const MAIN_PY: &str = "from lamp import Lamp\n\nlamp = Lamp.local()\nlamp.camera.capture()\n";

    /// An answer with its two files and `scripts`, each `(path, text)`.
    fn code_answer(toml: &str, scripts: &[(&str, &str)]) -> String {
        let mut answer = answer(toml, CODE_CARD);
        for (path, text) in scripts {
            answer.push_str(&format!("\n```{path}\n{text}```\n"));
        }
        answer
    }

    /// Every plain string const outside the tests (the lines said, kept and
    /// shown) has single spaces: a run of spaces, as `CODE_BY_VOICE` once
    /// had, is said and shown as written (the Reviewer-lead's P2b on #6355).
    #[test]
    fn no_said_line_has_a_run_of_spaces() {
        let source = include_str!("made_skill.rs");
        let body = source.split("\n#[cfg(test)]\nmod tests {").next().unwrap();
        let mut checked = 0;
        for (at, _) in body.match_indices("const ") {
            let rest = &body[at..];
            let Some(eq) = rest.find(": &str =") else {
                continue;
            };
            let value = rest[eq + ": &str =".len()..].trim_start();
            if !value.starts_with('"') || rest[..eq].contains('\n') {
                continue;
            }
            let end = value[1..].find("\";").expect("a closed literal") + 1;
            // A `\` line end drops the next line's leading spaces.
            let text = value[1..end]
                .split("\\\n")
                .map(str::trim_start)
                .collect::<String>();
            assert!(!text.contains("  "), "{}", &rest[..eq]);
            checked += 1;
        }
        assert!(checked > 50, "{checked}");
    }

    /// The manage helper holds a made package's code again as it puts it on
    /// (the Reviewer-lead's P3 on #6355): within its count, sizes and
    /// endings, with its switch, and its task step's file among it.
    #[test]
    fn a_packages_code_is_held_again_as_it_goes_on() {
        set_scripts_here(Some(true));
        let (_, uses) = put_on_cap(CODE_TOML).unwrap();
        let held = |files: &[(&str, &str)]| {
            let two = [("skill.toml", CODE_TOML), ("SKILL.md", CODE_CARD)];
            let all = two
                .iter()
                .chain(files)
                .map(|(path, text)| (*path, text.as_bytes()));
            put_on_scripts(CODE_TOML, &uses, all).map_err(|refused| refused.0)
        };
        let main = ("scripts/main.py", MAIN_PY);
        assert_eq!(held(&[main]), Ok(()));
        assert_eq!(held(&[main, ("scripts/notes.md", "hi\n")]), Ok(()));
        let big = "x".repeat(MOST_SCRIPT_BYTES + 1);
        let half = "x".repeat(MOST_SCRIPTS_BYTES / 2);
        let cases: [&[(&str, &str)]; 9] = [
            &[],
            &[("scripts/other.py", MAIN_PY)],
            &[main, ("scripts/run.sh", "ls\n")],
            &[main, ("scripts/.hidden.py", "x\n")],
            &[main, ("elsewhere.py", "x\n")],
            &[main, ("scripts/a.txt", "\u{1b}[2J")],
            &[main, ("scripts/big.txt", &big)],
            &[main, ("scripts/a.txt", &half), ("scripts/b.txt", &half)],
            &[
                main,
                ("scripts/a.txt", "a"),
                ("scripts/b.txt", "b"),
                ("scripts/c.txt", "c"),
                ("scripts/d.txt", "d"),
            ],
        ];
        for files in cases {
            assert_eq!(held(files), Err(SCRIPTS_NOT_HELD.to_owned()), "{files:?}");
        }
        // Not text at all.
        let two = [
            ("skill.toml", CODE_TOML.as_bytes()),
            ("scripts/main.py", &[0xff, 0xfe][..]),
        ];
        assert!(put_on_scripts(CODE_TOML, &uses, two).is_err());
        set_scripts_here(Some(false));
        assert_eq!(held(&[main]), Err(cant_yet(CODE_OF_ITS_OWN)));
        set_scripts_here(None);
    }

    /// Off (#6307): a `scripts/` file in the answer, a `task` step or a
    /// `grants` list is what the lamp can't do yet, never repaired; the
    /// brief never offers code.
    #[test]
    fn code_is_refused_and_never_offered_with_its_switch_off() {
        set_scripts_here(Some(false));
        let cant = Refused(cant_yet(CODE_OF_ITS_OWN), None);
        assert_eq!(
            files(&code_answer(TOML, &[("scripts/main.py", MAIN_PY)])).unwrap_err(),
            cant
        );
        assert_eq!(capped_with(CODE_TOML, false).unwrap_err(), cant);
        let grants_only = TOML.replace(
            "version = \"1.0.0\"\n",
            "version = \"1.0.0\"\ngrants = []\n",
        );
        assert_eq!(capped_with(&grants_only, false).unwrap_err(), cant);
        assert_eq!(plainly(CODE_TOML).unwrap_err(), cant);
        assert_eq!(
            not_made_line(&cant.0),
            "I can't do that yet: running code of its own."
        );
        let brief = brief("make me a photo booth").unwrap();
        assert!(!brief.contains("code"), "{brief}");
        // Its two files alone are taken as before.
        assert!(files(&answer(TOML, CARD)).unwrap().scripts.is_empty());
        set_scripts_here(None);
    }

    /// On (#6307): its code is taken within its bounds, and refused past
    /// them with lines for a repair: at most four files, each a plain-text
    /// file under scripts/ of a kind it may be, 8 KiB each and 16 KiB in
    /// all; run by one task step alone, naming a .py file it wrote; its
    /// grants only the lamp's, never `skills`, and no write beside it.
    #[test]
    fn code_is_taken_within_its_bounds_and_refused_past_them() {
        set_scripts_here(Some(true));
        let taken = files(&code_answer(
            CODE_TOML,
            &[
                ("scripts/main.py", MAIN_PY),
                ("scripts/data/frame.json", "{}\n"),
            ],
        ))
        .unwrap();
        assert_eq!(
            taken.scripts,
            [
                ("scripts/main.py".to_owned(), MAIN_PY.to_owned()),
                ("scripts/data/frame.json".to_owned(), "{}\n".to_owned())
            ]
        );
        let refused_for_form = |scripts: &[(&str, &str)]| {
            let refused = files(&code_answer(CODE_TOML, scripts)).unwrap_err();
            assert_eq!(refused.0, SCRIPTS_NOT_HELD, "{scripts:?}");
            assert!(refused.1.unwrap().contains(SCRIPTS_ONLY), "{scripts:?}");
        };
        for path in [
            "scripts/../main.py",
            "scripts/.main.py",
            "scripts/a/b/main.py",
            "scripts/main.sh",
            "scripts/main",
            "scripts/",
        ] {
            refused_for_form(&[(path, MAIN_PY)]);
        }
        refused_for_form(&[("scripts/main.py", "print('\u{7}')\n")]);
        refused_for_form(&[("scripts/main.py", "x = 1 # \u{202E}evil\n")]);
        let five: Vec<String> = (0..5).map(|n| format!("scripts/f{n}.py")).collect();
        let five: Vec<(&str, &str)> = five.iter().map(|path| (path.as_str(), "x = 1\n")).collect();
        refused_for_form(&five);
        let big = "x = 1\n".repeat(1_200);
        refused_for_form(&[
            ("scripts/a.py", &big),
            ("scripts/b.py", &big),
            ("scripts/c.py", &big),
        ]);
        assert!(
            files(&code_answer(
                CODE_TOML,
                &[("scripts/main.py", MAIN_PY), ("scripts/main.py", MAIN_PY)]
            ))
            .is_err()
        );
        // The cap: what its code may use, said by name.
        let (_, uses) = capped_with(CODE_TOML, false).unwrap();
        assert!(uses.code);
        assert_eq!(uses.grants, ["camera", "light"]);
        assert_eq!(Uses::of_json(&uses.json()), uses);
        assert!(
            uses.spoken()
                .contains("code of its own that may use the camera and its light"),
            "{}",
            uses.spoken()
        );
        let refused = |toml: &str| capped_with(toml, false).unwrap_err();
        let unknown = refused(&CODE_TOML.replace("\"light\"]", "\"root\"]"));
        assert_eq!(unknown.0, SCRIPTS_NOT_HELD);
        assert!(unknown.1.unwrap().contains("Grants are only body, light"));
        assert_eq!(
            refused(&CODE_TOML.replace("\"light\"]", "\"skills\"]")),
            Refused(cant_yet("code that puts other skills on the lamp"), None)
        );
        let twice = format!(
            "{CODE_TOML}\n[[do]]\ntask = {{ task = \"again\", run = \"scripts/main.py\" }}\n"
        );
        assert_eq!(refused(&twice).0, SCRIPTS_NOT_HELD);
        for (from, to) in [
            ("run = \"scripts/main.py\"", "run = \"scripts/main.json\""),
            ("run = \"scripts/main.py\"", "run = \"/usr/bin/python3\""),
            (
                "run = \"scripts/main.py\"",
                "run = \"scripts/main.py\", env = \"x\"",
            ),
            (
                "task = \"take a photo and send it to the app\"",
                "task = \"\"",
            ),
            ("grants = [\"camera\", \"light\"]", "grants = \"camera\""),
        ] {
            assert_eq!(
                refused(&CODE_TOML.replace(from, to)).0,
                SCRIPTS_NOT_HELD,
                "{to}"
            );
        }
        set_writes_here(Some(true));
        let sends = CODE_TOML.replace(
            "topics = [\"room/presence\"]",
            "topics = [\"room/presence\"]\nservices = { gmail = \"write\" }",
        );
        assert!(refused(&sends).0.starts_with(CANT_YET));
        set_writes_here(None);
        // No grants at all is none: its code may use nothing of the lamp's.
        let none = CODE_TOML.replace("grants = [\"camera\", \"light\"]\n", "");
        assert!(capped_with(&none, false).unwrap().1.grants.is_empty());
        set_scripts_here(None);
    }

    /// A `scripts/` fence begins a block of its own, even inside an open
    /// `SKILL.md`, its path kept as written: so with the switch off it is
    /// refused, never kept as the card's words; on, it is a file of code.
    #[test]
    fn a_scripts_fence_is_its_own_block_as_written() {
        let open_card = format!(
            "```skill.toml\n{CODE_TOML}```\n```SKILL.md\n{CODE_CARD}\
             ```scripts/Main.py\n{MAIN_PY}```\n"
        );
        set_scripts_here(Some(false));
        assert_eq!(
            files(&open_card).unwrap_err(),
            Refused(cant_yet(CODE_OF_ITS_OWN), None)
        );
        set_scripts_here(Some(true));
        let taken = files(&open_card).unwrap();
        assert_eq!(taken.card, CODE_CARD);
        assert_eq!(
            taken.scripts,
            vec![("scripts/Main.py".to_owned(), MAIN_PY.to_owned())]
        );
        set_scripts_here(None);
    }

    /// A made skill's code's grants are a `do.task`'s (#6305), name for
    /// name in order, and never the one a script never holds.
    #[test]
    fn code_grants_are_a_tasks_grants() {
        let names = |grants: &[(&str, &str)]| -> Vec<String> {
            grants.iter().map(|(name, _)| (*name).to_owned()).collect()
        };
        assert_eq!(names(&GRANTS), names(&crate::skill_task_yes::GRANTS));
        assert_eq!(SKILLS_GRANT.0, crate::skill_task_yes::NEVER);
    }

    /// On (#6307): the say-back says that it runs its own code and what
    /// for, that the lamp's model wrote it, each grant in plain words, and
    /// ends on its wait, with no "yes" in it; the lamp's line for it says the
    /// same whole, never "it's on".
    #[test]
    fn codes_say_back_names_its_grants_and_its_code_and_waits() {
        set_scripts_here(Some(true));
        let said = plainly(&mannered(CODE_TOML)).unwrap();
        for part in [
            "runs its own code to “take a photo and send it to the app”",
            "The lamp's model wrote its code, and that code may use the camera and its light.",
        ] {
            assert!(said.contains(part), "{part}: {said}");
        }
        assert!(said.ends_with(RUNS_AFTER_YES), "{said}");
        assert!(!said.to_lowercase().contains("yes"), "{said}");
        let none = CODE_TOML.replace("grants = [\"camera\", \"light\"]\n", "");
        assert!(
            plainly(&mannered(&none))
                .unwrap()
                .contains("that code may use nothing of the lamp's.")
        );
        let made = Made {
            name: "photo-booth".into(),
            version: "1.0.0".into(),
            sha256: "0".repeat(64),
            description: said.clone(),
            uses: capped_with(CODE_TOML, false).unwrap().1,
            package: PathBuf::from("photo-booth.tar"),
        };
        let line = on_line(&made);
        assert!(
            line.starts_with("I made photo booth: when someone"),
            "{line}"
        );
        assert!(!line.contains("it's on"), "{line}");
        assert!(line.ends_with(RUNS_AFTER_YES.trim()), "{line}");
        assert!(line.contains(code_said(&made.uses.grants).trim()), "{line}");
        assert_eq!(
            cut_code_line(&made),
            "I made photo booth. It waits for you to agree before it runs any code. I couldn't \
             tell you just now what its code may use, so it isn't on."
        );
        // The brief offers it, still within its bound.
        let longest = brief(&"x".repeat(MOST_REQUEST_BYTES)).unwrap();
        assert!(longest.contains("Or code, as above."), "{longest}");
        assert!(longest.len() <= MOST_BRIEF_BYTES, "{}", longest.len());
        set_scripts_here(None);
    }

    /// On (#6307): its code is packed with its two files, so the package's
    /// SHA-256 covers every byte of it, a changed script another package;
    /// it is never put on at once (the app's MAKE IT, `put_on`), only on the
    /// person's yes to that very SHA-256 (`put_on_agreed`), which a skill
    /// without code never goes on by. Code with no task step to run it, or
    /// a task step running a file not written, is refused for a repair.
    #[test]
    fn code_is_kept_in_its_package_and_goes_on_only_by_its_agreed_sha256() {
        use std::os::unix::fs::PermissionsExt;
        set_scripts_here(Some(true));
        let dir = scratch("made-code");
        let program = dir.join("pack");
        fs::write(
            &program,
            "#!/bin/sh\nname=$(basename \"$4\")\ncat \"$4/skill.toml\" \"$4\"/scripts/*.py > \"$3/$name-1.0.0.tar\" 2>/dev/null\nexit 0\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let keeping = Keeping {
            directory: dir.join("made"),
            program,
            manage: dir.join("manage.sock"),
            installed: dir.join("skills"),
        };
        let made = keep(
            &code_answer(CODE_TOML, &[("scripts/main.py", MAIN_PY)]),
            false,
            "local:1",
            &keeping,
        )
        .unwrap();
        assert!(made.uses.code, "{made:?}");
        let bytes = fs::read_to_string(&made.package).unwrap();
        assert!(bytes.contains(MAIN_PY), "{bytes}");
        assert_eq!(made.sha256, hex(&Sha256::digest(bytes.as_bytes())));
        assert!(
            made.description.ends_with(RUNS_AFTER_YES),
            "{}",
            made.description
        );
        // Never at once: no helper is even asked, and why is kept.
        assert_eq!(put_on(&keeping, &made), Err(CODE_BY_VOICE.to_owned()));
        assert_eq!(index(&keeping.directory)[0]["not_on"], CODE_BY_VOICE);
        assert_ne!(index(&keeping.directory)[0]["on"], true);
        // The yes puts on that SHA-256 alone.
        let asked = helper(&keeping.manage, r#"{"done": "installed"}"#);
        assert_eq!(
            put_on_agreed(&keeping, &made.sha256),
            Ok("photo-booth".to_owned())
        );
        let asked: Value = serde_json::from_str(&asked.join().unwrap()).unwrap();
        assert_eq!(asked["install_made"], made.sha256.as_str());
        // A changed script is another package, not on, asked again.
        let changed = keep(
            &code_answer(CODE_TOML, &[("scripts/main.py", "print('other')\n")]),
            false,
            "local:2",
            &keeping,
        )
        .unwrap();
        assert_ne!(changed.sha256, made.sha256);
        // Other words than the one on, its code with them: another skill,
        // never "you already have that skill".
        assert_eq!(changed.name, "photo-booth-2");
        let entry = |sha256: &str| {
            index(&keeping.directory)
                .into_iter()
                .find(|entry| entry["sha256"] == sha256)
                .unwrap()
        };
        assert_eq!(entry(&made.sha256)["on"], true);
        assert_ne!(entry(&changed.sha256)["on"], true);
        // A skill with no code never goes on by this path.
        let plain = keep(&answer(TOML, CARD), false, "local:3", &keeping).unwrap();
        assert!(put_on_agreed(&keeping, &plain.sha256).is_err());
        // Code with nothing to run it, or a run naming no file written.
        let no_task = keep(
            &code_answer(TOML, &[("scripts/main.py", MAIN_PY)]),
            false,
            "local:4",
            &keeping,
        )
        .unwrap_err();
        assert_eq!(no_task.0, SCRIPTS_NOT_HELD);
        let elsewhere = keep(
            &code_answer(CODE_TOML, &[("scripts/other.py", MAIN_PY)]),
            false,
            "local:5",
            &keeping,
        )
        .unwrap_err();
        assert_eq!(elsewhere.0, SCRIPTS_NOT_HELD);
        let unwritten =
            keep(&answer(CODE_TOML, CODE_CARD), false, "local:6", &keeping).unwrap_err();
        assert_eq!(unwritten.0, SCRIPTS_NOT_HELD);
        set_scripts_here(None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The script lamp-9adf's model wrote (#6749 E, 13 Nov): `for` is a
    /// Python keyword, so the line is a SyntaxError, and it was kept and
    /// said back.
    const FOR_PY: &str = "from lamp import light\n\
        color = \"red\"\n\
        light.set_light(colour=color, level=3, for=\"59s\")\n";

    /// #6749 E: a written `.py` that doesn't parse is refused before it is
    /// said back, with lines for a repair that name the file, the line and
    /// the lamp's real calls; one that parses is kept as before. Nothing of
    /// it is run: the script goes to Python's own parser on stdin alone.
    #[test]
    fn a_made_script_that_doesnt_parse_is_refused_for_a_repair() {
        use std::os::unix::fs::PermissionsExt;
        if a_python_here().is_none() {
            eprintln!("skipped: no python3 here to parse a made script with");
            return;
        }
        set_scripts_here(Some(true));
        let dir = scratch("made-code-parse");
        let program = dir.join("pack");
        fs::write(
            &program,
            "#!/bin/sh\nname=$(basename \"$4\")\ncat \"$4/skill.toml\" > \"$3/$name-1.0.0.tar\"\nexit 0\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let keeping = Keeping {
            directory: dir.join("made"),
            program,
            manage: dir.join("manage.sock"),
            installed: dir.join("skills"),
        };
        let refused = keep(
            &code_answer(CODE_TOML, &[("scripts/main.py", FOR_PY)]),
            false,
            "local:1",
            &keeping,
        )
        .unwrap_err();
        assert_eq!(refused.0, SCRIPT_DOESNT_PARSE);
        let lines = refused.1.expect("lines for a repair");
        for part in [
            "scripts/main.py",
            "line 3",
            "SyntaxError",
            "for=\"59s\"",
            "from lampos import Lamp",
            "await lamp.light(colour, level, for_ms)",
        ] {
            assert!(lines.contains(part), "{part}: {lines}");
        }
        assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
        // Nothing kept, so nothing said back.
        assert!(index(&keeping.directory).is_empty());
        // Said plainly, once its repairs are used up.
        assert_eq!(plain_reason(&refused.0), NOT_MADE_TRY);
        // Any .py of its code, not only the one its task runs.
        let other = keep(
            &code_answer(
                CODE_TOML,
                &[
                    ("scripts/main.py", MAIN_PY),
                    ("scripts/util.py", "def f(:\n"),
                ],
            ),
            false,
            "local:2",
            &keeping,
        )
        .unwrap_err();
        assert_eq!(other.0, SCRIPT_DOESNT_PARSE);
        assert!(other.1.unwrap().contains("scripts/util.py"));
        // A file that isn't Python is never parsed as Python.
        let made = keep(
            &code_answer(
                CODE_TOML,
                &[
                    ("scripts/main.py", MAIN_PY),
                    ("scripts/notes.txt", "def f(:\n"),
                ],
            ),
            false,
            "local:3",
            &keeping,
        )
        .unwrap();
        assert!(made.uses.code, "{made:?}");
        set_python_here(None);
        set_scripts_here(None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// #6749 E: the parse is Python's alone, on stdin, within its time; a
    /// Python that can't be started or doesn't end in time leaves the
    /// script unchecked (journaled), never refused: the person's yes still
    /// stands before it runs.
    #[test]
    fn a_scripts_parse_is_bounded_and_leaves_it_unchecked_when_python_cant_tell() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("made-code-parse-bounds");
        let missing = dir.join("no-python");
        assert!(matches!(
            parsed(&missing, "x = 1\n", PARSE_WITHIN),
            Parse::Unchecked(_)
        ));
        let slow = dir.join("slow-python");
        fs::write(&slow, "#!/bin/sh\nsleep 5\n").unwrap();
        fs::set_permissions(&slow, fs::Permissions::from_mode(0o755)).unwrap();
        let began = Instant::now();
        assert!(matches!(
            parsed(&slow, "x = 1\n", Duration::from_millis(200)),
            Parse::Unchecked(_)
        ));
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
        // A Python that fails some other way than a parse error is no
        // refusal either.
        let broken = dir.join("broken-python");
        fs::write(
            &broken,
            "#!/bin/sh\necho 'Fatal Python error: init' >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&broken, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            parsed(&broken, "x = 1\n", PARSE_WITHIN),
            Parse::Unchecked(_)
        ));
        if let Some(python) = a_python_here() {
            assert!(matches!(
                parsed(&python, MAIN_PY, PARSE_WITHIN),
                Parse::Parses
            ));
            match parsed(&python, FOR_PY, PARSE_WITHIN) {
                Parse::Not { line, error } => {
                    assert_eq!(line, Some(3));
                    assert!(error.starts_with("SyntaxError"), "{error}");
                }
                other => panic!("{other:?}"),
            }
            // Never run: a script that would write a file parses, and
            // writes nothing.
            let mark = dir.join("ran");
            let writes = format!("open({:?}, 'w').write('ran')\n", mark.display().to_string());
            assert!(matches!(
                parsed(&python, &writes, PARSE_WITHIN),
                Parse::Parses
            ));
            assert!(!mark.exists());
        }
        set_python_here(None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The loosened switch for this test's thread, put back as it ends.
    struct LooseHere;

    impl LooseHere {
        fn set(on: bool) -> Self {
            set_loose_here(Some(on));
            Self
        }
    }

    impl Drop for LooseHere {
        fn drop(&mut self) {
            set_loose_here(None);
        }
    }

    /// #6059 (2): loosened, a made skill gets no limits the request didn't
    /// ask for, and `[quiet]` only when it speaks; asked for, or off, the
    /// lamp's manners are set as before.
    #[test]
    fn loosened_a_made_skill_gets_no_limits_it_didnt_ask_for() {
        let glow = made_with(
            "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\ncooldown = \"1m\"\n",
            "light = { colour = \"green\", level = 3, for = \"5s\" }",
        );
        let manners = ["most_a_day", "[budget]", "[rest]", "[quiet]"];
        let has = |text: &str, part: &str| text.contains(part);
        {
            let _off = LooseHere::set(false);
            let set = mannered_with(&glow, &[], false);
            assert!(manners.iter().all(|part| has(&set, part)), "{set}");
        }
        let _on = LooseHere::set(true);
        let bare = mannered_with(&glow, &[], false);
        // Its rest stays: a skill ignored twice rests (the first builder's
        // yes, 22:44).
        assert!(has(&bare, "[rest]\nafter_ignored = 2"), "{bare}");
        assert!(
            !["most_a_day", "[budget]", "[quiet]"]
                .iter()
                .any(|part| has(&bare, part)),
            "{bare}"
        );
        // The model's own limits, not asked for, are taken out too.
        let own = mannered_with(
            &glow.replace("version", "most_a_day = 5\nversion"),
            &[],
            false,
        );
        assert!(!has(&own, "most_a_day"), "{own}");
        // Asked for in the request's words: set as before.
        assert!(limits_asked(
            "glow green at most twice a day when I stand up"
        ));
        assert!(!limits_asked("glow green when I stand up"));
        let asked = mannered_with(&glow, &[], true);
        assert!(manners.iter().all(|part| has(&asked, part)), "{asked}");
        // A skill that speaks keeps the night quiet, and its rest.
        let hello = mannered_with(HELLO, &[], false);
        assert!(has(&hello, "[quiet]") && has(&hello, "[rest]"), "{hello}");
        assert!(
            !["most_a_day", "[budget]"]
                .iter()
                .any(|part| has(&hello, part))
        );
        // Said back and kept as it is, never refused for its manners.
        let said = said_mannered(&bare, &[]).unwrap();
        assert!(
            said.ends_with(" It starts on its own, rests till morning if ignored twice."),
            "{said}"
        );
        assert!(
            said_mannered(&hello, &[])
                .unwrap()
                .contains("never from 22:00 to 08:00")
        );
    }

    /// The Reviewer-lead's P2 on #6600: loosened, a make whose request asks
    /// for limits ("not too often") is written with the lamp's default ones
    /// and `limits: asked` on its card, so `lamp-skills` keeps them as it
    /// loads; one that asks for none has neither.
    #[test]
    fn loosened_a_make_with_limits_asked_is_marked_limits_asked() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("made-limits");
        let program = dir.join("pack");
        fs::write(
            &program,
            "#!/bin/sh\nname=$(basename \"$4\")\ncat \"$4/SKILL.md\" \"$4/skill.toml\" > \"$3/$name-1.0.0.tar\"\nexit 0\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let keeping = Keeping {
            directory: dir.join("made"),
            program,
            manage: dir.join("manage.sock"),
            installed: dir.join("skills"),
        };
        let glow = made_with(
            "topic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\ncooldown = \"1m\"\n",
            "light = { colour = \"green\", level = 3, for = \"5s\" }",
        );
        let _on = LooseHere::set(true);
        let made = |request: &str, from: &str| {
            let asked = Asked {
                request,
                ..Asked::default()
            };
            let made = keep_asked(&answer(&glow, CARD), false, from, &keeping, &asked).unwrap();
            fs::read_to_string(&made.package).unwrap()
        };
        let asked = made("glow green when I stand up, but not too often", "local:1");
        assert!(
            asked.contains(&format!("\nmade_by: voice\n{LIMITS_ASKED}\n---")),
            "{asked}"
        );
        assert!(asked.contains(&format!("most_a_day = {MANNERS_MOST_A_DAY}")));
        assert!(asked.contains(&format!("per_hour = {MANNERS_PER_HOUR}")));
        let unasked = made("glow green when I stand up", "local:2");
        assert!(!unasked.contains("limits:"), "{unasked}");
        assert!(!unasked.contains("most_a_day"), "{unasked}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// #6059 (3): loosened, a made skill's say may run as often as asked,
    /// at least a minute apart; off, at least an hour.
    #[test]
    fn loosened_a_says_cooldown_is_as_asked_from_a_minute() {
        let five = HELLO.replace("cooldown = \"1h\"", "cooldown = \"5m\"");
        let ten_seconds = HELLO.replace("cooldown = \"1h\"", "cooldown = \"10s\"");
        let cooldown = |text: &str| {
            toml::from_str::<toml::Table>(text).unwrap()["trigger"]["cooldown"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        {
            let _off = LooseHere::set(false);
            assert_eq!(cooldown(&paced(&five)), "1h");
            assert!(capped(&five).unwrap_err().0.contains("cooldown = \"1h\""));
        }
        let _on = LooseHere::set(true);
        assert_eq!(cooldown(&paced(&five)), "5m");
        assert_eq!(cooldown(&paced(&ten_seconds)), "1m");
        assert!(capped(&five).is_ok());
        assert!(
            capped(&ten_seconds)
                .unwrap_err()
                .0
                .contains("cooldown = \"1m\"")
        );
        // A skill added from a file keeps its own cap.
        assert!(capped_added(&five).is_err());
    }

    /// A made skill's `skill.toml` as the lamp's own model wrote the three
    /// makes on lamp-9adf, 12 Nov 15:13-15:16 (loosened): on someone at the
    /// desk, with `trigger`'s own lines, `permissions` and `steps`, and the
    /// `[rest]` (and `[quiet]`) it carried.
    fn desk_made(permissions: &str, trigger: &str, steps: &str, manners: &str) -> String {
        format!(
            "kind = \"installable\"\nversion = \"1.0.0\"\n\n[permissions]\n{permissions}\
             topics = [\"room/presence\"]\n\n[trigger]\ntopic = \"room/presence\"\n\
             field = \"someone\"\nequals = true\n{trigger}\n[ask]\npolicy = \"nudge\"\n\n\
             {steps}{manners}[rest]\nafter_ignored = 2\n"
        )
    }

    /// [`paced_for`] as [`keep_asked`] calls it for `request`, as a table.
    fn paced_request(text: &str, request: &str) -> (String, toml::Table) {
        let paced = paced_for(
            text,
            &asked_windows(request),
            limits_asked(request),
            repeat_asked(request),
        );
        let table = toml::from_str::<toml::Table>(&paced).unwrap();
        (paced, table)
    }

    /// A repeat in the request's own words, in seconds; none for "every
    /// time" or "every day".
    #[test]
    fn a_repeat_is_read_from_the_requests_words() {
        let asked = [
            (
                "every five minutes while I am at my desk, say drink some water",
                Some(300),
            ),
            ("glow every 10 min while I sit here", Some(600)),
            ("every minute while I'm here, nod", Some(60)),
            ("every hour while I work, say stretch", Some(3_600)),
            ("every two hours, say drink", Some(7_200)),
            ("every half an hour, glow blue", Some(1_800)),
            ("every half hour, glow blue", Some(1_800)),
            ("every 30 seconds, nod", Some(30)),
            ("mỗi 10 phút khi tôi ngồi đây, nhắc uống nước", Some(600)),
            (
                "when I come back to my desk, blink the light green twice",
                None,
            ),
            ("every time I sit down, nod", None),
            ("every day at 22:00, dim the light", None),
            ("when I sit down, nod at me and then do a backflip", None),
            // An ordinal, a second alone, a few: no repeat.
            ("nod every second time I sit down", None),
            ("glow every other time I come in", None),
            ("every third time I sit down, nod", None),
            ("blink every second", None),
            ("every second while I'm here, blink", None),
            ("every few minutes, nod", None),
            ("every 2 minutes", Some(120)),
            ("every 2 seconds while I'm here, blink", Some(2)),
            ("every hour, say stretch", Some(3_600)),
            (
                "once an hour while I'm at my desk, say stretch",
                Some(3_600),
            ),
            ("when I come back, blink, once an hour", None),
        ];
        for (request, secs) in asked {
            assert_eq!(repeat_asked(request), secs, "{request}");
        }
    }

    /// Case 1 of the lamp-9adf makes (12 Nov 15:13): "when I come back to
    /// my desk, blink the light green twice". Loosened, a skill that
    /// neither speaks nor hums has a minute at least: a missing or shorter
    /// cooldown is raised to it, and one the model wrote longer is kept,
    /// never lowered ("the first time I come in each morning", `"12h"`);
    /// its `rearm` and `[rest]` kept, no `[quiet]`. Off, as before.
    #[test]
    fn loosened_a_silent_made_skill_keeps_the_cooldown_it_wrote() {
        let request = "when I come back to my desk, blink the light green twice";
        let blink = desk_made(
            "light = true\n",
            "cooldown = \"1h\"\nrearm = \"ends\"\n",
            "[[do]]\nlight = { colour = \"green\", level = 3, for = \"5s\", pattern = { kind = \
             \"pulse\" } }\n\n",
            "",
        );
        {
            let _off = LooseHere::set(false);
            let (paced, table) = paced_request(&blink, request);
            assert_eq!(table["trigger"]["cooldown"].as_str(), Some("1h"), "{paced}");
            assert_eq!(table["trigger"]["rearm"].as_str(), Some("ends"));
            assert!(
                ["most_a_day", "budget", "rest", "quiet"]
                    .iter()
                    .all(|part| table.contains_key(*part))
            );
            assert_eq!(paced, paced_with(&blink, &[], false));
        }
        let _on = LooseHere::set(true);
        let (paced, table) = paced_request(&blink, request);
        assert_eq!(table["trigger"]["cooldown"].as_str(), Some("1h"), "{paced}");
        assert_eq!(table["trigger"]["rearm"].as_str(), Some("ends"));
        assert_eq!(table["rest"]["after_ignored"].as_integer(), Some(2));
        assert!(
            !["most_a_day", "budget", "quiet"]
                .iter()
                .any(|part| table.contains_key(*part)),
            "{paced}"
        );
        assert!(capped(&paced).is_ok(), "{paced}");
        // One with none, or a shorter one, has a minute.
        for cooldown in ["", "cooldown = \"20s\"\n"] {
            let text = blink.replace("cooldown = \"1h\"\n", cooldown);
            let (paced, table) = paced_request(&text, request);
            assert_eq!(table["trigger"]["cooldown"].as_str(), Some("1m"), "{paced}");
            let said = said_mannered(&paced, &[]).unwrap();
            assert!(said.contains("at most once a minute"), "{said}");
        }
        // "The first time I come in each morning, glow green": its 12 hours
        // are the person's, never lowered.
        let morning = blink.replace("cooldown = \"1h\"", "cooldown = \"12h\"");
        let (paced, table) = paced_request(
            &morning,
            "the first time I come in each morning, glow green",
        );
        assert_eq!(
            table["trigger"]["cooldown"].as_str(),
            Some("12h"),
            "{paced}"
        );
        assert_eq!(table["trigger"]["rearm"].as_str(), Some("ends"));
    }

    /// The Reviewer-lead's 16:22 read: loosened, a held state with no
    /// `rearm` and no repeat asked ("glow green when I'm at my desk",
    /// `room/presence` someone true) would run on each presence message,
    /// or each minute with the floor: it gains `rearm = "ends"`, once each
    /// time it holds. An asked repeat keeps none. Off, byte-identical.
    #[test]
    fn loosened_a_held_state_with_no_rearm_runs_once_each_time_it_holds() {
        let request = "glow green when I'm at my desk";
        let glow = desk_made(
            "light = true\n",
            "",
            "[[do]]\nlight = { colour = \"green\", level = 3 }\n\n",
            "",
        );
        {
            let _off = LooseHere::set(false);
            let (paced, table) = paced_request(&glow, request);
            assert!(!table["trigger"].as_table().unwrap().contains_key("rearm"));
            assert_eq!(paced, paced_with(&glow, &[], false));
        }
        let _on = LooseHere::set(true);
        let (paced, table) = paced_request(&glow, request);
        assert_eq!(table["trigger"]["rearm"].as_str(), Some("ends"), "{paced}");
        assert_eq!(table["trigger"]["cooldown"].as_str(), Some("1m"), "{paced}");
        assert!(capped(&paced).is_ok(), "{paced}");
        let (paced, table) = paced_request(&glow, "every 2 minutes while I'm at my desk, glow");
        let trigger = table["trigger"].as_table().unwrap();
        assert!(!trigger.contains_key("rearm"), "{paced}");
        assert_eq!(trigger["cooldown"].as_str(), Some("2m"), "{paced}");
    }

    /// The Reviewer-lead's P3: loosened, a touch on the lamp (`controls/…`,
    /// a pat on the head too) answers each deliberate touch: no minute's
    /// floor is added to a silent one.
    #[test]
    fn loosened_a_silent_touch_skill_has_no_minute_floor() {
        let pet = made_with(
            "topic = \"controls/pet\"\nfield = \"ended\"\nequals = false\nrearm = \"ends\"\n",
            "light = { colour = \"green\", level = 3 }",
        );
        let written = toml::from_str::<toml::Table>(&pet).unwrap()["trigger"]
            .get("cooldown")
            .cloned();
        let _on = LooseHere::set(true);
        let (paced, table) = paced_request(&pet, "when I pat your head, glow green");
        let trigger = table["trigger"].as_table().unwrap();
        assert_eq!(trigger.get("cooldown").cloned(), written, "{paced}");
        assert_eq!(trigger["rearm"].as_str(), Some("ends"));
    }

    /// Case 2 (12 Nov 15:14): "every five minutes while I am at my desk,
    /// say drink some water", written with `rearm = "ends"`, which says it
    /// once a stay. Loosened, the repeat asked runs while the trigger holds:
    /// no `rearm`, its cooldown the asked five minutes; its `[quiet]` (it
    /// speaks) and `[rest]` kept. Off, as before.
    #[test]
    fn loosened_an_asked_repeat_runs_while_its_trigger_holds() {
        let request = "every five minutes while I am at my desk, say drink some water";
        let water = |cooldown: &str| {
            desk_made(
                "speak = true\n",
                &format!("for = \"5m\"\ncooldown = \"{cooldown}\"\nrearm = \"ends\"\n"),
                "[[do]]\nsay = { text = \"Drink some water.\" }\n\n",
                "[quiet]\nfrom = \"22:00\"\nto = \"08:00\"\n\n",
            )
        };
        {
            let _off = LooseHere::set(false);
            let (paced, table) = paced_request(&water("5m"), request);
            assert_eq!(table["trigger"]["cooldown"].as_str(), Some("1h"), "{paced}");
            assert_eq!(table["trigger"]["rearm"].as_str(), Some("ends"));
            assert_eq!(paced, paced_with(&water("5m"), &[], false));
        }
        let _on = LooseHere::set(true);
        for written in ["5m", "1h", "1m"] {
            let (paced, table) = paced_request(&water(written), request);
            let trigger = table["trigger"].as_table().unwrap();
            assert_eq!(trigger["cooldown"].as_str(), Some("5m"), "{paced}");
            assert_eq!(trigger["for"].as_str(), Some("5m"));
            assert!(!trigger.contains_key("rearm"), "{paced}");
            assert_eq!(table["rest"]["after_ignored"].as_integer(), Some(2));
            assert_eq!(table["quiet"]["from"].as_str(), Some("22:00"));
            assert!(capped(&paced).is_ok(), "{paced}");
            let said = said_mannered(&paced, &[]).unwrap();
            assert!(said.contains("at most once every 5 minutes"), "{said}");
        }
        // A repeat of less than a minute is a minute apart.
        let (paced, table) =
            paced_request(&water("1m"), "every 30 seconds while I am here, say hi");
        assert_eq!(table["trigger"]["cooldown"].as_str(), Some("1m"), "{paced}");
        // Never on a time of day, which runs once at its minute.
        let clock = made_with(
            "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
             during = { from = \"22:00\", to = \"22:01\" }\n",
            "light = { dim = 1 }",
        );
        let (paced, table) = paced_request(&clock, "every five minutes at 22:00, dim the light");
        assert_eq!(table["trigger"]["rearm"].as_str(), Some("ends"), "{paced}");
    }

    /// Case 3 (12 Nov 15:16): "when I sit down, nod at me and then do a
    /// backflip", written with `cooldown = "1m"` and `rearm = "ends"`.
    /// Loosened, it is kept as written, its `[rest]` too; one written with
    /// the hour keeps it, never lowered. Off, as before.
    #[test]
    fn loosened_a_moving_made_skill_keeps_its_minute_and_rest() {
        let request = "when I sit down, nod at me and then do a backflip";
        let nod = |cooldown: &str| {
            desk_made(
                "body = true\n",
                &format!("cooldown = \"{cooldown}\"\nrearm = \"ends\"\n"),
                "[[do]]\nbody = { gesture = \"nod\" }\n\n[[do]]\nbody = { gesture = \
                 \"greeting\" }\n\n",
                "",
            )
        };
        {
            let _off = LooseHere::set(false);
            for written in ["1m", "1h"] {
                let (paced, table) = paced_request(&nod(written), request);
                assert_eq!(
                    table["trigger"]["cooldown"].as_str(),
                    Some(written),
                    "{paced}"
                );
                assert_eq!(paced, paced_with(&nod(written), &[], false));
            }
        }
        let _on = LooseHere::set(true);
        for written in ["1m", "1h"] {
            let (paced, table) = paced_request(&nod(written), request);
            assert_eq!(
                table["trigger"]["cooldown"].as_str(),
                Some(written),
                "{paced}"
            );
            assert_eq!(table["trigger"]["rearm"].as_str(), Some("ends"));
            assert_eq!(table["rest"]["after_ignored"].as_integer(), Some(2));
            assert!(!table.contains_key("quiet"), "{paced}");
            assert!(capped(&paced).is_ok(), "{paced}");
        }
    }

    /// #6059 (4): loosened, a made skill's step that can't be said plainly
    /// is said in general words; off, it is refused for a repair. What the
    /// lamp can't do yet is refused either way.
    #[test]
    fn loosened_the_say_back_says_in_general_words_what_it_cant_say_plainly() {
        let dim_for = TOML.replace("{ dim = 1 }", "{ dim = 1, for = \"1m\" }");
        let while_part = format!(
            "{TOML}\n[[while]]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n"
        );
        // A part on the camera or the microphone is never said in general
        // words (the first builder's yes, 22:44): refused as before.
        let camera_while = while_part.replace(
            "topic = \"voice/state\"\nfield = \"state\"",
            "topic = \"room/presence\"\nfield = \"someone\"",
        );
        let look = TOML.replace("light = { dim = 1 }", "look = \"now\"");
        let face = TOML.replace(
            "light = { dim = 1 }",
            "body = { look_at = \"face\", for = \"1m\" }",
        );
        {
            let _off = LooseHere::set(false);
            assert_eq!(said_mannered(&dim_for, &[]).unwrap_err().0, UNSAID);
        }
        let _on = LooseHere::set(true);
        let said = said_mannered(&dim_for, &[]).unwrap();
        assert!(
            said.contains(", it changes the light, at most once every 12 hours"),
            "{said}"
        );
        let said = plainly_as(&while_part, false, true).unwrap();
        assert!(said.contains("under conditions of its own"), "{said}");
        let odd_trigger = TOML.replace("equals = \"resting\"", "equals = \"talking\"");
        let said = plainly_as(&odd_trigger, false, true).unwrap();
        assert!(said.starts_with("When its moment comes, it dims"), "{said}");
        for exact in [&camera_while, &look, &face] {
            assert_eq!(
                plainly_as(exact, false, true).unwrap_err().0,
                UNSAID,
                "{exact}"
            );
        }
        let on_faces = odd_trigger.replace("voice/state", "humans/faces/tracked");
        assert!(plainly_as(&on_faces, false, true).is_err());
        // Never for a skill not made here, nor what it can't do yet.
        assert!(plainly(&dim_for).is_err());
        assert_eq!(
            plainly_as(
                &TOML.replace("voice/state\"\nfield", "voice/heard\"\nfield"),
                false,
                true
            )
            .unwrap_err(),
            Refused(cant_yet("starting on words it hears"), None)
        );
        assert_eq!(
            general_step(&toml::Value::Table(toml::Table::new())).unwrap(),
            "does something"
        );
    }

    /// #6059 (1): the parts [`loosened`] may leave out, and how each is
    /// taken out and told.
    #[test]
    fn loosened_parts_are_taken_out_whole_and_told_plainly() {
        let blink = made_with(
            "topic = \"room/stood_up\"\ncooldown = \"1m\"\n",
            "light = { colour = \"green\", level = 3, for = \"2s\" }\n\n[[do]]\n\
             light = { out = true, for = \"500ms\" }",
        );
        let table: toml::Table = toml::from_str(&blink).unwrap();
        let parts = Loose::of(&table);
        let gap_for = Loose::KindKey {
            step: 1,
            kind: "light".into(),
            key: "for".into(),
        };
        assert!(parts.contains(&gap_for), "{parts:?}");
        assert!(parts.contains(&Loose::Step(1)));
        let words = "not packed: out takes no colour, dim, level, for, pattern or no_brighter";
        assert!(gap_for.named_in(words, &table));
        assert!(!Loose::Step(0).named_in("a trigger's cooldown", &table));
        let (cut, cut_table) = gap_for.taken_out(&blink, &table).unwrap();
        assert!(cut.contains("light = { out = true }"), "{cut}");
        assert_eq!(cut_table["do"][1]["light"].as_table().unwrap().len(), 1);
        assert_eq!(
            gap_for.said(&table),
            "how long it lasts in a step that changes the light"
        );
        let (cut, cut_table) = Loose::Step(1).taken_out(&blink, &table).unwrap();
        assert_eq!(cut_table["do"].as_array().unwrap().len(), 1, "{cut}");
        assert_eq!(Loose::Step(1).said(&table), "a step that changes the light");
        assert_eq!(
            left_out_said(&["a".into(), "b".into()]),
            "Left out: a and b."
        );
        // A write's step is never a part to leave out.
        let write: toml::Table = toml::from_str(
            "[[do]]\nwrite = { service = \"gmail\" }\n\n[[do]]\nlight = { dim = 1 }\n",
        )
        .unwrap();
        assert!(
            !Loose::of(&write)
                .iter()
                .any(|part| matches!(part, Loose::Step(0) | Loose::KindKey { step: 0, .. }))
        );
    }

    /// Heron's P2 on #6437: [`Loose::of`] never offers a trigger's key, nor
    /// a key inside a step on the camera or the microphone ([`named_exactly`]):
    /// a look's `for` never goes alone. The whole step may still go.
    #[test]
    fn loosened_never_takes_a_triggers_key_or_a_camera_steps_key_alone() {
        let looking = made_with(
            "topic = \"room/presence\"\nfield = \"someone\"\nequals = false\ncooldown = \"1m\"\n",
            "body = { look_at = \"face\", for = \"1m\" }\n\n[[do]]\nlight = { dim = 1 }",
        );
        let table: toml::Table = toml::from_str(&looking).unwrap();
        let parts = Loose::of(&table);
        assert!(
            !parts
                .iter()
                .any(|part| matches!(part, Loose::KindKey { step: 0, .. })),
            "{parts:?}"
        );
        assert!(parts.contains(&Loose::Step(0)), "{parts:?}");
        assert!(parts.contains(&Loose::KindKey {
            step: 1,
            kind: "light".into(),
            key: "dim".into(),
        }));
        // Nothing else: no trigger key (`equals`, `cooldown`) is a part.
        assert_eq!(parts.len(), 3, "{parts:?}");
        // A step's guards never go alone (the Reviewer-lead's P3): its
        // `on` and a kind's `after` stay, the step whole may go.
        let guarded: toml::Table = toml::from_str(
            "[[do]]\non = \"arrived\"\nlight = { dim = 1, after = \"say\" }\n\
             body = { gesture = \"nod\" }\n\n[[do]]\nlight = { dim = 2 }\n",
        )
        .unwrap();
        let parts = Loose::of(&guarded);
        assert!(
            !parts.iter().any(|part| matches!(
                part,
                Loose::StepKind { kind, .. } | Loose::KindKey { key: kind, .. }
                    if GUARD_KEYS.contains(&kind.as_str())
            )),
            "{parts:?}"
        );
        assert!(parts.contains(&Loose::StepKind {
            step: 0,
            kind: "body".into()
        }));
        assert!(parts.contains(&Loose::Step(0)));
    }
}
