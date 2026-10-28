//! Bounded local sockets, one durable owner and one independent Harness worker.
use crate::{
    engine::{
        Clock as ClockSample, CloseJob, Engine, Error as EngineError, LaunchJob, ObservationJob,
        Watched,
    },
    harness::{
        AccountMachine, Cli, CliConfig, Closed, Ended, EngineKind, Launch, LaunchOutcome, Listed,
        MOST_WAITED, Observation, SignIn, Target,
    },
    progress::{Reads, Tracker},
    protocol::{Answer, MAX_LINE_BYTES, MAX_REPLY_BYTES, Request},
    sign_in::{Session as SignInSession, Starter},
    store::Store,
};
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{Diagnostic, Level, TaskStatus};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
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
    time::{Duration, Instant},
};

pub const READ_DEADLINE: Duration = Duration::from_millis(300);
pub const REQUEST_DEADLINE: Duration = Duration::from_millis(1100);
pub const CLIENT_DEADLINE: Duration = Duration::from_millis(1200);
pub const STALE_AFTER: Duration = Duration::from_millis(1500);
/// How often the part says how it is on `diagnostics` while nothing changes
/// ([`Config::report_every`] on a lamp).
pub const REPORT_EVERY: Duration = Duration::from_secs(1);
/// A `harness start` after a sign-in that failed is tried again this long
/// after, then twice as long after each next failure, [`DAEMON_START_TRIES`]
/// tries in all ([`Config::daemon_retry_after`] on a lamp): on lamp-75c9
/// (21 October) the start right after a spoken sign-in said `start_failed`,
/// and one by hand about a minute later worked.
pub const DAEMON_RETRY_AFTER: Duration = Duration::from_secs(10);
/// How many times `harness start` is tried after a sign-in: at once, then
/// 10, 20 and 40 s after each failure on a lamp.
pub const DAEMON_START_TRIES: u32 = 4;
pub const WORKERS: usize = 4;
pub const CLIENT_QUEUE: usize = 4;
pub const REQUEST_QUEUE: usize = 16;
const TICK: Duration = Duration::from_millis(10);
/// After a `wait` came back with an agent's end, the next on that machine
/// starts no sooner than this: an agent Harness calls done while the store
/// still finds it at work is not waited on in a loop.
pub const WAIT_AGAIN: Duration = Duration::from_secs(3);
/// After a `wait` failed, the next on that machine starts no sooner than
/// this; the watch's own looks go on meanwhile.
pub const WAIT_BACKOFF: Duration = Duration::from_secs(30);

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn refusal(code: &str, message: &str) -> Value {
    json!({"ok":false,"error":code,"message":message})
}

pub trait Clock: Send + 'static {
    fn sample(&self) -> io::Result<ClockSample>;
}
pub struct SystemClock {
    boot_id: String,
}
impl SystemClock {
    pub fn new(boot_id: Option<&str>) -> io::Result<Self> {
        let boot_id = match boot_id {
            Some(value) => value.to_owned(),
            None if cfg!(target_os = "linux") => {
                fs::read_to_string("/proc/sys/kernel/random/boot_id")?
                    .trim()
                    .into()
            }
            None => return Err(invalid("supply --boot-id on this development host")),
        };
        if boot_id.is_empty()
            || boot_id.len() > 80
            || !boot_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(invalid(
                "boot identity must be 1..80 ASCII letters, digits, dash or underscore",
            ));
        }
        Ok(Self { boot_id })
    }
}
impl Clock for SystemClock {
    fn sample(&self) -> io::Result<ClockSample> {
        #[cfg(target_os = "linux")]
        let id = rustix::time::ClockId::Boottime;
        #[cfg(not(target_os = "linux"))]
        let id = rustix::time::ClockId::Monotonic;
        let now = rustix::time::clock_gettime(id);
        let seconds = u64::try_from(now.tv_sec).map_err(|_| invalid("negative boot clock"))?;
        let nanos = u64::try_from(now.tv_nsec).map_err(|_| invalid("negative boot clock"))?;
        let monotonic_ms = seconds
            .checked_mul(1000)
            .and_then(|s| s.checked_add(nanos / 1_000_000))
            .ok_or_else(|| invalid("boot clock overflow"))?;
        Ok(ClockSample {
            boot_id: self.boot_id.clone(),
            monotonic_ms,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub enabled: bool,
    pub dir: PathBuf,
    pub socket: PathBuf,
    pub voice_socket: PathBuf,
    pub bus_root: PathBuf,
    pub targets: BTreeMap<String, Target>,
    pub run_for: Option<Duration>,
    /// Look at each task handed over on its own until its agent has
    /// finished or asks something (on unless `serve --no-watch`, [`Engine::watch`]).
    pub watch: bool,
    /// Read how each task being watched is going with one `harness ps` a
    /// machine, on the watch's schedule, and say so on `tasks/status`
    /// (`serve --progress`, [`crate::progress`]). Off unless set.
    pub progress: bool,
    /// Hold Harness's own `wait --any` on the agents the watch looks at, one
    /// a machine, and read an agent at once when it ends (`serve
    /// --push-wait`, #560). The watch's looks stay, as the backstop. Off
    /// unless set.
    pub push_wait: bool,
    /// Close the harness each task started once its answer is told and a
    /// read just before finds it idle on that turn (`serve --close-told`,
    /// #1390). Off unless set.
    pub close_told: bool,
    /// The task targets were written for a newer build, with this
    /// `version` ([`read_targets_or_newer`]): the part runs with hand-overs
    /// off and says so, rather than stopping and saying nothing (#1505).
    pub targets_newer: Option<u32>,
    /// `serve --set-up-later`: the part runs before the lamp is set up for
    /// hand-overs, and says why they are off (#1676). Off unless set.
    pub set_up_later: Option<SetUpLater>,
    /// Sign Harness in by a spoken code (`serve --sign-in-by-code`, #1676,
    /// [`crate::sign_in`]): the voice starts Harness's own sign-in, says its
    /// code, and passes on the person's answer, all on the trusted route.
    /// Off unless set.
    pub sign_in_by_code: bool,
    /// How often the part says how it is on `diagnostics` while nothing
    /// changes: [`REPORT_EVERY`] on a lamp. A change in readiness is said
    /// at once, whatever this is. Tests pass less, so a test that reads the
    /// report waits a fraction of a second rather than one (#1128).
    pub report_every: Duration,
    /// The first wait before `harness start` is tried again after a
    /// sign-in ([`DAEMON_RETRY_AFTER`] on a lamp). Tests pass less.
    pub daemon_retry_after: Duration,
}

/// What `serve --set-up-later` looks at again, and how often.
#[derive(Clone, Debug)]
pub struct SetUpLater {
    /// The targets file, read again whenever it changes: a computer added
    /// after sign-in is used without a restart, and a file that is gone
    /// leaves no target.
    pub targets: PathBuf,
    /// How often the file and Harness's sign-in are looked at again.
    pub every: Duration,
    /// `serve --targets-from-machines ENGINE`: while Harness is signed in,
    /// the targets file is written from the person's machines, every
    /// [`MACHINES_EVERY_LOOKS`] looks, with this engine. Never over a file
    /// someone else wrote ([`write_targets_from_machines`]).
    pub from_machines: Option<EngineKind>,
}

/// `serve --targets-from-machines` asks Harness for the machines once in
/// this many looks: every five minutes on a lamp.
pub const MACHINES_EVERY_LOOKS: u32 = 5;
/// The folder each written target names on the person's computer: today's
/// on lamp-4ace (`docs/tasks.md`). Harness never makes it: a computer
/// without it refuses the hand-over (`serve --folder-missing`, #2840).
pub const MACHINES_PROJECT: &str = "~/harnesses/lamp-tasks";

/// How often `serve --set-up-later` looks at the targets and the sign-in.
pub const SET_UP_LOOK_EVERY: Duration = Duration::from_secs(60);

/// An operator-owned file selects targets; a model supplies only an alias.
/// Read once as the part starts, and again when it changes with `serve
/// --set-up-later`. Existing tasks retain their original target.
pub fn read_targets(path: &Path) -> io::Result<BTreeMap<String, Target>> {
    read_targets_or_newer(path)?.map_err(|version| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("targets were written for a newer build (version {version})"),
        )
    })
}

/// [`read_targets`], or the `version` of targets written for a newer build:
/// a `version` above 1, or a field this build does not know. They stay
/// strict, since a later field may narrow what a task may do and passing
/// over it would hand work over more widely than the person set; but the
/// part can run with hand-overs off and say why (#1505). A file that is
/// not a targets file at all is refused as before.
pub fn read_targets_or_newer(path: &Path) -> io::Result<Result<BTreeMap<String, Target>, u32>> {
    fn unique_targets<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<String, Target>, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = BTreeMap<String, Target>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unique target aliases")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut access: M,
            ) -> Result<Self::Value, M::Error> {
                let mut values = BTreeMap::new();
                while let Some((name, target)) = access.next_entry::<String, Target>()? {
                    if values.insert(name, target).is_some() {
                        return Err(serde::de::Error::custom("duplicate target alias"));
                    }
                    if values.len() > 8 {
                        return Err(serde::de::Error::custom("too many targets"));
                    }
                }
                Ok(values)
            }
        }
        deserializer.deserialize_map(Visitor)
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Document {
        version: u32,
        #[serde(deserialize_with = "unique_targets")]
        targets: BTreeMap<String, Target>,
    }
    if !path.is_absolute() {
        return Err(invalid("targets path must be absolute"));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.len() > 16_384
    {
        return Err(invalid(
            "targets must be an owned unlinked 0600 regular file at most 16384 bytes",
        ));
    }
    let mut bytes = Vec::new();
    file.take(16_385).read_to_end(&mut bytes)?;
    if bytes.len() > 16_384 {
        return Err(invalid("targets file grew beyond its bound"));
    }
    let newer = |version: u64| u32::try_from(version).unwrap_or(u32::MAX);
    // Read strictly from the bytes first: a duplicate alias is refused
    // there, which a `Value` would hide by keeping the last.
    let document: Document = match serde_json::from_slice::<Document>(&bytes) {
        Ok(document) if document.version > 1 => return Ok(Err(document.version)),
        Ok(document) => document,
        // Not this build's: a newer `version`, or a field this build does
        // not know in version 1. Anything else is not a targets file.
        Err(error) => {
            let version = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|value| value.get("version").and_then(serde_json::Value::as_u64));
            return match version {
                Some(version) if version > 1 => Ok(Err(newer(version))),
                // serde's own words for a field or an engine this build
                // does not know; if they change, every such file reads as
                // not a targets file again, as before, and
                // `targets_for_a_newer_build_are_told_apart_and_never_read`
                // fails.
                Some(1)
                    if error.to_string().starts_with("unknown field")
                        || error.to_string().starts_with("unknown variant") =>
                {
                    Ok(Err(1))
                }
                _ => Err(invalid("invalid targets JSON")),
            };
        }
    };
    if document.version != 1 || document.targets.is_empty() || document.targets.len() > 8 {
        return Err(invalid("targets requires version 1 and 1..8 aliases"));
    }
    for (name, target) in &document.targets {
        if name.is_empty()
            || name.len() > 40
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(invalid("invalid target alias"));
        }
        target
            .validate()
            .map_err(|_| invalid("invalid configured target"))?;
    }
    Ok(Ok(document.targets))
}

/// Harness's own `wait`, held beside the adapter's one lane (`serve
/// --push-wait`): several may run at once, one a machine.
pub trait WaitLane: Send + Sync + 'static {
    /// Held until one of `agents` on `machine_id` ends, or until its time
    /// passes (none ended), or `cancel` is set (`cancelled`).
    fn wait(
        &self,
        machine_id: &str,
        agents: &[String],
        cancel: &AtomicBool,
    ) -> Result<Vec<Ended>, String>;
}
impl WaitLane for Cli {
    fn wait(
        &self,
        machine_id: &str,
        agents: &[String],
        cancel: &AtomicBool,
    ) -> Result<Vec<Ended>, String> {
        Cli::wait(self, machine_id, agents, cancel)
    }
}

