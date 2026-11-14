//! One posture request (#6543 phase 3), V1's pose wire (`pose.py` on
//! `main`), on a socket of its own that is closed after it:
//!
//! 1. connect to `<base>/ws/hal/api/dl/pose-estimation/ws` with
//!    `X-API-Key`, `<base>` V1's base: the account's `llm_base_url` ending
//!    in exactly one `/v1` ([`crate::endpoint::v1_base`], as activity's);
//! 2. the key exchange ([`crate::crypto`]): no plaintext path;
//! 3. V1's pose config, its values exactly ([`PoseConfig::v1`]): the
//!    server's sessions share one config object, so another value would
//!    change every V1 lamp's results;
//! 4. one frame, its answer read (`ergo`, or `{"error":"busy"}`);
//! 5. the socket closed, so the server drops the session and whatever it
//!    kept of the frame.
//!
//! Every wait is bounded ([`crate::wire::ANSWER_WITHIN_S`]), and a cancel
//! (the gate closed) is seen before each connect and within [`POLL`] of
//! every read: the socket is closed, and a frame not yet sent is never
//! sent. A connect or write already begun runs to its own 15 s bound.
//!
//! The server's key is `<base>/crypto/public-key` (V1's
//! `DL_PUBLIC_KEY_URL`), raw PEM, or a local file (V1's
//! `DL_PUBLIC_KEY_FILE`). The account key goes only to the company relay
//! over TLS, or to this computer (the simulator and the tests) in the
//! clear: [`Address::parse`].
use crate::crypto::{self, Session, public_key_from_pem};
use crate::wire::{ANSWER_WITHIN_S, Ergo, PoseAnswer, Task};
use rsa::RsaPublicKey;
use serde::Serialize;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tungstenite::{
    Message, WebSocket,
    client::IntoClientRequest,
    handshake::HandshakeError,
    http::{HeaderValue, StatusCode},
};
use zeroize::Zeroizing;

/// V1's pose path (`DL_POSE_ENDPOINT`).
pub const POSE_PATH: &str = "/ws/hal/api/dl/pose-estimation/ws";
/// V1's public key path, under the same base.
pub const PUBLIC_KEY_PATH: &str = crate::endpoint::PUBLIC_KEY_PATH;
/// The only host the account key goes to over the network
/// (`hermes_llm::relay_base`'s rule).
pub const RELAY_HOST: &str = crate::endpoint::COMPANY_RELAY_HOST;
/// How often a wait looks at the cancel.
pub const POLL: Duration = Duration::from_millis(100);
/// V1's public key fetch timeout.
pub const PUBLIC_KEY_WITHIN: Duration = Duration::from_secs(5);
/// The longest public key answer read.
const PUBLIC_KEY_MOST_BYTES: usize = 64 * 1024;

/// V1's pose config, the server's own defaults (V1 sent none:
/// `PS/core/models/pose.py` `PosePerceptionSessionConfig`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PoseConfig {
    #[serde(rename = "type")]
    kind: &'static str,
    task: Task,
    pub frame_interval: f64,
    pub confidence_threshold_2d: f64,
    pub min_valid_keypoints: u32,
}

impl PoseConfig {
    pub fn v1() -> Self {
        Self {
            kind: "config",
            task: Task::Pose,
            frame_interval: 1.0,
            confidence_threshold_2d: 0.3,
            min_valid_keypoints: 5,
        }
    }
}

/// The frame message, borrowing the frame's Base64 so it is wiped by its
/// owner.
#[derive(Serialize)]
struct FrameMessage<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    task: Task,
    frame_b64: &'a str,
}

/// Where the service is: `scheme://host:port/prefix`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    /// The base's path, no trailing `/`, ending in `/v1` (`/api/v1/ai/v1`).
    pub prefix: String,
}

