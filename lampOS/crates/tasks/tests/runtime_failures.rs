//! Local failure fixtures. Injected stalls and authored deadlines do not prove
//! lamp storage latency, installed Harness interoperability or acoustic timing.
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::{Diagnostic, Level};
use lamp_tasks::{
    engine::Clock as Sample,
    harness::{EngineKind, Launch, LaunchOutcome, Observation, Target},
    runtime::{Adapter, Clock, Config, REQUEST_DEADLINE, request, serve_with_adapter},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    os::{
        fd::OwnedFd,
        unix::{fs::PermissionsExt, net::UnixStream},
    },
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(5);
const FIXTURE_BOUND: Duration = Duration::from_secs(5);

fn private_root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("tf-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(fs::canonicalize("/tmp").unwrap())
        .unwrap()
}

fn config(root: &Path) -> Config {
    Config {
        enabled: true,
        dir: root.join("data"),
        socket: root.join("r/p"),
        voice_socket: root.join("r/v"),
        bus_root: root.join("b"),
        targets: BTreeMap::from([(
            "desk".into(),
            Target {
                machine_id: "0123456789abcdef0123456789abcdef".into(),
                project: "fixture-project".into(),
                engine: EngineKind::Codex,
            },
        )]),
        run_for: Some(Duration::from_secs(20)),
        watch: false,
        progress: false,
        push_wait: false,
        close_told: false,
    }
}

fn left(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "fixture deadline"))
}

fn ask(path: &Path, value: &Value) -> Value {
    request(path, value, FIXTURE_BOUND).unwrap()
}

fn receive_diagnostic(subscriber: &mut Subscriber, ready: bool) -> Diagnostic {
    let until = Instant::now() + FIXTURE_BOUND;
    while Instant::now() < until {
        if let Some(message) = subscriber.recv(Duration::from_millis(25)).unwrap() {
            let report: Diagnostic = message.parse().unwrap();
            if report.name == "tasks"
                && report
                    .values
                    .iter()
                    .any(|entry| entry.key == "store_ready" && entry.value == ready.to_string())
            {
                return report;
            }
        }
    }
    panic!("Tasks diagnostic with store_ready={ready} did not arrive");
}

#[derive(Default)]
struct GateState {
    millis: u64,
    blocked: bool,
    entered: bool,
}
#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}
#[derive(Clone, Default)]
struct ControlledClock(Arc<Gate>);

impl ControlledClock {
    fn advance(&self, millis: u64) {
        self.0.state.lock().unwrap().millis = millis;
    }
    fn block_worker(&self) {
        let mut state = self.0.state.lock().unwrap();
        state.blocked = true;
        state.entered = false;
        let until = Instant::now() + FIXTURE_BOUND;
        while !state.entered {
            state = self
                .0
                .changed
                .wait_timeout(
                    state,
                    left(until).expect("owner never entered its clock gate"),
                )
                .unwrap()
                .0;
        }
    }
    fn release(&self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .blocked = false;
        self.0.changed.notify_all();
    }
}
impl Clock for ControlledClock {
    fn sample(&self) -> io::Result<Sample> {
        let mut state = self.0.state.lock().unwrap();
        let until = Instant::now() + Duration::from_secs(15);
        while state.blocked {
            state.entered = true;
            self.0.changed.notify_all();
            state = self.0.changed.wait_timeout(state, left(until)?).unwrap().0;
        }
        Ok(Sample {
            boot_id: "blocked-owner".into(),
            monotonic_ms: state.millis,
        })
    }
}

struct UncalledAdapter(Arc<AtomicUsize>);
impl Adapter for UncalledAdapter {
    fn launch(&self, _: &Launch) -> LaunchOutcome {
        self.0.fetch_add(1, Ordering::Relaxed);
        LaunchOutcome::Uncertain {
            code: "unexpected_fixture_call".into(),
        }
    }
    fn observe(&self, _: &str, _: &str) -> Result<Observation, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err("unexpected_fixture_call".into())
    }
}

