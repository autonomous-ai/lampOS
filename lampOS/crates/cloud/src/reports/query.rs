//! Bounded complete responses for Cloud report queries.
use std::{
    io::{self, Read},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

// Keep Cloud's existing 256 KiB response limit, refusing oversized replies.
const MAX_OUTPUT: usize = 256 * 1024;

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
            "cloud query timed out",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn output_within(command: &mut Command, until: Instant) -> io::Result<Vec<u8>> {
    time_left(until)?;
    let mut owned = OwnedChild {
        child: command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            // These query responses do not expose stderr.
            // Discard it instead of retaining another output buffer.
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?,
        reaped: false,
    };
    let mut output = owned.child.stdout.take().expect("piped query stdout");
    let flags = rustix::fs::fcntl_getfl(&output)?;
    rustix::fs::fcntl_setfl(&output, flags | rustix::fs::OFlags::NONBLOCK)?;
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
                            "cloud query output exceeds 256 KiB",
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
        if eof {
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
                        "cloud query output is not UTF-8",
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
                return if status.success() {
                    Ok(bytes)
                } else {
                    Err(io::Error::other(format!("cloud query ended with {status}")))
                };
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
