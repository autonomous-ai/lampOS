//! The direct path against a fake [`AgentSession`]: every turn, its
//! progress and its end scripted by the test.
use super::*;
use crate::harness::Target;
use lamp_agent_gateway::{Engine, PlanItem, ToolStep};
use std::collections::BTreeMap;

const MACHINE: &str = "0b5b8c1e-5d8a-4c3e-9f1a-2b3c4d5e6f70";

/// Called as a turn is sent.
type OnSend = Box<dyn Fn(&Turn) + Send>;

#[derive(Default)]
pub(crate) struct FakeState {
    /// `ready()` answers NotReady this many more times.
    not_ready: u32,
    readies: u32,
    /// The next `wait` answers Lost, and the runs are forgotten (a
    /// gateway restart that lost them) when `forget` is set.
    lose_next_wait: bool,
    forget: bool,
    sent: Vec<Turn>,
    runs: BTreeMap<RunId, (Progress, Option<Outcome>)>,
    next: u32,
    resets: u32,
    aborted: Vec<RunId>,
    allowlisted: bool,
    allow_adds: u32,
    /// The next sends fail with this, one each.
    fail_sends: Vec<Error>,
    /// The next sends are taken (the run registered, as the gateway's
    /// `chat.send` that started it) and then fail with this, one each: a
    /// reply that timed out or a link that broke after the gateway took it.
    taken_then_fail: Vec<Error>,
    /// Called at each `send`, with the turn: the agent's side (the
    /// launcher's claim, a make's `answer.md`).
    on_send: Option<OnSend>,
    /// The kept session's size as the gateway reads it: tokens, messages.
    gateway_size: Option<(u64, u64)>,
    /// Size reads asked of the gateway.
    size_reads: u32,
}

#[derive(Clone, Default)]
pub(crate) struct Fake(Arc<Mutex<FakeState>>);

impl Fake {
    fn state(&self) -> MutexGuard<'_, FakeState> {
        self.0.lock().unwrap()
    }
    fn run_of(&self, n: usize) -> RunId {
        RunId(format!("run-{n}"))
    }
    fn set(&self, run: &RunId, change: impl FnOnce(&mut Progress)) {
        let mut state = self.state();
        let (progress, _) = state.runs.get_mut(run).unwrap();
        change(progress);
        progress.updated_at = SystemTime::now();
    }
    fn answer(&self, run: &RunId, text: &str) {
        let mut state = self.state();
        let (progress, outcome) = state.runs.get_mut(run).unwrap();
        progress.phase = Phase::Done;
        *outcome = Some(Outcome::Answered {
            text: text.into(),
            progress: progress.clone(),
        });
    }
}

impl AgentSession for Fake {
    fn engine(&self) -> Engine {
        Engine::Openclaw
    }
    fn ready(&mut self, _within: Duration) -> Result<(), Error> {
        let mut state = self.state();
        state.readies += 1;
        if state.not_ready > 0 {
            state.not_ready -= 1;
            return Err(Error::NotReady("the gateway is starting".into()));
        }
        Ok(())
    }
    fn send(&mut self, turn: &Turn) -> Result<RunId, Error> {
        {
            let mut state = self.state();
            if state.not_ready > 0 {
                return Err(Error::NotReady("the gateway is starting".into()));
            }
            if !state.fail_sends.is_empty() {
                return Err(state.fail_sends.remove(0));
            }
        }
        let hook = self.state().on_send.take();
        if let Some(hook) = &hook {
            hook(turn);
        }
        let mut state = self.state();
        state.on_send = hook;
        state.next += 1;
        let run = RunId(format!("run-{}", state.next));
        state.sent.push(turn.clone());
        let now = SystemTime::now();
        state.runs.insert(
            run.clone(),
            (
                Progress {
                    run: run.clone(),
                    task_id: turn.task_id.clone(),
                    phase: Phase::Queued,
                    started_at: now,
                    updated_at: now,
                    tools_done: 0,
                    last_tool: None,
                    last_text: None,
                    plan: Vec::new(),
                },
                None,
            ),
        );
        if !state.taken_then_fail.is_empty() {
            return Err(state.taken_then_fail.remove(0));
        }
        Ok(run)
    }
    fn wait(&mut self, run: &RunId, within: Duration) -> Result<Outcome, Error> {
        {
            let mut state = self.state();
            if std::mem::take(&mut state.lose_next_wait) {
                if state.forget {
                    state.runs.clear();
                }
                return Err(Error::Lost("the gateway restarted".into()));
            }
            match state.runs.get(run) {
                None => return Err(Error::UnknownRun(run.clone())),
                Some((_, Some(outcome))) => return Ok(outcome.clone()),
                Some(_) => {}
            }
        }
        std::thread::sleep(within.min(Duration::from_millis(10)));
        Err(Error::TimedOut)
    }
    fn progress(&self, run: &RunId) -> Option<Progress> {
        self.state()
            .runs
            .get(run)
            .map(|(progress, _)| progress.clone())
    }
    fn running(&self) -> Vec<Progress> {
        self.state()
            .runs
            .values()
            .filter(|(_, outcome)| outcome.is_none())
            .map(|(progress, _)| progress.clone())
            .collect()
    }
    fn abort(&mut self, run: &RunId) -> Result<(), Error> {
        let mut state = self.state();
        state.aborted.push(run.clone());
        if let Some((progress, outcome)) = state.runs.get_mut(run) {
            *outcome = Some(Outcome::Aborted {
                progress: progress.clone(),
            });
        }
        Ok(())
    }
    fn reset(&mut self) -> Result<(), Error> {
        self.state().resets += 1;
        Ok(())
    }
}

