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
    pin: String,
    limits: Mutex<Limits>,
    /// Open connections by address, handshakes included.
    peers: Mutex<HashMap<std::net::IpAddr, usize>>,
}

impl Shared {
    pub fn new(door: Door, parts: Box<dyn Parts + Send>, pin: String) -> Arc<Self> {
        Arc::new(Self {
            door: Mutex::new(door),
            parts: Mutex::new(parts),
            open: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            started: Instant::now(),
            pin,
            limits: Mutex::new(Limits::default()),
            peers: Mutex::new(HashMap::new()),
        })
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
                pin,
                limits: Mutex::new(Limits::default()),
                peers: Mutex::new(HashMap::new()),
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
        u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX)
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
                    for open in self.open.lock().expect("open").values() {
                        let granted = kind
                            .needs()
                            .is_none_or(|grant| open.grants.contains(&grant));
                        if open.client.is_some() && granted && open.events.contains(&kind) {
                            let _ = open.outbox.send(event.clone());
                        }
                    }
                }
                Out::Heard(text) => {
                    for open in self.open.lock().expect("open").values() {
                        if open.client.is_none()
                            || !open.grants.contains(&Grant::Hear)
                            || !open.events.contains(&EventKind::PhraseHeard)
                        {
                            continue;
                        }
                        for phrase in phrases_in(&text, &open.phrases) {
                            let _ = open.outbox.send(serde_json::json!({"ev": "phrase.heard", "at": at, "phrase": phrase}));
                        }
                    }
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

    /// Sends each lease change to every connection of its client.
    fn tell(&self, changes: Vec<crate::lease::Change>) {
        if changes.is_empty() {
            return;
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
            let changes = ticking.door.lock().expect("door").leases().lapse(now);
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
                    let outcome = {
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
/// - `{"code": {"grants": ["body", "light"]}}` → `{"code": "12345678", "pin": "<sha256>"}`;
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
                let code = shared.door.lock().expect("door").issue_code(grants, now);
                json!({"code": code, "pin": shared.pin})
            }
            Err(error) => json!({"error": format!("grants: {error}")}),
        };
    }
    if let Some(Value::String(client)) = asked.get("revoke") {
        let revoked = shared.door.lock().expect("door").revoke(client, now);
        shared.tell(revoked.changes);
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
pub const TOPICS: [&str; 14] = [
    "humans/faces/tracked",
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
    "mood/state",
    "controls/touch",
];

/// Subscribes to [`TOPICS`] on the bus at `bus`, and hands every message to
/// `watch`, delivering what it means through `shared` on the runtime of
/// the caller.
pub fn watch_bus(
    bus: &lamp_bus::Bus,
    watch: Arc<Mutex<Watch>>,
    shared: Arc<Shared>,
) -> std::io::Result<()> {
    let (sent, mut taken) = mpsc::unbounded_channel::<(String, Value)>();
    for topic in TOPICS {
        let mut subscriber = bus.subscribe(topic, "control")?;
        let sent = sent.clone();
        std::thread::spawn(move || {
            loop {
                match subscriber.recv(Duration::from_secs(1)) {
                    Ok(Some(message)) => {
                        if let Ok(payload) = message.json()
                            && sent.send((topic.to_owned(), payload)).is_err()
                        {
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
        while let Some((topic, payload)) = taken.recv().await {
            let now = shared.now_us();
            let outs = watch.lock().expect("watch").take(&topic, &payload, now);
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
