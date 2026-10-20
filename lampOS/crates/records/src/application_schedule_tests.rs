use super::*;
use lamp_bus::Bus;
use lamp_msgs::Diagnostic;
use std::{
    cell::Cell,
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const HOST_WAIT: Duration = Duration::from_secs(4);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        // Keep real Unix socket paths short on macOS as well as Linux.
        let path = fs::canonicalize("/tmp").unwrap().join(format!(
            "rc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn config(&self) -> (PathBuf, Config) {
        let mut config = Config::customer_default();
        config.bus_root = self.0.join("bus");
        config.roots[0].path = self.0.join("sessions");
        let path = self.0.join("config.json");
        json_file::write(&path, &config).unwrap();
        (path, config)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Clone)]
struct Clock {
    origin: Instant,
    micros: Arc<AtomicU64>,
}

impl Clock {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            micros: Arc::new(AtomicU64::new(0)),
        }
    }

    fn now(&self) -> Instant {
        self.origin
            .checked_add(Duration::from_micros(self.micros.load(Ordering::SeqCst)))
            .unwrap()
    }

    fn advance(&self, duration: Duration) {
        let micros = u64::try_from(duration.as_micros()).unwrap();
        self.micros
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |before| {
                before.checked_add(micros)
            })
            .unwrap();
    }
}

struct Schedule<F> {
    clock: Clock,
    reads: Arc<AtomicUsize>,
    wait: F,
}

impl<F: FnMut(ReportCycle, Duration) -> io::Result<()>> ReportSchedule for Schedule<F> {
    fn now(&self) -> Instant {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.clock.now()
    }

    fn wait(&mut self, cycle: ReportCycle, remaining: Duration) -> io::Result<()> {
        (self.wait)(cycle, remaining)
    }
}

fn stopped() -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Interrupted,
        "explicit fixture wait stopped",
    ))
}

fn assert_stopped(result: io::Result<()>) {
    let error = result.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(error.to_string(), "explicit fixture wait stopped");
}

#[test]
fn system_schedule_samples_host_time_and_keeps_a_zero_wait_successful() {
    let before = Instant::now();
    let mut schedule = SystemSchedule;
    let sampled = schedule.now();
    assert!(sampled >= before && sampled <= Instant::now());
    schedule
        .wait(
            ReportCycle {
                started: sampled,
                host_started: before,
                host_seconds: Some(30),
                construction_or_publish: Err(io::Error::other("publication failed")),
            },
            Duration::ZERO,
        )
        .unwrap();
}

#[test]
fn initial_config_errors_precede_clock_factory_and_wait() {
    let scratch = Scratch::new();
    let (config_path, mut config) = scratch.config();
    config.roots.clear();
    json_file::write(&config_path, &config).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let mut schedule = Schedule {
        clock: Clock::new(),
        reads: Arc::clone(&reads),
        wait: |_, _| panic!("invalid configuration reached the wait"),
    };
    for path in [scratch.0.join("missing.json"), config_path] {
        assert!(
            reporting_with(
                path,
                scratch.0.join("last.json"),
                Some(30),
                &mut schedule,
                |_| { panic!("invalid configuration reached the factory") }
            )
            .is_err()
        );
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(!config.bus_root.exists());
}

#[test]
fn report_command_passes_the_original_arguments_once_and_check_does_not_select_it() {
    let scratch = Scratch::new();
    let config = scratch.0.join("missing-config.json");
    let result = scratch.0.join("last.json");
    let expected = (config.clone(), result.clone(), Some(30));
    let calls = Cell::new(0);
    let error = command_with_reporting(
        Command::Report {
            config: config.clone(),
            result,
            seconds: Some(30),
        },
        |config, result, seconds| {
            assert_eq!((config, result, seconds), expected);
            drop(expected);
            calls.set(calls.get() + 1);
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "factory selected",
            ))
        },
    )
    .unwrap_err();
    assert_eq!(calls.get(), 1);
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(
        command_with_reporting(
            Command::Check {
                config,
                apply: false,
                report: None,
            },
            |_, _, _| panic!("check selected the reporter"),
        )
        .is_err()
    );
}

