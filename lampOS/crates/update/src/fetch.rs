//! How a build's files are read: from a channel's folder over HTTPS (a
//! GitHub release, which answers with a redirect to where the file is), or
//! from a folder on the lamp itself (`file:///...`), which is how a build is
//! first tried by hand.
//!
//! Behind [`Channel`], so that the tests play a channel of their own, one
//! with no network among them. A file is written as it comes into what the
//! caller gives; the caller checks its SHA-256 and throws away what fails.
//! HTTP/1.1 is spoken here directly over rustls, as lamp-policy speaks it to
//! its decision models: one GET, `Connection: close`, at most five
//! redirects and only to https.
use rustls::pki_types::ServerName;
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

/// Why a file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetch {
    /// The channel could not be reached: no network, no name, no answer.
    Unreachable(String),
    /// The channel answered that there is no such file.
    Missing,
    /// Anything else: an answer that is not one, a file too large, a
    /// connection lost half way.
    Failed(String),
}

impl std::fmt::Display for Fetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "the channel cannot be reached: {why}"),
            Self::Missing => f.write_str("the channel has no such file"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

pub trait Channel {
    /// The file at `url`, written into `out`: at most `most` bytes, and how
    /// many it was.
    fn fetch(&mut self, url: &str, most: u64, out: &mut dyn Write) -> Result<u64, Fetch>;
}

/// The redirects followed before giving up: GitHub's release downloads
/// take one.
const MOST_REDIRECTS: usize = 5;
/// The largest head of an answer.
const MOST_HEAD_BYTES: usize = 32 * 1024;
/// The longest line of a chunked body's framing.
const MOST_LINE_BYTES: usize = 1024;

/// The real channel: HTTPS with the web's usual roots, and folders on the
/// lamp.
pub struct Web {
    tls: Arc<rustls::ClientConfig>,
    /// To connect to one address.
    connect: Duration,
    /// For one read or write to make progress.
    stall: Duration,
    /// For a whole file.
    whole: Duration,
}

impl Web {
    pub fn new() -> io::Result<Self> {
        Self::within(
            Duration::from_secs(10),
            Duration::from_secs(30),
            Duration::from_secs(900),
        )
    }

    /// The real channel with its own bounds: `connect` to one address,
    /// `stall` for one read or write to make progress, `whole` for a whole
    /// file. A skill's package from the Store is fetched within a shorter
    /// whole than a build's binaries (`lamp_skills::package`).
    pub fn within(connect: Duration, stall: Duration, whole: Duration) -> io::Result<Self> {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            tls: Arc::new(tls),
            connect,
            stall,
            whole,
        })
    }

    fn open(&self, url: &Url, until: Instant) -> Result<Box<dyn Stream>, Fetch> {
        let left = until.saturating_duration_since(Instant::now());
        let addresses = lookup_within(
            &url.host,
            url.port,
            self.connect.min(left),
            &LOOKUPS,
            MOST_LOOKUPS,
            |host, port| (host, port).to_socket_addrs().map(Iterator::collect),
        )
        .map_err(Fetch::Unreachable)?;
        let mut last = format!("{}: no address", url.host);
        for address in addresses {
            let left = until.saturating_duration_since(Instant::now());
            match TcpStream::connect_timeout(
                &address,
                self.connect.min(left).max(Duration::from_millis(1)),
            ) {
                Ok(socket) => {
                    socket.set_read_timeout(Some(self.stall)).ok();
                    socket.set_write_timeout(Some(self.stall)).ok();
                    let name = ServerName::try_from(url.host.clone())
                        .map_err(|_| Fetch::Failed(format!("{} is not a host name", url.host)))?;
                    let session = rustls::ClientConnection::new(Arc::clone(&self.tls), name)
                        .map_err(|error| Fetch::Failed(format!("TLS: {error}")))?;
                    return Ok(Box::new(rustls::StreamOwned::new(session, socket)));
                }
                Err(error) => last = format!("{address}: {error}"),
            }
        }
        Err(Fetch::Unreachable(last))
    }
}

/// Name lookups still running, across every round of this process: at most
/// [`MOST_LOOKUPS`] at once (the reviewer on #884).
static LOOKUPS: AtomicUsize = AtomicUsize::new(0);
/// One lookup at a time: a resolver that never answers holds one thread,
/// and no round makes another until it ends.
pub const MOST_LOOKUPS: usize = 1;