impl DirectSession for Fake {
    fn skill_start_allowed(&mut self) -> Result<bool, Error> {
        Ok(self.state().allowlisted)
    }
    fn allow_skill_start(&mut self) -> Result<(), Error> {
        let mut state = self.state();
        state.allow_adds += 1;
        state.allowlisted = true;
        Ok(())
    }
    fn session_size(&mut self) -> Option<crate::harness::SessionSize> {
        let mut state = self.state();
        state.size_reads += 1;
        state
            .gateway_size
            .map(|(tokens, messages)| crate::harness::SessionSize {
                messages,
                bytes: tokens * 4,
            })
    }
}

struct Started {
    direct: Arc<OpenclawDirect>,
    fake: Fake,
    told: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Started {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn started(fake: Fake) -> Started {
    started_with(fake, Duration::from_secs(600), Duration::from_secs(600))
}

fn started_with(fake: Fake, stuck_after: Duration, queued_most: Duration) -> Started {
    let stop = Arc::new(AtomicBool::new(false));
    let told = Arc::new(Mutex::new(Vec::new()));
    let heard = told.clone();
    let direct = Arc::new(
        OpenclawDirect::new(Box::new(fake.clone()), stop.clone())
            .with_bounds(
                Duration::from_millis(5),
                Duration::from_millis(5),
                Duration::from_millis(5),
            )
            .with_getting_ready(
                Duration::from_millis(100),
                Box::new(move |why| heard.lock().unwrap().push(why.to_owned())),
            )
            .with_limits(stuck_after, queued_most),
    );
    direct.set_lamp_machine(Box::new(|| Some(MACHINE.to_owned())));
    direct.start().unwrap();
    Started {
        direct,
        fake,
        told,
        stop,
    }
}

fn until(what: &str, done: impl Fn() -> bool) {
    let by = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < by, "never: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn launch(engine: EngineKind, words: &str) -> Launch {
    Launch {
        name: "lamp-0123456789abcdef0123456789abcdef".into(),
        prompt: words.into(),
        target: Target {
            machine_id: MACHINE.into(),
            project: "/var/lib/lampos-tasks/work".into(),
            engine,
        },
        session: None,
    }
}

/// A spoken launch as the tasks-harness worker makes it: the session's
/// readiness waited for, at most `within`, by `gateway::launch_with`.
fn spoken_within(lamp: &Started, launch: &Launch, within: Duration) -> Option<LaunchOutcome> {
    let mut gateway = lamp.direct.gateway(within);
    Some(crate::gateway::launch_with(
        Some(&mut gateway),
        launch,
        || lamp.direct.spoken_launch(launch).unwrap(),
    ))
}

fn joined(outcome: Option<LaunchOutcome>) -> String {
    match outcome {
        Some(LaunchOutcome::Joined { agent_id, .. }) => agent_id,
        other => panic!("not joined: {other:?}"),
    }
}

fn watched(task: u64, agent: &str) -> Watched {
    Watched {
        task_id: task,
        machine_id: MACHINE.into(),
        agent_id: agent.into(),
        engine: "openclaw".into(),
        accepted_ago_ms: Some(30_000),
    }
}

fn skill_run(id: &str, deadline: Duration) -> crate::skill_task::SkillRun {
    crate::skill_task::SkillRun {
        run: id.into(),
        skill: "weather".into(),
        prompt: "Run skill-start".into(),
        folder: "/tmp/skill".into(),
        script: "/tmp/skill/run.py".into(),
        task: "{}".into(),
        env: Vec::new(),
        deadline,
        started: AtomicBool::new(false),
        write: false,
        not_made: AtomicBool::new(false),
        start_in: None,
        env_file: None,
        answer: OnceLock::new(),
        net: false,
        tells: true,
        cancel: Arc::new(AtomicBool::new(false)),
    }
}

fn grant() -> crate::skill_start::Grant {
    crate::skill_start::Grant {
        script: "/tmp/skill/run.py".into(),
        interpreter: "python3".into(),
        skill: "/tmp/skill".into(),
        run: "/tmp/run".into(),
        socket: String::new(),
        pythonpath: String::new(),
        token: "secret".into(),
        egress: None,
    }
}

fn mid_run(run: RunId) -> Progress {
    let now = SystemTime::now();
    Progress {
        run,
        task_id: "lamp-x".into(),
        phase: Phase::Tool,
        started_at: now,
        updated_at: now,
        tools_done: 2,
        last_tool: Some(ToolStep {
            name: "web_search".into(),
            brief: "cheapest flight Hanoi\nDecember".into(),
            running: true,
        }),
        last_text: Some("Looking at three airlines".into()),
        plan: vec![
            PlanItem {
                text: "search flights".into(),
                done: true,
            },
            PlanItem {
                text: "compare prices".into(),
                done: false,
            },
            PlanItem {
                text: "write the answer".into(),
                done: false,
            },
        ],
    }
}

/// The progress mapping (fail-first): a run mid-tool reads `working`, with
/// the plan's step, the tool and its brief made plain; tools counted; no
/// answer and never idle while it runs; idle with its answer once answered.
#[test]
fn progress_maps_to_the_ps_and_show_rows() {
    let progress = mid_run(RunId("run-1".into()));
    let row = listed_of("a", Some(&progress), None);
    assert_eq!(row.state, "working");
    assert_eq!(
        row.about.as_deref(),
        Some("on step 2 of 3: compare prices; running web_search: cheapest flight Hanoi December")
    );
    assert_eq!(row.agent.as_deref(), Some("openclaw"));
    assert!(row.last_active_unix_ms.is_some());
    let seen = observation_of("a", Some(&progress), None);
    assert_eq!((seen.state.as_str(), seen.tools), ("working", 2));
    assert!(seen.answer.is_empty());
    assert_eq!(step_of(&progress), Some((3, "web_search".into())));
    // No tool, no plan: the tail of what it wrote, at most 160 characters.
    let mut writing = progress.clone();
    writing.plan.clear();
    writing.last_tool = None;
    writing.phase = Phase::Writing;
    writing.last_text = Some(format!("{}END", "x".repeat(300)));
    let about = listed_of("a", Some(&writing), None).about.unwrap();
    assert_eq!(about.chars().count(), ABOUT_MOST);
    assert!(about.ends_with("END"));
    // Answered: idle with the answer, cut to the store's bound, truncated.
    let long = "y".repeat(MAX_ANSWER_BYTES + 10);
    let end = End::Answered(long);
    let done = observation_of("a", Some(&progress), Some(&end));
    assert_eq!(done.state, "idle");
    assert_eq!(done.answer.len(), MAX_ANSWER_BYTES);
    assert!(done.truncated);
    // An empty answer, a failure and an abort settle, never idle unanswered.
    let empty = End::Answered(" ".into());
    assert_eq!(state_of(false, None, Some(&empty)), "failed");
    assert_eq!(state_of(false, None, Some(&End::Aborted)), "stopped");
    assert_eq!(state_of(true, None, None), "starting");
}

/// The line never carries a file path nor a word the sign matcher reads as
/// the work done (`Sign::Done`), so no unasked turn is started mid-run.
#[test]
fn about_leaves_out_paths_and_done_words() {
    let mut progress = mid_run(RunId("run-1".into()));
    progress.plan.iter_mut().for_each(|item| item.done = true);
    progress.last_tool = Some(ToolStep {
        name: "write".into(),
        brief: "/home/lampos/notes.md finished, done.".into(),
        running: false,
    });
    let line = about(&progress).unwrap();
    assert_eq!(line, "past step 3 of 3; ran write");
    assert!(crate::progress::signs_in(&line).is_empty(), "{line}");
    progress.plan.clear();
    progress.last_tool = None;
    progress.last_text = Some("Implemented it in ~/x.py, all done".into());
    let line = about(&progress).unwrap();
    assert_eq!(line, "it in all");
    assert!(crate::progress::signs_in(&line).is_empty());
}

/// U1: the pump connects at once, with no ask; a gateway not up at boot is
/// looked at again with backoff until ready; the first ask then does not
/// wait.
#[test]
fn ready_at_boot_with_backoff_and_no_wait_for_the_first_ask() {
    let fake = Fake::default();
    fake.state().not_ready = 3;
    let lamp = started(fake);
    until("ready", || lamp.direct.is_ready());
    assert!(lamp.fake.state().readies >= 4);
    // Nothing waited: no getting-ready line.
    assert!(lamp.told.lock().unwrap().is_empty());
    let began = Instant::now();
    joined(spoken_within(
        &lamp,
        &launch(EngineKind::Openclaw, "What's the weather?"),
        crate::gateway::READY_WITHIN,
    ));
    assert!(began.elapsed() < Duration::from_millis(100));
}

/// U2 + U4: a spoken ask handed into the kept session with its header and
/// words; followed by its own agent id; idle with the answer at its end,
/// which is what settles the store's task and fires the unasked news.
#[test]
fn a_spoken_task_is_handed_over_and_answered() {
    let lamp = started(Fake::default());
    until("ready", || lamp.direct.is_ready());
    let agent = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Plan my trip")),
    );
    assert!(crate::harness::agent_id(&agent), "{agent}");
    until("sent", || lamp.fake.state().sent.len() == 1);
    let turn = lamp.fake.state().sent[0].clone();
    assert_eq!(turn.kind, TurnKind::Spoken);
    assert!(
        turn.message
            .starts_with("New task lamp-0123456789abcdef0123456789abcdef:\n")
    );
    assert!(turn.message.ends_with("\nPlan my trip"));
    let seen = lamp.direct.observe(&agent).unwrap();
    assert_ne!(seen.state, "idle");
    assert_eq!(seen.ask.as_deref(), Some("Plan my trip"));
    lamp.fake
        .answer(&lamp.fake.run_of(1), "Hanoi on the 3rd, 120 dollars.");
    until("answered", || {
        lamp.direct.observe(&agent).unwrap().state == "idle"
    });
    let seen = lamp.direct.observe(&agent).unwrap();
    assert_eq!(seen.answer, "Hanoi on the 3rd, 120 dollars.");
    assert!(!seen.truncated);
}

/// U3: "how is it going?" mid-run: the tracker (`get_progress`), the work
/// record (`work`/`work_list`) and "tasks now" read from the run's own
/// progress at once: working, the plan's step and the tool, tools and step
/// counted, elapsed time; never done while it runs.
#[test]
fn how_far_along_mid_run_with_tool_progress() {
    let lamp = started(Fake::default());
    until("ready", || lamp.direct.is_ready());
    let agent = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Find a flight")),
    );
    until("sent", || lamp.fake.state().sent.len() == 1);
    let run = lamp.fake.run_of(1);
    lamp.fake.set(&run, |progress| {
        let mid = mid_run(run.clone());
        *progress = Progress {
            started_at: progress.started_at,
            ..mid
        };
    });
    until("progress", || {
        lamp.direct.listed()[0]
            .about
            .as_deref()
            .is_some_and(|a| a.contains("web_search"))
    });
    let task = 9_101;
    let watch = [watched(task, &agent)];
    let mut tracker = crate::progress::Tracker::started(1);
    tracker.watching(&watch);
    lamp.direct.refresh(Some(&mut tracker), &watch);
    let words = tracker.words(task).unwrap();
    assert_eq!(words["state"], "working");
    assert!(
        words["about"]
            .as_str()
            .unwrap()
            .starts_with("on step 2 of 3: compare prices; running web_search")
    );
    // The work record, as `work` answers it.
    let work = crate::works::TaskWork {
        task_id: task,
        engine: EngineKind::Openclaw,
        words: "Find a flight".into(),
        state: None,
        tools: lamp.direct.observe(&agent).unwrap().tools,
        answer: None,
        session: Some((MACHINE.into(), agent.clone())),
        started_ago_ms: Some(30_000),
        ended_ago_ms: None,
    };
    // As `works_refresh`: the records brought up to date, then the steps.
    crate::works::with(|board| {
        board.tasks(
            &[work],
            |id| crate::works::line_of(Some(&tracker), id),
            crate::works::unix_ms(),
        );
    });
    lamp.direct.record_steps(&watch);
    let record =
        crate::works::with(|board| board.get(&task.to_string(), crate::works::unix_ms())).unwrap();
    assert_eq!(record["state"], "working");
    assert_eq!(record["done"], false);
    assert_eq!(record["tools"], 2);
    assert_eq!(record["step"]["n"], 3);
    assert_eq!(record["step"]["tool"], "web_search");
    assert!(record["elapsed_s"].as_u64().unwrap() >= 30);
    assert!(record["answer"].is_null());
    // "Tasks now" takes the same line.
    let line = tracker.line(task).unwrap();
    assert_eq!(
        crate::now::step(line.about.unwrap(), ABOUT_MOST).as_deref(),
        Some("on step 2 of 3: compare prices; running web_search: cheapest flight Hanoi December")
    );
}

