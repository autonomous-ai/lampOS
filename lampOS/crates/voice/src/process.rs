//! Finite child lifetimes and private IPC. A failed child ends a qualification
//! session; there is no silent restart that could reuse old conversation state.
use crate::{
    diagnostic_control::DiagnosticStarted,
    transport::WorkerChannels,
    wire::{Control, WorkerEvent},
};
use lamp_turn_owner::BootId;
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

mod ring_startup;
use ring_startup::RingResources;
pub(crate) use ring_startup::RingStartup;

pub const WORKER_STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a worker whose peer went during startup is waited for to end
/// ([`Worker::ended_or`]).
const ENDED_WITHIN: Duration = Duration::from_millis(100);

/// The clock a worker's startup wait reads its deadline on: the host's. A
/// test build adds what a test has moved it on by (`startup_clock`), so a
/// test of the deadline need not wait it out.
fn startup_now() -> Instant {
    #[cfg(test)]
    {
        Instant::now() + startup_clock::moved()
    }
    #[cfg(not(test))]
    {
        Instant::now()
    }
}

/// How far this test's thread has moved the startup wait's clock on.
#[cfg(test)]
pub(crate) mod startup_clock {
    use std::{cell::Cell, time::Duration};

    thread_local! {
        static MOVED: Cell<Duration> = const { Cell::new(Duration::ZERO) };
    }

    pub(crate) fn moved() -> Duration {
        MOVED.with(Cell::get)
    }

    /// Move it on by `by`: the wait runs on the thread that started it.
    pub(crate) fn move_on(by: Duration) {
        MOVED.with(|moved| moved.set(moved.get() + by));
    }
}

pub fn new_boot() -> io::Result<BootId> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    BootId::new(bytes).map_err(io::Error::other)
}

pub fn boot_text(boot: BootId) -> String {
    boot.bytes().iter().map(|b| format!("{b:02x}")).collect()
}
pub fn parse_boot(input: &str) -> io::Result<BootId> {
    if input.len() != 32 || !input.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid boot ID",
        ));
    }
    let mut bytes = [0; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&input[index * 2..index * 2 + 2], 16).map_err(io::Error::other)?;
    }
    BootId::new(bytes).map_err(io::Error::other)
}

pub struct SessionDirectory {
    pub path: PathBuf,
}
impl SessionDirectory {
    pub fn create() -> io::Result<Self> {
        let path = PathBuf::from("/tmp").join(format!("lampOS-{}", boot_text(new_boot()?)));
        fs::create_dir(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        Ok(Self { path })
    }
}
impl Drop for SessionDirectory {
    fn drop(&mut self) {
        // Endpoints remove only their own inodes. Never recursively erase a path
        // a different process might have populated or replaced.
        let _ = fs::remove_dir(&self.path);
    }
}

/// A worker's process found ended cleanly, said as `"{role} worker exited:
/// {status}"`.
#[derive(Debug)]
struct EndedCleanly(String);

impl std::fmt::Display for EndedCleanly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EndedCleanly {}

pub struct Worker {
    pub channels: WorkerChannels,
    child: Child,
    pub boot: BootId,
    pub role: String,
    pub diagnostic_start: Option<DiagnosticStarted>,
    // Only the optional ring owns a separate startup namespace. Channels drop
    // before this directory; peer paths are removed only after verified exit.
    ring_resources: Option<RingResources>,
    /// A test fixture's own end of its child, run once Stop is sent: it closes
    /// the child's input and waits for the exit, so a busy host cannot make a
    /// fixture miss the 300 ms in `shutdown` (#1232).
    #[cfg(test)]
    on_stop: Option<Box<dyn FnOnce() -> io::Result<()> + Send>>,
    /// One-shot test barrier after native termination/reaping, before returning
    /// to the coordinator. It never replaces the real child cleanup.
    #[cfg(test)]
    after_terminate: Option<Box<dyn FnOnce() + Send>>,
}

/// The voice starts a worker only in a role on its list, the one the records
/// guard reads too (#1071): a role missing there fails here, on the
/// builder's machine, never first as a refused cleanup on a lamp.
pub fn listed(role: &str) -> io::Result<lamp_voice_processes::WorkerRole> {
    lamp_voice_processes::WorkerRole::parse(role).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("worker role `{role}` is not on the voice's list (lamp-voice-processes)"),
        )
    })
}

impl Worker {
    /// The worker's process id, for a test that reads how it runs (#1608).
    pub fn process_id(&self) -> u32 {
        self.child.id()
    }

    #[cfg(test)]
    pub(crate) fn sleeping_fixture(directory: &Path, role: &str) -> (Self, WorkerChannels) {
        Self::command_fixture(directory, role, "exec sleep 60")
    }

