//! A make whose model answered `cannot:` made as one task for the lamp's
//! agent (`LAMP_MAKE_NO_CANNOT`, off unless `on`; the owner, 16 Nov:
//! "Change the design. Every task must be doable; no 'couldn't' here").
//!
//! With the switch on, a make (by voice or from the app) whose last answer
//! was the model's own `cannot:` (the lamp's own model's, or the lamp's
//! agent's when the make was handed to it, `LAMP_MAKE_SKILL_AGENT`) is not
//! dropped:
//! - **Its task is the person's request**, in their words, plain and at most
//!   [`made_skill::MOST_TASK_CHARS`] ([`task_words`]). A request longer than
//!   that is never cut short: its task is one plain sentence the model
//!   writes in the same call (`task:`), kept only when every number, time,
//!   threshold and named thing in the request survives in it
//!   ([`task_kept`]), else asked again once with what it left out, then
//!   refused as too long ([`Uncovered::TooLong`]). The lamp's agent (Hermes,
//!   through lamp-tasks) does it itself each time the skill runs, with only
//!   the skill's grants.
//! - **When it runs and what it needs are asked of the lamp's own model in
//!   one more short call** ([`system`], [`brief`]): a name, the fewest
//!   grants, and either one `[trigger]` of the lamp's own (a time, someone
//!   coming or leaving, ...) or the words the person will say to ask for it
//!   (`asks:`). Nothing else the model writes is kept: the lamp writes the
//!   skill around it.
//! - **On a trigger**: the made agent task's form ([`skill_text`]): one
//!   `do.task` step, its `run` the lamp's own [`RUN`], which only prints
//!   [`SCRIPT`]; through the same checks, keeping and packing as any made
//!   skill with code (`made_skill::keep_asked`); said back at once and
//!   waiting for the spoken yes (`make_skill_local::code_approve`).
//! - **Only when asked** (no time or moment named, or "when I say …"): the
//!   form of an agent skill added from a file ([`asked_files`],
//!   `crate::upload_agent`, with `LAMP_UPLOAD_TO_AGENT` on): a voice tool
//!   whose note carries the words to say, one `do.task` that has the
//!   lamp's agent follow [`crate::upload_agent::INSTRUCTIONS`], the
//!   person's request in the lamp's own words; kept by
//!   `made_skill::keep_agent_asked`, marked [`ASKED_MARK`], so its read-back
//!   says it comes from [`FROM`]. It is on, its task not agreed: the first
//!   time it is asked for, it is read back and waits for the spoken yes
//!   (`crate::upload_agent::first_run`), then runs.
//! - **Its grants are checked against the ones there are** ([`covered`]):
//!   `crate::skill_task_yes::GRANTS` and, with `LAMP_AGENT_SERVICES` on, a
//!   known service's `-read` or `-write`, linked or not (one not linked is
//!   made all the same, and the make's link note asks for it to be linked).
//!   One no grant covers (the internet while `LAMP_TASK_NET` is off; on,
//!   `net` is kept whenever it runs, the owner's ruling of 17 Nov; one the
//!   lamp doesn't know; anything else) is
//!   told its reason in plain words ([`Uncovered::line`]), never the bare
//!   "I couldn't make that one.", and only its reason's class is
//!   journaled ([`Uncovered::class`]), never the person's words.
//! - **Never said as made**: the lamp says what the agent will do and when
//!   ("I can have the lamp's agent do that, as …: … hands one task to the
//!   lamp's agent: “…”. … It runs only once you agree.",
//!   `made_skill::agent_said`), and every run counts against the day's
//!   task runs as any skill's does (lamp-skills' own count).
//! - **Only when asked, with `LAMP_UPLOAD_TO_AGENT` off**, it is answered
//!   with what to add ([`Uncovered::NoWhen`]). The spoken asks of a made
//!   skill (`LAMP_MADE_SKILL_ASKS`, built on another branch) plug in at
//!   [`no_when`].
//!
//! Off, the make is byte for byte as before.
use crate::made_skill::{self, Made, Refused};

/// The switch, in lamp-voice alone: `on` only.
pub const ENVIRONMENT: &str = "LAMP_MAKE_NO_CANNOT";

#[cfg(test)]
thread_local! {
    /// The switch as a test of this thread sets it.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch is on in this process.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    on_with(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether `value` turns the switch on: `on`, and nothing else.
pub fn on_with(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The switch for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// How the make's answer begins for a request that needs the internet when
/// it runs, with [`net_by_agent`] (the owner's Bitcoin price, lamp-9adf, 17
/// Nov): one line, no blocks, then what it looks up. The model judges that
/// a request needs the internet; the lamp makes it one task for its agent
/// with [`made`], whatever the `LAMP_MAKE_NO_CANNOT` switch, so the one more
/// call picks its grants (`net` among them) and when it runs, and
/// [`covered`] gives it `net`, asked for by its words or on its own
/// trigger (the owner's ruling, 17 Nov).
pub const NEEDS_NET: &str = "internet:";

/// Whether a make's model is told to answer [`NEEDS_NET`] for what only the
/// internet tells: `LAMP_TASK_NET` on (`crate::skill_task_yes::net_on`), a
/// made skill's code and a skill's tasks on (as [`made`] needs), and
/// `LAMP_UPLOAD_TO_AGENT` on, which a task asked for by its words is kept
/// as. Off, the make's prompt and answers are as before: what only the
/// internet tells is the model's `cannot:`.
pub fn net_by_agent() -> bool {
    crate::skill_task_yes::net_on()
        && made_skill::scripts_on()
        && crate::skill_task_yes::on()
        && crate::upload_agent::on()
}

/// Whether the lamp makes tasks for its agent here, so a made skill whose
/// one `do.task` runs the lamp's own [`RUN`] is read as one
/// (`made_skill`): the switch on, or [`net_by_agent`], whose makes of what
/// only the internet tells are such tasks.
pub fn agent_tasks_made() -> bool {
    on() || net_by_agent()
}

/// Whether `answer` is the model's [`NEEDS_NET`] line: it begins so, in any
/// case, and holds no fenced block.
pub fn says_needs_net(answer: &str) -> bool {
    answer
        .trim_start()
        .get(..NEEDS_NET.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(NEEDS_NET))
        && !answer.contains("```")
        && !answer.contains("~~~")
}

/// What a make whose model answered [`NEEDS_NET`] is refused as until the
/// lamp makes it one task for its agent: never said, as [`made`]'s outcome
/// takes its place.
pub const NEEDS_NET_HELD: &str = "it needs the internet, which only the lamp's agent can reach";

/// The one file a task skill made here runs: the lamp's own, never a
/// model's, which only says to the agent that the task is its own to do.
pub const RUN: &str = "scripts/agent.py";

/// [`RUN`]'s text.
pub const SCRIPT: &str = "print(\"This skill's task is for the lamp's agent to do itself, with its \
own tools and only the skill's grants; then tell the owner in a sentence or two what it found or \
did.\")\n";

/// What the make is refused as when the lamp's agent answered `cannot:`
/// with the switch on: never its words, and always made a task
/// (`make_skill_local`), so never said.
pub const AGENTS_CANNOT: &str = "the lamp's agent said no made skill can do it";

/// How a reason no grant covers begins.
pub const CANT_MAKE_YET: &str = "I can't make that one yet:";

/// The most characters of what a `needs:` line names ([`Uncovered::Missing`]).
pub const MOST_MISSING_CHARS: usize = 60;

/// The brief's own refusals that keep people and their services safe
/// (`made_skill::BRIEFS_OWN_REFUSALS`, the Reviewer-lead on #6439), which
/// stay refusals with this switch on (sim-2's P2 on #7261; the owner: "if
/// something can't be done, say exactly why"): the words that mark each in
/// the model's `cannot:`, the class the journal names, and the lamp's own
/// plain reason said in its place ([`kept_refusal`]).
pub const KEPT_REFUSALS: [(&[&str], &str, &str); 3] = [
    (
        &["hear what is said", "hearing what is said"],
        "listening",
        "I can't make that one: a skill can't listen to what people say, to keep the room \
         private.",
    ),
    (
        &["know who someone is", "knowing who someone is"],
        "who_someone_is",
        "I can't make that one: a skill can't tell who someone is, to keep people private.",
    ),
    (
        &[
            "deleting or changing what is in your services",
            "delete or change what is in your services",
        ],
        "delete_or_change",
        "I can't make that one: a skill can't delete or change what's in your services; it \
         can read and send with your yes.",
    ),
];

/// The lamp's plain reason for a brief's own refusal kept with this switch
/// on ([`KEPT_REFUSALS`]), and its class; none for any other. The brief's
/// other refusals that stay (a reminder's, a send not word for word, an
/// event with no day or time, …) already name their reason and what to do,
/// so they are said as the model wrote them.
pub fn kept_refusal(why: &str) -> Option<(&'static str, &'static str)> {
    let why = why.to_lowercase();
    KEPT_REFUSALS
        .iter()
        .find(|(marks, ..)| marks.iter().any(|mark| why.contains(mark)))
        .map(|(_, class, line)| (*class, *line))
}

/// How the line for a request with no time or moment begins.
pub const NO_WHEN_HEAD: &str = "I can make that as a task for the lamp's agent";

/// The line a task skill only asked for carries in its card, beside
/// `crate::upload_agent::MARK`: made from the person's own ask, never from
/// a file (lamp-skills' `task_asks` then says it comes from [`FROM`]).
pub const ASKED_MARK: &str = "agent: asked";

/// Where such a skill's task is said to come from in its read-back.
pub const FROM: &str = "what you asked the lamp for";

/// The lamp's grants a task for the lamp's agent made here may hold, on a
/// trigger or only asked for; the camera ([`ASKED_CAMERA`]) only asked for,
/// and the internet and a service's beside them in [`covered`].
pub const FALLBACK_GRANTS: [&str; 2] = ["light", "speak"];

/// The camera, which a task only asked for may have, run right when the
/// person says its phrase, as a regular made skill may (the first
/// builder's ruling on #7261); never one that runs by itself.
pub const ASKED_CAMERA: &str = "camera";

/// The lamp's grants that watch ([`made_skill::GRANTS`]): never given to a
/// task that runs by itself, on a trigger ([`Uncovered::Sensing`]); of
/// them, only [`ASKED_CAMERA`] to one only asked for.
pub const WATCHING_GRANTS: [&str; 2] = ["camera", "look"];

/// The lamp's grants that listen ([`made_skill::GRANTS`]): never given to
/// a task for the lamp's agent made here ([`Uncovered::Listening`]; sim-2
/// on #7261, the brief's privacy rules).
pub const LISTENING_GRANTS: [&str; 4] = ["microphone", "hear", "speech", "sound"];

/// The name a task skill takes when the model gave none it may have.
const FALLBACK_NAME: &str = "asked-task";

/// The most of a name.
const MOST_NAME_CHARS: usize = 40;

/// Why a request is not made a task: no grant covers what it needs, or it
/// names no time or moment to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Uncovered {
    /// The internet, with `LAMP_TASK_NET` off.
    NetOff,
    /// A linked service, with `LAMP_AGENT_SERVICES` off.
    ServicesOff,
    /// A service the lamp doesn't know.
    ServiceUnknown,
    /// A made skill's code (`LAMP_MADE_SCRIPTS`) or a skill's tasks
    /// (`LAMP_SKILL_TASKS`) off: no task can be handed to the agent.
    TasksOff,
    /// The camera or another grant that watches ([`WATCHING_GRANTS`]), for
    /// a task that runs by itself, on a trigger.
    Sensing,
    /// The microphone or another grant that listens ([`LISTENING_GRANTS`]).
    Listening,
    /// Anything else no grant gives.
    Other,
    /// What the model named that no grant gives (`needs: what`), in plain
    /// words of its own, bounded ([`MOST_MISSING_CHARS`]).
    Missing(String),
    /// No time and no moment named for it to run.
    NoWhen,
    /// A reminder, the lamp's own timer: the model's `cannot:` with the
    /// make's own refusal of one ([`reminder_rule`]).
    Reminder,
    /// A request longer than its read-back ([`task_cut`]) whose model's
    /// own words for it ([`task_kept`]) left out what it names twice: the
    /// agent is told only what the person heard, never the request cut
    /// short (El's P1 on #7261).
    TooLong,
}

impl Uncovered {
    /// The class the journal names, never the person's words.
    pub fn class(&self) -> &'static str {
        match self {
            Self::NetOff => "net_off",
            Self::ServicesOff => "services_off",
            Self::ServiceUnknown => "service_unknown",
            Self::TasksOff => "tasks_off",
            Self::Sensing => "sensing",
            Self::Listening => "microphone",
            Self::Other => "other",
            Self::Missing(_) => "missing",
            Self::NoWhen => "no_when",
            Self::Reminder => "reminder",
            Self::TooLong => "too_long",
        }
    }

    /// What the person hears, in plain words.
    pub fn line(&self) -> String {
        match self {
            Self::NetOff => format!(
                "{CANT_MAKE_YET} it needs the internet, and skills on this lamp can't use the \
                 internet yet."
            ),
            Self::ServicesOff => format!(
                "{CANT_MAKE_YET} it needs your linked services, and skills on this lamp can't \
                 use them yet."
            ),
            Self::ServiceUnknown => {
                format!("{CANT_MAKE_YET} it needs a service the lamp can't link yet.")
            }
            Self::TasksOff => format!(
                "{CANT_MAKE_YET} it needs the lamp's agent, and skills on this lamp can't hand \
                 tasks to it yet."
            ),
            Self::Sensing => format!(
                "{CANT_MAKE_YET} it would need the camera, and a skill that runs by itself \
                 can't watch or listen, to keep the room private. Ask me to make it for when you \
                 say a few words, and it can use the camera then."
            ),
            Self::Listening => format!(
                "{CANT_MAKE_YET} it would need the microphone, and a skill can't listen, to keep \
                 the room private."
            ),
            Self::Other => {
                format!(
                    "{CANT_MAKE_YET} it needs something no skill on this lamp can be given yet."
                )
            }
            Self::Missing(what) => format!(
                "{CANT_MAKE_YET} it needs {what}, which no skill on this lamp can be given yet."
            ),
            Self::NoWhen => no_when(),
            Self::Reminder => format!(
                "{CANT_MAKE_YET} a reminder is my own timer, not a skill. {}",
                made_skill::REMINDER_INSTEAD
            ),
            Self::TooLong => format!(
                "{CANT_MAKE_YET} it's too long for me to read back to you whole, so ask for it in \
                 fewer words."
            ),
        }
    }

    /// Refused with [`Self::line`], journaled by [`Self::class`].
    fn refused(self) -> Refused {
        eprintln!(
            "lamp-voice: a skill made here: the model's cannot was not made a task for the lamp's \
             agent: {} ({ENVIRONMENT})",
            self.class()
        );
        Refused(self.line(), None)
    }
}

