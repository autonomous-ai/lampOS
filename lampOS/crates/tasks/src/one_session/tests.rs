//! The one session's pane, queue and reopen rules with a fake Harness.
use super::*;
use crate::harness::Observation;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::thread;

const MACHINE: &str = "0123456789abcdef0123456789abcdef";

/// A fake Harness: each open makes a new agent; typed words are the
/// agent's ask, and its turn is idle at once, answered with its ask's
/// first line, unless a scripted reply says otherwise.
#[derive(Default)]
struct Fake {
    opens: Mutex<u32>,
    /// Scripted answers to `reply`, in order; empty: typed.
    replies: Mutex<VecDeque<Typed>>,
    /// Opens that fail, in a row, first.
    failing_opens: Mutex<u32>,
    /// Every call, as `verb agent`.
    calls: Mutex<Vec<String>>,
    asked: Mutex<std::collections::BTreeMap<String, String>>,
    /// A show that reads another ask first, this many times.
    other_asks: Mutex<u32>,
    /// A pane's state as `show` reads it, in place of idle.
    states: Mutex<std::collections::BTreeMap<String, String>>,
    /// Panes Harness no longer has: `show` says `gone`.
    gone: Mutex<Vec<String>>,
    /// Shows that read the last words untaken, first.
    untaken: Mutex<u32>,
    /// What `resume` answers; none: refused.
    resumed: Mutex<Option<String>>,
    /// What `wait` reads, in order; empty: idle.
    waits: Mutex<VecDeque<String>>,
    /// Opens that make the pane but answer uncertain, first.
    uncertain_opens: Mutex<u32>,
    /// Agents `ps` lists live in the pane's folder.
    live: Mutex<Vec<String>>,
    /// Each `close` as `machine agent`.
    closed_on: Mutex<Vec<String>>,
    /// Closes that fail, in a row, first.
    failing_closes: Mutex<u32>,
    /// The code a failing close answers; none: `timed_out`.
    close_error: Mutex<Option<String>>,
    /// `harness capabilities` says program sessions (openharness#4393).
    programs: Mutex<bool>,
    /// The kept session Harness holds by name and token, if any.
    kept: Mutex<Option<String>>,
    /// Scripted refusals of `ensure`, in order, first.
    ensure_errors: Mutex<VecDeque<String>>,
    /// Scripted answers to an acknowledged `reply`, in order; empty: taken.
    takes: Mutex<VecDeque<Took>>,
    /// Inputs the kept session took.
    inputs: Mutex<u32>,
}

fn agent_of(n: u32) -> String {
    format!("{n:08}-1111-4111-8111-111111111111")
}

impl Fake {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
    fn count(&self, verb: &str) -> usize {
        self.calls()
            .iter()
            .filter(|call| call.split(' ').next() == Some(verb))
            .count()
    }
    fn call(&self, line: String) {
        self.calls.lock().unwrap().push(line);
    }
}

impl PaneHarness for Fake {
    fn open(&self, launch: &Launch) -> LaunchOutcome {
        self.call(format!("new {}", launch.name));
        let mut failing = self.failing_opens.lock().unwrap();
        if *failing > 0 {
            *failing -= 1;
            return LaunchOutcome::NotSent {
                code: "new_failed".into(),
            };
        }
        let mut opens = self.opens.lock().unwrap();
        *opens += 1;
        let agent = agent_of(*opens);
        self.asked
            .lock()
            .unwrap()
            .insert(agent.clone(), launch.prompt.clone());
        self.live.lock().unwrap().push(agent.clone());
        let mut uncertain = self.uncertain_opens.lock().unwrap();
        if *uncertain > 0 {
            *uncertain -= 1;
            return LaunchOutcome::Uncertain {
                code: "timed_out".into(),
            };
        }
        LaunchOutcome::Created { agent_id: agent }
    }
    fn type_into(&self, _: &str, agent: &str, words: &str) -> Typed {
        self.call(format!("reply {agent}"));
        let typed = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Typed::Typed);
        if matches!(typed, Typed::Typed | Typed::Taken(_)) {
            self.asked
                .lock()
                .unwrap()
                .insert(agent.to_owned(), words.to_owned());
        }
        typed
    }
    fn wait_end(&self, _: &str, agent: &str, _: &AtomicBool) -> Result<Option<String>, String> {
        self.call(format!("wait {agent}"));
        let state = self
            .waits
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "idle".into());
        // "none": `wait` timed out with no state.
        Ok((state != "none").then_some(state))
    }
    fn observe(&self, _: &str, agent: &str) -> Result<PaneSeen, String> {
        self.call(format!("show {agent}"));
        if self.gone.lock().unwrap().iter().any(|gone| gone == agent) {
            return Err("gone".into());
        }
        let state = self
            .states
            .lock()
            .unwrap()
            .get(agent)
            .cloned()
            .unwrap_or_else(|| "idle".into());
        let mut other = self.other_asks.lock().unwrap();
        let ask = if *other > 0 {
            *other -= 1;
            "An earlier task".to_owned()
        } else {
            self.asked
                .lock()
                .unwrap()
                .get(agent)
                .cloned()
                .unwrap_or_default()
        };
        let mut untaken = self.untaken.lock().unwrap();
        let taken = *untaken == 0;
        *untaken = untaken.saturating_sub(1);
        Ok(PaneSeen {
            seen: Observation {
                agent_id: agent.into(),
                state,
                answer: format!("answer to {}", ask.lines().next().unwrap_or_default()),
                ask: Some(ask),
                truncated: false,
                tools: 1,
            },
            taken,
        })
    }
    fn close_now(&self, machine: &str, agent: &str) -> Result<Closed, String> {
        self.call(format!("close {agent}"));
        self.closed_on
            .lock()
            .unwrap()
            .push(format!("{machine} {agent}"));
        let mut failing = self.failing_closes.lock().unwrap();
        if *failing > 0 {
            *failing -= 1;
            let code = self.close_error.lock().unwrap().clone();
            return Err(code.unwrap_or_else(|| "timed_out".into()));
        }
        Ok(Closed::Closed)
    }
    fn live_in(&self, _: &str, _: &str) -> Result<Vec<String>, String> {
        self.call("ps".into());
        Ok(self.live.lock().unwrap().clone())
    }
    fn resume(&self, _: &str, agent: &str) -> Result<String, String> {
        self.call(format!("resume {agent}"));
        self.resumed
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "session_closed".into())
    }
    fn program_sessions(&self) -> Option<bool> {
        self.call("capabilities".into());
        Some(*self.programs.lock().unwrap())
    }
    fn ensure_kept(
        &self,
        engine: EngineKind,
        name: &str,
        token: &Path,
        _: &str,
        fresh: bool,
    ) -> Result<Ensured, String> {
        self.call(format!(
            "ensure {name}{}",
            if fresh { " --fresh" } else { "" }
        ));
        assert_eq!(name, name_for(engine));
        assert!(token.exists(), "the token file is made before ensure");
        if let Some(code) = self.ensure_errors.lock().unwrap().pop_front() {
            return Err(code);
        }
        let mut kept = self.kept.lock().unwrap();
        let mut gone = self.gone.lock().unwrap();
        let (result, agent) = match kept.clone() {
            Some(agent) if !fresh && gone.contains(&agent) => {
                gone.retain(|g| *g != agent);
                ("resumed", agent)
            }
            Some(agent) if !fresh => ("live", agent),
            _ => {
                let mut opens = self.opens.lock().unwrap();
                *opens += 1;
                ("new", agent_of(*opens))
            }
        };
        *kept = Some(agent.clone());
        let agent_id_now = agent.clone();
        Ok(Ensured {
            result: result.into(),
            agent_id: agent,
            state: "idle".into(),
            why: None,
            engine_session: Some(format!("agent:main:kept-{}", agent_id_now)),
        })
    }
    fn reply_kept(&self, name: &str, _: &Path, words: &str) -> Took {
        self.call(format!("reply-kept {name}"));
        let took = self.takes.lock().unwrap().pop_front();
        let took = took.unwrap_or_else(|| {
            let mut inputs = self.inputs.lock().unwrap();
            *inputs += 1;
            Took::Taken(format!("in-{inputs}"))
        });
        if matches!(took, Took::Taken(_))
            && let Some(agent) = self.kept.lock().unwrap().clone()
        {
            self.asked.lock().unwrap().insert(agent, words.to_owned());
        }
        took
    }
    fn wait_kept(
        &self,
        _: &str,
        _: &Path,
        input: &str,
        _: &AtomicBool,
    ) -> Result<Option<String>, String> {
        self.call(format!("wait-after {input}"));
        let state = self
            .waits
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "idle".into());
        Ok((state != "none").then_some(state))
    }
    fn wait_after(
        &self,
        _: &str,
        _: &str,
        input: &str,
        _: &AtomicBool,
    ) -> Result<Option<String>, String> {
        self.call(format!("wait-after-pane {input}"));
        let state = self
            .waits
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "idle".into());
        Ok((state != "none").then_some(state))
    }
    fn close_kept(&self, name: &str, _: &Path) -> Result<(), String> {
        self.call(format!("close-kept {name}"));
        *self.kept.lock().unwrap() = None;
        Ok(())
    }
}

