//! One line while a hand-over waits on a cold session (#8445).
//!
//! After the person's yes, `confirm_agent_task` is answered at once and
//! the model says it is handing the work over ("Sure, I'm giving that to
//! …", said in the same breath with `LAMP_TOOL_LEAD_IN`). The work then
//! waits in lamp-tasks, `queued`, until its agent takes it up. On a cold
//! session that wait was 76 s on lamp-52e6 (24 Nov 04:3x, #8043's kit-8022:
//! the Harness pane not running, the session reopened, `ENGINE_NOT_READY`
//! once, the turn accepted in 29,099 ms), and the person heard nothing
//! after the first line.
//!
//! So [`AFTER`] after the hand-over, the work records are read once
//! (`{"op":"work_list","limit":10}` on lamp-tasks' request socket, as
//! `agent_tasks` lists them). If the work is still `queued`, no earlier
//! work on the records is still under way, and it is still the work this
//! voice handed over last, one short line is
//! asked of the say service ([`crate::say::OwnAsks`]), as the voice's own:
//! "Your computer is waking up; it'll start in a moment.", or, for the
//! lamp's own engine, "I'm still getting ready; I'll start in a moment."
//! At most once per hand-over, and never a third line: nothing more is
//! read or said for it. Taken up, finished, failed, gone, or another
//! hand-over since: nothing is said. A hand-over that then fails is still
//! told as the task news tells one (`task_news`, `failed_hand_over`): this
//! line marks nothing told. A wait already told at the hand-over (the
//! waking line, when the computer's pane is known to be stopped:
//! [`told_at_hand_over`]) gets no line at [`AFTER`]: one line about the
//! wait, never two.
//!
//! Work queued behind earlier work still under way gets no line: lamp-tasks
//! runs one at a time (`direct.rs`'s `QUEUED_ABOUT`, "waiting for the task
//! before it to end", with `LAMP_TASKS_OPENCLAW_DIRECT=on`; the one
//! session's queue behind the running turn, with `LAMP_TASKS_ONE_SESSION=on`),
//! so it waits on a busy agent, not a waking one, and the line would be
//! false (Jarvis's P2 on #8510).
//!
//! The say service's gates hold for it as for any say: nobody being heard,
//! nothing under way, the speaker free, and [`crate::say::QUIET_US`] since
//! the last turn. Not begun within [`EXPIRES_MS`], it is let go: the
//! moment for it has passed. Without the say service (`LAMP_SAY` off) the
//! voice has no own asks, and nothing is read or said.
use crate::say::{Answer, OwnAsks};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{Mutex, OnceLock},
    time::Duration,
};

/// How long after the hand-over the work's record is read.
pub const AFTER: Duration = Duration::from_secs(20);
/// The line for work handed to an agent on the person's computer.
pub const LINE_COMPUTER: &str = "Your computer is waking up; it'll start in a moment.";
/// The line for the lamp's own engine (`agent_tasks::OWN_ENGINE`), said in
/// the first person as its hand-over is (#7930).
pub const LINE_OWN: &str = "I'm still getting ready; I'll start in a moment.";
/// The say service's skill name for it.
pub const SKILL: &str = "voice-tasks";
/// Not begun within this, the line is let go.
pub const EXPIRES_MS: u64 = 15_000;
/// The bound on the one read of the work records.
const READ_DEADLINE: Duration = Duration::from_millis(1_500);
/// The most records read: lamp-tasks' `work_list` bound (`works::LIST_MOST`).
const LISTED: u16 = 10;
/// The most work ids kept in [`said`]: a line only ever goes to the work
/// handed over last, so the newest few are enough (Jarvis's P3 on #8510).
pub const SAID_KEPT: usize = 8;

/// The say service's own asks, set once at start ([`start`]).
static OWN: OnceLock<OwnAsks> = OnceLock::new();

/// The work ids a line was asked for, so that no hand-over gets two: the
/// newest [`SAID_KEPT`] only ([`mark_said`]).
fn said() -> std::sync::MutexGuard<'static, Vec<String>> {
    static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
    SAID.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Keep `id` in [`said`], once, dropping the oldest past [`SAID_KEPT`].
