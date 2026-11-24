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
//! So [`AFTER`] after the hand-over, the work's record is read once
//! (`{"op":"work"}` on lamp-tasks' request socket, as
//! `agent_tasks::open_work_note` reads it). If it is still `queued`, and
//! it is still the work this voice handed over last, one short line is
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
/// The bound on the one read of the work's record.
const READ_DEADLINE: Duration = Duration::from_millis(1_500);

/// The say service's own asks, set once at start ([`start`]).
static OWN: OnceLock<OwnAsks> = OnceLock::new();

/// The work ids a line was asked for, so that no hand-over gets two.
fn said() -> std::sync::MutexGuard<'static, Vec<String>> {
    static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
    SAID.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
    let mut said = said();
    if !said.iter().any(|said| said == id) {
        said.push(id.to_owned());
    }
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

/// The line for work `id` whose record reads `answer` ([`watch`]'s one
/// read, `None` unreached): `Some` only while it is still `queued`, it is
/// still the work handed over last (`still_last`), and none was asked for
/// it before.
fn line_for(
    answer: Option<&Value>,
    id: &str,
    own_engine: bool,
    still_last: bool,
) -> Option<&'static str> {
    let answer = answer?;
    if answer.get("ok") != Some(&json!(true)) || answer["work"]["state"] != json!("queued") {
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
    let answer = crate::agent_tasks::ask(socket, &json!({"op": "work", "id": id}), READ_DEADLINE);
    let line = line_for(answer.as_ref(), id, own_engine, still_last())?;
    said().push(id.to_owned());
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

    fn work(state: &str) -> Value {
        json!({"ok": true, "work": {"id": "7", "state": state, "done": false}})
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
                ("request.sock", work("queued")),
                ("request.sock", work("queued")),
                ("request.sock", work("failed")),
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
        assert_eq!(seen.recv().unwrap(), json!({"op": "work", "id": "q-1"}));
        assert!(check_once(&socket, "q-1", false, &own, || true).is_none());
        assert!(check_once(&socket, "q-1", false, &own, || true).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The lamp's own engine's work is said in the first person.
    #[test]
    fn the_lamps_own_work_still_queued_is_said_in_the_first_person() {
        let dir = folder("own");
        let (own, asked) = OwnAsks::for_test();
        let _seen = tasks_part_with(&dir, vec![("request.sock", work("queued"))], true);
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
                line_for(Some(&work(state)), &format!("t-{n}"), false, true),
                None,
                "{state}"
            );
        }
        let gone = json!({"ok": false, "error": "no_such_work"});
        assert_eq!(line_for(Some(&gone), "t-gone", false, true), None);
        assert_eq!(line_for(None, "t-unreached", false, true), None);
        assert_eq!(
            line_for(Some(&work("queued")), "t-other", false, false),
            None
        );
        assert_eq!(
            line_for(Some(&work("queued")), "t-queued", false, true),
            Some(LINE_COMPUTER)
        );
    }

    /// A wait already told at the hand-over (the computer's pane known to
    /// be stopped) gets no line at 20 s, though the work is still queued.
    #[test]
    fn a_wait_told_at_the_hand_over_gets_no_line_at_20_s() {
        told_at_hand_over("t-told");
        told_at_hand_over("t-told");
        assert_eq!(line_for(Some(&work("queued")), "t-told", false, true), None);
        assert_eq!(
            line_for(Some(&work("queued")), "t-not-told", false, true),
            Some(LINE_COMPUTER)
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