impl Address {
    /// The account's base, on V1's `/v1` ([`crate::endpoint::v1_base`]).
    /// `https` only to [`RELAY_HOST`]; `http` only to this computer
    /// (`127.0.0.1`, `localhost`, `[::1]`).
    pub fn parse(base: &str) -> Result<Self, String> {
        let base = crate::endpoint::v1_base(base);
        let base = base.as_str();
        let (tls, rest) = if let Some(rest) = base.strip_prefix("https://") {
            (true, rest)
        } else if let Some(rest) = base.strip_prefix("http://") {
            (false, rest)
        } else {
            return Err("the base is not http(s)".into());
        };
        let (authority, prefix) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        if authority.contains('@') || prefix.contains(['?', '#']) {
            return Err("the base has a login, a query or a fragment".into());
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !host.ends_with(':') && !port.contains(']') => (
                host,
                port.parse::<u16>()
                    .map_err(|_| "the base's port".to_owned())?,
            ),
            _ => (authority, if tls { 443 } else { 80 }),
        };
        let host = host.to_ascii_lowercase();
        let allowed = if tls {
            host == RELAY_HOST
        } else {
            matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]")
        };
        if !allowed {
            return Err(format!(
                "the key goes only to {RELAY_HOST} over https, or to this computer"
            ));
        }
        Ok(Self {
            tls,
            host,
            port,
            prefix: prefix.to_owned(),
        })
    }

    fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// The pose socket's URL.
    pub fn pose_url(&self) -> String {
        let scheme = if self.tls { "wss" } else { "ws" };
        format!("{scheme}://{}{}{POSE_PATH}", self.authority(), self.prefix)
    }
}

/// Why a request failed, for the backoff and `diagnostics`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// No connection (DNS, TCP, TLS).
    Connect,
    /// The socket's handshake refused, with the HTTP status if one came.
    Handshake(Option<u16>),
    /// No server key, from the file or the address.
    PublicKey,
    /// The key exchange was not accepted.
    KeyExchange,
    /// No answer within [`ANSWER_WITHIN_S`].
    Timeout,
    /// The socket closed before the answer.
    Closed,
    /// An answer not V1's shape, not decrypting, or the server's own error
    /// (`Processing failed`, a refused message).
    Server,
}

impl Failure {
    pub fn as_str(self) -> &'static str {
        match self {
            Failure::Connect => "connect",
            Failure::Handshake(Some(status)) if status >= 500 => "handshake_5xx",
            Failure::Handshake(_) => "handshake",
            Failure::PublicKey => "public_key",
            Failure::KeyExchange => "key_exchange",
            Failure::Timeout => "timeout",
            Failure::Closed => "closed",
            Failure::Server => "server",
        }
    }
}

/// How one request ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Answered: the score, or none when no body was found.
    Answered(Option<Ergo>),
    /// `{"error":"busy"}`: the frame was dropped by the server.
    Busy,
    Failed(Failure),
    /// The gate closed while it ran: closed at once; whether the frame had
    /// gone.
    Cancelled {
        frame_sent: bool,
    },
}

/// The service: its address, the account key, and the server's key once
/// known.
pub struct Client {
    address: Address,
    api_key: Zeroizing<String>,
    public_key_file: Option<PathBuf>,
    public_key: Mutex<Option<RsaPublicKey>>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

/// The stream under the socket: in the clear to this computer, TLS to the
/// relay.
enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(stream) => stream.read(buf),
            Stream::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(stream) => stream.write(buf),
            Stream::Tls(stream) => stream.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Plain(stream) => stream.flush(),
            Stream::Tls(stream) => stream.flush(),
        }
    }
}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, Failure> {
    static CONFIG: OnceLock<Option<Arc<rustls::ClientConfig>>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let roots = rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .ok()
            .map(|builder| Arc::new(builder.with_root_certificates(roots).with_no_client_auth()))
        })
        .clone()
        .ok_or(Failure::Connect)
}

