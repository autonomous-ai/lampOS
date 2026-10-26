//! What a person teaches the lamp about how to behave with them (#813):
//! corrections and standing preferences, kept as short plain rules and
//! obeyed in every later session.
//!
//! Off unless the provider worker has `LAMP_MEMORY_RULES=on` and the Days
//! memory (`LAMP_MEMORY_DAYS=on`); without both the model is told of no new
//! function, the memory is asked nothing new and every note is as before.
//!
//! - **Three functions for the model** ([`tools`]): `keep_rule`,
//!   `forget_rule` and `list_rules`. Not tasks, reminders or facts: those
//!   stay `remember`.
//! - **Kept beside, never in, a person's memory**: the rules of the person
//!   with memory id `p-<hex>` are the memory's `kept` entries of scope
//!   `r-<hex>`, and the room's of `r-everyone` ([`scope`]). So a recall
//!   never returns a rule, and a rule is never told to anyone else.
//! - **Told to each new session** ([`note`]): the room's, and the known
//!   person's after them: every rule, at most six a scope ([`admit`]).
//! - **A typed fact for the policy** ([`facts`], `lamp_msgs::Rules` on
//!   `memory/rules`): whether someone asked the lamp not to speak first,
//!   at all or in the mornings. Never the words of a rule.
//!
//! No word of a rule is ever logged.
use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Condvar, Mutex, OnceLock,
        mpsc::{Receiver, SyncSender, sync_channel},
    },
    time::{Duration, Instant},
};

/// Only the exact value `on` enables rules, and only with the Days memory.
/// The coordinator passes this one setting through its cleared worker
/// environment, like `LAMP_MEMORY_DAYS`.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_RULES";

pub const KEEP_RULE: &str = "keep_rule";
pub const FORGET_RULE: &str = "forget_rule";
pub const LIST_RULES: &str = "list_rules";

/// The longest rule kept: one short sentence, never a person's whole words.
const RULE_BYTES: usize = 200;
/// The most rules one scope keeps. A new one past it is refused with words
/// the model can say, never made room for by dropping an older one; so a
/// session is told every rule, at most 6 x 200 bytes of rule text a scope.
pub const MOST_RULES: usize = 6;
/// What a read of one scope asks for: the store's most, so that every rule
/// is listed, told and counted in the fact, never a newest slice of them.
const READ_BYTES: u64 = 16_384;
/// What a rule about speaking first is marked with in the memory.
const QUIET: &str = "[quiet] ";
const QUIET_MORNINGS: &str = "[quiet_mornings] ";
/// What a rule about one kind of thing said unprompted is marked with,
/// after any of the two above: `[unprompted <about> <limit>...] `.
const TYPED: &str = "[unprompted ";

static ON: OnceLock<bool> = OnceLock::new();
static BUS: OnceLock<PathBuf> = OnceLock::new();

pub fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

#[cfg(test)]
thread_local! {
    /// The switch for one test's own thread ([`on_for_test`]): the process
    /// switch is set once, and other tests read it off.
    static ON_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether this test's thread has the switch on ([`set_on_for_test`]).
#[cfg(test)]
fn on_for_test() -> bool {
    ON_FOR_TEST.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn on_for_test() -> bool {
    false
}

/// The switch on or off for the calling test's thread alone.
#[cfg(test)]
pub(crate) fn set_on_for_test(on: bool) {
    ON_FOR_TEST.with(|cell| cell.set(on));
}

/// Set once, by [`crate::remember::start`].
pub(crate) fn switch(on: bool) {
    let _ = ON.set(on);
}

pub fn on() -> bool {
    ON.get() == Some(&true) || on_for_test()
}

/// Where the typed fact is published: the bus's root, when the voice has a
/// bus. Without it the rules still work; the policy is not told. With the
/// bus, the room's rules are read from the memory on `memory` and said at
/// once (#1747): one call, so the start's fact is never left out.
pub fn bus_on(root: &Path, memory: &Path) {
    let _ = BUS.set(root.to_owned());
    crate::remember::tell_rules_at_start(memory.to_owned());
}

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: KEEP_RULE.into(),
            description: "Call this every time the person corrects how you behave with them or says how you should \
                          behave from now on, even when they say it in passing and you have already started doing \
                          it: anything with 'from now on', 'always', 'never', 'stop ...', 'don't call me ...', \
                          'you don't need to ...' about you. For example 'from now on keep your answers to one short \
                          sentence', 'please stop calling me buddy', 'never tell me the weather unless I ask'. Doing \
                          it only in this conversation is not enough: without this call you will do it wrong again \
                          tomorrow. Not for a one-off request about this answer alone ('say that again more slowly'), \
                          and not for tasks, reminders or things to remember: those are remember. Give the rule as \
                          one short plain sentence about what you will or will not do, such as 'Keep answers to one \
                          short sentence.', never their whole words. Then say back briefly what you will do \
                          differently."
                .into(),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "rule": {"type": "string", "description": "The rule, in one short sentence."},
                    "speaking_up": {"type": "string", "enum": ["never_first", "not_first_in_mornings"],
                        "description": "Only for a rule about you speaking first, unprompted: never_first if they asked you never to, not_first_in_mornings if only in the mornings. Leave out otherwise."},
                    "about": {"type": "string", "enum": ["greeting", "news", "both", "light"],
                        "description": "Only for a rule about one kind of thing you do unprompted: greeting when they arrive, news of work you were doing for them, or both; or light, your light going out by itself when nobody is there, which takes never only ('keep the light on'). Give with it what they asked: never, not_before, not_from or not_while_talking. Leave out otherwise. A light rule is the room's, whoever says it."},
                    "never": {"type": "boolean", "description": "With about: not at all."},
                    "not_before": {"type": "string", "description": "With about: not before this local time, as HH:MM in 24 hours, such as 09:00 for 'don't greet me before nine'."},
                    "not_from": {"type": "string", "description": "With about: not from this local time on, as HH:MM in 24 hours, such as 22:00 for 'no news after ten at night'."},
                    "not_while_talking": {"type": "boolean", "description": "With about: not while they are talking, on a call or with someone."}
                },
                "required": ["rule"]
            })),
            silent: false,
        },
        Tool {
            name: FORGET_RULE.into(),
            description: "Call this when the person takes back one of the rules they gave you, such as 'you can tell \
                          me the weather again' or 'forget that rule'. Give the number of the rule they take back, \
                          as numbered under 'How you were asked to behave' or by list_rules; if you are not sure \
                          which one, call list_rules first. Then stop following it. For 'you can turn your light \
                          off again' give about: light instead of a number: it takes back keeping the light on, \
                          whoever asked for it."
                .into(),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "number": {"type": "integer", "description": "The number of the rule they take back."},
                    "about": {"type": "string", "enum": ["light"], "description": "Instead of a number: light, to take back keeping the light on."}
                }
            })),
            silent: false,
        },
        Tool {
            name: LIST_RULES.into(),
            description: "Call this when the person asks what they asked you to do or not to do, or how they asked \
                          you to behave. Each rule comes with its number, for forget_rule only: tell them the \
                          rules in your own words, never their numbers."
                .into(),
            parameters: None,
            silent: false,
        },
    ]
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, KEEP_RULE | FORGET_RULE | LIST_RULES)
}

