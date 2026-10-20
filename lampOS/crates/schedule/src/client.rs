//! One bounded local JSON exchange. The caller supplies one absolute deadline
//! for the whole operation, including any preceding status/get hops. No request
//! is retried here. A failed exchange after any bytes were written is ambiguous
//! for mutations; it must never be retried using a fresh compare-and-set guard.
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde_json::Value;
use std::{
    fmt,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::{Component, Path},
    time::Instant,
};

/// JSON bytes, excluding the single newline framing byte.
pub const MAX_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    InvalidSocket,
    InvalidRequest,
    Unavailable,
    Timeout,
    Io,
    TooLong,
    InvalidResponse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    /// At least part of the request reached the socket. This is not evidence
    /// that the owner accepted or committed it; absence of a reply is unknown.
    pub may_have_sent: bool,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self.kind {
            ErrorKind::InvalidSocket => "invalid_socket",
            ErrorKind::InvalidRequest => "invalid_request",
            ErrorKind::Unavailable => "unavailable",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Io => "io",
            ErrorKind::TooLong => "too_long",
            ErrorKind::InvalidResponse => "invalid_response",
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.code())
    }
}
impl std::error::Error for Error {}

fn error(kind: ErrorKind, sent: bool) -> Error {
    Error {
        kind,
        may_have_sent: sent,
    }
}

/// Validate syntax without touching the socket or filesystem.
pub fn validate_socket(path: &Path) -> Result<(), Error> {
    if !path.is_absolute()
        || path.as_os_str().as_encoded_bytes().len() > 103
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || rustix::net::SocketAddrUnix::new(path).is_err()
    {
        return Err(error(ErrorKind::InvalidSocket, false));
    }
    Ok(())
}

fn remaining(deadline: Instant, sent: bool) -> Result<std::time::Duration, Error> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| error(ErrorKind::Timeout, sent))
}

fn ready(
    stream: &UnixStream,
    interest: PollFlags,
    deadline: Instant,
    sent: bool,
) -> Result<(), Error> {
    loop {
        let timeout = Timespec::try_from(remaining(deadline, sent)?)
            .map_err(|_| error(ErrorKind::Timeout, sent))?;
        let mut descriptors = [PollFd::new(stream, interest)];
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => continue,
            Ok(_) => {
                remaining(deadline, sent)?;
                let events = descriptors[0].revents();
                if events.contains(PollFlags::NVAL) {
                    return Err(error(ErrorKind::Io, sent));
                }
                if events.intersects(interest | PollFlags::HUP | PollFlags::ERR) {
                    return Ok(());
                }
            }
            Err(e) if e == rustix::io::Errno::INTR => {}
            Err(_) => return Err(error(ErrorKind::Io, sent)),
        }
    }
}

struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("bounded JSON request"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Connect under one absolute deadline, returning a nonblocking stream.
/// Unix backlog EAGAIN is refused as unavailable, not mistaken for a pending
/// connection or silently retried. No request bytes are sent. Filesystem
/// ownership remains the caller's responsibility.
pub fn connect(socket: &Path, deadline: Instant) -> Result<UnixStream, Error> {
    remaining(deadline, false)?;
    validate_socket(socket)?;
    let descriptor = rustix::net::socket(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        None,
    )
    .map_err(|_| error(ErrorKind::Unavailable, false))?;
    rustix::io::fcntl_setfd(&descriptor, rustix::io::FdFlags::CLOEXEC)
        .map_err(|_| error(ErrorKind::Io, false))?;
    let stream = UnixStream::from(descriptor);
    stream
        .set_nonblocking(true)
        .map_err(|_| error(ErrorKind::Io, false))?;
    let address = rustix::net::SocketAddrUnix::new(socket)
        .map_err(|_| error(ErrorKind::InvalidSocket, false))?;
    match rustix::net::connect(&stream, &address) {
        Ok(()) => {}
        Err(e) if e == rustix::io::Errno::INPROGRESS => {
            ready(&stream, PollFlags::OUT, deadline, false)?;
            if stream
                .take_error()
                .map_err(|_| error(ErrorKind::Io, false))?
                .is_some()
                || stream.peer_addr().is_err()
            {
                return Err(error(ErrorKind::Unavailable, false));
            }
        }
        Err(_) => return Err(error(ErrorKind::Unavailable, false)),
    }
    remaining(deadline, false)?;
    Ok(stream)
}

/// Connect, send one JSON object and receive one JSON object under `deadline`.
/// All socket operations are nonblocking. Unix backlog EAGAIN is refused as
/// unavailable, not mistaken for a pending connection or silently retried.
/// Filesystem ownership and service-specific reply validation remain the caller's
/// responsibility. Errors contain neither request text nor private socket paths.
pub fn exchange(socket: &Path, request: &Value, deadline: Instant) -> Result<Value, Error> {
    exchange_guarded(socket, request, deadline, || Some(()))
}

/// Like `exchange`, with authority checked immediately before every write.
/// `acquire` must return promptly. Its guard is held only across a nonblocking
/// write, never while connecting, polling or reading. Returning `None` refuses
/// further bytes as unavailable; an earlier write can still be in flight.
pub fn exchange_guarded<G>(
    socket: &Path,
    request: &Value,
    deadline: Instant,
    acquire: impl FnMut() -> Option<G>,
) -> Result<Value, Error> {
    remaining(deadline, false)?;
    validate_socket(socket)?;
    if !request.is_object() {
        return Err(error(ErrorKind::InvalidRequest, false));
    }
    let mut encoded = Bounded(Vec::new());
    serde_json::to_writer(&mut encoded, request).map_err(|_| error(ErrorKind::TooLong, false))?;
    encoded.0.push(b'\n');
    let mut stream = connect(socket, deadline)?;
    write_request(&mut stream, &encoded.0, deadline, acquire)?;
    read_response(&mut stream, deadline)
}

fn write_request<G>(
    stream: &mut UnixStream,
    encoded: &[u8],
    deadline: Instant,
    mut acquire: impl FnMut() -> Option<G>,
) -> Result<(), Error> {
    let mut written = 0;
    while written < encoded.len() {
        remaining(deadline, written != 0)?;
        let result = {
            let _guard = acquire().ok_or_else(|| error(ErrorKind::Unavailable, written != 0))?;
            remaining(deadline, written != 0)?;
            stream.write(&encoded[written..])
        };
        match result {
            Ok(0) => return Err(error(ErrorKind::Io, written != 0)),
            Ok(count) => written += count,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                ready(stream, PollFlags::OUT, deadline, written != 0)?;
            }
            Err(_) => return Err(error(ErrorKind::Io, written != 0)),
        }
    }
    Ok(())
}

