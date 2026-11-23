//! The socket: dialing and the handshake (bounded by the caller's
//! deadline), then one thread that owns the socket, writes what callers
//! queue, reads every frame, and folds it into the shared state.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::CloseFrame;
use tungstenite::{Message, WebSocket};

use super::frames::{self, Hello, Inbound, event, method};
use super::runs::Runs;
use super::{ApprovalPolicy, Config, Decision, Logger};
use crate::Error;

/// Why a run ended by a drop: the words shared by every engine.
pub use crate::DROPPED;
/// How long after a drop a run of the kept session that is not one of ours
/// still going is aborted (OpenClaw's restart recovery re-runs the failed
/// task by itself, ee17 2026-11-23: 76 s after the gateway was ready, under
/// a new gateway run id; the gateway took about 3.5 min to start).
pub(crate) const STRAYS_FOR: Duration = Duration::from_secs(15 * 60);

/// How long one read waits before the thread looks at its queue again.
const POLL: Duration = Duration::from_millis(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) type Ws = WebSocket<TcpStream>;

/// What the socket thread and the caller share.
#[derive(Default)]
pub(crate) struct Shared {
    st: Mutex<State>,
    pub cv: Condvar,
}

impl Shared {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Default)]
pub(crate) struct State {
    /// Which connection is current; a thread of an older one changes
    /// nothing.
    pub generation: u64,
    pub up: bool,
    pub why_down: String,
    pub key: String,
    /// Replies awaited by id: `None` until the reply (or the loss) comes.
    pub rpcs: HashMap<String, Option<Result<Value, Error>>>,
    pub runs: Runs,
    /// The code of the last close the gateway sent (1012: a restart).
    pub last_close: Option<u16>,
    /// Until when a stray run of the kept session is aborted, after a drop
    /// failed ours ([`DROPPED`], [`STRAYS_FOR`]).
    pub strays_until: Option<Instant>,
    /// The stray runs already aborted.
    pub strays_aborted: std::collections::HashSet<String>,
    io_n: u64,
}

/// A failed dial, with how soon the gateway said to try again
/// (`retryAfterMs` while starting).
#[derive(Debug)]
pub(crate) struct DialError {
    pub error: Error,
    pub retry_after: Option<Duration>,
    /// The gateway answered too slowly (no challenge, hello or first reply
    /// in time): a busy gateway, not a failure that grows the backoff.
    pub slow: bool,
}

impl From<Error> for DialError {
    fn from(error: Error) -> Self {
        DialError {
            error,
            retry_after: None,
            slow: false,
        }
    }
}

/// How soon to dial again after a gateway that is still starting.
const STARTING_RETRY: Duration = Duration::from_millis(500);

fn rpc_refusal(err: &frames::RpcError) -> DialError {
    if err.unavailable() {
        DialError {
            error: Error::NotReady(err.words()),
            retry_after: Some(
                err.retry_after_ms
                    .map(Duration::from_millis)
                    .unwrap_or(STARTING_RETRY),
            ),
            slow: false,
        }
    } else {
        Error::Refused(err.words()).into()
    }
}

