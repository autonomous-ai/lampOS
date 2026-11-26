//! A skill made by voice, by the lamp's own model, with no computer
//! (Fourteen's proposal on #4710, 2 Nov; the only maker since the first
//! builder's 3 Nov ruling on the Queue): with `LAMP_MAKE_SKILL=on`
//! ([`made_skill::ENVIRONMENT`]). The model calls [`made_skill::MAKE`] with
//! the person's request, and its [`made_skill::brief`] goes to the lamp's
//! own chat (the second route's, `llm_*`), streamed, once more after a 404,
//! a 5xx, a timeout or a broken connection ([`again`]).
//!
//! Every gate of a made skill stays: the answer is taken, held to what a
//! made skill may use, checked and packed by `lamp-skills pack` and kept
//! ([`made_skill::keep`]). Once kept it goes on at once, with no spoken
//! yes (the owner, 10 Nov: a made skill goes on with no spoken yes; the
//! app's MAKE IT and the voice ask are the ask, #5977), through
//! [`made_skill::put_on`] and the manage helper, which installs only those
//! bytes by their SHA-256, held again to the cap. The lamp then says one
//! short line, [`made_skill::on_line`] ("I made evening dim, and it's
//! on."), or [`made_skill::not_on_line`] with why it isn't on. An answer
//! that doesn't pass is said as not made, and nothing is kept.
//!
//! [`made_skill::MAKE`] is the voice's own, whether or not a tasks part is
//! there: a computer's agent is never handed a skill to make. With
//! `LAMP_MAKE_SKILL_AGENT=on` (on in the product's lamp-voice) a voice make the
//! lamp's model didn't make goes to the lamp's own agent when it is
//! Hermes, the one engine with a namespace runner for a make today
//! ([`crate::make_skill_agent`]): DeepSeek
//! then gets one try and no repairs, and the agent gets them.
use crate::made_skill::{self, Keeping, Made, REMINDER_INSTEAD, REMINDER_IS_TIMER, Refused};
use crate::second_provider::net::{self, Chat, ChatEvent, Message};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// The longest one answer is waited for, from the request to its end.
pub const WRITE_WITHIN: Duration = Duration::from_secs(30);
/// Every make straight to the lamp's agent (#7245): `on`, and nothing
/// else; off unless set.
pub const AGENT_FIRST_ENVIRONMENT: &str = "LAMP_MAKE_AGENT_FIRST";
/// The most times a refused answer goes back to the model with what was
/// refused (the owner, 10 Nov: "re-run it yourself until it works"; one,
/// before: lamp-74ff's "come back to my desk" failed its one repair).
pub const MOST_REPAIRS: usize = 3;
/// What a DeepSeek repair of a make judged a quick reaction is refused as
/// when it is none ([`made_skill::quick_reaction`]): never kept and never
/// said, as the middle form takes its place.
const NOT_A_REACTION: &str = "a repaired draft that is no quick reaction";
/// The longest a make goes on: no repair is begun once one more round
/// ([`ROUND_WITHIN`]) would end past it. 120 s, the longest a make with
/// one repair could take before (two answers, each asked once more).
pub const MAKE_WITHIN: Duration = Duration::from_secs(120);
/// The longest one round of asking takes: [`written`] asks a cut or failed
/// answer once more, so two answers (the readers' P2 on #6054: a repair
/// begun at 89 s, guarded by one answer alone, could end near 149 s while
/// the voice's `make_skill` held the turn).
pub const ROUND_WITHIN: Duration = WRITE_WITHIN.saturating_mul(2);

/// The longest a make goes on: [`MAKE_WITHIN`], and with
/// `LAMP_MAKE_SKILL_AGENT` the agent's own part after it
/// ([`crate::make_skill_agent::MAKE_WITHIN`]).
pub fn longest_make() -> Duration {
    if crate::make_skill_agent::on() {
        MAKE_WITHIN + crate::make_skill_agent::MAKE_WITHIN
    } else {
        MAKE_WITHIN
    }
}

/// Whether a repair begun `elapsed` into a make still ends within
/// [`MAKE_WITHIN`], its answer asked twice at worst.
fn round_fits(elapsed: Duration) -> bool {
    elapsed + ROUND_WITHIN <= MAKE_WITHIN
}
/// The most of an answer read: two files of at most 8 KiB, and the fences.
const MOST_ANSWER_BYTES: usize = 20 * 1024;

/// [`MOST_ANSWER_BYTES`], and with a made skill's code on (#6307) its
/// code's most too ([`made_skill::MOST_SCRIPTS_BYTES`]) and its fences.
fn most_answer_bytes() -> usize {
    if made_skill::scripts_on() {
        MOST_ANSWER_BYTES + made_skill::MOST_SCRIPTS_BYTES + 1024
    } else {
        MOST_ANSWER_BYTES
    }
}
/// What a made skill's answer is kept from: this, and when it was made.
pub const FROM: &str = "local:";
/// A made skill that `lamp-skills pack` and [`made_skill::capped`] take,
/// shown to the model whole: the brief alone names the fields, and the
/// lamp's own model, without a whole skill, wrote fields the check refused (lamp-9adf, 2 Nov 16:10 and 16:12: a
/// trigger field and a light step it doesn't know). Named as an example,
/// with a level and a time a request seldom names: given back as an
/// answer, it is refused ([`is_example`]).
const EXAMPLE: &str = "```skill.toml
kind = \"installable\"
version = \"1.0.0\"
needs = [\"light\"]

[permissions]
topics = [\"room/presence\"]
light = true

[trigger]
topic = \"room/presence\"
field = \"someone\"
equals = true
for = \"5s\"
cooldown = \"1h\"
rearm = \"ends\"

[ask]
policy = \"nudge\"

[[do]]
light = { colour = \"cyan\", level = 2, for = \"25s\" }
```

```SKILL.md
---
name: example-only
description: An example of the form only: glows cyan when someone comes back to the desk.
---

# Example only

When someone is back at the lamp, it glows cyan for a moment.
```";

/// How a light "blinks" or "flashes" in a made skill, and what `out` is,
/// told the model with the other light steps: the format has no blink or
/// flash (`lamp_skills::light_call` takes a breathe, candle or pulse
/// pattern only), and `out` takes nothing but `over` and `until`. On
/// lamp-9adf, 11 Nov 17:0x, "blink the light green twice when I arrive"
/// was written as an `out` step with a colour and a pattern, and refused on
/// the first answer and on every repair ("out takes no colour, dim, level,
/// for, pattern or no_brighter: only over and until").
const BLINK: &str = "A light never blinks or flashes: for a blink or a flash, write one \
     colour step with a soft pulse, light = { colour = \"green\", level = 3, for = \"5s\", \
     pattern = { kind = \"pulse\" } } (a pulse needs level 2 to 5 and plays once), and say \
     the blinking as left out. { out = true } only turns the light off: beside it only over \
     and until.";

/// [`BLINK`] with a made skill's light shows (#7245, told with
/// [`made_skill::ASKS_ENVIRONMENT`] on, as [`SHOWS_RULES`] is): a blink or a
/// flash in a skill started by its asks alone is a show alternating levels
/// at 2 s frames, the fastest a frame may be, the faster flashing said as
/// left out; elsewhere one pulse step, as [`BLINK`] says.
const BLINK_SHOWS: &str = "A light never blinks or flashes faster than a show's 2s frame: for \
     a blink or a flash in a skill started by its asks alone, write a show (below) of its colour \
     alternating level 5 and level 1, for = \"2s\" each, as long as asked up to a minute, and \
     say the faster flashing as left out; in any other skill, one colour step with a soft \
     pulse, light = { colour = \"green\", level = 3, for = \"5s\", pattern = { kind = \
     \"pulse\" } }, and say the blinking as left out. { out = true } only turns the light off: \
     beside it only over and until.";

/// A made skill's light show (#7245, lamp-skills' #1679 format), told with
/// [`made_skill::ASKS_ENVIRONMENT`] on: lamp-skills plays a show only in a
/// skill a person asks for, its tool or its own words alone.
pub const SHOWS_RULES: &str = "A skill started by its asks alone (below) may play a light show: one \
     [[do]] step show = \"traffic-light\", light = true in [permissions], and one more fenced \
     block named ```shows/traffic-light.toml of 2 to 30 [[light]] frames played in turn, each \
     colour = \"green\", level = 3 (1 to 5), for = \"3s\" (2s to 20s) and an optional \
     pattern as a light step's, a minute in all at most. Colours in turn, or repeated \
     (\"green, then yellow, then red, three seconds each, five times\"), are one show with \
     every frame written out in order; what is past a minute is said as left out.";

/// The check's lines a repair carries when the answer was [`EXAMPLE`]
/// given back: what the model is told, once.
const EXAMPLE_GIVEN_BACK: &str =
    "that is the example, not what was asked; make the skill the person asked for";

/// Whether `answer` is [`EXAMPLE`] given back (lamp-9adf, 3 Nov 16:15:
/// "when I come back to my desk, glow warm" was answered with the example
/// word for word, kept and offered as a cyan glow): its skill.toml the
/// example's but for whitespace, or its SKILL.md named as the example.
fn is_example(answer: &str) -> bool {
    let (Ok(given), Ok(example)) = (made_skill::files(answer), made_skill::files(EXAMPLE)) else {
        return false;
    };
    let bare = |text: &str| text.split_whitespace().collect::<String>();
    let name = |card: &str| {
        card.lines()
            .find_map(|line| line.trim().strip_prefix("name:"))
            .map(|name| name.trim().trim_matches('"').to_owned())
    };
    bare(&given.toml) == bare(&example.toml)
        || name(&given.card).is_some_and(|given| Some(given) == name(&example.card))
}

/// What the model is told it is, before the brief: the format, a whole
/// skill the lamp's check takes, and what a skill may use and read
/// ([`services`]), with a made skill's reads as this process has them.
fn system() -> String {
    system_with(made_skill::reads_on())
}

/// [`system`], with a made skill's reads on or off.
fn system_with(reads: bool) -> String {
    system_for(reads, made_skill::agent_services_here())
}

/// [`system_with`], with a linked service made as a task for the lamp's
/// agent or not (`agent`, [`made_skill::AGENT_SERVICES_ENVIRONMENT`]): on
/// only with a made skill's code on too ([`made_skill::scripts_on`]), as a
/// made `task` step is held to that cap; else the prompt is as before.
fn system_for(reads: bool, agent: bool) -> String {
    system_net(reads, agent, crate::make_no_cannot::net_by_agent())
}

/// What the make's model is told of what only the internet tells, with
/// `LAMP_TASK_NET` on ([`system_net`]): answered
/// [`crate::make_no_cannot::NEEDS_NET`], a task the lamp sets up itself.
fn net_told() -> String {
    format!(
        "What only the internet tells at the time it runs (the weather or a forecast, the news, \
         a web lookup, a price, anything from the internet) is no step and no code of a skill's \
         own: the lamp's agent looks it up each time, in a task the lamp sets up itself and \
         reads back for the person's yes. A request that needs it, whenever it runs, is answered \
         with one line and no blocks: {needs_net} and what it looks up, in a few plain words \
         (\"{needs_net} the weather forecast\"); never {cannot} for it, and never a say telling \
         the person to look it up themselves.",
        needs_net = crate::make_no_cannot::NEEDS_NET,
        cannot = made_skill::CANNOT,
    )
}

/// [`net_told`] for a make on a gateway agent whose run granted `net`
/// reaches the web from its own skill-start fence (#7742,
/// [`net_code_by`]): its own code fetches it, granted `net`.
fn net_code_told() -> String {
    format!(
        "What only the internet tells at the time it runs (the weather or a forecast, the news, \
         a web lookup, a price, anything from the internet) is the skill's own code on this \
         lamp, as told at the end, with \"{net}\" in its grants, beside any other grant its code \
         uses (\"notes\" when it keeps what it found for later and reads it back): its script \
         fetches it over https with Python's standard library alone (urllib.request; the lamp \
         gives the run its web proxy, and it reaches only the public web on ports 80 and 443). \
         A skill a time, an event or a repeat starts speaks only through the lamp's told \
         answer: no \"speak\" in its grants and no lamp.say; its script prints what it found (a \
         plain line when the fetch fails), and the lamp's agent that ran it ends with that in a \
         sentence or two, which the lamp says when the person is there. Only a skill the \
         person's words start may have \"speak\" too and say what it found with lamp.say, then \
         print only \"{said}\", so the agent ends with a dash and it is heard once. Never \
         {cannot} for it, never {needs_net} for it, and never a say telling the person to look \
         it up themselves.",
        net = crate::skill_task_yes::NET.0,
        said = NET_CODE_SAID,
        needs_net = crate::make_no_cannot::NEEDS_NET,
        cannot = made_skill::CANNOT,
    )
}

/// What a web make's own code ([`net_code_told`]) prints once it has said
/// what it found with `lamp.say`: the lamp's agent that ran it, asked to
/// end with a dash for a skill that already said it aloud through the lamp
/// (lamp-tasks' `TELL_LINE`), sees it, so the person hears it once.
const NET_CODE_SAID: &str = "Said aloud through the lamp.";

/// Whether a make handed to the lamp's agent `engine` may write its own
/// code granted `net` (#7742): OpenClaw, a gateway agent whose run's
/// script reaches the web from its own skill-start fence, with
/// `LAMP_TASK_NET_GATEWAY` on (`skill_task_yes::gateway_net_on`) and the
/// `internet:` make on ([`crate::make_no_cannot::net_by_agent`]). Else the
/// make is told and checked as before.
pub(crate) fn net_code_by(engine: &str) -> bool {
    engine == "OpenClaw"
        && crate::skill_task_yes::gateway_net_on()
        && crate::make_no_cannot::net_by_agent()
}

/// What DeepSeek is told after [`net_told`] for a make on a lamp whose
/// agent writes a web make as code granted `net` ([`net_code_by`], #7742;
/// lamp-c0a4, 18 Nov 20:06: a bitcoin price every 3 minutes made as a
/// task for the agent with the services example's script).
fn net_not_agent_task_told() -> String {
    format!(
        "A task for the lamp's agent is only for a linked service; anything from the internet \
         is the {needs_net} line, never such a task.",
        needs_net = crate::make_no_cannot::NEEDS_NET,
    )
}

/// What DeepSeek is told of a linked-service skill's script: the one-line
/// hand-over to the lamp's agent ([`AGENT_SERVICES_EXAMPLE`]).
const STUB_SCRIPT_TOLD: &str = "and one ```scripts/main.py block that is exactly the example's one line, which holds no token.";

/// What the lamp's agent is told in its place (#8157): its scripts are
/// written whole at the make.
const AGENTS_SCRIPT_TOLD: &str = "and its scripts written now, whole, never the example's one \
     line: scripts/main.py reads the service itself with await lamp.service_read(service, args) \
     and prints what you sum up at each run, with any helper script beside it; a script never \
     holds a token.";

/// The making rules as the lamp's agent is given them (#8157): DeepSeek's,
/// a linked-service skill's one-line script told as the agent's whole
/// scripts ([`AGENTS_SCRIPT_TOLD`]).
fn agents_own_scripts(system: &str) -> String {
    system.replacen(STUB_SCRIPT_TOLD, AGENTS_SCRIPT_TOLD, 1)
}

/// [`AGENT_SERVICES_EXAMPLE`]'s script: it only says the task is the
/// lamp's agent's. Tests only: the lib reads the example whole.
#[cfg(test)]
const AGENT_SERVICES_SCRIPT: &str =
    "print(\"The lamp's agent does this task with its lamp-services tools.\")";

/// Whether DeepSeek's `answer` is a skill for the web made as a task for
/// the lamp's agent, with no linked service's `-read` or `-write` grant:
/// its one script only prints a line handing the task to the agent (the
/// lamp's own [`crate::make_no_cannot::SCRIPT`], `AGENT_SERVICES_SCRIPT`
/// or its like), or its code holds the `internet:` line
/// ([`crate::make_no_cannot::says_needs_net`]; lamp-c0a4, 18 Nov 20:18:
/// `print("internet: the current gold price")`). On a lamp whose agent
/// writes a web make as code ([`net_code_by`], #7742), what only the
/// internet tells, never kept as DeepSeek's.
fn web_task_for_agent(answer: &str) -> bool {
    let Ok(files) = made_skill::files(answer) else {
        return false;
    };
    let service = files
        .toml
        .parse::<toml::Table>()
        .ok()
        .and_then(|table| table.get("grants").and_then(toml::Value::as_array).cloned())
        .into_iter()
        .flatten()
        .filter_map(|grant| grant.as_str().map(str::to_owned))
        .any(|grant| grant.ends_with("-read") || grant.ends_with("-write"));
    let says_net = files.scripts.iter().any(|(_, text)| {
        text.lines()
            .filter_map(quoted)
            .any(crate::make_no_cannot::says_needs_net)
    });
    let hands_over = match files.scripts.as_slice() {
        [(_, text)] => {
            printed_only(text).is_some_and(|said| said.to_ascii_lowercase().contains("agent"))
        }
        _ => false,
    };
    !service && (says_net || hands_over)
}

/// The first string `line` holds, within its quotes.
fn quoted(line: &str) -> Option<&str> {
    let at = line.find(['"', '\''])?;
    let mark = line[at..].chars().next()?;
    let rest = &line[at + 1..];
    rest.find(mark).map(|end| &rest[..end])
}

/// What a script of one `print(...)` line alone prints.
fn printed_only(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.contains('\n') && text.starts_with("print(") && text.ends_with(')'))
        .then(|| quoted(text))
        .flatten()
}

/// [`system_for`], with what only the internet tells answered
/// [`crate::make_no_cannot::NEEDS_NET`] (`net`,
/// [`crate::make_no_cannot::net_by_agent`]: `LAMP_TASK_NET` on) or the
/// model's `cannot:` as before. The rules file the lamp's agent makes from
/// is this same text, so it is told the same.
fn system_net(reads: bool, agent: bool, net: bool) -> String {
    let (cannot, left_out) = (made_skill::CANNOT, made_skill::LEFT_OUT);
    let agent = agent && made_skill::scripts_on();
    let writes = made_skill::writes_on();
    // Today and the offset only for a write's calendar event (#6066).
    let today = (writes && !agent)
        .then(crate::local_time::today_and_offset_here)
        .flatten();
    let services = if agent {
        agent_services()
    } else {
        services(reads, writes, &own_accounts(), today.as_ref())
    };
    // A send through the lamp's agent is one of what a skill may do, as a
    // made write is.
    let writes = writes || agent;
    // With a made skill's writes (#6066) a write to a linked service is
    // one of what a skill may do, told below; a text to a phone stays out
    // of reach. Its can't-yet example is then no send: on lamp-9adf, 11
    // Nov, writes on, every mail and calendar make came back `cannot:`
    // within 1.5 s, the may-do list naming no write and the example
    // refusal "sending a message".
    let (may_also, cant_send, cant_example) = if writes {
        (
            "a read or a write to a linked service as below",
            "or send a message to a phone (a mail or a calendar event is no message to a \
             phone: it is a write, allowed below)",
            "hearing what is said",
        )
    } else {
        (
            "a read as below",
            "send a message or write to a service",
            "sending a message",
        )
    };
    // A made skill loosened (`made_skill::LOOSE_ENVIRONMENT`): a say as
    // often as asked, at least a minute apart, and no limits the request
    // didn't ask for. Off, an hour apart, and the lamp sets the limits.
    let (cooldown, limits) = if made_skill::loose_on() {
        (
            "a cooldown of \"1m\" or longer, as often as the request asks",
            "for limits in its own words.",
        )
    } else {
        (
            "cooldown = \"1h\" or longer",
            "for more in its own words: the lamp sets them.",
        )
    };
    let triggers = made_skill::TRIGGER_MEANINGS;
    let topics = made_skill::readable_named();
    let gestures = made_skill::gesture_names();
    let (more, cant_yet) = (made_skill::MORE_STEPS, made_skill::CANT_YET);
    // Code of its own (#6307), told only with its switch: off, the model
    // never hears that a skill may carry code, and the prompt is as before.
    let (blocks, code) = if made_skill::scripts_on() {
        (
            "the two fenced blocks, and any code blocks as told at the end,",
            format!("\n{}", code_rules()),
        )
    } else {
        ("the two fenced blocks", String::new())
    };
    // A start said to the lamp (#6526), told only with its switch: off, the
    // model still hears that a skill can't hear what is said, and the
    // prompt is as before.
    // Its own words (the owner, 16 Nov: "no 'couldn't' here"), told only
    // with their switch, in place of a [said] start: on, a skill may start
    // when the person says the words they name for it, with any step. With
    // both switches on, both are told (#6526): a request told to the lamp
    // as a [said] start, and the words named to start it as asks.
    let (hear, said) = if made_skill::asks_on() && made_skill::said_on() {
        (
            "hear what is said in the room, other than a request told to the lamp (a [said] \
             start) or the words the person names to start it (asks), both below",
            format!("\n{}\n{}", made_skill::SAID_RULES, made_skill::ASKS_RULES),
        )
    } else if made_skill::asks_on() {
        (
            "hear what is said in the room, other than the words the person names to start it \
             (asks, below)",
            format!("\n{}", made_skill::ASKS_RULES),
        )
    } else if made_skill::said_on() {
        (
            "hear what is said in the room (a request told to the lamp is a [said] start, \
             below)",
            format!("\n{}", made_skill::SAID_RULES),
        )
    } else {
        ("hear what is said", String::new())
    };
    // Its can't-yet example, with writes on, is then the room's talk alone.
    let cant_example = match cant_example {
        "hearing what is said" if made_skill::said_on() || made_skill::asks_on() => {
            "a camera view from another room"
        }
        other => other,
    };
    // With the lamp's agent reading linked services (`agent`), mail and a
    // calendar are read by its task, told below: no "can't find out" for
    // them, which a strict agent took over that rule (#7245).
    let unread = if agent {
        ""
    } else {
        "a summary of mail or calendar content no read below gives, "
    };
    let like_unread = if agent {
        ""
    } else {
        ", like a summary of mail or calendar content no read below gives"
    };
    // What a skill can't find out when it runs (lamp-9adf, 17 Nov: "check
    // the Bitcoin price ... and tell me" refused by DeepSeek and by Hermes,
    // each told a price is "anything from the internet" a skill can't
    // find out). With `LAMP_TASK_NET` on, what only the internet tells is
    // the lamp's agent's, in a task the lamp sets up itself
    // (`make_no_cannot::NEEDS_NET`); off, as before, byte for byte.
    let found_out = if net {
        format!(
            "A skill also can't find out anything at the time it runs that no step here or \
             below gives{like_unread}. A \
             request that needs it is answered the same way, one line and no blocks: {cannot} \
             \"{cant_yet}\" and what, in plain words (\"{cant_yet} summing up your mail.\"); \
             never a say telling the person to check, look up or read it themselves. \
             {internet}",
            internet = net_told(),
        )
    } else {
        format!(
            "A skill also can't find out anything at the time it runs that no step here or \
             below gives: the weather or a forecast, the news, a web lookup, a price, \
             {unread}anything from the internet. A \
             request that needs it is answered the same way, one line and no blocks: {cannot} \
             \"{cant_yet}\" and what, in plain words (\"{cant_yet} checking the weather for \
             you.\"); never a say telling the person to check, look up or read it themselves."
        )
    };
    // #7245 (lamp-c0a4, 18 Nov: OpenClaw's model answered `cannot:` 4 of 5
    // makes of a notes skill): its own code and, with notes on, a note
    // named among what a skill may do, before the cannot rule; with notes
    // on, a note or code checked before a cannot, and what the maker knows
    // now no lookup. Off, as before, byte for byte.
    let notes = made_skill::notes_on() && made_skill::scripts_on();
    let own = match (made_skill::scripts_on(), notes) {
        (true, true) => "its own code (a script), a note it keeps, ",
        (true, false) => "its own code (a script), ",
        (false, _) => "",
    };
    let check_steps = match notes {
        true => format!(" {CHECK_STEPS}"),
        false => String::new(),
    };
    let found_out = match made_skill::notes_on() {
        true => format!("{found_out} {KNOWN_NOW}"),
        false => found_out,
    };
    // #7245: with asks on, a skill started by them may play a light show,
    // so a blink is a show. Off, as before, byte for byte.
    let (blink, steps) = if made_skill::asks_on() {
        (BLINK_SHOWS, format!("\n{SHOWS_RULES}"))
    } else {
        (BLINK, String::new())
    };
    // With the lamp's agent reading linked services, a daily read of one
    // reads as a reminder at a time to a strict agent, which then answered
    // the reminder's cannot (#7245: 2 of 3 played calendar makes on
    // lamp-c0a4 with only `unread`). Off, nothing is added.
    let at_a_time = if agent {
        " A read or a send of the person's mail, calendar or another linked service \
         at a time each day, on named days or on the words they name is no reminder and \
         no cannot: it is a skill made as told below, that time its clock trigger."
    } else {
        ""
    };
    format!(
        "You write skills for a lamp. Answer exactly as the request says, with {blocks} \
         and nothing else. Use only the keys and values the request names, written as \
         in this example, which the lamp's check takes:\n\n{EXAMPLE}\n\nThe example shows \
         the form only: never answer with it. {NAMED_TOLD} {REMINDER_IS_TIMER}, by the clock trigger \
         below.{at_a_time} A skill made on this lamp may do whatever the lamp can do: make what \
         the request asks with the light, the head, a few exact words it says, its hum, \
         {own}or {may_also}, on one of the triggers below. {more} {blink} To speak, a skill has speak = \
         true in [permissions], {cooldown}, and one [[do]] step say = {{ text = \
         \"Welcome back!\" }}: the exact words, at most 300 characters, no quotes, in the \
         language the person asked in, said as written. {CHANGING_LINE} Its one sound is a soft hum: a [[do]] \
         step cue = \"hum\", paced as a say, in place of a say; it has no other sound and no \
         sound file. When it runs is one \
         [trigger] table (never [[trigger]]; coming to the desk is room/presence, never the \
         faces): {triggers} A [trigger], its sooner or a [[while]] reads one of these \
         topics alone, and only the fields it carries: {topics}; the lamp sends no other. \
         A body step is one gesture, one of: {gestures}. Set none of \
         most_a_day, [quiet], [budget] or [rest] unless the request asks {limits} A skill can't yet \
         {hear}, know who someone is, {cant_send}: a \
         request that needs one, or anything else no step or trigger here or below does, is \
         answered with one line and no blocks: {cannot} then one short sentence said to the \
         person, beginning \"{cant_yet}\" and naming what in plain words (\"{cant_yet} \
         {cant_example}.\"), never a rule, ending with \
         what to ask instead when there is one; \
         for a reminder at a time or every so often: \"{REMINDER_INSTEAD}\"{check_steps} {found_out} Never \
         stand something else in for it, and add no time of day the request didn't name. When a part of the \
         request can't be done with these (dimming slowly, over a minute), make the rest and \
         add one line after the blocks: {left_out} what was left out, in a few words.\n\n\
         {services}{code}{steps}{said}"
    )
}

/// What the making model is told of a made skill's code of its own (#6307,
/// with [`made_skill::SCRIPTS_ENVIRONMENT`] on): when to write it, its
/// files' form and bounds (as [`made_skill::files`] holds them), the one
/// `task` step that runs it (#6305's `do.task`), its `grants`
/// ([`made_skill::GRANTS`]), how it reaches the lamp (the lamp's SDK alone,
/// #6071's design), and that the lamp says it back and waits for the
/// person's yes.
fn code_rules() -> String {
    let grants = made_skill::grants_told();
    format!(
        "A skill may also run code of its own, for what no step or trigger above does (a photo \
         booth, for one); never for what a step does. Then add up to {most} more fenced blocks, \
         each named ```scripts/NAME (letters, digits, -, _ and ., ending .py, .json, .txt or \
         .md), plain text, at most {each} KiB each and {all} KiB in all; and exactly one \
         [[do]] step, alone in its step: task = {{ task = \"what the code does, in a few plain \
         words\", run = \"scripts/main.py\" }}, run naming one of its .py files; and at the \
         top, before any table, grants = [ ... ] listing only what the code uses on the lamp, \
         of: {grants}. The code is Python 3 run on the lamp; it reaches the lamp only through \
         the lamp's own SDK, with only those grants, and has no other network: {sdk}. There \
         is no other lamp module, and each call takes only the arguments named. Its other \
         steps run at once; what the code finds comes back as the lamp's task news. The lamp \
         says back what it uses and waits for the person's yes before it runs any code.{notes}",
        sdk = made_skill::script_sdk(),
        notes = match (made_skill::notes_on(), made_skill::asks_on()) {
            (true, true) => format!("{NOTES_RULES}{NOTES_ASKS_RULES}"),
            (true, false) => NOTES_RULES.to_owned(),
            (false, _) => String::new(),
        },
        most = made_skill::MOST_SCRIPTS,
        each = made_skill::MOST_SCRIPT_BYTES / 1024,
        all = made_skill::MOST_SCRIPTS_BYTES / 1024,
        grants = grants.join(", "),
    )
}

/// What the making model knows now is no lookup (#7245, lamp-c0a4, 18 Nov:
/// "knowing facts about apples on my own" answered `cannot:`), with
/// [`made_skill::NOTES_ENVIRONMENT`] on.
const KNOWN_NOW: &str = "What you know now (facts, a poem, a joke, a list) is no lookup: write it \
                         into the say or the code yourself, as fixed text, never cannot for that. \
                         A count, a date or a price that changes is no fixed text.";

/// #8651 (the first builder's call after leg M: "ninety nine days left in
/// the year" written into a 6 AM say, wrong from the next day): what the
/// brief says of a line that changes, as `made_skill::told_only` refuses it.
const CHANGING_LINE: &str = "A say's words are fixed forever: a line that changes (a count, a \
     date, a price) is never a say in a skill a time, an event or a repeat starts, but a task \
     step whose script prints it, its answer told.";

/// The end of the cannot rule with notes on (#7245): a note or code is a
/// step too.
const CHECK_STEPS: &str =
    "A note or code of its own counts as a step: check them before you write cannot.";

/// What the making model is told of a made skill's notes (#7245), with
/// [`made_skill::NOTES_ENVIRONMENT`] on: a note is what the skill keeps and
/// its next run reads back, never a file the person opens; and a say never
/// claims one the code doesn't keep.
const NOTES_RULES: &str = " To keep something for later (\"write it down\", \"keep a list\", \
     \"save it to a file\"), its code keeps a note with lamp.notes.set, grant notes; its next \
     run reads it back with lamp.notes.get. A note is no file the person opens: a say calls it \
     a note the lamp keeps, and never says it wrote, saved or keeps a file or a note unless its \
     code calls lamp.notes.set. Its task's words and its say say what it keeps and when it \
     reads it back (\"I'll keep three facts about apples, and read them to you when you say \
     'apple file'\"), never a file the person can open. The SDK above has \
     lamp.notes.set and lamp.notes.get, so a skill that writes something down and reads it \
     back can be made. A request to write or save it to a file, even \"a file I can open\", is \
     made this way with a note, never answered cannot: the lamp has no screen, and its note is \
     how the person keeps it.";

/// With [`NOTES_RULES`] and [`made_skill::asks_on`] alone: a skill started
/// by the person's own words is no hearing of the room (lamp-c0a4, 18 Nov
/// 03:5x: OpenClaw took "when I say fruit notes" for it). Asks off, the
/// model is told nothing of asks, so nothing here names them.
const NOTES_ASKS_RULES: &str = " A skill that starts when the person says its words (\"when I \
     say fruit notes\") is started by its asks, never by hearing the room.";

/// The request [`AGENT_SERVICES_EXAMPLE`] is made for.
const AGENT_SERVICES_ASKED: &str = "every morning at eight, read me my new mail";

/// One whole service skill made as a task for the lamp's agent, shown the
/// model with [`agent_services`].
const AGENT_SERVICES_EXAMPLE: &str = "```skill.toml
kind = \"installable\"
version = \"1.0.0\"
grants = [\"gmail-read\"]

[permissions]
topics = [\"clock/local\"]

[trigger]
topic = \"clock/local\"
field = \"known\"
equals = true
rearm = \"ends\"
during = { from = \"08:00\", to = \"08:01\" }

[ask]
policy = \"nudge\"

[[do]]
task = { task = \"Read the owner's new mail since yesterday and tell them who wrote and what about\", run = \"scripts/main.py\" }
```

```SKILL.md
---
name: morning-mail
description: Every morning at eight, the lamp's agent reads me my new mail.
---

# Morning mail

Every morning at eight, the lamp's agent reads the new mail and tells me.
```

```scripts/main.py
print(\"The lamp's agent does this task with its lamp-services tools.\")
```";

/// What the making model is told of the person's linked services with
/// [`made_skill::AGENT_SERVICES_ENVIRONMENT`] on (the owner's ask, 12 Nov):
/// a skill that reads or writes one is one `do.task` for the lamp's agent,
/// which reaches the linked services through its `lamp-services` tools; a
/// send is read back by the lamp and goes only on the owner's spoken yes.
/// Never a `read = {...}` or `write = {...}` step: those stay for the
/// skills already made with them, which load and run as before.
fn agent_services() -> String {
    let cannot = made_skill::CANNOT;
    let most = made_skill::MOST_TASK_CHARS;
    format!(
        "A skill that reads or writes the person's mail, calendar, GitHub or any other linked \
         service hands that part to the lamp's agent, which reaches the linked services through \
         its lamp-services tools (services_connected, service_read, service_write). Such a skill \
         is made with code of its own as told at the end; its grants = [ ... ] exactly the \
         services it uses, each CODE-read to read it or CODE-write to send with it (a send \
         alone only CODE-write, CODE-read beside it only when it reads too), CODE one of: \
         {codes}; no other grant, and no services in [permissions]: only its trigger's \
         topics, and speak = true only beside a say step; its \
         trigger the lamp's own (a time, someone coming or leaving, as above){words}; and exactly one \
         [[do]] step, alone in its step: task = {{ task = \"...\", run = \"scripts/main.py\" }}, \
         the task's words, one plain sentence of at most {most} characters, read back whole for \
         the person's yes, saying plainly for the lamp's agent what \
         to read and what to tell the person, or what to send, in the request's own words, and \
         to whom (an address the request says, or the person's own for me); and one \
         ```scripts/main.py block that is exactly the example's one line, which holds no token. \
         Never a read = \
         {{ ... }} or write = {{ ... }} step. What the agent finds comes back as the lamp's task \
         news. Any send is read back by the lamp and goes only on the person's spoken yes. A \
         request to delete or change anything already in a service is answered with one line \
         and no blocks: {cannot} {cant_yet} deleting or changing what is in your services. \
         Never tell them to link a service. A whole such skill, for \"{AGENT_SERVICES_ASKED}\":\
         \n\n{AGENT_SERVICES_EXAMPLE}\n",
        cant_yet = made_skill::CANT_YET,
        codes = made_skill::SERVICE_GRANT_CODES.join(", "),
        words = if made_skill::asks_on() {
            ", or only the words they name for it (asks, as told at the end)"
        } else {
            ""
        },
    )
}

/// What the making model is told of the person's linked services (mail, a
/// calendar, Slack, ...): the rules a made skill is held to, in plain
/// words, for the model to judge any request by (the owner, 10 Nov:
/// "quăng cho deepseek hết đi, đừng if else"; no Rust word lists decide
/// it). Off, a skill uses none; on, it reads at most one, only reads it,
/// and only one of [`made_skill::read_steps`]. What it can't make is
/// answered with [`made_skill::CANNOT`], which the lamp tells as it is. It
/// never names a place to link one: on lamp-9adf, 4 Nov, the model told
/// the owner to link a calendar already linked (the first builder on
/// #5288). The cap (`made_skill::capped`) still holds every made skill to
/// these whatever the model answers.
fn services(
    reads: bool,
    writes: bool,
    own: &[(String, String)],
    today: Option<&(String, String)>,
) -> String {
    let cannot = made_skill::CANNOT;
    let ask_me = "You can ask me for it yourself any time.";
    // A write on its own (#6066): its rules with the switch, else the
    // lamp's "can't do that yet".
    let write = if writes {
        write_rules(own, today)
    } else {
        format!(
            "A skill never sends or writes anything to a service by itself: a request for one \
             that sends, mails, posts or adds anything on its own is answered with one line and \
             no blocks: {cannot} {} {}.",
            made_skill::CANT_YET,
            made_skill::SENDS_ON_ITS_OWN
        )
    };
    if !reads {
        let (what, needs) = if writes {
            ("read", "a read of one (a write below is no read)")
        } else {
            ("use", "one")
        };
        return format!(
            "A skill can't {what} the person's mail, calendar or any other linked service yet, \
             even one already linked: a request that needs {needs} is answered with one line and \
             no blocks: {cannot} Not possible yet: a skill can't {what} your mail, calendar or \
             other services, even ones already linked. {ask_me} Never tell them to link one.\n\
             {write}"
        );
    }
    let steps = made_skill::read_steps().join("\n- ");
    let unreadable = made_skill::unreadable_names().join(", ");
    // With writes on, a mail sent or an event added is a write, never a
    // change of what is there: read as one, every write is refused.
    // On lamp-9adf, 11 Nov, writes on, "email test@example.com ... when I
    // come back to my desk" was refused with this clause's ending: told
    // plainly a write is never it.
    let (only, asks_more, not_this) = if writes {
        (
            "A skill may read at most one of the person's linked services, and a read only \
             reads it: it never deletes or changes anything already in a service. Sending a \
             mail or adding a calendar event, an issue, a task or a comment is no change: it is \
             a write, made as the write rules at the end say",
            "A request to delete or change anything already in a service (a mail or an event \
             already there)",
            " Sending a mail or adding an event, an issue, a task or a comment is never answered \
             so: it is a write, made as below.",
        )
    } else {
        (
            "A skill may read at most one of the person's linked services, and only read it: it \
             never sends, writes, posts, books, schedules, deletes or changes anything in a \
             service",
            "A request to send, write or change anything in a service",
            "",
        )
    };
    format!(
        "{only}. A request to be read one of these is a skill with [permissions] only \
         speak = true, services = {{ CODE = \"read\" }} and its topics; its trigger's during \
         only for a time the request names; and exactly one [[do]] step, the one shown here \
         for that service, filled in from the request, and no light or head step. The services a skill can read, each as name (CODE): step:\n- {steps}\n\
         {asks_more}, to read two or more \
         services, or to read one not listed here ({unreadable}, or any other) is answered \
         with one line and no blocks: {cannot} then one short sentence saying what a skill \
         can do instead (read one linked service), ending \"{ask_me}\"{not_this} Never tell \
         them to link a service.\n{write}"
    )
}

/// What the making model is told of a made skill's write (#6066, with
/// [`made_skill::WRITES_ENVIRONMENT`] on): the kinds `lamp-services` sends,
/// their keys, and the first builder's conditions: the words only the
/// request's own, never a read's, the web's or memory's; the recipients
/// only an address the request says or the person's own (`own`, each
/// linked service's code and account); and the lamp says it back whole and
/// waits for the person's yes. A request it can't make so is `cannot:`.
fn write_rules(own: &[(String, String)], today: Option<&(String, String)>) -> String {
    let cannot = made_skill::CANNOT;
    let cant_yet = made_skill::CANT_YET;
    let kinds: Vec<String> = lamp_services::skill_writes::KINDS
        .iter()
        .map(|(code, action, keys)| format!("{code} {action}: {}", keys.join(", ")))
        .collect();
    let mine: Vec<String> = own
        .iter()
        .filter(|(_, account)| account.contains('@'))
        .map(|(code, account)| format!("{account} ({code})"))
        .collect();
    let me = own
        .iter()
        .find(|(code, account)| code.starts_with("gmail") && account.contains('@'))
        .map_or("you@example.com", |(_, account)| account.as_str());
    let mine = match mine.as_slice() {
        [] => "none linked".to_owned(),
        mine => mine.join(", "),
    };
    format!(
        "A skill may also send or write to one linked service by itself, after the person \
         says yes to it: a mail, a calendar event, an issue, a task or a comment. Such a skill \
         has [permissions] services = {{ CODE = \"write\" }} and its trigger's topics, no \
         speak, and exactly one [[do]] step: write = {{ service = \"CODE\", action = \
         \"ACTION\", its keys }}, one of:\n- {}\n\
         Every subject, text and title is the person's own words in the request, copied \
         exactly (a quoted part as it is): never write, add, translate or fill in words of your \
         own, and never take them from a read, the web or memory. Every to, cc and attendee is \
         an email address the request says, or the person's own: {mine}; me, myself or my \
         email is the person's own. A repo, team, project \
         or file is only one the request names. A request whose words to send, or whose \
         recipient's address, it doesn't say (\"mail Mai that I left\") is answered with one \
         line and no blocks: {cannot} {cant_yet} sending what you didn't say word for word, or \
         to an address you didn't say; say the exact words and the address. A whole write \
         skill, for \"{WRITE_EXAMPLE_ASKED}\":\n\n{}\n{}",
        kinds.join("\n- "),
        write_example(me),
        event_times(today)
    )
}

/// The request [`write_example`] is made for.
const WRITE_EXAMPLE_ASKED: &str =
    "when I come back to my desk, email me with the subject Back saying I am back";

/// One whole write skill, shown the model with [`write_rules`], `me` the
/// person's own linked mail address: on lamp-9adf, 11 Nov, with the rules
/// alone, every mail make came back `cannot:`.
fn write_example(me: &str) -> String {
    format!(
        "```skill.toml\nkind = \"installable\"\nversion = \"1.0.0\"\n\n[permissions]\n\
         topics = [\"room/presence\"]\nservices = {{ gmail = \"write\" }}\n\n[trigger]\n\
         topic = \"room/presence\"\nfield = \"someone\"\nequals = true\ncooldown = \"1h\"\n\
         rearm = \"ends\"\n\n[ask]\npolicy = \"nudge\"\n\n[[do]]\nwrite = {{ service = \
         \"gmail\", action = \"send_mail\", to = [\"{me}\"], subject = \"Back\", text = \
         \"I am back\" }}\n```"
    )
}

/// How a made calendar write's `start` and `end` are written (#6066):
/// fixed in the file, as `lamp-services` takes them (RFC 3339 with this
/// lamp's own UTC offset, both required), from today's local date and the
/// offset (`today`, [`crate::local_time::today_and_offset_here`]); with
/// neither known, an event is `cannot:`. On lamp-9adf, 11 Nov, the model,
/// told no date, offset or form, refused "a calendar event called Lamp
/// test on November twentieth at nine pm".
fn event_times(today: Option<&(String, String)>) -> String {
    let (cannot, cant_yet) = (made_skill::CANNOT, made_skill::CANT_YET);
    match today {
        Some((date, offset)) => format!(
            "A calendar event's start and end are each a date and a time with this lamp's UTC \
             offset, like 2026-11-20T21:00:00{offset}: today here is {date}, offset {offset}. \
             Its start is the day and time the request names (a day said from today, like \
             tonight or tomorrow, counted from {date}), and its end the one it names, else one \
             hour after its start. A request for an event that names no day or no time is \
             answered with one line and no blocks: {cannot} {cant_yet} adding an event with no \
             day and time; say the day and the time."
        ),
        None => format!(
            "This lamp's date isn't known now: a request for a calendar event is answered with \
             one line and no blocks: {cannot} {cant_yet} adding a calendar event while my clock \
             isn't set."
        ),
    }
}

/// The person's linked services' codes and accounts, for a write's own
/// address (#6066): asked of `lamp-services` once a make begins, only with
/// a made skill's writes on ([`own_accounts_now`]).
static OWN: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

fn own_accounts() -> Vec<(String, String)> {
    OWN.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// What is linked now, by code and account, asked of `lamp-services` with
/// a made skill's writes on and kept for the making model's instructions;
/// none with them off, or when it can't be told.
fn own_accounts_now() -> Vec<(String, String)> {
    let own = if made_skill::writes_on() {
        crate::services::linked_accounts(std::path::Path::new(crate::services::FOLDER))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    *OWN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = own.clone();
    own
}

/// The `make_skill` argument that asks for a new skill: what it should do
/// and when, in the person's words. Every other use of the function (a
/// remove, a run, a list, a yes, a clash's choice, a read-back) gives its
/// own argument alone, with none.
pub const REQUEST: &str = "request";

/// The `make_skill` argument for the name the person gave a new skill
/// as they asked for it (#7911): read from their words by the voice's
/// model, which judges what they said, and made the skill's name in code
/// on every make path ([`made_skill::given_name`]): DeepSeek's answer,
/// the lamp's agent's, and a task for the agent only asked for
/// (`make_no_cannot::made`).
pub const GIVEN_NAME: &str = "given_name";

/// What the make's model is told of its skill's name (#7911): the name the
/// request gives the skill, when it gives one. The voice's model gives it
/// as [`GIVEN_NAME`] too, and code makes it the name
/// ([`made_skill::given_name`]); told here as well for a call that left
/// it out but kept it in the request's words (#7926's row on lamp-9adf:
/// "make a skill called rain check" was kept as `morning-rain-check`).
pub const NAMED_TOLD: &str = "Its SKILL.md name is the name the request gives the skill when it \
     gives one, in lowercase words joined by -.";

/// Whether a `make_skill` call's whole `args` ask for a new skill: they
/// name its [`REQUEST`], as a streamed call whose first argument is
/// [`REQUEST`] does ([`new_make_argument`], `ChatEvent::Argued`). Neither
/// weighs its words or what else it carries, since the stream decides
/// before those come. A [`GIVEN_NAME`] alone, or beside a remove, is none.
pub fn asks_a_new_make(args: &Value) -> bool {
    args.get(REQUEST).is_some()
}

/// Whether a call (`name`, `args`) makes a skill: a new make of
/// `make_skill` ([`asks_a_new_make`]), or a change read as one
/// ([`crate::made_change::as_make`]).
pub fn makes_a_skill(name: &str, args: &Value) -> bool {
    (name == made_skill::MAKE && asks_a_new_make(args))
        || crate::made_change::as_make(name, args).is_some()
}

/// How a make's answer begins when nothing was made ([`told`]).
const NOT_MADE_HEAD: &str = "the skill couldn't be made: ";

/// Whether a make's answer `told` is the lamp's plain line for the
/// person's own words for its start breaking a rule no writer can change
/// ([`made_skill::their_ask`], #8451). The person spoke once: within the
/// same request, a later make is given this answer and makes nothing
/// (lamp-06c1 on #8469: round 2 made "pebble-yes" started by "go pebble",
/// words the person never said).
pub fn their_ask_told(told: &Value) -> bool {
    told["error"]
        .as_str()
        .and_then(|error| error.strip_prefix(NOT_MADE_HEAD))
        .is_some_and(made_skill::their_ask)
}

/// What a streamed `make_skill` call's first argument, `name`, decides:
/// a new make for its [`REQUEST`], none for any other, and nothing yet
/// (`None`) for its [`GIVEN_NAME`]. The schema's keys sort, so that one
/// streams first in any call that carries it, a remove's too (Heron's
/// HOLD on #7926): the whole calls decide then, by their `request`
/// ([`asks_a_new_make`]), so a named make still has its opening (Twelve's
/// HOLD on #7926).
pub fn new_make_argument(name: &str) -> Option<bool> {
    if name == GIVEN_NAME {
        return None;
    }
    Some(name == REQUEST)
}

/// The function the model is given with the switch: the lamp makes it
/// itself, at once, and puts it on as soon as it is kept.
pub fn tool() -> lamp_gemini::Tool {
    let writes = made_skill::writes_on();
    let mut description = "Make a new skill for the lamp: something the lamp then does by itself, \
             whatever the lamp can do (its light, its head, a few exact words it says, its hum, a read of a linked service), at a time each day, when someone comes or goes, a face comes \
             into view, someone gets up, pats its head, the air gets stuffy, or when it is resting. Make one only when asked. In the same breath, before the call, say in a \
             few words that you'll make it now. It is made, checked, kept and put on the \
             lamp at once: then say exactly the line you are told, nothing more."
        .to_owned();
    // A made skill read back (`LAMP_SKILL_WRITES`): what it sends, and
    // its wait for the person's agreement again.
    // A skill's task read back (`LAMP_SKILL_TASKS`, #6305): what its
    // script uses, and its wait for the person's agreement. Both on, one
    // sentence says both, so a remove fits within the link's 1,024 bytes.
    let tasks = crate::skill_task_yes::on();
    description.push_str(match (writes, tasks) {
        (true, true) => BOTH_READ_BACK_TOLD,
        (true, false) => READ_BACK_TOLD,
        (false, true) => TASKS_READ_BACK_TOLD,
        (false, false) => "",
    });
    // A made skill with code of its own (#6307): not on until the person
    // agrees to its say-back.
    let code = made_skill::scripts_on();
    if code {
        description.push_str(if crate::make_no_asks::on() {
            CODE_AT_ONCE_TOLD
        } else {
            CODE_TOLD
        });
    }
    // A made skill removed by voice (#6960), run now (lamp-9adf, 15 Nov),
    // and those made listed: the same function, its `remove`, `run` or
    // `made` alone; whether a line asks any, or a read-back, is the
    // model's judgment.
    // With `LAMP_MADE_SKILL_OFF_SAID` on (#7445), the same sentence tells
    // a turn-off, a pause or a stop from a remove, of one they made or
    // added (#7594), 154 bytes to 149, still within the link's 1,024 with
    // every switch on.
    description.push_str(if made_skill::off_said_on() {
        TURN_OFF_TOLD
    } else {
        REMOVE_TOLD
    });
    // A make or a remove at once, never asked first (`LAMP_MAKE_NO_ASKS`,
    // #7828's fix 1), the rest told on the remove's parameter.
    if crate::make_no_asks::on() {
        description.push_str(crate::make_no_asks::AT_ONCE_TOLD);
    }
    // A thing for the lamp to do at a time or on a repeat is a skill, never
    // a reminder (`LAMP_SKILL_RECURRING_TOLD`, lamp-9adf, 17 Nov): told on
    // the request, since with every switch on the description has no room.
    let mut parameters = parameters_with(writes, tasks, code);
    if crate::skill_recurring::on()
        && let Some(request) = parameters["properties"]["request"]["description"]
            .as_str()
            .map(str::to_owned)
    {
        parameters["properties"]["request"]["description"] =
            json!(format!("{request}{}", crate::skill_recurring::REQUEST_TOLD));
    }
    // A make while another is made waits its turn (`LAMP_MAKE_QUEUE`,
    // #7679): its label, and the id of a waiting one to drop. Off, the
    // parameters are as before.
    if crate::make_queue::on() {
        crate::make_queue::with_parameters(&mut parameters);
    }
    lamp_gemini::Tool {
        name: made_skill::MAKE.into(),
        description,
        parameters: Some(parameters),
        silent: false,
    }
}

/// What [`tool`]'s description adds with a made skill's code on (#6307):
/// plainly, that a made skill may carry a script of its own, not on until
/// the person agrees (#6749 B: on lamp-52e6, "make a skill that checks the
/// processor temperature and sets the light" was answered "I can't run a
/// script or check a temperature", and the function was never called).
/// Short: with every switch on the description stays within 1,024 bytes;
/// the example ask is the `request`'s ([`CODE_REQUEST_TOLD`]).
pub const CODE_TOLD: &str = " A made skill may carry its own Python script, run by its task once \
     the person agrees; never say the lamp can't run a script.";

/// [`CODE_TOLD`] with `LAMP_MAKE_NO_ASKS` on: nothing waits for the
/// person's agreement, so none is promised; shorter, so the description
/// stays within the link's 1,024 bytes.
pub const CODE_AT_ONCE_TOLD: &str = " A made skill may carry its own Python script, run by its \
     task; never say the lamp can't run a script.";

/// What the `request` parameter's description adds with a made skill's
/// code on (#6749 B): an ask its own script answers, passed whole.
const CODE_REQUEST_TOLD: &str = " A made skill's script may check what the lamp reads and act \
     on it (\"one that checks the processor temperature and sets the light\"): pass it whole.";

/// What [`tool`]'s description adds for a remove (#6960): on lamp-c0a4
/// "delete the skill called moon fact" went to `lamp_help` and "remove the
/// moon fact skill" to a read-back, since nothing here named a remove.
/// And for a run now (lamp-9adf, 15 Nov at 15:49 and 15:52: "Run my
/// unread mail skill now, please." went to a read-back, since nothing here
/// named a run, and nothing ran), and for the skills made, listed
/// (`crate::made_list`: "which skills have you made for me?" was answered
/// from the thread with no call), in the same sentence, so every switch on
/// still fits the link's 1,024 bytes.
pub const REMOVE_TOLD: &str = " To delete or remove a skill, call it with remove alone; to run \
     one they made now, run alone; to list those they made, made alone; say nothing first.";

/// What [`tool`]'s description says in [`REMOVE_TOLD`]'s place with
/// `LAMP_MADE_SKILL_OFF_SAID` on (#7445: on lamp-9adf "turn off the coffee
/// time skill" called the remove, the only verb `make_skill` had for a
/// made skill, and the lamp asked "Remove coffee time?"): turning one off,
/// pausing or stopping it is `turn_off`, never a remove, and back on
/// `turn_on`, beside the remove, the run and the list. One they made or
/// added (#7594: on lamp-9adf, 17 Nov, "turn off my btc price skill", one
/// added from a file in the app, was answered that it is done in the app,
/// since this told of one they made alone). The verbs are given to the
/// model, which judges the meaning; with every switch on the description
/// still fits the link's 1,024 bytes. A turn off is called at once (#7562's
/// rows on lamp-9adf, 18 Nov: "turn off my space facts skill" was asked
/// back "Do you want it off?" and "Remove space facts?"; the product
/// owner's 18:41 rule: it acts at once, in one line with its undo): "at
/// once" in the room of "back", 155 bytes, so every switch on fits at
/// 1,024; the rest is told on `turn_off`'s parameter.
pub const TURN_OFF_TOLD: &str = " To turn off, pause, stop one they made or added: turn_off at \
     once, never remove; on: turn_on; delete: remove; run now: run; list: made; say nothing first.";

/// What [`tool`]'s description adds with a made skill's writes on.
pub const READ_BACK_TOLD: &str = " Call it too, with read_back alone, when the person asks \
     what a skill they made sends or to hear one read back, saying nothing before it.";

/// What [`tool`]'s description adds with a skill's tasks on (#6305).
pub const TASKS_READ_BACK_TOLD: &str = " Call it with read_back alone too when the person \
     asks what a skill on the lamp uses, saying nothing before it.";

/// What [`tool`]'s description adds with a made skill's writes and a
/// skill's tasks both on: [`READ_BACK_TOLD`] and [`TASKS_READ_BACK_TOLD`]
/// in one sentence.
pub const BOTH_READ_BACK_TOLD: &str = " Call it with read_back alone to hear what a skill they \
     made sends or what a skill on the lamp uses, saying nothing first.";

/// The function's parameters: the request, and with a made skill's writes
/// (#6066) or code (#6307) on, the yes to one that waits for it,
/// `approve_id`, given alone, and with writes a read-back.
#[cfg(test)]
fn parameters(writes: bool, code: bool) -> Value {
    parameters_with(writes, false, code)
}

/// The most bytes `make_skill`'s parameters come to with every switch the
/// product's `lamp-voice` unit sets on (#8002): short of the link's 4,096
/// ([`lamp_gemini::MAX_TOOL_JSON_BYTES`]) by the room one more parameter
/// needs, about 336 bytes for #7765's `remake_old` and `put_back`. Each
/// description is said in as few words as keep its meaning.
pub const PRODUCT_MOST_PARAMETERS_BYTES: usize = 3_750;

/// [`parameters`], and with a skill's tasks on (#6305) the same
/// `approve_id` and `read_back` for a skill's task. A parameter given
/// "alone" is the call's one argument, with no request: said in that one
/// word (#8002), so with every switch the product's unit sets on these
/// stay within [`PRODUCT_MOST_PARAMETERS_BYTES`].
fn parameters_with(writes: bool, tasks: bool, code: bool) -> Value {
    let mut parameters = json!({
        "type": "object",
        "properties": {
            "request": {
                "type": "string",
                "description": "What this skill should do and when, in the person's own words for this ask alone: never a time, step or trigger of an earlier skill or turn. Never call it for an ask they take back in the same line."
            },
            GIVEN_NAME: {
                "type": "string",
                "description": "Its name if they give one, alone (\"evening rate\"); else left out."
            }
        },
        "required": []
    });
    // The person's choice on a made skill that clashes with one on the
    // lamp (#6593, [`crate::made_clash`]): its id and the choice, alone.
    parameters["properties"][crate::made_clash::CLASH_ID] = json!({
        "type": "string",
        "description": "Only when a skill you just made clashes with any on the lamp and they then choose: its clash_id, with choice alone."
    });
    // A made skill removed by voice (#6960, [`crate::made_remove`]): the
    // name as heard, matched in code to the person's own made skills.
    parameters["properties"][crate::made_remove::REMOVE] = json!({
        "type": "string",
        "description": "Only when the person asks to delete or remove a skill: its name as they said it, given alone with no request."
    });
    // A spoken change to a skill the person made ([`crate::made_change`],
    // #7975, #8008) is its own function, `change_skill`, run as this one's
    // call with `change` ([`crate::made_change::as_make`]): never a
    // parameter here, so these stay within
    // [`PRODUCT_MOST_PARAMETERS_BYTES`].
    // A skill the person made or added turned off and back on by voice
    // (#7445, #7594, [`crate::made_turn`], `LAMP_MADE_SKILL_OFF_SAID`):
    // never a remove, which is then told as for good alone. Nor is a
    // change (#7975, lamp-9adf, 19 Nov: "change rain check to seven
    // thirty" removed it): that is `change_skill`, whose words say it is
    // never a remove.
    if made_skill::off_said_on() {
        parameters["properties"][crate::made_remove::REMOVE]["description"] = json!(
            "Only when they ask to delete or remove a skill for good, never to turn one off, pause or stop it: its name as they said it, alone."
        );
        parameters["properties"][crate::made_turn::TURN_OFF] = json!({
            "type": "string",
            "description": "Only when they ask to turn off, pause or stop a skill they made or added (even from a file), kept to turn back on: its name as they said it, alone. Never remove for this, even one that starts only when they say its words. Call it at once. Ask nothing first."
        });
        parameters["properties"][crate::made_turn::TURN_ON] = json!({
            "type": "string",
            "description": "Only when they ask to turn back on or resume a skill they made or added (even from a file): its name as they said it, alone. To use or run one that is off, call run: it turns it on."
        });
    }
    // The skills the person made, listed by voice ([`crate::made_list`]):
    // how many, and the newest by name to the owner.
    parameters["properties"][crate::made_list::LIST] = json!({
        "type": "boolean",
        "description": "Only when they ask which or how many skills they made, or you made for them: true, alone. Never answer from the conversation."
    });
    // And whether a skill still being made is ready (#6811,
    // `LAMP_MAKE_MAY_RUN_ON`): the long make's state told with the list.
    if crate::long_make::on() {
        parameters["properties"][crate::made_list::LIST]["description"] = json!(
            "Only when the person asks which skills they made, or you made for them, or how many, or whether a skill they asked for is ready yet: true, given alone with no request. Never answer that from the conversation."
        );
    }
    // A made skill run now by voice ([`crate::made_run`]): the name as
    // heard, matched in code to the person's own made skills, as a remove's.
    parameters["properties"][crate::made_run::RUN] = json!({
        "type": "string",
        "description": "Only when they ask to run, start or do a skill they made, now: its name as they said it, alone. Never to hear what it sends or uses."
    });
    // What was just done to a skill they made, undone (`LAMP_MAKE_NO_ASKS`,
    // [`crate::make_no_asks::undo`]): the model judges the words.
    // With it, a remove is called at once and asked of only when two fit
    // (#7828's fix 1), and a removed skill asked back by its name is the
    // undo, never a new make (fix 2).
    if crate::make_no_asks::on() {
        parameters["properties"][crate::make_no_asks::UNDO] = json!({
            "type": "string",
            "description": format!(
                "Only when they ask to undo what you just did to a skill they made (\"remove it\" after you made one, \"keep the old one\" after one replaced theirs): \"last\", or its name as they said it, alone.{}",
                crate::make_no_asks::UNDO_RESTORE_TOLD
            )
        });
        let remove = format!(
            "{}{}",
            parameters["properties"][crate::made_remove::REMOVE]["description"]
                .as_str()
                .unwrap_or_default(),
            crate::make_no_asks::REMOVE_AT_ONCE_TOLD
        );
        parameters["properties"][crate::made_remove::REMOVE]["description"] = json!(remove);
    }
    parameters["properties"][crate::made_clash::CHOICE] = json!({
        "type": "string",
        "enum": crate::made_clash::CHOICES,
        "description": "With clash_id only: replace the other skill(s), keep all (they may take turns) or cancel, as they chose."
    });
    // The undo of a skill turned on in place of a make (#7747).
    crate::make_core_offer::with_parameters(&mut parameters);
    if code {
        let request = format!(
            "{}{CODE_REQUEST_TOLD}",
            parameters["properties"]["request"]["description"]
                .as_str()
                .unwrap_or_default()
        );
        parameters["properties"]["request"]["description"] = json!(request);
    }
    if writes || tasks || code {
        parameters["properties"][crate::skill_write_yes::APPROVE_ID] = json!({
            "type": "string",
            "description": "Only when a skill you just made or read back waits for their yes and they agree in any words, or a note asks their answer by it: its approve_id, alone."
        });
        // What the person answered (#6835 gap 2): a call made in code's
        // turn may be a no or unclear, never only a yes.
        parameters["properties"][crate::skill_write_yes::ANSWER] = json!({
            "type": "string",
            "enum": crate::skill_write_yes::ANSWERS,
            "description": "With approve_id only: their answer to the say-back: yes if they agree, no if they decline or wait, unclear if you can't tell, new_ask if they ask something new instead (another skill or thing), which you then answer; one naming the skill asked about is never new_ask. Left out: yes for a made skill's writes or code, unclear for a skill's task."
        });
        // The skill the person's line names (#7545): a yes is taken only
        // for the skill waiting, matched in code.
        parameters["properties"][crate::skill_write_yes::NAMED] = json!({
            "type": "string",
            "description": "With approve_id only: the skill their line names, or none. If it is the skill asked about, copy its name exactly as the note gives it. A yes counts only for that skill."
        });
        parameters["required"] = json!([]);
    }
    if writes || tasks {
        let asked = match (writes, tasks) {
            (true, false) => "what a skill they made sends, or to hear it read back",
            (false, _) => "what a skill on the lamp uses",
            (true, true) => {
                "what a skill they made sends or a skill on the lamp uses, or to hear one read back"
            }
        };
        parameters["properties"][READ_BACK] = json!({
            "type": "string",
            "description": format!("Only when they ask {asked}: its name, alone.")
        });
        parameters["required"] = json!([]);
    }
    parameters
}

/// Whether a failed answer is asked once more: the service not found or
/// failing for now (the relay's 404s, about one in ten on 2 Nov), a step
/// past its bound, or a connection that broke. Never a refusal of the
/// request itself (4xx but 404), nor an answer this client can't read.
pub fn again(error: &net::Error) -> bool {
    match error {
        net::Error::Status(status) => *status == 404 || (500..600).contains(status),
        net::Error::Timeout(_) | net::Error::Transport(_) => true,
        // The relay's stream cut before its end (lamp-9adf, 2 Nov 17:49):
        // a broken connection by another name. An answer the service
        // stopped at its cap on tokens, or one with no fenced block at
        // all, is not whole either.
        net::Error::Protocol(what) => [CUT, AT_LENGTH, NO_BLOCK].contains(what),
        // A usage limit doesn't pass in a moment (#7346).
        net::Error::Url | net::Error::UsageLimit(_) | net::Error::KeyInvalid => false,
    }
}

/// The model's answer to `brief` under the make's `system` prompt, its
/// text whole, asked once more when [`again`] says so; each answer that
/// ended whole, the second too, with a last fence left open at its end
/// closed there ([`whole`], `net` as the prompt was made with it).
async fn written(chat: &Chat, system: &str, net: bool, brief: &str) -> net::Result<String> {
    match write_once_with(chat, system, brief)
        .await
        .and_then(|text| whole(text, net))
    {
        Err(error) if again(&error) => {
            eprintln!(
                "lamp-voice: a skill made here: the model's answer failed ({error}); asked once more"
            );
            // The second answer is not asked again: one with no fenced
            // block goes on to be refused as not a skill's form.
            match write_once_with(chat, system, brief).await {
                Ok(text) => Ok(fence_closed(text)),
                Err(error) => Err(error),
            }
        }
        answer => answer,
    }
}

/// An answer whose stream ended whole, as the maker reads it: a `cannot:`
/// as it is (whole with no fence at all); one with no fenced block at all
/// asked once more ([`NO_BLOCK`]); and otherwise its last fence, left open
/// at the end of the answer, closed there ([`fence_closed`]). A product
/// lamp on 11798, 11 Nov: two answers that ended whole, about 250 tokens
/// each, were taken for cut streams because the last fence was never
/// written, and the make failed as cut off. A stream that really broke
/// never comes here: the chat client gives it as [`CUT`]. With `net`
/// ([`crate::make_no_cannot::net_by_agent`], the prompt's), the model's
/// `internet:` line is whole as it is too.
fn whole(text: String, net: bool) -> net::Result<String> {
    if text.trim_start().starts_with(made_skill::CANNOT) {
        return Ok(text);
    }
    // What only the internet tells, with `LAMP_TASK_NET` on: one line, no
    // blocks, made one task for the lamp's agent ([`net_made`]).
    if net && crate::make_no_cannot::says_needs_net(&text) {
        return Ok(text);
    }
    if !text.contains("```") && !text.contains("~~~") {
        return Err(net::Error::Protocol(NO_BLOCK));
    }
    Ok(fence_closed(text))
}

/// The answer with the fence it left open at its end closed there
/// ([`made_skill::open_fence`]), and one journal line when it was.
fn fence_closed(text: String) -> String {
    match made_skill::open_fence(&text) {
        Some(fence) => {
            eprintln!("lamp-voice: a made skill's last fence closed at the end of its answer");
            format!("{}\n{fence}\n", text.trim_end())
        }
        None => text,
    }
}

/// The chat client's word for a stream that ended before its end.
const CUT: &str = "chat stream cut";
/// An answer the service stopped at its cap on tokens (`finish_reason`
/// `length`): the stream whole, the answer not.
const AT_LENGTH: &str = "chat answer stopped at its length";
/// An answer that ended whole with no fenced block in it.
const NO_BLOCK: &str = "chat answer with no fenced block";

/// The model's answer to `brief` under `system` (`LAMP_MAKE_NO_CANNOT`'s
/// one more call, [`crate::make_no_cannot::system`]): asked once more when
/// [`again`] says so, a last fence left open closed, and no fenced block
/// needed (its one-line answers have none).
async fn written_with(chat: &Chat, system: &str, brief: &str) -> net::Result<String> {
    match write_once_with(chat, system, brief).await {
        Err(error) if again(&error) => write_once_with(chat, system, brief).await.map(fence_closed),
        answer => answer.map(fence_closed),
    }
}

async fn write_once_with(chat: &Chat, system: &str, brief: &str) -> net::Result<String> {
    tokio::time::timeout(WRITE_WITHIN, async {
        let mut stream = chat
            .send(&[Message::system(system), Message::user(brief)])
            .await?;
        let mut text = String::new();
        while let Some(event) = stream.next().await? {
            if let ChatEvent::Text(words) = event {
                text.push_str(&words);
                if text.len() > most_answer_bytes() {
                    return Err(net::Error::Protocol("answer length"));
                }
            }
        }
        if stream.stopped_at_length() {
            return Err(net::Error::Protocol(AT_LENGTH));
        }
        Ok(text)
    })
    .await
    .map_err(|_| net::Error::Timeout("answer"))?
}

/// The second worker's own configuration, as it read it at start (its
/// file, the unit's endpoints, the person's own keys): what [`own_chat`]
/// asks. Set once.
static CONFIG: std::sync::OnceLock<crate::second_config::SecondConfig> = std::sync::OnceLock::new();

/// Kept by the second worker as it starts, with the switch.
pub fn set_config(config: crate::second_config::SecondConfig) {
    let _ = CONFIG.set(config);
}

/// The lamp's own chat (`llm_*`), as the second worker configured it, with
/// no tools. None outside that worker: its environment names no file.
pub fn own_chat() -> Result<Chat, String> {
    let config = match CONFIG.get() {
        Some(config) => config.clone(),
        None => crate::second_config::load_from_env()
            .map_err(|error| format!("the second route's file: {error}"))?
            .ok_or_else(|| "no second route's configuration".to_owned())?,
    };
    Chat::new(&config, Arc::new(Vec::new)).map_err(|error| error.to_string())
}

/// `make_skill`: the skill for `request` written by the
/// model `chat` gives, kept in `keeping` as made at `now_unix_ms`, and put
/// on at once through `keeping`'s manage helper ([`made_skill::put_on`]),
/// with no spoken yes (the owner, 10 Nov). Blocking: the model is asked on
/// a thread of its own, with a runtime of its own. What the model is told.
///
/// A kept skill that reads a linked service not linked now, as
/// `lamp-services` tells it ([`crate::services::linked_codes`], asked only
/// then), is told with `needs_link` and a sentence to say where to link it
/// ([`made_skill::needs_link`]); none when that can't be told, and the
/// make stands either way.
pub fn make(
    request: &str,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    now_unix_ms: u64,
) -> Value {
    make_in(0, request, keeping, chat, now_unix_ms)
}

/// [`make`] in the voice's request `made_in`: a write's yes comes in a
/// later one (`crate::skill_write_yes`).
pub fn make_in(
    made_in: u64,
    request: &str,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    now_unix_ms: u64,
) -> Value {
    // The line heard for the make, as `forget` takes it (`tools`): a
    // write is held to it. None heard is no words, so a write fails
    // closed; `made_in` 0 is no request of the voice's. A make that waited
    // its turn (`LAMP_MAKE_QUEUE`, #7679) brings its own, kept at the ask
    // ([`with_heard_kept`]).
    let heard = (made_in != 0).then(|| {
        crate::thread::THREAD
            .lock()
            .ok()
            .and_then(|thread| heard_for_make(made_in, &thread))
            .or_else(kept_heard)
            .unwrap_or_default()
    });
    // The person's own line alone, for the judge of a made skill with no
    // word of the ask (#7621): never the newest words heard, another
    // utterance's, since a false refusal is worse than no check (#6537).
    let _own = (made_in != 0 && made_skill::unrelated_on()).then(|| {
        OwnLine::set(
            crate::thread::THREAD
                .lock()
                .ok()
                .and_then(|thread| own_line_for_make(made_in, &thread))
                .or_else(kept_heard),
        )
    });
    make_with_by(
        made_in,
        heard.as_deref(),
        request,
        keeping,
        chat,
        now_unix_ms,
        || crate::services::linked_codes(std::path::Path::new(crate::services::FOLDER)),
        crate::make_skill_agent::Agent::for_voice().as_ref(),
    )
}

std::thread_local! {
    /// The line heard for a make that waited its turn, kept when it was
    /// asked ([`with_asker_kept`]); none for any other make.
    static KEPT_HEARD: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// The owner read at the ask of a make that waited its turn: `Some`
    /// while one runs ([`with_asker_kept`]), holding none for an ask not
    /// the owner's; `None` for any other make, whose lease is read.
    static KEPT_OWNER: std::cell::RefCell<Option<Option<String>>> = const { std::cell::RefCell::new(None) };
}

/// `f` run with `heard`, the line heard for a make that waited its turn
/// (`LAMP_MAKE_QUEUE`, #7679), as [`make_in`]'s heard line and own line
/// in place of the thread's: kept at the ask, it can't be pushed out of the
/// thread's history while the make waits. Cleared after, a panic too.
pub fn with_heard_kept<R>(heard: Option<String>, f: impl FnOnce() -> R) -> R {
    struct Cleared;
    impl Drop for Cleared {
        fn drop(&mut self) {
            KEPT_HEARD.with(|kept| kept.replace(None));
        }
    }
    KEPT_HEARD.with(|kept| kept.replace(heard));
    let _cleared = Cleared;
    f()
}

/// [`with_heard_kept`], and `owner`, the lamp's owner whose line asked for
/// it as read at the ask (none for anyone else's): what [`answer`] holds
/// the make to in place of its lease's, gone by the time a waiting make
/// runs (`LAMP_MAKE_NO_ASKS`: a make with code put on, or a clash replacing
/// the owner's skill, only on the owner's ask). Cleared after, a panic too.
pub fn with_asker_kept<R>(
    heard: Option<String>,
    owner: Option<String>,
    f: impl FnOnce() -> R,
) -> R {
    struct Cleared;
    impl Drop for Cleared {
        fn drop(&mut self) {
            KEPT_OWNER.with(|kept| kept.replace(None));
        }
    }
    KEPT_OWNER.with(|kept| kept.replace(Some(owner)));
    let _cleared = Cleared;
    with_heard_kept(heard, f)
}

/// The skill made, kept as made in this conversation (#7660): the owner's
/// by the owner read at the ask of a make that waited its turn, its lease
/// gone (#7679), else by its lease's.
fn keep_just_made(told: &Value, request: u64, lease: Option<&crate::memory_scope::ScopeLease>) {
    crate::just_made::after_make_by(told, request, || owner_for_make(lease));
}

/// The owner a make's ask is held to: the one kept at the ask of a make
/// that waited its turn ([`with_asker_kept`]), else its `lease`'s.
fn owner_for_make(lease: Option<&crate::memory_scope::ScopeLease>) -> Option<String> {
    KEPT_OWNER
        .with(|kept| kept.borrow().clone())
        .unwrap_or_else(|| crate::services::owner_of(lease))
}

/// Tests: the owner and the line a make on this thread is held to now.
#[cfg(test)]
pub(crate) fn kept_for_test() -> (Option<String>, Option<String>) {
    (owner_for_make(None), kept_heard())
}

/// The line kept for a make that waited its turn, if this is one.
fn kept_heard() -> Option<String> {
    KEPT_HEARD.with(|kept| kept.borrow().clone())
}

/// The line `made_in` heard, as [`make_in`] reads it: the kept line of a
/// make that waited its turn ([`with_heard_kept`]), else the thread's.
fn heard_for_make(made_in: u64, thread: &crate::thread::Thread) -> Option<String> {
    kept_heard().or_else(|| thread.heard_for(made_in, std::time::Instant::now()))
}

/// The person's own line for `made_in`, as [`make_in`] reads it: the kept
/// line of a make that waited its turn, else the thread's own line.
fn own_line_for_make(made_in: u64, thread: &crate::thread::Thread) -> Option<String> {
    kept_heard().or_else(|| thread.heard_in_own_line(made_in))
}

/// [`make_in`], with the line `heard` for it (`None` for no request of the
/// voice's) and what is linked given by `linked`.
pub fn make_with(
    made_in: u64,
    heard: Option<&str>,
    request: &str,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    now_unix_ms: u64,
    linked: impl FnOnce() -> Option<Vec<String>>,
) -> Value {
    make_with_by(
        made_in,
        heard,
        request,
        keeping,
        chat,
        now_unix_ms,
        linked,
        None,
    )
}

/// [`make_with`], with the lamp's agent `agent` to hand a make DeepSeek
/// didn't make to (`LAMP_MAKE_SKILL_AGENT`, [`crate::make_skill_agent`]).
#[allow(clippy::too_many_arguments)]
fn make_with_by(
    made_in: u64,
    heard: Option<&str>,
    request: &str,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    now_unix_ms: u64,
    linked: impl FnOnce() -> Option<Vec<String>>,
    agent: Option<&crate::make_skill_agent::Agent>,
) -> Value {
    // A long make running on (`LAMP_MAKE_MAY_RUN_ON`, #6811): one at a
    // time, so a second ask is told it is still making, nothing made.
    let run_on = made_in != 0 && crate::long_make::on();
    if run_on && let Some(told) = crate::long_make::still_making(&keeping.directory, now_unix_ms) {
        return told;
    }
    // One ask makes one skill: a call for an ask made or being made a
    // moment ago, carried into the next line after its turn was cut, makes
    // nothing again ([`OneAsk`]). By voice only: `made_in` 0 is no
    // request of the voice's.
    let asked = match made_in {
        0 => None,
        _ => match OneAsk::begin_in(&keeping.directory, heard, request, now_unix_ms, made_in) {
            Ok(asked) => Some(asked),
            Err(told) => return told,
        },
    };
    let brief = match made_skill::brief(request) {
        Ok(brief) => brief,
        Err(why) => {
            crate::made_counts::refused(&keeping.directory, made_skill::Maker::Voice);
            return json!({
                "error": why,
                "next": "Tell them that in a few words."
            });
        }
    };
    // It may be let run on by its turn from here until it settles.
    if run_on {
        crate::long_make::begin(made_in, &keeping.directory);
    }
    let made = make_kept_heard(
        request,
        heard,
        brief,
        keeping,
        chat,
        &format!("{FROM}{now_unix_ms}"),
        agent,
    );
    // Declined while it was made ([`making_answered`]): never put on,
    // nothing of it said, and nothing of it kept (its package and off
    // entry go, as a cancelled clash's do: `made_skill::forget_unput`);
    // asked again, it is made again. Not declined, it is marked finishing
    // in the same lock, so a no from here on is never told it was dropped
    // while it goes on ([`FINISHING_NEXT`]).
    if asked.as_ref().is_some_and(OneAsk::dropped_or_finishing) {
        if let Ok(made) = &made {
            made_skill::forget_unput(&keeping.directory, &made.sha256);
        }
        MADE_LINE.with(|kept| kept.replace(None));
        // Let run on first (Heron's HOLD on #7774): it settles here, and
        // its `making` mark goes untold, so no later make hears "still
        // making" and nothing is told of it when its time would run out.
        if run_on && let Some(directory) = crate::long_make::settle(made_in) {
            crate::long_make::dropped(&directory, crate::app_make::now_unix_ms().max(now_unix_ms));
        }
        eprintln!(
            "lamp-voice: a skill made here was declined while it was made: not put on, not kept, nothing said"
        );
        return json!({ "dropped": true, "next": WONT_MAKE_NEXT });
    }
    // A make given the long make's time whose turn was cut before its
    // minute goes the same way (El's P2 on #7774): never on, nor said
    // through the say service, with no gate.
    if run_on && turn_cut() {
        crate::long_make::cut(made_in, crate::app_make::now_unix_ms().max(now_unix_ms));
    }
    // Let run on past a minute (#6811): kept off until its person hears it
    // is ready, or nothing kept and that told; never put on here, and no
    // line for this turn, which has ended.
    if run_on && let Some(directory) = crate::long_make::settle(made_in) {
        let ended_ms = crate::app_make::now_unix_ms().max(now_unix_ms);
        match &made {
            Ok(made) => crate::long_make::settled(
                &directory,
                Ok((made.name.as_str(), made.sha256.as_str())),
                ended_ms,
            ),
            Err(Refused(why, _)) => crate::long_make::settled(
                &directory,
                Err(why == made_skill::AGENT_LATE_TRY),
                ended_ms,
            ),
        }
        return crate::long_make::let_go_told();
    }
    // An ask by voice that made nothing, for the opt-in health report's
    // counts: one atomic add here, written later (`crate::made_counts`).
    if made.is_err() {
        crate::made_counts::refused(&keeping.directory, made_skill::Maker::Voice);
    }
    // `LAMP_MAKE_NO_ASKS`: an unsure line is not acted on, and nothing is
    // asked: no "could you say it again?".
    if crate::make_no_asks::on()
        && let Err(Refused(why, _)) = &made
        && why == made_skill::UNRELATED
    {
        eprintln!(
            "lamp-voice: request {made_in}: a skill wasn't made: not what was asked; nothing asked (LAMP_MAKE_NO_ASKS)"
        );
        return json!({
            "error": crate::make_no_asks::NOT_ACTED,
            "next": crate::make_no_asks::NOT_ACTED_NEXT
        });
    }
    // A made skill the judge said isn't what was asked (#7621): the person
    // is asked again in the lamp's own words alone, never told its name.
    if let Err(Refused(why, _)) = &made
        && why == made_skill::UNRELATED
    {
        // Nothing made: after a cut it is said only while the person has
        // not gone on with a line of their own (`worker::made_running`).
        MADE_LINE
            .with(|kept| kept.replace(Some((made_skill::UNRELATED.to_owned(), Outcome::Failed))));
        return json!({
            "error": made_skill::UNRELATED,
            "next": format!(
                "Say exactly this and nothing else: {}",
                made_skill::UNRELATED
            )
        });
    }
    let needs = made
        .as_ref()
        .ok()
        .and_then(|made| made_skill::needs_link(&made.uses, linked));
    // `LAMP_MAKE_NO_ASKS` (the first builder, 18:41): on at once, code too,
    // a clash replaced, and one line with its undo; nothing asked.
    if let Ok(kept) = &made
        && let Some(told) = made_no_asks(made_in, kept, keeping, needs.as_deref())
    {
        if let (Some(asked), true) = (asked, told["on"] == true) {
            asked.made(&kept.name, &told, now_unix_ms);
        }
        return told;
    }
    // A skill made by voice that clashes with one on the lamp (#6593: the
    // same trigger, both using the light or both moving the head) is not
    // put on: the person is asked, of every skill it clashes with, to
    // replace them, keep them or cancel ([`crate::made_clash`]). One with
    // code of its own waits for its yes as it did.
    if made_in != 0
        && let Ok(made) = &made
        && !made.uses.code
        && !made.uses.agent_task
        && let clashes = crate::made_clash::found(keeping, made)
        && !clashes.is_empty()
    {
        let told =
            crate::made_clash::asked(keeping, made, &clashes, made_in, std::time::Instant::now());
        // Said after a cut: an older clash it let go is told first.
        let cut = crate::made_clash::cut_line(made, &clashes);
        let line = match told["dropped"].as_str() {
            Some(dropped) => format!("{} {cut}", crate::made_clash::dropped_line(dropped)),
            None => cut,
        };
        MADE_LINE.with(|kept| kept.replace(Some((line, Outcome::Clash))));
        if let Some(asked) = asked {
            asked.made(&made.name, &told, now_unix_ms);
        }
        return told;
    }
    // One with code of its own (#6307) is not put on here: it waits for
    // the person's yes to its say-back, which puts on that package alone.
    // With `LAMP_MADE_CODE_ON_AT_ONCE`, it goes on now by that very path,
    // with no yes, and is said as on or why not.
    let at_once = match &made {
        Ok(made) if made_skill::code_goes_on_at_once(made) => Some(code_on_at_once(keeping, made)),
        _ => None,
    };
    let on = match (&made, &at_once) {
        (_, Some(at_once)) => at_once.clone().map(|_| ()),
        (Ok(made), None) if made.uses.code => Ok(()),
        (Ok(made), None) => made_skill::put_on(keeping, made),
        (Err(_), None) => Ok(()),
    };
    let told = match (&made, &at_once) {
        (Ok(kept), Some(at_once)) => {
            let line = code_on_line(kept, at_once);
            let outcome = Outcome::of(at_once.is_ok());
            MADE_LINE.with(|kept| kept.replace(Some((line.clone(), outcome))));
            told_with(line, made.clone(), on.clone(), needs.as_deref())
        }
        _ => {
            let outcome = Outcome::of(made.is_ok() && on.is_ok());
            MADE_LINE.with(|kept| {
                kept.replace(Some((said_line(&made, &on, needs.as_deref()), outcome)))
            });
            told(made.clone(), on.clone(), needs.as_deref())
        }
    };
    // A skill that sends or writes, once on (its yes binds the installed
    // package): its writes wait for the person's yes, in a later heard
    // line (#6066, `crate::skill_write_yes`).
    let told = match (&made, &on) {
        (Ok(_), Ok(())) if at_once.is_some() => {
            let mut told = told;
            told["task"] = json!(matches!(at_once, Some(Ok(true))));
            told
        }
        (Ok(made), Ok(())) if made.uses.code => {
            let id = crate::skill_write_yes::wait_for_yes(
                code_asks(made),
                made_in,
                std::time::Instant::now(),
            );
            let mut told = waits_for_yes(told, &made_skill::on_line(made), &id);
            told["on"] = json!(false);
            told["code_waits_for_yes"] = json!(true);
            if let Some(told) = told.as_object_mut() {
                told.remove("writes_wait_for_yes");
            }
            told
        }
        (Ok(made), Ok(())) => match asks_of(&keeping.directory, made) {
            Some(asks) => {
                let id =
                    crate::skill_write_yes::wait_for_yes(asks, made_in, std::time::Instant::now());
                waits_for_yes(told, &made_skill::on_line(made), &id)
            }
            None => told,
        },
        _ => told,
    };
    // Made and on (or waiting for its yes): held for the same ask asked
    // again; anything else is let go, so asking again tries again.
    if let (Some(asked), Ok(made), Ok(())) = (asked, &made, &on) {
        let yes = (told["code_waits_for_yes"] == true)
            .then(|| (code_asks(made), made_skill::on_line(made)));
        asked.made_with(&made.name, &told, now_unix_ms, yes);
    }
    told
}

/// A made skill `made` with `LAMP_MAKE_NO_ASKS` ([`crate::make_no_asks`]):
/// one that clashes, made by voice with no code of its own, replaces the
/// others on, on the owner's ask, or goes on beside them
/// ([`crate::make_no_asks::clashed`]); any other goes on at once
/// ([`crate::make_no_asks::put_on`]), code only on the owner's ask
/// ([`owners_make`]). Its line kept for a cut turn. None with the switch
/// off, or for a skill that sends, which keeps its yes.
fn made_no_asks(
    made_in: u64,
    made: &Made,
    keeping: &Keeping,
    needs: Option<&[Value]>,
) -> Option<Value> {
    if !crate::make_no_asks::on() || !crate::make_no_asks::keeps_no_yes(made) {
        return None;
    }
    // A change replaces only the skill it changes, in place, never another
    // (`crate::made_change`, Twelve's P2 2 on #8008: its near twins were
    // turned off with no word): its words that would start another are
    // refused with why, as anyone else's make's are.
    let replaces = made_in != 0
        && !made.uses.code
        && !made.uses.agent_task
        && made_skill::change_as().is_none();
    // Its words would start skills the person made, kept for this
    // (`made_skill::asks_taken_sparing`, #7828's fix 3): replaced below on
    // the owner's ask, as a trigger clash is; anyone else's make, or one
    // that can't replace, is refused with why, as before.
    let words = match made_skill::own_asks_taken(keeping, made) {
        Ok(words) => words,
        Err(why) => return Some(words_refused(keeping, made, &why)),
    };
    let owner = (!words.is_empty()).then(|| replaces && owners_make(made_in));
    if let (Some(false), Some((_, why))) = (owner, words.first()) {
        return Some(words_refused(keeping, made, why));
    }
    if replaces {
        let off = crate::make_no_asks::turned_off_here();
        // Those its words would start are turned off before it goes on;
        // any not turned off (the `off` file unreadable, the helper's
        // refusal) refuses it as before, never on beside them (Heron's P3 2
        // on #7836).
        let first = match crate::make_no_asks::words_turned_off(keeping, &words, off.as_ref()) {
            Ok(first) => first,
            Err(why) => return Some(words_refused(keeping, made, &why)),
        };
        // Only a skill doing the same thing is replaced (#7938: on
        // lamp-9adf a daily brief at 08:00 replaced a good morning, a good
        // morning 3 and a water reminder, each on the lamp's clock at its
        // own time and lighting); one that only clashes goes on beside it.
        let others: Vec<String> = crate::made_clash::same_found(keeping, made)
            .into_iter()
            .map(|clash| clash.name)
            .filter(|name| !words.iter().any(|(skill, _)| skill == name))
            .collect();
        if !others.is_empty() || !first.is_empty() {
            // Only skills on are replaced, and only on the owner's ask
            // (Twelve's P1 on #7783); anyone else's goes on beside them.
            let (told, line) = crate::make_no_asks::clashed(
                keeping,
                made,
                (&others, &first),
                off.as_ref(),
                || owner.unwrap_or_else(|| owners_make(made_in)),
            );
            // No clash waits here: put on, or not, as it is told.
            let outcome = Outcome::of(told["on"].as_bool() == Some(true));
            MADE_LINE.with(|kept| kept.replace(Some((line, outcome))));
            return Some(told);
        }
    }
    let (on, line) = crate::make_no_asks::put_on(keeping, made, || owners_make(made_in))?;
    let told = crate::make_no_asks::made_told(made, &on, &line, needs);
    let outcome = Outcome::of(on.is_ok());
    MADE_LINE.with(|kept| kept.replace(Some((line, outcome))));
    Some(told)
}

/// The made skill `made`, kept and not on, whose words would start skills
/// the person made, not replaced (anyone else's make, or one that can't
/// replace): refused as at the make, nothing of it kept, the reason `why`
/// said as it is ([`made_skill::not_made_line`]). What the model is told,
/// its line kept for a cut turn.
fn words_refused(keeping: &Keeping, made: &Made, why: &str) -> Value {
    made_skill::forget_unput(&keeping.directory, &made.sha256);
    crate::made_counts::refused(&keeping.directory, made_skill::Maker::Voice);
    eprintln!(
        "lamp-voice: a made skill was not made: its words start a skill the person made, not replaced (LAMP_MAKE_NO_ASKS)"
    );
    let refused: Result<Made, Refused> = Err(Refused::new(why));
    let line = said_line(&refused, &Ok(()), None);
    MADE_LINE.with(|kept| kept.replace(Some((line, Outcome::Failed))));
    told(refused, Ok(()), None)
}

/// Whether the line that asked for this thread's make is the owner's,
/// asked only for one with code of its own (`LAMP_MAKE_NO_ASKS`): set by
/// [`answer`] for its make, taken once.
type OwnerAsk = Box<dyn FnOnce() -> bool>;

thread_local! {
    static MAKE_OWNER: std::cell::RefCell<Option<OwnerAsk>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// [`owners_make`] as a test of this thread sets it.
    static OWNER_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// [`owners_make`] for this test's thread alone.
#[cfg(test)]
pub(crate) fn set_owner_here(owner: Option<bool>) {
    OWNER_HERE.with(|here| here.set(owner));
}

/// Whether the make in the voice's request `made_in` was the owner's ask,
/// as a yes to it was held (`services::owners_yes_of`): no request of the
/// voice's (`made_in` 0) is the app's, its owner's; a voice make whose
/// owner [`answer`] didn't say is no one's.
fn owners_make(made_in: u64) -> bool {
    #[cfg(test)]
    if let Some(owner) = OWNER_HERE.with(std::cell::Cell::get) {
        return owner;
    }
    made_in == 0
        || MAKE_OWNER
            .with(|owner| owner.borrow_mut().take())
            .is_some_and(|ask| ask())
}

/// How long a voice make's ask is held once its skill is made and on
/// ([`OneAsk`]): longer than a cut turn's carried line takes to be heard
/// and answered again, shorter than a person deciding to ask once more.
pub const ONE_ASK_WITHIN: Duration = Duration::from_secs(180);

/// The fewest words a line holds for another line holding it whole to be
/// the same ask ([`same_ask`]): "make me a skill" alone is no ask of its
/// own.
const ONE_ASK_FEWEST_WORDS: usize = 5;

/// A voice make's ask, held in [`ASKS`]: its keeping folder, the words
/// heard and the model's request (each [`ask_words`]), and, once made and
/// on, its skill's name, what its turn was told and when.
struct HeldAsk {
    id: u64,
    directory: std::path::PathBuf,
    heard: Vec<String>,
    request: Vec<String>,
    made: Option<(String, Value, u64)>,
    /// A skill with code of its own left off, waiting for its yes: what the
    /// yes agrees to and its say-back, to wait for it again when it is
    /// asked for again (`LAMP_MAKE_NO_CANNOT`).
    yes: Option<(crate::skill_write_yes::Asks, String)>,
    /// The voice's request that asked it, and the model's request, as told
    /// to a later line while it is made ([`making_note`]).
    made_in: u64,
    asked: String,
    /// The person declined it while it was made ([`making_answered`]): it
    /// is not put on, nothing of it is kept, and nothing of it is said.
    dropped: bool,
    /// Made, and past the last look for a no
    /// ([`OneAsk::dropped_or_finishing`]): being put on or said, too late
    /// to drop ([`FINISHING_NEXT`]).
    finishing: bool,
}

/// The voice's asks under way or made within [`ONE_ASK_WITHIN`].
static ASKS: std::sync::Mutex<Vec<HeldAsk>> = std::sync::Mutex::new(Vec::new());
static ASK_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A line's words for [`same_ask`]: lower case, each run of letters and
/// digits one word.
fn ask_words(line: &str) -> Vec<String> {
    line.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether a call's `heard` line and `request` are the ask held as
/// `held_heard` and `held_request` ([`same_ask`]). A heard line that holds
/// the held one whole and at least [`ONE_ASK_FEWEST_WORDS`] words more
/// carries an ask of its own beside it, and is the same ask only when the
/// requests are too (lamp-9adf, 17 Nov 18:05: a make cut by the next ask,
/// its line never answered, was heard again before "make a skill that says
/// stretch time whenever I say stretch", and that new ask was told it "is
/// being made already").
fn one_ask(
    held_heard: &[String],
    held_request: &[String],
    heard: &[String],
    request: &[String],
) -> bool {
    same_ask(held_request, request)
        || (same_ask(held_heard, heard)
            && held_heard.len().abs_diff(heard.len()) < ONE_ASK_FEWEST_WORDS)
}

/// Whether two lines' words are one ask: the one of at least
/// [`ONE_ASK_FEWEST_WORDS`] held whole, in order, in the other (the same
/// words, or a line carried into the next with the lamp's name or an
/// earlier piece before it, or without them). Two short lines alike, the
/// same answer to the lamp's question ("green", "yes", "do it") ending two
/// asks, are no one ask.
fn same_ask(one: &[String], other: &[String]) -> bool {
    if one.is_empty() || other.is_empty() {
        return false;
    }
    if one == other {
        return one.len() >= ONE_ASK_FEWEST_WORDS;
    }
    let (short, long) = if one.len() <= other.len() {
        (one, other)
    } else {
        (other, one)
    };
    short.len() >= ONE_ASK_FEWEST_WORDS && long.windows(short.len()).any(|words| words == short)
}

/// One voice make's ask (lamp-9adf, 12 Nov 15:13: "make me a skill: when
/// I come back to my desk, blink the light green twice" was cut while it
/// was made, carried into the next line, and `make_skill` called again
/// there: two skills of one trigger and the same steps, each outranking
/// the other's light). A call whose heard line or request is the same ask
/// ([`same_ask`]) as one under way in the same folder, or made and on
/// within [`ONE_ASK_WITHIN`] and still kept, makes nothing: it is told the
/// one under way, or the one made ("You already have that skill: NAME.
/// It's on.", or its say-back and yes again for one that waits for one).
/// Dropped before [`Self::made`], the ask is let go.
struct OneAsk(u64);

impl OneAsk {
    #[cfg(test)]
    fn begin(
        directory: &std::path::Path,
        heard: Option<&str>,
        request: &str,
        now_unix_ms: u64,
    ) -> Result<Self, Value> {
        Self::begin_in(directory, heard, request, now_unix_ms, 0)
    }

    /// [`Self::begin`] for the voice's request `made_in`.
    fn begin_in(
        directory: &std::path::Path,
        heard: Option<&str>,
        request: &str,
        now_unix_ms: u64,
        made_in: u64,
    ) -> Result<Self, Value> {
        let heard = ask_words(heard.unwrap_or_default());
        let asked = request;
        let request = ask_words(request);
        // Read from the folder before the lock, held by every voice make.
        let kept = made_skill::kept_names(directory);
        let mut asks = ASKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        asks.retain(|ask| match &ask.made {
            Some((name, _, at)) => {
                now_unix_ms.saturating_sub(*at) <= ONE_ASK_WITHIN.as_millis() as u64
                    && (ask.directory != directory || kept.contains(name))
            }
            None => true,
        });
        let earlier = asks.iter().position(|ask| {
            ask.directory == directory
                && !ask.dropped
                && one_ask(&ask.heard, &ask.request, &heard, &request)
        });
        // `LAMP_MAKE_NO_CANNOT` (lamp-9adf, 16 Nov: the same ask a minute
        // later was told the skill "is still on the lamp", and it was off):
        // one with code of its own left off says so, and its yes is waited
        // for again when the wait the make started is gone.
        if crate::make_no_cannot::on()
            && let Some(at) = earlier
            && let Some((name, told, _)) = &asks[at].made
            && told["code_waits_for_yes"] == true
        {
            let (name, mut told) = (name.clone(), told.clone());
            let waiting = told
                .get(crate::skill_write_yes::APPROVE_ID)
                .and_then(Value::as_str)
                .is_some_and(crate::skill_write_yes::waits);
            match (&asks[at].yes, waiting, made_in) {
                (Some((yes, line)), false, 1..) => {
                    let id = crate::skill_write_yes::wait_for_yes(
                        yes.clone(),
                        made_in,
                        std::time::Instant::now(),
                    );
                    told = waits_for_yes(told, line, &id);
                    if let Some(told) = told.as_object_mut() {
                        told.remove("writes_wait_for_yes");
                    }
                    if let Some((_, kept, _)) = asks[at].made.as_mut() {
                        *kept = told.clone();
                    }
                    eprintln!(
                        "lamp-voice: a skill wasn't made again: the same ask made {name} a moment ago, and it is off, waiting for its yes: read back again"
                    );
                }
                _ => eprintln!(
                    "lamp-voice: a skill wasn't made again: the same ask made {name} a moment ago, and it is off, waiting for its yes"
                ),
            }
            return Err(already_made(&name, &told));
        }
        match earlier.map(|at| &asks[at].made) {
            Some(Some((name, told, _))) => {
                eprintln!(
                    "lamp-voice: a skill wasn't made again: the same ask made {name} a moment ago, and it is still on the lamp"
                );
                Err(already_made(name, told))
            }
            Some(None) => {
                eprintln!(
                    "lamp-voice: a skill wasn't made again: the same ask is being made already"
                );
                Err(json!({
                    "making": true,
                    "next": format!(
                        "{}It is being made already, for this same ask, and nothing new is \
                         made. Tell the person in a few words, in the language of the \
                         conversation, that it is being made and they'll hear when it's on.",
                        made_skill::ALREADY_DONE
                    )
                }))
            }
            None => {
                let id = ASK_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                asks.push(HeldAsk {
                    id,
                    directory: directory.to_path_buf(),
                    heard,
                    request,
                    made: None,
                    yes: None,
                    made_in,
                    asked: asked_words(asked),
                    dropped: false,
                    finishing: false,
                });
                Ok(Self(id))
            }
        }
    }

    /// The ask's skill `name` made and on, its turn told `told`.
    fn made(self, name: &str, told: &Value, now_unix_ms: u64) {
        self.made_with(name, told, now_unix_ms, None);
    }

    /// [`Self::made`], with what a yes to a skill with code of its own left
    /// off agrees to and its say-back (`yes`).
    fn made_with(
        self,
        name: &str,
        told: &Value,
        now_unix_ms: u64,
        yes: Option<(crate::skill_write_yes::Asks, String)>,
    ) {
        let mut asks = ASKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(ask) = asks.iter_mut().find(|ask| ask.id == self.0) {
            ask.made = Some((name.to_owned(), told.clone(), now_unix_ms));
            ask.yes = yes;
        }
        std::mem::forget(self);
    }
}

impl Drop for OneAsk {
    fn drop(&mut self) {
        let mut asks = ASKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        asks.retain(|ask| ask.id != self.0);
    }
}

impl OneAsk {
    /// Whether the person declined the ask while it was made
    /// ([`making_answered`]).
    #[cfg(test)]
    fn dropped(&self) -> bool {
        ASKS.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|ask| ask.id == self.0 && ask.dropped)
    }

    /// Whether the person declined the ask while it was made
    /// ([`making_answered`]); if not, it is marked finishing in the same
    /// lock, so a no that comes later, while it is put on (the manage
    /// helper can take seconds) or said, is told it is too late to drop
    /// ([`FINISHING_NEXT`]), never that it was dropped.
    fn dropped_or_finishing(&self) -> bool {
        let mut asks = ASKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match asks.iter_mut().find(|ask| ask.id == self.0) {
            Some(ask) if ask.dropped => true,
            Some(ask) => {
                ask.finishing = true;
                false
            }
            None => false,
        }
    }
}

/// The most of the model's request told to a later line while it is made
/// ([`making_note`]).
const ASKED_MOST_CHARS: usize = 200;

/// The model's `request`, as a later line's note tells it: one line, at
/// most [`ASKED_MOST_CHARS`].
fn asked_words(request: &str) -> String {
    let line = request.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(ASKED_MOST_CHARS) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}

/// How the id of a voice make under way, told to a later line as its
/// `approve_id` ([`making_note`]), begins.
const MAKING_ID: &str = "making-";

/// What the model is told when the person declined a make, while it was
/// made or as its say-back waited for their yes (lamp-9adf, 17 Nov: "No,
/// don't turn it on." was answered "Should I stop making it altogether?",
/// and "Maybe later." with how to start a skill that was never kept). The
/// owner's fewest steps: dropped, said in one plain line, nothing asked.
pub const WONT_MAKE_NEXT: &str = "It is dropped: nothing of it is on. Say only, in the \
     conversation's language: \"Alright, I won't make it.\" Never say it is on, or how to \
     start it. Ask nothing, call nothing.";

/// What every note of a skill being made or waiting for its yes
/// ([`making_note`], `crate::skill_write_yes::waiting_note`) ends with
/// (lamp-9adf, 18 Nov 02:24–02:26: after a make waited for its yes, three
/// new make asks each got "I'll make that now. This takes about a minute.
/// I'm on it." with no call, nothing made): a skill asked for now is its
/// own ask, made by its own call, and a make is never said as begun
/// without one.
pub const NEW_ASK_OWN_CALL: &str = "A different skill they ask for now is a new ask, not \
     this one: call make_skill for it with its own request, as always. Never say a skill is \
     being made unless make_skill is called for it in this turn.";

/// What the model is told of a make still under way, to any answer but a
/// no: there is nothing to agree to yet.
const STILL_MAKING_NEXT: &str = "It is still being made: nothing is on yet, and there is \
     nothing to agree to now. Say only, in a few words, in the conversation's language, that \
     it's still being made and they'll hear when it's ready. Never say it is on, or how to \
     start it. Call nothing.";

/// What the model is told of a no to a make past its last look for one
/// ([`OneAsk::dropped_or_finishing`]): it is being finished, and the
/// make's own line says how it went.
const FINISHING_NEXT: &str = "It is just being finished, too late to stop: nothing was \
     dropped. Say only, in a few words, in the conversation's language, that it's almost done \
     and they'll hear in a moment; if they don't want it then, they can ask to remove it. \
     Never say it won't be made. Call nothing.";

/// What the model is told of an answer to a make no longer held.
const NOT_MAKING_NEXT: &str = "Nothing is being made now, and nothing was changed. Say so in \
     a few words, in the conversation's language. Call nothing.";

/// The note a later heard line `request` is told while a voice make is
/// under way (lamp-9adf, 17 Nov: an agent's make takes about a minute,
/// and lines heard meanwhile were answered as if the skill were on, or
/// with a question): what is being made, that nothing of it is on yet,
/// and the id a no drops it by ([`making_answered`]). The newest under
/// way, never in its own request; none with none. Read from memory: no
/// call, no file.
/// Only with `approve_id` declared (a made skill's writes, code, or a
/// skill's tasks on).
pub fn making_note(request: u64) -> Option<String> {
    if !(made_skill::writes_on() || crate::skill_task_yes::on() || made_skill::scripts_on()) {
        return None;
    }
    making_note_where(request, |_| true)
}

fn making_note_where(request: u64, of: impl Fn(&std::path::Path) -> bool) -> Option<String> {
    let asks = ASKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let ask = asks
        .iter()
        .filter(|ask| {
            ask.made.is_none()
                && !ask.dropped
                && ask.made_in != 0
                && request > ask.made_in
                && of(&ask.directory)
        })
        .max_by_key(|ask| ask.id)?;
    Some(format!(
        "(For you to know, not to answer: a skill is still being made for their ask \"{}\", \
         approve_id \"{MAKING_ID}{}\". It isn't made or on yet, and they'll hear when it is: \
         never say it is on, or how to start it. If this line declines it or puts it off, \
         call make_skill with only that approve_id and answer \"no\": it is dropped. \
         Otherwise don't call it with that approve_id. {NEW_ASK_OWN_CALL})",
        ask.asked, ask.id
    ))
}

/// What a `make_skill` call with `approve_id` `id` in `request` is, when
/// `id` names a voice make ([`making_note`]).
#[derive(Debug, PartialEq)]
pub enum Making {
    /// Not a make's id: the call goes on as a yes's.
    NotOurs,
    /// What the model is told.
    Told(Value),
    /// The make is done, and its say-back waits for a yes by this id: the
    /// call goes on with it.
    Waits(String),
}

/// The person's `answer` to the make `id` names, in a later `request`
/// ([`making_note`]): a no drops a make under way, which is then never put
/// on and says nothing ([`WONT_MAKE_NEXT`]); any other answer is told it
/// is still being made. A make done since, waiting for its yes, takes the
/// answer by its own id ([`Making::Waits`]). The model judges the answer;
/// code only reads the field.
pub fn making_answered(id: &str, request: u64, answer: Option<&str>) -> Making {
    let Some(held) = id
        .strip_prefix(MAKING_ID)
        .and_then(|held| held.parse::<u64>().ok())
    else {
        return Making::NotOurs;
    };
    let mut asks = ASKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(ask) = asks
        .iter_mut()
        .find(|ask| ask.id == held && request > ask.made_in)
    else {
        eprintln!("lamp-voice: request {request}: an answer to a make found none under way");
        return Making::Told(json!({ "approved": false, "next": NOT_MAKING_NEXT }));
    };
    match (&ask.made, answer) {
        (None, Some("no")) if ask.finishing => {
            eprintln!(
                "lamp-voice: request {request}: the person declined a make being finished: too late to drop"
            );
            Making::Told(json!({ "approved": false, "finishing": true, "next": FINISHING_NEXT }))
        }
        (None, Some("no")) => {
            ask.dropped = true;
            eprintln!(
                "lamp-voice: request {request}: the person declined a make under way: it is dropped, never put on"
            );
            Making::Told(json!({ "approved": false, "dropped": true, "next": WONT_MAKE_NEXT }))
        }
        (None, _) => {
            Making::Told(json!({ "approved": false, "making": true, "next": STILL_MAKING_NEXT }))
        }
        (Some((name, told, _)), _) => {
            match told
                .get(crate::skill_write_yes::APPROVE_ID)
                .and_then(Value::as_str)
            {
                Some(waits) => Making::Waits(waits.to_owned()),
                None => Making::Told(json!({
                    "approved": false,
                    "next": format!(
                        "It was made a moment ago, and nothing waits for their answer: {}. \
                         Say in a few words, in the conversation's language, what became of \
                         it; if they don't want it, they can ask to remove it. Call nothing.",
                        made_skill::spoken_name(name)
                    )
                })),
            }
        }
    }
}

/// What a call for an ask made a moment ago is told: the earlier `told`,
/// marked `already_made`. One that waits for the person's yes keeps its
/// say-back and `approve_id`, so the yes the cut turn never asked for can
/// still be asked; any other is said as had already
/// ([`made_skill::already_have_line`]).
fn already_made(name: &str, told: &Value) -> Value {
    let mut told = told.clone();
    told["already_made"] = json!(true);
    // One waiting for the person's yes, or for their choice on a clash
    // (#6593), keeps its question, so the cut turn's question is asked.
    if told.get(crate::skill_write_yes::APPROVE_ID).is_none()
        && told.get(crate::made_clash::CLASH_ID).is_none()
    {
        told["next"] = json!(format!(
            "{}Nothing new was made: this same ask made it a moment ago. Tell the person \
             this, in the language of the conversation, and add nothing else: \"{}\"",
            made_skill::ALREADY_DONE,
            made_skill::already_have_line(name)
        ));
    }
    told
}

/// What the person's yes to the kept skill `made` would approve: its
/// writes' exact contents kept beside it ([`made_skill::writes_of`]), its
/// package, and what must be heard whole before the yes, each write's
/// plain words as the say-back has them. None for a skill that writes
/// nothing.
pub(crate) fn asks_of(
    directory: &std::path::Path,
    made: &Made,
) -> Option<crate::skill_write_yes::Asks> {
    let writes = made_skill::writes_of(directory, &made.sha256);
    if writes.is_empty() {
        return None;
    }
    let say_back = writes
        .iter()
        .filter_map(made_skill::write_said)
        .collect::<Vec<_>>()
        .join(" ");
    Some(crate::skill_write_yes::Asks {
        skill: made.name.clone(),
        sha256: made.sha256.clone(),
        writes,
        say_back,
        grants: None,
        code: false,
        remove: false,
    })
}

/// What the person's yes to the kept skill `made`, with code of its own
/// (#6307), would agree to: that package, by its SHA-256 (every byte of its
/// code), put on, and its task kept for exactly the grants said
/// (`skill_task_yes`, #6305); and what must be heard whole before the yes,
/// what its code may use ([`made_skill::code_said`]).
pub(crate) fn code_asks(made: &Made) -> crate::skill_write_yes::Asks {
    crate::skill_write_yes::Asks {
        skill: made.name.clone(),
        sha256: made.sha256.clone(),
        writes: Vec::new(),
        say_back: if made.uses.agent_task {
            made_skill::agent_said(&made.uses.grants)
        } else {
            made_skill::code_said_as(&made.uses.grants, made_skill::code_goes_on_at_once(made))
        }
        .trim()
        .to_owned(),
        grants: Some(made.uses.grants.clone()),
        code: true,
        remove: false,
    }
}

/// A written turn `request`'s `text` on the second route, at `now_unix_ms`
/// and `now`: when it says back a skill with code of its own made from the
/// app (#6307, kept as asked by `app_make`, [`made_skill::code_asked_in`]
/// within the say service's longest wait, `app_make::LINE_WAITS_MS`), that
/// skill's yes is awaited from this request on through the very wait a
/// voice make starts ([`code_asks`], every guard of it: said whole in this
/// answer or a later one, a later heard line, the echo hold, this route),
/// its id told to each later heard line
/// ([`crate::skill_write_yes::waiting_note`]), and the turn ends as a voice
/// make's does ([`crate::skill_write_yes::then_wait`]). Outside the
/// private room it is said back only to the owner seen alone
/// (`sees_alone`, `services::owner_sees_alone`, as a skill run's write
/// is): else the turn says only that it stays off
/// ([`made_skill::CODE_BY_VOICE`]) and nothing waits. With
/// `LAMP_MADE_SCRIPTS` off, or any other text, the text as it is.
pub fn app_code_asked(
    directory: &std::path::Path,
    request: u64,
    text: String,
    now_unix_ms: u64,
    now: std::time::Instant,
    sees_alone: impl FnOnce() -> bool,
) -> String {
    if !made_skill::scripts_on() {
        return text;
    }
    let Some(made) =
        made_skill::code_asked_in(directory, now_unix_ms, crate::app_make::LINE_WAITS_MS)
    else {
        return text;
    };
    // Put on at once (`LAMP_MADE_CODE_ON_AT_ONCE`): nothing waits.
    if made_skill::code_goes_on_at_once(&made) {
        return text;
    }
    if !text.contains(&made_skill::on_line(&made)) {
        return text;
    }
    if !sees_alone() {
        eprintln!(
            "lamp-voice: request {request}: a skill with code of its own made from the app is not said back: the owner isn't seen alone (#6307)"
        );
        return made_skill::in_their_language(&made_skill::not_on_line(made_skill::CODE_BY_VOICE));
    }
    let id = crate::skill_write_yes::wait_for_yes_noted(code_asks(&made), request, now);
    eprintln!(
        "lamp-voice: request {request}: a skill with code of its own made from the app is said back; its yes is awaited (#6307)"
    );
    format!("{text} {}", crate::skill_write_yes::then_wait(&id))
}

/// [`app_code_asked`] on the lamp: the made skills' own folder, with
/// `LAMP_MAKE_SKILL` on in this process's environment.
pub fn app_code_asked_here(request: u64, text: String) -> String {
    if !made_skill::scripts_on() {
        return text;
    }
    let Some(keeping) = Keeping::from_environment() else {
        return text;
    };
    app_code_asked(
        &keeping.directory,
        request,
        text,
        crate::app_make::now_unix_ms(),
        std::time::Instant::now(),
        crate::services::owner_sees_alone_now,
    )
}

/// The yes `id` to the waiting made skill not the owner's (`owners_yes`,
/// `services::owners_yes`: outside the private room the owner seen saying
/// it or in their voice, as a skill run's write's send): its wait dropped,
/// nothing approved, and the model told [`crate::services::OTHER_VOICE`].
/// Every wait alike: a voice make's writes or code, a read-back's, a task's
/// and an app make's code (#6307; Heron's P1 on #6558: a voice make's write
/// approved on anyone's yes would send from the owner's linked account).
/// None for an id that isn't waiting, or the owner's yes.
pub fn not_owners_yes_refused(id: &str, owners_yes: impl FnOnce() -> bool) -> Option<Value> {
    if !crate::skill_write_yes::waits(id) || owners_yes() {
        return None;
    }
    let remove = crate::skill_write_yes::removes(id);
    crate::skill_write_yes::drop_wait(id);
    eprintln!(
        "lamp-voice: a made skill's yes not the owner's: its wait dropped, nothing approved (#6066, #6307)"
    );
    // A remove's (#6960): nothing removed, never "not sent".
    if remove {
        return Some(json!({ "error": NOT_OWNERS_REMOVE }));
    }
    Some(json!({ "error": crate::services::OTHER_VOICE }))
}

/// What a remove's yes not the owner's is told (#6960): nothing removed.
pub const NOT_OWNERS_REMOVE: &str = "Not removed, and cancelled: only the lamp's owner can agree to removing a skill they made. Say nothing was removed.";

/// The person's yes to a made skill with code of its own (#6307), once its
/// say-back was heard whole: its package put on by the SHA-256 said back
/// ([`made_skill::put_on_agreed`]), then, with a skill's tasks on (#6305),
/// its task's yes recorded by lamp-control for the SHA-256 of its folder as
/// the manage helper reads it once on, only while that folder's grants are
/// the ones said back (`task_asks` and `task_yes`,
/// `skill_task_yes::asks_by` and `approve_by`, never lamp-skills' state),
/// and only while the folder of that name went on as the very package
/// agreed to (its `.package.json`'s SHA-256 is the one said back), never
/// whatever folder has that name by then (the Reviewer-lead's P3 on #6355).
/// `{"on": NAME, "task": KEPT}`; a task not kept leaves the skill on with
/// its code run nowhere, as lamp-skills hands over only a kept task.
pub fn code_approve(
    keeping: &Keeping,
    asks: &crate::skill_write_yes::Asks,
    task_asks: impl FnOnce(&str) -> Result<Option<crate::skill_task_yes::TaskAsks>, String>,
    task_yes: impl FnOnce(&crate::skill_write_yes::Asks) -> Result<Value, String>,
) -> Result<Value, String> {
    let name = made_skill::put_on_agreed(keeping, &asks.sha256)?;
    let task = code_task_kept(&name, asks, task_asks, task_yes);
    Ok(json!({ "on": name, "task": task }))
}

#[cfg(test)]
thread_local! {
    /// What [`code_on_here`] reads and records in a test of this thread:
    /// the skill's task as the manage helper would read it (its `package`
    /// the one agreed), and each task's yes lamp-control would record.
    static TASKS_HERE: std::cell::RefCell<
        Option<(crate::skill_task_yes::TaskAsks, Vec<crate::skill_write_yes::Asks>)>,
    > = const { std::cell::RefCell::new(None) };
}

/// [`code_on_here`]'s task for this test's thread: `task` read, and the
/// yeses recorded since kept; `None` for the lamp's own sockets.
#[cfg(test)]
pub(crate) fn set_tasks_here(task: Option<crate::skill_task_yes::TaskAsks>) {
    TASKS_HERE.with(|here| *here.borrow_mut() = task.map(|task| (task, Vec::new())));
}

/// The task's yeses [`code_on_here`] recorded on this test's thread.
#[cfg(test)]
pub(crate) fn tasks_recorded_here() -> Vec<crate::skill_write_yes::Asks> {
    TASKS_HERE.with(|here| {
        here.borrow()
            .as_ref()
            .map(|(_, recorded)| recorded.clone())
            .unwrap_or_default()
    })
}

/// [`code_approve`] on the lamp: the task read by the manage helper at
/// `keeping.manage` and its yes recorded by lamp-control
/// ([`crate::skill_task_yes::CONTROL_SOCKET`]). The one path a skill with
/// code of its own goes on by: on the person's yes, or at once with
/// `LAMP_MADE_CODE_ON_AT_ONCE` ([`made_skill::code_goes_on_at_once`]).
pub fn code_on_here(
    keeping: &Keeping,
    asks: &crate::skill_write_yes::Asks,
) -> Result<Value, String> {
    #[cfg(test)]
    if let Some(told) = TASKS_HERE.with(|here| {
        let mut here = here.borrow_mut();
        let (task, recorded) = here.as_mut()?;
        let mut task = task.clone();
        task.package = Some(asks.sha256.clone());
        Some(code_approve(
            keeping,
            asks,
            |_| Ok(Some(task)),
            |kept| {
                recorded.push(kept.clone());
                Ok(json!({ "approved": 1 }))
            },
        ))
    }) {
        return told;
    }
    let control = std::path::Path::new(crate::skill_task_yes::CONTROL_SOCKET);
    code_approve(
        keeping,
        asks,
        |name| crate::skill_task_yes::asks_by(&keeping.manage, control, name),
        |task| crate::skill_task_yes::approve_by(&keeping.manage, control, task),
    )
}

/// A made skill with code of its own put on as it is made
/// (`LAMP_MADE_CODE_ON_AT_ONCE`, [`made_skill::code_goes_on_at_once`]), with no
/// spoken yes: exactly the package made, by its SHA-256
/// ([`made_skill::put_on_agreed`]). No yes is recorded (#8180): its
/// source needs none ([`made_skill::grants_need_no_yes`]), so lamp-control
/// mints its run's token from its folder's declared grants and agrees its
/// task's check, and its first run asks nothing. Whether its task runs
/// here (a skill's tasks on), or why it isn't on.
pub fn code_on_at_once(keeping: &Keeping, made: &Made) -> Result<bool, String> {
    made_skill::put_on_agreed(keeping, &code_asks(made).sha256)?;
    let task = crate::skill_task_yes::on();
    eprintln!(
        "lamp-voice: a made skill with code of its own is on at once (LAMP_MADE_CODE_ON_AT_ONCE); its task {}",
        if task { "kept" } else { "not kept" }
    );
    Ok(task)
}

/// What the lamp says of a skill with code of its own put on at once
/// ([`code_on_at_once`]): made and on, what it does and what its code may
/// use ([`made_skill::on_line`]), and that its code runs nowhere yet when
/// its task wasn't kept; or why it isn't on ([`made_skill::not_on_line`]).
pub fn code_on_line(made: &Made, on: &Result<bool, String>) -> String {
    // #8180: a tried skill put on is said with what its try did.
    let nowhere = match on {
        Ok(true) => Some(""),
        Ok(false) => Some(made_skill::CODE_RUNS_NOWHERE_YET),
        Err(_) => None,
    };
    if let Some(line) = nowhere.and_then(|extra| made_skill::tried_line(made, extra)) {
        return line;
    }
    match on {
        Ok(true) => made_skill::on_line(made),
        Ok(false) => format!(
            "{}{}",
            made_skill::on_line(made),
            made_skill::CODE_RUNS_NOWHERE_YET
        ),
        Err(why) => made_skill::not_on_line(why),
    }
}

/// [`code_approve`]'s task, kept for the skill `name` as it is on the lamp
/// now: whether it was.
fn code_task_kept(
    name: &str,
    asks: &crate::skill_write_yes::Asks,
    task_asks: impl FnOnce(&str) -> Result<Option<crate::skill_task_yes::TaskAsks>, String>,
    task_yes: impl FnOnce(&crate::skill_write_yes::Asks) -> Result<Value, String>,
) -> bool {
    if !crate::skill_task_yes::on() {
        eprintln!("lamp-voice: a made skill's task not kept: a skill's tasks are off");
        return false;
    }
    let set = |grants: &[String]| {
        grants
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<String>>()
    };
    let said = asks.grants.as_deref().unwrap_or_default();
    let task = match task_asks(name) {
        Ok(Some(task)) if task.package.as_deref() != Some(asks.sha256.as_str()) => {
            eprintln!(
                "lamp-voice: a made skill's task not kept: the skill on the lamp isn't the package agreed to"
            );
            return false;
        }
        Ok(Some(task)) if task.name == name && set(&task.grants) == set(said) => task,
        Ok(Some(_)) => {
            eprintln!("lamp-voice: a made skill's task not kept: its grants aren't those said");
            return false;
        }
        Ok(None) => {
            eprintln!("lamp-voice: a made skill's task not kept: the lamp reads no task in it");
            return false;
        }
        Err(why) => {
            eprintln!("lamp-voice: a made skill's task not kept: {why}");
            return false;
        }
    };
    let kept = crate::skill_write_yes::Asks {
        skill: task.name,
        sha256: task.sha256,
        writes: Vec::new(),
        say_back: asks.say_back.clone(),
        grants: Some(task.grants),
        code: false,
        remove: false,
    };
    match task_yes(&kept) {
        Ok(_) => true,
        Err(why) => {
            eprintln!("lamp-voice: a made skill's task not kept: {why}");
            false
        }
    }
}

/// The `make_skill` argument that reads a made skill back by its name.
pub const READ_BACK: &str = "read_back";

/// What the model is told when no made skill on the lamp has the name asked.
pub const NO_SKILL_BY_THAT_NAME: &str = "no skill made on this lamp is on by that name";

/// What the model is told when a read-back comes in a turn nobody spoke.
pub const READ_BACK_NOT_ASKED: &str =
    "a skill is read back only when the person asks for it out loud";

/// Who asks for a made skill's read-back ([`read_back`]), and where.
#[derive(Clone, Copy, Debug)]
pub struct Asking {
    /// The line's request.
    pub request: u64,
    /// A line someone spoke.
    pub heard: bool,
    /// What only the owner may hear is said whole to it: the private room,
    /// or, as a services read in full, the owner seen saying it, the one
    /// face in view and in their voice on the judge route
    /// (`services::owner_hears`).
    pub owner_hears: bool,
    /// The route with the echo judge, where a yes may be taken
    /// (`services::sends_here`).
    pub sends_here: bool,
}

/// What the model is told of a skill that sends, read back outside the
/// private room to a line not the owner's alone in view and in their voice
/// (`Asking::owner_hears` false): its name only.
pub fn sends_unsaid(spoken: &str) -> String {
    format!(
        "Say only this, in the conversation's language: \"{spoken} sends something on its own; \
         only its owner can hear what.\" Never whom it sends to or what it says. Ask nothing, \
         call nothing."
    )
}

/// What the model is told of a skill waiting for its yes, read back on a
/// route that can't take one (the Live route): it waits, and no wait starts.
pub fn yes_not_here(spoken: &str) -> String {
    format!(
        "Say plainly, in the conversation's language, that {spoken} waits for their yes before \
         it sends anything, and that I can't take that yes on this voice. Ask nothing, call \
         nothing."
    )
}

/// What the model is told of a skill whose package on the lamp is not the
/// one made (changed since, or being put on again): asked again later.
pub fn changed_since_made(spoken: &str) -> String {
    format!(
        "Say plainly, in the conversation's language, that {spoken} changed on the lamp since \
         it was made, so I can't read back what it sends right now, and that they can ask me \
         again in a moment. Ask nothing, call nothing."
    )
}

/// A made skill read back by voice ("what does my leaving mail skill
/// send?", the owner, 11 Nov), asked by `asking` (`heard`: a line someone
/// spoke), at `now`: the kept skill on the lamp named `name`
/// ([`made_skill::kept_on_named`]), from the made skills' `directory`.
/// One that sends or writes is read back only to the owner
/// (`owner_hears`: the private room, or, as a services read in full, the
/// owner seen saying the line, the one face in view and in their voice on
/// the judge route; App Only's P2s on #6230 and #6250); to anyone else its
/// name only, that it sends ([`sends_unsaid`]), and no wait starts.
/// One that sends or writes and is not approved for what it sends now
/// (`approved`, `lamp-services`' `skill_writes_approved`; not told is not
/// approved) is said back as its make said it ([`made_skill::on_line`],
/// word for word) and its writes wait for the yes again, through the very
/// wait a make starts ([`crate::skill_write_yes::wait_for_yes`]): the yes
/// comes as `approve_id` in a later heard line, with every guard of the
/// make's (said whole, uncut; the echo held; the second route; the
/// package said back; the link). Approved already, it is said with
/// [`made_skill::approved_line`] and nothing is asked. On a route that
/// can't take a yes (`sends_here` false), it is said to wait, no wait
/// starts and a pending make's wait is left as it is ([`yes_not_here`];
/// App Only's P3 on #6230). Its package on the lamp not the one made is
/// said so ([`changed_since_made`]). One that sends nothing is said so.
/// None of that name is said so, with the made skills' names on the lamp
/// only under `owner_hears`, since a name can carry a person's (Thirteen's
/// P3 on #6250). What the model is told.
pub fn read_back(
    name: &str,
    asking: Asking,
    directory: &std::path::Path,
    approved: impl FnOnce(&crate::skill_write_yes::Asks) -> Option<(bool, bool)>,
    now: std::time::Instant,
) -> Value {
    let request = asking.request;
    if !asking.heard {
        eprintln!(
            "lamp-voice: request {request}: a made skill wasn't read back: no one asked aloud"
        );
        return json!({ "error": READ_BACK_NOT_ASKED });
    }
    let Some(made) = made_skill::kept_on_named(directory, name) else {
        // Only the name asked, as a skill's name is written (Seventh on
        // #6071: nothing showed what the model passed).
        eprintln!(
            "lamp-voice: request {request}: a made skill to read back was not found by the name asked: {}",
            made_skill::name_asked(name)
        );
        if !asking.owner_hears {
            return json!({
                "error": NO_SKILL_BY_THAT_NAME,
                "on": [],
                "next": "Say plainly, in a few words, that there is no skill by that name. Name \
                         no other skill. Call nothing."
            });
        }
        let on: Vec<String> = made_skill::kept_on_names(directory)
            .iter()
            .map(|name| made_skill::spoken_name(name))
            .collect();
        return json!({
            "error": NO_SKILL_BY_THAT_NAME,
            "on": on,
            "next": "Say plainly, in a few words, that no skill they made is on by that name, \
                     and name the ones that are, if any. Call nothing."
        });
    };
    let spoken = made_skill::spoken_name(&made.name);
    let Some(asks) = asks_of(directory, &made) else {
        return json!({
            "skill": made.name,
            "sends": false,
            "next": format!(
                "Say plainly, in the conversation's language, that {spoken} doesn't send or \
                 write anything. Ask nothing, call nothing."
            )
        });
    };
    if !asking.owner_hears {
        eprintln!(
            "lamp-voice: request {request}: a made skill read back by name only: the owner wasn't seen asking"
        );
        return json!({
            "skill": made.name,
            "sends": true,
            "next": sends_unsaid(&spoken)
        });
    }
    match approved(&asks) {
        Some((false, _)) => {
            eprintln!(
                "lamp-voice: request {request}: a made skill to read back is not the one on the lamp"
            );
            json!({
                "skill": made.name,
                "error": "the skill on the lamp changed since it was made",
                "next": changed_since_made(&spoken)
            })
        }
        Some((true, true)) => {
            eprintln!("lamp-voice: request {request}: a made skill read back: approved already");
            json!({
                "skill": made.name,
                "sends": true,
                "approved": true,
                "next": format!(
                    "{} Ask nothing, call nothing: it is agreed already.",
                    crate::skill_write_yes::say_whole(&made_skill::approved_line(&made))
                )
            })
        }
        // Not approved, or not told, on a route that can't take its yes:
        // said to wait, and no wait started (a pending make's kept).
        _ if !asking.sends_here => {
            eprintln!(
                "lamp-voice: request {request}: a made skill read back: its yes can't be taken on this route"
            );
            json!({
                "skill": made.name,
                "sends": true,
                "error": "this voice can't take a yes",
                "next": yes_not_here(&spoken)
            })
        }
        // Not approved, or not told: said back, and its yes awaited.
        _ => {
            let id = crate::skill_write_yes::wait_for_yes(asks, request, now);
            eprintln!(
                "lamp-voice: request {request}: a made skill read back: it waits for the person's yes"
            );
            json!({
                "skill": made.name,
                "sends": true,
                "writes_wait_for_yes": true,
                crate::skill_write_yes::APPROVE_ID: id,
                "next": format!(
                    "{} {}",
                    crate::skill_write_yes::say_whole(&made_skill::on_line(&made)),
                    crate::skill_write_yes::then_wait(&id)
                )
            })
        }
    }
}

/// The words that start a question about a skill ([`read_back_asked`]).
const READ_BACK_QUESTIONS: [&str; 2] = ["what", "which"];

/// The words that may come between the question word and a skill's name
/// ([`read_back_asked`]): "what does desk check use", "what can it reach".
const READ_BACK_AUX: [&str; 4] = ["does", "do", "can", "will"];

/// The words that may come right before a skill's name
/// ([`read_back_asked`]): "what does my desk check skill need".
const READ_BACK_DETERMINERS: [&str; 5] = ["the", "my", "your", "this", "that"];

/// The words right after a skill's name (or after "skill") that ask what
/// it reaches ([`read_back_asked`]): "what does desk check use?", "what
/// can desk check reach". Never "say": what a skill says is not what it
/// reaches. Never "do" or "does" either, which most questions carry; "do"
/// counts only right after the name, with "does", "can" or "will" before
/// it ("what does desk check do with my mail", [`READ_BACK_DO`]).
const READ_BACK_ASKS: [&str; 13] = [
    "use", "uses", "using", "need", "needs", "access", "accesses", "reach", "reaches", "touch",
    "touches", "send", "sends",
];

/// The ask word only right after a skill's name, with an auxiliary before
/// it ([`READ_BACK_ASKS`]).
const READ_BACK_DO: &str = "do";

/// The words that run a skill ([`read_back_asked`]): a line that also runs
/// the skill asked about runs it, never read back in its place.
const READ_BACK_RUNS: [&str; 4] = ["run", "start", "use", "turn"];

/// The words a run verb may aim at the skill asked about by
/// ([`read_back_asked`]): "what does desk check use, and run it".
const READ_BACK_IT: [&str; 4] = ["it", "that", "this", "them"];

/// The most words a skill's name is said in ([`read_back_asked`]).
const READ_BACK_NAME_WORDS: usize = 6;

/// A heard line's words, lower case, split on anything but a letter or a
/// digit.
fn heard_words(heard: &str) -> Vec<String> {
    heard
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// A heard line's clauses, each its words ([`heard_words`]): broken at
/// `,` `;` `.` `?` `!` and at "and" and "then", so an ask word in one
/// clause is never another's ("what time is it? then run desk check, I
/// need it").
fn heard_clauses(heard: &str) -> Vec<Vec<String>> {
    let mut clauses = Vec::new();
    for part in heard.split([',', ';', '.', '?', '!']) {
        let mut clause = Vec::new();
        for word in heard_words(part) {
            if word == "and" || word == "then" {
                clauses.push(std::mem::take(&mut clause));
            } else {
                clause.push(word);
            }
        }
        clauses.push(clause);
    }
    clauses.retain(|clause| !clause.is_empty());
    clauses
}

/// The skill among `names` (kept made skills and pushed ones, as written:
/// `desk-check`) that a heard line asks what it uses, needs, does,
/// reaches or sends (V2 on #6413, lamp-52e6 at 12337: "what does desk
/// check use?" was answered by running the skill, "Hey Lamp, what does
/// desk check use?" by nothing). Within one clause ([`heard_clauses`]):
/// "what" or "which", at most one word ("which files"), then "does",
/// "do", "can" or "will" (needed after that one word), then "the", "my",
/// "your", "this" or "that" if said, then the name, then "skill" if said,
/// and then, as the very next word, one of [`READ_BACK_ASKS`], or "do"
/// after "does", "can" or "will" (SubDarren's and the Reviewer-lead's
/// HOLDs on #6619: "what music does she like" asks nothing of a `music`
/// skill). The name is matched loosely, number words as digits and the
/// dashes left out ([`made_skill::name_digits`]: "desk check",
/// "deskcheck", "skill two"). The longest name said wins; two different
/// names said as long, none, and the model is left the line (it never
/// picks one; no "which one?" is asked). A line that also runs the skill
/// asked about ([`READ_BACK_RUNS`] before its name, or before "it",
/// "that", "this" or "them", in any clause but the ask word itself: "what
/// does desk check use, and run it") asks none, so the run asked for
/// happens. "Use desk check", "run desk check" and "what time is it" ask
/// none. The name as written.
pub fn read_back_asked(heard: &str, names: &[String]) -> Option<String> {
    let loose = |name: &str| made_skill::name_digits(name).replace('-', "");
    let wanted: Vec<(String, &String)> = names
        .iter()
        .map(|name| (loose(name), name))
        .filter(|(wanted, _)| !wanted.is_empty())
        .collect();
    let clauses = heard_clauses(heard);
    // The name said at `words[start..]`, with its length.
    let named_at = |words: &[String], start: usize| -> Vec<(usize, &String)> {
        let mut found = Vec::new();
        for length in 1..=READ_BACK_NAME_WORDS.min(words.len().saturating_sub(start)) {
            let said = loose(&words[start..start + length].join("-"));
            for (wanted, name) in &wanted {
                if *wanted == said {
                    found.push((length, *name));
                }
            }
        }
        found
    };
    let mut best: Option<(usize, &String)> = None;
    let mut tied = false;
    // Each ask word matched, by clause and word, so a run check skips it.
    let mut ask_at: Vec<(usize, usize)> = Vec::new();
    for (index, words) in clauses.iter().enumerate() {
        let word = |at: usize| words.get(at).map(String::as_str);
        for question in 0..words.len() {
            if !READ_BACK_QUESTIONS.contains(&words[question].as_str()) {
                continue;
            }
            // Where the name may start: after the auxiliary (needed after
            // one word between), and after a determiner.
            let mut starts: Vec<(usize, bool)> = Vec::new();
            let after = question + 1;
            if word(after).is_some_and(|aux| READ_BACK_AUX.contains(&aux)) {
                starts.push((after + 1, word(after) != Some("do")));
            } else {
                starts.push((after, false));
                if word(after + 1).is_some_and(|aux| READ_BACK_AUX.contains(&aux)) {
                    starts.push((after + 2, word(after + 1) != Some("do")));
                }
            }
            for (start, may_do) in starts {
                let mut at = vec![start];
                if word(start).is_some_and(|det| READ_BACK_DETERMINERS.contains(&det)) {
                    at.push(start + 1);
                }
                for start in at {
                    for (length, name) in named_at(words, start) {
                        let mut next = start + length;
                        if word(next) == Some("skill") {
                            next += 1;
                        }
                        let asks = word(next).is_some_and(|ask| {
                            READ_BACK_ASKS.contains(&ask) || (may_do && ask == READ_BACK_DO)
                        });
                        if !asks {
                            continue;
                        }
                        ask_at.push((index, next));
                        match best {
                            Some((longest, _)) if longest > length => {}
                            Some((longest, kept)) if longest == length => tied |= kept != name,
                            _ => {
                                best = Some((length, name));
                                tied = false;
                            }
                        }
                    }
                }
            }
        }
    }
    let (_, name) = best.filter(|_| !tied)?;
    let asked = loose(name);
    let runs_it = clauses.iter().enumerate().any(|(index, words)| {
        (0..words.len()).any(|at| {
            if !READ_BACK_RUNS.contains(&words[at].as_str()) || ask_at.contains(&(index, at)) {
                return false;
            }
            let mut target = at + 1;
            if words[at] == "turn" {
                if words.get(target).map(String::as_str) != Some("on") {
                    return false;
                }
                target += 1;
            }
            if words
                .get(target)
                .is_some_and(|it| READ_BACK_IT.contains(&it.as_str()))
            {
                return true;
            }
            if words
                .get(target)
                .is_some_and(|det| READ_BACK_DETERMINERS.contains(&det.as_str()))
            {
                target += 1;
            }
            (1..=READ_BACK_NAME_WORDS.min(words.len().saturating_sub(target)))
                .any(|length| loose(&words[target..target + length].join("-")) == asked)
        })
    });
    (!runs_it).then(|| name.clone())
}

/// The skill a heard line asks what it uses ([`read_back_asked`]), among
/// the made skills kept on the lamp ([`made_skill::kept_on_names`]) and
/// those pushed from a computer ([`crate::skill_task_yes::pushed_names`]),
/// with the read-back on (`LAMP_SKILL_WRITES` or `LAMP_SKILL_TASKS`): the
/// second route reads it back in code, never left to the model (V2 on
/// #6413). The skills' folders are read only for a line with a question
/// word and an ask word.
pub fn read_back_heard(heard: &str) -> Option<String> {
    if !made_skill::writes_on() && !crate::skill_task_yes::on() {
        return None;
    }
    let words = heard_words(heard);
    let shaped = words
        .iter()
        .any(|word| READ_BACK_QUESTIONS.contains(&word.as_str()))
        && words
            .iter()
            .any(|word| READ_BACK_ASKS.contains(&word.as_str()) || word == READ_BACK_DO);
    if !shaped {
        return None;
    }
    let keeping = Keeping::when(true)?;
    read_back_asked(heard, &read_back_names(&keeping))
}

/// The skills [`read_back_heard`] reads back, from `keeping`: the made
/// skills kept on the lamp, those no longer on it dropped first
/// ([`made_skill::drop_gone`], #7100), and the pushed ones.
pub(crate) fn read_back_names(keeping: &Keeping) -> Vec<String> {
    made_skill::drop_gone(keeping);
    let mut names = made_skill::kept_on_names(&keeping.directory);
    names.extend(crate::skill_task_yes::pushed_names(&made_skill::pushed(
        &keeping.installed,
    )));
    names
}

/// Whether a model's call `name` with `args` is the skill `asked` run or
/// read back again, in a turn whose heard words already read it back in
/// code ([`read_back_asked`]): a `use_skill` of it (lamp-52e6 at 12337
/// ran it instead), or a `make_skill` read-back of it. Names match as
/// [`made_skill::name_asked`] writes them, number words as digits and
/// dashes left out.
pub fn same_skill_again(name: &str, args: &Value, asked: &str) -> bool {
    let named = match name {
        crate::use_skill::NAME => args.get("skill"),
        made_skill::MAKE => args.get(READ_BACK),
        _ => None,
    };
    let loose =
        |name: &str| made_skill::name_digits(&made_skill::name_asked(name)).replace('-', "");
    named
        .and_then(Value::as_str)
        .is_some_and(|named| loose(named) == loose(asked))
}

/// What a model's call of the skill already read back in code is told
/// ([`same_skill_again`]): not run, and the read-back's own result to say.
pub fn read_back_already(told: &Value) -> Value {
    json!({
        "error": "Not run: they asked what this skill uses, not to run it, and it was read back already.",
        "read_back": told,
        "next": "Say only what the read-back above tells you to say. Call nothing."
    })
}

/// The note round 1 gets once the heard words read `spoken` back in code
/// ([`read_back_asked`]), with its result `told`.
pub fn read_back_note(spoken: &str, told: &Value) -> String {
    format!(
        "(For you to know: they asked what {spoken} uses, so it was read back for them now, with \
         this result: {told}. Do as its \"next\" says. Do not run the skill, and do not call \
         use_skill or make_skill for it.)"
    )
}

/// What the model is told of a made skill whose writes wait for the yes
/// (#6066): its say-back said whole, every address and quoted word as
/// written, then nothing asked to repeat; on an agreement in any words, a
/// later call with `approve_id` alone; on a decline, a wait or a change,
/// no call.
fn waits_for_yes(mut told: Value, line: &str, id: &str) -> Value {
    told["writes_wait_for_yes"] = json!(true);
    told[crate::skill_write_yes::APPROVE_ID] = json!(id);
    told["next"] = json!(format!(
        "{}{} {}",
        made_skill::ALREADY_DONE,
        crate::skill_write_yes::say_whole(line),
        crate::skill_write_yes::then_wait(id)
    ));
    told
}

std::thread_local! {
    /// The line of the make [`make_with`] last finished on this thread
    /// ([`said_line`]), until [`take_said_line`] takes it.
    static MADE_LINE: std::cell::RefCell<Option<(String, Outcome)>> =
        const { std::cell::RefCell::new(None) };
}

/// What a make finished on this thread came to, beside its line
/// ([`take_said_outcome`]): told after a cut by what it is, never by its
/// turn's answer, which a cut turn's lapsed scope makes an `error` whatever
/// the make kept (lamp-9adf, 18 Nov, #7554's ROW at 2e864a0: request 86's
/// `dim-light-at-night` clashed and was asked, yet "kept nothing" was
/// journaled and its question never said).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Made, and on (or waiting for its yes).
    Made,
    /// Made, and not put on: its clash waits for the person's choice
    /// ([`crate::made_clash`]).
    Clash,
    /// Nothing made, or made and not put on.
    Failed,
}

impl Outcome {
    fn of(kept: bool) -> Self {
        if kept { Self::Made } else { Self::Failed }
    }
}

/// The line the lamp says of the make just finished on this thread, for
/// a caller that gave it no turn to say it in (lamp-9adf, 11 Nov 02:16
/// and 02:19: a wordless cut ended the turn waiting on `make_skill`, the
/// skill was made and put on, and its line was never said): made and on
/// ([`made_skill::on_line`]; one that sends or writes,
/// [`made_skill::cut_write_line`]), made and not on ([`made_skill::not_on_line`]),
/// or not made ([`made_skill::not_made_line`]). None when no make finished
/// here since the last take (not asked aloud, the brief refused).
pub fn take_said_line() -> Option<String> {
    take_said_outcome().map(|(line, _)| line)
}

/// [`take_said_line`], with what the make came to ([`Outcome`]).
pub fn take_said_outcome() -> Option<(String, Outcome)> {
    MADE_LINE.with(|kept| kept.take())
}

/// The line [`take_said_line`] gives next on this thread, said in place of
/// the make's own (`crate::made_change`: a change is told as one).
pub(crate) fn said_line_now(line: String, outcome: Outcome) {
    MADE_LINE.with(|kept| kept.replace(Some((line, outcome))));
}

/// Tests: the line [`take_said_line`] gives next on this thread, as a make
/// of another kind (one with code, cut) would leave it.
#[cfg(test)]
pub fn set_said_line(line: String) {
    MADE_LINE.with(|kept| kept.replace(Some((line, Outcome::Made))));
}

/// A make that failed with no line of its own set on this thread: `line`,
/// as one failed line, said once by whoever takes it (a make that waited
/// its turn, `crate::make_queue::Entry::run`). A line already set stays.
pub fn failed_unless_said(line: impl FnOnce() -> Option<String>) {
    MADE_LINE.with(|kept| {
        if kept.borrow().is_none()
            && let Some(line) = line()
        {
            kept.replace(Some((line, Outcome::Failed)));
        }
    });
}

/// Tests: [`set_said_line`] with what the make came to, `outcome`.
#[cfg(test)]
pub fn set_said_outcome(line: String, outcome: Outcome) {
    MADE_LINE.with(|kept| kept.replace(Some((line, outcome))));
}

/// The one short line of a make that kept `made`, `on` or not; or not
/// made and why: the line [`told`] gives the model, as written. One that
/// sends or writes, made and on, is [`made_skill::cut_write_line`]: this
/// line is said only for a turn cut, which never gave the model its say-back
/// or its `approve_id`, so its yes can't be taken (App Only's P2 on #6142).
fn said_line(
    made: &Result<Made, Refused>,
    on: &Result<(), String>,
    needs: Option<&[Value]>,
) -> String {
    match (made, on) {
        (Ok(made), Ok(())) if !made.uses.writes.is_empty() => made_skill::cut_write_line(made),
        (Ok(made), Ok(())) if made.uses.code => made_skill::cut_code_line(made, needs),
        (Ok(made), Ok(())) => made_skill::on_line(made),
        // A task for the lamp's agent (#7234): never said as made.
        (Ok(made), Err(why)) if made.uses.agent_task => made_skill::agent_not_on_line(why),
        (Ok(_), Err(why)) => made_skill::not_on_line(why),
        (Err(Refused(why, _)), _) => made_skill::not_made_line(why),
    }
}

/// How many voice makes are under way in this process, or have a line
/// still being said for a turn that was cut ([`Pending`]).
static PENDING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// One voice make under way, from its call until its line was given to
/// its turn or, its turn cut, said through the say service: dropped then.
/// While any is held, and at most [`MAKE_WITHIN`] after the make's call
/// (the second route's `Shared::quiet_for_a_make`), a wordless cut-in is
/// not asked "did anyone say something?" ([`pending`]).
pub struct Pending(());

impl Pending {
    pub fn begin() -> Self {
        PENDING.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Self(())
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        PENDING.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

/// Whether a voice make is under way, or its line is still to be said
/// ([`Pending`]).
pub fn pending() -> bool {
    PENDING.load(std::sync::atomic::Ordering::Acquire) > 0
}

/// The requests whose make's work runs now in this process ([`MakeUnderWay`]),
/// each with whether it was let run on ([`let_go`]).
static MAKING: std::sync::Mutex<Vec<(u64, bool)>> = std::sync::Mutex::new(Vec::new());

fn making_now() -> std::sync::MutexGuard<'static, Vec<(u64, bool)>> {
    MAKING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// How many of [`MAKING`] count as under way, sent as it changes: the
/// second route's engine forwards it to the coordinator
/// (`ProviderOutput::MakesUnderWay`, #7418).
fn under_way_sender() -> &'static tokio::sync::watch::Sender<u32> {
    static SENDER: std::sync::OnceLock<tokio::sync::watch::Sender<u32>> =
        std::sync::OnceLock::new();
    SENDER.get_or_init(|| tokio::sync::watch::Sender::new(0))
}

/// One voice make's work running for its request, the turn's or a queued
/// one's (#7418): from just before `work()` until it returns, not until
/// its line is said, which the say service may hold far longer. The lamp
/// thinks while any counts ([`under_way`]); a make let run on
/// ([`crate::long_make::let_go`], [`let_go`]) never counts again, as its
/// person was told they'd hear when it's ready.
pub struct MakeUnderWay(u64);

impl MakeUnderWay {
    pub fn begin(request: u64) -> Self {
        let mut making = making_now();
        making.push((request, false));
        recount(&making);
        Self(request)
    }
}

impl Drop for MakeUnderWay {
    fn drop(&mut self) {
        let mut making = making_now();
        if let Some(at) = making.iter().position(|(of, _)| *of == self.0) {
            making.remove(at);
        }
        recount(&making);
    }
}

/// `request`'s make was let run on (`crate::long_make::let_go`): it no
/// longer counts as under way.
pub fn let_go(request: u64) {
    let mut making = making_now();
    for (of, let_go) in making.iter_mut() {
        if *of == request {
            *let_go = true;
        }
    }
    recount(&making);
}

/// Whether `request`'s make counts as under way now ([`MakeUnderWay`]).
pub fn making(request: u64) -> bool {
    making_now()
        .iter()
        .any(|(of, let_go)| *of == request && !let_go)
}

/// The count of `making` under way told again, only when it changed: with
/// [`MAKING`] held, so two changes are never told out of their order.
fn recount(making: &[(u64, bool)]) {
    let count = making.iter().filter(|(_, let_go)| !let_go).count();
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    under_way_sender().send_if_modified(|told| {
        let changed = *told != count;
        *told = count;
        changed
    });
}

/// The count of makes under way ([`MakeUnderWay`]), as it changes.
pub fn under_way() -> tokio::sync::watch::Receiver<u32> {
    under_way_sender().subscribe()
}

/// The newest request whose line the person said with words and the
/// lamp answered ([`person_went_on`]).
static WENT_ON: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The second route answers `request`, a line heard with words: the
/// person has gone on from any make whose turn was cut before it. A sound
/// with no words, or a line held as not said to the lamp, never comes here
/// (lamp-9adf, 18 Nov, #7554's ROW at 2e864a0: requests 6 and 17 heard 0
/// words, and request 57 was held, yet each make they cut failed untold).
pub fn person_went_on(request: u64) {
    WENT_ON.fetch_max(request, std::sync::atomic::Ordering::AcqRel);
}

/// Whether the person went on ([`person_went_on`]) with a line after
/// `request`'s: a cut make of `request` that kept nothing then says nothing.
pub fn went_on_after(request: u64) -> bool {
    WENT_ON.load(std::sync::atomic::Ordering::Acquire) > request
}

/// The skill for `request` (its `brief` made already) written by the
/// model `chat` gives and kept in `keeping` as made from `from`; a
/// refused one asked again with what was refused, up to [`MOST_REPAIRS`] times. Blocking: the model
/// is asked on a thread of its own. Shared by the voice's `make_skill` and
/// the app's typed ask (`crate::app_make`). With the lamp's agent `agent`
/// (`LAMP_MAKE_SKILL_AGENT`, [`crate::make_skill_agent::Agent::for_app`]),
/// a make DeepSeek didn't make is handed to it exactly as a voice make's
/// is ([`handed_to_agent`]); with none, or when the agent doesn't take
/// it, DeepSeek keeps its repairs (Heron's P2 on #6788).
pub fn make_kept(
    request: &str,
    brief: String,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    from: &str,
    agent: Option<&crate::make_skill_agent::Agent>,
) -> Result<Made, Refused> {
    make_kept_heard(request, None, brief, keeping, chat, from, agent)
}

/// [`make_kept`] by voice: a write held to `heard`, the line the person
/// said, never to the model's `request` (App Only's P1 on #6142).
fn make_kept_heard(
    request: &str,
    heard: Option<&str>,
    brief: String,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    from: &str,
    agent: Option<&crate::make_skill_agent::Agent>,
) -> Result<Made, Refused> {
    let (made, line) = made_and_journaled_by(request, heard, brief, keeping, chat, from, agent);
    eprintln!("{line}");
    made
}

/// [`make_kept`]'s make, and the journal's line for it ([`kept_line`]).
#[cfg(test)]
fn made_and_journaled(
    request: &str,
    heard: Option<&str>,
    brief: String,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    from: &str,
) -> (Result<Made, Refused>, String) {
    made_and_journaled_by(request, heard, brief, keeping, chat, from, None)
}

/// [`made_and_journaled`], with the lamp's agent `agent` to hand a make
/// DeepSeek didn't make to ([`handed_to_agent`]).
fn made_and_journaled_by(
    request: &str,
    heard: Option<&str>,
    brief: String,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    from: &str,
    agent: Option<&crate::make_skill_agent::Agent>,
) -> (Result<Made, Refused>, String) {
    let began = std::time::Instant::now();
    let chat = Arc::new(chat);
    // The person's own words: the times of day they name, so only a window
    // within one runs at night without quiet hours (Thirteen's HOLD on
    // #6103); and a write's words and recipients (#6066), with what is
    // linked now for their own address.
    // The judge of a made skill with no word of the ask (#7621): the
    // person's own line ([`OwnLine`], or the line given for a make called
    // here directly), asked of the make's own chat in one short call.
    let own_line = OWN_LINE
        .with(std::cell::RefCell::take)
        .unwrap_or_else(|| heard.map(str::to_owned));
    let judge_ask = |question: &str| judged(Arc::clone(&chat), question.to_owned());
    let given = GIVEN.with(std::cell::RefCell::take);
    let asked = made_skill::Asked {
        request,
        own: own_accounts_now(),
        heard,
        named: given.as_deref(),
        judge: own_line
            .as_deref()
            .filter(|_| made_skill::unrelated_on())
            .map(|line| made_skill::Judge {
                line,
                ask: &judge_ask,
            }),
        made_with: made_skill::MadeWith::DeepSeek,
    };
    // Whether the last answer was the model's own `cannot:`: its reason is
    // never journaled ([`kept_line`]).
    let models_cannot = std::cell::Cell::new(false);
    // Whether the last answer was the model's word that what was asked
    // needs the internet when it runs (`LAMP_TASK_NET` on,
    // [`crate::make_no_cannot::NEEDS_NET`]): made one task for the lamp's
    // agent ([`net_made`]), never repaired nor handed to the agent to write.
    let net = crate::make_no_cannot::net_by_agent();
    let needs_net = std::cell::Cell::new(false);
    // DeepSeek's first answer on a lamp whose agent writes a web make as
    // code granted `net` ([`net_code_by`], #7742): a task for the agent
    // with no linked service ([`web_task_for_agent`]) needs the internet,
    // as its `internet:` line does. Never the agent's own answers.
    let web_task_held = std::cell::Cell::new(false);
    // Each answer kept as made by its engine (`asked`'s `made_with`,
    // [`made_skill::MADE_WITH`]): DeepSeek's by [`kept`], the agent's by
    // `agent_kept` below.
    let kept_by = |answer: Result<String, String>, asked: &made_skill::Asked<'_>| {
        models_cannot.set(matches!(&answer, Ok(text) if made_skill::says_cannot(text)));
        needs_net.set(
            net && matches!(&answer, Ok(text) if crate::make_no_cannot::says_needs_net(text)
                || (web_task_held.get() && web_task_for_agent(text))),
        );
        if needs_net.get() {
            return Err(Refused(crate::make_no_cannot::NEEDS_NET_HELD.into(), None));
        }
        match answer {
            // The maker's own example is never kept: refused as the check
            // refuses, so it goes back to the model once.
            Ok(text) if is_example(&text) => {
                eprintln!("lamp-voice: a skill made here: the model gave back the example");
                Err(Refused(
                    made_skill::EXAMPLE_GIVEN_BACK_WHY.into(),
                    Some(EXAMPLE_GIVEN_BACK.into()),
                ))
            }
            Ok(text) => made_skill::keep_asked(&text, false, from, keeping, asked),

            Err(why) => {
                eprintln!("lamp-voice: a skill made here: the model didn't answer: {why}");
                Err(Refused(made_skill::MODEL_SILENT.into(), None))
            }
        }
    };
    let kept = |answer: Result<String, String>| kept_by(answer, &asked);
    // With the switch and an agent on the lamp, DeepSeek gets one try and
    // no repairs; what it doesn't make goes to the agent, which gets the
    // repairs. Without either, DeepSeek's repairs stay as they are; for
    // the app's MAKE IT they stay too when the agent doesn't take it.
    // Hermes alone: only it has a namespace runner for a make today.
    // One targets ask per make: the lamp's agent, and whether there is
    // surely none.
    let seen = agent.map(|agent| (agent, agent.lamp_agent()));
    let engine = seen.and_then(|(agent, (seen, _))| Some((agent, agent.takes(seen?)?)));
    // With the one run path (#7245), a lamp with no agent at all (lamp-tasks
    // answered its targets with no `lamp` one): a make the agent would have
    // taken is told so in one sentence ([`made_skill::AGENT_MISSING`], the
    // first builder's point 3 on #6811 for #7375), never made by DeepSeek
    // alone as a task for an agent that isn't there. A targets read that
    // failed (lamp-tasks down, restarting, slow or refusing) is not known
    // to be no agent: DeepSeek makes it alone, as before. Off, as before.
    let missing = matches!(seen, Some((agent, (None, true))) if agent.one_path);
    // The agent's answers kept as its own (#6811, [`made_skill::MADE_WITH`],
    // written with `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on alone).
    let agent_asked = made_skill::Asked {
        made_with: engine.map_or(made_skill::MadeWith::DeepSeek, |(_, engine)| {
            made_skill::MadeWith::of_engine(engine)
        }),
        ..asked.clone()
    };
    let agent_kept = |answer: Result<String, String>| kept_by(answer, &agent_asked);
    let unknown = matches!(seen, Some((agent, (None, false))) if agent.one_path);
    if missing {
        eprintln!(
            "lamp-voice: a skill made here: no agent on the lamp (no lamp target): DeepSeek \
             makes only what it can keep alone"
        );
    } else if unknown {
        eprintln!(
            "lamp-voice: a skill made here: the tasks part didn't say its targets: DeepSeek \
             makes it alone, as before"
        );
    } else if agent.is_some() && engine.is_none() {
        eprintln!(
            "lamp-voice: a skill made here: no Hermes agent on the lamp: DeepSeek makes it alone"
        );
    }
    // The last refusal a repair was asked for: what the person hears when
    // a later answer is cut or never comes, never the model's cut alone.
    let refused_before = std::cell::Cell::new(None);
    // Refused by the lamp's check, by `plainly`, as TOML that can't be
    // read, or the example given back: its lines go back to the model, up
    // to [`MOST_REPAIRS`] times (`made_skill::repair_brief`; the owner,
    // 10 Nov: "re-run it yourself until it works"), each begun only while
    // one more round, two answers at worst, still fits [`MAKE_WITHIN`],
    // the time `skipped` (a hand-over the agent didn't take) not counted.
    // With `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on and a make judged a quick
    // reaction (`held_to_reaction`), every DeepSeek repair is held to
    // [`made_skill::quick_reaction`] as its first draft is, before it is
    // kept (the Reviewer-lead's P2 on #7764): one that is no quick reaction
    // is never kept, and `repaired_not_reaction` sends the make to the
    // middle form. Never set on any other make.
    let held_to_reaction = std::cell::Cell::new(false);
    let repaired_not_reaction = std::cell::Cell::new(None::<String>);
    let deepseek_repaired = |made: Result<Made, Refused>, skipped: Duration| {
        let (made, before) = repaired(
            made,
            request,
            "",
            MOST_REPAIRS,
            || round_fits(began.elapsed().saturating_sub(skipped)),
            |repair| {
                let answer = ask(Arc::clone(&chat), repair);
                if held_to_reaction.get()
                    && let Ok(text) = &answer
                    && let Some(why) = made_skill::not_a_reaction(text)
                {
                    repaired_not_reaction.set(Some(why));
                    return Err(Refused(NOT_A_REACTION.to_owned(), None));
                }
                kept(answer)
            },
        );
        refused_before.set(before);
        made
    };
    // With a service skill made as a task for the agent
    // (`LAMP_AGENT_SERVICES` and a made skill's code on), a make whose words
    // ask for a linked service (mail, a calendar, Drive, GitHub, or one
    // told linked) goes straight to the agent, DeepSeek not asked (the
    // owner's ask, 13 Nov; lamp-9adf: 3 of 3 voice makes that send mail
    // refused by DeepSeek, never handed over). The person's own line and
    // the model's request are both looked at; what is linked is asked of
    // `lamp-services` only when no plain word is there.
    let agent_services = crate::make_skill_agent::services_through_agent();
    // With no agent at all (`missing`) too: such a make is told so.
    let service = (agent_services && (engine.is_some() || missing))
        .then(|| {
            let words = format!("{request}\n{}", heard.unwrap_or_default());
            crate::make_skill_agent::service_asked(&words, || {
                crate::services::linked_codes(std::path::Path::new(crate::services::FOLDER))
            })
        })
        .flatten();
    // A make the agent was handed first, as it stands after it: judged not
    // what was asked (#7621) asked again; else the internet's task and the
    // model's cannot as any make's.
    let agents_made = |made: Result<Made, Refused>| {
        if matches!(&made, Err(Refused(why, _)) if why == made_skill::UNRELATED) {
            let line = kept_line(&made, false, began.elapsed().as_millis());
            return (made, line);
        }
        let made = net_made(
            made,
            &needs_net,
            request,
            &chat,
            began,
            &kept,
            (from, keeping, asked.named),
        );
        let made = no_cannot(
            made,
            &models_cannot,
            request,
            &chat,
            began,
            &kept,
            (from, keeping, asked.named),
        );
        let line = kept_line(&made, models_cannot.get(), began.elapsed().as_millis());
        (plain_unless_cannot(made, models_cannot.get()), line)
    };
    if missing && let Some(service) = service {
        eprintln!(
            "lamp-voice: a skill made here: it asks for a linked service ({service}): no agent \
             on the lamp to make it: told so, DeepSeek not asked"
        );
        let made = Err(Refused(made_skill::AGENT_MISSING.to_owned(), None));
        let line = kept_line(&made, false, began.elapsed().as_millis());
        return (plain_unless_cannot(made, false), line);
    }
    // `LAMP_MAKE_AGENT_FIRST=on` (#7245, the owner 18 Nov 14:0x): every
    // make goes straight to the lamp's agent, DeepSeek asked only when the
    // agent gives no answer; off unless set.
    let straight = service
        .map(|service| format!("it asks for a linked service ({service})"))
        .or_else(|| {
            std::env::var(AGENT_FIRST_ENVIRONMENT)
                .is_ok_and(|value| value == "on")
                .then(|| format!("{AGENT_FIRST_ENVIRONMENT} is on"))
        });
    if let (Some((agent, engine)), Some(why)) = (engine, straight) {
        eprintln!(
            "lamp-voice: a skill made here: {why}: straight to the agent ({engine}), DeepSeek \
             not asked"
        );
        let made = handed_to_agent(
            Before::NotAsked(Box::new(|| kept(ask(Arc::clone(&chat), brief.clone())))),
            request,
            &brief,
            (agent, engine),
            &agent_kept,
            &models_cannot,
            &deepseek_repaired,
        );
        return agents_made(made);
    }
    // On a lamp whose agent writes a web make as code (#7742): DeepSeek is
    // told a task for the agent is only for a linked service, and its
    // answer as one for the web is held as needing the internet.
    let net_code = engine.is_some_and(|(_, engine)| net_code_by(engine));
    let first_ask = || match net_code {
        true => ask_net_code(Arc::clone(&chat), brief.clone()),
        false => ask(Arc::clone(&chat), brief.clone()),
    };
    // With `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on (#6811, the first
    // builder's ruling and the wider route he said yes to at 16:34), and an
    // agent on the lamp or surely none: only a quick reaction is
    // DeepSeek's. The owner's "do it like V1" on #7764: the make's first
    // call is DeepSeek's judgement of the request's meaning
    // ([`reaction_judged`]), never a draft. Judged one, DeepSeek drafts it,
    // and a draft that is no quick reaction ([`made_skill::quick_reaction`]:
    // started by the person's own act, with only the light, a gesture, its
    // exact words or its hum, never the model's marker said) is never
    // kept. Judged none, unsure or with no answer, no draft is asked for.
    // Either way the first builder's 19:50 ruling (the middle form): the
    // lamp writes it itself, as one task for its agent ([`agent_task`]):
    // the trigger its own model picks in one more call (or the words that
    // ask for it), one `do.task` whose words are the person's request,
    // done by the agent when it fires, and sip-of-water's guards
    // (`make_no_cannot::skill_text`); no say and no other step a model
    // wrote. Not made, it is said so plainly and nothing is kept. A
    // `cannot:` (a one-off reminder among them, the lamp's own timer's),
    // the internet's line or what can't be read keeps its own route. With
    // no agent on the lamp (`missing`), told so; an agent not known, no
    // judgement: DeepSeek's as before.
    let routed = made_skill::reactions_on() && (engine.is_some() || missing);
    let judged_none = routed && reaction_judged(Arc::clone(&chat), request) != Some(true);
    let first = (!judged_none).then(first_ask);
    // A draft's name, for a long make's lines before the agent's is known
    // (`LAMP_MAKE_MAY_RUN_ON`, #6811); judged none, there is no draft.
    if crate::long_make::on()
        && let Some(Ok(text)) = &first
        && let Some(name) = made_skill::files(text)
            .ok()
            .and_then(|files| made_skill::card_name(&files.card))
    {
        crate::long_make::hint(crate::make_skill_agent::made_for(), &name);
    }
    let route = if judged_none {
        Some("judged not a quick reaction".to_owned())
    } else {
        routed
            .then(|| {
                first
                    .as_ref()?
                    .as_ref()
                    .ok()
                    .and_then(|text| made_skill::not_a_reaction(text))
                    .map(|why| format!("drafted, not a quick reaction ({why})"))
            })
            .flatten()
    };
    // The middle form, for a make judged none or drafted (or repaired) as
    // no quick reaction: one task for the agent, or told it has none.
    let middle_form = |why: &str| {
        if let Some((_, engine)) = engine {
            eprintln!(
                "lamp-voice: a skill made here: {why}: to the agent ({engine}), its trigger and \
                 one task of the request; no DeepSeek draft kept"
            );
            let made = agent_task(
                request,
                &chat,
                began,
                &agent_kept,
                (from, keeping, asked.named),
            );
            let line = kept_line(&made, false, began.elapsed().as_millis());
            return (plain_unless_cannot(made, false), line);
        }
        eprintln!(
            "lamp-voice: a skill made here: {why}: no agent on the lamp to make it: told so, \
             no DeepSeek draft kept"
        );
        let made = Err(Refused(made_skill::AGENT_MISSING.to_owned(), None));
        let line = kept_line(&made, false, began.elapsed().as_millis());
        (plain_unless_cannot(made, false), line)
    };
    if let Some(why) = route {
        return middle_form(&why);
    }
    // Judged a quick reaction: its repairs are held to it too.
    held_to_reaction.set(routed);
    let repaired_away = || {
        repaired_not_reaction
            .take()
            .map(|why| format!("repaired, not a quick reaction ({why})"))
    };
    // Never left unasked here: a route with no draft returned above.
    let first = first.unwrap_or_else(first_ask);
    // A timed skill whose only speaking is a fixed say beside the maker's
    // "couldn't do" note (the first builder's ruling on #8651): never kept
    // with its value dropped while an agent can write it as a task step
    // ([`made_skill::says_fixed_with_left_out`]); with none, kept as
    // before, and journaled.
    let value_left_out = matches!(&first, Ok(text) if made_skill::says_fixed_with_left_out(text));
    web_task_held.set(net_code);
    let mut made = match engine {
        Some(_) if value_left_out => Err(Refused(made_skill::VALUE_LEFT_OUT_HELD.into(), None)),
        _ => {
            if value_left_out {
                eprintln!(
                    "lamp-voice: a skill made here: DeepSeek couldn't work out what it says: no \
                     agent to hand it to: kept as DeepSeek's"
                );
            }
            kept(first)
        }
    };
    web_task_held.set(false);
    // Judged not what was asked (#7621): no repair, no agent, asked again.
    if matches!(&made, Err(Refused(why, _)) if why == made_skill::UNRELATED) {
        let line = kept_line(&made, false, began.elapsed().as_millis());
        return (made, line);
    }
    if engine.is_none() {
        made = deepseek_repaired(made, Duration::ZERO);
        if let Some(why) = repaired_away() {
            return middle_form(&why);
        }
    }
    let made = match engine {
        // What only the internet tells (`LAMP_TASK_NET` on): never handed
        // to the agent to write, which is told the same rules; made one
        // task for it below ([`net_made`]).
        // On a gateway agent whose run granted `net` reaches the web from
        // its own skill-start fence (#7742): handed to it to write as code
        // of the skill's own, granted `net`.
        Some((agent, engine)) if needs_net.get() && net_code_by(engine) => {
            eprintln!(
                "lamp-voice: a skill made here: DeepSeek said it needs the internet: handed to \
                 the agent ({engine}) to write as code granted net"
            );
            handed_to_agent(
                Before::Asked(Box::new(made)),
                request,
                &brief,
                (agent, engine),
                &agent_kept,
                &models_cannot,
                &deepseek_repaired,
            )
        }
        Some(_) if needs_net.get() => {
            eprintln!(
                "lamp-voice: a skill made here: DeepSeek said it needs the internet: not handed \
                 to the agent to write"
            );
            made
        }
        // A fixed say with its value dropped (held above): handed to the
        // agent as the internet's is, to write as a task step whose script
        // prints the line, its answer told (#8610).
        Some((agent, engine)) if matches!(&made, Err(Refused(why, _)) if why == made_skill::VALUE_LEFT_OUT_HELD) =>
        {
            eprintln!(
                "lamp-voice: a skill made here: DeepSeek couldn't work out what it says: handed \
                 to the agent ({engine}) to write as a task step"
            );
            handed_to_agent(
                Before::Asked(Box::new(made)),
                request,
                &brief,
                (agent, engine),
                &agent_kept,
                &models_cannot,
                &deepseek_repaired,
            )
        }
        // The person's own words for its start breaking a rule no writer
        // can change (#8451): told at once, never handed to the agent to
        // write again.
        Some((_, engine)) if matches!(&made, Err(Refused(why, _)) if made_skill::their_ask(why)) => {
            eprintln!(
                "lamp-voice: a skill made here: its start is the person's own words, which break \
                 a rule: told so, not handed to the agent ({engine})"
            );
            made
        }
        // A `cannot:` the system prompt itself asks for (the brief's own
        // refusals, [`made_skill::BRIEFS_OWN_REFUSALS`]) is said as it is,
        // never handed over (the Reviewer-lead on #6439); one of a send
        // ([`made_skill::BRIEFS_SEND_REFUSALS`]) goes to the agent when it
        // makes service skills, as it can send.
        // With the one run path (#7245), the lamp's agent can do what a
        // plain skill can't (write a file, run a job), so the brief's
        // "Not possible yet: a skill can't" goes to it; the brief's other
        // refusals stay DeepSeek's.
        Some((agent, _))
            if stays_deepseeks(&made, models_cannot.get(), agent_services, agent.one_path) =>
        {
            eprintln!(
                "lamp-voice: a skill made here: DeepSeek's cannot is one the brief asks for: \
                 not handed to the agent"
            );
            made
        }
        Some((agent, engine)) if made.is_err() => {
            if models_cannot.get()
                && agent_services
                && matches!(&made, Err(Refused(why, _)) if made_skill::briefs_send_refusal(why))
            {
                eprintln!(
                    "lamp-voice: a skill made here: DeepSeek's cannot is a send, which the agent \
                     makes with its lamp-services tools"
                );
            }
            eprintln!(
                "lamp-voice: a skill made here: DeepSeek failed: handed to the agent ({engine})"
            );
            handed_to_agent(
                Before::Asked(Box::new(made)),
                request,
                &brief,
                (agent, engine),
                &agent_kept,
                &models_cannot,
                &deepseek_repaired,
            )
        }
        Some(_) => {
            eprintln!("lamp-voice: a skill made here: made by DeepSeek");
            made
        }
        // No agent to hand it to (`missing`): DeepSeek's failure, after its
        // repairs, is told as the agent missing, but for what would have
        // stayed DeepSeek's with one.
        None if missing
            && made.is_err()
            && !stays_deepseeks(&made, models_cannot.get(), agent_services, true)
            && !matches!(&made, Err(Refused(why, _)) if made_skill::their_ask(why)) =>
        {
            eprintln!(
                "lamp-voice: a skill made here: DeepSeek failed and there is no agent on the \
                 lamp to hand it to: told so"
            );
            models_cannot.set(false);
            Err(Refused(made_skill::AGENT_MISSING.to_owned(), None))
        }
        None => made,
    };
    // DeepSeek's repairs for a make the agent didn't take (the app's).
    if let Some(why) = repaired_away() {
        return middle_form(&why);
    }
    let made = net_made(
        made,
        &needs_net,
        request,
        &chat,
        began,
        &kept,
        (from, keeping, asked.named),
    );
    let made = no_cannot(
        made,
        &models_cannot,
        request,
        &chat,
        began,
        &kept,
        (from, keeping, asked.named),
    );
    let line = kept_line(&made, models_cannot.get(), began.elapsed().as_millis());
    // A repair's answer cut or never come (a product lamp, 11 Nov, from the
    // app: three repairs on its hum, then a cut stream, and the person
    // heard "the lamp's model stopped before it finished writing it"):
    // the person hears why the skill itself was refused. A first answer
    // cut, with nothing refused before, is told as the cut.
    let made = match (made, refused_before.take()) {
        (Err(Refused(why, _)), Some(before))
            if [made_skill::MODEL_CUT_OFF, made_skill::MODEL_SILENT].contains(&why.as_str()) =>
        {
            eprintln!(
                "lamp-voice: a made skill's last answer failed after its repairs: told why it \
                 was refused"
            );
            Err(before)
        }
        (made, _) => made,
    };
    (plain_unless_cannot(made, models_cannot.get()), line)
}

/// Whether DeepSeek's failed `made` stays its own, never handed to the
/// lamp's agent: its `cannot:` (`models_cannot`) one the system prompt
/// itself asks for ([`made_skill::briefs_own_refusal`], the Reviewer-lead
/// on #6439), but for a send's when the agent makes service skills
/// (`agent_services`) and, with the one run path (`one_path`, #7245), the
/// brief's "Not possible yet: a skill can't".
fn stays_deepseeks(
    made: &Result<Made, Refused>,
    models_cannot: bool,
    agent_services: bool,
    one_path: bool,
) -> bool {
    models_cannot
        && matches!(made, Err(Refused(why, _)) if made_skill::briefs_own_refusal(why)
            && !(agent_services && made_skill::briefs_send_refusal(why))
            && !(one_path && made_skill::briefs_skill_cant(why)))
}

/// With `LAMP_MAKE_NO_CANNOT` on, a make whose last answer was the model's
/// own `cannot:` (`models_cannot`) made one task for the lamp's agent
/// ([`crate::make_no_cannot::made`]): its grants and trigger asked of the
/// lamp's own model in one more call (`chat`, begun only while one round
/// still fits the make's longest, [`longest_make`]), kept through the
/// make's own checks (`kept`), or only asked for, kept from `from` in
/// `keeping` as an agent skill ([`made_skill::keep_agent_asked`]); or a
/// reason no grant covers. So is a skill that would only tell the person
/// to do what they asked of it ([`made_skill::ECHOES_THE_ASK`]). A
/// `cannot:` of the lamp's own model that the system prompt itself asks for
/// ([`made_skill::briefs_own_refusal`]), but for a send's
/// ([`made_skill::briefs_send_refusal`]), stays a refusal, as #6439 has it:
/// one that keeps people and their services safe is said with the lamp's
/// own plain reason ([`crate::make_no_cannot::kept_refusal`]), any other as
/// the model wrote it. The agent's ([`crate::make_no_cannot::AGENTS_CANNOT`])
/// is never said. Off, or any other make, `made` as it is.
fn no_cannot(
    made: Result<Made, Refused>,
    models_cannot: &std::cell::Cell<bool>,
    request: &str,
    chat: &Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    began: std::time::Instant,
    kept: &impl Fn(Result<String, String>) -> Result<Made, Refused>,
    (from, keeping, named): (&str, &Keeping, Option<&str>),
) -> Result<Made, Refused> {
    if !crate::make_no_cannot::on() {
        return made;
    }
    // The brief's own refusals, but for a send's, stay refusals (#6439;
    // sim-2's P2 on #7261; the owner: "if something can't be done, say
    // exactly why"): one that keeps people or their services safe is said
    // with the lamp's own plain reason, never the bare line.
    if models_cannot.get()
        && let Err(Refused(why, _)) = &made
        && why != crate::make_no_cannot::AGENTS_CANNOT
        && made_skill::briefs_own_refusal(why)
        && !made_skill::briefs_send_refusal(why)
    {
        let Some((class, line)) = crate::make_no_cannot::kept_refusal(why) else {
            return made;
        };
        eprintln!(
            "lamp-voice: a skill made here: the brief's own refusal stays one, said with its \
             reason ({class})"
        );
        models_cannot.set(false);
        return Err(Refused(line.to_owned(), None));
    }
    // The owner, 16 Nov 11:4x: any other `cannot:` of the model's is never
    // final, nor is a skill that would only tell the person to do what they
    // asked of it.
    match &made {
        Err(Refused(why, _)) if why == made_skill::ECHOES_THE_ASK => {}
        Err(Refused(_, _)) if models_cannot.get() => {}
        _ => return made,
    }
    eprintln!(
        "lamp-voice: a skill made here: the model's cannot goes to the lamp's agent as one task \
         ({})",
        crate::make_no_cannot::ENVIRONMENT
    );
    let made = agent_task(request, chat, began, kept, (from, keeping, named));
    models_cannot.set(false);
    made
}

/// With `LAMP_TASK_NET` on ([`crate::make_no_cannot::net_by_agent`]), a
/// make whose last answer was the model's word that it needs the internet
/// when it runs (`needs_net`, [`crate::make_no_cannot::NEEDS_NET`]) made
/// one task for the lamp's agent ([`agent_task`]), whatever
/// `LAMP_MAKE_NO_CANNOT`: asked for by its words, an agent skill granted
/// `net` that reads back and waits for the yes at its first ask; on a
/// trigger, a made task for the agent on it, granted `net` (the owner's
/// ruling, 17 Nov), said back and waiting for the spoken yes. Any other
/// make, `made` as it is.
fn net_made(
    made: Result<Made, Refused>,
    needs_net: &std::cell::Cell<bool>,
    request: &str,
    chat: &Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    began: std::time::Instant,
    kept: &impl Fn(Result<String, String>) -> Result<Made, Refused>,
    (from, keeping, named): (&str, &Keeping, Option<&str>),
) -> Result<Made, Refused> {
    if !needs_net.replace(false) || made.is_ok() {
        return made;
    }
    eprintln!(
        "lamp-voice: a skill made here: the model said it needs the internet: one task for the \
         lamp's agent ({})",
        crate::skill_task_yes::NET_ENVIRONMENT
    );
    agent_task(request, chat, began, kept, (from, keeping, named))
}

/// The make of `request` as one task for the lamp's agent
/// ([`crate::make_no_cannot::made`]): its name, grants and when it runs
/// asked of the lamp's own model in one more call (`chat`, begun only while
/// one round still fits [`longest_make`]), kept through the make's own
/// checks (`kept`), or only asked for, kept from `from` in `keeping` as an
/// agent skill ([`made_skill::keep_agent_asked`]).
fn agent_task(
    request: &str,
    chat: &Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    began: std::time::Instant,
    kept: &impl Fn(Result<String, String>) -> Result<Made, Refused>,
    (from, keeping, named): (&str, &Keeping, Option<&str>),
) -> Result<Made, Refused> {
    crate::make_no_cannot::made(
        request,
        |brief| {
            ask_in(
                Arc::clone(chat),
                crate::make_no_cannot::system(),
                brief.to_owned(),
            )
        },
        || began.elapsed() + ROUND_WITHIN <= longest_make(),
        kept,
        |files, words| made_skill::keep_agent_asked(files, from, keeping, words),
        // Only a hint: the model decides by naming the grant (the lead's
        // rule; the first builder on #7261).
        crate::make_skill_agent::service_hint(request),
        named,
    )
}

/// The person hears the lamp's own reason in everyday words, the
/// journal's line keeping it whole (the owner, 11 Nov: reasons "nobody
/// understands"); the model's `cannot:` sentence (`models_cannot`) goes as
/// it is.
fn plain_unless_cannot(made: Result<Made, Refused>, models_cannot: bool) -> Result<Made, Refused> {
    match made {
        Err(Refused(why, lines)) if !models_cannot => {
            Err(Refused(made_skill::plain_reason(&why), lines))
        }
        made => made,
    }
}

/// What DeepSeek did before a make was handed to the agent: its one
/// answer, given no repairs, not kept ([`Before::Asked`]); or nothing yet,
/// a make that asks for a linked service going straight to the agent
/// ([`Before::NotAsked`]), with DeepSeek's one try, no repairs, for when
/// the agent gives no answer.
enum Before<'a> {
    Asked(Box<Result<Made, Refused>>),
    NotAsked(Box<dyn FnOnce() -> Result<Made, Refused> + 'a>),
}

impl Before<'_> {
    /// What stands when the agent gave no answer: DeepSeek's one answer,
    /// asked now when it wasn't yet.
    fn stands(self) -> Result<Made, Refused> {
        match self {
            Self::Asked(made) => *made,
            Self::NotAsked(ask) => ask(),
        }
    }

    /// How the journal says what stands then.
    fn journaled(&self) -> &'static str {
        match self {
            Self::Asked(_) => "DeepSeek's one answer, given no repairs, stands as not made",
            Self::NotAsked(_) => "DeepSeek is asked once, given no repairs",
        }
    }
}

/// A refused `made`'s repairs: its check's lines go back to the writer
/// (`again`), up to `most` times ([`MOST_REPAIRS`], or the agent's own
/// with its guide), each begun only while `fits`. `by` names the writer in
/// the journal ("" for DeepSeek, as before). Beside the last answer, the
/// last refusal a repair was asked for.
fn repaired(
    mut made: Result<Made, Refused>,
    request: &str,
    by: &str,
    most: usize,
    fits: impl Fn() -> bool,
    mut again: impl FnMut(String) -> Result<Made, Refused>,
) -> (Result<Made, Refused>, Option<Refused>) {
    let mut refused_before = None;
    for round in 1..=most {
        let Err(Refused(why, Some(lines))) = &made else {
            break;
        };
        if !fits() {
            eprintln!(
                "lamp-voice: a made skill's repair {round} of {most}{by}: no time left, \
                 not asked"
            );
            break;
        }
        let Ok(repair) = made_skill::repair_brief(request, lines) else {
            break;
        };
        // What was refused, in the lamp's own words: the part itself (its
        // keys and topic, or the check's first line) is journaled where it
        // was refused, never the model's lines.
        eprintln!("lamp-voice: a made skill's repair {round} of {most}{by}: {why}");
        refused_before = Some(Refused(why.clone(), None));
        made = again(repair);
    }
    (made, refused_before)
}

/// What stands when the agent made nothing for a reason of its own side
/// (`why`: lamp-tasks busy, its run failed, no answer within its 90 s):
/// DeepSeek's failure (`stood`), said as the agent's
/// ([`crate::make_skill_agent::not_made_try`]) when it would be the tip to
/// ask for one thing ([`made_skill::NOT_MADE_TRY`], [`made_skill::NOT_KEPT_TRY`])
/// or DeepSeek's `cannot:` of what the agent could make (the audit's A3,
/// 16 Nov: "each morning read my unread mail" with the agent busy was told
/// "Try asking for one thing"). A skill DeepSeek made after all, a lamp
/// reason of its own (it can't do that yet, they have it already) and the
/// brief's own refusals stand as they are.
fn agent_side(
    stood: Result<Made, Refused>,
    why: &str,
    models_cannot: &std::cell::Cell<bool>,
) -> Result<Made, Refused> {
    let agents = |said: &str| match models_cannot.get() {
        true => !made_skill::briefs_own_refusal(said) || made_skill::briefs_send_refusal(said),
        false => [made_skill::NOT_MADE_TRY, made_skill::NOT_KEPT_TRY]
            .contains(&made_skill::plain_reason(said).as_str()),
    };
    match stood {
        Err(Refused(said, lines)) if agents(&said) => {
            models_cannot.set(false);
            Err(Refused(
                crate::make_skill_agent::not_made_try(why).to_owned(),
                lines,
            ))
        }
        stood => stood,
    }
}

/// A make DeepSeek didn't make (`deepseek`), handed to the lamp's agent
/// `agent`, called `engine` (`LAMP_MAKE_SKILL_AGENT`): the same system
/// prompt and `brief`, its answer through the same checks (`kept`) and
/// with repairs of its own, each begun only while at least
/// [`REPAIR_AT_LEAST`] of the agent's [`crate::make_skill_agent::MAKE_WITHIN`]
/// is left and given no more than what is left. With no answer
/// from it at first (none within its 90 s, its run busy or failed, no make
/// run, or no agent after all), DeepSeek's failure stands, as without the
/// switch, said as the agent's but for no make run ([`agent_side`]); a
/// make sent straight to the agent ([`Before::NotAsked`]) then gets
/// DeepSeek's one try, given no repairs. For the app's MAKE IT
/// ([`crate::make_skill_agent::Agent::repairs_if_untaken`], Heron's P2 on
/// #6788) DeepSeek then gets its repairs as without the agent
/// (`deepseek_repaired`, the hand-over's time not counted).
#[allow(clippy::too_many_arguments)]
fn handed_to_agent(
    deepseek: Before<'_>,
    request: &str,
    brief: &str,
    (agent, engine): (&crate::make_skill_agent::Agent, &str),
    kept: &impl Fn(Result<String, String>) -> Result<Made, Refused>,
    models_cannot: &std::cell::Cell<bool>,
    deepseek_repaired: &impl Fn(Result<Made, Refused>, Duration) -> Result<Made, Refused>,
) -> Result<Made, Refused> {
    let began = std::time::Instant::now();
    // Given the long make's time (`LAMP_MAKE_MAY_RUN_ON`): its outcome goes
    // through the long make's gate if its turn is cut (El's P2 on #7774).
    if agent.may_run_on {
        crate::long_make::to_agent(crate::make_skill_agent::made_for());
    }
    // The person's own addresses never go in the agent's rules file: each
    // stands there as a placeholder, put back in its answer before the
    // checks (the Reviewer-lead on #6439).
    let hidden = placeholders(&own_accounts());
    let system = hide(&system(), &hidden);
    // On a gateway agent whose run granted `net` reaches the web from its
    // own skill-start fence (#7742): what only the internet tells is the
    // skill's own code granted `net`, told and checked so while it makes.
    let net_code = net_code_by(engine);
    let system = if net_code {
        eprintln!(
            "lamp-voice: a skill made here: the agent ({engine}) may write code granted net: \
             its run reaches the web from its own skill-start fence"
        );
        system.replacen(&net_told(), &net_code_told(), 1)
    } else {
        system
    };
    let _net_code = net_code.then(made_skill::NetCode::on);
    // #8157: the lamp's agent writes a linked-service skill's scripts
    // whole at the make, never the one-line hand-over DeepSeek writes.
    let system = agents_own_scripts(&system);
    // Its `internet:` line taken as the prompt it was given says.
    let net = crate::make_no_cannot::net_by_agent();
    // The waiting line with the first hand-over only, and never when the
    // turn was cut already (Twelve's P3 on #6439).
    let waiting = std::cell::Cell::new(!turn_cut());
    let write_by = |agent: &crate::make_skill_agent::Agent, brief: &str| {
        agent
            .write(&system, brief, waiting.replace(false))
            .map(|text| shown(&text, &hidden))
            .map_err(|why| why.to_string())
            .and_then(|text| whole(text, net).map_err(|why| why.to_string()))
    };
    let write = |brief: &str| write_by(agent, brief);
    // Not taken by the agent: DeepSeek's one answer stands for a voice
    // make, and gets its repairs for the app's (Heron's P2 on #6788).
    let stands = match (agent.repairs_if_untaken, &deepseek) {
        (false, _) => deepseek.journaled(),
        (true, Before::Asked(_)) => "DeepSeek gets its repairs, as without the agent",
        (true, Before::NotAsked(_)) => "DeepSeek is asked, with its repairs, as without the agent",
    };
    let untaken = |deepseek: Before<'_>| {
        let made = deepseek.stands();
        if !agent.repairs_if_untaken {
            return made;
        }
        eprintln!("lamp-voice: a skill made here: not taken by the agent: DeepSeek's repairs");
        deepseek_repaired(made, began.elapsed())
    };
    let first = match write(brief) {
        Ok(text) => text,
        // The first builder's guard on #6643: the agent writes only in a
        // skill run's namespace, never as a Harness task with its own tools.
        Err(why) if why == crate::make_skill_agent::NotWritten::NoMakeRun.to_string() => {
            eprintln!(
                "lamp-voice: a skill made here: the agent ({engine}) can't be run in a skill \
                 run's namespace here (lamp-tasks gives no make run): not handed over: {stands}"
            );
            return untaken(deepseek);
        }
        // Harness or the gateway not ready yet (#7245): the person is told
        // at once in the lamp's short words, never DeepSeek's answer.
        Err(why) if why == made_skill::NOT_READY_LINE => {
            eprintln!(
                "lamp-voice: a skill made here: the agent ({engine}) isn't ready yet: told to wait a minute"
            );
            return Err(Refused(made_skill::NOT_READY_LINE.to_owned(), None));
        }
        Err(why) => {
            eprintln!(
                "lamp-voice: a skill made here: the agent ({engine}) made none ({why}): {stands}"
            );
            // No answer of the agent's was taken, so whether the last was a
            // `cannot:` is still DeepSeek's (`models_cannot`); said as the
            // agent's own failure ([`agent_side`]), after DeepSeek's repairs
            // for the app's.
            // One let run on (#6811): nothing kept, never DeepSeek's after
            // it, and the agent's own failure said plainly.
            if crate::long_make::is_let_go(crate::make_skill_agent::made_for()) {
                return Err(Refused(
                    crate::make_skill_agent::not_made_try(&why).to_owned(),
                    None,
                ));
            }
            return agent_side(untaken(deepseek), &why, models_cannot);
        }
    };
    let first = kept(Ok(first));
    let first = cannot_asked_again(first, agent, began, models_cannot, |agent, again| {
        kept(write_by(agent, &format!("{brief}\n\n{again}")))
    });
    let (made, refused_before) = repaired(
        first,
        request,
        " by the agent",
        agent.most_repairs(),
        || agent.make_within.saturating_sub(began.elapsed()) >= REPAIR_AT_LEAST,
        |repair| {
            // Each repair's write is given no more than what is left of the
            // agent's make time, so the make's own bound holds.
            let left = agent.make_within.saturating_sub(began.elapsed());
            let within = crate::make_skill_agent::Agent {
                within: agent.within.min(left),
                ..agent.clone()
            };
            kept(write_by(&within, &repair))
        },
    );
    // A repair cut at its deadline or never answered (#8514, El's run 1816
    // on lamp-ee17: repair 1 ended at lamp-tasks' deadline, and the person
    // heard the model's silence, the say step's reason lost): what stands
    // is the refusal the repair was asked for, said once in its plain words
    // ([`made_skill::plain_reason`], #8454's one line), as DeepSeek's are.
    // A refusal with no time left for a repair already stands as it is.
    let made = match (made, refused_before) {
        (Err(Refused(why, _)), Some(before))
            if [made_skill::MODEL_CUT_OFF, made_skill::MODEL_SILENT].contains(&why.as_str()) =>
        {
            eprintln!(
                "lamp-voice: a made skill's repair by the agent ({engine}) gave no answer in \
                 time: told why it was refused"
            );
            Err(before)
        }
        (made, _) => made,
    };
    match &made {
        Ok(_) => eprintln!("lamp-voice: a skill made here: made by the agent ({engine})"),
        Err(_) => eprintln!("lamp-voice: a skill made here: the agent's ({engine}) wasn't kept"),
    }
    // The agent's own `cannot:` is never said as it wrote it: the lamp
    // says its own fixed line (the Reviewer-lead on #6439); with
    // `LAMP_MAKE_NO_CANNOT` on, its words are dropped all the same and the
    // make is made one task for the agent ([`no_cannot`]).
    match made {
        Err(Refused(_, lines)) if models_cannot.get() => Err(Refused(
            if crate::make_no_cannot::on() {
                crate::make_no_cannot::AGENTS_CANNOT
            } else {
                crate::make_skill_agent::CANNOT_LINE
            }
            .to_owned(),
            lines,
        )),
        made => made,
    }
}

/// The least of the agent's make time ([`crate::make_skill_agent::MAKE_WITHIN`])
/// left for one more repair by the agent, that repair given no more than
/// what is left (#7874). With the guide, one write may take its whole
/// [`crate::make_skill_agent::GUIDED_WRITE_WITHIN`] (10 min) of a 10 min
/// 30 s make, so asking for a whole write's time left let a repair run
/// only when the first answer came within 30 s, and OpenClaw's takes
/// 34-40 s: its repairs never ran. Two minutes is about three of its
/// answers, enough for a repair to be written and checked.
const REPAIR_AT_LEAST: Duration = Duration::from_secs(120);

/// The least of the agent's make time ([`crate::make_skill_agent::MAKE_WITHIN`])
/// left for [`cannot_asked_again`]'s one more round.
const CANNOT_AGAIN_LEAST: Duration = Duration::from_secs(40);

/// What the agent is told with its `cannot:` asked again.
const CANNOT_AGAIN_CAN: &str = "The lamp can do that: a skill keeps notes with lamp.notes.set \
     and reads them back with lamp.notes.get, writes facts, jokes or lines asked for into its \
     code or a say as fixed text, and is started by its own asks.";

/// The agent's first answer `made` on the one run path (#7245, lamp-c0a4,
/// 18 Nov: OpenClaw's cheap model answered `cannot:` 4 of 5 makes with a
/// reason of its own making): a `cannot:` (`models_cannot`) naming none
/// of the lamp's real limits ([`made_skill::real_limit`]) is asked once
/// more through `again`, its reason quoted back, while at least
/// [`CANNOT_AGAIN_LEAST`] of the agent's make time is left, that round
/// given no more than what is left. A second `cannot:` is taken as
/// today; a real limit's is never asked again.
fn cannot_asked_again(
    made: Result<Made, Refused>,
    agent: &crate::make_skill_agent::Agent,
    began: std::time::Instant,
    models_cannot: &std::cell::Cell<bool>,
    again: impl FnOnce(&crate::make_skill_agent::Agent, &str) -> Result<Made, Refused>,
) -> Result<Made, Refused> {
    let Err(Refused(why, None)) = &made else {
        return made;
    };
    if !agent.one_path || !models_cannot.get() || made_skill::real_limit(why) {
        return made;
    }
    let left = agent.make_within.saturating_sub(began.elapsed());
    if left < CANNOT_AGAIN_LEAST {
        eprintln!("lamp-voice: a made skill's cannot: no time left to ask again");
        return made;
    }
    // The reason may echo the person's words: never journaled
    // ([`made_skill::MODELS_CANNOT_JOURNALED`], Second's HOLD on #5985).
    eprintln!(
        "lamp-voice: a made skill's cannot asked again once: its reason names no limit of the \
         lamp's (told, not journaled)"
    );
    let within = crate::make_skill_agent::Agent {
        within: agent.within.min(left),
        ..agent.clone()
    };
    again(
        &within,
        &format!(
            "You answered cannot: {why}. {CANNOT_AGAIN_CAN} Make the skill now, as rules.md says."
        ),
    )
}

std::thread_local! {
    /// The voice turn waiting on this thread's make, while it waits: gone
    /// once the turn was cut (`second_provider::worker::made_running`).
    static TURN: std::cell::RefCell<Option<std::sync::Weak<()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Kept by the second route's worker on the thread a voice make runs on:
/// the turn waiting for it holds `turn`'s other end.
pub fn set_turn(turn: std::sync::Weak<()>) {
    TURN.with(|kept| *kept.borrow_mut() = Some(turn));
}

/// Whether the voice turn waiting for this thread's make was cut; no for
/// a make no turn waits on.
fn turn_cut() -> bool {
    TURN.with(|kept| {
        kept.borrow()
            .as_ref()
            .is_some_and(|turn| turn.strong_count() == 0)
    })
}

/// Each of the person's own addresses in `own` (a linked service's
/// account), with the placeholder that stands for it in the agent's rules
/// file, the longest first so none is replaced inside another.
fn placeholders(own: &[(String, String)]) -> Vec<(String, String)> {
    let mut accounts: Vec<String> = own
        .iter()
        .map(|(_, account)| account.clone())
        .filter(|account| account.contains('@'))
        .collect();
    accounts.sort_by_key(|account| std::cmp::Reverse(account.len()));
    accounts.dedup();
    accounts
        .into_iter()
        .enumerate()
        .map(|(n, account)| (account, format!("own{}.lamp@example.invalid", n + 1)))
        .collect()
}

/// `text` with each own address replaced by its placeholder.
fn hide(text: &str, hidden: &[(String, String)]) -> String {
    hidden
        .iter()
        .fold(text.to_owned(), |text, (own, stand_in)| {
            text.replace(own, stand_in)
        })
}

/// The agent's `text` with each placeholder put back as the address it
/// stands for.
fn shown(text: &str, hidden: &[(String, String)]) -> String {
    hidden
        .iter()
        .fold(text.to_owned(), |text, (own, stand_in)| {
            text.replace(stand_in, own)
        })
}

/// The journal's line for a make that took `ms`: kept, or not kept and
/// why, in the lamp's own plain words ([`Refused`]'s first part, never the
/// person's request, nor the check's lines with the model's files). On
/// lamp-74ff an app make "wasn't kept in 1848 ms" and the journal never
/// said why: it was only in the root-only `app.json` (#5977). The model's
/// own `cannot:` reason (`models_cannot`) may echo the person's words, so
/// only that it said so is journaled (Second's HOLD on #5985).
fn kept_line(made: &Result<Made, Refused>, models_cannot: bool, ms: u128) -> String {
    match made {
        Ok(_) => format!("lamp-voice: a skill made here was kept in {ms} ms"),
        Err(_) if models_cannot => format!(
            "lamp-voice: a skill made here wasn't kept in {ms} ms: {}",
            made_skill::MODELS_CANNOT_JOURNALED
        ),
        Err(Refused(why, _)) => {
            format!("lamp-voice: a skill made here wasn't kept in {ms} ms: {why}")
        }
    }
}

/// The longest the judge of a made skill with no word of the ask is waited
/// for (#7621): only on that path, the overlap's fast pass never waits.
pub const JUDGE_WITHIN: Duration = Duration::from_millis(1_500);

/// The most bytes of the judge's answer read, about 300 tokens.
const JUDGE_MOST_BYTES: usize = 1_200;

/// The judge's system prompt.
const JUDGE_SYSTEM: &str = "You check whether a voice assistant made the skill a person asked \
                            for. Answer yes or no, one word.";

/// The judge's answer to `question` ([`made_skill::judge_ask`]) from the
/// make's own chat, on a thread of its own, within [`JUDGE_WITHIN`]: yes,
/// no, unclear, a timeout or an error, never its words.
fn judged(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    question: String,
) -> made_skill::Judged {
    judge_text(chat, JUDGE_SYSTEM, question, JUDGE_WITHIN)
        .map(|text| made_skill::Judged::read(&text))
        .unwrap_or_else(|judged| judged)
}

/// The longest DeepSeek's judgement of whether a make is a quick reaction
/// is waited for ([`made_skill::REACTION_JUDGE`], the owner's "do it like
/// V1" on #7764): about a second is usual; past it, the make goes to the
/// lamp's agent, as for an unsure judgement.
pub const REACTION_JUDGE_WITHIN: Duration = Duration::from_millis(4_000);

/// DeepSeek's judgement of whether `request` is a quick reaction
/// ([`made_skill::REACTION_JUDGE`]), from the make's own chat within
/// [`REACTION_JUDGE_WITHIN`]: `None` when unsure, late or failed. Its
/// reason is never read; the journal has its time and its outcome alone.
fn reaction_judged(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    request: &str,
) -> Option<bool> {
    let began = std::time::Instant::now();
    let judged = judge_text(
        chat,
        made_skill::REACTION_JUDGE,
        made_skill::reaction_brief(request),
        REACTION_JUDGE_WITHIN,
    )
    .ok()
    .and_then(|text| made_skill::reaction_judged(&text));
    eprintln!(
        "lamp-voice: a skill made here: judged in {} ms: {}",
        began.elapsed().as_millis(),
        match judged {
            Some(true) => "a quick reaction",
            Some(false) => "not a quick reaction",
            None => "unsure or no answer",
        }
    );
    judged
}

/// The answer to `question` under `system` from the make's own chat, on a
/// thread of its own, within `within`: its text, or a timeout or an error.
fn judge_text(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    system: &'static str,
    question: String,
    within: Duration,
) -> Result<String, made_skill::Judged> {
    use made_skill::Judged;
    // Its answer by a channel, waited on for its bound and a little more:
    // never a join, which a DNS lookup hung on tokio's blocking pool holds
    // past the bound (the second adversarial read's P2). A thread past it
    // ends by itself; its answer is dropped.
    let (sent, answered) = std::sync::mpsc::sync_channel(1);
    let answer = std::thread::Builder::new()
        .name("lamp-voice-make-judge".into())
        .spawn(move || {
            let _ = sent.send(judge_once(chat, system, question, within));
        });
    match answer {
        Ok(_) => answered
            .recv_timeout(within + Duration::from_millis(100))
            .unwrap_or(Err(Judged::Timeout)),
        Err(_) => Err(Judged::Failed),
    }
}

/// One judge call within `within`, on its own runtime ([`judge_text`]).
fn judge_once(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    system: &str,
    question: String,
    within: Duration,
) -> Result<String, made_skill::Judged> {
    use made_skill::Judged;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Judged::Failed)?;
    let chat = chat().map_err(|_| Judged::Failed)?;
    runtime.block_on(async {
        tokio::time::timeout(within, async {
            let mut stream = chat
                .send(&[Message::system(system), Message::user(&question)])
                .await
                .map_err(|_| Judged::Failed)?;
            let mut text = String::new();
            while let Some(event) = stream.next().await.map_err(|_| Judged::Failed)? {
                if let ChatEvent::Text(words) = event {
                    text.push_str(&words);
                    if text.len() > JUDGE_MOST_BYTES {
                        break;
                    }
                }
            }
            Ok(text)
        })
        .await
        .unwrap_or(Err(Judged::Timeout))
    })
}

thread_local! {
    /// The name the person gave the skill, as the voice's model read it
    /// ([`GIVEN_NAME`], [`GivenName`]), for the make on this thread.
    static GIVEN: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The name the person gave a voice make ([`GIVEN_NAME`]), held while it
/// runs, then cleared, so no later make on this thread takes it.
struct GivenName;

impl GivenName {
    fn set(name: Option<String>) -> Self {
        GIVEN.with(|given| given.replace(name));
        Self
    }
}

impl Drop for GivenName {
    fn drop(&mut self) {
        GIVEN.with(|given| given.replace(None));
    }
}

/// The name the person gave a make [`answer`] makes (#7911): the call's
/// `args`' [`GIVEN_NAME`]; for an undo's (`undone`), which makes the
/// first ask again, the one given in that ask
/// ([`crate::make_core_offer::Turned::given`]), never the undo call's.
/// Trimmed; none when blank.
fn given_of(args: &Value, undone: Option<&crate::make_core_offer::Turned>) -> Option<String> {
    let given = match undone {
        Some(done) => done.given.as_deref(),
        None => args[GIVEN_NAME].as_str(),
    };
    given
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// [`answer`]'s `make`, run with the name the person gave it held for it
/// ([`GivenName`], [`given_of`]): the made skill's on every make path.
fn named_make<T>(given: Option<String>, make: impl FnOnce() -> T) -> T {
    let _given = GivenName::set(given);
    make()
}

thread_local! {
    /// The person's own line for the voice's make on this thread
    /// ([`OwnLine`]): `Some(None)` when it isn't known.
    static OWN_LINE: std::cell::RefCell<Option<Option<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// The person's own line for a voice make, held while it runs, then
/// cleared, so no later make on this thread is judged by it.
struct OwnLine;

impl OwnLine {
    fn set(line: Option<String>) -> Self {
        OWN_LINE.with(|own| own.replace(Some(line)));
        Self
    }
}

impl Drop for OwnLine {
    fn drop(&mut self) {
        OWN_LINE.with(|own| own.replace(None));
    }
}

/// The model's answer to `brief`, asked on a thread of its own with a
/// runtime of its own, under the make's [`system`] prompt, made on this
/// thread with this thread's switches, as is whether its `internet:` line
/// is taken ([`crate::make_no_cannot::net_by_agent`]).
fn ask(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    brief: String,
) -> Result<String, String> {
    let made = (system(), crate::make_no_cannot::net_by_agent());
    ask_on(chat, Err(made), brief)
}

/// [`ask`] on a lamp whose agent writes a web make as code granted `net`
/// ([`net_code_by`], #7742): the make's own prompt with
/// [`net_not_agent_task_told`] after [`net_told`].
fn ask_net_code(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    brief: String,
) -> Result<String, String> {
    let told = net_told();
    let system = system().replacen(&told, &format!("{told} {}", net_not_agent_task_told()), 1);
    ask_on(
        chat,
        Err((system, crate::make_no_cannot::net_by_agent())),
        brief,
    )
}

/// [`ask`], under `system` ([`written_with`]).
fn ask_in(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    system: String,
    brief: String,
) -> Result<String, String> {
    ask_on(chat, Ok(system), brief)
}

/// The model's answer to `brief` on a thread of its own: under `system`
/// ([`written_with`]), or the make's own prompt and `net` ([`written`]).
fn ask_on(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
    system: Result<String, (String, bool)>,
    brief: String,
) -> Result<String, String> {
    std::thread::Builder::new()
        .name("lamp-voice-make-skill".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| error.to_string())?;
            let chat = chat()?;
            runtime
                .block_on(async {
                    match &system {
                        Ok(system) => written_with(&chat, system, &brief).await,
                        Err((made, net)) => written(&chat, made, *net, &brief).await,
                    }
                })
                .map_err(|error| error.to_string())
        })
        .map_err(|error| error.to_string())
        .and_then(|thread| {
            thread
                .join()
                .unwrap_or_else(|_| Err("the model's thread failed".into()))
        })
}

/// What the model is told of a skill made here: on, kept and not on and
/// why, or not made and why, in plain words. On, it says exactly one short
/// line ([`made_skill::on_line`]); kept and not on, why
/// ([`made_skill::not_on_line`]). No ask follows either (the owner, 10 Nov:
/// a made skill goes on with no spoken yes).
///
/// `needs`: the linked services it reads that aren't linked now
/// ([`made_skill::needs_link`]): told as `needs_link`, and the model asked
/// to tell them after the line in its own words and the conversation's
/// language ([`made_skill::needs_link_ask`]), never a fixed sentence.
fn told(made: Result<Made, Refused>, on: Result<(), String>, needs: Option<&[Value]>) -> Value {
    told_with(said_line(&made, &on, needs), made, on, needs)
}

/// [`told`], its one short line `line`.
fn told_with(
    line: String,
    made: Result<Made, Refused>,
    on: Result<(), String>,
    needs: Option<&[Value]>,
) -> Value {
    match made {
        Ok(made) => {
            let link = needs
                .and_then(made_skill::needs_link_ask)
                .map(|ask| format!(" {ask}"))
                .unwrap_or_default();
            let mut told = json!({
                "made": made.name,
                "on": on.is_ok(),
                "description": made.description,
                "uses": made.uses.spoken(),
                "next": format!(
                    "{}{link} Ask nothing about putting it on.",
                    made_skill::in_their_language(&line)
                )
            });
            if let Err(why) = on {
                told["not_on"] = json!(why);
            }
            if let Some(needs) = needs.filter(|needs| !needs.is_empty()) {
                told["needs_link"] = json!(needs);
            }
            told
        }
        // Why, said as it is (#5977; the owner, 10 Nov: "phải nói rõ tại
        // sao không được"): the model's `cannot:` sentence with what to ask
        // instead, or the lamp's own plain reason.
        Err(Refused(why, _)) => json!({
            "error": format!("{NOT_MADE_HEAD}{why}"),
            "next": made_skill::not_made_in_their_language(&line)
        }),
    }
}

/// A call of [`made_skill::MAKE`], in the product's places: made, kept and
/// put on at once ([`make`]).
/// What a `make_skill` call in a turn nobody spoke (`heard` false: the
/// lamp's own event turn, a hand-over, or the model's own idea) is told:
/// nothing is made or kept (Thirteen's P2 on #5985: with no spoken yes, the
/// person's own spoken ask is the only consent on the voice path).
pub const NOT_ASKED_ALOUD: &str =
    "a skill is made only when the person asks for one out loud, or with MAKE IT in the app";

/// What a `make_skill` call with no `request` and no other argument it
/// takes is told (Heron's P3 on #6614: `request` is not required, so a
/// clash's choice can come alone): nothing is made.
pub const NO_REQUEST: &str =
    "nothing was made: a new skill needs request, what it should do and when";

pub fn answer(
    call: &lamp_gemini::ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
    skills: Option<&std::path::Path>,
) -> Value {
    // The person's choice on a made skill that clashes with one on the
    // lamp (#6593): only the owner's own later line, as for a yes.
    if let Some(id) = call.args[crate::made_clash::CLASH_ID].as_str() {
        return crate::made_clash::chosen(
            crate::made_clash::Answer {
                id,
                choice: call.args[crate::made_clash::CHOICE].as_str(),
                request,
                heard,
                here: crate::services::sends_here(),
            },
            || crate::services::owners_yes_now(request, heard, lease),
            std::time::Instant::now(),
            &crate::made_clash::helper_hands(),
        );
    }
    // The person's yes to a made skill's writes (#6066): the waiting
    // skill's, in a later heard line, approved by the voice itself.
    if let Some(id) = call.args[crate::skill_write_yes::APPROVE_ID].as_str() {
        // A voice make's id, told while it was made ([`making_note`]): a
        // no drops it; done since, its own wait takes the answer.
        let waits;
        let id = match making_answered(
            id,
            request,
            call.args[crate::skill_write_yes::ANSWER].as_str(),
        ) {
            Making::Told(told) => return told,
            Making::Waits(own) => {
                waits = own;
                waits.as_str()
            }
            Making::NotOurs => id,
        };
        // A no, an unclear answer (#6835 gap 2), or a yes that names
        // another skill than the one waiting (#7545): nothing is kept.
        if let Some(told) = crate::skill_write_yes::answered(
            id,
            request,
            call.args[crate::skill_write_yes::ANSWER].as_str(),
            call.args[crate::skill_write_yes::NAMED].as_str(),
        ) {
            return told;
        }
        // Every made skill's wait: only the owner's yes (#6307, #6558).
        if let Some(refused) = not_owners_yes_refused(id, || {
            crate::services::owners_yes_now(request, heard, lease)
        }) {
            return refused;
        }
        return crate::skill_write_yes::approve(
            id,
            request,
            heard,
            crate::services::sends_here(),
            std::time::Instant::now(),
            |asks| match (asks.code, &asks.grants) {
                // A remove (#6960): the made skill taken off by the manage
                // helper, then dropped from the made skills.
                _ if asks.remove => crate::made_remove::removed_here(asks),
                // Code of its own (#6307): that package put on, by its
                // SHA-256, then its task's yes recorded by lamp-control for
                // the folder as it went on; nothing is asked of
                // `lamp-services`.
                (true, _) => {
                    let keeping = Keeping::when(true).ok_or("the lamp can't make skills")?;
                    code_on_here(&keeping, asks)
                }
                // A skill's task (#6305): recorded by lamp-control, for
                // the folder and grants said back, as the manage helper
                // reads the skill again.
                (false, Some(_)) => crate::skill_task_yes::approve_by(
                    std::path::Path::new(made_skill::MANAGE_SOCKET),
                    std::path::Path::new(crate::skill_task_yes::CONTROL_SOCKET),
                    asks,
                ),
                (false, None) => crate::services::skill_write_approve(
                    std::path::Path::new(crate::services::FOLDER),
                    asks,
                ),
            },
        );
    }
    // A skill the person made, turned off or back on by voice (#7445,
    // `LAMP_MADE_SKILL_OFF_SAID`): read before the remove, so a call that
    // carries a turn-off is never a remove.
    if made_skill::off_said_on() {
        // A turn on with a run now in the same call (#7562's row on
        // lamp-9adf, 18 Nov, request 9) is the run now, which turns a skill
        // turned off on itself ([`crate::made_run::asked_turning`]).
        // One that names two skills (Heron's P3) does neither: each is asked
        // again as its own call, never one done and the other dropped.
        if let Some(on) = call.args[crate::made_turn::TURN_ON].as_str()
            && let Some(name) = call.args[crate::made_run::RUN].as_str()
        {
            if !crate::made_run::one_skill(on, name, Keeping::when(true).as_ref()) {
                return crate::made_run::two_skills(on, name);
            }
            return crate::made_run::asked_here(
                name,
                request,
                heard,
                lease,
                skills,
                // Whether the owner named it (#8475): the heard words' own
                // call, or the voice's record of their spoken asks.
                crate::tools::spoken_ask_here(call),
            );
        }
        for (key, off) in [
            (crate::made_turn::TURN_OFF, true),
            (crate::made_turn::TURN_ON, false),
        ] {
            if let Some(name) = call.args[key].as_str() {
                // Kept as this conversation's, or its candidates begun with
                // this conversation's (#7660, `crate::just_made`).
                return crate::just_made::after_turn(
                    crate::made_turn::asked_here(name, off, request, heard, lease),
                    request,
                    lease,
                );
            }
        }
    }
    // What was just done to a skill they made, undone (`LAMP_MAKE_NO_ASKS`):
    // the model's judgment of "remove it", "keep the old one" or "put it
    // back", never a list of words.
    if crate::make_no_asks::on()
        && let Some(asked) = call.args[crate::make_no_asks::UNDO].as_str()
    {
        // "Put it back" after a change, the latest thing done: its old
        // version back in place ([`crate::made_change::put_back`]).
        if let Some(name) = crate::made_change::put_back_for_undo_here(asked) {
            return crate::made_change::put_back_here(&name, request, heard, lease);
        }
        return crate::make_no_asks::undo_here(asked, request, heard, lease);
    }
    // A skill the person made, removed by voice (#6960): matched in code
    // to their own made skills, asked once by its name, removed on the yes.
    if let Some(name) = call.args[crate::made_remove::REMOVE].as_str() {
        return crate::made_remove::asked_here(name, request, heard, lease);
    }
    // The skills the person made, listed by voice: how many, and the
    // newest by name to the owner alone.
    if call.args[crate::made_list::LIST] == true {
        return crate::made_list::listed_here(request, heard, lease);
    }
    // A skill the person made, run once now by voice (lamp-9adf, 15 Nov:
    // "Run my unread mail skill now" was read back, and nothing ran): the
    // owner's alone, held to its trigger's bounds ([`crate::made_run`]).
    if let Some(name) = call.args[crate::made_run::RUN].as_str() {
        return crate::made_run::asked_here(
            name,
            request,
            heard,
            lease,
            skills,
            // Whether the owner named it (#8475): the heard words' own
            // call, or the voice's record of their spoken asks.
            crate::tools::spoken_ask_here(call),
        );
    }
    // A made skill read back by voice (`LAMP_SKILL_WRITES`): what it
    // sends, and its yes awaited again when it waits for one. Or a skill on
    // the lamp with a task (`LAMP_SKILL_TASKS`, #6305): what its script
    // uses, and its yes awaited.
    if let Some(name) = call.args[READ_BACK].as_str() {
        let (writes, tasks) = (made_skill::writes_on(), crate::skill_task_yes::on());
        if !writes && !tasks {
            return crate::tools::unavailable();
        }
        let Some(keeping) = Keeping::when(true) else {
            return crate::tools::unavailable();
        };
        made_skill::drop_gone(&keeping);
        let asking = Asking {
            request,
            heard,
            owner_hears: crate::services::owner_hears(request, heard, lease),
            sends_here: crate::services::sends_here(),
        };
        // A made skill's read-back, but an agent skill added from a file
        // (`LAMP_UPLOAD_TO_AGENT`), whose task is read back as any skill's.
        if tasks
            && made_skill::kept_on_named(&keeping.directory, name).is_none_or(|made| {
                made_skill::agent_grants(&keeping.directory, &made.sha256).is_some()
            })
            && let Some(told) = crate::skill_task_yes::read_back(
                name,
                asking,
                &made_skill::pushed(&keeping.installed),
                |asked| {
                    crate::skill_task_yes::asks_by(
                        &keeping.manage,
                        std::path::Path::new(crate::skill_task_yes::CONTROL_SOCKET),
                        asked,
                    )
                },
                std::time::Instant::now(),
            )
        {
            return told;
        }
        return read_back(
            name,
            asking,
            &keeping.directory,
            |asks| {
                crate::services::skill_writes_approved(
                    std::path::Path::new(crate::services::FOLDER),
                    asks,
                )
            },
            std::time::Instant::now(),
        );
    }
    if !heard {
        eprintln!(
            "lamp-voice: a skill wasn't made: no one asked for it aloud in this turn (#5985)"
        );
        return serde_json::json!({ "error": NOT_ASKED_ALOUD });
    }
    // The undo of a skill on the lamp turned on in place of a make (#7747,
    // `LAMP_MAKE_CORE_OFFER`): turned back off, then the first ask made.
    let undone = if crate::make_core_offer::on() && call.args[crate::make_core_offer::UNDO] == true
    {
        match crate::make_core_offer::undone_here(request) {
            Ok(done) => Some(done),
            Err(told) => return told,
        }
    } else {
        None
    };
    // `request` is not required, so a clash's choice can come alone
    // (#6593): a make with none, or a choice with no clash_id, makes
    // nothing and is told so plainly (Heron's P3 on #6614).
    if undone.is_none()
        && call.args["request"]
            .as_str()
            .is_none_or(|request| request.trim().is_empty())
    {
        eprintln!("lamp-voice: request {request}: a skill wasn't made: the call had no request");
        if crate::make_no_asks::on() {
            return serde_json::json!({
                "error": NO_REQUEST,
                "next": crate::make_no_asks::NOT_ACTED_NEXT
            });
        }
        return serde_json::json!({
            "error": NO_REQUEST,
            "next": "Ask them, in a few words, what the skill should do and when. Call nothing now."
        });
    }
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    let Some(keeping) = Keeping::when(true) else {
        return crate::tools::unavailable();
    };
    // A change to a skill the person made ([`crate::made_change`]): that
    // one re-made in place. Never after an undo.
    let changing = call.args[crate::made_change::CHANGE]
        .as_str()
        .map(str::trim)
        .filter(|asked| undone.is_none() && !asked.is_empty());
    // What a turned-off skill on the lamp already does turns it on, with
    // one line and its undo, never an offer (#7747, `LAMP_MAKE_CORE_OFFER`;
    // the first builder, 18 Nov 18:41). Never after an undo, nor for a
    // change, which changes the skill named.
    if undone.is_none()
        && changing.is_none()
        && let Some(turned) = crate::make_core_offer::before_make_here(&call.args, request, skills)
    {
        return turned;
    }
    let request_words = undone.as_ref().map_or_else(
        || call.args["request"].as_str().unwrap_or_default(),
        |done| done.request.as_str(),
    );
    // Whose ask it was, asked only for a make with code of its own
    // (`LAMP_MAKE_NO_ASKS`): the line's owner read now, the lease's, or
    // the one kept at the ask of a make that waited its turn (#7679).
    if crate::make_no_asks::on() {
        let owner = owner_for_make(lease);
        MAKE_OWNER.with(|asked| {
            *asked.borrow_mut() = Some(Box::new(move || {
                crate::services::owners_yes_of(request, heard, owner)
            }));
        });
    }
    // Whether the call gave the skill a name (#7926's row: none was kept
    // on lamp-9adf, so whether the model gave one is journaled).
    let given = given_of(&call.args, undone.as_ref());
    eprintln!(
        "lamp-voice: request {request}: a make {}",
        if given.is_some() {
            "with a name its person gave (given_name)"
        } else {
            "with no given_name"
        }
    );
    let told = match changing {
        Some(asked) => crate::made_change::changed(
            &keeping,
            asked,
            request_words,
            || crate::services::owners_yes_of(request, heard, owner_for_make(lease)),
            crate::make_no_asks::turned_off_here().as_ref(),
            |words| make_in(request, words, &keeping, own_chat, now_unix_ms),
            now_unix_ms,
        ),
        None => named_make(given, || {
            make_in(request, request_words, &keeping, own_chat, now_unix_ms)
        }),
    };
    MAKE_OWNER.with(|asked| asked.borrow_mut().take());
    // The skill made, kept as made in this conversation (#7660).
    keep_just_made(&told, request, lease);
    told
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// #7901, the settled design: the make is told no limit on what sits
    /// beside `net`, so a web skill keeps its own notes or uses the camera.
    #[test]
    fn the_make_is_told_no_grant_is_kept_from_beside_net() {
        let told = net_code_told();
        assert!(!told.contains("never sits beside"), "{told}");
        assert!(!told.contains("camera"), "{told}");
        // ee17's row at 5534888ee0 (leg A): told "net and speak" alone, the
        // agent made the coffee skill that keeps notes with net and speak
        // only; the other grants its code uses are named beside them.
        assert!(
            told.contains("beside any other grant its code uses"),
            "{told}"
        );
        assert!(told.contains("\"notes\" when it keeps"), "{told}");
    }

    /// #7679 (`LAMP_MAKE_QUEUE`): a make waiting its turn while a busy room
    /// talks on (13 more exchanges, past the thread's twelve) still runs
    /// with its own heard line, kept at the ask, never the newest words; and
    /// only while it runs.
    #[test]
    fn a_queued_make_runs_with_its_own_line_after_thirteen_more_exchanges() {
        const ASKED: &str = "check the gold price every evening at six";
        let mut thread = crate::thread::Thread::new();
        let start = std::time::Instant::now();
        thread.person(ASKED, true, start);
        thread.lamp(4, "I'll make the gold one next.", start);
        assert_eq!(own_line_for_make(4, &thread).as_deref(), Some(ASKED));
        for request in 5..19 {
            thread.person(
                &format!("what is the weather like, number {request}"),
                true,
                start,
            );
            thread.lamp(request, "Sunny.", start);
        }
        // The thread has lost it: none, or another line's words.
        assert_eq!(own_line_for_make(4, &thread), None);
        assert_ne!(heard_for_make(4, &thread).as_deref(), Some(ASKED));
        // Kept at the ask: its own line, for both readings.
        with_heard_kept(Some(ASKED.to_owned()), || {
            assert_eq!(heard_for_make(4, &thread).as_deref(), Some(ASKED));
            assert_eq!(own_line_for_make(4, &thread).as_deref(), Some(ASKED));
        });
        // Cleared once it ran: any other make reads the thread as before.
        assert_eq!(own_line_for_make(4, &thread), None);
    }

    /// #7679 with #7660: the owner's queued make, run with no lease, is kept
    /// as just made in the owner's conversation, so "the skill you just
    /// made" finds it; a guest's queued make is no one's.
    #[test]
    fn a_queued_owners_make_is_kept_as_their_just_made_skill() {
        if crate::person::private_room() {
            return;
        }
        made_skill::set_off_said_here(Some(true));
        let made = serde_json::json!({ "made": "gold-price", "on": true });
        assert_eq!(crate::just_made::kept_for_test("ann"), 0);
        with_asker_kept(None, None, || keep_just_made(&made, 7_679_301, None));
        assert_eq!(crate::just_made::kept_for_test("ann"), 0);
        with_asker_kept(None, Some("ann".into()), || {
            keep_just_made(&made, 7_679_302, None)
        });
        assert_eq!(crate::just_made::kept_for_test("ann"), 1);
        made_skill::set_off_said_here(None);
    }

    /// #7679 with `LAMP_MAKE_NO_ASKS`: a queued make runs with no lease, so
    /// its owner is the one read at its ask: the owner's own make is held
    /// to them (a make with code put on, a clash replacing their skill), a
    /// guest's to no one; only while it runs, and any other make reads its
    /// lease as before.
    #[test]
    fn a_queued_make_is_held_to_the_owner_read_at_its_ask() {
        // Outside the private room, no lease is no owner.
        if crate::person::private_room() {
            return;
        }
        assert_eq!(owner_for_make(None), None);
        let owners = with_asker_kept(Some("check gold at six".into()), Some("ann".into()), || {
            owner_for_make(None)
        });
        assert_eq!(owners.as_deref(), Some("ann"));
        let guests = with_asker_kept(Some("check gold at six".into()), None, || {
            owner_for_make(None)
        });
        assert_eq!(guests, None);
        // Cleared once it ran, a panic too.
        assert_eq!(owner_for_make(None), None);
        let _ = std::panic::catch_unwind(|| {
            with_asker_kept(None, Some("ann".into()), || panic!("the make failed"))
        });
        assert_eq!(owner_for_make(None), None);
    }

    /// Heron's P1 on #6558: a voice make's write waits for the owner's yes
    /// only. A visitor's "yes" approves nothing and drops the wait; the
    /// owner's approves; an id that isn't waiting is left to `approve`.
    // #7245: the notes switch off leaves the prompt byte for byte; on, the
    // model hears the notes calls and that a note is no file.
    #[test]
    fn notes_off_is_byte_for_byte_and_on_tells_the_notes_calls() {
        made_skill::set_scripts_here(Some(true));
        made_skill::set_notes_here(None);
        let before = system_for(false, false);
        made_skill::set_notes_here(Some(false));
        assert_eq!(system_for(false, false), before);
        assert_eq!(made_skill::script_sdk(), made_skill::SCRIPT_SDK);
        assert!(!before.contains("lamp.notes"), "{before}");
        made_skill::set_notes_here(Some(true));
        let on = system_for(false, false);
        for part in [
            "await lamp.notes.set(key, value)",
            "await lamp.notes.get(key)",
            "A note is no file the person opens",
            "unless its code calls lamp.notes.set",
            "never a file the person can open",
            // lamp-c0a4, 18 Nov 03:5x: OpenClaw answered `cannot:` twice,
            // its transcript saying "the SDK has no notes call" and taking
            // "when I say fruit notes" for hearing the room.
            "The SDK above has lamp.notes.set and lamp.notes.get",
            // lamp-c0a4, 18 Nov 10:04: OpenClaw answered `cannot: ...
            // writing to a file you can open` for "a file I can open".
            "made this way with a note, never answered cannot",
            // lamp-c0a4, 18 Nov: OpenClaw answered `cannot:` "knowing facts
            // about apples on my own" for three facts asked for.
            KNOWN_NOW,
            // Its own code and a note named among what a skill may do,
            // before the cannot rule, and checked before a cannot.
            "its hum, its own code (a script), a note it keeps, or",
            CHECK_STEPS,
        ] {
            assert!(on.contains(part), "{part}: {on}");
        }
        // Named words aren't hearing, said only with asks on (the
        // adversarial read's P1: off, the model is told nothing of asks).
        let heard = "is started by its asks, never by hearing the room";
        made_skill::set_asks_here(Some(false));
        assert!(!system_for(false, false).contains(heard));
        made_skill::set_asks_here(Some(true));
        let asks = system_for(false, false);
        assert!(asks.contains(heard));
        // The may-do list before the cannot rule; the room's talk no more
        // the cannot's example, which the model copied word for word.
        let may = asks.find("a note it keeps, or").unwrap();
        assert!(may < asks.find("A skill can't yet").unwrap(), "{asks}");
        assert!(may < asks.find(CHECK_STEPS).unwrap(), "{asks}");
        // The example with writes on (with them off, "sending a message").
        made_skill::set_writes_here(Some(true));
        let writes = system_for(false, false);
        made_skill::set_writes_here(None);
        assert!(writes.contains("I can't do that yet: a camera view from another room."));
        assert!(!writes.contains("I can't do that yet: hearing what is said in the room."));
        made_skill::set_asks_here(None);
        // Off, none of #7245's lines.
        made_skill::set_notes_here(Some(false));
        let off = system_for(false, false);
        for part in [KNOWN_NOW, CHECK_STEPS, "a note it keeps"] {
            assert!(!off.contains(part), "{part}: {off}");
        }
        assert!(
            off.contains("its hum, its own code (a script), or"),
            "{off}"
        );
        // The notes grant told only with notes on.
        assert!(!off.contains("microphone, notes"), "{off}");
        made_skill::set_notes_here(Some(true));
        assert!(system_for(false, false).contains("microphone, notes, speech"));
        assert!(
            made_skill::script_sdk().ends_with("await lamp.close(); last, asyncio.run(main())")
        );
        made_skill::set_notes_here(None);
        made_skill::set_scripts_here(None);
    }

    #[test]
    fn a_voice_made_writes_yes_is_the_owners_only() {
        use crate::skill_write_yes as yes;
        const SAID: &str = "mails mai@example.com from your Gmail, subject “Leaving”";
        let now = std::time::Instant::now();
        let asks = || yes::Asks {
            skill: "leaving-mail".to_owned(),
            sha256: "ab".repeat(32),
            writes: vec![json!({"service": "gmail", "action": "send_mail",
                "to": ["mai@example.com"], "subject": "Leaving", "text": "I'm leaving now."})],
            say_back: SAID.to_owned(),
            grants: None,
            code: false,
            remove: false,
        };
        let never = |_: &yes::Asks| -> Result<Value, String> { panic!("approved") };
        let id = yes::wait_for_yes(asks(), 4, now);
        yes::lamp_said(
            4,
            &format!("I made leaving mail: when you get up, it {SAID}."),
        );
        // A visitor's yes in request 5: refused, the wait dropped.
        assert_eq!(
            not_owners_yes_refused(&id, || false),
            Some(json!({ "error": crate::services::OTHER_VOICE }))
        );
        assert!(!yes::awaited(now));
        assert_eq!(
            yes::approve(&id, 5, true, true, now, never)["error"],
            yes::NOT_APPROVED
        );
        // The owner's yes: approved.
        let id = yes::wait_for_yes(asks(), 6, now);
        yes::lamp_said(
            6,
            &format!("I made leaving mail: when you get up, it {SAID}."),
        );
        assert_eq!(not_owners_yes_refused(&id, || true), None);
        let told = yes::approve(&id, 7, true, true, now, |_| {
            Ok(json!({"approved": 1, "public": []}))
        });
        assert_eq!(told["approved"], true, "{told}");
        // Nothing waiting: not asked whose it is.
        assert_eq!(not_owners_yes_refused(&id, || panic!("asked whose")), None);
    }

    /// V2 on #6413: the heard words, not the model, ask for a read-back.
    /// A question word, the skill's name said loosely, then what it
    /// reaches; asking to run it, or another question, asks none.
    #[test]
    fn the_heard_words_ask_what_a_skill_uses_by_its_name() {
        let names: Vec<String> = ["desk-check", "skill-2", "desk", "leaving-mail"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        for (heard, asked) in [
            ("what does desk check use?", Some("desk-check")),
            ("Hey Lamp, what does desk check use?", Some("desk-check")),
            ("What does my desk check skill need?", Some("desk-check")),
            ("what can desk check reach", Some("desk-check")),
            ("Which files does deskcheck access?", Some("desk-check")),
            ("what does desk check do with my mail", Some("desk-check")),
            ("What does skill two touch?", Some("skill-2")),
            (
                "what does my leaving mail skill send?",
                Some("leaving-mail"),
            ),
            ("what does desk use", Some("desk")),
            ("use desk check", None),
            ("run desk check", None),
            ("what time is it", None),
            ("what does desk check say?", None),
            ("what do you think of desk check", None),
            ("run desk check and tell me what it uses", None),
            ("what does the weather skill use", None),
            ("what does desk check use, and run it", None),
            ("what does desk check use? then use it", None),
            ("what time is it? then run desk check, I need it", None),
            ("what does desk check use and turn on desk check", None),
            ("which song should I play, desk check needs me", None),
        ] {
            assert_eq!(read_back_asked(heard, &names).as_deref(), asked, "{heard}");
        }
    }

    /// SubDarren's and the Reviewer-lead's HOLDs on #6619: the ask word
    /// comes right after the name, in the same clause, so a one-word name
    /// in an ordinary question, an ask word in another clause and a line
    /// that runs a skill ask none.
    #[test]
    fn ordinary_questions_and_mixed_lines_ask_no_read_back() {
        let names: Vec<String> = ["desk-check", "timer", "focus", "news", "music", "mail"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        for heard in [
            "what does desk check use, and run it",
            "what time is it? then run desk check, I need it",
            "what's the weather, and start the timer so I do my homework",
            "which song should I play, start focus, I need to concentrate",
            "what time does the news start, do you know?",
            "what music does she like",
            "which mail do I need to answer",
            "what mail do you need",
            "what does she do with the mail",
        ] {
            assert_eq!(read_back_asked(heard, &names), None, "{heard}");
        }
        for (heard, asked) in [
            ("what does desk check use", "desk-check"),
            ("what does the news skill need", "news"),
            ("what does the timer do with my mail", "timer"),
            ("which mail does my music skill send", "music"),
        ] {
            assert_eq!(
                read_back_asked(heard, &names).as_deref(),
                Some(asked),
                "{heard}"
            );
        }
        // Two names alike give none: the model is left the line.
        let alike: Vec<String> = vec!["desk-check".into(), "deskcheck".into()];
        assert_eq!(read_back_asked("what does desk check use", &alike), None);
    }

    /// The model's `use_skill` or `make_skill` read-back of the skill the
    /// words read back already is the same skill, however it is written.
    #[test]
    fn a_call_of_the_skill_read_back_is_the_same_skill_again() {
        let use_skill = crate::use_skill::NAME;
        assert!(same_skill_again(
            use_skill,
            &json!({"skill": "desk_check"}),
            "desk-check"
        ));
        assert!(same_skill_again(
            use_skill,
            &json!({"skill": "desk-check"}),
            "desk-check"
        ));
        assert!(same_skill_again(
            made_skill::MAKE,
            &json!({ READ_BACK: "my desk check skill" }),
            "desk-check"
        ));
        assert!(!same_skill_again(
            use_skill,
            &json!({"skill": "dance"}),
            "desk-check"
        ));
        assert!(!same_skill_again(
            made_skill::MAKE,
            &json!({"request": "desk check"}),
            "desk-check"
        ));
        assert!(!same_skill_again(
            "light_on",
            &json!({"skill": "desk-check"}),
            "desk-check"
        ));
    }

    /// Thirteen's P2 on #5985: with no spoken yes, a `make_skill` call in a
    /// turn nobody spoke (the lamp's own event turn, a hand-over, the
    /// model's own idea) makes and keeps nothing and puts nothing on; it is
    /// told why. A heard turn goes on to make it, as the tests below.
    #[test]
    fn a_make_in_a_turn_nobody_spoke_makes_and_puts_on_nothing() {
        let call = lamp_gemini::ToolCall {
            id: "c1".to_owned(),
            name: crate::made_skill::MAKE.to_owned(),
            args: serde_json::json!({"request": "Glow green when I come back to my desk."}),
        };
        assert_eq!(
            answer(&call, 1, false, None, None),
            serde_json::json!({"error": NOT_ASKED_ALOUD})
        );
    }

    /// Heron's P3 on #6614: with a clash's choice `request` is no longer
    /// required, so a plain make with none, or a choice with no clash_id,
    /// is refused with a plain line, and nothing is made.
    #[test]
    fn a_make_with_no_request_is_refused_plainly() {
        for args in [
            serde_json::json!({}),
            serde_json::json!({"request": "  "}),
            serde_json::json!({"choice": "keep"}),
        ] {
            let call = lamp_gemini::ToolCall {
                id: "c1".to_owned(),
                name: crate::made_skill::MAKE.to_owned(),
                args,
            };
            let told = answer(&call, 2, true, None, None);
            assert_eq!(told["error"], NO_REQUEST, "{told}");
            assert!(told["next"].is_string(), "{told}");
        }
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

    pub(crate) fn good() -> String {
        format!("```skill.toml\n{TOML}```\n\n```SKILL.md\n{CARD}```\n")
    }

    /// #7418: a make counts as under way from its work's start until it
    /// returns, and a make let run on (`long_make::let_go`) no longer
    /// counts: its person was told they'd hear when it's ready.
    #[test]
    fn a_make_let_run_on_is_no_longer_under_way() {
        const MADE_IN: u64 = 7_418_003;
        let (root, keeping) = keeping("under-way-let-go");
        std::fs::create_dir_all(&keeping.directory).unwrap();
        assert!(!making(MADE_IN));
        let under_way = MakeUnderWay::begin(MADE_IN);
        assert!(making(MADE_IN));
        crate::long_make::begin(MADE_IN, &keeping.directory);
        assert!(crate::long_make::let_go(MADE_IN, NOW));
        assert!(!making(MADE_IN), "let run on: not thought on");
        crate::long_make::settle(MADE_IN);
        drop(under_way);
        assert!(!making(MADE_IN));
        // Another make of the same request, not let go, counts again.
        let again = MakeUnderWay::begin(MADE_IN);
        assert!(making(MADE_IN));
        drop(again);
        assert!(!making(MADE_IN));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A folder of the test's own, with a stand-in for `lamp-skills pack`
    /// that packs whatever it is given.
    pub(crate) fn keeping(name: &str) -> (PathBuf, Keeping) {
        let root = std::env::temp_dir().join(format!("make-local-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let program = root.join("pack");
        std::fs::write(
            &program,
            "#!/bin/sh\nname=$(basename \"$4\")\nprintf 'tar of %s' \"$name\" > \"$3/$name-1.0.0.tar\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let keeping = Keeping {
            directory: root.join("made"),
            program,
            manage: root.join("manage.sock"),
            installed: root.join("skills"),
        };
        (root, keeping)
    }

    // The URL key distinguishes concurrent fixtures, not later reuse of a port.
    fn fixture_key(url: &str) -> String {
        format!("make-skill-fixture-{url}")
    }

    type FixtureRead = Result<Option<(tokio::net::TcpStream, Vec<u8>)>, String>;

    async fn fixture_request(mut socket: tokio::net::TcpStream, key: String) -> FixtureRead {
        use tokio::io::AsyncReadExt;
        const CAP: usize = 64 * 1024;
        // Once authenticated, truncation/timeout must fail the fixture visibly.
        let mut authenticated = false;
        let read = async {
            let mut bytes = Vec::new();
            let mut head_end = None;
            let mut length = None;
            loop {
                if let (Some(end), Some(len)) = (head_end, length)
                    && bytes.len() >= end + len
                {
                    return Ok(Some(bytes[end..end + len].to_vec()));
                }
                if bytes.len() == CAP {
                    return if authenticated {
                        Err("authenticated chat exceeds request cap".into())
                    } else {
                        Ok(None)
                    };
                }
                let mut buffer = [0u8; 4096];
                let room = buffer.len().min(CAP - bytes.len());
                let n = match socket.read(&mut buffer[..room]).await {
                    Ok(n) => n,
                    Err(error) => {
                        return if authenticated {
                            Err(format!("authenticated chat read failed: {error}"))
                        } else {
                            Ok(None)
                        };
                    }
                };
                if n == 0 {
                    return if authenticated {
                        Err("authenticated chat body is incomplete".into())
                    } else {
                        Ok(None)
                    };
                }
                bytes.extend_from_slice(&buffer[..n]);
                if head_end.is_some() {
                    continue;
                }
                let Some(boundary) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
                    continue;
                };
                let end = boundary + 4;
                let head = match std::str::from_utf8(&bytes[..boundary]) {
                    Ok(head) => head,
                    Err(_) => return Ok(None),
                };
                let mut lines = head.split("\r\n");
                if lines.next() != Some("POST /v1/chat/completions HTTP/1.1") {
                    return Ok(None);
                }
                let headers: Vec<_> = lines.collect();
                let expected = format!("Bearer {key}");
                let authorization: Vec<_> = headers
                    .iter()
                    .filter_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("authorization")
                            .then_some(value.trim())
                    })
                    .collect();
                if authorization.as_slice() != [expected.as_str()] {
                    return Ok(None);
                }
                authenticated = true;
                let mut content_length = None;
                for line in headers {
                    let (name, value) = line
                        .split_once(':')
                        .ok_or("malformed authenticated header")?;
                    if name.eq_ignore_ascii_case("transfer-encoding") {
                        return Err("authenticated chat uses unsupported transfer encoding".into());
                    }
                    if name.eq_ignore_ascii_case("content-length") {
                        if content_length.is_some() {
                            return Err("duplicate authenticated content length".into());
                        }
                        let value = value.trim();
                        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                            return Err("invalid authenticated content length".into());
                        }
                        content_length = Some(
                            value
                                .parse::<usize>()
                                .map_err(|_| "authenticated content length overflow")?,
                        );
                    }
                }
                let len = content_length.ok_or("authenticated chat has no content length")?;
                if end.checked_add(len).is_none_or(|total| total > CAP) {
                    return Err("authenticated chat exceeds request cap".into());
                }
                head_end = Some(end);
                length = Some(len);
            }
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), read).await;
        match result {
            Ok(Ok(Some(body))) => Ok(Some((socket, body))),
            Ok(Ok(None)) => Ok(None),
            Ok(Err(error)) => Err(error),
            Err(_) if authenticated => Err("authenticated chat read deadline expired".into()),
            Err(_) => Ok(None),
        }
    }

    /// A scripted reply's status for a stream that breaks: its words, then
    /// the connection closed with no `finish_reason` and no `[DONE]`.
    pub(crate) const BROKEN: u16 = 0;
    /// A scripted reply's status for an answer the service stopped at its
    /// cap on tokens: `finish_reason` `length`, then `[DONE]`.
    pub(crate) const STOPPED_AT_LENGTH: u16 = 1;

    /// Only this fixture's authenticated chat requests consume scripted replies.
    /// Status 200 is a stream that ends whole, [`BROKEN`] one cut and
    /// [`STOPPED_AT_LENGTH`] one at the service's cap.
    /// Foreign probes have bounded readers; malformed authenticated chats fail.
    pub(crate) fn service(replies: Vec<(u16, String)>) -> (String, std::thread::JoinHandle<usize>) {
        let (url, served) = service_sent(replies);
        let count = std::thread::spawn(move || served.join().unwrap().len());
        (url, count)
    }

    /// [`service`], giving back each authenticated chat's body as sent.
    pub(crate) fn service_sent(
        replies: Vec<(u16, String)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{address}/v1/");
        let key = fixture_key(&url);
        let served = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                use tokio::io::AsyncWriteExt;
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let mut readers = tokio::task::JoinSet::<FixtureRead>::new();
                let mut sent = Vec::new();
                for (status, text) in replies {
                    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
                    let (mut socket, body) = loop {
                        tokio::select! {
                            biased;
                            _ = tokio::time::sleep_until(deadline) => panic!("no authenticated skill chat within 10 seconds"),
                            completed = readers.join_next(), if !readers.is_empty() => {
                                let read = completed.expect("reader present").expect("fixture reader panicked").expect("invalid authenticated skill chat");
                                if let Some(chat) = read { break chat; }
                            }
                            accepted = listener.accept(), if readers.len() < 16 => {
                                let (socket, _) = accepted.unwrap();
                                readers.spawn(fixture_request(socket, key.clone()));
                            }
                        }
                    };
                    let body = std::str::from_utf8(&body).expect("authenticated chat body is UTF-8");
                    sent.push(body.to_owned());
                    // A make's brief, its judge's question (#7621), or the
                    // judgement of a quick reaction (#7764).
                    assert!(
                        body.contains(made_skill::BRIEF_HEAD)
                            || body.contains("Does the made skill do what was asked?")
                            || body.contains("You judge, for a desk lamp"),
                        "{body}"
                    );
                    assert!(!body.contains("\"tools\""), "{body}");
                    let reply = if [200, BROKEN, STOPPED_AT_LENGTH].contains(&status) {
                        let (first, rest) = text.split_at(text.len() / 2);
                        let chunk = |words: &str| format!("data: {}\n\n", json!({"choices": [{"delta": {"content": words}}]}));
                        let end = match status {
                            BROKEN => String::new(),
                            STOPPED_AT_LENGTH => format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices": [{"delta": {}, "finish_reason": "length"}]})),
                            _ => "data: [DONE]\n\n".to_owned(),
                        };
                        format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{}{}{end}", chunk(first), chunk(rest))
                    } else {
                        format!("HTTP/1.1 {status} Nope\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    };
                    tokio::time::timeout(std::time::Duration::from_secs(2), socket.write_all(reply.as_bytes())).await.expect("fixture response deadline expired").expect("fixture response write failed");
                }
                // Match the existing script lifetime. Pending connections do not
                // extend it; this does not claim to detect late extra chats.
                readers.abort_all();
                while let Some(result) = readers.join_next().await {
                    match result {
                        Err(error) if error.is_cancelled() => {}
                        Err(error) => panic!("fixture reader panicked: {error}"),
                        Ok(Err(error)) => panic!("invalid authenticated skill chat: {error}"),
                        Ok(Ok(_)) => {}
                    }
                }
                sent
            })
        });
        (url, served)
    }

    #[test]
    fn discovery_does_not_consume_the_skill_chat_reply() {
        let (root, keeping) = keeping("discovery-control");
        let (url, served) = service(vec![(200, good())]);
        let address = url
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches("/v1/");
        let mut discovery = std::net::TcpStream::connect(address).unwrap();
        discovery
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        discovery
            .write_all(b"GET /v1/models HTTP/1.1\r\nHost: fixture\r\n\r\n")
            .unwrap();
        let mut response = Vec::new();
        let _ = discovery.read_to_end(&mut response);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        let joined = served.join();
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            joined.is_ok(),
            "discovery consumed or crashed the chat plan"
        );
        assert_eq!(joined.unwrap(), 1);
        assert_eq!(told["made"], "evening-dim", "{told}");
    }

    fn fixture_socket(url: &str) -> std::net::TcpStream {
        let address = url
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches("/v1/");
        let socket = std::net::TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        socket
            .set_write_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        socket
    }

    fn fixture_exchange(url: &str, request: &str) -> Vec<u8> {
        let mut socket = fixture_socket(url);
        socket.write_all(request.as_bytes()).unwrap();
        socket.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = Vec::new();
        match socket.read_to_end(&mut response) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            Err(error) => panic!("fixture response failed: {error}"),
        }
        response
    }

    fn fixture_chat(url: &str, body: &str) -> String {
        format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: fixture\r\naUtHoRiZaTiOn: Bearer {}\r\nContent-Length: {}\r\n\r\n{body}",
            fixture_key(url),
            body.len()
        )
    }

    #[test]
    fn foreign_and_idle_requests_leave_genuine_duplicate_chats_counted() {
        let (url, served) = service(vec![(200, "First.".into()), (200, "Second.".into())]);
        let idle = fixture_socket(&url);
        let mut partial = fixture_socket(&url);
        partial.write_all(b"GET /v1/models HTTP/1.1\r\n").unwrap();
        for (path, key) in [
            ("GET /v1/models", fixture_key(&url)),
            ("POST /wrong-path", fixture_key(&url)),
            ("POST /v1/chat/completions", "foreign-key".into()),
        ] {
            let request = format!(
                "{path} HTTP/1.1\r\nHost: fixture\r\nAuthorization: Bearer {key}\r\nContent-Length: 0\r\n\r\n"
            );
            assert!(fixture_exchange(&url, &request).is_empty());
        }
        let body =
            json!({"messages": [{"role": "user", "content": made_skill::BRIEF_HEAD}]}).to_string();
        let request = fixture_chat(&url, &body);
        let began = std::time::Instant::now();
        let first = fixture_exchange(&url, &request);
        let second = fixture_exchange(&url, &request);
        eprintln!(
            "two authenticated chats with idle/partial probes: {:?}",
            began.elapsed()
        );
        // Each identical genuine request consumes its own scripted reply.
        assert!(String::from_utf8(first).unwrap().contains("Fir"));
        assert!(String::from_utf8(second).unwrap().contains("Sec"));
        assert_eq!(served.join().unwrap(), 2);
        drop((idle, partial));
    }

    #[test]
    fn authenticated_bad_chat_bodies_still_fail_the_fixture() {
        for body in [
            "{}".to_owned(),
            format!("{} \"tools\"", made_skill::BRIEF_HEAD),
        ] {
            let (url, served) = service(vec![(200, "Unused.".into())]);
            assert!(fixture_exchange(&url, &fixture_chat(&url, &body)).is_empty());
            assert!(
                served.join().is_err(),
                "invalid authenticated body was ignored"
            );
        }
    }

    #[test]
    fn authenticated_bad_framing_still_fails_the_fixture() {
        for headers in [
            "",
            "Content-Length: nope\r\n",
            "Content-Length: 1\r\nContent-Length: 1\r\n",
            "Content-Length: 99999999999999999999999999999\r\n",
            "Content-Length: 65536\r\n",
            "Content-Length: 100\r\n",
            "Transfer-Encoding: chunked\r\n",
        ] {
            let (url, served) = service(vec![(200, "Unused.".into())]);
            let request = format!(
                "POST /v1/chat/completions HTTP/1.1\r\nHost: fixture\r\nAuthorization: Bearer {}\r\n{headers}\r\nx",
                fixture_key(&url)
            );
            assert!(fixture_exchange(&url, &request).is_empty());
            assert!(
                served.join().is_err(),
                "invalid authenticated framing was ignored: {headers:?}"
            );
        }
    }

    #[test]
    fn authenticated_body_timeout_still_fails_the_fixture() {
        let (url, served) = service(vec![(200, "Unused.".into())]);
        let mut socket = fixture_socket(&url);
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: fixture\r\nAuthorization: Bearer {}\r\nContent-Length: 100\r\n\r\nx",
            fixture_key(&url)
        );
        socket.write_all(request.as_bytes()).unwrap();
        // Keep the write half open: the reader must expire, not rely on EOF.
        let mut response = Vec::new();
        socket.read_to_end(&mut response).unwrap();
        assert!(response.is_empty());
        assert!(served.join().is_err(), "authenticated timeout was ignored");
    }

    /// How long a test helper waits for the call it stands in for.
    pub(crate) const ACCEPT_WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

    /// A test helper's accept, bounded by [`ACCEPT_WITHIN`]: a make that
    /// never calls the helper (its chat ran out under a loaded host) fails
    /// its test plainly, never hangs its join with the helper still in
    /// `accept` (Heron's hour at 0% CPU on #8180, sampled: two joins, seven
    /// helpers in `accept`).
    pub(crate) fn accept_within(
        listener: &std::os::unix::net::UnixListener,
    ) -> std::os::unix::net::UnixStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + ACCEPT_WITHIN;
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    // An accepted socket keeps the listener's non-blocking
                    // mode on macOS: the helper reads it blocking.
                    stream.set_nonblocking(false).unwrap();
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "no call to the test helper's socket within {} s",
                        ACCEPT_WITHIN.as_secs()
                    );
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => panic!("test helper accept: {error}"),
            }
        }
    }

    /// [`accept_within`] for a stub chat on TCP: the same bound, so a make
    /// that never calls it fails plainly instead of hanging its join.
    pub(crate) fn accept_within_tcp(listener: &std::net::TcpListener) -> std::net::TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + ACCEPT_WITHIN;
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    return stream;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "no call to the test chat's socket within {} s",
                        ACCEPT_WITHIN.as_secs()
                    );
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(error) => panic!("test chat accept: {error}"),
            }
        }
    }

    /// A manage helper at `socket` that puts on whatever made skill it is
    /// asked for, once: the line it was asked.
    pub(crate) fn manage_helper(socket: &Path) -> std::thread::JoinHandle<Value> {
        use std::io::BufRead;
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let stream = accept_within(&listener);
            let mut line = String::new();
            std::io::BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap();
            writeln!(&stream, r#"{{"done": "installed"}}"#).unwrap();
            let asked: Value = serde_json::from_str(&line).unwrap();
            assert!(asked["install_made"].is_string(), "{asked}");
            asked
        })
    }

    pub(crate) fn chat_at(
        url: String,
    ) -> impl Fn() -> Result<Chat, String> + Send + Sync + 'static {
        let key = fixture_key(&url);
        move || Chat::with(&url, "m", &key, Arc::new(Vec::new)).map_err(|error| error.to_string())
    }

    const REQUEST: &str = "make me a skill that dims the light at 10 pm";

    /// #7621: the line heard on c0a4, transcribed whole.
    pub(crate) const FOURTEEN: &str = "Lamp, make a skill: every day at four fifteen in the morning, look \
                            up the title of the example dot com web page and tell me what it says.";
    /// The moon-time DeepSeek made from an old ask in `request`.
    pub(crate) const MOON_TIME: &str = "```skill.toml\nasks = [\"moon time\"]\nkind = \"installable\"\n\
                             version = \"1.0.0\"\n[permissions]\nspeak = true\n[[do]]\nsay = { \
                             text = \"The moon is moving away from Earth, a little more each \
                             year.\" }\n```\n\n```SKILL.md\n---\nname: moon-time\ndescription: \
                             A moon fact.\n---\n\n# Moon time\n```\n";

    /// What a voice make with Fourteen's line should have asked.
    pub(crate) const FOURTEEN_ASK: &str = "every day at 4:15 in the morning, look up the title of the \
                                example.com web page and tell me what it says";

    /// #7621, its switch on: moon-time, with no word of Fourteen's line, is
    /// judged in one more call of the make's chat; its no refuses it with
    /// no repair and no agent, nothing kept or put on, and the voice model
    /// is told to say exactly the ask-again line, never the skill's name.
    #[test]
    fn a_make_judged_not_what_was_asked_is_asked_again_with_no_repair() {
        made_skill::set_unrelated_here(Some(true));
        made_skill::set_english_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let (root, keeping) = keeping("unrelated-judged");
        let (url, served) =
            service_sent(vec![(200, MOON_TIME.to_owned()), (200, "No.".to_owned())]);
        let told = make_with(
            7,
            Some(FOURTEEN),
            FOURTEEN_ASK,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        let sent = served.join().unwrap();
        assert_eq!(sent.len(), 2, "the make and its judge alone: {told}");
        assert!(sent[1].contains("Does the made skill do what was asked?"));
        assert_eq!(told["error"], made_skill::UNRELATED, "{told}");
        assert_eq!(
            told["next"],
            format!(
                "Say exactly this and nothing else: {}",
                made_skill::UNRELATED
            )
        );
        assert!(told.get("made").is_none(), "{told}");
        assert!(!told.to_string().to_lowercase().contains("moon"), "{told}");
        // Nothing made: a failure, never told after a cut once the person
        // went on (`worker::made_running`).
        assert_eq!(
            take_said_outcome(),
            Some((made_skill::UNRELATED.to_owned(), Outcome::Failed))
        );
        assert!(made_skill::entries(&keeping.directory).is_empty());
        made_skill::set_asks_here(None);
        made_skill::set_english_here(None);
        made_skill::set_unrelated_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7621: what the judge adds to a make, printed: an answer at once
    /// costs one short call; one that never comes is cut at
    /// [`JUDGE_WITHIN`] and read as a timeout. (The overlap's fast pass,
    /// which never calls it, is timed in `made_skill`'s tests.)
    #[test]
    fn the_judges_added_latency_is_bounded() {
        let question = made_skill::judge_ask("look up the example page's title", "moon-time");
        let (url, served) = service(vec![(200, "No.".to_owned())]);
        let began = std::time::Instant::now();
        let judged_now = judged(Arc::new(chat_at(url)), question.clone());
        let at_once = began.elapsed();
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(judged_now, made_skill::Judged::No);
        // A chat that takes the call and never answers.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/", listener.local_addr().unwrap());
        let held = std::thread::spawn(move || {
            let socket = accept_within_tcp(&listener);
            std::thread::sleep(JUDGE_WITHIN + Duration::from_secs(1));
            drop(socket);
        });
        let began = std::time::Instant::now();
        let judged_late = judged(Arc::new(chat_at(url)), question);
        let at_bound = began.elapsed();
        held.join().unwrap();
        assert_eq!(judged_late, made_skill::Judged::Timeout);
        println!(
            "#7621 judge latency: answered at once {} ms, at its bound {} ms (bound {} ms)",
            at_once.as_millis(),
            at_bound.as_millis(),
            JUDGE_WITHIN.as_millis()
        );
        assert!(at_once < JUDGE_WITHIN, "{at_once:?}");
        assert!(
            at_bound >= JUDGE_WITHIN && at_bound < JUDGE_WITHIN + Duration::from_millis(500),
            "{at_bound:?}"
        );
    }

    /// #7621: the same make with the judge failing (an error answer) is
    /// kept and put on; with the switch off or the lamp not in English it
    /// is made with no judge at all, exactly as before.
    #[test]
    fn a_failed_judge_or_its_switch_off_makes_as_before() {
        made_skill::set_asks_here(Some(true));
        for (index, (on, english, replies)) in [
            (
                true,
                true,
                vec![(200, MOON_TIME.to_owned()), (500, "busy".to_owned())],
            ),
            (false, true, vec![(200, MOON_TIME.to_owned())]),
            (true, false, vec![(200, MOON_TIME.to_owned())]),
        ]
        .into_iter()
        .enumerate()
        {
            made_skill::set_unrelated_here(Some(on));
            made_skill::set_english_here(Some(english));
            let (root, keeping) = keeping(&format!("unrelated-off-{index}"));
            let calls = replies.len();
            let (url, served) = service(replies);
            let asked = manage_helper(&keeping.manage);
            let told = make_with(
                7,
                Some(FOURTEEN),
                FOURTEEN_ASK,
                &keeping,
                chat_at(url),
                NOW,
                || None,
            );
            assert_eq!(served.join().unwrap(), calls, "{index}: {told}");
            assert_eq!(told["made"], "moon-time", "{index}: {told}");
            assert_eq!(told["on"], true, "{index}: {told}");
            asked.join().unwrap();
            let _ = take_said_line();
            let _ = std::fs::remove_dir_all(&root);
        }
        made_skill::set_asks_here(None);
        made_skill::set_english_here(None);
        made_skill::set_unrelated_here(None);
    }
    const NOW: u64 = 1_800_000_000_000;

    /// Whether any kept skill in `directory` is on.
    fn any_on(directory: &Path) -> bool {
        made_skill::entries(directory)
            .iter()
            .any(|entry| entry["on"] == true)
    }

    /// The model's two files are checked, packed and kept, and the skill
    /// goes on at once through the manage helper, with no spoken yes (the
    /// owner, 10 Nov): the model is told to say one short line, that it's
    /// on, and to ask nothing.
    #[test]
    fn a_good_answer_is_kept_and_put_on_with_no_yes() {
        let (root, keeping) = keeping("good");
        let (url, served) = service(vec![(200, good())]);
        let asked = manage_helper(&keeping.manage);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(told["on"], true, "{told}");
        // The owner, 10 Nov ("đồng bộ luôn"): its content, told in the
        // conversation's language, never "Say exactly" in English.
        assert_eq!(
            told["next"],
            format!(
                "{} Ask nothing about putting it on.",
                made_skill::in_their_language(
                    "I made evening dim, and it's on: when the lamp goes quiet, between 22:00 and 22:30, it dims the light a step, at most once every 12 hours, using its light and whether it is talking. It starts on its own, at most 3 times a day, one an hour with the lamp's other nudges, and rests till morning if ignored twice."
                )
            )
        );
        assert!(!told["next"].as_str().unwrap().contains("Say exactly"));
        // The same line, as written, for a turn cut while it was made
        // (lamp-9adf, 11 Nov): taken once, and exactly the line the model is
        // told to say, whatever a later sentence (its manners) adds to it.
        let said = take_said_line().expect("the made line is left for a cut turn");
        assert!(
            said.starts_with("I made evening dim, and it's on: "),
            "{said}"
        );
        assert_eq!(
            told["next"],
            format!(
                "{} Ask nothing about putting it on.",
                made_skill::in_their_language(&said)
            )
        );
        assert_eq!(take_said_line(), None);
        assert!(!told.to_string().contains("Shall I"), "{told}");
        assert!(told["uses"].as_str().unwrap().contains("light"), "{told}");
        let entry = &made_skill::entries(&keeping.directory)[0];
        assert_eq!(entry["on"], true);
        assert_eq!(asked.join().unwrap()["install_made"], entry["sha256"]);
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Fifth's row on #6249 (lamp-52e6, 11 Nov): a hello skill was made
    /// with a 22:00 to 22:30 window, a glow and a nod nobody asked for, the
    /// brief's own shown values. Two makes in one conversation: the second
    /// model is sent its own ask alone, in a brief with no time, gesture or
    /// words of anyone's, and the same fixed system words as the first;
    /// nothing of the first make, its window or its colour goes with it.
    #[test]
    fn a_second_make_is_sent_only_its_own_ask() {
        let (root, keeping) = keeping("this-ask");
        let first_toml = "kind = \"installable\"\nversion = \"1.0.0\"\n\n[permissions]\n\
            topics = [\"room/presence\"]\nlight = true\n\n[trigger]\ntopic = \"room/presence\"\n\
            field = \"someone\"\nequals = true\ncooldown = \"1h\"\n\
            during = { from = \"22:00\", to = \"23:00\" }\n\n[ask]\npolicy = \"nudge\"\n\n\
            [[do]]\nlight = { colour = \"blue\", level = 3, for = \"20s\" }\n";
        let second_toml = "kind = \"installable\"\nversion = \"1.0.0\"\n\n[permissions]\n\
            topics = [\"room/stood_up\"]\nbody = true\n\n[trigger]\ntopic = \"room/stood_up\"\n\
            field = \"sitting_s\"\nabove = 0\ncooldown = \"1h\"\n\n[ask]\npolicy = \"nudge\"\n\n\
            [[do]]\nbody = { gesture = \"nod\" }\n";
        let answer = |toml: &str, name: &str| {
            format!(
                "```skill.toml\n{toml}```\n\n```SKILL.md\n---\nname: {name}\n\
                 description: {name}\n---\n\n# {name}\n```\n"
            )
        };
        let (url, served) = service_sent(vec![
            (200, answer(first_toml, "blue-at-ten")),
            (200, answer(second_toml, "nod-on-standing")),
        ]);
        let helper = manage_helper(&keeping.manage);
        let first_ask = "glow blue at 10 pm when I'm at my desk";
        let second_ask = "nod when I stand up";
        let first = make(first_ask, &keeping, chat_at(url.clone()), NOW);
        assert_eq!(first["made"], "blue-at-ten", "{first}");
        assert!(first["next"].as_str().unwrap().contains("22:00"), "{first}");
        let _ = take_said_line();
        helper.join().unwrap();
        std::fs::remove_file(&keeping.manage).unwrap();
        let helper = manage_helper(&keeping.manage);
        let second = make(second_ask, &keeping, chat_at(url), NOW + 60_000);
        assert_eq!(second["made"], "nod-on-standing", "{second}");
        assert_eq!(second["on"], true, "{second}");
        helper.join().unwrap();
        // What it does, before the lamp's own manners ("never from 22:00 to
        // 08:00"), which every made skill has.
        let said = second["next"].as_str().unwrap();
        let does = said.split(" It starts on its own").next().unwrap();
        assert!(does.contains("it nods"), "{said}");
        assert!(!does.contains("22:") && !does.contains("blue"), "{said}");
        let _ = take_said_line();
        let sent = served.join().unwrap();
        assert_eq!(sent.len(), 2);
        let messages = |body: &str| {
            let body: Value = serde_json::from_str(body).unwrap();
            let messages = body["messages"].as_array().unwrap().clone();
            assert_eq!(messages.len(), 2, "{body}");
            let content = |at: usize| messages[at]["content"].as_str().unwrap().to_owned();
            (content(0), content(1))
        };
        let (first_system, first_brief) = messages(&sent[0]);
        let (second_system, second_brief) = messages(&sent[1]);
        assert_eq!(first_brief, made_skill::brief(first_ask).unwrap());
        assert_eq!(second_brief, made_skill::brief(second_ask).unwrap());
        assert_eq!(second_system, first_system);
        for earlier in ["22:", "10 pm", "blue", "I'm at my desk", "blue-at-ten"] {
            assert!(!second_brief.contains(earlier), "{earlier}: {second_brief}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Lamp-9adf, 12 Nov 15:13: request 3's turn was cut while its
    /// `make_skill` ran, its words were carried into request 4 (heard with
    /// no words of its own), and the model called `make_skill` again with
    /// its own wording: one skill is made and put on, and the second call
    /// is told it is had already, nothing asked of the model or the manage
    /// helper again.
    #[test]
    fn a_cut_ask_carried_into_the_next_line_makes_one_skill() {
        let (root, keeping) = keeping("one-ask");
        let (url, served) = service(vec![(200, good())]);
        let url_gone = url.clone();
        let asked = manage_helper(&keeping.manage);
        let heard = "make me a skill: when I come back to my desk, blink the light green twice.";
        let first = make_with(
            3,
            Some(heard),
            "blink the light green twice when I come back to my desk",
            &keeping,
            chat_at(url.clone()),
            NOW,
            || None,
        );
        assert_eq!(first["made"], "evening-dim", "{first}");
        assert_eq!(first["on"], true, "{first}");
        let _ = take_said_line();
        asked.join().unwrap();
        // Request 4: the carried line, its lamp's name before it, and the
        // model's request in other words.
        let carried = format!("Hey Lamp, {heard}");
        let second = make_with(
            4,
            Some(&carried),
            "when I return to my desk, blink green two times",
            &keeping,
            chat_at(url),
            NOW + 9_000,
            || None,
        );
        assert_eq!(second["already_made"], true, "{second}");
        assert_eq!(second["made"], "evening-dim", "{second}");
        assert!(
            second["next"]
                .as_str()
                .unwrap()
                .contains("You already have that skill: evening dim. It's on."),
            "{second}"
        );
        assert_eq!(take_said_line(), None, "nothing new to say for a cut turn");
        assert_eq!(served.join().unwrap(), 1, "the model was asked once");
        assert_eq!(made_skill::entries(&keeping.directory).len(), 1);
        // Past the window, the same ask is asked of the model again (gone
        // here, so not made).
        let later = make_with(
            5,
            Some(heard),
            "blink the light green twice when I come back to my desk",
            &keeping,
            chat_at(url_gone),
            NOW + ONE_ASK_WITHIN.as_millis() as u64 + 1,
            || None,
        );
        assert!(later.get("already_made").is_none(), "{later}");
        assert!(later["error"].is_string(), "{later}");
        let _ = take_said_line();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Reviewer-lead's HOLD on #6604: two makes of other asks, each ending
    /// on the same short answer to the lamp's question ("which colour?",
    /// "green"), heard within [`ONE_ASK_WITHIN`]: both are made, the second
    /// not told it is had already.
    #[test]
    fn two_asks_both_answered_green_make_two_skills() {
        let (root, keeping) = keeping("two-greens");
        let toml = |topic: &str, when: &str| {
            format!(
                "kind = \"installable\"\nversion = \"1.0.0\"\n\n[permissions]\n\
                 topics = [\"{topic}\"]\nlight = true\n\n[trigger]\ntopic = \"{topic}\"\n\
                 {when}\ncooldown = \"1h\"\n\n[ask]\npolicy = \"nudge\"\n\n[[do]]\n\
                 light = {{ colour = \"green\", level = 3, for = \"20s\" }}\n"
            )
        };
        let back = toml("room/presence", "field = \"someone\"\nequals = true");
        let leaving = toml("room/stood_up", "field = \"sitting_s\"\nabove = 0");
        let answer = |toml: &str, name: &str| {
            format!(
                "```skill.toml\n{toml}```\n\n```SKILL.md\n---\nname: {name}\n\
                 description: {name}\n---\n\n# {name}\n```\n"
            )
        };
        let (url, served) = service_sent(vec![
            (200, answer(&back, "green-on-return")),
            (200, answer(&leaving, "green-on-leaving")),
        ]);
        let helper = manage_helper(&keeping.manage);
        let first = make_with(
            7,
            Some("green"),
            "glow green when I come back to my desk",
            &keeping,
            chat_at(url.clone()),
            NOW,
            || None,
        );
        assert_eq!(first["made"], "green-on-return", "{first}");
        let _ = take_said_line();
        let asked = helper.join().unwrap();
        // Installed as the helper installs it (#7100: one whose folder is
        // gone is no longer on).
        let folder = keeping.installed.join("green-on-return");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join(".package.json"),
            json!({"sha256": asked["install_made"]}).to_string(),
        )
        .unwrap();
        std::fs::remove_file(&keeping.manage).unwrap();
        let helper = manage_helper(&keeping.manage);
        let second = make_with(
            8,
            Some("green"),
            "glow green when I leave my desk",
            &keeping,
            chat_at(url),
            NOW + 60_000,
            || None,
        );
        assert!(second.get("already_made").is_none(), "{second}");
        assert_eq!(second["made"], "green-on-leaving", "{second}");
        helper.join().unwrap();
        let _ = take_said_line();
        assert_eq!(served.join().unwrap().len(), 2, "the model was asked twice");
        assert_eq!(made_skill::entries(&keeping.directory).len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Only the same ask is held: another ask, a few words alone, or the
    /// same ask in another folder, let go unmade or no longer kept is made.
    #[test]
    fn only_the_same_ask_is_held_as_made() {
        let words = |line: &str| ask_words(line);
        let ask = words("make me a skill: when I come back, glow green");
        assert!(same_ask(
            &ask,
            &words("Hey Lamp, make me a skill: when I come back, glow green.")
        ));
        assert!(same_ask(
            &ask,
            &words("Make me a skill, when I come back glow green")
        ));
        assert!(!same_ask(
            &ask,
            &words("make me a skill: when I come back, glow blue")
        ));
        assert!(!same_ask(
            &words("make me a skill"),
            &words("make me a skill that nods")
        ));
        assert!(!same_ask(&[], &[]));
        assert!(!same_ask(&words("green"), &words("Green.")));
        assert!(!same_ask(&words("do it"), &words("do it")));
        assert!(same_ask(&ask, &ask));
        let directory = std::env::temp_dir().join(format!("one-ask-held-{}", std::process::id()));
        let other = directory.join("other");
        let held =
            OneAsk::begin(&directory, Some("glow green when I come back"), "glow", NOW).unwrap();
        // Under way: the same ask is told so, another folder's is not held.
        let told = OneAsk::begin(&directory, Some("glow green when I come back"), "x", NOW)
            .err()
            .unwrap();
        assert_eq!(told["making"], true, "{told}");
        assert!(OneAsk::begin(&other, Some("glow green when I come back"), "x", NOW).is_ok());
        // Let go unmade: asked again, it is made.
        drop(held);
        let again = OneAsk::begin(&directory, Some("glow green when I come back"), "x", NOW);
        assert!(again.is_ok());
        // Made, but no longer kept here: made again.
        again
            .unwrap()
            .made("glow-on-return", &json!({"made": "glow-on-return"}), NOW);
        assert!(OneAsk::begin(&directory, Some("glow green when I come back"), "x", NOW).is_ok());
    }

    /// lamp-9adf, 17 Nov 18:05 and 18:10: a make cut by the next ask, its
    /// line never answered, was heard again before a new, other ask ("make
    /// a skill that says stretch time whenever I say stretch"; "make a
    /// skill that waters my plants. Actually, never mind."), and that ask
    /// was told the earlier one "is being made already", whose words the
    /// model then said. A line that holds the held ask and an ask of its
    /// own is the same ask only when the requests are too; the held line
    /// carried with the lamp's name, or a few words, still is.
    #[test]
    fn a_line_carrying_another_ask_beside_one_under_way_is_not_that_ask() {
        const OLD: &str =
            "Lamp, make a skill that sends my test note whenever I say send my test note.";
        const NEW: &str = "Lamp, make a skill that says stretch time whenever I say stretch.";
        const ASKED: &str = "send my test note whenever I say send my test note";
        let directory = std::env::temp_dir().join(format!("one-ask-other-{}", std::process::id()));
        let held = OneAsk::begin_in(&directory, Some(OLD), ASKED, NOW, 31).unwrap();
        // The new ask, the old line heard before it: made.
        let both = format!("{OLD} {NEW}");
        let new = OneAsk::begin_in(
            &directory,
            Some(&both),
            "say stretch time whenever I say stretch",
            NOW,
            33,
        );
        assert!(new.is_ok(), "{:?}", new.err());
        drop(new);
        // The old line carried again with the lamp's name: the same ask.
        let carried = OneAsk::begin_in(
            &directory,
            Some(&format!("Hey Lamp. {OLD}")),
            "a skill that sends my test note",
            NOW,
            34,
        );
        assert_eq!(carried.err().unwrap()["making"], true);
        // The same request, whatever was heard with it: the same ask.
        let same = OneAsk::begin_in(&directory, Some(&both), ASKED, NOW, 35);
        assert_eq!(same.err().unwrap()["making"], true);
        drop(held);
    }

    /// lamp-9adf, 17 Nov: lines heard while a make was under way were
    /// answered as if it were on ("Just say joke time whenever you want
    /// one."), or with a question ("Should I stop making it altogether?").
    /// A later line is told what is being made, that nothing of it is on,
    /// and the id a no drops it by; the model's no drops it, and the make,
    /// done, is never put on and says nothing; any other answer is told it
    /// is still being made. Never in the make's own request.
    #[test]
    fn a_make_under_way_is_told_to_later_lines_and_a_no_drops_it() {
        let directory = std::env::temp_dir().join(format!("one-ask-making-{}", std::process::id()));
        let of = |at: &std::path::Path| at == directory;
        let held = OneAsk::begin_in(
            &directory,
            Some("make a skill that tells me a joke whenever I say joke time"),
            "tell me a joke whenever I say joke time",
            NOW,
            40,
        )
        .unwrap();
        assert!(making_note_where(40, of).is_none(), "its own request");
        let note = making_note_where(41, of).unwrap();
        assert!(
            note.contains("\"tell me a joke whenever I say joke time\""),
            "{note}"
        );
        assert!(note.contains("never say it is on"), "{note}");
        let id = format!("{MAKING_ID}{}", held.0);
        assert!(note.contains(&id), "{note}");
        // Not a make's id: the call goes on as a yes's.
        assert_eq!(
            making_answered("yes-0000abcd", 41, Some("no")),
            Making::NotOurs
        );
        // A yes, or none: still being made, nothing dropped.
        for answer in [Some("yes"), None, Some("unclear")] {
            let Making::Told(told) = making_answered(&id, 41, answer) else {
                panic!("{answer:?}");
            };
            assert_eq!(told["making"], true, "{told}");
            assert!(!held.dropped());
        }
        // A no: dropped, and told the plain line.
        let Making::Told(told) = making_answered(&id, 41, Some("no")) else {
            panic!("no");
        };
        assert_eq!(told["dropped"], true, "{told}");
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("Alright, I won't make it."),
            "{told}"
        );
        assert!(held.dropped());
        assert!(making_note_where(42, of).is_none(), "dropped: no note");
        // Asked again, it is made again.
        let again = OneAsk::begin_in(
            &directory,
            Some("make a skill that tells me a joke whenever I say joke time"),
            "tell me a joke whenever I say joke time",
            NOW,
            43,
        );
        assert!(again.is_ok());
        drop(again);
        drop(held);
        // Gone: nothing under way, nothing changed.
        let Making::Told(told) = making_answered(&id, 44, Some("no")) else {
            panic!("gone");
        };
        assert!(told.get("dropped").is_none(), "{told}");
    }

    /// lamp-9adf, 18 Nov 02:24–02:26 (#7554's row): a make ended waiting
    /// for its yes, and later make asks got a waiting line with no call.
    /// Ended (waiting for its yes, failed or dropped), a make is never told
    /// to a later line as under way, and a different ask is its own make;
    /// while one is under way, its note has a new ask made by its own call.
    #[test]
    fn a_make_ended_waiting_for_its_yes_is_never_told_under_way() {
        let directory = std::env::temp_dir().join(format!("one-ask-ended-{}", std::process::id()));
        let of = |at: &std::path::Path| at == directory;
        let under_way = OneAsk::begin_in(
            &directory,
            Some("make me a skill that tells me my first calendar event every evening at six"),
            "tell me my first calendar event every evening at six",
            NOW,
            5,
        )
        .unwrap();
        let note = making_note_where(6, of).unwrap();
        assert!(
            note.contains("a new ask, not this one: call make_skill for it with its own request"),
            "{note}"
        );
        assert!(
            note.contains("Never say a skill is being made unless make_skill is called"),
            "{note}"
        );
        // Made, its code waiting for the person's yes.
        under_way.made_with(
            "first-event",
            &json!({"made": "first-event", "on": false, "code_waits_for_yes": true}),
            NOW,
            None,
        );
        assert!(
            making_note_where(8, of).is_none(),
            "waits for its yes: not under way"
        );
        // A different make ask is its own: begun, and only it is under way.
        let other = OneAsk::begin_in(
            &directory,
            Some("make me a skill that tells me how many unread emails I have at noon"),
            "tell me how many unread emails I have at noon",
            NOW,
            8,
        )
        .unwrap();
        let note = making_note_where(9, of).unwrap();
        assert!(note.contains("unread emails"), "{note}");
        assert!(!note.contains("calendar"), "{note}");
        // Failed (let go): nothing under way.
        drop(other);
        assert!(making_note_where(10, of).is_none(), "failed: not under way");
        // Dropped: nothing under way.
        let dropped = OneAsk::begin_in(
            &directory,
            Some("make me a skill that reads me my next meeting every weekday at nine"),
            "read me my next meeting every weekday at nine",
            NOW,
            11,
        )
        .unwrap();
        let id = format!("{MAKING_ID}{}", dropped.0);
        assert!(matches!(
            making_answered(&id, 12, Some("no")),
            Making::Told(_)
        ));
        assert!(
            making_note_where(13, of).is_none(),
            "dropped: not under way"
        );
        drop(dropped);
        ASKS.lock()
            .unwrap()
            .retain(|ask| ask.directory != directory);
    }

    /// A make declined while it was made ([`making_answered`]) is kept
    /// nowhere on: never put on, no line left to say, and its turn told it
    /// was dropped.
    #[test]
    fn a_make_declined_while_made_is_never_put_on_or_said() {
        let (root, keeping) = keeping("declined-while-made");
        let (url, served) = service(vec![(200, good())]);
        let directory = keeping.directory.clone();
        let chat = chat_at(url);
        let told = make_with(
            50,
            Some("make me a skill that dims the light at 10 pm"),
            REQUEST,
            &keeping,
            move || {
                // The person's no comes while the model writes it.
                let id = ASKS
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|ask| ask.directory == directory && ask.made.is_none())
                    .map(|ask| ask.id)
                    .unwrap();
                let answered = making_answered(&format!("{MAKING_ID}{id}"), 51, Some("no"));
                assert!(matches!(answered, Making::Told(_)), "{answered:?}");
                chat()
            },
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(told["dropped"], true, "{told}");
        assert_eq!(take_said_line(), None);
        assert!(!any_on(&keeping.directory));
        // Nothing of it kept (El's read on #7554): no index entry, off or
        // on, to count toward the cap or be listed, and no package.
        assert!(
            made_skill::entries(&keeping.directory).is_empty(),
            "{:?}",
            made_skill::entries(&keeping.directory)
        );
        let packages: Vec<_> = std::fs::read_dir(&keeping.directory)
            .map(|read| {
                read.filter_map(Result::ok)
                    .map(|entry| entry.file_name())
                    .filter(|name| name.to_string_lossy().ends_with(".tar"))
                    .collect()
            })
            .unwrap_or_default();
        assert!(packages.is_empty(), "{packages:?}");
        assert!(
            !ASKS
                .lock()
                .unwrap()
                .iter()
                .any(|ask| ask.directory == keeping.directory),
            "let go"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Twelve's and El's race on #7554: a no that comes after the make's
    /// last look for one, while the manage helper puts it on, is never told
    /// it was dropped: the make goes on, its line is said, and the no is
    /// told it is too late to drop.
    #[test]
    fn a_no_while_a_make_is_put_on_is_never_told_dropped() {
        use std::io::BufRead;
        let (root, keeping) = keeping("no-while-put-on");
        let (url, served) = service(vec![(200, good())]);
        let directory = keeping.directory.clone();
        let listener = std::os::unix::net::UnixListener::bind(&keeping.manage).unwrap();
        // The manage helper, gated: the person's no comes while it puts the
        // skill on, before it answers.
        let helper = std::thread::spawn(move || {
            let stream = crate::make_skill_local::tests::accept_within(&listener);
            let mut line = String::new();
            std::io::BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap();
            let id = ASKS
                .lock()
                .unwrap()
                .iter()
                .find(|ask| ask.directory == directory && ask.made.is_none())
                .map(|ask| ask.id)
                .unwrap();
            let answered = making_answered(&format!("{MAKING_ID}{id}"), 61, Some("no"));
            writeln!(&stream, r#"{{"done": "installed"}}"#).unwrap();
            answered
        });
        let told = make_with(
            60,
            Some("make me a skill that dims the light at 10 pm"),
            REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1);
        let Making::Told(answered) = helper.join().unwrap() else {
            panic!("a make's id");
        };
        assert!(answered.get("dropped").is_none(), "{answered}");
        assert_eq!(answered["finishing"], true, "{answered}");
        assert!(
            !answered["next"]
                .as_str()
                .unwrap()
                .contains("I won't make it"),
            "{answered}"
        );
        assert!(told.get("dropped").is_none(), "{told}");
        assert!(any_on(&keeping.directory), "{told}");
        assert!(take_said_line().is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_MAKE_NO_CANNOT` (lamp-9adf, 16 Nov): the same ask a minute
    /// after a skill with code of its own was made and left off, its turn
    /// cut, is not told it is on: its say-back and its yes are asked again,
    /// the wait the make started gone, a new one begun. Off, the earlier
    /// answer is given as it was.
    #[test]
    fn with_the_switch_the_same_ask_of_a_skill_left_off_asks_its_yes_again() {
        const ASK: &str =
            "when I sit down at my desk, tell me how many open issues I have on Linear";
        let directory =
            std::env::temp_dir().join(format!("one-ask-left-off-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(made_skill::INDEX),
            r#"[{"name":"linear-open-issues","on":false}]"#,
        )
        .unwrap();
        let line =
            "I made linear open issues: … It waits for you to agree before it runs any code.";
        let yes = crate::skill_write_yes::Asks {
            skill: "linear-open-issues".into(),
            sha256: "ab".repeat(32),
            writes: Vec::new(),
            say_back: "The lamp's model wrote its code, and that code may use your Linear.".into(),
            grants: Some(vec!["linear-read".into(), "speak".into()]),
            code: true,
            remove: false,
        };
        let told = json!({
            "made": "linear-open-issues",
            "on": false,
            "code_waits_for_yes": true,
            crate::skill_write_yes::APPROVE_ID: "yes-gone",
        });
        for on in [false, true] {
            crate::make_no_cannot::set_on_here(Some(on));
            OneAsk::begin_in(&directory, Some(ASK), ASK, NOW, 7)
                .unwrap()
                .made_with(
                    "linear-open-issues",
                    &told,
                    NOW,
                    Some((yes.clone(), line.into())),
                );
            let again = OneAsk::begin_in(&directory, Some(ASK), ASK, NOW + 60_000, 9)
                .err()
                .unwrap();
            assert_eq!(again["already_made"], true, "{again}");
            let id = again[crate::skill_write_yes::APPROVE_ID].as_str().unwrap();
            if on {
                assert_ne!(id, "yes-gone", "{again}");
                assert!(crate::skill_write_yes::waits(id), "{again}");
                assert!(again["next"].as_str().unwrap().contains(line), "{again}");
                assert!(again.get("writes_wait_for_yes").is_none(), "{again}");
            } else {
                assert_eq!(again, {
                    let mut was = told.clone();
                    was["already_made"] = json!(true);
                    was
                });
            }
            ASKS.lock()
                .unwrap()
                .retain(|ask| ask.directory != directory);
        }
        crate::skill_write_yes::clear_for_test();
        crate::make_no_cannot::set_on_here(None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A made skill that mails mai when the person gets up (#6066).
    const WRITE_TOML: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\
        [permissions]\ntopics = [\"room/stood_up\"]\nservices = { gmail = \"write\" }\n\
        [trigger]\ntopic = \"room/stood_up\"\nfield = \"sitting_s\"\nabove = 0\n\
        cooldown = \"1h\"\n[ask]\npolicy = \"nudge\"\n[[do]]\n\
        write = { service = \"gmail\", action = \"send_mail\", to = [\"mai@example.com\"], \
        subject = \"Leaving\", text = \"I'm leaving now.\" }\n";
    const WRITE_CARD: &str =
        "---\nname: leaving-mail\ndescription: Mails Mai when I leave.\n---\n\n# Leaving mail\n";
    const WRITE_REQUEST: &str = "make a skill that mails mai@example.com with the subject \
                                 Leaving, saying I'm leaving now, when I get up from my desk";

    fn write_answer(toml: &str) -> String {
        format!("```skill.toml\n{toml}```\n\n```SKILL.md\n{WRITE_CARD}```\n")
    }

    /// Lamp-9adf, 11 Nov: a calendar event made by voice, its start and
    /// end fixed from the date said, as the instructions now tell the
    /// model, is kept, put on and waits for the person's yes.
    #[test]
    fn a_made_calendar_event_with_its_times_fixed_is_kept_and_waits_for_a_yes() {
        made_skill::set_writes_here(Some(true));
        let (root, keeping) = keeping("calendar-write");
        let request = "make a skill that adds a calendar event called Lamp test on November \
                       twentieth at nine pm when I come back to my desk";
        let toml = "kind = \"installable\"\nversion = \"1.0.0\"\n\
            [permissions]\ntopics = [\"room/presence\"]\n\
            services = { google_calendar = \"write\" }\n\
            [trigger]\ntopic = \"room/presence\"\nfield = \"someone\"\nequals = true\n\
            cooldown = \"1h\"\nrearm = \"ends\"\n[ask]\npolicy = \"nudge\"\n[[do]]\n\
            write = { service = \"google_calendar\", action = \"create_event\", \
            title = \"Lamp test\", start = \"2026-11-20T21:00:00+07:00\", \
            end = \"2026-11-20T22:00:00+07:00\" }\n";
        let (url, served) = service(vec![(200, write_answer(toml))]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            7,
            Some(request),
            request,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1, "{told}");
        assert_eq!(told["made"], "leaving-mail", "{told}");
        assert_eq!(told["writes_wait_for_yes"], true, "{told}");
        asked.join().unwrap();
        let sha256 = made_skill::entries(&keeping.directory)[0]["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let writes = made_skill::writes_of(&keeping.directory, &sha256);
        assert_eq!(writes[0]["start"], "2026-11-20T21:00:00+07:00");
        assert_eq!(writes[0]["title"], "Lamp test");
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Items 2, 4 and 5 (#6066): a write whose words aren't the request's
    /// goes back for repair; the one kept is put on, said back whole with
    /// its wait, and its writes wait for a yes in a later heard line, which
    /// approves exactly its contents; the say-back's own words heard back
    /// approve nothing.
    #[test]
    fn a_made_write_is_said_back_whole_and_waits_for_a_later_yes() {
        made_skill::set_writes_here(Some(true));
        let (root, keeping) = keeping("write");
        let made_up = WRITE_TOML.replace("I'm leaving now.", "I'm off, see you tomorrow!");
        let (url, served) = service(vec![
            (200, write_answer(&made_up)),
            (200, write_answer(WRITE_TOML)),
        ]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            7,
            Some(WRITE_REQUEST),
            WRITE_REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 2, "repaired once: {told}");
        assert_eq!(told["made"], "leaving-mail", "{told}");
        assert_eq!(told["writes_wait_for_yes"], true, "{told}");
        let next = told["next"].as_str().unwrap();
        for part in [
            "mails mai@example.com from your Gmail, subject “Leaving”, saying “I'm leaving now.”",
            made_skill::WAITS_FOR_YES.trim(),
            "call make_skill again with only approve_id",
            "don't call it: nothing is sent",
        ] {
            assert!(next.contains(part), "{part}: {next}");
        }
        assert!(!next.contains("it's on:"), "{next}");
        assert!(next.contains("word for word"), "{next}");
        asked.join().unwrap();
        let id = told[crate::skill_write_yes::APPROVE_ID]
            .as_str()
            .unwrap()
            .to_owned();
        let now = std::time::Instant::now();
        // A yes before the say-back was heard: not taken, said again.
        let never =
            |_: &crate::skill_write_yes::Asks| -> Result<Value, String> { panic!("approved") };
        let early = crate::skill_write_yes::approve(&id, 8, true, true, now, never);
        assert_eq!(early["error"], crate::skill_write_yes::NOT_SAID_WHOLE);
        // The lamp says the line whole in the make's request; a yes in a
        // later heard line approves its exact write and its package.
        let line = next.split('"').nth(1).unwrap();
        crate::skill_write_yes::lamp_said(7, line);
        let sha256 = made_skill::entries(&keeping.directory)[0]["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let writes = made_skill::writes_of(&keeping.directory, &sha256);
        assert_eq!(writes[0]["text"], "I'm leaving now.");
        let mut approved = None;
        let told = crate::skill_write_yes::approve(&id, 9, true, true, now, |asks| {
            approved = Some(asks.clone());
            Ok(json!({"approved": 1, "public": []}))
        });
        assert_eq!(told["approved"], true, "{told}");
        let approved = approved.unwrap();
        assert_eq!(
            (approved.skill.as_str(), &approved.writes, &approved.sha256),
            ("leaving-mail", &writes, &sha256)
        );
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// App Only's P2 on #6142: a write skill made in a turn that was cut is
    /// said through the say service with no say-back and no `approve_id`
    /// given the model, so its line names the wait and that nothing is
    /// sent yet, never "it's on" alone; the model's own answer is unchanged.
    #[test]
    fn a_write_made_in_a_cut_turn_is_said_to_wait_never_just_on() {
        made_skill::set_writes_here(Some(true));
        let (root, keeping) = keeping("write-cut");
        let (url, served) = service(vec![(200, write_answer(WRITE_TOML))]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            7,
            Some(WRITE_REQUEST),
            WRITE_REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1, "{told}");
        assert_eq!(told["writes_wait_for_yes"], true, "{told}");
        asked.join().unwrap();
        let said = take_said_line().expect("a line for a cut turn");
        assert_eq!(
            said,
            format!(
                "I made leaving mail.{} {}",
                made_skill::WAITS_FOR_YES,
                made_skill::CUT_WRITE_UNSAID
            )
        );
        assert!(!said.contains("it's on"), "{said}");
        assert!(!said.contains("yes"), "no word to repeat: {said}");
        // The turn that waits still gets the say-back whole, and its wait.
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("mails mai@example.com"), "{next}");
        assert!(next.contains("approve_id"), "{next}");
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The leaving-mail write skill made and put on in request 7, its
    /// make's own wait left as it is: the test's folder, its keeping and
    /// the line the make said back.
    fn made_write(name: &str) -> (PathBuf, Keeping, String) {
        made_skill::set_writes_here(Some(true));
        let (root, keeping) = keeping(name);
        let (url, served) = service(vec![(200, write_answer(WRITE_TOML))]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            7,
            Some(WRITE_REQUEST),
            WRITE_REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1, "{told}");
        asked.join().unwrap();
        assert_eq!(told["writes_wait_for_yes"], true, "{told}");
        let line = told["next"].as_str().unwrap().split('"').nth(1).unwrap();
        (root, keeping, line.to_owned())
    }

    /// A heard line of `request`, the owner's, on the route with the echo
    /// judge: who a read-back is read whole to.
    fn asking(request: u64) -> Asking {
        Asking {
            request,
            heard: true,
            owner_hears: true,
            sends_here: true,
        }
    }

    fn not_approved(_: &crate::skill_write_yes::Asks) -> Option<(bool, bool)> {
        Some((true, false))
    }

    fn never(_: &crate::skill_write_yes::Asks) -> Result<Value, String> {
        panic!("approved")
    }

    /// The owner, 11 Nov: a made write skill still waiting for its yes
    /// (its turn cut, or no answer then) is read back by voice, "what does
    /// my leaving mail skill send?": said back word for word as its make
    /// said it, waiting again; a yes before it is heard whole is not taken,
    /// and a later heard yes approves exactly its writes and its package,
    /// through the make's own approval.
    #[test]
    fn a_waiting_write_skill_read_back_is_said_whole_and_a_later_yes_approves() {
        let (root, keeping, made_line) = made_write("read-back");
        let now = std::time::Instant::now();
        let told = read_back(
            "my Leaving mail skill",
            asking(20),
            &keeping.directory,
            not_approved,
            now,
        );
        assert_eq!(told["writes_wait_for_yes"], true, "{told}");
        assert_eq!(told["skill"], "leaving-mail", "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("word for word"), "{next}");
        assert!(
            next.contains("call make_skill again with only approve_id"),
            "{next}"
        );
        let line = next.split('"').nth(1).unwrap();
        assert_eq!(line, made_line, "the make's own say-back");
        assert!(line.ends_with(made_skill::WAITS_FOR_YES.trim()), "{line}");
        let id = told[crate::skill_write_yes::APPROVE_ID]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(crate::skill_write_yes::awaited(now), "the echo hold is on");
        // A yes before it was said: not taken, said again.
        let early = crate::skill_write_yes::approve(&id, 21, true, true, now, never);
        assert_eq!(early["error"], crate::skill_write_yes::NOT_SAID_WHOLE);
        // Said whole in the read-back's answer: a later heard yes approves.
        crate::skill_write_yes::lamp_said(20, line);
        let sha256 = made_skill::entries(&keeping.directory)[0]["sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let writes = made_skill::writes_of(&keeping.directory, &sha256);
        let mut approved = None;
        let told = crate::skill_write_yes::approve(&id, 22, true, true, now, |asks| {
            approved = Some(asks.clone());
            Ok(json!({"approved": 1, "public": []}))
        });
        assert_eq!(told["approved"], true, "{told}");
        let approved = approved.unwrap();
        assert_eq!(
            (approved.skill.as_str(), &approved.writes, &approved.sha256),
            ("leaving-mail", &writes, &sha256)
        );
        assert!(!crate::skill_write_yes::awaited(now));
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A read-back cut (a Stop, a line over it) is no read-back: a yes
    /// after it is not taken; nor is the read-back's own tail heard back
    /// (held as the lamp's echo, `own_echo::yes_echo`), a yes on the Live
    /// route, or one in the read-back's own request.
    #[test]
    fn a_cut_or_echoed_read_back_approves_nothing() {
        let (root, keeping, _) = made_write("read-back-cut");
        let now = std::time::Instant::now();
        let ask = |request| {
            read_back(
                "leaving mail",
                asking(request),
                &keeping.directory,
                not_approved,
                now,
            )
        };
        let told = ask(20);
        let id = told[crate::skill_write_yes::APPROVE_ID].as_str().unwrap();
        let line = told["next"].as_str().unwrap().split('"').nth(1).unwrap();
        crate::skill_write_yes::lamp_said(20, line);
        crate::skill_write_yes::lamp_cut(20);
        let told = crate::skill_write_yes::approve(id, 21, true, true, now, never);
        assert_eq!(told["error"], crate::skill_write_yes::NOT_SAID_WHOLE);
        // Read back again and said whole: its own last words heard back in
        // the tail are held, and an approval in that line is refused.
        let told = ask(30);
        let id = told[crate::skill_write_yes::APPROVE_ID].as_str().unwrap();
        crate::skill_write_yes::lamp_said(30, line);
        assert!(crate::skill_write_yes::awaited(now));
        crate::skill_write_yes::echo_held(31);
        let told = crate::skill_write_yes::approve(id, 31, true, true, now, never);
        assert_eq!(told["error"], crate::skill_write_yes::NOT_APPROVED);
        // In its own request, or on the Live route: refused too.
        for (request, sends_here, refused) in [
            (40, true, crate::skill_write_yes::NOT_APPROVED),
            (41, false, crate::skill_write_yes::NOT_HERE),
        ] {
            let told = ask(40);
            let id = told[crate::skill_write_yes::APPROVE_ID].as_str().unwrap();
            crate::skill_write_yes::lamp_said(40, line);
            let told = crate::skill_write_yes::approve(id, request, true, sends_here, now, never);
            assert_eq!(told["error"], refused, "{request}");
        }
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A "no", or no answer, calls nothing: the read-back approved nothing
    /// itself (it only asked whether it was approved), and the wait lapses.
    #[test]
    fn a_read_back_declined_or_unanswered_stays_unapproved() {
        let (root, keeping, _) = made_write("read-back-no");
        let now = std::time::Instant::now();
        let mut asked = 0;
        let told = read_back(
            "leaving mail",
            asking(20),
            &keeping.directory,
            |asks| {
                asked += 1;
                assert_eq!(asks.skill, "leaving-mail");
                Some((true, false))
            },
            now,
        );
        assert_eq!(asked, 1);
        let id = told[crate::skill_write_yes::APPROVE_ID].as_str().unwrap();
        let line = told["next"].as_str().unwrap().split('"').nth(1).unwrap();
        crate::skill_write_yes::lamp_said(20, line);
        let later = now + crate::skill_write_yes::YES_WITHIN + std::time::Duration::from_secs(1);
        assert!(!crate::skill_write_yes::awaited(later));
        let told = crate::skill_write_yes::approve(id, 22, true, true, later, never);
        assert_eq!(told["error"], crate::skill_write_yes::NOT_APPROVED);
        // Not told by lamp-services whether it is approved: read back and
        // asked, as not approved.
        let told = read_back(
            "leaving mail",
            asking(30),
            &keeping.directory,
            |_| None,
            now,
        );
        assert_eq!(told["writes_wait_for_yes"], true, "{told}");
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Approved already for what it sends now: said, with that it was
    /// agreed, and nothing asked or awaited.
    #[test]
    fn an_approved_write_skill_read_back_is_said_and_asks_nothing() {
        let (root, keeping, _) = made_write("read-back-approved");
        let now = std::time::Instant::now();
        let told = read_back(
            "leaving-mail",
            asking(20),
            &keeping.directory,
            |_| Some((true, true)),
            now,
        );
        assert_eq!(told["approved"], true, "{told}");
        assert!(
            told.get(crate::skill_write_yes::APPROVE_ID).is_none(),
            "{told}"
        );
        assert!(told.get("writes_wait_for_yes").is_none(), "{told}");
        let next = told["next"].as_str().unwrap();
        let line = next.split('"').nth(1).unwrap();
        assert!(
            line.starts_with("Your leaving mail: ")
                && line.contains("mails mai@example.com from your Gmail, subject “Leaving”, saying “I'm leaving now.”")
                && line.ends_with(made_skill::APPROVED_ALREADY),
            "{line}"
        );
        assert!(!line.contains(made_skill::WAITS_FOR_YES.trim()), "{line}");
        assert!(next.contains("Ask nothing, call nothing"), "{next}");
        assert!(!next.contains("approve_id"), "{next}");
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// One that sends nothing, none of that name, one not the package on
    /// the lamp, or a turn nobody spoke: said plainly, nothing awaited.
    #[test]
    fn a_read_back_of_no_write_skill_or_none_says_so_plainly() {
        made_skill::set_writes_here(Some(true));
        let (root, keeping) = keeping("read-back-none");
        let (url, served) = service(vec![(200, good())]);
        let asked = manage_helper(&keeping.manage);
        let made = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(made["on"], true, "{made}");
        served.join().unwrap();
        asked.join().unwrap();
        let now = std::time::Instant::now();
        let unasked =
            |_: &crate::skill_write_yes::Asks| -> Option<(bool, bool)> { panic!("asked") };
        let told = read_back("evening dim", asking(20), &keeping.directory, unasked, now);
        assert_eq!(told["sends"], false, "{told}");
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("evening dim doesn't send or write anything"),
            "{told}"
        );
        let told = read_back("morning mail", asking(20), &keeping.directory, unasked, now);
        assert_eq!(told["error"], NO_SKILL_BY_THAT_NAME, "{told}");
        assert_eq!(told["on"], json!(["evening dim"]), "{told}");
        // Thirteen's P3 on #6250: to a line the owner may not hear, a name
        // that matches none names no other skill, since a made skill's name
        // can carry a person's.
        let told = read_back(
            "morning mail",
            Asking {
                owner_hears: false,
                ..asking(20)
            },
            &keeping.directory,
            unasked,
            now,
        );
        assert_eq!(told["error"], NO_SKILL_BY_THAT_NAME, "{told}");
        assert_eq!(told["on"], json!([]), "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("no skill by that name"), "{next}");
        assert!(!next.contains("evening dim"), "{next}");
        assert!(!told.to_string().contains("evening dim"), "{told}");
        let told = read_back(
            "evening dim",
            Asking {
                heard: false,
                ..asking(20)
            },
            &keeping.directory,
            unasked,
            now,
        );
        assert_eq!(told["error"], READ_BACK_NOT_ASKED);
        assert!(!crate::skill_write_yes::awaited(now));
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
        // A write skill whose package on the lamp is another: said to have
        // changed since it was made, and to ask again (App Only's P3 on
        // #6230), never that no skill is on by that name.
        let (root, keeping, _) = made_write("read-back-other");
        let told = read_back(
            "leaving mail",
            asking(20),
            &keeping.directory,
            |_| Some((false, false)),
            now,
        );
        assert!(told["error"].is_string(), "{told}");
        assert!(told.get("on").is_none(), "{told}");
        assert_eq!(told["next"], changed_since_made("leaving mail"), "{told}");
        assert!(
            told.get(crate::skill_write_yes::APPROVE_ID).is_none(),
            "{told}"
        );
        assert!(told.get("writes_wait_for_yes").is_none(), "{told}");
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// App Only's P2s on #6230 and #6250: a write skill's recipients and
    /// words are read back only in the private room or, as a services read
    /// in full (`services::said_whole`), to the owner seen saying the line,
    /// the one face in view and in their voice at 0.55 or more on the judge
    /// route; to anyone else its name only, that it sends, nothing asked of
    /// `lamp-services` and no wait started.
    #[test]
    fn a_read_back_is_said_whole_only_in_the_private_room_or_to_the_owner_alone_and_heard() {
        let (root, keeping, made_line) = made_write("read-back-who");
        let now = std::time::Instant::now();
        for (why, private_room, owner, seen, alone, likeness, whole) in [
            (
                "the private room",
                true,
                Some("owner"),
                false,
                false,
                None,
                true,
            ),
            (
                "the owner alone, in their voice",
                false,
                Some("owner"),
                true,
                true,
                Some(0.6),
                true,
            ),
            (
                "a silent guest in view",
                false,
                Some("owner"),
                true,
                false,
                Some(0.9),
                false,
            ),
            (
                "a likeness under the bar",
                false,
                Some("owner"),
                true,
                true,
                Some(0.5),
                false,
            ),
            (
                "no likeness, or Live",
                false,
                Some("owner"),
                true,
                true,
                None,
                false,
            ),
            (
                "the owner not seen",
                false,
                Some("owner"),
                false,
                true,
                Some(0.9),
                false,
            ),
            ("no owner's line", false, None, true, true, Some(0.9), false),
        ] {
            let seen_owner =
                crate::services::owner_seen_saying(true, private_room, owner, |_| seen);
            let owner_hears =
                crate::services::said_whole(private_room, seen_owner, || alone, || likeness);
            let mut asked = false;
            let told = read_back(
                "leaving mail",
                Asking {
                    owner_hears,
                    ..asking(20)
                },
                &keeping.directory,
                |_| {
                    asked = true;
                    Some((true, false))
                },
                now,
            );
            let next = told["next"].as_str().unwrap();
            if whole {
                assert_eq!(told["writes_wait_for_yes"], true, "{why}: {told}");
                assert_eq!(next.split('"').nth(1).unwrap(), made_line, "{why}");
                assert!(crate::skill_write_yes::awaited(now), "{why}");
            } else {
                assert!(!asked, "{why}");
                assert_eq!(next, sends_unsaid("leaving mail"), "{why}");
                assert!(!next.contains("mai@example.com"), "{why}: {next}");
                assert!(!next.contains("leaving now"), "{why}: {next}");
                assert!(
                    told.get(crate::skill_write_yes::APPROVE_ID).is_none(),
                    "{why}: {told}"
                );
                assert!(told.get("writes_wait_for_yes").is_none(), "{why}");
            }
        }
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// App Only's P3 on #6230: on a route that can't take the yes (the
    /// Live route), a waiting skill read back is said to wait, no wait
    /// starts, and a pending make's wait is left as it is.
    #[test]
    fn a_read_back_on_a_route_with_no_yes_starts_no_wait() {
        let (root, keeping, _) = made_write("read-back-live");
        let now = std::time::Instant::now();
        let first = read_back(
            "leaving mail",
            asking(20),
            &keeping.directory,
            not_approved,
            now,
        );
        let id = first[crate::skill_write_yes::APPROVE_ID]
            .as_str()
            .unwrap()
            .to_owned();
        let told = read_back(
            "leaving mail",
            Asking {
                sends_here: false,
                ..asking(30)
            },
            &keeping.directory,
            not_approved,
            now,
        );
        assert_eq!(told["next"], yes_not_here("leaving mail"), "{told}");
        assert!(
            told.get(crate::skill_write_yes::APPROVE_ID).is_none(),
            "{told}"
        );
        assert!(told.get("writes_wait_for_yes").is_none(), "{told}");
        // The wait before it goes on, its id unchanged.
        assert!(crate::skill_write_yes::awaited(now));
        crate::skill_write_yes::lamp_said(
            20,
            first["next"].as_str().unwrap().split('"').nth(1).unwrap(),
        );
        let mut approved = false;
        let told = crate::skill_write_yes::approve(&id, 31, true, true, now, |_| {
            approved = true;
            Ok(json!({"approved": 1, "public": []}))
        });
        assert!(approved, "{told}");
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The read-back is offered only with a made skill's writes on, and
    /// `make_skill` stays within what the voice's link takes: a description
    /// of at most 1,024 bytes with no "yes", parameters of at most 4,096.
    #[test]
    fn the_read_back_is_offered_with_writes_alone_within_the_bounds() {
        for writes in [false, true] {
            made_skill::set_writes_here(Some(writes));
            let tool = tool();
            assert!(
                tool.description.len() <= 1_024,
                "{}",
                tool.description.len()
            );
            assert!(!tool.description.contains("yes"), "{}", tool.description);
            let parameters = tool.parameters.unwrap();
            assert!(parameters.to_string().len() <= 4_096);
            assert_eq!(parameters["properties"].get(READ_BACK).is_some(), writes);
            assert_eq!(tool.description.contains("read_back"), writes);
        }
        made_skill::set_writes_here(None);
    }

    /// App Only's P1 on #6142: a write's address and words are held to the
    /// line heard, never to the model's `request` argument, which may carry
    /// what a read said: an address or a text only in it is refused.
    #[test]
    fn a_writes_words_only_in_the_models_request_are_refused() {
        made_skill::set_writes_here(Some(true));
        let (root, keeping) = keeping("write-heard");
        let (url, served) = service(vec![(200, write_answer(WRITE_TOML)); 1 + MOST_REPAIRS]);
        let told = make_with(
            7,
            Some("make a skill that mails that to them when I get up from my desk"),
            WRITE_REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        assert!(told["error"].is_string(), "{told}");
        assert!(told.get("writes_wait_for_yes").is_none(), "{told}");
        assert!(made_skill::entries(&keeping.directory).is_empty());
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Item 1: with LAMP_SKILL_WRITES off the making model is told a skill
    /// can't send or write on its own, said "I can't do that yet: sending
    /// or writing on its own"; on, it is told the kinds and the request's
    /// own words, and `make_skill` takes an `approve_id`.
    #[test]
    fn the_system_prompt_and_function_carry_a_write_only_with_its_switch() {
        made_skill::set_writes_here(Some(false));
        for reads in [true, false] {
            let off = system_with(reads);
            assert!(
                off.contains("cannot: I can't do that yet: sending or writing on its own."),
                "{off}"
            );
            assert!(!off.contains("services = { CODE = \"write\" }"));
        }
        assert!(
            parameters(false, false)["properties"]
                .get("approve_id")
                .is_none()
        );
        // A clash's choice (#6593) comes with no request: none required.
        assert_eq!(parameters(false, false)["required"], json!([]));
        assert!(parameters(false, false)["properties"]["clash_id"].is_object());
        made_skill::set_writes_here(Some(true));
        for reads in [true, false] {
            let on = system_with(reads);
            for rule in [
                "services = { CODE = \"write\" }",
                "gmail send_mail: to, cc, subject, text",
                "the person's own words in the request, copied exactly",
                "never take them from a read, the web or memory",
                "an email address the request says, or the person's own",
            ] {
                assert!(on.contains(rule), "{rule}: {on}");
            }
            assert!(!on.contains("sending or writing on its own."), "{on}");
        }
        assert!(parameters(true, false)["properties"]["approve_id"].is_object());
        made_skill::set_writes_here(None);
    }

    /// Lamp-9adf, 11 Nov, writes on: "email me ..." and "email
    /// test@example.com ..." were refused `cannot:`, the second with the
    /// reads' ending. With writes on, the reads' refusal and the can't-yet
    /// say a mail or an event is a write, and the rules show one whole write
    /// skill to the person's own address, which the lamp's check keeps.
    #[test]
    fn with_writes_on_the_instructions_show_a_whole_write_the_check_keeps() {
        made_skill::set_writes_here(Some(true));
        let on = system_with(true);
        for rule in [
            "is never answered so: it is a write, made as below.",
            "a mail or a calendar event is no message to a phone: it is a write, allowed below",
            "me, myself or my email is the person's own",
            &format!("A whole write skill, for \"{WRITE_EXAMPLE_ASKED}\":\n\n```skill.toml\n"),
            "write = { service = \"gmail\", action = \"send_mail\", to = [",
        ] {
            assert!(on.contains(rule), "{rule}: {on}");
        }
        let own = vec![
            ("gmail".to_owned(), "me@example.test".to_owned()),
            ("google_calendar".to_owned(), "me@example.test".to_owned()),
        ];
        let rules = write_rules(&own, None);
        let example = write_example("me@example.test");
        assert!(rules.contains(&example), "{rules}");
        assert!(example.contains("to = [\"me@example.test\"]"), "{example}");
        let (root, keeping) = keeping("write-example");
        let card = "---\nname: back-mail\ndescription: Mails me when I come back to my desk.\n\
                    ---\n\n# Back mail\n";
        let answer = format!("{example}\n\n```SKILL.md\n{card}```\n");
        let asked = made_skill::Asked {
            request: WRITE_EXAMPLE_ASKED,
            own,
            heard: Some(WRITE_EXAMPLE_ASKED),
            judge: None,
            made_with: made_skill::MadeWith::DeepSeek,
            named: None,
        };
        let made = made_skill::keep_asked(&answer, false, FROM, &keeping, &asked).unwrap();
        assert_eq!(made.uses.writes, ["gmail"]);
        assert_eq!(made.uses.sends_to, ["me@example.test"]);
        made_skill::set_writes_here(Some(false));
        let off = system_with(true);
        assert!(!off.contains("A whole write skill") && !off.contains("is never answered so"));
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Lamp-9adf, 11 Nov, writes on: a mail or a calendar event made by
    /// voice was refused `cannot:` each time, the instructions naming no
    /// write among what a skill may do, giving "sending a message" as the
    /// example of what it can't, and refusing "a request to ... change
    /// anything in a service". With writes on, a write is among what a
    /// skill may do, no can't-yet rule or example names a send, and a mail
    /// sent or an event added is a write, never a change; the brief for
    /// each request points to the write and carries the request whole.
    #[test]
    fn with_writes_on_nothing_in_the_instructions_refuses_a_write() {
        made_skill::set_writes_here(Some(true));
        for reads in [true, false] {
            let on = system_with(reads);
            for rule in [
                "its hum, or a read or a write to a linked service as below, on one of the \
                 triggers below",
                "no step or trigger here or below does",
                "(\"I can't do that yet: hearing what is said.\")",
                "A skill may also send or write to one linked service by itself",
                "write = { service = \"CODE\", action = \"ACTION\", its keys }",
                "google_calendar create_event: title, start",
            ] {
                assert!(on.contains(rule), "{rule}: {on}");
            }
            for refusal in [
                "sending a message.",
                "send a message or write to a service",
                "sending or writing on its own",
                "and only read it",
                "A request to delete or change anything in a service",
                "A request to send, write or change anything",
                "never sends, writes, posts",
            ] {
                assert!(!on.contains(refusal), "{refusal}: {on}");
            }
            if reads {
                assert!(
                    on.contains(
                        "Sending a mail or adding a calendar event, an issue, a task or a \
                         comment is no change: it is a write"
                    ),
                    "{on}"
                );
            } else {
                assert!(
                    on.contains("a request that needs a read of one (a write below is no read)")
                );
            }
        }
        for request in [
            "make a skill that adds a calendar event called Lamp test on November twentieth at \
             nine pm when I come back to my desk",
            "make a skill that emails test@example.com with the subject Lamp test and the words \
             I am back when I come back to my desk",
        ] {
            let brief = made_skill::brief_with(request, true).unwrap();
            assert!(brief.contains(" Or a read/write as above."), "{brief}");
            assert!(brief.ends_with(&format!("Request: {request}")), "{brief}");
        }
        // A calendar event's start and end: today's local date, the
        // offset, the form `lamp-services` takes and an end when none is
        // said; with the date unknown, an event is `cannot:`.
        let today = ("Wednesday 2026-11-11".to_owned(), "+07:00".to_owned());
        let rules = services(true, true, &[], Some(&today));
        for rule in [
            "like 2026-11-20T21:00:00+07:00: today here is Wednesday 2026-11-11, offset +07:00",
            "the day and time the request names (a day said from today, like tonight or \
             tomorrow, counted from Wednesday 2026-11-11)",
            "else one hour after its start",
            "cannot: I can't do that yet: adding an event with no day and time",
        ] {
            assert!(rules.contains(rule), "{rule}: {rules}");
        }
        let unknown = services(true, true, &[], None);
        assert!(
            unknown.contains(
                "This lamp's date isn't known now: a request for a calendar event is answered \
                 with one line and no blocks: cannot:"
            ),
            "{unknown}"
        );
        assert!(!unknown.contains("today here is"), "{unknown}");
        assert!(!services(true, false, &[], Some(&today)).contains("today here is"));
        // Off, the lamp's "can't do that yet" stays as it was.
        made_skill::set_writes_here(Some(false));
        for reads in [true, false] {
            let off = system_with(reads);
            assert!(
                off.contains("(\"I can't do that yet: sending a message.\")"),
                "{off}"
            );
            assert!(
                off.contains("its hum, or a read as below, on one of"),
                "{off}"
            );
        }
        made_skill::set_writes_here(None);
    }

    /// Kept, and the manage helper not there: it stays off, the model is
    /// told to say why in one line, and nothing waits for a yes.
    #[test]
    fn a_kept_skill_not_put_on_is_said_with_why_and_stays_off() {
        let (root, keeping) = keeping("not-on");
        let (url, served) = service(vec![(200, good())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(told["on"], false, "{told}");
        assert_eq!(told["not_on"], "the lamp's skills couldn't be reached");
        assert_eq!(
            told["next"],
            format!(
                "{} Ask nothing about putting it on.",
                made_skill::in_their_language(
                    "I made it but couldn't put it on: the lamp's skills couldn't be reached."
                )
            )
        );
        assert_eq!(
            take_said_line().as_deref(),
            Some("I made it but couldn't put it on: the lamp's skills couldn't be reached.")
        );
        assert!(!any_on(&keeping.directory));
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The journal says why a make wasn't kept, in the lamp's own words,
    /// never the request nor the check's lines (#5977: lamp-74ff's "wasn't
    /// kept in 1848 ms" said no reason); kept, it says so.
    #[test]
    fn the_journal_says_why_a_make_was_not_kept() {
        let refused = Err(Refused(
            "it doesn't pass the lamp's check".into(),
            Some("skill.toml: unknown field `secret`".into()),
        ));
        assert_eq!(
            kept_line(&refused, false, 1848),
            "lamp-voice: a skill made here wasn't kept in 1848 ms: it doesn't pass the lamp's check"
        );
        let (root, keeping) = keeping("journal");
        let (url, served) = service(vec![(200, good())]);
        let made = make_kept(
            REQUEST,
            made_skill::brief(REQUEST).unwrap(),
            &keeping,
            chat_at(url),
            "local:1:1",
            None,
        );
        assert_eq!(served.join().unwrap(), 1);
        let line = kept_line(&made, false, 12);
        assert_eq!(line, "lamp-voice: a skill made here was kept in 12 ms");
        assert!(!line.contains("10 pm"), "{line}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A request no made skill can do (lamp-0c89's "when someone says good
    /// night", Mobile on #4710, 3 Nov): the model is told to answer
    /// `cannot:`, and that answer is asked once, never again as a cut one,
    /// and said as not made; nothing is kept.
    #[test]
    fn a_cannot_answer_is_asked_once_and_said_as_not_made() {
        assert!(system().contains("cannot: then one short sentence said to the person"));
        // The hint for a reminder at a time or every so often (#5977).
        assert!(
            system().contains(
                "Ask me for a reminder instead, like 'remind me every hour to stand up'."
            ),
            "{}",
            system()
        );
        assert!(system().contains("add no time of day the request didn't name"));
        let (root, keeping) = keeping("cannot");
        let (url, served) = service(vec![(
            200,
            "cannot: a skill can't hear what is said.".to_owned(),
        )]);
        let told = make(
            "when someone says good night, dim the light slowly over a minute",
            &keeping,
            chat_at(url),
            NOW,
        );
        assert_eq!(
            told["error"], "the skill couldn't be made: a skill can't hear what is said.",
            "{told}"
        );
        assert_eq!(
            told["next"],
            made_skill::not_made_in_their_language(
                "I couldn't make that: a skill can't hear what is said."
            )
        );
        assert_eq!(
            take_said_line().as_deref(),
            Some("I couldn't make that: a skill can't hear what is said.")
        );
        assert_eq!(served.join().unwrap(), 1);
        assert!(made_skill::entries(&keeping.directory).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The owner, 10 Nov ("quăng cho deepseek hết đi, đừng if else"): what
    /// a skill may read or do is told to the model in plain words, and the
    /// model, not a word list, answers what it can't with `cannot:`. With
    /// reads on: one linked service at most, only read, only a listed one,
    /// with each listed one's step; off: none at all. Neither names a place
    /// to link one.
    #[test]
    fn the_system_prompt_carries_the_rules_of_a_read() {
        let on = system_with(true);
        for rule in [
            "A skill may read at most one of the person's linked services, and only read it",
            "never sends, writes, posts, books, schedules, deletes or changes anything",
            "to read two or more services, or to read one not listed here",
            "is answered with one line and no blocks: cannot: then one short sentence",
            "You can ask me for it yourself any time.",
            "Never tell them to link a service.",
            "services = { CODE = \"read\" }",
            "its trigger's during only for a time the request names",
        ] {
            assert!(on.contains(rule), "{rule}");
        }
        for step in made_skill::read_steps() {
            assert!(on.contains(&format!("\n- {step}")), "{step}");
        }
        assert!(on.contains(made_skill::CALENDAR_READ) && on.contains(made_skill::DRIVE_READ));
        assert!(on.contains("Notion") && on.contains("Slack"), "{on}");
        let off = system_with(false);
        assert!(off.contains(
            "cannot: Not possible yet: a skill can't use your mail, calendar or other services"
        ));
        assert!(!off.contains("services = {") && !off.contains(made_skill::GMAIL_READ));
        for prompt in [&on, &off] {
            assert!(!prompt.contains("Connectors"), "{prompt}");
            assert!(prompt.contains("cannot: then one short sentence said to the person"));
        }
    }

    /// Second's HOLD on #5985: the model's `cannot:` reason may echo the
    /// person's words; it is told to the person in full, and the journal
    /// says only that the model said no made skill can do it.
    #[test]
    fn the_models_cannot_reason_is_told_never_journaled() {
        let (root, keeping) = keeping("cannot-journal");
        let (url, served) = service(vec![(
            200,
            "cannot: a skill can't know when PRIVATE-DETAIL-7 calls you.".to_owned(),
        )]);
        let (made, line) = made_and_journaled(
            "glow when PRIVATE-DETAIL-7 calls",
            None,
            made_skill::brief("glow when PRIVATE-DETAIL-7 calls").unwrap(),
            &keeping,
            chat_at(url),
            "local:1:1",
        );
        assert_eq!(served.join().unwrap(), 1);
        let Err(Refused(why, _)) = &made else {
            panic!("{made:?}")
        };
        assert!(why.contains("PRIVATE-DETAIL-7"), "told in full: {why}");
        assert!(!line.contains("PRIVATE-DETAIL-7"), "{line}");
        assert!(
            line.ends_with(made_skill::MODELS_CANNOT_JOURNALED),
            "{line}"
        );
        assert!(made_skill::says_cannot("Cannot: no."));
        assert!(!made_skill::says_cannot("cannot: no.\n```toml\n```"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What the model is told it kept says what will run, from its
    /// `skill.toml`, never the model's description; and what was asked and
    /// left out is said with it (#5041: lamp-0c89's "good night" skill,
    /// kept as a quiet minute, told as what was asked).
    #[test]
    fn what_it_says_is_what_will_run_and_what_was_left_out() {
        assert!(system().contains("left out: what was left out"));
        let (root, keeping) = keeping("plainly");
        let answer = format!(
            "{}\nleft out: dimming slowly; it dims in one step\n",
            good()
        );
        let (url, served) = service(vec![(200, answer)]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(
            told["description"],
            "When the lamp goes quiet, between 22:00 and 22:30, it dims the light a step, at \
             most once every 12 hours. It starts on its own, at most 3 times a day, one an hour \
             with the lamp's other nudges, and rests till morning if ignored twice. The maker \
             notes what it couldn't do: dimming slowly.",
            "{told}"
        );
        assert!(!told.to_string().contains("10 pm"), "{told}");
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A lamp with no tasks part at all makes a skill by voice and puts it
    /// on: `make_skill` is the voice's own, the lamp's model writes it, and
    /// it goes on at once by its SHA-256 through the manage helper, with no
    /// yes asked for (the owner, 10 Nov). There is no function to put it on.
    #[test]
    fn with_no_tasks_part_a_skill_is_made_and_put_on_by_voice() {
        use crate::tools::{Route, Settings, route};
        let settings = Settings {
            make_skill: true,
            ..Settings::default()
        };
        assert!(settings.tasks.is_none());
        assert_eq!(route(&settings, made_skill::MAKE), Route::MakeLocal(None));
        assert_eq!(route(&settings, "put_on_made_skill"), Route::Nobody);
        let (root, keeping) = keeping("no-tasks");
        let (url, served) = service(vec![(200, good())]);
        let asked = manage_helper(&keeping.manage);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        // On before the helper is joined: on the old code nothing asks it.
        assert!(any_on(&keeping.directory));
        let sha256 = made_skill::entries(&keeping.directory)[0]["sha256"].clone();
        assert_eq!(asked.join().unwrap(), json!({"install_made": sha256}));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An answer without the two files is said as not made; nothing is
    /// kept and nothing is put on.
    #[test]
    fn a_refused_answer_says_so_and_keeps_nothing() {
        let (root, keeping) = keeping("refused");
        let (url, served) = service(vec![(200, "Sure! Here is a skill: dim at ten.".into())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .starts_with("the skill couldn't be made"),
            "{told}"
        );
        assert!(told.get("made").is_none());
        assert!(made_skill::entries(&keeping.directory).is_empty());
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Refused by the lamp's check, the check's lines go back to the model
    /// once, and its corrected answer is kept (lamp-9adf, 2 Nov 16:10: the
    /// model's first skill had a trigger field the check didn't know).
    #[test]
    fn a_skill_the_check_refuses_goes_back_to_the_model_once() {
        let (root, keeping) = keeping("repair");
        // The check refuses a skill.toml with a field it doesn't know.
        std::fs::write(
            &keeping.program,
            "#!/bin/sh
if grep -q every \"$4/skill.toml\"; then echo \"skill.toml: trigger: unknown field every\"; echo \"skill.toml: trigger: unknown field every\" >&2; exit 1; fi
name=$(basename \"$4\")
printf 'tar of %s' \"$name\" > \"$3/$name-1.0.0.tar\"\n",
        )
        .unwrap();
        let bad = good().replace("[trigger]\n", "[trigger]\nevery = \"90m\"\n");
        let (url, served) = service(vec![(200, bad.clone()), (200, good())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 2);
        // Refused every time: said as not made, plainly, after its last
        // repair, and asked no more.
        let (url, served) = service(vec![(200, bad.clone()); 1 + MOST_REPAIRS]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW + 1);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #8651 (leg M: "ninety nine days left in the year" written into a
    /// 6 AM say): every brief, the lamp's own model's and the agent's, says
    /// a line that changes is a task step that prints it, never a fixed say.
    #[test]
    fn the_brief_says_a_line_that_changes_is_never_a_fixed_say() {
        for (reads, agent) in [(false, false), (true, false), (false, true), (true, true)] {
            let system = system_for(reads, agent);
            assert!(system.contains(CHANGING_LINE), "{reads} {agent}");
        }
        assert!(CHANGING_LINE.contains("task step whose script prints it"));
    }

    /// The owner, 10 Nov: "re-run it yourself until it works". A skill the
    /// lamp can't say plainly (lamp-74ff's "come back to my desk" on the
    /// faces) goes back with the part named and the triggers it can have,
    /// up to [`MOST_REPAIRS`] times: twice refused, the third answer is
    /// kept; refused every time, the person is told plainly, with what to
    /// try.
    #[test]
    fn a_skill_it_cant_say_is_repaired_up_to_three_times() {
        let (root, keeping) = keeping("plainly-repairs");
        let faces = good().replace(
            "topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n",
            "topic = \"humans/faces/tracked\"\nnear = 0.14\n",
        );
        assert!(faces != good());
        let (url, served) = service(vec![
            (200, faces.clone()),
            (200, faces.clone()),
            (200, good()),
        ]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 3);
        let (url, served) = service(vec![(200, faces); 1 + MOST_REPAIRS]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW + 1);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
        );
        assert_eq!(
            told["next"],
            made_skill::not_made_in_their_language(made_skill::NOT_MADE_TRY)
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        assert_eq!(made_skill::entries(&keeping.directory).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A skill's answer the lamp can't say plainly: refused by the check
    /// with its lines, so a repair would follow.
    pub(crate) fn faces() -> String {
        good().replace(
            "topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n",
            "topic = \"humans/faces/tracked\"\nnear = 0.14\n",
        )
    }

    /// `LAMP_MAKE_SKILL_AGENT` (#6059's proposal): a stub tasks part whose
    /// `lamp` target runs Hermes, answering each hand-over from `answers`.
    fn agent_answering(answers: Vec<Option<String>>) -> crate::make_skill_agent::tests::StubTasks {
        let answers = std::sync::Mutex::new(answers.into_iter());
        crate::make_skill_agent::tests::stub(
            Some("hermes"),
            std::sync::Arc::new(move |rules: &str| {
                // The same system prompt and brief DeepSeek is sent; a
                // repair's with the check's refusal first (#8651).
                let rules = match rules.strip_prefix("FIX THIS FIRST. ") {
                    Some(led) => {
                        assert!(led.starts_with(made_skill::REPAIR_MARK), "{rules}");
                        led.split_once("\n\n").unwrap().1
                    }
                    None => rules,
                };
                assert!(rules.starts_with(&system()), "{rules}");
                assert!(rules.contains(made_skill::BRIEF_HEAD), "{rules}");
                answers.lock().unwrap().next().flatten()
            }),
        )
    }

    fn made_by(
        keeping: &Keeping,
        url: String,
        agent: Option<&crate::make_skill_agent::Agent>,
        now: u64,
    ) -> Value {
        make_with_by(0, None, REQUEST, keeping, chat_at(url), now, || None, agent)
    }

    /// With the switch and an agent on the lamp, DeepSeek's answer kept is
    /// the make: the agent is never asked.
    #[test]
    fn deepseek_kept_hands_nothing_to_the_agent() {
        let (root, keeping) = keeping("agent-not-asked");
        let tasks = agent_answering(vec![Some(good())]);
        let (url, served) = service(vec![(200, good())]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The owner, 11 Nov: with an agent on the lamp, DeepSeek gets one try
    /// and no repairs. Its one answer refused, the make goes to the agent at
    /// once, whose answer goes through the same checks and is kept; the
    /// agent gets the repairs.
    #[test]
    fn deepseek_refused_once_goes_to_the_agent_at_once_with_no_deepseek_repair() {
        let (root, keeping) = keeping("agent-asked");
        let tasks = agent_answering(vec![Some(faces()), Some(good())]);
        let (url, served) = service(vec![(200, faces())]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        // DeepSeek asked once, no repair; the agent's first answer refused
        // and repaired by the agent itself.
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 2);
        assert_eq!(made_skill::entries(&keeping.directory).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// DeepSeek's answer for "year count" (lamp-ee17, 26 Nov): a fixed say
    /// `words`, started by the clock at 6 AM when `timed` (else asked
    /// aloud), with the maker's "couldn't do" note when `note`.
    fn year_count(words: &str, timed: bool, note: bool) -> String {
        let start = if timed {
            "[permissions]\nspeak = true\ntopics = [\"clock/local\"]\n\n[trigger]\n\
             topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
             during = { from = \"06:00\", to = \"06:01\" }\n"
        } else {
            "[permissions]\nspeak = true\n"
        };
        let asks = if timed {
            ""
        } else {
            "asks = [\"year count\"]\n"
        };
        let note = if note {
            "left out: the exact count of days left spoken, as a dynamic count which is not \
             possible\n"
        } else {
            ""
        };
        format!(
            "```skill.toml\n{asks}kind = \"installable\"\nversion = \"1.0.0\"\n\n{start}\n\
             [[do]]\nsay = {{ text = \"{words}\" }}\n```\n\n```SKILL.md\n---\nname: year-count\n\
             description: Tells me the days left in the year.\n---\n\n# Year count\n\nIt says \
             the days left.\n```\n{note}"
        )
    }

    /// The first builder's ruling on #8651 (lamp-ee17, 26 Nov): a timed
    /// skill whose only speaking is a fixed say beside the maker's
    /// "couldn't do" note is never kept with its value dropped: handed to
    /// the agent to write as a task step, as a make that needs the
    /// internet is. The same say with no note, or asked aloud, is kept as
    /// DeepSeek's, the agent never asked.
    #[test]
    fn a_timed_fixed_say_with_a_couldnt_do_note_goes_to_the_agent() {
        let days = "There are days left in the year.";
        let dropped = year_count(days, true, true);
        assert!(made_skill::says_fixed_with_left_out(&dropped), "{dropped}");
        let (root, kept_in) = keeping("value-left-out");
        let tasks = agent_answering(vec![Some(good())]);
        let (url, served) = service(vec![(200, dropped)]);
        let told = made_by(&kept_in, url, Some(&tasks.agent()), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!((served.join().unwrap(), tasks.hand_overs()), (1, 1));
        let entries = made_skill::entries(&kept_in.directory);
        assert_eq!(entries.len(), 1, "{entries:?}");
        let _ = std::fs::remove_dir_all(&root);
        // With no agent to hand it to: kept as DeepSeek's, as before.
        let (root, kept_in) = keeping("value-left-out-no-agent");
        let (url, served) = service(vec![(200, year_count(days, true, true))]);
        let told = made_by(&kept_in, url, None, NOW);
        assert_eq!(told["made"], "year-count", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
        // The same make as `answer` would be, its hand-overs and what was
        // made, with an agent on the lamp.
        let make_of = |case: &str, answer: String| {
            let (root, kept_in) = keeping(&format!("value-left-out-{case}"));
            let tasks = agent_answering(vec![Some(good())]);
            let (url, served) = service(vec![(200, answer)]);
            let told = made_by(&kept_in, url, Some(&tasks.agent()), NOW);
            let _ = std::fs::remove_dir_all(&root);
            (
                served.join().unwrap(),
                tasks.hand_overs(),
                told["made"].clone(),
            )
        };
        // No note ("Time to stretch!"): kept as DeepSeek's, the agent not
        // asked.
        let stretch = year_count("Time to stretch!", true, false);
        assert!(!made_skill::says_fixed_with_left_out(&stretch));
        assert_eq!(make_of("no-note", stretch), (1, 0, json!("year-count")));
        // Asked aloud, no trigger: the note changes nothing, the make goes
        // as the same answer with no note goes.
        let aloud = year_count(days, false, true);
        assert!(!made_skill::says_fixed_with_left_out(&aloud), "{aloud}");
        assert_eq!(
            make_of("aloud-note", aloud),
            make_of("aloud", year_count(days, false, false))
        );
    }

    /// #7926's row on lamp-9adf (build bede187d81): "make a skill called
    /// rain check …", DeepSeek refused, the agent's answer kept long after
    /// the turn. Here its turn is gone before the agent answers, and the
    /// answer comes late: the name the person gave still holds, the make
    /// and its keep being on the one thread the name is held on.
    #[test]
    fn a_name_in_the_requests_words_is_the_makers_to_keep_too() {
        // #7926's row: with no `given_name` in the call, the make's model
        // and the agent (the same system prompt) are told to name the skill
        // as the request does; so is the model choosing a task's name.
        assert!(system().contains(NAMED_TOLD), "{}", system());
        for agent in [false, true] {
            assert!(system_for(true, agent).contains(NAMED_TOLD));
        }
        assert!(
            crate::make_no_cannot::system()
                .contains("the name the request gives the skill when it gives one"),
            "{}",
            crate::make_no_cannot::system()
        );
    }

    #[test]
    fn an_agent_make_kept_after_its_turn_ended_keeps_the_given_name() {
        let (root, kept_in) = keeping("given-name-late-agent");
        let late = good().replace("name: evening-dim", "name: morning-rain-check");
        let tasks = crate::make_skill_agent::tests::stub(
            Some("hermes"),
            std::sync::Arc::new(move |_: &str| {
                std::thread::sleep(Duration::from_millis(300));
                Some(late.clone())
            }),
        );
        let (url, served) = service(vec![(200, faces())]);
        let agent = tasks.agent();
        let turn = std::sync::Arc::new(());
        set_turn(std::sync::Arc::downgrade(&turn));
        drop(turn);
        assert!(turn_cut());
        let told = {
            let _given = GivenName::set(Some("rain check".into()));
            made_by(&kept_in, url, Some(&agent), NOW)
        };
        TURN.with(|kept| kept.borrow_mut().take());
        assert_eq!(told["made"], "rain-check", "{told}");
        assert_eq!((served.join().unwrap(), tasks.hand_overs()), (1, 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7911 (lamp-9adf, 19 Nov 12:46): "make a skill called evening rate"
    /// was kept as `nightly-dollar-rate`. The name the voice's model reads
    /// from the ask (`given_name`) is the made skill's, as the person said
    /// it, whether DeepSeek's answer is kept or the agent's; with none
    /// given, the make's own name stands, on both paths.
    #[test]
    fn a_name_the_person_gave_is_the_made_skills_by_deepseek_and_by_the_agent() {
        let nightly =
            |answer: String| answer.replace("name: evening-dim", "name: nightly-dollar-rate");
        for (case, given, made) in [
            ("given", Some("evening rate"), "evening-rate"),
            ("none", None, "nightly-dollar-rate"),
        ] {
            // DeepSeek's answer kept, no agent on the lamp; then DeepSeek's
            // refused and the agent's kept.
            for by_agent in [false, true] {
                let (root, kept_in) = keeping(&format!("given-name-{case}-{by_agent}"));
                let tasks = agent_answering(vec![Some(nightly(good()))]);
                let first = if by_agent { faces() } else { nightly(good()) };
                let (url, served) = service(vec![(200, first)]);
                let agent = tasks.agent();
                let _given = GivenName::set(given.map(str::to_owned));
                let told = made_by(&kept_in, url, by_agent.then_some(&agent), NOW);
                assert_eq!(told["made"], made, "{case} {by_agent}: {told}");
                assert_eq!(
                    (served.join().unwrap(), tasks.hand_overs()),
                    (1, usize::from(by_agent)),
                    "{case} {by_agent}"
                );
                let _ = std::fs::remove_dir_all(&root);
            }
        }
        // The voice's model is asked for it, and only for a new make.
        let parameters = parameters(false, false);
        let told = parameters["properties"][GIVEN_NAME]["description"]
            .as_str()
            .unwrap();
        assert_eq!(parameters["required"], json!([]));
        assert!(told.contains("else left out"), "{told}");
        // The name alone in its example, never "called …" (Twelve's P3).
        assert!(told.contains("(\"evening rate\")"), "{told}");
        assert!(!told.contains("\"called"), "{told}");
    }

    /// #7911 (Twelve's and Heron's P3s on #7926): `answer` holds the
    /// call's `given_name` for its make, trimmed, none for a blank one or
    /// none given; an undo's make ("make my own instead") the name given
    /// in its first ask, never the undo call's; cleared once it is done.
    #[test]
    fn answer_holds_the_calls_given_name_for_its_make_and_an_undos_first() {
        let held = |args: Value, undone: Option<&crate::make_core_offer::Turned>| {
            named_make(given_of(&args, undone), || {
                GIVEN.with(|given| given.borrow().clone())
            })
        };
        let undone = |given: Option<&str>| crate::make_core_offer::Turned {
            name: "sip-of-water".into(),
            request: "remind me to drink water".into(),
            given: given.map(str::to_owned),
            at: std::time::Instant::now(),
        };
        assert_eq!(
            held(json!({REQUEST: "hum", GIVEN_NAME: " evening rate "}), None).as_deref(),
            Some("evening rate")
        );
        assert_eq!(held(json!({REQUEST: "hum", GIVEN_NAME: "  "}), None), None);
        assert_eq!(held(json!({REQUEST: "hum"}), None), None);
        let first = undone(Some("water time"));
        let undo = json!({crate::make_core_offer::UNDO: true, GIVEN_NAME: "other"});
        assert_eq!(
            held(undo.clone(), Some(&first)).as_deref(),
            Some("water time")
        );
        assert_eq!(held(undo, Some(&undone(None))), None);
        assert_eq!(GIVEN.with(|given| given.borrow().clone()), None);
    }

    /// The rules of each make handed to the agent, and whether each asked
    /// for the guide.
    fn handed_rules(tasks: &crate::make_skill_agent::tests::StubTasks) -> Vec<(String, bool)> {
        tasks
            .sent
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line["op"] == "skill_make")
            .map(|line| {
                (
                    line["rules"].as_str().unwrap().to_owned(),
                    line["guide"] == true,
                )
            })
            .collect()
    }

    /// `LAMP_MAKE_AGENT_GUIDE` (the owner's ask, 13 Nov): every make asks
    /// for the guide; the agent's first answer, refused by the lamp's own
    /// check, goes back once more with the check's words, and its fixed
    /// answer is the one kept.
    #[test]
    fn with_the_guide_a_refused_answer_goes_back_with_the_checks_words_and_its_fix_is_kept() {
        let (root, keeping) = keeping("agent-guided");
        let tasks = agent_answering(vec![Some(faces()), Some(good())]);
        let (url, served) = service(vec![(200, faces())]);
        let told = made_by(&keeping, url, Some(&tasks.guided()), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        let rules = handed_rules(&tasks);
        assert_eq!(rules.len(), 2);
        assert!(rules.iter().all(|(_, guide)| *guide));
        assert!(
            !rules[0].0.contains(made_skill::REPAIR_MARK),
            "{}",
            rules[0].0
        );
        let (_, repair) = rules[1].0.split_once(made_skill::REPAIR_MARK).unwrap();
        assert!(repair.contains("humans/faces/tracked"), "{repair}");
        assert_eq!(made_skill::entries(&keeping.directory).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// With the guide, an agent's answer the lamp's check still refuses
    /// after [`crate::make_skill_agent::GUIDED_MOST_REPAIRS`] more rounds is
    /// refused as today: said as not made, nothing kept.
    #[test]
    fn with_the_guide_an_answer_still_refused_after_its_rounds_is_refused_as_today() {
        let (root, keeping) = keeping("agent-guided-refused");
        let tasks = agent_answering(vec![Some(faces()); 1 + MOST_REPAIRS]);
        let (url, served) = service(vec![(200, faces())]);
        let told = made_by(&keeping, url, Some(&tasks.guided()), NOW);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(
            tasks.hand_overs(),
            1 + crate::make_skill_agent::GUIDED_MOST_REPAIRS
        );
        assert!(made_skill::entries(&keeping.directory).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7874: a guided make's first answer refused with 35 s gone (the
    /// clock: a `make_within` 35 s short of [`crate::make_skill_agent::MAKE_WITHIN`]
    /// at once is the same time left) is repaired, though one more write
    /// may take its whole [`crate::make_skill_agent::GUIDED_WRITE_WITHIN`];
    /// with under [`REPAIR_AT_LEAST`] left, no repair is asked.
    #[test]
    fn a_guided_make_refused_at_35_s_is_repaired_and_none_with_under_its_floor_left() {
        let cases = [
            (
                "35s",
                crate::make_skill_agent::MAKE_WITHIN - Duration::from_secs(35),
                2,
            ),
            ("late", REPAIR_AT_LEAST - Duration::from_secs(1), 1),
        ];
        for (name, make_within, rounds) in cases {
            let (root, keeping) = keeping(&format!("agent-guided-repair-{name}"));
            let tasks = agent_answering(vec![Some(faces()), Some(good())]);
            let (url, served) = service(vec![(200, faces())]);
            let agent = crate::make_skill_agent::Agent {
                within: crate::make_skill_agent::GUIDED_WRITE_WITHIN,
                make_within,
                ..tasks.guided()
            };
            let told = made_by(&keeping, url, Some(&agent), NOW);
            assert_eq!(served.join().unwrap(), 1, "{name}");
            assert_eq!(tasks.hand_overs(), rounds, "{name}: {told}");
            match rounds {
                2 => assert_eq!(told["made"], "evening-dim", "{name}: {told}"),
                _ => assert!(told.get("made").is_none(), "{name}: {told}"),
            }
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// #8514, El's runs 1816 and 1843-A on lamp-ee17 (25 Nov): the agent's
    /// first answer, a say step the check refuses, came late. With too
    /// little of the make left for a repair it gets one hand-over; with a
    /// repair asked and cut at its deadline (no answer) it gets two. Either
    /// way the person hears the say step's refusal once, in its plain line
    /// (#8454's [`made_skill::NOT_MADE_TRY`], El's condition: one line),
    /// never the model's silence.
    #[test]
    fn a_late_refused_say_is_said_at_once_and_a_repair_cut_at_its_deadline_says_the_refusal() {
        // A say of two lines: refused as it came and after it is made plain.
        let says = good()
            .replace("light = true\n", "light = true\nspeak = true\n")
            .replace(
                "light = { dim = 1 }\n",
                "say = { text = [\"Good evening.\", \"Time to rest.\"] }\n",
            );
        let cases = [
            ("late", REPAIR_AT_LEAST - Duration::from_secs(1), 1),
            ("cut", crate::make_skill_agent::MAKE_WITHIN, 2),
        ];
        for (name, make_within, hand_overs) in cases {
            let (root, keeping) = keeping(&format!("agent-say-refused-{name}"));
            let tasks = agent_answering(vec![Some(says.clone()), None]);
            let (url, served) = service(vec![(200, faces())]);
            let agent = crate::make_skill_agent::Agent {
                within: crate::make_skill_agent::GUIDED_WRITE_WITHIN,
                make_within,
                ..tasks.guided()
            };
            let told = made_by(&keeping, url, Some(&agent), NOW);
            assert_eq!(served.join().unwrap(), 1, "{name}");
            assert_eq!(tasks.hand_overs(), hand_overs, "{name}: {told}");
            assert!(told.get("made").is_none(), "{name}: {told}");
            assert_eq!(
                told["error"],
                format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY),
                "{name}"
            );
            // One line said, once (El's condition on #8514): the error holds
            // it once, and the turn's instruction quotes it once to say;
            // never the silence beside it.
            for part in ["error", "next"] {
                let said = told[part].as_str().unwrap_or_default();
                assert_eq!(
                    said.matches(made_skill::NOT_MADE_TRY).count(),
                    1,
                    "{name}: {part}: {told}"
                );
            }
            let whole = told.to_string();
            assert!(!whole.contains(made_skill::MODEL_SILENT), "{name}: {told}");
            assert!(made_skill::entries(&keeping.directory).is_empty(), "{name}");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// #8610 (the first builder's 16:17): the agent's skill a trigger
    /// starts whose code would speak on its own goes back to the agent as
    /// the try's failure. Repaired to print for the told answer, it is kept
    /// and the person hears nothing of the refusal; never repaired, or its
    /// repair cut, the person hears [`made_skill::TOLD_ONLY_TRY`] once,
    /// never the model's silence.
    #[test]
    fn an_agent_skill_that_speaks_on_its_own_is_repaired_or_said_once() {
        if made_skill::a_python_here().is_none() {
            eprintln!("skipped: no python3 here to parse a made script with");
            return;
        }
        made_skill::set_scripts_here(Some(true));
        made_skill::set_code_at_once_here(Some(false));
        let code = |grants: &str, script: &str| {
            format!(
                "```skill.toml\n{}```\n\n```SKILL.md\n{AGENT_CODE_CARD}```\n\n\
                 ```scripts/main.py\n{script}```\n",
                CODE_TOML.replace("grants = [\"camera\"]", grants)
            )
        };
        let says = code(
            "grants = [\"camera\", \"speak\"]",
            "import asyncio\nfrom lampos import Lamp\n\n\nasync def main():\n    \
             lamp = await Lamp.local()\n    await lamp.say('A photo.', wait=True)\n    \
             await lamp.close()\n\n\nasyncio.run(main())\n",
        );
        let prints = code("grants = [\"camera\"]", "print('A photo was taken.')\n");
        let cases = [
            ("repaired", vec![Some(says.clone()), Some(prints)], 2, true),
            ("twice", vec![Some(says.clone()); 8], 0, false),
            ("cut", vec![Some(says), None], 2, false),
        ];
        for (name, answers, hand_overs, kept) in cases {
            let (root, keeping) = keeping(&format!("agent-told-only-{name}"));
            // Answered in turn; the rules are the code make's own here.
            let answers = std::sync::Mutex::new(answers.into_iter());
            let tasks = crate::make_skill_agent::tests::stub(
                Some("hermes"),
                std::sync::Arc::new(move |_: &str| answers.lock().unwrap().next().flatten()),
            );
            let (url, served) = service(vec![(200, faces())]);
            let agent = crate::make_skill_agent::Agent {
                within: crate::make_skill_agent::GUIDED_WRITE_WITHIN,
                make_within: crate::make_skill_agent::MAKE_WITHIN,
                ..tasks.guided()
            };
            let told = made_by(&keeping, url, Some(&agent), NOW);
            assert_eq!(served.join().unwrap(), 1, "{name}");
            if hand_overs > 0 {
                assert_eq!(tasks.hand_overs(), hand_overs, "{name}: {told}");
            } else {
                assert_eq!(
                    tasks.hand_overs(),
                    1 + agent.most_repairs(),
                    "{name}: {told}"
                );
            }
            let whole = told.to_string();
            if kept {
                assert_eq!(told["made"], "photo-booth", "{name}: {told}");
                assert!(!whole.contains(made_skill::TOLD_ONLY_TRY), "{name}: {told}");
                assert!(!whole.contains("speak on its own"), "{name}: {told}");
                assert_eq!(made_skill::entries(&keeping.directory).len(), 1, "{name}");
            } else {
                assert!(told.get("made").is_none(), "{name}: {told}");
                assert_eq!(
                    told["error"],
                    format!("the skill couldn't be made: {}", made_skill::TOLD_ONLY_TRY),
                    "{name}"
                );
                for part in ["error", "next"] {
                    let said = told[part].as_str().unwrap_or_default();
                    assert_eq!(
                        said.matches(made_skill::TOLD_ONLY_TRY).count(),
                        1,
                        "{name}: {part}: {told}"
                    );
                }
                assert!(!whole.contains(made_skill::MODEL_SILENT), "{name}: {told}");
                assert!(made_skill::entries(&keeping.directory).is_empty(), "{name}");
            }
            let _ = std::fs::remove_dir_all(&root);
        }
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
    }

    /// The first builder's guard on #6643: when lamp-tasks can't run the
    /// agent in a skill run's namespace (an older build with no make
    /// socket, or held, a wrapper that can't hold it off), never handed over:
    /// DeepSeek's failure stands, said plainly, and nothing is kept.
    #[test]
    fn a_make_whose_make_run_is_refused_stays_deepseeks() {
        for engine in ["old", "not_held_off", "no_namespace"] {
            let (root, keeping) = keeping(&format!("agent-not-held-{engine}"));
            let tasks = crate::make_skill_agent::tests::stub(
                Some(engine),
                std::sync::Arc::new(|_: &str| Some(good())),
            );
            let (url, served) = service(vec![(200, faces())]);
            let told = made_by(&keeping, url, Some(&tasks.agent()), NOW);
            assert!(told.get("made").is_none(), "{told}");
            assert_eq!(
                told["error"],
                format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
            );
            assert_eq!(served.join().unwrap(), 1);
            assert!(made_skill::entries(&keeping.directory).is_empty());
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// #7245: a make lamp-tasks refuses `not_ready` (Harness or the gateway
    /// not ready yet) is told to the person at once in the lamp's short
    /// words, never DeepSeek's answer, and nothing is kept.
    #[test]
    fn a_make_not_ready_is_said_at_once_in_short_words() {
        let (root, keeping) = keeping("agent-not-ready");
        let tasks = crate::make_skill_agent::tests::stub(
            Some("not_ready"),
            std::sync::Arc::new(|_: &str| Some(good())),
        );
        let (url, served) = service(vec![(200, faces())]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_READY_LINE)
        );
        assert_eq!(
            made_skill::not_made_line(made_skill::NOT_READY_LINE),
            made_skill::NOT_READY_LINE
        );
        assert_eq!(tasks.hand_overs(), 1);
        assert_eq!(served.join().unwrap(), 1);
        assert!(made_skill::entries(&keeping.directory).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #6643's 19:14 ruling: a make is handed to Hermes alone, the one
    /// engine with a namespace runner for it today. With OpenClaw as the
    /// lamp's agent nothing is handed over: DeepSeek keeps its repairs and
    /// its answer stands, as with no agent.
    #[test]
    fn a_make_is_never_handed_to_openclaw() {
        let (root, keeping) = keeping("agent-openclaw");
        let openclaw = crate::make_skill_agent::tests::stub(
            Some("openclaw"),
            std::sync::Arc::new(|_: &str| Some(good())),
        );
        let (url, served) = service(vec![(200, faces()); 1 + MOST_REPAIRS]);
        let told = made_by(&keeping, url, Some(&openclaw.agent()), NOW);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        assert_eq!(openclaw.hand_overs(), 0);
        assert!(made_skill::entries(&keeping.directory).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A service skill made as a task for the lamp's agent: its switch
    /// (`LAMP_AGENT_SERVICES`) and a made skill's code on, or off.
    fn services_through_agent(on: Option<bool>) {
        made_skill::set_scripts_here(on);
        made_skill::set_agent_services_here(on);
    }

    /// A chat that counts each time DeepSeek is asked, and is never had.
    fn never_had() -> (
        impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = std::sync::Arc::clone(&asked);
        let chat = move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err("DeepSeek was asked".to_owned())
        };
        (chat, asked)
    }

    /// The Hermes agent of a stub tasks part, answering each hand-over with
    /// `answer` once it holds the service prompt (`agent_services`): the
    /// prompt is this thread's, so it is not asked of the stub's.
    fn hermes_with_services(answer: Option<String>) -> crate::make_skill_agent::tests::StubTasks {
        crate::make_skill_agent::tests::stub(
            Some("hermes"),
            std::sync::Arc::new(move |rules: &str| {
                assert!(rules.contains(made_skill::BRIEF_HEAD), "{rules}");
                assert!(
                    rules.contains("lamp-services tools (services_connected"),
                    "{rules}"
                );
                answer.clone()
            }),
        )
    }

    /// The owner's ask, 13 Nov (lamp-9adf: 3 of 3 voice makes that send
    /// mail refused by DeepSeek, never handed over): with
    /// `LAMP_MAKE_SKILL_AGENT` and `LAMP_AGENT_SERVICES` on, a make whose
    /// words ask for a linked service goes straight to the agent, DeepSeek
    /// never asked, by the person's line or the model's request alike.
    #[test]
    fn a_make_asking_for_a_linked_service_goes_straight_to_the_agent() {
        services_through_agent(Some(true));
        for (n, (request, heard)) in [
            ("when I leave my desk, email me that I'm out", None),
            ("every morning at eight, read me my new mail", None),
            (
                "every evening at six, tell me what's on my calendar tomorrow",
                None,
            ),
            (
                "make a skill that tells the owner the day's plan at eight",
                Some("every morning at eight read me my meetings"),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let (root, keeping) = keeping(&format!("service-straight-{n}"));
            let tasks = hermes_with_services(Some(good()));
            let (chat, asked) = never_had();
            let told = make_with_by(
                0,
                heard,
                request,
                &keeping,
                chat,
                NOW + n as u64,
                || None,
                Some(&tasks.agent()),
            );
            assert_eq!(told["made"], "evening-dim", "{request}: {told}");
            assert_eq!(
                asked.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "{request}"
            );
            assert_eq!(tasks.hand_overs(), 1, "{request}");
            let _ = std::fs::remove_dir_all(&root);
        }
        services_through_agent(None);
    }

    /// With both switches on, a make naming no linked service still tries
    /// DeepSeek first, and DeepSeek's kept skill is the make, as before.
    #[test]
    fn a_make_naming_no_service_still_tries_deepseek_first() {
        services_through_agent(Some(true));
        let (root, keeping) = keeping("service-none");
        let tasks = hermes_with_services(Some(good()));
        let (url, served) = service(vec![(200, good())]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 0);
        services_through_agent(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Either switch off (`LAMP_MAKE_SKILL_AGENT`, no agent given; or
    /// `LAMP_AGENT_SERVICES`, or a made skill's code), a make that sends
    /// mail tries DeepSeek first, as today.
    #[test]
    fn with_either_switch_off_a_service_make_tries_deepseek_first() {
        const MAIL: &str = "when I leave my desk, email me that I'm out";
        let (root, keeping) = keeping("service-off");
        let tasks = agent_answering(vec![Some(good())]);
        for (n, scripts, services) in [(0, true, false), (1, false, true), (2, false, false)] {
            made_skill::set_scripts_here(Some(scripts));
            made_skill::set_agent_services_here(Some(services));
            let (url, served) = service(vec![(200, good())]);
            let told = make_with_by(
                0,
                None,
                MAIL,
                &keeping,
                chat_at(url),
                NOW + n,
                || None,
                Some(&tasks.agent()),
            );
            assert_eq!(told["made"], "evening-dim", "{told}");
            assert_eq!(served.join().unwrap(), 1);
        }
        assert_eq!(tasks.hand_overs(), 0);
        services_through_agent(Some(true));
        let (url, served) = service(vec![(200, good())]);
        let told = make_with_by(
            0,
            None,
            MAIL,
            &keeping,
            chat_at(url),
            NOW + 3,
            || None,
            None,
        );
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        services_through_agent(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A make sent straight to the agent that the agent gives no answer
    /// for (no make run here, or none within its 45 s, short here) gets
    /// DeepSeek's one try, given no repairs, as a make DeepSeek failed
    /// first has its one answer stand.
    #[test]
    fn a_service_make_the_agent_gives_no_answer_for_gets_deepseeks_one_try() {
        const MAIL: &str = "every morning at eight, read me my new mail";
        services_through_agent(Some(true));
        let (root, keeping) = keeping("service-no-agent");
        let old = crate::make_skill_agent::tests::stub(
            Some("old"),
            std::sync::Arc::new(|_: &str| Some(good())),
        );
        let silent = hermes_with_services(None);
        // No make run: DeepSeek's failure as it is; the agent silent: said
        // as the agent's (the audit's A3, 16 Nov).
        for (n, agent, handed, said) in [
            (0, &old, 0, made_skill::NOT_MADE_TRY),
            (1, &silent, 1, made_skill::AGENT_LATE_TRY),
        ] {
            let (url, served) = service(vec![(200, faces())]);
            let told = make_with_by(
                0,
                None,
                MAIL,
                &keeping,
                chat_at(url),
                NOW + n,
                || None,
                Some(&agent.agent()),
            );
            assert!(told.get("made").is_none(), "{told}");
            assert_eq!(told["error"], format!("the skill couldn't be made: {said}"));
            assert_eq!(served.join().unwrap(), 1);
            assert_eq!(agent.hand_overs(), handed);
        }
        let (url, served) = service(vec![(200, good())]);
        let told = make_with_by(
            0,
            None,
            MAIL,
            &keeping,
            chat_at(url),
            NOW + 2,
            || None,
            Some(&old.agent()),
        );
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        services_through_agent(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A DeepSeek `cannot:` of a send the brief asks for
    /// (`made_skill::BRIEFS_SEND_REFUSALS`), on a make whose words named no
    /// service, goes to the agent when it makes service skills; without
    /// that it is said as it is, and the brief's other refusals always are.
    #[test]
    fn deepseeks_cannot_of_a_send_goes_to_the_agent_that_makes_service_skills() {
        const TELL: &str = "when I leave my desk, tell Minh that I'm out";
        let send = "cannot: I can't do that yet: sending a message. You could ask for a glow.";
        let hear = "cannot: I can't do that yet: hearing what is said.";
        let (root, keeping) = keeping("service-cannot");
        let by = |answer: &str, now: u64| {
            let tasks = hermes_with_services(Some(good()));
            let (url, served) = service(vec![(200, answer.to_owned())]);
            let told = make_with_by(
                0,
                None,
                TELL,
                &keeping,
                chat_at(url),
                now,
                || None,
                Some(&tasks.agent()),
            );
            (told, served.join().unwrap(), tasks.hand_overs())
        };
        services_through_agent(Some(true));
        let (told, served, handed) = by(send, NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!((served, handed), (1, 1));
        let (told, served, handed) = by(hear, NOW + 1);
        assert!(told.get("made").is_none(), "{told}");
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .contains("hearing what is said"),
            "{told}"
        );
        assert_eq!((served, handed), (1, 0));
        made_skill::set_agent_services_here(Some(false));
        let tasks = agent_answering(vec![Some(good())]);
        let (url, served) = service(vec![(200, send.to_owned())]);
        let told = make_with_by(
            0,
            None,
            TELL,
            &keeping,
            chat_at(url),
            NOW + 2,
            || None,
            Some(&tasks.agent()),
        );
        assert!(told.get("made").is_none(), "{told}");
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .contains("sending a message"),
            "{told}"
        );
        assert_eq!((served.join().unwrap(), tasks.hand_overs()), (1, 0));
        services_through_agent(None);
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            made_skill::BRIEFS_SEND_REFUSALS
                .iter()
                .all(|refusal| made_skill::BRIEFS_OWN_REFUSALS.contains(refusal))
        );
        assert!(made_skill::briefs_send_refusal(
            "I can't do that yet: sending or writing on its own."
        ));
        assert!(!made_skill::briefs_send_refusal(
            "I can't do that yet: deleting or changing what is in your services."
        ));
    }

    /// With the switch but no agent on the lamp, DeepSeek keeps its
    /// repairs and its failure stands, as without the switch; with the
    /// agent never answering (its 45 s, short here), DeepSeek's failure
    /// stands too.
    #[test]
    fn no_agent_or_a_silent_one_leaves_deepseeks_failure_as_today() {
        let (root, keeping) = keeping("agent-none");
        let none = crate::make_skill_agent::tests::stub(None, std::sync::Arc::new(|_: &str| None));
        let (url, served) = service(vec![(200, faces()); 1 + MOST_REPAIRS]);
        let told = made_by(&keeping, url, Some(&none.agent()), NOW);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        assert_eq!(none.hand_overs(), 0);
        // Silent past its time: DeepSeek's failure stands, said as the
        // agent's (the audit's A3, 16 Nov), never the tip to ask for one
        // thing.
        let silent = agent_answering(vec![None]);
        let (url, served) = service(vec![(200, faces())]);
        let told = made_by(&keeping, url, Some(&silent.agent()), NOW + 1);
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::AGENT_LATE_TRY)
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(silent.hand_overs(), 1);
        assert!(made_skill::entries(&keeping.directory).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The audit's A3 (16 Nov): the agent busy with another make, or its
    /// run failed, is the agent's side: the person hears so, and what to
    /// do, never "Try asking for one thing"; told to the model whole.
    #[test]
    fn an_agent_busy_or_failed_is_said_as_the_agents_never_the_tip() {
        let (root, keeping) = keeping("agent-busy");
        for (n, engine, said) in [
            (0, "busy", made_skill::AGENT_BUSY_TRY),
            (1, "run_failed", made_skill::AGENT_FAILED_TRY),
        ] {
            let tasks = crate::make_skill_agent::tests::stub(
                Some(engine),
                std::sync::Arc::new(|_: &str| Some(good())),
            );
            let (url, served) = service(vec![(200, faces())]);
            let told = made_by(&keeping, url, Some(&tasks.agent()), NOW + n);
            assert!(told.get("made").is_none(), "{told}");
            assert_eq!(
                told["error"],
                format!("the skill couldn't be made: {said}"),
                "{engine}"
            );
            assert_eq!(told["next"], made_skill::not_made_in_their_language(said));
            assert_eq!(served.join().unwrap(), 1);
            assert_eq!(tasks.hand_overs(), 1);
        }
        assert!(made_skill::entries(&keeping.directory).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The Reviewer-lead on #6439: a DeepSeek `cannot:` the system prompt
    /// asks for is said as it is and never handed to the agent; any other
    /// goes to the agent.
    #[test]
    fn the_briefs_own_refusals_stay_deepseeks_and_others_go_to_the_agent() {
        let (root, keeping) = keeping("agent-cannot");
        let tasks = agent_answering(vec![Some(good())]);
        let own =
            "cannot: I can't do that yet: hearing what is said. You could ask for a glow instead.";
        let (url, served) = service(vec![(200, own.to_owned())]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW);
        assert!(told.get("made").is_none(), "{told}");
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .contains("hearing what is said"),
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 0);
        let other = "cannot: a skill can't watch the weather.";
        let (url, served) = service(vec![(200, other.to_owned())]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW + 1);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7245 (lamp-c0a4, 16 Nov 17:24): DeepSeek *refuses* a file-writing
    /// make with the brief's "Not possible yet: a skill can't". With the one
    /// run path on, that refusal goes to the lamp's agent; off, it stays
    /// DeepSeek's as before, and the brief's other refusals stay with it on.
    #[test]
    fn with_the_one_path_a_skill_cant_refusal_goes_to_the_agent() {
        let (root, keeping) = keeping("agent-skill-cant");
        let cant = "cannot: Not possible yet: a skill can't write a file. You could ask for a reminder instead.";
        let tasks = agent_answering(vec![Some(good())]);
        let off = tasks.agent();
        let (url, served) = service(vec![(200, cant.to_owned())]);
        let told = made_by(&keeping, url, Some(&off), NOW);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 0, "off: DeepSeek's refusal stands");
        let on = crate::make_skill_agent::Agent {
            one_path: true,
            ..tasks.agent()
        };
        let own =
            "cannot: I can't do that yet: hearing what is said. You could ask for a glow instead.";
        let (url, served) = service(vec![(200, own.to_owned())]);
        let told = made_by(&keeping, url, Some(&on), NOW + 1);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 0, "on: the brief's other refusals stay");
        let (url, served) = service(vec![(200, cant.to_owned())]);
        let told = made_by(&keeping, url, Some(&on), NOW + 2);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 1, "on: the agent makes it");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The first builder's point 3 on #6811 for #7375: with the one run
    /// path on and no agent on the lamp (no `lamp` target), a make DeepSeek
    /// keeps is the make, the brief's own refusal stays as it is, and one
    /// DeepSeek fails after its repairs is told in one sentence that the
    /// lamp has no agent to make it, never the tip to ask for one thing.
    #[test]
    fn with_the_one_path_and_no_agent_a_failed_make_says_so() {
        let (root, keeping) = keeping("one-path-no-agent");
        let none = crate::make_skill_agent::tests::stub(None, std::sync::Arc::new(|_: &str| None));
        let on = crate::make_skill_agent::Agent {
            one_path: true,
            ..none.agent()
        };
        let (url, served) = service(vec![(200, good())]);
        let told = made_by(&keeping, url, Some(&on), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        let own =
            "cannot: I can't do that yet: hearing what is said. You could ask for a glow instead.";
        let (url, served) = service(vec![(200, own.to_owned())]);
        let told = made_by(&keeping, url, Some(&on), NOW + 1);
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .contains("hearing what is said"),
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 1);
        let (url, served) = service(vec![(200, faces()); 1 + MOST_REPAIRS]);
        let told = made_by(&keeping, url, Some(&on), NOW + 2);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::AGENT_MISSING)
        );
        assert_eq!(
            told["next"],
            made_skill::not_made_in_their_language(made_skill::AGENT_MISSING)
        );
        // DeepSeek keeps its repairs, as without the agent.
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        assert_eq!(none.hand_overs(), 0);
        assert_eq!(made_skill::entries(&keeping.directory).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7375: with the one run path on, a service skill made as a task for
    /// the agent and no agent on the lamp, a make asking for a linked
    /// service is told at once that the lamp has no agent to make it,
    /// DeepSeek never asked: never a kept skill whose task no agent runs.
    /// With the one run path off, DeepSeek makes it alone, as before.
    #[test]
    fn with_the_one_path_and_no_agent_a_service_make_says_so_at_once() {
        const MAIL: &str = "every morning at eight, read me my new mail";
        services_through_agent(Some(true));
        let (root, keeping) = keeping("one-path-no-agent-service");
        let none = crate::make_skill_agent::tests::stub(None, std::sync::Arc::new(|_: &str| None));
        let on = crate::make_skill_agent::Agent {
            one_path: true,
            ..none.agent()
        };
        let (chat, asked) = never_had();
        let told = make_with_by(0, None, MAIL, &keeping, chat, NOW, || None, Some(&on));
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::AGENT_MISSING)
        );
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(none.hand_overs(), 0);
        let (url, served) = service(vec![(200, good())]);
        let told = make_with_by(
            0,
            None,
            MAIL,
            &keeping,
            chat_at(url),
            NOW + 1,
            || None,
            Some(&none.agent()),
        );
        assert_eq!(told["made"], "evening-dim", "off: {told}");
        assert_eq!(served.join().unwrap(), 1);
        services_through_agent(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7375 (the P2 of an adversarial read): with the one run path on and
    /// lamp-tasks not saying its targets (refused `ok:false`, or no socket
    /// as while it restarts), the lamp is not known to have no agent: a
    /// make DeepSeek fails is told today's failure, as with no agent at
    /// all, never AGENT_MISSING.
    #[test]
    fn with_the_one_path_and_the_targets_unknown_a_failed_make_is_not_told_no_agent() {
        let (root, keeping) = keeping("one-path-targets-unknown");
        let (url, served) = service(vec![(200, faces()); 1 + MOST_REPAIRS]);
        let today = made_by(&keeping, url, None, NOW);
        assert!(today.get("made").is_none(), "{today}");
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        let refused = crate::make_skill_agent::tests::stub(
            Some(crate::make_skill_agent::tests::UNANSWERED),
            std::sync::Arc::new(|_: &str| None),
        );
        let (_gone, gone) = crate::make_skill_agent::tests::no_socket();
        for (n, base) in [refused.agent(), gone].into_iter().enumerate() {
            let on = crate::make_skill_agent::Agent {
                one_path: true,
                ..base
            };
            let (url, served) = service(vec![(200, faces()); 1 + MOST_REPAIRS]);
            let told = made_by(&keeping, url, Some(&on), NOW + 1 + n as u64);
            assert!(told.get("made").is_none(), "{told}");
            assert_eq!(told["error"], today["error"], "{n}: {told}");
            assert!(
                !told["error"]
                    .as_str()
                    .unwrap()
                    .contains(made_skill::AGENT_MISSING),
                "{n}: {told}"
            );
            assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS, "{n}");
        }
        assert_eq!(refused.hand_overs(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7375: with the one run path on, a service skill made as a task for
    /// the agent and lamp-tasks not saying its targets, a make asking for
    /// a linked service is not refused at once: DeepSeek is asked and
    /// makes it, as before.
    #[test]
    fn with_the_one_path_and_the_targets_unknown_a_service_make_is_not_refused_at_once() {
        const MAIL: &str = "every morning at eight, read me my new mail";
        services_through_agent(Some(true));
        let refused = crate::make_skill_agent::tests::stub(
            Some(crate::make_skill_agent::tests::UNANSWERED),
            std::sync::Arc::new(|_: &str| None),
        );
        let (_gone, gone) = crate::make_skill_agent::tests::no_socket();
        for (n, base) in [refused.agent(), gone].into_iter().enumerate() {
            let (root, keeping) = keeping(&format!("one-path-targets-unknown-service-{n}"));
            let on = crate::make_skill_agent::Agent {
                one_path: true,
                ..base
            };
            let (url, served) = service(vec![(200, good())]);
            let told = make_with_by(
                0,
                None,
                MAIL,
                &keeping,
                chat_at(url),
                NOW + n as u64,
                || None,
                Some(&on),
            );
            assert_eq!(told["made"], "evening-dim", "{n}: {told}");
            assert_eq!(served.join().unwrap(), 1, "{n}");
            let _ = std::fs::remove_dir_all(&root);
        }
        assert_eq!(refused.hand_overs(), 0);
        services_through_agent(None);
    }

    /// Each of the brief's own refusals is a sentence the system prompt
    /// asks for, word for word, so the list can't drift from it.
    #[test]
    fn each_of_the_briefs_own_refusals_is_in_the_system_prompt() {
        let today = ("2026-11-11".to_owned(), "+07:00".to_owned());
        let prompts = [
            system_with(true),
            system_with(false),
            services(false, false, &[], None),
            services(true, false, &[], None),
            services(false, true, &[], Some(&today)),
            services(true, true, &[], None),
            agent_services(),
        ]
        .join("\n");
        for refusal in made_skill::BRIEFS_OWN_REFUSALS {
            assert!(prompts.contains(refusal), "{refusal}");
            assert!(made_skill::briefs_own_refusal(&format!(
                "I can't do that yet: {refusal}."
            )));
        }
        assert!(made_skill::briefs_own_refusal(
            "I can't do that yet: hearing what is said."
        ));
        assert!(made_skill::briefs_own_refusal(
            "I can't do that yet: knowing who someone is."
        ));
        assert!(!made_skill::briefs_own_refusal(
            "a skill can't watch the weather"
        ));
        // What a skill can't find out at run time is asked for as a
        // `cannot:` that is none of them, so it goes to the agent.
        for reads in [true, false] {
            let prompt = system_with(reads);
            assert!(
                prompt.contains("the weather or a forecast, the news"),
                "{prompt}"
            );
        }
        assert!(!made_skill::briefs_own_refusal(
            "I can't do that yet: checking the weather for you."
        ));
    }

    /// Lamp-9adf, 12 Nov 18:10: "check the weather and tell me if I need an
    /// umbrella" was kept as DeepSeek's, a skill that only says "Check the
    /// weather now, please." Such a say is not kept and the make goes to
    /// the agent; a reminder the person asked to be told is still kept as
    /// DeepSeek's.
    #[test]
    fn a_say_handing_the_ask_back_goes_to_the_agent_and_a_reminder_stays() {
        const UMBRELLA: &str = "make me a skill: every morning at eight, check the weather and \
                                tell me if I need an umbrella";
        let umbrella = |say: &str| {
            format!(
                "```skill.toml\nkind = \"installable\"\nversion = \"1.0.0\"\nneeds = [\"weather\"]\n\n\
                 [permissions]\nlight = true\nbody = true\nspeak = true\n\
                 topics = [\"clock/local\", \"weather\"]\n\n[trigger]\ntopic = \"clock/local\"\n\
                 field = \"known\"\nequals = true\nduring = {{ from = \"08:00\", to = \"08:01\" }}\n\
                 cooldown = \"23h\"\nrearm = \"ends\"\n\n[ask]\npolicy = \"nudge\"\n\n[[do]]\n\
                 say = {{ text = \"{say}\" }}\n```\n\n```SKILL.md\n---\nname: morning-umbrella\n\
                 description: Says a line about the weather at eight.\n---\n\n# Umbrella\n\nAt \
                 eight it speaks.\n```\n"
            )
        };
        let by = |request: &str, answer: String, name: &str, now: u64| {
            let (root, keeping) = keeping(name);
            let tasks = agent_answering(vec![Some(good())]);
            let (url, served) = service(vec![(200, answer)]);
            let told = make_with_by(
                0,
                None,
                request,
                &keeping,
                chat_at(url),
                now,
                || None,
                Some(&tasks.agent()),
            );
            let _ = std::fs::remove_dir_all(&root);
            (told, served.join().unwrap(), tasks.hand_overs())
        };
        let (told, served, prepares) = by(
            UMBRELLA,
            umbrella("Check the weather now, please."),
            "echo-umbrella",
            NOW,
        );
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!((served, prepares), (1, 1));
        let (told, served, prepares) = by(
            "remind me to drink water every hour",
            umbrella("Time to drink some water."),
            "echo-water",
            NOW + 1,
        );
        assert_eq!(told["made"], "morning-umbrella", "{told}");
        assert_eq!((served, prepares), (1, 0));
    }

    /// The Reviewer-lead on #6439: the agent's own `cannot:` is never said
    /// as it wrote it; the lamp says its own line.
    #[test]
    fn the_agents_own_cannot_is_said_as_the_lamps_line() {
        let (root, keeping) = keeping("agent-own-cannot");
        let tasks = agent_answering(vec![Some(
            "cannot: AGENT-WORDS-9 can't be done by a skill.".to_owned(),
        )]);
        let (url, served) = service(vec![(200, faces())]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW);
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 1);
        let said = told.to_string();
        assert!(!said.contains("AGENT-WORDS-9"), "{said}");
        assert!(
            said.contains(crate::make_skill_agent::CANNOT_LINE),
            "{said}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7245's one run path agent, its `make_within` as given.
    fn one_path_agent(
        tasks: &crate::make_skill_agent::tests::StubTasks,
        make_within: Duration,
    ) -> crate::make_skill_agent::Agent {
        crate::make_skill_agent::Agent {
            one_path: true,
            make_within,
            ..tasks.agent()
        }
    }

    /// A made-up reason, as OpenClaw's model gave on lamp-c0a4 (18 Nov).
    const MADE_UP_CANNOT: &str =
        "cannot: I can't do that yet: knowing facts about apples on my own.";

    /// #7245: the agent's `cannot:` naming no real limit of the lamp's is
    /// asked once more, its reason quoted back, and the skill it then
    /// makes is kept: two rounds.
    #[test]
    fn a_made_up_cannot_is_asked_again_once_and_its_skill_kept() {
        let (root, keeping) = keeping("agent-cannot-again");
        let tasks = agent_answering(vec![Some(MADE_UP_CANNOT.to_owned()), Some(good())]);
        let (url, served) = service(vec![(200, faces())]);
        let agent = one_path_agent(&tasks, crate::make_skill_agent::MAKE_WITHIN);
        let told = made_by(&keeping, url, Some(&agent), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 2);
        let rules = handed_rules(&tasks);
        assert!(
            rules[1]
                .0
                .contains("You answered cannot: I can't do that yet: knowing facts about apples"),
            "{}",
            rules[1].0
        );
        assert!(rules[1].0.contains(CANNOT_AGAIN_CAN), "{}", rules[1].0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7245: a `cannot:` naming a real limit (a look-up on the web) is
    /// never asked again; two made-up ones are refused after two rounds;
    /// with under [`CANNOT_AGAIN_LEAST`] left, never asked again.
    #[test]
    fn a_real_limits_or_second_cannot_or_one_with_no_time_left_is_refused_as_today() {
        let cases = [
            (
                "real",
                vec![Some(
                    "cannot: I can't do that yet: looking up the web.".to_owned(),
                )],
                crate::make_skill_agent::MAKE_WITHIN,
                1,
            ),
            (
                "twice",
                vec![
                    Some(MADE_UP_CANNOT.to_owned()),
                    Some(MADE_UP_CANNOT.to_owned()),
                ],
                crate::make_skill_agent::MAKE_WITHIN,
                2,
            ),
            (
                "late",
                vec![Some(MADE_UP_CANNOT.to_owned()), Some(good())],
                CANNOT_AGAIN_LEAST - Duration::from_secs(1),
                1,
            ),
        ];
        for (name, answers, make_within, rounds) in cases {
            let (root, keeping) = keeping(&format!("agent-cannot-{name}"));
            let tasks = agent_answering(answers);
            let (url, served) = service(vec![(200, faces())]);
            let agent = one_path_agent(&tasks, make_within);
            let told = made_by(&keeping, url, Some(&agent), NOW);
            assert!(told.get("made").is_none(), "{name}: {told}");
            assert!(
                told.to_string()
                    .contains(crate::make_skill_agent::CANNOT_LINE),
                "{name}: {told}"
            );
            assert_eq!(served.join().unwrap(), 1, "{name}");
            assert_eq!(tasks.hand_overs(), rounds, "{name}");
            assert!(made_skill::entries(&keeping.directory).is_empty(), "{name}");
            let _ = std::fs::remove_dir_all(&root);
        }
        // Hearing the room is a real limit with asks off alone.
        let heard = "I can't do that yet: hearing what is said in the room";
        made_skill::set_asks_here(Some(false));
        assert!(made_skill::real_limit(heard));
        made_skill::set_asks_here(Some(true));
        assert!(!made_skill::real_limit(heard));
        made_skill::set_asks_here(None);
        for made_up in [
            "knowing facts about apples on my own",
            "writing notes and reading them back",
            "writing to a file you can open",
        ] {
            assert!(!made_skill::real_limit(made_up), "{made_up}");
        }
    }

    /// The person's own addresses never reach the agent's rules file: each
    /// is a placeholder there, put back in its answer.
    #[test]
    fn own_addresses_are_placeholders_for_the_agent() {
        let own = vec![
            ("gmail".to_owned(), "a@b.co".to_owned()),
            ("outlook".to_owned(), "xa@b.co".to_owned()),
            ("slack".to_owned(), "T123".to_owned()),
        ];
        let hidden = placeholders(&own);
        assert_eq!(hidden.len(), 2);
        let text = "to a@b.co and xa@b.co";
        let hid = hide(text, &hidden);
        assert!(!hid.contains("a@b.co"), "{hid}");
        assert_eq!(shown(&hid, &hidden), text);
    }

    /// The readers' P2 on #6054: a repair is begun only while a whole
    /// round, its answer asked once more at worst, still ends within
    /// [`MAKE_WITHIN`]; at 89 s, as at 61 s, none is.
    #[test]
    fn a_repair_is_begun_only_while_two_answers_still_fit() {
        assert_eq!(ROUND_WITHIN, 2 * WRITE_WITHIN);
        assert!(round_fits(Duration::ZERO));
        assert!(round_fits(MAKE_WITHIN - ROUND_WITHIN));
        for late in [61, 89, 91, 119] {
            assert!(!round_fits(Duration::from_secs(late)), "{late}");
        }
    }

    /// The making model is told, before it writes, what each trigger means:
    /// someone coming back to the desk is room/presence, never the faces.
    #[test]
    fn the_system_prompt_says_what_coming_back_to_the_desk_is() {
        for reads in [true, false] {
            let system = system_with(reads);
            assert!(
                system.contains(&format!(
                    "When it runs is one [trigger] table (never [[trigger]]; coming to the \
                     desk is room/presence, never the faces): {} A [trigger], its sooner or a \
                     [[while]] reads one of these topics alone, and only the fields it carries: \
                     {}; the lamp sends no other. A body step is one gesture, one of: nod, \
                     headshake, ",
                    made_skill::TRIGGER_MEANINGS,
                    made_skill::readable_named()
                )),
                "{system}"
            );
            // #6066: a time on named days of the week, in lamp-skills' form.
            assert!(
                system
                    .contains("a skill runs at a time only each day or on named days of the week")
                    && system.contains("add days = [\"wed\", \"fri\"] (mon to sun"),
                "{system}"
            );
        }
    }

    /// The making model is told the lamp's one sound, its soft hum, and
    /// that it has no other.
    #[test]
    fn the_system_prompt_names_the_one_sound() {
        for reads in [true, false] {
            let system = system_with(reads);
            assert!(system.contains("a [[do]] step cue = \"hum\""), "{system}");
            assert!(system.contains("no other sound and no sound file"));
        }
    }

    /// lamp-9adf, 3 Nov: a skill.toml that isn't TOML (a bare word as a
    /// value) goes back to the model once with the parser's words, as a
    /// check's refusal does, never refused outright.
    #[test]
    fn a_skill_toml_that_isnt_toml_goes_back_to_the_model_once() {
        let (root, keeping) = keeping("unreadable");
        let bad = good().replace("version = \"1.0.0\"", "version = 1.0.0 beta");
        assert!(bad != good());
        let (url, served) = service(vec![(200, bad.clone()), (200, good())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 2);
        let (url, served) = service(vec![(200, bad); 1 + MOST_REPAIRS]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW + 1);
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .contains(made_skill::NOT_MADE_TRY),
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The skill shown to the model is one a made skill may be: the cap
    /// and the keeping take it (lamp-skills pack took it too, 2 Nov).
    #[test]
    fn the_example_shown_to_the_model_is_a_skill_the_lamp_takes() {
        let (root, keeping) = keeping("example");
        let made = made_skill::keep(EXAMPLE, false, "local:1", &keeping).unwrap();
        assert_eq!(made.name, "example-only");
        assert!(system().contains(EXAMPLE));
        assert!(system().contains("never answer with it"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The owner's ask, 12 Nov (`LAMP_AGENT_SERVICES`): on, with a made
    /// skill's code on, a service skill is steered to one `do.task` for the
    /// lamp's agent and never to a read or write step; off, or with code
    /// off, the prompt is as before, word for word.
    #[test]
    fn a_service_skill_is_made_as_a_task_for_the_agent_only_with_its_switch() {
        assert!(made_skill::agent_services_on(Some("on")));
        for var in [None, Some(""), Some("1"), Some("ON"), Some("true")] {
            assert!(!made_skill::agent_services_on(var), "{var:?}");
        }
        made_skill::set_writes_here(Some(true));
        made_skill::set_scripts_here(Some(true));
        let task = "task = { task = \"...\", run = \"scripts/main.py\" }";
        for reads in [true, false] {
            let on = system_for(reads, true);
            for part in [
                task,
                "lamp-services tools (services_connected, service_read, service_write)",
                "goes only on the person's spoken yes",
                "Never a read = { ... } or write = { ... } step",
                "no other grant, and no services in [permissions]",
                "each CODE-read to read it or CODE-write to send with it",
                "gmail, google_calendar, google_drive",
                "grants = [\"gmail-read\"]",
                AGENT_SERVICES_EXAMPLE,
            ] {
                assert!(on.contains(part), "{part}: {on}");
            }
            assert!(!on.contains("services = { CODE = \"read\" }"), "{on}");
            assert!(!on.contains("services = { CODE = \"write\" }"), "{on}");
            let off = system_for(reads, false);
            assert!(!off.contains(task), "{off}");
            assert!(!off.contains("lamp-services tools"), "{off}");
            assert!(off.contains("services = { CODE = \"write\" }"), "{off}");
            if reads {
                assert!(off.contains("services = { CODE = \"read\" }"), "{off}");
            }
        }
        // A made task step is held to the code's cap: with code off, the
        // switch alone changes nothing.
        made_skill::set_scripts_here(Some(false));
        assert_eq!(system_for(true, true), system_for(true, false));
        made_skill::set_scripts_here(None);
        made_skill::set_writes_here(None);
    }

    /// #7245: with `LAMP_AGENT_SERVICES` on, a calendar or mail read is the
    /// agent's task, so the "can't find out" line no longer names mail or
    /// calendar content; off, it still does.
    #[test]
    fn agent_services_drop_the_mail_and_calendar_cant_find_out() {
        made_skill::set_scripts_here(Some(true));
        let clause = "a summary of mail or calendar content";
        assert!(!system_for(true, true).contains(clause));
        assert!(
            system_for(true, true)
                .contains("A skill that reads or writes the person's mail, calendar")
        );
        assert!(system_for(true, true).contains("anything from the internet"));
        assert!(system_for(true, false).contains(clause));
        made_skill::set_scripts_here(None);
    }

    /// #7245: with `LAMP_AGENT_SERVICES` on, a linked service's read at a
    /// time each day is told to be a skill, never the reminder's cannot,
    /// before the reminder's ask-instead line; off, nothing is added.
    #[test]
    fn agent_services_say_a_daily_service_read_is_no_reminder() {
        made_skill::set_scripts_here(Some(true));
        let rule = "is no reminder and no cannot: it is a skill made as told below";
        let on = system_for(true, true);
        let at = on.find(rule).expect("the daily service rule");
        assert!(at < on.find("Ask me for a reminder instead").unwrap());
        assert!(on.contains("linked service at a time each day"));
        assert!(!system_for(true, false).contains(rule));
        made_skill::set_scripts_here(None);
    }

    /// With `LAMP_AGENT_SERVICES` on, a made "every morning read my new
    /// mail" and "when I leave, email X that I'm out", each one `do.task`
    /// for the lamp's agent, pass the same checks and keeping as any made
    /// skill with code (`made_skill::keep`, its cap and say-back).
    #[test]
    fn a_service_skill_made_as_a_task_passes_the_lamps_checks() {
        made_skill::set_scripts_here(Some(true));
        made_skill::set_agent_services_here(Some(true));
        let (root, keeping) = keeping("agent-services");
        let made = made_skill::keep(AGENT_SERVICES_EXAMPLE, false, "local:1", &keeping).unwrap();
        assert_eq!(made.name, "morning-mail");
        assert_eq!(made.uses.grants, ["gmail-read"]);
        let leave = AGENT_SERVICES_EXAMPLE
            .replace("grants = [\"gmail-read\"]", "grants = [\"gmail-write\"]")
            .replace("topics = [\"clock/local\"]", "topics = [\"room/presence\"]")
            .replace(
                "topic = \"clock/local\"\nfield = \"known\"\nequals = true\nrearm = \"ends\"\n\
                 during = { from = \"08:00\", to = \"08:01\" }",
                "topic = \"room/presence\"\nfield = \"someone\"\nequals = false\nfor = \"5m\"\n\
                 cooldown = \"1h\"\nrearm = \"ends\"",
            )
            .replace(
                "Read the owner's new mail since yesterday and tell them who wrote and what about",
                "Email x@example.com saying I'm out",
            )
            .replace("morning-mail", "out-mail");
        assert!(leave.contains("equals = false"), "{leave}");
        let made = made_skill::keep(&leave, false, "local:2", &keeping).unwrap();
        assert_eq!(made.name, "out-mail");
        assert_eq!(made.uses.grants, ["gmail-write"]);
        // Without the switch, a service grant is refused; without code on,
        // the cap refuses a made task step, as before.
        made_skill::set_agent_services_here(Some(false));
        assert!(made_skill::keep(&leave, false, "local:3", &keeping).is_err());
        made_skill::set_agent_services_here(Some(true));
        made_skill::set_scripts_here(Some(false));
        assert!(made_skill::keep(&leave, false, "local:4", &keeping).is_err());
        made_skill::set_scripts_here(None);
        made_skill::set_agent_services_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lamp-9adf, 11 Nov 17:0x: "blink the light green twice when I
    /// arrive" was written as an `out` step with a colour and a pattern and
    /// failed every repair. The model is told how a blink is written and
    /// what `out` is, and a skill written so is kept (lamp-skills loads the
    /// same step: `a_blink_asked_of_a_made_skill_is_a_pulse_and_never_an_out_with_a_colour`).
    /// #7245: with asks on, the making model is told a light show (its
    /// step, its ```shows/NAME.toml block, its frames' bounds, colours in
    /// turn as one show) before the asks' rules, and a blink as a show
    /// alternating levels at 2 s frames; off, neither, and the blink is
    /// one pulse step, byte for byte as before.
    #[test]
    fn with_asks_on_a_light_show_is_told_and_a_blink_is_a_show() {
        made_skill::set_asks_here(Some(false));
        let off = system_with(false);
        assert!(
            !off.contains(SHOWS_RULES) && !off.contains(BLINK_SHOWS),
            "{off}"
        );
        assert!(off.contains(BLINK), "{off}");
        made_skill::set_asks_here(Some(true));
        let on = system_with(false);
        assert!(on.contains(&format!("\n{SHOWS_RULES}\n")), "{on}");
        assert!(on.ends_with(made_skill::ASKS_RULES), "{on}");
        assert!(
            on.contains(&format!(
                "{} {BLINK_SHOWS} To speak",
                made_skill::MORE_STEPS
            )),
            "{on}"
        );
        assert!(!on.contains(BLINK), "{on}");
        for part in [
            "show = \"traffic-light\"",
            "```shows/traffic-light.toml of 2 to 30 [[light]] frames",
            "for = \"3s\" (2s to 20s)",
            "a minute in all at most",
            "every frame written out in order",
        ] {
            assert!(SHOWS_RULES.contains(part), "{part}");
        }
        for part in [
            "alternating level 5 and level 1, for = \"2s\" each",
            "say the faster flashing as left out",
            "in any other skill, one colour step with a soft pulse",
        ] {
            assert!(BLINK_SHOWS.contains(part), "{part}");
        }
        made_skill::set_asks_here(None);
    }

    #[test]
    fn the_system_prompt_says_how_a_light_blinks_and_that_out_only_turns_it_off() {
        for reads in [true, false] {
            let system = system_with(reads);
            assert!(
                system.contains(&format!("{} {BLINK} To speak", made_skill::MORE_STEPS)),
                "{system}"
            );
        }
        let step = "light = { colour = \"green\", level = 3, for = \"5s\", pattern = { kind = \
                    \"pulse\" } }";
        assert!(BLINK.contains(step), "{BLINK}");
        assert!(BLINK.contains("{ out = true } only turns the light off: beside it only over"));
        let (root, keeping) = keeping("blink");
        let blink = EXAMPLE
            .replace(
                "light = { colour = \"cyan\", level = 2, for = \"25s\" }",
                step,
            )
            .replace("example-only", "green-on-arrival")
            .replace("An example of the form only: glows cyan", "Pulses green");
        assert!(blink.contains(step), "{blink}");
        let made = made_skill::keep(&blink, false, "local:1", &keeping).unwrap();
        assert_eq!(made.name, "green-on-arrival");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An answer whose genuine skill fires as the example does, on someone
    /// coming back, but glows warm: lamp-9adf's request, answered.
    fn glow_warm() -> String {
        EXAMPLE
            .replace(
                "colour = \"cyan\", level = 2, for = \"25s\"",
                "colour = \"warm\", level = 3, for = \"20s\"",
            )
            .replace("example-only", "warm-on-return")
            .replace("An example of the form only: glows cyan", "Glows warm")
    }

    /// The example is the example word for word, its skill.toml however
    /// spaced, or its name on another skill; a skill like it is not.
    #[test]
    fn the_example_given_back_is_known_however_written() {
        assert!(is_example(EXAMPLE));
        assert!(is_example(
            &EXAMPLE.replace(" = ", "=").replace("\n\n", "\n")
        ));
        assert!(is_example(
            &EXAMPLE.replace("example-only", "glow-on-return")
        ));
        assert!(is_example(&good().replace("evening-dim", "example-only")));
        assert!(!is_example(&good()));
        assert!(!is_example(&glow_warm()));
        let repair = made_skill::repair_brief(REQUEST, EXAMPLE_GIVEN_BACK).unwrap();
        assert!(repair.contains(EXAMPLE_GIVEN_BACK), "{repair}");
    }

    /// lamp-9adf, 3 Nov 16:15: the model gave the example back for "glow
    /// warm" and the lamp offered a cyan glow. It is refused as the check
    /// refuses, and goes back to the model once: its own skill is kept.
    #[test]
    fn the_example_given_back_goes_back_to_the_model_once() {
        let (root, keeping) = keeping("example-once");
        let (url, served) = service(vec![(200, EXAMPLE.to_owned()), (200, glow_warm())]);
        let told = make(
            "when I come back to my desk, glow warm",
            &keeping,
            chat_at(url),
            NOW,
        );
        assert_eq!(told["made"], "warm-on-return", "{told}");
        assert_eq!(served.join().unwrap(), 2);
        let kept = made_skill::entries(&keeping.directory);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0]["name"], "warm-on-return");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The example given back twice: said as not made, in plain words,
    /// asked no third time; nothing is kept, nothing is offered.
    #[test]
    fn the_example_given_back_twice_keeps_nothing() {
        let (root, keeping) = keeping("example-twice");
        let (url, served) = service(vec![(200, EXAMPLE.to_owned()); 1 + MOST_REPAIRS]);
        let told = make(
            "when I come back to my desk, glow warm",
            &keeping,
            chat_at(url),
            NOW,
        );
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY),
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        assert!(made_skill::entries(&keeping.directory).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A request whose genuine answer fires as the example does but isn't
    /// it is kept at once: the model is asked once.
    #[test]
    fn a_skill_like_the_example_but_not_it_is_kept() {
        let (root, keeping) = keeping("example-like");
        let (url, served) = service(vec![(200, glow_warm())]);
        let told = make(
            "when I come back to my desk, glow warm",
            &keeping,
            chat_at(url),
            NOW,
        );
        assert_eq!(told["made"], "warm-on-return", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lamp-9adf, 2 Nov 17:48: a stream that broke mid-file is asked once
    /// more; the whole one is kept. So is one the service stopped at its
    /// cap on tokens, and one with no fenced block at all.
    #[test]
    fn a_stream_cut_mid_file_is_asked_once_more() {
        let (root, keeping) = keeping("cut");
        let whole = good();
        let mid_file = whole[..whole.len() - 30].to_owned();
        assert!(again(&net::Error::Protocol(CUT)));
        assert!(again(&net::Error::Protocol(AT_LENGTH)));
        assert!(again(&net::Error::Protocol(NO_BLOCK)));
        let (url, served) = service(vec![(BROKEN, mid_file.clone()), (200, whole.clone())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 2);
        let (root_length, keeping) = super::tests::keeping("at-length");
        let (url, served) = service(vec![(STOPPED_AT_LENGTH, mid_file), (200, whole.clone())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 2);
        let (root_prose, keeping) = super::tests::keeping("no-block");
        let prose = "Here is a skill that dims the light at ten.".to_owned();
        let (url, served) = service(vec![(200, prose), (200, whole)]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 2);
        for root in [root, root_length, root_prose] {
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// A product lamp on 11798, 11 Nov: an answer that ended whole with its
    /// last fence never written was taken for a cut stream, asked again,
    /// and failed as cut off. Its last fence is closed at its end and it is
    /// kept, asked once; a second answer, after a stream that broke, gets
    /// the same.
    #[test]
    fn an_answer_ended_whole_with_its_last_fence_open_is_closed_and_kept() {
        let (root, keeping) = keeping("fence-end");
        let whole = good();
        let open = whole[..whole.rfind("```").unwrap()].to_owned();
        assert_eq!(made_skill::open_fence(&open).as_deref(), Some("```"));
        assert_eq!(made_skill::open_fence(&whole), None);
        assert_eq!(fence_closed(open.clone()).matches("```").count(), 4);
        let (url, served) = service(vec![(200, open.clone())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        let (root_second, keeping) = super::tests::keeping("fence-end-second");
        let broken = whole[..whole.len() / 3].to_owned();
        let (url, served) = service(vec![(BROKEN, broken), (200, open)]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 2);
        for root in [root, root_second] {
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// A product lamp, 11 Nov, from the app: three repairs on what the
    /// lamp refused, then the last answer cut twice, and the person heard
    /// that the model stopped before it finished. A repair's answer cut or
    /// never come is told as why the skill was refused; a first answer cut
    /// twice, with nothing refused before, is still told as the model's
    /// failure.
    #[test]
    fn a_cut_after_its_repairs_is_told_as_the_refusal() {
        let (root, keeping) = keeping("cut-after-repairs");
        let faces = good().replace(
            "topic = \"voice/state\"\nfield = \"state\"\nequals = \"resting\"\n",
            "topic = \"humans/faces/tracked\"\nnear = 0.14\n",
        );
        let whole = good();
        let cut = whole[..whole.rfind("```").unwrap()].to_owned();
        let mut replies = vec![(200, faces.clone()); MOST_REPAIRS];
        replies.extend([(BROKEN, cut.clone()), (BROKEN, cut.clone())]);
        let (url, served) = service(replies);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY),
            "{told}"
        );
        assert_eq!(
            told["next"],
            made_skill::not_made_in_their_language(made_skill::NOT_MADE_TRY)
        );
        assert_eq!(served.join().unwrap(), MOST_REPAIRS + 2);
        // A repair never answered: the same.
        let (url, served) = service(vec![
            (200, faces),
            (503, String::new()),
            (503, String::new()),
        ]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW + 1);
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY),
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 3);
        // Cut the first time, twice: the model's own failure.
        let (url, served) = service(vec![(BROKEN, cut.clone()), (BROKEN, cut)]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW + 2);
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::MODEL_SILENT),
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The relay's 404 is asked once more, and the second answer is kept;
    /// a second failure is said as not made.
    #[test]
    fn a_404_is_asked_once_more() {
        let (root, keeping) = keeping("again");
        let (url, served) = service(vec![(404, String::new()), (200, good())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 2);
        let (url, served) = service(vec![(503, String::new()), (404, String::new())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW + 5);
        assert!(
            told["error"].as_str().unwrap().contains("didn't answer"),
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_a_passing_failure_is_asked_again() {
        assert!(again(&net::Error::Status(404)));
        assert!(again(&net::Error::Status(502)));
        assert!(again(&net::Error::Timeout("request")));
        assert!(again(&net::Error::Transport("request")));
        assert!(!again(&net::Error::Status(401)));
        assert!(!again(&net::Error::Status(400)));
        assert!(!again(&net::Error::Protocol("answer length")));
    }

    /// A request the brief refuses asks the model nothing.
    #[test]
    fn an_empty_or_long_request_asks_nothing() {
        let (root, keeping) = keeping("bounds");
        let never = || -> Result<Chat, String> { panic!("no model is asked") };
        assert!(make("  ", &keeping, never, NOW)["error"].is_string());
        let long = "x".repeat(made_skill::MOST_REQUEST_CHARS + 1);
        assert!(make(&long, &keeping, never, NOW)["error"].is_string());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Off (`LAMP_MAKE_SKILL` unset), `make_skill` is nobody's and never
    /// declared, with the tasks part or without. On, it is the voice's own,
    /// declared once, with no tasks part at all as with one: a computer's
    /// agent never makes a skill. `put_on_made_skill` is gone either way: a
    /// made skill goes on at once (the owner, 10 Nov).
    #[test]
    fn the_switch_alone_makes_the_function_the_voices() {
        use crate::tools::{Route, Settings, declared, route};
        let tasks = crate::agent_tasks::Place {
            directory: "/run/t".into(),
            target: crate::agent_tasks::DEFAULT_TARGET.into(),
            who: false,
            lead_in: false,
            asked: false,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: false,
            named_only: false,
            correct: false,
            sign_in: false,
            sign_in_app: false,
            questions: false,
            now: false,
            one_yes: false,
            reply_named: false,
            harness_status: false,
        };
        for tasks in [None, Some(tasks)] {
            let off = Settings {
                tasks: tasks.clone(),
                ..Settings::default()
            };
            for name in [
                made_skill::MAKE,
                crate::made_change::FUNCTION,
                "put_on_made_skill",
            ] {
                assert_eq!(route(&off, name), Route::Nobody, "{name}");
                assert!(declared(&off).iter().all(|tool| tool.name != name));
            }
            let on = Settings {
                make_skill: true,
                tasks,
                ..Settings::default()
            };
            // A change's own function, offered and routed exactly where
            // `make_skill` is (#8008).
            for name in [made_skill::MAKE, crate::made_change::FUNCTION] {
                assert_eq!(route(&on, name), Route::MakeLocal(None), "{name}");
                assert_eq!(
                    declared(&on)
                        .iter()
                        .filter(|tool| tool.name == name)
                        .count(),
                    1,
                    "{name}"
                );
            }
            assert_eq!(route(&on, "put_on_made_skill"), Route::Nobody);
            assert!(
                declared(&on)
                    .iter()
                    .all(|tool| tool.name != "put_on_made_skill")
            );
            let make = declared(&on)
                .into_iter()
                .find(|tool| tool.name == made_skill::MAKE)
                .unwrap();
            assert!(make.description.contains("you'll make it now"));
            assert!(make.description.contains("put on the lamp at once"));
            assert!(!make.description.contains("yes"), "{}", make.description);
            assert!(!make.description.contains("confirm_agent_task"));
        }
    }

    /// Made by voice, a skill that reads Gmail not linked is told with
    /// `needs_link` and the model asked to tell, after the line that it's
    /// on, in its own words and language, where to link it; linked, or with `lamp-services` not telling,
    /// neither, and the make stands. A skill that reads nothing never asks.
    #[test]
    fn a_made_skill_that_reads_a_service_not_linked_says_where_to_link_it() {
        let made = |reads: Option<&str>| Made {
            name: "morning-mail".into(),
            version: "1.0.0".into(),
            sha256: "a".repeat(64),
            description: "Reads your latest mail at eight.".into(),
            uses: made_skill::Uses {
                speak: true,
                reads: reads.map(str::to_owned).into_iter().collect(),
                ..Default::default()
            },
            package: PathBuf::from("morning-mail.tar"),
        };
        let gmail = made(Some("gmail"));
        let needs = made_skill::needs_link(&gmail.uses, || Some(vec![]));
        let answer = told(Ok(gmail.clone()), Ok(()), needs.as_deref());
        assert_eq!(
            answer["needs_link"],
            json!([{"code": "gmail", "name": "Gmail"}]),
            "{answer}"
        );
        let next = answer["next"].as_str().unwrap();
        // The owner, 10 Nov ("đồng bộ luôn"): the model tells it in its own
        // words and the conversation's language; no fixed sentence is said.
        let ask = made_skill::needs_link_ask(&needs.clone().unwrap()).unwrap();
        let (on, link) = (
            next.find("I made morning mail, and it's on: reads your latest mail at eight")
                .expect(next),
            next.find(&ask).expect(next),
        );
        assert!(on < link, "{next}");
        assert!(!next.contains("isn't linked yet"), "{next}");
        assert!(!next.contains("Say exactly"), "{next}");
        for linked in [Some(vec!["gmail".to_owned()]), None] {
            let needs = made_skill::needs_link(&gmail.uses, || linked.clone());
            let answer = told(Ok(gmail.clone()), Ok(()), needs.as_deref());
            assert!(answer.get("needs_link").is_none(), "{answer}");
            assert!(!answer["next"].as_str().unwrap().contains("Connectors"));
            assert_eq!(answer["made"], "morning-mail");
        }
        let plain = made(None);
        let needs = made_skill::needs_link(&plain.uses, || panic!("never asked"));
        assert_eq!(needs, Some(vec![]));
        assert!(
            told(Ok(plain), Ok(()), needs.as_deref())
                .get("needs_link")
                .is_none()
        );
        // The make itself, with `lamp-services` not telling: kept and
        // put on as before.
        let (root, keeping) = keeping("needs-link-unknown");
        let (url, served) = service(vec![(200, good())]);
        let answer = make_with(0, None, REQUEST, &keeping, chat_at(url), NOW, || None);
        assert_eq!(answer["made"], "evening-dim", "{answer}");
        assert!(answer.get("needs_link").is_none(), "{answer}");
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A made photo booth with code of its own (#6307).
    pub(crate) const CODE_TOML: &str = "kind = \"installable\"\nversion = \"1.0.0\"\n\
        grants = [\"camera\"]\n[permissions]\ntopics = [\"room/presence\"]\n\
        [trigger]\ntopic = \"room/presence\"\nfield = \"someone\"\nequals = true\n\
        for = \"5s\"\ncooldown = \"1h\"\nrearm = \"ends\"\n[ask]\npolicy = \"nudge\"\n[[do]]\n\
        task = { task = \"take a photo\", run = \"scripts/main.py\" }\n";
    pub(crate) const CODE_CARD: &str =
        "---\nname: photo-booth\ndescription: Takes a photo.\n---\n\n# Photo booth\n";
    /// [`CODE_CARD`] as the lamp's agent writes it with code (#8157): its
    /// `## Uses`, `## Result` and `## Answer`.
    pub(crate) const AGENT_CODE_CARD: &str = "---\nname: photo-booth\ndescription: Takes a photo.\n---\n\n# Photo booth\n\n## Uses\n\n- scripts/main.py, no arguments.\n\n## Result\n\nNothing is printed: the result is said.\n\n## Answer\n\nThe script says it with lamp.say; the agent answers a lone -.\n";
    pub(crate) const CODE_REQUEST: &str =
        "make me a photo booth that takes a photo when I sit down";

    /// #6526, with `LAMP_MADE_SKILL_SAID` off: the making model is told a
    /// skill can't hear what is said, as before, and never of a said start;
    /// on, it is told the form ([`made_skill::SAID_RULES`]) and that only
    /// the room's talk is out of reach.
    #[test]
    fn a_said_start_is_told_to_the_maker_only_with_its_switch() {
        made_skill::set_said_here(Some(false));
        let off = system_with(false);
        assert!(
            off.contains("A skill can't yet hear what is said, know who"),
            "{off}"
        );
        assert!(!off.contains("[said]"), "{off}");
        made_skill::set_said_here(Some(true));
        let on = system_with(false);
        assert!(on.ends_with(made_skill::SAID_RULES), "{on}");
        assert!(
            on.contains("A skill can't yet hear what is said in the room (a request told"),
            "{on}"
        );
        made_skill::set_said_here(None);
    }

    /// The owner, 16 Nov ("no 'couldn't' here"): with
    /// `LAMP_MADE_SKILL_ASKS` off the making model's prompt and brief are
    /// byte for byte as with the switch unset, a skill can't yet hear what
    /// is said; on, it is told a skill may start on the words the person
    /// names ([`made_skill::ASKS_RULES`]) with any step, in place of a
    /// [said] start, the agent's service rules naming them too.
    #[test]
    fn a_made_skills_own_words_are_told_to_the_maker_only_with_their_switch() {
        let request = "when I say send my test note, email me a note saying hello";
        made_skill::set_asks_here(None);
        let unset = (
            system_with(false),
            system_for(true, true),
            made_skill::brief(request),
        );
        made_skill::set_asks_here(Some(false));
        let off = (
            system_with(false),
            system_for(true, true),
            made_skill::brief(request),
        );
        assert_eq!(off, unset, "off is as before, byte for byte");
        assert!(
            off.0
                .contains("A skill can't yet hear what is said, know who"),
            "{}",
            off.0
        );
        assert!(!off.0.contains("asks ="), "{}", off.0);
        made_skill::set_asks_here(Some(true));
        made_skill::set_said_here(Some(false));
        let on = system_with(false);
        assert!(on.ends_with(made_skill::ASKS_RULES), "{on}");
        assert!(!on.contains(made_skill::SAID_RULES), "{on}");
        assert!(!on.contains("[said]"), "{on}");
        assert!(
            on.contains(
                "A skill can't yet hear what is said in the room, other than the words the \
                 person names to start it (asks, below)"
            ),
            "{on}"
        );
        made_skill::set_scripts_here(Some(true));
        let agent = system_for(true, true);
        assert!(
            agent.contains("or only the words they name for it (asks, as told at the end)"),
            "{agent}"
        );
        let brief = made_skill::brief(request).unwrap();
        assert!(brief.contains("or asks as above"), "{brief}");
        assert!(brief.len() <= made_skill::MOST_BRIEF_BYTES);
        made_skill::set_scripts_here(None);
        made_skill::set_said_here(None);
        made_skill::set_asks_here(None);
    }

    /// The longest request in bytes (2,000 characters of four bytes since
    /// #7245, [`made_skill::MOST_REQUEST_BYTES`]):
    /// its longest repair, with the longest system prompt this worker can
    /// give (every switch that adds to it on), still fits the rules the
    /// lamp's agent's make run takes (`lamp_tasks::skill_make`), and the
    /// lamp's own chat is handed the same two. Nothing of a brief goes to
    /// the tasks part's 2,000 bytes any more.
    #[test]
    fn the_longest_repair_and_system_prompt_fit_the_agents_make_rules() {
        let longest = made_skill::longest_request();
        assert_eq!(longest.len(), made_skill::MOST_REQUEST_BYTES);
        let repair = made_skill::repair_brief(&longest, &"y".repeat(5_000)).unwrap();
        assert!(repair.len() <= made_skill::MOST_REPAIR_BRIEF_BYTES);
        assert_eq!(made_skill::request_of(&repair), longest);
        made_skill::set_writes_here(Some(true));
        made_skill::set_said_here(Some(true));
        made_skill::set_asks_here(Some(true));
        for scripts in [false, true] {
            made_skill::set_scripts_here(Some(scripts));
            for (reads, agent, net) in [
                (true, false, false),
                (true, true, false),
                (true, true, true),
                (true, false, true),
            ] {
                // As the agent's make run is handed them, the refusal first
                // too (#8651).
                let rules =
                    crate::make_skill_agent::rules_of(&system_net(reads, agent, net), &repair);
                assert!(
                    rules.len() <= lamp_tasks::skill_make::MOST_RULES_BYTES,
                    "{} > {}",
                    rules.len(),
                    lamp_tasks::skill_make::MOST_RULES_BYTES
                );
            }
        }
        made_skill::set_scripts_here(None);
        made_skill::set_asks_here(None);
        made_skill::set_said_here(None);
        made_skill::set_writes_here(None);
    }

    /// #6526 with the person's own words on too: with `LAMP_MADE_SKILL_SAID`
    /// and `LAMP_MADE_SKILL_ASKS` both on, the making model is told both
    /// starts (a [said] start's rules, then the asks'), the room's talk
    /// alone out of reach, and the brief points to both, the longest still
    /// within [`made_skill::MOST_BRIEF_BYTES`]. Each switch alone tells and
    /// points to its own start only, as before.
    #[test]
    fn a_said_start_and_asks_are_both_told_with_both_switches() {
        let request = "when I tell you I'm busy, turn red";
        let longest = made_skill::longest_request();
        let said_alone = "A skill can't yet hear what is said in the room (a request told to the \
                          lamp is a [said] start, below), know who";
        let asks_alone = "A skill can't yet hear what is said in the room, other than the words \
                          the person names to start it (asks, below), know who";
        // Each alone: its own rules last, its own words, the other's absent.
        made_skill::set_asks_here(Some(false));
        made_skill::set_said_here(Some(true));
        let said = system_with(false);
        assert!(
            said.ends_with(&format!("\n{}", made_skill::SAID_RULES)),
            "{said}"
        );
        assert!(said.contains(said_alone), "{said}");
        assert!(!said.contains(made_skill::ASKS_RULES), "{said}");
        let brief = made_skill::brief(request).unwrap();
        assert!(
            brief.contains("; one [trigger] as listed or [said] as above, with"),
            "{brief}"
        );
        made_skill::set_asks_here(Some(true));
        made_skill::set_said_here(Some(false));
        let asks = system_with(false);
        assert!(
            asks.ends_with(&format!("\n{}", made_skill::ASKS_RULES)),
            "{asks}"
        );
        assert!(asks.contains(asks_alone), "{asks}");
        assert!(!asks.contains(made_skill::SAID_RULES), "{asks}");
        let brief = made_skill::brief(request).unwrap();
        assert!(
            brief.contains("; one [trigger] as listed, or asks as above, with"),
            "{brief}"
        );
        // Both: both rules, the said start's first, and both named.
        made_skill::set_said_here(Some(true));
        let both = system_with(false);
        assert!(
            both.ends_with(&format!(
                "\n{}\n{}",
                made_skill::SAID_RULES,
                made_skill::ASKS_RULES
            )),
            "{both}"
        );
        assert!(
            both.contains(
                "A skill can't yet hear what is said in the room, other than a request told to \
                 the lamp (a [said] start) or the words the person names to start it (asks), \
                 both below, know who"
            ),
            "{both}"
        );
        assert!(
            !both.contains(said_alone) && !both.contains(asks_alone),
            "{both}"
        );
        let brief = made_skill::brief(request).unwrap();
        assert!(
            brief.contains("; one [trigger] listed, [said]/asks as above, with"),
            "{brief}"
        );
        // The longest brief within its bound with everything it may point
        // to on: reads, writes, code or a task for the lamp's agent.
        for writes in [false, true] {
            made_skill::set_writes_here(Some(writes));
            for scripts in [false, true] {
                made_skill::set_scripts_here(Some(scripts));
                for (reads, agent) in [(false, false), (true, false), (true, true)] {
                    let brief = made_skill::brief_for(&longest, reads, agent).unwrap();
                    assert!(brief.contains("[said]/asks as above"), "{brief}");
                    assert!(
                        brief.len() <= made_skill::MOST_BRIEF_BYTES,
                        "{} > {}",
                        brief.len(),
                        made_skill::MOST_BRIEF_BYTES
                    );
                }
            }
        }
        made_skill::set_scripts_here(None);
        made_skill::set_writes_here(None);
        made_skill::set_said_here(None);
        made_skill::set_asks_here(None);
    }

    /// The owner, 16 Nov: "when I say send my test note, email me ..." is
    /// made, with `LAMP_MADE_SKILL_ASKS` on, as a skill whose asks are those
    /// words beside its write: kept and put on, its say-back naming the
    /// words, its write waiting for the person's yes as any made write's.
    #[test]
    fn a_skill_started_by_its_words_is_made_with_its_asks_and_its_write() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let (root, keeping) = keeping("asks-write");
        let request = "when I say send my test note, email me@example.com with the subject \
                       Test note, saying This is my test note.";
        let toml = "asks = [\"send my test note\"]\n\
            kind = \"installable\"\nversion = \"1.0.0\"\n\
            [permissions]\nservices = { gmail = \"write\" }\n[[do]]\n\
            write = { service = \"gmail\", action = \"send_mail\", to = [\"me@example.com\"], \
            subject = \"Test note\", text = \"This is my test note.\" }\n";
        let (url, served) = service(vec![(200, write_answer(toml))]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            7,
            Some(request),
            request,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1, "{told}");
        assert_eq!(told["made"], "leaving-mail", "{told}");
        assert_eq!(told["writes_wait_for_yes"], true, "{told}");
        let next = told["next"].as_str().unwrap();
        for part in [
            "when you say “send my test note”, it mails me@example.com",
            made_skill::ASKS_ONLY_MANNERS.trim(),
            made_skill::WAITS_FOR_YES.trim(),
        ] {
            assert!(next.contains(part), "{part}: {next}");
        }
        asked.join().unwrap();
        assert_eq!(
            made_skill::entries(&keeping.directory)[0]["asks"],
            serde_json::json!(["send my test note"]),
            "its words kept beside it, for a later make's check while not on"
        );
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #8451, lamp-06c1, 24 Nov: a make whose asks end in a "!" ("pebble
    /// hello!" there) is kept on the model's first answer with the "!"
    /// dropped, never sent back for a repair; asks refused by a rule every
    /// time are said, once, plainly, as not made, never the asks or the
    /// form's example.
    #[test]
    fn a_skill_whose_asks_end_in_punctuation_is_kept_first_and_one_refused_is_said_plainly() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let (root, tidying) = keeping("asks-tidied");
        let request = "when I say pebble garden tune, email me@example.com with the subject \
                       Test note, saying This is my test note.";
        let toml = |asks: &str| {
            format!(
                "asks = [\"{asks}\"]\n\
                 kind = \"installable\"\nversion = \"1.0.0\"\n\
                 [permissions]\nservices = {{ gmail = \"write\" }}\n[[do]]\n\
                 write = {{ service = \"gmail\", action = \"send_mail\", to = [\"me@example.com\"], \
                 subject = \"Test note\", text = \"This is my test note.\" }}\n"
            )
        };
        let (url, served) = service(vec![(200, write_answer(&toml("pebble garden tune!")))]);
        let asked = manage_helper(&tidying.manage);
        let told = make_with(
            7,
            Some(request),
            request,
            &tidying,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1, "{told}");
        assert_eq!(told["made"], "leaving-mail", "{told}");
        asked.join().unwrap();
        assert_eq!(
            made_skill::entries(&tidying.directory)[0]["asks"],
            serde_json::json!(["pebble garden tune"]),
        );
        let _ = std::fs::remove_dir_all(&root);
        // Refused on its rule every time: one plain line.
        let (root, keeping) = keeping("asks-refused");
        let (url, served) = service(vec![
            (200, write_answer(&toml("pebble, hello")));
            1 + MOST_REPAIRS
        ]);
        let told = make_with(
            7,
            Some(request),
            request,
            &keeping,
            chat_at(url),
            NOW + 1,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS, "{told}");
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
        );
        let error = told["error"].as_str().unwrap();
        assert!(!error.contains("pebble") && !error.contains("send my test note"));
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #8451, the first builder's call (b) at 08:12: lamp-06c1, 24 Nov, a
    /// "pebble hello" make the person named ("whenever I say pebble
    /// hello") was refused three times in 530 s, "hello" being said around
    /// an ask. A trigger that is a run of the person's own words is kept
    /// with one word of its own, on the model's first answer, no repair;
    /// the same asks the model chose itself, never said in the line, are
    /// still refused on the two-word floor, named, and repaired.
    #[test]
    fn a_trigger_the_person_named_with_one_word_of_its_own_is_kept_on_the_first_answer() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let toml = |asks: &str| {
            format!(
                "asks = [\"{asks}\"]\n\
                 kind = \"installable\"\nversion = \"1.0.0\"\n\
                 [permissions]\nservices = {{ gmail = \"write\" }}\n[[do]]\n\
                 write = {{ service = \"gmail\", action = \"send_mail\", to = [\"me@example.com\"], \
                 subject = \"Test note\", text = \"This is my test note.\" }}\n"
            )
        };
        let theirs = "whenever I say pebble hello, email me@example.com with the subject Test \
                      note, saying This is my test note.";
        let (root, keeping) = keeping("asks-named");
        let (url, served) = service(vec![(200, write_answer(&toml("pebble hello")))]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(7, Some(theirs), theirs, &keeping, chat_at(url), NOW, || {
            None
        });
        assert_eq!(served.join().unwrap(), 1, "no repair: {told}");
        assert_eq!(told["made"], "leaving-mail", "{told}");
        asked.join().unwrap();
        assert_eq!(
            made_skill::entries(&keeping.directory)[0]["asks"],
            serde_json::json!(["pebble hello"]),
        );
        let _ = std::fs::remove_dir_all(&root);
        // The model's own "pebble hello", never in the person's line:
        // refused on the floor, its rule named, with a repair.
        let chosen = "when I say pebble garden tune, email me@example.com with the subject \
                      Test note, saying This is my test note.";
        let (root, keeping) = self::keeping("asks-chosen");
        let refused = made_skill::keep_asked(
            &write_answer(&toml("pebble hello")),
            false,
            FROM,
            &keeping,
            &made_skill::Asked {
                request: chosen,
                heard: Some(chosen),
                ..made_skill::Asked::default()
            },
        )
        .unwrap_err();
        assert_eq!(
            refused.0,
            "its asks weren't kept: asks = [\"pebble hello\"]: \"pebble hello\": only 1 word of its \
             own, \"pebble\""
        );
        assert!(refused.1.is_some(), "repaired");
        let _ = std::fs::remove_dir_all(&root);
        let (root, keeping) = self::keeping("asks-chosen-made");
        let (url, served) = service(vec![
            (200, write_answer(&toml("pebble hello")));
            1 + MOST_REPAIRS
        ]);
        let told = make_with(
            7,
            Some(chosen),
            chosen,
            &keeping,
            chat_at(url),
            NOW + 1,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS, "{told}");
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
        );
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #8451, the first builder's (a) for what still fails: the person's
    /// own words for a skill's start that break a rule no writer can change
    /// ("stop the music", a stop) are said at once, plainly, in one line
    /// for that rule, with no repair spent on them and never the form's
    /// example.
    #[test]
    fn the_persons_own_words_that_break_a_rule_are_said_at_once_with_no_repair() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let request = "whenever I say stop the music, email me@example.com with the subject \
                       Test note, saying This is my test note.";
        let toml = "asks = [\"stop the music\"]\n\
            kind = \"installable\"\nversion = \"1.0.0\"\n\
            [permissions]\nservices = { gmail = \"write\" }\n[[do]]\n\
            write = { service = \"gmail\", action = \"send_mail\", to = [\"me@example.com\"], \
            subject = \"Test note\", text = \"This is my test note.\" }\n";
        let (root, keeping) = keeping("asks-theirs-refused");
        let (url, served) = service(vec![(200, write_answer(toml))]);
        let told = make_with(
            7,
            Some(request),
            request,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1, "no repair: {told}");
        assert!(told.get("made").is_none(), "{told}");
        let line = "I can't start a skill with ‘stop the music’, because that's how you stop me.";
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {line}"),
            "{told}"
        );
        assert!(
            !told["error"]
                .as_str()
                .unwrap()
                .contains("send my test note")
        );
        // Said as it is, whole, once.
        assert_eq!(made_skill::plain_reason(line), line);
        assert_eq!(made_skill::not_made_line(line), line);
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #8451, the first builder's required change on #8469: the person's
    /// whole phrase for a skill's start ("hello there", all of what they
    /// named) refused by a rule no writer can change is said at once,
    /// plainly, with no repair spent on it.
    #[test]
    fn a_refused_ask_that_is_the_persons_whole_phrase_is_said_at_once_with_no_repair() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let request = "whenever I say hello there, email me@example.com with the subject Test \
                       note, saying This is my test note.";
        let toml = asks_mail_toml("hello there");
        let (root, keeping) = keeping("asks-whole-refused");
        let refused = made_skill::keep_asked(
            &write_answer(&toml),
            false,
            FROM,
            &keeping,
            &made_skill::Asked {
                request,
                heard: Some(request),
                ..made_skill::Asked::default()
            },
        )
        .unwrap_err();
        assert!(made_skill::their_ask(&refused.0), "{refused:?}");
        assert!(refused.0.contains("‘hello there’, because"), "{refused:?}");
        assert!(refused.1.is_none(), "no repair: {refused:?}");
        let _ = std::fs::remove_dir_all(&root);
        let (root, keeping) = self::keeping("asks-whole-refused-made");
        let (url, served) = service(vec![(200, write_answer(&toml))]);
        let told = make_with(
            7,
            Some(request),
            request,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1, "no repair: {told}");
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", refused.0),
            "{told}"
        );
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #8451, the first builder's call on #8469: the person naming a short
    /// answer as their whole phrase ("whenever I say yes please") is told
    /// at once that a yes or a no is the lamp's to answer, with no repair.
    #[test]
    fn a_short_answer_the_person_names_is_said_at_once_with_no_repair() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let request = "make a skill that mails me whenever I say yes please, to me@example.com \
                       with the subject Test note, saying This is my test note.";
        let toml = asks_mail_toml("yes please");
        let (root, keeping) = keeping("asks-answer-refused");
        let refused = made_skill::keep_asked(
            &write_answer(&toml),
            false,
            FROM,
            &keeping,
            &made_skill::Asked {
                request,
                heard: Some(request),
                ..made_skill::Asked::default()
            },
        )
        .unwrap_err();
        assert!(made_skill::their_ask(&refused.0), "{refused:?}");
        assert!(
            refused
                .0
                .contains("‘yes please’, because a yes or a no is mine to answer."),
            "{refused:?}"
        );
        assert!(refused.1.is_none(), "no repair: {refused:?}");
        let _ = std::fs::remove_dir_all(&root);
        let (root, keeping) = self::keeping("asks-answer-refused-made");
        let (url, served) = service(vec![(200, write_answer(&toml))]);
        let told = make_with(
            7,
            Some(request),
            request,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1, "no repair: {told}");
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", refused.0),
            "{told}"
        );
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #8451, the first builder's required change on #8469: an ask that is
    /// only part of a longer phrase the person named ("hello there" from
    /// "whenever I say hello there pebble") and is refused is the writer's
    /// mistake: a repair is spent on it, never the plain line, and the
    /// repair names the person's whole phrase ("use the words they said:
    /// hello there pebble"), which is then kept.
    #[test]
    fn a_refused_part_of_the_persons_longer_phrase_spends_a_repair_naming_it_whole() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let request = "whenever I say hello there pebble, email me@example.com with the subject \
                       Test note, saying This is my test note.";
        let (root, keeping) = keeping("asks-part-refused");
        let refused = made_skill::keep_asked(
            &write_answer(&asks_mail_toml("hello there")),
            false,
            FROM,
            &keeping,
            &made_skill::Asked {
                request,
                heard: Some(request),
                ..made_skill::Asked::default()
            },
        )
        .unwrap_err();
        assert!(!made_skill::their_ask(&refused.0), "{refused:?}");
        assert_eq!(
            refused.0,
            format!(
                "its asks weren't kept: asks = [\"hello there\"]: \"hello there\": {}",
                crate::skill_named::ask_refused("hello there").unwrap()
            )
        );
        let repair = refused.1.as_deref().expect("repaired");
        assert!(
            repair.contains("use the words they said: hello there pebble."),
            "{repair}"
        );
        let _ = std::fs::remove_dir_all(&root);
        let (root, keeping) = self::keeping("asks-part-refused-made");
        let (url, served) = service_sent(vec![
            (200, write_answer(&asks_mail_toml("hello there"))),
            (200, write_answer(&asks_mail_toml("hello there pebble"))),
        ]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            7,
            Some(request),
            request,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        let sent = served.join().unwrap();
        assert_eq!(sent.len(), 2, "one repair: {told}");
        assert!(
            sent[1].contains("use the words they said: hello there pebble"),
            "{}",
            sent[1]
        );
        assert_eq!(told["made"], "leaving-mail", "{told}");
        asked.join().unwrap();
        assert_eq!(
            made_skill::entries(&keeping.directory)[0]["asks"],
            serde_json::json!(["hello there pebble"]),
        );
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Thirteen's and Twelve's P1 on #8469: an ask the writer gave that
    /// passes is kept, though the person's whole phrase runs on past it
    /// with no comma ("movie night dim the lights") and that phrase breaks a
    /// rule: no plain line and no repair, as on lamp-voice.
    #[test]
    fn a_passing_trigger_the_person_named_is_kept_though_their_phrase_runs_on() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        for (at, (name, said, ask)) in [
            (
                "asks-movie-night",
                "whenever I say movie night dim the lights",
                "movie night",
            ),
            (
                "asks-pebble-night",
                "when I say pebble night turn off the lights",
                "pebble night",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let request = format!(
                "{said}, email me@example.com with the subject Test note, saying This is my \
                 test note."
            );
            let (root, keeping) = keeping(name);
            let (url, served) = service(vec![(200, write_answer(&asks_mail_toml(ask)))]);
            let asked = manage_helper(&keeping.manage);
            let told = make_with(
                7,
                Some(request.as_str()),
                &request,
                &keeping,
                chat_at(url),
                NOW + at as u64,
                || None,
            );
            assert_eq!(served.join().unwrap(), 1, "no repair: {told}");
            assert_eq!(told["made"], "leaving-mail", "{told}");
            asked.join().unwrap();
            assert_eq!(
                made_skill::entries(&keeping.directory)[0]["asks"],
                serde_json::json!([ask]),
            );
            let _ = std::fs::remove_dir_all(&root);
        }
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
    }

    /// Thirteen's P2 on #8469: "says" names what a skill says, never its
    /// start, so the writer's "good morning" from "a skill that says good
    /// morning" is no phrase the person named: refused with the ordinary
    /// repair, never the plain line, and kept once mended.
    #[test]
    fn a_refused_ask_after_says_is_the_writers_and_spends_a_repair() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let request = "make a skill that says good morning and plays jazz when I wake up, and \
                       email me@example.com with the subject Test note, saying This is my test \
                       note.";
        let (root, keeping) = keeping("asks-says-refused");
        let refused = made_skill::keep_asked(
            &write_answer(&asks_mail_toml("good morning")),
            false,
            FROM,
            &keeping,
            &made_skill::Asked {
                request,
                heard: Some(request),
                ..made_skill::Asked::default()
            },
        )
        .unwrap_err();
        assert!(!made_skill::their_ask(&refused.0), "{refused:?}");
        assert_eq!(
            refused.0,
            format!(
                "its asks weren't kept: asks = [\"good morning\"]: \"good morning\": {}",
                crate::skill_named::REFUSED_LAMPS_OWN
            )
        );
        assert!(refused.1.is_some(), "repaired: {refused:?}");
        let _ = std::fs::remove_dir_all(&root);
        let (root, keeping) = self::keeping("asks-says-refused-made");
        let (url, served) = service_sent(vec![
            (200, write_answer(&asks_mail_toml("good morning"))),
            (200, write_answer(&asks_mail_toml("morning jazz time"))),
        ]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            7,
            Some(request),
            request,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap().len(), 2, "one repair: {told}");
        assert_eq!(told["made"], "leaving-mail", "{told}");
        asked.join().unwrap();
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Thirteen's P3 on #8469: the one-word floor holds only for an ask
    /// that starts the person's phrase right after their say or hear; a run
    /// of their line elsewhere ("jazz please" in "plays jazz please")
    /// needs two words of its own, refused as on lamp-voice.
    #[test]
    fn a_run_of_the_line_not_after_a_say_keeps_the_two_word_floor() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let request = "make a skill that plays jazz please when I get home, and email \
                       me@example.com with the subject Test note, saying This is my test note.";
        let (root, keeping) = keeping("asks-run-floor");
        let refused = made_skill::keep_asked(
            &write_answer(&asks_mail_toml("jazz please")),
            false,
            FROM,
            &keeping,
            &made_skill::Asked {
                request,
                heard: Some(request),
                ..made_skill::Asked::default()
            },
        )
        .unwrap_err();
        assert_eq!(
            refused.0,
            "its asks weren't kept: asks = [\"jazz please\"]: \"jazz please\": only 1 word of its \
             own, \"jazz\""
        );
        assert!(refused.1.is_some(), "repaired");
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Thirteen's P2 at 77700b81fb: a say or a hear counts as naming a
    /// start only after "I" or "you". An order to the skill ("say good
    /// morning") names what it says, so a refused ask there is the writer's
    /// and spends the ordinary repair; "whenever I say good morning" is
    /// still told plainly at once.
    #[test]
    fn an_order_to_say_names_no_start_and_spends_a_repair() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        for (name, line, ask) in [
            (
                "asks-order-morning",
                "when I wake up, say good morning",
                "good morning",
            ),
            (
                "asks-order-night",
                "every night at ten, say good night and dim the light",
                "good night",
            ),
        ] {
            let refused = keep_refused(name, ask, line);
            assert!(!made_skill::their_ask(&refused.0), "{line}: {refused:?}");
            assert_eq!(
                refused.0,
                format!(
                    "its asks weren't kept: asks = [\"{ask}\"]: \"{ask}\": {}",
                    crate::skill_named::REFUSED_LAMPS_OWN
                ),
                "{line}"
            );
            assert!(refused.1.is_some(), "repaired: {line}");
        }
        let refused = keep_refused(
            "asks-said-morning",
            "good morning",
            "whenever I say good morning",
        );
        assert!(made_skill::their_ask(&refused.0), "{refused:?}");
        assert!(
            refused.0.contains(
                "‘good morning’, because those are my own words for something I do myself."
            ),
            "{refused:?}"
        );
        assert!(refused.1.is_none(), "no repair: {refused:?}");
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
    }

    /// Thirteen's P3 at 77700b81fb: an ask inside the person's phrase that
    /// doesn't start it ("night please" in "whenever I say movie night
    /// please") keeps the two-word floor.
    #[test]
    fn a_part_inside_the_persons_phrase_keeps_the_two_word_floor() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let refused = keep_refused(
            "asks-inside-floor",
            "night please",
            "whenever I say movie night please",
        );
        assert_eq!(
            refused.0,
            "its asks weren't kept: asks = [\"night please\"]: \"night please\": only 1 word of its \
             own, \"night\""
        );
        assert!(refused.1.is_some(), "repaired");
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
    }

    /// Thirteen's P3 at 77700b81fb: a refused ask inside a phrase of the
    /// person's too long to be an ask (9 words) is neither told plainly nor
    /// named in a repair as their phrase: the ordinary repair.
    #[test]
    fn a_refused_ask_in_a_phrase_too_long_for_an_ask_spends_the_ordinary_repair() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let refused = keep_refused(
            "asks-too-long",
            "good morning",
            "whenever I say good morning to my little desk lamp friend pebble, email me",
        );
        assert!(!made_skill::their_ask(&refused.0), "{refused:?}");
        assert_eq!(
            refused.0,
            format!(
                "its asks weren't kept: asks = [\"good morning\"]: \"good morning\": {}",
                crate::skill_named::REFUSED_LAMPS_OWN
            )
        );
        let repair = refused.1.as_deref().expect("repaired");
        assert!(!repair.contains("use the words they said"), "{repair}");
        made_skill::set_asks_here(None);
        made_skill::set_writes_here(None);
    }

    /// [`made_skill::keep_asked`]'s refusal of the test-note skill started
    /// by `ask`, made for `line`, in a fresh folder named `name` (#8451).
    fn keep_refused(name: &str, ask: &str, line: &str) -> Refused {
        let (root, keeping) = keeping(name);
        let refused = made_skill::keep_asked(
            &write_answer(&asks_mail_toml(ask)),
            false,
            FROM,
            &keeping,
            &made_skill::Asked {
                request: line,
                heard: Some(line),
                ..made_skill::Asked::default()
            },
        )
        .unwrap_err();
        let _ = std::fs::remove_dir_all(&root);
        refused
    }

    /// A made skill sending the test note, started by `asks` (#8451).
    fn asks_mail_toml(asks: &str) -> String {
        format!(
            "asks = [\"{asks}\"]\n\
             kind = \"installable\"\nversion = \"1.0.0\"\n\
             [permissions]\nservices = {{ gmail = \"write\" }}\n[[do]]\n\
             write = {{ service = \"gmail\", action = \"send_mail\", to = [\"me@example.com\"], \
             subject = \"Test note\", text = \"This is my test note.\" }}\n"
        )
    }

    /// #6307, with `LAMP_MADE_SCRIPTS` off: the making model is never told
    /// a skill may carry code, nor the function that a yes may put one on;
    /// on, it is told the form, its bounds and its grants.
    #[test]
    fn code_is_told_to_the_maker_only_with_its_switch() {
        made_skill::set_scripts_here(Some(false));
        let off = system_with(false);
        assert!(!off.contains("scripts/"), "{off}");
        assert!(!off.contains("code of its own"), "{off}");
        assert!(!tool().description.contains(CODE_TOLD));
        made_skill::set_scripts_here(Some(true));
        let on = system_with(false);
        for part in [
            "```scripts/NAME",
            "at most 8 KiB each and 16 KiB in all",
            "task = { task = \"what the code does",
            "grants = [ ... ]",
            "body, light, speak, look, hear, sound, air, device, camera, microphone, speech",
            "waits for the person's yes before it runs any code",
        ] {
            assert!(on.contains(part), "{part}: {on}");
        }
        assert!(!on.contains("skills,"), "{on}");
        assert!(tool().description.contains(CODE_TOLD));
        let parameters = parameters(false, true);
        assert!(parameters["properties"]["approve_id"].is_object());
        assert!(parameters["properties"].get(READ_BACK).is_none());
        made_skill::set_scripts_here(None);
    }

    /// #6749 B: on lamp-52e6 with `LAMP_MADE_SCRIPTS` on, "make a skill
    /// that checks the processor temperature and sets the light" got "I
    /// can't run a script or check a temperature" and `make_skill` was
    /// never called. With the switch on, the function says plainly that a
    /// made skill may carry its own Python script, run by its task once the
    /// person agrees, with that ask as its example, within the link's
    /// bounds with every switch on; off, the function is as before.
    #[test]
    fn a_made_script_is_offered_to_the_voice_with_its_switch_alone() {
        made_skill::set_scripts_here(Some(false));
        let off = tool();
        for word in ["script", "Python", "temperature"] {
            assert!(
                !off.description.contains(word),
                "{word}: {}",
                off.description
            );
        }
        let parameters = off.parameters.clone().unwrap();
        assert_eq!(
            parameters["properties"]["request"]["description"],
            "What this skill should do and when, in the person's own words for this ask \
             alone: never a time, step or trigger of an earlier skill or turn. Never call it \
             for an ask they take back in the same line."
        );
        made_skill::set_scripts_here(Some(true));
        let on = tool();
        for part in [
            "its own Python script",
            "run by its task once the person agrees",
            "never say the lamp can't run a script",
        ] {
            assert!(on.description.contains(part), "{part}: {}", on.description);
        }
        let request = on.parameters.unwrap()["properties"]["request"]["description"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            request.contains("checks the processor temperature and sets the light"),
            "{request}"
        );
        // Every switch on, as on the lamps: still within the link's bounds.
        made_skill::set_writes_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        let all = tool();
        assert!(all.description.len() <= 1_024, "{}", all.description.len());
        assert!(!all.description.contains("yes"), "{}", all.description);
        // The skills made, listed (lamp-9adf, 15 Nov): named with every
        // switch on, its parameter beside remove.
        assert!(
            all.description
                .contains("to list those they made, made alone"),
            "{}",
            all.description
        );
        let parameters = all.parameters.unwrap();
        assert_eq!(
            parameters["properties"][crate::made_list::LIST]["type"],
            "boolean"
        );
        assert!(parameters.to_string().len() <= 4_096);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_writes_here(None);
        made_skill::set_scripts_here(None);
    }

    /// `LAMP_SKILL_RECURRING_TOLD` (lamp-9adf, 17 Nov: "every morning at
    /// nine, read me my new emails" set a reminder and read the mail now):
    /// with it, the function's request says a thing for the lamp to do at
    /// a time or on a repeat is a skill, never a reminder, still within the
    /// link's bounds with every switch on, the description unchanged; off,
    /// as before.
    #[test]
    fn a_repeat_for_the_lamp_to_do_is_told_a_skill_with_its_switch_alone() {
        crate::skill_recurring::set_on_here(Some(false));
        let off = tool();
        let request = |tool: &lamp_gemini::Tool| {
            tool.parameters.as_ref().unwrap()["properties"]["request"]["description"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert!(!request(&off).contains("every morning at nine"));
        made_skill::set_writes_here(Some(true));
        made_skill::set_scripts_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        crate::skill_recurring::set_on_here(Some(false));
        let before = tool();
        crate::skill_recurring::set_on_here(Some(true));
        let all = tool();
        assert_eq!(all.description, before.description);
        assert!(all.description.len() <= 1_024, "{}", all.description.len());
        let asked = request(&all);
        assert!(
            asked.ends_with(crate::skill_recurring::REQUEST_TOLD),
            "{asked}"
        );
        assert!(
            asked.contains("checks the processor temperature"),
            "{asked}"
        );
        assert!(all.parameters.unwrap().to_string().len() <= 4_096);
        crate::skill_recurring::set_on_here(None);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_writes_here(None);
        made_skill::set_scripts_here(None);
    }

    /// #8002's ARM64 row: with every switch the product's `lamp-voice`
    /// unit sets on that shapes `make_skill` (writes, tasks, code, the
    /// turn-off, the make at once, the core offer's undo and the repeat
    /// told on the request), the link takes it whole: its description
    /// within 1,024 bytes and its parameters within
    /// [`PRODUCT_MOST_PARAMETERS_BYTES`], short of
    /// [`lamp_gemini::MAX_TOOL_JSON_BYTES`] by a later parameter's room.
    /// Before, they came to 4,490 bytes (3,942 on lamp-voice, the core
    /// offer's undo and the repeat's words adding 548) and the product
    /// lamp's chat requests left `make_skill` out; 4,174 in the product's
    /// whole environment once lamp-voice gave the name and the remove at
    /// once (`lamp-update`'s `functions` test measures that one too).
    #[test]
    fn make_skill_is_within_the_link_with_the_products_switches_on() {
        made_skill::set_writes_here(Some(true));
        made_skill::set_scripts_here(Some(true));
        made_skill::set_off_said_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        crate::make_no_asks::set_here(Some(true));
        crate::make_core_offer::set_on_here(Some(true));
        crate::skill_recurring::set_on_here(Some(true));
        let all = tool();
        let parameters = all.parameters.as_ref().unwrap();
        for part in [
            crate::made_turn::TURN_OFF,
            crate::make_no_asks::UNDO,
            crate::make_core_offer::UNDO,
            crate::skill_write_yes::APPROVE_ID,
            READ_BACK,
        ] {
            assert!(parameters["properties"][part].is_object(), "{part}");
        }
        let request = parameters["properties"]["request"]["description"]
            .as_str()
            .unwrap();
        assert!(
            request.ends_with(crate::skill_recurring::REQUEST_TOLD),
            "{request}"
        );
        assert!(all.description.len() <= 1_024, "{}", all.description.len());
        let bytes = parameters.to_string().len();
        assert!(bytes <= PRODUCT_MOST_PARAMETERS_BYTES, "{bytes}");
        assert!(all.validate().is_ok());
        let (kept, left_out) = crate::tools::admitted(vec![all]);
        assert_eq!((kept.len(), left_out), (1, Vec::<String>::new()));
        crate::skill_recurring::set_on_here(None);
        crate::make_core_offer::set_on_here(None);
        crate::make_no_asks::set_here(None);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_off_said_here(None);
        made_skill::set_scripts_here(None);
        made_skill::set_writes_here(None);
    }

    /// #6749 E: the making model is told the lamp's real calls for its
    /// code (`lampos`' `Lamp.local()`, `device()`, `light(colour, level,
    /// for_ms)`), never left to guess a module and its keywords (lamp-9adf,
    /// 13 Nov: `from lamp import light`, `light.set_light(..., for="59s")`).
    #[test]
    fn the_maker_is_told_the_lamps_real_calls_for_its_code() {
        made_skill::set_scripts_here(Some(true));
        let on = system_with(false);
        for part in [
            "from lampos import Lamp",
            "await Lamp.local()",
            "await lamp.device()",
            "temperature_c",
            "await lamp.light(colour, level, for_ms)",
            "asyncio.run(main())",
        ] {
            assert!(on.contains(part), "{part}: {on}");
        }
        made_skill::set_scripts_here(Some(false));
        assert!(!system_with(false).contains("lampos"));
        made_skill::set_scripts_here(None);
    }

    /// #6749 E: a made script that doesn't parse (lamp-9adf's `for="59s"`)
    /// goes back to the model as a repair before anything is said back;
    /// fixed, it is kept and said back, waiting for the yes; never fixed,
    /// the person hears it couldn't be made.
    #[test]
    fn a_made_script_that_doesnt_parse_is_repaired_before_its_said_back() {
        made_skill::set_scripts_here(Some(true));
        made_skill::set_code_at_once_here(Some(false));
        let (root, keeping) = keeping("code-parse");
        let with = |main: &str| {
            format!(
                "```skill.toml\n{CODE_TOML}```\n\n```SKILL.md\n{CODE_CARD}```\n\n\
                 ```scripts/main.py\n{main}```\n"
            )
        };
        let bad =
            with("from lamp import light\nlight.set_light(colour=\"red\", level=3, for=\"59s\")\n");
        let good = with(
            "import asyncio\nfrom lampos import Lamp\n\nasync def main():\n    lamp = await Lamp.local()\n    \
             await lamp.light(\"red\", 3, 59000)\n    await lamp.close()\n\nasyncio.run(main())\n",
        );
        if made_skill::a_python_here().is_none() {
            eprintln!("skipped: no python3 here to parse a made script with");
            made_skill::set_code_at_once_here(None);
            made_skill::set_scripts_here(None);
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let (url, served) = service(vec![(200, bad.clone()), (200, good)]);
        let told = make_with(
            7,
            Some(CODE_REQUEST),
            CODE_REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 2);
        assert_eq!(told["made"], "photo-booth", "{told}");
        assert_eq!(told["code_waits_for_yes"], true, "{told}");
        // Never fixed: on a lamp of its own, so it isn't the same ask made.
        let (other, keeping) = self::keeping("code-parse-never");
        let (url, served) = service(vec![(200, bad); 1 + MOST_REPAIRS]);
        let told = make_with(
            8,
            Some(CODE_REQUEST),
            CODE_REQUEST,
            &keeping,
            chat_at(url),
            NOW + 1,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
        );
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
        made_skill::set_python_here(None);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&other);
    }

    /// #6307, on: a made skill with code of its own is kept and said back
    /// whole, its grants and that it runs its own code, and is not put on;
    /// it waits for a yes in a later heard line, once said whole, which puts
    /// on exactly the package said back, by its SHA-256.
    #[test]
    fn a_made_skill_with_code_waits_for_a_later_yes_and_goes_on_by_its_sha256() {
        made_skill::set_scripts_here(Some(true));
        // `LAMP_MADE_CODE_ON_AT_ONCE` off: exactly as before.
        made_skill::set_code_at_once_here(Some(false));
        let (root, keeping) = keeping("code");
        let answer = format!(
            "```skill.toml\n{CODE_TOML}```\n\n```SKILL.md\n{CODE_CARD}```\n\n\
             ```scripts/main.py\nprint('photo')\n```\n"
        );
        let (url, served) = service(vec![(200, answer)]);
        let told = make_with(
            7,
            Some(CODE_REQUEST),
            CODE_REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(told["made"], "photo-booth", "{told}");
        assert_eq!(told["on"], false, "{told}");
        assert_eq!(told["code_waits_for_yes"], true, "{told}");
        let next = told["next"].as_str().unwrap();
        for part in [
            "runs its own code to “take a photo”",
            made_skill::code_said(&["camera".to_owned()]).trim(),
            "word for word",
            "call make_skill again with only approve_id",
        ] {
            assert!(next.contains(part), "{part}: {next}");
        }
        assert!(!next.contains("it's on"), "{next}");
        // Not put on, nor even tried: no helper was asked.
        let entry = &made_skill::entries(&keeping.directory)[0];
        assert_ne!(entry["on"], true, "{entry}");
        assert!(entry.get("not_on").is_none(), "{entry}");
        assert_eq!(entry["uses"]["grants"], json!(["camera"]));
        let sha256 = entry["sha256"].as_str().unwrap().to_owned();
        let said = take_said_line().unwrap();
        assert!(said.ends_with("so it isn't on."), "{said}");
        let id = told[crate::skill_write_yes::APPROVE_ID]
            .as_str()
            .unwrap()
            .to_owned();
        let now = std::time::Instant::now();
        let never = |_: &crate::skill_write_yes::Asks| -> Result<Value, String> {
            panic!("put on before its say-back was heard")
        };
        let early = crate::skill_write_yes::approve(&id, 8, true, true, now, never);
        assert_eq!(early["error"], crate::skill_write_yes::NOT_SAID_WHOLE);
        let line = next.split('"').nth(1).unwrap();
        crate::skill_write_yes::lamp_said(7, line);
        let asked = manage_helper(&keeping.manage);
        // With a skill's tasks on (#6305), its task is kept for the folder
        // as it went on, with exactly the grants said back.
        crate::skill_task_yes::set_on_here(Some(true));
        let folder = "cd".repeat(32);
        let kept = std::cell::RefCell::new(None);
        let agreed = crate::skill_write_yes::approve(&id, 9, true, true, now, |asks| {
            assert!(asks.code && asks.writes.is_empty(), "{asks:?}");
            assert_eq!(asks.grants, Some(vec!["camera".to_owned()]));
            code_approve(
                &keeping,
                asks,
                |name| Ok(Some(task_asks(name, &folder, &sha256, &["camera"]))),
                |task| {
                    *kept.borrow_mut() = Some(task.clone());
                    Ok(json!({ "approved": 1 }))
                },
            )
        });
        assert_eq!(agreed["approved"], true, "{agreed}");
        assert_eq!(agreed["task"], true, "{agreed}");
        assert_eq!(agreed["next"], crate::skill_write_yes::CODE_APPROVED_NEXT);
        assert_eq!(asked.join().unwrap()["install_made"], sha256.as_str());
        assert_eq!(made_skill::entries(&keeping.directory)[0]["on"], true);
        let kept = kept.into_inner().unwrap();
        assert_eq!(
            (kept.skill.as_str(), kept.sha256.as_str(), kept.code),
            ("photo-booth", folder.as_str(), false)
        );
        assert_eq!(kept.grants, Some(vec!["camera".to_owned()]));
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A model's answer for a skill with code of its own, `toml` its file.
    pub(crate) fn code_answer_of(toml: &str) -> String {
        format!(
            "```skill.toml\n{toml}```\n\n```SKILL.md\n{CODE_CARD}```\n\n\
             ```scripts/main.py\nprint('photo')\n```\n"
        )
    }

    /// [`CODE_TOML`] granted its light alone: within
    /// `made_skill::CODE_AT_ONCE_GRANTS`.
    pub(crate) fn code_light_toml() -> String {
        CODE_TOML.replace("grants = [\"camera\"]", "grants = [\"light\"]")
    }

    /// A model's answer for a photo booth whose code uses its light alone.
    fn code_answer() -> String {
        code_answer_of(&code_light_toml())
    }

    /// #8180 (the dev's ruling): with `LAMP_MADE_CODE_ON_AT_ONCE` on, a
    /// made skill's code goes on at once whatever it declares, the camera,
    /// the microphone, `net`, hearing and sends among them; off, never.
    #[test]
    fn with_the_switch_every_grant_goes_on_at_once() {
        made_skill::set_scripts_here(Some(true));
        let grants = |names: &[&str]| -> Vec<String> {
            names.iter().map(|name| (*name).to_owned()).collect()
        };
        made_skill::set_code_at_once_here(Some(true));
        for declared in [
            &[][..],
            &["light", "speak", "body"],
            &["camera", "microphone", "sound", "hear", "notes", "look"],
            &["net", "speak"],
            &["gmail-write", "speak"],
        ] {
            assert!(
                made_skill::grants_go_on_at_once(&grants(declared)),
                "{declared:?}"
            );
        }
        made_skill::set_code_at_once_here(Some(false));
        assert!(!made_skill::grants_go_on_at_once(&grants(&["light"])));
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
    }

    /// #8180 (the dev's ruling): a voice make whose code uses the camera
    /// and the microphone, and one that knows who is at the lamp, each go
    /// on at once with
    /// nothing asked: no `approve_id`, no wait, and no yes recorded at the
    /// keep, its run step's and task's tokens (`net` among them) minted by
    /// lamp-control from its folder (`grants_need_no_yes`).
    #[test]
    fn with_the_switch_a_code_make_with_the_camera_mic_or_who_goes_on_at_once() {
        made_skill::set_scripts_here(Some(true));
        made_skill::set_code_at_once_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        crate::skill_task_yes::set_net_here(Some(true));
        let folder = "cd".repeat(32);
        let sensed = code_light_toml().replace(
            "grants = [\"light\"]",
            "grants = [\"camera\", \"microphone\"]",
        );
        let who = code_light_toml().replace("[permissions]\n", "[permissions]\nwho = true\n");
        for (at, (key, toml, declared)) in [
            ("sensed", sensed, &["camera", "microphone"][..]),
            ("who", who, &["light"][..]),
        ]
        .into_iter()
        .enumerate()
        {
            let at = at as u64;
            set_tasks_here(Some(task_asks("photo-booth", &folder, "", declared)));
            let (root, keeping) = keeping(&format!("code-at-once-all-{key}"));
            let (url, served) = service(vec![(200, code_answer_of(&toml))]);
            let asked = manage_helper(&keeping.manage);
            let told = make_with(
                7 + at,
                Some(CODE_REQUEST),
                CODE_REQUEST,
                &keeping,
                chat_at(url),
                NOW + at,
                || None,
            );
            assert_eq!(served.join().unwrap(), 1, "{key}: {told}");
            assert_eq!(told["on"], true, "{key}: {told}");
            for none in [crate::skill_write_yes::APPROVE_ID, "code_waits_for_yes"] {
                assert!(told.get(none).is_none(), "{key} {none}: {told}");
            }
            let next = told["next"].as_str().unwrap();
            assert!(
                !next.contains(made_skill::RUNS_AFTER_YES.trim()),
                "{key}: {next}"
            );
            let entry = &made_skill::entries(&keeping.directory)[0];
            assert_eq!(entry["on"], true, "{key}: {entry}");
            let sha256 = entry["sha256"].as_str().unwrap().to_owned();
            assert_eq!(
                asked.join().unwrap()["install_made"],
                sha256.as_str(),
                "{key}"
            );
            let _ = take_said_line();
            // Nothing asked and no yes recorded: its tokens come from its
            // folder's grants (`grants_need_no_yes`).
            assert!(tasks_recorded_here().is_empty(), "{key}");
            assert_eq!(told["task"], true, "{key}: {told}");
            let _ = declared;
            set_tasks_here(None);
            let _ = std::fs::remove_dir_all(&root);
        }
        crate::skill_task_yes::set_net_here(None);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
    }

    /// `LAMP_MADE_CODE_ON_AT_ONCE` (the owner's ask: a made skill goes on
    /// at once, said as it went, with no confirming): a voice make with
    /// code of its own goes on as it is made, exactly that package by its
    /// SHA-256, through its yes's own path; said as made and on, what its
    /// code may use, the code never read out; no yes waits and no
    /// `approve_id` is told; and its task's yes is recorded for the folder
    /// as it went on and its grants, so its first run asks nothing.
    #[test]
    fn with_the_switch_a_voice_code_make_goes_on_at_once_and_its_first_run_asks_nothing() {
        made_skill::set_scripts_here(Some(true));
        made_skill::set_code_at_once_here(Some(true));
        crate::skill_task_yes::set_on_here(Some(true));
        let folder = "cd".repeat(32);
        set_tasks_here(Some(task_asks("photo-booth", &folder, "", &["light"])));
        let (root, keeping) = keeping("code-at-once");
        let (url, served) = service(vec![(200, code_answer())]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            7,
            Some(CODE_REQUEST),
            CODE_REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(told["made"], "photo-booth", "{told}");
        assert_eq!(told["on"], true, "{told}");
        assert_eq!(told["task"], true, "{told}");
        for none in [
            crate::skill_write_yes::APPROVE_ID,
            "code_waits_for_yes",
            "writes_wait_for_yes",
            "not_on",
        ] {
            assert!(told.get(none).is_none(), "{none}: {told}");
        }
        let next = told["next"].as_str().unwrap();
        for part in [
            "I made photo booth, and it's on: ",
            "runs its own code to “take a photo”",
            "The lamp's model wrote its code, and that code may use its light.",
            "Ask nothing about putting it on.",
        ] {
            assert!(next.contains(part), "{part}: {next}");
        }
        assert!(!next.contains(made_skill::RUNS_AFTER_YES.trim()), "{next}");
        assert!(!next.contains("print('photo')"), "{next}");
        assert!(!next.contains("approve_id"), "{next}");
        assert!(!crate::skill_write_yes::awaited(std::time::Instant::now()));
        let entry = &made_skill::entries(&keeping.directory)[0];
        assert_eq!(entry["on"], true, "{entry}");
        let sha256 = entry["sha256"].as_str().unwrap().to_owned();
        assert_eq!(asked.join().unwrap()["install_made"], sha256.as_str());
        let said = take_said_line().unwrap();
        assert!(
            said.starts_with("I made photo booth, and it's on: "),
            "{said}"
        );
        // #8180: no yes is recorded at the keep: lamp-control mints its
        // run's token from its folder's grants (`grants_need_no_yes`).
        assert!(tasks_recorded_here().is_empty());
        let _ = &folder;
        set_tasks_here(None);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_MADE_CODE_ON_AT_ONCE`: a code make whose pack fails is said
    /// as not made, with why; one that can't be put on is said so, with
    /// why, and nothing waits; one on whose task wasn't kept says its code
    /// runs nowhere yet.
    #[test]
    fn with_the_switch_a_code_make_not_made_or_not_on_says_so_plainly() {
        use std::os::unix::fs::PermissionsExt;
        made_skill::set_scripts_here(Some(true));
        made_skill::set_code_at_once_here(Some(true));
        let (root, mut keeping) = keeping("code-at-once-failed");
        // The pack fails: not made, its repair unanswered, so told as the
        // refusal.
        let failing = root.join("pack-fails");
        std::fs::write(&failing, "#!/bin/sh\necho 'no room left' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&failing, std::fs::Permissions::from_mode(0o755)).unwrap();
        let program = std::mem::replace(&mut keeping.program, failing);
        let (url, served) = service(vec![(200, code_answer())]);
        let told = make_with(
            7,
            Some(CODE_REQUEST),
            CODE_REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        let _ = served.join();
        assert!(
            told["error"]
                .as_str()
                .is_some_and(|error| error.starts_with("the skill couldn't be made")),
            "{told}"
        );
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains(made_skill::NOT_MADE_TRY),
            "{told}"
        );
        assert!(told.get(crate::skill_write_yes::APPROVE_ID).is_none());
        assert!(made_skill::entries(&keeping.directory).is_empty());
        let _ = take_said_line();
        // Packed, but the lamp's skills can't be reached: not on, and why.
        keeping.program = program;
        let (url, served) = service(vec![(200, code_answer())]);
        let told = make_with(
            8,
            Some(CODE_REQUEST),
            CODE_REQUEST,
            &keeping,
            chat_at(url),
            NOW + 1,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(told["on"], false, "{told}");
        assert_eq!(told["not_on"], "the lamp's skills couldn't be reached");
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains(
                "I made it but couldn't put it on: the lamp's skills couldn't be reached."
            ),
            "{next}"
        );
        assert!(told.get(crate::skill_write_yes::APPROVE_ID).is_none());
        assert!(!crate::skill_write_yes::awaited(std::time::Instant::now()));
        let _ = take_said_line();
        // On, with a skill's tasks off: its code runs nowhere yet, said so.
        crate::skill_task_yes::set_on_here(Some(false));
        let (other, keeping) = super::tests::keeping("code-at-once-nowhere");
        let (url, served) = service(vec![(200, code_answer())]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            9,
            Some(CODE_REQUEST),
            CODE_REQUEST,
            &keeping,
            chat_at(url),
            NOW + 2,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1);
        let _ = asked.join();
        assert_eq!(
            (told["on"].as_bool(), told["task"].as_bool()),
            (Some(true), Some(false)),
            "{told}"
        );
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains(made_skill::CODE_RUNS_NOWHERE_YET.trim()),
            "{next}"
        );
        let _ = take_said_line();
        let _ = std::fs::remove_dir_all(&other);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_MADE_CODE_ON_AT_ONCE` on: a skill that sends still waits for
    /// its own read-back's yes, unchanged.
    #[test]
    fn with_the_switch_a_send_still_waits_for_its_yes() {
        made_skill::set_scripts_here(Some(true));
        made_skill::set_code_at_once_here(Some(true));
        made_skill::set_writes_here(Some(true));
        let (root, keeping) = keeping("code-at-once-send");
        let (url, served) = service(vec![(200, write_answer(WRITE_TOML))]);
        let asked = manage_helper(&keeping.manage);
        let told = make_with(
            7,
            Some(WRITE_REQUEST),
            WRITE_REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
        );
        assert_eq!(served.join().unwrap(), 1, "{told}");
        let _ = asked.join();
        assert_eq!(told["made"], "leaving-mail", "{told}");
        assert_eq!(told["writes_wait_for_yes"], true, "{told}");
        assert!(
            told[crate::skill_write_yes::APPROVE_ID].is_string(),
            "{told}"
        );
        let _ = take_said_line();
        made_skill::set_writes_here(None);
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    pub(crate) fn task_asks(
        name: &str,
        sha256: &str,
        package: &str,
        grants: &[&str],
    ) -> crate::skill_task_yes::TaskAsks {
        crate::skill_task_yes::TaskAsks {
            name: name.to_owned(),
            sha256: sha256.to_owned(),
            grants: grants.iter().map(|grant| (*grant).to_owned()).collect(),
            task: "take a photo".to_owned(),
            package: Some(package.to_owned()),
            from: "the lamp's own model".to_owned(),
            approved: false,
            tool: None,
        }
    }

    /// #6307: the task of a made skill with code is kept only with a
    /// skill's tasks on, and only for the grants said back; otherwise none
    /// is asked for, and the skill is on with its code run nowhere.
    #[test]
    fn a_made_skills_task_is_kept_only_for_the_grants_said_back() {
        let said = crate::skill_write_yes::Asks {
            skill: "photo-booth".to_owned(),
            sha256: "ab".repeat(32),
            writes: Vec::new(),
            say_back: made_skill::code_said(&["camera".to_owned()]),
            grants: Some(vec!["camera".to_owned()]),
            code: true,
            remove: false,
        };
        let folder = "cd".repeat(32);
        let never = |_: &crate::skill_write_yes::Asks| -> Result<Value, String> {
            panic!("a task kept that wasn't said back")
        };
        crate::skill_task_yes::set_on_here(Some(false));
        assert!(!code_task_kept(
            "photo-booth",
            &said,
            |_| panic!("asked with tasks off"),
            never
        ));
        crate::skill_task_yes::set_on_here(Some(true));
        for (name, grants) in [
            ("photo-booth", ["camera", "microphone"].as_slice()),
            ("photo-booth", [].as_slice()),
            ("other", ["camera"].as_slice()),
        ] {
            let read = task_asks(name, &folder, &said.sha256, grants);
            assert!(!code_task_kept(
                "photo-booth",
                &said,
                |_| Ok(Some(read)),
                never
            ));
        }
        assert!(!code_task_kept("photo-booth", &said, |_| Ok(None), never));
        assert!(!code_task_kept(
            "photo-booth",
            &said,
            |_| Err("busy".into()),
            never
        ));
        // Another folder by that name by then: not the package agreed to.
        for package in [Some("ef".repeat(32)), None] {
            let mut other = task_asks("photo-booth", &folder, &said.sha256, &["camera"]);
            other.package = package;
            assert!(!code_task_kept(
                "photo-booth",
                &said,
                |_| Ok(Some(other)),
                never
            ));
        }
        let read = task_asks("photo-booth", &folder, &said.sha256, &["camera"]);
        assert!(!code_task_kept(
            "photo-booth",
            &said,
            |_| Ok(Some(read.clone())),
            |_| Err("changed".into())
        ));
        assert!(code_task_kept(
            "photo-booth",
            &said,
            |_| Ok(Some(read)),
            |_| Ok(json!({}))
        ));
        crate::skill_task_yes::set_on_here(None);
    }

    /// `LAMP_MAKE_NO_CANNOT` (#7234): what this test's thread needs on.
    fn no_cannot_here(on: Option<bool>) {
        crate::make_no_cannot::set_on_here(on);
        made_skill::set_scripts_here(on.map(|_| true));
        crate::skill_task_yes::set_on_here(on.map(|_| true));
    }

    const PLANT: &str = "every morning at eight, tell me a short poem about my plant";

    /// The one more call's answer for [`PLANT`].
    fn plant_answer(grants: &str) -> String {
        format!(
            "name: plant-check\ngrants: {grants}\n```skill.toml\n[permissions]\n\
             topics = [\"clock/local\"]\n\n[trigger]\ntopic = \"clock/local\"\n\
             field = \"known\"\nequals = true\nrearm = \"ends\"\n\
             during = {{ from = \"08:00\", to = \"08:01\" }}\n```\n"
        )
    }

    /// The owner, 16 Nov (#7234): with `LAMP_MAKE_NO_CANNOT` on, the lamp's
    /// model's `cannot:` is not dropped. One more call to it picks the
    /// grants and the trigger; the skill's one task is the person's request
    /// in their words, its grants the ones named and checked, said back as
    /// a task for the lamp's agent, never as made, and it waits for the yes.
    #[test]
    fn with_the_switch_a_cannot_becomes_a_task_for_the_agent_with_the_request_and_its_grants() {
        no_cannot_here(Some(true));
        let (root, keeping) = keeping("no-cannot-task");
        let (url, served) = service_sent(vec![
            (200, "cannot: a skill can't look at a plant.".to_owned()),
            (200, plant_answer("light, speak")),
        ]);
        let told = make(PLANT, &keeping, chat_at(url), NOW);
        let sent = served.join().unwrap();
        assert_eq!(sent.len(), 2);
        assert!(
            sent[1].contains("You choose, for a lamp, when a skill runs and what it needs"),
            "{}",
            sent[1]
        );
        assert!(
            sent[1].contains(&format!("Request: {PLANT}")),
            "{}",
            sent[1]
        );
        assert_eq!(told["made"], "plant-check", "{told}");
        assert_eq!(told["code_waits_for_yes"], true, "{told}");
        assert_eq!(told["on"], false, "{told}");
        let description = told["description"].as_str().unwrap();
        assert!(
            description.contains(&format!("hands one task to the lamp's agent: “{PLANT}”")),
            "{description}"
        );
        assert!(
            description.ends_with(&format!(
                "The lamp's agent does it, and {}. It runs only once you agree.",
                crate::skill_task_yes::uses_said(&["light".to_owned()])
            )),
            "{description}"
        );
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains("\"I can have the lamp's agent do that, as plant check: every day"),
            "{next}"
        );
        assert!(!next.contains("I made"), "{next}");
        let entries = made_skill::entries(&keeping.directory);
        let [entry] = entries.as_slice() else {
            panic!("{entries:?}")
        };
        // On its trigger, never `speak` (#8610).
        assert_eq!(entry["uses"]["grants"], json!(["light"]), "{entry}");
        assert_eq!(entry["uses"]["agent_task"], true, "{entry}");
        assert!(take_said_line().is_some_and(|line| !line.contains("I made")));
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The lamp's agent's own `cannot:` is made a task the same way, its
    /// words never said or journaled; the one more call goes to the lamp's
    /// own model.
    #[test]
    fn with_the_switch_the_agents_cannot_becomes_a_task_too_its_words_never_said() {
        no_cannot_here(Some(true));
        let (root, keeping) = keeping("no-cannot-agent");
        // The agent's thread has no switch of this one's, so its rules are
        // not held to this thread's system prompt here.
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = std::sync::Arc::clone(&asked);
        let tasks = crate::make_skill_agent::tests::stub(
            Some("hermes"),
            std::sync::Arc::new(move |rules: &str| {
                assert!(rules.contains(made_skill::BRIEF_HEAD), "{rules}");
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Some("cannot: a skill can't see PRIVATE-DETAIL-9's plant.".to_owned())
            }),
        );
        let (url, served) = service(vec![
            (200, "cannot: a skill can't look at a plant.".to_owned()),
            (200, plant_answer("light, speak")),
        ]);
        let told = make_with_by(
            0,
            None,
            PLANT,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&tasks.agent()),
        );
        assert_eq!((served.join().unwrap(), tasks.hand_overs()), (2, 1));
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(told["made"], "plant-check", "{told}");
        assert!(!told.to_string().contains("PRIVATE-DETAIL-9"), "{told}");
        assert!(
            !told
                .to_string()
                .contains(crate::make_skill_agent::CANNOT_LINE),
            "{told}"
        );
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A request no grant can cover is told why, in plain words that name
    /// what is missing and what the person can do, never the bare "I
    /// couldn't make that one."; the journal names neither the request nor
    /// the reason's words.
    #[test]
    fn with_the_switch_a_request_no_grant_covers_is_told_its_reason() {
        no_cannot_here(Some(true));
        crate::skill_task_yes::set_net_here(Some(false));
        let (root, keeping) = keeping("no-cannot-net");
        let (url, served) = service(vec![
            (
                200,
                "cannot: I can't do that yet: checking the weather for you.".to_owned(),
            ),
            (200, plant_answer("speak, net")),
        ]);
        let (made, line) = made_and_journaled(
            PLANT,
            None,
            made_skill::brief(PLANT).unwrap(),
            &keeping,
            chat_at(url),
            "local:1:1",
        );
        assert_eq!(served.join().unwrap(), 2);
        let net = "I can't make that one yet: it needs the internet, and skills on this lamp \
                   can't use the internet yet.";
        assert_eq!(made, Err(Refused(net.to_owned(), None)));
        assert_eq!(made_skill::not_made_line(net), net);
        assert!(!line.contains("plant"), "{line}");
        assert!(made_skill::entries(&keeping.directory).is_empty());
        // What the model names that no grant gives: said, and nothing more.
        let (url, served) = service(vec![
            (200, "cannot: a skill can't pay for things.".to_owned()),
            (200, "needs: paying for things".to_owned()),
        ]);
        let told = make("every friday pay my rent", &keeping, chat_at(url), NOW + 1);
        assert_eq!(served.join().unwrap(), 2);
        assert_eq!(
            take_said_line().as_deref(),
            Some(
                "I can't make that one yet: it needs paying for things, which no skill on this \
                 lamp can be given yet."
            ),
            "{told}"
        );
        // Nothing named: still a reason and what to do.
        let (url, served) = service(vec![
            (200, "cannot: a skill can't do that.".to_owned()),
            (200, "needs: other".to_owned()),
        ]);
        let told = make("every friday do the thing", &keeping, chat_at(url), NOW + 2);
        assert_eq!(served.join().unwrap(), 2);
        assert_eq!(
            take_said_line().as_deref(),
            Some(crate::make_no_cannot::Uncovered::Other.line().as_str()),
            "{told}"
        );
        // A reminder's refusal names what to ask for instead: said as it is.
        let reminder = "I can't do that yet: a reminder every hour. Ask me for a reminder \
                        instead, like 'remind me every hour to stand up'.";
        let (url, served) = service(vec![(200, format!("cannot: {reminder}"))]);
        let told = make(
            "remind me every hour to stand up",
            &keeping,
            chat_at(url),
            NOW + 3,
        );
        assert_eq!(served.join().unwrap(), 1, "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {reminder}"),
            "{told}"
        );
        crate::skill_task_yes::set_net_here(None);
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// sim-2's P2 on #7261 (#6439's rule; the owner: "if something can't be
    /// done, say exactly why"): with the switch on, the brief's own
    /// refusals that keep people and their services safe stay refusals,
    /// with no more call, each said with the lamp's own plain reason, never
    /// the bare line or the model's words; the journal names neither the
    /// request nor the model's sentence.
    #[test]
    fn with_the_switch_the_briefs_privacy_refusals_stay_refused_with_their_reason() {
        no_cannot_here(Some(true));
        let (root, keeping) = keeping("no-cannot-kept");
        for (request, cannot, said) in [
            (
                "tell me who comes in",
                "I can't do that yet: knowing who someone is.",
                "I can't make that one: a skill can't tell who someone is, to keep people \
                 private.",
            ),
            (
                "when someone says my name, glow blue",
                "I can't do that yet: hearing what is said.",
                "I can't make that one: a skill can't listen to what people say, to keep the \
                 room private.",
            ),
            (
                "every night delete my old mail",
                "I can't do that yet: deleting or changing what is in your services.",
                "I can't make that one: a skill can't delete or change what's in your \
                 services; it can read and send with your yes.",
            ),
        ] {
            let (url, served) = service(vec![(200, format!("cannot: {cannot}"))]);
            let (made, line) = made_and_journaled(
                request,
                None,
                made_skill::brief(request).unwrap(),
                &keeping,
                chat_at(url),
                "local:1:1",
            );
            assert_eq!(served.join().unwrap(), 1, "{request}: no more call");
            assert_eq!(made, Err(Refused(said.to_owned(), None)), "{request}");
            assert_ne!(said, crate::make_skill_agent::CANNOT_LINE);
            assert_eq!(made_skill::not_made_line(said), said, "{request}");
            assert!(!line.contains(request), "{line}");
            assert!(!line.contains(cannot), "{line}");
        }
        assert!(made_skill::entries(&keeping.directory).is_empty());
        // Through the whole make: what the voice's model is told to say.
        let (url, served) = service(vec![(
            200,
            "cannot: I can't do that yet: knowing who someone is.".to_owned(),
        )]);
        let told = make("tell me who comes in", &keeping, chat_at(url), NOW);
        assert_eq!(served.join().unwrap(), 1);
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("a skill can't tell who someone is, to keep people private."),
            "{told}"
        );
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// sim-2 on #7261 and the first builder's ruling: the one more call
    /// never gives a task for the lamp's agent the microphone or anything
    /// that listens, nor, when it runs by itself on a trigger, the camera,
    /// which it may have only asked for, run when the person says its
    /// phrase. "Listen to the room and tell me what people say" stays the
    /// brief's own refusal with its reason; a microphone from the model is
    /// refused with its own, on a trigger or asked for; a camera on a
    /// trigger is told to ask for it by a phrase; nothing is kept.
    #[test]
    fn with_the_switch_a_task_never_listens_and_watches_only_when_asked() {
        no_cannot_here(Some(true));
        crate::upload_agent::set_on_here(Some(true));
        let (root, keeping) = keeping("no-cannot-sensing");
        const LISTEN: &str = "listen to the room and tell me what people say";
        let (url, served) = service(vec![(
            200,
            "cannot: I can't do that yet: hearing what is said.".to_owned(),
        )]);
        let told = make(LISTEN, &keeping, chat_at(url), NOW);
        assert_eq!(served.join().unwrap(), 1);
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("a skill can't listen to what people say, to keep the room private."),
            "{told}"
        );
        use crate::make_no_cannot::Uncovered;
        for (answer, refused) in [
            (plant_answer("microphone, speak"), Uncovered::Listening),
            (
                "name: listen-room\ngrants: microphone, speak\nasks: listen to the room".to_owned(),
                Uncovered::Listening,
            ),
            (plant_answer("camera, speak"), Uncovered::Sensing),
        ] {
            let (url, served) = service(vec![
                (200, "cannot: a skill can't do that.".to_owned()),
                (200, answer.clone()),
            ]);
            let (made, _) = made_and_journaled(
                LISTEN,
                None,
                made_skill::brief(LISTEN).unwrap(),
                &keeping,
                chat_at(url),
                "local:1:1",
            );
            assert_eq!(served.join().unwrap(), 2, "{answer}");
            assert_eq!(made, Err(Refused(refused.line(), None)), "{answer}");
        }
        assert!(made_skill::entries(&keeping.directory).is_empty());
        crate::upload_agent::set_on_here(None);
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// El's P2 on #7261: a `do.task` running the agent's path is a task for
    /// the lamp's agent only with the lamp's own script there, byte for
    /// byte. Real code the model wrote at that path is said and kept as the
    /// model's code.
    #[test]
    fn with_the_switch_model_code_at_the_agents_path_is_the_models_code() {
        no_cannot_here(Some(true));
        let block = "[permissions]\ntopics = [\"clock/local\"]\n\n[trigger]\n\
                     topic = \"clock/local\"\nfield = \"known\"\nequals = true\n\
                     rearm = \"ends\"\nduring = { from = \"08:00\", to = \"08:01\" }";
        let grants = ["light".to_owned(), "speak".to_owned()];
        let ours = crate::make_no_cannot::skill_text("plant-check", &grants, block, PLANT);
        let theirs = ours.replace(
            crate::make_no_cannot::SCRIPT,
            "import os\nprint(os.getcwd())\n",
        );
        assert_ne!(ours, theirs);
        for (answer, agents) in [(theirs, false), (ours, true)] {
            let (root, keeping) = keeping(&format!("no-cannot-own-code-{agents}"));
            let (url, served) = service_sent(vec![(200, answer)]);
            let told = make(PLANT, &keeping, chat_at(url), NOW);
            assert_eq!(served.join().unwrap().len(), 1);
            assert_eq!(told["code_waits_for_yes"], true, "{told}");
            let description = told["description"].as_str().unwrap();
            assert_eq!(
                description.contains("The lamp's agent does it"),
                agents,
                "{description}"
            );
            let entries = made_skill::entries(&keeping.directory);
            let [entry] = entries.as_slice() else {
                panic!("{entries:?}")
            };
            assert_eq!(entry["uses"]["agent_task"] == true, agents, "{entry}");
            let _ = take_said_line();
            ASKS.lock()
                .unwrap()
                .retain(|ask| ask.directory != keeping.directory);
            let _ = std::fs::remove_dir_all(&root);
        }
        crate::skill_write_yes::clear_for_test();
        no_cannot_here(None);
    }

    /// The first builder on #7261 (the lead's rule): the bare word decides
    /// nothing; the lamp's own model, given it only as a hint, decides by
    /// naming the grant. "How many open issues I have on Linear", Linear
    /// not linked, gets `linear-read` and is made with the link note; "a
    /// linear timer", hinted the same, gets the light and is no Linear make.
    #[test]
    fn with_the_switch_the_model_names_the_service_the_word_only_hints() {
        no_cannot_here(Some(true));
        made_skill::set_agent_services_here(Some(true));
        let (root, keeping) = keeping("no-cannot-hint");
        let at_desk = "```skill.toml\n[permissions]\ntopics = [\"room/presence\"]\n\n\
                       [trigger]\ntopic = \"room/presence\"\nfield = \"someone\"\n\
                       equals = true\n```\n";
        for (made_in, request, answer, grants, linear) in [
            (
                1,
                "when I sit down at my desk, tell me how many open issues I have on Linear",
                format!("name: linear-issues\ngrants: linear-read, speak\n{at_desk}"),
                // On a trigger, never `speak` (#8610).
                json!(["linear-read"]),
                true,
            ),
            (
                2,
                "when I sit down at my desk, start a linear timer that dims the light",
                format!("name: desk-timer\ngrants: light\n{at_desk}"),
                json!(["light"]),
                false,
            ),
        ] {
            let (url, served) = service_sent(vec![
                (200, "cannot: a skill can't do that.".to_owned()),
                (200, answer),
            ]);
            let told = make_with(
                made_in,
                None,
                request,
                &keeping,
                chat_at(url),
                NOW + made_in * 600_000,
                || Some(Vec::new()),
            );
            let sent = served.join().unwrap();
            assert_eq!(sent.len(), 2, "{request}");
            assert!(
                sent[1].contains("The request may mean the person's Linear (linear-read"),
                "{}",
                sent[1]
            );
            assert_eq!(told["code_waits_for_yes"], true, "{told}");
            let entry = made_skill::entries(&keeping.directory)
                .into_iter()
                .find(|entry| entry["name"] == told["made"])
                .unwrap();
            assert_eq!(entry["uses"]["grants"], grants, "{entry}");
            let needs = told.get("needs_link").cloned();
            if linear {
                assert_eq!(
                    needs,
                    Some(json!([{"code": "linear", "name": "Linear"}])),
                    "{told}"
                );
            } else {
                assert!(needs.is_none(), "{told}");
            }
            let _ = take_said_line();
            ASKS.lock()
                .unwrap()
                .retain(|ask| ask.directory != keeping.directory);
        }
        crate::skill_write_yes::clear_for_test();
        made_skill::set_agent_services_here(None);
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The first builder's ruling on #7261: "check the plant with the
    /// camera", asked for by a phrase, is made, the camera its grant, as a
    /// regular made skill may have it; it runs only when the phrase is said,
    /// read back first and waiting for the yes.
    #[test]
    fn with_the_switch_a_camera_task_asked_by_its_phrase_is_made() {
        no_cannot_here(Some(true));
        crate::upload_agent::set_on_here(Some(true));
        const CHECK: &str =
            "when I say check my plant, look at it with the camera and tell me if it needs water";
        let (root, keeping) = keeping("no-cannot-camera");
        let helper = manage_helper(&keeping.manage);
        let (url, served) = service(vec![
            (200, "cannot: a skill can't look at a plant.".to_owned()),
            (
                200,
                "name: plant-check\ngrants: camera, speak\nasks: check my plant".to_owned(),
            ),
        ]);
        let told = make(CHECK, &keeping, chat_at(url), NOW);
        assert_eq!(served.join().unwrap(), 2);
        assert!(helper.join().unwrap()["install_made"].is_string());
        assert_eq!(told["made"], "plant-check", "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains("I can have the lamp's agent do that, as plant check: only when you ask"),
            "{next}"
        );
        assert!(
            next.contains("I'll read it back and wait for your yes."),
            "{next}"
        );
        let entries = made_skill::entries(&keeping.directory);
        let [entry] = entries.as_slice() else {
            panic!("{entries:?}")
        };
        assert_eq!(entry["grants"], json!(["camera", "speak"]), "{entry}");
        assert_eq!(entry["agent"], true, "{entry}");
        crate::upload_agent::set_on_here(None);
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// One of the morning's failed makes (#7234): "when I say send my test
    /// note, …" runs only when asked. It is kept as an agent skill the
    /// person asks for by those words, its task the request, its grant the
    /// send's alone; on, its task not agreed, so its first ask reads it
    /// back and waits for the spoken yes. Never said as made.
    #[test]
    fn with_the_switch_a_task_only_asked_for_is_an_agent_skill_asked_by_its_words() {
        no_cannot_here(Some(true));
        made_skill::set_agent_services_here(Some(true));
        crate::upload_agent::set_on_here(Some(true));
        const NOTE: &str = "when I say send my test note, email me the words lamp test one";
        let (root, keeping) = keeping("no-cannot-asked");
        let helper = manage_helper(&keeping.manage);
        let (url, served) = service(vec![
            (
                200,
                "cannot: a skill can't hear what you say to it.".to_owned(),
            ),
            (
                200,
                "name: test-note\ngrants: gmail-write\nasks: send my test note".to_owned(),
            ),
        ]);
        let told = make(NOTE, &keeping, chat_at(url), NOW);
        assert_eq!(served.join().unwrap(), 2);
        assert!(helper.join().unwrap()["install_made"].is_string());
        assert_eq!(told["made"], "test-note", "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains(&format!(
                "I can have the lamp's agent do that, as test note: only when you ask for it, it \
                 hands one task to the lamp's agent: “{NOTE}”. The lamp's agent does it, and it \
                 prepares sends with your Gmail, each sent only on your yes. The first time you \
                 ask, I'll read it back and wait for your yes."
            )),
            "{next}"
        );
        assert!(!next.contains("I made"), "{next}");
        let entries = made_skill::entries(&keeping.directory);
        let [entry] = entries.as_slice() else {
            panic!("{entries:?}")
        };
        assert_eq!(entry["agent"], true, "{entry}");
        assert_eq!(entry["grants"], json!(["gmail-write"]), "{entry}");
        assert_eq!(
            crate::upload_agent::agent_named(&keeping.directory, "test_note").as_deref(),
            Some("test-note")
        );
        crate::upload_agent::set_on_here(None);
        made_skill::set_agent_services_here(None);
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Switch off (#7234): a `cannot:` is said as before, DeepSeek's as it
    /// wrote it and the agent's as the lamp's fixed line, with no more call;
    /// the system prompt is the same either way.
    #[test]
    fn with_the_switch_off_a_cannot_is_said_as_before() {
        let prompt = system();
        crate::make_no_cannot::set_on_here(Some(true));
        assert_eq!(system(), prompt);
        crate::make_no_cannot::set_on_here(Some(false));
        let (root, keeping) = keeping("no-cannot-off");
        let (url, served) = service(vec![(
            200,
            "cannot: a skill can't look at a plant.".to_owned(),
        )]);
        let told = make(PLANT, &keeping, chat_at(url), NOW);
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(
            told["error"], "the skill couldn't be made: a skill can't look at a plant.",
            "{told}"
        );
        let tasks = agent_answering(vec![Some("cannot: no.".to_owned())]);
        let (url, served) = service(vec![(
            200,
            "cannot: a skill can't look at a plant.".to_owned(),
        )]);
        let told = make_with_by(
            0,
            None,
            PLANT,
            &keeping,
            chat_at(url),
            NOW + 1,
            || None,
            Some(&tasks.agent()),
        );
        assert_eq!((served.join().unwrap(), tasks.hand_overs()), (1, 1));
        assert_eq!(
            told["error"],
            format!(
                "the skill couldn't be made: {}",
                crate::make_skill_agent::CANNOT_LINE
            ),
            "{told}"
        );
        assert!(made_skill::entries(&keeping.directory).is_empty());
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_TASK_NET` and what a task for the lamp's agent needs, on for
    /// this test's thread; `LAMP_MAKE_NO_CANNOT` off, as in the product.
    fn net_make_here(on: Option<bool>) {
        crate::skill_task_yes::set_net_here(on);
        made_skill::set_scripts_here(on);
        crate::skill_task_yes::set_on_here(on);
        crate::upload_agent::set_on_here(on);
        crate::make_no_cannot::set_on_here(on.map(|_| false));
    }

    /// The owner's second make on lamp-9adf, 17 Nov.
    const BITCOIN: &str =
        "when I say bitcoin price, check the Bitcoin price on the web and tell me";

    /// The one more call's answer for [`BITCOIN`].
    const BITCOIN_ASKED: &str = "name: bitcoin-price\ngrants: net, speak\nasks: bitcoin price";

    /// The kept agent skill for [`BITCOIN`]: asked for by its words, granted
    /// `net`, said back as a task for the lamp's agent that reads back and
    /// waits for the yes at its first ask; never said as made.
    fn assert_bitcoin_kept(told: &Value, keeping: &Keeping) {
        assert_eq!(told["made"], "bitcoin-price", "{told}");
        let next = told["next"].as_str().unwrap();
        let uses = crate::skill_task_yes::uses_said(&["net".to_owned(), "speak".to_owned()]);
        assert!(uses.contains("reaches the public internet"), "{uses}");
        assert!(
            next.contains(&format!(
                "I can have the lamp's agent do that, as bitcoin price: only when you ask for it, \
                 it hands one task to the lamp's agent: “{BITCOIN}”. The lamp's agent does it, \
                 and {uses}. The first time you ask, I'll read it back and wait for your yes."
            )),
            "{next}"
        );
        assert!(!next.contains("I made"), "{next}");
        let entries = made_skill::entries(&keeping.directory);
        let [entry] = entries.as_slice() else {
            panic!("{entries:?}")
        };
        assert_eq!(entry["agent"], true, "{entry}");
        assert_eq!(entry["grants"], json!(["net", "speak"]), "{entry}");
        assert_eq!(
            crate::upload_agent::agent_named(&keeping.directory, "bitcoin_price").as_deref(),
            Some("bitcoin-price")
        );
    }

    /// lamp-9adf, 17 Nov: with `LAMP_TASK_NET` on, the make's prompt told
    /// DeepSeek and Hermes a price is "anything from the internet" a skill
    /// can't find out, so both answered `cannot:`. Now the model answers
    /// `internet:` for it, never handed to the agent to write; the lamp's
    /// own model picks its grants and words in one more call, and a make
    /// asked for by its words is an agent skill granted `net`, with
    /// `LAMP_MAKE_NO_CANNOT` off.
    #[test]
    fn with_task_net_a_web_make_asked_by_its_words_is_an_agent_skill_granted_net() {
        net_make_here(Some(true));
        let (root, keeping) = keeping("net-asked");
        let helper = manage_helper(&keeping.manage);
        let tasks = crate::make_skill_agent::tests::stub(
            Some("hermes"),
            std::sync::Arc::new(|_: &str| Some("cannot: never asked.".to_owned())),
        );
        let (url, served) = service_sent(vec![
            (200, "internet: the Bitcoin price".to_owned()),
            (200, BITCOIN_ASKED.to_owned()),
        ]);
        let told = make_with_by(
            0,
            None,
            BITCOIN,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&tasks.agent()),
        );
        let sent = served.join().unwrap();
        assert_eq!((sent.len(), tasks.hand_overs()), (2, 0), "{told}");
        assert!(
            sent[0].contains("the lamp's agent looks it up each time"),
            "{}",
            sent[0]
        );
        assert!(
            sent[1].contains("You choose, for a lamp, when a skill runs and what it needs"),
            "{}",
            sent[1]
        );
        assert!(helper.join().unwrap()["install_made"].is_string());
        assert_bitcoin_kept(&told, &keeping);
        net_make_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The same make when DeepSeek's answer isn't kept and the lamp's agent
    /// (Hermes, told the same rules) answers `internet:`: made the same
    /// way, the agent's line never said.
    #[test]
    fn with_task_net_the_agents_internet_line_is_made_the_same_way() {
        net_make_here(Some(true));
        let (root, keeping) = keeping("net-agent");
        let helper = manage_helper(&keeping.manage);
        let tasks = crate::make_skill_agent::tests::stub(
            Some("hermes"),
            std::sync::Arc::new(|rules: &str| {
                assert!(rules.contains(made_skill::BRIEF_HEAD), "{rules}");
                Some("internet: PRIVATE-DETAIL-7's Bitcoin price".to_owned())
            }),
        );
        let (url, served) = service(vec![
            (
                200,
                "cannot: I can't do that yet: checking a price.".to_owned(),
            ),
            (200, BITCOIN_ASKED.to_owned()),
        ]);
        let told = make_with_by(
            0,
            None,
            BITCOIN,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&tasks.agent()),
        );
        assert_eq!((served.join().unwrap(), tasks.hand_overs()), (2, 1));
        assert!(helper.join().unwrap()["install_made"].is_string());
        assert!(!told.to_string().contains("PRIVATE-DETAIL-7"), "{told}");
        assert_bitcoin_kept(&told, &keeping);
        net_make_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The agent's web code for a Bitcoin price ([`net_code_told`]), on a
    /// trigger: it prints the price, and the agent's answer is the told
    /// answer (#8610).
    fn net_code_main() -> String {
        "import json\nimport urllib.request\n\n\
             URL = \"https://api.coinbase.com/v2/prices/BTC-USD/spot\"\n\n\n\
             def fetch():\n    \
             with urllib.request.urlopen(URL, timeout=15) as reply:\n        \
             return json.load(reply)[\"data\"][\"amount\"]\n\n\n\
             try:\n    \
             print(f\"Bitcoin is {fetch()} US dollars.\")\n\
             except Exception:\n    \
             print(\"The Bitcoin price could not be reached just now.\")\n"
            .to_owned()
    }

    /// #7742: with `LAMP_TASK_NET_GATEWAY` on and OpenClaw the lamp's
    /// agent (a gateway agent whose run granted `net` reaches the web from
    /// its own skill-start fence), DeepSeek's `internet:` is handed to the
    /// agent, whose rules tell it to write the skill's own code granted
    /// `net` (never `internet:`); its answer, a task step whose script
    /// fetches over https, is kept granted `net` and waits for the yes to
    /// its code, as any made code does. DeepSeek's own prompt is as before
    /// but for one sentence after it ([`net_not_agent_task_told`]); with
    /// the switch off, or on Hermes, nothing changes.
    #[test]
    fn with_gateway_net_an_openclaw_make_writes_its_own_code_granted_net() {
        if made_skill::a_python_here().is_none() {
            eprintln!("skipped: no python3 here to parse a made script with");
            return;
        }
        net_make_here(Some(true));
        crate::skill_task_yes::set_gateway_net_here(Some(true));
        made_skill::set_code_at_once_here(Some(false));
        let (root, keeping) = keeping("net-gateway-code");
        let toml = CODE_TOML
            .replace("grants = [\"camera\"]", "grants = [\"net\"]")
            .replace("take a photo", "fetch the Bitcoin price from the web");
        let main = net_code_main();
        let answer = format!(
            "```skill.toml\n{toml}```\n\n```SKILL.md\n{AGENT_CODE_CARD}```\n\n```scripts/main.py\n{main}```\n"
        );
        let rules_seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let tasks = {
            let rules_seen = rules_seen.clone();
            crate::make_skill_agent::tests::stub(
                Some("openclaw"),
                std::sync::Arc::new(move |rules: &str| {
                    rules_seen.lock().unwrap().push(rules.to_owned());
                    Some(answer.clone())
                }),
            )
        };
        let agent = crate::make_skill_agent::Agent {
            one_path: true,
            ..tasks.agent()
        };
        let (url, served) = service_sent(vec![(200, "internet: the Bitcoin price".to_owned())]);
        let told = make_with_by(
            0,
            None,
            BITCOIN,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&agent),
        );
        let sent = served.join().unwrap();
        assert_eq!((sent.len(), tasks.hand_overs()), (1, 1), "{told}");
        // DeepSeek is told as before, and that a task for the agent is
        // only for a linked service; the agent the code way alone.
        // The body is JSON: its quotes escaped.
        let told_net = serde_json::to_string(&net_told()).unwrap();
        assert!(sent[0].contains(told_net.trim_matches('"')), "{}", sent[0]);
        assert!(sent[0].contains(&net_not_agent_task_told()), "{}", sent[0]);
        let rules = rules_seen.lock().unwrap()[0].clone();
        assert!(rules.contains(&net_code_told()), "{rules}");
        assert!(!rules.contains(&net_told()), "{rules}");
        assert_eq!(told["made"], "photo-booth", "{told}");
        assert_eq!(told["code_waits_for_yes"], true, "{told}");
        let entry = &made_skill::entries(&keeping.directory)[0];
        assert_eq!(entry["uses"]["grants"], json!(["net"]), "{entry}");
        assert_ne!(entry["on"], true, "{entry}");
        // Off on this thread once the make is done: code a model wrote is
        // never granted `net` outside it.
        assert!(!made_skill::net_code_here_for_tests());
        // The switch off, or Hermes: never the code way.
        assert!(net_code_by("OpenClaw"));
        assert!(!net_code_by("Hermes"));
        crate::skill_task_yes::set_gateway_net_here(Some(false));
        assert!(!net_code_by("OpenClaw"));
        crate::skill_task_yes::set_gateway_net_here(None);
        made_skill::set_code_at_once_here(None);
        net_make_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7901, the settled design: an OpenClaw make's own code granted
    /// `net` beside the camera and the skill's own notes is made and kept
    /// with all its grants, waiting for the yes to its code as any made
    /// code does; nothing is refused beside `net`.
    #[test]
    fn with_gateway_net_a_make_granted_net_beside_the_camera_and_notes_is_kept() {
        if made_skill::a_python_here().is_none() {
            eprintln!("skipped: no python3 here to parse a made script with");
            return;
        }
        net_make_here(Some(true));
        crate::skill_task_yes::set_gateway_net_here(Some(true));
        made_skill::set_code_at_once_here(Some(false));
        let (root, keeping) = keeping("net-camera-notes");
        let toml = CODE_TOML
            .replace(
                "grants = [\"camera\"]",
                "grants = [\"net\", \"camera\", \"notes\"]",
            )
            .replace("take a photo", "fetch the Bitcoin price from the web");
        let main = net_code_main();
        let answer = format!(
            "```skill.toml\n{toml}```\n\n```SKILL.md\n{AGENT_CODE_CARD}```\n\n```scripts/main.py\n{main}```\n"
        );
        let tasks = crate::make_skill_agent::tests::stub(
            Some("openclaw"),
            std::sync::Arc::new(move |_: &str| Some(answer.clone())),
        );
        let agent = crate::make_skill_agent::Agent {
            one_path: true,
            ..tasks.agent()
        };
        let (url, served) = service_sent(vec![(200, "internet: the Bitcoin price".to_owned())]);
        let told = make_with_by(
            0,
            None,
            BITCOIN,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&agent),
        );
        served.join().unwrap();
        assert_eq!(told["made"], "photo-booth", "{told}");
        assert_eq!(told["code_waits_for_yes"], true, "{told}");
        let entry = &made_skill::entries(&keeping.directory)[0];
        assert_eq!(
            entry["uses"]["grants"],
            json!(["net", "camera", "notes"]),
            "{entry}"
        );
        crate::skill_task_yes::set_gateway_net_here(None);
        made_skill::set_code_at_once_here(None);
        net_make_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7742, lamp-9adf 18 Nov 21:37: an OpenClaw make's own code granted
    /// `net`, kept and said back, then put on after the owner's yes. The
    /// manage helper is another process, with no make on its thread: its
    /// check (`put_on_cap`, `put_on_scripts`, as lamp-skills'
    /// `install_made` runs them) on the very files the voice packed lets
    /// it on with `LAMP_TASK_NET` and `LAMP_TASK_NET_GATEWAY` on in its
    /// unit, and refuses `net` with the gateway switch off.
    #[test]
    fn with_gateway_net_an_openclaw_net_skill_kept_is_put_on_by_the_helper() {
        if made_skill::a_python_here().is_none() {
            eprintln!("skipped: no python3 here to parse a made script with");
            return;
        }
        net_make_here(Some(true));
        crate::skill_task_yes::set_gateway_net_here(Some(true));
        made_skill::set_code_at_once_here(Some(false));
        let (root, mut keeping) = keeping("net-gateway-put-on");
        // The pack keeps a copy of the folder it packed: the helper's bytes.
        let packed = root.join("packed");
        std::fs::write(
            &keeping.program,
            format!(
                "#!/bin/sh\nname=$(basename \"$4\")\ncp -R \"$4\" '{}'\n\
                 printf 'tar of %s' \"$name\" > \"$3/$name-1.0.0.tar\"\n",
                packed.display()
            ),
        )
        .unwrap();
        keeping.installed = root.join("skills");
        let toml = CODE_TOML
            .replace("grants = [\"camera\"]", "grants = [\"net\"]")
            .replace("take a photo", "fetch the Bitcoin price from the web");
        let main = net_code_main();
        let answer = format!(
            "```skill.toml\n{toml}```\n\n```SKILL.md\n{AGENT_CODE_CARD}```\n\n```scripts/main.py\n{main}```\n"
        );
        let tasks = crate::make_skill_agent::tests::stub(
            Some("openclaw"),
            std::sync::Arc::new(move |_: &str| Some(answer.clone())),
        );
        let agent = crate::make_skill_agent::Agent {
            one_path: true,
            ..tasks.agent()
        };
        let (url, served) = service_sent(vec![(200, "internet: the Bitcoin price".to_owned())]);
        let told = make_with_by(
            0,
            None,
            BITCOIN,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&agent),
        );
        served.join().unwrap();
        assert_eq!(told["code_waits_for_yes"], true, "{told}");
        let entry = &made_skill::entries(&keeping.directory)[0];
        assert_eq!(entry["uses"]["grants"], json!(["net"]), "{entry}");
        // The helper's check, on its own thread with its unit's switches.
        let helper = |gateway: bool| {
            let packed = packed.clone();
            std::thread::spawn(move || {
                crate::skill_task_yes::set_net_here(Some(true));
                made_skill::set_scripts_here(Some(true));
                crate::skill_task_yes::set_gateway_net_here(Some(gateway));
                let mut files = Vec::new();
                let mut folders = vec![packed.clone()];
                while let Some(folder) = folders.pop() {
                    for entry in std::fs::read_dir(&folder).unwrap() {
                        let path = entry.unwrap().path();
                        if path.is_dir() {
                            folders.push(path);
                        } else {
                            let name = path.strip_prefix(&packed).unwrap();
                            let name = name.to_str().unwrap().to_owned();
                            files.push((name, std::fs::read(&path).unwrap()));
                        }
                    }
                }
                let toml = std::fs::read_to_string(packed.join("skill.toml")).unwrap();
                made_skill::put_on_cap(&toml)
                    .and_then(|(_, uses)| {
                        made_skill::put_on_scripts(
                            &toml,
                            &uses,
                            files
                                .iter()
                                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
                        )
                    })
                    .map_err(|refused| refused.0)
            })
            .join()
            .unwrap()
        };
        assert_eq!(helper(true), Ok(()));
        // The gateway switch off in the helper: `net` is still refused.
        assert!(helper(false).is_err());
        crate::skill_task_yes::set_gateway_net_here(None);
        made_skill::set_code_at_once_here(None);
        net_make_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7742, lamp-c0a4 18 Nov 20:06: on OpenClaw with
    /// `LAMP_TASK_NET_GATEWAY` on, DeepSeek answered a web make ("every 3
    /// minutes while I'm here ... the current bitcoin price") not with
    /// `internet:` but as a task for the lamp's agent: one `do.task`,
    /// granted `speak`, its script the services example's. It is never
    /// kept as DeepSeek's: handed to the agent, which writes its own code
    /// granted `net`.
    #[test]
    fn with_gateway_net_deepseeks_web_task_for_the_agent_goes_to_openclaw_as_net_code() {
        deepseeks_web_task_goes_to_openclaw(
            "net-gateway-web-task",
            AGENT_SERVICES_SCRIPT,
            "Fetch the current Bitcoin price online and tell me the price and how much it changed",
        );
    }

    /// The same at 20:18, from the app's MAKE IT ("gold price"): DeepSeek's
    /// script printed the `internet:` line itself.
    #[test]
    fn with_gateway_net_deepseeks_internet_line_in_its_script_goes_to_openclaw_as_net_code() {
        deepseeks_web_task_goes_to_openclaw(
            "net-gateway-web-line",
            "print(\"internet: the current gold price\")",
            "Fetch current gold price online and tell it to the owner shortly",
        );
    }

    /// DeepSeek's answer, one `do.task` granted `speak` whose script is
    /// `script`, made as [`make_with_by`] makes it with OpenClaw the agent.
    fn deepseeks_web_task_goes_to_openclaw(name: &str, script: &str, task: &str) {
        if made_skill::a_python_here().is_none() {
            eprintln!("skipped: no python3 here to parse a made script with");
            return;
        }
        net_make_here(Some(true));
        crate::skill_task_yes::set_gateway_net_here(Some(true));
        made_skill::set_code_at_once_here(Some(false));
        let (root, keeping) = keeping(name);
        let deepseeks = format!(
            "```skill.toml\n{}```\n\n```SKILL.md\n{}```\n\n```scripts/main.py\n{script}\n```\n",
            CODE_TOML
                .replace("grants = [\"camera\"]", "grants = [\"speak\"]")
                .replace("take a photo", task),
            CODE_CARD.replace("photo-booth", "web-watch"),
        );
        assert!(web_task_for_agent(&deepseeks), "{deepseeks}");
        let toml = CODE_TOML
            .replace("grants = [\"camera\"]", "grants = [\"net\"]")
            .replace("take a photo", "fetch the Bitcoin price from the web");
        let main = net_code_main();
        let answer = format!(
            "```skill.toml\n{toml}```\n\n```SKILL.md\n{AGENT_CODE_CARD}```\n\n```scripts/main.py\n{main}```\n"
        );
        let tasks = crate::make_skill_agent::tests::stub(
            Some("openclaw"),
            std::sync::Arc::new(move |rules: &str| {
                assert!(rules.contains(&net_code_told()), "{rules}");
                Some(answer.clone())
            }),
        );
        let agent = crate::make_skill_agent::Agent {
            one_path: true,
            ..tasks.agent()
        };
        let (url, served) = service_sent(vec![(200, deepseeks)]);
        let request = "every 3 minutes while I'm here, run a script that fetches the current \
                       bitcoin price online and tells me the price and how much it changed";
        let told = make_with_by(
            0,
            None,
            request,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&agent),
        );
        let sent = served.join().unwrap();
        assert_eq!((sent.len(), tasks.hand_overs()), (1, 1), "{told}");
        assert!(sent[0].contains(&net_not_agent_task_told()), "{}", sent[0]);
        assert_eq!(told["made"], "photo-booth", "{told}");
        let entries = made_skill::entries(&keeping.directory);
        let [entry] = entries.as_slice() else {
            panic!("{entries:?}")
        };
        assert_eq!(entry["uses"]["grants"], json!(["net"]), "{entry}");
        assert!(!made_skill::net_code_here_for_tests());
        crate::skill_task_yes::set_gateway_net_here(None);
        made_skill::set_code_at_once_here(None);
        net_make_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What DeepSeek's answer is held as a web task for the agent by
    /// ([`web_task_for_agent`]): only a script that says the task is the
    /// agent's, with no linked service's grant. A service skill made as a
    /// task for the agent, and a skill with real code, are not.
    #[test]
    fn a_web_task_for_the_agent_is_one_with_no_service_grant() {
        made_skill::set_scripts_here(Some(true));
        assert!(AGENT_SERVICES_EXAMPLE.contains(AGENT_SERVICES_SCRIPT));
        assert!(!web_task_for_agent(AGENT_SERVICES_EXAMPLE));
        let web =
            AGENT_SERVICES_EXAMPLE.replace("grants = [\"gmail-read\"]", "grants = [\"speak\"]");
        assert!(web_task_for_agent(&web), "{web}");
        let lamps = web.replace(
            AGENT_SERVICES_SCRIPT,
            crate::make_no_cannot::SCRIPT.trim_end(),
        );
        assert!(web_task_for_agent(&lamps), "{lamps}");
        let code = web.replace(AGENT_SERVICES_SCRIPT, "print(\"Bitcoin is high.\")");
        assert!(!web_task_for_agent(&code), "{code}");
        // lamp-c0a4, 18 Nov 20:18, from the app: the `internet:` line in
        // its script.
        let gold = web.replace(
            AGENT_SERVICES_SCRIPT,
            "print(\"internet: the current gold price\")",
        );
        assert!(web_task_for_agent(&gold), "{gold}");
        let service_gold = gold.replace("grants = [\"speak\"]", "grants = [\"gmail-read\"]");
        assert!(!web_task_for_agent(&service_gold), "{service_gold}");
        made_skill::set_scripts_here(None);
    }

    /// The owner's first make (lamp-9adf, 17 Nov), on a time, and the
    /// owner's ruling of 17 Nov: a skill that runs on its own trigger may
    /// have `net` with `LAMP_TASK_NET` on. Kept as a made task for the
    /// lamp's agent on its trigger, granted `net` and `speak`, said back as
    /// one that reaches the internet, and waiting for the spoken yes. The
    /// journal names neither the request nor the model's words.
    #[test]
    fn with_task_net_a_timed_web_make_is_kept_on_its_trigger_granted_net() {
        net_make_here(Some(true));
        let (root, keeping) = keeping("net-timed");
        let (url, served) = service_sent(vec![
            (200, "internet: the Bitcoin price".to_owned()),
            (
                200,
                plant_answer("net, speak").replace("plant-check", "bitcoin-daily"),
            ),
        ]);
        let request = "check the Bitcoin price every day at eight in the morning and tell me";
        let told = make(request, &keeping, chat_at(url), NOW);
        let sent = served.join().unwrap();
        assert_eq!(sent.len(), 2, "{told}");
        assert!(
            sent[1].contains("You choose, for a lamp, when a skill runs and what it needs"),
            "{}",
            sent[1]
        );
        assert_eq!(told["made"], "bitcoin-daily", "{told}");
        assert_eq!(told["code_waits_for_yes"], true, "{told}");
        assert_eq!(told["on"], false, "{told}");
        // On its trigger, never `speak` (#8610).
        let grants = ["net".to_owned()];
        let description = told["description"].as_str().unwrap();
        assert!(
            description.contains(&format!("hands one task to the lamp's agent: “{request}”")),
            "{description}"
        );
        let uses = crate::skill_task_yes::uses_said(&grants);
        assert!(uses.contains("reaches the public internet"), "{uses}");
        assert!(
            description.ends_with(&format!(
                "The lamp's agent does it, and {uses}. It runs only once you agree."
            )),
            "{description}"
        );
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains("\"I can have the lamp's agent do that, as bitcoin daily: every day"),
            "{next}"
        );
        assert!(!next.contains("I made"), "{next}");
        let entries = made_skill::entries(&keeping.directory);
        let [entry] = entries.as_slice() else {
            panic!("{entries:?}")
        };
        assert_eq!(entry["uses"]["grants"], json!(grants), "{entry}");
        assert_eq!(entry["uses"]["agent_task"], true, "{entry}");
        assert!(take_said_line().is_some_and(|line| !line.contains("Bitcoin")));
        net_make_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The switch off: a make whose one more call names `net` on a trigger
    /// (`LAMP_MAKE_NO_CANNOT` on, the model's `cannot:`) is still refused
    /// with its reason, and nothing is kept.
    #[test]
    fn with_task_net_off_a_timed_net_make_is_still_refused_net_off() {
        no_cannot_here(Some(true));
        crate::skill_task_yes::set_net_here(Some(false));
        let (root, keeping) = keeping("net-timed-off");
        let (url, served) = service(vec![
            (
                200,
                "cannot: I can't do that yet: checking a price.".to_owned(),
            ),
            (200, plant_answer("net, speak")),
        ]);
        let request = "check the Bitcoin price every day at eight in the morning and tell me";
        let (made, line) = made_and_journaled(
            request,
            None,
            made_skill::brief(request).unwrap(),
            &keeping,
            chat_at(url),
            "local:1:1",
        );
        assert_eq!(served.join().unwrap(), 2);
        let net_off = crate::make_no_cannot::Uncovered::NetOff.line();
        assert_eq!(made, Err(Refused(net_off, None)));
        assert!(!line.contains("Bitcoin"), "{line}");
        assert!(made_skill::entries(&keeping.directory).is_empty());
        crate::skill_task_yes::set_net_here(None);
        no_cannot_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// With `LAMP_TASK_NET` off (or what a task for the agent needs off),
    /// the make's prompt is as before, byte for byte: what only the
    /// internet tells is the model's `cannot:`, and no `internet:` line is
    /// asked for. On, the agent's rules are this same prompt.
    #[test]
    fn with_task_net_off_the_prompt_is_as_before() {
        const BEFORE: &str = "A skill also can't find out anything at the time it runs that \
             no step here or below gives: the weather or a forecast, the news, a web lookup, a \
             price, a summary of mail or calendar content no read below gives, anything from the \
             internet. A request that needs it is answered the same way, one line and no blocks: \
             cannot: \"I can't do that yet:\" and what, in plain words (\"I can't do that yet: \
             checking the weather for you.\"); never a say telling the person to check, look up \
             or read it themselves. Never stand something else in for it";
        for reads in [false, true] {
            for agent in [false, true] {
                let off = system_net(reads, agent, false);
                assert!(off.contains(BEFORE), "{off}");
                assert!(!off.contains(crate::make_no_cannot::NEEDS_NET), "{off}");
                let on = system_net(reads, agent, true);
                assert!(!on.contains(BEFORE), "{on}");
                assert!(
                    on.contains("answered with one line and no blocks: internet: and what"),
                    "{on}"
                );
                assert!(on.contains("never cannot: for it"), "{on}");
            }
        }
        net_make_here(Some(true));
        crate::skill_task_yes::set_net_here(Some(false));
        assert_eq!(system(), system_net(made_skill::reads_on(), false, false));
        assert!(whole("internet: the price".to_owned(), false).is_err());
        crate::skill_task_yes::set_net_here(Some(true));
        assert_eq!(system(), system_net(made_skill::reads_on(), false, true));
        assert!(whole("internet: the price".to_owned(), true).is_ok());
        net_make_here(None);
    }

    /// A make's answer of `toml` and a card named `name`, as a model
    /// writes it, for the quick-reaction route's tests (#6811).
    fn route_answer(toml: &str, name: &str) -> String {
        format!(
            "```skill.toml\n{toml}```\n\n```SKILL.md\n---\nname: {name}\ndescription: A test \
             skill.\n---\n\n# Test\n\nA test skill.\n```\n"
        )
    }

    /// A made skill with a say on `trigger`, the lamp's voice granted.
    fn say_on(topic: &str, trigger: &str) -> String {
        format!(
            "kind = \"installable\"\nversion = \"1.0.0\"\nneeds = [\"voice\"]\n\n\
             [permissions]\nspeak = true\ntopics = [\"{topic}\"]\n\n[trigger]\n{trigger}\
             cooldown = \"1h\"\n\n[ask]\npolicy = \"nudge\"\n\n[[do]]\nsay = {{ text = \"Time \
             for a sip of water.\" }}\n"
        )
    }

    /// Someone at the desk half an hour, then a line: DeepSeek's draft
    /// with no guards.
    fn presence_say() -> String {
        say_on(
            "room/presence",
            "topic = \"room/presence\"\nfield = \"someone\"\nequals = true\nfor = \"30m\"\n\
             rearm = \"ends\"\n",
        )
    }

    /// A pat on the head, a nod: a quick reaction.
    fn pat_nod() -> String {
        "kind = \"installable\"\nversion = \"1.0.0\"\nneeds = [\"body\"]\n\n[permissions]\n\
         body = true\ntopics = [\"controls/pet\"]\n\n[trigger]\ntopic = \"controls/pet\"\n\
         field = \"ended\"\nequals = false\nrearm = \"ends\"\n\n[ask]\npolicy = \"nudge\"\n\n\
         [[do]]\nbody = { gesture = \"nod\" }\n"
            .to_owned()
    }

    /// The made index's `made_with` of the skill `name`, if it has one.
    fn made_with_of(keeping: &Keeping, name: &str) -> Option<String> {
        made_skill::entries(&keeping.directory)
            .iter()
            .find(|entry| entry["name"] == name)
            .and_then(|entry| entry[made_skill::MADE_WITH].as_str().map(str::to_owned))
    }

    /// [`keeping`], its pack stand-in also copying the `skill.toml` it
    /// packs to `last-skill.toml` in the test's folder, so a test reads
    /// what was kept.
    fn keeping_copied(name: &str) -> (PathBuf, Keeping, PathBuf) {
        let (root, keeping) = keeping(name);
        let copy = root.join("last-skill.toml");
        std::fs::write(
            &keeping.program,
            format!(
                "#!/bin/sh\nname=$(basename \"$4\")\ncp \"$4/skill.toml\" \"{}\"\n\
                 printf 'tar of %s' \"$name\" > \"$3/$name-1.0.0.tar\"\n",
                copy.display()
            ),
        )
        .unwrap();
        (root, keeping, copy)
    }

    /// The kept `skill.toml` at `copy`: one step, a task whose words are
    /// `words`, run by `run`, and no say anywhere.
    fn one_task_of(copy: &Path, words: &str, run: &str) -> toml::Table {
        let text = std::fs::read_to_string(copy).unwrap();
        let table: toml::Table = toml::from_str(&text).unwrap();
        let steps = table["do"].as_array().unwrap();
        assert_eq!(steps.len(), 1, "{text}");
        assert_eq!(steps[0]["task"]["task"].as_str(), Some(words), "{text}");
        assert_eq!(steps[0]["task"]["run"].as_str(), Some(run), "{text}");
        assert!(!text.contains("say"), "no say step at all: {text}");
        table
    }

    /// The one more call's answer: `name`, `grants` and a trigger on
    /// `topic` with `trigger`'s keys.
    fn trigger_answer(name: &str, grants: &str, topic: &str, trigger: &str) -> String {
        format!(
            "name: {name}\ngrants: {grants}\n```skill.toml\n[permissions]\n\
             topics = [\"{topic}\"]\n\n[trigger]\ntopic = \"{topic}\"\n{trigger}```\n"
        )
    }

    /// The judgement's answer ([`made_skill::REACTION_JUDGE`]).
    fn judgement(reaction: bool) -> (u16, String) {
        (
            200,
            format!("{{\"quick_reaction\": {reaction}, \"reason\": \"PRIVATE-REASON-7\"}}"),
        )
    }

    /// Whether `sent` is the judgement's call, asking of `request` alone.
    fn is_judgement(sent: &str, request: &str) -> bool {
        sent.contains("You judge, for a desk lamp, what kind of skill")
            && sent.contains(&format!("Request: {request}"))
            && !sent.contains(made_skill::BRIEF_HEAD)
    }

    /// The first builder's 19:50 ruling on #6811 (the middle form) and the
    /// owner's "do it like V1" on #7764, with
    /// `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on: the make's first call is
    /// DeepSeek's judgement of the request's meaning. Judged no quick
    /// reaction, no draft is ever asked for: the lamp writes the skill
    /// itself, the trigger its own model picks in one more call and one
    /// task step whose words are the person's request, done by the lamp's
    /// agent when it fires; no say, no other step, sip-of-water's guards in
    /// `[[while]]` but on the lamp's clock, `made_with` the agent's. The
    /// agent is never asked to write it.
    #[test]
    fn with_the_switch_a_make_judged_no_reaction_is_its_trigger_and_one_task_never_drafted() {
        made_skill::set_reactions_here(Some(true));
        net_make_here(Some(true));
        let (root, keeping, copy) = keeping_copied("reactions-middle");
        const BTC: &str = "tell me the BTC price at 9";
        const SIP: &str = "after I've sat a long while, suggest a sip of water";
        let nine = "field = \"known\"\nequals = true\nrearm = \"ends\"\n\
                    during = { from = \"09:00\", to = \"09:01\" }\n";
        let sat = "field = \"someone\"\nequals = true\nfor = \"30m\"\nrearm = \"leaves\"\n";
        for (at, (request, answer, name, topic)) in [
            (
                BTC,
                trigger_answer("btc-price", "net, speak", "clock/local", nine),
                "btc-price",
                "clock/local",
            ),
            (
                SIP,
                trigger_answer("sip-at-desk", "speak", "room/presence", sat),
                "sip-at-desk",
                "room/presence",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let tasks = agent_answering(vec![]);
            let (url, served) = service_sent(vec![judgement(false), (200, answer)]);
            let told = make_with_by(
                0,
                None,
                request,
                &keeping,
                chat_at(url),
                NOW + at as u64,
                || None,
                Some(&tasks.agent()),
            );
            let sent = served.join().unwrap();
            assert_eq!((sent.len(), tasks.hand_overs()), (2, 0), "{told}");
            assert!(is_judgement(&sent[0], request), "{}", sent[0]);
            assert!(
                sent[1].contains("You choose, for a lamp, when a skill runs and what it needs"),
                "never DeepSeek's draft: {}",
                sent[1]
            );
            assert_eq!(told["made"], name, "{told}");
            assert_eq!(told["code_waits_for_yes"], true, "{told}");
            assert!(!told.to_string().contains("PRIVATE-REASON-7"), "{told}");
            let table = one_task_of(&copy, request, crate::make_no_cannot::RUN);
            assert_eq!(table["trigger"]["topic"].as_str(), Some(topic));
            // Never a guard on the lamp's clock: 9:00's run is never spent
            // waiting (the first builder, 19 Nov 01:29 and 02:41).
            if topic == "clock/local" {
                assert!(table.get("while").is_none(), "{table:?}");
                assert_eq!(made_with_of(&keeping, name).as_deref(), Some("hermes"));
                continue;
            }
            let guards = table["while"].as_array().unwrap();
            assert_eq!(guards.len(), 3, "sip-of-water's guards: {table:?}");
            let topics = table["permissions"]["topics"].as_array().unwrap();
            for guard in ["voice/state", "room/activity"] {
                assert!(
                    topics.iter().any(|t| t.as_str() == Some(guard)),
                    "{table:?}"
                );
            }
            assert_eq!(made_with_of(&keeping, name).as_deref(), Some("hermes"));
        }
        net_make_here(None);
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lamp-9adf, 18 Nov: "Lamp, make a skill that reminds me to stretch
    /// when I've been sitting a long while." was made on someone getting
    /// up. With `LAMP_MAKE_REACTIONS_ONLY_DEEPSEEK` on, the one more call
    /// is told sip-of-water's sitting with a whole answer for it, and a
    /// model answering so keeps sip-of-water's `for`, once-a-sitting `rearm`
    /// and `cooldown` in the kept `skill.toml`, on the desk's presence. "When
    /// I come back to my desk" keeps its arrival, never the desk left.
    #[test]
    fn with_the_switch_a_long_sitting_is_sip_of_waters_and_a_coming_back_an_arrival() {
        made_skill::set_reactions_here(Some(true));
        net_make_here(Some(true));
        let (root, keeping, copy) = keeping_copied("reactions-sitting");
        let shipped: toml::Table =
            toml::from_str(include_str!("../../../skills/sip-of-water/skill.toml")).unwrap();
        let sip = shipped["trigger"].as_table().unwrap();
        const STRETCH: &str =
            "Lamp, make a skill that reminds me to stretch when I've been sitting a long while.";
        const BACK: &str = "Lamp, make a skill that tells me the news when I come back to my desk.";
        let sat = format!(
            "field = \"someone\"\nequals = true\nfor = \"{}\"\nrearm = \"{}\"\n\
             cooldown = \"{}\"\n",
            sip["for"].as_str().unwrap(),
            sip["rearm"].as_str().unwrap(),
            sip["cooldown"].as_str().unwrap(),
        );
        let back = "field = \"someone\"\nequals = true\nwhen = \"appears\"\n\
                    absent_for = \"5m\"\n";
        for (at, (request, answer, name)) in [
            (
                STRETCH,
                trigger_answer("stretch-break", "speak", "room/presence", &sat),
                "stretch-break",
            ),
            (
                BACK,
                trigger_answer("desk-news", "net, speak", "room/presence", back),
                "desk-news",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let tasks = agent_answering(vec![]);
            let (url, served) = service_sent(vec![judgement(false), (200, answer)]);
            let told = make_with_by(
                0,
                None,
                request,
                &keeping,
                chat_at(url),
                NOW + at as u64,
                || None,
                Some(&tasks.agent()),
            );
            let sent = served.join().unwrap();
            assert_eq!((sent.len(), tasks.hand_overs()), (2, 0), "{told}");
            assert!(
                sent[1].contains("sip-of-water") && sent[1].contains("stretch"),
                "the sitting told, with its example: {}",
                sent[1]
            );
            assert_eq!(told["made"], name, "{told}");
            let words = crate::make_no_cannot::task_words(request);
            let table = one_task_of(&copy, &words, crate::make_no_cannot::RUN);
            let trigger = table["trigger"].as_table().unwrap();
            assert_eq!(
                trigger["topic"].as_str(),
                Some("room/presence"),
                "{table:?}"
            );
            assert_eq!(trigger["field"].as_str(), Some("someone"), "{table:?}");
            assert_eq!(trigger["equals"].as_bool(), Some(true), "{table:?}");
            if request == STRETCH {
                for key in ["for", "rearm", "cooldown"] {
                    assert_eq!(trigger.get(key), sip.get(key), "{key}: {table:?}");
                }
                assert!(!trigger.contains_key("when"), "{table:?}");
            } else {
                assert_eq!(trigger["when"].as_str(), Some("appears"), "{table:?}");
                assert!(!trigger.contains_key("for"), "{table:?}");
            }
            assert_eq!(table["while"].as_array().map(Vec::len), Some(3));
        }
        net_make_here(None);
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The owner's "do it like V1": a judgement with no answer, or one that
    /// isn't sure, sends the make straight to the agent's middle form,
    /// never to a DeepSeek draft.
    #[test]
    fn with_the_switch_a_failed_judgement_goes_to_the_agent() {
        made_skill::set_reactions_here(Some(true));
        net_make_here(Some(true));
        let (root, keeping, copy) = keeping_copied("reactions-unsure");
        const SIP: &str = "after I've sat a long while, suggest a sip of water";
        let sat = "field = \"someone\"\nequals = true\nfor = \"30m\"\nrearm = \"leaves\"\n";
        for (at, judgement) in [
            (200, "maybe, it depends".to_owned()),
            (200, "{\"quick_reaction\": \"unsure\"}".to_owned()),
        ]
        .into_iter()
        .enumerate()
        {
            let tasks = agent_answering(vec![]);
            let name = format!("sip-at-desk-{at}");
            let (url, served) = service_sent(vec![
                judgement,
                (200, trigger_answer(&name, "speak", "room/presence", sat)),
            ]);
            let told = make_with_by(
                0,
                None,
                SIP,
                &keeping,
                chat_at(url),
                NOW + at as u64,
                || None,
                Some(&tasks.agent()),
            );
            let sent = served.join().unwrap();
            assert_eq!((sent.len(), tasks.hand_overs()), (2, 0), "{told}");
            assert!(is_judgement(&sent[0], SIP), "{}", sent[0]);
            assert_eq!(told["made"], name.as_str(), "{told}");
            one_task_of(&copy, SIP, crate::make_no_cannot::RUN);
        }
        net_make_here(None);
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The 19:50 ruling and "do it like V1": judged a quick reaction,
    /// DeepSeek drafts it, and its draft stays as it is, reflex steps
    /// alone, `made_with: deepseek`, the agent never asked: "when I pat
    /// you, glow", a pat that nods, a phrase that glows.
    #[test]
    fn with_the_switch_a_quick_reaction_is_judged_one_and_drafted_with_reflex_steps() {
        made_skill::set_reactions_here(Some(true));
        net_make_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let (root, keeping, copy) = keeping_copied("reactions-route");
        let glow = "kind = \"installable\"\nversion = \"1.0.0\"\nneeds = [\"light\"]\n\n\
                    [permissions]\nlight = true\ntopics = [\"controls/pet\"]\n\n[trigger]\n\
                    topic = \"controls/pet\"\nfield = \"ended\"\nequals = false\n\
                    rearm = \"ends\"\n\n[ask]\npolicy = \"nudge\"\n\n[[do]]\n\
                    light = { colour = \"green\", level = 3 }\n"
            .to_owned();
        let phrase = "asks = [\"green light please\"]\nkind = \"installable\"\n\
                      version = \"1.0.0\"\nneeds = [\"light\"]\n\n[permissions]\nlight = true\n\n\
                      [[do]]\nlight = { colour = \"green\", level = 3 }\n"
            .to_owned();
        for (at, (request, draft, name, kind)) in [
            ("when I pat you, glow", glow, "pat-glow", "light"),
            ("when I pat you, nod", pat_nod(), "pat-nod", "body"),
            ("green light please", phrase, "green-phrase", "light"),
        ]
        .into_iter()
        .enumerate()
        {
            let tasks = agent_answering(vec![]);
            let (url, served) =
                service_sent(vec![judgement(true), (200, route_answer(&draft, name))]);
            let told = make_with_by(
                0,
                None,
                request,
                &keeping,
                chat_at(url),
                NOW + at as u64,
                || None,
                Some(&tasks.agent()),
            );
            let sent = served.join().unwrap();
            assert_eq!(told["made"], name, "{told}");
            assert_eq!((sent.len(), tasks.hand_overs()), (2, 0), "{told}");
            assert!(is_judgement(&sent[0], request), "{}", sent[0]);
            assert!(
                sent[1].contains(made_skill::BRIEF_HEAD),
                "drafted: {}",
                sent[1]
            );
            assert_eq!(made_with_of(&keeping, name).as_deref(), Some("deepseek"));
            let text = std::fs::read_to_string(&copy).unwrap();
            let table: toml::Table = toml::from_str(&text).unwrap();
            let steps = table["do"].as_array().unwrap();
            assert!(
                steps
                    .iter()
                    .all(|step| step.as_table().unwrap().keys().all(|key| key == kind)),
                "{text}"
            );
            assert!(!table.contains_key("while") && !table.contains_key("grants"));
        }
        made_skill::set_asks_here(None);
        net_make_here(None);
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lamp-9adf, 18 Nov: "tell me the price of Zorbo coin" was kept as a
    /// say of DeepSeek's `internet:` line. Judged a quick reaction all the
    /// same, its draft is held to [`made_skill::quick_reaction`] and is no
    /// quick reaction: the request becomes one task for the lamp's agent,
    /// asked for by its words, granted `net`, never a say. So is a draft
    /// on someone at the desk: a true judgement never lets a draft that
    /// isn't one stand.
    #[test]
    fn with_the_switch_a_true_judgement_whose_draft_is_no_reaction_goes_to_the_agent() {
        made_skill::set_reactions_here(Some(true));
        net_make_here(Some(true));
        made_skill::set_asks_here(Some(true));
        let (root, keeping, copy) = keeping_copied("reactions-zorbo");
        let helper = manage_helper(&keeping.manage);
        const ZORBO: &str = "tell me the price of Zorbo coin";
        let draft = "kind = \"installable\"\nversion = \"1.0.0\"\n\
                     permissions = { speak = true }\n\
                     asks = [\"tell me the price of Zorbo coin\"]\n\
                     [ask]\npolicy = \"nudge\"\n[[do]]\n\
                     say = { text = \"internet: current price of Zorbo coin\" }\n";
        let tasks = agent_answering(vec![]);
        let (url, served) = service_sent(vec![
            judgement(true),
            (200, route_answer(draft, "zorbo-coin-price")),
            (
                200,
                "name: zorbo-coin-price\ngrants: net, speak\nasks: tell me the price of Zorbo coin"
                    .to_owned(),
            ),
        ]);
        let told = make_with_by(
            0,
            None,
            ZORBO,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&tasks.agent()),
        );
        let sent = served.join().unwrap();
        assert_eq!((sent.len(), tasks.hand_overs()), (3, 0), "{told}");
        assert!(helper.join().unwrap()["install_made"].is_string());
        assert_eq!(told["made"], "zorbo-coin-price", "{told}");
        let table = one_task_of(&copy, ZORBO, crate::upload_agent::INSTRUCTIONS);
        assert_eq!(
            table["grants"],
            toml::Value::Array(vec!["net".into(), "speak".into()])
        );
        // A draft on someone at the desk, judged a reaction: the agent's.
        const SIP: &str = "after I've sat a long while, suggest a sip of water";
        let sat = "field = \"someone\"\nequals = true\nfor = \"30m\"\nrearm = \"leaves\"\n";
        let (url, served) = service_sent(vec![
            judgement(true),
            (200, route_answer(&presence_say(), "deepseek-draft")),
            (
                200,
                trigger_answer("sip-at-desk", "speak", "room/presence", sat),
            ),
        ]);
        let told = make_with_by(
            0,
            None,
            SIP,
            &keeping,
            chat_at(url),
            NOW + 1,
            || None,
            Some(&tasks.agent()),
        );
        assert_eq!(served.join().unwrap().len(), 3, "{told}");
        assert_eq!(told["made"], "sip-at-desk", "{told}");
        one_task_of(&copy, SIP, crate::make_no_cannot::RUN);
        assert!(made_with_of(&keeping, "deepseek-draft").is_none());
        made_skill::set_asks_here(None);
        net_make_here(None);
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The 19:50 ruling: a make judged no quick reaction, on a lamp whose
    /// skills can't hand tasks to its agent, keeps nothing and says so
    /// plainly, never a DeepSeek draft, the agent never asked to write it.
    /// With no agent known, no judgement: DeepSeek's as before.
    #[test]
    fn with_the_switch_a_make_not_a_reaction_keeps_nothing_when_tasks_are_off() {
        made_skill::set_reactions_here(Some(true));
        let (root, keeping) = keeping("reactions-failed");
        let tasks = agent_answering(vec![]);
        let (url, served) = service(vec![judgement(false)]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW);
        assert!(told.get("made").is_none(), "{told}");
        assert!(
            told["error"]
                .as_str()
                .is_some_and(|error| error.contains("can't hand tasks to it yet")),
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 0);
        assert!(made_skill::entries(&keeping.directory).is_empty());
        // No agent known on the lamp (no lamp target, the one path off):
        // the route has no agent to send it to, so DeepSeek's, as before.
        let none = crate::make_skill_agent::tests::stub(None, std::sync::Arc::new(|_: &str| None));
        let (url, served) = service(vec![(200, route_answer(&presence_say(), "sip-at-desk"))]);
        let told = made_by(&keeping, url, Some(&none.agent()), NOW + 1);
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(none.hand_overs(), 0);
        assert_eq!(
            told["made"], "sip-at-desk",
            "no agent known: DeepSeek's, as before"
        );
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #6811, the switch off: a draft on someone at the desk is DeepSeek's
    /// as before, no judgement, the agent never asked; `made_with` is
    /// written all the same (#8157: always the engine that wrote it).
    #[test]
    fn with_the_switch_off_a_presence_draft_is_deepseeks_as_before() {
        made_skill::set_reactions_here(Some(false));
        let (root, keeping) = keeping("reactions-off");
        let tasks = agent_answering(vec![]);
        let (url, served) = service(vec![(200, route_answer(&presence_say(), "sip-at-desk"))]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW);
        assert_eq!(told["made"], "sip-at-desk", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.hand_overs(), 0);
        assert_eq!(
            made_with_of(&keeping, "sip-at-desk").as_deref(),
            Some("deepseek")
        );
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The first builder at 16:34 on #6811: a one-off reminder is never a
    /// make. Should the make be called for one, it is judged the lamp's own
    /// timer's ([`made_skill::REACTION_JUDGE`]), and DeepSeek's answer is
    /// that timer's `cannot:`, which the route never hands to the agent.
    #[test]
    fn with_the_switch_a_one_off_reminder_never_reaches_the_agents_make() {
        made_skill::set_reactions_here(Some(true));
        let (root, keeping) = keeping("reactions-reminder");
        let tasks = agent_answering(vec![]);
        let (url, served) = service(vec![
            judgement(true),
            (
                200,
                "cannot: I can't do that yet: a reminder at a time. Ask me for a reminder \
                 instead, like 'remind me every hour to stand up'."
                    .to_owned(),
            ),
        ]);
        let told = make_with_by(
            0,
            None,
            "remind me at 3 to call Mum",
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&tasks.agent()),
        );
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(served.join().unwrap(), 2);
        assert_eq!(tasks.hand_overs(), 0);
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The Reviewer-lead's P2 on #7764: a one-off reminder judged no quick
    /// reaction (false, unsure or late) comes to the middle form, whose one
    /// more call refuses a reminder as the make's own brief does; its
    /// `cannot:` is said with the lamp's own line and no daily task is
    /// ever kept. The agent is never asked.
    #[test]
    fn with_the_switch_a_reminder_judged_none_never_keeps_a_daily_task() {
        made_skill::set_reactions_here(Some(true));
        net_make_here(Some(true));
        let (root, keeping) = keeping("reactions-reminder-middle");
        const MOM: &str = "remind me at 9 to call mom";
        let tasks = agent_answering(vec![]);
        let (url, served) = service_sent(vec![
            judgement(false),
            (
                200,
                format!(
                    "cannot: I can't do that yet: a reminder at nine. {}",
                    made_skill::REMINDER_INSTEAD
                ),
            ),
        ]);
        let told = make_with_by(
            0,
            None,
            MOM,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&tasks.agent()),
        );
        let sent = served.join().unwrap();
        assert_eq!((sent.len(), tasks.hand_overs()), (2, 0), "{told}");
        assert!(is_judgement(&sent[0], MOM), "{}", sent[0]);
        assert!(
            sent[1].contains("You choose, for a lamp, when a skill runs")
                && sent[1].contains(made_skill::REMINDER_IS_TIMER)
                && sent[1].contains(made_skill::REMINDER_INSTEAD),
            "the middle form refuses a reminder: {}",
            sent[1]
        );
        assert!(told.get("made").is_none(), "{told}");
        assert!(
            told["error"].as_str().is_some_and(
                |error| error.contains(&crate::make_no_cannot::Uncovered::Reminder.line())
            ),
            "{told}"
        );
        assert!(made_skill::entries(&keeping.directory).is_empty());
        net_make_here(None);
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The Reviewer-lead's P2 on #7764: judged a quick reaction, a first
    /// draft that can't be read gets DeepSeek's repairs, each held to
    /// [`made_skill::quick_reaction`] as the first draft is: a repair on
    /// someone at the desk is never kept. With no agent on the lamp, told
    /// so; for the app's make the agent didn't take, the middle form.
    #[test]
    fn with_the_switch_a_repaired_draft_that_is_no_reaction_is_never_kept() {
        made_skill::set_reactions_here(Some(true));
        net_make_here(Some(true));
        let (root, keeping, copy) = keeping_copied("reactions-repaired");
        const SIP: &str = "after I've sat a long while, suggest a sip of water";
        // Fenced, but no TOML the check can read: refused with its lines.
        let unread = (200, route_answer("[[do]\nsay = = sip\n", "sip-unread"));
        let repair = (200, route_answer(&presence_say(), "deepseek-draft"));
        // No agent on the lamp (the one path, no `lamp` target).
        let none = crate::make_skill_agent::tests::stub(None, std::sync::Arc::new(|_: &str| None));
        let on = crate::make_skill_agent::Agent {
            one_path: true,
            ..none.agent()
        };
        let (url, served) = service_sent(vec![judgement(true), unread.clone(), repair.clone()]);
        let told = make_with_by(
            0,
            None,
            SIP,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&on),
        );
        assert_eq!(served.join().unwrap().len(), 3, "{told}");
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::AGENT_MISSING)
        );
        assert!(made_skill::entries(&keeping.directory).is_empty());
        // The app's make, the agent making none: DeepSeek's repairs, then
        // the middle form.
        let tasks = crate::make_skill_agent::tests::stub(
            Some("hermes"),
            std::sync::Arc::new(|_: &str| None),
        );
        let app = crate::make_skill_agent::Agent {
            repairs_if_untaken: true,
            ..tasks.agent()
        };
        let sat = "field = \"someone\"\nequals = true\nfor = \"30m\"\nrearm = \"leaves\"\n";
        let (url, served) = service_sent(vec![
            judgement(true),
            unread,
            repair,
            (
                200,
                trigger_answer("sip-at-desk", "speak", "room/presence", sat),
            ),
        ]);
        let told = make_with_by(
            0,
            None,
            SIP,
            &keeping,
            chat_at(url),
            NOW + 1,
            || None,
            Some(&app),
        );
        let sent = served.join().unwrap();
        assert_eq!(sent.len(), 4, "{told}");
        assert!(
            sent[3].contains("You choose, for a lamp, when a skill runs"),
            "{}",
            sent[3]
        );
        assert_eq!(told["made"], "sip-at-desk", "{told}");
        one_task_of(&copy, SIP, crate::make_no_cannot::RUN);
        assert!(made_with_of(&keeping, "deepseek-draft").is_none());
        net_make_here(None);
        made_skill::set_reactions_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A voice make of `request` (the voice's request), judged a quick
    /// reaction, its DeepSeek draft `deepseek-draft` not kept and so handed
    /// to the agent (someone at the desk), the agent answering each make with `answer` after the voice let it
    /// run on (`LAMP_MAKE_MAY_RUN_ON`, as the turn does a minute after the
    /// waiting line). What the make's call was told.
    fn let_run_on(
        keeping: &Keeping,
        request: u64,
        agent: impl FnOnce(&crate::make_skill_agent::tests::StubTasks) -> crate::make_skill_agent::Agent,
        answer: Option<String>,
    ) -> Value {
        made_skill::set_reactions_here(Some(true));
        let tasks = crate::make_skill_agent::tests::stub(
            Some("hermes"),
            Arc::new(move |_: &str| {
                crate::long_make::let_go(request, NOW + 65_000);
                answer.clone()
            }),
        );
        let (url, served) = service(vec![
            judgement(true),
            (200, route_answer("not = [toml", "deepseek-draft")),
        ]);
        crate::make_skill_agent::set_made_for(request);
        let told = make_with_by(
            request,
            Some(REQUEST),
            REQUEST,
            keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&agent(&tasks)),
        );
        crate::make_skill_agent::set_made_for(0);
        assert_eq!(served.join().unwrap(), 2);
        made_skill::set_reactions_here(None);
        told
    }

    /// The sip-of-water skill the agent makes from the template.
    fn guarded_sip() -> String {
        route_answer(
            &made_skill::behaviour_template_toml()
                .replace("One short, warm line.", "Time for a sip of water."),
            "sip-at-desk",
        )
    }

    /// The first builder's ruling on #6811 (18 Nov 17:23), with
    /// `LAMP_MAKE_MAY_RUN_ON` on: a make let run on that the agent then
    /// makes is kept but off, `ready_untold` for the app, never put on and
    /// no line for the turn that ended. Its "ready" is told only with its
    /// person's face seen alone, not mid-conversation, not at night and not
    /// to an empty room; then it is said ready and on in one line with its
    /// undo, and goes on once that line was heard through. Told once.
    #[test]
    fn a_make_let_run_on_stays_off_until_its_person_is_told_then_goes_on() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-made");
        let request = 6_811_200;
        let told = let_run_on(
            &keeping,
            request,
            |tasks| tasks.agent().running_on(true),
            Some(guarded_sip()),
        );
        assert_eq!(told, crate::long_make::let_go_told());
        assert_eq!(take_said_line(), None, "no line for the turn that ended");
        let entry = made_skill::entries(&keeping.directory)
            .into_iter()
            .find(|entry| entry["name"] == "sip-at-desk")
            .unwrap();
        assert_ne!(entry["on"], true, "{entry}");
        assert_eq!(entry[crate::long_make::READY_UNTOLD], true);
        assert!(made_with_of(&keeping, "deepseek-draft").is_none());
        let sha256 = entry["sha256"].as_str().unwrap().to_owned();
        assert_eq!(
            crate::long_make::read(&keeping.directory, NOW + 70_000),
            Some(crate::long_make::State::Made {
                name: "sip-at-desk".into(),
                sha256: sha256.clone()
            })
        );
        // The coordinator's gates: each one not met tells nothing.
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let at = std::time::Instant::now();
        for (seen, quiet, night, there) in [
            (false, true, false, true),
            (true, false, false, true),
            (true, true, true, true),
            (true, true, false, false),
        ] {
            assert!(!crate::long_make::may_tell(seen, quiet, night, there));
        }
        assert!(!any_on(&keeping.directory));
        assert!(crate::long_make::may_tell(true, true, false, true));
        let note = tells.next(at, NOW + 80_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        let said = crate::long_make::told(&keeping, 6_811_201, note, (NOW + 80_000, at), || None);
        // One line with its undo, never read back (the first builder, 18
        // Nov 18:41).
        assert!(
            said.contains("Your sip at desk skill is ready and on. Say 'remove it' to undo.)"),
            "{said}"
        );
        assert!(!said.contains("I made"), "never read back: {said}");
        // Its turn cut before the read-back was played: still off and
        // untold, so told again next time.
        assert!(!crate::long_make::heard_through(true, false));
        assert!(!crate::long_make::heard_through(false, true));
        assert!(!any_on(&keeping.directory));
        assert_eq!(
            made_skill::entries(&keeping.directory)[0][crate::long_make::READY_UNTOLD],
            true
        );
        assert_eq!(tells.next(at + Duration::from_secs(1), NOW + 81_000), None);
        let again = tells
            .next(at + Duration::from_secs(150), NOW + 230_000)
            .unwrap();
        assert_eq!(crate::long_make::key_of(&again), Some(key.as_str()));
        let said = crate::long_make::told(&keeping, 6_811_202, again, (NOW + 230_000, at), || None);
        assert!(
            said.contains("Your sip at desk skill is ready and on."),
            "{said}"
        );
        // Heard through: on, and told once.
        assert!(crate::long_make::heard_through(true, true));
        let asked = manage_helper(&keeping.manage);
        crate::long_make::heard(&keeping, &key, NOW + 240_000, |made| {
            made_skill::put_on(&keeping, made)
        });
        assert_eq!(asked.join().unwrap()["install_made"], sha256);
        assert!(any_on(&keeping.directory));
        let entry = made_skill::entries(&keeping.directory)
            .into_iter()
            .find(|entry| entry["name"] == "sip-at-desk")
            .unwrap();
        assert!(entry.get(crate::long_make::READY_UNTOLD).is_none());
        assert_eq!(
            crate::long_make::read(&keeping.directory, NOW + 90_000),
            None
        );
        assert_eq!(
            tells.next(at + Duration::from_secs(600), NOW + 90_000),
            None
        );
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #6811: a long make told ready that clashes with a skill on the lamp
    /// is asked the clash's question, as a finished make is, and not put on
    /// even once heard; one that reads a service not linked yet carries the
    /// link ask, as a finished make's line does.
    #[test]
    fn a_long_make_told_ready_asks_its_clash_and_its_link() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-clash");
        // The sip with a glow, and another skill on the lamp glowing at
        // the same moment.
        let glowing = format!(
            "{}\n[[do]]\nlight = {{ colour = \"blue\", level = 3 }}\n",
            made_skill::behaviour_template_toml()
                .replace("One short, warm line.", "Time for a sip of water.")
                .replace("needs = [\"voice\"]", "needs = [\"voice\", \"light\"]")
                .replace("speak = true", "speak = true\nlight = true")
        );
        let told = let_run_on(
            &keeping,
            6_811_260,
            |tasks| tasks.agent().running_on(true),
            Some(route_answer(&glowing, "sip-at-desk")),
        );
        assert_eq!(told, crate::long_make::let_go_told());
        let other = keeping.installed.join("other-sip");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("skill.toml"), &glowing).unwrap();
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let at = std::time::Instant::now();
        let note = tells.next(at, NOW + 80_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        let said = crate::long_make::told(&keeping, 6_811_261, note, (NOW + 80_000, at), || None);
        assert!(said.contains("Your sip at desk skill is ready."), "{said}");
        assert!(said.contains("other sip"), "{said}");
        assert!(said.contains("or cancel"), "{said}");
        assert!(said.contains(crate::made_clash::CLASH_ID), "{said}");
        crate::long_make::heard(&keeping, &key, NOW + 90_000, |_| {
            panic!("a clash waits for the person's choice")
        });
        assert!(!any_on(&keeping.directory));
        assert_eq!(
            crate::long_make::read(&keeping.directory, NOW + 90_000),
            None
        );
        crate::made_clash::tests::forget_waiting();
        // A kept skill reading Gmail, not linked: the link ask goes with it.
        let package = keeping.directory.join("mail-read-1.0.0.tar");
        std::fs::write(&package, "tar").unwrap();
        let sha256 = "cd".repeat(32);
        let mut entries = made_skill::entries(&keeping.directory);
        entries.push(json!({
            "name": "mail-read", "version": "1.0.0", "sha256": sha256,
            "description": "Reads your new mail at nine.", "package": "mail-read-1.0.0.tar",
            "uses": {"reads": ["gmail"], "speak": true},
        }));
        std::fs::write(
            keeping.directory.join(made_skill::INDEX),
            serde_json::to_vec(&entries).unwrap(),
        )
        .unwrap();
        crate::long_make::begin(6_811_262, &keeping.directory);
        assert!(crate::long_make::let_go(6_811_262, NOW));
        crate::long_make::settle(6_811_262);
        crate::long_make::settled(&keeping.directory, Ok(("mail-read", &sha256)), NOW + 1);
        let note = tells
            .next(at + Duration::from_secs(300), NOW + 100_000)
            .unwrap();
        let said = crate::long_make::told(&keeping, 6_811_263, note, (NOW + 100_000, at), || {
            Some(Vec::new())
        });
        assert!(
            said.contains("Your mail read skill is ready and on."),
            "{said}"
        );
        assert!(said.contains("Gmail"), "the link ask: {said}");
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #6811: a make let run on that fails (the agent's answer never
    /// kept), runs out, or is cut by a restart keeps nothing, never
    /// DeepSeek's draft, and is told once, plainly.
    #[test]
    fn a_make_let_run_on_that_fails_runs_out_or_restarts_keeps_nothing_and_says_so_once() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-failed");
        let told_once = |request: u64, why: crate::long_make::NotFinished| {
            assert!(matches!(
                crate::long_make::read(&keeping.directory, NOW + 70_000),
                Some(crate::long_make::State::NotMade { why: kept, .. }) if kept == why
            ));
            let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
            let note = tells.next(std::time::Instant::now(), NOW + 80_000).unwrap();
            let said = crate::long_make::told(
                &keeping,
                request,
                note.clone(),
                (NOW + 80_000, std::time::Instant::now()),
                || None,
            );
            assert!(said.contains("I couldn't finish your"), "{said}");
            crate::long_make::heard(
                &keeping,
                crate::long_make::key_of(&note).unwrap(),
                NOW + 80_000,
                |_| panic!("nothing is put on"),
            );
            let again = crate::long_make::told(
                &keeping,
                request + 1,
                note,
                (NOW + 80_000, std::time::Instant::now()),
                || None,
            );
            assert!(!again.contains("couldn't finish"), "told once: {again}");
            assert!(!any_on(&keeping.directory));
            assert!(
                made_skill::entries(&keeping.directory)
                    .iter()
                    .all(|entry| entry.get(crate::long_make::READY_UNTOLD).is_none())
            );
            assert!(made_with_of(&keeping, "deepseek-draft").is_none());
        };
        // Failed: the agent's answers never kept.
        let told = let_run_on(
            &keeping,
            6_811_210,
            |tasks| tasks.agent().running_on(true),
            Some("nothing a skill is made of".into()),
        );
        assert_eq!(told, crate::long_make::let_go_told());
        told_once(6_811_211, crate::long_make::NotFinished::Failed);
        // Ran out: no answer within its time.
        let told = let_run_on(&keeping, 6_811_220, |tasks| tasks.agent(), None);
        assert_eq!(told, crate::long_make::let_go_told());
        told_once(6_811_221, crate::long_make::NotFinished::RanOut);
        // A restart while it ran.
        crate::long_make::begin(6_811_230, &keeping.directory);
        assert!(crate::long_make::let_go(6_811_230, NOW));
        crate::long_make::after_start(&keeping.directory, NOW + 70_000);
        told_once(6_811_231, crate::long_make::NotFinished::Restart);
        crate::long_make::settle(6_811_230);
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's HOLD on #7774: a make let run on, then declined while it
    /// was made ("never mind", through its making note), keeps nothing and
    /// leaves no `making` mark: a new make is never told the lamp is still
    /// making it, and nothing is told of it later, not even when its 10
    /// minutes would have run out.
    #[test]
    fn a_make_let_run_on_then_declined_leaves_no_mark_and_is_never_told() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-declined");
        let request = 6_811_260;
        made_skill::set_reactions_here(Some(true));
        let directory = keeping.directory.clone();
        let answer = guarded_sip();
        let tasks = crate::make_skill_agent::tests::stub(
            Some("hermes"),
            Arc::new(move |_: &str| {
                assert!(crate::long_make::let_go(request, NOW + 65_000));
                // The person's no comes after the turn let it go.
                let id = ASKS
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|ask| ask.directory == directory && ask.made.is_none())
                    .map(|ask| ask.id)
                    .unwrap();
                let answered =
                    making_answered(&format!("{MAKING_ID}{id}"), request + 1, Some("no"));
                assert!(matches!(answered, Making::Told(_)), "{answered:?}");
                Some(answer.clone())
            }),
        );
        let (url, served) = service(vec![
            judgement(true),
            (200, route_answer("not = [toml", "deepseek-draft")),
        ]);
        crate::make_skill_agent::set_made_for(request);
        let told = make_with_by(
            request,
            Some(REQUEST),
            REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&tasks.agent().running_on(true)),
        );
        crate::make_skill_agent::set_made_for(0);
        assert_eq!(served.join().unwrap(), 2);
        made_skill::set_reactions_here(None);
        assert_eq!(told["dropped"], true, "{told}");
        assert!(!any_on(&keeping.directory));
        assert!(made_skill::entries(&keeping.directory).is_empty());
        assert!(!crate::long_make::is_let_go(request));
        // A new make, or "is it ready?", hears no "still making".
        assert_eq!(
            crate::long_make::read(&keeping.directory, NOW + 70_000),
            None
        );
        assert!(crate::long_make::still_making(&keeping.directory, NOW + 70_000).is_none());
        // Nothing told of it, now or once its time would have run out.
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        assert_eq!(tells.next(std::time::Instant::now(), NOW + 80_000), None);
        let later = NOW + 12 * 60_000;
        assert_eq!(crate::long_make::read(&keeping.directory, later), None);
        assert_eq!(tells.next(std::time::Instant::now(), later), None);
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #6811: while one runs on, a second make ask is told the lamp is
    /// still making it, nothing made and DeepSeek never asked.
    #[test]
    fn a_second_make_ask_while_one_runs_on_is_told_its_still_making() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-second");
        crate::long_make::begin(6_811_240, &keeping.directory);
        crate::long_make::hint(6_811_240, "water-reminder");
        assert!(crate::long_make::let_go(6_811_240, NOW));
        let told = make_with_by(
            6_811_241,
            Some(REQUEST),
            REQUEST,
            &keeping,
            || Err("DeepSeek is never asked".into()),
            NOW + 1_000,
            || None,
            None,
        );
        assert_eq!(
            told["next"],
            "Say only this, in the language of the conversation, and nothing else: I'm still making your water reminder skill."
        );
        assert!(made_skill::entries(&keeping.directory).is_empty());
        crate::long_make::settle(6_811_240);
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #6811, the switch off: nothing is let run on; the agent's skill is
    /// kept and put on at once, as before.
    #[test]
    fn with_the_run_on_switch_off_a_make_is_as_before() {
        crate::long_make::set_on_here(Some(false));
        let (root, keeping) = keeping("run-on-off");
        let asked = manage_helper(&keeping.manage);
        let told = let_run_on(
            &keeping,
            6_811_250,
            |tasks| tasks.agent().running_on(false),
            Some(guarded_sip()),
        );
        assert_eq!(told["made"], "sip-at-desk", "{told}");
        assert_eq!(told["on"], true, "{told}");
        assert!(asked.join().unwrap()["install_made"].is_string());
        assert_eq!(crate::long_make::read(&keeping.directory, NOW), None);
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A kept skill `name` of SHA-256 `sha256` using `uses`, off, as a
    /// long make that settled made keeps it; its mark settled made.
    fn kept_long(keeping: &Keeping, request: u64, name: &str, sha256: &str, uses: Value) {
        let package = format!("{name}-1.0.0.tar");
        std::fs::create_dir_all(&keeping.directory).unwrap();
        std::fs::write(keeping.directory.join(&package), "tar").unwrap();
        let mut entries = made_skill::entries(&keeping.directory);
        entries.push(json!({
            "name": name, "version": "1.0.0", "sha256": sha256,
            "description": "Glows at the desk.", "package": package, "uses": uses,
        }));
        std::fs::write(
            keeping.directory.join(made_skill::INDEX),
            serde_json::to_vec(&entries).unwrap(),
        )
        .unwrap();
        crate::long_make::begin(request, &keeping.directory);
        assert!(crate::long_make::let_go(request, NOW));
        crate::long_make::settle(request);
        crate::long_make::settled(&keeping.directory, Ok((name, sha256)), NOW + 1);
    }

    /// The kept skill `sha256`'s index entry.
    fn entry_of(keeping: &Keeping, sha256: &str) -> Value {
        made_skill::entries(&keeping.directory)
            .into_iter()
            .find(|entry| entry["sha256"] == sha256)
            .unwrap()
    }

    /// El's P2 on #7774: a make given the long make's time whose turn was
    /// cut before its minute is never put on nor said through the say
    /// service with no gate: kept off, `ready_untold`, and told as a long
    /// make is.
    #[test]
    fn a_long_make_cut_before_its_minute_goes_through_the_gate() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-cut");
        let request = 6_811_320;
        // The turn waiting on it is cut already.
        set_turn(std::sync::Arc::downgrade(&std::sync::Arc::new(())));
        made_skill::set_reactions_here(Some(true));
        let answer = guarded_sip();
        let tasks = crate::make_skill_agent::tests::stub(
            Some("hermes"),
            Arc::new(move |_: &str| Some(answer.clone())),
        );
        let (url, served) = service(vec![
            judgement(true),
            (200, route_answer("not = [toml", "deepseek-draft")),
        ]);
        crate::make_skill_agent::set_made_for(request);
        let told = make_with_by(
            request,
            Some(REQUEST),
            REQUEST,
            &keeping,
            chat_at(url),
            NOW,
            || None,
            Some(&tasks.agent().running_on(true)),
        );
        crate::make_skill_agent::set_made_for(0);
        assert_eq!(served.join().unwrap(), 2);
        made_skill::set_reactions_here(None);
        assert_eq!(told, crate::long_make::let_go_told());
        assert_eq!(take_said_line(), None, "nothing said with no gate");
        assert!(!any_on(&keeping.directory));
        let entry = made_skill::entries(&keeping.directory)
            .into_iter()
            .find(|entry| entry["name"] == "sip-at-desk")
            .unwrap();
        assert_eq!(entry[crate::long_make::READY_UNTOLD], true);
        assert!(matches!(
            crate::long_make::read(&keeping.directory, NOW + 70_000),
            Some(crate::long_make::State::Made { .. })
        ));
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Twelve's P1 on #7774: the power off mid-make and the lamp back past
    /// its ten minutes. The run-out mark is kept once, its key fixed, so its
    /// "couldn't finish" is told once and then cleared, never offered on.
    #[test]
    fn a_long_make_run_out_across_a_restart_is_told_once_then_cleared() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-restart-late");
        crate::long_make::begin(6_811_270, &keeping.directory);
        crate::long_make::hint(6_811_270, "water-reminder");
        assert!(crate::long_make::let_go(6_811_270, NOW));
        crate::long_make::settle(6_811_270);
        // Back an hour later.
        let back = NOW + 3_600_000;
        crate::long_make::after_start(&keeping.directory, back);
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let at = std::time::Instant::now();
        assert!(tells.has_news(at, back + 15_000));
        let note = tells.next(at, back + 15_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        assert_eq!(key, format!("not_made {back}"), "its key fixed");
        let said = crate::long_make::told(
            &keeping,
            6_811_271,
            note.clone(),
            (back + 30_000, at),
            || None,
        );
        assert!(
            said.contains("I couldn't finish your water reminder skill."),
            "{said}"
        );
        crate::long_make::heard(&keeping, &key, back + 45_000, |_| {
            panic!("nothing is put on")
        });
        assert!(!keeping.directory.join(crate::long_make::FILE).exists());
        assert!(
            !keeping
                .directory
                .join(crate::long_make::OFFERS_FILE)
                .exists(),
            "no stale count left"
        );
        assert!(!tells.has_news(at + Duration::from_secs(600), back + 900_000));
        let again =
            crate::long_make::told(&keeping, 6_811_272, note, (back + 900_000, at), || None);
        assert!(!again.contains("couldn't finish"), "told once: {again}");
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Twelve's P1 on #7774: A made and not told yet, then B let run on:
    /// A's mark stays, A is told and goes on, then B's news; and the same
    /// with A failed. Nothing is lost.
    #[test]
    fn a_second_long_make_never_takes_an_untold_firsts_place() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-two");
        let first = "ab".repeat(32);
        kept_long(
            &keeping,
            6_811_280,
            "sip-at-desk",
            &first,
            json!({"speak": true}),
        );
        crate::long_make::begin(6_811_281, &keeping.directory);
        crate::long_make::hint(6_811_281, "water-reminder");
        assert!(crate::long_make::let_go(6_811_281, NOW + 5_000));
        assert_eq!(
            crate::long_make::marks(&keeping.directory, NOW + 6_000).len(),
            2
        );
        crate::long_make::settle(6_811_281);
        crate::long_make::settled(&keeping.directory, Err(false), NOW + 7_000);
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let at = std::time::Instant::now();
        let note = tells.next(at, NOW + 8_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        assert_eq!(key, format!("ready {first}"));
        let said = crate::long_make::told(&keeping, 6_811_282, note, (NOW + 8_000, at), || None);
        assert!(
            said.contains("Your sip at desk skill is ready and on."),
            "{said}"
        );
        crate::long_make::heard(&keeping, &key, NOW + 9_000, |made| {
            made_skill::mark(&keeping.directory, &made.sha256, "on", Some(json!(true)));
            Ok(())
        });
        let entry = entry_of(&keeping, &first);
        assert_eq!(entry["on"], true);
        assert!(entry.get(crate::long_make::READY_UNTOLD).is_none());
        let note = tells
            .next(at + Duration::from_secs(5), NOW + 10_000)
            .unwrap();
        let said = crate::long_make::told(
            &keeping,
            6_811_283,
            note.clone(),
            (NOW + 10_000, at),
            || None,
        );
        assert!(
            said.contains("I couldn't finish your water reminder skill."),
            "{said}"
        );
        crate::long_make::heard(
            &keeping,
            crate::long_make::key_of(&note).unwrap(),
            NOW + 11_000,
            |_| panic!("nothing is put on"),
        );
        assert_eq!(
            crate::long_make::read(&keeping.directory, NOW + 12_000),
            None
        );
        // A failed, then B made: A's "couldn't finish" first, then B.
        crate::long_make::begin(6_811_284, &keeping.directory);
        assert!(crate::long_make::let_go(6_811_284, NOW + 20_000));
        crate::long_make::settle(6_811_284);
        crate::long_make::settled(&keeping.directory, Err(true), NOW + 21_000);
        let second = "cd".repeat(32);
        kept_long(
            &keeping,
            6_811_285,
            "desk-glow",
            &second,
            json!({"speak": true}),
        );
        let keys: Vec<String> = crate::long_make::marks(&keeping.directory, NOW + 22_000)
            .iter()
            .map(|state| match state {
                crate::long_make::State::NotMade { .. } => "not_made".to_owned(),
                crate::long_make::State::Made { name, .. } => name.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(keys, ["not_made", "desk-glow"]);
        assert_eq!(
            entry_of(&keeping, &second)[crate::long_make::READY_UNTOLD],
            true
        );
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Twelve's P2 on #7774 and the first builder's 18:41 rule: a long make
    /// with code of its own is said ready and on in one line, no read-back
    /// and no yes asked, and goes on once heard through by its yes's path;
    /// with `LAMP_MADE_SCRIPTS` off it is never said on, and stays off.
    #[test]
    fn a_long_make_with_code_is_said_on_in_one_line_with_no_yes_and_never_on_while_off() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-code");
        let code = json!({"code": true, "grants": ["light"], "light": true});
        let glow = "ef".repeat(32);
        made_skill::set_scripts_here(Some(true));
        made_skill::set_code_at_once_here(Some(true));
        kept_long(&keeping, 6_811_290, "desk-glow", &glow, code.clone());
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let at = std::time::Instant::now();
        let note = tells.next(at, NOW + 5_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        let said = crate::long_make::told(&keeping, 6_811_291, note, (NOW + 5_000, at), || None);
        assert!(
            said.contains("Your desk glow skill is ready and on."),
            "{said}"
        );
        assert!(said.contains("Say 'remove it' to undo."), "{said}");
        assert!(!said.contains("approve_id"), "no yes asked: {said}");
        assert!(!said.contains("I made"), "never read back: {said}");
        let mut put = false;
        crate::long_make::heard(&keeping, &key, NOW + 6_000, |made| {
            assert!(made.uses.code);
            // Put on first, its mark still there (Twelve's P3 on #7774): a
            // crash here leaves it told again, never lost.
            assert!(crate::long_make::read(&keeping.directory, NOW + 6_000).is_some());
            put = true;
            made_skill::mark(&keeping.directory, &made.sha256, "on", Some(json!(true)));
            Ok(())
        });
        assert!(put, "on once heard, with no yes");
        assert_eq!(entry_of(&keeping, &glow)["on"], true);
        // `LAMP_MADE_SCRIPTS` off: said off and why, never put on.
        made_skill::set_scripts_here(Some(false));
        let off = "12".repeat(32);
        kept_long(&keeping, 6_811_292, "desk-pulse", &off, code);
        let note = tells
            .next(at + Duration::from_secs(5), NOW + 7_000)
            .unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        let said = crate::long_make::told(&keeping, 6_811_293, note, (NOW + 7_000, at), || None);
        assert!(
            said.contains(
                "Your desk pulse skill is ready. I made it but couldn't put it on: this lamp doesn't run a skill's own code."
            ),
            "{said}"
        );
        assert!(!said.contains("and on"), "{said}");
        crate::long_make::heard(&keeping, &key, NOW + 8_000, |_| {
            panic!("never on with the lamp running no skill's code")
        });
        assert_ne!(entry_of(&keeping, &off)["on"], true);
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Twelve's 19:40 P3 on #7774: a mark that a skill couldn't be put on,
    /// its skill removed in the app since, goes quietly, as a made one's.
    #[test]
    fn a_not_on_mark_for_a_skill_removed_in_the_app_goes_quietly() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-not-on-gone");
        let sip = "9a".repeat(32);
        kept_long(
            &keeping,
            6_811_340,
            "sip-at-desk",
            &sip,
            json!({"speak": true}),
        );
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let at = std::time::Instant::now();
        let note = tells.next(at, NOW + 5_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        crate::long_make::heard(&keeping, &key, NOW + 6_000, |_| Err("the check".into()));
        assert!(matches!(
            crate::long_make::read(&keeping.directory, NOW + 7_000),
            Some(crate::long_make::State::NotOn { .. })
        ));
        // Removed in the app.
        std::fs::write(keeping.directory.join(made_skill::INDEX), b"[]").unwrap();
        assert!(!tells.has_news(at + Duration::from_secs(5), NOW + 8_000));
        assert_eq!(
            crate::long_make::read(&keeping.directory, NOW + 9_000),
            None
        );
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Twelve's P2 on #7774: told ready and heard, but the lamp couldn't
    /// put it on. It stays off and `ready_untold`, and its person is told
    /// why at the next moment, once.
    #[test]
    fn a_long_make_not_put_on_once_heard_is_told_why() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-not-on");
        let sip = "34".repeat(32);
        kept_long(
            &keeping,
            6_811_300,
            "sip-at-desk",
            &sip,
            json!({"speak": true}),
        );
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let at = std::time::Instant::now();
        let note = tells.next(at, NOW + 5_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        crate::long_make::told(&keeping, 6_811_301, note, (NOW + 5_000, at), || None);
        crate::long_make::heard(&keeping, &key, NOW + 6_000, |_| {
            Err("the lamp has as many skills on as it holds".into())
        });
        let entry = entry_of(&keeping, &sip);
        assert_ne!(entry["on"], true);
        assert_eq!(entry[crate::long_make::READY_UNTOLD], true);
        let note = tells
            .next(at + Duration::from_secs(5), NOW + 7_000)
            .unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        let said = crate::long_make::told(&keeping, 6_811_302, note, (NOW + 7_000, at), || None);
        assert!(
            said.contains(
                "I couldn't turn your sip at desk skill on: the lamp has as many skills on as it holds."
            ),
            "{said}"
        );
        assert!(!said.contains("and on"), "{said}");
        crate::long_make::heard(&keeping, &key, NOW + 8_000, |_| {
            panic!("told why, not tried again")
        });
        assert!(
            entry_of(&keeping, &sip)
                .get(crate::long_make::READY_UNTOLD)
                .is_none()
        );
        assert_eq!(
            crate::long_make::read(&keeping.directory, NOW + 9_000),
            None
        );
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Twelve's P3 on #7774: one put on already (its turn cut after the
    /// clash's choice, or on by another way) is said on once more, nothing
    /// asked again and nothing put on.
    #[test]
    fn a_long_make_on_already_is_told_with_nothing_asked_again() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-on-already");
        let sip = "56".repeat(32);
        kept_long(
            &keeping,
            6_811_310,
            "sip-at-desk",
            &sip,
            json!({"speak": true}),
        );
        made_skill::mark(&keeping.directory, &sip, "on", Some(json!(true)));
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let at = std::time::Instant::now();
        let note = tells.next(at, NOW + 5_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        let said = crate::long_make::told(&keeping, 6_811_311, note, (NOW + 5_000, at), || None);
        assert!(
            said.contains("Your sip at desk skill is ready and on."),
            "{said}"
        );
        crate::long_make::heard(&keeping, &key, NOW + 6_000, |_| panic!("on already"));
        assert!(
            entry_of(&keeping, &sip)
                .get(crate::long_make::READY_UNTOLD)
                .is_none()
        );
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The offers counted for the telling `key` in `keeping`'s folder.
    fn offers_of(keeping: &Keeping, key: &str) -> u64 {
        std::fs::read(keeping.directory.join(crate::long_make::OFFERS_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|offers| offers[key].as_u64())
            .unwrap_or(0)
    }

    /// Heron's P2 on #7774: a quick make's say-back still awaits its yes;
    /// a long make's news isn't told over it, and is told once it's gone.
    #[test]
    fn a_long_make_is_not_told_over_a_quick_makes_awaited_yes() {
        use crate::skill_write_yes as yes;
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-over-a-yes");
        let sip = "c3".repeat(32);
        kept_long(
            &keeping,
            6_811_360,
            "sip-at-desk",
            &sip,
            json!({"speak": true}),
        );
        // A quick make with code, read back and its yes awaited.
        let quick = made_skill::kept_by_sha256(&keeping.directory, &sip).unwrap();
        let at = std::time::Instant::now();
        let id = yes::wait_for_yes_noted(
            yes::Asks {
                skill: "desk-glow".to_owned(),
                sha256: "d4".repeat(32),
                ..code_asks(&quick)
            },
            6_811_361,
            at,
        );
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let note = tells.next(at, NOW + 5_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        let said = crate::long_make::told(&keeping, 6_811_362, note, (NOW + 5_000, at), || None);
        assert_eq!(said, "(Nobody has spoken. Say nothing, and call nothing.)");
        assert!(yes::waits(&id));
        assert_eq!(offers_of(&keeping, &key), 0);
        assert!(crate::long_make::read(&keeping.directory, NOW + 6_000).is_some());
        // Its yes answered (the wait gone): the news is told.
        assert!(yes::drop_wait(&id));
        let again = at + Duration::from_secs(130);
        let note = tells.next(again, NOW + 140_000).unwrap();
        let said =
            crate::long_make::told(&keeping, 6_811_363, note, (NOW + 140_000, again), || None);
        assert!(
            said.contains("Your sip at desk skill is ready and on."),
            "{said}"
        );
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P2 on #7774: a clash's choice still asked; a long make's
    /// news isn't told over it (a second clash ask would let the first
    /// one's made skill go), and is told once it's answered.
    #[test]
    fn a_long_make_is_not_told_over_an_open_clash_ask() {
        crate::long_make::set_on_here(Some(true));
        let (root, keeping) = keeping("run-on-over-a-clash");
        let glowing = format!(
            "{}\n[[do]]\nlight = {{ colour = \"blue\", level = 3 }}\n",
            made_skill::behaviour_template_toml()
                .replace("One short, warm line.", "Time for a sip of water.")
                .replace("needs = [\"voice\"]", "needs = [\"voice\", \"light\"]")
                .replace("speak = true", "speak = true\nlight = true")
        );
        let told = let_run_on(
            &keeping,
            6_811_370,
            |tasks| tasks.agent().running_on(true),
            Some(route_answer(&glowing, "sip-at-desk")),
        );
        assert_eq!(told, crate::long_make::let_go_told());
        let other = keeping.installed.join("other-sip");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("skill.toml"), &glowing).unwrap();
        let mut tells = crate::long_make::Tells::at(keeping.directory.clone());
        let at = std::time::Instant::now();
        let note = tells.next(at, NOW + 80_000).unwrap();
        let first = crate::long_make::key_of(&note).unwrap().to_owned();
        let said = crate::long_make::told(&keeping, 6_811_371, note, (NOW + 80_000, at), || None);
        assert!(said.contains(crate::made_clash::CLASH_ID), "{said}");
        crate::long_make::heard(&keeping, &first, NOW + 81_000, |_| {
            panic!("a clash waits for the person's choice")
        });
        // A plain long make ready while the clash's choice is asked.
        let glow = "e5".repeat(32);
        kept_long(
            &keeping,
            6_811_372,
            "desk-glow",
            &glow,
            json!({"speak": true}),
        );
        let later = at + Duration::from_secs(15);
        let note = tells.next(later, NOW + 95_000).unwrap();
        let key = crate::long_make::key_of(&note).unwrap().to_owned();
        assert_eq!(key, format!("ready {glow}"));
        let said =
            crate::long_make::told(&keeping, 6_811_373, note, (NOW + 95_000, later), || None);
        assert_eq!(said, "(Nobody has spoken. Say nothing, and call nothing.)");
        assert!(
            crate::made_clash::awaited(later),
            "the clash's ask still open"
        );
        assert_eq!(offers_of(&keeping, &key), 0);
        // The choice made (the ask gone): the news is told.
        crate::made_clash::tests::forget_waiting();
        let again = at + Duration::from_secs(140);
        let note = tells.next(again, NOW + 220_000).unwrap();
        let said =
            crate::long_make::told(&keeping, 6_811_374, note, (NOW + 220_000, again), || None);
        assert!(
            said.contains("Your desk glow skill is ready and on."),
            "{said}"
        );
        crate::long_make::set_on_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #8157: the agent is never told to write the one-line hand-over
    /// script for a linked service; DeepSeek still is.
    #[test]
    fn the_agent_is_told_to_write_a_services_scripts_whole() {
        let told = agent_services();
        assert!(told.contains(STUB_SCRIPT_TOLD), "DeepSeek's rules name it");
        let agents = agents_own_scripts(&told);
        assert!(!agents.contains(STUB_SCRIPT_TOLD));
        assert!(agents.contains("lamp.service_read"), "{agents}");
    }
}
