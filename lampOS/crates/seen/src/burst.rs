//! One activity burst on V1's wire (#6543 phase 2), on a thread of its own
//! so the gate, the frames and the bus never wait on the network:
//!
//! 1. the server's public key, fetched if not known (V1's
//!    `GET <base>/crypto/public-key` with `X-API-Key`);
//! 2. a new WebSocket to the activity path, with `X-API-Key`;
//! 3. the key exchange ([`crate::crypto`]), then V1's exact config
//!    ([`crate::wire::v1_action_config`]), its answer read and not
//!    checked, as V1 does;
//! 4. each frame as it comes, at least [`Pace::gap`] after the one before
//!    (the server buffers a frame into its clip only a second or more
//!    after the last), and its answer read before the next;
//! 5. the socket closed, so the server's session, and the frames it
//!    buffered for the clip, end with the burst.
//!
//! A `busy` answer drops the burst at once, with no retry inside it. A
//! failure (no key, no connection, an HTTP refusal, a refused exchange, a
//! server error, no answer within [`Pace::answer_within`]) ends it too.
//! [`Burst::abort`] (or dropping the [`Burst`]) ends it at once, whatever
//! it is waiting on, and closes the socket. No heartbeat is sent: a burst
//! lasts seconds, V1's heartbeat came once a minute.
//!
//! Each frame copy ([`Copied`]) is wiped when it is dropped: once its
//! message is made, or with the burst if it was never sent. Its Base64 and
//! the message's plaintext are wiped too; only ciphertext reaches the
//! socket.
use crate::{
    crypto::{self, Session},
    endpoint::Endpoint,
    wire::{self, ActionAnswer},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use rsa::RsaPublicKey;
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc as std_mpsc,
    },
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot},
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{self, Message, client::IntoClientRequest, http::HeaderValue},
};
use zeroize::{Zeroize, Zeroizing};

/// The most frames a burst takes.
pub const MOST_FRAMES: usize = 8;
/// The longest public key read.
const MOST_KEY_BYTES: usize = 16 * 1024;
/// The close frame's wait, before the socket is simply dropped.
const CLOSE_WITHIN: Duration = Duration::from_millis(500);

/// A burst's times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pace {
    /// The least time between two frames sent.
    pub gap: Duration,
    /// The wait for the connection, and for the public key.
    pub connect_within: Duration,
    /// The wait for any one answer (V1's `DL_WS_RECV_TIMEOUT_S`).
    pub answer_within: Duration,
    /// The wait for the next frame from the camera before the burst ends
    /// with what it has.
    pub frame_within: Duration,
}

impl Default for Pace {
    fn default() -> Self {
        Self {
            gap: Duration::from_millis(1_100),
            connect_within: Duration::from_secs(wire::ANSWER_WITHIN_S),
            answer_within: Duration::from_secs(wire::ANSWER_WITHIN_S),
            frame_within: Duration::from_secs(10),
        }
    }
}

/// A frame copied for a burst: wiped, and counted, when dropped.
pub struct Copied {
    bytes: Vec<u8>,
    wiped: Arc<AtomicU64>,
}

impl Copied {
    /// The camera's `jpeg` as it is sent ([`crate::scale::for_sending`]),
    /// made the in-flight copy: decoded, scaled and encoded again in here,
    /// every buffer on the way wiped.
    pub fn scaled(
        jpeg: &[u8],
        size: (u32, u32),
        wiped: Arc<AtomicU64>,
    ) -> Result<Self, crate::scale::Refused> {
        let mut sent = crate::scale::for_sending(jpeg, size.0, size.1)?;
        Ok(Self {
            bytes: std::mem::take(&mut *sent),
            wiped,
        })
    }
}

impl Drop for Copied {
    fn drop(&mut self) {
        self.bytes.zeroize();
        self.wiped.fetch_add(1, Ordering::Relaxed);
    }
}

/// Why a burst failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// No public key: not fetched, or not an RSA key.
    PublicKey,
    /// No connection: refused, unreachable, TLS, or not in time.
    Connect,
    /// The upgrade refused with this HTTP status (a 5xx when full).
    Refused(u16),
    /// The key exchange was not accepted.
    KeyExchange,
    /// The server answered a frame with an error other than busy.
    Server,
    /// No answer in time.
    NoAnswer,
    /// The server closed the socket.
    Closed,
    /// An answer that is not V1's shape or does not decrypt.
    Protocol,
}

impl Failure {
    pub fn as_str(self) -> &'static str {
        match self {
            Failure::PublicKey => "public_key",
            Failure::Connect => "connect",
            Failure::Refused(_) => "refused",
            Failure::KeyExchange => "key_exchange",
            Failure::Server => "server_error",
            Failure::NoAnswer => "no_answer",
            Failure::Closed => "closed",
            Failure::Protocol => "protocol",
        }
    }
}

/// How a burst ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every frame sent was answered.
    Answered,
    /// The server said busy: the burst dropped.
    Busy,
    Failed(Failure),
}

