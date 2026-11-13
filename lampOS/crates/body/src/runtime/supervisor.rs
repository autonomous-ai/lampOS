//! Nonblocking child supervision. Replacements are allowed only after waitpid.
use super::{
    SystemClock,
    channel::Channels,
    config::{Config, bounded_path, invalid},
    wire::{Command as MotorCommand, Control, Event},
};
use crate::control::Clock;
use lamp_turn_owner::BootId;
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
};

pub const STARTUP_US: u64 = 3_000_000;
pub const PROGRESS_DEADLINE_US: u64 = 50_000;
pub const INHIBIT_DEADLINE_US: u64 = 20_000;
pub const TEARDOWN_US: u64 = 250_000;
/// Reaping is polled, never blocking. Missing this deadline is a failure, not
/// proof of release; the still-owned child remains in Supervisor for later reap.
pub const REAP_US: u64 = 250_000;

pub fn new_boot() -> io::Result<BootId> {
    let mut b = [0; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    BootId::new(b).map_err(invalid)
}
pub fn boot_text(b: BootId) -> String {
    b.bytes().iter().map(|v| format!("{v:02x}")).collect()
}
pub fn parse_boot(s: &str) -> io::Result<BootId> {
    if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("invalid boot ID"));
    }
    let mut b = [0; 16];
    for (i, v) in b.iter_mut().enumerate() {
        *v = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(invalid)?;
    }
    BootId::new(b).map_err(invalid)
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    uid: u32,
}
impl Identity {
    fn of(meta: &fs::Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            uid: meta.uid(),
        }
    }
}
struct Directory {
    path: PathBuf,
    identity: Identity,
    peers: [Option<Identity>; 2],
    peers_cleaned: bool,
    cleanup_error: Option<String>,
    may_remove: bool,
}
impl Directory {
    const PEERS: [&'static str; 2] = ["motor.c.w", "motor.d.w"];

    fn new(path: &Path) -> io::Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_dir()
            || meta.uid() != rustix::process::geteuid().as_raw()
            || meta.mode() & 0o077 != 0
        {
            return Err(invalid("motor directory is not owned/private"));
        }
        Ok(Self {
            path: path.to_owned(),
            identity: Identity::of(&meta),
            peers: [None; 2],
            peers_cleaned: false,
            cleanup_error: None,
            may_remove: true,
        })
    }
    fn check(&self) -> io::Result<()> {
        let meta = fs::symlink_metadata(&self.path)?;
        if !meta.is_dir() || Identity::of(&meta) != self.identity || meta.mode() & 0o077 != 0 {
            return Err(invalid("motor directory identity/privacy changed"));
        }
        Ok(())
    }
    /// Only the single spawned child may bind in this new private directory.
    /// Pin each first-observed socket independently, including partial startup.
    /// Never adopt a later replacement. This also runs before observing reap.
    fn observe_peers(&mut self) -> io::Result<()> {
        self.check()?;
        for (i, name) in Self::PEERS.iter().enumerate() {
            if self.peers[i].is_some() {
                continue;
            }
            let meta = match fs::symlink_metadata(self.path.join(name)) {
                Ok(meta) => meta,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            if !meta.file_type().is_socket() || meta.uid() != self.identity.uid {
                return Err(invalid("motor child endpoint is not an owned socket"));
            }
            self.peers[i] = Some(Identity::of(&meta));
        }
        Ok(())
    }
    /// Call only after this Supervisor's exact Child has been observed reaped.
    /// Two fixed paths, no recursive removal, no adoption of replacement files.
    fn cleanup_reaped_peers(&mut self) -> io::Result<()> {
        if self.peers_cleaned || self.cleanup_error.is_some() {
            return Ok(());
        }
        self.may_remove = true;
        if let Err(e) = self.check() {
            self.cleanup_error = Some(super::wire::bounded(&e));
            return Err(e);
        }
        let mut failure = None;
        for (i, name) in Self::PEERS.iter().enumerate() {
            let path = self.path.join(name);
            let result = (|| {
                let meta = match fs::symlink_metadata(&path) {
                    Ok(meta) => meta,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
                    Err(e) => return Err(e),
                };
                if !meta.file_type().is_socket()
                    || self.peers[i] != Some(Identity::of(&meta))
                    || meta.mode() & 0o077 != 0
                {
                    return Err(invalid("motor child endpoint identity changed; retained"));
                }
                fs::remove_file(path)
            })();
            if let Err(e) = result {
                failure.get_or_insert(e);
            }
        }
        // A failed identity check is not retried against a different pathname.
        if let Some(e) = failure {
            self.cleanup_error = Some(super::wire::bounded(&e));
            Err(e)
        } else {
            self.peers_cleaned = true;
            Ok(())
        }
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        // Parent endpoints are dropped first. Never remove a renamed/replaced
        // directory, and never recursively delete unknown entries.
        if self.may_remove && self.check().is_ok() {
            let _ = fs::remove_dir(&self.path);
        }
    }
}
/// No automatic restart exists. Explicit replacement requires observed reap and
/// a fresh directory/boot. Child crash cannot trigger automatic motion.
/// Production uses `SystemClock`; a test may pass a stepped clock instead, and
/// every deadline below is then measured on it.
pub struct Supervisor<C: Clock = SystemClock> {
    channels: Channels,
    child: Child,
    directory: Directory,
    pub worker: BootId,
    parent: BootId,
    started: u64,
    connected: bool,
    ready: bool,
    last_observed: u64,
    stop_at: Option<u64>,
    inhibited: bool,
    inhibited_received: Option<u64>,
    stopped: bool,
    expect_serial: bool,
    privacy_revision: Option<u64>,
    sent_privacy: Option<(u64, u64)>,
    release_io_verified: bool,
    release_discard_verified: bool,
    lifecycle_fault: bool,
    killed_at: Option<u64>,
    exit: Option<ExitStatus>,
    failure: Option<String>,
    clock: C,
    /// Tests only: the inhibit deadline a clean-stop test waits for. A test
    /// child is a re-executed test binary on a possibly loaded host, so it gets
    /// longer than the production bound; the production build always uses
    /// `INHIBIT_DEADLINE_US` (20 ms), unchanged.
    #[cfg(test)]
    inhibit_deadline_us: u64,
}
impl Supervisor {
    /// Spawn a dedicated executable, no shell/config search. Setup may fork/open
    /// process handles; call outside the conversation hot path. poll/send are
    /// nonblocking after spawn and must be serviced at the documented cadence.
    pub fn spawn(
        executable: &Path,
        config: &Path,
        new_directory: &Path,
        parent: BootId,
        worker: BootId,
    ) -> io::Result<Self> {
        Supervisor::spawn_with_clock(
            executable,
            config,
            new_directory,
            parent,
            worker,
            SystemClock,
        )
    }
    #[cfg(test)]
    fn spawn_command(
        command: Command,
        new_directory: &Path,
        parent: BootId,
        worker: BootId,
    ) -> io::Result<Self> {
        Supervisor::spawn_command_with_clock(command, new_directory, parent, worker, SystemClock)
    }
}
impl<C: Clock + Copy> Supervisor<C> {
    /// `spawn` with the clock every deadline is measured on.
    pub fn spawn_with_clock(
        executable: &Path,
        config: &Path,
        new_directory: &Path,
        parent: BootId,
        worker: BootId,
        clock: C,
    ) -> io::Result<Self> {
        bounded_path(executable)?;
        Config::read(config)?;
        let mut command = Command::new(executable);
        command
            .arg("--config")
            .arg(config)
            .arg("--directory")
            .arg(new_directory)
            .arg("--controller")
            .arg(boot_text(parent))
            .arg("--worker")
            .arg(boot_text(worker))
            .arg("--parent-pid")
            .arg(std::process::id().to_string());
        let mut supervisor =
            Self::spawn_command_with_clock(command, new_directory, parent, worker, clock)?;
        supervisor.expect_serial = true;
        Ok(supervisor)
    }
    fn spawn_command_with_clock(
        mut command: Command,
        new_directory: &Path,
        parent: BootId,
        worker: BootId,
        clock: C,
    ) -> io::Result<Self> {
        bounded_path(new_directory)?;
        fs::DirBuilder::new().mode(0o700).create(new_directory)?;
        let mut directory = Directory::new(new_directory)?;
        let channels = Channels::bind(new_directory, parent, worker, false)?;
        let started = clock.now().as_micros();
        let child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        directory.may_remove = false;
        Ok(Self {
            channels,
            child,
            directory,
            worker,
            parent,
            started,
            connected: false,
            ready: false,
            last_observed: started,
            stop_at: None,
            inhibited: false,
            inhibited_received: None,
            stopped: false,
            expect_serial: false,
            privacy_revision: None,
            sent_privacy: None,
            release_io_verified: false,
            release_discard_verified: false,
            lifecycle_fault: false,
            killed_at: None,
            exit: None,
            failure: None,
            clock,
            #[cfg(test)]
            inhibit_deadline_us: INHIBIT_DEADLINE_US,
        })
    }
    /// How long after a stop the child may take to acknowledge the inhibit:
    /// `INHIBIT_DEADLINE_US` in every build but a test that widened it.
    fn inhibit_deadline_us(&self) -> u64 {
        #[cfg(test)]
        {
            self.inhibit_deadline_us
        }
        #[cfg(not(test))]
        {
            INHIBIT_DEADLINE_US
        }
    }
    /// Explicit replacement only after the exact old child has been reaped.
    /// This creates an idle instance; no authority, qualification observation or
    /// pending command is copied from the predecessor.
    pub fn replace_after_reap(
        &mut self,
        executable: &Path,
        config: &Path,
        new_directory: &Path,
        parent: BootId,
        worker: BootId,
    ) -> io::Result<()> {
        self.replacement_allowed(new_directory, worker)?;
        let replacement = Self::spawn_with_clock(
            executable,
            config,
            new_directory,
            parent,
            worker,
            self.clock,
        )?;
        *self = replacement;
        Ok(())
    }
    fn replacement_allowed(&self, new_directory: &Path, worker: BootId) -> io::Result<()> {
        if !self.reaped()
            || !self.directory.peers_cleaned
            || worker == self.worker
            || new_directory == self.directory()
        {
            return Err(invalid(
                "replacement requires observed reap, peer cleanup and fresh worker/directory",
            ));
        }
        Ok(())
    }
    pub fn directory(&self) -> &Path {
        &self.directory.path
    }
    /// The instant on this Supervisor's clock.
    pub fn now(&self) -> lamp_turn_owner::MonoTime {
        self.clock.now()
    }
    pub fn is_ready(&self) -> bool {
        self.ready && self.failure.is_none() && self.stop_at.is_none() && self.exit.is_none()
    }
    pub fn connected(&self) -> bool {
        self.connected
    }
    pub fn reaped(&self) -> bool {
        self.exit.is_some()
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    /// Cleanup failure is retained separately from the original motor fault.
    /// A reap alone does not establish that old peer pathnames were released.
    pub fn ipc_cleanup_error(&self) -> Option<&str> {
        self.directory.cleanup_error.as_deref()
    }
    pub fn clean_exit(&self) -> bool {
        self.exit.is_some_and(|s| s.success())
            && self.directory.peers_cleaned
            && self.stopped
            && self.inhibited
            && self.failure.is_none()
            && self.killed_at.is_none()
    }
    /// Ownership/cleanup proof for an expected explicit privacy closure, kept
    /// separate from the failed operation's original reason and exit status.
    /// Never accepts timeout, forced kill, malformed IPC, discard/close failure,
    /// or a fresh caller revision substituted for the consumed original denial.
    pub fn privacy_release_confirmed(&self, revision: u64) -> bool {
        revision != 0
            && self.privacy_revision == Some(revision)
            && self.sent_privacy.is_some_and(|(sent, _)| sent == revision)
            && self.release_io_verified
            && self.release_discard_verified
            && !self.lifecycle_fault
            && self.exit.is_some_and(|status| status.code().is_some())
            && self.directory.peers_cleaned
            && self.directory.cleanup_error.is_none()
            && self.stopped
            && self.inhibited
            && self.killed_at.is_none()
    }
    pub fn send_control(&mut self, c: &Control) -> io::Result<()> {
        if matches!(c, Control::Stop {}) {
            return self.stop();
        }
        if self.exit.is_some() || self.killed_at.is_some() || self.stop_at.is_some() {
            return Err(invalid("motor worker is stopping"));
        }
        let n = self.clock.now();
        if let Err(e) = self.channels.control.send(c, n) {
            self.fail(
                &format!("motor priority delivery failed: {e}"),
                n.as_micros(),
            );
            return Err(e);
        }
        if let Control::Authority { snapshot } = c
            && snapshot.boot() == self.parent
            && snapshot.issued_at() <= n
            && n < snapshot.expires_at()
            && (snapshot.microphone_permission() == lamp_turn_owner::Permission::Denied
                || snapshot.camera_permission() == lamp_turn_owner::Permission::Denied)
        {
            self.sent_privacy
                .get_or_insert((snapshot.revision(), n.as_micros()));
        }
        Ok(())
    }
    pub fn send_command(&mut self, c: &MotorCommand) -> io::Result<()> {
        if !self.is_ready() {
            return Err(invalid("motor worker is not ready for commands"));
        }
        self.channels.data.send(c, self.clock.now())
    }
    /// One Stop send, no queue/retry. Congestion is a failure and signals only the
    /// owned child; an unproved receipt never becomes a successful cleanup.
    pub fn stop(&mut self) -> io::Result<()> {
        if self.stop_at.is_some() {
            return Ok(());
        }
        let n = self.clock.now();
        self.stop_at = Some(n.as_micros());
        if self.exit.is_some() {
            return Ok(());
        }
        if let Err(e) = self.channels.control.send(&Control::Stop {}, n) {
            self.fail(&e.to_string(), n.as_micros());
            return Err(e);
        }
        Ok(())
    }
    fn fail(&mut self, why: &str, n: u64) {
        self.lifecycle_fault = true;
        self.ready = false;
        self.failure
            .get_or_insert_with(|| super::wire::bounded(why));
        if self.exit.is_none() && self.killed_at.is_none() {
            self.killed_at = Some(n);
            if let Err(e) = self.child.kill() {
                self.failure = Some(super::wire::bounded(format!("{why}; owned kill: {e}")));
            }
        }
    }
    fn enforce(&mut self, n: u64) {
        if n < self.last_observed || n < self.started {
            self.fail("supervisor clock regressed", n);
            return;
        }
        if self.exit.is_some() {
            return;
        }
        if let Some(killed) = self.killed_at {
            if n.saturating_sub(killed) > REAP_US {
                self.failure.get_or_insert_with(|| {
                    "motor child reaping deadline expired; ownership unresolved".into()
                });
            }
            return;
        }
        if let Some(start) = self.stop_at {
            if !self.inhibited && n.saturating_sub(start) >= self.inhibit_deadline_us() {
                self.fail("motor inhibit acknowledgement timed out", n)
            } else if self
                .inhibited_received
                .is_some_and(|at| n.saturating_sub(at) >= TEARDOWN_US)
            {
                self.fail("motor driver teardown/reap timed out", n)
            }
        } else if !self.ready {
            if n.saturating_sub(self.started) >= STARTUP_US {
                self.fail("motor startup timed out", n)
            }
        } else if n.saturating_sub(self.last_observed) >= PROGRESS_DEADLINE_US {
            self.fail("motor worker progress timed out", n)
        }
    }
    fn event(&mut self, e: &Event, n: u64) -> io::Result<()> {
        let at = e.at_us();
        if at > n || at < self.last_observed {
            return Err(invalid("motor receipt time regressed/future"));
        }
        self.last_observed = at;
        match e {
            Event::Ready { .. } if !self.ready && !self.inhibited => self.ready = true,
            Event::Ready { .. } => return Err(invalid("duplicate/late motor Ready")),
            Event::Inhibited { motion, report, .. } => {
                if self.inhibited
                    || report.stopped_at_us < self.started
                    || report.stopped_at_us > at
                    || report.accepted_request_bytes > 23
                    || report.partial_request_bytes > 23
                    || report.privacy_revision == Some(0)
                    || (report.privacy_revision.is_some() && report.reason != "ControlLost")
                    || report.reason.len() > 192
                    || report.discard_error.as_ref().is_some_and(|s| s.len() > 192)
                {
                    return Err(invalid("invalid motor inhibition receipt"));
                }
                if let Some(m) = motion {
                    self.motion(*m)?;
                }
                if let Some(revision) = report.privacy_revision
                    && !self
                        .sent_privacy
                        .is_some_and(|(sent, at)| sent == revision && at <= report.stopped_at_us)
                {
                    return Err(invalid("privacy release has no matching sent denial"));
                }
                self.privacy_revision = report.privacy_revision;
                self.release_discard_verified = report.discard_error.is_none();
                if !matches!(report.reason.as_str(), "Shutdown" | "Cancelled") {
                    self.failure
                        .get_or_insert_with(|| format!("motor inhibited: {}", report.reason));
                }
                if report.discard_error.is_some() {
                    self.failure
                        .get_or_insert_with(|| "motor output discard failed".into());
                }
                self.inhibited = true;
                self.inhibited_received = Some(n);
                self.stop_at.get_or_insert(n);
            }
            Event::Stopped { cleanup, .. } => {
                if !self.inhibited || self.stopped {
                    return Err(invalid("motor close without unique inhibition"));
                }
                self.stopped = true;
                self.release_io_verified = cleanup.serial_closed == Some(true)
                    && cleanup.settings_restored == Some(true)
                    && cleanup.error.is_none();
                if self.expect_serial
                    && (cleanup.serial_closed != Some(true)
                        || cleanup.settings_restored != Some(true))
                {
                    self.failure.get_or_insert_with(|| {
                        "real motor worker omitted successful serial cleanup evidence".into()
                    });
                }
                if cleanup.error.is_some()
                    || cleanup.serial_closed == Some(false)
                    || cleanup.settings_restored == Some(false)
                {
                    self.failure
                        .get_or_insert_with(|| "motor serial cleanup failed".into());
                }
            }
            Event::Fault { reason, .. } => {
                if reason.len() > 192 {
                    return Err(invalid("oversized motor fault"));
                }
                self.failure.get_or_insert_with(|| reason.clone());
                self.stop_at.get_or_insert(n);
            }
            Event::ProfileStarted { motion, .. }
            | Event::ProfilePrepared { motion, .. }
            | Event::Started { motion, .. }
            | Event::HostAccepted { motion, .. }
            | Event::ReadbackReached { motion, .. } => {
                self.motion(*motion)?;
                if self.inhibited {
                    return Err(invalid("motor activity after inhibition"));
                }
            }
            Event::Progress { .. }
            | Event::Inspected { .. }
            | Event::Rejected { .. }
            | Event::CancelRejected { .. }
                if self.inhibited =>
            {
                return Err(invalid("motor activity after inhibition"));
            }
            _ => {}
        }
        Ok(())
    }
    fn motion(&self, m: super::wire::Motion) -> io::Result<()> {
        if m.worker != self.worker || m.owner.boot() != self.parent || m.sequence == 0 {
            return Err(invalid("wrong motor receipt lineage"));
        }
        Ok(())
    }
    /// At most one receipt per call. Enforcement and try_wait run even after a
    /// malformed receipt; authenticated garbage cannot postpone kill/reaping.
    pub fn poll(&mut self) -> io::Result<Option<Event>> {
        let n = self.clock.now().as_micros();
        self.enforce(n);
        let result: io::Result<Option<Event>> = (|| {
            if self.exit.is_none() {
                self.directory.observe_peers()?;
            }
            if !self.connected {
                self.connected = self.channels.connect_step()?;
            }
            if !self.connected || self.stopped {
                return Ok(None);
            }
            let clock = self.clock;
            let event = self
                .channels
                .control
                .receive_with::<Event>(|| Ok(clock.now()))?;
            if let Some(e) = &event {
                self.event(e, self.clock.now().as_micros())?;
            }
            Ok(event)
        })();
        if let Err(e) = &result {
            self.fail(&e.to_string(), self.clock.now().as_micros());
        }
        match self.child.try_wait() {
            Ok(Some(s)) => {
                self.exit = Some(s);
                if !s.success() {
                    self.failure
                        .get_or_insert_with(|| "motor child failed or was killed".into());
                }
            }
            Ok(None) => {}
            Err(e) => self.fail(&format!("motor waitpid: {e}"), self.clock.now().as_micros()),
        }
        if self.exit.is_some()
            && let Err(e) = self.directory.cleanup_reaped_peers()
        {
            self.fail(
                &format!("motor IPC cleanup failed: {e}"),
                self.clock.now().as_micros(),
            );
        }
        self.enforce(self.clock.now().as_micros());
        result
    }
}
impl<C: Clock> Drop for Supervisor<C> {
    fn drop(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
            if let Ok(Some(status)) = self.child.try_wait() {
                self.exit = Some(status);
            }
        }
        if self.exit.is_some() {
            let _ = self.directory.cleanup_reaped_peers();
        }
    }
}

#[cfg(test)]
mod tests;
