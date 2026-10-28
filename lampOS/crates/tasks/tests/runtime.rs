use lamp_bus::Bus;
use lamp_msgs::{AgentState, Diagnostic, Level, Sign, TaskStatus};
use lamp_tasks::{
    engine::{Clock as Sample, WATCH_EARLY_MS},
    harness::{
        AccountMachine, Closed, Ended, EngineKind, Launch, LaunchOutcome, Listed, Observation,
        SignIn, Target,
    },
    progress::SETTLED_FOR_MS,
    runtime::{Adapter, Clock, Config, SetUpLater, WaitLane, request, serve_with_adapter},
    store::Store,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs, io,
    io::Write,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc, Mutex, PoisonError, RwLock, RwLockReadGuard,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const AGENT: &str = "11111111-1111-4111-8111-111111111111";

/// The person's own harness on the same machine (#1390).
const THEIRS: &str = "33333333-3333-4333-8333-333333333333";

/// A spawn and a test letting go of a tasks directory it takes again take
/// turns (#1569 row 27). A child spawned by one test thread holds a copy of
/// every descriptor, the directory's lock among them, until its exec closes
/// the close-on-exec ones, and `spawn` returns only after that; a service
/// started again on the same directory in that moment finds it `already
/// owned`. So a spawn takes the turn alone ([`spawned`]), and letting go of
/// a store shares it ([`Running::stop`], [`released`]): no child spawned
/// before can still hold what was let go. On this Mac, beside a test
/// spawning `lamp-tasks` in a loop, 4 of 10 runs of this file found a
/// folder already owned, and none of 10 with the turns. nextest, a process
/// for each test, is not affected.
static SPAWNS: RwLock<()> = RwLock::new(());

fn share() -> RwLockReadGuard<'static, ()> {
    SPAWNS.read().unwrap_or_else(PoisonError::into_inner)
}

/// `command` spawned alone in the turn ([`SPAWNS`]).
fn spawned(command: &mut Command) -> Child {
    let _turn = SPAWNS.write().unwrap_or_else(PoisonError::into_inner);
    command.spawn().unwrap()
}

/// `command`'s output, as `Command::output` gives it, spawned in the turn.
fn output_of(command: &mut Command) -> Output {
    spawned(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
    .wait_with_output()
    .unwrap()
}

#[derive(Clone)]
struct Injected(Arc<Mutex<Sample>>);
impl Clock for Injected {
    fn sample(&self) -> io::Result<Sample> {
        Ok(self.0.lock().unwrap().clone())
    }
}
#[derive(Default)]
struct Counts {
    launches: AtomicUsize,
    reads: AtomicUsize,
    listings: AtomicUsize,
    /// What `ps` lists next: the agent's progress line.
    about: Mutex<Option<String>>,
    /// `ps` fails while set.
    ps_fails: AtomicBool,
    /// `show` finds the agent idle with an answer while set.
    idle: AtomicBool,
    /// Each `wait` held, with the agents it named.
    waits: Mutex<Vec<Vec<String>>>,
    /// What the held `wait` comes back with, once set.
    wait_ends: Mutex<Option<Vec<Ended>>>,
    /// `show` finds the agent idle with this answer while set, as Harness
    /// gives it: its `ask` is the task's own text.
    plain: Mutex<Option<String>>,
    /// `ps` also lists the person's own harness, idle, while set.
    theirs: AtomicBool,
    /// Each `close`, with the machine and agent it named (#1390).
    closes: Mutex<Vec<(String, String)>>,
    /// Harness is signed out while set (#1676), and how often whether it is
    /// signed in was asked.
    signed_out: AtomicBool,
    sign_in_reads: AtomicUsize,
    /// What `harness machines` lists, and how often it was asked (#1676).
    machines: Mutex<Vec<AccountMachine>>,
    machine_reads: AtomicUsize,
    /// Harness's sign-in lines still to come, the answers written and how
    /// many sign-ins were started (#1676).
    sign_in_lines: Arc<Mutex<std::collections::VecDeque<SignInRead>>>,
    sign_in_answers: Arc<Mutex<Vec<bool>>>,
    sign_in_starts: AtomicUsize,
    /// How many times Harness's daemon was started (openharness#1463).
    daemon_starts: AtomicUsize,
    /// How many daemon starts still fail, `start_failed` (lamp-75c9, 21 Oct).
    daemon_fails: AtomicUsize,
    /// Each launch and daemon start, in the order they ran, and a launch
    /// held on the job lane while set (the reviewer-lead's P2 on #2309).
    order: Mutex<Vec<&'static str>>,
    hold_launch: AtomicBool,
}
type SignInRead = Result<lamp_tasks::sign_in::Event, &'static str>;
/// A sign-in that reads the script's lines and keeps its answers.
struct ScriptSignIn(
    Arc<Mutex<std::collections::VecDeque<SignInRead>>>,
    Arc<Mutex<Vec<bool>>>,
);
impl lamp_tasks::sign_in::Link for ScriptSignIn {
    fn poll(&mut self) -> Option<SignInRead> {
        self.0.lock().unwrap().pop_front()
    }
    fn answer(&mut self, yes: bool) -> io::Result<()> {
        self.1.lock().unwrap().push(yes);
        Ok(())
    }
}
struct ScriptLane(Arc<Counts>);
impl WaitLane for ScriptLane {
    fn wait(
        &self,
        machine: &str,
        agents: &[String],
        cancel: &AtomicBool,
    ) -> Result<Vec<Ended>, String> {
        assert_eq!(machine, "0123456789abcdef0123456789abcdef");
        self.0.waits.lock().unwrap().push(agents.to_vec());
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("cancelled".into());
            }
            if let Some(ends) = self.0.wait_ends.lock().unwrap().take() {
                return Ok(ends);
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}
struct Script {
    counts: Arc<Counts>,
    stop: Arc<AtomicBool>,
    slow: bool,
}
impl Adapter for Script {
    fn wait_lane(&self) -> Option<Arc<dyn WaitLane>> {
        Some(Arc::new(ScriptLane(self.counts.clone())))
    }
    fn launch(&self, _: &Launch) -> LaunchOutcome {
        self.counts.launches.fetch_add(1, Ordering::SeqCst);
        self.counts.order.lock().unwrap().push("launch");
        while self.counts.hold_launch.load(Ordering::SeqCst) && !self.stop.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(5));
        }
        if self.slow {
            while !self.stop.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(5));
            }
            return LaunchOutcome::Uncertain {
                code: "cancelled".into(),
            };
        }
        LaunchOutcome::Created {
            agent_id: AGENT.into(),
        }
    }
    fn observe(&self, machine: &str, id: &str) -> Result<Observation, String> {
        self.counts.reads.fetch_add(1, Ordering::SeqCst);
        // Read on the machine the task was started on.
        assert_eq!(machine, "0123456789abcdef0123456789abcdef");
        assert_eq!(id, AGENT);
        if let Some(answer) = self.counts.plain.lock().unwrap().clone() {
            return Ok(Observation {
                agent_id: AGENT.into(),
                state: "idle".into(),
                ask: Some("Private scripted task".into()),
                answer,
                truncated: false,
                tools: 1,
            });
        }
        if self.counts.idle.load(Ordering::SeqCst) {
            return Ok(Observation {
                agent_id: AGENT.into(),
                state: "idle".into(),
                ask: Some("Private scripted task".into()),
                answer: "Lumi, Sunny and Archie.".into(),
                truncated: false,
                tools: 1,
            });
        }
        Ok(Observation {
            agent_id: AGENT.into(),
            state: "needs-input".into(),
            ask: Some("An earlier question".into()),
            answer: "Private scripted answer".into(),
            truncated: false,
            tools: 2,
        })
    }
    fn close(&self, machine: &str, id: &str) -> Result<Closed, String> {
        self.counts
            .closes
            .lock()
            .unwrap()
            .push((machine.into(), id.into()));
        Ok(Closed::Closed)
    }
    fn machines(&self) -> Result<Vec<AccountMachine>, String> {
        self.counts.machine_reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.counts.machines.lock().unwrap().clone())
    }
    fn start_daemon(&self) -> Result<(), String> {
        self.counts.daemon_starts.fetch_add(1, Ordering::SeqCst);
        self.counts.order.lock().unwrap().push("daemon");
        let fails = &self.counts.daemon_fails;
        if fails
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err("start_failed".into());
        }
        Ok(())
    }
    fn sign_in_starter(&self) -> Option<lamp_tasks::sign_in::Starter> {
        let counts = self.counts.clone();
        Some(Box::new(move || {
            counts.sign_in_starts.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(ScriptSignIn(
                counts.sign_in_lines.clone(),
                counts.sign_in_answers.clone(),
            )))
        }))
    }
    fn signed_in(&self) -> Result<SignIn, String> {
        self.counts.sign_in_reads.fetch_add(1, Ordering::SeqCst);
        Ok(if self.counts.signed_out.load(Ordering::SeqCst) {
            SignIn::Out
        } else {
            SignIn::In
        })
    }
    fn status(&self, machine: &str) -> Result<Vec<Listed>, String> {
        self.counts.listings.fetch_add(1, Ordering::SeqCst);
        assert_eq!(machine, "0123456789abcdef0123456789abcdef");
        if self.counts.ps_fails.load(Ordering::SeqCst) {
            return Err("timeout".into());
        }
        let mut listed = vec![Listed {
            agent_id: AGENT.into(),
            state: "working".into(),
            last_active_unix_ms: Some(1_792_022_712_345),
            about: self.counts.about.lock().unwrap().clone(),
        }];
        if self.counts.theirs.load(Ordering::SeqCst) {
            listed.push(Listed {
                agent_id: THEIRS.into(),
                state: "idle".into(),
                last_active_unix_ms: Some(1_792_022_712_345),
                about: None,
            });
        }
        Ok(listed)
    }
}

