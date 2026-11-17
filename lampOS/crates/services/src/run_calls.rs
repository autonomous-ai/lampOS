//! `lamp-services --run-calls` (the linked-service memory plan, slice 4):
//! the last [`KEPT`] calls across all skill runs on the lamp's agent, read
//! or write, for the voice's `run_calls_since` (the voice's own call on the
//! tools socket, never offered to its model). One ring for every run: a
//! busy run pushes another's calls out, and a late pull is told only how
//! many it `missed`. Each call is its run, skill, tool, service, action and
//! status, a number and a time: **never its content** (no query, no text,
//! no recipient, no result); its action is kept only as one of the
//! catalog's own words for that service. Held in memory only, the oldest
//! dropped first; a restart starts it empty. Off unless `--run-calls`:
//! nothing kept and the call refused.
//!
//! This is what a later slice writes memory's agent rows from (`agent`
//! records, `memory-days.md`, "Linked records"); nothing reads it yet.
use crate::{reads, tools, writes};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};

/// The flag that turns it on.
pub const FLAG: &str = "--run-calls";
/// The voice's call.
pub const CALL: &str = "run_calls_since";
/// How many calls are kept across all runs, the oldest dropped first.
pub const KEPT: usize = 64;
/// The most one answer's `calls` take, as JSON: inside the tools socket's
/// answer (`reads::ANSWER_MOST`, 4 KiB) with room for its frame, and so
/// under the plan's 8 KiB a pull. The rest waits for the next pull.
pub const ANSWER_CALLS_MOST: usize = 3 * 1024;
/// The longest run id kept; a call naming none, or another, is not kept.
pub const RUN_MOST: usize = 64;
/// The longest action kept; a longer one is kept as [`OTHER`], whatever
/// the catalog lists.
pub const ACTION_MOST: usize = 40;
/// An action not one of the catalog's words for its tool and service.
pub const OTHER: &str = "other";
/// What a call is told with the flag off.
pub const OFF: &str = "a skill run's calls are not kept on this lamp (--run-calls)";

/// How a call ended, as the run was answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// A read answered.
    Read,
    /// A write prepared and held for the person's spoken yes.
    Held,
    /// A skill's fixed write sent (`LAMP_SKILL_WRITE_AGENT`).
    Sent,
    /// Refused here, nothing reached.
    Refused,
}

impl Status {
    fn word(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Held => "held",
            Self::Sent => "sent",
            Self::Refused => "refused",
        }
    }
}

/// One call, as kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunCall {
    /// From 1, one more for each call kept since start.
    pub n: u64,
    pub run: String,
    pub skill: String,
    /// `service_read` or `service_write`.
    pub tool: &'static str,
    /// The catalog code, or none for a name the lamp can't link.
    pub service: Option<&'static str>,
    pub action: String,
    pub status: Status,
    pub at_unix_ms: u64,
}

impl RunCall {
    fn json(&self) -> Value {
        json!({
            "n": self.n,
            "run": self.run,
            "skill": self.skill,
            "tool": self.tool,
            "service": self.service,
            "action": self.action,
            "status": self.status.word(),
            "at_unix_ms": self.at_unix_ms,
        })
    }
}

/// Whether `run` is a run id kept: 1 to [`RUN_MOST`] bytes of letters,
/// digits, `-`, `_` and `.`, as lamp-control's `runs::plain` takes them
/// (its own run ids, and lamp-tasks' `task-<ms>-<pid>-<n>`).
pub fn run_ok(run: &str) -> bool {
    (1..=RUN_MOST).contains(&run.len())
        && run
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// The action as kept: `asked` when it is one of the catalog's own words
/// for `service` and `tool` (a read's kinds, [`reads::kinds`]; a write's
/// actions, [`writes::actions`]), else [`OTHER`]. Free text a run's model
/// wrote as its `what` or `action` never goes in, whatever its letters.
pub fn action_of(tool: &str, service: Option<&str>, asked: Option<&str>) -> String {
    let (Some(service), Some(word)) = (service, asked) else {
        return OTHER.to_owned();
    };
    let listed = match tool {
        tools::READ => reads::kinds(service),
        tools::WRITE => writes::actions(service),
        _ => Vec::new(),
    };
    if word.len() <= ACTION_MOST && listed.contains(&word) {
        word.to_owned()
    } else {
        OTHER.to_owned()
    }
}

#[derive(Default)]
struct Ring {
    calls: VecDeque<RunCall>,
    last: u64,
}

/// The kept calls.
#[derive(Default)]
pub struct RunCalls {
    ring: Mutex<Ring>,
}

/// One call to keep, before it is numbered.
pub struct Noted<'a> {
    pub run: &'a str,
    pub skill: &'a str,
    pub tool: &'static str,
    pub service: Option<&'static str>,
    pub action: Option<&'a str>,
    pub status: Status,
    pub at_unix_ms: u64,
}