fn place() -> Place {
    Place {
        machine: MACHINE.into(),
        engine: EngineKind::Openclaw,
        folder: "/tmp/one-session".into(),
    }
}

/// With no OpenClaw to delete sessions in: a deletion is a no-op.
fn session() -> OneSession {
    OneSession::new(
        Duration::from_secs(5),
        Duration::from_millis(5),
        Duration::from_secs(1),
    )
    .with_session_delete(Box::new(|_| Ok(())))
}

fn far() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

/// One whole turn for `id`: entered, typed, waited for.
fn one_turn(one: &OneSession, fake: &Fake, id: &str) -> Ended {
    let turn = one.enter(id, Kind::Skill, far(), &|| false).unwrap();
    assert!(
        matches!(
            turn.send(fake, &place(), "Run the skill.", far(), &|| false),
            Sent::Typed(_)
        ),
        "typed"
    );
    turn.wait_end(fake, MACHINE, &AtomicBool::new(false), &|| {})
}

#[test]
fn the_switch_is_on_exactly() {
    assert!(on(Some("on")));
    for off in [
        None,
        Some(""),
        Some("ON"),
        Some("on "),
        Some("1"),
        Some("true"),
    ] {
        assert!(!on(off), "{off:?}");
    }
}

/// The pane is opened at start, before any run, and its first turn
/// waited for; nothing is typed then.
#[test]
fn the_pane_is_opened_at_start_before_any_run() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    assert_eq!(one.agent(), Some(agent_of(1)));
    assert_eq!(fake.count("new"), 1);
    assert_eq!(fake.count("reply"), 0);
    // Open already: nothing more.
    one.open_if_due(&fake, &place());
    assert_eq!(fake.count("new"), 1);
    // The first run uses it.
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!(fake.count("new"), 1);
}

/// An open that failed is tried again only after its backoff.
#[test]
fn a_failed_open_is_tried_again_after_its_backoff() {
    let (one, fake) = (session(), Fake::default());
    *fake.failing_opens.lock().unwrap() = 1;
    one.open_if_due(&fake, &place());
    assert_eq!(one.agent(), None);
    one.open_if_due(&fake, &place());
    assert_eq!(fake.count("new"), 1, "within the backoff");
    one.lock().next_open = Some(Instant::now());
    one.open_if_due(&fake, &place());
    assert_eq!(one.agent(), Some(agent_of(1)));
}

/// Many runs: one pane, every turn typed into it after its header line,
/// never closed, each answer its own turn's.
#[test]
fn many_runs_share_one_pane_and_it_is_never_closed() {
    let (one, fake) = (session(), Fake::default());
    for n in 1..=12 {
        let id = format!("task-{n}");
        assert_eq!(
            one_turn(&one, &fake, &id),
            Ended::Answered(format!("answer to New task {id}:"))
        );
    }
    assert_eq!(fake.count("new"), 1);
    assert_eq!(fake.count("reply"), 12);
    assert_eq!(fake.count("close"), 0);
    assert_eq!(one.lock().pane.as_ref().map(|pane| pane.turns), Some(12));
    assert!(
        fake.calls()
            .iter()
            .filter(|call| call.starts_with("reply"))
            .all(|call| call == &format!("reply {}", agent_of(1)))
    );
}

/// The words typed start with the header line.
#[test]
fn each_turn_starts_with_its_header_line() {
    let (one, fake) = (session(), Fake::default());
    one_turn(&one, &fake, "task-7");
    let asked = fake
        .asked
        .lock()
        .unwrap()
        .get(&agent_of(1))
        .cloned()
        .unwrap();
    assert_eq!(asked, "New task task-7:\nRun the skill.");
}

/// A pane idle on another ask is waited on again: a task keeps only its
/// own turn's answer.
#[test]
fn an_idle_pane_on_another_ask_is_not_this_tasks_end() {
    let (one, fake) = (session(), Fake::default());
    *fake.other_asks.lock().unwrap() = 2;
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!(fake.count("show"), 3);
}

/// A run while another holds the pane waits, and runs once it is free.
#[test]
fn a_run_while_busy_waits_then_runs() {
    let (one, fake) = (Arc::new(session()), Arc::new(Fake::default()));
    let held = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    let (one2, fake2) = (one.clone(), fake.clone());
    let second = thread::spawn(move || one_turn(&one2, &fake2, "task-2"));
    let until = Instant::now() + Duration::from_secs(5);
    while one.waiting() == 0 {
        assert!(Instant::now() < until, "queued");
        thread::sleep(Duration::from_millis(1));
    }
    thread::sleep(Duration::from_millis(30));
    assert_eq!(fake.count("reply"), 0, "nothing typed while busy");
    drop(held);
    assert_eq!(
        second.join().unwrap(),
        Ended::Answered("answer to New task task-2:".into())
    );
}

/// Spoken tasks go ahead of skill runs; a queue full of skill runs is
/// busy at once for one more (Heron's H2: a spoken task never counts).
#[test]
fn spoken_tasks_go_first_and_a_full_queue_is_busy() {
    let one = Arc::new(session());
    let held = one.enter("task-0", Kind::Skill, far(), &|| false).unwrap();
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut waiting = Vec::new();
    for (id, kind) in [
        ("skill-1", Kind::Skill),
        ("skill-2", Kind::Skill),
        ("spoken-1", Kind::Spoken),
        ("skill-0", Kind::Skill),
    ] {
        let (mine, order) = (one.clone(), order.clone());
        let before = one.waiting();
        waiting.push(thread::spawn(move || {
            let turn = mine.enter(id, kind, far(), &|| false).unwrap();
            order.lock().unwrap().push(id);
            drop(turn);
        }));
        while one.waiting() == before {
            thread::sleep(Duration::from_millis(1));
        }
    }
    let (one4, order4) = (one.clone(), order.clone());
    waiting.push(thread::spawn(move || {
        let turn = one4
            .enter("skill-3", Kind::Skill, far(), &|| false)
            .unwrap();
        order4.lock().unwrap().push("skill-3");
        drop(turn);
    }));
    while one.waiting() < QUEUE_MOST + 1 {
        thread::sleep(Duration::from_millis(1));
    }
    let began = Instant::now();
    assert_eq!(
        one.enter("skill-4", Kind::Skill, far(), &|| false).err(),
        Some(Refused::Busy)
    );
    assert!(began.elapsed() < Duration::from_millis(100), "at once");
    drop(held);
    for thread in waiting {
        thread.join().unwrap();
    }
    assert_eq!(
        *order.lock().unwrap(),
        ["spoken-1", "skill-1", "skill-2", "skill-0", "skill-3"]
    );
}

/// A skill run waits its whole bound, then is busy.
#[test]
fn a_skill_run_past_its_wait_is_busy() {
    let one = OneSession::new(
        Duration::from_millis(80),
        Duration::from_millis(5),
        Duration::from_secs(1),
    );
    let _held = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert_eq!(
        one.enter("task-2", Kind::Skill, far(), &|| false).err(),
        Some(Refused::Busy)
    );
}

/// Harness says the pane is not running: reopened once, typed again.
#[test]
fn a_pane_not_running_is_reopened_once() {
    let (one, fake) = (session(), Fake::default());
    one_turn(&one, &fake, "task-1");
    fake.replies.lock().unwrap().push_back(Typed::NotRunning);
    assert_eq!(
        one_turn(&one, &fake, "task-2"),
        Ended::Answered("answer to New task task-2:".into())
    );
    assert_eq!(fake.count("new"), 2);
    assert_eq!(one.agent(), Some(agent_of(2)));
}

/// A second failure after the reopen sends that run the per-run way.
#[test]
fn a_second_failure_is_the_per_run_way() {
    let (one, fake) = (session(), Fake::default());
    fake.replies
        .lock()
        .unwrap()
        .extend([Typed::NotRunning, Typed::NotRunning]);
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run the skill.", far(), &|| false),
        Sent::NotTyped(_)
    ));
    assert_eq!(fake.count("new"), 2, "reopened once only");
}

/// A busy pane (`NOT_IDLE`) is waited for, never typed over.
#[test]
fn a_busy_pane_is_waited_for_then_typed() {
    let (one, fake) = (session(), Fake::default());
    fake.replies
        .lock()
        .unwrap()
        .extend([Typed::NotIdle, Typed::NotIdle]);
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!(fake.count("new"), 1);
    assert_eq!(fake.count("reply"), 3);
}

/// A cancel ends the turn: the pane closed `--now` (the token ended
/// first), and the next run opens a new one and goes on in it.
#[test]
fn a_cancel_closes_the_pane_and_the_next_run_reopens() {
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let seen = deleted.clone();
    let one = session().with_session_delete(Box::new(move |key| {
        seen.lock().unwrap().push(key.to_owned());
        Ok(())
    }));
    let fake = Fake::default();
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run the skill.", far(), &|| false),
        Sent::Typed(1)
    ));
    let token_ended = std::cell::Cell::new(None);
    let ended = turn.wait_end(&fake, MACHINE, &AtomicBool::new(true), &|| {
        token_ended.set(Some(fake.calls().len()))
    });
    drop(turn);
    assert_eq!(ended, Ended::Cancelled);
    assert_eq!(fake.count("close"), 1);
    assert_eq!(
        *deleted.lock().unwrap(),
        vec![format!("agent:main:harness-{}", agent_of(1))],
        "the cancelled pane's OpenClaw conversation deleted"
    );
    let closed_at = fake
        .calls()
        .iter()
        .position(|call| call.starts_with("close"))
        .unwrap();
    assert!(
        token_ended.get().is_some_and(|at| at <= closed_at),
        "token first"
    );
    assert_eq!(one.agent(), None);
    one_turn(&one, &fake, "task-2");
    one_turn(&one, &fake, "task-3");
    assert_eq!(fake.count("new"), 2);
    assert_eq!(fake.count("close"), 1);
    assert_eq!(one.agent(), Some(agent_of(2)));
}

