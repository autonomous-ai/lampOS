//! Second-route private disclosure requires a V3 source AND its hold's kernel
//! witness. File age/audio ownership are independent; no legacy private fallback.
//! One off-path worker coalesces one current-hold grant job. Ticket polling
//! never connects/authenticates/handshakes. Permission checks still perform the
//! mandatory bounded-byte source-file validation; regular-file I/O latency is
//! not bounded by O_NONBLOCK or qualified without device measurements.
use crate::{
    memory_scope::ScopeLease,
    speaking_faces::{KnownSpeaker, KnownSpeakerEpoch, KnownSpeakerSource},
};
use lamp_ipc::camera_authority::{ReceiveGrant, Ticket, Token};
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    io::Errno,
};
use std::{
    fs,
    io::{self, Read, Write},
    net::Shutdown,
    num::NonZeroU64,
    os::unix::{ffi::OsStrExt, fs::MetadataExt, net::UnixStream},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

const GRANT_WITHIN: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(20);

pub(crate) struct Evidence {
    pub(crate) source: KnownSpeakerSource,
    publisher: [u8; 16],
    hold: u64,
    began_us: u64,
    expires: Instant,
    owner_uid: u32,
}
impl Evidence {
    fn token(&self) -> Option<Token> {
        Token::new(self.publisher, self.hold)
    }
}

/// Parse a single bounded source FD. V1/V2, unknown/mixed fields, future or old
/// captures and missing hold evidence are refused, never upgraded by read time.
pub(crate) fn read(known: &Path) -> Option<Evidence> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Source {
        version: u32,
        state: String,
        boot: String,
        publisher: String,
        generation: u64,
        camera_hold: u64,
        camera_began_us: u64,
        observed_us: u64,
        permission_until_us: u64,
        name: Option<String>,
        id: Option<String>,
        face: Option<String>,
    }
    let started = Instant::now();
    // NONBLOCK lets non-regular FIFO input be refused without waiting for a
    // writer; it does not promise nonblocking regular-file/filesystem latency.
    let fd = rustix::fs::open(
        crate::speaking_faces::known_speaker_of(known),
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .ok()?;
    let file = fs::File::from(fd);
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.mode() & 0o022 != 0 {
        return None;
    }
    let age = SystemTime::now()
        .duration_since(metadata.modified().ok()?)
        .ok()?;
    let file_expires = started.checked_add(crate::known_faces::FRESH.checked_sub(age)?)?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    // Presence is significant: even null identity fields on an identity-free
    // state are a malformed mixed envelope, not an optional missing value.
    // Deserialize the typed envelope DIRECTLY from bytes: going through Value
    // first would erase duplicate keys before serde can refuse them.
    let source: Source = serde_json::from_slice(&bytes).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let identity_fields = ["name", "id", "face"]
        .iter()
        .any(|field| value.get(field).is_some());
    // Instant is sampled BEFORE the cross-process monotonic read. Parsing or
    // scheduling afterward must consume, never restamp, either source bound.
    let checked = Instant::now();
    let now = lamp_ipc::monotonic_us();
    if source.version != 3
        || source.generation == 0
        || source.camera_hold == 0
        || source.camera_began_us == 0
        || source.camera_began_us > source.observed_us
        || source.observed_us > now
        || now - source.observed_us >= crate::known_faces::FRESH.as_micros().try_into().ok()?
        || now >= source.permission_until_us
        || source.permission_until_us - now > 1_500_000
        || !crate::memory_scope::qualified_speaker(&format!("{}/f1", source.boot))
        || source.publisher.len() != 32
    {
        return None;
    }
    let mut publisher = [0; 16];
    for (byte, pair) in publisher
        .iter_mut()
        .zip(source.publisher.as_bytes().chunks_exact(2))
    {
        let hex = |c| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        *byte = hex(pair[0])?.checked_mul(16)?.checked_add(hex(pair[1])?)?;
    }
    Token::new(publisher, source.camera_hold)?;
    let epoch = KnownSpeakerEpoch {
        boot: source.boot.clone(),
        publisher: source.publisher,
        generation: source.generation,
    };
    let parsed = match source.state.as_str() {
        "seen" => {
            let name = source.name?;
            let face = format!("{}/{}", source.boot, source.face?);
            if name.is_empty()
                || name.trim() != name
                || name.chars().count() > 40
                || name.chars().any(char::is_control)
                || !crate::memory_scope::qualified_speaker(&face)
                || source
                    .id
                    .as_deref()
                    .is_some_and(|id| !lamp_msgs::known::is_person_id(id))
            {
                return None;
            }
            KnownSpeakerSource::Seen(KnownSpeaker {
                person: crate::known_faces::person_of(
                    &name,
                    source.id.as_deref(),
                    lamp_msgs::known::stable_ids(),
                ),
                name,
                id: source.id,
                face,
                epoch: Some(epoch),
            })
        }
        "absent" | "checking" if !identity_fields => {
            if source.state == "absent" {
                KnownSpeakerSource::Absent(epoch)
            } else {
                KnownSpeakerSource::Checking(epoch)
            }
        }
        "revoked" if !identity_fields => KnownSpeakerSource::Revoked(epoch),
        _ => return None,
    };
    let capture_left =
        crate::known_faces::FRESH.checked_sub(Duration::from_micros(now - source.observed_us))?;
    let expires = file_expires
        .min(checked.checked_add(Duration::from_micros(source.permission_until_us - now))?)
        .min(checked.checked_add(capture_left)?);
    (Instant::now() < expires).then_some(Evidence {
        source: parsed,
        publisher,
        hold: source.camera_hold,
        began_us: source.camera_began_us,
        expires,
        owner_uid: metadata.uid(),
    })
}

enum Outcome {
    Pending,
    Ready(Arc<Ticket>),
    Lost,
}
struct Job {
    token: Token,
    publisher: [u8; 16],
    hold: u64,
    began_us: u64,
    until: Instant,
    cancelled: AtomicBool,
    outcome: Mutex<Outcome>,
}
struct Queue {
    current: Option<Arc<Job>>,
    pending: Option<Arc<Job>>,
    stop: bool,
}
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    stop: AtomicBool,
}
/// Startup-owned bounded client. Grant acquisition/socket path and peer checks
/// run off the interaction path. Startup reads its configured parent anchor;
/// permission checks independently retain regular source-file validation.
pub(crate) struct Client {
    known: PathBuf,
    anchor: Anchor,
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Client {
    pub(crate) fn start(known: PathBuf) -> io::Result<Arc<Self>> {
        let anchor = parent_anchor(&known).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "untrusted known source parent",
            )
        })?;
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                current: None,
                pending: None,
                stop: false,
            }),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let work = shared.clone();
        let path = known.clone();
        let expected = anchor;
        let worker = thread::Builder::new()
            .name("camera-ticket".into())
            .spawn(move || run(&path, expected, &work))?;
        Ok(Arc::new(Self {
            known,
            anchor,
            shared,
            worker: Mutex::new(Some(worker)),
        }))
    }
    fn request(&self, evidence: &Evidence, proof_until: Instant) -> Option<Arc<Job>> {
        let token = evidence.token()?;
        // Inspect a cached candidate outside the queue mutex. Only a pending
        // acquisition inside its original deadline or a successfully open FD
        // may be shared; a failed request must not poison later requests.
        let previous = {
            let queue = self.shared.queue.try_lock().ok()?;
            if queue.stop {
                return None;
            }
            queue.current.clone()
        };
        let reusable = if let Some(current) = previous.as_ref().filter(|job| job.token == token) {
            if current.began_us != evidence.began_us {
                return None;
            }
            let ticket = {
                let outcome = current.outcome.try_lock().ok()?;
                match &*outcome {
                    Outcome::Pending
                        if Instant::now() < current.until
                            && !current.cancelled.load(Ordering::Acquire) =>
                    {
                        return Some(current.clone());
                    }
                    Outcome::Pending => None,
                    Outcome::Ready(ticket) => Some(ticket.clone()),
                    Outcome::Lost => None,
                }
            };
            ticket.is_some_and(|ticket| ticket.is_open())
                && !current.cancelled.load(Ordering::Acquire)
        } else {
            false
        };
        let mut queue = self.shared.queue.try_lock().ok()?;
        if queue.stop
            || !match (&previous, &queue.current) {
                (Some(was), Some(now)) => Arc::ptr_eq(was, now),
                (None, None) => true,
                _ => false,
            }
        {
            return None;
        }
        if reusable {
            return previous;
        }
        let until = (Instant::now() + GRANT_WITHIN)
            .min(evidence.expires)
            .min(proof_until);
        if Instant::now() >= until {
            return None;
        }
        let job = Arc::new(Job {
            token,
            publisher: evidence.publisher,
            hold: evidence.hold,
            began_us: evidence.began_us,
            until,
            cancelled: AtomicBool::new(false),
            outcome: Mutex::new(Outcome::Pending),
        });
        let old_current = queue.current.replace(job.clone());
        let old_pending = queue.pending.replace(job.clone());
        if let Some(old) = &old_current {
            old.cancelled.store(true, Ordering::Release);
        }
        self.shared.wake.notify_one();
        drop(queue);
        drop(old_current);
        drop(old_pending);
        Some(job)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.stop = true;
            if let Some(job) = &queue.current {
                job.cancelled.store(true, Ordering::Release);
            }
            self.shared.wake.notify_one();
        }
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}

