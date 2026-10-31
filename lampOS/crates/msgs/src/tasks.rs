//! How the work the lamp handed over is going (`tasks/status`,
//! `docs/tasks.md`, "Progress on the bus"). Typed state only: the agent's
//! own words stay in the tasks part, which hands them over on its socket
//! when the lamp is about to say them, and never put them on the bus.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};

/// No more tasks than the tasks part keeps (`lamp-tasks`, `MAX_TASKS`).
pub const TASK_STATUS_MOST_TASKS: usize = 16;
/// Last millisecond of 9999-12-31 UTC.
const MOST_UNIX_MS: u64 = 253_402_300_799_999;

/// How an agent stands, as Harness last listed it (`harness ps`). An agent
/// not in the list at all is `missing`: Harness leaves out one that has
/// stopped, and one that is gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Working,
    Idle,
    NeedsInput,
    Starting,
    Installing,
    Stopped,
    Offline,
    Failed,
    Unknown,
    Missing,
}

/// A cheap sign, read without any model from the agent's progress line,
/// that its work took a step forward. Words matched, not understood: a
/// sign is a hint for the policy, never a claim that the step happened.
/// The phrases behind each are in `lamp-tasks` (`progress::SIGNS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sign {
    /// "tests pass", "all 42 tests passed".
    TestsPass,
    /// "build succeeds", "it compiles".
    BuildWorks,
    /// "fixed", "resolved".
    Fixed,
    /// "found the cause", "root cause".
    Found,
    /// "done", "finished", "implemented".
    Done,
    /// "opened a pull request", "PR #12".
    PullRequest,
}

/// One task handed over, as its agent was last read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskProgress {
    /// The task's number in the tasks part's store: what `get_task` and
    /// `get_progress` take.
    pub task: u64,
    /// Who has the work, as the task's target names it on the bus
    /// (`lamp_tasks`' `EngineKind::bus_name`): `claude`, `codex`, `opencode`,
    /// `devin`, `devin_cloud` or `hermes`.
    pub engine: String,
    pub state: AgentState,
    /// Seconds since the task was handed over, if that was in this boot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_s: Option<u64>,
    /// When Harness last saw the agent do anything, in milliseconds since
    /// 1970 UTC, by the agent's machine's clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_unix_ms: Option<u64>,
    /// When the tasks part read this, in milliseconds since 1970 UTC.
    pub read_unix_ms: u64,
    /// How many times the agent's progress line has changed since the
    /// tasks part began to read it. The first line read is where counting
    /// begins, not a change.
    pub changes: u32,
    /// Whether the line changed at the read this message was made from.
    pub changed: bool,
    /// How much of the line was new at its latest change: the share of its
    /// words that were not in the line before, from 0 to 1.
    pub new_share: f32,
    /// Signs in the line at its latest change that were not in the line
    /// before it.
    pub signs: Vec<Sign>,
    /// Every sign seen in the line since the tasks part began to read it.
    pub signs_seen: Vec<Sign>,
}

/// Every task the tasks part is watching, as last read
/// (`lamp-tasks serve --progress`). Published at start with none, after
/// every read, when one joins or leaves the watch, and when one joins or
/// leaves `settled` (at the end of its two minutes with nothing else
/// happening), so the one kept value always holds all of them and an empty
/// list means nothing is being worked on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskStatus {
    /// The tasks part's store: task numbers belong to it.
    pub store_id: String,
    /// Which run of the tasks part counted these changes: when it began to
    /// read progress, in milliseconds since 1970 UTC. Counts begin again at
    /// each run, so a change is named by its run and its count together.
    pub run: u64,
    pub tasks: Vec<TaskProgress>,
    /// Tasks being watched whose progress has not been read yet in this
    /// run: just handed over, or there when the run began, or every read of
    /// them has failed so far. Not in `tasks`; so `tasks` empty and this
    /// empty is nothing being worked on, and either alone is not (#793
    /// review, 2026-10-15). Added 2026-10-15; left out when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unread: Vec<u64>,
    /// Tasks the watch let go because their agent settled, for a while
    /// after (#1021): the tasks part's own look, while it is the task's
    /// latest read, found the agent finished
    /// (`idle`, with an answer), stopped to ask (`needs_input`), `stopped`
    /// or `failed`. A task is let go then, so `tasks` never shows that end
    /// when no `ps` read came between; this does, for a skill (Task news)
    /// to act on, per task. Not in `tasks` or `unread`. Added 2026-10-16;
    /// left out when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settled: Vec<TaskSettled>,
}