/// Harness's daemon started again: the pane is let go, and reopened.
#[test]
fn a_daemon_restart_reopens_the_pane() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    one.forget("Harness's daemon restarted");
    assert_eq!(one.agent(), None);
    one.open_if_due(&fake, &place());
    assert_eq!(one.agent(), Some(agent_of(2)));
    assert_eq!(fake.count("close"), 0, "nothing left to close");
}

/// Another machine id or another default engine: the old pane closed
/// and a new one opened; the same place: kept.
#[test]
fn another_machine_or_engine_reopens_the_pane() {
    let (one, fake) = (session(), Fake::default());
    one_turn(&one, &fake, "task-1");
    one_turn(&one, &fake, "task-2");
    assert_eq!(fake.count("new"), 1);
    let mut other = place();
    other.machine = "fedcba9876543210fedcba9876543210".into();
    let turn = one.enter("task-3", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &other, "Run.", far(), &|| false),
        Sent::Typed(1)
    ));
    drop(turn);
    assert_eq!(fake.count("new"), 2);
    assert_eq!(fake.count("close"), 1, "the old pane closed");
}

#[test]
fn turns_are_counted_in_words() {
    assert_eq!(nth(1), "1st turn");
    assert_eq!(nth(2), "2nd turn");
    assert_eq!(nth(3), "3rd turn");
    assert_eq!(nth(11), "11th turn");
    assert_eq!(nth(22), "22nd turn");
    assert_eq!(nth(40), "40th turn");
}

fn kept_session(dir: &std::path::Path) -> OneSession {
    session().with_state_file(dir.join(STATE_FILE))
}

/// The pane's id from its `new` is kept, and the next start (a new
/// lamp-tasks, the same file) adopts it by that id when it is live,
/// with no `new`; never by name.
#[test]
fn the_pane_is_adopted_by_its_kept_id() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = (kept_session(dir.path()), Fake::default());
    one.open_if_due(&fake, &place());
    assert_eq!(fake.calls()[0], "new lamp-one-openclaw");
    let kept: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(STATE_FILE)).unwrap())
            .unwrap();
    assert_eq!(kept["agent"], agent_of(1));
    for state in ["idle", "working", "offline"] {
        let (again, fake) = (kept_session(dir.path()), Fake::default());
        fake.states
            .lock()
            .unwrap()
            .insert(agent_of(1), state.into());
        again.open_if_due(&fake, &place());
        assert_eq!(again.agent(), Some(agent_of(1)), "{state}");
        assert_eq!(fake.count("new"), 0, "{state}");
        assert_eq!(fake.count("resume"), 0, "{state}");
    }
    // Another engine's kept pane is not adopted.
    let (again, fake) = (kept_session(dir.path()), Fake::default());
    let mut hermes = place();
    hermes.engine = EngineKind::Hermes;
    again.open_if_due(&fake, &hermes);
    assert_eq!(fake.count("new"), 1);
}

/// A kept pane stopped or gone is resumed once; resumed live, it is
/// adopted; else a fresh `new`.
#[test]
fn a_kept_pane_stopped_or_gone_is_resumed_once_then_opened_anew() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = (kept_session(dir.path()), Fake::default());
    one.open_if_due(&fake, &place());
    // Stopped, resumed idle: adopted.
    let (again, fake) = (kept_session(dir.path()), Fake::default());
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "stopped".into());
    *fake.resumed.lock().unwrap() = Some("idle".into());
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(1)));
    assert_eq!((fake.count("resume"), fake.count("new")), (1, 0));
    // Gone, not resumed: a new pane, kept in its place.
    let (again, fake) = (kept_session(dir.path()), Fake::default());
    fake.gone.lock().unwrap().push(agent_of(1));
    *fake.opens.lock().unwrap() = 1;
    again.open_if_due(&fake, &place());
    assert_eq!((fake.count("resume"), fake.count("new")), (1, 1));
    assert_eq!(again.agent(), Some(agent_of(2)));
    let kept = std::fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    assert!(kept.contains(&agent_of(2)), "{kept}");
}

/// The next words are typed only once the last were taken: a pane idle
/// with untaken words is waited for, never typed over.
#[test]
fn words_are_typed_only_after_the_last_were_taken() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    *fake.untaken.lock().unwrap() = 3;
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    let calls = fake.calls();
    let reply = calls
        .iter()
        .position(|call| call.starts_with("reply"))
        .unwrap();
    let shows_before = calls[..reply]
        .iter()
        .filter(|call| call.starts_with("show"))
        .count();
    assert_eq!(shows_before, 4, "three untaken, then taken: {calls:?}");
    // Working: waited for too.
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "working".into());
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    let soon = Instant::now() + Duration::from_millis(60);
    assert!(matches!(
        turn.send(&fake, &place(), "Run.", soon, &|| false),
        Sent::NotTyped(_)
    ));
    assert_eq!(fake.count("reply"), 0, "never typed while working");
}

/// A pane Harness no longer has (`show`: `UNKNOWN_HARNESS`, as after a
/// user-manager restart killed every pane) is reopened, not waited on; one
/// `show` reads offline is waited on, never reopened.
#[test]
fn a_pane_harness_no_longer_has_is_reopened() {
    let (one, fake) = (session(), Fake::default());
    one_turn(&one, &fake, "task-1");
    fake.gone.lock().unwrap().push(agent_of(1));
    assert_eq!(
        one_turn(&one, &fake, "task-2"),
        Ended::Answered("answer to New task task-2:".into())
    );
    assert_eq!(one.agent(), Some(agent_of(2)));
    let (one, fake) = (session(), Fake::default());
    one_turn(&one, &fake, "task-1");
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "offline".into());
    let turn = one.enter("task-2", Kind::Skill, far(), &|| false).unwrap();
    let soon = Instant::now() + Duration::from_millis(60);
    assert!(matches!(
        turn.send(&fake, &place(), "Run.", soon, &|| false),
        Sent::NotTyped(_)
    ));
    assert_eq!(fake.count("new"), 1, "not reopened");
}

/// A turn that asks the person something ends that task as today; the
/// pane is closed `--now` and opened again, so the ask never holds every
/// later turn.
#[test]
fn a_turn_that_asks_ends_its_task_and_the_pane_is_reopened() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    fake.waits.lock().unwrap().push_back("needs-input".into());
    let ended = one_turn(&one, &fake, "task-1");
    assert!(
        matches!(ended, Ended::Failed(ref why) if why.contains("stopped to ask")),
        "{ended:?}"
    );
    assert_eq!(fake.count("close"), 1);
    assert_eq!(one.agent(), None);
    one_turn(&one, &fake, "task-2");
    assert_eq!(one.agent(), Some(agent_of(2)));
}

/// Harness says the pane stopped at a turn's end: let go, reopened next.
#[test]
fn a_pane_that_stopped_is_reopened_at_the_next_run() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    fake.waits.lock().unwrap().push_back("stopped".into());
    assert!(matches!(one_turn(&one, &fake, "task-1"), Ended::Failed(_)));
    assert_eq!(one.agent(), None);
    one_turn(&one, &fake, "task-2");
    assert_eq!(one.agent(), Some(agent_of(2)));
}

/// Removed for good (the default engine changed, a sign-out): closed
/// `--now`, its OpenClaw session deleted by its key, its kept id deleted,
/// so the next start opens a new pane rather than adopting it.
#[test]
fn a_dropped_pane_is_closed_its_session_deleted_and_its_id_forgotten() {
    let dir = tempfile::tempdir().unwrap();
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let seen = deleted.clone();
    let one = kept_session(dir.path()).with_session_delete(Box::new(move |key| {
        seen.lock().unwrap().push(key.to_owned());
        Ok(())
    }));
    let fake = Fake::default();
    one.open_if_due(&fake, &place());
    one.drop_pane(&fake, "a sign-out");
    assert_eq!(fake.count("close"), 1);
    assert_eq!(
        *deleted.lock().unwrap(),
        vec![format!("agent:main:harness-{}", agent_of(1))]
    );
    assert!(!dir.path().join(STATE_FILE).exists());
    assert_eq!(one.agent(), None);
    let (again, fake) = (kept_session(dir.path()), Fake::default());
    again.open_if_due(&fake, &place());
    assert_eq!((fake.count("resume"), fake.count("new")), (0, 1));
}