fn would_block(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

/// Why a wait ended without its answer.
enum Stop {
    Cancelled,
    Failed(Failure),
}

fn waiting(cancel: &AtomicBool, deadline: Instant, failure: Failure) -> Result<(), Stop> {
    if cancel.load(Ordering::Relaxed) {
        Err(Stop::Cancelled)
    } else if Instant::now() >= deadline {
        Err(Stop::Failed(failure))
    } else {
        Ok(())
    }
}

impl Client {
    pub fn new(
        address: Address,
        api_key: Zeroizing<String>,
        public_key_file: Option<PathBuf>,
    ) -> Self {
        Self {
            address,
            api_key,
            public_key_file,
            public_key: Mutex::new(None),
        }
    }

    /// For activity's checked endpoint ([`crate::endpoint::Endpoint`]): the
    /// same base, key and server key (if given ahead), on the pose path.
    pub fn for_endpoint(endpoint: &crate::endpoint::Endpoint) -> Result<Self, String> {
        let base = endpoint
            .public_key_url
            .strip_suffix(crate::endpoint::PUBLIC_KEY_PATH)
            .ok_or("the endpoint's base")?;
        let client = Self::new(Address::parse(base)?, endpoint.api_key.clone(), None);
        Ok(match &endpoint.public_key {
            Some(key) => client.with_public_key(key.clone()),
            None => client,
        })
    }

    /// With the server's key already known (the tests).
    pub fn with_public_key(mut self, key: RsaPublicKey) -> Self {
        self.public_key = Mutex::new(Some(key));
        self
    }

    pub fn address(&self) -> &Address {
        &self.address
    }

    /// The stream to the service. A cancel seen after the name is resolved,
    /// before each connect, returns without connecting; a connect begun
    /// still runs to its own 15 s bound.
    fn open(&self, cancel: &AtomicBool) -> Result<Stream, Stop> {
        let addresses = (
            self.address.host.trim_matches(['[', ']']),
            self.address.port,
        )
            .to_socket_addrs()
            .map_err(|_| Stop::Failed(Failure::Connect))?;
        let mut tcp = None;
        for address in addresses {
            if cancel.load(Ordering::Relaxed) {
                return Err(Stop::Cancelled);
            }
            if let Ok(stream) =
                TcpStream::connect_timeout(&address, Duration::from_secs(ANSWER_WITHIN_S))
            {
                tcp = Some(stream);
                break;
            }
        }
        let tcp = tcp.ok_or(Stop::Failed(Failure::Connect))?;
        tcp.set_read_timeout(Some(POLL))
            .and_then(|()| tcp.set_write_timeout(Some(Duration::from_secs(ANSWER_WITHIN_S))))
            .and_then(|()| tcp.set_nodelay(true))
            .map_err(|_| Stop::Failed(Failure::Connect))?;
        if !self.address.tls {
            return Ok(Stream::Plain(tcp));
        }
        let name = rustls::pki_types::ServerName::try_from(self.address.host.clone())
            .map_err(|_| Stop::Failed(Failure::Connect))?;
        let connection = rustls::ClientConnection::new(tls_config().map_err(Stop::Failed)?, name)
            .map_err(|_| Stop::Failed(Failure::Connect))?;
        Ok(Stream::Tls(Box::new(rustls::StreamOwned::new(
            connection, tcp,
        ))))
    }

    /// The server's key: kept once known, else the file, else fetched.
    fn public_key(&self, cancel: &AtomicBool) -> Result<RsaPublicKey, Stop> {
        if let Some(key) = self.public_key.lock().ok().and_then(|kept| kept.clone()) {
            return Ok(key);
        }
        let pem = match &self.public_key_file {
            Some(path) => {
                std::fs::read_to_string(path).map_err(|_| Stop::Failed(Failure::PublicKey))?
            }
            None => self.fetch_public_key(cancel)?,
        };
        let key = public_key_from_pem(&pem).map_err(|_| Stop::Failed(Failure::PublicKey))?;
        if let Ok(mut kept) = self.public_key.lock() {
            *kept = Some(key.clone());
        }
        Ok(key)
    }

    /// Forget the server's key: the next request reads it again.
    fn forget_public_key(&self) {
        if let Ok(mut kept) = self.public_key.lock() {
            *kept = None;
        }
    }

    /// `GET <base>/crypto/public-key` with `X-API-Key`, as V1's
    /// `fetch_public_key`: a 200's body is the PEM.
    fn fetch_public_key(&self, cancel: &AtomicBool) -> Result<String, Stop> {
        let deadline = Instant::now() + PUBLIC_KEY_WITHIN;
        let mut stream = self.open(cancel)?;
        let head = Zeroizing::new(format!(
            "GET {}{PUBLIC_KEY_PATH} HTTP/1.1\r\nHost: {}\r\nX-API-Key: {}\r\nAccept: */*\r\nUser-Agent: lamp-seen\r\nConnection: close\r\n\r\n",
            self.address.prefix,
            self.address.host,
            self.api_key.as_str()
        ));
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.flush())
            .map_err(|_| Stop::Failed(Failure::Connect))?;
        let mut answer = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    answer.extend_from_slice(&buffer[..read]);
                    if answer.len() > PUBLIC_KEY_MOST_BYTES {
                        return Err(Stop::Failed(Failure::PublicKey));
                    }
                }
                Err(error) if would_block(&error) => {
                    waiting(cancel, deadline, Failure::Timeout)?;
                }
                // A TLS peer that closes without its close_notify.
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(_) => return Err(Stop::Failed(Failure::Connect)),
            }
        }
        let (status, body) = http_body(&answer).ok_or(Stop::Failed(Failure::PublicKey))?;
        match status {
            200 => String::from_utf8(body).map_err(|_| Stop::Failed(Failure::PublicKey)),
            500.. => Err(Stop::Failed(Failure::Handshake(Some(status)))),
            _ => Err(Stop::Failed(Failure::PublicKey)),
        }
    }

    /// One request: the socket opened, the key exchange, V1's config, one
    /// frame (`frame_b64`, the JPEG in standard Base64), its answer, and the
    /// socket closed. `cancel` stops it within [`POLL`].
    pub fn request(&self, frame_b64: &str, cancel: &AtomicBool) -> Outcome {
        let key = match self.public_key(cancel) {
            Ok(key) => key,
            Err(Stop::Cancelled) => return Outcome::Cancelled { frame_sent: false },
            Err(Stop::Failed(failure)) => return Outcome::Failed(failure),
        };
        let mut frame_sent = false;
        let outcome = match self.exchange(&key, frame_b64, cancel, &mut frame_sent) {
            Ok(outcome) => outcome,
            Err(Stop::Cancelled) => Outcome::Cancelled { frame_sent },
            Err(Stop::Failed(failure)) => Outcome::Failed(failure),
        };
        if outcome == Outcome::Failed(Failure::KeyExchange) {
            // The server's key may have changed: read it again next time.
            self.forget_public_key();
        }
        outcome
    }

    fn exchange(
        &self,
        key: &RsaPublicKey,
        frame_b64: &str,
        cancel: &AtomicBool,
        frame_sent: &mut bool,
    ) -> Result<Outcome, Stop> {
        let deadline = Instant::now() + Duration::from_secs(ANSWER_WITHIN_S);
        waiting(cancel, deadline, Failure::Timeout)?;
        let stream = self.open(cancel)?;
        let mut request = self
            .address
            .pose_url()
            .into_client_request()
            .map_err(|_| Stop::Failed(Failure::Connect))?;
        let mut header = HeaderValue::from_str(&self.api_key)
            .map_err(|_| Stop::Failed(Failure::Handshake(None)))?;
        header.set_sensitive(true);
        request.headers_mut().insert("X-API-Key", header);
        let mut handshake = tungstenite::client(request, stream);
        let mut socket = loop {
            match handshake {
                Ok((socket, _)) => break Socket(socket),
                Err(HandshakeError::Interrupted(mid)) => {
                    waiting(cancel, deadline, Failure::Timeout)?;
                    handshake = mid.handshake();
                }
                Err(HandshakeError::Failure(tungstenite::Error::Http(response))) => {
                    return Err(Stop::Failed(Failure::Handshake(Some(
                        response.status().as_u16(),
                    ))));
                }
                Err(HandshakeError::Failure(tungstenite::Error::Io(error)))
                    if !would_block(&error) =>
                {
                    return Err(Stop::Failed(Failure::Connect));
                }
                Err(HandshakeError::Failure(_)) => {
                    return Err(Stop::Failed(Failure::Handshake(None)));
                }
            }
        };
        let result = socket.converse(key, frame_b64, cancel, deadline, frame_sent);
        socket.close();
        result
    }
}

