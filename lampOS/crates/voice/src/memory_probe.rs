//! The memory benchmark's probe (#4748): what the benchmark's full product
//! arm needs to run a month of days through Voice's own Days client and the
//! real store, and to look at it in between.
//!
//! - A clock and zone ([`Clock`]): once installed, the client's tool
//!   dispatch, its session and rule calls, the observation pass and the
//!   conversation summary read it in place of the system's;
//!   [`DaysClient::run_probe_at`] takes the moment and zone per call.
//! - A bounded drain ([`drain`]): waits, within its time, until the writer,
//!   the observation pass and the summary thread are each done with every
//!   job queued before it, their store replies included.
//! - A checkpoint ([`checkpoint`]) of a drained, stopped view: the store's
//!   files (sources, kept things, observations, talk notes, follow-up marks
//!   and offers, forget records, sessions) and the client's own state (its
//!   provider context, follow-up marks, forget state and cursors), and the
//!   running notes. [`restore`] puts it back on the same view; [`copy_store`]
//!   gives a disposable child its store.
//!
//! Three bounds (reviewer-three on #4748):
//! - Test only: this module exists only in a test build or in one made with
//!   `RUSTFLAGS="--cfg lamp_memory_probe"`, which no product build sets and
//!   no feature can turn on; with optimisation it does not build at all
//!   (`lib.rs`). Nothing here reads a socket, flag or switch of its own.
//! - A restore never brings back what a forget removed after its
//!   checkpoint: any forget asked of this process since then, by the tools
//!   (each `forget`, `forget_everything` and rule forget, done or refused),
//!   or by memory's `memory/forgot` (the running notes' count), refuses it.
//! - A timeout or a cancel never gives a checkpoint: [`drain`] gives its
//!   [`Drained`] only when every barrier was answered in time; [`checkpoint`]
//!   takes one, and gives a [`Checkpoint`] only once every file is copied,
//!   checked and moved into place. Otherwise nothing is left.
//!
//! The observation and summary threads are this process's, as is the clock:
//! one view to a process. The store must be stopped by its owner between a
//! drain and a checkpoint or a restore: the files are checked unchanged
//! across the copy, never copied hot.
use super::{
    Current, DaysClient, DaysContext, DaysJob, LocalTime, OBSERVE_FORGETS, OBSERVING, ToolCall,
    Turn,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs, io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{Receiver, RecvTimeoutError, TrySendError, sync_channel},
    },
    time::{Duration, Instant, SystemTime},
};

/// A test clock: the moment, in milliseconds since 1970, and the zone.
/// It only goes forward, so the [`Instant`] it gives does too.
pub struct Clock {
    now_ms: AtomicU64,
    zone: Option<LocalTime>,
    base: Instant,
    base_ms: u64,
}

// Process ownership: the test/debug memory-probe process and its local observer threads; absent
// from product builds.
static CLOCK: OnceLock<Clock> = OnceLock::new();

/// The clock went back: a probe's time only goes forward.
#[derive(Debug, PartialEq, Eq)]
pub struct Backwards;

impl Clock {
    /// Installs this process's clock at `start_ms`, in `zone` (`None`: UTC).
    /// `None` when one is installed already: one view to a process.
    pub fn install(start_ms: u64, zone: Option<LocalTime>) -> Option<&'static Self> {
        let mut made = false;
        let clock = CLOCK.get_or_init(|| {
            made = true;
            Self::new(start_ms, zone)
        });
        made.then_some(clock)
    }

    fn new(start_ms: u64, zone: Option<LocalTime>) -> Self {
        Self {
            now_ms: AtomicU64::new(start_ms),
            zone,
            base: Instant::now(),
            base_ms: start_ms,
        }
    }

    pub fn now_ms(&self) -> u64 {
        self.now_ms.load(Ordering::Acquire)
    }

    pub fn zone(&self) -> Option<&LocalTime> {
        self.zone.as_ref()
    }

    /// The moment as this process's [`Instant`]: the install's, moved on
    /// by as much as the clock was.
    pub fn instant(&self) -> Instant {
        self.base + Duration::from_millis(self.now_ms().saturating_sub(self.base_ms))
    }

    /// Moves the clock to `ms`; never back.
    pub fn set(&self, ms: u64) -> Result<(), Backwards> {
        let was = self.now_ms.fetch_max(ms, Ordering::AcqRel);
        if was > ms { Err(Backwards) } else { Ok(()) }
    }
}

