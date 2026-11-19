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
    /// Panes `show` names "Terminal": their engine exited (#7906 sign 2).
    shells: Mutex<Vec<String>>,
    /// As a real Harness: a kept session closed is resumed by name and
    /// token, unless `--fresh` (#7906, lamp-52e6).
    closed_resumes: Mutex<bool>,
    /// `show`'s tool count; none: 1.
    tools: Mutex<Option<u64>>,
    /// Plain (not `--fresh`) `ensure`s that leave a died session as it is
    /// before one brings it back, its shell gone (#7906 step a); none:
    /// never.
    resume_heals: Mutex<Option<u32>>,
    /// The session keys OpenClaw's record holds; none: can't be read.
    held: Mutex<Option<Vec<String>>>,
    /// `ensure`'s `confirmed` and `whyCode` (openharness#4484); none: absent.
    ensure_confirmed: Mutex<Option<bool>>,
    ensure_why_code: Mutex<Option<String>>,
    /// The state `ensure` answers; none: idle.
    ensure_state: Mutex<Option<String>>,
    /// What the Gateway check says for any key when `held` is none (#7906
    /// sign 3); none: can't be read.
    holds: Mutex<Option<bool>>,
    /// Reads of the Gateway check.
    holds_read: Mutex<u32>,
    /// The Gateway's record of the kept session (#7906 sign 4): its `seq`,
    /// none: not read; moved by one at each read while `moving`; its
    /// status and last row's role (none: `running`, `user`).
    seq: Mutex<Option<i64>>,
    moving: Mutex<bool>,
    status: Mutex<Option<String>>,
    last_role: Mutex<Option<String>>,
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
                tools: self.tools.lock().unwrap().unwrap_or(1),
            },
            taken,
            shell: self
                .shells
                .lock()
                .unwrap()
                .iter()
                .any(|shell| shell == agent),
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
        let mut heals = self.resume_heals.lock().unwrap();
        if !fresh && let Some(left) = heals.as_mut() {
            if *left == 0 {
                if let Some(agent) = kept.as_ref() {
                    self.shells.lock().unwrap().retain(|shell| shell != agent);
                }
            } else {
                *left -= 1;
            }
        }
        drop(heals);
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
            state: self
                .ensure_state
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "idle".into()),
            why: None,
            engine_session: Some(format!("agent:main:kept-{}", agent_id_now)),
            confirmed: *self.ensure_confirmed.lock().unwrap(),
            why_code: self.ensure_why_code.lock().unwrap().clone(),
        })
    }
    fn reply_kept(&self, name: &str, _: &Path, words: &str) -> Took {
        self.call(format!("reply-kept {name}"));
        let starting = self.kept.lock().unwrap().clone().is_some_and(|agent| {
            self.states.lock().unwrap().get(&agent).map(String::as_str) == Some("starting")
        });
        if starting {
            return Took::EngineNotReady;
        }
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
        cancel: &AtomicBool,
    ) -> Result<Option<String>, String> {
        self.call(format!("wait-after {input}"));
        let state = self
            .waits
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "idle".into());
        // "hold": the turn still going, 300 ms of Harness's hold, cut by
        // `cancel` (a watch's verdict, #7906) as Harness's wait is.
        if state == "hold" {
            let until = Instant::now() + Duration::from_millis(300);
            while Instant::now() < until {
                if cancel.load(Ordering::Acquire) {
                    return Err("cancelled".into());
                }
                thread::sleep(Duration::from_millis(1));
            }
            return Ok(None);
        }
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
    fn close_kept(&self, name: &str, _: &Path, now: bool) -> Result<(), String> {
        self.call(format!(
            "close-kept {name}{}",
            if now { " --now" } else { "" }
        ));
        let mut failing = self.failing_closes.lock().unwrap();
        if *failing > 0 {
            *failing -= 1;
            return Err("busy".into());
        }
        if *self.closed_resumes.lock().unwrap() {
            // Kept by name; its engine gone ("No session found").
            if let Some(agent) = self.kept.lock().unwrap().clone() {
                self.gone.lock().unwrap().push(agent);
            }
        } else {
            *self.kept.lock().unwrap() = None;
        }
        Ok(())
    }
    fn session_held(&self, key: &str) -> Option<bool> {
        self.call(format!("held {key}"));
        *self.holds_read.lock().unwrap() += 1;
        self.held
            .lock()
            .unwrap()
            .as_ref()
            .map(|keys| keys.iter().any(|held| held == key))
            .or(*self.holds.lock().unwrap())
    }
    fn openclaw_turns<'a>(&'a self, _: &str) -> Option<TurnReader<'a>> {
        Some(Box::new(move || {
            let mut seq = self.seq.lock().unwrap();
            if *self.moving.lock().unwrap()
                && let Some(seq) = seq.as_mut()
            {
                *seq += 1;
            }
            let seq = (*seq)?;
            Some(TurnRecord {
                session: "w1".into(),
                seq,
                status: Some(
                    self.status
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| "running".into()),
                ),
                last_role: Some(
                    self.last_role
                        .lock()
                        .unwrap()
                        .clone()
                        .unwrap_or_else(|| "user".into()),
                ),
            })
        }))
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
/// count: past [`ROTATE_TURNS`] turns it is still the one pane. Only an
/// engine with no compaction (Hermes) has the bounds; OpenClaw's size is
/// bounded by its context instead ([`near_context_limit`], #7906).
#[test]
fn an_openclaw_pane_is_never_rotated_by_count() {
    assert_eq!((ROTATE_TURNS, ROTATE_TOKENS), (40, 250_000));
    assert!(!rotation_due(EngineKind::Openclaw, u64::MAX, u64::MAX));
    assert!(rotation_due(EngineKind::Hermes, ROTATE_TURNS, 0));
    assert!(rotation_due(EngineKind::Hermes, 0, ROTATE_TOKENS));
    assert!(!rotation_due(
        EngineKind::Hermes,
        ROTATE_TURNS - 1,
        ROTATE_TOKENS - 1
    ));
    let dir = tempfile::tempdir().unwrap();
    let one = kept_session(dir.path()).with_session_delete(Box::new(|_| Ok(())));
    let fake = Fake::default();
    for n in 0..=ROTATE_TURNS + 5 {
        one_turn(&one, &fake, &format!("task-{n}"));
    }
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
    fn program_sessions(&self) -> Option<bool> {
        (**self).program_sessions()
    }
    fn ensure_kept(
        &self,
        engine: EngineKind,
        name: &str,
        token: &Path,
        folder: &str,
        fresh: bool,
    ) -> Result<Ensured, String> {
        (**self).ensure_kept(engine, name, token, folder, fresh)
    }
    fn reply_kept(&self, name: &str, token: &Path, words: &str) -> Took {
        (**self).reply_kept(name, token, words)
    }
    fn wait_kept(
        &self,
        name: &str,
        token: &Path,
        input: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<String>, String> {
        (**self).wait_kept(name, token, input, cancel)
    }
    fn wait_after(
        &self,
        machine: &str,
        agent: &str,
        input: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<String>, String> {
        (**self).wait_after(machine, agent, input, cancel)
    }
    fn close_kept(&self, name: &str, token: &Path, now: bool) -> Result<(), String> {
        (**self).close_kept(name, token, now)
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

/// Seen on lamp-c0a4 (openharness#4393): a kept session opened signed
/// out, adopted by a lamp-tasks started again before its first sign-in
/// read, then its engine killed. The adopt keeps the record's signed out,
/// so the pane let go is not removed as one opened signed in: never
/// closed, ensured again as the same session with its conversation.
#[test]
fn a_kept_session_whose_engine_died_after_a_start_is_resumed_not_removed() {
    let dir = tempfile::tempdir().unwrap();
    let key = Some(format!("{MACHINE} openclaw"));
    let none = || None::<&Fake>;
    let (one, fake) = kept_on(dir.path());
    one.remove_if_due(&Looks::default(), key.as_ref(), true, &none);
    one.open_if_due(&fake, &place());
    assert!(!one.kept_signed_in());
    // Started again: adopted before any sign-in read.
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(1)));
    assert!(
        !again.kept_signed_in(),
        "the adopt keeps the sign-in it was opened under"
    );
    // Its engine died: let go by the turn, then the next look signed out.
    again.gone(
        &agent_of(1),
        "Harness did not take the words into the kept session",
    );
    let harness = || Some(&fake);
    again.remove_if_due(&Looks::default(), key.as_ref(), true, &harness);
    assert_eq!(fake.count("close"), 0, "{:?}", fake.calls());
    assert!(dir.path().join(STATE_FILE).exists());
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(1)), "the same session");
    assert_eq!(fake.count("ensure"), 3);
}

