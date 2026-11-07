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
//! most [`KEPT_MOST`]), with its SHA-256 and what it asks to use. The lamp
//! then says what it would do, what it asks to use, and asks [`ASK_TO_PUT_ON`]
//! (the lead, 20 Oct 08:37 on #1711: a spoken yes bound to the SHA-256, an
//! exception to the "only signed packages" ruling of 19 Oct 18:33 for a
//! skill made here alone). The ask is kept beside the package ([`offered`]);
//! on the person's spoken yes within [`PUT_ON_WITHIN`] the model calls
//! [`PUT_ON`], and [`put_on`] asks `lamp-skills`' manage helper for that
//! hash alone, which installs only those bytes, held again to the cap.
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
/// The topics a made skill may read: whether the voice is talking, and
/// whether someone is at the lamp. Nothing it reads is about a person's
/// words, face or picture.
pub const TOPICS: [&str; 2] = ["voice/state", "room/presence"];
/// What the lamp says of a skill it made, word for word.
pub const ASK_TO_PUT_ON: &str = "Shall I put it on the lamp?";
/// Whether the lamp's words said [`ASK_TO_PUT_ON`], in any case: the model
/// runs it into a sentence ("Just to be sure: shall I put it on the
/// lamp?", lamp-9adf, 2 Nov 22:51), and that ask is the same ask.
pub fn says_the_ask(said: &str) -> bool {
    said.to_lowercase().contains(&ASK_TO_PUT_ON.to_lowercase())
}
/// The function the model calls on the person's spoken yes to [`ASK_TO_PUT_ON`].
pub const PUT_ON: &str = "put_on_made_skill";
/// A yes counts this long after the lamp asked, as a hand-over's does.
pub const PUT_ON_WITHIN: Duration = crate::agent_tasks::CONFIRM_WITHIN;
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
/// The most of the model's reason said with it, or of what it left out.
const MOST_CANNOT_CHARS: usize = 120;
/// How a line outside the blocks begins that names what was asked and
/// isn't in the skill ("left out: dimming slowly; it dims in one step"):
/// said after what it does, so a yes is to what was left out too.
pub const LEFT_OUT: &str = "left out:";
/// The file that keeps what was made, in [`DIRECTORY`].
pub const INDEX: &str = "made.json";

/// The brief's first words: a task that begins with them is a skill to make.
pub const BRIEF_HEAD: &str = "Write one lamp skill (lampOS lamp-skills)";
/// What comes before the person's request in the brief.
const REQUEST_MARK: &str = "Request: ";

/// What a request for a skill that uses one of the person's services (mail,
/// a calendar, Slack, ...) is told: a made skill never reaches a linked
/// service yet, linked or not, but the person can ask the lamp itself. It
/// names no place to link one: on lamp-9adf, 4 Nov, the model told the
/// owner to link a calendar already linked (the first builder on #5288).
/// Words to the person, as the app shows them as they are
/// (`app_make::begin`'s `refused`); the voice tells the model to say them.
pub const NOT_A_SERVICE: &str = "Not possible yet: a skill can't use your mail, calendar or \
     other services, even ones already linked. You can ask me for it yourself any time.";
/// What a request to make a skill that would write to a linked service is
/// told with a made skill's reads on ([`READS_ENVIRONMENT`]): a skill reads
/// one service and never writes ([`NOT_A_SERVICE`] isn't true there; the
/// testers' sheet, 6 Nov, "a skill is limited with no reason").
pub const NOT_A_WRITE: &str = "Not possible yet: a skill can read one linked service, but \
     it can't send, write or change anything there. You can ask me for it yourself any time.";
/// What a request naming two linked services is told with reads on.
pub const ONE_SERVICE: &str = "Not possible yet: a skill can read only one linked service, \
     not two or more. You can ask me for it yourself any time.";
/// What a request is told with reads on when the service it names has
/// nothing a made skill reads (Notion, read only through its own tools).
pub const NOT_READABLE: &str = "Not possible yet: a skill can read one linked service, but \
     not that one. You can ask me for it yourself any time.";
/// Words that name a linked service ([`crate::services`]) in a request, as
/// a whole word: mail and a calendar, and the services' own names.
const SERVICE_WORDS: [&str; 17] = [
    "mail",
    "mails",
    "email",
    "emails",
    "gmail",
    "inbox",
    "calendar",
    "gdrive",
    "notion",
    "figma",
    "github",
    "asana",
    "ahrefs",
    "facebook",
    "slack",
    "hubspot",
    "amplitude",
];
/// Services whose one word is also a plain one ("drive", "linear",
/// "monday"): named only as these.
const SERVICE_PHRASES: [&str; 3] = ["google drive", "linear app", "monday com"];

/// The switch for a made skill that reads a linked service to its owner
/// alone (#5288, the first builder's ruling there, 4 Nov 20:5x): here, in
/// `lamp-skills`, its manage helper and `lamp-services`. `on` only; off,
/// every request that names a service is [`NOT_A_SERVICE`], as before.
pub const READS_ENVIRONMENT: &str = "LAMP_SKILL_READS";

/// Whether a made skill may read a linked service in this process.
pub fn reads_on() -> bool {
    std::env::var(READS_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The service each of [`SERVICE_WORDS`] and [`SERVICE_PHRASES`] names, by
/// its code in `lamp-services`' catalogue. A made skill may read any of
/// them that has reads (`lamp_services::spoken::not_readable`).
const SERVICE_CODES: [(&str, &str); 20] = [
    ("mail", "gmail"),
    ("mails", "gmail"),
    ("email", "gmail"),
    ("emails", "gmail"),
    ("gmail", "gmail"),
    ("inbox", "gmail"),
    ("calendar", "google_calendar"),
    ("gdrive", "google_drive"),
    ("notion", "notion"),
    ("figma", "figma"),
    ("github", "github"),
    ("asana", "asana"),
    ("ahrefs", "ahrefs"),
    ("facebook", "facebook"),
    ("slack", "slack"),
    ("hubspot", "hubspot"),
    ("amplitude", "amplitude"),
    ("google drive", "google_drive"),
    ("linear app", "linear"),
    ("monday com", "mondaydotcom"),
];

/// Words of a request that would write, send or change something: a made
/// skill only reads (writes are not ruled yet).
/// ("posts" is left out: a Page's posts are read.)
const WRITE_WORDS: [&str; 21] = [
    "send", "sends", "draft", "drafts", "reply", "replies", "write", "writes", "add", "adds",
    "post", "create", "creates", "forward", "delete", "mark", "archive", "invite", "book",
    "schedule", "put",
];

/// Write words that are also things read: "my schedule", "my latest
/// Facebook post", "the book titles" (El's P3 on #5679).
const ALSO_A_THING: [&str; 3] = ["schedule", "post", "book"];

/// Words before one of [`ALSO_A_THING`] that make it the thing read,
/// whatever follows: "my schedule on my calendar", "the latest post on
/// Facebook" (El's P2 on #5696).
const A_THING_AFTER: [&str; 6] = ["my", "the", "s", "latest", "last", "newest"];

/// Words before one of [`ALSO_A_THING`] that make it the thing read only
/// at the request's end or before one of [`A_THING_BEFORE`]: "my latest
/// Facebook post", "my calendar schedule for tomorrow"; else it is a verb,
/// "tomorrow book the room", "facebook post that I'm out" (El's P3s on
/// #5690 and #5696).
const MAYBE_A_THING_AFTER: [&str; 6] = ["tomorrow", "today", "day", "calendar", "facebook", "page"];

/// Words after one of [`ALSO_A_THING`] that keep it the thing read after
/// one of [`MAYBE_A_THING_AFTER`].
const A_THING_BEFORE: [&str; 3] = ["from", "for", "of"];

/// Words a request asks to be read with: before a thing said with "say",
/// the saying is what is read ("read me the last post saying sale"), not
/// a post to write ("latest post: say hi on Facebook").
const ASKED_TO_READ: [&str; 6] = ["read", "reads", "tell", "what", "show", "check"];

/// Whether `words[at]`, one of [`ALSO_A_THING`], is the thing read.
fn a_thing(words: &[&str], at: usize) -> bool {
    let Some(before) = at.checked_sub(1).map(|before| words[before]) else {
        return false;
    };
    if A_THING_AFTER.contains(&before) {
        let says = words[at + 1..]
            .iter()
            .any(|word| ["say", "saying"].contains(word));
        !says || words[..at].iter().any(|word| ASKED_TO_READ.contains(word))
    } else if MAYBE_A_THING_AFTER.contains(&before) {
        words
            .get(at + 1)
            .is_none_or(|next| A_THING_BEFORE.contains(next))
    } else {
        false
    }
}

/// Whether "put" at `words[at]` writes: with a service named after it, or
/// put "in", "on", "into" or "to" "my drive" (El's P3 on #5696: "put the
/// count on the light from my drive" is the lamp's own).
fn puts_to_a_service(words: &[&str], at: usize) -> bool {
    let after = &words[at + 1..];
    names_a_linked_service(&after.join(" "))
        || after.windows(3).any(|three| {
            ["in", "on", "into", "to"].contains(&three[0]) && three[1..] == ["my", "drive"]
        })
}

/// Words that only ask for the skill itself, before "skill" at a request's
/// start: "create a skill that reads my calendar" asks for a skill that
/// reads, and its "create" is no write (on lamp-9adf on 4 Nov, 23:51, a
/// calendar read skill asked for by voice was refused so).
const ASKING_FOR_A_SKILL: [&str; 20] = [
    "create", "make", "add", "build", "write", "set", "up", "me", "a", "an", "new", "please",
    "lampy", "lamp", "can", "you", "could", "i", "want", "like",
];

/// `words` without the asking for the skill itself ("create me a new
/// skill that ...", "add a skill to ..."): only when every word before
/// "skill" is such asking, so a write said elsewhere still counts.
fn asked_of_the_skill<'a>(words: &[&'a str]) -> Vec<&'a str> {
    let Some(at) = words.iter().position(|word| *word == "skill") else {
        return words.to_vec();
    };
    if !words[..at]
        .iter()
        .all(|word| ASKING_FOR_A_SKILL.contains(word))
    {
        return words.to_vec();
    }
    let rest = &words[at + 1..];
    let skip = usize::from(
        rest.first()
            .is_some_and(|word| ["that", "which", "to"].contains(word)),
    );
    rest[skip..].to_vec()
}