struct ServiceGuard {
    stop: Arc<AtomicBool>,
    clock: ControlledClock,
    calls: Arc<AtomicUsize>,
    handle: Option<JoinHandle<io::Result<()>>>,
}
impl ServiceGuard {
    fn start(config: Config, clock: ControlledClock) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let owner_stop = stop.clone();
        let owner_clock = clock.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let adapter_calls = calls.clone();
        Self {
            stop,
            clock,
            calls,
            handle: Some(thread::spawn(move || {
                serve_with_adapter(config, owner_clock, owner_stop, move |_| {
                    Ok(UncalledAdapter(adapter_calls))
                })
            })),
        }
    }
    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.clock.release();
        let until = Instant::now() + FIXTURE_BOUND;
        while !self.handle.as_ref().unwrap().is_finished() {
            assert!(Instant::now() < until, "fixture service did not stop");
            thread::sleep(POLL);
        }
        self.handle.take().unwrap().join().unwrap().unwrap();
    }
}
impl Drop for ServiceGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.clock.release();
        if let Some(handle) = self.handle.take() {
            let until = Instant::now() + FIXTURE_BOUND;
            while !handle.is_finished() && Instant::now() < until {
                thread::sleep(POLL);
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

#[test]
fn blocked_owner_expires_queued_preparation_and_withdraws_then_restores_readiness() {
    let root = private_root();
    let config = config(root.path());
    let bus = Bus::new(&config.bus_root).unwrap();
    let mut diagnostics = bus
        .subscribe_typed(Diagnostic::TOPIC, "tf_diag", Diagnostic::TYPE)
        .unwrap();
    let clock = ControlledClock::default();
    let mut service = ServiceGuard::start(config.clone(), clock.clone());
    receive_diagnostic(&mut diagnostics, true);
    let status = ask(&config.socket, &json!({"op":"status"}));
    let first = ask(
        &config.socket,
        &json!({
            "op":"prepare","guard":{"store_id":status["store_id"],"revision":status["revision"],"request_id":"retained"},
            "text":"Private retained fixture task","target":"desk"
        }),
    );
    assert_eq!(first["ok"], true, "{first}");
    let before = ask(&config.socket, &json!({"op":"status"}));
    let abandoned = json!({
        "op":"prepare","guard":{"store_id":before["store_id"],"revision":before["revision"],"request_id":"queued-expiry"},
        "text":"Private abandoned fixture task must not dispatch","target":"desk"
    });
    clock.block_worker();
    let blocked_at = Instant::now();
    // The periodic sample is already blocked before this request is enqueued.
    // Its deadline is real elapsed time, independent of the injected boot clock.
    let reply = ask(&config.socket, &abandoned);
    let receipt_elapsed = blocked_at.elapsed();
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["error"], "uncertain", "{reply}");
    assert!(receipt_elapsed >= REQUEST_DEADLINE);
    let unavailable = receive_diagnostic(&mut diagnostics, false);
    let stale_elapsed = blocked_at.elapsed();
    assert_eq!(unavailable.level, Level::Error);
    assert!(
        unavailable
            .values
            .iter()
            .any(|entry| entry.key == "retained" && entry.value == "1")
    );
    let diagnostic_text = serde_json::to_string(&unavailable).unwrap();
    assert!(!diagnostic_text.contains("Private retained"));
    assert!(!diagnostic_text.contains("Private abandoned"));

    clock.advance(1000);
    let released_at = Instant::now();
    clock.release();
    let healthy = receive_diagnostic(&mut diagnostics, true);
    let recover_elapsed = released_at.elapsed();
    assert_eq!(healthy.level, Level::Ok);
    let after = ask(&config.socket, &json!({"op":"status"}));
    assert_eq!(
        after["summary"]["retained"], 1,
        "expired queued Prepare executed: {after}"
    );
    assert_eq!(after["revision"], before["revision"]);
    assert_eq!(after["last_receipt"]["request_id"], "retained");
    let task = ask(&config.socket, &json!({"op":"get_task","id":first["id"]}));
    assert_eq!(task["text"], "Private retained fixture task");
    assert_eq!(task["task"]["dispatch"]["state"], "prepared");
    assert_eq!(service.calls.load(Ordering::Relaxed), 0);
    service.stop_and_join();
    assert!(!config.socket.exists());
    assert!(!config.voice_socket.exists());
    eprintln!(
        "host injected-clock fixture: blocked owner to uncertain receipt={receipt_elapsed:?}; blocked owner to unavailable diagnostic={stale_elapsed:?}; release to ready diagnostic={recover_elapsed:?}; no CLI calls"
    );
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let until = Instant::now() + FIXTURE_BOUND;
        while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < until {
            thread::sleep(POLL);
        }
    }
}

