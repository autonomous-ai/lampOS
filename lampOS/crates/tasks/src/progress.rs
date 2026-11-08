//! How each task handed over is going, while its agent works
//! (`serve --progress`, `docs/tasks.md`, "Progress on the bus").
//!
//! `harness show` is empty while an agent works (measured from lamp-4ace on
//! 2026-10-15, 00:05); `harness ps` lists, for every harness on a machine,
//! its state, when it was last active and an `about` line made of its last
//! few events, which changes as the agent reports. So each machine with
//! tasks being watched is read with one `ps`, on the watch's schedule, and
//! what changed is put on the bus as typed state (`tasks/status`): who has
//! the work, its state, how long, when last active, whether the line
//! changed and how much, and cheap signs read from it without any model.
//! The line itself stays here, in memory, and is handed over on the
//! request socket (`get_progress`) only when the lamp is about to say it.
use crate::{
    engine::{Settled, WATCH_EARLY_FOR_MS, WATCH_EARLY_MS, WATCH_LATER_MS, Watched},
    harness::Listed,
};
use lamp_msgs::{AgentState, Sign, TaskProgress, TaskSettled, TaskStatus};

/// How long a task whose agent settled stays on `tasks/status` after the
/// watch let it go (#1021): long enough for a skill that triggers on it to
/// see it, as each value is published at once; short enough that `settled`
/// says what has just happened. Authored.
pub const SETTLED_FOR_MS: u64 = 120_000;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The signs, each with the phrases that show it: words in order, matched
/// whole and without regard to case, `#` standing for any number. A sign
/// is a hint that a step was taken, read from words an agent wrote about
/// its own work; it is never taken for proof that the step happened.
///
/// Chosen from how coding agents report (Claude Code's and Codex's own
/// progress summaries, as their makers show them), not measured on the
/// lamp's own tasks yet: how often each fires on real `about` lines is
/// what the first builder hands back (`docs/tasks.md`).
pub const SIGNS: &[(Sign, &[&str])] = &[
    (
        Sign::TestsPass,
        &[
            "tests pass",
            "tests passed",
            "tests passing",
            "tests are passing",
            "tests now pass",
            "tests all pass",
            "all tests pass",
            "all tests passed",
            "test suite passes",
            "suite passes",
            "tests are green",
            "tests green",
            "# passed",
            "# passing",
            "# tests pass",
            "# tests passed",
            "0 failed",
        ],
    ),
    (
        Sign::BuildWorks,
        &[
            "build succeeds",
            "build succeeded",
            "build passes",
            "build passed",
            "build works",
            "build is green",
            "builds cleanly",
            "builds successfully",
            "compiles",
            "compiles cleanly",
            "compiled successfully",
            "clippy is clean",
            "clippy clean",
        ],
    ),
    (
        Sign::Fixed,
        &["fixed", "resolved", "the fix works", "fix works"],
    ),
    (
        Sign::Found,
        &[
            "found",
            "root cause",
            "the cause",
            "the culprit",
            "identified",
            "tracked down",
            "figured out",
        ],
    ),
    (
        Sign::Done,
        &[
            "done",
            "finished",
            "completed",
            "implemented",
            "all set",
            "wrapped up",
        ],
    ),
    (
        Sign::PullRequest,
        &[
            "pull request",
            "opened a pr",
            "opened pr",
            "created a pr",
            "pr opened",
            "pr is up",
            "pr #",
        ],
    ),
];

/// A phrase with one of these among the three words before it is not taken:
/// it is denied, hoped for or still to come ("not fixed", "once the build
/// succeeds", "I'll open a pull request"). Words matched whole.
const NOT_YET: &[&str] = &[
    "not",
    "no",
    "never",
    "cannot",
    "can't",
    "don't",
    "doesn't",
    "didn't",
    "isn't",
    "aren't",
    "wasn't",
    "weren't",
    "won't",
    "hasn't",
    "haven't",
    "couldn't",
    "shouldn't",
    "without",
    "until",
    "unless",
    "if",
    "once",
    "when",
    "whether",
    "to",
    "will",
    "would",
    "should",
    "might",
    "may",
    "must",
    "need",
    "needs",
    "try",
    "trying",
    "next",
    "i'll",
    "we'll",
    "let's",
    "going",
    "before",
];

/// The words of `text`, lowercase, each with where it ends: runs of letters,
/// digits and apostrophes (a curly one read as straight).
fn words(text: &str) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    let mut word = String::new();
    for (at, c) in text.char_indices() {
        let c = if c == '\u{2019}' { '\'' } else { c };
        if c.is_alphanumeric() || c == '\'' {
            word.extend(c.to_lowercase());
        } else if !word.is_empty() {
            found.push((std::mem::take(&mut word), at));
        }
    }
    if !word.is_empty() {
        found.push((word, text.len()));
    }
    found
}

fn matches(pattern: &str, word: &str) -> bool {
    if pattern == "#" {
        !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit())
    } else {
        pattern == word
    }
}

