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
//! ([`made_skill::keep`]); the lamp says what it does and what it uses, and
//! asks [`made_skill::ASK_TO_PUT_ON`] in the same answer. The ask is kept
//! only when a yes comes right after an answer of the lamp's that said it
//! ([`made_skill::asked_in_answer`], the Reviewer-lead's P2 on #4914), so a
//! yes after a cut-in or another question puts nothing on; only the person's spoken yes
//! within [`made_skill::PUT_ON_WITHIN`] puts it on ([`made_skill::put_on`]),
//! by its SHA-256 alone. An answer that doesn't pass is said as not made,
//! and nothing is kept. The model only writes: it puts nothing on.
//!
//! [`made_skill::MAKE`] and [`made_skill::PUT_ON`] are the voice's own,
//! whether or not a tasks part is there: a computer's agent is never
//! handed a skill to make.
use crate::made_skill::{self, Keeping, Made, Refused};
use crate::second_provider::net::{self, Chat, ChatEvent, Message};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// The longest one answer is waited for, from the request to its end.
pub const WRITE_WITHIN: Duration = Duration::from_secs(30);
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

/// What the model is told it is, before the brief: the format, and a whole
/// skill the lamp's check takes.
fn system() -> String {
    let (cannot, left_out) = (made_skill::CANNOT, made_skill::LEFT_OUT);
    format!(
        "You write skills for a lamp. Answer exactly as the request says, with the two fenced \
         blocks and nothing else. Use only the keys and values the request names, written as \
         in this example, which the lamp's check takes:\n\n{EXAMPLE}\n\nThe example shows \
         the form only: never answer with it. A skill can't speak \
         or keep time by itself: a reminder at a time or every so often is the lamp's own \
         timer, not a skill. Make what the request asks with the light or the head, on the \
         lamp's resting or someone being there. A skill can't hear what is said: a request \
         that needs words or a sound heard, or anything else a skill here can't sense or do, \
         is answered with one line, {cannot} and why, and no blocks. Never stand something \
         else in for it, and add no time of day the request didn't name. When a part of the \
         request can't be done with these (dimming slowly, over a minute), make the rest and \
         add one line after the blocks: {left_out} what was left out, in a few words."
    )
}

