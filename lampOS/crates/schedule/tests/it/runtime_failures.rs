//! Local failure fixtures, not measurements of a stalled disk or lamp timing.
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::{Diagnostic, Level, ScheduleDue};
use lamp_schedule::{
    runtime::{Clock, Config, STALE_AFTER, serve},
    time::ClockSample,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    os::{fd::OwnedFd, unix::fs::PermissionsExt},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(5);
const WALL: i64 = 1_791_964_800_000;
const CLIENT_BOUND: Duration = Duration::from_secs(2);
const STOP_BOUND: Duration = Duration::from_secs(2);

fn private_root() -> tempfile::TempDir {
    let base = if Path::new("/private/tmp").is_dir() {
        Path::new("/private/tmp").to_path_buf()
    } else {
        std::env::temp_dir().canonicalize().unwrap()
    };
    tempfile::Builder::new()
        .prefix("sf-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(base)
        .unwrap()
}

fn config(root: &Path) -> Config {
    Config {
        enabled: true,
        dir: root.join("data"),
        socket: root.join("run/s"),
        bus_root: root.join("b"),
        zone: "Etc/UTC".into(),
        utc_offset_minutes: None,
        run_for: Some(Duration::from_secs(15)),
        // A report every 50 ms, not the lamp's second, so a test that reads
        // it waits a fraction of a second (#1806).
        report_every: Duration::from_millis(50),
        request_deadline: lamp_schedule::runtime::REQUEST_DEADLINE,
    }
}

fn left(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "fixture deadline"))
}

fn ask(path: &Path, request: &Value) -> io::Result<Value> {
    // Only one request is in flight at a time on this isolated fixture socket.
    let mut stream = UnixStream::connect(path)?;
    let deadline = Instant::now() + CLIENT_BOUND;
    let mut bytes = serde_json::to_vec(request)?;
    bytes.push(b'\n');
    let mut sent = 0;
    while sent < bytes.len() {
        stream.set_write_timeout(Some(left(deadline)?))?;
        match stream.write(&bytes[sent..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "fixture peer closed",
                ));
            }
            Ok(n) => sent += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let mut line = Vec::new();
    let mut chunk = [0; 512];
    loop {
        stream.set_read_timeout(Some(left(deadline)?))?;
        let n = match stream.read(&mut chunk) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "missing reply",
                ));
            }
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let end = chunk[..n].iter().position(|b| *b == b'\n');
        line.extend_from_slice(&chunk[..end.unwrap_or(n)]);
        if line.len() > lamp_schedule::protocol::MAX_REPLY_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unbounded reply",
            ));
        }
        if end.is_some() {
            return serde_json::from_slice(&line).map_err(io::Error::other);
        }
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let until = Instant::now() + STOP_BOUND;
        while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < until {
            thread::sleep(POLL);
        }
    }
}

fn filled_stderr() -> (Stdio, UnixStream, usize) {
    let (mut writer, peer) = UnixStream::pair().unwrap();
    writer.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut filled = 0;
    // Finish with one-byte writes so even the first byte of the former
    // per-request eprintln would encounter backpressure.
    for chunk in [&[b'x'; 4096][..], &b"x"[..]] {
        loop {
            assert!(Instant::now() < deadline, "stderr fixture did not fill");
            assert!(filled < 8 * 1024 * 1024, "unexpected stderr buffer size");
            match writer.write(chunk) {
                Ok(0) => panic!("stderr fixture closed while filling"),
                Ok(n) => filled += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => panic!("stderr fixture: {e}"),
            }
        }
    }
    assert!(filled > 0);
    writer.set_nonblocking(false).unwrap();
    (Stdio::from(OwnedFd::from(writer)), peer, filled)
}

