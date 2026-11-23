//! A scripted Hermes API server for tests: plain HTTP/1.1 on 127.0.0.1:0,
//! speaking the pinned v0.22.2 routes and shapes the session uses
//! (`/health`, `/v1/capabilities`, `/v1/runs` with `Idempotency-Key`
//! replay, the run's chunked SSE with `seq`, `: open`/`: keepalive`/
//! `: stream closed` comments and `Last-Event-ID` replay, `GET
//! /v1/runs/{id}`, `/stop`, `/approval`, `/api/sessions[/{id}[/messages]]`).
//! A test pushes each run's events; it can kill the server (connections
//! refused), restart it (runs not settled become `interrupted`, their
//! streams gone), or break the open streams.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::NEEDED_FEATURES;

pub(crate) const KEY: &str = "fake-key-0123";

#[derive(Default)]
pub(crate) struct Knobs {
    /// `/health` answers 503.
    pub health_down: bool,
    /// A Runs feature left out of `/v1/capabilities`.
    pub missing_feature: Option<&'static str>,
    /// `POST /v1/runs` takes the run, then holds its reply this long.
    pub runs_delay: Duration,
    /// `POST /v1/runs` answers this status (an error) instead.
    pub runs_status: Option<u16>,
    /// `/stop` answers `stopping` and the run goes on.
    pub stop_ignored: bool,
}

/// One request as the server read it.
#[derive(Clone, Debug)]
pub(crate) struct Rec {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl Rec {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

struct FRun {
    id: String,
    session_id: String,
    status: String,
    events: Vec<Value>,
    extra: Value,
    /// The stream is gone (a restarted server has none).
    stream_gone: bool,
}

#[derive(Default)]
struct St {
    knobs: Knobs,
    runs: Vec<FRun>,
    idem: HashMap<String, String>,
    sessions: HashMap<String, Vec<Value>>,
    reqs: Vec<Rec>,
    next_run: u32,
    /// Bumped by a kill: connections of an older one close.
    generation: u64,
    /// Bumped by [`Fake::break_streams`].
    stream_gen: u64,
    alive: bool,
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
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Release);
        self.kill();
    }
}