pub trait Adapter: Send + 'static {
    fn launch(&self, launch: &Launch) -> LaunchOutcome;
    fn observe(&self, machine_id: &str, agent_id: &str) -> Result<Observation, String>;
    /// Every harness on one machine (`harness ps`), for progress. Asked only
    /// with `serve --progress`; an adapter without it says so.
    fn status(&self, machine_id: &str) -> Result<Vec<Listed>, String> {
        let _ = machine_id;
        Err("unsupported".into())
    }
    /// A lane for `wait`, apart from this adapter's own (`serve
    /// --push-wait`); none when the adapter has no such thing.
    fn wait_lane(&self) -> Option<Arc<dyn WaitLane>> {
        None
    }
    /// Close a harness this lamp started, its result told (#1390). An
    /// adapter without it says so, and nothing is closed.
    fn close(&self, machine_id: &str, agent_id: &str) -> Result<Closed, String> {
        let _ = (machine_id, agent_id);
        Err("unsupported".into())
    }
    /// Whether Harness is signed in to the person's account (`serve
    /// --set-up-later`). An adapter without it says so.
    fn signed_in(&self) -> Result<SignIn, String> {
        Err("unsupported".into())
    }
    /// The machines on the person's account (`serve
    /// --targets-from-machines`). An adapter without it says so.
    fn machines(&self) -> Result<Vec<AccountMachine>, String> {
        Err("unsupported".into())
    }
    /// There is no Harness program behind this adapter (`serve
    /// --set-up-later`): every hand-over is refused, with why.
    fn absent(&self) -> bool {
        false
    }
    /// What starts Harness's sign-in by a spoken code (`serve
    /// --sign-in-by-code`), apart from this adapter's lane: a sign-in waits
    /// minutes on a person. None when the adapter has no such thing.
    fn sign_in_starter(&self) -> Option<Starter> {
        None
    }
    /// Start Harness's daemon (`harness start`), once a sign-in by code has
    /// succeeded: the sign-in stops it, and its user service doesn't start
    /// it again (openharness#1463). An adapter without it says so.
    fn start_daemon(&self) -> Result<(), String> {
        Err("unsupported".into())
    }
}
impl Adapter for Cli {
    fn wait_lane(&self) -> Option<Arc<dyn WaitLane>> {
        match self.lane() {
            Ok(lane) => Some(Arc::new(lane)),
            Err(error) => {
                eprintln!("tasks: no lane for wait: {error}");
                None
            }
        }
    }
    fn launch(&self, launch: &Launch) -> LaunchOutcome {
        Cli::launch(self, launch)
    }
    fn observe(&self, machine_id: &str, agent_id: &str) -> Result<Observation, String> {
        Cli::observe(self, machine_id, agent_id)
    }
    fn status(&self, machine_id: &str) -> Result<Vec<Listed>, String> {
        Cli::status(self, machine_id)
    }
    fn close(&self, machine_id: &str, agent_id: &str) -> Result<Closed, String> {
        Cli::close(self, machine_id, agent_id)
    }
    fn signed_in(&self) -> Result<SignIn, String> {
        Cli::signed_in(self)
    }
    fn machines(&self) -> Result<Vec<AccountMachine>, String> {
        Cli::machines(self)
    }
    fn sign_in_starter(&self) -> Option<Starter> {
        match self.lane() {
            Ok(lane) => Some(Box::new(move || lane.sign_in_by_code())),
            Err(error) => {
                eprintln!("tasks: no lane for the sign-in: {error}");
                None
            }
        }
    }
    fn start_daemon(&self) -> Result<(), String> {
        Cli::start_daemon(self)
    }
}

/// The production adapter: Harness's CLI, or, with `serve --set-up-later`
/// and no program at `--harness`, nothing to hand over with. Harness put on
/// later is used once the part starts again.
enum Production {
    Cli(Box<Cli>),
    Absent,
}
impl Adapter for Production {
    fn wait_lane(&self) -> Option<Arc<dyn WaitLane>> {
        match self {
            Self::Cli(cli) => Adapter::wait_lane(&**cli),
            Self::Absent => None,
        }
    }
    fn launch(&self, launch: &Launch) -> LaunchOutcome {
        match self {
            Self::Cli(cli) => cli.launch(launch),
            Self::Absent => LaunchOutcome::NotSent {
                code: "no_harness".into(),
            },
        }
    }
    fn observe(&self, machine_id: &str, agent_id: &str) -> Result<Observation, String> {
        match self {
            Self::Cli(cli) => cli.observe(machine_id, agent_id),
            Self::Absent => Err("no_harness".into()),
        }
    }
    fn status(&self, machine_id: &str) -> Result<Vec<Listed>, String> {
        match self {
            Self::Cli(cli) => cli.status(machine_id),
            Self::Absent => Err("no_harness".into()),
        }
    }
    fn close(&self, machine_id: &str, agent_id: &str) -> Result<Closed, String> {
        match self {
            Self::Cli(cli) => cli.close(machine_id, agent_id),
            Self::Absent => Err("no_harness".into()),
        }
    }
    fn signed_in(&self) -> Result<SignIn, String> {
        match self {
            Self::Cli(cli) => cli.signed_in(),
            Self::Absent => Err("no_harness".into()),
        }
    }
    fn machines(&self) -> Result<Vec<AccountMachine>, String> {
        match self {
            Self::Cli(cli) => cli.machines(),
            Self::Absent => Err("no_harness".into()),
        }
    }
    fn absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
    fn sign_in_starter(&self) -> Option<Starter> {
        match self {
            Self::Cli(cli) => Adapter::sign_in_starter(&**cli),
            Self::Absent => Some(Box::new(|| Err("no_harness".into()))),
        }
    }
    fn start_daemon(&self) -> Result<(), String> {
        match self {
            Self::Cli(cli) => Adapter::start_daemon(&**cli),
            Self::Absent => Err("no_harness".into()),
        }
    }
}

/// Read the agent, then close it only if it is still idle on the very turn
/// the lamp told (#1390): the same answer to the same asked text. Anything
/// else is the person going on with it at the computer, and it is left to
/// them; one already stopped or failed has nothing to close.
fn close_told(adapter: &impl Adapter, job: &CloseJob) -> Result<Closed, String> {
    let now = adapter.observe(&job.machine_id, &job.agent_id)?;
    if matches!(now.state.as_str(), "stopped" | "failed") {
        return Ok(Closed::Gone);
    }
    if now.state != "idle" || now.answer != job.answer || now.ask != job.asked {
        return Ok(Closed::Left);
    }
    adapter.close(&job.machine_id, &job.agent_id)
}

