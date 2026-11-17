//! An OpenClaw task's own steps on the bus (`tasks/step`, #7267,
//! `docs/tasks.md`, "An agent's steps"), behind [`SWITCH`].
//!
//! While a task lamp-tasks started on the lamp's own OpenClaw is watched,
//! its session's transcript is read every [`EVERY_MS`], and each tool call,
//! its result, the agent's reply text and an error become one [`TaskStep`].
//! A call is published as soon as it is read (`ok` none: still running),
//! and the same step number again once its result is read (`result`,
//! `ok`): the latest a `(task, n)` wins.
//!
//! OpenClaw (2026.11, read on lamp-52e6) keeps its transcripts in SQLite,
//! `<state>/agents/<agent>/agent/openclaw-agent.sqlite`, not in files:
//! `session_nodes` maps a session's key to its current session id, and
//! `transcript_events` holds each event by `(session_id, seq)`. Harness
//! runs OpenClaw as `openclaw tui --session harness-<agentId>`
//! (`harness::EngineKind::Openclaw`), so the task's session is the node
//! whose key ends `:harness-<agentId>`. The database is opened read-only
//! (`mode=ro`, never through a link), once a read.
//!
//! A task's steps begin at its own event: the session's highest `seq` when
//! the task is first watched (a session that is not there yet is the
//! task's own, from its first event). One session may hold many asks
//! (#7258's one pane), so nothing at or before that `seq` is ever read for
//! it, a task before it on the same session reads no further than it, and
//! an event with no time of its own, or from before the hand-over, is
//! skipped. An event kept compressed (`event_zstd`, no `event_json`) is
//! decoded with `ruzstd` as it streams, at most 64 KiB; past that, or
//! corrupt, it is skipped and counted.
//!
//! Nothing is written: the positions and the calls still running live in
//! memory, and the bus event is the only copy. A read that fails publishes
//! nothing and is tried again at the next.
use crate::engine::Watched;
use lamp_msgs::{
    StepKind, TASK_STEP_MOST_RESULT_BYTES, TASK_STEP_MOST_TOOL_BYTES, TaskStep, cut_whole_chars,
};
use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// `LAMP_TASKS_STEPS`: `on` exactly; off unless set, and off in the
/// product unit. The steps carry the agent's words (a tool's arguments and
/// the start of its result), so they are on the bus only when asked.
pub const SWITCH: &str = "LAMP_TASKS_STEPS";
/// `LAMP_TASKS_STEPS_STATE`: an absolute path, OpenClaw's state folder to
/// read transcripts under in place of [`roots`]' defaults. A setting.
pub const STATE_SETTING: &str = "LAMP_TASKS_STEPS_STATE";
/// How often each watched task's transcript is read, by the boot clock: a
/// step is on the bus within this of its event, ~2 s on the page with the
/// bus and peek's push.
pub const EVERY_MS: u64 = 1_000;
/// The most steps a task puts on the bus; later ones are counted and
/// dropped, the count said in the journal when the task leaves the watch.
pub const MOST_STEPS: u32 = 50;
/// OpenClaw's agent database, under `<state>/agents/<agent>/`.
pub const DATABASE: &str = "agent/openclaw-agent.sqlite";
/// The most events one read takes, every task together; the rest wait.
const MOST_EVENTS: i64 = 200;
/// The most event text, decoded or not, one read takes, every task
/// together; the rest wait.
const MOST_READ_BYTES: usize = 256 * 1024;
/// An event (or a compressed one) longer than this is skipped unparsed (a
/// whole file read back).
const MOST_EVENT_BYTES: usize = 256 * 1024;
/// The most calls kept waiting for their result, per task.
const MOST_RUNNING: usize = 64;

/// [`SWITCH`]'s value: `on` exactly. Off, there is no reader and no
/// publisher at all.
pub fn switched(value: Option<&str>, roots: Vec<PathBuf>) -> Option<Steps> {
    (value == Some("on")).then(|| Steps::new(roots))
}