/// U5: a skill's `do.task`: the fence's allow-list put back over the
/// session first; the run handed over; its answer kept only once the
/// launcher claimed the start and the fence said the script started.
#[test]
fn a_skill_do_task_runs_through_the_skill_start_fence() {
    let fake = Fake::default();
    let starts = crate::skill_start::Starts::default();
    let claims = starts.clone();
    fake.state().on_send = Some(Box::new(move |turn| {
        if turn.task_id == "run-fenced" {
            claims.claim("run-fenced").unwrap();
            claims.started_for_tests("run-fenced");
        }
    }));
    let lamp = started(fake);
    until("ready", || lamp.direct.is_ready());
    let answerer = lamp.fake.clone();
    std::thread::spawn(move || {
        until("sent", || answerer.state().sent.len() == 1);
        answerer.answer(&answerer.run_of(1), "It is 24 degrees.");
    });
    let run = skill_run("run-fenced", Duration::from_secs(5));
    let live = starts.open("run-fenced", grant()).unwrap();
    let began = Instant::now();
    let ended = skill_turn(
        &lamp.direct,
        &run,
        Some(&live),
        Path::new("/nonexistent"),
        "Run skill-start run-fenced",
        false,
        began,
        began + run.deadline,
        &|| false,
    );
    assert_eq!(ended, Ok(0));
    assert_eq!(
        run.answer.get().map(String::as_str),
        Some("It is 24 degrees.")
    );
    assert!(run.started.load(Ordering::Acquire));
    let state = lamp.fake.state();
    assert_eq!(state.allow_adds, 1);
    assert_eq!(state.sent[0].kind, TurnKind::SkillTask);
}