impl RunCalls {
    /// Keeps `call`, the oldest of any run dropped when [`KEPT`] are there,
    /// its action as [`action_of`] keeps it; a run id
    /// not [`run_ok`] keeps nothing. Never waits on anything but the ring.
    pub fn note(&self, call: Noted<'_>) {
        if !run_ok(call.run) {
            return;
        }
        let mut ring = self
            .ring
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ring.last += 1;
        let n = ring.last;
        if ring.calls.len() >= KEPT {
            ring.calls.pop_front();
        }
        ring.calls.push_back(RunCall {
            n,
            run: call.run.to_owned(),
            skill: call.skill.to_owned(),
            tool: call.tool,
            service: call.service,
            action: action_of(call.tool, call.service, call.action),
            status: call.status,
            at_unix_ms: call.at_unix_ms,
        });
    }

    /// The calls after number `after`, oldest first, as many as fit
    /// [`ANSWER_CALLS_MOST`]: `{"calls", "last", "more", "missed",
    /// "restarted"}`. `last` is the newest given (or `after`, none given),
    /// for the next pull; `more` says some were left for it; `missed` how
    /// many after `after` were already dropped; `restarted` that `after` is
    /// past the newest kept since start, so this program started again and
    /// every call kept is given from the first.
    pub fn since(&self, after: u64) -> Value {
        let ring = self
            .ring
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let restarted = after > ring.last;
        let after = if restarted { 0 } else { after };
        let first = ring.calls.front().map_or(ring.last + 1, |call| call.n);
        let missed = first.saturating_sub(after + 1);
        let mut calls = Vec::new();
        let mut used = 2;
        let mut last = after;
        let mut more = false;
        for call in ring.calls.iter().filter(|call| call.n > after) {
            let one = call.json();
            let bytes = one.to_string().len() + 1;
            if used + bytes > ANSWER_CALLS_MOST {
                more = true;
                break;
            }
            used += bytes;
            last = call.n;
            calls.push(one);
        }
        json!({
            "calls": calls,
            "last": last,
            "more": more,
            "missed": missed,
            "restarted": restarted,
        })
    }
}

