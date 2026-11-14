//! `lamp-tasks skill` (#5978): the lamp's own Hermes lists and calls the
//! lamp's skill tools the same way the voice does, from the shell it runs
//! commands in. The image has no Python, so the caller is this program,
//! which is on every lamp that has the tasks part.
//!
//! - `lamp-tasks skill list`: the tools in [`TOOLS_DIR`]'s `tools.json`
//!   (`{"tools":[{"name":..,"description":..,"parameters":..}]}`), printed
//!   as one JSON line `{"ok":true,"tools":[...]}`.
//! - `lamp-tasks skill call NAME ['{"key":"value"}']`: one line
//!   `{"op":"call","name":NAME,"args":{...}}` on [`TOOLS_DIR`]'s
//!   `call.sock`, and the one line back printed as it came:
//!   `{"ok":true,"result":{...}}` or `{"ok":false,"error":"..."}`.
//!
//! Only [`TOOLS_DIR`], fixed here, is ever read or connected to: no option
//! or variable names another folder or socket, the socket is never
//! followed through a link, and a name `tools.json` doesn't list is refused
//! without connecting. A call waits at most [`DEADLINE`], connecting
//! included. Exit status: 0 for `ok:true`, 1 for the skills' `ok:false`,
//! 2 for a refusal of this program's own (`{"ok":false,"error":...}` too).
//!
//! The lamp's skills service applies its own checks to the call, as to the
//! voice's: cooldowns, the lamp's pause and its policy.
use serde_json::{Value, json};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

/// lamp-skills' tools folder (`lamp-skills --tools-dir`), the voice's
/// `LAMP_SKILLS_DIR`.
pub const TOOLS_DIR: &str = "/run/lampos-skills";
/// This program on a lamp, as the guidance names it
/// (`deploy/product/lamp-tasks.service`).
pub const PROGRAM: &str = "/opt/lampOS/tasks/current/lamp-tasks";
/// The tool list, beside the socket.
pub const CATALOG_FILE: &str = "tools.json";
/// The call socket.
pub const CALL_SOCKET: &str = "call.sock";
/// The longest a call waits for its answer, connecting included: the
/// voice's 1.5 s, and time to wait behind a voice call, as the socket
/// answers one at a time.
pub const DEADLINE: Duration = Duration::from_secs(3);
/// The longest `tools.json` read, as the voice's bound.
pub const MOST_CATALOG_BYTES: u64 = 65_536;
/// The longest answer read.
pub const MOST_ANSWER_BYTES: u64 = 65_536;
/// The longest line sent: lamp-skills reads 8192 bytes of it.
pub const MOST_CALL_BYTES: usize = 8_192;

/// Exit status for the skills' own `ok:false`.
pub const EXIT_REFUSED_BY_SKILLS: i32 = 1;
/// Exit status for a refusal of this program's own.
pub const EXIT_REFUSED_HERE: i32 = 2;

const USAGE: &str =
    "usage: lamp-tasks skill list | lamp-tasks skill call NAME ['{\"key\":\"value\"}']";

/// The tools `dir`'s `tools.json` lists, each an object with a name.
pub fn tools(dir: &Path) -> Result<Vec<Value>, String> {
    let path = dir.join(CATALOG_FILE);
    let mut text = String::new();
    fs::File::open(&path)
        .and_then(|file| file.take(MOST_CATALOG_BYTES + 1).read_to_string(&mut text))
        .map_err(|error| format!("the lamp's skill tools are not there ({error})"))?;
    if text.len() as u64 > MOST_CATALOG_BYTES {
        return Err(format!("{CATALOG_FILE} is over {MOST_CATALOG_BYTES} bytes"));
    }
    let file: Value =
        serde_json::from_str(&text).map_err(|_| format!("{CATALOG_FILE} is not JSON"))?;
    let tools = file["tools"]
        .as_array()
        .ok_or_else(|| format!("{CATALOG_FILE} has no tools list"))?;
    if tools.iter().any(|tool| !tool["name"].is_string()) {
        return Err(format!("{CATALOG_FILE} has a tool with no name"));
    }
    Ok(tools.clone())
}