fn filled_stderr() -> (Stdio, UnixStream, usize) {
    let (mut writer, reader) = UnixStream::pair().unwrap();
    writer.set_nonblocking(true).unwrap();
    let until = Instant::now() + FIXTURE_BOUND;
    let mut filled = 0;
    for chunk in [&[b'x'; 4096][..], &[b'x'][..]] {
        loop {
            assert!(Instant::now() < until, "stderr did not fill");
            assert!(filled < 8 * 1024 * 1024, "unexpected stderr buffer size");
            match writer.write(chunk) {
                Ok(0) => panic!("stderr fixture closed while filling"),
                Ok(n) => filled += n,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => panic!("stderr fixture: {error}"),
            }
        }
    }
    assert!(filled > 0);
    writer.set_nonblocking(false).unwrap();
    (Stdio::from(OwnedFd::from(writer)), reader, filled)
}

fn start_production(root: &Path, config: &Config) -> (ChildGuard, UnixStream, usize) {
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(
        home.join("plan.json"),
        br#"{"machines":{"default":true},"new":{"default":true},"show":{"default":true}}"#,
    )
    .unwrap();
    let targets = root.join("targets.json");
    fs::write(
        &targets,
        serde_json::to_vec(&json!({"version":1,"targets":config.targets})).unwrap(),
    )
    .unwrap();
    fs::set_permissions(&targets, fs::Permissions::from_mode(0o600)).unwrap();
    // Cargo writes the fixture under the builder's umask, and under 0002 it is
    // group-writable, which the Harness adapter rightly refuses.
    let fixture = env!("CARGO_BIN_EXE_lamp-tasks-harness-fixture");
    let mode = fs::metadata(fixture).unwrap().permissions().mode();
    if mode & 0o022 != 0 {
        fs::set_permissions(fixture, fs::Permissions::from_mode(mode & !0o022)).unwrap();
    }
    let (stderr, unread_reader, filled) = filled_stderr();
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_lamp-tasks"))
            .args(["serve", "--enable", "--boot-id", "stderr-test", "--dir"])
            .arg(&config.dir)
            .arg("--socket")
            .arg(&config.socket)
            .arg("--voice-socket")
            .arg(&config.voice_socket)
            .arg("--bus-root")
            .arg(&config.bus_root)
            .arg("--targets")
            .arg(&targets)
            .arg("--harness")
            .arg(fixture)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + FIXTURE_BOUND;
    while !config.socket.exists() || !config.voice_socket.exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "Tasks exited before binding"
        );
        assert!(
            Instant::now() < until,
            "Tasks did not bind its fixture sockets"
        );
        thread::sleep(POLL);
    }
    (child, unread_reader, filled)
}