/// What a request to be read a linked service asks for: the service's
/// code, and for a calendar or mail the one step `lamp-skills` takes (a
/// calendar's events of a day, tomorrow when it says so, else today; the
/// latest mail). Any other service's step the model writes from the
/// service's reads. Its limit, in the person's words, when it would write
/// ([`NOT_A_WRITE`]), names two services ([`ONE_SERVICE`]), or names one
/// with nothing to read or none the lamp knows ([`NOT_READABLE`]).
fn read_of(request: &str) -> Result<(&'static str, Option<&'static str>), &'static str> {
    let request = request.to_lowercase().replace("e-mail", "email");
    let words: Vec<&str> = request
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let words = asked_of_the_skill(&words);
    let writes = words.iter().enumerate().any(|(at, word)| {
        WRITE_WORDS.contains(word)
            && !(ALSO_A_THING.contains(word) && a_thing(&words, at))
            // "put" writes to a service only when one is named after it:
            // "put the count on the light" is the lamp's own.
            && (*word != "put" || puts_to_a_service(&words, at))
    });
    if writes {
        return Err(NOT_A_WRITE);
    }
    let joined = format!(" {} ", words.join(" "));
    let mut codes: Vec<&'static str> = SERVICE_CODES
        .iter()
        .filter(|(said, _)| {
            if said.contains(' ') {
                joined.contains(&format!(" {said} "))
            } else {
                words.contains(said)
            }
        })
        .map(|(_, code)| *code)
        .collect();
    codes.sort_unstable();
    codes.dedup();
    let code = match codes.as_slice() {
        [code] => code,
        // Not reached from `brief_with`, which asks this only of a request
        // that names a service; were it reached, no service is one with
        // nothing to read.
        [] => return Err(NOT_READABLE),
        _ => return Err(ONE_SERVICE),
    };
    if lamp_services::spoken::not_readable(code).is_some() {
        return Err(NOT_READABLE);
    }
    let step = match *code {
        "google_calendar" if words.contains(&"tomorrow") => {
            Some("read = { service = \"google_calendar\", what = \"events\", day = \"tomorrow\" }")
        }
        "google_calendar" => {
            Some("read = { service = \"google_calendar\", what = \"events\", day = \"today\" }")
        }
        "gmail" => Some("read = { service = \"gmail\", what = \"recent\" }"),
        "google_drive" => Some(DRIVE_READ),
        _ => None,
    };
    Ok((code, step))
}

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

/// Whether a request names a linked service (any case; "e-mail" too), so
/// a skill to make is never asked of the model for one.
fn names_a_linked_service(request: &str) -> bool {
    let request = request.to_lowercase().replace("e-mail", "email");
    let words: Vec<&str> = request
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let joined = format!(" {} ", words.join(" "));
    words.iter().any(|word| SERVICE_WORDS.contains(word))
        || SERVICE_PHRASES
            .iter()
            .any(|phrase| joined.contains(&format!(" {phrase} ")))
}

/// The brief: the format of a skill a lamp can make, what it may use, and
/// the person's request, within [`MOST_BRIEF_BYTES`]. Nothing of
/// the conversation but the request: the model passes it, in the person's
/// words.
pub fn brief(request: &str) -> Result<String, &'static str> {
    brief_with(request, reads_on())
}