/// #7245, seen on a lamp: a make's turn cancelled (or at its deadline)
/// while the kept session still works closes it `--now`, since Harness
/// refuses a plain close of a working session; without it the session was
/// resumed still working and every later make was told `NOT_IDLE`.
#[test]
fn a_cancelled_turn_closes_the_kept_session_now() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_session_delete(Box::new(|_| Ok(())));
    one.open_if_due(&fake, &place());
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Make the skill.", far(), &|| false),
        Sent::Typed(_)
    ));
    let ended = turn.wait_end(&fake, MACHINE, &AtomicBool::new(true), &|| {});
    drop(turn);
    assert_eq!(ended, Ended::Cancelled);
    let calls = fake.calls();
    let closes: Vec<&String> = calls
        .iter()
        .filter(|call| call.starts_with("close-kept"))
        .collect();
    assert_eq!(
        closes,
        vec![&"close-kept lamp-one-openclaw --now".to_owned()],
        "{calls:?}"
    );
}

/// #7245 (First's and El's P2 on #7737): a cancelled turn's `--now` close
/// that fails (Harness busy) keeps the kept record and its conversation,
/// and the next look closes it `--now` again, never a plain close a
/// working session refuses.
#[test]
fn a_failed_cancel_close_is_tried_again_now_with_its_record_kept() {
    let dir = tempfile::tempdir().unwrap();
    let deleted = Arc::new(Mutex::new(0));
    let seen = deleted.clone();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_session_delete(Box::new(move |_| {
        *seen.lock().unwrap() += 1;
        Ok(())
    }));
    one.open_if_due(&fake, &place());
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Make the skill.", far(), &|| false),
        Sent::Typed(_)
    ));
    *fake.failing_closes.lock().unwrap() = 1;
    let ended = turn.wait_end(&fake, MACHINE, &AtomicBool::new(true), &|| {});
    drop(turn);
    assert_eq!(ended, Ended::Cancelled);
    assert_eq!(*deleted.lock().unwrap(), 0, "its conversation kept");
    assert!(one.drop_if_due(&fake));
    let closes: Vec<String> = fake
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("close-kept"))
        .collect();
    assert_eq!(
        closes,
        [
            "close-kept lamp-one-openclaw --now",
            "close-kept lamp-one-openclaw --now"
        ]
    );
    assert_eq!(*deleted.lock().unwrap(), 1, "deleted once closed");
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

/// The keep-open loop's one step: removals, then the look.
fn keep_open_step(one: &OneSession, fake: &Fake, looks: &mut Looks) {
    let place_now = || -> Result<(&Fake, Place), String> { Ok((fake, place())) };
    one.remove_if_due(looks, None, false, &|| Some(fake));
    one.look(looks, Instant::now(), None, &place_now);
}

/// The owner's "forget everything" (#6274, openharness#4393): the kept
/// session is closed, its OpenClaw session deleted and its token removed,
/// as at a sign-out, and a new one, with a new token, is opened in the
/// same keep-open step, with no task.
#[test]
fn a_forget_closes_the_kept_session_and_opens_a_new_one_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let seen = deleted.clone();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_session_delete(Box::new(move |key| {
        seen.lock().unwrap().push(key.to_owned());
        Ok(())
    }));
    let mut looks = Looks::default();
    keep_open_step(&one, &fake, &mut looks);
    one_turn(&one, &fake, "task-1");
    assert_eq!(one.agent(), Some(agent_of(1)));
    let token = std::fs::read(dir.path().join(TOKEN_FILE)).unwrap();
    one.forget_conversation();
    keep_open_step(&one, &fake, &mut looks);
    assert_eq!(fake.count("close-kept"), 1);
    assert_eq!(
        *deleted.lock().unwrap(),
        vec![format!("agent:main:kept-{}", agent_of(1))]
    );
    assert_ne!(
        std::fs::read(dir.path().join(TOKEN_FILE)).unwrap(),
        token,
        "the old token removed, a new one made"
    );
    assert_eq!(one.agent(), Some(agent_of(2)), "{:?}", fake.calls());
    assert_eq!(fake.count("ensure"), 2);
}

/// The Reviewer-lead's P2 on #7507: a forget asked while task A holds the
/// kept session, B queued behind it. When A ends, B waits for the drop and
/// is typed into the new session, never into the one the owner asked to
/// forget.
#[test]
fn a_task_queued_behind_a_forget_is_typed_into_the_new_session() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    let one = Arc::new(one.with_session_delete(Box::new(|_| Ok(()))));
    let fake = Arc::new(fake);
    let mut looks = Looks::default();
    keep_open_step(&one, &fake, &mut looks);
    one_turn(&one, &fake, "task-0");
    assert_eq!(one.agent(), Some(agent_of(1)));
    let held = one.enter("task-a", Kind::Skill, far(), &|| false).unwrap();
    one.forget_conversation();
    let (one2, fake2) = (one.clone(), fake.clone());
    let second = thread::spawn(move || one_turn(&one2, &fake2, "task-b"));
    let until = Instant::now() + Duration::from_secs(5);
    while one.waiting() == 0 {
        assert!(Instant::now() < until, "queued");
        thread::sleep(Duration::from_millis(1));
    }
    drop(held);
    // Past the queue's own look: B would have been typed by now.
    thread::sleep(Duration::from_millis(50));
    keep_open_step(&one, &fake, &mut looks);
    assert_eq!(
        second.join().unwrap(),
        Ended::Answered("answer to New task task-b:".into())
    );
    let calls = fake.calls();
    assert_eq!(fake.count("close-kept"), 1, "{calls:?}");
    let asked = fake.asked.lock().unwrap().clone();
    assert!(
        !asked[&agent_of(1)].contains("task-b"),
        "B typed into the session the owner asked to forget: {calls:?}"
    );
    assert!(asked[&agent_of(2)].contains("task-b"), "{asked:?}");
    assert_eq!(one.agent(), Some(agent_of(2)), "{calls:?}");
}

/// The Reviewer-lead's P3 on #7507: while a removal is under way (the pane
/// taken, its OpenClaw session not deleted yet), a task entering waits for
/// it, so it never ensures the old name with the old token.
#[test]
fn a_task_entering_during_a_removal_waits_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
    let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
    let (entered_tx, go_rx) = (Mutex::new(entered_tx), Mutex::new(go_rx));
    let (one, fake) = kept_on(dir.path());
    let one = Arc::new(one.with_session_delete(Box::new(move |_| {
        entered_tx.lock().unwrap().send(()).unwrap();
        go_rx.lock().unwrap().recv().unwrap();
        Ok(())
    })));
    let fake = Arc::new(fake);
    one_turn(&one, &fake, "task-0");
    one.drop_when_free("the lamp signed out");
    let (one2, fake2) = (one.clone(), fake.clone());
    let dropper = thread::spawn(move || one2.drop_if_due(&*fake2));
    entered_rx.recv().unwrap();
    let soon = Instant::now() + Duration::from_millis(50);
    assert!(
        one.enter("task-c", Kind::Skill, soon, &|| false).is_err(),
        "no task enters while the removal is under way"
    );
    go_tx.send(()).unwrap();
    assert!(dropper.join().unwrap());
    assert!(!dir.path().join(TOKEN_FILE).exists());
    assert!(one.enter("task-d", Kind::Skill, far(), &|| false).is_ok());
}

/// The removal is marked under way in the same lock it is taken in: no
/// task enters between taking the drop due and starting the drop.
#[test]
fn no_task_enters_between_taking_a_drop_and_starting_it() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one_turn(&one, &fake, "task-0");
    one.drop_when_free("the owner asked to forget");
    assert!(one.take_due().is_some());
    let soon = Instant::now() + Duration::from_millis(50);
    assert!(
        one.enter("task-e", Kind::Skill, soon, &|| false).is_err(),
        "a drop taken is under way at once"
    );
}

