//! One bounded storage executor per service. No caller waits for filesystem I/O.
use super::{Progress, privacy};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_PROGRESS: usize = 16 * 1024;
const MAX_MAILBOX: usize = 16 * 1024;
const MAX_LINES: usize = 128;

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
                while let Ok(job) = jobs.recv() {
                    let result = before(job.tag)
                        .and_then(|()| execute(&files, job.operation))
                        .map(|mut update| {
                            update.hardware_location = hardware_location.clone();
                            update
                        });
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
fn atomic_progress(paths: &Paths, progress: &Progress) -> io::Result<()> {
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

/// A yes for another part (room sounds for lamp-ears, face expressions for
/// lamp-eyes; `docs/onboarding.md`), from the progress just written or
/// read: removed unless the step's kept outcome is a yes (`done`); written,
/// when `new` (the write that first keeps the yes), whole as
/// `{"version":1,STEP:"yes","at_unix_s":..}`, 0640 and the reader group's
/// (0600 with no group). Never written again from the progress: a forget of
/// everything removes it for good (`remember.rs`). Only the answer and its
/// time, never words, sound or a face.
fn consent(paths: &Paths, progress: &Progress, step: &str, new: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let path = &consent_path(paths, step);
    let Some(at) = kept_yes(progress, step) else {
        return match fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    };
    if !new {
        return Ok(());
    }
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::other("a consent file has no folder"))?;
    let bytes = serde_json::to_vec(&serde_json::json!({"version":1,step:"yes","at_unix_s":at}))
        .map_err(io::Error::other)?;
    let next = path.with_extension("storage-next");
    let _ = fs::remove_file(&next);
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&next)?;
        // The voice's UMask=0077 would leave it 0600: set after create.
        let mode = if paths.room_sounds_group.is_some() {
            0o640
        } else {
            0o600
        };
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        if let Some(group) = paths.room_sounds_group {
            std::os::unix::fs::fchown(&file, None, Some(group))?;
        }
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&next, path)?;
        fs::File::open(folder)?.sync_all()
    })();
    // Not kept is no to the reader, the safe side; the progress stands.
    if let Err(error) = result {
        let _ = fs::remove_file(next);
        eprintln!(
            "lamp-voice: the {step} yes was not kept at {}: {error}",
            path.display()
        );
    }
    Ok(())
}
fn execute(paths: &Paths, operation: Operation) -> io::Result<Update> {
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
            update.progress = Some(progress);
            update.checkpoint = Some(None);
        }
        Operation::Commit(progress, erase) => {
            atomic_progress(paths, &progress)?;
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
            atomic_progress(paths, &progress)?;
            privacy::discard_checkpoint(&paths.checkpoint)?;
            progress.privacy_discard_pending = false;
            atomic_progress(paths, &progress)?;
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
        assert_eq!(
            fs::read(&paths.room_sounds).unwrap(),
            br#"{"at_unix_s":7,"room_sounds":"yes","version":1}"#
        );
        // The face expressions yes, kept the same way for lamp-eyes.
        let kept = fs::metadata(&face).unwrap();
        assert_eq!(kept.permissions().mode() & 0o777, 0o640);
        assert_eq!(kept.gid(), group);
        assert_eq!(
            fs::read(&face).unwrap(),
            br#"{"at_unix_s":7,"face_expressions":"yes","version":1}"#
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
            score: 0.98,
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
