use lamp_bus::Bus;
use lamp_msgs::{AgentState, Diagnostic, Sign, TaskStatus};
use lamp_tasks::{
    engine::{Clock as Sample, WATCH_EARLY_MS},
    harness::{Closed, Ended, EngineKind, Launch, LaunchOutcome, Listed, Observation, Target},
    progress::SETTLED_FOR_MS,
    runtime::{Adapter, Clock, Config, WaitLane, request, serve_with_adapter},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs, io,
    io::Write,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const AGENT: &str = "11111111-1111-4111-8111-111111111111";
/// The person's own harness on the same machine (#1390).
const THEIRS: &str = "33333333-3333-4333-8333-333333333333";
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
        };
        let mut running = Self {
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
        };
        running.start();
        running
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
    let output = Command::new(env!("CARGO_BIN_EXE_lamp-tasks"))
        .args([
            "serve",
            "--targets",
            "/does-not-exist",
            "--harness",
            "/does-not-exist",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["enabled"], false);
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_lamp-tasks"))
            .args(["serve", "--enable", "--enable"])
            .output()
            .unwrap()
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
    let mut child = command
        .args(["request", "--socket", "/no-tasks-test-socket"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
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
    // it leaves, with no request and no read.
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
        (reads, listings)
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
    let mut s = Running::configured(false, false, true);
    s.stop();
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
