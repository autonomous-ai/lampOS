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
//! part (lamp-cloud's `control.code`, `control.clients` and `control.revoke`,
//! docs/cloud.md).

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
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::cert::Identity;
use crate::parts::Media;
use crate::protocol::{EventKind, Grant, Op};
use crate::session::{Door, Parts, event_of};
use crate::watch::{Out, Watch, phrases_in};

/// The largest frame a client may send.
pub const FRAME_MAX: usize = 64 * 1024;
/// Binary frames (camera frames, microphone blocks) queued for one
/// connection at most; past that one is dropped.
pub const FRAMES_QUEUED: usize = 4;
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
    /// Its binary frames, camera pictures and microphone blocks, each with
    /// its kind, [`FRAMES_QUEUED`] at most: past that a frame is dropped,
    /// never queued, so a slow link never grows memory.
    frames: mpsc::Sender<(Media, Option<u64>, Vec<u8>)>,
    /// What it subscribed to, and its phrases, as its session says.
    events: BTreeSet<EventKind>,
    phrases: Vec<String>,
    /// Its client's grants, read at its hello.
    grants: BTreeSet<Grant>,
    /// Its newest `body.state` not yet sent, apart from the outbox: a
    /// newer one takes its place, so a stalled computer holds one, never a
    /// queue of 12.5 a second (the Reviewer-lead's P2 on #4564).
    body_state: watch::Sender<Option<Value>>,
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
    /// Whether computers beyond the lamp's own network may connect
    /// (`--remote on`): off unless given.
    remote: std::sync::atomic::AtomicBool,
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
            remote: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// The manage helper at `socket` for pushed skills.
    /// Lets computers beyond the lamp's own network connect too (`--remote
    /// on`), never to the camera or the microphone.
    pub fn set_remote(&self, remote: bool) {
        self.remote.store(remote, Ordering::Relaxed);
    }

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

    /// A reply the part gives later (the voice's to a `turn.end` or a
    /// `listen.set`), waited for off the door's lock and the runtime's
    /// thread; it goes to `outbox`, the asking connection's, and the
    /// journal says how it ended.
    fn answer_later(
        self: &Arc<Self>,
        later: crate::session::Later,
        outbox: mpsc::WeakUnboundedSender<Value>,
    ) {
        let shared = self.clone();
        tokio::task::spawn_blocking(move || {
            let (client, op) = (later.client.clone(), later.op);
            let reply = later.reply();
            let now = shared.now_us();
            shared
                .door
                .lock()
                .expect("door")
                .journal_answer(&client, op, &reply, now);
            if let Some(outbox) = outbox.upgrade() {
                let _ = outbox.send(reply);
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
                remote: std::sync::atomic::AtomicBool::new(false),
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

    /// Sends a binary frame of `media` to every connection of `client` that
    /// holds its grant: dropped for one whose queue is full, never waited
    /// for. Whether every one, and at least one, took it.
    fn send_binary_to(&self, client: &str, media: Media, turn: Option<u64>, frame: &[u8]) -> bool {
        let mut taken = None;
        for open in self.open.lock().expect("open").values() {
            if open.client.as_deref() == Some(client) && open.grants.contains(&media.grant()) {
                let took = open.frames.try_send((media, turn, frame.to_vec())).is_ok();
                taken = Some(taken.unwrap_or(true) && took);
            }
        }
        taken.unwrap_or(false)
    }

    /// Binary frames for a client (`crate::camera`'s fast stream,
    /// `crate::microphone`'s blocks).
    pub fn binary(self: &Arc<Self>) -> crate::parts::Binary {
        let shared = Arc::downgrade(self);
        Arc::new(
            move |client: &str, media: Media, turn: Option<u64>, frame: &[u8]| {
                shared
                    .upgrade()
                    .is_some_and(|shared| shared.send_binary_to(client, media, turn, frame))
            },
        )
    }

    fn send_to(&self, client: &str, event: Value) {
        for open in self.open.lock().expect("open").values() {
            if open.client.as_deref() == Some(client) {
                let _ = open.outbox.send(event.clone());
            }
        }
        // A stream's end is in the journal with its why, as its start was
        // (Thirteen's P3 on #4521): the camera's and the microphone's.
        if let Some(ev @ ("camera.ended" | "microphone.ended")) = event["ev"].as_str() {
            let why = event["why"].as_str().unwrap_or("ended");
            let now = self.now_us();
            self.door
                .lock()
                .expect("door")
                .journal_event(client, &format!("{ev} ({why})"), now);
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
                        if open.client.is_none() || !granted || !open.events.contains(&kind) {
                            continue;
                        }
                        if kind == EventKind::BodyState {
                            open.body_state.send_replace(Some(event.clone()));
                        } else if open.outbox.send(event.clone()).is_ok()
                            && let Some(client) = &open.client
                        {
                            sent.push(client.clone());
                        }
                    }
                    // `body.state` is never journaled: at 12.5 a second it
                    // would spend each computer's lines a second on itself
                    // and leave its commands out (the Reviewer-lead's P2 on
                    // #4564).
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
/// Whether `ip` is local whatever interface it came by: loopback only (the
/// lamp itself, or an ssh tunnel the owner gave), and an IPv4 address
/// carried in IPv6 by the same rule. The rest of the lamp's own network is
/// judged by the interface a connection arrives on ([`is_local_peer`]): a
/// private range or a link-local address by itself is not local, since a
/// VPN (WireGuard, a tailnet, ZeroTier) hands out 10/8, fc00::/7 and its
/// own fe80:: from anywhere (Thirteen's P2s on #4521; docs/control.md, "The
/// wire").
pub fn is_local(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback(),
        std::net::IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_local(std::net::IpAddr::V4(v4));
            }
            v6.is_loopback()
        }
    }
}

/// Whether network interface `iface` is a physical LAN, Ethernet or
/// Wi-Fi: Ethernet-type (`/sys/class/net/IFACE/type` 1) with a device
/// behind it (`/sys/class/net/IFACE/device`). A WireGuard, tun or tailnet
/// interface has neither; a ZeroTier or other tap, a bridge, docker's and a
/// veth are Ethernet-type with no device; their names are left out as well.
pub fn is_physical(iface: &str) -> bool {
    const VIRTUAL: [&str; 9] = [
        "lo",
        "wg",
        "tun",
        "tap",
        "zt",
        "tailscale",
        "docker",
        "veth",
        "br-",
    ];
    if VIRTUAL.iter().any(|name| iface.starts_with(name)) {
        return false;
    }
    let net = std::path::Path::new("/sys/class/net").join(iface);
    std::fs::read_to_string(net.join("type")).is_ok_and(|kind| kind.trim() == "1")
        && net.join("device").exists()
}

/// The subnets the lamp reaches directly, by its routes with no gateway:
/// its own network, whatever addresses it uses. lamp-75c9's office LAN is
/// 172.168.20.0/23 (30 Oct), which is none of the private ranges.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OnLink {
    v4: Vec<(String, u32, u32)>,
    v6: Vec<(String, u128, u8)>,
}

impl OnLink {
    /// Read now from `/proc/net/route` and `/proc/net/ipv6_route`; none if
    /// they can't be read.
    pub fn read() -> Self {
        let text = |path| std::fs::read_to_string(path).unwrap_or_default();
        Self::parse(
            &text("/proc/net/route"),
            &text("/proc/net/ipv6_route"),
            is_physical,
        )
    }

    /// From the two tables' text: an IPv4 route with no gateway and a mask,
    /// an IPv6 route with no next hop and a prefix, each on an interface
    /// `physical` says is a LAN ([`is_physical`]): never `lo`, a VPN's or a
    /// bridge's.
    pub fn parse(v4: &str, v6: &str, physical: impl Fn(&str) -> bool) -> Self {
        let hex32 = |word: &str| u32::from_str_radix(word, 16).ok();
        let v4 = v4
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                let (iface, destination, gateway, mask) = (
                    fields.first()?,
                    fields.get(1)?,
                    fields.get(2)?,
                    fields.get(7)?,
                );
                // Each is the address's bytes in network order, printed as
                // the host's own number: on a little-endian lamp, read back.
                let address = |word: &str| hex32(word).map(|n| u32::from_be_bytes(n.to_ne_bytes()));
                let (destination, gateway, mask) =
                    (address(destination)?, hex32(gateway)?, address(mask)?);
                (*iface != "lo" && physical(iface) && gateway == 0 && mask != 0)
                    .then(|| ((*iface).to_owned(), destination, mask))
            })
            .collect();
        let v6 = v6
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                let (destination, length, next_hop, iface) = (
                    fields.first()?,
                    fields.get(1)?,
                    fields.get(4)?,
                    fields.get(9)?,
                );
                let destination = u128::from_str_radix(destination, 16).ok()?;
                let length = u8::from_str_radix(length, 16).ok()?;
                let next_hop = u128::from_str_radix(next_hop, 16).ok()?;
                (*iface != "lo" && physical(iface) && next_hop == 0 && (1..=128).contains(&length))
                    .then(|| ((*iface).to_owned(), destination, length))
            })
            .collect();
        Self { v4, v6 }
    }

    /// Whether `ip` is in one of the subnets.
    pub fn holds(&self, ip: std::net::IpAddr) -> bool {
        self.iface_of(ip).is_some()
    }

    /// The physical interface whose subnet holds `ip`, if one does.
    pub fn iface_of(&self, ip: std::net::IpAddr) -> Option<&str> {
        match ip {
            std::net::IpAddr::V4(v4) => {
                let ip = u32::from(v4);
                self.v4
                    .iter()
                    .find(|(_, net, mask)| ip & mask == net & mask)
                    .map(|(iface, _, _)| iface.as_str())
            }
            std::net::IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped() {
                    return self.iface_of(std::net::IpAddr::V4(v4));
                }
                let ip = u128::from(v6);
                self.v6
                    .iter()
                    .find(|(_, net, length)| {
                        let mask = u128::MAX << (128 - u32::from(*length));
                        ip & mask == net & mask
                    })
                    .map(|(iface, _, _)| iface.as_str())
            }
        }
    }

    /// Whether `ip` is in a subnet on `iface`.
    pub fn holds_on(&self, iface: &str, ip: std::net::IpAddr) -> bool {
        match ip {
            std::net::IpAddr::V4(v4) => {
                let ip = u32::from(v4);
                self.v4
                    .iter()
                    .any(|(on, net, mask)| on == iface && ip & mask == net & mask)
            }
            std::net::IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped() {
                    return self.holds_on(iface, std::net::IpAddr::V4(v4));
                }
                let ip = u128::from(v6);
                self.v6.iter().any(|(on, net, length)| {
                    let mask = u128::MAX << (128 - u32::from(*length));
                    on == iface && ip & mask == net & mask
                })
            }
        }
    }
}

