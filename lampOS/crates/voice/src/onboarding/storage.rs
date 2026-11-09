//! One bounded storage executor per service. No caller waits for filesystem I/O.
use super::{Progress, privacy};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_PROGRESS: usize = 16 * 1024;
const MAX_MAILBOX: usize = 16 * 1024;
const MAX_LINES: usize = 128;
/// How often the worker retries a take-back the disk refused, as often as
/// lamp-eyes re-reads its yes.
const RETRY_TAKE_BACK: Duration = Duration::from_secs(2);
/// Consent files a take-back failed to remove, each with the bytes it held
/// then: retried only while it still holds them, so a yes written after
/// (the app's, or the voice's on a later yes) is never removed by a retry.
type Owed = Vec<(PathBuf, Vec<u8>)>;

#[derive(Clone)]
pub(crate) struct Paths {
    pub progress: PathBuf,
    pub checkpoint: PathBuf,
    pub mailbox: PathBuf,
    /// This home's near-call bar (`lamp_near_call::FILE` in the product).
    pub near_call: PathBuf,
    /// The room sounds yes for lamp-ears ([`super::ROOM_SOUNDS_PATH`] on a
    /// lamp), and the group that may read it: `None` keeps it root's alone.
    pub room_sounds: PathBuf,
    pub room_sounds_group: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tag {
    pub epoch: u64,
    pub revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Submit {
    Queued(Tag),
    Busy,
    Closed,
}
#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pub epoch: u64,
    pub revision: u64,
    pub ready: bool,
    pub busy: bool,
    pub progress: Option<Progress>,
    pub checkpoint: Option<Vec<u8>>,
    pub mailbox: Option<(u64, String)>,
    pub failed: Option<String>,
    pub hardware_location: Option<String>,
}
enum Operation {
    Initialize,
    Commit(Progress, bool),
    Closed(privacy::Checkpoint),
    Invalidate(Option<Progress>),
    Mailbox,
    /// Keep a measured bar, or remove any bar when the measurement made none.
    NearCall(Option<lamp_near_call::Calibrated>),
}
struct Job {
    tag: Tag,
    operation: Operation,
}
#[derive(Default)]
struct Update {
    progress: Option<Progress>,
    checkpoint: Option<Option<Vec<u8>>>,
    mailbox: Option<String>,
    hardware_location: Option<String>,
}
struct Completion {
    tag: Tag,
    result: io::Result<Update>,
}
struct Shared {
    sender: Option<SyncSender<Job>>,
    receiver: Receiver<Completion>,
    snapshot: Snapshot,
    next_revision: u64,
    flight: Option<Tag>,
    pending: Option<Job>,
    closed: bool,
}
#[derive(Clone)]
pub(crate) struct Client {
    shared: Arc<Mutex<Shared>>,
    paths: Paths,
}
pub(crate) struct ServiceStorage {
    client: Client,
    worker: Option<JoinHandle<()>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StorageShutdown {
    Finished,
    Pending,
}

impl Shared {
    fn tag(&mut self) -> io::Result<Tag> {
        self.next_revision = self
            .next_revision
            .checked_add(1)
            .ok_or_else(|| io::Error::other("storage revision exhausted"))?;
        Ok(Tag {
            epoch: self.snapshot.epoch,
            revision: self.next_revision,
        })
    }
    fn dispatch(&mut self, job: Job) -> io::Result<()> {
        let tag = job.tag;
        if matches!(
            &job.operation,
            Operation::Closed(_) | Operation::Invalidate(_) | Operation::Initialize
        ) {
            self.snapshot.ready = false;
        }
        match self
            .sender
            .as_ref()
            .ok_or_else(|| io::Error::other("storage closed"))?
            .try_send(job)
        {
            Ok(()) => {
                self.flight = Some(tag);
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                Err(io::Error::other("storage one-flight invariant violated"))
            }
            Err(TrySendError::Disconnected(_)) => {
                Err(io::Error::other("storage worker disconnected"))
            }
        }
    }
    fn poll(&mut self) -> io::Result<()> {
        match self.receiver.try_recv() {
            Ok(completion) => {
                if self.flight != Some(completion.tag) {
                    return Err(io::Error::other("storage completion identity changed"));
                }
                self.flight = None;
                if completion.tag.epoch == self.snapshot.epoch {
                    match completion.result {
                        Ok(update) => {
                            self.snapshot.hardware_location = update.hardware_location;
                            if let Some(progress) = update.progress {
                                self.snapshot.progress = Some(progress);
                            }
                            if let Some(checkpoint) = update.checkpoint {
                                self.snapshot.checkpoint = checkpoint;
                            }
                            if let Some(mailbox) = update.mailbox {
                                self.snapshot.mailbox = Some((completion.tag.revision, mailbox));
                            }
                            self.snapshot.revision = completion.tag.revision;
                            if !self.closed && self.snapshot.failed.is_none() {
                                self.snapshot.ready = true;
                            }
                        }
                        Err(error) => {
                            self.snapshot.ready = false;
                            self.snapshot.failed = Some(short_error(&error));
                            self.pending = None;
                        }
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) if self.flight.is_some() => {
                return Err(io::Error::other("storage worker lost a completion"));
            }
            Err(TryRecvError::Disconnected) => {}
        }
        if self.flight.is_none()
            && self.snapshot.failed.is_none()
            && let Some(job) = self.pending.take()
        {
            self.dispatch(job)?;
        }
        self.snapshot.busy = self.flight.is_some() || self.pending.is_some();
        if self.closed && !self.snapshot.busy {
            self.sender = None;
        }
        Ok(())
    }
}
impl Client {
    fn lock(&self) -> io::Result<std::sync::MutexGuard<'_, Shared>> {
        self.shared
            .try_lock()
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "storage client busy"))
    }
    pub(crate) fn snapshot(&self) -> io::Result<Snapshot> {
        let mut shared = self.lock()?;
        shared.poll()?;
        Ok(shared.snapshot.clone())
    }
    fn submit(&self, operation: Operation, defer: bool) -> io::Result<Submit> {
        let mut shared = self.lock()?;
        shared.poll()?;
        if shared.closed
            || (shared.snapshot.failed.is_some() && !matches!(&operation, Operation::Mailbox))
        {
            return Ok(Submit::Closed);
        }
        if shared.pending.is_some() || (!defer && shared.flight.is_some()) {
            return Ok(Submit::Busy);
        }
        if matches!(&operation, Operation::Closed(_)) {
            shared.snapshot.ready = false;
        }
        let tag = shared.tag()?;
        let job = Job { tag, operation };
        if shared.flight.is_some() {
            shared.pending = Some(job);
        } else {
            shared.dispatch(job)?;
        }
        shared.snapshot.busy = true;
        Ok(Submit::Queued(tag))
    }
    pub(crate) fn commit(&self, progress: Progress, erase_checkpoint: bool) -> io::Result<Submit> {
        progress.check()?;
        self.submit(Operation::Commit(progress, erase_checkpoint), false)
    }
    /// Only the runner acknowledges a processed batch; service polling leaves
    /// it available across cycles until this explicit matching acknowledgement.
    pub(crate) fn ack_mailbox(&self, revision: u64) -> io::Result<()> {
        let mut shared = self.lock()?;
        if shared
            .snapshot
            .mailbox
            .as_ref()
            .is_some_and(|(seen, _)| *seen == revision)
        {
            shared.snapshot.mailbox = None;
        }
        Ok(())
    }