/// The host's addresses, or why not, within `limit`. The system's name
/// lookup has no timeout of its own: a resolver that never answers (a home
/// router rebooting, #830) would hold the round. It runs on a thread of its
/// own and the round waits at most `limit`; one that runs over is left to
/// end by itself, counted in `running`, and while `most` are still running
/// a round starts none and says so at once.
pub fn lookup_within(
    host: &str,
    port: u16,
    limit: Duration,
    running: &'static AtomicUsize,
    most: usize,
    lookup: impl FnOnce(&str, u16) -> io::Result<Vec<SocketAddr>> + Send + 'static,
) -> Result<Vec<SocketAddr>, String> {
    if running
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |now| {
            (now < most).then_some(now + 1)
        })
        .is_err()
    {
        return Err(format!("{host}: an earlier name lookup has not ended"));
    }
    /// Counts the lookup's thread out however it ends.
    struct Running(&'static AtomicUsize);
    impl Drop for Running {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }
    let counted = Running(running);
    let (tell, told) = mpsc::sync_channel(1);
    let name = host.to_owned();
    std::thread::Builder::new()
        .name("lamp-update-lookup".into())
        .spawn(move || {
            let answer = lookup(&name, port);
            // Counted out before the answer is sent, so a caller that has it
            // never finds this lookup still running: a redirect's lookup,
            // made at once after the first answered, is not refused as "an
            // earlier name lookup has not ended" (Tests, #1112).
            drop(counted);
            let _ = tell.send(answer);
        })
        .map_err(|error| format!("{host}: {error}"))?;
    match told.recv_timeout(limit) {
        Ok(Ok(addresses)) => Ok(addresses),
        Ok(Err(error)) => Err(format!("{host}: {error}")),
        Err(_) => Err(format!(
            "{host}: the name lookup gave no answer within {} s",
            limit.as_secs()
        )),
    }
}

impl Channel for Web {
    fn fetch(&mut self, url: &str, most: u64, out: &mut dyn Write) -> Result<u64, Fetch> {
        if let Some(path) = url.strip_prefix("file://") {
            return from_folder(Path::new(path), most, out);
        }
        let until = Instant::now() + self.whole;
        let this = &*self;
        get(url, most, out, until, |url| this.open(url, until))
    }
}

/// One GET of the https `url` and the redirects it leads to (only to https,
/// at most five), within `whole`, each exchange over a stream `open` makes
/// for a host and a port: what [`Web`] does, with the connection the
/// caller's. Another crate's tests speak plain HTTP to a local server
/// through it, as this module's own tests do.
pub fn get_over(
    url: &str,
    most: u64,
    out: &mut dyn Write,
    whole: Duration,
    mut open: impl FnMut(&str, u16) -> Result<Box<dyn Stream>, Fetch>,
) -> Result<u64, Fetch> {
    get(url, most, out, Instant::now() + whole, |url| {
        open(&url.host, url.port)
    })
}

/// A file on the lamp itself.
fn from_folder(path: &Path, most: u64, out: &mut dyn Write) -> Result<u64, Fetch> {
    let file = std::fs::File::open(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Fetch::Missing,
        _ => Fetch::Failed(format!("{}: {error}", path.display())),
    })?;
    copy_most(&mut io::BufReader::new(file), most, out, None)
}

/// Something HTTP can be spoken over.
pub trait Stream: Read + Write {}
impl<S: Read + Write> Stream for S {}

/// An https URL, taken apart.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Url {
    host: String,
    port: u16,
    /// From the first `/`.
    path: String,
}

impl Url {
    fn parse(url: &str) -> Result<Self, Fetch> {
        let not = || Fetch::Failed(format!("{url} is not an https URL"));
        let rest = url.strip_prefix("https://").ok_or_else(not)?;
        let (authority, path) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        if authority.is_empty() || authority.contains('@') || path.contains(char::is_whitespace) {
            return Err(not());
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().map_err(|_| not())?),
            None => (authority, 443),
        };
        Ok(Self {
            host: host.to_ascii_lowercase(),
            port,
            path: path.to_owned(),
        })
    }

    /// Where a redirect leads: an https URL, or a path on the same host.
    fn follow(&self, location: &str) -> Result<Self, Fetch> {
        if location.starts_with('/') && !location.starts_with("//") {
            return Ok(Self {
                path: location.to_owned(),
                ..self.clone()
            });
        }
        Self::parse(location)
    }
}

