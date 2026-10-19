//! Work handed to an agent on another computer: the voice's side of
//! `lamp-tasks` (`docs/tasks.md`), which starts a Claude Code session there
//! through Harness.
//!
//! Two functions for the model. [`START`] prepares the task: nothing runs,
//! and the model is told to read it back and ask. [`CONFIRM`] starts it, and
//! only the voice decides whether it may: the person must have said
//! something to the lamp after the task was prepared (a request the lamp
//! heard, not one it wrote itself, or a text typed in the same chat in the
//! Autonomous app, `crate::messages`), within [`CONFIRM_WITHIN`]. The model
//! hears the yes; the voice checks that there was one to hear. The
//! confirmation goes on the tasks part's own socket for the voice, which
//! nothing the model can name reaches.
//!
//! Off unless set: `LAMP_TASKS_DIR=/absolute/directory` holding `lamp-tasks`'s
//! `request.sock` and `voice.sock`, passed to the provider worker as
//! `--tasks-dir DIR`. The target is the tasks part's alias `office` unless
//! `LAMP_TASKS_TARGET` names another.
//!
//! Who gets the work (`docs/tasks.md`, "Who gets the work"), off unless
//! `LAMP_TASKS_WHO=on` (`--tasks-who`): the person is the boss and the
//! agents are the staff ("ask Codex to ...", "have Claude Code ..."). The
//! staff are read from the tasks part's targets (`{"op":"targets"}`, alias
//! and engine) when the provider worker starts and every 30 s after, never
//! written into the prompt. With two or more, [`START`] and [`CHECK`] take
//! `who`, one of their names, and nobody named is Claude Code. The model
//! may hear "Codex" as "codecs" and "Claude" as "cloud": `who` is an enum
//! of the names, its description says what each may sound like, and the
//! voice takes a close form of a name only when it is one member's and
//! nobody else's. A name that is none of them prepares nothing, and the
//! model is told to ask. With fewer than two, or the staff not yet read,
//! the functions and everything sent to the tasks part are as without the
//! switch.
//!
//! A lead-in (#799), off unless `LAMP_TOOL_LEAD_IN=on` (`--tasks-lead-in`):
//! a turn with a function call is answered about a second later than a plain
//! one, because the model makes the call in one generation and speaks in
//! another, after the answer. With the switch the model is asked to say the
//! start of its answer in the same breath, before the call: the read-back
//! before `start_agent_task`, "giving that to …" before
//! `confirm_agent_task`, "let me see" before `check_agent_task`; and the
//! answers' `next` tell it the person has heard that, so it is not said
//! twice. Words that carry the answer, not fillers. On lamp-4ace on
//! 2026-10-16 (00:08 to 00:11), with the lead-in, work asked of the staff
//! and two of them to choose from, nothing was called in either round, not
//! even [`CHECK`], and the model made up what became of the work: [`START`]'s
//! description was 1,036 bytes, over the 1,024 the link takes, and a list
//! the link refused left the session with no function at all
//! (`thread::equipped`; now only that one is left out, and the journal says
//! so). Its lead-in sentence is now the shortest that says
//! it, and a test holds every description under the limit for every
//! combination of the switches. The read-back is a statement, and the
//! question whether to start comes after the call, from its answer, while
//! the read-back still plays: a question ends a spoken turn.
//!
//! Asked of the staff (#854), off unless `LAMP_TASKS_ASKED=on`
//! (`--tasks-asked`): on lamp-4ace on 2026-10-15 the person asked "can you
//! ask Claude Code to write a short story …" twice and the lamp told the
//! story itself, then went on with it when asked again; a name it could not
//! make out ("your go") was taken for Codex. The persona asks for stories to
//! be told gladly, and [`START`] was for "computer work … code, documents or
//! files", which a story is easy to think is not. With the switch:
//! [`START`] is called whenever the person asks to have one of the staff do
//! something, whatever the work and even in the middle of a story; a new
//! session is told so after the persona ([`session_note`]); and a name the
//! model cannot quite make out is given as the one it thinks was meant,
//! without a question first: the read-back names who will do it, and
//! nothing starts without a yes, so the person can put it right. On
//! lamp-4ace on 2026-10-15 (21:33) a "not sure" choice offered for such a
//! name was never taken, in played scenes or in the model check, and the
//! model asked about its guess before the read-back as well, so twice.
//!
//! A story asked of the lamp is the lamp's (#854, reopened 2026-10-16): with
//! the switch, on lamp-4ace at 02:00 to 02:12, "tell me a story about a
//! clever fox, at least eight sentences long", asked of the lamp, got
//! "Claude Code can write that story. Should I start it?", and a session
//! that began offering stories went on offering every one. The session note
//! ([`session_note`]) now says, after its hand-over sentence, word for word
//! as before, that what is asked of the lamp without naming the staff is
//! the lamp's, however long, even after a hand-over. Only the note: two
//! heads of #903 that changed [`START`] lost the hand-overs on lamp-4ace,
//! one by beginning "call it only when …" ("I can't write code or use your
//! office computer", 2 of 2), the other by giving it two more required
//! arguments (no call at all, 0 of 2 named ones admitted, and twice "I've passed that along"
//! said without one). [`START`] is as it was.
//!
//! Hearing back: the tasks part watches what it started (on unless `serve --no-watch`),
//! [`crate::task_news`] tells the person unasked when the agent has
//! finished or asks something, and [`CHECK`] answers "how's it going?" from
//! what was last seen, at once. What the person heard through [`CHECK`] is
//! kept as told, so it is not told again unasked.
use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Where the coordinator reads the tasks part's directory.
pub const ENVIRONMENT: &str = "LAMP_TASKS_DIR";
/// Where it reads the target's alias, if not [`DEFAULT_TARGET`].
pub const TARGET_ENVIRONMENT: &str = "LAMP_TASKS_TARGET";
/// Where it reads whether the person may name who gets the work. `on` only.
pub const WHO_ENVIRONMENT: &str = "LAMP_TASKS_WHO";
/// Where it reads whether the model says the start of its answer before a
/// call (#799). `on` only.
pub const LEAD_IN_ENVIRONMENT: &str = "LAMP_TOOL_LEAD_IN";
/// Where it reads whether work asked of the staff is handed over every time
/// (#854). `on` only.
pub const ASKED_ENVIRONMENT: &str = "LAMP_TASKS_ASKED";
/// Where it reads whether the person's own accounts (calendar, email) are
/// asked of the computer's agent, and the computer is not called the
/// office's (#1676). `on` only, and only with [`ASKED_ENVIRONMENT`].
pub const ACCOUNTS_ENVIRONMENT: &str = "LAMP_TASKS_ACCOUNTS";
/// Where it reads whether the person's computers are called by their own
/// names ("your MacBook Air"), from the tasks part's `targets.names.json`
/// (#1676). `on` only.
pub const NAMES_ENVIRONMENT: &str = "LAMP_TASKS_MACHINE_NAMES";
/// The provider worker's flags for them.
pub const FLAG: &str = "--tasks-dir";
pub const TARGET_FLAG: &str = "--tasks-target";
pub const WHO_FLAG: &str = "--tasks-who";
pub const LEAD_IN_FLAG: &str = "--tasks-lead-in";
pub const ASKED_FLAG: &str = "--tasks-asked";
pub const ACCOUNTS_FLAG: &str = "--tasks-accounts";
pub const NAMES_FLAG: &str = "--tasks-machine-names";
/// The file the tasks part needs to run at all (#811: on a home lamp, the
/// Harness program its unit's condition names). While it is missing, a call
/// that cannot reach the part is told it isn't set up; otherwise as a part
/// not running. Unset, always the latter.
pub const NEEDS_ENVIRONMENT: &str = "LAMP_TASKS_NEEDS";
pub const NEEDS_FLAG: &str = "--tasks-needs";
pub const DEFAULT_TARGET: &str = "office";
/// The model's two functions.
pub const START: &str = "start_agent_task";
pub const CONFIRM: &str = "confirm_agent_task";
pub const CHECK: &str = "check_agent_task";
/// A yes counts this long after the task was prepared.
pub const CONFIRM_WITHIN: Duration = Duration::from_secs(120);
/// The longest task text the tasks part takes.
const MOST_TASK_BYTES: usize = 1_000;
/// The longest the first read of the staff holds up the provider worker's
/// start, as for the memory's: the tasks part answered a request on
/// lamp-4ace in 35 ms (`docs/tasks.md`), and a socket nobody serves fails
/// at once.
const FIRST_READ: Duration = Duration::from_millis(300);
/// The longest a call about work waits for the tasks part, every try
/// included: longer than [`crate::tools::ASK_DEADLINE`], so that a change
/// the tasks part is unsure of after its own 1.1 s can be asked for again.
pub const DEADLINE: Duration = Duration::from_millis(3_500);
/// How often the staff are read again, so that a tasks part started after
/// the voice, or restarted with other targets, is seen by the next session.
const READ_AGAIN: Duration = Duration::from_secs(30);
/// Who gets the work when the switch is off: as it always was.
const CLAUDE_CODE: &str = "Claude Code";

/// The task prepared and not yet started.
#[derive(Clone, Debug)]
struct Pending {
    id: u64,
    digest: String,
    request: u64,
    at: Instant,
    /// Who it was prepared for, as the person calls them.
    who: &'static str,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

/// An agent the tasks part can start: what the person calls it, how the
/// model is told it may sound, and the close forms the voice takes for it
/// (lower case, letters and digits only).
#[derive(Debug, Eq, PartialEq)]
struct Kind {
    engine: &'static str,
    name: &'static str,
    sounds: &'static str,
    close: &'static [&'static str],
}

/// The engines `lamp-tasks` takes. "Cloud" and "codecs" are what the issue
/// (#733) expects the model to hear for Claude and Codex; none of these
/// forms is close to another engine's.
/// The agents the lamp hands work to, as the person calls them: what the
/// second route's speech to text listens for (`second_provider::net::key_terms`).
pub fn agent_names() -> [&'static str; 3] {
    KINDS.map(|kind| kind.name)
}

const KINDS: [Kind; 3] = [
    Kind {
        engine: "claude",
        name: CLAUDE_CODE,
        sounds: "\"Claude\" or \"Claude Code\", which can sound like \"cloud\" or \"cloud code\"",
        close: &["claudecode", "claude", "cloudcode", "cloud"],
    },
    Kind {
        engine: "codex",
        name: "Codex",
        sounds: "\"Codex\", which can sound like \"codecs\" or \"code X\"",
        close: &["codex", "codecs", "codec"],
    },
    Kind {
        engine: "opencode",
        name: "OpenCode",
        sounds: "\"OpenCode\" or \"Open Code\"",
        close: &["opencode"],
    },
];

/// One of the staff: a target of the tasks part, and its kind.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Member {
    alias: String,
    kind: &'static Kind,
}

/// The staff as last read from the tasks part; empty until it answers.
static STAFF: Mutex<Vec<Member>> = Mutex::new(Vec::new());
static READING: OnceLock<()> = OnceLock::new();

/// The functions a session is told of: with [`WHO_FLAG`], for the staff as
/// last read.
pub fn tools(place: &Place) -> Vec<Tool> {
    declared(place, &staff_now(place))
}

fn staff_now(place: &Place) -> Vec<Member> {
    if !place.who {
        return Vec::new();
    }
    STAFF.lock().map(|staff| staff.clone()).unwrap_or_default()
}

/// The person may name one of them: the switch is on and there is more
/// than one.
fn choosing(place: &Place, staff: &[Member]) -> bool {
    place.who && staff.len() >= 2
}

/// What `start_agent_task`'s description asks to be said, and when: after
/// the call, as always, or with the lead-in before it, in the same turn, as
/// a statement. The question comes after, from the answer's `next`
/// ([`prepared_next`]), while the read-back is still being played: a
/// question said before a call ends a spoken turn, and the call need not
/// come. Kept short: with work asked of the staff ([`ASKED_FLAG`]) and
/// three of them, the description has 165 bytes left for it of the 1,024
/// the service takes.
fn read_back(lead_in: bool, with_who: &str) -> String {
    if lead_in {
        format!(
            "Say the task back in one short sentence{with_who} as a statement, then call it \
             at once; ask whether to start it only after it answers."
        )
    } else {
        format!(
            "Nothing starts yet: then say the task back in one short sentence{with_who} \
             and ask whether to start it."
        )
    }
}

fn declared(place: &Place, staff: &[Member]) -> Vec<Tool> {
    let choosing = choosing(place, staff);
    // Who does it when nobody is named; without the switch, as always.
    let default = if place.who {
        default_member(staff, &place.target).map_or(CLAUDE_CODE, |member| member.kind.name)
    } else {
        CLAUDE_CODE
    };
    let task = json!({
        "type": "string",
        "description": "The work, in the person's own words, complete enough to do without asking."
    });
    let (start_description, start_parameters) = if choosing {
        let all = names(staff, "and");
        let sounds: Vec<String> = staff
            .iter()
            .map(|member| format!("{} (they may say {})", member.kind.name, member.kind.sounds))
            .collect();
        let who: Vec<&str> = staff.iter().map(|member| member.kind.name).collect();
        let unsure = if place.asked {
            " If you could not quite make out the name they said, give the one you think they \
             meant, and do not ask about it first: the task said back names who will do it, \
             so they can put you right."
        } else {
            ""
        };
        (
            if place.asked {
                format!(
                    "Hand a piece of work to one of the person's agents on their {} \
                     ({all}). {} {} \
                     You can do this: if they ask you to start one of them, open a session or \
                     give it something, without saying what it should do, never say you cannot; \
                     ask in a few words what it should work on, and call this with their answer.",
                    computer(place),
                    asked_of(&all, &examples(staff), place.staff_note, computer(place)),
                    read_back(place.lead_in, ", with who will do it,")
                )
            } else {
                format!(
                    "Prepare a piece of computer work for one of the person's agents on their office \
                     computer ({all}): writing or changing code, documents or files there. Call it \
                     only when the person asks for such work to be done there. {} \
                     You can do this: if they ask you to start one of them, open a session or \
                     give it something, without saying what it should do, never say you cannot; \
                     ask in a few words what it should work on, and call this with their answer.",
                    read_back(place.lead_in, ", with who will do it,")
                )
            },
            json!({
                "type": "object",
                "properties": {
                    "task": task,
                    "who": {
                        "type": "string",
                        "enum": who,
                        "description": format!(
                            "Who does it, if the person named one: {}. Leave it out if they named nobody: then {default} does it.{unsure}",
                            sounds.join("; ")
                        )
                    }
                },
                "required": ["task"]
            }),
        )
    } else {
        (
            if place.asked {
                format!(
                    "Hand a piece of work to {default} on the person's {}. {} {} \
                     You can do this: if they ask you to start {default}, open a session or \
                     give it something, without saying what it should do, never say you cannot; \
                     ask in a few words what it should work on, and call this with their answer.",
                    computer(place),
                    asked_of(
                        default,
                        &examples_of(default),
                        place.staff_note,
                        computer(place)
                    ),
                    read_back(place.lead_in, "")
                )
            } else {
                format!(
                    "Prepare a piece of computer work for {default} on the person's office \
                     computer: writing or changing code, documents or files there. Call it only \
                     when the person asks for such work to be done there. {} \
                     You can do this: if they ask you to start {default}, open a session or \
                     give it something, without saying what it should do, never say you cannot; \
                     ask in a few words what it should work on, and call this with their answer.",
                    read_back(place.lead_in, "")
                )
            },
            json!({
                "type": "object",
                "properties": {"task": task},
                "required": ["task"]
            }),
        )
    };
    vec![
        Tool {
            name: START.into(),
            description: start_description,
            parameters: Some(start_parameters),
            silent: false,
        },
        Tool {
            name: CONFIRM.into(),
            description: if place.lead_in {
                format!(
                    "Start the task you prepared with start_agent_task. Call it only right \
                     after the person has said yes to the task you read back; never on your \
                     own, and not if they said no or something else. In the same breath, \
                     before you call it, say in a few natural words that you are giving it \
                     to whoever will do it (\"Okay, giving that to {default}.\"), and nothing \
                     more until it answers."
                )
            } else {
                "Start the task you prepared with start_agent_task. Call it only right \
                 after the person has said yes to the task you read back; never on your \
                 own, and not if they said no or something else."
                    .into()
            },
            parameters: None,
            silent: false,
        },
        Tool {
            name: CHECK.into(),
            description: format!(
                "How the latest work you handed over is going, and what the agent came \
                 back with if it has finished. Call it only when the person asks about that \
                 work (\"how's it going?\", \"what did {default} say?\"), never on your own: \
                 you are told by yourself when it finishes.{}",
                if place.lead_in {
                    " In the same breath, before you call it, say in a few words that you are \
                     looking (\"Let me see how it's going.\")."
                } else {
                    ""
                }
            ),
            parameters: choosing.then(|| {
                json!({
                    "type": "object",
                    "properties": {
                        "who": {
                            "type": "string",
                            "enum": staff.iter().map(|member| member.kind.name).collect::<Vec<_>>(),
                            "description": format!(
                                "Whose work, if the person named one: {}. Leave it out for the latest work handed over.",
                                names(staff, "or")
                            )
                        }
                    }
                })
            }),
            silent: false,
        },
    ]
}

