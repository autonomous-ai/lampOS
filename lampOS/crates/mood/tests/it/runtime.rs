use lamp_mood::{
    engine::{Event, EventKind},
    runtime::{Clock, Config, Counters, Report, Sample, SystemClock, Transport, run, serve},
};
use lamp_msgs::MoodState;
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

struct TestClock {
    system: SystemClock,
    samples: AtomicU64,
}

impl TestClock {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            system: SystemClock::new("UTC", Some("mood-test-host")).unwrap(),
            samples: AtomicU64::new(0),
        })
    }
}

impl Clock for TestClock {
    fn clock_id(&self) -> &str {
        self.system.clock_id()
    }
    fn sample(&self) -> io::Result<Sample> {
        self.samples.fetch_add(1, Ordering::Relaxed);
        let mut sample = self.system.sample()?;
        sample.local_minute = Some(9 * 60);
        Ok(sample)
    }
}

fn config(ms: u64) -> Config {
    Config {
        enabled: true,
        run_for: Some(Duration::from_millis(ms)),
        ..Config::default()
    }
}

type Outputs = Arc<Mutex<Vec<MoodState>>>;

struct TestTransport {
    clock: Arc<TestClock>,
    input: mpsc::Receiver<EventKind>,
    outputs: Outputs,
    flood: bool,
}

impl Transport for TestTransport {
    fn session_id(&self) -> &str {
        "1234567890abcdef"
    }
    fn poll(&mut self, _: &Counters) -> io::Result<Vec<Event>> {
        let now = self.clock.sample()?.now_us;
        let kinds: Vec<_> = if self.flood {
            vec![EventKind::Heard; 128]
        } else {
            self.input.try_iter().take(64).collect()
        };
        Ok(kinds
            .into_iter()
            .map(|kind| Event {
                observed_us: now,
                valid_until_us: now + 1_000_000,
                kind,
            })
            .collect())
    }
    fn publish(&mut self, state: &MoodState, _: &Counters) -> io::Result<()> {
        self.outputs.lock().unwrap().push(state.clone());
        Ok(())
    }
}

fn transport(
    clock: Arc<TestClock>,
    flood: bool,
) -> (TestTransport, mpsc::Sender<EventKind>, Outputs) {
    let (sender, input) = mpsc::channel();
    let outputs = Arc::new(Mutex::new(Vec::new()));
    (
        TestTransport {
            clock,
            input,
            outputs: outputs.clone(),
            flood,
        },
        sender,
        outputs,
    )
}

