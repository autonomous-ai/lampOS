//! [`Session`]: the [`AgentSession`] over Hermes' own API server (the
//! pinned v0.22.2 `gateway/platforms/api_server*.py`), its Runs API, on
//! loopback with the server's Bearer key. Synchronous: plain HTTP/1.1
//! ([`http`]), one thread per run following its SSE ([`follow`]), callers
//! waiting on a `Condvar` with their own deadline. Runs move only in
//! [`runs`].
//!
//! - Ready: `GET /health` 200, `GET /v1/capabilities` (the key accepted,
//!   the Runs features there), and the kept session there
//!   (`GET /api/sessions/{id}`, else `POST /api/sessions {id}`).
//! - One kept session: its id is lamp-tasks' own (`lamp-main-<ms>`), kept
//!   in a 0600 file ([`Config::state_path`]) so it survives restarts of
//!   lamp-tasks, of the gateway, and reboots; rotated only by [`reset`].
//!   Hermes compresses the context itself and follows its own compression
//!   chain from that id.
//! - Hand-over: `POST /v1/runs {input, session_id}` with an
//!   `Idempotency-Key` (`lamp-<task>-<n>`); 202 names the run.
//! - Done: the run's terminal event, or its status after a broken stream.
//! - A drop (the gateway gone mid-run) ends the run at once with
//!   [`crate::DROPPED`]; the same turn handed over again within [`STRAYS_FOR`]
//!   gets that ended run back, so nothing is ever run twice.
//! - Approvals: answered at once by policy (`once` only for the
//!   skill-start launcher, else `deny`), never `session`/`always`.
//!
//! [`reset`]: AgentSession::reset

mod follow;
mod http;
mod runs;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use self::follow::{Ctx, Shared};
use self::http::{HttpError, Req, Resp};
use crate::{AgentSession, Engine, Error, Outcome, Progress, RunId, Turn};

/// The API server's loopback address (`API_SERVER_HOST`/`PORT` defaults).
pub const SERVER_URL: &str = "http://127.0.0.1:8642";
/// Where the product keeps the API server's key (`API_SERVER_KEY`), 0600.
pub const KEY_PATH: &str = "/var/lib/lampos-hermes-gateway/api-key";
/// Where lamp-tasks keeps the kept session's id and counts.
pub const STATE_PATH: &str = "/var/lib/lampos-tasks/hermes-direct.json";
/// The skill-start fence: the only command an approval may run.
pub const SKILL_START: &str = crate::openclaw::SKILL_START;
/// How long after a drop the same turn handed over again gets its ended
/// run back instead of a new run.
pub const STRAYS_FOR: Duration = Duration::from_secs(15 * 60);
/// The Runs features `GET /v1/capabilities` must say.
pub const NEEDED_FEATURES: [&str; 6] = [
    "run_submission",
    "run_status",
    "run_events_sse",
    "run_stop",
    "run_approval_response",
    "approval_events",
];

/// Longest reconnect backoff.
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// Ended runs whose usage was counted, remembered.
const COUNTED_MAX: usize = 256;

#[derive(Clone, Debug)]
pub struct Config {
    /// `http://host:port` of the API server.
    pub url: String,
    pub key_path: PathBuf,
    /// The kept session's file; `None` keeps it in memory only.
    pub state_path: Option<PathBuf>,
    /// The one command path the default approval policy allows.
    pub fence: PathBuf,
    /// The bound on each call that takes no deadline of its own (`send`,
    /// `abort`, `reset`, a status read, an approval answer).
    pub call_timeout: Duration,
    /// A run's stream with no byte this long is taken as broken (the
    /// server writes a keepalive every 10 s).
    pub stream_idle: Duration,
    /// Status reads left unanswered (not refused) this long end a run as
    /// dropped (V1's 30 s settle).
    pub settle: Duration,
    /// The pause between status reads while settling.
    pub poll: Duration,
    /// A ready session is looked at again (`/health`) after this long.
    pub recheck: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            url: SERVER_URL.to_owned(),
            key_path: PathBuf::from(KEY_PATH),
            state_path: Some(PathBuf::from(STATE_PATH)),
            fence: PathBuf::from(SKILL_START),
            call_timeout: Duration::from_secs(10),
            stream_idle: Duration::from_secs(30),
            settle: Duration::from_secs(30),
            poll: Duration::from_millis(250),
            recheck: Duration::from_secs(5),
        }
    }
}