/// A daemon restart only forgets the pane: nothing is closed or deleted,
/// and its kept id is adopted again.
#[test]
fn a_daemon_restart_keeps_the_pane_and_its_session() {
    let dir = tempfile::tempdir().unwrap();
    let one = kept_session(dir.path())
        .with_session_delete(Box::new(|_| panic!("never deleted on a restart")));
    let fake = Fake::default();
    one.open_if_due(&fake, &place());
    one.forget("Harness's daemon restarted");
    one.open_if_due(&fake, &place());
    assert_eq!(fake.count("close"), 0);
    assert_eq!(fake.count("new"), 1, "adopted by its id");
    assert_eq!(one.agent(), Some(agent_of(1)));
}

/// The default engine or machine changed while a pane is held idle: the
/// old one is removed for good, then a new one opened.
#[test]
fn a_moved_place_removes_the_old_pane() {
    let deleted = Arc::new(Mutex::new(0));
    let seen = deleted.clone();
    let one = session().with_session_delete(Box::new(move |_| {
        *seen.lock().unwrap() += 1;
        Ok(())
    }));
    let fake = Fake::default();
    one.tend(&fake, &place(), &|| false);
    let mut other = place();
    other.machine = "fedcba9876543210fedcba9876543210".into();
    one.tend(&fake, &other, &|| false);
    assert_eq!((fake.count("close"), fake.count("new")), (1, 2));
    assert_eq!(*deleted.lock().unwrap(), 1);
}

/// Only the machine or the engine moves the pane: the same machine and
/// engine with another folder keeps it; another engine still removes it.
#[test]
fn a_place_with_the_same_machine_and_engine_keeps_the_pane() {
    let one = session().with_session_delete(Box::new(|_| Ok(())));
    let fake = Fake::default();
    one.tend(&fake, &place(), &|| false);
    let mut folder = place();
    folder.folder = "/elsewhere".into();
    one.tend(&fake, &folder, &|| false);
    assert_eq!((fake.count("close"), fake.count("new")), (0, 1), "kept");
    let mut hermes = place();
    hermes.engine = EngineKind::Hermes;
    one.tend(&fake, &hermes, &|| false);
    assert_eq!((fake.count("close"), fake.count("new")), (1, 2), "moved");
}

/// No overflow session: a spoken task behind a skill run waits in the
/// queue, however long, and runs once the pane is free.
#[test]
fn a_spoken_task_waits_in_the_queue_with_no_overflow() {
    let one = Arc::new(session());
    let held = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    let waiter = one.clone();
    let spoken = thread::spawn(move || {
        waiter
            .enter("spoken-1", Kind::Spoken, far(), &|| false)
            .map(|_| ())
    });
    thread::sleep(Duration::from_millis(100));
    assert_eq!(one.waiting(), 1, "still waiting");
    drop(held);
    assert_eq!(spoken.join().unwrap(), Ok(()));
}

fn spoken(name: &str) -> Launch {
    Launch {
        name: name.into(),
        prompt: "Turn the light blue.".into(),
        target: Target {
            machine_id: MACHINE.into(),
            project: "/tmp".into(),
            engine: EngineKind::Openclaw,
        },
        session: None,
    }
}

/// A spoken task for the lamp is typed into the pane after its header
/// (`Joined`: the store never closes it), and holds the pane until the
/// store has its answer: a skill run waits until it is released.
#[test]
fn a_spoken_task_is_typed_into_the_pane_and_holds_it_until_released() {
    let (one, fake) = (Arc::new(session()), Fake::default());
    one.open_if_due(&fake, &place());
    assert_eq!(
        spoken_launch(&one, &fake, &spoken("lamp-7")),
        Some(LaunchOutcome::Joined {
            agent_id: agent_of(1),
            session: None
        })
    );
    assert!(fake.asked.lock().unwrap()[&agent_of(1)].starts_with("New task lamp-7:\n"));
    assert_eq!(one.spoken_holder().as_deref(), Some("lamp-7"));
    assert!(!one.free());
    let waiter = one.clone();
    let run = thread::spawn(move || {
        waiter
            .enter("task-1", Kind::Skill, far(), &|| false)
            .map(|_| ())
    });
    thread::sleep(Duration::from_millis(50));
    assert_eq!(one.waiting(), 1);
    one.release("lamp-7");
    assert_eq!(run.join().unwrap(), Ok(()));
    assert_eq!(fake.count("close"), 0);
    // Not the lamp's machine, or a named session: the per-run way.
    let mut other = spoken("lamp-8");
    other.target.machine_id = "fedcba9876543210fedcba9876543210".into();
    assert_eq!(spoken_launch(&one, &fake, &other), None);
    let mut named = spoken("lamp-9");
    named.session = Some("docs".into());
    assert_eq!(spoken_launch(&one, &fake, &named), None);
}

/// The store's spoken tasks queued for the pane go before a skill run,
/// and each is told its place.
#[test]
fn queued_spoken_tasks_go_first_and_know_their_place() {
    let one = Arc::new(session());
    one.set_spoken_queue(vec!["lamp-1".into(), "lamp-2".into()]);
    assert_eq!(one.queued_place("lamp-1"), Some(1));
    assert_eq!(one.queued_place("lamp-2"), Some(2));
    assert_eq!(one.queued_place("lamp-3"), None);
    let waiter = one.clone();
    let run = thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(5);
        waiter
            .enter("task-1", Kind::Skill, until, &|| false)
            .map(|_| ())
    });
    thread::sleep(Duration::from_millis(50));
    assert_eq!(one.waiting(), 1, "behind the store's spoken tasks");
    one.set_spoken_queue(Vec::new());
    assert_eq!(run.join().unwrap(), Ok(()));
}

/// OpenClaw compacts its own sessions, so its pane is never rotated by
/// count: past [`ROTATE_TURNS`] turns and a 250k-token answer it is still
/// the one pane. Only an engine with no compaction (Hermes) has the bounds.
#[test]
fn an_openclaw_pane_is_never_rotated_by_count() {
    assert_eq!((ROTATE_TURNS, ROTATE_TOKENS), (40, 250_000));
    assert!(!rotation_due(EngineKind::Openclaw, u64::MAX, u64::MAX));
    assert!(rotation_due(EngineKind::Hermes, ROTATE_TURNS, 0));
    assert!(rotation_due(EngineKind::Hermes, 0, ROTATE_TOKENS));
    assert!(!rotation_due(EngineKind::Hermes, ROTATE_TURNS - 1, ROTATE_TOKENS - 1));
    let dir = tempfile::tempdir().unwrap();
    let one = kept_session(dir.path()).with_session_delete(Box::new(|_| Ok(())));
    let fake = Fake::default();
    for n in 0..=ROTATE_TURNS + 5 {
        one_turn(&one, &fake, &format!("task-{n}"));
    }
    one.count_answer(&"y".repeat(1_000_001));
    one_turn(&one, &fake, "after");
    assert_eq!(fake.count("new"), 1, "the same pane all along");
    assert_eq!(fake.count("close"), 0);
    assert_eq!(one.agent(), Some(agent_of(1)));
}

/// The open's first-turn wait ends with the run's own end, and with
/// lamp-tasks' stop.
#[test]
fn the_opens_first_turn_wait_is_bounded_by_the_run_and_the_stop() {
    let fake = Fake::default();
    for _ in 0..100_000 {
        fake.waits.lock().unwrap().push_back("none".into());
    }
    let one = OneSession::new(
        Duration::from_secs(5),
        Duration::from_millis(5),
        Duration::from_secs(60),
    );
    let began = Instant::now();
    one.open_if_due_while(&fake, &place(), &|| {
        began.elapsed() > Duration::from_millis(50)
    });
    assert!(began.elapsed() < Duration::from_secs(5), "stopped");
    let one = OneSession::new(
        Duration::from_secs(5),
        Duration::from_millis(5),
        Duration::from_secs(60),
    );
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    let began = Instant::now();
    let soon = began + Duration::from_millis(100);
    let _ = turn.send(&fake, &place(), "Run.", soon, &|| false);
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "the run's own end"
    );
}

/// A sign-out removes the pane at the next look, once nothing holds it:
/// closed `--now`, its OpenClaw session and kept id deleted.
#[test]
fn a_sign_out_removes_the_pane_once_free() {
    let dir = tempfile::tempdir().unwrap();
    let deleted = Arc::new(Mutex::new(0));
    let seen = deleted.clone();
    let one = kept_session(dir.path()).with_session_delete(Box::new(move |_| {
        *seen.lock().unwrap() += 1;
        Ok(())
    }));
    let fake = Fake::default();
    one.tend(&fake, &place(), &|| false);
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    one.drop_when_free("the lamp signed out");
    assert!(!one.drop_if_due(&fake), "not while a turn holds it");
    drop(turn);
    one.tend(&fake, &place(), &|| false);
    assert_eq!(fake.count("close"), 1);
    assert_eq!(*deleted.lock().unwrap(), 1);
    assert!(!dir.path().join(STATE_FILE).exists());
    assert_eq!(one.agent(), None);
}