struct Running {
    root: tempfile::TempDir,
    config: Config,
    clock: Injected,
    stop: Arc<AtomicBool>,
    counts: Arc<Counts>,
    handle: Option<JoinHandle<io::Result<()>>>,
    slow: bool,
}
impl Running {
    fn new(slow: bool) -> Self {
        Self::with(slow, false)
    }
    fn with(slow: bool, progress: bool) -> Self {
        Self::configured(slow, progress, false)
    }
    fn configured(slow: bool, progress: bool, watch: bool) -> Self {
        Self::all(slow, progress, watch, false)
    }
    /// With `serve --close-told` on or off, and progress on, so `ps` runs.
    fn closing(close_told: bool) -> Self {
        Self::all(false, true, false, close_told)
    }
    fn all(slow: bool, progress: bool, watch: bool, close_told: bool) -> Self {
        let mut running = Self::unstarted(slow, progress, watch, close_told);
        running.start();
        running
    }
    /// As [`Running::all`], not started: for a test that changes its config
    /// first, with no service started and stopped for nothing (#1806).
    fn unstarted(slow: bool, progress: bool, watch: bool, close_told: bool) -> Self {
        let root = tempfile::Builder::new()
            .prefix("tsk-")
            .tempdir_in("/private/tmp")
            .or_else(|_| tempfile::Builder::new().prefix("tsk-").tempdir())
            .unwrap();
        let config = Config {
            enabled: true,
            dir: root.path().join("data"),
            socket: root.path().join("run/public"),
            voice_socket: root.path().join("run/voice"),
            bus_root: root.path().join("b"),
            targets: BTreeMap::from([(
                "coding".into(),
                Target {
                    machine_id: "0123456789abcdef0123456789abcdef".into(),
                    project: "project".into(),
                    engine: EngineKind::Codex,
                },
            )]),
            run_for: None,
            watch,
            progress,
            push_wait: false,
            close_told,
            targets_newer: None,
            set_up_later: None,
            // Its report every 50 ms, not each second: a test reading it
            // waits a fraction of a second (#1128).
            sign_in_by_code: false,
            report_every: Duration::from_millis(50),
            // A failed daemon start tried again in 50, 100 and 200 ms.
            daemon_retry_after: Duration::from_millis(50),
        };
        Self {
            root,
            config,
            clock: Injected(Arc::new(Mutex::new(Sample {
                boot_id: "test-boot".into(),
                monotonic_ms: 0,
            }))),
            stop: Arc::new(AtomicBool::new(false)),
            counts: Arc::new(Counts::default()),
            handle: None,
            slow,
        }
    }
    fn start(&mut self) {
        self.stop.store(false, Ordering::Release);
        let config = self.config.clone();
        let clock = self.clock.clone();
        let stop = self.stop.clone();
        let script = Script {
            counts: self.counts.clone(),
            stop: stop.clone(),
            slow: self.slow,
        };
        self.handle = Some(thread::spawn(move || {
            serve_with_adapter(config, clock, stop, move |_| Ok(script))
        }));
        let start = Instant::now();
        loop {
            if request(
                &self.config.socket,
                &json!({"op":"status"}),
                Duration::from_secs(2),
            )
            .is_ok_and(|s| s["ok"] == true)
            {
                break;
            }
            if self.handle.as_ref().unwrap().is_finished() {
                panic!(
                    "service failed: {:?}",
                    self.handle.take().unwrap().join().unwrap()
                );
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn stop(&mut self) {
        // The service lets go of its store in the turn's share.
        let _share = share();
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.join().unwrap().unwrap();
        }
    }
    fn ask(&self, value: Value) -> Value {
        request(&self.config.socket, &value, Duration::from_secs(2)).unwrap()
    }
    fn trusted(&self, value: Value) -> Value {
        request(&self.config.voice_socket, &value, Duration::from_secs(2)).unwrap()
    }
    fn status(&self) -> Value {
        self.ask(json!({"op":"status"}))
    }
    fn guard(&self) -> Value {
        let s = self.status();
        json!({"store_id":s["store_id"],"revision":s["revision"],"request_id":format!("request-{}",s["revision"])})
    }
    fn prepare(&self) -> Value {
        let r=self.ask(json!({"op":"prepare","guard":self.guard(),"text":"Private scripted task","target":"coding"}));
        assert_eq!(r["ok"], true, "{r}");
        r
    }
    fn confirmation(&self, prepared: &Value) -> Value {
        json!({"op":"confirm","guard":self.guard(),"id":prepared["id"],"prepared_digest":prepared["prepared_digest"],"session_id":self.status()["summary"]["session_id"]})
    }
    fn wait(&self, id: &Value, predicate: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let value = self.ask(json!({"op":"get_task","id":id}));
            if predicate(&value) {
                return value;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "task did not reach expected state: {value}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

#[test]
fn model_cannot_confirm_and_restart_keeps_exact_dispatch_without_resending() {
    let mut s = Running::new(false);
    let prepared = s.prepare();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(s.counts.launches.load(Ordering::SeqCst), 0);
    let confirm = s.confirmation(&prepared);
    assert_eq!(s.ask(confirm.clone())["error"], "untrusted");
    assert_eq!(s.trusted(confirm.clone())["ok"], true);
    let accepted = s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    assert_eq!(accepted["text"], "Private scripted task");
    assert_eq!(
        accepted["target"]["machine_id"],
        "0123456789abcdef0123456789abcdef"
    );
    assert_eq!(accepted["task"]["completion"], "unverified");
    s.stop();
    assert!(!s.config.socket.exists());
    assert!(!s.config.voice_socket.exists());
    s.start();
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    assert_eq!(s.trusted(confirm)["error"], "stale_session");
    assert_eq!(s.counts.launches.load(Ordering::SeqCst), 1);
}

#[test]
fn who_can_be_given_work_is_read_on_the_request_socket_and_runs_nothing() {
    let s = Running::new(false);
    let told = s.ask(json!({"op":"targets"}));
    assert_eq!(told["ok"], true, "{told}");
    assert_eq!(
        told["targets"],
        json!([{"alias":"coding","engine":"codex"}])
    );
    assert_eq!(s.status()["revision"], told["revision"]);
    assert_eq!(s.counts.launches.load(Ordering::SeqCst), 0);
}

#[test]
fn observed_output_and_reported_playout_remain_separate_and_survive_restart() {
    let mut s = Running::new(false);
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    assert_eq!(
        s.ask(json!({"op":"observe","guard":s.guard(),"id":prepared["id"]}))["ok"],
        true
    );
    let task = s.wait(&prepared["id"], |v| {
        v["observations"].as_array().is_some_and(|v| !v.is_empty())
    });
    let observation = task["observations"][0]["id"].clone();
    let observed =
        s.ask(json!({"op":"get_observation","id":prepared["id"],"observation_id":observation}));
    assert_eq!(observed["data"]["answer"], "Private scripted answer");
    assert_eq!(observed["data"]["state"], "needs-input");
    assert_eq!(observed["completion"], "unverified");
    assert!(observed["speech"].is_null());
    assert_eq!(s.status()["summary"]["speech_spoken"], 0);
    let session = s.status()["summary"]["session_id"].clone();
    let claim = json!({"op":"claim_speech","guard":s.guard(),"id":prepared["id"],"observation_id":observation,"text":"The agent needs attention in Harness.","session_id":session});
    assert_eq!(s.ask(claim.clone())["error"], "untrusted");
    let granted = s.trusted(claim);
    assert_eq!(granted["ok"], true, "{granted}");
    assert_eq!(s.status()["summary"]["speech_spoken"], 0);
    let ack = json!({"op":"ack_speech","guard":s.guard(),"id":prepared["id"],"observation_id":observation,"claim_id":granted["claim_id"],"text_digest":granted["text_digest"],"session_id":session,"played":true});
    assert_eq!(s.trusted(ack)["reported_spoken"], true);
    s.stop();
    s.start();
    let observed =
        s.ask(json!({"op":"get_observation","id":prepared["id"],"observation_id":observation}));
    assert_eq!(observed["speech"]["state"], "spoken");
    assert_eq!(
        observed["speech"]["text"],
        "The agent needs attention in Harness."
    );
    assert_eq!(s.counts.launches.load(Ordering::SeqCst), 1);
    assert_eq!(s.counts.reads.load(Ordering::SeqCst), 1);
}

/// Read, then told: the observation's claim and its played acknowledgement.
fn read_and_tell(s: &Running, id: &Value) {
    let observation = read(s, id);
    tell(s, id, observation);
}

/// Read: the kept observation's id.
fn read(s: &Running, id: &Value) -> Value {
    assert_eq!(
        s.ask(json!({"op":"observe","guard":s.guard(),"id":id}))["ok"],
        true
    );
    let task = s.wait(id, |v| {
        v["observations"].as_array().is_some_and(|v| !v.is_empty())
    });
    task["observations"][0]["id"].clone()
}

/// Told: the observation's claim and its played acknowledgement.
fn tell(s: &Running, id: &Value, observation: Value) {
    let session = s.status()["summary"]["session_id"].clone();
    let granted = s.trusted(json!({"op":"claim_speech","guard":s.guard(),"id":id,"observation_id":observation,"text":"The agent finished.","session_id":session}));
    assert_eq!(granted["ok"], true, "{granted}");
    let ack = json!({"op":"ack_speech","guard":s.guard(),"id":id,"observation_id":observation,"claim_id":granted["claim_id"],"text_digest":granted["text_digest"],"session_id":session,"played":true});
    assert_eq!(s.trusted(ack)["reported_spoken"], true);
}

fn closes(s: &Running) -> Vec<(String, String)> {
    s.counts.closes.lock().unwrap().clone()
}

/// Prepared, confirmed and accepted: its id. Nothing is closed yet.
fn handed(s: &Running) -> Value {
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    thread::sleep(Duration::from_millis(100));
    assert!(closes(s).is_empty(), "closed before it was told");
    prepared["id"].clone()
}

/// Handed over, then read and told.
fn handed_and_told(s: &Running) -> Value {
    let id = handed(s);
    read_and_tell(s, &id);
    id
}

fn closed_within(s: &Running, ms: u64) -> Vec<(String, String)> {
    let begun = Instant::now();
    while closes(s).is_empty() && begun.elapsed() < Duration::from_millis(ms) {
        thread::sleep(Duration::from_millis(5));
    }
    closes(s)
}

/// #1390, through the service with `--close-told`: a harness done and told,
/// and still idle on that turn when read again, is closed once, on its
/// machine, by the agent this lamp started, and never again. The person's
/// own idle harness that `ps` lists beside it is never closed.
#[test]
fn a_harness_done_and_told_is_closed_once_by_the_service() {
    let s = Running::closing(true);
    s.counts.theirs.store(true, Ordering::SeqCst);
    *s.counts.plain.lock().unwrap() = Some("A poem about coffee.".into());
    handed_and_told(&s);
    assert!(!closed_within(&s, 5_000).is_empty(), "never closed");
    s.clock.0.lock().unwrap().monotonic_ms = 10_000_000;
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        closes(&s),
        [(
            "0123456789abcdef0123456789abcdef".to_string(),
            AGENT.to_string()
        )]
    );
    assert!(s.counts.listings.load(Ordering::SeqCst) > 0, "ps was read");
}

/// Without the switch, nothing is closed.
#[test]
fn without_close_told_no_harness_is_closed() {
    let s = Running::closing(false);
    *s.counts.plain.lock().unwrap() = Some("A poem about coffee.".into());
    handed_and_told(&s);
    assert!(closed_within(&s, 500).is_empty());
}

/// The person went on at the computer after the telling: the read just
/// before the close finds another turn, so it is left to them for good. One
/// that waits for the person (`needs-input`) is never closed either.
#[test]
fn a_harness_the_person_went_on_with_or_that_waits_for_them_is_left() {
    let s = Running::closing(true);
    *s.counts.plain.lock().unwrap() = Some("A poem about coffee.".into());
    let id = handed(&s);
    let observation = read(&s, &id);
    // The person types a follow-up at the computer; then the lamp tells.
    *s.counts.plain.lock().unwrap() = Some("Their own next answer.".into());
    tell(&s, &id, observation);
    s.wait(&id, |v| v["task"]["close"]["state"] == "left");
    s.clock.0.lock().unwrap().monotonic_ms = 10_000_000;
    assert!(closed_within(&s, 500).is_empty());

    let s = Running::closing(true);
    handed_and_told(&s);
    assert!(closed_within(&s, 500).is_empty());
}

#[test]
fn a_slow_cli_does_not_block_status_diagnostics_or_bounded_shutdown() {
    let mut s = Running::new(true);
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "attempting"
    });
    let start = Instant::now();
    let status = s.status();
    assert_eq!(status["summary"]["attempting"], 1);
    assert!(start.elapsed() < Duration::from_secs(1));
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let mut diagnostic = bus
        .subscribe_typed(Diagnostic::TOPIC, "tasks_test", Diagnostic::TYPE)
        .unwrap();
    let start = Instant::now();
    let mut found = false;
    while start.elapsed() < Duration::from_secs(3) {
        if let Some(message) = diagnostic.recv(Duration::from_millis(50)).unwrap() {
            let value: Value = message.parse().unwrap();
            let text = value.to_string();
            assert!(!text.contains("Private scripted"));
            if text.contains("cli_busy") && text.contains("true") {
                found = true;
                break;
            }
        }
    }
    assert!(found, "diagnostics stopped during CLI work");
    let stop = Instant::now();
    s.stop();
    assert!(stop.elapsed() < Duration::from_secs(2));
    s.slow = false;
    s.start();
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "uncertain"
    });
    assert_eq!(s.counts.launches.load(Ordering::SeqCst), 1);
}

