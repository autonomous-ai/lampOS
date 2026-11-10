//! A skill typed in the app (#2813; the first builder on #2602, 22 Oct
//! 10:3x): the person's words go to the same maker the voice uses, the
//! lamp's own model ([`crate::make_skill_local`], #4914; the only maker
//! since 3 Nov: no tasks part and no computer), and the skill goes on as
//! soon as it is kept and checked, with no spoken yes (#5977; the owner, 10 Nov: a
//! made skill goes on with no spoken yes; the app's MAKE IT and the voice
//! ask are the ask). Off unless [`ENVIRONMENT`] is `on`, and only with `LAMP_MAKE_SKILL` and
//! the say service (`LAMP_SAY`).
//!
//! - **The socket** ([`SOCKET_ENVIRONMENT`], [`DEFAULT_SOCKET`] if unset),
//!   for `lamp-cloud`: one JSON line in, one out, then closed.
//!   - `{"make": {"words", "request_id"}}`: the words become the voice's
//!     own brief ([`crate::made_skill::brief`], at most 480 bytes) and are
//!     made at once on a thread of their own: typing them in the app is the
//!     person's ask. The answer is `{"making": true}` once the ask is
//!     recorded, before it is made, or `{"refused": WHY}` in plain words.
//!   - `{"made": {"request_id"?}}`: the state of that ask, or of the
//!     newest, only ever one the app made ([`status`]).
//!   - `{"ready": {}}`: `{"ready": true}`, at once: the lamp makes it
//!     itself. It reads nothing of the asks.
//!   - `{"counts": {}}`: what became of the skills made by voice, running
//!     totals ([`crate::made_counts::counts`]): numbers for the opt-in
//!     health report, never a name, a word or a package.
//!   - `{"upload": {"skill_md", "skill_toml"?}}`: a skill added from a file
//!     in the app, with `LAMP_SKILL_UPLOAD=on` ([`crate::upload_skill`]):
//!     `{"kept": {...}}` once checked and kept, then put on at once and
//!     said aloud as a made-here skill is, or `{"refused": WHY}`.
//!   - `{"written": {"name", "when", "instructions", "request_id"}}`, only
//!     with [`made_skill::WRITTEN_ENVIRONMENT`] `on` (its own switch; a
//!     lamp may have it alone): a skill written in the app, held to its
//!     bounds ([`begin_written`]), kept word for word with no model
//!     ([`write_here`]) and put on at once as a made one is.
//! - **Made here.** A `make` is written by the lamp's own chat with the
//!   voice's own example, check and one repair
//!   ([`crate::make_skill_local::make_kept`], [`make_here`]), kept as made
//!   from [`LOCAL_STORE`], and once kept it is put on at once
//!   (`made_skill::put_on`, and the manage helper's own check of the mark
//!   it makes, `made_skill::going_on`). The lamp then says one short line
//!   through the say service, at a good moment: `made_skill::on_line` ("I
//!   made evening dim, and it's on."), or `made_skill::not_on_line` with
//!   why it isn't on. No ask is said and no yes is waited for.
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

/// A client has this long to send its line, and the line is at most
/// [`MOST_LINE_BYTES`]: room for a skill added from a file's two files,
/// each at most 8 KiB, escaped (`crate::upload_skill`).
const LINE_WAIT: Duration = Duration::from_secs(2);
/// It also holds a written skill's 2,000 bytes of instructions and 200 of
/// its `when` ([`begin_written`]).
const MOST_LINE_BYTES: u64 = 64 * 1_024;
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
/// made skills' `directory`: the brief to make, or why not, in plain
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
    admit(directory, request_id, now_unix_ms, false)?;
    Ok(brief)
}

/// The app's ask `request_id` recorded at `now_unix_ms`, `written` if it is
/// a skill written in the app ([`begin_written`]); or why not. One at a
/// time, and each request id once.
fn admit(
    directory: &Path,
    request_id: &str,
    now_unix_ms: u64,
    written: bool,
) -> Result<(), String> {
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
        let mut entry = json!({"request_id": request_id, "at_unix_ms": now_unix_ms});
        if written {
            entry["written"] = json!(true);
        }
        entries.push(entry);
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
        Ok(Some(Ok(()))) => Ok(()),
        Ok(Some(Err(why))) => Err(why),
        Ok(None) | Err(_) => Err("it couldn't be kept on the lamp".to_owned()),
    }
}

/// Record the app's ask `request_id` to keep a skill written in the app
/// (`{"written": {"name", "when", "instructions", "request_id"}}`, with
/// [`made_skill::WRITTEN_ENVIRONMENT`] on) at `now_unix_ms`: the skill held
/// to its bounds ([`made_skill::Written::new`]), or why not, in plain words.
/// One at a time with the app's other asks, each request id once.
pub fn begin_written(
    directory: &Path,
    written: &Value,
    now_unix_ms: u64,
) -> Result<(made_skill::Written, String), String> {
    let field = |key: &str| written[key].as_str().unwrap_or_default();
    let request_id = field("request_id");
    if request_id.is_empty() {
        return Err("the ask needs a request id".into());
    }
    if !request_id_ok(request_id) {
        return Err(BAD_REQUEST_ID.into());
    }
    let skill = made_skill::Written::new(field("name"), field("when"), field("instructions"))?;
    admit(directory, request_id, now_unix_ms, true)?;
    Ok((skill, request_id.to_owned()))
}

/// The ask `request_id` is being made as `task` (`local:app:<ms>`): what
/// it makes is kept as made from it ([`made_of`]).
pub fn handed(directory: &Path, request_id: &str, task: &str) {
    let changed = transact(directory, false, |entries| {
        let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry["request_id"] == request_id)
        else {
            return (false, ());
        };
        entry["task"] = json!(task);
        (true, ())
    });
    if let Err(error) = changed {
        eprintln!("lamp-voice: an app's skill: its ask not kept: {error}");
    }
}

