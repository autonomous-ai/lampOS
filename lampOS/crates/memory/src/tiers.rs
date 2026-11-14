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
//!
//! The night's model is the voice's (the memory owner's choice, 3 Nov):
//! lamp-memory stays offline, so with no [`Sleeper`] each scope's night is
//! a job in a bounded queue ([`NIGHT_JOBS`]) that the voice takes on the
//! main socket (`tiers_sleep_take`) while the lamp is quiet at night, asks
//! its relay's model, and gives back (`tiers_sleep_give`). The reply is
//! checked by the tiered store's own validator; a job a forget touched
//! since it was taken, or that morning reached, is never applied.
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
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fs, io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
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
/// The night's jobs waiting for the voice, all scopes together. A night
/// that finds the queue full is not queued: its job is counted expired and
/// its night failed (`nights_failed`), and its records wait for the next
/// night. A later pass that finds it full is counted expired only.
pub const NIGHT_JOBS: usize = 16;
/// A job taken and not given back this long is dropped (counted expired):
/// the voice let it go (someone spoke, the privacy slide) or stopped. A job
/// is given out once, never again: nothing is retried in a loop, and its
/// records wait for the next night. Above the voice's own call timeout.
pub const JOB_LEASE: Duration = Duration::from_secs(120);
/// How long a `tiers_sleep_take` or `tiers_sleep_give` waits for the
/// tiers' thread before answering `busy`.
///
/// The memory server answers one connection at a time, so this wait holds
/// the socket for every other caller: a turn's first read
/// (`remember::FIRST_READ`, 300 ms) or a spoken forget at 04:00 waits
/// behind it. 100 ms keeps a night op well under that, so the night stays
/// off the turn's path, socket included. (Answering night ops off the
/// server's thread would need the server's one-at-a-time loop changed for
/// every op; a short bound is the smaller, safer change.) The worker can be
/// slower than that (another scope's `apply_sleep` and `prepare_sleep`
/// with full fsyncs), and nothing is lost when it is:
///
/// - a take it answers after the wait gave up puts its job back
///   ([`Handoff`]), so the next take gets it;
/// - a give it reaches after the wait is still applied, once, and its
///   outcome kept ([`GIVEN_KEPT`]): the voice gives it again later and is
///   told that outcome, never applied twice. A give that could not even be
///   queued is applied by that retry.
pub const NIGHT_OP_WAIT: Duration = Duration::from_millis(100);
/// The outcomes of the latest gives, by job id, answered to a give of the
/// same job again: the most a night can give out, every scope's passes.
pub const GIVEN_KEPT: usize = NIGHT_JOBS * NIGHT_PASSES;
/// The two socket ops the voice does the night with; known only with the
/// switch set (Days alone refuses them as any unknown op).
pub const TAKE_OP: &str = "tiers_sleep_take";
pub const GIVE_OP: &str = "tiers_sleep_give";

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

/// The nightly sleep's model in this process: the tiered store's bounded
/// input in, its proposal JSON out ([`lamp_memory_tiers::SleepProposal`]).
/// The product runs none (lamp-memory has no network): without one each
/// night is a job for the voice ([`TAKE_OP`], [`GIVE_OP`]). Tests and the
/// bench give one.
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
    /// Nights whose first job morning reached untaken: no voice did them.
    pub nights_unconfigured: u64,
    pub nights_applied: u64,
    pub nights_failed: u64,
    /// `on`: tiers answers given beside Days', and those left out as late.
    pub recalls: u64,
    pub recalls_late: u64,
    /// The voice's night: jobs queued, taken, and how each ended. A job is
    /// applied, invalid (the reply failed the validator), stale (a forget
    /// or another change reached its scope after it was made) or expired
    /// (morning came, or the queue was full).
    pub jobs_queued: u64,
    pub jobs_taken: u64,
    pub jobs_applied: u64,
    pub jobs_invalid: u64,
    pub jobs_stale: u64,
    pub jobs_expired: u64,
    /// Takes the worker answered after the socket side stopped waiting:
    /// each job put back, untaken (and not counted taken).
    pub jobs_put_back: u64,
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
    /// The voice's `tiers_sleep_take`: the next job, or none. Judged on the
    /// worker's clock, as the night is.
    Take {
        reply: Handoff<Option<Value>>,
    },
    /// The voice's `tiers_sleep_give`: the model's reply for a job.
    Give {
        id: String,
        json: String,
        reply: Handoff<&'static str>,
    },
}

