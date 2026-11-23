//! [`Session`]: the [`AgentSession`] over OpenClaw's gateway WebSocket
//! (protocol 4, the pinned `openclaw@2026.11.2`), synchronous: one thread
//! owns the socket ([`io`]), callers wait on a `Condvar` with their own
//! deadline. Frames are built and read only in [`frames`]; runs move only
//! in [`runs`].
//!
//! Connects with loopback token auth and no device (`gateway-client`,
//! mode `backend`), as the gateway's own CLI does. Reconnects after a drop
//! on the next call (backoff 1, 2, 4 ... 30 s) and resyncs each open run
//! from `chat.history` (`inFlightRun`, then the transcript).

mod frames;
mod io;
mod runs;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod replay;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use self::frames::method;
pub use self::io::DROPPED;
// Shared with the Hermes session: one tool summary and text tail rule.
use self::io::Shared;
pub(crate) use self::runs::{TAIL_MAX, brief, tail};
use crate::{AgentSession, Engine, Error, Outcome, Progress, RunId, Turn};

/// Where the product keeps the gateway's token.
pub const TOKEN_PATH: &str = "/var/lib/lampos-openclaw/gateway-token";
/// The gateway's loopback address.
pub const GATEWAY_URL: &str = "ws://127.0.0.1:18789";
/// The one kept session.
pub const SESSION_KEY: &str = "agent:main:lamp";
/// The skill-start fence: the only command an approval prompt may run.
pub const SKILL_START: &str = "/usr/local/lib/lampos/skill-start";

/// Longest reconnect backoff.
const BACKOFF_MAX: Duration = Duration::from_secs(30);
/// The next dial after a slow handshake (a busy gateway, not a failure).
const SLOW_RETRY: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub struct Config {
    pub url: String,
    pub token_path: PathBuf,
    /// The kept session's key.
    pub session_key: String,
    /// `gateway-client` for token auth with no device; another id without a
    /// device is granted no scopes.
    pub client_id: String,
    pub client_version: String,
    /// The one command path the default approval policy allows.
    pub fence: PathBuf,
    /// The agent whose exec allow-list [`Session::allowlist_add`] edits.
    pub agent_id: String,
    /// The bound on each call that takes no deadline of its own (`send`,
    /// `abort`, `reset`).
    pub call_timeout: Duration,
    /// Messages read back from `chat.history` on a resync.
    pub history_limit: u32,
    /// `queueMode` for a task's hand-over: `followup` makes each task its
    /// own run even while another goes. `None` keeps the session's own.
    pub send_queue_mode: Option<String>,
    /// Ask for approval prompts (`exec-approvals`, `approvals` caps), so
    /// none is left waiting.
    pub approval_events: bool,
    /// The bound on one connect (dial, challenge, hello, first reply),
    /// whatever the caller's own: a gateway starting up or busy with a
    /// recovered run answers slowly (ee17 2026-11-23: event loop p99 1.7 s,
    /// utilization 1) and a short bound never connects.
    pub connect_timeout: Duration,
    /// [`Self::connect_timeout`] while the last close was a restart (1012)
    /// or the gateway said it is starting.
    pub restart_connect_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            url: GATEWAY_URL.to_owned(),
            token_path: PathBuf::from(TOKEN_PATH),
            session_key: SESSION_KEY.to_owned(),
            client_id: frames::CLIENT_ID.to_owned(),
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
            fence: PathBuf::from(SKILL_START),
            agent_id: "main".to_owned(),
            call_timeout: Duration::from_secs(10),
            history_limit: 50,
            send_queue_mode: Some("followup".to_owned()),
            approval_events: true,
            connect_timeout: Duration::from_secs(30),
            restart_connect_timeout: Duration::from_secs(60),
        }
    }
}

/// An exec approval prompt, as the policy sees it.
#[derive(Clone, Debug)]
pub struct ApprovalRequest {
    pub id: String,
    /// The command it asks to run.
    pub command: String,
    /// The exact argv, when the gateway says it.
    pub command_argv: Option<Vec<String>>,
    /// Environment keys the command would be run with.
    pub env_keys: Vec<String>,
    /// Where it runs (`gateway`, `node`, `worker`).
    pub host: Option<String>,
    pub agent_id: Option<String>,
    pub session_key: Option<String>,
    /// The answers the gateway takes for it (`allow-once`, `deny`, ...).
    pub allowed_decisions: Vec<String>,
    /// The whole event payload.
    pub raw: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    AllowOnce,
    Deny,
}