#[test]
fn a_silent_client_uses_only_one_bounded_worker() {
    let s = Running::new(false);
    let _silent = UnixStream::connect(&s.config.socket).unwrap();
    let began = Instant::now();
    assert_eq!(s.status()["ok"], true);
    assert!(began.elapsed() < Duration::from_secs(1));
    for path in [&s.config.socket, &s.config.voice_socket] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(s.root.path().join("data/tasks.sqlite3").is_file());
}

#[test]
fn expired_and_restarted_preparations_never_reach_harness() {
    let mut s = Running::new(false);
    let old = s.prepare();
    let confirm = s.confirmation(&old);
    s.clock.0.lock().unwrap().monotonic_ms = 120_000;
    assert_eq!(s.trusted(confirm)["ok"], false);
    let recent = s.prepare();
    let confirm = s.confirmation(&recent);
    s.stop();
    s.start();
    assert_eq!(s.trusted(confirm)["error"], "stale_session");
    assert_eq!(s.counts.launches.load(Ordering::SeqCst), 0);
}

#[test]
fn command_line_is_off_without_loading_any_provisioned_paths() {
    let output = output_of(Command::new(env!("CARGO_BIN_EXE_lamp-tasks")).args([
        "serve",
        "--targets",
        "/does-not-exist",
        "--harness",
        "/does-not-exist",
    ]));
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["enabled"], false);
    assert!(
        !output_of(
            Command::new(env!("CARGO_BIN_EXE_lamp-tasks")).args(["serve", "--enable", "--enable"])
        )
        .status
        .success()
    );
}

#[test]
fn command_line_rejects_duplicate_fields_before_connecting() {
    for input in DUPLICATE_FIELDS {
        refuse_duplicate_fields(input);
    }
}

/// #876: the refusal is written just before the process exits, and once
/// went missing on ARM64. It reaches stderr every time, not most times.
#[test]
fn command_line_refusal_reaches_stderr_every_time() {
    for _ in 0..100 {
        for input in DUPLICATE_FIELDS {
            refuse_duplicate_fields(input);
        }
    }
}

/// #876: the writer of the refusal runs late, and the process still waits
/// for its line rather than for a fixed few milliseconds.
#[test]
fn command_line_refusal_waits_for_a_slow_writer() {
    for input in DUPLICATE_FIELDS {
        refuse_duplicate_fields_with(input, Some("80"));
    }
}

const DUPLICATE_FIELDS: [&str; 2] = [
    r#"{"op":"prepare","op":"status"}"#,
    r#"{"op":"cancel","guard":{"store_id":"a","revision":0,"revision":1,"request_id":"r"},"id":1}"#,
];

fn refuse_duplicate_fields(input: &str) {
    refuse_duplicate_fields_with(input, None);
}