/// A kept session left with no task for [`KEPT_IDLE_MAX`] is dropped the
/// same way (closed, deleted, token removed) and a fresh one opened.
#[test]
fn a_kept_session_left_idle_is_dropped_and_a_fresh_one_opened() {
    let dir = tempfile::tempdir().unwrap();
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let seen = deleted.clone();
    let (mut one, fake) = kept_on(dir.path());
    one = one.with_session_delete(Box::new(move |key| {
        seen.lock().unwrap().push(key.to_owned());
        Ok(())
    }));
    assert_eq!(KEPT_IDLE_MAX, Duration::from_secs(2 * 60 * 60));
    let mut looks = Looks::default();
    keep_open_step(&one, &fake, &mut looks);
    keep_open_step(&one, &fake, &mut looks);
    assert_eq!(fake.count("close-kept"), 0, "not idle long enough");
    one.kept_idle_max = Duration::ZERO;
    keep_open_step(&one, &fake, &mut looks);
    assert_eq!(fake.count("close-kept"), 1);
    assert_eq!(
        *deleted.lock().unwrap(),
        vec![format!("agent:main:kept-{}", agent_of(1))]
    );
    assert_eq!(one.agent(), Some(agent_of(2)), "{:?}", fake.calls());
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
    let one = OneSession::new(
        Duration::from_secs(5),
        Duration::from_millis(5),
        Duration::from_secs(20),
    )
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

/// One round of #7895's loop on lamp-52e6: a spoken task's words into the
/// kept session answered `NOT_RUNNING`, so it is put back (`busy`) and the
/// one-session thread ensures the session again, which comes back
/// `resumed`. Returns the spoken launch's answer.
fn not_running_round(one: &OneSession, fake: &Fake, id: &str) -> Option<LaunchOutcome> {
    fake.takes.lock().unwrap().push_back(Took::NotRunning);
    fake.gone.lock().unwrap().push(agent_of(1));
    let outcome = spoken_launch(one, fake, &spoken(id));
    one.open_if_due(fake, &place());
    outcome
}

fn busy() -> Option<LaunchOutcome> {
    Some(LaunchOutcome::NotSent {
        code: "busy".into(),
    })
}

/// #7895: a kept session that never takes the words through every resume.
/// The spoken task is put back for [`STARTING_RESUMES_MOST`] resumes, then
/// ends `not_sent` with [`NOT_STARTED`], once: the store fails it and Task
/// news tells the asker its one failed line. No resume after that.
fn ends_at_the_bound(one: &OneSession, fake: &Fake) {
    one.open_if_due(fake, &place());
    for round in 1..=STARTING_RESUMES_MOST {
        assert_eq!(not_running_round(one, fake, "lamp-1"), busy(), "{round}");
        assert_eq!(fake.count("ensure"), 1 + round as usize, "resumed");
    }
    assert_eq!(
        not_running_round(one, fake, "lamp-1"),
        Some(LaunchOutcome::NotSent {
            code: NOT_STARTED.into()
        })
    );
    // Ended there: the session is not let go for it, so nothing ensures it
    // again until the next task's words find it not running.
    let ensured = fake.count("ensure");
    assert_eq!(ensured, 1 + STARTING_RESUMES_MOST as usize);
    one.open_if_due(fake, &place());
    assert_eq!(fake.count("ensure"), ensured, "{:?}", fake.calls());
    assert!(one.free());
    // The next ask has its own bound: put back, never ended at once.
    fake.gone.lock().unwrap().clear();
    assert_eq!(not_running_round(one, fake, "lamp-2"), busy());
}

/// #7895 (lamp-52e6, 19 Nov 11:15:58): the session stays `starting`
/// through every resume.
#[test]
fn a_spoken_task_whose_kept_session_never_starts_ends_at_the_bound() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    ends_at_the_bound(&one, &fake);
}

/// #7895, Fifth's read of the same jam (#7814): OpenClaw's gateway did not
/// know the kept session, so each resume's `sessions.resolve` said `No
/// session found: agent:main:harness-cf840d2c-…`, whatever the cause. lamp-tasks sees
/// only Harness's side of it, as the journal shows: `harness ensure`
/// answers `resumed` and `starting` (`harness other: … state=starting
/// id=cf840d2c`), and the next reply `exit=4 … error=NOT_RUNNING`. Every
/// such resume counts toward the same bound.
#[test]
fn a_kept_session_the_gateway_cannot_resolve_ends_at_the_same_bound() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    *fake.ensure_state.lock().unwrap() = Some("starting".into());
    ends_at_the_bound(&one, &fake);
}

/// #7895: a kept session that comes up within the bound (its words taken
/// after [`STARTING_RESUMES_MOST`] resumes) runs the task as today, joined,
/// with nothing failed or told.
#[test]
fn a_spoken_task_whose_kept_session_starts_within_the_bound_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    for _ in 0..STARTING_RESUMES_MOST {
        assert_eq!(not_running_round(&one, &fake, "lamp-1"), busy());
    }
    assert_eq!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::Joined {
            agent_id: agent_of(1),
            session: None
        })
    );
    assert_eq!(one.spoken_holder().as_deref(), Some("lamp-1"));
}

/// #7895, V2's 13:02 follow-up: `harness ensure` itself refused while a
/// spoken task waits in the store for the pane, `times` opens in a row.
fn opens_refused(one: &OneSession, fake: &Fake, times: u32) {
    for _ in 0..times {
        fake.ensure_errors
            .lock()
            .unwrap()
            .push_back("ensure_failed".into());
        // Past the open's backoff at once.
        one.lock().next_open = None;
        one.open_if_due(fake, &place());
    }
}

/// Each failed open counts toward the same bound; past it the waiting
/// spoken task is handed over and ends `not_sent` [`NOT_STARTED`] at once,
/// with no reply and no further ensure.
#[test]
fn a_spoken_task_whose_kept_session_is_never_ensured_ends_at_the_bound() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.knows(&place());
    one.set_spoken_queue(vec!["lamp-1".into()]);
    opens_refused(&one, &fake, STARTING_RESUMES_MOST + 1);
    let ensured = fake.count("ensure");
    assert_eq!(ensured, 1 + STARTING_RESUMES_MOST as usize);
    assert_eq!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::NotSent {
            code: NOT_STARTED.into()
        })
    );
    assert_eq!(fake.count("ensure"), ensured, "{:?}", fake.calls());
    assert_eq!(fake.count("reply-kept"), 0);
    // Within the open's backoff, nothing ensures it again.
    one.open_if_due(&fake, &place());
    assert_eq!(fake.count("ensure"), ensured);
}

/// #7895: refused [`STARTING_RESUMES_MOST`] times, then ensured: the
/// spoken task waiting for it runs as today.
#[test]
fn a_spoken_task_whose_kept_session_is_ensured_within_the_bound_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.knows(&place());
    one.set_spoken_queue(vec!["lamp-1".into()]);
    opens_refused(&one, &fake, STARTING_RESUMES_MOST);
    one.lock().next_open = None;
    one.open_if_due(&fake, &place());
    assert_eq!(one.agent(), Some(agent_of(1)));
    assert_eq!(
        spoken_launch(&one, &fake, &spoken("lamp-1")),
        Some(LaunchOutcome::Joined {
            agent_id: agent_of(1),
            session: None
        })
    );
}