fn read_response(stream: &mut UnixStream, deadline: Instant) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        remaining(deadline, true)?;
        match stream.read(&mut buffer) {
            Ok(0) => return Err(error(ErrorKind::InvalidResponse, true)),
            Ok(count) => {
                let end = buffer[..count].iter().position(|b| *b == b'\n');
                let part = &buffer[..end.unwrap_or(count)];
                if part.len() > MAX_BYTES.saturating_sub(bytes.len()) {
                    return Err(error(ErrorKind::TooLong, true));
                }
                bytes.extend_from_slice(part);
                if let Some(end) = end {
                    if buffer[end + 1..count]
                        .iter()
                        .any(|b| !b.is_ascii_whitespace())
                    {
                        return Err(error(ErrorKind::InvalidResponse, true));
                    }
                    let value: Value = serde_json::from_slice(&bytes)
                        .map_err(|_| error(ErrorKind::InvalidResponse, true))?;
                    remaining(deadline, true)?;
                    return if value.is_object() {
                        Ok(value)
                    } else {
                        Err(error(ErrorKind::InvalidResponse, true))
                    };
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                ready(stream, PollFlags::IN, deadline, true)?;
            }
            Err(_) => return Err(error(ErrorKind::Io, true)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Arc, Mutex, MutexGuard, mpsc},
        time::Duration,
    };

    struct Written<'a> {
        guard: Option<MutexGuard<'a, bool>>,
        finished: mpsc::Sender<()>,
    }
    impl Drop for Written<'_> {
        fn drop(&mut self) {
            drop(self.guard.take());
            self.finished.send(()).unwrap();
        }
    }

    #[test]
    fn a_blocked_write_releases_its_guard_and_rechecks_after_readiness() {
        let (mut stream, mut peer) = UnixStream::pair().unwrap();
        stream.set_nonblocking(true).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        // Fill the socket without relying on a platform's buffer size.
        // Finish one byte at a time so even the small request must wait.
        let mut filled = 0;
        for block in [&[0u8; 4096][..], &[0u8][..]] {
            loop {
                match stream.write(block) {
                    Ok(0) => panic!("fixture socket closed"),
                    Ok(count) => {
                        filled += count;
                        assert!(filled < 16 * 1024 * 1024, "fixture did not fill");
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("fixture fill: {error}"),
                }
            }
        }
        let allowed = Arc::new(Mutex::new(true));
        let held = Arc::clone(&allowed);
        let (send, receive) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            write_request(
                &mut stream,
                b"{\"op\":\"set\"}\n",
                Instant::now() + Duration::from_secs(3),
                || {
                    let guard = held.lock().unwrap();
                    if !*guard {
                        return None;
                    }
                    Some(Written {
                        guard: Some(guard),
                        finished: send.clone(),
                    })
                },
            )
        });
        receive.recv_timeout(Duration::from_secs(1)).unwrap();
        *allowed
            .try_lock()
            .expect("guard held during readiness wait") = false;
        let mut padding = vec![0; filled];
        peer.read_exact(&mut padding).unwrap();
        let error = worker.join().unwrap().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unavailable);
        assert!(!error.may_have_sent);
        assert_eq!(
            peer.read(&mut [0]).unwrap(),
            0,
            "revoked request reached peer"
        );
    }
}