/// What the burst's thread says back.
#[derive(Debug)]
pub enum Event {
    /// The public key it fetched, to keep for the next bursts.
    PublicKey(Box<RsaPublicKey>),
    /// One frame's answer.
    Answer(ActionAnswer),
    /// The end; the socket is closed.
    Done(Outcome),
}

/// A burst under way, from the caller's side.
pub struct Burst {
    frames: Option<mpsc::Sender<Copied>>,
    abort: Option<oneshot::Sender<()>>,
    events: std_mpsc::Receiver<Event>,
}

impl Burst {
    /// Start one: its thread connects at once.
    pub fn start(endpoint: Endpoint, pace: Pace) -> io::Result<Self> {
        let (frames, frames_rx) = mpsc::channel(MOST_FRAMES);
        let (abort, abort_rx) = oneshot::channel();
        let (events_tx, events) = std_mpsc::channel();
        std::thread::Builder::new()
            .name("seen-burst".into())
            .spawn(move || {
                let done = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => {
                        runtime.block_on(run(&endpoint, pace, frames_rx, abort_rx, &events_tx))
                    }
                    Err(_) => Some(Outcome::Failed(Failure::Connect)),
                };
                if let Some(outcome) = done {
                    let _ = events_tx.send(Event::Done(outcome));
                }
            })?;
        Ok(Self {
            frames: Some(frames),
            abort: Some(abort),
            events,
        })
    }

    /// Hand a frame to the burst; false if it takes no more (ended, full,
    /// or its thread finished), and the copy is wiped at once.
    pub fn send(&mut self, frame: Copied) -> bool {
        self.frames
            .as_ref()
            .is_some_and(|frames| frames.try_send(frame).is_ok())
    }

    /// Whether it still takes frames.
    pub fn open(&self) -> bool {
        self.frames
            .as_ref()
            .is_some_and(|frames| !frames.is_closed())
    }

    /// No more frames: the burst ends once the ones sent are answered.
    pub fn end(&mut self) {
        self.frames = None;
    }

    /// End it at once and close the socket; frames not sent are wiped.
    pub fn abort(&mut self) {
        self.frames = None;
        if let Some(abort) = self.abort.take() {
            let _ = abort.send(());
        }
    }

    /// What the thread said since the last look.
    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for Burst {
    fn drop(&mut self) {
        self.abort();
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The burst; `None` when aborted (no one is listening).
async fn run(
    endpoint: &Endpoint,
    pace: Pace,
    mut frames: mpsc::Receiver<Copied>,
    abort: oneshot::Receiver<()>,
    events: &std_mpsc::Sender<Event>,
) -> Option<Outcome> {
    let mut socket: Option<Socket> = None;
    let outcome = tokio::select! {
        biased;
        // An abort, or the caller gone.
        _ = abort => None,
        outcome = session(endpoint, pace, &mut frames, &mut socket, events) => Some(outcome),
    };
    // Frames not sent are wiped with the queue.
    frames.close();
    while frames.try_recv().is_ok() {}
    if let Some(mut socket) = socket.take() {
        let _ = timeout(CLOSE_WITHIN, socket.close(None)).await;
    }
    outcome
}

/// TLS 1.2 and 1.3 with the `ring` provider and the webpki roots, as the
/// other lampOS clients have it.
fn tls() -> Option<rustls::ClientConfig> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    Some(
        rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .ok()?
        .with_root_certificates(roots)
        .with_no_client_auth(),
    )
}

/// The server's public key, as V1's `fetch_public_key` reads it: the body
/// a PEM.
async fn fetch_public_key(endpoint: &Endpoint, within: Duration) -> Result<RsaPublicKey, Failure> {
    let client = reqwest::Client::builder()
        .tls_backend_preconfigured(tls().ok_or(Failure::Connect)?)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(within)
        .build()
        .map_err(|_| Failure::Connect)?;
    let response = client
        .get(&endpoint.public_key_url)
        .header("X-API-Key", endpoint.api_key.as_str())
        .send()
        .await
        .map_err(|_| Failure::Connect)?;
    let status = response.status();
    if !status.is_success() {
        return Err(Failure::Refused(status.as_u16()));
    }
    let body = response.bytes().await.map_err(|_| Failure::Connect)?;
    if body.len() > MOST_KEY_BYTES {
        return Err(Failure::PublicKey);
    }
    let pem = std::str::from_utf8(&body).map_err(|_| Failure::PublicKey)?;
    crypto::public_key_from_pem(pem).map_err(|_| Failure::PublicKey)
}

/// The next text message, within `within`; pings are answered by the
/// socket as it reads.
async fn receive(socket: &mut Socket, within: Duration) -> Result<String, Failure> {
    let deadline = Instant::now() + within;
    loop {
        match tokio::time::timeout_at(deadline, socket.next()).await {
            Err(_) => return Err(Failure::NoAnswer),
            Ok(Some(Ok(Message::Text(text)))) => return Ok(text.as_str().to_owned()),
            Ok(Some(Ok(Message::Binary(bytes)))) => {
                return String::from_utf8(bytes.to_vec()).map_err(|_| Failure::Protocol);
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)))) => {}
            Ok(Some(Ok(Message::Close(_))) | None | Some(Err(_))) => return Err(Failure::Closed),
        }
    }
}

