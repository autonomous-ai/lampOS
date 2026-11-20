//! The one GET of a play: https to a host the cloud named, every address it
//! resolves to public, every redirect checked the same way, before any
//! connect. HTTP/1.1 is spoken here directly over rustls, as `lamp-update`
//! does: `Connection: close`, no cookies, no keep-alive, no compression, at
//! most [`MOST_REDIRECTS`] redirects.
//!
//! The name lookup and the connect are behind [`Resolve`] and [`Connect`],
//! so the checks and the response handling are tested with no network.
use crate::hosts::{Allowed, Refused, public_addresses};
use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use url::Url;

/// The most redirects followed.
pub const MOST_REDIRECTS: usize = 3;
/// The longest the answer's head may be, in bytes.
const MOST_HEAD: usize = 16 * 1024;
/// A name lookup is given this long.
pub const RESOLVE_WITHIN: Duration = Duration::from_secs(5);
/// A connect (TCP, then TLS) is given this long.
pub const CONNECT_WITHIN: Duration = Duration::from_secs(5);
/// A read or a write that moves nothing for this long ends the play.
pub const STALL_WITHIN: Duration = Duration::from_secs(10);

/// Why a fetch did not give a body. The words the app is told are
/// [`FetchError::word`]; nothing of the URL is in any of them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FetchError {
    /// Checked before a connect and refused.
    Refused(Refused),
    /// No lookup, no connect or no TLS.
    Unreachable,
    /// A stall or a deadline.
    Timeout,
    /// An answer that is not a 200 with audio.
    NotAudio,
    /// A redirect loop or past [`MOST_REDIRECTS`].
    TooManyRedirects,
    /// A body announced larger than the cap.
    TooBig,
}

impl FetchError {
    /// The fixed word for the app and the journal.
    pub fn word(self) -> &'static str {
        match self {
            FetchError::Refused(Refused::Host | Refused::Address | Refused::Literal) => {
                "refused_host"
            }
            FetchError::Refused(_) => "refused_host",
            FetchError::Unreachable => "unreachable",
            FetchError::Timeout => "timeout",
            FetchError::NotAudio => "bad_audio",
            FetchError::TooManyRedirects => "refused_host",
            FetchError::TooBig => "too_big",
        }
    }
}

/// A connected, readable and writable byte stream.
pub trait Wire: Read + Write + Send {}
impl<T: Read + Write + Send> Wire for T {}

/// The name lookup: every address a name resolves to.
pub trait Resolve {
    fn resolve(&mut self, host: &str) -> io::Result<Vec<IpAddr>>;
}

/// The connect: TCP to `address` and, for the real one, TLS for `host`.
pub trait Connect {
    fn connect(&mut self, host: &str, address: SocketAddrV4) -> io::Result<Box<dyn Wire>>;
}

/// The lookup and the connect together: what one fetch is given.
pub trait Net: Resolve + Connect {}
impl<T: Resolve + Connect> Net for T {}

/// A body: what the server sends after its head, read as it arrives.
pub struct Body {
    inner: Box<dyn Read + Send>,
}

impl Read for Body {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buffer)
    }
}

/// A `Read + Send + Sync` for the decoder (which wants `Sync`): the same
/// reader behind a lock that is only ever taken by the one thread reading.
pub struct SyncBody(Mutex<Body>);

impl SyncBody {
    pub fn new(body: Body) -> Self {
        Self(Mutex::new(body))
    }
}

impl Read for SyncBody {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.0.get_mut() {
            Ok(body) => body.read(buffer),
            Err(poisoned) => poisoned.into_inner().read(buffer),
        }
    }
}

