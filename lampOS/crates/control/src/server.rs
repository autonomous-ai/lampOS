//! The door on the network (docs/control.md, "The wire protocol"):
//! WebSocket over TLS at `/v1`, one JSON object per text frame, each frame
//! handed to the [`Door`] and its answers sent back in order. Lease changes
//! (a lapse, a conversation, a hand) go to every connection of the client
//! that held the lease.
//!
//! Frames are capped at [`FRAME_MAX`] (the Reviewer-lead's note on #3715:
//! `parse` itself has no cap). A connection says nothing within
//! [`HELLO_WITHIN`] of its welcome, or goes quiet for [`QUIET_MAX`], and it
//! is closed.
//!
//! The admin socket is the owner's side ([`admin`]): a pairing code with its
//! grants, a revoke, the list of paired computers. It is a Unix socket only
//! the door's own user may open; the app reaches it through the lamp's cloud
//! part in a later piece.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::{TcpListener, UnixListener};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::cert::Identity;
use crate::protocol::{EventKind, Grant};
use crate::session::{Door, Parts, event_of};
use crate::watch::{Out, Watch, phrases_in};

/// The largest frame a client may send.
pub const FRAME_MAX: usize = 64 * 1024;
/// How long a new connection has to say `hello` (or `pair`).
pub const HELLO_WITHIN: Duration = Duration::from_secs(30);
/// How long a connection may say nothing before it is closed (an SDK's
/// lease renewals keep one open).
pub const QUIET_MAX: Duration = Duration::from_secs(120);
/// How long a new TCP connection has for its TLS and WebSocket handshakes
/// (the Reviewer-lead's P2 on #3778: a silent peer held a task and an fd for
/// good).
pub const HANDSHAKE_WITHIN: Duration = Duration::from_secs(10);
/// The most connections at once, from anyone.
pub const CONNECTIONS_MAX: usize = 32;
/// The most connections at once from one address.
pub const PER_PEER_MAX: usize = 4;
/// How long the door waits after an accept that failed (out of fds, say)
/// before it accepts again, never spinning.
pub const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// The door's bounds on connections, [`HANDSHAKE_WITHIN`],
/// [`CONNECTIONS_MAX`] and [`PER_PEER_MAX`] unless a test sets others.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub handshake: Duration,
    pub total: usize,
    pub per_peer: usize,
    /// [`QUIET_MAX`].
    pub quiet: Duration,
    /// [`HELLO_WITHIN`].
    pub hello: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            handshake: HANDSHAKE_WITHIN,
            total: CONNECTIONS_MAX,
            per_peer: PER_PEER_MAX,
            quiet: QUIET_MAX,
            hello: HELLO_WITHIN,
        }
    }
}

/// One connection's place in the door's count, given back when dropped.
struct Slot {
    shared: Arc<Shared>,
    peer: std::net::IpAddr,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut peers = self.shared.peers.lock().expect("peers");
        if let Some(count) = peers.get_mut(&self.peer) {
            *count -= 1;
            if *count == 0 {
                peers.remove(&self.peer);
            }
        }
    }
}

/// How long the helper's change thread waits before asking again, after a
/// change the helper didn't answer: three tries in all, about a minute with
/// their waits, before the change is given up and journaled (Fourteen's P3
/// on #3910: a lost `off_pushed_by` would leave a revoked computer's skills
/// on).
pub const RETRY_AFTER: [Duration; 2] = [Duration::from_secs(2), Duration::from_secs(20)];

/// `request` told to the helper at `socket`, asked again after each of
/// [`RETRY_AFTER`] while it gives no answer; later changes wait, so the
/// order holds. An answer of any kind, a refusal included, is the helper's
/// word and is not asked again.
fn tell_until_answered(socket: &std::path::Path, request: &Value) {
    let mut waits = RETRY_AFTER.iter();
    loop {
        match crate::push::ask_helper(socket, request, crate::push::ASK_WAIT) {
            Ok(answer) => {
                eprintln!("lamp-control: told the helper {request}: {answer}");
                return;
            }
            Err(error) => match waits.next() {
                Some(wait) => {
                    eprintln!(
                        "lamp-control: telling the helper {request}: {error}; again in {} s",
                        wait.as_secs()
                    );
                    std::thread::sleep(*wait);
                }
                // An unanswered ask may still have reached the helper (a
                // read that timed out after the write), so the journal says
                // it may be served; asking again is harmless, as each change
                // sets a computer's state (the Reviewer-lead's P3 on #4027).
                None => {
                    eprintln!(
                        "lamp-control: gave up telling the helper {request} after {} tries: {error}; it may still be served",
                        RETRY_AFTER.len() + 1
                    );
                    return;
                }
            },
        }
    }
}

