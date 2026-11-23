//! The runs handed over on the kept session, and how Hermes' run events
//! (`GET /v1/runs/{id}/events`) and run status (`GET /v1/runs/{id}`) move
//! them: pure state, no I/O.
//!
//! Event names and fields are those of the pinned v0.22.2
//! `gateway/platforms/api_server_runs.py`: `message.delta {delta}`,
//! `message.interim {text, already_streamed}`, `tool.started {tool,
//! preview}`, `tool.completed {tool, duration, error, preview}`,
//! `reasoning.available {text}`, `subagent.start|complete {goal, ...}`,
//! `approval.request|responded`, `run.steered`, `replay.truncated`, and the
//! terminal `run.completed {output, usage}`, `run.failed {error}`,
//! `run.cancelled`, `run.interrupted {error}`. Each event carries `seq`.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use crate::openclaw::{TAIL_MAX, brief, tail};
use crate::{DROPPED, Outcome, Phase, Progress, RunId, ToolStep, TurnKind};

/// Ended runs kept, so late calls for them still answer.
const KEEP_ENDED: usize = 64;

/// Terminal run statuses (`api_server_run_idempotency.TERMINAL_STATUSES`).
pub(crate) const TERMINAL: [&str; 4] = ["completed", "failed", "cancelled", "interrupted"];

#[derive(Clone, Debug, PartialEq)]
enum Closed {
    Answered(String),
    Failed(String),
    Aborted,
}

/// A run's terminal `usage` (`_USAGE_FIELDS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    fn of(v: Option<&Value>) -> Option<Usage> {
        let v = v?.as_object()?;
        let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        Some(Usage {
            input_tokens: n("input_tokens"),
            output_tokens: n("output_tokens"),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub id: RunId,
    /// The hand-over's task and words: a drop's guard against handing the
    /// same turn over again ([`Runs::dropped_turn`]).
    pub turn: (String, String),
    #[allow(dead_code)] // kept with the run for the journal
    pub kind: TurnKind,
    p: Progress,
    /// What the agent has written so far (deltas, and commentary not
    /// streamed), for the tail only; the answer is `run.completed.output`.
    shown: String,
    closed: Option<Closed>,
    ended_at: Option<Instant>,
    /// The last event's `seq` (`Last-Event-ID` on a reopen).
    pub last_seq: Option<u64>,
    pub usage: Option<Usage>,
    /// Tools started and not completed (Hermes may run several at once).
    open_tools: u32,
}

impl Run {
    fn touch(&mut self) {
        self.p.updated_at = SystemTime::now();
    }

    fn started(&mut self) {
        if self.p.phase == Phase::Queued {
            self.p.phase = Phase::Thinking;
        }
    }

    fn show(&mut self, text: &str) {
        self.shown.push_str(text);
        let t = self.shown.trim();
        self.p.last_text = (!t.is_empty()).then(|| tail(t, TAIL_MAX));
    }
}

/// What one event did to its run.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Applied {
    /// Not ours, already seen (`seq`), or the run had ended.
    Nothing,
    Moved,
    /// The run ended with this event.
    Ended,
}

#[derive(Debug, Default)]
pub(crate) struct Runs {
    list: Vec<Run>,
    /// A run ended by a drop since the session last looked
    /// ([`Runs::take_dropped`]).
    dropped: bool,
}

impl Runs {
    pub(crate) fn register(&mut self, id: RunId, task_id: &str, message: &str, kind: TurnKind) {
        let now = SystemTime::now();
        self.list.push(Run {
            p: Progress {
                run: id.clone(),
                task_id: task_id.to_owned(),
                phase: Phase::Queued,
                started_at: now,
                updated_at: now,
                tools_done: 0,
                last_tool: None,
                last_text: None,
                plan: Vec::new(),
            },
            id,
            turn: (task_id.to_owned(), message.to_owned()),
            kind,
            shown: String::new(),
            closed: None,
            ended_at: None,
            last_seq: None,
            usage: None,
            open_tools: 0,
        });
    }

    pub(crate) fn contains(&self, id: &RunId) -> bool {
        self.get(id).is_some()
    }

    pub(crate) fn get(&self, id: &RunId) -> Option<&Run> {
        self.list.iter().find(|r| &r.id == id)
    }

    fn get_mut(&mut self, id: &RunId) -> Option<&mut Run> {
        self.list.iter_mut().find(|r| &r.id == id)
    }

    pub(crate) fn is_open(&self, id: &RunId) -> bool {
        self.get(id).is_some_and(|r| r.closed.is_none())
    }