/// The memory's scope for the rules of this person ([`crate::remember::person_id`]),
/// or of the room's for `None`: beside their memory, never in it.
pub fn scope(person: Option<&str>) -> String {
    let person = person.map_or("everyone", |id| id.strip_prefix("p-").unwrap_or(id));
    format!("r-{person}")
}

/// The memory's request for a call, or what to tell the model at once.
pub fn request(call: &ToolCall, now_unix_ms: u64, scope: &str) -> Result<Value, Value> {
    Ok(match call.name.as_str() {
        KEEP_RULE => {
            let rule = call
                .args
                .get("rule")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if !rule.chars().any(char::is_alphanumeric) {
                return Err(json!({"error": "There was no rule in that for me to keep."}));
            }
            if rule.len() > RULE_BYTES {
                return Err(
                    json!({"error": "That is too long for a rule: give it as one short sentence."}),
                );
            }
            let mark = match call.args.get("speaking_up").and_then(Value::as_str) {
                Some("never_first") => QUIET,
                Some("not_first_in_mornings") => QUIET_MORNINGS,
                _ => "",
            };
            let typed = typed_mark(&call.args)?;
            json!({"op":"remember","person":scope,"at_unix_ms":now_unix_ms,"text":format!("{mark}{typed}{rule}")})
        }
        // Never a default: "forget that rule" once took back the last rule
        // kept instead of the one meant (lamp-52e6, PR #822).
        FORGET_RULE => match call
            .args
            .get("number")
            .and_then(Value::as_u64)
            .filter(|id| *id > 0 && *id <= i64::MAX as u64)
        {
            Some(id) => json!({"op":"forget","person":scope,"id":id}),
            None => return Err(which()),
        },
        _ => read_request(scope),
    })
}

/// The mark of a rule about one kind of thing said unprompted, from the
/// `about` arguments: empty when the rule has none, so it is told to
/// sessions and enforced by nothing. A limit that cannot be read is
/// refused with words the model can say, never kept half-typed.
fn typed_mark(args: &Value) -> Result<String, Value> {
    let time = |name: &str| -> Result<Option<u16>, Value> {
        let Some(value) = args.get(name).filter(|value| !value.is_null()) else {
            return Ok(None);
        };
        minute(value.as_str().unwrap_or_default()).map(Some).ok_or_else(
            || json!({"error": format!("{name} must be a time of day as HH:MM in 24 hours, such as 09:00.")}),
        )
    };
    // A limit of the wrong type is refused, never read as false: the
    // provider does not hold the model to the declared schema.
    let flag = |name: &str| -> Result<bool, Value> {
        match args.get(name).filter(|value| !value.is_null()) {
            None => Ok(false),
            Some(value) => value
                .as_bool()
                .ok_or_else(|| json!({"error": format!("{name} must be true or false.")})),
        }
    };
    let (never, talking) = (flag("never")?, flag("not_while_talking")?);
    let (before, from) = (time("not_before")?, time("not_from")?);
    if !never && !talking && before.is_none() && from.is_none() {
        return Ok(String::new());
    }
    let about = match args.get("about").and_then(Value::as_str) {
        Some(about @ ("greeting" | "news" | "both")) => about,
        // #1677: the light going out when nobody is there, held or not.
        Some("light") if never && !talking && before.is_none() && from.is_none() => "light",
        Some("light") => {
            return Err(
                json!({"error": "For the light, give never only: keep the light on, or not."}),
            );
        }
        _ => {
            return Err(
                json!({"error": "Say what that is about: about must be greeting, news, both or light."}),
            );
        }
    };
    if before.is_some() && before == from {
        return Err(json!({"error": "not_before and not_from cannot be the same time."}));
    }
    let mut mark = format!("{TYPED}{about}");
    if never {
        mark.push_str(" never");
    }
    if let Some(minute) = before {
        mark.push_str(&format!(" not_before={minute}"));
    }
    if let Some(minute) = from {
        mark.push_str(&format!(" not_from={minute}"));
    }
    if talking {
        mark.push_str(" not_while_talking");
    }
    mark.push_str("] ");
    Ok(mark)
}

