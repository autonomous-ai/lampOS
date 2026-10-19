//! A person marks a bad moment for review: "Lamp, that was weird" (#803,
//! `docs/marked.md`).
//!
//! The model calls [`MARK`] with what it heard. The voice writes one journal
//! line and copies its own session records of the last [`WINDOW`] into
//! `marked/` beside `sessions/`, where `lamp-records` never rotates them, and
//! deletes marks older than [`KEPT_FOR`]. Nothing leaves the lamp; no audio
//! is kept, since the voice keeps none. `lamp-voice marked` lists them.
//!
//! Behind `LAMP_MARK_WEIRD=on`: without it the function is not declared and
//! nothing here runs.

use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// `on` for the function to be declared. Anything else: never.
pub const ENVIRONMENT: &str = "LAMP_MARK_WEIRD";
/// The provider worker's flag for it, followed by the session's folder.
pub const FLAG: &str = "--mark-weird";
/// The function the model calls.
pub const MARK: &str = "mark_for_review";
/// How much of the session before the mark is kept.
pub const WINDOW: Duration = Duration::from_secs(60);
/// How long a mark is kept.
pub const KEPT_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// What the person hears back, said once and nothing more.
pub const THANKS: &str = "Thanks, I'll learn from that.";
/// The longest reason kept, in bytes.
const REASON_BYTES: usize = 200;

/// The serving voice's session folder, set once when it is made, so that
/// the provider worker can be told it.
static SESSION: OnceLock<PathBuf> = OnceLock::new();

/// The coordinator made its session folder. With the switch on, marks past
/// [`KEPT_FOR`] are deleted now too, so they go even if nobody marks again.
pub fn session_opened(folder: &Path) {
    let _ = SESSION.set(folder.to_owned());
    if std::env::var(ENVIRONMENT).as_deref() == Ok("on")
        && let Some(marks) = marks_of(folder)
    {
        expire(&marks, SystemTime::now());
    }
}

/// The provider worker's arguments: the flag and the session folder, with
/// the switch on and a session being served; nothing otherwise.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        SESSION.get().map(PathBuf::as_path),
    )
}

fn arguments(switch: Option<&str>, session: Option<&Path>) -> Vec<String> {
    match (switch, session) {
        (Some("on"), Some(folder)) if folder.is_absolute() => {
            vec![FLAG.to_owned(), folder.display().to_string()]
        }
        _ => Vec::new(),
    }
}

/// Where a session's marks go: `marked/` beside the `sessions/` folder that
/// holds it.
pub fn marks_of(session: &Path) -> Option<PathBuf> {
    Some(session.parent()?.parent()?.join("marked"))
}

pub fn is_tool(name: &str) -> bool {
    name == MARK
}

pub fn tool() -> Tool {
    Tool {
        name: MARK.into(),
        description: format!(
            "Call this when the person tells you a moment just now went wrong: \"that was \
             weird\", \"that was wrong\", \"bad answer\", \"you interrupted me\". Give their \
             words as the reason. Then say only one short, warm thanks, such as \"{THANKS}\" \
             No apology and no questions. Never call it for anything else."
        ),
        parameters: Some(json!({
            "type": "object",
            "properties": {"reason": {"type": "string", "description": "What the person said was wrong, in their words."}},
            "required": ["reason"]
        })),
        silent: false,
    }
}

/// One call: mark the last [`WINDOW`] of `session` now. Only for words a
/// person said: a request the lamp wrote itself marks nothing.
pub fn run(session: &Path, call: &ToolCall, heard: bool) -> (Value, bool) {
    if !heard {
        return (
            json!({"error": "Only a person can mark a moment for review."}),
            false,
        );
    }
    let reason = call
        .args
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    match mark(session, reason, lamp_ipc::monotonic_us(), SystemTime::now()) {
        Ok(_) => (json!({"ok": true, "say": THANKS}), true),
        Err(error) => {
            eprintln!("lamp-voice: a mark for review could not be kept: {error}");
            (crate::tools::unavailable(), false)
        }
    }
}