/// How often the steps' own thread looks at the watch: a task new to it is
/// marked at most this long after the store's owner lists it.
const LOOK_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// `steps` run on a thread of their own (`tasks-steps`), never on the
/// accept loop that serves the voice's socket: every [`LOOK_EVERY`] it takes
/// the tasks watched now from `watched` and hands each step read, checked
/// against its type's caps, to `publish`. Ends once `stop` is set.
pub fn spawn(
    mut steps: Steps,
    watched: impl Fn() -> Vec<Watched> + Send + 'static,
    mut publish: impl FnMut(&TaskStep) + Send + 'static,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("tasks-steps".into())
        .spawn(move || {
            let began = std::time::Instant::now();
            while !stop.load(std::sync::atomic::Ordering::Acquire) {
                let now_ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
                let wall_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| {
                        u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
                    });
                for step in steps.tick(&watched(), now_ms, wall_ms) {
                    if step.check().is_ok() {
                        publish(&step);
                    }
                }
                std::thread::sleep(LOOK_EVERY);
            }
        })
}

/// Whether [`SWITCH`] is on in this process's environment, read once: the
/// store's owner hands the watched tasks over only then.
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var(SWITCH).as_deref() == Ok("on"))
}

/// Where OpenClaw's state is looked for: `override_dir`
/// ([`STATE_SETTING`]) when it is an absolute path; else the state folder
/// lamp-update's wrapper and the gateway's unit give OpenClaw
/// (`runtime::OPENCLAW_STATE_DIR`), where lamp-52e6 keeps it.
pub fn roots(override_dir: Option<&str>) -> Vec<PathBuf> {
    match override_dir.filter(|dir| dir.starts_with('/')) {
        Some(dir) => vec![PathBuf::from(dir)],
        None => vec![PathBuf::from(crate::runtime::OPENCLAW_STATE_DIR)],
    }
}

/// One watched task's session, as far as it was read.
#[derive(Debug, Default)]
struct Tail {
    /// Harness's agent id: every task on one agent shares its session.
    agent: String,
    /// The database and session id, once found.
    session: Option<(PathBuf, String)>,
    /// The highest `seq` read or passed: the task's own start at first.
    after: i64,
    /// Where a newer task on the same session began: this task reads no
    /// further (#7258's one pane holds every ask in one session).
    until: Option<i64>,
    /// Steps put on the bus, those dropped past [`MOST_STEPS`], and events
    /// kept compressed that were skipped: corrupt, or past 64 KiB decoded.
    n: u32,
    dropped: u64,
    undecoded: u64,
    /// Calls on the bus whose result has not been read: id → (n, tool, args).
    running: BTreeMap<String, (u32, String, String)>,
    /// When the task was handed over, ms since 1970, if in this boot.
    since_unix_ms: Option<u64>,
}

/// Every watched OpenClaw task's session, read on a 1 s schedule.
#[derive(Debug)]
pub struct Steps {
    roots: Vec<PathBuf>,
    tails: BTreeMap<u64, Tail>,
    /// Watched tasks whose bound could not be read yet, with when it was
    /// last tried (boot clock): not read at all until it is.
    unbound: BTreeMap<u64, u64>,
    next_ms: u64,
}