/// `name` called with `args` (a JSON object, as text) on `dir`'s socket,
/// once `dir`'s tools list it: the answer line, an object with `ok`.
/// `Err` for a refusal here, nothing sent, or no answer in `deadline`.
pub fn call(dir: &Path, name: &str, args: &str, deadline: Duration) -> Result<Value, String> {
    let started = Instant::now();
    let args: Value =
        serde_json::from_str(args).map_err(|_| "the arguments are not JSON".to_owned())?;
    if !args.is_object() {
        return Err("the arguments are not a JSON object".into());
    }
    let known = tools(dir)?;
    if !known.iter().any(|tool| tool["name"] == name) {
        return Err(format!(
            "no skill tool named {name}; see: lamp-tasks skill list"
        ));
    }
    let mut line = serde_json::to_vec(&json!({"op": "call", "name": name, "args": args}))
        .map_err(|_| "the call is not sendable".to_owned())?;
    if line.len() + 1 > MOST_CALL_BYTES {
        return Err(format!("the call is over {MOST_CALL_BYTES} bytes"));
    }
    line.push(b'\n');
    let socket = dir.join(CALL_SOCKET);
    // Never through a link: only lamp-skills' own socket in this folder.
    let is_socket =
        fs::symlink_metadata(&socket).is_ok_and(|metadata| metadata.file_type().is_socket());
    if !is_socket {
        return Err("the lamp's skills are not answering (no socket)".into());
    }
    let left = || {
        deadline
            .checked_sub(started.elapsed())
            .filter(|left| !left.is_zero())
    };
    let answer = (|| -> io::Result<Value> {
        let stream = UnixStream::connect(&socket)?;
        let timed_out = || io::Error::from(io::ErrorKind::TimedOut);
        stream.set_write_timeout(Some(left().ok_or_else(timed_out)?))?;
        (&stream).write_all(&line)?;
        let mut answer = Vec::new();
        let mut reader = BufReader::new((&stream).take(MOST_ANSWER_BYTES));
        loop {
            // As the voice's read: on macOS a socket whose peer has already
            // answered and closed refuses a timeout (EINVAL), though its
            // answer waits to be read.
            if let Err(error) = stream.set_read_timeout(Some(left().ok_or_else(timed_out)?))
                && error.raw_os_error() != Some(rustix::io::Errno::INVAL.raw_os_error())
            {
                return Err(error);
            }
            let read = reader.read_until(b'\n', &mut answer)?;
            if answer.last() == Some(&b'\n') || read == 0 {
                break;
            }
        }
        serde_json::from_slice(&answer).map_err(io::Error::other)
    })()
    .map_err(|error| format!("the lamp's skills gave no answer ({error})"))?;
    if !answer["ok"].is_boolean() {
        return Err("the lamp's skills gave an answer with no ok".into());
    }
    Ok(answer)
}

fn refused(why: impl Into<String>) -> (String, i32) {
    let why: String = why.into();
    (
        json!({"ok": false, "error": why}).to_string(),
        EXIT_REFUSED_HERE,
    )
}