impl Fake {
    pub(crate) fn start() -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("api-server-key"), format!("{KEY}\n")).expect("key");
        let inner = Arc::new(Inner::default());
        let fake = Fake { addr, dir, inner };
        fake.listen(listener);
        fake
    }

    fn listen(&self, listener: TcpListener) {
        let inner = self.inner.clone();
        let generation = {
            let mut st = inner.lock();
            st.alive = true;
            st.generation
        };
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if inner.stop.load(Ordering::Acquire) || inner.lock().generation != generation {
                    return;
                }
                if let Ok(stream) = stream {
                    let inner = inner.clone();
                    std::thread::spawn(move || serve(&inner, stream, generation));
                }
            }
        });
    }

    pub(crate) fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub(crate) fn key_path(&self) -> std::path::PathBuf {
        self.dir.path().join("api-server-key")
    }

    pub(crate) fn state_path(&self) -> std::path::PathBuf {
        self.dir.path().join("hermes-direct.json")
    }

    pub(crate) fn knobs(&self, f: impl FnOnce(&mut Knobs)) {
        f(&mut self.inner.lock().knobs);
    }

    /// The server stops: open connections close, new ones are refused.
    pub(crate) fn kill(&self) {
        {
            let mut st = self.inner.lock();
            if !st.alive {
                return;
            }
            st.alive = false;
            st.generation += 1;
        }
        self.inner.cv.notify_all();
        // Wakes the accept loop so it lets go of the port.
        let _ = TcpStream::connect(self.addr);
        std::thread::sleep(Duration::from_millis(50));
    }

    /// Serves again on the same port.
    pub(crate) fn revive(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let listener = loop {
            match TcpListener::bind(self.addr) {
                Ok(l) => break l,
                Err(e) if Instant::now() < deadline => {
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => panic!("rebind {}: {e}", self.addr),
            }
        };
        self.listen(listener);
    }

    /// As a gateway restart: every run not settled becomes `interrupted`
    /// (`_durable_run_status`, owner gone), no stream survives.
    pub(crate) fn restart(&self) {
        self.kill();
        {
            let mut st = self.inner.lock();
            for r in &mut st.runs {
                r.stream_gone = true;
                if !super::runs::TERMINAL.contains(&r.status.as_str()) {
                    r.status = "interrupted".to_owned();
                    r.extra = json!({"error": "The gateway restarted before this run settled."});
                }
            }
        }
        self.revive();
    }

    /// Closes every open event stream mid-body; the runs go on.
    pub(crate) fn break_streams(&self) {
        self.inner.lock().stream_gen += 1;
        self.inner.cv.notify_all();
    }

    /// One event on `run`'s stream (`seq` and the envelope added); a
    /// terminal one settles the run's status.
    pub(crate) fn push(&self, run: &str, name: &str, extra: Value) {
        let mut st = self.inner.lock();
        let r = st
            .runs
            .iter_mut()
            .find(|r| r.id == run)
            .unwrap_or_else(|| panic!("no run {run}"));
        let mut evt = json!({"event": name, "run_id": run, "timestamp": 1.0,
                             "seq": r.events.len()});
        if let (Some(o), Some(e)) = (evt.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        if let Some(status) = name.strip_prefix("run.")
            && super::runs::TERMINAL.contains(&status)
        {
            r.status = status.to_owned();
            r.extra = extra;
        }
        r.events.push(evt);
        drop(st);
        self.inner.cv.notify_all();
    }

    /// Settles `run`'s status without an event (one lost with a stream).
    pub(crate) fn settle_quietly(&self, run: &str, status: &str, extra: Value) {
        let mut st = self.inner.lock();
        if let Some(r) = st.runs.iter_mut().find(|r| r.id == run) {
            r.status = status.to_owned();
            r.extra = extra;
        }
    }

    pub(crate) fn run_ids(&self) -> Vec<String> {
        self.inner
            .lock()
            .runs
            .iter()
            .map(|r| r.id.clone())
            .collect()
    }

    pub(crate) fn set_messages(&self, id: &str, messages: Vec<Value>) {
        self.inner.lock().sessions.insert(id.to_owned(), messages);
    }

    pub(crate) fn has_session(&self, id: &str) -> bool {
        self.inner.lock().sessions.contains_key(id)
    }

    /// Requests of `method` whose path starts with `path`.
    pub(crate) fn reqs(&self, method: &str, path: &str) -> Vec<Rec> {
        self.inner
            .lock()
            .reqs
            .iter()
            .filter(|r| r.method == method && r.path.starts_with(path))
            .cloned()
            .collect()
    }

    /// Waits for the `n`th (1-based) such request.
    pub(crate) fn wait_req(&self, method: &str, path: &str, n: usize, within: Duration) -> Rec {
        let deadline = Instant::now() + within;
        let mut st = self.inner.lock();
        loop {
            let found: Vec<&Rec> = st
                .reqs
                .iter()
                .filter(|r| r.method == method && r.path.starts_with(path))
                .collect();
            if found.len() >= n {
                return found[n - 1].clone();
            }
            let wait = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| panic!("no {method} {path} #{n} within {within:?}"));
            st = self.inner.cv.wait_timeout(st, wait).expect("lock").0;
        }
    }
}

fn read_req(stream: &mut TcpStream) -> Option<Rec> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    Some(Rec {
        method,
        path,
        headers,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    })
}

