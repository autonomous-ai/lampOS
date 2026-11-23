//! The runs handed over on one session, and how the gateway's events move
//! them: pure state, no I/O, so every ordering is testable on its own.
//!
//! A run is known only by its own id: `chat.send`'s `runId` is always our
//! idempotency key, so a frame of any other id (the gateway's own runs, a
//! heartbeat, a boot-time skill review) is not ours and is ignored.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use super::frames::{AgentEvt, ChatEvt, HistMsg, History};
use crate::{Outcome, Phase, Progress, RunId, ToolStep, TurnKind};

/// Most characters of a tool's argument summary.
pub(crate) const BRIEF_MAX: usize = 120;
/// Most characters of the written tail kept for "it's ...".
pub(crate) const TAIL_MAX: usize = 400;
/// How long a run whose `lifecycle end` came waits for its `chat final`
/// (the whole text) before it is answered with what it has.
pub(crate) const FINAL_GRACE: Duration = Duration::from_millis(1500);
/// Ended runs kept, so late frames for them are known as late.
const KEEP_ENDED: usize = 64;

#[derive(Clone, Debug, PartialEq)]
enum Closed {
    Answered(String),
    Failed(String),
    Aborted,
}

/// The assistant text of a run: finished items, and the one being written.
#[derive(Clone, Debug, Default)]
struct Text {
    items: Vec<String>,
    cur: String,
    /// The `itemId` being written: a new one starts a new item.
    item: Option<String>,
}

impl Text {
    fn commit(&mut self) {
        if !self.cur.trim().is_empty() {
            self.items.push(std::mem::take(&mut self.cur));
        }
        self.cur.clear();
        self.item = None;
    }

    /// Every item, joined as the gateway joins them in `chat final`.
    fn whole(&self) -> String {
        let mut parts: Vec<&str> = self.items.iter().map(|s| s.trim()).collect();
        if !self.cur.trim().is_empty() {
            parts.push(self.cur.trim());
        }
        parts.join("\n\n")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub id: RunId,
    #[allow(dead_code)] // kept with the run for the journal
    pub message: String,
    #[allow(dead_code)] // kept with the run for progress and the journal
    pub kind: TurnKind,
    p: Progress,
    text: Text,
    /// Text from the `chat` stream, used only when no `agent` assistant
    /// text came.
    chat_text: String,
    agent_text_seen: bool,
    closed: Option<Closed>,
    end_seen: Option<Instant>,
    /// `lifecycle end`'s `terminalReply.text`: the answer if no `chat
    /// final` follows.
    terminal_reply: Option<String>,
    /// A message the gateway folded into another run (an empty `final`
    /// before it ever started): its answer is that run's.
    merged_into: Option<RunId>,
    started: bool,
    tools_counted: HashSet<String>,
    /// The run's own token count, from its `usage` events.
    tokens: u64,
}

impl Run {
    fn answer(&self) -> String {
        if let Some(reply) = self
            .terminal_reply
            .as_deref()
            .filter(|t| !t.trim().is_empty())
        {
            return reply.trim().to_owned();
        }
        let whole = self.text.whole();
        if !whole.is_empty() || self.agent_text_seen {
            return whole;
        }
        self.chat_text.trim().to_owned()
    }

    fn touch(&mut self) {
        self.p.updated_at = SystemTime::now();
    }

    fn refresh_tail(&mut self) {
        let whole = self.text.whole();
        let text = if whole.is_empty() && !self.agent_text_seen {
            self.chat_text.trim().to_owned()
        } else {
            whole
        };
        self.p.last_text = (!text.is_empty()).then(|| tail(&text, TAIL_MAX));
    }