    /// The near-call file is written on this worker only: its synchronous
    /// write and fsync never run on the interaction loop (#1803).
    pub(crate) fn near_call(
        &self,
        calibrated: Option<lamp_near_call::Calibrated>,
    ) -> io::Result<Submit> {
        if calibrated.as_ref().is_some_and(|kept| !kept.fits()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "near-call bar does not fit its tries",
            ));
        }
        self.submit(Operation::NearCall(calibrated), false)
    }

    pub(crate) fn read_mailbox(&self) -> io::Result<Submit> {
        self.submit(Operation::Mailbox, false)
    }
    /// Fence visibility immediately. The ordered barrier still runs AFTER an
    /// in-flight native write; ignoring its stale completion alone is not cancellation.
    pub(crate) fn fence(&self, progress: Option<Progress>) -> io::Result<Tag> {
        if let Some(progress) = &progress {
            progress.check()?;
        }
        let mut shared = self.lock()?;
        shared.poll()?;
        if shared.closed {
            return Err(io::Error::other("storage closed"));
        }
        let progress = progress.or_else(|| shared.snapshot.progress.clone());
        shared.snapshot.epoch = shared
            .snapshot
            .epoch
            .checked_add(1)
            .ok_or_else(|| io::Error::other("storage epoch exhausted"))?;
        shared.snapshot.ready = false;
        shared.snapshot.checkpoint = None;
        shared.snapshot.mailbox = None;
        shared.snapshot.failed = None;
        let tag = shared.tag()?;
        shared.pending = Some(Job {
            tag,
            operation: Operation::Invalidate(progress),
        });
        if shared.flight.is_none() {
            let job = shared.pending.take().unwrap();
            shared.dispatch(job)?;
        }
        shared.snapshot.busy = true;
        Ok(tag)
    }
    pub(crate) fn persist_closed(&self, checkpoint: privacy::Checkpoint) -> io::Result<Submit> {
        if !checkpoint.matches_path(&self.paths.checkpoint) {
            return Err(io::Error::other(
                "checkpoint path differs from storage owner",
            ));
        }
        if checkpoint.is_discard() {
            return self.fence(None).map(Submit::Queued);
        }
        self.submit(Operation::Closed(checkpoint), true)
    }
    fn close(&self) -> io::Result<()> {
        let mut shared = self.lock()?;
        shared.closed = true;
        // Stop new submissions, but retain accepted ordered work. In particular
        // a cancellation barrier must follow the old write before sender close.
        if shared.flight.is_none() && shared.pending.is_none() {
            shared.sender = None;
        }
        shared.snapshot.ready = false;
        Ok(())
    }
}
impl ServiceStorage {
    pub(crate) fn start(paths: Paths) -> io::Result<Self> {
        Self::start_inner(
            paths,
            Box::new(|_| Ok(())),
            Box::new(super::hardware::from_environment),
        )
    }
    #[cfg(test)]
    pub(crate) fn start_with_hook(
        paths: Paths,
        before: Box<dyn FnMut(Tag) -> io::Result<()> + Send>,
    ) -> io::Result<Self> {
        Self::start_inner(paths, before, Box::new(super::hardware::from_environment))
    }
    fn start_inner(
        paths: Paths,
        mut before: Box<dyn FnMut(Tag) -> io::Result<()> + Send>,
        profile: Box<dyn FnOnce() -> Option<String> + Send>,
    ) -> io::Result<Self> {
        for path in [
            &paths.progress,
            &paths.checkpoint,
            &paths.mailbox,
            &paths.near_call,
        ] {
            if !path.is_absolute() || path.file_name().is_none() {
                return Err(io::Error::other("storage paths must be absolute files"));
            }
        }
        let (sender, jobs) = mpsc::sync_channel::<Job>(1);
        let (completed, receiver) = mpsc::sync_channel(1);
        let files = paths.clone();
        let worker = thread::Builder::new()
            .name("onboarding-storage".into())
            .spawn(move || {
                // Optional profile I/O belongs to this worker too, once per service.
                let hardware_location = profile();
                let mut owed = Owed::new();
                let mut retry_at = Instant::now();
                loop {
                    // A failed write leaves storage idle until a fence or a
                    // restart, so a refused take-back is retried here.
                    let job = if owed.is_empty() {
                        jobs.recv().map_err(|_| RecvTimeoutError::Disconnected)
                    } else {
                        jobs.recv_timeout(retry_at.saturating_duration_since(Instant::now()))
                    };
                    let job = match job {
                        Ok(job) => job,
                        Err(RecvTimeoutError::Timeout) => {
                            retry_take_backs(&mut owed);
                            retry_at = Instant::now() + RETRY_TAKE_BACK;
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    };
                    let was_owed = !owed.is_empty();
                    let result = before(job.tag)
                        .and_then(|()| run(&files, job.operation, &mut owed))
                        .map(|mut update| {
                            update.hardware_location = hardware_location.clone();
                            update
                        });
                    if !was_owed && !owed.is_empty() {
                        eprintln!(
                            "lamp-voice: a take-back the disk refused is retried every 2 s until it goes"
                        );
                        retry_at = Instant::now() + RETRY_TAKE_BACK;
                    }
                    if completed
                        .send(Completion {
                            tag: job.tag,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        let client = Client {
            paths,
            shared: Arc::new(Mutex::new(Shared {
                sender: Some(sender),
                receiver,
                snapshot: Snapshot {
                    epoch: 1,
                    ..Snapshot::default()
                },
                next_revision: 0,
                flight: None,
                pending: None,
                closed: false,
            })),
        };
        client.submit(Operation::Initialize, false)?;
        Ok(Self {
            client,
            worker: Some(worker),
        })
    }
    pub(crate) fn client(&self) -> Client {
        self.client.clone()
    }
    /// Only outside the interaction loop. Pending retains the same worker; it
    /// must never be replaced while a native write can still finish.
    pub(crate) fn finish(&mut self, limit: Duration) -> io::Result<StorageShutdown> {
        self.client.close()?;
        let deadline = Instant::now() + limit;
        while self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            let _ = self.client.snapshot();
            if Instant::now() >= deadline {
                return Ok(StorageShutdown::Pending);
            }
            thread::sleep(Duration::from_millis(1));
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("storage worker panicked"))?;
        }
        Ok(StorageShutdown::Finished)
    }
}
impl Drop for ServiceStorage {
    fn drop(&mut self) {
        let _ = self.client.close();
    }
}
fn short_error(error: &io::Error) -> String {
    error.to_string().chars().take(512).collect()
}
fn read_bounded(path: &Path, max: usize) -> io::Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take((max + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(io::Error::other("onboarding storage payload exceeds bound"));
    }
    Ok(Some(bytes))
}
fn atomic_progress(paths: &Paths, progress: &Progress, owed: &mut Owed) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = &paths.progress;
    progress.check()?;
    let bytes = serde_json::to_vec(progress).map_err(io::Error::other)?;
    if bytes.len() > MAX_PROGRESS {
        return Err(io::Error::other("progress exceeds bound"));
    }
    // Whether the progress this write replaces already held each yes: a
    // record is written only by the write that first keeps it, so a forget
    // that removed it is not undone by a later write.
    let before = read_bounded(path, MAX_PROGRESS)
        .ok()
        .flatten()
        .and_then(|bytes| serde_json::from_slice::<Progress>(&bytes).ok());
    // A "no" or a redo first written here takes back the app's yes before
    // the progress holding it is kept, so a crash between the two leaves
    // no yes rather than one the owner just said no to. A take-back that
    // fails fails the write before the progress keeps the "no", so the next
    // write still sees it new and tries again (Jarvis's P2 on #5848); the
    // worker also retries it on its own ([`retry_take_backs`]).
    take_back(paths, progress, before.as_ref(), owed)?;
    let next = path.with_extension("storage-next");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&next)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&next, path)?;
        fs::File::open(path.parent().unwrap())?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(next);
        return result;
    }
    consents(paths, progress, |step| {
        before
            .as_ref()
            .is_none_or(|before| kept_yes(before, step).is_none())
    })
}