/// Keep `session`'s records of the [`WINDOW`] before `now_us` (the voice's
/// monotonic clock, as its records' `at_us`) in a new mark, after deleting
/// marks older than [`KEPT_FOR`]. Returns the mark's folder.
pub fn mark(session: &Path, reason: &str, now_us: u64, wall: SystemTime) -> io::Result<PathBuf> {
    let marks = marks_of(session).ok_or_else(|| io::Error::other("no marks folder"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&marks)?;
    expire(&marks, wall);
    let wall_ms = wall
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis() as u64;
    let id = session
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_owned();
    let mut said = String::new();
    for c in reason.chars() {
        if said.len() + c.len_utf8() > REASON_BYTES {
            break;
        }
        said.push(if c.is_control() { ' ' } else { c });
    }
    let from_us = now_us.saturating_sub(WINDOW.as_micros() as u64);
    let lines = window(session, from_us, now_us)?;
    let folder = marks.join(format!("{wall_ms}-{id}"));
    fs::DirBuilder::new().mode(0o700).create(&folder)?;
    write_private(
        &folder.join("events.jsonl"),
        lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>()
            .as_bytes(),
    )?;
    let about = json!({
        "reason": said,
        "session": id,
        "session_folder": session.display().to_string(),
        "marked_at_unix_ms": wall_ms,
        "window_from_us": from_us,
        "window_to_us": now_us,
        "window_seconds": WINDOW.as_secs(),
        "events": lines.len(),
    });
    write_private(&folder.join("mark.json"), about.to_string().as_bytes())?;
    eprintln!(
        "lamp-voice: marked for review: {said}, session {id}, last {} s",
        WINDOW.as_secs()
    );
    Ok(folder)
}

/// The session's record lines whose `at_us` is in `[from_us, to_us]`, in
/// time order. A line being written as it is read, or not JSON, is left out;
/// a folder or file that cannot be read is an error, never an empty window.
fn window(session: &Path, from_us: u64, to_us: u64) -> io::Result<Vec<String>> {
    let mut kept: Vec<(u64, String)> = Vec::new();
    for entry in fs::read_dir(session.join("metadata"))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("events-") && name.ends_with(".jsonl")) {
            continue;
        }
        for line in BufReader::new(fs::File::open(entry.path())?).split(b'\n') {
            let Ok(line) = String::from_utf8(line?) else {
                continue;
            };
            let at = serde_json::from_str::<Value>(&line)
                .ok()
                .and_then(|event| event.get("at_us").and_then(Value::as_u64));
            if let Some(at) = at.filter(|at| (from_us..=to_us).contains(at)) {
                kept.push((at, line));
            }
        }
    }
    kept.sort_by_key(|(at, _)| *at);
    Ok(kept.into_iter().map(|(_, line)| line).collect())
}

