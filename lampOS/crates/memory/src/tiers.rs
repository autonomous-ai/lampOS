//! The tiered store beside Days, behind `LAMP_MEMORY_TIERS` (#4746/#4747;
//! `docs/memory-design.md`, "Wiring into lamp-memory"). Off unless set.
//!
//! - `shadow`: every exchange and kept thing Days takes is also written to
//!   the tiered store ([`lamp_memory_tiers`]), every removal Days makes is
//!   applied there too, and the nightly sleep runs; every answer is Days',
//!   byte for byte.
//! - `on`: as shadow, and a `recall` or `search` that Days answered for a
//!   mirrored scope also carries `"tiers"`: what the tiered store recalls for
//!   the same words, with the Days ids it came from. Today's voice reads no
//!   such field, so the person hears nothing new until a voice reads it.
//!
//! All the tiers' work runs on one thread of its own, fed by a bounded
//! queue: a request's only extra work is taking the change log Days wrote in
//! memory ([`crate::tiers_log`]) and one `try_send`. A full queue never
//! loses a forget: the queue's epoch moves on, the worker drops what was
//! queued before and empties the tiered store. A failing tiered store is
//! logged and counted and never changes a Days answer.
//!
//! What the tiers hold is a subset of what Days holds, by construction:
//! each tiered record is mapped to the Days entry it was written from; a
//! removal in Days removes the mapped records with everything derived from
//! them (facts, lines, episodes: the library's own forget); a forget of
//! everything of a scope deletes that scope's tiered store with its
//! fingerprint key; and each start removes whatever the tiers hold that Days
//! no longer does, or that no mapping names. The room's scope
//! (`everyone`, `r-everyone`) is never mirrored: an unplaced voice's words
//! are not evidence of anyone's (the design's attribution rule).
use crate::{
    days::Days,
    protocol::{self, Forgot},
    server::Memory,
    store::{LocalTime, Person, private_dir},
    tiers_log::{Change, Held},
};
use lamp_memory_tiers::{
    Access, Arm, Cues, FactKind, Incoming, PassKind, RECALL_BUDGET, RecallQuery, SleepInput,
    Speaker, Store, Strength,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs, io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// The switch, read by lamp-memory at start: `shadow` or `on`; off unless set.
pub const SWITCH: &str = "LAMP_MEMORY_TIERS";
/// Beside the Days database. Its dot keeps it from being read as a legacy
/// person's directory (a person id has none).
pub const DIR: &str = "memory-tiers.d";
const MIRROR: &str = "mirror.sqlite3";
/// Jobs waiting for the worker. A burst past it empties the tiers (above).
pub const QUEUE: usize = 256;
/// In `on`, the most a `recall` or `search` waits for the tiers' answer
/// beside Days'; a later one is left out and counted.
pub const ANSWER_WAIT: Duration = Duration::from_millis(50);
/// The nightly sleep runs once per local day, between these hours.
pub const NIGHT_FROM_HOUR: u8 = 3;
pub const NIGHT_UNTIL_HOUR: u8 = 6;
const IDLE: Duration = Duration::from_secs(60);
const SETTLE: Duration = Duration::from_secs(30);
/// After the worker is found gone, the most a request waits for its
/// thread to finish before emptying the tiers (tried again on the next).
const STOPPING: Duration = Duration::from_millis(200);
const OPEN_STORES: usize = 16;
const NIGHT_PASSES: usize = 4;
const RECALL_BYTES: usize = 2048;

/// A switch value other than off, `shadow` or `on`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Shadow,
    On,
}

impl Mode {
    /// The switch's value. Unset, empty or `off` is off (`Ok(None)`);
    /// anything else but `shadow` or `on` is refused, and the caller runs
    /// with the tiers off.
    pub fn parse(value: Option<&str>) -> Result<Option<Self>, UnknownMode> {
        match value {
            None | Some("" | "off") => Ok(None),
            Some("shadow") => Ok(Some(Self::Shadow)),
            Some("on") => Ok(Some(Self::On)),
            Some(_) => Err(UnknownMode),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Shadow => "shadow",
            Self::On => "on",
        }
    }
}

/// The nightly sleep's model: the tiered store's bounded input in, its
/// proposal JSON out ([`lamp_memory_tiers::SleepProposal`]). The product
/// runs none (lamp-memory has no network), so each night is counted
/// `unconfigured`; tests and the bench give one.
pub trait Sleeper: Send {
    fn propose(&mut self, scope: &str, input: &SleepInput) -> Result<String, String>;
}

pub struct Options {
    /// The Days directory; the tiers live in its [`DIR`].
    pub dir: PathBuf,
    pub mode: Mode,
    /// For the episodes' days and the night's hours: the same as Days'.
    pub local: LocalTime,
    /// Unix ms, for the night's schedule.
    pub clock: Box<dyn Fn() -> u64 + Send>,
    pub sleeper: Option<Box<dyn Sleeper>>,
    pub queue: usize,
    pub log: Arc<dyn Fn(&str) + Send + Sync>,
}