struct Work {
    request: Request,
    trusted: bool,
    deadline: Instant,
    response: SyncSender<Result<Value, EngineError>>,
}
#[derive(Clone)]
struct Report {
    updated: Instant,
    summary: Value,
    requests: u64,
    cli_busy: bool,
    /// With `--progress`: the newest status and its number, to publish
    /// once, and how the reads have gone.
    status: Option<(u64, Arc<TaskStatus>)>,
    reads: Option<Reads>,
    /// With `serve --set-up-later`: what was last found.
    set_up: Option<SetUp>,
}
/// What `serve --set-up-later` last found.
#[derive(Clone, Copy, Default)]
struct SetUp {
    /// The targets file changed and is not valid: the targets before it
    /// are kept.
    targets_invalid: bool,
    /// Harness's sign-in, once read.
    sign_in: Option<SignIn>,
}
enum Job {
    Launch(LaunchJob),
    Observe(ObservationJob),
    Status(String),
    Close(CloseJob),
    SignIn,
    Machines,
    /// `harness start` after a sign-in by code (openharness#1463).
    StartDaemon,
}
enum Outcome {
    Launch(u64, LaunchOutcome),
    Observe(ObservationJob, Result<Observation, String>),
    Status(String, Result<Vec<Listed>, String>),
    Close(CloseJob, Result<Closed, String>),
    SignIn(Result<SignIn, String>),
    Machines(Result<Vec<AccountMachine>, String>),
    StartDaemon(Result<(), String>),
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

/// How often the owner looks again for a store kept aside (#1505).
const LOOK_ASIDE_EVERY: Duration = Duration::from_secs(60);

/// The owner's switches: `serve --watch`, `--progress`, the `wait` lane
/// (`--push-wait`), `--close-told`, `--set-up-later` and what starts a
/// sign-in (`--sign-in-by-code`).
type Switches = (
    bool,
    bool,
    Option<Arc<dyn WaitLane>>,
    bool,
    Option<SetUpLater>,
    Option<Starter>,
    // [`Config::daemon_retry_after`].
    Duration,
);

fn owner<C: Clock>(
    mut engine: Engine<Store>,
    clock: C,
    incoming: Receiver<Work>,
    (jobs, outcomes): (SyncSender<Job>, Receiver<Outcome>),
    current: Arc<Mutex<Report>>,
    stop: Arc<AtomicBool>,
    (watch, progress, lane, close_told, set_up_later, mut sign_in_starter, daemon_retry_after): Switches,
) -> io::Result<()> {
    let mut requests = 0u64;
    let mut busy = false;
    let mut waits = Waits::new(lane);
    // Progress lives here, beside the engine, and only in memory: what an
    // agent says while it works is not part of the durable record.
    // Each run counts changes afresh, and says which run it is.
    let mut tracker = progress.then(|| Tracker::started(unix_ms()));
    let mut status: Option<(u64, Arc<TaskStatus>)> = None;
    let mut status_began = Instant::now();
    // A store kept aside is looked for again once a minute (#1505).
    let mut looked_aside = Instant::now();
    // `serve --set-up-later`: the targets file as last read, and when the
    // file and the sign-in are next looked at. The first look is at once.
    let mut set_up = set_up_later.as_ref().map(|_| SetUp::default());
    let mut targets_seen = None;
    let mut names_said = String::new();
    let mut targets_due = Instant::now();
    let mut sign_in_due = Instant::now();
    let mut machines_due = Instant::now();
    // What the last write from the machines said, so the journal has one
    // line a change, not one a look.
    let mut machines_said = String::new();
    // `serve --sign-in-by-code`: the sign-in open, or the last one, done.
    let mut sign_in: Option<SignInSession> = None;
    // A sign-in by code succeeded: Harness's daemon is started, and when a
    // start fails, tried again later, with the tries made so far. A
    // hand-over waits while a try is due.
    let mut start_daemon_due: Option<Instant> = None;
    let mut daemon_tries: u32 = 0;
    // The last sign-in's id. Counted from the clock, so a sign-in after a
    // restart isn't taken for one before it (the reviewer's P3 on #2174).
    let mut sign_ins: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    while !stop.load(Ordering::Acquire) {
        if let Some(session) = sign_in.as_mut() {
            let was_open = session.open();
            for said in session.tend(Instant::now()) {
                eprintln!("tasks: sign-in {}: {said}", session.id());
            }
            // Signed in: the sign-in and the machines are looked at at
            // once, not a minute later.
            if was_open
                && matches!(
                    session.state(),
                    crate::sign_in::State::Done(crate::sign_in::Done::Success { .. })
                )
            {
                sign_in_due = Instant::now();
                machines_due = Instant::now();
                // Harness's own sign-in stops its daemon, and its user
                // service doesn't start it again, so every hand-over after
                // it says "Harness is not running here" (lamp-75c9, 20 Oct;
                // openharness#1463): started here.
                start_daemon_due = Some(Instant::now());
                daemon_tries = 0;
            }
        }
        if looked_aside.elapsed() >= LOOK_ASIDE_EVERY {
            engine.look_aside();
            looked_aside = Instant::now();
        }
        if let (Some(later), Some(found)) = (set_up_later.as_ref(), set_up.as_mut())
            && Instant::now() >= targets_due
        {
            targets_due = Instant::now() + later.every;
            look_at_targets(&mut engine, &later.targets, &mut targets_seen, found);
            look_at_names(&mut engine, &later.targets, &mut names_said);
        }
        // Something that may have changed which tasks are watched.
        let mut moved = false;
        if waits.lane.is_some() {
            let now = clock.sample()?;
            waits.tend(&engine.watched(&now), &engine.pushed(), Instant::now());
        }
        if let Ok(outcome) = outcomes.try_recv() {
            moved = true;
            let now = clock.sample()?;
            match outcome {
                Outcome::Launch(id, result) => engine.finish_launch(id, result, &now),
                Outcome::Observe(job, Ok(value)) => engine.finish_observation(&job, value, &now),
                Outcome::Observe(job, Err(code)) => engine.observation_failed(&job, &code, &now),
                Outcome::Close(job, result) => {
                    // One line a close, so the journal says which harness
                    // the lamp closed and how it went.
                    eprintln!("tasks: closing task {}'s harness: {result:?}", job.task_id);
                    engine.finish_close(&job, result, &now)
                }
                Outcome::Machines(read) => {
                    if let Some(later) = set_up_later.as_ref()
                        && let Some(engine) = later.from_machines
                    {
                        let said = match read {
                            Ok(listed) => {
                                match write_targets_from_machines(&later.targets, &listed, engine) {
                                    Ok(written) => {
                                        if written.changed {
                                            // Read at once, not a minute later.
                                            targets_due = Instant::now();
                                        }
                                        written.said
                                    }
                                    Err(error) => {
                                        format!(
                                            "task targets not written from the machines: {error}"
                                        )
                                    }
                                }
                            }
                            Err(code) => format!("Harness's machines not read: {code}"),
                        };
                        if said != machines_said {
                            eprintln!("tasks: {said}");
                            machines_said = said;
                        }
                    }
                    Ok(())
                }
                Outcome::StartDaemon(started) => {
                    daemon_tries += 1;
                    match started {
                        Ok(()) => eprintln!(
                            "tasks: Harness's daemon started after the sign-in (openharness#1463)"
                        ),
                        Err(code) if daemon_tries < DAEMON_START_TRIES => {
                            let after = daemon_retry_after * (1 << (daemon_tries - 1));
                            eprintln!(
                                "tasks: Harness's daemon not started after the sign-in: {code}; \
                                 tried again in {} s",
                                after.as_secs_f32()
                            );
                            start_daemon_due = Some(Instant::now() + after);
                        }
                        Err(code) => eprintln!(
                            "tasks: Harness's daemon not started after the sign-in: {code}; \
                             {daemon_tries} tries, not tried again until the next sign-in"
                        ),
                    }
                    Ok(())
                }
                Outcome::SignIn(read) => {
                    if let Some(found) = set_up.as_mut() {
                        match read {
                            Ok(sign_in) => {
                                if found.sign_in != Some(sign_in) {
                                    eprintln!("tasks: Harness signed in: {}", sign_in.name());
                                }
                                found.sign_in = Some(sign_in);
                            }
                            // Kept as it was: one failed read says nothing
                            // about the account.
                            Err(code) => eprintln!("tasks: Harness's sign-in not read: {code}"),
                        }
                    }
                    Ok(())
                }
                Outcome::Status(machine, listed) => {
                    if let Some(tracker) = tracker.as_mut() {
                        let took = status_began.elapsed().as_millis();
                        let line = tracker.read(
                            &machine,
                            listed,
                            &engine.watched(&now),
                            unix_ms(),
                            u64::try_from(took).unwrap_or(u64::MAX),
                        );
                        // One line a read, so a minute on the lamp can be
                        // judged from the journal.
                        eprintln!("tasks: {line}");
                    }
                    Ok(())
                }
            }
            .map_err(io::Error::other)?;
            busy = false;
        }
        match incoming.recv_timeout(TICK) {
            Ok(work) => {
                if Instant::now() >= work.deadline {
                    let _ = work.response.try_send(Err(EngineError {
                        code: "timeout",
                        message: "The queued request expired before it started.",
                    }));
                } else if let (Some(starter), true) = (
                    sign_in_starter.as_mut(),
                    matches!(
                        work.request,
                        Request::SignInStart {}
                            | Request::SignInState {}
                            | Request::SignInTakeCode { .. }
                            | Request::SignInAsked { .. }
                            | Request::SignInAnswer { .. }
                    ),
                ) {
                    let answer = sign_in_operation(
                        &work.request,
                        work.trusted,
                        starter,
                        &mut sign_in,
                        &mut sign_ins,
                        Instant::now(),
                    )
                    .and_then(|body| engine.identified(body));
                    requests = requests.saturating_add(1);
                    let _ = work.response.try_send(answer);
                } else if let (Request::GetProgress { id }, Some(tracker)) =
                    (&work.request, tracker.as_ref())
                {
                    let answer = match tracker.words(*id) {
                        Some(progress) => engine.identified(json!({"id":id,"progress":progress})),
                        None if tracker.unread(*id) => Err(EngineError {
                            code: "not_read",
                            message: "That task is watched, and its progress has not been read yet.",
                        }),
                        // Let go because its agent settled: what it ended
                        // in, while it is on `tasks/status` as settled,
                        // never `not_found` (Ten and Nine on #705, problem
                        // 3 (b)). Its words are `get_observation`'s.
                        None => match tracker.settled_of(*id) {
                            Some(settled) => engine.identified(json!({"id":id,"settled":settled})),
                            None => Err(EngineError {
                                code: "not_found",
                                message: "No task with that ID is being read for progress.",
                            }),
                        },
                    };
                    requests = requests.saturating_add(1);
                    let _ = work.response.try_send(answer);
                } else {
                    moved = true;
                    let answer = engine.handle(&work.request, &clock.sample()?, work.trusted);
                    requests = requests.saturating_add(1);
                    let _ = work.response.try_send(answer);
                    if engine.failed() {
                        return Err(io::Error::other("Tasks storage failed; restart required"));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if !busy && !stop.load(Ordering::Acquire) {
            let now = clock.sample()?;
            // Harness's daemon first, once, after a sign-in by code
            // succeeded (openharness#1463): a hand-over already waiting
            // would go to the daemon the sign-in stopped, and fail (the
            // reviewer-lead's P2 on #2309).
            // A start that failed is tried again later, and until then no
            // hand-over goes either: it would fail on the stopped daemon.
            let daemon = match start_daemon_due {
                Some(due) if due <= Instant::now() => {
                    start_daemon_due = None;
                    Some(Job::StartDaemon)
                }
                _ => None,
            };
            let launch = match daemon {
                Some(job) => Some(job),
                None if start_daemon_due.is_some() => None,
                None => engine
                    .begin_launch(&now)
                    .map_err(io::Error::other)?
                    .map(Job::Launch),
            };
            let next = match launch {
                Some(job) => Some(job),
                None => {
                    // An agent Harness's `wait` says ended is read first,
                    // and its idle answer kept at once.
                    waits.push(&mut engine, &now).map_err(io::Error::other)?;
                    if watch {
                        engine.watch(&now).map_err(io::Error::other)?;
                    }
                    let observe = engine
                        .begin_observation(&now)
                        .map_err(io::Error::other)?
                        .map(Job::Observe);
                    // A harness done and told is closed after launches and
                    // reads (#1390).
                    let observe = match observe {
                        Some(job) => Some(job),
                        None if close_told => engine
                            .begin_close(&now)
                            .map_err(io::Error::other)?
                            .map(Job::Close),
                        None => None,
                    };
                    // One `ps` a machine, after launches and reads: it is a
                    // read only and takes nothing from them.
                    observe
                        .or_else(|| {
                            let tracker = tracker.as_mut()?;
                            let watched = engine.watched(&now);
                            let machine = tracker.due(&watched, now.monotonic_ms)?;
                            tracker.asked(&machine, &watched, now.monotonic_ms);
                            status_began = Instant::now();
                            Some(Job::Status(machine))
                        })
                        // The sign-in last of all, once a while, and never
                        // with no Harness to ask (`serve --set-up-later`).
                        .or_else(|| {
                            let later = set_up_later.as_ref()?;
                            if engine.summary()["no_harness"] == true
                                || Instant::now() < sign_in_due
                            {
                                return None;
                            }
                            sign_in_due = Instant::now() + later.every;
                            Some(Job::SignIn)
                        })
                        // The machines, once signed in, less often again.
                        .or_else(|| {
                            let later = set_up_later.as_ref()?;
                            later.from_machines?;
                            let signed_in = set_up
                                .as_ref()
                                .is_some_and(|found| found.sign_in == Some(SignIn::In));
                            if !signed_in || Instant::now() < machines_due {
                                return None;
                            }
                            machines_due = Instant::now() + later.every * MACHINES_EVERY_LOOKS;
                            Some(Job::Machines)
                        })
                }
            };
            if let Some(job) = next {
                // The durable attempt precedes this handoff. If this queue or
                // worker fails, reopening records uncertainty without a resend.
                jobs.try_send(job).map_err(|_| {
                    io::Error::other("Tasks CLI worker unavailable after durable handoff")
                })?;
                busy = true;
            }
        }
        // A task that left the watch leaves the bus at once, not at the
        // next read.
        if moved && let Some(tracker) = tracker.as_mut() {
            tracker.watching(&engine.watched(&clock.sample()?));
        }
        // A task let go because its agent settled is said so a while
        // (#1021); looked at each turn, so that it also leaves on time.
        if let Some(tracker) = tracker.as_mut() {
            let found = engine.settled_within(&clock.sample()?, crate::progress::SETTLED_FOR_MS);
            tracker.settled(&found, unix_ms());
        }
        if let Some(tracker) = tracker.as_mut()
            && let Some(fresh) = tracker.take(engine.store_id())
        {
            let number = status.as_ref().map_or(1, |(number, _)| number + 1);
            status = Some((number, Arc::new(fresh)));
        }
        *current
            .lock()
            .map_err(|_| io::Error::other("Tasks report lock poisoned"))? = Report {
            updated: Instant::now(),
            summary: engine.summary(),
            requests,
            cli_busy: busy,
            status: status.clone(),
            reads: tracker.as_ref().map(Tracker::reads),
            set_up,
        };
    }
    Ok(())
}

/// One of the sign-in's operations (`serve --sign-in-by-code`, #1676), on
/// the trusted route only. A start while one is open gives that one back;
/// a start after one is done begins anew. The answer is passed on only
/// while Harness is asking.
fn sign_in_operation(
    request: &Request,
    trusted: bool,
    starter: &mut Starter,
    sign_in: &mut Option<SignInSession>,
    sign_ins: &mut u64,
    now: Instant,
) -> Result<Value, EngineError> {
    if !trusted {
        return Err(EngineError {
            code: "untrusted",
            message: "This operation requires the trusted Voice route.",
        });
    }
    match request {
        Request::SignInStart {} => {
            if !sign_in.as_ref().is_some_and(SignInSession::open) {
                *sign_ins += 1;
                let session = match starter() {
                    Ok(link) => {
                        eprintln!("tasks: sign-in {sign_ins}: started");
                        SignInSession::new(*sign_ins, link, now)
                    }
                    Err(code) => {
                        eprintln!("tasks: sign-in {sign_ins}: not started: {code}");
                        SignInSession::failed(*sign_ins, &code)
                    }
                };
                *sign_in = Some(session);
            }
            let session = sign_in.as_mut().expect("just set");
            for said in session.tend(now) {
                eprintln!("tasks: sign-in {}: {said}", session.id());
            }
            session.code_given();
            Ok(session.view(now))
        }
        Request::SignInState {} => {
            let mut view = sign_in
                .as_ref()
                .map_or_else(|| json!({"sign_in": "none"}), |session| session.view(now));
            view["code_claims"] = 1.into();
            Ok(view)
        }
        Request::SignInTakeCode { id } => {
            let Some(session) = sign_in.as_mut().filter(|session| session.id() == *id) else {
                return Ok(json!({"claim": "not_available", "id": id}));
            };
            for said in session.tend(now) {
                eprintln!("tasks: sign-in {}: {said}", session.id());
            }
            Ok(session.take_code(*id, now))
        }
        Request::SignInAsked { id, request } => {
            let session = sign_in.as_mut().ok_or(EngineError {
                code: "not_asking",
                message: "No sign-in is asking for an answer.",
            })?;
            for said in session.tend(now) {
                eprintln!("tasks: sign-in {}: {said}", session.id());
            }
            match session.asked(*id, *request, now) {
                Ok(()) => {
                    eprintln!("tasks: sign-in {id}: question told in request {request}");
                    Ok(session.view(now))
                }
                Err("not_this_sign_in") => Err(EngineError {
                    code: "not_this_sign_in",
                    message: "That sign-in is not the one open.",
                }),
                Err(_) => Err(EngineError {
                    code: "not_asking",
                    message: "The sign-in is not asking for an answer now.",
                }),
            }
        }
        Request::SignInAnswer { answer, request } => {
            let session = sign_in.as_mut().ok_or(EngineError {
                code: "not_asking",
                message: "No sign-in is asking for an answer.",
            })?;
            for said in session.tend(now) {
                eprintln!("tasks: sign-in {}: {said}", session.id());
            }
            let said = if *answer == Answer::Yes { "yes" } else { "no" };
            match session.answer(*answer == Answer::Yes, *request, now) {
                Ok(()) => {
                    eprintln!("tasks: sign-in {}: answered {said}", session.id());
                    Ok(session.view(now))
                }
                Err(code) => {
                    // A refusal leaves the question open: journalled, so a
                    // yes the lamp didn't take can be told from one never
                    // heard.
                    eprintln!("tasks: sign-in {}: {said} not taken: {code}", session.id());
                    Err(match code {
                        "not_asking" => EngineError {
                            code: "not_asking",
                            message: "The sign-in is not asking for an answer now.",
                        },
                        "not_told" => EngineError {
                            code: "not_told",
                            message: "The question was not heard before this request.",
                        },
                        "too_late" => EngineError {
                            code: "too_late",
                            message: "The question was told more than two minutes ago.",
                        },
                        _ => EngineError {
                            code: "ended",
                            message: "Harness's sign-in ended before the answer reached it.",
                        },
                    })
                }
            }
        }
        _ => unreachable!("only the sign-in's operations"),
    }
}

/// The targets the person's machines give (`serve
/// --targets-from-machines`): one for each machine but the lamp itself, at
/// most 8, newest first as Harness lists them. The first is `office`, the
/// alias the voice asks for unless told another (`LAMP_TASKS_TARGET`); each
/// other is its Harness name made into an alias (`MacBook Air` is
/// `macbook-air`), or `computer` with none, and `-2`, `-3` … where two
/// would be the same. Each names [`MACHINES_PROJECT`] and `engine`.
pub fn targets_from_machines(
    machines: &[AccountMachine],
    engine: EngineKind,
) -> BTreeMap<String, Target> {
    from_machines(machines, engine).0
}

/// Each computer's own name by its alias, as [`targets_from_machines`]
/// names them: its Harness name, else its hostname, as a label of at most
/// [`MOST_NAME_BYTES`] with no control characters; none where nothing is
/// left (#1676).
pub fn names_from_machines(machines: &[AccountMachine]) -> BTreeMap<String, String> {
    from_machines(machines, EngineKind::Claude).1
}

/// A machine's name as said to the person: control characters out, spaces
/// at the ends trimmed, cut to [`MOST_NAME_BYTES`] on a character boundary.
fn label_of(label: &str) -> Option<String> {
    let clean: String = label.chars().filter(|c| !c.is_control()).collect();
    let mut name = String::new();
    for c in clean.trim().chars() {
        if name.len() + c.len_utf8() > MOST_NAME_BYTES {
            break;
        }
        name.push(c);
    }
    let name = name.trim_end().to_owned();
    (!name.is_empty()).then_some(name)
}

fn from_machines(
    machines: &[AccountMachine],
    engine: EngineKind,
) -> (BTreeMap<String, Target>, BTreeMap<String, String>) {
    let mut targets = BTreeMap::new();
    let mut names = BTreeMap::new();
    for (index, machine) in machines.iter().filter(|m| !m.current).take(8).enumerate() {
        let base = if index == 0 {
            "office".to_owned()
        } else {
            alias_of(&machine.label)
        };
        let mut alias = base.clone();
        let mut next = 2;
        while targets.contains_key(&alias) {
            alias = format!("{base}-{next}");
            next += 1;
        }
        if let Some(name) = label_of(&machine.label) {
            names.insert(alias.clone(), name);
        }
        targets.insert(
            alias,
            Target {
                machine_id: machine.machine_id.clone(),
                project: MACHINES_PROJECT.into(),
                engine,
            },
        );
    }
    (targets, names)
}

/// The longest name a computer is told by (#1676).
pub const MOST_NAME_BYTES: usize = 64;

/// The computers' names beside a targets file: `targets.names.json` beside
/// `targets.json` (#1676). A file of its own, so an older build, which never
/// reads it, keeps its hand-overs on a rollback.
pub fn names_path(targets: &Path) -> PathBuf {
    targets.with_extension("names.json")
}

/// The computers' names, read as strictly as the targets: `{"version":1,
/// "names":{ALIAS:NAME}}`, an owned 0600 file of at most 16384 bytes, each
/// alias a target alias's shape and once, each name at most
/// [`MOST_NAME_BYTES`], with no control characters and no spaces at the
/// ends. Anything else is an error, and the voice then says what it says
/// today.
pub fn read_target_names(path: &Path) -> io::Result<BTreeMap<String, String>> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Document {
        version: u32,
        names: serde_json::Map<String, Value>,
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.len() > 16_384
    {
        return Err(invalid(
            "names must be an owned unlinked 0600 regular file at most 16384 bytes",
        ));
    }
    let mut bytes = Vec::new();
    file.take(16_385).read_to_end(&mut bytes)?;
    let document: Document =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid names JSON"))?;
    if document.version != 1 || document.names.len() > 32 {
        return Err(invalid("names requires version 1 and at most 32"));
    }
    // A key twice is refused: the map read above keeps only the last.
    if duplicate_key(&bytes) {
        return Err(invalid("a name given twice"));
    }
    let mut names = BTreeMap::new();
    for (alias, name) in document.names {
        let Some(name) = name.as_str() else {
            return Err(invalid("a name that is not text"));
        };
        if alias.is_empty()
            || alias.len() > 40
            || !alias
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || label_of(name).as_deref() != Some(name)
        {
            return Err(invalid("invalid name"));
        }
        names.insert(alias, name.to_owned());
    }
    Ok(names)
}

/// Whether `names` in a names file has a key twice, read from the bytes.
fn duplicate_key(bytes: &[u8]) -> bool {
    struct Keys;
    impl<'de> serde::de::Visitor<'de> for Keys {
        type Value = bool;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(self, mut access: M) -> Result<bool, M::Error> {
            let mut seen = BTreeSet::new();
            let mut twice = false;
            while let Some(key) = access.next_key::<String>()? {
                twice |= !seen.insert(key);
                access.next_value::<serde::de::IgnoredAny>()?;
            }
            Ok(twice)
        }
    }
    struct Names(bool);
    impl<'de> Deserialize<'de> for Names {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Top;
            impl<'de> serde::de::Visitor<'de> for Top {
                type Value = bool;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("a names file")
                }
                fn visit_map<M: serde::de::MapAccess<'de>>(
                    self,
                    mut access: M,
                ) -> Result<bool, M::Error> {
                    let mut twice = false;
                    while let Some(key) = access.next_key::<String>()? {
                        if key == "names" {
                            twice |= access.next_value_seed(KeysSeed)?;
                        } else {
                            access.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                    Ok(twice)
                }
            }
            deserializer.deserialize_map(Top).map(Names)
        }
    }
    struct KeysSeed;
    impl<'de> serde::de::DeserializeSeed<'de> for KeysSeed {
        type Value = bool;
        fn deserialize<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<bool, D::Error> {
            deserializer.deserialize_map(Keys)
        }
    }
    serde_json::from_slice::<Names>(bytes).is_ok_and(|names| names.0)
}

/// A machine's name as a target alias: lower case letters and digits, one
/// `-` between words, at most 32 bytes; `computer` when nothing is left.
fn alias_of(label: &str) -> String {
    let mut alias = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            if alias.len() >= 32 {
                break;
            }
            alias.push(c.to_ascii_lowercase());
        } else if !alias.is_empty() && !alias.ends_with('-') {
            alias.push('-');
        }
    }
    let alias = alias.trim_end_matches('-');
    if alias.is_empty() {
        "computer".into()
    } else {
        alias.into()
    }
}

/// What one write from the machines did, for the journal.
#[derive(Debug, PartialEq, Eq)]
pub struct Written {
    /// The targets file changed: written, or removed.
    pub changed: bool,
    pub said: String,
}

/// The file beside the targets that names what this part wrote there: the
/// sha256 of each version it may have left, one a line.
fn written_mark(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".from-machines");
    PathBuf::from(name)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Written beside and renamed over, 0600, synced: a reader sees the old
/// file or the new one, never half of one.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".next-{}", std::process::id()));
    let next = PathBuf::from(name);
    let _ = fs::remove_file(&next);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&next)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&next, path)
}

