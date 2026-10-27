//! Bounded output for updater queries; stop and restart policy stays in world.
use std::{
    io::{self, Read},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

// Preserve the existing 300,000-byte pipe-drain contract with a bounded buffer.
const MAX_OUTPUT: usize = 1024 * 1024;
// The most of a query's stderr kept for its error text; the rest is read and
// dropped, so a query that writes much there is never blocked by its pipe.
const MAX_STDERR: usize = 64 * 1024;

struct OwnedChild {
    child: Child,
    reaped: bool,
}

impl OwnedChild {
    fn stop_group(&mut self) {
        if self.reaped {
            return;
        }
        // An unreaped leader retains the process-group identity. Never signal
        // that identity after reaping: its PID could then belong to another job.
        let group = rustix::process::Pid::from_child(&self.child);
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        let _ = self.child.kill();
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        self.stop_group();
        let _ = self.child.wait();
    }
}

fn time_left(until: Instant) -> io::Result<()> {
    if Instant::now() >= until {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "update query timed out",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn output_within(command: &mut Command, until: Instant) -> io::Result<Vec<u8>> {
    // These query responses do not expose stderr.
    // Discard it instead of retaining another output buffer.
    read_within(command.stderr(Stdio::null()), until, false)
}

/// As [`output_within`], with stderr piped too and drained while it runs,
/// its first 64 KiB kept. Both pipes must reach EOF by `until`. On a nonzero
/// exit the error (kind `Other`) is that stderr, lossy and trimmed, or the
/// exit status when it wrote nothing there.
pub(crate) fn output_and_stderr_within(
    command: &mut Command,
    until: Instant,
) -> io::Result<Vec<u8>> {
    read_within(command.stderr(Stdio::piped()), until, true)
}

fn nonblocking(pipe: &impl std::os::fd::AsFd) -> io::Result<()> {
    let flags = rustix::fs::fcntl_getfl(pipe)?;
    rustix::fs::fcntl_setfl(pipe, flags | rustix::fs::OFlags::NONBLOCK)?;
    Ok(())
}

/// Reads what is there now of `pipe`, keeping at most `MAX_STDERR` of it;
/// true at its EOF.
fn drain_stderr(pipe: &mut impl Read, kept: &mut Vec<u8>, until: Instant) -> io::Result<bool> {
    let mut buffer = [0; 4096];
    loop {
        time_left(until)?;
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(count) => {
                let room = MAX_STDERR.saturating_sub(kept.len());
                kept.extend_from_slice(&buffer[..count.min(room)]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn read_within(command: &mut Command, until: Instant, keep_stderr: bool) -> io::Result<Vec<u8>> {
    time_left(until)?;
    let mut owned = OwnedChild {
        child: command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()?,
        reaped: false,
    };
    let mut output = owned.child.stdout.take().expect("piped query stdout");
    nonblocking(&output)?;
    let mut errors = if keep_stderr {
        let pipe = owned.child.stderr.take().expect("piped query stderr");
        nonblocking(&pipe)?;
        Some(pipe)
    } else {
        None
    };
    let mut stderr = Vec::new();
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let eof = loop {
            time_left(until)?;
            match output.read(&mut buffer) {
                Ok(0) => break true,
                Ok(count) => {
                    if bytes.len() + count > MAX_OUTPUT {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "update query output exceeds 1 MiB",
                        ));
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break false,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        };
        // A descendant may hold the pipe after the leader exits. Retain the
        // unreaped leader until EOF so timeout cleanup still owns its group.
        let errors_eof = match &mut errors {
            Some(pipe) => drain_stderr(pipe, &mut stderr, until)?,
            None => true,
        };
        if eof && errors_eof {
            // Observe exit without consuming the leader. A failed leader may
            // leave descendants that closed stdout; its group must be stopped
            // before reaping releases the identity, just as on timeout.
            let exited = match rustix::process::waitid(
                rustix::process::WaitId::Pid(rustix::process::Pid::from_child(&owned.child)),
                rustix::process::WaitIdOptions::EXITED
                    | rustix::process::WaitIdOptions::NOHANG
                    | rustix::process::WaitIdOptions::NOWAIT,
            ) {
                Ok(exited) => exited,
                Err(rustix::io::Errno::INTR) => continue,
                Err(error @ rustix::io::Errno::CHILD) => {
                    // An external reaper or inherited SIGCHLD disposition
                    // removed our proof of ownership. Never signal that PID.
                    owned.reaped = true;
                    return Err(error.into());
                }
                Err(error) => return Err(error.into()),
            };
            if let Some(exited) = exited {
                // Invalid UTF-8 is a failed response too. Reject it while the
                // leader still reserves the group identity for error cleanup.
                if exited.exit_status() == Some(0) && std::str::from_utf8(&bytes).is_err() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "update query output is not UTF-8",
                    ));
                }
                if exited.exit_status() != Some(0) {
                    owned.stop_group();
                }
                let status = match owned.child.wait() {
                    Ok(status) => {
                        owned.reaped = true;
                        status
                    }
                    Err(error) => {
                        if error.raw_os_error() == Some(rustix::io::Errno::CHILD.raw_os_error()) {
                            owned.reaped = true;
                        }
                        return Err(error);
                    }
                };
                let said = String::from_utf8_lossy(&stderr);
                return if status.success() {
                    Ok(bytes)
                } else if !said.trim().is_empty() {
                    Err(io::Error::other(said.trim().to_owned()))
                } else {
                    Err(io::Error::other(format!(
                        "update query ended with {status}"
                    )))
                };
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