/// This process's probe clock, once installed.
pub fn installed() -> Option<&'static Clock> {
    CLOCK.get()
}

impl DaysClient {
    /// One tool call as [`Self::run_in`] makes it, at `now_ms` in `zone`:
    /// the scope's check, the request's identity and a forget's reply
    /// kept out of the thread are as in production.
    pub fn run_probe_at(
        &self,
        call: &ToolCall,
        deadline: Duration,
        person: Option<&str>,
        request: Option<u64>,
        now_ms: u64,
        zone: Option<&LocalTime>,
    ) -> (Value, bool) {
        let turn: Turn<'_> = (None, request);
        self.run_for_request_at(call, deadline, person, turn, || (now_ms, zone))
    }
}

/// Forgets asked through this process's Days clients (each `forget`,
/// `forget_everything` and rule forget, done or refused).
// Process ownership: the test/debug memory-probe process and its local observer threads; absent
// from product builds.
static FORGETS: AtomicU64 = AtomicU64::new(0);

pub(super) fn forget_asked() {
    FORGETS.fetch_add(1, Ordering::AcqRel);
}

/// A rule's call: its forgets count too, the light's included.
pub(super) fn rule_asked(call: &ToolCall) {
    if call.name == crate::rules::FORGET_RULE || crate::rules::forgets_light(call) {
        forget_asked();
    }
}

/// Everything that counts a forget, as it was at one moment: equal at two
/// moments, no forget of any kind came between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Epoch {
    probe: u64,
    observe: u64,
    hand_ons: u64,
    notes: u64,
}

fn epoch(client: &DaysClient) -> Epoch {
    Epoch {
        probe: FORGETS.load(Ordering::Acquire),
        observe: OBSERVE_FORGETS.load(Ordering::Acquire),
        hand_ons: client.hand_ons.forgotten.load(Ordering::Acquire),
        notes: crate::conversation_summary::forgets(),
    }
}

/// Stops a drain, a checkpoint or a restore from another thread.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// The queues a drain waits for, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Writer,
    Observation,
    Summary,
}

/// Why a drain gave nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum NotDrained {
    TimedOut(Stage),
    Cancelled(Stage),
    /// The stage's thread is gone.
    Gone(Stage),
    /// An answer's hand-on is held (`LAMP_SAID_AS_HEARD`): not yet queued.
    Held,
    /// An exchange was lost on its way to the writer: the journal, and
    /// every tidy-up of this worker after it, is incomplete.
    Lost,
}

/// A drain that finished in time: the only way to a [`checkpoint`].
/// Neither cloned nor made outside this module.
#[derive(Debug)]
pub struct Drained {
    view: String,
    epoch: Epoch,
    queued: u64,
}

/// The id a barrier on the observation thread carries; a job from the
/// memory always has `"job"`, which a barrier never has.
const OBSERVE_BARRIER: &str = "lamp_memory_probe_barrier";
// Process ownership: the test/debug memory-probe process and its local observer threads; absent
// from product builds.
static OBSERVE_SENT: AtomicU64 = AtomicU64::new(0);
// Process ownership: the test/debug memory-probe process and its local observer threads; absent
// from product builds.
static OBSERVE_DONE: AtomicU64 = AtomicU64::new(0);

/// On the observation thread: whether `input` is a probe's barrier, marked
/// reached. Every job before it is done.
pub(super) fn observe_barrier_reached(input: &Value) -> bool {
    match input.get(OBSERVE_BARRIER).and_then(Value::as_u64) {
        Some(id) if input.get("job").is_none() => {
            OBSERVE_DONE.fetch_max(id, Ordering::AcqRel);
            true
        }
        _ => false,
    }
}