fn refuse_duplicate_fields_with(input: &str, slow_writer_ms: Option<&str>) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-tasks"));
    if let Some(ms) = slow_writer_ms {
        command.env("LAMP_TASKS_TEST_SLOW_EXIT_REPORT_MS", ms);
    }
    let mut child = spawned(
        command
            .args(["request", "--socket", "/no-tasks-test-socket"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let seen = format!(
        "input {input}: {}, stdout {:?}, stderr {stderr:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!output.status.success(), "{seen}");
    assert!(output.stdout.is_empty(), "{seen}");
    assert!(stderr.contains("bad_request"), "{seen}");
}

/// The next status on the bus, checked. Received in slices of 100 ms within
/// the same 5 s: a publisher looks for new subscribers at most every 200 ms
/// (`rescan_every`), so a status published just after this subscribed
/// reaches it only as the topic's kept value, which `recv` reads once its
/// wait on the socket is over. One 5 s wait then took the whole 5 s.
fn next_status(watch: &mut lamp_bus::Subscriber) -> TaskStatus {
    let until = Instant::now() + Duration::from_secs(5);
    let message = loop {
        if let Some(message) = watch.recv(Duration::from_millis(100)).unwrap() {
            break message;
        }
        assert!(Instant::now() < until, "a status on the bus");
    };
    assert_eq!(message.header.source, "tasks");
    let text = String::from_utf8(message.payload.clone()).unwrap();
    // Typed state only: no word an agent wrote, no task text.
    for words in [
        "Reading the brief",
        "tests pass",
        "Fixed",
        "Private scripted",
    ] {
        assert!(!text.contains(words), "{text}");
    }
    let status: TaskStatus = message.parse().unwrap();
    status.check().unwrap();
    status
}

#[test]
fn with_progress_a_value_from_the_start_each_task_read_at_once_then_on_schedule_and_the_words_stay_here()
 {
    let mut s = Running::with(false, true);
    let bus = Bus::new(&s.config.bus_root).unwrap();
    // A value from the start, before anything is handed over: no task, so
    // "nothing being worked on" is told apart from "not there".
    let begun = Instant::now();
    let start = loop {
        if let Some(message) = bus.latest(TaskStatus::TOPIC).unwrap() {
            break message.parse::<TaskStatus>().unwrap();
        }
        assert!(
            begun.elapsed() < Duration::from_secs(5),
            "no value at start"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert!(start.tasks.is_empty());
    assert_eq!(start.store_id, s.status()["store_id"]);
    assert_eq!(s.counts.listings.load(Ordering::SeqCst), 0);
    let mut watch = bus
        .subscribe_typed(TaskStatus::TOPIC, "tasks_test", TaskStatus::TYPE)
        .unwrap();
    *s.counts.about.lock().unwrap() = Some("Reading the brief".into());
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    // Read at once, with the clock where it was handed over. The task may
    // first be on the bus as unread, joining the watch before its first read
    // came back (#793 review, 17:48); then its first read.
    let id = prepared["id"].as_u64().unwrap();
    let first = loop {
        let status = next_status(&mut watch);
        if status.tasks.is_empty() {
            assert_eq!(status.unread, [id], "{status:?}");
            continue;
        }
        break status;
    };
    assert!(first.unread.is_empty());
    assert_eq!(s.counts.listings.load(Ordering::SeqCst), 1);
    let task = &first.tasks[0];
    assert_eq!(first.store_id, start.store_id);
    assert_eq!(
        (
            task.task,
            task.engine.as_str(),
            task.state,
            task.changes,
            task.changed
        ),
        (
            prepared["id"].as_u64().unwrap(),
            "codex",
            AgentState::Working,
            0,
            false
        )
    );
    assert_eq!(task.working_s, Some(0));
    assert_eq!(task.last_active_unix_ms, Some(1_792_022_712_345));
    // The line changes; the next read is half a minute later.
    *s.counts.about.lock().unwrap() = Some("Fixed it; all tests pass".into());
    s.clock.0.lock().unwrap().monotonic_ms = 29_999;
    thread::sleep(Duration::from_millis(100));
    assert_eq!(s.counts.listings.load(Ordering::SeqCst), 1);
    s.clock.0.lock().unwrap().monotonic_ms = 30_000;
    let changed = next_status(&mut watch);
    let task = &changed.tasks[0];
    assert_eq!((task.changes, task.changed), (1, true));
    assert_eq!(task.signs, vec![Sign::TestsPass, Sign::Fixed]);
    assert_eq!(task.new_share, 1.0);
    // The words are handed over on the socket, for the lamp about to say them.
    let words = s.ask(json!({"op":"get_progress","id":prepared["id"]}));
    assert_eq!(words["ok"], true, "{words}");
    assert_eq!(words["progress"]["about"], "Fixed it; all tests pass");
    assert_eq!(words["progress"]["changes"], 1);
    assert_eq!(words["progress"]["run"], changed.run);
    assert_eq!(words["store_id"], changed.store_id);
    assert_eq!(
        s.ask(json!({"op":"get_progress","id":99}))["error"],
        "not_found"
    );
    // The reads are counted in the diagnostics.
    let mut diagnostic = bus
        .subscribe_typed(Diagnostic::TOPIC, "tasks_test", Diagnostic::TYPE)
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(message) = diagnostic.recv(Duration::from_millis(50)).unwrap() {
            let value: Value = message.parse().unwrap();
            if value.to_string().contains("status_reads") {
                assert!(!value.to_string().contains("tests pass"));
                break;
            }
        }
        assert!(start.elapsed() < Duration::from_secs(3), "no status_reads");
    }
    // A restart keeps the store and the task's number and counts again
    // from nothing, as another run: a count names a change only with it.
    assert_eq!(first.run, changed.run);
    s.stop();
    *s.counts.about.lock().unwrap() = Some("Another line altogether".into());
    s.start();
    s.clock.0.lock().unwrap().monotonic_ms = 120_000;
    // The restarted run's value at start has no task; its read at once does.
    let again = loop {
        let status = next_status(&mut watch);
        if !status.tasks.is_empty() {
            break status;
        }
    };
    assert_eq!(again.store_id, changed.store_id);
    assert_eq!(
        (again.tasks[0].task, again.tasks[0].changes),
        (prepared["id"].as_u64().unwrap(), 0)
    );
    assert!(
        again.run > changed.run,
        "{} after {}",
        again.run,
        changed.run
    );
    let words = s.ask(json!({"op":"get_progress","id":prepared["id"]}));
    assert_eq!(words["progress"]["run"], again.run);
    // Settled (the agent asks the person): it leaves the watch, and the
    // bus, at once rather than at the next read, which with the clock held
    // at 120 s is not due. A read already under way may come first.
    assert_eq!(
        s.ask(json!({"op":"observe","guard":s.guard(),"id":prepared["id"]}))["ok"],
        true
    );
    let mut seen = 0;
    let left = loop {
        let status = next_status(&mut watch);
        seen += 1;
        if status.tasks.is_empty() || seen == 3 {
            break status;
        }
    };
    assert!(left.tasks.is_empty(), "{left:?}");
}

/// Fourth on #1048, through the service with the watch on: a task the
/// watch finds settled goes from `tasks` to `settled` in one value, is
/// there in the first value after a restart inside the two minutes, and
/// leaves at their end with nothing else happening. It is never in two
/// lists, and never in none until it leaves.
#[test]
fn a_task_the_watch_finds_settled_moves_in_one_value_survives_a_restart_and_leaves_on_time() {
    let mut s = Running::configured(false, true, true);
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let mut watch = bus
        .subscribe_typed(TaskStatus::TOPIC, "tasks_test", TaskStatus::TYPE)
        .unwrap();
    // The service's publisher looks for new subscribers at most every
    // 200 ms (`lamp_bus`): a value sent before it has looked reaches only
    // the kept file, which `recv` reads at the end of its 5 s, so every
    // value is seen only once it has.
    thread::sleep(Duration::from_millis(250));
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    let id = prepared["id"].as_u64().unwrap();
    // Where the task is: worked on, not read yet, settled.
    let places = |status: &TaskStatus| {
        [
            status.tasks.iter().any(|task| task.task == id),
            status.unread.contains(&id),
            status.settled.iter().any(|settled| settled.task == id),
        ]
    };
    // `ps` reads it at once: worked on.
    let read = loop {
        let status = next_status(&mut watch);
        if places(&status) == [true, false, false] {
            break status;
        }
        assert_ne!(places(&status), [false, false, true], "{status:?}");
    };
    // The watch's first look, half a minute on, finds it asking the
    // person: the next value that differs has it settled, and only there.
    s.clock.0.lock().unwrap().monotonic_ms = WATCH_EARLY_MS;
    let moved = loop {
        let status = next_status(&mut watch);
        if places(&status) != [true, false, false] {
            break status;
        }
    };
    assert_eq!(places(&moved), [false, false, true], "{moved:?}");
    let settled = &moved.settled[0];
    assert_eq!(
        (settled.engine.as_str(), settled.state, settled.settled),
        ("codex", AgentState::NeedsInput, true)
    );
    assert_eq!(s.counts.reads.load(Ordering::SeqCst), 1);
    // Asked for its progress, it says what it ended in, never `not_found`
    // (Ten and Nine on #705, problem 3 (b)).
    let asked = s.ask(json!({"op":"get_progress","id":id}));
    assert_eq!(asked["ok"], true, "{asked}");
    assert_eq!(
        (&asked["settled"]["task"], &asked["settled"]["state"]),
        (&json!(id), &json!("needs_input")),
        "{asked}"
    );
    assert!(asked.get("progress").is_none(), "{asked}");
    // Restarted inside the two minutes: the new run's first value has it.
    s.stop();
    s.start();
    let again = loop {
        let status = next_status(&mut watch);
        if status.run > read.run {
            break status;
        }
    };
    assert_eq!(places(&again), [false, false, true], "{again:?}");
    assert_eq!(again.settled[0].state, AgentState::NeedsInput);
    // Same clock, so the same moment, give or take the restart.
    assert!(
        again.settled[0].at_unix_ms.abs_diff(settled.at_unix_ms) < 5_000,
        "{} then {}",
        settled.at_unix_ms,
        again.settled[0].at_unix_ms
    );
    // Nothing more is read or published until the two minutes are up; then
    // it leaves, with no request. The watch looks once more, as it follows a
    // question for the person's answer (lamp-75c9, 21 Oct): one read, which
    // finds the same question and lists nothing again.
    let (reads, listings) = (
        s.counts.reads.load(Ordering::SeqCst),
        s.counts.listings.load(Ordering::SeqCst),
    );
    s.clock.0.lock().unwrap().monotonic_ms = WATCH_EARLY_MS + SETTLED_FOR_MS - 1;
    assert!(
        watch.recv(Duration::from_millis(300)).unwrap().is_none(),
        "a value before the two minutes were up"
    );
    s.clock.0.lock().unwrap().monotonic_ms = WATCH_EARLY_MS + SETTLED_FOR_MS;
    let gone = next_status(&mut watch);
    assert_eq!(places(&gone), [false, false, false], "{gone:?}");
    assert_eq!(
        s.ask(json!({"op":"get_progress","id":id}))["error"],
        "not_found"
    );
    assert_eq!(
        (
            s.counts.reads.load(Ordering::SeqCst),
            s.counts.listings.load(Ordering::SeqCst)
        ),
        (reads + 1, listings)
    );
}

/// The reviewer on #793 (16:46): with every `ps` failing, a task handed
/// over, and again after a restart of the persisted store, was on the bus
/// as nothing, and `get_progress` said there was no such task. It is
/// `unread` on `tasks/status`, and `get_progress` says it has not been read.
#[test]
fn a_watched_task_whose_reads_fail_is_unread_not_gone_and_after_a_restart_too() {
    let mut s = Running::with(false, true);
    s.counts.ps_fails.store(true, Ordering::SeqCst);
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let mut watch = bus
        .subscribe_typed(TaskStatus::TOPIC, "tasks_test", TaskStatus::TYPE)
        .unwrap();
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    let id = prepared["id"].as_u64().unwrap();
    let unread = |watch: &mut lamp_bus::Subscriber| loop {
        let status = next_status(watch);
        if status.unread == [id] {
            break status;
        }
    };
    let first = unread(&mut watch);
    assert!(first.tasks.is_empty());
    let asked = s.ask(json!({"op":"get_progress","id":id}));
    assert_eq!(asked["error"], "not_read", "{asked}");
    // Restarted, the store kept, reads still failing: unread again.
    s.stop();
    s.start();
    let again = unread(&mut watch);
    assert!(again.tasks.is_empty() && again.run > first.run);
    // Reads come back: read, no longer unread.
    s.counts.ps_fails.store(false, Ordering::SeqCst);
    s.clock.0.lock().unwrap().monotonic_ms = 30_000;
    let read = loop {
        let status = next_status(&mut watch);
        if !status.tasks.is_empty() {
            break status;
        }
    };
    assert!(read.unread.is_empty());
    assert_eq!(read.tasks[0].task, id);
}

#[test]
fn without_progress_nothing_is_listed_the_topic_is_untouched_and_the_report_is_as_before() {
    let s = Running::new(false);
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    s.clock.0.lock().unwrap().monotonic_ms = 3 * 60_000;
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let mut diagnostic = bus
        .subscribe_typed(Diagnostic::TOPIC, "tasks_test", Diagnostic::TYPE)
        .unwrap();
    let message = loop {
        if let Some(message) = diagnostic.recv(Duration::from_secs(3)).unwrap() {
            break message;
        }
    };
    let report: Value = message.parse().unwrap();
    let keys: Vec<String> = report["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| pair["key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        keys,
        [
            "store_ready",
            "cli_busy",
            "requests",
            "publish_errors",
            "clients_refused",
            "completion_supported",
            "retained",
            "queued",
            "attempting",
            "accepted",
            "pending",
            "uncertain",
            "observations",
            "speech_spoken",
            "speech_uncertain"
        ]
    );
    assert_eq!(s.counts.listings.load(Ordering::SeqCst), 0);
    // The topic was never made: no publisher, no declaration, no value.
    assert!(!s.config.bus_root.join("tasks").exists());
    assert!(bus.latest(TaskStatus::TOPIC).unwrap().is_none());
    // Asked for anyway, the words are not there.
    assert_eq!(
        s.ask(json!({"op":"get_progress","id":prepared["id"]}))["error"],
        "progress_off"
    );
}

/// #560: with `--push-wait`, Harness's own `wait` is held on the agents the
/// watch looks at, and when it comes back the agent is read at once and an
/// idle answer kept without the watch's second look. The injected clock
/// never moves here, so the watch itself never looks: only the push reads.
#[test]
fn a_held_wait_that_comes_back_reads_the_agent_at_once_and_keeps_its_idle_answer() {
    let mut s = Running::unstarted(false, false, true, false);
    s.config.push_wait = true;
    s.start();
    // Nothing handed over: nothing waited on.
    thread::sleep(Duration::from_millis(50));
    assert!(s.counts.waits.lock().unwrap().is_empty());
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    // Accepted: one wait, on its agent, and no read yet.
    let start = Instant::now();
    while s.counts.waits.lock().unwrap().is_empty() {
        assert!(start.elapsed() < Duration::from_secs(5), "no wait held");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        *s.counts.waits.lock().unwrap(),
        vec![vec![AGENT.to_owned()]]
    );
    thread::sleep(Duration::from_millis(50));
    assert_eq!(s.counts.reads.load(Ordering::SeqCst), 0);
    // The agent ends: read at once, and its idle answer kept on that one
    // read, so the task is settled and the watch lets it go.
    s.counts.idle.store(true, Ordering::SeqCst);
    *s.counts.wait_ends.lock().unwrap() = Some(vec![Ended {
        agent_id: AGENT.into(),
        state: "idle".into(),
    }]);
    let kept = s.wait(&prepared["id"], |v| v["task"]["observations"] == json!(1));
    assert_eq!(s.counts.reads.load(Ordering::SeqCst), 1, "{kept}");
    // Settled: no longer watched, so no wait is held for it again.
    thread::sleep(Duration::from_millis(100));
    assert_eq!(s.counts.waits.lock().unwrap().len(), 1);
    assert_eq!(s.counts.reads.load(Ordering::SeqCst), 1);
}

/// Without `--push-wait` no wait is ever held, whatever the adapter has.
#[test]
fn without_push_wait_no_wait_is_held() {
    let s = Running::configured(false, false, true);
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    thread::sleep(Duration::from_millis(100));
    assert!(s.counts.waits.lock().unwrap().is_empty());
}

/// The newest `tasks` diagnostic.
fn tasks_diagnostic(s: &Running) -> Diagnostic {
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let mut diagnostic = bus
        .subscribe_typed(Diagnostic::TOPIC, "tasks_test", Diagnostic::TYPE)
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(message) = diagnostic.recv(Duration::from_secs(3)).unwrap() {
            let report: Diagnostic = message.parse().unwrap();
            if report.name == "tasks" {
                return report;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "no tasks diagnostic"
        );
    }
}

/// #1505: task targets written for a newer build (a `version` above this
/// build's, or a field it does not know) keep the part running with
/// hand-overs off. A task offered is refused with why and never reaches
/// Harness, and the part says so every second, where a stopped part would
/// say nothing.
#[test]
fn targets_for_a_newer_build_keep_the_part_running_with_hand_overs_off() {
    let mut s = Running::unstarted(false, false, false, false);
    s.config.targets = BTreeMap::new();
    s.config.targets_newer = Some(2);
    s.start();
    let refused = s.ask(
        json!({"op":"prepare","guard":s.guard(),"text":"Private scripted task","target":"coding"}),
    );
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"], "targets_newer", "{refused}");
    assert_eq!(s.status()["summary"]["retained"], 0);
    assert_eq!(s.counts.launches.load(Ordering::SeqCst), 0);
    let report = tasks_diagnostic(&s);
    assert_eq!(report.level, Level::Warn);
    assert_eq!(
        report.message,
        "The task targets were written for a newer build (version 2); hand-overs are off until they match this build"
    );
    assert!(
        report
            .values
            .iter()
            .any(|pair| pair.key == "targets_newer" && pair.value == "2")
    );
}

/// The first `tasks` diagnostic that `wanted` takes, within five seconds.
fn tasks_diagnostic_until(s: &Running, wanted: impl Fn(&Diagnostic) -> bool) -> Diagnostic {
    let start = Instant::now();
    loop {
        let report = tasks_diagnostic(s);
        if wanted(&report) {
            return report;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "no such tasks diagnostic: {report:?}"
        );
    }
}

fn said(report: &Diagnostic, key: &str) -> Option<String> {
    report
        .values
        .iter()
        .find(|pair| pair.key == key)
        .map(|pair| pair.value.clone())
}

fn write_targets(path: &std::path::Path, bytes: &[u8]) {
    // Written beside and renamed in, as a writer of the real file would.
    let next = path.with_extension("next");
    fs::write(&next, bytes).unwrap();
    fs::set_permissions(&next, fs::Permissions::from_mode(0o600)).unwrap();
    fs::rename(&next, path).unwrap();
}

fn told_targets(s: &Running) -> usize {
    s.ask(json!({"op":"targets"}))["targets"]
        .as_array()
        .unwrap()
        .len()
}

/// #1676: with `serve --set-up-later` the part runs with no targets file,
/// says there is no computer, and uses the file once it is written, with
/// no restart. Harness signed out is said; a file that is not valid keeps
/// the targets before it and is warned of; and a file that is gone leaves
/// no computer again. Not set up is not unwell: the level stays ok.
#[test]
fn set_up_later_reads_the_targets_again_and_says_why_hand_overs_are_off() {
    let mut s = Running::unstarted(false, false, false, false);
    let path = s.root.path().join("targets.json");
    s.config.targets = BTreeMap::new();
    s.config.set_up_later = Some(SetUpLater {
        targets: path.clone(),
        every: Duration::from_millis(50),
        from_machines: None,
    });
    s.start();
    let report = tasks_diagnostic_until(&s, |d| said(d, "handovers_off").is_some());
    assert_eq!(
        said(&report, "handovers_off").as_deref(),
        Some("no_computer")
    );
    assert_eq!(report.level, Level::Ok);
    assert_eq!(
        report.message,
        "No computer to hand tasks to yet: the task targets are missing or empty"
    );
    let refused = s.ask(
        json!({"op":"prepare","guard":s.guard(),"text":"Private scripted task","target":"coding"}),
    );
    assert_eq!(refused["error"], "target", "{refused}");

    let targets = json!({"version":1,"targets":{"coding":{"machine_id":"0123456789abcdef0123456789abcdef","project":"project","engine":"codex"}}});
    write_targets(&path, targets.to_string().as_bytes());
    let start = Instant::now();
    while told_targets(&s) != 1 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "targets not read again"
        );
        thread::sleep(Duration::from_millis(10));
    }
    s.prepare();
    let report = tasks_diagnostic_until(&s, |d| said(d, "handovers_off").is_none());
    assert_eq!(said(&report, "signed_in").as_deref(), Some("yes"));

    s.counts.signed_out.store(true, Ordering::SeqCst);
    let report = tasks_diagnostic_until(&s, |d| said(d, "handovers_off").is_some());
    assert_eq!(
        said(&report, "handovers_off").as_deref(),
        Some("not_signed_in")
    );
    assert_eq!(said(&report, "signed_in").as_deref(), Some("no"));
    s.counts.signed_out.store(false, Ordering::SeqCst);

    write_targets(&path, b"{\"version\":1,\"targets\":");
    // File refresh and sign-in observation finish independently. Wait for
    // both before judging the diagnostic; signed-out has higher priority.
    let report = tasks_diagnostic_until(&s, |d| {
        said(d, "targets_invalid").is_some() && said(d, "signed_in").as_deref() == Some("yes")
    });
    assert_eq!(report.level, Level::Warn);
    assert_eq!(
        report.message,
        "The task targets file changed and is not valid; the targets before it are kept"
    );
    assert_eq!(told_targets(&s), 1, "the targets before are kept");

    fs::remove_file(&path).unwrap();
    let report = tasks_diagnostic_until(&s, |d| {
        said(d, "handovers_off").as_deref() == Some("no_computer")
    });
    assert!(said(&report, "targets_invalid").is_none());
    assert_eq!(told_targets(&s), 0);
}

/// #1676: with `--targets-from-machines`, a lamp signed in to Harness writes
/// its targets from the person's machines and uses them with no restart: the
/// first is `office`, the voice's own default. A machine added is added; the
/// machines are not asked while signed out.
#[test]
fn targets_from_machines_are_written_and_used_while_signed_in() {
    let mut s = Running::unstarted(false, false, false, false);
    let path = s.root.path().join("targets.json");
    s.config.targets = BTreeMap::new();
    // A look every 20 ms, the machines' every 100 (#1806).
    s.config.set_up_later = Some(SetUpLater {
        targets: path.clone(),
        every: Duration::from_millis(20),
        from_machines: Some(EngineKind::Claude),
    });
    let machine = |id: char, label: &str, current: bool| AccountMachine {
        machine_id: id.to_string().repeat(32),
        label: label.into(),
        current,
    };
    *s.counts.machines.lock().unwrap() = vec![
        machine('a', "lamp", true),
        machine('b', "Office iMac", false),
    ];
    s.start();
    let until = |count: usize| {
        let start = Instant::now();
        while told_targets(&s) != count {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "not {count} targets"
            );
            thread::sleep(Duration::from_millis(10));
        }
    };
    until(1);
    // With its own name beside it (#1676).
    let start = Instant::now();
    while s.ask(json!({"op":"targets"}))["targets"]
        != json!([{"alias":"office","engine":"claude","name":"Office iMac"}])
    {
        assert!(start.elapsed() < Duration::from_secs(5), "no name told");
        thread::sleep(Duration::from_millis(10));
    }
    let report = tasks_diagnostic_until(&s, |d| said(d, "handovers_off").is_none());
    assert_eq!(said(&report, "signed_in").as_deref(), Some("yes"));

    s.counts.signed_out.store(true, Ordering::SeqCst);
    tasks_diagnostic_until(&s, |d| said(d, "signed_in").as_deref() == Some("no"));
    let asked = s.counts.machine_reads.load(Ordering::SeqCst);
    s.counts
        .machines
        .lock()
        .unwrap()
        .push(machine('c', "MacBook Air", false));
    // Keep the original 600 ms observation: sign-in calls are not the
    // machines timer, and a call can be counted before its result is used.
    let unchanged = || {
        assert_eq!(
            s.counts.machine_reads.load(Ordering::SeqCst),
            asked,
            "asked while signed out"
        );
    };
    let start = Instant::now();
    let report = tasks_diagnostic_until(&s, |_| {
        unchanged();
        start.elapsed() >= Duration::from_millis(600)
    });
    let requests: u64 = said(&report, "requests").unwrap().parse().unwrap();
    let answer = s.ask(json!({"op":"targets"}));
    assert_eq!(answer["ok"], true, "{answer}");
    // The owner reports this request only after scheduling jobs. Two more
    // sign-in reads after it mean whole passes ran past it while signed out,
    // each choosing the machines or not, rather than just that the request
    // was answered. Not an idle report: the sign-in is asked every 20 ms
    // here, so on a slow runner the worker is busy at nearly every report
    // (lamp-voice's ARM64 at 07:58 and #2936's at 15:44, 22 Oct).
    let reads = s.counts.sign_in_reads.load(Ordering::SeqCst);
    tasks_diagnostic_until(&s, |d| {
        unchanged();
        said(d, "requests")
            .and_then(|count| count.parse::<u64>().ok())
            .is_some_and(|count| count > requests)
            && said(d, "signed_in").as_deref() == Some("no")
            && s.counts.sign_in_reads.load(Ordering::SeqCst) >= reads + 2
    });
    assert_eq!(
        s.counts.machine_reads.load(Ordering::SeqCst),
        asked,
        "asked while signed out"
    );
    assert_eq!(told_targets(&s), 1);

    s.counts.signed_out.store(false, Ordering::SeqCst);
    until(2);
    let told = s.ask(json!({"op":"targets"}));
    let start = Instant::now();
    while s.ask(json!({"op":"targets"}))["targets"]
        != json!([{"alias":"macbook-air","engine":"claude","name":"MacBook Air"},
            {"alias":"office","engine":"claude","name":"Office iMac"}])
    {
        assert!(start.elapsed() < Duration::from_secs(5), "{told}");
        thread::sleep(Duration::from_millis(10));
    }
    // The first renamed: the same alias, its new name told.
    s.counts.machines.lock().unwrap()[1].label = "Studio".into();
    let start = Instant::now();
    while s.ask(json!({"op":"targets"}))["targets"][1]["name"] != "Studio" {
        assert!(start.elapsed() < Duration::from_secs(5), "rename not told");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        s.ask(json!({"op":"targets"}))["targets"][1]["alias"],
        "office"
    );
}

#[test]
fn sign_in_code_claims_share_one_owner_and_keep_legacy_repeats_explicit() {
    use lamp_tasks::sign_in::Event;
    let off = Running::new(false);
    assert_eq!(
        off.trusted(json!({"op":"sign_in_take_code","id":1}))["error"],
        "sign_in_off"
    );
    let mut s = Running::unstarted(false, false, false, false);
    s.config.sign_in_by_code = true;
    s.start();
    let state = s.trusted(json!({"op":"sign_in_state"}));
    assert_eq!(state["sign_in"], "none");
    assert_eq!(state["code_claims"], 1);
    let absent = s.trusted(json!({"op":"sign_in_take_code","id":1}));
    assert_eq!(absent["claim"], "not_available");
    assert!(absent.get("code").is_none());
    assert_eq!(s.counts.sign_in_starts.load(Ordering::SeqCst), 0);

    let started = s.trusted(json!({"op":"sign_in_start"}));
    assert_eq!(started["sign_in"], "starting");
    let id = started["id"].as_u64().unwrap();
    assert_eq!(
        s.trusted(json!({"op":"sign_in_take_code","id":id}))["claim"],
        "waiting"
    );
    s.counts
        .sign_in_lines
        .lock()
        .unwrap()
        .push_back(Ok(Event::Code {
            code: "12345678".into(),
            expires_in: 120,
        }));
    let untrusted = s.ask(json!({"op":"sign_in_take_code","id":id}));
    assert_eq!(untrusted["error"], "untrusted");
    assert!(untrusted.get("code").is_none());
    let stale = s.trusted(json!({"op":"sign_in_take_code","id":id + 1}));
    assert_eq!(stale["claim"], "not_available");
    assert!(stale.get("code").is_none());

    // Whichever Voice process claims first owns this initial payload.
    let first = s.trusted(json!({"op":"sign_in_take_code","id":id}));
    assert_eq!(first["claim"], "granted");
    assert_eq!(first["sign_in"], "code");
    assert_eq!(first["code"], "12345678");
    assert_eq!(first["code_told"], true);
    let second = s.trusted(json!({"op":"sign_in_take_code","id":id}));
    assert_eq!(second["claim"], "already_given");
    assert!(second.get("code").is_none());
    assert!(second.get("email").is_none());
    let repeated = s.trusted(json!({"op":"sign_in_start"}));
    assert_eq!(repeated["id"], id);
    assert_eq!(repeated["code"], "12345678");
    assert_eq!(repeated["code_told"], true);
    s.counts
        .sign_in_lines
        .lock()
        .unwrap()
        .push_back(Ok(Event::Code {
            code: "12345678".into(),
            expires_in: 120,
        }));
    let kept = s.trusted(json!({"op":"sign_in_take_code","id":id}));
    assert_eq!(kept["claim"], "already_given");
    assert!(kept.get("code").is_none());

    // The opposite order: a legacy START reply consumes the next code's
    // initial handoff before a news reader tries its claim.
    s.counts
        .sign_in_lines
        .lock()
        .unwrap()
        .push_back(Ok(Event::Code {
            code: "87654321".into(),
            expires_in: 120,
        }));
    let legacy = s.trusted(json!({"op":"sign_in_start"}));
    assert_eq!(legacy["code"], "87654321");
    assert_eq!(legacy["code_told"], true);
    let after_legacy = s.trusted(json!({"op":"sign_in_take_code","id":id}));
    assert_eq!(after_legacy["claim"], "already_given");
    assert!(after_legacy.get("code").is_none());
    assert_eq!(s.counts.sign_in_starts.load(Ordering::SeqCst), 1);
    assert!(s.counts.sign_in_answers.lock().unwrap().is_empty());
}

/// #1676: Harness's sign-in by a spoken code through the service, on the
/// trusted route only: started once while open, its code, the question,
/// the answer passed on only once the voice heard the question told, in a
/// later request, and how it ended, kept for the voice; started anew, with
/// the next id, once done. Without `--sign-in-by-code` it is off.
#[test]
fn the_sign_in_by_code_is_the_trusted_routes_from_its_code_to_its_end() {
    use lamp_tasks::sign_in::{Done, Event};
    let off = Running::new(false);
    assert_eq!(
        off.trusted(json!({"op":"sign_in_start"}))["error"],
        "sign_in_off"
    );
    let mut s = Running::unstarted(false, false, false, false);
    s.config.sign_in_by_code = true;
    s.start();
    assert_eq!(s.ask(json!({"op":"sign_in_start"}))["error"], "untrusted");
    assert_eq!(s.ask(json!({"op":"sign_in_state"}))["error"], "untrusted");
    assert_eq!(s.trusted(json!({"op":"sign_in_state"}))["sign_in"], "none");
    s.counts
        .sign_in_lines
        .lock()
        .unwrap()
        .push_back(Ok(Event::Code {
            code: "12345678".into(),
            expires_in: 120,
        }));
    let started = s.trusted(json!({"op":"sign_in_start"}));
    assert_eq!(started["ok"], true, "{started}");
    assert_eq!(started["sign_in"], "code");
    assert_eq!(started["code"], "12345678");
    // Counted from the clock, so not taken for one before a restart.
    let id = started["id"].as_u64().unwrap();
    assert!(id > 1_700_000_000_000, "{started}");
    assert_eq!(started["code_told"], true);
    // Asked again while open: the same one, not another.
    assert_eq!(s.trusted(json!({"op":"sign_in_start"}))["code"], "12345678");
    assert_eq!(s.counts.sign_in_starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        s.trusted(json!({"op":"sign_in_answer","answer":"yes","request":2}))["error"],
        "not_asking"
    );
    assert_eq!(
        s.trusted(json!({"op":"sign_in_asked","id":id,"request":2}))["error"],
        "not_asking"
    );
    s.counts
        .sign_in_lines
        .lock()
        .unwrap()
        .push_back(Ok(Event::Confirm {
            email: "someone@example.com".into(),
        }));
    let until = |want: &str| {
        let start = Instant::now();
        loop {
            let state = s.trusted(json!({"op":"sign_in_state"}));
            if state["sign_in"] == want {
                return state;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "{state}");
            thread::sleep(Duration::from_millis(10));
        }
    };
    assert_eq!(until("asking")["email"], "someone@example.com");
    assert_eq!(
        s.ask(json!({"op":"sign_in_answer","answer":"yes","request":9}))["error"],
        "untrusted"
    );
    assert_eq!(
        s.ask(json!({"op":"sign_in_asked","id":id,"request":3}))["error"],
        "untrusted"
    );
    // Asked in Harness, but not yet heard said by the lamp: a yes is for
    // something else.
    assert_eq!(
        s.trusted(json!({"op":"sign_in_answer","answer":"yes","request":9}))["error"],
        "not_told"
    );
    assert_eq!(
        s.trusted(json!({"op":"sign_in_asked","id":id + 7,"request":3}))["error"],
        "not_this_sign_in"
    );
    let told = s.trusted(json!({"op":"sign_in_asked","id":id,"request":3}));
    assert_eq!(told["sign_in"], "asking", "{told}");
    // The yes in the request the question was told in isn't its answer.
    assert_eq!(
        s.trusted(json!({"op":"sign_in_answer","answer":"yes","request":3}))["error"],
        "not_told"
    );
    assert!(s.counts.sign_in_answers.lock().unwrap().is_empty());
    let answered = s.trusted(json!({"op":"sign_in_answer","answer":"yes","request":4}));
    assert_eq!(answered["sign_in"], "answered", "{answered}");
    assert_eq!(*s.counts.sign_in_answers.lock().unwrap(), [true]);
    s.counts
        .sign_in_lines
        .lock()
        .unwrap()
        .push_back(Ok(Event::Result(Done::Success {
            email: Some("someone@example.com".into()),
        })));
    // Not before the sign-in has succeeded.
    assert_eq!(s.counts.daemon_starts.load(Ordering::SeqCst), 0);
    assert_eq!(until("done")["result"], "success");
    // Harness's own sign-in stops its daemon (openharness#1463): started
    // once after it, and not again.
    let start = Instant::now();
    while s.counts.daemon_starts.load(Ordering::SeqCst) == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "the daemon was never started"
        );
        thread::sleep(Duration::from_millis(10));
    }
    thread::sleep(Duration::from_millis(200));
    assert_eq!(s.counts.daemon_starts.load(Ordering::SeqCst), 1);
    // Done: kept for the voice, and a start begins anew.
    assert_eq!(
        s.trusted(json!({"op":"sign_in_state"}))["result"],
        "success"
    );
    assert_eq!(s.trusted(json!({"op":"sign_in_start"}))["id"], id + 1);
    assert_eq!(s.counts.sign_in_starts.load(Ordering::SeqCst), 2);
}