    #[cfg(test)]
    pub(crate) fn command_fixture(
        directory: &Path,
        role: &str,
        command: &str,
    ) -> (Self, WorkerChannels) {
        let parent = new_boot().unwrap();
        let boot = new_boot().unwrap();
        let mut channels = WorkerChannels::bind(directory, role, parent, boot, false).unwrap();
        let mut peer = WorkerChannels::bind(directory, role, parent, boot, true).unwrap();
        channels.connect(directory, role, false).unwrap();
        peer.connect(directory, role, true).unwrap();
        // QEMU can return from spawn before the child's close-on-exec socket
        // references are released. The marker proves the first shell is running,
        // including closure of peer sockets belonging to earlier test fixtures.
        let mut child = Command::new("sh")
            .args([
                "-c",
                "printf R; exec >/dev/null; exec sh -c \"$1\"",
                "lamp-fixture",
                command,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        fixture_exec_barrier(&mut child, FIXTURE_EXEC_WAIT).unwrap();
        (
            Self {
                channels,
                child,
                boot,
                role: role.into(),
                diagnostic_start: None,
                ring_resources: None,
                #[cfg(test)]
                on_stop: None,
                #[cfg(test)]
                after_terminate: None,
            },
            peer,
        )
    }

    #[cfg(test)]
    pub(crate) fn cooperative_fixture(
        directory: &Path,
        role: &str,
    ) -> (Self, WorkerChannels, std::process::ChildStdin) {
        Self::cooperative_fixture_with_readiness(directory, role, false)
    }

    /// The stepped fixture cannot rely on the scheduler running cat before
    /// logical time advances. Its echo proves exec/input readiness only.
    #[cfg(test)]
    pub(crate) fn cooperative_ready_fixture(
        directory: &Path,
        role: &str,
    ) -> (Self, WorkerChannels, std::process::ChildStdin) {
        Self::cooperative_fixture_with_readiness(directory, role, true)
    }

    #[cfg(test)]
    pub(crate) fn fixture_pid(&self) -> u32 {
        self.child.id()
    }

    /// Sends Stop until a send is refused because the peer is gone, for at
    /// most 5 s. A child another test forks just as the peer drops holds a
    /// copy of its socket until its exec (CLOEXEC), so a first send can still
    /// be accepted, or find the queue full (#1569, as #1478).
    #[cfg(test)]
    pub(crate) fn send_stop_until_peer_gone(&mut self) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match self.channels.control.send(Control::Stop) {
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionReset
                            | io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::NotConnected
                            | io::ErrorKind::BrokenPipe
                    ) =>
                {
                    return;
                }
                sent => assert!(
                    Instant::now() < until,
                    "{}'s peer was not gone after 5 s: {sent:?}",
                    self.role
                ),
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// What ends this fixture's child once Stop has been sent to it.
    #[cfg(test)]
    pub(crate) fn on_stop(&mut self, end: impl FnOnce() -> io::Result<()> + Send + 'static) {
        self.on_stop = Some(Box::new(end));
    }

    #[cfg(test)]
    pub(crate) fn after_terminate(&mut self, observe: impl FnOnce() + Send + 'static) {
        self.after_terminate = Some(Box::new(observe));
    }

    /// Closes a cooperative fixture's input as its Stop is sent, on the
    /// stopping thread, and waits there for the child's exit (observed, not
    /// reaped), so a busy host cannot hold that exit past the shutdown's
    /// 300 ms (#1249, #1569). A peer thread that waited to read the Stop
    /// first could be scheduled too late.
    #[cfg(test)]
    pub(crate) fn end_on_stop(&mut self, input: std::process::ChildStdin) {
        let pid = self.fixture_pid();
        self.on_stop(move || {
            drop(input);
            crate::scenario_clock::wait_child_exited(pid, Instant::now() + Duration::from_secs(5))
        });
    }

    /// The fixture's `on_stop`, taken out to be run by a shutdown that stops
    /// several workers at once (#1569).
    #[cfg(test)]
    pub(crate) fn take_on_stop(&mut self) -> Option<Box<dyn FnOnce() -> io::Result<()> + Send>> {
        self.on_stop.take()
    }

    /// Runs the fixture's `on_stop`, once, right after a Stop was sent: by
    /// `shutdown`, or by a cleanup of its own (#1249). A fixture that failed
    /// to end its child is caught after, as any child is.
    #[cfg(test)]
    pub(crate) fn run_on_stop(&mut self) {
        if let Some(on_stop) = self.on_stop.take() {
            let _ = on_stop();
        }
    }

    #[cfg(test)]
    fn cooperative_fixture_with_readiness(
        directory: &Path,
        role: &str,
        ready: bool,
    ) -> (Self, WorkerChannels, std::process::ChildStdin) {
        let parent = new_boot().unwrap();
        let boot = new_boot().unwrap();
        let mut channels = WorkerChannels::bind(directory, role, parent, boot, false).unwrap();
        let mut peer = WorkerChannels::bind(directory, role, parent, boot, true).unwrap();
        channels.connect(directory, role, false).unwrap();
        peer.connect(directory, role, true).unwrap();
        // The test peer drops stdin only after observing priority Stop. The
        // ordinary child then exits zero, exercising actual supervisor reaping.
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(if ready { Stdio::piped() } else { Stdio::null() })
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        if ready {
            // The child remains owned and is killed/reaped on either write or
            // read failure. No fixture process is leaked by a startup panic.
            use std::io::Write;
            let sent = child.stdin.as_mut().expect("piped input").write_all(b"R");
            if let Err(error) = sent {
                panic!("{}", failed_fixture(&mut child, error));
            }
            fixture_exec_barrier(&mut child, FIXTURE_EXEC_WAIT).unwrap();
        }
        let input = child.stdin.take().unwrap();
        (
            Self {
                channels,
                child,
                boot,
                role: role.into(),
                diagnostic_start: None,
                ring_resources: None,
                #[cfg(test)]
                on_stop: None,
                #[cfg(test)]
                after_terminate: None,
            },
            peer,
            input,
        )
    }

    pub fn spawn(
        executable: &Path,
        directory: &Path,
        role: &str,
        parent: BootId,
        extra: &[&str],
    ) -> io::Result<Self> {
        Self::spawn_with_tick(executable, directory, role, parent, extra, || Ok(()))
    }

    pub fn spawn_with_tick(
        executable: &Path,
        directory: &Path,
        role: &str,
        parent: BootId,
        extra: &[&str],
        on_wait: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Self> {
        Self::spawn_with_boot_and_tick(
            executable,
            directory,
            role,
            parent,
            new_boot()?,
            extra,
            on_wait,
        )
    }

    /// Trusted preallocation permits mutually authenticated direct worker links.
    /// Existing spawn entry points still allocate a fresh random boot themselves.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_with_boot_and_tick(
        executable: &Path,
        directory: &Path,
        role: &str,
        parent: BootId,
        boot: BootId,
        extra: &[&str],
        on_wait: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Self> {
        Self::spawn_given(
            executable,
            directory,
            role,
            (parent, boot),
            extra,
            on_wait,
            |name| std::env::var_os(name),
        )
    }

    /// [`Self::spawn_with_boot_and_tick`] with the switches read by `var`
    /// rather than from the voice's own environment: a test's own (#1608).
    pub fn spawn_given(
        executable: &Path,
        directory: &Path,
        role: &str,
        (parent, boot): (BootId, BootId),
        extra: &[&str],
        mut on_wait: impl FnMut() -> io::Result<()>,
        var: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> io::Result<Self> {
        listed(role)?;
        let channels = WorkerChannels::bind(directory, role, parent, boot, false)?;
        let child =
            worker_command(executable, directory, role, (parent, boot), extra, var).spawn()?;
        let mut worker = Self {
            channels,
            child,
            boot,
            role: role.into(),
            diagnostic_start: None,
            ring_resources: None,
            #[cfg(test)]
            on_stop: None,
            #[cfg(test)]
            after_terminate: None,
        };
        worker.wait_until_ready(directory, &mut on_wait)?;
        Ok(worker)
    }

    /// A peer gone during startup is most often its process ending, its
    /// sockets removed before its exit can be read: that exit, read within
    /// [`ENDED_WITHIN`], says why, so a worker that ended cleanly (the
    /// privacy worker with Controls stopped) is told as such, not as the
    /// socket's `No such file or directory` (lamp-52e6, 27 Oct 17:16:43,
    /// #3697: the voice exited 1 and came back muted by the restart guard).
    /// One still running then keeps `error`.
    pub(crate) fn ended_or(&mut self, error: io::Error) -> io::Error {
        let until = Instant::now() + ENDED_WITHIN;
        loop {
            match self.check_running() {
                Err(ended) => return ended,
                Ok(()) if Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
                Ok(()) => return error,
            }
        }
    }

    fn wait_until_ready(
        &mut self,
        directory: &Path,
        mut on_wait: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        let deadline = startup_now() + WORKER_STARTUP_TIMEOUT;
        let role = self.role.clone();
        let peer_c = directory.join(format!("{role}.c.w"));
        let peer_d = directory.join(format!("{role}.d.w"));
        // Startup waits run before listening readiness is advertised. A later
        // recovery must use a separate supervisor path, never block live audio.
        loop {
            if let Some(resources) = &mut self.ring_resources {
                resources.observe_peers()?;
            }
            if peer_sockets_ready(&[&peer_c, &peer_d])? {
                break;
            }
            on_wait()?;
            self.check_running()?;
            if startup_now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "worker socket startup timed out",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Err(error) = self.channels.connect(directory, &role, false) {
            return Err(self.ended_or(error));
        }
        loop {
            on_wait()?;
            self.check_running()?;
            let received = match self.channels.control.receive() {
                Ok(received) => received,
                Err(error) => return Err(self.ended_or(error)),
            };
            match received {
                Some(WorkerEvent::Ready) => return Ok(()),
                Some(WorkerEvent::DiagnosticsStarted { started })
                    if self.diagnostic_start.is_none()
                        && matches!(role.as_str(), "capture" | "speaker") =>
                {
                    self.diagnostic_start = Some(started);
                }
                Some(_) => return Err(io::Error::other("unexpected worker startup message")),
                None => {}
            }
            if startup_now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "worker readiness timed out",
                ));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    pub fn check_running(&mut self) -> io::Result<()> {
        if let Some(status) = self.child.try_wait()? {
            let said = format!("{} worker exited: {status}", self.role);
            // Only a clean end can be the worker's own choice (#1396); a
            // panic, a signal or any other status stays an abnormal exit.
            return Err(if status.success() {
                io::Error::other(EndedCleanly(said))
            } else {
                io::Error::other(said)
            });
        }
        Ok(())
    }

    /// Whether `error` says a worker's process had ended cleanly, status 0
    /// ([`Self::check_running`]), so a caller can tell the worker's own end
    /// from a crash or any other start failure (#1396, the reviewer-lead's
    /// P2 on #2506).
    pub fn ended_cleanly(error: &io::Error) -> bool {
        error
            .get_ref()
            .is_some_and(|inner| inner.is::<EndedCleanly>())
    }
    pub(crate) fn try_exit(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
    /// Wait at most 300 ms for cooperative stop, then kill and reap. A forced
    /// termination or abnormal exit is a failure even when cleanup succeeds.
    /// The grace/poll loop is bounded; kernel kill/wait has no userspace hard
    /// deadline. Drop remains best-effort cleanup, never evidence of success.
    pub fn shutdown(&mut self) -> io::Result<()> {
        match self.child.try_wait() {
            Ok(Some(status)) => return self.exit_result(status),
            Ok(None) => {}
            Err(error) => {
                return Err(self.terminate_and_reap(
                    io::ErrorKind::Other,
                    format!("status check before shutdown failed: {error}"),
                ));
            }
        }
        let stop_error = self.channels.control.send(Control::Stop).err();
        #[cfg(test)]
        self.run_on_stop();
        let deadline = Instant::now() + Duration::from_millis(300);
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return self.exit_result(status),
                Ok(None) => {}
                Err(error) => {
                    return Err(self.terminate_and_reap(
                        io::ErrorKind::Other,
                        format!("status check during shutdown failed: {error}"),
                    ));
                }
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut reason = "did not stop within 300 ms; forced termination required".to_owned();
        if let Some(error) = stop_error {
            reason.push_str(&format!("; stop delivery failed: {error}"));
        }
        Err(self.terminate_and_reap(io::ErrorKind::TimedOut, reason))
    }

    pub(crate) fn exit_result(&mut self, status: ExitStatus) -> io::Result<()> {
        let cleanup = self.cleanup_ring_paths();
        match (status.success(), cleanup) {
            (true, result) => result,
            (false, result) => Err(io::Error::other(format!(
                "{} worker exited abnormally: {status}{}",
                self.role,
                result.err().map_or(String::new(), |e| format!("; {e}"))
            ))),
        }
    }

    fn cleanup_ring_paths(&mut self) -> io::Result<()> {
        match &mut self.ring_resources {
            Some(resources) => resources.cleanup_after_exit(),
            None => Ok(()),
        }
    }

    /// Always attempt wait, even if kill races a child that has already exited.
    /// Preserve both cleanup errors rather than returning early and losing a reap.
    pub(crate) fn terminate_and_reap(&mut self, kind: io::ErrorKind, reason: String) -> io::Error {
        let killed = self.child.kill();
        let reaped = self.child.wait();
        let mut message = format!("{} worker {reason}", self.role);
        if let Err(error) = killed {
            message.push_str(&format!("; kill failed: {error}"));
        }
        match reaped {
            Ok(status) => {
                message.push_str(&format!("; reaped with {status}"));
                if let Err(error) = self.cleanup_ring_paths() {
                    message.push_str(&format!("; {error}"));
                }
            }
            Err(error) => message.push_str(&format!("; reap failed: {error}")),
        }
        #[cfg(test)]
        if let Some(observe) = self.after_terminate.take() {
            observe();
        }
        io::Error::new(kind, message)
    }
    /// Signal only this owned child, without waiting for kernel teardown or
    /// reaping. Live optional-worker failure uses this once, then `try_exit`;
    /// final shared shutdown still owns reaping and reports forced termination.
    pub(crate) fn request_termination(&mut self) -> io::Result<()> {
        self.child.kill()
    }
}

/// The longest a fixture's first `sh` has to print its marker. A start takes
/// milliseconds, so this is far above it: only a fixture that never starts
/// fails here, not a host that stalled for seconds in a long run beside other
/// work (#1317, as the serve fixtures' bound). The lamp's own
/// `WORKER_STARTUP_TIMEOUT` is not this.
#[cfg(test)]
const FIXTURE_EXEC_WAIT: Duration = Duration::from_secs(30);

/// A test's run of a script it has just written, to its end. On Linux an
/// exec fails with `ETXTBSY` while any process still holds the file open
/// for writing, and a child another test forks holds a new script that way
/// until its own exec (CLOEXEC): that is waited out, for at most 15 s, as
/// the motor fixture does (Ten on #705, 18 Oct).
#[cfg(test)]
pub(crate) fn status_of_fresh(command: &mut Command) -> ExitStatus {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        match command.status() {
            Err(error)
                if error.kind() == io::ErrorKind::ExecutableFileBusy && Instant::now() < until =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            status => return status.unwrap(),
        }
    }
}

/// Test-only exec completion, never an audio/runtime readiness signal. The
/// one-byte pipe is nonblocking and has a finite bound, [`FIXTURE_EXEC_WAIT`]
/// for the fixtures.
#[cfg(test)]
fn fixture_exec_barrier(child: &mut Child, timeout: Duration) -> io::Result<()> {
    let result = (|| {
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("fixture has no marker pipe"))?;
        let flags = rustix::fs::fcntl_getfl(&output)?;
        rustix::fs::fcntl_setfl(&output, flags | rustix::fs::OFlags::NONBLOCK)?;
        let deadline = Instant::now() + timeout;
        let mut marker = [0_u8; 1];
        loop {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "fixture exec marker timed out",
                ));
            }
            match output.read(&mut marker) {
                Ok(1) if marker[0] == b'R' => return Ok(()),
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "fixture exited before exec marker",
                    ));
                }
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid fixture exec marker",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    })();
    if let Err(error) = result {
        // A failed fixture must not leak a child just because no Worker was built.
        return Err(failed_fixture(child, error));
    }
    Ok(())
}

#[cfg(test)]
fn failed_fixture(child: &mut Child, error: io::Error) -> io::Error {
    let killed = child.kill();
    let reaped = child.wait();
    io::Error::new(
        error.kind(),
        format!("{error}; fixture kill={killed:?}, reap={reaped:?}"),
    )
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    #[test]
    fn cooperative_ready_child_exit_observation_does_not_reap_or_hide_a_timeout() {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, _peer, input) =
            Worker::cooperative_ready_fixture(&directory.path, "probe");
        let pid = worker.fixture_pid();
        worker.check_running().unwrap();
        let error = crate::scenario_clock::wait_child_exited(
            pid,
            Instant::now() + Duration::from_millis(2),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        drop(input);
        let deadline = Instant::now() + WORKER_STARTUP_TIMEOUT;
        crate::scenario_clock::wait_child_exited(pid, deadline).unwrap();
        crate::scenario_clock::wait_child_exited(pid, deadline).unwrap();
        // A deadline already passed is a one-shot probe: it still sees the exit.
        crate::scenario_clock::wait_child_exited(pid, Instant::now()).unwrap();
        let status = worker
            .try_exit()
            .unwrap()
            .expect("supervisor still owns reap");
        worker.exit_result(status).unwrap();
        assert!(
            crate::scenario_clock::wait_child_exited(pid, deadline).is_err(),
            "a reaped child cannot provide a new exit receipt"
        );
    }

    #[test]
    fn child_exit_observation_rejects_nonzero_and_signal_death() {
        let directory = SessionDirectory::create().unwrap();
        let (mut failed, _failed_peer) =
            Worker::command_fixture(&directory.path, "capture", "exit 7");
        let error = crate::scenario_clock::wait_child_exited(
            failed.fixture_pid(),
            Instant::now() + WORKER_STARTUP_TIMEOUT,
        )
        .unwrap_err();
        assert!(error.to_string().contains("exited abnormally"), "{error}");
        assert_eq!(failed.child.wait().unwrap().code(), Some(7));
        let (mut killed, _killed_peer, input) =
            Worker::cooperative_ready_fixture(&directory.path, "speaker");
        killed.child.kill().unwrap();
        let error = crate::scenario_clock::wait_child_exited(
            killed.fixture_pid(),
            Instant::now() + WORKER_STARTUP_TIMEOUT,
        )
        .unwrap_err();
        assert!(error.to_string().contains("exited abnormally"), "{error}");
        assert!(!killed.child.wait().unwrap().success());
        drop(input);
    }

    #[test]
    fn exec_barrier_closes_earlier_peer_descriptors_without_exiting_workers() {
        let directory = SessionDirectory::create().unwrap();
        let (mut capture, capture_peer) = Worker::sleeping_fixture(&directory.path, "capture");
        let (mut provider, _provider_peer) = Worker::sleeping_fixture(&directory.path, "provider");
        let (mut ring, ring_peer) = Worker::sleeping_fixture(&directory.path, "ring");
        #[cfg(target_os = "linux")]
        let dropped: Vec<String> = [&capture_peer, &ring_peer]
            .into_iter()
            .flat_map(|peer| [&peer.control, &peer.data])
            .map(|channel| {
                let inode = rustix::fs::fstat(channel.socket()).unwrap().st_ino;
                format!("socket:[{inode}]")
            })
            .collect();
        drop(capture_peer);
        drop(ring_peer);
        // The barrier's own claim, read at once from each worker with no retry:
        // none holds a dropped peer's socket. Only Linux shows it in /proc, and
        // only QEMU's spawn needs the barrier (#1569).
        #[cfg(target_os = "linux")]
        for worker in [&capture, &provider, &ring] {
            let fds = format!("/proc/{}/fd", worker.fixture_pid());
            for fd in fs::read_dir(&fds).unwrap() {
                let held = fs::read_link(fd.unwrap().path()).unwrap_or_default();
                assert!(
                    !dropped
                        .iter()
                        .any(|socket| held.as_os_str() == socket.as_str()),
                    "{} holds a dropped peer's {}",
                    worker.role,
                    held.display()
                );
            }
        }
        // The transport sees each peer gone. No worker is killed to make it so.
        capture.send_stop_until_peer_gone();
        ring.send_stop_until_peer_gone();
        capture.check_running().unwrap();
        provider.check_running().unwrap();
        ring.check_running().unwrap();
    }

    fn unmarked_child(script: &str) -> Child {
        Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    #[test]
    fn exec_barrier_timeout_reaps_only_its_owned_child() {
        let mut child = unmarked_child("exec sleep 60");
        let error = fixture_exec_barrier(&mut child, Duration::from_millis(20)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(!child.try_wait().unwrap().unwrap().success());
    }

    #[test]
    fn exec_barrier_rejects_wrong_marker_and_reaps_child() {
        let mut child = unmarked_child("printf X; exec sleep 60");
        let error = fixture_exec_barrier(&mut child, WORKER_STARTUP_TIMEOUT).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!child.try_wait().unwrap().unwrap().success());
    }
}

/// `bind` publishes a pathname before Endpoint finishes setting mode 0600.
/// Do not race that finalization or connect one half of the channel pair first.
/// This runs only inside the existing bounded pre-readiness startup loop.
fn peer_sockets_ready(paths: &[&Path]) -> io::Result<bool> {
    let mut ready = true;
    for path in paths {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ready = false;
                continue;
            }
            Err(error) => return Err(error),
        };
        if !metadata.file_type().is_socket()
            || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "worker peer must be a socket owned by this user",
            ));
        }
        // A permanently unprivate socket is never connected; startup times out.
        ready &= metadata.mode() & 0o077 == 0;
    }
    Ok(ready)
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    use std::os::unix::{fs::symlink, net::UnixDatagram};

    #[test]
    fn startup_waits_for_both_sockets_to_finish_becoming_private() {
        let directory = SessionDirectory::create().unwrap();
        let control = directory.path.join("c");
        let data = directory.path.join("d");
        let paths = [control.as_path(), data.as_path()];
        assert!(!peer_sockets_ready(&paths).unwrap());
        let c = UnixDatagram::bind(&control).unwrap();
        let d = UnixDatagram::bind(&data).unwrap();
        fs::set_permissions(&control, fs::Permissions::from_mode(0o666)).unwrap();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(!peer_sockets_ready(&paths).unwrap());
        fs::set_permissions(&control, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!peer_sockets_ready(&paths).unwrap());
        fs::set_permissions(&data, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(peer_sockets_ready(&paths).unwrap());
        drop((c, d));
        fs::remove_file(control).unwrap();
        fs::remove_file(data).unwrap();
    }

    #[test]
    fn startup_rejects_files_and_symlinks_instead_of_calling_them_ready() {
        let directory = SessionDirectory::create().unwrap();
        let path = directory.path.join("peer");
        fs::write(&path, []).unwrap();
        assert_eq!(
            peer_sockets_ready(&[&path]).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        fs::remove_file(&path).unwrap();
        let bound = directory.path.join("bound");
        let peer = UnixDatagram::bind(&bound).unwrap();
        fs::set_permissions(&bound, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&bound, &path).unwrap();
        assert_eq!(
            peer_sockets_ready(&[&path]).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        drop(peer);
        fs::remove_file(path).unwrap();
        fs::remove_file(bound).unwrap();
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Never leave microphone/provider children behind after startup errors.
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        if self.child.wait().is_ok() {
            let _ = self.cleanup_ring_paths();
        }
    }
}

/// The command a worker starts with: its arguments, and a cleared
/// environment with only what its role is given, the switches read with
/// `var` from the voice's own.
fn worker_command(
    executable: &Path,
    directory: &Path,
    role: &str,
    (parent, boot): (BootId, BootId),
    extra: &[&str],
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Command {
    let mut command = Command::new(executable);
    command
        .arg(lamp_voice_processes::Command::Worker.as_str())
        .arg(role)
        .arg(directory)
        .arg(boot_text(parent))
        .arg(boot_text(boot))
        .args(extra)
        .env_clear()
        .envs(crate::stalls::passed_on())
        .envs(memory_days_worker_environment(
            role,
            var("LAMP_MEMORY_DAYS").as_deref(),
        ))
        .envs(memory_rules_worker_environment(
            role,
            var(crate::rules::ENVIRONMENT).as_deref(),
        ))
        .envs(memory_threads_worker_environment(
            role,
            var(crate::remember::THREADS_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_events_worker_environment(
            role,
            var(crate::remember::EVENTS_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_forget_all_worker_environment(
            role,
            var(crate::remember::FORGET_ALL_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_journal_resumes_worker_environment(
            role,
            var(crate::remember::JOURNAL_RESUMES_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_recall_v2_worker_environment(
            role,
            var(crate::remember::RECALL_V2_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_early_words_worker_environment(
            role,
            var(crate::early_words::ENVIRONMENT).as_deref(),
        ))
        .envs(memory_follows_face_worker_environment(
            role,
            var(crate::follows_face::ENVIRONMENT).as_deref(),
        ))
        .envs(memory_forget_about_worker_environment(
            role,
            var(crate::remember::FORGET_ABOUT_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_room_not_asker_worker_environment(
            role,
            var(crate::remember::ROOM_NOT_ASKER_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_personal_needs_face_worker_environment(
            role,
            var(crate::remember::needs_face::ENVIRONMENT).as_deref(),
        ))
        .envs(memory_room_after_forget_worker_environment(
            role,
            var(crate::remember::ROOM_AFTER_FORGET_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_link_used_worker_environment(
            role,
            var(crate::remember::LINK_USED_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_room_speaker_worker_environment(
            role,
            var(crate::remember::ROOM_SPEAKER_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_keep_asked_worker_environment(
            role,
            var(crate::remember::asked::ENVIRONMENT).as_deref(),
        ))
        .envs(schedule_asked_worker_environment(
            role,
            var(crate::tools::schedule_asked::ENVIRONMENT).as_deref(),
        ))
        .envs(memory_observe_worker_environment(
            role,
            var(crate::remember::OBSERVE_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_reopen_quiet_worker_environment(
            role,
            var(crate::second_provider::worker::REOPEN_QUIET_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_keep_heard_worker_environment(
            role,
            var(crate::remember::heard::ENVIRONMENT).as_deref(),
        ))
        .envs(memory_remember_once_worker_environment(
            role,
            var(crate::remember::REMEMBER_ONCE_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_replaces_worker_environment(
            role,
            var(crate::remember::REPLACES_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_forget_asks_worker_environment(
            role,
            var(crate::remember::FORGET_ASKS_ENVIRONMENT).as_deref(),
        ))
        .envs(memory_stable_id_worker_environment(
            role,
            var(lamp_msgs::known::ENVIRONMENT).as_deref(),
        ))
        .envs(kernel_clock_worker_environment(
            role,
            var(crate::local_time::KERNEL_CLOCK_ENVIRONMENT).as_deref(),
        ))
        .envs(near_calibrated_worker_environment(
            role,
            var(crate::attention::NEAR_CALIBRATED_ENVIRONMENT).as_deref(),
        ))
        .envs(face_gap_worker_environment(
            role,
            var(crate::memory_scope::GAP_ENVIRONMENT).as_deref(),
        ))
        .envs(quiet_evidence_worker_environment(
            role,
            var(crate::quiet_pace::EVIDENCE_ENVIRONMENT).as_deref(),
        ))
        .envs(capture_wait_worker_environment(
            role,
            var(crate::quiet_pace::CAPTURE_WAIT_ENVIRONMENT).as_deref(),
        ))
        .envs(vad_input_worker_environment(
            role,
            var(crate::activity::INPUT_ENVIRONMENT).as_deref(),
        ))
        .envs(capture_cores_worker_environment(
            role,
            var(crate::capture_cores::ENVIRONMENT),
        ))
        .envs(capture_stages_worker_environment(
            role,
            var(crate::stage_cost::ENVIRONMENT).as_deref(),
        ))
        .envs(person_name_worker_environment(
            role,
            var(crate::person::ENVIRONMENT).as_deref(),
        ))
        .envs(person_name_owner_only_worker_environment(
            role,
            var(crate::person::OWNER_ONLY_ENVIRONMENT).as_deref(),
        ))
        .envs(tasks_told_worker_environment(
            role,
            var(crate::task_news::TOLD_ENVIRONMENT).as_deref(),
        ))
        .envs(credentials_worker_environment(
            role,
            var(crate::second_config::CREDENTIALS_ENV).as_deref(),
        ))
        .envs(second_route_worker_environment(role, &var))
        .envs(second_scoped_context_worker_environment(
            role,
            var(crate::second_provider::worker::SCOPED_CONTEXT_ENVIRONMENT).as_deref(),
        ))
        .envs(unplug_worker_environment(
            role,
            var(lamp_body::unplug::ENVIRONMENT).as_deref(),
        ))
        .envs(scheduling_worker_environment(
            role,
            var(crate::worker_scheduling::ENVIRONMENT).as_deref(),
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

// Keep the voice routes' one opt-in explicit (the Live provider and the
// second route share the memory). No other coordinator environment reaches
// a worker, and unset, non-Unicode and unrecognized values stay off.
fn memory_days_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some(("LAMP_MEMORY_DAYS", "on"))
}

// Each worker's own scheduling (#1608, `LAMP_WORKER_SCHEDULING`): the exact
// `on`, to every worker on the voice's list, each of which reads it in `main`.
fn scheduling_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (lamp_voice_processes::WorkerRole::parse(role).is_some()
        && value == Some(std::ffi::OsStr::new("on")))
    .then_some((crate::worker_scheduling::ENVIRONMENT, "on"))
}

// The second route's scoped context (#1675, #1839), passed the same narrow
// way: the exact `on`, to the second route only, which reads it.
fn second_scoped_context_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "second" && value == Some(std::ffi::OsStr::new("on"))).then_some((
        crate::second_provider::worker::SCOPED_CONTEXT_ENVIRONMENT,
        "on",
    ))
}

// The rules' switch (#813), passed the same narrow way: the exact `on`, to
// the two routes that offer the memory's functions (twelve-P2): the second
// route keeps and lists rules through the same Days client.
fn memory_rules_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::rules::ENVIRONMENT, "on"))
}

// Forgetting that reaches everything (#1693), passed the same narrow way:
// the exact `on`, to the two routes that offer the memory's functions, as the
// Days store's own switch is. The second route starts the same Days client;
// without it there, "forget that" and "forget everything" ran as before
// #1723 on the new flow (twelve-P2).
fn memory_forget_all_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::FORGET_ALL_ENVIRONMENT, "on"))
}

// Forgetting what a thing was about (#2176), passed the same narrow way, to
// the same two routes as forgetting everything.
fn memory_forget_about_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::FORGET_ABOUT_ENVIRONMENT, "on"))
}

// Journalling resumed with a fresh session (#2176 row 1b), passed the same
// narrow way: the exact `on`, to the two routes whose Days client stops and
// resumes it.
fn memory_journal_resumes_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::JOURNAL_RESUMES_ENVIRONMENT, "on"))
}

// Recall's second version (#2176), passed the same narrow way: the exact
// `on`, to the two routes that offer recall through the Days client.
fn memory_recall_v2_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::RECALL_V2_ENVIRONMENT, "on"))
}

// A remember of what is kept already (#2176), passed the same narrow way:
// the exact `on`, to the two routes that offer `remember`.
fn memory_remember_once_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::REMEMBER_ONCE_ENVIRONMENT, "on"))
}

// The room's early words following a lone face once known (#2176, 5a),
// passed the same narrow way: the exact `on`, to the Live route alone, the
// one that reads the eyes' faces between turns (`early_words`).
fn memory_early_words_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "provider" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::early_words::ENVIRONMENT, "on"))
}

// The caller's own early lines following their face once known (#2176),
// passed the same narrow way: the exact `on`, to the second route alone, the
// one that keeps each line's voice and speaking faces (`follows_face`).
fn memory_follows_face_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "second" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::follows_face::ENVIRONMENT, "on"))
}

// The observation pass's switch (#1693 (2)), passed the same narrow way:
// the exact `on`, to the second route alone, the one worker with a model's
// credential (`credentials_worker_environment`) to ask.
fn memory_observe_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "second" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::OBSERVE_ENVIRONMENT, "on"))
}

// Keeping a fact as it was heard (#2176), passed the same narrow way: the
// exact `on`, to the two routes that run `remember` for a heard line.
fn memory_keep_heard_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::heard::ENVIRONMENT, "on"))
}