/// A barrier on the observation thread: `None` with no thread (nothing can
/// be queued), else its id, or nothing sent with the queue full.
fn observe_barrier() -> Option<Result<u64, ()>> {
    let jobs = OBSERVING.get()?;
    let id = OBSERVE_SENT.fetch_add(1, Ordering::AcqRel) + 1;
    Some(
        match jobs.try_send((
            None,
            json!({ OBSERVE_BARRIER: id }),
            0,
            Vec::new(),
            Vec::new(),
            false,
        )) {
            Ok(()) => Ok(id),
            Err(_) => Err(()),
        },
    )
}

const POLL: Duration = Duration::from_millis(2);

/// Waits, until `until`, while `pending`; cancelled or late, why not.
fn wait_while(
    until: Instant,
    cancel: &Cancel,
    stage: Stage,
    mut pending: impl FnMut(Duration) -> Result<bool, NotDrained>,
) -> Result<(), NotDrained> {
    loop {
        if cancel.cancelled() {
            return Err(NotDrained::Cancelled(stage));
        }
        let left = until.saturating_duration_since(Instant::now());
        if !pending(left.min(POLL))? {
            return Ok(());
        }
        if left.is_zero() {
            return Err(NotDrained::TimedOut(stage));
        }
    }
}

/// Waits for `answered`, the barrier's answer.
fn answered(
    answered: &Receiver<()>,
    until: Instant,
    cancel: &Cancel,
    stage: Stage,
) -> Result<(), NotDrained> {
    wait_while(until, cancel, stage, |slice| {
        match answered.recv_timeout(slice) {
            Ok(()) => Ok(false),
            Err(RecvTimeoutError::Timeout) => Ok(true),
            Err(RecvTimeoutError::Disconnected) => Err(NotDrained::Gone(stage)),
        }
    })
}

/// Waits, within `within`, until the writer, then the observation pass,
/// then the summary thread have each done every job queued before their
/// barrier, the store's reply to each included. A timeout, a cancel, a
/// held answer or a lost exchange gives no [`Drained`].
pub fn drain(
    client: &DaysClient,
    within: Duration,
    cancel: &Cancel,
) -> Result<Drained, NotDrained> {
    let until = Instant::now() + within;
    // The writer: its barrier queued after everything before it.
    let (done, writer) = sync_channel(1);
    let mut barrier = Some(DaysJob::Barrier(done));
    wait_while(until, cancel, Stage::Writer, |slice| {
        match client
            .jobs
            .try_send(barrier.take().expect("held until sent"))
        {
            Ok(()) => Ok(false),
            Err(TrySendError::Full(back)) => {
                barrier = Some(back);
                std::thread::sleep(slice);
                Ok(true)
            }
            Err(TrySendError::Disconnected(_)) => Err(NotDrained::Gone(Stage::Writer)),
        }
    })?;
    answered(&writer, until, cancel, Stage::Writer)?;
    // The observation pass: the writer queues its jobs, so after it.
    let mut sent = None;
    wait_while(until, cancel, Stage::Observation, |slice| {
        match sent {
            Some(id) => {
                if OBSERVE_DONE.load(Ordering::Acquire) >= id {
                    return Ok(false);
                }
            }
            None => match observe_barrier() {
                None => return Ok(false),
                Some(Ok(id)) => sent = Some(id),
                Some(Err(())) => {}
            },
        }
        std::thread::sleep(slice);
        Ok(true)
    })?;
    // The summary thread: the writer queues its talk notes, so after it.
    let mut summary = None;
    wait_while(until, cancel, Stage::Summary, |slice| {
        if summary.is_some() {
            return Ok(false);
        }
        match crate::conversation_summary::barrier() {
            None => return Ok(false),
            Some(Ok(answer)) => summary = Some(answer),
            Some(Err(())) => std::thread::sleep(slice),
        }
        Ok(summary.is_none())
    })?;
    if let Some(summary) = summary {
        answered(&summary, until, cancel, Stage::Summary)?;
    }
    if client
        .hand_ons
        .held
        .lock()
        .map_or(true, |held| !held.is_empty())
    {
        return Err(NotDrained::Held);
    }
    if client.lost.load(Ordering::Acquire) != 0 {
        return Err(NotDrained::Lost);
    }
    if cancel.cancelled() {
        return Err(NotDrained::Cancelled(Stage::Summary));
    }
    Ok(Drained {
        view: client.prefix.clone(),
        epoch: epoch(client),
        queued: client.hand_ons.queued.load(Ordering::Acquire),
    })
}