#[test]
fn full_unread_stderr_cannot_block_production_status_or_sigterm_cleanup() {
    let root = private_root();
    let config = config(root.path());
    let home = root.path().join("home");
    let (mut child, _unread_reader, filled) = start_production(root.path(), &config);
    let began = Instant::now();
    let status = ask(&config.socket, &json!({"op":"status"}));
    let status_elapsed = began.elapsed();
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(status["summary"]["retained"], 0);
    let prepared = ask(
        &config.socket,
        &json!({
            "op":"prepare","guard":{"store_id":status["store_id"],"revision":status["revision"],"request_id":"stderr-prepare"},
            "text":"This fixture is never confirmed","target":"desk"
        }),
    );
    assert_eq!(prepared["ok"], true, "{prepared}");
    let status = ask(&config.voice_socket, &json!({"op":"status"}));
    assert_eq!(status["summary"]["retained"], 1);
    assert_eq!(status["summary"]["prepared"], 1);
    assert!(
        !home.join("calls.jsonl").exists(),
        "unconfirmed preparation invoked the CLI"
    );
    let stopped_at = Instant::now();
    rustix::process::kill_process(
        rustix::process::Pid::from_child(&child.0),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    let exit = loop {
        if let Some(exit) = child.0.try_wait().unwrap() {
            break exit;
        }
        assert!(
            stopped_at.elapsed() < FIXTURE_BOUND,
            "SIGTERM did not stop Tasks"
        );
        thread::sleep(POLL);
    };
    assert!(exit.success(), "Tasks exited with {exit}");
    assert!(!config.socket.exists());
    assert!(!config.voice_socket.exists());
    assert!(!home.join("calls.jsonl").exists());
    eprintln!(
        "host fixture: prefilled stderr={filled} bytes; status round trip={status_elapsed:?}; SIGTERM to exit={:?}; no CLI calls",
        stopped_at.elapsed()
    );
}

#[test]
fn full_unread_stderr_cannot_hold_storage_failure_exit_or_socket_cleanup() {
    let root = private_root();
    let config = config(root.path());
    let home = root.path().join("home");
    let (mut child, _unread_reader, filled) = start_production(root.path(), &config);
    let status = ask(&config.socket, &json!({"op":"status"}));
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(status["summary"]["retained"], 0);
    assert!(!home.join("calls.jsonl").exists());

    // Replace only this test's database inode. The already-open SQLite handle
    // still refers to the original; Store must reject the path identity before
    // writing the next prepared task, and the service must fail closed.
    let database = config.dir.join("tasks.sqlite3");
    fs::rename(&database, config.dir.join("original.sqlite3")).unwrap();
    let replacement = b"private fixture replacement; must remain untouched";
    fs::write(&database, replacement).unwrap();
    fs::set_permissions(&database, fs::Permissions::from_mode(0o600)).unwrap();
    let failed_at = Instant::now();
    let refusal = ask(
        &config.socket,
        &json!({
            "op":"prepare","guard":{"store_id":status["store_id"],"revision":status["revision"],"request_id":"storage-failure"},
            "text":"This fixture must never become a dispatch","target":"desk"
        }),
    );
    assert_eq!(refusal["ok"], false, "{refusal}");
    assert_eq!(refusal["error"], "storage", "{refusal}");
    let exit = loop {
        if let Some(exit) = child.0.try_wait().unwrap() {
            break exit;
        }
        assert!(
            failed_at.elapsed() < FIXTURE_BOUND,
            "full stderr held the process after the storage failure"
        );
        thread::sleep(POLL);
    };
    assert!(!exit.success(), "storage failure exited successfully");
    assert_eq!(exit.code(), Some(1));
    assert!(
        !config.socket.exists(),
        "public socket survived failed shutdown"
    );
    assert!(
        !config.voice_socket.exists(),
        "trusted socket survived failed shutdown"
    );
    assert_eq!(fs::read(&database).unwrap(), replacement);
    assert!(!home.join("calls.jsonl").exists());
    eprintln!(
        "host storage-failure fixture: prefilled stderr={filled} bytes; Prepare start to error exit={:?}; both sockets removed; replacement untouched; no CLI calls",
        failed_at.elapsed()
    );
}