#[test]
fn disabled_entry_point_does_not_resolve_zone_clock_or_create_bus_files() {
    let root = tempfile::tempdir().unwrap();
    let bus_root = root.path().join("must-not-exist");
    let report = serve(
        Config {
            bus_root: bus_root.clone(),
            zone: Some("invalid/zone".into()),
            clock_id: Some("".into()),
            ..Config::default()
        },
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(!report.enabled);
    assert!(!bus_root.exists());
}

#[test]
fn one_owner_reduces_events_and_publishes_ordered_metadata() {
    let clock = TestClock::new();
    let (io, sender, output) = transport(clock.clone(), false);
    sender.send(EventKind::Touch { kind: true }).unwrap();
    let report = run(&config(120), clock, io, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(report.accepted, 1);
    assert_eq!(report.engine_rejected, 0);
    assert!(report.published >= 1);
    let states = output.lock().unwrap();
    assert!(states.iter().any(|s| s.awake >= 0.84 && s.warmth > 0.64));
    for state in states.iter() {
        state.check().unwrap();
    }
    for pair in states.windows(2) {
        assert!(pair[0].stamp.sequence < pair[1].stamp.sequence);
        assert!(pair[0].stamp.at_us < pair[1].stamp.at_us);
        assert_eq!(pair[0].session_id, pair[1].session_id);
    }
}

#[test]
fn input_flood_is_dropped_at_a_fixed_queue_and_the_engine_keeps_advancing() {
    let clock = TestClock::new();
    let (io, _sender, _) = transport(clock.clone(), true);
    let report = run(&config(100), clock, io, Arc::new(AtomicBool::new(false))).unwrap();
    assert!(report.queue_dropped > 0);
    assert!(report.accepted > 0);
    assert!(report.steps > 1);
}

struct BlockedTransport {
    release: mpsc::Receiver<()>,
    completed: mpsc::Sender<()>,
    entered: Arc<AtomicBool>,
    publications: Arc<AtomicU64>,
}

impl Drop for BlockedTransport {
    fn drop(&mut self) {
        let _ = self.completed.send(());
    }
}

impl Transport for BlockedTransport {
    fn session_id(&self) -> &str {
        "1234567890abcdef"
    }
    fn poll(&mut self, _: &Counters) -> io::Result<Vec<Event>> {
        self.entered.store(true, Ordering::Release);
        // The test owns the completion contract. A two-second fallback also
        // ensures a regression cannot leave a detached worker permanently.
        let _ = self.release.recv_timeout(Duration::from_secs(2));
        Ok(Vec::new())
    }
    fn publish(&mut self, _: &MoodState, _: &Counters) -> io::Result<()> {
        self.publications.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn blocked_bus_does_not_block_state_steps_and_shutdown_refuses_an_unfinished_worker() {
    let clock = TestClock::new();
    let (release, incoming) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    let publications = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    let result = run(
        &config(80),
        clock.clone(),
        BlockedTransport {
            release: incoming,
            completed: done,
            entered: Arc::new(AtomicBool::new(false)),
            publications: publications.clone(),
        },
        Arc::new(AtomicBool::new(false)),
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("exit this process")
    );
    // The fallback blocking call lasts two seconds; the runtime must leave
    // through its independent 200 ms shutdown boundary, with scheduler slack.
    assert!(started.elapsed() < Duration::from_millis(1500));
    assert!(clock.samples.load(Ordering::Relaxed) > 3);
    release.send(()).unwrap();
    completed.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(publications.load(Ordering::Relaxed), 0);
}

struct FailedClock {
    first: Sample,
    samples: AtomicU64,
    entered: Arc<AtomicBool>,
}

impl Clock for FailedClock {
    fn clock_id(&self) -> &str {
        "mood-test-host"
    }
    fn sample(&self) -> io::Result<Sample> {
        if self.samples.fetch_add(1, Ordering::Relaxed) == 0 {
            return Ok(self.first);
        }
        let waiting = Instant::now();
        while !self.entered.load(Ordering::Acquire) && waiting.elapsed() < Duration::from_secs(1) {
            thread::yield_now();
        }
        Err(io::Error::other("authored clock failure"))
    }
}

#[test]
fn clock_failure_after_spawn_still_runs_bounded_cleanup_and_reports_a_blocked_worker() {
    let entered = Arc::new(AtomicBool::new(false));
    let clock = Arc::new(FailedClock {
        first: TestClock::new().sample().unwrap(),
        samples: AtomicU64::new(0),
        entered: entered.clone(),
    });
    let (release, incoming) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    let publications = Arc::new(AtomicU64::new(0));
    let result = run(
        &config(5000),
        clock,
        BlockedTransport {
            release: incoming,
            completed: done,
            entered,
            publications: publications.clone(),
        },
        Arc::new(AtomicBool::new(false)),
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("exit this process")
    );
    release.send(()).unwrap();
    completed.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(publications.load(Ordering::Relaxed), 0);
}

struct FailingTransport;
impl Transport for FailingTransport {
    fn session_id(&self) -> &str {
        "1234567890abcdef"
    }
    fn poll(&mut self, _: &Counters) -> io::Result<Vec<Event>> {
        Ok(Vec::new())
    }
    fn publish(&mut self, _: &MoodState, _: &Counters) -> io::Result<()> {
        Err(io::Error::other("authored disk failure"))
    }
}

#[test]
fn failed_output_is_counted_without_ending_the_state_engine() {
    let report = run(
        &config(80),
        TestClock::new(),
        FailingTransport,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(report.publish_errors > 0);
    assert_eq!(report.published, 0);
    assert!(report.steps > 1);
}

struct DelayedWorkerClock;
impl Clock for DelayedWorkerClock {
    fn clock_id(&self) -> &str {
        "mood-test-host"
    }
    fn sample(&self) -> io::Result<Sample> {
        // Model a worker reaching a queued snapshot 250001 us after it was
        // made. The owner remains fixed so it cannot replace that snapshot.
        let delayed = thread::current().name() == Some("mood-bus");
        Ok(Sample {
            now_us: 10_000_000 + if delayed { 250_001 } else { 0 },
            unix_ms: 1_791_974_400_000,
            local_minute: Some(540),
        })
    }
}

#[test]
fn a_queued_snapshot_older_than_250ms_is_discarded_before_publication() {
    let report = run(
        &config(80),
        Arc::new(DelayedWorkerClock),
        FailingTransport,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(report.old_outputs, 1);
    assert_eq!(report.published, 0);
    // FailingTransport would increment publish_errors if it were called.
    assert_eq!(report.publish_errors, 0);
}

#[test]
fn an_external_stop_finishes_the_bounded_worker() {
    let clock = TestClock::new();
    let (io, _sender, _) = transport(clock.clone(), false);
    let stop = Arc::new(AtomicBool::new(false));
    let running_stop = stop.clone();
    let owner = thread::spawn(move || run(&config(10_000), clock, io, running_stop));
    thread::sleep(Duration::from_millis(30));
    stop.store(true, Ordering::Release);
    let report: Report = owner.join().unwrap().unwrap();
    assert!(report.enabled);
}