impl Steps {
    /// Over OpenClaw's state folders, looked in in order ([`roots`]).
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            tails: BTreeMap::new(),
            unbound: BTreeMap::new(),
            next_ms: 0,
        }
    }

    /// The steps read since the last read, if one is due at `now_ms` (boot
    /// clock); `wall_ms` is the wall clock. A task new to the watch is
    /// marked at its session's last event at once, at every call, so its
    /// own steps begin as near its hand-over as the watch sees it. A task
    /// that left the watch is read once more, then let go.
    pub fn tick(&mut self, watched: &[Watched], now_ms: u64, wall_ms: u64) -> Vec<TaskStep> {
        let ours: Vec<&Watched> = watched.iter().filter(|w| w.engine == "openclaw").collect();
        self.unbound
            .retain(|id, _| ours.iter().any(|w| w.task_id == *id));
        for task in &ours {
            let due = self
                .unbound
                .get(&task.task_id)
                .is_none_or(|tried| now_ms.saturating_sub(*tried) >= EVERY_MS);
            if !self.tails.contains_key(&task.task_id) && due {
                if self.begin(task, wall_ms) {
                    self.unbound.remove(&task.task_id);
                } else {
                    // No bound yet: nothing is read for it, and the bound
                    // is read again a second later, never taken as 0.
                    self.unbound.insert(task.task_id, now_ms);
                }
            }
        }
        if now_ms < self.next_ms {
            return Vec::new();
        }
        self.next_ms = now_ms.saturating_add(EVERY_MS);
        let mut out = Vec::new();
        let mut budget = Budget::default();
        for task in &ours {
            if !self.tails.contains_key(&task.task_id) {
                continue;
            }
            let found = session(&self.roots, &task.agent_id);
            if let Some(tail) = self.tails.get_mut(&task.task_id) {
                read(found, task.task_id, tail, &mut budget, &mut out);
            }
        }
        let gone: Vec<u64> = self
            .tails
            .keys()
            .filter(|id| !ours.iter().any(|w| w.task_id == **id))
            .copied()
            .collect();
        for id in gone {
            if let Some(mut tail) = self.tails.remove(&id) {
                let last = Ok(tail.session.clone());
                read(last, id, &mut tail, &mut budget, &mut out);
                if tail.dropped > 0 || tail.undecoded > 0 {
                    eprintln!(
                        "tasks: task {id}: {} steps put on tasks/step, {} more dropped past {MOST_STEPS}, {} compressed events skipped (corrupt or past 64 KiB)",
                        tail.n, tail.dropped, tail.undecoded
                    );
                }
            }
        }
        out
    }

    /// A task new to the watch: it begins after its session's last event
    /// now, and a task before it on the same session ends there. With no
    /// session in a database that was read, the session is the task's own,
    /// from its first event. A read that fails gives no bound: false, and
    /// the task is not read until a later one gives it.
    fn begin(&mut self, task: &Watched, wall_ms: u64) -> bool {
        let mut tail = Tail {
            agent: task.agent_id.clone(),
            since_unix_ms: task.accepted_ago_ms.map(|ago| wall_ms.saturating_sub(ago)),
            ..Tail::default()
        };
        let Ok(found) = session(&self.roots, &task.agent_id) else {
            return false;
        };
        // The newest task on an agent has what comes after its start. An
        // earlier one reads no further: up to this start on the session it
        // shares (or will, once OpenClaw makes it: -1, nothing, when there
        // is none yet), and never into another session it moves to.
        let last = match &found {
            Some(found) => match last_seq(found) {
                Some(last) => last,
                None => return false,
            },
            None => -1,
        };
        for earlier in self.tails.values_mut() {
            if earlier.agent == task.agent_id && earlier.until.is_none() {
                earlier.until = Some(if earlier.session.is_none() || earlier.session == found {
                    last
                } else {
                    i64::MAX
                });
            }
        }
        if let Some(found) = found {
            tail.session = Some(found);
            tail.after = last;
        }
        eprintln!(
            "tasks: task {}: its steps read from its OpenClaw session, {}",
            task.task_id,
            if tail.session.is_some() {
                "after its last event now"
            } else {
                "from its first event, once it is there"
            }
        );
        self.tails.insert(task.task_id, tail);
        true
    }
}

/// What one read (one tick, every task together) may take: at most
/// [`MOST_EVENTS`] events and [`MOST_READ_BYTES`] of event text, decoded or
/// not; the rest waits for the next read.
#[derive(Debug)]
struct Budget {
    events: i64,
    bytes: usize,
    /// Nothing taken yet: the first event is taken whatever its size, so a
    /// large one is never left waiting for ever.
    fresh: bool,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            events: MOST_EVENTS,
            bytes: MOST_READ_BYTES,
            fresh: true,
        }
    }
}

/// How long a read waits for OpenClaw's writer before it gives up for
/// this second.
const BUSY: std::time::Duration = std::time::Duration::from_millis(100);