/// #7906, the lamp-52e6 jam: a make's deadline closed the kept session and
/// deleted its OpenClaw conversation, but a real Harness's `ensure` finds
/// a session by name and token and resumed it: its engine exited ("No
/// session found"), a plain shell, for ever. The next task's `ensure` is
/// `--fresh`, a new session, and the task runs; the deleted one is never
/// resumed.
#[test]
fn a_deadline_deleted_kept_session_is_never_resumed_the_next_task_runs_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    *fake.closed_resumes.lock().unwrap() = true;
    one.open_if_due(&fake, &place());
    let turn = one.enter("make-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Make the skill.", far(), &|| false),
        Sent::Typed(_)
    ));
    // The deadline.
    assert_eq!(
        turn.wait_end(&fake, MACHINE, &AtomicBool::new(true), &|| {}),
        Ended::Cancelled
    );
    drop(turn);
    // The engine of the closed session is gone: resumed, it is a shell.
    fake.shells.lock().unwrap().push(agent_of(1));
    assert_eq!(
        one_turn(&one, &fake, "task-2"),
        Ended::Answered("answer to New task task-2:".into())
    );
    let ensures: Vec<String> = fake
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("ensure"))
        .collect();
    assert_eq!(
        ensures,
        [
            "ensure lamp-one-openclaw",
            "ensure lamp-one-openclaw --fresh"
        ],
        "{:?}",
        fake.calls()
    );
    assert_eq!(one.agent(), Some(agent_of(2)), "a fresh session");
    assert!(dir.path().join(TOKEN_FILE).exists(), "its token kept");
}

/// A kept session adopted after a restart whose `ensure` says it is
/// `starting`, and stays so past the open's bound (#7906 sign 1): closed,
/// forgotten and replaced with `--fresh`, and the task runs there.
#[test]
fn a_kept_session_stuck_starting_past_the_bound_is_replaced_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    drop(one);
    *fake.ensure_state.lock().unwrap() = Some("starting".into());
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "starting".into());
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(2)), "{:?}", fake.calls());
    assert!(
        fake.calls()
            .contains(&"close-kept lamp-one-openclaw --now".to_owned())
    );
    assert_eq!(
        one_turn(&again, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
}

/// A kept session that goes `starting` under a task, its replies refused
/// `ENGINE_NOT_READY` (#7906 sign 1): past the bound, one `show`, replaced
/// fresh, and the same task typed into the new one.
#[test]
fn a_kept_session_refusing_while_starting_is_replaced_and_the_task_runs() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "starting".into());
    let began = Instant::now();
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert!(began.elapsed() < Duration::from_secs(10));
    assert_eq!(one.agent(), Some(agent_of(2)));
    assert!(
        fake.calls()
            .contains(&"ensure lamp-one-openclaw --fresh".to_owned())
    );
}

/// A kept session found after a restart whose pane is a plain shell
/// (#7906 sign 2, `show` names it "Terminal"): replaced fresh at the open.
#[test]
fn a_kept_session_found_a_plain_shell_is_replaced_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    drop(one);
    fake.shells.lock().unwrap().push(agent_of(1));
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(2)));
    assert!(
        fake.calls()
            .contains(&"ensure lamp-one-openclaw --fresh".to_owned())
    );
}

/// A pane of `harness new` that is a plain shell (#7906 sign 2), which
/// `wait` reads idle: never typed into; closed, forgotten, a new pane.
#[test]
fn a_plain_shell_pane_is_closed_and_a_new_one_takes_the_task() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    fake.shells.lock().unwrap().push(agent_of(1));
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!(fake.count("new"), 2);
    assert!(!fake.calls().contains(&format!("reply {}", agent_of(1))));
    assert!(fake.calls().contains(&format!("close {}", agent_of(1))));
    assert_eq!(one.agent(), Some(agent_of(2)));
}

/// A healthy kept session stays kept (#7906): after a restart it is found
/// live and idle, one `show` at the open, never `--fresh`, never closed,
/// and many turns later still the same one.
#[test]
fn a_healthy_kept_session_stays_kept_with_no_fresh_start() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    *fake.held.lock().unwrap() = Some(vec![format!("agent:main:kept-{}", agent_of(1))]);
    one.open_if_due(&fake, &place());
    one_turn(&one, &fake, "task-1");
    drop(one);
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    for n in 2..=8 {
        assert_eq!(
            one_turn(&again, &fake, &format!("task-{n}")),
            Ended::Answered(format!("answer to New task task-{n}:"))
        );
    }
    assert_eq!(again.agent(), Some(agent_of(1)));
    assert!(!fake.calls().iter().any(|call| call.ends_with("--fresh")));
    assert_eq!(fake.count("close-kept"), 0);
    assert_eq!(fake.count("ensure"), 2);
}

/// #7906, the owner's rule: no resume of a dead id. A kept record whose
/// OpenClaw conversation the Gateway no longer holds (deleted under
/// lamp-tasks) is forgotten before `ensure`, which is `--fresh`: Harness
/// would resume it with no Gateway check.
#[test]
fn a_kept_session_the_gateway_no_longer_holds_is_never_ensured_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    drop(one);
    *fake.held.lock().unwrap() = Some(Vec::new());
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    let ensures: Vec<String> = fake
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("ensure"))
        .collect();
    assert_eq!(
        ensures,
        [
            "ensure lamp-one-openclaw",
            "ensure lamp-one-openclaw --fresh"
        ]
    );
    assert_eq!(again.agent(), Some(agent_of(2)));
}

/// #7906, the adversarial read's P1-b: OpenClaw's record is matched by the
/// kept record's own session key (`engineSession`), never by
/// `agent:main:harness-<agent id>`. A record whose agent id is not in its
/// session key, still held, stays kept: no `--fresh`, no close.
#[test]
fn a_kept_session_held_under_its_own_session_key_stays_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    drop(one);
    // Held under the record's key only, not under the agent's own id.
    *fake.held.lock().unwrap() = Some(vec![format!("agent:main:kept-{}", agent_of(1))]);
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(1)));
    assert!(
        !fake.calls().iter().any(|call| call.ends_with("--fresh")),
        "{:?}",
        fake.calls()
    );
    assert_eq!(fake.count("close-kept"), 0);
    // OpenClaw's record not readable: unknown, never gone.
    drop(again);
    *fake.held.lock().unwrap() = None;
    let third = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    third.open_if_due(&fake, &place());
    assert_eq!(third.agent(), Some(agent_of(1)));
}

/// #7906, P1-a: only `starting` read by `show` times out into stuck. A
/// session `installing` (an OTA's OpenClaw install), `offline` or
/// `unknown` past the open's bound is kept as it is, never replaced.
#[test]
fn a_kept_session_installing_or_offline_past_the_bound_is_never_replaced() {
    for state in ["installing", "offline", "unknown"] {
        let dir = tempfile::tempdir().unwrap();
        let (one, fake) = kept_on(dir.path());
        one.open_if_due(&fake, &place());
        drop(one);
        fake.states
            .lock()
            .unwrap()
            .insert(agent_of(1), state.into());
        let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
        let began = Instant::now();
        again.open_if_due(&fake, &place());
        assert!(
            began.elapsed() >= Duration::from_secs(1),
            "{state}: looked at to the bound"
        );
        assert_eq!(again.agent(), Some(agent_of(1)), "{state}");
        assert_eq!(fake.count("close-kept"), 0, "{state}");
        assert!(
            !fake.calls().iter().any(|call| call.ends_with("--fresh")),
            "{state}"
        );
    }
}

/// #7906, P2-a: a stuck session's close that fails is tried again `--now`
/// at the next look and keeps the token (a new one on the same name could
/// be refused `NAME_TAKEN`); the next `ensure` is `--fresh`.
#[test]
fn a_stuck_sessions_failed_close_keeps_its_token_and_the_next_is_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    drop(one);
    fake.shells.lock().unwrap().push(agent_of(1));
    *fake.failing_closes.lock().unwrap() = 1;
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), None, "the close failed: no new one yet");
    assert!(again.drop_if_due(&fake));
    assert!(dir.path().join(TOKEN_FILE).exists(), "its token kept");
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(2)));
    assert_eq!(
        fake.calls()
            .iter()
            .filter(|call| call.as_str() == "close-kept lamp-one-openclaw --now")
            .count(),
        2
    );
    assert!(
        fake.calls()
            .contains(&"ensure lamp-one-openclaw --fresh".to_owned())
    );
}