/// What a request only asked for hears while it can't be made so
/// (`LAMP_UPLOAD_TO_AGENT` off). The seam for a made skill's spoken asks
/// (`LAMP_MADE_SKILL_ASKS`): with them, such a request becomes a made task
/// skill asked for by its phrases here instead.
pub fn no_when() -> String {
    format!(
        "{NO_WHEN_HEAD} once you tell me when it should run, like every morning at eight or when \
         you sit down at your desk."
    )
}

/// Whether `why` is one of this module's own lines, said as it is.
pub fn said_as_is(why: &str) -> bool {
    why.starts_with(CANT_MAKE_YET)
        || why.starts_with(NO_WHEN_HEAD)
        || KEPT_REFUSALS.iter().any(|(_, _, line)| why == *line)
}

/// The grants the model may name, each in plain words, for [`system`].
fn grants_told() -> String {
    let mut told: Vec<String> = made_skill::GRANTS
        .iter()
        .filter(|(name, _)| FALLBACK_GRANTS.contains(name))
        .map(|(name, words)| format!("{name} ({words})"))
        .collect();
    told.push(format!(
        "{ASKED_CAMERA} (the camera, only for a skill that runs when the person asks for it, never \
         on a trigger)"
    ));
    told.push(format!(
        "{} (the internet: anything looked up or fetched, like the weather, the news, a web page \
         or a price)",
        crate::skill_task_yes::NET.0
    ));
    told.push(format!(
        "CODE-read to read the person's linked CODE, CODE-write to send with it, CODE one of: {}",
        made_skill::SERVICE_GRANT_CODES.join(", ")
    ));
    told.join("; ")
}

/// The one whole answer shown to the model, for [`EXAMPLE_ASKED`].
const EXAMPLE: &str = "name: morning-joke
grants: speak
```skill.toml
[permissions]
topics = [\"clock/local\"]

[trigger]
topic = \"clock/local\"
field = \"known\"
equals = true
rearm = \"ends\"
during = { from = \"08:00\", to = \"08:01\" }
```";

/// The request [`EXAMPLE`] is for.
const EXAMPLE_ASKED: &str = "every morning at eight, tell me a joke";

/// The sitting the lamp's own sip-of-water fires on, told the one more
/// call with `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on (lamp-9adf, 18 Nov:
/// "remind me to stretch when I've been sitting a long while" was made on
/// someone getting up): sip-of-water's `for`, its once-a-sitting `rearm`
/// and its `cooldown`, on the desk's presence, the one sitting a made
/// trigger can say (`made_skill`'s check takes no `near` or `gap` on the
/// faces). The model judges by meaning which one a request is.
const SITTING: &str = " Someone has sat at the desk a long while, as the lamp's own \
sip-of-water fires: \"room/presence\", \"someone\", equals true, for = \"30m\" (or the while the \
request names), rearm = \"leaves\" (once a sitting, again only after they have left), \
cooldown = \"45m\"; a long sitting is never a getting up, which runs only as they stand. \
Someone coming back to the desk is an arrival, never the desk left.";

/// One whole answer for a long sitting ([`SITTING`]), shown to the model
/// with `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on: sip-of-water's sitting.
const SAT_EXAMPLE: &str = "name: shoulder-roll
grants: speak
```skill.toml
[permissions]
topics = [\"room/presence\"]

[trigger]
topic = \"room/presence\"
field = \"someone\"
equals = true
for = \"30m\"
rearm = \"leaves\"
cooldown = \"45m\"
```";

/// The request [`SAT_EXAMPLE`] is for.
const SAT_EXAMPLE_ASKED: &str = "when I've sat at my desk a long while, remind me to stretch";

/// One whole answer for a task only asked for, shown to the model.
const ASKED_EXAMPLE: &str = "name: desk-photo
grants: camera, speak
asks: take a desk photo";

/// The request [`ASKED_EXAMPLE`] is for.
const ASKED_EXAMPLE_ASKED: &str =
    "when I say take a desk photo, take a photo of my desk and tell me what's on it";

/// What the lamp's own model is told for the one more call: it picks the
/// name, the fewest grants and when the task runs, never the task or code.
/// With `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on (`made_skill::reactions_on`),
/// where a make judged no quick reaction comes here first, it also refuses
/// a reminder as the make's own brief does ([`reminder_rule`], the
/// Reviewer-lead's P2 on #7764), and is told sip-of-water's sitting beside
/// the triggers ([`SITTING`]) with a whole answer for it ([`SAT_EXAMPLE`]);
/// off, as before.
pub fn system() -> String {
    let (reminder, sitting, sat_example) = if made_skill::reactions_on() {
        (
            reminder_rule(),
            SITTING,
            format!("\nAnd for \"{SAT_EXAMPLE_ASKED}\":\n\n{SAT_EXAMPLE}\n"),
        )
    } else {
        (String::new(), "", String::new())
    };
    format!(
        "You choose, for a lamp, when a skill runs and what it needs. The lamp's agent, an AI \
         agent on the lamp, does the person's request itself as one task each time the skill \
         runs, with only the grants the skill names; the lamp reads the skill back to the person \
         and runs it only once they agree. You never write the task or any code: only its name, \
         its grants and when it runs. Answer exactly in this form and nothing else: a line \
         name: and a short name, lowercase words joined by -, the name the request gives the \
         skill when it gives one; a line grants: and the fewest \
         grants the task needs, joined by commas, or none; then, when the request names a time \
         or a moment for it to run, one fenced block named skill.toml holding only \
         [permissions] with topics = its trigger's topics, and one [trigger] table (never \
         [[trigger]]); else, when it runs only when the person asks for it or says something \
         to the lamp (\"when I say send my test note\"), no block but a line asks: and the \
         few words they will say, as they said them or, when they named none, a short phrase \
         for the task, several joined by |. When it runs: {triggers}{sitting} Add no time of day the \
         request didn't name. The grants, each only when the task needs it: {grants}. speak \
         whenever the agent tells the person anything aloud. When the task needs something \
         none of these grants gives, answer only this one line: needs: and what, in a few plain \
         words (\"needs: paying for things\").{reminder} A whole answer, \
         for \"{EXAMPLE_ASKED}\":\n\n{EXAMPLE}\n\nAnd for \"{ASKED_EXAMPLE_ASKED}\":\n\n\
         {ASKED_EXAMPLE}\n{sat_example}",
        triggers = made_skill::TRIGGER_MEANINGS,
        grants = grants_told(),
    )
}

/// The make's own refusal of a reminder ([`made_skill::REMINDER_IS_TIMER`],
/// its `cannot:` ending [`made_skill::REMINDER_INSTEAD`]), told the one
/// more call: a one-off reminder judged no quick reaction, or not judged in
/// time, is refused ([`Uncovered::Reminder`]), never kept as a daily task.
fn reminder_rule() -> String {
    format!(
        " {}. A request for one is answered with only this one line: {} then one short \
         sentence said to the person, beginning \"{}\" and naming it, ending \"{}\"",
        made_skill::REMINDER_IS_TIMER,
        made_skill::CANNOT,
        made_skill::CANT_YET,
        made_skill::REMINDER_INSTEAD,
    )
}

