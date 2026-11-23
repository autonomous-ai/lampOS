//! A scripted OpenClaw gateway for tests: a real tungstenite server on
//! 127.0.0.1:0 that speaks protocol 4 as the pinned gateway does (closed
//! connect schema, scopes cleared for a non-`gateway-client` id with no
//! device, startup `UNAVAILABLE`, close 1013, per-connection `seq`, ticks),
//! one connection at a time, and runs a test's script per method.

use std::collections::{HashMap, VecDeque};
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{Message, WebSocket};

pub(crate) const TOKEN: &str = "fake-token-0123";

/// Given a request frame, the frames to write back (its `res` among them,
/// in the order they go).
pub(crate) type Script = Box<dyn FnMut(&Value) -> Vec<Value> + Send>;

#[derive(Default)]
pub(crate) struct Knobs {
    /// Connects answered with a retryable `UNAVAILABLE` (startup sidecars).
    pub starting: u32,
    /// Connections closed with 1013 before the challenge.
    pub close_1013: u32,
    /// The tick interval said in hello.
    pub tick_ms: u64,
    /// Stops ticks and replies (a hung gateway).
    pub silent: bool,
    /// Skips one outer `seq` number on the next event.
    pub skip_seq: bool,
    /// Holds each connection's challenge this long (a busy gateway).
    pub challenge_delay: Duration,
}

#[derive(Default)]
struct St {
    knobs: Knobs,
    scripts: HashMap<String, Script>,
    outbox: VecDeque<Value>,
    reqs: Vec<Value>,
    conns: u32,
    drop_now: bool,
    /// Closes the current connection with this code, after the frames
    /// pushed before it (1012: a service restart).
    close_with: Option<u16>,
}

#[derive(Default)]
struct Inner {
    st: Mutex<St>,
    cv: Condvar,
    stop: AtomicBool,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, St> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub(crate) struct Fake {
    pub addr: SocketAddr,
    pub dir: tempfile::TempDir,
    inner: Arc<Inner>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.addr);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Connect params' allowed fields: the gateway's closed schema.
const CONNECT_FIELDS: [&str; 12] = [
    "minProtocol",
    "maxProtocol",
    "client",
    "role",
    "scopes",
    "caps",
    "auth",
    "device",
    "locale",
    "userAgent",
    "pathEnv",
    "permissions",
];
const CLIENT_FIELDS: [&str; 7] = [
    "id",
    "version",
    "platform",
    "mode",
    "displayName",
    "deviceFamily",
    "instanceId",
];

/// Why the gateway's schema refuses these connect params, if it does.
pub(crate) fn schema_refusal(params: &Value) -> Option<String> {
    let o = params.as_object()?;
    if let Some(k) = o.keys().find(|k| !CONNECT_FIELDS.contains(&k.as_str())) {
        return Some(format!("unexpected property {k}"));
    }
    let client = o.get("client").and_then(Value::as_object)?;
    if let Some(k) = client.keys().find(|k| !CLIENT_FIELDS.contains(&k.as_str())) {
        return Some(format!("unexpected property client.{k}"));
    }
    if o.get("minProtocol").and_then(Value::as_u64) != Some(4)
        || o.get("maxProtocol").and_then(Value::as_u64) != Some(4)
    {
        return Some("protocol mismatch".to_owned());
    }
    None
}

impl Fake {
    pub(crate) fn start() -> Fake {
        Fake::with(Knobs {
            tick_ms: 15_000,
            ..Knobs::default()
        })
    }