/// What one exchange ended with.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Body(u64),
    Redirect(String),
    Missing,
}

/// One GET of `url` and the redirects it leads to, each over a stream
/// `open` makes.
fn get(
    url: &str,
    most: u64,
    out: &mut dyn Write,
    until: Instant,
    mut open: impl FnMut(&Url) -> Result<Box<dyn Stream>, Fetch>,
) -> Result<u64, Fetch> {
    let mut url = Url::parse(url)?;
    for _ in 0..=MOST_REDIRECTS {
        let mut stream = open(&url)?;
        match exchange(&mut stream, &url, most, out, until)? {
            Answer::Body(bytes) => return Ok(bytes),
            Answer::Missing => return Err(Fetch::Missing),
            Answer::Redirect(location) => url = url.follow(&location)?,
        }
    }
    Err(Fetch::Failed(format!(
        "more than {MOST_REDIRECTS} redirects"
    )))
}

fn lost(error: io::Error) -> Fetch {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
            Fetch::Unreachable(format!("no answer: {error}"))
        }
        _ => Fetch::Failed(format!("the connection was lost: {error}")),
    }
}

/// A GET over `stream` and its answer: the body written into `out` when it
/// is 200.
fn exchange(
    stream: &mut dyn Stream,
    url: &Url,
    most: u64,
    out: &mut dyn Write,
    until: Instant,
) -> Result<Answer, Fetch> {
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: lamp-update\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        url.path, url.host
    );
    stream.write_all(request.as_bytes()).map_err(lost)?;
    stream.flush().map_err(lost)?;
    let mut reader = Held::new(stream, until);
    let head = reader.head()?;
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    let parsed = response
        .parse(&head)
        .map_err(|error| Fetch::Failed(format!("the answer's head cannot be read: {error}")))?;
    if parsed.is_partial() {
        return Err(Fetch::Failed("the answer's head is cut short".into()));
    }
    let code = response.code.unwrap_or(0);
    let header = |name: &str| {
        response
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| String::from_utf8_lossy(header.value).trim().to_owned())
    };
    match code {
        200 => {}
        301 | 302 | 303 | 307 | 308 => {
            let location = header("location")
                .ok_or_else(|| Fetch::Failed(format!("a redirect ({code}) with no Location")))?;
            return Ok(Answer::Redirect(location));
        }
        404 | 410 => return Ok(Answer::Missing),
        _ => return Err(Fetch::Failed(format!("the channel answered {code}"))),
    }
    let chunked = header("transfer-encoding")
        .is_some_and(|value| value.to_ascii_lowercase().contains("chunked"));
    if chunked {
        return reader.chunked(most, out).map(Answer::Body);
    }
    match header("content-length") {
        Some(length) => {
            let length: u64 = length
                .parse()
                .map_err(|_| Fetch::Failed(format!("Content-Length {length:?}")))?;
            if length > most {
                return Err(too_large(most));
            }
            let read = copy_most(&mut reader, length, out, Some(until))?;
            if read != length {
                return Err(Fetch::Failed(format!(
                    "the file ended after {read} of {length} bytes"
                )));
            }
            Ok(Answer::Body(read))
        }
        None => copy_most(&mut reader, most, out, Some(until)).map(Answer::Body),
    }
}

fn too_large(most: u64) -> Fetch {
    Fetch::Failed(format!("the file is larger than the {most} bytes expected"))
}

/// Copies until the end, refusing more than `most` bytes.
fn copy_most(
    from: &mut dyn Read,
    most: u64,
    out: &mut dyn Write,
    until: Option<Instant>,
) -> Result<u64, Fetch> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        if until.is_some_and(|until| Instant::now() > until) {
            return Err(Fetch::Failed("the file took too long".into()));
        }
        let read = match from.read(&mut buffer) {
            Ok(0) => return Ok(total),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            // A server that closes without TLS's goodbye: what came is
            // checked by its hash all the same.
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(total),
            Err(error) => return Err(lost(error)),
        };
        total += read as u64;
        if total > most {
            return Err(too_large(most));
        }
        out.write_all(&buffer[..read])
            .map_err(|error| Fetch::Failed(format!("the file cannot be written: {error}")))?;
    }
}