#[test]
fn cadence_reads_preserve_work_residual_and_reuse_the_actual_publisher() {
    let scratch = Scratch::new();
    let (config_path, config) = scratch.config();
    let mut subscriber = Bus::new(&config.bus_root)
        .unwrap()
        .subscribe_typed(Diagnostic::TOPIC, "cadence", Diagnostic::TYPE)
        .unwrap();
    let clock = Clock::new();
    let reads = Arc::new(AtomicUsize::new(0));
    let calls = Cell::new(0);
    let mut cycles = 0;
    let mut host_started = None;
    let before_host = Instant::now();
    let mut schedule = Schedule {
        clock: clock.clone(),
        reads: Arc::clone(&reads),
        wait: |cycle: ReportCycle, remaining| {
            cycles += 1;
            assert_eq!(reads.load(Ordering::SeqCst), cycles * 2);
            assert_eq!(
                cycle.started,
                clock.origin + Duration::from_secs((cycles - 1) as u64)
            );
            assert_eq!(cycle.host_seconds, Some(30));
            assert!(cycle.host_started >= before_host && cycle.host_started <= Instant::now());
            assert_eq!(
                *host_started.get_or_insert(cycle.host_started),
                cycle.host_started
            );
            cycle.construction_or_publish.unwrap();
            let message = subscriber.recv(Duration::ZERO).unwrap().unwrap();
            assert_eq!(message.header.seq, cycles as u64);
            let report: Diagnostic = serde_json::from_value(message.json().unwrap()).unwrap();
            assert_eq!(report.name, "records");
            assert!(report.message.contains("disabled"));
            assert_eq!(
                remaining,
                if cycles == 1 {
                    Duration::from_millis(750)
                } else {
                    Duration::from_secs(1)
                }
            );
            if cycles == 3 {
                return stopped();
            }
            clock.advance(remaining);
            Ok(())
        },
    };
    assert_stopped(reporting_with(
        config_path,
        scratch.0.join("last.json"),
        Some(30),
        &mut schedule,
        |root| {
            calls.set(calls.get() + 1);
            assert_eq!(root, config.bus_root.as_path());
            clock.advance(Duration::from_millis(250));
            let first_read = clock.now();
            let second_read = clock.now();
            assert_eq!(first_read, clock.origin + Duration::from_millis(250));
            assert_eq!(first_read, second_read);
            let publisher_clock = clock.clone();
            Reporter::new_with_cadence_clock(root, move || publisher_clock.now())
        },
    ));
    assert_eq!(calls.get(), 1);
    assert_eq!(cycles, 3);
    assert_eq!(reads.load(Ordering::SeqCst), 6);
    assert!(!scratch.0.join("last.json").exists());
}

#[test]
fn zero_residual_is_preserved_and_logical_time_does_not_exhaust_the_host_lifetime() {
    let scratch = Scratch::new();
    let (config_path, _) = scratch.config();
    let clock = Clock::new();
    let mut waits = 0;
    let mut schedule = Schedule {
        clock: clock.clone(),
        reads: Arc::new(AtomicUsize::new(0)),
        wait: |cycle: ReportCycle, remaining| {
            waits += 1;
            cycle.construction_or_publish.unwrap();
            assert_eq!(cycle.host_seconds, Some(30));
            assert_eq!(
                remaining,
                if waits == 1 {
                    Duration::ZERO
                } else {
                    Duration::from_secs(1)
                }
            );
            if waits == 1 { Ok(()) } else { stopped() }
        },
    };
    assert_stopped(reporting_with(
        config_path,
        scratch.0.join("last.json"),
        Some(30),
        &mut schedule,
        |root| {
            clock.advance(Duration::from_secs(3600));
            let publisher_clock = clock.clone();
            Reporter::new_with_cadence_clock(root, move || publisher_clock.now())
        },
    ));
    assert_eq!(waits, 2);
}

