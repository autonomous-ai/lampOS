//! Child adapter boundaries. These do not replace the original health scenarios.
use crate::{fixture_protocol, fixture_schedule};

use fixture_protocol::{
    Channel, Checked, HOST_WAIT, Outcome, Step, marker, read_frame, write_frame,
};
use fixture_schedule::{FixtureSchedule, ProgressDriver, write_marker};
use lamp_memory::health::{
    CountSource, PublicationOutcome, REPORT_EVERY, ReportProgress, ReportSchedule, Reporter,
};
use std::{
    fs,
    io::{self, Cursor, Read, Write},
    net::Shutdown,
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering::SeqCst},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!(
            "lmfc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, SeqCst)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Opening {
    reporter: Reporter,
    schedule: Arc<FixtureSchedule>,
    driver: ProgressDriver,
    scratch: Scratch,
}

impl Opening {
    fn new() -> Self {
        let scratch = Scratch::new();
        let (schedule, driver) = FixtureSchedule::pair("case".into()).unwrap();
        let reporter = Reporter::start_with_schedule(
            scratch.0.join("bus"),
            CountSource::Days,
            schedule.clone(),
        )
        .unwrap();
        Self {
            reporter,
            schedule,
            driver,
            scratch,
        }
    }

    fn step(&mut self, round: u64, at_us: u64) -> Checked {
        self.driver
            .settle(&step(round, at_us), Instant::now() + HOST_WAIT)
            .unwrap()
    }
}

impl Drop for Opening {
    fn drop(&mut self) {
        self.schedule.close();
        self.reporter.finish(false);
    }
}

fn step(round: u64, at_us: u64) -> Step {
    Step {
        nonce: "case".into(),
        round,
        at_us,
    }
}

fn complete(schedule: &FixtureSchedule, outcome: PublicationOutcome) {
    schedule.observe(ReportProgress::CycleComplete {
        started: schedule.now(),
        publication: outcome,
    });
}

fn checked(schedule: &FixtureSchedule) {
    let at = schedule.now();
    schedule.observe(ReportProgress::WaitChecked {
        started: at,
        at,
        remaining: REPORT_EVERY,
    });
}

#[test]
fn reads_are_pure_and_equal_time_steps_do_not_create_reports() {
    let mut fixture = Opening::new();
    let now = fixture.schedule.now();
    assert_eq!(fixture.schedule.now(), now);
    let first = fixture.step(1, 0);
    assert_eq!(
        (
            first.cycles,
            first.attempts,
            first.cycle_started_us,
            first.next_due_us
        ),
        (1, 0, 0, 1_000_000)
    );
    assert_eq!(first.last_outcome, Outcome::NotAttempted);
    let again = fixture.step(2, 0);
    assert_eq!(
        (
            again.cycles,
            again.attempts,
            again.cycle_started_us,
            again.next_due_us
        ),
        (1, 0, 0, 1_000_000)
    );
    assert_eq!(fixture.schedule.now(), now);
    assert_ne!(first.marker, again.marker);
}

#[test]
fn pre_deadline_wait_preserves_the_full_period_and_due_requires_real_publication() {
    let mut fixture = Opening::new();
    let mut subscriber = lamp_bus::Bus::new(fixture.scratch.0.join("bus"))
        .unwrap()
        .subscribe_typed(
            lamp_msgs::Diagnostic::TOPIC,
            "test",
            lamp_msgs::Diagnostic::TYPE,
        )
        .unwrap();
    let first = fixture.step(1, 0);
    fixture.reporter.state().serving();
    let before = fixture.step(2, 999_999);
    assert_eq!(before.cycles, first.cycles);
    assert_eq!(before.attempts, 0);
    assert_eq!(before.next_due_us, 1_000_000);
    let due = fixture.step(3, 1_000_000);
    assert_eq!(
        (
            due.cycles,
            due.attempts,
            due.cycle_started_us,
            due.next_due_us
        ),
        (2, 1, 1_000_000, 2_000_000)
    );
    assert_eq!(due.last_outcome, Outcome::ReturnedOk);
    // The independent real socket receive is the delivery evidence, not Checked.
    let message = subscriber
        .recv(HOST_WAIT)
        .unwrap()
        .expect("actual Diagnostic datagram");
    let report: lamp_msgs::Diagnostic = message.parse().unwrap();
    assert_eq!(report.name, "memory");
}