    /// One event of `id`'s stream.
    pub(crate) fn on_event(&mut self, id: &RunId, evt: &Value) -> Applied {
        let Some(run) = self.get_mut(id) else {
            return Applied::Nothing;
        };
        if run.closed.is_some() {
            return Applied::Nothing;
        }
        if let Some(seq) = evt.get("seq").and_then(Value::as_u64) {
            // A reopened stream replays from `Last-Event-ID`; anything at or
            // before the last seen is a repeat.
            if run.last_seq.is_some_and(|last| seq <= last) {
                return Applied::Nothing;
            }
            run.last_seq = Some(seq);
        }
        let s = |k: &str| evt.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
        run.touch();
        let name = evt.get("event").and_then(Value::as_str).unwrap_or("");
        match name {
            "message.delta" => {
                run.started();
                run.show(&s("delta"));
                if run.open_tools == 0 {
                    run.p.phase = Phase::Writing;
                }
            }
            "message.interim" => {
                run.started();
                if evt.get("already_streamed").and_then(Value::as_bool) != Some(true) {
                    let text = s("text");
                    if !run.shown.is_empty() {
                        run.show("\n\n");
                    }
                    run.show(&text);
                }
                if run.open_tools == 0 {
                    run.p.phase = Phase::Writing;
                }
            }
            "reasoning.available" => {
                run.started();
                if run.open_tools == 0 {
                    run.p.phase = Phase::Thinking;
                }
            }
            "tool.started" | "subagent.start" => {
                run.started();
                let (tool, args) = if name == "tool.started" {
                    let tool = s("tool");
                    (
                        if tool.is_empty() {
                            "tool".to_owned()
                        } else {
                            tool
                        },
                        Value::String(s("preview")),
                    )
                } else {
                    let goal = evt.get("goal").or_else(|| evt.get("preview"));
                    ("subagent".to_owned(), goal.cloned().unwrap_or(Value::Null))
                };
                run.open_tools += 1;
                run.p.last_tool = Some(ToolStep {
                    name: tool,
                    brief: brief(&args),
                    running: true,
                });
                run.p.phase = Phase::Tool;
            }
            "tool.completed" | "subagent.complete" => {
                run.started();
                run.open_tools = run.open_tools.saturating_sub(1);
                run.p.tools_done += 1;
                if run.open_tools == 0 {
                    if let Some(step) = run.p.last_tool.as_mut() {
                        step.running = false;
                    }
                    run.p.phase = Phase::Thinking;
                }
            }
            "run.completed" => {
                run.usage = Usage::of(evt.get("usage"));
                let output = s("output");
                close(run, Closed::Answered(output.trim().to_owned()));
                return Applied::Ended;
            }
            "run.failed" => {
                let why = s("error");
                let why = if why.trim().is_empty() {
                    "the run failed".to_owned()
                } else {
                    why
                };
                close(run, Closed::Failed(why));
                return Applied::Ended;
            }
            "run.cancelled" => {
                close(run, Closed::Aborted);
                return Applied::Ended;
            }
            // The gateway's shutdown (or a restart before the run settled)
            // ended it: the same words as any drop, never run again.
            "run.interrupted" => {
                close(run, Closed::Failed(DROPPED.to_owned()));
                self.dropped = true;
                return Applied::Ended;
            }
            // approval.request|responded, run.steered, replay.truncated: a
            // sign of life only.
            _ => run.started(),
        }
        Applied::Moved
    }

