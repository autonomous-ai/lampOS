//! Real selected Voice child, private IPC and Bus; switch observations are
//! synthetic. This needs Linux's kernel boot clock, with no GPIO or audio I/O.
//! Host scheduling/filesystem stalls remain failures under the production
//! 50 ms freshness and 60 ms stable-HIGH rules; no timing bound is widened.
//! One exception: a test that fails closed while its own Controls producer
//! was held up past 50 ms, at the failure or shortly before it, is begun
//! again, three runs at most (`on_time`). A failure that permits or revokes
//! late is never begun again.
//! Producer work has a 10 s lifetime and checks stop every 5 ms; Drop joins it.
//! Local Bus filesystem calls, like Worker kill/reap, have no kernel deadline.
#![cfg(target_os = "linux")]

use lamp_bus::{Bus, Subscriber};
use lamp_controls::privacy::selected_bus;
use lamp_ipc::monotonic_us;
use lamp_msgs::{
    ClockStamp, ControlFault, ControlsState, HoldTier, MICROPHONE_PRIVACY_FRESH_US,
    MicrophonePrivacy, PrivacySwitch,
};
use lamp_turn_owner::{BootId, Controller, MonoTime, Permission};
use lamp_voice::{
    privacy::REOPEN_STABLE_US,
    privacy_bus::{GAP_RIDE_FLAG, NO_SLIDE_FLAG, ROOT_FLAG, wall_ms},
    privacy_gap::RIDE_US,
    process::{Worker, boot_text, new_boot},
    wire::{Control, WorkerEvent},
};
use std::{
    any::Any,
    cell::RefCell,
    fs, io,
    os::unix::fs::DirBuilderExt,
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(2);
const SAMPLE: Duration = Duration::from_millis(5);

fn now() -> MonoTime {
    MonoTime::from_micros(monotonic_us())
}

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // Keep Unix socket names below the transport's path bound.
        let path = PathBuf::from(format!(
            "/tmp/vpw-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path.join("ipc"))
            .unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        // This private directory is created by this test, after all its owned
        // threads, children and endpoints have been stopped/dropped.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Debug, Default)]
struct Production {
    samples: u64,
    dropped: u64,
    /// The first sample's time and the last's, in µs.
    first_us: u64,
    last_us: u64,
}

impl Production {
    /// The producer sampled through the scenario from `from_us` to `to_us`:
    /// its samples span at least 80% of it, at least half as many as that
    /// span allows at one each [`SAMPLE`]. A fixed count, as before, held only
    /// for a scenario of a given length: the no-slide test's lasts about
    /// 100 ms and took 21 or 22 samples against "more than 20" (#1569 row 46).
    fn kept_up(&self, from_us: u64, to_us: u64) -> bool {
        let span_us = self.last_us - self.first_us;
        span_us * 5 >= to_us.saturating_sub(from_us) * 4
            && self.samples * 2 * SAMPLE.as_micros() as u64 >= span_us
    }
}

/// A steady rate over part of the scenario is not keeping up through it.
#[test]
fn a_producer_kept_up_only_if_its_samples_span_the_scenario() {
    let produced = |samples, first_us, last_us| Production {
        samples,
        dropped: 0,
        first_us,
        last_us,
    };
    assert!(produced(21, 1_000, 101_000).kept_up(0, 110_000));
    assert!(!produced(6, 1_000, 31_000).kept_up(0, 110_000));
    assert!(!produced(5, 1_000, 101_000).kept_up(0, 110_000));
}

struct Producer {
    muted: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    publishing: Arc<AtomicBool>,
    publication_paused: Arc<AtomicBool>,
    publish_one: Arc<AtomicBool>,
    published: Arc<AtomicU64>,
    last_us: Arc<AtomicU64>,
    first_low_us: Arc<AtomicU64>,
    /// How the host held the test up, as it went ([`on_time`]).
    held_up: HeldUp,
    thread: Option<JoinHandle<io::Result<Production>>>,
}

/// How far a failure may come after the producer's last gap past 50 ms
/// and still be that gap's: a Controls message lives 300 ms (its
/// `expires_ms`), and the rig reads the worker's feedback every millisecond.
const SHORTLY_US: u64 = 500_000;

/// What the rig's failures read as when the worker fails closed: the
/// worker's fault for a switch it can't read, a gap in the lease stream
/// this rig reads, and the worker's clean end after either, seen as its
/// exit or as its control socket refusing the rig. Under a 150 ms stall of
/// the producer, 2 of 5 runs failed only as that refusal. A worker killed by
/// a signal between the rig's `check_running` and its send reads as that
/// refusal too, so under a producer stall such a crash is begun again; it
/// still opens nothing (the reviewer on #2255).
const FAIL_CLOSED: [&str; 4] = [
    lamp_voice::privacy_bus::UNREADABLE,
    "privacy lease stream lost continuity",
    "privacy worker exited: exit status: 0",
    "privacy.c.p: Connection refused",
];

/// The test's own producer, as the host ran it: when it last sampled, and
/// its last gap between two samples longer than the physical reading lasts
/// (when that gap ended, and its length). [`Rig`]'s `Drop` takes these at
/// the moment a scenario fails, before the unwind stops anything.
#[derive(Clone, Default)]
struct HeldUp {
    last_sample_us: Arc<AtomicU64>,
    long_gap: Arc<Mutex<Option<(u64, u64)>>>,
    at_failure: Arc<Mutex<Option<AtFailure>>>,
}

#[derive(Clone, Copy)]
struct AtFailure {
    failed_us: u64,
    last_sample_us: u64,
    long_gap: Option<(u64, u64)>,
}

impl HeldUp {
    fn sampled(&self, at_us: u64, gap_us: Option<u64>) {
        if let Some(gap_us) = gap_us.filter(|gap| *gap > MICROPHONE_PRIVACY_FRESH_US) {
            *self.long_gap.lock().unwrap() = Some((at_us, gap_us));
        }
        self.last_sample_us.store(at_us, Ordering::Release);
    }

    /// The scenario failed now: keep what the producer had done until now.
    fn failed(&self) {
        let mut at_failure = self.at_failure.lock().unwrap();
        if at_failure.is_none() {
            *at_failure = Some(AtFailure {
                failed_us: monotonic_us(),
                last_sample_us: self.last_sample_us.load(Ordering::Acquire),
                long_gap: *self.long_gap.lock().unwrap(),
            });
        }
    }

    /// The producer's gap past the physical reading's 50 ms that explains
    /// the failure, in µs: one still open when it failed, or one that ended
    /// at most [`SHORTLY_US`] before it.
    fn producer_gap_at_failure(&self) -> Option<u64> {
        let at = (*self.at_failure.lock().unwrap())?;
        let open_us = at.failed_us.saturating_sub(at.last_sample_us);
        if at.last_sample_us != 0 && open_us > MICROPHONE_PRIVACY_FRESH_US {
            return Some(open_us);
        }
        at.long_gap
            .filter(|(ended_us, _)| at.failed_us.saturating_sub(*ended_us) <= SHORTLY_US)
            .map(|(_, gap_us)| gap_us)
    }
}

thread_local! {
    /// The running scenario's producer, for [`on_time`] to read once the
    /// scenario has failed and its rig is gone.
    static RUNNING: RefCell<Option<HeldUp>> = const { RefCell::new(None) };
}

fn failure_text(failure: &(dyn Any + Send)) -> &str {
    failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .unwrap_or("")
}

/// Runs `scenario`, and begins it again, three times at most in all, only
/// when it failed closed ([`FAIL_CLOSED`]) while the test's own Controls
/// producer was held up past the physical reading's 50 ms, at the failure
/// or shortly before it ([`HeldUp::producer_gap_at_failure`]). That makes
/// the real worker say, rightly, that the switch can't be read (#1569 row
/// 45, ARM64 run 39375553918). Every other failure fails at once: one that
/// permits or revokes late, whatever the host did, even right after a stall
/// (the text decides that, not the time); a spawn that panics in
/// `Rig::start` (a respawn in `restart_verified` that exits 0 while the
/// producer is held up is begun again, as it fails closed); the
/// cooperative exit and the old lease's expiry around a restart; and a
/// fail-closed one with the producer on time, such as a worker that never
/// came up while the producer ran on time. One that comes three times
/// fails. No timing bound is widened.
fn on_time(name: &str, scenario: impl Fn()) {
    for attempt in 1..=3 {
        RUNNING.with(|running| running.borrow_mut().take());
        let Err(failure) = panic::catch_unwind(AssertUnwindSafe(&scenario)) else {
            return;
        };
        let held = RUNNING.with(|running| running.borrow_mut().take());
        let text = failure_text(failure.as_ref());
        let closed = FAIL_CLOSED.iter().any(|fault| text.contains(fault));
        match held.as_ref().and_then(HeldUp::producer_gap_at_failure) {
            Some(gap_us) if closed && attempt < 3 => eprintln!(
                "{name}: attempt {attempt} failed closed while the test's producer \
                 was held up {} ms: begun again",
                gap_us / 1_000
            ),
            _ => panic::resume_unwind(failure),
        }
    }
}

impl Producer {
    /// With `no_slide`, each state says no slide is wired and carries no
    /// slide evidence, as Controls with `LAMP_PRIVACY_CONTROLS=no-slide`.
    fn start(bus: &Bus, clock: String, no_slide: bool) -> Self {
        Self::start_paired(bus, clock, no_slide, false)
    }

    /// With `paired`, two states are published at a time, one right after
    /// the other, so a reader of the topic's kept value skips one each time.
    fn start_paired(bus: &Bus, clock: String, no_slide: bool, paired: bool) -> Self {
        let mut publisher = bus
            .publisher_declared(
                ControlsState::TOPIC,
                &ControlsState::declaration(),
                "controls",
            )
            .unwrap();
        let session = publisher.boot().to_owned();
        let mut source = Self {
            muted: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            publishing: Arc::new(AtomicBool::new(true)),
            publication_paused: Arc::new(AtomicBool::new(false)),
            publish_one: Arc::new(AtomicBool::new(false)),
            published: Arc::new(AtomicU64::new(0)),
            last_us: Arc::new(AtomicU64::new(0)),
            first_low_us: Arc::new(AtomicU64::new(0)),
            held_up: HeldUp::default(),
            thread: None,
        };
        let (muted, stop, published, last_us, first_low_us, held_up) = (
            source.muted.clone(),
            source.stop.clone(),
            source.published.clone(),
            source.last_us.clone(),
            source.first_low_us.clone(),
            source.held_up.clone(),
        );
        let (publishing, publication_paused, publish_one) = (
            source.publishing.clone(),
            source.publication_paused.clone(),
            source.publish_one.clone(),
        );
        source.thread = Some(thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut result = Production::default();
            let mut previous_us = None;
            let mut first_us = None;
            while !stop.load(Ordering::Acquire) {
                if Instant::now() >= deadline {
                    return Err(io::Error::other(
                        "synthetic producer exceeded its 10 s lifetime",
                    ));
                }
                let began = Instant::now();
                let at_us = monotonic_us();
                held_up.sampled(
                    at_us,
                    previous_us.map(|previous| at_us.saturating_sub(previous)),
                );
                previous_us = Some(at_us);
                result.first_us = *first_us.get_or_insert(at_us);
                result.last_us = at_us;
                result.samples += 1;
                let muted = muted.load(Ordering::Acquire);
                if muted {
                    let _ = first_low_us.compare_exchange(
                        0,
                        at_us,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
                let stamp = ClockStamp {
                    clock_id: clock.clone(),
                    at_us,
                    sequence: result.samples,
                };
                let state = ControlsState {
                    stamp: stamp.clone(),
                    session_id: session.clone(),
                    enabled: false,
                    ready: false,
                    fault: Some(ControlFault::Missing),
                    buttons: vec![],
                    head_active: false,
                    base_ready: false,
                    base_held_us: None,
                    base_tier: HoldTier::None,
                    latest_stop: None,
                    latest_mute: None,
                    event_sequence: 0,
                    rejected_inputs: 0,
                    privacy: (!no_slide).then(|| PrivacySwitch {
                        stamp,
                        reader_session_id: "synthetic-slide".into(),
                        muted: Some(muted),
                        fault: None,
                    }),
                    no_privacy_slide: no_slide,
                };
                state.check().map_err(io::Error::other)?;
                // A cached path can disappear during verified worker stop.
                // Active delivery gaps are checked by the real Voice fence.
                let sending = publishing.load(Ordering::Acquire);
                if sending || publish_one.swap(false, Ordering::AcqRel) {
                    result.dropped += publisher.publish_json(at_us, &state)?.dropped as u64;
                    last_us.store(at_us, Ordering::Release);
                    published.store(result.samples, Ordering::Release);
                }
                // Acquisition continues while publication is held. The next
                // publication is fresh, from the same reader and publisher;
                // only the worker's delivery stream has been interrupted.
                publication_paused.store(!sending, Ordering::Release);
                if !paired || result.samples % 2 == 0 {
                    thread::sleep(SAMPLE.saturating_sub(began.elapsed()));
                }
            }
            Ok(result)
        }));
        // Publish repeatedly before Voice binds: exercise delayed subscriber
        // discovery through the production selected_bus setting.
        let deadline = Instant::now() + WAIT;
        while source.published.load(Ordering::Acquire) < 3 {
            assert!(
                Instant::now() < deadline,
                "synthetic producer did not start"
            );
            thread::sleep(Duration::from_millis(1));
        }
        source
    }

    /// Stop publication after any in-flight publish, keeping acquisition and
    /// publisher ownership alive. Return the last published acquisition.
    fn pause_publication(&self) -> u64 {
        self.publishing.store(false, Ordering::Release);
        let deadline = Instant::now() + WAIT;
        while !self.publication_paused.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "producer did not pause publication"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.last_us.load(Ordering::Acquire)
    }

    fn resume_publication(&self) {
        self.publishing.store(true, Ordering::Release);
    }

    /// Supply one new acquisition during a pause, then remain paused. This
    /// cannot provide the full 60 ms of continuous HIGH required to reopen.
    fn publish_once(&self) -> u64 {
        assert!(self.publication_paused.load(Ordering::Acquire));
        let previous = self.published.load(Ordering::Acquire);
        self.publish_one.store(true, Ordering::Release);
        let deadline = Instant::now() + WAIT;
        while self.published.load(Ordering::Acquire) == previous {
            assert!(
                Instant::now() < deadline,
                "producer did not publish its single sample"
            );
            thread::sleep(Duration::from_millis(1));
        }
        self.last_us.load(Ordering::Acquire)
    }

    fn finish(&mut self) -> Production {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap().unwrap()
    }
}
impl Drop for Producer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Rig {
    worker: Option<Worker>,
    subscriber: Option<Subscriber>,
    producer: Producer,
    controller: Controller,
    boot: BootId,
    clock: String,
    last_authority: Option<Instant>,
    authority_until: u64,
    first_high: Option<u64>,
    first_high_after_gap: Option<u64>,
    unavailable_since: Vec<u64>,
    last_low_event: Option<u64>,
    last_lease: Option<MicrophonePrivacy>,
    retired_session: Option<String>,
    no_slide: bool,
    gap_ride: bool,
    root: Root,
}
/// The Controls a rig's worker reads.
#[derive(Clone, Copy)]
enum Controls {
    /// One state each 5 ms, on the selected bus: the worker is found at once.
    Steady,
    /// Two states at a time on a bus that never looks for subscribers again
    /// after its first publication, before the worker starts: it never sends
    /// the worker a datagram, and each kept value the worker reads after its
    /// first skips one.
    KeptOnly,
    /// Two states at a time on the selected bus: the worker is found, and has
    /// both of each pair as datagrams.
    Paired,
}

impl Rig {
    fn start(permission: Permission) -> Self {
        Self::start_with(permission, false)
    }

    fn start_with(permission: Permission, no_slide: bool) -> Self {
        Self::start_from(permission, no_slide, Controls::Steady)
    }

    /// Starts the worker beside the Controls `controls` says ([`Controls`]).
    fn start_from(permission: Permission, no_slide: bool, controls: Controls) -> Self {
        Self::start_options(permission, no_slide, controls, false)
    }

    fn start_riding(permission: Permission, no_slide: bool) -> Self {
        Self::start_options(permission, no_slide, Controls::Steady, true)
    }

    fn start_options(
        permission: Permission,
        no_slide: bool,
        controls: Controls,
        gap_ride: bool,
    ) -> Self {
        let root = Root::new();
        let bus = selected_bus(Bus::new(root.0.join("bus")).unwrap());
        let clock = lamp_controls::runtime::clock_id(None).unwrap();
        let subscriber = Self::subscribe(&bus);
        let producer = match controls {
            Controls::Steady => Producer::start(&bus, clock.clone(), no_slide),
            Controls::KeptOnly => {
                let never_looks = Bus::new(root.0.join("bus"))
                    .unwrap()
                    .rescan_every(Duration::from_secs(3_600));
                Producer::start_paired(&never_looks, clock.clone(), no_slide, true)
            }
            Controls::Paired => Producer::start_paired(&bus, clock.clone(), no_slide, true),
        };
        RUNNING.with(|running| *running.borrow_mut() = Some(producer.held_up.clone()));
        let boot = new_boot().unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), permission)
            .unwrap();
        let worker = Self::spawn(&root, boot, no_slide, gap_ride);
        Self {
            worker: Some(worker),
            subscriber: Some(subscriber),
            producer,
            controller,
            boot,
            clock,
            last_authority: None,
            authority_until: 0,
            first_high: None,
            first_high_after_gap: None,
            unavailable_since: Vec::new(),
            last_low_event: None,
            last_lease: None,
            retired_session: None,
            no_slide,
            gap_ride,
            root,
        }
    }

    fn subscribe(bus: &Bus) -> Subscriber {
        bus.subscribe_typed(MicrophonePrivacy::TOPIC, "test", MicrophonePrivacy::TYPE)
            .unwrap()
    }

    fn spawn(root: &Root, boot: BootId, no_slide: bool, gap_ride: bool) -> Worker {
        let bus = root.0.join("bus");
        let mut arguments = vec![ROOT_FLAG, bus.to_str().unwrap()];
        if no_slide {
            arguments.push(NO_SLIDE_FLAG);
        }
        if gap_ride {
            arguments.push(GAP_RIDE_FLAG);
        }
        Worker::spawn(
            Path::new(env!("CARGO_BIN_EXE_lamp-voice")),
            &root.0.join("ipc"),
            "privacy",
            boot,
            &arguments,
        )
        .unwrap()
    }

    fn permission(&mut self, permission: Permission) {
        self.controller
            .set_microphone_permission(now(), permission)
            .unwrap();
        self.last_authority = None;
    }

    fn note_feedback(&mut self, event: WorkerEvent) -> io::Result<()> {
        match event {
            WorkerEvent::Privacy {
                acquired_at_us,
                muted,
                ..
            } => {
                if muted {
                    self.last_low_event = Some(acquired_at_us);
                } else {
                    self.first_high.get_or_insert(acquired_at_us);
                    if !self.unavailable_since.is_empty() {
                        self.first_high_after_gap.get_or_insert(acquired_at_us);
                    }
                }
            }
            WorkerEvent::PrivacyUnavailable { since_us } => {
                assert!(self.gap_ride, "default worker unexpectedly rode an expiry");
                self.unavailable_since.push(since_us);
                self.first_high_after_gap = None;
            }
            other => {
                return Err(io::Error::other(format!("unexpected feedback: {other:?}")));
            }
        }
        Ok(())
    }

    fn feedback(&mut self) -> io::Result<()> {
        for count in 0..32 {
            let Some(event) = self
                .worker
                .as_mut()
                .unwrap()
                .channels
                .control
                .receive::<WorkerEvent>()?
            else {
                break;
            };
            self.note_feedback(event)?;
            if count == 31 {
                return Err(io::Error::other("fixture feedback drain overflow"));
            }
        }
        Ok(())
    }

    fn tick(&mut self) -> io::Result<Vec<MicrophonePrivacy>> {
        let worker = self.worker.as_mut().unwrap();
        worker.check_running()?;
        if self
            .last_authority
            .is_none_or(|at| at.elapsed() >= Duration::from_millis(10))
        {
            let snapshot = self.controller.snapshot(now()).map_err(io::Error::other)?;
            worker
                .channels
                .control
                .send(Control::Authority { snapshot })?;
            self.authority_until = snapshot.expires_at().as_micros();
            self.last_authority = Some(Instant::now());
        }
        self.feedback()?;
        let values = self.leases()?;
        // Feedback is sent before its corresponding lease. It may have
        // arrived between the first drain and the lease read, so take it
        // again before callers inspect the lease and its source event.
        let feedback = self.feedback();
        self.audit_grants(&values);
        feedback?;
        Ok(values
            .into_iter()
            .filter_map(|(value, fresh)| fresh.then_some(value))
            .collect())
    }

    /// Keep every valid lease for the grant audit, even one no longer fresh
    /// enough to satisfy a wait. An observer stall cannot hide an early grant.
    fn leases(&mut self) -> io::Result<Vec<(MicrophonePrivacy, bool)>> {
        if let Some(retired) = &self.retired_session {
            // The new observer is already bound, but must not consume the
            // old retained file before the replacement publishes. Publication
            // sends datagrams before retaining, so its fresh queue comes first.
            let current = Bus::new(self.root.0.join("bus"))?.latest(MicrophonePrivacy::TOPIC)?;
            if current.is_none_or(|message| message.header.boot == *retired) {
                return Ok(Vec::new());
            }
            self.retired_session = None;
        }
        let mut values = Vec::new();
        for count in 0..32 {
            let Some(message) = self.subscriber.as_mut().unwrap().recv(Duration::ZERO)? else {
                break;
            };
            if message.gap != 0 || message.publisher_restarted {
                return Err(io::Error::other("privacy lease stream lost continuity"));
            }
            let now = monotonic_us();
            let (value, fresh) =
                match MicrophonePrivacy::from_message(&message, &self.clock, now, wall_ms()) {
                    Ok(value) => (value, true),
                    // Sound as of when it was published, and only too old by
                    // now: this reader was held up past the lease's 50 ms on a
                    // loaded machine. It cannot satisfy a wait, but its order
                    // and the permission it granted are still audited (#1544).
                    Err(error) => match MicrophonePrivacy::from_message(
                        &message,
                        &self.clock,
                        message.header.at_us,
                        message.header.at_unix_ms,
                    ) {
                        Ok(value) if message.header.at_us < now => (value, false),
                        _ => return Err(io::Error::other(error)),
                    },
                };
            if value.controller_boot != boot_text(self.boot)
                || self.last_lease.as_ref().is_some_and(|last| {
                    last.session_id != value.session_id
                        || value.stamp.sequence != last.stamp.sequence + 1
                        || value.stamp.at_us < last.stamp.at_us
                })
            {
                return Err(io::Error::other("privacy lease identity/order changed"));
            }
            self.last_lease = Some(value.clone());
            values.push((value, fresh));
            if count == 31 {
                return Err(io::Error::other("fixture lease drain overflow"));
            }
        }
        Ok(values)
    }

    fn audit_grants(&self, values: &[(MicrophonePrivacy, bool)]) {
        let Some(&since) = self.unavailable_since.last() else {
            return;
        };
        for (value, _) in values {
            // A queued grant published before expiry belongs to the preceding
            // valid interval, even when it is read after the unavailable event.
            if value.permitted && value.stamp.at_us >= since {
                let first = self
                    .first_high_after_gap
                    .expect("recovery granted without fresh HIGH");
                assert!(
                    value
                        .stamp
                        .at_us
                        .checked_sub(first)
                        .is_some_and(|age| age >= REOPEN_STABLE_US),
                    "recovery granted before a full stable HIGH interval"
                );
            }
        }
    }

    fn until(&mut self, predicate: impl Fn(&MicrophonePrivacy) -> bool) -> MicrophonePrivacy {
        let deadline = Instant::now() + WAIT;
        loop {
            for value in self.tick().unwrap() {
                if predicate(&value) {
                    return value;
                }
            }
            assert!(
                Instant::now() < deadline,
                "expected privacy lease never arrived"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn observe(&mut self, duration: Duration, check: impl Fn(&MicrophonePrivacy)) {
        let until = Instant::now() + duration;
        let mut seen = 0;
        while Instant::now() < until {
            for value in self.tick().unwrap() {
                check(&value);
                seen += 1;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert!(seen > 0, "observation interval had no renewed leases");
    }

    fn observe_through(&mut self, at_us: u64, check: impl Fn(&MicrophonePrivacy)) {
        while monotonic_us() < at_us {
            for value in self.tick().unwrap() {
                check(&value);
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// Continue supplying controller authority until the child's own clean
    /// unreadable exit. The deadline belongs to the scenario, not the child;
    /// a Stop is sent only after its exit has already been observed.
    fn unreadable_exit(&mut self, deadline_us: u64, check: impl Fn(&MicrophonePrivacy)) -> u64 {
        let mut faults = Vec::new();
        let exited_us = loop {
            if let Err(error) = self.worker.as_mut().unwrap().check_running() {
                assert!(Worker::ended_cleanly(&error), "{error}");
                break monotonic_us();
            }
            match self.tick() {
                Ok(values) => {
                    for value in values {
                        check(&value);
                    }
                }
                Err(error)
                    if error
                        .to_string()
                        .contains(lamp_voice::privacy_bus::UNREADABLE) =>
                {
                    faults.push(error.to_string());
                }
                Err(error) => {
                    assert!(
                        Worker::ended_cleanly(&error)
                            || matches!(
                                error.kind(),
                                io::ErrorKind::BrokenPipe
                                    | io::ErrorKind::ConnectionReset
                                    | io::ErrorKind::ConnectionRefused
                                    | io::ErrorKind::NotConnected
                            ),
                        "{error}"
                    );
                }
            }
            assert!(
                monotonic_us() < deadline_us,
                "worker survived the expiry deadline"
            );
            thread::sleep(Duration::from_millis(1));
        };
        assert!(
            exited_us < deadline_us,
            "worker exited after the expiry deadline"
        );
        let mut worker = self.worker.take().unwrap();
        while let Ok(Some(event)) = worker.channels.control.receive::<WorkerEvent>() {
            match event {
                WorkerEvent::Fault { code } => faults.push(code),
                other => self.note_feedback(other).unwrap(),
            }
        }
        // The child can publish between the last live drain and try_wait.
        // Once exit is confirmed, both channels are complete and all final
        // leases are checked, including leases aged while the host was busy.
        let values = self.leases().unwrap();
        self.audit_grants(&values);
        for (value, _) in values {
            check(&value);
        }
        // A final fault can age out before a loaded host reads it. Any fault
        // still readable must be UNREADABLE, and exit status must be zero.
        assert!(
            faults
                .iter()
                .all(|fault| fault.contains(lamp_voice::privacy_bus::UNREADABLE)),
            "{faults:?}"
        );
        worker
            .shutdown()
            .expect("an unreadable switch ends cleanly");
        exited_us
    }

    fn stop_worker(&mut self) {
        let mut worker = self.worker.take().unwrap();
        worker
            .shutdown()
            .expect("recovery requires verified cooperative exit");
        drop(worker);
    }

    fn restart_verified(&mut self) {
        self.stop_worker();
        let last = self.last_lease.as_ref().unwrap();
        let deadline = Instant::now() + WAIT;
        while last.fresh_at(&self.clock, monotonic_us()) {
            assert!(Instant::now() < deadline, "old lease did not expire");
            thread::sleep(Duration::from_millis(1));
        }
        self.retired_session = Some(last.session_id.clone());
        self.subscriber.take();
        self.subscriber = Some(Self::subscribe(&selected_bus(
            Bus::new(self.root.0.join("bus")).unwrap(),
        )));
        self.last_lease = None;
        self.first_high = None;
        self.last_authority = None;
        self.worker = Some(Self::spawn(
            &self.root,
            self.boot,
            self.no_slide,
            self.gap_ride,
        ));
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        // What the producer had done when the scenario failed, before the
        // unwind stops anything ([`on_time`]).
        if thread::panicking() {
            self.producer.held_up.failed();
        }
        // Worker Drop kills/reaps its owned child on assertion failure. The
        // bounded producer is always stopped and joined, even during unwind.
        self.worker.take();
        self.producer.stop.store(true, Ordering::Release);
        if let Some(thread) = self.producer.thread.take() {
            let _ = thread.join();
        }
        self.subscriber.take();
    }
}

#[test]
fn selected_worker_requires_authority_latches_low_and_recovers_only_after_verified_stop() {
    on_time(
        "selected_worker_requires_authority_latches_low_and_recovers_only_after_verified_stop",
        || {
            let mut rig = Rig::start(Permission::Denied);
            rig.until(|value| value.physical_muted == Some(false));
            // Refused by the missing authority, not by evidence gone stale: the
            // worker still reads the switch as fresh and HIGH (#1241).
            rig.observe(Duration::from_millis(80), |value| {
                assert!(!value.permitted);
                assert_eq!(value.physical_muted, Some(false));
            });
            rig.permission(Permission::Allowed);
            let allowed = rig.until(|value| value.permitted);
            assert!(allowed.stamp.at_us - rig.first_high.unwrap() >= REOPEN_STABLE_US);
            rig.producer.muted.store(true, Ordering::Release);
            let low = rig.until(|value| value.physical_muted == Some(true));
            assert!(
                !low.permitted,
                "the first LOW lease must revoke without debounce"
            );
            assert_eq!(low.microphone_generation, allowed.microphone_generation);
            let low_acquired = rig.producer.first_low_us.load(Ordering::Acquire);
            assert!(low_acquired > 0);
            assert!(
                low.stamp.at_us - low_acquired < MICROPHONE_PRIVACY_FRESH_US,
                "LOW publication must revoke within the original physical freshness window"
            );
            // Keep the old Allowed authority current: only the physical LOW retires
            // capture, and subsequent stable HIGH must not revive this same child.
            rig.producer.muted.store(false, Ordering::Release);
            let high = rig.until(|value| value.physical_muted == Some(false));
            assert!(!high.permitted);
            assert!(
                rig.last_low_event.is_some(),
                "LOW must reach the private owner channel"
            );
            // Refused by the latch, not by evidence gone stale (#1241).
            rig.observe(Duration::from_millis(100), |value| {
                assert!(!value.permitted);
                assert_eq!(value.physical_muted, Some(false));
                assert_eq!(value.microphone_generation, allowed.microphone_generation);
            });
            rig.restart_verified();
            let recovered = rig.until(|value| value.permitted);
            assert_ne!(recovered.session_id, allowed.session_id);
            assert!(recovered.stamp.at_us - rig.first_high.unwrap() >= REOPEN_STABLE_US);
            rig.stop_worker();
            let produced = rig.producer.finish();
            eprintln!("synthetic Controls production including verified replacement: {produced:?}");
            assert!(produced.samples > 20);
        },
    );
}

/// #1569 row 45 (Third's P3 2 on #2244, pinning `serve`'s `resync`): until a
/// datagram comes, the worker takes Controls' kept value past a gap. This
/// Controls never finds the worker and puts two states down at a time, so
/// each kept value the worker reads after its first is one past the last.
/// It reads on until, with no datagram for twice the topic's 100 ms, the bus
/// says Controls is late and the worker ends, unreadable. With the gap
/// counted, its second read would end it as discontinuous.
#[test]
fn before_any_datagram_the_worker_reads_the_kept_value_past_gaps_until_controls_is_late() {
    let mut rig = Rig::start_from(Permission::Denied, false, Controls::KeptOnly);
    let mut observed = std::collections::BTreeSet::new();
    let deadline = Instant::now() + WAIT;
    loop {
        if rig.worker.as_mut().unwrap().check_running().is_err() {
            break;
        }
        match rig.tick() {
            Ok(values) => observed.extend(
                values
                    .iter()
                    .map(|value| value.observed_us)
                    .filter(|at| *at > 0),
            ),
            Err(error) => {
                let text = error.to_string();
                assert!(!text.contains("discontinuous"), "{text}");
                assert!(
                    !text.contains("Fault") || text.contains(lamp_voice::privacy_bus::UNREADABLE),
                    "a fault other than the switch's: {text}"
                );
                if !text.contains(lamp_voice::privacy_bus::UNREADABLE) {
                    break;
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "the worker read on with no datagram from Controls"
        );
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        observed.len() >= 3,
        "only {} switch readings were taken: {observed:?}",
        observed.len()
    );
}

/// The reviewer on #2267 (P3, pinning the datagram count at `serve`'s
/// `resyncs`): once a datagram has come, a gap ends the worker as
/// discontinuous, in its first second as after it. Controls puts two states
/// down at a time and finds the worker; then the worker's socket is moved out
/// of the topic's `subs/`, so Controls forgets it (its send finds no file)
/// and the worker has only the kept value, one past the last each time.
/// Taken as from a worker with no datagram yet, that gap would pass, and the
/// worker would end only when Controls is late, 200 ms on.
#[test]
fn after_its_first_datagram_a_gap_ends_the_worker_as_discontinuous() {
    let mut rig = Rig::start_from(Permission::Denied, false, Controls::Paired);
    rig.until(|value| value.observed_us > 0);
    // Datagrams flowing, both states of each pair.
    rig.observe(Duration::from_millis(50), |_| {});
    let subs = rig
        .root
        .0
        .join("bus")
        .join(ControlsState::TOPIC)
        .join("subs");
    let mut moved = 0;
    for entry in fs::read_dir(&subs).unwrap() {
        let path = entry.unwrap().path();
        fs::rename(&path, rig.root.0.join(format!("aside-{moved}"))).unwrap();
        moved += 1;
    }
    assert!(moved > 0, "no worker socket in {subs:?}");
    let aside = Instant::now();
    let mut said = Vec::new();
    loop {
        if rig.worker.as_mut().unwrap().check_running().is_err() {
            break;
        }
        if let Err(error) = rig.tick() {
            said.push(error.to_string());
        }
        assert!(aside.elapsed() < WAIT, "the worker read on past the gap");
        thread::sleep(Duration::from_millis(1));
    }
    let ended_after = aside.elapsed();
    let mut worker = rig.worker.take().unwrap();
    // A fault said just before the exit may still be queued, and fresh.
    while let Ok(Some(event)) = worker.channels.control.receive::<WorkerEvent>() {
        said.push(format!("{event:?}"));
    }
    drop(worker);
    assert!(
        !said
            .iter()
            .any(|text| text.contains("publisher unavailable")),
        "{said:?}"
    );
    // The fault can age out before it is read on a loaded runner (the exit
    // seen first), so an end well before Controls would be late is taken
    // for it too.
    assert!(
        said.iter().any(|text| text.contains("discontinuous"))
            || ended_after < Duration::from_millis(150),
        "ended {ended_after:?} after the gap, saying {said:?}"
    );
}

/// Losing Controls is a switch that cannot be read, not a crash (#795): the
/// worker ends cleanly, and its lease expires on its own whatever authority
/// the voice still holds. The clean exit is what the voice relies on; the
/// fault the worker sends first (`UNREADABLE`) is read only if it comes
/// within the channel's age limit, and any fault it sends is that one.
/// (ARM64 run 38871693811: on a loaded runner the exit was seen first and
/// the fault had aged out, so a test that required it failed.)
#[test]
fn losing_controls_ends_the_real_worker_cleanly_and_expires_its_lease() {
    let mut rig = Rig::start(Permission::Allowed);
    rig.until(|value| value.permitted);
    rig.producer.finish();
    let last_acquired = rig.producer.last_us.load(Ordering::Acquire);
    let deadline = Instant::now() + WAIT;
    let unreadable_only = |text: &str| {
        assert!(
            !text.contains("Fault") || text.contains(lamp_voice::privacy_bus::UNREADABLE),
            "a fault other than the switch's: {text}"
        );
    };
    loop {
        if rig.worker.as_mut().unwrap().check_running().is_err() {
            break;
        }
        if let Err(error) = rig.tick() {
            unreadable_only(&error.to_string());
            if error
                .to_string()
                .contains(lamp_voice::privacy_bus::UNREADABLE)
            {
                continue;
            }
            if rig.worker.as_mut().unwrap().check_running().is_err() {
                break;
            }
            // Exit may close IPC just before try_wait observes process death.
            // Linux reports that peer as gone in any of these ways; ARM64 run
            // 38726030376 saw ENOTCONN.
            assert!(
                matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::NotConnected
                ),
                "{error}"
            );
        }
        assert!(
            Instant::now() < deadline,
            "worker survived loss of physical evidence"
        );
        thread::sleep(Duration::from_millis(1));
    }
    assert!(
        monotonic_us() < rig.authority_until,
        "controller authority must still be current"
    );
    let mut worker = rig.worker.take().unwrap();
    // A fault said just before the exit may still be queued, and fresh.
    while let Ok(Some(event)) = worker.channels.control.receive::<WorkerEvent>() {
        unreadable_only(&format!("{event:?}"));
    }
    worker
        .shutdown()
        .expect("an unreadable switch ends the worker cleanly");
    drop(worker);
    let retained: MicrophonePrivacy = Bus::new(rig.root.0.join("bus"))
        .unwrap()
        .latest(MicrophonePrivacy::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    retained.check().unwrap();
    assert!(retained.expires_at_us <= last_acquired + MICROPHONE_PRIVACY_FRESH_US);
    assert!(!retained.fresh_at(&rig.clock, monotonic_us()));
}

/// `no-slide` through the real worker and its real argument: each fresh
/// marker publication is an observation read as not muted. A Denied
/// controller (the software mute) is never granted; an Allowed one is after
/// 60 ms of steady markers, the lease bound to the publication with no
/// physical level; Denied again revokes at once.
#[test]
fn the_real_worker_with_no_slide_grants_from_its_controller_alone() {
    on_time(
        "the_real_worker_with_no_slide_grants_from_its_controller_alone",
        || {
            let from_us = monotonic_us();
            let mut rig = Rig::start_with(Permission::Denied, true);
            rig.until(|value| value.no_slide);
            rig.observe(Duration::from_millis(80), |value| {
                assert!(!value.permitted);
                assert!(value.no_slide);
                assert_eq!(value.physical_muted, None);
            });
            rig.permission(Permission::Allowed);
            let allowed = rig.until(|value| value.permitted);
            assert!(allowed.no_slide);
            assert_eq!(allowed.physical_muted, None);
            assert!(allowed.observed_us > 0 && allowed.observed_us <= allowed.stamp.at_us);
            assert!(allowed.expires_at_us <= allowed.observed_us + MICROPHONE_PRIVACY_FRESH_US);
            assert!(allowed.stamp.at_us - rig.first_high.unwrap() >= REOPEN_STABLE_US);
            rig.permission(Permission::Denied);
            let muted = rig.until(|value| !value.permitted);
            assert!(muted.no_slide);
            assert!(
                rig.last_low_event.is_none(),
                "with no slide nothing is ever observed muted"
            );
            let to_us = monotonic_us();
            rig.stop_worker();
            let produced = rig.producer.finish();
            assert!(
                produced.kept_up(from_us, to_us),
                "{produced:?} over {from_us}..{to_us}"
            );
        },
    );
}

/// The opt-in worker keeps its own publisher and subscriber through a short
/// delivery gap. Both slide and no-slide routes close their lease, say the
/// actual expiry once, and require a new full stable interval before granting.
#[test]
fn a_gap_ride_recovers_short_repeated_publication_gaps_in_the_same_worker() {
    for no_slide in [false, true] {
        on_time(
            "a_gap_ride_recovers_short_repeated_publication_gaps_in_the_same_worker",
            || {
                let mut rig = Rig::start_riding(Permission::Allowed, no_slide);
                let allowed = rig.until(|value| value.permitted);
                for attempt in 0..2 {
                    let last_acquired = rig.producer.pause_publication();
                    let closed = rig.until(|value| !value.permitted && value.observed_us == 0);
                    assert_eq!(closed.physical_muted, None);
                    assert_eq!(closed.session_id, allowed.session_id);
                    assert_eq!(closed.microphone_generation, allowed.microphone_generation);
                    assert_eq!(rig.unavailable_since.len(), attempt + 1);
                    assert_eq!(
                        rig.unavailable_since[attempt],
                        last_acquired + MICROPHONE_PRIVACY_FRESH_US
                    );
                    rig.observe_through(last_acquired + 60_000, |value| {
                        assert!(!value.permitted);
                        assert_eq!(value.observed_us, 0);
                    });
                    rig.producer.resume_publication();
                    let recovered = rig.until(|value| value.permitted);
                    assert_eq!(recovered.session_id, allowed.session_id);
                    assert_eq!(recovered.controller_boot, allowed.controller_boot);
                    assert_eq!(
                        recovered.microphone_generation,
                        allowed.microphone_generation
                    );
                    assert!(recovered.stamp.sequence > closed.stamp.sequence);
                    let first_recovered = rig.first_high_after_gap.unwrap();
                    assert!(first_recovered >= last_acquired + 60_000);
                    assert!(
                        recovered.stamp.at_us - first_recovered >= REOPEN_STABLE_US,
                        "recovery granted before a full stable HIGH interval"
                    );
                    assert_eq!(rig.unavailable_since.len(), attempt + 1);
                }
                rig.stop_worker();
            },
        );
    }
}

/// Controls is Late after 200 ms without a publication. That advisory,
/// including one delivered beside recovery, must not end an active ride.
#[test]
fn a_gap_ride_survives_controls_late_and_restabilizes_before_reopening() {
    on_time(
        "a_gap_ride_survives_controls_late_and_restabilizes_before_reopening",
        || {
            let mut rig = Rig::start_riding(Permission::Allowed, false);
            let allowed = rig.until(|value| value.permitted);
            let last_acquired = rig.producer.pause_publication();
            let closed = rig.until(|value| !value.permitted && value.observed_us == 0);
            rig.observe_through(last_acquired + 250_000, |value| {
                assert!(!value.permitted);
                assert_eq!(value.physical_muted, None);
                assert_eq!(value.session_id, allowed.session_id);
            });
            assert_eq!(
                rig.unavailable_since,
                [last_acquired + MICROPHONE_PRIVACY_FRESH_US]
            );
            rig.producer.resume_publication();
            let recovered = rig.until(|value| value.permitted);
            assert_eq!(recovered.session_id, allowed.session_id);
            assert!(recovered.stamp.sequence > closed.stamp.sequence);
            assert!(recovered.stamp.at_us - rig.first_high_after_gap.unwrap() >= REOPEN_STABLE_US);
            assert_eq!(rig.unavailable_since.len(), 1);
            rig.stop_worker();
        },
    );
}

/// One fresh HIGH during the ride cannot restart its one-second bound. The
/// worker remains closed and ends at the original acquisition-expiry deadline.
#[test]
fn an_isolated_fresh_high_does_not_extend_the_real_workers_gap_ride() {
    on_time(
        "an_isolated_fresh_high_does_not_extend_the_real_workers_gap_ride",
        || {
            let mut rig = Rig::start_riding(Permission::Allowed, false);
            let allowed = rig.until(|value| value.permitted);
            let last_acquired = rig.producer.pause_publication();
            rig.until(|value| !value.permitted && value.observed_us == 0);
            let since = last_acquired + MICROPHONE_PRIVACY_FRESH_US;
            assert_eq!(rig.unavailable_since, [since]);
            rig.observe_through(since + RIDE_US * 3 / 4, |value| {
                assert!(!value.permitted);
                assert_eq!(value.session_id, allowed.session_id);
            });
            let isolated = rig.producer.publish_once();
            let reading = rig.until(|value| value.observed_us == isolated);
            assert!(!reading.permitted);
            assert_eq!(reading.physical_muted, Some(false));
            let exited = rig.unreadable_exit(since + RIDE_US + 200_000, |value| {
                assert!(!value.permitted);
                assert_eq!(value.session_id, allowed.session_id);
            });
            // The 200 ms allowance is for observing/reaping the exit on the
            // host. It is shorter than a timer reset at the isolated sample.
            assert!(exited >= since + RIDE_US);
            assert!(exited < isolated + RIDE_US);
            assert_eq!(rig.unavailable_since, [since]);
        },
    );
}

/// The default worker still ends at expiry. Enabling the flag cannot ride a
/// gap when the controller has never allowed this conversation to capture.
#[test]
fn expiry_without_the_opt_in_or_allowed_authority_still_ends_the_worker() {
    for (ride, permission) in [(false, Permission::Allowed), (true, Permission::Denied)] {
        on_time(
            "expiry_without_the_opt_in_or_allowed_authority_still_ends_the_worker",
            || {
                let mut rig = if ride {
                    Rig::start_riding(permission, false)
                } else {
                    Rig::start(permission)
                };
                if permission == Permission::Allowed {
                    rig.until(|value| value.permitted);
                } else {
                    rig.until(|value| value.observed_us > 0);
                }
                let last_acquired = rig.producer.pause_publication();
                let expires = last_acquired + MICROPHONE_PRIVACY_FRESH_US;
                rig.unreadable_exit(expires + 200_000, |value| {
                    assert!(
                        !value.permitted || value.expires_at_us <= expires,
                        "a stale acquisition renewed capture permission"
                    );
                });
                assert!(rig.unavailable_since.is_empty());
            },
        );
    }
}

/// #4332, fault A: lamp-06c1's cold boot held the board ~0.9 s on memory,
/// the wait's Denied lease lapsed and the real worker ended with exit 1. A
/// lapse of a lease granting nothing is now ridden, with nothing published,
/// and the next authority re-arms it; nothing is ever permitted.
#[test]
fn the_real_worker_rides_a_900_ms_lapse_of_a_denied_lease() {
    on_time(
        "the_real_worker_rides_a_900_ms_lapse_of_a_denied_lease",
        || {
            let mut rig = Rig::start(Permission::Denied);
            rig.controller
                .set_camera_permission(now(), Permission::Denied)
                .unwrap();
            rig.last_authority = None;
            rig.until(|value| value.physical_muted == Some(false));
            let until = Instant::now() + Duration::from_millis(900);
            while Instant::now() < until {
                rig.worker
                    .as_mut()
                    .unwrap()
                    .check_running()
                    .expect("the worker ended on a lapse of a Denied lease");
                // Its readings still come, as the wait takes them.
                rig.feedback().unwrap();
                // The leases are read on, as Ears reads them: left queued
                // for 900 ms, the Bus passes them over as expired with no
                // gap, and the next one read seems to skip its order. None
                // is published once the held lease lapsed, while it rides.
                for (value, _) in rig.leases().unwrap() {
                    assert!(!value.permitted, "a lapsed Denied lease permitted capture");
                    assert!(
                        value.stamp.at_us < rig.authority_until,
                        "a lease was published while its authority had lapsed"
                    );
                }
                thread::sleep(Duration::from_millis(5));
            }
            rig.until(|value| value.physical_muted == Some(false));
            rig.observe(Duration::from_millis(80), |value| assert!(!value.permitted));
            rig.stop_worker();
        },
    );
}

/// A lapse of a lease that grants the microphones still ends the real
/// worker, as an error the restart guard counts, not as an unreadable switch.
#[test]
fn a_lapse_of_a_granting_lease_still_ends_the_real_worker() {
    on_time(
        "a_lapse_of_a_granting_lease_still_ends_the_real_worker",
        || {
            let mut rig = Rig::start(Permission::Allowed);
            rig.until(|value| value.permitted);
            let deadline = Instant::now() + WAIT;
            let ended = loop {
                if let Err(error) = rig.worker.as_mut().unwrap().check_running() {
                    break error;
                }
                assert!(
                    Instant::now() < deadline,
                    "the worker rode a lapse of a granting lease"
                );
                thread::sleep(Duration::from_millis(5));
            };
            assert!(!Worker::ended_cleanly(&ended), "{ended}");
            rig.worker.take();
        },
    );
}

/// Allowed authority alone does not establish a conversation to preserve.
/// With a live but silent publisher, the flagged worker must still end at
/// startup's ordinary source-expiry deadline, before granting any lease.
#[test]
fn allowed_authority_without_a_first_observation_cannot_start_a_gap_ride() {
    fn established_attempt() -> bool {
        let root = Root::new();
        let bus = selected_bus(Bus::new(root.0.join("bus")).unwrap());
        let _source = bus
            .publisher_declared(
                ControlsState::TOPIC,
                &ControlsState::declaration(),
                "controls",
            )
            .unwrap();
        let mut leases = Rig::subscribe(&bus);
        let boot = new_boot().unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let check_feedback = |event| match event {
            WorkerEvent::Fault { code } => assert!(
                code.starts_with(lamp_voice::privacy_bus::UNREADABLE),
                "{code}"
            ),
            other => panic!("worker without source evidence sent {other:?}"),
        };
        let drain_feedback = |worker: &mut Worker| {
            for count in 0..32 {
                let Some(event) = worker.channels.control.receive::<WorkerEvent>().unwrap() else {
                    break;
                };
                check_feedback(event);
                assert!(count < 31, "fixture feedback drain overflow");
            }
        };
        let check_lease = |value: &MicrophonePrivacy| {
            value.check().unwrap();
            assert!(
                !value.permitted,
                "worker granted without any source observation"
            );
            assert_eq!(value.observed_us, 0);
            assert_eq!(value.physical_muted, None);
            assert_eq!(value.controller_boot, boot_text(boot));
        };
        let drain_leases = |leases: &mut Subscriber| {
            let mut seen = false;
            for count in 0..32 {
                let Some(message) = leases.recv(Duration::ZERO).unwrap() else {
                    break;
                };
                let value: MicrophonePrivacy = message.parse().unwrap();
                check_lease(&value);
                seen = true;
                assert!(count < 31, "fixture lease drain overflow");
            }
            seen
        };
        let renew = |worker: &mut Worker, controller: &mut Controller| {
            let snapshot = controller.snapshot(now()).unwrap();
            assert_eq!(snapshot.microphone_permission(), Permission::Allowed);
            if let Err(error) = worker
                .channels
                .control
                .send(Control::Authority { snapshot })
            {
                // The expected expiry can close the peer between try_wait and
                // this renewal; the next iteration still verifies clean exit.
                assert!(
                    matches!(
                        error.kind(),
                        io::ErrorKind::BrokenPipe
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::ConnectionRefused
                            | io::ErrorKind::NotConnected
                    ),
                    "{error}"
                );
            }
        };
        let mut worker = Rig::spawn(&root, boot, false, true);
        // Install Allowed before any feedback or Bus reads. A host stall can
        // still let the correct worker expire before receiving that first lease.
        let ready_us = monotonic_us();
        renew(&mut worker, &mut controller);
        let mut last_authority = Instant::now();
        // Allow 200 ms only to observe the 50 ms startup exit on the host. A
        // mistaken one-second ride cannot fit within this observation window.
        let deadline_us = ready_us + MICROPHONE_PRIVACY_FRESH_US + 200_000;
        let mut seen_closed = false;
        let exited_us = loop {
            drain_feedback(&mut worker);
            seen_closed |= drain_leases(&mut leases);
            if let Err(error) = worker.check_running() {
                assert!(Worker::ended_cleanly(&error), "{error}");
                break monotonic_us();
            }
            assert!(
                monotonic_us() < deadline_us,
                "worker rode startup without a first grant"
            );
            if last_authority.elapsed() >= Duration::from_millis(10) {
                renew(&mut worker, &mut controller);
                last_authority = Instant::now();
            }
            thread::sleep(Duration::from_millis(1));
        };
        // Exit can race the preceding reads. Inspect both queues and the kept
        // lease before deciding whether this attempt established its precondition.
        drain_feedback(&mut worker);
        seen_closed |= drain_leases(&mut leases);
        if let Some(message) = bus.latest(MicrophonePrivacy::TOPIC).unwrap() {
            let value: MicrophonePrivacy = message.parse().unwrap();
            check_lease(&value);
            seen_closed = true;
        }
        worker
            .shutdown()
            .expect("missing startup evidence ends cleanly");
        if seen_closed {
            assert!(
                exited_us < deadline_us,
                "startup exit was observed too late"
            );
        }
        seen_closed
    }

    for attempt in 1..=3 {
        if established_attempt() {
            return;
        }
        // Only a clean exit with no Allowed-authority publication reaches
        // here. Wrong feedback, any grant, an abnormal exit, and an established
        // attempt's missed deadline all fail immediately inside the attempt.
        assert!(
            attempt < 3,
            "worker never established the Allowed-authority startup precondition"
        );
        eprintln!(
            "startup privacy attempt {attempt}: clean exit before Allowed authority was established; retrying"
        );
    }
}

/// A real LOW during recovery revokes immediately and retires this child.
/// Later stable HIGH cannot turn the bounded ride into a bypass of that latch.
#[test]
fn fresh_low_during_a_gap_ride_never_grants_the_same_worker_again() {
    on_time(
        "fresh_low_during_a_gap_ride_never_grants_the_same_worker_again",
        || {
            let mut rig = Rig::start_riding(Permission::Allowed, false);
            let allowed = rig.until(|value| value.permitted);
            rig.producer.pause_publication();
            rig.until(|value| !value.permitted && value.observed_us == 0);
            rig.producer.muted.store(true, Ordering::Release);
            rig.producer.resume_publication();
            let low = rig.until(|value| value.physical_muted == Some(true));
            assert!(!low.permitted);
            assert_eq!(low.session_id, allowed.session_id);
            assert_eq!(low.microphone_generation, allowed.microphone_generation);
            assert!(rig.last_low_event.is_some());
            rig.producer.muted.store(false, Ordering::Release);
            rig.until(|value| value.physical_muted == Some(false));
            rig.observe(Duration::from_millis(100), |value| {
                assert!(!value.permitted);
                assert_eq!(value.session_id, allowed.session_id);
                assert_eq!(value.physical_muted, Some(false));
            });
            assert_eq!(rig.unavailable_since.len(), 1);
            rig.stop_worker();
        },
    );
}