    fn open(&self) -> bool {
        self.closed.is_none() && self.end_seen.is_none() && self.merged_into.is_none()
    }
}

#[derive(Debug, Default)]
pub(crate) struct Runs {
    list: Vec<Run>,
    by_id: HashMap<String, RunId>,
}

impl Runs {
    pub(crate) fn register(&mut self, id: RunId, task_id: &str, message: &str, kind: TurnKind) {
        let now = SystemTime::now();
        self.by_id.insert(id.0.clone(), id.clone());
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
            message: message.to_owned(),
            kind,
            text: Text::default(),
            chat_text: String::new(),
            agent_text_seen: false,
            closed: None,
            end_seen: None,
            terminal_reply: None,
            merged_into: None,
            started: false,
            tools_counted: HashSet::new(),
            tokens: 0,
        });
    }

    pub(crate) fn forget(&mut self, id: &RunId) {
        self.list.retain(|r| &r.id != id);
        self.by_id.retain(|_, r| r != id);
    }

    pub(crate) fn contains(&self, id: &RunId) -> bool {
        self.get(id).is_some()
    }

    fn get(&self, id: &RunId) -> Option<&Run> {
        self.list.iter().find(|r| &r.id == id)
    }

    fn get_mut(&mut self, id: &RunId) -> Option<&mut Run> {
        self.list.iter_mut().find(|r| &r.id == id)
    }

    /// The run going now in the session, other than `except`.
    fn active(&self, except: &RunId) -> Option<RunId> {
        self.list
            .iter()
            .rev()
            .find(|r| &r.id != except && r.started && r.open())
            .map(|r| r.id.clone())
    }

    /// One `agent` event; a frame of a run not ours changes nothing.
    pub(crate) fn on_agent(&mut self, evt: AgentEvt) {
        let Some(id) = self.by_id.get(&evt.run_id).cloned() else {
            return;
        };
        let Some(run) = self.get_mut(&id) else {
            return;
        };
        // A run that ended stays ended: late or duplicate frames for it
        // (a second `end`, an `error` after `end`) change nothing.
        if run.closed.is_some() || run.end_seen.is_some() || run.merged_into.is_some() {
            return;
        }
        run.touch();
        match evt.stream.as_str() {
            "lifecycle" => match evt.phase() {
                "start" | "model" => {
                    run.started = true;
                    if run.p.phase == Phase::Queued {
                        run.p.phase = Phase::Thinking;
                    }
                }
                "end" => {
                    run.started = true;
                    if evt.data.get("aborted").and_then(Value::as_bool) == Some(true) {
                        close(run, Closed::Aborted);
                        return;
                    }
                    // `chat final` follows with the whole text: wait for it
                    // (a short grace), with `terminalReply` as the fallback.
                    run.p.phase = Phase::Done;
                    run.terminal_reply = evt
                        .data
                        .get("terminalReply")
                        .and_then(|r| r.get("text"))
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    run.end_seen = Some(Instant::now());
                }
                "error" => {
                    let why = evt
                        .str("error")
                        .unwrap_or_else(|| "the run failed".to_owned());
                    close(run, Closed::Failed(why));
                }
                _ => {}
            },
            "assistant" => {
                run.agent_text_seen = true;
                run.started = true;
                let item = evt.str("itemId");
                if item.is_some() && run.text.item.is_some() && item != run.text.item {
                    run.text.commit();
                }
                if item.is_some() {
                    run.text.item = item;
                }
                let delta = evt.str("delta");
                if let Some(text) = evt.str("text") {
                    run.text.cur = text;
                } else if evt.data.get("replace").and_then(Value::as_bool) == Some(true) {
                    run.text.cur = delta.unwrap_or_default();
                } else if let Some(delta) = delta {
                    run.text.cur.push_str(&delta);
                }
                if run.p.phase != Phase::Tool {
                    run.p.phase = Phase::Writing;
                }
                run.refresh_tail();
            }
            "tool" => {
                run.started = true;
                let name = evt
                    .str("name")
                    .or_else(|| evt.str("tool"))
                    .unwrap_or_else(|| "tool".to_owned());
                let call = evt.str("toolCallId").unwrap_or_else(|| name.clone());
                match evt.phase() {
                    "start" => {
                        run.text.commit();
                        let args = evt.data.get("args").cloned().unwrap_or(Value::Null);
                        run.p.last_tool = Some(ToolStep {
                            name,
                            brief: brief(&args),
                            running: true,
                        });
                        run.p.phase = Phase::Tool;
                    }
                    "result" | "end" => {
                        if run.tools_counted.insert(call) {
                            run.p.tools_done += 1;
                        }
                        if let Some(step) = run.p.last_tool.as_mut() {
                            step.running = false;
                        }
                        run.p.phase = Phase::Thinking;
                    }
                    _ => {}
                }
            }
            "thinking" => {
                run.started = true;
                if matches!(run.p.phase, Phase::Queued | Phase::Writing) {
                    run.p.phase = Phase::Thinking;
                }
            }
            "plan" => run.p.plan = super::frames::plan(&evt.data),
            "usage" => {
                let n = |k: &str| evt.data.get(k).and_then(Value::as_u64).unwrap_or(0);
                let total = match evt.data.get("totalTokens").and_then(Value::as_u64) {
                    Some(t) => t,
                    None => n("inputTokens") + n("outputTokens"),
                };
                // Counts within a run only grow.
                run.tokens = run.tokens.max(total);
            }
            // run_status, usage, item, command_output, and the run-local
            // `error` stream (`seq gap`) say nothing lamp-tasks keeps.
            _ => {}
        }
    }

    pub(crate) fn on_chat(&mut self, evt: ChatEvt) {
        let Some(id) = self.by_id.get(&evt.run_id).cloned() else {
            return;
        };
        let active = self.active(&id);
        let Some(run) = self.get_mut(&id) else {
            return;
        };
        if run.closed.is_some() || run.merged_into.is_some() {
            return;
        }
        run.touch();
        match evt.state.as_str() {
            "delta" => {
                if let Some(snapshot) = evt.snapshot {
                    run.chat_text = snapshot;
                } else if evt.replace {
                    run.chat_text = evt.delta.unwrap_or_default();
                } else if let Some(delta) = evt.delta {
                    run.chat_text.push_str(&delta);
                }
                if run.end_seen.is_none() {
                    run.refresh_tail();
                }
            }
            "final" => {
                let snapshot = evt.snapshot.filter(|s| !s.trim().is_empty());
                // An empty final for a run that never started is a message
                // the gateway folded into the active run: its answer is
                // that run's, never "".
                if snapshot.is_none() && !run.started {
                    if let Some(into) = active {
                        run.merged_into = Some(into);
                        run.started = true;
                    }
                    return;
                }
                run.p.phase = Phase::Done;
                let answer = match snapshot {
                    Some(text) => text.trim().to_owned(),
                    None => run.answer(),
                };
                close(run, Closed::Answered(answer));
            }
            "aborted" => close(run, Closed::Aborted),
            "error" if run.end_seen.is_none() => {
                let why = evt.error.unwrap_or_else(|| "the run failed".to_owned());
                close(run, Closed::Failed(why));
            }
            _ => {}
        }
        self.prune();
    }

    /// How `id` ended, or `None` while it goes on.
    pub(crate) fn outcome(&self, id: &RunId) -> Option<Outcome> {
        let run = self.get(id)?;
        if run.closed.is_none()
            && let Some(into) = &run.merged_into
        {
            return self.outcome(into).map(|o| {
                let progress = self.progress(id).expect("a known run");
                match o {
                    Outcome::Answered { text, .. } => Outcome::Answered { text, progress },
                    Outcome::Failed { why, .. } => Outcome::Failed { why, progress },
                    Outcome::Aborted { .. } => Outcome::Aborted { progress },
                }
            });
        }
        let progress = run.p.clone();
        match &run.closed {
            Some(Closed::Answered(text)) => Some(Outcome::Answered {
                text: text.clone(),
                progress,
            }),
            Some(Closed::Failed(why)) => Some(Outcome::Failed {
                why: why.clone(),
                progress,
            }),
            Some(Closed::Aborted) => Some(Outcome::Aborted { progress }),
            None => run
                .end_seen
                .filter(|at| at.elapsed() >= FINAL_GRACE)
                .map(|_| Outcome::Answered {
                    text: run.answer(),
                    progress,
                }),
        }
    }

    pub(crate) fn progress(&self, id: &RunId) -> Option<Progress> {
        let run = self.get(id)?;
        if let Some(into) = &run.merged_into
            && run.closed.is_none()
            && let Some(target) = self.progress(into)
        {
            return Some(Progress {
                run: run.id.clone(),
                task_id: run.p.task_id.clone(),
                started_at: run.p.started_at,
                ..target
            });
        }
        Some(run.p.clone())
    }

    pub(crate) fn running(&self) -> Vec<Progress> {
        self.list
            .iter()
            .filter(|r| self.outcome(&r.id).is_none())
            .filter_map(|r| self.progress(&r.id))
            .collect()
    }

    /// Runs not ended: what a resync looks for.
    pub(crate) fn open(&self) -> Vec<RunId> {
        self.list
            .iter()
            .filter(|r| r.open())
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

    /// A drop (a gateway restart, a crash, a lost socket): every open run
    /// ends failed at once with [`super::io::DROPPED`], the person told to
    /// try again; the ones ended.
    pub(crate) fn fail_open(&mut self) -> Vec<RunId> {
        let open = self.open();
        for id in &open {
            self.fail(id, super::io::DROPPED);
        }
        open
    }

    /// Whether `run_id` is a stray: not one of this session's runs, or one
    /// the drop failed ([`Self::fail_open`]) that the gateway's restart
    /// recovery runs again. Ours still going, or ended otherwise (a late
    /// event after its final), is not.
    pub(crate) fn stray(&self, run_id: &str) -> bool {
        match self.by_id.get(run_id).and_then(|id| self.get(id)) {
            None => true,
            Some(run) => matches!(&run.closed,
                Some(Closed::Failed(why)) if why == super::io::DROPPED),
        }
    }

    /// Ends `id` as failed.
    pub(crate) fn fail(&mut self, id: &RunId, why: &str) {
        if let Some(run) = self.get_mut(id)
            && run.closed.is_none()
        {
            close(run, Closed::Failed(why.to_owned()));
        }
        self.prune();
    }

    /// Tokens of the runs this session has seen.
    pub(crate) fn tokens(&self) -> u64 {
        self.list.iter().map(|r| r.tokens).sum()
    }

    /// A fresh conversation: no tokens counted yet.
    pub(crate) fn zero_tokens(&mut self) {
        for run in &mut self.list {
            run.tokens = 0;
        }
    }

    pub(crate) fn abort_all(&mut self) {
        for id in self.open() {
            self.abort(&id);
        }
    }

    /// After a reconnect: brings each open run up to `history`. A run the
    /// gateway still runs is refreshed; one it no longer runs ended while
    /// away and is closed from the transcript (found by its user message's
    /// `idempotencyKey`). Returns the runs closed.
    pub(crate) fn resync(&mut self, history: &History) -> Vec<RunId> {
        self.resync_with(history, true)
    }

    fn resync_with(&mut self, history: &History, fail_missing: bool) -> Vec<RunId> {
        let mut closed = Vec::new();
        for id in self.open() {
            let Some(run) = self.get_mut(&id) else {
                continue;
            };
            let asked = history
                .messages
                .iter()
                .rposition(|m| m.role == "user" && m.idem.as_deref() == Some(run.id.0.as_str()));
            let replies: Vec<&HistMsg> = asked
                .map(|at| {
                    history.messages[at + 1..]
                        .iter()
                        .take_while(|m| m.role != "user")
                        .filter(|m| m.role == "assistant")
                        .collect()
                })
                .unwrap_or_default();
            let answer: Vec<&str> = replies
                .iter()
                .map(|m| m.text.trim())
                .filter(|t| !t.is_empty())
                .collect();
            let finished = !answer.is_empty() && replies.last().is_some_and(|m| !m.calls_tool);
            let in_flight = history.in_flight.as_ref().filter(|f| f.run_id == run.id.0);
            // Still running? `sessionInfo.activeRunIds`, when the gateway
            // gives it, is the exact set. Without it, `inFlightRun` counts
            // unless it is the empty entry that lingers a moment after
            // `final` while the transcript already holds the answer.
            let running = match &history.active_ids {
                Some(ids) => ids.iter().any(|r| r == &run.id.0),
                None => in_flight.is_some_and(|f| !(f.text.trim().is_empty() && finished)),
            };
            run.touch();
            if running {
                run.started = true;
                if let Some(f) = in_flight {
                    if !f.text.trim().is_empty() && run.text.whole().is_empty() {
                        run.text.cur = f.text.clone();
                        run.refresh_tail();
                    }
                    if !f.plan.is_empty() {
                        run.p.plan = f.plan.clone();
                    }
                }
                if run.p.phase == Phase::Queued {
                    run.p.phase = Phase::Thinking;
                }
                continue;
            }
            if asked.is_some() || run.started {
                if finished {
                    run.p.phase = Phase::Done;
                    close(run, Closed::Answered(answer.join("\n\n")));
                } else if !fail_missing {
                    continue;
                } else {
                    let why = "the run ended while the gateway was away, with no answer";
                    close(run, Closed::Failed(why.to_owned()));
                }
                closed.push(id);
            }
            // Not started and not in the transcript: still queued.
        }
        self.prune();
        closed
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
        for id in &ended[..ended.len() - KEEP_ENDED] {
            self.forget(id);
        }
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
    run.end_seen = None;
    if let Closed::Answered(text) = &how {
        run.p.last_text = (!text.is_empty()).then(|| tail(text, TAIL_MAX));
    } else {
        run.refresh_tail();
    }
    run.closed = Some(how);
}

/// The last `max` characters of `s`, on a character boundary.
pub(crate) fn tail(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_owned();
    }
    s.chars().skip(n - max).collect()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

fn secret_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "passwd",
        "apikey",
        "api_key",
        "auth",
        "credential",
        "cookie",
        "private",
    ]
    .iter()
    .any(|s| k.contains(s))
        || k == "key"
        || k.ends_with("key")
        || k.ends_with("_key")
}

