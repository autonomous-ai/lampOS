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
//! `agent_tasks` lists them). If the work is still waiting for its agent
//! (`queued`, or `working` with `waiting` true: OpenClaw direct's turn
//! queued on a cold gateway, which the watch reads `starting`, #8510), no
//! earlier
//! work on the records is still under way, and it is still the work this
//! voice handed over last, one short line is
//! asked of the say service, under the voice's own skill name [`SKILL`]:
//! "Your computer is waking up; it'll start in a moment.", or, for work
//! that runs on the lamp (its own engine, or one the person named), "I'm
//! still getting ready; I'll start in a moment."
//! At most once per hand-over, and never a third line: nothing more is
//! read or said for it. Taken up, finished, failed, gone, or another
//! hand-over since: nothing is said. A hand-over that then fails is still
//! told as the task news tells one (`task_news`, `failed_hand_over`): this
//! line marks nothing told.
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
//! moment for it has passed.
//!
//! The line is asked as a client of the say socket
//! ([`crate::say::ask_at`], at [`crate::say::socket_path`]), never through
//! the voice's in-process own asks: the hand-over's tool call runs in the
//! second route's worker (`lamp-voice worker second|provider`), a process
//! of its own that holds none (#8510, lamp-ee17: `no line: no say
//! service` on every hand-over while the asks were kept at the main
//! process's start). The ask is the same line the own asks build, checked
//! by the service as theirs is. Without the say service (`LAMP_SAY` off,
//! nothing answering at the socket) nothing is read or said.
//!
//! Every look that ends with no line leaves one journal line saying why
//! (`lamp-voice: tasks: work N after 20 s: no line: <reason>`), the
//! hand-over without the say service one too, and a line the say service
//! refused or did not say one with its answer.
use crate::say::{Answer, Outcome};
use serde_json::{Value, json};
use std::{path::Path, sync::Mutex, time::Duration};

/// How long after the hand-over the work's record is read.
pub const AFTER: Duration = Duration::from_secs(20);
/// The line for work handed to an agent on the person's computer.
pub const LINE_COMPUTER: &str = "Your computer is waking up; it'll start in a moment.";
/// The line for work that runs on the lamp, by its own engine or one the
/// person named, said in the first person as its hand-over is (#7930; the
/// first builder's P3 on #8510: never "your computer" for the lamp's work).
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

/// How long past the line's wait its outcome may still come over the say
/// socket: the line begun at the end of its wait, said whole (as
/// `app_make`'s socket lines wait).
const SAID_WITHIN: Duration = Duration::from_secs(120);

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

/// Work `id` was just handed over through lamp-tasks at `directory`: one
/// line [`AFTER`] from now while it still waits ([`check_once`]), on a
/// thread of its own, never the turn's, asked on the say socket
/// ([`crate::say::socket_path`]). Nothing when no say service answers
/// there: one connection taken and closed at once, nothing asked.
pub fn watch(directory: &Path, id: &str, on_lamp: bool) {
    let say = crate::say::socket_path();
    if std::os::unix::net::UnixStream::connect(&say).is_err() {
        eprintln!("lamp-voice: tasks: work {id}: no line: no say service");
        return;
    }
    let socket = directory.join("request.sock");
    let id = id.to_owned();
    let _ = std::thread::Builder::new()
        .name("hand-wait".into())
        .spawn(move || {
            std::thread::sleep(AFTER);
            let still_last = || crate::agent_tasks::handed_last_is(&id);
            check_once(&socket, &say, &id, on_lamp, still_last);
        });
}

/// The line for work `id` by the records `answer` lists ([`watch`]'s one
/// read, `None` unreached): `Some` only while it still waits for its agent
/// ([`still_waits`]), no work started before it is still under way (not
/// `done`: it waits its turn behind that one, on a busy agent), it is
/// still the work handed over last (`still_last`), and none was asked for
/// it before. The tests' view of [`decide`], which [`check_once`] uses.
#[cfg(test)]
fn line_for(
    answer: Option<&Value>,
    id: &str,
    on_lamp: bool,
    still_last: bool,
) -> Option<&'static str> {
    decide(answer, id, on_lamp, still_last).ok()
}