impl PaneHarness for &Fake {
    fn open(&self, launch: &Launch) -> LaunchOutcome {
        (**self).open(launch)
    }
    fn type_into(&self, machine: &str, agent: &str, words: &str) -> Typed {
        (**self).type_into(machine, agent, words)
    }
    fn wait_end(
        &self,
        m: &str,
        agent: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<String>, String> {
        (**self).wait_end(m, agent, cancel)
    }
    fn observe(&self, machine: &str, agent: &str) -> Result<PaneSeen, String> {
        (**self).observe(machine, agent)
    }
    fn close_now(&self, machine: &str, agent: &str) -> Result<Closed, String> {
        (**self).close_now(machine, agent)
    }
    fn resume(&self, machine: &str, agent: &str) -> Result<String, String> {
        (**self).resume(machine, agent)
    }
    fn live_in(&self, machine: &str, folder: &str) -> Result<Vec<String>, String> {
        (**self).live_in(machine, folder)
    }
}

/// No agent ready (a fresh or signed-out lamp, OpenClaw not set up): no
/// pane is opened and the lamp is looked at once, not at every tick; again
/// only after the long backoff, or at once after a trigger.
#[test]
fn with_no_agent_ready_nothing_is_opened_and_it_is_not_looked_at_each_tick() {
    let (one, fake) = (session(), Fake::default());
    let probes = std::cell::Cell::new(0);
    let place = || -> Result<(&Fake, Place), String> {
        probes.set(probes.get() + 1);
        Err("the lamp's own machine is not known".into())
    };
    let mut looks = Looks::default();
    let start = Instant::now();
    let key = || Some(" openclaw".to_owned());
    // 25 ticks of KEEP_OPEN_EVERY: 50 s, within NOT_READY_FIRST.
    for tick in 0..25 {
        one.look(&mut looks, start + KEEP_OPEN_EVERY * tick, key(), &place);
    }
    assert_eq!(probes.get(), 1, "one look in 25 ticks");
    assert_eq!(fake.count("new"), 0);
    // Past the first backoff: once more, then the gap doubles.
    let later = start + NOT_READY_FIRST + Duration::from_secs(1);
    one.look(&mut looks, later, key(), &place);
    one.look(&mut looks, later + NOT_READY_FIRST, key(), &place);
    assert_eq!(probes.get(), 2, "the gap doubled");
    // A trigger: Harness's daemon started again, then the key changed.
    one.forget("Harness's daemon started");
    one.look(&mut looks, later, key(), &place);
    assert_eq!(probes.get(), 3, "looked at once after forget");
    one.look(&mut looks, later, Some("m openclaw".into()), &place);
    assert_eq!(probes.get(), 4, "looked at once after the key changed");
    one.look(&mut looks, later, Some("m openclaw".into()), &place);
    assert_eq!(probes.get(), 4);
    assert_eq!(fake.count("new"), 0, "{:?}", fake.calls());
    assert_eq!(one.agent(), None);
}

/// Ready: opened at the first look; a held pane is never looked for again.
#[test]
fn a_held_pane_is_not_looked_for() {
    let (one, fake) = (session(), Fake::default());
    let probes = std::cell::Cell::new(0);
    let place = || -> Result<(&Fake, Place), String> {
        probes.set(probes.get() + 1);
        Ok((&fake, place()))
    };
    let mut looks = Looks::default();
    let start = Instant::now();
    for tick in 0..10 {
        one.look(&mut looks, start + KEEP_OPEN_EVERY * tick, None, &place);
    }
    assert_eq!(one.agent(), Some(agent_of(1)));
    assert_eq!((probes.get(), fake.count("new")), (1, 1));
}

/// The lamp's machine or default engine changed since the pane was
/// opened: removed for good at the next look, once nothing holds it.
#[test]
fn a_changed_lamp_key_removes_the_open_pane() {
    let deleted = Arc::new(Mutex::new(0));
    let seen = deleted.clone();
    let one = session().with_session_delete(Box::new(move |_| {
        *seen.lock().unwrap() += 1;
        Ok(())
    }));
    let fake = Fake::default();
    let place = || -> Result<(&Fake, Place), String> { Ok((&fake, place())) };
    let harness = || Some(&fake);
    let mut looks = Looks::default();
    let key = Some("m openclaw".to_owned());
    one.remove_if_due(&looks, key.as_ref(), false, &harness);
    one.look(&mut looks, Instant::now(), key.clone(), &place);
    assert_eq!(one.agent(), Some(agent_of(1)));
    one.remove_if_due(&looks, key.as_ref(), false, &harness);
    assert_eq!(fake.count("close"), 0, "the same key: kept");
    let hermes = Some("m hermes".to_owned());
    one.remove_if_due(&looks, hermes.as_ref(), false, &harness);
    assert_eq!(fake.count("close"), 1);
    assert_eq!(*deleted.lock().unwrap(), 1);
    assert_eq!(one.agent(), None);
}

/// A lamp key read unknown for one look (Harness not set up, the machine
/// list failed: no key, or a blank machine or engine) is not a move: the
/// open pane is kept, then a real move still removes it.
#[test]
fn an_unknown_lamp_key_keeps_the_open_pane() {
    let one = session().with_session_delete(Box::new(|_| Ok(())));
    let fake = Fake::default();
    let place = || -> Result<(&Fake, Place), String> { Ok((&fake, place())) };
    let harness = || Some(&fake);
    let mut looks = Looks::default();
    let key = Some(format!("{MACHINE} openclaw"));
    one.look(&mut looks, Instant::now(), key.clone(), &place);
    assert_eq!(one.agent(), Some(agent_of(1)));
    for unknown in [
        None,
        Some(" openclaw".to_owned()),
        Some(format!("{MACHINE} ")),
        Some(" ".to_owned()),
    ] {
        one.remove_if_due(&looks, unknown.as_ref(), false, &harness);
        one.look(&mut looks, Instant::now(), unknown, &place);
    }
    one.remove_if_due(&looks, key.as_ref(), false, &harness);
    assert_eq!((fake.count("close"), fake.count("new")), (0, 1), "kept");
    assert_eq!(one.agent(), Some(agent_of(1)));
    let other = Some("fedcba9876543210fedcba9876543210 openclaw".to_owned());
    one.remove_if_due(&looks, other.as_ref(), false, &harness);
    assert_eq!(fake.count("close"), 1, "another known machine: removed");
}

/// #7246 (the Reviewer-lead's #4): a spoken task and a skill run that
/// arrive together share the one pane: exactly ONE `new`, never a second
/// session, the pane not open before either. The spoken task opens none
/// (the Reviewer-lead's #3 on #7258): it is `busy`, put back, and goes into
/// the pane the skill run opened.
#[test]
fn a_spoken_task_and_a_skill_run_together_make_one_new() {
    let (one, fake) = (Arc::new(session()), Arc::new(Fake::default()));
    one.knows(&place());
    assert_eq!(
        spoken_launch(&one, &*fake, &spoken("lamp-1")),
        Some(LaunchOutcome::NotSent {
            code: "busy".into()
        })
    );
    assert_eq!(fake.count("new"), 0, "the spoken path opens none");
    let until = Instant::now() + Duration::from_secs(5);
    let turn = one.enter("task-1", Kind::Skill, until, &|| false).unwrap();
    assert!(matches!(
        turn.send(&*fake, &place(), "Run.", until, &|| false),
        Sent::Typed(_)
    ));
    drop(turn);
    let joined = spoken_launch(&one, &*fake, &spoken("lamp-1"));
    assert!(
        matches!(joined, Some(LaunchOutcome::Joined { .. })),
        "{joined:?}"
    );
    one.release("lamp-1");
    assert_eq!(fake.count("new"), 1, "{:?}", fake.calls());
    assert_eq!(fake.count("reply"), 2);
}

/// A pane that stays busy until the task's end fails the task: no
/// second session (`None` would send it its own `new`).
#[test]
fn a_busy_pane_never_sends_a_spoken_task_its_own_way() {
    let one = OneSession::new(
        Duration::from_millis(100),
        Duration::from_millis(5),
        Duration::from_secs(1),
    );
    let fake = Fake::default();
    one.open_if_due(&fake, &place());
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "working".into());
    assert_eq!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::NotSent {
            code: "busy".into()
        })
    );
    assert_eq!(fake.count("new"), 1);
}

/// The Reviewer-lead's #2: a pane closed after a needs-input turn or a
/// cancel has its OpenClaw conversation deleted and its kept id removed,
/// so a reopen's new id is the only one kept and no transcript is left.
#[test]
fn a_closed_turns_pane_has_its_conversation_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let seen = deleted.clone();
    let one = kept_session(dir.path()).with_session_delete(Box::new(move |key| {
        seen.lock().unwrap().push(key.to_owned());
        Ok(())
    }));
    let fake = Fake::default();
    one.open_if_due(&fake, &place());
    fake.waits.lock().unwrap().push_back("needs-input".into());
    one_turn(&one, &fake, "task-1");
    assert_eq!(
        *deleted.lock().unwrap(),
        vec![format!("agent:main:harness-{}", agent_of(1))]
    );
    assert!(!dir.path().join(STATE_FILE).exists());
    one_turn(&one, &fake, "task-2");
    let kept = std::fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    assert!(
        kept.contains(&agent_of(2)) && !kept.contains(&agent_of(1)),
        "{kept}"
    );
}

