//! Forget reaches the account's history (#7771): what a forget removes on
//! the lamp is deleted from the conversation history the account keeps
//! too, by the lamp's own cloud API (`POST <base>/history/turns/delete`,
//! with the lamp's key). Only with `LAMP_HISTORY_FORGET=on` (off: nothing
//! is queued or sent).
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
//! - **not reached:** a forget of everything of every person, or a page
//!   that could not be read, is said in the journal as not reaching the
//!   account (the voice sends neither today).
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

/// `on` and nothing else: forgets reach the account's history. Off in
/// every unit.
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
    /// Everything of one person (`""` the room's): never without one.
    All { person: String },
    /// These turns, at most [`MOST_IDS_A_CALL`], of one person.
    Ids { person: String, ids: Vec<String> },
}

impl Call {
    /// The request's body.
    pub fn body(&self) -> Value {
        match self {
            Self::All { person } => serde_json::json!({"all": true, "person": person}),
            Self::Ids { ids, .. } => serde_json::json!({"ids": ids}),
        }
    }

    /// How many ids it names (an `all` none), for the journal.
    pub fn ids(&self) -> usize {
        match self {
            Self::All { .. } => 0,
            Self::Ids { ids, .. } => ids.len(),
        }
    }
}

/// What waits, as kept on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// Person keys whose whole history is to go.
    #[serde(default)]
    pub all: BTreeSet<String>,
    /// Each person key's turn ids to go.
    #[serde(default)]
    pub ids: BTreeMap<String, BTreeSet<String>>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.all.is_empty() && self.ids.is_empty()
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
                    let over = self.count() - MOST_PENDING;
                    if let Some(room) = self.ids.get_mut("") {
                        for _ in 0..over {
                            room.pop_last();
                        }
                    }
                    eprintln!(
                        "lamp-voice history: the forget queue is full; {over} of the room's turns are not reached on the account"
                    );
                }
            }
        }
    }

    /// The next call: a person's whole history first, then up to
    /// [`MOST_IDS_A_CALL`] of one person's ids.
    pub fn next(&self) -> Option<Call> {
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
        if answer["ok"] != true || !(forget || op == lamp_msgs::MEMORY_FORGOT_EXCHANGES_OP) {
            return;
        }
        if op == "forget_all" {
            match line.get("person") {
                Some(Value::String(person)) if lamp_msgs::known::is_person_id(person) => {
                    self.add_all(person);
                }
                // Every person's: memory tells no ids for it.
                Some(Value::Null) => eprintln!(
                    "lamp-voice history: a forget of every person's memory is not reached on the account"
                ),
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
        read(&self.path).unwrap_or_default()
    }

    /// `change` applied to what waits, written back when it changed.
    pub fn change(&self, change: impl FnOnce(&mut Pending)) -> io::Result<Pending> {
        let _held = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let before = match read(&self.path) {
            Ok(pending) => pending,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Pending::default(),
            Err(_) => {
                eprintln!(
                    "lamp-voice history: the forget queue could not be read; it starts again empty"
                );
                Pending::default()
            }
        };
        let mut pending = before.clone();
        change(&mut pending);
        if pending != before {
            write(&self.path, &pending)?;
        }
        Ok(pending)
    }
}

fn read(path: &Path) -> io::Result<Pending> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write(path: &Path, pending: &Pending) -> io::Result<()> {
    if pending.is_empty() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
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
    file.write_all(&serde_json::to_vec(pending).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&new, path)
}

/// The voice's queue at [`FILE`], and the bell that wakes its sender.
pub fn queue() -> &'static (Queue, Arc<tokio::sync::Notify>) {
    static QUEUE: OnceLock<(Queue, Arc<tokio::sync::Notify>)> = OnceLock::new();
    QUEUE.get_or_init(|| (Queue::at(FILE), Arc::new(tokio::sync::Notify::new())))
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
        self.answer["exchanges"].as_object().map_or(0, |scopes| {
            scopes
                .values()
                .filter_map(Value::as_array)
                .map(Vec::len)
                .sum()
        })
    }
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
}

