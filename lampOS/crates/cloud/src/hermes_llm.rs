//! The lamp's DeepSeek for Hermes on the lamp (#5978, part 2): a forwarder
//! on `127.0.0.1:`[`PORT`] that hands Hermes' chat requests to the
//! Autonomous relay's DeepSeek with the voice's key, which lamp-cloud
//! already holds (its unit's `LoadCredential=key`). Hermes is given only a
//! local token ([`TOKEN_FILE`]), so the relay's key is never in a file
//! Hermes, or a command Hermes runs, can read.
//!
//! OpenClaw on the lamp (#6226) chats through the same forwarder, port,
//! token and paths (its provider `lampos`, base
//! `http://127.0.0.1:7741/v1/deepseek`).
//!
//! Off unless `LAMP_HERMES_ON_LAMP=on` or `LAMP_OPENCLAW_ON_LAMP=on`.
//! Served, with the token as the bearer:
//! - `POST /v1/deepseek/chat/completions`: to `<relay>/chat/completions`,
//!   the answer streamed back as it comes (Server-Sent Events included);
//! - `GET /v1/deepseek/models`: to `<relay>/models`.
//!
//! Anything else is 404, a wrong or missing token 401, a chat for any
//! model but [`MODEL`] 400. A body is at most [`MOST_BODY`]; at most
//! [`AT_ONCE`] requests run at once and [`PER_MINUTE`] start in a minute,
//! both taken before a body is read. No body, token or key is ever logged.
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// The switch for Hermes on the lamp.
pub const SWITCH: &str = "LAMP_HERMES_ON_LAMP";
/// The switch for OpenClaw on the lamp (#6226): the same forwarder serves
/// it.
pub const OPENCLAW_SWITCH: &str = "LAMP_OPENCLAW_ON_LAMP";
/// The forwarder's port, on loopback only. Not used by any other lampOS
/// part (deploy/product), nor by Hermes' own (8642, 8645).
pub const PORT: u16 = 7741;
/// The local token's file, in lamp-cloud's state folder
/// (`/var/lib/lampos-cloud`, `lampos`'s, 0700): 0600, made once.
pub const TOKEN_FILE: &str = "hermes-token";
/// The paths served, under Hermes' base `http://127.0.0.1:7741/v1/deepseek`.
pub const CHAT_PATH: &str = "/v1/deepseek/chat/completions";
pub const MODELS_PATH: &str = "/v1/deepseek/models";
/// The company relay's DeepSeek, as the voice reaches it from the product
/// `second.json` (`llm_base_url` `…/api/v1/ai`, `second_config.rs`'s
/// `relay_deepseek`).
pub const COMPANY_DEEPSEEK: &str = "https://campaign-api.autonomous.ai/api/v1/ai/v1/deepseek";
const COMPANY_RELAY_HOST: &str = "campaign-api.autonomous.ai";

/// The largest request body forwarded.
pub const MOST_BODY: usize = 2 * 1024 * 1024;
/// The largest request head read.
const MOST_HEAD: usize = 16 * 1024;
/// Requests forwarded at the same time.
pub const AT_ONCE: usize = 4;
/// Requests started in any minute.
pub const PER_MINUTE: usize = 60;
/// Connections accepted but not yet read through, so no more than this
/// many bodies of [`MOST_BODY`] are ever held at once.
pub const UNREAD_AT_ONCE: usize = 16;
/// The only model asked: the voice's DeepSeek model, on the voice's key.
pub const MODEL: &str = "deepseek-flash";
/// The head, then the body, must arrive within these.
const HEAD_WITHIN: Duration = Duration::from_secs(10);
const BODY_WITHIN: Duration = Duration::from_secs(30);
/// A refused request's unread rest is drained at most this long.
const DRAIN_WITHIN: Duration = Duration::from_secs(1);
/// Each write to the caller must be taken within this.
const WRITE_WITHIN: Duration = Duration::from_secs(30);
/// The relay: a connection, each read of its answer, and the whole answer.
const CONNECT_WITHIN: Duration = Duration::from_secs(10);
const READ_WITHIN: Duration = Duration::from_secs(120);
const ANSWER_WITHIN: Duration = Duration::from_secs(600);

