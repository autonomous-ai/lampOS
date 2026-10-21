//! The memory's socket: where the voice, and whoever else on the lamp is let
//! in, asks for something to be remembered or recalled.
//!
//! One Unix stream socket, readable and writable by its owner only. A
//! client connects, writes a request on a line, reads the answer on a line,
//! and writes another or closes. One connection is served at a time, and one
//! request at a time on it: everything the store does is in order.
//!
//! Since the one being served is the only one being served, nobody may hold
//! the socket. A whole request line has to arrive within two seconds of the
//! connection opening or of the last answer, and an answer has to be taken
//! within two seconds; a client that does neither is dropped and the next
//! one served. So a client that keeps a connection open and silent is kept
//! for two seconds at most, and makes whoever comes next wait that long: a
//! client should close when it has its answer.
//!
//! A second socket, the app's narrow one (`--forget-socket`, `docs/memory.md`
//! "Forgetting a person"), can be opened beside it for one group: mode 0660,
//! and it takes `forget_person` only ([`protocol::narrow`]). Both are
//! served in turn by the same loop, so the store still does one thing at a
//! time. A `forget_person` from either is told on `memory/forgot` as a
//! `forget_all` is.
//!
//! A file `ready` beside the socket says the process has its store open and
//! is taking requests. Both are removed when the server ends in good order.
//! A process that is killed leaves both; the next one clears them as it
//! starts.
use crate::{
    forgot::ForgotOut,
    protocol::{self, MOST_LINE_BYTES},
    store::Store,
};
use rustix::event::{PollFd, PollFlags};
use std::{
    fs::{self, DirBuilder, OpenOptions, Permissions},
    io::{self, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, FileTypeExt, OpenOptionsExt, PermissionsExt},
        net::{SocketAddr, UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// How long a client is waited for: to send a whole request, and to take
/// its answer.
pub const PATIENCE: Duration = Duration::from_secs(2);

/// A single owner answers requests; expensive model work never enters here.
pub trait Memory {
    fn answer(&mut self, line: &[u8], now_unix_ms: u64) -> protocol::Answer;

    /// Only called with reporting enabled. Legacy files are counted by the
    /// reporter; Days reads its connection-local aggregate, without a scan.
    fn health_counts(&self) -> Option<io::Result<crate::health_counts::Counts>> {
        None
    }

    /// A database a newer build wrote, set aside and still on disk (#1680).
    fn set_aside(&self) -> Option<&str> {
        None
    }
}

impl Memory for Store {
    fn answer(&mut self, line: &[u8], now_unix_ms: u64) -> protocol::Answer {
        protocol::answer(self, line, now_unix_ms)
    }
}

impl Memory for crate::days::Days {
    fn answer(&mut self, line: &[u8], now_unix_ms: u64) -> protocol::Answer {
        crate::days_protocol::answer(self, line, now_unix_ms)
    }

    fn health_counts(&self) -> Option<io::Result<crate::health_counts::Counts>> {
        Some(self.health_counts().map_err(io::Error::other))
    }

    fn set_aside(&self) -> Option<&str> {
        crate::days::Days::set_aside(self)
    }
}

pub struct Server {
    listener: UnixListener,
    socket: PathBuf,
    ready: PathBuf,
    patience: Duration,
    stop: Arc<AtomicBool>,
    /// Where a forget is told (`memory/forgot`), with `--bus-root`.
    forgot: Option<Mutex<ForgotOut>>,
    /// The app's narrow socket, and its path.
    forget: Option<(UnixListener, PathBuf)>,
}

/// Ends a running server from another thread.
#[derive(Clone)]
pub struct Stopper {
    stop: Arc<AtomicBool>,
    socket: PathBuf,
}

impl Stopper {
    /// The server finishes the connection it is serving, if any, and
    /// returns.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        // It is waiting for someone to connect. Someone does.
        let _ = UnixStream::connect(&self.socket);
    }
}

fn gone(removed: io::Result<()>) -> io::Result<()> {
    match removed {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// The directory a socket path is in.
fn directory_of(socket: &Path) -> io::Result<&Path> {
    socket
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the socket path has no directory",
            )
        })
}