fn mark_said(id: &str) {
    keep_newest(&mut said(), id);
}

/// `id` into `said` once, the oldest dropped past [`SAID_KEPT`].
fn keep_newest(said: &mut Vec<String>, id: &str) {
    if said.iter().any(|said| said == id) {
        return;
    }
    said.push(id.to_owned());
    let over = said.len().saturating_sub(SAID_KEPT);
    said.drain(..over);
}

/// Keep the say service's own asks for the lines (the coordinator, at
/// start, as the app-make socket takes them). None: no line is ever said.
pub fn start(own: Option<OwnAsks>) {
    if let Some(own) = own {
        let _ = OWN.set(own);
    }
}

/// Work `id`'s wait was already told at its hand-over: the waking line,
/// said from the start when the computer's pane is known to be stopped
/// (the first builder on #8445, 14:51). No line comes for it at [`AFTER`]:
/// one line about the wait, never two.
pub fn told_at_hand_over(id: &str) {
    mark_said(id);
}

/// Work `id` was just handed over through lamp-tasks at `directory`: one
/// line [`AFTER`] from now while it still waits ([`check_once`]), on a
/// thread of its own, never the turn's. Nothing without the say service.
pub fn watch(directory: &Path, id: &str, own_engine: bool) {
    let Some(own) = OWN.get().cloned() else {
        return;
    };
    let socket = directory.join("request.sock");
    let id = id.to_owned();
    let _ = std::thread::Builder::new()
        .name("hand-wait".into())
        .spawn(move || {
            std::thread::sleep(AFTER);
            let still_last = || crate::agent_tasks::handed_last_is(&id);
            check_once(&socket, &id, own_engine, &own, still_last);
        });
}

/// The line for work `id` by the records `answer` lists ([`watch`]'s one
/// read, `None` unreached): `Some` only while it is still `queued`, no
/// work started before it is still under way (not `done`: it waits its
/// turn behind that one, on a busy agent), it is still the work handed
/// over last (`still_last`), and none was asked for it before.
fn line_for(
    answer: Option<&Value>,
    id: &str,
    own_engine: bool,
    still_last: bool,
) -> Option<&'static str> {
    let answer = answer?;
    if answer.get("ok") != Some(&json!(true)) {
        return None;
    }
    let works = answer["works"].as_array()?;
    let work = works.iter().find(|work| work["id"] == json!(id))?;
    if work["state"] != json!("queued") {
        return None;
    }
    let started = work["started_ms"].as_u64().unwrap_or(u64::MAX);
    let behind = works.iter().any(|other| {
        other["id"] != json!(id)
            && other["done"] != json!(true)
            && other["started_ms"].as_u64().unwrap_or(0) <= started
    });
    if behind {
        return None;
    }
    if !still_last || said().iter().any(|said| said == id) {
        return None;
    }
    Some(if own_engine { LINE_OWN } else { LINE_COMPUTER })
}