/// One live connection's thread and its queue.
pub(crate) struct Link {
    pub tx: Sender<String>,
    pub generation: u64,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn left(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
}

fn timed_out(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

fn host_port(url: &str) -> Result<String, Error> {
    let rest = url
        .strip_prefix("ws://")
        .ok_or_else(|| Error::Refused(format!("only ws:// is spoken here, not {url}")))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    Ok(if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    })
}

fn close_words(frame: Option<&CloseFrame>) -> (u16, String) {
    match frame {
        Some(f) => (u16::from(f.code), f.reason.to_string()),
        None => (1005, String::new()),
    }
}

/// A close before hello: 1013 is a gateway still starting; 1008 refuses.
fn closed_early(frame: Option<&CloseFrame>) -> DialError {
    let (code, reason) = close_words(frame);
    if code == frames::CLOSE_STARTING {
        DialError {
            error: Error::NotReady(format!("the gateway is starting ({code} {reason})")),
            retry_after: Some(STARTING_RETRY),
            slow: false,
        }
    } else if code == 1008 {
        Error::Refused(format!("closed by the gateway ({code} {reason})")).into()
    } else {
        Error::NotReady(format!("closed by the gateway ({code} {reason})")).into()
    }
}

/// Reads the next text frame before `deadline`.
fn read_text(ws: &mut Ws, deadline: Instant, what: &str) -> Result<String, DialError> {
    loop {
        let Some(wait) = left(deadline) else {
            return Err(DialError {
                error: Error::NotReady(format!("no {what} in time")),
                retry_after: None,
                slow: true,
            });
        };
        let _ = ws.get_ref().set_read_timeout(Some(wait.min(POLL * 10)));
        match ws.read() {
            Ok(Message::Text(t)) => return Ok(t.to_string()),
            Ok(Message::Close(f)) => return Err(closed_early(f.as_ref())),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if timed_out(&e) => {}
            Err(e) => return Err(Error::NotReady(format!("reading the {what}: {e}")).into()),
        }
    }
}

/// Dials, takes the challenge, connects with the token, and reads hello:
/// all before `deadline`.
pub(crate) fn dial(cfg: &Config, deadline: Instant) -> Result<(Ws, Hello), DialError> {
    let token = std::fs::read_to_string(&cfg.token_path)
        .map(|t| t.trim().to_owned())
        .unwrap_or_default();
    if token.is_empty() {
        return Err(Error::NotReady(format!(
            "no gateway token yet in {}",
            cfg.token_path.display()
        ))
        .into());
    }
    let authority = host_port(&cfg.url)?;
    let addr = authority
        .to_socket_addrs()
        .map_err(|e| Error::NotReady(format!("{authority}: {e}")))?
        .next()
        .ok_or_else(|| Error::NotReady(format!("{authority}: no address")))?;
    let wait = left(deadline).ok_or_else(|| Error::NotReady("no time to connect".to_owned()))?;
    let stream = TcpStream::connect_timeout(&addr, wait)
        .map_err(|e| Error::NotReady(format!("{authority}: {e}")))?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(wait));
    let _ = stream.set_write_timeout(Some(wait.min(WRITE_TIMEOUT)));
    let request = cfg
        .url
        .as_str()
        .into_client_request()
        .map_err(|e| Error::Refused(format!("{}: {e}", cfg.url)))?;
    let mut ws = match tungstenite::client(request, stream) {
        Ok((ws, _)) => ws,
        Err(tungstenite::HandshakeError::Failure(e)) => {
            return Err(Error::NotReady(format!("websocket upgrade: {e}")).into());
        }
        Err(tungstenite::HandshakeError::Interrupted(_)) => {
            return Err(Error::NotReady("websocket upgrade timed out".to_owned()).into());
        }
    };

    // The challenge first; this path has no device, so its nonce is not
    // signed, but the gateway expects connect only after it.
    loop {
        let text = read_text(&mut ws, deadline, "connect.challenge")?;
        match frames::parse(&text) {
            Inbound::Event(e) if e.name == event::CHALLENGE => break,
            // A gateway still starting answers before any challenge
            // (`UNAVAILABLE`, `startup-sidecars`), then closes 1013.
            Inbound::Res(res) => {
                if let Some(err) = res.error {
                    return Err(rpc_refusal(&err));
                }
            }
            _ => {}
        }
    }
    let connect = frames::connect(
        &cfg.client_id,
        &cfg.client_version,
        &token,
        cfg.approval_events,
    );
    ws.send(Message::text(connect))
        .map_err(|e| Error::NotReady(format!("writing connect: {e}")))?;
    let hello = loop {
        let text = read_text(&mut ws, deadline, "connect reply")?;
        let Inbound::Res(res) = frames::parse(&text) else {
            continue;
        };
        if res.id != frames::CONNECT_ID {
            continue;
        }
        if let Some(err) = res.error {
            return Err(rpc_refusal(&err));
        }
        break frames::hello(&res.payload);
    };
    let missing: Vec<&str> = frames::NEEDED_SCOPES
        .iter()
        .copied()
        .filter(|s| !hello.scopes.iter().any(|g| g == s))
        .collect();
    if !missing.is_empty() {
        let _ = ws.close(None);
        return Err(Error::Refused(format!(
            "granted scopes [{}] lack {}",
            hello.scopes.join(","),
            missing.join(",")
        ))
        .into());
    }
    let _ = ws.get_ref().set_read_timeout(Some(POLL));
    let _ = ws.get_ref().set_write_timeout(Some(WRITE_TIMEOUT));
    Ok((ws, hello))
}

