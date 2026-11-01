//! A skill typed in the app (#2813; the first builder on #2602, 22 Oct
//! 10:3x): the person's words go to the same maker the voice uses
//! ([`crate::made_skill`]), and the skill goes on only by a spoken yes at
//! the lamp (the lead's ruling on #1711, 20 Oct 08:37). Off unless
//! [`ENVIRONMENT`] is `on`, and only with `LAMP_MAKE_SKILL` and the tasks
//! part.
//!
//! - **The socket** ([`SOCKET_ENVIRONMENT`], [`DEFAULT_SOCKET`] if unset),
//!   for `lamp-cloud`: one JSON line in, one out, then closed.
//!   - `{"make": {"words", "request_id"}}`: the words become the voice's
//!     own brief ([`crate::made_skill::brief`], at most 180 bytes) and are
//!     handed over at once on a thread of their own
//!     ([`crate::agent_tasks::hand_over`]): typing them in the app is the
//!     person's ask, so there is no "shall I start it?". The answer is
//!     `{"making": true}` once the ask is recorded, before the hand-over,
//!     or `{"refused": WHY}` in plain words.
//!   - `{"made": {"request_id"?}}`: the state of that ask, or of the
//!     newest, only ever one the app made ([`status`]).
//!   - `{"ready": {}}`: `{"ready": true}` when the tasks part has a
//!     computer to hand the work to ([`crate::agent_tasks::has_a_target`],
//!     within [`READY_WAIT`]), else `{"ready": false, "why": NOT_SET_UP}`.
//!     `lamp-cloud` names `skills.make` in `can` only while it is ready
//!     (fourteen-P15), so the app's button shows only on a lamp whose
//!     Harness has a computer. It reads nothing of the asks.
//! - **Told aloud.** The hand-over is never kept as typed
//!   (`task_news::keep_typed`), so its news, the skill kept and "Shall I
//!   put it on the lamp?", is told by the voice's spoken task news at a
//!   good moment, and the ask is dated only when it is heard whole
//!   (`made_skill::asked`). Only a spoken yes within its 120 s puts it on
//!   (`made_skill::put_on`, and the manage helper's own check, #2874).
//! - **Kept** beside the made skills, in their root-only folder: the app's
//!   asks, at most [`MOST_ASKS`] ([`APP_INDEX`]), each its request id,
//!   when, the task it became, and why it wasn't kept if it wasn't. Never
//!   the words.
use crate::made_skill;
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// The switch, in the coordinator's environment. `on` only.
pub const ENVIRONMENT: &str = "LAMP_APP_MAKE_SKILL";
/// The socket's path, absolute.
pub const SOCKET_ENVIRONMENT: &str = "LAMP_APP_MAKE_SOCKET";
/// The group, by name, that may ask (`lampos`, `lamp-cloud`'s).
pub const GROUP_ENVIRONMENT: &str = "LAMP_APP_MAKE_GROUP";
/// The socket in a directory of its own, as the say service's.
pub const DEFAULT_SOCKET: &str = "/run/lampos-app-make/make.sock";
/// The app's asks, beside the made skills' own index.
pub const APP_INDEX: &str = "app.json";
/// The app's asks kept, the newest; the made skills keep five too.
pub const MOST_ASKS: usize = made_skill::KEPT_MOST;
/// An ask still being made after this long is let go: another may start.
pub const MAKING_LONGEST_MS: u64 = 30 * 60_000;
/// The hand-over's whole wait, on its own thread.
pub const HAND_OVER_WAIT: Duration = Duration::from_secs(20);

/// How long a `ready` waits on the tasks part's `targets`: within the 2 s
/// `lamp-cloud` waits on this socket.
pub const READY_WAIT: Duration = Duration::from_secs(1);
/// A client has this long to send its line, and the line is at most
/// [`MOST_LINE_BYTES`].
const LINE_WAIT: Duration = Duration::from_secs(2);
const MOST_LINE_BYTES: u64 = 2_048;
const MOST_REQUEST_ID_BYTES: usize = 64;

/// Now, as `made.json`'s times are kept.
pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

fn asks(directory: &Path) -> Vec<Value> {
    fs::read(directory.join(APP_INDEX))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
}

