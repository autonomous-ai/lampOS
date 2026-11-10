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
    let services = services(reads);
    let triggers = made_skill::TRIGGER_MEANINGS;
    format!(
        "You write skills for a lamp. Answer exactly as the request says, with the two fenced \
         blocks and nothing else. Use only the keys and values the request names, written as \
         in this example, which the lamp's check takes:\n\n{EXAMPLE}\n\nThe example shows \
         the form only: never answer with it. A skill can't keep time by itself: a reminder at \
         a time or every so often is the lamp's own timer, not a skill. Make what the request \
         asks with the light, the head or a short line it says, or a read as below, on the \
         lamp's resting or someone being there. To speak, a skill has speak = true in \
         [permissions] and a [[do]] step say = \"greet them warmly\": a few words of what to \
         say, one clause, no quotes, which the lamp says in its own words. When it runs is one \
         [trigger] table (never [[trigger]], never the faces' topic): {triggers} A skill can't hear what is said: a request \
         that needs words or a sound heard, or anything else a skill here can't sense or do, \
         is answered with one line and no blocks: {cannot} then one short sentence said to \
         the person, in plain words, why, ending with what to ask instead when there is one; \
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
fn services(reads: bool) -> String {
    let cannot = made_skill::CANNOT;
    let ask_me = "You can ask me for it yourself any time.";
    if !reads {
        return format!(
            "A skill can't use the person's mail, calendar or any other linked service yet, \
             even one already linked: a request that needs one is answered with one line and \
             no blocks: {cannot} Not possible yet: a skill can't use your mail, calendar or \
             other services, even ones already linked. {ask_me} Never tell them to link one."
        );
    }
    let steps = made_skill::read_steps().join("\n- ");
    let unreadable = made_skill::unreadable_names().join(", ");
    format!(
        "A skill may read at most one of the person's linked services, and only read it: it \
         never sends, writes, posts, books, schedules, deletes or changes anything in a \
         service. A request to be read one of these is a skill with [permissions] only \
         speak = true, services = {{ CODE = \"read\" }} and its topics; its trigger's during \
         only for a time the request names; and exactly one [[do]] step, the one shown here \
         for that service, filled in from the request, and no light or head step. The services a skill can read, each as name (CODE): step:\n- {steps}\n\
         A request to send, write or change anything in a service, to read two or more \
         services, or to read one not listed here ({unreadable}, or any other) is answered \
         with one line and no blocks: {cannot} then one short sentence saying what a skill \
         can do instead (read one linked service), ending \"{ask_me}\" Never tell them to \
         link a service."
    )
}

/// The function the model is given with the switch: the lamp makes it
/// itself, at once, and puts it on as soon as it is kept.
pub fn tool() -> lamp_gemini::Tool {
    lamp_gemini::Tool {
        name: made_skill::MAKE.into(),
        description: "Make a new skill for the lamp: something the lamp then does by itself with \
             its light, its head or a short line it says, at a time of day, when someone comes or goes, or when it is \
             resting (\"make me a skill that dims the light at 10 pm\"). Call it only when the \
             person asks for a skill to be made. In the same breath, before you call it, say in a \
             few words that you'll make it now. The skill is made, checked, kept and put on the \
             lamp at once: then say exactly the line you are told, nothing more."
            .into(),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                "request": {
                    "type": "string",
                    "description": "What the skill should do and when, in the person's own words, nothing else."
                }
            },
            "required": ["request"]
        })),
        silent: false,
    }
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
        // a broken connection by another name.
        net::Error::Protocol(what) => *what == CUT,
        net::Error::Url => false,
    }
}

/// The model's answer to `brief`, its text whole, asked once more when
/// [`again`] says so.
pub async fn written(chat: &Chat, brief: &str) -> net::Result<String> {
    // An answer whose last fence never closed was cut short too (17:48:
    // "its answer was cut off"): asked once more, as a broken stream.
    let once = write_once(chat, brief).await.and_then(|text| {
        // A `cannot:` answer is whole with no fence at all.
        if fences_closed(&text) || text.trim_start().starts_with(made_skill::CANNOT) {
            Ok(text)
        } else {
            Err(net::Error::Protocol(CUT))
        }
    });
    match once {
        Err(error) if again(&error) => {
            eprintln!(
                "lamp-voice: a skill made here: the model's answer failed ({error}); asked once more"
            );
            write_once(chat, brief).await
        }
        answer => answer,
    }
}