/// Starts the socket thread for connection `generation`.
pub(crate) fn spawn(
    ws: Ws,
    hello: &Hello,
    shared: Arc<Shared>,
    generation: u64,
    policy: ApprovalPolicy,
    log: Logger,
) -> Link {
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let thread = {
        let stop = stop.clone();
        let dead_after = Duration::from_millis(hello.tick_ms.saturating_mul(2));
        std::thread::Builder::new()
            .name("openclaw-gateway".to_owned())
            .spawn(move || {
                let mut io = Io {
                    ws,
                    rx,
                    shared,
                    generation,
                    policy,
                    log,
                    last_seq: None,
                };
                let why = io.run(&stop, dead_after);
                io.down(&why);
            })
            .expect("spawn the gateway thread")
    };
    Link {
        tx,
        generation,
        stop,
        thread: Some(thread),
    }
}

struct Io {
    ws: Ws,
    rx: Receiver<String>,
    shared: Arc<Shared>,
    generation: u64,
    policy: ApprovalPolicy,
    log: Logger,
    last_seq: Option<u64>,
}

impl Io {
    fn run(&mut self, stop: &AtomicBool, dead_after: Duration) -> String {
        let mut last_in = Instant::now();
        loop {
            if stop.load(Ordering::Acquire) {
                let _ = self.ws.close(None);
                let _ = self.ws.flush();
                return "closed by lamp-tasks".to_owned();
            }
            while let Ok(out) = self.rx.try_recv() {
                if let Err(e) = self.ws.send(Message::text(out)) {
                    return format!("write: {e}");
                }
            }
            match self.ws.read() {
                Ok(Message::Text(t)) => {
                    last_in = Instant::now();
                    match self.handle(&t) {
                        Ok(outs) => {
                            for out in outs {
                                if let Err(e) = self.ws.send(Message::text(out)) {
                                    return format!("write: {e}");
                                }
                            }
                        }
                        Err((outs, why)) => {
                            for out in outs {
                                let _ = self.ws.send(Message::text(out));
                            }
                            return why;
                        }
                    }
                }
                Ok(Message::Close(f)) => {
                    let (code, reason) = close_words(f.as_ref());
                    let mut st = self.shared.lock();
                    if st.generation == self.generation {
                        st.last_close = Some(code);
                    }
                    drop(st);
                    return format!("closed by the gateway ({code} {reason})");
                }
                Ok(_) => last_in = Instant::now(),
                Err(tungstenite::Error::Io(e)) if timed_out(&e) => {
                    let _ = self.ws.flush();
                }
                Err(e) => return format!("read: {e}"),
            }
            if last_in.elapsed() > dead_after {
                return format!(
                    "nothing from the gateway for {}s (no tick)",
                    dead_after.as_secs()
                );
            }
        }
    }

    /// The connection is gone: callers waiting on it learn so.
    fn down(&mut self, why: &str) {
        let mut st = self.shared.lock();
        let mut failed = Vec::new();
        if st.generation == self.generation {
            st.up = false;
            st.why_down = why.to_owned();
            // Every run in flight fails at once, the person told to try
            // again (the owner's call, 2026-11-23): no wait for the
            // gateway's restart recovery, whose re-run is aborted as a
            // stray once back ([`Self::abort_stray`]).
            failed = st.runs.fail_open();
            if !failed.is_empty() {
                st.strays_until = Some(Instant::now() + STRAYS_FOR);
            }
            for slot in st.rpcs.values_mut() {
                if slot.is_none() {
                    *slot = Some(Err(Error::Lost(why.to_owned())));
                }
            }
        }
        drop(st);
        self.shared.cv.notify_all();
        (self.log)(&format!("connection down: {why}"));
        for run in failed {
            (self.log)(&format!("run {} ended by the drop", run.0));
        }
    }