/// How often leases are checked for a lapse.
pub const LEASE_TICK: Duration = Duration::from_millis(250);

/// An open connection: its client, once hello'd, and its outbox.
struct Open {
    client: Option<String>,
    outbox: mpsc::UnboundedSender<Value>,
    /// What it subscribed to, and its phrases, as its session says.
    events: BTreeSet<EventKind>,
    phrases: Vec<String>,
    /// Its client's grants, read at its hello.
    grants: BTreeSet<Grant>,
}

/// What every connection shares.
pub struct Shared {
    door: Mutex<Door>,
    parts: Mutex<Box<dyn Parts + Send>>,
    /// Each open connection's client (once hello'd) and its outbox.
    open: Mutex<HashMap<u64, Open>>,
    next: AtomicU64,
    started: Instant,
    /// How far tests have moved the door's clock on ([`Shared::move_clock_on`]).
    ahead_us: AtomicU64,
    pin: String,
    limits: Mutex<Limits>,
    /// Open connections by address, handshakes included.
    peers: Mutex<HashMap<std::net::IpAddr, usize>>,
    /// The manage helper's socket, root's, which installs, lists and
    /// removes pushed skills (`crate::push`).
    manage: Mutex<Option<PathBuf>>,
    /// What the helper is told of pairings, one at a time in the door's
    /// order, by one thread started at the first (Reviewer-two on #3504: a
    /// late wider grant change must never be served after a newer narrower
    /// one).
    changes: Mutex<Option<std::sync::mpsc::Sender<Value>>>,
}

impl Shared {
    pub fn new(door: Door, parts: Box<dyn Parts + Send>, pin: String) -> Arc<Self> {
        Arc::new(Self {
            door: Mutex::new(door),
            parts: Mutex::new(parts),
            open: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            started: Instant::now(),
            ahead_us: AtomicU64::new(0),
            pin,
            limits: Mutex::new(Limits::default()),
            peers: Mutex::new(HashMap::new()),
            manage: Mutex::new(None),
            changes: Mutex::new(None),
        })
    }

    /// The manage helper at `socket` for pushed skills.
    pub fn set_manage_socket(&self, socket: PathBuf) {
        *self.manage.lock().expect("manage") = Some(socket);
    }

    /// `ask` of the manage helper off the door's lock and the runtime's
    /// thread; its answer goes to `outbox`, the asking connection's.
    fn ask_helper(
        self: &Arc<Self>,
        ask: crate::push::Ask,
        outbox: mpsc::WeakUnboundedSender<Value>,
    ) {
        let shared = self.clone();
        tokio::task::spawn_blocking(move || {
            let socket = shared.manage.lock().expect("manage").clone();
            let answer = match socket {
                Some(socket) => crate::push::ask_helper(&socket, &ask.request, ask.wait()),
                None => Err("no manage socket given".to_owned()),
            };
            // A pairing changed while the install was with the helper: its
            // change may have been served first, so the door tells it
            // again now, after the install (Fourteen's P2 on #3910).
            {
                let mut door = shared.door.lock().expect("door");
                match ask.kind {
                    crate::push::AskKind::Install => door.push_installed(&ask.client, ask.rev),
                    _ => door.helper_answered(&ask.client),
                }
            }
            shared.tell_helper();
            if let Some(outbox) = outbox.upgrade() {
                let _ = outbox.send(ask.answer(answer));
            }
        });
    }