/// The linked services the ask `request_id`'s skill reads that weren't
/// linked when it was made (`made_skill::needs_link`, empty with none),
/// kept against the ask: told to the app as `needs_link` ([`status`]).
pub fn link_noted(directory: &Path, request_id: &str, needs: &[Value]) {
    let changed = transact(directory, false, |entries| {
        let Some(entry) = entries
            .iter_mut()
            .find(|entry| entry["request_id"] == request_id)
        else {
            return (false, ());
        };
        entry["needs_link"] = json!(needs);
        (true, ())
    });
    match changed {
        Ok(_) if !needs.is_empty() => eprintln!(
            "lamp-voice: an app's skill: {} service(s) it reads not linked yet",
            needs.len()
        ),
        Ok(_) => {}
        Err(error) => {
            eprintln!("lamp-voice: an app's skill: what it needs linked not kept: {error}")
        }
    }
}

/// The made skill `key` (`local:app:<ms>:1`) not kept, and why, in the
/// maker's plain words ([`made_skill::Refused`]): kept against the app's
/// ask it came from.
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

/// `making` (being made, or kept and being put on), `on`, or `not_kept`
/// (not made, not kept, or kept and not put on: [`why_not`] says why). A
/// kept skill goes on at once, with no spoken yes (the owner, 10 Nov), so
/// a new ask is never `kept`, `asked` or `left_off`; only a skill asked
/// about before that, never said yes to, still reads `left_off`.
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
    if made.get("not_on").is_some() {
        return "not_kept";
    }
    // Asked about aloud before 10 Nov and never said yes to.
    if made["offered_at_unix_ms"].as_u64().is_some() {
        return "left_off";
    }
    // Kept, and put on next: being put on while the ask is fresh; past
    // that, never put on (the lamp stopped between the two).
    if now_unix_ms.saturating_sub(entry["at_unix_ms"].as_u64().unwrap_or(0)) < MAKING_LONGEST_MS {
        "making"
    } else {
        "not_kept"
    }
}

/// Why the app's ask `entry` is `not_kept`, in plain words: why it wasn't
/// made or kept, or why the kept skill `made` isn't on.
fn why_not(entry: &Value, made: Option<&Value>) -> Value {
    if let Some(why) = entry.get("not_kept") {
        return why.clone();
    }
    match made.and_then(|made| made["not_on"].as_str()) {
        Some(why) => json!(format!("it couldn't be put on: {why}")),
        None => json!("it wasn't put on"),
    }
}