/// One immutable hold acquisition for all clones of ONE accepted request.
/// The cache can share an FD, never its source identity or audio permission.
pub(crate) struct RequestBinding {
    subject: KnownSpeaker,
    token: Token,
    began_us: u64,
    owner_uid: u32,
    job: Arc<Job>,
}
impl RequestBinding {
    pub(crate) fn state(&self, evidence: &Evidence) -> crate::second_provider::PermissionState {
        use crate::second_provider::PermissionState;
        if evidence.token() != Some(self.token)
            || evidence.began_us != self.began_us
            || self.owner_uid != evidence.owner_uid
            || Instant::now() >= evidence.expires
            || self.job.cancelled.load(Ordering::Acquire)
        {
            return PermissionState::Lost;
        }
        let same_subject = match &evidence.source {
            KnownSpeakerSource::Seen(known) => {
                known.epoch == self.subject.epoch
                    && known.name == self.subject.name
                    && known.person == self.subject.person
                    && known.id == self.subject.id
            }
            KnownSpeakerSource::Absent(epoch) | KnownSpeakerSource::Checking(epoch) => {
                self.subject.epoch.as_ref() == Some(epoch)
            }
            KnownSpeakerSource::Revoked(_) => false,
        };
        if !same_subject {
            return PermissionState::Lost;
        }
        let Ok(mut outcome) = self.job.outcome.try_lock() else {
            return PermissionState::Pending;
        };
        match &*outcome {
            Outcome::Pending
                if Instant::now() < self.job.until
                    && !self.job.cancelled.load(Ordering::Acquire) =>
            {
                PermissionState::Pending
            }
            Outcome::Ready(ticket) if ticket.is_open() => PermissionState::Ready,
            _ => {
                *outcome = Outcome::Lost;
                PermissionState::Lost
            }
        }
    }
}
pub(crate) fn permission(
    known: &Path,
    lease: &ScopeLease,
    evidence: &Evidence,
) -> crate::second_provider::PermissionState {
    use crate::second_provider::PermissionState;
    if !lease.is_current() || lease.speaker().is_none() {
        return PermissionState::Lost;
    }
    if let Some(binding) = lease.camera_binding() {
        return binding.state(evidence);
    }
    let Some(client) = lease.camera_client().filter(|client| client.known == known) else {
        return PermissionState::Lost;
    };
    let KnownSpeakerSource::Seen(subject) = &evidence.source else {
        return PermissionState::Lost;
    };
    if evidence.owner_uid != client.anchor.uid
        || lease.speaker() != Some(subject.face.as_str())
        || lease.scope().and_then(|scope| scope.person()) != Some(subject.person.as_str())
    {
        return PermissionState::Lost;
    }
    let Some(job) = lease
        .speaker_deadline()
        .and_then(|until| client.request(evidence, until))
    else {
        return PermissionState::Pending;
    };
    let Some(token) = evidence.token() else {
        return PermissionState::Lost;
    };
    lease
        .pin_camera_binding(RequestBinding {
            subject: subject.clone(),
            token,
            began_us: evidence.began_us,
            owner_uid: client.anchor.uid,
            job,
        })
        .state(evidence)
}