    /// The door's pairing changes for the manage helper (a computer's
    /// pushed skills off, or held to new grants), sent in order by one
    /// thread; each answer said in the journal, as nobody waits for it.
    fn tell_helper(&self) {
        // The queue's lock first, then the door's: two threads (an install's
        // answer, a pair or a revoke) can't each take a batch and send them
        // reversed (Reviewer-two's P3 on #3910).
        let mut queue = self.changes.lock().expect("changes");
        let changes = self.door.lock().expect("door").take_helper_changes();
        if changes.is_empty() {
            return;
        }
        let Some(socket) = self.manage.lock().expect("manage").clone() else {
            return;
        };
        let sender = queue.get_or_insert_with(|| {
            let (sender, received) = std::sync::mpsc::channel::<Value>();
            std::thread::spawn(move || {
                for request in received {
                    tell_until_answered(&socket, &request);
                }
            });
            sender
        });
        for change in changes {
            let _ = sender.send(change);
        }
    }

    /// Other bounds on connections, for tests.
    pub fn set_limits(&self, limits: Limits) {
        *self.limits.lock().expect("limits") = limits;
    }

    /// A place for a connection from `peer`, if the door has one: none past
    /// the total or the address's own bound.
    fn admit(self: &Arc<Self>, peer: std::net::IpAddr) -> Option<Slot> {
        let limits = *self.limits.lock().expect("limits");
        let mut peers = self.peers.lock().expect("peers");
        let total: usize = peers.values().sum();
        let mine = peers.get(&peer).copied().unwrap_or(0);
        if total >= limits.total || mine >= limits.per_peer {
            return None;
        }
        *peers.entry(peer).or_default() += 1;
        Some(Slot {
            shared: self.clone(),
            peer,
        })
    }