#[test]
fn invalid_first_step_stale_round_backwards_time_and_skipped_due_fail_closed() {
    for invalid_step in [step(1, 1), step(2, 0)] {
        let mut fixture = Opening::new();
        assert!(
            fixture
                .driver
                .settle(&invalid_step, Instant::now() + HOST_WAIT)
                .is_err()
        );
        assert!(
            fixture
                .driver
                .settle(&step(1, 0), Instant::now() + HOST_WAIT)
                .is_err()
        );
    }
    for invalid_step in [
        step(1, 0),
        step(3, 0),
        step(2, 1_000_001),
        Step {
            nonce: "wrong".into(),
            ..step(2, 0)
        },
    ] {
        let mut fixture = Opening::new();
        fixture.step(1, 0);
        assert!(
            fixture
                .driver
                .settle(&invalid_step, Instant::now() + HOST_WAIT)
                .is_err()
        );
    }
    let mut fixture = Opening::new();
    fixture.step(1, 0);
    fixture.step(2, 100);
    assert!(
        fixture
            .driver
            .settle(&step(3, 99), Instant::now() + HOST_WAIT)
            .is_err()
    );
}

#[test]
fn zero_left_wait_and_observer_overflow_cannot_produce_checked() {
    let (schedule, mut driver) = FixtureSchedule::pair("case".into()).unwrap();
    complete(&schedule, PublicationOutcome::ReturnedOk);
    let at = schedule.now();
    schedule.observe(ReportProgress::WaitChecked {
        started: at,
        at,
        remaining: Duration::ZERO,
    });
    assert!(
        driver
            .settle(&step(1, 0), Instant::now() + HOST_WAIT)
            .is_err()
    );

    let (schedule, mut driver) = FixtureSchedule::pair("case".into()).unwrap();
    complete(&schedule, PublicationOutcome::NotAttempted);
    for _ in 0..128 {
        checked(&schedule);
    }
    let error = driver
        .settle(&step(1, 0), Instant::now() + HOST_WAIT)
        .unwrap_err();
    assert!(error.to_string().contains("overflow"));
    assert!(
        driver
            .settle(&step(1, 0), Instant::now() + HOST_WAIT)
            .is_err()
    );
}

/// Every spawned test worker has a completion channel and a cancellation path.
/// Join is only called after completion; failure still closes/wakes the worker.
struct Worker {
    schedule: Arc<FixtureSchedule>,
    release: Option<SyncSender<()>>,
    before_park: Arc<BeforePark>,
    done: Receiver<()>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    fn finish(&mut self) {
        if self.handle.is_none() {
            return;
        }
        if let Some(release) = self.release.take() {
            let _ = release.try_send(());
        }
        self.before_park.open();
        self.schedule.close();
        if self.done.recv_timeout(HOST_WAIT).is_ok()
            && let Some(handle) = self.handle.take()
        {
            handle.join().unwrap();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.finish();
    }
}

#[derive(Default)]
struct BeforePark {
    released: Mutex<bool>,
    changed: Condvar,
}

impl BeforePark {
    fn open(&self) {
        *self.released.lock().unwrap() = true;
        self.changed.notify_one();
    }

