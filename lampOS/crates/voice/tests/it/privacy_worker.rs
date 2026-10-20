//! Real selected Voice child, private IPC and Bus; switch observations are
//! synthetic. This needs Linux's kernel boot clock, with no GPIO or audio I/O.
//! Host scheduling/filesystem stalls remain failures under the production
//! 50 ms freshness and 60 ms stable-HIGH rules; no timing bound is widened.
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
    privacy_bus::{NO_SLIDE_FLAG, ROOT_FLAG, wall_ms},
    process::{Worker, boot_text, new_boot},
    wire::{Control, WorkerEvent},
};
use std::{
    fs, io,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
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
}

struct Producer {
    muted: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    published: Arc<AtomicU64>,
    last_us: Arc<AtomicU64>,
    first_low_us: Arc<AtomicU64>,
    thread: Option<JoinHandle<io::Result<Production>>>,
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
            published: Arc::new(AtomicU64::new(0)),
            last_us: Arc::new(AtomicU64::new(0)),
            first_low_us: Arc::new(AtomicU64::new(0)),
            thread: None,
        };
        let (muted, stop, published, last_us, first_low_us) = (
            source.muted.clone(),
            source.stop.clone(),
            source.published.clone(),
            source.last_us.clone(),
            source.first_low_us.clone(),
        );
        source.thread = Some(thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut result = Production::default();
            while !stop.load(Ordering::Acquire) {
                if Instant::now() >= deadline {
                    return Err(io::Error::other(
                        "synthetic producer exceeded its 10 s lifetime",
                    ));
                }
                let began = Instant::now();
                let at_us = monotonic_us();
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
                result.dropped += publisher.publish_json(at_us, &state)?.dropped as u64;
                last_us.store(at_us, Ordering::Release);
                published.store(result.samples, Ordering::Release);
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
    last_low_event: Option<u64>,
    last_lease: Option<MicrophonePrivacy>,
    retired_session: Option<String>,
    no_slide: bool,
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
        let boot = new_boot().unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), permission)
            .unwrap();
        let worker = Self::spawn(&root, boot, no_slide);
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
            last_low_event: None,
            last_lease: None,
            retired_session: None,
            no_slide,
            root,
        }
    }

    fn subscribe(bus: &Bus) -> Subscriber {
        bus.subscribe_typed(MicrophonePrivacy::TOPIC, "test", MicrophonePrivacy::TYPE)
            .unwrap()
    }

    fn spawn(root: &Root, boot: BootId, no_slide: bool) -> Worker {
        let bus = root.0.join("bus");
        let mut arguments = vec![ROOT_FLAG, bus.to_str().unwrap()];
        if no_slide {
            arguments.push(NO_SLIDE_FLAG);
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
        for count in 0..32 {
            match worker.channels.control.receive::<WorkerEvent>()? {
                Some(WorkerEvent::Privacy {
                    acquired_at_us,
                    muted,
                    ..
                }) => {
                    if muted {
                        self.last_low_event = Some(acquired_at_us);
                    } else {
                        self.first_high.get_or_insert(acquired_at_us);
                    }
                }
                Some(other) => {
                    return Err(io::Error::other(format!("unexpected feedback: {other:?}")));
                }
                None => break,
            }
            if count == 31 {
                return Err(io::Error::other("fixture feedback drain overflow"));
            }
        }
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
                    // loaded machine. It is no lease any more and is passed
                    // over, but its place in the order still counts (#1544).
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
            if fresh {
                values.push(value);
            }
            if count == 31 {
                return Err(io::Error::other("fixture lease drain overflow"));
            }
        }
        Ok(values)
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
        self.worker = Some(Self::spawn(&self.root, self.boot, self.no_slide));
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
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
    rig.stop_worker();
    let produced = rig.producer.finish();
    assert!(produced.samples > 20);
}
