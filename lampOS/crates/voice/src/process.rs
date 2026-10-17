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

    /// What ends this fixture's child once Stop has been sent to it.
    #[cfg(test)]
    pub(crate) fn on_stop(&mut self, end: impl FnOnce() -> io::Result<()> + Send + 'static) {
        self.on_stop = Some(Box::new(end));
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
        mut on_wait: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Self> {
        listed(role)?;
        let channels = WorkerChannels::bind(directory, role, parent, boot, false)?;
        let child = worker_command(executable, directory, role, (parent, boot), extra, |name| {
            std::env::var_os(name)
        })
        .spawn()?;
        let mut worker = Self {
            channels,
            child,
            boot,
            role: role.into(),
            diagnostic_start: None,
            ring_resources: None,
            #[cfg(test)]
            on_stop: None,
        };
        worker.wait_until_ready(directory, &mut on_wait)?;
        Ok(worker)
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
        self.channels.connect(directory, &role, false)?;
        loop {
            on_wait()?;
            self.check_running()?;
            match self.channels.control.receive()? {
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
            return Err(io::Error::other(format!(
                "{} worker exited: {status}",
                self.role
            )));
        }
        Ok(())
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
        drop(capture_peer);
        drop(ring_peer);
        // The first sends fail; no probe/retry, delayed assertion, or killed child
        // is used to manufacture a disconnected transport.
        assert!(capture.channels.control.send(Control::Stop).is_err());
        assert!(ring.channels.control.send(Control::Stop).is_err());
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
        .envs(kernel_clock_worker_environment(
            role,
            var(crate::local_time::KERNEL_CLOCK_ENVIRONMENT).as_deref(),
        ))
        .envs(quiet_evidence_worker_environment(
            role,
            var(crate::quiet_pace::EVIDENCE_ENVIRONMENT).as_deref(),
        ))
        .envs(vad_input_worker_environment(
            role,
            var(crate::activity::INPUT_ENVIRONMENT).as_deref(),
        ))
        .envs(person_name_worker_environment(
            role,
            var(crate::person::ENVIRONMENT).as_deref(),
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

// The rules' switch (#813), passed the same narrow way: the exact `on`, to
// the provider only.
fn memory_rules_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "provider" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::rules::ENVIRONMENT, "on"))
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

// The echo evidence's skip (#705), to the one worker that runs it: capture.
fn quiet_evidence_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "capture" && value == Some(std::ffi::OsStr::new("on")))
        .then_some((crate::quiet_pace::EVIDENCE_ENVIRONMENT, "on"))
}

// The detector's input (#828), to the one worker that runs it: capture,
// and only the exact `linear`.
fn vad_input_worker_environment(
    role: &str,
    value: Option<&std::ffi::OsStr>,
) -> Option<(&'static str, &'static str)> {
    (role == "capture" && value == Some(std::ffi::OsStr::new("linear")))
        .then_some((crate::activity::INPUT_ENVIRONMENT, "linear"))
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

#[cfg(test)]
mod tests {
    use super::*;

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
                    // Rules reach the Live provider only.
                    (
                        memory_rules_worker_environment,
                        "LAMP_MEMORY_RULES",
                        ["provider"].as_slice(),
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

    /// The reviewer on #1158: the switches reach a worker through the real
    /// command it is started with. A script in the worker's place writes
    /// the environment it got: the provider has the memory's two switches,
    /// the clock and the typed name's file, the second route the memory's
    /// days, the clock and the name's file (#1323), and capture the quiet
    /// evidence switch (#1285) and the detector's input (#828). With every other variable set to a secret,
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
                    crate::local_time::KERNEL_CLOCK_ENVIRONMENT,
                    crate::quiet_pace::EVIDENCE_ENVIRONMENT,
                ]
                .contains(&name)
                {
                    "on"
                } else if name == crate::activity::INPUT_ENVIRONMENT {
                    "linear"
                } else if name == crate::person::ENVIRONMENT {
                    "/etc/lampOS/person/name"
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
                    "LAMP_KERNEL_CLOCK=on",
                    "LAMP_MEMORY_DAYS=on",
                    "LAMP_MEMORY_RULES=on",
                    "LAMP_PERSON_NAME=/etc/lampOS/person/name",
                ],
            ),
            (
                "second",
                vec![
                    "LAMP_KERNEL_CLOCK=on",
                    "LAMP_MEMORY_DAYS=on",
                    "LAMP_PERSON_NAME=/etc/lampOS/person/name",
                ],
            ),
            (
                "capture",
                vec!["LAMP_QUIET_EVIDENCE=on", "LAMP_VAD_INPUT=linear"],
            ),
        ] {
            let status = worker_command(&script, &home.path, role, (boot, boot), &[], on)
                .status()
                .unwrap();
            assert!(status.success(), "{role}");
            let written =
                std::fs::read_to_string(home.path.join(format!("env-{role}.txt"))).unwrap();
            let got = given(&written);
            assert!(got == expected, "{role}: the child got {:?}", names(&got));
        }
        let status = worker_command(&script, &home.path, "provider", (boot, boot), &[], |_| None)
            .status()
            .unwrap();
        assert!(status.success());
        let written = std::fs::read_to_string(home.path.join("env-provider.txt")).unwrap();
        let got = given(&written);
        assert!(got.is_empty(), "the child got {:?}", names(&got));
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
