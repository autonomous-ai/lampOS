//! Work records (#7930, `docs/tasks.md`, "Work records"): one progress
//! record for each piece of work the lamp handed to an agent, so the voice
//! can ask after the right one (`work`, `work_list`) instead of only the
//! newest task.
//!
//! One route feeds it, overwriting each record with the latest it knows:
//! **`task`**, a task in the store (`start_agent_task`), its id the store's
//! as a decimal string. Brought up to date from the store
//! ([`crate::engine::Engine::works`]: the dispatch's state, the newest
//! settled `harness show` observation's state, tools and answer), the
//! watch's `harness ps` read ([`crate::progress::Tracker::line`]: state and
//! last active, never its `about` words) and, with `LAMP_TASKS_STEPS=on`
//! only, each OpenClaw tool call's name and number from its transcript
//! ([`crate::steps`]); off, its step stays empty and its tools come from
//! `harness show` alone.
//!
//! Kept in memory only, never written: after a restart of lamp-tasks a
//! store task's record is opened again: its start and end the store's,
//! when the store dates them in this boot, else when this run first saw
//! it. The owner's "forget everything" (`forget_sessions`) clears them all
//! ([`Board::forget_all`]) and keeps one mark beside the store
//! ([`Forgotten`], [`FORGOTTEN_FILE`]): every store task it held then, on
//! the records or not yet, is never opened again, after a restart too. At
//! most
//! [`MOST_WORKS`] records; one not changed for [`KEPT_MS`] goes. The lock
//! is held only to read or change the records, never across I/O.
use crate::{harness::EngineKind, progress::Tracker};
use lamp_msgs::AgentState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

/// At most this many records; past it the one changed longest ago goes.
pub const MOST_WORKS: usize = 20;
/// A record not changed for this long goes: 12 hours.
pub const KEPT_MS: u64 = 12 * 3_600_000;
/// The task's words are cut to this many characters, at a whole character.
pub const MOST_WORDS_CHARS: usize = 300;
/// The answer is cut to this many characters, at a whole character.
pub const MOST_ANSWER_CHARS: usize = 1_200;
/// A session's machine and agent ids are cut to this many characters.
pub const MOST_ID_CHARS: usize = 64;
/// `work_list`'s records when it names no `limit`.
pub const LIST_DEFAULT: u16 = 5;
/// `work_list`'s most records.
pub const LIST_MOST: u16 = 10;
/// The store tasks whose records went (aged out or pushed out), kept so
/// the refresh from the store doesn't open them again: more than the
/// store's own tasks ([`crate::engine::MAX_TASKS`]).
const MOST_FORGOTTEN: usize = 32;
/// The file beside the store keeping the owner's last "forget everything"
/// ([`Forgotten`]).
pub const FORGOTTEN_FILE: &str = "works-forgotten.json";

/// The owner's last "forget everything" (#7930): the store it was and its
/// newest task id then. Every task of that store up to it is never on the
/// records again: one confirmed a moment before, not yet on the records,
/// or one a restarted lamp-tasks would open again from the store. Kept
/// beside the store ([`FORGOTTEN_FILE`]), ids only. A task only prepared
/// then (`prepared`) is left out: a yes after the forget is new, and that
/// task is on the records once it is confirmed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forgotten {
    pub store_id: String,
    pub through: u64,
    /// The tasks only prepared at the forget, not yet confirmed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prepared: Vec<u64>,
}

impl Forgotten {
    /// Whether store `store_id`'s task `task` was forgotten.
    pub fn hides(&self, store_id: &str, task: u64) -> bool {
        self.store_id == store_id && task <= self.through && !self.prepared.contains(&task)
    }

    /// The mark kept in `file`; none when there is none or it can't be read.
    pub fn load(file: &Path) -> Option<Self> {
        let bytes = std::fs::read(file).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Kept in `file`: written whole beside it (mode 0600, never through a
    /// link), then renamed over it.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        let partial = file.with_extension("json.partial");
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&partial)?;
        out.write_all(&bytes)?;
        out.sync_all()?;
        drop(out);
        std::fs::rename(&partial, file)?;
        if let Some(parent) = file.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}

/// What the work is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Task,
}