/// [`brief`], with a made skill's reads on or off ([`READS_ENVIRONMENT`]).
/// On, a request to be read one linked service with reads is briefed as a
/// skill whose one step reads it ([`read_of`]): a calendar's, mail's or
/// Drive's step given whole, any other service's reads listed for the model to choose
/// from (`lamp_services::spoken::whats`, never a whole text). A request
/// that would write, names two services or one with nothing to read is
/// told that limit ([`read_of`]); with reads off, any is [`NOT_A_SERVICE`].
pub fn brief_with(request: &str, reads: bool) -> Result<String, &'static str> {
    let request = request.split_whitespace().collect::<Vec<_>>().join(" ");
    if request.is_empty() {
        return Err("Say what the skill should do.");
    }
    if request.len() > MOST_REQUEST_BYTES {
        return Err("That is too long to ask for as one skill: say it more shortly.");
    }
    if names_a_linked_service(&request) {
        if !reads {
            return Err(NOT_A_SERVICE);
        }
        let (service, step) = read_of(&request)?;
        let step = match step {
            Some(step) => step.to_owned(),
            None => format!(
                "read = {{ service = \"{service}\", what = one of {}, its keys }}",
                lamp_services::spoken::whats(service).join("; ")
            ),
        };
        return Ok(format!(
            "{BRIEF_HEAD} for the request at the end. Reply with exactly two fenced blocks, \
             ```skill.toml and ```SKILL.md, and nothing else; write no files. skill.toml: \
             kind = \"installable\", version = \"1.0.0\", no name key; [permissions] with only \
             speak = true, services = {{ {service} = \"read\" }} and topics only voice/state and \
             room/presence; one [trigger] on voice/state (field \"state\", equals \"resting\") or \
             room/presence (field \"someone\", equals true), with for; during = \
             {{ from = \"HH:MM\", to = \"HH:MM\" }} only for a time asked for; \
             [ask] policy = \"nudge\"; \
             one [[do]] step: {step}. \
             SKILL.md: front matter name (lowercase, hyphens) and description (one sentence: \
             what it does, when). {REQUEST_MARK}{request}"
        ));
    }
    Ok(format!(
        "{BRIEF_HEAD} for the request at the end. Reply with exactly two fenced blocks, \
         ```skill.toml and ```SKILL.md, and nothing else; write no files. skill.toml: \
         kind = \"installable\", version = \"1.0.0\", no name key; [permissions] \
         with only light = true and/or body = true, topics only voice/state and room/presence; \
         one [trigger] on \
         voice/state (field \"state\", equals \"resting\") or room/presence (field \"someone\", \
         equals true or false), with for, cooldown, rearm = \"ends\"; a time of day as the \
         trigger's during = {{ from = \"22:00\", to = \"22:30\" }}; \
         [ask] policy = \"nudge\"; [[do]] steps: light = {{ colour, level 1-5, for }} or \
         {{ dim = 1 }}, body = {{ gesture = \"nod\" or \"curious\" }}. SKILL.md: front matter \
         name (lowercase, hyphens) and description (one sentence: what it does, when). \
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
/// builder on #3126, 23 Oct 13:07).
pub const MOST_REPAIR_BYTES: usize = 600;
/// The longest a [`brief`] is with the longest request, any service's
/// read step included: no longer half the tasks part's 2,000 bytes, but
/// small enough that its repair, with the [`REPAIR_MARK`], the check's
/// [`MOST_REPAIR_BYTES`] and the ask, still fits that bound.
pub const MOST_BRIEF_BYTES: usize = 1_300;
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

/// The two files of the model's answer: exactly one fenced block named
/// `skill.toml` and one named `SKILL.md` (its info string's last word),
/// and no other fenced block; each at most [`MOST_FILE_BYTES`].
pub fn files(answer: &str) -> Result<Files, Refused> {
    // The model's word that no made skill can do what was asked (it can't
    // hear words said, say anything, or keep time by itself): said as
    // that, never stood in for by something else.
    if let Some(why) = after_word(answer.trim_start(), CANNOT)
        && !answer.contains("```")
    {
        return Err(Refused::new(format!(
            "a skill can't do that: {}",
            cut_chars(why, MOST_CANNOT_CHARS)
        )));
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
            return Err(Refused::new("its answer was cut off"));
        }
        if body.len() > MOST_FILE_BYTES {
            return Err(Refused::new("one of its files is too long"));
        }
        let slot = match name {
            "skill.toml" => &mut toml,
            "SKILL.md" => &mut card,
            _ => return Err(Refused::new("it wrote a file a made skill can't have")),
        };
        if slot.replace(body).is_some() {
            return Err(Refused::new("it wrote one of its files twice"));
        }
    }
    match (toml, card) {
        (Some(toml), Some(card)) => Ok(Files {
            toml,
            card,
            left_out,
        }),
        _ => Err(Refused::new("it didn't write both of a skill's files")),
    }
}

/// What a made skill asks to use: within [`capped`]'s bounds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Uses {
    pub light: bool,
    pub body: bool,
    pub topics: Vec<String>,
    /// Its voice: only a skill written in the app ([`Written`]), whose
    /// call's answer is the person's instructions for the model; or one
    /// that reads a linked service to its owner ([`Uses::reads`]).
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
/// voice's [`needs_link_said`]. `linked` gives the codes linked now
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

/// What the lamp adds aloud for each of [`needs_link`]'s services: one
/// short plain sentence where the person links it. None with none.
pub fn needs_link_said(needs: &[Value]) -> Option<String> {
    let said: Vec<String> = needs
        .iter()
        .filter_map(|need| need["name"].as_str())
        .map(|name| {
            format!(
                "{name} isn't linked yet: link it in the app, under Skills, then Connectors, and \
                 the skill will start then."
            )
        })
        .collect();
    (!said.is_empty()).then(|| said.join(" "))
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
    if table.contains_key("tool") {
        return Err(Refused::new(
            "it gives the model a function, which a made skill can't",
        ));
    }
    held(&table, false, reads)
}

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
            "it asks the Policy to start it, which a skill added from a file never does",
        ));
    }
    if !table.get("tool").is_some_and(toml::Value::is_str) {
        return Err(Refused::new(
            "it has no function for the model, so it could never be asked for",
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
        _ => return Err(Refused::new("a made skill can only be installable")),
    }
    let Some(version) = table.get("version").and_then(|version| version.as_str()) else {
        return Err(Refused::new("it has no version"));
    };
    // Asked of the Policy as a nudge, never as a hello, news or a reminder
    // (the reviewer on #2011): the act `lamp-skills` passes on.
    if let Some(ask) = table.get("ask")
        && ask.get("policy").and_then(|policy| policy.as_str()) != Some("nudge")
    {
        return Err(Refused::new(
            "it asks the Policy as something other than a nudge",
        ));
    }
    let mut uses = Uses::default();
    if let Some(permissions) = table.get("permissions") {
        let Some(permissions) = permissions.as_table() else {
            return Err(Refused::new("its permissions can't be read"));
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
                // A read of a linked service is said aloud (#5288): its
                // voice, held below to a skill that reads.
                ("speak", toml::Value::Boolean(on)) if reads => uses.speak = *on,
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
                _ => return Err(Refused::new("its permissions can't be read")),
            }
        }
    }
    // Its voice only for what it reads to the owner.
    if !added && uses.speak && uses.reads.is_none() {
        return Err(Refused::new(
            "it asks to use more than the light and the head, which a made skill can't",
        ));
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
        "its skill.toml can't be read".into(),
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
    // Its lines go back to the model once, as the check's do; with reads
    // on, they keep the brief's read step ([`PLAIN_OR_READ`]).
    let unsaid = || {
        Refused(
            "it does something the lamp can't say plainly".into(),
            Some(if reads {
                format!("{PLAIN_ONLY}{PLAIN_OR_READ}")
            } else {
                PLAIN_ONLY.into()
            }),
        )
    };
    let table: toml::Table = toml::from_str(text).map_err(unreadable)?;
    // A skill written in the app ([`Written`]): a function whose answer is
    // the person's own instructions, said as that and nothing more. Only its
    // exact shape; another with a function, added from a file, is said below
    // (Twelve's P2 on #5072).
    if let Some(said) = written_plainly(&table) {
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
    if table.keys().any(|key| !parts.contains(&key.as_str())) {
        return Err(unsaid());
    }
    // A skill added from a file (`crate::upload_skill`): started by its
    // function alone, when the person asks, never on its own. A made skill
    // never gets here with a tool: [`capped`] refuses it first.
    if table.contains_key("tool") {
        if table.contains_key("trigger") || table.contains_key("ask") {
            return Err(unsaid());
        }
        let steps = table
            .get("do")
            .and_then(|steps| steps.as_array())
            .filter(|steps| !steps.is_empty())
            .ok_or_else(unsaid)?
            .iter()
            .map(|step| {
                note_plainly(step)
                    .or_else(|| step_plainly(step))
                    .ok_or_else(unsaid)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let said = format!(
            "Only when you ask for it, it {}, and never on its own.",
            steps.join(", then ")
        );
        if said.chars().count() > MOST_PLAIN_CHARS {
            return Err(unsaid());
        }
        return Ok(said);
    }
    let trigger = table
        .get("trigger")
        .and_then(|trigger| trigger.as_table())
        .ok_or_else(unsaid)?;
    let known = [
        "topic", "field", "equals", "for", "cooldown", "rearm", "during",
    ];
    if trigger.keys().any(|key| !known.contains(&key.as_str())) {
        return Err(unsaid());
    }
    let text_of = |key: &str| trigger.get(key).and_then(|value| value.as_str());
    let lasting = match text_of("for") {
        Some(lasting) => Some(spell(lasting).ok_or_else(unsaid)?),
        None => None,
    };
    let topic = (text_of("topic"), text_of("field"), trigger.get("equals"));
    let mut said = match (topic, lasting) {
        ((Some("voice/state"), Some("state"), Some(state)), lasting)
            if state.as_str() == Some("resting") =>
        {
            match lasting {
                Some(lasting) => format!("When the lamp has been quiet for {lasting}"),
                None => "When the lamp goes quiet".to_owned(),
            }
        }
        ((Some("room/presence"), Some("someone"), Some(toml::Value::Boolean(true))), lasting) => {
            match lasting {
                Some(lasting) => format!("When someone has been at the desk for {lasting}"),
                None => "When someone comes to the desk".to_owned(),
            }
        }
        ((Some("room/presence"), Some("someone"), Some(toml::Value::Boolean(false))), lasting) => {
            match lasting {
                Some(lasting) => format!("When the desk has been empty for {lasting}"),
                None => "When the desk is left".to_owned(),
            }
        }
        _ => return Err(unsaid()),
    };
    if let Some(during) = trigger.get("during") {
        let hour = |key: &str| during.get(key).and_then(|hour| hour.as_str());
        let (Some(from), Some(to)) = (hour("from"), hour("to")) else {
            return Err(unsaid());
        };
        said.push_str(&format!(", between {from} and {to}"));
    }
    let steps = table
        .get("do")
        .and_then(|steps| steps.as_array())
        .filter(|steps| !steps.is_empty())
        .ok_or_else(unsaid)?;
    // A read of a linked service (#5288): its skill's one step, said with
    // the gate it waits for.
    if let [step] = steps.as_slice()
        && let Some(read) = read_plainly(step).filter(|_| reads)
    {
        said.push_str(&format!(", when you're alone with the lamp, it {read}"));
    } else {
        let steps = steps
            .iter()
            .map(|step| step_plainly(step).ok_or_else(unsaid))
            .collect::<Result<Vec<_>, _>>()?;
        said.push_str(&format!(", it {}", steps.join(", then ")));
    }
    if let Some(cooldown) = text_of("cooldown") {
        let (count, unit) = duration(cooldown).ok_or_else(unsaid)?;
        said.push_str(&match count {
            1 => format!(", at most once {}", one(unit)),
            _ => format!(", at most once every {count} {unit}s"),
        });
    }
    said.push('.');
    // Said whole or not at all: never cut before a step or the cooldown.
    if said.chars().count() > MOST_PLAIN_CHARS {
        return Err(unsaid());
    }
    Ok(said)
}

/// The longest [`plainly`] says: one breath, and with [`left_out`]'s
/// words still within [`MOST_DESCRIPTION_CHARS`], so nothing is cut.
const MOST_PLAIN_CHARS: usize = 200;

/// What a repair is told when [`plainly`] refused.
const PLAIN_ONLY: &str = "only kind, version, needs, permissions, one [trigger], [ask] and \
                          [[do]]; the trigger may have only topic, field, equals, for, cooldown, \
                          rearm and during, and at most a few [[do]] steps, each one light or \
                          one body gesture, as the request names them";

/// What [`PLAIN_ONLY`] goes on to say with reads on: a skill that reads a
/// linked service keeps its one read step. On lamp-9adf, 5 Nov 13:26, an
/// app make of "Read me my latest email when I sit down at my desk." was
/// refused twice: the repair, told only lights and gestures, took the
/// mail read away.
const PLAIN_OR_READ: &str = "; or, for a read of a linked service, only one [[do]] step: the \
                             brief's read step exactly as the brief gives it, and nothing else";

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

/// One `[[do]]` step in plain words: a light or a gesture, as the brief
/// names them.
fn step_plainly(step: &toml::Value) -> Option<String> {
    let step = step.as_table().filter(|step| step.len() == 1)?;
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
        (1, Some("nod")) => Some("nods".to_owned()),
        (1, Some("curious")) => Some("looks up, curious".to_owned()),
        _ => None,
    }
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
    let unread = || Refused::new("its SKILL.md has no name and description");
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
/// is refused; a kept one of that name not on, asked about or not, is
/// replaced, its offer with it. Kept once: asked again, the kept one is
/// given back.
/// Nothing is put on: only the person's spoken yes does ([`put_on`]).
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
    // A kept one of that name not on the lamp, asked about or not, gives way
    // to the person's newer words: a spoken no leaves no mark, so a fix sent
    // after it must not wait out the old ask (Seventh's P2 on #5072). Its
    // offer goes with it, so a late yes puts nothing on; the new one is
    // asked about on its own.
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

/// Where made skills are kept, and the program that checks them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keeping {
    pub directory: PathBuf,
    pub program: PathBuf,
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
        })
    }
}