/// `HH:MM`, 24 hours, as the minute of the day.
fn minute(time: &str) -> Option<u16> {
    // Digits only: `parse` alone takes a sign, so "+1:+2" read as 01:02.
    let (hours, minutes) = time.split_once(':')?;
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if !(1..=2).contains(&hours.len()) || minutes.len() != 2 || !digits(hours) || !digits(minutes) {
        return None;
    }
    let (hours, minutes) = (hours.parse::<u16>().ok()?, minutes.parse::<u16>().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// The limits a typed mark's words give, for the kinds it is about: the
/// greeting, news, and whether the light is kept on (#1677). An unknown
/// word is left out: a newer voice's limit is not enforced by this one,
/// rather than misread.
fn limits(mark: &str) -> (lamp_msgs::Unprompted, bool, bool, bool) {
    let mut words = mark.split(' ');
    let about = words.next().unwrap_or_default();
    let mut limits = lamp_msgs::Unprompted::default();
    for word in words {
        match word.split_once('=') {
            None if word == "never" => limits.never = true,
            None if word == "not_while_talking" => limits.not_while_talking = true,
            Some(("not_before", minute)) => limits.not_before_minute = minute.parse().ok(),
            Some(("not_from", minute)) => limits.not_from_minute = minute.parse().ok(),
            _ => {}
        }
    }
    (
        limits,
        matches!(about, "greeting" | "both"),
        matches!(about, "news" | "both"),
        about == "light" && limits.never,
    )
}

/// Every limit of both, the strictest winning: the latest `not_before`,
/// the earliest `not_from`.
fn strictest(a: lamp_msgs::Unprompted, b: lamp_msgs::Unprompted) -> lamp_msgs::Unprompted {
    lamp_msgs::Unprompted {
        never: a.never || b.never,
        not_before_minute: a.not_before_minute.max(b.not_before_minute),
        not_from_minute: match (a.not_from_minute, b.not_from_minute) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        },
        not_while_talking: a.not_while_talking || b.not_while_talking,
    }
}

/// What the model is told for a number not given or not one of theirs:
/// nothing was removed.
fn which() -> Value {
    json!({"forgotten": false, "note": "Nothing was removed: I need the number of the rule they take back. Call list_rules, and if it is still not clear which one, ask them."})
}

/// One kept rule, without its mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub id: u64,
    pub at_unix_ms: u64,
    pub text: String,
    pub quiet: bool,
    pub quiet_mornings: bool,
    /// Its limits on the greeting, and on news, said unprompted.
    pub hello: lamp_msgs::Unprompted,
    pub news: lamp_msgs::Unprompted,
    /// Asked to keep the light on: light-out never starts (#1677).
    pub keep_light_on: bool,
}

/// A record ID as the store gives them, 1 to `i64::MAX`; anything else is
/// a malformed answer.
pub fn valid_id(id: &Value) -> Option<u64> {
    id.as_u64().filter(|id| *id > 0 && *id <= i64::MAX as u64)
}

/// The rules in the memory's answer to `kept`, oldest first.
pub fn rules(answer: &Value) -> Option<Vec<Rule>> {
    if answer["ok"] != true {
        return None;
    }
    answer
        .get("entries")?
        .as_array()?
        .iter()
        .map(|entry| {
            let text = entry.get("text")?.as_str()?;
            let (quiet, quiet_mornings, text) = if let Some(text) = text.strip_prefix(QUIET) {
                (true, false, text)
            } else if let Some(text) = text.strip_prefix(QUIET_MORNINGS) {
                (false, true, text)
            } else {
                (false, false, text)
            };
            let none = lamp_msgs::Unprompted::default();
            let (text, hello, news, keep_light_on) = match text
                .strip_prefix(TYPED)
                .and_then(|text| text.split_once("] "))
            {
                Some((mark, text)) => {
                    let (limits, hello, news, light) = limits(mark);
                    let only = |about| if about { limits } else { none };
                    (text, only(hello), only(news), light)
                }
                None => (text, none, none, false),
            };
            Some(Rule {
                id: entry.get("id").and_then(valid_id)?,
                at_unix_ms: entry.get("at_unix_ms")?.as_u64()?,
                text: text.to_owned(),
                quiet,
                quiet_mornings,
                hello,
                news,
                keep_light_on,
            })
        })
        .collect()
}