    /// The run's pollable status (`GET /v1/runs/{id}`): ends it when the
    /// status is terminal. Whether it ended.
    pub(crate) fn on_status(&mut self, id: &RunId, status: &Value) -> bool {
        let Some(run) = self.get_mut(id) else {
            return false;
        };
        if run.closed.is_some() {
            return true;
        }
        let s = |k: &str| {
            status
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        match s("status").as_str() {
            "completed" => {
                run.usage = Usage::of(status.get("usage"));
                close(run, Closed::Answered(s("output").trim().to_owned()));
            }
            "failed" => {
                let why = s("error");
                close(
                    run,
                    Closed::Failed(if why.trim().is_empty() {
                        "the run failed".to_owned()
                    } else {
                        why
                    }),
                );
            }
            "cancelled" => close(run, Closed::Aborted),
            "interrupted" => {
                close(run, Closed::Failed(DROPPED.to_owned()));
                self.dropped = true;
            }
            "running" | "waiting_for_approval" | "stopping" => {
                run.started();
                return false;
            }
            _ => return false,
        }
        true
    }

    /// How `id` ended, or `None` while it goes on.
    pub(crate) fn outcome(&self, id: &RunId) -> Option<Outcome> {
        let run = self.get(id)?;
        let progress = run.p.clone();
        Some(match run.closed.as_ref()? {
            Closed::Answered(text) => Outcome::Answered {
                text: text.clone(),
                progress,
            },
            Closed::Failed(why) => Outcome::Failed {
                why: why.clone(),
                progress,
            },
            Closed::Aborted => Outcome::Aborted { progress },
        })
    }

    pub(crate) fn progress(&self, id: &RunId) -> Option<Progress> {
        self.get(id).map(|r| r.p.clone())
    }

    pub(crate) fn running(&self) -> Vec<Progress> {
        self.list
            .iter()
            .filter(|r| r.closed.is_none())
            .map(|r| r.p.clone())
            .collect()
    }

    pub(crate) fn open(&self) -> Vec<RunId> {
        self.list
            .iter()
            .filter(|r| r.closed.is_none())
            .map(|r| r.id.clone())
            .collect()
    }

    pub(crate) fn abort(&mut self, id: &RunId) {
        if let Some(run) = self.get_mut(id)
            && run.closed.is_none()
        {
            close(run, Closed::Aborted);
        }
        self.prune();
    }

    /// Ends `id` as failed (a drop: [`DROPPED`]).
    pub(crate) fn fail(&mut self, id: &RunId, why: &str) {
        if let Some(run) = self.get_mut(id)
            && run.closed.is_none()
        {
            close(run, Closed::Failed(why.to_owned()));
            self.dropped |= why == DROPPED;
        }
        self.prune();
    }

    /// Whether a run ended by a drop since the last call: the dead gateway
    /// may still hold the kept session's turn lease.
    pub(crate) fn take_dropped(&mut self) -> bool {
        std::mem::take(&mut self.dropped)
    }

    /// A run of this exact turn (task and words) that a drop ended no more
    /// than `within` ago: a hand-over of it again gets that run back, never
    /// a second one.
    pub(crate) fn dropped_turn(&self, turn: &(String, String), within: Duration) -> Option<RunId> {
        self.list
            .iter()
            .rev()
            .find(|r| {
                &r.turn == turn
                    && matches!(&r.closed, Some(Closed::Failed(w)) if w == DROPPED)
                    && r.ended_at.is_some_and(|at| at.elapsed() <= within)
            })
            .map(|r| r.id.clone())
    }

    /// Drops the oldest ended runs past [`KEEP_ENDED`].
    fn prune(&mut self) {
        let ended: Vec<RunId> = self
            .list
            .iter()
            .filter(|r| r.closed.is_some())
            .map(|r| r.id.clone())
            .collect();
        if ended.len() <= KEEP_ENDED {
            return;
        }
        let gone: HashMap<RunId, ()> = ended[..ended.len() - KEEP_ENDED]
            .iter()
            .map(|id| (id.clone(), ()))
            .collect();
        self.list.retain(|r| !gone.contains_key(&r.id));
    }
}

fn close(run: &mut Run, how: Closed) {
    run.p.phase = match &how {
        Closed::Answered(_) => Phase::Done,
        Closed::Failed(why) => Phase::Failed { why: why.clone() },
        Closed::Aborted => Phase::Aborted,
    };
    if let Some(step) = run.p.last_tool.as_mut() {
        step.running = false;
    }
    run.open_tools = 0;
    if let Closed::Answered(text) = &how
        && !text.is_empty()
    {
        run.p.last_text = Some(tail(text, TAIL_MAX));
    }
    run.ended_at = Some(Instant::now());
    run.touch();
    run.closed = Some(how);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one() -> (Runs, RunId) {
        let mut runs = Runs::default();
        let id = RunId("run_1".to_owned());
        runs.register(id.clone(), "t1", "New task t1: hi", TurnKind::Spoken);
        (runs, id)
    }

    fn ev(seq: u64, name: &str, extra: Value) -> Value {
        let mut v = json!({"event": name, "run_id": "run_1", "timestamp": 1.0, "seq": seq});
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        v
    }

    #[test]
    fn events_move_the_phase_and_end_with_the_output() {
        let (mut runs, id) = one();
        assert_eq!(runs.progress(&id).unwrap().phase, Phase::Queued);
        runs.on_event(&id, &ev(0, "reasoning.available", json!({"text": "hm"})));
        assert_eq!(runs.progress(&id).unwrap().phase, Phase::Thinking);
        runs.on_event(
            &id,
            &ev(
                1,
                "tool.started",
                json!({"tool": "terminal", "preview": "ls --token abc"}),
            ),
        );
        let p = runs.progress(&id).unwrap();
        assert_eq!(p.phase, Phase::Tool);
        let step = p.last_tool.unwrap();
        assert_eq!(step.name, "terminal");
        assert!(
            step.running && !step.brief.contains("abc"),
            "{}",
            step.brief
        );
        runs.on_event(
            &id,
            &ev(
                2,
                "tool.completed",
                json!({"tool": "terminal", "duration": 0.1, "error": false}),
            ),
        );
        let p = runs.progress(&id).unwrap();
        assert_eq!((p.phase.clone(), p.tools_done), (Phase::Thinking, 1));
        assert!(!p.last_tool.unwrap().running);
        runs.on_event(&id, &ev(3, "message.delta", json!({"delta": "Hel"})));
        runs.on_event(&id, &ev(4, "message.delta", json!({"delta": "lo"})));
        let p = runs.progress(&id).unwrap();
        assert_eq!(p.phase, Phase::Writing);
        assert_eq!(p.last_text.as_deref(), Some("Hello"));
        // A repeat after a reopen changes nothing.
        assert_eq!(
            runs.on_event(&id, &ev(4, "message.delta", json!({"delta": "lo"}))),
            Applied::Nothing
        );
        assert!(runs.outcome(&id).is_none());
        let end = ev(
            5,
            "run.completed",
            json!({"output": " Hello. ", "usage": {"input_tokens": 900, "output_tokens": 12}}),
        );
        assert_eq!(runs.on_event(&id, &end), Applied::Ended);
        match runs.outcome(&id).unwrap() {
            Outcome::Answered { text, progress } => {
                assert_eq!(text, "Hello.");
                assert_eq!(progress.phase, Phase::Done);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(runs.get(&id).unwrap().usage.unwrap().input_tokens, 900);
        assert!(runs.running().is_empty());
    }

    #[test]
    fn failed_cancelled_and_interrupted_end_as_such() {
        let (mut runs, id) = one();
        runs.on_event(&id, &ev(0, "run.failed", json!({"error": "model 500"})));
        assert!(
            matches!(runs.outcome(&id), Some(Outcome::Failed { why, .. }) if why == "model 500")
        );
        let (mut runs, id) = one();
        runs.on_event(&id, &ev(0, "run.cancelled", json!({})));
        assert!(matches!(runs.outcome(&id), Some(Outcome::Aborted { .. })));
        let (mut runs, id) = one();
        runs.on_event(
            &id,
            &ev(
                0,
                "run.interrupted",
                json!({"error": "Gateway shutdown interrupted the run."}),
            ),
        );
        assert!(matches!(runs.outcome(&id), Some(Outcome::Failed { why, .. }) if why == DROPPED));
        let turn = ("t1".to_owned(), "New task t1: hi".to_owned());
        assert!(runs.dropped_turn(&turn, Duration::from_secs(60)).is_some());
        assert!(runs.dropped_turn(&turn, Duration::ZERO).is_none());
    }

    #[test]
    fn status_reads_end_a_run_like_its_events() {
        let (mut runs, id) = one();
        assert!(!runs.on_status(&id, &json!({"status": "running"})));
        assert_eq!(runs.progress(&id).unwrap().phase, Phase::Thinking);
        assert!(runs.on_status(
            &id,
            &json!({"status": "interrupted", "error": "The gateway restarted before this run settled."})
        ));
        assert!(matches!(runs.outcome(&id), Some(Outcome::Failed { why, .. }) if why == DROPPED));
        let (mut runs, id) = one();
        assert!(runs.on_status(&id, &json!({"status": "completed", "output": "ok"})));
        assert!(matches!(runs.outcome(&id), Some(Outcome::Answered { text, .. }) if text == "ok"));
    }

    #[test]
    fn parallel_tools_stay_running_until_the_last_ends() {
        let (mut runs, id) = one();
        runs.on_event(
            &id,
            &ev(0, "tool.started", json!({"tool": "a", "preview": ""})),
        );
        runs.on_event(&id, &ev(1, "subagent.start", json!({"goal": "find it"})));
        runs.on_event(&id, &ev(2, "tool.completed", json!({"tool": "a"})));
        let p = runs.progress(&id).unwrap();
        assert_eq!((p.phase.clone(), p.tools_done), (Phase::Tool, 1));
        assert_eq!(p.last_tool.as_ref().unwrap().name, "subagent");
        runs.on_event(
            &id,
            &ev(3, "subagent.complete", json!({"status": "completed"})),
        );
        assert_eq!(runs.progress(&id).unwrap().phase, Phase::Thinking);
    }
}