impl Options {
    pub fn new(dir: PathBuf, mode: Mode, local: LocalTime) -> Self {
        Self {
            dir,
            mode,
            local,
            clock: Box::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64)
            }),
            sleeper: None,
            queue: QUEUE,
            log: Arc::new(|line| eprintln!("lamp-memory: {line}")),
        }
    }
}

/// What the tiers did, for the played test's journal and for tests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tally {
    /// Tiered records written from Days entries.
    pub mirrored: u64,
    /// Entries not mirrored: the room's, or no longer active.
    pub skipped: u64,
    /// Entries the tiered store refused as forgotten before (fingerprints).
    pub blocked: u64,
    /// Tiered records removed after a removal in Days, or at start.
    pub removed: u64,
    /// Scopes' tiered stores deleted (a forget of everything, or a failed
    /// forget's last resort).
    pub resets: u64,
    /// Tiered store failures. None of them changes a Days answer.
    pub failures: u64,
    /// Changes not queued; each empties the tiers before anything newer.
    pub dropped: u64,
    /// Times the tiers were emptied after a dropped change.
    pub wipes: u64,
    pub nights: u64,
    pub nights_unconfigured: u64,
    pub nights_applied: u64,
    pub nights_failed: u64,
    /// `on`: tiers answers given beside Days', and those left out as late.
    pub recalls: u64,
    pub recalls_late: u64,
}

/// What one scope's tiered store holds, for tests and diagnostics.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub records: Vec<String>,
    pub facts: Vec<String>,
    /// The facts' quotes, copied from their records.
    pub quotes: Vec<String>,
    pub lines: Vec<String>,
    /// Each episode's topic and gist.
    pub episodes: Vec<String>,
    /// The Days ids the records were written from.
    pub days_ids: Vec<u64>,
}

impl View {
    /// Whether any text held (records, facts with their quotes, lines,
    /// episodes) contains `words`, ignoring case.
    pub fn holds(&self, words: &str) -> bool {
        let words = words.to_lowercase();
        self.records
            .iter()
            .chain(&self.facts)
            .chain(&self.quotes)
            .chain(&self.lines)
            .chain(&self.episodes)
            .any(|text| text.to_lowercase().contains(&words))
    }
}

/// Days, with the tiered store beside it.
pub struct Tiered {
    days: Days,
    mode: Mode,
    link: Option<Link>,
    /// The tiers are off but the change log could not be stopped: each
    /// request's changes are taken and dropped, so nothing grows.
    discard: bool,
    tally: Arc<Mutex<Tally>>,
}

struct Link {
    jobs: Option<SyncSender<Queued>>,
    epoch: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
    /// The tiers' directory, emptied from the request side if the worker
    /// stops (a panic): no later forget could reach it.
    root: PathBuf,
    emptied: AtomicBool,
    log: Arc<dyn Fn(&str) + Send + Sync>,
}

struct Queued {
    epoch: u64,
    job: Job,
}

enum Job {
    Reconcile(Vec<(String, u64)>),
    Changes {
        now: u64,
        changes: Vec<Change>,
        resets: Vec<Forgot>,
        ended: Option<String>,
    },
    Recall {
        scope: String,
        words: String,
        now: u64,
        reply: SyncSender<Option<Value>>,
    },
    Settle(SyncSender<()>),
    View {
        scope: String,
        reply: SyncSender<Option<View>>,
    },
}

impl Job {
    /// Writes and removals: dropped with the tiers emptied after an
    /// overflow. Reads are answered whatever the epoch.
    fn writes(&self) -> bool {
        matches!(self, Self::Reconcile(_) | Self::Changes { .. })
    }
}

/// The scopes mirrored: everyone's but the room's and the room's rules.
pub fn mirrored(scope: &str) -> bool {
    Person::new(scope).is_some() && scope != Person::EVERYONE && scope != "r-everyone"
}

fn add(tally: &Mutex<Tally>, f: impl FnOnce(&mut Tally)) {
    if let Ok(mut tally) = tally.lock() {
        f(&mut tally);
    }
}