/// What to tell the model for the memory's answer, and whether the rules
/// changed. `None` for an answer that is not one to that request. `when`
/// says how long ago a moment was, the way a person would.
pub fn answer_for(
    name: &str,
    answer: &Value,
    when: impl Fn(u64) -> String,
) -> Option<(Value, bool)> {
    if answer["ok"] != true {
        return match answer.get("error").and_then(Value::as_str)? {
            "full" if name == KEEP_RULE => Some((
                json!({"error": "I am keeping as many rules as I can. Ask which one they want me to drop."}),
                false,
            )),
            "empty" => Some((
                json!({"error": "There was no rule in that for me to keep."}),
                false,
            )),
            _ => None,
        };
    }
    Some(match name {
        KEEP_RULE => {
            answer.get("id").and_then(valid_id)?;
            (
                json!({"kept_rule": true, "note": "Follow it from now on, with them, without being reminded."}),
                true,
            )
        }
        // `forget {id}` answers how many records went: a count, nothing else.
        FORGET_RULE => match answer.get("removed")?.as_u64()? {
            0 => (which(), false),
            _ => (
                json!({"forgotten": true, "note": "Stop following it from now on."}),
                true,
            ),
        },
        _ => {
            let listed: Vec<Value> = rules(answer)?
                .iter()
                .map(|rule| json!({"number": rule.id, "rule": rule.text, "when": when(rule.at_unix_ms)}))
                .collect();
            if listed.is_empty() {
                (
                    json!({"rules": [], "note": "They have not asked you for anything like that."}),
                    false,
                )
            } else {
                (
                    json!({"rules": listed, "note": "The numbers are for forget_rule only: never say them."}),
                    false,
                )
            }
        }
    })
}

/// The memory's request for every rule of a scope.
pub fn read_request(scope: &str) -> Value {
    json!({"op":"kept","person":scope,"max_bytes":READ_BYTES})
}

/// Whether one more rule may be kept beside these, or what to tell the
/// model: nothing is dropped to make room.
pub fn admit(kept: &[Rule]) -> Result<(), Value> {
    if kept.len() >= MOST_RULES {
        return Err(json!({"kept_rule": false, "note": format!(
            "Nothing was kept: I already keep {MOST_RULES} rules for them, the most I keep. Tell them, and ask which rule they want me to drop; then call forget_rule with its number and keep the new one."
        )}));
    }
    Ok(())
}

/// The room's rules, and the known person's when someone is, read before
/// `until`. `None` when either could not be had.
#[allow(clippy::type_complexity)]
pub fn read(
    ask: impl Fn(&Value, Duration) -> Option<Value>,
    person: Option<&str>,
    until: Instant,
) -> Option<(Vec<Rule>, Option<Vec<Rule>>)> {
    let one = |person: Option<&str>| {
        let left = until.checked_duration_since(Instant::now())?;
        rules(&ask(&read_request(&scope(person)), left)?)
    };
    let room = one(None)?;
    let theirs = match person {
        Some(person) => Some(one(Some(person))?),
        None => None,
    };
    Some((room, theirs))
}

/// What a session is told of the rules: nothing at all when there are
/// none. The rules `forget_rule` acts on (theirs, or the room's when
/// nobody is known) are numbered.
pub fn note(room: &[Rule], theirs: Option<&[Rule]>) -> String {
    if room.is_empty() && theirs.is_none_or(<[Rule]>::is_empty) {
        return String::new();
    }
    let mut note = String::from(
        "\n\nHow you were asked to behave\nThese are corrections and standing preferences people gave you about \
         how you behave with them. Follow them without being reminded and do not mention them unless asked; if \
         someone asks what they asked of you, call list_rules. A number in brackets is for forget_rule only: never \
         say it.\n",
    );
    let lines = |note: &mut String, rules: &[Rule], numbered: bool| {
        for rule in rules {
            note.push_str("- ");
            if numbered {
                note.push_str(&format!("({}) ", rule.id));
            }
            note.push_str(&rule.text);
            note.push('\n');
        }
    };
    match theirs {
        None => {
            note.push_str("Asked in this room:\n");
            lines(&mut note, room, true);
        }
        Some(theirs) => {
            if !room.is_empty() {
                note.push_str("Asked in this room by someone unknown:\n");
                lines(&mut note, room, false);
            }
            if !theirs.is_empty() {
                note.push_str(
                    "Asked by the person at the lamp as this conversation began, whose face you know:\n",
                );
                lines(&mut note, theirs, true);
            }
        }
    }
    note
}

/// The typed fact for the policy: the room's rules and theirs together.
pub fn facts(person: Option<&str>, room: &[Rule], theirs: &[Rule]) -> lamp_msgs::Rules {
    let all = || room.iter().chain(theirs);
    lamp_msgs::Rules {
        person: person.unwrap_or("everyone").to_owned(),
        quiet: all().any(|rule| rule.quiet),
        quiet_mornings: all().any(|rule| rule.quiet || rule.quiet_mornings),
        rules: u32::try_from(all().count()).unwrap_or(u32::MAX),
        hello: all()
            .map(|rule| rule.hello)
            .fold(Default::default(), strictest),
        news: all()
            .map(|rule| rule.news)
            .fold(Default::default(), strictest),
        // The room's (#1677): the light is one thing everyone at the lamp
        // shares, and it goes out only when nobody is there.
        keep_light_on: room.iter().any(|rule| rule.keep_light_on),
    }
}