/// What a read of OpenClaw's database needs: each query's columns, so an
/// OpenClaw that changed its schema is said once and read as nothing.
const SCHEMA: [&str; 2] = [
    "SELECT session_key, current_session_id FROM session_nodes LIMIT 0",
    "SELECT session_id, seq, event_json, event_zstd, created_at FROM transcript_events LIMIT 0",
];

/// One journal line a process for each kind of trouble, never with words.
fn say_once(what: &'static str) {
    static SAID: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    if let Ok(mut said) = SAID.lock()
        && !said.contains(&what)
    {
        said.push(what);
        eprintln!("tasks: steps: {what}; no steps from it are published");
    }
}

/// `path` opened read-only (`mode=ro`, never `immutable`: OpenClaw writes
/// it in WAL), never through a link and never created, with a short wait
/// for the writer, its schema checked. Any trouble is `None`, said once.
fn open(path: &Path) -> Option<Connection> {
    let uri = format!("file:{}?mode=ro", path.to_str()?);
    let Ok(db) = Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    ) else {
        say_once("OpenClaw's agent database could not be opened read-only");
        return None;
    };
    if db.busy_timeout(BUSY).is_err() {
        return None;
    }
    if SCHEMA.iter().any(|query| db.prepare(query).is_err()) {
        say_once("OpenClaw's agent database is locked or not the schema lampOS reads");
        return None;
    }
    Some(db)
}

/// A read of OpenClaw's database that failed: locked, busy, not readable or
/// not the schema read. Never taken for "no session".
#[derive(Debug, PartialEq, Eq)]
pub struct Unread;

/// The session Harness opened for `agent`: in the first state folder whose
/// agents' databases have a node whose key ends `:harness-<agent>` (case
/// ignored), the database and that node's current session id. `Ok(None)`
/// only when every database there was read and none has it; a database
/// that is there but could not be read is [`Unread`].
pub fn session(roots: &[PathBuf], agent: &str) -> Result<Option<(PathBuf, String)>, Unread> {
    if agent.is_empty()
        || !agent
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Ok(None);
    }
    let wanted = format!(":harness-{}", agent.to_ascii_lowercase());
    let mut unread = false;
    for state in roots {
        // SQLite's NOFOLLOW refuses a link anywhere in the path: the state
        // folder is taken as it really is (a data partition, a moved
        // folder), and only what lies under it must not be a link.
        let Ok(state) = fs::canonicalize(state) else {
            continue;
        };
        let folder = state.join("agents");
        if fs::symlink_metadata(&folder).is_err() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&folder) else {
            unread = true;
            continue;
        };
        let mut agents: Vec<PathBuf> = entries
            .filter_map(|entry| Some(entry.ok()?.path().join(DATABASE)))
            .filter(|path| fs::symlink_metadata(path).is_ok())
            .collect();
        agents.sort();
        for path in agents {
            match node(&path, &wanted) {
                Ok(Some(id)) => return Ok(Some((path, id))),
                Ok(None) => {}
                Err(Unread) => unread = true,
            }
        }
    }
    if unread { Err(Unread) } else { Ok(None) }
}

/// The current session id of the node in `path` whose key ends `wanted`.
fn node(path: &Path, wanted: &str) -> Result<Option<String>, Unread> {
    let db = open(path).ok_or(Unread)?;
    let mut nodes = db
        .prepare("SELECT session_key, current_session_id FROM session_nodes")
        .map_err(|_| Unread)?;
    let rows = nodes
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        .map_err(|_| Unread)?;
    Ok(rows
        .into_iter()
        .find(|(key, _)| key.starts_with("agent:") && key.to_ascii_lowercase().ends_with(wanted))
        .and_then(|(_, id)| id))
}

/// The session's highest `seq` now; -1 for a session with no event yet.
fn last_seq((path, id): &(PathBuf, String)) -> Option<i64> {
    open(path)?
        .query_row(
            "SELECT COALESCE(MAX(seq), -1) FROM transcript_events WHERE session_id = ?1",
            params![id],
            |row| row.get(0),
        )
        .ok()
}