/// The yes for each step in [`YES_STEPS`] removed, whoever gave it, when
/// `progress` is the first write of a "no" (`skipped`) at the lamp or of a
/// redo for that step: its count of them ([`Progress::yes_taken_back`]),
/// its kept "no" or its redo time differs from the progress it replaces
/// (`before`, none when unreadable; the count catches a second one in the
/// same second, the times a file from a build without the count). The
/// newest thing the walk heard wins whatever the clocks say (the first
/// builder's P3 on #5567), and a start or a write that keeps the same "no"
/// or redo leaves an app yes given after it. The consent folder is synced
/// after a removal, so the take-back is on disk before the progress that
/// keeps the "no". Each is made holding the consent folder, a fresh
/// revision kept first (`lamp_msgs::consent::take_back_at`, #5500), so a
/// `consent.set` from before it never applies after. The first error is
/// returned, after every step is tried; a file that failed to go is added
/// to `owed` for the worker to retry.
fn take_back(
    paths: &Paths,
    progress: &Progress,
    before: Option<&Progress>,
    owed: &mut Owed,
) -> io::Result<()> {
    let no = |progress: &Progress, step: &str| {
        progress
            .steps
            .get(step)
            .filter(|kept| kept.outcome == super::Outcome::Skipped)
            .map(|kept| kept.at_unix_s)
    };
    let redone = |progress: &Progress, step: &str| progress.yes_redone_at_unix_s.get(step).copied();
    let count = |progress: &Progress, step: &str| progress.yes_taken_back.get(step).copied();
    let mut first = Ok(());
    for step in YES_STEPS {
        let new_count = count(progress, step).is_some()
            && before.is_none_or(|before| count(before, step) != count(progress, step));
        let new_no = no(progress, step).is_some()
            && before.is_none_or(|before| no(before, step) != no(progress, step));
        let new_redo = redone(progress, step).is_some()
            && before.is_none_or(|before| redone(before, step) != redone(progress, step));
        if !(new_count || new_no || new_redo) {
            continue;
        }
        let path = consent_path(paths, step);
        // What the file held is read holding the folder, before anything
        // changes: a retry removes only that yes, never a newer one given
        // after this take-back failed and the folder was let go.
        let result = match lamp_msgs::consent::withdraw_at(&path, step) {
            Ok(()) => Ok(()),
            Err(refused) => {
                if let Some(held) = refused.held {
                    owed.retain(|(owed, _)| *owed != path);
                    owed.push((path, held));
                }
                Err(refused.error)
            }
        };
        if first.is_ok() {
            first = result;
        }
    }
    first
}

/// The step whose yes is kept at `path` ([`consent_path`]): face
/// expressions at its own name, room sounds at any other.
fn step_of(path: &Path) -> &'static str {
    if super::face_expressions_beside(path) == path {
        super::FACE_EXPRESSIONS
    } else {
        super::ROOM_SOUNDS
    }
}

/// Each take-back the disk refused tried again: kept owed only while the
/// file still holds what it held then and its removal fails again, each
/// holding the consent folder with a fresh revision kept first, as
/// [`take_back`]. Only removes, never writes a yes or the progress.
fn retry_take_backs(owed: &mut Owed) {
    owed.retain(|(path, held)| {
        let step = step_of(path);
        let Some(folder) = path.parent() else {
            return false;
        };
        let Ok(holding) = lamp_msgs::consent::hold(folder) else {
            return true;
        };
        match read_bounded(path, MAX_PROGRESS) {
            Ok(Some(bytes)) if bytes == *held => holding.withdraw_at(path, step).is_err(),
            Ok(_) => false,
            Err(_) => true,
        }
    });
    if owed.is_empty() {
        eprintln!("lamp-voice: no take-back the disk refused is owed any more");
    }
}

/// [`consent`] for every step in [`YES_STEPS`], each handled before the
/// first error is returned, so a room sounds file that fails to go never
/// leaves the face expressions yes behind (the reviewer's P3 on #2947).
fn consents(paths: &Paths, progress: &Progress, new: impl Fn(&str) -> bool) -> io::Result<()> {
    let mut first = Ok(());
    for step in YES_STEPS {
        let result = consent(paths, progress, step, new(step));
        if first.is_ok() {
            first = result;
        }
    }
    first
}

/// The steps whose yes another part reads, each kept in a file named for it.
const YES_STEPS: [&str; 2] = [super::ROOM_SOUNDS, super::FACE_EXPRESSIONS];

/// Where `step`'s yes is kept: room sounds at [`Paths::room_sounds`], face
/// expressions beside it.
fn consent_path(paths: &Paths, step: &str) -> PathBuf {
    if step == super::ROOM_SOUNDS {
        paths.room_sounds.clone()
    } else {
        super::face_expressions_beside(&paths.room_sounds)
    }
}

/// When `step`'s kept outcome is a yes (`done`), the time it was kept.
fn kept_yes(progress: &Progress, step: &str) -> Option<u64> {
    progress
        .steps
        .get(step)
        .filter(|kept| kept.outcome == super::Outcome::Done)
        .map(|kept| kept.at_unix_s)
}

/// Whether the file at `path` is the owner's yes for `step` from the app
/// (`"by":"app"`, written by `lamp-setup change`, #5500), which the voice's
/// starts and writes keep. A "no" or a redo at the lamp takes it back when
/// first written ([`take_back`]), never by comparing times. With no time,
/// it doesn't stand.
fn app_yes_stands(path: &Path, step: &str) -> bool {
    let kept = lamp_msgs::consent::read(path, step);
    kept.yes && kept.by_app() && kept.at_unix_s.is_some()
}