/// What the model is told after `start_agent_task` prepared the work.
pub fn prepared_next(lead_in: bool, choosing: bool, who: &str) -> String {
    match (lead_in, choosing) {
        (true, false) => "If you already said the task back, now only ask in a few words whether to start it (\"Shall I start it?\"). If you did not, say it back in one short sentence and ask. Start it only if the person says yes.".to_owned(),
        (true, true) => format!(
            "If you already said the task back, now only ask in a few words whether to start it (\"Shall I start it?\"). If you did not, say it back in one short sentence, saying {who} will do it, and ask. Start it only if the person says yes."
        ),
        (false, true) => format!(
            "Say the task back in one short sentence, saying {who} will do it, and ask whether to start it. Start it only if the person says yes."
        ),
        (false, false) => "Say the task back in one short sentence and ask whether to start it. Start it only if the person says yes.".to_owned(),
    }
}

/// What the model is told after `confirm_agent_task` handed the work over.
pub fn handing_over_next(lead_in: bool, who: &str) -> String {
    if lead_in {
        format!(
            "They have heard you say you are giving it to {who}. Add only, in a few words, that you will tell them when it is done, unless you said that already. It has not been taken up yet: do not say it has."
        )
    } else {
        format!(
            "Tell them in a few natural words that you are handing it to {who} now and will let them know when it is done, the way a chief of staff would (\"Sure, I'm giving that to {who}. I'll tell you when it's done.\"). It has not been taken up yet: do not say it has."
        )
    }
}

/// Where the work is done: the person's computer, called the office's
/// without [`ACCOUNTS_FLAG`] (a home has no office, #1676).
fn computer(place: &Place) -> &'static str {
    if place.accounts {
        "computer"
    } else {
        "office computer"
    }
}

/// When `start_agent_task` is called with [`ASKED_FLAG`]: whenever the
/// person asks to have `whom` do something, whatever the work (#854).
fn asked_of(whom: &str, examples: &str, noted: bool, computer: &str) -> String {
    format!(
        "Call it whenever the person asks to have {whom} do something ({examples}), \
         whatever the work is: code, documents or files, and as much a story, a poem, a plan \
         or anything you could do yourself; and even in the middle of something else, a story \
         you are telling included. {} Call it too when they ask for work to be done on their \
         {computer}.",
        if noted { CHOSE_NOTED } else { CHOSE }
    )
}

/// Who does the work is the person's choice, and the model's never.
const CHOSE: &str = "The person chose who does it: never do the work yourself instead, never begin it, and never offer to.";
/// The same, saying that the staff's note ([`crate::staff_note`]) names who:
/// 9 bytes more, which every description has room for. The longest, three
/// of the staff with the lead-in, is 1,014 of the 1,024 bytes with it
/// (`every_description_fits_the_service_whatever_the_switches`).
const CHOSE_NOTED: &str = "The person chose who does it (a note may name them): never do the work yourself, never begin it, nor offer to.";

/// "\"ask Claude Code to …\", \"get Codex to …\"" for the staff.
fn examples(staff: &[Member]) -> String {
    let mut said: Vec<String> = Vec::new();
    for (index, member) in staff.iter().enumerate() {
        let name = member.kind.name;
        said.push(match index % 3 {
            0 => format!("\"ask {name} to …\""),
            1 => format!("\"get {name} to …\""),
            _ => format!("\"have {name} …\""),
        });
    }
    said.join(", ")
}

/// The same for one agent.
fn examples_of(name: &str) -> String {
    format!("\"ask {name} to …\", \"get {name} to …\", \"have {name} …\"")
}

/// What a new session is told after the persona with [`ASKED_FLAG`]: the
/// persona asks for stories to be told gladly, and a story asked of one of
/// the staff is theirs, but one asked of the lamp is its own (#854). `None`
/// without the switch.
pub fn session_note(place: &Place) -> Option<String> {
    if !place.asked {
        return None;
    }
    let staff = staff_now(place);
    let whom = if choosing(place, &staff) {
        names(&staff, "or")
    } else if place.who {
        default_member(&staff, &place.target)
            .map_or(CLAUDE_CODE, |member| member.kind.name)
            .to_owned()
    } else {
        CLAUDE_CODE.to_owned()
    };
    let computer = computer(place);
    let accounts = if place.accounts {
        format!(
            " The person's own calendar, meetings, email and other accounts are on their \
             computer, not with you: you have none, and your timers and reminders are not their \
             calendar. When they ask about them (\"what's on my calendar today\", \"any email \
             from …\", \"when is my next meeting\"), never answer from what you know or from \
             your reminders: hand it to {whom} with start_agent_task, the ask in their words, \
             and its answer is told to them when it comes."
        )
    } else {
        String::new()
    };
    Some(format!(
        "\nWhen the person asks to have {whom} do something, it is theirs to do, not yours: \
         hand it over with start_agent_task every time, whatever it is, a story or a poem as \
         much as code, even one you could tell yourself, and even in the middle of a story you \
         are telling. Never do it yourself instead. A name that comes to you garbled, such as \
         \"Claude Co\" for Claude Code, still names them. Work on their {computer} is \
         theirs too, named or not.{accounts} But a story, a poem or anything else they ask of you that \
         names neither {whom} nor work on their {computer}, however long, is yours: tell \
         it yourself, gladly, and never offer it to {whom} or ask whether {whom} should, even \
         after you handed something over earlier in this conversation."
    ))
}

/// What a new session is told after the persona, `told`, with
/// [`session_note`] added when the switch is on.
pub fn noted(told: Option<String>, place: Option<&Place>) -> Option<String> {
    match place.and_then(session_note) {
        Some(note) => Some(told.unwrap_or_default() + &note),
        None => told,
    }
}

/// "Claude Code, Codex and OpenCode", or "or".
fn names(staff: &[Member], and: &str) -> String {
    let names: Vec<&str> = staff.iter().map(|member| member.kind.name).collect();
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} {and} {last}", rest.join(", ")),
        Some((last, _)) => (*last).to_owned(),
        None => String::new(),
    }
}

/// Who does the work when nobody is named: the target the voice was given
/// if it is one of the staff, else Claude Code, else the first.
fn default_member<'a>(staff: &'a [Member], target: &str) -> Option<&'a Member> {
    staff
        .iter()
        .find(|member| member.alias == target)
        .or_else(|| staff.iter().find(|member| member.kind.engine == "claude"))
        .or_else(|| staff.first())
}

/// Lower case, letters and digits only: "Code X" and "codex" are one.
fn plain(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|letter| letter.to_ascii_lowercase())
        .collect()
}

/// The one member a name the model gave is: by the member's name, a close
/// form of it, or its alias. A form two members could share names neither.
fn named<'a>(staff: &'a [Member], said: &str) -> Option<&'a Member> {
    let said = plain(said);
    let mut found = staff.iter().filter(|member| {
        member.kind.close.contains(&said.as_str()) || plain(&member.alias) == said
    });
    let one = found.next()?;
    found.next().is_none().then_some(one)
}

/// Which of the staff the heard words of a request name, as the person
/// calls them, in the order first named ([`crate::staff_note`]): the staff
/// as last read, or Claude Code alone when the person may not name anyone
/// (all work goes there). Empty when the words name nobody.
pub fn named_in_words(place: &Place, heard: &str) -> Vec<&'static str> {
    let staff = staff_now(place);
    let kinds: Vec<&'static Kind> = if staff.is_empty() {
        KINDS
            .iter()
            .filter(|kind| kind.name == CLAUDE_CODE)
            .collect()
    } else {
        staff.iter().map(|member| member.kind).collect()
    };
    kinds_in_words(&kinds, heard)
        .into_iter()
        .map(|kind| kind.name)
        .collect()
}

/// The words before a single-word form that make it a name said to have
/// work done ("ask Claude to …"): alone, "Claude" or "codecs" is as likely
/// someone else or something else ("which codec?" names nobody). The first
/// builder on #1173. "Tell" counts only with "to" after the name: "tell
/// Claude to …" asks, "tell Claude I'm late" is a message.
const ASKING: [&str; 4] = ["ask", "get", "have", "tell"];

/// The kinds among `kinds` that `heard` names, in the order first named:
/// two words said together that are a close form ("Claude Code", "code X",
/// "cloud code", "open code"), anywhere; one word that is ("Codex",
/// "Claude", "codecs"), only right after one of [`ASKING`] ("tell" with
/// "to" after the name). "Cloud" alone
/// is never taken: "save it to the cloud" names nobody.
fn kinds_in_words(kinds: &[&'static Kind], heard: &str) -> Vec<&'static Kind> {
    // Each word, and whether a stop or comma comes between it and the one
    // before: two words make a pair only across spaces (or a hyphen), so
    // "the cloud. Code it later." names nobody (the reviewer on #1173).
    let mut words: Vec<String> = Vec::new();
    let mut broken: Vec<bool> = Vec::new();
    let mut gap_broken = false;
    for piece in heard.split_inclusive(|letter: char| !letter.is_alphanumeric()) {
        let word: String = piece
            .chars()
            .filter(|letter| letter.is_alphanumeric())
            .collect();
        if !word.is_empty() {
            words.push(plain(&word));
            broken.push(gap_broken);
            gap_broken = false;
        }
        gap_broken |= piece
            .chars()
            .any(|letter| !letter.is_alphanumeric() && !letter.is_whitespace() && letter != '-');
    }
    let mut found: Vec<&'static Kind> = Vec::new();
    for (at, word) in words.iter().enumerate() {
        let asked = at.checked_sub(1).is_some_and(|before| {
            let before = words[before].as_str();
            ASKING.contains(&before)
                && (before != "tell" || words.get(at + 1).is_some_and(|next| next == "to"))
        });
        let pair = words
            .get(at + 1)
            .filter(|_| !broken[at + 1])
            .map(|next| format!("{word}{next}"));
        for kind in kinds {
            let alone = asked && word != "cloud" && kind.close.contains(&word.as_str());
            let together = pair
                .as_deref()
                .is_some_and(|pair| kind.close.contains(&pair));
            if (alone || together) && !found.contains(kind) {
                found.push(kind);
            }
        }
    }
    found
}

/// Who gets the work in a call: nobody in particular when the person
/// cannot name anyone (the switch off, or the staff not read), so it goes
/// where it always went; the one named; the default when nobody is named;
/// or, for a name that is none of the staff, what to tell the model.
fn chosen<'a>(
    place: &Place,
    staff: &'a [Member],
    args: &Value,
) -> Result<Option<&'a Member>, Value> {
    if !place.who || staff.is_empty() {
        return Ok(None);
    }
    // Nobody to choose between: the one there is, whatever was said.
    if !choosing(place, staff) {
        return Ok(default_member(staff, &place.target));
    }
    let said = args
        .get("who")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|said| !said.is_empty());
    match said {
        None => Ok(default_member(staff, &place.target)),
        Some(said) => named(staff, said).map(Some).ok_or_else(|| {
            json!({"error": format!(
                "Nothing was done: nobody by that name works here. Ask the person who should do it: {}.",
                names(staff, "or")
            )})
        }),
    }
}

/// The staff in the tasks part's answer to `targets`: the first alias of
/// each engine, in the tasks part's order, and none of an engine this
/// voice does not know.
fn staff_from(answer: &Value) -> Option<Vec<Member>> {
    if answer.get("ok") != Some(&json!(true)) {
        return None;
    }
    let mut staff: Vec<Member> = Vec::new();
    for target in answer.get("targets")?.as_array()? {
        let (Some(name), Some(engine)) = (target["alias"].as_str(), target["engine"].as_str())
        else {
            continue;
        };
        let Some(kind) = KINDS.iter().find(|kind| kind.engine == engine) else {
            continue;
        };
        if alias(name) && !staff.iter().any(|member| member.kind == kind) {
            staff.push(Member {
                alias: name.to_owned(),
                kind,
            });
        }
    }
    Some(staff)
}

/// Every target in the tasks part's answer to `targets`, alias and engine,
/// in its order: unlike [`staff_from`], two of one engine are both kept, so
/// they can be counted (#1390). Aliases that aren't plain are left out.
fn targets_from(answer: &Value) -> Option<Vec<(String, String)>> {
    if answer.get("ok") != Some(&json!(true)) {
        return None;
    }
    Some(
        answer
            .get("targets")?
            .as_array()?
            .iter()
            .filter_map(|target| {
                let (name, engine) = (target["alias"].as_str()?, target["engine"].as_str()?);
                (alias(name) && alias(engine)).then(|| (name.to_owned(), engine.to_owned()))
            })
            .collect(),
    )
}

