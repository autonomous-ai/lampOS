//! Durable task policy. A launch intent is committed before it can leave this
//! module. Transcript observations and qualified speech receipts are separate.
use crate::{
    harness::{Closed, Launch, LaunchOutcome, Observation, Target},
    protocol::{MAX_PAGE_SIZE, MAX_REPLY_BYTES, Request},
    store::{MAX_STATE_BYTES, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt, io};

pub const MAX_TASKS: usize = 16;
pub const MAX_OBSERVATIONS: usize = 4;
pub const MAX_PROMPT_BYTES: usize = 1000;
pub const MAX_ANSWER_BYTES: usize = 16_384;
pub const MAX_ASK_BYTES: usize = 4096;
pub const MAX_SPEECH_BYTES: usize = 1000;
pub const CONFIRM_MS: u64 = 120_000;
pub const SPEECH_AGE_MS: u64 = 600_000;
/// A close that failed is tried again this long after, at most
/// [`CLOSE_TRIES`] times in all, each time after reading the agent again.
pub const CLOSE_RETRY_MS: u64 = 60_000;
pub const CLOSE_TRIES: u8 = 3;
/// How often a task handed over is looked at on its own (unless `serve --no-watch`):
/// every half minute for its first ten minutes, then every two, until the
/// agent has finished or asks something, and for at most a day.
pub const WATCH_EARLY_MS: u64 = 30_000;
pub const WATCH_EARLY_FOR_MS: u64 = 600_000;
pub const WATCH_LATER_MS: u64 = 120_000;
pub const WATCH_AT_MOST_MS: u64 = 86_400_000;
/// How soon the watch looks again after it first finds an agent idle with
/// an answer, to see whether that turn holds ([`Task::watch_idle`]).
pub const WATCH_CONFIRM_MS: u64 = 10_000;
/// How long the watch goes on looking, every [`WATCH_LATER_MS`], after it
/// kept an idle answer: an agent that waits on its own background work for
/// longer than [`WATCH_CONFIRM_MS`] reads idle twice and then goes on, and
/// neither `show` nor Harness's own `wait` can tell (#1145 review). A
/// different answer it ends with later is kept as a new observation.
pub const WATCH_FOLLOW_MS: u64 = 600_000;

pub trait Persistence {
    fn load(&self) -> io::Result<Option<Vec<u8>>>;
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()>;
}
impl Persistence for Store {
    fn load(&self) -> io::Result<Option<Vec<u8>>> {
        self.load()
    }
    fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.commit(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: &'static str,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
fn err(code: &'static str, message: &'static str) -> Error {
    Error { code, message }
}
fn storage() -> Error {
    err(
        "storage",
        "Tasks storage is unavailable; restart and inspect before retrying.",
    )
}
fn exhausted() -> Error {
    err("full", "Tasks capacity or identifiers are exhausted.")
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Clock {
    pub boot_id: String,
    pub monotonic_ms: u64,
}
impl Clock {
    pub fn check(&self) -> Result<(), Error> {
        if !token(&self.boot_id, 128) {
            return Err(err("clock", "A valid boot identity is required."));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Audit {
    boot_id: String,
    session_id: String,
    monotonic_ms: u64,
    revision: u64,
}
impl Audit {
    fn follows(&self, before: &Self) -> bool {
        self.revision >= before.revision
            && (self.boot_id != before.boot_id || self.monotonic_ms >= before.monotonic_ms)
    }
    fn in_session(&self, session: &str, now: &Clock) -> bool {
        self.session_id == session
            && self.boot_id == now.boot_id
            && self.monotonic_ms <= now.monotonic_ms
    }
    fn check(&self, state: &State) -> bool {
        token(&self.boot_id, 128)
            && hex(&self.session_id, 32)
            && self.revision <= state.revision
            && (self.boot_id != state.saved_at.boot_id
                || self.monotonic_ms <= state.saved_at.monotonic_ms)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DispatchState {
    Prepared,
    Queued,
    Attempting,
    Accepted,
    Pending,
    NotSent,
    Uncertain,
    Cancelled,
    Invalidated,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Dispatch {
    state: DispatchState,
    confirmed: Option<Audit>,
    attempted: Option<Audit>,
    finished: Option<Audit>,
    agent_id: Option<String>,
    creation_id: Option<String>,
    code: Option<String>,
}
impl Dispatch {
    fn prepared() -> Self {
        Self {
            state: DispatchState::Prepared,
            confirmed: None,
            attempted: None,
            finished: None,
            agent_id: None,
            creation_id: None,
            code: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReadState {
    Requested,
    Attempting,
    Finished,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Read {
    request_id: u64,
    state: ReadState,
    requested: Audit,
    started: Option<Audit>,
    finished: Option<Audit>,
    observation_id: Option<u64>,
    code: Option<String>,
    /// Asked for by the watch, not by anyone: an agent still at work is
    /// not kept as an observation, and the read ends `working`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    watch: bool,
    /// Asked for because Harness's own `wait` said the agent ended
    /// ([`Engine::push`], `serve --push-wait`): its two looks at one turn
    /// stand for the watch's second look, so an idle answer is kept at once.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    confirmed: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SpeechState {
    Claimed,
    Spoken,
    Uncertain,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Speech {
    claim_id: u64,
    text: String,
    text_digest: String,
    state: SpeechState,
    claimed: Audit,
    finished: Option<Audit>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Observed {
    id: u64,
    digest: String,
    data: Observation,
    observed: Audit,
    last_seen: Audit,
    seen_count: u64,
    speech: Option<Speech>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CloseState {
    Attempting,
    /// Closed, or found already stopped or gone.
    Closed,
    /// Harness refused it: never asked again.
    Refused,
    /// The person went on with it at the computer: theirs, never closed.
    Left,
    /// Failed this time; tried again after [`CLOSE_RETRY_MS`], up to
    /// [`CLOSE_TRIES`].
    Failed,
}
/// The lamp closing the harness it started, once its result was told
/// (#1390).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Close {
    state: CloseState,
    tries: u8,
    attempted: Audit,
    finished: Option<Audit>,
    code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Task {
    id: u64,
    target_alias: String,
    launch: Launch,
    prepared_digest: String,
    prepare_request_id: String,
    created: Audit,
    updated: Audit,
    expires_monotonic_ms: u64,
    dispatch: Dispatch,
    read: Option<Read>,
    observations: Vec<Observed>,
    /// The watch's last look found the agent idle with this answer (the
    /// observation's digest), seen once. An agent can end a turn while it
    /// waits on its own background work and then go on (lamp-75c9,
    /// 2026-10-16, idle → working → idle on one line), so the watch keeps
    /// an idle answer only when the next look finds the same one, as
    /// Harness's own `wait` takes two looks at one turn for done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    watch_idle: Option<String>,
    /// Closing its harness once its result was told (#1390).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    close: Option<Close>,
    /// Made by a version that closes its harness once told. A task from an
    /// older store is never closed: its harness may be one the person has
    /// gone on with since, and the issue leaves those to the person.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    closable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    request: Request,
    response: Value,
    at: Audit,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct State {
    schema: u32,
    store_id: String,
    session_id: String,
    revision: u64,
    next_task_id: u64,
    next_read_id: u64,
    next_observation_id: u64,
    next_claim_id: u64,
    saved_at: Audit,
    tasks: Vec<Task>,
    last_receipt: Option<Receipt>,
}

fn token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}
fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
}
fn code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn agent_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
fn creation_id(value: &str) -> bool {
    (16..=96).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn speech_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_SPEECH_BYTES
        && !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
fn valid_observation(value: &Observation) -> bool {
    agent_id(&value.agent_id)
        && matches!(
            value.state.as_str(),
            "working"
                | "idle"
                | "needs-input"
                | "stopped"
                | "offline"
                | "unknown"
                | "installing"
                | "starting"
                | "failed"
        )
        && value.answer.len() <= MAX_ANSWER_BYTES
        && value.ask.as_ref().is_none_or(|v| v.len() <= MAX_ASK_BYTES)
}
/// Whether an agent has finished with what it was given, or stopped to
/// ask: what the person would want to hear about. An idle agent with no
/// answer has not begun.
fn settled(value: &Observation) -> bool {
    match value.state.as_str() {
        "idle" => !value.answer.trim().is_empty(),
        "needs-input" | "stopped" | "failed" => true,
        _ => false,
    }
}
/// Whether this lamp should close the harness it started for `task` now
/// (#1390): a task this version made, whose newest answer was kept from an
/// agent `idle` with an answer, and told (its telling played, or not known
/// to have). Harness's `ask` is what the agent was last asked, the lamp's
/// own task text, so it says nothing here. One that waits for the person
/// (`needs-input`) is left to them: Harness would refuse it anyway. The
/// close itself first reads the agent again, and closes only if it is
/// still idle on that turn. A close that failed is tried again after
/// [`CLOSE_RETRY_MS`], [`CLOSE_TRIES`] times in all; a refused, left or
/// done one never.
fn close_due(task: &Task, now: &Clock) -> bool {
    if !task.closable
        || task.dispatch.state != DispatchState::Accepted
        || task.dispatch.agent_id.is_none()
    {
        return false;
    }
    match &task.close {
        None => {}
        Some(close) if close.state == CloseState::Failed && close.tries < CLOSE_TRIES => {
            let last = close.finished.as_ref().unwrap_or(&close.attempted);
            if last.boot_id == now.boot_id
                && now.monotonic_ms.saturating_sub(last.monotonic_ms) < CLOSE_RETRY_MS
            {
                return false;
            }
        }
        Some(_) => return false,
    }
    let (Some(first), Some(newest)) = (task.observations.first(), task.observations.last()) else {
        return false;
    };
    // The told turn must still be the lamp's own task: the same asked text
    // as its first kept answer. A newer one asked by the person at the
    // computer ("Now make it longer.") is theirs, kept and told or not.
    newest.data.ask == first.data.ask
        && newest.data.state == "idle"
        && settled(&newest.data)
        && newest
            .speech
            .as_ref()
            .is_some_and(|speech| speech.state != SpeechState::Claimed)
}

/// Whether a task is over, so that it may give way to a new one when the
/// store is full: it was never sent (it expired, was cancelled or was not
/// sent), or its agent has finished, stopped or failed. A question waiting
/// for the person, and a hand-over whose outcome is not known, are not over.
/// On lamp-4ace on 2026-10-15 the sixteenth task filled the store, and with
/// nothing ever giving way the lamp could hand nothing over again.
fn spent(task: &Task) -> bool {
    match task.dispatch.state {
        DispatchState::Invalidated | DispatchState::Cancelled | DispatchState::NotSent => true,
        DispatchState::Accepted => task
            .observations
            .last()
            .is_some_and(|seen| settled(&seen.data) && seen.data.state != "needs-input"),
        _ => false,
    }
}
fn digest(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("owned serializable data");
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn text_digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn task_label(store_id: &str, id: u64) -> String {
    format!("lamp-{}", &digest(&(store_id, id))[..32])
}

impl State {
    fn check(&self) -> Result<(), Error> {
        let fail = storage;
        if self.schema != 1
            || !hex(&self.store_id, 32)
            || !hex(&self.session_id, 32)
            || self.tasks.len() > MAX_TASKS
            || self.next_task_id == 0
            || self.next_read_id == 0
            || self.next_observation_id == 0
            || self.next_claim_id == 0
            || !self.saved_at.check(self)
            || self.saved_at.session_id != self.session_id
            || self.saved_at.revision != self.revision
        {
            return Err(fail());
        }
        let mut prior_task = 0;
        let mut observations = std::collections::BTreeSet::new();
        let mut claims = std::collections::BTreeSet::new();
        let mut reads = std::collections::BTreeSet::new();
        let mut prepare_ids = std::collections::BTreeSet::new();
        let mut agent_ids = std::collections::BTreeSet::new();
        let mut creation_ids = std::collections::BTreeSet::new();
        let mut active = 0;
        for task in &self.tasks {
            if task.id <= prior_task
                || task.id >= self.next_task_id
                || !token(&task.target_alias, 64)
                || task.launch.validate().is_err()
                || task.launch.prompt.len() > MAX_PROMPT_BYTES
                || task.launch.name != task_label(&self.store_id, task.id)
                || task.prepared_digest
                    != digest(&(&self.store_id, task.id, &task.target_alias, &task.launch))
                || !request_id(&task.prepare_request_id)
                || !prepare_ids.insert(&task.prepare_request_id)
                || !task.created.check(self)
                || !task.updated.check(self)
                || !task.updated.follows(&task.created)
                || task.created.monotonic_ms.checked_add(CONFIRM_MS)
                    != Some(task.expires_monotonic_ms)
                || task.observations.len() > MAX_OBSERVATIONS
            {
                return Err(fail());
            }
            prior_task = task.id;
            let d = &task.dispatch;
            if d.agent_id.as_ref().is_some_and(|id| !agent_ids.insert(id))
                || d.creation_id
                    .as_ref()
                    .is_some_and(|id| !creation_ids.insert(id))
            {
                return Err(fail());
            }
            let confirmed = d.confirmed.as_ref().is_some_and(|a| {
                a.check(self)
                    && a.follows(&task.created)
                    && a.session_id == task.created.session_id
                    && a.boot_id == task.created.boot_id
                    && a.monotonic_ms < task.expires_monotonic_ms
                    && task.updated.follows(a)
            });
            let attempted = d.attempted.as_ref().is_some_and(|a| {
                a.check(self)
                    && d.confirmed.as_ref().is_some_and(|b| {
                        a.follows(b) && a.session_id == b.session_id && a.boot_id == b.boot_id
                    })
                    && a.monotonic_ms < task.expires_monotonic_ms
                    && task.updated.follows(a)
            });
            let finished = d.finished.as_ref().is_some_and(|a| {
                a.check(self)
                    && a.follows(
                        d.attempted
                            .as_ref()
                            .or(d.confirmed.as_ref())
                            .unwrap_or(&task.created),
                    )
                    && task.updated.follows(a)
            });
            let base = d.agent_id.is_none() && d.creation_id.is_none() && d.code.is_none();
            let valid = match d.state {
                DispatchState::Prepared => {
                    d.confirmed.is_none()
                        && d.attempted.is_none()
                        && d.finished.is_none()
                        && base
                        && task.created.session_id == self.session_id
                }
                DispatchState::Queued => {
                    confirmed
                        && d.attempted.is_none()
                        && d.finished.is_none()
                        && base
                        && task.created.session_id == self.session_id
                }
                DispatchState::Attempting => {
                    active += 1;
                    confirmed
                        && attempted
                        && d.finished.is_none()
                        && base
                        && task.created.session_id == self.session_id
                }
                DispatchState::Accepted => {
                    confirmed
                        && attempted
                        && finished
                        && d.agent_id.as_deref().is_some_and(agent_id)
                        && d.creation_id.is_none()
                        && d.code.is_none()
                }
                DispatchState::Pending => {
                    confirmed
                        && attempted
                        && finished
                        && d.creation_id.as_deref().is_some_and(creation_id)
                        && d.agent_id.is_none()
                        && d.code.is_none()
                }
                DispatchState::NotSent | DispatchState::Uncertain => {
                    confirmed
                        && attempted
                        && finished
                        && d.agent_id.is_none()
                        && d.creation_id.is_none()
                        && d.code.as_deref().is_some_and(code)
                }
                DispatchState::Cancelled => {
                    (d.confirmed.is_none() || confirmed)
                        && d.attempted.is_none()
                        && finished
                        && base
                }
                DispatchState::Invalidated => {
                    (d.confirmed.is_none() || confirmed)
                        && d.attempted.is_none()
                        && finished
                        && d.agent_id.is_none()
                        && d.creation_id.is_none()
                        && matches!(d.code.as_deref(), Some("restart" | "expired"))
                }
            };
            if !valid {
                return Err(fail());
            }
            let mut prior_observation = 0;
            for observed in &task.observations {
                if d.state != DispatchState::Accepted
                    || d.agent_id.as_ref() != Some(&observed.data.agent_id)
                    || !valid_observation(&observed.data)
                    || observed.digest != digest(&observed.data)
                    || observed.id <= prior_observation
                    || observed.id >= self.next_observation_id
                    || !observations.insert(observed.id)
                    || !observed.observed.check(self)
                    || !observed.last_seen.check(self)
                    || !observed
                        .observed
                        .follows(d.finished.as_ref().ok_or_else(fail)?)
                    || !observed.last_seen.follows(&observed.observed)
                    || !task.updated.follows(&observed.last_seen)
                    || observed.seen_count == 0
                {
                    return Err(fail());
                }
                prior_observation = observed.id;
                if task
                    .observations
                    .iter()
                    .filter(|o| o.digest == observed.digest)
                    .count()
                    != 1
                {
                    return Err(fail());
                }
                if let Some(speech) = &observed.speech {
                    if speech.claim_id == 0
                        || speech.claim_id >= self.next_claim_id
                        || !claims.insert(speech.claim_id)
                        || !speech_text(&speech.text)
                        || speech.text_digest != text_digest(&speech.text)
                        || !speech.claimed.check(self)
                        || !speech.claimed.follows(&observed.observed)
                        || speech.claimed.boot_id != observed.observed.boot_id
                        || speech.claimed.session_id != observed.observed.session_id
                        || speech.claimed.monotonic_ms - observed.observed.monotonic_ms
                            >= SPEECH_AGE_MS
                        || !task.updated.follows(&speech.claimed)
                    {
                        return Err(fail());
                    }
                    match speech.state {
                        SpeechState::Claimed
                            if speech.finished.is_none()
                                && speech.claimed.session_id == self.session_id => {}
                        SpeechState::Spoken | SpeechState::Uncertain
                            if speech.finished.as_ref().is_some_and(|a| {
                                a.check(self)
                                    && a.follows(&speech.claimed)
                                    && task.updated.follows(a)
                                    && (speech.state != SpeechState::Spoken
                                        || a.session_id == speech.claimed.session_id
                                            && a.boot_id == speech.claimed.boot_id)
                            }) => {}
                        _ => return Err(fail()),
                    }
                }
            }
            if let Some(read) = &task.read {
                if d.state != DispatchState::Accepted
                    || read.request_id == 0
                    || read.request_id >= self.next_read_id
                    || !reads.insert(read.request_id)
                    || !read.requested.check(self)
                    || !read
                        .requested
                        .follows(d.finished.as_ref().ok_or_else(fail)?)
                    || !task.updated.follows(&read.requested)
                {
                    return Err(fail());
                }
                let started = read.started.as_ref().is_some_and(|a| {
                    a.check(self)
                        && a.follows(&read.requested)
                        && a.session_id == read.requested.session_id
                        && a.boot_id == read.requested.boot_id
                        && task.updated.follows(a)
                });
                let finished = read.finished.as_ref().is_some_and(|a| {
                    a.check(self)
                        && a.follows(read.started.as_ref().unwrap_or(&read.requested))
                        && task.updated.follows(a)
                });
                let valid = match read.state {
                    ReadState::Requested => {
                        read.started.is_none()
                            && read.finished.is_none()
                            && read.observation_id.is_none()
                            && read.code.is_none()
                            && read.requested.session_id == self.session_id
                    }
                    ReadState::Attempting => {
                        active += 1;
                        started
                            && read.finished.is_none()
                            && read.observation_id.is_none()
                            && read.code.is_none()
                            && read.requested.session_id == self.session_id
                    }
                    ReadState::Finished => {
                        started
                            && finished
                            && read.code.is_none()
                            && task
                                .observations
                                .iter()
                                .any(|o| Some(o.id) == read.observation_id)
                    }
                    ReadState::Failed => {
                        (read.started.is_none() || started)
                            && finished
                            && read.observation_id.is_none()
                            && read.code.as_deref().is_some_and(code)
                    }
                };
                if !valid {
                    return Err(fail());
                }
            }
        }
        if active > 1 {
            return Err(fail());
        }
        if let Some(receipt) = &self.last_receipt {
            let guard = receipt.request.guard().ok_or_else(fail)?;
            if guard.store_id != self.store_id
                || !request_id(&guard.request_id)
                || guard.revision.checked_add(1) != Some(receipt.at.revision)
                || !receipt.at.check(self)
                || receipt.response
                    != identity(
                        receipt_body(self, &receipt.request)?,
                        &self.store_id,
                        receipt.at.revision,
                    )
                || receipt
                    .request
                    .session_id()
                    .is_some_and(|s| s != receipt.at.session_id)
                || receipt_effect(self, &receipt.request)? != &receipt.at
            {
                return Err(fail());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchJob {
    pub task_id: u64,
    pub launch: Launch,
}
/// A task the watch looks at, as progress reads it (`serve --progress`):
/// accepted, nothing settled kept yet, and handed over less than a day ago
/// in this boot or in an earlier one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watched {
    pub task_id: u64,
    pub machine_id: String,
    pub agent_id: String,
    /// Who has it, as the task's target names it (`claude`, `codex`).
    pub engine: String,
    /// How long ago it was handed over, if that was in this boot.
    pub accepted_ago_ms: Option<u64>,
}

/// A task whose agent settled ([`Engine::settled_within`]), as the watch's
/// own read found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settled {
    pub task_id: u64,
    /// Who had it, as the task's target names it (`claude`, `codex`).
    pub engine: String,
    /// As Harness wrote it: `idle`, `needs-input`, `stopped` or `failed`.
    pub state: String,
    /// How long ago that read found it settled, by this boot's clock.
    pub ago_ms: u64,
}

/// One harness to close ([`Engine::begin_close`]): closed only if a read just
/// before finds it idle on the very turn the lamp told, this answer to this
/// asked text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseJob {
    pub task_id: u64,
    pub machine_id: String,
    pub agent_id: String,
    pub answer: String,
    pub asked: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationJob {
    pub task_id: u64,
    pub request_id: u64,
    /// The machine the agent was started on: Harness reads another
    /// machine's agent only when told which (`--machine`, since 0.4.2).
    pub machine_id: String,
    pub agent_id: String,
}

pub struct Engine<P: Persistence> {
    persistence: P,
    targets: BTreeMap<String, Target>,
    state: State,
    last_clock: Clock,
    failed: bool,
    /// Whether the newest hand-over in this run reached Harness: true when
    /// it was created or pending there, false when it was not sent or its
    /// fate is uncertain. None before the first (#830: with no internet
    /// every hand-over fails while the store itself is well).
    last_handover: Option<bool>,
    /// `serve --close-told` (#1390): only a task made while it is on is
    /// marked `closable`, so turning it on later closes no older task.
    close_told: bool,
}
impl<P: Persistence> Engine<P> {
    /// Whether tasks made from now on are closed once told
    /// (`serve --close-told`).
    pub fn close_told(&mut self, on: bool) {
        self.close_told = on;
    }

    pub fn open(
        mut persistence: P,
        targets: BTreeMap<String, Target>,
        new_store_id: &str,
        session_id: &str,
        now: &Clock,
    ) -> Result<Self, Error> {
        now.check()?;
        if !hex(session_id, 32)
            || targets.len() > 32
            || targets
                .iter()
                .any(|(alias, target)| !token(alias, 64) || target.validate().is_err())
        {
            return Err(err(
                "configuration",
                "Task targets or service identity are invalid.",
            ));
        }
        let bytes = persistence.load().map_err(|_| storage())?;
        let existing = bytes.is_some();
        let mut state = if let Some(bytes) = bytes {
            if bytes.is_empty() || bytes.len() > MAX_STATE_BYTES {
                return Err(storage());
            }
            serde_json::from_slice::<State>(&bytes).map_err(|_| storage())?
        } else {
            if !hex(new_store_id, 32) {
                return Err(err(
                    "configuration",
                    "A fresh random store identity is required.",
                ));
            }
            State {
                schema: 1,
                store_id: new_store_id.into(),
                session_id: session_id.into(),
                revision: 0,
                next_task_id: 1,
                next_read_id: 1,
                next_observation_id: 1,
                next_claim_id: 1,
                saved_at: Audit {
                    boot_id: now.boot_id.clone(),
                    session_id: session_id.into(),
                    monotonic_ms: now.monotonic_ms,
                    revision: 0,
                },
                tasks: Vec::new(),
                last_receipt: None,
            }
        };
        state.check()?;
        if existing {
            if state.session_id == session_id
                || state.saved_at.boot_id == now.boot_id
                    && state.saved_at.monotonic_ms > now.monotonic_ms
            {
                return Err(err(
                    "clock",
                    "Use a new service session and a nondecreasing boot clock.",
                ));
            }
            state.session_id = session_id.into();
            let at = next_audit(&mut state, now)?;
            for task in &mut state.tasks {
                let mut changed = false;
                match task.dispatch.state {
                    DispatchState::Prepared | DispatchState::Queued => {
                        task.dispatch.state = DispatchState::Invalidated;
                        task.dispatch.finished = Some(at.clone());
                        task.dispatch.code = Some("restart".into());
                        changed = true;
                    }
                    DispatchState::Attempting => {
                        task.dispatch.state = DispatchState::Uncertain;
                        task.dispatch.finished = Some(at.clone());
                        task.dispatch.code = Some("restart".into());
                        changed = true;
                    }
                    _ => {}
                }
                if let Some(read) = &mut task.read
                    && matches!(read.state, ReadState::Requested | ReadState::Attempting)
                {
                    read.state = ReadState::Failed;
                    read.finished = Some(at.clone());
                    read.code = Some("restart".into());
                    changed = true;
                }
                if let Some(close) = &mut task.close
                    && close.state == CloseState::Attempting
                {
                    // Not known whether it closed: tried again, as closing
                    // twice is harmless.
                    close.state = CloseState::Failed;
                    close.finished = Some(at.clone());
                    close.code = Some("restart".into());
                    changed = true;
                }
                for observed in &mut task.observations {
                    if let Some(speech) = &mut observed.speech
                        && speech.state == SpeechState::Claimed
                    {
                        speech.state = SpeechState::Uncertain;
                        speech.finished = Some(at.clone());
                        changed = true;
                    }
                }
                if changed {
                    task.updated = at.clone();
                }
            }
        }
        state.check()?;
        let bytes = serde_json::to_vec(&state).map_err(|_| storage())?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(storage());
        }
        persistence.commit(&bytes).map_err(|_| storage())?;
        Ok(Self {
            persistence,
            targets,
            state,
            last_clock: now.clone(),
            failed: false,
            last_handover: None,
            close_told: false,
        })
    }

    pub fn store_id(&self) -> &str {
        &self.state.store_id
    }
    pub fn session_id(&self) -> &str {
        &self.state.session_id
    }
    pub fn revision(&self) -> u64 {
        self.state.revision
    }
    pub fn failed(&self) -> bool {
        self.failed
    }

    fn clock(&mut self, now: &Clock) -> Result<(), Error> {
        if self.failed {
            return Err(storage());
        }
        if now.check().is_err()
            || now.boot_id != self.last_clock.boot_id
            || now.monotonic_ms < self.last_clock.monotonic_ms
        {
            self.failed = true;
            return Err(err(
                "clock",
                "The boot clock changed or moved backwards; restart Tasks.",
            ));
        }
        self.last_clock = now.clone();
        Ok(())
    }
    fn save(&mut self, next: State) -> Result<(), Error> {
        next.check()?;
        let bytes = serde_json::to_vec(&next).map_err(|_| storage())?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(exhausted());
        }
        if self.persistence.commit(&bytes).is_err() {
            self.failed = true;
            return Err(storage());
        }
        self.state = next;
        Ok(())
    }
    pub fn tick(&mut self, now: &Clock) -> Result<bool, Error> {
        self.clock(now)?;
        if !self.state.tasks.iter().any(|t| {
            matches!(
                t.dispatch.state,
                DispatchState::Prepared | DispatchState::Queued
            ) && now.monotonic_ms >= t.expires_monotonic_ms
        }) {
            return Ok(false);
        }
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        for task in &mut next.tasks {
            if matches!(
                task.dispatch.state,
                DispatchState::Prepared | DispatchState::Queued
            ) && now.monotonic_ms >= task.expires_monotonic_ms
            {
                task.dispatch.state = DispatchState::Invalidated;
                task.dispatch.code = Some("expired".into());
                task.dispatch.finished = Some(at.clone());
                task.updated = at.clone();
            }
        }
        self.save(next)?;
        Ok(true)
    }

    pub fn summary(&self) -> Value {
        let count = |state| {
            self.state
                .tasks
                .iter()
                .filter(|t| t.dispatch.state == state)
                .count()
        };
        let speech = |state| {
            self.state
                .tasks
                .iter()
                .flat_map(|t| &t.observations)
                .filter(|o| o.speech.as_ref().is_some_and(|s| s.state == state))
                .count()
        };
        json!({"store_id":self.store_id(),"revision":self.revision(),"session_id":self.session_id(),"failed":self.failed,
            "retained":self.state.tasks.len(),"prepared":count(DispatchState::Prepared),"queued":count(DispatchState::Queued),
            "attempting":count(DispatchState::Attempting),"accepted":count(DispatchState::Accepted),"pending":count(DispatchState::Pending),
            "uncertain":count(DispatchState::Uncertain),"observations":self.state.tasks.iter().map(|t|t.observations.len()).sum::<usize>(),
            "speech_claimed":speech(SpeechState::Claimed),"speech_spoken":speech(SpeechState::Spoken),"speech_uncertain":speech(SpeechState::Uncertain),"completion":"unverified",
            "last_handover":self.last_handover.map(|handed| if handed { "handed" } else { "failed" })})
    }

    pub fn handle(
        &mut self,
        request: &Request,
        now: &Clock,
        trusted: bool,
    ) -> Result<Value, Error> {
        if request.trusted() && !trusted {
            return Err(err(
                "untrusted",
                "This operation requires the trusted Voice route.",
            ));
        }
        self.tick(now)?;
        if request
            .session_id()
            .is_some_and(|session| session != self.session_id())
        {
            return Err(err(
                "stale_session",
                "The service session changed; obtain a fresh confirmation or speech claim.",
            ));
        }
        let read = match request {
            Request::Status {} => Some(
                json!({"summary":self.summary(),"max_tasks":MAX_TASKS,"max_observations":MAX_OBSERVATIONS,
                "last_receipt":self.state.last_receipt.as_ref().map(|r| {
                    let mut response = r.response.clone();
                    if let Some(object) = response.as_object_mut() { object.remove("text"); }
                    json!({"request_id":r.request.guard().expect("checked receipt").request_id,"op":r.request.name(),"response":response})
                })}),
            ),
            // In alias order, as loaded. The machine and project stay here.
            Request::Targets {} => Some(json!({"targets":self.targets.iter()
                .map(|(alias, target)| json!({"alias":alias,"engine":target.engine}))
                .collect::<Vec<_>>()})),
            Request::List { after_id, limit } => {
                if !(1..=MAX_PAGE_SIZE).contains(limit) {
                    return Err(err("bad_request", "Page size must be between 1 and 8."));
                }
                let all: Vec<_> = self
                    .state
                    .tasks
                    .iter()
                    .filter(|t| t.id > *after_id)
                    .take(usize::from(*limit) + 1)
                    .collect();
                let items: Vec<_> = all
                    .iter()
                    .take(usize::from(*limit))
                    .map(|t| task_metadata(t))
                    .collect();
                let next_after_id = (all.len() > items.len()).then(|| all[items.len() - 1].id);
                Some(json!({"items":items,"next_after_id":next_after_id}))
            }
            Request::GetTask { id } => {
                let task = &self.state.tasks[find(&self.state, *id)?];
                Some(
                    json!({"task":task_metadata(task),"text":task.launch.prompt,"target":task.launch.target,
                    "label":task.launch.name,"prepared_digest":task.prepared_digest,"expires_monotonic_ms":task.expires_monotonic_ms,
                    "observations":task.observations.iter().map(observation_metadata).collect::<Vec<_>>()}),
                )
            }
            Request::GetObservation { id, observation_id } => {
                let task = &self.state.tasks[find(&self.state, *id)?];
                let observed = &task.observations[find_observation(task, *observation_id)?];
                Some(
                    json!({"id":id,"observation":observation_metadata(observed),"data":observed.data,"speech":observed.speech,"completion":"unverified"}),
                )
            }
            // Progress is held by the service around the engine, and only
            // with `--progress`; one that reaches here is not kept.
            Request::GetProgress { .. } => {
                return Err(err(
                    "progress_off",
                    "Progress is not read; the service runs without --progress.",
                ));
            }
            _ => None,
        };
        if let Some(body) = read {
            return self.reply(body);
        }
        let guard = request.guard().expect("mutation guard");
        if guard.store_id != self.store_id() || !request_id(&guard.request_id) {
            return Err(err(
                "bad_guard",
                "Use the current store identity and a valid request ID.",
            ));
        }
        if let Some(receipt) = &self.state.last_receipt {
            if receipt.request == *request {
                if matches!(request, Request::ClaimSpeech { .. }) {
                    return Err(err(
                        "claim_uncertain",
                        "That speech claim was already handed out; inspect it without replaying audio.",
                    ));
                }
                return Ok(receipt.response.clone());
            }
            if receipt.request.guard().expect("checked receipt").request_id == guard.request_id {
                return Err(err(
                    "request_conflict",
                    "A request ID cannot name different operations.",
                ));
            }
        }
        if guard.revision != self.revision() {
            return Err(err(
                "conflict",
                "Tasks changed; inspect current state without silently retrying a mutation.",
            ));
        }
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        match request {
            Request::Prepare { text, target, .. } => {
                // Full: the oldest task that is over gives way. Only when
                // every task kept is still going is a new one refused.
                if next.tasks.len() >= MAX_TASKS {
                    let oldest = next.tasks.iter().position(spent).ok_or_else(exhausted)?;
                    next.tasks.remove(oldest);
                }
                if next
                    .tasks
                    .iter()
                    .any(|t| t.prepare_request_id == guard.request_id)
                {
                    return Err(err(
                        "request_conflict",
                        "That preparation request ID was already used.",
                    ));
                }
                let target_value = self
                    .targets
                    .get(target)
                    .ok_or_else(|| err("target", "Choose a provisioned target alias."))?;
                let id = take(&mut next.next_task_id)?;
                let launch = Launch {
                    name: task_label(&next.store_id, id),
                    prompt: text.clone(),
                    target: target_value.clone(),
                };
                if launch.validate().is_err() || text.len() > MAX_PROMPT_BYTES {
                    return Err(err(
                        "bad_text",
                        "Task text or provisioned target is not valid.",
                    ));
                }
                let prepared_digest = digest(&(&next.store_id, id, target, &launch));
                let expires_monotonic_ms = now
                    .monotonic_ms
                    .checked_add(CONFIRM_MS)
                    .ok_or_else(exhausted)?;
                next.tasks.push(Task {
                    id,
                    target_alias: target.clone(),
                    launch,
                    prepared_digest,
                    prepare_request_id: guard.request_id.clone(),
                    created: at.clone(),
                    updated: at.clone(),
                    expires_monotonic_ms,
                    dispatch: Dispatch::prepared(),
                    read: None,
                    observations: Vec::new(),
                    watch_idle: None,
                    close: None,
                    closable: self.close_told,
                });
            }
            Request::Confirm {
                id,
                prepared_digest,
                ..
            } => {
                let task = &mut next.tasks[find(&self.state, *id)?];
                if task.dispatch.state != DispatchState::Prepared
                    || task.prepared_digest != *prepared_digest
                    || !task.created.in_session(self.session_id(), now)
                    || now.monotonic_ms >= task.expires_monotonic_ms
                {
                    return Err(err(
                        "stale_confirmation",
                        "The exact prepared task needs a fresh spoken confirmation.",
                    ));
                }
                task.dispatch.state = DispatchState::Queued;
                task.dispatch.confirmed = Some(at.clone());
                task.updated = at.clone();
            }
            Request::Cancel { id, .. } => {
                let task = &mut next.tasks[find(&self.state, *id)?];
                if !matches!(
                    task.dispatch.state,
                    DispatchState::Prepared | DispatchState::Queued
                ) {
                    return Err(err(
                        "already_attempted",
                        "Only a task that has not begun dispatch can be cancelled here.",
                    ));
                }
                task.dispatch.state = DispatchState::Cancelled;
                task.dispatch.finished = Some(at.clone());
                task.updated = at.clone();
            }
            Request::Observe { id, .. } => {
                let index = find(&self.state, *id)?;
                let task = &next.tasks[index];
                if task.dispatch.state != DispatchState::Accepted {
                    return Err(err(
                        "unresolved",
                        "Only a task with an exact accepted agent ID can be observed.",
                    ));
                }
                if task.read.as_ref().is_some_and(|r| {
                    matches!(r.state, ReadState::Requested | ReadState::Attempting)
                }) {
                    return Err(err("busy", "An observation is already requested."));
                }
                if task.observations.len() >= MAX_OBSERVATIONS {
                    return Err(exhausted());
                }
                let request_id = take(&mut next.next_read_id)?;
                let task = &mut next.tasks[index];
                task.read = Some(Read {
                    request_id,
                    state: ReadState::Requested,
                    requested: at.clone(),
                    started: None,
                    finished: None,
                    observation_id: None,
                    code: None,
                    watch: false,
                    confirmed: false,
                });
                task.updated = at.clone();
            }
            Request::ClaimSpeech {
                id,
                observation_id,
                text,
                ..
            } => {
                if !speech_text(text) {
                    return Err(err(
                        "bad_text",
                        "Speech text must be nonempty and at most 1000 UTF-8 bytes.",
                    ));
                }
                let index = find(&self.state, *id)?;
                let oi = find_observation(&next.tasks[index], *observation_id)?;
                let observed = &next.tasks[index].observations[oi];
                if observed.speech.is_some() {
                    return Err(err(
                        "already_claimed",
                        "This observation already has a durable speech claim.",
                    ));
                }
                if !observed.observed.in_session(self.session_id(), now)
                    || now.monotonic_ms - observed.observed.monotonic_ms >= SPEECH_AGE_MS
                {
                    return Err(err(
                        "stale_observation",
                        "Speech requires a recent observation from this service session.",
                    ));
                }
                let claim_id = take(&mut next.next_claim_id)?;
                let task = &mut next.tasks[index];
                task.observations[oi].speech = Some(Speech {
                    claim_id,
                    text: text.clone(),
                    text_digest: text_digest(text),
                    state: SpeechState::Claimed,
                    claimed: at.clone(),
                    finished: None,
                });
                task.updated = at.clone();
            }
            Request::AckSpeech {
                id,
                observation_id,
                claim_id,
                text_digest,
                played,
                ..
            } => {
                let task = &mut next.tasks[find(&self.state, *id)?];
                let oi = find_observation(task, *observation_id)?;
                let speech = task.observations[oi]
                    .speech
                    .as_mut()
                    .ok_or_else(|| err("stale_claim", "That exact speech claim is not active."))?;
                if !played
                    || speech.state != SpeechState::Claimed
                    || speech.claim_id != *claim_id
                    || speech.text_digest != *text_digest
                    || !speech.claimed.in_session(self.session_id(), now)
                {
                    return Err(err(
                        "stale_claim",
                        "Only the exact qualified completed playout may acknowledge this claim.",
                    ));
                }
                speech.state = SpeechState::Spoken;
                speech.finished = Some(at.clone());
                task.updated = at.clone();
            }
            _ => unreachable!("read requests returned above"),
        }
        let response = identity(receipt_body(&next, request)?, &next.store_id, next.revision);
        if serde_json::to_vec(&response).map_err(|_| storage())?.len() > MAX_REPLY_BYTES {
            return Err(err("too_long", "The response exceeds the protocol bound."));
        }
        next.last_receipt = Some(Receipt {
            request: request.clone(),
            response: response.clone(),
            at,
        });
        self.save(next)?;
        Ok(response)
    }

    /// A read's reply with this store's identity, as every reply has it.
    pub fn identified(&self, body: Value) -> Result<Value, Error> {
        self.reply(body)
    }
    fn reply(&self, body: Value) -> Result<Value, Error> {
        let response = identity(body, self.store_id(), self.revision());
        if serde_json::to_vec(&response).map_err(|_| storage())?.len() > MAX_REPLY_BYTES {
            return Err(err("too_long", "The response exceeds the protocol bound."));
        }
        Ok(response)
    }
    fn active(&self) -> bool {
        self.state.tasks.iter().any(|t| {
            t.dispatch.state == DispatchState::Attempting
                || t.read
                    .as_ref()
                    .is_some_and(|r| r.state == ReadState::Attempting)
                || t.close
                    .as_ref()
                    .is_some_and(|c| c.state == CloseState::Attempting)
        })
    }

    /// The returned job is permission for one invocation only. A caller that
    /// loses the return must reopen as uncertain, never obtain the job again.
    pub fn begin_launch(&mut self, now: &Clock) -> Result<Option<LaunchJob>, Error> {
        self.tick(now)?;
        if self.active() {
            return Ok(None);
        }
        let Some(index) = self
            .state
            .tasks
            .iter()
            .position(|t| t.dispatch.state == DispatchState::Queued)
        else {
            return Ok(None);
        };
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        let task = &mut next.tasks[index];
        task.dispatch.state = DispatchState::Attempting;
        task.dispatch.attempted = Some(at.clone());
        task.updated = at;
        let job = LaunchJob {
            task_id: task.id,
            launch: task.launch.clone(),
        };
        self.save(next)?;
        Ok(Some(job))
    }

    pub fn finish_launch(
        &mut self,
        task_id: u64,
        outcome: LaunchOutcome,
        now: &Clock,
    ) -> Result<(), Error> {
        self.clock(now)?;
        let index = find(&self.state, task_id)?;
        if self.state.tasks[index].dispatch.state != DispatchState::Attempting {
            return Err(err(
                "stale_job",
                "The launch is not the current committed attempt.",
            ));
        }
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        let reused = match &outcome {
            LaunchOutcome::Created { agent_id } => next
                .tasks
                .iter()
                .any(|task| task.dispatch.agent_id.as_ref() == Some(agent_id)),
            LaunchOutcome::Pending { creation_id } => next
                .tasks
                .iter()
                .any(|task| task.dispatch.creation_id.as_ref() == Some(creation_id)),
            _ => false,
        };
        let outcome = if reused {
            LaunchOutcome::Uncertain {
                code: "reused_remote_id".into(),
            }
        } else {
            outcome
        };
        let task = &mut next.tasks[index];
        match outcome {
            LaunchOutcome::Created { agent_id: id } if agent_id(&id) => {
                task.dispatch.state = DispatchState::Accepted;
                task.dispatch.agent_id = Some(id);
            }
            LaunchOutcome::Pending { creation_id: id } if creation_id(&id) => {
                task.dispatch.state = DispatchState::Pending;
                task.dispatch.creation_id = Some(id);
            }
            LaunchOutcome::NotSent { code: value } if code(&value) => {
                task.dispatch.state = DispatchState::NotSent;
                task.dispatch.code = Some(value);
            }
            LaunchOutcome::Uncertain { code: value } if code(&value) => {
                task.dispatch.state = DispatchState::Uncertain;
                task.dispatch.code = Some(value);
            }
            _ => {
                task.dispatch.state = DispatchState::Uncertain;
                task.dispatch.code = Some("invalid_reply".into());
            }
        }
        let handed = matches!(
            task.dispatch.state,
            DispatchState::Accepted | DispatchState::Pending
        );
        task.dispatch.finished = Some(at.clone());
        task.updated = at;
        self.save(next)?;
        self.last_handover = Some(handed);
        Ok(())
    }

    /// Ask, on the watch's own account, for a read of one task handed over
    /// whose look is due: accepted, nothing being read, nothing settled
    /// kept yet, and its last look (or its acceptance) longer ago than
    /// [`WATCH_EARLY_MS`] or [`WATCH_LATER_MS`] in this boot. A task
    /// accepted more than [`WATCH_AT_MOST_MS`] ago in this boot is left.
    /// Whether one was asked for. The read itself goes as any other.
    pub fn watch(&mut self, now: &Clock) -> Result<bool, Error> {
        if self.active() {
            return Ok(false);
        }
        let since = |audit: &Audit| {
            (audit.boot_id == now.boot_id)
                .then(|| now.monotonic_ms.saturating_sub(audit.monotonic_ms))
        };
        let due =
            self.state.tasks.iter().position(|task| {
                let Some(accepted) = task.dispatch.finished.as_ref() else {
                    return false;
                };
                let age = watched_age(task, now);
                let follow = age.is_none() && following(task, now);
                if age.is_none() && !follow {
                    return false;
                }
                if task.read.as_ref().is_some_and(|r| {
                    matches!(r.state, ReadState::Requested | ReadState::Attempting)
                }) {
                    return false;
                }
                let every = if task.watch_idle.is_some() {
                    WATCH_CONFIRM_MS
                } else if follow {
                    WATCH_LATER_MS
                } else if age.flatten().is_some_and(|age| age < WATCH_EARLY_FOR_MS) {
                    WATCH_EARLY_MS
                } else {
                    WATCH_LATER_MS
                };
                let last = task
                    .read
                    .as_ref()
                    .and_then(|r| r.finished.as_ref())
                    .unwrap_or(accepted);
                since(last).is_none_or(|ago| ago >= every)
            });
        let Some(index) = due else {
            return Ok(false);
        };
        self.watch_read(index, false, now)?;
        Ok(true)
    }

    /// Harness's own `wait` (`serve --push-wait`) says the agent of `task_id`
    /// ended: ask for the watch's read of it now, not at its next look, and
    /// keep an idle answer it finds at once. Only for a task the watch looks
    /// at with nothing being read, and only while no launch or other read is
    /// in hand; whether one was asked for, so the caller tries again later.
    pub fn push(&mut self, task_id: u64, now: &Clock) -> Result<bool, Error> {
        if self.active() {
            return Ok(false);
        }
        let Some(index) = self.state.tasks.iter().position(|task| {
            task.id == task_id
                && watched_age(task, now).is_some()
                && !task.read.as_ref().is_some_and(|r| {
                    matches!(r.state, ReadState::Requested | ReadState::Attempting)
                })
        }) else {
            return Ok(false);
        };
        self.watch_read(index, true, now)?;
        Ok(true)
    }

    /// The tasks whose latest read was asked for by a push ([`Engine::push`]),
    /// whatever it found: one still watched after it was not settled by it,
    /// and is left out of `wait` until the watch's own next read replaces
    /// that read, so an agent Harness calls done while `show` finds it at
    /// work costs one read a watch period, not one every few seconds (the
    /// reviewer on #1157). Reads nothing and changes nothing.
    pub fn pushed(&self) -> std::collections::BTreeSet<u64> {
        self.state
            .tasks
            .iter()
            .filter(|task| task.read.as_ref().is_some_and(|read| read.confirmed))
            .map(|task| task.id)
            .collect()
    }

    fn watch_read(&mut self, index: usize, confirmed: bool, now: &Clock) -> Result<(), Error> {
        self.clock(now)?;
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        let request_id = take(&mut next.next_read_id)?;
        // A receipt for the read this one replaces no longer describes the
        // store. A retry of that request finds the revision moved on and is
        // refused, so it is never run twice.
        let task_id = next.tasks[index].id;
        if next.last_receipt.as_ref().is_some_and(
            |receipt| matches!(&receipt.request, Request::Observe { id, .. } if *id == task_id),
        ) {
            next.last_receipt = None;
        }
        let task = &mut next.tasks[index];
        task.read = Some(Read {
            request_id,
            state: ReadState::Requested,
            requested: at.clone(),
            started: None,
            finished: None,
            observation_id: None,
            code: None,
            watch: true,
            confirmed,
        });
        task.updated = at;
        self.save(next)
    }

    /// Every task whose newest observation is settled (#1021): finished with
    /// an answer, stopped to ask, stopped or failed, kept less than
    /// `within_ms` ago in this boot. The watch lets such a task go at once;
    /// this is what is said of it after. Only while the watch's read is the
    /// task's latest and did not find the agent back at work: what the
    /// person asked to have read, or anything read with the watch off or
    /// after it let the task go for its age, has been heard already or is
    /// not news (Fourth on #1048). Timed from when the answer was first kept
    /// (`observed`), which a later look that finds it again does not move:
    /// a task stays listed while such a look is under way, and does not
    /// come back with a new time after it (Ten on #1145, with its follow
    /// looks). Reads nothing and changes nothing.
    pub fn settled_within(&self, now: &Clock, within_ms: u64) -> Vec<Settled> {
        self.state
            .tasks
            .iter()
            .filter_map(|task| {
                let read = task.read.as_ref().filter(|read| read.watch)?;
                if read.state == ReadState::Failed && read.code.as_deref() == Some("working") {
                    return None;
                }
                let newest = task.observations.last()?;
                let found = &newest.observed;
                if !settled(&newest.data) || found.boot_id != now.boot_id {
                    return None;
                }
                let ago_ms = now.monotonic_ms.checked_sub(found.monotonic_ms)?;
                if ago_ms >= within_ms {
                    return None;
                }
                Some(Settled {
                    task_id: task.id,
                    engine: serde_json::to_value(task.launch.target.engine)
                        .ok()?
                        .as_str()?
                        .to_owned(),
                    state: newest.data.state.clone(),
                    ago_ms,
                })
            })
            .collect()
    }

    /// Every task the watch looks at now ([`Watched`]), whether or not a
    /// look is due. Reads nothing and changes nothing.
    pub fn watched(&self, now: &Clock) -> Vec<Watched> {
        self.state
            .tasks
            .iter()
            .filter_map(|task| {
                let age = watched_age(task, now)?;
                Some(Watched {
                    task_id: task.id,
                    machine_id: task.launch.target.machine_id.clone(),
                    agent_id: task.dispatch.agent_id.clone()?,
                    // As the target file names it; the same words as `new`.
                    engine: serde_json::to_value(task.launch.target.engine)
                        .ok()?
                        .as_str()?
                        .to_owned(),
                    accepted_ago_ms: age,
                })
            })
            .collect()
    }

    /// The next harness this lamp started whose work is done and told
    /// ([`close_due`]), its close recorded before it is handed on. The
    /// returned job is permission for one `harness close`.
    pub fn begin_close(&mut self, now: &Clock) -> Result<Option<CloseJob>, Error> {
        self.tick(now)?;
        if self.active() {
            return Ok(None);
        }
        let Some(index) = self.state.tasks.iter().position(|t| close_due(t, now)) else {
            return Ok(None);
        };
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        let task = &mut next.tasks[index];
        let tries = task.close.as_ref().map_or(0, |close| close.tries) + 1;
        task.close = Some(Close {
            state: CloseState::Attempting,
            tries,
            attempted: at.clone(),
            finished: None,
            code: None,
        });
        task.updated = at;
        let told = &task.observations.last().expect("a told answer").data;
        let job = CloseJob {
            task_id: task.id,
            machine_id: task.launch.target.machine_id.clone(),
            agent_id: task.dispatch.agent_id.clone().expect("accepted task"),
            answer: told.answer.clone(),
            asked: told.ask.clone(),
        };
        self.save(next)?;
        Ok(Some(job))
    }

    pub fn finish_close(
        &mut self,
        job: &CloseJob,
        result: Result<Closed, String>,
        now: &Clock,
    ) -> Result<(), Error> {
        self.clock(now)?;
        let index = find(&self.state, job.task_id)?;
        if self.state.tasks[index]
            .close
            .as_ref()
            .is_none_or(|close| close.state != CloseState::Attempting)
            || self.state.tasks[index].dispatch.agent_id.as_deref() != Some(&job.agent_id)
        {
            return Err(err("stale_job", "The close is not the current attempt."));
        }
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        let task = &mut next.tasks[index];
        let close = task.close.as_mut().expect("current close");
        (close.state, close.code) = match result {
            Ok(Closed::Closed) => (CloseState::Closed, None),
            Ok(Closed::Gone) => (CloseState::Closed, Some("gone".into())),
            Ok(Closed::Refused) => (CloseState::Refused, None),
            Ok(Closed::Left) => (CloseState::Left, None),
            Err(code) => (CloseState::Failed, Some(code)),
        };
        close.finished = Some(at.clone());
        task.updated = at;
        self.save(next)
    }

    pub fn begin_observation(&mut self, now: &Clock) -> Result<Option<ObservationJob>, Error> {
        self.tick(now)?;
        if self.active() {
            return Ok(None);
        }
        let Some(index) = self.state.tasks.iter().position(|t| {
            t.read
                .as_ref()
                .is_some_and(|r| r.state == ReadState::Requested)
        }) else {
            return Ok(None);
        };
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        let task = &mut next.tasks[index];
        let read = task.read.as_mut().expect("selected read");
        read.state = ReadState::Attempting;
        read.started = Some(at.clone());
        task.updated = at;
        let job = ObservationJob {
            task_id: task.id,
            request_id: read.request_id,
            machine_id: task.launch.target.machine_id.clone(),
            agent_id: task.dispatch.agent_id.clone().expect("accepted task"),
        };
        self.save(next)?;
        Ok(Some(job))
    }

    pub fn finish_observation(
        &mut self,
        job: &ObservationJob,
        observation: Observation,
        now: &Clock,
    ) -> Result<(), Error> {
        self.clock(now)?;
        self.read_job(job)?;
        if !valid_observation(&observation) || observation.agent_id != job.agent_id {
            return self.observation_failed(job, "invalid_reply", now);
        }
        let index = find(&self.state, job.task_id)?;
        let observed_digest = digest(&observation);
        // The watch keeps only what the person would want to hear about:
        // an agent still at work is looked at again later, and an idle
        // answer only once a second look finds the same one.
        if self.state.tasks[index]
            .read
            .as_ref()
            .is_some_and(|read| read.watch)
        {
            let task = &self.state.tasks[index];
            let seen = task.watch_idle.as_ref();
            // An answer already kept needs no second look: it is only seen
            // again.
            let kept = task
                .observations
                .iter()
                .any(|o| o.digest == observed_digest);
            let confirmed = task.read.as_ref().is_some_and(|read| read.confirmed);
            let idle = observation.state == "idle" && settled(&observation) && !kept && !confirmed;
            if !settled(&observation) || (idle && seen != Some(&observed_digest)) {
                let watch_idle = idle.then(|| observed_digest.clone());
                let failure = if idle { "idle_once" } else { "working" };
                return self.watch_looked(job, watch_idle, failure, now);
            }
        }
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        let duplicate = next.tasks[index]
            .observations
            .iter()
            .position(|o| o.digest == observed_digest);
        let id = if let Some(oi) = duplicate {
            let observed = &mut next.tasks[index].observations[oi];
            observed.seen_count = observed.seen_count.checked_add(1).ok_or_else(exhausted)?;
            observed.last_seen = at.clone();
            observed.id
        } else {
            if next.tasks[index].observations.len() >= MAX_OBSERVATIONS {
                return self.observation_failed(job, "full", now);
            }
            let id = take(&mut next.next_observation_id)?;
            next.tasks[index].observations.push(Observed {
                id,
                digest: observed_digest,
                data: observation,
                observed: at.clone(),
                last_seen: at.clone(),
                seen_count: 1,
                speech: None,
            });
            id
        };
        let task = &mut next.tasks[index];
        task.watch_idle = None;
        let read = task.read.as_mut().expect("current read");
        read.state = ReadState::Finished;
        read.finished = Some(at.clone());
        read.observation_id = Some(id);
        task.updated = at;
        self.save(next)
    }

    pub fn observation_failed(
        &mut self,
        job: &ObservationJob,
        failure: &str,
        now: &Clock,
    ) -> Result<(), Error> {
        self.clock(now)?;
        self.read_job(job)?;
        let index = find(&self.state, job.task_id)?;
        let watch_idle = self.state.tasks[index].watch_idle.clone();
        self.watch_looked(job, watch_idle, failure, now)
    }

    /// A read that ends without an observation kept, with what the watch
    /// remembers of an idle answer seen once.
    fn watch_looked(
        &mut self,
        job: &ObservationJob,
        watch_idle: Option<String>,
        failure: &str,
        now: &Clock,
    ) -> Result<(), Error> {
        let index = find(&self.state, job.task_id)?;
        let mut next = self.state.clone();
        let at = next_audit(&mut next, now)?;
        let task = &mut next.tasks[index];
        task.watch_idle = watch_idle;
        let read = task.read.as_mut().expect("current read");
        read.state = ReadState::Failed;
        read.finished = Some(at.clone());
        read.code = Some(
            if code(failure) {
                failure
            } else {
                "invalid_reply"
            }
            .into(),
        );
        task.updated = at;
        self.save(next)
    }
    fn read_job(&self, job: &ObservationJob) -> Result<(), Error> {
        let task = &self.state.tasks[find(&self.state, job.task_id)?];
        if task.dispatch.agent_id.as_ref() != Some(&job.agent_id)
            || !task.read.as_ref().is_some_and(|r| {
                r.state == ReadState::Attempting
                    && r.request_id == job.request_id
                    && r.requested.session_id == self.session_id()
            })
        {
            return Err(err(
                "stale_job",
                "The observation is not the current committed read.",
            ));
        }
        Ok(())
    }
}

/// Whether the watch looks at `task`: accepted, with room for an
/// observation, nothing settled kept, and handed over at most
/// [`WATCH_AT_MOST_MS`] ago in this boot (or in an earlier one, when it
/// cannot be dated). How long ago, if in this boot.
fn watched_age(task: &Task, now: &Clock) -> Option<Option<u64>> {
    let accepted = task.dispatch.finished.as_ref()?;
    if task.dispatch.state != DispatchState::Accepted
        || task.observations.len() >= MAX_OBSERVATIONS
        || task.observations.iter().any(|o| settled(&o.data))
    {
        return None;
    }
    let age = (accepted.boot_id == now.boot_id)
        .then(|| now.monotonic_ms.saturating_sub(accepted.monotonic_ms));
    if age.is_some_and(|age| age > WATCH_AT_MOST_MS) {
        return None;
    }
    Some(age)
}

/// Whether the watch still looks at `task` after it kept an idle answer:
/// that answer is its newest observation, kept in this boot at most
/// [`WATCH_FOLLOW_MS`] ago, with room for another. A question, a stop or a
/// failure ends it. What progress reads ([`Engine::watched`]) is unchanged.
fn following(task: &Task, now: &Clock) -> bool {
    let Some(newest) = task.observations.last() else {
        return false;
    };
    task.dispatch.state == DispatchState::Accepted
        && task.observations.len() < MAX_OBSERVATIONS
        && newest.data.state == "idle"
        && settled(&newest.data)
        && newest.observed.boot_id == now.boot_id
        && now
            .monotonic_ms
            .saturating_sub(newest.observed.monotonic_ms)
            <= WATCH_FOLLOW_MS
}

fn take(counter: &mut u64) -> Result<u64, Error> {
    let id = *counter;
    *counter = counter.checked_add(1).ok_or_else(exhausted)?;
    Ok(id)
}
fn next_audit(state: &mut State, now: &Clock) -> Result<Audit, Error> {
    state.revision = state.revision.checked_add(1).ok_or_else(exhausted)?;
    let at = Audit {
        boot_id: now.boot_id.clone(),
        session_id: state.session_id.clone(),
        monotonic_ms: now.monotonic_ms,
        revision: state.revision,
    };
    state.saved_at = at.clone();
    Ok(at)
}
fn find(state: &State, id: u64) -> Result<usize, Error> {
    state
        .tasks
        .iter()
        .position(|t| t.id == id)
        .ok_or_else(|| err("not_found", "No retained task has that ID."))
}
fn find_observation(task: &Task, id: u64) -> Result<usize, Error> {
    task.observations
        .iter()
        .position(|o| o.id == id)
        .ok_or_else(|| err("not_found", "No observation of that task has that ID."))
}
fn identity(mut body: Value, store_id: &str, revision: u64) -> Value {
    let object = body.as_object_mut().expect("object response");
    object.insert("ok".into(), true.into());
    object.insert("store_id".into(), store_id.into());
    object.insert("revision".into(), revision.into());
    object.insert("completion".into(), "unverified".into());
    body
}
fn task_metadata(task: &Task) -> Value {
    json!({"id":task.id,"target_alias":task.target_alias,"prepare_request_id":task.prepare_request_id,"dispatch":task.dispatch,
        "created":task.created,"updated":task.updated,"observations":task.observations.len(),"read":task.read,"close":task.close,"completion":"unverified"})
}
fn observation_metadata(observed: &Observed) -> Value {
    json!({"id":observed.id,"digest":observed.digest,"state":observed.data.state,"observed":observed.observed,
        "last_seen":observed.last_seen,"seen_count":observed.seen_count,"truncated":observed.data.truncated,"tools":observed.data.tools,
        "speech":observed.speech.as_ref().map(|s|json!({"claim_id":s.claim_id,"text_digest":s.text_digest,"state":s.state,"claimed":s.claimed,"finished":s.finished})),"completion":"unverified"})
}

fn receipt_effect<'a>(state: &'a State, request: &Request) -> Result<&'a Audit, Error> {
    let task = match request {
        Request::Prepare { guard, .. } => state
            .tasks
            .iter()
            .find(|task| task.prepare_request_id == guard.request_id),
        Request::Confirm { id, .. }
        | Request::Cancel { id, .. }
        | Request::Observe { id, .. }
        | Request::ClaimSpeech { id, .. }
        | Request::AckSpeech { id, .. } => state.tasks.iter().find(|task| task.id == *id),
        _ => None,
    }
    .ok_or_else(storage)?;
    match request {
        Request::Prepare { .. } => Ok(&task.created),
        Request::Confirm { .. } => task.dispatch.confirmed.as_ref().ok_or_else(storage),
        Request::Cancel { .. } => task.dispatch.finished.as_ref().ok_or_else(storage),
        Request::Observe { .. } => Ok(&task.read.as_ref().ok_or_else(storage)?.requested),
        Request::ClaimSpeech { observation_id, .. } | Request::AckSpeech { observation_id, .. } => {
            let observed = &task.observations
                [find_observation(task, *observation_id).map_err(|_| storage())?];
            let speech = observed.speech.as_ref().ok_or_else(storage)?;
            if matches!(request, Request::ClaimSpeech { .. }) {
                Ok(&speech.claimed)
            } else {
                speech.finished.as_ref().ok_or_else(storage)
            }
        }
        _ => Err(storage()),
    }
}

fn receipt_body(state: &State, request: &Request) -> Result<Value, Error> {
    match request {
        Request::Prepare {
            guard,
            text,
            target,
        } => {
            let task = state
                .tasks
                .iter()
                .find(|t| t.prepare_request_id == guard.request_id)
                .ok_or_else(storage)?;
            if task.launch.prompt != *text || task.target_alias != *target {
                return Err(storage());
            }
            Ok(
                json!({"id":task.id,"prepared_digest":task.prepared_digest,"label":task.launch.name,"expires_monotonic_ms":task.expires_monotonic_ms}),
            )
        }
        Request::Confirm {
            id,
            prepared_digest,
            session_id,
            ..
        } => {
            let task = &state.tasks[find(state, *id).map_err(|_| storage())?];
            if task.prepared_digest != *prepared_digest
                || task
                    .dispatch
                    .confirmed
                    .as_ref()
                    .is_none_or(|a| a.session_id != *session_id)
            {
                return Err(storage());
            }
            Ok(json!({"id":id,"queued":true,"prepared_digest":prepared_digest}))
        }
        Request::Cancel { id, .. } => {
            if state.tasks[find(state, *id).map_err(|_| storage())?]
                .dispatch
                .state
                != DispatchState::Cancelled
            {
                return Err(storage());
            }
            Ok(json!({"id":id,"cancelled":true,"remote_cancelled":false}))
        }
        Request::Observe { id, .. } => {
            let task = &state.tasks[find(state, *id).map_err(|_| storage())?];
            let read = task.read.as_ref().ok_or_else(storage)?;
            Ok(json!({"id":id,"observation_request":read.request_id}))
        }
        Request::ClaimSpeech {
            id,
            observation_id,
            text,
            session_id,
            ..
        } => {
            let task = &state.tasks[find(state, *id).map_err(|_| storage())?];
            let observed = &task.observations
                [find_observation(task, *observation_id).map_err(|_| storage())?];
            let speech = observed.speech.as_ref().ok_or_else(storage)?;
            if speech.text != *text || speech.claimed.session_id != *session_id {
                return Err(storage());
            }
            Ok(
                json!({"id":id,"observation_id":observation_id,"claim_id":speech.claim_id,"text":text,"text_digest":speech.text_digest,"session_id":session_id}),
            )
        }
        Request::AckSpeech {
            id,
            observation_id,
            claim_id,
            text_digest,
            session_id,
            played,
            ..
        } => {
            let task = &state.tasks[find(state, *id).map_err(|_| storage())?];
            let observed = &task.observations
                [find_observation(task, *observation_id).map_err(|_| storage())?];
            let speech = observed.speech.as_ref().ok_or_else(storage)?;
            if !played
                || speech.state != SpeechState::Spoken
                || speech.claim_id != *claim_id
                || speech.text_digest != *text_digest
                || speech.claimed.session_id != *session_id
            {
                return Err(storage());
            }
            Ok(
                json!({"id":id,"observation_id":observation_id,"claim_id":claim_id,"text_digest":text_digest,"reported_spoken":true}),
            )
        }
        _ => Err(storage()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{harness::EngineKind, protocol::Guard};
    use std::{cell::RefCell, rc::Rc};

    const STORE: &str = "11111111111111111111111111111111";
    const SESSION: &str = "22222222222222222222222222222222";
    const RESTART: &str = "33333333333333333333333333333333";
    const AGENT: &str = "11111111-1111-4111-8111-111111111111";

    #[derive(Default)]
    struct MemoryState {
        bytes: Option<Vec<u8>>,
        writes: usize,
        fail_before: bool,
        fail_after: bool,
    }
    #[derive(Clone, Default)]
    struct Memory(Rc<RefCell<MemoryState>>);
    impl Persistence for Memory {
        fn load(&self) -> io::Result<Option<Vec<u8>>> {
            Ok(self.0.borrow().bytes.clone())
        }
        fn commit(&mut self, bytes: &[u8]) -> io::Result<()> {
            let mut state = self.0.borrow_mut();
            if state.fail_before {
                return Err(io::Error::other("injected before commit"));
            }
            state.bytes = Some(bytes.to_vec());
            state.writes += 1;
            if state.fail_after {
                return Err(io::Error::other("injected after commit"));
            }
            Ok(())
        }
    }
    fn clock(monotonic_ms: u64) -> Clock {
        Clock {
            boot_id: "test-boot".into(),
            monotonic_ms,
        }
    }
    fn targets() -> BTreeMap<String, Target> {
        BTreeMap::from([(
            "desk".into(),
            Target {
                machine_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                project: "review-project".into(),
                engine: EngineKind::Codex,
            },
        )])
    }
    fn opened() -> (Engine<Memory>, Memory) {
        let memory = Memory::default();
        let mut engine =
            Engine::open(memory.clone(), targets(), STORE, SESSION, &clock(0)).unwrap();
        engine.close_told(true);
        (engine, memory)
    }
    fn guard(engine: &Engine<Memory>, prefix: &str) -> Guard {
        Guard {
            store_id: engine.store_id().into(),
            revision: engine.revision(),
            request_id: format!("{prefix}-{}", engine.revision()),
        }
    }
    fn preparation(engine: &Engine<Memory>, text: &str) -> Request {
        Request::Prepare {
            guard: guard(engine, "prepare"),
            text: text.into(),
            target: "desk".into(),
        }
    }
    fn prepare(engine: &mut Engine<Memory>, at: u64) -> u64 {
        let request = preparation(engine, "Review this project without changing files.");
        engine.handle(&request, &clock(at), false).unwrap()["id"]
            .as_u64()
            .unwrap()
    }
    fn confirmation(engine: &Engine<Memory>, id: u64) -> Request {
        Request::Confirm {
            guard: guard(engine, "confirm"),
            id,
            prepared_digest: engine.state.tasks[find(&engine.state, id).unwrap()]
                .prepared_digest
                .clone(),
            session_id: engine.session_id().into(),
        }
    }
    fn queued(engine: &mut Engine<Memory>, at: u64) -> u64 {
        let id = prepare(engine, at);
        engine
            .handle(&confirmation(engine, id), &clock(at), true)
            .unwrap();
        id
    }
    fn accepted(engine: &mut Engine<Memory>, at: u64) -> u64 {
        let id = queued(engine, at);
        assert_eq!(
            engine.begin_launch(&clock(at)).unwrap().unwrap().task_id,
            id
        );
        engine
            .finish_launch(
                id,
                LaunchOutcome::Created {
                    agent_id: AGENT.into(),
                },
                &clock(at),
            )
            .unwrap();
        id
    }
    fn data(answer: &str) -> Observation {
        Observation {
            agent_id: AGENT.into(),
            state: "idle".into(),
            ask: None,
            answer: answer.into(),
            truncated: false,
            tools: 2,
        }
    }
    fn request_read(engine: &mut Engine<Memory>, id: u64, at: u64) -> ObservationJob {
        let request = Request::Observe {
            guard: guard(engine, "observe"),
            id,
        };
        engine.handle(&request, &clock(at), false).unwrap();
        engine.begin_observation(&clock(at)).unwrap().unwrap()
    }
    fn observe(engine: &mut Engine<Memory>, id: u64, answer: &str, at: u64) -> u64 {
        let job = request_read(engine, id, at);
        engine
            .finish_observation(&job, data(answer), &clock(at))
            .unwrap();
        engine.state.tasks[find(&engine.state, id).unwrap()]
            .read
            .as_ref()
            .unwrap()
            .observation_id
            .unwrap()
    }
    fn claim(engine: &Engine<Memory>, id: u64, observation_id: u64) -> Request {
        Request::ClaimSpeech {
            guard: guard(engine, "claim"),
            id,
            observation_id,
            text: "The agent reports that its review is available.".into(),
            session_id: engine.session_id().into(),
        }
    }
    fn acknowledgement(engine: &Engine<Memory>, id: u64, observation_id: u64) -> Request {
        let task = &engine.state.tasks[find(&engine.state, id).unwrap()];
        let speech = task.observations[find_observation(task, observation_id).unwrap()]
            .speech
            .as_ref()
            .unwrap();
        Request::AckSpeech {
            guard: guard(engine, "ack"),
            id,
            observation_id,
            claim_id: speech.claim_id,
            text_digest: speech.text_digest.clone(),
            session_id: engine.session_id().into(),
            played: true,
        }
    }
    fn dispatch(engine: &Engine<Memory>, id: u64) -> DispatchState {
        engine.state.tasks[find(&engine.state, id).unwrap()]
            .dispatch
            .state
    }
    fn stored(memory: &Memory) -> State {
        serde_json::from_slice(memory.0.borrow().bytes.as_ref().unwrap()).unwrap()
    }

    #[test]
    fn preparation_is_immutable_and_exact_retry_is_a_receipt_not_another_task() {
        let (mut engine, memory) = opened();
        let request = preparation(&engine, "Check the parser.");
        let response = engine.handle(&request, &clock(10), false).unwrap();
        assert_eq!(
            engine.handle(&request, &clock(20), false).unwrap(),
            response
        );
        assert_eq!(engine.state.tasks.len(), 1);
        assert_eq!(memory.0.borrow().writes, 2);
        let original = engine.state.tasks[0].launch.clone();
        engine.targets.get_mut("desk").unwrap().project = "different-project".into();
        engine
            .handle(&confirmation(&engine, 1), &clock(30), true)
            .unwrap();
        assert_eq!(
            engine.begin_launch(&clock(40)).unwrap().unwrap().launch,
            original
        );
        assert_eq!(response["completion"], "unverified");
    }

    #[test]
    fn guards_refuse_changed_retry_old_revision_and_wrong_store() {
        let (mut engine, _) = opened();
        let request = preparation(&engine, "Original");
        engine.handle(&request, &clock(0), false).unwrap();
        let mut changed = request.clone();
        if let Request::Prepare { text, .. } = &mut changed {
            *text = "Changed".into();
        }
        assert_eq!(
            engine.handle(&changed, &clock(0), false).unwrap_err().code,
            "request_conflict"
        );
        let mut stale = preparation(&engine, "Another");
        if let Request::Prepare { guard, .. } = &mut stale {
            guard.revision = 0;
        }
        assert_eq!(
            engine.handle(&stale, &clock(0), false).unwrap_err().code,
            "conflict"
        );
        if let Request::Prepare { guard, .. } = &mut stale {
            guard.store_id = RESTART.into();
        }
        assert_eq!(
            engine.handle(&stale, &clock(0), false).unwrap_err().code,
            "bad_guard"
        );
        assert_eq!(engine.state.tasks.len(), 1);
    }

    #[test]
    fn trusted_confirmation_binds_digest_session_and_exact_expiry() {
        let (mut engine, _) = opened();
        let id = prepare(&mut engine, 100);
        let original = confirmation(&engine, id);
        assert_eq!(
            engine
                .handle(&original, &clock(101), false)
                .unwrap_err()
                .code,
            "untrusted"
        );
        let mut altered = original.clone();
        if let Request::Confirm {
            prepared_digest, ..
        } = &mut altered
        {
            *prepared_digest = "0".repeat(64);
        }
        assert_eq!(
            engine.handle(&altered, &clock(101), true).unwrap_err().code,
            "stale_confirmation"
        );
        if let Request::Confirm { session_id, .. } = &mut altered {
            *session_id = RESTART.into();
        }
        assert_eq!(
            engine.handle(&altered, &clock(101), true).unwrap_err().code,
            "stale_session"
        );
        engine
            .handle(&original, &clock(100 + CONFIRM_MS - 1), true)
            .unwrap();
        assert!(
            engine
                .begin_launch(&clock(100 + CONFIRM_MS))
                .unwrap()
                .is_none()
        );
        assert_eq!(dispatch(&engine, id), DispatchState::Invalidated);
        assert_eq!(
            engine.state.tasks[0].dispatch.code.as_deref(),
            Some("expired")
        );
    }

    #[test]
    fn automatic_expiry_preserves_last_mutation_receipt_but_never_requeues_it() {
        let (mut engine, _) = opened();
        let id = prepare(&mut engine, 0);
        let request = confirmation(&engine, id);
        let receipt = engine.handle(&request, &clock(0), true).unwrap();
        engine.tick(&clock(CONFIRM_MS)).unwrap();
        assert_eq!(
            engine.handle(&request, &clock(CONFIRM_MS), true).unwrap(),
            receipt
        );
        assert_eq!(dispatch(&engine, id), DispatchState::Invalidated);
        assert!(engine.begin_launch(&clock(CONFIRM_MS)).unwrap().is_none());
    }

    #[test]
    fn attempt_is_durable_before_job_and_is_never_issued_twice() {
        let (mut engine, memory) = opened();
        let id = queued(&mut engine, 0);
        let job = engine.begin_launch(&clock(1)).unwrap().unwrap();
        assert_eq!(job.task_id, id);
        assert_eq!(
            stored(&memory).tasks[0].dispatch.state,
            DispatchState::Attempting
        );
        assert!(engine.begin_launch(&clock(2)).unwrap().is_none());
        let request = Request::Cancel {
            guard: guard(&engine, "cancel"),
            id,
        };
        assert_eq!(
            engine.handle(&request, &clock(2), false).unwrap_err().code,
            "already_attempted"
        );
    }

    #[test]
    fn failed_attempt_commit_releases_no_job_and_latches_failure() {
        for after in [false, true] {
            let (mut engine, memory) = opened();
            queued(&mut engine, 0);
            if after {
                memory.0.borrow_mut().fail_after = true;
            } else {
                memory.0.borrow_mut().fail_before = true;
            }
            assert_eq!(engine.begin_launch(&clock(1)).unwrap_err().code, "storage");
            assert!(engine.failed());
            assert_eq!(engine.begin_launch(&clock(2)).unwrap_err().code, "storage");
            memory.0.borrow_mut().fail_after = false;
            memory.0.borrow_mut().fail_before = false;
            let mut recovered = Engine::open(memory, targets(), STORE, RESTART, &clock(3)).unwrap();
            assert_eq!(
                dispatch(&recovered, 1),
                if after {
                    DispatchState::Uncertain
                } else {
                    DispatchState::Invalidated
                }
            );
            assert!(recovered.begin_launch(&clock(3)).unwrap().is_none());
        }
    }

    #[test]
    fn restart_invalidates_unattempted_work_and_marks_inflight_uncertain() {
        for phase in [
            DispatchState::Prepared,
            DispatchState::Queued,
            DispatchState::Attempting,
        ] {
            let (mut engine, memory) = opened();
            let id = prepare(&mut engine, 0);
            let request = confirmation(&engine, id);
            if phase != DispatchState::Prepared {
                engine.handle(&request, &clock(1), true).unwrap();
            }
            if phase == DispatchState::Attempting {
                engine.begin_launch(&clock(2)).unwrap();
            }
            let mut recovered = Engine::open(memory, targets(), STORE, RESTART, &clock(3)).unwrap();
            assert_eq!(
                dispatch(&recovered, id),
                if phase == DispatchState::Attempting {
                    DispatchState::Uncertain
                } else {
                    DispatchState::Invalidated
                }
            );
            assert_eq!(
                recovered
                    .handle(&request, &clock(3), true)
                    .unwrap_err()
                    .code,
                "stale_session"
            );
            assert!(recovered.begin_launch(&clock(3)).unwrap().is_none());
            assert_eq!(
                recovered
                    .finish_launch(
                        id,
                        LaunchOutcome::Created {
                            agent_id: AGENT.into()
                        },
                        &clock(3)
                    )
                    .unwrap_err()
                    .code,
                "stale_job"
            );
        }
    }

    #[test]
    fn final_dispatch_outcomes_never_automatically_resend() {
        for outcome in [
            LaunchOutcome::Created {
                agent_id: AGENT.into(),
            },
            LaunchOutcome::Pending {
                creation_id: "creation-key-00000001".into(),
            },
            LaunchOutcome::NotSent {
                code: "policy_refused".into(),
            },
            LaunchOutcome::Uncertain {
                code: "lost_reply".into(),
            },
        ] {
            let (mut engine, memory) = opened();
            let id = queued(&mut engine, 0);
            engine.begin_launch(&clock(0)).unwrap();
            engine.finish_launch(id, outcome, &clock(1)).unwrap();
            assert!(engine.begin_launch(&clock(2)).unwrap().is_none());
            let mut recovered = Engine::open(memory, targets(), STORE, RESTART, &clock(3)).unwrap();
            assert!(recovered.begin_launch(&clock(3)).unwrap().is_none());
            assert_eq!(recovered.summary()["completion"], "unverified");
        }
    }

    #[test]
    fn the_summary_says_whether_the_newest_hand_over_reached_harness() {
        for (outcome, said) in [
            (
                LaunchOutcome::Created {
                    agent_id: AGENT.into(),
                },
                "handed",
            ),
            (
                LaunchOutcome::Pending {
                    creation_id: "creation-key-00000001".into(),
                },
                "handed",
            ),
            (
                LaunchOutcome::NotSent {
                    code: "policy_refused".into(),
                },
                "failed",
            ),
            (
                LaunchOutcome::Uncertain {
                    code: "new_failed".into(),
                },
                "failed",
            ),
        ] {
            let (mut engine, memory) = opened();
            assert!(engine.summary()["last_handover"].is_null());
            let id = queued(&mut engine, 0);
            engine.begin_launch(&clock(0)).unwrap();
            engine.finish_launch(id, outcome, &clock(1)).unwrap();
            assert_eq!(engine.summary()["last_handover"], said);
            // A new run has not handed anything over yet.
            let recovered = Engine::open(memory, targets(), STORE, RESTART, &clock(3)).unwrap();
            assert!(recovered.summary()["last_handover"].is_null());
        }
    }

    #[test]
    fn malformed_launch_reply_is_uncertain_without_leaking_private_text() {
        let (mut engine, _) = opened();
        let id = queued(&mut engine, 0);
        engine.begin_launch(&clock(0)).unwrap();
        engine
            .finish_launch(
                id,
                LaunchOutcome::NotSent {
                    code: "secret error / account".into(),
                },
                &clock(0),
            )
            .unwrap();
        assert_eq!(dispatch(&engine, id), DispatchState::Uncertain);
        assert_eq!(
            engine.state.tasks[0].dispatch.code.as_deref(),
            Some("invalid_reply")
        );
        assert!(!engine.summary().to_string().contains("secret"));
    }

    #[test]
    fn cancellation_is_local_and_only_before_the_attempt() {
        for confirm in [false, true] {
            let (mut engine, _) = opened();
            let id = if confirm {
                queued(&mut engine, 0)
            } else {
                prepare(&mut engine, 0)
            };
            let request = Request::Cancel {
                guard: guard(&engine, "cancel"),
                id,
            };
            let reply = engine.handle(&request, &clock(1), false).unwrap();
            assert_eq!(reply["remote_cancelled"], false);
            assert_eq!(engine.handle(&request, &clock(2), false).unwrap(), reply);
            assert!(engine.begin_launch(&clock(2)).unwrap().is_none());
        }
    }

    #[test]
    fn pending_creation_is_unresolved_and_cannot_be_observed_by_name() {
        let (mut engine, _) = opened();
        let id = queued(&mut engine, 0);
        engine.begin_launch(&clock(0)).unwrap();
        engine
            .finish_launch(
                id,
                LaunchOutcome::Pending {
                    creation_id: "creation-key-00000001".into(),
                },
                &clock(0),
            )
            .unwrap();
        let request = Request::Observe {
            guard: guard(&engine, "observe"),
            id,
        };
        assert_eq!(
            engine.handle(&request, &clock(1), false).unwrap_err().code,
            "unresolved"
        );
        assert!(engine.begin_observation(&clock(1)).unwrap().is_none());
    }

    #[test]
    fn observations_are_explicit_exact_identity_and_current_job_only() {
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 0);
        assert!(engine.begin_observation(&clock(1)).unwrap().is_none());
        let job = request_read(&mut engine, id, 2);
        assert_eq!(job.agent_id, AGENT);
        assert_eq!(
            stored(&memory).tasks[0].read.as_ref().unwrap().state,
            ReadState::Attempting
        );
        let mut other = job.clone();
        other.request_id += 1;
        assert_eq!(
            engine
                .finish_observation(&other, data("wrong job"), &clock(3))
                .unwrap_err()
                .code,
            "stale_job"
        );
        let mut wrong = data("wrong agent");
        wrong.agent_id = "22222222-2222-4222-8222-222222222222".into();
        engine.finish_observation(&job, wrong, &clock(3)).unwrap();
        assert_eq!(
            engine.state.tasks[0].read.as_ref().unwrap().code.as_deref(),
            Some("invalid_reply")
        );
        assert!(engine.state.tasks[0].observations.is_empty());
        assert_eq!(
            engine
                .finish_observation(&job, data("late"), &clock(4))
                .unwrap_err()
                .code,
            "stale_job"
        );
    }

    /// A task read in `state`, as Harness's `show` gives it (its `ask` is
    /// the task's own text), and its telling played: (`at`, `at + 2`).
    fn told(engine: &mut Engine<Memory>, state: &str, at: u64) -> u64 {
        let id = accepted(engine, 0);
        let job = request_read(engine, id, at);
        let mut answer = data("A poem about coffee, in poem.txt.");
        answer.state = state.into();
        answer.ask = Some("Write a two-line poem about coffee.".into());
        engine.finish_observation(&job, answer, &clock(at)).unwrap();
        let observation = engine.state.tasks[0]
            .read
            .as_ref()
            .unwrap()
            .observation_id
            .unwrap();
        engine
            .handle(&claim(engine, id, observation), &clock(at + 1), true)
            .unwrap();
        engine
            .handle(
                &acknowledgement(engine, id, observation),
                &clock(at + 2),
                true,
            )
            .unwrap();
        id
    }

    /// #1390: the harness this lamp started is closed once its answer is told,
    /// by its own agent id, carrying the told turn for the read just before;
    /// nothing before the telling played, and never again after.
    #[test]
    fn a_harness_done_and_told_is_closed_once() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 0);
        let read = request_read(&mut engine, id, 10);
        engine
            .finish_observation(&read, data("Done."), &clock(10))
            .unwrap();
        assert!(
            engine.begin_close(&clock(11)).unwrap().is_none(),
            "not told yet"
        );

        let (mut engine, memory) = opened();
        let id = told(&mut engine, "idle", 10);
        let job = engine.begin_close(&clock(13)).unwrap().unwrap();
        assert_eq!((job.task_id, job.agent_id.as_str()), (id, AGENT));
        assert_eq!(
            job.machine_id,
            engine.state.tasks[0].launch.target.machine_id
        );
        assert_eq!(
            (job.answer.as_str(), job.asked.as_deref()),
            (
                "A poem about coffee, in poem.txt.",
                Some("Write a two-line poem about coffee.")
            )
        );
        // Recorded before it is handed on, and one at a time.
        assert_eq!(
            stored(&memory).tasks[0].close.as_ref().unwrap().state,
            CloseState::Attempting
        );
        assert!(engine.begin_close(&clock(14)).unwrap().is_none());
        engine
            .finish_close(&job, Ok(Closed::Closed), &clock(15))
            .unwrap();
        assert!(engine.begin_close(&clock(10_000_000)).unwrap().is_none());
        assert_eq!(
            engine
                .finish_close(&job, Ok(Closed::Closed), &clock(10_000_001))
                .unwrap_err()
                .code,
            "stale_job"
        );
    }

    /// One that waits for the person is left to them, however long; one the
    /// person went on with (the read before the close found another turn) is
    /// never asked again; and a task from a store before this version is
    /// never closed at all.
    #[test]
    fn a_harness_the_person_has_is_left_to_them() {
        let (mut engine, _) = opened();
        told(&mut engine, "needs-input", 10);
        assert!(engine.begin_close(&clock(100_000_000)).unwrap().is_none());

        let (mut engine, _) = opened();
        told(&mut engine, "idle", 10);
        let job = engine.begin_close(&clock(13)).unwrap().unwrap();
        engine
            .finish_close(&job, Ok(Closed::Left), &clock(14))
            .unwrap();
        assert!(engine.begin_close(&clock(100_000_000)).unwrap().is_none());

        let (mut engine, _) = opened();
        told(&mut engine, "idle", 10);
        engine.state.tasks[0].closable = false;
        assert!(engine.begin_close(&clock(13)).unwrap().is_none());
        // Such a task loads from a store written before the field.
        let mut task = serde_json::to_value(&engine.state.tasks[0]).unwrap();
        task.as_object_mut().unwrap().remove("closable");
        let loaded: Task = serde_json::from_value(task).unwrap();
        assert!(!loaded.closable);
    }

    /// The reviewer on #1395, 22:06: the agent answered, and before the
    /// lamp told it the person asked it more at the computer ("Now make it
    /// longer."). That newer turn, kept and told, is the person's: never
    /// closed. A task made while `--close-told` was off is never closed
    /// either, though the switch is on later.
    #[test]
    fn a_turn_the_person_asked_or_a_task_from_before_the_switch_is_never_closed() {
        let (mut engine, _) = opened();
        let id = told(&mut engine, "idle", 10);
        let job = request_read(&mut engine, id, 20);
        let mut theirs = data("A longer poem about coffee.");
        theirs.ask = Some("Now make it longer.".into());
        engine.finish_observation(&job, theirs, &clock(20)).unwrap();
        let newest = engine.state.tasks[0]
            .read
            .as_ref()
            .unwrap()
            .observation_id
            .unwrap();
        engine
            .handle(&claim(&engine, id, newest), &clock(21), true)
            .unwrap();
        engine
            .handle(&acknowledgement(&engine, id, newest), &clock(22), true)
            .unwrap();
        assert!(engine.begin_close(&clock(100_000_000)).unwrap().is_none());

        let (mut engine, _) = opened();
        engine.close_told(false);
        told(&mut engine, "idle", 10);
        engine.close_told(true);
        assert!(!engine.state.tasks[0].closable);
        assert!(engine.begin_close(&clock(13)).unwrap().is_none());
    }

    /// A close that failed is tried again a minute later, three times in
    /// all; one Harness refused never again.
    #[test]
    fn a_failed_close_is_tried_three_times_and_a_refused_one_never() {
        let (mut engine, _) = opened();
        told(&mut engine, "idle", 10);
        let mut at = 13;
        for _ in 0..CLOSE_TRIES {
            let job = engine.begin_close(&clock(at)).unwrap().unwrap();
            engine
                .finish_close(&job, Err("close_failed".into()), &clock(at))
                .unwrap();
            assert!(
                engine
                    .begin_close(&clock(at + CLOSE_RETRY_MS - 1))
                    .unwrap()
                    .is_none()
            );
            at += CLOSE_RETRY_MS;
        }
        assert!(engine.begin_close(&clock(at * 10)).unwrap().is_none());

        let (mut engine, _) = opened();
        told(&mut engine, "idle", 10);
        let job = engine.begin_close(&clock(13)).unwrap().unwrap();
        engine
            .finish_close(&job, Ok(Closed::Refused), &clock(14))
            .unwrap();
        assert!(engine.begin_close(&clock(10_000_000)).unwrap().is_none());
    }

    /// A restart while a close was under way leaves it not known, and so
    /// tried again: the try reads the agent first.
    #[test]
    fn a_close_under_way_at_a_restart_is_tried_again() {
        let (mut engine, memory) = opened();
        told(&mut engine, "idle", 10);
        engine.begin_close(&clock(13)).unwrap().unwrap();
        let mut recovered = Engine::open(memory, targets(), STORE, RESTART, &clock(20)).unwrap();
        let close = recovered.state.tasks[0].close.as_ref().unwrap();
        assert_eq!(
            (close.state, close.code.as_deref()),
            (CloseState::Failed, Some("restart"))
        );
        assert!(
            recovered
                .begin_close(&clock(20 + CLOSE_RETRY_MS))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn duplicate_observation_preserves_identity_first_age_and_speech_state() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 0);
        let first = observe(&mut engine, id, "Review available", 10);
        engine
            .handle(&claim(&engine, id, first), &clock(11), true)
            .unwrap();
        engine
            .handle(&acknowledgement(&engine, id, first), &clock(12), true)
            .unwrap();
        let second = observe(&mut engine, id, "Review available", 20);
        assert_eq!(first, second);
        let observed = &engine.state.tasks[0].observations[0];
        assert_eq!(observed.observed.monotonic_ms, 10);
        assert_eq!(observed.last_seen.monotonic_ms, 20);
        assert_eq!(observed.seen_count, 2);
        assert_eq!(observed.speech.as_ref().unwrap().state, SpeechState::Spoken);
        assert_eq!(engine.summary()["observations"], 1);
        assert_eq!(engine.summary()["completion"], "unverified");
    }

    #[test]
    fn duplicate_observation_after_restart_does_not_rejuvenate_speech_age() {
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 0);
        let observation = observe(&mut engine, id, "same result", 1);
        let mut recovered = Engine::open(memory, targets(), STORE, RESTART, &clock(2)).unwrap();
        assert_eq!(observe(&mut recovered, id, "same result", 3), observation);
        assert_eq!(
            recovered
                .handle(&claim(&recovered, id, observation), &clock(4), true)
                .unwrap_err()
                .code,
            "stale_observation"
        );
        let new = observe(&mut recovered, id, "changed result", 5);
        recovered
            .handle(&claim(&recovered, id, new), &clock(6), true)
            .unwrap();
    }

    #[test]
    fn restart_cancels_requested_and_inflight_reads_without_automatic_poll() {
        for begin in [false, true] {
            let (mut engine, memory) = opened();
            let id = accepted(&mut engine, 0);
            let request = Request::Observe {
                guard: guard(&engine, "observe"),
                id,
            };
            engine.handle(&request, &clock(1), false).unwrap();
            if begin {
                engine.begin_observation(&clock(2)).unwrap();
            }
            let mut recovered = Engine::open(memory, targets(), STORE, RESTART, &clock(3)).unwrap();
            assert_eq!(
                recovered.state.tasks[0].read.as_ref().unwrap().state,
                ReadState::Failed
            );
            assert_eq!(
                recovered.state.tasks[0]
                    .read
                    .as_ref()
                    .unwrap()
                    .code
                    .as_deref(),
                Some("restart")
            );
            assert!(recovered.begin_observation(&clock(4)).unwrap().is_none());
            // The old receipt describes the old request; it cannot queue a new read.
            recovered.handle(&request, &clock(4), false).unwrap();
            assert!(recovered.begin_observation(&clock(4)).unwrap().is_none());
        }
    }

    #[test]
    fn jobs_are_serial_and_queued_launches_expire_while_a_read_is_inflight() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 0);
        let job = request_read(&mut engine, id, 1);
        let queued_id = queued(&mut engine, 2);
        assert!(engine.begin_launch(&clock(3)).unwrap().is_none());
        assert!(engine.begin_observation(&clock(3)).unwrap().is_none());
        engine
            .finish_observation(&job, data("after slow read"), &clock(2 + CONFIRM_MS))
            .unwrap();
        assert!(
            engine
                .begin_launch(&clock(2 + CONFIRM_MS))
                .unwrap()
                .is_none()
        );
        assert_eq!(dispatch(&engine, queued_id), DispatchState::Invalidated);
    }

    #[test]
    fn speech_requires_trusted_route_and_recent_original_observation() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 0);
        let observation = observe(&mut engine, id, "result", 10);
        let request = claim(&engine, id, observation);
        assert_eq!(
            engine.handle(&request, &clock(11), false).unwrap_err().code,
            "untrusted"
        );
        observe(&mut engine, id, "result", 10 + SPEECH_AGE_MS - 1);
        let request = claim(&engine, id, observation);
        assert_eq!(
            engine
                .handle(&request, &clock(10 + SPEECH_AGE_MS), true)
                .unwrap_err()
                .code,
            "stale_observation"
        );
        assert!(engine.state.tasks[0].observations[0].speech.is_none());
    }

    #[test]
    fn lost_speech_claim_reply_and_voice_only_restart_never_reissue_playback_grant() {
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 0);
        let observation = observe(&mut engine, id, "result", 0);
        let request = claim(&engine, id, observation);
        let grant = engine.handle(&request, &clock(1), true).unwrap();
        assert_eq!(
            grant["text"],
            "The agent reports that its review is available."
        );
        assert_eq!(
            stored(&memory).tasks[0].observations[0]
                .speech
                .as_ref()
                .unwrap()
                .state,
            SpeechState::Claimed
        );
        // The Voice caller may have played the first grant before losing its state.
        assert_eq!(
            engine.handle(&request, &clock(2), true).unwrap_err().code,
            "claim_uncertain"
        );
        let fresh_request = claim(&engine, id, observation);
        assert_eq!(
            engine
                .handle(&fresh_request, &clock(2), true)
                .unwrap_err()
                .code,
            "already_claimed"
        );
        let status = engine
            .handle(&Request::Status {}, &clock(2), false)
            .unwrap();
        assert!(status["last_receipt"]["response"].get("text").is_none());
    }

    #[test]
    fn uncertain_claim_commit_never_grants_and_recovers_as_uncertain() {
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 0);
        let observation = observe(&mut engine, id, "result", 0);
        memory.0.borrow_mut().fail_after = true;
        assert_eq!(
            engine
                .handle(&claim(&engine, id, observation), &clock(1), true)
                .unwrap_err()
                .code,
            "storage"
        );
        assert!(engine.failed());
        memory.0.borrow_mut().fail_after = false;
        let mut recovered = Engine::open(memory, targets(), STORE, RESTART, &clock(2)).unwrap();
        assert_eq!(recovered.summary()["speech_uncertain"], 1);
        assert_eq!(
            recovered
                .handle(&claim(&recovered, id, observation), &clock(2), true)
                .unwrap_err()
                .code,
            "already_claimed"
        );
    }

    #[test]
    fn qualified_playout_receipt_requires_exact_tuple_digest_and_completed_flag() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 0);
        let observation = observe(&mut engine, id, "result", 0);
        engine
            .handle(&claim(&engine, id, observation), &clock(1), true)
            .unwrap();
        let original = acknowledgement(&engine, id, observation);
        assert_eq!(
            engine.handle(&original, &clock(2), false).unwrap_err().code,
            "untrusted"
        );
        for variant in 0..3 {
            let mut changed = original.clone();
            if let Request::AckSpeech {
                claim_id,
                text_digest,
                played,
                ..
            } = &mut changed
            {
                match variant {
                    0 => *claim_id += 1,
                    1 => *text_digest = "0".repeat(64),
                    _ => *played = false,
                }
            }
            assert_eq!(
                engine.handle(&changed, &clock(2), true).unwrap_err().code,
                "stale_claim"
            );
        }
        let reply = engine.handle(&original, &clock(2), true).unwrap();
        assert_eq!(reply["reported_spoken"], true);
        assert_eq!(reply["completion"], "unverified");
        assert_eq!(engine.handle(&original, &clock(3), true).unwrap(), reply);
        assert_eq!(engine.summary()["speech_spoken"], 1);
    }

    #[test]
    fn tasks_restart_never_accepts_old_claim_or_playout_acknowledgement() {
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 0);
        let observation = observe(&mut engine, id, "result", 0);
        let request = claim(&engine, id, observation);
        engine.handle(&request, &clock(1), true).unwrap();
        let ack = acknowledgement(&engine, id, observation);
        let mut recovered = Engine::open(memory, targets(), STORE, RESTART, &clock(2)).unwrap();
        assert_eq!(
            recovered
                .handle(&request, &clock(2), true)
                .unwrap_err()
                .code,
            "stale_session"
        );
        assert_eq!(
            recovered.handle(&ack, &clock(2), true).unwrap_err().code,
            "stale_session"
        );
        let fresh_ack = acknowledgement(&recovered, id, observation);
        assert_eq!(
            recovered
                .handle(&fresh_ack, &clock(2), true)
                .unwrap_err()
                .code,
            "stale_claim"
        );
        assert_eq!(recovered.summary()["speech_uncertain"], 1);
    }

    #[test]
    fn a_full_store_lets_its_oldest_finished_task_go_and_refuses_only_live_ones() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 0);
        for n in 0..MAX_OBSERVATIONS {
            observe(&mut engine, id, &format!("result {n}"), 0);
        }
        let request = Request::Observe {
            guard: guard(&engine, "observe"),
            id,
        };
        assert_eq!(
            engine.handle(&request, &clock(0), false).unwrap_err().code,
            "full"
        );
        for _ in 1..MAX_TASKS {
            prepare(&mut engine, 0);
        }
        assert_eq!(engine.state.tasks.len(), MAX_TASKS);
        assert_eq!(engine.state.tasks[0].observations.len(), MAX_OBSERVATIONS);
        // The first task's agent has finished: it gives way to a new one.
        engine
            .handle(&preparation(&engine, "one more"), &clock(0), false)
            .unwrap();
        assert_eq!(engine.state.tasks.len(), MAX_TASKS);
        assert!(engine.state.tasks.iter().all(|task| task.id != id));
        assert!(engine.state.check().is_ok());
        // Every task kept now waits for a yes: none is over, so the next is
        // refused and nothing is lost.
        assert_eq!(
            engine
                .handle(&preparation(&engine, "too many"), &clock(0), false)
                .unwrap_err()
                .code,
            "full"
        );
        assert_eq!(engine.state.tasks.len(), MAX_TASKS);
    }

    #[test]
    fn list_and_diagnostics_are_bounded_metadata_and_get_returns_one_observation() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 0);
        let observation = observe(&mut engine, id, "PRIVATE ANSWER", 0);
        for _ in 1..10 {
            prepare(&mut engine, 0);
        }
        let page = engine
            .handle(
                &Request::List {
                    after_id: 0,
                    limit: 8,
                },
                &clock(0),
                false,
            )
            .unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 8);
        assert_eq!(page["next_after_id"], 8);
        assert!(!page.to_string().contains("PRIVATE ANSWER"));
        assert!(!page.to_string().contains("Review this project"));
        assert!(!engine.summary().to_string().contains("Review this project"));
        let page = engine
            .handle(
                &Request::List {
                    after_id: 8,
                    limit: 8,
                },
                &clock(0),
                false,
            )
            .unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 2);
        assert!(page["next_after_id"].is_null());
        let reply = engine
            .handle(
                &Request::GetObservation {
                    id,
                    observation_id: observation,
                },
                &clock(0),
                false,
            )
            .unwrap();
        assert_eq!(reply["data"]["answer"], "PRIVATE ANSWER");
        assert_eq!(reply["completion"], "unverified");
        assert_eq!(
            engine
                .handle(
                    &Request::List {
                        after_id: 0,
                        limit: 0
                    },
                    &clock(0),
                    false
                )
                .unwrap_err()
                .code,
            "bad_request"
        );
    }

    #[test]
    fn the_targets_are_named_by_alias_and_engine_and_never_by_machine_or_project() {
        let target = |engine| Target {
            machine_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            project: "lamp-tasks".into(),
            engine,
        };
        let staff = BTreeMap::from([
            ("office-codex".to_owned(), target(EngineKind::Codex)),
            ("office-claude".to_owned(), target(EngineKind::Claude)),
            ("office-opencode".to_owned(), target(EngineKind::Opencode)),
        ]);
        let mut engine = Engine::open(Memory::default(), staff, STORE, SESSION, &clock(0)).unwrap();
        let before = engine.revision();
        let reply = engine
            .handle(&Request::Targets {}, &clock(0), false)
            .unwrap();
        assert_eq!(
            reply["targets"],
            json!([{"alias":"office-claude","engine":"claude"},{"alias":"office-codex","engine":"codex"},
                {"alias":"office-opencode","engine":"opencode"}])
        );
        assert_eq!(reply["store_id"], STORE);
        assert!(!reply.to_string().contains("bbbbbbbb"));
        assert!(!reply.to_string().contains("lamp-tasks"));
        // A read: nothing is written.
        assert_eq!(engine.revision(), before);
    }

    #[test]
    fn worst_case_json_escaped_observation_fits_reply_bound() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 0);
        let job = request_read(&mut engine, id, 0);
        let mut observed = data(&"\u{0001}".repeat(MAX_ANSWER_BYTES));
        observed.ask = Some("\u{0002}".repeat(MAX_ASK_BYTES));
        engine
            .finish_observation(&job, observed, &clock(0))
            .unwrap();
        let reply = engine
            .handle(
                &Request::GetObservation {
                    id,
                    observation_id: 1,
                },
                &clock(0),
                false,
            )
            .unwrap();
        let bytes = serde_json::to_vec(&reply).unwrap();
        assert!(bytes.len() > 32 * 1024);
        assert!(bytes.len() < MAX_REPLY_BYTES);
    }

    #[test]
    fn invalid_or_oversized_transcripts_do_not_enter_the_ledger() {
        for variant in 0..4 {
            let (mut engine, _) = opened();
            let id = accepted(&mut engine, 0);
            let job = request_read(&mut engine, id, 0);
            let mut observed = data("result");
            match variant {
                0 => observed.answer = "x".repeat(MAX_ANSWER_BYTES + 1),
                1 => observed.ask = Some("x".repeat(MAX_ASK_BYTES + 1)),
                2 => observed.state = "completed".into(),
                _ => observed.agent_id = "same-name".into(),
            }
            engine
                .finish_observation(&job, observed, &clock(0))
                .unwrap();
            assert!(engine.state.tasks[0].observations.is_empty());
            assert_eq!(
                engine.state.tasks[0].read.as_ref().unwrap().state,
                ReadState::Failed
            );
        }
    }

    #[test]
    fn clock_regression_boot_change_and_reused_process_identity_fail_closed() {
        for now in [
            Clock {
                boot_id: "other-boot".into(),
                monotonic_ms: 100,
            },
            clock(9),
        ] {
            let (mut engine, _) = opened();
            engine.tick(&clock(10)).unwrap();
            assert_eq!(engine.tick(&now).unwrap_err().code, "clock");
            assert!(engine.failed());
            assert_eq!(
                engine
                    .handle(&Request::Status {}, &clock(100), false)
                    .unwrap_err()
                    .code,
                "storage"
            );
        }
        let (_, memory) = opened();
        assert_eq!(
            Engine::open(memory, targets(), STORE, SESSION, &clock(1))
                .err()
                .unwrap()
                .code,
            "clock"
        );
        assert!(
            Clock {
                boot_id: "bad:clock".into(),
                monotonic_ms: 0
            }
            .check()
            .is_err()
        );
        assert!(
            Clock {
                boot_id: "a".repeat(129),
                monotonic_ms: 0
            }
            .check()
            .is_err()
        );
    }

    #[test]
    fn boot_restart_preserves_history_but_cannot_fabricate_observation_age() {
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 100);
        let observed = observe(&mut engine, id, "result", 101);
        let now = Clock {
            boot_id: "new-boot".into(),
            monotonic_ms: 0,
        };
        let mut recovered = Engine::open(memory, targets(), STORE, RESTART, &now).unwrap();
        assert_eq!(dispatch(&recovered, id), DispatchState::Accepted);
        assert_eq!(
            recovered
                .handle(&claim(&recovered, id, observed), &now, true)
                .unwrap_err()
                .code,
            "stale_observation"
        );
    }

    #[test]
    fn prompt_target_speech_and_checked_arithmetic_bounds_are_enforced() {
        let (mut engine, _) = opened();
        for text in [
            String::new(),
            "--force".into(),
            "x".repeat(MAX_PROMPT_BYTES + 1),
            "é".repeat(501),
        ] {
            assert_eq!(
                engine
                    .handle(&preparation(&engine, &text), &clock(0), false)
                    .unwrap_err()
                    .code,
                "bad_text"
            );
        }
        let mut request = preparation(&engine, "safe text");
        if let Request::Prepare { target, .. } = &mut request {
            *target = "not-provisioned".into();
        }
        assert_eq!(
            engine.handle(&request, &clock(0), false).unwrap_err().code,
            "target"
        );
        let id = accepted(&mut engine, 0);
        let observation = observe(&mut engine, id, "result", 0);
        let mut request = claim(&engine, id, observation);
        if let Request::ClaimSpeech { text, .. } = &mut request {
            *text = "é".repeat(501);
        }
        assert_eq!(
            engine.handle(&request, &clock(0), true).unwrap_err().code,
            "bad_text"
        );
        let memory = Memory::default();
        let mut engine = Engine::open(memory, targets(), STORE, SESSION, &clock(u64::MAX)).unwrap();
        assert_eq!(
            engine
                .handle(&preparation(&engine, "safe"), &clock(u64::MAX), false)
                .unwrap_err()
                .code,
            "full"
        );
        assert!(engine.state.tasks.is_empty());
    }

    #[test]
    fn malformed_snapshots_fail_closed_before_writing() {
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 0);
        let observation = observe(&mut engine, id, "result", 0);
        engine
            .handle(&claim(&engine, id, observation), &clock(0), true)
            .unwrap();
        let original: Value =
            serde_json::from_slice(memory.0.borrow().bytes.as_ref().unwrap()).unwrap();
        for variant in 0..9 {
            let mut altered = original.clone();
            match variant {
                0 => altered["schema"] = json!(2),
                1 => altered["unexpected"] = json!(true),
                2 => altered["next_task_id"] = json!(1),
                3 => altered["tasks"][0]["prepared_digest"] = json!("0".repeat(64)),
                4 => altered["tasks"][0]["dispatch"]["state"] = json!("prepared"),
                5 => altered["tasks"][0]["observations"][0]["digest"] = json!("0".repeat(64)),
                6 => {
                    altered["tasks"][0]["observations"][0]["speech"]["text_digest"] =
                        json!("0".repeat(64))
                }
                7 => altered["last_receipt"]["response"]["claim_id"] = json!(999),
                _ => altered["tasks"][0]["read"]["observation_id"] = json!(999),
            }
            let memory = Memory::default();
            memory.0.borrow_mut().bytes = Some(serde_json::to_vec(&altered).unwrap());
            assert_eq!(
                Engine::open(memory.clone(), targets(), STORE, RESTART, &clock(1))
                    .err()
                    .unwrap()
                    .code,
                "storage",
                "variant {variant}"
            );
            assert_eq!(memory.0.borrow().writes, 0);
        }
        for bytes in [Vec::new(), b"{}".to_vec(), vec![b' '; MAX_STATE_BYTES + 1]] {
            let memory = Memory::default();
            memory.0.borrow_mut().bytes = Some(bytes);
            assert_eq!(
                Engine::open(memory, targets(), STORE, RESTART, &clock(1))
                    .err()
                    .unwrap()
                    .code,
                "storage"
            );
        }
    }

    #[test]
    fn failed_prepare_or_observation_commit_is_not_installed_in_memory() {
        let (mut engine, memory) = opened();
        memory.0.borrow_mut().fail_before = true;
        assert_eq!(
            engine
                .handle(&preparation(&engine, "not committed"), &clock(0), false)
                .unwrap_err()
                .code,
            "storage"
        );
        assert!(engine.state.tasks.is_empty());
        assert!(stored(&memory).tasks.is_empty());
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 0);
        let job = request_read(&mut engine, id, 0);
        memory.0.borrow_mut().fail_before = true;
        assert_eq!(
            engine
                .finish_observation(&job, data("not committed"), &clock(1))
                .unwrap_err()
                .code,
            "storage"
        );
        assert!(engine.state.tasks[0].observations.is_empty());
        assert_eq!(
            stored(&memory).tasks[0].read.as_ref().unwrap().state,
            ReadState::Attempting
        );
    }

    #[test]
    fn reused_remote_agent_and_creation_ids_never_bind_distinct_tasks() {
        for outcome in [
            LaunchOutcome::Created {
                agent_id: AGENT.into(),
            },
            LaunchOutcome::Pending {
                creation_id: "creation-key-00000001".into(),
            },
        ] {
            let (mut engine, memory) = opened();
            let first = queued(&mut engine, 0);
            engine.begin_launch(&clock(0)).unwrap();
            engine
                .finish_launch(first, outcome.clone(), &clock(0))
                .unwrap();
            let second = queued(&mut engine, 0);
            engine.begin_launch(&clock(0)).unwrap();
            engine.finish_launch(second, outcome, &clock(0)).unwrap();
            assert_eq!(dispatch(&engine, second), DispatchState::Uncertain);
            assert_eq!(
                engine.state.tasks[1].dispatch.code.as_deref(),
                Some("reused_remote_id")
            );
            let mut invalid = stored(&memory);
            invalid.tasks[1].dispatch = invalid.tasks[0].dispatch.clone();
            assert_eq!(invalid.check().unwrap_err().code, "storage");
        }
    }

    #[test]
    fn receipt_revision_must_identify_the_actual_committed_effect() {
        let (mut engine, memory) = opened();
        prepare(&mut engine, 10);
        let mut invalid = stored(&memory);
        let receipt = invalid.last_receipt.as_mut().unwrap();
        receipt.at.monotonic_ms = 9;
        assert_eq!(invalid.check().unwrap_err().code, "storage");
    }

    /// #560, the first builder's bound (2026-10-16, 20:49): a person at the
    /// lamp who handed work to it hears the result within 20 s of its agent
    /// settling. The tasks part's share of that can be no more than the
    /// whole, so an agent that settles must be listed as settled
    /// ([`Engine::settled_within`], what `tasks/status` says) by then.
    /// The watch alone looks too seldom for it (30 s, then 2 min, and a
    /// second look 10 s later), so the end is pushed: Harness's own `wait`
    /// (`serve --push-wait`) comes back when the agent ends, after its two
    /// looks [`WAIT_LOOKS_MS`] apart, and [`Engine::push`] reads it at once.
    const RESULT_HEARD_WITHIN_MS: u64 = 20_000;
    /// Harness's `wait` looks every 3 s and calls an agent done on two looks
    /// at one turn (Harness 0.4.17), so it returns within two looks.
    const WAIT_LOOKS_MS: u64 = 2 * 3_000;

    #[test]
    fn an_agent_that_settles_is_listed_settled_within_the_time_the_result_is_owed() {
        let at_work = || Observation {
            state: "working".into(),
            tools: 0,
            ..data("")
        };
        let mut late = Vec::new();
        // Soon after the hand-over, while the watch looks often, and after
        // its first ten minutes, when it looks every two.
        for settles_after in [1_000, 45_000, WATCH_EARLY_FOR_MS + 5_000] {
            let (mut engine, _) = opened();
            let id = accepted(&mut engine, 10);
            let settled_at = 10 + settles_after;
            let owed_by = settled_at + RESULT_HEARD_WITHIN_MS;
            let mut listed = None;
            // The runtime asks whether a look is due every tick (10 ms);
            // each second here is generous to the watch. Harness answers at
            // once: at work before the agent settles, done after.
            let mut at = 10;
            let mut pushed = false;
            while listed.is_none() && at <= owed_by + WATCH_LATER_MS {
                // The wait comes back two looks after the agent settled; the
                // runtime pushes until a read is taken.
                let push = !pushed && at >= settled_at + WAIT_LOOKS_MS;
                let push = push && engine.push(id, &clock(at)).unwrap();
                pushed |= push;
                if push || engine.watch(&clock(at)).unwrap() {
                    let job = engine.begin_observation(&clock(at)).unwrap().unwrap();
                    let seen = if at < settled_at {
                        at_work()
                    } else {
                        data("The names: Lumi, Sunny and Archie.")
                    };
                    engine.finish_observation(&job, seen, &clock(at)).unwrap();
                }
                if !engine.settled_within(&clock(at), u64::MAX).is_empty() {
                    listed = Some(at);
                }
                at += 1_000;
            }
            if listed.is_none_or(|listed| listed > owed_by) {
                late.push(format!(
                    "settled {settles_after} ms after the hand-over, listed {} s after it settled",
                    listed.map_or("never".into(), |listed| ((listed - settled_at) / 1_000)
                        .to_string())
                ));
            }
        }
        assert!(
            late.is_empty(),
            "owed within {RESULT_HEARD_WITHIN_MS} ms: {late:?}"
        );
    }

    #[test]
    fn a_push_reads_at_once_keeps_an_idle_answer_and_is_refused_when_it_cannot_be_taken() {
        let (mut engine, memory) = opened();
        // Not handed over yet: nothing to push.
        let prepared = prepare(&mut engine, 5);
        assert!(!engine.push(prepared, &clock(5)).unwrap());
        let id = accepted(&mut engine, 10);
        // A read already asked for: refused, the caller tries again.
        let job = request_read(&mut engine, id, 20);
        assert!(!engine.push(id, &clock(20)).unwrap());
        engine
            .finish_observation(
                &job,
                Observation {
                    state: "working".into(),
                    ..data("Halfway.")
                },
                &clock(30),
            )
            .unwrap();
        // Long before the watch's own look: read now, stored as confirmed.
        assert!(engine.push(id, &clock(40)).unwrap());
        let saved = stored(&memory);
        let index = find(&saved, id).unwrap();
        assert!(saved.tasks[index].read.as_ref().unwrap().confirmed);
        assert_eq!(engine.pushed(), std::collections::BTreeSet::from([id]));
        let job = engine.begin_observation(&clock(40)).unwrap().unwrap();
        engine
            .finish_observation(&job, data("Lumi, Sunny and Archie."), &clock(41))
            .unwrap();
        // Kept on that one read, with no second look; settled, so no more.
        let task = &engine.state.tasks[find(&engine.state, id).unwrap()];
        assert_eq!(task.observations.len(), 2);
        assert!(task.watch_idle.is_none());
        assert!(!engine.push(id, &clock(50)).unwrap());
        assert!(!engine.push(999, &clock(50)).unwrap());
    }

    #[test]
    fn the_watch_looks_on_its_own_keeps_only_what_is_settled_and_then_stops() {
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 10);
        let task =
            |engine: &Engine<Memory>| engine.state.tasks[find(&engine.state, id).unwrap()].clone();
        let at_work = || Observation {
            state: "working".into(),
            tools: 0,
            ..data("")
        };
        // Half a minute after it was handed over, not before.
        assert!(!engine.watch(&clock(10 + WATCH_EARLY_MS - 1)).unwrap());
        assert!(engine.watch(&clock(10 + WATCH_EARLY_MS)).unwrap());
        // One read at a time.
        assert!(!engine.watch(&clock(10 + WATCH_EARLY_MS)).unwrap());
        let job = engine
            .begin_observation(&clock(10 + WATCH_EARLY_MS))
            .unwrap()
            .unwrap();
        assert!(!engine.watch(&clock(10 + 3 * WATCH_EARLY_MS)).unwrap());
        // Still at work: nothing is kept, and the read ends `working`.
        let first = WATCH_EARLY_MS + 20;
        engine
            .finish_observation(&job, at_work(), &clock(first))
            .unwrap();
        assert!(task(&engine).observations.is_empty());
        assert_eq!(task(&engine).read.unwrap().code.as_deref(), Some("working"));
        // The next look half a minute after that one ended. An idle agent
        // with no answer has not begun.
        assert!(!engine.watch(&clock(first + WATCH_EARLY_MS - 1)).unwrap());
        assert!(engine.watch(&clock(first + WATCH_EARLY_MS)).unwrap());
        let job = engine
            .begin_observation(&clock(first + WATCH_EARLY_MS))
            .unwrap()
            .unwrap();
        let second = first + WATCH_EARLY_MS + 5;
        engine
            .finish_observation(&job, data("  "), &clock(second))
            .unwrap();
        assert!(task(&engine).observations.is_empty());
        // After ten minutes, every two.
        let late = 10 + WATCH_EARLY_FOR_MS;
        assert!(engine.watch(&clock(late)).unwrap());
        let job = engine.begin_observation(&clock(late)).unwrap().unwrap();
        engine
            .finish_observation(&job, at_work(), &clock(late))
            .unwrap();
        assert!(!engine.watch(&clock(late + WATCH_LATER_MS - 1)).unwrap());
        assert!(engine.watch(&clock(late + WATCH_LATER_MS)).unwrap());
        // Finished: seen once, not kept yet; looked at again soon.
        let job = engine
            .begin_observation(&clock(late + WATCH_LATER_MS))
            .unwrap()
            .unwrap();
        let once = late + WATCH_LATER_MS + 3_000;
        engine
            .finish_observation(&job, data("Lumi, Sunny and Archie."), &clock(once))
            .unwrap();
        assert!(task(&engine).observations.is_empty());
        assert_eq!(
            task(&engine).read.unwrap().code.as_deref(),
            Some("idle_once")
        );
        assert!(!engine.watch(&clock(once + WATCH_CONFIRM_MS - 1)).unwrap());
        assert!(engine.watch(&clock(once + WATCH_CONFIRM_MS)).unwrap());
        // The same answer again: kept, and the watch stops once it has
        // followed it for WATCH_FOLLOW_MS.
        let job = engine
            .begin_observation(&clock(once + WATCH_CONFIRM_MS))
            .unwrap()
            .unwrap();
        let done = once + WATCH_CONFIRM_MS + 3_000;
        engine
            .finish_observation(&job, data("Lumi, Sunny and Archie."), &clock(done))
            .unwrap();
        assert!(task(&engine).watch_idle.is_none());
        assert_eq!(task(&engine).observations.len(), 1);
        assert_eq!(
            task(&engine).observations[0].data.answer,
            "Lumi, Sunny and Archie."
        );
        // Let go, and said settled for a while after, by this boot's clock
        // (#1021); never by another boot's.
        assert!(engine.watched(&clock(done + 1_000)).is_empty());
        assert_eq!(
            engine.settled_within(&clock(done + 1_000), 120_000),
            [Settled {
                task_id: id,
                engine: "codex".into(),
                state: "idle".into(),
                ago_ms: 1_000,
            }]
        );
        assert!(
            engine
                .settled_within(&clock(done + 120_000), 120_000)
                .is_empty()
        );
        let elsewhere = Clock {
            boot_id: "another-boot".into(),
            monotonic_ms: done + 1_000,
        };
        assert!(engine.settled_within(&elsewhere, 120_000).is_empty());
        assert!(!engine.watch(&clock(done + WATCH_AT_MOST_MS / 2)).unwrap());
        // The watch's reads are stored as such and reopen.
        assert!(stored(&memory).tasks[0].read.as_ref().unwrap().watch);
        let reopened = Engine::open(memory, targets(), STORE, RESTART, &clock(done + 1)).unwrap();
        assert_eq!(reopened.state.tasks[0].observations.len(), 1);
    }

    /// Only the watch's own read makes a task settled news, and only while
    /// it is the task's latest (Fourth on #1048): what the person asked to
    /// have read, they have heard.
    #[test]
    fn only_the_watchs_own_latest_read_makes_a_task_settled_news() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 10);
        // Someone asks, and the read finds it finished: not news, and the
        // watch has nothing left to look at.
        let asked = Request::Observe {
            guard: guard(&engine, "observe"),
            id,
        };
        engine.handle(&asked, &clock(20), false).unwrap();
        let job = engine.begin_observation(&clock(20)).unwrap().unwrap();
        engine
            .finish_observation(&job, data("Lumi, Sunny and Archie."), &clock(30))
            .unwrap();
        assert!(engine.settled_within(&clock(40), 120_000).is_empty());
        assert!(!engine.watch(&clock(30 + WATCH_EARLY_MS)).unwrap());
        // Another (its own store: the agent is the same), which the watch
        // finds asking the person: news, until someone asks to have it read.
        let (mut engine, _) = opened();
        let other = accepted(&mut engine, 10);
        assert!(engine.watch(&clock(10 + WATCH_EARLY_MS)).unwrap());
        let job = engine
            .begin_observation(&clock(10 + WATCH_EARLY_MS))
            .unwrap()
            .unwrap();
        let found = 10 + WATCH_EARLY_MS + 5;
        let asks = Observation {
            state: "needs-input".into(),
            ask: Some("Which colour?".into()),
            ..data("I need one thing.")
        };
        engine
            .finish_observation(&job, asks, &clock(found))
            .unwrap();
        let said: Vec<(u64, String, u64)> = engine
            .settled_within(&clock(found + 10), 120_000)
            .into_iter()
            .map(|settled| (settled.task_id, settled.state, settled.ago_ms))
            .collect();
        assert_eq!(said, [(other, "needs-input".to_owned(), 10)]);
        let asked = Request::Observe {
            guard: guard(&engine, "observe"),
            id: other,
        };
        engine.handle(&asked, &clock(found + 20), false).unwrap();
        assert!(
            engine
                .settled_within(&clock(found + 20), 120_000)
                .is_empty()
        );
    }

    /// Ten on #1145 (its follow looks at an idle answer it kept): a later
    /// look of the watch at a task it found settled neither takes it off
    /// the list while under way nor gives it a new time when it finds the
    /// same answer again; one that finds the agent back at work does take
    /// it off. Its two minutes count from the first finding.
    #[test]
    fn a_later_look_neither_hides_a_settled_task_nor_moves_its_time() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 10);
        assert!(engine.watch(&clock(10 + WATCH_EARLY_MS)).unwrap());
        let job = engine
            .begin_observation(&clock(10 + WATCH_EARLY_MS))
            .unwrap()
            .unwrap();
        let found = 10 + WATCH_EARLY_MS + 5;
        // Asking the person: kept on the first look, with #1145 too (which
        // keeps an idle answer only on a second).
        let asks = Observation {
            state: "needs-input".into(),
            ask: Some("Which colour?".into()),
            ..data("I need one thing.")
        };
        engine
            .finish_observation(&job, asks, &clock(found))
            .unwrap();
        let listed = |engine: &Engine<Memory>, at: u64| -> Vec<(u64, u64)> {
            engine
                .settled_within(&clock(at), 120_000)
                .into_iter()
                .map(|settled| (settled.task_id, settled.ago_ms))
                .collect()
        };
        assert_eq!(listed(&engine, found + 1_000), [(id, 1_000)]);
        // A later look of the watch under way.
        let index = find(&engine.state, id).unwrap();
        let mut read = engine.state.tasks[index].read.clone().unwrap();
        read.state = ReadState::Attempting;
        read.finished = None;
        engine.state.tasks[index].read = Some(read.clone());
        assert_eq!(listed(&engine, found + 2_000), [(id, 2_000)]);
        // It found the same answer again: the first finding's time stands.
        read.state = ReadState::Finished;
        engine.state.tasks[index].read = Some(read.clone());
        engine.state.tasks[index].observations[0].seen_count += 1;
        assert_eq!(listed(&engine, found + 3_000), [(id, 3_000)]);
        assert!(listed(&engine, found + 120_000).is_empty());
        // It found the agent back at work: off the list at once.
        read.state = ReadState::Failed;
        read.code = Some("working".into());
        engine.state.tasks[index].read = Some(read);
        assert!(listed(&engine, found + 4_000).is_empty());
    }

    #[test]
    fn the_watch_keeps_an_idle_answer_only_when_the_next_look_finds_it_again() {
        // lamp-75c9, 2026-10-16: an agent that ran `sleep 25` in the
        // background ended its turn, read idle, went back to work when the
        // job ended, and only then was done.
        let (mut engine, memory) = opened();
        let id = accepted(&mut engine, 10);
        let task =
            |engine: &Engine<Memory>| engine.state.tasks[find(&engine.state, id).unwrap()].clone();
        let look = |engine: &mut Engine<Memory>, at: u64, seen: Observation| {
            assert!(engine.watch(&clock(at)).unwrap());
            let job = engine.begin_observation(&clock(at)).unwrap().unwrap();
            engine.finish_observation(&job, seen, &clock(at)).unwrap();
        };
        let waiting = data("Sleeping 25 s in the background; I'll say done after.");
        let at_work = Observation {
            state: "working".into(),
            ..waiting.clone()
        };
        let mut at = 10 + WATCH_EARLY_MS;
        look(&mut engine, at, waiting.clone());
        assert!(task(&engine).observations.is_empty());
        assert!(task(&engine).watch_idle.is_some());
        // The idle seen once is remembered across a restart.
        assert!(stored(&memory).tasks[0].watch_idle.is_some());
        // Back at work: forgotten, and the usual half minute again.
        at += WATCH_CONFIRM_MS;
        look(&mut engine, at, at_work);
        assert!(task(&engine).watch_idle.is_none());
        assert_eq!(task(&engine).read.unwrap().code.as_deref(), Some("working"));
        assert!(!engine.watch(&clock(at + WATCH_CONFIRM_MS)).unwrap());
        // Idle on the same line again: seen once more, still not kept.
        at += WATCH_EARLY_MS;
        look(&mut engine, at, waiting);
        assert!(task(&engine).observations.is_empty());
        // A different answer on the next look is a new turn, seen once.
        at += WATCH_CONFIRM_MS;
        look(&mut engine, at, data("Done."));
        assert!(task(&engine).observations.is_empty());
        assert_eq!(
            task(&engine).read.unwrap().code.as_deref(),
            Some("idle_once")
        );
        // A failed read keeps what was seen: the next look still confirms.
        at += WATCH_CONFIRM_MS;
        assert!(engine.watch(&clock(at)).unwrap());
        let job = engine.begin_observation(&clock(at)).unwrap().unwrap();
        engine
            .observation_failed(&job, "timeout", &clock(at))
            .unwrap();
        assert!(task(&engine).watch_idle.is_some());
        at += WATCH_CONFIRM_MS;
        look(&mut engine, at, data("Done."));
        assert_eq!(task(&engine).observations.len(), 1);
        assert_eq!(task(&engine).observations[0].data.answer, "Done.");
        assert!(task(&engine).watch_idle.is_none());
        // Kept, and still looked at every two minutes for ten: the same
        // answer is only seen again, with no second look.
        assert!(!engine.watch(&clock(at + WATCH_LATER_MS - 1)).unwrap());
        at += WATCH_LATER_MS;
        look(&mut engine, at, data("Done."));
        assert_eq!(task(&engine).observations.len(), 1);
        assert_eq!(task(&engine).observations[0].seen_count, 2);
        assert!(task(&engine).watch_idle.is_none());
        // Ten minutes after it was kept, the watch leaves it.
        let kept = at - WATCH_LATER_MS;
        assert!(!engine.watch(&clock(kept + WATCH_FOLLOW_MS + 1)).unwrap());
        // Progress reads it no longer either way.
        assert!(engine.watched(&clock(at)).is_empty());
        // A question for the person is kept on the first look.
        let (mut asking, _) = opened();
        accepted(&mut asking, 10);
        let question = Observation {
            state: "needs-input".into(),
            ..data("Which folder?")
        };
        look(&mut asking, 10 + WATCH_EARLY_MS, question);
        assert_eq!(asking.state.tasks[0].observations.len(), 1);
    }

    #[test]
    fn an_idle_wait_longer_than_the_second_look_is_followed_to_its_real_end() {
        // The #1145 review: the agent waits 25 s on its background `sleep`,
        // so both looks find it idle and its "I'll say done after" is kept.
        // The watch goes on looking and keeps what it really ends with.
        let (mut engine, _) = opened();
        accepted(&mut engine, 10);
        let look = |engine: &mut Engine<Memory>, at: u64, seen: Observation| {
            assert!(engine.watch(&clock(at)).unwrap());
            let job = engine.begin_observation(&clock(at)).unwrap().unwrap();
            engine.finish_observation(&job, seen, &clock(at)).unwrap();
        };
        let waiting = data("Sleeping 25 s in the background; I'll say done after.");
        let mut at = 10 + WATCH_EARLY_MS;
        look(&mut engine, at, waiting.clone());
        at += WATCH_CONFIRM_MS;
        look(&mut engine, at, waiting);
        assert_eq!(engine.state.tasks[0].observations.len(), 1);
        // Two minutes on, the agent has gone on and finished: a new answer,
        // confirmed by a second look, kept as a new observation.
        at += WATCH_LATER_MS;
        look(&mut engine, at, data("Done: slept 25 s."));
        assert_eq!(engine.state.tasks[0].observations.len(), 1);
        at += WATCH_CONFIRM_MS;
        look(&mut engine, at, data("Done: slept 25 s."));
        let observations = &engine.state.tasks[0].observations;
        assert_eq!(observations.len(), 2);
        assert_eq!(observations[1].data.answer, "Done: slept 25 s.");
        // A question ends the following at once.
        at += WATCH_LATER_MS;
        let question = Observation {
            state: "needs-input".into(),
            ..data("May I delete the log?")
        };
        look(&mut engine, at, question);
        assert_eq!(engine.state.tasks[0].observations.len(), 3);
        assert!(!engine.watch(&clock(at + WATCH_LATER_MS)).unwrap());
    }

    #[test]
    fn the_watch_hears_a_question_leaves_a_day_old_task_and_an_asked_read_keeps_work_in_progress() {
        let (mut engine, _) = opened();
        let id = accepted(&mut engine, 10);
        // Someone asked for the read: what it finds is kept, at work or not.
        let asked = Request::Observe {
            guard: guard(&engine, "observe"),
            id,
        };
        engine.handle(&asked, &clock(20), false).unwrap();
        let job = engine.begin_observation(&clock(20)).unwrap().unwrap();
        let at_work = Observation {
            state: "working".into(),
            ..data("Halfway through.")
        };
        engine
            .finish_observation(&job, at_work, &clock(30))
            .unwrap();
        assert_eq!(engine.state.tasks[0].observations.len(), 1);
        // The agent stops to ask: settled, kept, and the watch stops.
        assert!(engine.watch(&clock(30 + WATCH_EARLY_MS)).unwrap());
        // The asked read's receipt went with it: the same request again is
        // refused, never run a second time.
        assert!(
            engine
                .handle(&asked, &clock(30 + WATCH_EARLY_MS), false)
                .is_err()
        );
        let job = engine
            .begin_observation(&clock(30 + WATCH_EARLY_MS))
            .unwrap()
            .unwrap();
        let question = Observation {
            state: "needs-input".into(),
            ..data("Which folder should the names go in?")
        };
        engine
            .finish_observation(&job, question, &clock(40 + WATCH_EARLY_MS))
            .unwrap();
        assert_eq!(engine.state.tasks[0].observations.len(), 2);
        assert!(!engine.watch(&clock(40 + 10 * WATCH_LATER_MS)).unwrap());
        // A task handed over more than a day ago in this boot is left.
        let (mut old, _) = opened();
        accepted(&mut old, 10);
        assert!(!old.watch(&clock(11 + WATCH_AT_MOST_MS)).unwrap());
        assert!(old.watch(&clock(10 + WATCH_AT_MOST_MS)).unwrap());
    }

    #[test]
    fn progress_reads_the_tasks_the_watch_looks_at_and_nothing_else() {
        let (mut engine, _) = opened();
        // Prepared, not yet handed over: not watched.
        prepare(&mut engine, 5);
        assert!(engine.watched(&clock(5)).is_empty());
        let id = accepted(&mut engine, 10);
        let revision = engine.revision();
        assert_eq!(
            engine.watched(&clock(70_010)),
            vec![Watched {
                task_id: id,
                machine_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
                agent_id: AGENT.into(),
                engine: "codex".into(),
                accepted_ago_ms: Some(70_000),
            }]
        );
        // Listing changes nothing.
        assert_eq!(engine.revision(), revision);
        // In another boot it cannot be dated, and is still watched.
        let other = Clock {
            boot_id: "next-boot".into(),
            monotonic_ms: 1,
        };
        assert_eq!(engine.watched(&other)[0].accepted_ago_ms, None);
        // More than a day old in this boot: left, as the watch leaves it.
        assert!(engine.watched(&clock(11 + WATCH_AT_MOST_MS)).is_empty());
        // Asked for progress without the service holding it: refused, and
        // nothing is changed.
        let asked = engine
            .handle(&Request::GetProgress { id }, &clock(20), false)
            .unwrap_err();
        assert_eq!(asked.code, "progress_off");
        assert_eq!(engine.revision(), revision);
        // Settled: no longer watched.
        let job = request_read(&mut engine, id, 30);
        engine
            .finish_observation(&job, data("Lumi, Sunny and Archie."), &clock(40))
            .unwrap();
        assert!(engine.watched(&clock(50)).is_empty());
    }
}