/// A yes for another part (room sounds for lamp-ears, face expressions for
/// lamp-eyes; `docs/onboarding.md`), from the progress just written or
/// read: removed unless the step's kept outcome is a yes (`done`), or it is
/// the owner's yes from the app that still stands ([`app_yes_stands`]);
/// written, when `new` (the write that first keeps the yes), whole as
/// `{"version":1,STEP:"yes","at_unix_s":..,"by":"voice","rev":REV}`, 0640 and the
/// reader group's (0600 with no group). Never written again from the
/// progress: a forget of everything removes it for good (`remember.rs`),
/// whoever gave it. Only the answer and its time, never words, sound or a
/// face.
fn consent(paths: &Paths, progress: &Progress, step: &str, new: bool) -> io::Result<()> {
    let path = &consent_path(paths, step);
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::other("a consent file has no folder"))?;
    let Some(at) = kept_yes(progress, step) else {
        // Nothing there: nothing to change, and no folder to hold.
        if matches!(fs::symlink_metadata(path), Err(e) if e.kind() == io::ErrorKind::NotFound) {
            return Ok(());
        }
        // Read and removed holding the folder, a fresh revision first
        // (#5500), so the app's set can't come between.
        let held = lamp_msgs::consent::hold(folder)?;
        if app_yes_stands(path, step) {
            return Ok(());
        }
        return held.take_back_at(path, step).map(|_| ());
    };
    if !new {
        return Ok(());
    }
    // Written holding the folder, a fresh revision kept first and the yes
    // with it (#5500), 0640 and the reader group's (0600 with none: the
    // voice's UMask=0077 is set over after create).
    let result = lamp_msgs::consent::hold(folder).and_then(|held| {
        held.keep_yes_at(
            path,
            step,
            at,
            lamp_msgs::consent::BY_VOICE,
            paths.room_sounds_group,
        )
    });
    // Not kept is no to the reader, the safe side; the progress stands.
    if let Err(error) = result {
        eprintln!(
            "lamp-voice: the {step} yes was not kept at {}: {error}",
            path.display()
        );
    }
    Ok(())
}
#[cfg(test)]
fn execute(paths: &Paths, operation: Operation) -> io::Result<Update> {
    run(paths, operation, &mut Owed::new())
}
fn run(paths: &Paths, operation: Operation, owed: &mut Owed) -> io::Result<Update> {
    let mut update = Update::default();
    match operation {
        Operation::Initialize => {
            privacy::discard_checkpoint(&paths.checkpoint)?;
            privacy::discard_temporaries(&paths.checkpoint)?;
            let next = paths.progress.with_extension("storage-next");
            match fs::remove_file(next) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            let progress = read_bounded(&paths.progress, MAX_PROGRESS)?
                .map(|bytes| serde_json::from_slice::<Progress>(&bytes).map_err(io::Error::other))
                .transpose()?
                .unwrap_or_default();
            progress.check()?;
            consents(paths, &progress, |_| false)?;
            // The consent folder's revisions put in place if missing (the
            // voice starts at boot, as root), so the app's first
            // `consent.get` gives a real one (#5500). Not kept: the app's
            // sets read stale until it is, the safe side.
            if let Some(folder) = paths.room_sounds.parent()
                && let Err(error) = lamp_msgs::consent::hold(folder).and_then(|held| held.revs())
                && error.kind() != io::ErrorKind::NotFound
            {
                eprintln!("lamp-voice: the consent revisions were not kept: {error}");
            }
            update.progress = Some(progress);
            update.checkpoint = Some(None);
        }
        Operation::Commit(progress, erase) => {
            atomic_progress(paths, &progress, owed)?;
            if erase {
                privacy::discard_checkpoint(&paths.checkpoint)?;
                update.checkpoint = Some(None);
            }
            update.progress = Some(progress);
        }
        Operation::Closed(checkpoint) => {
            checkpoint.persist_after_cleanup()?;
            update.checkpoint = Some(checkpoint.closed_bytes()?);
        }
        Operation::Invalidate(progress) => {
            let mut progress = match progress {
                Some(progress) => progress,
                None => read_bounded(&paths.progress, MAX_PROGRESS)?
                    .map(|bytes| {
                        serde_json::from_slice::<Progress>(&bytes).map_err(io::Error::other)
                    })
                    .transpose()?
                    .unwrap_or_default(),
            };
            progress.check()?;
            progress.privacy_discard_pending = true;
            atomic_progress(paths, &progress, owed)?;
            privacy::discard_checkpoint(&paths.checkpoint)?;
            progress.privacy_discard_pending = false;
            atomic_progress(paths, &progress, owed)?;
            update.progress = Some(progress);
            update.checkpoint = Some(None);
        }
        Operation::Mailbox => {
            let taking = paths.mailbox.with_extension("storage-taking");
            if !taking.exists() {
                match fs::rename(&paths.mailbox, &taking) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        update.mailbox = Some(String::new());
                        return Ok(update);
                    }
                    Err(e) => return Err(e),
                }
            }
            let bytes = match read_bounded(&taking, MAX_MAILBOX) {
                Ok(bytes) => bytes.unwrap_or_default(),
                Err(error) => {
                    fs::remove_file(&taking)?;
                    return Err(error);
                }
            };
            // A malformed bounded batch must not permanently hide a later
            // explicit redo appended to the next mailbox file.
            fs::remove_file(taking)?;
            let text = String::from_utf8(bytes).map_err(io::Error::other)?;
            if text.lines().count() > MAX_LINES {
                return Err(io::Error::other("onboarding mailbox exceeds line bound"));
            }
            update.mailbox = Some(text);
        }
        Operation::NearCall(Some(calibrated)) => {
            lamp_near_call::write(&paths.near_call, &calibrated)?;
        }
        Operation::NearCall(None) => match fs::remove_file(&paths.near_call) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        },
    }
    Ok(update)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Files(PathBuf);
    impl Files {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "onboarding-store-{}",
                crate::process::boot_text(crate::process::new_boot().unwrap())
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn paths(&self) -> Paths {
            Paths {
                progress: self.0.join("progress.json"),
                checkpoint: self.0.join("privacy-pending.json"),
                mailbox: self.0.join("progress.told"),
                near_call: self.0.join("near-call.json"),
                room_sounds: self.0.join("room-sounds.json"),
                room_sounds_group: None,
            }
        }
    }
    impl Drop for Files {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    struct Release(Option<SyncSender<()>>);
    impl Release {
        fn now(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.try_send(());
            }
        }
    }
    impl Drop for Release {
        fn drop(&mut self) {
            self.now();
        }
    }
    fn stalled(paths: Paths, revision: u64) -> (ServiceStorage, Receiver<()>, Release) {
        let (entered, seen) = mpsc::sync_channel(1);
        let (release, freed) = mpsc::sync_channel(1);
        let owner = ServiceStorage::start_with_hook(
            paths,
            Box::new(move |tag| {
                if tag.revision == revision {
                    entered.send(()).map_err(io::Error::other)?;
                    freed
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(io::Error::other)?;
                }
                Ok(())
            }),
        )
        .unwrap();
        (owner, seen, Release(Some(release)))
    }
    fn idle(client: &Client) -> Snapshot {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let state = client.snapshot().unwrap();
            if !state.busy {
                return state;
            }
            assert!(Instant::now() < until, "storage fixture timeout");
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn candidate(paths: &Paths) -> privacy::Checkpoint {
        use lamp_msgs::{ClockStamp, PrivacySwitch};
        let sample = |sequence, at_us, muted| privacy::Acquisition {
            controls_session: "0123456789abcdef".into(),
            physical: PrivacySwitch {
                stamp: ClockStamp {
                    clock_id: "test-clock".into(),
                    sequence,
                    at_us,
                },
                reader_session_id: "reader".into(),
                muted: Some(muted),
                fault: None,
            },
        };
        let mut attempt = privacy::Attempt::arm(
            privacy::Epoch {
                device_boot: "boot".into(),
                clock_id: "test-clock".into(),
            },
            privacy::Invitation {
                voice_boot: lamp_turn_owner::BootId::new([1; 16]).unwrap(),
                turn: 1,
                delivered_at_us: 100,
            },
            sample(1, 100, false),
            100,
        )
        .unwrap();
        attempt.observe_close(sample(2, 200, true), 200).unwrap();
        privacy::Checkpoint::new(attempt, paths.checkpoint.clone())
    }

    /// The yes for lamp-ears (#2110): 0640 and the reader group's, written
    /// by the write that first keeps it; a removal (a forget of everything)
    /// is not undone by a later write or start; a progress without the yes
    /// removes it.
    #[test]
    fn the_yeses_for_ears_and_eyes_are_group_readable_and_a_forget_stays_forgotten() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let files = Files::new();
        let mut paths = files.paths();
        // A group this test may give a file to: its own.
        let group = fs::metadata(&files.0).unwrap().gid();
        paths.room_sounds_group = Some(group);
        let yes = |at_unix_s| {
            let mut progress = Progress::default();
            for step in YES_STEPS {
                progress.steps.insert(
                    step.into(),
                    super::super::Kept {
                        outcome: super::super::Outcome::Done,
                        at_unix_s,
                        framing: None,
                        offered_at_unix_s: None,
                        later_offers: 0,
                        ended_at_unix_ms: None,
                    },
                );
            }
            progress
        };
        let face = super::super::face_expressions_beside(&paths.room_sounds);
        let progress = yes(7);
        execute(&paths, Operation::Commit(progress.clone(), false)).unwrap();
        let kept = fs::metadata(&paths.room_sounds).unwrap();
        assert_eq!(kept.permissions().mode() & 0o777, 0o640);
        assert_eq!(kept.gid(), group);
        // With the fresh revision kept for it first (#5500).
        let rev = lamp_msgs::consent::read(&paths.room_sounds, "room_sounds")
            .rev
            .unwrap();
        assert_eq!(
            fs::read(&paths.room_sounds).unwrap(),
            lamp_msgs::consent::bytes_with_rev("room_sounds", 7, "voice", &rev)
        );
        // The face expressions yes, kept the same way for lamp-eyes.
        let kept = fs::metadata(&face).unwrap();
        assert_eq!(kept.permissions().mode() & 0o777, 0o640);
        assert_eq!(kept.gid(), group);
        // With the fresh revision kept for it first (#5500).
        let rev = lamp_msgs::consent::read(&face, "face_expressions")
            .rev
            .unwrap();
        assert_eq!(
            fs::read(&face).unwrap(),
            lamp_msgs::consent::bytes_with_rev("face_expressions", 7, "voice", &rev)
        );
        fs::remove_file(&paths.room_sounds).unwrap();
        fs::remove_file(&face).unwrap();
        execute(&paths, Operation::Commit(progress, false)).unwrap();
        execute(&paths, Operation::Initialize).unwrap();
        assert!(!paths.room_sounds.exists());
        assert!(!face.exists());
        // No group known: root's alone.
        paths.room_sounds_group = None;
        execute(&paths, Operation::Commit(Progress::default(), false)).unwrap();
        execute(&paths, Operation::Commit(yes(8), false)).unwrap();
        let mode = fs::metadata(&paths.room_sounds)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        execute(&paths, Operation::Commit(Progress::default(), false)).unwrap();
        assert!(!paths.room_sounds.exists());
        assert!(!face.exists());
    }

    /// The owner's yes from the app (`"by":"app"`, #5500): kept by a start
    /// and by every write whose progress holds no yes for its step (none
    /// asked, `not_available`, `later`), where a voice's yes would go; taken
    /// back by a redo of the step or of the walk after it, and by a "no" at
    /// the lamp after it. One given after either stands again, and a yes
    /// said at the lamp is the voice's.
    #[test]
    fn an_app_yes_survives_starts_and_writes_until_a_redo_or_a_no() {
        use super::super::{Kept, Outcome};
        let files = Files::new();
        let paths = files.paths();
        let face = super::super::face_expressions_beside(&paths.room_sounds);
        let app = |path: &Path, step: &str, at| {
            fs::write(path, lamp_msgs::consent::bytes(step, at, "app")).unwrap();
        };
        let with = |step: &str, outcome, at_unix_s| {
            let mut progress = Progress::default();
            progress.steps.insert(
                step.into(),
                Kept {
                    outcome,
                    at_unix_s,
                    framing: None,
                    offered_at_unix_s: None,
                    later_offers: 0,
                    ended_at_unix_ms: None,
                },
            );
            progress
        };
        app(&face, "face_expressions", 100);
        app(&paths.room_sounds, "room_sounds", 100);
        // A start, and writes with no yes behind either step: both stay.
        execute(&paths, Operation::Initialize).unwrap();
        for progress in [
            Progress::default(),
            with("face_expressions", Outcome::NotAvailable, 50),
            with("face_expressions", Outcome::Later, 150),
            with("room_sounds", Outcome::Failed, 150),
        ] {
            execute(&paths, Operation::Commit(progress, false)).unwrap();
            execute(&paths, Operation::Initialize).unwrap();
            assert!(face.exists() && paths.room_sounds.exists());
        }
        // The voice's own yes with no `done` behind it goes, as before.
        fs::write(
            &face,
            lamp_msgs::consent::bytes("face_expressions", 100, "voice"),
        )
        .unwrap();
        execute(&paths, Operation::Initialize).unwrap();
        assert!(!face.exists());
        // A "no" at the lamp written after the app's yes takes it; one
        // given after the "no" stands.
        app(&face, "face_expressions", 100);
        let no = with("face_expressions", Outcome::Skipped, 110);
        execute(&paths, Operation::Commit(no.clone(), false)).unwrap();
        assert!(!face.exists());
        app(&face, "face_expressions", 120);
        execute(&paths, Operation::Initialize).unwrap();
        execute(&paths, Operation::Commit(no, false)).unwrap();
        assert!(face.exists());
        // A redo of the step after the app's yes takes it; one given after
        // the redo, even in its second, stands across starts and writes.
        let mut redone = Progress::default();
        redone
            .yes_redone_at_unix_s
            .insert("room_sounds".into(), 120);
        execute(&paths, Operation::Commit(redone.clone(), false)).unwrap();
        assert!(!paths.room_sounds.exists());
        app(&paths.room_sounds, "room_sounds", 120);
        execute(&paths, Operation::Initialize).unwrap();
        execute(&paths, Operation::Commit(redone, false)).unwrap();
        assert!(paths.room_sounds.exists());
        // An app yes with no time, or one not `"yes"`, never stands.
        fs::write(&face, br#"{"face_expressions":"yes","by":"app"}"#).unwrap();
        execute(&paths, Operation::Initialize).unwrap();
        assert!(!face.exists());
        // A yes said at the lamp is written as the voice's, over the app's.
        app(&face, "face_expressions", 100);
        execute(
            &paths,
            Operation::Commit(with("face_expressions", Outcome::Done, 130), false),
        )
        .unwrap();
        assert_eq!(
            lamp_msgs::consent::read(&face, "face_expressions")
                .by
                .as_deref(),
            Some("voice")
        );
    }

    /// A progress holding only `step`'s kept `outcome` at `at_unix_s`.
    fn kept(step: &str, outcome: super::super::Outcome, at_unix_s: u64) -> Progress {
        let mut progress = Progress::default();
        progress.steps.insert(
            step.into(),
            super::super::Kept {
                outcome,
                at_unix_s,
                framing: None,
                offered_at_unix_s: None,
                later_offers: 0,
                ended_at_unix_ms: None,
            },
        );
        progress
    }

    /// The first builder's P3 on #5567: a spoken "no", or a redo, is the
    /// newest thing the walk heard, so the write that first keeps it takes
    /// the app's yes back whatever the two clocks say, even one stamped
    /// long after it (a lamp clock that stepped back).
    #[test]
    fn a_spoken_no_or_a_redo_takes_back_an_app_yes_whatever_the_clocks_say() {
        use super::super::Outcome;
        let files = Files::new();
        let paths = files.paths();
        let face = super::super::face_expressions_beside(&paths.room_sounds);
        let app = |path: &Path, step: &str, at| {
            fs::write(path, lamp_msgs::consent::bytes(step, at, "app")).unwrap();
        };
        app(&face, "face_expressions", 1_000);
        execute(
            &paths,
            Operation::Commit(kept("face_expressions", Outcome::Skipped, 90), false),
        )
        .unwrap();
        assert!(!face.exists(), "a no stamped before the app's yes");
        app(&paths.room_sounds, "room_sounds", 1_000);
        let mut redone = Progress::default();
        redone.yes_redone_at_unix_s.insert("room_sounds".into(), 50);
        execute(&paths, Operation::Commit(redone, false)).unwrap();
        assert!(
            !paths.room_sounds.exists(),
            "a redo stamped before the app's yes"
        );
    }

    /// An app yes given after a "no" (or a redo) at setup was written
    /// stands through every later start and write that keeps the same
    /// "no", whatever its time says; only a new "no", a new redo, a forget
    /// of everything (`remember.rs`) or the reset's plan takes it back.
    #[test]
    fn an_app_yes_after_a_setup_no_survives_restarts_whatever_the_clocks_say() {
        use super::super::Outcome;
        let files = Files::new();
        let paths = files.paths();
        let face = super::super::face_expressions_beside(&paths.room_sounds);
        let app = |path: &Path, step: &str, at| {
            fs::write(path, lamp_msgs::consent::bytes(step, at, "app")).unwrap();
        };
        let no = kept("face_expressions", Outcome::Skipped, 500);
        execute(&paths, Operation::Commit(no.clone(), false)).unwrap();
        // Given after the "no", stamped before it and in its second.
        for at in [100, 500] {
            app(&face, "face_expressions", at);
            for _ in 0..2 {
                execute(&paths, Operation::Initialize).unwrap();
                execute(&paths, Operation::Commit(no.clone(), false)).unwrap();
            }
            assert!(face.exists(), "an app yes at {at} after the no");
        }
        // A second "no" at the lamp takes it back.
        execute(
            &paths,
            Operation::Commit(kept("face_expressions", Outcome::Skipped, 501), false),
        )
        .unwrap();
        assert!(!face.exists());
        // The same after a redo of the step.
        let mut redone = Progress::default();
        redone
            .yes_redone_at_unix_s
            .insert("room_sounds".into(), 500);
        execute(&paths, Operation::Commit(redone.clone(), false)).unwrap();
        app(&paths.room_sounds, "room_sounds", 400);
        execute(&paths, Operation::Initialize).unwrap();
        execute(&paths, Operation::Commit(redone.clone(), false)).unwrap();
        execute(&paths, Operation::Initialize).unwrap();
        assert!(paths.room_sounds.exists(), "an app yes after the redo");
        // A later redo, even stamped earlier, takes it back.
        redone
            .yes_redone_at_unix_s
            .insert("room_sounds".into(), 300);
        execute(&paths, Operation::Commit(redone, false)).unwrap();
        assert!(!paths.room_sounds.exists());
    }

    /// Jarvis's P2 on #5848: a "no" whose take-back fails (here the consent
    /// folder read-only, EACCES) fails its write before the progress keeps
    /// it, so the next write of it still sees a new "no" and retries the
    /// take-back: the app's yes never outlives the owner's "no".
    #[test]
    fn a_failed_take_back_fails_the_write_and_the_next_write_retries_it() {
        use super::super::Outcome;
        use std::os::unix::fs::PermissionsExt;
        let files = Files::new();
        let mut paths = files.paths();
        // The consent files live in a folder of their own on a lamp.
        let folder = files.0.join("consent");
        fs::create_dir(&folder).unwrap();
        paths.room_sounds = folder.join("room-sounds.json");
        let face = super::super::face_expressions_beside(&paths.room_sounds);
        fs::write(
            &face,
            lamp_msgs::consent::bytes("face_expressions", 100, "app"),
        )
        .unwrap();
        let mode = |mode| fs::set_permissions(&folder, fs::Permissions::from_mode(mode)).unwrap();
        mode(0o500);
        // Root writes into a read-only folder: nothing fails to show here.
        if fs::write(folder.join("probe"), b"").is_ok() {
            mode(0o700);
            return;
        }
        let no = kept("face_expressions", Outcome::Skipped, 110);
        let failed = execute(&paths, Operation::Commit(no.clone(), false));
        mode(0o700);
        assert!(failed.is_err(), "a take-back that fails fails the write");
        assert!(face.exists());
        // The same "no" written again, once the folder takes it.
        execute(&paths, Operation::Commit(no.clone(), false)).unwrap();
        execute(&paths, Operation::Initialize).unwrap();
        assert!(
            !lamp_msgs::consent::read(&face, "face_expressions").yes,
            "the app's yes stands after the owner's no"
        );
        let kept_no = read_bounded(&paths.progress, MAX_PROGRESS)
            .unwrap()
            .map(|bytes| serde_json::from_slice::<Progress>(&bytes).unwrap());
        assert_eq!(kept_no, Some(no));
    }

    /// The privacy reviewer's ask on #5884: storage never retries on its
    /// own once a write failed, so a take-back the disk refused is retried
    /// by the worker every [`RETRY_TAKE_BACK`] until the app's yes goes,
    /// with no further write, fence or restart.
    #[test]
    fn a_take_back_the_disk_refused_is_retried_until_the_yes_goes() {
        use super::super::Outcome;
        use std::os::unix::fs::PermissionsExt;
        let files = Files::new();
        let mut paths = files.paths();
        let folder = files.0.join("consent");
        fs::create_dir(&folder).unwrap();
        paths.room_sounds = folder.join("room-sounds.json");
        let face = super::super::face_expressions_beside(&paths.room_sounds);
        fs::write(
            &face,
            lamp_msgs::consent::bytes("face_expressions", 100, "app"),
        )
        .unwrap();
        let mut owner = ServiceStorage::start(paths).unwrap();
        let client = owner.client();
        assert!(idle(&client).ready);
        let mode = |mode| fs::set_permissions(&folder, fs::Permissions::from_mode(mode)).unwrap();
        mode(0o500);
        // Root writes into a read-only folder: nothing fails to show here.
        if fs::write(folder.join("probe"), b"").is_ok() {
            mode(0o700);
            return;
        }
        let no = kept("face_expressions", Outcome::Skipped, 110);
        assert!(matches!(
            client.commit(no, false).unwrap(),
            Submit::Queued(_)
        ));
        let state = idle(&client);
        mode(0o700);
        assert!(
            state.failed.is_some(),
            "a take-back that fails fails the write"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while face.exists() {
            assert!(
                Instant::now() < deadline,
                "the app's yes outlived the owner's no while storage is failed"
            );
            thread::sleep(Duration::from_millis(20));
        }
        // Taken back through a fresh revision (#5500): an app's set from
        // before the "no" is stale.
        let revs = lamp_msgs::consent::Revs::read(&folder);
        assert!(
            lamp_msgs::consent::rev(&folder, "face_expressions", &revs).starts_with("gone-"),
            "{revs:?}"
        );
        assert_eq!(
            owner.finish(Duration::from_secs(2)).unwrap(),
            StorageShutdown::Finished
        );
    }

    /// #5500, the second reviewer's P2 3: what a failed take-back owes is
    /// read holding the folder, so a yes the app gives after the failure
    /// (the folder let go) is never the one the retry removes.
    #[test]
    fn a_retried_take_back_spares_a_yes_given_after_it_failed() {
        use super::super::Outcome;
        use std::os::unix::fs::PermissionsExt;
        let files = Files::new();
        let mut paths = files.paths();
        let folder = files.0.join("consent-newer");
        fs::create_dir(&folder).unwrap();
        paths.room_sounds = folder.join("room-sounds.json");
        let face = super::super::face_expressions_beside(&paths.room_sounds);
        let app = |at| {
            lamp_msgs::consent::hold(&folder)
                .unwrap()
                .keep_yes("face_expressions", at, "app")
                .unwrap()
        };
        app(100);
        let old = fs::read(&face).unwrap();
        let mode = |mode| fs::set_permissions(&folder, fs::Permissions::from_mode(mode)).unwrap();
        mode(0o500);
        // Root writes into a read-only folder: nothing fails to show here.
        if fs::write(folder.join("probe"), b"").is_ok() {
            mode(0o700);
            return;
        }
        let mut owed = Owed::new();
        let no = kept("face_expressions", Outcome::Skipped, 110);
        assert!(take_back(&paths, &no, None, &mut owed).is_err());
        mode(0o700);
        assert_eq!(owed, vec![(face.clone(), old.clone())]);
        app(120);
        let newer = fs::read(&face).unwrap();
        retry_take_backs(&mut owed);
        assert!(owed.is_empty());
        assert_eq!(fs::read(&face).unwrap(), newer);
    }

    /// #5500: the voice's start keeps the consent folder's revisions when
    /// none are there, so the app's first `consent.get` gives one a set can
    /// match, never `unknown`.
    #[test]
    fn a_start_keeps_the_consent_revisions() {
        let files = Files::new();
        let paths = files.paths();
        let folder = paths.room_sounds.parent().unwrap().to_path_buf();
        let rev = || {
            lamp_msgs::consent::rev(
                &folder,
                "face_expressions",
                &lamp_msgs::consent::Revs::read(&folder),
            )
        };
        assert_eq!(rev(), lamp_msgs::consent::REV_UNKNOWN);
        execute(&paths, Operation::Initialize).unwrap();
        let first = rev();
        assert!(first.starts_with(lamp_msgs::consent::GONE), "{first}");
        // A later start leaves them as they are.
        execute(&paths, Operation::Initialize).unwrap();
        assert_eq!(rev(), first);
    }

    /// Two redos of a step in the same second (or after the clock stepped
    /// back) are each a new redo: each takes back an app yes given before
    /// it, though both carry the same time.
    #[test]
    fn two_redos_in_one_second_each_take_back_the_app_yes() {
        let files = Files::new();
        let paths = files.paths();
        let app = || {
            fs::write(
                &paths.room_sounds,
                lamp_msgs::consent::bytes("room_sounds", 120, "app"),
            )
            .unwrap();
        };
        let mut redone = Progress::default();
        redone.yes_redone("room_sounds");
        let at = redone.yes_redone_at_unix_s["room_sounds"];
        app();
        execute(&paths, Operation::Commit(redone.clone(), false)).unwrap();
        assert!(!paths.room_sounds.exists(), "the first redo");
        // An app yes after it stands through the same redo written again.
        app();
        execute(&paths, Operation::Initialize).unwrap();
        execute(&paths, Operation::Commit(redone.clone(), false)).unwrap();
        assert!(paths.room_sounds.exists());
        // A second redo, its time the same second as the first.
        redone.yes_redone("room_sounds");
        redone.yes_redone_at_unix_s.insert("room_sounds".into(), at);
        execute(&paths, Operation::Commit(redone.clone(), false)).unwrap();
        assert!(!paths.room_sounds.exists(), "the second redo in its second");
        // The same for a second "no" kept with the first's time.
        let mut no = kept("room_sounds", super::super::Outcome::Skipped, at);
        no.yes_taken_back = redone.yes_taken_back.clone();
        no.took_back("room_sounds");
        app();
        execute(&paths, Operation::Commit(no.clone(), false)).unwrap();
        assert!(!paths.room_sounds.exists(), "the first no");
        app();
        execute(&paths, Operation::Commit(no.clone(), false)).unwrap();
        assert!(paths.room_sounds.exists(), "the same no written again");
        no.took_back("room_sounds");
        execute(&paths, Operation::Commit(no, false)).unwrap();
        assert!(!paths.room_sounds.exists(), "the second no in its second");
    }

    /// A room sounds file that fails to go (here a folder in its place)
    /// still lets the face expressions yes go, and the error is returned.
    #[test]
    fn a_failed_room_sounds_removal_still_removes_the_face_expressions_yes() {
        let files = Files::new();
        let paths = files.paths();
        let face = super::super::face_expressions_beside(&paths.room_sounds);
        fs::create_dir_all(paths.room_sounds.join("in-the-way")).unwrap();
        for operation in [
            Operation::Commit(Progress::default(), false),
            Operation::Initialize,
        ] {
            fs::write(&face, br#"{"face_expressions":"yes"}"#).unwrap();
            assert!(execute(&paths, operation).is_err());
            assert!(!face.exists());
        }
    }

    #[test]
    fn a_stalled_write_is_fenced_then_overwritten_before_any_new_proof_is_visible() {
        let files = Files::new();
        let paths = files.paths();
        let (mut owner, entered, mut release) = stalled(paths.clone(), 2);
        let client = owner.client();
        assert!(idle(&client).ready);
        let old = Progress {
            privacy_invitation_tries: 1,
            ..Progress::default()
        };
        assert!(matches!(
            client.commit(old, false).unwrap(),
            Submit::Queued(_)
        ));
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..20 {
            assert!(client.snapshot().unwrap().busy);
            assert_eq!(client.read_mailbox().unwrap(), Submit::Busy);
        }
        let fence = client.fence(Some(Progress::default())).unwrap();
        assert!(!client.snapshot().unwrap().ready);
        release.now();
        let state = idle(&client);
        assert!(state.ready);
        assert_eq!(state.epoch, fence.epoch);
        assert_eq!(state.revision, fence.revision);
        assert_eq!(state.progress.unwrap().privacy_invitation_tries, 0);
        let disk: Progress = serde_json::from_slice(&fs::read(&paths.progress).unwrap()).unwrap();
        assert_eq!(disk.privacy_invitation_tries, 0);
        assert!(!disk.privacy_discard_pending);
        assert_eq!(
            owner.finish(Duration::from_secs(2)).unwrap(),
            StorageShutdown::Finished
        );
    }

    #[test]
    fn deferred_closed_proof_stays_hidden_until_its_own_commit() {
        let files = Files::new();
        let paths = files.paths();
        let (mut owner, entered, mut release) = stalled(paths.clone(), 2);
        let client = owner.client();
        idle(&client);
        client.read_mailbox().unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let Submit::Queued(tag) = client.persist_closed(candidate(&paths)).unwrap() else {
            panic!("closed proof not queued")
        };
        let waiting = client.snapshot().unwrap();
        assert!(!waiting.ready);
        assert!(waiting.checkpoint.is_none());
        assert_eq!(client.read_mailbox().unwrap(), Submit::Busy);
        release.now();
        let committed = idle(&client);
        assert!(committed.ready);
        assert_eq!(committed.revision, tag.revision);
        assert!(committed.checkpoint.is_some());
        client.fence(None).unwrap();
        let erased = idle(&client);
        assert!(erased.checkpoint.is_none());
        assert!(!paths.checkpoint.exists());
        owner.finish(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn a_fence_during_initialization_preserves_the_existing_retry_budget() {
        let files = Files::new();
        let paths = files.paths();
        fs::write(
            &paths.progress,
            serde_json::to_vec(&Progress {
                privacy_invitation_tries: 2,
                ..Progress::default()
            })
            .unwrap(),
        )
        .unwrap();
        let (mut owner, entered, mut release) = stalled(paths, 1);
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let client = owner.client();
        client.fence(None).unwrap();
        release.now();
        assert_eq!(idle(&client).progress.unwrap().privacy_invitation_tries, 2);
        owner.finish(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn hardware_profile_is_loaded_once_on_the_storage_worker() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let files = Files::new();
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = reads.clone();
        let caller = thread::current().id();
        let mut owner = ServiceStorage::start_inner(
            files.paths(),
            Box::new(|_| Ok(())),
            Box::new(move || {
                assert_ne!(thread::current().id(), caller);
                observed.fetch_add(1, Ordering::SeqCst);
                Some("test profile location".into())
            }),
        )
        .unwrap();
        let client = owner.client();
        assert_eq!(
            idle(&client).hardware_location.as_deref(),
            Some("test profile location")
        );
        client.read_mailbox().unwrap();
        idle(&client);
        client.fence(None).unwrap();
        assert_eq!(
            idle(&client).hardware_location.as_deref(),
            Some("test profile location")
        );
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        owner.finish(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn service_poll_does_not_consume_the_runners_mailbox_batch() {
        let files = Files::new();
        let paths = files.paths();
        let mut owner = ServiceStorage::start(paths.clone()).unwrap();
        let client = owner.client();
        idle(&client);
        fs::write(&paths.mailbox, "{\"redo\":true}\n").unwrap();
        client.read_mailbox().unwrap();
        let service = idle(&client);
        let next_cycle = owner.client().snapshot().unwrap();
        assert_eq!(service.mailbox, next_cycle.mailbox);
        let batch = next_cycle.mailbox.unwrap();
        assert_eq!(batch.1, "{\"redo\":true}\n");
        client.ack_mailbox(batch.0).unwrap();
        assert!(owner.client().snapshot().unwrap().mailbox.is_none());
        owner.finish(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn shutdown_retains_a_stalled_worker_and_refuses_more_commands() {
        let files = Files::new();
        let (mut owner, entered, mut release) = stalled(files.paths(), 1);
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let client = owner.client();
        assert_eq!(
            owner.finish(Duration::ZERO).unwrap(),
            StorageShutdown::Pending
        );
        assert!(owner.worker.is_some());
        assert_eq!(client.read_mailbox().unwrap(), Submit::Closed);
        release.now();
        assert_eq!(
            owner.finish(Duration::from_secs(2)).unwrap(),
            StorageShutdown::Finished
        );
        assert!(owner.worker.is_none());
    }

    #[test]
    fn shutdown_drains_a_redo_barrier_behind_a_stalled_old_done_write() {
        let files = Files::new();
        let paths = files.paths();
        let (mut owner, entered, mut release) = stalled(paths.clone(), 2);
        let client = owner.client();
        idle(&client);
        let mut done = Progress::default();
        done.steps.insert(
            "privacy".into(),
            super::super::Kept {
                outcome: super::super::Outcome::Done,
                at_unix_s: 1,
                framing: None,
                offered_at_unix_s: None,
                later_offers: 0,
                ended_at_unix_ms: None,
            },
        );
        done.privacy_done_attempt = Some(lamp_turn_owner::BootId::new([7; 16]).unwrap());
        client.commit(done, false).unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let barrier = client.fence(Some(Progress::default())).unwrap();
        assert_eq!(
            owner.finish(Duration::ZERO).unwrap(),
            StorageShutdown::Pending
        );
        assert!(owner.worker.is_some());
        assert_eq!(client.read_mailbox().unwrap(), Submit::Closed);
        assert!(client.snapshot().unwrap().busy);
        release.now();
        assert_eq!(
            owner.finish(Duration::from_secs(2)).unwrap(),
            StorageShutdown::Finished
        );
        let final_state = client.snapshot().unwrap();
        assert_eq!(final_state.revision, barrier.revision);
        assert!(
            !final_state.ready,
            "a stopped store cannot publish a fresh offer"
        );
        let disk: Progress = serde_json::from_slice(&fs::read(&paths.progress).unwrap()).unwrap();
        assert!(disk.steps.is_empty());
        assert_eq!(disk.privacy_done_attempt, None);
        assert!(!disk.privacy_discard_pending);
        // A replacement starts only after the previous worker actually ended.
        let mut next = ServiceStorage::start(paths).unwrap();
        assert!(idle(&next.client()).progress.unwrap().steps.is_empty());
        next.finish(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn the_near_call_bar_is_written_or_removed_only_on_the_worker() {
        let files = Files::new();
        let paths = files.paths();
        let caller = thread::current().id();
        let mut owner = ServiceStorage::start_with_hook(
            paths.clone(),
            Box::new(move |_| {
                assert_ne!(thread::current().id(), caller);
                Ok(())
            }),
        )
        .unwrap();
        let client = owner.client();
        idle(&client);
        let tries = [22.4, 25.0, 23.1].map(|rise_db| lamp_near_call::Try {
            rise_db,
            score: Some(0.98),
            heard_by: lamp_near_call::HeardBy::Name,
        });
        let bar = lamp_near_call::Calibrated::of(tries.to_vec(), 7).unwrap();
        // A bar its tries don't make is refused before it reaches the worker.
        let edited = lamp_near_call::Calibrated {
            near_rise_db: 30.0,
            ..bar.clone()
        };
        assert!(client.near_call(Some(edited)).is_err());
        assert!(matches!(
            client.near_call(Some(bar)).unwrap(),
            Submit::Queued(_)
        ));
        assert!(idle(&client).failed.is_none());
        assert_eq!(lamp_near_call::read(&paths.near_call), Some(19.4));
        assert!(matches!(client.near_call(None).unwrap(), Submit::Queued(_)));
        idle(&client);
        assert!(!paths.near_call.exists());
        // Removing what is not there is no failure.
        client.near_call(None).unwrap();
        assert!(idle(&client).failed.is_none());
        owner.finish(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn failed_storage_and_oversized_mailboxes_never_publish_success() {
        let files = Files::new();
        let paths = files.paths();
        let mut owner = ServiceStorage::start(paths.clone()).unwrap();
        let client = owner.client();
        idle(&client);
        fs::write(&paths.mailbox, vec![b'x'; MAX_MAILBOX + 1]).unwrap();
        client.read_mailbox().unwrap();
        let state = idle(&client);
        assert!(!state.ready);
        assert!(state.failed.is_some());
        assert_eq!(
            client.commit(Progress::default(), false).unwrap(),
            Submit::Closed
        );
        fs::write(&paths.mailbox, "{\"redo\":true}\n").unwrap();
        assert!(matches!(client.read_mailbox().unwrap(), Submit::Queued(_)));
        let redo = idle(&client);
        assert!(!redo.ready);
        assert!(redo.failed.is_some());
        assert!(redo.mailbox.unwrap().1.contains("redo"));
        client.fence(Some(Progress::default())).unwrap();
        let recovered = idle(&client);
        assert!(recovered.ready);
        assert!(recovered.failed.is_none());
        owner.finish(Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn start_up_removes_temporaries_a_crash_left_mid_write() {
        let files = Files::new();
        let paths = files.paths();
        let mut leftover = paths.checkpoint.as_os_str().to_owned();
        leftover.push(".next-0123456789abcdef0123456789abcdef");
        let leftover = std::path::PathBuf::from(leftover);
        fs::write(&leftover, b"partial").unwrap();
        fs::write(paths.progress.with_extension("storage-next"), b"partial").unwrap();
        let mut owner = ServiceStorage::start(paths.clone()).unwrap();
        assert!(idle(&owner.client()).ready);
        assert!(!leftover.exists());
        assert!(!paths.progress.with_extension("storage-next").exists());
        owner.finish(Duration::from_secs(2)).unwrap();
    }
}