/// The forget worker: one thread taking the forgets in the order they were
/// answered, started on the first, and again if it has stopped (said in
/// the journal).
pub struct Worker {
    to: Mutex<Option<std::sync::mpsc::SyncSender<Job>>>,
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
    fn start(&self) -> Option<std::sync::mpsc::SyncSender<Job>> {
        let (to, from) = std::sync::mpsc::sync_channel::<Job>(MOST_HANDED);
        let (queue, bell, ask, flight) = (
            self.queue,
            self.bell.clone(),
            self.ask.clone(),
            self.flight.clone(),
        );
        let started = std::thread::Builder::new()
            .name("history-forget".into())
            .spawn(move || {
                for job in from {
                    let stopping = || flight.stopping.load(std::sync::atomic::Ordering::SeqCst);
                    work(
                        queue,
                        &bell,
                        &job,
                        |page| (!stopping()).then(|| ask(&job.socket, page)).flatten(),
                        crate::history_send::forget_seen,
                    );
                    flight.finish(job.ids());
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
        let mut job = Job {
            socket: socket.to_owned(),
            line: line.clone(),
            answer: answer.clone(),
        };
        let ids = job.ids();
        let mut to = self.to.lock().unwrap_or_else(|poison| poison.into_inner());
        for again in [false, true] {
            if to.is_none() {
                *to = self.start();
            }
            let Some(sender) = to.as_ref() else {
                break;
            };
            self.flight.add(ids);
            match sender.try_send(job) {
                Ok(()) => return true,
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    self.flight.finish(ids);
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
                    job = back;
                }
            }
        }
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

/// One handed forget, off the voice's way: queued on disk, then every
/// page of its rest asked by `ask` with its answer's token ([`pages`]),
/// each page's answer given to `seen` (the save's hold,
/// [`crate::history_send::forget_seen`]) and queued.
pub fn work(
    queue: &Queue,
    bell: &tokio::sync::Notify,
    job: &Job,
    ask: impl Fn(&Value) -> Option<Value>,
    seen: impl Fn(&Value),
) {
    answered_into(true, queue, bell, &job.line, &job.answer);
    if job.answer["exchanges_more"] != true {
        return;
    }
    let Some(token) = job.answer["exchanges_token"].as_str() else {
        eprintln!(
            "lamp-voice history: the rest of a forget's turns came with no token; they are not reached on the account"
        );
        return;
    };
    for (line, answer) in pages(token, ask) {
        seen(&answer);
        answered_into(true, queue, bell, &line, &answer);
    }
}

/// How long one page may take.
const PAGE_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// The pages of `forgot_exchanges` for the forget whose answer gave
/// `token`, each its request and answer, asked by `ask` until one says no
/// `more`, at most [`MOST_PAGES`]. A page not answered, or refused (its
/// rest let go, or the memory restarted: `rest_gone`), ends them, said in
/// the journal as not reached; another forget's rest is never read.
pub fn pages(token: &str, ask: impl Fn(&Value) -> Option<Value>) -> Vec<(Value, Value)> {
    let line = serde_json::json!({"op": lamp_msgs::MEMORY_FORGOT_EXCHANGES_OP, "token": token});
    let mut pages = Vec::new();
    for _ in 0..MOST_PAGES {
        match ask(&line) {
            Some(answer) if answer["ok"] == true => {
                let more = answer["more"] == true;
                pages.push((line.clone(), answer));
                if !more {
                    return pages;
                }
            }
            _ => break,
        }
    }
    eprintln!(
        "lamp-voice history: the rest of a forget's turns could not be read; they are not reached on the account"
    );
    pages
}

/// [`answered`] on a given queue, `on` or not, with no pages.
pub fn answered_into(
    on: bool,
    queue: &Queue,
    bell: &tokio::sync::Notify,
    line: &Value,
    answer: &Value,
) {
    if !on {
        return;
    }
    let mut added = Pending::default();
    added.add_answered(line, answer);
    if added.is_empty() {
        return;
    }
    match queue.change(|pending| pending.add_answered(line, answer)) {
        Ok(pending) => {
            eprintln!(
                "lamp-voice history: a forget queued for the account's history ({} whole, {} turns waiting)",
                pending.all.len(),
                pending.count()
            );
            bell.notify_one();
        }
        Err(_) => eprintln!("lamp-voice history: a forget could not be queued (the file)"),
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
            (json!({"op":"forget_all","person":null}), json!({"ok":true})),
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
        let paged = pages("t1", |_| {
            Some(json!({"ok":true,"exchanges":{MINH:ids(MINH, 400..600)},"more":false}))
        });
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
        }) {
            everything.add_answered(&line, &answer);
        }
        assert_eq!(everything.all, BTreeSet::from([LAN.to_owned()]));
        assert_eq!(everything.ids[""].len(), 450);
        // A page not answered ends the pages.
        assert!(pages("t3", |_| None).is_empty());
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
        // Only the room's past it: its newest are let go, never its whole.
        full.add_ids("", (0..100).map(|id| turn_id("everyone", 1_000_000 + id)));
        assert!(!full.all.contains(""));
        assert!(full.count() <= MOST_PENDING);
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
        let mut every = Pending::default();
        every.add_answered(
            &json!({"op":"forget_all","person":null}),
            &json!({"ok":true,"removed":9}),
        );
        assert!(every.is_empty());
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
        let job: Job = from.try_recv().unwrap();
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
}