/// The client's own state at a checkpoint.
struct Voice {
    context: DaysContext,
    current: Current,
    forgot_everything: HashMap<String, (i64, Option<String>)>,
    kept_since_note: bool,
    notes: Vec<(Option<String>, String)>,
}

/// A drained view's store files and client state, complete: made only by
/// [`checkpoint`]. Its directory is the caller's to remove.
pub struct Checkpoint {
    view: String,
    epoch: Epoch,
    dir: PathBuf,
    files: Vec<String>,
    voice: Voice,
}

impl Checkpoint {
    /// Where its store files are.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Why no checkpoint was made; nothing of it is left.
#[derive(Debug)]
pub enum NotTaken {
    Cancelled,
    /// The drain was another client's.
    OtherView,
    /// A forget came after the drain.
    ForgotSince,
    /// An exchange was queued after the drain.
    Moved,
    /// The writer's conversation is open: end it ([`DaysClient::end_conversation`])
    /// and drain again.
    ConversationOpen,
    /// The store's files changed while they were copied: it was not stopped.
    StoreChanged,
    /// The checkpoint's directory is there already.
    Exists,
    /// Something in the store's directory is not a plain file.
    NotAFile(String),
    Io(io::Error),
}

impl From<io::Error> for NotTaken {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Each plain file in `dir` by name, with its length and modification
/// time; anything else is refused.
fn listed(dir: &Path) -> Result<Vec<(String, u64, SystemTime)>, NotTaken> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|name| NotTaken::NotAFile(name.to_string_lossy().into_owned()))?;
        let meta = fs::symlink_metadata(entry.path())?;
        if !meta.file_type().is_file() {
            return Err(NotTaken::NotAFile(name));
        }
        files.push((name, meta.len(), meta.modified()?));
    }
    files.sort();
    Ok(files)
}

/// A checkpoint of the view `drained` drained, of its stopped store in
/// `store`, into `into` (which must not be there). Given only complete:
/// a cancel, a forget or a queued exchange since the drain, an open
/// conversation, or a store that changed during the copy leaves nothing.
pub fn checkpoint(
    client: &DaysClient,
    drained: Drained,
    store: &Path,
    into: &Path,
    cancel: &Cancel,
) -> Result<Checkpoint, NotTaken> {
    if drained.view != client.prefix {
        return Err(NotTaken::OtherView);
    }
    let still = || -> Result<(), NotTaken> {
        if cancel.cancelled() {
            return Err(NotTaken::Cancelled);
        }
        if epoch(client) != drained.epoch {
            return Err(NotTaken::ForgotSince);
        }
        if client.hand_ons.queued.load(Ordering::Acquire) != drained.queued {
            return Err(NotTaken::Moved);
        }
        Ok(())
    };
    still()?;
    if fs::symlink_metadata(into).is_ok() {
        return Err(NotTaken::Exists);
    }
    let partial = into.with_extension("partial");
    let _ = fs::remove_dir_all(&partial);
    fs::DirBuilder::new().mode(0o700).create(&partial)?;
    let made = (|| -> Result<Checkpoint, NotTaken> {
        let before = listed(store)?;
        for (name, _, _) in &before {
            still()?;
            fs::copy(store.join(name), partial.join(name))?;
            fs::set_permissions(partial.join(name), fs::Permissions::from_mode(0o600))?;
        }
        if listed(store)? != before {
            return Err(NotTaken::StoreChanged);
        }
        let voice = {
            let current = client.current.lock().map_err(|_| NotTaken::Moved)?.clone();
            if !current.1.is_empty() {
                return Err(NotTaken::ConversationOpen);
            }
            Voice {
                context: client.context.lock().map_err(|_| NotTaken::Moved)?.clone(),
                current,
                forgot_everything: client
                    .forgot_everything
                    .lock()
                    .map_err(|_| NotTaken::Moved)?
                    .clone(),
                kept_since_note: client.kept_since_note.load(Ordering::Acquire),
                notes: crate::conversation_summary::notes(),
            }
        };
        still()?;
        fs::rename(&partial, into)?;
        Ok(Checkpoint {
            view: drained.view.clone(),
            epoch: drained.epoch,
            dir: into.to_owned(),
            files: before.into_iter().map(|(name, _, _)| name).collect(),
            voice,
        })
    })();
    if made.is_err() {
        let _ = fs::remove_dir_all(&partial);
    }
    made
}