/// Listen at `socket` with `mode`, and with `group` as its group when
/// given. `cleared` runs once the path is known to be free, before the
/// socket is made.
fn listen(
    socket: &Path,
    tag: &str,
    mode: u32,
    group: Option<u32>,
    cleared: impl FnOnce() -> io::Result<()>,
) -> io::Result<UnixListener> {
    let directory = directory_of(socket)?;
    // The socket is made under another name and moved here, so nothing
    // below would notice a path too long for a client to connect to.
    SocketAddr::from_pathname(socket)?;
    match fs::symlink_metadata(socket) {
        Ok(found) if found.file_type().is_socket() => {
            if UnixStream::connect(socket).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("another process is answering on {}", socket.display()),
                ));
            }
            fs::remove_file(socket)?;
        }
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} is there and is not a socket", socket.display()),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    cleared()?;
    // A socket file is made with whatever mode the process's mask
    // leaves, and anyone that mode lets in could connect before it is
    // changed. So the socket is made where nobody else can reach, given
    // its mode and group, and only then moved to its path: it is never
    // there with any other mode.
    let private = directory.join(format!(".{tag}{}", std::process::id()));
    let bound = private.join("s");
    let _ = fs::remove_file(&bound);
    let _ = fs::remove_dir(&private);
    DirBuilder::new().mode(0o700).create(&private)?;
    let listener = UnixListener::bind(&bound).and_then(|listener| {
        if let Some(group) = group {
            rustix::fs::chown(&bound, None, Some(rustix::fs::Gid::from_raw(group)))?;
        }
        fs::set_permissions(&bound, Permissions::from_mode(mode))?;
        fs::rename(&bound, socket)?;
        Ok(listener)
    });
    let _ = fs::remove_file(&bound);
    let _ = fs::remove_dir(&private);
    listener
}

/// The gid of the group called `name` in `/etc/group`.
pub fn group_named(name: &str) -> io::Result<u32> {
    group_in(&fs::read_to_string("/etc/group")?, name).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("there is no group {name} in /etc/group"),
        )
    })
}

/// The gid of `name` in the text of a group file: `name:password:gid:members`.
fn group_in(file: &str, name: &str) -> Option<u32> {
    file.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next() == Some(name))
            .then(|| fields.nth(1)?.trim().parse().ok())
            .flatten()
    })
}

impl Server {
    /// Take the socket path. A socket file nobody answers on, left by a
    /// process that was killed, is replaced; one that is answered, or
    /// anything at the path that is not a socket, is a reason not to start.
    pub fn bind(socket: &Path) -> io::Result<Self> {
        let directory = directory_of(socket)?;
        let ready = directory.join("ready");
        let listener = listen(socket, "m", 0o600, None, || {
            gone(fs::remove_file(directory.join("ready")))
        })?;
        Ok(Self {
            listener,
            socket: socket.into(),
            ready,
            patience: PATIENCE,
            stop: Arc::new(AtomicBool::new(false)),
            forgot: None,
            forget: None,
        })
    }

    /// Tell each forget of everything kept under a person on the bus
    /// (`memory/forgot`), once its asker has its answer.
    pub fn telling_forgets(mut self, out: ForgotOut) -> Self {
        self.forgot = Some(Mutex::new(out));
        self
    }