/// `url` fetched: its body, if every step was allowed. `most` is the byte
/// cap, and a `Content-Length` over it ends the fetch at once.
pub fn get(allowed: &Allowed, url: &str, most: u64, net: &mut dyn Net) -> Result<Body, FetchError> {
    let mut url = url.to_owned();
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..=MOST_REDIRECTS {
        // Everything is checked before a name is looked up or a socket made.
        let target = allowed.check(&url).map_err(FetchError::Refused)?;
        if seen.contains(&url) {
            return Err(FetchError::TooManyRedirects);
        }
        seen.push(url.clone());
        let answer = net
            .resolve(&target.host)
            .map_err(|error| lookup_error(&error))?;
        let addresses = public_addresses(&answer).map_err(FetchError::Refused)?;
        let mut wire = connect_any(net, &target.host, &addresses)?;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nAccept: audio/mpeg\r\nAccept-Encoding: identity\r\n\
             User-Agent: lamp-play\r\nConnection: close\r\n\r\n",
            target.request, target.host
        );
        wire.write_all(request.as_bytes())
            .map_err(|error| io_error(&error))?;
        let head = read_head(&mut wire)?;
        match head.status {
            200 => {
                head.audio_type()?;
                if head.length.is_some_and(|length| length > most) {
                    return Err(FetchError::TooBig);
                }
                return Ok(head.into_body(wire));
            }
            301 | 302 | 303 | 307 | 308 => {
                let location = head.location.ok_or(FetchError::NotAudio)?;
                // Relative to the URL that answered; the next turn checks it.
                let base = Url::parse(&url).map_err(|_| FetchError::NotAudio)?;
                url = base
                    .join(&location)
                    .map_err(|_| FetchError::Refused(Refused::Malformed))?
                    .to_string();
            }
            _ => return Err(FetchError::NotAudio),
        }
    }
    Err(FetchError::TooManyRedirects)
}

fn connect_any(
    connector: &mut dyn Net,
    host: &str,
    addresses: &[Ipv4Addr],
) -> Result<Box<dyn Wire>, FetchError> {
    let mut failure = FetchError::Unreachable;
    for address in addresses.iter().take(4) {
        match connector.connect(host, SocketAddrV4::new(*address, 443)) {
            Ok(wire) => return Ok(wire),
            Err(error) => failure = io_error(&error),
        }
    }
    Err(failure)
}

fn lookup_error(error: &io::Error) -> FetchError {
    if error.kind() == io::ErrorKind::TimedOut {
        FetchError::Timeout
    } else {
        FetchError::Unreachable
    }
}

fn io_error(error: &io::Error) -> FetchError {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => FetchError::Timeout,
        _ => FetchError::Unreachable,
    }
}

/// An answer's head, as far as a play needs it.
struct Head {
    status: u16,
    location: Option<String>,
    content_type: Option<String>,
    length: Option<u64>,
    chunked: bool,
    /// The body's first bytes, read with the head.
    rest: Vec<u8>,
}

impl Head {
    /// A 200 that is audio: MP3's types, or a binary type, or none.
    fn audio_type(&self) -> Result<(), FetchError> {
        let Some(kind) = &self.content_type else {
            return Ok(());
        };
        let kind = kind
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        match kind.as_str() {
            "audio/mpeg"
            | "audio/mp3"
            | "audio/x-mpeg"
            | "application/octet-stream"
            | "binary/octet-stream" => Ok(()),
            _ => Err(FetchError::NotAudio),
        }
    }

    fn into_body(self, wire: Box<dyn Wire>) -> Body {
        let rest = io::Cursor::new(self.rest);
        let joined = rest.chain(wire);
        let inner: Box<dyn Read + Send> = if self.chunked {
            Box::new(Chunked::new(joined))
        } else if let Some(length) = self.length {
            Box::new(joined.take(length))
        } else {
            Box::new(joined)
        };
        Body { inner }
    }
}

fn read_head(wire: &mut Box<dyn Wire>) -> Result<Head, FetchError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let count = wire.read(&mut chunk).map_err(|error| io_error(&error))?;
        if count == 0 {
            return Err(FetchError::Unreachable);
        }
        buffer.extend_from_slice(&chunk[..count]);
        let mut headers = [httparse::EMPTY_HEADER; 48];
        let mut response = httparse::Response::new(&mut headers);
        match response.parse(&buffer) {
            Ok(httparse::Status::Complete(end)) => {
                let mut head = Head {
                    status: response.code.unwrap_or(0),
                    location: None,
                    content_type: None,
                    length: None,
                    chunked: false,
                    rest: buffer[end..].to_vec(),
                };
                for header in response.headers.iter() {
                    let value = String::from_utf8_lossy(header.value).trim().to_owned();
                    match header.name.to_ascii_lowercase().as_str() {
                        "location" => head.location = Some(value),
                        "content-type" => head.content_type = Some(value),
                        "content-length" => head.length = value.parse().ok(),
                        "transfer-encoding" => {
                            head.chunked = value.to_ascii_lowercase().contains("chunked");
                        }
                        // A compressed body is never asked for and not read.
                        "content-encoding" if !value.eq_ignore_ascii_case("identity") => {
                            return Err(FetchError::NotAudio);
                        }
                        _ => {}
                    }
                }
                return Ok(head);
            }
            Ok(httparse::Status::Partial) if buffer.len() < MOST_HEAD => {}
            _ => return Err(FetchError::NotAudio),
        }
    }
}