/// A made skill, checked and kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    pub name: String,
    pub version: String,
    /// The package's SHA-256, lowercase hex: what a yes to put it on would
    /// be bound to.
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
        return Err(Refused::new("its answer was cut off"));
    }
    let files = files(answer)?;
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
        "it doesn't pass the lamp's check".into(),
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

/// Keep, beside the package whose SHA-256 is `sha256`, that the lamp asked
/// [`ASK_TO_PUT_ON`] at `now_unix_ms`: what a yes is held to ([`put_on`]).
/// Only the newest ask waits; another skill asked about later takes its
/// place.
pub fn offered(directory: &Path, sha256: &str, now_unix_ms: u64) -> io::Result<()> {
    let mut entries = index(directory);
    let mut found = false;
    for entry in &mut entries {
        if entry["sha256"] == sha256 {
            entry["offered_at_unix_ms"] = json!(now_unix_ms);
            found = true;
        }
    }
    if found {
        write_index(directory, &entries)?;
    }
    Ok(())
}

/// The made skill `sha256`'s ask was begun aloud at `now_unix_ms` (a say
/// turn of the lamp's own, crate::app_make): kept as `ask_begun_at_unix_ms`,
/// from which an untold skill's window is counted
/// (`make_skill_local::UNTOLD_WITHIN`), so an ask queued long is still
/// answerable once said.
pub fn ask_begun(directory: &Path, sha256: &str, now_unix_ms: u64) -> io::Result<()> {
    let mut entries = index(directory);
    let mut found = false;
    for entry in &mut entries {
        if entry["sha256"] == sha256 {
            entry["ask_begun_at_unix_ms"] = json!(now_unix_ms);
            found = true;
        }
    }
    if found {
        write_index(directory, &entries)?;
    }
    Ok(())
}

/// The lamp's own answer said [`ASK_TO_PUT_ON`] `ago` before now, with
/// `lamp_said` its words: the model, asked whether the skill is ready, may
/// say it before the news is told (lamp-9adf, 31 Oct 09:32: "Is the skill I
/// asked for ready yet?" was answered "… Shall I put it on the lamp?", and
/// the plain yes after it found nothing waiting). Then the one made skill
/// never asked about, not on and kept after the newest ask, is kept as
/// asked when that answer was said, so [`waiting`] holds the yes to the
/// usual [`PUT_ON_WITHIN`] (the first builder, 2 Nov 10:1x; the
/// Reviewer-lead's P2 on #4880: only the turn right after the lamp itself
/// asked, never a yes to another question). With two or more such skills
/// the answer may have named any of them, so none is kept (the
/// Reviewer-lead's second P2 on #4880): a yes then finds nothing waiting,
/// as before. Whether one was kept so.
pub fn asked_in_answer(
    directory: &Path,
    lamp_said: &str,
    ago: Duration,
    now_unix_ms: u64,
) -> io::Result<bool> {
    if !says_the_ask(lamp_said) || ago > PUT_ON_WITHIN {
        return Ok(false);
    }
    let entries = index(directory);
    let asked_at = entries
        .iter()
        .filter_map(|entry| entry["offered_at_unix_ms"].as_u64())
        .max()
        .unwrap_or(0);
    let said_at = now_unix_ms.saturating_sub(ago.as_millis() as u64);
    let untold: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|entry| entry["offered_at_unix_ms"].as_u64().is_none() && entry["on"] != true)
        .filter(|entry| {
            entry["at_unix_ms"]
                .as_u64()
                .is_some_and(|kept_at| kept_at > asked_at && kept_at <= said_at)
        })
        .collect();
    let [only] = untold.as_slice() else {
        return Ok(false);
    };
    match only["sha256"].as_str() {
        Some(sha256) => offered(directory, sha256, said_at).map(|()| true),
        None => Ok(false),
    }
}

/// The made skill the lamp asked about within [`PUT_ON_WITHIN`] before
/// `now_unix_ms` and not yet on: its name and SHA-256. The newest ask
/// alone: once it is on, or out of its time, none. lamp-skills' manage
/// helper holds `install_made` to it too, by its own clock, so a kept
/// SHA-256 is never put on later by any path.
pub fn waiting(directory: &Path, now_unix_ms: u64) -> Option<(String, String)> {
    let within = PUT_ON_WITHIN.as_millis() as u64;
    // The newest ask first, whatever became of it: once it is on, or with
    // the clock before it, nothing waits, and an older ask never stands in
    // for it (reviewer-two's P2 on #2874).
    let (at, entry) = index(directory)
        .into_iter()
        .filter_map(|entry| Some((entry["offered_at_unix_ms"].as_u64()?, entry)))
        .max_by_key(|(at, _)| *at)?;
    (entry["on"] != true && at <= now_unix_ms && now_unix_ms - at <= within).then(|| {
        (
            entry["name"].as_str().unwrap_or_default().to_owned(),
            entry["sha256"].as_str().unwrap_or_default().to_owned(),
        )
    })
}