    /// Open the app's narrow socket too, mode 0660 and owned by the group
    /// `group` (a gid): it takes `forget_person` only. The same care as the
    /// main socket: a stale one is replaced, and it is never at its path
    /// with any other mode.
    pub fn forget_socket(mut self, socket: &Path, group: u32) -> io::Result<Self> {
        directory_of(socket)?;
        if socket == self.socket {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the forget socket is not the memory socket",
            ));
        }
        let listener = listen(socket, "f", 0o660, Some(group), || Ok(()))?;
        // Polled together: neither may block the loop on a client that left.
        listener.set_nonblocking(true)?;
        self.listener.set_nonblocking(true)?;
        self.forget = Some((listener, socket.into()));
        Ok(self)
    }

    /// Wait this long for a client and no longer.
    pub fn patience(mut self, patience: Duration) -> Self {
        self.patience = patience;
        self
    }

    pub fn stopper(&self) -> Stopper {
        Stopper {
            stop: Arc::clone(&self.stop),
            socket: self.socket.clone(),
        }
    }

    /// Serve until stopped or until the socket fails. `clock` is asked the
    /// time, in milliseconds since 1970, for each request; `log` is given
    /// one line for each.
    pub fn run(
        self,
        store: &mut (impl Memory + ?Sized),
        clock: &mut dyn FnMut() -> u64,
        log: &mut dyn FnMut(&str),
    ) -> io::Result<()> {
        self.run_reporting(store, clock, log, None)
    }

    pub fn run_reporting(
        self,
        store: &mut (impl Memory + ?Sized),
        clock: &mut dyn FnMut() -> u64,
        log: &mut dyn FnMut(&str),
        health: Option<&crate::health::State>,
    ) -> io::Result<()> {
        // Written whole beside its place and moved there, so nobody reads
        // half of it.
        let writing = self.ready.with_extension("new");
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&writing)
            .and_then(|mut file| file.write_all(b"ready\n"))
            .and_then(|()| fs::rename(&writing, &self.ready))
            .inspect_err(|_| {
                let _ = fs::remove_file(&writing);
            })?;
        if let Some(health) = health {
            health.serving();
        }
        loop {
            let (accepted, narrow) = self.accept()?;
            if self.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            match accepted {
                Ok((stream, _)) => self.serve(&stream, narrow, store, clock, log, health),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => return Err(error),
            }
        }
    }

    /// The next client, and whether it came on the narrow socket. With one
    /// socket, a plain blocking accept; with two, whichever is ready first.
    fn accept(&self) -> io::Result<(io::Result<(UnixStream, SocketAddr)>, bool)> {
        let Some((forget, _)) = &self.forget else {
            return Ok((self.listener.accept(), false));
        };
        let mut fds = [
            PollFd::new(&self.listener, PollFlags::IN),
            PollFd::new(forget, PollFlags::IN),
        ];
        match rustix::event::poll(&mut fds, None) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => {
                return Ok((Err(io::ErrorKind::Interrupted.into()), false));
            }
            Err(error) => return Err(error.into()),
        }
        // The main socket first: the stopper knocks there.
        let (listener, narrow) = if !fds[0].revents().is_empty() {
            (&self.listener, false)
        } else {
            (forget, true)
        };
        // Both listeners are non-blocking here, so a client gone between
        // the poll and the accept never holds the loop. A stream accepted
        // from one is set back to blocking, as one from `accept` always is
        // on Linux and is not on every system.
        let accepted = listener.accept().and_then(|(stream, address)| {
            stream.set_nonblocking(false)?;
            Ok((stream, address))
        });
        Ok((accepted, narrow))
    }

    /// Answer this client's requests until it closes, goes quiet or stops
    /// reading.
    fn serve(
        &self,
        stream: &UnixStream,
        narrow: bool,
        store: &mut (impl Memory + ?Sized),
        clock: &mut dyn FnMut() -> u64,
        log: &mut dyn FnMut(&str),
        health: Option<&crate::health::State>,
    ) {
        let mut held = Vec::new();
        loop {
            let answer = match self.line(stream, &mut held) {
                // The narrow socket's line is checked and re-encoded before
                // the store sees it; a refusal never reaches the store.
                Line::Whole(line) if narrow => match protocol::narrow(&line) {
                    Ok(line) => self.answer(&line, store, clock, health),
                    Err(refusal) => refusal,
                },
                Line::Whole(line) => self.answer(&line, store, clock, health),
                Line::TooLong => protocol::too_long(),
                Line::Late { unfinished: true } => {
                    log("a client did not finish its request in time and was dropped");
                    return;
                }
                Line::Late { unfinished: false } | Line::Closed => return,
            };
            log(&answer.log);
            let forgot = answer.forgot;
            let mut line = answer.line.into_bytes();
            line.push(b'\n');
            let sent = self.send(stream, &line);
            // Done either way, so told either way: after the answer, so the
            // asker never waits for the bus.
            if let (Some(forgot), Some(out)) = (forgot, self.forgot.as_ref()) {
                let told = out
                    .lock()
                    .map_err(|_| io::Error::other("poisoned"))
                    .and_then(|mut out| out.tell(&forgot));
                if told.is_err() {
                    log("a forget could not be told on the bus (memory/forgot)");
                }
            }
            // What was asked has been done either way; only the answer is
            // lost.
            match sent.map_err(|error| error.kind()) {
                Ok(()) => {}
                Err(io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    log("a client did not take its answer in time and was dropped");
                    return;
                }
                Err(_) => {
                    log("a client left before its answer");
                    return;
                }
            }
        }
    }

    /// The store's answer to one request line, measured when reporting.
    fn answer(
        &self,
        line: &[u8],
        store: &mut (impl Memory + ?Sized),
        clock: &mut dyn FnMut() -> u64,
        health: Option<&crate::health::State>,
    ) -> protocol::Answer {
        if let Some(health) = health {
            let ticket = health.begin(line);
            let answer = store.answer(line, clock());
            let ended = health.answered_at();
            health.finish(ticket, ended, &answer, store.health_counts());
            // A forget, or a stamp older than `keep_days`, removes it (#2067).
            health.set_aside(store.set_aside());
            answer
        } else {
            store.answer(line, clock())
        }
    }

    /// The client's next line. `held` is what has been read past the end
    /// of the last one.
    fn line(&self, mut stream: &UnixStream, held: &mut Vec<u8>) -> Line {
        let deadline = Instant::now() + self.patience;
        let mut chunk = [0u8; 8192];
        // How much of `held` is known to have no line end in it, and
        // whether the line being read has already outgrown its limit.
        let (mut searched, mut too_long) = (0, false);
        loop {
            if let Some(end) = held[searched..].iter().position(|byte| *byte == b'\n') {
                let mut line: Vec<u8> = held.drain(..=searched + end).collect();
                line.pop();
                return if too_long || line.len() > MOST_LINE_BYTES {
                    Line::TooLong
                } else {
                    Line::Whole(line)
                };
            }
            if held.len() > MOST_LINE_BYTES {
                // No end in sight. The rest of the line is let go by, not
                // held, so a client cannot make this process grow.
                too_long = true;
                held.clear();
            }
            searched = held.len();
            let unfinished = too_long || !held.is_empty();
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
                return Line::Late { unfinished };
            }
            match stream.read(&mut chunk) {
                // Closed. A last line without its end is still a request,
                // unless it was too long to have been kept.
                Ok(0) if too_long || held.is_empty() => return Line::Closed,
                Ok(0) => return Line::Whole(std::mem::take(held)),
                Ok(count) => held.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Line::Late { unfinished };
                }
                Err(_) => return Line::Closed,
            }
        }
    }

    /// Write the whole answer, or give up when the client has not taken it
    /// in time.
    fn send(&self, mut stream: &UnixStream, mut bytes: &[u8]) -> io::Result<()> {
        let deadline = Instant::now() + self.patience;
        while !bytes.is_empty() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            stream.set_write_timeout(Some(left))?;
            match stream.write(bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => bytes = &bytes[count..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

enum Line {
    Whole(Vec<u8>),
    /// A line over the limit, read to its end and thrown away.
    TooLong,
    /// Nothing more came in time. `unfinished`: part of a request had.
    Late {
        unfinished: bool,
    },
    Closed,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.ready);
        let _ = fs::remove_file(&self.socket);
        if let Some((_, forget)) = &self.forget {
            let _ = fs::remove_file(forget);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_is_found_by_its_whole_name_only() {
        let file = "root:x:0:\nlamp-app:x:1001:lamp\nlamp:x:1000:\nbroken:x:\n";
        assert_eq!(group_in(file, "lamp-app"), Some(1001));
        assert_eq!(group_in(file, "lamp"), Some(1000));
        assert_eq!(group_in(file, "lamp-ap"), None);
        assert_eq!(group_in(file, "broken"), None);
        assert_eq!(group_in(file, "x"), None);
    }
}