/// openharness#1463, the reviewer-lead's P2 on #2309: a hand-over waiting
/// when a sign-in by code succeeds goes to Harness only after its daemon
/// was started again, and a sign-in that fails starts nothing (the
/// reviewer's P3 2).
#[test]
fn after_a_sign_in_the_daemon_is_started_before_a_waiting_hand_over() {
    use lamp_tasks::sign_in::{Done, Event};
    let mut s = Running::new(false);
    s.stop();
    s.config.sign_in_by_code = true;
    s.start();
    let state = |s: &Running, want: &str| {
        let start = Instant::now();
        loop {
            let state = s.trusted(json!({"op":"sign_in_state"}));
            if state["sign_in"] == want {
                return state;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "{state}");
            thread::sleep(Duration::from_millis(10));
        }
    };
    // Refused or cancelled: nothing is started. Each ends after its start,
    // so the loop sees it open first (the reviewer's P3 on #2309).
    for code in ["DENIED", "CANCELLED"] {
        s.counts
            .sign_in_lines
            .lock()
            .unwrap()
            .push_back(Ok(Event::Code {
                code: "12345678".into(),
                expires_in: 120,
            }));
        let started = s.trusted(json!({"op":"sign_in_start"}));
        assert_eq!(started["ok"], true, "{started}");
        state(&s, "code");
        s.counts
            .sign_in_lines
            .lock()
            .unwrap()
            .push_back(Ok(Event::Result(Done::Error { code: code.into() })));
        let ended = state(&s, "done");
        assert_eq!(ended["result"], "error", "{ended}");
    }
    thread::sleep(Duration::from_millis(200));
    assert_eq!(s.counts.daemon_starts.load(Ordering::SeqCst), 0);
    // A first hand-over holds the job lane while the sign-in succeeds and
    // a second is confirmed.
    s.counts.hold_launch.store(true, Ordering::SeqCst);
    let first = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&first))["ok"], true);
    let start = Instant::now();
    while s.counts.launches.load(Ordering::SeqCst) == 0 {
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(s.trusted(json!({"op":"sign_in_start"}))["ok"], true);
    s.counts
        .sign_in_lines
        .lock()
        .unwrap()
        .push_back(Ok(Event::Result(Done::Success { email: None })));
    assert_eq!(state(&s, "done")["result"], "success");
    let second = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&second))["ok"], true);
    thread::sleep(Duration::from_millis(100));
    s.counts.hold_launch.store(false, Ordering::SeqCst);
    let start = Instant::now();
    while s.counts.launches.load(Ordering::SeqCst) < 2 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "{:?}",
            s.counts.order
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        *s.counts.order.lock().unwrap(),
        ["launch", "daemon", "launch"]
    );
    // Each launch's end kept before the service is stopped: the store syncs
    // with `fullfsync`, and on a Mac under the whole binary's load a write
    // still under way took past the stop's 300 ms (#1569 row 76).
    for prepared in [&first, &second] {
        s.wait(&prepared["id"], |v| {
            !v["task"]["dispatch"]["finished"].is_null()
        });
    }
}

