//! Forget reaches the account's history (#7771): what a forget removes on
//! the lamp is deleted from the conversation history the account keeps
//! too, by the lamp's own cloud API (`POST <base>/history/turns/delete`,
//! with the lamp's key). Only with `LAMP_HISTORY_FORGET=on` (off: nothing
//! is queued or sent; after history was ever sent, a forget's ids are kept
//! by the memory until the switch is on again, [`asking`]).
//!
//! The lamp's own forget is the only one the person waits for: "forgotten"
//! follows the memory's answer, as before. Its answer is handed to this
//! module's worker thread ([`answered`], no disk and no page read on the
//! voice's way), which puts what it removed on a queue persisted in the
//! voice's state folder ([`FILE`]), read with the pages of its rest by
//! the token its answer gave; the queue is sent in the background by
//! [`crate::second_provider::net::history`]:
//!
//! - **forget everything** of a known person (`forget_all` of a `p-` id,
//!   answered `ok`): that person's whole history, `{"all":true,
//!   "person":<their id>}`. A delete of everything never goes without a
//!   person ([`Call::All`] always names one).
//! - **any other forget** (`forget`, `forget_last`, `exchanges_forget`,
//!   `forget_said`, and the room's lines a forget everything took): the
//!   exchanges the memory says it removed (its answer's `exchanges`, by
//!   scope and id, and every page `forgot_exchanges` gives for its
//!   `exchanges_token` when it says `exchanges_more`), `{"ids":[…]}` as the
//!   memory tells them. Every removed id is
//!   told: a forget never deletes more of the account's history than the
//!   lamp forgot. A whole-history delete is a known person's forget
//!   everything only.
//! - **the lamp's whole history** (every person's, the room's too): a
//!   forget of everything of every person, or a memory found wiped at a
//!   start (a reset: its epoch changed since the last start, [`epoch_seen`]),
//!   [`Call::Lamp`] ([`WHOLE_LAMP_BODY`]).
//!
//! A forget is never let go: one not queued (the worker behind or stopped,
//! a page not read) is read again from what the memory kept for it
//! ([`recover`]), and while any forget waits, the save sends no turn it
//! could reach ([`holds`]).
//!
//! The person key is lamp-memory's own scope id: a known person's `p-`
//! id (the eyes' person id), or `""` for the room's (`everyone`), as the
//! server's `person` is "the lamp's own person key, or empty". A saved
//! turn's id is the memory's own, `<epoch>.<scope>:<id>`
//! ([`lamp_msgs::history_turn_id`], the store's epoch changed by a wipe, so
//! an exchange id given again after one never names an older turn): the
//! memory makes it in its forget answers and this sends it as told; the
//! turn's save (#7771's other half) makes the same with the same function.
//!
//! Bounded: at most [`MOST_PENDING`] ids wait, far past anything real (an
//! id is about 30 bytes). Past it, the known person with the most waiting
//! is coalesced into a delete of their whole history, never the room's
//! (whose `""` would take every room line); the room's newest past it are
//! let go. Either is journalled as a count. Only counts are journalled,
//! never an id or a word.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// `on` and nothing else: forgets reach the account's history. On in the
/// product unit, always with `LAMP_HISTORY_SEND`.
pub const ENVIRONMENT: &str = "LAMP_HISTORY_FORGET";
/// The queue's file, in the voice's state folder.
pub const FILE: &str = "/var/lib/lampOS/voice/history-forget.json";
/// The most ids that wait (about 600 KB); past it, see [`Pending::add_ids`].
pub const MOST_PENDING: usize = 20_000;
/// The most `forgot_exchanges` pages read after one forget.
const MOST_PAGES: usize = 64;
/// The most ids one delete names (the server's cap).
pub const MOST_IDS_A_CALL: usize = 100;
/// The room's scope in lamp-memory; its person key on the server is `""`.
const ROOM: &str = "everyone";
/// The body of the delete of the lamp's whole history, every person's and
/// the room's, on the same delete call (the first builder's privacy read on
/// #8125, item 2; Backend3's server, 18:26 on #8125): it removes this
/// lamp's turns and actions on the account and nothing of the owner's other
/// lamps, and refuses a later save stamped at or before it. Until it is
/// done, the save sends nothing ([`holds`]).
pub const WHOLE_LAMP_BODY: &str = r#"{"all":true,"every_person":true}"#;
/// How long a worker owing a read of what the memory kept waits for a job
/// before it reads again, as the delete's first wait.
const RECOVER_AGAIN: std::time::Duration = std::time::Duration::from_secs(5);

/// Whether [`ENVIRONMENT`] is `on`, read once.
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on"))
}

/// The server's person key for a memory scope: a known person's `p-` id,
/// `""` for the room's; `None` for any other scope (a rules scope, a slug),
/// which no saved turn has.
pub fn person_key(scope: &str) -> Option<&str> {
    if scope == ROOM {
        Some("")
    } else if lamp_msgs::known::is_person_id(scope) {
        Some(scope)
    } else {
        None
    }
}

/// One delete call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// The lamp's whole history ([`WHOLE_LAMP_BODY`]).
    Lamp,
    /// Everything of one person (`""` the room's): never without one.
    All { person: String },
    /// These turns, at most [`MOST_IDS_A_CALL`], of one person.
    Ids { person: String, ids: Vec<String> },
}

impl Call {
    /// The request's body.
    pub fn body(&self) -> Value {
        match self {
            Self::Lamp => serde_json::from_str(WHOLE_LAMP_BODY).unwrap_or_default(),
            Self::All { person } => serde_json::json!({"all": true, "person": person}),
            Self::Ids { ids, .. } => serde_json::json!({"ids": ids}),
        }
    }

    /// How many ids it names (an `all` none), for the journal.
    pub fn ids(&self) -> usize {
        match self {
            Self::Lamp | Self::All { .. } => 0,
            Self::Ids { ids, .. } => ids.len(),
        }
    }
}

/// What waits, as kept on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// The lamp's whole history is to go, before anything else.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lamp: bool,
    /// The memory store's epoch at the last start, to see a wipe (a reset)
    /// at the next ([`Self::saw_epoch`]); `None`: not known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<String>,
    /// Person keys whose whole history is to go.
    #[serde(default)]
    pub all: BTreeSet<String>,
    /// Each person key's turn ids to go.
    #[serde(default)]
    pub ids: BTreeMap<String, BTreeSet<String>>,
}

impl Pending {
    /// Nothing waits to be sent.
    pub fn is_empty(&self) -> bool {
        !self.lamp && self.all.is_empty() && self.ids.is_empty()
    }

    /// Whether a delete waits that a turn of memory `scope` could be in:
    /// the lamp's, or that person's (`""` the room's).
    pub fn reaches(&self, scope: &str) -> bool {
        self.lamp
            || person_key(scope).is_some_and(|person| {
                self.all.contains(person) || self.ids.get(person).is_some_and(|ids| !ids.is_empty())
            })
    }

    /// The lamp's whole history: everything else waiting goes with it.
    pub fn add_lamp(&mut self) {
        self.all.clear();
        self.ids.clear();
        self.lamp = true;
    }

    /// The memory's epoch at a start: a different one from the last start's
    /// is a wiped memory (a reset, or a forget of everyone), so the lamp's
    /// whole history goes. Kept for the next.
    pub fn saw_epoch(&mut self, epoch: &str) {
        if self.epoch.as_deref().is_some_and(|last| last != epoch) {
            eprintln!(
                "lamp-voice history: the lamp's memory was wiped since the last start; the lamp's whole history is deleted on the account"
            );
            self.add_lamp();
        }
        self.epoch = Some(epoch.to_owned());
    }

    fn count(&self) -> usize {
        self.ids.values().map(BTreeSet::len).sum()
    }

    /// Everything of `person`: their ids wait no more.
    pub fn add_all(&mut self, person: &str) {
        self.ids.remove(person);
        self.all.insert(person.to_owned());
    }

    /// `ids` of `person`, unless all of theirs already waits (it takes
    /// these too); then bounded ([`MOST_PENDING`]): the known person with
    /// the most waiting is coalesced into their whole history, never the
    /// room's `""`; with only the room's left past it, its newest are let
    /// go. Each said in the journal as a count.
    pub fn add_ids(&mut self, person: &str, ids: impl IntoIterator<Item = String>) {
        if self.all.contains(person) {
            return;
        }
        let mut ids = ids.into_iter().peekable();
        if ids.peek().is_none() {
            return;
        }
        self.ids.entry(person.to_owned()).or_default().extend(ids);
        while self.count() > MOST_PENDING {
            let most = self
                .ids
                .iter()
                .filter(|(person, _)| !person.is_empty())
                .max_by_key(|(_, ids)| ids.len())
                .map(|(person, ids)| (person.clone(), ids.len()));
            match most {
                Some((most, waiting)) => {
                    eprintln!(
                        "lamp-voice history: the forget queue is full; {waiting} turns of one person go as their whole history"
                    );
                    self.add_all(&most);
                }
                None => {
                    // Only the room's past it: kept, never let go (the
                    // first builder's privacy read on #8125).
                    eprintln!(
                        "lamp-voice history: the forget queue holds {} of the room's turns past its bound; all are kept",
                        self.count() - MOST_PENDING
                    );
                    break;
                }
            }
        }
    }

    /// The next call: a person's whole history first, then up to
    /// [`MOST_IDS_A_CALL`] of one person's ids.
    pub fn next(&self) -> Option<Call> {
        if self.lamp {
            return Some(Call::Lamp);
        }
        if let Some(person) = self.all.first() {
            return Some(Call::All {
                person: person.clone(),
            });
        }
        let (person, ids) = self.ids.iter().find(|(_, ids)| !ids.is_empty())?;
        Some(Call::Ids {
            person: person.clone(),
            ids: ids.iter().take(MOST_IDS_A_CALL).cloned().collect(),
        })
    }

    /// `call` done: what it named waits no more.
    pub fn done(&mut self, call: &Call) {
        match call {
            Call::Lamp => self.lamp = false,
            Call::All { person } => {
                self.all.remove(person);
            }
            Call::Ids { person, ids } => {
                if let Some(kept) = self.ids.get_mut(person) {
                    for id in ids {
                        kept.remove(id);
                    }
                    if kept.is_empty() {
                        self.ids.remove(person);
                    }
                }
            }
        }
    }