/// #7906, P2-b and P2-c: the backoff comes from the replacements in a row,
/// not `failed_opens`: none at the first, 5 s from the second, doubled.
#[test]
fn stuck_replacements_in_a_row_back_off_from_the_second() {
    let (one, fake) = (session(), Fake::default());
    one.open_if_due(&fake, &place());
    one.replace_kept(&fake, MACHINE, &agent_of(1), "a test");
    assert!(one.lock().next_open.is_none(), "the first: at once");
    one.open_if_due(&fake, &place());
    let wait = |one: &OneSession| {
        one.lock()
            .next_open
            .map(|next| next.saturating_duration_since(Instant::now()))
    };
    one.replace_kept(&fake, MACHINE, &agent_of(2), "a test");
    assert!(
        wait(&one).is_some_and(|wait| wait > Duration::from_secs(4) && wait <= OPEN_RETRY_FIRST)
    );
    one.lock().next_open = Some(Instant::now());
    one.open_if_due(&fake, &place());
    one.replace_kept(&fake, MACHINE, &agent_of(3), "a test");
    assert!(wait(&one).is_some_and(|wait| wait > Duration::from_secs(9)));
}

/// #7906: OpenClaw's record is read by the exact session key.
#[test]
fn openclaw_holds_key_matches_the_key_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("openclaw-agent.sqlite");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE session_nodes (session_key TEXT NOT NULL PRIMARY KEY, current_session_id TEXT NOT NULL, status TEXT);
             CREATE TABLE session_windows (session_id TEXT NOT NULL PRIMARY KEY, session_key TEXT NOT NULL);
             CREATE TABLE transcript_events (session_id TEXT NOT NULL, seq INTEGER NOT NULL, message_role TEXT, PRIMARY KEY (session_id, seq));
             INSERT INTO session_nodes VALUES ('agent:main:harness-aaaaaaaa-1', 'w1', NULL);
             INSERT INTO transcript_events VALUES ('w1', 1, 'user');",
        )
        .unwrap();
    drop(connection);
    use crate::harness::openclaw_holds_key;
    assert_eq!(
        openclaw_holds_key(&db, "agent:main:harness-aaaaaaaa-1"),
        Ok(true)
    );
    assert_eq!(
        openclaw_holds_key(&db, "agent:main:harness-aaaaaaaa"),
        Ok(false)
    );
    assert!(openclaw_holds_key(&dir.path().join("none.sqlite"), "agent:main:x").is_err());
}

/// openharness#4484 (proposed) `state: "stopped"`: replaced at once,
/// before any `show`. Absent (`idle`, today's fake): kept.
#[test]
fn an_ensure_that_says_stopped_is_replaced_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    drop(one);
    *fake.ensure_state.lock().unwrap() = Some("stopped".into());
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(2)));
    assert!(
        fake.calls()
            .contains(&"ensure lamp-one-openclaw --fresh".to_owned())
    );
    assert_eq!(fake.count("show"), 0, "{:?}", fake.calls());
}

/// openharness#4484 (proposed) `confirmed: false`: a resume with no verdict
/// from `show` past the bound is replaced; the same with `confirmed`
/// absent is kept.
#[test]
fn an_unconfirmed_resume_with_no_verdict_is_replaced_absent_it_is_kept() {
    for confirmed in [Some(false), None] {
        let dir = tempfile::tempdir().unwrap();
        let (one, fake) = kept_on(dir.path());
        one.open_if_due(&fake, &place());
        drop(one);
        *fake.ensure_confirmed.lock().unwrap() = confirmed;
        fake.states
            .lock()
            .unwrap()
            .insert(agent_of(1), "unknown".into());
        let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
        again.open_if_due(&fake, &place());
        let want = if confirmed.is_some() {
            agent_of(2)
        } else {
            agent_of(1)
        };
        assert_eq!(again.agent(), Some(want), "{confirmed:?}");
    }
}

/// openharness#4484 (proposed) `result: "new"`, `whyCode:
/// "ENGINE_SESSION_GONE"`: the new one is kept and recorded, the old one
/// forgotten, no extra close and no `--fresh`.
#[test]
fn harness_replacing_a_dead_conversation_is_kept_with_no_extra_close() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    drop(one);
    *fake.kept.lock().unwrap() = None;
    *fake.ensure_why_code.lock().unwrap() = Some("ENGINE_SESSION_GONE".into());
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(2)));
    assert_eq!(fake.count("close-kept"), 0);
    assert!(!fake.calls().iter().any(|call| call.ends_with("--fresh")));
    assert_eq!(again.kept().map(|(agent, _, _)| agent), Some(agent_of(2)));
}

/// #7906 ladder (a): a kept session whose engine died (a plain shell) while
/// OpenClaw still holds its conversation gets one plain `ensure`, a resume
/// that keeps its memory: no close, no delete, no `--fresh`.
#[test]
fn ladder_a_a_died_session_still_held_is_resumed_with_its_memory() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    let deleted = Arc::new(Mutex::new(0));
    let seen = deleted.clone();
    let one = one.with_session_delete(Box::new(move |_| {
        *seen.lock().unwrap() += 1;
        Ok(())
    }));
    one.open_if_due(&fake, &place());
    drop(one);
    fake.shells.lock().unwrap().push(agent_of(1));
    *fake.resume_heals.lock().unwrap() = Some(1);
    *fake.held.lock().unwrap() = Some(vec![format!("agent:main:kept-{}", agent_of(1))]);
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(1)), "{:?}", fake.calls());
    assert_eq!(fake.count("close-kept"), 0);
    assert_eq!(*deleted.lock().unwrap(), 0, "its conversation kept");
    assert!(!fake.calls().iter().any(|call| call.ends_with("--fresh")));
    assert_eq!(
        fake.count("ensure"),
        3,
        "the first, the restart's, the resume"
    );
}

/// #7906 ladder (b): died again after its one resume: replaced fresh.
#[test]
fn ladder_b_a_session_that_dies_again_after_its_resume_is_replaced_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    drop(one);
    fake.shells.lock().unwrap().push(agent_of(1));
    *fake.held.lock().unwrap() = Some(vec![format!("agent:main:kept-{}", agent_of(1))]);
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    let ensures: Vec<String> = fake
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("ensure"))
        .collect();
    assert_eq!(
        ensures,
        [
            "ensure lamp-one-openclaw",
            "ensure lamp-one-openclaw",
            "ensure lamp-one-openclaw",
            "ensure lamp-one-openclaw --fresh"
        ]
    );
    assert_eq!(again.agent(), Some(agent_of(2)));
}

/// #7906, never lose the task: a skill run whose session dies under its
/// turn before any tool ran is put back once into the reopened session
/// and answered there; the person's one line comes from one replacement.
#[test]
fn a_task_whose_session_died_before_any_tool_is_put_back_and_answered() {
    let dir = tempfile::tempdir().unwrap();
    let told = Arc::new(Mutex::new(0));
    let seen = told.clone();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_on_replaced(Box::new(move |_| *seen.lock().unwrap() += 1));
    one.open_if_due(&fake, &place());
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run the skill.", far(), &|| false),
        Sent::Typed(_)
    ));
    fake.shells.lock().unwrap().push(agent_of(1));
    *fake.tools.lock().unwrap() = Some(0);
    let ended = turn.wait_end(&fake, MACHINE, &AtomicBool::new(false), &|| {});
    assert_eq!(ended, Ended::Answered("answer to New task task-1:".into()));
    assert_eq!(one.agent(), Some(agent_of(2)), "{:?}", fake.calls());
    assert_eq!(*told.lock().unwrap(), 1, "one replacement, one line");
}

/// #7906: a turn that had already run a tool is never replayed blindly:
/// it ends saying it was interrupted after it started, and the session is
/// reopened for the next task.
#[test]
fn a_task_that_had_run_a_tool_is_not_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run the skill.", far(), &|| false),
        Sent::Typed(_)
    ));
    let typed = fake.count("reply-kept");
    fake.shells.lock().unwrap().push(agent_of(1));
    *fake.tools.lock().unwrap() = Some(2);
    let ended = turn.wait_end(&fake, MACHINE, &AtomicBool::new(false), &|| {});
    assert!(
        matches!(&ended, Ended::Failed(why) if why.contains("after the task had started work") && why.contains("ask again")),
        "{ended:?}"
    );
    assert_eq!(fake.count("reply-kept"), typed, "never typed again");
}