/// The note a request is given while a made skill waits for its yes (the
/// second route keeps its exchanges' words, not their calls, as
/// `agent_tasks::pending_note` says): without it a yes is a yes to nothing.
pub fn waiting_note(directory: &Path, now_unix_ms: u64) -> Option<String> {
    waiting(directory, now_unix_ms).map(|(name, _)| {
        format!(
            "(For you to know, not to answer: you have asked the person whether to put the skill \"{name}\" on the lamp. If they say yes now, call {PUT_ON}; if they say no, don't, and it stays off.)"
        )
    })
}

/// The function the model calls on the person's yes to [`ASK_TO_PUT_ON`].
pub fn put_on_tool() -> lamp_gemini::Tool {
    lamp_gemini::Tool {
        name: PUT_ON.into(),
        description: format!(
            "Put on the lamp the skill it just made, which you asked about (\"{ASK_TO_PUT_ON}\"). \
             Call it only when the person has just said yes to that question out loud."
        ),
        parameters: None,
        silent: false,
    }
}

/// On the person's yes ([`PUT_ON`]), put on the made skill waiting for it,
/// by its SHA-256 alone, through the manage helper at `socket`, which
/// installs only those bytes, held again to the cap. Only a yes said out
/// loud counts, and only a plain one (`said`: the words heard in the line,
/// `None` for one not heard). The lead's decision is a spoken yes, so a
/// typed one, or one the lamp wrote, puts nothing on, and neither does a
/// "no, not yet" the model took for a yes (the reviewer's P3 on #2424, as
/// the held forget's plain yes). What the model is told.
pub fn put_on(directory: &Path, socket: &Path, said: Option<&str>, now_unix_ms: u64) -> Value {
    put_on_within(directory, socket, said, now_unix_ms, PUT_ON_WAIT)
}