/// The Reviewer-lead's #3: after a lamp-tasks restart (no pane in
/// memory), an engine switch still removes the pane kept on disk: closed,
/// its conversation deleted, its id file gone.
#[test]
fn an_engine_switch_after_a_restart_removes_the_kept_pane() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = (kept_session(dir.path()), Fake::default());
    one.open_if_due(&fake, &place());
    let deleted = Arc::new(Mutex::new(0));
    let seen = deleted.clone();
    let again = kept_session(dir.path()).with_session_delete(Box::new(move |_| {
        *seen.lock().unwrap() += 1;
        Ok(())
    }));
    let fake = Fake::default();
    let harness = || Some(&fake);
    let looks = Looks::default();
    // Not known yet: kept.
    again.remove_if_due(&looks, Some(&" ".to_owned()), false, &harness);
    // The same machine and engine: kept.
    again.remove_if_due(
        &looks,
        Some(&format!("{MACHINE} openclaw")),
        false,
        &harness,
    );
    assert_eq!(fake.count("close"), 0);
    again.remove_if_due(&looks, Some(&format!("{MACHINE} hermes")), false, &harness);
    assert_eq!(fake.calls(), vec![format!("close {}", agent_of(1))]);
    assert_eq!(*deleted.lock().unwrap(), 1);
    assert!(!dir.path().join(STATE_FILE).exists());
}

/// Twelve's B3: a pane kept while the lamp is signed out at a start (a
/// sign-out while lamp-tasks was down) is removed: closed, its
/// conversation deleted, its id file gone.
#[test]
fn a_start_signed_out_removes_the_kept_pane() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = (kept_session(dir.path()), Fake::default());
    one.open_if_due(&fake, &place());
    let deleted = Arc::new(Mutex::new(0));
    let seen = deleted.clone();
    let again = kept_session(dir.path()).with_session_delete(Box::new(move |_| {
        *seen.lock().unwrap() += 1;
        Ok(())
    }));
    let fake = Fake::default();
    let harness = || Some(&fake);
    let key = Some(format!("{MACHINE} openclaw"));
    again.remove_if_due(&Looks::default(), key.as_ref(), false, &harness);
    assert_eq!(fake.count("close"), 0, "signed in: kept");
    again.remove_if_due(&Looks::default(), key.as_ref(), true, &harness);
    assert_eq!(fake.count("close"), 1);
    assert_eq!(*deleted.lock().unwrap(), 1);
    assert!(!dir.path().join(STATE_FILE).exists());
}

/// Twelve's B2: with the pane let go (a daemon start, a cancel), a
/// spoken task for the lamp still goes into the one pane, never the
/// per-run way: `busy` (put back) until the one-session thread reopened it
/// (the Reviewer-lead's #3 on #7258), then typed into.
#[test]
fn a_spoken_task_with_the_pane_let_go_opens_the_one_pane() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    one.forget("Harness's daemon started");
    fake.gone.lock().unwrap().push(agent_of(1));
    assert_eq!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::NotSent {
            code: "busy".into()
        })
    );
    assert_eq!(fake.count("new"), 1, "none on the spoken path");
    one.open_if_due(&fake, &place());
    assert!(matches!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::Joined { .. })
    ));
    assert_eq!(
        fake.count("new"),
        2,
        "the first pane, then the one reopened"
    );
    assert_eq!(one.agent(), Some(agent_of(2)));
}

/// The Reviewer-lead's HOLD 1 on #7258: a kept pane that is not taken
/// back (gone, not resumed) is closed and its OpenClaw conversation
/// deleted BEFORE the fresh `new`, so its words never stay behind and
/// it never runs beside the new pane.
#[test]
fn a_kept_pane_not_taken_back_is_deleted_before_the_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = (kept_session(dir.path()), Fake::default());
    one.open_if_due(&fake, &place());
    let fake = Arc::new(Fake::default());
    let log = fake.clone();
    let again = kept_session(dir.path()).with_session_delete(Box::new(move |key| {
        log.call(format!("delete {key}"));
        Ok(())
    }));
    fake.gone.lock().unwrap().push(agent_of(1));
    *fake.opens.lock().unwrap() = 1;
    again.open_if_due(&*fake, &place());
    let calls = fake.calls();
    let deleted = calls
        .iter()
        .position(|call| *call == format!("delete agent:main:harness-{}", agent_of(1)))
        .expect("the old pane's conversation deleted");
    let closed = calls
        .iter()
        .position(|call| *call == format!("close {}", agent_of(1)))
        .expect("the old pane closed");
    let new = calls
        .iter()
        .position(|call| call.starts_with("new"))
        .unwrap();
    assert!(closed < new && deleted < new, "{calls:?}");
    assert_eq!(again.agent(), Some(agent_of(2)));
    let kept = std::fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    assert!(kept.contains(&agent_of(2)), "{kept}");
}

/// Heron's P2 on #7258: a kept pane not taken back whose close fails on
/// this machine (it may still be live) is backed off and tried again,
/// never a `new` beside it, its record and conversation kept until it is
/// closed.
#[test]
fn a_kept_pane_whose_close_fails_backs_off_with_no_new() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = (kept_session(dir.path()), Fake::default());
    one.open_if_due(&fake, &place());
    let fake = Arc::new(Fake::default());
    let log = fake.clone();
    let again = kept_session(dir.path()).with_session_delete(Box::new(move |key| {
        log.call(format!("delete {key}"));
        Ok(())
    }));
    fake.gone.lock().unwrap().push(agent_of(1));
    *fake.opens.lock().unwrap() = 1;
    *fake.failing_closes.lock().unwrap() = 1;
    again.open_if_due(&*fake, &place());
    assert_eq!(fake.count("close"), 1, "{:?}", fake.calls());
    assert_eq!(fake.count("new"), 0, "{:?}", fake.calls());
    assert_eq!(fake.count("delete"), 0, "{:?}", fake.calls());
    let kept = std::fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    assert!(kept.contains(&agent_of(1)), "{kept}");
    // Within the backoff: nothing tried.
    again.open_if_due(&*fake, &place());
    assert_eq!(fake.count("close"), 1, "{:?}", fake.calls());
}

/// The lamp's journal on 16 Nov (#7246): a kept pane Harness no longer
/// has (`show` and `resume` fail) whose close Harness answers
/// `UNKNOWN_HARNESS` is already gone: dropped, record and conversation
/// too, and exactly one new pane opened. Any other close failure still
/// backs off with no `new`.
#[test]
fn a_kept_pane_whose_close_says_unknown_harness_is_dropped_for_one_new() {
    for (code, news) in [("UNKNOWN_HARNESS", 1), ("close_failed", 0)] {
        let dir = tempfile::tempdir().unwrap();
        let (one, fake) = (kept_session(dir.path()), Fake::default());
        one.open_if_due(&fake, &place());
        let fake = Arc::new(Fake::default());
        let log = fake.clone();
        let again = kept_session(dir.path()).with_session_delete(Box::new(move |key| {
            log.call(format!("delete {key}"));
            Ok(())
        }));
        fake.gone.lock().unwrap().push(agent_of(1));
        *fake.opens.lock().unwrap() = 1;
        *fake.failing_closes.lock().unwrap() = 1;
        *fake.close_error.lock().unwrap() = Some(code.into());
        again.open_if_due(&*fake, &place());
        let calls = fake.calls();
        assert_eq!(fake.count("resume"), 1, "{code}: {calls:?}");
        assert_eq!(fake.count("close"), 1, "{code}: {calls:?}");
        assert_eq!(fake.count("new"), news, "{code}: {calls:?}");
        assert_eq!(fake.count("delete"), news, "{code}: {calls:?}");
        let kept = std::fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
        if news == 1 {
            assert_eq!(again.agent(), Some(agent_of(2)), "{code}");
            assert!(
                kept.contains(&agent_of(2)) && !kept.contains(&agent_of(1)),
                "{kept}"
            );
        } else {
            assert_eq!(again.agent(), None, "{code}");
            assert!(kept.contains(&agent_of(1)), "{kept}");
        }
        // Again at once: no second `new` and no second close.
        again.open_if_due(&*fake, &place());
        assert_eq!(fake.count("new"), news, "{code}: {:?}", fake.calls());
    }
}

/// The Reviewer-lead's HOLD 2 on #7258: an uncertain `harness new` that
/// did make the pane is found in its folder and kept; a send after it
/// makes no second `new`.
#[test]
fn an_uncertain_new_is_found_and_kept_never_made_twice() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = (kept_session(dir.path()), Fake::default());
    *fake.uncertain_opens.lock().unwrap() = 1;
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!(fake.count("new"), 1, "{:?}", fake.calls());
    assert_eq!(one.agent(), Some(agent_of(1)));
    let kept = std::fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    assert!(kept.contains(&agent_of(1)), "{kept}");
}

/// The ship lamp runs signed out for good (#6407): a pane opened signed
/// out is adopted at a start still signed out, never thrown away; one
/// opened signed in is removed at a start signed out.
#[test]
fn a_pane_opened_signed_out_is_kept_at_a_start_signed_out() {
    let dir = tempfile::tempdir().unwrap();
    let key = Some(format!("{MACHINE} openclaw"));
    let none = || None::<&Fake>;
    let one = kept_session(dir.path());
    one.remove_if_due(&Looks::default(), key.as_ref(), true, &none);
    one.open_if_due(&Fake::default(), &place());
    let again = kept_session(dir.path());
    let fake = Fake::default();
    let harness = || Some(&fake);
    again.remove_if_due(&Looks::default(), key.as_ref(), true, &harness);
    assert_eq!(fake.count("close"), 0, "{:?}", fake.calls());
    assert!(dir.path().join(STATE_FILE).exists());
    again.open_if_due(&fake, &place());
    assert_eq!(fake.count("new"), 0, "adopted: {:?}", fake.calls());
    assert_eq!(again.agent(), Some(agent_of(1)));
}