/// The fence fails closed: an agent that answered without claiming the
/// start never ran the skill's script, so no answer is kept.
#[test]
fn a_skill_do_task_whose_start_was_never_claimed_fails_closed() {
    let fake = Fake::default();
    fake.state().allowlisted = true;
    let lamp = started(fake);
    until("ready", || lamp.direct.is_ready());
    let answerer = lamp.fake.clone();
    std::thread::spawn(move || {
        until("sent", || answerer.state().sent.len() == 1);
        answerer.answer(&answerer.run_of(1), "Done!");
    });
    let starts = crate::skill_start::Starts::default();
    let run = skill_run("run-unclaimed", Duration::from_secs(5));
    let live = starts.open("run-unclaimed", grant()).unwrap();
    let began = Instant::now();
    let ended = skill_turn(
        &lamp.direct,
        &run,
        Some(&live),
        Path::new("/nonexistent"),
        "Run skill-start run-unclaimed",
        false,
        began,
        began + run.deadline,
        &|| false,
    );
    assert_eq!(ended, Err(crate::gateway::NEVER_STARTED.to_owned()));
    assert!(run.answer.get().is_none());
    assert_eq!(lamp.fake.state().allow_adds, 0);
}

/// U5: a make goes through OpenClaw as a make turn, no allow-list asked,
/// and its answer is the make folder's `answer.md`.
#[test]
fn a_make_goes_through_the_kept_session() {
    let folder = tempfile::tempdir().unwrap();
    let answer = folder.path().join(crate::skill_make::ANSWER_FILE);
    let fake = Fake::default();
    fake.state().on_send = Some(Box::new(move |turn| {
        if turn.kind == TurnKind::Make {
            std::fs::write(&answer, "I made the plant-watering skill.").unwrap();
        }
    }));
    let lamp = started(fake);
    until("ready", || lamp.direct.is_ready());
    let answerer = lamp.fake.clone();
    std::thread::spawn(move || {
        until("sent", || answerer.state().sent.len() == 1);
        answerer.answer(&answerer.run_of(1), "see answer.md");
    });
    let run = skill_run("make-1", Duration::from_secs(5));
    let began = Instant::now();
    let ended = skill_turn(
        &lamp.direct,
        &run,
        None,
        folder.path(),
        "Make a skill that waters the plant",
        true,
        began,
        began + run.deadline,
        &|| false,
    );
    assert_eq!(ended, Ok(0));
    assert_eq!(
        run.answer.get().map(String::as_str),
        Some("I made the plant-watering skill.")
    );
    let state = lamp.fake.state();
    assert_eq!(state.sent[0].kind, TurnKind::Make);
    assert_eq!(state.allow_adds, 0);
}