    /// One frame in; frames to write back out, or why to drop the
    /// connection.
    fn handle(&mut self, text: &str) -> Result<Vec<String>, (Vec<String>, String)> {
        let mut outs = Vec::new();
        match frames::parse(text) {
            Inbound::Res(res) => {
                let mut st = self.shared.lock();
                if let Some(slot) = st.rpcs.get_mut(&res.id) {
                    *slot = Some(match res.error {
                        None => Ok(res.payload),
                        Some(e) if e.unavailable() => Err(Error::NotReady(e.words())),
                        Some(e) => Err(Error::Refused(e.words())),
                    });
                }
                drop(st);
                self.shared.cv.notify_all();
            }
            Inbound::Event(e) => {
                let gap = match (e.seq, self.last_seq) {
                    (Some(seq), Some(prev)) => seq != prev + 1,
                    _ => false,
                };
                if let Some(seq) = e.seq {
                    self.last_seq = Some(seq);
                }
                // A gap means lost frames and there is no replay: drop and
                // reconnect (a resync follows). The frame that shows the gap
                // is still applied when it is authoritative on its own (a chat
                // terminal, an approval prompt that must be answered); an
                // agent frame is not (its text baseline may be missing).
                match e.name.as_str() {
                    event::AGENT | event::CHAT if self.abort_stray(&e.payload, &mut outs) => {}
                    event::AGENT if gap => {}
                    event::AGENT => {
                        if let Some(evt) = frames::agent_event(&e.payload) {
                            self.shared.lock().runs.on_agent(evt);
                            self.shared.cv.notify_all();
                        }
                    }
                    event::CHAT => {
                        if let Some(evt) = frames::chat_event(&e.payload) {
                            self.shared.lock().runs.on_chat(evt);
                            self.shared.cv.notify_all();
                        }
                    }
                    event::EXEC_APPROVAL => {
                        if let Some(ask) = frames::approval_request(&e.payload) {
                            let id = ask.id.clone();
                            let decision = (self.policy)(&ask);
                            (self.log)(&format!(
                                "exec approval {id} for `{}`: {}",
                                super::runs::brief(&Value::String(ask.command.clone())),
                                decision.word()
                            ));
                            outs.push(self.io_req(
                                method::EXEC_APPROVAL_RESOLVE,
                                frames::approval_resolve(&id, decision.word()),
                            ));
                        }
                    }
                    event::PLUGIN_APPROVAL => {
                        if let Some(id) = frames::str_of(&e.payload, "id") {
                            (self.log)(&format!("plugin approval {id}: deny"));
                            outs.push(self.io_req(
                                method::PLUGIN_APPROVAL_RESOLVE,
                                frames::approval_resolve(&id, Decision::Deny.word()),
                            ));
                        }
                    }
                    _ => {}
                }
                if gap {
                    return Err((
                        outs,
                        format!("event sequence gap at {}", e.seq.unwrap_or(0)),
                    ));
                }
            }
            Inbound::Other => {}
        }
        Ok(outs)
    }

    /// After a drop failed our runs: an event of the kept session for a
    /// run that is not one of ours still going (the gateway's restart
    /// recovery re-running a failed task, under our run id or its own) gets
    /// that run aborted once, so it never holds the next task back
    /// (ee17 2026-11-23: a short task waited 4 min behind it). Whether it
    /// was a stray (its event then changes nothing).
    fn abort_stray(&mut self, payload: &Value, outs: &mut Vec<String>) -> bool {
        let Some(run) = frames::str_of(payload, "runId") else {
            return false;
        };
        let mut st = self.shared.lock();
        let ours = frames::str_of(payload, "sessionKey").as_deref() == Some(st.key.as_str());
        let watching = st.strays_until.is_some_and(|until| Instant::now() < until);
        if !(ours && watching && st.runs.stray(&run)) {
            return false;
        }
        let first = st.strays_aborted.insert(run.clone());
        let key = st.key.clone();
        drop(st);
        if first {
            (self.log)(&format!("stray run {run} after the drop: aborted"));
            outs.push(self.io_req(method::CHAT_ABORT, frames::chat_abort(&key, Some(&run))));
        }
        true
    }

    fn io_req(&self, method: &str, params: Value) -> String {
        let mut st = self.shared.lock();
        st.io_n += 1;
        let id = format!("lamp-io-{}", st.io_n);
        frames::req(&id, method, params)
    }
}