/// The Reviewer-lead's P1 on #7258: a pane opened signed out, then used
/// while an account is signed in, is kept as signed in; a start signed
/// out (a sign-out while lamp-tasks was down) removes it: closed, its
/// conversation deleted, its id file gone, never adopted.
#[test]
fn a_pane_opened_signed_out_and_used_signed_in_is_removed_at_a_start_signed_out() {
    let dir = tempfile::tempdir().unwrap();
    let key = Some(format!("{MACHINE} openclaw"));
    let none = || None::<&Fake>;
    let one = kept_session(dir.path());
    let fake = Fake::default();
    one.remove_if_due(&Looks::default(), key.as_ref(), true, &none);
    one.open_if_due(&fake, &place());
    one.remove_if_due(&Looks::default(), key.as_ref(), false, &none);
    one_turn(&one, &fake, "task-1");
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let seen = deleted.clone();
    let again = kept_session(dir.path()).with_session_delete(Box::new(move |key| {
        seen.lock().unwrap().push(key.to_owned());
        Ok(())
    }));
    let fake = Fake::default();
    let harness = || Some(&fake);
    again.remove_if_due(&Looks::default(), key.as_ref(), true, &harness);
    assert_eq!(fake.calls(), vec![format!("close {}", agent_of(1))]);
    assert_eq!(
        *deleted.lock().unwrap(),
        vec![format!("agent:main:harness-{}", agent_of(1))]
    );
    assert!(!dir.path().join(STATE_FILE).exists());
    again.open_if_due(&fake, &place());
    assert_eq!(fake.count("new"), 1, "not adopted: {:?}", fake.calls());
}

/// Harness changes the lamp's own machine id at a sign-in (the computer
/// id signed out, the account's machine id signed in) and back at a
/// sign-out. The place is read again at each open: a pane kept on the old
/// id, in memory or on disk after a restart, is closed there and its
/// conversation deleted, and a new one is opened and kept on the current
/// id; never adopted.
#[test]
fn a_pane_on_the_old_machine_id_is_removed_and_a_new_one_opened_on_the_current() {
    const ACCOUNT: &str = "fedcba9876543210fedcba9876543210";
    let mut account = place();
    account.machine = ACCOUNT.into();
    for restart in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let deleted = Arc::new(Mutex::new(Vec::new()));
        let seen = deleted.clone();
        let delete: SessionDelete = Box::new(move |key| {
            seen.lock().unwrap().push(key.to_owned());
            Ok(())
        });
        let one = kept_session(dir.path());
        let fake = Fake::default();
        one.open_if_due(&fake, &place());
        let one = if restart {
            kept_session(dir.path())
        } else {
            one
        }
        .with_session_delete(delete);
        one.open_if_due(&fake, &account);
        assert_eq!(
            *fake.closed_on.lock().unwrap(),
            vec![format!("{MACHINE} {}", agent_of(1))],
            "restart {restart}"
        );
        assert_eq!(
            *deleted.lock().unwrap(),
            vec![format!("agent:main:harness-{}", agent_of(1))],
            "restart {restart}"
        );
        assert_eq!(one.agent(), Some(agent_of(2)), "restart {restart}");
        let kept: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(STATE_FILE)).unwrap())
                .unwrap();
        assert_eq!(kept["agent"], agent_of(2), "restart {restart}");
        assert_eq!(kept["machine"], ACCOUNT, "restart {restart}");
    }
}

/// Heron's H1 and H3 on #7258: a spoken task that ends unanswered (an
/// ask, then a cancel) keeps the pane held until the one-session thread
/// closes it `--now` and deletes its conversation; then the next turn
/// gets a new pane, never the stuck one.
#[test]
fn a_spoken_task_ended_unanswered_closes_and_deletes_the_pane() {
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let seen = deleted.clone();
    let one = session().with_session_delete(Box::new(move |key| {
        seen.lock().unwrap().push(key.to_owned());
        Ok(())
    }));
    let fake = Fake::default();
    one.open_if_due(&fake, &place());
    assert!(matches!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::Joined { .. })
    ));
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "needs-input".into());
    one.release_unanswered("lamp-1");
    assert!(!one.free(), "held until closed");
    let harness = || Some(&fake);
    one.remove_if_due(&Looks::default(), None, false, &harness);
    assert_eq!(fake.count("close"), 1, "{:?}", fake.calls());
    assert_eq!(
        *deleted.lock().unwrap(),
        vec![format!("agent:main:harness-{}", agent_of(1))]
    );
    assert!(one.free());
    assert_eq!(one.agent(), None);
    one.open_if_due(&fake, &place());
    assert_eq!(one.agent(), Some(agent_of(2)));
}

/// Heron's H1: a kept pane found asking something at a start, with no
/// one holding it, is closed and deleted, never adopted.
#[test]
fn a_kept_pane_asking_something_is_not_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = (kept_session(dir.path()), Fake::default());
    one.open_if_due(&fake, &place());
    let again = kept_session(dir.path());
    let fake = Fake::default();
    *fake.opens.lock().unwrap() = 1;
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "needs-input".into());
    again.open_if_due(&fake, &place());
    assert_eq!(fake.count("close"), 1, "{:?}", fake.calls());
    assert_eq!(again.agent(), Some(agent_of(2)));
}

/// #7246 on c0a4: Harness takes a reply only with the person's
/// `programs-reply` setting on, off on a fresh lamp. A reply refused
/// `reply_off` sends that task the per-task way (`None`: its own
/// `harness new`), not failed `busy`; from then on no task is typed or
/// queued for the pane, and no reply is tried again.
#[test]
fn a_reply_off_sends_this_task_and_every_later_one_the_per_task_way() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    fake.replies
        .lock()
        .unwrap()
        .push_back(Typed::NotSent("reply_off".into()));
    assert!(one.takes_reply());
    assert_eq!(spoken_launch(&one, &fake, &spoken("lamp-1")), None);
    assert!(!one.takes_reply());
    assert_eq!(fake.count("reply"), 1);
    assert!(one.free(), "the pane let go");
    // A later spoken task is not for the pane at all, and a skill run's
    // turn is the per-task way with no reply tried.
    assert_eq!(for_the_pane(&one, &spoken("lamp-2")), None);
    assert_eq!(spoken_launch(&one, &fake, &spoken("lamp-2")), None);
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert_eq!(
        turn.send(&fake, &place(), "Run the skill.", far(), &|| false),
        Sent::NoReply
    );
    assert_eq!(fake.count("reply"), 1, "no reply tried again");
    assert_eq!(fake.count("new"), 1, "the pane's own only");
}

/// Any other refused reply keeps today's way: the task fails, never a
/// second session, and the next task still tries the pane.
#[test]
fn another_refused_reply_still_fails_the_task_with_no_second_session() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    fake.replies
        .lock()
        .unwrap()
        .push_back(Typed::NotSent("reply_failed".into()));
    assert_eq!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::NotSent {
            code: "busy".into()
        })
    );
    assert!(one.takes_reply());
    one.release("lamp-1");
    assert!(matches!(
        spoken_launch(&one, &fake, &spoken("lamp-2")),
        Some(LaunchOutcome::Joined { .. })
    ));
    assert_eq!(fake.count("reply"), 2);
}

/// After a reply refused `reply_off` the pane, now unused, is closed
/// `--now` at the keep-open thread's next look (as at a sign-out), and no
/// other is opened by the keep-open loop until lamp-tasks starts again.
#[test]
fn a_reply_off_closes_the_pane_and_the_keep_open_loop_opens_no_other() {
    let (one, fake) = (session(), Fake::default());
    let place_now = || -> Result<(&Fake, Place), String> { Ok((&fake, place())) };
    let mut looks = Looks::default();
    let start = Instant::now();
    one.look(&mut looks, start, None, &place_now);
    assert_eq!(one.agent(), Some(agent_of(1)));
    fake.replies
        .lock()
        .unwrap()
        .push_back(Typed::NotSent("reply_off".into()));
    assert_eq!(spoken_launch(&one, &fake, &spoken("lamp-1")), None);
    // The keep-open loop's step: removals, then the look.
    for tick in 1..10 {
        one.remove_if_due(&looks, None, false, &|| Some(&fake));
        one.look(&mut looks, start + KEEP_OPEN_EVERY * tick, None, &place_now);
        one.tend(&fake, &place(), &|| false);
    }
    assert_eq!(
        *fake.closed_on.lock().unwrap(),
        [format!("{MACHINE} {}", agent_of(1))]
    );
    assert_eq!(fake.count("new"), 1, "{:?}", fake.calls());
    assert_eq!(one.agent(), None);
}