/// #7906 (Harness's timing on OpenClaw 2026.11.2): a fresh session refuses
/// its first replies ENGINE_NOT_READY for a while, then takes them. Within
/// the bound that is waited on, never a sign: no replace, no `--fresh`,
/// and the task is answered in the same session.
#[test]
fn a_fresh_session_refusing_engine_not_ready_for_a_while_is_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    // Its first words: refused twice, 250 ms apart, within the 1 s bound.
    fake.takes
        .lock()
        .unwrap()
        .extend(std::iter::repeat_n(Took::EngineNotReady, 2));
    one.open_if_due(&fake, &place());
    assert_eq!(one.agent(), Some(agent_of(1)));
    assert_eq!(
        fake.asked
            .lock()
            .unwrap()
            .get(&agent_of(1))
            .map(String::as_str),
        Some(OPEN_PROMPT),
        "its first words taken after the refusals"
    );
    // The task's: refused 40 times, 5 ms apart, also within the bound.
    fake.takes
        .lock()
        .unwrap()
        .extend(std::iter::repeat_n(Took::EngineNotReady, 40));
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run the skill.", far(), &|| false),
        Sent::Typed(_)
    ));
    assert_eq!(
        turn.wait_end(&fake, MACHINE, &AtomicBool::new(false), &|| {}),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!(one.agent(), Some(agent_of(1)));
    assert_eq!(fake.count("close-kept"), 0);
    assert!(!fake.calls().iter().any(|call| call.ends_with("--fresh")));
    assert!(
        fake.calls().contains(&"wait-after in-1".to_owned()),
        "its first words were taken after the refusals: {:?}",
        fake.calls()
    );
}

/// OpenClaw's agent database (#7906), with the columns lampOS reads as
/// lamp-52e6 has them, holding `messages` transcript rows of `bytes` each
/// for Harness's `agent`'s session.
fn openclaw_db(dir: &std::path::Path, agent: &str, messages: u64, bytes: u64) -> PathBuf {
    let db = crate::harness::openclaw_record_path(&dir.join("openclaw-state"));
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE session_nodes (session_key TEXT NOT NULL PRIMARY KEY, current_session_id TEXT, status TEXT);
             CREATE TABLE transcript_events (session_id TEXT NOT NULL, seq INTEGER NOT NULL, message_role TEXT,
               event_json TEXT, event_zstd BLOB, event_utf8_bytes INTEGER, PRIMARY KEY (session_id, seq));",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session_nodes VALUES (?1, 's1', 'done')",
            [format!("agent:main:harness-{agent}")],
        )
        .unwrap();
    for seq in 0..messages {
        connection
            .execute(
                "INSERT INTO transcript_events VALUES ('s1', ?1, 'assistant', '{}', NULL, ?2)",
                rusqlite::params![seq as i64, bytes as i64],
            )
            .unwrap();
    }
    db
}

/// A make's turn, entered, typed and waited for.
fn one_make(one: &OneSession, fake: &Fake, id: &str) {
    let turn = one
        .enter(id, Kind::Skill, far(), &|| false)
        .unwrap()
        .as_make(true);
    assert!(matches!(
        turn.send(fake, &place(), "Make a skill.", far(), &|| false),
        Sent::Typed(_)
    ));
    turn.wait_end(fake, MACHINE, &AtomicBool::new(false), &|| {});
}

fn fresh_ensures(fake: &Fake) -> usize {
    fake.calls()
        .iter()
        .filter(|call| call.starts_with("ensure ") && call.ends_with(" --fresh"))
        .count()
}

/// #7906: a make on a kept session past 60% of the context (80 messages of
/// ~1000 tokens: 80k tokens) is given a fresh one (`ensure --fresh`, the
/// old OpenClaw session deleted), once; an ordinary task on the same size
/// (below 85%) keeps it.
#[test]
fn a_make_on_a_big_kept_session_starts_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let db = openclaw_db(dir.path(), &agent_of(1), 80, 4_000);
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let seen = deleted.clone();
    let (one, fake) = kept_on(dir.path());
    let one = one
        .with_openclaw_db(db)
        .with_session_delete(Box::new(move |key| {
            seen.lock().unwrap().push(key.to_owned());
            Ok(())
        }));
    one_turn(&one, &fake, "task-1");
    one_turn(&one, &fake, "task-2");
    assert_eq!(one.agent(), Some(agent_of(1)), "an ordinary task keeps it");
    assert_eq!(fresh_ensures(&fake), 0);
    one_make(&one, &fake, "make-1");
    assert_eq!(fresh_ensures(&fake), 1, "{:?}", fake.calls());
    assert_eq!(
        one.agent(),
        Some(agent_of(2)),
        "the make's own fresh session"
    );
    assert_eq!(
        deleted.lock().unwrap().as_slice(),
        [format!("agent:main:kept-{}", agent_of(1))]
    );
    // The fresh session holds nothing in the record yet: kept.
    one_make(&one, &fake, "make-2");
    assert_eq!(fresh_ensures(&fake), 1);
    assert_eq!(one.agent(), Some(agent_of(2)));
}

/// #7906: a make on a small kept session (10 messages, ~2.5k tokens) is
/// typed into it: no fresh start.
#[test]
fn a_make_on_a_small_kept_session_stays_kept() {
    let dir = tempfile::tempdir().unwrap();
    let db = openclaw_db(dir.path(), &agent_of(1), 10, 1_000);
    let (one, fake) = kept_on(dir.path());
    let one = one.with_openclaw_db(db);
    one_turn(&one, &fake, "task-1");
    one_make(&one, &fake, "make-1");
    assert_eq!((fake.count("ensure"), fresh_ensures(&fake)), (1, 0));
    assert_eq!(one.agent(), Some(agent_of(1)));
}

/// #7906: OpenClaw's record unreadable (no file): the pane's own token
/// count is weighed; past a make's bound, a fresh session.
#[test]
fn an_unreadable_openclaw_record_falls_back_to_the_panes_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_openclaw_db(dir.path().join("no-such.sqlite"));
    one_turn(&one, &fake, "task-1");
    one_make(&one, &fake, "make-1");
    assert_eq!(fresh_ensures(&fake), 0, "a few tokens counted: kept");
    one.count_answer(&"y".repeat(MAKE_FRESH_TOKENS as usize * 4));
    one_make(&one, &fake, "make-2");
    assert_eq!(fresh_ensures(&fake), 1, "{:?}", fake.calls());
    assert_eq!(one.agent(), Some(agent_of(2)));
}

/// #7906: an ordinary task on a session past 85% (140 messages of 800
/// tokens: 112k) is given a fresh one too.
#[test]
fn an_ordinary_task_near_the_limit_starts_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let db = openclaw_db(dir.path(), &agent_of(1), 140, 3_200);
    let (one, fake) = kept_on(dir.path());
    let one = one.with_openclaw_db(db.clone());
    one.open_if_due(&fake, &place());
    assert_eq!(one.agent(), Some(agent_of(1)));
    one_turn(&one, &fake, "task-1");
    assert_eq!(fresh_ensures(&fake), 1, "{:?}", fake.calls());
    assert_eq!(one.agent(), Some(agent_of(2)));
    let began = Instant::now();
    let size = crate::harness::openclaw_session_size(&db, "agent:main:x", &agent_of(1)).unwrap();
    eprintln!("openclaw_session_size: {} us", began.elapsed().as_micros());
    assert_eq!(
        size,
        SessionSize {
            messages: 140,
            bytes: 140 * 3_200
        }
    );
}