/// The function the model is given with the switch: the lamp makes it
/// itself, at once.
pub fn tool() -> lamp_gemini::Tool {
    let ask = made_skill::ASK_TO_PUT_ON;
    let put_on = made_skill::PUT_ON;
    lamp_gemini::Tool {
        name: made_skill::MAKE.into(),
        description: format!(
            "Make a new skill for the lamp: something the lamp then does by itself with its \
             light or its head, at a time of day, when someone comes or goes, or when it is \
             resting (\"make me a skill that dims the light at 10 pm\"). Call it only when the \
             person asks for a skill to be made. In the same breath, before you call it, say in a \
             few words that you'll make it now. The skill is made, checked and kept, and you are \
             told what it does: then say so and ask \"{ask}\". It goes on the lamp only if the \
             person then says yes out loud ({put_on})."
        ),
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
/// model `chat` gives, kept in `keeping` as made at `now_unix_ms`, and
/// asked about then. Blocking: the model is asked on a thread of its own,
/// with a runtime of its own. What the model is told.
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
    told(made, needs.as_deref())
}

/// The skill for `request` (its `brief` made already) written by the
/// model `chat` gives and kept in `keeping` as made from `from`; a
/// refused one asked once more with the check's lines. Blocking: the model
/// is asked on a thread of its own. Shared by the voice's `make_skill` and
/// the app's typed ask (`crate::app_make`).
pub fn make_kept(
    request: &str,
    brief: String,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    from: &str,
) -> Result<Made, Refused> {
    let began = std::time::Instant::now();
    let chat = Arc::new(chat);
    let kept = |answer: Result<String, String>| match answer {
        // The maker's own example is never kept: refused as the check
        // refuses, so it goes back to the model once.
        Ok(text) if is_example(&text) => {
            eprintln!("lamp-voice: a skill made here: the model gave back the example");
            Err(Refused(
                "it gave back the lamp's own example, not what was asked".into(),
                Some(EXAMPLE_GIVEN_BACK.into()),
            ))
        }
        Ok(text) => made_skill::keep(&text, false, from, keeping),
        Err(why) => {
            eprintln!("lamp-voice: a skill made here: the model didn't answer: {why}");
            Err(Refused("the lamp's model didn't answer".into(), None))
        }
    };
    let mut made = kept(ask(Arc::clone(&chat), brief));
    // Refused by the lamp's check, or the example given back: its lines go
    // back to the model once (`made_skill::repair_brief`).
    if let Err(Refused(_, Some(lines))) = &made
        && let Ok(repair) = made_skill::repair_brief(request, lines)
    {
        eprintln!("lamp-voice: a skill made here: the check's lines go back to the model once");
        made = kept(ask(chat, repair));
    }
    eprintln!(
        "lamp-voice: a skill made here {} in {} ms",
        if made.is_ok() {
            "was kept"
        } else {
            "wasn't kept"
        },
        began.elapsed().as_millis()
    );
    made
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

/// What the model is told of a skill made here: kept, or not made, and
/// why, in plain words. It is not kept as asked here: only an answer that
/// said [`made_skill::ASK_TO_PUT_ON`] makes it asked, when the yes comes
/// ([`answer`], the Reviewer-lead's P2 on #4914).
///
/// `needs`: the linked services it reads that aren't linked now
/// ([`made_skill::needs_link`]): told as `needs_link`, and their sentence
/// said before the ask, so the ask stays last.
fn told(made: Result<Made, Refused>, needs: Option<&[Value]>) -> Value {
    match made {
        Ok(made) => {
            let link = needs
                .and_then(made_skill::needs_link_said)
                .map(|said| format!("then say exactly: \"{said}\" "))
                .unwrap_or_default();
            let mut told = json!({
                "made": made.name,
                "description": made.description,
                "uses": made.uses.spoken(),
                "next": format!(
                    "Say what it does as its description says, leaving nothing out, then what it \
                     uses, {link}then ask exactly: \"{}\" Only if they then say yes out loud, \
                     call {}.",
                    made_skill::ASK_TO_PUT_ON,
                    made_skill::PUT_ON
                )
            });
            if let Some(needs) = needs.filter(|needs| !needs.is_empty()) {
                told["needs_link"] = json!(needs);
            }
            told
        }
        Err(Refused(why, _)) => json!({
            "error": format!("the skill couldn't be made: {why}"),
            "next": "Tell them in one short sentence that you couldn't make that one, with no \
                     technical words, and that they can ask again in other words."
        }),
    }
}

/// Whether a yes at `now_unix_ms` is to the ask of the made skill waiting
/// for one: the lamp's last answer (`answered`, its words and how long ago)
/// came before the skill was asked, or said [`made_skill::ASK_TO_PUT_ON`].
/// With no skill waiting, true: `put_on` then says so itself.
fn yes_is_to_the_ask(
    directory: &std::path::Path,
    answered: Option<&(String, Duration)>,
    now_unix_ms: u64,
) -> bool {
    let Some((_, sha256)) = made_skill::waiting(directory, now_unix_ms) else {
        return true;
    };
    let Some(asked_at) = made_skill::entries(directory)
        .into_iter()
        .find(|entry| entry["sha256"] == sha256.as_str())
        .and_then(|entry| entry["offered_at_unix_ms"].as_u64())
    else {
        return true;
    };
    match answered {
        None => true,
        Some((said, ago)) => {
            made_skill::says_the_ask(said)
                || now_unix_ms.saturating_sub(ago.as_millis() as u64) < asked_at
        }
    }
}

/// What the model is told of a yes to another question: ask again, in the
/// ask's own words. Only an answer that said [`made_skill::ASK_TO_PUT_ON`]
/// makes the next yes one to it; an ask in other words ("shall I put the
/// red glow on the lamp?") was refused again at every yes (lamp-9adf,
/// 2 Nov 22:26).
fn not_to_the_ask() -> Value {
    json!({
        "error": "that yes was to another question, not to putting the skill on",
        "next": format!(
            "Don't put it on. If they still want it, ask once more, exactly: \"{}\"",
            made_skill::ASK_TO_PUT_ON
        )
    })
}

/// The just-made skill kept as asked when a yes comes at `now_unix_ms`
/// right after `answered`, the lamp's last answer and its age: only if
/// that answer said [`made_skill::ASK_TO_PUT_ON`] within
/// [`made_skill::PUT_ON_WITHIN`]. Dated at the yes, not at the answer: its
/// age runs from its first piece ("I'll make it now"), said before the
/// call kept the skill, and the ask comes later in the same answer (the
/// Reviewer-lead's second P2 on #4914). Whether one was kept so.
pub(crate) fn asked_at_yes(
    directory: &std::path::Path,
    answered: Option<(String, Duration)>,
    now_unix_ms: u64,
) -> std::io::Result<bool> {
    match answered {
        Some((lamp_said, ago))
            if ago <= made_skill::PUT_ON_WITHIN && made_skill::says_the_ask(&lamp_said) =>
        {
            match untold(directory, now_unix_ms).as_slice() {
                [(_, sha256)] => made_skill::offered(directory, sha256, now_unix_ms).map(|()| true),
                _ => Ok(false),
            }
        }
        _ => Ok(false),
    }
}

/// How long a skill made here and never asked about is still the one a
/// yes is to. One whose ask was cut (lamp-9adf, 2 Nov 17:08: a sound right
/// after it was kept ended the answer before the ask) would otherwise stay
/// untold for good, and as a second untold skill hold back every later one.
pub const UNTOLD_WITHIN: Duration = Duration::from_secs(10 * 60);

/// The skills made here never asked about, not on, kept after the newest
/// ask, and kept, or their ask last begun aloud (`ask_begun_at_unix_ms`,
/// an app's ask queued for a good moment: Mobile's HOLD on #4963), within
/// [`UNTOLD_WITHIN`] of `now_unix_ms`: their names and SHA-256s, oldest
/// first.
fn untold(directory: &std::path::Path, now_unix_ms: u64) -> Vec<(String, String)> {
    let entries = made_skill::entries(directory);
    let asked_at = entries
        .iter()
        .filter_map(|entry| entry["offered_at_unix_ms"].as_u64())
        .max()
        .unwrap_or(0);
    let since = now_unix_ms.saturating_sub(UNTOLD_WITHIN.as_millis() as u64);
    entries
        .iter()
        .filter(|entry| entry["offered_at_unix_ms"].as_u64().is_none() && entry["on"] != true)
        .filter(|entry| {
            entry["at_unix_ms"].as_u64().is_some_and(|kept_at| {
                let last = entry["ask_begun_at_unix_ms"]
                    .as_u64()
                    .map_or(kept_at, |begun| begun.max(kept_at));
                kept_at > asked_at && last >= since && kept_at <= now_unix_ms
            })
        })
        .filter_map(|entry| {
            Some((
                entry["name"].as_str()?.to_owned(),
                entry["sha256"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// The note a request is given while a skill made here was kept and never
/// asked about (its ask cut, or not said): the model then asks it, so a
/// yes has something to answer. None while one waits for its yes, or with
/// none or more than one such skill.
pub fn untold_note(directory: &std::path::Path, now_unix_ms: u64) -> Option<String> {
    if made_skill::waiting(directory, now_unix_ms).is_some() {
        return None;
    }
    match untold(directory, now_unix_ms).as_slice() {
        [(name, _)] => Some(format!(
            "(For you to know: you made the skill \"{name}\" for the person, but haven't asked them yet whether to put it on. When it fits, say in a few words what it does and ask exactly: \"{}\")",
            made_skill::ASK_TO_PUT_ON
        )),
        _ => None,
    }
}

/// A call of [`made_skill::MAKE`] or [`made_skill::PUT_ON`], in the
/// product's places: `request`'s words heard (`heard`) for a
/// yes, which must be said out loud.
pub fn answer(call: &lamp_gemini::ToolCall, request: u64, heard: bool) -> Value {
    let now_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    let Some(keeping) = Keeping::when(true) else {
        return crate::tools::unavailable();
    };
    if call.name == made_skill::PUT_ON {
        // The just-made skill is asked only if the lamp's own last answer
        // said the ask (#4880's rule, `made_skill::asked_in_answer`): a yes
        // after a cut-in or another question puts nothing on.
        let answered = crate::thread::THREAD
            .lock()
            .ok()
            .and_then(|thread| thread.answered_last_but(request, std::time::Instant::now()));
        if heard && let Err(error) = asked_at_yes(&keeping.directory, answered.clone(), now_unix_ms)
        {
            eprintln!("lamp-voice: a skill made here: its ask in an answer was not kept: {error}");
        }
        // A yes is to the ask only when the lamp has said nothing since it
        // asked, or its last answer was the ask: a yes to another question
        // ("For ten minutes?") puts nothing on (the Reviewer-lead's P2 on
        // #4971, as #4880's rule for the voice path).
        if !yes_is_to_the_ask(&keeping.directory, answered.as_ref(), now_unix_ms) {
            return not_to_the_ask();
        }
        let said = heard.then(|| {
            crate::thread::THREAD
                .lock()
                .ok()
                .and_then(|thread| thread.heard_for(request, std::time::Instant::now()))
                .unwrap_or_default()
        });
        return made_skill::put_on(
            &keeping.directory,
            std::path::Path::new(made_skill::MANAGE_SOCKET),
            said.as_deref(),
            now_unix_ms,
        );
    }
    let request_words = call.args["request"].as_str().unwrap_or_default();
    make(request_words, &keeping, own_chat, now_unix_ms)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

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
        };
        (root, keeping)
    }

    /// A chat service on a local port: each request in turn gets the next
    /// of `replies`, an HTTP status and, for a 200, the text streamed in
    /// two pieces. How many requests came.
    pub(crate) fn service(replies: Vec<(u16, String)>) -> (String, std::thread::JoinHandle<usize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let served = std::thread::spawn(move || {
            let mut count = 0;
            for (status, text) in replies {
                let (mut socket, _) = listener.accept().unwrap();
                count += 1;
                let mut request = Vec::new();
                let mut buffer = [0; 8192];
                // The head, then the body by its length.
                loop {
                    let n = socket.read(&mut buffer).unwrap();
                    request.extend_from_slice(&buffer[..n]);
                    let seen = String::from_utf8_lossy(&request).to_string();
                    if let Some((head, body)) = seen.split_once("\r\n\r\n") {
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|value| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            assert!(body.contains(made_skill::BRIEF_HEAD), "{body}");
                            assert!(!body.contains("\"tools\""), "{body}");
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let reply = if status == 200 {
                    let (first, rest) = text.split_at(text.len() / 2);
                    let chunk = |words: &str| {
                        format!(
                            "data: {}\n\n",
                            json!({"choices": [{"delta": {"content": words}}]})
                        )
                    };
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{}{}data: [DONE]\n\n",
                        chunk(first),
                        chunk(rest)
                    )
                } else {
                    format!(
                        "HTTP/1.1 {status} Nope\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    )
                };
                socket.write_all(reply.as_bytes()).unwrap();
            }
            count
        });
        (format!("http://{address}/v1/"), served)
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
        move || {
            Chat::with(&url, "m", "secret", Arc::new(Vec::new)).map_err(|error| error.to_string())
        }
    }

    const REQUEST: &str = "make me a skill that dims the light at 10 pm";
    const NOW: u64 = 1_800_000_000_000;

    fn waiting(directory: &Path) -> Option<(String, String)> {
        made_skill::waiting(directory, NOW + 1_000)
    }

    /// The model's two files are checked, packed and kept. Nothing waits
    /// for a yes until the lamp's own answer has said the ask: an answer
    /// without it (a cut-in, another question) keeps it unasked, and one
    /// with it makes it wait, by its SHA-256 (the Reviewer-lead's P2 on
    /// #4914).
    #[test]
    fn a_good_answer_is_kept_and_asked_about() {
        let (root, keeping) = keeping("good");
        let (url, served) = service(vec![(200, good())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains(made_skill::ASK_TO_PUT_ON)
        );
        assert!(told["uses"].as_str().unwrap().contains("light"), "{told}");
        assert_eq!(waiting(&keeping.directory), None);
        let ago = Duration::from_secs(2);
        assert!(
            !made_skill::asked_in_answer(
                &keeping.directory,
                "It dims the light at ten. Want a reminder too?",
                ago,
                NOW
            )
            .unwrap()
        );
        assert_eq!(waiting(&keeping.directory), None);
        let said = format!("It dims the light at ten. {}", made_skill::ASK_TO_PUT_ON);
        assert!(made_skill::asked_in_answer(&keeping.directory, &said, ago, NOW).unwrap());
        assert_eq!(waiting(&keeping.directory).unwrap().0, "evening-dim");
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A request no made skill can do (lamp-0c89's "when someone says good
    /// night", Mobile on #4710, 3 Nov): the model is told to answer
    /// `cannot:`, and that answer is asked once, never again as a cut one,
    /// and said as not made; nothing is kept.
    #[test]
    fn a_cannot_answer_is_asked_once_and_said_as_not_made() {
        assert!(system().contains("cannot: and why"));
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
            told["error"],
            "the skill couldn't be made: a skill can't do that: a skill can't hear what is said.",
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 1);
        assert!(untold(&keeping.directory, NOW).is_empty());
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

    /// The Reviewer-lead's second P2 on #4914, through the thread as the
    /// lamp says it: "I'll make it now", then the call keeps the skill, then
    /// the ask in the same answer, then a yes: the skill waits. An answer
    /// that asked something else keeps it unasked.
    #[test]
    fn the_ask_after_the_make_in_the_same_answer_keeps_the_skill_asked() {
        use crate::thread::Thread;
        for (ending, asked) in [
            (made_skill::ASK_TO_PUT_ON, true),
            ("Want a reminder too?", false),
        ] {
            let (root, keeping) = keeping(if asked { "thread-ask" } else { "thread-other" });
            let mut thread = Thread::new();
            let began = std::time::Instant::now();
            thread.person("Make me a skill that dims the light at 10 pm.", true, began);
            thread.lamp(1, "I'll make it now.", began);
            let (url, served) = service(vec![(200, good())]);
            let told = make(REQUEST, &keeping, chat_at(url), NOW);
            assert_eq!(told["made"], "evening-dim", "{told}");
            thread.lamp(1, &format!("It dims the light at ten. {ending}"), began);
            thread.person("Yes.", true, std::time::Instant::now());
            let now_unix_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let answered = thread.answered_last_but(2, std::time::Instant::now());
            assert_eq!(
                asked_at_yes(&keeping.directory, answered, now_unix_ms).unwrap(),
                asked
            );
            assert_eq!(
                made_skill::waiting(&keeping.directory, now_unix_ms).map(|(name, _)| name),
                asked.then(|| "evening-dim".to_owned())
            );
            assert_eq!(served.join().unwrap(), 1);
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// A lamp with no tasks part at all makes a skill by voice and puts it
    /// on: `make_skill` is the voice's own, the lamp's model writes it, the
    /// lamp's answer says the ask, and the plain spoken yes after it puts
    /// that skill on, by its SHA-256, through the manage helper.
    #[test]
    fn with_no_tasks_part_a_skill_is_made_and_put_on_by_voice() {
        use crate::tools::{Route, Settings, route};
        let settings = Settings {
            make_skill: true,
            ..Settings::default()
        };
        assert!(settings.tasks.is_none());
        assert_eq!(route(&settings, made_skill::MAKE), Route::MakeLocal);
        assert_eq!(route(&settings, made_skill::PUT_ON), Route::MakeLocal);
        let (root, keeping) = keeping("no-tasks");
        let (url, served) = service(vec![(200, good())]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW);
        assert_eq!(told["made"], "evening-dim", "{told}");
        assert_eq!(served.join().unwrap(), 1);
        let said = format!("It dims the light at ten. {}", made_skill::ASK_TO_PUT_ON);
        // Kept at the real time: the yes comes just after.
        let yes_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(
            asked_at_yes(
                &keeping.directory,
                Some((said, Duration::from_secs(2))),
                yes_at
            )
            .unwrap()
        );
        let sha256 = made_skill::waiting(&keeping.directory, yes_at).unwrap().1;
        let socket = root.join("manage.sock");
        let asked = manage_helper(&socket);
        let put = made_skill::put_on(&keeping.directory, &socket, Some("Yes."), yes_at);
        assert_eq!(put["put_on"], "evening-dim", "{put}");
        assert_eq!(asked.join().unwrap(), json!({"install_made": sha256}));
        assert_eq!(made_skill::waiting(&keeping.directory, yes_at), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An answer without the two files is said as not made; nothing is
    /// kept and nothing waits for a yes.
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
        assert_eq!(waiting(&keeping.directory), None);
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
        // Refused twice: said as not made, and asked no third time.
        let (url, served) = service(vec![(200, bad.clone()), (200, bad)]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW + 1);
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(served.join().unwrap(), 2);
        let _ = std::fs::remove_dir_all(&root);
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
        let (url, served) = service(vec![(200, bad.clone()), (200, bad)]);
        let told = make(REQUEST, &keeping, chat_at(url), NOW + 1);
        assert!(
            told["error"].as_str().unwrap().contains("can't be read"),
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lamp-9adf, 2 Nov 17:08: a skill whose ask was cut stays untold. A
    /// later one, asked and said yes to, is put on: the cut one, kept more
    /// than ten minutes before, no longer holds it back. While only one is
    /// untold, the model is noted to ask about it.
    #[test]
    fn a_skill_whose_ask_was_cut_never_holds_back_a_later_one() {
        let (root, keeping) = keeping("orphan");
        std::fs::create_dir_all(&keeping.directory).unwrap();
        let now = 1_800_000_000_000u64;
        let entry = |name: &str, sha: char, at: u64| {
            json!({"name": name, "sha256": sha.to_string().repeat(64),
                   "package": format!("{name}.tar"), "from": format!("local:{at}"),
                   "at_unix_ms": at, "description": "d"})
        };
        let write = |entries: &[Value]| {
            std::fs::write(
                keeping.directory.join(made_skill::INDEX),
                serde_json::to_vec(entries).unwrap(),
            )
            .unwrap()
        };
        let asked = format!("It glows cyan. {}", made_skill::ASK_TO_PUT_ON);
        let ago = Duration::from_secs(3);
        // One untold, kept a minute ago: the model is noted to ask.
        write(&[entry("cut-ask", 'a', now - 60_000)]);
        assert!(
            untold_note(&keeping.directory, now)
                .unwrap()
                .contains("\"cut-ask\"")
        );
        // Two untold within ten minutes: neither is asked, and no note.
        write(&[
            entry("cut-ask", 'a', now - 60_000),
            entry("water-glow", 'b', now - 5_000),
        ]);
        assert_eq!(untold_note(&keeping.directory, now), None);
        assert!(!asked_at_yes(&keeping.directory, Some((asked.clone(), ago)), now).unwrap());
        // The cut one kept eleven minutes ago: the later one is asked.
        write(&[
            entry("cut-ask", 'a', now - 11 * 60_000),
            entry("water-glow", 'b', now - 5_000),
        ]);
        assert!(asked_at_yes(&keeping.directory, Some((asked, ago)), now).unwrap());
        assert_eq!(
            made_skill::waiting(&keeping.directory, now).unwrap().0,
            "water-glow"
        );
        // Waiting for its yes: no untold note on top.
        assert_eq!(untold_note(&keeping.directory, now), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The Reviewer-lead's P2 on #4971: once a skill was asked (its ask
    /// said whole), a yes counts only while the lamp has said nothing since,
    /// or its last answer was the ask. A yes to "For ten minutes?" doesn't.
    #[test]
    fn a_yes_to_another_question_after_the_ask_puts_nothing_on() {
        let (root, keeping) = keeping("other-question");
        std::fs::create_dir_all(&keeping.directory).unwrap();
        let now = 1_800_000_000_000u64;
        let asked_at = now - 20_000;
        std::fs::write(
            keeping.directory.join(made_skill::INDEX),
            serde_json::to_vec(&[json!({"name": "glow", "sha256": "a".repeat(64),
                "package": "glow.tar", "from": "local:app:1:1", "at_unix_ms": now - 60_000,
                "offered_at_unix_ms": asked_at, "description": "d"})])
            .unwrap(),
        )
        .unwrap();
        let ask = format!("I made it. {}", made_skill::ASK_TO_PUT_ON);
        // No answer since the ask, or the last answer the ask itself: yes counts.
        assert!(yes_is_to_the_ask(&keeping.directory, None, now));
        assert!(yes_is_to_the_ask(
            &keeping.directory,
            Some(&(ask, Duration::from_secs(5))),
            now
        ));
        // An answer from before the ask: still the ask that was said last.
        assert!(yes_is_to_the_ask(
            &keeping.directory,
            Some(&("Sure.".to_owned(), Duration::from_secs(30))),
            now
        ));
        // Another question after the ask: a yes to it puts nothing on.
        assert!(!yes_is_to_the_ask(
            &keeping.directory,
            Some(&("For ten minutes?".to_owned(), Duration::from_secs(3))),
            now
        ));
        // The model is told to ask again in the ask's own words, and a yes
        // to that ask counts (lamp-9adf, 2 Nov 22:26: an ask in other words
        // was refused at every yes).
        let refused = not_to_the_ask();
        let again = refused["next"].as_str().unwrap();
        assert!(again.contains(made_skill::ASK_TO_PUT_ON), "{refused}");
        assert!(yes_is_to_the_ask(
            &keeping.directory,
            Some(&(made_skill::ASK_TO_PUT_ON.to_owned(), Duration::from_secs(2))),
            now
        ));
        // The ask run into a sentence, lower case, is the same ask
        // (lamp-9adf, 2 Nov 22:51).
        assert!(yes_is_to_the_ask(
            &keeping.directory,
            Some(&(
                "Just to be sure: shall I put it on the lamp?".to_owned(),
                Duration::from_secs(2)
            )),
            now
        ));
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
        let (url, served) = service(vec![(200, EXAMPLE.to_owned()), (200, EXAMPLE.to_owned())]);
        let told = make(
            "when I come back to my desk, glow warm",
            &keeping,
            chat_at(url),
            NOW,
        );
        assert!(told.get("made").is_none(), "{told}");
        assert_eq!(
            told["error"],
            "the skill couldn't be made: it gave back the lamp's own example, not what was asked",
            "{told}"
        );
        assert_eq!(served.join().unwrap(), 2);
        assert!(made_skill::entries(&keeping.directory).is_empty());
        assert!(untold(&keeping.directory, NOW).is_empty());
        assert_eq!(waiting(&keeping.directory), None);
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

    /// Off (`LAMP_MAKE_SKILL` unset), `make_skill` and `put_on_made_skill`
    /// are nobody's and never declared, with the tasks part or without. On,
    /// they are the voice's own, each declared once, with no tasks part at
    /// all as with one: a computer's agent never makes a skill.
    #[test]
    fn the_switch_alone_makes_the_two_functions_the_voices() {
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
            for name in [made_skill::MAKE, made_skill::PUT_ON] {
                assert_eq!(route(&off, name), Route::Nobody, "{name}");
                assert!(declared(&off).iter().all(|tool| tool.name != name));
            }
            let on = Settings {
                make_skill: true,
                tasks,
                ..Settings::default()
            };
            for name in [made_skill::MAKE, made_skill::PUT_ON] {
                assert_eq!(route(&on, name), Route::MakeLocal);
                assert_eq!(
                    declared(&on)
                        .iter()
                        .filter(|tool| tool.name == name)
                        .count(),
                    1,
                    "{name}"
                );
            }
            let make = declared(&on)
                .into_iter()
                .find(|tool| tool.name == made_skill::MAKE)
                .unwrap();
            assert!(make.description.contains("you'll make it now"));
            assert!(!make.description.contains("confirm_agent_task"));
        }
    }

    /// Made by voice, a skill that reads Gmail not linked is told with
    /// `needs_link` and the sentence where to link it, said before the ask
    /// so the ask stays last; linked, or with `lamp-services` not telling,
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
        let answer = told(Ok(gmail.clone()), needs.as_deref());
        assert_eq!(
            answer["needs_link"],
            json!([{"code": "gmail", "name": "Gmail"}]),
            "{answer}"
        );
        let next = answer["next"].as_str().unwrap();
        let sentence = "Gmail isn't linked yet: link it in the app, under Skills, then \
                        Connectors, and the skill will start then.";
        let (link, ask) = (
            next.find(sentence).expect(next),
            next.find(made_skill::ASK_TO_PUT_ON).expect(next),
        );
        assert!(link < ask, "{next}");
        for linked in [Some(vec!["gmail".to_owned()]), None] {
            let needs = made_skill::needs_link(&gmail.uses, || linked.clone());
            let answer = told(Ok(gmail.clone()), needs.as_deref());
            assert!(answer.get("needs_link").is_none(), "{answer}");
            assert!(!answer["next"].as_str().unwrap().contains("isn't linked"));
            assert_eq!(answer["made"], "morning-mail");
        }
        let plain = made(None);
        let needs = made_skill::needs_link(&plain.uses, || panic!("never asked"));
        assert_eq!(needs, Some(vec![]));
        assert!(
            told(Ok(plain), needs.as_deref())
                .get("needs_link")
                .is_none()
        );
        // The make itself, with `lamp-services` not telling: kept and
        // asked about as before.
        let (root, keeping) = keeping("needs-link-unknown");
        let (url, served) = service(vec![(200, good())]);
        let answer = make_with(REQUEST, &keeping, chat_at(url), NOW, || None);
        assert_eq!(answer["made"], "evening-dim", "{answer}");
        assert!(answer.get("needs_link").is_none(), "{answer}");
        assert_eq!(served.join().unwrap(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