/// A chunked body, decoded as it is read.
struct Chunked<R> {
    inner: R,
    /// Bytes left in the chunk now being read.
    left: u64,
    /// Whether the last chunk (size 0) was read.
    done: bool,
    /// Whether the CRLF after a chunk's data is still owed.
    owed: bool,
}

impl<R: Read> Chunked<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            left: 0,
            done: false,
            owed: false,
        }
    }

    fn byte(&mut self) -> io::Result<u8> {
        let mut one = [0u8; 1];
        match self.inner.read(&mut one)? {
            0 => Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
            _ => Ok(one[0]),
        }
    }

    /// The size line of the next chunk.
    fn size(&mut self) -> io::Result<u64> {
        let mut line = Vec::new();
        loop {
            let byte = self.byte()?;
            if byte == b'\n' {
                break;
            }
            if byte != b'\r' {
                line.push(byte);
            }
            if line.len() > 64 {
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
        }
        let text = String::from_utf8_lossy(&line);
        let hex = text.split(';').next().unwrap_or("").trim();
        u64::from_str_radix(hex, 16).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
    }
}

impl<R: Read> Read for Chunked<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.done || buffer.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            if self.owed {
                self.byte()?;
                self.byte()?;
                self.owed = false;
            }
            self.left = self.size()?;
            if self.left == 0 {
                self.done = true;
                return Ok(0);
            }
        }
        let take = buffer
            .len()
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let count = self.inner.read(&mut buffer[..take])?;
        if count == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        self.left -= count as u64;
        if self.left == 0 {
            self.owed = true;
        }
        Ok(count)
    }
}

/// The real network: the system's lookup and the TLS connect.
#[derive(Default)]
pub struct SystemNet {
    resolver: SystemResolver,
    connector: TlsConnector,
}

impl Resolve for SystemNet {
    fn resolve(&mut self, host: &str) -> io::Result<Vec<IpAddr>> {
        self.resolver.resolve(host)
    }
}

impl Connect for SystemNet {
    fn connect(&mut self, host: &str, address: SocketAddrV4) -> io::Result<Box<dyn Wire>> {
        self.connector.connect(host, address)
    }
}

/// The real lookup: the system's, on a thread so a stuck resolver is given
/// up on after [`RESOLVE_WITHIN`].
#[derive(Default)]
pub struct SystemResolver;

impl Resolve for SystemResolver {
    fn resolve(&mut self, host: &str) -> io::Result<Vec<IpAddr>> {
        let (send, receive) = mpsc::channel();
        let name = host.to_owned();
        std::thread::spawn(move || {
            let found = (name.as_str(), 443u16)
                .to_socket_addrs()
                .map(|found| found.map(|address| address.ip()).collect::<Vec<_>>());
            let _ = send.send(found);
        });
        receive
            .recv_timeout(RESOLVE_WITHIN)
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    }
}

/// The real connect: TCP to the checked address, then TLS for `host` against
/// the web's roots, each read and write bounded by [`STALL_WITHIN`].
pub struct TlsConnector {
    config: Arc<rustls::ClientConfig>,
}

impl TlsConnector {
    pub fn new() -> Self {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the ring provider has the default protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        Self {
            config: Arc::new(config),
        }
    }
}

impl Default for TlsConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl Connect for TlsConnector {
    fn connect(&mut self, host: &str, address: SocketAddrV4) -> io::Result<Box<dyn Wire>> {
        let tcp = TcpStream::connect_timeout(&SocketAddr::V4(address), CONNECT_WITHIN)?;
        tcp.set_read_timeout(Some(STALL_WITHIN))?;
        tcp.set_write_timeout(Some(STALL_WITHIN))?;
        tcp.set_nodelay(true)?;
        let name = rustls::pki_types::ServerName::try_from(host.to_owned())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let connection = rustls::ClientConnection::new(Arc::clone(&self.config), name)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(Box::new(rustls::StreamOwned::new(connection, tcp)))
    }
}

#[cfg(test)]
mod tests;