/// The task's session, as looked up again now (`found`), read on from its
/// position, within `budget`. A session that first appears is the task's
/// own and is read from its first event; one that moves to another id while
/// the task is watched is read after that id's last event, as its earlier
/// events may be another ask's. A lookup or read that fails reads nothing
/// and changes nothing: the position is never reset or moved by it.
fn read(
    found: Result<Option<(PathBuf, String)>, Unread>,
    task: u64,
    tail: &mut Tail,
    budget: &mut Budget,
    out: &mut Vec<TaskStep>,
) {
    let Ok(found) = found else {
        return;
    };
    if budget.events <= 0 {
        return;
    }
    if let Some(found) = found
        && tail.session.as_ref() != Some(&found)
    {
        if tail.session.is_none() {
            // The task's own session, made after it began: from its first
            // event, up to a newer task's start if one came meanwhile.
            tail.after = -1;
        } else if tail.until.is_some() {
            // A newer task has this agent: this one never follows it.
        } else {
            // The session moved: only what comes after its last event now.
            let Some(after) = last_seq(&found) else {
                return;
            };
            tail.after = after;
        }
        if tail.session.is_none() || tail.until.is_none() {
            tail.session = Some(found);
        }
    }
    let Some((path, id)) = tail.session.clone() else {
        return;
    };
    let Some(db) = open(&path) else {
        return;
    };
    // Sizes first; an event's text is fetched only once it fits this read.
    let Ok(mut events) = db.prepare(
        "SELECT seq, event_json IS NULL AND event_zstd IS NOT NULL, \
         COALESCE(length(event_json), length(event_zstd), 0), created_at \
         FROM transcript_events WHERE session_id = ?1 AND seq > ?2 AND seq <= ?3 \
         ORDER BY seq LIMIT ?4",
    ) else {
        return;
    };
    let Ok(mut rows) = events.query(params![
        id,
        tail.after,
        tail.until.unwrap_or(i64::MAX),
        budget.events
    ]) else {
        return;
    };
    let text = |seq: i64, compressed: bool| -> Option<(Option<String>, Option<Vec<u8>>)> {
        let column = if compressed {
            "event_zstd"
        } else {
            "event_json"
        };
        db.query_row(
            &format!("SELECT {column} FROM transcript_events WHERE session_id = ?1 AND seq = ?2"),
            params![id, seq],
            |row| {
                Ok(if compressed {
                    (None, row.get::<_, Option<Vec<u8>>>(0)?)
                } else {
                    (row.get::<_, Option<String>>(0)?, None)
                })
            },
        )
        .ok()
    };
    loop {
        let row = match rows.next() {
            Ok(Some(row)) => row,
            Ok(None) | Err(_) => return,
        };
        let (Ok(seq), Ok(compressed), Ok(stored), Ok(created_at)) = (
            row.get::<_, i64>(0),
            row.get::<_, bool>(1),
            row.get::<_, i64>(2),
            row.get::<_, Option<i64>>(3),
        ) else {
            return;
        };
        let stored = usize::try_from(stored).unwrap_or(usize::MAX);
        if stored > budget.bytes && !budget.fresh {
            // Past this read's bytes: this event waits for the next read.
            return;
        }
        let (json, blob) = if stored > MOST_EVENT_BYTES {
            (None, None)
        } else {
            match text(seq, compressed) {
                Some(found) => found,
                None => return,
            }
        };
        let (json, skipped) = if compressed {
            match blob.as_deref().map(decoded) {
                Some(Ok(json)) => (Some(json), None),
                Some(Err(why)) => (None, Some(why)),
                None => (None, Some("a compressed event is over 256 KiB")),
            }
        } else {
            (json, None)
        };
        let size = json.as_ref().map_or(0, String::len);
        if size > budget.bytes && !budget.fresh {
            // Past this read's bytes: this event waits for the next read.
            return;
        }
        budget.fresh = false;
        budget.bytes = budget.bytes.saturating_sub(size);
        budget.events -= 1;
        tail.after = seq;
        if let Some(why) = skipped {
            say_once(why);
            tail.undecoded += 1;
        }
        if let Some(event) = json.and_then(|json| serde_json::from_str::<Value>(&json).ok()) {
            steps_of(&event, created_at, task, tail, out);
        }
        if budget.events <= 0 {
            return;
        }
    }
}