/// The `after` of a `run_calls_since` call's args: a whole number, or 0
/// when not given.
pub fn after_of(args: &Value) -> Result<u64, String> {
    match args.get("after") {
        None | Some(Value::Null) => Ok(0),
        Some(after) => after
            .as_u64()
            .ok_or_else(|| "after is a whole number".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noted(run: &str, n: u64) -> Noted<'_> {
        Noted {
            run,
            skill: "mail-count",
            tool: tools::READ,
            service: Some("gmail"),
            action: Some("search"),
            status: Status::Read,
            at_unix_ms: 1_000 + n,
        }
    }

    /// It keeps the last 64, the oldest dropped first, and says how many a
    /// late pull missed.
    #[test]
    fn the_ring_keeps_the_last_64() {
        let calls = RunCalls::default();
        for n in 0..(KEPT as u64 + 6) {
            calls.note(noted("run-1", n));
        }
        assert_eq!(calls.since(0)["missed"], 6);
        let mut given = Vec::new();
        let mut after = 0;
        loop {
            let page = calls.since(after);
            if after > 0 {
                assert_eq!(page["missed"], 0, "{page}");
            }
            given.extend(page["calls"].as_array().unwrap().iter().cloned());
            after = page["last"].as_u64().unwrap();
            if page["more"] != true {
                break;
            }
        }
        assert_eq!(given.len(), KEPT);
        assert_eq!(given[0]["n"], 7);
        assert_eq!(given[KEPT - 1]["n"], KEPT as u64 + 6);
        // Nothing new: none given, `last` where it was.
        let none = calls.since(after);
        assert_eq!(none["calls"], json!([]));
        assert_eq!(none["last"], after);
        assert_eq!(none["more"], false);
    }

    /// A call keeps only its run, skill, tool, service, action, status, its
    /// number and time: no content field at all; an action of free words is
    /// `other`, and a call with no fit run id keeps nothing; a dotted run id,
    /// as lamp-control's `plain` allows, is kept.
    #[test]
    fn a_call_keeps_no_content() {
        let calls = RunCalls::default();
        let mut call = noted("run-1", 0);
        call.action = Some("Send my mail to x@evil.test");
        calls.note(call);
        for bad in ["", "r 1", "r/1", &"r".repeat(RUN_MOST + 1)] {
            calls.note(noted(bad, 1));
        }
        calls.note(noted("task-1.2", 2));
        let page = calls.since(0);
        let kept = page["calls"].as_array().unwrap();
        assert_eq!(kept.len(), 2, "{page}");
        assert_eq!(kept[1]["run"], "task-1.2");
        let fields: Vec<&str> = kept[0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            fields,
            [
                "action",
                "at_unix_ms",
                "n",
                "run",
                "service",
                "skill",
                "status",
                "tool"
            ]
        );
        assert_eq!(kept[0]["action"], OTHER);
        assert!(!page.to_string().contains("evil"));
        assert_eq!(action_of(tools::WRITE, Some("gmail"), None), OTHER);
    }

    /// Only the catalog's own words for the call's tool and service are
    /// kept (Heron's P2 on #7481): a word of the right letters a run's
    /// model wrote is `other`, as is a read's word for a write, another
    /// service's word, or any word with no service.
    #[test]
    fn only_the_catalogs_words_are_kept() {
        let (read, write) = (tools::READ, tools::WRITE);
        for (tool, service, word) in [
            (read, "gmail", "recent"),
            (read, "gmail", "attachment"),
            (read, "google_calendar", "events"),
            (read, "github", "get"),
            (write, "gmail", "send_mail"),
            (write, "gmail", "delete_draft"),
            (write, "google_calendar", "update_event"),
            (write, "google_drive", "create_file"),
            (write, "asana", "create_task"),
        ] {
            assert_eq!(action_of(tool, Some(service), Some(word)), word);
        }
        for (tool, service, word) in [
            (read, Some("gmail"), "dinner_with_mom_at_7"),
            (write, Some("gmail"), "dinner_with_mom_at_7"),
            (write, Some("gmail"), "recent"),
            (read, Some("gmail"), "send_mail"),
            (write, Some("github"), "send_mail"),
            (write, Some("google_drive"), "or"),
            (write, Some("gmail"), "app"),
            (read, None, "recent"),
            (tools::CONNECTED, Some("gmail"), "recent"),
        ] {
            assert_eq!(action_of(tool, service, Some(word)), OTHER, "{word}");
        }
        let calls = RunCalls::default();
        let mut call = noted("run-1", 0);
        call.action = Some("dinner_with_mom_at_7");
        calls.note(call);
        calls.note(noted("run-1", 1));
        let page = calls.since(0);
        assert_eq!(page["calls"][0]["action"], OTHER);
        assert_eq!(page["calls"][1]["action"], "search");
        assert!(!page.to_string().contains("dinner"), "{page}");
    }

    /// Every answer fits: 64 calls of the longest run and skill are given
    /// over pulls each under the bound (and under the plan's 8 KiB).
    #[test]
    fn each_answer_is_bounded() {
        let calls = RunCalls::default();
        let run = "r".repeat(RUN_MOST);
        let skill = "s".repeat(64);
        let action = "update_event";
        for n in 0..KEPT as u64 {
            calls.note(Noted {
                run: &run,
                skill: &skill,
                tool: tools::WRITE,
                service: Some("google_calendar"),
                action: Some(action),
                status: Status::Refused,
                at_unix_ms: u64::MAX - n,
            });
        }
        // Inside the tools socket's 4 KB, so under the plan's 8 KiB too.
        const { assert!(crate::reads::ANSWER_MOST <= 8 * 1024) };
        let (mut after, mut given, mut pulls) = (0, 0, 0);
        loop {
            let page = calls.since(after);
            let calls_bytes = page["calls"].to_string().len();
            assert!(calls_bytes <= ANSWER_CALLS_MOST, "{calls_bytes}");
            let line = json!({"ok": true, "result": page}).to_string().len();
            assert!(line < crate::reads::ANSWER_MOST, "{line}");
            given += page["calls"].as_array().unwrap().len();
            after = page["last"].as_u64().unwrap();
            pulls += 1;
            if page["more"] != true {
                break;
            }
        }
        assert_eq!(given, KEPT);
        assert!(pulls > 1, "the bound was never reached");
    }

    /// A pull past the newest kept (this program started again): every call
    /// kept since is given from the first, and said.
    #[test]
    fn a_restart_is_said() {
        let calls = RunCalls::default();
        calls.note(noted("run-1", 0));
        let page = calls.since(500);
        assert_eq!(page["restarted"], true);
        assert_eq!(page["calls"][0]["n"], 1);
        assert_eq!(calls.since(1)["restarted"], false);
        assert_eq!(after_of(&json!({})), Ok(0));
        assert_eq!(after_of(&json!({"after": 7})), Ok(7));
        assert!(after_of(&json!({"after": "7"})).is_err());
        assert!(after_of(&json!({"after": -1})).is_err());
    }
}