fn write_asks(directory: &Path, entries: &[Value]) -> io::Result<()> {
    // A writer's own name: two never share one half-written file.
    let new = directory.join(format!(
        ".{APP_INDEX}.{}.{:?}.new",
        std::process::id(),
        std::thread::current().id()
    ));
    let mut file = fs::File::create(&new)?;
    file.write_all(&serde_json::to_vec(entries).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&new, directory.join(APP_INDEX))
}

fn write_seen(directory: &Path, ids: &[String]) -> io::Result<()> {
    let new = directory.join(format!(
        ".{SEEN_IDS}.{}.{:?}.new",
        std::process::id(),
        std::thread::current().id()
    ));
    let mut file = fs::File::create(&new)?;
    file.write_all(&serde_json::to_vec(ids).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&new, directory.join(SEEN_IDS))
}

/// The lock every writer of [`APP_INDEX`] holds across its read, its
/// decision and its write, in whichever process (the socket's, the task
/// news', the texts' reader): one ask never undoes another (reviewer-two's
/// P2 on #2887).
const LOCK: &str = ".app.lock";

/// Read the app's asks, let `change` decide, and write them back if it
/// says so, all under [`LOCK`]. With `create` false and no asks kept,
/// nothing is locked or made.
fn transact<R>(
    directory: &Path,
    create: bool,
    change: impl FnOnce(&mut Vec<Value>) -> (bool, R),
) -> io::Result<Option<R>> {
    if !create && !directory.join(APP_INDEX).exists() {
        return Ok(None);
    }
    made_skill::ensure_private(directory)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(LOCK))?;
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive)?;
    let mut entries = asks(directory);
    let (write, told) = change(&mut entries);
    if write {
        write_asks(directory, &entries)?;
    }
    Ok(Some(told))
}

/// Why a request id sent is not taken (El's P3 on #2887).
pub const BAD_REQUEST_ID: &str = "the request id is 1 to 64 letters, digits, '-', '_', '.' or ':'";

/// The app's request ids remembered, the newest, so one sent again is
/// refused long after its ask left [`APP_INDEX`] (El's P3 on #2887).
const SEEN_IDS: &str = "app-ids.json";
const MOST_SEEN_IDS: usize = 100;

fn seen_ids(directory: &Path) -> Vec<String> {
    fs::read(directory.join(SEEN_IDS))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        .unwrap_or_default()
}

fn request_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MOST_REQUEST_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

/// Record the app's ask `request_id` for `words` at `now_unix_ms` in the
/// made skills' `directory`: the brief to hand over, or why not, in plain
/// words. One at a time: while the newest is still being made (and not
/// [`MAKING_LONGEST_MS`] old), another is refused.
pub fn begin(
    directory: &Path,
    words: &str,
    request_id: &str,
    now_unix_ms: u64,
) -> Result<String, String> {
    if request_id.is_empty() {
        return Err("the ask needs a request id".into());
    }
    if !request_id_ok(request_id) {
        return Err(BAD_REQUEST_ID.into());
    }
    let brief = made_skill::brief(words).map_err(str::to_owned)?;
    let admitted = transact(directory, true, |entries| {
        let mut seen = seen_ids(directory);
        if seen.iter().any(|id| id == request_id)
            || entries
                .iter()
                .any(|entry| entry["request_id"] == request_id)
        {
            return (false, Err("that ask was made already".to_owned()));
        }
        if let Some(newest) = entries.last()
            && state(directory, newest, now_unix_ms) == "making"
            && now_unix_ms.saturating_sub(newest["at_unix_ms"].as_u64().unwrap_or(0))
                < MAKING_LONGEST_MS
        {
            return (false, Err("a skill is still being made".to_owned()));
        }
        entries.push(json!({"request_id": request_id, "at_unix_ms": now_unix_ms}));
        while entries.len() > MOST_ASKS {
            entries.remove(0);
        }
        // Under the same lock: kept before the ask, so a lost write never
        // lets the same id in twice.
        seen.push(request_id.to_owned());
        while seen.len() > MOST_SEEN_IDS {
            seen.remove(0);
        }
        if write_seen(directory, &seen).is_err() {
            return (false, Err("it couldn't be kept on the lamp".to_owned()));
        }
        (true, Ok(()))
    });
    match admitted {
        Ok(Some(Ok(()))) => Ok(brief),
        Ok(Some(Err(why))) => Err(why),
        Ok(None) | Err(_) => Err("it couldn't be kept on the lamp".to_owned()),
    }
}

