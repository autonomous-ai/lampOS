//! A lamp agent's gateway, spoken to directly: no Harness CLI, no TUI, no
//! tmux. lamp-tasks keeps one connection and one long-lived session on the
//! agent's own gateway, hands every task and every make into that session,
//! and reads a run's progress from the gateway's own events.
//!
//! [`AgentSession`] is the engine-neutral contract (V1's `AgentGateway`,
//! narrowed to what lamp-tasks needs). [`openclaw`] implements it over
//! OpenClaw's gateway WebSocket (`ws://127.0.0.1:18789`, token auth, as V1's
//! `runtimes/openclaw` does). Hermes implements it later; until then it
//! answers [`Error::Unsupported`] and lamp-tasks keeps its current path.
//!
//! Fully non-interactive: nothing here ever waits on a person. The gateway
//! is configured so no run asks (exec `security`/`ask` off, no ask tools),
//! and an approval request that still comes is answered by policy, never
//! left to stall the run.

use std::time::{Duration, SystemTime};

pub mod openclaw;

/// Which agent engine a session speaks to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Openclaw,
    Hermes,
}

/// What lamp-tasks hands over: one task, or one make, into the kept session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    /// lamp-tasks' own id for this task (its record, its spoken news).
    pub task_id: String,
    /// The words the agent is given, already in lamp-tasks' shape
    /// ("New task <id>: ...", the reply-language line).
    pub message: String,
    /// What kind of hand-over this is: a spoken task, a skill's `do.task`,
    /// or a make. Kept with the run, for progress and the journal.
    pub kind: TurnKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnKind {
    Spoken,
    SkillTask,
    Make,
}

/// The gateway's id of one run (its `runId`), or lamp-tasks' own key until
/// the gateway names it.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct RunId(pub String);

/// Where a run is now, as the gateway's events say it: what lamp-tasks
/// answers "how far along is the task?" from.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Progress {
    pub run: RunId,
    pub task_id: String,
    pub phase: Phase,
    /// When the run was handed over (lamp-tasks' clock).
    pub started_at: SystemTime,
    /// When the last event of this run came.
    pub updated_at: SystemTime,
    /// Tools the run has finished so far.
    pub tools_done: u32,
    /// The tool running now, or the last one, with a short, safe summary of
    /// its arguments (never secrets, at most 120 characters).
    pub last_tool: Option<ToolStep>,
    /// The end of what the agent has written so far (at most 400
    /// characters), for a spoken "it's ..." line.
    pub last_text: Option<String>,
    /// The agent's own plan or to-do list, when the engine says one.
    pub plan: Vec<PlanItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "is")]
pub enum Phase {
    /// Handed over; the gateway has not started it yet.
    Queued,
    /// Started; thinking, no tool and no text yet.
    Thinking,
    /// A tool is running.
    Tool,
    /// Writing its answer.
    Writing,
    Done,
    Failed {
        why: String,
    },
    Aborted,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolStep {
    pub name: String,
    pub brief: String,
    pub running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlanItem {
    pub text: String,
    pub done: bool,
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The agent's final answer, whole.
    Answered {
        text: String,
        progress: Progress,
    },
    Failed {
        why: String,
        progress: Progress,
    },
    Aborted {
        progress: Progress,
    },
}

/// What a session can't do now, in words lamp-tasks journals or says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The gateway is not up or not serving yet (the #8237 "getting
    /// ready" window): try again; lamp-tasks may tell the person.
    NotReady(String),
    /// Connected, but the gateway refused (auth, protocol version, scope).
    Refused(String),
    /// The connection broke mid-call; the session reconnects on its own.
    Lost(String),
    /// Waited as long as asked.
    TimedOut,
    /// No such run in this session.
    UnknownRun(RunId),
    /// This engine does not do this (Hermes, for now).
    Unsupported(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotReady(why) => write!(f, "the agent's gateway is not ready: {why}"),
            Error::Refused(why) => write!(f, "the agent's gateway refused: {why}"),
            Error::Lost(why) => write!(f, "the agent's gateway connection broke: {why}"),
            Error::TimedOut => f.write_str("timed out"),
            Error::UnknownRun(run) => write!(f, "no run {} in this session", run.0),
            Error::Unsupported(what) => write!(f, "this agent does not {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// The one long-lived session lamp-tasks keeps on an agent's gateway.
///
/// Every call is bounded by its own deadline; none blocks on a person.
pub trait AgentSession: Send {
    fn engine(&self) -> Engine;

    /// Connected, authenticated and serving, with the kept session open
    /// (made or adopted), within `within`. Cheap when already so.
    fn ready(&mut self, within: Duration) -> Result<(), Error>;

    /// Hands `turn` into the kept session; returns once the gateway has
    /// taken it (its `res`), not when the run ends.
    fn send(&mut self, turn: &Turn) -> Result<RunId, Error>;

    /// Waits until `run` ends or `within` passes ([`Error::TimedOut`]: the
    /// run goes on; call again).
    fn wait(&mut self, run: &RunId, within: Duration) -> Result<Outcome, Error>;

    /// Where `run` is now; `None` for a run this session never handed over.
    fn progress(&self, run: &RunId) -> Option<Progress>;

    /// Every run handed over and not yet ended, oldest first.
    fn running(&self) -> Vec<Progress>;

    /// Stops `run` (OpenClaw's `chat.abort`).
    fn abort(&mut self, run: &RunId) -> Result<(), Error>;

    /// A fresh conversation in place of the kept one (context near its
    /// limit, or asked): the old one is ended and its transcript deleted.
    fn reset(&mut self) -> Result<(), Error>;
}