/// An `approval.request` of a run, as the policy sees it (the server's
/// `_approval_request_event`: the command already redacted).
#[derive(Clone, Debug)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub command: String,
    pub description: String,
    pub pattern_keys: Vec<String>,
    /// The answers the server takes (`once`, `session`, `always`, `deny`).
    pub choices: Vec<String>,
    /// The whole event.
    pub raw: Value,
}

impl ApprovalRequest {
    pub(crate) fn from_event(evt: &Value) -> ApprovalRequest {
        let s = |k: &str| evt.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        let list = |k: &str| {
            evt.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        ApprovalRequest {
            request_id: s("request_id"),
            command: s("command"),
            description: s("description"),
            pattern_keys: list("pattern_keys"),
            choices: list("choices"),
            raw: evt.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Once,
    Deny,
}

impl Decision {
    /// The `choice` of `POST /v1/runs/{id}/approval`.
    pub fn word(self) -> &'static str {
        match self {
            Decision::Once => "once",
            Decision::Deny => "deny",
        }
    }
}

/// Answers an approval prompt at once, on the run's thread.
pub type ApprovalPolicy = Arc<dyn Fn(&ApprovalRequest) -> Decision + Send + Sync>;
/// Where log lines go (never the key).
pub type Logger = Arc<dyn Fn(&str) + Send + Sync>;

/// `once` only for exactly `fence` (by its path or its bare file name) run
/// with one or more plain-word arguments (no shell),
/// when the server offers `once`; `deny` for everything else.
pub fn fence_policy(fence: PathBuf) -> ApprovalPolicy {
    Arc::new(move |ask: &ApprovalRequest| {
        let fence = fence.to_string_lossy();
        let plain = |w: &str| {
            !w.is_empty()
                && w.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        };
        let shell = ask.command.chars().any(|c| {
            matches!(
                c,
                ';' | '|' | '&' | '$' | '`' | '>' | '<' | '\n' | '(' | ')' | '\'' | '"' | '\\'
            )
        });
        // The launcher by its full path, or by its bare name (it is on the
        // gateway's PATH).
        let bare = fence.rsplit('/').next().unwrap_or(&fence).to_owned();
        let mut words = ask.command.split_whitespace();
        let first = words.next();
        let launcher =
            first == Some(fence.as_ref()) || (!bare.is_empty() && first == Some(bare.as_str()));
        let args: Vec<&str> = words.collect();
        let allowed = !shell && launcher && !args.is_empty() && args.iter().all(|w| plain(w));
        let offered = ask.choices.is_empty() || ask.choices.iter().any(|c| c == "once");
        if allowed && offered {
            Decision::Once
        } else {
            Decision::Deny
        }
    })
}

fn house_log() -> Logger {
    Arc::new(|line: &str| eprintln!("lamp-tasks: hermes gateway: {line}"))
}

/// The kept session as the state file holds it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Kept {
    pub session_id: String,
    /// When it was made (Unix seconds).
    pub created: u64,
    /// Runs handed over into it.
    pub turns: u64,
    /// The last ended run's `usage.input_tokens` (the prompts of that one
    /// turn's model calls), as V1 compared one turn's usage with its 250k
    /// backstop. Never summed across turns: a sum passes 250k after a few
    /// web searches and would rotate the kept session at nearly every turn
    /// (ee17, 24 Nov 02:55 and 03:00).
    pub tokens: u64,
}

impl Kept {
    fn fresh() -> Kept {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Kept {
            session_id: format!("lamp-main-{}", now.as_millis()),
            created: now.as_secs(),
            turns: 0,
            tokens: 0,
        }
    }
}

/// Reconnect pacing: 1, 2, 4 ... 30 s after each failure, none after a
/// success.
#[derive(Debug, Default)]
struct Backoff {
    fails: u32,
    next_at: Option<Instant>,
}

impl Backoff {
    fn fail(&mut self) {
        let delay = Duration::from_secs(1u64 << self.fails.min(5)).min(BACKOFF_MAX);
        self.fails = self.fails.saturating_add(1);
        self.next_at = Some(Instant::now() + delay);
    }