/// A night op's answer, handed from the worker to the socket side that may
/// stop waiting for it. Under one lock either the answer is handed or the
/// socket side has given up, never both: an answer handed back to the
/// worker ([`Handoff::hand`]'s `Err`) is one the voice never saw.
struct Handoff<T>(Arc<(Mutex<Slot<T>>, Condvar)>);

enum Slot<T> {
    Waiting,
    Ready(T),
    Gone,
}

impl<T> Handoff<T> {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(Slot::Waiting), Condvar::new())))
    }

    fn other(&self) -> Self {
        Self(self.0.clone())
    }

    /// The worker's side: `Err` with the answer when the socket side gave
    /// up (or the request was dropped unasked).
    fn hand(&self, answer: T) -> Result<(), T> {
        let (slot, ready) = &*self.0;
        let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(*slot, Slot::Waiting) {
            return Err(answer);
        }
        *slot = Slot::Ready(answer);
        ready.notify_all();
        Ok(())
    }

    /// The socket side: the answer within `wait`, else `None`, after which
    /// the worker's [`Handoff::hand`] fails.
    fn wait(&self, wait: Duration) -> Option<T> {
        let (slot, ready) = &*self.0;
        let slot = slot.lock().unwrap_or_else(|e| e.into_inner());
        let (mut slot, _) = ready
            .wait_timeout_while(slot, wait, |s| matches!(s, Slot::Waiting))
            .unwrap_or_else(|e| e.into_inner());
        match std::mem::replace(&mut *slot, Slot::Gone) {
            Slot::Ready(answer) => Some(answer),
            Slot::Waiting | Slot::Gone => None,
        }
    }
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
            Err(TrySendError::Full(queued)) => {
                if writes {
                    link.epoch.fetch_add(1, Ordering::SeqCst);
                    add(&self.tally, |t| t.dropped += 1);
                } else if matches!(queued.job, Job::Recall { .. }) {
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

/// A night op's answer: its line and its log, nothing of the job's words.
fn night_answer(line: Value, log: String) -> protocol::Answer {
    protocol::Answer {
        line: line.to_string(),
        log,
        storage_fault: false,
        wrote: false,
        forgot: None,
    }
}

impl Tiered {
    /// `tiers_sleep_take` and `tiers_sleep_give`, if `line` is one; `None`
    /// for anything else, which Days answers. Only lines naming the ops are
    /// parsed here.
    fn night_op(&self, line: &[u8]) -> Option<protocol::Answer> {
        const MARK: &[u8] = b"tiers_sleep_";
        if !line.windows(MARK.len()).any(|w| w == MARK) {
            return None;
        }
        let request: Value = serde_json::from_slice(line).ok()?;
        let op = request.get("op").and_then(Value::as_str)?;
        let refused = |message: &str| {
            night_answer(
                json!({"ok":false,"error":"bad_request","message":message}),
                format!("{op} person=- refused"),
            )
        };
        match op {
            TAKE_OP => {
                let answer = Handoff::new();
                let reply = answer.other();
                // A take the worker answers after this gave up is put back.
                let answered = self
                    .offer(Job::Take { reply })
                    .then(|| answer.wait(NIGHT_OP_WAIT))
                    .flatten();
                let (line, outcome) = match answered {
                    Some(Some(job)) => (json!({"ok":true,"job":job}), "job"),
                    Some(None) => (json!({"ok":true,"job":null}), "none"),
                    // Not waited for: the voice looks again later.
                    None => (json!({"ok":true,"job":null,"busy":true}), "busy"),
                };
                Some(night_answer(line, format!("{op} person=- {outcome}")))
            }
            GIVE_OP => {
                let Some(id) = request.get("job").and_then(Value::as_str) else {
                    return Some(refused("tiers_sleep_give takes the job's id"));
                };
                // A reply that is no JSON object (the voice gives `null` for a
                // model answer it could not read) drops the job as invalid.
                let Some(reply) = request.get("reply") else {
                    return Some(refused("tiers_sleep_give takes the model's reply"));
                };
                let answer = Handoff::new();
                let job = Job::Give {
                    id: id.to_owned(),
                    json: reply.to_string(),
                    reply: answer.other(),
                };
                // `busy`: queued, the worker still applies the reply once when
                // it reaches it; not queued, nothing is applied. Either way
                // the voice gives it again and is told the outcome.
                let outcome = if self.offer(job) {
                    answer.wait(NIGHT_OP_WAIT).unwrap_or("busy")
                } else {
                    "busy"
                };
                Some(night_answer(
                    json!({"ok":true,"outcome":outcome}),
                    format!("{op} person=- {outcome}"),
                ))
            }
            _ => None,
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
        if self.link.is_some()
            && let Some(answer) = self.night_op(line)
        {
            return answer;
        }
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
    /// The night's jobs for the voice, oldest first, at most [`NIGHT_JOBS`].
    jobs: VecDeque<NightJob>,
    /// For the jobs' ids: a count, hashed with this process's start.
    job_seq: u64,
    job_salt: [u8; 32],
    /// The latest gives' outcomes by job id ([`GIVEN_KEPT`]).
    given: VecDeque<(String, &'static str)>,
}

/// One scope's night pass, waiting for the voice's model. Its input stays
/// here; the voice is given its words without the scope's name.
struct NightJob {
    id: String,
    scope: String,
    /// The local day it is for; morning, or another day, expires it.
    day: i64,
    input: SleepInput,
    /// The input as the voice is given it: the store's own bounded JSON
    /// less its owner.
    json: Value,
    pass: usize,
    taken: bool,
    lease_until: u64,
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
            jobs: VecDeque::new(),
            job_seq: 0,
            given: VecDeque::new(),
            job_salt: Sha256::digest(format!(
                "{}-{:?}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)
            ))
            .into(),
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
        self.drop_jobs(None);
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
            Job::Take { reply } => {
                let job = self.take((self.clock)());
                if let Err(Some(job)) = reply.hand(job)
                    && let Some(id) = job["id"].as_str()
                {
                    // Nobody got it: given out again on the next take.
                    self.untake(id);
                }
            }
            Job::Give { id, json, reply } => {
                let outcome = self.give((self.clock)(), &id, &json);
                if reply.hand(outcome).is_err() {
                    (self.log)(&format!("tiers night give answered busy: {outcome}"));
                }
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
                // A night's job made before this forget is never applied.
                self.drop_jobs(Some(scope));
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
        self.drop_jobs(Some(scope));
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
    /// sleeps: its open episode ends, expiry and fading run, and its
    /// bounded input is proposed on and applied: by this process's model
    /// when it has one, else as a job the voice takes ([`TAKE_OP`]). A
    /// night whose job morning reaches untaken is counted `unconfigured`,
    /// never a successful night.
    fn night(&mut self) {
        let now = (self.clock)();
        self.expire_jobs(now);
        if !self.in_night(now) || self.mirror.is_none() {
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
        let at = now as i64;
        let mut sleeper = self.sleeper.take();
        let mut outcome = "nothing";
        let mut for_voice = None;
        if let Some(store) = self.store(scope) {
            let prepared = store
                .owner_left(at)
                .and_then(|()| store.maintain(at, Arm::Tiers).map(|_| ()));
            if let Err(error) = prepared {
                outcome = "failed";
                let line = format!("tiers night failed: {error}");
                add(&self.tally, |t| t.failures += 1);
                (self.log)(&line);
            } else {
                for _ in 0..NIGHT_PASSES {
                    let input = match store.prepare_sleep(at, PassKind::Nightly) {
                        Ok(Some(input)) => input,
                        Ok(None) => break,
                        Err(_) => {
                            outcome = "failed";
                            break;
                        }
                    };
                    let Some(model) = sleeper.as_mut() else {
                        // The voice does it: one job a pass, the next
                        // queued when this one is given back.
                        for_voice = Some(input);
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
        if let Some(input) = for_voice {
            outcome = if self.queue_job(scope, input, 0, now) {
                "queued"
            } else {
                "failed"
            };
        }
        add(&self.tally, |t| match outcome {
            "applied" => t.nights_applied += 1,
            "failed" => t.nights_failed += 1,
            _ => {}
        });
        self.said_night(scope, outcome);
    }

    /// `tiers night person=<scope> model=<outcome>` with the store's
    /// counts: no words of it.
    fn said_night(&mut self, scope: &str, outcome: &str) {
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

    fn in_night(&self, now: u64) -> bool {
        (NIGHT_FROM_HOUR..NIGHT_UNTIL_HOUR).contains(&self.local.hour_of_day(now))
    }

    /// A pass's input queued for the voice; false when the queue is full
    /// (counted expired, and for a night's first pass its night failed:
    /// its records wait for the next night) or the input can no longer be
    /// given.
    fn queue_job(&mut self, scope: &str, input: SleepInput, pass: usize, now: u64) -> bool {
        if self.jobs.len() >= NIGHT_JOBS {
            add(&self.tally, |t| t.jobs_expired += 1);
            return false;
        }
        let Some(mut json) = input
            .json()
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        else {
            add(&self.tally, |t| t.jobs_stale += 1);
            return false;
        };
        // The scope's name is not the model's business: the voice is given
        // the words, never whose store they are.
        if let Some(object) = json.as_object_mut() {
            object.remove("owner");
        }
        self.job_seq += 1;
        let mut hash = Sha256::new();
        hash.update(self.job_salt);
        hash.update(self.job_seq.to_le_bytes());
        let id: String = hash.finalize()[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.jobs.push_back(NightJob {
            id,
            scope: scope.to_owned(),
            day: self.local.day(now),
            input,
            json,
            pass,
            taken: false,
            lease_until: 0,
        });
        add(&self.tally, |t| t.jobs_queued += 1);
        true
    }

    /// Jobs morning reached, or of another day: dropped, counted expired; a
    /// night whose first pass was never taken is counted `unconfigured`.
    fn expire_jobs(&mut self, now: u64) {
        let (night, day) = (self.in_night(now), self.local.day(now));
        let before = self.jobs.len();
        let mut untaken = 0;
        self.jobs.retain(|job| {
            let keep = night && job.day == day && !(job.taken && job.lease_until <= now);
            if !keep && !job.taken && job.pass == 0 {
                untaken += 1;
            }
            keep
        });
        let expired = (before - self.jobs.len()) as u64;
        if expired > 0 {
            add(&self.tally, |t| {
                t.jobs_expired += expired;
                t.nights_unconfigured += untaken;
            });
            (self.log)(&format!(
                "tiers night jobs expired={expired} untaken={untaken}"
            ));
        }
    }

    /// A scope's jobs (or every one), dropped as stale: a forget or a reset
    /// reached the store they were made from.
    fn drop_jobs(&mut self, scope: Option<&str>) {
        let before = self.jobs.len();
        self.jobs
            .retain(|job| scope.is_some_and(|scope| job.scope != scope));
        let dropped = (before - self.jobs.len()) as u64;
        if dropped > 0 {
            add(&self.tally, |t| t.jobs_stale += dropped);
        }
    }

    /// The voice's take: the oldest job never given out, its id, the
    /// model's instructions and the input.
    fn take(&mut self, now: u64) -> Option<Value> {
        self.expire_jobs(now);
        let job = self.jobs.iter_mut().find(|job| !job.taken)?;
        job.taken = true;
        job.lease_until = now.saturating_add(JOB_LEASE.as_millis() as u64);
        let given = json!({
            "id": job.id,
            "system": lamp_memory_tiers::model::PROMPT,
            "input": job.json,
        });
        add(&self.tally, |t| t.jobs_taken += 1);
        Some(given)
    }

    /// A take whose answer reached nobody (the socket side stopped waiting):
    /// the job is untaken, the next take's again.
    fn untake(&mut self, id: &str) {
        if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id && job.taken) {
            job.taken = false;
            job.lease_until = 0;
            add(&self.tally, |t| {
                t.jobs_taken = t.jobs_taken.saturating_sub(1);
                t.jobs_put_back += 1;
            });
            (self.log)("tiers night take answered late: the job put back");
        }
    }

    /// The voice's give: the model's reply for job `id`, applied through
    /// the tiered store's own validator, or why not. A job whose sources a
    /// forget took (the store's records, or Days' entries they were
    /// written from) since it was made is stale and never applied.
    fn give(&mut self, now: u64, id: &str, reply: &str) -> &'static str {
        self.expire_jobs(now);
        // Given before (its answer was `busy`): the same outcome, nothing
        // applied again.
        if let Some((_, outcome)) = self.given.iter().find(|(given, _)| given == id) {
            return outcome;
        }
        let Some(at) = self.jobs.iter().position(|job| job.id == id) else {
            return "unknown";
        };
        let Some(job) = self.jobs.remove(at) else {
            return "unknown";
        };
        let outcome = self.give_job(now, job, reply);
        if self.given.len() >= GIVEN_KEPT {
            self.given.pop_front();
        }
        self.given.push_back((id.to_owned(), outcome));
        outcome
    }

    fn give_job(&mut self, now: u64, job: NightJob, reply: &str) -> &'static str {
        // Dropped whatever happens next: a reply that fails the checks is
        // never asked again.
        if !reply.starts_with('{') {
            add(&self.tally, |t| t.jobs_invalid += 1);
            self.said_night(&job.scope, "invalid");
            return "invalid";
        }
        let mapped: HashSet<i64> = self
            .map_of(&job.scope)
            .into_iter()
            .map(|(_, r)| r)
            .collect();
        let mut next = None;
        let outcome = match self.store(&job.scope) {
            None => "stale",
            Some(store) => {
                let present: Option<HashSet<i64>> = store
                    .records(Access::OWNER_ALONE)
                    .ok()
                    .map(|records| records.iter().map(|r| r.id).collect());
                let sources_held = present.is_some_and(|present| {
                    job.input
                        .records()
                        .iter()
                        .all(|r| present.contains(&r.id) && mapped.contains(&r.id))
                });
                if !sources_held {
                    "stale"
                } else {
                    match store.apply_sleep(&job.input, reply) {
                        Ok(report) if report.pending && job.pass + 1 < NIGHT_PASSES => {
                            next = store
                                .prepare_sleep(now as i64, PassKind::Nightly)
                                .ok()
                                .flatten();
                            "applied"
                        }
                        Ok(_) => "applied",
                        Err(lamp_memory_tiers::Error::Stale) => "stale",
                        Err(_) => "invalid",
                    }
                }
            }
        };
        add(&self.tally, |t| match outcome {
            "applied" => {
                t.jobs_applied += 1;
                if job.pass == 0 {
                    t.nights_applied += 1;
                }
            }
            "invalid" => t.jobs_invalid += 1,
            _ => t.jobs_stale += 1,
        });
        if let Some(next) = next {
            self.queue_job(&job.scope, next, job.pass + 1, now);
        }
        self.said_night(&job.scope, outcome);
        outcome
    }
}