/// Write the targets file from the person's machines (`serve
/// --targets-from-machines`), only where the file is this part's to write:
/// there is none, or it is one this part wrote (its sha256 is in the mark
/// beside it). A file someone wrote by hand, or a pairing, is never
/// touched. With no machine but the lamp, a file this part wrote is
/// removed: there is no computer any more. The mark is written before the
/// file, naming both the file there and the new one, so a stop between
/// the two never leaves a file of its own looking like someone else's.
pub fn write_targets_from_machines(
    path: &Path,
    machines: &[AccountMachine],
    engine: EngineKind,
) -> io::Result<Written> {
    let said = |changed: bool, said: String| Ok(Written { changed, said });
    let mark = written_mark(path);
    let present = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let marked = match fs::read_to_string(&mark) {
        Ok(text) => text.lines().map(str::to_owned).collect::<Vec<_>>(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    if let Some(bytes) = &present
        && !marked.contains(&sha256_hex(bytes))
    {
        return said(
            false,
            "the targets file was not written from the machines; it is left as it is".into(),
        );
    }
    let (targets, names) = from_machines(machines, engine);
    let names_file = names_path(path);
    if targets.is_empty() {
        if present.is_some() {
            fs::remove_file(path)?;
            let _ = fs::remove_file(&mark);
            let _ = fs::remove_file(&names_file);
            return said(
                true,
                "no machine but the lamp: the task targets written before are removed".into(),
            );
        }
        return said(false, "no machine but the lamp: no task targets".into());
    }
    let mut bytes =
        serde_json::to_vec(&json!({"version":1,"targets":targets})).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let count = targets.len();
    let targets_changed = present.as_deref() != Some(bytes.as_slice());
    if targets_changed {
        let new = sha256_hex(&bytes);
        let lines = present
            .as_deref()
            .map(sha256_hex)
            .into_iter()
            .chain([new.clone()])
            .collect::<Vec<_>>()
            .join("\n");
        write_atomically(&mark, format!("{lines}\n").as_bytes())?;
        write_atomically(path, &bytes)?;
        write_atomically(&mark, format!("{new}\n").as_bytes())?;
    }
    // The names beside them, only where the targets are this part's own.
    let names_changed = keep_names(&names_file, &names)?;
    let said_now = match (targets_changed, names_changed) {
        (true, _) => format!("{count} task targets written from the machines"),
        (false, true) => format!("{count} task targets from the machines, their names written"),
        (false, false) => format!("{count} task targets from the machines, unchanged"),
    };
    said(targets_changed || names_changed, said_now)
}

/// The names file as `names` says: written whole when it differs, removed
/// when there are none. Whether it changed.
fn keep_names(path: &Path, names: &BTreeMap<String, String>) -> io::Result<bool> {
    let present = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if names.is_empty() {
        if present.is_some() {
            fs::remove_file(path)?;
            return Ok(true);
        }
        return Ok(false);
    }
    let mut bytes =
        serde_json::to_vec(&json!({"version":1,"names":names})).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if present.as_deref() == Some(bytes.as_slice()) {
        return Ok(false);
    }
    write_atomically(path, &bytes)?;
    Ok(true)
}

/// The computers' names read again (`serve --set-up-later`): a missing
/// names file is none, and one that is not valid is none too, said once,
/// so the voice says what it says today (#1676).
fn look_at_names(engine: &mut Engine<Store>, targets: &Path, said: &mut String) {
    let (names, now) = match read_target_names(&names_path(targets)) {
        Ok(names) => {
            let count = names.len();
            (names, format!("{count} computer names"))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            (BTreeMap::new(), "no computer names".to_owned())
        }
        Err(error) => (
            BTreeMap::new(),
            format!("computer names not valid ({error}); none are told"),
        ),
    };
    engine.set_names(names);
    if now != *said {
        eprintln!("tasks: {now}");
        *said = now;
    }
}

/// What makes a targets file another one: written again in place, or
/// replaced by a rename.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    dev: u64,
    ino: u64,
    len: u64,
    modified: i64,
    nanos: i64,
}

/// Read the targets again when the file changed (`serve --set-up-later`).
/// The same strictness as at start ([`read_targets_or_newer`]): valid ones
/// are used from now on; ones for a newer build turn hand-overs off; a
/// file that is gone leaves no target; and one that is not valid keeps the
/// targets before it, and says so. `seen` is the file as last read: `None`
/// before the first look, `Some(None)` while there is no file.
fn look_at_targets(
    engine: &mut Engine<Store>,
    path: &Path,
    seen: &mut Option<Option<FileIdentity>>,
    found: &mut SetUp,
) {
    let identity = fs::symlink_metadata(path)
        .ok()
        .map(|metadata| FileIdentity {
            dev: metadata.dev(),
            ino: metadata.ino(),
            len: metadata.len(),
            modified: metadata.mtime(),
            nanos: metadata.mtime_nsec(),
        });
    if *seen == Some(identity) {
        return;
    }
    let first = seen.is_none();
    *seen = Some(identity);
    let read = match read_targets_or_newer(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Ok(BTreeMap::new())),
        read => read,
    };
    // One line a change, with how many and never their names: a target's
    // alias may be a computer's name.
    let said = match read {
        Ok(Ok(targets)) => {
            let count = targets.len();
            match engine.set_targets(targets) {
                Ok(()) => {
                    found.targets_invalid = false;
                    format!("{count} task targets")
                }
                Err(_) => {
                    found.targets_invalid = true;
                    "task targets not valid; the ones before are kept".into()
                }
            }
        }
        Ok(Err(version)) => {
            found.targets_invalid = false;
            engine.targets_newer(version);
            format!("task targets for a newer build (version {version}); hand-overs are off")
        }
        Err(error) => {
            found.targets_invalid = true;
            format!("task targets not valid ({error}); the ones before are kept")
        }
    };
    if !first {
        eprintln!("tasks: the targets file changed: {said}");
    }
}

