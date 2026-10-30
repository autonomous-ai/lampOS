//! The real binary's optional reports, with synthetic records and temporary
//! stores. These are development-host integration checks, not lamp timings.
use crate::health_control;

use health_control::{Controller, Window};
use lamp_bus::{Bus, Message, Subscriber};
use lamp_memory::{days::DATABASE, health::REPORT_EVERY};
use lamp_msgs::{Diagnostic, Level};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    fs::{self, DirBuilder, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const BINARY: &str = env!("CARGO_BIN_EXE_lamp-memory");
const FIXTURE_BINARY: &str = env!("CARGO_BIN_EXE_lamp-memory-fixture");
/// Watchdogs, not latency bounds: `--memory-days` opens and writes with full
/// flushes, which a busy Mac disk held past 8 s for readiness alone (#1569,
/// as `HOST_WAIT`).
const WAIT: Duration = Duration::from_secs(30);
const REQUEST_WAIT: Duration = Duration::from_secs(30);
static NEXT: AtomicU64 = AtomicU64::new(0);

type ControlSlot = Rc<RefCell<Option<Controller>>>;

struct Scratch(PathBuf, ControlSlot);

impl Scratch {
    fn new() -> Self {
        // macOS's default temporary directory can exceed Unix socket limits
        // once the bus adds a topic and subscriber name.
        let path = PathBuf::from("/tmp").join(format!(
            "lmh-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path, Rc::new(RefCell::new(None)))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn subscribe(&self) -> Reports {
        Reports {
            subscriber: Bus::new(self.path("b"))
                .unwrap()
                .subscribe_typed(Diagnostic::TOPIC, "t", Diagnostic::TYPE)
                .unwrap(),
            control: Rc::clone(&self.1),
        }
    }
}

struct Reports {
    subscriber: Subscriber,
    control: ControlSlot,
}

impl Reports {
    // The ordinary failed-open cases retain their actual post-reap receive.
    fn recv(&mut self, timeout: Duration) -> io::Result<Option<Message>> {
        self.subscriber.recv(timeout)
    }
}

fn control_window(control: &ControlSlot) -> io::Result<Option<Window>> {
    control
        .borrow()
        .as_ref()
        .map(Controller::window)
        .transpose()
}

fn with_control<T>(
    control: &ControlSlot,
    operation: impl FnOnce(&mut Controller) -> io::Result<T>,
) -> io::Result<T> {
    let mut attached = control.borrow_mut();
    let controller = attached.as_mut().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "missing explicit Memory control",
        )
    })?;
    operation(controller)
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Every assertion retains this guard, including startup-failure assertions.
/// Logs go to a file so an unread pipe cannot block the child.
struct Process {
    child: Child,
    log: PathBuf,
    control: ControlSlot,
}

impl Process {
    fn start(scratch: &Scratch, days: bool, bus: bool, env_bus: bool) -> Self {
        Self::spawn(scratch, days, bus, env_bus, false)
    }

    fn start_controlled(scratch: &Scratch, days: bool, bus: bool, env_bus: bool) -> Self {
        Self::spawn(scratch, days, bus, env_bus, true)
    }

    fn spawn(scratch: &Scratch, days: bool, bus: bool, env_bus: bool, controlled: bool) -> Self {
        assert!(
            scratch.1.borrow().is_none(),
            "a child already owns this control slot"
        );
        let log = scratch.path("log");
        let mut command = Command::new(if controlled { FIXTURE_BINARY } else { BINARY });
        command
            .arg("--dir")
            .arg(scratch.path("store"))
            .arg("--socket")
            .arg(scratch.path("m.sock"))
            .env_remove("LAMP_BUS_ROOT")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(fs::File::create(&log).unwrap());
        if days {
            command.arg("--memory-days");
        }
        if bus {
            command.arg("--bus-root").arg(scratch.path("b"));
        }
        if env_bus {
            command.env("LAMP_BUS_ROOT", scratch.path("b"));
        }
        let (child, controller) = if controlled {
            let (child, controller) = health_control::start(&mut command, &scratch.0, &log)
                .unwrap_or_else(|error| {
                    panic!(
                        "memory fixture startup: {error}; {}",
                        fs::read_to_string(&log).unwrap_or_default()
                    )
                });
            (child, Some(controller))
        } else {
            (command.spawn().unwrap(), None)
        };
        // Guard the actual Child before publishing its controller to helpers.
        let process = Self {
            child,
            log,
            control: Rc::clone(&scratch.1),
        };
        *process.control.borrow_mut() = controller;
        process
    }

    fn raw_log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn window(&self) -> Option<Window> {
        let result = control_window(&self.control);
        result.unwrap_or_else(|error| panic!("memory fixture window: {error}; {}", self.raw_log()))
    }

    fn settle_in(&self, window: &Window) {
        let result = with_control(&self.control, |control| control.settle(window));
        result.unwrap_or_else(|error| panic!("memory fixture settle: {error}; {}", self.raw_log()));
    }

    fn log(&self) -> String {
        match self.window() {
            Some(window) => self.log_in(&window),
            None => self.raw_log(),
        }
    }

    fn log_in(&self, window: &Window) -> String {
        self.settle_in(window);
        self.raw_log()
    }

    fn cadence_window(&self) -> Window {
        self.window().expect("explicit reporting child")
    }

    fn cadence_now(&self) -> Instant {
        self.control
            .borrow()
            .as_ref()
            .expect("explicit reporting child")
            .cadence_now()
    }

    fn wait_cadence_in(&self, duration: Duration, window: &Window) {
        let result = with_control(&self.control, |control| {
            let target = control.cadence_now().checked_add(duration).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "memory fixture cadence overflow",
                )
            })?;
            control.advance_to(target, window)
        });
        result
            .unwrap_or_else(|error| panic!("memory fixture advance: {error}; {}", self.raw_log()));
    }

    fn ready(&mut self, scratch: &Scratch) {
        let window = self.window();
        let until = Instant::now() + WAIT;
        let mut delay = Duration::from_millis(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                panic!(
                    "memory exited before readiness: {status}; {}",
                    self.raw_log()
                );
            }
            if fs::read(scratch.path("ready")).is_ok_and(|bytes| bytes == b"ready\n") {
                if let Some(window) = &window {
                    self.settle_in(window);
                }
                return;
            }
            assert!(
                Instant::now() < until,
                "memory never became ready; {}",
                self.raw_log()
            );
            thread::sleep(delay);
            delay = (delay * 2).min(Duration::from_millis(50));
        }
    }

    fn exited(&mut self) -> ExitStatus {
        let until = Instant::now() + WAIT;
        let mut delay = Duration::from_millis(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < until,
                "memory did not exit; {}",
                self.raw_log()
            );
            thread::sleep(delay);
            delay = (delay * 2).min(Duration::from_millis(50));
        }
    }

    fn logged(&mut self, expected: &str) {
        let window = self.window();
        let until = Instant::now() + WAIT;
        let mut delay = Duration::from_millis(5);
        loop {
            let log = match &window {
                Some(window) => self.log_in(window),
                None => self.raw_log(),
            };
            if log.contains(expected) {
                return;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "memory exited before logging {expected}; {log}"
            );
            assert!(Instant::now() < until, "never logged {expected}; {log}");
            if let Some(window) = &window {
                let result = with_control(&self.control, |control| {
                    let due = control.next_due(window)?;
                    control.advance_to(due, window)
                });
                result.unwrap_or_else(|error| {
                    panic!("memory fixture log wait: {error}; {}", self.raw_log())
                });
            } else {
                thread::sleep(delay);
                delay = (delay * 2).min(Duration::from_millis(50));
            }
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let controller = self.control.borrow_mut().take();
        drop(controller);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn ask_raw(scratch: &Scratch, request: &str) -> Value {
    let stream = UnixStream::connect(scratch.path("m.sock")).unwrap();
    stream.set_read_timeout(Some(REQUEST_WAIT)).unwrap();
    stream.set_write_timeout(Some(REQUEST_WAIT)).unwrap();
    let mut reader = BufReader::new(stream);
    reader.get_mut().write_all(request.as_bytes()).unwrap();
    reader.get_mut().write_all(b"\n").unwrap();
    let mut line = String::new();
    assert!(
        reader.read_line(&mut line).unwrap() > 0,
        "no answer to {request}"
    );
    serde_json::from_str(&line).unwrap()
}

fn ask(scratch: &Scratch, request: Value) -> Value {
    ask_raw(scratch, &request.to_string())
}

fn ok(scratch: &Scratch, request: Value) -> Value {
    let response = ask(scratch, request.clone());
    assert_eq!(response["ok"], true, "{request}: {response}");
    response
}

fn value<'a>(report: &'a Diagnostic, key: &str) -> &'a str {
    report
        .values
        .iter()
        .find(|entry| entry.key == key)
        .unwrap_or_else(|| panic!("missing {key} in {report:?}"))
        .value
        .as_str()
}