/// "'office' (claude), 'laptop' (codex)", or "none", for the journal:
/// aliases and engines only, never a machine or project.
fn listed(targets: &[(String, String)]) -> String {
    if targets.is_empty() {
        return "none".to_owned();
    }
    targets
        .iter()
        .map(|(name, engine)| format!("'{name}' ({engine})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What becomes of a hand-over whose target the tasks part doesn't have.
#[derive(Debug, PartialEq, Eq)]
enum Missing {
    /// The one target of the engine asked for: the work goes there.
    Use(String),
    /// None of that engine, or more than one to choose between.
    NotSetUp,
    /// Who is there couldn't be read: answered as any other refusal.
    Unread,
}

/// The tasks part refused `asked` as no target of its own (#1390). On
/// lamp-4ace on 18 Oct its targets file changed under the voice, which went
/// on asking for `office-claude` while only `office` was there, and the
/// person heard "I can't reach Claude Code". Who is there is read once, kept
/// as the staff, and said in the journal. With exactly one target of the
/// `engine` asked for, the work goes there: it is the only place it could
/// go, and the read-back and the person's yes still come first. Otherwise
/// the model is told it isn't set up, never that it can't be reached. A
/// named member is never swapped for another engine.
fn missing_target(
    place: &Place,
    request: u64,
    asked: &str,
    engine: &str,
    deadline: Duration,
) -> Missing {
    let Some(there) = ask(
        &place.directory.join("request.sock"),
        &json!({"op":"targets"}),
        deadline,
    )
    .as_ref()
    .and_then(|answer| {
        // Kept as the staff only where the staff are used ([`WHO_FLAG`]).
        if place.machine_names
            && let Some(names) = names_from(answer)
        {
            keep_names(names);
        }
        if place.who
            && let Some(staff) = staff_from(answer)
        {
            keep_staff(staff);
        }
        targets_from(answer)
    }) else {
        return Missing::Unread;
    };
    eprintln!(
        "lamp-voice provider: request {request}: no target '{asked}' in the tasks part; it has {}",
        listed(&there)
    );
    let same: Vec<&String> = there
        .iter()
        .filter(|(_, kind)| kind == engine)
        .map(|(name, _)| name)
        .collect();
    match same.as_slice() {
        [one] if *one == asked => Missing::Unread,
        [one] => {
            eprintln!(
                "lamp-voice provider: request {request}: using '{one}', the only {engine} target"
            );
            Missing::Use((*one).clone())
        }
        _ => Missing::NotSetUp,
    }
}

/// What the model is told when `who` isn't set up for the lamp: plainly
/// that, and never that it can't be reached just now (#1390). No place is
/// named: a home has no "office", and a target's name can be anything.
fn not_set_up(who: &str) -> Value {
    json!({"error": format!(
        "{who} isn't set up for the lamp yet, so this can't be handed over. \
         Say so plainly in a few words: it needs setting up on the computer first."
    )})
}

/// The computers' own names by target alias, as last read from the tasks
/// part ([`NAMES_FLAG`], #1676); said only while [`NAMES_ON`].
static NAMES: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
static NAMES_ON: AtomicBool = AtomicBool::new(false);

/// The names in the tasks part's answer to `targets`: each target's `name`,
/// where it has one that is a plain label of at most 64 bytes with no
/// control characters; others are left out, and said as today.
fn names_from(answer: &Value) -> Option<BTreeMap<String, String>> {
    if answer.get("ok") != Some(&json!(true)) {
        return None;
    }
    Some(
        answer
            .get("targets")?
            .as_array()?
            .iter()
            .filter_map(|target| {
                let (alias_, name) = (target["alias"].as_str()?, target["name"].as_str()?);
                let plain = !name.is_empty()
                    && name.len() <= 64
                    && name.trim() == name
                    && !name.chars().any(char::is_control);
                (alias(alias_) && plain).then(|| (alias_.to_owned(), name.to_owned()))
            })
            .collect(),
    )
}

/// The names as read now, kept. The journal says how many, never which: a
/// computer's name may be its person's.
fn keep_names(read: BTreeMap<String, String>) {
    if let Ok(mut known) = NAMES.lock()
        && *known != read
    {
        eprintln!(
            "lamp-voice provider: {} computers known by name",
            read.len()
        );
        *known = read;
    }
}

/// The computer `alias` names, by its own name, with [`NAMES_FLAG`] and a
/// name read for it; none otherwise.
pub(crate) fn machine_name(alias: &str) -> Option<String> {
    if !NAMES_ON.load(Ordering::Relaxed) {
        return None;
    }
    NAMES.lock().ok()?.get(alias).cloned()
}

/// Where the read-back says the work goes: "your MacBook Air" for a
/// computer known by name ([`NAMES_FLAG`]), else "the office computer" (or
/// "the computer"), as without it.
fn where_done(place: &Place, alias: &str) -> String {
    match place.machine_names.then(|| machine_name(alias)).flatten() {
        Some(name) => format!("your {name}"),
        None => format!("the {}", computer(place)),
    }
}

/// The staff as read now, kept, with a journal line when it changed.
fn keep_staff(read: Vec<Member>) {
    if let Ok(mut known) = STAFF.lock()
        && *known != read
    {
        let each: Vec<String> = read
            .iter()
            .map(|member| format!("{} ({})", member.kind.name, member.alias))
            .collect();
        eprintln!(
            "lamp-voice provider: work can go to {}",
            if each.is_empty() {
                "nobody this voice knows".to_owned()
            } else {
                each.join(", ")
            }
        );
        *known = read;
    }
}

/// The journal line for `target` missing from the tasks part's targets
/// `there`, once per change of them: `seen` is what was read last (#1390).
fn missing_line(
    seen: &mut Option<Vec<(String, String)>>,
    there: Vec<(String, String)>,
    target: &str,
) -> Option<String> {
    if seen.as_ref() == Some(&there) {
        return None;
    }
    let line = (!there.iter().any(|(name, _)| name == target)).then(|| {
        format!(
            "lamp-voice provider: no target '{target}' in the tasks part; it has {}",
            listed(&there)
        )
    });
    *seen = Some(there);
    line
}

/// The targets the reader last saw, so that a target the voice was set to
/// and the tasks part lacks is said once per change (#1390).
static TARGETS_SEEN: Mutex<Option<Vec<(String, String)>>> = Mutex::new(None);

/// Read the staff from the tasks part: once now, briefly, and then every
/// 30 s on a thread of its own, so that opening a session waits for
/// nobody. Only with [`WHO_FLAG`] or [`NAMES_FLAG`], once per process. A
/// read that fails keeps what was known.
pub fn start(place: &Place) {
    if !(place.who || place.machine_names) || READING.set(()).is_err() {
        return;
    }
    NAMES_ON.store(place.machine_names, Ordering::Relaxed);
    let socket = place.directory.join("request.sock");
    let target = place.target.clone();
    let who = place.who;
    read_staff(&socket, &target, FIRST_READ, who);
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-staff".into())
        .spawn(move || {
            loop {
                std::thread::sleep(READ_AGAIN);
                read_staff(&socket, &target, crate::tools::ASK_DEADLINE, who);
            }
        });
    if spawned.is_err() {
        eprintln!("lamp-voice provider: no thread to read who work can go to; read once");
    }
}

fn read_staff(socket: &Path, target: &str, deadline: Duration, who: bool) {
    let Some(answer) = crate::tools::exchange_up_to(
        socket,
        &json!({"op":"targets"}),
        deadline,
        crate::tools::MAX_TASKS_ANSWER_BYTES,
    ) else {
        return;
    };
    let Some(read) = staff_from(&answer) else {
        return;
    };
    if let Some(names) = names_from(&answer) {
        keep_names(names);
    }
    if !who {
        return;
    }
    keep_staff(read);
    // The target the voice was set to, missing from the tasks part: said
    // once per change of its targets, before anyone asks (#1390).
    if let Some(there) = targets_from(&answer)
        && let Ok(mut seen) = TARGETS_SEEN.lock()
        && let Some(line) = missing_line(&mut seen, there, target)
    {
        eprintln!("{line}");
    }
}

pub fn is_tool(name: &str) -> bool {
    name == START || name == CONFIRM || name == CHECK
}

/// What the tasks part is reached by.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Place {
    pub directory: PathBuf,
    pub target: String,
    /// The person may name who gets the work ([`WHO_FLAG`]).
    pub who: bool,
    /// The model says the start of its answer before a call
    /// ([`LEAD_IN_FLAG`]).
    pub lead_in: bool,
    /// Work asked of the staff is handed over every time ([`ASKED_FLAG`]).
    pub asked: bool,
    /// The person's own accounts are asked of the computer's agent, and the
    /// computer is not called the office's ([`ACCOUNTS_FLAG`], #1676).
    pub accounts: bool,
    /// The computers are called by their own names where the tasks part
    /// has one ([`NAMES_FLAG`], #1676).
    pub machine_names: bool,
    /// The file the tasks part needs ([`NEEDS_FLAG`]).
    pub needs: Option<PathBuf>,
    /// A request whose words name one of the staff is told so
    /// ([`crate::staff_note`]), and `start_agent_task` says what that note
    /// asks. Set by the provider worker from its own flag.
    pub staff_note: bool,
}

/// The provider worker's arguments for what the environment says.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var(TARGET_ENVIRONMENT).ok().as_deref(),
        std::env::var(WHO_ENVIRONMENT).ok().as_deref(),
        std::env::var(LEAD_IN_ENVIRONMENT).ok().as_deref(),
        std::env::var(ASKED_ENVIRONMENT).ok().as_deref(),
        std::env::var(ACCOUNTS_ENVIRONMENT).ok().as_deref(),
        std::env::var(NAMES_ENVIRONMENT).ok().as_deref(),
        std::env::var(NEEDS_ENVIRONMENT).ok().as_deref(),
    )
}

// One value a switch, as the environment gives them, in their flags' order.
#[allow(clippy::too_many_arguments)]
fn arguments(
    directory: Option<&str>,
    target: Option<&str>,
    who: Option<&str>,
    lead_in: Option<&str>,
    asked: Option<&str>,
    accounts: Option<&str>,
    names: Option<&str>,
    needs: Option<&str>,
) -> Vec<String> {
    let Some(directory) = directory.filter(|path| Path::new(path).is_absolute()) else {
        return Vec::new();
    };
    let mut arguments = vec![FLAG.to_owned(), directory.to_owned()];
    if let Some(target) = target.filter(|target| alias(target)) {
        arguments.push(TARGET_FLAG.to_owned());
        arguments.push(target.to_owned());
    }
    if who == Some("on") {
        arguments.push(WHO_FLAG.to_owned());
    }
    if lead_in == Some("on") {
        arguments.push(LEAD_IN_FLAG.to_owned());
    }
    if asked == Some("on") {
        arguments.push(ASKED_FLAG.to_owned());
        // Only with the hand-over asked every time: its note says it.
        if accounts == Some("on") {
            arguments.push(ACCOUNTS_FLAG.to_owned());
        }
    }
    if names == Some("on") {
        arguments.push(NAMES_FLAG.to_owned());
    }
    if let Some(needs) = needs.filter(|path| Path::new(path).is_absolute()) {
        arguments.push(NEEDS_FLAG.to_owned());
        arguments.push(needs.to_owned());
    }
    arguments
}

fn alias(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Take a leading `--tasks-dir DIR [--tasks-target ALIAS] [--tasks-who]
/// [--tasks-lead-in] [--tasks-asked [--tasks-accounts]] [--tasks-machine-names]
/// [--tasks-needs PATH]`
/// off the flags.
pub fn take(flags: &[String]) -> std::io::Result<(Option<Place>, &[String])> {
    let [flag, rest @ ..] = flags else {
        return Ok((None, flags));
    };
    if flag != FLAG {
        return Ok((None, flags));
    }
    let [directory, rest @ ..] = rest else {
        return Err(std::io::Error::other("--tasks-dir needs a path"));
    };
    if !Path::new(directory).is_absolute() {
        return Err(std::io::Error::other("--tasks-dir must be absolute"));
    }
    let (target, rest) = match rest {
        [flag, target, rest @ ..] if flag == TARGET_FLAG => {
            if !alias(target) {
                return Err(std::io::Error::other("--tasks-target is not an alias"));
            }
            (target.clone(), rest)
        }
        _ => (DEFAULT_TARGET.to_owned(), rest),
    };
    let (who, rest) = match rest {
        [flag, rest @ ..] if flag == WHO_FLAG => (true, rest),
        _ => (false, rest),
    };
    let (lead_in, rest) = match rest {
        [flag, rest @ ..] if flag == LEAD_IN_FLAG => (true, rest),
        _ => (false, rest),
    };
    let (asked, rest) = match rest {
        [flag, rest @ ..] if flag == ASKED_FLAG => (true, rest),
        _ => (false, rest),
    };
    let (accounts, rest) = match rest {
        [flag, rest @ ..] if flag == ACCOUNTS_FLAG && asked => (true, rest),
        _ => (false, rest),
    };
    let (machine_names, rest) = match rest {
        [flag, rest @ ..] if flag == NAMES_FLAG => (true, rest),
        _ => (false, rest),
    };
    let (needs, rest) = match rest {
        [flag, path, rest @ ..] if flag == NEEDS_FLAG => {
            if !Path::new(path).is_absolute() {
                return Err(std::io::Error::other("--tasks-needs must be absolute"));
            }
            (Some(PathBuf::from(path)), rest)
        }
        _ => (None, rest),
    };
    Ok((
        Some(Place {
            directory: PathBuf::from(directory),
            target,
            who,
            lead_in,
            asked,
            accounts,
            machine_names,
            needs,
            staff_note: false,
        }),
        rest,
    ))
}

/// Whether a yes in request `request` may start what was prepared: it came
/// in a request after the one that prepared it, one the lamp heard rather
/// than wrote, or one typed in the same chat as the request that prepared
/// it (#1673, `crate::messages::same_chat`), soon enough. A spoken yes never
/// starts a typed task, nor a typed yes a spoken one.
fn may_confirm(pending: &Pending, request: u64, heard: bool, now: Instant) -> bool {
    (heard || crate::messages::same_chat(pending.request, request))
        && request > pending.request
        && now.duration_since(pending.at) <= CONFIRM_WITHIN
}

/// The note a request is given beside its words on a route that keeps only
/// the words of its last exchanges, not its calls (the second route,
/// `crate::second_provider`): a task this voice prepared is waiting for the
/// person's yes, within [`CONFIRM_WITHIN`]. Without it, on lamp-4ace on 19
/// Oct the yes was taken as a new hand-over: the work was prepared again
/// and asked about again, and never started (#1712). None when nothing is
/// waiting, or for the request that prepared it.
pub fn pending_note(request: u64, now: Instant) -> Option<String> {
    let pending = PENDING.lock().ok()?.clone()?;
    (request > pending.request && now.duration_since(pending.at) <= CONFIRM_WITHIN).then(|| {
        format!(
            "(For you to know, not to answer: you have prepared a task for {} and asked the person whether to start it. If they say yes now, call {CONFIRM}; do not call {START} again for that work.)",
            pending.who
        )
    })
}

/// Run one call. Blocking, for a thread of its own. `heard`: the request the
/// call came in was something a person said, not something the lamp wrote.
pub fn run(
    place: &Place,
    call: &ToolCall,
    request: u64,
    heard: bool,
    deadline: Duration,
) -> (Value, bool) {
    run_for(place, &staff_now(place), call, request, heard, deadline)
}

/// What the model is told when the tasks part is not on this lamp at all
/// (#811: a home lamp without Harness, whose tasks unit is skipped): a few
/// light words it can say, not a fault.
pub const NOT_SET_UP: &str =
    "I can't hand work on to another computer yet: that isn't set up on this lamp.";

/// What the model is told when the tasks part did not answer: not set up
/// only while the file it needs is missing, as its unit's condition decides
/// (the reviewer on #1063: a crashed part's socket is gone too, since systemd
/// removes its runtime folder, and that is a fault); in every other case a
/// part not running ([`crate::tools::unavailable`]).
fn unreached(place: &Place) -> Value {
    match &place.needs {
        Some(needs) if !needs.exists() => json!({"error": NOT_SET_UP}),
        _ => crate::tools::unavailable(),
    }
}

/// [`run`] with the staff as known.
fn run_for(
    place: &Place,
    staff: &[Member],
    call: &ToolCall,
    request: u64,
    heard: bool,
    deadline: Duration,
) -> (Value, bool) {
    let started = Instant::now();
    let left = || deadline.saturating_sub(started.elapsed());
    match call.name.as_str() {
        START => {
            let Some(task) = call
                .args
                .get("task")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|task| !task.is_empty())
            else {
                return (json!({"error": "Say what the work is."}), true);
            };
            if task.len() > MOST_TASK_BYTES || task.starts_with("--") {
                return (
                    json!({"error": "That task is too long to send as it is: say it more shortly."}),
                    true,
                );
            }
            // Who is settled before the tasks part is asked anything.
            let member = match chosen(place, staff, &call.args) {
                Ok(member) => member,
                Err(told) => return (told, true),
            };
            let (target, who) = member.map_or((place.target.as_str(), CLAUDE_CODE), |member| {
                (member.alias.as_str(), member.kind.name)
            });
            let Some(status) = ask(
                &place.directory.join("request.sock"),
                &json!({"op":"status"}),
                left(),
            ) else {
                eprintln!(
                    "lamp-voice provider: request {request}: no answer to the status before preparing, {} ms in",
                    started.elapsed().as_millis()
                );
                return (unreached(place), false);
            };
            let Some(guard) = guard(&status, format!("voice-{request}-prepare-{}", nonce())) else {
                return (unreached(place), false);
            };
            let mut prepared = ask_until_sure(
                &place.directory.join("request.sock"),
                &json!({"op":"prepare","guard":guard,"text":task,"target":target}),
                left,
            );
            // The target the work was prepared for, for the read-back.
            let mut sent_to = target.to_owned();
            // The tasks part has no such target (#1390): its targets changed
            // under the voice, or the voice was set to a name it lacks.
            if prepared
                .as_ref()
                .is_some_and(|answer| answer.get("error") == Some(&json!("target")))
            {
                let engine = member.map_or("claude", |member| member.kind.engine);
                match missing_target(place, request, target, engine, left()) {
                    Missing::Use(other) => {
                        let Some(again) =
                            self::guard(&status, format!("voice-{request}-prepare-{}", nonce()))
                        else {
                            return (unreached(place), false);
                        };
                        prepared = ask_until_sure(
                            &place.directory.join("request.sock"),
                            &json!({"op":"prepare","guard":again,"text":task,"target":other}),
                            left,
                        );
                        sent_to = other;
                    }
                    Missing::NotSetUp => return (not_set_up(who), true),
                    Missing::Unread => {}
                }
            }
            match prepared {
                Some(answer) if answer.get("ok") == Some(&json!(true)) => {
                    let (Some(id), Some(digest)) = (
                        answer.get("id").and_then(Value::as_u64),
                        answer.get("prepared_digest").and_then(Value::as_str),
                    ) else {
                        return (unreached(place), false);
                    };
                    if let Ok(mut pending) = PENDING.lock() {
                        *pending = Some(Pending {
                            id,
                            digest: digest.to_owned(),
                            request,
                            at: Instant::now(),
                            who,
                        });
                    }
                    let next = prepared_next(place.lead_in, choosing(place, staff), who);
                    (
                        json!({
                            "prepared": true,
                            "task": task,
                            "where": format!("{who} on {}", where_done(place, &sent_to)),
                            "next": next
                        }),
                        true,
                    )
                }
                Some(answer) => {
                    // The tasks part's code only, never the task's words.
                    eprintln!(
                        "lamp-voice provider: request {request}: the tasks part did not prepare the work ({}, {} ms in)",
                        answer
                            .get("error")
                            .and_then(Value::as_str)
                            .filter(|code| alias(code))
                            .unwrap_or("no code"),
                        started.elapsed().as_millis()
                    );
                    (
                        json!({"error": "The work could not be prepared just now."}),
                        true,
                    )
                }
                None => {
                    eprintln!(
                        "lamp-voice provider: request {request}: no answer to the preparation, {} ms in",
                        started.elapsed().as_millis()
                    );
                    (unreached(place), false)
                }
            }
        }
        CONFIRM => {
            let pending = PENDING.lock().ok().and_then(|pending| pending.clone());
            let Some(pending) = pending else {
                return (
                    json!({"error": "There is no task waiting to be started."}),
                    true,
                );
            };
            if !may_confirm(&pending, request, heard, Instant::now()) {
                return (
                    json!({"error": "Not started: the person has not said yes to it since you asked. Ask again if they still want it."}),
                    true,
                );
            }
            let Some(status) = ask(
                &place.directory.join("request.sock"),
                &json!({"op":"status"}),
                left(),
            ) else {
                return (unreached(place), false);
            };
            let (Some(guard), Some(session)) = (
                guard(&status, format!("voice-{request}-confirm-{}", nonce())),
                status
                    .get("summary")
                    .and_then(|summary| summary.get("session_id"))
                    .and_then(Value::as_str),
            ) else {
                return (unreached(place), false);
            };
            let confirmed = ask_until_sure(
                &place.directory.join("voice.sock"),
                &json!({"op":"confirm","guard":guard,"id":pending.id,"prepared_digest":pending.digest,"session_id":session}),
                left,
            );
            match confirmed {
                Some(answer) if answer.get("ok") == Some(&json!(true)) => {
                    if let Ok(mut waiting) = PENDING.lock() {
                        *waiting = None;
                    }
                    // Asked for by text: its news goes back by text, and
                    // only such work's (`task_news::ForTexts`, #1673).
                    if crate::messages::is_text(pending.request)
                        && let Some(store) = status.get("store_id").and_then(Value::as_str)
                    {
                        crate::task_news::keep_typed(store, pending.id);
                    }
                    let who = pending.who;
                    (
                        json!({
                            "handing_over": true,
                            "who": who,
                            "next": handing_over_next(place.lead_in, who)
                        }),
                        true,
                    )
                }
                Some(_) => (json!({"error": "It could not be started just now."}), true),
                None => (unreached(place), false),
            }
        }
        CHECK => {
            // Only someone named narrows it, and only when there is a choice.
            let asked_about = call
                .args
                .get("who")
                .is_some_and(|who| who.as_str().is_some_and(|who| !who.trim().is_empty()));
            let only = if choosing(place, staff) && asked_about {
                match chosen(place, staff, &call.args) {
                    Ok(member) => member,
                    Err(told) => return (told, true),
                }
            } else {
                None
            };
            check(place, only, &crate::task_news::told_file(), left())
        }
        _ => (crate::tools::unavailable(), false),
    }
}