impl Tiered {
    /// Days with the tiers beside it. A tiered store that cannot start is
    /// said once, and Days serves as it would alone.
    pub fn start(mut days: Days, options: Options) -> Self {
        let tally = Arc::new(Mutex::new(Tally::default()));
        let log = options.log.clone();
        let mode = options.mode;
        let known = days.enable_tiers_log().and_then(|()| days.tiers_known());
        let link = match known {
            Ok(known) => {
                let root = options.dir.join(DIR);
                let (jobs, queue) = mpsc::sync_channel(options.queue.max(1));
                let epoch = Arc::new(AtomicU64::new(0));
                let worker = Worker::new(options, tally.clone(), epoch.clone());
                let started = thread::Builder::new()
                    .name("memory-tiers".into())
                    .spawn(move || worker.run(queue));
                match started {
                    Ok(handle) => {
                        // The first job, before any request: what Days no
                        // longer holds goes from the tiers.
                        let _ = jobs.send(Queued {
                            epoch: 0,
                            job: Job::Reconcile(known),
                        });
                        Some(Link {
                            jobs: Some(jobs),
                            epoch,
                            worker: Some(handle),
                            root,
                            emptied: AtomicBool::new(false),
                            log: log.clone(),
                        })
                    }
                    Err(error) => {
                        log(&format!("tiers off: the worker could not start: {error}"));
                        None
                    }
                }
            }
            Err(error) => {
                log(&format!(
                    "tiers off: the change log could not start: {error}"
                ));
                None
            }
        };
        // Off, though the change log may have started (the known entries or
        // the worker failed after it): its triggers go, or its table would
        // hold heard and said text for the process's life, never taken.
        let mut discard = false;
        if link.is_none()
            && let Err(error) = days.disable_tiers_log()
        {
            log(&format!(
                "tiers off: the change log could not stop: {error}"
            ));
            discard = true;
        }
        Self {
            days,
            mode,
            link,
            discard,
            tally,
        }
    }

    pub fn days(&self) -> &Days {
        &self.days
    }

    pub fn tally(&self) -> Tally {
        self.tally.lock().map(|t| *t).unwrap_or_default()
    }

    /// Wait until every job queued so far is done (tests, diagnostics).
    pub fn settle(&self) -> bool {
        let (reply, done) = mpsc::sync_channel(1);
        self.ask(Job::Settle(reply)) && done.recv_timeout(SETTLE).is_ok()
    }

    /// What one scope's tiered store holds, after the jobs queued so far.
    pub fn view(&self, scope: &str) -> Option<View> {
        let (reply, done) = mpsc::sync_channel(1);
        let job = Job::View {
            scope: scope.into(),
            reply,
        };
        if !self.ask(job) {
            return None;
        }
        done.recv_timeout(SETTLE).ok().flatten()
    }

    /// A read job, waited for: blocking send, since tests and diagnostics
    /// may wait behind a full queue. Never on a request's path.
    fn ask(&self, job: Job) -> bool {
        let Some(link) = &self.link else { return false };
        let Some(jobs) = &link.jobs else { return false };
        let sent = jobs.send(Queued {
            epoch: link.epoch.load(Ordering::SeqCst),
            job,
        });
        if sent.is_err() {
            self.stopped(link);
        }
        sent.is_ok()
    }

    /// The worker is gone (it panicked): nothing queued reaches the tiers
    /// any more, a forget included, so they are emptied from here, once its
    /// thread has finished, as an overflow empties them from the worker.
    fn stopped(&self, link: &Link) {
        if link.emptied.load(Ordering::SeqCst) {
            return;
        }
        if let Some(worker) = &link.worker {
            let deadline = Instant::now() + STOPPING;
            while !worker.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(2));
            }
            if !worker.is_finished() {
                return;
            }
        }
        if empty_tiers(&link.root, &*link.log) {
            link.emptied.store(true, Ordering::SeqCst);
            add(&self.tally, |t| t.wipes += 1);
            (link.log)("tiers emptied: the worker stopped");
        } else {
            add(&self.tally, |t| t.failures += 1);
        }
    }

    /// Queue a job without waiting. A write that does not fit moves the
    /// epoch on: the worker empties the tiers before anything newer.
    fn offer(&self, job: Job) -> bool {
        let Some(link) = &self.link else { return false };
        let Some(jobs) = &link.jobs else { return false };
        let writes = job.writes();
        let epoch = link.epoch.load(Ordering::SeqCst);
        match jobs.try_send(Queued { epoch, job }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                if writes {
                    link.epoch.fetch_add(1, Ordering::SeqCst);
                    add(&self.tally, |t| t.dropped += 1);
                } else {
                    add(&self.tally, |t| t.recalls_late += 1);
                }
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                add(&self.tally, |t| t.failures += 1);
                self.stopped(link);
                false
            }
        }
    }

    /// `on`: the tiers' recall for a mirrored scope's words, if ready
    /// within [`ANSWER_WAIT`].
    fn recall(&self, scope: &str, words: String, now: u64) -> Option<Value> {
        let (reply, answer) = mpsc::sync_channel(1);
        let job = Job::Recall {
            scope: scope.into(),
            words,
            now,
            reply,
        };
        if !self.offer(job) {
            return None;
        }
        match answer.recv_timeout(ANSWER_WAIT) {
            Ok(found) => {
                if found.is_some() {
                    add(&self.tally, |t| t.recalls += 1);
                }
                found
            }
            Err(_) => {
                add(&self.tally, |t| t.recalls_late += 1);
                None
            }
        }
    }
}

impl Drop for Tiered {
    fn drop(&mut self) {
        if let Some(link) = &mut self.link {
            drop(link.jobs.take());
            if let Some(worker) = link.worker.take()
                && worker.join().is_err()
                && !link.emptied.load(Ordering::SeqCst)
                && empty_tiers(&link.root, &*link.log)
            {
                // The worker stopped with no request after it to see it.
                (link.log)("tiers emptied: the worker stopped");
            }
        }
    }
}