fn number(report: &Diagnostic, key: &str) -> u64 {
    value(report, key).parse().unwrap()
}

fn report_when(reports: &mut Reports, accept: impl Fn(&Diagnostic) -> bool) -> Diagnostic {
    let window = control_window(&reports.control).expect("memory fixture report window");
    let until = Instant::now() + WAIT;
    let mut last = None;
    if let Some(window) = &window {
        with_control(&reports.control, |control| control.settle(window))
            .expect("memory fixture report settle");
    }
    loop {
        let left = until.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no matching memory report; last={last:?}");
        let timeout = if window.is_some() {
            Duration::ZERO
        } else {
            left.min(Duration::from_millis(250))
        };
        if let Some(message) = reports.recv(timeout).unwrap() {
            assert_eq!(message.header.type_name.as_deref(), Some(Diagnostic::TYPE));
            let report: Diagnostic = message.parse().unwrap();
            assert_eq!(report.name, "memory");
            if accept(&report) {
                return report;
            }
            last = Some(report);
        } else if let Some(window) = &window {
            // An actual empty receive is the only permission to run the next
            // original due cycle. Checked itself never becomes a report.
            let result = with_control(&reports.control, |control| {
                let due = control.next_due(window)?;
                control.advance_to(due, window)
            });
            result.unwrap_or_else(|error| {
                panic!("memory fixture report wait: {error}; last={last:?}")
            });
        }
    }
}

