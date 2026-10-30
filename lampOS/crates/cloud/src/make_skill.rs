//! A skill typed in the owner's app (#2813; the first builder on #2602,
//! 22 Oct 10:3x): `skills.make` hands the words to lamp-voice's app-make
//! socket (`lamp_voice::app_make`), which hands them over to the computer's
//! agent as the voice's own brief; `skills.made` reads what became of the
//! app's ask. Off unless `LAMP_APP_MAKE_SKILL=on` here and on the skills
//! page (`LAMP_SKILLS_PAGE=on`); lamp-voice checks the same switch in its
//! own unit, so this is not the only gate.
//!
//! **Nothing from the app puts a skill on.** The lamp asks about the kept
//! skill out loud, and only a spoken yes at the lamp puts it on (the lead's
//! ruling on #1711, 20 Oct 08:37). The app shows what it will ask about:
//! the name, what it does, what it may use and the SHA-256.
//!
//! The owner's alone, as every kind here is: only the lamp's own account
//! channel commands it (`online::commanded`).
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The switch, in lamp-cloud's unit: `on` exactly. lamp-voice's own name
/// for it (`lamp_voice::app_make::ENVIRONMENT`, held equal by a test).
pub const ENVIRONMENT: &str = "LAMP_APP_MAKE_SKILL";

/// lamp-voice's app-make socket, in its own runtime folder
/// (`lamp_voice::app_make::DEFAULT_SOCKET`, held equal by a test).
pub const SOCKET: &str = "/run/lampos-app-make/make.sock";

/// lamp-voice answers at once (the hand-over runs after, on its own
/// thread); the broker's loop waits on it, so it is bounded short.
pub const WAIT: Duration = Duration::from_secs(2);

/// What the app is told when lamp-voice can't be reached or doesn't answer:
/// nothing was asked.
pub const UNREACHED: &str = "making a skill can't be reached on the lamp now";

/// The longest words: the voice's own bound for a skill's request.
pub const MOST_WORDS_BYTES: usize = 180;

/// How often lamp-cloud asks lamp-voice whether making a skill is ready
/// ([`watch`]).
pub const READY_EVERY: Duration = Duration::from_secs(60);

/// The newest [`ready`] answer, read by `system.info`'s `can` with no wait.
static READY: AtomicBool = AtomicBool::new(false);

/// `{"ready": {}}` (fourteen-P15): whether lamp-voice's maker has a computer
/// to hand the work to, so the app's "Make a skill" can work on this lamp.
/// No answer is no.
pub fn ready(socket: &Path) -> bool {
    exchange(socket, &json!({"ready": {}}))
        .is_ok_and(|answer| answer.get("ready") == Some(&json!(true)))
}

/// [`ready`]'s newest answer.
pub fn is_ready() -> bool {
    READY.load(Ordering::Relaxed)
}

/// Asks [`ready`] at once and every [`READY_EVERY`] on a thread of its own,
/// so `can` names `skills.make` only while it is (the first builder's yes on
/// fourteen-P15: the button shows only on a lamp whose Harness has a
/// computer). The journal says each change, never more.
pub fn watch(socket: PathBuf) {
    let spawned = std::thread::Builder::new()
        .name("lamp-cloud-make-ready".into())
        .spawn(move || {
            let mut was = None;
            loop {
                let now = ready(&socket);
                READY.store(now, Ordering::Relaxed);
                if was != Some(now) {
                    eprintln!(
                        "lamp-cloud: making a skill is {}",
                        if now {
                            "ready: the computer's agent is set up"
                        } else {
                            "not ready: the computer's agent isn't set up for the lamp yet"
                        }
                    );
                    was = Some(now);
                }
                std::thread::sleep(READY_EVERY);
            }
        });
    if let Err(error) = spawned {
        eprintln!("lamp-cloud: making a skill: its readiness can't be watched: {error}");
    }
}