impl Decision {
    pub fn word(self) -> &'static str {
        match self {
            Decision::AllowOnce => "allow-once",
            Decision::Deny => "deny",
        }
    }
}

/// Answers an approval prompt at once, on the socket thread.
pub type ApprovalPolicy = Arc<dyn Fn(&ApprovalRequest) -> Decision + Send + Sync>;
/// Where log lines go (never the token).
pub type Logger = Arc<dyn Fn(&str) + Send + Sync>;

/// Allows only the exact `fence` path run with plain-word arguments (no
/// shell; the argv itself when the gateway gives it), denies everything
/// else.
pub fn fence_policy(fence: PathBuf) -> ApprovalPolicy {
    Arc::new(move |ask: &ApprovalRequest| {
        let fence = fence.to_string_lossy();
        let plain = |w: &str| {
            !w.is_empty()
                && w.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        };
        let allowed = match &ask.command_argv {
            Some(argv) => {
                argv.first().map(String::as_str) == Some(fence.as_ref())
                    && argv[1..].iter().all(|w| plain(w))
            }
            None => {
                let shell = ask.command.chars().any(|c| {
                    matches!(
                        c,
                        ';' | '|' | '&' | '$' | '`' | '>' | '<' | '\n' | '(' | ')'
                    )
                });
                let mut words = ask.command.split_whitespace();
                !shell && words.next() == Some(fence.as_ref()) && words.all(plain)
            }
        };
        if allowed && ask.env_keys.is_empty() {
            Decision::AllowOnce
        } else {
            Decision::Deny
        }
    })
}

fn house_log() -> Logger {
    Arc::new(|line: &str| eprintln!("lamp-tasks: openclaw gateway: {line}"))
}

/// Reconnect pacing: 1, 2, 4 ... 30 s after each failure, none after a
/// success.
#[derive(Debug, Default)]
struct Backoff {
    fails: u32,
    next_at: Option<Instant>,
}

impl Backoff {
    fn fail(&mut self, most: Duration) {
        let delay = Duration::from_secs(1u64 << self.fails.min(5)).min(most);
        self.fails = self.fails.saturating_add(1);
        self.next_at = Some(Instant::now() + delay);
    }

    /// A gateway still starting said when to come back: follow it, and
    /// keep the exponential steps for real failures.
    fn retry_after(&mut self, after: Duration) {
        self.next_at = Some(Instant::now() + after.clamp(Duration::from_millis(100), BACKOFF_MAX));
    }

    fn reset(&mut self) {
        *self = Backoff::default();
    }
}

/// Whether a (re)connect reads `chat.history` to bring open runs up to
/// date. Off for now: runs are tracked by their events only, and a run
/// whose events were lost on a drop is caught by lamp-tasks' stuck rule.
const RESYNC_ON_CONNECT: bool = false;

/// The one kept session on OpenClaw's gateway.
pub struct Session {
    cfg: Config,
    shared: Arc<Shared>,
    link: Option<io::Link>,
    policy: ApprovalPolicy,
    log: Logger,
    backoff: Backoff,
    last_why: String,
    /// The `<n>` of the next `lamp-<task_id>-<n>`: seeded from the clock so
    /// a restarted lamp-tasks never repeats an idempotency key.
    next_n: u64,
    req_n: u64,
    resync_due: bool,
    /// When a failed resync is tried again.
    resync_at: Option<Instant>,
    resync_fails: u32,
    /// Hand-overs whose reply never came (timed out, connection lost), by
    /// task and words: handed over again with the same key, so the gateway
    /// dedupes and the task never runs twice.
    unconfirmed: HashMap<(String, String), RunId>,
    /// `sessionInfo.totalTokens` from the last history read.
    history_tokens: Option<u64>,
    /// `totalMessages` from the last history read.
    history_messages: Option<u64>,
    /// Whether the size was read since the last connect, reset or send
    /// ([`Session::size_now`]).
    size_fresh: bool,
    /// The run the token count was last read after.
    tokens_read_for: Option<RunId>,
    server_version: Option<String>,
    /// The gateway said it is starting (1013, `UNAVAILABLE`) since the last
    /// connect.
    starting: bool,
}