/// The signs in `text` ([`SIGNS`]), each once.
pub fn signs_in(text: &str) -> BTreeSet<Sign> {
    let words = words(text);
    let mut found = BTreeSet::new();
    // A link to a pull request says one was opened, whatever is around it.
    if text.contains("/pull/") {
        found.insert(Sign::PullRequest);
    }
    for (sign, phrases) in SIGNS {
        'phrases: for phrase in *phrases {
            let pattern: Vec<&str> = phrase.split(' ').collect();
            for start in 0..words.len() {
                let Some(run) = words.get(start..start + pattern.len()) else {
                    break;
                };
                if !pattern.iter().zip(run).all(|(p, (w, _))| matches(p, w)) {
                    continue;
                }
                let before = &words[start.saturating_sub(3)..start];
                if before.iter().any(|(w, _)| NOT_YET.contains(&w.as_str())) {
                    continue;
                }
                // A question is not a step taken ("is it fixed?").
                let end = run[run.len() - 1].1;
                if text[end..].trim_start().starts_with('?') {
                    continue;
                }
                found.insert(*sign);
                break 'phrases;
            }
        }
    }
    found
}

/// The share of the distinct words of `after` that are not in `before`,
/// from 0 to 1: how much of a progress line is new.
pub fn new_share(before: Option<&str>, after: &str) -> f32 {
    let old: BTreeSet<String> = before
        .map(|text| words(text).into_iter().map(|(w, _)| w).collect())
        .unwrap_or_default();
    let new: BTreeSet<String> = words(after).into_iter().map(|(w, _)| w).collect();
    if new.is_empty() {
        return 0.0;
    }
    let fresh = new.iter().filter(|word| !old.contains(*word)).count();
    (fresh as f32 / new.len() as f32).clamp(0.0, 1.0)
}

/// A state as `harness ps` writes it, in the bus's words. One this version
/// does not know is `unknown`, never guessed.
pub fn agent_state(listed: &str) -> AgentState {
    match listed {
        "working" => AgentState::Working,
        "idle" => AgentState::Idle,
        "needs-input" => AgentState::NeedsInput,
        "starting" => AgentState::Starting,
        "installing" => AgentState::Installing,
        "stopped" => AgentState::Stopped,
        "offline" => AgentState::Offline,
        "failed" => AgentState::Failed,
        _ => AgentState::Unknown,
    }
}

#[derive(Clone, Debug)]
struct Tracked {
    engine: String,
    state: AgentState,
    working_s: Option<u64>,
    last_active_unix_ms: Option<u64>,
    read_unix_ms: u64,
    about: Option<String>,
    changes: u32,
    changed: bool,
    new_share: f32,
    signs: BTreeSet<Sign>,
    signs_seen: BTreeSet<Sign>,
}

/// One watched task as last read ([`Tracker::line`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line<'a> {
    pub state: AgentState,
    pub last_active_unix_ms: Option<u64>,
    /// The agent's own words: they stay in the tasks part.
    pub about: Option<&'a str>,
}

/// What came of one `ps`, for the diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    pub reads: u64,
    pub failures: u64,
    /// How long the last read took, and the longest, in milliseconds, from
    /// handing it to the CLI worker to its outcome.
    pub last_ms: u64,
    pub most_ms: u64,
}

/// Every task being watched, as last read, and when each machine was.
#[derive(Debug)]
pub struct Tracker {
    /// This run of reading progress: when it began, in milliseconds since
    /// 1970. Changes are counted from nothing in each run, and a restart
    /// keeps the store and its task numbers, so the run is what keeps a
    /// change of one run from being taken for the same count of another
    /// (#765 review, 2026-10-15).
    run: u64,
    tasks: BTreeMap<u64, Tracked>,
    /// When each machine was last asked, by the boot clock, answered or
    /// not: a failed read waits its turn like any other.
    asked: BTreeMap<String, u64>,
    /// The watched tasks a `ps` has been asked for since they came into the
    /// watch. A machine with one that has not is read at once.
    asked_for: BTreeSet<u64>,
    /// Every task being watched, read or not: those not in `tasks` are
    /// published as unread (#793 review, 2026-10-15).
    watched: BTreeSet<u64>,
    reads: Reads,
    /// Tasks let go because their agent settled, for [`SETTLED_FOR_MS`].
    settled: Vec<TaskSettled>,
    /// A value worth publishing has not been taken yet.
    fresh: bool,
}

impl Tracker {
    /// The tracker as the service starts, for a run that began at `run`,
    /// milliseconds since 1970 (never 0): with a value to publish at once
    /// and no task in it, so the kept topic has a value from the start and
    /// a reader can tell "nothing being worked on" from "the tasks part is
    /// not there".
    pub fn started(run: u64) -> Self {
        Self {
            run: run.max(1),
            tasks: BTreeMap::new(),
            asked: BTreeMap::new(),
            asked_for: BTreeSet::new(),
            watched: BTreeSet::new(),
            reads: Reads::default(),
            settled: Vec::new(),
            fresh: true,
        }
    }