    /// As [`Shared::new`], with parts made knowing where their later events
    /// go (a `say`'s `done`): to every connection of the client named.
    pub fn new_with(
        door: Door,
        pin: String,
        parts: impl FnOnce(crate::parts::Events) -> Box<dyn Parts + Send>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|shared: &std::sync::Weak<Self>| {
            let shared = shared.clone();
            let events: crate::parts::Events = Arc::new(move |client: &str, event: Value| {
                if let Some(shared) = shared.upgrade() {
                    shared.send_to(client, event);
                }
            });
            Self {
                door: Mutex::new(door),
                parts: Mutex::new(parts(events)),
                open: Mutex::new(HashMap::new()),
                next: AtomicU64::new(1),
                started: Instant::now(),
                ahead_us: AtomicU64::new(0),
                pin,
                limits: Mutex::new(Limits::default()),
                peers: Mutex::new(HashMap::new()),
                manage: Mutex::new(None),
                changes: Mutex::new(None),
            }
        })
    }

    /// Events for a client's later answers, as `parts` are given them: the
    /// body lease's `done` for a `look` (`crate::body_lease::hold`).
    pub fn events(self: &Arc<Self>) -> crate::parts::Events {
        let shared = Arc::downgrade(self);
        Arc::new(move |client: &str, event: Value| {
            if let Some(shared) = shared.upgrade() {
                shared.send_to(client, event);
            }
        })
    }

    /// Sends `event` to every connection of `client`.
    fn send_to(&self, client: &str, event: Value) {
        for open in self.open.lock().expect("open").values() {
            if open.client.as_deref() == Some(client) {
                let _ = open.outbox.send(event.clone());
            }
        }
    }

    fn now_us(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_micros())
            .unwrap_or(u64::MAX)
            .saturating_add(self.ahead_us.load(Ordering::Relaxed))
    }

    /// For tests: the door's clock moves on by `by`, as if that long had
    /// passed, so a test of "one pairing try a second" need not sleep it
    /// (#1806). Every rule that reads the clock is unchanged, and it only
    /// moves forward. Nothing in the product calls it.
    pub fn move_clock_on(&self, by: Duration) {
        let by = u64::try_from(by.as_micros()).unwrap_or(u64::MAX);
        self.ahead_us.fetch_add(by, Ordering::Relaxed);
    }

    /// The client holding the protocol's `body` lease now, if any: the door
    /// then holds the body's own (`crate::body_lease`).
    pub fn body_holder(&self) -> Option<String> {
        let now = self.now_us();
        self.door
            .lock()
            .expect("door")
            .leases()
            .holder(crate::protocol::Part::Body, now)
            .map(str::to_owned)
    }

    /// The `body` lease held now, by its id: a look asked under one is
    /// never sent under another (the Reviewer-lead's P3 on #4023).
    pub fn body_lease(&self) -> Option<String> {
        let now = self.now_us();
        self.door
            .lock()
            .expect("door")
            .leases()
            .holding(crate::protocol::Part::Body, now)
            .map(str::to_owned)
    }

    /// What the bus said, handed on: each event to every connection that
    /// subscribed to it and holds its grant; words heard only as the
    /// phrases each connection listens for, never the words; the lamp's own
    /// lease signals to the leases (docs/control.md, "Events").
    pub fn deliver(&self, outs: Vec<Out>) {
        let now = self.now_us();
        let at = rfc3339_utc(std::time::SystemTime::now());
        for out in outs {
            match out {
                Out::Event(kind, fields) => {
                    let mut event = fields;
                    event["ev"] = serde_json::to_value(kind).unwrap_or_default();
                    event["at"] = Value::String(at.clone());
                    let mut sent = Vec::new();
                    for open in self.open.lock().expect("open").values() {
                        let granted = kind
                            .needs()
                            .is_none_or(|grant| open.grants.contains(&grant));
                        if let Some(client) = &open.client
                            && granted
                            && open.events.contains(&kind)
                            && open.outbox.send(event.clone()).is_ok()
                        {
                            sent.push(client.clone());
                        }
                    }
                    self.journal_events(&sent, &event["ev"], now);
                }
                Out::Heard(text) => {
                    let mut heard = Vec::new();
                    for open in self.open.lock().expect("open").values() {
                        if open.client.is_none()
                            || !open.grants.contains(&Grant::Hear)
                            || !open.events.contains(&EventKind::PhraseHeard)
                        {
                            continue;
                        }
                        for phrase in phrases_in(&text, &open.phrases) {
                            if open.outbox.send(serde_json::json!({"ev": "phrase.heard", "at": at, "phrase": phrase})).is_ok()
                                && let Some(client) = &open.client
                            {
                                heard.push(client.clone());
                            }
                        }
                    }
                    // The kind alone: never the phrase, nor what was said.
                    self.journal_events(&heard, &serde_json::json!("phrase.heard"), now);
                }
                Out::Suspend(part, why) => {
                    let changes = self
                        .door
                        .lock()
                        .expect("door")
                        .leases()
                        .suspend(part, why, now);
                    self.tell(changes);
                }
                Out::Resume => {
                    let changes = self.door.lock().expect("door").leases().resume(now);
                    self.tell(changes);
                }
                Out::Lose(part, why) => {
                    let changes = self
                        .door
                        .lock()
                        .expect("door")
                        .leases()
                        .lose(part, why, now);
                    self.tell(changes);
                }
            }
        }
    }

    /// Each event sent, by kind and computer (docs/control.md, "The
    /// journal"), once the connections' lock is let go.
    fn journal_events(&self, clients: &[String], kind: &Value, now_us: u64) {
        if clients.is_empty() {
            return;
        }
        let kind = kind.as_str().unwrap_or("event");
        let mut door = self.door.lock().expect("door");
        for client in clients {
            door.journal_event(client, kind, now_us);
        }
    }

    /// Sends each lease change to every connection of its client.
    fn tell(&self, changes: Vec<crate::lease::Change>) {
        if changes.is_empty() {
            return;
        }
        {
            let now = self.now_us();
            let mut door = self.door.lock().expect("door");
            for change in &changes {
                door.journal_lease(change, now);
            }
        }
        let open = self.open.lock().expect("open");
        for change in changes {
            for each in open.values() {
                if each.client.as_deref() == Some(change.client.as_str()) {
                    let _ = each.outbox.send(event_of(&change));
                }
            }
        }
    }

    /// Closes every connection of `client` at once, a listener's included:
    /// dropping its outbox ends the connection once what's queued (its
    /// leases' `lost`) is sent (harness-reviewer's P2 on #3778).
    fn close_client(&self, client: &str) {
        self.open
            .lock()
            .expect("open")
            .retain(|_, open| open.client.as_deref() != Some(client));
    }
}

/// The TLS side of the door, from its certificate.
pub fn acceptor(identity: &Identity) -> std::io::Result<tokio_rustls::TlsAcceptor> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(std::io::Error::other)?
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(
                identity.cert.clone(),
            )],
            rustls::pki_types::PrivateKeyDer::Pkcs8(identity.key.clone().into()),
        )
        .map_err(std::io::Error::other)?;
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
}