/// Secrets inside free text: `token=…`, `--password …`, `Bearer …`.
fn redact_words(s: &str) -> String {
    let mut out = Vec::new();
    let mut hide_next = false;
    for word in s.split_whitespace() {
        if hide_next {
            out.push("***".to_owned());
            hide_next = false;
            continue;
        }
        if let Some((k, _)) = word.split_once('=')
            && secret_key(k.trim_start_matches('-'))
        {
            out.push(format!("{k}=***"));
            continue;
        }
        if let Some(flag) = word.strip_prefix("--")
            && secret_key(flag)
        {
            hide_next = true;
        }
        if word
            .trim_matches(|c: char| c == '\'' || c == '"')
            .eq_ignore_ascii_case("bearer")
        {
            hide_next = true;
        }
        out.push(word.to_owned());
    }
    out.join(" ")
}

/// A short, safe summary of a tool's arguments (at most [`BRIEF_MAX`]
/// characters, no secret values).
pub(crate) fn brief(args: &Value) -> String {
    let parsed;
    let args = match args {
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(v @ Value::Object(_)) => {
                parsed = v;
                &parsed
            }
            _ => return clip(&redact_words(s), BRIEF_MAX),
        },
        other => other,
    };
    let text = match args {
        Value::Object(o) => o
            .iter()
            .map(|(k, v)| {
                if secret_key(k) {
                    format!("{k}=***")
                } else {
                    let v = match v {
                        Value::String(s) => redact_words(s),
                        other => redact_words(&other.to_string()),
                    };
                    format!("{k}={v}")
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        Value::Null => String::new(),
        other => redact_words(&other.to_string()),
    };
    clip(&text, BRIEF_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn brief_hides_secrets_and_is_short() {
        let b = brief(
            &json!({"command": "curl -H 'Bearer abc' --token xyz api_key=k1", "token": "t0"}),
        );
        assert!(
            !b.contains("abc") && !b.contains("xyz") && !b.contains("k1") && !b.contains("t0"),
            "{b}"
        );
        assert!(b.contains("token=***"), "{b}");
        let long = brief(&json!({"command": "x".repeat(500)}));
        assert_eq!(long.chars().count(), BRIEF_MAX);
        assert_eq!(brief(&json!("{\"password\":\"p\"}")), "password=***");
    }

    #[test]
    fn tail_keeps_the_end_on_char_boundaries() {
        let s = "é".repeat(500);
        assert_eq!(tail(&s, TAIL_MAX).chars().count(), TAIL_MAX);
        assert_eq!(tail("short", TAIL_MAX), "short");
    }
}