fn retained(reports: &mut Reports, exchanges: u64, kept: u64, answered: u64) -> Diagnostic {
    report_when(reports, |report| {
        report.level == Level::Ok
            && value(report, "exchanges") == exchanges.to_string()
            && value(report, "kept") == kept.to_string()
            && number(report, "answered") >= answered
    })
}

fn counts_through_real_requests(days: bool) {
    let scratch = Scratch::new();
    let mut reports = scratch.subscribe();
    let mut process = Process::start_controlled(&scratch, days, true, false);
    process.ready(&scratch);
    let initial = retained(&mut reports, 0, 0, 0);
    assert_eq!(value(&initial, "slowest_answer_ms"), "none");
    assert_eq!(number(&initial, "answered"), 0);
    // A second unchanged report proves the publisher continues while the
    // socket owner waits for a client. It does not assert scheduler precision.
    retained(&mut reports, 0, 0, 0);
    let at = now();
    for person in ["everyone", "other"] {
        ok(
            &scratch,
            json!({"op":"append","person":person,"at_unix_ms":at,"heard":"synthetic hello"}),
        );
        ok(
            &scratch,
            json!({"op":"remember","person":person,"at_unix_ms":at,"text":"synthetic coffee"}),
        );
    }
    let added = retained(&mut reports, 2, 2, 4);
    assert!(value(&added, "slowest_answer_ms").parse::<f64>().unwrap() > 0.0);
    assert_eq!(number(&added, "note_budget_ms"), 300);
    assert_eq!(number(&added, "call_budget_ms"), 1500);
    assert_eq!(number(&added, "recent_answers"), 4);
    ok(&scratch, json!({"op":"forget_last","kind":"exchange"}));
    retained(&mut reports, 1, 2, 5);
    ok(&scratch, json!({"op":"forget_all","person":"other"}));
    retained(&mut reports, 0, 1, 6);
    ok(&scratch, json!({"op":"forget_all","person":null}));
    retained(&mut reports, 0, 0, 7);

    for (request, code) in [
        ("not JSON".to_owned(), "bad_json"),
        (json!({"op":"unknown"}).to_string(), "unknown_op"),
        (
            json!({"op":"append","at_unix_ms":at,"heard":" "}).to_string(),
            "empty",
        ),
        (
            json!({"op":"remember","at_unix_ms":at,"text":" "}).to_string(),
            "empty",
        ),
    ] {
        let response = ask_raw(&scratch, &request);
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"], code);
    }
    let refused = retained(&mut reports, 0, 0, 11);
    assert!(refused.message.is_empty());
    assert_eq!(number(&refused, "answered"), 11);
}

