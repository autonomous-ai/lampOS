//! A skill run's two ways out (#6306, the first builder's design on
//! #6443): a skill's run (`skill_task::HermesRunner`) is started with
//! [`crate::skill_task::NET_ENV`] `run`, so Hermes's wrapper gives it a
//! network namespace of its own with only `lo`
//! (`lamp_update::hermes_install::wrapper_text`). No address outside it
//! resolves or connects. Its two ways out are files its namespace script
//! shows read-only: lamp-control's `control.sock`, and [`MODEL_SOCKET`],
//! this module's bridge to lamp-cloud's model forwarder.
//!
//! - **The bridge** ([`Bridge`], outside, in `lamp-tasks serve` while
//!   `LAMP_SKILL_TASKS` is on): [`MODEL_SOCKET`], made as lamp-tasks' other
//!   sockets are (0600 in its 0700 runtime folder, `runtime::bind`), each
//!   connection piped to lamp-cloud's forwarder on TCP [`FORWARDER`].
//! - **The relay** ([`relay_main`], `lamp-tasks net-relay`, inside the
//!   run's namespace, started by its namespace script before Hermes):
//!   TCP `127.0.0.1:7741`, where Hermes's model calls go, each connection
//!   piped to [`MODEL_SOCKET`].
//!
//! Both are bounded: at most [`MOST_PIPES`] connections at once (one more
//! is closed at once), [`CONNECT_WITHIN`] to reach the other side, each
//! direction ended after [`IDLE_WITHIN`] with no byte and each connection
//! after [`PIPE_WITHIN`]. Bytes are piped, never read as HTTP: nothing of
//! a body or a header (the forwarder's token travels in one) is ever
//! logged; the few lines said are fixed text.
//!
//! Neither gives a run more than its own uid already has: lamp-cloud's
//! forwarder answers any local caller on `127.0.0.1:7741`, and only with
//! its own token.
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The bridge's socket, in lamp-tasks' runtime folder (`lampos`'s, 0700),
/// 0600: what a skill run's namespace script shows it, read-only
/// (`lamp_update::hermes_install::MODEL_SOCKET`).
pub const MODEL_SOCKET: &str = "/run/lampos-tasks/model.sock";
/// lamp-cloud's model forwarder (`lamp_cloud::hermes_llm::PORT`), where
/// the bridge pipes each connection.
pub const FORWARDER: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 7741);
/// The most connections piped at once, each way: lamp-cloud's forwarder
/// runs as many requests at once (`hermes_llm::AT_ONCE`).
pub const MOST_PIPES: usize = 4;
/// How long reaching the other side may take.
pub const CONNECT_WITHIN: Duration = Duration::from_secs(5);
/// How long one direction may pass no byte before the connection ends:
/// past the forwarder's own read bound (`hermes_llm::READ_WITHIN`, 120 s).
pub const IDLE_WITHIN: Duration = Duration::from_secs(130);
/// One connection's whole life: past the forwarder's longest answer
/// (`hermes_llm::ANSWER_WITHIN`, 600 s) with its head, body and write.
pub const PIPE_WITHIN: Duration = Duration::from_secs(700);
/// How often a non-blocking listener is looked at while no one connects.
const ACCEPT_TICK: Duration = Duration::from_millis(20);
/// The line the relay prints once it listens, then closes its output: the
/// namespace script waits for it before Hermes runs.
pub const READY: &str = "ready";

/// What is said: fixed text only, never a byte piped.
pub type Say = Arc<dyn Fn(&str) + Send + Sync>;

/// One end of a pipe: a TCP or unix stream.
pub trait End: Read + Write + Send + 'static {
    fn split(&self) -> io::Result<Box<dyn End>>;
    fn read_within(&self, wait: Option<Duration>) -> io::Result<()>;
    fn write_within(&self, wait: Option<Duration>) -> io::Result<()>;
    fn blocking(&self) -> io::Result<()>;
    fn end(&self, how: Shutdown);
}

impl End for TcpStream {
    fn split(&self) -> io::Result<Box<dyn End>> {
        Ok(Box::new(self.try_clone()?))
    }
    fn read_within(&self, wait: Option<Duration>) -> io::Result<()> {
        self.set_read_timeout(wait)
    }
    fn write_within(&self, wait: Option<Duration>) -> io::Result<()> {
        self.set_write_timeout(wait)
    }
    fn blocking(&self) -> io::Result<()> {
        self.set_nonblocking(false)
    }
    fn end(&self, how: Shutdown) {
        let _ = self.shutdown(how);
    }
}