/// How the newest task handed over is going (to `only`, if named), from
/// what the tasks part last saw: no new look at the agent, so the answer
/// comes at once. A finished one the person now hears about is kept as
/// told. One they were already told is said to be, so the model answers
/// "it's the one I mentioned" rather than telling it again as new: on
/// lamp-4ace on 2026-10-15 at 08:02 news told unasked was announced again,
/// as if new, when the person asked 12 s later (#757).
fn check(
    place: &Place,
    only: Option<&Member>,
    told_file: &Path,
    deadline: Duration,
) -> (Value, bool) {
    let started = Instant::now();
    let left = || deadline.saturating_sub(started.elapsed());
    let socket = place.directory.join("request.sock");
    let mut newest: Option<u64> = None;
    let mut store;
    let mut after = 0;
    loop {
        let Some(page) = ask(&socket, &json!({"op":"list","after_id":after}), left()) else {
            return (unreached(place), false);
        };
        store = page["store_id"].as_str().unwrap_or_default().to_owned();
        for item in page["items"].as_array().into_iter().flatten() {
            if item["dispatch"]["state"] == json!("accepted")
                && only.is_none_or(|member| item["target_alias"] == json!(member.alias))
                && let Some(id) = item["id"].as_u64()
            {
                newest = newest.max(Some(id));
            }
        }
        match page["next_after_id"].as_u64() {
            Some(next) if next > after => after = next,
            _ => break,
        }
    }
    let Some(id) = newest else {
        let next = only.map_or_else(
            || "Tell them there is no work handed over just now.".to_owned(),
            |member| {
                format!(
                    "Tell them there is no work handed over to {} just now.",
                    member.kind.name
                )
            },
        );
        return (json!({"none": true, "next": next}), true);
    };
    let Some(task) = ask(&socket, &json!({"op":"get_task","id":id}), left()) else {
        return (unreached(place), false);
    };
    let who = crate::task_news::who(&task);
    let (asked, _) = crate::task_news::quoted(
        task["text"].as_str().unwrap_or_default(),
        crate::task_news::MOST_TASK_CHARS,
    );
    let Some(observation_id) = task["observations"]
        .as_array()
        .and_then(|seen| seen.iter().filter_map(|seen| seen["id"].as_u64()).max())
    else {
        return (
            json!({"who": who, "task": asked, "state": "working",
                "next": format!("Tell them in a few words that {who} is still working on it and that you will tell them yourself when it is done. Do not check again unless they ask again.")}),
            true,
        );
    };
    let Some(observation) = ask(
        &socket,
        &json!({"op":"get_observation","id":id,"observation_id":observation_id}),
        left(),
    ) else {
        return (unreached(place), false);
    };
    let data = &observation["data"];
    let state = data["state"].as_str().unwrap_or("unknown");
    let (answer, cut) = crate::task_news::quoted(
        data["answer"].as_str().unwrap_or_default(),
        crate::task_news::MOST_ANSWER_CHARS,
    );
    let settled = match state {
        "idle" => !answer.is_empty(),
        "needs-input" | "stopped" | "failed" => true,
        _ => false,
    };
    let key = format!("{store}:{id}:{observation_id}");
    let told_before = settled && crate::task_news::was_told(told_file, &key);
    if settled && !told_before {
        crate::task_news::keep_told(told_file, &key);
    }
    let more = cut || data["truncated"] == json!(true);
    let next = next(state, settled, who, &answer, told_before, more);
    let mut answered = json!({"who": who, "task": asked, "state": state, "answer": answer,
        "more": more, "next": next});
    if told_before {
        answered["told_before"] = json!(true);
    }
    // The provider worker takes this out before the model is told it, and
    // the tasks part hears the news was told once its turn is heard whole
    // (#1390). Told before or not: one told unasked may have been cut short.
    if settled {
        answered[NEWS_KEY] = json!(key);
    }
    (answered, true)
}

/// Where a `check_agent_task` answer carries the key of the finished news it
/// gives (#1390), for the provider worker alone.
pub(crate) const NEWS_KEY: &str = "news_key";

/// The news key in a `check_agent_task` answer `told`, taken out, so that the
/// model is never told it (#1390).
pub fn take_news_key(told: &mut Value) -> Option<String> {
    told.as_object_mut()?
        .remove(NEWS_KEY)?
        .as_str()
        .map(str::to_owned)
}

/// What the model is told to do with the last look at the agent.
fn next(
    state: &str,
    settled: bool,
    who: &str,
    answer: &str,
    told_before: bool,
    more: bool,
) -> String {
    match state {
        // Told already, unasked or asked (#757): the one mentioned, not
        // news again.
        "idle" if told_before => {
            "You already told them this. Do not tell it again as if it were new: say in a few words that it is done, the one you mentioned. If you gave no gist of it then, give it now in one plain line; otherwise add nothing unless they ask for more. No code, lists, paths or links."
        }
        "needs-input" if told_before => {
            return format!(
                "You already told them {who} is waiting for them. Say in a few words that it is still waiting for them on their computer; do not ask a question for it or guess one."
            );
        }
        "stopped" | "failed" if told_before => {
            "You already told them it stopped. Say so again in a few words, as the one you mentioned, without alarm."
        }
        "idle" if settled && more => {
            "Give them the gist of what it came back with in one plain spoken line first (a story: who it is about and what happens; code: what it does now), then say the rest is on their computer; never only that. No code, lists, paths or links."
        }
        "idle" if settled => {
            "Give them the gist of what it came back with in one or two plain spoken sentences; no code, lists, paths or links. If there is more, say the rest is on their computer."
        }
        "needs-input" => return crate::task_news::waiting(who, "their computer", answer),
        "stopped" | "failed" => {
            "Tell them in one plain sentence that it stopped without finishing."
        }
        _ => "Tell them in a few words that it is still working on it and that you will tell them yourself when it is done. Do not check again unless they ask again.",
    }
    .to_owned()
}

/// The tasks part's guard for a mutation: its store, its revision now, and
/// a request id of our own.
fn guard(status: &Value, request_id: String) -> Option<Value> {
    let store = status.get("store_id")?.as_str()?;
    let revision = status.get("revision")?.as_u64()?;
    Some(json!({"store_id": store, "revision": revision, "request_id": request_id}))
}

fn ask(socket: &Path, line: &Value, deadline: Duration) -> Option<Value> {
    if deadline.is_zero() {
        return None;
    }
    crate::tools::exchange_up_to(socket, line, deadline, crate::tools::MAX_TASKS_ANSWER_BYTES)
}

/// What the tasks part answers when it cannot say whether a change was made
/// (`uncertain`: its store took longer than the 1.1 s it waits), or when the
/// change never began (`busy`, `timeout`). Its answer asks for the same
/// request again, exactly, which gives back what was done if it was.
const ASK_AGAIN: [&str; 3] = ["uncertain", "busy", "timeout"];
/// Time between one try and the next.
const AGAIN_AFTER: Duration = Duration::from_millis(100);

/// [`ask`], and again with the very same line while the tasks part says it
/// is not sure and there is time. On lamp-4ace on 2026-10-15 two hand-overs
/// were answered `uncertain` after 1.1 s, while the lamp's disk was busy just
/// after a restart, and the lamp said the work could not be prepared; the
/// tasks part had prepared each of them a moment later.
fn ask_until_sure(socket: &Path, line: &Value, left: impl Fn() -> Duration) -> Option<Value> {
    loop {
        let answer = ask(socket, line, left())?;
        let again = answer.get("ok") != Some(&json!(true))
            && answer
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|code| ASK_AGAIN.contains(&code));
        if !again || left() <= AGAIN_AFTER {
            return Some(answer);
        }
        std::thread::sleep(AGAIN_AFTER);
    }
}