/// Whether `call` keeps a rule about the light: kept in the room's scope,
/// whoever says it (#1677, the first builder on 19 Oct).
pub fn keeps_light(call: &ToolCall) -> bool {
    call.name == KEEP_RULE && call.args.get("about").and_then(Value::as_str) == Some("light")
}

/// Whether `call` takes back keeping the light on: every such rule of the
/// room's, without a number (the room's rules are not numbered for a known
/// person).
pub fn forgets_light(call: &ToolCall) -> bool {
    call.name == FORGET_RULE
        && call.args.get("number").is_none_or(Value::is_null)
        && call.args.get("about").and_then(Value::as_str) == Some("light")
}

/// Say the fact on `memory/rules`, kept until the next, from a thread of its
/// own: the bus's file work never holds up a session's note or a call.
/// Coalescing: only the newest fact waits, and an older one not yet said is
/// replaced. A bus that cannot be used is said once and changes nothing
/// else.
pub fn publish(facts: &lamp_msgs::Rules) {
    // A test's own switch ([`set_on_for_test`]) never says anything on the
    // process's one bus, which another test may be reading.
    if on_for_test() {
        return;
    }
    let Some(root) = BUS.get() else { return };
    SAID.store(true, std::sync::atomic::Ordering::Release);
    // Held only to put or take one fact, never across the bus's work: a
    // newer fact must not be dropped because the publisher is taking one.
    match WAITING.lock() {
        Ok(mut waiting) => *waiting = Some(facts.clone()),
        Err(_) => return,
    }
    let wake = WAKE.get_or_init(|| {
        let (wake, woken) = sync_channel::<()>(1);
        let root = root.clone();
        let spawned = std::thread::Builder::new()
            .name("lamp-voice-rules-bus".into())
            .spawn(move || publisher(&root, &woken));
        if spawned.is_err() {
            eprintln!("lamp-voice provider: no thread for the rules' bus; they are not published");
        }
        wake
    });
    let _ = wake.try_send(());
}

/// Whether any fact has been given to the publisher since this process
/// started.
static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a fact has been said since the start.
pub fn said() -> bool {
    SAID.load(std::sync::atomic::Ordering::Acquire)
}

/// The room's fact read at the voice's start (#1747): said only if no
/// session or rule has said one first, so a newer fact with a known
/// person's limits is never replaced by the room's alone.
pub fn publish_first(facts: &lamp_msgs::Rules) {
    if on_for_test() {
        return;
    }
    if !said() {
        publish(facts);
    }
}

/// The newest fact not yet said, and the publisher thread's wake-up.
static WAITING: Mutex<Option<lamp_msgs::Rules>> = Mutex::new(None);
static WAKE: OnceLock<SyncSender<()>> = OnceLock::new();

fn publisher(root: &Path, woken: &Receiver<()>) {
    let mut out: Option<lamp_bus::Publisher> = None;
    let mut said = false;
    while woken.recv().is_ok() {
        let Some(facts) = WAITING.lock().ok().and_then(|mut waiting| waiting.take()) else {
            continue;
        };
        // A test holds this to stand for a bus whose file work hangs.
        #[cfg(test)]
        let _stalled = tests::STALL.lock();
        if out.is_none() {
            out = lamp_bus::Bus::new(root)
                .and_then(|bus| {
                    bus.publisher_declared(
                        lamp_msgs::Rules::TOPIC,
                        &lamp_msgs::Rules::declaration(),
                        crate::bus_out::SOURCE,
                    )
                })
                .map_err(|error| {
                    if !std::mem::replace(&mut said, true) {
                        eprintln!(
                            "lamp-voice provider: rules are not published on the bus: {error}"
                        );
                    }
                })
                .ok();
        }
        if let Some(publisher) = out.as_mut() {
            let _ = publisher.publish_json(0, &facts);
        }
        let (count, changed) = &HANDLED;
        if let Ok(mut count) = count.lock() {
            *count += 1;
            changed.notify_all();
        }
    }
}

/// How many facts the publisher thread has handled, published or not: what
/// a test waits on instead of the clock.
static HANDLED: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

#[cfg(test)]
fn handled() -> u64 {
    *HANDLED.0.lock().unwrap()
}