impl End for UnixStream {
    fn split(&self) -> io::Result<Box<dyn End>> {
        Ok(Box::new(self.try_clone()?))
    }
    fn read_within(&self, wait: Option<Duration>) -> io::Result<()> {
        self.set_read_timeout(wait)
    }
    fn write_within(&self, wait: Option<Duration>) -> io::Result<()> {
        self.set_write_timeout(wait)
    }
    fn blocking(&self) -> io::Result<()> {
        self.set_nonblocking(false)
    }
    fn end(&self, how: Shutdown) {
        let _ = self.shutdown(how);
    }
}

/// The bounds of every pipe.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub most: usize,
    pub connect: Duration,
    pub idle: Duration,
    pub life: Duration,
}

impl Bounds {
    /// [`MOST_PIPES`], [`CONNECT_WITHIN`], [`IDLE_WITHIN`], [`PIPE_WITHIN`].
    pub const LAMP: Self = Self {
        most: MOST_PIPES,
        connect: CONNECT_WITHIN,
        idle: IDLE_WITHIN,
        life: PIPE_WITHIN,
    };
}

/// Bytes from `from` to `to` until `from` ends (its write side then ended
/// on `to`), no byte for `idle`, `until` passes, or either fails (both
/// then ended whole, so the other direction ends too).
fn copy(mut from: Box<dyn End>, mut to: Box<dyn End>, idle: Duration, until: Instant) {
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || from.read_within(Some(left.min(idle))).is_err() {
            break;
        }
        match from.read(&mut chunk) {
            Ok(0) => {
                to.end(Shutdown::Write);
                return;
            }
            Ok(read) => {
                if to.write_all(&chunk[..read]).is_err() {
                    break;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    from.end(Shutdown::Both);
    to.end(Shutdown::Both);
}

/// `a` and `b` piped both ways until both directions end, within
/// `bounds`. Blocks the calling thread; one more thread for the other way.
pub fn pipe(a: Box<dyn End>, b: Box<dyn End>, bounds: Bounds) {
    let until = Instant::now() + bounds.life;
    let set = |end: &dyn End| {
        end.blocking()
            .and_then(|()| end.write_within(Some(bounds.idle)))
    };
    if set(&*a).is_err() || set(&*b).is_err() {
        a.end(Shutdown::Both);
        b.end(Shutdown::Both);
        return;
    }
    let (a2, b2) = match (a.split(), b.split()) {
        (Ok(a2), Ok(b2)) => (a2, b2),
        _ => {
            a.end(Shutdown::Both);
            b.end(Shutdown::Both);
            return;
        }
    };
    let idle = bounds.idle;
    let back = std::thread::Builder::new()
        .name("tasks-net-pipe".into())
        .spawn(move || copy(b2, a2, idle, until));
    match back {
        Ok(back) => {
            copy(a, b, idle, until);
            let _ = back.join();
        }
        Err(_) => {
            a.end(Shutdown::Both);
            b.end(Shutdown::Both);
        }
    }
}

/// One of [`Bounds::most`] held while its pipe lives.
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Every connection `accept` gives piped to one `connect` makes, each on a
/// thread of its own, at most `bounds.most` at once (one more closed at
/// once), until `stop`. A non-blocking listener is looked at every
/// [`ACCEPT_TICK`]; a blocking one waits.
fn serve(
    mut accept: impl FnMut() -> io::Result<Box<dyn End>>,
    connect: Arc<dyn Fn(Duration) -> io::Result<Box<dyn End>> + Send + Sync>,
    bounds: Bounds,
    stop: &AtomicBool,
    say: &Say,
    name: &'static str,
) {
    let open = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::Acquire) {
        let stream = match accept() {
            Ok(stream) => stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(ACCEPT_TICK);
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                say(&format!(
                    "[lamp-tasks] {name}: a connection was not accepted"
                ));
                std::thread::sleep(ACCEPT_TICK);
                continue;
            }
        };
        if open.fetch_add(1, Ordering::AcqRel) >= bounds.most {
            open.fetch_sub(1, Ordering::AcqRel);
            say(&format!(
                "[lamp-tasks] {name}: closed a connection past the most at once"
            ));
            stream.end(Shutdown::Both);
            continue;
        }
        let held = Counted(open.clone());
        let (connect, say2) = (connect.clone(), say.clone());
        // A thread not started drops `held` and the stream with it.
        let _ = std::thread::Builder::new()
            .name("tasks-net-bridge".into())
            .spawn(move || {
                let _held = held;
                match connect(bounds.connect) {
                    Ok(other) => pipe(stream, other, bounds),
                    Err(_) => {
                        say2(&format!(
                            "[lamp-tasks] {name}: the other side did not answer"
                        ));
                        stream.end(Shutdown::Both);
                    }
                }
            });
    }
}

/// A unix socket path reached within `wait`. Connecting to a unix socket
/// does not block on a listener that is there, so `wait` bounds only the
/// connection's first use.
fn connect_unix(path: &Path, _wait: Duration) -> io::Result<Box<dyn End>> {
    Ok(Box::new(UnixStream::connect(path)?))
}

fn connect_tcp(to: SocketAddr, wait: Duration) -> io::Result<Box<dyn End>> {
    Ok(Box::new(TcpStream::connect_timeout(&to, wait)?))
}

/// The bridge: [`MODEL_SOCKET`] served, each connection piped to `to`
/// ([`FORWARDER`] on a lamp).
pub struct Bridge {
    pub socket: PathBuf,
    pub to: SocketAddr,
    pub bounds: Bounds,
    pub say: Say,
}

impl Bridge {
    /// The lamp's: [`MODEL_SOCKET`] to [`FORWARDER`], [`Bounds::LAMP`],
    /// said on lamp-tasks' journal.
    pub fn lamp() -> Self {
        Self {
            socket: PathBuf::from(MODEL_SOCKET),
            to: FORWARDER,
            bounds: Bounds::LAMP,
            say: Arc::new(|line| eprintln!("{line}")),
        }
    }

    /// With `on`, the socket bound (0600, `runtime::bind`) and served on a
    /// thread of its own until `stop`, the socket removed then; off,
    /// nothing is bound and `None`.
    pub fn start(
        self,
        on: bool,
        stop: Arc<AtomicBool>,
    ) -> io::Result<Option<std::thread::JoinHandle<()>>> {
        if !on {
            return Ok(None);
        }
        let (listener, binding) = crate::runtime::bind(&self.socket)?;
        let served = std::thread::Builder::new()
            .name("tasks-model-bridge".into())
            .spawn(move || {
                let _binding = binding;
                let to = self.to;
                serve(
                    || accept_unix(&listener),
                    Arc::new(move |wait| connect_tcp(to, wait)),
                    self.bounds,
                    &stop,
                    &self.say,
                    "model bridge",
                );
            })?;
        Ok(Some(served))
    }
}

fn accept_unix(listener: &UnixListener) -> io::Result<Box<dyn End>> {
    Ok(Box::new(listener.accept()?.0))
}

fn accept_tcp(listener: &TcpListener) -> io::Result<Box<dyn End>> {
    Ok(Box::new(listener.accept()?.0))
}

/// The relay inside a run's namespace: each connection on `listener`
/// piped to the unix socket `to`, until `stop`.
pub fn relay(listener: &TcpListener, to: &Path, bounds: Bounds, stop: &AtomicBool, say: &Say) {
    let to = to.to_path_buf();
    serve(
        || accept_tcp(listener),
        Arc::new(move |wait| connect_unix(&to, wait)),
        bounds,
        stop,
        say,
        "net relay",
    );
}

/// `net-relay --listen ADDR --to PATH`: a loopback address and an absolute
/// socket path, each once, nothing else.
pub fn relay_args(args: &[String]) -> Result<(SocketAddr, PathBuf), String> {
    let usage = "usage: lamp-tasks net-relay --listen 127.0.0.1:PORT --to /ABS/SOCKET";
    let (mut listen, mut to) = (None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let value = args.next().ok_or(usage)?;
        match arg.as_str() {
            "--listen" if listen.is_none() => {
                let addr: SocketAddr = value.parse().map_err(|_| usage)?;
                if !addr.ip().is_loopback() {
                    return Err("net-relay listens only on a loopback address".into());
                }
                listen = Some(addr);
            }
            "--to" if to.is_none() => {
                let path = PathBuf::from(value);
                if !path.is_absolute() {
                    return Err(usage.into());
                }
                to = Some(path);
            }
            _ => return Err(usage.into()),
        }
    }
    Ok((listen.ok_or(usage)?, to.ok_or(usage)?))
}

/// `lamp-tasks net-relay`: listens, prints [`READY`], puts `/dev/null`
/// over its output (so the namespace script's `$(...)` ends), and relays
/// until it is killed (the run's PID namespace ending).
pub fn relay_main(args: &[String]) -> Result<(), String> {
    let (listen, to) = relay_args(args)?;
    let listener = TcpListener::bind(listen).map_err(|error| format!("{listen}: {error}"))?;
    let mut out = io::stdout();
    writeln!(out, "{READY}")
        .and_then(|()| out.flush())
        .map_err(|error| format!("ready not said: {error}"))?;
    let null = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .map_err(|error| format!("/dev/null: {error}"))?;
    rustix::stdio::dup2_stdout(&null).map_err(|error| format!("output not closed: {error}"))?;
    let stop = AtomicBool::new(false);
    let say: Say = Arc::new(|line| eprintln!("{line}"));
    relay(&listener, &to, Bounds::LAMP, &stop, &say);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn private() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        tempfile::Builder::new()
            .prefix("tsk-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap()
    }

    fn quick(most: usize) -> Bounds {
        Bounds {
            most,
            connect: Duration::from_secs(2),
            idle: Duration::from_secs(5),
            life: Duration::from_secs(20),
        }
    }

    fn heard() -> (Say, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let kept = lines.clone();
        (
            Arc::new(move |line: &str| kept.lock().unwrap().push(line.to_owned())),
            lines,
        )
    }

    /// A stand-in forwarder on an ephemeral port: reads one request up to
    /// its blank line, keeps it, answers with `answer` and closes.
    fn stand_in(answer: &'static [u8]) -> (SocketAddr, std::sync::mpsc::Receiver<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (got_tx, got_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let got_tx = got_tx.clone();
                std::thread::spawn(move || {
                    let mut got = Vec::new();
                    let mut byte = [0u8; 1];
                    while !got.ends_with(b"\r\n\r\n") {
                        match stream.read(&mut byte) {
                            Ok(1) => got.push(byte[0]),
                            _ => return,
                        }
                    }
                    let _ = got_tx.send(got);
                    let _ = stream.write_all(answer);
                });
            }
        });
        (addr, got_rx)
    }

    const ASK: &[u8] = b"POST /v1/deepseek/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:7741\r\n\
        Authorization: Bearer tok_secret_6306\r\n\r\n";
    const ANSWER: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";

    fn bridge(
        to: SocketAddr,
        most: usize,
        say: Say,
    ) -> (tempfile::TempDir, PathBuf, Arc<AtomicBool>) {
        let dir = private();
        let socket = dir.path().join("model.sock");
        let stop = Arc::new(AtomicBool::new(false));
        Bridge {
            socket: socket.clone(),
            to,
            bounds: quick(most),
            say,
        }
        .start(true, stop.clone())
        .unwrap()
        .expect("on binds");
        (dir, socket, stop)
    }

    fn relay_on(to: PathBuf, most: usize, say: Say) -> (SocketAddr, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || relay(&listener, &to, quick(most), &stopped, &say));
        (addr, stop)
    }

    fn read_all(mut stream: impl Read) -> Vec<u8> {
        let mut got = Vec::new();
        let _ = stream.read_to_end(&mut got);
        got
    }

    /// A model call as a run makes it: TCP into the relay, through the
    /// bridge's unix socket, TCP out to the forwarder, and its answer
    /// back the same way, byte for byte.
    #[test]
    fn a_call_is_piped_tcp_to_unix_to_tcp_and_back() {
        let (forwarder, got) = stand_in(ANSWER);
        let (say, lines) = heard();
        let (_dir, socket, stop_bridge) = bridge(forwarder, 4, say.clone());
        let (relay_at, stop_relay) = relay_on(socket.clone(), 4, say);
        let mut call = TcpStream::connect(relay_at).unwrap();
        call.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        call.write_all(ASK).unwrap();
        assert_eq!(got.recv_timeout(Duration::from_secs(10)).unwrap(), ASK);
        assert_eq!(read_all(&call), ANSWER);
        // The socket is the runtime folder's: 0600.
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&socket).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        stop_bridge.store(true, Ordering::Release);
        stop_relay.store(true, Ordering::Release);
        assert!(
            lines.lock().unwrap().is_empty(),
            "nothing said for a good call"
        );
    }

    /// Past the most at once, a connection is closed at once, without
    /// reaching the other side, and the bound is let go when a pipe ends.
    #[test]
    fn past_the_most_at_once_a_connection_is_closed() {
        // A forwarder that holds each connection open, answering nothing.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let forwarder = listener.local_addr().unwrap();
        let reached = Arc::new(AtomicUsize::new(0));
        let counted = reached.clone();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming().flatten() {
                counted.fetch_add(1, Ordering::AcqRel);
                held.push(stream);
            }
        });
        let (say, lines) = heard();
        let (_dir, socket, stop) = bridge(forwarder, 1, say);
        let first = UnixStream::connect(&socket).unwrap();
        let began = Instant::now();
        while reached.load(Ordering::Acquire) < 1 {
            assert!(began.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
        let second = UnixStream::connect(&socket).unwrap();
        second
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        assert!(read_all(&second).is_empty(), "closed at once");
        assert_eq!(
            reached.load(Ordering::Acquire),
            1,
            "never reached the forwarder"
        );
        assert!(
            lines
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains("past the most")),
            "{:?}",
            lines.lock().unwrap()
        );
        // The first ended, a new one is piped again.
        first.shutdown(Shutdown::Both).unwrap();
        drop(first);
        let began = Instant::now();
        loop {
            let third = UnixStream::connect(&socket).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            if reached.load(Ordering::Acquire) >= 2 {
                break;
            }
            drop(third);
            assert!(
                began.elapsed() < Duration::from_secs(10),
                "the bound was not let go"
            );
        }
        stop.store(true, Ordering::Release);
    }

    /// A direction with no byte for its idle bound ends the connection.
    #[test]
    fn an_idle_pipe_is_ended() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let forwarder = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming().flatten() {
                held.push(stream);
            }
        });
        let a = TcpStream::connect(forwarder).unwrap();
        let (b_near, b_far) = UnixStream::pair().unwrap();
        let bounds = Bounds {
            idle: Duration::from_millis(200),
            ..quick(1)
        };
        let began = Instant::now();
        let piping = std::thread::spawn(move || pipe(Box::new(a), Box::new(b_near), bounds));
        b_far
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        assert!(read_all(&b_far).is_empty());
        piping.join().unwrap();
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "{:?}",
            began.elapsed()
        );
    }

    /// Nothing piped is ever said: a forwarder that is not there is said in
    /// fixed text, never with the ask's head (its token among it).
    #[test]
    fn nothing_piped_is_ever_said() {
        // An address with no listener: bound, then closed.
        let gone = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let (say, lines) = heard();
        let (_dir, socket, stop) = bridge(gone, 4, say.clone());
        let (relay_at, stop_relay) = relay_on(socket, 4, say);
        let mut call = TcpStream::connect(relay_at).unwrap();
        call.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let _ = call.write_all(ASK);
        assert!(read_all(&call).is_empty());
        let began = Instant::now();
        while lines.lock().unwrap().is_empty() {
            assert!(began.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
        stop.store(true, Ordering::Release);
        stop_relay.store(true, Ordering::Release);
        for line in lines.lock().unwrap().iter() {
            assert!(!line.contains("tok_secret"), "{line}");
            assert!(!line.contains("Bearer") && !line.contains("POST"), "{line}");
            assert!(line.contains("did not answer"), "{line}");
        }
    }

    #[test]
    fn the_relay_takes_only_a_loopback_address_and_an_absolute_socket() {
        let args =
            |line: &str| -> Vec<String> { line.split_whitespace().map(str::to_owned).collect() };
        assert_eq!(
            relay_args(&args(
                "--listen 127.0.0.1:7741 --to /run/lampos-tasks/model.sock"
            )),
            Ok((FORWARDER, PathBuf::from(MODEL_SOCKET)))
        );
        for bad in [
            "",
            "--listen 0.0.0.0:7741 --to /run/x.sock",
            "--listen 10.0.0.2:7741 --to /run/x.sock",
            "--listen 127.0.0.1:7741 --to run/x.sock",
            "--listen 127.0.0.1:7741",
            "--to /run/x.sock",
            "--listen 127.0.0.1:7741 --listen 127.0.0.1:7742 --to /run/x.sock",
            "--listen 127.0.0.1:7741 --to /run/x.sock --more x",
            "--listen 127.0.0.1:7741 --to",
        ] {
            assert!(relay_args(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_lamps_bridge_is_its_socket_to_the_forwarder() {
        let bridge = Bridge::lamp();
        assert_eq!(bridge.socket, Path::new("/run/lampos-tasks/model.sock"));
        assert_eq!(bridge.to.to_string(), "127.0.0.1:7741");
        assert_eq!(bridge.bounds.most, 4);
        // The same folder as lamp-tasks' other sockets.
        assert_eq!(
            Path::new(MODEL_SOCKET).parent(),
            Path::new(crate::skill_task::HAND_OVER_SOCKET).parent()
        );
    }

    #[test]
    fn off_binds_nothing() {
        let dir = private();
        let socket = dir.path().join("model.sock");
        let started = Bridge {
            socket: socket.clone(),
            to: FORWARDER,
            bounds: quick(1),
            say: heard().0,
        }
        .start(false, Arc::new(AtomicBool::new(false)))
        .unwrap();
        assert!(started.is_none());
        assert!(!socket.exists());
    }
}