/// The ask `request_id` was handed over as the task `store`:`id`, or
/// wasn't, and why.
pub fn handed(directory: &Path, request_id: &str, outcome: Result<(String, u64), &str>) {
    let changed = transact(directory, false, |entries| {
        let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry["request_id"] == request_id)
        else {
            return (false, ());
        };
        match outcome {
            Ok((store, id)) => entry["task"] = json!(format!("{store}:{id}")),
            Err(why) => entry["not_kept"] = json!(why),
        }
        (true, ())
    });
    if let Err(error) = changed {
        eprintln!("lamp-voice: an app's skill: its ask not kept: {error}");
    }
}

/// Why an app's ask wasn't kept when the tasks part says its work was
/// never sent to the agent.
pub const NOT_SENT: &str = "the computer didn't take the work";
/// Why, when the computer has no agent the work could go to, or more than
/// one to choose between.
pub const NOT_SET_UP: &str = "the computer's agent isn't set up for the lamp yet";
/// Why, when the agent failed or stopped before it was made.
pub const STOPPED: &str = "the computer stopped before the skill was made";

/// The news `key` (`store:id:observation`) of a made skill not kept, and
/// why, in the maker's plain words ([`made_skill::Refused`]), never the
/// agent's: kept against the app's ask it came from, if it did.
pub fn not_kept(directory: &Path, key: &str, why: &str) {
    let changed = transact(directory, false, |entries| {
        let mut found = false;
        for entry in entries.iter_mut() {
            if entry["task"]
                .as_str()
                .is_some_and(|task| key.starts_with(&format!("{task}:")))
                && entry["not_kept"] != why
            {
                entry["not_kept"] = json!(why);
                found = true;
            }
        }
        (found, ())
    });
    if let Err(error) = changed {
        eprintln!("lamp-voice: an app's skill: why it wasn't kept not kept: {error}");
    }
}

/// Whether the app's ask whose task gave the answer `key`
/// (`store:id:observation`), if one did, is still being made: only then is
/// a refused skill repaired (#2813 piece 2). A skill asked for by voice has
/// no ask here, and is.
pub fn still_making(directory: &Path, key: &str, now_unix_ms: u64) -> bool {
    asks(directory)
        .iter()
        .find(|entry| {
            entry["task"]
                .as_str()
                .is_some_and(|task| key.starts_with(&format!("{task}:")))
        })
        .is_none_or(|entry| state(directory, entry, now_unix_ms) == "making")
}

/// The app's ask whose task gave the answer `key` follows its repair, the
/// task `to` (`store:id`): what becomes of the repair is the ask's.
pub fn repaired(directory: &Path, key: &str, to: &str) {
    let changed = transact(directory, false, |entries| {
        let mut found = false;
        for entry in entries.iter_mut() {
            if entry["task"]
                .as_str()
                .is_some_and(|task| key.starts_with(&format!("{task}:")))
            {
                entry["task"] = json!(to);
                found = true;
            }
        }
        (found, ())
    });
    if let Err(error) = changed {
        eprintln!("lamp-voice: an app's skill: its repair not kept: {error}");
    }
}

/// The made skill the app's ask `entry` became, if it is kept: the newest
/// from its task.
fn made_of(directory: &Path, entry: &Value) -> Option<Value> {
    let task = entry["task"].as_str()?;
    let prefix = format!("{task}:");
    made_skill::entries(directory)
        .into_iter()
        .filter(|made| {
            made["from"]
                .as_str()
                .is_some_and(|from| from.starts_with(&prefix))
        })
        .max_by_key(|made| made["at_unix_ms"].as_u64().unwrap_or(0))
}