/// Wait until the publisher thread has handled a fact after `seen`. The
/// timeout only guards against a hung test; it decides nothing.
#[cfg(test)]
fn wait_handled(seen: u64) {
    let (count, changed) = &HANDLED;
    let (count, timeout) = changed
        .wait_timeout_while(count.lock().unwrap(), Duration::from_secs(60), |count| {
            *count <= seen
        })
        .unwrap();
    assert!(
        !timeout.timed_out(),
        "the publisher handled nothing: {count}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: name.into(),
            args,
        }
    }

    fn rule(id: u64, text: &str) -> Rule {
        rules(&json!({"ok":true,"entries":[{"id":id,"at_unix_ms":1,"text":text}]})).unwrap()[0]
            .clone()
    }

    /// lamp-52e6, PR #822: "from now on please keep your answers to one
    /// short sentence" was obeyed in the session and never kept. The
    /// declaration names the standing forms, and leaves one-off requests out.
    #[test]
    fn keep_rule_is_declared_for_standing_corrections_and_not_one_off_requests() {
        let tools = tools();
        for tool in &tools {
            tool.validate().unwrap();
        }
        let keep = &tools[0].description;
        for form in [
            "from now on",
            "'always'",
            "'never'",
            "'stop ...'",
            "'don't call me ...'",
            "one short sentence",
            "not enough",
            "one-off request",
        ] {
            assert!(keep.contains(form), "{form}");
        }
        let forget = &tools[1];
        assert!(forget.description.contains("list_rules"));
        // A number, or about: light (#1677), so neither is required alone.
        let parameters = forget.parameters.as_ref().unwrap();
        assert_eq!(parameters["required"], Value::Null);
        assert_eq!(parameters["properties"]["about"]["enum"], json!(["light"]));
    }

    /// #1677: "keep the light on" is a typed rule about the light, never
    /// only; it is the room's, kept there whoever says it, and the fact says
    /// it from the room's rules alone. "You can turn your light off again"
    /// takes it back without a number.
    #[test]
    fn keep_the_light_on_is_the_rooms_rule_and_taken_back_without_a_number() {
        let keep = |args| request(&call(KEEP_RULE, args), 7, "r-everyone");
        let kept = keep(json!({"rule":"Keep the light on.","about":"light","never":true})).unwrap();
        assert_eq!(kept["text"], "[unprompted light never] Keep the light on.");
        assert!(keeps_light(&call(
            KEEP_RULE,
            json!({"rule":"x","about":"light","never":true})
        )));
        assert!(!keeps_light(&call(
            KEEP_RULE,
            json!({"rule":"x","about":"news","never":true})
        )));
        // Never only: a time or talking is refused with words to say.
        for args in [
            json!({"rule":"x","about":"light","never":true,"not_before":"09:00"}),
            json!({"rule":"x","about":"light","not_from":"22:00"}),
            json!({"rule":"x","about":"light","never":true,"not_while_talking":true}),
        ] {
            let told = keep(args.clone()).unwrap_err();
            assert!(
                told["error"].as_str().unwrap().contains("never only"),
                "{args}"
            );
        }
        // Read back: the room's rule sets the fact; the same rule among a
        // person's (kept before this) does not, and greeting rules don't.
        let light = rule(4, "[unprompted light never] Keep the light on.");
        assert!(light.keep_light_on);
        assert_eq!(light.text, "Keep the light on.");
        assert!(!rule(5, "[unprompted greeting never] Don't greet me.").keep_light_on);
        assert!(facts(Some("p-1"), std::slice::from_ref(&light), &[]).keep_light_on);
        assert!(facts(None, std::slice::from_ref(&light), &[]).keep_light_on);
        assert!(!facts(Some("p-1"), &[], std::slice::from_ref(&light)).keep_light_on);
        // Taken back with about: light and no number; a number is as ever.
        assert!(forgets_light(&call(FORGET_RULE, json!({"about":"light"}))));
        assert!(!forgets_light(&call(
            FORGET_RULE,
            json!({"about":"light","number":3})
        )));
        assert!(!forgets_light(&call(FORGET_RULE, json!({"number":3}))));
    }

    #[test]
    fn a_rule_is_kept_beside_the_persons_memory_with_its_mark() {
        assert_eq!(scope(Some("p-be9688998f085098")), "r-be9688998f085098");
        assert_eq!(scope(None), "r-everyone");
        let keep = |args| request(&call(KEEP_RULE, args), 7, "r-x");
        assert_eq!(
            keep(json!({"rule":" Do not tell them the weather. ","speaking_up":"not_first_in_mornings"})).unwrap(),
            json!({"op":"remember","person":"r-x","at_unix_ms":7,"text":"[quiet_mornings] Do not tell them the weather."})
        );
        assert_eq!(
            keep(json!({"rule":"Call them Minh."})).unwrap()["text"],
            "Call them Minh."
        );
        assert!(keep(json!({"rule":" ... "})).is_err());
        assert!(keep(json!({"rule":"x".repeat(201)})).is_err());
        assert_eq!(
            request(&call(FORGET_RULE, json!({"number":3})), 7, "r-x").unwrap(),
            json!({"op":"forget","person":"r-x","id":3})
        );
        // No number, or not a rule's: the memory is asked nothing.
        for args in [json!({}), json!({"number":0}), json!({"number":"first"})] {
            let told = request(&call(FORGET_RULE, args), 7, "r-x").unwrap_err();
            assert_eq!(told["forgotten"], false);
            assert!(told["note"].as_str().unwrap().contains("list_rules"));
        }
        assert_eq!(
            request(&call(LIST_RULES, json!({})), 7, "r-x").unwrap(),
            json!({"op":"kept","person":"r-x","max_bytes":16384})
        );
    }

    #[test]
    fn the_mark_becomes_the_typed_fact_and_never_reaches_the_model() {
        let morning = rule(1, "[quiet_mornings] Do not speak first in the morning.");
        assert_eq!(morning.text, "Do not speak first in the morning.");
        let name = rule(2, "Call them Minh.");
        let fact = facts(Some("p-1"), &[name], &[morning]);
        assert_eq!(
            fact,
            lamp_msgs::Rules {
                person: "p-1".into(),
                quiet: false,
                quiet_mornings: true,
                rules: 2,
                hello: Default::default(),
                news: Default::default(),
                keep_light_on: false,
            }
        );
        let never = rule(3, "[quiet] Never speak first.");
        let fact = facts(None, &[never], &[]);
        assert!(fact.quiet && fact.quiet_mornings && fact.person == "everyone");
        let (listed, changed) = answer_for(
            LIST_RULES,
            &json!({"ok":true,"entries":[{"id":1,"at_unix_ms":1,"text":"[quiet_mornings] Do not speak first in the morning."}]}),
            |_| "today".into(),
        )
        .unwrap();
        assert!(!changed);
        assert_eq!(
            listed["rules"],
            json!([{"number":1,"rule":"Do not speak first in the morning.","when":"today"}])
        );
    }

    /// #813: "don't greet me before nine" and "no news while I'm on a call"
    /// become the policy's hard rules, kept as a mark the model never sees.
    #[test]
    fn a_rule_about_greeting_or_news_becomes_typed_limits_for_the_policy() {
        let keep = |args: Value| request(&call(KEEP_RULE, args), 7, "r-x");
        let text = |request: Value| request["text"].as_str().unwrap().to_owned();
        let greet = text(
            keep(json!({"rule":"Do not greet them before nine.","about":"greeting","not_before":"09:00"}))
                .unwrap(),
        );
        assert_eq!(
            greet,
            "[unprompted greeting not_before=540] Do not greet them before nine."
        );
        let call_news = text(
            keep(json!({"rule":"No news while they are on a call.","about":"news","not_while_talking":true}))
                .unwrap(),
        );
        let late = text(
            keep(json!({"rule":"Nothing unasked after ten at night.","about":"both","not_from":"22:00","speaking_up":"not_first_in_mornings"}))
                .unwrap(),
        );
        assert!(late.starts_with("[quiet_mornings] [unprompted both not_from=1320] "));
        let (greet, call_news, late) = (rule(1, &greet), rule(2, &call_news), rule(3, &late));
        assert_eq!(greet.text, "Do not greet them before nine.");
        assert_eq!(late.text, "Nothing unasked after ten at night.");
        assert!(late.quiet_mornings);
        let fact = facts(Some("p-1"), &[late], &[greet, call_news]);
        assert_eq!(
            fact.hello,
            lamp_msgs::Unprompted {
                not_before_minute: Some(540),
                not_from_minute: Some(1320),
                ..Default::default()
            }
        );
        assert_eq!(
            fact.news,
            lamp_msgs::Unprompted {
                not_from_minute: Some(1320),
                not_while_talking: true,
                ..Default::default()
            }
        );
        // The strictest of two windows; a word a newer voice wrote is left out.
        let earlier = rule(
            4,
            "[unprompted greeting not_from=1260 louder] Quiet from nine at night.",
        );
        assert_eq!(
            facts(
                None,
                &[earlier],
                &[rule(5, "[unprompted greeting never] Never greet.")]
            )
            .hello,
            lamp_msgs::Unprompted {
                never: true,
                not_from_minute: Some(1260),
                ..Default::default()
            }
        );
        // A rule with no limit is told to sessions and enforces nothing.
        // Explicit false or null is no limit, as before.
        let unset = keep(
            json!({"rule":"Call them Minh.","about":"news","never":false,"not_while_talking":null}),
        );
        assert_eq!(unset.unwrap()["text"], "Call them Minh.");
        let plain = keep(json!({"rule":"Call them Minh.","about":"greeting"})).unwrap();
        assert_eq!(plain["text"], "Call them Minh.");
        let fact = facts(None, &[rule(6, "Call them Minh.")], &[]);
        assert_eq!((fact.hello, fact.news), Default::default());
        // A limit that cannot be read is refused, never kept half-typed.
        for bad in [
            json!({"rule":"r","not_before":"09:00"}),
            json!({"rule":"r","about":"weather","never":true}),
            json!({"rule":"r","about":"news","not_before":"9"}),
            json!({"rule":"r","about":"news","not_before":"24:00"}),
            json!({"rule":"r","about":"news","not_before":"09:5"}),
            json!({"rule":"r","about":"news","not_before":"+1:+2"}),
            json!({"rule":"r","about":"news","not_before":"-0:00"}),
            json!({"rule":"r","about":"news","not_before":" 9:00"}),
            json!({"rule":"r","about":"news","not_before":"09: 00"}),
            json!({"rule":"r","about":"news","not_from":"23:60"}),
            json!({"rule":"r","about":"news","not_from":"123:00"}),
            json!({"rule":"r","about":"greeting","never":"true"}),
            json!({"rule":"r","about":"news","not_while_talking":1}),
            json!({"rule":"r","about":"greeting","never":"true","not_before":"09:00"}),
            json!({"rule":"r","about":"news","not_before":540}),
            json!({"rule":"r","about":"news","not_before":"09:00","not_from":"09:00"}),
        ] {
            assert!(keep(bad.clone()).unwrap_err()["error"].is_string(), "{bad}");
        }
    }

    #[test]
    fn a_session_is_told_the_rooms_rules_and_only_the_known_persons_numbered() {
        assert_eq!(note(&[], None), "");
        assert_eq!(note(&[], Some(&[])), "");
        let room = [rule(4, "Answer in short sentences.")];
        let theirs = [rule(9, "Do not call them sir.")];
        let told = note(&room, Some(&theirs));
        assert!(told.contains("- Answer in short sentences.\n"), "{told}");
        assert!(told.contains("- (9) Do not call them sir.\n"), "{told}");
        assert!(note(&room, None).contains("- (4) Answer in short sentences.\n"));
    }

    /// What the publisher thread waits on while a test stands for a bus
    /// whose file work hangs (a declaration FIFO nobody writes, on the lamp).
    pub(super) static STALL: Mutex<()> = Mutex::new(());

    /// The only test that names a bus: [`BUS`] is set once per process.
    /// A bus that hangs does not hold up a session's note; once it moves the
    /// fact is kept there. Nothing here waits on the clock but a hang guard.
    #[test]
    fn the_fact_is_kept_on_the_bus_and_a_stalled_bus_never_holds_up_a_session() {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::net::UnixListener,
        };
        let root = PathBuf::from(format!("/tmp/lvr-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // A memory with one room rule and no session note, answering every
        // request: a failed open is closed by the client in the background.
        let socket = root.join("m.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .ok();
                let answer = if line.contains("\"kept\"") {
                    json!({"ok":true,"entries":[{"id":1,"at_unix_ms":1,"text":"[quiet] Never speak first."}]})
                } else {
                    json!({"ok":false,"error":"storage"})
                };
                let _ = stream.write_all(format!("{answer}\n").as_bytes());
            }
        });
        // The bus taken with the memory: the room's rules are said with no
        // session at all (#1747), as after a reboot before anyone speaks.
        bus_on(&root, &socket);
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let at_start = facts(None, &[rule(1, "[quiet] Never speak first.")], &[]);
        let mut said_at_start = None;
        for _ in 0..500 {
            said_at_start = bus
                .latest(lamp_msgs::Rules::TOPIC)
                .ok()
                .flatten()
                .and_then(|message| message.parse::<lamp_msgs::Rules>().ok());
            if said_at_start.is_some() {
                break;
            }
            // A hang guard only: the start's thread publishes once.
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(said_at_start, Some(at_start));
        let client = crate::remember::DaysClient::new(socket).unwrap();
        let stalled = STALL.lock().unwrap();
        let before = handled();
        let started = Instant::now();
        let note = client.told_for_session(1, None, true);
        let took = started.elapsed();
        assert!(note.contains("- (1) Never speak first.\n"), "{note}");
        assert!(took < Duration::from_millis(300), "{took:?}");
        // The publisher is still held: nothing it was given is done.
        assert_eq!(handled(), before);
        drop(stalled);
        let fact = facts(
            Some("p-1"),
            &[],
            &[rule(
                1,
                "[quiet_mornings] Do not speak first in the morning.",
            )],
        );
        // Each try waits for the publisher to handle a fact, not for the
        // clock; another test's fact can be the one handled, so a few tries.
        let mut kept = None;
        for _ in 0..20 {
            let seen = handled();
            publish(&fact);
            wait_handled(seen);
            kept = bus
                .latest(lamp_msgs::Rules::TOPIC)
                .ok()
                .flatten()
                .and_then(|message| message.parse::<lamp_msgs::Rules>().ok());
            if kept.as_ref() == Some(&fact) {
                break;
            }
        }
        assert_eq!(kept, Some(fact));
        drop(client);
        // The client's background closes may still be writing beside it.
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn forgetting_says_whether_a_rule_went() {
        let forgot = |removed| {
            answer_for(FORGET_RULE, &json!({"ok":true,"removed":removed}), |_| {
                String::new()
            })
        };
        assert_eq!(forgot(json!(1)).unwrap().0["forgotten"], true);
        assert!(forgot(json!(1)).unwrap().1);
        assert_eq!(forgot(json!(0)).unwrap().0["forgotten"], false);
        // Anything but a count is a malformed acknowledgement.
        for removed in [
            json!(null),
            json!(false),
            json!(true),
            json!("1"),
            json!(-1),
            json!(1.5),
            json!({"kind":"kept","id":2,"at_unix_ms":1}),
        ] {
            assert!(forgot(removed.clone()).is_none(), "{removed}");
        }
        let kept = |id| answer_for(KEEP_RULE, &json!({"ok":true,"id":id}), |_| String::new());
        assert_eq!(kept(json!(1)).unwrap().0["kept_rule"], true);
        for id in [
            json!(0),
            json!(u64::MAX),
            json!(-1),
            json!("1"),
            json!(null),
        ] {
            assert!(kept(id.clone()).is_none(), "{id}");
        }
        assert!(
            answer_for(KEEP_RULE, &json!({"ok":false,"error":"storage"}), |_| {
                String::new()
            })
            .is_none()
        );
    }
}
