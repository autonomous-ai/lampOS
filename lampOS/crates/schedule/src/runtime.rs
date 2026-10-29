//! Bounded local transport. Disk work has one owner; socket readers and bus
//! heartbeats remain independent of a slow client or a blocked storage commit.
use crate::{
    engine::{Engine, Error as EngineError},
    protocol::{MAX_LINE_BYTES, MAX_REPLY_BYTES, Request, refusal},
    store::Store,
    time::ClockSample,
};
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{ClockStamp, Diagnostic, Level, ScheduleDue};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const TICK: Duration = Duration::from_millis(50);
pub const READ_DEADLINE: Duration = Duration::from_millis(300);
pub const REQUEST_DEADLINE: Duration = Duration::from_millis(1100);
pub const CLIENT_DEADLINE: Duration = Duration::from_millis(1200);
pub const WORKERS: usize = 4;
pub const CLIENT_QUEUE: usize = 4;
pub const REQUEST_QUEUE: usize = 16;
pub const STALE_AFTER: Duration = Duration::from_millis(1500);
/// How often the part says how it is on `diagnostics` and `schedule/due`
/// while nothing changes ([`Config::report_every`] on a lamp): the period
/// `schedule/due` declares (`ScheduleDue::declaration`), which a longer one
/// would make late.
pub const REPORT_EVERY: Duration = Duration::from_secs(1);

pub trait Clock: Send + 'static {
    fn sample(&self) -> io::Result<ClockSample>;
}

pub struct SystemClock {
    boot_id: String,
    trust_clock: bool,
    /// Also trust the wall clock when the kernel says a time client has set
    /// it (`--kernel-clock`, #830): a board kept by chrony or ntpd has no
    /// timesyncd marker.
    kernel_clock: bool,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

impl SystemClock {
    pub fn new(boot_id: Option<&str>, trust_clock: bool) -> io::Result<Self> {
        let boot_id = match boot_id {
            Some(id) => id.into(),
            None if cfg!(target_os = "linux") => {
                fs::read_to_string("/proc/sys/kernel/random/boot_id")?
                    .trim()
                    .into()
            }
            None => return Err(invalid("supply --boot-id on this development host")),
        };
        let clock = Self {
            boot_id,
            trust_clock,
            kernel_clock: false,
        };
        clock
            .sample()?
            .check()
            .map_err(|_| invalid("invalid clock identity or sample"))?;
        Ok(clock)
    }