/// The most bytes an event kept compressed is decoded to: past it, it is
/// skipped and counted, never decoded further.
const MOST_DECODED: u64 = 64 * 1024;

/// The largest window a compressed event's frame may ask for. A frame made
/// for at most 64 KiB needs far less; OpenClaw's own small rows ask for
/// their size.
const MOST_WINDOW: u64 = 1024 * 1024;

/// Why a compressed event whose frame asks for a window past
/// [`MOST_WINDOW`] is skipped.
const WINDOW_TOO_LARGE: &str = "a compressed event asks for a window past 1 MiB";

/// A compressed event (`event_zstd`, one zstd frame) decoded as it streams,
/// stopping at [`MOST_DECODED`] + 1 bytes: never more is decoded.
fn decoded(blob: &[u8]) -> Result<String, &'static str> {
    use std::io::Read;
    const CORRUPT: &str = "a compressed event could not be decoded";
    // The header first: the decoder reserves the frame's whole window (up to
    // 100 MiB in ruzstd) before it decodes a byte.
    let (frame, _) = ruzstd::frame::read_frame_header(blob).map_err(|_| CORRUPT)?;
    if frame.header.window_size().map_err(|_| CORRUPT)? > MOST_WINDOW {
        return Err(WINDOW_TOO_LARGE);
    }
    let mut source = blob;
    let decoder = ruzstd::StreamingDecoder::new(&mut source).map_err(|_| CORRUPT)?;
    let mut bytes = Vec::new();
    decoder
        .take(MOST_DECODED + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CORRUPT)?;
    if bytes.len() as u64 > MOST_DECODED {
        return Err("a compressed event decodes past 64 KiB");
    }
    String::from_utf8(bytes).map_err(|_| CORRUPT)
}

/// Whitespace runs as one space, secrets redacted ([`redacted`]), then cut
/// on a whole character.
fn short(text: &str, most: usize) -> String {
    cut_whole_chars(&redacted(text), most).to_owned()
}

/// Whether a name (`API_KEY`, `"token"`) says its value is a secret.
fn secret_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    [
        "KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "AUTH", "COOKIE",
    ]
    .iter()
    .any(|part| name.contains(part))
}

/// A call's arguments with the value of every key named like a secret
/// ([`secret_name`]) replaced by `***`, at any depth.
fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(name, inner)| {
                    let inner = if secret_name(name) && !inner.is_object() && !inner.is_array() {
                        Value::String("***".into())
                    } else {
                        redact_json(inner)
                    };
                    (name.clone(), inner)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact_json).collect()),
        other => other.clone(),
    }
}

/// `text` with whitespace runs as one space and what looks like a secret
/// replaced by `***` (Heron's read on #7286): the word after `Bearer` or
/// `Basic`, the value of `NAME=value` or `NAME: value` whose name says
/// secret, and a token-shaped word (24 or more letters, digits, `-` or `_`
/// with both a letter and a digit, or a known key prefix). Words matched,
/// not understood: a long hash or id is hidden too.
fn redacted(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut hide_next = false;
    for word in text.split_whitespace() {
        let bare =
            word.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        let shown = if hide_next && !bare.is_empty() {
            "***".to_owned()
        } else if let Some((name, value)) = word.split_once('=')
            && secret_name(name)
            && !value.is_empty()
        {
            format!("{name}=***")
        } else if token_shaped(bare) {
            word.replacen(bare, "***", 1)
        } else {
            word.to_owned()
        };
        hide_next = bare.eq_ignore_ascii_case("bearer")
            || bare.eq_ignore_ascii_case("basic")
            || (word.ends_with(':') && secret_name(bare));
        out.push(shown);
    }
    out.join(" ")
}

