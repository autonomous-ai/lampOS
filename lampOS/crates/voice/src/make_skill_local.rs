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
//! there: a computer's agent is never handed a skill to make.
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

/// Whether a repair begun `elapsed` into a make still ends within
/// [`MAKE_WITHIN`], its answer asked twice at worst.
fn round_fits(elapsed: Duration) -> bool {
    elapsed + ROUND_WITHIN <= MAKE_WITHIN
}
/// The most of an answer read: two files of at most 8 KiB, and the fences.
const MOST_ANSWER_BYTES: usize = 20 * 1024;
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
    let (cannot, left_out) = (made_skill::CANNOT, made_skill::LEFT_OUT);
    let writes = made_skill::writes_on();
    let services = services(reads, writes, &own_accounts());
    // With a made skill's writes (#6066) a write to a linked service is
    // told below; a text to a phone stays out of reach.
    let cant_send = if writes {
        "or send a message to a phone"
    } else {
        "send a message or write to a service"
    };
    let triggers = made_skill::TRIGGER_MEANINGS;
    let gestures = made_skill::gesture_names();
    let (more, cant_yet) = (made_skill::MORE_STEPS, made_skill::CANT_YET);
    format!(
        "You write skills for a lamp. Answer exactly as the request says, with the two fenced \
         blocks and nothing else. Use only the keys and values the request names, written as \
         in this example, which the lamp's check takes:\n\n{EXAMPLE}\n\nThe example shows \
         the form only: never answer with it. A reminder at a time or every so often is the \
         lamp's own timer, not a skill; a skill runs at a time only each day or on named \
         days of the week, by the clock trigger below. A skill made on this lamp may do whatever the lamp can do: make what \
         the request asks with the light, the head, a few exact words it says, its hum, or a \
         read as below, on one of the triggers below. {more} To speak, a skill has speak = \
         true in [permissions], cooldown = \"1h\" or longer, and one [[do]] step say = {{ text = \
         \"Welcome back!\" }}: the exact words, one or two short sentences, no quotes, in the \
         language the person asked in, said as written. Its one sound is a soft hum: a [[do]] \
         step cue = \"hum\", paced as a say, in place of a say; it has no other sound and no \
         sound file. When it runs is one \
         [trigger] table (never [[trigger]]; coming to the desk is room/presence, never the \
         faces): {triggers} A body step is one gesture, one of: {gestures}. Set none of \
         most_a_day, [quiet], [budget] or [rest] unless the request asks for more in its own \
         words: the lamp sets them. A skill can't yet \
         hear what is said, know who someone is, {cant_send}: a \
         request that needs one, or anything else no step or trigger here does, is answered \
         with one line and no blocks: {cannot} then one short sentence said to the person, \
         beginning \"{cant_yet}\" and naming what in plain words (\"{cant_yet} sending a \
         message.\"), never a rule, ending with \
         what to ask instead when there is one; \
         for a reminder at a time or every so often: \"Ask me for a reminder instead, like 'remind \
         me every hour to stand up'.\" Never stand something \
         else in for it, and add no time of day the request didn't name. When a part of the \
         request can't be done with these (dimming slowly, over a minute), make the rest and \
         add one line after the blocks: {left_out} what was left out, in a few words.\n\n\
         {services}"
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
fn services(reads: bool, writes: bool, own: &[(String, String)]) -> String {
    let cannot = made_skill::CANNOT;
    let ask_me = "You can ask me for it yourself any time.";
    // A write on its own (#6066): its rules with the switch, else the
    // lamp's "can't do that yet".
    let write = if writes {
        write_rules(own)
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
        let what = if writes { "read" } else { "use" };
        return format!(
            "A skill can't {what} the person's mail, calendar or any other linked service yet, \
             even one already linked: a request that needs one is answered with one line and \
             no blocks: {cannot} Not possible yet: a skill can't {what} your mail, calendar or \
             other services, even ones already linked. {ask_me} Never tell them to link one.\n\
             {write}"
        );
    }
    let steps = made_skill::read_steps().join("\n- ");
    let unreadable = made_skill::unreadable_names().join(", ");
    let (only, asks_more) = if writes {
        (
            "never deletes or changes anything in a service, and writes only as below",
            "A request to delete or change anything in a service",
        )
    } else {
        (
            "never sends, writes, posts, books, schedules, deletes or changes anything in a \
             service",
            "A request to send, write or change anything in a service",
        )
    };
    format!(
        "A skill may read at most one of the person's linked services, and only read it: it \
         {only}. A request to be read one of these is a skill with [permissions] only \
         speak = true, services = {{ CODE = \"read\" }} and its topics; its trigger's during \
         only for a time the request names; and exactly one [[do]] step, the one shown here \
         for that service, filled in from the request, and no light or head step. The services a skill can read, each as name (CODE): step:\n- {steps}\n\
         {asks_more}, to read two or more \
         services, or to read one not listed here ({unreadable}, or any other) is answered \
         with one line and no blocks: {cannot} then one short sentence saying what a skill \
         can do instead (read one linked service), ending \"{ask_me}\" Never tell them to \
         link a service.\n{write}"
    )
}

/// What the making model is told of a made skill's write (#6066, with
/// [`made_skill::WRITES_ENVIRONMENT`] on): the kinds `lamp-services` sends,
/// their keys, and the first builder's conditions: the words only the
/// request's own, never a read's, the web's or memory's; the recipients
/// only an address the request says or the person's own (`own`, each
/// linked service's code and account); and the lamp says it back whole and
/// waits for the person's yes. A request it can't make so is `cannot:`.
fn write_rules(own: &[(String, String)]) -> String {
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
         an email address the request says, or the person's own: {mine}. A repo, team, project \
         or file is only one the request names. A request whose words to send, or whose \
         recipient's address, it doesn't say (\"mail Mai that I left\") is answered with one \
         line and no blocks: {cannot} {cant_yet} sending what you didn't say word for word, or \
         to an address you didn't say; say the exact words and the address.",
        kinds.join("\n- ")
    )
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
    lamp_gemini::Tool {
        name: made_skill::MAKE.into(),
        description,
        parameters: Some(parameters(writes)),
        silent: false,
    }
}