/// The name of the network interface with index `index`, by
/// `/sys/class/net/*/ifindex`: an IPv6 link-local peer's scope.
pub fn iface_named(index: u32) -> Option<String> {
    std::fs::read_dir("/sys/class/net")
        .ok()?
        .flatten()
        .find_map(|entry| {
            let at = std::fs::read_to_string(entry.path().join("ifindex")).ok()?;
            (at.trim().parse::<u32>().ok()? == index)
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
}

/// Whether a connection from `peer`, which arrived on the lamp's own
/// address `local`, is from the lamp's own network: loopback (the lamp
/// itself, or the owner's ssh tunnel); a link-local peer on a physical
/// interface, by its scope (`scope_name`, `physical`); or a peer on-link on
/// the same physical interface the connection came in on (`on_link` holds
/// only those). Judged by the interface it arrived on, never by the
/// address alone: ZeroTier's `zt*` has its own link-local addresses, and a
/// VPN its own private ones (Thirteen's P2s on #4521).
pub fn is_local_peer(
    peer: std::net::SocketAddr,
    local: std::net::SocketAddr,
    on_link: &OnLink,
    scope_name: impl Fn(u32) -> Option<String>,
    physical: impl Fn(&str) -> bool,
) -> bool {
    if is_local(peer.ip()) {
        return true;
    }
    if let std::net::SocketAddr::V6(v6) = peer
        && (v6.ip().segments()[0] & 0xffc0) == 0xfe80
    {
        let scope = match (v6.scope_id(), local) {
            (0, std::net::SocketAddr::V6(local)) => local.scope_id(),
            (scope, _) => scope,
        };
        return scope != 0 && scope_name(scope).is_some_and(|iface| physical(&iface));
    }
    on_link
        .iface_of(local.ip())
        .is_some_and(|iface| on_link.holds_on(iface, peer.ip()))
}

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
        // Only the lamp's own network, unless `--remote on`: anyone else is
        // closed before TLS, and said once in the journal.
        let remote = !stream.local_addr().is_ok_and(|local| {
            is_local_peer(peer, local, &OnLink::read(), iface_named, is_physical)
        });
        if remote && !shared.remote.load(Ordering::Relaxed) {
            eprintln!(
                "lamp-control: refused a connection from {}: not on the lamp's own network",
                peer.ip()
            );
            drop(stream);
            continue;
        }
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
                connection(socket, shared, remote).await;
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

async fn connection<S>(
    mut socket: tokio_tungstenite::WebSocketStream<S>,
    shared: Arc<Shared>,
    remote: bool,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let id = shared.next.fetch_add(1, Ordering::Relaxed);
    let (outbox, mut inbox) = mpsc::unbounded_channel::<Value>();
    let (frames, mut framed) = mpsc::channel::<(Media, Option<u64>, Vec<u8>)>(FRAMES_QUEUED);
    // The writer's own way back into its queue: an end drains only its own
    // kind and puts the others back.
    let requeue = frames.clone();
    let (body_state, mut newest_body) = watch::channel::<Option<Value>>(None);
    // Until its sender goes with the connection's entry.
    let mut body_open = true;
    // Weak, so a revoke that drops the connection's outbox still closes it.
    let answers = outbox.downgrade();
    let (mut session, welcome) = shared.door.lock().expect("door").open();
    if remote {
        session.set_remote();
    }
    shared.open.lock().expect("open").insert(
        id,
        Open {
            client: None,
            outbox,
            frames,
            events: BTreeSet::new(),
            phrases: Vec::new(),
            grants: BTreeSet::new(),
            body_state,
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
                            // An unsubscribe drops a state still held, so
                            // none goes after it (El's P3 on #4564).
                            if !entry.events.contains(&EventKind::BodyState) {
                                entry.body_state.send_replace(None);
                            }
                            entry.phrases = session.phrases().to_vec();
                            entry.grants = grants;
                        }
                    }
                    if let Some(ask) = outcome.ask.take() {
                        shared.ask_helper(ask, answers.clone());
                    }
                    if let Some(later) = outcome.later.take() {
                        shared.answer_later(later, answers.clone());
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
                frame = framed.recv() => {
                    let Some((media, turn, frame)) = frame else { continue };
                    // Never beyond the lamp's own network (control.md, "The
                    // camera", "The microphone").
                    if remote {
                        continue;
                    }
                    // Only while its own stream still runs: a picture or a
                    // block queued before an `off`, a `stop`, a re-pairing
                    // or an end never goes (Thirteen's P3 on #4521), and a
                    // block of an ended stream never goes through the stream
                    // that replaced it (Second's P1 on #5839).
                    let streaming = session.client().is_some_and(|client| {
                        shared.parts.lock().expect("parts").streaming(client, media, turn)
                    });
                    if !streaming {
                        continue;
                    }
                    if socket.send(Message::Binary(frame.into())).await.is_err() {
                        break;
                    }
                }
                event = inbox.recv() => {
                    let Some(event) = event else { break };
                    // A picture or a sound started from the lamp's own
                    // network never goes to the same computer's connection
                    // from beyond it.
                    if remote
                        && event["ev"]
                            .as_str()
                            .is_some_and(|ev| ev.starts_with("camera.") || ev.starts_with("microphone."))
                    {
                        continue;
                    }
                    // Ended: the pictures or blocks still queued for it never
                    // go (Thirteen's P3 on #4521); the other kind's stay.
                    let ended = match event["ev"].as_str() {
                        Some("camera.ended") => Some(Media::Camera),
                        Some("microphone.ended") => Some(Media::Microphone),
                        _ => None,
                    };
                    let mut event = event;
                    if let Some(ended) = ended {
                        // An older stream's end, late after a newer stream of
                        // this computer started, is not the newer one's: it
                        // neither empties its queue nor reaches the computer
                        // (Second's P2 on #5839). Its turn is the door's own,
                        // taken off before it goes.
                        let turn = event
                            .as_object_mut()
                            .and_then(|fields| fields.remove(crate::camera::TURN_FIELD))
                            .and_then(|turn| turn.as_u64());
                        let newest = session.client().and_then(|client| {
                            shared.parts.lock().expect("parts").newest_turn(client, ended)
                        });
                        if crate::camera::older_end(turn, newest) {
                            continue;
                        }
                        let others: Vec<(Media, Option<u64>, Vec<u8>)> = std::iter::from_fn(|| framed.try_recv().ok())
                            .filter(|(media, _, _)| *media != ended)
                            .collect();
                        for other in others {
                            let _ = requeue.try_send(other);
                        }
                    }
                    // A picture goes only while the stream that made it runs,
                    // never through one started after it (banana, 9 Nov), and
                    // its turn is the door's own, taken off before it goes.
                    if event["ev"] == "camera.frame" {
                        let turn = event
                            .as_object_mut()
                            .and_then(|fields| fields.remove(crate::camera::TURN_FIELD))
                            .and_then(|turn| turn.as_u64());
                        let streaming = turn.is_some_and(|turn| {
                            session.client().is_some_and(|client| {
                                shared
                                    .parts
                                    .lock()
                                    .expect("parts")
                                    .streaming(client, Media::Camera, Some(turn))
                            })
                        });
                        if !streaming {
                            continue;
                        }
                    }
                    if socket.send(Message::text(event.to_string())).await.is_err() {
                        break;
                    }
                }
                changed = newest_body.changed(), if body_open => {
                    if changed.is_err() {
                        body_open = false;
                        continue;
                    }
                    let Some(event) = newest_body.borrow_and_update().clone() else {
                        continue;
                    };
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
            shared.parts.lock().expect("parts").gone(client, now);
        }
    }
    shared.tell(changes);
}

/// The owner's side, one JSON object a line on a Unix socket:
/// - `{"code": {"grants": ["body", "light"]}}` → `{"code": "12345678", "pin": "<sha256>",
///   "shown": [...], "until_unix_ms": N}`, `shown` the pairing screen's words
///   for each grant and `until_unix_ms` when the code runs out by the wall
///   clock (the code itself lapses by the monotonic clock, [`CODE_FOR_US`]
///   from now, so this is the wall clock now plus that). With `"say_pin":
///   true` the lamp also says the first 8 characters of its pin aloud (the
///   spoken pin check, #5299), and the answer says `"pin_said"`: whether the
///   voice took the line;
/// - `{"take_back": true}` (and `"client": "c_…"` for one computer) →
///   `{"taken": N}`: every lease of that computer, or of every computer, is
///   lost as `taken_by_owner_app` and its holder told; the computers stay
///   paired;
/// - `{"cancel_code": true}` → `{"dropped": true|false}`: the code out is
///   dropped, so nobody can pair with it (the app's answer to a spoken check
///   that doesn't match), and whether there was one;
/// - `{"revoke": "c_…"}` → `{"revoked": true, "saved": true}`, or `"saved":
///   false` with an `error` when the file couldn't take it yet;
/// - `{"clients": true}` → `{"clients": [{id, name, grants, paired_at,
///   paired_at_unix_ms}]}`, `paired_at` in Unix seconds as the file keeps it
///   (for older readers) and `paired_at_unix_ms` the same in milliseconds.
///
/// [`CODE_FOR_US`]: crate::pairing::CODE_FOR_US
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
        let say_pin = code.get("say_pin") == Some(&json!(true));
        let grants: Result<BTreeSet<Grant>, _> =
            serde_json::from_value(code.get("grants").cloned().unwrap_or(json!([])));
        return match grants {
            Ok(grants) => {
                // The pairing screen's words for what is granted.
                let shown: Vec<&str> = grants.iter().map(|grant| grant.shown()).collect();
                let code = shared.door.lock().expect("door").issue_code(grants, now);
                // A new code always runs the full CODE_FOR_US from now.
                let until_unix_ms = wall_ms().saturating_add(crate::pairing::CODE_FOR_US / 1_000);
                let mut answer = json!({"code": code, "pin": shared.pin, "shown": shown, "until_unix_ms": until_unix_ms});
                if say_pin {
                    answer["pin_said"] = json!(said_pin(shared, now));
                }
                answer
            }
            Err(error) => json!({"error": format!("grants: {error}")}),
        };
    }
    // The owner's app takes the lamp back (#4579's P3): leases end as
    // `taken_by_owner_app`, the computers stay paired.
    if asked.get("take_back") == Some(&json!(true)) {
        let client = match asked.get("client") {
            None => None,
            Some(Value::String(client)) => Some(client.as_str()),
            Some(_) => return json!({"error": "client: a computer's id"}),
        };
        let changes = shared.door.lock().expect("door").take_back(client, now);
        let taken = changes
            .iter()
            .filter(|change| change.why == Some(crate::protocol::LeaseWhy::TakenByOwnerApp))
            .count();
        shared.tell(changes);
        eprintln!(
            "lamp-control: the owner's app took the lamp back from {}: {taken} lease(s)",
            client.unwrap_or("every computer")
        );
        return json!({"taken": taken});
    }
    if asked.get("cancel_code") == Some(&json!(true)) {
        let dropped = shared.door.lock().expect("door").cancel_code();
        eprintln!(
            "lamp-control: the owner's app dropped the pairing code (one was out: {dropped})"
        );
        return json!({"dropped": dropped});
    }
    if let Some(Value::String(client)) = asked.get("revoke") {
        let revoked = shared.door.lock().expect("door").revoke(client, now);
        shared.tell(revoked.changes);
        // Its pushed skills off (not deleted), whether or not it had any.
        shared.tell_helper();
        // What it asked for that still goes stops, as its `stop` would, and
        // its mute is let go.
        if revoked.was {
            shared.parts.lock().expect("parts").gone(client, now);
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
            .map(|client| {
                json!({
                    "id": client.id,
                    "name": client.name,
                    "grants": client.grants,
                    "paired_at": client.paired_at,
                    "paired_at_unix_ms": client.paired_at.saturating_mul(1_000),
                })
            })
            .collect();
        return json!({"clients": clients});
    }
    json!({"error": "ask for code, cancel_code, take_back, revoke or clients"})
}

/// Who says the spoken pin check, as the voice's say socket and a `stop`
/// know it: the lamp itself, never a computer.
const PIN_SPEAKER: &str = "lamp";

/// The spoken pin check (#5299): the first 8 characters of the lamp's pin,
/// the fingerprint of the certificate a computer will trust, said aloud in
/// two groups of four so the owner can match them with the app's. Whether
/// the voice took the line; the pin is in no journal line.
fn said_pin(shared: &Shared, now: u64) -> bool {
    let said = shared.parts.lock().expect("parts").act(
        PIN_SPEAKER,
        0,
        &Op::Say {
            text: spoken_pin(&shared.pin),
            rephrase: None,
        },
        now,
    );
    match said {
        Ok(_) => {
            eprintln!("lamp-control: the pairing check was given to the voice");
            true
        }
        Err(error) => {
            eprintln!(
                "lamp-control: the pairing check was not said: {}",
                error.why
            );
            false
        }
    }
}

/// The words of the spoken pin check: "My pairing check is 3 F 9 A, 0 B 2 C."
pub fn spoken_pin(pin: &str) -> String {
    let characters: Vec<String> = pin
        .chars()
        .take(8)
        .map(|character| character.to_ascii_uppercase().to_string())
        .collect();
    let (first, second) = characters.split_at(characters.len().min(4));
    format!(
        "My pairing check is {}, {}.",
        first.join(" "),
        second.join(" ")
    )
}

/// Milliseconds since the epoch, by the lamp's wall clock.
fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// The bus topics the door reads, each subscribed on its own thread, every
/// message handed to `watch` and what it means delivered.
pub const TOPICS: [&str; 22] = [
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
    "speaker/volume",
    "body/limits",
    "body/joint_states",
    "diagnostics",
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

    /// A door whose journal lines go to the list it gives back, and one
    /// connection on it, hello'd as `c_1` with the body grant, subscribed
    /// to `body.state` and `touch`, that never reads what is sent it.
    #[allow(clippy::type_complexity)]
    fn stalled() -> (
        Arc<Shared>,
        Arc<Mutex<Vec<String>>>,
        mpsc::UnboundedReceiver<Value>,
        watch::Receiver<Option<Value>>,
    ) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let into = lines.clone();
        let door = Door::new(
            "lamp-test",
            "Lamp",
            [0; 32],
            crate::pairing::Clients::default(),
            None,
        )
        .with_journal(crate::journal::Journal::to(move |line| {
            into.lock().unwrap().push(line.to_owned());
        }));
        let shared = Shared::new(door, Box::new(crate::session::Unrouted), String::new());
        let (outbox, inbox) = mpsc::unbounded_channel();
        let (frames, _) = mpsc::channel(FRAMES_QUEUED);
        let (body_state, newest) = watch::channel(None);
        shared.open.lock().unwrap().insert(
            1,
            Open {
                client: Some("c_1".to_owned()),
                outbox,
                frames,
                events: [EventKind::BodyState, EventKind::Touch].into(),
                phrases: Vec::new(),
                grants: [Grant::Body].into(),
                body_state,
            },
        );
        (shared, lines, inbox, newest)
    }

    fn body_state(yaw: f64) -> Out {
        Out::Event(
            EventKind::BodyState,
            json!({"joints": {"yaw": {"deg": yaw}}, "lamp": {}}),
        )
    }

    /// The Reviewer-lead's P2 on #4564: a stalled computer holds the newest
    /// `body.state` only, however many the body sent, and none in its
    /// outbox.
    #[test]
    fn a_stalled_computer_holds_one_body_state_the_newest() {
        let (shared, _, mut inbox, newest) = stalled();
        for at in 0..200 {
            shared.deliver(vec![body_state(f64::from(at))]);
        }
        assert!(inbox.try_recv().is_err(), "nothing queued in the outbox");
        let held = newest.borrow().clone().expect("one held");
        assert_eq!(held["ev"], "body.state");
        assert_eq!(held["joints"]["yaw"]["deg"], 199.0);
        // Other events still queue as they did.
        shared.deliver(vec![Out::Event(EventKind::Touch, json!({"kind": "tap"}))]);
        assert_eq!(inbox.try_recv().unwrap()["ev"], "touch");
    }

    /// El's P3 on #4564: `body.state` needs the `body` grant, and a
    /// connection without it is held none.
    #[test]
    fn body_state_goes_only_with_the_body_grant() {
        assert_eq!(EventKind::BodyState.needs(), Some(Grant::Body));
        let (shared, _, _inbox, newest) = stalled();
        shared
            .open
            .lock()
            .unwrap()
            .get_mut(&1)
            .unwrap()
            .grants
            .clear();
        shared.deliver(vec![body_state(1.0)]);
        assert!(newest.borrow().is_none(), "none held without the grant");
        shared.open.lock().unwrap().get_mut(&1).unwrap().grants = [Grant::Body].into();
        shared.deliver(vec![body_state(2.0)]);
        assert_eq!(
            newest.borrow().clone().expect("held")["joints"]["yaw"]["deg"],
            2.0
        );
    }

    /// The Reviewer-lead's P2 on #4564: `body.state` writes no journal line,
    /// so a computer's lines a second are all left for the rest.
    #[test]
    fn body_state_spends_none_of_the_journals_lines() {
        let (shared, lines, _inbox, _newest) = stalled();
        for at in 0..50 {
            shared.deliver(vec![body_state(f64::from(at))]);
        }
        for _ in 0..crate::journal::MOST_A_SECOND {
            shared.deliver(vec![Out::Event(EventKind::Touch, json!({"kind": "tap"}))]);
        }
        let lines = lines.lock().unwrap();
        assert!(
            !lines.iter().any(|line| line.contains("body.state")),
            "{lines:?}"
        );
        let touches = lines
            .iter()
            .filter(|line| line.ends_with("event touch sent"))
            .count();
        assert_eq!(touches as u32, crate::journal::MOST_A_SECOND, "{lines:?}");
    }

    #[test]
    fn only_the_lamps_own_network_may_connect() {
        let local = |text: &str| is_local(text.parse().unwrap());
        for near in ["127.0.0.1", "::1", "::ffff:127.0.0.1"] {
            assert!(local(near), "{near}");
        }
        // By its address alone, nothing else is local (Thirteen's P2s on
        // #4521): a VPN hands out private and link-local ones from
        // anywhere. The rest is judged by the interface (`is_local_peer`).
        for far in [
            "10.0.0.7",
            "172.16.4.2",
            "192.168.1.20",
            "169.254.3.3",
            "fd12:3456::1",
            "fe80::1",
            "::ffff:192.168.1.20",
            "8.8.8.8",
            "172.32.0.1",
            "100.64.0.1",
            "203.0.113.5",
            "2001:db8::1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(!local(far), "{far}");
        }
    }

    /// Thirteen's P2s on #4521: a peer is the lamp's own network by the
    /// interface its connection came in on, never its address alone. On
    /// lamp-75c9's Wi-Fi (172.168.20.0/23 on wlan0) the laptop is local; a
    /// ZeroTier peer (zt0, 10.147.17.0/24, and its own fe80::) is not, nor is
    /// a peer whose address happens to sit in wlan0's subnet when it came in
    /// by zt0.
    #[test]
    fn a_peer_is_local_by_the_interface_it_came_in_on() {
        let v4 = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                  wlan0\t0014A8AC\t00000000\t0001\t0\t0\t3006\t00FEFFFF\t0\t0\t0\n\
                  zt0\t0011930A\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n";
        let physical = |iface: &str| iface == "wlan0";
        let on_link = OnLink::parse(v4, "", physical);
        let scope = |index: u32| match index {
            3 => Some("wlan0".to_owned()),
            7 => Some("zt0".to_owned()),
            _ => None,
        };
        let local = |peer: &str, at: &str| {
            is_local_peer(
                peer.parse().unwrap(),
                at.parse().unwrap(),
                &on_link,
                scope,
                physical,
            )
        };
        assert!(
            local("172.168.21.8:50000", "172.168.20.40:7731"),
            "the laptop on the office Wi-Fi"
        );
        assert!(
            local("127.0.0.1:50000", "127.0.0.1:7731"),
            "the owner's ssh tunnel"
        );
        assert!(
            !local("10.147.17.9:50000", "10.147.17.2:7731"),
            "a ZeroTier peer"
        );
        assert!(
            !local("172.168.21.8:50000", "10.147.17.2:7731"),
            "a wlan0-looking address that came in by zt0"
        );
        assert!(
            local("[fe80::1%3]:50000", "[fe80::2%3]:7731"),
            "link-local on wlan0"
        );
        assert!(
            !local("[fe80::1%7]:50000", "[fe80::2%7]:7731"),
            "link-local on zt0"
        );
        assert!(
            !local("[fe80::1]:50000", "[fe80::2]:7731"),
            "link-local with no scope"
        );
        assert!(!local("8.8.8.8:50000", "172.168.20.40:7731"));
    }

    #[test]
    fn the_lamps_own_subnets_are_its_network_whatever_their_addresses() {
        // lamp-75c9, 30 Oct: 172.168.20.0/23 on wlan0, a default route.
        let v4 = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                  wlan0\t00000000\t0114A8AC\t0003\t0\t0\t3006\t00000000\t0\t0\t0\n\
                  wlan0\t0014A8AC\t00000000\t0001\t0\t0\t3006\t00FEFFFF\t0\t0\t0\n";
        let v6 = "fd001234000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001 wlan0\n\
                  00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200       lo\n";
        let on_link = OnLink::parse(v4, v6, |iface| iface == "wlan0");
        let holds = |text: &str| on_link.holds(text.parse().unwrap());
        assert!(holds("172.168.21.8"), "the laptop beside 75c9");
        assert!(holds("172.168.20.1"));
        assert!(!holds("172.168.22.1"), "the next /23");
        assert!(!holds("8.8.8.8"), "the default route is no subnet");
        assert!(holds("fd00:1234::7"));
        assert!(!holds("fd00:1235::7"));
        assert!(!holds("::1"), "lo is left out; loopback is local by itself");
        assert_eq!(OnLink::parse("", "", |_| true), OnLink::default());
    }

    /// Thirteen's P2 on #4521: wg-quick adds an on-link route on `wg0`
    /// (no gateway), and a tailnet or ZeroTier one is the same: a peer on it
    /// may be anywhere, so it is not the lamp's own network, and gets no
    /// camera with `--remote` off. Only the physical LAN's subnets are.
    #[test]
    fn a_vpns_on_link_route_is_not_the_lamps_own_network() {
        let v4 = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                  wlan0\t0014A8AC\t00000000\t0001\t0\t0\t3006\t00FEFFFF\t0\t0\t0\n\
                  wg0\t0000000A\t00000000\t0001\t0\t0\t0\t000000FF\t0\t0\t0\n\
                  zt0\t0000C0A8\t00000000\t0001\t0\t0\t0\t0000FFFF\t0\t0\t0\n";
        let physical = |iface: &str| iface == "wlan0";
        let on_link = OnLink::parse(v4, "", physical);
        let holds = |text: &str| on_link.holds(text.parse().unwrap());
        assert!(holds("172.168.21.8"), "the physical LAN");
        assert!(!holds("10.0.0.2"), "a WireGuard peer");
        assert!(!holds("192.168.4.4"), "a ZeroTier peer");
        for name in [
            "lo",
            "wg0",
            "tun0",
            "tailscale0",
            "zt5u4y6",
            "tap0",
            "docker0",
            "veth1a",
            "br-0f",
        ] {
            assert!(!is_physical(name), "{name}");
        }
    }

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