// A correction replacing a kept thing (#2510), passed the same narrow way:
// the exact `on`, to the two routes that run `remember` (each reads it in
// `remember::start`; the second route also for its turns' journal lines).
fn memory_replaces_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::REPLACES_ENVIRONMENT, "on"))
}

// A forget that found nothing asks about a kept thing near misheard words,
// passed the same narrow way: the exact `on`, to the two routes that run
// `remember` (each reads it in `remember::start`).
fn memory_forget_asks_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::FORGET_ASKS_ENVIRONMENT, "on"))
}

// Memory keyed on the eyes' stable id (#2176), passed the same narrow way:
// the exact `on`, to the two routes that make a memory id from a name the
// eyes gave (`known_faces::memory_person`).
fn memory_stable_id_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((lamp_msgs::known::ENVIRONMENT, "on"))
}

// The note asked again after ten quiet minutes (#1693 (1)), passed the
// same narrow way: the exact `on`, to the second route alone, whose run of
// turns is otherwise asked its note only when the person changes (a Live
// session is opened anew after a quiet already).
fn memory_reopen_quiet_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "second" && value == Some(std::ffi::OsStr::new("on"))).then_some((
        crate::second_provider::worker::REOPEN_QUIET_ENVIRONMENT,
        "on",
    ))
}