fn reply(stream: &mut TcpStream, status: u16, body: &Value, extra: &[(&str, &str)]) {
    let body = body.to_string();
    let mut head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn error(code: &str, message: &str) -> Value {
    json!({"error": {"message": message, "type": "invalid_request_error", "param": null, "code": code}})
}

fn status_of(r: &FRun) -> Value {
    let mut v = json!({"object": "hermes.run", "run_id": r.id, "status": r.status,
                       "session_id": r.session_id, "updated_at": 1.0});
    if let (Some(o), Some(e)) = (v.as_object_mut(), r.extra.as_object()) {
        o.extend(e.clone());
    }
    v
}

fn serve(inner: &Inner, mut stream: TcpStream, generation: u64) {
    let Some(req) = read_req(&mut stream) else {
        return;
    };
    {
        let mut st = inner.lock();
        if st.generation != generation || !st.alive {
            return;
        }
        st.reqs.push(req.clone());
    }
    inner.cv.notify_all();
    let path = req.path.split('?').next().unwrap_or("").to_owned();
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if path == "/health" {
        if inner.lock().knobs.health_down {
            return reply(&mut stream, 503, &error("down", "starting"), &[]);
        }
        return reply(
            &mut stream,
            200,
            &json!({"status": "ok", "platform": "hermes-agent", "version": "0.22.2"}),
            &[],
        );
    }
    if req.header("authorization") != Some(&format!("Bearer {KEY}")) {
        return reply(
            &mut stream,
            401,
            &json!({"error": {"message": "Invalid gateway API key (API_SERVER_KEY)",
                              "type": "gateway_auth_error", "code": "gateway_auth_failed"}}),
            &[],
        );
    }
    match (req.method.as_str(), parts.as_slice()) {
        ("GET", ["v1", "capabilities"]) => {
            let missing = inner.lock().knobs.missing_feature;
            let mut features = serde_json::Map::new();
            for f in NEEDED_FEATURES {
                if Some(f) != missing {
                    features.insert(f.to_owned(), json!(true));
                }
            }
            features.insert("run_steer".to_owned(), json!(true));
            reply(
                &mut stream,
                200,
                &json!({"object": "hermes.api_server.capabilities", "platform": "hermes-agent",
                        "features": features}),
                &[],
            );
        }
        ("POST", ["v1", "runs"]) => post_run(inner, &mut stream, &req),
        ("GET", ["v1", "runs", id]) => {
            let st = inner.lock();
            match st.runs.iter().find(|r| r.id == *id) {
                Some(r) => {
                    let v = status_of(r);
                    drop(st);
                    reply(&mut stream, 200, &v, &[]);
                }
                None => {
                    drop(st);
                    reply(
                        &mut stream,
                        404,
                        &error("run_not_found", "Run not found"),
                        &[],
                    );
                }
            }
        }
        ("GET", ["v1", "runs", id, "events"]) => events(inner, stream, id, &req, generation),
        ("POST", ["v1", "runs", id, "stop"]) => {
            let mut st = inner.lock();
            let ignored = st.knobs.stop_ignored;
            let Some(r) = st.runs.iter_mut().find(|r| r.id == *id) else {
                drop(st);
                return reply(
                    &mut stream,
                    404,
                    &error("run_not_found", "Run not found"),
                    &[],
                );
            };
            if super::runs::TERMINAL.contains(&r.status.as_str()) {
                let v = status_of(r);
                drop(st);
                return reply(&mut stream, 200, &v, &[]);
            }
            if r.stream_gone {
                drop(st);
                return reply(
                    &mut stream,
                    409,
                    &error("run_not_active", "not active"),
                    &[],
                );
            }
            r.status = "stopping".to_owned();
            let id = r.id.clone();
            drop(st);
            if !ignored {
                // The cooperative stop lands a moment later.
                let fake_push = |inner: &Inner| {
                    let mut st = inner.lock();
                    if let Some(r) = st.runs.iter_mut().find(|r| r.id == id) {
                        let seq = r.events.len();
                        r.events.push(json!({"event": "run.cancelled", "run_id": id,
                            "timestamp": 1.0, "seq": seq, "completed": false,
                            "partial": false, "interrupted": true}));
                        r.status = "cancelled".to_owned();
                    }
                };
                fake_push(inner);
                inner.cv.notify_all();
            }
            reply(
                &mut stream,
                200,
                &json!({"run_id": parts[2], "status": "stopping"}),
                &[],
            );
        }
        ("POST", ["v1", "runs", id, "approval"]) => {
            reply(
                &mut stream,
                200,
                &json!({"object": "hermes.run.approval_response", "run_id": id,
                        "choice": req.body["choice"], "resolved": 1}),
                &[],
            );
        }
        ("POST", ["api", "sessions"]) => {
            let id = req.body["id"].as_str().unwrap_or("").to_owned();
            let mut st = inner.lock();
            if st.sessions.contains_key(&id) {
                drop(st);
                return reply(&mut stream, 409, &error("session_exists", "exists"), &[]);
            }
            st.sessions.insert(id.clone(), Vec::new());
            drop(st);
            reply(
                &mut stream,
                201,
                &json!({"object": "hermes.session", "session": {"id": id, "message_count": 0}}),
                &[],
            );
        }
        ("GET", ["api", "sessions", id]) => {
            let st = inner.lock();
            let found = st.sessions.get(*id).map(|m| m.len());
            drop(st);
            match found {
                Some(n) => reply(
                    &mut stream,
                    200,
                    &json!({"object": "hermes.session", "session": {"id": id, "message_count": n}}),
                    &[],
                ),
                None => reply(
                    &mut stream,
                    404,
                    &error("session_not_found", "Session not found"),
                    &[],
                ),
            }
        }
        ("DELETE", ["api", "sessions", id]) => {
            let gone = inner.lock().sessions.remove(*id).is_some();
            if gone {
                reply(
                    &mut stream,
                    200,
                    &json!({"object": "hermes.session.deleted", "id": id, "deleted": true}),
                    &[],
                );
            } else {
                reply(
                    &mut stream,
                    404,
                    &error("session_not_found", "Session not found"),
                    &[],
                );
            }
        }
        ("GET", ["api", "sessions", id, "messages"]) => {
            let st = inner.lock();
            let found = st.sessions.get(*id).cloned();
            drop(st);
            match found {
                Some(m) => reply(
                    &mut stream,
                    200,
                    &json!({"object": "list", "session_id": id, "data": m,
                            "pagination": {"returned": m.len()}}),
                    &[],
                ),
                None => reply(
                    &mut stream,
                    404,
                    &error("session_not_found", "Session not found"),
                    &[],
                ),
            }
        }
        _ => reply(&mut stream, 404, &error("not_found", "no route"), &[]),
    }
}

fn post_run(inner: &Inner, stream: &mut TcpStream, req: &Rec) {
    let (forced, delay) = {
        let st = inner.lock();
        (st.knobs.runs_status, st.knobs.runs_delay)
    };
    if let Some(code) = forced {
        return reply(stream, code, &error("forced", "forced by the test"), &[]);
    }
    let key = req.header("idempotency-key").map(str::to_owned);
    let mut st = inner.lock();
    if let Some(run) = key.as_ref().and_then(|k| st.idem.get(k)).cloned() {
        let status = st
            .runs
            .iter()
            .find(|r| r.id == run)
            .map(|r| r.status.clone())
            .unwrap_or_default();
        drop(st);
        return reply(
            stream,
            202,
            &json!({"run_id": run, "status": status, "replayed": true}),
            &[("Idempotency-Replayed", "true")],
        );
    }
    st.next_run += 1;
    let id = format!("run_{:032}", st.next_run);
    let session_id = req.body["session_id"].as_str().unwrap_or(&id).to_owned();
    st.sessions.entry(session_id.clone()).or_default();
    st.runs.push(FRun {
        id: id.clone(),
        session_id,
        status: "running".to_owned(),
        events: Vec::new(),
        extra: json!({}),
        stream_gone: false,
    });
    if let Some(k) = key {
        st.idem.insert(k, id.clone());
    }
    drop(st);
    inner.cv.notify_all();
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    reply(
        stream,
        202,
        &json!({"run_id": id, "status": "started", "replayed": false}),
        &[],
    );
}

fn chunk(stream: &mut TcpStream, data: &str) -> bool {
    let framed = format!("{:x}\r\n{data}\r\n", data.len());
    stream.write_all(framed.as_bytes()).is_ok() && stream.flush().is_ok()
}

fn events(inner: &Inner, mut stream: TcpStream, id: &str, req: &Rec, generation: u64) {
    let last: i64 = req
        .header("last-event-id")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);
    {
        let st = inner.lock();
        let known = st.runs.iter().find(|r| r.id == id).map(|r| r.stream_gone);
        if known != Some(false) {
            drop(st);
            return reply(
                &mut stream,
                404,
                &error("run_not_found", "Run not found"),
                &[],
            );
        }
    }
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nTransfer-Encoding: chunked\r\n\r\n";
    if stream.write_all(head.as_bytes()).is_err() || !chunk(&mut stream, ": open\n\n") {
        return;
    }
    let mut sent = last;
    let stream_gen = inner.lock().stream_gen;
    let mut last_keepalive = Instant::now();
    loop {
        let (out, terminal, broken) = {
            let st = inner.lock();
            if st.generation != generation || inner.stop.load(Ordering::Acquire) {
                return; // the TCP goes, no closing chunk
            }
            let broken = st.stream_gen != stream_gen;
            let Some(r) = st.runs.iter().find(|r| r.id == id) else {
                return;
            };
            let out: Vec<Value> = r
                .events
                .iter()
                .filter(|e| e["seq"].as_i64().unwrap_or(-1) > sent)
                .cloned()
                .collect();
            let terminal = super::runs::TERMINAL.contains(&r.status.as_str());
            (out, terminal, broken)
        };
        if broken {
            return;
        }
        for e in &out {
            let seq = e["seq"].as_i64().unwrap_or(0);
            if !chunk(&mut stream, &format!("id: {seq}\ndata: {e}\n\n")) {
                return;
            }
            sent = seq;
        }
        if terminal {
            let _ = chunk(&mut stream, ": stream closed\n\n");
            let _ = stream.write_all(b"0\r\n\r\n");
            return;
        }
        if last_keepalive.elapsed() >= Duration::from_millis(200) {
            last_keepalive = Instant::now();
            if !chunk(&mut stream, ": keepalive\n\n") {
                return;
            }
        }
        let st = inner.lock();
        let _ = inner.cv.wait_timeout(st, Duration::from_millis(50));
    }
}