/// What the app is told of its ask `request_id`, or of its newest: its
/// state ([`state`]), and once on, the skill's name, what it does, its
/// version, what it may use and its SHA-256; when not kept or not on, why,
/// in plain words ([`why_not`]). Only the app's own asks: a skill made by
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
    // Written in the app, never the Store's nor made by a model.
    if entry["written"] == true {
        told["written"] = json!(true);
    }
    let made = made_of(directory, entry);
    if state == "not_kept" {
        told["why"] = why_not(entry, made.as_ref());
    } else if let Some(made) = made {
        for field in ["name", "description", "version", "sha256", "uses"] {
            told[field] = made[field].clone();
        }
        // The linked services it reads that weren't linked when it was
        // made, `[{"code", "name"}]`; absent when that couldn't be told.
        if entry["needs_link"].is_array() {
            told["needs_link"] = entry["needs_link"].clone();
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

/// What an ask made here is kept as made from: `local:app:<ms>:1`, the
/// ask's own task `local:app:<ms>`, so [`made_of`] finds it.
pub const LOCAL_STORE: &str = "local:app";
/// The longest the line said once it is on waits for a good moment: an
/// hour, the say service's longest.
pub const LINE_WAITS_MS: u64 = crate::say::LONGEST_EXPIRY_MS;

/// The app's ask `request_id`, its `brief` recorded by [`begin`] at
/// `at_unix_ms`, made by the lamp's own model (`chat`) with the voice's
/// own example, check and one repair, kept in `keeping`, and put on at
/// once through its manage helper ([`made_skill::put_on`]), with no spoken
/// yes (the owner, 10 Nov); then `say` is given the line the lamp says
/// aloud ([`made_skill::on_line`], or [`made_skill::not_on_line`] with why
/// it isn't on). Not kept: why, for the app, and nothing is said. Blocking.
///
/// Kept, a skill that reads a linked service is checked against what
/// `lamp-services` says is linked now ([`crate::services::linked_codes`]),
/// and what isn't is kept against the ask for the app, its `needs_link`
/// ([`link_noted`]); a check that can't be made keeps nothing, and the
/// make stands either way.
pub fn make_here(
    keeping: &made_skill::Keeping,
    brief: &str,
    request_id: &str,
    at_unix_ms: u64,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    say: impl FnOnce(&str),
) {
    make_here_with(keeping, brief, request_id, at_unix_ms, chat, say, || {
        crate::services::linked_codes(Path::new(crate::services::FOLDER))
    })
}

/// [`make_here`], what is linked given by `linked`.
pub fn make_here_with(
    keeping: &made_skill::Keeping,
    brief: &str,
    request_id: &str,
    at_unix_ms: u64,
    chat: impl Fn() -> Result<Chat, String> + Send + Sync + 'static,
    say: impl FnOnce(&str),
    linked: impl FnOnce() -> Option<Vec<String>>,
) {
    handed(
        &keeping.directory,
        request_id,
        &format!("{LOCAL_STORE}:{at_unix_ms}"),
    );
    let from = format!("{LOCAL_STORE}:{at_unix_ms}:1");
    match crate::make_skill_local::make_kept(
        made_skill::request_of(brief),
        brief.to_owned(),
        keeping,
        chat,
        &from,
    ) {
        Ok(made) => {
            if let Some(needs) = made_skill::needs_link(&made.uses, linked) {
                link_noted(&keeping.directory, request_id, &needs);
            }
            eprintln!("lamp-voice: an app's skill: made here and kept; put on now");
            say(&put_on_line(keeping, &made));
        }
        Err(made_skill::Refused(why, _)) => {
            // An app's ask that made nothing, counted on the lamp as the
            // voice's are, so the backend never counts the app's makes
            // (`crate::made_counts`).
            crate::made_counts::refused(&keeping.directory, made_skill::Maker::App);
            not_kept(&keeping.directory, &from, &why)
        }
    }
}

/// The kept skill `made` put on at once ([`made_skill::put_on`]), and the
/// one short line the lamp says of it: on, or why not.
fn put_on_line(keeping: &made_skill::Keeping, made: &made_skill::Made) -> String {
    match made_skill::put_on(keeping, made) {
        Ok(()) => made_skill::on_line(made),
        Err(why) => made_skill::not_on_line(&why),
    }
}

/// What a skill written in the app is kept as made from:
/// `written:app:<ms>:1`, the ask's own task `written:app:<ms>`, so
/// [`made_of`] finds it.
pub const WRITTEN_STORE: &str = "written:app";

/// The app's ask `request_id` to keep `written`, recorded by
/// [`begin_written`] at `at_unix_ms`: its files written here from the
/// person's words, checked and kept ([`made_skill::keep_written`], no
/// model), put on at once as a made one is ([`made_skill::put_on`], the
/// owner, 10 Nov), and `say` given the one short line the lamp says aloud
/// of it. Not kept: why, for the app, and nothing is said. Blocking.
pub fn write_here(
    keeping: &made_skill::Keeping,
    written: &made_skill::Written,
    request_id: &str,
    at_unix_ms: u64,
    installed: &Path,
    say: impl FnOnce(&str),
) {
    handed(
        &keeping.directory,
        request_id,
        &format!("{WRITTEN_STORE}:{at_unix_ms}"),
    );
    let from = format!("{WRITTEN_STORE}:{at_unix_ms}:1");
    match made_skill::keep_written(written, &from, keeping, installed) {
        Ok(made) => {
            eprintln!("lamp-voice: a written skill: kept; put on now");
            say(&put_on_line(keeping, &made));
        }
        Err(made_skill::Refused(why, _)) => {
            eprintln!("lamp-voice: a written skill: not kept: {why}");
            not_kept(&keeping.directory, &from, &why);
        }
    }
}

/// `line`, said through the say service's own asks, as written, at a good
/// moment within [`LINE_WAITS_MS`]; what became of it logged. Nothing
/// waits on its answer: the skill is on, or not, already.
pub fn say_line(asks: &crate::say::OwnAsks, line: &str) {
    let answered = asks.text("app-make", line, crate::say::NORMAL_PRIORITY, LINE_WAITS_MS);
    while let Ok(answer) = answered.recv() {
        if let crate::say::Answer::Done(outcome) = answer {
            eprintln!("lamp-voice: an app's skill: its line: {outcome:?}");
            return;
        }
    }
}

/// The lamp's own chat, as [`crate::make_skill_local::own_chat`] gives it.
type Chat = crate::second_provider::net::Chat;
/// What gives the chat a skill is made with.
pub type ChatFrom = fn() -> Result<Chat, String>;

/// Which of the socket's asks are answered: `make` and `ready` with
/// [`ENVIRONMENT`] on, `written` with [`made_skill::WRITTEN_ENVIRONMENT`]
/// on. Each switch alone answers only its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct On {
    pub make: bool,
    pub written: bool,
}

/// What an ask recorded becomes, on a thread of its own: a brief to make
/// a skill from, or a skill written in the app to keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Make(String),
    Written(made_skill::Written),
}

/// One line asked, answered, of the asks kept in `made`, as far as `on`
/// lets. A `make` or a `written` recorded is handed to `hand` with its
/// request id, on a thread of its own, after the answer.
pub fn answer(
    made: &Path,
    on: On,
    line: &str,
    now_unix_ms: u64,
    hand: impl FnOnce(Asked, String) + Send + 'static,
) -> Value {
    let asked: Option<Value> = serde_json::from_str(line.trim_end()).ok();
    let only = |key: &str| {
        asked
            .as_ref()
            .and_then(Value::as_object)
            .filter(|asked| asked.len() == 1)
            .and_then(|asked| asked.get(key))
    };
    let start = |asked: Asked, request_id: String| {
        if std::thread::Builder::new()
            .name("lamp-voice-app-make".into())
            .spawn(move || hand(asked, request_id))
            .is_err()
        {
            return json!({"refused": "it couldn't be started"});
        }
        json!({"making": true})
    };
    if let Some(written) = only("written") {
        if !on.written {
            return json!({"refused": "a skill written in the app isn't on for this lamp"});
        }
        return match begin_written(made, written, now_unix_ms) {
            Ok((skill, request_id)) => start(Asked::Written(skill), request_id),
            Err(why) => json!({"refused": why}),
        };
    }
    // A lamp with written skills alone makes none from words.
    if !on.make {
        let not_on = "making a skill isn't on for this lamp";
        if only("ready").is_some() {
            return json!({"ready": false, "why": not_on});
        }
        if only("make").is_some() {
            return json!({"refused": not_on});
        }
    }
    if let Some(ask) = only("make") {
        let words = ask["words"].as_str().unwrap_or_default();
        let request_id = ask["request_id"].as_str().unwrap_or_default();
        return match begin(made, words, request_id, now_unix_ms) {
            Ok(brief) => start(Asked::Make(brief), request_id.to_owned()),
            Err(why) => json!({"refused": why}),
        };
    }
    if let Some(asked) = only("made") {
        return status(made, asked["request_id"].as_str(), now_unix_ms);
    }
    // Made here: ready at once, with no computer to wait for.
    if only("ready").is_some() {
        return json!({"ready": true});
    }
    // Numbers for lamp-cloud's opt-in health report, never a name or word.
    if only("counts").is_some() {
        return crate::made_counts::counts(made);
    }
    json!({"refused": "a request is one line, {\"make\": {\"words\", \"request_id\"}}, {\"written\": {\"name\", \"when\", \"instructions\", \"request_id\"}}, {\"made\": {\"request_id\"}}, {\"ready\": {}} or {\"counts\": {}}"})
}