    fn reset(&mut self) {
        *self = Backoff::default();
    }
}

struct Follower {
    run: RunId,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Follower {
    fn halt(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The one kept session on Hermes' API server.
pub struct Session {
    cfg: Config,
    shared: Arc<Shared>,
    policy: ApprovalPolicy,
    log: Logger,
    backoff: Backoff,
    last_why: String,
    /// The API key, read at each full check.
    key: String,
    /// Checked (health, capabilities, the kept session) and not seen down
    /// since.
    up: bool,
    /// The last time the server was seen serving.
    checked_at: Option<Instant>,
    /// The kept session is there on the server.
    session_ok: bool,
    kept: Kept,
    server_version: Option<String>,
    /// The `<n>` of the next `lamp-<task_id>-<n>`: seeded from the clock so
    /// a restarted lamp-tasks never repeats an idempotency key.
    next_n: u64,
    /// Hand-overs whose reply never came, by task and words: sent again
    /// with the same key, so the server replays instead of running twice.
    unconfirmed: HashMap<(String, String), String>,
    followers: Vec<Follower>,
    counted: HashSet<RunId>,
    /// `(tokens, messages)` of the kept session's live context.
    size: Option<(u64, u64)>,
    size_fresh: bool,
    /// The compression chain's live tip, as the server named it last.
    tip: Option<String>,
}

pub type HermesSession = Session;

impl Session {
    pub fn new(cfg: Config) -> Session {
        let kept = cfg
            .state_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Kept>(&b).ok())
            .filter(|k| !k.session_id.is_empty());
        let adopted = kept.is_some();
        let next_n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1);
        let s = Session {
            policy: fence_policy(cfg.fence.clone()),
            cfg,
            shared: Arc::new(Shared::default()),
            log: house_log(),
            backoff: Backoff::default(),
            last_why: "not checked yet".to_owned(),
            key: String::new(),
            up: false,
            checked_at: None,
            session_ok: false,
            kept: kept.unwrap_or_else(Kept::fresh),
            server_version: None,
            next_n,
            unconfirmed: HashMap::new(),
            followers: Vec::new(),
            counted: HashSet::new(),
            size: None,
            size_fresh: false,
            tip: None,
        };
        if !adopted {
            s.save();
        }
        s
    }

    /// Answers approval prompts with `policy` instead of the fence policy.
    pub fn with_policy(mut self, policy: ApprovalPolicy) -> Session {
        self.policy = policy;
        self
    }

    pub fn with_logger(mut self, log: Logger) -> Session {
        self.log = log;
        self
    }

    /// The kept session's id now.
    pub fn session_id(&self) -> &str {
        &self.kept.session_id
    }

    /// The kept session as the state file holds it.
    pub fn kept(&self) -> &Kept {
        &self.kept
    }

    /// Runs handed over into the kept session (V1's 40-turn backstop).
    pub fn turns(&self) -> u64 {
        self.kept.turns
    }

    /// The last ended run's `usage.input_tokens` ([`Kept::tokens`], V1's
    /// 250k backstop, per turn).
    pub fn run_tokens(&self) -> u64 {
        self.kept.tokens
    }

    /// Hermes' own version, from its last `/health`.
    pub fn server_version(&self) -> Option<&str> {
        self.server_version.as_deref()
    }

    fn log(&self, line: &str) {
        (self.log)(line);
    }

    fn save(&self) {
        let Some(path) = &self.cfg.state_path else {
            return;
        };
        if let Err(e) = write_private(
            path,
            &serde_json::to_vec_pretty(&self.kept).unwrap_or_default(),
        ) {
            self.log(&format!(
                "keeping the session id in {}: {e}",
                path.display()
            ));
        }
    }

    fn call_deadline(&self) -> Instant {
        Instant::now() + self.cfg.call_timeout
    }

    /// One request with the key, its whole reply before `deadline` (at most
    /// the call timeout).
    fn http(&self, req: Req<'_>, deadline: Instant) -> Result<Resp, HttpError> {
        let deadline = deadline.min(self.call_deadline());
        let req = req.header("Authorization", format!("Bearer {}", self.key));
        http::call(&self.cfg.url, &req, deadline)
    }

    fn lost(e: HttpError) -> Error {
        match e {
            HttpError::Connect(why) => Error::NotReady(why),
            HttpError::Io(why) => Error::Lost(why),
        }
    }

    /// Health, capabilities with the key, and the kept session there.
    fn check(&mut self, deadline: Instant) -> Result<(), Error> {
        let key = std::fs::read_to_string(&self.cfg.key_path)
            .map(|t| t.trim().to_owned())
            .unwrap_or_default();
        if key.is_empty() {
            return Err(Error::NotReady(format!(
                "no API key yet in {}",
                self.cfg.key_path.display()
            )));
        }
        self.key = key;
        self.health(deadline)?;
        let caps = self
            .http(Req::new("GET", "/v1/capabilities"), deadline)
            .map_err(Self::lost)?;
        match caps.status {
            200 => {}
            401 | 403 => return Err(Error::Refused(caps.error_words())),
            _ => return Err(Error::NotReady(caps.error_words())),
        }
        let features = caps.json().get("features").cloned().unwrap_or(Value::Null);
        let missing: Vec<&str> = NEEDED_FEATURES
            .iter()
            .copied()
            .filter(|f| features.get(*f).and_then(Value::as_bool) != Some(true))
            .collect();
        if !missing.is_empty() {
            return Err(Error::Refused(format!(
                "the API server lacks {}",
                missing.join(",")
            )));
        }
        if !self.session_ok {
            self.open_session(deadline)?;
        }
        Ok(())
    }

    fn health(&mut self, deadline: Instant) -> Result<(), Error> {
        let health = http::call(
            &self.cfg.url,
            &Req::new("GET", "/health"),
            deadline.min(self.call_deadline()),
        )
        .map_err(|e| Error::NotReady(e.to_string()))?;
        let body = health.json();
        if health.status != 200 || body.get("status").and_then(Value::as_str) != Some("ok") {
            return Err(Error::NotReady(format!(
                "/health: {}",
                health.error_words()
            )));
        }
        self.server_version = body
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(())
    }

    /// The kept session adopted (`GET`) or made (`POST /api/sessions`). One
    /// that had turns and is gone is not made again under its id: a fresh
    /// id is.
    fn open_session(&mut self, deadline: Instant) -> Result<(), Error> {
        let path = format!("/api/sessions/{}", self.kept.session_id);
        let got = self
            .http(Req::new("GET", &path), deadline)
            .map_err(Self::lost)?;
        match got.status {
            200 => {
                self.session_ok = true;
                return Ok(());
            }
            404 => {}
            401 | 403 => return Err(Error::Refused(got.error_words())),
            _ => return Err(Error::NotReady(got.error_words())),
        }
        if self.kept.turns > 0 {
            let old = self.kept.session_id.clone();
            self.kept = Kept::fresh();
            self.save();
            self.log(&format!(
                "kept session {old} is gone; a fresh one: {}",
                self.kept.session_id
            ));
        }
        let made = self
            .http(
                Req::new("POST", "/api/sessions").json(&json!({"id": self.kept.session_id})),
                deadline,
            )
            .map_err(Self::lost)?;
        match made.status {
            201 | 409 => {
                self.session_ok = true;
                self.log(&format!("kept session {}", self.kept.session_id));
                Ok(())
            }
            401 | 403 => Err(Error::Refused(made.error_words())),
            _ => Err(Error::NotReady(made.error_words())),
        }
    }

    fn mark_down(&mut self, why: &str) {
        self.up = false;
        self.checked_at = None;
        self.last_why = why.to_owned();
    }

    /// Serving before `deadline`. With `wait_backoff`, sleeps out a backoff
    /// that ends before the deadline; else answers at once.
    fn ensure(&mut self, deadline: Instant, wait_backoff: bool) -> Result<(), Error> {
        loop {
            let down = self.shared.lock().down.take();
            // A run thread saw the server gone: checked in full now (no
            // backoff yet: it may be back already).
            if let Some(why) = down
                && self.up
            {
                self.mark_down(&why);
            }
            if self.up && self.session_ok {
                if self
                    .checked_at
                    .is_some_and(|at| at.elapsed() < self.cfg.recheck)
                {
                    return Ok(());
                }
                match self.health(deadline) {
                    Ok(()) => {
                        self.checked_at = Some(Instant::now());
                        return Ok(());
                    }
                    Err(e) => {
                        self.mark_down(&e.to_string());
                        self.backoff.fail();
                    }
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::NotReady(self.last_why.clone()));
            }
            if let Some(at) = self.backoff.next_at
                && at > now
            {
                if !wait_backoff {
                    return Err(Error::NotReady(format!(
                        "{} (next try in {} ms)",
                        self.last_why,
                        (at - now).as_millis()
                    )));
                }
                std::thread::sleep(at.min(deadline) - now);
                continue;
            }
            match self.check(deadline) {
                Ok(()) => {
                    if !self.up {
                        self.log(&format!(
                            "serving: Hermes {} (session {})",
                            self.server_version.as_deref().unwrap_or("?"),
                            self.kept.session_id
                        ));
                    }
                    self.up = true;
                    self.checked_at = Some(Instant::now());
                    self.backoff.reset();
                    self.shared.lock().down = None;
                    return Ok(());
                }
                Err(e) => {
                    self.mark_down(&e.to_string());
                    self.backoff.fail();
                    match e {
                        Error::Refused(_) => return Err(e),
                        _ if !wait_backoff => return Err(e),
                        _ => {}
                    }
                }
            }
        }
    }

    fn follow(&mut self, run: RunId) {
        self.followers
            .retain(|f| f.thread.as_ref().is_some_and(|t| !t.is_finished()));
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = Ctx {
            base: self.cfg.url.clone(),
            key: self.key.clone(),
            shared: self.shared.clone(),
            policy: self.policy.clone(),
            log: self.log.clone(),
            stop: stop.clone(),
            call_timeout: self.cfg.call_timeout,
            stream_idle: self.cfg.stream_idle,
            settle: self.cfg.settle,
            poll: self.cfg.poll,
        };
        let id = run.clone();
        let thread = std::thread::Builder::new()
            .name("hermes-run".to_owned())
            .spawn(move || follow::follow(ctx, id))
            .expect("spawn a run thread");
        self.followers.push(Follower {
            run,
            stop,
            thread: Some(thread),
        });
    }

    /// Keeps an ended run's usage once as the kept session's last turn's
    /// ([`Kept::tokens`]).
    fn count(&mut self, run: &RunId) {
        if !self.counted.insert(run.clone()) {
            return;
        }
        if self.counted.len() > COUNTED_MAX {
            self.counted.clear();
            self.counted.insert(run.clone());
        }
        let usage = self.shared.lock().runs.get(run).and_then(|r| r.usage);
        if let Some(u) = usage {
            self.kept.tokens = u.input_tokens;
            self.save();
        }
        self.size_fresh = false;
    }

    /// The kept session's live context as the server holds it, `(tokens,
    /// messages)`: the compression tip's messages (`GET
    /// /api/sessions/{id}/messages`, compacted ones left out), tokens
    /// estimated at 4 bytes each of their text, tool calls and reasoning.
    /// Read now when not read since the last run ended; `None` when it
    /// can't be read.
    pub fn size_now(&mut self) -> Option<(u64, u64)> {
        if !self.size_fresh {
            let deadline = self.call_deadline();
            if self.ensure(deadline, false).is_ok() {
                self.read_size(deadline);
            }
        }
        if self.size_fresh { self.size } else { None }
    }

    fn read_size(&mut self, deadline: Instant) {
        const PAGE: usize = 500;
        let path = format!(
            "/api/sessions/{}/messages?limit={PAGE}&order=latest",
            self.kept.session_id
        );
        let Ok(r) = self.http(Req::new("GET", &path), deadline) else {
            return;
        };
        if r.status == 404 {
            self.session_ok = false;
            return;
        }
        if r.status != 200 {
            return;
        }
        let v = r.json();
        let tip = v
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let data = v
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let bytes: u64 = data
            .iter()
            .map(|m| {
                ["content", "tool_calls", "reasoning", "reasoning_content"]
                    .iter()
                    .map(|k| match m.get(*k) {
                        None | Some(Value::Null) => 0,
                        Some(Value::String(s)) => s.len(),
                        Some(other) => other.to_string().len(),
                    })
                    .sum::<usize>() as u64
            })
            .sum();
        let mut messages = data.len() as u64;
        if data.len() >= PAGE
            && let Some(tip) = &tip
            && let Ok(s) = self.http(Req::new("GET", &format!("/api/sessions/{tip}")), deadline)
            && let Some(n) = s.json()["session"]["message_count"].as_u64()
        {
            messages = messages.max(n);
        }
        self.tip = tip;
        self.size = Some((bytes.div_ceil(4), messages));
        self.size_fresh = true;
    }

    /// `POST /v1/runs/{id}/stop`: the server's reply status, or why not.
    fn stop_run(&self, run: &RunId, deadline: Instant) -> Result<Resp, Error> {
        let path = format!("/v1/runs/{}/stop", run.0);
        self.http(Req::new("POST", &path), deadline)
            .map_err(Self::lost)
    }
}

/// Writes `bytes` to `path` as a 0600 file, whole or not at all.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let tmp = path.with_extension("json.tmp");
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        open.mode(0o600);
    }
    let mut f = open.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// `task_id` as a plain word for the idempotency key.
fn plain(task_id: &str) -> String {
    task_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl AgentSession for Session {
    fn engine(&self) -> Engine {
        Engine::Hermes
    }

    fn ready(&mut self, within: Duration) -> Result<(), Error> {
        self.ensure(Instant::now() + within, true)
    }

    fn send(&mut self, turn: &Turn) -> Result<RunId, Error> {
        let pending = (turn.task_id.clone(), turn.message.clone());
        // Ended by a drop a moment ago: that run, never a second one.
        if let Some(run) = self.shared.lock().runs.dropped_turn(&pending, STRAYS_FOR) {
            self.log(&format!(
                "task {} was ended by a drop as {}; not handed over again",
                turn.task_id, run.0
            ));
            return Ok(run);
        }
        let deadline = self.call_deadline();
        self.ensure(deadline, false)?;
        let key = match self.unconfirmed.get(&pending) {
            Some(key) => key.clone(),
            None => {
                let n = self.next_n;
                self.next_n += 1;
                format!("lamp-{}-{n}", plain(&turn.task_id))
            }
        };
        let body = json!({"input": turn.message, "session_id": self.kept.session_id});
        let reply = self.http(
            Req::new("POST", "/v1/runs")
                .header("Idempotency-Key", key.clone())
                .json(&body),
            deadline,
        );
        let r = match reply {
            Ok(r) => r,
            Err(HttpError::Connect(why)) => {
                // Never sent: nothing to replay.
                self.mark_down(&why);
                self.backoff.fail();
                self.log(&format!("task {} not handed over: {why}", turn.task_id));
                return Err(Error::NotReady(why));
            }
            Err(HttpError::Io(why)) => {
                // The server may have it: the next send replays its key.
                self.unconfirmed.insert(pending, key.clone());
                self.checked_at = None;
                self.log(&format!(
                    "task {} not confirmed ({why}); the next send reuses {key}",
                    turn.task_id
                ));
                return Err(Error::Lost(why));
            }
        };
        match r.status {
            202 => {}
            401 | 403 => {
                self.mark_down(&r.error_words());
                return Err(Error::Refused(r.error_words()));
            }
            // At the concurrent-run cap, or draining: try again.
            429 | 503 => return Err(Error::NotReady(r.error_words())),
            _ => {
                self.unconfirmed.remove(&pending);
                self.log(&format!(
                    "task {} not handed over: {}",
                    turn.task_id,
                    r.error_words()
                ));
                return Err(Error::Refused(r.error_words()));
            }
        }
        self.unconfirmed.remove(&pending);
        let Some(run) = r
            .json()
            .get("run_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            return Err(Error::Refused(
                "the run was taken with no run_id".to_owned(),
            ));
        };
        let run = RunId(run);
        let replayed = r.header("Idempotency-Replayed") == Some("true");
        if self.shared.lock().runs.contains(&run) {
            return Ok(run);
        }
        self.shared
            .lock()
            .runs
            .register(run.clone(), &turn.task_id, &turn.message, turn.kind);
        self.follow(run.clone());
        if !replayed {
            self.kept.turns += 1;
            self.save();
        }
        self.size_fresh = false;
        self.log(&format!(
            "task {} handed over as {} (key {key}{})",
            turn.task_id,
            run.0,
            if replayed { ", replayed" } else { "" }
        ));
        Ok(run)
    }

