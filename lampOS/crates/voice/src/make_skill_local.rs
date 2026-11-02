//! A skill made by the lamp's own model, with no computer (Fourteen's
//! proposal on #4710, 2 Nov): off unless `LAMP_MAKE_SKILL_LOCAL=on`. The
//! model calls [`made_skill::MAKE`] with the person's request, and the
//! same [`made_skill::brief`] a computer's agent is handed goes instead to
//! the lamp's own chat (the second route's, `llm_*`), streamed, once more
//! after a 404, a 5xx, a timeout or a broken connection ([`again`]).
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
//! With the switch on, [`made_skill::MAKE`] and [`made_skill::PUT_ON`] are
//! the voice's own, whether or not a tasks part is there, and a computer's
//! agent is never handed a skill to make.
use crate::made_skill::{self, Keeping, Made, Refused};
use crate::second_provider::net::{self, Chat, ChatEvent, Message};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// Where the voice reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_MAKE_SKILL_LOCAL";
/// The longest one answer is waited for, from the request to its end.
pub const WRITE_WITHIN: Duration = Duration::from_secs(30);
/// The most of an answer read: two files of at most 8 KiB, and the fences.
const MOST_ANSWER_BYTES: usize = 20 * 1024;
/// What a made skill's answer is kept from: this, and when it was made.
pub const FROM: &str = "local:";
/// A made skill that `lamp-skills pack` and [`made_skill::capped`] take,
/// shown to the model whole: the brief alone was written for a computer's
/// agent with the repository at hand, and the lamp's own model, without it,
/// wrote fields the check refused (lamp-9adf, 2 Nov 16:10 and 16:12: a
/// trigger field and a light step it doesn't know).
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
light = { colour = \"cyan\", level = 3, for = \"20s\" }
```

```SKILL.md
---
name: water-on-return
description: Glows cyan for twenty seconds when you come back to the desk, a nudge to drink some water.
---

# Water on return

When someone is back at the lamp, it glows cyan for a moment.
```";

/// What the model is told it is, before the brief: the format, and a whole
/// skill the lamp's check takes.
fn system() -> String {
    format!(
        "You write skills for a lamp. Answer exactly as the request says, with the two fenced \
         blocks and nothing else. Use only the keys and values the request names, written as \
         in this skill, which the lamp's check takes:\n\n{EXAMPLE}\n\nA skill can't speak \
         or keep time by itself: a reminder at a time or every so often is the lamp's own \
         timer, not a skill. Make what the request asks with the light or the head, on the \
         lamp's resting or someone being there."
    )
}

/// Whether the switch is on in this process's environment.
pub fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
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
        net::Error::Url | net::Error::Protocol(_) => false,
    }
}

/// The model's answer to `brief`, its text whole, asked once more when
/// [`again`] says so.
pub async fn written(chat: &Chat, brief: &str) -> net::Result<String> {
    match write_once(chat, brief).await {
        Err(error) if again(&error) => {
            eprintln!(
                "lamp-voice: a skill made here: the model's answer failed ({error}); asked once more"
            );
            write_once(chat, brief).await
        }
        answer => answer,
    }
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

/// `make_skill` with the switch: the skill for `request` written by the
/// model `chat` gives, kept in `keeping` as made at `now_unix_ms`, and
/// asked about then. Blocking: the model is asked on a thread of its own,
/// with a runtime of its own. What the model is told.
pub fn make(
    request: &str,
    keeping: &Keeping,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    now_unix_ms: u64,
) -> Value {
    let brief = match made_skill::brief(request) {
        Ok(brief) => brief,
        Err(why) => {
            return json!({
                "error": why,
                "next": "Tell them that in a few words."
            });
        }
    };
    told(make_kept(
        request,
        brief,
        keeping,
        chat,
        &format!("{FROM}{now_unix_ms}"),
    ))
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
        Ok(text) => made_skill::keep(&text, false, from, keeping),
        Err(why) => {
            eprintln!("lamp-voice: a skill made here: the model didn't answer: {why}");
            Err(Refused("the lamp's model didn't answer".into(), None))
        }
    };
    let mut made = kept(ask(Arc::clone(&chat), brief));
    // Refused by the lamp's check: its lines go back to the model once, as
    // a computer's agent's repair does (`made_skill::repair_brief`).
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
fn told(made: Result<Made, Refused>) -> Value {
    match made {
        Ok(made) => {
            json!({
                "made": made.name,
                "description": made.description,
                "uses": made.uses.spoken(),
                "next": format!(
                    "In one or two short sentences say what it does and what it uses, then ask \
                     exactly: \"{}\" Only if they then say yes out loud, call {}.",
                    made_skill::ASK_TO_PUT_ON,
                    made_skill::PUT_ON
                )
            })
        }
        Err(Refused(why, _)) => json!({
            "error": format!("the skill couldn't be made: {why}"),
            "next": "Tell them in one short sentence that you couldn't make that one, with no \
                     technical words, and that they can ask again in other words."
        }),
    }
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
            if ago <= made_skill::PUT_ON_WITHIN
                && lamp_said.contains(made_skill::ASK_TO_PUT_ON) =>
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
/// ask and within [`UNTOLD_WITHIN`] of `now_unix_ms`: their names and
/// SHA-256s, oldest first.
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
                kept_at > asked_at && kept_at >= since && kept_at <= now_unix_ms
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

/// A call of [`made_skill::MAKE`] or [`made_skill::PUT_ON`] with the
/// switch, in the product's places: `request`'s words heard (`heard`) for a
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
        // said the ask (`made_skill::asked_in_answer`, as `agent_tasks`
        // does since #4880): a yes after a cut-in or another question puts
        // nothing on.
        if heard
            && let Err(error) = asked_at_yes(
                &keeping.directory,
                crate::thread::THREAD.lock().ok().and_then(|thread| {
                    thread.answered_last_but(request, std::time::Instant::now())
                }),
                now_unix_ms,
            )
        {
            eprintln!("lamp-voice: a skill made here: its ask in an answer was not kept: {error}");
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

    /// The skill shown to the model is one a made skill may be: the cap
    /// and the keeping take it (lamp-skills pack took it too, 2 Nov).
    #[test]
    fn the_example_shown_to_the_model_is_a_skill_the_lamp_takes() {
        let (root, keeping) = keeping("example");
        let made = made_skill::keep(EXAMPLE, false, "local:1", &keeping).unwrap();
        assert_eq!(made.name, "water-on-return");
        assert!(system().contains(EXAMPLE));
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

    /// Off, `make_skill` and `put_on_made_skill` are routed and declared as
    /// before: the tasks part's, or nobody's. On, they are the voice's own,
    /// with or without the tasks part, each declared once.
    #[test]
    fn the_switch_alone_makes_the_two_functions_the_voices() {
        use crate::tools::{Route, Settings, declared, route};
        let off = Settings::default();
        for name in [made_skill::MAKE, made_skill::PUT_ON] {
            assert_eq!(route(&off, name), Route::Nobody);
        }
        assert!(
            declared(&off)
                .iter()
                .all(|tool| tool.name != made_skill::MAKE)
        );
        let on = Settings {
            make_skill_local: true,
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