/// Every file in the tiers' directory removed (the worker is gone). True
/// when none is left; a failure is logged without a path.
fn empty_tiers(root: &Path, log: &(dyn Fn(&str) + Send + Sync)) -> bool {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return true,
        Err(error) => {
            log(&format!("tiers empty failed: {error}"));
            return false;
        }
    };
    let mut left = false;
    for entry in entries.flatten() {
        match fs::remove_file(entry.path()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                log(&format!("tiers empty failed: {error}"));
                left = true;
            }
        }
    }
    !left
}

/// A start with the switch off (or unknown): a tiered copy an earlier
/// `shadow` or `on` left under `dir` is removed, since no forget made while
/// off would reach it. The log line has counts only. `None` when there was
/// none.
pub fn clear_left(dir: &Path) -> Option<String> {
    let root = dir.join(DIR);
    if fs::symlink_metadata(&root).is_err() {
        return None;
    }
    let (mut stores, mut files) = (0usize, 0usize);
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            files += 1;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name != MIRROR && name.ends_with(".sqlite3") {
                stores += 1;
            }
        }
    }
    let removed = match fs::symlink_metadata(&root) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(&root),
        Ok(_) => fs::remove_file(&root),
        Err(error) => Err(error),
    };
    Some(match removed {
        Ok(()) => format!("tiers off: a left tiered copy removed stores={stores} files={files}"),
        Err(_) => format!(
            "tiers off: a left tiered copy could not be removed stores={stores} files={files}"
        ),
    })
}

/// The op, the scope and whether it was done, from the answer's log line
/// (`{op} person={who} {outcome}`, [`crate::days_protocol::answer`]):
/// nothing of the request is parsed again on its path.
fn asked(log: &str) -> (&str, &str, bool) {
    let mut words = log.split(' ');
    let op = words.next().unwrap_or("");
    let who = words
        .next()
        .and_then(|w| w.strip_prefix("person="))
        .unwrap_or("");
    let done = words.next() == Some("ok");
    (op, who, done)
}

impl Memory for Tiered {
    fn answer(&mut self, line: &[u8], now_unix_ms: u64) -> protocol::Answer {
        let mut answer = crate::days_protocol::answer(&mut self.days, line, now_unix_ms);
        if self.link.is_none() {
            if self.discard {
                let _ = self.days.take_tiers_log();
            }
            return answer;
        }
        let changes = match self.days.take_tiers_log() {
            Ok(changes) => changes,
            Err(_) => {
                // What was removed can't be known: empty the tiers.
                if let Some(link) = &self.link {
                    link.epoch.fetch_add(1, Ordering::SeqCst);
                }
                add(&self.tally, |t| {
                    t.failures += 1;
                    t.dropped += 1;
                });
                Vec::new()
            }
        };
        let (op, who, done) = asked(&answer.log);
        let mut resets: Vec<Forgot> = answer.forgot.iter().cloned().collect();
        // A `forget_person` also emptied the person's rules scope
        // (`Days::forget_person`), which the answer doesn't tell apart: its
        // tiered store, key and fingerprints go too. A `forget_all` of one
        // person leaves their rules, in Days and here.
        if done
            && op == "forget_person"
            && let Some(Forgot::One(scope)) = &answer.forgot
            && let Some(rules) = Person::new(scope).and_then(|p| p.rules())
        {
            resets.push(Forgot::One(rules.as_str().to_owned()));
        }
        let ended =
            (done && matches!(op, "session_close" | "conversation_end")).then(|| who.to_owned());
        if !changes.is_empty() || !resets.is_empty() || ended.is_some() {
            self.offer(Job::Changes {
                now: now_unix_ms,
                changes,
                resets,
                ended,
            });
        }
        if self.mode == Mode::On
            && done
            && matches!(op, "recall" | "search")
            && mirrored(who)
            && answer.line.ends_with('}')
            && let Some(words) = serde_json::from_slice::<Value>(line)
                .ok()
                .and_then(|v| v.get("query").and_then(Value::as_str).map(str::to_owned))
                .filter(|q| !q.trim().is_empty())
            && let Some(found) = self.recall(who, words, now_unix_ms)
        {
            // Spliced in, so Days' own bytes are as they were.
            answer.line.pop();
            answer.line.push_str(&format!(",\"tiers\":{found}}}"));
        }
        answer
    }

    fn health_counts(&self) -> Option<io::Result<crate::health_counts::Counts>> {
        Some(self.days.health_counts().map_err(io::Error::other))
    }

    fn set_aside(&self) -> Option<&str> {
        self.days.set_aside()
    }
}

struct Worker {
    root: PathBuf,
    mirror: Option<Connection>,
    stores: HashMap<String, Store>,
    local: LocalTime,
    clock: Box<dyn Fn() -> u64 + Send>,
    sleeper: Option<Box<dyn Sleeper>>,
    tally: Arc<Mutex<Tally>>,
    log: Arc<dyn Fn(&str) + Send + Sync>,
    epoch: Arc<AtomicU64>,
    wiped: u64,
}