/// #7906 sign 3: a kept session whose conversation OpenClaw's Gateway no
/// longer holds (deleted under lamp-tasks) is closed `--now` and let go
/// before any task is handed to it, and the task goes into the session
/// ensured again; one the Gateway still holds is typed into as before.
#[test]
fn a_kept_session_the_gateway_no_longer_holds_is_gone_before_the_hand_over() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    // Held: typed, nothing closed; the new one's first turn is not looked
    // at (the record may not hold it yet).
    *fake.holds.lock().unwrap() = Some(true);
    one_turn(&one, &fake, "task-1");
    assert_eq!(
        *fake.holds_read.lock().unwrap(),
        0,
        "a new one's first turn"
    );
    assert_eq!(
        one_turn(&one, &fake, "task-2"),
        Ended::Answered("answer to New task task-2:".into())
    );
    assert_eq!(*fake.holds_read.lock().unwrap(), 1);
    assert_eq!(fake.count("close-kept"), 0, "{:?}", fake.calls());
    assert_eq!(one.agent(), Some(agent_of(1)));
    // Gone from the Gateway: closed `--now` before the hand-over, then the
    // task typed into the session ensured again.
    *fake.holds.lock().unwrap() = Some(false);
    let turn = one.enter("task-3", Kind::Skill, far(), &|| false).unwrap();
    let replies = fake.count("reply-kept");
    assert!(matches!(
        turn.send(&fake, &place(), "Run.", far(), &|| false),
        Sent::Typed(_)
    ));
    let calls = fake.calls();
    assert!(
        calls.contains(&"close-kept lamp-one-openclaw --now".to_owned()),
        "{calls:?}"
    );
    assert_eq!(fake.count("ensure"), 2, "{calls:?}");
    assert_eq!(one.agent(), Some(agent_of(2)));
    // Its first words and this task, none into the gone one.
    assert_eq!(fake.count("reply-kept"), replies + 2);
}

/// #7906 sign 3: a record that can't be read is never taken as gone.
#[test]
fn a_kept_session_whose_record_cant_be_read_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    one_turn(&one, &fake, "task-1");
    one_turn(&one, &fake, "task-2");
    assert!(*fake.holds_read.lock().unwrap() > 0);
    assert_eq!(fake.count("close-kept"), 0);
}

fn record(seq: i64, status: &str, last_role: &str) -> TurnRecord {
    TurnRecord {
        session: "w1".into(),
        seq,
        status: Some(status.into()),
        last_role: Some(last_role.into()),
    }
}

/// #7906 sign 4a, by an injected clock: words taken but nothing past the
/// hand-over's record 60 s after Harness's `Taken` is not moving; at 59 s
/// it is still waited on.
#[test]
fn a_taken_turn_with_no_record_progress_in_60_s_is_not_moving() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut watch = TurnWatch::taken(t0, Some(("w1".into(), 7)), TURN_START_WITHIN, TURN_STALL);
    for s in [5, 30, 45, 59] {
        assert_eq!(
            watch.look(Some(&record(7, "done", "assistant")), true, at(s)),
            None,
            "{s} s"
        );
    }
    assert!(matches!(
        watch.look(Some(&record(7, "done", "assistant")), true, at(60)),
        Some(KeptGone::NotMoving(why)) if why.contains("within 60 s")
    ));
}

/// #7906 sign 4: a turn whose record moves, however slowly, is never cut;
/// nor one whose record can't be read.
#[test]
fn a_turn_with_steady_progress_is_kept() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut watch = TurnWatch::taken(t0, Some(("w1".into(), 7)), TURN_START_WITHIN, TURN_STALL);
    for (n, s) in (5..=1800).step_by(5).enumerate() {
        // One event every 2 min 50 s, waiting on the model each time.
        let seq = 8 + (n as i64) / 34;
        let look = watch.look(Some(&record(seq, "running", "user")), true, at(s));
        assert_eq!(look, None, "{s} s");
    }
    let mut unread = TurnWatch::taken(t0, Some(("w1".into(), 7)), TURN_START_WITHIN, TURN_STALL);
    for s in (5..=1800).step_by(5) {
        assert_eq!(unread.look(None, true, at(s)), None, "{s} s");
    }
}

/// #7906 sign 4b: a turn that reached OpenClaw, then `running` with its
/// record unmoved for 3 min and the model's answer never come (a user row
/// or a tool's result last), is stuck; at 2 min 59 s it is not yet.
#[test]
fn a_working_turn_unchanged_for_3_min_is_stuck() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    for last in ["user", "toolResult"] {
        let mut watch = TurnWatch::taken(t0, Some(("w1".into(), 7)), TURN_START_WITHIN, TURN_STALL);
        assert_eq!(
            watch.look(Some(&record(9, "running", last)), true, at(20)),
            None
        );
        assert_eq!(
            watch.look(Some(&record(9, "running", last)), true, at(199)),
            None
        );
        assert!(matches!(
            watch.look(Some(&record(9, "running", last)), true, at(200)),
            Some(KeptGone::NotMoving(why)) if why.contains("did not move for 180 s")
        ));
    }
    // A refused hand-over waiting on such a turn.
    let mut refused = TurnWatch::refused(t0, TURN_STALL);
    assert_eq!(
        refused.look(Some(&record(9, "running", "user")), true, at(0)),
        None
    );
    assert_eq!(
        refused.look(Some(&record(9, "running", "user")), true, at(179)),
        None
    );
    assert!(
        refused
            .look(Some(&record(9, "running", "user")), true, at(180))
            .is_some()
    );
}

/// #7906: a long exec (the assistant's tool call last, no result for
/// 25 min, the record unmoved) is never cut, and its turn goes on once the
/// result is written; nor a turn not open (`ENGINE_NOT_READY`).
#[test]
fn a_long_exec_answered_later_is_never_killed() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut watch = TurnWatch::taken(t0, Some(("w1".into(), 7)), TURN_START_WITHIN, TURN_STALL);
    for s in (5..=1500).step_by(5) {
        let look = watch.look(Some(&record(9, "running", "assistant")), true, at(s));
        assert_eq!(look, None, "{s} s");
    }
    assert_eq!(
        watch.look(Some(&record(10, "running", "toolResult")), true, at(1505)),
        None
    );
    assert_eq!(
        watch.look(Some(&record(11, "done", "assistant")), true, at(1510)),
        None
    );
    let mut closed = TurnWatch::refused(t0, TURN_STALL);
    for s in (0..=1500).step_by(5) {
        assert_eq!(
            closed.look(Some(&record(9, "running", "user")), false, at(s)),
            None
        );
    }
}

/// #7906: a turn open while its session's status says it ended (`killed`,
/// `failed`, `timeout`) for 15 s is dead but working; a short flip back to
/// `running` (a chained run) is not; before the turn reached OpenClaw, the
/// last turn's status is not judged.
#[test]
fn a_turn_open_on_a_session_not_running_for_15_s_is_dead() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut watch = TurnWatch::taken(t0, Some(("w1".into(), 7)), TURN_START_WITHIN, TURN_STALL);
    assert_eq!(
        watch.look(Some(&record(7, "done", "assistant")), true, at(30)),
        None
    );
    assert_eq!(
        watch.look(Some(&record(8, "killed", "user")), true, at(35)),
        None
    );
    assert_eq!(
        watch.look(Some(&record(8, "running", "user")), true, at(45)),
        None
    );
    assert_eq!(
        watch.look(Some(&record(8, "killed", "user")), true, at(50)),
        None
    );
    assert_eq!(
        watch.look(Some(&record(8, "killed", "user")), true, at(64)),
        None
    );
    assert_eq!(
        watch.look(Some(&record(8, "killed", "user")), true, at(65)),
        Some(KeptGone::DeadButWorking("killed".into()))
    );
}

/// #7906 sign 4 end to end, bounds shortened: a kept turn taken whose
/// record never moves fails as not moving and its session is closed
/// `--now`; one whose record moves runs past the stall bound to its
/// answer, and a healthy turn gets no look at all.
#[test]
fn a_kept_turn_not_moving_is_closed_and_a_moving_one_answers() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_turn_watch(
        Duration::from_millis(10),
        Duration::from_millis(80),
        Duration::from_millis(150),
    );
    one.open_if_due(&fake, &place());
    *fake.seq.lock().unwrap() = Some(3);
    // Healthy: answered at once, no show beside its answer's.
    let shows = fake.count("show");
    assert_eq!(
        one_turn(&one, &fake, "task-1"),
        Ended::Answered("answer to New task task-1:".into())
    );
    assert_eq!(fake.count("show"), shows + 1);
    // Moving: 3 holds (~900 ms, past the stall bound), then its answer.
    *fake.moving.lock().unwrap() = true;
    fake.waits
        .lock()
        .unwrap()
        .extend(std::iter::repeat_n("hold".to_owned(), 3));
    assert_eq!(
        one_turn(&one, &fake, "task-2"),
        Ended::Answered("answer to New task task-2:".into())
    );
    assert_eq!(fake.count("close-kept"), 0, "{:?}", fake.calls());
    // Not moving: never past the hand-over's record.
    *fake.moving.lock().unwrap() = false;
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "working".into());
    fake.waits
        .lock()
        .unwrap()
        .extend(std::iter::repeat_n("hold".to_owned(), 1000));
    let began = Instant::now();
    let ended = one_turn(&one, &fake, "task-3");
    assert!(
        matches!(&ended, Ended::Failed(why) if why.contains("reached OpenClaw")),
        "{ended:?}"
    );
    assert!(began.elapsed() < Duration::from_secs(5));
    assert!(
        fake.calls()
            .contains(&"close-kept lamp-one-openclaw --now".to_owned())
    );
    assert_eq!(one.agent(), None, "let go");
}