/// The `wait`s held, one a machine, and the agents' ends they brought that
/// are still to be read.
struct Waits {
    lane: Option<Arc<dyn WaitLane>>,
    held: BTreeMap<String, Held>,
    /// When the next `wait` on a machine may start.
    rest: BTreeMap<String, Instant>,
    /// Tasks whose agent ended, to be read at once.
    pushes: BTreeSet<u64>,
    ended: (mpsc::Sender<WaitEnd>, Receiver<WaitEnd>),
    /// Each `wait` started is numbered, so an answer is matched to the one
    /// that gave it: a cancelled `wait` that answers late, after another on
    /// the same agents replaced it, is dropped (#1157 review).
    next: u64,
}
/// What one `wait` came back with: its machine, its number, and the ends it
/// brought or why it failed.
type WaitEnd = (String, u64, Result<Vec<Ended>, String>);
struct Held {
    number: u64,
    agents: Vec<String>,
    cancel: Arc<AtomicBool>,
    began: Instant,
}

impl Waits {
    fn new(lane: Option<Arc<dyn WaitLane>>) -> Self {
        Self {
            lane,
            held: BTreeMap::new(),
            rest: BTreeMap::new(),
            pushes: BTreeSet::new(),
            ended: mpsc::channel(),
            next: 0,
        }
    }

    /// Takes what came back, and holds one `wait` a machine on exactly the
    /// agents the watch looks at there: one whose set changed is cancelled
    /// and started again with the new set. A task already pushed and not
    /// yet read, or whose latest read was a push (`pushed`), is left out
    /// until the watch's own next read of it: one agent Harness keeps
    /// calling done never holds up the others on its machine (the reviewer
    /// on #1157).
    fn tend(&mut self, watched: &[Watched], pushed: &BTreeSet<u64>, now: Instant) {
        let Some(lane) = self.lane.clone() else {
            return;
        };
        while let Ok((machine, number, result)) = self.ended.1.try_recv() {
            let Some(held) = self.held.get(&machine).filter(|held| held.number == number) else {
                continue;
            };
            let took = held.began.elapsed().as_millis();
            self.held.remove(&machine);
            match result {
                Ok(ended) if ended.is_empty() => {}
                Ok(ended) => {
                    let named: Vec<String> = ended
                        .iter()
                        .map(|end| {
                            let task = watched
                                .iter()
                                .find(|w| w.machine_id == machine && w.agent_id == end.agent_id);
                            if let Some(task) = task {
                                self.pushes.insert(task.task_id);
                            }
                            format!(
                                "task {} {}",
                                task.map_or("?".into(), |task| task.task_id.to_string()),
                                end.state
                            )
                        })
                        .collect();
                    eprintln!(
                        "tasks: wait on {machine} came back after {took} ms: {}",
                        named.join(", ")
                    );
                    self.rest.insert(machine, now + WAIT_AGAIN);
                }
                Err(code) if code == "cancelled" => {}
                Err(code) => {
                    eprintln!("tasks: wait on {machine} failed after {took} ms ({code})");
                    self.rest.insert(machine, now + WAIT_BACKOFF);
                }
            }
        }
        let mut wanted: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for task in watched
            .iter()
            .filter(|task| !pushed.contains(&task.task_id) && !self.pushes.contains(&task.task_id))
        {
            wanted
                .entry(task.machine_id.as_str())
                .or_default()
                .insert(task.agent_id.as_str());
        }
        let wanted: BTreeMap<String, Vec<String>> = wanted
            .into_iter()
            .map(|(machine, agents)| {
                let agents = agents.into_iter().take(MOST_WAITED).map(str::to_owned);
                (machine.to_owned(), agents.collect())
            })
            .collect();
        self.held.retain(|machine, held| {
            let keep = wanted.get(machine) == Some(&held.agents);
            if !keep {
                held.cancel.store(true, Ordering::Release);
            }
            keep
        });
        self.pushes
            .retain(|id| watched.iter().any(|task| task.task_id == *id));
        for (machine, agents) in wanted {
            if self.held.contains_key(&machine)
                || self.rest.get(&machine).is_some_and(|until| now < *until)
            {
                continue;
            }
            let cancel = Arc::new(AtomicBool::new(false));
            let number = self.next;
            self.next = self.next.wrapping_add(1);
            let (lane, sender) = (lane.clone(), self.ended.0.clone());
            let (asked, flag, on) = (agents.clone(), cancel.clone(), machine.clone());
            let started = thread::Builder::new()
                .name("tasks-wait".into())
                .spawn(move || {
                    let result = lane.wait(&on, &asked, &flag);
                    let _ = sender.send((on, number, result));
                });
            if started.is_err() {
                eprintln!("tasks: wait on {machine} could not start");
                self.rest.insert(machine, now + WAIT_BACKOFF);
                continue;
            }
            self.held.insert(
                machine,
                Held {
                    number,
                    agents,
                    cancel,
                    began: Instant::now(),
                },
            );
        }
    }

    /// Asks for the read of one task whose agent ended, if the engine can
    /// take it now; the rest wait their turn.
    fn push(&mut self, engine: &mut Engine<Store>, now: &ClockSample) -> Result<(), EngineError> {
        let Some(&id) = self.pushes.iter().next() else {
            return Ok(());
        };
        if engine.push(id, now)? {
            self.pushes.remove(&id);
        }
        Ok(())
    }
}

impl Drop for Waits {
    fn drop(&mut self) {
        for held in self.held.values() {
            held.cancel.store(true, Ordering::Release);
        }
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Tasks client deadline"))
}

fn wait_ready(stream: &UnixStream, events: PollFlags, deadline: Instant) -> io::Result<()> {
    loop {
        let timeout = Timespec::try_from(remaining(deadline)?).map_err(io::Error::other)?;
        let mut descriptors = [PollFd::new(stream, events)];
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => continue,
            Ok(_) => {
                remaining(deadline)?;
                let returned = descriptors[0].revents();
                if returned.contains(PollFlags::NVAL) {
                    return Err(invalid("invalid client descriptor"));
                }
                if returned.intersects(events | PollFlags::HUP | PollFlags::ERR) {
                    return Ok(());
                }
            }
            Err(e) if e == rustix::io::Errno::INTR => {}
            Err(e) => return Err(e.into()),
        }
    }
}

fn read_line(stream: &mut UnixStream, bound: usize, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        remaining(deadline)?;
        match stream.read(&mut buffer) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Tasks message needs a newline",
                ));
            }
            Ok(n) => {
                let end = buffer[..n].iter().position(|b| *b == b'\n');
                let part = &buffer[..end.unwrap_or(n)];
                if bytes.len() + part.len() > bound {
                    return Err(invalid("message exceeds its byte bound"));
                }
                bytes.extend_from_slice(part);
                if let Some(end) = end {
                    if end + 1 != n {
                        return Err(invalid("one message per connection"));
                    }
                    return Ok(bytes);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                wait_ready(stream, PollFlags::IN, deadline)?
            }
            Err(e) => return Err(e),
        }
    }
}

fn write_bytes(stream: &mut UnixStream, bytes: &[u8], deadline: Instant) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        remaining(deadline)?;
        match stream.write(&bytes[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Tasks client closed",
                ));
            }
            Ok(n) => offset += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                wait_ready(stream, PollFlags::OUT, deadline)?
            }
            Err(e) => return Err(e),
        }
    }
    remaining(deadline)?;
    Ok(())
}

fn serve_client(
    mut stream: UnixStream,
    accepted: Instant,
    trusted: bool,
    owner: &SyncSender<Work>,
) {
    if stream.set_nonblocking(true).is_err() {
        return;
    }
    let reply = match read_line(&mut stream, MAX_LINE_BYTES, accepted + READ_DEADLINE) {
        Ok(line) => match Request::parse(&line) {
            Ok(request) => {
                let (tx, rx) = mpsc::sync_channel(1);
                let deadline = accepted + REQUEST_DEADLINE;
                match owner.try_send(Work {
                    request,
                    trusted,
                    deadline,
                    response: tx,
                }) {
                    Ok(()) => match remaining(deadline)
                        .and_then(|left| rx.recv_timeout(left).map_err(io::Error::other))
                    {
                        Ok(Ok(value)) => value,
                        Ok(Err(error)) => refusal(error.code, error.message),
                        Err(_) => refusal(
                            "uncertain",
                            "No receipt arrived. Read status; retry only the exact guarded request because it may have committed.",
                        ),
                    },
                    Err(TrySendError::Full(_)) => refusal(
                        "busy",
                        "Tasks request queue is full; this request did not start.",
                    ),
                    Err(TrySendError::Disconnected(_)) => refusal(
                        "unavailable",
                        "Tasks owner is unavailable; this request did not start.",
                    ),
                }
            }
            Err(_) => refusal(
                "bad_request",
                "Send one bounded JSON object with recognized fields.",
            ),
        },
        Err(e) if e.kind() == io::ErrorKind::InvalidInput => {
            refusal("bad_request", "Send one JSON line of at most 8192 bytes.")
        }
        Err(_) => refusal(
            "incomplete",
            "A complete request did not arrive within 300 ms.",
        ),
    };
    if let Ok(mut bytes) = serde_json::to_vec(&reply) {
        if bytes.len() > MAX_REPLY_BYTES {
            bytes = b"{\"ok\":false,\"error\":\"reply_bound\"}".to_vec();
        }
        bytes.push(b'\n');
        let _ = write_bytes(&mut stream, &bytes, accepted + CLIENT_DEADLINE);
    }
}

/// Bounded reference client used by host checks and trusted local consumers.
/// A timeout is not proof that a mutation failed to commit.
pub fn request(path: &Path, value: &Value, limit: Duration) -> io::Result<Value> {
    let deadline = Instant::now()
        .checked_add(limit)
        .ok_or_else(|| invalid("client deadline overflow"))?;
    let mut bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_LINE_BYTES {
        return Err(invalid("request exceeds 8192 bytes"));
    }
    bytes.push(b'\n');
    let descriptor = rustix::net::socket(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        None,
    )?;
    rustix::io::fcntl_setfd(&descriptor, rustix::io::FdFlags::CLOEXEC)?;
    let mut stream = UnixStream::from(descriptor);
    stream.set_nonblocking(true)?;
    let address = rustix::net::SocketAddrUnix::new(path)?;
    // Unix nonblocking connect reports a full accept queue rather than waiting.
    // Retrying connect is left to the caller and never resends a mutation here.
    rustix::net::connect(&stream, &address)?;
    write_bytes(&mut stream, &bytes, deadline)?;
    let line = read_line(&mut stream, MAX_REPLY_BYTES, deadline)?;
    serde_json::from_slice(&line).map_err(io::Error::other)
}