fn open_mirror(path: &Path) -> rusqlite::Result<Connection> {
    // Owner-only before SQLite opens it; its journal takes the same mode.
    let _ = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path);
    let db = Connection::open(path)?;
    db.execute_batch(
        "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA secure_delete=ON;
        CREATE TABLE IF NOT EXISTS map(scope TEXT NOT NULL, days_id INTEGER NOT NULL, record_id INTEGER NOT NULL, PRIMARY KEY(scope,record_id));
        CREATE INDEX IF NOT EXISTS map_days ON map(scope,days_id);
        CREATE TABLE IF NOT EXISTS nights(scope TEXT PRIMARY KEY, day INTEGER NOT NULL);",
    )?;
    Ok(db)
}

impl Worker {
    fn new(options: Options, tally: Arc<Mutex<Tally>>, epoch: Arc<AtomicU64>) -> Self {
        let root = options.dir.join(DIR);
        let mut worker = Self {
            root,
            mirror: None,
            stores: HashMap::new(),
            local: options.local,
            clock: options.clock,
            sleeper: options.sleeper,
            tally,
            log: options.log,
            epoch,
            wiped: 0,
        };
        worker.open();
        worker
    }

    /// The directory and the map. Without them nothing is written; each
    /// write is counted a failure.
    fn open(&mut self) {
        let opened = private_dir(&self.root)
            .map_err(|e| e.to_string())
            .and_then(|()| open_mirror(&self.root.join(MIRROR)).map_err(|e| e.to_string()));
        match opened {
            Ok(db) => self.mirror = Some(db),
            Err(error) => {
                self.fail("start", &error);
                // Without the map no forget could find what a store holds:
                // stores an earlier process left go now, unread.
                for scope in self.scopes_on_disk() {
                    self.reset(&scope);
                }
            }
        }
    }

    fn fail(&self, what: &str, error: &dyn std::fmt::Display) {
        add(&self.tally, |t| t.failures += 1);
        (self.log)(&format!("tiers {what} failed: {error}"));
    }

    fn run(mut self, queue: mpsc::Receiver<Queued>) {
        loop {
            match queue.recv_timeout(IDLE) {
                Ok(queued) => {
                    self.check_epoch();
                    if queued.job.writes() && queued.epoch < self.wiped {
                        continue;
                    }
                    self.handle(queued.job);
                    self.night();
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.check_epoch();
                    self.night();
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        let tally = self.tally.lock().map(|t| *t).unwrap_or_default();
        (self.log)(&format!(
            "tiers stopped mirrored={} removed={} resets={} failures={} dropped={}",
            tally.mirrored, tally.removed, tally.resets, tally.failures, tally.dropped
        ));
    }

    /// A change was not queued: whatever it was, everything goes, before
    /// anything queued after it is written.
    fn check_epoch(&mut self) {
        let epoch = self.epoch.load(Ordering::SeqCst);
        if epoch > self.wiped {
            self.wipe();
            self.wiped = epoch;
            add(&self.tally, |t| t.wipes += 1);
            (self.log)("tiers emptied: a change could not be queued");
        }
    }

    fn wipe(&mut self) {
        self.stores.clear();
        self.mirror = None;
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                if let Err(error) = fs::remove_file(entry.path()) {
                    self.fail("empty", &error);
                }
            }
        }
        self.open();
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::Reconcile(known) => self.reconcile(known),
            Job::Changes {
                now,
                changes,
                resets,
                ended,
            } => {
                let mut gone = HashSet::new();
                for reset in resets {
                    match reset {
                        Forgot::Every => {
                            for scope in self.scopes_on_disk() {
                                self.reset(&scope);
                                gone.insert(scope);
                            }
                        }
                        Forgot::One(scope) => {
                            self.reset(&scope);
                            gone.insert(scope);
                        }
                    }
                }
                for change in changes {
                    match change {
                        Change::Added { person, .. } | Change::Removed { person, .. }
                            if gone.contains(&person) => {}
                        Change::Added { person, id, entry } => self.added(&person, id, &entry),
                        Change::Removed { person, id } => self.removed(&person, id),
                        Change::Moved {
                            from,
                            from_id,
                            to,
                            to_id,
                            entry,
                        } => {
                            if !gone.contains(&from) {
                                self.removed(&from, from_id);
                            }
                            if !gone.contains(&to) {
                                self.added(&to, to_id, &entry);
                            }
                        }
                    }
                }
                if let Some(scope) = ended
                    && mirrored(&scope)
                    && self.exists(&scope)
                    && let Some(store) = self.store(&scope)
                    && let Err(error) = store.owner_left(now as i64)
                {
                    self.fail("episode end", &error);
                }
            }
            Job::Recall {
                scope,
                words,
                now,
                reply,
            } => {
                let found = self.recall(&scope, words, now);
                let _ = reply.try_send(found);
            }
            Job::Settle(reply) => {
                let _ = reply.try_send(());
            }
            Job::View { scope, reply } => {
                let view = self.view(&scope);
                let _ = reply.try_send(view);
            }
        }
    }