/// Delete marks older than [`KEPT_FOR`], by the time in their folder's name.
/// Run when a serving voice starts with the switch on, when a mark is made,
/// and when the marks are listed.
pub fn expire(marks: &Path, wall: SystemTime) {
    let Ok(entries) = fs::read_dir(marks) else {
        return;
    };
    let Ok(now) = wall.duration_since(UNIX_EPOCH) else {
        return;
    };
    let oldest = now.saturating_sub(KEPT_FOR).as_millis() as u64;
    for entry in entries.flatten() {
        let at = entry
            .file_name()
            .to_str()
            .and_then(|name| name.split('-').next())
            .and_then(|ms| ms.parse::<u64>().ok());
        if at.is_some_and(|at| at < oldest) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

/// `lamp-voice marked [MARKS]`: the marks, newest first, one line each.
pub fn list(marks: &Path) -> io::Result<Vec<String>> {
    expire(marks, SystemTime::now());
    let mut found: Vec<(u64, String)> = Vec::new();
    for entry in fs::read_dir(marks)?.flatten() {
        let Ok(text) = fs::read_to_string(entry.path().join("mark.json")) else {
            continue;
        };
        let Ok(about) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let at = about["marked_at_unix_ms"].as_u64().unwrap_or(0);
        found.push((
            at,
            format!(
                "{} · {} · session {} · last {} s · {} records · {}",
                when(at),
                about["reason"].as_str().unwrap_or(""),
                about["session"].as_str().unwrap_or(""),
                about["window_seconds"].as_u64().unwrap_or(0),
                about["events"].as_u64().unwrap_or(0),
                entry.path().display()
            ),
        ));
    }
    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    Ok(found.into_iter().map(|(_, line)| line).collect())
}

/// A mark's time as the lamp's local date and minute: with the zone's
/// offset at the mark, not now's (#820). UTC only while the lamp's zone
/// cannot be read at all.
fn when(unix_ms: u64) -> String {
    let offset = crate::local_time::system()
        .map_or(0, |zone| i64::from(zone.offset_minutes_at(unix_ms)) * 60);
    let seconds = (unix_ms / 1000) as i64 + offset;
    let (year, month, day) = crate::local_time::civil(seconds.div_euclid(86_400));
    let minute = seconds.rem_euclid(86_400) / 60;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        minute / 60,
        minute % 60
    )
}

/// The default folder `lamp-voice marked` reads.
pub const DEFAULT_MARKS: &str = "/var/lib/lampOS/voice/marked";

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "lamp-voice-marked-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        let session = root.join("sessions").join("abc123");
        fs::create_dir_all(session.join("metadata")).unwrap();
        session
    }

    #[test]
    fn the_flag_goes_to_the_worker_only_with_the_switch_on_and_a_session() {
        let session = Path::new("/var/lib/lampOS/voice/sessions/abc");
        assert!(arguments(None, Some(session)).is_empty());
        assert!(arguments(Some("off"), Some(session)).is_empty());
        assert!(arguments(Some("on"), None).is_empty());
        assert!(arguments(Some("on"), Some(Path::new("relative"))).is_empty());
        assert_eq!(
            arguments(Some("on"), Some(session)),
            [FLAG, "/var/lib/lampOS/voice/sessions/abc"]
        );
        assert_eq!(
            marks_of(session).unwrap(),
            Path::new("/var/lib/lampOS/voice/marked")
        );
    }

    #[test]
    fn a_mark_keeps_exactly_the_last_sixty_seconds() {
        let session = folder("window");
        let now = 500_000_000u64;
        let window = WINDOW.as_micros() as u64;
        let line = |at: u64, kind: &str| format!("{{\"at_us\":{at},\"kind\":\"{kind}\"}}\n");
        let early = line(now - window - 1, "too_early");
        let first = line(now - window, "first");
        let last = line(now, "last");
        let late = line(now + 1, "too_late");
        fs::write(
            session.join("metadata/events-0.jsonl"),
            format!("{early}{last}not json\n{{\"at_us\":"),
        )
        .unwrap();
        fs::write(
            session.join("metadata/events-1.jsonl"),
            format!("{first}{late}"),
        )
        .unwrap();
        let wall = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let mark = mark(&session, "you interrupted me\n", now, wall).unwrap();
        let kept = fs::read_to_string(mark.join("events.jsonl")).unwrap();
        assert_eq!(kept, format!("{first}{last}"));
        let about: Value =
            serde_json::from_str(&fs::read_to_string(mark.join("mark.json")).unwrap()).unwrap();
        assert_eq!(about["reason"], "you interrupted me ");
        assert_eq!(about["session"], "abc123");
        assert_eq!(about["events"], 2);
        assert!(mark.starts_with(marks_of(&session).unwrap()));
        let _ = fs::remove_dir_all(session.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn marks_older_than_seven_days_are_deleted_and_the_rest_listed_newest_first() {
        let session = folder("sweep");
        let day = Duration::from_secs(24 * 60 * 60);
        let start = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        mark(&session, "that was weird", 1, start).unwrap();
        mark(&session, "bad answer", 2, start + day).unwrap();
        let marks = marks_of(&session).unwrap();
        let listed = list(&marks).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed[0].contains("bad answer") && listed[1].contains("that was weird"));
        // Eight days after the first, a new mark sweeps it, and only it.
        mark(&session, "that was wrong", 3, start + day * 8).unwrap();
        let listed = list(&marks).unwrap();
        assert_eq!(listed.len(), 2, "{listed:?}");
        assert!(listed[0].contains("that was wrong") && listed[1].contains("bad answer"));
        let _ = fs::remove_dir_all(session.parent().unwrap().parent().unwrap());
    }

    /// Reviewer on #855: a mark goes after seven days with no later mark.
    #[test]
    fn an_old_mark_expires_with_no_later_mark() {
        let session = folder("expire");
        let start = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let made = mark(&session, "that was weird", 1, start).unwrap();
        let marks = marks_of(&session).unwrap();
        expire(&marks, start + KEPT_FOR - Duration::from_secs(1));
        assert!(made.join("events.jsonl").exists());
        expire(&marks, start + KEPT_FOR + Duration::from_secs(1));
        assert!(!made.exists());
        // Listing runs the same expiry, at the real time: this mark was
        // made long before it.
        let old = mark(&session, "bad answer", 2, start).unwrap();
        assert!(list(&marks).unwrap().is_empty());
        assert!(!old.exists());
        let _ = fs::remove_dir_all(session.parent().unwrap().parent().unwrap());
    }

    /// Reviewer on #855: records that cannot be read are a failure, and the
    /// person is not thanked as if they were kept.
    #[test]
    fn records_that_cannot_be_read_are_not_a_kept_mark() {
        let session = folder("unreadable");
        fs::remove_dir_all(session.join("metadata")).unwrap();
        fs::write(session.join("metadata"), "not a folder").unwrap();
        let wall = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        assert!(mark(&session, "that was weird", 1, wall).is_err());
        let call = ToolCall {
            id: String::new(),
            name: MARK.into(),
            args: json!({"reason": "that was weird"}),
        };
        let (told, answered) = run(&session, &call, true);
        assert!(!answered && told.get("ok").is_none(), "{told}");
        let _ = fs::remove_dir_all(session.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_request_the_lamp_wrote_itself_marks_nothing() {
        let session = folder("written");
        let call = ToolCall {
            id: String::new(),
            name: MARK.into(),
            args: json!({"reason": "that was weird"}),
        };
        let (told, answered) = run(&session, &call, false);
        assert!(!answered && told.get("error").is_some());
        assert!(!marks_of(&session).unwrap().exists());
        let _ = fs::remove_dir_all(session.parent().unwrap().parent().unwrap());
    }
}