/// `making`, `kept` (not yet asked about aloud), `asked` (the offer a
/// spoken yes would put on now: the newest, within its 120 s), `on`,
/// `left_off` (asked, and no longer that offer) or `not_kept`.
fn state(directory: &Path, entry: &Value, now_unix_ms: u64) -> &'static str {
    let Some(made) = made_of(directory, entry) else {
        return if entry.get("not_kept").is_some() {
            "not_kept"
        } else {
            "making"
        };
    };
    if made["on"] == true {
        return "on";
    }
    if made["offered_at_unix_ms"].as_u64().is_none() {
        return "kept";
    }
    // `asked` only while it is the offer a spoken yes would put on now: the
    // newest, in its time (#2874's `waiting`, the helper's own rule).
    match made_skill::waiting(directory, now_unix_ms) {
        Some((_, sha256)) if made["sha256"] == sha256 => "asked",
        _ => "left_off",
    }
}

/// What the app is told of its ask `request_id`, or of its newest: its
/// state ([`state`]), and once kept, the skill's name, what it does, its
/// version, what it may use and the SHA-256 the lamp will ask about; when
/// not kept, why, in plain words. Only the app's own asks: a skill made by
/// voice is never told here. `{"state": "none"}` with none.
pub fn status(directory: &Path, request_id: Option<&str>, now_unix_ms: u64) -> Value {
    let entries = asks(directory);
    let entry = match request_id {
        Some(id) => entries.iter().find(|entry| entry["request_id"] == id),
        None => entries.last(),
    };
    let Some(entry) = entry else {
        return json!({"state": "none"});
    };
    let state = state(directory, entry, now_unix_ms);
    let mut told = json!({"request_id": entry["request_id"], "state": state});
    if state == "not_kept" {
        told["why"] = entry["not_kept"].clone();
    } else if let Some(made) = made_of(directory, entry) {
        for field in ["name", "description", "version", "sha256", "uses"] {
            told[field] = made[field].clone();
        }
    }
    told
}

/// The group lamp-cloud runs as, by default: the socket is opened to it.
pub const DEFAULT_GROUP: &str = "lampos";

/// The socket's group: `value` ([`GROUP_ENVIRONMENT`]), or
/// [`DEFAULT_GROUP`] unset, by `groups` (`/etc/group`). One not known
/// starts nothing: never a socket nobody may reach (reviewer-two's P2 on
/// #2887).
pub fn group_named(value: Option<&str>, groups: &str) -> Result<u32, String> {
    let name = value
        .filter(|name| !name.is_empty())
        .unwrap_or(DEFAULT_GROUP);
    crate::say::group_id(groups, name).ok_or_else(|| format!("the group {name:?} is not known"))
}

/// Where the hand-over goes: the tasks part and the target.
#[derive(Clone, Debug)]
pub struct Place {
    pub tasks: PathBuf,
    pub target: String,
    /// The target is the default, not one set: one the tasks part hasn't
    /// is replaced by its one Claude target ([`crate::agent_tasks::hand_over`]).
    pub discover: bool,
    pub made: PathBuf,
}

/// One line asked, answered. A `make` recorded is handed over by `hand`
/// on a thread of its own, after the answer.
pub fn answer(
    place: &Place,
    line: &str,
    now_unix_ms: u64,
    hand: impl FnOnce(String, String) + Send + 'static,
) -> Value {
    let asked: Option<Value> = serde_json::from_str(line.trim_end()).ok();
    let only = |key: &str| {
        asked
            .as_ref()
            .and_then(Value::as_object)
            .filter(|asked| asked.len() == 1)
            .and_then(|asked| asked.get(key))
    };
    if let Some(make) = only("make") {
        let words = make["words"].as_str().unwrap_or_default();
        let request_id = make["request_id"].as_str().unwrap_or_default();
        return match begin(&place.made, words, request_id, now_unix_ms) {
            Ok(brief) => {
                let request_id = request_id.to_owned();
                if std::thread::Builder::new()
                    .name("lamp-voice-app-make".into())
                    .spawn(move || hand(brief, request_id))
                    .is_err()
                {
                    return json!({"refused": "it couldn't be started"});
                }
                json!({"making": true})
            }
            Err(why) => json!({"refused": why}),
        };
    }
    if let Some(made) = only("made") {
        return status(&place.made, made["request_id"].as_str(), now_unix_ms);
    }
    if only("ready").is_some() {
        return if crate::agent_tasks::has_a_target(&place.tasks, READY_WAIT) {
            json!({"ready": true})
        } else {
            json!({"ready": false, "why": NOT_SET_UP})
        };
    }
    json!({"refused": "a request is one line, {\"make\": {\"words\", \"request_id\"}}, {\"made\": {\"request_id\"}} or {\"ready\": {}}"})
}