fn nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        sync::mpsc,
    };

    /// #811, with the reviewer's P2: a lamp whose tasks part needs a file
    /// that is not there (no Harness) tells the model in light words that
    /// work can't be handed on yet. A part that needs it and has it, but
    /// whose socket is gone (crashed: systemd removed its folder), is a fault;
    /// so is any part with no `needs` given.
    #[test]
    fn only_a_tasks_part_missing_what_it_needs_is_told_not_set_up() {
        let directory =
            std::env::temp_dir().join(format!("agent-not-set-up-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&directory);
        let place = |needs: Option<&str>| Place {
            directory: directory.clone(),
            target: "office".into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: needs.map(PathBuf::from),
            staff_note: false,
        };
        let told = |place: &Place, name: &str, args: Value| {
            let call = ToolCall {
                id: "c".into(),
                name: name.into(),
                args,
            };
            run(place, &call, 3, true, Duration::from_millis(200)).0
        };
        let missing = place(Some("/nonexistent/harness"));
        let there = place(Some("/"));
        let unset = place(None);
        // Starting and checking ask the tasks part; a confirm with nothing
        // prepared never gets that far.
        for (name, args) in [
            (START, json!({"task":"Write hello.txt"})),
            (CHECK, json!({})),
        ] {
            assert_eq!(
                told(&missing, name, args.clone())["error"],
                NOT_SET_UP,
                "{name}"
            );
            assert_eq!(
                told(&there, name, args.clone()),
                crate::tools::unavailable(),
                "{name}"
            );
            assert_eq!(
                told(&unset, name, args),
                crate::tools::unavailable(),
                "{name}"
            );
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// How long a test waits for its fake tasks part to answer, or for a line
    /// it sent. A fixture's progress bound, not a latency: on a loaded Mac
    /// (2026-10-15) a peer thread missed 2 s. Production's own bounds, such as
    /// `FIRST_READ` and the deadline the provider worker passes, are unchanged.
    const PEER_WAIT: Duration = Duration::from_secs(10);

    #[test]
    fn a_yes_counts_only_when_heard_after_the_task_and_soon() {
        let at = Instant::now();
        let pending = Pending {
            id: 1,
            digest: "d".into(),
            request: 5,
            at,
            who: CLAUDE_CODE,
        };
        assert!(may_confirm(&pending, 6, true, at + Duration::from_secs(10)));
        // In the request that prepared it: the model asking itself.
        assert!(!may_confirm(&pending, 5, true, at));
        // A request the lamp wrote, not one it heard.
        assert!(!may_confirm(&pending, 6, false, at));
        // Too late.
        assert!(!may_confirm(
            &pending,
            6,
            true,
            at + CONFIRM_WITHIN + Duration::from_secs(1)
        ));
        // A typed yes never starts a spoken task (#1673).
        let mut chats = crate::messages::Chats::default();
        assert!(!may_confirm(&pending, chats.next_request("s1"), false, at));
    }

    /// #1673: a task asked for in a text is started by a "yes" typed in the
    /// same chat, after it and soon; not by one from another chat, nor by a
    /// spoken one.
    #[test]
    fn a_typed_yes_counts_only_in_the_chat_that_asked_and_soon() {
        let mut chats = crate::messages::Chats::default();
        let asked = chats.next_request("s1");
        let other = chats.next_request("s2");
        let yes = chats.next_request("s1");
        let at = Instant::now();
        let pending = Pending {
            id: 1,
            digest: "d".into(),
            request: asked,
            at,
            who: CLAUDE_CODE,
        };
        assert!(may_confirm(
            &pending,
            yes,
            false,
            at + Duration::from_secs(10)
        ));
        assert!(
            !may_confirm(&pending, asked, false, at),
            "the text that asked"
        );
        assert!(!may_confirm(&pending, other, false, at), "another chat");
        assert!(!may_confirm(&pending, 6, true, at), "a spoken yes");
        assert!(!may_confirm(
            &pending,
            yes,
            false,
            at + CONFIRM_WITHIN + Duration::from_secs(1)
        ));
    }

    #[test]
    fn the_switch_is_an_absolute_directory_and_an_alias_and_is_taken_from_the_front() {
        assert!(arguments(None, None, None, None, None, None, None, None).is_empty());
        assert!(arguments(Some("relative"), None, None, None, None, None, None, None).is_empty());
        assert_eq!(
            arguments(Some("/run/t"), None, None, None, None, None, None, None),
            [FLAG, "/run/t"]
        );
        assert_eq!(
            arguments(
                Some("/run/t"),
                Some("m2"),
                None,
                None,
                None,
                None,
                None,
                None
            ),
            [FLAG, "/run/t", TARGET_FLAG, "m2"]
        );
        assert_eq!(
            arguments(
                Some("/run/t"),
                Some("no spaces"),
                None,
                None,
                None,
                None,
                None,
                None
            ),
            [FLAG, "/run/t"]
        );
        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                None,
                None,
                None,
                None,
                None,
                Some("/usr/local/bin/harness")
            ),
            [FLAG, "/run/t", NEEDS_FLAG, "/usr/local/bin/harness"]
        );
        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                None,
                None,
                None,
                None,
                None,
                Some("relative")
            ),
            [FLAG, "/run/t"]
        );
        let needs: Vec<String> = [
            "--tasks-dir",
            "/run/t",
            "--tasks-needs",
            "/usr/local/bin/harness",
        ]
        .map(String::from)
        .into();
        let (place, rest) = take(&needs).unwrap();
        assert_eq!(
            place.unwrap().needs.as_deref(),
            Some(Path::new("/usr/local/bin/harness"))
        );
        assert!(rest.is_empty());
        let flags: Vec<String> = ["--tasks-dir", "/run/t", "--other"]
            .map(String::from)
            .into();
        let (place, rest) = take(&flags).unwrap();
        assert_eq!(
            place,
            Some(Place {
                directory: "/run/t".into(),
                target: DEFAULT_TARGET.into(),
                who: false,
                lead_in: false,
                asked: false,
                accounts: false,
                machine_names: false,
                needs: None,
                staff_note: false,
            })
        );
        assert_eq!(rest, &flags[2..]);
        assert!(take(&["--tasks-dir".to_owned(), "x".to_owned()]).is_err());
        assert_eq!(take(&flags[2..]).unwrap().0, None);
    }

    /// A tasks part of one socket pair, answering each line with what the
    /// test gives it, and passing on what it was sent.
    pub(crate) fn tasks_part(
        directory: &Path,
        answers: Vec<(&'static str, Value)>,
    ) -> mpsc::Receiver<Value> {
        let (sent, seen) = mpsc::channel();
        for name in ["request.sock", "voice.sock"] {
            let listener = UnixListener::bind(directory.join(name)).unwrap();
            let answers: Vec<Value> = answers
                .iter()
                .filter(|(socket, _)| *socket == name)
                .map(|(_, answer)| answer.clone())
                .collect();
            let sent = sent.clone();
            std::thread::spawn(move || {
                for (stream, answer) in listener.incoming().zip(answers) {
                    let mut stream = stream.unwrap();
                    let mut line = String::new();
                    BufReader::new(&stream).read_line(&mut line).unwrap();
                    sent.send(serde_json::from_str::<Value>(&line).unwrap())
                        .unwrap();
                    writeln!(stream, "{answer}").unwrap();
                }
            });
        }
        seen
    }

    /// Held by each test that prepares and confirms: they share [`PENDING`].
    pub(crate) static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    /// Held by each test that writes or reads the shared [`STAFF`].
    static STAFF_HELD: Mutex<()> = Mutex::new(());

    /// The reviewer on #1446: a named member keeps its own engine through
    /// `run`. Codex is named, its `office-codex` is gone, and only `office`
    /// (claude) is there: Codex isn't set up, and nothing is prepared on
    /// Claude Code. With `LAMP_TASKS_WHO`, who is there becomes the staff;
    /// without it, the staff stay as they were.
    #[test]
    fn a_named_member_whose_target_is_gone_keeps_its_engine_through_run() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _staff = STAFF_HELD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory =
            std::env::temp_dir().join(format!("agent-tasks-named-{}", std::process::id()));
        let status =
            json!({"ok":true,"store_id":"s1","revision":0,"summary":{"session_id":"sess"}});
        let office = json!({"ok":true,"targets":[{"alias":"office","engine":"claude"}]});
        let call = ToolCall {
            id: "c".into(),
            name: START.into(),
            args: json!({"task":"Write hello.txt","who":"Codex"}),
        };
        *STAFF.lock().unwrap() = two();
        for who in [true, false] {
            let _ = std::fs::remove_dir_all(&directory);
            std::fs::create_dir_all(&directory).unwrap();
            let mut answers = vec![
                ("request.sock", status.clone()),
                ("request.sock", json!({"ok":false,"error":"target"})),
                ("request.sock", office.clone()),
            ];
            if !who {
                // Without the switch nobody is named: Claude Code's one
                // target is prepared.
                answers.push((
                    "request.sock",
                    json!({"ok":true,"id":7,"prepared_digest":"abc","revision":1}),
                ));
            }
            let seen = tasks_part(&directory, answers);
            let place = Place {
                target: "office-claude".into(),
                ..place(&directory, who)
            };
            let (told, answered) = run_for(&place, &two(), &call, 3, true, PEER_WAIT);
            assert!(answered, "{told}");
            let ops: Vec<Value> = seen.try_iter().collect();
            if who {
                assert_eq!(ops[1]["target"], "office-codex");
                assert_eq!(
                    told["error"],
                    "Codex isn't set up for the lamp yet, so this can't be handed over. \
                     Say so plainly in a few words: it needs setting up on the computer first."
                );
                assert_eq!(ops.len(), 3, "nothing prepared after: {ops:?}");
                assert_eq!(*STAFF.lock().unwrap(), vec![member("office", "claude")]);
                *STAFF.lock().unwrap() = two();
            } else {
                assert_eq!(ops[1]["target"], "office-claude");
                assert_eq!(ops[3]["target"], "office");
                assert_eq!(*STAFF.lock().unwrap(), two(), "the staff stay as they were");
            }
            if let Ok(mut pending) = PENDING.lock() {
                *pending = None;
            }
        }
        STAFF.lock().unwrap().clear();
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// #1390, lamp-4ace on 18 Oct: the voice asked for `office-claude`, and
    /// the tasks part had only `office`. A `target` refusal reads who is
    /// there once. The one target of the engine asked for is prepared
    /// instead. With none of that engine, or two to choose between, the
    /// model is told plainly that it isn't set up, never "can't reach it".
    /// Any other refusal is answered as before, with nothing read.
    #[test]
    fn a_target_the_tasks_part_lacks_is_answered_from_who_is_there() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory =
            std::env::temp_dir().join(format!("agent-tasks-target-{}", std::process::id()));
        let status =
            json!({"ok":true,"store_id":"s1","revision":0,"summary":{"session_id":"sess"}});
        let refused = |code: &str| json!({"ok":false,"error":code});
        let there = |targets: Value| json!({"ok":true,"targets":targets});
        let prepared = json!({"ok":true,"id":7,"prepared_digest":"abc","revision":1});
        let call = ToolCall {
            id: "c".into(),
            name: START.into(),
            args: json!({"task":"Write hello.txt"}),
        };
        let place = Place {
            directory: directory.clone(),
            target: "office-claude".into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
        };
        let not_set_up = "Claude Code isn't set up for the lamp yet, so this can't be handed over. \
                          Say so plainly in a few words: it needs setting up on the computer first.";
        let cases = [
            (
                vec![
                    status.clone(),
                    refused("target"),
                    there(
                        json!([{"alias":"office","engine":"claude"},{"alias":"laptop","engine":"codex"}]),
                    ),
                    prepared.clone(),
                ],
                Some("office"),
            ),
            (
                vec![
                    status.clone(),
                    refused("target"),
                    there(json!([{"alias":"laptop","engine":"codex"}])),
                ],
                None,
            ),
            (
                vec![
                    status.clone(),
                    refused("target"),
                    there(
                        json!([{"alias":"desk","engine":"claude"},{"alias":"office","engine":"claude"}]),
                    ),
                ],
                None,
            ),
        ];
        for (answers, used) in cases {
            let _ = std::fs::remove_dir_all(&directory);
            std::fs::create_dir_all(&directory).unwrap();
            let seen = tasks_part(
                &directory,
                answers
                    .into_iter()
                    .map(|answer| ("request.sock", answer))
                    .collect(),
            );
            let (told, answered) = run(&place, &call, 3, true, PEER_WAIT);
            let ops: Vec<Value> = seen.try_iter().collect();
            assert!(answered, "{told} {ops:?}");
            assert_eq!(ops[1]["target"], "office-claude");
            assert_eq!(ops[2], json!({"op":"targets"}));
            match used {
                Some(alias) => {
                    assert_eq!(told["prepared"], true, "{told}");
                    assert_eq!(ops[3]["op"], "prepare");
                    assert_eq!(ops[3]["target"], alias);
                    assert_ne!(ops[3]["guard"]["request_id"], ops[1]["guard"]["request_id"]);
                }
                None => {
                    assert_eq!(told, json!({"error": not_set_up}));
                    assert_eq!(ops.len(), 3, "nothing prepared after: {ops:?}");
                }
            }
        }
        // The target is listed again by the time it's read (the tasks part
        // came back with it): nothing is prepared twice, and the answer is
        // today's (Tests on #1446).
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let seen = tasks_part(
            &directory,
            vec![
                ("request.sock", status.clone()),
                ("request.sock", refused("target")),
                (
                    "request.sock",
                    there(json!([{"alias":"office-claude","engine":"claude"}])),
                ),
            ],
        );
        let (told, _) = run(&place, &call, 3, true, PEER_WAIT);
        assert_eq!(
            told,
            json!({"error": "The work could not be prepared just now."})
        );
        assert_eq!(seen.try_iter().count(), 3);
        // Another refusal: today's answer, and who is there is not read.
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let seen = tasks_part(
            &directory,
            vec![
                ("request.sock", status),
                ("request.sock", refused("bad_text")),
            ],
        );
        let (told, _) = run(&place, &call, 3, true, PEER_WAIT);
        assert_eq!(
            told,
            json!({"error": "The work could not be prepared just now."})
        );
        assert_eq!(seen.try_iter().count(), 2);
        if let Ok(mut pending) = PENDING.lock() {
            *pending = None;
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// #1390: the staff reader says a target the voice was set to and the
    /// tasks part lacks once per change of its targets, not at every read.
    #[test]
    fn a_missing_set_target_is_said_once_per_change_of_the_targets() {
        let office = || vec![("office".to_owned(), "claude".to_owned())];
        let mut seen = None;
        let line = missing_line(&mut seen, office(), "office-claude");
        assert_eq!(
            line.as_deref(),
            Some(
                "lamp-voice provider: no target 'office-claude' in the tasks part; it has 'office' (claude)"
            )
        );
        assert_eq!(missing_line(&mut seen, office(), "office-claude"), None);
        let mut both = office();
        both.push(("office-claude".to_owned(), "claude".to_owned()));
        assert_eq!(missing_line(&mut seen, both, "office-claude"), None);
        assert!(missing_line(&mut seen, office(), "office-claude").is_some());
    }

    /// A named member whose target is gone is never swapped for another
    /// engine: with only a Claude Code target there, Codex isn't set up.
    #[test]
    fn a_missing_target_is_never_taken_by_another_engine() {
        let directory =
            std::env::temp_dir().join(format!("agent-tasks-engine-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let _seen = tasks_part(
            &directory,
            vec![(
                "request.sock",
                json!({"ok":true,"targets":[{"alias":"office","engine":"claude"}]}),
            )],
        );
        let place = place(&directory, false);
        assert_eq!(
            missing_target(&place, 3, "laptop", "codex", PEER_WAIT),
            Missing::NotSetUp
        );
        assert_eq!(listed(&[]), "none");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_task_is_prepared_on_the_request_socket_and_started_only_on_the_voice_socket_after_a_yes() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = std::env::temp_dir().join(format!("agent-tasks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let status = |revision: u64| json!({"ok":true,"store_id":"s1","revision":revision,"summary":{"session_id":"sess"}});
        let seen = tasks_part(
            &directory,
            vec![
                ("request.sock", status(0)),
                (
                    "request.sock",
                    json!({"ok":true,"id":7,"prepared_digest":"abc","revision":1}),
                ),
                ("request.sock", status(1)),
                (
                    "voice.sock",
                    json!({"ok":true,"id":7,"queued":true,"revision":2}),
                ),
            ],
        );
        let place = Place {
            directory: directory.clone(),
            target: "office".into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
        };
        let call = |name: &str, args: Value| ToolCall {
            id: "c".into(),
            name: name.into(),
            args,
        };
        let deadline = PEER_WAIT;
        let (told, answered) = run(
            &place,
            &call(START, json!({"task":"Write hello.txt"})),
            3,
            true,
            deadline,
        );
        assert!(answered && told["prepared"] == json!(true), "{told}");
        assert_eq!(seen.recv().unwrap()["op"], "status");
        let prepare = seen.recv().unwrap();
        assert_eq!(prepare["op"], "prepare");
        assert_eq!(prepare["text"], "Write hello.txt");
        assert_eq!(prepare["target"], "office");
        assert_eq!(prepare["guard"]["revision"], 0);
        // The same request cannot start it.
        let (told, _) = run(&place, &call(CONFIRM, json!({})), 3, true, deadline);
        assert!(told.get("error").is_some(), "{told}");
        // The next thing the person said can.
        let (told, answered) = run(&place, &call(CONFIRM, json!({})), 4, true, deadline);
        assert!(answered && told["handing_over"] == json!(true), "{told}");
        assert_eq!(seen.recv().unwrap()["op"], "status");
        let confirm = seen.recv().unwrap();
        assert_eq!(confirm["op"], "confirm");
        assert_eq!(confirm["id"], 7);
        assert_eq!(confirm["prepared_digest"], "abc");
        assert_eq!(confirm["session_id"], "sess");
        assert_eq!(confirm["guard"]["revision"], 1);
        // Started once: nothing is waiting any more.
        let (told, _) = run(&place, &call(CONFIRM, json!({})), 5, true, deadline);
        assert!(told.get("error").is_some(), "{told}");
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// #1673 (the first builder, 19 Oct): work asked for and said yes to by
    /// text is kept as a text's, so its news, and only such work's, is given
    /// in a text. Work asked for at the lamp is not.
    #[test]
    fn work_asked_for_by_text_is_kept_as_a_texts() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory =
            std::env::temp_dir().join(format!("agent-tasks-typed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let status = |revision: u64| json!({"ok":true,"store_id":"typed-s","revision":revision,"summary":{"session_id":"sess"}});
        let prepared = |id: u64| json!({"ok":true,"id":id,"prepared_digest":"abc","revision":1});
        let queued = |id: u64| json!({"ok":true,"id":id,"queued":true,"revision":2});
        let _seen = tasks_part(
            &directory,
            vec![
                ("request.sock", status(0)),
                ("request.sock", prepared(7)),
                ("request.sock", status(1)),
                ("voice.sock", queued(7)),
                ("request.sock", status(2)),
                ("request.sock", prepared(8)),
                ("request.sock", status(3)),
                ("voice.sock", queued(8)),
            ],
        );
        let place = Place {
            directory: directory.clone(),
            target: "office".into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
        };
        let call = |name: &str, args: Value| ToolCall {
            id: "c".into(),
            name: name.into(),
            args,
        };
        let text = crate::messages::REQUESTS_FROM + 1;
        let (told, _) = run(
            &place,
            &call(START, json!({"task":"Write a poem."})),
            text,
            false,
            PEER_WAIT,
        );
        assert!(told["prepared"] == json!(true), "{told}");
        let (told, _) = run(
            &place,
            &call(CONFIRM, json!({})),
            text + 1,
            false,
            PEER_WAIT,
        );
        assert!(told["handing_over"] == json!(true), "{told}");
        // Asked and said yes to at the lamp.
        let (told, _) = run(
            &place,
            &call(START, json!({"task":"Write a story."})),
            20,
            true,
            PEER_WAIT,
        );
        assert!(told["prepared"] == json!(true), "{told}");
        let (told, _) = run(&place, &call(CONFIRM, json!({})), 21, true, PEER_WAIT);
        assert!(told["handing_over"] == json!(true), "{told}");
        let nowhere = directory.join("no-typed-file");
        assert!(crate::task_news::is_typed("typed-s:7:3", &nowhere));
        assert!(!crate::task_news::is_typed("typed-s:8:3", &nowhere));
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// lamp-4ace, 2026-10-15: the tasks part said `uncertain` after 1.1 s and
    /// had prepared the work a moment later. The very same line again gives
    /// back what was done; any other refusal is not asked again.
    #[test]
    fn a_change_the_tasks_part_is_unsure_of_is_asked_for_again_exactly() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory =
            std::env::temp_dir().join(format!("agent-tasks-again-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let status = |revision: u64| json!({"ok":true,"store_id":"s1","revision":revision,"summary":{"session_id":"sess"}});
        let unsure = json!({"ok":false,"error":"uncertain","message":"No receipt arrived."});
        let seen = tasks_part(
            &directory,
            vec![
                ("request.sock", status(0)),
                ("request.sock", unsure.clone()),
                (
                    "request.sock",
                    json!({"ok":true,"id":7,"prepared_digest":"abc","revision":1}),
                ),
                ("request.sock", status(1)),
                ("voice.sock", unsure),
                (
                    "voice.sock",
                    json!({"ok":true,"id":7,"queued":true,"revision":2}),
                ),
                ("request.sock", status(2)),
                (
                    "request.sock",
                    json!({"ok":false,"error":"conflict","message":"Tasks changed."}),
                ),
            ],
        );
        let place = Place {
            directory: directory.clone(),
            target: "office".into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
        };
        let call = |name: &str, args: Value| ToolCall {
            id: "c".into(),
            name: name.into(),
            args,
        };
        let (told, answered) = run(
            &place,
            &call(START, json!({"task":"Write hello.txt"})),
            3,
            true,
            DEADLINE,
        );
        assert!(answered && told["prepared"] == json!(true), "{told}");
        assert_eq!(seen.recv().unwrap()["op"], "status");
        let first = seen.recv().unwrap();
        assert_eq!(first["op"], "prepare");
        assert_eq!(seen.recv().unwrap(), first, "asked again exactly");
        let (told, answered) = run(&place, &call(CONFIRM, json!({})), 4, true, DEADLINE);
        assert!(answered && told["handing_over"] == json!(true), "{told}");
        assert_eq!(seen.recv().unwrap()["op"], "status");
        let first = seen.recv().unwrap();
        assert_eq!(first["op"], "confirm");
        assert_eq!(seen.recv().unwrap(), first, "asked again exactly");
        // A conflict is an answer: asked once, and the model is told.
        let (told, answered) = run(
            &place,
            &call(START, json!({"task":"Write bye.txt"})),
            5,
            true,
            DEADLINE,
        );
        assert!(answered && told.get("error").is_some(), "{told}");
        assert_eq!(seen.recv().unwrap()["op"], "status");
        assert_eq!(seen.recv().unwrap()["op"], "prepare");
        assert!(seen.recv_timeout(Duration::from_millis(300)).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_empty_or_option_like_task_is_refused_before_anything_is_asked() {
        let place = Place {
            directory: "/nonexistent".into(),
            target: "office".into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
        };
        let call = |task: &str| ToolCall {
            id: "c".into(),
            name: START.into(),
            args: json!({"task": task}),
        };
        assert!(
            run(&place, &call("  "), 1, true, Duration::from_secs(1))
                .0
                .get("error")
                .is_some()
        );
        assert!(
            run(
                &place,
                &call("--mode full"),
                1,
                true,
                Duration::from_secs(1)
            )
            .0
            .get("error")
            .is_some()
        );
        // Nobody there: unavailable, not answered.
        assert!(
            !run(
                &place,
                &call("Write a file"),
                1,
                true,
                Duration::from_millis(200)
            )
            .1
        );
    }

    #[test]
    fn asked_how_it_is_going_the_lamp_answers_from_the_last_look_and_keeps_it_told() {
        let directory = std::env::temp_dir().join(format!("agent-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let told = directory.join("told");
        let place = Place {
            directory: directory.clone(),
            target: DEFAULT_TARGET.into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
        };
        let task = json!({"ok":true,"text":"Suggest three names","target":{"engine":"claude"},
            "observations":[{"id":4,"state":"idle"}]});
        let seen = tasks_part(
            &directory,
            vec![
                (
                    "request.sock",
                    // Longer than the executive's 8 KiB bound, as a page of
                    // eight tasks with their records was on lamp-4ace.
                    json!({"ok":true,"store_id":"s1","padding":"x".repeat(9_000),"items":[
                    {"id":1,"dispatch":{"state":"accepted"}},{"id":2,"dispatch":{"state":"accepted"}},
                    {"id":3,"dispatch":{"state":"invalidated"}}]}),
                ),
                ("request.sock", task),
                (
                    "request.sock",
                    json!({"ok":true,"data":{"state":"idle","answer":"Lumi, Sunny and Archie.","truncated":false}}),
                ),
            ],
        );
        let (told_model, answered) = check(&place, None, &told, PEER_WAIT);
        assert!(answered);
        assert_eq!(told_model["who"], "Claude Code");
        assert_eq!(told_model["state"], "idle");
        assert_eq!(told_model["answer"], "Lumi, Sunny and Archie.");
        // The newest task handed over, not the newest prepared.
        assert_eq!(seen.recv().unwrap()["op"], "list");
        assert_eq!(seen.recv().unwrap(), json!({"op":"get_task","id":2}));
        assert_eq!(
            seen.recv().unwrap(),
            json!({"op":"get_observation","id":2,"observation_id":4})
        );
        // Heard now, so not told again unasked.
        assert_eq!(std::fs::read_to_string(&told).unwrap(), "s1:2:4\n");
        assert!(told_model.get("told_before").is_none(), "{told_model}");
        assert!(
            told_model["next"]
                .as_str()
                .unwrap()
                .starts_with("Give them the gist"),
            "{told_model}"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// The tasks part answering one `check`: a page with task 2, the task,
    /// and its observation 4 with `data`.
    fn checked(directory: &Path, told: &Path, state: &str, answer: &str, truncated: bool) -> Value {
        let (place, _seen) = serving(directory, state, answer, truncated);
        let (answered, ok) = check(&place, None, told, PEER_WAIT);
        assert!(ok, "{answered}");
        answered
    }

    /// [`checked`]'s tasks part, ready for one `check`: its place, and what
    /// it was sent, held while it answers.
    pub(crate) fn serving(
        directory: &Path,
        state: &str,
        answer: &str,
        truncated: bool,
    ) -> (Place, mpsc::Receiver<Value>) {
        let _ = std::fs::remove_file(directory.join("request.sock"));
        let _ = std::fs::remove_file(directory.join("voice.sock"));
        let place = Place {
            directory: directory.to_owned(),
            target: DEFAULT_TARGET.into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
        };
        let seen = tasks_part(
            directory,
            vec![
                (
                    "request.sock",
                    json!({"ok":true,"store_id":"s1","items":[{"id":2,"dispatch":{"state":"accepted"}}]}),
                ),
                (
                    "request.sock",
                    json!({"ok":true,"text":"Write a 300-word story about a desk lamp that learns to sing","target":{"engine":"claude"},
                        "observations":[{"id":4,"state":state}]}),
                ),
                (
                    "request.sock",
                    json!({"ok":true,"data":{"state":state,"answer":answer,"truncated":truncated}}),
                ),
            ],
        );
        (place, seen)
    }

    /// #1390: an answer with a finished task's news carries its key, told
    /// before or not (a telling unasked may have been cut short), for the
    /// provider worker to take out before the model is told; one still
    /// working carries none.
    #[test]
    fn an_answer_with_finished_news_carries_its_key_for_the_worker_alone() {
        let directory = std::env::temp_dir().join(format!("agent-news-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let told = directory.join("told");
        for _ in 0..2 {
            let mut answered = checked(&directory, &told, "idle", "Lumi, Sunny and Archie.", false);
            assert_eq!(take_news_key(&mut answered).as_deref(), Some("s1:2:4"));
            assert!(answered.get(NEWS_KEY).is_none(), "{answered}");
            assert_eq!(answered["answer"], "Lumi, Sunny and Archie.");
        }
        let mut working = checked(&directory, &told, "working", "", false);
        assert_eq!(take_news_key(&mut working), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Seventh's peer review on #1427: through the real routes. The Live worker's
    /// asking thread (`provider_worker::answered_scoped`) takes the key out of what
    /// its model is told and keeps it; `tools::answer` takes it out too, and
    /// keeps nothing; the second voice's spoken turn
    /// (`tools::answer_keeping_news`) gets it, to take out itself.
    #[test]
    fn the_key_is_never_the_models_on_either_voice() {
        let directory =
            std::env::temp_dir().join(format!("agent-news-routes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let call = lamp_gemini::ToolCall {
            id: "call-1".into(),
            name: CHECK.into(),
            args: json!({}),
        };
        let (place, _seen) = serving(&directory, "idle", "Lumi, Sunny and Archie.", false);
        let route = crate::tools::Route::Tasks(place);
        let scope = crate::memory_scope::RequestScope::new(
            crate::memory_scope::RequestKey {
                worker: lamp_turn_owner::BootId::new([9; 16]).unwrap(),
                session: 1,
                request: 1,
            },
            crate::memory_scope::ScopeSnapshot::room(),
        );
        let (_, told, answered, news) =
            crate::provider_worker::answered_scoped(&route, &call, 1, true, &scope.lease());
        assert!(answered, "{told}");
        assert_eq!(news.as_deref(), Some("s1:2:4"));
        assert!(told.get(NEWS_KEY).is_none(), "{told}");
        assert_eq!(told["answer"], "Lumi, Sunny and Archie.");

        let (place, _seen) = serving(&directory, "idle", "Lumi, Sunny and Archie.", false);
        let settings = crate::tools::Settings {
            tasks: Some(place),
            ..crate::tools::Settings::default()
        };
        let told = crate::tools::answer(&settings, &call, 2, true);
        assert!(told.get(NEWS_KEY).is_none(), "{told}");
        assert_eq!(told["answer"], "Lumi, Sunny and Archie.");

        // The second voice's spoken turn keeps it for its own turn to take
        // out (`second_provider`'s `news_asked_about_rides_on_its_turn…`).
        let (place, _seen) = serving(&directory, "idle", "Lumi, Sunny and Archie.", false);
        let settings = crate::tools::Settings {
            tasks: Some(place),
            ..crate::tools::Settings::default()
        };
        let mut kept = crate::tools::answer_keeping_news(&settings, &call, 3, true, None);
        assert_eq!(take_news_key(&mut kept).as_deref(), Some("s1:2:4"));
        assert_eq!(kept["answer"], "Lumi, Sunny and Archie.");
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// On lamp-4ace on 2026-10-15 at 08:02 the story was told unasked at
    /// 193 s, and at 205 s "How is that task going?" got the same news
    /// again, as if new (#757). Asked about news already told, the model is
    /// told so, and asked for a short follow-up, not a second announcement.
    #[test]
    fn asked_about_news_already_told_the_lamp_says_it_is_the_one_it_mentioned() {
        let directory =
            std::env::temp_dir().join(format!("agent-told-before-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let told = directory.join("told");
        let story = "Every night, when the office emptied, the desk lamp hummed. ".repeat(30);
        // Told unasked first: its key is in the file.
        crate::task_news::keep_told(&told, "s1:2:4");
        let answered = checked(&directory, &told, "idle", &story, false);
        assert_eq!(answered["told_before"], true, "{answered}");
        let next = answered["next"].as_str().unwrap();
        assert!(
            next.starts_with("You already told them this. Do not tell it again as if it were new"),
            "{next}"
        );
        assert!(next.contains("the one you mentioned"), "{next}");
        // The words are still there, for the gist if none was given.
        assert!(
            answered["answer"]
                .as_str()
                .unwrap()
                .starts_with("Every night")
        );
        // Kept once, not again.
        assert_eq!(std::fs::read_to_string(&told).unwrap(), "s1:2:4\n");

        // Asked twice, never told unasked: the second answer is the short one.
        std::fs::remove_file(&told).unwrap();
        let first = checked(&directory, &told, "idle", "Lumi, Sunny and Archie.", false);
        assert!(first.get("told_before").is_none(), "{first}");
        let second = checked(&directory, &told, "idle", "Lumi, Sunny and Archie.", false);
        assert_eq!(second["told_before"], true, "{second}");
        assert_eq!(std::fs::read_to_string(&told).unwrap(), "s1:2:4\n");

        // A question already told is asked again, shorter; a stop already
        // told is said again without alarm.
        let asking = checked(&directory, &told, "needs-input", "Which folder?", false);
        assert!(
            asking["next"]
                .as_str()
                .unwrap()
                .contains("still waiting for them"),
            "{asking}"
        );
        let stopped = checked(&directory, &told, "stopped", "No repository.", false);
        assert!(
            stopped["next"]
                .as_str()
                .unwrap()
                .starts_with("You already told them it stopped"),
            "{stopped}"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// A long result asked about gets its gist first, then where the rest is.
    #[test]
    fn asked_about_a_long_result_the_gist_comes_before_the_rest_is_on_the_computer() {
        let directory = std::env::temp_dir().join(format!("agent-gist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let told = directory.join("told");
        let story = "Every night, when the office emptied, the desk lamp hummed. ".repeat(30);
        for (answer, truncated) in [(story.as_str(), false), ("A short start.", true)] {
            let _ = std::fs::remove_file(&told);
            let answered = checked(&directory, &told, "idle", answer, truncated);
            assert_eq!(answered["more"], true, "{answered}");
            let next = answered["next"].as_str().unwrap();
            assert!(
                next.starts_with(
                    "Give them the gist of what it came back with in one plain spoken line first"
                ) && next.contains("then say the rest is on their computer; never only that"),
                "{next}"
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// The three functions exactly as they were declared before the person
    /// could name who gets the work (voice `fd1a273ee`).
    fn before() -> Vec<Tool> {
        vec![
            Tool {
                name: START.into(),
                description: "Prepare a piece of computer work for Claude Code on the person's office \
                              computer: writing or changing code, documents or files there. Call it only \
                              when the person asks for such work to be done there. Nothing starts yet: \
                              then say the task back in one short sentence and ask whether to start it. \
                              You can do this: if they ask you to start Claude Code, open a session or \
                              give it something, without saying what it should do, never say you cannot; \
                              ask in a few words what it should work on, and call this with their answer."
                    .into(),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "task": {
                            "type": "string",
                            "description": "The work, in the person's own words, complete enough to do without asking."
                        }
                    },
                    "required": ["task"]
                })),
                silent: false,
            },
            Tool {
                name: CONFIRM.into(),
                description: "Start the task you prepared with start_agent_task. Call it only right \
                              after the person has said yes to the task you read back; never on your \
                              own, and not if they said no or something else."
                    .into(),
                parameters: None,
                silent: false,
            },
            Tool {
                name: CHECK.into(),
                description: "How the latest work you handed over is going, and what the agent came \
                              back with if it has finished. Call it only when the person asks about that \
                              work (\"how's it going?\", \"what did Claude Code say?\"), never on your own: \
                              you are told by yourself when it finishes."
                    .into(),
                parameters: None,
                silent: false,
            },
        ]
    }

    fn member(alias: &str, engine: &str) -> Member {
        Member {
            alias: alias.into(),
            kind: KINDS.iter().find(|kind| kind.engine == engine).unwrap(),
        }
    }
    /// lamp-4ace's two, as `docs/tasks.md` gives them.
    fn two() -> Vec<Member> {
        vec![
            member("office-claude", "claude"),
            member("office-codex", "codex"),
        ]
    }
    /// `start_agent_task` says the staff's note names who only with the
    /// note on and work asked of the staff; otherwise it is as before.
    #[test]
    fn the_description_says_the_staffs_note_names_who_only_with_it() {
        for staff in [Vec::new(), two()] {
            let mut switched = place(Path::new("/run/t"), !staff.is_empty());
            switched.asked = true;
            let start = |switched: &Place| super::declared(switched, &staff)[0].description.clone();
            let before = start(&switched);
            assert!(before.contains(CHOSE), "{before}");
            switched.staff_note = true;
            assert_eq!(start(&switched), before.replacen(CHOSE, CHOSE_NOTED, 1));
            switched.asked = false;
            assert!(!start(&switched).contains("a note may name them"));
        }
    }

    /// The heard words of a request name the staff: two words said
    /// together anywhere, one word only after "ask", "get", "have" or
    /// "tell"; nobody for "the cloud", "tell Claude I'm late" or "which
    /// codec?"; both, in order, for a line naming both.
    #[test]
    fn the_words_of_a_request_name_the_staff() {
        let staff = two();
        let kinds: Vec<&'static Kind> = staff.iter().map(|member| member.kind).collect();
        let named = |heard: &str| -> Vec<&str> {
            kinds_in_words(&kinds, heard)
                .into_iter()
                .map(|kind| kind.name)
                .collect()
        };
        // lamp-4ace, 2026-10-16, 22:51 and about 21:05.
        assert_eq!(
            named("Hey Lamp, ask Codex to write a haiku about coffee."),
            ["Codex"]
        );
        assert_eq!(
            named("Can you ask Claude Code to write a short poem about the sea?"),
            ["Claude Code"]
        );
        for heard in [
            "ask code X to tidy the readme",
            "have codecs look at the build",
            "Get cloud code to write a test.",
            "can you ask Claude Co to write me a limerick",
            "ask Claude to sum it up",
            "Tell Codex to tidy the readme.",
            "Claude Code, write me a test.",
            "ask Claude-Code to check it",
        ] {
            assert_eq!(named(heard).len(), 1, "{heard}");
        }
        for heard in [
            "Save it to the cloud.",
            "Write me a haiku about coffee.",
            "What does the code do?",
            "Tell Claude I'm late.",
            "Which codec does it use?",
            "Claude said it was fine.",
            "Save it to the cloud. Code it later.",
            "Open, code it again.",
        ] {
            assert!(named(heard).is_empty(), "{heard}");
        }
        assert_eq!(
            named("Ask Claude Code and get Codex to check it."),
            ["Claude Code", "Codex"]
        );
        // Without the switch the staff are not read: Claude Code alone.
        let alone = place(Path::new("/nowhere"), false);
        assert_eq!(
            named_in_words(&alone, "ask Claude Code for a poem"),
            ["Claude Code"]
        );
        assert!(named_in_words(&alone, "ask Codex for a poem").is_empty());
    }

    fn place(directory: &Path, who: bool) -> Place {
        Place {
            directory: directory.into(),
            target: DEFAULT_TARGET.into(),
            who,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
        }
    }

    /// One task prepared and started through a tasks part of one socket
    /// pair: what the tasks part was sent (each guard's request id, made
    /// new each time, left out) and what the model was told.
    fn handed_over(
        tag: &str,
        place_who: bool,
        staff: Option<&[Member]>,
        start: Value,
    ) -> (Vec<Value>, Vec<Value>) {
        let directory =
            std::env::temp_dir().join(format!("agent-who-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let status = |revision: u64| json!({"ok":true,"store_id":"s1","revision":revision,"summary":{"session_id":"sess"}});
        let seen = tasks_part(
            &directory,
            vec![
                ("request.sock", status(0)),
                (
                    "request.sock",
                    json!({"ok":true,"id":7,"prepared_digest":"abc","revision":1}),
                ),
                ("request.sock", status(1)),
                (
                    "voice.sock",
                    json!({"ok":true,"id":7,"queued":true,"revision":2}),
                ),
            ],
        );
        let place = place(&directory, place_who);
        let call = |name: &str, args: Value| ToolCall {
            id: "c".into(),
            name: name.into(),
            args,
        };
        let deadline = PEER_WAIT;
        let calls = [(call(START, start), 3), (call(CONFIRM, json!({})), 4)];
        let told: Vec<Value> = calls
            .iter()
            .map(|(call, request)| match staff {
                // As the provider worker runs it, staff and all.
                None => run(&place, call, *request, true, deadline).0,
                Some(staff) => run_for(&place, staff, call, *request, true, deadline).0,
            })
            .collect();
        let sent = (0..4)
            .map(|_| {
                let mut line = seen.recv_timeout(PEER_WAIT).unwrap();
                if let Some(guard) = line.get_mut("guard").and_then(Value::as_object_mut) {
                    assert!(guard.remove("request_id").is_some());
                }
                line
            })
            .collect();
        std::fs::remove_dir_all(directory).unwrap();
        (sent, told)
    }

    /// One task prepared at `place` through a fake tasks part: what the model
    /// was told of it.
    fn prepared_at(tag: &str, place: &Place) -> Value {
        let directory =
            std::env::temp_dir().join(format!("agent-names-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let status =
            json!({"ok":true,"store_id":"s1","revision":0,"summary":{"session_id":"sess"}});
        let _seen = tasks_part(
            &directory,
            vec![
                ("request.sock", status),
                (
                    "request.sock",
                    json!({"ok":true,"id":7,"prepared_digest":"abc","revision":1}),
                ),
            ],
        );
        let place = Place {
            directory: directory.clone(),
            ..place.clone()
        };
        let call = ToolCall {
            id: "c".into(),
            name: START.into(),
            args: json!({"task":"Write hello.txt"}),
        };
        let told = run_for(&place, &[], &call, 3, true, PEER_WAIT).0;
        std::fs::remove_dir_all(directory).unwrap();
        told
    }

    /// #1676: with `LAMP_TASKS_MACHINE_NAMES`, the read-back names the
    /// computer the work goes to by its own name, each of two by its own;
    /// one with no name, or the switch off, reads as today; and task news
    /// says where the rest is the same way.
    #[test]
    fn with_machine_names_each_computer_is_said_by_its_own_name() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let answer = json!({"ok":true,"targets":[
            {"alias":"office","engine":"claude","name":"Office iMac"},
            {"alias":"macbook-air","engine":"claude","name":"MacBook Air"},
            {"alias":"desk","engine":"claude"},
            {"alias":"long","engine":"claude","name":"x".repeat(65)},
            {"alias":"bell","engine":"claude","name":"Ring\u{7}"},
            {"alias":"space","engine":"claude","name":" Pad"}]});
        let names = names_from(&answer).unwrap();
        assert_eq!(names.len(), 2, "{names:?}");
        keep_names(names);
        NAMES_ON.store(true, Ordering::Relaxed);
        let mut on = place(Path::new("/run/t"), false);
        on.machine_names = true;
        for (target, said) in [
            ("office", "Claude Code on your Office iMac"),
            ("macbook-air", "Claude Code on your MacBook Air"),
            ("desk", "Claude Code on the office computer"),
        ] {
            on.target = target.into();
            assert_eq!(prepared_at(target, &on)["where"], said, "{target}");
        }
        let task =
            |alias: &str| json!({"text":"x","target":{"engine":"claude"},"target_alias":alias});
        assert_eq!(
            crate::task_news::their_computer(&task("macbook-air")),
            "their MacBook Air"
        );
        assert_eq!(
            crate::task_news::their_computer(&task("desk")),
            "their computer"
        );
        let mut off = on.clone();
        off.machine_names = false;
        off.target = "macbook-air".into();
        NAMES_ON.store(false, Ordering::Relaxed);
        assert_eq!(
            prepared_at("off", &off)["where"],
            "Claude Code on the office computer"
        );
        assert_eq!(
            crate::task_news::their_computer(&task("macbook-air")),
            "their computer"
        );
        keep_names(BTreeMap::new());
    }

    /// The switch is its own: taken with or without the others, and off
    /// unless `on`.
    #[test]
    fn the_machine_names_switch_is_taken_on_its_own() {
        let flags =
            |list: &[&str]| -> Vec<String> { list.iter().map(|flag| (*flag).to_owned()).collect() };
        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                None,
                None,
                None,
                None,
                Some("on"),
                None
            ),
            flags(&[FLAG, "/run/t", NAMES_FLAG])
        );
        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                None,
                None,
                None,
                None,
                Some("yes"),
                None
            ),
            flags(&[FLAG, "/run/t"])
        );
        let given = flags(&[
            FLAG,
            "/run/t",
            ASKED_FLAG,
            ACCOUNTS_FLAG,
            NAMES_FLAG,
            "rest",
        ]);
        let (place, rest) = take(&given).unwrap();
        let place = place.unwrap();
        assert!(place.asked && place.accounts && place.machine_names);
        assert_eq!(rest, ["rest"]);
        let (place, _) = take(&flags(&[FLAG, "/run/t"])).unwrap();
        assert!(!place.unwrap().machine_names);
    }

    #[test]
    fn without_the_switch_or_anyone_else_to_name_the_functions_are_exactly_as_before() {
        let off = place(Path::new("/run/t"), false);
        assert_eq!(tools(&off), before());
        // Off, whatever the tasks part has.
        assert_eq!(declared(&off, &two()), before());
        // On, but nobody read yet, or one target `office` for Claude Code.
        let on = place(Path::new("/run/t"), true);
        assert_eq!(declared(&on, &[]), before());
        assert_eq!(declared(&on, &[member("office", "claude")]), before());
    }

    /// #799: off, the model hears what it always did after each call, to
    /// the byte; `on` is the only word that turns the lead-in on, and it
    /// comes after the other tasks flags.
    #[test]
    fn without_the_lead_in_what_the_model_is_told_after_a_call_is_as_before() {
        assert_eq!(
            prepared_next(false, false, "Claude Code"),
            "Say the task back in one short sentence and ask whether to start it. Start it only if the person says yes."
        );
        assert_eq!(
            prepared_next(false, true, "Codex"),
            "Say the task back in one short sentence, saying Codex will do it, and ask whether to start it. Start it only if the person says yes."
        );
        assert_eq!(
            handing_over_next(false, "Claude Code"),
            "Tell them in a few natural words that you are handing it to Claude Code now and will let them know when it is done, the way a chief of staff would (\"Sure, I'm giving that to Claude Code. I'll tell you when it's done.\"). It has not been taken up yet: do not say it has."
        );
        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                None,
                Some("on"),
                None,
                None,
                None,
                None
            ),
            [FLAG, "/run/t", LEAD_IN_FLAG]
        );
        assert_eq!(
            arguments(
                Some("/run/t"),
                Some("m2"),
                Some("on"),
                Some("on"),
                None,
                None,
                None,
                None
            ),
            [FLAG, "/run/t", TARGET_FLAG, "m2", WHO_FLAG, LEAD_IN_FLAG]
        );
        for not_on in [None, Some("yes"), Some("ON"), Some("1")] {
            assert_eq!(
                arguments(Some("/run/t"), None, None, not_on, None, None, None, None),
                [FLAG, "/run/t"]
            );
        }
        assert!(arguments(None, None, None, Some("on"), None, None, None, None).is_empty());
        let flags: Vec<String> = [FLAG, "/run/t", WHO_FLAG, LEAD_IN_FLAG, "--other"]
            .map(String::from)
            .into();
        let (place, rest) = take(&flags).unwrap();
        assert!(place.is_some_and(|place| place.who && place.lead_in));
        assert_eq!(rest, ["--other"]);
        let (place, _) = take(&flags[..3]).unwrap();
        assert!(place.is_some_and(|place| !place.lead_in));
    }

    /// #799: with the lead-in, each function asks for the start of its
    /// answer in the same breath, before the call, and its answer says that
    /// was heard, so nothing is said twice.
    #[test]
    fn with_the_lead_in_the_start_of_the_answer_comes_before_the_call_and_is_not_said_again() {
        let mut on = place(Path::new("/run/t"), false);
        on.lead_in = true;
        let declared = tools(&on);
        let described = |name: &str| {
            declared
                .iter()
                .find(|tool| tool.name == name)
                .unwrap()
                .description
                .clone()
        };
        let start = described(START);
        assert!(
            start.contains("Say the task back in one short sentence as a statement, then call it at once; ask whether to start it only after it answers."),
            "{start}"
        );
        assert!(!start.contains("Nothing starts yet: then say the task back"));
        // No question before the call: a question ends a spoken turn, and
        // the call need not come (#799).
        let before_the_call = &start[..start.find("then call it").unwrap()];
        assert!(!before_the_call.contains("and ask"), "{before_the_call}");
        assert!(
            described(CONFIRM).contains("(\"Okay, giving that to Claude Code.\")"),
            "{}",
            described(CONFIRM)
        );
        assert!(described(CHECK).ends_with("(\"Let me see how it's going.\")."));
        // The same functions, parameters and order as without it.
        let off: Vec<(String, Option<Value>)> = before()
            .into_iter()
            .map(|tool| (tool.name, tool.parameters))
            .collect();
        let with: Vec<(String, Option<Value>)> = declared
            .into_iter()
            .map(|tool| (tool.name, tool.parameters))
            .collect();
        assert_eq!(with, off);
        // Afterwards: the question, and nothing said twice.
        assert!(prepared_next(true, false, "Claude Code").starts_with(
            "If you already said the task back, now only ask in a few words whether to start it (\"Shall I start it?\")."
        ));
        assert!(prepared_next(true, true, "Codex").contains("saying Codex will do it, and ask"));
        assert!(
            handing_over_next(true, "Codex")
                .starts_with("They have heard you say you are giving it to Codex.")
        );
        // With a choice of whom, the read-back says who.
        let mut choosing = place(Path::new("/run/t"), true);
        choosing.lead_in = true;
        let start = super::declared(&choosing, &two())
            .into_iter()
            .find(|tool| tool.name == START)
            .unwrap()
            .description;
        assert!(
            start.contains("one short sentence, with who will do it, as a statement, then call it"),
            "{start}"
        );
    }

    /// #1676: with `--tasks-accounts`, the person's calendar, email and
    /// other accounts are asked of the computer's agent, and the lamp's own
    /// reminders are said not to be their calendar. The computer is not
    /// called the office's in the descriptions, the note or the read-back.
    /// Without it, the note and the descriptions are as before.
    #[test]
    fn with_accounts_the_persons_calendar_and_email_are_handed_over_and_no_office_is_named() {
        for staff in [Vec::new(), two()] {
            let mut before = place(Path::new("/run/t"), !staff.is_empty());
            before.asked = true;
            let mut after = before.clone();
            after.accounts = true;
            let note = super::session_note(&before).unwrap();
            assert!(!note.contains("calendar"), "{note}");
            assert!(note.contains("office computer"), "{note}");
            let note = super::session_note(&after).unwrap();
            for said in [
                "calendar, meetings, email and other accounts are on their computer",
                "your timers and reminders are not their calendar",
                "what's on my calendar today",
                "hand it to",
            ] {
                assert!(note.contains(said), "{said}: {note}");
            }
            assert!(!note.contains("office"), "{note}");
            for (tool, plain) in super::declared(&after, &staff)
                .into_iter()
                .zip(super::declared(&before, &staff))
            {
                assert!(!tool.description.contains("office"), "{}", tool.description);
                // Only the office goes: the rest of each description is as before.
                assert_eq!(
                    tool.description,
                    plain.description.replace("office computer", "computer")
                );
            }
        }
    }

    /// `--tasks-accounts` is taken only right after `--tasks-asked`, and the
    /// environment gives it only with `LAMP_TASKS_ASKED=on`: the note that
    /// says it is only there with the hand-over asked every time.
    #[test]
    fn the_accounts_switch_is_on_only_with_asked_and_taken_after_it() {
        let flags =
            |list: &[&str]| -> Vec<String> { list.iter().map(|flag| (*flag).to_owned()).collect() };
        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                None,
                None,
                Some("on"),
                Some("on"),
                None,
                None
            ),
            flags(&[FLAG, "/run/t", ASKED_FLAG, ACCOUNTS_FLAG])
        );
        for not_asked in [None, Some("yes")] {
            assert_eq!(
                arguments(
                    Some("/run/t"),
                    None,
                    None,
                    None,
                    not_asked,
                    Some("on"),
                    None,
                    None
                ),
                flags(&[FLAG, "/run/t"])
            );
        }
        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                None,
                None,
                Some("on"),
                Some("yes"),
                None,
                None
            ),
            flags(&[FLAG, "/run/t", ASKED_FLAG])
        );
        let given = flags(&[
            FLAG,
            "/run/t",
            ASKED_FLAG,
            ACCOUNTS_FLAG,
            NEEDS_FLAG,
            "/usr/local/bin/harness",
            "--next",
        ]);
        let (place, rest) = take(&given).unwrap();
        let place = place.unwrap();
        assert!(place.asked && place.accounts);
        assert_eq!(
            place.needs.as_deref(),
            Some(Path::new("/usr/local/bin/harness"))
        );
        assert_eq!(rest, &given[6..]);
        // Without `--tasks-asked` it is not taken, and is left for the next.
        let given = flags(&[FLAG, "/run/t", ACCOUNTS_FLAG]);
        let (place, rest) = take(&given).unwrap();
        assert!(!place.unwrap().accounts);
        assert_eq!(rest, &given[2..]);
    }

    /// #799: one description the service would refuse leaves a session
    /// with no functions at all (`thread::equipped`). On lamp-4ace on
    /// 2026-10-16, with work asked of the staff, two of them to choose from
    /// and the lead-in, `start_agent_task` was 1,036 bytes, and nothing was
    /// called in either round. Every combination of the switches fits, with
    /// one, two or three of the staff.
    #[test]
    fn every_description_fits_the_service_whatever_the_switches() {
        let three = vec![
            member("office-claude", "claude"),
            member("office-codex", "codex"),
            member("office-opencode", "opencode"),
        ];
        let mut refused = Vec::new();
        for staff in [Vec::new(), two(), three] {
            for (((asked, lead_in), noted), accounts) in
                [(false, false), (false, true), (true, false), (true, true)]
                    .into_iter()
                    .flat_map(|switches| [(switches, false), (switches, true)])
                    .flat_map(|switches| [(switches, false), (switches, true)])
            {
                let mut switched = place(Path::new("/run/t"), !staff.is_empty());
                switched.asked = asked;
                switched.lead_in = lead_in;
                switched.staff_note = noted;
                switched.accounts = accounts && asked;
                for tool in super::declared(&switched, &staff) {
                    if tool.validate().is_err() {
                        refused.push(format!(
                            "{} with {} of the staff, asked {asked}, lead-in {lead_in}, noted {noted}, accounts {accounts}: {} bytes",
                            tool.name,
                            staff.len(),
                            tool.description.len()
                        ));
                    }
                }
            }
        }
        assert!(refused.is_empty(), "{refused:#?}");
    }

    #[test]
    fn without_the_switch_or_anyone_else_to_name_the_tasks_part_and_the_model_hear_what_they_did() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let task = json!({"task":"Write hello.txt"});
        let (sent, told) = handed_over("off", false, None, task.clone());
        assert_eq!(
            sent,
            [
                json!({"op":"status"}),
                json!({"op":"prepare","guard":{"store_id":"s1","revision":0},"text":"Write hello.txt","target":"office"}),
                json!({"op":"status"}),
                json!({"op":"confirm","guard":{"store_id":"s1","revision":1},"id":7,"prepared_digest":"abc","session_id":"sess"}),
            ]
        );
        assert_eq!(
            told,
            [
                json!({"prepared":true,"task":"Write hello.txt","where":"Claude Code on the office computer",
                    "next":"Say the task back in one short sentence and ask whether to start it. Start it only if the person says yes."}),
                json!({"handing_over":true,"who":"Claude Code",
                    "next":"Tell them in a few natural words that you are handing it to Claude Code now and will let them know when it is done, the way a chief of staff would (\"Sure, I'm giving that to Claude Code. I'll tell you when it's done.\"). It has not been taken up yet: do not say it has."}),
            ]
        );
        // Off with a `who` the model was never told of: ignored, as before.
        let named = json!({"task":"Write hello.txt","who":"Codex"});
        assert_eq!(
            handed_over("off-named", false, None, named.clone()),
            (sent.clone(), told.clone())
        );
        // On, with nobody read yet, or only `office`: the same.
        assert_eq!(
            handed_over("on-none", true, Some(&[]), task.clone()),
            (sent.clone(), told.clone())
        );
        let office = [member("office", "claude")];
        assert_eq!(
            handed_over("on-office", true, Some(&office), named),
            (sent, told)
        );
    }

    #[test]
    fn the_person_names_who_gets_the_work_and_nobody_named_is_claude_code() {
        let _one = ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let on = place(Path::new("/run/t"), true);
        let told = declared(&on, &two());
        assert!(told.iter().all(|tool| tool.validate().is_ok()));
        let start = told[0].parameters.as_ref().unwrap();
        assert_eq!(
            start["properties"]["who"]["enum"],
            json!(["Claude Code", "Codex"])
        );
        assert_eq!(start["required"], json!(["task"]));
        let hint = start["properties"]["who"]["description"].as_str().unwrap();
        for heard in ["\"cloud\"", "\"codecs\"", "then Claude Code does it"] {
            assert!(hint.contains(heard), "{hint}");
        }
        assert!(told[0].description.contains("(Claude Code and Codex)"));
        assert_eq!(
            told[2].parameters.as_ref().unwrap()["properties"]["who"]["enum"],
            json!(["Claude Code", "Codex"])
        );
        // All three the tasks part takes still fit what the link carries.
        let three = [two(), vec![member("office-opencode", "opencode")]].concat();
        assert!(
            declared(&on, &three)
                .iter()
                .all(|tool| tool.validate().is_ok())
        );

        for (tag, who, target, name) in [
            ("codex", json!("Codex"), "office-codex", "Codex"),
            ("codecs", json!("codecs"), "office-codex", "Codex"),
            ("cloud", json!("cloud code"), "office-claude", "Claude Code"),
            ("nobody", Value::Null, "office-claude", "Claude Code"),
            ("blank", json!(" "), "office-claude", "Claude Code"),
        ] {
            let mut start = json!({"task":"Write hello.txt"});
            if !who.is_null() {
                start["who"] = who;
            }
            let (sent, told) = handed_over(tag, true, Some(&two()), start);
            assert_eq!(sent[1]["target"], target, "{tag}");
            assert_eq!(sent[1]["text"], "Write hello.txt");
            assert_eq!(told[0]["where"], format!("{name} on the office computer"));
            assert!(
                told[0]["next"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("saying {name} will do it")),
                "{tag}"
            );
            assert_eq!(told[1]["who"], name);
            assert!(
                told[1]["next"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("I'm giving that to {name}."))
            );
        }
    }

    #[test]
    fn a_name_that_is_none_of_the_staff_prepares_nothing_and_the_model_is_told_to_ask() {
        // Nothing listens there: a call that asked the tasks part anything
        // would come back unavailable, not answered.
        let on = place(Path::new("/nonexistent"), true);
        for said in ["Cursor", "code", "OpenCode", "Gemini"] {
            let call = ToolCall {
                id: "c".into(),
                name: START.into(),
                args: json!({"task":"Write a file","who":said}),
            };
            let (told, answered) = run_for(&on, &two(), &call, 1, true, Duration::from_secs(1));
            assert!(answered, "{said}");
            let error = told["error"].as_str().unwrap();
            assert!(error.contains("Claude Code or Codex"), "{error}");
        }
    }

    #[test]
    fn a_close_form_names_one_member_and_never_one_of_another_engine() {
        let staff = two();
        for (said, alias) in [
            ("Claude", "office-claude"),
            ("CLAUDE CODE", "office-claude"),
            ("Cloud", "office-claude"),
            ("cloud-code", "office-claude"),
            ("codex", "office-codex"),
            ("Code X", "office-codex"),
            ("codecs", "office-codex"),
            ("codec", "office-codex"),
            ("office-codex", "office-codex"),
        ] {
            assert_eq!(
                named(&staff, said).map(|member| member.alias.as_str()),
                Some(alias),
                "{said}"
            );
        }
        for said in ["code", "claw", "codes", "open code", "", "Claude and Codex"] {
            assert_eq!(named(&staff, said), None, "{said}");
        }
        // Codex is not here: what sounds like it names nobody.
        assert_eq!(named(&[member("office-claude", "claude")], "codecs"), None);
        // A form two members could share names neither.
        let muddled = [member("codex", "claude"), member("office-codex", "codex")];
        assert_eq!(named(&muddled, "codex"), None);
    }

    #[test]
    fn the_staff_are_the_first_target_of_each_engine_the_voice_knows() {
        let answer = json!({"ok":true,"store_id":"s1","targets":[
            {"alias":"home-claude","engine":"claude"},{"alias":"office-claude","engine":"claude"},
            {"alias":"office-codex","engine":"codex"},{"alias":"office-cursor","engine":"cursor"},
            {"alias":"no spaces","engine":"opencode"},{"alias":"office-opencode","engine":"opencode"}]});
        let staff = staff_from(&answer).unwrap();
        let aliases: Vec<&str> = staff.iter().map(|member| member.alias.as_str()).collect();
        assert_eq!(aliases, ["home-claude", "office-codex", "office-opencode"]);
        assert_eq!(names(&staff, "and"), "Claude Code, Codex and OpenCode");
        // An older tasks part that does not know `targets`, or nonsense.
        assert_eq!(staff_from(&json!({"ok":false,"error":"bad_request"})), None);
        assert_eq!(staff_from(&json!({"ok":true})), None);
        // Nobody named: the target the voice was given, else Claude Code,
        // else the first.
        let alias = |staff: &[Member], target: &str| {
            default_member(staff, target).map(|member| member.alias.clone())
        };
        assert_eq!(
            alias(&two(), "office-codex").as_deref(),
            Some("office-codex")
        );
        assert_eq!(
            alias(&two(), DEFAULT_TARGET).as_deref(),
            Some("office-claude")
        );
        let others = [
            member("office-codex", "codex"),
            member("office-opencode", "opencode"),
        ];
        assert_eq!(
            alias(&others, DEFAULT_TARGET).as_deref(),
            Some("office-codex")
        );
        assert_eq!(alias(&[], DEFAULT_TARGET), None);
        // The news names OpenCode too.
        assert_eq!(
            crate::task_news::who(&json!({"target":{"engine":"opencode"}})),
            "OpenCode"
        );
    }

    #[test]
    fn the_switch_to_name_who_is_on_only_and_follows_the_target() {
        assert_eq!(
            arguments(
                Some("/run/t"),
                Some("office-claude"),
                Some("on"),
                None,
                None,
                None,
                None,
                None
            ),
            [FLAG, "/run/t", TARGET_FLAG, "office-claude", WHO_FLAG]
        );
        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                Some("on"),
                None,
                None,
                None,
                None,
                None
            ),
            [FLAG, "/run/t", WHO_FLAG]
        );
        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                Some("yes"),
                None,
                None,
                None,
                None,
                None
            ),
            [FLAG, "/run/t"]
        );
        assert!(arguments(None, None, Some("on"), None, None, None, None, None).is_empty());
        let flags: Vec<String> = [
            FLAG,
            "/run/t",
            TARGET_FLAG,
            "office-claude",
            WHO_FLAG,
            "--other",
        ]
        .map(String::from)
        .into();
        let (place, rest) = take(&flags).unwrap();
        assert_eq!(
            place,
            Some(Place {
                directory: "/run/t".into(),
                target: "office-claude".into(),
                who: true,
                lead_in: false,
                asked: false,
                accounts: false,
                machine_names: false,
                needs: None,
                staff_note: false,
            })
        );
        assert_eq!(rest, ["--other"]);
        let alone = [FLAG.to_owned(), "/run/t".to_owned(), WHO_FLAG.to_owned()];
        let (place, rest) = take(&alone).unwrap();
        assert!(place.is_some_and(|place| place.who && place.target == DEFAULT_TARGET));
        assert!(rest.is_empty());
    }

    #[test]
    fn asked_about_one_of_them_the_lamp_answers_about_that_ones_newest_work() {
        let directory =
            std::env::temp_dir().join(format!("agent-check-who-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let on = place(&directory, true);
        let list = json!({"ok":true,"store_id":"s1","items":[
            {"id":1,"target_alias":"office-claude","dispatch":{"state":"accepted"}},
            {"id":2,"target_alias":"office-codex","dispatch":{"state":"accepted"}},
            {"id":3,"target_alias":"office-claude","dispatch":{"state":"accepted"}}]});
        let seen = tasks_part(
            &directory,
            vec![
                ("request.sock", list.clone()),
                (
                    "request.sock",
                    json!({"ok":true,"text":"Name a lamp","target":{"engine":"codex"},"observations":[]}),
                ),
                (
                    "request.sock",
                    json!({"ok":true,"store_id":"s1","items":[]}),
                ),
            ],
        );
        let call = |who: &str| ToolCall {
            id: "c".into(),
            name: CHECK.into(),
            args: json!({"who":who}),
        };
        let deadline = PEER_WAIT;
        let (told, answered) = run_for(&on, &two(), &call("codecs"), 5, true, deadline);
        assert!(answered);
        assert_eq!(told["who"], "Codex");
        assert_eq!(told["state"], "working");
        assert_eq!(seen.recv().unwrap()["op"], "list");
        assert_eq!(seen.recv().unwrap(), json!({"op":"get_task","id":2}));
        // Nothing handed to them yet.
        let (told, _) = run_for(&on, &two(), &call("Codex"), 6, true, deadline);
        assert_eq!(
            told["next"],
            "Tell them there is no work handed over to Codex just now."
        );
        // Nobody by that name: nothing is asked.
        let (told, answered) = run_for(&on, &two(), &call("Cursor"), 7, true, deadline);
        assert!(answered && told["error"].is_string(), "{told}");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_staff_read_from_the_tasks_part_are_what_the_next_session_is_told_of() {
        let _staff = STAFF_HELD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = std::env::temp_dir().join(format!("agent-staff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let seen = tasks_part(
            &directory,
            vec![
                (
                    "request.sock",
                    json!({"ok":true,"targets":[{"alias":"office-claude","engine":"claude"},{"alias":"office-codex","engine":"codex"}]}),
                ),
                // An older tasks part: what was known stays.
                ("request.sock", json!({"ok":false,"error":"bad_request"})),
            ],
        );
        let socket = directory.join("request.sock");
        read_staff(&socket, DEFAULT_TARGET, PEER_WAIT, true);
        assert_eq!(seen.recv().unwrap(), json!({"op":"targets"}));
        read_staff(&socket, DEFAULT_TARGET, PEER_WAIT, true);
        assert_eq!(seen.recv().unwrap(), json!({"op":"targets"}));
        let on = place(&directory, true);
        let told = tools(&on);
        assert_eq!(
            told[0].parameters.as_ref().unwrap()["properties"]["who"]["enum"],
            json!(["Claude Code", "Codex"])
        );
        // Without the switch nothing read counts.
        assert_eq!(tools(&place(&directory, false)), before());
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// Asked how it is going while the agent waits with no question text
    /// (Harness 0.4.6 auto mode, #770): it is waiting on their computer,
    /// by name, and no question is asked for it. The other states' words
    /// are byte for byte what they were before.
    #[test]
    fn asked_while_the_agent_waits_with_no_question_the_lamp_says_it_waits_on_their_computer() {
        let waits = next("needs-input", true, "Codex", "", false, false);
        assert_eq!(
            waits,
            "Tell them once, in one short sentence, that Codex is waiting for them on their computer and that they can answer it there. \
             It did not say what it is asking: do not ask a question for it or guess one."
        );
        assert!(!waits.contains("ask its question"));
        let with_words = next(
            "needs-input",
            true,
            "Claude Code",
            "Which folder?",
            false,
            false,
        );
        assert!(
            with_words.contains("Claude Code is waiting for them on their computer")
                && with_words.contains("you may pass it on")
        );
        assert_eq!(
            next("idle", true, "Claude Code", "Lumi.", false, false),
            "Give them the gist of what it came back with in one or two plain spoken sentences; no code, lists, paths or links. If there is more, say the rest is on their computer."
        );
        for state in ["stopped", "failed"] {
            assert_eq!(
                next(state, true, "Claude Code", "x", false, false),
                "Tell them in one plain sentence that it stopped without finishing."
            );
        }
        for (state, settled) in [("idle", false), ("working", false), ("unknown", false)] {
            assert_eq!(
                next(state, settled, "Claude Code", "", false, false),
                "Tell them in a few words that it is still working on it and that you will tell them yourself when it is done. Do not check again unless they ask again."
            );
        }
    }

    /// #854: with the switch, a request to have one of the staff do
    /// something is a hand-over whatever the work, and even in the middle
    /// of a story; a new session is told so after the persona; without it,
    /// every function and every session is as before.
    #[test]
    fn with_work_asked_of_the_staff_every_ask_is_a_hand_over_and_without_it_nothing_changes() {
        let off = place(Path::new("/run/t"), false);
        assert_eq!(declared(&off, &[]), before());
        assert_eq!(session_note(&off), None);
        assert_eq!(noted(Some("time".into()), Some(&off)), Some("time".into()));
        assert_eq!(noted(None, None), None);

        let mut on = off.clone();
        on.asked = true;
        let told = declared(&on, &[]);
        let start = &told[0].description;
        for words in [
            "Hand a piece of work to Claude Code on the person's office computer.",
            "whenever the person asks to have Claude Code do something",
            "\"ask Claude Code to …\"",
            "as much a story, a poem, a plan or anything you could do yourself",
            "even in the middle of something else, a story you are telling included",
            "never do the work yourself instead, never begin it, and never offer to",
            "Call it too when they ask for work to be done on their office computer.",
        ] {
            assert!(start.contains(words), "{words}: {start}");
        }
        // What it takes, and the other two functions, are as before.
        assert_eq!(told[0].parameters, before()[0].parameters);
        assert_eq!(told[1..], before()[1..]);
        let note = session_note(&on).unwrap();
        assert!(
            note.starts_with("\nWhen the person asks to have Claude Code do something, it is theirs to do, not yours: hand it over with start_agent_task every time"),
            "{note}"
        );
        assert!(note.contains("even in the middle of a story you are telling"));
        // After it, the lamp's own (2026-10-16): a story asked of it is
        // told, and a name heard garbled still names the staff.
        assert!(note.contains(
            "Never do it yourself instead. A name that comes to you garbled, such as \"Claude Co\" for Claude Code, still names them. Work on their office computer is theirs too, named or not. But a story, a poem or anything else they ask of you that names neither Claude Code nor work on their office computer, however long, is yours: tell it yourself, gladly, and never offer it to Claude Code or ask whether Claude Code should, even after you handed something over earlier in this conversation."
        ));
        // The reviewer on #903: the note and the function agree that work on
        // the office computer is handed over, named or not.
        assert!(
            told[0].description.contains(
                "Call it too when they ask for work to be done on their office computer."
            )
        );
        assert_eq!(
            noted(Some("time".into()), Some(&on)),
            Some(format!("time{note}"))
        );
        assert_eq!(noted(None, Some(&on)), Some(note));

        assert_eq!(
            arguments(
                Some("/run/t"),
                None,
                Some("on"),
                Some("on"),
                Some("on"),
                None,
                None,
                None
            ),
            [FLAG, "/run/t", WHO_FLAG, LEAD_IN_FLAG, ASKED_FLAG]
        );
        for not_on in [None, Some("yes"), Some("ON"), Some("1")] {
            assert_eq!(
                arguments(Some("/run/t"), None, None, None, not_on, None, None, None),
                [FLAG, "/run/t"]
            );
        }
        let flags: Vec<String> = [FLAG, "/run/t", LEAD_IN_FLAG, ASKED_FLAG, "--other"]
            .map(String::from)
            .into();
        let (place, rest) = take(&flags).unwrap();
        assert!(place.is_some_and(|place| place.asked && place.lead_in && !place.who));
        assert_eq!(rest, ["--other"]);
    }

    /// #854: a name the model could not quite make out is given as the one
    /// it thinks was meant, said in the read-back, which waits for a yes:
    /// asked about first as well, the person was asked twice on lamp-4ace.
    /// The names offered are the staff's, with the switch or without it.
    #[test]
    fn a_name_not_made_out_is_named_in_the_read_back_and_asked_about_once() {
        let staff = two();
        let mut on = place(Path::new("/run/t"), true);
        let who = |place: &Place| {
            declared(place, &staff)[0].parameters.as_ref().unwrap()["properties"]["who"].clone()
        };
        let before = who(&on);
        assert!(
            before["description"]
                .as_str()
                .unwrap()
                .ends_with("Leave it out if they named nobody: then Claude Code does it."),
            "{before}"
        );

        on.asked = true;
        assert_eq!(who(&on)["enum"], before["enum"]);
        assert_eq!(who(&on)["enum"], json!(["Claude Code", "Codex"]));
        assert!(
            who(&on)["description"].as_str().unwrap().ends_with(
                "then Claude Code does it. If you could not quite make out the name they said, \
                 give the one you think they meant, and do not ask about it first: the task said \
                 back names who will do it, so they can put you right."
            ),
            "{}",
            who(&on)
        );
        // The read-back names who, and starts nothing without a yes.
        assert!(
            prepared_next(false, true, "Claude Code").contains("saying Claude Code will do it")
        );
        assert!(
            prepared_next(false, true, "Codex").ends_with("Start it only if the person says yes.")
        );
        // Named, nobody named, and a name nobody has, as before.
        assert_eq!(
            chosen(&on, &staff, &json!({"who": "Codex"}))
                .unwrap()
                .unwrap()
                .kind
                .name,
            "Codex"
        );
        assert_eq!(
            chosen(&on, &staff, &json!({})).unwrap().unwrap().kind.name,
            "Claude Code"
        );
        assert!(chosen(&on, &staff, &json!({"who": "George Jones"})).is_err());
        let start = &declared(&on, &staff)[0].description;
        assert!(
            start.contains("(\"ask Claude Code to …\", \"get Codex to …\")"),
            "{start}"
        );
    }
}