#[test]
fn legacy_reports_retained_counts_and_stays_well_after_caller_refusals() {
    counts_through_real_requests(false);
}

#[test]
fn sqlite_reports_retained_counts_and_stays_well_after_caller_refusals() {
    counts_through_real_requests(true);
}

#[test]
fn a_failed_legacy_write_stays_warn_through_reads_until_a_write_succeeds() {
    let scratch = Scratch::new();
    let mut reports = scratch.subscribe();
    let mut process = Process::start_controlled(&scratch, false, true, false);
    process.ready(&scratch);
    let at = now();
    ok(
        &scratch,
        json!({"op":"append","at_unix_ms":at,"heard":"synthetic hello"}),
    );
    ok(
        &scratch,
        json!({"op":"remember","at_unix_ms":at,"text":"synthetic coffee"}),
    );
    retained(&mut reports, 1, 1, 2);

    let kept = scratch.path("store/everyone/kept.jsonl");
    let saved = scratch.path("saved-kept");
    let mut answered = 2;
    for (recovery, old_exchanges, new_exchanges, new_kept) in [
        (
            json!({"op":"append","at_unix_ms":at,"heard":"synthetic recovery"}),
            1,
            2,
            1,
        ),
        (
            json!({"op":"remember","at_unix_ms":at,"text":"synthetic recovery"}),
            2,
            2,
            2,
        ),
    ] {
        fs::rename(&kept, &saved).unwrap();
        fs::create_dir(&kept).unwrap();
        let refused = ask(
            &scratch,
            json!({"op":"remember","at_unix_ms":at,"text":"synthetic failure"}),
        );
        answered += 1;
        assert_eq!(refused["error"], "storage");
        report_when(&mut reports, |report| {
            report.level == Level::Warn
                && report.message.contains("storage failed")
                && number(report, "answered") >= answered
        });

        fs::remove_dir(&kept).unwrap();
        fs::rename(&saved, &kept).unwrap();
        let read = ok(&scratch, json!({"op":"kept"}));
        answered += 1;
        assert_eq!(read["entries"].as_array().unwrap().len(), 1);
        let warning = report_when(&mut reports, |report| {
            report.level == Level::Warn
                && report.message.contains("awaiting a successful write")
                && number(report, "answered") >= answered
                && value(report, "kept") == "1"
        });
        assert_eq!(number(&warning, "exchanges"), old_exchanges);
        ok(&scratch, recovery);
        answered += 1;
        retained(&mut reports, new_exchanges, new_kept, answered);
    }
}

#[test]
fn the_environment_alone_does_not_enable_reporting_or_create_bus_data() {
    for days in [false, true] {
        let scratch = Scratch::new();
        let mut process = Process::start(&scratch, days, false, true);
        process.ready(&scratch);
        ok(
            &scratch,
            json!({"op":"remember","at_unix_ms":now(),"text":"synthetic offline note"}),
        );
        let _ = ok(&scratch, json!({"op":"status"}));
        // The program says reports are off, an event, instead of the test
        // watching two report periods for nothing: with no `--bus-root` it
        // makes no reporter, so nothing could appear later.
        process.logged("lamp-memory: health reports off: no --bus-root");
        assert!(process.child.try_wait().unwrap().is_none());
        assert!(!scratch.path("b").exists());
        assert!(!process.log().contains("health delivery"));
    }
}

