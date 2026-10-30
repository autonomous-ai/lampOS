use lamp_bus::{Bus, Kind};
use lamp_controls::{
    engine::Config,
    input::{InputError, Level as PinLevel, Observation, PinReader, ScriptedPins},
    mute::MuteRecord,
    receiver::Receiver,
    replay,
    runtime::{DRAIN_LIMIT, MissingPins, Service},
};
use lamp_msgs::{
    ClockStamp, ControlFault, ControlGesture, ControlsState, Diagnostic, Gesture, HoldTier, Level,
};
use std::{
    cell::Cell,
    fs, io,
    os::unix::fs::symlink,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const CLOCK: &str = "test-boot";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        // Short paths also fit Darwin's Unix-domain socket path limit.
        let path = PathBuf::from(format!(
            "/tmp/cr-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn bus(&self) -> Bus {
        Bus::new(&self.0).unwrap().rescan_every(Duration::ZERO)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn config(enabled: bool) -> Config {
    Config {
        enabled,
        buttons: vec!["primary".into()],
        ..Config::default()
    }
}

fn sample(at_us: u64, sequence: u64, active: bool) -> Observation {
    Observation {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us,
            sequence,
        },
        session_id: "reader-one".into(),
        buttons: vec![PinLevel {
            name: "primary".into(),
            active,
        }],
        head_pads: vec![],
        base_mask: None,
    }
}

fn tap() -> Vec<Observation> {
    vec![
        sample(1_000_000, 1, false),
        sample(1_010_000, 2, true),
        sample(1_040_000, 3, false),
    ]
}

fn value<'a>(report: &'a Diagnostic, key: &str) -> &'a str {
    &report.values.iter().find(|v| v.key == key).unwrap().value
}