/// The gateway not ready: past the bound the getting-ready hook is told
/// once; once ready, the waiting task goes over.
#[test]
fn not_ready_then_ready_tells_getting_ready_once() {
    let fake = Fake::default();
    fake.state().not_ready = 1_000;
    let lamp = started(fake);
    let gate = lamp.fake.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        gate.state().not_ready = 0;
    });
    let agent = joined(spoken_within(
        &lamp,
        &launch(EngineKind::Openclaw, "Hello"),
        crate::gateway::READY_WITHIN,
    ));
    assert!(lamp.direct.observe(&agent).is_some());
    let told = lamp.told.lock().unwrap().clone();
    assert_eq!(
        told,
        vec![Error::NotReady("the gateway is starting".into()).to_string()]
    );
    until("sent", || lamp.fake.state().sent.len() == 1);
}

/// Never ready within the spoken bound: not sent, `openclaw_gateway_not_ready`,
/// which task news tells as "still getting ready".
#[test]
fn never_ready_is_not_sent_as_not_ready() {
    let fake = Fake::default();
    fake.state().not_ready = u32::MAX;
    let lamp = started(fake);
    let outcome = spoken_within(
        &lamp,
        &launch(EngineKind::Openclaw, "Hello"),
        Duration::from_millis(300),
    );
    assert_eq!(
        outcome.unwrap(),
        LaunchOutcome::NotSent {
            code: crate::gateway::NOT_READY.into()
        }
    );
    assert_eq!(lamp.told.lock().unwrap().len(), 1);
    assert!(lamp.fake.state().sent.is_empty());
}

/// Lost mid-run: the session reconnects; a run the restart lost ends
/// failed (told as failed, never waited on for ever), and the next task
/// goes over and is answered.
#[test]
fn lost_then_the_next_task_after_reconnect() {
    let lamp = started(Fake::default());
    until("ready", || lamp.direct.is_ready());
    let first = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "First")),
    );
    until("sent", || lamp.fake.state().sent.len() == 1);
    {
        let mut state = lamp.fake.state();
        state.lose_next_wait = true;
        state.forget = true;
        state.not_ready = 2;
    }
    until("first failed", || {
        lamp.direct.observe(&first).unwrap().state == "failed"
    });
    let second = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Second")),
    );
    until("second sent", || lamp.fake.state().sent.len() == 2);
    lamp.fake.answer(&lamp.fake.run_of(2), "Second done.");
    until("second answered", || {
        lamp.direct.observe(&second).unwrap().answer == "Second done."
    });
}

/// A run lost with the connection but kept by the gateway (resynced after
/// the reconnect) is answered as it would have been.
#[test]
fn lost_then_resynced_run_is_still_answered() {
    let lamp = started(Fake::default());
    until("ready", || lamp.direct.is_ready());
    let agent = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Long one")),
    );
    until("sent", || lamp.fake.state().sent.len() == 1);
    lamp.fake.state().lose_next_wait = true;
    until("lost seen", || !lamp.fake.state().lose_next_wait);
    lamp.fake.answer(&lamp.fake.run_of(1), "Done after all.");
    until("answered", || {
        lamp.direct.observe(&agent).unwrap().answer == "Done after all."
    });
    assert_eq!(lamp.fake.state().sent.len(), 1);
}

/// A cancelled skill run is aborted on the gateway.
#[test]
fn a_cancelled_run_is_aborted() {
    let lamp = started(Fake::default());
    until("ready", || lamp.direct.is_ready());
    let run = skill_run("make-2", Duration::from_millis(300));
    let began = Instant::now();
    let ended = skill_turn(
        &lamp.direct,
        &run,
        None,
        Path::new("/nonexistent"),
        "Make something slow",
        true,
        began,
        began + run.deadline,
        &|| false,
    );
    assert!(ended.unwrap_err().contains("deadline"));
    assert_eq!(lamp.fake.state().aborted, vec![lamp.fake.run_of(1)]);
}