fn run(known: &Path, anchor: Anchor, shared: &Shared) {
    loop {
        let job = {
            let Ok(mut queue) = shared.queue.lock() else {
                return;
            };
            while queue.pending.is_none() && !queue.stop {
                queue = match shared.wake.wait(queue) {
                    Ok(queue) => queue,
                    Err(_) => return,
                };
            }
            if queue.stop {
                return;
            }
            queue.pending.take()
        };
        let Some(job) = job else {
            continue;
        };
        let ticket = exchange(known, anchor, &job, shared);
        if let Ok(mut outcome) = job.outcome.lock() {
            // A boundary that already observed expiry/loss wins against a
            // delayed completion; neither worker scheduling nor Ready renews it.
            if matches!(&*outcome, Outcome::Pending) {
                *outcome = match ticket {
                    Some(ticket) if current(&job, shared) && ticket.is_open() => {
                        Outcome::Ready(Arc::new(ticket))
                    }
                    _ => Outcome::Lost,
                };
            }
        }
    }
}
fn current(job: &Job, shared: &Shared) -> bool {
    Instant::now() < job.until
        && !job.cancelled.load(Ordering::Acquire)
        && !shared.stop.load(Ordering::Acquire)
}
fn wait(stream: &UnixStream, interest: PollFlags, job: &Job, shared: &Shared) -> Option<()> {
    while current(job, shared) {
        let left = job.until.checked_duration_since(Instant::now())?.min(POLL);
        let timeout = Timespec {
            tv_sec: left.as_secs().try_into().ok()?,
            tv_nsec: left.subsec_nanos().into(),
        };
        let mut fds = [PollFd::new(stream, interest)];
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) => continue,
            Ok(_)
                if fds[0]
                    .revents()
                    .intersects(interest | PollFlags::HUP | PollFlags::ERR) =>
            {
                return Some(());
            }
            Err(Errno::INTR) => continue,
            _ => return None,
        }
    }
    None
}