// The open threads' switch (#1693 (3)), passed the same narrow way: the
// exact `on`, to the two routes that open the sessions told of them (each
// through `remember::told_for_session`; twelve-P2).
fn memory_threads_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::THREADS_ENVIRONMENT, "on"))
}

// The dated events (#1693), passed the same narrow way: the exact `on`, to
// the two routes that tell a follow-up (`remember::told_for_session`); the
// second's observation pass dates them.
fn memory_events_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::EVENTS_ENVIRONMENT, "on"))
}

// The room's kept things told as someone in the room's (#2176), passed the
// same narrow way: the exact `on`, to the two routes that open the sessions
// told of them (each through `remember::told_for_session`).
fn memory_room_not_asker_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::ROOM_NOT_ASKER_ENVIRONMENT, "on"))
}

// Nothing about a person kept while nobody is known, passed the same narrow
// way: the exact `on`, to the two routes that run `remember` (the second's
// observation pass too).
fn memory_personal_needs_face_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::needs_face::ENVIRONMENT, "on"))
}

// The room's lines about a person kept from whoever forgot everything, for
// the rest of that day (#2849), passed the same narrow way: the exact `on`,
// to the two routes that recall and open sessions through the Days client.
fn memory_room_after_forget_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::ROOM_AFTER_FORGET_ENVIRONMENT, "on"))
}

// An exchange linked only to what its turn used (#2176 finding 14), passed
// the same narrow way: the exact `on`, to the two routes that journal
// exchanges through the Days client.
fn memory_link_used_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::LINK_USED_ENVIRONMENT, "on"))
}

// Who said a room line (#2849), passed the same narrow way: the exact `on`,
// to the second route alone, the one that reads which face was seen saying
// each line (`speaking_faces`).
fn memory_room_speaker_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "second" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::ROOM_SPEAKER_ENVIRONMENT, "on"))
}

// A plain ask to keep something calls remember (`remember::asked`), passed
// the same narrow way: the exact `on`, to the second route, whose chat can
// be made to call a function.
fn memory_keep_asked_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "second" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::remember::asked::ENVIRONMENT, "on"))
}

// A plain ask for a timer or a reminder calls the function that sets it
// (`tools::schedule_asked`), passed the same narrow way: the exact `on`, to
// the second route, whose chat can be made to call a function.
fn schedule_asked_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "second" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::tools::schedule_asked::ENVIRONMENT, "on"))
}

// The clock's switch (#830), passed the same narrow way: the exact `on`, to
// the two workers that tell the time and judge the night. Cleared, it never
// reached them, so a lamp whose clock was not set took its saved 22:00 as
// night by day (Ten on #1158).
fn kernel_clock_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::local_time::KERNEL_CLOCK_ENVIRONMENT, "on"))
}

// This home's near bar (#753), to the two routes' workers, which judge a
// call with it: the exact `on`.
fn near_calibrated_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (matches!(role, "provider" | "second") && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::attention::NEAR_CALIBRATED_ENVIRONMENT, "on"))
}

// A known face's held gap (#2176 row 5b), to the one route whose memory
// scope holds it: the second's, the exact `on`.
fn face_gap_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "second" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::memory_scope::GAP_ENVIRONMENT, "on"))
}

// The echo evidence's skip (#705), to the one worker that runs it: capture.
fn quiet_evidence_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "capture" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::quiet_pace::EVIDENCE_ENVIRONMENT, "on"))
}

// The capture loop's wait (#705), to the capture worker only.
fn capture_wait_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "capture" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::quiet_pace::CAPTURE_WAIT_ENVIRONMENT, "on"))
}

// The per-stage minute line, to the one worker it times: capture.
fn capture_stages_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "capture" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::stage_cost::ENVIRONMENT, "on"))
}

// The detector's input (#828), to the one worker that runs it: capture,
// and only the exact `linear` or `linear-gated`.
fn vad_input_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    let value =
        crate::activity::DetectorInput::parse(value.and_then(std::ffi::OsStr::to_str)).value()?;
    (role == "capture").then_some((crate::activity::INPUT_ENVIRONMENT, value))
}

// The capture worker's cores (#705), to capture only, as set: the worker
// parses it, and says so when it is not a cpu list.
fn capture_cores_worker_environment(
    role: &str,
    value: Option<std::ffi::OsString>,
) -> Option<(&'static str, std::ffi::OsString)> {
    value
        .filter(|_| role == "capture")
        .map(|value| (crate::capture_cores::ENVIRONMENT, value))
}

// The typed name's file (#806), to the two workers that build the model's
// instruction: the Live provider and the second route. A path, not `on`:
// passed only when absolute. Cleared, it never reached them, and the name
// was never told (Fourth on #1323).
fn person_name_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, std::ffi::OsString)> {
    value
        .filter(|path| matches!(role, "provider" | "second") && Path::new(path).is_absolute())
        .map(|path| (crate::person::ENVIRONMENT, path.to_owned()))
}

// The typed name told only to the owner's line (#2602), to the same two
// workers as the name's file: only the exact value `on`.
fn person_name_owner_only_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    value
        .filter(|value| matches!(role, "provider" | "second") && *value == "on")
        .map(|_| (crate::person::OWNER_ONLY_ENVIRONMENT, "on"))
}

// The file of task news already told (#2762), to the two workers that read
// it as the coordinator does: the second route's text news
// (`task_news::ForTexts::for_tasks`) and both routes' task tools
// (`agent_tasks::run`). A path, passed only when absolute; cleared, a set
// file was the coordinator's alone and the workers kept the default.
fn tasks_told_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, std::ffi::OsString)> {
    value
        .filter(|path| matches!(role, "provider" | "second") && Path::new(path).is_absolute())
        .map(|path| (crate::task_news::TOLD_ENVIRONMENT, path.to_owned()))
}

// The voice unit's credentials (`LoadCredential=`, systemd's
// `CREDENTIALS_DIRECTORY`), to the one worker that reads them: the second
// route's, for OpenRouter's and DeepSeek's keys. A path, passed only when
// absolute; the keys themselves never travel in an environment.
fn credentials_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, std::ffi::OsString)> {
    value
        .filter(|path| role == "second" && Path::new(path).is_absolute())
        .map(|path| (crate::second_config::CREDENTIALS_ENV, path.to_owned()))
}