    /// The tasks found settled lately ([`crate::engine::Engine::settled_within`]),
    /// at `now_unix_ms`: each with its settled state and when, kept as first
    /// said so that a value changes only when one comes or goes. A value is
    /// published when the list changes. A task still watched, or a state
    /// this version does not take for settled, is left out.
    pub fn settled(&mut self, found: &[Settled], now_unix_ms: u64) {
        let mut now: Vec<TaskSettled> = found
            .iter()
            .filter(|settled| !self.watched.contains(&settled.task_id))
            .filter_map(|settled| {
                let state = agent_state(&settled.state);
                matches!(
                    state,
                    AgentState::Idle
                        | AgentState::NeedsInput
                        | AgentState::Stopped
                        | AgentState::Failed
                )
                .then(|| {
                    let kept = self
                        .settled
                        .iter()
                        .find(|kept| kept.task == settled.task_id && kept.state == state);
                    TaskSettled {
                        task: settled.task_id,
                        engine: settled.engine.clone(),
                        state,
                        settled: true,
                        at_unix_ms: kept.map_or_else(
                            || now_unix_ms.saturating_sub(settled.ago_ms),
                            |kept| kept.at_unix_ms,
                        ),
                    }
                })
            })
            .collect();
        now.sort_by_key(|settled| settled.task);
        now.dedup_by_key(|settled| settled.task);
        now.truncate(lamp_msgs::TASK_STATUS_MOST_TASKS);
        if now != self.settled {
            self.settled = now;
            self.fresh = true;
        }
    }

    pub fn run(&self) -> u64 {
        self.run
    }

    pub fn reads(&self) -> Reads {
        self.reads
    }

    /// Which tasks are watched now: those no longer watched (settled, or a
    /// day old) are let go, and those new to the watch are unread until a
    /// read of them comes back. When the set changes, a value is published.
    pub fn watching(&mut self, watched: &[Watched]) {
        let before = self.tasks.len();
        self.tasks
            .retain(|id, _| watched.iter().any(|w| w.task_id == *id));
        self.asked_for
            .retain(|id| watched.iter().any(|w| w.task_id == *id));
        let now: BTreeSet<u64> = watched.iter().map(|w| w.task_id).collect();
        if self.tasks.len() != before || now != self.watched {
            self.watched = now;
            self.fresh = true;
        }
    }

    /// The task, if it is on `tasks/status` as settled ([`Self::settled`]).
    pub fn settled_of(&self, task: u64) -> Option<&TaskSettled> {
        self.settled.iter().find(|settled| settled.task == task)
    }

    /// Whether `task` is watched but its progress has not been read yet.
    pub fn unread(&self, task: u64) -> bool {
        self.watched.contains(&task) && !self.tasks.contains_key(&task)
    }

    /// The machine whose `ps` is due, if any. At once for a machine with a
    /// task new to the watch (just handed over, or there when the service
    /// started), so it is on the bus within one read and its first line,
    /// where counting begins, is read as early as it can be. Otherwise the
    /// watch's schedule: every half minute while any of its tasks is in
    /// its first ten minutes and every two after, counted from the last
    /// time it was asked. Tasks no longer watched are let go here too.
    pub fn due(&mut self, watched: &[Watched], now_ms: u64) -> Option<String> {
        self.watching(watched);
        // For each machine: any task in its first ten minutes, any task
        // not yet asked for.
        let mut machines: BTreeMap<&str, (bool, bool)> = BTreeMap::new();
        for task in watched {
            let entry = machines
                .entry(task.machine_id.as_str())
                .or_insert((false, false));
            entry.0 |= task
                .accepted_ago_ms
                .is_some_and(|age| age < WATCH_EARLY_FOR_MS);
            entry.1 |= !self.asked_for.contains(&task.task_id);
        }
        machines
            .into_iter()
            .filter_map(|(machine, (early, new))| {
                let every = if early {
                    WATCH_EARLY_MS
                } else {
                    WATCH_LATER_MS
                };
                let ago = self
                    .asked
                    .get(machine)
                    .map_or(u64::MAX, |at| now_ms.saturating_sub(*at));
                (new || ago >= every).then_some((new, ago, machine))
            })
            .max()
            .map(|(_, _, machine)| machine.to_owned())
    }

    /// The read of `machine` is handed to the CLI worker now, for its
    /// watched tasks.
    pub fn asked(&mut self, machine: &str, watched: &[Watched], now_ms: u64) {
        self.asked.insert(machine.to_owned(), now_ms);
        self.asked_for.extend(
            watched
                .iter()
                .filter(|task| task.machine_id == machine)
                .map(|task| task.task_id),
        );
    }

