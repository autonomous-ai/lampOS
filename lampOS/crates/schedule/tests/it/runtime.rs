use lamp_bus::Bus;
use lamp_msgs::{Diagnostic, Level, ScheduleDue};
use lamp_schedule::{
    runtime::{Clock, Config, SystemClock, serve},
    time::ClockSample,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixStream,
    },
    path::Path,
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc, Mutex, PoisonError, RwLock, RwLockReadGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WALL: i64 = 1_791_964_800_000;
#[derive(Clone)]
struct Injected(Arc<Mutex<ClockSample>>);
impl Clock for Injected {
    fn sample(&self) -> io::Result<ClockSample> {
        Ok(self.0.lock().unwrap().clone())
    }
}
fn sample(ms: u64) -> ClockSample {
    ClockSample {
        unix_ms: WALL + ms as i64,
        boot_ms: ms,
        boot_id: "test-boot".into(),
        wall_trusted: true,
    }
}

/// A spawn and a test letting go of a schedule directory it takes again
/// take turns (#1569 row 27). A child spawned by one test thread holds a
/// copy of every descriptor, the directory's lock among them, until its
/// exec closes the close-on-exec ones, and `spawn` returns only after that;
/// a service started again on the same directory in that moment finds it
/// `already owned`. So a spawn takes the turn alone ([`spawned`]), and
/// letting go of a store shares it ([`Running::stop`], and the finite
/// service in `a_finite_service_stops_and_releases_its_owner`): no child
/// spawned before can still hold what was let go. It was shown in
/// lamp-tasks' `tests/runtime.rs`; here it failed so in two workspace
/// sweeps on this Mac, and this file runs too briefly for a rate to show
/// alone. nextest, a process for each test, is not affected.
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