fn exchange(known: &Path, anchor: Anchor, job: &Job, shared: &Shared) -> Option<Ticket> {
    let (path, uid) = trusted_path(known)?;
    if parent_anchor(known)? != anchor || uid != anchor.uid {
        return None;
    }
    if !current(job, shared) {
        return None;
    }
    let fd = rustix::net::socket(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        None,
    )
    .ok()?;
    let stream = UnixStream::from(fd);
    rustix::io::fcntl_setfd(&stream, rustix::io::FdFlags::CLOEXEC).ok()?;
    stream.set_nonblocking(true).ok()?;
    #[cfg(target_os = "macos")]
    rustix::net::sockopt::set_socket_nosigpipe(&stream, true).ok()?;
    let address = rustix::net::SocketAddrUnix::new(&path).ok()?;
    match rustix::net::connect(&stream, &address) {
        Ok(()) => {}
        Err(Errno::INPROGRESS) => {
            wait(&stream, PollFlags::OUT, job, shared)?;
            if stream.take_error().ok()?.is_some() {
                return None;
            }
        }
        _ => return None,
    }
    if !current(job, shared) {
        return None;
    }
    authenticate_peer(&stream, uid).ok()?;
    let mut request = [0; 48];
    request[..8].copy_from_slice(b"LMPASK03");
    // Immutable bytes captured with the exact validated token, never model data.
    let (publisher, hold) = (job.publisher, job.hold);
    request[8..24].copy_from_slice(&publisher);
    request[24..32].copy_from_slice(&hold.to_be_bytes());
    static NONCE: AtomicU64 = AtomicU64::new(1);
    let nonce = NONCE
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
        .ok()
        .and_then(NonZeroU64::new)?;
    request[32..40].copy_from_slice(&nonce.get().to_be_bytes());
    let mut sent = 0;
    while sent < request.len() && current(job, shared) {
        match (&stream).write(&request[sent..]) {
            Ok(0) => return None,
            Ok(n) => sent += n,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait(&stream, PollFlags::OUT, job, shared)?
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    if sent != request.len() || !current(job, shared) {
        return None;
    }
    stream.shutdown(Shutdown::Write).ok()?;
    let mut receive = ReceiveGrant::new(job.token, nonce, job.until);
    while current(job, shared) {
        match receive.try_receive(&stream) {
            Ok(Some(ticket)) => return ticket.is_open().then_some(ticket),
            Ok(None) => wait(&stream, PollFlags::IN, job, shared)?,
            Err(_) => return None,
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Anchor {
    uid: u32,
    dev: u64,
    ino: u64,
}
fn parent_anchor(known: &Path) -> Option<Anchor> {
    if !known.is_absolute()
        || known
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return None;
    }
    let parent = known.parent()?;
    for ancestor in parent.ancestors() {
        if fs::symlink_metadata(ancestor)
            .ok()?
            .file_type()
            .is_symlink()
        {
            return None;
        }
    }
    let owner = fs::symlink_metadata(parent).ok()?;
    if !owner.is_dir() || owner.mode() & 0o022 != 0 {
        return None;
    }
    Some(Anchor {
        uid: owner.uid(),
        dev: owner.dev(),
        ino: owner.ino(),
    })
}
fn trusted_path(known: &Path) -> Option<(PathBuf, u32)> {
    let owner = parent_anchor(known)?;
    let parent = known.parent()?;
    let mut name = known.file_name()?.to_os_string();
    name.push(".authority.d");
    let directory = parent.join(name);
    let metadata = fs::symlink_metadata(&directory).ok()?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != owner.uid
        || metadata.mode() & 0o777 != 0o700
    {
        return None;
    }
    let socket = directory.join("hold.sock");
    let metadata = fs::symlink_metadata(&socket).ok()?;
    use std::os::unix::fs::FileTypeExt;
    if !metadata.file_type().is_socket()
        || metadata.uid() != owner.uid
        || metadata.mode() & 0o777 != 0o600
        || socket.as_os_str().as_bytes().len() >= 100
    {
        return None;
    }
    Some((socket, owner.uid))
}
/// Authenticate the connected kernel peer against the already trusted parent
/// anchor BEFORE constructing/sending any grant request or receiving rights.
fn authenticate_peer(stream: &UnixStream, expected_uid: u32) -> io::Result<()> {
    if peer_uid(stream)? != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "camera authority peer UID mismatch",
        ));
    }
    Ok(())
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    #[cfg(target_os = "linux")]
    {
        Ok(rustix::net::sockopt::socket_peercred(stream)?.uid.as_raw())
    }
    #[cfg(target_os = "macos")]
    {
        nix::unistd::getpeereid(stream)
            .map(|(uid, _)| uid.as_raw())
            .map_err(Into::into)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = stream;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "camera ticket peer credentials",
        ))
    }
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