    fn wait(&mut self, run: &RunId, within: Duration) -> Result<Outcome, Error> {
        let deadline = Instant::now() + within;
        let mut st = self.shared.lock();
        loop {
            if !st.runs.contains(run) {
                return Err(Error::UnknownRun(run.clone()));
            }
            if let Some(outcome) = st.runs.outcome(run) {
                drop(st);
                self.count(run);
                return Ok(outcome);
            }
            let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
                return Err(Error::TimedOut);
            };
            st = self
                .shared
                .cv
                .wait_timeout(st, wait)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn progress(&self, run: &RunId) -> Option<Progress> {
        self.shared.lock().runs.progress(run)
    }

    fn running(&self) -> Vec<Progress> {
        self.shared.lock().runs.running()
    }

    /// `POST /v1/runs/{id}/stop`, then a settle: the run's own end
    /// (`run.cancelled`) or its status read every [`Config::poll`], within
    /// [`Config::settle`]; ended here as aborted when the server has no
    /// such run active, or no end came.
    fn abort(&mut self, run: &RunId) -> Result<(), Error> {
        {
            let st = self.shared.lock();
            if !st.runs.contains(run) {
                return Err(Error::UnknownRun(run.clone()));
            }
            if !st.runs.is_open(run) {
                return Ok(());
            }
        }
        let deadline = self.call_deadline();
        self.ensure(deadline, false)?;
        let r = self.stop_run(run, deadline)?;
        match r.status {
            200 => {
                let body = r.json();
                let status = body.get("status").and_then(Value::as_str).unwrap_or("");
                if runs::TERMINAL.contains(&status) {
                    self.shared.lock().runs.on_status(run, &body);
                }
            }
            404 | 409 => {}
            401 | 403 => return Err(Error::Refused(r.error_words())),
            _ => return Err(Error::Lost(r.error_words())),
        }
        // The settle: the run's own end (its stream), or its status read
        // every `poll`, within `settle` (a stuck run's stream may be hung).
        let settle_by = Instant::now() + self.cfg.settle;
        let mut settled = false;
        loop {
            {
                let st = self.shared.lock();
                if !st.runs.is_open(run) {
                    settled = true;
                    break;
                }
                let Some(wait) = settle_by.checked_duration_since(Instant::now()) else {
                    break;
                };
                let _ = self
                    .shared
                    .cv
                    .wait_timeout(st, wait.min(self.cfg.poll))
                    .unwrap_or_else(|e| e.into_inner());
            }
            let path = format!("/v1/runs/{}", run.0);
            match self.http(Req::new("GET", &path), self.call_deadline()) {
                Ok(r) if r.status == 200 => {
                    self.shared.lock().runs.on_status(run, &r.json());
                }
                // Gone from the server: nothing runs it any more.
                Ok(r) if r.status == 404 => break,
                _ => {}
            }
        }
        self.shared.lock().runs.abort(run);
        self.shared.cv.notify_all();
        self.log(&format!(
            "abort {}: {}",
            run.0,
            if settled {
                "stopped"
            } else {
                "no end from the server; ended here"
            }
        ));
        Ok(())
    }

    /// A fresh session id in place of the kept one: open runs stopped, the
    /// new id kept and made, the old session (and its compression tip)
    /// deleted, best effort.
    fn reset(&mut self) -> Result<(), Error> {
        let deadline = self.call_deadline();
        self.ensure(deadline, false)?;
        let open = self.shared.lock().runs.open();
        for run in &open {
            let _ = self.stop_run(run, deadline);
            self.shared.lock().runs.abort(run);
        }
        for f in &mut self.followers {
            f.halt();
        }
        self.followers.clear();
        self.shared.cv.notify_all();
        let old = self.kept.session_id.clone();
        let tip = self.tip.take();
        self.kept = Kept::fresh();
        self.save();
        self.session_ok = false;
        self.size = None;
        self.size_fresh = false;
        let made = self.open_session(deadline);
        for id in std::iter::once(old.clone()).chain(tip.filter(|t| *t != old)) {
            let path = format!("/api/sessions/{id}");
            match self.http(Req::new("DELETE", &path), deadline) {
                Ok(r) if r.status == 200 || r.status == 404 => {}
                Ok(r) => self.log(&format!("deleting session {id}: {}", r.error_words())),
                Err(e) => self.log(&format!("deleting session {id}: {e}")),
            }
        }
        self.log(&format!(
            "session {old} replaced by {}",
            self.kept.session_id
        ));
        made
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        for f in &mut self.followers {
            f.halt();
        }
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("url", &self.cfg.url)
            .field("session_id", &self.kept.session_id)
            .field("up", &self.up)
            .field(
                "following",
                &self.followers.iter().map(|f| &f.run.0).collect::<Vec<_>>(),
            )
            .finish()
    }
}