/// Signed out the lamp's engines still work (#6407: the ship lamp runs
/// signed out): the keep-open loop opens the pane as signed in, and a
/// spoken task is typed into it. Only a `reply_off` stops replies.
#[test]
fn signed_out_the_pane_is_still_opened_and_tasks_are_typed_into_it() {
    let (one, fake) = (session(), Fake::default());
    let place_now = || -> Result<(&Fake, Place), String> { Ok((&fake, place())) };
    let key = Some(format!("{MACHINE} openclaw"));
    let mut looks = Looks::default();
    let start = Instant::now();
    for tick in 0..5 {
        one.remove_if_due(&looks, key.as_ref(), true, &|| Some(&fake));
        one.look(
            &mut looks,
            start + KEEP_OPEN_EVERY * tick,
            key.clone(),
            &place_now,
        );
    }
    assert_eq!(fake.count("new"), 1, "{:?}", fake.calls());
    assert!(one.takes_reply());
    assert!(matches!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::Joined { .. })
    ));
    assert_eq!(fake.count("reply"), 1);
}

/// Only `reply_off` stops replies: an uncertain reply or a busy pane
/// (`NOT_IDLE`) keeps the pane in use.
#[test]
fn an_uncertain_or_busy_reply_never_stops_replies() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    fake.replies
        .lock()
        .unwrap()
        .push_back(Typed::Uncertain("timed_out".into()));
    assert_eq!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::Uncertain {
            code: "timed_out".into()
        })
    );
    assert!(one.takes_reply());
    one.release("lamp-1");
    fake.replies.lock().unwrap().push_back(Typed::NotIdle);
    assert!(matches!(
        spoken_launch(&one, &fake, &spoken("lamp-2")),
        Some(LaunchOutcome::Joined { .. })
    ));
    assert!(one.takes_reply());
    assert_eq!(fake.count("reply"), 3);
    assert_eq!(fake.count("close"), 0);
}

/// The kept session (openharness#4393): a lamp-tasks with the switch on,
/// its state and token files in `dir`, and a Harness that keeps program
/// sessions.
fn kept_on(dir: &std::path::Path) -> (OneSession, Fake) {
    let one = kept_session(dir).with_kept_session(dir.join(TOKEN_FILE));
    let fake = Fake::default();
    *fake.programs.lock().unwrap() = true;
    (one, fake)
}

/// The switch on, but an older Harness (no program sessions): the pane is
/// as before, `harness new` and `reply`, no `ensure`, no token file.
#[test]
fn with_no_program_sessions_the_pane_is_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let one = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    let fake = Fake::default();
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!(fake.count("capabilities"), 1);
    assert_eq!((fake.count("new"), fake.count("reply")), (1, 1));
    assert_eq!(fake.count("ensure"), 0);
    assert!(!dir.path().join(TOKEN_FILE).exists());
    // Asked once only.
    one_turn(&one, &fake, "task-2");
    assert_eq!(fake.count("capabilities"), 1);
}

/// Ensured new, given its first words, then each task an acknowledged
/// reply and `wait --after` its input, with its own answer; the next start
/// finds it live by name and token, never a second.
#[test]
fn the_kept_session_is_ensured_typed_with_acks_and_waited_after() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    assert_eq!(one.agent(), Some(agent_of(1)));
    assert_eq!(fake.count("new"), 0);
    assert_eq!(fake.count("ensure"), 1);
    assert_eq!(fake.count("reply-kept"), 1, "its first words");
    let token = std::fs::metadata(dir.path().join(TOKEN_FILE)).unwrap();
    assert_eq!(token.permissions().mode() & 0o777, 0o600);
    assert!(token.len() >= 32);
    for n in 1..=3 {
        let id = format!("task-{n}");
        assert_eq!(
            one_turn(&one, &fake, &id),
            Ended::Answered(format!("answer to New task {id}:"))
        );
    }
    let calls = fake.calls();
    assert!(calls.contains(&"wait-after in-4".to_owned()), "{calls:?}");
    assert_eq!(fake.count("show"), 3, "only for each answer");
    assert_eq!(fake.count("ensure"), 1);
    // A restart: the same token, found live.
    let before = std::fs::read(dir.path().join(TOKEN_FILE)).unwrap();
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(1)));
    assert_eq!(std::fs::read(dir.path().join(TOKEN_FILE)).unwrap(), before);
    assert_eq!(fake.count("reply-kept"), 4, "no first words for a live one");
}

/// `NOT_IDLE` and `ENGINE_NOT_READY` keep the task waiting, never counted
/// until taken; `NOT_RUNNING` ensures again (resumed) and types once more.
#[test]
fn the_kept_session_busy_or_not_running_is_waited_or_ensured_again() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    fake.takes
        .lock()
        .unwrap()
        .extend([Took::NotIdle, Took::EngineNotReady]);
    one_turn(&one, &fake, "task-1");
    assert_eq!(fake.count("reply-kept"), 4);
    assert_eq!(one.turns(), 1);
    // Died: gone in Harness, the reply says not running.
    fake.gone.lock().unwrap().push(agent_of(1));
    fake.takes.lock().unwrap().push_back(Took::NotRunning);
    assert_eq!(
        one_turn(&one, &fake, "task-2"),
        Ended::Answered("answer to New task task-2:".into())
    );
    assert_eq!(fake.count("ensure"), 2);
    assert_eq!(one.agent(), Some(agent_of(1)), "resumed, the same one");
    // Not taken twice: the turn fails, never counted.
    fake.takes
        .lock()
        .unwrap()
        .extend([Took::Timeout, Took::Timeout]);
    let turn = one.enter("task-3", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run.", far(), &|| false),
        Sent::NotTyped(_)
    ));
}

/// OpenClaw's kept session is not rotated by count; a sign-out closes it,
/// deletes its OpenClaw session by `ensure`'s `engineSession` (Harness's
/// `close --forget` is not built), and removes its token and record.
#[test]
fn the_kept_session_is_never_rotated_and_a_sign_out_forgets_it() {
    let dir = tempfile::tempdir().unwrap();
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let seen = deleted.clone();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_session_delete(Box::new(move |key| {
        seen.lock().unwrap().push(key.to_owned());
        Ok(())
    }));
    for n in 0..=ROTATE_TURNS {
        one_turn(&one, &fake, &format!("task-{n}"));
    }
    assert!(
        !fake.calls().iter().any(|call| call.ends_with("--fresh")),
        "OpenClaw compacts: no fresh conversation by count"
    );
    assert_eq!(fake.count("close"), 0);
    one.drop_when_free("the lamp signed out");
    one.tend(&fake, &place(), &|| false);
    assert_eq!(fake.count("close-kept"), 1);
    assert_eq!(
        *deleted.lock().unwrap(),
        vec![format!("agent:main:kept-{}", agent_of(1))]
    );
    assert!(!dir.path().join(TOKEN_FILE).exists());
    assert!(!dir.path().join(STATE_FILE).exists());
    assert_eq!(one.agent(), None);
}

/// A reply refused `NOT_YOURS`: the pane the old way at once (`harness
/// new` and `reply`), and no `ensure` until the daemon starts again.
#[test]
fn a_kept_session_not_ours_sends_the_pane_the_old_way() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    fake.takes
        .lock()
        .unwrap()
        .push_back(Took::NotSent("reply_not_yours".into()));
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!((fake.count("new"), fake.count("reply")), (1, 1));
    one_turn(&one, &fake, "task-2");
    assert_eq!(fake.count("ensure"), 1);
    one.forget("Harness's daemon started");
    one.open_if_due(&fake, &place());
    assert_eq!(fake.count("capabilities"), 2, "asked again after a restart");
}

/// A Harness with acknowledged replies, the pane of `harness new` (signed
/// in, no kept session): a reply `taken` is waited `--after` its input,
/// and a spoken task is joined with no look at `show`; `TIMEOUT` with
/// nothing typed is typed again. An older Harness's `Typed` waits as
/// before.
#[test]
fn an_acknowledged_reply_into_the_pane_is_waited_after_its_input() {
    let (one, fake) = (session(), Fake::default());
    fake.replies
        .lock()
        .unwrap()
        .extend([Typed::Cancelled, Typed::Taken("in-9".into())]);
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!(fake.count("reply"), 2);
    assert!(fake.calls().contains(&"wait-after-pane in-9".to_owned()));
    assert_eq!(fake.count("wait"), 1, "only the open's first turn");
    // An older Harness: `wait --any` as before.
    one_turn(&one, &fake, "task-2");
    assert_eq!(fake.count("wait-after-pane"), 1);
    assert_eq!(fake.count("wait"), 2);
}

/// `ENGINE_NOT_READY` from `ensure` (the old engine still exiting) is
/// tried again within a short bound before the long backoff.
#[test]
fn an_engine_not_ready_ensure_is_tried_again_shortly() {
    let dir = tempfile::tempdir().unwrap();
    let (_, fake) = kept_on(dir.path());
    // A first-turn bound well past the retries' (250 ms apart here).
    let one = OneSession::new(Duration::from_secs(5), Duration::from_millis(5), Duration::from_secs(20))
        .with_session_delete(Box::new(|_| Ok(())))
        .with_state_file(dir.path().join(STATE_FILE))
        .with_kept_session(dir.path().join(TOKEN_FILE));
    fake.ensure_errors
        .lock()
        .unwrap()
        .extend(["engine_not_ready".to_owned(), "engine_not_ready".to_owned()]);
    one.open_if_due(&fake, &place());
    assert_eq!(fake.count("ensure"), 3);
    assert_eq!(one.agent(), Some(agent_of(1)));
}