#[test]
fn failed_construction_is_observed_after_real_work_and_retried_only_next_cycle() {
    let scratch = Scratch::new();
    let (config_path, config) = scratch.config();
    let mut subscriber = Bus::new(&config.bus_root)
        .unwrap()
        .subscribe_typed(Diagnostic::TOPIC, "failure", Diagnostic::TYPE)
        .unwrap();
    let clock = Clock::new();
    let calls = Cell::new(0);
    let mut waits = 0;
    let mut schedule = Schedule {
        clock: clock.clone(),
        reads: Arc::new(AtomicUsize::new(0)),
        wait: |cycle: ReportCycle, remaining| {
            waits += 1;
            assert_eq!(calls.get(), waits);
            assert_eq!(remaining, Duration::from_secs(1));
            if waits == 1 {
                assert_eq!(
                    cycle.construction_or_publish.unwrap_err().to_string(),
                    "construction refused"
                );
                assert!(subscriber.recv(Duration::ZERO).unwrap().is_none());
                clock.advance(remaining);
                Ok(())
            } else {
                cycle.construction_or_publish.unwrap();
                assert!(subscriber.recv(Duration::ZERO).unwrap().is_some());
                stopped()
            }
        },
    };
    assert_stopped(reporting_with(
        config_path,
        scratch.0.join("last.json"),
        None,
        &mut schedule,
        |root| {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                return Err(io::Error::other("construction refused"));
            }
            let publisher_clock = clock.clone();
            Reporter::new_with_cadence_clock(root, move || publisher_clock.now())
        },
    ));
    assert_eq!(waits, 2);
}

#[test]
fn actual_bus_root_reload_recreates_the_selected_factory() {
    let scratch = Scratch::new();
    let (config_path, mut config) = scratch.config();
    let first = config.bus_root.clone();
    let second = scratch.0.join("other");
    let mut subscribers = [&first, &second].map(|root| {
        Bus::new(root)
            .unwrap()
            .subscribe_typed(Diagnostic::TOPIC, "reload", Diagnostic::TYPE)
            .unwrap()
    });
    let clock = Clock::new();
    let mut calls = Vec::new();
    let mut waits = 0;
    let mut schedule = Schedule {
        clock: clock.clone(),
        reads: Arc::new(AtomicUsize::new(0)),
        wait: |cycle: ReportCycle, remaining| {
            cycle.construction_or_publish.unwrap();
            assert!(subscribers[waits].recv(Duration::ZERO).unwrap().is_some());
            waits += 1;
            if waits == 2 {
                return stopped();
            }
            config.bus_root = second.clone();
            json_file::write(&config_path, &config).unwrap();
            clock.advance(remaining);
            Ok(())
        },
    };
    assert_stopped(reporting_with(
        config_path.clone(),
        scratch.0.join("last.json"),
        None,
        &mut schedule,
        |root| {
            calls.push(root.to_path_buf());
            let publisher_clock = clock.clone();
            Reporter::new_with_cadence_clock(root, move || publisher_clock.now())
        },
    ));
    assert_eq!(calls, vec![first, second]);
}

/// #1680, the first builder on #2078: while `records.json` is a schema newer
/// than this build's, `check` prunes nothing, and the reporter says so in
/// lamp-health, "records: a newer schema, cleanup paused", at warn, on the
/// bus the newer file names, so a lamp left rolled back shows why its disk
/// grows. With this build's schema back, it reports as before.
#[test]
fn a_newer_schema_is_reported_as_the_cleanup_paused_until_this_builds_is_back() {
    let scratch = Scratch::new();
    let (config_path, config) = scratch.config();
    let mut newer = serde_json::to_value(&config).unwrap();
    newer["schema_version"] = 2.into();
    newer["roots"][0]["kind"] = "memory_days".into();
    json_file::write(&config_path, &newer).unwrap();
    let mut subscriber = Bus::new(&config.bus_root)
        .unwrap()
        .subscribe_typed(Diagnostic::TOPIC, "paused", Diagnostic::TYPE)
        .unwrap();
    let clock = Clock::new();
    let mut said = Vec::new();
    let mut schedule = Schedule {
        clock: clock.clone(),
        reads: Arc::new(AtomicUsize::new(0)),
        wait: |cycle: ReportCycle, remaining| {
            cycle.construction_or_publish.unwrap();
            let message = subscriber.recv(Duration::ZERO).unwrap().unwrap();
            said.push(serde_json::from_value::<Diagnostic>(message.json().unwrap()).unwrap());
            if said.len() == 3 {
                return stopped();
            }
            if said.len() == 2 {
                json_file::write(&config_path, &config).unwrap();
            }
            clock.advance(remaining);
            Ok(())
        },
    };
    assert_stopped(reporting_with(
        config_path.clone(),
        scratch.0.join("last.json"),
        None,
        &mut schedule,
        |root| {
            let publisher_clock = clock.clone();
            Reporter::new_with_cadence_clock(root, move || publisher_clock.now())
        },
    ));
    for paused in &said[..2] {
        assert_eq!(paused.level, lamp_msgs::Level::Warn);
        assert_eq!(
            (paused.name.as_str(), paused.message.as_str()),
            ("records", "a newer schema, cleanup paused")
        );
        assert_eq!(paused.values[0].value, "2");
    }
    // This build's own, off by default: said as before.
    assert_eq!(
        said[2].message,
        "cleanup is disabled; no retention is enforced"
    );
}

