//! Explicit local fixture transport. Receipts do not prove log delivery or exit.
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    time::{Duration, Instant},
};

pub const VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 4096;
/// A watchdog, not a latency bound: a `--memory-days` child opens SQLite with
/// full flushes (F_FULLFSYNC on macOS), which wait behind other processes'
/// file work. Beside eight processes creating, flushing, renaming and removing
/// files, a Mac took 8.8–13.6 s to be ready (#1569).
pub const HOST_WAIT: Duration = Duration::from_secs(30);
pub const MAX_MARKER_BYTES: usize = 128;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bootstrap {
    pub version: u32,
    pub control: PathBuf,
    pub nonce: String,
}

impl Bootstrap {
    pub fn validate(&self) -> io::Result<()> {
        if self.version != VERSION || !valid_nonce(&self.nonce) || !self.control.is_absolute() {
            return Err(invalid("invalid memory fixture bootstrap"));
        }
        let parent = self
            .control
            .parent()
            .ok_or_else(|| invalid("missing control directory"))?;
        let directory = std::fs::symlink_metadata(parent)?;
        let socket = std::fs::symlink_metadata(&self.control)?;
        use std::os::unix::fs::FileTypeExt;
        if !directory.is_dir()
            || directory.permissions().mode() & 0o777 != 0o700
            || !socket.file_type().is_socket()
            || socket.permissions().mode() & 0o777 != 0o600
        {
            return Err(invalid(
                "fixture control requires a private directory and socket",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Connected {
    pub version: u32,
    pub nonce: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub nonce: String,
    pub round: u64,
    pub at_us: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    NotAttempted,
    ReturnedOk,
    ReturnedErr,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Checked {
    pub nonce: String,
    pub round: u64,
    pub at_us: u64,
    pub cycles: u64,
    pub attempts: u64,
    pub last_outcome: Outcome,
    pub cycle_started_us: u64,
    pub next_due_us: u64,
    /// Exact line, including its LF, written to actual stderr before this receipt.
    pub marker: String,
}

pub fn valid_nonce(nonce: &str) -> bool {
    !nonce.is_empty() && nonce.len() <= 32 && nonce.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

pub fn marker(nonce: &str, round: u64) -> io::Result<String> {
    if !valid_nonce(nonce) || round == 0 {
        return Err(invalid("invalid memory fixture marker identity"));
    }
    let marker = format!("memory-fixture barrier {nonce} {round}\n");
    if marker.len() > MAX_MARKER_BYTES {
        return Err(invalid("memory fixture marker exceeds bound"));
    }
    Ok(marker)
}

pub fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "memory fixture host deadline"))
}

/// Byte bounded, but not a wall-clock guard. Bootstrap owns an outer watchdog.
pub fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> io::Result<T> {
    let mut frame = Vec::with_capacity(256);
    loop {
        let mut byte = [0];
        reader.read_exact(&mut byte)?;
        frame.push(byte[0]);
        if frame.len() > MAX_FRAME_BYTES {
            return Err(invalid("memory fixture frame exceeds bound"));
        }
        if byte[0] == b'\n' {
            return serde_json::from_slice(&frame[..frame.len() - 1]).map_err(io::Error::from);
        }
    }
}

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let mut frame = serde_json::to_vec(value)?;
    frame.push(b'\n');
    if frame.len() > MAX_FRAME_BYTES {
        return Err(invalid("memory fixture frame exceeds bound"));
    }
    writer.write_all(&frame)?;
    writer.flush()
}

pub struct Channel {
    stream: UnixStream,
}

impl Channel {
    pub fn new(stream: UnixStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self { stream })
    }

    pub fn try_clone_stream(&self) -> io::Result<UnixStream> {
        self.stream.try_clone()
    }

    pub fn read_until<T: DeserializeOwned>(&mut self, deadline: Instant) -> io::Result<T> {
        read_frame(&mut DeadlineStream {
            stream: &mut self.stream,
            deadline,
        })
    }

    pub fn write_until<T: Serialize>(&mut self, value: &T, deadline: Instant) -> io::Result<()> {
        write_frame(
            &mut DeadlineStream {
                stream: &mut self.stream,
                deadline,
            },
            value,
        )
    }

    pub fn shutdown(&self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct DeadlineStream<'a> {
    stream: &'a mut UnixStream,
    deadline: Instant,
}

impl Read for DeadlineStream<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        loop {
            remaining(self.deadline)?;
            match self.stream.read(bytes) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_ready(self.stream, PollFlags::IN, self.deadline)?;
                }
                result => return result,
            }
        }
    }
}

impl Write for DeadlineStream<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        loop {
            remaining(self.deadline)?;
            match self.stream.write(bytes) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    wait_ready(self.stream, PollFlags::OUT, self.deadline)?;
                }
                result => return result,
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        remaining(self.deadline)?;
        self.stream.flush()
    }
}

fn wait_ready(stream: &UnixStream, flags: PollFlags, deadline: Instant) -> io::Result<()> {
    loop {
        let left = remaining(deadline)?.min(Duration::from_millis(i32::MAX as u64));
        let timeout =
            Timespec::try_from(left).map_err(|_| invalid("fixture poll timeout overflow"))?;
        let mut descriptors = [PollFd::new(stream, flags)];
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => continue,
            Ok(_) => {
                let events = descriptors[0].revents();
                if events.contains(PollFlags::NVAL) {
                    return Err(invalid("fixture poll found an invalid descriptor"));
                }
                // HUP is not EOF. The next real read first consumes queued data.
                if events.intersects(flags | PollFlags::HUP | PollFlags::ERR) {
                    return Ok(());
                }
            }
            Err(error) if error == rustix::io::Errno::INTR => continue,
            Err(error) => return Err(error.into()),
        }
    }
}