    fn path(&self, scope: &str) -> PathBuf {
        self.root.join(format!("{scope}.sqlite3"))
    }

    fn exists(&self, scope: &str) -> bool {
        self.path(scope).exists()
    }

    fn store(&mut self, scope: &str) -> Option<&mut Store> {
        if !self.stores.contains_key(scope) {
            if self.mirror.is_none() || !mirrored(scope) {
                return None;
            }
            if self.stores.len() >= OPEN_STORES
                && let Some(other) = self.stores.keys().next().cloned()
            {
                self.stores.remove(&other);
            }
            let fresh = !self.exists(scope);
            match Store::open(self.path(scope), scope) {
                Ok(mut store) => {
                    if fresh && let Some(zone) = self.local.name() {
                        // A zone the store can't name stays UTC: only its
                        // own budgets' days depend on it.
                        let _ = store.set_time_zone(&zone);
                    }
                    self.stores.insert(scope.into(), store);
                }
                Err(error) => {
                    self.fail("open", &error);
                    return None;
                }
            }
        }
        self.stores.get_mut(scope)
    }

    fn map_insert(&self, scope: &str, days_id: u64, record_id: i64) -> bool {
        let Some(db) = &self.mirror else { return false };
        db.execute(
            "INSERT OR REPLACE INTO map(scope,days_id,record_id) VALUES(?1,?2,?3)",
            params![scope, days_id, record_id],
        )
        .is_ok()
    }

    /// The records one Days entry was mirrored to. Without a map nothing
    /// was written, so none; a map that can't be read is an error.
    fn mapped(&self, scope: &str, days_id: u64) -> rusqlite::Result<Vec<i64>> {
        let Some(db) = &self.mirror else {
            return Ok(Vec::new());
        };
        db.prepare_cached("SELECT record_id FROM map WHERE scope=?1 AND days_id=?2")
            .and_then(|mut s| {
                s.query_map(params![scope, days_id], |r| r.get(0))?
                    .collect::<rusqlite::Result<Vec<i64>>>()
            })
    }