/// The one more call's request: the person's, as the make's brief carried
/// it.
pub fn brief(request: &str, hint: Option<&str>) -> String {
    let hint = hint
        .map(|code| {
            format!(
                "\n\nThe request may mean the person's {} ({code}-read to read it, {code}-write \
                 to send with it): name its grant only if the request asks for that service.",
                made_skill::service_name(code)
            )
        })
        .unwrap_or_default();
    // A request longer than its read-back: its task's words are the
    // model's, checked by [`task_kept`].
    let task = if task_cut(request) {
        format!(
            "\n\nThe request is too long to read back whole: also answer a line task: and the \
             task for the lamp's agent in one plain sentence of at most {} characters that \
             keeps every number, time, threshold and named thing in the request.",
            made_skill::MOST_TASK_CHARS
        )
    } else {
        String::new()
    };
    format!(
        "{}, as one task for the lamp's agent: choose its name, grants and when it runs.\n\n\
         Request: {}{hint}{task}",
        made_skill::BRIEF_HEAD,
        request.trim()
    )
}

/// [`brief`] again, with the lamp's check's `lines` on the last answer.
fn repair_brief(request: &str, hint: Option<&str>, lines: &str) -> String {
    format!(
        "{}\n\nThe lamp refused your last answer: {} Answer again, exactly in the form told.",
        brief(request, hint),
        lines.trim()
    )
}

/// The model's answer to the one more call, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// A name, the grants named and when it runs.
    Skill {
        name: String,
        grants: Vec<String>,
        when: When,
    },
    /// No grant gives what it needs (`needs:`), or no time is named
    /// (`when: none`).
    Uncovered(Uncovered),
}

/// When a task skill runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum When {
    /// On its trigger: the model's block.
    Trigger(String),
    /// Only when asked: the words to say.
    Asked(Vec<String>),
}

/// What a repair is told of an answer not in the form.
const NOT_THE_FORM: &str = "answer a line name:, a line grants: and either one fenced \
                            skill.toml block holding only [permissions] topics and one \
                            [trigger], or a line asks:; or only needs: and what";

/// `answer` read as [`Answer`]; else what a repair is told.
pub fn read(answer: &str) -> Result<Answer, String> {
    let mut name = None;
    let mut grants = None;
    let mut asks: Vec<String> = Vec::new();
    let mut block: Option<Vec<&str>> = None;
    let mut fenced = false;
    for line in answer.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if fenced {
                fenced = false;
                continue;
            }
            if block.is_some() {
                return Err(NOT_THE_FORM.to_owned());
            }
            fenced = true;
            block = Some(Vec::new());
            continue;
        }
        if fenced {
            if let Some(block) = block.as_mut() {
                block.push(line);
            }
            continue;
        }
        let lower = trimmed.to_lowercase();
        if let Some(rest) = after_head(trimmed, "needs:") {
            return Ok(Answer::Uncovered(missing(rest)));
        }
        // Only the make's own refusal of a reminder ([`reminder_rule`]):
        // any other `cannot:` is not the form, never said.
        if let Some(rest) = after_head(trimmed, made_skill::CANNOT) {
            if made_skill::briefs_reminder_refusal(rest) {
                return Ok(Answer::Uncovered(Uncovered::Reminder));
            }
            return Err(NOT_THE_FORM.to_owned());
        }
        if let Some(rest) = after_head(trimmed, "asks:") {
            for said in rest.split('|').filter_map(phrase) {
                if !asks
                    .iter()
                    .any(|kept: &String| kept.eq_ignore_ascii_case(&said))
                {
                    asks.push(said);
                }
            }
            asks.truncate(crate::upload_agent::MOST_TRIGGERS);
        } else if let Some(rest) = lower.strip_prefix("name:") {
            name = Some(rest.trim().to_owned());
        } else if let Some(rest) = lower.strip_prefix("grants:") {
            let rest = rest.trim().trim_matches(['[', ']']);
            grants = Some(if rest == "none" || rest.is_empty() {
                Vec::new()
            } else {
                rest.split(',')
                    .map(|grant| grant.trim().trim_matches(['"', '\'', '`']).to_owned())
                    .filter(|grant| !grant.is_empty())
                    .collect()
            });
        }
    }
    let name = name_kept(name.as_deref().unwrap_or_default());
    match (grants, block) {
        (Some(grants), Some(block)) if !fenced => Ok(Answer::Skill {
            name,
            grants,
            when: When::Trigger(block.join("\n")),
        }),
        (Some(grants), None) if !asks.is_empty() => Ok(Answer::Skill {
            name,
            grants,
            when: When::Asked(asks),
        }),
        _ => Err(NOT_THE_FORM.to_owned()),
    }
}

/// What follows `head` at the start of `line`, in any case.
fn after_head<'a>(line: &'a str, head: &str) -> Option<&'a str> {
    line.get(..head.len())
        .filter(|start| start.eq_ignore_ascii_case(head))
        .map(|_| &line[head.len()..])
}

/// A phrase to say as a tool's note carries it: one line, no quotes, no
/// backslash or control character, 1 to 80 characters.
fn phrase(text: &str) -> Option<String> {
    let text: String = text
        .trim()
        .trim_matches(['"', '\'', '“', '”', '`', '*'])
        .chars()
        .filter(|c| {
            !c.is_control() && !made_skill::is_format(*c) && !matches!(c, '"' | '\\' | '“' | '”')
        })
        .collect();
    let text = text.trim();
    (!text.is_empty() && text.chars().count() <= 80).then(|| text.to_owned())
}

/// A task's name: the one the person gave (`named`, #7911,
/// [`made_skill::given_name`]) when it is one a made skill may have
/// ([`name_kept`]), else the model's own `name`, as before.
fn name_given(name: String, named: Option<&str>) -> String {
    match named.and_then(|said| made_skill::given_name(said, MOST_NAME_CHARS)) {
        Some(given) if name_kept(&given) == given => {
            if given != name {
                eprintln!(
                    "lamp-voice: a task only asked for takes the name its person gave: {given}, not {name}"
                );
            }
            given
        }
        Some(_) => {
            eprintln!("lamp-voice: a task's given name is no skill's name: {name} stands");
            name
        }
        None => name,
    }
}

/// A name as a made skill may have it: lowercase ASCII letters, digits and
/// `-`, beginning with a letter, at most [`MOST_NAME_CHARS`]; else
/// [`FALLBACK_NAME`].
fn name_kept(name: &str) -> String {
    let name = name.trim().trim_matches(['"', '\'', '`']);
    let ok = (3..=MOST_NAME_CHARS).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        name.to_owned()
    } else {
        FALLBACK_NAME.to_owned()
    }
}

/// The trigger's block as a task skill takes it: TOML of only
/// `[permissions]`, with `topics` alone, and one `[trigger]` table; else
/// what a repair is told.
pub fn block_kept(block: &str) -> Result<String, String> {
    let table: toml::Table = toml::from_str(block).map_err(|_| NOT_THE_FORM.to_owned())?;
    let only = table
        .keys()
        .all(|key| key == "permissions" || key == "trigger");
    let trigger = table.get("trigger").is_some_and(toml::Value::is_table);
    let permissions = table.get("permissions").is_none_or(|permissions| {
        permissions
            .as_table()
            .is_some_and(|permissions| permissions.keys().all(|key| key == "topics"))
    });
    if only && trigger && permissions {
        Ok(block.trim().to_owned())
    } else {
        Err(NOT_THE_FORM.to_owned())
    }
}

/// The grants `asked` checked against the ones there are, once each in the
/// order named: one of [`FALLBACK_GRANTS`]; never one that listens
/// ([`LISTENING_GRANTS`], [`Uncovered::Listening`]), nor, on a trigger, one
/// that watches ([`WATCHING_GRANTS`], [`Uncovered::Sensing`]); for a task
/// `only_asked`, [`ASKED_CAMERA`]; the internet with `LAMP_TASK_NET` on,
/// whenever it runs; or with
/// `LAMP_AGENT_SERVICES` on a known service's, linked or not:
/// one not linked is made all the same, and the make's link note
/// (`made_skill::needs_link`) asks for it to be linked; the run's own grant
/// check holds it until then. Else what no grant covers.
pub fn covered(asked: &[String], only_asked: bool) -> Result<Vec<String>, Uncovered> {
    let mut grants: Vec<String> = Vec::new();
    for grant in asked {
        let grant = grant.trim().to_lowercase();
        if grants.contains(&grant) {
            continue;
        }
        // The internet: only with `LAMP_TASK_NET` on, asked for or on a
        // trigger (the owner's ruling, 17 Nov: "check the Bitcoin price
        // every day at 9 and tell me" may have it); its run reaches the
        // web only with the owner's kept yes.
        if grant == crate::skill_task_yes::NET.0 {
            if !crate::skill_task_yes::net_on() {
                return Err(Uncovered::NetOff);
            }
            grants.push(grant);
            continue;
        }
        if FALLBACK_GRANTS.contains(&grant.as_str()) {
            grants.push(grant);
            continue;
        }
        if LISTENING_GRANTS.contains(&grant.as_str()) {
            return Err(Uncovered::Listening);
        }
        if only_asked && grant == ASKED_CAMERA {
            grants.push(grant);
            continue;
        }
        if !only_asked && WATCHING_GRANTS.contains(&grant.as_str()) {
            return Err(Uncovered::Sensing);
        }
        if made_skill::service_grant(&grant).is_some() {
            if !made_skill::agent_services_here() {
                return Err(Uncovered::ServicesOff);
            }
            grants.push(grant);
            continue;
        }
        if grant.ends_with("-read") || grant.ends_with("-write") {
            return Err(Uncovered::ServiceUnknown);
        }
        return Err(Uncovered::Other);
    }
    Ok(grants)
}

/// The person's `request` as the task's words: one line, no control or
/// format character, no double quote, backtick or backslash, at most
/// [`made_skill::MOST_TASK_CHARS`], cut at a word's end, no end stop.
/// What a `needs:` line names, as [`Uncovered::Missing`]: plain, cut on a
/// word at [`MOST_MISSING_CHARS`], its last stop dropped; nothing, or
/// `other`, is [`Uncovered::Other`].
fn missing(named: &str) -> Uncovered {
    let mut what = task_words(named);
    if what.chars().count() > MOST_MISSING_CHARS {
        let cut: String = what.chars().take(MOST_MISSING_CHARS).collect();
        what = match cut.rfind(' ') {
            Some(at) => cut[..at].to_owned(),
            None => cut,
        };
    }
    let what = what
        .trim()
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .trim();
    if what.is_empty() || what.eq_ignore_ascii_case("other") {
        Uncovered::Other
    } else {
        Uncovered::Missing(what.to_owned())
    }
}