/// The service, from the coordinator's environment: started with
/// [`ENVIRONMENT`] `on`, [`made_skill::WRITTEN_ENVIRONMENT`] `on` or both,
/// each answering only its own asks ([`On`]), and always with
/// `LAMP_MAKE_SKILL` `on` and the say service's own `asks`, to say aloud what it made.
/// Logged either way, never the words.
pub fn start_from_environment(asks: Option<crate::say::OwnAsks>) {
    let switch = |name: &str| std::env::var(name).ok().as_deref() == Some("on");
    let on = On {
        make: switch(ENVIRONMENT),
        written: switch(made_skill::WRITTEN_ENVIRONMENT),
    };
    if !on.make && !on.written {
        return;
    }
    let Some(keeping) = made_skill::Keeping::from_environment() else {
        eprintln!(
            "lamp-voice: an app's skill: {ENVIRONMENT} and {} need {}=on; not started",
            made_skill::WRITTEN_ENVIRONMENT,
            made_skill::ENVIRONMENT
        );
        return;
    };
    let Some(asks) = asks else {
        eprintln!(
            "lamp-voice: an app's skill: {ENVIRONMENT} and {} need the say service ({}=on) to say aloud what it made; not started",
            made_skill::WRITTEN_ENVIRONMENT,
            crate::say::ENVIRONMENT
        );
        return;
    };
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
    match serve(
        &path,
        group,
        on,
        keeping,
        Some(asks),
        crate::make_skill_local::own_chat,
        PathBuf::from(crate::skill_words::INSTALLED),
        app_caller_is_lamp_cloud,
    ) {
        Ok(()) => eprintln!(
            "lamp-voice: an app's skill: listening at {} (made: {}, written: {})",
            path.display(),
            on.make,
            on.written
        ),
        Err(error) => eprintln!(
            "lamp-voice: an app's skill: cannot listen at {}: {error}",
            path.display()
        ),
    }
}

/// The one unit the app-make socket answers (App Only's P2 on #5985).
pub const APP_CALLER_UNIT: &str = "lamp-cloud.service";

/// Whether a caller the kernel named may use the app-make socket: only
/// [`APP_CALLER_UNIT`] run as `own_uid`, as the tasks part's app route
/// holds it (#5379); a caller whose unit can't be read is refused.
pub fn app_caller_may(caller: Option<&lamp_ipc::caller::Caller>, own_uid: u32) -> bool {
    caller.is_some_and(|caller| {
        caller.uid == own_uid && caller.unit.as_deref() == Some(APP_CALLER_UNIT)
    })
}

/// [`app_caller_may`] with this process's own uid: the socket's check on a
/// lamp.
pub fn app_caller_is_lamp_cloud(caller: Option<&lamp_ipc::caller::Caller>) -> bool {
    app_caller_may(caller, rustix::process::geteuid().as_raw())
}