    /// What one `ps` of `machine` came back with. `wall_ms` is the wall
    /// clock as it came; `took_ms` how long it took. Gives the journal's
    /// line for it: the machine, how long, and each of its watched tasks
    /// as read, in typed words only (never an agent's line).
    pub fn read(
        &mut self,
        machine: &str,
        listed: Result<Vec<Listed>, String>,
        watched: &[Watched],
        wall_ms: u64,
        took_ms: u64,
    ) -> String {
        self.reads.last_ms = took_ms;
        self.reads.most_ms = self.reads.most_ms.max(took_ms);
        let ours: Vec<u64> = watched
            .iter()
            .filter(|task| task.machine_id == machine)
            .map(|task| task.task_id)
            .collect();
        let listed = match listed {
            Ok(listed) => listed,
            Err(code) => {
                self.reads.failures = self.reads.failures.saturating_add(1);
                return format!(
                    "progress read of {machine} failed after {took_ms} ms ({code}); {} as before",
                    tasks_named(&ours)
                );
            }
        };
        self.reads.reads = self.reads.reads.saturating_add(1);
        // "Changed" says what this read found; another machine's tasks
        // keep their count and their latest change, not the flag.
        for tracked in self.tasks.values_mut() {
            tracked.changed = false;
        }
        let mut first = Vec::new();
        for task in watched.iter().filter(|w| w.machine_id == machine) {
            let row = listed.iter().find(|row| row.agent_id == task.agent_id);
            let state = row.map_or(AgentState::Missing, |row| agent_state(&row.state));
            let about = row.and_then(|row| row.about.clone());
            let working_s = task.accepted_ago_ms.map(|ms| ms / 1_000);
            let last_active_unix_ms = row.and_then(|row| row.last_active_unix_ms);
            match self.tasks.get_mut(&task.task_id) {
                None => {
                    first.push(task.task_id);
                    // The first line read is where counting begins.
                    let signs_seen = about.as_deref().map(signs_in).unwrap_or_default();
                    self.tasks.insert(
                        task.task_id,
                        Tracked {
                            engine: task.engine.clone(),
                            state,
                            working_s,
                            last_active_unix_ms,
                            read_unix_ms: wall_ms,
                            about,
                            changes: 0,
                            changed: false,
                            new_share: 0.0,
                            signs: BTreeSet::new(),
                            signs_seen,
                        },
                    );
                }
                Some(tracked) => {
                    tracked.state = state;
                    tracked.working_s = working_s;
                    tracked.read_unix_ms = wall_ms;
                    if last_active_unix_ms.is_some() {
                        tracked.last_active_unix_ms = last_active_unix_ms;
                    }
                    // A line that went away (the agent stopped and is no
                    // longer listed) is not a change of what it says.
                    if let Some(text) = about
                        && tracked.about.as_deref() != Some(text.as_str())
                    {
                        let now = signs_in(&text);
                        let before = tracked.about.as_deref().map(signs_in).unwrap_or_default();
                        tracked.new_share = new_share(tracked.about.as_deref(), &text);
                        tracked.signs = now.difference(&before).copied().collect();
                        tracked.signs_seen.extend(now);
                        tracked.changes = tracked.changes.saturating_add(1);
                        tracked.changed = true;
                        tracked.about = Some(text);
                    }
                }
            }
        }
        self.fresh = true;
        let each: Vec<String> = ours
            .iter()
            .filter_map(|id| {
                let tracked = self.tasks.get(id)?;
                let how = if first.contains(id) {
                    "first read".to_owned()
                } else if tracked.changed {
                    let signs: Vec<String> = tracked.signs.iter().map(named).collect();
                    if signs.is_empty() {
                        "new line".to_owned()
                    } else {
                        format!("new line, {}", signs.join(" "))
                    }
                } else {
                    "same line".to_owned()
                };
                Some(format!("task {id} {}, {how}", named(&tracked.state)))
            })
            .collect();
        format!(
            "progress read of {machine} in {took_ms} ms: {} listed; {}",
            listed.len(),
            if each.is_empty() {
                "no task watched there now".to_owned()
            } else {
                each.join("; ")
            }
        )
    }

    /// The status to publish, if there is a newer one than the last taken.
    pub fn take(&mut self, store_id: &str) -> Option<TaskStatus> {
        if !std::mem::take(&mut self.fresh) {
            return None;
        }
        Some(TaskStatus {
            store_id: store_id.to_owned(),
            run: self.run,
            tasks: self
                .tasks
                .iter()
                .map(|(task, tracked)| TaskProgress {
                    task: *task,
                    engine: tracked.engine.clone(),
                    state: tracked.state,
                    working_s: tracked.working_s,
                    last_active_unix_ms: tracked.last_active_unix_ms,
                    read_unix_ms: tracked.read_unix_ms,
                    changes: tracked.changes,
                    changed: tracked.changed,
                    new_share: tracked.new_share,
                    signs: tracked.signs.iter().copied().collect(),
                    signs_seen: tracked.signs_seen.iter().copied().collect(),
                })
                .collect(),
            unread: self
                .watched
                .iter()
                .filter(|task| !self.tasks.contains_key(task))
                .copied()
                .collect(),
            settled: self.settled.clone(),
        })
    }