/// Serves the door on `listener` until the task is dropped, and checks the
/// leases every [`LEASE_TICK`].
pub async fn serve(listener: TcpListener, tls: tokio_rustls::TlsAcceptor, shared: Arc<Shared>) {
    let ticking = shared.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(LEASE_TICK);
        loop {
            tick.tick().await;
            let now = ticking.now_us();
            let changes = {
                let mut door = ticking.door.lock().expect("door");
                door.journal_tick(now);
                door.leases().lapse(now)
            };
            ticking.tell(changes);
        }
    });
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(_) => {
                tokio::time::sleep(ACCEPT_BACKOFF).await;
                continue;
            }
        };
        // Over the bound: closed at once, holding nothing.
        let Some(slot) = shared.admit(peer.ip()) else {
            drop(stream);
            continue;
        };
        let tls = tls.clone();
        let shared = shared.clone();
        tokio::spawn(async move {
            let _slot = slot;
            let within = shared.limits.lock().expect("limits").handshake;
            let opened = tokio::time::timeout(within, async {
                let stream = tls.accept(stream).await.ok()?;
                let config = WebSocketConfig::default()
                    .max_message_size(Some(FRAME_MAX))
                    .max_frame_size(Some(FRAME_MAX));
                tokio_tungstenite::accept_hdr_async_with_config(stream, only_v1, Some(config))
                    .await
                    .ok()
            })
            .await;
            if let Ok(Some(socket)) = opened {
                connection(socket, shared).await;
            }
        });
    }
}

/// Only `/v1` is the door: any other path gets 404 before the WebSocket
/// opens.
// The signature is tungstenite's handshake callback's, its error type
// theirs: it can't be boxed here.
#[allow(clippy::result_large_err)]
fn only_v1(request: &Request, response: Response) -> Result<Response, ErrorResponse> {
    if request.uri().path() == "/v1" {
        Ok(response)
    } else {
        let mut refused = ErrorResponse::new(Some("only /v1".to_owned()));
        *refused.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::NOT_FOUND;
        Err(refused)
    }
}

async fn connection<S>(mut socket: tokio_tungstenite::WebSocketStream<S>, shared: Arc<Shared>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let id = shared.next.fetch_add(1, Ordering::Relaxed);
    let (outbox, mut inbox) = mpsc::unbounded_channel::<Value>();
    // Weak, so a revoke that drops the connection's outbox still closes it.
    let answers = outbox.downgrade();
    let (mut session, welcome) = shared.door.lock().expect("door").open();
    shared.open.lock().expect("open").insert(
        id,
        Open {
            client: None,
            outbox,
            events: BTreeSet::new(),
            phrases: Vec::new(),
            grants: BTreeSet::new(),
        },
    );
    let (quiet, hello) = {
        let limits = shared.limits.lock().expect("limits");
        (limits.quiet, limits.hello)
    };
    // Set once from the welcome: frames before `hello` buy no more time
    // (Fourteen's P3 on #3778).
    let mut quiet_until = tokio::time::Instant::now() + hello;
    if socket
        .send(Message::text(welcome.to_string()))
        .await
        .is_ok()
    {
        loop {
            tokio::select! {
                incoming = socket.next() => {
                    let text = match incoming {
                        Some(Ok(Message::Text(text))) => text,
                        // A live client's keepalive keeps it open, a listener
                        // holding no lease included (blue's P2 on #3778);
                        // before hello it buys no more than the hello's time.
                        Some(Ok(Message::Ping(_) | Message::Pong(_))) => {
                            if session.client().is_some() {
                                quiet_until = tokio::time::Instant::now() + quiet;
                            }
                            continue;
                        }
                        Some(Ok(Message::Binary(_))) => {
                            let _ = socket.send(Message::text(crate::protocol::refused(None, &crate::protocol::Error::new(crate::protocol::ErrorCode::BadRequest, "text frames only")).to_string())).await;
                            continue;
                        }
                        _ => break,
                    };
                    let now = shared.now_us();
                    let wall = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|since| since.as_secs())
                        .unwrap_or_default();
                    let mut outcome = {
                        let mut door = shared.door.lock().expect("door");
                        let mut parts = shared.parts.lock().expect("parts");
                        door.handle(&mut session, text.as_str(), parts.as_mut(), now, wall)
                    };
                    if let Some(client) = session.client() {
                        let grants = shared
                            .door
                            .lock()
                            .expect("door")
                            .clients()
                            .get(client)
                            .map(|paired| paired.grants.clone())
                            .unwrap_or_default();
                        if let Some(entry) = shared.open.lock().expect("open").get_mut(&id) {
                            entry.client = Some(client.to_owned());
                            entry.events = session.events().clone();
                            entry.phrases = session.phrases().to_vec();
                            entry.grants = grants;
                        }
                    }
                    if let Some(ask) = outcome.ask.take() {
                        shared.ask_helper(ask, answers.clone());
                    }
                    // A pairing that changed a computer's grants.
                    shared.tell_helper();
                    let mut gone = false;
                    for reply in outcome.send {
                        gone |= socket.send(Message::text(reply.to_string())).await.is_err();
                    }
                    if gone || outcome.close {
                        break;
                    }
                    if session.client().is_some() {
                        quiet_until = tokio::time::Instant::now() + quiet;
                    }
                }
                event = inbox.recv() => {
                    let Some(event) = event else { break };
                    if socket.send(Message::text(event.to_string())).await.is_err() {
                        break;
                    }
                }
                () = tokio::time::sleep_until(quiet_until) => break,
            }
        }
    }
    let _ = socket.close(None).await;
    shared.open.lock().expect("open").remove(&id);
    let now = shared.now_us();
    let changes = shared.door.lock().expect("door").closed(&session, now);
    // Its last connection gone, what it asked for that still goes stops (a
    // light held, a say waiting), as its `stop` would (the Reviewer-lead's
    // P3 on #3736 and #3778).
    if let Some(client) = session.client() {
        let still_open = shared
            .open
            .lock()
            .expect("open")
            .values()
            .any(|open| open.client.as_deref() == Some(client));
        if !still_open {
            shared.parts.lock().expect("parts").stop(client, now);
        }
    }
    shared.tell(changes);
}