/// The socket, with the switch on.
pub fn from_environment() -> Option<PathBuf> {
    (std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")).then(|| SOCKET.into())
}

/// One line to lamp-voice, one line back.
fn exchange(socket: &Path, line: &Value) -> Result<Value, String> {
    let asked = || -> std::io::Result<String> {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(WAIT))?;
        stream.set_write_timeout(Some(WAIT))?;
        stream.write_all(format!("{line}\n").as_bytes())?;
        let mut reply = String::new();
        BufReader::new(stream.take(8_192)).read_line(&mut reply)?;
        Ok(reply)
    };
    let reply = asked().map_err(|error| {
        eprintln!("lamp-cloud: making a skill: lamp-voice did not answer: {error}");
        UNREACHED.to_owned()
    })?;
    serde_json::from_str(reply.trim()).map_err(|_| UNREACHED.to_owned())
}

fn request_id(data: &Value) -> Option<&str> {
    data.get("request_id").and_then(Value::as_str)
}

/// Why a request id sent is not taken, in the voice's words.
const BAD_REQUEST_ID: &str = "the request id is 1 to 64 letters, digits, '-', '_', '.' or ':'";

/// A request id as lamp-voice takes one (`app_make`): 1 to 64 bytes of
/// letters, digits, `-`, `_`, `.` and `:`. Checked here, so the app is told
/// what is wrong with the id it sent (El on #2889).
fn request_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

/// `skills.make` (`{"words", "request_id"}`): `{"making": true}`, or why
/// not in plain words. Never the words in the journal.
pub fn make(socket: &Path, data: &Value) -> Result<Option<Value>, String> {
    let words = data
        .get("words")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|words| !words.is_empty())
        .ok_or("say what the skill should do")?;
    if words.len() > MOST_WORDS_BYTES {
        return Err(format!(
            "what the skill should do is at most {MOST_WORDS_BYTES} bytes"
        ));
    }
    let id = request_id(data).ok_or("the ask needs a request id")?;
    if !request_id_ok(id) {
        return Err(BAD_REQUEST_ID.to_owned());
    }
    let reply = exchange(socket, &json!({"make": {"words": words, "request_id": id}}))?;
    if reply.get("making") == Some(&json!(true)) {
        eprintln!("lamp-cloud: making a skill: asked");
        return Ok(Some(json!({"making": true, "request_id": id})));
    }
    let why = reply
        .get("refused")
        .and_then(Value::as_str)
        .unwrap_or(UNREACHED)
        .to_owned();
    eprintln!("lamp-cloud: making a skill refused: {why}");
    Err(why)
}

