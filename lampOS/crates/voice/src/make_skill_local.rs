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
//! `LAMP_MAKE_SKILL_AGENT=on` (off, in no product unit) a voice make the
//! lamp's model didn't make goes to the lamp's own agent, OpenClaw or
//! Hermes, when the lamp has one ([`crate::make_skill_agent`]): DeepSeek
//! then gets one try and no repairs, and the agent gets them.
use crate::made_skill::{self, Keeping, Made, Refused};
use crate::second_provider::net::{self, Chat, ChatEvent, Message};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// The longest one answer is waited for, from the request to its end.
pub const WRITE_WITHIN: Duration = Duration::from_secs(30);
/// The most times a refused answer goes back to the model with what was
/// refused (the owner, 10 Nov: "re-run it yourself until it works"; one,
/// before: lamp-74ff's "come back to my desk" failed its one repair).
pub const MOST_REPAIRS: usize = 3;
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
    format!(
        "You write skills for a lamp. Answer exactly as the request says, with {blocks} \
         and nothing else. Use only the keys and values the request names, written as \
         in this example, which the lamp's check takes:\n\n{EXAMPLE}\n\nThe example shows \
         the form only: never answer with it. A reminder at a time or every so often is the \
         lamp's own timer, not a skill; a skill runs at a time only each day or on named \
         days of the week, by the clock trigger below. A skill made on this lamp may do whatever the lamp can do: make what \
         the request asks with the light, the head, a few exact words it says, its hum, or \
         {may_also}, on one of the triggers below. {more} {BLINK} To speak, a skill has speak = \
         true in [permissions], {cooldown}, and one [[do]] step say = {{ text = \
         \"Welcome back!\" }}: the exact words, one or two short sentences, no quotes, in the \
         language the person asked in, said as written. Its one sound is a soft hum: a [[do]] \
         step cue = \"hum\", paced as a say, in place of a say; it has no other sound and no \
         sound file. When it runs is one \
         [trigger] table (never [[trigger]]; coming to the desk is room/presence, never the \
         faces): {triggers} A body step is one gesture, one of: {gestures}. Set none of \
         most_a_day, [quiet], [budget] or [rest] unless the request asks {limits} A skill can't yet \
         hear what is said, know who someone is, {cant_send}: a \
         request that needs one, or anything else no step or trigger here or below does, is \
         answered with one line and no blocks: {cannot} then one short sentence said to the \
         person, beginning \"{cant_yet}\" and naming what in plain words (\"{cant_yet} \
         {cant_example}.\"), never a rule, ending with \
         what to ask instead when there is one; \
         for a reminder at a time or every so often: \"Ask me for a reminder instead, like 'remind \
         me every hour to stand up'.\" Never stand something \
         else in for it, and add no time of day the request didn't name. When a part of the \
         request can't be done with these (dimming slowly, over a minute), make the rest and \
         add one line after the blocks: {left_out} what was left out, in a few words.\n\n\
         {services}{code}"
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
    let grants: Vec<&str> = made_skill::GRANTS.iter().map(|(name, _)| *name).collect();
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
         says back what it uses and waits for the person's yes before it runs any code.",
        sdk = made_skill::SCRIPT_SDK,
        most = made_skill::MOST_SCRIPTS,
        each = made_skill::MOST_SCRIPT_BYTES / 1024,
        all = made_skill::MOST_SCRIPTS_BYTES / 1024,
        grants = grants.join(", "),
    )
}

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
         trigger the lamp's own (a time, someone coming or leaving, as above); and exactly one \
         [[do]] step, alone in its step: task = {{ task = \"...\", run = \"scripts/main.py\" }}, \
         the task's words, at most {most} characters, saying plainly for the lamp's agent what \
         to read and what to tell the person, or what to send, in the request's own words, and \
         to whom (an address the request says, or the person's own for me); and one \
         ```scripts/main.py block that only prints a line, as in the example. Never a read = \
         {{ ... }} or write = {{ ... }} step. What the agent finds comes back as the lamp's task \
         news. Any send is read back by the lamp and goes only on the person's spoken yes. A \
         request to delete or change anything already in a service is answered with one line \
         and no blocks: {cannot} {cant_yet} deleting or changing what is in your services. \
         Never tell them to link a service. A whole such skill, for \"{AGENT_SERVICES_ASKED}\":\
         \n\n{AGENT_SERVICES_EXAMPLE}\n",
        cant_yet = made_skill::CANT_YET,
        codes = made_skill::SERVICE_GRANT_CODES.join(", "),
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

/// The function the model is given with the switch: the lamp makes it
/// itself, at once, and puts it on as soon as it is kept.
pub fn tool() -> lamp_gemini::Tool {
    let writes = made_skill::writes_on();
    let mut description = "Make a new skill for the lamp: something the lamp then does by itself, \
             whatever the lamp can do (its light, its head, a few exact words it says, its hum, a read of a linked service), at a time each day, when someone comes or goes, a face comes \
             into view, someone gets up, pats its head, the air gets stuffy, or when it is resting (\"make me a skill that dims the light at 10 pm\"). Call it only when the \
             person asks for a skill to be made. In the same breath, before you call it, say in a \
             few words that you'll make it now. The skill is made, checked, kept and put on the \
             lamp at once: then say exactly the line you are told, nothing more."
        .to_owned();
    // A made skill read back (`LAMP_SKILL_WRITES`): what it sends, and
    // its wait for the person's agreement again.
    if writes {
        description.push_str(READ_BACK_TOLD);
    }
    // A skill's task read back (`LAMP_SKILL_TASKS`, #6305): what its
    // script uses, and its wait for the person's agreement.
    let tasks = crate::skill_task_yes::on();
    if tasks {
        description.push_str(TASKS_READ_BACK_TOLD);
    }
    // A made skill with code of its own (#6307): not on until the person
    // agrees to its say-back.
    let code = made_skill::scripts_on();
    if code {
        description.push_str(CODE_TOLD);
    }
    lamp_gemini::Tool {
        name: made_skill::MAKE.into(),
        description,
        parameters: Some(parameters_with(writes, tasks, code)),
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

/// What the `request` parameter's description adds with a made skill's
/// code on (#6749 B): an ask its own script answers, passed whole.
const CODE_REQUEST_TOLD: &str = " A made skill's own script may check what the lamp reads and \
     act on it (\"make a skill that checks the processor temperature and sets the light\"): \
     pass such an ask whole.";

/// What [`tool`]'s description adds with a made skill's writes on.
pub const READ_BACK_TOLD: &str = " Call it too, with read_back alone, when the person asks \
     what a skill they made sends or to hear one read back, saying nothing before it.";

/// What [`tool`]'s description adds with a skill's tasks on (#6305).
pub const TASKS_READ_BACK_TOLD: &str = " Call it with read_back alone too when the person \
     asks what a skill on the lamp uses, saying nothing before it.";

/// The function's parameters: the request, and with a made skill's writes
/// (#6066) or code (#6307) on, the yes to one that waits for it,
/// `approve_id`, given alone, and with writes a read-back.
#[cfg(test)]
fn parameters(writes: bool, code: bool) -> Value {
    parameters_with(writes, false, code)
}

/// [`parameters`], and with a skill's tasks on (#6305) the same
/// `approve_id` and `read_back` for a skill's task.
fn parameters_with(writes: bool, tasks: bool, code: bool) -> Value {
    let mut parameters = json!({
        "type": "object",
        "properties": {
            "request": {
                "type": "string",
                "description": "What this skill should do and when, in the person's own words for this ask, nothing else: never a time, step or trigger of an earlier skill or turn."
            }
        },
        "required": []
    });
    // The person's choice on a made skill that clashes with one on the
    // lamp (#6593, [`crate::made_clash`]): its id and the choice, alone.
    parameters["properties"][crate::made_clash::CLASH_ID] = json!({
        "type": "string",
        "description": "Only when a skill you just made clashes with one on the lamp and the person then chooses: its clash_id, given with choice alone, with no request."
    });
    parameters["properties"][crate::made_clash::CHOICE] = json!({
        "type": "string",
        "enum": crate::made_clash::CHOICES,
        "description": "With clash_id only: replace the other skill, keep both (they may take turns), or cancel, as the person chose in any words."
    });
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
            "description": "Only when a skill you just made or read back waits for the person's yes and they then agree, in any words: its approve_id, given alone with no request."
        });
        parameters["required"] = json!([]);
    }
    if writes || tasks {
        let asked = match (writes, tasks) {
            (true, false) => "what a skill they made sends, or to hear it read back",
            (false, _) => "what a skill on the lamp uses",
            (true, true) => {
                "what a skill they made sends, what a skill on the lamp uses, or to hear one read back"
            }
        };
        parameters["properties"][READ_BACK] = json!({
            "type": "string",
            "description": format!("Only when the person asks {asked}: its name, given alone with no request.")
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
        net::Error::Url => false,
    }
}

/// The model's answer to `brief`, its text whole, asked once more when
/// [`again`] says so; each answer that ended whole, the second too, with
/// a last fence left open at its end closed there ([`whole`]).
pub async fn written(chat: &Chat, brief: &str) -> net::Result<String> {
    match write_once(chat, brief).await.and_then(whole) {
        Err(error) if again(&error) => {
            eprintln!(
                "lamp-voice: a skill made here: the model's answer failed ({error}); asked once more"
            );
            // The second answer is not asked again: one with no fenced
            // block goes on to be refused as not a skill's form.
            match write_once(chat, brief).await {
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
/// never comes here: the chat client gives it as [`CUT`].
fn whole(text: String) -> net::Result<String> {
    if text.trim_start().starts_with(made_skill::CANNOT) {
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

async fn write_once(chat: &Chat, brief: &str) -> net::Result<String> {
    tokio::time::timeout(WRITE_WITHIN, async {
        let mut stream = chat
            .send(&[Message::system(system()), Message::user(brief)])
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
    // closed; `made_in` 0 is no request of the voice's.
    let heard = (made_in != 0).then(|| {
        crate::thread::THREAD
            .lock()
            .ok()
            .and_then(|thread| thread.heard_for(made_in, std::time::Instant::now()))
            .unwrap_or_default()
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
    // One ask makes one skill: a call for an ask made or being made a
    // moment ago, carried into the next line after its turn was cut, makes
    // nothing again ([`OneAsk`]). By voice only: `made_in` 0 is no
    // request of the voice's.
    let asked = match made_in {
        0 => None,
        _ => match OneAsk::begin(&keeping.directory, heard, request, now_unix_ms) {
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
    let made = make_kept_heard(
        request,
        heard,
        brief,
        keeping,
        chat,
        &format!("{FROM}{now_unix_ms}"),
        agent,
    );
    // An ask by voice that made nothing, for the opt-in health report's
    // counts: one atomic add here, written later (`crate::made_counts`).
    if made.is_err() {
        crate::made_counts::refused(&keeping.directory, made_skill::Maker::Voice);
    }
    let needs = made
        .as_ref()
        .ok()
        .and_then(|made| made_skill::needs_link(&made.uses, linked));
    // A skill made by voice that clashes with one on the lamp (#6593: the
    // same trigger, both using the light or both moving the head) is not
    // put on: the person is asked to replace the other, keep both or
    // cancel ([`crate::made_clash`]). One with code of its own waits for
    // its yes as it did.
    if made_in != 0
        && let Ok(made) = &made
        && !made.uses.code
        && let Some(clash) = crate::made_clash::found(keeping, made)
    {
        MADE_LINE.with(|kept| kept.replace(Some(crate::made_clash::cut_line(made, &clash))));
        let told =
            crate::made_clash::asked(keeping, made, &clash, made_in, std::time::Instant::now());
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
            MADE_LINE.with(|kept| kept.replace(Some(line.clone())));
            told_with(line, made.clone(), on.clone(), needs.as_deref())
        }
        _ => {
            MADE_LINE.with(|kept| kept.replace(Some(said_line(&made, &on))));
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
        asked.made(&made.name, &told, now_unix_ms);
    }
    told
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
    fn begin(
        directory: &std::path::Path,
        heard: Option<&str>,
        request: &str,
        now_unix_ms: u64,
    ) -> Result<Self, Value> {
        let heard = ask_words(heard.unwrap_or_default());
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
        let earlier = asks.iter().find(|ask| {
            ask.directory == directory
                && (same_ask(&ask.heard, &heard) || same_ask(&ask.request, &request))
        });
        match earlier.map(|ask| &ask.made) {
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
                });
                Ok(Self(id))
            }
        }
    }

    /// The ask's skill `name` made and on, its turn told `told`.
    fn made(self, name: &str, told: &Value, now_unix_ms: u64) {
        let mut asks = ASKS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(ask) = asks.iter_mut().find(|ask| ask.id == self.0) {
            ask.made = Some((name.to_owned(), told.clone(), now_unix_ms));
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
fn asks_of(directory: &std::path::Path, made: &Made) -> Option<crate::skill_write_yes::Asks> {
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
    })
}

/// What the person's yes to the kept skill `made`, with code of its own
/// (#6307), would agree to: that package, by its SHA-256 (every byte of its
/// code), put on, and its task kept for exactly the grants said
/// (`skill_task_yes`, #6305); and what must be heard whole before the yes,
/// what its code may use ([`made_skill::code_said`]).
fn code_asks(made: &Made) -> crate::skill_write_yes::Asks {
    crate::skill_write_yes::Asks {
        skill: made.name.clone(),
        sha256: made.sha256.clone(),
        writes: Vec::new(),
        say_back: made_skill::code_said_as(
            &made.uses.grants,
            made_skill::code_goes_on_at_once(made),
        )
        .trim()
        .to_owned(),
        grants: Some(made.uses.grants.clone()),
        code: true,
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
    crate::skill_write_yes::drop_wait(id);
    eprintln!(
        "lamp-voice: a made skill's yes not the owner's: its wait dropped, nothing approved (#6066, #6307)"
    );
    Some(json!({ "error": crate::services::OTHER_VOICE }))
}

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
/// spoken yes: exactly the package made, by its SHA-256, through its yes's
/// own path ([`code_on_here`]), its task's yes recorded for the grants its
/// say-back names, so its first run asks nothing. Whether its task was
/// kept, or why it isn't on.
pub fn code_on_at_once(keeping: &Keeping, made: &Made) -> Result<bool, String> {
    let on = code_on_here(keeping, &code_asks(made))?;
    let task = on["task"] == true;
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
    let mut names = made_skill::kept_on_names(&keeping.directory);
    names.extend(crate::skill_task_yes::pushed_names(&made_skill::pushed(
        &keeping.installed,
    )));
    read_back_asked(heard, &names)
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
    static MADE_LINE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
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
    MADE_LINE.with(|kept| kept.take())
}

/// The one short line of a make that kept `made`, `on` or not; or not
/// made and why: the line [`told`] gives the model, as written. One that
/// sends or writes, made and on, is [`made_skill::cut_write_line`]: this
/// line is said only for a turn cut, which never gave the model its say-back
/// or its `approve_id`, so its yes can't be taken (App Only's P2 on #6142).
fn said_line(made: &Result<Made, Refused>, on: &Result<(), String>) -> String {
    match (made, on) {
        (Ok(made), Ok(())) if !made.uses.writes.is_empty() => made_skill::cut_write_line(made),
        (Ok(made), Ok(())) if made.uses.code => made_skill::cut_code_line(made),
        (Ok(made), Ok(())) => made_skill::on_line(made),
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

/// The skill for `request` (its `brief` made already) written by the
/// model `chat` gives and kept in `keeping` as made from `from`; a
/// refused one asked again with what was refused, up to [`MOST_REPAIRS`] times. Blocking: the model
/// is asked on a thread of its own. Shared by the voice's `make_skill` and
/// the app's typed ask (`crate::app_make`).
pub fn make_kept(
    request: &str,
    brief: String,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    from: &str,
) -> Result<Made, Refused> {
    make_kept_heard(request, None, brief, keeping, chat, from, None)
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
    let asked = made_skill::Asked {
        request,
        own: own_accounts_now(),
        heard,
    };
    // Whether the last answer was the model's own `cannot:`: its reason is
    // never journaled ([`kept_line`]).
    let models_cannot = std::cell::Cell::new(false);
    let kept = |answer: Result<String, String>| {
        models_cannot.set(matches!(&answer, Ok(text) if made_skill::says_cannot(text)));
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
            Ok(text) => made_skill::keep_asked(&text, false, from, keeping, &asked),

            Err(why) => {
                eprintln!("lamp-voice: a skill made here: the model didn't answer: {why}");
                Err(Refused(made_skill::MODEL_SILENT.into(), None))
            }
        }
    };
    // With the switch and an agent on the lamp, DeepSeek gets one try and
    // no repairs; what it doesn't make goes to the agent, which gets the
    // repairs. Without either, DeepSeek's repairs stay as they are.
    let engine = agent.and_then(|agent| agent.engine().map(|engine| (agent, engine)));
    if agent.is_some() && engine.is_none() {
        eprintln!("lamp-voice: a skill made here: no agent on the lamp: DeepSeek makes it alone");
    }
    let mut made = kept(ask(Arc::clone(&chat), brief.clone()));
    // The last refusal a repair was asked for: what the person hears when
    // a later answer is cut or never comes, never the model's cut alone.
    let mut refused_before = None;
    // Refused by the lamp's check, by `plainly`, as TOML that can't be
    // read, or the example given back: its lines go back to the model, up
    // to [`MOST_REPAIRS`] times (`made_skill::repair_brief`; the owner,
    // 10 Nov: "re-run it yourself until it works"), each begun only while
    // one more round, two answers at worst, still fits [`MAKE_WITHIN`].
    if engine.is_none() {
        (made, refused_before) = repaired(
            made,
            request,
            "",
            || round_fits(began.elapsed()),
            |repair| kept(ask(Arc::clone(&chat), repair)),
        );
    }
    let made = match engine {
        // A `cannot:` the system prompt itself asks for (the brief's own
        // refusals, [`made_skill::BRIEFS_OWN_REFUSALS`]) is said as it is,
        // never handed over (the Reviewer-lead on #6439).
        Some(_)
            if models_cannot.get()
                && matches!(&made, Err(Refused(why, _)) if made_skill::briefs_own_refusal(why)) =>
        {
            eprintln!(
                "lamp-voice: a skill made here: DeepSeek's cannot is one the brief asks for: \
                 not handed to the agent"
            );
            made
        }
        Some((agent, engine)) if made.is_err() => {
            handed_to_agent(made, request, &brief, agent, engine, &kept, &models_cannot)
        }
        Some(_) => {
            eprintln!("lamp-voice: a skill made here: made by DeepSeek");
            made
        }
        None => made,
    };
    let line = kept_line(&made, models_cannot.get(), began.elapsed().as_millis());
    // A repair's answer cut or never come (a product lamp, 11 Nov, from the
    // app: three repairs on its hum, then a cut stream, and the person
    // heard "the lamp's model stopped before it finished writing it"):
    // the person hears why the skill itself was refused. A first answer
    // cut, with nothing refused before, is told as the cut.
    let made = match (made, refused_before) {
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
    // The person hears the lamp's own reason in everyday words, the
    // journal's line above keeping it whole (the owner, 11 Nov: reasons
    // "nobody understands"); the model's `cannot:` sentence goes as it is.
    let made = match made {
        Err(Refused(why, lines)) if !models_cannot.get() => {
            Err(Refused(made_skill::plain_reason(&why), lines))
        }
        made => made,
    };
    (made, line)
}

/// A refused `made`'s repairs: its check's lines go back to the writer
/// (`again`), up to [`MOST_REPAIRS`] times, each begun only while `fits`.
/// `by` names the writer in the journal ("" for DeepSeek, as before).
/// Beside the last answer, the last refusal a repair was asked for.
fn repaired(
    mut made: Result<Made, Refused>,
    request: &str,
    by: &str,
    fits: impl Fn() -> bool,
    mut again: impl FnMut(String) -> Result<Made, Refused>,
) -> (Result<Made, Refused>, Option<Refused>) {
    let mut refused_before = None;
    for round in 1..=MOST_REPAIRS {
        let Err(Refused(why, Some(lines))) = &made else {
            break;
        };
        if !fits() {
            eprintln!(
                "lamp-voice: a made skill's repair {round} of {MOST_REPAIRS}{by}: no time left, \
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
        eprintln!("lamp-voice: a made skill's repair {round} of {MOST_REPAIRS}{by}: {why}");
        refused_before = Some(Refused(why.clone(), None));
        made = again(repair);
    }
    (made, refused_before)
}

/// A make DeepSeek didn't make (`deepseek`), handed to the lamp's agent
/// `agent`, called `engine` (`LAMP_MAKE_SKILL_AGENT`): the same system
/// prompt and `brief`, its answer through the same checks (`kept`) and
/// with repairs of its own, each begun only while one more answer fits
/// the agent's [`crate::make_skill_agent::MAKE_WITHIN`]. With no answer
/// from it at first (none within its 45 s, or no agent after all),
/// DeepSeek's failure stands, as without the switch.
fn handed_to_agent(
    deepseek: Result<Made, Refused>,
    request: &str,
    brief: &str,
    agent: &crate::make_skill_agent::Agent,
    engine: &str,
    kept: &impl Fn(Result<String, String>) -> Result<Made, Refused>,
    models_cannot: &std::cell::Cell<bool>,
) -> Result<Made, Refused> {
    eprintln!("lamp-voice: a skill made here: DeepSeek failed: handed to the agent ({engine})");
    let began = std::time::Instant::now();
    // The person's own addresses never go in the agent's rules file: each
    // stands there as a placeholder, put back in its answer before the
    // checks (the Reviewer-lead on #6439).
    let hidden = placeholders(&own_accounts());
    let system = hide(&system(), &hidden);
    // The waiting line with the first hand-over only, and never when the
    // turn was cut already (Twelve's P3 on #6439).
    let waiting = std::cell::Cell::new(!turn_cut());
    let write = |brief: &str| {
        agent
            .write(&system, brief, waiting.replace(false))
            .map(|text| shown(&text, &hidden))
            .map_err(|why| why.to_string())
            .and_then(|text| whole(text).map_err(|why| why.to_string()))
    };
    let first = match write(brief) {
        Ok(text) => text,
        Err(why) => {
            eprintln!(
                "lamp-voice: a skill made here: the agent ({engine}) made none ({why}): \
                 DeepSeek's one answer, given no repairs, stands as not made"
            );
            // No answer of the agent's was taken, so whether the last was a
            // `cannot:` is still DeepSeek's (`models_cannot`).
            return deepseek;
        }
    };
    let (made, _) = repaired(
        kept(Ok(first)),
        request,
        " by the agent",
        || began.elapsed() + agent.within <= agent.make_within,
        |repair| kept(write(&repair)),
    );
    match &made {
        Ok(_) => eprintln!("lamp-voice: a skill made here: made by the agent ({engine})"),
        Err(_) => eprintln!("lamp-voice: a skill made here: the agent's ({engine}) wasn't kept"),
    }
    // The agent's own `cannot:` is never said as it wrote it: the lamp
    // says its own fixed line (the Reviewer-lead on #6439).
    match made {
        Err(Refused(_, lines)) if models_cannot.get() => Err(Refused(
            crate::make_skill_agent::CANNOT_LINE.to_owned(),
            lines,
        )),
        made => made,
    }
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

/// The model's answer to `brief`, asked on a thread of its own with a
/// runtime of its own.
fn ask(
    chat: Arc<impl Fn() -> Result<Chat, String> + Send + Sync + 'static>,
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
                .block_on(written(&chat, &brief))
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
    told_with(said_line(&made, &on), made, on, needs)
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
            "error": format!("the skill couldn't be made: {why}"),
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
        let asking = Asking {
            request,
            heard,
            owner_hears: crate::services::owner_hears(request, heard, lease),
            sends_here: crate::services::sends_here(),
        };
        if tasks
            && made_skill::kept_on_named(&keeping.directory, name).is_none()
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
    // `request` is not required, so a clash's choice can come alone
    // (#6593): a make with none, or a choice with no clash_id, makes
    // nothing and is told so plainly (Heron's P3 on #6614).
    if call.args["request"]
        .as_str()
        .is_none_or(|request| request.trim().is_empty())
    {
        eprintln!("lamp-voice: request {request}: a skill wasn't made: the call had no request");
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
    let request_words = call.args["request"].as_str().unwrap_or_default();
    make_in(request, request_words, &keeping, own_chat, now_unix_ms)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    /// Heron's P1 on #6558: a voice make's write waits for the owner's yes
    /// only. A visitor's "yes" approves nothing and drops the wait; the
    /// owner's approves; an id that isn't waiting is left to `approve`.
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
            answer(&call, 1, false, None),
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
            let told = answer(&call, 2, true, None);
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
                    assert!(body.contains(made_skill::BRIEF_HEAD), "{body}");
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

    /// A manage helper at `socket` that puts on whatever made skill it is
    /// asked for, once: the line it was asked.
    pub(crate) fn manage_helper(socket: &Path) -> std::thread::JoinHandle<Value> {
        use std::io::BufRead;
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
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
        helper.join().unwrap();
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
        assert_eq!(route(&settings, made_skill::MAKE), Route::MakeLocal);
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
    fn faces() -> String {
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
                // The same system prompt and brief DeepSeek is sent.
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
        assert_eq!(tasks.prepares(), 0);
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
        assert_eq!(tasks.prepares(), 2);
        assert_eq!(made_skill::entries(&keeping.directory).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
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
        assert_eq!(none.prepares(), 0);
        let silent = agent_answering(vec![None]);
        let (url, served) = service(vec![(200, faces())]);
        let told = made_by(&keeping, url, Some(&silent.agent()), NOW + 1);
        assert_eq!(
            told["error"],
            format!("the skill couldn't be made: {}", made_skill::NOT_MADE_TRY)
        );
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(silent.prepares(), 1);
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
        assert_eq!(tasks.prepares(), 0);
        let other = "cannot: a skill can't watch the weather.";
        let (url, served) = service(vec![(200, other.to_owned())]);
        let told = made_by(&keeping, url, Some(&tasks.agent()), NOW + 1);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(tasks.prepares(), 1);
        let _ = std::fs::remove_dir_all(&root);
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
        assert_eq!(tasks.prepares(), 1);
        let said = told.to_string();
        assert!(!said.contains("AGENT-WORDS-9"), "{said}");
        assert!(
            said.contains(crate::make_skill_agent::CANNOT_LINE),
            "{said}"
        );
        let _ = std::fs::remove_dir_all(&root);
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
                     desk is room/presence, never the faces): {} A body step is one gesture, \
                     one of: nod, headshake, ",
                    made_skill::TRIGGER_MEANINGS
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
        let long = "x".repeat(made_skill::MOST_REQUEST_BYTES + 1);
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
            for name in [made_skill::MAKE, "put_on_made_skill"] {
                assert_eq!(route(&off, name), Route::Nobody, "{name}");
                assert!(declared(&off).iter().all(|tool| tool.name != name));
            }
            let on = Settings {
                make_skill: true,
                tasks,
                ..Settings::default()
            };
            assert_eq!(route(&on, made_skill::MAKE), Route::MakeLocal);
            assert_eq!(
                declared(&on)
                    .iter()
                    .filter(|tool| tool.name == made_skill::MAKE)
                    .count(),
                1
            );
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
    const CODE_REQUEST: &str = "make me a photo booth that takes a photo when I sit down";

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
            "body, light, speak, look, hear, sound, air, device, camera, microphone, notes, speech",
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
            "What this skill should do and when, in the person's own words for this ask, \
             nothing else: never a time, step or trigger of an earlier skill or turn."
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
        assert!(all.parameters.unwrap().to_string().len() <= 4_096);
        crate::skill_task_yes::set_on_here(None);
        made_skill::set_writes_here(None);
        made_skill::set_scripts_here(None);
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

    /// The first builder's ruling on #6059 (19:14): the grants that go on
    /// at once are light, the lamp's own voice and sounds (`speak`) and the
    /// body alone (19:44: `sound` is hearing); the camera, the microphone
    /// and `sound` always keep the spoken yes.
    #[test]
    fn the_grants_that_go_on_at_once_never_hold_the_camera_or_the_microphone() {
        assert_eq!(made_skill::CODE_AT_ONCE_GRANTS, ["light", "speak", "body"]);
        assert_eq!(
            made_skill::CODE_YES_GRANTS,
            ["camera", "microphone", "sound"]
        );
        for grant in made_skill::CODE_AT_ONCE_GRANTS {
            assert!(!made_skill::CODE_YES_GRANTS.contains(&grant), "{grant}");
            assert!(made_skill::GRANTS.iter().any(|(name, _)| *name == grant));
        }
        made_skill::set_scripts_here(Some(true));
        let grants = |names: &[&str]| -> Vec<String> {
            names.iter().map(|name| (*name).to_owned()).collect()
        };
        made_skill::set_code_at_once_here(Some(true));
        assert!(made_skill::grants_go_on_at_once(&grants(&[])));
        assert!(made_skill::grants_go_on_at_once(&grants(&[
            "light", "speak", "body"
        ])));
        for kept in [
            "camera",
            "microphone",
            "sound",
            "hear",
            "speech",
            "notes",
            "look",
        ] {
            assert!(
                !made_skill::grants_go_on_at_once(&grants(&["light", kept])),
                "{kept}"
            );
        }
        made_skill::set_code_at_once_here(Some(false));
        assert!(!made_skill::grants_go_on_at_once(&grants(&["light"])));
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
    }

    /// `LAMP_MADE_CODE_ON_AT_ONCE` on: a voice make whose code is granted
    /// only its light but knows who is at the lamp (`who`, `known_faces`:
    /// the first builder at 20:16 on #6659) keeps its spoken yes, its
    /// say-back ending on its wait.
    #[test]
    fn with_the_switch_a_voice_code_make_knowing_who_is_there_still_waits_for_its_yes() {
        made_skill::set_scripts_here(Some(true));
        made_skill::set_code_at_once_here(Some(true));
        for (at, key) in [(0, "who"), (1, "known_faces")] {
            let (root, keeping) = keeping(&format!("code-at-once-{key}"));
            let toml = code_light_toml()
                .replace("[permissions]\n", &format!("[permissions]\n{key} = true\n"));
            let (url, served) = service(vec![(200, code_answer_of(&toml))]);
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
            assert_eq!(told["on"], false, "{key}: {told}");
            assert_eq!(told["code_waits_for_yes"], true, "{key}: {told}");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains("knowing who is in view"), "{key}: {next}");
            assert!(
                next.contains(made_skill::RUNS_AFTER_YES.trim()),
                "{key}: {next}"
            );
            assert!(!next.contains("it's on"), "{key}: {next}");
            assert_ne!(made_skill::entries(&keeping.directory)[0]["on"], true);
            let _ = take_said_line();
            let id = told[crate::skill_write_yes::APPROVE_ID].as_str().unwrap();
            crate::skill_write_yes::drop_wait(id);
            let _ = std::fs::remove_dir_all(&root);
        }
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
    }

    /// `LAMP_MADE_CODE_ON_AT_ONCE` on: a voice make whose code uses the
    /// camera keeps its spoken yes, exactly as with the switch off.
    #[test]
    fn with_the_switch_a_voice_code_make_with_the_camera_still_waits_for_its_yes() {
        made_skill::set_scripts_here(Some(true));
        made_skill::set_code_at_once_here(Some(true));
        let (root, keeping) = keeping("code-at-once-camera");
        let (url, served) = service(vec![(200, code_answer_of(CODE_TOML))]);
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
        assert_eq!(told["on"], false, "{told}");
        assert_eq!(told["code_waits_for_yes"], true, "{told}");
        assert!(
            told[crate::skill_write_yes::APPROVE_ID].is_string(),
            "{told}"
        );
        let next = told["next"].as_str().unwrap();
        assert!(next.contains(made_skill::RUNS_AFTER_YES.trim()), "{next}");
        assert!(!next.contains("it's on"), "{next}");
        assert_ne!(made_skill::entries(&keeping.directory)[0]["on"], true);
        assert!(take_said_line().unwrap().ends_with("so it isn't on."));
        let id = told[crate::skill_write_yes::APPROVE_ID].as_str().unwrap();
        crate::skill_write_yes::drop_wait(id);
        made_skill::set_code_at_once_here(None);
        made_skill::set_scripts_here(None);
        let _ = std::fs::remove_dir_all(&root);
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
        // Its task's yes, as `code_approve` records it on a spoken yes: the
        // folder as it went on, with exactly the grants said back.
        let recorded = tasks_recorded_here();
        let [kept] = recorded.as_slice() else {
            panic!("{recorded:?}")
        };
        assert_eq!(
            (kept.skill.as_str(), kept.sha256.as_str(), kept.code),
            ("photo-booth", folder.as_str(), false)
        );
        assert_eq!(kept.grants, Some(vec!["light".to_owned()]));
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
}