/// Where the work is, as the record says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum State {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "working")]
    Working,
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "needs-input")]
    NeedsInput,
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "timed_out")]
    TimedOut,
    #[serde(rename = "cancelled")]
    Cancelled,
}

impl State {
    /// Whether the work is over: its record says `done` true.
    pub fn over(self) -> bool {
        matches!(
            self,
            Self::Stopped | Self::Failed | Self::Done | Self::TimedOut | Self::Cancelled
        )
    }

    /// A settled `harness show` observation's state ([`crate::engine`]'s
    /// `settled`): an idle one with its answer is done.
    pub fn of_settled(state: &str) -> Self {
        match state {
            "idle" => Self::Done,
            "needs-input" => Self::NeedsInput,
            "stopped" => Self::Stopped,
            _ => Self::Failed,
        }
    }

    /// The watch's `harness ps` state, while nothing is settled; none for
    /// one that says nothing of the work (offline, unknown, missing).
    pub fn of_agent(state: AgentState) -> Option<Self> {
        match state {
            AgentState::Working | AgentState::Starting | AgentState::Installing => {
                Some(Self::Working)
            }
            AgentState::Idle => Some(Self::Idle),
            AgentState::NeedsInput => Some(Self::NeedsInput),
            AgentState::Stopped => Some(Self::Stopped),
            AgentState::Failed => Some(Self::Failed),
            AgentState::Offline | AgentState::Unknown | AgentState::Missing => None,
        }
    }
}