/// [`put_on`], waiting at most `wait` for the helper.
pub fn put_on_within(
    directory: &Path,
    socket: &Path,
    said: Option<&str>,
    now_unix_ms: u64,
    wait: Duration,
) -> Value {
    let Some(said) = said else {
        return json!({
            "error": "only a yes said out loud puts a made skill on the lamp",
            "next": "Tell them in a few words to say yes out loud at the lamp to put it on."
        });
    };
    if !crate::known_faces::plain_agreement(said) {
        return json!({
            "error": "what they said was not a plain yes",
            "next": "Ask them once more, in a few words, whether to put it on the lamp."
        });
    }
    let Some((name, sha256)) = waiting(directory, now_unix_ms) else {
        return json!({
            "error": "no skill made here is waiting for a yes",
            "next": "Tell them in a few words that there is nothing waiting to be put on."
        });
    };
    // A helper not there (no socket, or nobody listening) never puts it on:
    // only one asked and silent may still (the reviewer's P3 on #2424).
    let Some(answer) = crate::tools::reached(socket, &json!({ "install_made": sha256 }), wait)
    else {
        eprintln!("lamp-voice: a made skill: no skills helper to put it on: {name}");
        return json!({
            "error": "it couldn't be put on",
            "next": "Tell them in one short sentence that it couldn't be put on the lamp, with no technical words."
        });
    };
    match answer.as_ref().map(|answer| answer.get("done").is_some()) {
        // No answer in time: the helper may still put it on, so the person
        // is told that, not that it failed.
        None => {
            eprintln!("lamp-voice: a made skill: the skills helper didn't answer in time: {name}");
            json!({
                "error": "the lamp didn't say in time whether it is on",
                "next": "Tell them in a few words that it is being put on, and that they can ask in a minute whether it is on."
            })
        }
        Some(true) => {
            let mut entries = index(directory);
            for entry in &mut entries {
                if entry["sha256"] == sha256.as_str() {
                    entry["on"] = json!(true);
                }
            }
            if let Err(error) = write_index(directory, &entries) {
                eprintln!("lamp-voice: a made skill is on, but not marked so: {error}");
            }
            eprintln!("lamp-voice: a made skill is on the lamp: {name}, sha256 {sha256}");
            json!({
                "put_on": name,
                "next": "Tell them in a few words that it is on the lamp now."
            })
        }
        Some(false) => {
            let why = answer
                .as_ref()
                .and_then(|answer| answer["refused"].as_str())
                .unwrap_or("");
            eprintln!("lamp-voice: a made skill was not put on: {name}: {why}");
            // One they put on another way has its name (#2414): said, not
            // replaced.
            if why.contains("put on another way") {
                return json!({
                    "error": format!("a skill called {name} is already on the lamp, put on another way"),
                    "next": format!("Tell them in one short sentence that a skill called {name} is already on the lamp, so this one wasn't put on in its place.")
                });
            }
            json!({
                "error": "it couldn't be put on",
                "next": "Tell them in one short sentence that it couldn't be put on the lamp, with no technical words."
            })
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

    /// A skill asked to use any linked service is never made: the person
    /// is told a skill can't, that the lamp itself can, and where it is
    /// linked; a word that only holds one, or a plain word that is also a
    /// service's name, is still a skill.
    #[test]
    fn a_skill_for_a_linked_service_points_to_asking_the_lamp() {
        for request in [
            "read my mail every morning",
            "Glow blue when I get an E-mail",
            "check my Gmail inbox",
            "nod when my calendar has a meeting",
            "glow red when Slack has a message",
            "open my Google Drive file",
            "nod when a GitHub build fails",
            "glow when my Notion page changes",
        ] {
            assert_eq!(brief(request), Err(NOT_A_SERVICE), "{request}");
        }
        // Not possible yet, linked or not: no place to link it is named.
        assert!(NOT_A_SERVICE.contains("yet") && NOT_A_SERVICE.contains("already linked"));
        assert!(!NOT_A_SERVICE.contains("Connectors"));
        for request in [
            "glow warm when the mailman comes",
            "dim the light at 10 pm",
            "nod every monday morning",
            "glow when I drive home",
        ] {
            assert!(brief(request).is_ok(), "{request}");
        }
    }

    /// #5288: with a made skill's reads on, a request to be read a calendar
    /// or mail is briefed with its one read step, within [`MOST_BRIEF_BYTES`]
    /// bytes; a write, two services or one with nothing a skill reads are
    /// told that limit (the testers' sheet, 6 Nov), and with the switch off
    /// every one is [`NOT_A_SERVICE`], as before.
    #[test]
    fn with_reads_on_a_calendar_or_mail_read_is_briefed() {
        let calendar = "every evening at nine, read me tomorrow's calendar";
        let brief = brief_with(calendar, true).unwrap();
        assert!(is_brief(&brief) && request_of(&brief) == calendar);
        assert!(brief.contains(
            "read = { service = \"google_calendar\", what = \"events\", day = \"tomorrow\" }"
        ));
        assert!(brief.contains("services = { google_calendar = \"read\" }"));
        let mail = brief_with("every morning read me my mail", true).unwrap();
        assert!(mail.contains("read = { service = \"gmail\", what = \"recent\" }"));
        assert!(mail.contains("services = { gmail = \"read\" }"));
        assert!(
            brief_with("what is on my calendar, each morning", true)
                .unwrap()
                .contains("day = \"today\"")
        );
        // Every other linked service: its reads listed, never a whole text.
        // Notion is read only through its MCP server's tools (#5308), which
        // a made skill can't read yet.
        assert_eq!(
            brief_with("each morning read me my Notion plans", true),
            Err(NOT_READABLE)
        );
        let drive = brief_with("read me my Google Drive files at nine", true).unwrap();
        assert!(drive.contains("services = { google_drive = \"read\" }"));
        // A lamp, 5 Nov: "read my latest Google Drive files" was made with
        // query = "order by modifiedTime desc", which matched no file. Drive's
        // step is given whole: newest first, query only search words.
        assert!(
            drive.contains(&format!("one [[do]] step: {DRIVE_READ}. SKILL.md")),
            "{drive}"
        );
        assert!(
            DRIVE_READ.contains("what = \"files\" }, no other key")
                && DRIVE_READ.contains("newest first")
                && DRIVE_READ.contains("query is search words")
                && !drive.contains("text {id}")
                && !drive.contains("file {id}")
        );
        // On 10300 the note's "(newest first; …)" right after the braces
        // came back as one more key in them, twice. The note now says a
        // field's word nowhere a key could be read from it.
        assert!(!DRIVE_READ.contains('(') && !DRIVE_READ.contains("newest_first"));
        let latest = brief_with("read my latest Google Drive files", true).unwrap();
        assert!(latest.contains(DRIVE_READ), "{latest}");
        // Thirteen's P2 on #5303: fixed reads only, no query, path or call.
        let github = brief_with("each morning read me my GitHub issues", true).unwrap();
        assert!(github.contains("notifications; issues {repo}; pulls {repo}; repos, its keys"));
        assert!(!github.contains("query") && !github.contains("graphql"));
        // P3.3: the time of day is the request's. No time is shown: lamp-9adf,
        // 5 Nov 01:00, "when I come back to my desk" was made with the shown
        // 08:00 to 08:30.
        for brief in [&brief, &mail, &github, &drive] {
            assert!(brief.contains("only for a time asked for"), "{brief}");
            assert!(
                !brief.contains("21:00") && !brief.contains("08:00"),
                "{brief}"
            );
        }
        // The testers' sheet, 6 Nov ("a skill is limited with no reason"):
        // with reads on, "a skill can't use your mail" is false, so each is
        // told its own limit: a read only, of one service that has one.
        for (request, limit) in [
            ("glow red when Slack has a message", NOT_READABLE),
            ("read me my Ahrefs numbers", NOT_READABLE),
            ("read me my HubSpot deals", NOT_READABLE),
            ("every friday draft my weekly mail to the team", NOT_A_WRITE),
            (
                "when I say wrap up add tomorrow's first meeting to my calendar",
                NOT_A_WRITE,
            ),
            ("send my calendar to the team", NOT_A_WRITE),
            ("read my calendar and my mail", ONE_SERVICE),
            ("read my Notion page and my GitHub issues", ONE_SERVICE),
            ("post my Notion page to Slack", NOT_A_WRITE),
            // Asking for the skill is no write, but a write after it still is.
            (
                "create a skill that adds a meeting to my calendar",
                NOT_A_WRITE,
            ),
            ("write me a skill to send my mail", NOT_A_WRITE),
            ("schedule a meeting on my calendar", NOT_A_WRITE),
            // Heron's P3: a write before "skill" is never dropped with the ask.
            (
                "send my mail with a skill that reads my calendar",
                NOT_A_WRITE,
            ),
        ] {
            assert_eq!(brief_with(request, true), Err(limit), "{request}");
        }
        // Each says what a skill can do, and none that it can't use a
        // service at all.
        for limit in [NOT_A_WRITE, ONE_SERVICE, NOT_READABLE] {
            assert!(limit.contains("a skill can read"), "{limit}");
            assert!(!limit.contains("can't use"), "{limit}");
            assert!(
                limit.ends_with("You can ask me for it yourself any time."),
                "{limit}"
            );
        }
        // How people ask for one (lamp-9adf, 4 Nov 23:51): "create a skill
        // that ..." and "my schedule" are no writes.
        for request in [
            "Create a skill that reads my calendar for tomorrow when I come back to my desk",
            "Add a new skill to read me my mail each morning",
            "Lampy, make me a skill: read my calendar schedule for tomorrow at my desk",
            "when I sit down, read me tomorrow's schedule from my calendar",
            // El's P3 on #5679: "post" and "book" said as the thing read, and
            // "put" with no service after it, are no writes either.
            "read me my latest Facebook post",
            "read me the book titles in my Google Drive",
            "read me my mail and put the count on the light",
            // El's P2 on #5696: a word after the thing doesn't make it a
            // verb after "my", "the", "latest", "last" or "newest".
            "read me my schedule on my calendar",
            "read me the schedule on my calendar",
            "read me my latest post on Facebook",
            "what's the latest post on Facebook",
            "read me the newest post on my Facebook page",
            "read the post on my page on Facebook",
            "read me the last post to my Facebook page",
            "read me the latest post that my Facebook page has",
            "read the post it got on Facebook",
            "read me the post a friend left on Facebook",
            "read me the last post saying sale on Facebook",
            "read my schedule from my calendar",
            "read me tomorrow's schedule from my calendar",
            "what's on my schedule today in my calendar",
            "read me my calendar schedule for tomorrow",
            "read me the files in my Google Drive and put the count on the light",
            "read me my mail and put the count on the light from my drive",
        ] {
            assert!(brief_with(request, true).is_ok(), "{request}");
        }
        // As verbs, they still write.
        for request in [
            "post my mail to Facebook",
            "book a meeting on my calendar",
            "put a meeting on my calendar",
            "read my mail and post it on Facebook",
            // El's P3 on #5690: a word that makes it the thing read, then
            // what it writes.
            "my calendar, book a meeting",
            "my calendar schedule a meeting",
            "tomorrow book a meeting on my calendar",
            "today post a photo to Facebook",
            "each day post a hello on Facebook",
            "the calendar book me a dentist at nine",
            "at the end of the day book a room on my calendar",
            "facebook post that I'm out",
            "on my Facebook page post that we are closed",
            "latest post: say hi on Facebook",
            "last post on facebook say we are open",
            "newest post to facebook saying hi",
            "read my mail then put it in my drive",
            // El's P3s on #5696: any object after "tomorrow", "today",
            // "day", "calendar", "facebook" or "page" and the verb.
            "today post my photo to Facebook",
            "today post this on Facebook",
            "today post them to Facebook",
            "today post something on Facebook",
            "facebook post this",
            "facebook post them",
            "facebook post something nice",
            "facebook post hello world",
            "page post we are open today on Facebook",
            "tomorrow book my flight on my calendar",
            "tomorrow book us a table on my calendar",
            "my calendar book us a table",
            "my calendar book the room",
            "tomorrow book the room on my calendar",
            "my calendar book dinner",
            "the calendar book in a meeting",
            "the calendar schedule my week",
            "my calendar schedule my week",
            "my calendar schedule lunch with Sam",
            "tomorrow schedule lunch on my calendar",
            "calendar schedule dinner at seven",
            "post my photo to Facebook",
            "tomorrow post an update on Facebook",
            "facebook post saying hi",
            "read my mail and put the file in Google Drive",
        ] {
            assert_eq!(brief_with(request, true), Err(NOT_A_WRITE), "{request}");
        }
        for request in [
            calendar,
            "every morning read me my mail",
            "read me my Notion plans",
        ] {
            assert_eq!(brief_with(request, false), Err(NOT_A_SERVICE), "{request}");
        }
        for service in [
            "calendar tomorrow",
            "GitHub",
            "Google Drive",
            "Asana",
            "Facebook",
        ] {
            let words = format!("read my {service} ");
            let longest = format!("{words}{}", "x".repeat(MOST_REQUEST_BYTES - words.len()));
            let brief = brief_with(&longest, true).unwrap();
            assert!(
                brief.len() <= MOST_BRIEF_BYTES,
                "{service}: {}",
                brief.len()
            );
        }
    }

    /// A made skill that reads (#5288).
    const READ_TOML: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n[permissions]\n\
        speak = true\nservices = { google_calendar = \"read\" }\ntopics = [\"voice/state\"]\n\
        [trigger]\ntopic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n\
        during = { from = \"21:00\", to = \"21:30\" }\n[ask]\npolicy = \"nudge\"\n\
        [[do]]\nread = { service = \"google_calendar\", what = \"events\", day = \"tomorrow\" }\n";

    /// #5288: its cap takes one linked service, as a read, and its voice
    /// for saying it, only with the switch on; its spoken yes names the
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
        assert!(files_err(&extra).contains("can't have"));
        let twice = format!("{}```skill.toml\n{TOML}```\n", answer(TOML, CARD));
        assert!(files_err(&twice).contains("twice"));
        assert!(files_err(&format!("```skill.toml\n{TOML}```\n")).contains("both"));
        assert!(files_err(&format!("```skill.toml\n{TOML}")).contains("cut off"));
        let long = format!(
            "```skill.toml\n{}\n```\n```SKILL.md\n{CARD}```\n",
            "#".repeat(9_000)
        );
        assert!(files_err(&long).contains("too long"));
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
            assert_eq!(why, "its skill.toml can't be read");
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
            ("speak = true", "more than the light"),
            ("look = true", "more than the light"),
            ("messages = true", "more than the light"),
            ("known_faces = true", "more than the light"),
            ("actions = [\"volume_up\"]", "more than the light"),
            ("frobs = true", "can't be read"),
        ] {
            let text = TOML.replace("light = true\n", &format!("light = true\n{wider}\n"));
            assert!(capped(&text).unwrap_err().0.contains(why), "{wider}");
        }
        let faces = TOML.replace(
            "[\"voice/state\"]",
            "[\"voice/state\", \"humans/faces/tracked\"]",
        );
        assert!(capped(&faces).unwrap_err().0.contains("read more"));
        let heard = TOML.replace("[\"voice/state\"]", "[\"voice/heard\"]");
        assert!(capped(&heard).unwrap_err().0.contains("read more"));
        let tool = format!("tool = \"dim_now\"\n{TOML}");
        assert!(capped(&tool).unwrap_err().0.contains("function"));
        let core = TOML.replace("installable", "core");
        assert!(capped(&core).unwrap_err().0.contains("installable"));
        let reminder = TOML.replace("policy = \"nudge\"", "policy = \"reminder\"");
        assert!(
            capped(&reminder)
                .unwrap_err()
                .0
                .contains("other than a nudge")
        );
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
    /// repair: the person is never asked about something they weren't told.
    #[test]
    fn what_it_cant_say_is_refused_with_lines_for_a_repair() {
        let refused = |toml: &str| match plainly_with(toml, false) {
            Err(Refused(why, Some(lines))) => {
                assert!(why.contains("can't say plainly"), "{why}");
                assert_eq!(lines, PLAIN_ONLY);
            }
            other => panic!("{other:?}"),
        };
        refused(&TOML.replace("cooldown", "once_per_day = true\ncooldown"));
        refused(&TOML.replace("equals = \"resting\"", "equals = \"talking\""));
        refused(&TOML.replace("voice/state\"\nfield", "room/activity\"\nfield"));
        refused(&TOML.replace("{ dim = 1 }", "{ dim = 1, for = \"1m\" }"));
        refused(&TOML.replace("light = { dim = 1 }", "say = \"good night\""));
        refused(&TOML.replace("[[do]]\nlight = { dim = 1 }\n", ""));
        refused(&TOML.replace("cooldown = \"12h\"", "cooldown = \"1h30m\""));
        refused(&TOML.replace(
            "during = { from = \"22:00\", to = \"22:30\" }",
            "during = {}",
        ));

        // Thirteen's and Mobile's P2s on #5042: a part that would run
        // otherwise than the words, and more steps than one breath says,
        // never cut.
        refused(&format!(
            "{TOML}\n[[while]]\ntopic = \"room/presence\"\nfield = \"someone\"\nequals = true\n"
        ));
        refused(&format!(
            "{TOML}\n[quiet]\nfrom = \"21:00\"\nto = \"08:00\"\n"
        ));
        refused(&TOML.replace("version = ", "most_a_day = 1\nversion = "));
        refused(&TOML.replace("version = ", "enabled = false\nversion = "));
        refused(&TOML.replace("[trigger]", "[[trigger]]"));
        let many = "\n[[do]]\nlight = { colour = \"warm\", level = 2, for = \"20s\" }\n".repeat(6);
        refused(&format!(
            "{TOML}{many}\n[[do]]\nbody = {{ gesture = \"nod\" }}\n"
        ));
    }

    /// With reads on, a read refused as unsaid goes back with lines that
    /// keep the brief's read step, so the repair doesn't take the read away
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
        assert!(why.contains("can't say plainly"), "{why}");
        assert_eq!(lines, format!("{PLAIN_ONLY}{PLAIN_OR_READ}"));
        assert!(
            lines.contains("the brief's read step exactly as the brief gives it"),
            "{lines}"
        );
        assert!(lines.len() <= MOST_REPAIR_BYTES, "{}", lines.len());
        let repair = repair_brief(&"x".repeat(MOST_REQUEST_BYTES), &lines).unwrap();
        assert!(repair.contains(&lines), "{repair}");
        assert!(repair.len() <= crate::agent_tasks::MOST_TASK_BYTES);
        let Err(Refused(_, Some(off))) = plainly_with(&mail, false) else {
            panic!("a read is unsaid with reads off");
        };
        assert_eq!(off, PLAIN_ONLY);
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
        assert_eq!(
            why,
            "a skill can't do that: a skill can't hear words like \"good night\"."
        );
        let long = format!("cannot: {}", "x ".repeat(200));
        assert!(files(&long).unwrap_err().0.chars().count() < 160);
        let both = format!("cannot: no\n{}", answer(TOML, CARD));
        assert!(files(&both).is_ok());
        // In any case (Mobile's P3 on #5042).
        assert!(
            files("Cannot: hear words.")
                .unwrap_err()
                .0
                .ends_with(": hear words.")
        );
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
                .contains("cut off")
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
        };
        let refused = keep(&answer(TOML, CARD), false, "s:1:2", &keeping).unwrap_err();
        assert_eq!(refused.0, "it doesn't pass the lamp's check");
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
        };
        let began = Instant::now();
        let kept = keep(&answer(TOML, CARD), false, "s:1:4", &keeping);
        assert_eq!(kept.unwrap_err().0, "it doesn't pass the lamp's check");
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

    /// The ask waits [`PUT_ON_WITHIN`], the newest ask alone, and the note
    /// says which skill a yes would put on.
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

    /// lamp-9adf, 31 Oct 09:32: the news of `posture-glow` untold, asked "Is
    /// the skill I asked for ready yet?", the model said the ask in its own
    /// answer, and the yes after it found nothing waiting. Now that answer
    /// keeps the newest untold skill as asked when it was said, so the yes
    /// is held to the usual two minutes; another question's yes, an answer
    /// too long ago, or an older skill than the newest ask, keeps nothing
    /// (the Reviewer-lead's P2 on #4880).
    #[test]
    fn the_lamps_own_answer_saying_the_ask_keeps_the_untold_skill_asked() {
        let dir = scratch("mk-in-answer");
        let made = dir.join("made");
        fs::create_dir_all(&made).unwrap();
        let kept = |name: &str, sha: char, at: u64| {
            json!({"name": name, "sha256": sha.to_string().repeat(64),
                   "package": format!("{name}.tar"), "from": format!("s1:{at}:1"),
                   "at_unix_ms": at})
        };
        write_index(
            &made,
            &[
                kept("evening-dim", 'a', 1_000),
                kept("posture-glow", 'b', 2_000),
            ],
        )
        .unwrap();
        let answer = format!("It glows warm once an hour. {ASK_TO_PUT_ON}");
        let now = 300_000;
        // Another question's yes keeps nothing.
        assert!(
            !asked_in_answer(
                &made,
                "Shall I set that reminder?",
                Duration::from_secs(5),
                now
            )
            .unwrap()
        );
        assert_eq!(waiting(&made, now), None);
        // Said too long ago keeps nothing.
        assert!(
            !asked_in_answer(&made, &answer, PUT_ON_WITHIN + Duration::from_secs(1), now).unwrap()
        );
        assert_eq!(waiting(&made, now), None);
        // Two untold: the answer may have named either ("is evening-dim
        // ready?"), so neither is asked, and a yes finds nothing waiting.
        assert!(!asked_in_answer(&made, &answer, Duration::from_secs(5), now).unwrap());
        assert_eq!(waiting(&made, now), None);
        // One untold after the newest ask: it is asked then, and waits.
        offered(&made, &"a".repeat(64), 1_500).unwrap();
        assert!(asked_in_answer(&made, &answer, Duration::from_secs(5), now).unwrap());
        let newest = Some(("posture-glow".to_owned(), "b".repeat(64)));
        assert_eq!(waiting(&made, now), newest);
        assert_eq!(index(&made)[1]["offered_at_unix_ms"], json!(now - 5_000));
        // Asked now, the older untold one never takes its place.
        assert!(!asked_in_answer(&made, &answer, Duration::from_secs(1), now + 1_000).unwrap());
        assert_eq!(waiting(&made, now + 1_000), newest);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_ask_waits_and_only_for_two_minutes() {
        let dir = scratch("mk-wait");
        let made = dir.join("made");
        two_kept(&made);
        assert_eq!(waiting_note(&made, 1_000), None);
        offered(&made, &"a".repeat(64), 1_000).unwrap();
        offered(&made, &"b".repeat(64), 5_000).unwrap();
        let within = PUT_ON_WITHIN.as_millis() as u64;
        assert_eq!(
            waiting(&made, 6_000),
            Some(("morning-glow".to_owned(), "b".repeat(64)))
        );
        let note = waiting_note(&made, 6_000).unwrap();
        assert!(
            note.contains("\"morning-glow\"") && note.contains(PUT_ON),
            "{note}"
        );
        assert_eq!(
            waiting(&made, 5_000 + within),
            Some(("morning-glow".to_owned(), "b".repeat(64)))
        );
        assert_eq!(waiting(&made, 5_001 + within), None);
        // An ask for a SHA-256 nothing kept changes nothing.
        offered(&made, &"c".repeat(64), 7_000).unwrap();
        assert_eq!(waiting(&made, 7_000).unwrap().0, "morning-glow");
    }

    /// On a spoken yes, the helper is asked for the waiting skill's SHA-256
    /// alone, and once it is on, nothing waits.
    #[test]
    fn a_spoken_yes_asks_the_helper_for_that_sha256_and_nothing_else() {
        let dir = scratch("mk-yes");
        let made = dir.join("made");
        two_kept(&made);
        offered(&made, &"a".repeat(64), 1_000).unwrap();
        let socket = dir.join("manage.sock");
        let asked = helper(&socket, r#"{"done": "installed evening-dim"}"#);
        let told = put_on(&made, &socket, Some("Yes."), 2_000);
        assert_eq!(told["put_on"], "evening-dim", "{told}");
        let asked: Value = serde_json::from_str(&asked.join().unwrap()).unwrap();
        assert_eq!(asked, json!({"install_made": "a".repeat(64)}));
        assert_eq!(waiting(&made, 2_000), None, "on, so nothing waits");
        let again = put_on(&made, &socket, Some("Yes."), 2_000);
        assert_eq!(
            again["error"], "no skill made here is waiting for a yes",
            "{again}"
        );
    }

    /// A yes not said out loud, or no ask waiting, asks the helper nothing;
    /// a helper's refusal leaves the skill off and still waiting.
    #[test]
    fn no_spoken_yes_or_no_ask_puts_nothing_on_and_a_refusal_is_told() {
        let dir = scratch("mk-no");
        let made = dir.join("made");
        two_kept(&made);
        let socket = dir.join("nobody.sock");
        let told = put_on(&made, &socket, Some("Yes."), 2_000);
        assert_eq!(
            told["error"], "no skill made here is waiting for a yes",
            "{told}"
        );
        offered(&made, &"a".repeat(64), 1_000).unwrap();
        let typed = put_on(&made, &socket, None, 2_000);
        assert_eq!(
            typed["error"], "only a yes said out loud puts a made skill on the lamp",
            "{typed}"
        );
        let not_yes = put_on(&made, &socket, Some("No, not yet."), 2_000);
        assert_eq!(
            not_yes["error"], "what they said was not a plain yes",
            "{not_yes}"
        );
        // No helper there: it is not being put on (the reviewer's P3 on #2424).
        let nobody = put_on(&made, &socket, Some("Yes."), 2_000);
        assert_eq!(nobody["error"], "it couldn't be put on", "{nobody}");
        // A helper asked and silent may still put it on.
        let socket = dir.join("silent.sock");
        let _listening = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let silent = put_on_within(
            &made,
            &socket,
            Some("Yes."),
            2_000,
            Duration::from_millis(200),
        );
        assert_eq!(
            silent["error"], "the lamp didn't say in time whether it is on",
            "{silent}"
        );
        let socket = dir.join("manage.sock");
        let asked = helper(
            &socket,
            r#"{"refused": "not installed: its SHA-256 has changed"}"#,
        );
        let refused = put_on(&made, &socket, Some("Yes."), 2_000);
        asked.join().unwrap();
        assert_eq!(refused["error"], "it couldn't be put on", "{refused}");
        assert_eq!(waiting(&made, 2_000).unwrap().0, "evening-dim");
        let socket = dir.join("in-the-way.sock");
        let asked = helper(
            &socket,
            r#"{"refused": "not installed: evening-dim is already on the lamp, put on another way, and a skill made here doesn't take its place"}"#,
        );
        let in_the_way = put_on(&made, &socket, Some("Yes."), 2_000);
        asked.join().unwrap();
        assert_eq!(
            in_the_way["error"],
            "a skill called evening-dim is already on the lamp, put on another way",
            "{in_the_way}"
        );
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
        assert!(capped(&text).unwrap_err().0.contains("function"));
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

    /// Kept as a made skill is, with what it does said plainly; nothing
    /// waits for a yes until the lamp has asked, and nothing goes into the
    /// skills folder: the function is declared only once a spoken yes puts
    /// it on.
    #[test]
    fn a_written_skill_is_kept_and_waits_for_nothing_before_it_is_asked() {
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
        assert_eq!(waiting(&keeping.directory, now_ms()), None);
        assert_eq!(fs::read_dir(&installed).unwrap().count(), 0);
        // The same ask again: the kept one.
        let again = keep_written(&study_mode(), "written:app:1:1", &keeping, &installed).unwrap();
        assert_eq!(again, made);
        // Asked about: only then does a yes find it.
        offered(&keeping.directory, &made.sha256, now_ms()).unwrap();
        assert_eq!(
            waiting(&keeping.directory, now_ms()).map(|(name, _)| name),
            Some("study-mode".to_owned())
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// A name any skill on the lamp has, installed or pushed, is refused, and
    /// nothing is kept. A kept one of that name not on, asked about or not,
    /// is replaced by the new one, its package and its offer with it: a no
    /// leaves no mark, so a fix sent after it goes at once (Seventh's P2 on
    /// #5072).
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
        // Asked about and said no to, within its time: a fix goes at once,
        // and the old offer is gone, so a late yes puts nothing on.
        offered(&keeping.directory, &first.sha256, now_ms()).unwrap();
        assert!(waiting(&keeping.directory, now_ms()).is_some());
        let fixed = keep_written(&study_mode(), "written:app:3:2", &keeping, &installed).unwrap();
        assert_eq!(index(&keeping.directory).len(), 1);
        assert_eq!(waiting(&keeping.directory, now_ms()), None);
        // Its ask ran out with no yes: the new one takes its place too.
        let long_ago = now_ms() - PUT_ON_WITHIN.as_millis() as u64 - 1_000;
        offered(&keeping.directory, &fixed.sha256, long_ago).unwrap();
        let first = fixed;
        // An older one of that name, said no to, with a package of its own.
        let older = keeping.directory.join("study-mode-0.9.0-000000000000.tar");
        fs::write(&older, b"older").unwrap();
        let mut entries = index(&keeping.directory);
        entries.insert(
            0,
            json!({"from": "written:app:0:1", "name": "study-mode", "package": "study-mode-0.9.0-000000000000.tar", "sha256": "0", "offered_at_unix_ms": long_ago}),
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
        offered(&keeping.directory, &second.sha256, long_ago).unwrap();
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
        assert_eq!(
            needs_link_said(&[json!({"code": "gmail", "name": "Gmail"})]).as_deref(),
            Some(
                "Gmail isn't linked yet: link it in the app, under Skills, then Connectors, and \
                 the skill will start then."
            )
        );
        assert_eq!(needs_link_said(&[]), None);
    }
}
