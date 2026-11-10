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
/// The topics a made skill may read: whether the voice is talking,
/// whether someone is at the lamp, whether a face is in view and someone
/// stood up from the desk (`lamp_skills::PRESENCE_TOPICS`, presence alone:
/// its trigger reads no field of the faces, so never who), and the local
/// clock (`lamp_skills::LOCAL_CLOCK_TOPIC`, which lamp-skills makes itself).
/// Nothing it reads is about a person's words, identity or picture.
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
/// The most of a description said: what it does, and what was left out.
const MOST_DESCRIPTION_CHARS: usize = 400;
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
    let request = request.split_whitespace().collect::<Vec<_>>().join(" ");
    if request.is_empty() {
        return Err("Say what the skill should do.");
    }
    if request.len() > MOST_REQUEST_BYTES {
        return Err("That is too long to ask for as one skill: say it more shortly.");
    }
    let read = if reads { " Or one read, as above." } else { "" };
    Ok(format!(
        "{BRIEF_HEAD} for the request. Reply with two fenced blocks: \
         ```skill.toml, ```SKILL.md. skill.toml: \
         kind = \"installable\", version = \"1.0.0\", no name key; [permissions] \
         light/body/speak = true as used, its topic; one [trigger] as listed (coming to \
         the desk is room/presence), with for, cooldown, rearm = \"ends\", \
         during = {{ from = \"22:00\", to = \"22:30\" }}; \
         [ask] policy = \"nudge\"; [[do]]: light = {{ colour, level 1-5, for }} or \
         {{ dim = 1 }}, body = {{ gesture = \"nod\" }} or one listed, say = {{ text = \
         \"Hi!\" }} (exact words, asker's language) or cue = \"hum\".{read} SKILL.md: \
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

/// Why a made skill is not kept, in plain words the lamp can say; and when
/// the lamp's check refused it, the check's lines for a repair
/// ([`check_lines`]), never said.
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

/// What the lamp says of a skill it couldn't make, by voice or from the
/// app: "I couldn't make that:" and why as it is, ending as a sentence
/// (#5977). Every reason the lamp gives says what to try, or is the
/// model's own `cannot:` sentence with what to ask instead.
pub fn not_made_line(why: &str) -> String {
    let why = why.trim();
    if why.ends_with(['.', '!', '?']) {
        format!("{NOT_MADE_HEAD} {why}")
    } else {
        format!("{NOT_MADE_HEAD} {why}.")
    }
}

/// How [`not_made_line`] begins.
pub const NOT_MADE_HEAD: &str = "I couldn't make that:";

/// The two files of the model's answer: exactly one fenced block named
/// `skill.toml` and one named `SKILL.md` (its info string's last word),
/// and no other fenced block; each at most [`MOST_FILE_BYTES`].
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
    let mut left_out = None;
    let mut lines = answer.lines();
    while let Some(line) = lines.next() {
        let Some(info) = line.trim_start().strip_prefix("```") else {
            if let Some(what) = after_word(line.trim_start(), LEFT_OUT)
                && !what.trim().is_empty()
            {
                left_out = Some(cut_chars(what, MOST_CANNOT_CHARS));
            }
            continue;
        };
        let name = info.split_whitespace().last().unwrap_or_default();
        let mut body = String::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line.trim_start().starts_with("```") {
                closed = true;
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
        if !closed {
            return Err(Refused::new(MODEL_CUT_OFF));
        }
        if body.len() > MOST_FILE_BYTES {
            return Err(Refused::new(
                "the lamp's model wrote more than a skill can hold; try asking for something \
                 simpler",
            ));
        }
        let slot = match name {
            "skill.toml" => &mut toml,
            "SKILL.md" => &mut card,
            _ => return Err(not_a_skills_form("a file a made skill can't have")),
        };
        if slot.replace(body).is_some() {
            return Err(not_a_skills_form("one of its files twice"));
        }
    }
    match (toml, card) {
        (Some(toml), Some(card)) => Ok(Files {
            toml,
            card,
            left_out,
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
    pub reads: Option<String>,
}

impl Uses {
    /// In plain words: "its light and its head", "whether someone is at the
    /// desk".
    pub fn spoken(&self) -> String {
        let mut parts = Vec::new();
        if self.speak {
            parts.push("its voice".to_owned());
        }
        if let Some(service) = &self.reads {
            parts.push(format!("reading your {}", service_name(service)));
        }
        if self.light {
            parts.push("its light".to_owned());
        }
        if self.body {
            parts.push("its head and arm".to_owned());
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

    fn json(&self) -> Value {
        let mut uses = json!({"light": self.light, "body": self.body, "topics": self.topics, "speak": self.speak});
        if let Some(service) = &self.reads {
            uses["reads"] = json!(service);
        }
        uses
    }
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
    let Some(code) = uses.reads.as_deref() else {
        return Some(Vec::new());
    };
    let linked = linked()?;
    let alike = |linked: &str| {
        linked == code
            || matches!(
                (code, linked),
                ("gmail", "gmail-app") | ("gmail-app", "gmail")
            )
    };
    if linked.iter().any(|linked| alike(linked)) {
        return Some(Vec::new());
    }
    Some(vec![json!({"code": code, "name": service_name(code)})])
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

/// A made skill's `skill.toml` held to what one may use (the coordinator's
/// cap, 19 Oct): installable, with a version; `[permissions]` with only
/// `light`, `body` and `topics` among [`TOPICS`]; no function for the
/// model (`tool`). Speaking, messages, the look and every other topic are
/// refused; widening this is a decision still to be taken. The one widening
/// so far (#5288): with [`READS_ENVIRONMENT`] on, `services` naming one
/// linked service with reads (`lamp_services::spoken::not_readable`) as
/// `"read"`, with `speak` for saying it. What the skill
/// does within it is checked by `lamp-skills pack` after.
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
    held.map_err(|Refused(why, _)| {
        let read = if reads { CAPPED_OR_READ } else { "" };
        let lines = format!("The lamp refused it: {why}. {CAPPED_ONLY}{read}");
        Refused(why, Some(lines))
    })
}

/// What a repair is told when [`capped`] refused.
const CAPPED_ONLY: &str = "kind = \"installable\", version = \"1.0.0\", no [tool]; \
                           [permissions] only light, body and speak = true as its steps use \
                           them (speak for a say step or cue = \"hum\", its one sound), topics \
                           only its trigger's; [ask] policy = \"nudge\"; a say is one \
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
/// as it came.
pub fn paced(text: &str) -> String {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return text.to_owned();
    };
    let Some(trigger) = table.get("trigger").and_then(|trigger| trigger.as_table()) else {
        return text.to_owned();
    };
    let topic = trigger.get("topic").and_then(|topic| topic.as_str());
    let says = says_aloud(&table);
    let mut set: Vec<(&str, String)> = Vec::new();
    let least = if says {
        Some((LEAST_SAY_COOLDOWN_SECS, "1h"))
    } else if topic == Some(STOOD_UP_TOPIC) {
        Some((LEAST_STOOD_UP_COOLDOWN_SECS, "1m"))
    } else {
        None
    };
    if let Some((least, line)) = least
        && cooldown_secs(trigger).is_none_or(|secs| secs < least)
    {
        set.push(("cooldown", format!("cooldown = \"{line}\"")));
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
    if set.is_empty() {
        return text.to_owned();
    }
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let Some(head) = lines.iter().position(|line| line.trim() == "[trigger]") else {
        return text.to_owned();
    };
    let end = lines[head + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |at| head + 1 + at);
    let key_of = |line: &str| line.split_once('=').map(|(key, _)| key.trim().to_owned());
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
const SOUND_NOT_SHIPPED: &str = "it would play a sound the lamp doesn't have; the one sound a \
                                 made skill plays is its soft hum";

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
    // Asked of the Policy as a nudge, never as a hello, news or a reminder
    // (the reviewer on #2011): the act `lamp-skills` passes on.
    if let Some(ask) = table.get("ask")
        && ask.get("policy").and_then(|policy| policy.as_str()) != Some("nudge")
    {
        return Err(Refused::new(
            "it would start in a way a made skill can't: a made skill only offers itself as a \
             gentle nudge",
        ));
    }
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
                ("speak", toml::Value::Boolean(_)) if added => {}
                // Its voice: a read of a linked service said aloud
                // (#5288), or its own `say` step (#6055), held below to a
                // skill that has one.
                ("speak", toml::Value::Boolean(on)) => uses.speak = *on,
                // One linked service, read and never written (#5288).
                ("services", toml::Value::Table(services)) if !added && reads => {
                    let read = match services.iter().collect::<Vec<_>>().as_slice() {
                        [(service, how)]
                            if lamp_services::spoken::not_readable(service).is_none()
                                && how.as_str() == Some("read") =>
                        {
                            service.to_string()
                        }
                        _ => {
                            return Err(Refused::new(
                                "it asks to use linked services other than by reading one that has something to read, which a made skill can't",
                            ));
                        }
                    };
                    uses.reads = Some(read);
                }
                ("speak" | "look" | "messages" | "known_faces" | "actions" | "services", _) => {
                    return Err(Refused::new(
                        "it asks to use more than the light and the head, which a made skill can't",
                    ));
                }
                _ => return Err(Refused::new("the lamp couldn't tell what it would use")),
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
        // Its sound only the lamp's own, by name ([`SOUNDS`]).
        let cues: Vec<&toml::Value> = steps().filter_map(|step| step.get("cue")).collect();
        if cues.iter().any(|cue| sound_plainly(cue).is_none())
            || steps().any(|step| step.get("sound").is_some())
        {
            return Err(Refused::new(SOUND_NOT_SHIPPED));
        }
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
        if uses.speak && says.is_empty() && cues.is_empty() && uses.reads.is_none() {
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
            if cooldown_secs(trigger).is_none_or(|secs| secs < LEAST_SAY_COOLDOWN_SECS) {
                return Err(Refused::new(
                    "it would speak more than once an hour; a made skill that speaks has \
                     cooldown = \"1h\" or longer",
                ));
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
    let table: toml::Table = toml::from_str(text).map_err(unreadable)?;
    said_plainly(&table, reads).map_err(|unsaid| {
        // The part's keys and topic, never a value nor the model's words.
        eprintln!(
            "lamp-voice: a made skill can't be said plainly: {}",
            unsaid.part
        );
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
}

impl Unsaid {
    fn of(part: impl Into<String>) -> Self {
        Self {
            part: part.into(),
            trigger: false,
        }
    }

    fn trigger(part: impl Into<String>) -> Self {
        Self {
            part: part.into(),
            trigger: true,
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
            "{UNSAID_PART}{}. {fix}",
            cut_bytes(&self.part, MOST_UNSAID_PART_BYTES)
        )
    }
}

/// What the person is told when [`plainly`] refused, the app's note and
/// the lamp's line: plain words, and what to try (the owner, 10 Nov: "when
/// making fails I hear no reason from the lamp, nor in the app").
pub const UNSAID: &str = "when it would run and what it would do didn't come out in plain \
                          words; try saying when it should run, like 'when I come to my desk', \
                          and what it should do, like 'glow green'";

/// What a repair is told after a say step [`plainly`] couldn't say.
const SAY_TEXT_PART: &str =
    ": say = { text = \"the exact words\" }, one or two short sentences, no quotes";

/// What a repair is told after a sound step [`plainly`] couldn't say.
const SOUND_PART: &str = ": its one sound is cue = \"hum\"";

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

/// [`plainly_with`]'s words, or the part it couldn't say.
fn said_plainly(table: &toml::Table, reads: bool) -> Result<String, Unsaid> {
    // A skill written in the app ([`Written`]): a function whose answer is
    // the person's own instructions, said as that and nothing more. Only its
    // exact shape; another with a function, added from a file, is said below
    // (Twelve's P2 on #5072).
    if let Some(said) = written_plainly(table) {
        return Ok(said);
    }
    // Only what is said below, or changes nothing that runs: a `[[while]]`,
    // a `[quiet]`, a budget or any other key would run otherwise than the
    // words (Thirteen's P2 on #5042).
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
    ];
    let extras: Vec<&String> = table
        .keys()
        .filter(|key| !parts.contains(&key.as_str()))
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
    let unsaid_step = |index: usize, step: &toml::Value| {
        let say = match (step.get("say"), step.get("cue").or(step.get("sound"))) {
            (Some(_), _) => SAY_TEXT_PART,
            (None, Some(_)) => SOUND_PART,
            (None, None) => "",
        };
        Unsaid::of(format!(
            "[[do]] step {}, {}{say}",
            index + 1,
            step_named(step)
        ))
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
            "Only when you ask for it, it {}, and never on its own.",
            steps.join(", then ")
        );
        if said.chars().count() > MOST_PLAIN_CHARS {
            return Err(too_long());
        }
        return Ok(said);
    }
    // The trigger first, any part beside it named with it: a skill shaped
    // like a built-in one (a [[trigger]] on the faces, a [[while]]) is told
    // both in its one repair.
    let (mut said, trigger) = match trigger_plainly(table.get("trigger")) {
        Ok(said) => said,
        Err(part) => {
            return Err(Unsaid::trigger(match extras {
                Some(extras) => format!("{part}; {extras}"),
                None => part,
            }));
        }
    };
    if let Some(extras) = extras {
        return Err(Unsaid::of(extras));
    }
    let steps = steps_of(table)?;
    // A read of a linked service (#5288): its skill's one step, said with
    // the gate it waits for.
    if let [step] = steps.as_slice()
        && let Some(read) = read_plainly(step).filter(|_| reads)
    {
        said.push_str(&format!(", when you're alone with the lamp, it {read}"));
    } else {
        let steps = steps
            .iter()
            .enumerate()
            .map(|(index, step)| step_plainly(step).ok_or_else(|| unsaid_step(index, step)))
            .collect::<Result<Vec<_>, _>>()?;
        said.push_str(&format!(", it {}", steps.join(", then ")));
    }
    if let Some(cooldown) = trigger.get("cooldown") {
        let (count, unit) = cooldown
            .as_str()
            .and_then(duration)
            .ok_or_else(|| Unsaid::trigger(format!("{}, its cooldown", trigger_named(trigger))))?;
        said.push_str(&match count {
            1 => format!(", at most once {}", one(unit)),
            _ => format!(", at most once every {count} {unit}s"),
        });
    }
    said.push('.');
    // Said whole or not at all: never cut before a step or the cooldown.
    if said.chars().count() > MOST_PLAIN_CHARS {
        return Err(too_long());
    }
    Ok(said)
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
/// Six triggers are said: someone comes to the desk (or comes back, after
/// it was empty a while), the desk is left, the lamp goes quiet, a face
/// comes into view (after none, or none for a while), someone at the desk
/// gets up, and a time of day, each day; each with its `for`, `rearm`,
/// `during` and `cooldown` (said after the steps) as lamp-skills counts
/// them for that topic.
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
        Some(lasting) => Some(lasting.as_str().and_then(spell).ok_or_else(|| its("for"))?),
        None => None,
    };
    // `when = "appears"`: once each time it comes to hold, as
    // `rearm = "ends"` runs it, so said in the same words; and only with
    // it does `lamp-skills` count `absent_for`, the while it didn't hold
    // before (`lamp_skills::Trigger::absent_for_us`).
    let appears = match trigger.get("when").map(toml::Value::as_str) {
        None | Some(Some("present")) => false,
        Some(Some("appears")) => true,
        Some(_) => return Err(its("when")),
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
    if trigger.contains_key("above") && topic.0 != Some(STOOD_UP_TOPIC) {
        return Err(its("above"));
    }
    let mut said = match (lasting, absent) {
        // Each day at a time (lamp-skills' own clock, `clock/local`): the
        // built-in daily triggers' shape, known local time held once
        // (`rearm`) in a window of one minute, so it runs once at that
        // minute; never a window's while, nor a week's day (lamp-skills
        // has none).
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
            return Ok((format!("Every day at {at}"), trigger));
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
            let above = trigger.get("above").and_then(|line| match line {
                toml::Value::Integer(line) => Some(*line as f64),
                toml::Value::Float(line) => Some(*line),
                _ => None,
            });
            if topic.1 != Some("sitting_s") || topic.2.is_some() || above != Some(0.0) {
                return Err(its("field and above: sitting_s above 0"));
            }
            "When someone sitting at the desk gets up".to_owned()
        }
        (_, _) if topic.0 == Some(STOOD_UP_TOPIC) => {
            return Err(its("for or absent_for, which a getting up never lasts"));
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

/// The longest [`plainly`] says: one breath, and with [`left_out`]'s
/// words still within [`MOST_DESCRIPTION_CHARS`], so nothing is cut.
const MOST_PLAIN_CHARS: usize = 200;

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
const PLAIN_ONLY: &str = "Write only kind, version, needs, permissions, one [trigger], [ask] \
                          and [[do]], and at most a few [[do]] steps, each one light, one \
                          listed body gesture, one say = { text = \"the exact words\" } or \
                          cue = \"hum\" (with speak = true), as the request names them";

/// What a repair is told when [`plainly`] refused the trigger: the ones it
/// says, each with what it means, so a request to "come back to my desk"
/// is written on the desk's presence, not on the faces as the built-in
/// welcome back's is (lamp-74ff, 10 Nov 17:39). `make_skill_local`'s
/// system prompt tells the making model the same before it writes.
pub const SAYABLE_TRIGGERS: &str = concat!(
    "One [trigger] table, one of: ",
    trigger_meanings!(),
    " Beside it only for, cooldown, rearm = \"ends\", during."
);

/// The triggers [`plainly`] says, each with what it means, for the making
/// model's system prompt (`make_skill_local`).
pub const TRIGGER_MEANINGS: &str = trigger_meanings!();

/// What [`PLAIN_ONLY`] goes on to say with reads on: a skill that reads a
/// linked service keeps its one read step. On lamp-9adf, 5 Nov 13:26, an
/// app make of "Read me my latest email when I sit down at my desk." was
/// refused twice: the repair, told only lights and gestures, took the
/// mail read away.
const PLAIN_OR_READ: &str = "; or, for a read of a linked service, only one [[do]] step: that \
                             service's read step exactly as your instructions list it, and \
                             nothing else";

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
    if let Some(light) = step.get("light").and_then(|light| light.as_table()) {
        if let Some(dim) = light.get("dim") {
            return match (light.len(), dim.as_integer()?) {
                (1, 1) => Some("dims the light a step".to_owned()),
                (1, steps @ 2..) => Some(format!("dims the light {steps} steps")),
                _ => None,
            };
        }
        let known = ["colour", "level", "for"];
        if light.keys().any(|key| !known.contains(&key.as_str())) {
            return None;
        }
        let mut said = match light.get("colour") {
            Some(colour) => format!("glows {}", colour.as_str()?),
            None => "turns the light".to_owned(),
        };
        if let Some(level) = light.get("level") {
            said.push_str(&format!(" at level {}", level.as_integer()?));
        }
        if let Some(lasting) = light.get("for") {
            said.push_str(&format!(" for {}", spell(lasting.as_str()?)?));
        }
        return Some(said);
    }
    let body = step.get("body").and_then(|body| body.as_table())?;
    match (
        body.len(),
        body.get("gesture").and_then(|gesture| gesture.as_str()),
    ) {
        (1, Some(gesture)) => gesture_said(gesture).map(str::to_owned),
        _ => None,
    }
}

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
    card_marked(
        text,
        description,
        &format!("made: here\n{MADE_BY}: {}", maker.key()),
    )
}

/// [`card`] with `marks`, the front matter's last lines: `made: here` and
/// its maker, and for a skill added from a file [`ADDED_FROM_FILE`] too.
/// Any such line the file had is taken out first.
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
            .is_some_and(|(key, _)| matches!(key.trim(), "made" | MADE_BY | "added"));
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
/// A name any skill on the lamp has (in `installed`, or pushed beside it)
/// is refused; a kept one of that name not on is replaced. Kept once:
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
        Refused::new("the lamp couldn't keep it")
    };
    ensure_private(&keeping.directory).map_err(unkept)?;
    drop_unfinished(&keeping.directory);
    if let Some(made) = kept(&keeping.directory, from) {
        return Ok(made);
    }
    let name = &written.name;
    if installed.join(name).exists() || pushed(installed).join(name).exists() {
        return Err(Refused::new(format!(
            "a skill called {name} is already on the lamp or kept"
        )));
    }
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
fn pushed(installed: &Path) -> PathBuf {
    installed.with_file_name("skills-dev")
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
    Ok(())
}

/// Where made skills are kept, the program that checks them, and the
/// manage helper's socket that puts a kept one on ([`put_on`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keeping {
    pub directory: PathBuf,
    pub program: PathBuf,
    pub manage: PathBuf,
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
    let unkept = |why: io::Error| {
        eprintln!("lamp-voice: a made skill was not kept: {why}");
        Refused::new("the lamp couldn't keep it")
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
    // A say's pace set, never refused (#6062): the text checked, said and
    // packed is the paced one.
    files.toml = paced(&files.toml);
    let (version, uses) = capped(&files.toml)?;
    // Said only once the check has passed: its lines, which name the
    // field, go to a repair before these do.
    let said = plainly(&files.toml).map(|does| match &files.left_out {
        Some(what) => format!("{does} {}", left_out(what)),
        None => does,
    });
    let (name, marked) = card(
        &files.card,
        said.as_deref().unwrap_or_default(),
        Maker::of(from),
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
        },
        said,
        false,
    )
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
        Refused::new("the lamp couldn't keep it")
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
        Refused::new("the lamp couldn't keep it")
    };
    let Card {
        toml,
        name,
        marked,
        version,
        uses,
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
            description: cut_chars(&description, MOST_DESCRIPTION_CHARS),
            uses,
            package: kept_as,
        };
        remember(&keeping.directory, from, &made, added).map_err(unkept)?;
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
    let said = out.with_file_name("check.err");
    let stderr = fs::File::create(&said).map_err(|why| {
        eprintln!("lamp-voice: a made skill was not checked: {why}");
        Refused::new("the lamp couldn't check it")
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
            Refused::new("the lamp couldn't check it")
        })?;
    let began = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if began.elapsed() < PACK_WITHIN => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Refused::new("the lamp couldn't check it in time"));
            }
        }
    };
    if status.success() {
        return Ok(());
    }
    let mut words = Vec::new();
    if let Ok(file) = fs::File::open(&said) {
        let _ = file.take(MOST_CHECK_BYTES).read_to_end(&mut words);
    }
    eprintln!(
        "lamp-voice: a made skill did not pass lamp-skills' check (exit {}): {}",
        status
            .code()
            .map_or("none".to_owned(), |code| code.to_string()),
        check_reason(&String::from_utf8_lossy(&words), folder, name)
    );
    Err(Refused(
        NOT_PASSED.into(),
        Some(check_lines(&String::from_utf8_lossy(&words), folder)),
    ))
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
        .and_then(|entry| {
            let package = directory.join(entry["package"].as_str()?);
            package.is_file().then(|| Made {
                name: entry["name"].as_str().unwrap_or_default().to_owned(),
                version: entry["version"].as_str().unwrap_or_default().to_owned(),
                sha256: entry["sha256"].as_str().unwrap_or_default().to_owned(),
                description: entry["description"].as_str().unwrap_or_default().to_owned(),
                uses: Uses {
                    light: entry["uses"]["light"] == true,
                    body: entry["uses"]["body"] == true,
                    topics: entry["uses"]["topics"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|topic| topic.as_str().map(str::to_owned))
                        .collect(),
                    speak: entry["uses"]["speak"] == true,
                    reads: entry["uses"]["reads"].as_str().map(str::to_owned),
                },
                package,
            })
        })
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
/// entry still names stays.
fn remember(directory: &Path, from: &str, made: &Made, added: bool) -> io::Result<()> {
    let mut entries = index(directory);
    let mut entry = json!({
        "from": from,
        "name": made.name,
        "version": made.version,
        "sha256": made.sha256,
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
    if does.is_empty() {
        return format!("I made {name}, and it's on.");
    }
    let uses = made.uses.spoken();
    let using = if uses == "nothing" {
        String::new()
    } else {
        format!(", using {uses}")
    };
    format!("I made {name}, and it's on: {does}{using}.")
}

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
        // lamp-0c89: a 23.5 h [quiet] around 22:00, as the brief once said).
        assert!(
            asked.contains("during = { from = \"22:00\", to = \"22:30\" }"),
            "{asked}"
        );
        assert!(!asked.contains("from its end to its start"), "{asked}");
        // The name is SKILL.md's alone: a `name =` in skill.toml fails
        // lamp-skills' check as an unknown field (Mobile on lamp-0c89, #2813).
        assert!(asked.contains("no name key"), "{asked}");
        assert!(brief(&"x".repeat(MOST_REQUEST_BYTES + 1)).is_err());
        assert!(brief("  ").is_err());
        assert!(!is_brief("Write a poem about lamps"));
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
        assert_eq!(uses.reads.as_deref(), Some("google_calendar"));
        assert!(uses.speak);
        assert_eq!(
            uses.spoken(),
            "its voice, reading your Google Calendar and whether it is talking"
        );
        assert_eq!(uses.json()["reads"], "google_calendar");
        assert!(capped_with(READ_TOML, false).is_err());
        for (from, to) in [
            ("google_calendar = \"read\"", "google_calendar = \"write\""),
            (
                "google_calendar = \"read\"",
                "google_calendar = \"read\", gmail = \"read\"",
            ),
            ("google_calendar = \"read\"", "\"gmail-app\" = \"read\""),
            ("google_calendar = \"read\"", "dropbox = \"read\""),
            ("services = { google_calendar = \"read\" }\n", ""),
        ] {
            let wider = READ_TOML.replace(from, to);
            assert!(capped_with(&wider, true).is_err(), "{to}");
        }
        // Any other service with reads: a Drive list, said by name.
        let drive = READ_TOML
            .replace("google_calendar = \"read\"", "google_drive = \"read\"")
            .replace(
                "service = \"google_calendar\", what = \"events\", day = \"tomorrow\"",
                "service = \"google_drive\", what = \"files\", query = \"taxes\"",
            );
        let (_, uses) = capped_with(&drive, true).unwrap();
        assert_eq!(uses.reads.as_deref(), Some("google_drive"));
        assert!(uses.spoken().contains("reading your Google Drive"));
        let said = plainly_with(&drive, true).unwrap();
        assert!(
            said.ends_with(
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
             it reads you tomorrow's Google Calendar: how many events, then each time and title."
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
            said.ends_with("each sender and subject, never the text."),
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

    /// The coordinator's cap (19 Oct): the light and the body, and only
    /// voice/state and room/presence; speaking, messages, the look, any
    /// other topic, a function for the model, a core kind and a missing
    /// version are each refused.
    #[test]
    fn a_made_skill_may_use_only_the_light_and_the_body() {
        let (version, uses) = capped(TOML).unwrap();
        assert_eq!(version, "1.0.0");
        assert_eq!(
            uses,
            Uses {
                light: true,
                body: false,
                topics: vec!["voice/state".into()],
                speak: false,
                reads: None,
            }
        );
        assert_eq!(uses.spoken(), "its light and whether it is talking");
        for (wider, why) in [
            ("speak = true", "never says anything"),
            ("look = true", "more than the light"),
            ("messages = true", "more than the light"),
            ("known_faces = true", "more than the light"),
            ("actions = [\"volume_up\"]", "more than the light"),
            ("frobs = true", "couldn't tell what it would use"),
        ] {
            let text = TOML.replace("light = true\n", &format!("light = true\n{wider}\n"));
            assert!(capped(&text).unwrap_err().0.contains(why), "{wider}");
        }
        // Presence and the clock: a face in view, someone getting up, the
        // time of day; never what is heard nor a picture.
        let presence = TOML.replace(
            "[\"voice/state\"]",
            "[\"humans/faces/tracked\", \"room/stood_up\", \"clock/local\"]",
        );
        assert_eq!(
            capped(&presence).unwrap().1.spoken(),
            "its light, whether it sees a face, someone getting up from the desk and the time \
             of day"
        );
        for wider in ["camera/frame", "ears/classes", "name/heard"] {
            let text = TOML.replace("[\"voice/state\"]", &format!("[\"{wider}\"]"));
            assert!(
                capped(&text).unwrap_err().0.contains("read more"),
                "{wider}"
            );
        }
        let heard = TOML.replace("[\"voice/state\"]", "[\"voice/heard\"]");
        assert!(capped(&heard).unwrap_err().0.contains("read more"));
        let tool = format!("tool = \"dim_now\"\n{TOML}");
        assert!(capped(&tool).unwrap_err().0.contains("runs by itself"));
        let core = TOML.replace("installable", "core");
        assert!(capped(&core).unwrap_err().0.contains("installable"));
        let reminder = TOML.replace("policy = \"nudge\"", "policy = \"reminder\"");
        assert!(capped(&reminder).unwrap_err().0.contains("gentle nudge"));
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
            assert!(uses.speak && !uses.light && uses.reads.is_none());
            assert_eq!(
                uses.spoken(),
                "its voice and whether someone is at the desk"
            );
            assert_eq!(
                plainly_with(HELLO, reads).unwrap(),
                "When someone has been at the desk for 5 seconds, it says “Welcome back!”, at \
                 most once an hour."
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
             seconds, then nods, then says “Welcome back!”, at most once an hour."
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
        // The other limits hold beside a say.
        for wider in ["look = true", "messages = true", "known_faces = true"] {
            let text = HELLO.replace("speak = true\n", &format!("speak = true\n{wider}\n"));
            assert!(
                capped(&text).unwrap_err().0.contains("more than the light"),
                "{wider}"
            );
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
            plainly(&paced(&HELLO.replace("cooldown = \"1h\"\n", "")))
                .unwrap()
                .ends_with("at most once an hour.")
        );
        // A skill that never speaks is given back as it came.
        assert_eq!(paced(TOML), TOML);
        assert_eq!(paced(HELLO), HELLO);
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
                    assert_eq!(
                        plainly(&paced).unwrap(),
                        format!(
                            "When the lamp goes quiet, between {from} and {to}, it says \
                             “Welcome back!”, at most once an hour."
                        )
                    );
                }
                None => {
                    assert_eq!(paced, text, "an all-night window is left for the cap");
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
            assert!(uses.speak && !uses.light && uses.reads.is_none());
            assert_eq!(
                plainly_with(&hum, reads).unwrap(),
                "When someone has been at the desk for 5 seconds, it hums softly, at most once \
                 an hour."
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
             seconds, then nods, then hums softly, at most once an hour."
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

    /// A sound the lamp doesn't ship is refused, by name, path or file, with
    /// lines for a repair; one sound and no say beside it, as `lamp-skills`
    /// takes it, and its voice asked for.
    #[test]
    fn a_sound_the_lamp_doesnt_ship_is_refused() {
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
            match capped(&text) {
                Err(Refused(why, Some(lines))) => {
                    assert_eq!(why, SOUND_NOT_SHIPPED, "{step}");
                    assert!(lines.contains("cue = \"hum\""), "{lines}");
                    assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
                }
                other => panic!("{step}: {other:?}"),
            }
            match plainly_with(&text, false) {
                Err(Refused(why, Some(lines))) => {
                    assert_eq!(why, UNSAID);
                    assert!(lines.contains(SOUND_PART), "{lines}");
                    assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
                }
                other => panic!("{step}: {other:?}"),
            }
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
             light a step, at most once an hour."
        );
        assert_eq!(
            plainly(TOML).unwrap(),
            "When the lamp goes quiet, between 22:00 and 22:30, it dims the light a step, at \
             most once every 12 hours."
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
             seconds, then nods, at most once an hour."
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
            "trigger topic room/activity, its field and equals",
            true,
        );
        refused(
            &TOML.replace("{ dim = 1 }", "{ dim = 1, for = \"1m\" }"),
            "[[do]] step 1, light (dim, for)",
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
        refused(
            &format!("{TOML}\n[quiet]\nfrom = \"21:00\"\nto = \"08:00\"\n"),
            "parts quiet",
            false,
        );
        refused(
            &TOML.replace("version = ", "most_a_day = 1\nversion = "),
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
             minutes, it glows green at level 3 for 20 seconds, at most once every 20 minutes."
        );
    }

    /// A made skill with `trigger` and one `step`, reading presence and the
    /// clock, its light and its body.
    fn made_with(trigger: &str, step: &str) -> String {
        format!(
            "kind = \"installable\"\nversion = \"1.0.0\"\n\n[permissions]\nlight = true\n\
             body = true\ntopics = [\"humans/faces/tracked\", \"room/stood_up\", \
             \"clock/local\"]\n\n[trigger]\n{trigger}\n[ask]\npolicy = \"nudge\"\n\n\
             [[do]]\n{step}\n"
        )
    }

    /// A face coming into view (never whose), someone at the desk getting
    /// up, and a time each day are said as lamp-skills runs them, and kept.
    #[test]
    fn a_face_a_getting_up_and_a_time_each_day_are_said() {
        let kept_and_said = |trigger: &str, step: &str, said: &str| {
            let text = made_with(trigger, step);
            assert!(capped_with(&text, false).is_ok(), "{text}");
            assert_eq!(plainly_with(&text, false).unwrap(), said);
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
             arm in a greeting."
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
        // Other triggers, silent, are given back as they came.
        let clock = made_with(
            "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
             during = { from = \"22:00\", to = \"22:01\" }\n",
            "light = { dim = 1 }",
        );
        assert_eq!(paced(&clock), clock);
    }

    /// What lamp-skills would run otherwise than said, or that reads who
    /// someone is, is refused: a face by a field or a list (a name, a known
    /// face), a face held while in view, a getting up held a while, a time
    /// over a window or on a week's day (lamp-skills has none).
    #[test]
    fn a_face_by_who_a_window_or_a_weekday_is_refused() {
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
        let known = made_with(face, "light = { dim = 1 }")
            .replace("body = true\n", "body = true\nknown_faces = true\n");
        assert!(
            capped_with(&known, false)
                .unwrap_err()
                .0
                .contains("more than the light")
        );
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
        assert!(
            part(&format!("{clock}rearm = \"ends\"\n{at}days = [\"mon\"]\n"))
                .starts_with(&format!("{clocked}, keys days."))
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
                format!("When someone comes to the desk, it {saying}.")
            );
        }
        for refused in ["moonwalk", "idle", "dip", "tilt_left", "Nod"] {
            match said(refused) {
                Err(Refused(why, Some(lines))) => {
                    assert_eq!(why, UNSAID);
                    assert!(lines.starts_with("Can't be said plainly: [[do]] step 1, body"));
                    assert!(lines.contains("listed body gesture"), "{lines}");
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
             green at level 2 for 20 seconds."
        );
        assert_eq!(
            plainly(&back("when = \"appears\"\n")).unwrap(),
            "When someone comes to the desk, it glows green at level 2 for 20 seconds."
        );
        assert_eq!(
            plainly(&back("when = \"present\"\nfor = \"5s\"\n")).unwrap(),
            "When someone has been at the desk for 5 seconds, it glows green at level 2 for 20 \
             seconds."
        );
        let part = |toml: &str| match plainly_with(toml, false) {
            Err(Refused(_, Some(lines))) => lines,
            other => panic!("{other:?}"),
        };
        let presence = "Can't be said plainly: trigger topic room/presence";
        assert!(part(&back("absent_for = \"10m\"\n")).starts_with(&format!(
            "{presence}, its absent_for, which goes with when = \"appears\"."
        )));
        assert!(part(&back("when = \"changes\"\n")).starts_with(&format!("{presence}, its when.")));
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
        assert!(SAYABLE_TRIGGERS.contains(TRIGGER_MEANINGS));
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
                reads: None,
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
        remember(&dir, "local:1", &made, false).unwrap();
        remember(&dir, "local:2", &made, false).unwrap();
        assert_eq!(names(&dir), ["water"]);
        assert_eq!(kept(&dir, "local:2").unwrap().sha256, made.sha256);
        assert!(kept(&dir, "local:1").is_none());
        assert!(made.package.is_file());
        let other = made_as(&dir, "water", 'b');
        remember(&dir, "local:3", &other, false).unwrap();
        assert_eq!(names(&dir), ["water"]);
        assert_eq!(index(&dir)[0]["sha256"], other.sha256);
        assert!(!made.package.exists() && other.package.is_file());
        // On, its bytes kept again stay on; another of its name sits beside.
        let mut entries = index(&dir);
        entries[0]["on"] = json!(true);
        write_index(&dir, &entries).unwrap();
        remember(&dir, "local:4", &other, false).unwrap();
        assert_eq!(index(&dir).len(), 1);
        assert_eq!(index(&dir)[0]["on"], true);
        let third = made_as(&dir, "water", 'c');
        remember(&dir, "local:5", &third, false).unwrap();
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
        remember(&dir, "local:6", &made_as(&dir, "six", '6'), false).unwrap();
        assert_eq!(names(&dir), ["on-one", "three", "shared-b", "five", "six"]);
        assert!(dir.join("on-one.tar").is_file());
        assert!(dir.join("shared.tar").is_file());
        remember(&dir, "local:7", &made_as(&dir, "seven", '7'), false).unwrap();
        assert!(!dir.join("three.tar").exists());
        // All on: kept, past the cap.
        let mut entries = index(&dir);
        for entry in &mut entries {
            entry["on"] = json!(true);
        }
        write_index(&dir, &entries).unwrap();
        let eighth = made_as(&dir, "eight", '8');
        remember(&dir, "local:8", &eighth, false).unwrap();
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
            remember(&dir, &format!("local:{at}"), &water, false).unwrap();
        }
        for (at, sha) in ['p', 'q', 'r'].into_iter().enumerate() {
            remember(
                &dir,
                &format!("app:{at}"),
                &made_as(&dir, &format!("more-{at}"), sha),
                false,
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

    /// A name any skill on the lamp has, installed or pushed, is refused, and
    /// nothing is kept. A kept one of that name not on, marked to go on or
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
            let refused = keep_written(&study_mode(), "written:app:1:1", &keeping, &installed);
            assert!(refused.unwrap_err().0.contains("already on the lamp"));
            fs::remove_dir_all(&on_lamp).unwrap();
        }
        assert!(index(&keeping.directory).is_empty());
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
            reads: Some(code.to_owned()),
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
                reads: Some("gmail".into()),
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
}