// The second route's Gemini endpoint and voice, its key terms and the
// lamp's place (#1712), and whether it listens for the names the lamp knows
// (#753), and whether a named ask is a hand-over (#1722, read by `serve`
// there), to its worker only, as set: the worker checks them, and says so.
fn second_route_worker_environment(
    role: &str,
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Vec<(&'static str, std::ffi::OsString)> {
    [
        crate::second_config::GEMINI_BASE_URL_ENV,
        crate::second_config::DEEPSEEK_BASE_URL_ENV,
        crate::second_config::SPEECH_BASE_URL_ENV,
        crate::second_config::LISTENING_BASE_URL_ENV,
        crate::second_config::SPEECH_MODEL_ENV,
        crate::second_config::KEY_TERMS_ENV,
        crate::second_provider::NAME_SECOND_SIGN_ENVIRONMENT,
        crate::second_provider::NAME_SAID_TO_A_PERSON_ENVIRONMENT,
        crate::second_provider::NAME_STAYS_ENVIRONMENT,
        // The name at a line's start dated from its usual start (#4226).
        crate::second_provider::NAME_START_USUAL_ENVIRONMENT,
        crate::local_time::PLACE_ENVIRONMENT,
        crate::second_provider::net::weather::ENVIRONMENT,
        crate::second_provider::PREPARED_ENVIRONMENT,
        crate::second_provider::OPEN_NAME_ENVIRONMENT,
        crate::second_provider::NAME_CARRIES_ENVIRONMENT,
        crate::second_provider::FIRST_SECONDS_QUIET_ENVIRONMENT,
        lamp_msgs::NameHeard::NEAR_ENVIRONMENT,
        crate::second_provider::OWN_ECHO_ENVIRONMENT,
        crate::second_provider::FACE_NAME_ENVIRONMENT,
        crate::second_provider::STT_WARM_ENVIRONMENT,
        // Read by the speech-to-text stream in this worker (#2527): without
        // it here the worker never saw the switch (lamp-c0a4's A/B).
        crate::second_provider::net::QUIET_HELD_ENVIRONMENT,
        crate::second_provider::FIRST_WORDS_ENVIRONMENT,
        crate::second_provider::CHAT_WARM_ENVIRONMENT,
        crate::second_provider::net::CHAT_KEPT_ENVIRONMENT,
        crate::second_provider::SPEECH_WARM_ENVIRONMENT,
        crate::second_provider::SPEECH_SOCKET_ENVIRONMENT,
        crate::second_provider::SPEECH_SOCKET_MODEL_ENVIRONMENT,
        // Read by the second worker's services (#2882).
        crate::second_provider::RELAY_ROUTES_ENVIRONMENT,
        crate::second_provider::STEADY_FIRST_ENVIRONMENT,
        crate::second_provider::QUIET_GOING_ON_SAME_ONE_ENVIRONMENT,
        crate::second_provider::FEW_TOOLS_ENVIRONMENT,
        crate::second_provider::SAID_ONCE_ENVIRONMENT,
        crate::second_provider::SPEECH_TAGS_ENVIRONMENT,
        // A light or volume turn answered with the call's own line (#2877).
        crate::second_provider::SHORT_DONE_ENVIRONMENT,
        // An empty answer to a line said by name asked again (#3704).
        crate::second_provider::NO_WORDS_AGAIN_ENVIRONMENT,
        crate::second_provider::NAMED_ALWAYS_ENVIRONMENT,
        crate::remember::HITS_ENVIRONMENT,
        crate::second_provider::SKILL_LOOK_ENVIRONMENT,
        crate::second_provider::GOING_ON_NOT_OWN_ENVIRONMENT,
        crate::second_provider::BUSY_CALLED_STRICT_ENVIRONMENT,
        // A remark about the lamp held in a busy room (#4175).
        crate::remark_about_it::ENVIRONMENT,
        crate::second_provider::TURNED_SPEAKING_ENVIRONMENT,
        crate::second_provider::JOIN_CUT_WORDS_ENVIRONMENT,
        // Read by the second worker, which tells the coordinator (#3026).
        crate::unfinished_words::ENVIRONMENT,
        // Read by the second route's same-voice check (#2602).
        crate::same_voice::CALLER_ONLY_ENVIRONMENT,
        crate::second_provider::BUSY_ENDS_SOONER_ENVIRONMENT,
        crate::second_provider::SAME_SPEAKER_CARRIES_ENVIRONMENT,
        crate::second_provider::TURNED_FACE_SPEAKS_ENVIRONMENT,
        crate::second_provider::TWO_FACES_HELD_ENVIRONMENT,
        crate::second_provider::PERSON_SCOPE_SEEN_ENVIRONMENT,
        // Read by the second worker for the reply's owner-by-default (#3884).
        crate::person::BY_DEFAULT_ENVIRONMENT,
        crate::remember::FORGET_ASKED_ENVIRONMENT,
        crate::second_provider::WRITTEN_WAIT_ENVIRONMENT,
        crate::second_provider::CUE_AFTER_ENVIRONMENT,
        crate::second_provider::LOOK_NOTE_ENVIRONMENT,
        crate::second_provider::LOOK_WALK_ENVIRONMENT,
        crate::second_provider::NOT_FOR_ME_WORDS_ENVIRONMENT,
        crate::speaking_faces::ENVIRONMENT,
        crate::speaking_faces::EYES_ENVIRONMENT,
        crate::agent_tasks::NAMED_ASK_ENVIRONMENT,
        // A line asking to remember the person's face, with their name,
        // calls remember_my_face (the owner's try on lamp-4ace, 23 Oct).
        crate::known_faces::ASKED_ENVIRONMENT,
        crate::second_config::FLUX_ENV,
        crate::second_config::OWN_KEYS_ENV,
        // The owner's place for a question that needs one, and cleared
        // with the owner's forget everything (#4333).
        crate::owner_place::ENVIRONMENT,
        crate::second_provider::net::key_terms::ENVIRONMENT,
    ]
    .into_iter()
    .filter(|_| role == "second")
    .filter_map(|name| var(name).map(|value| (name, value)))
    .collect()
}

// The unplug switch (#1674), passed the same narrow way: the exact `on`, to
// the ring only, which draws the asleep picture and says its receipt.
fn unplug_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "ring" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((lamp_body::unplug::ENVIRONMENT, "on"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// lamp-52e6, 27 Oct 17:16:43 (#3697): Controls stopped under a running
    /// voice; the wait's new privacy worker ended at once, its sockets gone
    /// before its exit could be read, and the voice's connect found `No
    /// such file or directory` and exited 1. That ENOENT is now the
    /// worker's clean end, an unreadable switch the wait reads again; a
    /// worker still running keeps the error, and a crash stays a crash.
    #[test]
    fn a_peer_gone_at_startup_is_told_as_the_workers_own_end() {
        let gone = || io::Error::from(io::ErrorKind::NotFound);
        let ended = |command: &str| {
            let directory = SessionDirectory::create().unwrap();
            let (mut worker, peer) = Worker::command_fixture(&directory.path, "privacy", command);
            drop(peer);
            let error = worker.ended_or(gone());
            let _ = worker.shutdown();
            drop(worker);
            let _ = fs::remove_dir_all(&directory.path);
            error
        };
        let clean = ended("sleep 0.03; exit 0");
        assert!(Worker::ended_cleanly(&clean), "{clean}");
        let error = crate::service_privacy_controls::started(Err(clean))
            .err()
            .unwrap();
        assert!(crate::service_privacy::is_unavailable(&*error), "{error}");
        let crashed = ended("sleep 0.03; exit 101");
        assert!(!Worker::ended_cleanly(&crashed), "{crashed}");
        assert!(crashed.to_string().contains("worker exited"), "{crashed}");
        let running = ended("sleep 5");
        assert_eq!(running.kind(), io::ErrorKind::NotFound, "{running}");
    }

    /// #1712: the Gemini endpoint and voice go to the second route's worker
    /// only.
    #[test]
    fn the_second_routes_settings_reach_only_its_worker() {
        let var = |name: &str| {
            (name == crate::second_config::SPEECH_MODEL_ENV)
                .then(|| std::ffi::OsString::from("gemini-3.8-flash-lite-tts"))
        };
        assert_eq!(
            second_route_worker_environment("second", var),
            [(
                crate::second_config::SPEECH_MODEL_ENV,
                std::ffi::OsString::from("gemini-3.8-flash-lite-tts")
            )]
        );
        assert!(second_route_worker_environment("provider", var).is_empty());
        assert!(second_route_worker_environment("second", |_| None).is_empty());
    }

    /// #753: whether the second route listens for the names the lamp knows
    /// goes to its worker only.
    #[test]
    fn the_known_names_switch_reaches_only_the_second_route() {
        for name in [
            crate::second_provider::NAME_SECOND_SIGN_ENVIRONMENT,
            crate::second_provider::NAME_SAID_TO_A_PERSON_ENVIRONMENT,
            crate::second_provider::NAME_STAYS_ENVIRONMENT,
            crate::second_provider::NAME_START_USUAL_ENVIRONMENT,
            crate::second_provider::net::key_terms::ENVIRONMENT,
            crate::second_config::FLUX_ENV,
            crate::second_config::OWN_KEYS_ENV,
            crate::second_provider::STEADY_FIRST_ENVIRONMENT,
            crate::second_provider::QUIET_GOING_ON_SAME_ONE_ENVIRONMENT,
            crate::second_provider::FEW_TOOLS_ENVIRONMENT,
            crate::second_provider::SAID_ONCE_ENVIRONMENT,
            crate::second_provider::SPEECH_TAGS_ENVIRONMENT,
            crate::second_provider::SHORT_DONE_ENVIRONMENT,
            crate::second_provider::NO_WORDS_AGAIN_ENVIRONMENT,
            crate::second_provider::NAMED_ALWAYS_ENVIRONMENT,
            crate::second_provider::SKILL_LOOK_ENVIRONMENT,
            crate::second_provider::GOING_ON_NOT_OWN_ENVIRONMENT,
            crate::second_provider::BUSY_CALLED_STRICT_ENVIRONMENT,
            crate::remark_about_it::ENVIRONMENT,
            crate::second_provider::TURNED_SPEAKING_ENVIRONMENT,
            crate::second_provider::JOIN_CUT_WORDS_ENVIRONMENT,
            crate::unfinished_words::ENVIRONMENT,
            crate::same_voice::CALLER_ONLY_ENVIRONMENT,
            crate::second_provider::WRITTEN_WAIT_ENVIRONMENT,
            crate::second_provider::CUE_AFTER_ENVIRONMENT,
            crate::second_provider::OPEN_NAME_ENVIRONMENT,
            crate::second_provider::NAME_CARRIES_ENVIRONMENT,
            crate::second_provider::FIRST_SECONDS_QUIET_ENVIRONMENT,
            lamp_msgs::NameHeard::NEAR_ENVIRONMENT,
            crate::second_provider::OWN_ECHO_ENVIRONMENT,
            crate::second_provider::FACE_NAME_ENVIRONMENT,
            crate::second_provider::BUSY_ENDS_SOONER_ENVIRONMENT,
            crate::second_provider::SAME_SPEAKER_CARRIES_ENVIRONMENT,
            crate::second_provider::TURNED_FACE_SPEAKS_ENVIRONMENT,
            crate::second_provider::TWO_FACES_HELD_ENVIRONMENT,
            crate::second_provider::PERSON_SCOPE_SEEN_ENVIRONMENT,
            crate::person::BY_DEFAULT_ENVIRONMENT,
            crate::second_provider::LOOK_NOTE_ENVIRONMENT,
            crate::second_provider::LOOK_WALK_ENVIRONMENT,
            crate::second_provider::NOT_FOR_ME_WORDS_ENVIRONMENT,
            crate::speaking_faces::ENVIRONMENT,
            crate::speaking_faces::EYES_ENVIRONMENT,
            crate::agent_tasks::NAMED_ASK_ENVIRONMENT,
            crate::known_faces::ASKED_ENVIRONMENT,
        ] {
            let var = |asked: &str| (asked == name).then(|| std::ffi::OsString::from("on"));
            assert_eq!(
                second_route_worker_environment("second", var),
                [(name, std::ffi::OsString::from("on"))]
            );
            for role in ["provider", "capture", "speaker"] {
                assert!(
                    second_route_worker_environment(role, var).is_empty(),
                    "{role}"
                );
            }
        }
    }

    /// The reviewer on #2365: with only `LAMP_TASKS_NAMED_ASK=on` set, the
    /// started second worker gets it, since `serve` reads it there; no other
    /// worker does.
    #[test]
    fn the_named_ask_switch_alone_reaches_the_started_second_worker() {
        use std::os::unix::fs::PermissionsExt;
        let home = SessionDirectory::create().unwrap();
        let script = home.path.join("worker.sh");
        std::fs::write(&script, "#!/bin/sh\n/usr/bin/env > \"$3/env-$2.txt\"\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let only = |name: &str| {
            (name == crate::agent_tasks::NAMED_ASK_ENVIRONMENT)
                .then(|| std::ffi::OsString::from("on"))
        };
        let boot = new_boot().unwrap();
        for (role, expected) in [
            ("second", true),
            ("provider", false),
            ("capture", false),
            ("speaker", false),
        ] {
            let status = status_of_fresh(&mut worker_command(
                &script,
                &home.path,
                role,
                (boot, boot),
                &[],
                only,
            ));
            assert!(status.success(), "{role}");
            let written =
                std::fs::read_to_string(home.path.join(format!("env-{role}.txt"))).unwrap();
            assert_eq!(
                written
                    .lines()
                    .any(|line| line == "LAMP_TASKS_NAMED_ASK=on"),
                expected,
                "{role}"
            );
        }
    }

    /// #2510: on lamp-06c1 `LAMP_MEMORY_REPLACES` never reached a worker,
    /// so `replaces` was never offered. Each switch below, alone, reaches a
    /// started worker of its routes and no other.
    #[test]
    fn the_memory_switches_alone_reach_the_started_routes() {
        use std::os::unix::fs::PermissionsExt;
        let home = SessionDirectory::create().unwrap();
        let script = home.path.join("worker.sh");
        std::fs::write(&script, "#!/bin/sh\n/usr/bin/env > \"$3/env-$2.txt\"\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let boot = new_boot().unwrap();
        for (name, routes) in [
            (
                crate::remember::REPLACES_ENVIRONMENT,
                ["provider", "second"].as_slice(),
            ),
            (
                crate::remember::FORGET_ASKS_ENVIRONMENT,
                ["provider", "second"].as_slice(),
            ),
            (
                lamp_msgs::known::ENVIRONMENT,
                ["provider", "second"].as_slice(),
            ),
            (
                crate::same_voice::CALLER_ONLY_ENVIRONMENT,
                ["second"].as_slice(),
            ),
        ] {
            let only = |asked: &str| (asked == name).then(|| std::ffi::OsString::from("on"));
            for role in ["second", "provider", "capture", "speaker"] {
                let status = status_of_fresh(&mut worker_command(
                    &script,
                    &home.path,
                    role,
                    (boot, boot),
                    &[],
                    only,
                ));
                assert!(status.success(), "{name} {role}");
                let written =
                    std::fs::read_to_string(home.path.join(format!("env-{role}.txt"))).unwrap();
                assert_eq!(
                    written.lines().any(|line| line == format!("{name}=on")),
                    routes.contains(&role),
                    "{name} {role}"
                );
            }
        }
    }

    /// The told file of task news goes to the provider and second workers
    /// only, and only as an absolute path (#2762).
    #[test]
    fn the_told_file_reaches_only_the_workers_that_tell_task_news() {
        let absolute = std::ffi::OsStr::new("/var/lib/lampOS/voice/tasks-told");
        for role in ["provider", "second"] {
            assert_eq!(
                tasks_told_worker_environment(role, Some(absolute)),
                Some((crate::task_news::TOLD_ENVIRONMENT, absolute.to_owned()))
            );
            assert_eq!(tasks_told_worker_environment(role, None), None);
            for unfit in ["tasks-told", "", "on"] {
                assert_eq!(
                    tasks_told_worker_environment(role, Some(std::ffi::OsStr::new(unfit))),
                    None,
                    "{unfit:?}"
                );
            }
        }
        for role in [
            "capture", "speaker", "denoise", "echo", "ring", "camera", "privacy",
        ] {
            assert_eq!(
                tasks_told_worker_environment(role, Some(absolute)),
                None,
                "{role}"
            );
        }
    }

    /// The voice unit's credentials' directory goes to the second route's
    /// worker only, and only as an absolute path.
    #[test]
    fn the_units_credentials_reach_only_the_second_route() {
        let absolute = std::ffi::OsStr::new("/run/credentials/lamp-voice.service");
        assert_eq!(
            credentials_worker_environment("second", Some(absolute)),
            Some(("CREDENTIALS_DIRECTORY", absolute.to_owned()))
        );
        assert_eq!(credentials_worker_environment("second", None), None);
        assert_eq!(
            credentials_worker_environment("second", Some(std::ffi::OsStr::new("run/x"))),
            None
        );
        for role in [
            "provider", "capture", "speaker", "privacy", "ring", "camera", "unknown",
        ] {
            assert_eq!(
                credentials_worker_environment(role, Some(absolute)),
                None,
                "{role}"
            );
        }
    }

    /// #753: this home's near bar reaches the two routes' workers, which
    /// judge a call with it, and only as the exact `on`.
    #[test]
    fn the_near_bars_switch_reaches_the_two_routes_only() {
        let on = Some(std::ffi::OsStr::new("on"));
        for role in ["provider", "second"] {
            assert_eq!(
                near_calibrated_worker_environment(role, on),
                Some((crate::attention::NEAR_CALIBRATED_ENVIRONMENT, "on")),
                "{role}"
            );
        }
        for role in ["capture", "speaker"] {
            assert_eq!(near_calibrated_worker_environment(role, on), None, "{role}");
        }
        for off in [
            None,
            Some(std::ffi::OsStr::new("ON")),
            Some(std::ffi::OsStr::new("")),
        ] {
            assert_eq!(near_calibrated_worker_environment("provider", off), None);
        }
    }

    #[test]
    fn cleared_worker_environment_passes_only_the_exact_voice_route_opt_in() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let values = [
            None,
            Some(OsString::from("on")),
            Some(OsString::from("ON")),
            Some(OsString::from("true")),
            Some(OsString::from("1")),
            Some(OsString::from("off")),
            Some(OsString::from("")),
            Some(OsString::from("on ")),
            Some(OsString::from_vec(vec![0xff])),
        ];
        for role in [
            "provider", "second", "capture", "speaker", "denoise", "echo", "unknown",
        ] {
            for value in &values {
                for (pass, name, roles) in [
                    (
                        memory_days_worker_environment as fn(_, _) -> _,
                        "LAMP_MEMORY_DAYS",
                        ["provider", "second"].as_slice(),
                    ),
                    // Each worker's own scheduling reaches every listed
                    // worker, and no other (#1608).
                    (
                        scheduling_worker_environment,
                        "LAMP_WORKER_SCHEDULING",
                        ["provider", "second", "capture", "speaker"].as_slice(),
                    ),
                    // The second route's scoped context reaches it alone.
                    (
                        second_scoped_context_worker_environment,
                        "LAMP_SECOND_SCOPED_CONTEXT",
                        ["second"].as_slice(),
                    ),
                    // Rules reach both routes too (twelve-P2).
                    (
                        memory_rules_worker_environment,
                        "LAMP_MEMORY_RULES",
                        ["provider", "second"].as_slice(),
                    ),
                    // The open threads (#1693 (3)) reach both routes, as the
                    // Days store does (twelve-P2).
                    (
                        memory_threads_worker_environment,
                        "LAMP_MEMORY_THREADS",
                        ["provider", "second"].as_slice(),
                    ),
                    // The dated events (#1693) reach both routes too.
                    (
                        memory_events_worker_environment,
                        "LAMP_MEMORY_EVENTS",
                        ["provider", "second"].as_slice(),
                    ),
                    // So does forgetting everything (#1693).
                    (
                        memory_forget_all_worker_environment,
                        "LAMP_MEMORY_FORGET_ALL",
                        ["provider", "second"].as_slice(),
                    ),
                    // So does recall's second version (#2176).
                    (
                        memory_recall_v2_worker_environment,
                        "LAMP_MEMORY_RECALL_V2",
                        ["provider", "second"].as_slice(),
                    ),
                    // So does keeping a fact as heard (#2176).
                    (
                        memory_keep_heard_worker_environment,
                        "LAMP_MEMORY_KEEP_HEARD",
                        ["provider", "second"].as_slice(),
                    ),
                    // A remember of what is kept already (#2176).
                    (
                        memory_remember_once_worker_environment,
                        "LAMP_MEMORY_REMEMBER_ONCE",
                        ["provider", "second"].as_slice(),
                    ),
                    // A correction replacing a kept thing (#2510).
                    (
                        memory_replaces_worker_environment,
                        "LAMP_MEMORY_REPLACES",
                        ["provider", "second"].as_slice(),
                    ),
                    // A forget that found nothing asks about a near kept thing.
                    (
                        memory_forget_asks_worker_environment,
                        "LAMP_MEMORY_FORGET_ASKS",
                        ["provider", "second"].as_slice(),
                    ),
                    // Memory keyed on the eyes' stable id (#2176).
                    (
                        memory_stable_id_worker_environment,
                        "LAMP_MEMORY_STABLE_ID",
                        ["provider", "second"].as_slice(),
                    ),
                    // The room's early words, the Live route alone (#2176).
                    (
                        memory_early_words_worker_environment,
                        "LAMP_MEMORY_EARLY_WORDS",
                        ["provider"].as_slice(),
                    ),
                    // The caller's own early lines, the second route alone (#2176).
                    (
                        memory_follows_face_worker_environment,
                        "LAMP_MEMORY_FOLLOWS_FACE",
                        ["second"].as_slice(),
                    ),
                    // And forgetting by what it is about (#2176).
                    (
                        memory_forget_about_worker_environment,
                        "LAMP_MEMORY_FORGET_ABOUT",
                        ["provider", "second"].as_slice(),
                    ),
                    // The room's kept things as someone's (#2176): both routes.
                    (
                        memory_room_not_asker_worker_environment,
                        "LAMP_MEMORY_ROOM_NOT_ASKER",
                        ["provider", "second"].as_slice(),
                    ),
                    // Nothing about a person kept while nobody is known: both.
                    (
                        memory_personal_needs_face_worker_environment,
                        "LAMP_MEMORY_PERSONAL_NEEDS_FACE",
                        ["provider", "second"].as_slice(),
                    ),
                    // The room's lines about a person after a forget of
                    // everything (#2849): both routes.
                    (
                        memory_room_after_forget_worker_environment,
                        "LAMP_MEMORY_ROOM_AFTER_FORGET",
                        ["provider", "second"].as_slice(),
                    ),
                    // An exchange linked to what its turn used (#2176): both.
                    (
                        memory_link_used_worker_environment,
                        "LAMP_MEMORY_LINK_USED",
                        ["provider", "second"].as_slice(),
                    ),
                    // Who said a room line, the second route alone (#2849).
                    (
                        memory_room_speaker_worker_environment,
                        "LAMP_MEMORY_ROOM_SPEAKER",
                        ["second"].as_slice(),
                    ),
                    // A plain ask to keep something calls remember: the
                    // second route alone.
                    (
                        memory_keep_asked_worker_environment,
                        "LAMP_MEMORY_KEEP_ASKED",
                        ["second"].as_slice(),
                    ),
                    // A plain ask for a timer or a reminder calls the
                    // schedule: the second route alone.
                    (
                        schedule_asked_worker_environment,
                        "LAMP_SCHEDULE_ASKED",
                        ["second"].as_slice(),
                    ),
                    // The observation pass, the route with a model (#1693 (2)).
                    (
                        memory_observe_worker_environment,
                        "LAMP_MEMORY_OBSERVE",
                        ["second"].as_slice(),
                    ),
                    // The note asked again after a quiet, the second route's
                    // (#1693 (1)).
                    (
                        memory_reopen_quiet_worker_environment,
                        "LAMP_MEMORY_REOPEN_QUIET",
                        ["second"].as_slice(),
                    ),
                    // The clock's switch, the two that tell the time.
                    (
                        kernel_clock_worker_environment,
                        "LAMP_KERNEL_CLOCK",
                        ["provider", "second"].as_slice(),
                    ),
                    // The echo evidence's skip, the capture worker's.
                    (
                        quiet_evidence_worker_environment,
                        "LAMP_QUIET_EVIDENCE",
                        ["capture"].as_slice(),
                    ),
                    // The capture loop's wait, the capture worker's.
                    (
                        capture_wait_worker_environment,
                        "LAMP_CAPTURE_WAIT",
                        ["capture"].as_slice(),
                    ),
                    // The per-stage line, the capture worker's.
                    (
                        capture_stages_worker_environment,
                        "LAMP_CAPTURE_STAGES",
                        ["capture"].as_slice(),
                    ),
                ] {
                    // Observe an actual child's environment without changing this
                    // test process's environment or exposing any inherited values.
                    let output = Command::new("/usr/bin/env")
                        .env("LAMP_TEST_UNRELATED", "synthetic")
                        .env_clear()
                        .envs(pass(role, value.as_deref()))
                        .output()
                        .unwrap();
                    assert!(output.status.success());
                    let expected = if roles.contains(&role)
                        && value.as_deref() == Some(std::ffi::OsStr::new("on"))
                    {
                        format!("{name}=on\n")
                    } else {
                        String::new()
                    };
                    assert_eq!(
                        output.stdout,
                        expected.as_bytes(),
                        "role {role}, value {value:?}"
                    );
                }
            }
        }
    }

    /// The detector's input (#828) goes to the capture worker only, and only
    /// as one of its two exact values.
    #[test]
    fn the_detector_input_reaches_capture_only_as_an_exact_value() {
        for role in ["capture", "provider", "second", "speaker", "ring", "camera"] {
            for (value, passed) in [
                ("linear", Some("linear")),
                ("linear-gated", Some("linear-gated")),
                ("on", None),
                ("Linear", None),
                ("linear-gated ", None),
            ] {
                let got = vad_input_worker_environment(role, Some(std::ffi::OsStr::new(value)));
                let expected = passed
                    .filter(|_| role == "capture")
                    .map(|passed| ("LAMP_VAD_INPUT", passed));
                assert_eq!(got, expected, "role {role}, value {value:?}");
            }
            assert_eq!(vad_input_worker_environment(role, None), None);
        }
    }

    /// The typed name's file goes to the provider and second workers only,
    /// and only as an absolute path (#806, Fourth on #1323).
    #[test]
    fn the_typed_names_file_reaches_only_the_workers_that_tell_the_model() {
        let absolute = std::ffi::OsStr::new("/etc/lampOS/person/name");
        for role in ["provider", "second"] {
            assert_eq!(
                person_name_worker_environment(role, Some(absolute)),
                Some((crate::person::ENVIRONMENT, absolute.to_owned()))
            );
            assert_eq!(person_name_worker_environment(role, None), None);
            for unfit in ["person/name", "", "on"] {
                assert_eq!(
                    person_name_worker_environment(role, Some(std::ffi::OsStr::new(unfit))),
                    None,
                    "{unfit:?}"
                );
            }
        }
        for role in [
            "capture", "speaker", "denoise", "echo", "privacy", "unknown",
        ] {
            assert_eq!(
                person_name_worker_environment(role, Some(absolute)),
                None,
                "{role}"
            );
        }
    }

    /// #2602: the owner-only switch goes to the provider and second workers
    /// only, and only as `on`.
    #[test]
    fn the_owner_only_switch_reaches_only_the_workers_that_tell_the_model() {
        let on = std::ffi::OsStr::new("on");
        for role in ["provider", "second"] {
            assert_eq!(
                person_name_owner_only_worker_environment(role, Some(on)),
                Some((crate::person::OWNER_ONLY_ENVIRONMENT, "on"))
            );
            assert_eq!(person_name_owner_only_worker_environment(role, None), None);
            for unfit in ["off", "", "ON", "on "] {
                assert_eq!(
                    person_name_owner_only_worker_environment(
                        role,
                        Some(std::ffi::OsStr::new(unfit))
                    ),
                    None,
                    "{unfit:?}"
                );
            }
        }
        for role in ["capture", "speaker", "ring", "camera", "privacy"] {
            assert_eq!(
                person_name_owner_only_worker_environment(role, Some(on)),
                None,
                "{role}"
            );
        }
    }

    /// The reviewer on #1158: the switches reach a worker through the real
    /// command it is started with. A script in the worker's place writes
    /// the environment it got: the provider has the memory's two switches,
    /// the clock and the typed name's file, the second route the memory's
    /// days, the clock, the name's file (#1323) and its scoped context
    /// (#1839), both task news's told file (#2762), capture the quiet
    /// evidence switch (#1285) and the detector's input (#828), and the ring
    /// the unplug switch (#1674, the reviewer on #1699). With every other variable set to a secret,
    /// nothing else reaches any of them but the shell's own.
    #[test]
    fn a_started_worker_gets_its_roles_switches_and_nothing_else() {
        use std::os::unix::fs::PermissionsExt;
        let home = SessionDirectory::create().unwrap();
        let script = home.path.join("worker.sh");
        std::fs::write(&script, "#!/bin/sh\n/usr/bin/env > \"$3/env-$2.txt\"\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Every variable the voice has is set: the three switches `on`, and
        // anything else to a secret, so nothing the command reads besides
        // them can pass unseen (the reviewer on #1295).
        let on = |name: &str| {
            Some(std::ffi::OsString::from(
                if [
                    "LAMP_MEMORY_DAYS",
                    crate::rules::ENVIRONMENT,
                    crate::remember::FORGET_ALL_ENVIRONMENT,
                    crate::remember::ROOM_NOT_ASKER_ENVIRONMENT,
                    crate::remember::ROOM_AFTER_FORGET_ENVIRONMENT,
                    crate::remember::needs_face::ENVIRONMENT,
                    crate::remember::FORGET_ABOUT_ENVIRONMENT,
                    crate::remember::JOURNAL_RESUMES_ENVIRONMENT,
                    crate::remember::LINK_USED_ENVIRONMENT,
                    crate::remember::ROOM_SPEAKER_ENVIRONMENT,
                    crate::remember::asked::ENVIRONMENT,
                    crate::tools::schedule_asked::ENVIRONMENT,
                    crate::remember::THREADS_ENVIRONMENT,
                    crate::remember::EVENTS_ENVIRONMENT,
                    crate::remember::OBSERVE_ENVIRONMENT,
                    crate::second_provider::worker::REOPEN_QUIET_ENVIRONMENT,
                    crate::remember::RECALL_V2_ENVIRONMENT,
                    crate::remember::heard::ENVIRONMENT,
                    crate::remember::REMEMBER_ONCE_ENVIRONMENT,
                    crate::remember::REPLACES_ENVIRONMENT,
                    crate::remember::FORGET_ASKS_ENVIRONMENT,
                    lamp_msgs::known::ENVIRONMENT,
                    crate::early_words::ENVIRONMENT,
                    crate::follows_face::ENVIRONMENT,
                    crate::local_time::KERNEL_CLOCK_ENVIRONMENT,
                    crate::quiet_pace::EVIDENCE_ENVIRONMENT,
                    crate::second_provider::net::key_terms::ENVIRONMENT,
                    crate::second_config::FLUX_ENV,
                    crate::attention::NEAR_CALIBRATED_ENVIRONMENT,
                    crate::memory_scope::GAP_ENVIRONMENT,
                    crate::second_provider::worker::SCOPED_CONTEXT_ENVIRONMENT,
                    lamp_body::unplug::ENVIRONMENT,
                    crate::person::OWNER_ONLY_ENVIRONMENT,
                ]
                .contains(&name)
                {
                    "on"
                } else if name == crate::activity::INPUT_ENVIRONMENT {
                    "linear"
                } else if name == crate::capture_cores::ENVIRONMENT {
                    "6-7"
                } else if name == crate::person::ENVIRONMENT {
                    "/etc/lampOS/person/name"
                } else if name == crate::task_news::TOLD_ENVIRONMENT {
                    "/var/lib/lampOS/voice/tasks-told"
                } else if name == crate::second_config::CREDENTIALS_ENV {
                    "/run/credentials/lamp-voice.service"
                } else if name == crate::second_config::GEMINI_BASE_URL_ENV {
                    "https://relay.example.invalid/v1beta"
                } else if name == crate::second_config::DEEPSEEK_BASE_URL_ENV {
                    "https://relay.example.invalid/deepseek"
                } else if name == crate::second_config::SPEECH_BASE_URL_ENV
                    || name == crate::second_config::LISTENING_BASE_URL_ENV
                {
                    "https://near.example.invalid/ai"
                } else if name == crate::second_config::SPEECH_MODEL_ENV {
                    "gemini-3.8-flash-lite-tts"
                } else if name == crate::second_provider::SPEECH_SOCKET_MODEL_ENVIRONMENT {
                    "eleven_flash_v2_5"
                } else if name == crate::second_config::KEY_TERMS_ENV {
                    "Hanoi"
                } else if name == crate::local_time::PLACE_ENVIRONMENT {
                    "Hanoi, Vietnam"
                } else if name == crate::second_provider::net::weather::ENVIRONMENT
                    || name == crate::second_provider::NAME_SECOND_SIGN_ENVIRONMENT
                    || name == crate::second_provider::NAME_SAID_TO_A_PERSON_ENVIRONMENT
                    || name == crate::second_provider::NAME_STAYS_ENVIRONMENT
                    || name == crate::second_provider::NAME_START_USUAL_ENVIRONMENT
                    || name == crate::second_provider::PREPARED_ENVIRONMENT
                    || name == crate::second_provider::STT_WARM_ENVIRONMENT
                    || name == crate::second_provider::net::QUIET_HELD_ENVIRONMENT
                    || name == crate::second_provider::CHAT_WARM_ENVIRONMENT
                    || name == crate::second_provider::net::CHAT_KEPT_ENVIRONMENT
                    || name == crate::second_provider::SPEECH_WARM_ENVIRONMENT
                    || name == crate::second_provider::SPEECH_SOCKET_ENVIRONMENT
                    || name == crate::second_provider::RELAY_ROUTES_ENVIRONMENT
                    || name == crate::second_provider::STEADY_FIRST_ENVIRONMENT
                    || name == crate::second_provider::QUIET_GOING_ON_SAME_ONE_ENVIRONMENT
                    || name == crate::second_provider::FEW_TOOLS_ENVIRONMENT
                    || name == crate::second_provider::SAID_ONCE_ENVIRONMENT
                    || name == crate::second_provider::SPEECH_TAGS_ENVIRONMENT
                    || name == crate::second_provider::SHORT_DONE_ENVIRONMENT
                    || name == crate::second_provider::NO_WORDS_AGAIN_ENVIRONMENT
                    || name == crate::second_provider::NAMED_ALWAYS_ENVIRONMENT
                    || name == crate::remember::HITS_ENVIRONMENT
                    || name == crate::second_provider::SKILL_LOOK_ENVIRONMENT
                    || name == crate::second_provider::GOING_ON_NOT_OWN_ENVIRONMENT
                    || name == crate::second_provider::BUSY_CALLED_STRICT_ENVIRONMENT
                    || name == crate::remark_about_it::ENVIRONMENT
                    || name == crate::second_provider::TURNED_SPEAKING_ENVIRONMENT
                    || name == crate::second_provider::JOIN_CUT_WORDS_ENVIRONMENT
                    || name == crate::unfinished_words::ENVIRONMENT
                    || name == crate::same_voice::CALLER_ONLY_ENVIRONMENT
                    || name == crate::second_provider::WRITTEN_WAIT_ENVIRONMENT
                    || name == crate::second_provider::CUE_AFTER_ENVIRONMENT
                    || name == crate::second_provider::OPEN_NAME_ENVIRONMENT
                    || name == crate::second_provider::NAME_CARRIES_ENVIRONMENT
                    || name == crate::second_provider::FIRST_SECONDS_QUIET_ENVIRONMENT
                    || name == lamp_msgs::NameHeard::NEAR_ENVIRONMENT
                    || name == crate::second_provider::OWN_ECHO_ENVIRONMENT
                    || name == crate::second_provider::FACE_NAME_ENVIRONMENT
                    || name == crate::second_provider::BUSY_ENDS_SOONER_ENVIRONMENT
                    || name == crate::second_provider::SAME_SPEAKER_CARRIES_ENVIRONMENT
                    || name == crate::second_provider::TURNED_FACE_SPEAKS_ENVIRONMENT
                    || name == crate::second_provider::TWO_FACES_HELD_ENVIRONMENT
                    || name == crate::second_provider::PERSON_SCOPE_SEEN_ENVIRONMENT
                    || name == crate::person::BY_DEFAULT_ENVIRONMENT
                    || name == crate::second_provider::LOOK_NOTE_ENVIRONMENT
                    || name == crate::second_provider::LOOK_WALK_ENVIRONMENT
                    || name == crate::second_provider::NOT_FOR_ME_WORDS_ENVIRONMENT
                    || name == crate::speaking_faces::ENVIRONMENT
                    || name == crate::speaking_faces::EYES_ENVIRONMENT
                    || name == crate::agent_tasks::NAMED_ASK_ENVIRONMENT
                    || name == crate::known_faces::ASKED_ENVIRONMENT
                    || name == crate::second_config::OWN_KEYS_ENV
                    || name == crate::owner_place::ENVIRONMENT
                {
                    "on"
                } else if name == crate::remember::FORGET_ASKED_ENVIRONMENT {
                    "off"
                } else if name == crate::second_provider::FIRST_WORDS_ENVIRONMENT {
                    "4"
                } else {
                    "a-secret"
                },
            ))
        };
        // What the child has that the command did not give it: the shell's
        // own, and the stall switches this test process may have.
        let stalls: Vec<String> = crate::stalls::passed_on()
            .into_iter()
            .map(|(name, value)| format!("{name}={}", value.to_string_lossy()))
            .collect();
        // A failure says only the names it got, never their values: a
        // runner's tokens must not reach a CI log (the reviewer on #1311).
        let names = |lines: &[String]| -> Vec<String> {
            lines
                .iter()
                .map(|line| line.split('=').next().unwrap_or_default().to_owned())
                .collect()
        };
        let given = |written: &str| -> Vec<String> {
            let mut lines: Vec<String> = written
                .lines()
                .filter(|line| {
                    let name = line.split('=').next().unwrap_or_default();
                    !["PWD", "OLDPWD", "SHLVL", "_"].contains(&name)
                        && !stalls.iter().any(|stall| stall == line)
                })
                .map(str::to_owned)
                .collect();
            lines.sort_unstable();
            lines
        };
        let boot = new_boot().unwrap();
        for (role, expected) in [
            (
                "provider",
                vec![
                    "LAMP_ATTENTION_NEAR_CALIBRATED=on",
                    "LAMP_KERNEL_CLOCK=on",
                    "LAMP_MEMORY_DAYS=on",
                    "LAMP_MEMORY_EARLY_WORDS=on",
                    "LAMP_MEMORY_EVENTS=on",
                    "LAMP_MEMORY_FORGET_ABOUT=on",
                    "LAMP_MEMORY_FORGET_ALL=on",
                    "LAMP_MEMORY_FORGET_ASKS=on",
                    "LAMP_MEMORY_JOURNAL_RESUMES=on",
                    "LAMP_MEMORY_KEEP_HEARD=on",
                    "LAMP_MEMORY_LINK_USED=on",
                    "LAMP_MEMORY_PERSONAL_NEEDS_FACE=on",
                    "LAMP_MEMORY_RECALL_V2=on",
                    "LAMP_MEMORY_REMEMBER_ONCE=on",
                    "LAMP_MEMORY_REPLACES=on",
                    "LAMP_MEMORY_ROOM_AFTER_FORGET=on",
                    "LAMP_MEMORY_ROOM_NOT_ASKER=on",
                    "LAMP_MEMORY_RULES=on",
                    "LAMP_MEMORY_STABLE_ID=on",
                    "LAMP_MEMORY_THREADS=on",
                    "LAMP_PERSON_NAME=/etc/lampOS/person/name",
                    "LAMP_PERSON_NAME_OWNER_ONLY=on",
                    "LAMP_TASKS_TOLD=/var/lib/lampOS/voice/tasks-told",
                ],
            ),
            (
                "second",
                vec![
                    "CREDENTIALS_DIRECTORY=/run/credentials/lamp-voice.service",
                    "LAMP_ATTENTION_NEAR_CALIBRATED=on",
                    "LAMP_BUSY_CALLED_STRICT=on",
                    "LAMP_BUSY_ENDS_SOONER=on",
                    "LAMP_EYES_SPEAKING=on",
                    "LAMP_FACE_GAP_HOLD=on",
                    "LAMP_FIRST_SECONDS_QUIET=on",
                    "LAMP_FORGET_ASKED=off",
                    "LAMP_GOING_ON_NOT_OWN=on",
                    "LAMP_JOIN_CUT_WORDS=on",
                    "LAMP_KERNEL_CLOCK=on",
                    "LAMP_MEMORY_DAYS=on",
                    "LAMP_MEMORY_EVENTS=on",
                    "LAMP_MEMORY_FOLLOWS_FACE=on",
                    "LAMP_MEMORY_FORGET_ABOUT=on",
                    "LAMP_MEMORY_FORGET_ALL=on",
                    "LAMP_MEMORY_FORGET_ASKS=on",
                    "LAMP_MEMORY_JOURNAL_RESUMES=on",
                    "LAMP_MEMORY_KEEP_ASKED=on",
                    "LAMP_MEMORY_KEEP_HEARD=on",
                    "LAMP_MEMORY_LINK_USED=on",
                    "LAMP_MEMORY_OBSERVE=on",
                    "LAMP_MEMORY_PERSONAL_NEEDS_FACE=on",
                    "LAMP_MEMORY_RECALL_V2=on",
                    "LAMP_MEMORY_REMEMBER_ONCE=on",
                    "LAMP_MEMORY_REOPEN_QUIET=on",
                    "LAMP_MEMORY_REPLACES=on",
                    "LAMP_MEMORY_ROOM_AFTER_FORGET=on",
                    "LAMP_MEMORY_ROOM_NOT_ASKER=on",
                    "LAMP_MEMORY_ROOM_SPEAKER=on",
                    "LAMP_MEMORY_RULES=on",
                    "LAMP_MEMORY_STABLE_ID=on",
                    "LAMP_MEMORY_THREADS=on",
                    "LAMP_NAMED_ALWAYS=on",
                    "LAMP_NAME_NEAR=on",
                    "LAMP_NAME_SAID_TO_A_PERSON=on",
                    "LAMP_NAME_SECOND_SIGN=on",
                    "LAMP_NAME_START_USUAL=on",
                    "LAMP_NAME_STAYS_IN_ITS_LINE=on",
                    "LAMP_NOT_FOR_ME_WORDS=on",
                    "LAMP_NO_WORDS_AGAIN=on",
                    "LAMP_OWNER_PLACE=on",
                    "LAMP_OWN_KEYS=on",
                    "LAMP_PERSON_NAME=/etc/lampOS/person/name",
                    "LAMP_PERSON_NAME_BY_DEFAULT=on",
                    "LAMP_PERSON_NAME_OWNER_ONLY=on",
                    "LAMP_PERSON_SCOPE_SEEN=on",
                    "LAMP_PLACE=Hanoi, Vietnam",
                    "LAMP_QUIET_GOING_ON_SAME_ONE=on",
                    "LAMP_RELAY_ROUTES=on",
                    "LAMP_REMARK_ABOUT_IT=on",
                    "LAMP_SAME_SPEAKER_CARRIES=on",
                    "LAMP_SAME_VOICE_CALLER_ONLY=on",
                    "LAMP_SCHEDULE_ASKED=on",
                    "LAMP_SECOND_CHAT_KEPT=on",
                    "LAMP_SECOND_CHAT_WARM=on",
                    "LAMP_SECOND_CUE_AFTER_MS=on",
                    "LAMP_SECOND_DEEPSEEK_BASE_URL=https://relay.example.invalid/deepseek",
                    "LAMP_SECOND_FACE_NAME=on",
                    "LAMP_SECOND_FEW_TOOLS=on",
                    "LAMP_SECOND_FIRST_WORDS=4",
                    "LAMP_SECOND_FLUX=on",
                    "LAMP_SECOND_GEMINI_BASE_URL=https://relay.example.invalid/v1beta",
                    "LAMP_SECOND_KEY_TERMS=Hanoi",
                    "LAMP_SECOND_KNOWN_NAMES=on",
                    "LAMP_SECOND_LISTENING_BASE_URL=https://near.example.invalid/ai",
                    "LAMP_SECOND_LOOK_NOTE=on",
                    "LAMP_SECOND_LOOK_WALK=on",
                    "LAMP_SECOND_MEMORY_HITS=on",
                    "LAMP_SECOND_NAME_CARRIES=on",
                    "LAMP_SECOND_OPEN_NAME=on",
                    "LAMP_SECOND_OWN_ECHO=on",
                    "LAMP_SECOND_PREPARED=on",
                    "LAMP_SECOND_QUIET_HELD=on",
                    "LAMP_SECOND_REMEMBER_ASKED=on",
                    "LAMP_SECOND_SAID_ONCE=on",
                    "LAMP_SECOND_SCOPED_CONTEXT=on",
                    "LAMP_SECOND_SKILL_LOOK=on",
                    "LAMP_SECOND_SPEAKING_FACE=on",
                    "LAMP_SECOND_SPEECH_BASE_URL=https://near.example.invalid/ai",
                    "LAMP_SECOND_SPEECH_MODEL=gemini-3.8-flash-lite-tts",
                    "LAMP_SECOND_SPEECH_SOCKET=on",
                    "LAMP_SECOND_SPEECH_SOCKET_MODEL=eleven_flash_v2_5",
                    "LAMP_SECOND_SPEECH_TAGS=on",
                    "LAMP_SECOND_SPEECH_WARM=on",
                    "LAMP_SECOND_STEADY_FIRST=on",
                    "LAMP_SECOND_STT_WARM=on",
                    "LAMP_SECOND_UNFINISHED_WAIT=on",
                    "LAMP_SECOND_WEATHER=on",
                    "LAMP_SECOND_WRITTEN_WAIT=on",
                    "LAMP_SHORT_DONE=on",
                    "LAMP_TASKS_NAMED_ASK=on",
                    "LAMP_TASKS_TOLD=/var/lib/lampOS/voice/tasks-told",
                    "LAMP_TURNED_FACE_SPEAKS=on",
                    "LAMP_TURNED_SPEAKING=on",
                    "LAMP_TWO_FACES_HELD=on",
                ],
            ),
            (
                "capture",
                vec![
                    "LAMP_CAPTURE_CORES=6-7",
                    "LAMP_QUIET_EVIDENCE=on",
                    "LAMP_VAD_INPUT=linear",
                ],
            ),
            ("ring", vec!["LAMP_UNPLUG=on"]),
            ("speaker", vec![]),
        ] {
            let status = status_of_fresh(&mut worker_command(
                &script,
                &home.path,
                role,
                (boot, boot),
                &[],
                on,
            ));
            assert!(status.success(), "{role}");
            let written =
                std::fs::read_to_string(home.path.join(format!("env-{role}.txt"))).unwrap();
            let got = given(&written);
            assert!(got == expected, "{role}: the child got {:?}", names(&got));
        }
        let status = status_of_fresh(&mut worker_command(
            &script,
            &home.path,
            "provider",
            (boot, boot),
            &[],
            |_| None,
        ));
        assert!(status.success());
        let written = std::fs::read_to_string(home.path.join("env-provider.txt")).unwrap();
        let got = given(&written);
        assert!(got.is_empty(), "the child got {:?}", names(&got));
    }

    /// `LAMP_PRIVACY_CONTROLS=no-slide`: the privacy worker's environment is
    /// cleared, so a started worker learns the route from its arguments,
    /// exactly as the voice's own command line reads them back, and never
    /// from the switch, which does not reach it.
    #[test]
    fn a_started_privacy_worker_learns_no_slide_from_its_arguments_only() {
        use crate::privacy_bus::{Route, worker_arguments, worker_route};
        use std::os::unix::fs::PermissionsExt;
        let home = SessionDirectory::create().unwrap();
        let script = home.path.join("worker.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\ndir=\"$3\"\nshift 5\nprintf '%s\\n' \"$@\" > \"$dir/args.txt\"\n/usr/bin/env > \"$dir/env.txt\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = Path::new("/run/lampos-bus");
        let extra = worker_arguments(Some((root, Route::NoSlide))).unwrap();
        let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
        let boot = new_boot().unwrap();
        let status = status_of_fresh(&mut worker_command(
            &script,
            &home.path,
            "privacy",
            (boot, boot),
            &extra,
            |name| (name == crate::privacy_bus::ENVIRONMENT).then(|| "no-slide".into()),
        ));
        assert!(status.success());
        let given: Vec<String> = std::fs::read_to_string(home.path.join("args.txt"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(given, ["--controls-root", "/run/lampos-bus", "--no-slide"]);
        assert_eq!(
            worker_route(&given).unwrap(),
            (root.into(), Route::NoSlide, false)
        );
        let environment = std::fs::read_to_string(home.path.join("env.txt")).unwrap();
        assert!(!environment.contains(crate::privacy_bus::ENVIRONMENT));
    }

    fn child_worker(directory: &SessionDirectory, script: &str) -> Worker {
        let parent = new_boot().unwrap();
        let boot = new_boot().unwrap();
        let channels = WorkerChannels::bind(&directory.path, "probe", parent, boot, false).unwrap();
        let child = Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Worker {
            channels,
            child,
            boot,
            role: "probe".into(),
            diagnostic_start: None,
            ring_resources: None,
            #[cfg(test)]
            on_stop: None,
            #[cfg(test)]
            after_terminate: None,
        }
    }

    #[test]
    fn shutdown_reports_an_already_exited_nonzero_child() {
        let directory = SessionDirectory::create().unwrap();
        let mut worker = child_worker(&directory, "exit 23");
        // Reap explicitly here to make the pre-existing-exit case deterministic.
        assert_eq!(worker.child.wait().unwrap().code(), Some(23));
        let error = worker.shutdown().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("exited abnormally"));
        assert_eq!(worker.child.try_wait().unwrap().unwrap().code(), Some(23));
    }

    #[test]
    fn signal_only_termination_leaves_reaping_to_final_shutdown() {
        use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
        let directory = SessionDirectory::create().unwrap();
        let mut worker = child_worker(&directory, "exec sleep 60");
        let pid = Pid::from_raw(worker.child.id().try_into().unwrap()).unwrap();
        worker.request_termination().unwrap();
        // WNOWAIT is evidence that the signal-only call did not wait/reap.
        // Replacing it with terminate_and_reap makes waitid fail with ECHILD.
        let deadline = Instant::now() + Duration::from_secs(2);
        let observed = loop {
            let state = waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
            )
            .unwrap();
            if let Some(state) = state {
                break state;
            }
            assert!(Instant::now() < deadline, "owned child did not exit");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(observed.terminating_signal().is_some());
        assert!(
            worker.shutdown().is_err(),
            "forced termination stays a failure"
        );
        assert!(
            worker.try_exit().unwrap().is_some(),
            "final cleanup reaped it"
        );
    }

    #[test]
    fn shutdown_kills_and_reaps_a_hung_child_but_never_reports_success() {
        let directory = SessionDirectory::create().unwrap();
        // exec leaves one owned child, not a shell with an orphaned sleep process.
        let mut worker = child_worker(&directory, "exec sleep 60");
        let started = Instant::now();
        let error = worker.shutdown().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("forced termination required"));
        assert!(error.to_string().contains("reaped with"));
        assert!(!worker.child.try_wait().unwrap().unwrap().success());
        // This is observed test behavior with slack, not a hard OS deadline claim.
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

#[cfg(test)]
pub(crate) mod ring_startup_tests;

#[cfg(test)]
mod switch_census_tests;