/// `lamp-tasks skill ARGS...` on `dir`: the JSON line to print and the
/// exit status.
pub fn run(dir: &Path, args: &[String]) -> (String, i32) {
    match args {
        [list] if list == "list" => match tools(dir) {
            Ok(tools) => (json!({"ok": true, "tools": tools}).to_string(), 0),
            Err(why) => refused(why),
        },
        [verb, name, rest @ ..] if verb == "call" && rest.len() <= 1 => {
            let args = rest.first().map_or("{}", String::as_str);
            match call(dir, name, args, DEADLINE) {
                Ok(answer) => {
                    let status = if answer["ok"] == true {
                        0
                    } else {
                        EXIT_REFUSED_BY_SKILLS
                    };
                    (answer.to_string(), status)
                }
                Err(why) => refused(why),
            }
        }
        _ => refused(USAGE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;

    fn folder(tools: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(CATALOG_FILE), tools).unwrap();
        dir
    }

    const TOOLS: &str = r#"{"body":["dance"],"core":[],"tools":[{"name":"dance","description":"Dance.","parameters":{"type":"object","properties":{"style":{"type":"string","enum":["happy","slow"]}}}},{"name":"light_on","description":"Light."}]}"#;

    /// A skills socket in `dir` answering one call with `answer`, giving
    /// back the line it was sent.
    fn answering(dir: &Path, answer: &'static str) -> mpsc::Receiver<String> {
        let listener = UnixListener::bind(dir.join(CALL_SOCKET)).unwrap();
        let (sent, got) = mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            sent.send(line).unwrap();
            writeln!(&stream, "{answer}").unwrap();
        });
        got
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_list_is_the_tools_as_the_skills_wrote_them() {
        let dir = folder(TOOLS);
        let (out, status) = run(dir.path(), &strings(&["list"]));
        assert_eq!(status, 0);
        let out: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["tools"][0]["name"], "dance");
        assert_eq!(out["tools"][1]["name"], "light_on");
        let empty = tempfile::tempdir().unwrap();
        let (out, status) = run(empty.path(), &strings(&["list"]));
        assert_eq!(status, EXIT_REFUSED_HERE);
        assert!(out.contains("\"ok\":false"), "{out}");
    }

    #[test]
    fn a_call_is_the_voices_one_line_and_its_answer_is_printed_as_it_came() {
        let dir = folder(TOOLS);
        let sent = answering(dir.path(), r#"{"ok":true,"result":{"done":true}}"#);
        let (out, status) = run(
            dir.path(),
            &strings(&["call", "dance", r#"{"style":"happy"}"#]),
        );
        assert_eq!(status, 0, "{out}");
        assert_eq!(out, r#"{"ok":true,"result":{"done":true}}"#);
        let line: Value = serde_json::from_str(&sent.recv().unwrap()).unwrap();
        assert_eq!(
            line,
            json!({"op":"call","name":"dance","args":{"style":"happy"}})
        );
    }

    #[test]
    fn the_skills_refusal_is_printed_with_a_nonzero_exit() {
        let dir = folder(TOOLS);
        let sent = answering(dir.path(), r#"{"ok":false,"error":"the lamp is paused"}"#);
        let (out, status) = run(dir.path(), &strings(&["call", "light_on"]));
        assert_eq!(status, EXIT_REFUSED_BY_SKILLS);
        assert!(out.contains("the lamp is paused"));
        let line: Value = serde_json::from_str(&sent.recv().unwrap()).unwrap();
        assert_eq!(line["args"], json!({}), "no arguments is an empty object");
    }

    #[test]
    fn a_name_not_listed_or_bad_arguments_never_reach_the_socket() {
        let dir = folder(TOOLS);
        // A listener that is never accepted from: a connection would wait.
        let _listener = UnixListener::bind(dir.path().join(CALL_SOCKET)).unwrap();
        for args in [
            strings(&["call", "guard_on"]),
            strings(&["call", "dance", "not json"]),
            strings(&["call", "dance", "[1]"]),
            strings(&["call", "dance", "{}", "more"]),
            strings(&["call"]),
            strings(&["other"]),
            strings(&[]),
        ] {
            let started = Instant::now();
            let (out, status) = run(dir.path(), &args);
            assert_eq!(status, EXIT_REFUSED_HERE, "{args:?}");
            assert!(out.contains("\"ok\":false"), "{out}");
            assert!(started.elapsed() < Duration::from_millis(500), "{args:?}");
        }
        let long = format!("{{\"style\":\"{}\"}}", "x".repeat(MOST_CALL_BYTES));
        let (out, status) = run(dir.path(), &strings(&["call", "dance", &long]));
        assert_eq!(status, EXIT_REFUSED_HERE);
        assert!(out.contains("over 8192 bytes"), "{out}");
    }

    #[test]
    fn a_socket_through_a_link_is_never_followed() {
        let dir = folder(TOOLS);
        let elsewhere = tempfile::tempdir().unwrap();
        let _other = answering(elsewhere.path(), r#"{"ok":true,"result":{}}"#);
        std::os::unix::fs::symlink(
            elsewhere.path().join(CALL_SOCKET),
            dir.path().join(CALL_SOCKET),
        )
        .unwrap();
        let (out, status) = run(dir.path(), &strings(&["call", "dance"]));
        assert_eq!(status, EXIT_REFUSED_HERE);
        assert!(out.contains("no socket"), "{out}");
    }

    #[test]
    fn a_silent_service_is_given_up_on_by_the_deadline() {
        let dir = folder(TOOLS);
        let listener = UnixListener::bind(dir.path().join(CALL_SOCKET)).unwrap();
        let (held, hold) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            let _ = hold.recv();
        });
        let started = Instant::now();
        let refused = call(dir.path(), "dance", "{}", Duration::from_millis(300));
        assert!(refused.unwrap_err().contains("no answer"));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(held);
    }

    #[test]
    fn a_tools_file_that_is_not_the_skills_shape_is_refused() {
        for text in ["not json", "{}", r#"{"tools":[{"description":"x"}]}"#] {
            let dir = folder(text);
            assert!(tools(dir.path()).is_err(), "{text}");
        }
        let dir = folder(&format!(
            "{{\"tools\":[],\"pad\":\"{}\"}}",
            "x".repeat(MOST_CATALOG_BYTES as usize)
        ));
        assert!(tools(dir.path()).unwrap_err().contains("over"));
    }
}