/// The service, from the coordinator's environment: started only with
/// [`ENVIRONMENT`] `on`, `LAMP_MAKE_SKILL` `on` and the tasks part
/// (`LAMP_TASKS_DIR`). Logged either way, never the words.
pub fn start_from_environment() {
    if std::env::var(ENVIRONMENT).ok().as_deref() != Some("on") {
        return;
    }
    let Some(keeping) = made_skill::Keeping::from_environment() else {
        eprintln!(
            "lamp-voice: an app's skill: {ENVIRONMENT} needs {}=on; not started",
            made_skill::ENVIRONMENT
        );
        return;
    };
    let Some(tasks) = std::env::var_os(crate::agent_tasks::ENVIRONMENT)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    else {
        eprintln!("lamp-voice: an app's skill: no tasks part; not started");
        return;
    };
    let set = std::env::var(crate::agent_tasks::TARGET_ENVIRONMENT)
        .ok()
        .filter(|target| !target.is_empty());
    let discover = set.is_none();
    let target = set.unwrap_or_else(|| crate::agent_tasks::DEFAULT_TARGET.to_owned());
    let path = std::env::var_os(SOCKET_ENVIRONMENT)
        .map_or_else(|| PathBuf::from(DEFAULT_SOCKET), PathBuf::from);
    let groups = fs::read_to_string("/etc/group").unwrap_or_default();
    let group = match group_named(std::env::var(GROUP_ENVIRONMENT).ok().as_deref(), &groups) {
        Ok(id) => Some(id),
        Err(why) => {
            eprintln!("lamp-voice: an app's skill: {why}; not started");
            return;
        }
    };
    let place = Place {
        tasks,
        target,
        discover,
        made: keeping.directory,
    };
    match serve(&path, group, place) {
        Ok(()) => eprintln!(
            "lamp-voice: an app's skill: listening at {}",
            path.display()
        ),
        Err(error) => eprintln!(
            "lamp-voice: an app's skill: cannot listen at {}: {error}",
            path.display()
        ),
    }
}

/// Listen at `path`, opened to `group` if given, on a thread of its own:
/// each client in turn, its line read within [`LINE_WAIT`].
pub fn serve(path: &Path, group: Option<u32>, place: Place) -> io::Result<()> {
    let (listener, bound) = crate::say::bind(path, group)?;
    std::thread::Builder::new()
        .name("lamp-voice-app-make-socket".into())
        .spawn(move || {
            let _bound = bound;
            accept(&listener, &place);
        })?;
    Ok(())
}