    /// The task as last read, for "tasks now" ([`crate::now`]): its state,
    /// when Harness last saw its agent do anything, and its line. Kept in
    /// memory there, as here.
    pub fn line(&self, task: u64) -> Option<Line<'_>> {
        let tracked = self.tasks.get(&task)?;
        Some(Line {
            state: tracked.state,
            last_active_unix_ms: tracked.last_active_unix_ms,
            about: tracked.about.as_deref(),
        })
    }

    /// The task's progress with its words, for `get_progress`: asked for
    /// only when the lamp is about to say it.
    pub fn words(&self, task: u64) -> Option<Value> {
        let tracked = self.tasks.get(&task)?;
        Some(json!({
            "engine": tracked.engine,
            "state": tracked.state,
            "about": tracked.about,
            "run": self.run,
            "changes": tracked.changes,
            "signs_seen": tracked.signs_seen,
            "last_active_unix_ms": tracked.last_active_unix_ms,
            "read_unix_ms": tracked.read_unix_ms,
        }))
    }
}

/// A state or a sign as the bus spells it (`needs_input`, `tests_pass`).
fn named(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// "task 7", "tasks 7, 8", or "no task".
fn tasks_named(ids: &[u64]) -> String {
    let list: Vec<String> = ids.iter().map(u64::to_string).collect();
    match ids.len() {
        0 => "no task".into(),
        1 => format!("task {}", list[0]),
        _ => format!("tasks {}", list.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signs(text: &str) -> Vec<Sign> {
        signs_in(text).into_iter().collect()
    }

    #[test]
    fn each_sign_fires_on_its_words_and_not_on_their_denial_or_promise() {
        use Sign::*;
        for (text, expected) in [
            ("All 42 tests pass now.", vec![TestsPass]),
            ("Ran cargo test: 118 passed; 0 failed", vec![TestsPass]),
            (
                "The build succeeded after the fix to config.rs",
                vec![BuildWorks],
            ),
            ("It compiles cleanly on Linux", vec![BuildWorks]),
            ("Fixed the off-by-one in the parser", vec![Fixed]),
            ("Found the root cause: a stale cache", vec![Found]),
            ("Implemented the retry and wrote the docs", vec![Done]),
            (
                "Opened a PR: https://github.com/o/r/pull/12",
                vec![PullRequest],
            ),
            ("Created a pull request for review", vec![PullRequest]),
            ("PR #731 is up", vec![PullRequest]),
            (
                "Tests now pass / build is green",
                vec![TestsPass, BuildWorks],
            ),
            // Denied, hoped for, still to come, or asked.
            ("The tests do not pass yet", vec![]),
            ("Not fixed: the race is still there", vec![]),
            ("Once the build succeeds I'll open a pull request", vec![]),
            ("I need to fix the parser", vec![]),
            ("Is it fixed?", vec![]),
            ("Reading src/main.rs and the tests", vec![]),
            ("Will be done in a minute", vec![]),
            // A curly apostrophe is a straight one.
            ("It isn\u{2019}t fixed", vec![]),
        ] {
            assert_eq!(signs(text), expected, "{text:?}");
        }
        // Words, not parts of them.
        assert_eq!(signs("refound unfixed abandoned"), vec![]);
    }

    #[test]
    fn how_much_of_a_line_is_new() {
        assert_eq!(new_share(None, "Reading the parser"), 1.0);
        assert_eq!(
            new_share(Some("Reading the parser"), "reading THE parser"),
            0.0
        );
        assert_eq!(new_share(Some("a b c d"), "a b e f"), 0.5);
        assert_eq!(new_share(Some("a b"), "  "), 0.0);
    }

    fn watched(task_id: u64, machine: &str, agent: &str, age: Option<u64>) -> Watched {
        Watched {
            task_id,
            machine_id: machine.into(),
            agent_id: agent.into(),
            engine: "claude".into(),
            accepted_ago_ms: age,
        }
    }
    fn listed(agent: &str, state: &str, about: Option<&str>) -> Listed {
        Listed {
            agent_id: agent.into(),
            state: state.into(),
            last_active_unix_ms: Some(1_791_964_800_000),
            about: about.map(str::to_owned),
        }
    }
    const RUN: u64 = 1_791_964_000_000;
    const A: &str = "11111111-1111-4111-8111-111111111111";
    const B: &str = "22222222-2222-4222-8222-222222222222";
    const OFFICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HOME: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn a_task_new_to_the_watch_is_read_at_once_then_on_the_watchs_schedule() {
        let mut tracker = Tracker::started(RUN);
        // Handed over just now: read at once.
        let young = vec![watched(1, OFFICE, A, Some(1_000))];
        assert_eq!(tracker.due(&young, 1_000).as_deref(), Some(OFFICE));
        tracker.asked(OFFICE, &young, 1_000);
        // Then every half minute while it is in its first ten minutes,
        // counted from when the machine was last asked.
        assert_eq!(tracker.due(&young, 1_000 + WATCH_EARLY_MS - 1), None);
        let asked = 1_000 + WATCH_EARLY_MS;
        assert_eq!(tracker.due(&young, asked).as_deref(), Some(OFFICE));
        tracker.asked(OFFICE, &young, asked);
        // A second task on that machine is read at once, and the two are
        // one read.
        let two = vec![
            watched(1, OFFICE, A, Some(WATCH_EARLY_MS + 6_000)),
            watched(2, OFFICE, B, Some(1_000)),
        ];
        assert_eq!(tracker.due(&two, asked + 5_000).as_deref(), Some(OFFICE));
        let asked = asked + 5_000;
        tracker.asked(OFFICE, &two, asked);
        assert_eq!(tracker.due(&two, asked + 1), None);
        // After ten minutes, every two.
        let old = vec![
            watched(1, OFFICE, A, Some(WATCH_EARLY_FOR_MS)),
            watched(2, OFFICE, B, None),
        ];
        assert_eq!(tracker.due(&old, asked + WATCH_EARLY_MS), None);
        assert_eq!(
            tracker.due(&old, asked + WATCH_LATER_MS).as_deref(),
            Some(OFFICE)
        );
        // A machine with a task new to the watch goes first; then the one
        // asked longest ago.
        let both = vec![
            watched(1, OFFICE, A, None),
            watched(3, HOME, B, Some(1_000)),
        ];
        let now = asked + WATCH_LATER_MS;
        assert_eq!(tracker.due(&both, now).as_deref(), Some(HOME));
        tracker.asked(HOME, &both, now);
        assert_eq!(tracker.due(&both, now).as_deref(), Some(OFFICE));
        // Nothing watched: nothing read.
        assert_eq!(tracker.due(&[], u64::MAX), None);

        // The tasks there when the service starts are read at once, even
        // those handed over in an earlier boot.
        let mut restarted = Tracker::started(RUN);
        let earlier = vec![watched(5, OFFICE, A, None)];
        assert_eq!(restarted.due(&earlier, 0).as_deref(), Some(OFFICE));
        restarted.asked(OFFICE, &earlier, 0);
        // A failed read waits its turn like any other.
        let _ = restarted.read(OFFICE, Err("timeout".into()), &earlier, 1, 10_000);
        assert_eq!(restarted.due(&earlier, 10_000), None);
        assert_eq!(
            restarted.due(&earlier, WATCH_LATER_MS).as_deref(),
            Some(OFFICE)
        );
    }

    #[test]
    fn a_value_at_start_and_whenever_a_task_on_the_bus_leaves_the_watch() {
        let store = "0".repeat(32);
        let mut tracker = Tracker::started(RUN);
        // At start, before anything is watched: a value with no task.
        let start = tracker.take(&store).unwrap();
        start.check().unwrap();
        assert_eq!(
            (start.store_id.as_str(), start.tasks.len()),
            (store.as_str(), 0)
        );
        assert!(tracker.take(&store).is_none());
        // A task that joins the watch is on the bus as unread (#793
        // review), and leaves it as it leaves the watch, read or not.
        tracker.watching(&[watched(1, OFFICE, A, Some(0))]);
        assert_eq!(tracker.take(&store).unwrap().unread, vec![1]);
        tracker.watching(&[]);
        let left = tracker.take(&store).unwrap();
        assert!(left.tasks.is_empty() && left.unread.is_empty());
        let one = vec![watched(2, OFFICE, A, Some(0))];
        tracker.watching(&one);
        assert_eq!(tracker.take(&store).unwrap().unread, vec![2]);
        let _ = tracker.read(OFFICE, Ok(vec![listed(A, "working", None)]), &one, 1, 1);
        let read = tracker.take(&store).unwrap();
        assert_eq!((read.tasks.len(), read.unread.len()), (1, 0));
        tracker.watching(&one);
        assert!(tracker.take(&store).is_none());
        tracker.watching(&[]);
        assert!(tracker.take(&store).unwrap().tasks.is_empty());
        assert!(tracker.take(&store).is_none());
    }

    /// #1021: a task let go because its agent settled is published apart,
    /// with its settled state, while the engine still finds it settled, and
    /// leaves when it does not; a value only when the list changes.
    #[test]
    fn a_task_let_go_because_it_settled_is_published_apart_while_it_is_recent() {
        let store = "0123456789abcdef0123456789abcdef".to_owned();
        let mut tracker = Tracker::started(1_000);
        let _ = tracker.take(&store);
        let found = |task_id: u64, state: &str, ago_ms: u64| Settled {
            task_id,
            engine: "codex".into(),
            state: state.into(),
            ago_ms,
        };
        tracker.settled(&[found(7, "idle", 2_000)], 1_000_000);
        let status = tracker.take(&store).unwrap();
        status.check().unwrap();
        assert_eq!(
            status.settled,
            [TaskSettled {
                task: 7,
                engine: "codex".into(),
                state: AgentState::Idle,
                settled: true,
                at_unix_ms: 998_000,
            }]
        );
        // The same again, later: nothing new to publish, and its moment kept.
        tracker.settled(&[found(7, "idle", 3_000)], 1_001_000);
        assert!(tracker.take(&store).is_none());
        // Another, which asks; one still watched, and a state that is not
        // settled, are left out.
        tracker.watching(&[watched(9, "office", "a9", Some(1))]);
        let _ = tracker.take(&store);
        tracker.settled(
            &[
                found(7, "idle", 4_000),
                found(8, "needs-input", 10),
                found(9, "failed", 10),
                found(10, "working", 10),
            ],
            1_002_000,
        );
        let status = tracker.take(&store).unwrap();
        status.check().unwrap();
        let said: Vec<(u64, AgentState, u64)> = status
            .settled
            .iter()
            .map(|settled| (settled.task, settled.state, settled.at_unix_ms))
            .collect();
        assert_eq!(
            said,
            [
                (7, AgentState::Idle, 998_000),
                (8, AgentState::NeedsInput, 1_001_990)
            ]
        );
        // Out of the engine's window: gone, and published so.
        tracker.settled(&[], 1_200_000);
        assert!(tracker.take(&store).unwrap().settled.is_empty());
    }

    /// The reviewer on #793 (16:46): a task watched and not yet read was
    /// published as nothing at all, so `tasks: []` could mean work going
    /// on. It is `unread` until a read of it comes back, and a failed read
    /// keeps it so.
    #[test]
    fn a_watched_task_not_yet_read_is_published_as_unread() {
        let store = "0".repeat(32);
        let mut tracker = Tracker::started(RUN);
        let one = vec![watched(4, OFFICE, A, Some(0))];
        tracker.watching(&one);
        let waiting = tracker.take(&store).unwrap();
        waiting.check().unwrap();
        assert_eq!((waiting.tasks.len(), waiting.unread.clone()), (0, vec![4]));
        assert!(tracker.unread(4) && !tracker.unread(5));
        // A failed read changes nothing: still unread, nothing new to say.
        let _ = tracker.read(OFFICE, Err("timeout".into()), &one, 1, 10_000);
        assert!(tracker.take(&store).is_none());
        assert!(tracker.unread(4));
        // Read: among the tasks, no longer unread.
        let _ = tracker.read(OFFICE, Ok(vec![listed(A, "working", None)]), &one, 2, 1);
        let read = tracker.take(&store).unwrap();
        assert_eq!((read.tasks.len(), read.unread.len()), (1, 0));
        assert!(!tracker.unread(4));
        // Another joins: published at once, unread beside the one read.
        let two = vec![
            watched(4, OFFICE, A, Some(0)),
            watched(5, OFFICE, B, Some(0)),
        ];
        tracker.watching(&two);
        let both = tracker.take(&store).unwrap();
        both.check().unwrap();
        assert_eq!((both.tasks.len(), both.unread.clone()), (1, vec![5]));
        // Both leave the watch: gone from both.
        tracker.watching(&[]);
        let none = tracker.take(&store).unwrap();
        assert!(none.tasks.is_empty() && none.unread.is_empty());
    }

    #[test]
    fn each_read_is_one_journal_line_in_typed_words_only() {
        let mut tracker = Tracker::started(RUN);
        let watched = vec![
            watched(1, OFFICE, A, Some(0)),
            watched(2, OFFICE, B, Some(0)),
            watched(3, HOME, A, Some(0)),
        ];
        let line = tracker.read(
            OFFICE,
            Ok(vec![
                listed(A, "working", Some("Reading the brief")),
                listed("33333333-3333-4333-8333-333333333333", "idle", None),
            ]),
            &watched,
            1,
            412,
        );
        assert_eq!(
            line,
            format!(
                "progress read of {OFFICE} in 412 ms: 2 listed; task 1 working, first read; \
                 task 2 missing, first read"
            )
        );
        let line = tracker.read(
            OFFICE,
            Ok(vec![
                listed(A, "working", Some("Fixed it; all tests pass")),
                listed(B, "needs-input", None),
            ]),
            &watched,
            2,
            380,
        );
        assert_eq!(
            line,
            format!(
                "progress read of {OFFICE} in 380 ms: 2 listed; task 1 working, new line, \
                 tests_pass fixed; task 2 needs_input, same line"
            )
        );
        let line = tracker.read(
            OFFICE,
            Ok(vec![listed(A, "working", Some("Now the docs"))]),
            &watched,
            3,
            390,
        );
        assert!(
            line.contains("task 1 working, new line; task 2 missing, same line"),
            "{line}"
        );
        let line = tracker.read(HOME, Err("timeout".into()), &watched, 4, 10_000);
        assert_eq!(
            line,
            format!("progress read of {HOME} failed after 10000 ms (timeout); task 3 as before")
        );
        let line = tracker.read(HOME, Ok(vec![]), &[], 5, 300);
        assert_eq!(
            line,
            format!("progress read of {HOME} in 300 ms: 0 listed; no task watched there now")
        );
        let line = tracker.read(OFFICE, Err("busy".into()), &watched[..2], 6, 1);
        assert!(line.ends_with("(busy); tasks 1, 2 as before"), "{line}");
    }

    #[test]
    fn a_change_of_the_line_is_counted_measured_and_signed_and_the_words_stay_here() {
        let mut tracker = Tracker::started(RUN);
        let watched = vec![watched(3, OFFICE, A, Some(60_000))];
        // The first line read is where counting begins.
        let _ = tracker.read(
            OFFICE,
            Ok(vec![listed(
                A,
                "working",
                Some("Reading the parser; found the cause"),
            )]),
            &watched,
            1_000,
            900,
        );
        let first = tracker.take(&"0".repeat(32)).unwrap();
        first.check().unwrap();
        assert_eq!(first.run, RUN);
        let task = &first.tasks[0];
        assert_eq!(
            (task.task, task.state, task.changes, task.changed),
            (3, AgentState::Working, 0, false)
        );
        assert_eq!(task.signs_seen, vec![Sign::Found]);
        assert!(task.signs.is_empty());
        assert_eq!(task.working_s, Some(60));
        // Nothing newer to publish until the next read.
        assert!(tracker.take(&"0".repeat(32)).is_none());
        // The same line again: read, not changed.
        let _ = tracker.read(
            OFFICE,
            Ok(vec![listed(
                A,
                "working",
                Some("Reading the parser; found the cause"),
            )]),
            &watched,
            2_000,
            800,
        );
        let same = tracker.take(&"0".repeat(32)).unwrap();
        assert_eq!(
            (
                same.tasks[0].changes,
                same.tasks[0].changed,
                same.tasks[0].read_unix_ms
            ),
            (0, false, 2_000)
        );
        // A new line: counted, how much is new, and the signs new in it.
        let _ = tracker.read(
            OFFICE,
            Ok(vec![listed(A, "working", Some("Fixed it; all tests pass"))]),
            &watched,
            3_000,
            700,
        );
        let changed = tracker.take(&"0".repeat(32)).unwrap();
        changed.check().unwrap();
        let task = &changed.tasks[0];
        assert_eq!((task.changes, task.changed), (1, true));
        assert_eq!(task.signs, vec![Sign::TestsPass, Sign::Fixed]);
        assert_eq!(
            task.signs_seen,
            vec![Sign::TestsPass, Sign::Fixed, Sign::Found]
        );
        assert!(task.new_share > 0.8, "{}", task.new_share);
        // No words on the bus.
        let text = serde_json::to_string(&changed).unwrap();
        assert!(
            !text.contains("tests pass") && !text.contains("Fixed"),
            "{text}"
        );
        // The words, here, for the lamp about to say them.
        let words = tracker.words(3).unwrap();
        assert_eq!(words["run"], RUN);
        assert_eq!(words["about"], "Fixed it; all tests pass");
        assert_eq!(words["state"], "working");
        assert!(tracker.words(4).is_none());
        // A failed read changes nothing and is counted.
        let _ = tracker.read(OFFICE, Err("timeout".into()), &watched, 4_000, 10_000);
        assert!(tracker.take(&"0".repeat(32)).is_none());
        assert_eq!(
            tracker.reads(),
            Reads {
                reads: 3,
                failures: 1,
                last_ms: 10_000,
                most_ms: 10_000
            }
        );
        // Not listed any more: missing, and its last line is kept.
        let _ = tracker.read(OFFICE, Ok(vec![]), &watched, 5_000, 600);
        let gone = tracker.take(&"0".repeat(32)).unwrap();
        assert_eq!(
            (
                gone.tasks[0].state,
                gone.tasks[0].changes,
                gone.tasks[0].changed
            ),
            (AgentState::Missing, 1, false)
        );
        // No longer watched: let go, and that is published.
        assert_eq!(tracker.due(&[], 0), None);
        assert!(tracker.take(&"0".repeat(32)).unwrap().tasks.is_empty());
        assert!(tracker.words(3).is_none());
    }

    #[test]
    fn a_read_of_one_machine_flags_only_its_own_tasks_as_changed() {
        let mut tracker = Tracker::started(RUN);
        let watched = vec![watched(1, OFFICE, A, None), watched(2, HOME, B, None)];
        for (machine, agent, about) in [(OFFICE, A, "one"), (HOME, B, "two")] {
            let _ = tracker.read(
                machine,
                Ok(vec![listed(agent, "working", Some(about))]),
                &watched,
                1,
                1,
            );
        }
        let _ = tracker.read(
            OFFICE,
            Ok(vec![listed(A, "working", Some("one more"))]),
            &watched,
            2,
            1,
        );
        let _ = tracker.read(
            HOME,
            Ok(vec![listed(B, "working", Some("two more"))]),
            &watched,
            3,
            1,
        );
        let status = tracker.take(&"0".repeat(32)).unwrap();
        assert_eq!(
            status
                .tasks
                .iter()
                .map(|t| (t.task, t.changes, t.changed))
                .collect::<Vec<_>>(),
            vec![(1, 1, false), (2, 1, true)]
        );
        // A state Harness may add later is not guessed.
        assert_eq!(agent_state("thinking"), AgentState::Unknown);
        assert_eq!(agent_state("needs-input"), AgentState::NeedsInput);
    }
}