/// Whether `work`'s record says it is not yet with its agent: `queued`, or
/// `working` while `waiting` (#8510: OpenClaw direct's turn, enqueued at the
/// hand-over, reads `working` in lamp-tasks while the gateway has not taken
/// it, `starting` on the watch).
fn still_waits(work: &Value) -> bool {
    work["state"] == json!("queued")
        || (work["state"] == json!("working") && work["waiting"] == json!(true))
}

/// [`line_for`], with why there is none: the reason its journal line names.
fn decide(
    answer: Option<&Value>,
    id: &str,
    on_lamp: bool,
    still_last: bool,
) -> Result<&'static str, &'static str> {
    let answer = answer.ok_or("read failed")?;
    if answer.get("ok") != Some(&json!(true)) {
        return Err("read failed");
    }
    let works = answer["works"].as_array().ok_or("read failed")?;
    let work = works
        .iter()
        .find(|work| work["id"] == json!(id))
        .ok_or("not on the records")?;
    if !still_waits(work) {
        return Err(if work["state"] == json!("working") {
            "with its agent"
        } else {
            "not queued or working"
        });
    }
    let started = work["started_ms"].as_u64().unwrap_or(u64::MAX);
    let behind = works.iter().any(|other| {
        other["id"] != json!(id)
            && other["done"] != json!(true)
            && other["started_ms"].as_u64().unwrap_or(0) <= started
    });
    if behind {
        return Err("behind earlier work");
    }
    if !still_last {
        return Err("not the last hand-over");
    }
    if said().iter().any(|said| said == id) {
        return Err("already said");
    }
    Ok(if on_lamp { LINE_OWN } else { LINE_COMPUTER })
}

/// [`watch`]'s one read of work `id` on `socket`, and its one line, asked
/// on the say socket at `say` as [`SKILL`]: what was asked, if anything,
/// and what became of it (none: the say service unreached or quiet).
/// Blocking until the line's outcome; [`watch`]'s thread alone waits.
pub(crate) fn check_once(
    socket: &Path,
    say: &Path,
    id: &str,
    on_lamp: bool,
    still_last: impl FnOnce() -> bool,
) -> Option<(&'static str, Option<Outcome>)> {
    let answer = crate::agent_tasks::ask(
        socket,
        &json!({"op": "work_list", "limit": LISTED}),
        READ_DEADLINE,
    );
    let after = AFTER.as_secs();
    let line = match decide(answer.as_ref(), id, on_lamp, still_last()) {
        Ok(line) => line,
        Err(why) => {
            eprintln!("lamp-voice: tasks: work {id} after {after} s: no line: {why}");
            return None;
        }
    };
    mark_said(id);
    eprintln!(
        "lamp-voice: tasks: work {id} still waits for its agent after {after} s: said it is starting"
    );
    let outcome = ask_line(say, line);
    match &outcome {
        None => eprintln!("lamp-voice: tasks: work {id} after {after} s: no line: no say service"),
        Some(Outcome::Refused(why)) => eprintln!(
            "lamp-voice: tasks: work {id} after {after} s: no line: the say service refused: {why}"
        ),
        Some(Outcome::Spoken) => {}
        Some(other) => {
            eprintln!("lamp-voice: tasks: work {id} after {after} s: the line not said: {other:?}");
        }
    }
    Some((line, outcome))
}