    /// Trusts the wall clock also when the kernel says it is set.
    pub fn with_kernel_clock(mut self, on: bool) -> Self {
        self.kernel_clock = on;
        self
    }
}

impl Clock for SystemClock {
    fn sample(&self) -> io::Result<ClockSample> {
        #[cfg(target_os = "linux")]
        let id = rustix::time::ClockId::Boottime;
        #[cfg(not(target_os = "linux"))]
        let id = rustix::time::ClockId::Monotonic;
        let boot = rustix::time::clock_gettime(id);
        let seconds = u64::try_from(boot.tv_sec).map_err(|_| invalid("negative boot clock"))?;
        let nanos = u64::try_from(boot.tv_nsec).map_err(|_| invalid("negative boot clock"))?;
        let boot_ms = seconds
            .checked_mul(1000)
            .and_then(|s| s.checked_add(nanos / 1_000_000))
            .ok_or_else(|| invalid("boot clock overflow"))?;
        let unix_ms = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_millis(),
        )
        .map_err(|_| invalid("wall clock overflow"))?;
        // This explicit operator assertion also makes host qualification possible.
        // The automatic Linux boundary matches Health's timesyncd evidence; it
        // cannot establish the accuracy or authenticity of upstream NTP.
        let wall_trusted = self.trust_clock
            || (cfg!(target_os = "linux")
                && Path::new("/run/systemd/timesync/synchronized").is_file())
            || (self.kernel_clock && lamp_ipc::clock_synced() == Some(true));
        Ok(ClockSample {
            unix_ms,
            boot_ms,
            boot_id: self.boot_id.clone(),
            wall_trusted,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub enabled: bool,
    pub dir: PathBuf,
    pub socket: PathBuf,
    pub bus_root: PathBuf,
    pub zone: String,
    /// The lamp's UTC offset east when setup ran (`LAMP_UTC_OFFSET_MINUTES`
    /// in `zone.env`): the fallback when `zone` is not one this build knows
    /// (#1680).
    pub utc_offset_minutes: Option<i32>,
    pub run_for: Option<Duration>,
    /// How often the part says how it is while nothing changes:
    /// [`REPORT_EVERY`] on a lamp. A new revision or a change in readiness
    /// is said at once, whatever this is. Tests pass less, so a test that
    /// reads the report waits a fraction of a second rather than one (#1806).
    pub report_every: Duration,
    /// How long one request may take, from its accept to its answer:
    /// [`REQUEST_DEADLINE`] on a lamp, after which it is answered
    /// `uncertain`. A host test against this service on a loaded machine
    /// passes more, since one durable commit can take past 1100 ms there
    /// (lamp-voice's push runs on 29 Oct).
    pub request_deadline: Duration,
}

#[derive(Clone)]
struct Report {
    updated: Instant,
    sample: ClockSample,
    due: ScheduleDue,
    items: usize,
    /// Stored reminders this build can't read, kept and not sounded (#1680).
    unread: usize,
    /// The zone asked for and the one used, when this build doesn't know
    /// the one asked for (#1680).
    zone_fallback: Option<(String, String)>,
    requests: u64,
}

struct Work {
    request: Request,
    deadline: Instant,
    response: SyncSender<Result<Value, EngineError>>,
}

struct Binding {
    path: PathBuf,
    dev: u64,
    ino: u64,
    _lock: File,
}

struct StopOnDrop(Arc<AtomicBool>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl Drop for Binding {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.file_type().is_socket() && m.dev() == self.dev && m.ino() == self.ino)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn bind(path: &Path) -> io::Result<(UnixListener, Binding)> {
    if !path.is_absolute() || path.as_os_str().len() > lamp_bus::MOST_SOCKET_PATH_BYTES {
        return Err(invalid(
            "socket path must be absolute and at most 103 bytes",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("socket needs a parent"))?;
    if !parent.exists() {
        fs::DirBuilder::new().mode(0o700).create(parent)?;
    }
    let parent_meta = fs::symlink_metadata(parent)?;
    if !parent_meta.is_dir()
        || parent_meta.permissions().mode() & 0o077 != 0
        || parent_meta.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(invalid(
            "socket parent must be a private directory owned by this user",
        ));
    }
    let lock_path = path.with_extension("lock");
    if let Ok(m) = fs::symlink_metadata(&lock_path)
        && (!m.is_file()
            || m.nlink() != 1
            || m.uid() != parent_meta.uid()
            || m.permissions().mode() & 0o077 != 0)
    {
        return Err(invalid("unsafe socket lock"));
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(lock_path)?;
    lock.try_lock().map_err(io::Error::other)?;
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_socket() && m.uid() == parent_meta.uid() => {
            // A lock protects cooperating owners; a live non-cooperating socket
            // is preserved too. Private parents exclude other users' races.
            let descriptor = rustix::net::socket(
                rustix::net::AddressFamily::UNIX,
                rustix::net::SocketType::STREAM,
                None,
            )?;
            rustix::io::fcntl_setfd(&descriptor, rustix::io::FdFlags::CLOEXEC)?;
            let probe = UnixStream::from(descriptor);
            probe.set_nonblocking(true)?;
            let address = rustix::net::SocketAddrUnix::new(path)?;
            match rustix::net::connect(&probe, &address).map_err(io::Error::from) {
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "socket already answers",
                    ));
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    fs::remove_file(path)?
                }
                Err(e) => return Err(e),
            }
        }
        Ok(_) => return Err(invalid("existing socket path is not an owned socket")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let metadata = fs::symlink_metadata(path)?;
    Ok((
        listener,
        Binding {
            path: path.into(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            _lock: lock,
        },
    ))
}

fn stamp(sample: &ClockSample, sequence: u64) -> io::Result<ClockStamp> {
    // Bus stamps use the shared MONOTONIC clock, as other lamp topics do.
    // Linux elapsed timers separately use BOOTTIME so suspend counts there.
    let tick = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let seconds = u64::try_from(tick.tv_sec).map_err(|_| invalid("negative monotonic clock"))?;
    let nanos = u64::try_from(tick.tv_nsec).map_err(|_| invalid("negative monotonic clock"))?;
    let at_us = seconds
        .checked_mul(1_000_000)
        .and_then(|s| s.checked_add(nanos / 1000))
        .ok_or_else(|| invalid("monotonic timestamp overflow"))?;
    Ok(ClockStamp {
        clock_id: sample.boot_id.clone(),
        at_us,
        sequence,
    })
}

fn report<P: crate::engine::Persistence>(
    engine: &Engine<P>,
    sample: ClockSample,
    requests: u64,
) -> io::Result<Report> {
    Ok(Report {
        updated: Instant::now(),
        due: engine.due(&sample, stamp(&sample, 0)?, true),
        items: engine.counts(&sample).0,
        unread: engine.unread(),
        zone_fallback: engine
            .zone_fallback()
            .map(|(asked, zone)| (asked.to_owned(), zone.to_owned())),
        sample,
        requests,
    })
}

fn work<C: Clock>(
    mut engine: Engine<Store>,
    clock: C,
    incoming: Receiver<Work>,
    current: Arc<Mutex<Report>>,
    stop: Arc<AtomicBool>,
) -> io::Result<()> {
    let mut requests = 0u64;
    while !stop.load(Ordering::Acquire) {
        match incoming.recv_timeout(TICK) {
            Ok(work) => {
                if Instant::now() >= work.deadline {
                    let _ = work.response.try_send(Err(EngineError {
                        code: "timeout",
                        message: "The queued request expired before it started.",
                    }));
                } else {
                    let now = clock.sample()?;
                    let result = engine.answer(work.request, &now);
                    requests = requests.saturating_add(1);
                    let failed = engine.failed();
                    let _ = work.response.try_send(result);
                    if failed {
                        return Err(io::Error::other(
                            "Schedule storage failed; restart required",
                        ));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let now = clock.sample()?;
        engine.tick(&now).map_err(io::Error::other)?;
        let next = report(&engine, now, requests)?;
        *current
            .lock()
            .map_err(|_| io::Error::other("Schedule report lock poisoned"))? = next;
    }
    Ok(())
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "client deadline"))
}

fn read_line(stream: &mut UnixStream, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut line = Vec::with_capacity(512);
    let mut buffer = [0u8; 512];
    loop {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        match stream.read(&mut buffer) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "request needs a newline",
                ));
            }
            Ok(n) => {
                let end = buffer[..n].iter().position(|b| *b == b'\n');
                let part = &buffer[..end.unwrap_or(n)];
                if line.len() + part.len() > MAX_LINE_BYTES {
                    return Err(invalid("too_long"));
                }
                line.extend_from_slice(part);
                if end.is_some() {
                    return Ok(line);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

fn write_line(stream: &mut UnixStream, value: &Value, deadline: Instant) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_REPLY_BYTES {
        bytes = serde_json::to_vec(&refusal(
            "too_long",
            "The reply exceeded its bound; request a smaller page.",
        ))?;
    }
    bytes.push(b'\n');
    let mut written = 0;
    while written < bytes.len() {
        stream.set_write_timeout(Some(remaining(deadline)?))?;
        match stream.write(&bytes[written..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "client stopped reading",
                ));
            }
            Ok(n) => written += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn serve_client(
    mut stream: UnixStream,
    accepted: Instant,
    request_deadline: Duration,
    owner: &SyncSender<Work>,
) {
    // Darwin accepts inherit the listener's nonblocking flag. A socket timeout
    // alone does not clear it: fragmented valid lines would fail immediately.
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let answer = match read_line(&mut stream, accepted + READ_DEADLINE) {
        Ok(line) => match Request::parse(&line) {
            Ok(request) => {
                let (tx, rx) = mpsc::sync_channel(1);
                let deadline = accepted + request_deadline;
                let queued = Work {
                    request,
                    deadline,
                    response: tx,
                };
                match owner.try_send(queued) {
                    Ok(()) => match remaining(deadline)
                        .and_then(|left| rx.recv_timeout(left).map_err(io::Error::other))
                    {
                        Ok(Ok(reply)) => reply,
                        Ok(Err(error)) => refusal(error.code, error.message),
                        Err(_) => refusal(
                            "uncertain",
                            "No receipt arrived. Read status and retry only the exact guarded request; it may have committed.",
                        ),
                    },
                    Err(TrySendError::Full(_)) => refusal(
                        "busy",
                        "Schedule's request queue is full; this request did not start.",
                    ),
                    Err(TrySendError::Disconnected(_)) => refusal(
                        "unavailable",
                        "Schedule's store is unavailable; this request did not start.",
                    ),
                }
            }
            Err(code) => refusal(
                code,
                "Send one bounded JSON object with a recognized operation and fields.",
            ),
        },
        Err(e) if e.kind() == io::ErrorKind::InvalidInput => {
            refusal("too_long", "Request exceeds 8192 bytes.")
        }
        Err(_) => refusal(
            "incomplete",
            "A complete request line did not arrive within 300 ms.",
        ),
    };
    let _ = write_line(&mut stream, &answer, accepted + CLIENT_DEADLINE);
}

fn publish(out: &mut Publisher, at_us: u64, value: &impl serde::Serialize, errors: &mut u64) {
    if out.publish_json(at_us, value).is_err() {
        *errors = errors.saturating_add(1);
    }
}

/// With `enabled=false`, return before reading the clock or touching a file,
/// socket, bus topic or worker. Signal setup belongs to the enabled CLI path.
pub fn serve<C: Clock>(config: Config, clock: C, stop: Arc<AtomicBool>) -> io::Result<()> {
    if !config.enabled {
        return Ok(());
    }
    let started = Instant::now();
    let sample = clock.sample()?;
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let id = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let store = Store::open(&config.dir)?;
    let mut engine =
        Engine::open_with_offset(store, &config.zone, config.utc_offset_minutes, &sample, &id)
            .map_err(io::Error::other)?;
    engine.tick(&sample).map_err(io::Error::other)?;
    let current = Arc::new(Mutex::new(report(&engine, sample, 0)?));
    let (listener, _binding) = bind(&config.socket)?;
    let bus = Bus::new(&config.bus_root)?;
    let mut due =
        bus.publisher_declared(ScheduleDue::TOPIC, &ScheduleDue::declaration(), "schedule")?;
    let mut diagnostic =
        bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), "schedule")?;
    let (owner_tx, owner_rx) = mpsc::sync_channel(REQUEST_QUEUE);
    let _stop_on_drop = StopOnDrop(stop.clone());
    let owner_current = current.clone();
    let owner_stop = stop.clone();
    let owner = thread::Builder::new()
        .name("schedule-store".into())
        .spawn(move || work(engine, clock, owner_rx, owner_current, owner_stop))?;
    let (clients_tx, clients_rx) = mpsc::sync_channel::<(UnixStream, Instant)>(CLIENT_QUEUE);
    let clients_rx = Arc::new(Mutex::new(clients_rx));
    let rejected = Arc::new(AtomicU64::new(0));
    let mut workers = Vec::new();
    let request_deadline = config.request_deadline;
    for number in 0..WORKERS {
        let incoming = clients_rx.clone();
        let writer = owner_tx.clone();
        let stopped = stop.clone();
        workers.push(
            thread::Builder::new()
                .name(format!("schedule-client-{number}"))
                .spawn(move || {
                    while !stopped.load(Ordering::Acquire) {
                        let received = incoming.lock().ok().and_then(|r| r.try_recv().ok());
                        if let Some((client, accepted)) = received {
                            serve_client(client, accepted, request_deadline, &writer);
                        } else {
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
                })?,
        );
    }
    let mut sequence = 0u64;
    let mut last_publish = None;
    let mut last_revision = None;
    let mut was_ready = None;
    let mut publish_errors = 0u64;
    let mut failure = None;
    while !stop.load(Ordering::Acquire)
        && config.run_for.is_none_or(|limit| started.elapsed() < limit)
    {
        for _ in 0..(WORKERS + CLIENT_QUEUE) {
            match listener.accept() {
                Ok((client, _)) => {
                    if clients_tx.try_send((client, Instant::now())).is_err() {
                        rejected.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
        if let Ok(locked) = current.try_lock() {
            let report = locked.clone();
            drop(locked);
            let ended = owner.is_finished();
            let ready = report.updated.elapsed() <= STALE_AFTER && !ended;
            // A worker that ended for a stop asked for is not reported as
            // one that stopped progressing (#869).
            let stopped_as_asked = ended && stop.load(Ordering::Acquire);
            let should_publish = !stopped_as_asked
                && (last_publish.is_none_or(|last: Instant| last.elapsed() >= config.report_every)
                    || last_revision != Some(report.due.revision)
                    || was_ready != Some(ready));
            if should_publish {
                sequence = sequence.saturating_add(1);
                let mut state = report.due;
                state.stamp.sequence = sequence;
                state.ready = ready;
                if !ready {
                    state.oldest = None;
                }
                let at_us = state.stamp.at_us;
                publish(&mut due, at_us, &state, &mut publish_errors);
                // A folder that can't be listed says nothing rather than
                // stopping the part.
                let set_aside = crate::store::kept_aside(&config.dir).ok().flatten();
                let (level, message) = if !ready {
                    (Level::Error, "Schedule storage worker is not progressing")
                } else if publish_errors > 0 {
                    (Level::Warn, "Schedule has bus publication failures")
                } else if set_aside.is_some() {
                    (
                        Level::Warn,
                        "Schedule set aside a store from a newer build; its reminders will not sound",
                    )
                } else if report.unread > 0 {
                    (
                        Level::Warn,
                        "Schedule kept stored reminders from a newer build that this build can't read; they will not sound",
                    )
                } else if report.zone_fallback.is_some() {
                    (
                        Level::Warn,
                        "Schedule doesn't know the lamp's zone; a new reminder is set in the fallback zone's time",
                    )
                } else if state.waiting_for_clock > 0 {
                    (
                        Level::Warn,
                        "Some reminders are waiting for a trusted wall clock",
                    )
                } else {
                    (Level::Ok, "")
                };
                let report_out = Diagnostic::new(level, "schedule", message)
                    .with("items", report.items)
                    .with("pending", state.pending)
                    .with("waiting_for_clock", state.waiting_for_clock)
                    .with("requests", report.requests)
                    .with("wall_trusted", report.sample.wall_trusted)
                    .with("store_ready", ready)
                    .with("publish_errors", publish_errors)
                    .with("clients_refused", rejected.load(Ordering::Relaxed));
                let mut report_out = match set_aside.as_deref() {
                    Some(name) => report_out.with("set_aside", name),
                    None => report_out,
                };
                if report.unread > 0 {
                    report_out = report_out.with("unread", report.unread);
                }
                if let Some((asked, zone)) = &report.zone_fallback {
                    report_out = report_out.with("zone_asked", asked).with("zone", zone);
                }
                publish(&mut diagnostic, at_us, &report_out, &mut publish_errors);
                last_revision = Some(state.revision);
                was_ready = Some(ready);
                last_publish = Some(Instant::now());
            }
        }
        // The store worker watches the same stop flag, so once a stop is
        // asked for it may end at any moment, even within this pass: that is
        // the stop asked for. The worker is looked at first and the flag
        // after, so a worker found ended while no stop had been asked for
        // even then has failed. One that ended after a stop has stopped, and
        // its own result is still read when it is joined below (#869).
        #[cfg(test)]
        tests::before_owner_check();
        if owner.is_finished() {
            if !stop.load(Ordering::Acquire) {
                failure = Some(io::Error::other("Schedule store worker stopped"));
            }
            break;
        }
        if failure.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    stop.store(true, Ordering::Release);
    drop(clients_tx);
    drop(owner_tx);
    // Client threads have cumulative deadlines. A storage system call can be
    // unbounded; do not join an unfinished storage thread during shutdown.
    for worker in workers {
        let _ = worker.join();
    }
    let shutdown = Instant::now();
    while !owner.is_finished() && shutdown.elapsed() < Duration::from_millis(200) {
        thread::sleep(Duration::from_millis(5));
    }
    if owner.is_finished() {
        match owner.join() {
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(io::Error::other("Schedule store worker panicked")),
            Ok(Ok(())) => {}
        }
    } else {
        return Err(io::Error::other(
            "Schedule storage worker did not stop; exit this process before restarting",
        ));
    }
    if let Some(e) = failure {
        Err(e)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        /// Runs in the service's loop just before it looks at its store
        /// worker, in the thread that set it only.
        static BEFORE_OWNER_CHECK: RefCell<Option<Box<dyn FnMut()>>> = RefCell::new(None);
    }

    pub(super) fn before_owner_check() {
        BEFORE_OWNER_CHECK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().as_mut() {
                hook();
            }
        });
    }

    /// A clock that fails once `failing` is set.
    #[derive(Clone)]
    struct Test {
        failing: Arc<AtomicBool>,
    }
    impl Clock for Test {
        fn sample(&self) -> io::Result<ClockSample> {
            if self.failing.load(Ordering::Acquire) {
                return Err(io::Error::other("test clock failed"));
            }
            Ok(ClockSample {
                unix_ms: 1_791_964_800_000,
                boot_ms: 1_000,
                boot_id: "test-boot".into(),
                wall_trusted: true,
            })
        }
    }

    /// The service in its own thread with `hook` before each look at its
    /// store worker, and its result.
    fn serve_with(
        clock: Test,
        stop: Arc<AtomicBool>,
        hook: impl FnMut() + Send + 'static,
    ) -> io::Result<()> {
        let root = tempfile::Builder::new()
            .prefix("sch-")
            .tempdir_in("/private/tmp")
            .or_else(|_| tempfile::Builder::new().prefix("sch-").tempdir())
            .unwrap();
        let config = Config {
            enabled: true,
            dir: root.path().join("data"),
            socket: root.path().join("run/s.sock"),
            bus_root: root.path().join("b"),
            zone: "Etc/UTC".into(),
            utc_offset_minutes: None,
            run_for: Some(Duration::from_secs(5)),
            report_every: REPORT_EVERY,
            request_deadline: REQUEST_DEADLINE,
        };
        let result = thread::spawn(move || {
            BEFORE_OWNER_CHECK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
            serve(config, clock, stop)
        })
        .join()
        .unwrap();
        drop(root);
        result
    }

    #[test]
    fn a_stop_asked_for_while_the_store_worker_ends_within_a_pass_is_a_clean_stop() {
        // The stop comes after the loop's own look at the flag, and the
        // worker ends on it before the loop looks at the worker: #869's run.
        let stop = Arc::new(AtomicBool::new(false));
        let asked = stop.clone();
        let mut passes = 0;
        let clock = Test {
            failing: Arc::new(AtomicBool::new(false)),
        };
        let result = serve_with(clock, stop, move || {
            passes += 1;
            if passes == 3 {
                asked.store(true, Ordering::Release);
                // Longer than the worker's tick: it has seen the stop.
                thread::sleep(TICK * 3);
            }
        });
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn a_store_worker_that_fails_with_no_stop_asked_for_is_still_a_failure() {
        let failing = Arc::new(AtomicBool::new(false));
        let fail = failing.clone();
        let mut passes = 0;
        let result = serve_with(
            Test { failing },
            Arc::new(AtomicBool::new(false)),
            move || {
                passes += 1;
                if passes == 3 {
                    fail.store(true, Ordering::Release);
                    thread::sleep(TICK * 3);
                }
            },
        );
        let error = result.expect_err("a failed worker is a failure");
        assert!(error.to_string().contains("test clock failed"), "{error}");
    }
}
