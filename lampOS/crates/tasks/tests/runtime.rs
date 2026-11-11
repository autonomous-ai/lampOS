use lamp_bus::Bus;
use lamp_msgs::{AgentState, Diagnostic, Level, Sign, TaskStatus};
use lamp_tasks::{
    engine::{CLOSE_RETRY_MS, CLOSE_TRIES, Clock as Sample, WATCH_EARLY_MS},
    follow::WatchLane,
    harness::{
        AccountMachine, Closed, Ended, EngineKind, EngineRow, Launch, LaunchOutcome, Listed,
        Observation, SignIn, Target, WatchEnd, WatchEvent,
    },
    progress::SETTLED_FOR_MS,
    readiness::{Computer, engines_path, read_engines},
    runtime::{
        Adapter, Clock, Config, EnginesLane, LampSide, SetUpLater, WaitLane, lamp_side, request,
        serve_with_adapter,
    },
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
    /// Each machine a task was shown on, in order (#5978).
    shown_on: Mutex<Vec<String>>,
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
    /// Each `watch` held, with the agent it named (`--push-watch`).
    watches: Mutex<Vec<String>>,
    /// The `watch`es held now.
    watching: AtomicUsize,
    /// What the held `watch` says next, once set; `done` and `failed` end
    /// it, as `Cli::watch` does.
    watch_says: Mutex<Option<WatchEvent>>,
    /// `show` finds the agent idle with this answer while set, as Harness
    /// gives it: its `ask` is the task's own text.
    plain: Mutex<Option<String>>,
    /// `ps` also lists the person's own harness, idle, while set.
    theirs: AtomicBool,
    /// Each `close`, with the machine and agent it named (#1390).
    closes: Mutex<Vec<(String, String)>>,
    /// `show` takes longer than the 10 s `read_timeout` while set, as
    /// against the person's Mac on a lamp on 3 Nov (16 to 39 s): every
    /// ordinary read runs out (`timeout`), and only the read before a close,
    /// given longer, reads through.
    show_slow: AtomicBool,
    /// How many reads before a close still run out even so, as a `show`
    /// longer than that read's own bound; and how many were made.
    close_read_fails: AtomicUsize,
    close_reads: AtomicUsize,
    /// Harness is signed out while set (#1676), and how often whether it is
    /// signed in was asked.
    signed_out: AtomicBool,
    sign_in_reads: AtomicUsize,
    /// Reading whether Harness is signed in fails while set (#5488
    /// follow-up), as `harness auth status` that cannot be read.
    sign_in_fails: AtomicBool,
    /// What `harness machines` lists, and how often it was asked (#1676).
    machines: Mutex<Vec<AccountMachine>>,
    machine_reads: AtomicUsize,
    /// Harness's sign-in lines still to come, the answers written and how
    /// many sign-ins were started (#1676).
    sign_in_lines: Arc<Mutex<std::collections::VecDeque<SignInRead>>>,
    sign_in_answers: Arc<Mutex<Vec<bool>>>,
    sign_in_starts: AtomicUsize,
    /// How each sign-in was started (#5198): `--code` or `--qr`.
    sign_in_methods: Mutex<Vec<lamp_tasks::sign_in::Method>>,
    /// How many times Harness's daemon was started (openharness#1463).
    daemon_starts: AtomicUsize,
    /// How many daemon starts still fail, `start_failed` (lamp-75c9, 21 Oct).
    daemon_fails: AtomicUsize,
    /// Whether the adapter says its daemon is to be started again (its env
    /// file changed while it ran, #5978), and how many times it was.
    restart_wanted: AtomicBool,
    daemon_restarts: AtomicUsize,
    /// Each launch's machine and engine, as it was asked of Harness.
    launched_on: Mutex<Vec<(String, EngineKind)>>,
    /// Each launch and daemon start, in the order they ran, and a launch
    /// held on the job lane while set (the reviewer-lead's P2 on #2309).
    order: Mutex<Vec<&'static str>>,
    hold_launch: AtomicBool,
    /// What `harness machines engines` says of each machine, by id, and
    /// how many looks were made (#5199). A machine with none is unreachable.
    engines: Mutex<BTreeMap<String, Result<Vec<EngineRow>, String>>>,
    engine_looks: AtomicUsize,
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
struct ScriptWatch(Arc<Counts>);
impl WatchLane for ScriptWatch {
    fn watch(
        &self,
        machine: &str,
        agent: &str,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(WatchEvent),
    ) -> WatchEnd {
        assert_eq!(machine, "0123456789abcdef0123456789abcdef");
        self.0.watches.lock().unwrap().push(agent.to_owned());
        self.0.watching.fetch_add(1, Ordering::SeqCst);
        // The first line is how the harness stands now.
        on_event(WatchEvent::Working);
        let end = loop {
            if cancel.load(Ordering::Acquire) {
                break WatchEnd::Cancelled;
            }
            if let Some(event) = self.0.watch_says.lock().unwrap().take() {
                on_event(event);
                if matches!(event, WatchEvent::Done | WatchEvent::Failed) {
                    break WatchEnd::Settled;
                }
            }
            thread::sleep(Duration::from_millis(5));
        };
        self.0.watching.fetch_sub(1, Ordering::SeqCst);
        end
    }
}
struct ScriptEngines(Arc<Counts>);
impl EnginesLane for ScriptEngines {
    fn engines(&self, machine: &str) -> Result<Vec<EngineRow>, String> {
        self.0.engine_looks.fetch_add(1, Ordering::SeqCst);
        self.0
            .engines
            .lock()
            .unwrap()
            .get(machine)
            .cloned()
            .unwrap_or_else(|| Err("unreachable".into()))
    }
}
struct Script {
    counts: Arc<Counts>,
    stop: Arc<AtomicBool>,
    slow: bool,
}
impl Script {
    /// What `show` finds, however long it took.
    fn shown(&self, machine: &str, id: &str) -> Result<Observation, String> {
        // Read on the machine the task was started on: the scripted one,
        // or the lamp's own (#5978).
        assert!(
            machine == "0123456789abcdef0123456789abcdef" || machine == "a".repeat(32),
            "{machine}"
        );
        self.counts
            .shown_on
            .lock()
            .unwrap()
            .push(machine.to_owned());
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
}
impl Adapter for Script {
    fn wait_lane(&self) -> Option<Arc<dyn WaitLane>> {
        Some(Arc::new(ScriptLane(self.counts.clone())))
    }
    fn watch_lane(&self) -> Option<Arc<dyn WatchLane>> {
        Some(Arc::new(ScriptWatch(self.counts.clone())))
    }
    fn engines_lane(&self) -> Option<Arc<dyn EnginesLane>> {
        Some(Arc::new(ScriptEngines(self.counts.clone())))
    }
    fn launch(&self, launch: &Launch) -> LaunchOutcome {
        self.counts
            .launched_on
            .lock()
            .unwrap()
            .push((launch.target.machine_id.clone(), launch.target.engine));
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
        if self.counts.show_slow.load(Ordering::SeqCst) {
            return Err("timeout".into());
        }
        self.shown(machine, id)
    }
    fn observe_to_close(&self, machine: &str, id: &str) -> Result<Observation, String> {
        self.counts.close_reads.fetch_add(1, Ordering::SeqCst);
        let fails = &self.counts.close_read_fails;
        if fails
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err("timeout".into());
        }
        self.shown(machine, id)
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
    fn restart_wanted(&self) -> bool {
        self.counts.restart_wanted.load(Ordering::SeqCst)
    }
    fn restart_daemon(&self) -> Result<(), String> {
        self.counts.daemon_restarts.fetch_add(1, Ordering::SeqCst);
        self.counts.order.lock().unwrap().push("restart");
        Ok(())
    }
    fn sign_in_starter(&self) -> Option<lamp_tasks::sign_in::Starter> {
        let counts = self.counts.clone();
        Some(Box::new(move |method| {
            counts.sign_in_starts.fetch_add(1, Ordering::SeqCst);
            counts.sign_in_methods.lock().unwrap().push(method);
            Ok(Box::new(ScriptSignIn(
                counts.sign_in_lines.clone(),
                counts.sign_in_answers.clone(),
            )))
        }))
    }
    fn signed_in(&self) -> Result<SignIn, String> {
        self.counts.sign_in_reads.fetch_add(1, Ordering::SeqCst);
        if self.counts.sign_in_fails.load(Ordering::SeqCst) {
            return Err("timeout".into());
        }
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
            push_watch: false,
            close_told,
            targets_newer: None,
            set_up_later: None,
            // Its report every 50 ms, not each second: a test reading it
            // waits a fraction of a second (#1128).
            sign_in_by_code: false,
            app_socket: None,
            app_caller: lamp_tasks::runtime::app_caller_is_lamp_cloud,
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
    /// The owner's app's route (`--app-socket`, #5198).
    fn app(&self, value: Value) -> Value {
        let socket = self.config.app_socket.as_ref().expect("an app socket");
        request(socket, &value, Duration::from_secs(2)).unwrap()
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

/// The close's state, its try and its code, as `get_task` shows them.
fn close_of(s: &Running, id: &Value) -> Value {
    s.ask(json!({"op":"get_task","id":id}))["task"]["close"].clone()
}

/// Seen on a lamp on 3 Nov: `show` against the person's Mac took 16 to 39
/// s, past the 10 s every read has, so the read before each close ran out
/// and no harness was ever closed. The read before a close has a bound of
/// its own (`harness::CLOSE_READ_TIMEOUT`): with every ordinary read running
/// out, the harness done and told is still read idle and closed. One the
/// same slow read finds waiting for the person is left to them, never closed.
#[test]
fn a_show_slower_than_the_read_bound_still_ends_in_a_close() {
    let s = Running::closing(true);
    *s.counts.plain.lock().unwrap() = Some("A poem about coffee.".into());
    let id = handed(&s);
    let observation = read(&s, &id);
    s.counts.show_slow.store(true, Ordering::SeqCst);
    tell(&s, &id, observation);
    assert_eq!(
        closed_within(&s, 5_000),
        [(
            "0123456789abcdef0123456789abcdef".to_string(),
            AGENT.to_string()
        )]
    );
    assert!(s.counts.close_reads.load(Ordering::SeqCst) > 0);
    s.wait(&id, |v| v["task"]["close"]["state"] == "closed");

    // Told idle, then the person answered it at the computer and it waits
    // for them again: the slow read before the close says so, and it is
    // left to them.
    let s = Running::closing(true);
    *s.counts.plain.lock().unwrap() = Some("A poem about coffee.".into());
    let id = handed(&s);
    let observation = read(&s, &id);
    s.counts.show_slow.store(true, Ordering::SeqCst);
    *s.counts.plain.lock().unwrap() = None;
    tell(&s, &id, observation);
    s.wait(&id, |v| v["task"]["close"]["state"] == "left");
    assert!(s.counts.close_reads.load(Ordering::SeqCst) > 0);
    s.clock.0.lock().unwrap().monotonic_ms = 10_000_000;
    assert!(closed_within(&s, 500).is_empty());
}

/// A read before a close that runs out even with its own bound is no idle
/// agent: the close fails (`observe_timeout`) and nothing is closed. It is
/// tried again a minute later, read first again, and closes then.
#[test]
fn a_close_whose_read_runs_out_is_tried_again_read_first() {
    let s = Running::closing(true);
    *s.counts.plain.lock().unwrap() = Some("A poem about coffee.".into());
    s.counts.close_read_fails.store(1, Ordering::SeqCst);
    let id = handed_and_told(&s);
    s.wait(&id, |v| v["task"]["close"]["state"] == "failed");
    let close = close_of(&s, &id);
    assert_eq!(close["code"], "observe_timeout", "{close}");
    assert_eq!(close["tries"], 1, "{close}");
    assert!(closes(&s).is_empty(), "closed on a read that ran out");
    s.clock.0.lock().unwrap().monotonic_ms += CLOSE_RETRY_MS;
    assert!(!closed_within(&s, 5_000).is_empty(), "never closed");
    assert_eq!(s.counts.close_reads.load(Ordering::SeqCst), 2);
    s.wait(&id, |v| v["task"]["close"]["state"] == "closed");
}

/// A read before a close that runs out every time is given up after
/// `CLOSE_TRIES` tries, a minute apart: nothing is closed, and it is never
/// read again for a close however long the lamp runs.
#[test]
fn a_close_whose_read_always_runs_out_gives_up_after_its_tries() {
    let s = Running::closing(true);
    *s.counts.plain.lock().unwrap() = Some("A poem about coffee.".into());
    s.counts
        .close_read_fails
        .store(usize::MAX, Ordering::SeqCst);
    let id = handed_and_told(&s);
    for tried in 1..=CLOSE_TRIES {
        s.wait(&id, |v| {
            v["task"]["close"]["state"] == "failed" && v["task"]["close"]["tries"] == tried
        });
        s.clock.0.lock().unwrap().monotonic_ms += CLOSE_RETRY_MS;
    }
    s.clock.0.lock().unwrap().monotonic_ms += 10 * CLOSE_RETRY_MS;
    thread::sleep(Duration::from_millis(300));
    assert_eq!(
        s.counts.close_reads.load(Ordering::SeqCst),
        usize::from(CLOSE_TRIES)
    );
    let close = close_of(&s, &id);
    assert_eq!(close["state"], "failed", "{close}");
    assert_eq!(close["tries"], CLOSE_TRIES, "{close}");
    assert_eq!(close["code"], "observe_timeout", "{close}");
    assert!(closes(&s).is_empty());
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
    assert_eq!(s.ask(json!({"op":"tasks_now"}))["error"], "progress_off");
}

/// The coordinator's lamp run on #4342: a task told before a restart of
/// the tasks part is told after it too, from the store, and leaves 30
/// minutes after its telling, not 2 hours after it settled. Its last step
/// is gone with the memory it was in.
#[test]
fn tasks_now_a_task_told_before_a_restart_stays_told_and_leaves_thirty_minutes_after() {
    let mut s = Running::with(false, true);
    *s.counts.about.lock().unwrap() = Some("Reading the brief".into());
    let id = handed(&s);
    let observation = read(&s, &id);
    tell(&s, &id, observation);
    assert_eq!(s.ask(json!({"op":"tasks_now"}))["tasks"][0]["told"], true);
    s.stop();
    s.start();
    let again = s.ask(json!({"op":"tasks_now"}));
    let task = &again["tasks"][0];
    assert_eq!(
        (&task["task"], &task["told"]),
        (&id, &json!(true)),
        "{again}"
    );
    assert_eq!(task["state"], "needs_input");
    assert_eq!(task["last_step"], Value::Null);
    // Thirty minutes after the telling, by the boot clock, across another
    // restart: gone, well before the two hours.
    s.stop();
    s.clock.0.lock().unwrap().monotonic_ms += lamp_tasks::now::TOLD_KEPT_MS;
    s.start();
    let gone = s.ask(json!({"op":"tasks_now"}));
    assert_eq!(gone["tasks"], json!([]), "{gone}");
}

/// Every file under `dir`, read whole, for words that must not be there.
fn files_hold(dir: &std::path::Path, words: &str) -> bool {
    fs::read_dir(dir).unwrap().any(|entry| {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_hold(&path, words)
        } else {
            fs::read(&path).is_ok_and(|bytes| {
                bytes
                    .windows(words.len())
                    .any(|window| window == words.as_bytes())
            })
        }
    })
}

/// #4342: `tasks_now` answers, from the progress read already made and the
/// store, who has each task, its state and since when, and its last step in
/// the agent's words, marked untrusted, in a reply under 2 KiB. The step
/// outlives the watch: kept once the task settles, and the task is marked
/// told once its news is. The words are never on the bus, in the
/// diagnostics or in the store's files. No `ps` of its own.
#[test]
fn with_progress_tasks_now_says_each_task_and_its_last_step_and_the_words_stay_here() {
    let s = Running::with(false, true);
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let mut watch = bus
        .subscribe_typed(TaskStatus::TOPIC, "tasks_test", TaskStatus::TYPE)
        .unwrap();
    // Nothing handed over: an empty record.
    let empty = s.ask(json!({"op":"tasks_now"}));
    assert_eq!(empty["ok"], true, "{empty}");
    assert_eq!(empty["tasks"], json!([]));
    assert_eq!(empty["left_out"], 0);
    *s.counts.about.lock().unwrap() = Some("Reading the brief".into());
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    let id = prepared["id"].as_u64().unwrap();
    // The fake `ps` is read at once; then the record has it.
    let start = Instant::now();
    let reply = loop {
        let reply = s.ask(json!({"op":"tasks_now"}));
        if reply["tasks"][0]["state"] == "working" {
            break reply;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "{reply}");
        thread::sleep(Duration::from_millis(10));
    };
    assert!(serde_json::to_vec(&reply).unwrap().len() <= 2_048);
    assert_eq!(reply["store_id"], s.status()["store_id"]);
    let task = &reply["tasks"][0];
    assert_eq!(
        (
            &task["task"],
            &task["agent"],
            &task["target"],
            &task["last_step"],
            &task["told"]
        ),
        (
            &json!(id),
            &json!("codex"),
            &json!("coding"),
            &json!({"untrusted":"Reading the brief"}),
            &json!(false)
        ),
        "{reply}"
    );
    assert!(
        task["since_ms"].is_u64() && task["last_active_ms"].is_u64(),
        "{task}"
    );
    let listings = s.counts.listings.load(Ordering::SeqCst);
    // Settled (the agent asks the person): it leaves the watch, and the
    // step read before is kept.
    let observation = read(&s, &prepared["id"]);
    let asked = s.ask(json!({"op":"tasks_now"}));
    assert_eq!(asked["tasks"][0]["state"], "needs_input", "{asked}");
    assert_eq!(
        asked["tasks"][0]["last_step"]["untrusted"],
        "Reading the brief"
    );
    assert_eq!(asked["tasks"][0]["told"], false);
    // Told: marked so.
    tell(&s, &prepared["id"], observation);
    assert_eq!(s.ask(json!({"op":"tasks_now"}))["tasks"][0]["told"], true);
    // `tasks_now` runs no `ps` of its own.
    assert_eq!(s.counts.listings.load(Ordering::SeqCst), listings);
    // The words stay here: not on the bus (`next_status` looks), not in
    // the diagnostics, not in the store's files.
    let _ = next_status(&mut watch);
    let mut diagnostic = bus
        .subscribe_typed(Diagnostic::TOPIC, "tasks_test", Diagnostic::TYPE)
        .unwrap();
    let message = loop {
        if let Some(message) = diagnostic.recv(Duration::from_secs(3)).unwrap() {
            break message;
        }
    };
    assert!(!String::from_utf8_lossy(&message.payload).contains("Reading the brief"));
    assert!(!files_hold(&s.config.dir, "Reading the brief"));
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

/// #5230: with `--push-watch`, one Harness `watch` is held on the task's
/// agent, a `working` line reads nothing, and `done` reads the agent at
/// once and keeps its idle answer; the watch is then let go. With
/// `--push-wait` too, the followed task is never also waited on. The
/// injected clock never moves, so only the push reads.
#[test]
fn a_held_watch_reads_the_agent_at_once_on_done_and_is_let_go() {
    let mut s = Running::unstarted(false, false, true, false);
    s.config.push_watch = true;
    s.config.push_wait = true;
    s.start();
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    let start = Instant::now();
    while s.counts.watches.lock().unwrap().is_empty() {
        assert!(start.elapsed() < Duration::from_secs(5), "no watch held");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(*s.counts.watches.lock().unwrap(), vec![AGENT.to_owned()]);
    // `working`: nothing read, and once heard from, no `wait` beside it.
    thread::sleep(Duration::from_millis(100));
    assert_eq!(s.counts.reads.load(Ordering::SeqCst), 0);
    let waited = s.counts.waits.lock().unwrap().len();
    thread::sleep(Duration::from_millis(100));
    assert_eq!(s.counts.waits.lock().unwrap().len(), waited);
    assert!(waited <= 1, "a wait held beside the watch: {waited}");
    // Done: read at once, its idle answer kept, the watch let go.
    s.counts.idle.store(true, Ordering::SeqCst);
    *s.counts.watch_says.lock().unwrap() = Some(WatchEvent::Done);
    let kept = s.wait(&prepared["id"], |v| v["task"]["observations"] == json!(1));
    assert_eq!(s.counts.reads.load(Ordering::SeqCst), 1, "{kept}");
    thread::sleep(Duration::from_millis(100));
    assert_eq!(s.counts.watching.load(Ordering::SeqCst), 0);
    assert_eq!(s.counts.watches.lock().unwrap().len(), 1);
    assert_eq!(s.counts.reads.load(Ordering::SeqCst), 1);
}

/// #5230: a watch held when the part stops is ended with it.
#[test]
fn every_held_watch_ends_when_the_part_stops() {
    let mut s = Running::unstarted(false, false, true, false);
    s.config.push_watch = true;
    s.start();
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    let start = Instant::now();
    while s.counts.watching.load(Ordering::SeqCst) == 0 {
        assert!(start.elapsed() < Duration::from_secs(5), "no watch held");
        thread::sleep(Duration::from_millis(5));
    }
    s.stop();
    assert_eq!(s.counts.watching.load(Ordering::SeqCst), 0);
}

/// Without `--push-watch` no watch is ever held, whatever the adapter has.
#[test]
fn without_push_watch_no_watch_is_held() {
    let s = Running::configured(false, false, true);
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        v["task"]["dispatch"]["state"] == "accepted"
    });
    thread::sleep(Duration::from_millis(100));
    assert!(s.counts.watches.lock().unwrap().is_empty());
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
        engines: false,
        lamp: None,
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

/// #5488 follow-up (the reviewer-lead's P3 on #5583): a sign-in read that
/// fails, through the part's own loop, keeps what was last read for two
/// failed reads, lets it go at the third, so `status` says no `signed_in`,
/// and says it again once a read answers.
#[test]
fn a_sign_in_read_failing_three_times_in_a_row_is_let_go_by_the_part() {
    let mut s = Running::unstarted(false, false, false, false);
    s.config.set_up_later = Some(SetUpLater {
        targets: s.root.path().join("targets.json"),
        every: Duration::from_millis(300),
        from_machines: None,
        engines: false,
        lamp: None,
    });
    s.start();
    let signed_in = |s: &Running| s.status()["signed_in"].clone();
    let until = |s: &Running, wanted: Value| {
        let start = Instant::now();
        while signed_in(s) != wanted {
            assert!(start.elapsed() < Duration::from_secs(5), "never {wanted}");
            thread::sleep(Duration::from_millis(10));
        }
    };
    until(&s, json!("yes"));
    let base = s.counts.sign_in_reads.load(Ordering::SeqCst);
    s.counts.sign_in_fails.store(true, Ordering::SeqCst);
    let start = Instant::now();
    while s.counts.sign_in_reads.load(Ordering::SeqCst) < base + 2 {
        assert!(start.elapsed() < Duration::from_secs(5), "no failed reads");
        thread::sleep(Duration::from_millis(5));
    }
    thread::sleep(Duration::from_millis(60));
    assert_eq!(signed_in(&s), json!("yes"), "two failed reads keep it");
    until(&s, Value::Null);
    assert!(
        s.counts.sign_in_reads.load(Ordering::SeqCst) >= base + 3,
        "let go only after three failed reads"
    );
    s.counts.sign_in_fails.store(false, Ordering::SeqCst);
    until(&s, json!("yes"));
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
        engines: false,
        lamp: None,
    });
    let machine = |id: char, label: &str, current: bool| AccountMachine {
        machine_id: id.to_string().repeat(32),
        label: label.into(),
        current,
        status: None,
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

/// A service with `--targets-from-machines claude` and, as asked,
/// `--engines-from-machines` (#5199): the lamp and one computer, `Office
/// iMac`, running; a look every 20 ms, the machines' every 100.
fn engines_running(engines: bool) -> (Running, std::path::PathBuf) {
    let mut s = Running::unstarted(false, false, false, false);
    let path = s.root.path().join("targets.json");
    s.config.targets = BTreeMap::new();
    s.config.set_up_later = Some(SetUpLater {
        targets: path.clone(),
        every: Duration::from_millis(20),
        from_machines: Some(EngineKind::Claude),
        engines,
        lamp: None,
    });
    let machine = |id: char, label: &str, current: bool| AccountMachine {
        machine_id: id.to_string().repeat(32),
        label: label.into(),
        current,
        status: Some("running".into()),
    };
    *s.counts.machines.lock().unwrap() = vec![
        machine('a', "lamp", true),
        machine('b', "Office iMac", false),
    ];
    (s, path)
}

fn office_engines(s: &Running, rows: Result<Vec<EngineRow>, String>) {
    s.counts
        .engines
        .lock()
        .unwrap()
        .insert("b".repeat(32), rows);
}

fn row(engine: &str, installed: Option<bool>, signed_in: Option<bool>) -> EngineRow {
    EngineRow {
        engine: engine.into(),
        installed,
        installable: None,
        signed_in,
    }
}

fn prepare_office(s: &Running) -> Value {
    s.ask(
        json!({"op":"prepare","guard":s.guard(),"text":"Private scripted task","target":"office"}),
    )
}

/// Prepares to `office` until the answer's `error` is `code` (`None`: until
/// one is prepared), within 5 s.
fn prepared_until(s: &Running, code: Option<&str>) -> Value {
    let start = Instant::now();
    loop {
        let answer = prepare_office(s);
        let done = match code {
            Some(code) => answer["error"] == code,
            None => answer["ok"] == true,
        };
        if done {
            return answer;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "not {code:?}: {answer}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// #5199: with `--engines-from-machines`, each computer is asked which
/// engines it has, off the hand-overs' lane. Its target gets one it has
/// (Codex, with Claude Code unknown there), and the engines are written
/// beside the targets. A `prepare` is refused at once when that engine is
/// known not signed in, or the computer known offline; a failed look keeps
/// what was known; once it is signed in and awake, the work is prepared.
#[test]
fn engines_from_machines_choose_an_engine_there_and_refuse_early() {
    let (mut s, path) = engines_running(true);
    office_engines(
        &s,
        Ok(vec![
            row("claude", None, None),
            row("codex", Some(true), Some(false)),
        ]),
    );
    s.start();
    let start = Instant::now();
    while s.ask(json!({"op":"targets"}))["targets"]
        != json!([{"alias":"office","engine":"codex","name":"Office iMac"}])
    {
        assert!(start.elapsed() < Duration::from_secs(5), "no codex target");
        thread::sleep(Duration::from_millis(10));
    }
    let refused = prepared_until(&s, Some("engine_not_signed_in"));
    assert_eq!(refused["ok"], false, "{refused}");
    let known = read_engines(&engines_path(&path)).unwrap();
    assert_eq!(known.keys().collect::<Vec<_>>(), ["office"]);
    let office: &Computer = &known["office"];
    assert_eq!(office.status.as_ref().unwrap().value, "running");
    assert!(!office.engines["codex"].signed_in.as_ref().unwrap().value);
    assert_eq!(office.engines.get("claude"), None, "a null is unknown");
    // A failed look keeps what was known: still refused after it.
    office_engines(&s, Err("unreachable".into()));
    let looks = s.counts.engine_looks.load(Ordering::SeqCst);
    let start = Instant::now();
    while s.counts.engine_looks.load(Ordering::SeqCst) < looks + 2 {
        assert!(start.elapsed() < Duration::from_secs(5), "not looked again");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(prepare_office(&s)["error"], "engine_not_signed_in");
    // Signed in there: prepared.
    office_engines(&s, Ok(vec![row("codex", Some(true), Some(true))]));
    let prepared = prepared_until(&s, None);
    assert_eq!(prepared["ok"], true, "{prepared}");
    // Asleep: refused, with nothing prepared; awake again: prepared.
    s.counts.machines.lock().unwrap()[1].status = Some("offline".into());
    prepared_until(&s, Some("computer_offline"));
    s.counts.machines.lock().unwrap()[1].status = Some("running".into());
    prepared_until(&s, None);
    // Nothing known not to be there, so the targets say nothing of it.
    assert_eq!(
        s.ask(json!({"op":"targets"}))["targets"][0].get("installed"),
        None
    );
}

/// Asks `targets` until they are `want`, within 5 s.
fn targets_until(s: &Running, want: Value) {
    let start = Instant::now();
    loop {
        let told = s.ask(json!({"op":"targets"}))["targets"].clone();
        if told == want {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "{told}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Prepares to `target` until `done` holds for the answer, within 5 s.
fn prepared_for(s: &Running, target: &str, done: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        let answer = s.ask(
            json!({"op":"prepare","guard":s.guard(),"text":"Private scripted task","target":target}),
        );
        if done(&answer) {
            return answer;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "{answer}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// #5978: with `LAMP_HERMES_ON_LAMP` (`lamp`), the lamp's own machine is a
/// target, `lamp` with Hermes, only while its look says Hermes is
/// installed; off, or Hermes missing there, it is left out as before. A
/// `prepare` for the office computer goes there while it is not known
/// offline, and to the lamp, saying so, once it is; one asking for the
/// lamp goes there whatever the computers.
#[test]
fn the_lamp_is_a_target_with_hermes_only_when_on_and_installed_there() {
    let lamp_engines = |s: &Running, rows: Vec<EngineRow>| {
        s.counts
            .engines
            .lock()
            .unwrap()
            .insert("a".repeat(32), Ok(rows));
    };
    let office = json!([{"alias":"office","engine":"claude","name":"Office iMac"}]);
    let both = json!([{"alias":"lamp","engine":"hermes"},{"alias":"office","engine":"claude","name":"Office iMac"}]);
    // Off: Hermes installed on the lamp changes nothing.
    let (mut off, _) = engines_running(true);
    office_engines(&off, Ok(vec![row("claude", Some(true), Some(true))]));
    lamp_engines(&off, vec![row("hermes", Some(true), None)]);
    off.start();
    targets_until(&off, office.clone());
    let looks = off.counts.engine_looks.load(Ordering::SeqCst);
    let start = Instant::now();
    while off.counts.engine_looks.load(Ordering::SeqCst) < looks + 3 {
        assert!(start.elapsed() < Duration::from_secs(5), "not looked again");
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(off.ask(json!({"op":"targets"}))["targets"], office);
    drop(off);

    let home = tempfile::tempdir().unwrap();
    let (mut s, _) = engines_running(true);
    s.config.set_up_later.as_mut().unwrap().lamp = Some(LampSide::new(home.path().to_owned()));
    office_engines(&s, Ok(vec![row("claude", Some(true), Some(true))]));
    // On, Hermes not found on the lamp: left out.
    lamp_engines(
        &s,
        vec![EngineRow {
            installable: Some(true),
            ..row("hermes", None, None)
        }],
    );
    s.start();
    targets_until(&s, office.clone());
    // Installed there: the lamp is a target, its folder made under
    // Harness's home, 0700.
    lamp_engines(&s, vec![row("hermes", Some(true), None)]);
    targets_until(&s, both);
    let folder = fs::metadata(home.path().join("harnesses/lamp-tasks/lamp")).unwrap();
    assert!(folder.is_dir());
    assert_eq!(folder.permissions().mode() & 0o7777, 0o700);
    // A computer asked for: it does the work, as before.
    let prepared = prepared_until(&s, None);
    assert_eq!(prepared.get("target"), None, "{prepared}");
    // The lamp asked for: the lamp, whatever the computers.
    let lamp = prepared_for(&s, "lamp", |answer| answer["ok"] == true);
    let id = lamp["id"].clone();
    let task = s.ask(json!({"op":"get_task","id":id}));
    assert_eq!(task["target"]["engine"], "hermes", "{task}");
    assert_eq!(task["target"]["machine_id"], "a".repeat(32));
    assert_eq!(task["target"]["project"], "~/harnesses/lamp-tasks/lamp");
    assert_eq!(task["task"]["target_alias"], "lamp");
    // The computer asleep: a hand-over asked of it is refused, never moved
    // to the lamp quietly.
    s.counts.machines.lock().unwrap()[1].status = Some("offline".into());
    prepared_for(&s, "office", |answer| answer["error"] == "computer_offline");
    // The lamp's task is followed as a computer's: started, shown on the
    // lamp's own machine, its answer read.
    s.counts.idle.store(true, Ordering::SeqCst);
    let confirm = json!({"op":"confirm","guard":s.guard(),"id":id,"prepared_digest":lamp["prepared_digest"],"session_id":s.status()["summary"]["session_id"]});
    assert_eq!(s.trusted(confirm)["ok"], true);
    s.wait(&id, |v| v["task"]["dispatch"]["state"] == "accepted");
    assert_eq!(
        s.ask(json!({"op":"observe","guard":s.guard(),"id":id}))["ok"],
        true
    );
    let task = s.wait(&id, |v| {
        v["observations"].as_array().is_some_and(|v| !v.is_empty())
    });
    let observation = task["observations"][0]["id"].clone();
    let observed = s.ask(json!({"op":"get_observation","id":id,"observation_id":observation}));
    assert_eq!(
        observed["data"]["answer"], "Lumi, Sunny and Archie.",
        "{observed}"
    );
    assert_eq!(*s.counts.shown_on.lock().unwrap(), ["a".repeat(32)]);
}

/// #5978, the owner's rule: a fresh lamp after an update with no computer
/// paired works on its own Hermes, the lamp being one device. Harness's
/// machines list only the lamp, its engines say Hermes is installed: the
/// targets are exactly `lamp` with Hermes, no error for having no
/// computer, and a hand-over to `lamp` (what the voice prepares with no
/// `where` and no `who`) is started with `harness new` on the lamp's own
/// machine.
#[test]
fn a_lamp_with_no_computer_hands_work_to_its_own_hermes() {
    let home = tempfile::tempdir().unwrap();
    let (mut s, path) = engines_running(true);
    s.config.set_up_later.as_mut().unwrap().lamp = Some(LampSide::new(home.path().to_owned()));
    s.counts.machines.lock().unwrap().truncate(1);
    s.counts
        .engines
        .lock()
        .unwrap()
        .insert("a".repeat(32), Ok(vec![row("hermes", Some(true), None)]));
    s.start();
    targets_until(&s, json!([{"alias":"lamp","engine":"hermes"}]));
    let written: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let targets = written["targets"].as_object().unwrap();
    assert_eq!(targets.len(), 1, "{written}");
    assert_eq!(targets["lamp"]["engine"], "hermes");
    assert_eq!(targets["lamp"]["machine_id"], "a".repeat(32));
    // The voice's call with no `where` and no `who` prepares for `lamp`.
    let prepared = prepared_for(&s, "lamp", |answer| answer["ok"] == true);
    let id = prepared["id"].clone();
    let confirm = json!({"op":"confirm","guard":s.guard(),"id":id,"prepared_digest":prepared["prepared_digest"],"session_id":s.status()["summary"]["session_id"]});
    assert_eq!(s.trusted(confirm)["ok"], true);
    s.wait(&id, |v| v["task"]["dispatch"]["state"] == "accepted");
    assert_eq!(
        *s.counts.launched_on.lock().unwrap(),
        [("a".repeat(32), EngineKind::Hermes)]
    );
}

/// #5978, the PM: with `LAMP_HERMES_ON_LAMP` off (unset, `off`, or any
/// value but `on`), routing is as on stable. The lamp is listed as a
/// Harness machine with Hermes installed, yet it is no target: the targets
/// are the office computer alone, work naming no machine (the voice's
/// `prepare` for its place's target, `office`) is started on the office
/// computer with Claude Code, and a `prepare` for `lamp` is refused as no
/// such target, never moved anywhere.
#[test]
fn with_hermes_on_lamp_off_work_naming_no_machine_goes_to_the_computer() {
    let home = tempfile::tempdir().unwrap();
    for value in [None, Some("off"), Some(""), Some("ON")] {
        let side = lamp_side(value, Some(home.path().to_owned()));
        assert!(side.is_none(), "{value:?}");
        let (mut s, path) = engines_running(true);
        s.config.set_up_later.as_mut().unwrap().lamp = side;
        office_engines(&s, Ok(vec![row("claude", Some(true), Some(true))]));
        s.counts
            .engines
            .lock()
            .unwrap()
            .insert("a".repeat(32), Ok(vec![row("hermes", Some(true), None)]));
        s.start();
        let office = json!([{"alias":"office","engine":"claude","name":"Office iMac"}]);
        targets_until(&s, office.clone());
        // Looked at again, the lamp is still left out.
        let looks = s.counts.engine_looks.load(Ordering::SeqCst);
        let start = Instant::now();
        while s.counts.engine_looks.load(Ordering::SeqCst) < looks + 3 {
            assert!(start.elapsed() < Duration::from_secs(5), "not looked again");
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(s.ask(json!({"op":"targets"}))["targets"], office);
        let written: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let targets = written["targets"].as_object().unwrap();
        assert_eq!(targets.len(), 1, "{value:?}: {written}");
        assert!(!targets.contains_key("lamp"), "{value:?}: {written}");
        assert!(
            !home.path().join("harnesses/lamp-tasks/lamp").exists(),
            "{value:?}"
        );
        // The lamp asked for: no such target, nothing prepared.
        assert_eq!(
            s.ask(json!({"op":"prepare","guard":s.guard(),"text":"Private scripted task","target":"lamp"}))["error"],
            "target",
            "{value:?}"
        );
        // Work naming no machine: prepared and started on the computer.
        let prepared = prepared_until(&s, None);
        let id = prepared["id"].clone();
        let task = s.ask(json!({"op":"get_task","id":id}));
        assert_eq!(task["target"]["engine"], "claude", "{value:?}: {task}");
        assert_eq!(task["target"]["machine_id"], "b".repeat(32));
        assert_eq!(task["task"]["target_alias"], "office");
        s.counts.idle.store(true, Ordering::SeqCst);
        let confirm = json!({"op":"confirm","guard":s.guard(),"id":id,"prepared_digest":prepared["prepared_digest"],"session_id":s.status()["summary"]["session_id"]});
        assert_eq!(s.trusted(confirm)["ok"], true, "{value:?}");
        s.wait(&id, |v| v["task"]["dispatch"]["state"] == "accepted");
        assert_eq!(
            *s.counts.launched_on.lock().unwrap(),
            [("b".repeat(32), EngineKind::Claude)],
            "{value:?}"
        );
    }
    // On, with a home: the lamp's side, as the on-case above.
    assert!(lamp_side(Some("on"), Some(home.path().to_owned())).is_some());
    assert!(lamp_side(Some("on"), None).is_none());
}

/// #5199: an engine not found on that computer (`installed` null with
/// `installable` true) is refused early, and the targets say so; unknown
/// proceeds.
#[test]
fn an_engine_known_not_there_is_refused_and_told_and_unknown_proceeds() {
    let (mut s, path) = engines_running(true);
    // A file written by hand, Codex on the office computer: never written
    // over, and its computer still asked.
    write_targets(
        &path,
        format!(
            "{}\n",
            json!({"version":1,"targets":{"office":{"machine_id":"b".repeat(32),"project":"~/p","engine":"codex"}}})
        )
        .as_bytes(),
    );
    let by_hand = fs::read(&path).unwrap();
    office_engines(
        &s,
        Ok(vec![
            row("claude", Some(true), Some(true)),
            // Harness's row for an engine not on the computer.
            EngineRow {
                installable: Some(true),
                ..row("codex", None, None)
            },
        ]),
    );
    s.start();
    prepared_until(&s, Some("engine_missing"));
    assert_eq!(
        s.ask(json!({"op":"targets"}))["targets"],
        json!([{"alias":"office","engine":"codex","installed":false}])
    );
    assert_eq!(fs::read(&path).unwrap(), by_hand);
    // The computer can't say any more: unknown, so it goes as before.
    office_engines(&s, Ok(vec![row("codex", None, None)]));
    prepared_until(&s, None);
}

/// #5199: without `--engines-from-machines` nothing is asked or refused:
/// the target is Claude Code's as before, an engines file beside the
/// targets is never read, and the work is prepared.
#[test]
fn without_engines_from_machines_nothing_is_asked_or_refused() {
    let (mut s, path) = engines_running(false);
    office_engines(&s, Ok(vec![row("claude", Some(true), Some(false))]));
    s.counts.machines.lock().unwrap()[1].status = Some("offline".into());
    // An engines file saying the computer is offline, read just now: not
    // read.
    let engines = engines_path(&path);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    write_targets(
        &engines,
        format!(
            "{}\n",
            json!({"version":1,"computers":{"office":{"status":{"value":"offline","read_ms":now_ms}}}})
        )
        .as_bytes(),
    );
    s.start();
    let start = Instant::now();
    while s.ask(json!({"op":"targets"}))["targets"]
        != json!([{"alias":"office","engine":"claude","name":"Office iMac"}])
    {
        assert!(start.elapsed() < Duration::from_secs(5), "no target");
        thread::sleep(Duration::from_millis(10));
    }
    let asked = s.counts.machine_reads.load(Ordering::SeqCst);
    let start = Instant::now();
    while s.counts.machine_reads.load(Ordering::SeqCst) < asked + 2 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "machines not read again"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(prepare_office(&s)["ok"], true);
    assert_eq!(s.counts.engine_looks.load(Ordering::SeqCst), 0);
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

/// #5379, lamp-c0a4 on 5 Nov: Harness put on by the onboarding install has
/// no daemon at boot, so after a reboot a signed-in lamp had none, and no
/// hand-over could go. With `--sign-in-by-code` and `--set-up-later`, a
/// part that first reads Harness as signed in starts its daemon once; one
/// that reads it signed out starts nothing, and nor does a part without
/// `--sign-in-by-code`.
#[test]
fn a_part_started_signed_in_starts_harnesss_daemon_once() {
    let started = |by_code: bool, signed_out: bool| {
        let mut s = Running::unstarted(false, false, false, false);
        s.config.sign_in_by_code = by_code;
        s.config.set_up_later = Some(SetUpLater {
            targets: s.root.path().join("targets.json"),
            every: Duration::from_millis(20),
            from_machines: None,
            engines: false,
            lamp: None,
        });
        s.counts.signed_out.store(signed_out, Ordering::SeqCst);
        s.start();
        // Many sign-in reads, every 20 ms: the start is made once, not at
        // each read.
        let start = Instant::now();
        while s.counts.sign_in_reads.load(Ordering::SeqCst) < 5 {
            assert!(start.elapsed() < Duration::from_secs(5), "no sign-in read");
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(Duration::from_millis(200));
        s.counts.daemon_starts.load(Ordering::SeqCst)
    };
    assert_eq!(started(true, false), 1, "signed in at the start");
    assert_eq!(started(true, true), 0, "signed out at the start");
    assert_eq!(started(false, false), 0, "no sign-in by code");
}

/// #5978, lamp-c0a4 on 10 Nov: Harness's daemon reads its env file only at
/// its start, so when lamp-tasks wrote it with other words while the
/// daemon ran, the daemon is started again once, before a hand-over; never
/// while a task is busy (watched): then it waits. An adapter that doesn't
/// ask for it is never restarted.
#[test]
fn a_changed_env_file_restarts_harnesss_daemon_once_and_never_while_a_task_is_busy() {
    // Asked, no task: once, however long the part runs.
    let mut s = Running::new(false);
    s.stop();
    s.counts.restart_wanted.store(true, Ordering::SeqCst);
    s.start();
    let start = Instant::now();
    while s.counts.daemon_restarts.load(Ordering::SeqCst) == 0 {
        assert!(start.elapsed() < Duration::from_secs(5), "never restarted");
        thread::sleep(Duration::from_millis(5));
    }
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        !v["task"]["dispatch"]["finished"].is_null()
    });
    thread::sleep(Duration::from_millis(200));
    assert_eq!(s.counts.daemon_restarts.load(Ordering::SeqCst), 1);
    assert_eq!(*s.counts.order.lock().unwrap(), ["restart", "launch"]);
    // The part started again with that task still busy: no restart.
    s.stop();
    s.start();
    thread::sleep(Duration::from_millis(300));
    assert_eq!(s.counts.daemon_restarts.load(Ordering::SeqCst), 1);
    // Not asked: never.
    let s = Running::new(false);
    let prepared = s.prepare();
    assert_eq!(s.trusted(s.confirmation(&prepared))["ok"], true);
    s.wait(&prepared["id"], |v| {
        !v["task"]["dispatch"]["finished"].is_null()
    });
    assert_eq!(s.counts.daemon_restarts.load(Ordering::SeqCst), 0);
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
    // One past the engine's `SCHEMA` (6 since #5978's `joined_on`).
    newer["schema"] = json!(7);
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
    assert!(kept.starts_with("tasks.sqlite3.newer-7-"), "{kept}");
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
    // #5199: `--engines-from-machines` asks the computers it finds.
    let alone = refused(&[&base[..], &["--set-up-later", "--engines-from-machines"]].concat());
    assert!(alone.contains("it needs that too"), "{alone}");
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

/// #5379: the app's route is lamp-cloud's unit's only. The socket is
/// `lampos`'s, so any unit run as `lampos` could connect; the kernel's word
/// on who connected is what lets one in. This test is no lamp-cloud, so
/// with the part's own check it is refused: the connection closed, no
/// answer, nothing started, while the other routes still answer it.
#[test]
fn the_apps_route_refuses_a_caller_that_is_not_lamp_cloud() {
    let mut s = Running::unstarted(false, false, false, false);
    s.config.sign_in_by_code = true;
    s.config.app_socket = Some(s.root.path().join("run/app"));
    s.start();
    let socket = s.config.app_socket.clone().unwrap();
    for op in [
        json!({"op":"app_sign_in_state"}),
        json!({"op":"app_sign_in_start","method":"code"}),
    ] {
        let answer = request(&socket, &op, Duration::from_secs(2));
        assert!(answer.is_err(), "{op} answered {answer:?}");
    }
    assert_eq!(
        s.trusted(json!({"op":"sign_in_state"}))["sign_in"],
        "none",
        "nothing was started"
    );
    s.stop();
}

/// #5198: the owner's app signs Harness in on its own route. Off without
/// `--app-socket`; the route takes only the app's operations, and they no
/// other route. An app-started sign-in is the app's to answer, once it was
/// given the question, and the voice sees only `in_app`: no code, no email,
/// no start, no claim, no answer. A voice-started one shows in the app with
/// its code and no email, and the app can neither start another nor answer
/// it. Every change moves `revision`; a read never does.
#[test]
fn the_apps_sign_in_is_its_own_routes_and_never_the_voices() {
    use lamp_tasks::sign_in::{Done, Event, Method};
    let push =
        |s: &Running, event: SignInRead| s.counts.sign_in_lines.lock().unwrap().push_back(event);
    let code = || {
        Ok(Event::Code {
            code: "12345678".into(),
            expires_in: 120,
        })
    };
    let confirm = || {
        Ok(Event::Confirm {
            email: "someone@example.com".into(),
        })
    };
    // Off: no app socket bound, and the app's operations refused elsewhere.
    let mut s = Running::unstarted(false, false, false, false);
    s.config.sign_in_by_code = true;
    s.start();
    assert_eq!(
        s.trusted(json!({"op":"app_sign_in_state"}))["error"],
        "untrusted"
    );
    assert_eq!(
        s.ask(json!({"op":"app_sign_in_start"}))["error"],
        "untrusted"
    );
    s.stop();
    drop(s);

    let mut s = Running::unstarted(false, false, false, false);
    s.config.sign_in_by_code = true;
    s.config.app_socket = Some(s.root.path().join("run/app"));
    // This test stands in for lamp-cloud (#5379).
    s.config.app_caller = |_| true;
    s.start();
    // The app's route takes nothing else, and its ops no other route.
    for op in [
        json!({"op":"status"}),
        json!({"op":"sign_in_start"}),
        json!({"op":"sign_in_state"}),
        json!({"op":"sign_in_answer","answer":"yes","request":9}),
    ] {
        assert_eq!(s.app(op.clone())["error"], "not_on_this_route", "{op}");
    }
    for op in [
        json!({"op":"app_sign_in_start"}),
        json!({"op":"app_sign_in_state"}),
        json!({"op":"app_sign_in_answer","id":1,"answer":"yes"}),
    ] {
        assert_eq!(s.trusted(op.clone())["error"], "untrusted", "{op}");
        assert_eq!(s.ask(op.clone())["error"], "untrusted", "{op}");
    }
    // Only the code for now.
    assert_eq!(
        s.app(json!({"op":"app_sign_in_start","method":"qr"}))["error"],
        "bad_request"
    );
    let none = s.app(json!({"op":"app_sign_in_state"}));
    assert_eq!(none["sign_in"], "none");
    assert_eq!(none["revision"], 0);
    let instance = none["instance_id"].as_str().unwrap().to_owned();
    assert_eq!(instance.len(), 32);
    assert!(none.get("store_id").is_none());

    // Started from the app, by its code.
    push(&s, code());
    let started = s.app(json!({"op":"app_sign_in_start","method":"code"}));
    assert_eq!(started["ok"], true, "{started}");
    assert_eq!(started["by"], "app");
    assert_eq!(started["method"], "code");
    assert_eq!(started["sign_in"], "code");
    assert_eq!(started["code"], "12345678");
    assert_eq!(started["expires_in"], 120);
    let expires_at = started["expires_at"].as_u64().unwrap();
    assert!(expires_at > 1_700_000_000_000, "{started}");
    assert_eq!(started["instance_id"], instance.as_str());
    assert_eq!(*s.counts.sign_in_methods.lock().unwrap(), [Method::Code]);
    let id = started["id"].as_u64().unwrap();
    let revision = started["revision"].as_u64().unwrap();
    // A read moves nothing; a start again gives the same one back.
    let read = s.app(json!({"op":"app_sign_in_state"}));
    assert_eq!(read["revision"], revision);
    assert_eq!(read["expires_at"], expires_at);
    assert_eq!(s.app(json!({"op":"app_sign_in_start"}))["id"], id);
    assert_eq!(s.counts.sign_in_starts.load(Ordering::SeqCst), 1);
    // lamp-cloud's put to the backend failed: the app is told, with a new
    // revision, and the sign-in goes on; a put that worked clears it.
    assert_eq!(
        s.app(json!({"op":"app_sign_in_relay","id":id + 1,"delivered":false}))["error"],
        "not_this_sign_in"
    );
    assert_eq!(
        s.trusted(json!({"op":"app_sign_in_relay","id":id,"delivered":false}))["error"],
        "untrusted"
    );
    let failed = s.app(json!({"op":"app_sign_in_relay","id":id,"delivered":false}));
    assert_eq!(failed["sign_in"], "code", "{failed}");
    assert_eq!(failed["error"], "relay_failed");
    assert_eq!(
        failed["message"],
        "The sign-in code couldn't reach the app. Try again."
    );
    assert!(failed["revision"].as_u64().unwrap() > revision);
    let again = s.app(json!({"op":"app_sign_in_relay","id":id,"delivered":false}));
    assert_eq!(
        again["revision"], failed["revision"],
        "the same word is no change"
    );
    let cleared = s.app(json!({"op":"app_sign_in_relay","id":id,"delivered":true}));
    assert!(cleared.get("error").is_none() && cleared.get("message").is_none());
    assert!(cleared["revision"].as_u64().unwrap() > failed["revision"].as_u64().unwrap());
    assert_eq!(cleared["code"], "12345678");
    let revision = cleared["revision"].as_u64().unwrap();

    // The voice: only `in_app`, nothing to say, start or answer.
    let voice = s.trusted(json!({"op":"sign_in_state"}));
    assert_eq!(voice["sign_in"], "in_app", "{voice}");
    assert!(voice.get("code").is_none() && voice.get("email").is_none());
    assert_eq!(
        s.trusted(json!({"op":"sign_in_start"}))["error"],
        "started_in_app"
    );
    let claim = s.trusted(json!({"op":"sign_in_take_code","id":id}));
    assert_eq!(claim["claim"], "not_available");
    assert!(claim.get("code").is_none());

    // The question: asked in Harness, given to the app, answered there.
    push(&s, confirm());
    assert_eq!(
        s.app(json!({"op":"app_sign_in_answer","id":id,"answer":"yes"}))["error"],
        "not_read"
    );
    let asking = s.app(json!({"op":"app_sign_in_state"}));
    assert_eq!(asking["sign_in"], "asking", "{asking}");
    assert_eq!(asking["email"], "someone@example.com");
    assert_eq!(asking["answer_here"], true);
    assert_eq!(asking["answer_within"], 120);
    assert!(asking["revision"].as_u64().unwrap() > revision);
    let voice = s.trusted(json!({"op":"sign_in_state"}));
    assert!(!voice.to_string().contains('@'), "{voice}");
    assert_eq!(
        s.trusted(json!({"op":"sign_in_asked","id":id,"request":3}))["error"],
        "started_in_app"
    );
    assert_eq!(
        s.trusted(json!({"op":"sign_in_answer","answer":"yes","request":4}))["error"],
        "started_in_app"
    );
    assert_eq!(
        s.app(json!({"op":"app_sign_in_answer","id":id + 1,"answer":"yes"}))["error"],
        "not_this_sign_in"
    );
    assert!(s.counts.sign_in_answers.lock().unwrap().is_empty());
    let answered = s.app(json!({"op":"app_sign_in_answer","id":id,"answer":"yes"}));
    assert_eq!(answered["sign_in"], "answered", "{answered}");
    assert_eq!(*s.counts.sign_in_answers.lock().unwrap(), [true]);
    assert_eq!(
        s.app(json!({"op":"app_sign_in_answer","id":id,"answer":"no"}))["error"],
        "not_asking"
    );
    push(
        &s,
        Ok(Event::Result(Done::Success {
            email: Some("someone@example.com".into()),
        })),
    );
    let done = s.app(json!({"op":"app_sign_in_state"}));
    assert_eq!(done["sign_in"], "done");
    assert_eq!(done["result"], "success");
    assert!(done.get("email").is_none());

    // Started by voice: shown in the app with its code, never its email,
    // and never the app's to start over or answer.
    push(&s, code());
    let spoken = s.trusted(json!({"op":"sign_in_start"}));
    assert_eq!(spoken["code"], "12345678");
    let voice_id = spoken["id"].as_u64().unwrap();
    assert_eq!(voice_id, id + 1);
    let shown = s.app(json!({"op":"app_sign_in_state"}));
    assert_eq!(shown["by"], "voice");
    assert_eq!(shown["code"], "12345678");
    assert_eq!(shown["answer_here"], false);
    assert!(shown["revision"].as_u64().unwrap() > done["revision"].as_u64().unwrap());
    assert_eq!(
        s.app(json!({"op":"app_sign_in_start"}))["error"],
        "voice_sign_in_open"
    );
    push(&s, confirm());
    let asking = s.app(json!({"op":"app_sign_in_state"}));
    assert_eq!(asking["sign_in"], "asking");
    assert!(asking.get("email").is_none(), "{asking}");
    assert_eq!(asking["answer_here"], false);
    assert_eq!(
        s.app(json!({"op":"app_sign_in_answer","id":voice_id,"answer":"yes"}))["error"],
        "started_by_voice"
    );
    assert_eq!(s.counts.sign_in_answers.lock().unwrap().len(), 1);
    // The voice's own rules hold: told, then answered in a later request.
    s.trusted(json!({"op":"sign_in_asked","id":voice_id,"request":5}));
    let answered = s.trusted(json!({"op":"sign_in_answer","answer":"no","request":6}));
    assert_eq!(answered["sign_in"], "answered", "{answered}");
    assert_eq!(*s.counts.sign_in_answers.lock().unwrap(), [true, false]);
    push(
        &s,
        Ok(Event::ResultWith {
            done: Done::Error {
                code: "DENIED".into(),
            },
            message: Some("Too many wrong codes. Try again in an hour.".into()),
            already_signed_in: false,
        }),
    );
    let ended = s.app(json!({"op":"app_sign_in_state"}));
    assert_eq!(ended["error"], "DENIED");
    assert_eq!(
        ended["message"],
        "Too many wrong codes. Try again in an hour."
    );
}

/// Backend2 on #5198: a sign-in's id only grows, across a restart of the
/// part too, even if the lamp's clock went back: the newest is kept in the
/// state folder beside the store's and the next one counts up from it.
#[test]
fn a_sign_in_id_grows_across_a_restart_with_the_clock_gone_back() {
    let mut s = Running::unstarted(false, false, false, false);
    s.config.sign_in_by_code = true;
    s.config.app_socket = Some(s.root.path().join("run/app"));
    // This test stands in for lamp-cloud (#5379).
    s.config.app_caller = |_| true;
    s.start();
    let first = s.app(json!({"op":"app_sign_in_start"}))["id"]
        .as_u64()
        .unwrap();
    s.stop();
    let kept = s
        .config
        .dir
        .parent()
        .unwrap()
        .join(lamp_tasks::runtime::SIGN_IN_ID_FILE);
    assert_eq!(fs::read_to_string(&kept).unwrap().trim(), first.to_string());
    s.start();
    let second = s.app(json!({"op":"app_sign_in_start"}))["id"]
        .as_u64()
        .unwrap();
    assert!(second > first, "{second} after {first}");
    s.stop();
    // The clock gone back behind the newest id kept (as a lamp's does
    // before its time is set): the next id is still greater.
    let ahead = second + 10_000_000_000;
    fs::write(&kept, format!("{ahead}\n")).unwrap();
    s.start();
    let third = s.trusted(json!({"op":"sign_in_start"}))["id"]
        .as_u64()
        .unwrap();
    assert_eq!(third, ahead + 1);
}

/// #5524, for the app's Harness row (app #111): the app's state says
/// whether Harness is signed in from the part's own read of Harness, not
/// from the last sign-in it saw, so a restart, with no sign-in open and the
/// view `none`, still says it. Signed out from a computer, it says `no`, and
/// after a restart still `no`; signed in again, `yes`. Without
/// `--set-up-later` nothing is read, and the view is as before.
#[test]
fn the_apps_state_says_harness_signed_in_across_a_restart() {
    let mut s = Running::unstarted(false, false, false, false);
    s.config.sign_in_by_code = true;
    s.config.app_socket = Some(s.root.path().join("run/app"));
    // This test stands in for lamp-cloud (#5379).
    s.config.app_caller = |_| true;
    s.config.set_up_later = Some(SetUpLater {
        targets: s.root.path().join("targets.json"),
        every: Duration::from_millis(20),
        from_machines: None,
        engines: false,
        lamp: None,
    });
    let said_until = |s: &Running, want: &str| {
        let start = Instant::now();
        loop {
            let view = s.app(json!({"op":"app_sign_in_state"}));
            if view["signed_in"] == want {
                return view;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "{view}");
            thread::sleep(Duration::from_millis(10));
        }
    };
    s.start();
    let view = said_until(&s, "yes");
    assert_eq!(view["ok"], true);
    assert_eq!(view["sign_in"], "none");
    s.counts.signed_out.store(true, Ordering::SeqCst);
    said_until(&s, "no");
    s.stop();
    s.start();
    let view = said_until(&s, "no");
    assert_eq!(view["sign_in"], "none", "nothing of the last run's sign-in");
    s.counts.signed_out.store(false, Ordering::SeqCst);
    said_until(&s, "yes");
    s.stop();
    // Without `--set-up-later`: no read, no `signed_in`.
    s.config.set_up_later = None;
    s.start();
    thread::sleep(Duration::from_millis(100));
    let view = s.app(json!({"op":"app_sign_in_state"}));
    assert_eq!(view["sign_in"], "none");
    assert!(view.get("signed_in").is_none(), "{view}");
}