/// The chat client's word for a stream that ended before its end.
const CUT: &str = "chat stream cut";

/// Whether every fenced block the answer opened is closed: an even,
/// non-zero count of fences.
fn fences_closed(text: &str) -> bool {
    let fences = text
        .lines()
        .filter(|line| line.trim_start().starts_with("```"))
        .count();
    fences > 0 && fences % 2 == 0
}

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
    make_with(request, keeping, chat, now_unix_ms, || {
        crate::services::linked_codes(std::path::Path::new(crate::services::FOLDER))
    })
}

/// [`make`], what is linked given by `linked`.
pub fn make_with(
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
    let made = make_kept(
        request,
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
    told(made, on, needs.as_deref())
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
    let (made, line) = made_and_journaled(request, brief, keeping, chat, from);
    eprintln!("{line}");
    made
}

/// [`make_kept`]'s make, and the journal's line for it ([`kept_line`]).
fn made_and_journaled(
    request: &str,
    brief: String,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    from: &str,
) -> (Result<Made, Refused>, String) {
    let began = std::time::Instant::now();
    let chat = Arc::new(chat);
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
            Ok(text) => made_skill::keep(&text, false, from, keeping),
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
    match made {
        Ok(made) => {
            let link = needs
                .and_then(made_skill::needs_link_ask)
                .map(|ask| format!(" {ask}"))
                .unwrap_or_default();
            let line = match &on {
                Ok(()) => made_skill::on_line(&made),
                Err(why) => made_skill::not_on_line(why),
            };
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
            "next": made_skill::not_made_in_their_language(&made_skill::not_made_line(&why))
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

pub fn answer(call: &lamp_gemini::ToolCall, heard: bool) -> Value {
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
    make(request_words, &keeping, own_chat, now_unix_ms)
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
            answer(&call, false),
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

    /// Only this fixture's authenticated chat requests consume scripted replies.
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
                    let reply = if status == 200 {
                        let (first, rest) = text.split_at(text.len() / 2);
                        let chunk = |words: &str| format!("data: {}\n\n", json!({"choices": [{"delta": {"content": words}}]}));
                        format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{}{}data: [DONE]\n\n", chunk(first), chunk(rest))
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
                    "I made evening dim, and it's on: when the lamp goes quiet, between 22:00 and 22:30, it dims the light a step, at most once every 12 hours, using its light and whether it is talking."
                )
            )
        );
        assert!(!told["next"].as_str().unwrap().contains("Say exactly"));
        assert!(!told.to_string().contains("Shall I"), "{told}");
        assert!(told["uses"].as_str().unwrap().contains("light"), "{told}");
        let entry = &made_skill::entries(&keeping.directory)[0];
        assert_eq!(entry["on"], true);
        assert_eq!(asked.join().unwrap()["install_made"], entry["sha256"]);
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
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
             most once every 12 hours. The maker notes what it couldn't do: dimming slowly.",
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
                    "When it runs is one [trigger] table (never [[trigger]], never the faces' \
                     topic): {}",
                    made_skill::TRIGGER_MEANINGS
                )),
                "{system}"
            );
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

    /// lamp-9adf, 2 Nov 17:48: an answer cut before its last fence closed
    /// is asked once more, as a cut stream is; the whole one is kept.
    #[test]
    fn an_answer_cut_before_its_last_fence_is_asked_once_more() {
        let (root, keeping) = keeping("cut");
        let whole = good();
        let cut = whole[..whole.rfind("```").unwrap()].to_owned();
        assert!(!fences_closed(&cut) && fences_closed(&whole));
        assert!(again(&net::Error::Protocol(CUT)));
        let (url, served) = service(vec![(200, cut), (200, whole)]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
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
                reads: reads.map(str::to_owned),
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
        let answer = make_with(REQUEST, &keeping, chat_at(url), NOW, || None);
        assert_eq!(answer["made"], "evening-dim", "{answer}");
        assert!(answer.get("needs_link").is_none(), "{answer}");
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