#[test]
fn a_real_publish_error_is_retained_after_successful_construction() {
    let scratch = Scratch::new();
    let (config_path, config) = scratch.config();
    let mut subscriber = Bus::new(&config.bus_root)
        .unwrap()
        .subscribe_typed(Diagnostic::TOPIC, "blocked", Diagnostic::TYPE)
        .unwrap();
    let clock = Clock::new();
    let constructed = Cell::new(false);
    let mut schedule = Schedule {
        clock: clock.clone(),
        reads: Arc::new(AtomicUsize::new(0)),
        wait: |cycle: ReportCycle, remaining| {
            assert!(constructed.get());
            assert!(cycle.construction_or_publish.is_err());
            assert_eq!(remaining, Duration::from_secs(1));
            assert!(subscriber.recv(Duration::ZERO).unwrap().is_none());
            stopped()
        },
    };
    assert_stopped(reporting_with(
        config_path,
        scratch.0.join("last.json"),
        None,
        &mut schedule,
        |root| {
            let publisher_clock = clock.clone();
            let reporter = Reporter::new_with_cadence_clock(root, move || publisher_clock.now())?;
            constructed.set(true);
            fs::rename(root, scratch.0.join("saved-bus"))?;
            fs::write(root, b"actual obstruction")?;
            Ok(reporter)
        },
    ));
    assert_eq!(fs::read(config.bus_root).unwrap(), b"actual obstruction");
}

#[test]
fn a_held_factory_cannot_reach_the_wait_or_fabricate_a_bus_message() {
    let scratch = Scratch::new();
    let (config_path, config) = scratch.config();
    let result_path = scratch.0.join("last.json");
    let mut subscriber = Bus::new(&config.bus_root)
        .unwrap()
        .subscribe_typed(Diagnostic::TOPIC, "held", Diagnostic::TYPE)
        .unwrap();
    let (entered, entry) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let (completed, completion) = mpsc::sync_channel(1);
    thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let clock = Clock::new();
            let mut schedule = Schedule {
                clock: clock.clone(),
                reads: Arc::new(AtomicUsize::new(0)),
                wait: |cycle: ReportCycle, remaining| {
                    cycle.construction_or_publish.unwrap();
                    assert_eq!(remaining, Duration::from_secs(1));
                    completed.send(()).unwrap();
                    stopped()
                },
            };
            reporting_with(config_path, result_path, Some(30), &mut schedule, |root| {
                entered.send(()).unwrap();
                released.recv_timeout(HOST_WAIT).unwrap();
                let publisher_clock = clock.clone();
                Reporter::new_with_cadence_clock(root, move || publisher_clock.now())
            })
        });
        entry.recv_timeout(HOST_WAIT).unwrap();
        assert!(matches!(
            completion.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(subscriber.recv(Duration::ZERO).unwrap().is_none());
        release.send(()).unwrap();
        completion.recv_timeout(HOST_WAIT).unwrap();
        assert!(subscriber.recv(Duration::ZERO).unwrap().is_some());
        assert_stopped(worker.join().unwrap());
    });
}