/// One task whose agent settled ([`TaskStatus::settled`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSettled {
    pub task: u64,
    /// Who had the work, as the task's target names it.
    pub engine: String,
    /// `idle` (finished), `needs_input`, `stopped` or `failed`.
    pub state: AgentState,
    /// Always true: a trigger may name it (`field = "settled"`).
    pub settled: bool,
    /// When the tasks part found it settled, in milliseconds since 1970
    /// UTC.
    pub at_unix_ms: u64,
}

impl TaskSettled {
    pub fn check(&self) -> Result<(), Invalid> {
        if self.task == 0 {
            return Err(Invalid("a task's number is positive".into()));
        }
        if !engine_named(&self.engine) {
            return Err(Invalid(
                "an engine is 1..32 lowercase letters, digits or underscores".into(),
            ));
        }
        if !matches!(
            self.state,
            AgentState::Idle | AgentState::NeedsInput | AgentState::Stopped | AgentState::Failed
        ) {
            return Err(Invalid(
                "a settled state is idle, needs_input, stopped or failed".into(),
            ));
        }
        if !self.settled {
            return Err(Invalid("a settled task says settled".into()));
        }
        if self.at_unix_ms > MOST_UNIX_MS {
            return Err(Invalid("a time is within years 1970 through 9999".into()));
        }
        Ok(())
    }
}

fn engine_named(engine: &str) -> bool {
    !engine.is_empty()
        && engine.len() <= 32
        && engine
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn sorted_unique(signs: &[Sign]) -> bool {
    signs.windows(2).all(|pair| pair[0] < pair[1])
}

impl TaskProgress {
    pub fn check(&self) -> Result<(), Invalid> {
        if self.task == 0 {
            return Err(Invalid("a task's number is positive".into()));
        }
        if !engine_named(&self.engine) {
            return Err(Invalid(
                "an engine is 1..32 lowercase letters, digits or underscores".into(),
            ));
        }
        if self.read_unix_ms > MOST_UNIX_MS
            || self.last_active_unix_ms.is_some_and(|ms| ms > MOST_UNIX_MS)
        {
            return Err(Invalid("a time is within years 1970 through 9999".into()));
        }
        if !(0.0..=1.0).contains(&self.new_share) {
            return Err(Invalid(format!(
                "new_share is {}: from 0 to 1",
                self.new_share
            )));
        }
        if !sorted_unique(&self.signs) || !sorted_unique(&self.signs_seen) {
            return Err(Invalid("signs are listed once each, in order".into()));
        }
        if self
            .signs
            .iter()
            .any(|sign| !self.signs_seen.contains(sign))
        {
            return Err(Invalid("a new sign is also a sign seen".into()));
        }
        if self.changed && self.changes == 0 {
            return Err(Invalid("a changed line has been counted".into()));
        }
        Ok(())
    }
}

impl TaskStatus {
    pub const TYPE: &str = "lamp_msgs/TaskStatus";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "tasks/status";

    /// A kept value, replaced when anything in it changes. No period is
    /// promised: its publisher says once a second that it is alive.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.store_id.len() != 32
            || !self
                .store_id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Invalid(
                "a store_id is 32 lowercase hexadecimal characters".into(),
            ));
        }
        if self.run == 0 {
            return Err(Invalid("a run is named by a positive number".into()));
        }
        if self.tasks.len() > TASK_STATUS_MOST_TASKS {
            return Err(Invalid(format!(
                "{} tasks: at most {TASK_STATUS_MOST_TASKS}",
                self.tasks.len()
            )));
        }
        if !self
            .tasks
            .windows(2)
            .all(|pair| pair[0].task < pair[1].task)
        {
            return Err(Invalid("tasks are listed once each, in order".into()));
        }
        if self.unread.len() > TASK_STATUS_MOST_TASKS
            || !self.unread.windows(2).all(|pair| pair[0] < pair[1])
            || self
                .unread
                .iter()
                .any(|task| self.tasks.iter().any(|read| read.task == *task))
        {
            return Err(Invalid(
                "unread tasks are listed once each, in order, and not among the tasks read".into(),
            ));
        }
        if self.settled.len() > TASK_STATUS_MOST_TASKS
            || !self
                .settled
                .windows(2)
                .all(|pair| pair[0].task < pair[1].task)
            || self.settled.iter().any(|settled| {
                self.tasks.iter().any(|read| read.task == settled.task)
                    || self.unread.contains(&settled.task)
            })
        {
            return Err(Invalid(
                "settled tasks are listed once each, in order, and not among the watched".into(),
            ));
        }
        self.settled.iter().try_for_each(TaskSettled::check)?;
        self.tasks.iter().try_for_each(TaskProgress::check)
    }
}

/// The tasks part's optional topic, apart from the first topics.
pub fn tasks_topics() -> Vec<(&'static str, Declaration)> {
    vec![(TaskStatus::TOPIC, TaskStatus::declaration())]
}
