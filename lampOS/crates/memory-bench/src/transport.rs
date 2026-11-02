//! Deadline-bound child pipes. No blocking write, reader thread, or inherited key.
use crate::{Result, invalid, protocol::MAX_FRAME_BYTES};
use nix::{
    errno::Errno,
    fcntl::{FcntlArg, OFlag, fcntl},
    poll::{PollFd, PollFlags, PollTimeout, poll},
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    io::{ErrorKind, Read, Write},
    os::fd::{AsFd, AsRawFd, BorrowedFd},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::Instant,
};

pub struct Process {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    buffered: Vec<u8>,
    reaped: bool,
}
fn nonblocking(fd: &impl AsRawFd) -> Result<()> {
    let flags = fcntl(fd.as_raw_fd(), FcntlArg::F_GETFL).map_err(std::io::Error::from)?;
    fcntl(
        fd.as_raw_fd(),
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(std::io::Error::from)?;
    Ok(())
}
fn ready(fd: BorrowedFd<'_>, events: PollFlags, deadline: Instant) -> Result<bool> {
    loop {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(false);
        };
        // Round upward: poll's millisecond conversion otherwise spins in the
        // final sub-millisecond. Recheck the absolute deadline after readiness.
        let millis = left.as_millis().saturating_add(1).min(i32::MAX as u128);
        let timeout = PollTimeout::try_from(millis as u64)
            .map_err(|_| invalid("invalid transport timeout"))?;
        let mut fds = [PollFd::new(fd, events)];
        match poll(&mut fds, timeout) {
            Ok(0) => return Ok(false),
            Ok(_) => return Ok(Instant::now() < deadline),
            Err(Errno::EINTR) => continue,
            Err(error) => return Err(std::io::Error::from(error).into()),
        }
    }
}
impl Process {
    /// Programs are reviewed adapters, not an OS sandbox. Use an absolute
    /// executable and a fresh private directory. No caller environment (keys,
    /// fixture paths, or provider settings) reaches the adapter.
    pub fn spawn(executable: &Path, args: &[String], state_dir: &Path) -> Result<Self> {
        if !executable.is_absolute() || !state_dir.is_absolute() {
            return Err(invalid("adapter and state paths must be absolute"));
        }
        let metadata = std::fs::symlink_metadata(state_dir)?;
        if !metadata.is_dir()
            || metadata.permissions().mode() & 0o077 != 0
            || std::fs::read_dir(state_dir)?.next().is_some()
        {
            return Err(invalid(
                "adapter state directory must be private, fresh, and not a symlink",
            ));
        }
        let mut child = Command::new(executable)
            .args(args)
            .current_dir(state_dir)
            .env_clear()
            .env("LANG", "C.UTF-8")
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Diagnostics cannot fill a pipe or grow an unbounded log. The
            // protocol error and terminal status remain in the run ledger.
            .stderr(Stdio::null())
            .spawn()?;
        let setup = (|| {
            let input = child.stdin.take().ok_or_else(|| invalid("missing stdin"))?;
            let output = child
                .stdout
                .take()
                .ok_or_else(|| invalid("missing stdout"))?;
            nonblocking(&input)?;
            nonblocking(&output)?;
            Ok((input, output))
        })();
        match setup {
            Ok((input, output)) => Ok(Self {
                child,
                input,
                output,
                buffered: Vec::new(),
                reaped: false,
            }),
            Err(error) => {
                stop_child(&mut child);
                Err(error)
            }
        }
    }
    /// A partial write on expiry is fatal: a peer cannot reuse that frame.
    pub fn send(&mut self, value: &impl Serialize, deadline: Instant) -> Result<()> {
        let mut bytes = serde_json::to_vec(value)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(invalid("outgoing adapter frame exceeds bound"));
        }
        let mut offset = 0;
        while offset < bytes.len() {
            if !ready(self.input.as_fd(), PollFlags::POLLOUT, deadline)? {
                return Err(invalid("adapter write deadline expired"));
            }
            match self.input.write(&bytes[offset..]) {
                Ok(0) => return Err(invalid("adapter stdin closed")),
                Ok(n) => offset += n,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
    /// None is a deadline miss, never EOF. The bounded partial frame is kept
    /// for draining before a later request can be sent.
    pub fn receive(&mut self, deadline: Instant) -> Result<Option<Value>> {
        loop {
            if Instant::now() >= deadline {
                return Ok(None);
            }
            if let Some(end) = self.buffered.iter().position(|b| *b == b'\n') {
                let frame: Vec<_> = self.buffered.drain(..=end).collect();
                let value: Value = serde_json::from_slice(&frame)?;
                if !value.is_object() {
                    return Err(invalid("adapter frame must be an object"));
                }
                return Ok(Some(value));
            }
            if self.buffered.len() >= MAX_FRAME_BYTES {
                return Err(invalid("adapter frame exceeds bound"));
            }
            if !ready(self.output.as_fd(), PollFlags::POLLIN, deadline)? {
                return Ok(None);
            }
            let mut chunk = [0; 8192];
            let capacity = chunk.len().min(MAX_FRAME_BYTES - self.buffered.len());
            match self.output.read(&mut chunk[..capacity]) {
                Ok(0) => return Err(invalid("adapter exited or truncated a response")),
                Ok(n) => self.buffered.extend_from_slice(&chunk[..n]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    /// After close, require EOF and a successful exit. Extra terminal frames
    /// are an error even when there is no next request to reveal the duplicate.
    pub fn finish(&mut self, deadline: Instant) -> Result<()> {
        if !self.buffered.is_empty() {
            return Err(invalid("unexpected output after close"));
        }
        loop {
            if !ready(self.output.as_fd(), PollFlags::POLLIN, deadline)? {
                return Err(invalid("adapter did not close stdout before deadline"));
            }
            let mut byte = [0];
            match self.output.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => return Err(invalid("unexpected output after close")),
                Err(error) if error.kind() == ErrorKind::WouldBlock => continue,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        // EOF need not mean process exit. Bound the wait without a blocking
        // waitpid; each pause is short and belongs to this process watchdog.
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.reaped = true;
                return if status.success() {
                    Ok(())
                } else {
                    Err(invalid("adapter exited unsuccessfully"))
                };
            }
            if Instant::now() >= deadline {
                return Err(invalid("adapter exit deadline expired"));
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if !self.reaped {
            // This newly created process group belongs only to our adapter
            // and its helpers. Its leader is unreaped, so the id cannot be
            // recycled into another task's process group during cleanup.
            stop_child(&mut self.child);
        }
    }
}
fn stop_child(child: &mut Child) {
    let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
    let _ = child.kill();
    let deadline = Instant::now() + std::time::Duration::from_secs(1);
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