/// Whether `LAMP_HERMES_ON_LAMP=on` or `LAMP_OPENCLAW_ON_LAMP=on`.
pub fn on_from_environment() -> bool {
    on_from(
        std::env::var(SWITCH).ok().as_deref(),
        std::env::var(OPENCLAW_SWITCH).ok().as_deref(),
    )
}

/// Whether the forwarder is served, from the two switches' values: either
/// exactly `on` (#5978, #6226).
pub fn on_from(hermes: Option<&str>, openclaw: Option<&str>) -> bool {
    hermes == Some("on") || openclaw == Some("on")
}

/// The relay's DeepSeek base from the voice's `second.json` text, as the
/// voice works it out: its `deepseek_base_url`, else the company relay's
/// DeepSeek under its `llm_base_url`. [`COMPANY_DEEPSEEK`] without either,
/// or with no file. Only ever on the company relay's host: a base on any
/// other host is never given the voice's key, whatever the file says.
pub fn relay_base(second_json: Option<&str>) -> String {
    #[derive(serde::Deserialize, Default)]
    struct Second {
        #[serde(default)]
        deepseek_base_url: Option<String>,
        #[serde(default)]
        llm_base_url: String,
    }
    let second: Second = second_json
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_default();
    if let Some(base) = second
        .deepseek_base_url
        .map(|base| base.trim().trim_end_matches('/').to_owned())
        .filter(|base| on_company_relay(base))
    {
        return base;
    }
    let llm = second.llm_base_url.trim();
    if on_company_relay(llm) {
        let base = llm.trim_end_matches('/');
        return if base.ends_with("/ai") {
            format!("{base}/v1/deepseek")
        } else {
            format!("{base}/deepseek")
        };
    }
    COMPANY_DEEPSEEK.to_owned()
}

/// HTTPS on exactly [`COMPANY_RELAY_HOST`] (no port, no user part).
fn on_company_relay(base: &str) -> bool {
    base.strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        == Some(COMPANY_RELAY_HOST)
}