/// lamp-75c9, 21 October: the daemon's start right after a spoken sign-in
/// said `start_failed`, and one by hand a minute later worked. A failed
/// start is tried again later, twice as long after each failure, and a
/// hand-over confirmed meanwhile waits for it; after the last try it goes
/// as before.
#[test]
fn a_failed_daemon_start_is_tried_again_before_a_waiting_hand_over() {
    use lamp_tasks::sign_in::{Done, Event};
    let mut s = Running::new(false);
    s.stop();
    s.config.sign_in_by_code = true;
    s.start();
    let signed_in = |s: &Running| {
        assert_eq!(s.trusted(json!({"op":"sign_in_start"}))["ok"], true);
        s.counts
            .sign_in_lines
            .lock()
            .unwrap()
            .push_back(Ok(Event::Result(Done::Success { email: None })));
    };
    let launched = |s: &Running, want: usize| {
        let start = Instant::now();
        while s.counts.launches.load(Ordering::SeqCst) < want {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "{:?}",
                s.counts.order
            );
            thread::sleep(Duration::from_millis(5));
        }
    };
    // Two starts fail: the third, 50 and 100 ms later, works, and the
    // hand-over confirmed after the first failure goes only then.
    s.counts.daemon_fails.store(2, Ordering::SeqCst);
    signed_in(&s);
    let start = Instant::now();
    while s.counts.daemon_starts.load(Ordering::SeqCst) == 0 {
        assert!(start.elapsed() < Duration::from_secs(5), "never started");
        thread::sleep(Duration::from_millis(2));
    }
    let first = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&first))["ok"], true);
    launched(&s, 1);
    assert_eq!(
        *s.counts.order.lock().unwrap(),
        ["daemon", "daemon", "daemon", "launch"]
    );
    // Every try fails: four in all, and then the hand-over goes as before.
    s.counts.order.lock().unwrap().clear();
    s.counts.daemon_fails.store(10, Ordering::SeqCst);
    signed_in(&s);
    let start = Instant::now();
    while s.counts.daemon_starts.load(Ordering::SeqCst) < 4 {
        assert!(start.elapsed() < Duration::from_secs(5), "not tried again");
        thread::sleep(Duration::from_millis(2));
    }
    let second = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&second))["ok"], true);
    launched(&s, 2);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(
        *s.counts.order.lock().unwrap(),
        ["daemon", "daemon", "daemon", "daemon", "launch"]
    );
    // Each launch's end kept before the service is stopped: the store syncs
    // with `fullfsync`, and on a Mac under the whole binary's load a write
    // still under way took past the stop's 300 ms (#1569 row 76).
    for prepared in [&first, &second] {
        s.wait(&prepared["id"], |v| {
            !v["task"]["dispatch"]["finished"].is_null()
        });
    }
}