/// What [`tool`]'s description adds with a made skill's writes on.
pub const READ_BACK_TOLD: &str = " Call it too, with read_back alone, when the person asks \
     what a skill they made sends or to hear one read back, saying nothing before it.";

/// The function's parameters: the request, and with a made skill's writes
/// on (#6066) the yes to one that waits for it, `approve_id`, given alone.
fn parameters(writes: bool) -> Value {
    let mut parameters = json!({
        "type": "object",
        "properties": {
            "request": {
                "type": "string",
                "description": "What the skill should do and when, in the person's own words, nothing else."
            }
        },
        "required": ["request"]
    });
    if writes {
        parameters["properties"][crate::skill_write_yes::APPROVE_ID] = json!({
            "type": "string",
            "description": "Only when a skill you just made or read back waits for the person's yes and they then agree, in any words: its approve_id, given alone with no request."
        });
        parameters["properties"][READ_BACK] = json!({
            "type": "string",
            "description": "Only when the person asks what a skill they made sends, or to hear it read back: its name, given alone with no request."
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
                if text.len() > MOST_ANSWER_BYTES {
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
    make_with(
        made_in,
        heard.as_deref(),
        request,
        keeping,
        chat,
        now_unix_ms,
        || crate::services::linked_codes(std::path::Path::new(crate::services::FOLDER)),
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
    let on = made
        .as_ref()
        .map(|made| made_skill::put_on(keeping, made))
        .unwrap_or(Ok(()));
    MADE_LINE.with(|kept| kept.replace(Some(said_line(&made, &on))));
    let told = told(made.clone(), on.clone(), needs.as_deref());
    // A skill that sends or writes, once on (its yes binds the installed
    // package): its writes wait for the person's yes, in a later heard
    // line (#6066, `crate::skill_write_yes`).
    match (&made, &on) {
        (Ok(made), Ok(())) => {
            let Some(asks) = asks_of(&keeping.directory, made) else {
                return told;
            };
            let id = crate::skill_write_yes::wait_for_yes(asks, made_in, std::time::Instant::now());
            waits_for_yes(told, &made_skill::on_line(made), &id)
        }
        _ => told,
    }
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
    })
}

/// The `make_skill` argument that reads a made skill back by its name.
pub const READ_BACK: &str = "read_back";

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
    /// What only the owner may hear is said to it: the private room, or
    /// the owner seen saying it (`services::owner_hears`, as a read in full).
    pub owner_hears: bool,
    /// The route with the echo judge, where a yes may be taken
    /// (`services::sends_here`).
    pub sends_here: bool,
}

/// What the model is told of a skill that sends, read back to a line the
/// owner was not seen saying, outside the private room: its name only.
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
/// (`owner_hears`: the private room, or the owner seen saying the line, as
/// a services read in full; App Only's P2 on #6230); to anyone else its
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
/// said so ([`changed_since_made`]). One that sends nothing, or none of
/// that name, is said so. What the model is told.
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
    let not_on = |names: Vec<String>| {
        let on: Vec<String> = names
            .iter()
            .map(|name| made_skill::spoken_name(name))
            .collect();
        json!({
            "error": "no skill made on this lamp is on by that name",
            "on": on,
            "next": "Say plainly, in a few words, that no skill they made is on by that name, \
                     and name the ones that are, if any. Call nothing."
        })
    };
    let Some(made) = made_skill::kept_on_named(directory, name) else {
        eprintln!("lamp-voice: request {request}: a made skill to read back was not found");
        return not_on(made_skill::kept_on_names(directory));
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
    make_kept_heard(request, None, brief, keeping, chat, from)
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
) -> Result<Made, Refused> {
    let (made, line) = made_and_journaled(request, heard, brief, keeping, chat, from);
    eprintln!("{line}");
    made
}

/// [`make_kept`]'s make, and the journal's line for it ([`kept_line`]).
fn made_and_journaled(
    request: &str,
    heard: Option<&str>,
    brief: String,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    from: &str,
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
                    "the lamp's model gave back its own example, not what was asked; try asking again".into(),
                    Some(EXAMPLE_GIVEN_BACK.into()),
                ))
            }
            Ok(text) => made_skill::keep_asked(&text, false, from, keeping, &asked),

            Err(why) => {
                eprintln!("lamp-voice: a skill made here: the model didn't answer: {why}");
                Err(Refused("the lamp's model didn't answer".into(), None))
            }
        }
    };
    let mut made = kept(ask(Arc::clone(&chat), brief));
    // Refused by the lamp's check, by `plainly`, as TOML that can't be
    // read, or the example given back: its lines go back to the model, up
    // to [`MOST_REPAIRS`] times (`made_skill::repair_brief`; the owner,
    // 10 Nov: "re-run it yourself until it works"), each begun only while
    // one more round, two answers at worst, still fits [`MAKE_WITHIN`].
    for round in 1..=MOST_REPAIRS {
        let Err(Refused(why, Some(lines))) = &made else {
            break;
        };
        if !round_fits(began.elapsed()) {
            eprintln!(
                "lamp-voice: a made skill's repair {round} of {MOST_REPAIRS}: no time left, \
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
        eprintln!("lamp-voice: a made skill's repair {round} of {MOST_REPAIRS}: {why}");
        made = kept(ask(Arc::clone(&chat), repair));
    }
    let line = kept_line(&made, models_cannot.get(), began.elapsed().as_millis());
    (made, line)
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
    let line = said_line(&made, &on);
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

pub fn answer(
    call: &lamp_gemini::ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    // The person's yes to a made skill's writes (#6066): the waiting
    // skill's, in a later heard line, approved by the voice itself.
    if let Some(id) = call.args[crate::skill_write_yes::APPROVE_ID].as_str() {
        return crate::skill_write_yes::approve(
            id,
            request,
            heard,
            crate::services::sends_here(),
            std::time::Instant::now(),
            |asks| {
                crate::services::skill_write_approve(
                    std::path::Path::new(crate::services::FOLDER),
                    asks,
                )
            },
        );
    }
    // A made skill read back by voice (`LAMP_SKILL_WRITES`): what it
    // sends, and its yes awaited again when it waits for one.
    if let Some(name) = call.args[READ_BACK].as_str() {
        if !made_skill::writes_on() {
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
                let mut count = 0;
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
                    count += 1;
                    let body = std::str::from_utf8(&body).expect("authenticated chat body is UTF-8");
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
                count
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
        assert!(told["error"].is_string(), "{told}");
        assert_eq!(told["on"], json!(["evening dim"]), "{told}");
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

    /// App Only's P2 on #6230: a write skill's recipients and words are
    /// read back only in the private room or to the owner seen saying the
    /// line (`services::owner_seen_saying`, as a read in full); to anyone
    /// else its name only, that it sends, nothing asked of `lamp-services`
    /// and no wait started.
    #[test]
    fn a_read_back_is_said_whole_only_in_the_private_room_or_to_the_owner_seen() {
        let (root, keeping, made_line) = made_write("read-back-who");
        let now = std::time::Instant::now();
        for (why, private_room, owner, seen, whole) in [
            ("the private room", true, Some("owner"), false, true),
            ("the owner seen saying it", false, Some("owner"), true, true),
            ("the owner not seen", false, Some("owner"), false, false),
            ("no owner's line", false, None, true, false),
        ] {
            let owner_hears = private_room
                || crate::services::owner_seen_saying(true, private_room, owner, |_| seen);
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
        assert!(parameters(false)["properties"].get("approve_id").is_none());
        assert_eq!(parameters(false)["required"], json!(["request"]));
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
        assert!(parameters(true)["properties"]["approve_id"].is_object());
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
            format!("the skill couldn't be made: {}", made_skill::NOT_PASSED)
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
            format!("the skill couldn't be made: {}", made_skill::UNSAID)
        );
        assert_eq!(
            told["next"],
            made_skill::not_made_in_their_language(&made_skill::not_made_line(made_skill::UNSAID))
        );
        assert_eq!(served.join().unwrap(), 1 + MOST_REPAIRS);
        assert_eq!(made_skill::entries(&keeping.directory).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
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
                .contains("the lamp can't read"),
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
            "the skill couldn't be made: the lamp's model gave back its own example, not what was asked; try asking again",
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
}