/// The open socket; [`Socket::close`] ends it.
struct Socket(WebSocket<Stream>);

impl Socket {
    fn send(&mut self, text: String) -> Result<(), Stop> {
        self.0
            .send(Message::text(text))
            .map_err(|_| Stop::Failed(Failure::Closed))
    }

    /// The next text message within `deadline`.
    fn text(&mut self, cancel: &AtomicBool, deadline: Instant) -> Result<Zeroizing<String>, Stop> {
        loop {
            match self.0.read() {
                Ok(Message::Text(text)) => return Ok(Zeroizing::new(text.as_str().to_owned())),
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
                Ok(Message::Binary(_)) => return Err(Stop::Failed(Failure::Server)),
                Ok(Message::Close(_)) => return Err(Stop::Failed(Failure::Closed)),
                Err(tungstenite::Error::Io(error)) if would_block(&error) => {
                    waiting(cancel, deadline, Failure::Timeout)?;
                }
                Err(_) => return Err(Stop::Failed(Failure::Closed)),
            }
        }
    }

    fn converse(
        &mut self,
        key: &RsaPublicKey,
        frame_b64: &str,
        cancel: &AtomicBool,
        deadline: Instant,
        frame_sent: &mut bool,
    ) -> Result<Outcome, Stop> {
        let session =
            Session::with_system_random(key).map_err(|_| Stop::Failed(Failure::KeyExchange))?;
        self.send(session.key_exchange())?;
        if !crypto::key_exchange_ok(&self.text(cancel, deadline)?) {
            return Err(Stop::Failed(Failure::KeyExchange));
        }
        let wrap = |plain: &str| {
            session
                .wrap_ws_message(plain, &mut rand_core::OsRng)
                .map_err(|_| Stop::Failed(Failure::KeyExchange))
        };
        let open = |raw: &str| {
            session
                .unwrap_ws_message(raw)
                .map(Zeroizing::new)
                .map_err(|_| Stop::Failed(Failure::Server))
        };
        let config =
            serde_json::to_string(&PoseConfig::v1()).map_err(|_| Stop::Failed(Failure::Server))?;
        self.send(wrap(&config)?)?;
        let said = open(&self.text(cancel, deadline)?)?;
        let status = serde_json::from_str::<serde_json::Value>(&said)
            .ok()
            .and_then(|value| value.get("status")?.as_str().map(str::to_owned));
        if status.as_deref() != Some("config_updated") {
            return Err(Stop::Failed(Failure::Server));
        }
        // The last look before the frame goes: a gate closed by now sends none.
        waiting(cancel, deadline, Failure::Timeout)?;
        let plain = Zeroizing::new(
            serde_json::to_string(&FrameMessage {
                kind: "frame",
                task: Task::Pose,
                frame_b64,
            })
            .map_err(|_| Stop::Failed(Failure::Server))?,
        );
        let wrapped = wrap(&plain)?;
        drop(plain);
        *frame_sent = true;
        self.send(wrapped)?;
        let answer = open(&self.text(cancel, deadline)?)?;
        let answer: PoseAnswer =
            serde_json::from_str(&answer).map_err(|_| Stop::Failed(Failure::Server))?;
        Ok(match answer.error.as_deref() {
            Some("busy") => Outcome::Busy,
            Some(_) => Outcome::Failed(Failure::Server),
            None => Outcome::Answered(answer.ergo),
        })
    }