pub fn task_words(request: &str) -> String {
    let plain = task_plain(request);
    let most = made_skill::MOST_TASK_CHARS;
    let words = if plain.chars().count() <= most {
        plain
    } else {
        let cut: String = plain.chars().take(most).collect();
        match cut.rsplit_once(' ') {
            Some((head, _)) if !head.is_empty() => head.to_owned(),
            _ => cut,
        }
    };
    words
        .trim()
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .trim()
        .to_owned()
}

/// Whether [`task_words`] cut `request` short: more of it than the
/// trailing punctuation it trims was left out, so its task's words are the
/// model's own, checked ([`task_kept`]), never the request cut short (El's
/// P1 on #7261).
pub fn task_cut(request: &str) -> bool {
    let words = task_words(request);
    task_plain(request)
        .trim()
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .trim()
        != words
}

/// The task's words for `request`, from the model's `answer` for a skill
/// named `name`: the request's own ([`task_words`]) when it fits; else the
/// answer's `task:` line, plain as [`task_words`] makes it, at most
/// [`made_skill::MOST_TASK_CHARS`], kept only when nothing the request
/// names is left out of it ([`left_out`]). Else what a repair is told.
pub fn task_kept(request: &str, answer: &str, name: &str) -> Result<String, String> {
    if !task_cut(request) {
        return Ok(task_words(request));
    }
    let most = made_skill::MOST_TASK_CHARS;
    let told = format!(
        "the request is too long to read back whole, so answer a line task: and its task in one \
         plain sentence of at most {most} characters that keeps every number, time, threshold \
         and named thing in it"
    );
    let words = task_plain(said_task(answer).unwrap_or_default());
    let words = words
        .trim()
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .trim();
    if words.is_empty() {
        return Err(format!("{told}."));
    }
    let count = words.chars().count();
    if count > most {
        return Err(format!("{told}; yours was {count} characters."));
    }
    let left = left_out(request, words, name);
    if !left.is_empty() {
        return Err(format!("{told}; yours left out: {}.", left.join(", ")));
    }
    Ok(words.to_owned())
}

/// What follows the first `task:` line of `answer` outside a fenced block,
/// read as [`read`] reads its other lines.
fn said_task(answer: &str) -> Option<&str> {
    let mut fenced = false;
    for line in answer.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced && let Some(rest) = after_head(trimmed, "task:") {
            return Some(rest);
        }
    }
    None
}

/// The words of a time or a moment a task keeps as they are: a day, a
/// month (but "may"), one beside today, and what a time is before or after.
const TIME_WORDS: [&str; 30] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "weekend",
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "today",
    "tonight",
    "tomorrow",
    "yesterday",
    "noon",
    "midnight",
    "before",
    "after",
    "since",
    "until",
    "ago",
];

/// The words of a threshold's comparison: one in the request needs one,
/// any, in the task's words.
const COMPARING: [&str; 17] = [
    "more", "less", "over", "under", "above", "below", "least", "most", "than", "exceed",
    "exceeds", "higher", "lower", "greater", "fewer", "beyond", "within",
];

/// A number word's value, and whether it multiplies the words before it
/// ("hundred").
fn number_word(word: &str) -> Option<(f64, bool)> {
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
    const ORDINALS: [&str; 12] = [
        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth", "eleventh", "twelfth",
    ];
    let at = |list: &[&str]| list.iter().position(|one| *one == word);
    if let Some(at) = at(&UNITS) {
        return Some((at as f64, false));
    }
    if let Some(at) = at(&TENS) {
        return Some(((at as f64 + 2.0) * 10.0, false));
    }
    if let Some(at) = at(&ORDINALS) {
        return Some((at as f64 + 1.0, false));
    }
    match word {
        "half" => Some((0.5, false)),
        "quarter" => Some((0.25, false)),
        "hundred" => Some((100.0, true)),
        "thousand" => Some((1_000.0, true)),
        "million" => Some((1_000_000.0, true)),
        _ => None,
    }
}

/// A number `text` names: its words, its parts' values and its whole's
/// ("twenty five": 20, 5 and 25; "9:30" is two, 9 and 30).
type Number = (String, Vec<f64>, f64);

/// What `text` names that a task must keep: its numbers; its words,
/// lowercase, each with whether it is a name (capitalised, not a
/// sentence's first word, not "I"); whether it compares ([`COMPARING`],
/// `>` or `<`); and whether it is in percent.
struct Named {
    numbers: Vec<Number>,
    words: Vec<(String, bool)>,
    compares: bool,
    percent: bool,
}

impl Named {
    fn of(text: &str) -> Self {
        let mut named = Named {
            numbers: Vec::new(),
            words: Vec::new(),
            compares: text.contains(['>', '<']),
            percent: text.contains('%') || text.to_lowercase().contains("per cent"),
        };
        // A run of number words: its words, its parts, and its whole as
        // the words before a scale ("thousand") and after it.
        let mut run = Run::default();
        let mut first = true;
        for token in text.split_whitespace() {
            for (value, digits) in digit_runs(token) {
                run.end(&mut named.numbers);
                named.numbers.push((digits, vec![value], value));
            }
            for (part, piece) in token.split(['-', '/']).enumerate() {
                let word = piece.trim_matches(|c: char| !c.is_alphanumeric());
                if word.is_empty() {
                    continue;
                }
                let lower = word.to_lowercase();
                match number_word(&lower) {
                    Some((value, scales)) => run.add(&lower, value, scales),
                    None => run.end(&mut named.numbers),
                }
                named.compares |= COMPARING.contains(&lower.as_str());
                named.percent |= lower.starts_with("percent");
                let name = !(first && part == 0)
                    && word.chars().any(char::is_uppercase)
                    && word != "I"
                    && !word.starts_with("I'");
                named.words.push((lower, name));
            }
            first = token.ends_with(['.', '!', '?', ':', ';']);
        }
        run.end(&mut named.numbers);
        named
    }
}

/// Number words in a row, read as one number.
#[derive(Default)]
struct Run {
    words: Vec<String>,
    parts: Vec<f64>,
    scaled: f64,
    now: f64,
}

impl Run {
    fn add(&mut self, word: &str, value: f64, scales: bool) {
        self.words.push(word.to_owned());
        self.parts.push(value);
        if !scales {
            self.now += value;
            return;
        }
        self.now = self.now.max(1.0) * value;
        if value >= 1_000.0 {
            self.scaled += self.now;
            self.now = 0.0;
        }
    }

    fn end(&mut self, numbers: &mut Vec<Number>) {
        let run = std::mem::take(self);
        if !run.words.is_empty() {
            numbers.push((run.words.join(" "), run.parts, run.scaled + run.now));
        }
    }
}

/// The numbers written in digits in `token`, each with its digits ("0.5",
/// "1,000"): a `.` or `,` only between digits.
fn digit_runs(token: &str) -> Vec<(f64, String)> {
    let chars: Vec<char> = token.chars().collect();
    let mut runs = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if !chars[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let begin = at;
        while at < chars.len()
            && (chars[at].is_ascii_digit()
                || (matches!(chars[at], '.' | ',')
                    && chars.get(at + 1).is_some_and(char::is_ascii_digit)))
        {
            at += 1;
        }
        let digits: String = chars[begin..at].iter().collect();
        if let Ok(value) = digits.replace(',', "").parse::<f64>() {
            runs.push((value, digits));
        }
    }
    runs
}

/// What `request` names that the task's `words` leave out, in the
/// request's words: a number (in words or digits, the same value either
/// way), a time word ([`TIME_WORDS`]), a name, a comparison or a percent.
/// The words of the skill's own `name` ("evening-rate") need not be in its
/// task, nor the lamp's own name.
fn left_out(request: &str, words: &str, name: &str) -> Vec<String> {
    let asked = Named::of(request);
    let said = Named::of(words);
    let values: Vec<f64> = said
        .numbers
        .iter()
        .flat_map(|(_, parts, whole)| parts.iter().copied().chain([*whole]))
        .collect();
    let has = |value: f64| values.iter().any(|one| (one - value).abs() < 1e-9);
    let mut left: Vec<String> = Vec::new();
    let mut leave = |what: &str| {
        if !left.iter().any(|one| one == what) {
            left.push(what.to_owned());
        }
    };
    for (what, parts, whole) in &asked.numbers {
        if !has(*whole) && !parts.iter().all(|part| has(*part)) {
            leave(what);
        }
    }
    let own: Vec<&str> = name.split('-').chain(["lamp"]).collect();
    for (word, is_name) in &asked.words {
        let kept = !(*is_name || TIME_WORDS.contains(&word.as_str()))
            || own.contains(&word.as_str())
            || said
                .words
                .iter()
                .any(|(said, _)| said.starts_with(word.as_str()));
        if !kept {
            leave(word);
        }
    }
    if asked.compares && !said.compares {
        leave("its comparison (more than, less than, ...)");
    }
    if asked.percent && !said.percent {
        leave("percent");
    }
    left
}