    fn wait(&self) -> bool {
        // This condition variable has its own wait state. A blocking mpsc
        // receive here could consume the thread park token being tested.
        let (released, _) = self
            .changed
            .wait_timeout_while(self.released.lock().unwrap(), HOST_WAIT, |released| {
                !*released
            })
            .unwrap();
        *released
    }
}

#[test]
fn held_work_then_positive_wait_is_required_and_wake_before_park_is_retained() {
    let (schedule, mut driver) = FixtureSchedule::pair("case".into()).unwrap();
    let (release, released) = mpsc::sync_channel(1);
    let (work, worked) = mpsc::sync_channel(1);
    let (park, parked) = mpsc::sync_channel(1);
    let (done, finished) = mpsc::sync_channel(1);
    let before_park = Arc::new(BeforePark::default());
    let parked_gate = before_park.clone();
    let shared = schedule.clone();
    let handle = thread::spawn(move || {
        // This models held work at the adapter boundary. The existing Reporter
        // unit test separately holds its actual publication closure.
        work.send(()).unwrap();
        if released.recv_timeout(HOST_WAIT).is_err() {
            return;
        }
        complete(&shared, PublicationOutcome::ReturnedOk);
        checked(&shared);
        park.send(thread::current()).unwrap();
        if !parked_gate.wait() {
            return;
        }
        shared.park(REPORT_EVERY);
        done.send(()).unwrap();
    });
    let mut worker = Worker {
        schedule: schedule.clone(),
        release: Some(release),
        before_park,
        done: finished,
        handle: Some(handle),
    };
    worked.recv_timeout(HOST_WAIT).unwrap();
    // No CycleComplete or positive WaitChecked has yet been observed.
    let (answer, answered) = mpsc::sync_channel(1);
    let control = thread::spawn(move || {
        let result = driver.settle(&step(1, 0), Instant::now() + HOST_WAIT);
        answer.send((result, driver)).unwrap();
    });
    assert!(matches!(answered.try_recv(), Err(TryRecvError::Empty)));
    worker.release.as_ref().unwrap().send(()).unwrap();
    let parked_thread = parked.recv_timeout(HOST_WAIT).unwrap();
    let (result, mut driver) = answered.recv_timeout(HOST_WAIT).unwrap();
    control.join().unwrap();
    let first = result.unwrap();
    assert_eq!((first.cycles, first.attempts), (1, 1));
    parked_thread.unpark();
    worker.before_park.open();
    worker.done.recv_timeout(HOST_WAIT).unwrap();
    worker.handle.take().unwrap().join().unwrap();
    // A retained token released the actual park; it did not create a cycle.
    let second = driver
        .settle(&step(2, 0), Instant::now() + HOST_WAIT)
        .unwrap();
    assert_eq!((second.cycles, second.attempts), (1, 1));
    worker.release = None;
}

#[test]
fn close_freezes_clock_and_does_not_block_the_real_reporter_finish() {
    let mut fixture = Opening::new();
    fixture.step(1, 0);
    let at = fixture.schedule.now();
    fixture.schedule.close();
    let started = Instant::now();
    fixture.reporter.finish(false);
    assert!(started.elapsed() < HOST_WAIT);
    assert_eq!(fixture.schedule.now(), at);
    assert!(
        fixture
            .driver
            .settle(&step(2, 0), Instant::now() + HOST_WAIT)
            .is_err()
    );
}

#[test]
fn malformed_oversized_and_unterminated_frames_are_rejected() {
    for bytes in [
        b"{}\n".as_slice(),
        b"{\"nonce\":\"case\",\"round\":1,\"at_us\":0,\"extra\":1}\n",
        b"{\"nonce\":\"case\",\"round\":1,\"at_us\":0}",
        b"not-json\n",
    ] {
        assert!(read_frame::<_, Step>(&mut Cursor::new(bytes)).is_err());
    }
    let large = format!("{}\n", "x".repeat(fixture_protocol::MAX_FRAME_BYTES));
    assert!(read_frame::<_, Step>(&mut Cursor::new(large)).is_err());
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &step(1, 0)).unwrap();
    let found: Step = read_frame(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(
        (found.nonce.as_str(), found.round, found.at_us),
        ("case", 1, 0)
    );
    assert!(marker("bad\nnonce", 1).is_err());
    assert!(marker("case", 0).is_err());
    assert!(marker(&"x".repeat(32), u64::MAX).unwrap().len() <= fixture_protocol::MAX_MARKER_BYTES);
}

#[test]
fn bootstrap_requires_the_explicit_private_socket_and_matching_version() {
    let scratch = Scratch::new();
    let endpoint = scratch.0.join("control");
    let _listener = UnixListener::bind(&endpoint).unwrap();
    fs::set_permissions(&endpoint, fs::Permissions::from_mode(0o600)).unwrap();
    let mut bootstrap = fixture_protocol::Bootstrap {
        version: fixture_protocol::VERSION,
        control: endpoint.clone(),
        nonce: "case".into(),
    };
    bootstrap.validate().unwrap();
    bootstrap.version += 1;
    assert!(bootstrap.validate().is_err());
    bootstrap.version = fixture_protocol::VERSION;
    fs::set_permissions(&endpoint, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(bootstrap.validate().is_err());
    fs::set_permissions(&endpoint, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(bootstrap.validate().is_err());
    fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o700)).unwrap();
    let link = scratch.0.join("link");
    std::os::unix::fs::symlink(&endpoint, &link).unwrap();
    bootstrap.control = link;
    assert!(bootstrap.validate().is_err());
}

#[test]
fn complete_queued_frame_survives_peer_close_and_eof_is_not_a_receipt() {
    let (mut writer, reader) = UnixStream::pair().unwrap();
    write_frame(&mut writer, &step(1, 0)).unwrap();
    drop(writer);
    let mut channel = Channel::new(reader).unwrap();
    let found: Step = channel.read_until(Instant::now() + HOST_WAIT).unwrap();
    assert_eq!((found.round, found.at_us), (1, 0));
    assert_eq!(
        channel
            .read_until::<Step>(Instant::now() + HOST_WAIT)
            .unwrap_err()
            .kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn an_expired_absolute_deadline_rejects_even_queued_data() {
    let (mut writer, reader) = UnixStream::pair().unwrap();
    write_frame(&mut writer, &step(1, 0)).unwrap();
    let mut channel = Channel::new(reader).unwrap();
    assert_eq!(
        channel
            .read_until::<Step>(Instant::now())
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    let found: Step = channel.read_until(Instant::now() + HOST_WAIT).unwrap();
    assert_eq!(found.round, 1);
}

#[test]
fn real_channel_shutdown_wakes_a_waiting_reader_without_fabricating_a_frame() {
    let (_peer, reader) = UnixStream::pair().unwrap();
    let mut channel = Channel::new(reader).unwrap();
    let cancel = channel.try_clone_stream().unwrap();
    let (answer, answered) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = channel.read_until::<Step>(Instant::now() + HOST_WAIT);
        let _ = answer.send(result);
    });
    cancel.shutdown(Shutdown::Both).unwrap();
    let result = answered.recv_timeout(HOST_WAIT).unwrap();
    worker.join().unwrap();
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
}

#[test]
fn actual_marker_follows_prior_bytes_through_the_real_log_relay() {
    let mut fixture = Opening::new();
    let receipt = fixture.step(1, 0);
    let (mut producer, input) = UnixStream::pair().unwrap();
    let (output, mut destination) = UnixStream::pair().unwrap();
    output.set_nonblocking(true).unwrap();
    let cancel = producer.try_clone().unwrap();
    let dropped = Arc::new(AtomicU64::new(0));
    let total = dropped.clone();
    let (done, completed) = mpsc::sync_channel(1);
    let relay = thread::spawn(move || {
        lamp_ipc::log_relay::relay(input.into(), output.into(), &total);
        let _ = done.send(());
    });
    producer.write_all(b"prior actual stream bytes\n").unwrap();
    write_marker(&mut producer, &receipt).unwrap();
    producer.shutdown(Shutdown::Write).unwrap();
    let completion = completed.recv_timeout(HOST_WAIT);
    let _ = cancel.shutdown(Shutdown::Both);
    assert!(completion.is_ok(), "real log relay did not finish");
    relay.join().unwrap();
    let mut received = String::new();
    destination.read_to_string(&mut received).unwrap();
    assert_eq!(
        received,
        format!("prior actual stream bytes\n{}", receipt.marker)
    );
    assert_eq!(dropped.load(SeqCst), 0);
    let mut forged = receipt;
    forged.marker = "unrelated\n".into();
    let mut bytes = Vec::new();
    assert!(write_marker(&mut bytes, &forged).is_err());
    assert!(bytes.is_empty());
}