#[test]
fn a_full_stderr_sink_cannot_hold_status_or_sigterm_shutdown() {
    let root = private_root();
    let config = config(root.path());
    let (stderr, _unread_peer, filled) = filled_stderr();
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_lamp-schedule"))
            .args([
                "serve",
                "--enable",
                "--trust-clock",
                "--boot-id",
                "stderr-test",
                "--zone",
                "Etc/UTC",
                "--dir",
            ])
            .arg(&config.dir)
            .arg("--socket")
            .arg(&config.socket)
            .arg("--bus-root")
            .arg(&config.bus_root)
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .unwrap(),
    );
    // The socket file appears at `bind`, before `listen`, so wait for the
    // service to answer rather than for the file (#965). Only the attempt that
    // connected is timed.
    let startup = Instant::now() + Duration::from_secs(5);
    let (status, request_elapsed) = loop {
        let request_at = Instant::now();
        match ask(&config.socket, &json!({"op":"status"})) {
            Ok(status) => break (status, request_at.elapsed()),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) => {}
            Err(e) => panic!("status failed: {e}"),
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "CLI exited before listening"
        );
        assert!(Instant::now() < startup, "CLI did not answer on its socket");
        thread::sleep(POLL);
    };
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(status["items"], 0);
    assert!(
        request_elapsed < CLIENT_BOUND,
        "status took {request_elapsed:?}"
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
            stopped_at.elapsed() < STOP_BOUND,
            "SIGTERM did not stop the CLI"
        );
        thread::sleep(POLL);
    };
    assert!(exit.success(), "CLI exited with {exit}");
    assert!(!config.socket.exists(), "owned socket was not cleaned up");
    eprintln!(
        "host fixture: prefilled stderr={filled} bytes; status round trip={request_elapsed:?}; SIGTERM to exit={:?}",
        stopped_at.elapsed()
    );
}

#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Default)]
struct GateState {
    millis: u64,
    blocked: bool,
    entered: bool,
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
        let until = Instant::now() + Duration::from_secs(2);
        while !state.entered {
            let remaining = left(until).expect("store worker never entered the clock gate");
            state = self.0.changed.wait_timeout(state, remaining).unwrap().0;
        }
    }

    fn release(&self) {
        // Cleanup must also release the gate if an earlier assertion poisoned
        // this fixture mutex while checking that the worker had entered it.
        self.0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .blocked = false;
        self.0.changed.notify_all();
    }
}

impl Clock for ControlledClock {
    fn sample(&self) -> io::Result<ClockSample> {
        let mut state = self.0.state.lock().unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while state.blocked {
            state.entered = true;
            self.0.changed.notify_all();
            // A broken test must not leave an indefinitely blocked owner.
            state = self.0.changed.wait_timeout(state, left(until)?).unwrap().0;
        }
        Ok(ClockSample {
            unix_ms: WALL + i64::try_from(state.millis).unwrap(),
            boot_ms: state.millis,
            boot_id: "blocked-clock-test".into(),
            wall_trusted: true,
        })
    }
}

struct ServiceGuard {
    stop: Arc<AtomicBool>,
    clock: ControlledClock,
    handle: Option<JoinHandle<io::Result<()>>>,
}