fn accept(listener: &UnixListener, place: &Place) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        let _ = stream.set_read_timeout(Some(LINE_WAIT));
        let _ = stream.set_write_timeout(Some(LINE_WAIT));
        let mut line = String::new();
        if BufReader::new(Read::by_ref(&mut stream).take(MOST_LINE_BYTES))
            .read_line(&mut line)
            .is_err()
        {
            continue;
        }
        let handing = place.clone();
        let told = answer(place, &line, now_unix_ms(), move |brief, request_id| {
            let outcome = crate::agent_tasks::hand_over(
                &handing.tasks,
                &handing.target,
                handing.discover,
                &brief,
                "app-make",
                HAND_OVER_WAIT,
            );
            match &outcome {
                Ok(_) => eprintln!("lamp-voice: an app's skill: handed over"),
                Err(why) => eprintln!("lamp-voice: an app's skill: not handed over: {why}"),
            }
            handed(&handing.made, &request_id, outcome);
        });
        let _ = writeln!(stream, "{told}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    /// A folder of the test's own, empty.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("app-make-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn made_entry(from: &str, sha: &str) -> Value {
        json!({"from": from, "name": "evening-dim", "version": "1.0.0", "sha256": sha,
            "description": "Dims the light a shade at 10 pm.",
            "uses": {"light": true, "body": false, "topics": []},
            "package": "evening-dim-1.0.0.tar", "at_unix_ms": NOW})
    }

    fn write_made(directory: &Path, entries: &[Value]) {
        fs::create_dir_all(directory).unwrap();
        fs::write(
            directory.join(made_skill::INDEX),
            serde_json::to_vec(entries).unwrap(),
        )
        .unwrap();
    }

    /// An ask goes through its states: making, kept, asked within its
    /// 120 s, then left off; or on. The app is told the name, what it
    /// does, what it may use and the SHA-256, never the words.
    #[test]
    fn an_apps_ask_is_told_making_kept_asked_on_or_left_off() {
        let dir = folder("states");
        let made = dir.as_path();
        assert_eq!(status(made, None, NOW), json!({"state": "none"}));
        let brief = begin(
            made,
            "make me a skill that dims the light at 10 pm",
            "r1",
            NOW,
        )
        .unwrap();
        assert!(made_skill::is_brief(&brief));
        assert_eq!(status(made, Some("r1"), NOW)["state"], "making");
        handed(made, "r1", Ok(("s1".into(), 7)));
        assert_eq!(status(made, None, NOW)["state"], "making");
        let sha = "a".repeat(64);
        write_made(made, &[made_entry("s1:7:1", &sha)]);
        let told = status(made, Some("r1"), NOW);
        assert_eq!(told["state"], "kept");
        assert_eq!(told["name"], "evening-dim");
        assert_eq!(told["sha256"], sha);
        assert_eq!(told["uses"]["light"], true);
        assert!(!told.to_string().contains("10 pm\""), "no words: {told}");
        made_skill::offered(made, &sha, NOW + 1_000).unwrap();
        assert_eq!(status(made, None, NOW + 2_000)["state"], "asked");
        let within = made_skill::PUT_ON_WITHIN.as_millis() as u64;
        assert_eq!(
            status(made, None, NOW + 1_001 + within)["state"],
            "left_off"
        );
        // Within its 120 s but asked over by another made skill since: no
        // longer the offer a yes would put on, so left off (#2874's rule).
        let mut offered = made_entry("s1:7:1", &sha);
        offered["offered_at_unix_ms"] = json!(NOW + 1_000);
        let mut newer = made_entry("s1:9:1", &"c".repeat(64));
        newer["offered_at_unix_ms"] = json!(NOW + 1_500);
        write_made(made, &[offered, newer]);
        assert_eq!(status(made, None, NOW + 2_000)["state"], "left_off");
        let mut on = made_entry("s1:7:1", &sha);
        on["on"] = json!(true);
        write_made(made, &[on]);
        assert_eq!(status(made, None, NOW + 1_001 + within)["state"], "on");
    }

    /// Not handed over, or not kept: told why in plain words, and another
    /// may start. A made skill from the voice, not the app, is never told.
    #[test]
    fn an_ask_not_kept_says_why_and_only_the_apps_own_are_told() {
        let dir = folder("not-kept");
        let made = dir.as_path();
        write_made(made, &[made_entry("s1:3:1", &"b".repeat(64))]);
        assert_eq!(status(made, None, NOW), json!({"state": "none"}));
        begin(made, "make me a skill that dims the light", "r1", NOW).unwrap();
        handed(made, "r1", Err("the computer couldn't be reached"));
        let told = status(made, None, NOW);
        assert_eq!(
            (told["state"].as_str(), told["why"].as_str()),
            (Some("not_kept"), Some("the computer couldn't be reached"))
        );
        begin(made, "make me a skill that glows at dusk", "r2", NOW).unwrap();
        handed(made, "r2", Ok(("s1".into(), 9)));
        not_kept(made, "s1:9:2", "it doesn't pass the lamp's check");
        let told = status(made, Some("r2"), NOW);
        assert_eq!(told["state"], "not_kept");
        assert_eq!(told["why"], "it doesn't pass the lamp's check");
        // The voice's own made skill (task 3) is never the app's.
        assert!(told.get("name").is_none(), "{told}");
    }

    /// One at a time, a request id each, the words within the voice's own
    /// bound; an old ask still "making" lets another start.
    #[test]
    fn asks_are_one_at_a_time_and_bounded() {
        let dir = folder("bounded");
        let made = dir.as_path();
        assert!(begin(made, "make me a skill", "", NOW).is_err());
        assert!(begin(made, &"a".repeat(200), "r0", NOW).is_err());
        begin(made, "make me a skill that dims the light", "r1", NOW).unwrap();
        assert_eq!(
            begin(made, "make me another", "r2", NOW + 1).unwrap_err(),
            "a skill is still being made"
        );
        assert_eq!(
            begin(made, "make me another", "r1", NOW + 1).unwrap_err(),
            "that ask was made already"
        );
        assert!(begin(made, "make me another", "r2", NOW + MAKING_LONGEST_MS).is_ok());
        for n in 3..=8 {
            handed(made, &format!("r{}", n - 1), Err("x"));
            begin(
                made,
                "make me another",
                &format!("r{n}"),
                NOW + MAKING_LONGEST_MS,
            )
            .unwrap();
        }
        assert_eq!(asks(made).len(), MOST_ASKS);
        // An id sent again long after its ask left the five: still refused
        // (El's P3 on #2887); an id not as the voice takes one, said so.
        assert_eq!(
            begin(made, "make me another", "r1", NOW + 2 * MAKING_LONGEST_MS).unwrap_err(),
            "that ask was made already"
        );
        assert_eq!(
            begin(made, "make me another", "r 1", NOW).unwrap_err(),
            BAD_REQUEST_ID
        );
        assert_eq!(
            begin(made, "make me another", &"r".repeat(65), NOW).unwrap_err(),
            BAD_REQUEST_ID
        );
    }

    /// The socket's lines: a make answered at once and handed over after,
    /// with the brief and the request id; a status; anything else refused.
    #[test]
    fn the_socket_answers_a_make_at_once_and_hands_it_over_after() {
        let dir = folder("socket");
        let place = Place {
            tasks: dir.as_path().join("tasks"),
            target: "office".into(),
            discover: false,
            made: dir.as_path().join("made"),
        };
        let (sent, got) = std::sync::mpsc::channel();
        let told = answer(
            &place,
            r#"{"make": {"words": "make me a skill that dims the light", "request_id": "r1"}}"#,
            NOW,
            move |brief, id| sent.send((brief, id)).unwrap(),
        );
        assert_eq!(told, json!({"making": true}));
        let (brief, id) = got.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(made_skill::is_brief(&brief) && id == "r1");
        let told = answer(&place, r#"{"made": {}}"#, NOW, |_, _| {});
        assert_eq!(told["state"], "making");
        let told = answer(&place, r#"{"make": {"words": "x"}}"#, NOW, |_, _| {
            panic!("not handed")
        });
        assert!(told["refused"].is_string(), "{told}");
        let told = answer(&place, r#"{"install_made": "x"}"#, NOW, |_, _| {
            panic!("not handed")
        });
        assert!(told["refused"].as_str().unwrap().contains("one line"));
    }

    /// Through the socket itself: a make answered, handed over on its own
    /// thread (here to no tasks part, so not kept, and why), and the
    /// status read back on a second connection.
    #[test]
    fn through_the_socket_a_make_is_answered_and_its_outcome_kept() {
        let dir = folder("served");
        // Its own directory: the socket's is set to the voice's alone.
        let path = PathBuf::from(format!("/tmp/am-{}/make.sock", std::process::id()));
        let _ = fs::remove_dir_all(path.parent().unwrap());
        let place = Place {
            tasks: dir.join("no-tasks"),
            target: "office".into(),
            discover: false,
            made: dir.join("made"),
        };
        serve(&path, None, place).unwrap();
        let ask = |line: &str| {
            let mut stream = std::os::unix::net::UnixStream::connect(&path).unwrap();
            writeln!(stream, "{line}").unwrap();
            let mut told = String::new();
            BufReader::new(stream).read_line(&mut told).unwrap();
            serde_json::from_str::<Value>(&told).unwrap()
        };
        let told = ask(
            r#"{"make": {"words": "make me a skill that dims the light", "request_id": "r1"}}"#,
        );
        assert_eq!(told, json!({"making": true}));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut told = ask(r#"{"made": {"request_id": "r1"}}"#);
        while told["state"] == "making" && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            told = ask(r#"{"made": {"request_id": "r1"}}"#);
        }
        assert_eq!(told["state"], "not_kept", "{told}");
        assert_eq!(told["why"], "the computer couldn't be reached");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    /// `ready` (fourteen-P15): yes only when the tasks part answers
    /// `targets` with at least one; an empty list, a refusal or no tasks
    /// part at all is no, with why. The asks are never touched.
    #[test]
    fn ready_only_when_the_tasks_part_has_a_computer() {
        let dir = folder("ready");
        // A short folder of its own: a socket's path is at most 104 bytes.
        let tasks = PathBuf::from(format!("/tmp/amr-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tasks);
        fs::create_dir_all(&tasks).unwrap();
        let place = Place {
            tasks: tasks.clone(),
            target: "office".into(),
            discover: false,
            made: dir.join("made"),
        };
        let ready = || answer(&place, r#"{"ready": {}}"#, NOW, |_, _| panic!("not handed"));
        let not_ready = json!({"ready": false, "why": NOT_SET_UP});
        assert_eq!(ready(), not_ready, "no tasks part");
        let listener = UnixListener::bind(tasks.join("request.sock")).unwrap();
        let answers = [
            json!({"ok": true, "targets": [{"alias": "office", "engine": "claude"}]}),
            json!({"ok": true, "targets": []}),
            json!({"ok": false, "error": "busy"}),
        ];
        let served = answers.clone();
        std::thread::spawn(move || {
            for (stream, told) in listener.incoming().zip(served) {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                assert_eq!(
                    serde_json::from_str::<Value>(&line).unwrap(),
                    json!({"op": "targets"})
                );
                writeln!(stream, "{told}").unwrap();
            }
        });
        assert_eq!(ready(), json!({"ready": true}), "a computer");
        assert_eq!(ready(), not_ready, "no computer");
        assert_eq!(ready(), not_ready, "refused");
        assert!(!place.made.join(APP_INDEX).exists(), "no ask written");
        let _ = fs::remove_dir_all(&tasks);
    }

    /// Writers that overlap (reviewer-two's P2 on #2887): an old ask's
    /// late refusal, a new ask admitted, and its hand-over, each from its
    /// own thread, many times over. Every identity survives each time, and
    /// the index stays whole.
    #[test]
    fn overlapping_writers_never_lose_an_ask() {
        for round in 0..100 {
            let dir = folder("overlap");
            let made = dir.as_path();
            begin(made, "make me a skill that dims the light", "r1", NOW).unwrap();
            handed(made, "r1", Ok(("s1".into(), 1)));
            let late = made.to_path_buf();
            let refusal = std::thread::spawn(move || {
                not_kept(&late, "s1:1:hand-over", NOT_SENT);
            });
            // Past the 30 minutes, so admitted whichever writes first.
            let newer = made.to_path_buf();
            let admission = std::thread::spawn(move || {
                begin(
                    &newer,
                    "make me a skill that glows",
                    "r2",
                    NOW + MAKING_LONGEST_MS,
                )
                .unwrap();
                handed(&newer, "r2", Ok(("s1".into(), 2)));
            });
            refusal.join().unwrap();
            admission.join().unwrap();
            let entries = asks(made);
            assert_eq!(entries.len(), 2, "round {round}: {entries:?}");
            assert_eq!(
                entries[0]["not_kept"], NOT_SENT,
                "round {round}: {entries:?}"
            );
            assert_eq!(entries[1]["task"], "s1:2", "round {round}: {entries:?}");
        }
    }

    /// The socket's group: `lampos` unset, the one named otherwise, and
    /// none known starts nothing (reviewer-two's P2 on #2887).
    #[test]
    fn the_socket_is_opened_to_lampos_unless_another_group_is_named() {
        let groups = "root:x:0:\nlampos:x:990:\nstaff:x:50:\n";
        assert_eq!(group_named(None, groups), Ok(990));
        assert_eq!(group_named(Some(""), groups), Ok(990));
        assert_eq!(group_named(Some("staff"), groups), Ok(50));
        assert_eq!(
            group_named(Some("nobody-here"), groups),
            Err("the group \"nobody-here\" is not known".to_owned())
        );
        assert!(group_named(None, "root:x:0:\n").is_err());
    }
}