/// `bytes` written to the caller within [`WRITE_WITHIN`].
async fn send(stream: &mut TcpStream, bytes: &[u8]) -> io::Result<()> {
    tokio::time::timeout(WRITE_WITHIN, stream.write_all(bytes))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

fn plain(token: &str) -> bool {
    (32..=128).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// The local token in `dir`'s [`TOKEN_FILE`]: the one kept, or a new one
/// (32 random bytes, hex) made and kept 0600 when there is none.
pub fn token(dir: &Path) -> io::Result<String> {
    let path = dir.join(TOKEN_FILE);
    match std::fs::read_to_string(&path) {
        Ok(kept) if plain(kept.trim()) => return Ok(kept.trim().to_owned()),
        Ok(_) => std::fs::remove_file(&path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut bytes = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
        .map_err(|_| io::Error::other("no random bytes"))?;
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    std::io::Write::write_all(&mut file, format!("{token}\n").as_bytes())?;
    file.sync_all()?;
    Ok(token)
}

/// Equal, in a time that doesn't depend on where they differ.
fn same(given: &[u8], token: &[u8]) -> bool {
    if given.len() != token.len() {
        return false;
    }
    given
        .iter()
        .zip(token)
        .fold(0u8, |differ, (a, b)| differ | (a ^ b))
        == 0
}

/// Only a peer on this lamp is answered.
pub fn admits(peer: SocketAddr) -> bool {
    peer.ip().is_loopback()
}

/// Requests started in the last minute, for [`PER_MINUTE`].
#[derive(Default)]
struct Started(Vec<Instant>);

impl Started {
    fn take(&mut self, now: Instant) -> bool {
        self.0
            .retain(|at| now.duration_since(*at) < Duration::from_secs(60));
        if self.0.len() >= PER_MINUTE {
            return false;
        }
        self.0.push(now);
        true
    }
}

/// The forwarder: the relay's DeepSeek, the key, and the local token.
pub struct Forwarder {
    base: String,
    key: String,
    token: String,
    client: reqwest::Client,
    running: Arc<Semaphore>,
    unread: Arc<Semaphore>,
    started: Mutex<Started>,
}

impl std::fmt::Debug for Forwarder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Forwarder")
            .field("base", &self.base)
            .field("key", &"<hidden>")
            .field("token", &"<hidden>")
            .finish()
    }
}

/// A request as read and let through: its method, path (no query) and
/// body, with its place among the [`AT_ONCE`]. The token is checked while
/// the head is read and not kept.
struct Request {
    method: String,
    path: String,
    body: Vec<u8>,
    _running: OwnedSemaphorePermit,
}

/// Why a request is answered here, not by the relay.
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Bad,
    Unauthorized,
    NotFound,
    NotAllowed,
    TooLarge,
    LengthRequired,
    TooMany,
    Unreachable,
    NotThisModel,
}

impl Refusal {
    fn status(&self) -> (u16, &'static str, &'static str) {
        match self {
            Self::Bad => (400, "Bad Request", "the request can't be read"),
            Self::Unauthorized => (401, "Unauthorized", "the lamp's local token is needed"),
            Self::NotFound => (404, "Not Found", "only DeepSeek's chat and models"),
            Self::NotAllowed => (405, "Method Not Allowed", "not with this method"),
            Self::TooLarge => (413, "Payload Too Large", "the request is too large"),
            Self::LengthRequired => (411, "Length Required", "a Content-Length is needed"),
            Self::TooMany => (429, "Too Many Requests", "too many requests now"),
            Self::Unreachable => (502, "Bad Gateway", "the relay can't be reached now"),
            Self::NotThisModel => (400, "Bad Request", "only the model deepseek-flash"),
        }
    }
}

fn find_head_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

impl Forwarder {
    /// The relay's DeepSeek at `base` (HTTPS, but for the tests' own local
    /// server), asked with `key`; requests must carry `token`.
    pub fn new(base: &str, key: &str, token: &str) -> Result<Self, String> {
        if key.trim().is_empty() || !plain(token) {
            return Err("the Hermes forwarder needs the lamp's key and a local token".into());
        }
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(crate::beat::tls()?)
            .https_only(!cfg!(test))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_WITHIN)
            .read_timeout(READ_WITHIN)
            .timeout(ANSWER_WITHIN)
            .build()
            .map_err(|_| "the Hermes forwarder's HTTPS client".to_owned())?;
        Ok(Self {
            base: base.trim().trim_end_matches('/').to_owned(),
            key: key.trim().to_owned(),
            token: token.to_owned(),
            client,
            running: Arc::new(Semaphore::new(AT_ONCE)),
            unread: Arc::new(Semaphore::new(UNREAD_AT_ONCE)),
            started: Mutex::new(Started::default()),
        })
    }

    /// The listener on `127.0.0.1:port` ([`PORT`] in the product).
    pub async fn bind(port: u16) -> io::Result<TcpListener> {
        TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)).await
    }

    /// Every connection on `listener` answered, one request each, until
    /// the task is dropped.
    pub async fn serve(self: Arc<Self>, listener: TcpListener) {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            if !admits(peer) {
                continue;
            }
            // Past the most connections being read, a new one is closed.
            let Ok(unread) = self.unread.clone().try_acquire_owned() else {
                continue;
            };
            let forwarder = self.clone();
            tokio::spawn(async move {
                let _ = forwarder.answer(stream, unread).await;
            });
        }
    }

    async fn read(&self, stream: &mut TcpStream) -> Result<Request, Refusal> {
        let mut bytes = Vec::with_capacity(4096);
        let mut chunk = [0u8; 4096];
        let head_end = tokio::time::timeout(HEAD_WITHIN, async {
            loop {
                if let Some(end) = find_head_end(&bytes) {
                    return Ok(end);
                }
                if bytes.len() > MOST_HEAD {
                    return Err(Refusal::Bad);
                }
                match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => return Err(Refusal::Bad),
                    Ok(read) => bytes.extend_from_slice(&chunk[..read]),
                }
            }
        })
        .await
        .map_err(|_| Refusal::Bad)??;
        let head = std::str::from_utf8(&bytes[..head_end]).map_err(|_| Refusal::Bad)?;
        let mut lines = head.split("\r\n");
        let mut first = lines.next().unwrap_or_default().split(' ');
        let (Some(method), Some(target), Some(version)) =
            (first.next(), first.next(), first.next())
        else {
            return Err(Refusal::Bad);
        };
        if !version.starts_with("HTTP/1.") {
            return Err(Refusal::Bad);
        }
        let path = target.split('?').next().unwrap_or_default().to_owned();
        let (mut length, mut bearer, mut chunked, mut expects) = (None, None, false, false);
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                return Err(Refusal::Bad);
            };
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => {
                    let parsed: usize = value.parse().map_err(|_| Refusal::Bad)?;
                    if length.is_some_and(|before| before != parsed) {
                        return Err(Refusal::Bad);
                    }
                    length = Some(parsed);
                }
                "transfer-encoding" => chunked = true,
                "authorization" => {
                    bearer = value
                        .strip_prefix("Bearer ")
                        .or_else(|| value.strip_prefix("bearer "))
                        .map(|token| token.trim().to_owned());
                }
                "expect" => expects = value.eq_ignore_ascii_case("100-continue"),
                _ => {}
            }
        }
        let authorized = bearer
            .as_deref()
            .is_some_and(|given| same(given.as_bytes(), self.token.as_bytes()));
        if !authorized {
            return Err(Refusal::Unauthorized);
        }
        match (method, path.as_str()) {
            ("POST", CHAT_PATH) | ("GET", MODELS_PATH) => {}
            (_, CHAT_PATH | MODELS_PATH) => return Err(Refusal::NotAllowed),
            _ => return Err(Refusal::NotFound),
        }
        let running = self.take_a_place()?;
        if chunked {
            return Err(Refusal::LengthRequired);
        }
        let length = match (method, length) {
            ("POST", None) => return Err(Refusal::LengthRequired),
            (_, length) => length.unwrap_or(0),
        };
        if length > MOST_BODY {
            return Err(Refusal::TooLarge);
        }
        if expects {
            let _ = send(stream, b"HTTP/1.1 100 Continue\r\n\r\n").await;
        }
        let mut body = bytes[head_end + 4..].to_vec();
        if body.len() > length {
            return Err(Refusal::Bad);
        }
        tokio::time::timeout(BODY_WITHIN, async {
            while body.len() < length {
                match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => return Err(Refusal::Bad),
                    Ok(read) if body.len() + read > length => return Err(Refusal::Bad),
                    Ok(read) => body.extend_from_slice(&chunk[..read]),
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| Refusal::Bad)??;
        if method == "POST" {
            // Only the voice's model, so nothing dearer goes on its key.
            let asked: serde_json::Value =
                serde_json::from_slice(&body).map_err(|_| Refusal::Bad)?;
            if asked.get("model").and_then(|model| model.as_str()) != Some(MODEL) {
                return Err(Refusal::NotThisModel);
            }
        }
        Ok(Request {
            method: method.to_owned(),
            path,
            body,
            _running: running,
        })
    }

    /// A place among the [`AT_ONCE`], then one of the minute's
    /// [`PER_MINUTE`]: a request refused for want of a place uses up no
    /// minute's start.
    fn take_a_place(&self) -> Result<OwnedSemaphorePermit, Refusal> {
        let permit = self
            .running
            .clone()
            .try_acquire_owned()
            .map_err(|_| Refusal::TooMany)?;
        let started = self
            .started
            .lock()
            .map(|mut started| started.take(Instant::now()))
            .unwrap_or(false);
        started.then_some(permit).ok_or(Refusal::TooMany)
    }

    async fn refuse(stream: &mut TcpStream, refusal: &Refusal) -> io::Result<()> {
        let (code, reason, message) = refusal.status();
        let body = serde_json::json!({
            "error": { "message": message, "type": "lamp_forwarder", "code": code }
        })
        .to_string();
        let head = format!(
            "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        send(stream, head.as_bytes()).await?;
        send(stream, body.as_bytes()).await?;
        stream.shutdown().await?;
        // What the caller still sends is read and dropped, briefly, so the
        // close is not a reset that loses the answer before it is read.
        let mut sink = [0u8; 4096];
        let _ = tokio::time::timeout(DRAIN_WITHIN, async {
            while matches!(stream.read(&mut sink).await, Ok(read) if read > 0) {}
        })
        .await;
        Ok(())
    }

    async fn answer(&self, mut stream: TcpStream, unread: OwnedSemaphorePermit) -> io::Result<()> {
        let read = self.read(&mut stream).await;
        drop(unread);
        let request = match read {
            Ok(request) => request,
            Err(refusal) => return Self::refuse(&mut stream, &refusal).await,
        };
        let upstream = match (request.method.as_str(), request.path.as_str()) {
            ("POST", CHAT_PATH) => self
                .client
                .post(format!("{}/chat/completions", self.base))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(request.body),
            _ => self.client.get(format!("{}/models", self.base)),
        };
        let mut answer = match upstream.bearer_auth(&self.key).send().await {
            Ok(answer) => answer,
            Err(_) => return Self::refuse(&mut stream, &Refusal::Unreachable).await,
        };
        let status = answer.status();
        let kind = answer
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|kind| kind.to_str().ok())
            .filter(|kind| !kind.contains(['\r', '\n']))
            .unwrap_or("application/json")
            .to_owned();
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {kind}\r\nCache-Control: no-cache\r\n\
             Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Answer"),
        );
        send(&mut stream, head.as_bytes()).await?;
        // Each piece as it comes, so an SSE stream reaches Hermes as the
        // relay sends it. A cut answer ends the connection without the
        // closing chunk, so Hermes sees it cut.
        loop {
            match answer.chunk().await {
                Ok(Some(piece)) if piece.is_empty() => continue,
                Ok(Some(piece)) => {
                    send(&mut stream, format!("{:x}\r\n", piece.len()).as_bytes()).await?;
                    send(&mut stream, &piece).await?;
                    send(&mut stream, b"\r\n").await?;
                }
                Ok(None) => break,
                Err(_) => return Ok(()),
            }
        }
        send(&mut stream, b"0\r\n\r\n").await?;
        stream.shutdown().await
    }
}