#[test]
fn an_unusable_bus_does_not_prevent_serving_and_reports_recover_when_it_returns() {
    let scratch = Scratch::new();
    fs::write(scratch.path("b"), b"synthetic bus obstruction").unwrap();
    let mut process = Process::start_controlled(&scratch, false, true, false);
    process.ready(&scratch);
    ok(
        &scratch,
        json!({"op":"remember","at_unix_ms":now(),"text":"synthetic note during bus failure"}),
    );
    assert_eq!(
        fs::read(scratch.path("b")).unwrap(),
        b"synthetic bus obstruction"
    );
    process.logged("health delivery failed");
    fs::remove_file(scratch.path("b")).unwrap();
    let mut reports = scratch.subscribe();
    retained(&mut reports, 0, 1, 1);
    let next = retained(&mut reports, 0, 1, 1);
    assert!(next.message.is_empty());
    let read = ok(&scratch, json!({"op":"kept"}));
    assert_eq!(read["entries"].as_array().unwrap().len(), 1);
}

#[test]
fn a_working_bus_that_fails_stays_failed_until_publication_really_recovers() {
    const FAILED: &str = "lamp-memory: health delivery failed";
    const RECOVERED: &str = "lamp-memory: health delivery recovered";
    let scratch = Scratch::new();
    let mut reports = scratch.subscribe();
    let mut process = Process::start_controlled(&scratch, false, true, false);
    process.ready(&scratch);
    retained(&mut reports, 0, 0, 0);
    assert!(!process.log().contains(FAILED));
    drop(reports);

    // Keep the old publisher's tree for inspection while its configured root
    // becomes a file. This exercises failure of an already-used Publisher.
    let bus = scratch.path("b");
    fs::rename(&bus, scratch.path("old-b")).unwrap();
    fs::write(&bus, b"synthetic sustained bus obstruction").unwrap();
    ok(
        &scratch,
        json!({"op":"remember","at_unix_ms":now(),"text":"synthetic note while reporting is broken"}),
    );
    let mut answered = 1;
    process.logged(FAILED);

    // After the first observed failure, keep the obstruction through three
    // more report periods and continue answering requests. This window is
    // synchronization for retries, not a claim of exact worker scheduling.
    let window = process.cadence_window();
    let until = process.cadence_now() + REPORT_EVERY * 3;
    loop {
        let read = ok(&scratch, json!({"op":"status"}));
        answered += 1;
        assert_eq!(read["people"][0]["kept"], 1);
        assert_eq!(
            fs::read(&bus).unwrap(),
            b"synthetic sustained bus obstruction"
        );
        let log = process.log_in(&window);
        assert_eq!(
            log.lines().filter(|line| *line == FAILED).count(),
            1,
            "{log}"
        );
        assert_eq!(
            log.lines().filter(|line| *line == RECOVERED).count(),
            0,
            "{log}"
        );
        let left = until.saturating_duration_since(process.cadence_now());
        if left.is_zero() {
            break;
        }
        process.wait_cadence_in(left.min(Duration::from_millis(200)), &window);
    }

    // Rebuild the bus tree, rather than returning the old publisher's files.
    // create_dir_all also permits the retrying publisher to create it first.
    fs::remove_file(&bus).unwrap();
    fs::create_dir_all(&bus).unwrap();
    let mut reports = scratch.subscribe();
    retained(&mut reports, 0, 1, answered);
    process.logged(RECOVERED);
    retained(&mut reports, 0, 1, answered);
    let log = process.log();
    assert_eq!(
        log.lines().filter(|line| *line == FAILED).count(),
        1,
        "{log}"
    );
    assert_eq!(
        log.lines().filter(|line| *line == RECOVERED).count(),
        1,
        "{log}"
    );
}

fn private_file(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
}