/// A stream with what was read ahead of the caller held.
struct Held<'a> {
    stream: &'a mut dyn Stream,
    held: Vec<u8>,
    until: Instant,
}

impl<'a> Held<'a> {
    fn new(stream: &'a mut dyn Stream, until: Instant) -> Self {
        Self {
            stream,
            held: Vec::new(),
            until,
        }
    }

    fn more(&mut self) -> Result<usize, Fetch> {
        if Instant::now() > self.until {
            return Err(Fetch::Failed("the answer took too long".into()));
        }
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match self.stream.read(&mut buffer) {
                Ok(read) => {
                    self.held.extend_from_slice(&buffer[..read]);
                    return Ok(read);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(0),
                Err(error) => return Err(lost(error)),
            }
        }
    }

    /// The head, up to and with its empty line. What follows stays held.
    fn head(&mut self) -> Result<Vec<u8>, Fetch> {
        loop {
            if let Some(at) = self.held.windows(4).position(|four| four == b"\r\n\r\n") {
                let rest = self.held.split_off(at + 4);
                return Ok(std::mem::replace(&mut self.held, rest));
            }
            if self.held.len() > MOST_HEAD_BYTES {
                return Err(Fetch::Failed("the answer's head is too long".into()));
            }
            if self.more()? == 0 {
                return Err(Fetch::Failed(
                    "the connection closed before an answer".into(),
                ));
            }
        }
    }

    /// One line of the chunked framing, without its end.
    fn line(&mut self) -> Result<Vec<u8>, Fetch> {
        loop {
            if let Some(at) = self.held.windows(2).position(|two| two == b"\r\n") {
                let rest = self.held.split_off(at + 2);
                let mut line = std::mem::replace(&mut self.held, rest);
                line.truncate(at);
                return Ok(line);
            }
            if self.held.len() > MOST_LINE_BYTES {
                return Err(Fetch::Failed("a chunk's line is too long".into()));
            }
            if self.more()? == 0 {
                return Err(Fetch::Failed("the file ended inside its framing".into()));
            }
        }
    }

    /// A chunked body, into `out`.
    fn chunked(&mut self, most: u64, out: &mut dyn Write) -> Result<u64, Fetch> {
        let mut total = 0u64;
        loop {
            let line = self.line()?;
            let size = std::str::from_utf8(&line)
                .ok()
                .and_then(|line| line.split(';').next())
                .and_then(|size| u64::from_str_radix(size.trim(), 16).ok())
                .ok_or_else(|| Fetch::Failed("a chunk's size cannot be read".into()))?;
            if size == 0 {
                // The trailer, if any, to its empty line.
                while !self.line()?.is_empty() {}
                return Ok(total);
            }
            total = total.saturating_add(size);
            if total > most {
                return Err(too_large(most));
            }
            let mut left = size;
            while left > 0 {
                if self.held.is_empty() && self.more()? == 0 {
                    return Err(Fetch::Failed("the file ended inside a chunk".into()));
                }
                let take = (left as usize).min(self.held.len());
                out.write_all(&self.held[..take]).map_err(|error| {
                    Fetch::Failed(format!("the file cannot be written: {error}"))
                })?;
                self.held.drain(..take);
                left -= take as u64;
            }
            if !self.line()?.is_empty() {
                return Err(Fetch::Failed("a chunk is longer than it said".into()));
            }
        }
    }
}