struct Running {
    root: tempfile::TempDir,
    config: Config,
    clock: Injected,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<io::Result<()>>>,
}
impl Running {
    fn new() -> Self {
        Self::in_zone("Etc/UTC")
    }
    fn in_zone(zone: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix("sch-")
            .tempdir_in("/private/tmp")
            .or_else(|_| tempfile::Builder::new().prefix("sch-").tempdir())
            .unwrap();
        let config = Config {
            enabled: true,
            dir: root.path().join("data"),
            socket: root.path().join("run/s.sock"),
            bus_root: root.path().join("b"),
            zone: zone.into(),
            utc_offset_minutes: None,
            run_for: None,
            // A report every 50 ms, not the lamp's second, so a test that
            // reads it waits a fraction of a second (#1806).
            report_every: Duration::from_millis(50),
            request_deadline: lamp_schedule::runtime::REQUEST_DEADLINE,
        };
        let clock = Injected(Arc::new(Mutex::new(sample(0))));
        let stop = Arc::new(AtomicBool::new(false));
        let mut s = Self {
            root,
            config,
            clock,
            stop,
            handle: None,
        };
        s.start();
        s
    }
    fn start(&mut self) {
        self.stop.store(false, Ordering::Release);
        let config = self.config.clone();
        let clock = self.clock.clone();
        let stop = self.stop.clone();
        self.handle = Some(thread::spawn(move || serve(config, clock, stop)));
        let began = Instant::now();
        loop {
            if let Ok(answer) = ask(&self.config.socket, &json!({"op":"status"}))
                && answer["ok"] == true
            {
                break;
            }
            if self.handle.as_ref().unwrap().is_finished() {
                panic!(
                    "service start failed: {:?}",
                    self.handle.take().unwrap().join().unwrap()
                );
            }
            assert!(
                began.elapsed() < Duration::from_secs(5),
                "Schedule did not become ready"
            );
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
    fn advance(&self, ms: u64) {
        *self.clock.0.lock().unwrap() = sample(ms);
    }
    fn ask(&self, request: Value) -> Value {
        ask(&self.config.socket, &request).unwrap()
    }
    fn guard(&self) -> Value {
        let s = self.ask(json!({"op":"status"}));
        json!({"store_id":s["store_id"],"revision":s["revision"],"request_id":format!("r{}",s["revision"])})
    }
    fn set(&self, seconds: u64) -> Value {
        let r=self.ask(json!({"op":"set","guard":self.guard(),"text":"Private reminder words","when":{"kind":"after","seconds":seconds}}));
        assert_eq!(r["ok"], true, "{r}");
        r
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}
fn ask(path: &Path, request: &Value) -> io::Result<Value> {
    let mut client = UnixStream::connect(path)?;
    client.set_read_timeout(Some(Duration::from_secs(2)))?;
    client.set_write_timeout(Some(Duration::from_secs(2)))?;
    writeln!(client, "{request}")?;
    let mut line = String::new();
    BufReader::new(client).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(io::Error::other)
}
fn wait_for(s: &Running, predicate: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        let status = s.ask(json!({"op":"status"}));
        if predicate(&status) {
            return status;
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        thread::sleep(Duration::from_millis(5));
    }
}
fn receive_due(bus: &Bus, predicate: impl Fn(&ScheduleDue) -> bool) -> ScheduleDue {
    let mut subscriber = bus
        .subscribe_typed(ScheduleDue::TOPIC, "schedule_test", ScheduleDue::TYPE)
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(message) = subscriber.recv(Duration::from_millis(50)).unwrap() {
            let state: ScheduleDue = message.parse().unwrap();
            state.check().unwrap();
            if predicate(&state) {
                return state;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "expected due snapshot did not arrive"
        );
    }
}

#[test]
fn disabled_service_never_reads_clock_or_creates_any_path() {
    struct Unreadable;
    impl Clock for Unreadable {
        fn sample(&self) -> io::Result<ClockSample> {
            panic!("disabled clock read");
        }
    }
    let root = tempfile::tempdir().unwrap();
    let config = Config {
        enabled: false,
        dir: root.path().join("data"),
        socket: root.path().join("run/s"),
        bus_root: root.path().join("bus"),
        zone: "Bad/Zone".into(),
        utc_offset_minutes: None,
        run_for: None,
        report_every: Duration::from_millis(50),
        request_deadline: lamp_schedule::runtime::REQUEST_DEADLINE,
    };
    serve(config, Unreadable, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}
#[test]
fn socket_is_private_and_request_receipt_survives_service_restart() {
    let mut s = Running::new();
    let first = s.set(600);
    assert_eq!(
        fs::metadata(&s.config.socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
    s.stop();
    assert!(!s.config.socket.exists());
    s.advance(1000);
    s.start();
    let entry = s.ask(json!({"op":"get","id":first["id"]}));
    assert_eq!(entry["item"]["remaining_ms"], 599_000);
}
#[test]
fn bus_recovery_is_from_durable_pending_work_not_just_the_latest_file() {
    let mut s = Running::new();
    s.set(1);
    s.stop();
    fs::remove_dir_all(&s.config.bus_root).unwrap();
    s.advance(4000);
    s.start();
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let state = receive_due(&bus, |d| d.pending == 1);
    assert_eq!(state.oldest.as_ref().unwrap().delivery, 1);
    assert!(state.oldest.as_ref().unwrap().late);
    let bytes = serde_json::to_string(&state).unwrap();
    assert!(!bytes.contains("Private reminder words"));
    let another = receive_due(&bus, |d| d.pending == 1);
    assert_eq!(another.oldest.unwrap().id, 1);
}
#[test]
fn cancellation_and_ack_update_the_retained_notice() {
    let s = Running::new();
    s.set(1);
    s.set(2);
    s.advance(3000);
    wait_for(&s, |v| v["pending"] == 2);
    let bus = Bus::new(&s.config.bus_root).unwrap();
    receive_due(&bus, |d| d.pending == 2);
    assert_eq!(
        s.ask(json!({"op":"cancel","guard":s.guard(),"id":1}))["ok"],
        true
    );
    let state = receive_due(&bus, |d| d.pending == 1);
    assert_eq!(state.oldest.unwrap().id, 2);
    assert_eq!(
        s.ask(json!({"op":"ack","guard":s.guard(),"id":2,"delivery":1}))["ok"],
        true
    );
    let state = receive_due(&bus, |d| d.pending == 0);
    assert!(state.oldest.is_none());
}
#[test]
fn a_silent_connection_does_not_hold_due_work_or_other_clients() {
    let s = Running::new();
    s.set(1);
    let _silent = UnixStream::connect(&s.config.socket).unwrap();
    s.advance(2000);
    let start = Instant::now();
    let status = wait_for(&s, |v| v["pending"] == 1);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(status["items"], 1);
    receive_due(&Bus::new(&s.config.bus_root).unwrap(), |d| d.pending == 1);
}
#[test]
fn a_trickling_client_has_a_cumulative_read_deadline() {
    let s = Running::new();
    let mut client = UnixStream::connect(&s.config.socket).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let start = Instant::now();
    client.write_all(b"{").unwrap();
    for _ in 0..3 {
        thread::sleep(Duration::from_millis(70));
        client.write_all(b" ").unwrap();
    }
    let mut line = String::new();
    BufReader::new(client).read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(reply["error"], "incomplete");
    assert!(start.elapsed() < Duration::from_secs(1));
}
#[test]
fn malformed_oversized_and_partial_lines_never_mutate_the_store() {
    let s = Running::new();
    for bytes in [
        b"{bad}\n".to_vec(),
        [vec![b'x'; 8193], b"\n".to_vec()].concat(),
        b"{\"op\":\"status\",\"extra\":1}\n".to_vec(),
    ] {
        let mut client = UnixStream::connect(&s.config.socket).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let _ = client.write_all(&bytes);
        let mut line = String::new();
        BufReader::new(client).read_line(&mut line).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["ok"], false);
    }
    let mut partial = UnixStream::connect(&s.config.socket).unwrap();
    partial.write_all(b"{\"op\":\"set\"").unwrap();
    drop(partial);
    assert_eq!(s.ask(json!({"op":"status"}))["items"], 0);
}
#[test]
fn two_guarded_mutations_from_the_same_snapshot_do_not_both_win() {
    let s = Running::new();
    let guard = s.guard();
    let path = s.config.socket.clone();
    let guard2 = guard.clone();
    let first = thread::spawn(move || {
        ask(
            &path,
            &json!({"op":"set","guard":guard2,"text":"One","when":{"kind":"after","seconds":1}}),
        )
        .unwrap()
    });
    let second =
        s.ask(json!({"op":"set","guard":guard,"text":"Two","when":{"kind":"after","seconds":1}}));
    let first = first.join().unwrap();
    assert_ne!(first["ok"], second["ok"]);
    assert_eq!(s.ask(json!({"op":"status"}))["items"], 1);
}
#[test]
fn a_full_client_pool_does_not_stop_due_publication() {
    let s = Running::new();
    s.set(1);
    let mut clients = Vec::new();
    for _ in 0..16 {
        if let Ok(client) = UnixStream::connect(&s.config.socket) {
            clients.push(client);
        }
    }
    s.advance(2000);
    let state = receive_due(&Bus::new(&s.config.bus_root).unwrap(), |d| d.pending == 1);
    assert!(state.ready);
    drop(clients);
    wait_for(&s, |v| v["pending"] == 1);
}
#[test]
fn diagnostics_name_the_owner_and_do_not_include_personal_text() {
    let s = Running::new();
    s.set(1);
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let mut sub = bus
        .subscribe_typed(Diagnostic::TOPIC, "diag_test", Diagnostic::TYPE)
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(message) = sub.recv(Duration::from_millis(100)).unwrap() {
            let report: Diagnostic = message.parse().unwrap();
            assert_eq!(report.name, "schedule");
            assert!(
                !serde_json::to_string(&report)
                    .unwrap()
                    .contains("Private reminder")
            );
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}
/// #1562's follow-up: while a store set aside from a newer build is still
/// in Schedule's folder, its health warns and names it, as lamp-tasks' does
/// (#1554); removed, the warning goes.
#[test]
fn a_store_set_aside_is_said_in_health_until_it_is_gone() {
    let s = Running::new();
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let mut sub = bus
        .subscribe_typed(Diagnostic::TOPIC, "diag_set_aside", Diagnostic::TYPE)
        .unwrap();
    let kept = s.config.dir.join("schedule.sqlite3.newer-2-1791964800");
    fs::write(&kept, b"kept").unwrap();
    let said = |sub: &mut lamp_bus::Subscriber, warned: bool| {
        let start = Instant::now();
        loop {
            if let Some(message) = sub.recv(Duration::from_millis(100)).unwrap() {
                let report: Diagnostic = message.parse().unwrap();
                let named = report.values.iter().any(|v| {
                    v.key == "set_aside" && v.value == "schedule.sqlite3.newer-2-1791964800"
                });
                if named == warned {
                    return report;
                }
            }
            assert!(start.elapsed() < Duration::from_secs(5), "never {warned}");
        }
    };
    let report = said(&mut sub, true);
    assert_eq!(report.level, Level::Warn);
    assert!(
        report.message.contains("will not sound"),
        "{}",
        report.message
    );
    fs::remove_file(&kept).unwrap();
    let report = said(&mut sub, false);
    assert!(!report.message.contains("set aside"), "{}", report.message);
}
/// The first health report that `wanted` takes, within five seconds.
fn health_until(s: &Running, wanted: impl Fn(&Diagnostic) -> bool) -> Diagnostic {
    let bus = Bus::new(&s.config.bus_root).unwrap();
    let mut sub = bus
        .subscribe_typed(Diagnostic::TOPIC, "diag_until", Diagnostic::TYPE)
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(message) = sub.recv(Duration::from_millis(100)).unwrap() {
            let report: Diagnostic = message.parse().unwrap();
            if wanted(&report) {
                return report;
            }
        }
        assert!(start.elapsed() < Duration::from_secs(5), "never said");
    }
}

fn said(report: &Diagnostic, key: &str) -> Option<String> {
    report
        .values
        .iter()
        .find(|pair| pair.key == key)
        .map(|pair| pair.value.clone())
}

/// The store's snapshot, read and written by hand as a newer build would.
fn stored(s: &Running) -> Value {
    let db = rusqlite::Connection::open(s.config.dir.join("schedule.sqlite3")).unwrap();
    let snapshot: Vec<u8> = db
        .query_row("SELECT snapshot FROM state", [], |row| row.get(0))
        .unwrap();
    serde_json::from_slice(&snapshot).unwrap()
}

fn store(s: &Running, state: &Value) {
    rusqlite::Connection::open(s.config.dir.join("schedule.sqlite3"))
        .unwrap()
        .execute(
            "UPDATE state SET snapshot = ?1",
            [serde_json::to_vec(state).unwrap()],
        )
        .unwrap();
}

/// #1680: a reminder with a value this build doesn't know, at the same
/// `schema`, doesn't stop the part: it is kept in the store as it is, the
/// part serves, and health counts it as a warning.
#[test]
fn a_stored_reminder_this_build_cannot_read_is_kept_and_the_part_serves() {
    let mut s = Running::new();
    let first = s.set(60);
    s.stop();
    let mut state = stored(&s);
    state["items"][0]["deadline"]["repeat"] =
        json!({"kind":"monthly","day":1,"time":"09:00","zone":"Etc/UTC","date":"2026-11-01"});
    let later = state["items"][0].clone();
    store(&s, &state);
    s.start();
    let status = s.ask(json!({"op":"status"}));
    assert_eq!(status["items"], 0, "{status}");
    let next = s.set(60);
    assert_ne!(next["id"], first["id"]);
    let report = health_until(&s, |report| said(report, "unread").as_deref() == Some("1"));
    assert_eq!(report.level, Level::Warn);
    assert!(report.message.contains("can't read"), "{}", report.message);
    s.stop();
    let state = stored(&s);
    assert_eq!(state["items"][0], later, "kept as it is");
    assert_eq!(state["items"][1]["id"], next["id"]);
}

/// #1680: a schedule database from a newer build (`user_version` 2) doesn't
/// stop the part: it is kept whole, the part starts with an empty store,
/// serves, and health names the kept file.
#[test]
fn a_newer_schedule_database_is_set_aside_and_the_part_serves() {
    let mut s = Running::new();
    s.set(60);
    s.stop();
    rusqlite::Connection::open(s.config.dir.join("schedule.sqlite3"))
        .unwrap()
        .execute_batch("PRAGMA user_version=2")
        .unwrap();
    s.start();
    assert_eq!(s.ask(json!({"op":"status"}))["items"], 0);
    s.set(60);
    let report = health_until(&s, |report| said(report, "set_aside").is_some());
    assert_eq!(report.level, Level::Warn);
    let kept = said(&report, "set_aside").unwrap();
    assert!(kept.starts_with("schedule.sqlite3.newer-db2-"), "{kept}");
    assert!(s.config.dir.join(&kept).exists());
}

/// #1680: a zone this build doesn't know (`LAMP_ZONE` from a newer tzdb)
/// doesn't stop the part: it serves in a fallback zone, and health warns
/// and names both.
#[test]
fn an_unknown_zone_is_a_health_warning_not_a_stop() {
    let s = Running::in_zone("Middle_Earth/Rivendell");
    let status = s.ask(json!({"op":"status"}));
    assert_ne!(status["zone"], "Middle_Earth/Rivendell", "{status}");
    s.set(60);
    let report = health_until(&s, |report| said(report, "zone_asked").is_some());
    assert_eq!(report.level, Level::Warn);
    assert_eq!(
        said(&report, "zone_asked").as_deref(),
        Some("Middle_Earth/Rivendell")
    );
    assert_eq!(
        said(&report, "zone"),
        status["zone"].as_str().map(str::to_owned)
    );
}

#[test]
fn a_second_owner_cannot_replace_the_live_socket_or_database() {
    let s = Running::new();
    let stop = Arc::new(AtomicBool::new(false));
    assert!(serve(s.config.clone(), s.clock.clone(), stop).is_err());
    assert_eq!(s.ask(json!({"op":"status"}))["ok"], true);
}
#[test]
fn a_socket_symlink_is_preserved_and_refused() {
    let mut s = Running::new();
    s.stop();
    let target = s.root.path().join("keep");
    fs::write(&target, b"keep me").unwrap();
    symlink(&target, &s.config.socket).unwrap();
    assert!(
        serve(
            s.config.clone(),
            s.clock.clone(),
            Arc::new(AtomicBool::new(false))
        )
        .is_err()
    );
    assert_eq!(fs::read(target).unwrap(), b"keep me");
    assert!(
        fs::symlink_metadata(&s.config.socket)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
#[test]
fn unsafe_socket_parent_is_rejected() {
    let mut s = Running::new();
    s.stop();
    fs::set_permissions(
        s.config.socket.parent().unwrap(),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(
        serve(
            s.config.clone(),
            s.clock.clone(),
            Arc::new(AtomicBool::new(false))
        )
        .is_err()
    );
    assert!(!s.config.socket.exists());
}
#[test]
fn a_finite_service_stops_and_releases_its_owner() {
    let mut s = Running::new();
    s.stop();
    let mut config = s.config.clone();
    config.run_for = Some(Duration::from_millis(100));
    {
        // It lets go of the store in the turn's share, as a stop does.
        let _share = share();
        serve(config, s.clock.clone(), Arc::new(AtomicBool::new(false))).unwrap();
    }
    s.start();
    assert_eq!(s.ask(json!({"op":"status"}))["items"], 0);
}
#[test]
fn cli_default_off_does_not_require_a_clock_or_touch_paths() {
    let root = tempfile::tempdir().unwrap();
    let output = output_of(
        Command::new(env!("CARGO_BIN_EXE_lamp-schedule"))
            .args(["serve", "--zone", "Bad/Zone", "--dir"])
            .arg(root.path().join("data")),
    );
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["enabled"],
        false
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}
#[test]
fn cli_rejects_unknown_and_repeated_options() {
    for args in [
        vec!["serve", "--unknown"],
        vec!["serve", "--enable", "--enable"],
        vec!["serve", "--run-for-ms", "0"],
    ] {
        let output = output_of(Command::new(env!("CARGO_BIN_EXE_lamp-schedule")).args(args));
        assert!(!output.status.success());
    }
}
#[test]
fn host_clock_requires_an_identity_and_reports_explicit_trust() {
    #[cfg(not(target_os = "linux"))]
    assert!(SystemClock::new(None, true).is_err());
    let clock = SystemClock::new(Some("test-clock"), true).unwrap();
    let first = clock.sample().unwrap();
    let second = clock.sample().unwrap();
    assert!(first.wall_trusted);
    assert!(second.boot_ms >= first.boot_ms);
}

#[test]
fn kernel_clock_is_trusted_only_when_named_and_the_kernel_says_set() {
    let marker = cfg!(target_os = "linux")
        && std::path::Path::new("/run/systemd/timesync/synchronized").is_file();
    let off = SystemClock::new(Some("test-clock"), false).unwrap();
    assert_eq!(off.sample().unwrap().wall_trusted, marker);
    let kernel = SystemClock::new(Some("test-clock"), false)
        .unwrap()
        .with_kernel_clock(true);
    assert_eq!(
        kernel.sample().unwrap().wall_trusted,
        marker || lamp_ipc::clock_synced() == Some(true)
    );
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn cli(config: &Config) -> ChildGuard {
    ChildGuard(spawned(
        Command::new(env!("CARGO_BIN_EXE_lamp-schedule"))
            .args([
                "serve",
                "--enable",
                "--trust-clock",
                "--boot-id",
                "crash-test",
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
            .stderr(Stdio::null()),
    ))
}
fn wait_cli(config: &Config, child: &mut ChildGuard) {
    let start = Instant::now();
    loop {
        if ask(&config.socket, &json!({"op":"status"})).is_ok() {
            return;
        }
        assert!(child.0.try_wait().unwrap().is_none(), "CLI exited early");
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn killed_process_reopens_the_same_store_and_recovers_its_stale_socket() {
    let mut s = Running::new();
    s.stop();
    let mut child = cli(&s.config);
    wait_cli(&s.config, &mut child);
    let status = ask(&s.config.socket, &json!({"op":"status"})).unwrap();
    let request = json!({"op":"set","guard":{"store_id":status["store_id"],"revision":status["revision"],"request_id":"crash-create"},"text":"Survive kill","when":{"kind":"after","seconds":600}});
    let receipt = ask(&s.config.socket, &request).unwrap();
    assert_eq!(receipt["ok"], true);
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(s.config.socket.exists());
    let mut restarted = cli(&s.config);
    wait_cli(&s.config, &mut restarted);
    assert_eq!(ask(&s.config.socket, &request).unwrap(), receipt);
    assert_eq!(
        ask(&s.config.socket, &json!({"op":"status"})).unwrap()["items"],
        1
    );
}