/// `skills.made` (`{"request_id"}`, or `{}` for the newest): the app's ask
/// as lamp-voice tells it, with `uses` as the skills page's `may_use` rows
/// (`crate::skills::may_use`).
pub fn made(socket: &Path, data: &Value) -> Result<Option<Value>, String> {
    // An id that isn't a string is refused, never taken for "the newest"
    // (El on #2889): the app would be told of another ask.
    let asked = match (request_id(data), data.get("request_id")) {
        (Some(id), _) if request_id_ok(id) => json!({"made": {"request_id": id}}),
        (None, None) => json!({"made": {}}),
        _ => return Err(BAD_REQUEST_ID.to_owned()),
    };
    let mut told = exchange(socket, &asked)?;
    if let Some(why) = told.get("refused").and_then(Value::as_str) {
        return Err(why.to_owned());
    }
    if told.get("state").and_then(Value::as_str).is_none() {
        return Err(UNREACHED.to_owned());
    }
    if let Some(uses) = told.as_object_mut().and_then(|told| told.remove("uses")) {
        let permissions = lamp_skills::Permissions {
            light: uses["light"] == true,
            body: uses["body"] == true,
            topics: uses["topics"]
                .as_array()
                .map(|topics| {
                    topics
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            ..Default::default()
        };
        told["may_use"] = json!(crate::skills::may_use(&permissions));
    }
    Ok(Some(told))
}

/// `lamp-cloud make-skill make WORDS REQUEST_ID` or `made REQUEST_ID`, for
/// the played check (`tools/played/make-skill.sh`, `MAKE_FROM=app`): the
/// same [`make`] and [`made`] the app's kinds call, on [`SOCKET`], told in
/// plain words on one line. `making`, or `refused: WHY`; `STATE SHA256`
/// (`-` with none), or `unread: WHY`. The exit is 0 when asked and told,
/// else 1; 2 for words that are not a command.
pub fn command(arguments: &[String]) -> (String, u8) {
    command_at(Path::new(SOCKET), arguments)
}

/// [`command`] on `socket`.
pub fn command_at(socket: &Path, arguments: &[String]) -> (String, u8) {
    match arguments {
        [what, words, id] if what == "make" => {
            match make(socket, &json!({"words": words, "request_id": id})) {
                Ok(_) => ("making".to_owned(), 0),
                Err(why) => (format!("refused: {why}"), 1),
            }
        }
        [what, id] if what == "made" => match made(socket, &json!({"request_id": id})) {
            Ok(Some(told)) => (
                format!(
                    "{} {}",
                    told["state"].as_str().unwrap_or("unread"),
                    told["sha256"].as_str().unwrap_or("-")
                ),
                0,
            ),
            Ok(None) => ("unread: no answer".to_owned(), 1),
            Err(why) => (format!("unread: {why}"), 1),
        },
        _ => (
            "usage: lamp-cloud make-skill make WORDS REQUEST_ID | made REQUEST_ID".to_owned(),
            2,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A stand-in lamp-voice on a short path: each line asked, and the
    /// canned answers in turn.
    fn voice(name: &str, answers: Vec<Value>) -> (PathBuf, std::sync::mpsc::Receiver<Value>) {
        let dir = PathBuf::from(format!("/tmp/mk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("make.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (seen, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for answer in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                seen.send(serde_json::from_str(&line).unwrap()).unwrap();
                writeln!(stream, "{answer}").unwrap();
            }
        });
        (socket, lines)
    }

    /// `ready` (fourteen-P15): yes only for lamp-voice's `{"ready": true}`;
    /// its no, an answer of another shape, or no lamp-voice is no.
    #[test]
    fn ready_only_when_the_voice_says_so() {
        let (socket, lines) = voice(
            "ready",
            vec![
                json!({"ready": true}),
                json!({"ready": false, "why": "the computer's agent isn't set up for the lamp yet"}),
                json!({"making": true}),
            ],
        );
        assert!(ready(&socket));
        assert_eq!(lines.recv().unwrap(), json!({"ready": {}}));
        assert!(!ready(&socket));
        assert!(!ready(&socket));
        assert!(!ready(Path::new("/nowhere/make.sock")));
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// lamp-voice's own names, so the two never part.
    #[test]
    fn the_switch_and_the_socket_are_lamp_voices() {
        assert_eq!(ENVIRONMENT, lamp_voice::app_make::ENVIRONMENT);
        assert_eq!(SOCKET, lamp_voice::app_make::DEFAULT_SOCKET);
    }

    /// The words and the request id go to the voice as one line; its
    /// answer, or its refusal, comes back.
    #[test]
    fn a_make_is_handed_to_the_voice_and_its_answer_told() {
        let (socket, seen) = voice(
            "make",
            vec![
                json!({"making": true}),
                json!({"refused": "a skill is still being made"}),
            ],
        );
        let data = json!({"words": "a skill that dims the light at 10 pm", "request_id": "r1"});
        assert_eq!(
            make(&socket, &data),
            Ok(Some(json!({"making": true, "request_id": "r1"})))
        );
        assert_eq!(
            seen.recv().unwrap(),
            json!({"make": {"words": "a skill that dims the light at 10 pm", "request_id": "r1"}})
        );
        assert_eq!(
            make(&socket, &json!({"words": "another", "request_id": "r2"})),
            Err("a skill is still being made".to_owned())
        );
        // Refused here, before the voice is asked.
        assert!(make(&socket, &json!({"words": "x"})).is_err());
        assert!(make(&socket, &json!({"words": " ", "request_id": "r3"})).is_err());
        assert!(
            make(
                &socket,
                &json!({"words": "a".repeat(181), "request_id": "r3"})
            )
            .is_err()
        );
        for id in ["", "r 1", "r/1", &"r".repeat(65)] {
            let why = make(&socket, &json!({"words": "x", "request_id": id})).unwrap_err();
            assert!(why.contains("request id"), "{id:?}: {why}");
        }
        // No voice: nothing asked, and said so.
        assert_eq!(
            make(
                Path::new("/nowhere/make.sock"),
                &json!({"words": "x", "request_id": "r4"})
            ),
            Err(UNREACHED.to_owned())
        );
    }

    /// The state read back, its `uses` as the page's `may_use` rows.
    #[test]
    fn made_is_told_with_may_use_as_the_skills_page_says_it() {
        let (socket, seen) = voice(
            "made",
            vec![
                json!({"request_id": "r1", "state": "kept", "name": "evening-dim",
                    "description": "Dims the light a shade at 10 pm.", "version": "1.0.0",
                    "sha256": "a".repeat(64),
                    "uses": {"light": true, "body": false, "topics": ["room/presence"]}}),
                json!({"state": "none"}),
            ],
        );
        let told = made(&socket, &json!({"request_id": "r1"}))
            .unwrap()
            .unwrap();
        assert_eq!(seen.recv().unwrap(), json!({"made": {"request_id": "r1"}}));
        assert_eq!(told["state"], "kept");
        assert_eq!(told["sha256"], "a".repeat(64));
        assert!(told.get("uses").is_none(), "{told}");
        let names: Vec<&str> = told["may_use"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["name"].as_str())
            .collect();
        assert_eq!(names, ["light", "presence"], "{told}");
        assert_eq!(
            made(&socket, &json!({})).unwrap().unwrap(),
            json!({"state": "none"})
        );
        assert_eq!(seen.recv().unwrap(), json!({"made": {}}));
        // An id that isn't a string, or isn't of the voice's form, is
        // refused here: never taken for the newest (El on #2889).
        for id in [json!(42), json!("r 1"), json!("")] {
            assert_eq!(
                made(&socket, &json!({"request_id": id})),
                Err(BAD_REQUEST_ID.to_owned()),
                "{id}"
            );
        }
    }

    /// The played check's command: the same calls, told in plain words.
    #[test]
    fn the_played_command_tells_the_same_answers_in_plain_words() {
        let (socket, seen) = voice(
            "command",
            vec![
                json!({"making": true}),
                json!({"request_id": "r1", "state": "kept", "sha256": "a".repeat(64),
                    "uses": {"light": true, "body": false, "topics": []}}),
                json!({"state": "none"}),
                json!({"refused": "a skill is still being made"}),
            ],
        );
        let arguments = |words: &[&str]| {
            words
                .iter()
                .map(|word| (*word).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            command_at(
                &socket,
                &arguments(&["make", "a skill that dims the light", "r1"])
            ),
            ("making".to_owned(), 0)
        );
        assert_eq!(
            seen.recv().unwrap(),
            json!({"make": {"words": "a skill that dims the light", "request_id": "r1"}})
        );
        assert_eq!(
            command_at(&socket, &arguments(&["made", "r1"])),
            (format!("kept {}", "a".repeat(64)), 0)
        );
        assert_eq!(
            command_at(&socket, &arguments(&["made", "r9"])),
            ("none -".to_owned(), 0)
        );
        assert_eq!(
            command_at(&socket, &arguments(&["make", "another", "r2"])),
            ("refused: a skill is still being made".to_owned(), 1)
        );
        assert_eq!(command_at(&socket, &arguments(&["make", "x"])).1, 2);
        assert_eq!(
            command_at(Path::new("/nowhere/make.sock"), &arguments(&["made", "r1"])),
            (format!("unread: {UNREACHED}"), 1)
        );
    }
}