impl ServiceGuard {
    fn start(config: Config, clock: ControlledClock) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let owner_stop = stop.clone();
        let owner_clock = clock.clone();
        Self {
            stop,
            clock,
            handle: Some(thread::spawn(move || {
                serve(config, owner_clock, owner_stop)
            })),
        }
    }

    fn stop_and_join(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.clock.release();
        let until = Instant::now() + STOP_BOUND;
        let handle = self.handle.as_ref().unwrap();
        while !handle.is_finished() {
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
            let until = Instant::now() + STOP_BOUND;
            while !handle.is_finished() && Instant::now() < until {
                thread::sleep(POLL);
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

fn receive_due(
    subscriber: &mut Subscriber,
    bound: Duration,
    predicate: impl Fn(&ScheduleDue) -> bool,
) -> ScheduleDue {
    let until = Instant::now() + bound;
    while Instant::now() < until {
        if let Some(message) = subscriber.recv(Duration::from_millis(25)).unwrap() {
            let state: ScheduleDue = message.parse().unwrap();
            state.check().unwrap();
            if predicate(&state) {
                return state;
            }
        }
    }
    panic!("expected ScheduleDue did not arrive in {bound:?}");
}

fn receive_diagnostic(subscriber: &mut Subscriber, ready: bool) -> Diagnostic {
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        if let Some(message) = subscriber.recv(Duration::from_millis(25)).unwrap() {
            let report: Diagnostic = message.parse().unwrap();
            if report.name == "schedule"
                && report
                    .values
                    .iter()
                    .any(|entry| entry.key == "store_ready" && entry.value == ready.to_string())
            {
                return report;
            }
        }
    }
    panic!("Schedule diagnostic with store_ready={ready} did not arrive");
}

#[test]
fn a_blocked_owner_withdraws_due_readiness_then_recovers_the_same_delivery() {
    let root = private_root();
    let config = config(root.path());
    let bus = Bus::new(&config.bus_root).unwrap();
    let mut due = bus
        .subscribe_typed(ScheduleDue::TOPIC, "sf_due", ScheduleDue::TYPE)
        .unwrap();
    let mut diagnostics = bus
        .subscribe_typed(Diagnostic::TOPIC, "sf_diag", Diagnostic::TYPE)
        .unwrap();
    let clock = ControlledClock::default();
    let mut service = ServiceGuard::start(config.clone(), clock.clone());
    receive_due(&mut due, Duration::from_secs(5), |state| state.ready);
    let status = ask(&config.socket, &json!({"op":"status"})).unwrap();
    let created = ask(
        &config.socket,
        &json!({
            "op":"set",
            "guard":{"store_id":status["store_id"],"revision":status["revision"],"request_id":"liveness"},
            "text":"Private liveness fixture",
            "when":{"kind":"after","seconds":1}
        }),
    )
    .unwrap();
    assert_eq!(created["ok"], true, "{created}");
    clock.advance(2000);
    let before = receive_due(&mut due, Duration::from_secs(3), |state| {
        state.ready && state.pending == 1
    });
    let original = before.oldest.unwrap();
    let queued_request = json!({
        "op":"set",
        "guard":{"store_id":before.store_id,"revision":before.revision,"request_id":"queued-expiry"},
        "text":"This abandoned request must not become a reminder",
        "when":{"kind":"after","seconds":60}
    });

    clock.block_worker();
    let blocked_at = Instant::now();
    // The worker is inside its periodic clock sample, before it can receive
    // this complete request. Its caller deadline expires while still queued.
    let abandoned = ask(&config.socket, &queued_request).unwrap();
    let receipt_elapsed = blocked_at.elapsed();
    assert_eq!(abandoned["ok"], false, "{abandoned}");
    assert_eq!(abandoned["error"], "uncertain", "{abandoned}");
    assert!(receipt_elapsed < CLIENT_BOUND);
    let stale = receive_due(&mut due, STALE_AFTER + Duration::from_secs(2), |state| {
        !state.ready
    });
    let stale_elapsed = blocked_at.elapsed();
    assert!(stale.enabled);
    assert_eq!(stale.pending, 1);
    assert_eq!(stale.revision, before.revision);
    assert_eq!(stale.store_id, before.store_id);
    assert!(stale.oldest.is_none());
    assert!(stale.stamp.sequence > before.stamp.sequence);
    let failure = receive_diagnostic(&mut diagnostics, false);
    assert_eq!(failure.level, Level::Error);
    assert!(failure.message.contains("not progressing"));
    assert!(
        !serde_json::to_string(&failure)
            .unwrap()
            .contains("Private liveness")
    );

    clock.advance(3000);
    let resumed_at = Instant::now();
    clock.release();
    let resumed = receive_due(&mut due, Duration::from_secs(3), |state| {
        state.ready && state.stamp.sequence > stale.stamp.sequence
    });
    let resumed_elapsed = resumed_at.elapsed();
    let notice = resumed.oldest.unwrap();
    assert_eq!(resumed.pending, 1);
    assert_eq!(resumed.revision, before.revision);
    assert_eq!(resumed.store_id, before.store_id);
    assert!(resumed.stamp.at_us > stale.stamp.at_us);
    assert_eq!(notice.id, original.id);
    assert_eq!(notice.delivery, original.delivery);
    assert_eq!(notice.due_unix_ms, original.due_unix_ms);
    let healthy = receive_diagnostic(&mut diagnostics, true);
    assert_eq!(healthy.level, Level::Ok);
    let status = ask(&config.socket, &json!({"op":"status"})).unwrap();
    assert_eq!(status["ok"], true, "{status}");
    assert_eq!(
        status["items"], 1,
        "expired queued Set was executed: {status}"
    );
    assert_eq!(status["pending"], 1);
    assert_eq!(status["revision"], before.revision);
    assert_eq!(
        ask(&config.socket, &json!({"op":"get","id":notice.id})).unwrap()["item"]["delivery"],
        notice.delivery
    );
    service.stop_and_join();
    assert!(!config.socket.exists());
    eprintln!(
        "host injected-clock fixture: blocked worker to uncertain receipt={receipt_elapsed:?}; blocked worker to unavailable due={stale_elapsed:?}; release to ready due={resumed_elapsed:?}; token=({}, {})",
        notice.id, notice.delivery
    );
}