fn state(bus: &Bus) -> ControlsState {
    bus.latest(ControlsState::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn typed_tap_event_has_a_matching_retained_stop_for_a_late_owner() {
    let root = Root::new();
    let bus = root.bus();
    let mut events = bus
        .subscribe_typed(ControlGesture::TOPIC, "test", ControlGesture::TYPE)
        .unwrap();
    let mut service =
        Service::new(&bus, config(true), ScriptedPins::new(tap()).unwrap(), CLOCK).unwrap();
    for now in [1_000_000, 1_010_000, 1_040_000] {
        assert!(service.step(|| now).state.ready);
        assert_eq!(state(&bus).stamp.at_us, now);
    }
    let message = events.recv(Duration::from_millis(100)).unwrap().unwrap();
    let event: ControlGesture = message.parse().unwrap();
    event.check().unwrap();
    assert_eq!(
        message.header.type_name.as_deref(),
        Some(ControlGesture::TYPE)
    );
    assert_eq!(message.header.kind, Kind::Event);
    assert_eq!(event.gesture, Gesture::Tap);
    assert_eq!(event.duration_us, 30_000);
    assert_eq!(event.released_us, 1_040_000);
    let retained = state(&bus);
    retained.check().unwrap();
    assert_eq!(retained.latest_stop.as_ref(), Some(&event));
    assert_eq!(retained.session_id, event.session_id);
    assert!(bus.latest(ControlGesture::TOPIC).unwrap().is_none());
    assert_eq!(
        bus.declaration(ControlsState::TOPIC).unwrap().unwrap(),
        ControlsState::declaration()
    );
    assert_eq!(
        bus.declaration(ControlGesture::TOPIC).unwrap().unwrap(),
        ControlGesture::declaration()
    );
    // This owner missed every event socket delivery and starts from the bus's
    // retained state. The repeated state must not repeat the Stop request.
    let mut owner = Receiver::new(MuteRecord::new(CLOCK).unwrap()).unwrap();
    assert!(owner.observe(1_040_001, &retained).unwrap().stop);
    assert!(!owner.observe(1_040_002, &retained).unwrap().stop);
    assert_eq!(service.counts.accepted, 3);
    assert_eq!(service.counts.rejected, 0);
    assert_eq!(service.counts.publish_errors, 0);
}

#[test]
fn shadow_gestures_remain_observable_but_cannot_become_owner_commands() {
    let root = Root::new();
    let bus = root.bus();
    let mut service = Service::new(
        &bus,
        config(false),
        ScriptedPins::new(tap()).unwrap(),
        CLOCK,
    )
    .unwrap();
    service.step(|| 1_000_000);
    service.step(|| 1_010_000);
    let output = service.step(|| 1_040_000);
    assert_eq!(output.gestures.len(), 1);
    assert!(!output.gestures[0].enabled);
    let retained = state(&bus);
    assert!(retained.ready && !retained.enabled);
    let mut owner = Receiver::new(MuteRecord::new(CLOCK).unwrap()).unwrap();
    assert!(owner.observe(1_040_000, &retained).is_err());
    assert!(!owner.mute_record().muted);
    let report = service.report(&output);
    // Watching only is well, and says so: not a warning for the whole lamp.
    assert_eq!(report.level, Level::Ok);
    assert_eq!(report.message, "shadow mode; gestures are not commands");
    assert_eq!(value(&report, "physical_stop_verified"), "false");
    assert_eq!(value(&report, "capture_permission"), "not_owned");
}

#[test]
fn missing_reader_publishes_an_error_without_claiming_hardware_readiness() {
    let root = Root::new();
    let bus = root.bus();
    let mut reports = bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let mut service = Service::new(&bus, config(true), MissingPins, CLOCK).unwrap();
    let output = service.step(|| 1_000_000);
    assert!(!output.state.ready);
    assert_eq!(output.state.fault, Some(ControlFault::Missing));
    assert!(output.gestures.is_empty());
    let report: Diagnostic = reports
        .recv(Duration::from_millis(100))
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(report.level, Level::Error);
    assert_eq!(value(&report, "input_ready"), "false");
    assert_eq!(value(&report, "reader_errors"), "1");
    assert_eq!(value(&report, "physical_stop_verified"), "false");
    assert_eq!(value(&report, "capture_permission"), "not_owned");
    assert_eq!(service.counts.accepted, 0);
    assert_eq!(state(&bus).fault, Some(ControlFault::Missing));
}

#[test]
fn the_state_says_its_monotonic_clock_only_when_its_clock_is_this_boot() {
    // Its `at_us` is the stamp's, of the clock named `CLOCK`: when the bus
    // takes that to be this boot, the header says so (queue #3997, C), and
    // a subscriber judges the state by `at_us` and not the wall clock.
    for (boot, said) in [
        (Some(CLOCK), Some(CLOCK)),
        (Some("another-boot"), None),
        (None, None),
    ] {
        let root = Root::new();
        let bus = root.bus().boot_id(boot);
        let mut service = Service::new(&bus, config(true), MissingPins, CLOCK).unwrap();
        let output = service.step(|| 1_000_000);
        let kept = bus.latest(ControlsState::TOPIC).unwrap().unwrap();
        assert_eq!(kept.header.clock.as_deref(), said, "{boot:?}");
        assert_eq!(kept.header.at_us, output.state.stamp.at_us);
        // Its other topics say nothing of the kind.
        let report = bus.latest(Diagnostic::TOPIC).unwrap();
        assert!(report.is_none_or(|report| report.header.clock.is_none()));
    }
}

#[test]
fn silence_from_a_reader_is_not_a_fresh_released_level() {
    let root = Root::new();
    let bus = root.bus();
    let mut service = Service::new(
        &bus,
        config(true),
        ScriptedPins::new([sample(1_000_000, 1, false)]).unwrap(),
        CLOCK,
    )
    .unwrap();
    assert!(service.step(|| 1_000_000).state.ready);
    assert!(service.step(|| 1_099_999).state.ready);
    let stale = service.step(|| 1_100_000);
    assert!(!stale.state.ready);
    assert_eq!(stale.state.fault, Some(ControlFault::Stale));
    assert_eq!(state(&bus).fault, Some(ControlFault::Stale));
    assert_eq!(service.counts.accepted, 1);
    assert_eq!(service.counts.reader_errors, 0);
}

#[test]
fn a_short_head_pulse_is_published_on_both_edges_between_heartbeats() {
    let root = Root::new();
    let bus = root.bus();
    let times: Vec<_> = (1_000_000..=1_500_000)
        .step_by(50_000)
        .chain([1_510_000, 1_560_000])
        .collect();
    let samples = times.iter().enumerate().map(|(index, &at)| {
        let mut sample = sample(at, index as u64 + 1, false);
        sample.buttons.clear();
        sample.head_pads = vec![PinLevel {
            name: "crown".into(),
            active: at == 1_510_000,
        }];
        sample
    });
    let mut service = Service::new(
        &bus,
        Config {
            enabled: true,
            head_pads: vec!["crown".into()],
            ..Config::default()
        },
        ScriptedPins::new(samples).unwrap(),
        CLOCK,
    )
    .unwrap();
    for &at in &times {
        let output = service.step(|| at);
        assert!(output.gestures.is_empty());
        if at >= 1_500_000 {
            let retained = state(&bus);
            assert_eq!(retained.stamp.at_us, at);
            assert_eq!(retained.head_active, at == 1_510_000);
        }
    }
}

#[test]
fn base_hold_tier_is_published_between_heartbeats_without_committing_an_action() {
    let root = Root::new();
    let bus = root.bus();
    let times: Vec<_> = (1_000_000..=3_030_000).step_by(10_000).collect();
    let samples = times.iter().enumerate().map(|(index, &at)| {
        let mut sample = sample(at, index as u64 + 1, false);
        sample.buttons.clear();
        sample.base_mask = Some(if at >= 1_030_000 { 7 } else { 0 });
        sample
    });
    let mut service = Service::new(
        &bus,
        Config {
            enabled: true,
            base_selected_mask: Some(0x0fff),
            ..Config::default()
        },
        ScriptedPins::new(samples).unwrap(),
        CLOCK,
    )
    .unwrap();
    for &at in &times[..times.len() - 1] {
        assert!(service.step(|| at).gestures.is_empty());
    }
    let previous = state(&bus);
    assert_eq!(previous.base_tier, HoldTier::None);
    assert!(3_030_000 - previous.stamp.at_us < 100_000);
    let output = service.step(|| 3_030_000);
    assert!(output.gestures.is_empty());
    let retained = state(&bus);
    assert_eq!(retained.stamp.at_us, 3_030_000);
    assert_eq!(retained.base_tier, HoldTier::Mute);
    assert_eq!(retained.base_held_us, Some(2_000_000));
    assert!(retained.latest_mute.is_none());
    assert_eq!(
        service.step(|| 3_035_000).state.base_held_us,
        Some(2_000_000)
    );
}

struct EndlessPins {
    reads: Rc<Cell<u64>>,
}
impl PinReader for EndlessPins {
    fn read(&mut self, now: u64) -> Result<Option<Observation>, InputError> {
        let sequence = self.reads.get() + 1;
        self.reads.set(sequence);
        Ok(Some(sample(
            now - 50_000 + sequence,
            sequence,
            sequence == 2,
        )))
    }
}

#[test]
fn an_unbounded_adapter_is_drained_at_most_64_times_then_faults() {
    let root = Root::new();
    let bus = root.bus();
    let reads = Rc::new(Cell::new(0));
    let mut service = Service::new(
        &bus,
        config(true),
        EndlessPins {
            reads: reads.clone(),
        },
        CLOCK,
    )
    .unwrap();
    let output = service.step(|| 1_000_000);
    assert_eq!(reads.get(), DRAIN_LIMIT as u64);
    assert_eq!(service.counts.accepted, DRAIN_LIMIT as u64);
    assert_eq!(service.counts.drain_overflows, 1);
    assert_eq!(output.state.fault, Some(ControlFault::Overflow));
    assert!(!output.state.ready);
    assert!(output.gestures.is_empty());
    assert_eq!(state(&bus).fault, Some(ControlFault::Overflow));
}

struct SlowPins {
    now: Rc<Cell<u64>>,
    sample: Option<Observation>,
}
impl PinReader for SlowPins {
    fn read(&mut self, _: u64) -> Result<Option<Observation>, InputError> {
        if self.sample.is_some() {
            self.now.set(self.now.get() + 100_000);
        }
        Ok(self.sample.take())
    }
}

#[test]
fn time_spent_inside_read_is_counted_before_input_freshness_is_checked() {
    let root = Root::new();
    let bus = root.bus();
    let now = Rc::new(Cell::new(1_000_000));
    let mut service = Service::new(
        &bus,
        config(true),
        SlowPins {
            now: now.clone(),
            sample: Some(sample(1_000_000, 1, false)),
        },
        CLOCK,
    )
    .unwrap();
    let output = service.step(|| now.get());
    assert_eq!(output.state.fault, Some(ControlFault::Stale));
    assert_eq!(service.counts.accepted, 0);
    assert_eq!(service.counts.rejected, 1);
    assert_eq!(service.counts.longest_step_us, 100_000);
    assert_eq!(output.state.stamp.at_us, 1_100_000);
    assert!(output.gestures.is_empty());
}

#[test]
fn owner_lock_prevents_a_second_service_and_restart_changes_session() {
    let root = Root::new();
    let bus = root.bus();
    let mut first = Service::new(&bus, config(true), MissingPins, CLOCK).unwrap();
    let first_session = first.step(|| 1_000_000).state.session_id;
    let conflict = Service::new(&bus, config(false), MissingPins, CLOCK);
    assert!(matches!(conflict, Err(error) if error.kind() == io::ErrorKind::AlreadyExists));
    drop(first);
    let mut second = Service::new(&bus, config(true), MissingPins, CLOCK).unwrap();
    let restarted = second.step(|| 1_002_000);
    assert_ne!(restarted.state.session_id, first_session);
    assert!(restarted.state.latest_stop.is_none());
    assert!(restarted.state.latest_mute.is_none());
    assert_eq!(state(&bus).session_id, restarted.state.session_id);
}

#[test]
fn owner_lock_refuses_a_symlink_without_modifying_its_target() {
    let root = Root::new();
    let bus = root.bus();
    let target = root.0.join("unrelated");
    fs::write(&target, b"keep").unwrap();
    symlink(&target, root.0.join("controls.lock")).unwrap();
    assert!(Service::new(&bus, config(true), MissingPins, CLOCK).is_err());
    assert_eq!(fs::read(target).unwrap(), b"keep");
}

#[test]
fn quiet_event_publisher_renews_liveness_without_inventing_a_gesture() {
    let root = Root::new();
    let bus = root.bus().lease(Duration::from_millis(500));
    let mut events = bus
        .subscribe_typed(ControlGesture::TOPIC, "test", ControlGesture::TYPE)
        .unwrap();
    let mut service = Service::new(&bus, config(true), MissingPins, CLOCK).unwrap();
    service.step(|| 1_000_000);
    let first = bus.publishers(ControlGesture::TOPIC).unwrap();
    assert_eq!(first.len(), 1);
    // lamp-bus leases and refresh throttling use real wall/Instant clocks.
    // The recognizer clock remains injected; this is no lamp timing claim.
    // Until the lease has lapsed, not a fixed 1050 ms (#1806).
    let until = Instant::now() + Duration::from_secs(5);
    while !bus.publishers(ControlGesture::TOPIC).unwrap().is_empty() {
        assert!(Instant::now() < until, "the lease never lapsed");
        thread::sleep(Duration::from_millis(10));
    }
    service.step(|| 2_000_000);
    let renewed = bus.publishers(ControlGesture::TOPIC).unwrap();
    assert_eq!(renewed.len(), 1);
    assert_eq!(renewed[0].boot, first[0].boot);
    assert!(events.recv(Duration::from_millis(10)).unwrap().is_none());
    assert_eq!(service.counts.publish_errors, 0);
}

#[test]
fn acquisition_recording_preserves_receipt_and_edge_times_and_replays() {
    let root = Root::new();
    let bus = root.bus();
    let path = root.0.join("acquisitions.jsonl");
    let original = tap();
    let mut service = Service::new(
        &bus,
        config(true),
        ScriptedPins::new(original.clone()).unwrap(),
        CLOCK,
    )
    .unwrap();
    service.record_to(&path).unwrap();
    assert!(service.record_to(&root.0.join("second.jsonl")).is_err());
    assert!(!root.0.join("second.jsonl").exists());
    for now in [1_005_000, 1_015_000, 1_045_000] {
        service.step(|| now);
    }
    // The lamp's 100 ms finish window covers the writer's last `sync_all`,
    // which a busy test machine can take longer over: five seconds here,
    // still failing a writer that never finishes (#1542).
    let stats = service
        .finish_recording_within(Duration::from_secs(5))
        .unwrap();
    assert!(stats.finished && !stats.failed);
    assert_eq!(stats.written, 3);
    assert_eq!(stats.dropped, 0);
    assert!(service.finish_recording().is_none());
    let records = replay::read(fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(records.len(), 3);
    for (record, sample) in records.iter().zip(original) {
        assert_eq!(record.now_us, sample.stamp.at_us + 5_000);
        assert_eq!(record.observation.as_ref(), Some(&sample));
    }
    let mut replayed = Vec::new();
    let summary = replay::run(config(true), CLOCK, &records, None, &mut replayed).unwrap();
    assert_eq!(summary.gestures, 1);
    assert_eq!(summary.rejected, 0);
    let gestures: Vec<_> = String::from_utf8(replayed)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<replay::Frame>(line).unwrap())
        .flat_map(|frame| frame.output.gestures)
        .collect();
    assert_eq!(gestures[0].duration_us, 30_000);
    assert_eq!(gestures[0].released_us, 1_040_000);
    let bytes = fs::read(&path).unwrap();
    assert!(service.record_to(&path).is_err());
    assert_eq!(fs::read(path).unwrap(), bytes);
}