    pub(crate) fn with(knobs: Knobs) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("gateway-token"), format!("{TOKEN}\n")).expect("token");
        let inner = Arc::new(Inner::default());
        inner.lock().knobs = knobs;
        let thread = {
            let inner = inner.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if inner.stop.load(Ordering::Acquire) {
                        return;
                    }
                    if let Ok(stream) = stream {
                        serve(&inner, stream);
                    }
                }
            })
        };
        Fake {
            addr,
            dir,
            inner,
            thread: Some(thread),
        }
    }

    pub(crate) fn url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    pub(crate) fn token_path(&self) -> std::path::PathBuf {
        self.dir.path().join("gateway-token")
    }

    pub(crate) fn knobs(&self, f: impl FnOnce(&mut Knobs)) {
        f(&mut self.inner.lock().knobs);
    }

    pub(crate) fn on(
        &self,
        method: &str,
        script: impl FnMut(&Value) -> Vec<Value> + Send + 'static,
    ) {
        self.inner
            .lock()
            .scripts
            .insert(method.to_owned(), Box::new(script));
    }

    /// Frames to write to the current connection.
    pub(crate) fn push(&self, frames: impl IntoIterator<Item = Value>) {
        self.inner.lock().outbox.extend(frames);
    }

    /// Drops the current connection's TCP at once (a gateway restart).
    pub(crate) fn drop_conn(&self) {
        self.inner.lock().drop_now = true;
    }

    /// Closes the current connection with 1012 (service restart), as the
    /// gateway does at a restart, after the frames pushed before it.
    pub(crate) fn restart(&self) {
        self.inner.lock().close_with = Some(1012);
    }

    pub(crate) fn conns(&self) -> u32 {
        self.inner.lock().conns
    }

    pub(crate) fn reqs(&self, method: &str) -> Vec<Value> {
        self.inner
            .lock()
            .reqs
            .iter()
            .filter(|r| r.get("method").and_then(Value::as_str) == Some(method))
            .cloned()
            .collect()
    }

    /// Waits for the `n`th (1-based) request of `method`.
    pub(crate) fn wait_req(&self, method: &str, n: usize, within: Duration) -> Value {
        let deadline = Instant::now() + within;
        let mut st = self.inner.lock();
        loop {
            let found: Vec<&Value> = st
                .reqs
                .iter()
                .filter(|r| r.get("method").and_then(Value::as_str) == Some(method))
                .collect();
            if found.len() >= n {
                return found[n - 1].clone();
            }
            let wait = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("no {method} #{n} within {within:?}"));
            st = self.inner.cv.wait_timeout(st, wait).expect("lock").0;
        }
    }
}

pub(crate) fn res(req: &Value, payload: Value) -> Value {
    json!({"type": "res", "id": req["id"], "ok": true, "payload": payload})
}

pub(crate) fn res_err(req: &Value, code: &str, message: &str, details: Value) -> Value {
    json!({"type": "res", "id": req["id"], "ok": false,
           "error": {"code": code, "message": message, "details": details}})
}

pub(crate) fn agent(run: &str, stream: &str, data: Value) -> Value {
    json!({"type": "event", "event": "agent",
           "payload": {"runId": run, "stream": stream, "sessionKey": "agent:main:lamp", "data": data}})
}

pub(crate) fn chat(run: &str, state: &str, extra: Value) -> Value {
    let mut payload = json!({"runId": run, "sessionKey": "agent:main:lamp", "state": state});
    if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    json!({"type": "event", "event": "chat", "payload": payload})
}

pub(crate) fn event(name: &str, payload: Value) -> Value {
    json!({"type": "event", "event": name, "payload": payload})
}

fn idle(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
}

struct Conn {
    ws: WebSocket<TcpStream>,
    seq: u64,
}

impl Conn {
    fn send(&mut self, inner: &Inner, mut frame: Value) -> bool {
        if frame.get("type").and_then(Value::as_str) == Some("event")
            && frame.get("event").and_then(Value::as_str) != Some("connect.challenge")
        {
            let mut st = inner.lock();
            if std::mem::take(&mut st.knobs.skip_seq) {
                self.seq += 1;
            }
            drop(st);
            self.seq += 1;
            frame["seq"] = json!(self.seq);
        }
        self.ws.send(Message::text(frame.to_string())).is_ok()
    }
}

fn read_req(ws: &mut WebSocket<TcpStream>, within: Duration) -> Option<Value> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        match ws.read() {
            Ok(Message::Text(t)) => return serde_json::from_str(&t).ok(),
            Ok(_) => {}
            Err(e) if idle(&e) => {}
            Err(_) => return None,
        }
    }
    None
}