fn random_id() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn publish<T: serde::Serialize>(out: &mut Publisher, value: &T, errors: &mut u64) {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let at_us = (now.tv_sec.max(0) as u64)
        .saturating_mul(1_000_000)
        .saturating_add(now.tv_nsec.max(0) as u64 / 1000);
    if out.publish_json(at_us, value).is_err() {
        *errors = errors.saturating_add(1);
    }
}

/// Production adapter. CLI configuration and environment are inspected only
/// after the default-off check. Harness owns account/session provisioning.
pub fn serve<C: Clock>(
    config: Config,
    clock: C,
    executable: PathBuf,
    as_person: bool,
    folder_missing: bool,
    engines_ready: bool,
    stop: Arc<AtomicBool>,
) -> io::Result<()> {
    let cancelled = stop.clone();
    // Only a program that is not there at all: one there and not fit to
    // run is refused as before.
    let absent = config.set_up_later.is_some()
        && fs::symlink_metadata(&executable)
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound);
    if absent {
        eprintln!(
            "tasks: no Harness program at {}; hand-overs are off",
            executable.display()
        );
    }
    serve_with_adapter(config, clock, stop, move |session| {
        if absent {
            return Ok(Production::Absent);
        }
        let mut cli = CliConfig::new(executable);
        cli.cancel = Some(cancelled);
        cli.as_person = as_person;
        cli.folder_missing = folder_missing;
        cli.engines_ready = engines_ready;
        Cli::new(cli, session.into()).map(|cli| Production::Cli(Box::new(cli)))
    })
}