/// `text` cut to at most `most` characters, at a whole character.
pub fn cut(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// A store task as the store has it now ([`crate::engine::Engine::works`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskWork {
    pub task_id: u64,
    pub engine: EngineKind,
    /// The task's words, already cut ([`MOST_WORDS_CHARS`]).
    pub words: String,
    /// What the dispatch or the newest settled observation says; none
    /// while the agent has it and nothing is settled (the watch's read
    /// says then).
    pub state: Option<State>,
    /// The tools the newest observation counted.
    pub tools: u64,
    /// The newest settled observation's answer, already cut
    /// ([`MOST_ANSWER_CHARS`]), when it has one.
    pub answer: Option<String>,
    /// The machine and Harness agent the task runs on, once Harness made
    /// its agent.
    pub session: Option<(String, String)>,
    /// How long before this read the store's task was handed over, when
    /// that was in this boot: its record, opened again after a lamp-tasks
    /// restart, keeps that start. None when the store cannot date it.
    pub started_ago_ms: Option<u64>,
    /// How long before this read the store first saw it over, when that
    /// was in this boot: a finished task opened again after a restart
    /// keeps its end, so its elapsed time is its own. None while not over,
    /// or when the store cannot date it.
    pub ended_ago_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Work {
    id: String,
    kind: Kind,
    who: String,
    words: String,
    state: State,
    started_ms: u64,
    updated_ms: u64,
    /// When it was first seen over: its elapsed time stops there.
    ended_ms: Option<u64>,
    last_active_ms: Option<u64>,
    /// Tools counted by the agent's own record (`harness show`).
    shown_tools: u64,
    /// Tool calls read from its transcript ([`crate::steps`]).
    calls: u64,
    /// The latest tool call: its step number and the tool's name alone.
    step: Option<(u64, String)>,
    answer: Option<String>,
    /// Where it runs, once its agent began: the machine and the agent, ids
    /// only.
    session: Option<(String, String)>,
    /// Not yet handed to its agent: the watch's read says `starting` or
    /// `installing` ([`AgentState::Starting`], [`AgentState::Installing`])
    /// while nothing in the store says its state (#8510). Its `state` reads
    /// `working` then, as before; the voice's 20 s line reads this
    /// (`hand_wait.rs`).
    waiting: bool,
}

impl Work {
    fn render(&self, now_ms: u64) -> Value {
        let done = self.state.over();
        let end = self.ended_ms.unwrap_or(now_ms).max(self.started_ms);
        json!({
            "id": self.id,
            "kind": self.kind,
            "who": self.who,
            "words": self.words,
            "state": self.state,
            "started_ms": self.started_ms,
            "updated_ms": self.updated_ms,
            "elapsed_s": end.saturating_sub(self.started_ms) / 1_000,
            "last_active_ms": self.last_active_ms,
            "tools": self.shown_tools.max(self.calls),
            "step": self.step.as_ref().map(|(n, tool)| json!({"n": n, "tool": tool})),
            "done": done,
            "waiting": self.waiting,
            "answer": self.answer.as_ref().filter(|_| done),
            "session": self
                .session
                .as_ref()
                .map(|(machine, agent)| json!({"machine": machine, "agent": agent})),
        })
    }

    /// Into `state`; its end (`end_ms`) kept the first time it is over.
    fn set_state(&mut self, state: State, end_ms: u64) {
        self.state = state;
        if state.over() {
            self.ended_ms.get_or_insert(end_ms);
        } else {
            self.ended_ms = None;
        }
    }
}

/// The records, as last changed.
#[derive(Debug)]
pub struct Board {
    /// In the order they were opened, the newest last.
    works: VecDeque<Work>,
    forgotten: VecDeque<u64>,
    /// At most this many records ([`MOST_WORKS`]).
    most: usize,
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

impl Board {
    pub const fn new() -> Self {
        Self::holding(MOST_WORKS)
    }

    /// Records holding at most `most`.
    const fn holding(most: usize) -> Self {
        Self {
            works: VecDeque::new(),
            forgotten: VecDeque::new(),
            most,
        }
    }

    fn find(&mut self, id: &str) -> Option<&mut Work> {
        self.works.iter_mut().find(|work| work.id == id)
    }

    /// The owner's "forget everything" (#7930): every record goes, with
    /// the person's words and the agents' answers in it. The store's tasks
    /// it held are kept off the records by the store's mark
    /// ([`Forgotten`], [`crate::engine::Engine::works`]), which covers the
    /// ones not on the records yet and a restart too.
    pub fn forget_all(&mut self) {
        self.works.clear();
    }

    /// A store task's tool call `n`, to `tool`,
    /// read from its transcript: the latest step, by its number and the
    /// tool's name alone. A step read again (its result) or older changes
    /// nothing.
    pub fn step(&mut self, task: u64, n: u64, tool: &str, now_ms: u64) {
        let id = task.to_string();
        if let Some(work) = self
            .works
            .iter_mut()
            .find(|work| work.kind == Kind::Task && work.id == id)
            && work.step.as_ref().is_none_or(|(at, _)| n > *at)
            && !tool.is_empty()
        {
            work.calls = work.calls.saturating_add(1);
            work.step = Some((n, tool.to_owned()));
            work.updated_ms = now_ms;
        }
        self.prune(now_ms);
    }

    /// The store's tasks as they are now (`seen`), with the watch's read of
    /// each (`line`: its state, when it was last active, and whether it is
    /// still starting, not yet handed to its agent): each task's record
    /// opened, or overwritten where it changed. A task whose record went is
    /// not opened again.
    pub fn tasks(
        &mut self,
        seen: &[TaskWork],
        line: impl Fn(u64) -> Option<(Option<State>, Option<u64>, bool)>,
        now_ms: u64,
    ) {
        for task in seen {
            let id = task.task_id.to_string();
            let read = line(task.task_id);
            let state = task
                .state
                .or_else(|| read.and_then(|(state, _, _)| state))
                .unwrap_or(State::Working);
            let last_active = read.and_then(|(_, at, _)| at);
            // Only while the watch's read gives the state: the store's own
            // (queued, settled) says it then.
            let waiting = task.state.is_none() && read.is_some_and(|(_, _, starting)| starting);
            let who = task.engine.spoken();
            if self.find(&id).is_none() {
                if self.forgotten.contains(&task.task_id) {
                    continue;
                }
                self.works.push_back(Work {
                    id: id.clone(),
                    kind: Kind::Task,
                    who: String::new(),
                    words: String::new(),
                    state,
                    started_ms: task
                        .started_ago_ms
                        .map_or(now_ms, |ago| now_ms.saturating_sub(ago)),
                    updated_ms: now_ms,
                    ended_ms: None,
                    last_active_ms: None,
                    shown_tools: 0,
                    calls: 0,
                    step: None,
                    answer: None,
                    session: None,
                    waiting,
                });
            }
            let Some(work) = self.find(&id) else {
                continue;
            };
            let before = work.clone();
            work.who = who.to_owned();
            work.words.clone_from(&task.words);
            if state != work.state || (work.ended_ms.is_none() && state.over()) {
                let end = task
                    .ended_ago_ms
                    .map_or(now_ms, |ago| now_ms.saturating_sub(ago));
                work.set_state(state, end);
            }
            if last_active.is_some() {
                work.last_active_ms = last_active;
            }
            work.waiting = waiting;
            work.shown_tools = task.tools;
            work.answer.clone_from(&task.answer);
            if let Some((machine, agent)) = &task.session {
                work.session = Some((cut(machine, MOST_ID_CHARS), cut(agent, MOST_ID_CHARS)));
            }
            if *work != before {
                work.updated_ms = now_ms;
            }
        }
        self.prune(now_ms);
    }

    /// The record of `id` at `now_ms`.
    pub fn get(&mut self, id: &str, now_ms: u64) -> Option<Value> {
        self.prune(now_ms);
        self.works
            .iter()
            .find(|work| work.id == id)
            .map(|work| work.render(now_ms))
    }

    /// At most `limit` records at `now_ms`, the newest handed over first.
    pub fn list(&mut self, limit: usize, now_ms: u64) -> Vec<Value> {
        self.prune(now_ms);
        let mut works: Vec<&Work> = self.works.iter().rev().collect();
        // Stable: of two opened in one millisecond, the later first.
        works.sort_by_key(|work| std::cmp::Reverse(work.started_ms));
        works
            .into_iter()
            .take(limit)
            .map(|work| work.render(now_ms))
            .collect()
    }

    /// Records not changed for [`KEPT_MS`] go, then the ones changed
    /// longest ago until at most [`MOST_WORKS`] are left.
    fn prune(&mut self, now_ms: u64) {
        let mut gone = Vec::new();
        self.works.retain(|work| {
            let keep = now_ms.saturating_sub(work.updated_ms) < KEPT_MS;
            if !keep {
                gone.push(work.clone());
            }
            keep
        });
        while self.works.len() > self.most {
            let Some(oldest) = self
                .works
                .iter()
                .enumerate()
                .min_by_key(|(_, work)| work.updated_ms)
                .map(|(at, _)| at)
            else {
                break;
            };
            if let Some(work) = self.works.remove(oldest) {
                gone.push(work);
            }
        }
        for work in gone.into_iter().filter(|work| work.kind == Kind::Task) {
            if let Ok(task) = work.id.parse::<u64>() {
                self.forgotten.push_back(task);
            }
        }
        while self.forgotten.len() > MOST_FORGOTTEN {
            self.forgotten.pop_front();
        }
    }
}

/// The lamp's records, one store a process: the owner, the steps' thread
/// and the make and skill sockets' threads reach it without a handle
/// passed down every runner, as `skill_task`'s `RUNS_TOLD` is reached.
///
/// In the crate's own tests the many tests running at once share it, so
/// it holds more there: no test's record is pushed out by another's. The
/// bound itself is tested on a [`Board::new`] of its own.
static BOARD: Mutex<Board> =
    Mutex::new(Board::holding(if cfg!(test) { 1_000 } else { MOST_WORKS }));

/// `change` with the lamp's records locked: nothing slow inside it.
pub fn with<R>(change: impl FnOnce(&mut Board) -> R) -> R {
    let mut board = BOARD.lock().unwrap_or_else(PoisonError::into_inner);
    change(&mut board)
}

/// The wall clock, in milliseconds since 1970.
pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// [`Board::forget_all`] on the lamp's records.
pub fn forget_all() {
    with(Board::forget_all);
}

/// The watch's read of `task` ([`Tracker::line`]) as [`Board::tasks`]
/// takes it: its state, when it was last active, and whether it is still
/// `starting` or `installing` (not yet handed to its agent: OpenClaw
/// direct's queued turn, `direct.rs`'s `state_of`; a Harness agent on a
/// computer still coming up), never its words.
pub fn line_of(tracker: Option<&Tracker>, task: u64) -> Option<(Option<State>, Option<u64>, bool)> {
    let line = tracker?.line(task)?;
    Some((
        State::of_agent(line.state),
        line.last_active_unix_ms,
        matches!(line.state, AgentState::Starting | AgentState::Installing),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_795_000_000_000;

    fn task(id: u64, state: Option<State>) -> TaskWork {
        TaskWork {
            task_id: id,
            engine: EngineKind::Openclaw,
            words: "Find the cheapest flight to Hanoi".into(),
            state,
            tools: 0,
            answer: None,
            session: None,
            started_ago_ms: None,
            ended_ago_ms: None,
        }
    }
    fn none(_: u64) -> Option<(Option<State>, Option<u64>, bool)> {
        None
    }

    /// #7930: a store task's record opened again after a lamp-tasks
    /// restart keeps the start the store dates (handed over 90 s before),
    /// so its elapsed time goes on; one the store cannot date starts now.
    #[test]
    fn a_task_reopened_after_a_restart_keeps_its_start_from_the_store() {
        let mut board = Board::new();
        let dated = TaskWork {
            started_ago_ms: Some(90_000),
            ..task(6, None)
        };
        board.tasks(&[dated], none, T0);
        let record = board.get("6", T0 + 1_000).unwrap();
        assert_eq!(record["started_ms"], T0 - 90_000);
        assert_eq!(record["elapsed_s"], 91);
        board.tasks(&[task(7, None)], none, T0);
        assert_eq!(board.get("7", T0).unwrap()["started_ms"], T0);
    }

    /// #7930 (Twelve's P3): a finished task opened again after a restart
    /// keeps the end the store dates, so a 40 s task reads 40 s, not the
    /// time since it was handed over; an hour later it still does.
    #[test]
    fn a_finished_task_reopened_after_a_restart_keeps_its_end_from_the_store() {
        let mut board = Board::new();
        let done = TaskWork {
            started_ago_ms: Some(90_000),
            ended_ago_ms: Some(50_000),
            answer: Some("Done.".into()),
            ..task(6, Some(State::Done))
        };
        board.tasks(std::slice::from_ref(&done), none, T0);
        assert_eq!(board.get("6", T0).unwrap()["elapsed_s"], 40);
        board.tasks(&[done], none, T0 + 3_600_000);
        assert_eq!(board.get("6", T0 + 3_600_000).unwrap()["elapsed_s"], 40);
    }

    /// #7930: the forget mark hides its own store's tasks up to its id
    /// only, never one only prepared then, and is read back as it was kept.
    #[test]
    fn the_forget_mark_hides_its_stores_tasks_up_to_its_id_and_is_kept() {
        let mark = Forgotten {
            store_id: "s1".into(),
            through: 4,
            prepared: vec![3],
        };
        assert!(mark.hides("s1", 1) && mark.hides("s1", 4));
        assert!(!mark.hides("s1", 5) && !mark.hides("s2", 1));
        assert!(!mark.hides("s1", 3));
        // A mark kept before `prepared` was added reads as none prepared.
        let old: Forgotten = serde_json::from_str(r#"{"store_id":"s1","through":4}"#).unwrap();
        assert!(old.hides("s1", 3));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FORGOTTEN_FILE);
        assert_eq!(Forgotten::load(&file), None);
        mark.save(&file).unwrap();
        assert_eq!(Forgotten::load(&file), Some(mark));
    }

    #[test]
    fn a_task_record_is_opened_updated_then_overwritten_by_a_later_observation() {
        let mut board = Board::new();
        // Accepted, not read yet: working.
        board.tasks(&[task(3, None)], none, T0);
        let first = board.get("3", T0).unwrap();
        assert_eq!(first["kind"], "task");
        assert_eq!(first["who"], "OpenClaw");
        assert_eq!(first["state"], "working");
        assert_eq!(first["started_ms"], T0);
        assert_eq!(first["done"], false);
        assert_eq!(first["answer"], Value::Null);
        assert_eq!(first["last_active_ms"], Value::Null);
        // The watch's read: idle, last active 2 s before.
        board.tasks(
            &[task(3, None)],
            |_| Some((Some(State::Idle), Some(T0 + 28_000), false)),
            T0 + 30_000,
        );
        let read = board.get("3", T0 + 30_000).unwrap();
        assert_eq!(read["state"], "idle");
        assert_eq!(read["last_active_ms"], T0 + 28_000);
        assert_eq!(read["updated_ms"], T0 + 30_000);
        assert_eq!(read["elapsed_s"], 30);
        // Nothing changed: the record isn't touched.
        board.tasks(
            &[task(3, None)],
            |_| Some((Some(State::Idle), Some(T0 + 28_000), false)),
            T0 + 31_000,
        );
        assert_eq!(
            board.get("3", T0 + 31_000).unwrap()["updated_ms"],
            T0 + 30_000
        );
        // The settled observation overwrites it: done, its tools, its answer.
        let settled = TaskWork {
            tools: 4,
            answer: Some("The cheapest is 1.2 million dong on Tuesday.".into()),
            ..task(3, Some(State::Done))
        };
        board.tasks(&[settled], none, T0 + 60_000);
        let done = board.get("3", T0 + 90_000).unwrap();
        assert_eq!(done["state"], "done");
        assert_eq!(done["done"], true);
        assert_eq!(done["tools"], 4);
        assert_eq!(
            done["answer"],
            "The cheapest is 1.2 million dong on Tuesday."
        );
        assert_eq!(done["updated_ms"], T0 + 60_000);
        // Its elapsed time stops at its end.
        assert_eq!(done["elapsed_s"], 60);
        // Still one record.
        assert_eq!(board.list(10, T0 + 90_000).len(), 1);
    }

    #[test]
    fn tasks_are_listed_newest_first() {
        let mut board = Board::new();
        board.tasks(&[task(1, None)], none, T0);
        board.tasks(&[task(2, None)], none, T0 + 1_000);
        board.tasks(&[task(3, None)], none, T0 + 2_000);
        let list = board.list(usize::from(LIST_DEFAULT), T0 + 4_000);
        let ids: Vec<&str> = list
            .iter()
            .map(|work| work["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["3", "2", "1"]);
        assert!(list.iter().all(|work| work["kind"] == "task"));
        assert_eq!(board.list(1, T0 + 4_000).len(), 1);
    }

    #[test]
    fn done_sets_done_and_the_cut_answer_and_only_then_an_answer() {
        let mut board = Board::new();
        let long = "\u{1F600}".repeat(MOST_ANSWER_CHARS);
        board.tasks(&[task(1, None)], none, T0);
        board.tasks(
            &[TaskWork {
                answer: Some(long.clone()),
                ..task(1, Some(State::Done))
            }],
            none,
            T0 + 5_000,
        );
        let work = board.get("1", T0 + 9_000).unwrap();
        assert_eq!(work["done"], true);
        assert_eq!(work["state"], "done");
        assert_eq!(work["answer"], long.as_str());
        assert_eq!(work["elapsed_s"], 5);
        // A task needing input has its words kept but no answer: not done.
        board.tasks(
            &[TaskWork {
                answer: Some("Which day?".into()),
                ..task(2, Some(State::NeedsInput))
            }],
            none,
            T0,
        );
        let asks = board.get("2", T0).unwrap();
        assert_eq!(
            (asks["state"].as_str(), asks["done"].as_bool()),
            (Some("needs-input"), Some(false))
        );
        assert_eq!(asks["answer"], Value::Null);
    }

    #[test]
    fn a_step_shows_the_tool_name_and_number_alone() {
        let mut board = Board::new();
        board.tasks(&[task(5, None)], none, T0);
        board.step(5, 1, "web_search", T0 + 1_000);
        board.step(5, 2, "read_file", T0 + 2_000);
        // The same step read again with its result changes nothing.
        board.step(5, 2, "read_file", T0 + 3_000);
        let work = board.get("5", T0 + 3_000).unwrap();
        assert_eq!(work["step"], json!({"n": 2, "tool": "read_file"}));
        assert_eq!(work["tools"], 2);
        assert_eq!(work["updated_ms"], T0 + 2_000);
        let keys: Vec<&String> = work["step"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["n", "tool"]);
        // A step for an unknown task is no record's.
        board.step(9, 1, "exec", T0);
        assert!(board.get("9", T0).is_none());
    }

    #[test]
    fn at_most_twenty_records_and_none_older_than_twelve_hours() {
        let mut board = Board::new();
        for n in 0..25u64 {
            board.tasks(&[task(n, None)], none, T0 + n);
        }
        let list = board.list(usize::MAX, T0 + 30);
        assert_eq!(list.len(), MOST_WORKS);
        assert_eq!(list[0]["id"], "24");
        assert_eq!(list[MOST_WORKS - 1]["id"], "5");
        assert!(board.get("4", T0 + 30).is_none());
        // Twelve hours after its last change a record goes.
        board.step(24, 1, "exec", T0 + 1_000);
        assert!(board.get("24", T0 + 1_000 + KEPT_MS - 1).is_some());
        assert!(board.get("23", T0 + 1_000 + KEPT_MS - 1).is_none());
        assert!(board.get("24", T0 + 1_000 + KEPT_MS).is_none());
        assert!(board.list(10, T0 + 1_000 + KEPT_MS).is_empty());
        // A store task whose record went is not opened again by the store.
        let mut board = Board::new();
        board.tasks(&[task(1, Some(State::Done))], none, T0);
        assert!(board.get("1", T0 + KEPT_MS).is_none());
        board.tasks(&[task(1, Some(State::Done))], none, T0 + KEPT_MS + 1);
        assert!(board.get("1", T0 + KEPT_MS + 1).is_none());
    }

    /// Where it runs: null until its agent began, then its machine and
    /// agent from the store.
    #[test]
    fn a_record_says_its_session_once_its_agent_began() {
        let mut board = Board::new();
        board.tasks(&[task(4, None)], none, T0);
        assert_eq!(board.get("4", T0).unwrap()["session"], Value::Null);
        let started = TaskWork {
            session: Some(("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(), "agent-7".into())),
            ..task(4, None)
        };
        board.tasks(&[started], none, T0 + 5);
        assert_eq!(
            board.get("4", T0 + 5).unwrap()["session"],
            json!({"machine": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "agent": "agent-7"})
        );
    }

    /// The owner's "forget everything" (#7930): every record goes, the
    /// person's words and the agent's answer with it; the store's mark keeps
    /// its tasks off after it (`engine.rs`,
    /// `forget_everything_leaves_every_store_task_off_the_work_records`).
    #[test]
    fn forget_all_leaves_no_record_words_or_answer() {
        let mut board = Board::new();
        let done = TaskWork {
            answer: Some("The cheapest is on Tuesday.".into()),
            ..task(8, Some(State::Done))
        };
        board.tasks(&[done, task(9, None)], none, T0);
        assert_eq!(
            board.get("8", T0).unwrap()["words"],
            "Find the cheapest flight to Hanoi"
        );
        board.forget_all();
        assert!(board.get("8", T0).is_none() && board.get("9", T0).is_none());
        assert!(board.list(10, T0).is_empty());
        let shown = format!("{board:?}");
        assert!(
            !shown.contains("Hanoi") && !shown.contains("Tuesday"),
            "{shown}"
        );
        // A task handed over after the forget has its record.
        board.tasks(&[task(10, None)], none, T0 + 2);
        assert!(board.get("10", T0 + 2).is_some());
    }

    /// #8510: OpenClaw direct's turn queued on a cold gateway reads
    /// `starting` on the watch: its record says `working`, as before, and
    /// `waiting` true, so the voice's 20 s line still finds it; once the
    /// gateway has it, `waiting` is false. The store's own state (queued,
    /// settled) is never `waiting`.
    #[test]
    fn a_task_the_watch_reads_starting_is_working_and_waiting_until_it_runs() {
        let mut board = Board::new();
        let starting = |_: u64| Some((State::of_agent(AgentState::Starting), None, true));
        board.tasks(&[task(11, None)], starting, T0);
        let record = board.get("11", T0).unwrap();
        assert_eq!(record["state"], "working");
        assert_eq!(record["waiting"], true);
        let working = |_: u64| Some((State::of_agent(AgentState::Working), Some(T0 + 900), false));
        board.tasks(&[task(11, None)], working, T0 + 1_000);
        let record = board.get("11", T0 + 1_000).unwrap();
        assert_eq!(record["state"], "working");
        assert_eq!(record["waiting"], false);
        board.tasks(&[task(12, Some(State::Queued))], starting, T0);
        assert_eq!(board.get("12", T0).unwrap()["waiting"], false);
        board.tasks(&[task(13, None)], none, T0);
        assert_eq!(board.get("13", T0).unwrap()["waiting"], false);
    }

    #[test]
    fn an_unknown_id_has_no_record() {
        let mut board = Board::new();
        assert!(board.get("1", T0).is_none());
        board.tasks(&[task(1, None)], none, T0);
        assert!(board.get("2", T0).is_none());
    }
}