/// Why a restore was refused; nothing was changed.
#[derive(Debug)]
pub enum Refused {
    Cancelled,
    /// The checkpoint is another client's.
    OtherView,
    /// A forget came after the checkpoint: what it removed stays removed.
    ForgotSince,
    /// A file of the checkpoint is gone.
    Incomplete(String),
    Io(io::Error),
}

/// Puts `checkpoint` back on the view it was taken of: its store files in
/// the stopped store's `store`, and the client's state. Refused for
/// another client's checkpoint, and after any forget since it was taken,
/// so nothing a forget removed comes back.
pub fn restore(
    client: &DaysClient,
    checkpoint: &Checkpoint,
    store: &Path,
    cancel: &Cancel,
) -> Result<(), Refused> {
    if checkpoint.view != client.prefix {
        return Err(Refused::OtherView);
    }
    if epoch(client) != checkpoint.epoch {
        return Err(Refused::ForgotSince);
    }
    for name in &checkpoint.files {
        if !checkpoint.dir.join(name).is_file() {
            return Err(Refused::Incomplete(name.clone()));
        }
    }
    if cancel.cancelled() {
        return Err(Refused::Cancelled);
    }
    // From here every step is taken: a half-restored store is worse than
    // either.
    let voice = &checkpoint.voice;
    if !crate::conversation_summary::put_back(&voice.notes, checkpoint.epoch.notes) {
        return Err(Refused::ForgotSince);
    }
    let io = |error: io::Error| Refused::Io(error);
    for entry in fs::read_dir(store).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !checkpoint.files.contains(&name) {
            fs::remove_file(entry.path()).map_err(io)?;
        }
    }
    for name in &checkpoint.files {
        let placing = store.join(format!(".{name}.probe"));
        fs::copy(checkpoint.dir.join(name), &placing).map_err(io)?;
        fs::set_permissions(&placing, fs::Permissions::from_mode(0o600)).map_err(io)?;
        fs::rename(&placing, store.join(name)).map_err(io)?;
    }
    if let Ok(mut context) = client.context.lock() {
        *context = voice.context.clone();
    }
    if let Ok(mut current) = client.current.lock() {
        *current = voice.current.clone();
    }
    if let Ok(mut forgot) = client.forgot_everything.lock() {
        forgot.clone_from(&voice.forgot_everything);
    }
    client
        .kept_since_note
        .store(voice.kept_since_note, Ordering::Release);
    Ok(())
}

/// A disposable child's store: the checkpoint's files in `into`, a new
/// directory. The child starts as a fresh Voice process would, with its
/// own client; the view checkpointed is never written.
pub fn copy_store(checkpoint: &Checkpoint, into: &Path) -> io::Result<()> {
    fs::DirBuilder::new().mode(0o700).create(into)?;
    for name in &checkpoint.files {
        fs::copy(checkpoint.dir.join(name), into.join(name))?;
        fs::set_permissions(into.join(name), fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "memory_probe_tests.rs"]
mod tests;