/// The owner's side, one JSON object a line on a Unix socket:
/// - `{"code": {"grants": ["body", "light"]}}` → `{"code": "12345678", "pin": "<sha256>",
///   "shown": [...]}`, `shown` the pairing screen's words for each grant;
/// - `{"revoke": "c_…"}` → `{"revoked": true, "saved": true}`, or `"saved":
///   false` with an `error` when the file couldn't take it yet;
/// - `{"clients": true}` → `{"clients": [{id, name, grants, paired_at}]}`.
pub async fn admin(listener: UnixListener, shared: Arc<Shared>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let shared = shared.clone();
        tokio::spawn(async move {
            let (read, mut write) = stream.into_split();
            let mut lines = BufReader::new(read).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let answer = admin_answer(&shared, &line);
                if write
                    .write_all(format!("{answer}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }
}

fn admin_answer(shared: &Shared, line: &str) -> Value {
    let Ok(Value::Object(asked)) = serde_json::from_str::<Value>(line) else {
        return json!({"error": "one JSON object a line"});
    };
    let now = shared.now_us();
    if let Some(code) = asked.get("code") {
        let grants: Result<BTreeSet<Grant>, _> =
            serde_json::from_value(code.get("grants").cloned().unwrap_or(json!([])));
        return match grants {
            Ok(grants) => {
                // The pairing screen's words for what is granted.
                let shown: Vec<&str> = grants.iter().map(|grant| grant.shown()).collect();
                let code = shared.door.lock().expect("door").issue_code(grants, now);
                json!({"code": code, "pin": shared.pin, "shown": shown})
            }
            Err(error) => json!({"error": format!("grants: {error}")}),
        };
    }
    if let Some(Value::String(client)) = asked.get("revoke") {
        let revoked = shared.door.lock().expect("door").revoke(client, now);
        shared.tell(revoked.changes);
        // Its pushed skills off (not deleted), whether or not it had any.
        shared.tell_helper();
        // What it asked for that still goes stops, as its `stop` would.
        if revoked.was {
            shared.parts.lock().expect("parts").stop(client, now);
        }
        // In effect now either way; a failed save is said, since a restart
        // before a save succeeds would bring the key back.
        let answer = match revoked.saved {
            Ok(()) => json!({"revoked": revoked.was, "saved": true}),
            Err(error) => json!({
                "revoked": revoked.was,
                "saved": false,
                "error": format!("not saved yet, so a restart now would bring it back: {error}"),
            }),
        };
        // Refused in memory either way, so its connections go now.
        shared.close_client(client);
        return answer;
    }
    if asked.get("clients").is_some() {
        let door = shared.door.lock().expect("door");
        let clients: Vec<Value> = door
            .clients()
            .clients
            .iter()
            .map(|client| json!({"id": client.id, "name": client.name, "grants": client.grants, "paired_at": client.paired_at}))
            .collect();
        return json!({"clients": clients});
    }
    json!({"error": "ask for code, revoke or clients"})
}

/// The bus topics the door reads, each subscribed on its own thread, every
/// message handed to `watch` and what it means delivered.
pub const TOPICS: [&str; 18] = [
    "humans/faces/tracked",
    "humans/hands/tracked",
    "room/stood_up",
    "name/heard",
    "voice/heard",
    "ears/classes",
    "controls/gesture",
    "body/hand",
    "voice/state",
    "lamp/pause",
    "voice/mute",
    "voice/privacy",
    "light/state",
    "light/drawn",
    "mood/state",
    "controls/touch",
    "air/state",
    "board/state",
];

/// Subscribes to [`TOPICS`] on the bus at `bus`, and hands every message to
/// `watch`, delivering what it means through `shared` on the runtime of
/// the caller.
pub fn watch_bus(
    bus: &lamp_bus::Bus,
    watch: Arc<Mutex<Watch>>,
    shared: Arc<Shared>,
) -> std::io::Result<()> {
    let (sent, mut taken) = mpsc::unbounded_channel::<(String, Value, u64)>();
    for topic in TOPICS {
        let mut subscriber = bus.subscribe(topic, "control")?;
        let sent = sent.clone();
        let clock = shared.clone();
        std::thread::spawn(move || {
            // What `watch` last read from this topic, and when: a state
            // message that says the same is passed over here, not handed on
            // (lamp-75c9's idle CPU was `voice/privacy`'s hundred a second),
            // but a repeat still goes on now and then for `Watch::lapse`.
            let mut kept: Option<(Value, u64)> = None;
            loop {
                match subscriber.recv(Duration::from_secs(1)) {
                    Ok(Some(message)) => {
                        let Ok(payload) = message.json() else {
                            continue;
                        };
                        // When it came, not when the runtime gets to it: a
                        // runtime held up (a slow save) mustn't make a lease
                        // renewed in time look late (Fourteen's P3 on #4041).
                        let now_us = clock.now_us();
                        if let Some(key) = Watch::kept_of(topic, &payload) {
                            if Watch::passes_over(kept.as_ref(), &key, now_us) {
                                continue;
                            }
                            kept = Some((key, now_us));
                        }
                        if sent.send((topic.to_owned(), payload, now_us)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => std::thread::sleep(Duration::from_millis(200)),
                }
            }
        });
    }
    tokio::spawn(async move {
        // A privacy lease that stops coming lapses with no message to say
        // so: checked every LEASE_TICK, and only once every message already
        // come is taken (`biased`), so a stall never judges a lease before
        // its renewal waiting in the queue.
        let mut tick = tokio::time::interval(LEASE_TICK);
        loop {
            let outs = tokio::select! {
                biased;
                taken = taken.recv() => {
                    let Some((topic, payload, came_us)) = taken else { return };
                    watch.lock().expect("watch").take(&topic, &payload, came_us)
                }
                _ = tick.tick() => watch.lock().expect("watch").lapse(shared.now_us()),
            };
            shared.deliver(outs);
        }
    });
    Ok(())
}

/// `at` as RFC 3339 in UTC, to the second (`2026-10-27T08:30:01Z`).
pub fn rfc3339_utc(at: std::time::SystemTime) -> String {
    let seconds = at
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    let days = seconds / 86_400;
    let rest = seconds % 86_400;
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_rfc3339_in_utc() {
        let at = std::time::UNIX_EPOCH + Duration::from_secs(1_793_090_401);
        assert_eq!(rfc3339_utc(at), "2026-10-27T08:40:01Z");
        assert_eq!(rfc3339_utc(std::time::UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            rfc3339_utc(std::time::UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00Z"
        );
    }
}