async fn send(socket: &mut Socket, text: String) -> Result<(), Failure> {
    socket
        .send(Message::text(text))
        .await
        .map_err(|_| Failure::Closed)
}

/// Connect, with `X-API-Key`.
async fn connect(endpoint: &Endpoint, pace: Pace) -> Result<Socket, Failure> {
    let mut request = endpoint
        .socket_url
        .as_str()
        .into_client_request()
        .map_err(|_| Failure::Connect)?;
    let mut key = HeaderValue::from_str(&endpoint.api_key).map_err(|_| Failure::Connect)?;
    key.set_sensitive(true);
    request.headers_mut().insert("X-API-Key", key);
    let connector = if endpoint.socket_url.starts_with("wss://") {
        Connector::Rustls(Arc::new(tls().ok_or(Failure::Connect)?))
    } else {
        Connector::Plain
    };
    match timeout(
        pace.connect_within,
        connect_async_tls_with_config(request, None, true, Some(connector)),
    )
    .await
    {
        Err(_) => Err(Failure::Connect),
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(tungstenite::Error::Http(response))) => {
            Err(Failure::Refused(response.status().as_u16()))
        }
        Ok(Err(_)) => Err(Failure::Connect),
    }
}

/// One frame's message, its plaintext wiped once encrypted.
fn frame_message(crypto: &Session, frame: Copied) -> Result<String, Failure> {
    let b64 = Zeroizing::new(STANDARD.encode(&frame.bytes));
    drop(frame);
    // As `wire::Sent::Frame` writes it; Base64 needs no escaping.
    let plain = Zeroizing::new(format!(
        r#"{{"type":"frame","task":"action","frame_b64":"{}"}}"#,
        b64.as_str()
    ));
    crypto
        .wrap_ws_message(&plain, &mut rand_core::OsRng)
        .map_err(|_| Failure::Protocol)
}

/// What a frame's answer says.
fn read_answer(crypto: &Session, raw: &str) -> Result<Result<ActionAnswer, Outcome>, Failure> {
    let plain = crypto
        .unwrap_ws_message(raw)
        .map_err(|_| Failure::Protocol)?;
    let value: serde_json::Value = serde_json::from_str(&plain).map_err(|_| Failure::Protocol)?;
    if let Some(error) = value.get("error") {
        return Ok(Err(if error.as_str() == Some("busy") {
            Outcome::Busy
        } else {
            Outcome::Failed(Failure::Server)
        }));
    }
    serde_json::from_value(value)
        .map(Ok)
        .map_err(|_| Failure::Protocol)
}

async fn session(
    endpoint: &Endpoint,
    pace: Pace,
    frames: &mut mpsc::Receiver<Copied>,
    socket: &mut Option<Socket>,
    events: &std_mpsc::Sender<Event>,
) -> Outcome {
    match session_steps(endpoint, pace, frames, socket, events).await {
        Ok(outcome) => outcome,
        Err(failure) => Outcome::Failed(failure),
    }
}

async fn session_steps(
    endpoint: &Endpoint,
    pace: Pace,
    frames: &mut mpsc::Receiver<Copied>,
    socket: &mut Option<Socket>,
    events: &std_mpsc::Sender<Event>,
) -> Result<Outcome, Failure> {
    let public_key = match &endpoint.public_key {
        Some(key) => key.clone(),
        None => {
            let key = fetch_public_key(endpoint, pace.connect_within).await?;
            let _ = events.send(Event::PublicKey(Box::new(key.clone())));
            key
        }
    };
    let crypto = Session::with_system_random(&public_key).map_err(|_| Failure::PublicKey)?;
    let socket = socket.insert(connect(endpoint, pace).await?);
    send(socket, crypto.key_exchange()).await?;
    if !crypto::key_exchange_ok(&receive(socket, pace.answer_within).await?) {
        return Err(Failure::KeyExchange);
    }
    let config = serde_json::to_string(&wire::v1_action_config()).map_err(|_| Failure::Protocol)?;
    let config = crypto
        .wrap_ws_message(&config, &mut rand_core::OsRng)
        .map_err(|_| Failure::Protocol)?;
    send(socket, config).await?;
    // V1 reads the config's answer and checks nothing in it.
    let _ = receive(socket, pace.answer_within).await?;
    let mut last_sent: Option<Instant> = None;
    for _ in 0..MOST_FRAMES {
        let frame = match timeout(pace.frame_within, frames.recv()).await {
            Ok(Some(frame)) => frame,
            // Ended, or no frame came in time: done with what it has.
            Ok(None) | Err(_) => break,
        };
        if let Some(at) = last_sent {
            tokio::time::sleep_until(at + pace.gap).await;
        }
        let message = frame_message(&crypto, frame)?;
        last_sent = Some(Instant::now());
        send(socket, message).await?;
        let raw = receive(socket, pace.answer_within).await?;
        match read_answer(&crypto, &raw)? {
            Ok(answer) => {
                let _ = events.send(Event::Answer(answer));
            }
            Err(ended) => return Ok(ended),
        }
    }
    Ok(Outcome::Answered)
}