    /// Close the socket: the close frame, then the TCP connection, waiting
    /// at most a second for the server's close.
    fn close(mut self) {
        let _ = self.0.close(None);
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until {
            match self.0.read() {
                Ok(_) => {}
                Err(tungstenite::Error::Io(error)) if would_block(&error) => {
                    let _ = self.0.flush();
                }
                Err(_) => break,
            }
        }
        if let Stream::Plain(tcp) = self.0.get_ref() {
            let _ = tcp.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// An HTTP/1.1 answer's status and body, the body de-chunked when sent so.
fn http_body(answer: &[u8]) -> Option<(u16, Vec<u8>)> {
    let end = answer.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&answer[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse::<u16>()
        .ok()
        .filter(|status| StatusCode::from_u16(*status).is_ok())?;
    let chunked = lines.any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.trim().eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
        })
    });
    let body = &answer[end + 4..];
    if !chunked {
        return Some((status, body.to_vec()));
    }
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let line_end = rest.windows(2).position(|window| window == b"\r\n")?;
        let size = std::str::from_utf8(&rest[..line_end]).ok()?;
        let size = usize::from_str_radix(size.split(';').next()?.trim(), 16).ok()?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Some((status, out));
        }
        out.extend_from_slice(rest.get(..size)?);
        rest = rest.get(size + 2..)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_goes_only_to_the_relay_or_this_computer() {
        // Exactly one /v1: added when missing, never doubled.
        for base in ["/ai", "/ai/", "/ai/v1", "/ai/v1/"] {
            let relay = Address::parse(&format!("https://Campaign-API.autonomous.ai/api/v1{base}"))
                .unwrap();
            assert_eq!(relay.prefix, "/api/v1/ai/v1", "{base}");
            assert_eq!(
                relay.pose_url(),
                "wss://campaign-api.autonomous.ai:443/api/v1/ai/v1/ws/hal/api/dl/pose-estimation/ws",
                "{base}"
            );
        }
        let local = Address::parse("http://127.0.0.1:8123").unwrap();
        assert_eq!(
            local.pose_url(),
            "ws://127.0.0.1:8123/v1/ws/hal/api/dl/pose-estimation/ws"
        );
        assert!(Address::parse("http://[::1]:9/x").is_ok());
        for refused in [
            "http://campaign-api.autonomous.ai/api/v1/ai",
            "https://example.com/api/v1/ai",
            "https://campaign-api.autonomous.ai.example.com/",
            "https://user@campaign-api.autonomous.ai/",
            "https://campaign-api.autonomous.ai/x?y=1",
            "http://10.0.0.2:80/",
            "ws://127.0.0.1:1/",
            "",
            "http://127.0.0.1:port/",
        ] {
            assert!(Address::parse(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn activitys_endpoint_gives_the_pose_path_on_the_same_base() {
        let endpoint =
            crate::endpoint::Endpoint::new("https://campaign-api.autonomous.ai/api/v1/ai", "k")
                .unwrap();
        let client = Client::for_endpoint(&endpoint).unwrap();
        assert_eq!(
            client.address().pose_url(),
            "wss://campaign-api.autonomous.ai:443/api/v1/ai/v1/ws/hal/api/dl/pose-estimation/ws"
        );
        assert_eq!(client.api_key.as_str(), "k");
    }

    #[test]
    fn the_config_is_v1s_server_defaults() {
        assert_eq!(
            serde_json::to_string(&PoseConfig::v1()).unwrap(),
            r#"{"type":"config","task":"pose","frame_interval":1.0,"confidence_threshold_2d":0.3,"min_valid_keypoints":5}"#
        );
    }

    const PEM: &str = include_str!("../../tests/fixtures/test-server-public.pem");

    #[test]
    fn the_servers_key_is_fetched_with_the_account_key_and_kept() {
        let mut chunked =
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n"
                .to_vec();
        chunked.extend(format!("{:x}\r\n{PEM}\r\n0\r\n\r\n", PEM.len()).as_bytes());
        let (address, heads) = super::super::mock::public_key_server(chunked);
        let base = format!("http://127.0.0.1:{}/api/v1/ai", address.port());
        let client = Client::new(
            Address::parse(&base).unwrap(),
            Zeroizing::new("account-key".into()),
            None,
        );
        let cancel = AtomicBool::new(false);
        let key = client.public_key(&cancel).ok().unwrap();
        assert_eq!(key, super::super::mock::public());
        let head = heads.lock().unwrap()[0].clone();
        assert!(
            head.starts_with("GET /api/v1/ai/v1/crypto/public-key HTTP/1.1\r\n"),
            "{head}"
        );
        assert!(head.contains("\r\nX-API-Key: account-key\r\n"), "{head}");
        // Kept: no second fetch.
        client.public_key(&cancel).ok().unwrap();
        assert_eq!(heads.lock().unwrap().len(), 1);

        let (address, _) = super::super::mock::public_key_server(
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n".to_vec(),
        );
        let base = format!("http://127.0.0.1:{}", address.port());
        let client = Client::new(
            Address::parse(&base).unwrap(),
            Zeroizing::new("account-key".into()),
            None,
        );
        assert_eq!(
            client.request("AAAA", &cancel),
            Outcome::Failed(Failure::Handshake(Some(503)))
        );
        let (address, _) =
            super::super::mock::public_key_server(b"HTTP/1.1 200 OK\r\n\r\nnot a key".to_vec());
        let base = format!("http://127.0.0.1:{}", address.port());
        let client = Client::new(
            Address::parse(&base).unwrap(),
            Zeroizing::new("account-key".into()),
            None,
        );
        assert_eq!(
            client.request("AAAA", &cancel),
            Outcome::Failed(Failure::PublicKey)
        );
    }

    #[test]
    fn a_cancel_before_connecting_connects_nothing() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let cancel = AtomicBool::new(true);
        // The key not known yet: the fetch's open sees the cancel first.
        let fetching = Client::new(
            Address::parse(&base).unwrap(),
            Zeroizing::new("account-key".into()),
            None,
        );
        assert_eq!(
            fetching.request("AAAA", &cancel),
            Outcome::Cancelled { frame_sent: false }
        );
        // The socket's own open, past the exchange's first look.
        let known = Client::new(
            Address::parse(&base).unwrap(),
            Zeroizing::new("account-key".into()),
            None,
        )
        .with_public_key(super::super::mock::public());
        assert!(matches!(known.open(&cancel), Err(Stop::Cancelled)));
        let accepted = listener.accept().map(|_| ()).map_err(|error| error.kind());
        assert_eq!(accepted, Err(io::ErrorKind::WouldBlock), "no connection");
    }

    #[test]
    fn an_http_answer_plain_or_chunked() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n-----BEGIN";
        assert_eq!(http_body(plain), Some((200, b"-----BEGIN".to_vec())));
        let chunked =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6;x\r\n world\r\n0\r\n\r\n";
        assert_eq!(http_body(chunked), Some((200, b"hello world".to_vec())));
        assert_eq!(
            http_body(b"HTTP/1.1 503 Busy\r\n\r\n"),
            Some((503, Vec::new()))
        );
        assert_eq!(http_body(b"HTTP/1.1 200 OK\r\n"), None);
        assert_eq!(
            http_body(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"),
            None
        );
    }
}