fn serve(inner: &Inner, stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(ws) = tungstenite::accept(stream) else {
        return;
    };
    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(10)));
    let mut c = Conn { ws, seq: 0 };
    {
        let mut st = inner.lock();
        st.conns += 1;
        st.drop_now = false;
        if st.knobs.close_1013 > 0 {
            st.knobs.close_1013 -= 1;
            drop(st);
            let _ = c.ws.close(Some(CloseFrame {
                code: CloseCode::Again,
                reason: "gateway starting".into(),
            }));
            let _ = c.ws.flush();
            let _ = read_req(&mut c.ws, Duration::from_millis(200));
            return;
        }
    }
    let delay = inner.lock().knobs.challenge_delay;
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    let challenge = json!({"type": "event", "event": "connect.challenge",
                           "payload": {"nonce": "nonce-1", "ts": 1_737_264_000_000u64}});
    c.send(inner, challenge);
    let Some(connect) = read_req(&mut c.ws, Duration::from_secs(5)) else {
        return;
    };
    record(inner, &connect);
    let params = connect.get("params").cloned().unwrap_or(Value::Null);
    let reply = if let Some(why) = schema_refusal(&params) {
        Err(res_err(&connect, "INVALID_REQUEST", &why, json!({})))
    } else if take_starting(inner) {
        Err(json!({"type": "res", "id": connect["id"], "ok": false,
                   "error": {"code": "UNAVAILABLE", "message": "gateway is starting",
                             "details": {"reason": "startup-sidecars"},
                             "retryable": true, "retryAfterMs": 200}}))
    } else if params["auth"]["token"].as_str() != Some(TOKEN) {
        Err(res_err(
            &connect,
            "INVALID_REQUEST",
            "unauthorized: gateway token mismatch",
            json!({"reason": "token_mismatch"}),
        ))
    } else {
        let no_device = params.get("device").is_none();
        let scopes = if params["client"]["id"] != "gateway-client" && no_device {
            json!([])
        } else {
            params["scopes"].clone()
        };
        let tick = inner.lock().knobs.tick_ms;
        Ok(res(
            &connect,
            json!({"type": "hello-ok", "protocol": 4,
                   "server": {"version": "2026.11.2", "connId": "c1", "bootId": "b1"},
                   "features": {"methods": [], "events": []}, "snapshot": {},
                   "auth": {"role": "operator", "scopes": scopes},
                   "policy": {"maxPayload": 26_214_400u64, "tickIntervalMs": tick}}),
        ))
    };
    match reply {
        Ok(hello) => {
            c.send(inner, hello);
        }
        Err(refusal) => {
            c.send(inner, refusal);
            let _ = c.ws.close(None);
            let _ = c.ws.flush();
            return;
        }
    }

    let mut last_tick = Instant::now();
    loop {
        if inner.stop.load(Ordering::Acquire) {
            return;
        }
        let (out, drop_now, silent, tick_ms) = {
            let mut st = inner.lock();
            let out: Vec<Value> = st.outbox.drain(..).collect();
            (
                out,
                std::mem::take(&mut st.drop_now),
                st.knobs.silent,
                st.knobs.tick_ms,
            )
        };
        if drop_now {
            return; // the TCP goes with `c`, no close frame
        }
        for frame in out {
            if !c.send(inner, frame) {
                return;
            }
        }
        if let Some(code) = inner.lock().close_with.take() {
            let _ = c.ws.close(Some(CloseFrame {
                code: CloseCode::from(code),
                reason: "service restart".into(),
            }));
            let _ = c.ws.flush();
            return;
        }
        if !silent && last_tick.elapsed() >= Duration::from_millis(tick_ms) {
            last_tick = Instant::now();
            c.send(inner, event("tick", json!({"ts": 0})));
        }
        match c.ws.read() {
            Ok(Message::Text(t)) => {
                let Ok(req) = serde_json::from_str::<Value>(&t) else {
                    continue;
                };
                record(inner, &req);
                if silent {
                    continue;
                }
                let method = req["method"].as_str().unwrap_or("").to_owned();
                let frames = {
                    let mut st = inner.lock();
                    match st.scripts.get_mut(&method) {
                        Some(script) => script(&req),
                        None => vec![default_reply(&req, &method)],
                    }
                };
                for frame in frames {
                    if !c.send(inner, frame) {
                        return;
                    }
                }
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(e) if idle(&e) => {}
            Err(_) => return,
        }
    }
}

fn take_starting(inner: &Inner) -> bool {
    let mut st = inner.lock();
    let starting = st.knobs.starting > 0;
    if starting {
        st.knobs.starting -= 1;
    }
    starting
}

fn record(inner: &Inner, req: &Value) {
    inner.lock().reqs.push(req.clone());
    inner.cv.notify_all();
}

fn default_reply(req: &Value, method: &str) -> Value {
    match method {
        "sessions.list" => res(req, json!({"sessions": []})),
        "chat.send" => res(
            req,
            json!({"runId": req["params"]["idempotencyKey"], "status": "started"}),
        ),
        "chat.history" => res(req, json!({"messages": []})),
        _ => res(req, json!({})),
    }
}