#[test]
fn a_store_that_cannot_open_never_claims_readiness_and_any_delivered_report_is_warn() {
    for days in [false, true] {
        let scratch = Scratch::new();
        DirBuilder::new()
            .mode(0o700)
            .create(scratch.path("store"))
            .unwrap();
        // The same file blocks the legacy opener's backend guard, or is
        // rejected as a corrupt SQLite database by the opt-in opener.
        private_file(
            &scratch.path("store").join(DATABASE),
            b"synthetic invalid database",
        );
        let mut reports = scratch.subscribe();
        let mut process = Process::start(&scratch, days, true, false);
        assert_eq!(process.exited().code(), Some(1), "{}", process.log());
        assert!(!scratch.path("ready").exists());
        // Final reporting is best-effort and shutdown waits only 100 ms.
        // Absence is allowed; a delivered report must not claim health.
        let until = Instant::now() + Duration::from_millis(250);
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let Some(message) = reports.recv(left).unwrap() else {
                break;
            };
            let report: Diagnostic = message.parse().unwrap();
            assert_eq!(report.name, "memory");
            assert_eq!(report.level, Level::Warn);
            assert!(!report.message.is_empty());
        }
        assert_eq!(
            fs::read(scratch.path("store").join(DATABASE)).unwrap(),
            b"synthetic invalid database"
        );
    }
}

/// #1680: memory a newer build wrote doesn't stop the part. It serves, the
/// journal names the kept file, and health warns with its name.
#[test]
fn a_newer_database_is_set_aside_and_health_says_so_while_memory_serves() {
    let scratch = Scratch::new();
    let store = scratch.path("store");
    DirBuilder::new().mode(0o700).create(&store).unwrap();
    private_file(&store.join(DATABASE), b"");
    rusqlite::Connection::open(store.join(DATABASE))
        .unwrap()
        .execute_batch("CREATE TABLE later(value TEXT); PRAGMA user_version=2")
        .unwrap();
    let original = fs::read(store.join(DATABASE)).unwrap();
    let mut reports = scratch.subscribe();
    // Step the existing report schedule, then require the actual set-aside
    // report; a controller acknowledgement is not the health observation.
    let mut process = Process::start_controlled(&scratch, true, true, false);
    process.ready(&scratch);
    process.logged("it is set aside as memory-days.sqlite3.newer-db2-");
    let report = report_when(&mut reports, |report| {
        report.values.iter().any(|pair| pair.key == "set_aside")
    });
    assert_eq!(report.level, Level::Warn);
    let kept = value(&report, "set_aside").to_owned();
    assert!(kept.starts_with("memory-days.sqlite3.newer-db2-"), "{kept}");
    assert_eq!(fs::read(store.join(&kept)).unwrap(), original);
    ok(
        &scratch,
        json!({"op":"remember","person":"everyone","at_unix_ms":now(),"text":"synthetic coffee"}),
    );
}

/// #2067: forgetting everything removes the set-aside database, and health
/// stops warning about it.
#[test]
fn forgetting_everything_removes_a_set_aside_database_and_health_stops_warning() {
    let scratch = Scratch::new();
    let store = scratch.path("store");
    DirBuilder::new().mode(0o700).create(&store).unwrap();
    private_file(&store.join(DATABASE), b"");
    rusqlite::Connection::open(store.join(DATABASE))
        .unwrap()
        .execute_batch("PRAGMA user_version=2")
        .unwrap();
    let mut reports = scratch.subscribe();
    // The fixture's schedule, stepped by `report_when` and `logged`: not
    // two of the host's report seconds (#1806).
    let mut process = Process::start_controlled(&scratch, true, true, false);
    process.ready(&scratch);
    let report = report_when(&mut reports, |report| {
        report.values.iter().any(|pair| pair.key == "set_aside")
    });
    let kept = value(&report, "set_aside").to_owned();
    ok(&scratch, json!({"op":"forget_all","person":null}));
    process.logged(&format!(
        "a forget also removed the memory a newer build wrote: {kept}"
    ));
    assert!(!store.join(&kept).exists());
    let report = report_when(&mut reports, |report| {
        report.values.iter().all(|pair| pair.key != "set_aside")
    });
    assert_ne!(report.level, Level::Warn, "{report:?}");
}