/// `line` asked once on the say socket at `say`, as written, as [`SKILL`],
/// at the normal priority, not begun after [`EXPIRES_MS`]: its outcome,
/// none when the say service isn't there or goes quiet.
fn ask_line(say: &Path, line: &str) -> Option<Outcome> {
    let answered = crate::say::ask_at(
        say,
        SKILL,
        line,
        false,
        crate::say::NORMAL_PRIORITY,
        EXPIRES_MS,
        SAID_WITHIN,
    )
    .ok()?;
    loop {
        match answered.recv() {
            Ok(Answer::Done(outcome)) => break Some(outcome),
            Ok(_) => continue,
            Err(_) => break None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_tasks::tests::tasks_part_with;
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        path::PathBuf,
        thread::JoinHandle,
    };

    /// A fake say service bound at `dir`'s `say.sock`, as `app_make`'s
    /// socket tests bind one: one connection an answer in `answers`, each
    /// ask begun, then told its outcome. What it was asked, and the
    /// listener, to tell that nothing more was asked.
    fn say_service(
        dir: &Path,
        answers: Vec<&'static str>,
    ) -> (PathBuf, JoinHandle<(Vec<Value>, UnixListener)>) {
        let socket = dir.join("say.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let served = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for answer in answers {
                let stream = crate::make_skill_local::tests::accept_within(&listener);
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                asked.push(serde_json::from_str::<Value>(&line).unwrap());
                writeln!(&stream, r#"{{"event":"begun","at_us":7}}"#).unwrap();
                writeln!(&stream, "{answer}").unwrap();
            }
            (asked, listener)
        });
        (socket, served)
    }

    /// Nothing more was asked of `listener`'s say service.
    fn asked_nothing_more(listener: &UnixListener) {
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().map(|_| ()).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "a say asked"
        );
    }

    /// The ask the line goes as: the voice's own skill name, as written,
    /// at the normal priority, within [`EXPIRES_MS`].
    fn assert_the_ask(asked: &Value, line: &str) {
        assert_eq!(asked["skill"], SKILL);
        assert_eq!(asked["text"], line);
        assert!(asked.get("note").is_none());
        assert_eq!(asked["priority"], crate::say::NORMAL_PRIORITY);
        assert_eq!(asked["expires_in_ms"], EXPIRES_MS);
    }

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
        let (say, served) = say_service(&dir, vec![r#"{"outcome":"spoken"}"#]);
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
        let (line, outcome) =
            check_once(&socket, &say, "q-1", false, || true).expect("the line, after 20 s queued");
        assert_eq!(line, LINE_COMPUTER);
        assert_eq!(outcome, Some(Outcome::Spoken));
        assert_eq!(
            seen.recv().unwrap(),
            json!({"op": "work_list", "limit": 10})
        );
        assert!(check_once(&socket, &say, "q-1", false, || true).is_none());
        assert!(check_once(&socket, &say, "q-1", false, || true).is_none());
        let (asked, listener) = served.join().unwrap();
        assert_eq!(asked.len(), 1, "said once");
        assert_the_ask(&asked[0], LINE_COMPUTER);
        asked_nothing_more(&listener);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The lamp's own engine's work is said in the first person.
    #[test]
    fn the_lamps_own_work_still_queued_is_said_in_the_first_person() {
        let dir = folder("own");
        let (say, served) = say_service(&dir, vec![r#"{"outcome":"spoken"}"#]);
        let _seen = tasks_part_with(&dir, vec![("request.sock", work("q-own", "queued"))], true);
        let (line, _outcome) =
            check_once(&dir.join("request.sock"), &say, "q-own", true, || true).unwrap();
        assert_eq!(line, LINE_OWN);
        let (asked, _listener) = served.join().unwrap();
        assert_the_ask(&asked[0], LINE_OWN);
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

    /// #8510, lamp-ee17: OpenClaw direct enqueues the turn at the
    /// hand-over, so lamp-tasks reads it `working` while a cold gateway
    /// has not taken it (`waiting` true). 20 s on, that work gets the line,
    /// as `queued` work does; `working` without `waiting` is with its agent
    /// and gets none; the earlier-work rule holds for it as for `queued`.
    #[test]
    fn work_working_but_still_waiting_gets_the_line_and_working_alone_none() {
        let waiting = |id: &str, started_ms: u64| {
            let mut work = record(id, "working", started_ms);
            work["waiting"] = json!(true);
            work
        };
        let cold = json!({"ok": true, "works": [waiting("w-cold", 1_000)]});
        assert_eq!(line_for(Some(&cold), "w-cold", true, true), Some(LINE_OWN));
        let mut taken = record("w-taken", "working", 1_000);
        taken["waiting"] = json!(false);
        let taken = json!({"ok": true, "works": [taken]});
        assert_eq!(line_for(Some(&taken), "w-taken", true, true), None);
        assert_eq!(
            decide(Some(&taken), "w-taken", true, true),
            Err("with its agent")
        );
        assert_eq!(
            line_for(Some(&work("w-plain", "working")), "w-plain", true, true),
            None
        );
        let behind = json!({"ok": true, "works": [
            waiting("w-behind", 2_000),
            record("w-earlier", "working", 1_000),
        ]});
        assert_eq!(
            decide(Some(&behind), "w-behind", true, true),
            Err("behind earlier work")
        );
    }

    /// Every look with no line says why, one reason a look.
    #[test]
    fn a_look_with_no_line_names_its_reason() {
        assert_eq!(decide(None, "r-1", false, true), Err("read failed"));
        let gone = json!({"ok": false, "error": "no_such_work"});
        assert_eq!(decide(Some(&gone), "r-1", false, true), Err("read failed"));
        assert_eq!(
            decide(Some(&work("r-other", "queued")), "r-1", false, true),
            Err("not on the records")
        );
        assert_eq!(
            decide(Some(&work("r-2", "done")), "r-2", false, true),
            Err("not queued or working")
        );
        assert_eq!(
            decide(Some(&work("r-3", "queued")), "r-3", false, false),
            Err("not the last hand-over")
        );
        mark_said("r-4");
        assert_eq!(
            decide(Some(&work("r-4", "queued")), "r-4", false, true),
            Err("already said")
        );
    }

    /// The one session on the computer, end to end over the socket: a task
    /// typed behind the running turn is read `queued` at 20 s and gets no
    /// line, and nothing is asked of the say service.
    #[test]
    fn the_computers_queue_behind_a_running_turn_says_nothing() {
        let dir = folder("one-session");
        let (say, served) = say_service(&dir, Vec::new());
        let listed = json!({"ok": true, "works": [
            record("s-2", "queued", 2_000),
            record("s-1", "working", 1_000),
        ]});
        let _seen = tasks_part_with(&dir, vec![("request.sock", listed)], true);
        assert!(check_once(&dir.join("request.sock"), &say, "s-2", false, || true).is_none());
        let (asked, listener) = served.join().unwrap();
        assert!(asked.is_empty());
        asked_nothing_more(&listener);
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

    /// #8510, lamp-ee17: the hand-over's tool call runs in the second
    /// route's worker, a process that holds no own asks, and every
    /// hand-over journaled `no line: no say service`. The line is asked on
    /// the say socket as a client, with nothing set up in this process
    /// first: the say service's answer reaches the look, and a refusal is
    /// told back as the outcome, never as said.
    #[test]
    fn the_line_is_asked_on_the_say_socket_with_no_start_in_this_process() {
        let dir = folder("socket");
        let (say, served) = say_service(
            &dir,
            vec![
                r#"{"outcome":"spoken"}"#,
                r#"{"outcome":"refused","why":"the microphones are closed"}"#,
            ],
        );
        let _seen = tasks_part_with(
            &dir,
            vec![
                ("request.sock", work("o-1", "queued")),
                ("request.sock", work("o-2", "queued")),
            ],
            true,
        );
        let socket = dir.join("request.sock");
        assert_eq!(
            check_once(&socket, &say, "o-1", false, || true),
            Some((LINE_COMPUTER, Some(Outcome::Spoken)))
        );
        assert_eq!(
            check_once(&socket, &say, "o-2", true, || true),
            Some((
                LINE_OWN,
                Some(Outcome::Refused(
                    "refused the microphones are closed".to_owned()
                ))
            ))
        );
        let (asked, listener) = served.join().unwrap();
        assert_eq!(asked.len(), 2);
        assert_the_ask(&asked[0], LINE_COMPUTER);
        assert_the_ask(&asked[1], LINE_OWN);
        asked_nothing_more(&listener);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// No say service at the socket: the line is decided, nothing is said,
    /// and the look tells it had no outcome.
    #[test]
    fn with_no_say_service_the_line_has_no_outcome() {
        let dir = folder("no-say");
        let _seen = tasks_part_with(&dir, vec![("request.sock", work("n-1", "queued"))], true);
        assert_eq!(
            check_once(
                &dir.join("request.sock"),
                &dir.join("none.sock"),
                "n-1",
                false,
                || true
            ),
            Some((LINE_COMPUTER, None))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The yes's hand-over starts the one look, for work asked by voice,
    /// right after it is kept as the work handed over last; nothing in the
    /// coordinator sets it up, and the look asks on the say socket.
    #[test]
    fn the_hand_over_starts_the_look_with_no_start_in_the_coordinator() {
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
        assert!(!coordinator.contains("crate::hand_wait::"));
        let own = include_str!("hand_wait.rs");
        let watch = own.find("pub fn watch(").unwrap();
        let ends = watch + own[watch..].find("\n}\n").unwrap();
        assert!(own[watch..ends].contains("crate::say::socket_path()"));
    }
}