    /// What a memory request `line` and its `answer` add (nothing unless it
    /// is one of memory's forgets, or a page of one, answered `ok`): a
    /// `forget_all` of a known person, their whole history; the exchanges
    /// the answer names, their turns. A `forget_all` of every person
    /// reaches nothing on the account, said in the journal.
    pub fn add_answered(&mut self, line: &Value, answer: &Value) {
        let op = line["op"].as_str().unwrap_or_default();
        let forget = lamp_msgs::MEMORY_FORGETS_TELLING_EXCHANGES.contains(&op);
        let unsent = op == lamp_msgs::MEMORY_HISTORY_UNSENT_OP;
        if answer["ok"] != true
            || !(forget || unsent || op == lamp_msgs::MEMORY_FORGOT_EXCHANGES_OP)
        {
            return;
        }
        if unsent {
            // A known person's forget everything the memory kept for us:
            // their whole history, never without one.
            for person in answer["all"].as_array().into_iter().flatten() {
                if let Some(person) = person
                    .as_str()
                    .filter(|p| lamp_msgs::known::is_person_id(p))
                {
                    self.add_all(person);
                }
            }
        }
        if op == "forget_all" {
            match line.get("person") {
                Some(Value::String(person)) if lamp_msgs::known::is_person_id(person) => {
                    self.add_all(person);
                }
                // Every person's: the lamp's whole history.
                Some(Value::Null) => self.add_lamp(),
                // The room's: its lines come by id below.
                _ => {}
            }
        }
        if let Some(scopes) = answer["exchanges"].as_object() {
            for (scope, ids) in scopes {
                let Some(person) = person_key(scope) else {
                    continue;
                };
                // Each the memory's own turn id, made by
                // `lamp_msgs::history_turn_id` with its store's epoch, and
                // of the scope it is told under: none else is sent.
                let ids = ids
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|turn| lamp_msgs::history_turn_scope(turn) == Some(scope.as_str()))
                    .map(str::to_owned);
                self.add_ids(person, ids);
            }
        }
    }
}

/// The queue: [`Pending`] kept in `path`, read again before each change so
/// that what another process of the voice added is never lost to a stale
/// copy, and written whole (a new file renamed over it) after.
pub struct Queue {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Queue {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    /// What waits now; an unreadable file is nothing (said once in the
    /// journal by [`Self::change`]).
    pub fn pending(&self) -> Pending {
        load(&self.path, false)
    }

    /// `change` applied to what waits, written back when it changed.
    pub fn change(&self, change: impl FnOnce(&mut Pending)) -> io::Result<Pending> {
        let _held = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let before = load(&self.path, true);
        let mut pending = before.clone();
        change(&mut pending);
        if pending.epoch != before.epoch
            && let Some(epoch) = &pending.epoch
        {
            write_epoch(&self.path, epoch)?;
        }
        if pending != before {
            write(&self.path, &pending)?;
        }
        Ok(pending)
    }
}

/// The queue's epoch kept beside it (`<file>.epoch`), for a queue file
/// found corrupt.
fn epoch_path(path: &Path) -> PathBuf {
    path.with_extension("epoch")
}

fn read_epoch(path: &Path) -> Option<String> {
    let epoch = fs::read_to_string(epoch_path(path)).ok()?;
    let epoch = epoch.trim();
    lamp_msgs::is_history_epoch(epoch).then(|| epoch.to_owned())
}

fn write_epoch(path: &Path, epoch: &str) -> io::Result<()> {
    write_whole(&epoch_path(path), epoch.as_bytes())
}

/// The queue at `path`: none, empty; one unreadable (corrupt), said in the
/// journal when `say`, starts again empty but from its epoch kept beside
/// it, so a wipe is still seen at the next start (Twelve's P3 on #8167).
fn load(path: &Path, say: bool) -> Pending {
    match read(path) {
        Ok(pending) => pending,
        Err(error) => {
            if say && error.kind() != io::ErrorKind::NotFound {
                eprintln!(
                    "lamp-voice history: the forget queue was unreadable (corrupt: {error}); it starts again empty from its kept epoch"
                );
            }
            Pending {
                epoch: read_epoch(path),
                ..Pending::default()
            }
        }
    }
}

fn read(path: &Path) -> io::Result<Pending> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write(path: &Path, pending: &Pending) -> io::Result<()> {
    if pending.is_empty() && pending.epoch.is_none() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    write_whole(
        path,
        &serde_json::to_vec(pending).map_err(io::Error::other)?,
    )
}

/// `bytes` written whole to `path` (a new file synced and renamed over it,
/// 0600), then its folder synced, so the rename survives a power cut
/// (Twelve's P3 on #8167).
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("no file name"))?;
    let new = path.with_file_name(format!(".{name}.new"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&new)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&new, path)?;
    if let Some(folder) = path.parent() {
        fs::File::open(folder)?.sync_all()?;
    }
    Ok(())
}

/// The voice's queue at [`FILE`], and the bell that wakes its sender.
pub fn queue() -> &'static (Queue, Arc<tokio::sync::Notify>) {
    static QUEUE: OnceLock<(Queue, Arc<tokio::sync::Notify>)> = OnceLock::new();
    QUEUE.get_or_init(|| (Queue::at(FILE), Arc::new(tokio::sync::Notify::new())))
}

/// The request `line` as sent to the memory: with the switch on, or off
/// after history was ever sent from this lamp ([`ever_sent`], a product
/// rollback), one of its forgets asks it to keep what it removes on its
/// disk until the worker has it on the queue (`"history_unsent":true`, in
/// the forget's own transaction, so no sync more before the reply). Off,
/// the rows wait in the memory, nothing sent, and the next start with the
/// switch on reads them ([`recover_at_start`]): a turn the account took
/// while the switch was on goes after a forget made while it is off.
/// Anything else, or nothing ever sent, as it is.
pub fn asking(line: &Value) -> std::borrow::Cow<'_, Value> {
    asking_when(line, || on() || ever_sent())
}

/// [`asking`], with `keep` whether the memory is asked to keep (read only
/// for a forget).
pub fn asking_when(line: &Value, keep: impl FnOnce() -> bool) -> std::borrow::Cow<'_, Value> {
    let op = line["op"].as_str().unwrap_or_default();
    if lamp_msgs::MEMORY_FORGETS_TELLING_EXCHANGES.contains(&op) && keep() {
        asking_with(line)
    } else {
        std::borrow::Cow::Borrowed(line)
    }
}

/// Whether history may ever have reached the account from this lamp: this
/// queue's file ([`FILE`], kept with the memory's epoch from the first start
/// with the switch on) or the save's marks ([`crate::history_send::MARKS_FILE`],
/// written from the save's first start) is there. Either is only removed
/// with the voice's state folder, at a reset.
pub fn ever_sent() -> bool {
    ever_sent_at(&[Path::new(FILE), Path::new(crate::history_send::MARKS_FILE)])
}

/// [`ever_sent`] for these files: any there, or not readable as missing.
pub fn ever_sent_at(files: &[&Path]) -> bool {
    files.iter().any(|file| {
        !matches!(fs::symlink_metadata(file), Err(error) if error.kind() == io::ErrorKind::NotFound)
    })
}

/// [`asking`], the switch taken as on.
pub fn asking_with(line: &Value) -> std::borrow::Cow<'_, Value> {
    let op = line["op"].as_str().unwrap_or_default();
    if !lamp_msgs::MEMORY_FORGETS_TELLING_EXCHANGES.contains(&op) || !line.is_object() {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut asked = line.clone();
    asked[lamp_msgs::MEMORY_HISTORY_UNSENT_FIELD] = Value::Bool(true);
    std::borrow::Cow::Owned(asked)
}

/// A memory request `line` to `socket` answered `answer`, on the voice's
/// way: what it removed is never posted by the save
/// (`LAMP_HISTORY_SEND`, whatever this switch), and, with the switch on, a
/// forget's answer is handed to the forget worker ([`work`]), which queues
/// it on disk, reads the pages of its rest and wakes the sender. Off,
/// nothing. No file is written and no page read here, so a forget's reply
/// never waits on the SD card or a page (Twelve's P2 on #7935).
pub fn answered(socket: &Path, line: &Value, answer: &Value) {
    crate::history_send::forget_seen(answer);
    if !on() {
        return;
    }
    answered_with(Worker::voices(), socket, line, answer);
}

/// [`answered`]'s hand-off to a given worker, the switch taken as on:
/// nothing but a channel send.
pub fn answered_with(worker: &Worker, socket: &Path, line: &Value, answer: &Value) -> bool {
    worker.hand(socket, line, answer)
}

/// A forget's answer, handed from the voice's way to [`work`].
#[derive(Debug)]
pub struct Job {
    socket: PathBuf,
    line: Value,
    answer: Value,
}

impl Job {
    /// The ids its answer names, for the journal's counts.
    fn ids(&self) -> usize {
        told_ids(&self.answer)
    }
}

/// How many turn ids `answer`'s `exchanges` names, for the journal.
fn told_ids(answer: &Value) -> usize {
    answer["exchanges"].as_object().map_or(0, |scopes| {
        scopes
            .values()
            .filter_map(Value::as_array)
            .map(Vec::len)
            .sum()
    })
}

/// What the voice hands its worker, in order.
#[derive(Debug)]
enum Handed {
    /// One forget's answer.
    Forget(Job),
    /// At the start: what the memory kept and the worker never said done
    /// ([`recover`]).
    Recover(PathBuf),
}

/// The most forgets waiting for the worker; past it (the worker stuck on
/// pages), one is said in the journal as not reached.
const MOST_HANDED: usize = 256;

/// How long a clean stop of the voice gives the worker to write what it
/// holds ([`drain`]): one queue write each, the pages cut short.
pub const DRAIN_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// How a page is asked: the memory's socket, the page's request.
type Ask = dyn Fn(&Path, &Value) -> Option<Value> + Send + Sync;

/// What the worker holds: forgets handed and not yet written, and their
/// ids, for [`Worker::drain`].
#[derive(Default)]
struct Flight {
    held: Mutex<(usize, usize)>,
    done: std::sync::Condvar,
    stopping: std::sync::atomic::AtomicBool,
    /// A forget not handed (the worker behind or stopped): what the memory
    /// kept of it is read again after the next job ([`recover`]).
    owed: std::sync::atomic::AtomicBool,
}

/// The forget worker: one thread taking the forgets in the order they were
/// answered, started on the first, and again if it has stopped (said in
/// the journal).
pub struct Worker {
    to: Mutex<Option<std::sync::mpsc::SyncSender<Handed>>>,
    queue: &'static Queue,
    bell: Arc<tokio::sync::Notify>,
    ask: Arc<Ask>,
    flight: Arc<Flight>,
}

impl Worker {
    /// A worker on `queue`, its pages asked by `ask`; no thread until the
    /// first forget.
    pub fn new(queue: &'static Queue, bell: Arc<tokio::sync::Notify>, ask: Arc<Ask>) -> Self {
        Self {
            to: Mutex::new(None),
            queue,
            bell,
            ask,
            flight: Arc::default(),
        }
    }