impl Session {
    pub fn new(cfg: Config) -> Session {
        let shared = Arc::new(Shared::default());
        shared.lock().key = cfg.session_key.clone();
        let next_n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(1);
        Session {
            policy: fence_policy(cfg.fence.clone()),
            cfg,
            shared,
            link: None,
            log: house_log(),
            backoff: Backoff::default(),
            last_why: "not connected yet".to_owned(),
            next_n,
            req_n: 0,
            resync_due: false,
            resync_at: None,
            resync_fails: 0,
            unconfirmed: HashMap::new(),
            history_tokens: None,
            history_messages: None,
            size_fresh: false,
            tokens_read_for: None,
            server_version: None,
            starting: false,
        }
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

    /// The kept session's key now.
    pub fn session_key(&self) -> String {
        self.shared.lock().key.clone()
    }

    /// The gateway's own version, from its last hello.
    pub fn server_version(&self) -> Option<&str> {
        self.server_version.as_deref()
    }

    /// Whether the socket is up now (no I/O).
    pub fn connected(&self) -> bool {
        let Some(link) = &self.link else {
            return false;
        };
        let st = self.shared.lock();
        st.up && st.generation == link.generation
    }

    fn log(&self, line: &str) {
        (self.log)(line);
    }

    fn connect_once(&mut self, deadline: Instant) -> Result<frames::Hello, io::DialError> {
        let (ws, hello) = io::dial(&self.cfg, deadline)?;
        let generation = {
            let mut st = self.shared.lock();
            st.generation += 1;
            st.up = true;
            st.generation
        };
        self.link = Some(io::spawn(
            ws,
            &hello,
            self.shared.clone(),
            generation,
            self.policy.clone(),
            self.log.clone(),
        ));
        // Serving, not just listening: one cheap read, within the
        // connect's own bound (not the call timeout): a busy gateway is slow.
        match self.rpc_until(method::SESSIONS_LIST, frames::sessions_list(), deadline) {
            Ok(_) => Ok(hello),
            Err(Error::TimedOut) => Err(io::DialError {
                error: Error::NotReady("no sessions.list reply in time".to_owned()),
                retry_after: None,
                slow: true,
            }),
            Err(error) => Err(error.into()),
        }
    }

    /// One connect's bound: longer after a restart (1012) or while the
    /// gateway says it is starting.
    fn connect_within(&self) -> Duration {
        if self.starting || self.shared.lock().last_close == Some(1012) {
            self.cfg.restart_connect_timeout
        } else {
            self.cfg.connect_timeout
        }
    }

    fn try_connect(&mut self, deadline: Instant) -> Result<(), Error> {
        self.link = None;
        // The caller's bound may be a short slice (a 250 ms wait, a 2 s
        // ready look): a connect gets its own, or a slow gateway never
        // connects (ee17 2026-11-23 15:59-16:07).
        let deadline = deadline.max(Instant::now() + self.connect_within());
        let result = self.connect_once(deadline);
        match result {
            Ok(hello) => {
                self.log(&format!(
                    "connected to OpenClaw {} (session {})",
                    hello.server_version,
                    self.session_key()
                ));
                self.server_version = Some(hello.server_version);
                self.starting = false;
                self.backoff.reset();
                // After a drop that failed our runs, the transcript is read
                // once for a stray already going ([`Self::resync`]).
                let strays = {
                    let mut st = self.shared.lock();
                    st.last_close = None;
                    st.strays_until.is_some_and(|until| Instant::now() < until)
                };
                self.resync_due = RESYNC_ON_CONNECT || strays;
                self.resync_at = None;
                self.resync_fails = 0;
                self.resync_if_due(deadline);
                // The session's size at once: right after a (re)connect no
                // run has ended to read it after.
                self.size_fresh = false;
                self.read_size(deadline);
                Ok(())
            }
            Err(io::DialError {
                error,
                retry_after,
                slow,
            }) => {
                self.link = None;
                self.shared.lock().up = false;
                match retry_after {
                    Some(after) => {
                        self.starting = true;
                        self.backoff.retry_after(after);
                    }
                    // A slow handshake is a busy gateway: dialled again
                    // soon, the backoff not grown.
                    None if slow => self.backoff.retry_after(SLOW_RETRY),
                    None => {
                        self.backoff.fail(BACKOFF_MAX);
                    }
                }
                self.last_why = error.to_string();
                Err(error)
            }
        }
    }

    /// Connected before `deadline`. With `wait_backoff`, sleeps out a
    /// backoff that ends before the deadline; else answers at once.
    fn ensure(&mut self, deadline: Instant, wait_backoff: bool) -> Result<(), Error> {
        loop {
            if self.connected() {
                self.resync_if_due(deadline);
                return Ok(());
            }
            if self.link.is_some() {
                let why = self.shared.lock().why_down.clone();
                self.link = None;
                self.last_why = why;
                self.backoff.fail(BACKOFF_MAX);
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
                // Blocks: a backoff past the deadline is slept up to the
                // deadline, so a caller looping on `ready` never spins.
                std::thread::sleep(at.min(deadline) - now);
                continue;
            }
            match self.try_connect(deadline) {
                Ok(()) => return Ok(()),
                Err(e @ Error::Refused(_)) => return Err(e),
                Err(e) if !wait_backoff => return Err(e),
                Err(_) => {}
            }
        }
    }

    /// One request, its reply awaited until `deadline` (and at most the
    /// call timeout).
    fn rpc(&mut self, method: &str, params: Value, deadline: Instant) -> Result<Value, Error> {
        let deadline = deadline.min(Instant::now() + self.cfg.call_timeout);
        self.rpc_until(method, params, deadline)
    }

    /// [`Self::rpc`] awaited until `deadline` itself (a connect's bound).
    fn rpc_until(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let Some(link) = &self.link else {
            return Err(Error::Lost("not connected".to_owned()));
        };
        self.req_n += 1;
        let id = format!("lamp-{}", self.req_n);
        let generation = link.generation;
        {
            let mut st = self.shared.lock();
            if !st.up || st.generation != generation {
                return Err(Error::Lost(st.why_down.clone()));
            }
            st.rpcs.insert(id.clone(), None);
        }
        if link.tx.send(frames::req(&id, method, params)).is_err() {
            self.shared.lock().rpcs.remove(&id);
            return Err(Error::Lost("the socket thread is gone".to_owned()));
        }
        let mut st = self.shared.lock();
        loop {
            match st.rpcs.get(&id) {
                Some(Some(_)) => {
                    let got = st.rpcs.remove(&id).flatten().expect("a reply");
                    return got;
                }
                None => return Err(Error::Lost("the reply was dropped".to_owned())),
                Some(None) => {}
            }
            if !st.up || st.generation != generation {
                st.rpcs.remove(&id);
                return Err(Error::Lost(st.why_down.clone()));
            }
            let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
                st.rpcs.remove(&id);
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

    fn resync_due_now(&self) -> bool {
        self.resync_due && self.resync_at.is_none_or(|at| Instant::now() >= at)
    }

    /// A resync owed and due: tried, and on failure paced (1, 2, 4 ... 15 s)
    /// so a waiting caller never spins on `chat.history`.
    fn resync_if_due(&mut self, deadline: Instant) {
        if !self.resync_due_now() {
            return;
        }
        if let Err(e) = self.resync(deadline) {
            let delay =
                Duration::from_secs(1u64 << self.resync_fails.min(4)).min(Duration::from_secs(15));
            self.resync_fails = self.resync_fails.saturating_add(1);
            self.resync_at = Some(Instant::now() + delay);
            self.log(&format!(
                "resync failed ({e}); again in {} s",
                delay.as_secs()
            ));
        }
    }

    /// Brings open runs up to `chat.history` after a (re)connect. With the
    /// general resync off, after a drop that failed our runs only: a run of
    /// the kept session going that is not one of ours (the gateway's
    /// restart recovery of a failed task) is aborted, so it never holds the
    /// next task back; one that starts later is aborted at its first event
    /// (`io::abort_stray`).
    fn resync(&mut self, deadline: Instant) -> Result<(), Error> {
        if !RESYNC_ON_CONNECT {
            return self.abort_strays(deadline);
        }
        let (open, key) = {
            let st = self.shared.lock();
            (st.runs.open(), st.key.clone())
        };
        if open.is_empty() {
            self.resync_due = false;
            return Ok(());
        }
        let payload = self.rpc(
            method::CHAT_HISTORY,
            frames::chat_history(&key, self.cfg.history_limit),
            deadline,
        )?;
        let history = frames::history(&payload);
        self.keep_size(&history);
        let closed = self.shared.lock().runs.resync(&history);
        self.shared.cv.notify_all();
        self.resync_due = false;
        self.resync_at = None;
        self.resync_fails = 0;
        self.log(&format!(
            "resynced {} open run(s) from history, {} ended while away",
            open.len(),
            closed.len()
        ));
        Ok(())
    }

    /// One `chat.history` read on a reconnect after a drop: each run it
    /// says is going (`activeRunIds`, `inFlightRun`) that is not one of
    /// ours still going is aborted once.
    fn abort_strays(&mut self, deadline: Instant) -> Result<(), Error> {
        let key = self.session_key();
        let payload = self.rpc(
            method::CHAT_HISTORY,
            frames::chat_history(&key, 1),
            deadline,
        )?;
        let history = frames::history(&payload);
        self.keep_size(&history);
        self.resync_due = false;
        self.resync_at = None;
        self.resync_fails = 0;
        let going: Vec<String> = history
            .active_ids
            .clone()
            .unwrap_or_default()
            .into_iter()
            .chain(history.in_flight.map(|f| f.run_id))
            .collect();
        for run in going {
            let stray = {
                let mut st = self.shared.lock();
                st.runs.stray(&run) && st.strays_aborted.insert(run.clone())
            };
            if stray {
                let _ = self.rpc(
                    method::CHAT_ABORT,
                    frames::chat_abort(&key, Some(&run)),
                    deadline,
                );
                self.log(&format!("stray run {run} after the drop: aborted"));
            }
        }
        Ok(())
    }

    fn call_deadline(&self) -> Instant {
        Instant::now() + self.cfg.call_timeout
    }

    /// Reads the session's size once after `run` ended, best effort.
    fn refresh_tokens(&mut self, run: &RunId) {
        if self.tokens_read_for.as_ref() == Some(run) || !self.connected() {
            return;
        }
        self.tokens_read_for = Some(run.clone());
        self.size_fresh = false;
        let deadline = self.call_deadline();
        self.read_size(deadline);
    }

    /// One short `chat.history` (a page of one) for the session's size,
    /// best effort, kept ([`Self::keep_size`]).
    fn read_size(&mut self, deadline: Instant) {
        if !self.connected() {
            return;
        }
        let key = self.session_key();
        if let Ok(payload) = self.rpc(
            method::CHAT_HISTORY,
            frames::chat_history(&key, 1),
            deadline,
        ) {
            self.keep_size(&frames::history(&payload));
        }
    }

    /// A history read's size kept: its tokens and its message count.
    fn keep_size(&mut self, history: &frames::History) {
        if let Some(t) = history.total_tokens {
            self.history_tokens = Some(t);
            self.size_fresh = true;
        }
        if let Some(n) = history.total_messages {
            self.history_messages = Some(n);
        }
    }

    /// The kept session's size as the gateway counts it, `(tokens,
    /// messages)`: read now (one short `chat.history`) when not read since
    /// the last connect, reset or send; `None` when it can't be read. For
    /// the context rotation before a turn is sent.
    pub fn size_now(&mut self) -> Option<(u64, u64)> {
        if !self.size_fresh {
            let deadline = self.call_deadline();
            if self.ensure(deadline, false).is_ok() {
                self.read_size(deadline);
            }
        }
        if !self.size_fresh {
            return None;
        }
        Some((
            self.history_tokens.unwrap_or(0),
            self.history_messages.unwrap_or(0),
        ))
    }

    /// The session's tokens: the gateway's own count, read once after each
    /// run ends; before any read, the sum of this session's runs' `usage`
    /// (output tokens only).
    pub fn session_tokens(&self) -> u64 {
        self.history_tokens
            .unwrap_or_else(|| self.shared.lock().runs.tokens())
    }

    /// The gateway's exec approvals: `{path, exists, hash, file}` (the
    /// JSON `openclaw approvals get` printed).
    pub fn approvals_get(&mut self, within: Duration) -> Result<Value, Error> {
        let deadline = Instant::now() + within;
        self.ensure(deadline, true)?;
        self.rpc(method::APPROVALS_GET, frames::approvals_get(), deadline)
    }

    /// Adds `pattern` to the agent's exec allow-list (`openclaw approvals
    /// allowlist add`): read, edit, write with the read's hash, once more
    /// on a stale hash. `Ok(false)` when it was there already.
    pub fn allowlist_add(&mut self, pattern: &str, within: Duration) -> Result<bool, Error> {
        let deadline = Instant::now() + within;
        let agent = self.cfg.agent_id.clone();
        for attempt in 0..2 {
            let snapshot =
                self.approvals_get(deadline.saturating_duration_since(Instant::now()))?;
            let file = snapshot.get("file").cloned().unwrap_or(Value::Null);
            if frames::allowlist_has(&file, &agent, pattern) {
                return Ok(false);
            }
            let hash = snapshot
                .get("hash")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let file = frames::allowlist_with(file, &agent, pattern);
            match self.rpc(
                method::APPROVALS_SET,
                frames::approvals_set(&file, hash.as_deref()),
                deadline,
            ) {
                Ok(_) => {
                    self.log(&format!("allow-list of {agent}: added {pattern}"));
                    return Ok(true);
                }
                Err(Error::Refused(why)) if attempt == 0 => {
                    self.log(&format!("allow-list write refused ({why}); reading again"));
                }
                Err(e) => return Err(e),
            }
        }
        Err(Error::Refused("the approvals kept changing".to_owned()))
    }
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
        Engine::Openclaw
    }

    fn ready(&mut self, within: Duration) -> Result<(), Error> {
        self.ensure(Instant::now() + within, true)
    }

    fn send(&mut self, turn: &Turn) -> Result<RunId, Error> {
        let deadline = self.call_deadline();
        self.ensure(deadline, false)?;
        let pending = (turn.task_id.clone(), turn.message.clone());
        // A hand-over whose reply never came goes again with its own key.
        let id = match self.unconfirmed.get(&pending) {
            Some(id) if self.shared.lock().runs.contains(id) => id.clone(),
            _ => {
                let n = self.next_n;
                self.next_n += 1;
                let id = RunId(format!("lamp-{}-{n}", plain(&turn.task_id)));
                // Registered before the write: a fast run's events can come
                // before chat.send's reply.
                self.shared.lock().runs.register(
                    id.clone(),
                    &turn.task_id,
                    &turn.message,
                    turn.kind,
                );
                id
            }
        };
        let key = self.session_key();
        let params = frames::chat_send(
            &key,
            &turn.message,
            &id.0,
            self.cfg.send_queue_mode.as_deref(),
        );
        match self.rpc(method::CHAT_SEND, params, deadline) {
            Ok(payload) => {
                self.unconfirmed.remove(&pending);
                let gw = payload.get("runId").and_then(Value::as_str);
                if gw.is_some_and(|gw| gw != id.0) {
                    self.log(&format!(
                        "task {}: the gateway named the run {} (not its key)",
                        turn.task_id,
                        gw.unwrap_or_default()
                    ));
                }
                self.log(&format!("task {} handed over as {}", turn.task_id, id.0));
                // The run grows the session: read again after it.
                self.size_fresh = false;
                Ok(id)
            }
            // The gateway may have it: keep the run and its key.
            Err(e @ (Error::TimedOut | Error::Lost(_))) => {
                self.unconfirmed.insert(pending, id.clone());
                self.log(&format!(
                    "task {} not confirmed ({e}); the next send reuses {}",
                    turn.task_id, id.0
                ));
                Err(e)
            }
            Err(e) => {
                self.unconfirmed.remove(&pending);
                self.shared.lock().runs.forget(&id);
                self.log(&format!("task {} not handed over: {e}", turn.task_id));
                Err(e)
            }
        }
    }

    fn wait(&mut self, run: &RunId, within: Duration) -> Result<Outcome, Error> {
        let deadline = Instant::now() + within;
        loop {
            {
                let st = self.shared.lock();
                if !st.runs.contains(run) {
                    return Err(Error::UnknownRun(run.clone()));
                }
                if let Some(outcome) = st.runs.outcome(run) {
                    drop(st);
                    self.refresh_tokens(run);
                    return Ok(outcome);
                }
                let up = self
                    .link
                    .as_ref()
                    .is_some_and(|l| st.up && st.generation == l.generation);
                if up && !self.resync_due_now() {
                    let Some(wait) = deadline.checked_duration_since(Instant::now()) else {
                        return Err(Error::TimedOut);
                    };
                    // Woken by every event; at least every 100 ms for the
                    // end-without-final grace.
                    let _ = self
                        .shared
                        .cv
                        .wait_timeout(st, wait.min(Duration::from_millis(100)));
                    continue;
                }
            }
            if Instant::now() >= deadline {
                return Err(Error::TimedOut);
            }
            match self.ensure(deadline, true) {
                Ok(()) => {}
                Err(e @ Error::Refused(_)) => return Err(e),
                Err(_) => {
                    // A backoff past the deadline: sit it out, then say so.
                    if let Some(rest) = deadline.checked_duration_since(Instant::now()) {
                        std::thread::sleep(rest.min(Duration::from_millis(250)));
                    }
                }
            }
        }
    }

    fn progress(&self, run: &RunId) -> Option<Progress> {
        self.shared.lock().runs.progress(run)
    }

    fn running(&self) -> Vec<Progress> {
        self.shared.lock().runs.running()
    }

    fn abort(&mut self, run: &RunId) -> Result<(), Error> {
        let deadline = self.call_deadline();
        let key = {
            let st = self.shared.lock();
            if !st.runs.contains(run) {
                return Err(Error::UnknownRun(run.clone()));
            }
            if st.runs.outcome(run).is_some() {
                return Ok(());
            }
            st.key.clone()
        };
        self.ensure(deadline, false)?;
        let reply = self.rpc(
            method::CHAT_ABORT,
            frames::chat_abort(&key, Some(&run.0)),
            deadline,
        )?;
        // Aborted only when the gateway says so (`aborted`, or the run among
        // `runIds`); else its own `chat aborted` / end decides.
        let aborted = reply.get("aborted").and_then(Value::as_bool) == Some(true)
            || reply
                .get("runIds")
                .and_then(Value::as_array)
                .is_some_and(|ids| ids.iter().any(|r| r.as_str() == Some(run.0.as_str())));
        if aborted {
            self.shared.lock().runs.abort(run);
            self.shared.cv.notify_all();
        }
        self.log(&format!(
            "abort {}: {}",
            run.0,
            if aborted {
                "aborted"
            } else {
                "nothing to abort"
            }
        ));
        Ok(())
    }

    /// `sessions.reset {key, reason: "new"}`: the same key, a fresh
    /// conversation; the old transcript is archived by the gateway.
    fn reset(&mut self) -> Result<(), Error> {
        let deadline = self.call_deadline();
        self.ensure(deadline, false)?;
        let key = self.session_key();
        self.rpc(
            method::SESSIONS_RESET,
            frames::sessions_reset(&key),
            deadline,
        )?;
        {
            let mut st = self.shared.lock();
            st.runs.abort_all();
            st.runs.zero_tokens();
        }
        self.history_tokens = None;
        self.history_messages = None;
        self.size_fresh = false;
        self.shared.cv.notify_all();
        self.log(&format!("session {key} reset"));
        Ok(())
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("url", &self.cfg.url)
            .field("session_key", &self.session_key())
            .field("connected", &self.connected())
            .finish()
    }
}