/// Injection boundary for scripted host tests. Implementations must bound and
/// cancel their local work; the service cannot forcibly stop a Rust thread.
pub fn serve_with_adapter<C: Clock, A: Adapter>(
    config: Config,
    clock: C,
    stop: Arc<AtomicBool>,
    make_adapter: impl FnOnce(&str) -> io::Result<A>,
) -> io::Result<()> {
    if !config.enabled {
        return Ok(());
    }
    if config.socket == config.voice_socket {
        return Err(invalid("public and trusted sockets must differ"));
    }
    let began = Instant::now();
    let sample = clock.sample()?;
    let mut engine = Engine::open(
        Store::open(&config.dir)?,
        config.targets,
        &random_id()?,
        &random_id()?,
        &sample,
    )
    .map_err(io::Error::other)?;
    engine.close_told(config.close_told);
    if let Some(version) = config.targets_newer {
        engine.targets_newer(version);
    }
    let adapter = make_adapter(engine.store_id())?;
    if adapter.absent() {
        engine.no_harness();
    }
    let sign_in_starter = if config.sign_in_by_code {
        let starter = adapter.sign_in_starter();
        if starter.is_none() {
            eprintln!(
                "tasks: --sign-in-by-code asked, but this adapter has no sign-in; it stays off"
            );
        }
        starter
    } else {
        None
    };
    let lane = if config.push_wait {
        let lane = adapter.wait_lane();
        if lane.is_none() {
            eprintln!(
                "tasks: --push-wait asked, but this adapter has no wait; the watch looks as before"
            );
        }
        lane
    } else {
        None
    };
    let current = Arc::new(Mutex::new(Report {
        updated: Instant::now(),
        summary: engine.summary(),
        requests: 0,
        cli_busy: false,
        status: None,
        reads: None,
        set_up: None,
    }));
    let (public, _public_binding) = bind(&config.socket)?;
    let (trusted, _trusted_binding) = bind(&config.voice_socket)?;
    let bus = Bus::new(&config.bus_root)?;
    let mut diagnostic =
        bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), "tasks")?;
    // Without `--progress` the topic is not touched at all.
    let mut status_out = if config.progress {
        Some(bus.publisher_declared(TaskStatus::TOPIC, &TaskStatus::declaration(), "tasks")?)
    } else {
        None
    };
    let mut status_published = 0u64;
    let _stopping = StopOnDrop(stop.clone());
    let (jobs_tx, jobs_rx) = mpsc::sync_channel(1);
    let (outcomes_tx, outcomes_rx) = mpsc::sync_channel(1);
    let cli_stop = stop.clone();
    let cli = thread::Builder::new()
        .name("tasks-harness".into())
        .spawn(move || {
            while !cli_stop.load(Ordering::Acquire) {
                match jobs_rx.recv_timeout(TICK) {
                    Ok(job) => {
                        let result = match job {
                            Job::Launch(job) => {
                                Outcome::Launch(job.task_id, adapter.launch(&job.launch))
                            }
                            Job::Observe(job) => {
                                let result = adapter.observe(&job.machine_id, &job.agent_id);
                                Outcome::Observe(job, result)
                            }
                            Job::Status(machine) => {
                                let result = adapter.status(&machine);
                                Outcome::Status(machine, result)
                            }
                            Job::Close(job) => {
                                let result = close_told(&adapter, &job);
                                Outcome::Close(job, result)
                            }
                            Job::SignIn => Outcome::SignIn(adapter.signed_in()),
                            Job::Machines => Outcome::Machines(adapter.machines()),
                            Job::StartDaemon => Outcome::StartDaemon(adapter.start_daemon()),
                        };
                        if outcomes_tx.try_send(result).is_err() {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })?;
    let (owner_tx, owner_rx) = mpsc::sync_channel(REQUEST_QUEUE);
    let owner_stop = stop.clone();
    let switches = (config.watch, config.progress, config.close_told);
    let daemon_retry_after = config.daemon_retry_after;
    let set_up_later = config.set_up_later.clone();
    let report = current.clone();
    let owner = thread::Builder::new()
        .name("tasks-store".into())
        .spawn(move || {
            owner(
                engine,
                clock,
                owner_rx,
                (jobs_tx, outcomes_rx),
                report,
                owner_stop,
                (
                    switches.0,
                    switches.1,
                    lane,
                    switches.2,
                    set_up_later,
                    sign_in_starter,
                    daemon_retry_after,
                ),
            )
        })?;
    let (clients_tx, clients_rx) = mpsc::sync_channel::<(UnixStream, Instant, bool)>(CLIENT_QUEUE);
    let clients_rx = Arc::new(Mutex::new(clients_rx));
    let rejected = Arc::new(AtomicU64::new(0));
    let mut workers = Vec::new();
    for number in 0..WORKERS {
        let incoming = clients_rx.clone();
        let writer = owner_tx.clone();
        let stopped = stop.clone();
        workers.push(
            thread::Builder::new()
                .name(format!("tasks-client-{number}"))
                .spawn(move || {
                    while !stopped.load(Ordering::Acquire) {
                        if let Some((client, accepted, trusted)) =
                            incoming.lock().ok().and_then(|r| r.try_recv().ok())
                        {
                            serve_client(client, accepted, trusted, &writer);
                        } else {
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
                })?,
        );
    }
    let mut last_publish: Option<Instant> = None;
    let mut last_ready = None;
    let mut errors = 0u64;
    let mut failure = None;
    while !stop.load(Ordering::Acquire)
        && config.run_for.is_none_or(|limit| began.elapsed() < limit)
    {
        for (listener, trusted) in [(&public, false), (&trusted, true)] {
            for _ in 0..WORKERS + CLIENT_QUEUE {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if clients_tx
                            .try_send((stream, Instant::now(), trusted))
                            .is_err()
                        {
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
        }
        if let Ok(locked) = current.try_lock() {
            let report = locked.clone();
            drop(locked);
            if let (Some(out), Some((number, status))) = (status_out.as_mut(), &report.status)
                && *number > status_published
            {
                publish(out, status.as_ref(), &mut errors);
                status_published = *number;
            }
            let ready = report.updated.elapsed() <= STALE_AFTER
                && !owner.is_finished()
                && !cli.is_finished();
            if last_publish.is_none_or(|last| last.elapsed() >= config.report_every)
                || last_ready != Some(ready)
            {
                // The newest hand-over did not reach Harness: with no
                // internet, every one fails while the store is well (#830).
                let handover_failed = report.summary["last_handover"] == "failed";
                // Targets for a newer build, and a store kept aside: the
                // part runs, and says so until someone looks (#1505).
                let targets_newer = report.summary["targets_newer"].as_u64();
                let set_aside = report.summary["set_aside"].as_str().map(str::to_owned);
                // Stored tasks a newer build wrote that this one can't read:
                // kept as they are, and not followed (#1680).
                let unread = report.summary["unread"].as_u64().unwrap_or(0);
                // With `--set-up-later`: why hand-overs are off, or will
                // fail, while the lamp is not yet set up for them (#1676).
                // Not set up is not unwell; a targets file that is not
                // valid is.
                let set_up = report.set_up;
                let targets_invalid = set_up.is_some_and(|found| found.targets_invalid);
                let off = set_up.and_then(|found| {
                    if report.summary["no_harness"] == true {
                        Some(("no_harness", "Harness is not installed on this lamp; hand-overs are off until it is"))
                    } else if targets_newer.is_some() {
                        None
                    } else if report.summary["targets"] == 0 {
                        Some(("no_computer", "No computer to hand tasks to yet: the task targets are missing or empty"))
                    } else if found.sign_in == Some(SignIn::Out) {
                        Some(("not_signed_in", "Harness is not signed in to the person's account; hand-overs fail until it is"))
                    } else {
                        None
                    }
                });
                let level = if !ready {
                    Level::Error
                } else if errors > 0
                    || targets_newer.is_some()
                    || targets_invalid
                    || set_aside.is_some()
                    || unread > 0
                    || handover_failed
                {
                    Level::Warn
                } else {
                    Level::Ok
                };
                let newer_message = targets_newer.map(|version| match version {
                    // Version 1 with a field or an engine this build does
                    // not know: a newer build's, or a hand-made typo.
                    1 => "The task targets have a field or an engine this build doesn't know (from a newer build, or mistyped); hand-overs are off until they match this build".to_owned(),
                    version => format!("The task targets were written for a newer build (version {version}); hand-overs are off until they match this build"),
                });
                let message = if !ready {
                    "Tasks owner or CLI worker is unavailable"
                } else if errors > 0 {
                    "Tasks has bus publication failures"
                } else if let Some(message) = newer_message.as_deref() {
                    message
                } else if let Some((_, message)) = off {
                    message
                } else if targets_invalid {
                    "The task targets file changed and is not valid; the targets before it are kept"
                } else if set_aside.is_some() {
                    "Tasks set aside a store from a newer build; its tasks are not followed"
                } else if unread > 0 {
                    "Tasks kept stored tasks from a newer build that this build can't read; they are not followed"
                } else if handover_failed {
                    "Tasks could not hand the last task over"
                } else {
                    ""
                };
                let mut value = Diagnostic::new(level, "tasks", message)
                    .with("store_ready", ready)
                    .with("cli_busy", report.cli_busy)
                    .with("requests", report.requests)
                    .with("publish_errors", errors)
                    .with("clients_refused", rejected.load(Ordering::Relaxed))
                    .with("completion_supported", false);
                for field in [
                    "retained",
                    "queued",
                    "attempting",
                    "accepted",
                    "pending",
                    "uncertain",
                    "observations",
                    "speech_spoken",
                    "speech_uncertain",
                ] {
                    if let Some(count) = report.summary[field].as_u64() {
                        value = value.with(field, count);
                    }
                }
                if let Some(version) = targets_newer {
                    value = value.with("targets_newer", version);
                }
                if let Some(name) = set_aside.as_deref() {
                    value = value.with("set_aside", name);
                }
                if unread > 0 {
                    value = value.with("unread", unread);
                }
                if let Some(found) = set_up {
                    if let Some((reason, _)) = off {
                        value = value.with("handovers_off", reason);
                    }
                    if let Some(sign_in) = found.sign_in {
                        value = value.with("signed_in", sign_in.name());
                    }
                    if found.targets_invalid {
                        value = value.with("targets_invalid", true);
                    }
                }
                // Only with `--progress`: without it the report is as it was.
                if let Some(reads) = report.reads {
                    value = value
                        .with("status_reads", reads.reads)
                        .with("status_failures", reads.failures)
                        .with("status_last_ms", reads.last_ms)
                        .with("status_most_ms", reads.most_ms)
                        .with("status_published", status_published);
                }
                publish(&mut diagnostic, &value, &mut errors);
                // The status topic promises no period: it says it is alive.
                if let Some(out) = status_out.as_mut()
                    && out.alive().is_err()
                {
                    errors = errors.saturating_add(1);
                }
                last_publish = Some(Instant::now());
                last_ready = Some(ready);
            }
        }
        // A worker that ended because the service was asked to stop has
        // not failed: the stop can come between this loop's check and
        // here. One that failed on its own still says so when joined.
        if !stop.load(Ordering::Acquire) && (owner.is_finished() || cli.is_finished()) {
            failure = Some(io::Error::other("Tasks worker stopped"));
        }
        if failure.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    stop.store(true, Ordering::Release);
    drop(clients_tx);
    drop(owner_tx);
    for worker in workers {
        let _ = worker.join();
    }
    let shutdown = Instant::now();
    while (!owner.is_finished() || !cli.is_finished())
        && shutdown.elapsed() < Duration::from_millis(300)
    {
        thread::sleep(Duration::from_millis(5));
    }
    if !owner.is_finished() || !cli.is_finished() {
        return Err(io::Error::other(
            "Tasks worker did not stop; exit this process before restarting",
        ));
    }
    cli.join()
        .map_err(|_| io::Error::other("Tasks CLI worker panicked"))?;
    owner
        .join()
        .map_err(|_| io::Error::other("Tasks store worker panicked"))??;
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// A lane whose waits are held until cancelled; the first one called
    /// answers its cancel 300 ms late, as a slow child's reaping can.
    #[derive(Default)]
    struct SlowLane {
        calls: AtomicU64,
        running: AtomicU64,
    }
    impl WaitLane for SlowLane {
        fn wait(&self, _: &str, _: &[String], cancel: &AtomicBool) -> Result<Vec<Ended>, String> {
            let first = self.calls.fetch_add(1, Ordering::SeqCst) == 0;
            self.running.fetch_add(1, Ordering::SeqCst);
            while !cancel.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(2));
            }
            if first {
                thread::sleep(Duration::from_millis(300));
            }
            self.running.fetch_sub(1, Ordering::SeqCst);
            Err("cancelled".into())
        }
    }
    fn watched(agents: &[&str]) -> Vec<Watched> {
        agents
            .iter()
            .enumerate()
            .map(|(at, agent)| Watched {
                task_id: at as u64 + 1,
                machine_id: "m".into(),
                agent_id: (*agent).into(),
                engine: "claude".into(),
                accepted_ago_ms: Some(0),
            })
            .collect()
    }
    fn settle(lane: &SlowLane, started: u64) {
        let until = Instant::now() + Duration::from_secs(2);
        while lane.calls.load(Ordering::SeqCst) < started {
            assert!(Instant::now() < until, "wait {started} never started");
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// #1157 review: {A}, then {A, B}, then {A} again. The first wait,
    /// cancelled, answers after the third began on the same agents; that
    /// late answer must not untrack the third and leave two held.
    #[test]
    fn a_cancelled_waits_late_answer_never_untracks_the_wait_that_replaced_it() {
        let lane = Arc::new(SlowLane::default());
        let mut waits = Waits::new(Some(lane.clone()));
        waits.tend(&watched(&["a"]), &BTreeSet::new(), Instant::now());
        settle(&lane, 1);
        waits.tend(&watched(&["a", "b"]), &BTreeSet::new(), Instant::now());
        settle(&lane, 2);
        waits.tend(&watched(&["a"]), &BTreeSet::new(), Instant::now());
        settle(&lane, 3);
        // Long enough for the first to answer, tending all the while.
        let until = Instant::now() + Duration::from_millis(600);
        while Instant::now() < until {
            waits.tend(&watched(&["a"]), &BTreeSet::new(), Instant::now());
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            lane.calls.load(Ordering::SeqCst),
            3,
            "a fourth wait started"
        );
        assert_eq!(lane.running.load(Ordering::SeqCst), 1);
        assert_eq!(waits.held.len(), 1);
        // The owner going away ends the one still held.
        drop(waits);
        let until = Instant::now() + Duration::from_secs(2);
        while lane.running.load(Ordering::SeqCst) > 0 {
            assert!(Instant::now() < until, "a wait outlived its owner");
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// A lane that comes back at once naming "a" whenever it is asked about
    /// it (Harness calls it done while `show` finds it at work), and holds a
    /// wait without it until "b" is let go.
    #[derive(Default)]
    struct StuckLane {
        asked: Mutex<Vec<Vec<String>>>,
        b_ends: AtomicBool,
    }
    impl WaitLane for StuckLane {
        fn wait(
            &self,
            _: &str,
            agents: &[String],
            cancel: &AtomicBool,
        ) -> Result<Vec<Ended>, String> {
            self.asked.lock().unwrap().push(agents.to_vec());
            let end = |agent: &str| {
                Ok(vec![Ended {
                    agent_id: agent.into(),
                    state: "idle".into(),
                }])
            };
            if agents.iter().any(|agent| agent == "a") {
                return end("a");
            }
            while !cancel.load(Ordering::Acquire) {
                if self.b_ends.load(Ordering::Acquire) {
                    return end("b");
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err("cancelled".into())
        }
    }

    /// The reviewer on #1157: task 1's agent is stuck (Harness calls it done,
    /// `show` finds it unanswered) and task 2 is on the same machine. Once
    /// task 1 is pushed, its agent is left out until the watch's own read of
    /// it, and task 2's end is still pushed within WAIT_AGAIN and a few ticks.
    #[test]
    fn one_stuck_agent_never_holds_up_the_others_on_its_machine() {
        let lane = Arc::new(StuckLane::default());
        let mut waits = Waits::new(Some(lane.clone()));
        let both = watched(&["a", "b"]);
        let start = Instant::now();
        let tend_until = |waits: &mut Waits,
                          pushed: &BTreeSet<u64>,
                          at: Instant,
                          done: &dyn Fn(&Waits) -> bool| {
            let until = Instant::now() + Duration::from_secs(2);
            while !done(waits) {
                assert!(
                    Instant::now() < until,
                    "asked: {:?}",
                    lane.asked.lock().unwrap()
                );
                waits.tend(&both, pushed, at);
                thread::sleep(Duration::from_millis(2));
            }
        };
        // The first wait names "a" at once: task 1 is pushed.
        tend_until(&mut waits, &BTreeSet::new(), start, &|w| {
            w.pushes.contains(&1)
        });
        // Its push is taken, and the read leaves it watched: from now on it
        // is a pushed task, left out until the watch reads it again.
        waits.pushes.remove(&1);
        let pushed = BTreeSet::from([1]);
        let after = start + WAIT_AGAIN;
        tend_until(&mut waits, &pushed, after, &|w| !w.held.is_empty());
        assert_eq!(
            *lane.asked.lock().unwrap(),
            vec![vec!["a".to_owned(), "b".to_owned()], vec!["b".to_owned()]]
        );
        // Task 2's agent ends: pushed at once, not 30 s later.
        lane.b_ends.store(true, Ordering::Release);
        tend_until(&mut waits, &pushed, after, &|w| w.pushes.contains(&2));
        // The watch reads task 1 again (no longer a pushed read): it is
        // waited on once more.
        waits.pushes.remove(&2);
        let watch_read = after + WAIT_AGAIN;
        tend_until(&mut waits, &BTreeSet::new(), watch_read, &|w| {
            w.pushes.contains(&1)
        });
        assert_eq!(
            lane.asked.lock().unwrap().last().unwrap(),
            &vec!["a".to_owned(), "b".to_owned()]
        );
    }

    fn root() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("tsk-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in("/private/tmp")
            .or_else(|_| {
                tempfile::Builder::new()
                    .prefix("tsk-")
                    .permissions(fs::Permissions::from_mode(0o700))
                    .tempdir()
            })
            .unwrap()
    }
    fn targets() -> Value {
        json!({"version":1,"targets":{"coding":{"machine_id":"11111111111111111111111111111111","project":"project","engine":"codex"}}})
    }
    fn write_targets(path: &Path, value: &Value) {
        fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn configuration_is_private_bounded_strict_and_has_explicit_targets() {
        let dir = root();
        let path = dir.path().join("targets.json");
        write_targets(&path, &targets());
        let value = read_targets(&path).unwrap();
        assert_eq!(value.len(), 1);
        assert_eq!(value["coding"].project, "project");
        for mode in [0o644, 0o660, 0o1600] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert!(read_targets(&path).is_err());
        }
        write_targets(&path, &targets());
        let link = dir.path().join("linked");
        symlink(&path, &link).unwrap();
        assert!(read_targets(&link).is_err());
        let hard = dir.path().join("hard");
        fs::hard_link(&path, &hard).unwrap();
        assert!(read_targets(&path).is_err());
        fs::remove_file(hard).unwrap();
        assert!(read_targets(Path::new("targets.json")).is_err());
        fs::write(&path, vec![b' '; 16_385]).unwrap();
        assert!(read_targets(&path).is_err());
    }

    /// #1505: targets written for a newer build, a `version` above 1 or a
    /// field this build does not know (at the top or in a target), are
    /// told apart from a file that is not a targets file: the part can run
    /// with hand-overs off and say why. They are never read as targets.
    #[test]
    fn targets_for_a_newer_build_are_told_apart_and_never_read() {
        let dir = root();
        let path = dir.path().join("targets.json");
        let target = targets()["targets"]["coding"].clone();
        let mut later = target.clone();
        later["read_only"] = json!(true);
        for (value, newer) in [
            (json!({"version":2,"targets":{"coding":target}}), 2),
            (json!({"version":7,"targets":{}}), 7),
            (
                json!({"version":1,"targets":{"coding":target},"trusted":true}),
                1,
            ),
            (json!({"version":1,"targets":{"coding":later}}), 1),
            // An engine this build does not know (as `opencode` came).
            (
                json!({"version":1,"targets":{"coding":{"machine_id":"11111111111111111111111111111111","project":"project","engine":"a_later_engine"}}}),
                1,
            ),
        ] {
            write_targets(&path, &value);
            assert_eq!(read_targets_or_newer(&path).unwrap(), Err(newer), "{value}");
            assert!(read_targets(&path).is_err(), "{value}");
        }
        for value in [
            json!({"targets":{"coding":target}}),
            json!({"version":"1","targets":{}}),
            json!({"version":1,"targets":{}}),
            json!({"version":1,"targets":{"bad alias":target}}),
        ] {
            write_targets(&path, &value);
            assert!(read_targets_or_newer(&path).is_err(), "{value}");
        }
        write_targets(&path, &targets());
        assert_eq!(read_targets_or_newer(&path).unwrap().unwrap().len(), 1);
        // Devin is this build's (#1854), not a later engine's.
        write_targets(
            &path,
            &json!({"version":1,"targets":{"devin":{"machine_id":"11111111111111111111111111111111","project":"project","engine":"devin"}}}),
        );
        assert_eq!(
            read_targets(&path).unwrap()["devin"].engine,
            EngineKind::Devin
        );
    }

    #[test]
    fn configuration_never_silently_selects_the_last_duplicate_alias() {
        let dir = root();
        let path = dir.path().join("targets.json");
        let target = targets()["targets"]["coding"].clone();
        fs::write(
            &path,
            format!(r#"{{"version":1,"targets":{{"coding":{target},"coding":{target}}}}}"#),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_targets(&path).is_err());
        for value in [
            json!({"version":2,"targets":{}}),
            json!({"version":1,"targets":{}}),
            json!({"version":1,"targets":{"bad alias":target}}),
            json!({"version":1,"targets":{"coding":target},"trusted":true}),
        ] {
            write_targets(&path, &value);
            assert!(read_targets(&path).is_err());
        }
    }

    #[test]
    fn binding_preserves_foreign_files_and_live_sockets() {
        let dir = root();
        let path = dir.path().join("socket");
        fs::write(&path, "preserve").unwrap();
        assert!(bind(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "preserve");
        fs::remove_file(&path).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        assert!(bind(&path).is_err());
        assert!(path.exists());
        drop(listener);
        let (listener, binding) = bind(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert!(bind(&path).is_err());
        drop(listener);
        drop(binding);
        assert!(!path.exists());
    }

    #[test]
    fn binding_cleanup_does_not_delete_a_replacement() {
        let dir = root();
        let path = dir.path().join("socket");
        let (_listener, binding) = bind(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::write(&path, "replacement").unwrap();
        drop(binding);
        assert_eq!(fs::read_to_string(path).unwrap(), "replacement");
    }

    #[test]
    fn disabled_service_does_not_read_clock_adapter_or_storage() {
        struct Never;
        impl Clock for Never {
            fn sample(&self) -> io::Result<ClockSample> {
                panic!("clock read while disabled")
            }
        }
        impl Adapter for Never {
            fn launch(&self, _: &Launch) -> LaunchOutcome {
                panic!("launch while disabled")
            }
            fn observe(&self, _: &str, _: &str) -> Result<Observation, String> {
                panic!("observe while disabled")
            }
        }
        let dir = root();
        let config = Config {
            enabled: false,
            dir: dir.path().join("data"),
            socket: dir.path().join("public"),
            voice_socket: dir.path().join("voice"),
            bus_root: dir.path().join("bus"),
            targets: BTreeMap::new(),
            run_for: None,
            watch: false,
            progress: false,
            push_wait: false,
            close_told: false,
            targets_newer: None,
            set_up_later: None,
            sign_in_by_code: false,
            report_every: REPORT_EVERY,
            daemon_retry_after: DAEMON_RETRY_AFTER,
        };
        serve_with_adapter::<_, Never>(config, Never, Arc::new(AtomicBool::new(false)), |_| {
            panic!("adapter configured while disabled")
        })
        .unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn line_reader_drains_buffered_bytes_after_peer_close() {
        let (mut sender, mut reader) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let payload = vec![b'x'; 4096];
        sender.write_all(&payload).unwrap();
        sender.write_all(b"\n").unwrap();
        drop(sender);
        assert_eq!(
            read_line(&mut reader, 4096, Instant::now() + Duration::from_secs(1)).unwrap(),
            payload
        );
    }

    #[test]
    fn fragmented_line_and_absolute_silent_deadline_are_bounded() {
        let (mut sender, mut reader) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let writer = thread::spawn(move || {
            sender.write_all(b"a").unwrap();
            thread::sleep(Duration::from_millis(10));
            sender.write_all(b"b\n").unwrap();
        });
        assert_eq!(
            read_line(&mut reader, 2, Instant::now() + Duration::from_secs(1)).unwrap(),
            b"ab"
        );
        writer.join().unwrap();
        let (_sender, mut reader) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let start = Instant::now();
        assert_eq!(
            read_line(&mut reader, 2, start + Duration::from_millis(30))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn newline_and_byte_bounds_precede_parsing() {
        for (bytes, bound) in [(b"abc\n".as_slice(), 2), (b"a\nb\n", 20), (b"abc", 2)] {
            let (mut sender, mut reader) = UnixStream::pair().unwrap();
            reader.set_nonblocking(true).unwrap();
            sender.write_all(bytes).unwrap();
            drop(sender);
            assert!(
                read_line(&mut reader, bound, Instant::now() + Duration::from_secs(1)).is_err()
            );
        }
    }

    #[test]
    fn output_to_a_nonreading_client_has_one_deadline() {
        let (mut sender, _reader) = UnixStream::pair().unwrap();
        sender.set_nonblocking(true).unwrap();
        let start = Instant::now();
        assert_eq!(
            write_bytes(
                &mut sender,
                &vec![b'x'; 2 * 1024 * 1024],
                start + Duration::from_millis(30)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn request_rejects_oversized_input_before_connecting() {
        let value = json!({"text":"x".repeat(MAX_LINE_BYTES)});
        assert_eq!(
            request(
                Path::new("/no-tasks-socket"),
                &value,
                Duration::from_secs(1)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    fn machine(id: char, label: &str, current: bool) -> AccountMachine {
        AccountMachine {
            machine_id: id.to_string().repeat(32),
            label: label.into(),
            current,
        }
    }

    /// #1676: the lamp itself is left out; the first is `office`, the
    /// voice's own default; the rest by their names made into aliases,
    /// each once; at most 8.
    #[test]
    fn targets_from_machines_name_the_first_office_and_the_rest_by_name() {
        let listed = [
            machine('a', "lamp", true),
            machine('b', "Office iMac", false),
            machine('c', "MacBook Air", false),
            machine('d', "macbook   air!!", false),
            machine('e', "", false),
            machine('f', "Café PC — 2", false),
        ];
        let targets = targets_from_machines(&listed, EngineKind::Claude);
        let aliases: Vec<_> = targets.keys().cloned().collect();
        assert_eq!(
            aliases,
            [
                "caf-pc-2",
                "computer",
                "macbook-air",
                "macbook-air-2",
                "office"
            ]
        );
        assert_eq!(targets["office"].machine_id, "b".repeat(32));
        assert_eq!(targets["macbook-air-2"].machine_id, "d".repeat(32));
        assert!(
            targets
                .values()
                .all(|target| target.engine == EngineKind::Claude
                    && target.project == MACHINES_PROJECT
                    && target.validate().is_ok())
        );
        let many: Vec<_> = "bcdefghij0"
            .chars()
            .map(|id| machine(id, "pc", false))
            .collect();
        let eight = targets_from_machines(&many, EngineKind::Codex);
        assert_eq!(eight.len(), 8);
        assert!(!eight.values().any(|t| t.machine_id == "0".repeat(32)));
        assert!(
            targets_from_machines(&[machine('a', "lamp", true)], EngineKind::Claude).is_empty()
        );
        let long = alias_of(&"x".repeat(80));
        assert_eq!(long.len(), 32);
    }

    /// #1676: the part writes the targets file only where it is its own: none
    /// there, or one it wrote. What it writes is read back as strictly as a
    /// file a person wrote. A file someone else wrote is left as it is, and
    /// with no machine but the lamp, its own file is removed.
    #[test]
    fn targets_are_written_from_machines_only_where_the_file_is_the_parts_own() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("targets.json");
        let one = [
            machine('a', "lamp", true),
            machine('b', "Office iMac", false),
        ];
        let two = [
            machine('a', "lamp", true),
            machine('b', "Office iMac", false),
            machine('c', "MacBook Air", false),
        ];
        let write = |listed: &[AccountMachine]| {
            write_targets_from_machines(&path, listed, EngineKind::Claude).unwrap()
        };
        assert!(write(&one).changed);
        let read = read_targets(&path).unwrap();
        assert_eq!(read.keys().collect::<Vec<_>>(), ["office"]);
        assert!(!write(&one).changed, "the same: not written again");
        assert!(write(&two).changed);
        assert_eq!(read_targets(&path).unwrap().len(), 2);
        // A stop between the mark and the file: the mark names both, so the
        // file there is still the part's own.
        let bytes = fs::read(&path).unwrap();
        fs::write(
            written_mark(&path),
            format!("{}\n{}\n", sha256_hex(&bytes), "0".repeat(64)),
        )
        .unwrap();
        assert!(!write(&two).changed);
        // No machine but the lamp: its own file goes.
        assert!(write(&[machine('a', "lamp", true)]).changed);
        assert!(!path.exists());
        // A file someone wrote is never touched, then or later.
        let theirs = br#"{"version":1,"targets":{"desk":{"machine_id":"cccccccccccccccccccccccccccccccc","project":"mine","engine":"codex"}}}"#;
        fs::write(&path, theirs).unwrap();
        for listed in [&one[..], &two[..], &[machine('a', "lamp", true)][..]] {
            let written = write(listed);
            assert!(!written.changed, "{written:?}");
            assert_eq!(fs::read(&path).unwrap(), theirs);
        }
    }

    /// #1676: each computer's own name, cleaned to a label of at most 64
    /// bytes; none where nothing is left.
    #[test]
    fn names_from_machines_are_clean_labels_by_alias() {
        let long = "Ä".repeat(40);
        let listed = [
            machine('a', "lamp", true),
            machine('b', "  Office iMac\n", false),
            machine('c', "MacBook Air", false),
            machine('d', "", false),
            machine('e', &long, false),
        ];
        let names = names_from_machines(&listed);
        assert_eq!(names["office"], "Office iMac");
        assert_eq!(names["macbook-air"], "MacBook Air");
        assert!(!names.contains_key("computer"));
        let cut = names.values().find(|name| name.starts_with('Ä')).unwrap();
        assert_eq!(cut.len(), 64);
        assert_eq!(cut.chars().count(), 32);
    }

    /// #1676: the names file is read as strictly as the targets; anything
    /// else is an error, so the voice says what it says today.
    #[test]
    fn the_names_file_is_read_strictly() {
        let dir = tempfile::tempdir().unwrap();
        let targets = dir.path().join("targets.json");
        let path = names_path(&targets);
        assert_eq!(path, dir.path().join("targets.names.json"));
        let put = |text: &str, mode: u32| {
            let _ = fs::remove_file(&path);
            fs::write(&path, text).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        };
        put(
            r#"{"version":1,"names":{"office":"Office iMac","macbook-air":"MacBook Air"}}"#,
            0o600,
        );
        let names = read_target_names(&path).unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names["office"], "Office iMac");
        let long = "x".repeat(65);
        for bad in [
            r#"{"version":2,"names":{"office":"Office iMac"}}"#.to_owned(),
            r#"{"version":1,"names":{"office":"Office iMac"},"more":1}"#.to_owned(),
            r#"{"version":1,"names":{"office":"A","office":"B"}}"#.to_owned(),
            r#"{"version":1,"names":{"office":7}}"#.to_owned(),
            r#"{"version":1,"names":{"office":" Office"}}"#.to_owned(),
            r#"{"version":1,"names":{"office":"Office\u0007"}}"#.to_owned(),
            r#"{"version":1,"names":{"office":""}}"#.to_owned(),
            r#"{"version":1,"names":{"the office":"Office"}}"#.to_owned(),
            format!(r#"{{"version":1,"names":{{"office":"{long}"}}}}"#),
            "not json".to_owned(),
        ] {
            put(&bad, 0o600);
            assert!(read_target_names(&path).is_err(), "{bad}");
        }
        put(r#"{"version":1,"names":{"office":"Office iMac"}}"#, 0o644);
        assert!(read_target_names(&path).is_err(), "a file others may read");
    }

    /// #1676: the names go beside the targets the part writes, change when a
    /// computer is renamed though its alias stays, and go with them; beside a
    /// file someone else wrote, nothing is written.
    #[test]
    fn names_are_written_beside_the_parts_own_targets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("targets.json");
        let names = names_path(&path);
        let write = |listed: &[AccountMachine]| {
            write_targets_from_machines(&path, listed, EngineKind::Claude).unwrap()
        };
        assert!(
            write(&[
                machine('a', "lamp", true),
                machine('b', "Office iMac", false)
            ])
            .changed
        );
        assert_eq!(read_target_names(&names).unwrap()["office"], "Office iMac");
        let renamed = write(&[machine('a', "lamp", true), machine('b', "Studio", false)]);
        assert!(renamed.changed, "{renamed:?}");
        assert!(renamed.said.contains("their names written"), "{renamed:?}");
        assert_eq!(read_target_names(&names).unwrap()["office"], "Studio");
        assert!(!write(&[machine('a', "lamp", true), machine('b', "Studio", false)]).changed);
        assert!(write(&[machine('a', "lamp", true), machine('b', "", false)]).changed);
        assert!(!names.exists(), "no name left: no file");
        assert!(write(&[machine('a', "lamp", true)]).changed);
        assert!(!path.exists() && !names.exists());
        let theirs = br#"{"version":1,"targets":{"desk":{"machine_id":"cccccccccccccccccccccccccccccccc","project":"mine","engine":"codex"}}}"#;
        fs::write(&path, theirs).unwrap();
        assert!(!write(&[machine('b', "Office iMac", false)]).changed);
        assert!(!names.exists());
    }
}