/// Runs the forwarder when `LAMP_HERMES_ON_LAMP=on` or
/// `LAMP_OPENCLAW_ON_LAMP=on`: the token kept in
/// `state` (lamp-cloud's state folder), the relay from `second_json` (the
/// voice's `second.json`, when handed in) and the voice's `key`. Never
/// returns; with the switch off, or when it can't start, it says why once
/// and waits forever, so the rest of lamp-cloud goes on.
pub async fn run(
    state: Option<&Path>,
    second_json: Option<&str>,
    key: &str,
    say: &mut dyn FnMut(&str),
) {
    serve_when(on_from_environment(), PORT, state, second_json, key, say).await
}

/// [`run`] with the switches already read (`on`, [`on_from`]) and the port
/// given, so a test can show either switch alone starts the listener
/// (#6226). Off, nothing is listened on.
pub async fn serve_when(
    on: bool,
    port: u16,
    state: Option<&Path>,
    second_json: Option<&str>,
    key: &str,
    say: &mut dyn FnMut(&str),
) {
    if !on {
        return std::future::pending().await;
    }
    let started = async {
        let state = state.ok_or("no state folder for the local token")?;
        let token = token(state).map_err(|_| "the local token can't be kept")?;
        let forwarder = Forwarder::new(&relay_base(second_json), key, &token)?;
        let listener = Forwarder::bind(port)
            .await
            .map_err(|_| format!("127.0.0.1:{port} can't be listened on"))?;
        Ok::<_, String>((Arc::new(forwarder), listener))
    };
    match started.await {
        Ok((forwarder, listener)) => {
            let at = listener
                .local_addr()
                .map_or_else(|_| format!("127.0.0.1:{port}"), |at| at.to_string());
            say(&format!("the Hermes forwarder listens on {at}"));
            forwarder.serve(listener).await;
        }
        Err(why) => {
            say(&format!("no Hermes forwarder: {why}"));
            std::future::pending().await
        }
    }
}

#[cfg(test)]
mod tests;