/// [`watch`]'s one read of work `id` on `socket`, and its one line, asked
/// of `own`: what was asked, if anything, and where its answer comes.
pub(crate) fn check_once(
    socket: &Path,
    id: &str,
    own_engine: bool,
    own: &OwnAsks,
    still_last: impl FnOnce() -> bool,
) -> Option<(&'static str, std::sync::mpsc::Receiver<Answer>)> {
    let answer = crate::agent_tasks::ask(
        socket,
        &json!({"op": "work_list", "limit": LISTED}),
        READ_DEADLINE,
    );
    let line = line_for(answer.as_ref(), id, own_engine, still_last())?;
    mark_said(id);
    eprintln!(
        "lamp-voice: tasks: work {id} still waits for its agent after {} s: said it is starting",
        AFTER.as_secs()
    );
    Some((
        line,
        own.text(SKILL, line, crate::say::NORMAL_PRIORITY, EXPIRES_MS),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_tasks::tests::tasks_part_with;
    use std::path::PathBuf;

    /// A folder of its own for one test's tasks part.
    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-hand-wait-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// One record of `id` in `state`, as `work_list` lists it.
    fn record(id: &str, state: &str, started_ms: u64) -> Value {
        let done = matches!(
            state,
            "done" | "failed" | "stopped" | "timed_out" | "cancelled"
        );
        json!({"id": id, "state": state, "done": done, "started_ms": started_ms})
    }

    /// `work_list`'s answer with `id`'s record alone.
    fn work(id: &str, state: &str) -> Value {
        json!({"ok": true, "works": [record(id, state, 1_000)]})
    }

    /// lamp-52e6 on #8043 (24 Nov 04:3x): the person's yes, then 76 s of
    /// silence while the work sat `queued` on a cold session. 20 s on, a
    /// work still `queued` gets one line, as the voice's own say at the
    /// normal priority; a third read of it says nothing more, and neither
    /// does its failure after that (the task news tells a failed hand-over,
    /// `task_news::failed_hand_over`; this line marks nothing told).
    #[test]
    fn a_hand_over_still_queued_after_20_s_gets_one_line_and_never_a_third() {
        let dir = folder("queued");
        let (own, asked) = OwnAsks::for_test();
        let seen = tasks_part_with(
            &dir,
            vec![
                ("request.sock", work("q-1", "queued")),
                ("request.sock", work("q-1", "queued")),
                ("request.sock", work("q-1", "failed")),
            ],
            true,
        );
        let socket = dir.join("request.sock");
        let (line, _answer) =
            check_once(&socket, "q-1", false, &own, || true).expect("the line, after 20 s queued");
        assert_eq!(line, LINE_COMPUTER);
        assert_eq!(
            asked(),
            Some((SKILL.to_owned(), crate::say::NORMAL_PRIORITY))
        );
        assert_eq!(
            seen.recv().unwrap(),
            json!({"op": "work_list", "limit": 10})
        );
        assert!(check_once(&socket, "q-1", false, &own, || true).is_none());
        assert!(check_once(&socket, "q-1", false, &own, || true).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The lamp's own engine's work is said in the first person.
    #[test]
    fn the_lamps_own_work_still_queued_is_said_in_the_first_person() {
        let dir = folder("own");
        let (own, asked) = OwnAsks::for_test();
        let _seen = tasks_part_with(&dir, vec![("request.sock", work("q-own", "queued"))], true);
        let (line, _answer) =
            check_once(&dir.join("request.sock"), "q-own", true, &own, || true).unwrap();
        assert_eq!(line, LINE_OWN);
        assert!(asked().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Taken up, finished, failed or gone, or another hand-over since:
    /// nothing is said.
    #[test]
    fn work_taken_up_finished_gone_or_not_the_last_gets_no_line() {
        for (n, state) in ["working", "idle", "failed", "done", "stopped"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                line_for(
                    Some(&work(&format!("t-{n}"), state)),
                    &format!("t-{n}"),
                    false,
                    true
                ),
                None,
                "{state}"
            );
        }
        let gone = json!({"ok": false, "error": "no_such_work"});
        assert_eq!(line_for(Some(&gone), "t-gone", false, true), None);
        let off_the_records = work("t-another", "queued");
        assert_eq!(
            line_for(Some(&off_the_records), "t-gone", false, true),
            None
        );
        assert_eq!(line_for(None, "t-unreached", false, true), None);
        assert_eq!(
            line_for(Some(&work("t-other", "queued")), "t-other", false, false),
            None
        );
        assert_eq!(
            line_for(Some(&work("t-queued", "queued")), "t-queued", false, true),
            Some(LINE_COMPUTER)
        );
    }

    /// A wait already told at the hand-over (the computer's pane known to
    /// be stopped) gets no line at 20 s, though the work is still queued.
    #[test]
    fn a_wait_told_at_the_hand_over_gets_no_line_at_20_s() {
        told_at_hand_over("t-told");
        told_at_hand_over("t-told");
        assert_eq!(
            line_for(Some(&work("t-told", "queued")), "t-told", false, true),
            None
        );
        assert_eq!(
            line_for(
                Some(&work("t-not-told", "queued")),
                "t-not-told",
                false,
                true
            ),
            Some(LINE_COMPUTER)
        );
    }

    /// Jarvis's P2 on #8510: lamp-tasks runs one at a time, so work handed
    /// over while earlier work still runs reads `queued` at 20 s on an agent
    /// that is busy, not waking. On the lamp (OpenClaw direct,
    /// `QUEUED_ABOUT`) and on the computer (the one session's queue behind
    /// the running turn) it gets no line; earlier work already over does
    /// not hold it back, and work started after it does not either.
    #[test]
    fn work_queued_behind_running_work_gets_no_line() {
        for (n, before) in ["working", "queued", "needs-input", "idle"]
            .into_iter()
            .enumerate()
        {
            let id = format!("b-own-{n}");
            let behind = json!({"ok": true, "works": [
                record(&id, "queued", 2_000),
                record("b-earlier", before, 1_000),
            ]});
            assert_eq!(
                line_for(Some(&behind), &id, true, true),
                None,
                "lamp, {before}"
            );
            let id = format!("b-computer-{n}");
            let behind = json!({"ok": true, "works": [
                record(&id, "queued", 2_000),
                record("b-earlier", before, 1_000),
            ]});
            assert_eq!(
                line_for(Some(&behind), &id, false, true),
                None,
                "computer, {before}"
            );
        }
        let after_one_over = json!({"ok": true, "works": [
            record("b-later-ok", "queued", 2_000),
            record("b-done", "done", 1_000),
            record("b-failed", "failed", 1_500),
        ]});
        assert_eq!(
            line_for(Some(&after_one_over), "b-later-ok", false, true),
            Some(LINE_COMPUTER)
        );
        let before_a_newer = json!({"ok": true, "works": [
            record("b-newer", "queued", 3_000),
            record("b-first", "queued", 2_000),
        ]});
        assert_eq!(
            line_for(Some(&before_a_newer), "b-first", true, true),
            Some(LINE_OWN)
        );
    }

    /// The one session on the computer, end to end over the socket: a task
    /// typed behind the running turn is read `queued` at 20 s and gets no
    /// line, and nothing is asked of the say service.
    #[test]
    fn the_computers_queue_behind_a_running_turn_says_nothing() {
        let dir = folder("one-session");
        let (own, asked) = OwnAsks::for_test();
        let listed = json!({"ok": true, "works": [
            record("s-2", "queued", 2_000),
            record("s-1", "working", 1_000),
        ]});
        let _seen = tasks_part_with(&dir, vec![("request.sock", listed)], true);
        assert!(check_once(&dir.join("request.sock"), "s-2", false, &own, || true).is_none());
        assert_eq!(asked(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Jarvis's P3 on #8510: the ids kept against a second line stay the
    /// newest [`SAID_KEPT`], each once.
    #[test]
    fn the_said_ids_keep_only_the_newest_few() {
        let mut said = Vec::new();
        for n in 0..(SAID_KEPT + 5) {
            keep_newest(&mut said, &format!("k-{n}"));
            keep_newest(&mut said, &format!("k-{n}"));
        }
        assert_eq!(said.len(), SAID_KEPT);
        assert_eq!(said.first().map(String::as_str), Some("k-5"));
        assert_eq!(
            said.last().map(String::as_str),
            Some(format!("k-{}", SAID_KEPT + 4).as_str())
        );
    }

    /// The yes's hand-over starts the one look, for work asked by voice,
    /// right after it is kept as the work handed over last; and the
    /// coordinator gives it the say service's own asks at start.
    #[test]
    fn the_hand_over_starts_the_look_and_the_coordinator_gives_it_the_say_service() {
        let tasks = include_str!("agent_tasks.rs");
        let handed = tasks
            .find("handed_over(&pending.id.to_string(), &label_of(&pending.text));")
            .unwrap();
        let watched = tasks
            .find("crate::hand_wait::watch(")
            .expect("the hand-over starts it");
        assert!(handed < watched && watched - handed < 400);
        assert!(tasks[handed..watched].contains("if !crate::messages::is_text(pending.request)"));
        let coordinator = include_str!("coordinator.rs");
        let parked = coordinator
            .find("crate::app_make::start_from_environment(crate::say::Says::parked_asks());")
            .unwrap();
        let started = coordinator
            .find("crate::hand_wait::start(crate::say::Says::parked_asks());")
            .expect("started with the say service's own asks");
        assert!(parked < started);
    }
}