/// Context hygiene: past the one session's bound, a fresh conversation
/// (`reset()`) before the next turn.
#[test]
fn near_the_context_limit_the_session_is_reset() {
    let lamp = started(Fake::default());
    until("ready", || lamp.direct.is_ready());
    let big = "w".repeat(usize::try_from(crate::one_session::TASK_FRESH_TOKENS * 4).unwrap());
    let first = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, &big)),
    );
    until("sent", || lamp.fake.state().sent.len() == 1);
    lamp.fake.answer(&lamp.fake.run_of(1), "ok");
    until("answered", || {
        lamp.direct.observe(&first).unwrap().state == "idle"
    });
    assert_eq!(lamp.fake.state().resets, 0);
    joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Next")),
    );
    until("second sent", || lamp.fake.state().sent.len() == 2);
    assert_eq!(lamp.fake.state().resets, 1);
    assert_eq!(lamp.direct.resets(), 1);
}

/// ee17: the gateway's own count of the kept session, read before the
/// turn, past the one session's bound for a task (85% of the 128k context,
/// under the old 150k) or past its 150 messages: a fresh conversation
/// before the next turn. Below both: none.
#[test]
fn the_gateways_own_size_near_the_context_limit_resets_the_session() {
    for (size, resets) in [((110_000, 10), 1), ((1_000, 151), 1), ((100_000, 150), 0)] {
        let fake = Fake::default();
        fake.state().gateway_size = Some(size);
        let lamp = started(fake);
        until("ready", || lamp.direct.is_ready());
        joined(
            lamp.direct
                .spoken_launch(&launch(EngineKind::Openclaw, "Next")),
        );
        until("sent", || lamp.fake.state().sent.len() == 1);
        assert_eq!(lamp.fake.state().resets, resets, "{size:?}");
        assert!(lamp.fake.state().size_reads >= 1, "read before the send");
    }
}

/// The owner's call (2026-11-23): a drop under the run (the client's
/// `DROPPED`) fails it at once with the person's "try again" words, as its
/// observation's last words for the voice's news, and the turn queued
/// behind it ends the same, never handed over.
#[test]
fn a_drop_fails_the_run_and_the_queued_turns_with_the_try_again_words() {
    let dropped = lamp_agent_gateway::openclaw::DROPPED;
    let lamp = started(Fake::default());
    until("ready", || lamp.direct.is_ready());
    let first = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "First")),
    );
    until("sent", || lamp.fake.state().sent.len() == 1);
    let second = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Second")),
    );
    {
        let mut state = lamp.fake.state();
        let (progress, outcome) = state.runs.get_mut(&RunId("run-1".into())).unwrap();
        progress.phase = Phase::Failed {
            why: dropped.into(),
        };
        *outcome = Some(Outcome::Failed {
            why: dropped.into(),
            progress: progress.clone(),
        });
    }
    until("failed", || {
        lamp.direct.observe(&first).unwrap().state == "failed"
    });
    let seen = lamp.direct.observe(&first).unwrap();
    assert_eq!(seen.answer, dropped);
    let queued = lamp.direct.observe(&second).unwrap();
    assert_eq!(
        (queued.state.as_str(), queued.answer.as_str()),
        ("failed", dropped)
    );
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(lamp.fake.state().sent.len(), 1, "never handed over");
}

/// Hermes, a named session or another machine is not the direct path's;
/// and with the switch off (anything but `on`) nothing is set, so every
/// seam leaves the Harness path untouched.
#[test]
fn switch_off_and_hermes_keep_the_harness_path() {
    assert!(!on(None));
    assert!(!on(Some("1")));
    assert!(!on(Some("ON")));
    assert!(on(Some("on")));
    // The crate's tests never set the lamp's: every seam is skipped.
    assert!(LAMP.get().is_none());
    assert!(lamp_owns("0b5b8c1e-5d8a-4c3e-9f1a-2b3c4d5e6f70").is_none());
    let lamp = started(Fake::default());
    assert!(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Hermes, "Hello"))
            .is_none()
    );
    let mut named = launch(EngineKind::Openclaw, "Hello");
    named.session = Some("docs".into());
    assert!(lamp.direct.spoken_launch(&named).is_none());
    let mut elsewhere = launch(EngineKind::Openclaw, "Hello");
    elsewhere.target.machine_id = "0123456789abcdef0123456789abcdef".into();
    assert!(lamp.direct.spoken_launch(&elsewhere).is_none());
    assert!(lamp.fake.state().sent.is_empty());
}

/// A run with no event past the stuck bound is checked, aborted and ended
/// failed with a short reason; the next turn goes on and is answered.
#[test]
fn a_stuck_run_is_ended_and_the_next_turn_goes_on() {
    let lamp = started_with(
        Fake::default(),
        Duration::from_millis(150),
        Duration::from_secs(600),
    );
    until("ready", || lamp.direct.is_ready());
    let stuck = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Stuck one")),
    );
    let next = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Next one")),
    );
    // One at a time: the second waits, and says so.
    until("first sent", || lamp.fake.state().sent.len() == 1);
    let waiting = lamp
        .direct
        .listed()
        .into_iter()
        .find(|row| row.agent_id == next)
        .unwrap();
    assert_eq!(waiting.about.as_deref(), Some(QUEUED_ABOUT));
    assert_eq!(waiting.state, "starting");
    until("stuck ended", || {
        lamp.direct.observe(&stuck).unwrap().state == "failed"
    });
    assert_eq!(lamp.fake.state().aborted, vec![lamp.fake.run_of(1)]);
    until("next sent", || lamp.fake.state().sent.len() == 2);
    lamp.fake.answer(&lamp.fake.run_of(2), "Next answered.");
    until("next answered", || {
        lamp.direct.observe(&next).unwrap().answer == "Next answered."
    });
}