/// `request` as one plain line: no control or format character, no quote
/// mark but `'`, no fence or backslash, its spaces one.
fn task_plain(request: &str) -> String {
    let plain: String = request
        .chars()
        .map(|c| match c {
            '"' | '“' | '”' => '\'',
            c if c.is_control() || made_skill::is_format(c) || matches!(c, '\\' | '`') => ' ',
            c => c,
        })
        .collect();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The whole task skill, as a model's answer: its `skill.toml` (the grants,
/// the trigger's `block`, `[ask]` a nudge, one `do.task` of `words` running
/// [`RUN`]), its `SKILL.md` named `name`, and [`RUN`] as [`SCRIPT`]. With
/// `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on (`made_skill::reactions_on`, the
/// first builder's 19:50 ruling on #6811), sip-of-water's guards
/// (`made_skill::template_guards`) after the trigger: never in the middle
/// of a conversation or with the head down. Off, as before. Never on a
/// clock (`made_skill::CLOCK_TOPIC`): its one minute's run is never spent
/// waiting on a guard ("tell me the BTC price at 9" with the owner head
/// down at 9 still runs; Twelve's P1 on #7764, the first builder's 01:29
/// and 02:41).
pub fn skill_text(name: &str, grants: &[String], block: &str, words: &str) -> String {
    // On its trigger it tells the person only through the lamp's told
    // answer, its agent's last answer (#8610): never `speak` or `speaker`,
    // which `made_skill::keep_asked` refuses on a trigger.
    let grants: Vec<String> = grants
        .iter()
        .filter(|grant| !["speak", "speaker"].contains(&grant.as_str()))
        .map(|grant| format!("\"{grant}\""))
        .collect();
    let guards = if made_skill::reactions_on() && !on_the_clock(block) {
        format!("{}\n\n", made_skill::template_guards())
    } else {
        String::new()
    };
    format!(
        "```skill.toml\nkind = \"installable\"\nversion = \"1.0.0\"\ngrants = [{grants}]\n\n\
         {block}\n\n{guards}[ask]\npolicy = \"nudge\"\n\n[[do]]\ntask = {{ task = \"{words}\", run = \
         \"{RUN}\" }}\n```\n\n```SKILL.md\n---\nname: {name}\ndescription: The lamp's agent \
         does a task you asked for.\n---\n\n# {name}\n\nThe lamp's agent does this task: \
         {words}\n```\n\n```{RUN}\n{SCRIPT}```\n",
        grants = grants.join(", "),
    )
}

/// Whether `block`'s `[trigger]` is the lamp's own clock
/// (`made_skill::CLOCK_TOPIC`).
pub fn on_the_clock(block: &str) -> bool {
    toml::from_str::<toml::Table>(block).is_ok_and(|table| {
        table
            .get("trigger")
            .and_then(|trigger| trigger.get("topic"))
            .and_then(toml::Value::as_str)
            == Some(made_skill::CLOCK_TOPIC)
    })
}

/// A task skill only asked for, in the form of an agent skill added from a
/// file (`crate::upload_agent::files`): its `skill.toml` (a voice tool named
/// for it, `grants`, one `do.task` of `words` that runs
/// [`crate::upload_agent::INSTRUCTIONS`]), a card whose description is the
/// tool's note with the words to say (`asks`), and the instructions:
/// exactly `words`, the task read back and agreed to, in the lamp's own
/// words around it, never more of the request (El's P1 on #7261).
pub fn asked_files(
    name: &str,
    asks: &[String],
    grants: &[String],
    words: &str,
) -> made_skill::Files {
    let tool = name.replace('-', "_");
    let quoted: Vec<String> = grants.iter().map(|grant| format!("\"{grant}\"")).collect();
    let toml = format!(
        "kind = \"installable\"\nversion = \"1.0.0\"\nneeds = [\"voice\"]\ntool = \"{tool}\"\n\
         grants = [{}]\n\n[[do]]\ntask = {{ task = \"{words}\", run = \"{}\" }}\n",
        quoted.join(", "),
        crate::upload_agent::INSTRUCTIONS,
    );
    let note = crate::upload_agent::note(
        &format!("The lamp's agent does a task the person asked for: {words}."),
        asks,
    );
    let card = format!(
        "---\nname: {name}\ndescription: \"{}\"\n---\n\nAsked for by voice. The lamp's agent does \
         its task, kept in {}.\n",
        note.replace(['"', '\\'], "'"),
        crate::upload_agent::INSTRUCTIONS,
    );
    let instructions = format!(
        "---\nname: {name}\ndescription: {words}\n---\n\n# {name}\n\nDo this for the lamp's \
         owner, as they asked the lamp in their own words, and tell them in a sentence or two \
         what you found or did:\n\n{words}\n"
    );
    made_skill::Files {
        toml,
        card,
        left_out: None,
        scripts: vec![(crate::upload_agent::INSTRUCTIONS.to_owned(), instructions)],
        shows: Vec::new(),
    }
}

/// The make of `request`, its model's `cannot:` made one task for the
/// lamp's agent: the one more call to the lamp's own model (`write`, each
/// begun only while `fits`), its answer read ([`read`]), its grants
/// covered ([`covered`]), its task's words kept ([`task_kept`]), the skill
/// written ([`skill_text`]) and
/// kept through the make's own checks (`kept`), with up to
/// `make_skill_local::MOST_REPAIRS` repairs of the form or the check. Not
/// made: a reason no grant covers, in plain words ([`Uncovered::line`]).
pub fn made(
    request: &str,
    mut write: impl FnMut(&str) -> Result<String, String>,
    fits: impl Fn() -> bool,
    kept: &impl Fn(Result<String, String>) -> Result<Made, Refused>,
    kept_asked: impl Fn(&made_skill::Files, &str) -> Result<Made, Refused>,
    hint: Option<&str>,
    named: Option<&str>,
) -> Result<Made, Refused> {
    if !made_skill::scripts_on() || !crate::skill_task_yes::on() {
        return Err(Uncovered::TasksOff.refused());
    }
    let words = task_words(request);
    if words.is_empty() {
        return Err(Uncovered::Other.refused());
    }
    let mut brief = brief(request, hint);
    let most = crate::make_skill_local::MOST_REPAIRS;
    let mut last = Err(Refused::new(made_skill::MODEL_SILENT));
    // Whether this round's task words, and an earlier round's, left out
    // what a long request names ([`task_kept`]).
    let (mut task_missed, mut task_asked_again) = (false, false);
    for round in 0..=most {
        if !fits() {
            eprintln!(
                "lamp-voice: a skill made here: no time left to make the model's cannot a task \
                 for the lamp's agent (round {round})"
            );
            return last;
        }
        let answer = match write(&brief) {
            Ok(answer) => answer,
            Err(why) => {
                eprintln!(
                    "lamp-voice: a skill made here: the model didn't answer for the agent's task: \
                     {why}"
                );
                return Err(Refused::new(made_skill::MODEL_SILENT));
            }
        };
        let lines = match read(&answer) {
            Ok(Answer::Uncovered(uncovered)) => return Err(uncovered.refused()),
            Ok(Answer::Skill {
                name,
                grants,
                when: When::Asked(asks),
            }) => {
                // What no grant covers first: that is the reason, whenever
                // it runs.
                let grants = match covered(&grants, true) {
                    Ok(grants) => grants,
                    Err(uncovered) => return Err(uncovered.refused()),
                };
                if !crate::upload_agent::on() {
                    return Err(Uncovered::NoWhen.refused());
                }
                // The name the person gave it, its tool's too (#7911).
                let name = name_given(name, named);
                // Its instructions carry the words read back alone: a
                // request longer than them is worded by the model and
                // checked, never cut unheard.
                match task_kept(request, &answer, &name) {
                    Err(lines) => {
                        task_missed = true;
                        lines
                    }
                    Ok(words) => {
                        let made = kept_asked(&asked_files(&name, &asks, &grants, &words), &words);
                        if made.is_ok() {
                            eprintln!(
                                "lamp-voice: a skill made here: the model's cannot made one task \
                                 for the lamp's agent, only asked for ({} grant(s), {} \
                                 phrase(s))",
                                grants.len(),
                                asks.len()
                            );
                        }
                        return made;
                    }
                }
            }
            Ok(Answer::Skill {
                name,
                grants,
                when: When::Trigger(block),
            }) => match block_kept(&block) {
                Ok(block) => {
                    let grants = match covered(&grants, false) {
                        Ok(grants) => grants,
                        Err(uncovered) => return Err(uncovered.refused()),
                    };
                    match task_kept(request, &answer, &name) {
                        Err(lines) => {
                            task_missed = true;
                            lines
                        }
                        Ok(words) => {
                            last = kept(Ok(skill_text(&name, &grants, &block, &words)));
                            match &last {
                                Ok(_) => {
                                    eprintln!(
                                        "lamp-voice: a skill made here: the model's cannot made \
                                         one task for the lamp's agent ({} grant(s))",
                                        grants.len()
                                    );
                                    return last;
                                }
                                Err(Refused(_, Some(lines))) => lines.clone(),
                                Err(_) => return last,
                            }
                        }
                    }
                }
                Err(lines) => lines,
            },
            Err(lines) => lines,
        };
        // A long request's words left out what it names: asked again once
        // with what, then refused as too long, never kept cut.
        if std::mem::take(&mut task_missed) {
            if task_asked_again {
                eprintln!(
                    "lamp-voice: a skill made here: the agent's task's words left out what the \
                     request names again: too long"
                );
                return Err(Uncovered::TooLong.refused());
            }
            task_asked_again = true;
            eprintln!(
                "lamp-voice: a skill made here: the agent's task's words left out what the \
                 request names: asked again"
            );
            last = Err(Uncovered::TooLong.refused());
            brief = repair_brief(request, hint, &lines);
            continue;
        }
        eprintln!(
            "lamp-voice: a skill made here: the agent's task {} of {most}: refused, asked again",
            round + 1
        );
        if last.is_ok() {
            last = Err(Refused::new(made_skill::NOT_A_SKILLS_FORM));
        }
        if last
            .as_ref()
            .is_err_and(|Refused(_, lines)| lines.is_none())
        {
            last = Err(Refused(
                made_skill::NOT_A_SKILLS_FORM.into(),
                Some(lines.clone()),
            ));
        }
        brief = repair_brief(request, hint, &lines);
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grants(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_switch_is_on_only_for_on() {
        assert!(on_with(Some("on")));
        for value in [
            None,
            Some(""),
            Some("1"),
            Some("ON"),
            Some("true"),
            Some("yes"),
        ] {
            assert!(!on_with(value), "{value:?}");
        }
    }

    /// The Reviewer-lead's P2 on #7764: with only quick reactions made by
    /// DeepSeek, the one more call refuses a reminder as the make's own
    /// brief does, and only that `cannot:` is read, as the lamp's own line;
    /// any other is not the form. Off, the prompt is as before.
    #[test]
    fn with_reactions_on_a_reminder_is_refused_as_the_brief_does() {
        made_skill::set_reactions_here(Some(false));
        assert!(!system().contains(made_skill::REMINDER_IS_TIMER));
        made_skill::set_reactions_here(Some(true));
        let told = system();
        assert!(
            told.contains(made_skill::REMINDER_IS_TIMER)
                && told.contains(made_skill::REMINDER_INSTEAD)
                && told.contains(made_skill::CANT_YET),
            "{told}"
        );
        made_skill::set_reactions_here(None);
        let reminder = format!(
            "cannot: I can't do that yet: a reminder at nine. {}",
            made_skill::REMINDER_INSTEAD
        );
        assert_eq!(read(&reminder), Ok(Answer::Uncovered(Uncovered::Reminder)));
        assert_eq!(Uncovered::Reminder.class(), "reminder");
        assert!(said_as_is(&Uncovered::Reminder.line()));
        assert_eq!(
            read("cannot: I can't do that yet: checking the weather."),
            Err(NOT_THE_FORM.to_owned())
        );
    }

    /// sip-of-water's own `[trigger]`, as the lamp ships it.
    fn sip_of_waters_trigger() -> toml::Table {
        let shipped: toml::Table =
            toml::from_str(include_str!("../../../skills/sip-of-water/skill.toml")).unwrap();
        shipped["trigger"].as_table().unwrap().clone()
    }

    /// lamp-9adf, 18 Nov: "remind me to stretch when I've been sitting a
    /// long while" was made on someone getting up. With
    /// `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on, the one more call is told the
    /// sitting as sip-of-water fires on it, its `for`, its once-a-sitting
    /// `rearm` and its `cooldown`, with a whole answer for a sat-a-while
    /// ask, beside the arrival and the leaving; the model still judges by
    /// meaning. Off, the prompt is as before.
    #[test]
    fn with_reactions_on_the_prompt_carries_sip_of_waters_sitting() {
        made_skill::set_reactions_here(Some(false));
        let before = system();
        assert!(!before.contains(SAT_EXAMPLE), "{before}");
        assert!(!before.contains("sip-of-water"), "{before}");
        made_skill::set_reactions_here(Some(true));
        let told = system();
        made_skill::set_reactions_here(None);
        assert!(told.contains(SAT_EXAMPLE), "{told}");
        assert!(told.contains(SAT_EXAMPLE_ASKED), "{told}");
        assert!(told.contains(made_skill::TRIGGER_MEANINGS), "{told}");
        let sip = sip_of_waters_trigger();
        for key in ["for", "rearm", "cooldown"] {
            let line = format!("{key} = \"{}\"", sip[key].as_str().unwrap());
            assert!(told.contains(&line), "{line}: {told}");
        }
        // The example is an answer the lamp keeps, its trigger
        // sip-of-water's sitting on the desk's presence.
        let Ok(Answer::Skill {
            when: When::Trigger(block),
            ..
        }) = read(SAT_EXAMPLE)
        else {
            panic!("{SAT_EXAMPLE}")
        };
        let block: toml::Table = toml::from_str(&block_kept(&block).unwrap()).unwrap();
        let trigger = block["trigger"].as_table().unwrap();
        assert_eq!(trigger["topic"].as_str(), Some("room/presence"));
        assert_eq!(trigger["field"].as_str(), Some("someone"));
        assert_eq!(trigger["equals"].as_bool(), Some(true));
        for key in ["for", "rearm", "cooldown"] {
            assert_eq!(trigger.get(key), sip.get(key), "{key}");
        }
    }

    #[test]
    fn an_answer_is_read_as_a_skill_or_what_it_needs() {
        let answer = format!("{EXAMPLE}\n");
        let Ok(Answer::Skill {
            name,
            grants,
            when: When::Trigger(block),
        }) = read(&answer)
        else {
            panic!("{answer}")
        };
        assert_eq!(name, "morning-joke");
        assert_eq!(grants, ["speak"]);
        assert!(
            block.contains("[trigger]") && !block.contains("```"),
            "{block}"
        );
        assert!(block_kept(&block).is_ok());
        assert_eq!(
            read(ASKED_EXAMPLE),
            Ok(Answer::Skill {
                name: "desk-photo".into(),
                grants: self::grants(&["camera", "speak"]),
                when: When::Asked(vec!["take a desk photo".into()]),
            })
        );
        assert_eq!(
            read(
                "name: test-note\ngrants: gmail-write\nAsks: \"send my test note\" | Send my test note | mail it"
            ),
            Ok(Answer::Skill {
                name: "test-note".into(),
                grants: self::grants(&["gmail-write"]),
                when: When::Asked(vec!["send my test note".into(), "mail it".into()]),
            })
        );
        assert_eq!(
            read("needs: knowing who someone is."),
            Ok(Answer::Uncovered(Uncovered::Missing(
                "knowing who someone is".into()
            )))
        );
        assert_eq!(
            read("needs: other"),
            Ok(Answer::Uncovered(Uncovered::Other))
        );
        assert_eq!(read("needs:"), Ok(Answer::Uncovered(Uncovered::Other)));
        let Ok(Answer::Uncovered(Uncovered::Missing(long))) =
            read(&format!("needs: {}", "a very long thing ".repeat(10)))
        else {
            panic!("a long needs is still what is missing")
        };
        assert!(long.chars().count() <= MOST_MISSING_CHARS, "{long}");
        assert!(read("cannot: no").is_err());
        assert!(read("name: x\ngrants: speak").is_err());
        let none = EXAMPLE.replace("grants: speak", "grants: none");
        assert!(matches!(read(&none), Ok(Answer::Skill { grants, .. }) if grants.is_empty()));
        let bad = EXAMPLE.replace("name: morning-joke", "name: Morning Joke!");
        assert!(matches!(read(&bad), Ok(Answer::Skill { name, .. }) if name == FALLBACK_NAME));
    }

    #[test]
    fn a_block_holds_only_its_topics_and_one_trigger() {
        assert!(block_kept("[trigger]\ntopic = \"room/presence\"\n").is_ok());
        for bad in [
            "",
            "[permissions]\ntopics = [\"clock/local\"]\n",
            "[trigger]\ntopic = \"x\"\n[[do]]\nsay = { text = \"hi\" }\n",
            "grants = [\"camera\"]\n[trigger]\ntopic = \"x\"\n",
            "[permissions]\ncamera = true\n[trigger]\ntopic = \"x\"\n",
            "[trigger\n",
        ] {
            assert!(block_kept(bad).is_err(), "{bad}");
        }
    }

    fn covered_t(asked: &[String]) -> Result<Vec<String>, Uncovered> {
        covered(asked, false)
    }

    #[test]
    fn grants_are_the_ones_there_are_and_what_no_grant_covers_has_its_reason() {
        assert_eq!(
            covered_t(&grants(&["speak", "light", "speak"])),
            Ok(grants(&["speak", "light"]))
        );
        // sim-2 on #7261 and the first builder's ruling: never a grant that
        // listens; on a trigger, never one that watches; only asked for, the
        // camera.
        for listening in LISTENING_GRANTS {
            for only_asked in [false, true] {
                assert_eq!(
                    covered(&grants(&["speak", listening]), only_asked),
                    Err(Uncovered::Listening),
                    "{listening}"
                );
            }
        }
        for watching in WATCHING_GRANTS {
            assert_eq!(
                covered(&grants(&["speak", watching]), false),
                Err(Uncovered::Sensing),
                "{watching}"
            );
        }
        assert_eq!(
            covered(&grants(&["camera", "speak"]), true),
            Ok(grants(&["camera", "speak"]))
        );
        assert_eq!(covered(&grants(&["look"]), true), Err(Uncovered::Other));
        crate::skill_task_yes::set_net_here(Some(false));
        assert_eq!(
            covered_t(&grants(&["speak", "net"])),
            Err(Uncovered::NetOff)
        );
        // On a trigger too, with LAMP_TASK_NET on (the owner's ruling, 17
        // Nov: "check the Bitcoin price every day at 9 and tell me").
        crate::skill_task_yes::set_net_here(Some(true));
        assert_eq!(
            covered_t(&grants(&["net", "speak"])),
            Ok(grants(&["net", "speak"]))
        );
        crate::skill_task_yes::set_net_here(None);
        assert_eq!(covered_t(&grants(&["skills"])), Err(Uncovered::Other));
        assert_eq!(covered_t(&grants(&["weather"])), Err(Uncovered::Other));
        made_skill::set_agent_services_here(Some(false));
        assert_eq!(
            covered_t(&grants(&["gmail-read"])),
            Err(Uncovered::ServicesOff)
        );
        made_skill::set_agent_services_here(Some(true));
        assert_eq!(
            covered_t(&grants(&["zoom-read"])),
            Err(Uncovered::ServiceUnknown)
        );
        assert_eq!(
            covered_t(&grants(&["speak", "gmail-read"])),
            Ok(grants(&["speak", "gmail-read"]))
        );
        // A known service not linked is made all the same: the make's link
        // note asks for it to be linked (lamp-9adf, 16 Nov: Linear).
        assert_eq!(
            covered_t(&grants(&["linear-read"])),
            Ok(grants(&["linear-read"]))
        );
        // Only asked for: the same grants, and the internet with
        // LAMP_TASK_NET on.
        assert_eq!(
            covered(&grants(&["light", "speak"]), true),
            Ok(grants(&["light", "speak"]))
        );
        assert_eq!(covered(&grants(&["notes"]), true), Err(Uncovered::Other));
        crate::skill_task_yes::set_net_here(Some(true));
        assert_eq!(covered(&grants(&["net"]), true), Ok(grants(&["net"])));
        assert_eq!(
            covered(&grants(&["speak", "net"]), true),
            Ok(grants(&["speak", "net"]))
        );
        // #7901, the settled design: nothing is refused beside `net`.
        made_skill::set_agent_services_here(Some(true));
        for other in ["camera", "gmail-read", "gmail-write"] {
            assert_eq!(
                covered(&grants(&[other, "net"]), true),
                Ok(grants(&[other, "net"])),
                "{other}"
            );
            assert_eq!(
                covered(&grants(&["net", "speak", other]), true),
                Ok(grants(&["net", "speak", other])),
                "{other}"
            );
            assert_eq!(covered(&grants(&[other]), true), Ok(grants(&[other])));
        }
        made_skill::set_agent_services_here(None);
        crate::skill_task_yes::set_net_here(Some(false));
        assert_eq!(covered(&grants(&["net"]), true), Err(Uncovered::NetOff));
        crate::skill_task_yes::set_net_here(None);
        made_skill::set_agent_services_here(None);
    }

    /// El's P1 on #7261: a task only asked for tells the agent exactly the
    /// words read back. A request longer than them (~300 characters) is
    /// refused plainly, its unheard tail never in `SKILL.md`; a short one
    /// is made with its words, its trailing stop alone trimmed.
    #[test]
    fn a_task_only_asked_for_carries_the_words_read_back_alone() {
        set_on_here(Some(true));
        made_skill::set_scripts_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        crate::upload_agent::set_on_here(Some(true));
        let tail = "and then email the whole summary to my accountant";
        let long = format!(
            "when I say desk report, look at my desk with the camera, count the cups, the \
             papers, the pens and the plants on it, tell me which of them should go, say \
             how tidy it is from one to ten, and what to put away first, {tail}"
        );
        assert!(long.chars().count() > 250, "{}", long.len());
        assert!(task_cut(&long));
        assert!(!task_words(&long).contains(tail));
        let answer = || Ok("name: desk-report\ngrants: camera, speak\nasks: desk report".into());
        let kept = |_: Result<String, String>| -> Result<Made, Refused> { unreachable!() };
        let made_files = std::cell::RefCell::new(Vec::new());
        let kept_asked = |files: &made_skill::Files, _: &str| {
            made_files.borrow_mut().push(files.scripts.clone());
            Err(Refused::new("kept here"))
        };
        assert_eq!(
            made(&long, |_| answer(), || true, &kept, kept_asked, None, None),
            Err(Uncovered::TooLong.refused())
        );
        assert!(made_files.borrow().is_empty());
        let short = "when I say desk report, tell me how tidy my desk is.";
        assert!(!task_cut(short));
        assert_eq!(
            made(short, |_| answer(), || true, &kept, kept_asked, None, None),
            Err(Refused::new("kept here"))
        );
        let scripts = made_files.borrow();
        let [scripts] = scripts.as_slice() else {
            panic!("{scripts:?}")
        };
        let [(_, instructions)] = scripts.as_slice() else {
            panic!("{scripts:?}")
        };
        assert!(
            instructions.ends_with(
                "what you found or did:\n\nwhen I say desk report, tell me how tidy my desk is\n"
            ),
            "{instructions}"
        );
        crate::upload_agent::set_on_here(None);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_scripts_here(None);
        set_on_here(None);
    }

    /// #7911 (Twelve's HOLD on #7926, item 2): a task only asked for takes
    /// the name the person gave ("make a skill called dollar check that
    /// tells me the rate when I ask"), its card's and its voice tool's;
    /// with none given, or one no made skill may have, the model's name
    /// stands, as before.
    #[test]
    fn a_task_only_asked_for_takes_the_name_the_person_gave() {
        set_on_here(Some(true));
        made_skill::set_scripts_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        crate::upload_agent::set_on_here(Some(true));
        let request = "when I ask for the dollar check, tell me the dollar to dong rate.";
        let answer = || Ok("name: rate-teller\ngrants: speak\nasks: dollar check".into());
        let kept = |_: Result<String, String>| -> Result<Made, Refused> { unreachable!() };
        for (named, name) in [
            (Some("Dollar check"), "dollar-check"),
            (None, "rate-teller"),
            (Some("tỷ giá"), "rate-teller"),
            (Some("ab"), "rate-teller"),
        ] {
            let made_files = std::cell::RefCell::new(Vec::new());
            let kept_asked = |files: &made_skill::Files, _: &str| {
                made_files.borrow_mut().push(files.clone());
                Err(Refused::new("kept here"))
            };
            assert_eq!(
                made(
                    request,
                    |_| answer(),
                    || true,
                    &kept,
                    kept_asked,
                    None,
                    named
                ),
                Err(Refused::new("kept here"))
            );
            let files = made_files.borrow();
            let [files] = files.as_slice() else {
                panic!("{named:?}: {files:?}")
            };
            let tool = format!("tool = \"{}\"", name.replace('-', "_"));
            assert!(files.toml.contains(&tool), "{named:?}: {}", files.toml);
            assert!(
                files.card.starts_with(&format!("---\nname: {name}\n")),
                "{named:?}: {}",
                files.card
            );
        }
        crate::upload_agent::set_on_here(None);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_scripts_here(None);
        set_on_here(None);
    }

    /// A task only asked for is an agent skill as the lamp's cap takes it: a
    /// tool whose note carries the words to say, one task that has the
    /// agent follow the request, and only the grants named.
    #[test]
    fn a_task_only_asked_for_is_an_agent_skill_the_cap_takes() {
        made_skill::set_agent_services_here(Some(true));
        let request = "when I say send my test note, email me the words `hello there`";
        let files = asked_files(
            "test-note",
            &["send my test note".to_owned()],
            &grants(&["gmail-write"]),
            &task_words(request),
        );
        let (_, uses) = crate::upload_agent::capped(&files.toml).unwrap();
        assert!(!uses.camera && !uses.light);
        assert_eq!(
            crate::upload_agent::grants_of_toml(&files.toml),
            ["gmail-write"]
        );
        assert!(
            files.toml.contains("tool = \"test_note\""),
            "{}",
            files.toml
        );
        assert!(files.card.contains("'send my test note'"), "{}", files.card);
        let [(path, instructions)] = files.scripts.as_slice() else {
            panic!("{:?}", files.scripts)
        };
        assert_eq!(path, crate::upload_agent::INSTRUCTIONS);
        assert!(
            instructions.contains("email me the words hello there"),
            "{instructions}"
        );
        assert!(!instructions.contains('`'));
        assert!(
            crate::upload_agent::package_files_ok([
                ("skill.toml", files.toml.as_bytes()),
                ("SKILL.md", files.card.as_bytes()),
                (path.as_str(), instructions.as_bytes()),
            ])
            .is_ok()
        );
        made_skill::set_agent_services_here(None);
    }

    /// The reason is said in plain words, never the bare "I couldn't make
    /// that one.", and journaled by its class alone.
    #[test]
    fn each_reason_is_plain_and_never_the_bare_line() {
        for uncovered in [
            Uncovered::NetOff,
            Uncovered::ServicesOff,
            Uncovered::ServiceUnknown,
            Uncovered::TasksOff,
            Uncovered::Sensing,
            Uncovered::Listening,
            Uncovered::Other,
            Uncovered::Missing("paying for things".into()),
            Uncovered::NoWhen,
            Uncovered::TooLong,
        ] {
            let line = uncovered.line();
            assert_ne!(line, crate::make_skill_agent::CANNOT_LINE);
            assert!(!line.contains("couldn't"), "{line}");
            assert!(said_as_is(&line), "{line}");
            assert!(line.ends_with('.'), "{line}");
            assert_eq!(made_skill::plain_reason(&line), line);
            assert_eq!(made_skill::not_made_line(&line), line);
            assert!(!uncovered.class().contains(' '));
        }
        // Every line the lamp says, whole: what is missing, then what the
        // person can do (the owner, 16 Nov 11:4x).
        for (uncovered, said) in [
            (
                Uncovered::NetOff,
                "I can't make that one yet: it needs the internet, and skills on this lamp \
                 can't use the internet yet.",
            ),
            (
                Uncovered::ServicesOff,
                "I can't make that one yet: it needs your linked services, and skills on this \
                 lamp can't use them yet.",
            ),
            (
                Uncovered::ServiceUnknown,
                "I can't make that one yet: it needs a service the lamp can't link yet.",
            ),
            (
                Uncovered::TasksOff,
                "I can't make that one yet: it needs the lamp's agent, and skills on this lamp \
                 can't hand tasks to it yet.",
            ),
            (
                Uncovered::Sensing,
                "I can't make that one yet: it would need the camera, and a skill that runs by \
                 itself can't watch or listen, to keep the room private. Ask me to make it for \
                 when you say a few words, and it can use the camera then.",
            ),
            (
                Uncovered::Listening,
                "I can't make that one yet: it would need the microphone, and a skill can't \
                 listen, to keep the room private.",
            ),
            (
                Uncovered::Other,
                "I can't make that one yet: it needs something no skill on this lamp can be \
                 given yet.",
            ),
            (
                Uncovered::Missing("paying for things".into()),
                "I can't make that one yet: it needs paying for things, which no skill on this \
                 lamp can be given yet.",
            ),
            (
                Uncovered::TooLong,
                "I can't make that one yet: it's too long for me to read back to you whole, so \
                 ask for it in fewer words.",
            ),
        ] {
            assert_eq!(uncovered.line(), said, "{}", uncovered.class());
        }
    }

    /// The owner's ruling (17 Nov): a task for the lamp's agent on its own
    /// trigger may be granted `net` with `LAMP_TASK_NET` on. The lamp's own
    /// form of it is a made skill's (`made_skill::files`, `capped`) and one
    /// lamp-skills' manage helper puts on (`put_on_cap`, `put_on_scripts`);
    /// `net` is never for code a model wrote, at another path or at the
    /// agent script's; the switch off, no made skill is granted it.
    #[test]
    fn a_timed_task_for_the_agent_is_kept_with_net_only_with_its_switch() {
        let all = |on: Option<bool>| {
            crate::skill_task_yes::set_net_here(on);
            made_skill::set_scripts_here(on);
            crate::skill_task_yes::set_on_here(on);
            crate::upload_agent::set_on_here(on);
            set_on_here(on.map(|_| false));
        };
        all(Some(true));
        let block = EXAMPLE.split_once("```skill.toml\n").unwrap().1;
        let block = block_kept(block.trim_end_matches("```")).unwrap();
        let words = task_words("check the Bitcoin price every day at nine and tell me");
        let kept = covered(&grants(&["net", "speak"]), false).unwrap();
        let text = skill_text("bitcoin-daily", &kept, &block, &words);
        let files = made_skill::files(&text).unwrap();
        let (_, uses) = made_skill::capped(&files.toml).unwrap();
        // On its trigger, never `speak` (#8610).
        assert_eq!(uses.grants, ["net"]);
        assert!(uses.agent_task && uses.code);
        let (_, put) = made_skill::put_on_cap(&files.toml).unwrap();
        let held = |scripts: &[(String, String)]| {
            made_skill::put_on_scripts(
                &files.toml,
                &put,
                scripts
                    .iter()
                    .map(|(path, text)| (path.as_str(), text.as_bytes())),
            )
        };
        assert!(held(&files.scripts).is_ok());
        // A model's own code at the agent script's path: never `net`.
        let own = [(RUN.to_owned(), "import urllib.request\n".to_owned())];
        assert!(held(&own).is_err());
        // Code a model wrote at its own path: `net` is no grant of it.
        let model = files.toml.replace(RUN, "scripts/main.py");
        assert!(made_skill::capped(&model).is_err());
        // The switch off: the grant is refused, and the form too.
        crate::skill_task_yes::set_net_here(Some(false));
        assert_eq!(
            covered(&grants(&["net", "speak"]), false),
            Err(Uncovered::NetOff)
        );
        assert!(made_skill::capped(&files.toml).is_err());
        all(None);
    }

    /// The owner's Bitcoin price (lamp-9adf, 17 Nov): the model's word that
    /// a request needs the internet is one line, no blocks, in any case;
    /// the lamp asks for it only with `LAMP_TASK_NET` and what its task
    /// needs on, and reads the lamp's own task form as one only then too.
    #[test]
    fn the_internet_line_is_one_line_and_asked_for_only_with_the_net_switch() {
        for said in [
            "internet: the Bitcoin price",
            "  Internet: the weather forecast\n",
            "INTERNET:",
        ] {
            assert!(says_needs_net(said), "{said}");
        }
        for said in [
            "cannot: I can't do that yet: checking prices.",
            "It needs the internet: the price",
            "internet: the price\n```skill.toml\n```",
            "internet: the price\n~~~\n~~~",
            "",
        ] {
            assert!(!says_needs_net(said), "{said}");
        }
        let all = |on: Option<bool>| {
            crate::skill_task_yes::set_net_here(on);
            made_skill::set_scripts_here(on);
            crate::skill_task_yes::set_on_here(on);
            crate::upload_agent::set_on_here(on);
        };
        all(Some(true));
        set_on_here(Some(false));
        assert!(net_by_agent() && agent_tasks_made());
        for off in 0..4 {
            all(Some(true));
            match off {
                0 => crate::skill_task_yes::set_net_here(Some(false)),
                1 => made_skill::set_scripts_here(Some(false)),
                2 => crate::skill_task_yes::set_on_here(Some(false)),
                _ => crate::upload_agent::set_on_here(Some(false)),
            }
            assert!(!net_by_agent(), "{off}");
            assert!(!agent_tasks_made(), "{off}");
        }
        set_on_here(Some(true));
        assert!(agent_tasks_made());
        set_on_here(None);
        all(None);
    }

    #[test]
    fn the_task_is_the_request_in_its_own_words_bounded() {
        assert_eq!(
            task_words("  every morning at eight,\ntell me a \"joke\".  "),
            "every morning at eight, tell me a 'joke'"
        );
        let long = "check the plant ".repeat(20);
        let words = task_words(&long);
        assert!(
            words.chars().count() <= made_skill::MOST_TASK_CHARS,
            "{words}"
        );
        assert!(words.ends_with("plant") || words.ends_with("check") || words.ends_with("the"));
        assert!(!task_words("a\\b\u{200B}c").contains(['\\', '\u{200B}']));
    }

    /// lamp-9adf, 19 Nov (build ebbc6161): the evening rate's task was its
    /// request cut at 120 characters, so the agent never learned "more
    /// than half a percent since the day before".
    const EVENING_RATE: &str = "Lamp, make a skill called evening rate: every night at nine, \
                                tell me the dollar to dong rate, but only if it moved more than \
                                half a percent since the day before.";

    /// The model's answer for the evening rate, its `task:` line `task`.
    fn evening_answer(task: &str) -> String {
        let block = EXAMPLE
            .split_once("```skill.toml")
            .unwrap()
            .1
            .replace("08:00", "21:00")
            .replace("08:01", "21:01");
        format!("name: evening-rate\ngrants: speak\ntask: {task}\n```skill.toml{block}")
    }

    /// Every switch a task for the lamp's agent needs, on (or back to its
    /// default with `None`).
    fn all_on(on: Option<bool>) {
        set_on_here(on);
        made_skill::set_scripts_here(on);
        crate::skill_task_yes::set_on_here(on);
        crate::upload_agent::set_on_here(on);
    }

    /// The task's words of the skills `made` kept, and the briefs it sent,
    /// for `request` with the model's `task:` line `task` each time.
    fn evening_made(
        request: &str,
        task: &str,
    ) -> (Result<Made, Refused>, Vec<String>, Vec<String>) {
        let briefs = std::cell::RefCell::new(Vec::new());
        let texts = std::cell::RefCell::new(Vec::new());
        let kept = |text: Result<String, String>| -> Result<Made, Refused> {
            texts.borrow_mut().push(text.unwrap());
            Err(Refused::new("kept here"))
        };
        let kept_asked =
            |_: &made_skill::Files, _: &str| -> Result<Made, Refused> { unreachable!() };
        let result = made(
            request,
            |brief| {
                briefs.borrow_mut().push(brief.to_owned());
                Ok(evening_answer(task))
            },
            || true,
            &kept,
            kept_asked,
            None,
            None,
        );
        let words = texts
            .into_inner()
            .iter()
            .map(|text| {
                let at = text.find("task = { task = \"").unwrap() + "task = { task = \"".len();
                text[at..].split_once('"').unwrap().0.to_owned()
            })
            .collect();
        (result, words, briefs.into_inner())
    }

    /// A request longer than its read-back: the task's words are the
    /// making model's own sentence, at most 120 characters, kept because
    /// every number, time, threshold and named thing in the request
    /// survives in it; never the request cut short.
    #[test]
    fn a_long_requests_task_keeps_every_condition_in_the_models_words() {
        all_on(Some(true));
        assert!(task_cut(EVENING_RATE), "{}", EVENING_RATE.len());
        let task = "Every night at nine, tell me the dollar to dong rate if it moved over half a \
                    percent since the day before";
        let (result, words, briefs) = evening_made(EVENING_RATE, task);
        assert_eq!(result, Err(Refused::new("kept here")));
        let [words] = words.as_slice() else {
            panic!("{words:?}")
        };
        assert!(
            words.contains("half a percent") || words.contains("0.5"),
            "{words}"
        );
        assert!(words.contains("day before"), "{words}");
        assert!(
            words.chars().count() <= made_skill::MOST_TASK_CHARS,
            "{words}"
        );
        assert!(briefs[0].contains("task:"), "{}", briefs[0]);
        // Only asked for, the same: made with the model's words.
        let asked = "when I say evening rate, tell me the dollar to dong rate, but only if it \
                     moved more than half a percent since the day before, and say by how much";
        assert!(task_cut(asked));
        let said = "Tell me the dollar to dong rate only if it moved over half a percent since the \
                    day before, and by how much";
        let made_words = std::cell::RefCell::new(Vec::new());
        let kept = |_: Result<String, String>| -> Result<Made, Refused> { unreachable!() };
        let kept_asked = |files: &made_skill::Files, words: &str| {
            assert!(files.scripts[0].1.contains(words), "{:?}", files.scripts);
            made_words.borrow_mut().push(words.to_owned());
            Err(Refused::new("kept here"))
        };
        let answer = format!("name: evening-rate\ngrants: speak\ntask: {said}\nasks: evening rate");
        assert_eq!(
            made(
                asked,
                |_| Ok(answer.clone()),
                || true,
                &kept,
                kept_asked,
                None,
                None
            ),
            Err(Refused::new("kept here"))
        );
        assert_eq!(made_words.into_inner(), [said]);
        all_on(None);
    }

    /// A model whose words drop the threshold is asked again once, told
    /// what it left out; dropped again, the make is refused as too long,
    /// never kept cut.
    #[test]
    fn a_task_that_drops_a_condition_is_asked_again_then_refused() {
        all_on(Some(true));
        let task = "Every night at nine, tell me the dollar to dong rate";
        let (made, words, briefs) = evening_made(EVENING_RATE, task);
        assert_eq!(made, Err(Uncovered::TooLong.refused()));
        assert!(words.is_empty(), "{words:?}");
        let [_, again] = briefs.as_slice() else {
            panic!("{briefs:?}")
        };
        assert!(
            again.contains("half") && again.contains("percent"),
            "{again}"
        );
        // No task: line at all, the same.
        let (made, words, briefs) = evening_made(EVENING_RATE, "");
        assert_eq!(made, Err(Uncovered::TooLong.refused()));
        assert!(
            words.is_empty() && briefs.len() == 2,
            "{words:?} {briefs:?}"
        );
        all_on(None);
    }

    /// A request that fits stays the task's words as it is, whatever the
    /// model writes, and its brief asks for no words.
    #[test]
    fn a_short_requests_task_stays_its_own_words() {
        all_on(Some(true));
        let short = "every night at nine, tell me the dollar to dong rate";
        assert!(!task_cut(short));
        let (made, words, briefs) = evening_made(short, "Tell me something else");
        assert_eq!(made, Err(Refused::new("kept here")));
        assert_eq!(words, [task_words(short)]);
        assert!(!briefs[0].contains("task:"), "{}", briefs[0]);
        all_on(None);
    }

    /// The check of a long request's task words: a number in words or
    /// digits is the same, a name and a time word stay, the skill's own
    /// name need not.
    #[test]
    fn a_tasks_words_keep_what_the_request_names() {
        let none: [&str; 0] = [];
        let request = EVENING_RATE;
        for kept in [
            "Every night at 9, tell me the dollar to dong rate if it moved over 0.5% since the day \
             before",
            "At nine each night say the dollar to dong rate when it moved more than half a \
             percent since the day before",
        ] {
            assert_eq!(left_out(request, kept, "evening-rate"), none, "{kept}");
        }
        assert_eq!(
            left_out(
                request,
                "Every night at ten, tell me the dollar to dong rate if it moved",
                "evening-rate"
            ),
            [
                "nine",
                "half",
                "since",
                "before",
                "its comparison (more than, less than, ...)",
                "percent"
            ]
        );
        let named = "Every Monday at twenty five past nine, read me the Bitcoin price from the \
                     news and tell me if it is above 1,000 dollars";
        assert_eq!(
            left_out(
                named,
                "Monday at 9:25, tell me if the Bitcoin price is above 1000 dollars",
                "x"
            ),
            none
        );
        assert_eq!(
            left_out(
                named,
                "Each week at 9:25 say if the price is over 1000",
                "x"
            ),
            ["monday", "bitcoin"]
        );
        assert_eq!(
            said_task("name: x\n```skill.toml\ntask: no\n```\nTask: yes"),
            Some(" yes")
        );
    }

    #[test]
    fn the_system_prompt_names_every_grant_and_the_two_one_line_answers() {
        let system = system();
        for name in FALLBACK_GRANTS {
            assert!(system.contains(&format!("{name} (")), "{name}");
        }
        // The camera only for a task asked for; never what listens.
        assert!(system.contains("camera (the camera, only for a skill that runs when"));
        for name in LISTENING_GRANTS {
            assert!(!system.contains(&format!("{name} (")), "{name}");
        }
        for part in [
            "net (the internet",
            "CODE-read",
            "asks:",
            "needs: and what",
            ASKED_EXAMPLE,
            "never write the task or any code",
            made_skill::TRIGGER_MEANINGS,
            EXAMPLE,
        ] {
            assert!(system.contains(part), "{part}");
        }
    }

    /// The first builder's 19:50 ruling on #6811 (the middle form): with
    /// `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on, a task the lamp writes on a
    /// trigger carries sip-of-water's three guards and still only its one
    /// task step, no say; its cap takes it. Never on the lamp's clock
    /// (Twelve's P1 on #7764, the first builder's 19 Nov 01:29 and 02:41):
    /// "tell me the BTC price at 9" runs at 9 whatever the room does. Off,
    /// as before, byte for byte.
    #[test]
    fn with_the_reactions_switch_a_task_on_a_trigger_keeps_the_guards_but_never_on_a_clock() {
        crate::skill_task_yes::set_net_here(Some(true));
        made_skill::set_scripts_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        crate::upload_agent::set_on_here(Some(true));
        set_on_here(Some(false));
        let clock = EXAMPLE.split_once("```skill.toml\n").unwrap().1;
        let clock = block_kept(clock.trim_end_matches("```")).unwrap();
        assert!(on_the_clock(&clock), "{clock}");
        let words = task_words("tell me the BTC price at 9");
        let kept = covered(&grants(&["net", "speak"]), false).unwrap();
        made_skill::set_reactions_here(Some(true));
        let timed = skill_text("btc-price", &kept, &clock, &words);
        assert!(!timed.contains("[[while]]"), "{timed}");
        made_skill::set_reactions_here(Some(false));
        assert_eq!(skill_text("btc-price", &kept, &clock, &words), timed);
        let block = "[permissions]\ntopics = [\"room/presence\"]\n\n[trigger]\n\
                     topic = \"room/presence\"\nfield = \"someone\"\nequals = true\n\
                     for = \"30m\"\nrearm = \"leaves\"";
        assert!(!on_the_clock(block));
        let before = skill_text("btc-price", &kept, block, &words);
        assert!(!before.contains("[[while]]"), "{before}");
        made_skill::set_reactions_here(Some(true));
        let text = skill_text("btc-price", &kept, block, &words);
        let files = made_skill::files(&text).unwrap();
        let table: toml::Table = toml::from_str(&files.toml).unwrap();
        assert_eq!(table["while"].as_array().map(Vec::len), Some(3), "{text}");
        let steps = table["do"].as_array().unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0]["task"]["task"].as_str(), Some(words.as_str()));
        assert!(!files.toml.contains("say"), "{text}");
        let guards = format!("{}\n\n", made_skill::template_guards());
        assert_eq!(text.replace(&guards, ""), before, "only the guards added");
        let paced = made_skill::paced(&files.toml);
        let capped = made_skill::capped(&paced);
        assert!(capped.is_ok(), "{capped:?}");
        made_skill::set_reactions_here(None);
        crate::skill_task_yes::set_net_here(None);
        made_skill::set_scripts_here(None);
        crate::skill_task_yes::set_on_here(None);
        crate::upload_agent::set_on_here(None);
        set_on_here(None);
    }
}