    /// The voice's worker, on its queue at [`FILE`], pages read from the
    /// memory's socket.
    pub fn voices() -> &'static Self {
        static WORKER: OnceLock<Worker> = OnceLock::new();
        WORKER.get_or_init(|| {
            let (queue, bell) = queue();
            Self::new(
                queue,
                bell.clone(),
                Arc::new(|socket: &Path, page: &Value| {
                    crate::tools::exchange_up_to(socket, page, PAGE_WITHIN, 65_536)
                }),
            )
        })
    }

    /// Its thread, started.
    fn start(&self) -> Option<std::sync::mpsc::SyncSender<Handed>> {
        let (to, from) = std::sync::mpsc::sync_channel::<Handed>(MOST_HANDED);
        let (queue, bell, ask, flight) = (
            self.queue,
            self.bell.clone(),
            self.ask.clone(),
            self.flight.clone(),
        );
        let started = std::thread::Builder::new()
            .name("history-forget".into())
            .spawn(move || {
                // The memory's socket last handed, for a read again owed
                // with no job to come (a hand-off that failed, a start whose
                // read failed): tried every [`RECOVER_AGAIN`].
                let mut known: Option<PathBuf> = None;
                loop {
                    let handed = if flight.owed.load(std::sync::atomic::Ordering::SeqCst)
                        && known.is_some()
                    {
                        match from.recv_timeout(RECOVER_AGAIN) {
                            Ok(handed) => Some(handed),
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        match from.recv() {
                            Ok(handed) => Some(handed),
                            Err(_) => break,
                        }
                    };
                    let stopping = || flight.stopping.load(std::sync::atomic::Ordering::SeqCst);
                    let (socket, ids) = match &handed {
                        Some(Handed::Forget(job)) => (job.socket.clone(), job.ids()),
                        Some(Handed::Recover(socket)) => (socket.clone(), 0),
                        None => (known.clone().unwrap_or_default(), 0),
                    };
                    known = Some(socket.clone());
                    let asked = |page: &Value| (!stopping()).then(|| ask(&socket, page)).flatten();
                    let mut owed = match &handed {
                        Some(Handed::Forget(job)) => {
                            !work(queue, &bell, job, asked, crate::history_send::forget_seen)
                        }
                        Some(Handed::Recover(_)) | None => true,
                    };
                    owed |= flight.owed.swap(false, std::sync::atomic::Ordering::SeqCst);
                    if owed {
                        // The epoch on every owed read, not only at a start:
                        // a forget of everyone keeps no row in the memory, so
                        // one that missed the hand-off comes back only as a
                        // changed epoch (Twelve's P2-1 on #8167).
                        let read = !stopping()
                            && epoch_seen(queue, &bell, asked)
                            && recover(queue, &bell, asked, crate::history_send::forget_seen);
                        if !read {
                            flight.owed.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                    if handed.is_some() {
                        flight.finish(ids);
                    }
                }
            });
        match started {
            Ok(_) => Some(to),
            Err(_) => {
                eprintln!("lamp-voice history: the forget worker could not start");
                None
            }
        }
    }

    /// `answer` to `line` handed to the thread, when it is one of
    /// memory's forgets answered `ok`; whether it was. Never blocks: a full
    /// hand-off is said in the journal as not reached; a stopped thread is
    /// said as stopped and started again once.
    pub fn hand(&self, socket: &Path, line: &Value, answer: &Value) -> bool {
        let op = line["op"].as_str().unwrap_or_default();
        if answer["ok"] != true || !lamp_msgs::MEMORY_FORGETS_TELLING_EXCHANGES.contains(&op) {
            return false;
        }
        let job = Job {
            socket: socket.to_owned(),
            line: line.clone(),
            answer: answer.clone(),
        };
        let ids = job.ids();
        self.send(Handed::Forget(job), ids)
    }

    /// [`holds`] for this worker and its queue.
    pub fn holds(&self, scope: &str) -> bool {
        self.flight.owed.load(std::sync::atomic::Ordering::SeqCst)
            || self
                .flight
                .held
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .0
                > 0
            || self.queue.pending().reaches(scope)
    }

    /// At the voice's start: the worker reads what the memory kept for
    /// the account and it never said done ([`recover`]), before any forget
    /// handed after. Never blocks.
    pub fn recover_from(&self, socket: &Path) -> bool {
        self.send(Handed::Recover(socket.to_owned()), 0)
    }

    /// [`unanswered`] for this worker: when `asked`, the request as sent,
    /// is a forget the memory was asked to keep ([`asking_with`]), the
    /// worker reads what it kept, as at a start ([`Worker::recover_from`]);
    /// whether it was handed. Any other request, nothing.
    pub fn unanswered(&self, socket: &Path, asked: &Value) -> bool {
        let op = asked["op"].as_str().unwrap_or_default();
        if asked[lamp_msgs::MEMORY_HISTORY_UNSENT_FIELD] != true
            || !lamp_msgs::MEMORY_FORGETS_TELLING_EXCHANGES.contains(&op)
        {
            return false;
        }
        eprintln!(
            "lamp-voice history: a forget's answer was not read; what the memory kept of it is read once it answers"
        );
        self.recover_from(socket)
    }

    /// `handed` (of `ids` turns) to the thread, never blocking; one not
    /// taken is said in the journal, and what the memory kept of it read
    /// again after the next job.
    fn send(&self, mut handed: Handed, ids: usize) -> bool {
        let mut to = self.to.lock().unwrap_or_else(|poison| poison.into_inner());
        for again in [false, true] {
            if to.is_none() {
                *to = self.start();
            }
            let Some(sender) = to.as_ref() else {
                break;
            };
            self.flight.add(ids);
            match sender.try_send(handed) {
                Ok(()) => return true,
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    self.flight.finish(ids);
                    self.flight
                        .owed
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    eprintln!(
                        "lamp-voice history: the forget worker is behind; a forget of {ids} turns is not reached on the account"
                    );
                    return false;
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(back)) => {
                    self.flight.finish(ids);
                    eprintln!(
                        "lamp-voice history: the forget worker had stopped{}",
                        if again { "" } else { "; it starts again" }
                    );
                    *to = None;
                    handed = back;
                }
            }
        }
        self.flight
            .owed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        eprintln!(
            "lamp-voice history: the forget worker is stopped; a forget of {ids} turns is not reached on the account"
        );
        false
    }

    /// A clean stop: the thread is given `within` to write every forget it
    /// holds (pages are cut short, said as not reached); what is still
    /// held after is said in the journal as a count. Whether it all was.
    pub fn drain(&self, within: std::time::Duration) -> bool {
        self.flight
            .stopping
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let until = std::time::Instant::now() + within;
        let mut held = self
            .flight
            .held
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        while held.0 > 0 {
            let Some(left) = until.checked_duration_since(std::time::Instant::now()) else {
                break;
            };
            held = self
                .flight
                .done
                .wait_timeout(held, left)
                .unwrap_or_else(|poison| poison.into_inner())
                .0;
        }
        if held.0 > 0 {
            eprintln!(
                "lamp-voice history: at the stop, {} forgets ({} turns) were not written; they are not reached on the account",
                held.0, held.1
            );
            return false;
        }
        true
    }
}

impl Flight {
    fn add(&self, ids: usize) {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        held.0 += 1;
        held.1 += ids;
    }

    fn finish(&self, ids: usize) {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        held.0 = held.0.saturating_sub(1);
        held.1 = held.1.saturating_sub(ids);
        self.done.notify_all();
    }
}

/// The voice stopping cleanly (`lamp-voice`'s `main`, after its run):
/// with the switch on and a forget handed, the worker is drained for at
/// most [`DRAIN_WITHIN`]. Otherwise nothing.
pub fn drain() {
    if on() {
        Worker::voices().drain(DRAIN_WITHIN);
    }
}

/// The second route starting with the memory at `socket`: with the switch
/// on, the worker reads what the memory kept for the account and it never
/// said done, a forget lost to a restart or a crash (the first builder's
/// switch-on condition on #8086). Off, nothing.
pub fn recover_at_start(socket: &Path) {
    if on() {
        Worker::voices().recover_from(socket);
    }
}

/// A forget sent whole to the memory at `socket` with no answer read (the
/// voice stopped waiting while the memory was still at it, or the answer
/// was lost; leg 7 on #8125, the first builder's call, item 2): with the
/// switch on, the worker reads what the memory kept of it
/// ([`Worker::unanswered`]), owed and read again every [`RECOVER_AGAIN`]
/// until the memory answers, so its delete is queued once the memory is
/// done, never only at the next start; while owed, the save holds
/// ([`holds`]). Off, its rows wait in the memory for a start with the
/// switch on, as every forget's ([`asking`]).
pub fn unanswered(socket: &Path, asked: &Value) {
    #[cfg(test)]
    UNANSWERED.with(|seen| seen.borrow_mut().push(asked.clone()));
    if on() {
        Worker::voices().unanswered(socket, asked);
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: each request [`unanswered`] was given on this thread.
    pub(crate) static UNANSWERED: std::cell::RefCell<Vec<Value>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The name this reads `memory/forgot` under, apart from the tone's.
const REMOVALS_SUBSCRIBER: &str = "history_removals";

/// The app's removal of a person (lamp-cloud's `forget_person` on the
/// memory's forget socket, the Reviewer-lead's HOLD on #8125) never passes
/// the voice's way: the memory keeps that person's whole history for the
/// account ([`recover`]'s rows) and tells `memory/forgot`. With the switch
/// on, a thread of its own reads that topic from the bus at `root` and,
/// for each known person's forget the memory tells, has the worker read
/// what the memory kept ([`Worker::recover_from`]), as at a start; once on
/// each subscribe too, so a removal while it was not reading is not
/// missed. Started once. Off, nothing.
pub fn watch_removals(root: &Path, socket: &Path) {
    static NEVER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static STARTED: OnceLock<()> = OnceLock::new();
    if on() && STARTED.set(()).is_ok() {
        watch_removals_with(Worker::voices(), root, socket, &NEVER);
    }
}

/// [`watch_removals`] for `worker`, until `stop`; whether its thread
/// started.
pub fn watch_removals_with(
    worker: &'static Worker,
    root: &Path,
    socket: &Path,
    stop: &'static std::sync::atomic::AtomicBool,
) -> bool {
    let (root, socket) = (root.to_owned(), socket.to_owned());
    let started = std::thread::Builder::new()
        .name("history-removals".into())
        .spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                if let Err(error) = removals(worker, &root, &socket, stop) {
                    eprintln!(
                        "lamp-voice history: memory/forgot could not be read ({}); an app's removal is read at the next start",
                        error.kind()
                    );
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
    if started.is_err() {
        eprintln!(
            "lamp-voice history: no thread reads memory/forgot; an app's removal is read at the next start"
        );
    }
    started.is_ok()
}

fn removals(
    worker: &Worker,
    root: &Path,
    socket: &Path,
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    let bus = lamp_bus::Bus::new(root)?;
    let mut forgot = bus.subscribe_typed(
        lamp_msgs::MemoryForgot::TOPIC,
        REMOVALS_SUBSCRIBER,
        lamp_msgs::MemoryForgot::TYPE,
    )?;
    worker.recover_from(socket);
    while !stop.load(std::sync::atomic::Ordering::SeqCst) {
        if let Some(message) = forgot.recv(std::time::Duration::from_millis(200))?
            && message.header.source == "memory"
            && let Ok(told) = message.parse::<lamp_msgs::MemoryForgot>()
            && told.check().is_ok()
            && told
                .person
                .as_deref()
                .is_some_and(lamp_msgs::known::is_person_id)
        {
            worker.recover_from(socket);
        }
        let _ = forgot.notices();
    }
    Ok(())
}

/// One handed forget, off the voice's way: queued on disk, then every
/// page of its rest asked by `ask` with its answer's token ([`pages`]),
/// each page's answer given to `seen` (the save's hold,
/// [`crate::history_send::forget_seen`]) and queued; then, when the memory
/// kept it (`"unsent":[from,to]`) and every id of it is on the queue, said
/// done to the memory. Whether nothing of it is owed: `false` when the
/// memory kept it and it was not all queued or not said done, read again by
/// [`recover`].
pub fn work(
    queue: &Queue,
    bell: &tokio::sync::Notify,
    job: &Job,
    ask: impl Fn(&Value) -> Option<Value>,
    seen: impl Fn(&Value),
) -> bool {
    let kept = job.answer["unsent"]
        .as_array()
        .and_then(|range| Some((range.first()?.as_u64()?, range.get(1)?.as_u64()?)));
    let every = job.line["op"] == "forget_all"
        && job.line.get("person") == Some(&Value::Null)
        && job.answer["ok"] == true;
    let mut whole = if every {
        // A forget of everyone wipes the store whole (a new epoch): the
        // lamp's whole history goes, with the new epoch kept, so the next
        // start does not take the wipe for another ([`epoch_seen`]).
        let epoch = epoch_of(&ask);
        match queue.change(|pending| {
            pending.add_lamp();
            pending.epoch = epoch.clone();
        }) {
            Ok(_) => {
                bell.notify_one();
                true
            }
            Err(_) => false,
        }
    } else {
        answered_into(true, queue, bell, &job.line, &job.answer)
    };
    if job.answer["exchanges_more"] == true {
        match job.answer["exchanges_token"].as_str() {
            None => {
                eprintln!(
                    "lamp-voice history: the rest of a forget's turns came with no token; they are not reached on the account"
                );
                whole = false;
            }
            Some(token) => {
                let (read, all) = pages(token, &ask);
                whole &= all;
                for (line, answer) in read {
                    seen(&answer);
                    whole &= answered_into(true, queue, bell, &line, &answer);
                }
            }
        }
    }
    let Some((from, to)) = kept else {
        return true;
    };
    whole && done(&ask, from, to)
}

/// The memory store's epoch, asked of it.
fn epoch_of(ask: impl Fn(&Value) -> Option<Value>) -> Option<String> {
    let answer = ask(&serde_json::json!({"op": lamp_msgs::MEMORY_HISTORY_EPOCH_OP}))?;
    let epoch = answer["epoch"].as_str()?;
    (answer["ok"] == true && lamp_msgs::is_history_epoch(epoch)).then(|| epoch.to_owned())
}

/// At a start: the memory's epoch compared with the last start's, kept
/// on the queue ([`Pending::saw_epoch`]): a wiped memory (a reset) deletes
/// the lamp's whole history. Whether it was read and kept.
pub fn epoch_seen(
    queue: &Queue,
    bell: &tokio::sync::Notify,
    ask: impl Fn(&Value) -> Option<Value>,
) -> bool {
    let Some(epoch) = epoch_of(ask) else {
        return false;
    };
    match queue.change(|pending| pending.saw_epoch(&epoch)) {
        Ok(pending) => {
            if pending.lamp {
                bell.notify_one();
            }
            true
        }
        Err(_) => false,
    }
}

/// Whether the save must not send a turn of memory `scope` now (the first
/// builder's privacy read on #8125, item 1): with the switch on, while a
/// forget is in the worker's hands or owed a read again, or a delete waits
/// that the turn could be in. Off, never.
pub fn holds(scope: &str) -> bool {
    on() && Worker::voices().holds(scope)
}

/// Turn ids the save posted before a forget covering them came (the
/// Reviewer-lead's must-fix on #8091), queued for delete; with the switch
/// off, nothing.
pub fn delete_posted(ids: &[String]) {
    if !on() {
        return;
    }
    let (queue, bell) = queue();
    delete_posted_into(queue, bell, ids);
}

/// [`delete_posted`] on a given queue.
pub fn delete_posted_into(queue: &Queue, bell: &tokio::sync::Notify, ids: &[String]) {
    let mut scopes: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for turn in ids {
        if let Some(person) = lamp_msgs::history_turn_scope(turn).and_then(person_key) {
            scopes.entry(person).or_default().push(turn.clone());
        }
    }
    if scopes.is_empty() {
        return;
    }
    // Into the ids even while that person's whole history waits: that
    // delete may already be out and ran before the post landed (Twelve's
    // P2-2 on #8167). `next` still sends the whole history first.
    match queue.change(|pending| {
        for (person, turns) in &scopes {
            pending
                .ids
                .entry((*person).to_owned())
                .or_default()
                .extend(turns.iter().cloned());
        }
    }) {
        Ok(_) => bell.notify_one(),
        Err(_) => {
            eprintln!("lamp-voice history: a posted turn's delete could not be queued (the file)")
        }
    }
}

/// The memory's rows `from` to `to` said done: they are on the queue.
fn done(ask: impl Fn(&Value) -> Option<Value>, from: u64, to: u64) -> bool {
    let line =
        serde_json::json!({"op": lamp_msgs::MEMORY_HISTORY_UNSENT_DONE_OP, "from": from, "to": to});
    ask(&line).is_some_and(|answer| answer["ok"] == true)
}

/// What the memory kept for the account and the worker never said done (a
/// forget waiting in the hand-off at a crash or a stop, one not handed, a
/// rest let go or lost to the memory's restart), read oldest first, a page
/// at a time ([`lamp_msgs::MEMORY_HISTORY_UNSENT_OP`], at most
/// [`MOST_PAGES`]): each page given to `seen`, queued, and only then said
/// done up to its last row. Whether all of it was. Read twice is harmless:
/// the queue is a set, and the server takes a turn already gone as done.
pub fn recover(
    queue: &Queue,
    bell: &tokio::sync::Notify,
    ask: impl Fn(&Value) -> Option<Value>,
    seen: impl Fn(&Value),
) -> bool {
    let (mut after, mut turns, mut wholes) = (0u64, 0usize, 0usize);
    let mut whole = false;
    for _ in 0..MOST_PAGES {
        let line = serde_json::json!({"op": lamp_msgs::MEMORY_HISTORY_UNSENT_OP, "after": after});
        let Some(answer) = ask(&line).filter(|answer| answer["ok"] == true) else {
            break;
        };
        let Some(to) = answer["to"].as_u64() else {
            whole = true;
            break;
        };
        seen(&answer);
        if !answered_into(true, queue, bell, &line, &answer) || !done(&ask, after + 1, to) {
            break;
        }
        turns += told_ids(&answer);
        wholes += answer["all"].as_array().map_or(0, Vec::len);
        after = to;
        if answer["more"] != true {
            whole = true;
            break;
        }
    }
    if turns + wholes > 0 {
        eprintln!(
            "lamp-voice history: {turns} turns and {wholes} whole histories a forget left unsent were queued again"
        );
    }
    if !whole {
        eprintln!(
            "lamp-voice history: what the memory kept of earlier forgets could not all be read; it is read again later"
        );
    }
    whole
}

/// How long one page may take.
const PAGE_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// The pages of `forgot_exchanges` for the forget whose answer gave
/// `token`, each its request and answer, asked by `ask` until one says no
/// `more`, at most [`MOST_PAGES`]. A page not answered, or refused (its
/// rest let go, or the memory restarted: `rest_gone`), ends them, said in
/// the journal as not reached; another forget's rest is never read.
pub fn pages(token: &str, ask: impl Fn(&Value) -> Option<Value>) -> (Vec<(Value, Value)>, bool) {
    let line = serde_json::json!({"op": lamp_msgs::MEMORY_FORGOT_EXCHANGES_OP, "token": token});
    let mut pages = Vec::new();
    for _ in 0..MOST_PAGES {
        match ask(&line) {
            Some(answer) if answer["ok"] == true => {
                let more = answer["more"] == true;
                pages.push((line.clone(), answer));
                if !more {
                    return (pages, true);
                }
            }
            _ => break,
        }
    }
    eprintln!(
        "lamp-voice history: the rest of a forget's turns could not be read now; what the memory kept of it is read again later"
    );
    (pages, false)
}

/// [`answered`] on a given queue, `on` or not, with no pages. Whether what
/// it adds is on the queue's file (nothing to add is).
pub fn answered_into(
    on: bool,
    queue: &Queue,
    bell: &tokio::sync::Notify,
    line: &Value,
    answer: &Value,
) -> bool {
    if !on {
        return true;
    }
    let mut added = Pending::default();
    added.add_answered(line, answer);
    if added.is_empty() {
        return true;
    }
    match queue.change(|pending| pending.add_answered(line, answer)) {
        Ok(pending) => {
            eprintln!(
                "lamp-voice history: a forget queued for the account's history ({} whole, {} turns waiting)",
                pending.all.len(),
                pending.count()
            );
            bell.notify_one();
            true
        }
        Err(_) => {
            eprintln!("lamp-voice history: a forget could not be queued (the file)");
            false
        }
    }
}

/// A memory for tests that keeps forgets' rows as lamp-memory's
/// `history_unsent` does, two to a page, every request it was asked kept in
/// order.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeMemory {
    pub rows: Mutex<Vec<(u64, String, Option<String>)>>,
    pub asked: Mutex<Vec<Value>>,
}

#[cfg(test)]
impl FakeMemory {
    /// `scope`'s `turns` kept as one forget's rows (`None`: the whole
    /// history), and their range, as the forget's answer says it.
    pub fn kept(&self, scope: &str, turns: &[Option<String>]) -> Value {
        let mut rows = self.rows.lock().unwrap();
        let from = rows.last().map_or(1, |row| row.0 + 1);
        for (n, turn) in turns.iter().enumerate() {
            rows.push((from + n as u64, scope.to_owned(), turn.clone()));
        }
        serde_json::json!([from, from + turns.len() as u64 - 1])
    }

    pub fn answer(&self, line: &Value) -> Option<Value> {
        self.asked.lock().unwrap().push(line.clone());
        let mut rows = self.rows.lock().unwrap();
        match line["op"].as_str() {
            Some(lamp_msgs::MEMORY_HISTORY_UNSENT_OP) => {
                let after = line["after"].as_u64().unwrap_or(0);
                let next: Vec<_> = rows.iter().filter(|row| row.0 > after).cloned().collect();
                let mut exchanges: BTreeMap<String, Vec<String>> = BTreeMap::new();
                let mut all = Vec::new();
                for (_, scope, turn) in next.iter().take(2) {
                    match turn {
                        Some(turn) => exchanges
                            .entry(scope.clone())
                            .or_default()
                            .push(turn.clone()),
                        None => all.push(scope.clone()),
                    }
                }
                Some(
                    serde_json::json!({"ok":true,"exchanges":exchanges,"all":all,
                    "to":next[..next.len().min(2)].last().map(|row| row.0),"more":next.len() > 2}),
                )
            }
            Some(lamp_msgs::MEMORY_HISTORY_EPOCH_OP) => {
                Some(serde_json::json!({"ok":true,"epoch":"0a1b2c3d"}))
            }
            Some(lamp_msgs::MEMORY_HISTORY_UNSENT_DONE_OP) => {
                let (from, to) = (line["from"].as_u64()?, line["to"].as_u64()?);
                rows.retain(|row| row.0 < from || row.0 > to);
                Some(serde_json::json!({"ok":true}))
            }
            _ => Some(serde_json::json!({"ok":false,"error":"rest_gone"})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MINH: &str = "p-0123456789abcdef";
    const LAN: &str = "p-fedcba9876543210";

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-history-forget-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("history-forget.json")
    }

    fn bell() -> tokio::sync::Notify {
        tokio::sync::Notify::new()
    }

    /// A memory store's epoch, as its answers carry it.
    const EPOCH: &str = "0a1b2c3d";

    fn turn_id(scope: &str, id: u64) -> String {
        lamp_msgs::history_turn_id(EPOCH, scope, id)
    }

    fn turns(scope: &str, ids: impl IntoIterator<Item = u64>) -> Vec<String> {
        ids.into_iter().map(|id| turn_id(scope, id)).collect()
    }

    /// Forget everything of a known person queues their whole history,
    /// with their person key, and never a delete of everything without one.
    #[test]
    fn forget_everything_queues_the_persons_whole_history() {
        let queue = Queue::at(scratch("all"));
        answered_into(
            true,
            &queue,
            &bell(),
            &json!({"op":"forget_all","person":MINH}),
            &json!({"ok":true,"removed":4}),
        );
        let next = queue.pending().next().unwrap();
        assert_eq!(next.body(), json!({"all":true,"person":MINH}));
        // The room's lines it took go by id, under the room's empty key.
        let mut pending = Pending::default();
        pending.add_answered(
            &json!({"op":"forget_all","person":MINH}),
            &json!({"ok":true,"removed":4,"exchanges":{"everyone":turns("everyone", [3])}}),
        );
        assert!(pending.all.contains(MINH));
        assert_eq!(pending.ids[""], BTreeSet::from([turn_id("everyone", 3)]));
        // Refused, or a forget_all with no known person: nothing.
        for (line, answer) in [
            (
                json!({"op":"forget_all","person":MINH}),
                json!({"ok":false}),
            ),
            (json!({"op":"forget_all"}), json!({"ok":true,"removed":1})),
            (
                json!({"op":"forget_all","person":"r-0123456789abcdef"}),
                json!({"ok":true,"removed":1}),
            ),
        ] {
            let mut pending = Pending::default();
            pending.add_answered(&line, &answer);
            assert!(pending.is_empty(), "{line} {answer}");
        }
    }

    /// "Forget that" queues exactly the exchanges the memory removed, by
    /// their turn ids, in one call for that person.
    #[test]
    fn forget_that_queues_exactly_the_removed_ids() {
        let queue = Queue::at(scratch("ids"));
        answered_into(
            true,
            &queue,
            &bell(),
            &json!({"op":"forget","person":MINH,"id":9,"selective":true}),
            &json!({"ok":true,"removed":3,"exchanges":{
                MINH:turns(MINH, [4, 6]),
                "r-0123456789abcdef":turns("r-0123456789abcdef", [1]),
            }}),
        );
        let pending = queue.pending();
        assert!(pending.all.is_empty());
        assert_eq!(
            pending.next().unwrap(),
            Call::Ids {
                person: MINH.into(),
                ids: vec![turn_id(MINH, 4), turn_id(MINH, 6)],
            }
        );
        assert_eq!(
            pending.next().unwrap().body(),
            json!({"ids":["0a1b2c3d.p-0123456789abcdef:4","0a1b2c3d.p-0123456789abcdef:6"]})
        );
        // A forget that removed no exchange queues nothing.
        let empty = Queue::at(scratch("ids-none"));
        answered_into(
            true,
            &empty,
            &bell(),
            &json!({"op":"forget","person":MINH,"id":9}),
            &json!({"ok":true,"removed":1}),
        );
        assert!(empty.pending().is_empty());
    }

    /// Switched off, a forget queues nothing at all.
    #[test]
    fn switched_off_queues_nothing() {
        let path = scratch("off");
        let queue = Queue::at(&path);
        answered_into(
            false,
            &queue,
            &bell(),
            &json!({"op":"forget_all","person":MINH}),
            &json!({"ok":true,"removed":4,"exchanges":{"everyone":turns("everyone", [3])}}),
        );
        assert!(queue.pending().is_empty());
        assert!(!path.exists());
    }

    /// What waits is on disk: a new queue at the same file (the voice
    /// started again) has it, and a call done leaves no file behind.
    #[test]
    fn the_queue_survives_a_restart() {
        let path = scratch("restart");
        Queue::at(&path)
            .change(|pending| {
                pending.add_ids(MINH, [turn_id(MINH, 1)]);
                pending.add_all(LAN);
            })
            .unwrap();
        let again = Queue::at(&path);
        let pending = again.pending();
        assert!(pending.all.contains(LAN));
        assert_eq!(pending.ids[MINH].len(), 1);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        again
            .change(|pending| {
                pending.done(&Call::All { person: LAN.into() });
                pending.done(&Call::Ids {
                    person: MINH.into(),
                    ids: vec![turn_id(MINH, 1)],
                });
            })
            .unwrap();
        assert!(!path.exists());
    }

    /// A forget of 600 turns in one scope sends 600 ids and no whole
    /// history, whatever pages they came in; a known person's forget
    /// everything that took 450 room lines sends those ids and never a
    /// whole-history delete for the room (the first builder's P1 on #7935).
    #[test]
    fn a_forget_never_deletes_more_than_the_lamp_forgot() {
        let ids = |scope: &str, range: std::ops::Range<u64>| turns(scope, range);
        let mut pending = Pending::default();
        pending.add_answered(
            &json!({"op":"exchanges_forget","person":MINH}),
            &json!({"ok":true,"removed":600,"exchanges":{MINH:ids(MINH, 0..400)},"exchanges_more":true}),
        );
        let (paged, all) = pages("t1", |_| {
            Some(json!({"ok":true,"exchanges":{MINH:ids(MINH, 400..600)},"more":false}))
        });
        assert!(all);
        assert_eq!(paged.len(), 1);
        for (line, answer) in &paged {
            pending.add_answered(line, answer);
        }
        assert!(pending.all.is_empty());
        let mut sent = 0;
        while let Some(call) = pending.next() {
            assert!(call.body().get("all").is_none(), "{}", call.body());
            sent += call.ids();
            pending.done(&call);
        }
        assert_eq!(sent, 600);
        let mut everything = Pending::default();
        everything.add_answered(
            &json!({"op":"forget_all","person":LAN}),
            &json!({"ok":true,"removed":900,"exchanges":{"everyone":ids("everyone", 0..400)},"exchanges_more":true}),
        );
        for (line, answer) in pages("t2", |_| {
            Some(json!({"ok":true,"exchanges":{"everyone":ids("everyone", 400..450)},"more":false}))
        })
        .0
        {
            everything.add_answered(&line, &answer);
        }
        assert_eq!(everything.all, BTreeSet::from([LAN.to_owned()]));
        assert_eq!(everything.ids[""].len(), 450);
        // A page not answered ends the pages.
        let (none, all) = pages("t3", |_| None);
        assert!(none.is_empty() && !all);
    }

    /// With 450 waiting, a second person's 51 stay 51; only past
    /// [`MOST_PENDING`] is the person with the most coalesced, never the
    /// room's; a call names at most [`MOST_IDS_A_CALL`].
    #[test]
    fn the_bound_coalesces_only_the_person_with_the_most_never_the_room() {
        let mut pending = Pending::default();
        pending.add_ids(MINH, (0..450).map(|id| turn_id(MINH, id)));
        pending.add_ids(LAN, (0..51).map(|id| turn_id(LAN, id)));
        assert!(pending.all.is_empty());
        assert_eq!(pending.ids[LAN].len(), 51);
        assert_eq!(pending.count(), 501);
        let mut full = Pending::default();
        full.add_ids(
            "",
            (0..MOST_PENDING as u64 - 10).map(|id| turn_id("everyone", id)),
        );
        full.add_ids(LAN, (0..5).map(|id| turn_id(LAN, id)));
        full.add_ids(MINH, (0..20).map(|id| turn_id(MINH, id)));
        assert_eq!(full.all, BTreeSet::from([MINH.to_owned()]));
        assert_eq!(full.ids[LAN].len(), 5);
        // Only the room's past it: kept, never let go and never its whole
        // (the first builder's privacy read on #8125).
        full.add_ids("", (0..100).map(|id| turn_id("everyone", 1_000_000 + id)));
        assert!(!full.all.contains(""));
        assert_eq!(full.ids[""].len(), MOST_PENDING - 10 + 100);
        let mut many = Pending::default();
        many.add_ids(LAN, (0..250).map(|id| turn_id(LAN, id)));
        assert_eq!(many.next().unwrap().ids(), MOST_IDS_A_CALL);
    }

    /// Only memory's forgets (and their pages) are read: another answer's
    /// `exchanges` queues nothing; a forget of every person's memory
    /// queues nothing and never a delete without a person; the room's own
    /// forget everything goes by its ids.
    #[test]
    fn only_a_forget_answer_queues_and_never_all_without_a_person() {
        for op in ["status", "recall", "search", "kept", "adopt", "remember"] {
            let mut pending = Pending::default();
            pending.add_answered(
                &json!({"op":op,"person":MINH}),
                &json!({"ok":true,"exchanges":{MINH:turns(MINH, [1, 2])}}),
            );
            assert!(pending.is_empty(), "{op}");
        }
        // A forget of every person's memory: the lamp's whole history, never
        // a person's delete without one.
        let mut every = Pending::default();
        every.add_answered(
            &json!({"op":"forget_all","person":null}),
            &json!({"ok":true,"removed":9}),
        );
        assert_eq!(every.next(), Some(Call::Lamp));
        assert!(every.all.is_empty());
        let mut room = Pending::default();
        room.add_answered(
            &json!({"op":"forget_all"}),
            &json!({"ok":true,"removed":2,"exchanges":{"everyone":turns("everyone", [1, 2])}}),
        );
        assert!(room.all.is_empty());
        assert_eq!(room.ids[""].len(), 2);
        room.add_all(MINH);
        while let Some(call) = room.next() {
            let body = call.body();
            if body.get("all").is_some() {
                assert!(body["person"].is_string(), "{body}");
            }
            room.done(&call);
        }
    }

    /// Only the memory's own turn ids, of the scope they are told under,
    /// are sent: an id without the store's epoch (an older memory's bare
    /// number or `<scope>:<id>`) or under another scope is let go, never
    /// sent as some other turn (#7771, Backend3's (a) on #7957).
    #[test]
    fn only_turn_ids_with_the_stores_epoch_are_sent() {
        let mut pending = Pending::default();
        pending.add_answered(
            &json!({"op":"forget","person":MINH,"id":9}),
            &json!({"ok":true,"removed":5,"exchanges":{MINH:[
                4,
                "p-0123456789abcdef:5",
                turn_id(LAN, 6),
                "0a1b2c3d.p-0123456789abcdef:x",
                turn_id(MINH, 7),
            ]}}),
        );
        assert_eq!(pending.ids[MINH], BTreeSet::from([turn_id(MINH, 7)]));
        assert_eq!(pending.count(), 1);
    }

    /// A forget's answer as memory gives it past one answer's room: the
    /// first ids, `exchanges_more` and its token.
    fn big(person: &str, scope: &str, told: std::ops::Range<u64>, token: &str) -> Job {
        Job {
            socket: PathBuf::from("/nonexistent"),
            line: json!({"op":"exchanges_forget","person":person}),
            answer: json!({"ok":true,"exchanges":{scope:turns(scope, told)},"exchanges_more":true,"exchanges_token":token}),
        }
    }

    /// Two big forgets in a row: each one's rest is read by its own token,
    /// and every id of both is queued (the first builder's P2 on #7935).
    #[test]
    fn two_forgets_in_a_row_each_send_their_own_rest() {
        let queue = Queue::at(scratch("two-rests"));
        let asked = Mutex::new(Vec::new());
        let memory = |page: &Value| -> Option<Value> {
            let token = page["token"].as_str().unwrap_or_default().to_owned();
            asked.lock().unwrap().push(token.clone());
            match token.as_str() {
                "a" => {
                    Some(json!({"ok":true,"exchanges":{MINH:turns(MINH, 100..250)},"more":false}))
                }
                "b" => Some(json!({"ok":true,"exchanges":{LAN:turns(LAN, 100..180)},"more":false})),
                _ => Some(json!({"ok":false,"error":"rest_gone"})),
            }
        };
        let (first, second) = (big(MINH, MINH, 0..100, "a"), big(LAN, LAN, 0..100, "b"));
        // Each page's answer goes to the save's hold too.
        let seen = Mutex::new(Vec::new());
        let hold = |answer: &Value| seen.lock().unwrap().push(answer["exchanges"].clone());
        work(&queue, &bell(), &first, memory, hold);
        work(&queue, &bell(), &second, memory, hold);
        assert_eq!(*asked.lock().unwrap(), ["a", "b"]);
        assert_eq!(
            *seen.lock().unwrap(),
            [
                json!({MINH:turns(MINH, 100..250)}),
                json!({LAN:turns(LAN, 100..180)})
            ]
        );
        let pending = queue.pending();
        assert_eq!(
            pending.ids[MINH],
            turns(MINH, 0..250).into_iter().collect::<BTreeSet<_>>()
        );
        assert_eq!(
            pending.ids[LAN],
            turns(LAN, 0..180).into_iter().collect::<BTreeSet<_>>()
        );
        assert!(pending.all.is_empty());
    }

    /// A forget whose rest the memory no longer keeps (replaced, or the
    /// memory restarted) reads no other forget's rest: only its own told
    /// ids are queued, and an answer with no token pages nothing.
    #[test]
    fn a_stale_rest_is_never_sent_for_another_forget() {
        let queue = Queue::at(scratch("stale-rest"));
        let asked = Mutex::new(Vec::new());
        // The memory keeps only "b"'s rest now.
        let memory = |page: &Value| -> Option<Value> {
            asked.lock().unwrap().push(page["token"].clone());
            (page["token"] == "b")
                .then(|| json!({"ok":true,"exchanges":{LAN:turns(LAN, 100..180)},"more":false}))
                .or_else(|| Some(json!({"ok":false,"error":"rest_gone"})))
        };
        work(
            &queue,
            &bell(),
            &big(MINH, MINH, 0..100, "a"),
            memory,
            |_| {},
        );
        let pending = queue.pending();
        assert_eq!(pending.ids[MINH].len(), 100);
        assert!(!pending.ids.contains_key(LAN));
        let mut tokenless = big(MINH, MINH, 0..100, "");
        tokenless
            .answer
            .as_object_mut()
            .unwrap()
            .remove("exchanges_token");
        work(&queue, &bell(), &tokenless, memory, |_| {});
        assert_eq!(*asked.lock().unwrap(), [json!("a")]);
        assert!(!queue.pending().ids.contains_key(LAN));
    }

    /// A worker on its own queue at `path`, pages answered by `ask`.
    fn test_worker(path: &Path, ask: Arc<Ask>) -> Worker {
        let queue: &'static Queue = Box::leak(Box::new(Queue::at(path)));
        Worker::new(queue, Arc::new(bell()), ask)
    }

    fn no_page() -> Arc<Ask> {
        Arc::new(|_: &Path, _: &Value| -> Option<Value> {
            panic!("no page is read on the voice's way")
        })
    }

    const SOCKET: &str = "/nonexistent/memory.sock";

    fn forget() -> Value {
        json!({"op":"forget","person":MINH,"id":9})
    }

    fn told() -> Value {
        json!({"ok":true,"removed":101,"exchanges":{MINH:turns(MINH, [4])},"exchanges_more":true,"exchanges_token":"a"})
    }

    /// On the voice's way a forget writes nothing and reads no page:
    /// [`answered_with`] only hands it to the worker's channel, never
    /// blocking when the worker is behind; the worker's [`work`] writes
    /// the queue (Twelve's P2 on #7935).
    #[test]
    fn the_forget_turn_does_no_disk_or_page_on_the_voice_path() {
        let path = scratch("voice-path");
        let worker = test_worker(&path, no_page());
        // The worker's thread held by the test: nothing takes the job.
        let (to, from) = std::sync::mpsc::sync_channel(1);
        *worker.to.lock().unwrap() = Some(to);
        let socket = Path::new(SOCKET);
        assert!(answered_with(&worker, socket, &forget(), &told()));
        assert!(!path.exists());
        // Not a forget, or refused: nothing handed.
        assert!(!answered_with(
            &worker,
            socket,
            &json!({"op":"status"}),
            &json!({"ok":true,"exchanges":{}})
        ));
        assert!(!answered_with(
            &worker,
            socket,
            &forget(),
            &json!({"ok":false})
        ));
        // The worker behind: let go at once, never waited on.
        let started = std::time::Instant::now();
        assert!(!answered_with(&worker, socket, &forget(), &told()));
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert!(!path.exists());
        let Handed::Forget(job) = from.try_recv().unwrap() else {
            panic!("a forget was handed")
        };
        assert_eq!(job.socket, socket);
        work(
            worker.queue,
            &bell(),
            &job,
            |_| Some(json!({"ok":true,"exchanges":{MINH:turns(MINH, [5])},"more":false})),
            |_| {},
        );
        assert!(path.exists());
        assert_eq!(worker.queue.pending().ids[MINH].len(), 2);
        // What the thread still held at a stop is counted, never waited
        // on past the bound.
        assert!(answered_with(&worker, socket, &forget(), &told()));
        let started = std::time::Instant::now();
        assert!(!worker.drain(std::time::Duration::from_millis(50)));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    /// A worker that has stopped (its channel's other end gone) is said as
    /// stopped and started again, and the forget still reaches the queue.
    #[test]
    fn a_stopped_worker_is_started_again() {
        let path = scratch("stopped");
        let worker = test_worker(
            &path,
            Arc::new(|_: &Path, _: &Value| {
                Some(json!({"ok":true,"exchanges":{MINH:turns(MINH, [5])},"more":false}))
            }),
        );
        let (to, from) = std::sync::mpsc::sync_channel(1);
        drop(from);
        *worker.to.lock().unwrap() = Some(to);
        // A forget with no rest: a drain cuts pages short, never its write.
        let small = json!({"ok":true,"removed":1,"exchanges":{MINH:turns(MINH, [4])}});
        assert!(answered_with(&worker, Path::new(SOCKET), &forget(), &small));
        assert!(worker.drain(DRAIN_WITHIN));
        assert_eq!(
            worker.queue.pending().ids[MINH],
            BTreeSet::from([turn_id(MINH, 4)])
        );
    }

    /// A clean stop gives the worker [`DRAIN_WITHIN`] to write every forget
    /// it holds: pages still coming are cut short, and each held forget's
    /// told ids are on disk before the drain returns.
    #[test]
    fn a_clean_stop_drains_what_the_worker_holds() {
        let path = scratch("drain");
        // A memory whose pages never end, each slow.
        let worker = test_worker(
            &path,
            Arc::new(|_: &Path, page: &Value| {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let n = page["token"].as_str().map_or(0, str::len) as u64;
                Some(json!({"ok":true,"exchanges":{LAN:turns(LAN, [1_000 + n])},"more":true}))
            }),
        );
        let socket = Path::new(SOCKET);
        assert!(answered_with(&worker, socket, &forget(), &told()));
        let second = json!({"ok":true,"removed":1,"exchanges":{MINH:turns(MINH, [8])}});
        assert!(answered_with(&worker, socket, &forget(), &second));
        let started = std::time::Instant::now();
        assert!(worker.drain(DRAIN_WITHIN));
        assert!(started.elapsed() < DRAIN_WITHIN);
        let pending = worker.queue.pending();
        assert!(pending.ids[MINH].contains(&turn_id(MINH, 4)));
        assert!(pending.ids[MINH].contains(&turn_id(MINH, 8)));
    }

    /// Waits, at most 5 s, until `worker` holds nothing, without stopping
    /// it (a drain cuts its memory asks short by design).
    fn settled(worker: &Worker) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < until {
            if worker.flight.held.lock().unwrap().0 == 0 {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    fn kept_turns(scope: &str, ids: impl IntoIterator<Item = u64>) -> Vec<Option<String>> {
        turns(scope, ids).into_iter().map(Some).collect()
    }

    /// A worker on a queue at `path`, its memory `memory`.
    fn remembering_worker(path: &Path, memory: &Arc<FakeMemory>) -> Worker {
        let memory = memory.clone();
        test_worker(
            path,
            Arc::new(move |_: &Path, line: &Value| memory.answer(line)),
        )
    }

    /// With the switch on, a forget asks the memory to keep what it
    /// removes; nothing else, and the line is otherwise the same.
    #[test]
    fn a_forget_asks_the_memory_to_keep_its_ids() {
        let line = forget();
        let asked = asking_with(&line);
        assert_eq!(asked["history_unsent"], true);
        let mut back = asked.into_owned();
        back.as_object_mut().unwrap().remove("history_unsent");
        assert_eq!(back, forget());
        for line in [
            json!({"op":"recall","person":MINH}),
            json!({"op":"forgot_exchanges","token":"a"}),
        ] {
            assert!(matches!(asking_with(&line), std::borrow::Cow::Borrowed(_)));
        }
    }

    /// The gap found on #8125: with the switch off after history was sent
    /// (a rollback), a forget still has the real memory keep its ids, and
    /// nothing is queued or sent; with nothing ever sent, nothing is kept;
    /// once the switch is on again, the next start queues them for the
    /// account and the memory lets them go.
    #[test]
    fn a_forget_with_the_switch_off_after_a_send_is_kept_until_on() {
        use lamp_memory::{days::Days, days_protocol, store};
        let state = scratch("off-after-send").parent().unwrap().to_path_buf();
        let files = [
            state.join("history-forget.json"),
            state.join("history-marks.json"),
        ];
        let files: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        let dir = state.join("memory");
        const NOW: u64 = 1_760_356_800_000;
        let store = std::cell::RefCell::new(
            Days::open(
                store::Options {
                    dir: dir.clone(),
                    keep_days: 30,
                    local: store::LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap(),
        );
        let ask = |line: &Value| -> Value {
            let answer =
                days_protocol::answer(&mut store.borrow_mut(), line.to_string().as_bytes(), NOW);
            serde_json::from_str(answer.line.trim()).unwrap()
        };
        let said = |n: u64| {
            ask(&json!({"op":"append","person":MINH,"at_unix_ms":NOW + n,"heard":"h","said":"s"}))
                ["id"]
                .as_u64()
                .unwrap()
        };
        let (one, two) = (said(1), said(2));
        let kept = || ask(&json!({"op": lamp_msgs::MEMORY_HISTORY_UNSENT_OP, "after": 0}));
        // Nothing ever sent: the forget is as it was, and nothing is kept.
        assert!(!ever_sent_at(&files));
        let line = json!({"op":"forget","person":MINH,"id":one});
        let asked = asking_when(&line, || ever_sent_at(&files));
        assert!(matches!(asked, std::borrow::Cow::Borrowed(_)));
        let answer = ask(&asked);
        assert_eq!(answer["ok"], true, "{answer}");
        assert!(answer.get("unsent").is_none(), "{answer}");
        assert_eq!(kept()["to"], Value::Null);
        // The save's marks are there (it ran once with its switch on): the
        // forget is kept, though this switch is off.
        fs::write(files[1], "{}").unwrap();
        assert!(ever_sent_at(&files));
        let line = json!({"op":"forget","person":MINH,"id":two});
        let answer = ask(&asking_when(&line, || ever_sent_at(&files)));
        assert_eq!(answer["ok"], true, "{answer}");
        assert!(answer["unsent"].is_array(), "{answer}");
        let epoch = store.borrow().history_epoch().to_owned();
        let turn = lamp_msgs::history_turn_id(&epoch, MINH, two);
        assert_eq!(kept()["exchanges"][MINH], json!([turn.clone()]));
        // Off, nothing goes anywhere: no queue file was written.
        assert!(!files[0].exists());
        // On again: the start's read queues it, and the memory lets it go.
        let queue = Queue::at(files[0]);
        assert!(recover(&queue, &bell(), |line| Some(ask(line)), |_| {}));
        assert_eq!(queue.pending().ids[MINH], BTreeSet::from([turn]));
        assert_eq!(kept()["to"], Value::Null);
        drop(store);
        let _ = fs::remove_dir_all(&state);
    }

    /// The first builder's condition on #8086: a forget the memory kept,
    /// handed to a worker that never wrote it (the voice restarted, or
    /// crashed, before), reaches the queue at the next start, and the
    /// memory lets it go only after.
    #[test]
    fn a_forget_lost_to_a_restart_is_queued_at_the_next_start() {
        let path = scratch("restart-lost");
        let memory = Arc::new(FakeMemory::default());
        // A big forget: 2 told, its rest of 3 never paged; and a forget
        // everything.
        let range = memory.kept(MINH, &kept_turns(MINH, 1..6));
        let whole = memory.kept(LAN, &[None]);
        {
            let before = remembering_worker(&path, &memory);
            // The worker's thread taken by the test and never run: the
            // voice stopped before it wrote anything.
            let (to, from) = std::sync::mpsc::sync_channel(4);
            *before.to.lock().unwrap() = Some(to);
            let mut answer = told();
            answer["unsent"] = range;
            assert!(answered_with(
                &before,
                Path::new(SOCKET),
                &forget(),
                &answer
            ));
            let mut all = json!({"ok":true,"removed":3});
            all["unsent"] = whole;
            let line = json!({"op":"forget_all","person":LAN});
            assert!(answered_with(&before, Path::new(SOCKET), &line, &all));
            drop(from);
        }
        assert!(!path.exists());
        let after = remembering_worker(&path, &memory);
        assert!(after.recover_from(Path::new(SOCKET)));
        assert!(settled(&after));
        let pending = after.queue.pending();
        assert_eq!(
            pending.ids[MINH],
            turns(MINH, 1..6).into_iter().collect::<BTreeSet<_>>()
        );
        assert_eq!(pending.all, BTreeSet::from([LAN.to_owned()]));
        assert!(memory.rows.lock().unwrap().is_empty());
    }

    /// The first builder's call on #8125 (item 2): a forget the voice
    /// stopped waiting for, its request sent whole and the memory still at
    /// it (leg 7: 1.5 s, the memory done about 15 s later), is queued for
    /// the account once the memory's late answer is in, by the worker's
    /// own read again, never only at the next start. While the memory is
    /// busy the read is owed, tried again every [`RECOVER_AGAIN`], and the
    /// save holds; once it is done, its rows go on the queue and are said
    /// done. A forget the memory was not asked to keep reads nothing.
    #[test]
    fn a_forget_answered_late_is_queued_without_a_start() {
        use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
        let path = scratch("late");
        let memory = Arc::new(FakeMemory::default());
        let busy = Arc::new(AtomicBool::new(true));
        let worker = {
            let (memory, busy) = (memory.clone(), busy.clone());
            test_worker(
                &path,
                Arc::new(move |_: &Path, line: &Value| {
                    (!busy.load(SeqCst)).then(|| memory.answer(line)).flatten()
                }),
            )
        };
        assert!(!worker.unanswered(Path::new(SOCKET), &forget()));
        let asked = asking_with(&forget()).into_owned();
        assert!(worker.unanswered(Path::new(SOCKET), &asked));
        assert!(settled(&worker));
        // The memory still at the forget: owed, and the save holds.
        assert!(worker.flight.owed.load(SeqCst));
        assert!(worker.holds("everyone"));
        assert!(worker.queue.pending().is_empty());
        // The memory done: what it removed is kept, its late answer lost.
        memory.kept(MINH, &kept_turns(MINH, [4, 5]));
        busy.store(false, SeqCst);
        let until = std::time::Instant::now() + 3 * RECOVER_AGAIN;
        while std::time::Instant::now() < until && !memory.rows.lock().unwrap().is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(memory.rows.lock().unwrap().is_empty());
        assert_eq!(
            worker.queue.pending().ids[MINH],
            turns(MINH, [4, 5]).into_iter().collect::<BTreeSet<_>>()
        );
        assert!(!worker.flight.owed.load(SeqCst));
        assert!(!worker.holds("everyone"));
    }

    /// A forget the worker queued whole is said done once, and never read
    /// again: the next start finds nothing, and once sent it is never
    /// queued again.
    #[test]
    fn a_forget_said_done_is_never_queued_again() {
        let path = scratch("said-done");
        let memory = Arc::new(FakeMemory::default());
        let worker = remembering_worker(&path, &memory);
        let mut small = json!({"ok":true,"removed":1,"exchanges":{MINH:turns(MINH, [4])}});
        small["unsent"] = memory.kept(MINH, &kept_turns(MINH, [4]));
        assert!(answered_with(&worker, Path::new(SOCKET), &forget(), &small));
        assert!(settled(&worker));
        assert!(memory.rows.lock().unwrap().is_empty());
        let dones = |memory: &FakeMemory| {
            memory
                .asked
                .lock()
                .unwrap()
                .iter()
                .filter(|line| line["op"] == lamp_msgs::MEMORY_HISTORY_UNSENT_DONE_OP)
                .count()
        };
        assert_eq!(dones(&memory), 1);
        // Sent: the queue is empty.
        let call = worker.queue.pending().next().unwrap();
        worker.queue.change(|pending| pending.done(&call)).unwrap();
        assert!(worker.queue.pending().is_empty());
        // Two more starts read nothing and queue nothing.
        for _ in 0..2 {
            assert!(recover(
                worker.queue,
                &bell(),
                |line| memory.answer(line),
                |_| {}
            ));
        }
        assert!(worker.queue.pending().is_empty());
        assert_eq!(dones(&memory), 1);
    }

    /// At a start, the memory's rows are read oldest first, a page at a
    /// time, each said done only once it is on the queue's file, and
    /// before any forget handed after; a page that could not be written is
    /// never said done, and is read again later.
    #[test]
    fn recovery_keeps_order_and_says_done_only_what_it_wrote() {
        let memory = Arc::new(FakeMemory::default());
        memory.kept(MINH, &kept_turns(MINH, 1..4));
        memory.kept("everyone", &kept_turns("everyone", [7]));
        // The queue's folder is missing: nothing can be written.
        let broken = Queue::at(std::env::temp_dir().join("lamp-history-forget-none/no/queue.json"));
        assert!(!recover(
            &broken,
            &bell(),
            |line| memory.answer(line),
            |_| {}
        ));
        assert_eq!(memory.rows.lock().unwrap().len(), 4);
        memory.asked.lock().unwrap().clear();
        let path = scratch("order");
        let worker = remembering_worker(&path, &memory);
        // A forget answered just as the voice starts: its row is in the
        // memory before the start's read, its hand-off after.
        let mut later = json!({"ok":true,"removed":1,"exchanges":{MINH:turns(MINH, [9])}});
        later["unsent"] = memory.kept(MINH, &kept_turns(MINH, [9]));
        assert!(worker.recover_from(Path::new(SOCKET)));
        assert!(answered_with(&worker, Path::new(SOCKET), &forget(), &later));
        assert!(settled(&worker));
        let asked: Vec<(String, Value, Value)> = memory
            .asked
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line["op"] != lamp_msgs::MEMORY_HISTORY_EPOCH_OP)
            .map(|line| {
                (
                    line["op"].as_str().unwrap().to_owned(),
                    line.get("after")
                        .or(line.get("from"))
                        .cloned()
                        .unwrap_or_default(),
                    line.get("to").cloned().unwrap_or_default(),
                )
            })
            .collect();
        let unsent = lamp_msgs::MEMORY_HISTORY_UNSENT_OP.to_owned();
        let done = lamp_msgs::MEMORY_HISTORY_UNSENT_DONE_OP.to_owned();
        // The start's pages first, two rows each, each said done after its
        // own write; the later forget's own range only after them.
        assert_eq!(
            asked[..4],
            [
                (unsent.clone(), json!(0), Value::Null),
                (done.clone(), json!(1), json!(2)),
                (unsent.clone(), json!(2), Value::Null),
                (done.clone(), json!(3), json!(4)),
            ]
        );
        assert!(memory.rows.lock().unwrap().is_empty());
        let pending = worker.queue.pending();
        assert_eq!(
            pending.ids[MINH],
            turns(MINH, [1, 2, 3, 9])
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(pending.ids[""], BTreeSet::from([turn_id("everyone", 7)]));
    }

    /// The first builder's privacy read on #8125, item 2: a memory wiped
    /// since the last start (a reset: its epoch changed) deletes the
    /// lamp's whole history, before anything else, once; and a forget of
    /// everyone does too, with the new epoch kept so the next start does
    /// not take that wipe for another.
    #[test]
    fn a_reset_deletes_the_lamps_whole_history() {
        let queue = Queue::at(scratch("reset"));
        let epoch = |e: &'static str| move |_: &Value| Some(json!({"ok":true,"epoch":e}));
        assert!(epoch_seen(&queue, &bell(), epoch("0a1b2c3d")));
        assert!(queue.pending().is_empty());
        queue
            .change(|pending| pending.add_ids(MINH, [turn_id(MINH, 1)]))
            .unwrap();
        // The lamp was reset: the memory's store is new.
        assert!(epoch_seen(&queue, &bell(), epoch("9f8e7d6c")));
        let pending = queue.pending();
        assert_eq!(pending.next(), Some(Call::Lamp));
        assert_eq!(
            Call::Lamp.body(),
            serde_json::from_str::<Value>(WHOLE_LAMP_BODY).unwrap()
        );
        assert!(Call::Lamp.body()["person"].is_null());
        queue.change(|pending| pending.done(&Call::Lamp)).unwrap();
        // The same store at the next start: nothing more.
        assert!(epoch_seen(&queue, &bell(), epoch("9f8e7d6c")));
        assert!(queue.pending().is_empty());
        // A memory not answering: not taken as a wipe.
        assert!(!epoch_seen(&queue, &bell(), |_: &Value| None));
        assert!(queue.pending().is_empty());
        // A forget of everyone: the lamp's whole history, the new epoch
        // kept with it.
        let job = Job {
            socket: PathBuf::from(SOCKET),
            line: json!({"op":"forget_all","person":null}),
            answer: json!({"ok":true,"removed":4}),
        };
        assert!(work(&queue, &bell(), &job, epoch("11223344"), |_| {}));
        assert_eq!(queue.pending().next(), Some(Call::Lamp));
        queue.change(|pending| pending.done(&Call::Lamp)).unwrap();
        assert!(epoch_seen(&queue, &bell(), epoch("11223344")));
        assert!(queue.pending().is_empty());
    }

    /// The first builder's privacy read on #8125, item 1: while a forget is
    /// in the worker's hands, owed a read again, or its delete waits, the
    /// save holds the turns it could reach: that person's, or every turn
    /// for the lamp's whole history; never another person's.
    #[test]
    fn a_waiting_forget_holds_the_turns_it_could_reach() {
        let path = scratch("holds");
        let worker = test_worker(&path, no_page());
        assert!(!worker.holds(MINH));
        worker
            .queue
            .change(|pending| pending.add_ids(MINH, [turn_id(MINH, 1)]))
            .unwrap();
        assert!(worker.holds(MINH));
        assert!(!worker.holds(LAN));
        assert!(!worker.holds("everyone"));
        worker.queue.change(|pending| pending.add_all(LAN)).unwrap();
        assert!(worker.holds(LAN));
        worker.queue.change(|pending| pending.add_lamp()).unwrap();
        assert!(worker.holds("everyone"));
        worker
            .queue
            .change(|pending| pending.done(&Call::Lamp))
            .unwrap();
        assert!(!worker.holds("everyone"));
        // A forget not handed: owed a read again, everything waits.
        worker
            .flight
            .owed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(worker.holds("everyone"));
    }

    /// The Reviewer-lead's must-fix on #8091: the ids of turns posted while
    /// a forget came are queued for delete under their person.
    #[test]
    fn turns_posted_before_a_forget_came_are_queued_for_delete() {
        let queue = Queue::at(scratch("posted"));
        delete_posted_into(
            &queue,
            &bell(),
            &[turn_id(MINH, 3), turn_id("everyone", 4), "bad".into()],
        );
        let pending = queue.pending();
        assert_eq!(pending.ids[MINH], BTreeSet::from([turn_id(MINH, 3)]));
        assert_eq!(pending.ids[""], BTreeSet::from([turn_id("everyone", 4)]));
    }

    /// Twelve's P2-2 on #8167, on the real queue: a person's whole-history
    /// delete goes out while their turn is still being posted; the server
    /// runs the delete, then stores the post, which answers late. Its id is
    /// still queued and deleted after the whole history is done.
    #[test]
    fn a_post_answering_after_its_persons_whole_delete_went_out_is_still_deleted() {
        let queue = Queue::at(scratch("posted-all"));
        queue.change(|pending| pending.add_all(MINH)).unwrap();
        let out = queue.pending().next().unwrap();
        assert_eq!(
            out,
            Call::All {
                person: MINH.into()
            }
        );
        delete_posted_into(&queue, &bell(), &[turn_id(MINH, 7)]);
        // The whole history still goes first.
        assert_eq!(queue.pending().next(), Some(out.clone()));
        queue.change(|pending| pending.done(&out)).unwrap();
        assert_eq!(
            queue.pending().next(),
            Some(Call::Ids {
                person: MINH.into(),
                ids: vec![turn_id(MINH, 7)]
            })
        );
    }

    /// Twelve's P2-1 on #8167: a forget of everyone keeps no row in the
    /// memory; when its hand-off is full, the next owed read sees the
    /// memory's new epoch and the lamp's whole history is still deleted,
    /// with no restart.
    #[test]
    fn a_forget_of_everyone_that_missed_a_full_hand_off_is_still_deleted() {
        let path = scratch("missed-everyone");
        let memory = Arc::new(FakeMemory::default());
        let wiped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker = {
            let (memory, wiped) = (memory.clone(), wiped.clone());
            test_worker(
                &path,
                Arc::new(move |_: &Path, line: &Value| {
                    if line["op"] == lamp_msgs::MEMORY_HISTORY_EPOCH_OP {
                        let epoch = if wiped.load(std::sync::atomic::Ordering::SeqCst) {
                            "11223344"
                        } else {
                            EPOCH
                        };
                        return Some(json!({"ok":true,"epoch":epoch}));
                    }
                    memory.answer(line)
                }),
            )
        };
        // Seen at the start.
        worker
            .queue
            .change(|pending| pending.saw_epoch(EPOCH))
            .unwrap();
        // The hand-off full: the forget of everyone is not taken.
        let (to, _held) = std::sync::mpsc::sync_channel(0);
        *worker.to.lock().unwrap() = Some(to);
        wiped.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!answered_with(
            &worker,
            Path::new(SOCKET),
            &json!({"op":"forget_all","person":null}),
            &json!({"ok":true,"removed":4})
        ));
        assert!(worker.holds("everyone"));
        // The worker again, a later forget handed: its owed read deletes
        // the lamp's whole history.
        *worker.to.lock().unwrap() = None;
        let later = json!({"ok":true,"removed":1,"exchanges":{LAN:turns(LAN, [2])}});
        assert!(answered_with(
            &worker,
            Path::new(SOCKET),
            &json!({"op":"forget","person":LAN,"id":2}),
            &later
        ));
        assert!(settled(&worker));
        let pending = worker.queue.pending();
        assert_eq!(pending.next(), Some(Call::Lamp));
        assert_eq!(pending.epoch.as_deref(), Some("11223344"));
        assert!(worker.holds("everyone"));
    }

    /// Twelve's P3 on #8167: a corrupt queue file is said in the journal
    /// and starts again from its kept epoch, so a wipe since is still seen.
    #[test]
    fn a_corrupt_queue_keeps_its_epoch() {
        let path = scratch("corrupt");
        let queue = Queue::at(&path);
        queue.change(|pending| pending.saw_epoch(EPOCH)).unwrap();
        fs::write(&path, b"{not json").unwrap();
        assert_eq!(queue.pending().epoch.as_deref(), Some(EPOCH));
        let epoch = |_: &Value| Some(json!({"ok":true,"epoch":"9f8e7d6c"}));
        assert!(epoch_seen(&queue, &bell(), epoch));
        assert_eq!(queue.pending().next(), Some(Call::Lamp));
    }
}