impl Read for Held<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if !self.held.is_empty() {
            let take = buffer.len().min(self.held.len());
            buffer[..take].copy_from_slice(&self.held[..take]);
            self.held.drain(..take);
            return Ok(take);
        }
        self.stream.read(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    #[test]
    fn a_name_lookup_that_hangs_is_given_up_and_no_round_starts_another_until_it_ends() {
        static RUNNING: AtomicUsize = AtomicUsize::new(0);
        let (release, held) = mpsc::channel::<()>();
        let held = std::sync::Mutex::new(held);
        let started = std::time::Instant::now();
        // A resolver that never answers until released.
        let hung = lookup_within(
            "updates.example",
            443,
            Duration::from_millis(50),
            &RUNNING,
            1,
            move |_, _| {
                let _ = held.lock().unwrap().recv();
                Ok(Vec::new())
            },
        );
        assert!(hung.unwrap_err().contains("no answer within"));
        assert!(started.elapsed() < Duration::from_secs(2));
        // Rounds while it is still blocked start no new lookup.
        for _ in 0..8 {
            let refused = lookup_within(
                "updates.example",
                443,
                Duration::from_millis(50),
                &RUNNING,
                1,
                |_, _| {
                    panic!("no second lookup may start");
                },
            );
            assert!(
                refused
                    .unwrap_err()
                    .contains("earlier name lookup has not ended")
            );
        }
        assert_eq!(RUNNING.load(Ordering::Acquire), 1);
        // A lookup's thread is counted until it ends, which is a moment
        // after its answer has been received: what depends on it having
        // ended waits for that, within five seconds.
        let counted_out = || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while RUNNING.load(Ordering::Acquire) != 0 {
                assert!(
                    std::time::Instant::now() < deadline,
                    "the lookup never ended"
                );
                thread::sleep(Duration::from_millis(5));
            }
        };
        // Once it ends, the next round looks up again.
        release.send(()).unwrap();
        counted_out();
        let found = lookup_within(
            "updates.example",
            443,
            Duration::from_secs(2),
            &RUNNING,
            1,
            |_, port| Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))]),
        );
        assert_eq!(found.unwrap(), [SocketAddr::from(([127, 0, 0, 1], 443))]);
        counted_out();
        let refused = lookup_within(
            "updates.example",
            443,
            Duration::from_secs(2),
            &RUNNING,
            1,
            |_, _| Err(io::Error::other("no such name")),
        );
        assert!(refused.unwrap_err().contains("no such name"));
        counted_out();
        assert_eq!(RUNNING.load(Ordering::Acquire), 0);
    }

    /// A server that gives each connection the next answer, as written,
    /// and keeps the requests.
    fn serve(answers: Vec<Vec<u8>>) -> (u16, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for answer in answers {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                requests.push(String::from_utf8(request).unwrap());
                socket.write_all(&answer).unwrap();
            }
            requests
        });
        (port, handle)
    }

    /// The exchange over plain TCP to the local server, as if it were
    /// TLS: what is tested is HTTP.
    fn fetch_from(port: u16, url: &str, most: u64) -> (Result<u64, Fetch>, Vec<u8>) {
        let mut out = Vec::new();
        let until = Instant::now() + Duration::from_secs(10);
        let result = get(url, most, &mut out, until, |_| {
            let socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
            Ok(Box::new(socket) as Box<dyn Stream>)
        });
        (result, out)
    }

    #[test]
    fn a_file_by_its_length_after_a_redirect() {
        let (port, server) = serve(vec![
            b"HTTP/1.1 302 Found\r\nLocation: https://objects.example.com/file?x=1\r\nContent-Length: 0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello".to_vec(),
        ]);
        let (result, out) = fetch_from(port, "https://github.com/a/b/lamp-voice", 100);
        assert_eq!(result, Ok(5));
        assert_eq!(out, b"hello");
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /a/b/lamp-voice HTTP/1.1\r\nHost: github.com\r\n"));
        assert!(requests[1].starts_with("GET /file?x=1 HTTP/1.1\r\nHost: objects.example.com\r\n"));
    }

    #[test]
    fn a_chunked_file_and_one_to_the_end() {
        let (port, server) = serve(vec![
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nhel\r\n2;x=y\r\nlo\r\n0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\n\r\nto the end".to_vec(),
        ]);
        let (result, out) = fetch_from(port, "https://h/x", 100);
        assert_eq!((result, out.as_slice()), (Ok(5), b"hello".as_slice()));
        let (result, out) = fetch_from(port, "https://h/x", 100);
        assert_eq!((result, out.as_slice()), (Ok(10), b"to the end".as_slice()));
        server.join().unwrap();
    }

    #[test]
    fn missing_too_large_short_and_other_answers() {
        let (port, server) = serve(vec![
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10\r\n0123456789abcdef\r\n0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort".to_vec(),
            b"HTTP/1.1 500 Oops\r\nContent-Length: 0\r\n\r\n".to_vec(),
            b"HTTP/1.1 302 Found\r\nLocation: http://plain.example.com/\r\n\r\n".to_vec(),
        ]);
        assert_eq!(fetch_from(port, "https://h/x", 100).0, Err(Fetch::Missing));
        assert!(
            matches!(fetch_from(port, "https://h/x", 100).0, Err(Fetch::Failed(why)) if why.contains("larger"))
        );
        assert!(
            matches!(fetch_from(port, "https://h/x", 8).0, Err(Fetch::Failed(why)) if why.contains("larger"))
        );
        assert!(
            matches!(fetch_from(port, "https://h/x", 100).0, Err(Fetch::Failed(why)) if why.contains("ended after 5"))
        );
        assert!(
            matches!(fetch_from(port, "https://h/x", 100).0, Err(Fetch::Failed(why)) if why.contains("500"))
        );
        assert!(
            matches!(fetch_from(port, "https://h/x", 100).0, Err(Fetch::Failed(why)) if why.contains("not an https URL"))
        );
        server.join().unwrap();
    }

    #[test]
    fn urls_and_redirects() {
        let url = Url::parse("https://GitHub.com:8443/a/b").unwrap();
        assert_eq!(
            (url.host.as_str(), url.port, url.path.as_str()),
            ("github.com", 8443, "/a/b")
        );
        assert_eq!(url.follow("/c").unwrap().path, "/c");
        assert_eq!(url.follow("/c").unwrap().host, "github.com");
        assert!(url.follow("//evil/c").is_err());
        assert!(Url::parse("http://h/").is_err());
        assert!(Url::parse("https://user@h/").is_err());
    }

    #[test]
    fn a_folder_on_the_lamp() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("f"), b"abc").unwrap();
        let mut web = Web::new().unwrap();
        let mut out = Vec::new();
        let url = format!("file://{}/f", folder.path().display());
        assert_eq!(web.fetch(&url, 3, &mut out), Ok(3));
        assert_eq!(out, b"abc");
        assert!(web.fetch(&url, 2, &mut Vec::new()).is_err());
        let gone = format!("file://{}/g", folder.path().display());
        assert_eq!(web.fetch(&gone, 3, &mut Vec::new()), Err(Fetch::Missing));
    }

    #[test]
    fn no_network_is_unreachable() {
        // A port nobody listens on, on this machine.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let mut web = Web::new().unwrap();
        let url = format!("https://127.0.0.1:{port}/x");
        assert!(matches!(
            web.fetch(&url, 1, &mut Vec::new()),
            Err(Fetch::Unreachable(_))
        ));
    }

    /// The real path a lamp takes: GitHub's release download, its redirect
    /// and TLS, for a small file of this repository's own releases. Needs
    /// the network: `cargo test -p lamp-update -- --ignored`.
    #[test]
    #[ignore]
    fn a_release_file_from_github() {
        let mut web = Web::new().unwrap();
        let mut out = Vec::new();
        let url = "https://github.com/autonomous-ai/lampOS/releases/download/models/ambernet/ambernet.labels.json";
        assert_eq!(web.fetch(url, 4096, &mut out), Ok(646));
        assert!(serde_json::from_slice::<serde_json::Value>(&out).is_ok());
        let missing = "https://github.com/autonomous-ai/lampOS/releases/download/models/ambernet/no-such-file";
        assert_eq!(
            web.fetch(missing, 4096, &mut Vec::new()),
            Err(Fetch::Missing)
        );
    }

    /// Tests on #1112: once a caller has a lookup's answer, the lookup is no
    /// longer counted, so the next one (a redirect's) starts at once.
    #[test]
    fn an_answered_lookup_is_counted_out_before_its_answer_arrives() {
        static COUNTED: AtomicUsize = AtomicUsize::new(0);
        for round in 0..200 {
            let found = lookup_within(
                "updates.example",
                443,
                Duration::from_secs(5),
                &COUNTED,
                1,
                |_, port| Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))]),
            );
            assert!(found.is_ok(), "round {round}: {found:?}");
            assert_eq!(COUNTED.load(Ordering::Acquire), 0, "round {round}");
        }
    }
}