/// Without `serve --set-up-later` nothing of it is said, and the targets
/// file is not looked at again.
#[test]
fn without_set_up_later_the_report_is_as_before() {
    let s = Running::new(false);
    s.counts.signed_out.store(true, Ordering::SeqCst);
    thread::sleep(Duration::from_millis(100));
    let report = tasks_diagnostic(&s);
    for key in ["handovers_off", "signed_in", "targets_invalid"] {
        assert!(said(&report, key).is_none(), "{key}: {report:?}");
    }
    assert_eq!(report.message, "");
    assert_eq!(told_targets(&s), 1);
}

/// The store once the stopped service's owner has let its folder go. A
/// start once failed here with "tasks directory is already owned" (once in
/// twelve full runs of this file): a child another test spawned held a copy
/// of the lock ([`SPAWNS`]). The store is let go in the turn's share too.
fn released(s: &Running) -> Released {
    let share = share();
    let start = Instant::now();
    loop {
        match Store::open(&s.config.dir) {
            Ok(store) => {
                return Released {
                    store,
                    _share: share,
                };
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                assert!(
                    start.elapsed() < Duration::from_secs(5),
                    "the store stayed owned"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("{error}"),
        }
    }
}

/// A store the test opened itself, let go before its share of the turn.
struct Released {
    store: Store,
    _share: RwLockReadGuard<'static, ()>,
}

impl std::ops::Deref for Released {
    type Target = Store;
    fn deref(&self) -> &Store {
        &self.store
    }
}

impl std::ops::DerefMut for Released {
    fn deref_mut(&mut self) -> &mut Store {
        &mut self.store
    }
}

/// #1505: a store a newer build wrote is set aside, and until someone looks
/// (the kept file removed, or put back by hand), the part says so: across a
/// restart too, and not once it is gone.
#[test]
fn a_store_set_aside_is_said_until_its_file_is_gone() {
    let mut s = Running::new(false);
    s.stop();
    let mut store = released(&s);
    let mut newer: Value = serde_json::from_slice(&store.load().unwrap().unwrap()).unwrap();
    newer["schema"] = json!(5);
    store.commit(&serde_json::to_vec(&newer).unwrap()).unwrap();
    drop(store);
    s.start();
    let said = |s: &Running| {
        let report = tasks_diagnostic(s);
        let kept = report
            .values
            .iter()
            .find(|pair| pair.key == "set_aside")
            .map(|pair| pair.value.clone());
        (report.level, report.message, kept)
    };
    let (level, message, kept) = said(&s);
    assert_eq!(level, Level::Warn);
    assert_eq!(
        message,
        "Tasks set aside a store from a newer build; its tasks are not followed"
    );
    let kept = kept.expect("the kept file is named");
    assert!(kept.starts_with("tasks.sqlite3.newer-5-"), "{kept}");
    // Restarted: the store opens well now, and the kept file is still said.
    s.stop();
    drop(released(&s));
    s.start();
    assert_eq!(said(&s).2.as_deref(), Some(kept.as_str()));
    // Someone looked: the file is gone, and it is said no more.
    s.stop();
    drop(released(&s));
    std::fs::remove_file(s.config.dir.join(&kept)).unwrap();
    s.start();
    let (level, message, kept) = said(&s);
    assert_eq!((level, message.as_str(), kept), (Level::Ok, "", None));
}

/// #1676: `--targets-from-machines` takes an engine Harness knows, and only
/// goes with `--set-up-later`, refused before anything is read.
#[test]
fn targets_from_machines_needs_an_engine_and_set_up_later() {
    let refused = |args: &[&str]| {
        let output = output_of(Command::new(env!("CARGO_BIN_EXE_lamp-tasks")).args(args));
        assert!(!output.status.success(), "{args:?}");
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let base = [
        "serve",
        "--enable",
        "--targets",
        "/does-not-exist",
        "--harness",
        "/does-not-exist",
    ];
    let without = refused(&[&base[..], &["--targets-from-machines", "claude"]].concat());
    assert!(without.contains("only with --set-up-later"), "{without}");
    let unknown = refused(
        &[
            &base[..],
            &["--set-up-later", "--targets-from-machines", "gpt"],
        ]
        .concat(),
    );
    assert!(
        unknown.contains("needs claude, codex or opencode"),
        "{unknown}"
    );
}

/// The tasks database, opened by hand as a newer build would write it.
fn tasks_database(s: &Running) -> rusqlite::Connection {
    rusqlite::Connection::open(s.config.dir.join("tasks.sqlite3")).unwrap()
}

/// #1680: a tasks database from a newer build (`user_version` 2) doesn't
/// stop the part: it is kept whole, the part starts with an empty store,
/// serves, and says so on health.
#[test]
fn a_newer_tasks_database_is_set_aside_and_the_part_serves() {
    let mut s = Running::new(false);
    let prepared = s.prepare();
    s.stop();
    tasks_database(&s)
        .execute_batch("PRAGMA user_version=2")
        .unwrap();
    s.start();
    assert_eq!(s.status()["summary"]["retained"], 0);
    let kept = s.status()["summary"]["set_aside"]
        .as_str()
        .expect("the kept store is said")
        .to_owned();
    assert!(kept.starts_with("tasks.sqlite3.newer-db2-"), "{kept}");
    let old = rusqlite::Connection::open(s.config.dir.join(&kept)).unwrap();
    let snapshot: Vec<u8> = old
        .query_row("SELECT snapshot FROM state", [], |row| row.get(0))
        .unwrap();
    let snapshot: Value = serde_json::from_slice(&snapshot).unwrap();
    assert_eq!(
        snapshot["tasks"][0]["id"], prepared["id"],
        "its task is kept"
    );
    drop(old);
    s.prepare();
    let report = tasks_diagnostic_until(&s, |report| {
        report.values.iter().any(|pair| pair.key == "set_aside")
    });
    assert_eq!(report.level, Level::Warn);
}

/// #1680: a stored task with a value this build doesn't know, at the same
/// `schema`, doesn't stop the part: it is kept in the store as it is, the
/// part serves, and health counts it.
#[test]
fn a_stored_task_this_build_cannot_read_is_kept_and_the_part_serves() {
    let mut s = Running::new(false);
    let prepared = s.prepare();
    s.stop();
    let db = tasks_database(&s);
    let snapshot: Vec<u8> = db
        .query_row("SELECT snapshot FROM state", [], |row| row.get(0))
        .unwrap();
    let mut state: Value = serde_json::from_slice(&snapshot).unwrap();
    state["tasks"][0]["dispatch"]["state"] = json!("a_state_from_later");
    let later = state["tasks"][0].clone();
    db.execute(
        "UPDATE state SET snapshot = ?1",
        [serde_json::to_vec(&state).unwrap()],
    )
    .unwrap();
    drop(db);
    s.start();
    let summary = s.status()["summary"].clone();
    assert_eq!(summary["retained"], 0, "{summary}");
    assert_eq!(summary["unread"], 1, "{summary}");
    assert!(summary["set_aside"].is_null(), "{summary}");
    let next = s.prepare();
    assert_ne!(next["id"], prepared["id"]);
    let report = tasks_diagnostic_until(&s, |report| {
        report
            .values
            .iter()
            .any(|pair| pair.key == "unread" && pair.value == "1")
    });
    assert_eq!(report.level, Level::Warn);
    s.stop();
    let snapshot: Vec<u8> = tasks_database(&s)
        .query_row("SELECT snapshot FROM state", [], |row| row.get(0))
        .unwrap();
    let state: Value = serde_json::from_slice(&snapshot).unwrap();
    assert_eq!(state["tasks"][0], later, "kept as it is");
    assert_eq!(state["tasks"][1]["id"], next["id"]);
}