    fn map_of(&self, scope: &str) -> Vec<(u64, i64)> {
        let Some(db) = &self.mirror else {
            return Vec::new();
        };
        db.prepare_cached("SELECT days_id,record_id FROM map WHERE scope=?1")
            .and_then(|mut s| {
                s.query_map([scope], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap_or_default()
    }

    fn added(&mut self, scope: &str, id: u64, entry: &Held) {
        if !mirrored(scope) || !entry.active {
            add(&self.tally, |t| t.skipped += 1);
            return;
        }
        let at = entry.at_unix_ms as i64;
        let topic = format!("day {}", self.local.day(entry.at_unix_ms));
        let lines: Vec<(&str, Speaker, bool)> = match entry.kind.as_str() {
            "exchange" => vec![
                (entry.heard.as_str(), Speaker::Owner, false),
                (entry.said.as_str(), Speaker::Assistant, false),
            ],
            "kept" => vec![(entry.text.as_str(), Speaker::Owner, true)],
            _ => Vec::new(),
        };
        if self.store(scope).is_none() {
            return;
        }
        for (text, speaker, remember) in lines {
            if text.trim().is_empty() {
                continue;
            }
            let incoming = Incoming {
                text: text.into(),
                speaker,
                at_ms: at,
                stt_confidence: None,
                strength: Strength::Said,
                addressed_to_lamp: true,
                topic: topic.clone(),
                remember,
            };
            let Some(store) = self.stores.get_mut(scope) else {
                return;
            };
            match store.ingest(incoming) {
                Ok(Some(record)) => {
                    // Mapped after the record is written: a crash between
                    // leaves a record no map names, removed at the next start.
                    if !self.map_insert(scope, id, record) {
                        // A record no map names could not be forgotten.
                        self.fail("map", &"the map could not be written");
                        self.forget(scope, &[record]);
                        continue;
                    }
                    add(&self.tally, |t| t.mirrored += 1);
                    if remember
                        && let Some(store) = self.stores.get_mut(scope)
                        && let Err(error) = store.remember(record, FactKind::Statement, None)
                    {
                        self.fail("keep", &error);
                    }
                }
                Ok(None) => add(&self.tally, |t| t.blocked += 1),
                Err(error) => self.fail("write", &error),
            }
        }
    }

    fn removed(&mut self, scope: &str, id: u64) {
        match self.mapped(scope, id) {
            Ok(records) if records.is_empty() => {}
            Ok(records) => self.forget(scope, &records),
            // A forget that can't find its records deletes the scope's
            // whole store, as one that fails does: nothing forgotten stays.
            Err(error) => {
                self.fail("map read", &error);
                self.reset(scope);
            }
        }
    }

    /// The library's own forget: the records, every fact quoting them, every
    /// line citing those, every episode's derived text. Anything that fails
    /// deletes the scope's whole store: nothing forgotten stays.
    fn forget(&mut self, scope: &str, records: &[i64]) {
        let Some(store) = self.store(scope) else {
            self.reset(scope);
            return;
        };
        let result = (|| -> lamp_memory_tiers::Result<(usize, BTreeSet<i64>)> {
            let present: BTreeSet<i64> = store
                .records(Access::OWNER_ALONE)?
                .iter()
                .map(|r| r.id)
                .collect();
            let doomed: Vec<i64> = records
                .iter()
                .copied()
                .filter(|r| present.contains(r))
                .collect();
            let count = store.forget_records(&doomed)?;
            let left = store
                .records(Access::OWNER_ALONE)?
                .iter()
                .map(|r| r.id)
                .collect();
            Ok((count, left))
        })();
        match result {
            Ok((count, left)) => {
                add(&self.tally, |t| t.removed += count as u64);
                // The library may take more than was named (a record whose
                // words a forgotten one shares): its map rows go too.
                if let Some(db) = &self.mirror {
                    for (_, record) in self.map_of(scope) {
                        if !left.contains(&record) {
                            let _ = db.execute(
                                "DELETE FROM map WHERE scope=?1 AND record_id=?2",
                                params![scope, record],
                            );
                        }
                    }
                }
            }
            Err(error) => {
                self.fail("forget", &error);
                self.reset(scope);
            }
        }
    }

    /// The scope's store, its fingerprint key and its map rows, gone.
    fn reset(&mut self, scope: &str) {
        self.stores.remove(scope);
        if Person::new(scope).is_none() {
            return;
        }
        let path = self.path(scope);
        let mut removed = false;
        for suffix in ["", "-journal", ".forget-key", ".lock"] {
            let mut file = path.as_os_str().to_os_string();
            file.push(suffix);
            match fs::remove_file(&file) {
                Ok(()) => removed = true,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => self.fail("reset", &error),
            }
        }
        if let Some(db) = &self.mirror {
            let _ = db.execute("DELETE FROM map WHERE scope=?1", [scope]);
            let _ = db.execute("DELETE FROM nights WHERE scope=?1", [scope]);
        }
        if removed {
            add(&self.tally, |t| t.resets += 1);
        }
    }

    fn scopes_on_disk(&self) -> Vec<String> {
        let mut scopes = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if name == MIRROR {
                    continue;
                }
                if let Some(scope) = name.strip_suffix(".sqlite3")
                    && Person::new(scope).is_some()
                {
                    scopes.push(scope.to_owned());
                }
            }
        }
        scopes.sort();
        scopes
    }

    /// At start: whatever the tiers hold that Days doesn't (a forget the
    /// last process didn't live to apply, a set-aside database) or that no
    /// map row names (a crash between a record and its row) goes.
    fn reconcile(&mut self, known: Vec<(String, u64)>) {
        let mut held: HashMap<String, HashSet<u64>> = HashMap::new();
        for (scope, id) in known {
            held.entry(scope).or_default().insert(id);
        }
        let scopes = self.scopes_on_disk();
        for scope in &scopes {
            let Some(ids) = held.get(scope).filter(|_| mirrored(scope)) else {
                self.reset(scope);
                continue;
            };
            let map = self.map_of(scope);
            let named: HashSet<i64> = map.iter().map(|(_, r)| *r).collect();
            let mut doomed: BTreeSet<i64> = map
                .iter()
                .filter(|(days, _)| !ids.contains(days))
                .map(|(_, r)| *r)
                .collect();
            match self.store(scope) {
                Some(store) => match store.records(Access::OWNER_ALONE) {
                    Ok(records) => {
                        doomed.extend(records.iter().map(|r| r.id).filter(|r| !named.contains(r)));
                    }
                    Err(error) => {
                        self.fail("start", &error);
                        self.reset(scope);
                        continue;
                    }
                },
                None => {
                    self.reset(scope);
                    continue;
                }
            }
            if !doomed.is_empty() {
                self.forget(scope, &doomed.into_iter().collect::<Vec<_>>());
            }
        }
        // Map rows of a store no longer on disk.
        if let Some(db) = &self.mirror {
            let listed: Vec<String> = db
                .prepare("SELECT DISTINCT scope FROM map")
                .and_then(|mut s| {
                    s.query_map([], |r| r.get(0))?
                        .collect::<rusqlite::Result<Vec<String>>>()
                })
                .unwrap_or_default();
            for scope in listed {
                if !scopes.contains(&scope) {
                    let _ = db.execute("DELETE FROM map WHERE scope=?1", [&scope]);
                }
            }
        }
    }

    fn recall(&mut self, scope: &str, words: String, now: u64) -> Option<Value> {
        if !mirrored(scope) || !self.exists(scope) {
            return None;
        }
        let query = RecallQuery {
            access: Access::OWNER_ALONE,
            now_ms: now as i64,
            words,
            cues: Cues::default(),
            max_bytes: RECALL_BYTES,
            budget: RECALL_BUDGET,
            arm: Arm::Tiers,
        };
        let result = match self.store(scope)?.recall(&query) {
            Ok(result) => result,
            Err(error) => {
                self.fail("recall", &error);
                return None;
            }
        };
        let map: HashMap<i64, u64> = self
            .map_of(scope)
            .into_iter()
            .map(|(d, r)| (r, d))
            .collect();
        let items: Vec<Value> = result
            .items
            .iter()
            .map(|item| {
                let ids: BTreeSet<u64> = item
                    .source_ids
                    .iter()
                    .filter_map(|r| map.get(r).copied())
                    .collect();
                json!({"kind":item.kind,"text":item.text,"strength":item.strength,"ids":ids})
            })
            .collect();
        Some(json!({"items":items,"timed_out":result.timed_out}))
    }

    fn view(&mut self, scope: &str) -> Option<View> {
        if !self.exists(scope) {
            return None;
        }
        let days_ids = {
            let mut ids: Vec<u64> = self.map_of(scope).into_iter().map(|(d, _)| d).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let store = self.store(scope)?;
        let all = Access::OWNER_ALONE;
        let facts = store.facts(all).ok()?;
        Some(View {
            records: store
                .records(all)
                .ok()?
                .into_iter()
                .map(|r| r.text)
                .collect(),
            facts: facts.iter().map(|f| f.claim.clone()).collect(),
            quotes: facts
                .into_iter()
                .flat_map(|f| f.quotes.into_iter().map(|q| q.text))
                .collect(),
            lines: store.lines(all).ok()?.into_iter().map(|l| l.text).collect(),
            episodes: store
                .episodes(all)
                .ok()?
                .into_iter()
                .map(|e| format!("{} {} {}", e.topic, e.gist, e.detail))
                .collect(),
            days_ids,
        })
    }

    /// Once per local day, in the night's hours, each mirrored scope
    /// sleeps: its open episode ends, expiry and fading run, and with a
    /// model its bounded input is proposed on and applied. Without one the
    /// night is counted `unconfigured`, never a successful night.
    fn night(&mut self) {
        let now = (self.clock)();
        let hour = self.local.hour_of_day(now);
        if !(NIGHT_FROM_HOUR..NIGHT_UNTIL_HOUR).contains(&hour) || self.mirror.is_none() {
            return;
        }
        let day = self.local.day(now);
        for scope in self.scopes_on_disk() {
            let Some(db) = &self.mirror else { return };
            let last: i64 = db
                .query_row("SELECT day FROM nights WHERE scope=?1", [&scope], |r| {
                    r.get(0)
                })
                .unwrap_or(i64::MIN);
            if last >= day {
                continue;
            }
            // Marked first, so a night that fails is not retried in a loop.
            if db
                .execute(
                    "INSERT OR REPLACE INTO nights(scope,day) VALUES(?1,?2)",
                    params![scope, day],
                )
                .is_err()
            {
                continue;
            }
            self.sleep(&scope, now);
        }
    }

    fn sleep(&mut self, scope: &str, now: u64) {
        add(&self.tally, |t| t.nights += 1);
        let now = now as i64;
        let mut sleeper = self.sleeper.take();
        let mut outcome = "nothing";
        if let Some(store) = self.store(scope) {
            let prepared = store
                .owner_left(now)
                .and_then(|()| store.maintain(now, Arm::Tiers).map(|_| ()));
            if let Err(error) = prepared {
                outcome = "failed";
                let line = format!("tiers night failed: {error}");
                add(&self.tally, |t| t.failures += 1);
                (self.log)(&line);
            } else {
                for _ in 0..NIGHT_PASSES {
                    let input = match store.prepare_sleep(now, PassKind::Nightly) {
                        Ok(Some(input)) => input,
                        Ok(None) => break,
                        Err(_) => {
                            outcome = "failed";
                            break;
                        }
                    };
                    let Some(model) = sleeper.as_mut() else {
                        outcome = "unconfigured";
                        break;
                    };
                    let applied = model
                        .propose(scope, &input)
                        .ok()
                        .and_then(|json| store.apply_sleep(&input, &json).ok());
                    match applied {
                        Some(report) => {
                            outcome = "applied";
                            if !report.pending {
                                break;
                            }
                        }
                        None => {
                            outcome = "failed";
                            break;
                        }
                    }
                }
            }
        }
        self.sleeper = sleeper;
        add(&self.tally, |t| match outcome {
            "applied" => t.nights_applied += 1,
            "unconfigured" => t.nights_unconfigured += 1,
            "failed" => t.nights_failed += 1,
            _ => {}
        });
        let counts = self.view(scope).map_or((0, 0, 0, 0), |v| {
            (
                v.records.len(),
                v.facts.len(),
                v.lines.len(),
                v.episodes.len(),
            )
        });
        (self.log)(&format!(
            "tiers night person={scope} model={outcome} records={} facts={} lines={} episodes={}",
            counts.0, counts.1, counts.2, counts.3
        ));
    }
}