/// #7906 sign 4: a hand-over refused `NOT_IDLE` while what holds the
/// session does not move ends at the stall bound, not at the run's end,
/// and the session is closed; one that moves is waited on.
#[test]
fn a_refused_hand_over_on_a_turn_not_moving_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_turn_watch(
        Duration::from_millis(10),
        Duration::from_millis(80),
        Duration::from_millis(150),
    );
    one.open_if_due(&fake, &place());
    *fake.seq.lock().unwrap() = Some(3);
    fake.states
        .lock()
        .unwrap()
        .insert(agent_of(1), "working".into());
    // Moving: NOT_IDLE for ~300 ms, then taken.
    *fake.moving.lock().unwrap() = true;
    fake.takes
        .lock()
        .unwrap()
        .extend(std::iter::repeat_n(Took::NotIdle, 60));
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run.", far(), &|| false),
        Sent::Typed(_)
    ));
    drop(turn);
    assert_eq!(fake.count("close-kept"), 0, "{:?}", fake.calls());
    // Not moving: NOT_IDLE past the stall bound; closed and ensured again
    // (still refused here, so the run's own way), never to the run's end.
    *fake.moving.lock().unwrap() = false;
    fake.takes
        .lock()
        .unwrap()
        .extend(std::iter::repeat_n(Took::NotIdle, 1000));
    let turn = one.enter("task-2", Kind::Skill, far(), &|| false).unwrap();
    let began = Instant::now();
    let sent = turn.send(&fake, &place(), "Run.", far(), &|| false);
    assert!(began.elapsed() < Duration::from_secs(5), "{sent:?}");
    assert!(
        fake.calls()
            .contains(&"close-kept lamp-one-openclaw --now".to_owned()),
        "{:?}",
        fake.calls()
    );
}

/// #7906 sign 3 before `ensure`: the kept record's session the Gateway no
/// longer holds is gone, so the open is `--fresh`; held, or unread, or no
/// record, it is not.
#[test]
fn before_ensure_a_kept_record_the_gateway_no_longer_holds_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    assert_eq!(one.kept_gone_before_ensure(&fake), None, "no record");
    one.open_if_due(&fake, &place());
    one_turn(&one, &fake, "task-1");
    assert_eq!(one.kept_gone_before_ensure(&fake), None, "unread");
    *fake.holds.lock().unwrap() = Some(true);
    assert_eq!(one.kept_gone_before_ensure(&fake), None, "held");
    *fake.holds.lock().unwrap() = Some(false);
    assert_eq!(
        one.kept_gone_before_ensure(&fake),
        Some(KeptGone::GatewaySession)
    );
    // A lamp-tasks started again reads the same record.
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    assert_eq!(
        again.kept_gone_before_ensure(&fake),
        Some(KeptGone::GatewaySession)
    );
}

/// #7906 end to end, bounds shortened: a long exec (the record moved past
/// the hand-over, then the assistant's tool call last and nothing more for
/// well past the stall bound) is never cut, and the turn's answer comes.
#[test]
fn a_kept_turn_on_a_long_exec_is_never_closed() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_turn_watch(
        Duration::from_millis(10),
        Duration::from_millis(80),
        Duration::from_millis(150),
    );
    one.open_if_due(&fake, &place());
    *fake.seq.lock().unwrap() = Some(3);
    one_turn(&one, &fake, "task-1");
    let turn = one.enter("task-2", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Build it.", far(), &|| false),
        Sent::Typed(_)
    ));
    *fake.seq.lock().unwrap() = Some(5);
    *fake.last_role.lock().unwrap() = Some("assistant".into());
    // Held ~900 ms, past both bounds: the exec still running.
    fake.waits
        .lock()
        .unwrap()
        .extend(std::iter::repeat_n("hold".to_owned(), 3));
    let ended = turn.wait_end(&fake, MACHINE, &AtomicBool::new(false), &|| {});
    assert_eq!(ended, Ended::Answered("answer to New task task-2:".into()));
    assert_eq!(fake.count("close-kept"), 0, "{:?}", fake.calls());
}

/// #7906's told line: a task whose session died under it and was done
/// again in the replaced one says so once ([`Turn::restarted`]); a healthy
/// turn, and a turn that had run a tool (given up, its own failed line),
/// do not; a replacement with no task waiting (the one-session thread's
/// open) has no turn, so no line.
#[test]
fn the_told_line_comes_once_with_a_waiting_task_and_never_without() {
    let dir = tempfile::tempdir().unwrap();
    let replaced = Arc::new(Mutex::new(0));
    let seen = replaced.clone();
    let (one, fake) = kept_on(dir.path());
    let one = one.with_on_replaced(Box::new(move |_| *seen.lock().unwrap() += 1));
    one.open_if_due(&fake, &place());
    // Healthy: no line.
    let turn = one.enter("task-0", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run.", far(), &|| false),
        Sent::Typed(_)
    ));
    turn.wait_end(&fake, MACHINE, &AtomicBool::new(false), &|| {});
    assert!(!turn.restarted());
    drop(turn);
    // Died under it, no tool run: replaced, done again, one line.
    let turn = one.enter("task-1", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run the skill.", far(), &|| false),
        Sent::Typed(_)
    ));
    fake.shells.lock().unwrap().push(agent_of(1));
    *fake.tools.lock().unwrap() = Some(0);
    let ended = turn.wait_end(&fake, MACHINE, &AtomicBool::new(false), &|| {});
    assert_eq!(ended, Ended::Answered("answer to New task task-1:".into()));
    assert!(turn.restarted(), "one line for the waiting task");
    assert_eq!(*replaced.lock().unwrap(), 1);
    drop(turn);
    // A tool had run: given up, its own failed line, not this one.
    let turn = one.enter("task-2", Kind::Skill, far(), &|| false).unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run the skill.", far(), &|| false),
        Sent::Typed(_)
    ));
    fake.shells.lock().unwrap().push(agent_of(2));
    *fake.tools.lock().unwrap() = Some(2);
    let ended = turn.wait_end(&fake, MACHINE, &AtomicBool::new(false), &|| {});
    assert!(matches!(ended, Ended::Failed(_)), "{ended:?}");
    assert!(!turn.restarted());
    drop(turn);
}

/// #7906's told line with no task: a kept session found a plain shell at
/// the one-session thread's open is replaced, and no turn exists to tell.
#[test]
fn a_replacement_with_no_task_waiting_has_no_told_line() {
    let dir = tempfile::tempdir().unwrap();
    let (one, fake) = kept_on(dir.path());
    one.open_if_due(&fake, &place());
    drop(one);
    fake.shells.lock().unwrap().push(agent_of(1));
    let again = kept_session(dir.path()).with_kept_session(dir.path().join(TOKEN_FILE));
    again.open_if_due(&fake, &place());
    assert_eq!(again.agent(), Some(agent_of(2)));
    let turn = again
        .enter("task-1", Kind::Skill, far(), &|| false)
        .unwrap();
    assert!(matches!(
        turn.send(&fake, &place(), "Run.", far(), &|| false),
        Sent::Typed(_)
    ));
    turn.wait_end(&fake, MACHINE, &AtomicBool::new(false), &|| {});
    assert!(!turn.restarted(), "the replacement was before this task");
}