/// A send the gateway took but whose reply timed out: the retried turn
/// adopts the run the gateway already has for it (by its task id), never
/// sends it twice; one run, answered once.
#[test]
fn a_retried_send_adopts_the_run_the_gateway_already_took() {
    let fake = Fake::default();
    fake.state().taken_then_fail = vec![Error::TimedOut];
    let lamp = started(fake);
    until("ready", || lamp.direct.is_ready());
    let agent = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Hello")),
    );
    until("taken", || lamp.fake.state().sent.len() == 1);
    until("adopted", || {
        lamp.direct
            .lock()
            .find(&agent)
            .is_some_and(|run| run.run.is_some())
    });
    std::thread::sleep(Duration::from_millis(100));
    lamp.fake.answer(&lamp.fake.run_of(1), "Hi.");
    until("answered", || {
        lamp.direct.observe(&agent).unwrap().answer == "Hi."
    });
    let state = lamp.fake.state();
    assert_eq!(state.sent.len(), 1, "never sent twice");
    assert_eq!(state.runs.len(), 1, "one run");
}

/// Two sends failed in a row make the session broken: a fresh session,
/// and the waiting turn tried once more, answered.
#[test]
fn a_broken_session_is_reset_and_the_turn_tried_again() {
    let fake = Fake::default();
    fake.state().fail_sends = vec![Error::TimedOut, Error::TimedOut];
    let lamp = started(fake);
    until("ready", || lamp.direct.is_ready());
    let agent = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Hello")),
    );
    until("sent", || lamp.fake.state().sent.len() == 1);
    assert_eq!(lamp.fake.state().resets, 1);
    lamp.fake.answer(&lamp.fake.run_of(1), "Hi.");
    until("answered", || {
        lamp.direct.observe(&agent).unwrap().answer == "Hi."
    });
}

/// A refused session is reset at once; refused again, the turn fails.
#[test]
fn a_refused_session_is_reset_once_then_the_turn_fails() {
    let fake = Fake::default();
    fake.state().fail_sends = vec![
        Error::Refused("scope".into()),
        Error::Refused("scope".into()),
    ];
    let lamp = started(fake);
    until("ready", || lamp.direct.is_ready());
    let agent = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Hello")),
    );
    until("failed", || {
        lamp.direct.observe(&agent).unwrap().state == "failed"
    });
    assert_eq!(lamp.fake.state().resets, 1);
    assert!(lamp.fake.state().sent.is_empty());
}

/// The gateway down after it was ready: the queued turn waits, getting
/// ready is said once, and past the queued bound it ends not ready; once
/// the gateway is up again the next task goes over and is answered.
#[test]
fn gateway_down_then_up() {
    let lamp = started_with(
        Fake::default(),
        Duration::from_secs(600),
        Duration::from_millis(400),
    );
    until("ready", || lamp.direct.is_ready());
    lamp.fake.state().not_ready = u32::MAX;
    let first = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "First")),
    );
    until("first ended", || {
        lamp.direct.observe(&first).unwrap().state == "failed"
    });
    assert_eq!(lamp.told.lock().unwrap().len(), 1);
    assert!(lamp.fake.state().sent.is_empty());
    lamp.fake.state().not_ready = 0;
    until("ready again", || lamp.direct.is_ready());
    let second = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Second")),
    );
    until("second sent", || lamp.fake.state().sent.len() == 1);
    lamp.fake.answer(&lamp.fake.run_of(1), "Up again.");
    until("second answered", || {
        lamp.direct.observe(&second).unwrap().answer == "Up again."
    });
    assert_eq!(lamp.told.lock().unwrap().len(), 1);
}

/// The lamp's session for a store test on this thread, owning `agent`
/// with `answer`: no pump, nothing sent.
/// The lamp's direct session with `agent`'s turn failed by a gateway drop.
pub(crate) fn lamp_dropped(agent: &str) -> Arc<OpenclawDirect> {
    let direct = Arc::new(OpenclawDirect::new(
        Box::new(Fake::default()),
        Arc::new(AtomicBool::new(true)),
    ));
    direct.ended_for_tests(
        agent,
        End::Failed(lamp_agent_gateway::openclaw::DROPPED.into()),
    );
    direct
}

pub(crate) fn lamp_owning(agent: &str, answer: &str) -> Arc<OpenclawDirect> {
    let direct = Arc::new(OpenclawDirect::new(
        Box::new(Fake::default()),
        Arc::new(AtomicBool::new(true)),
    ));
    direct.answered_for_tests(agent, answer);
    direct
}

/// A quiet run thinking (no tool open) is stuck at the base limit.
#[test]
fn a_quiet_thinking_run_is_stuck_at_the_base_limit() {
    let base = Duration::from_secs(300);
    let mut progress = mid_run(RunId("run-1".into()));
    progress.phase = Phase::Thinking;
    progress.last_tool.as_mut().unwrap().running = false;
    let at = progress.updated_at;
    assert_eq!(quiet_limit(base, &progress), base);
    assert!(!quiet_past(base, &progress, at + Duration::from_secs(299)));
    assert!(quiet_past(base, &progress, at + base));
    progress.last_tool = None;
    assert!(quiet_past(base, &progress, at + base));
}