/// Listen at `path`, opened to `group` if given, on a thread of its own:
/// each client in turn, its line read within [`LINE_WAIT`], answered as far
/// as `on` lets; each make written by the model `chat` gives, each written
/// skill kept taking no name of a skill in `installed`, each put on at
/// once, and its line said through `asks`.
#[allow(clippy::too_many_arguments)]
pub fn serve(
    path: &Path,
    group: Option<u32>,
    on: On,
    keeping: made_skill::Keeping,
    asks: Option<crate::say::OwnAsks>,
    chat: ChatFrom,
    installed: PathBuf,
    caller_may: fn(Option<&lamp_ipc::caller::Caller>) -> bool,
) -> io::Result<()> {
    let (listener, bound) = crate::say::bind(path, group)?;
    std::thread::Builder::new()
        .name("lamp-voice-app-make-socket".into())
        .spawn(move || {
            let _bound = bound;
            accept(
                &listener,
                on,
                &keeping,
                asks.as_ref(),
                chat,
                &installed,
                caller_may,
            );
        })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn accept(
    listener: &UnixListener,
    on: On,
    keeping: &made_skill::Keeping,
    asks: Option<&crate::say::OwnAsks>,
    chat: ChatFrom,
    installed: &Path,
    caller_may: fn(Option<&lamp_ipc::caller::Caller>) -> bool,
) {
    // A skill added from a file (`LAMP_SKILL_UPLOAD=on`), on this socket.
    let upload = crate::upload_skill::from_environment();
    let upload_place = crate::upload_skill::Place::product();
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else {
            continue;
        };
        // Only lamp-cloud, for the app (App Only's P2 on #5985): with no
        // spoken yes, any other part in the socket's group could put a
        // skill on with no person; closed with no answer.
        let caller = lamp_ipc::caller::of_stream(&stream);
        if !caller_may(caller.as_ref()) {
            eprintln!(
                "lamp-voice: the app-make socket refused a caller that isn't {APP_CALLER_UNIT}"
            );
            continue;
        }
        let _ = stream.set_read_timeout(Some(LINE_WAIT));
        let _ = stream.set_write_timeout(Some(LINE_WAIT));
        let mut line = String::new();
        if BufReader::new(Read::by_ref(&mut stream).take(MOST_LINE_BYTES))
            .read_line(&mut line)
            .is_err()
        {
            continue;
        }
        if let Some((told, kept)) = serde_json::from_str::<Value>(line.trim_end())
            .ok()
            .and_then(|asked| {
                crate::upload_skill::line(
                    &asked,
                    upload,
                    asks.is_some(),
                    &upload_place,
                    keeping,
                    now_unix_ms(),
                )
            })
        {
            let _ = writeln!(stream, "{told}");
            // Kept: put on at once, after the answer, then said.
            if let (Some(made), Some(asks)) = (kept, asks.cloned()) {
                let keeping = keeping.clone();
                let _ = std::thread::Builder::new()
                    .name("lamp-voice-upload-on".into())
                    .spawn(move || {
                        let line = match made_skill::put_on(&keeping, &made) {
                            Ok(()) => crate::upload_skill::on_line(&made),
                            Err(why) => crate::upload_skill::not_on_line(&why),
                        };
                        say_line(&asks, &line);
                    });
            }
            continue;
        }
        let making = keeping.clone();
        let asks = asks.cloned();
        let installed = installed.to_path_buf();
        let told = answer(
            &keeping.directory,
            on,
            &line,
            now_unix_ms(),
            move |asked, request_id| {
                let say = |line: &str| {
                    if let Some(asks) = &asks {
                        say_line(asks, line);
                    }
                };
                match asked {
                    Asked::Make(brief) => {
                        make_here(&making, &brief, &request_id, now_unix_ms(), chat, say);
                    }
                    Asked::Written(written) => write_here(
                        &making,
                        &written,
                        &request_id,
                        now_unix_ms(),
                        &installed,
                        say,
                    ),
                }
            },
        );
        let _ = writeln!(stream, "{told}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// App Only's P2 on #5985: with no spoken yes, only lamp-cloud (its
    /// unit, run as the voice's own user) may use the app-make socket; any
    /// other part, another user or a caller not read is refused.
    #[test]
    fn only_lamp_cloud_may_ask_the_app_make_socket() {
        let caller = |unit: Option<&str>, uid| lamp_ipc::caller::Caller {
            pid: 1,
            uid,
            unit: unit.map(str::to_owned),
        };
        assert!(app_caller_may(
            Some(&caller(Some(APP_CALLER_UNIT), 990)),
            990
        ));
        assert!(!app_caller_may(
            Some(&caller(Some("lamp-tasks.service"), 990)),
            990
        ));
        assert!(!app_caller_may(
            Some(&caller(Some(APP_CALLER_UNIT), 0)),
            990
        ));
        assert!(!app_caller_may(Some(&caller(None, 990)), 990));
        assert!(!app_caller_may(None, 990));
    }

    const NOW: u64 = 1_800_000_000_000;
    /// Making from words on, written skills off.
    const MAKE: On = On {
        make: true,
        written: false,
    };

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

    /// An ask goes through its states: making (made, then kept and being
    /// put on), then on, or not kept with why: a kept skill goes on at
    /// once, with no spoken yes (the owner, 10 Nov), so a new ask is never
    /// `kept`, `asked` or `left_off`. Only one asked about before that,
    /// never said yes to, still reads `left_off`. The app is told the
    /// name, what it does, what it may use and the SHA-256, never the words.
    #[test]
    fn an_apps_ask_is_told_making_then_on_or_not_kept_with_why() {
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
        handed(made, "r1", "s1:7");
        assert_eq!(status(made, None, NOW)["state"], "making");
        let sha = "a".repeat(64);
        // Kept, and being put on.
        write_made(made, &[made_entry("s1:7:1", &sha)]);
        let told = status(made, Some("r1"), NOW + 1_000);
        assert_eq!(told["state"], "making");
        assert_eq!(told["name"], "evening-dim");
        assert_eq!(told["sha256"], sha);
        assert_eq!(told["uses"]["light"], true);
        assert!(!told.to_string().contains("10 pm\""), "no words: {told}");
        // Never put on (the lamp stopped between the two): not kept.
        let told = status(made, Some("r1"), NOW + MAKING_LONGEST_MS);
        assert_eq!(
            (told["state"].as_str(), told["why"].as_str()),
            (Some("not_kept"), Some("it wasn't put on"))
        );
        // Kept and not put on: not kept, and why.
        let mut not_on = made_entry("s1:7:1", &sha);
        not_on["not_on"] = json!("the lamp's check didn't let it on");
        write_made(made, &[not_on]);
        let told = status(made, Some("r1"), NOW + 1_000);
        assert_eq!(
            (told["state"].as_str(), told["why"].as_str()),
            (
                Some("not_kept"),
                Some("it couldn't be put on: the lamp's check didn't let it on")
            )
        );
        assert!(told.get("name").is_none(), "{told}");
        // Asked about before 10 Nov and never said yes to: left off.
        let mut offered = made_entry("s1:7:1", &sha);
        offered["offered_at_unix_ms"] = json!(NOW + 1_000);
        write_made(made, &[offered]);
        assert_eq!(status(made, None, NOW + 2_000)["state"], "left_off");
        let mut on = made_entry("s1:7:1", &sha);
        on["on"] = json!(true);
        write_made(made, &[on]);
        assert_eq!(status(made, None, NOW + 2_000)["state"], "on");
    }

    /// Not made, or not kept: told why in plain words, and another may
    /// start. A made skill from the voice, not the app, is never told.
    #[test]
    fn an_ask_not_kept_says_why_and_only_the_apps_own_are_told() {
        let dir = folder("not-kept");
        let made = dir.as_path();
        write_made(made, &[made_entry("local:3:1", &"b".repeat(64))]);
        assert_eq!(status(made, None, NOW), json!({"state": "none"}));
        begin(made, "make me a skill that dims the light", "r1", NOW).unwrap();
        handed(made, "r1", "local:app:1");
        not_kept(made, "local:app:1:1", "the lamp's model didn't answer");
        let told = status(made, None, NOW);
        assert_eq!(
            (told["state"].as_str(), told["why"].as_str()),
            (Some("not_kept"), Some("the lamp's model didn't answer"))
        );
        begin(made, "make me a skill that glows at dusk", "r2", NOW).unwrap();
        handed(made, "r2", "local:app:9");
        not_kept(made, "local:app:9:1", "it doesn't pass the lamp's check");
        let told = status(made, Some("r2"), NOW);
        assert_eq!(told["state"], "not_kept");
        assert_eq!(told["why"], "it doesn't pass the lamp's check");
        // The voice's own made skill (`local:3`) is never the app's.
        assert!(told.get("name").is_none(), "{told}");
    }

    /// A skill naming a linked service is refused in the person's own
    /// words, shown in the app as they are (the read of #5290).
    #[test]
    fn a_skill_using_a_service_is_refused_in_plain_words() {
        let dir = folder("service");
        let refused = begin(
            dir.as_path(),
            "a skill that checks my calendar every evening",
            "r1",
            NOW,
        )
        .unwrap_err();
        assert_eq!(refused, made_skill::NOT_A_SERVICE);
        assert!(!refused.contains("Say so"), "{refused}");
    }

    /// One at a time, a request id each, the words within the voice's own
    /// bound; an old ask still "making" lets another start.
    #[test]
    fn asks_are_one_at_a_time_and_bounded() {
        let dir = folder("bounded");
        let made = dir.as_path();
        assert!(begin(made, "make me a skill", "", NOW).is_err());
        let long = "a".repeat(made_skill::MOST_REQUEST_BYTES + 1);
        assert!(begin(made, &long, "r0", NOW).is_err());
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
            let task = format!("local:app:{n}");
            handed(made, &format!("r{}", n - 1), &task);
            not_kept(made, &format!("{task}:1"), "x");
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

    /// The socket's lines: a make answered at once and made after, with
    /// the brief and the request id; a status; `ready` at once; anything
    /// else refused.
    #[test]
    fn the_socket_answers_a_make_at_once_and_makes_it_after() {
        let dir = folder("socket");
        let place = dir.as_path().join("made");
        let (sent, got) = std::sync::mpsc::channel();
        let told = answer(
            &place,
            MAKE,
            r#"{"make": {"words": "make me a skill that dims the light", "request_id": "r1"}}"#,
            NOW,
            move |asked, id| sent.send((asked, id)).unwrap(),
        );
        assert_eq!(told, json!({"making": true}));
        let (asked, id) = got.recv_timeout(Duration::from_secs(5)).unwrap();
        let Asked::Make(brief) = asked else {
            panic!("a make: {asked:?}")
        };
        assert!(made_skill::is_brief(&brief) && id == "r1");
        let told = answer(&place, MAKE, r#"{"made": {}}"#, NOW, |_, _| {});
        assert_eq!(told["state"], "making");
        let told = answer(&place, MAKE, r#"{"make": {"words": "x"}}"#, NOW, |_, _| {
            panic!("not handed")
        });
        assert!(told["refused"].is_string(), "{told}");
        let told = answer(&place, MAKE, r#"{"install_made": "x"}"#, NOW, |_, _| {
            panic!("not handed")
        });
        assert!(told["refused"].as_str().unwrap().contains("one line"));
        assert_eq!(
            answer(&place, MAKE, r#"{"ready": {}}"#, NOW, |_, _| panic!(
                "not made"
            )),
            json!({"ready": true})
        );
    }

    /// Through the socket itself, with no tasks part anywhere: a make
    /// answered, made on its own thread (here with no model to ask, so not
    /// kept, and why), and the status read back on a second connection.
    #[test]
    fn through_the_socket_a_make_is_answered_and_its_outcome_kept() {
        let dir = folder("served");
        // Its own directory: the socket's is set to the voice's alone.
        let path = PathBuf::from(format!("/tmp/am-{}/make.sock", std::process::id()));
        let _ = fs::remove_dir_all(path.parent().unwrap());
        let keeping = made_skill::Keeping {
            directory: dir.join("made"),
            program: dir.join("no-pack"),
            manage: dir.join("manage.sock"),
        };
        fn no_model() -> Result<Chat, String> {
            Err("no model here".into())
        }
        serve(
            &path,
            None,
            MAKE,
            keeping,
            None,
            no_model,
            dir.join("skills"),
            |_| true,
        )
        .unwrap();
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
        assert_eq!(told["why"], "the lamp's model didn't answer");
        let _ = fs::remove_dir_all(path.parent().unwrap());
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
            handed(made, "r1", "s1:1");
            let late = made.to_path_buf();
            let refusal = std::thread::spawn(move || {
                not_kept(&late, "s1:1:1", "the lamp's model didn't answer");
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
                handed(&newer, "r2", "s1:2");
            });
            refusal.join().unwrap();
            admission.join().unwrap();
            let entries = asks(made);
            assert_eq!(entries.len(), 2, "round {round}: {entries:?}");
            assert_eq!(
                entries[0]["not_kept"], "the lamp's model didn't answer",
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

    /// Made by the lamp's own model (#4914), on a lamp with no tasks part
    /// at all: `ready` at once; a typed ask made, kept and put on at once
    /// through the manage helper, with no ask said and no yes waited for
    /// (the owner, 10 Nov): the one line said is that it's on, and the app
    /// is told `on`.
    #[test]
    fn made_here_goes_on_at_once_with_no_ask_said() {
        use crate::make_skill_local::tests::{chat_at, good, keeping, manage_helper, service};
        assert_eq!(
            answer(
                Path::new("/nonexistent"),
                MAKE,
                r#"{"ready": {}}"#,
                NOW,
                |_, _| panic!("not handed")
            ),
            json!({"ready": true})
        );
        let (root, keeping) = keeping("app-on");
        let at = now_unix_ms();
        let words = "make me a skill that dims the light at 10 pm";
        let brief = begin(&keeping.directory, words, "r1", at).unwrap();
        assert_eq!(
            status(&keeping.directory, Some("r1"), at)["state"],
            "making"
        );
        let (url, served) = service(vec![(200, good())]);
        let asked = manage_helper(&keeping.manage);
        let mut said = Vec::new();
        make_here(&keeping, &brief, "r1", at, chat_at(url), |line| {
            said.push(line.to_owned());
        });
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(
            said,
            [
                "I made evening dim, and it's on: when the lamp goes quiet, between 22:00 and 22:30, it dims the light a step, at most once every 12 hours, using its light and whether it is talking. Say \"take it off\" to take it off."
            ]
        );
        assert!(!said[0].contains("Shall I"), "{said:?}");
        let told = status(&keeping.directory, Some("r1"), at);
        assert_eq!(told["state"], "on", "{told}");
        assert_eq!(told["name"], "evening-dim");
        assert_eq!(asked.join().unwrap()["install_made"], told["sha256"]);
        let _ = fs::remove_dir_all(&root);
    }

    /// Made and kept, and the manage helper not there: it stays off, the
    /// lamp says why in one line, and the app is told `not_kept` and why;
    /// nothing waits for a yes.
    #[test]
    fn made_here_and_not_put_on_says_why_and_tells_the_app() {
        use crate::make_skill_local::tests::{chat_at, good, keeping, service};
        let (root, keeping) = keeping("app-not-on");
        let at = now_unix_ms();
        let brief = begin(&keeping.directory, "make me a skill", "r1", at).unwrap();
        let (url, served) = service(vec![(200, good())]);
        let mut said = Vec::new();
        make_here(&keeping, &brief, "r1", at, chat_at(url), |line| {
            said.push(line.to_owned());
        });
        assert_eq!(served.join().unwrap(), 1);
        assert_eq!(
            said,
            ["I made it but couldn't put it on: the lamp's skills couldn't be reached."]
        );
        let told = status(&keeping.directory, Some("r1"), at);
        assert_eq!(
            (told["state"].as_str(), told["why"].as_str()),
            (
                Some("not_kept"),
                Some("it couldn't be put on: the lamp's skills couldn't be reached")
            ),
            "{told}"
        );
        assert_ne!(made_skill::entries(&keeping.directory)[0]["on"], true);
        let _ = fs::remove_dir_all(&root);
    }

    /// Made here and not kept: the app is told why in plain words, and
    /// nothing is said aloud.
    #[test]
    fn made_here_not_kept_tells_the_app_why_and_says_nothing() {
        use crate::make_skill_local::tests::{chat_at, keeping, service};
        let (root, keeping) = keeping("app-refused");
        let at = now_unix_ms();
        let brief = begin(&keeping.directory, "make me a skill that glows", "r1", at).unwrap();
        let (url, served) = service(vec![(200, "Sure! Here is a skill.".into())]);
        make_here(&keeping, &brief, "r1", at, chat_at(url), |line| {
            panic!("nothing is said: {line}")
        });
        assert_eq!(served.join().unwrap(), 1);
        let told = status(&keeping.directory, Some("r1"), at);
        assert_eq!(told["state"], "not_kept", "{told}");
        assert!(told["why"].is_string(), "{told}");
        let _ = fs::remove_dir_all(&root);
    }

    /// The model's `cannot:` reason is told to the app as it is, whole,
    /// with what to ask instead, and nothing is said aloud (#5977: on
    /// lamp-74ff the app was shown a reason cut at "beyond s…").
    #[test]
    fn a_cannot_reason_is_told_to_the_app_as_it_is() {
        use crate::make_skill_local::tests::{chat_at, keeping, service};
        let (root, keeping) = keeping("app-cannot");
        let at = now_unix_ms();
        let brief = begin(
            &keeping.directory,
            "remind me every hour to stand",
            "r1",
            at,
        )
        .unwrap();
        let reason = "A skill can't keep time by itself. Ask me for a reminder instead, like \
                      'remind me every hour to stand up'.";
        let (url, served) = service(vec![(200, format!("cannot: {reason}"))]);
        make_here(&keeping, &brief, "r1", at, chat_at(url), |line| {
            panic!("nothing is said: {line}")
        });
        assert_eq!(served.join().unwrap(), 1);
        let told = status(&keeping.directory, Some("r1"), at);
        assert_eq!(
            (told["state"].as_str(), told["why"].as_str()),
            (Some("not_kept"), Some(reason)),
            "{told}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// A made skill's `needs_link` for the app: a skill that reads no
    /// service never asks what is linked and is told none missing; one that
    /// reads Gmail, not linked, is told Gmail; linked, none; and with
    /// `lamp-services` not telling, no `needs_link` at all, the skill kept
    /// all the same.
    #[test]
    fn the_app_is_told_what_a_made_skill_needs_linked() {
        use crate::make_skill_local::tests::{chat_at, good, keeping, manage_helper, service};
        let (root, keeping) = keeping("app-needs-link");
        let at = now_unix_ms();
        let brief = begin(
            &keeping.directory,
            "make me a skill that dims at 10",
            "r1",
            at,
        )
        .unwrap();
        let (url, served) = service(vec![(200, good())]);
        let asked = manage_helper(&keeping.manage);
        make_here_with(
            &keeping,
            &brief,
            "r1",
            at,
            chat_at(url),
            |_| {},
            || panic!("a skill that reads nothing never asks what is linked"),
        );
        assert_eq!(served.join().unwrap(), 1);
        let told = status(&keeping.directory, Some("r1"), at);
        assert_eq!(told["state"], "on", "{told}");
        asked.join().unwrap();
        assert_eq!(told["needs_link"], json!([]), "{told}");
        // The same ask, as a skill that reads Gmail would be noted.
        let reads = made_skill::Uses {
            speak: true,
            reads: Some("gmail".into()),
            ..Default::default()
        };
        let gmail = json!([{"code": "gmail", "name": "Gmail"}]);
        for (linked, needs) in [
            (Some(vec![]), Some(gmail.clone())),
            (Some(vec!["notion".to_owned()]), Some(gmail.clone())),
            (Some(vec!["gmail".to_owned()]), Some(json!([]))),
            (None, None),
        ] {
            let mut entries = asks(&keeping.directory);
            entries[0].as_object_mut().unwrap().remove("needs_link");
            write_asks(&keeping.directory, &entries).unwrap();
            if let Some(noted) = made_skill::needs_link(&reads, || linked.clone()) {
                link_noted(&keeping.directory, "r1", &noted);
            }
            let told = status(&keeping.directory, Some("r1"), at);
            assert_eq!(told["state"], "on", "{told}");
            assert_eq!(told.get("needs_link").cloned(), needs, "{linked:?}: {told}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    fn written_line(name: &str, when: &str, instructions: &str, id: &str) -> String {
        json!({"written": {"name": name, "when": when, "instructions": instructions, "request_id": id}})
            .to_string()
    }

    /// `LAMP_SKILL_WRITTEN` off: a written skill is refused and nothing is
    /// asked; on, it is answered at once and handed on with the text as
    /// written, and its fields are held to their bounds before anything
    /// is recorded. Making a skill from words stays its own switch's.
    #[test]
    fn a_written_skill_is_answered_only_with_its_switch() {
        let dir = folder("written-switch");
        let made = dir.join("made");
        let mut on = On {
            make: false,
            written: false,
        };
        let line = written_line(
            "study-mode",
            "when I say study mode",
            "Dim the light.\nStay quiet.",
            "w1",
        );
        let told = answer(&made, on, &line, NOW, |_, _| panic!("not handed"));
        assert!(
            told["refused"].as_str().unwrap().contains("isn't on"),
            "{told}"
        );
        assert!(!made.join(APP_INDEX).exists(), "nothing recorded");
        on.written = true;
        let told = answer(
            &made,
            on,
            r#"{"make": {"words": "make me a skill", "request_id": "m1"}}"#,
            NOW,
            |_, _| panic!("not handed"),
        );
        assert!(
            told["refused"].as_str().unwrap().contains("isn't on"),
            "{told}"
        );
        let told = answer(&made, on, r#"{"ready": {}}"#, NOW, |_, _| panic!());
        assert_eq!(told["ready"], false, "{told}");
        for bad in [
            written_line("", "x", "y", "w2"),
            written_line("study", "", "y", "w2"),
            written_line("study", "x", "", "w2"),
            written_line("study", "x", &"a".repeat(2_001), "w2"),
            written_line("study", &"a".repeat(201), "y", "w2"),
            written_line("Study Mode", "x", "y", "w2"),
            written_line("study", "x", "y", ""),
        ] {
            let told = answer(&made, on, &bad, NOW, |_, _| panic!("not handed"));
            assert!(told["refused"].is_string(), "{bad}: {told}");
        }
        assert!(!made.join(APP_INDEX).exists(), "nothing recorded");
        let (sent, got) = std::sync::mpsc::channel();
        let told = answer(&made, on, &line, NOW, move |asked, id| {
            sent.send((asked, id)).unwrap()
        });
        assert_eq!(told, json!({"making": true}));
        let (asked, id) = got.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            asked,
            Asked::Written(
                made_skill::Written::new(
                    "study-mode",
                    "when I say study mode",
                    "Dim the light.\nStay quiet."
                )
                .unwrap()
            )
        );
        assert_eq!(id, "w1");
        let told = status(&made, Some("w1"), NOW);
        assert_eq!(
            (told["state"].as_str(), told["written"].as_bool()),
            (Some("making"), Some(true))
        );
        assert!(
            !told.to_string().contains("Stay quiet"),
            "never the words: {told}"
        );
        // The same request id again: refused.
        let told = answer(&made, on, &line, NOW + MAKING_LONGEST_MS, |_, _| {
            panic!("not handed")
        });
        assert_eq!(told["refused"], "that ask was made already");
    }

    /// Kept from the app's ask with no model and put on at once, as a made
    /// one is (the owner, 10 Nov): the app told it is on and written, and
    /// the one line said is that it's on. A name taken is told to the app,
    /// and nothing is said.
    #[test]
    fn a_written_skill_is_kept_put_on_and_told() {
        let (root, keeping) = crate::make_skill_local::tests::keeping("app-written");
        let installed = root.join("skills");
        fs::create_dir_all(installed.join("taken")).unwrap();
        let at = now_unix_ms();
        let skill = |name: &str| {
            made_skill::Written::new(name, "when I say study mode", "Dim the light.").unwrap()
        };
        let (written, id) = begin_written(
            &keeping.directory,
            &json!({"name": "study-mode", "when": "when I say study mode",
                "instructions": "Dim the light.", "request_id": "w1"}),
            at,
        )
        .unwrap();
        assert_eq!((written.clone(), id.as_str()), (skill("study-mode"), "w1"));
        let asked = crate::make_skill_local::tests::manage_helper(&keeping.manage);
        let mut said = None;
        write_here(&keeping, &written, "w1", at, &installed, |line| {
            said = Some(line.to_owned());
        });
        assert_eq!(
            said.as_deref(),
            Some(
                "I made study mode, and it's on: when you ask for it (when I say study mode), the lamp follows the instructions you wrote: Dim the light, using its voice. Say \"take it off\" to take it off."
            )
        );
        let told = status(&keeping.directory, Some("w1"), at);
        assert_eq!(told["state"], "on", "{told}");
        assert_eq!(
            (told["name"].as_str(), told["written"].as_bool()),
            (Some("study-mode"), Some(true))
        );
        assert_eq!(told["uses"]["speak"], true);
        assert_eq!(asked.join().unwrap()["install_made"], told["sha256"]);
        // A name a skill on the lamp has: not kept, the app told why, and
        // nothing said.
        let (taken, _) = begin_written(
            &keeping.directory,
            &json!({"name": "taken", "when": "x", "instructions": "y", "request_id": "w2"}),
            at + 3_000,
        )
        .unwrap();
        write_here(&keeping, &taken, "w2", at + 3_000, &installed, |line| {
            panic!("nothing is said: {line}")
        });
        let told = status(&keeping.directory, Some("w2"), at + 3_000);
        assert_eq!(told["state"], "not_kept", "{told}");
        assert!(
            told["why"]
                .as_str()
                .unwrap()
                .contains("already on the lamp"),
            "{told}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