/// A word that looks like a key or token.
fn token_shaped(word: &str) -> bool {
    let known = [
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
        "ghs_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "AKIA",
        "AIza",
    ]
    .iter()
    .any(|prefix| word.starts_with(prefix) && word.len() >= 16);
    let long = word.len() >= 24
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && word.chars().any(|c| c.is_ascii_digit())
        && word.chars().any(|c| c.is_ascii_alphabetic());
    known || long
}

/// The text parts of a message's `content`, joined.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part["type"] == "text")
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// A time in milliseconds since 1970, from milliseconds or seconds.
fn unix_ms(value: i64) -> Option<u64> {
    let value = u64::try_from(value).ok().filter(|value| *value > 0)?;
    Some(if value < 100_000_000_000 {
        value.saturating_mul(1_000)
    } else {
        value
    })
}

/// The steps in one event: a message (`{"type": "message", "message":
/// {...}}`, or the message itself), with its own time, or the row's
/// `created_at`. Kinds and roles not known are skipped, and so is an event
/// with no time at all.
fn steps_of(
    event: &Value,
    created_at: Option<i64>,
    task: u64,
    tail: &mut Tail,
    out: &mut Vec<TaskStep>,
) {
    let message = if event["type"] == "message" {
        &event["message"]
    } else {
        event
    };
    let Some(at) = message["timestamp"]
        .as_i64()
        .or(created_at)
        .and_then(unix_ms)
    else {
        return;
    };
    if tail.since_unix_ms.is_some_and(|since| at < since) {
        return;
    }
    let mut push =
        |tail: &mut Tail, n: Option<u32>, kind, tool: &str, args: &str, result: &str, ok| {
            let n = match n {
                Some(n) => n,
                None if tail.n < MOST_STEPS => {
                    tail.n += 1;
                    tail.n
                }
                None => {
                    tail.dropped += 1;
                    return None;
                }
            };
            out.push(TaskStep {
                task,
                engine: "openclaw".into(),
                n,
                at_unix_ms: at,
                kind,
                tool: short(tool, TASK_STEP_MOST_TOOL_BYTES),
                args: short(args, TASK_STEP_MOST_RESULT_BYTES),
                result: short(result, TASK_STEP_MOST_RESULT_BYTES),
                ok,
            });
            Some(n)
        };
    match message["role"].as_str() {
        Some("assistant") => {
            let reply = text_of(&message["content"]);
            if !reply.trim().is_empty() {
                push(tail, None, StepKind::Reply, "", "", &reply, None);
            }
            for call in message["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|part| part["type"] == "toolCall")
            {
                let Some(tool) = call["name"].as_str().filter(|name| !name.is_empty()) else {
                    continue;
                };
                let arguments = &call["arguments"];
                let args = match arguments["command"].as_str() {
                    Some(command) => command.to_owned(),
                    None if arguments.is_null() => String::new(),
                    None => redact_json(arguments).to_string(),
                };
                if let Some(n) = push(tail, None, StepKind::Call, tool, &args, "", None)
                    && let Some(id) = call["id"].as_str()
                {
                    if tail.running.len() >= MOST_RUNNING {
                        tail.running.pop_first();
                    }
                    tail.running
                        .insert(id.to_owned(), (n, tool.to_owned(), args));
                }
            }
            let error = message["errorMessage"].as_str();
            if error.is_some() || message["stopReason"] == "error" {
                push(
                    tail,
                    None,
                    StepKind::Error,
                    "",
                    "",
                    error.unwrap_or(""),
                    Some(false),
                );
            }
        }
        Some("toolResult") => {
            let Some((n, tool, args)) = message["toolCallId"]
                .as_str()
                .and_then(|id| tail.running.remove(id))
            else {
                return;
            };
            let ok = !message["isError"].as_bool().unwrap_or(false);
            let result = text_of(&message["content"]);
            push(
                tail,
                Some(n),
                StepKind::Call,
                &tool,
                &args,
                &result,
                Some(ok),
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