/// A tool started with no result yet (a build, a `sleep`) is not stuck at
/// the base limit, only at six times it.
#[test]
fn a_running_tool_is_stuck_only_at_six_times_the_base_limit() {
    let base = Duration::from_secs(300);
    let progress = mid_run(RunId("run-1".into()));
    assert!(progress.last_tool.as_ref().unwrap().running);
    let at = progress.updated_at;
    assert_eq!(quiet_limit(base, &progress), Duration::from_secs(1800));
    assert!(!quiet_past(base, &progress, at + base));
    assert!(!quiet_past(
        base,
        &progress,
        at + Duration::from_secs(6 * 60)
    ));
    assert!(!quiet_past(base, &progress, at + Duration::from_secs(1799)));
    assert!(quiet_past(base, &progress, at + Duration::from_secs(1800)));
}

/// `LAMP_TASKS_DIRECT_STUCK_S`: whole seconds, 10 or more; else the default.
#[test]
fn the_stuck_limit_override_is_read() {
    assert_eq!(STUCK_SWITCH, "LAMP_TASKS_DIRECT_STUCK_S");
    assert_eq!(stuck_after_of(Some("60")), Some(Duration::from_secs(60)));
    assert_eq!(stuck_after_of(Some(" 10 ")), Some(Duration::from_secs(10)));
    for bad in [
        None,
        Some(""),
        Some("9"),
        Some("0"),
        Some("-5"),
        Some("1m"),
        Some("2.5"),
    ] {
        assert_eq!(stuck_after_of(bad), None, "{bad:?}");
    }
}

/// ee17 row, 13:58:48: a turn the gateway took reads `working` on the
/// status rows (the bus's `tasks/status`) and in its observation before
/// the run's first event, as `get_progress` did; `starting` is only a turn
/// not yet handed over.
#[test]
fn a_handed_over_run_reads_working_before_its_first_event() {
    let lamp = started(Fake::default());
    until("ready", || lamp.direct.is_ready());
    let agent = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Plan a trip")),
    );
    until("sent", || lamp.fake.state().sent.len() == 1);
    until("recorded", || {
        lamp.direct
            .listed()
            .iter()
            .any(|row| row.agent_id == agent && row.last_active_unix_ms.is_some())
            && lamp
                .direct
                .lock()
                .find(&agent)
                .is_some_and(|run| run.run.is_some())
    });
    let row = lamp
        .direct
        .listed()
        .into_iter()
        .find(|row| row.agent_id == agent)
        .unwrap();
    assert_eq!(row.state, "working");
    assert_eq!(lamp.direct.observe(&agent).unwrap().state, "working");
    // Not yet handed over: starting.
    assert_eq!(state_of(true, None, None), "starting");
    assert_eq!(state_of(false, None, None), "working");
    assert_eq!(state_of(false, Some(&Phase::Queued), None), "working");
}

/// ee17 row, 14:04:35: a make whose folder name has no run id still hands
/// over a turn with a task id, so its journal line names it.
#[test]
fn a_make_with_no_run_id_still_has_a_task_id() {
    let folder = tempfile::tempdir().unwrap();
    let answer = folder.path().join(crate::skill_make::ANSWER_FILE);
    let fake = Fake::default();
    fake.state().on_send = Some(Box::new(move |_| {
        std::fs::write(&answer, "Made.").unwrap();
    }));
    let lamp = started(fake);
    until("ready", || lamp.direct.is_ready());
    let answerer = lamp.fake.clone();
    std::thread::spawn(move || {
        until("sent", || answerer.state().sent.len() == 1);
        answerer.answer(&answerer.run_of(1), "see answer.md");
    });
    let mut run = skill_run("", Duration::from_secs(5));
    run.skill = "make".into();
    let began = Instant::now();
    let ended = skill_turn(
        &lamp.direct,
        &run,
        None,
        folder.path(),
        "Make a skill",
        true,
        began,
        began + run.deadline,
        &|| false,
    );
    assert_eq!(ended, Ok(0));
    let task_id = lamp.fake.state().sent[0].task_id.clone();
    assert!(!task_id.trim().is_empty(), "task id: {task_id:?}");
    assert!(task_id.starts_with("make"), "task id: {task_id:?}");
}

/// The bus's `tasks/status` follows a direct run as its row changes, with
/// no `ps` read: handed over, the tracker's status says working; read again
/// unchanged, nothing new to publish.
#[test]
fn the_bus_status_follows_a_direct_run_without_ps() {
    let lamp = started(Fake::default());
    until("ready", || lamp.direct.is_ready());
    let agent = joined(
        lamp.direct
            .spoken_launch(&launch(EngineKind::Openclaw, "Plan a trip")),
    );
    until("handed over", || {
        lamp.direct
            .lock()
            .find(&agent)
            .is_some_and(|run| run.run.is_some())
    });
    let watch = vec![watched(5, &agent)];
    let mut tracker = crate::progress::Tracker::started(1);
    tracker.watching(&watch);
    lamp.direct.refresh_if_changed(Some(&mut tracker), &watch);
    let status = tracker.take("store").expect("a fresh status");
    let task = status.tasks.iter().find(|task| task.task == 5).unwrap();
    assert_eq!(task.state, lamp_msgs::AgentState::Working);
    lamp.direct.refresh_if_changed(Some(&mut tracker), &watch);
    assert!(tracker.take("store").is_none());
}
