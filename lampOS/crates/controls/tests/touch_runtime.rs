use lamp_bus::Bus;
use lamp_controls::{
    engine::Config,
    input::{InputError, Level as PinLevel, Observation, PinReader},
    mpr121,
    mute::MuteRecord,
    pins::{Pin, Pull},
    receiver::Receiver,
    strip_worker::StripReader,
    touch::{StripSample, TouchOutput},
    touch_pins::TouchPinsConfig,
    touch_receiver::TouchReceiver,
    touch_runtime::{DRAIN_LIMIT, Service},
};
use lamp_msgs::{
    ClockStamp, ControlFault, ControlGesture, ControlsState, Diagnostic, Gesture, Level,
    TouchGesture, TouchIntent, TouchSource, TouchSourceState, TouchState,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

const CLOCK: &str = "touch-runtime-test";

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = if Path::new("/private/tmp").is_dir() {
            PathBuf::from("/private/tmp")
        } else {
            fs::canonicalize(std::env::temp_dir()).unwrap()
        };
        let path = parent.join(format!(
            "ctr-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Default)]
struct Gpio {
    values: Rc<RefCell<VecDeque<Result<Observation, InputError>>>>,
    reads: Rc<Cell<usize>>,
}
impl PinReader for Gpio {
    fn read(&mut self, _: u64) -> Result<Option<Observation>, InputError> {
        self.reads.set(self.reads.get() + 1);
        self.values.borrow_mut().pop_front().transpose()
    }
}

#[derive(Clone, Default)]
struct Strip {
    values: Rc<RefCell<VecDeque<Result<StripSample, InputError>>>>,
    reads: Rc<Cell<usize>>,
}
impl StripReader for Strip {
    fn read(&mut self, _: u64) -> Result<Option<StripSample>, InputError> {
        self.reads.set(self.reads.get() + 1);
        self.values.borrow_mut().pop_front().transpose()
    }
}

// Explicit authored host profile. No GPIO/I2C is opened by these tests, and
// these values are not a recommended electrical calibration.
fn base() -> mpr121::Config {
    serde_json::from_value(serde_json::json!({
        "bus":0,"address":90,"every_ms":10,"pads":[11,10,9,8,7,6,5,4,3,2,1,0],
        "profile":{
            "touch_thresholds":([12;12]),"release_thresholds":([6;12]),
            "baseline":{
                "rising":{"mhd":1,"nhd":1,"ncl":14,"fdl":0},
                "falling":{"mhd":1,"nhd":1,"ncl":255,"fdl":2},
                "touched":{"nhd":0,"ncl":0,"fdl":0}
            },
            "ffi":34,"sfi":10,"esi_ms":1,"global_cdc":16,"global_cdt":1,
            "cdc":([0;12]),"cdt":([0;12]),"debounce_touch":2,"debounce_release":2,
            "ecr":143,"autoconfig":{"mode":"disabled"}
        }
    }))
    .unwrap()
}

fn wiring() -> TouchPinsConfig {
    let pin = |name: &str, line| Pin {
        name: name.into(),
        line,
        active_low: true,
        pull: Pull::Up,
    };
    TouchPinsConfig {
        privacy: None,
        chip: "/dev/gpiochip0".into(),
        buttons: vec![pin("primary", 99)],
        head_pads: vec![pin("head", 37)],
        reset: Some(pin("reset", 100)),
        every_ms: 10,
        base: Some(base()),
        head_qualify_ms: None,
        head_tap_min_ms: None,
        head_gap_ms: None,
        legacy_head: false,
    }
}

fn config(enabled: bool) -> Config {
    Config {
        enabled,
        buttons: vec!["primary".into()],
        head_pads: vec!["head".into()],
        base_selected_mask: Some(4095),
    }
}

struct Rig {
    service: Service<Gpio, Strip>,
    bus: Bus,
    gpio: Gpio,
    strip: Strip,
    gpio_sequence: u64,
    strip_sequence: u64,
    _root: Root,
}

impl Rig {
    fn new(enabled: bool) -> Self {
        Self::with(enabled, wiring())
    }

    fn with(enabled: bool, wiring: TouchPinsConfig) -> Self {
        Self::with_config(config(enabled), wiring)
    }

    fn with_config(config: Config, wiring: TouchPinsConfig) -> Self {
        let root = Root::new();
        let bus = Bus::new(&root.0).unwrap().rescan_every(Duration::ZERO);
        let gpio = Gpio::default();
        let strip = Strip::default();
        let service =
            Service::new(&bus, config, wiring, gpio.clone(), strip.clone(), CLOCK).unwrap();
        Self {
            service,
            bus,
            gpio,
            strip,
            gpio_sequence: 0,
            strip_sequence: 0,
            _root: root,
        }
    }

    fn gpio_sample(&mut self, at: u64, primary: bool, reset: bool, head: bool) -> Observation {
        self.gpio_sequence += 1;
        Observation {
            stamp: ClockStamp {
                clock_id: CLOCK.into(),
                at_us: at,
                sequence: self.gpio_sequence,
            },
            session_id: "gpio-reader".into(),
            buttons: vec![
                PinLevel {
                    name: "primary".into(),
                    active: primary,
                },
                PinLevel {
                    name: "reset".into(),
                    active: reset,
                },
            ],
            head_pads: vec![PinLevel {
                name: "head".into(),
                active: head,
            }],
            base_mask: None,
        }
    }

    fn gpio(&mut self, at: u64, primary: bool, reset: bool, head: bool) {
        let sample = self.gpio_sample(at, primary, reset, head);
        self.gpio.values.borrow_mut().push_back(Ok(sample));
    }

    fn strip(&mut self, at: u64, mask: u16) {
        self.strip_sequence += 1;
        self.strip.values.borrow_mut().push_back(Ok(StripSample {
            stamp: ClockStamp {
                clock_id: CLOCK.into(),
                at_us: at,
                sequence: self.strip_sequence,
            },
            session_id: "strip-reader".into(),
            mask,
        }));
    }

    fn step(&mut self, at: u64) -> TouchOutput {
        let output = self.service.step(|| at);
        output.state.check().unwrap();
        for event in &output.events {
            event.check().unwrap();
        }
        output
    }

    fn legacy(&self) -> ControlsState {
        let state: ControlsState = self
            .bus
            .latest(ControlsState::TOPIC)
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        state.check().unwrap();
        state
    }
}

fn source(output: &TouchOutput, source: TouchSource) -> &TouchSourceState {
    output
        .state
        .sources
        .iter()
        .find(|state| state.source == source)
        .unwrap()
}

fn diagnostic_value<'a>(report: &'a Diagnostic, key: &str) -> &'a str {
    &report
        .values
        .iter()
        .find(|value| value.key == key)
        .unwrap()
        .value
}

#[test]
fn rejected_complete_gpio_cannot_retain_a_primary_stop_or_long_hold_mute() {
    for release_at in [60_000, 2_040_000] {
        let mut rig = Rig::new(true);
        rig.gpio(0, false, false, false);
        rig.step(0);
        for at in (20_000..release_at).step_by(20_000) {
            rig.gpio(at, true, false, false);
            assert!(rig.step(at).events.is_empty());
        }
        let mut bad = rig.gpio_sample(release_at, false, false, false);
        bad.head_pads[0].name = "unexpected-pad".into();
        // Structurally valid, but inconsistent with the complete configured input.
        bad.check().unwrap();
        rig.gpio.values.borrow_mut().push_back(Ok(bad));
        let output = rig.step(release_at);
        assert!(output.events.is_empty());
        assert_eq!(output.state.event_sequence, 0);
        assert_eq!(output.state.rejected_inputs, 1);
        assert_eq!(
            source(&output, TouchSource::Primary).fault,
            Some(ControlFault::Invalid)
        );
        let state = rig.legacy();
        assert!(!state.ready);
        assert!(state.latest_stop.is_none() && state.latest_mute.is_none());
        assert_eq!(state.event_sequence, 0);
        assert_eq!(state.rejected_inputs, 1);
        let mut owner = Receiver::new(MuteRecord::new(CLOCK).unwrap()).unwrap();
        let effect = owner.observe(release_at, &state).unwrap();
        assert!(!effect.stop && !effect.software_muted);
        assert_eq!(rig.service.counts.rejected, 1);
        assert_eq!(rig.service.counts.publish_errors, 0);
    }
}

#[test]
fn strip_startup_failure_leaves_primary_gpio_and_legacy_stop_working() {
    let mut rig = Rig::new(true);
    let mut reports = rig
        .bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    rig.service
        .strip_unavailable(0, &format!("spawn failed\n{}", "x".repeat(400)));
    for (at, primary) in [(0, false), (20_000, true), (40_000, false)] {
        rig.gpio(at, primary, false, false);
        let output = rig.step(at);
        assert!(source(&output, TouchSource::Primary).ready);
        assert_eq!(
            source(&output, TouchSource::Strip).fault,
            Some(ControlFault::Missing)
        );
        if at == 40_000 {
            assert_eq!(output.events.len(), 1);
            assert_eq!(output.events[0].when_speaking, TouchIntent::Stop);
            assert_eq!(output.events[0].source, TouchSource::Primary);
        }
    }
    let state = rig.legacy();
    assert!(state.ready && state.latest_stop.is_some());
    let mut owner = Receiver::new(MuteRecord::new(CLOCK).unwrap()).unwrap();
    assert!(owner.observe(40_000, &state).unwrap().stop);
    assert_eq!(rig.service.counts.reader_errors, 1);
    assert_eq!(rig.service.gpio_accepted, 3);
    assert_eq!(rig.service.strip_accepted, 0);
    let report: Diagnostic = reports
        .recv(Duration::from_millis(100))
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(report.level, Level::Error);
    assert_eq!(diagnostic_value(&report, "physical_stop_verified"), "false");
    assert_eq!(diagnostic_value(&report, "capture_permission"), "not_owned");
    assert!(diagnostic_value(&report, "last_strip_error").contains("spawn failed"));
    assert!(!diagnostic_value(&report, "last_strip_error").contains('\n'));
}

#[test]
fn strip_reader_failure_withdraws_strip_contact_without_losing_primary() {
    let mut rig = Rig::new(true);
    for at in (0..=80_000).step_by(20_000) {
        rig.gpio(at, false, false, false);
        rig.strip(at, if at < 40_000 { 0 } else { 7 });
        rig.step(at);
    }
    rig.strip
        .values
        .borrow_mut()
        .push_back(Err(InputError::new(ControlFault::Missing, "I2C lost")));
    rig.gpio(100_000, true, false, false);
    let output = rig.step(100_000);
    let strip = source(&output, TouchSource::Strip);
    assert!(!strip.ready && strip.active.is_none() && strip.contact_id.is_none());
    assert!(source(&output, TouchSource::Primary).ready);
    assert!(
        output
            .state
            .latest
            .iter()
            .all(|event| event.source != TouchSource::Strip)
    );
    rig.gpio(120_000, false, false, false);
    let output = rig.step(120_000);
    assert!(
        output
            .events
            .iter()
            .any(|event| event.source == TouchSource::Primary
                && event.when_speaking == TouchIntent::Stop)
    );
    assert!(rig.legacy().latest_stop.is_some());
    assert_eq!(rig.service.counts.reader_errors, 1);
}

#[test]
fn gpio_reader_failure_does_not_cancel_fresh_strip_touch() {
    let mut rig = Rig::new(true);
    for at in [0, 20_000] {
        rig.gpio(at, false, false, false);
        rig.strip(at, 0);
        rig.step(at);
    }
    rig.gpio
        .values
        .borrow_mut()
        .push_back(Err(InputError::new(ControlFault::Missing, "GPIO lost")));
    let mut strip_touches = 0;
    for at in [40_000, 60_000, 80_000] {
        rig.strip(at, 7);
        let output = rig.step(at);
        assert_eq!(
            source(&output, TouchSource::Primary).fault,
            Some(ControlFault::Missing)
        );
        assert!(source(&output, TouchSource::Strip).ready);
        strip_touches += output
            .events
            .iter()
            .filter(|event| {
                event.source == TouchSource::Strip && event.gesture == TouchGesture::Touch
            })
            .count();
    }
    assert_eq!(strip_touches, 1);
    assert!(!rig.legacy().ready);
    assert!(rig.legacy().latest_stop.is_none());
}

#[test]
fn one_source_never_renews_the_other_sources_acquisition_time() {
    for live_strip in [false, true] {
        let mut rig = Rig::new(true);
        rig.gpio(0, false, false, false);
        rig.strip(0, 0);
        rig.step(0);
        let mut last = None;
        for at in (20_000..=100_000).step_by(20_000) {
            if live_strip {
                rig.strip(at, 0);
            } else {
                rig.gpio(at, false, false, false);
            }
            last = Some(rig.step(at));
        }
        let output = last.unwrap();
        let (live, stale) = if live_strip {
            (TouchSource::Strip, TouchSource::Primary)
        } else {
            (TouchSource::Primary, TouchSource::Strip)
        };
        assert!(source(&output, live).ready);
        let stale = source(&output, stale);
        assert!(!stale.ready && stale.active.is_none());
        assert_eq!(stale.observed_us, Some(0));
        assert_eq!(stale.fault, Some(ControlFault::Stale));
    }
}

#[test]
fn stale_arrival_never_makes_a_fresh_source_fail_or_restamps_old_data() {
    let mut rig = Rig::new(true);
    rig.gpio(100_000, false, false, false);
    rig.strip(0, 7);
    let output = rig.step(100_000);
    assert!(source(&output, TouchSource::Primary).ready);
    assert_eq!(
        source(&output, TouchSource::Strip).fault,
        Some(ControlFault::Stale)
    );
    assert_eq!(source(&output, TouchSource::Strip).observed_us, None);
    assert!(output.events.is_empty());
    assert_eq!(rig.service.counts.rejected, 1);
    assert_eq!(rig.service.gpio_accepted, 1);
    assert_eq!(rig.service.strip_accepted, 0);
}

#[test]
fn draining_a_source_is_bounded_and_overflow_faults_only_that_source() {
    for overflow_strip in [false, true] {
        let mut rig = Rig::new(true);
        let now = 1_000_000;
        for offset in 0..DRAIN_LIMIT {
            let at = now - (DRAIN_LIMIT - offset - 1) as u64;
            if overflow_strip {
                rig.strip(at, 0);
            } else {
                rig.gpio(at, false, false, false);
            }
        }
        if overflow_strip {
            rig.gpio(now, false, false, false);
        } else {
            rig.strip(now, 0);
        }
        let output = rig.step(now);
        let (failed, healthy, reads) = if overflow_strip {
            (
                TouchSource::Strip,
                TouchSource::Primary,
                rig.strip.reads.get(),
            )
        } else {
            (
                TouchSource::Primary,
                TouchSource::Strip,
                rig.gpio.reads.get(),
            )
        };
        assert_eq!(reads, DRAIN_LIMIT);
        assert_eq!(source(&output, failed).fault, Some(ControlFault::Overflow));
        assert!(source(&output, healthy).ready);
        assert!(output.events.is_empty());
        assert_eq!(rig.service.counts.drain_overflows, 1);
        assert_eq!(rig.service.counts.accepted, DRAIN_LIMIT as u64 + 1);
    }
}

#[test]
fn shadow_state_and_events_are_observable_but_both_receivers_refuse_authority() {
    let mut rig = Rig::new(false);
    for (at, primary) in [(0, false), (20_000, true), (40_000, false)] {
        rig.gpio(at, primary, false, false);
        rig.strip(at, 0);
        let output = rig.step(at);
        assert!(!output.state.enabled);
        if at == 40_000 {
            assert_eq!(output.events.len(), 1);
            assert!(!output.events[0].enabled);
            assert!(
                TouchReceiver::new(CLOCK)
                    .unwrap()
                    .observe(at, &output.state)
                    .is_err()
            );
        }
    }
    let state = rig.legacy();
    assert!(state.ready && !state.enabled && state.latest_stop.is_some());
    let mut owner = Receiver::new(MuteRecord::new(CLOCK).unwrap()).unwrap();
    assert!(owner.observe(40_000, &state).is_err());
    assert!(!owner.mute_record().muted);
    let advertised: TouchState = rig
        .bus
        .latest(TouchState::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert!(!advertised.enabled);
    assert_eq!(rig.service.counts.publish_errors, 0);
}

#[test]
fn head_and_reset_roles_never_enter_legacy_primary_retention() {
    let mut rig = Rig::new(true);
    // Released a full second first: the reset arms (RESET_ARM_US).
    for at in (0..=1_000_000).step_by(50_000) {
        rig.gpio(at, false, false, false);
        rig.step(at);
    }
    let mut head_events = Vec::new();
    for at in [1_020_000, 1_040_000] {
        rig.gpio(at, false, false, true);
        head_events.extend(rig.step(at).events);
    }
    assert!(
        head_events
            .iter()
            .any(|event| event.source == TouchSource::Head && event.gesture == TouchGesture::Touch)
    );
    for at in (1_060_000..=6_060_000).step_by(50_000) {
        rig.gpio(at, false, true, false);
        rig.step(at);
    }
    rig.gpio(6_080_000, false, false, false);
    let output = rig.step(6_080_000);
    assert_eq!(output.events.len(), 1);
    assert_eq!(output.events[0].source, TouchSource::Reset);
    assert_eq!(output.events[0].when_speaking, TouchIntent::FactoryReset);
    let state = rig.legacy();
    assert!(state.latest_stop.is_none() && state.latest_mute.is_none());
    assert_eq!(state.event_sequence, 0);
}

/// The reviewer on #1167: on a product lamp, whose voice and Expression do
/// not read `controls/touch`, a pat on the head is still the old engine's
/// `Pet` with `legacy_head`, which wakes a lamp asleep at night. The head's
/// own touch events go on as well; the reset never reaches the old engine.
#[test]
fn with_legacy_head_a_pat_is_still_the_old_engines_pet() {
    let mut rig = Rig::with(
        true,
        TouchPinsConfig {
            legacy_head: true,
            ..wiring()
        },
    );
    let mut gestures = rig.bus.subscribe(ControlGesture::TOPIC, "test").unwrap();
    for at in (0..=600_000).step_by(20_000) {
        rig.gpio(at, false, false, false);
        rig.step(at);
    }
    let mut head_events = Vec::new();
    for at in (620_000..=900_000).step_by(20_000) {
        rig.gpio(at, false, false, true);
        head_events.extend(rig.step(at).events);
    }
    assert!(rig.legacy().head_active, "the old engine sees the hand");
    assert!(
        head_events
            .iter()
            .any(|event| event.source == TouchSource::Head && event.gesture == TouchGesture::Touch)
    );
    for at in (920_000..=3_000_000).step_by(20_000) {
        rig.gpio(at, false, false, false);
        rig.step(at);
    }
    let pet: ControlGesture = gestures
        .recv(Duration::ZERO)
        .unwrap()
        .expect("a pat is a Pet on controls/gesture")
        .parse()
        .unwrap();
    assert_eq!((pet.gesture, pet.enabled), (Gesture::Pet, true));
    assert!(!rig.legacy().head_active);
    // Without it, as before: the head never reaches the old engine.
    let mut plain = Rig::new(true);
    let mut plain_gestures = plain.bus.subscribe(ControlGesture::TOPIC, "test").unwrap();
    for at in (0..=600_000).step_by(20_000) {
        plain.gpio(at, false, false, false);
        plain.step(at);
    }
    for at in (620_000..=900_000).step_by(20_000) {
        plain.gpio(at, false, false, true);
        plain.step(at);
    }
    assert!(!plain.legacy().head_active);
    for at in (920_000..=3_000_000).step_by(20_000) {
        plain.gpio(at, false, false, false);
        plain.step(at);
    }
    assert!(plain_gestures.recv(Duration::ZERO).unwrap().is_none());
    assert_eq!(plain.legacy().event_sequence, 0);
}

/// The reviewer on #795: a legacy engine with only the head pads
/// (`legacy_head`, no button) hears a GPIO read fault as one with the
/// button does, so its `controls/state` says the hand cannot be read, not
/// only that it went stale later.
#[test]
fn a_head_only_legacy_engine_hears_a_gpio_read_fault() {
    let wiring = TouchPinsConfig {
        buttons: vec![],
        base: None,
        legacy_head: true,
        ..wiring()
    };
    let config = Config {
        enabled: true,
        buttons: vec![],
        head_pads: vec!["head".into()],
        base_selected_mask: None,
    };
    let mut rig = Rig::with_config(config, wiring);
    for at in (0..=200_000).step_by(20_000) {
        rig.gpio_sequence += 1;
        let sample = Observation {
            stamp: ClockStamp {
                clock_id: CLOCK.into(),
                at_us: at,
                sequence: rig.gpio_sequence,
            },
            session_id: "gpio-reader".into(),
            buttons: vec![PinLevel {
                name: "reset".into(),
                active: false,
            }],
            head_pads: vec![PinLevel {
                name: "head".into(),
                active: false,
            }],
            base_mask: None,
        };
        rig.gpio.values.borrow_mut().push_back(Ok(sample));
        rig.step(at);
    }
    assert!(rig.legacy().ready);
    rig.gpio
        .values
        .borrow_mut()
        .push_back(Err(InputError::new(ControlFault::Missing, "GPIO lost")));
    for at in [220_000, 240_000, 340_000] {
        rig.step(at);
    }
    let state = rig.legacy();
    assert!(!state.ready);
    assert_eq!(state.fault, Some(ControlFault::Missing));
}

/// The reviewer on #1209: `legacy_head` with no head pads is ignored, not
/// refused, so the touch path starts and the reset works, with a button and
/// without one. Alone, the old engine once had nothing to read and took the
/// touch path's GPIO down: a 5 s hold made no reset (the reviewer on #1176).
#[test]
fn legacy_head_with_no_head_pads_is_ignored_and_the_reset_still_works() {
    for button in [true, false] {
        let wiring = TouchPinsConfig {
            buttons: if button { wiring().buttons } else { vec![] },
            head_pads: vec![],
            base: None,
            legacy_head: true,
            ..wiring()
        };
        let config = Config {
            enabled: true,
            buttons: if button {
                vec!["primary".into()]
            } else {
                vec![]
            },
            head_pads: vec![],
            base_selected_mask: None,
        };
        let mut rig = Rig::with_config(config, wiring);
        let mut resets = 0;
        for at in (0..=7_100_000u64).step_by(20_000) {
            rig.gpio_sequence += 1;
            let mut buttons = vec![PinLevel {
                name: "reset".into(),
                active: (2_000_000..=7_000_000).contains(&at),
            }];
            if button {
                buttons.insert(
                    0,
                    PinLevel {
                        name: "primary".into(),
                        active: false,
                    },
                );
            }
            let sample = Observation {
                stamp: ClockStamp {
                    clock_id: CLOCK.into(),
                    at_us: at,
                    sequence: rig.gpio_sequence,
                },
                session_id: "gpio-reader".into(),
                buttons,
                head_pads: vec![],
                base_mask: None,
            };
            rig.gpio.values.borrow_mut().push_back(Ok(sample));
            resets += rig
                .step(at)
                .events
                .iter()
                .filter(|event| event.source == TouchSource::Reset)
                .count();
        }
        assert_eq!(resets, 1, "with a button: {button}");
    }
}

#[test]
fn missing_all_inputs_never_advertises_a_released_or_ready_hand() {
    let mut rig = Rig::new(true);
    let output = rig.step(1_000_000);
    assert!(output.events.is_empty());
    for source in &output.state.sources {
        assert!(!source.ready && !source.armed);
        assert_eq!(source.fault, Some(ControlFault::Missing));
        assert!(source.active.is_none() && source.observed_us.is_none());
    }
    assert!(!rig.legacy().ready);
    assert_eq!(rig.service.counts.accepted, 0);
}

#[test]
fn a_second_touch_owner_cannot_publish_into_the_same_bus_root() {
    let rig = Rig::new(true);
    let result = Service::new(
        &rig.bus,
        config(true),
        wiring(),
        Gpio::default(),
        Strip::default(),
        CLOCK,
    );
    assert!(result.is_err());
}

/// #711: the body's `body/hand` is a push only while fresh and from the body;
/// anything else, or nothing, is not, so a stopped body never silences the head.
#[test]
fn the_bodys_hand_counts_only_fresh_and_from_the_body() {
    use lamp_controls::touch_runtime::pushed_now;
    use lamp_msgs::Hand;
    let root = Root::new();
    let bus = Bus::new(&root.0).unwrap();
    let now_unix_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };
    assert!(!pushed_now(None, now_unix_ms()));
    let mut body = bus
        .publisher_declared(Hand::TOPIC, &Hand::declaration(), "body")
        .unwrap();
    body.expires_after(Some(Duration::from_secs(1)));
    let said = |pushed| Hand {
        pushed,
        since_us: u64::from(pushed) * 5_000_000,
    };
    body.publish_json(1, &said(true)).unwrap();
    let latest = bus.latest(Hand::TOPIC).unwrap();
    assert!(pushed_now(latest.as_ref(), now_unix_ms()));
    // Past its second it says nothing.
    assert!(!pushed_now(latest.as_ref(), now_unix_ms() + 5_000));
    body.publish_json(2, &said(false)).unwrap();
    assert!(!pushed_now(
        bus.latest(Hand::TOPIC).unwrap().as_ref(),
        now_unix_ms()
    ));
    drop(body);
    let mut other = bus
        .publisher_declared(Hand::TOPIC, &Hand::declaration(), "test")
        .unwrap();
    other.publish_json(3, &said(true)).unwrap();
    assert!(!pushed_now(
        bus.latest(Hand::TOPIC).unwrap().as_ref(),
        now_unix_ms()
    ));
}

/// What a still pad's and a hand's states are counted with: the two topics,
/// read after every step (a Mac's socket queue holds only a few of them).
struct Heard {
    touch: lamp_bus::Subscriber,
    legacy: lamp_bus::Subscriber,
    counts: (usize, usize),
    /// The last step's touch state, its times set aside.
    last: Option<TouchState>,
}

impl Heard {
    fn new(rig: &Rig) -> Self {
        Self {
            touch: rig.bus.subscribe(TouchState::TOPIC, "test_touch").unwrap(),
            legacy: rig
                .bus
                .subscribe(ControlsState::TOPIC, "test_legacy")
                .unwrap(),
            counts: (0, 0),
            last: None,
        }
    }

    fn read(&mut self) {
        while self.touch.recv(Duration::ZERO).unwrap().is_some() {
            self.counts.0 += 1;
        }
        while self.legacy.recv(Duration::ZERO).unwrap().is_some() {
            self.counts.1 += 1;
        }
    }

    fn take(&mut self) -> (usize, usize) {
        self.read();
        std::mem::take(&mut self.counts)
    }
}

/// A touch state with its times set aside, and what only grows while a
/// hand stays (`held_us`, the strip's motion): what changes when a reader
/// has something new to act on at once.
fn changed_part(state: &TouchState) -> TouchState {
    let mut part = state.clone();
    part.stamp.at_us = 0;
    part.stamp.sequence = 0;
    for source in &mut part.sources {
        source.observed_us = source.observed_us.map(|_| 0);
        source.held_us = source.held_us.map(|_| 0);
        source.position = source.position.map(|_| 0.0);
        source.distance = 0.0;
        source.speed = 0.0;
    }
    part
}

/// Steps the service every 2 ms from `from` to before `to`, the pins read
/// every 20 ms with the primary button and the head as given. A step whose
/// touch state changed must say it in that step. The last step's touch
/// state.
fn served(
    rig: &mut Rig,
    heard: &mut Heard,
    from: u64,
    to: u64,
    primary: bool,
    head: bool,
) -> TouchState {
    served_reading(rig, heard, (from, to), 20_000, (primary, head))
}

/// The same with the pins read every `pins_us` (the product's every 5 ms).
fn served_reading(
    rig: &mut Rig,
    heard: &mut Heard,
    (from, to): (u64, u64),
    pins_us: u64,
    (primary, head): (bool, bool),
) -> TouchState {
    let mut last = None;
    // The pins are read at the first 2 ms step at or after each boundary of
    // theirs, so every 5 ms is truly 200 readings a second (the reviewer on
    // #1469: reading only on steps that land on one was every 10 ms).
    let mut next = from.next_multiple_of(pins_us);
    for at in (from..to).step_by(2_000) {
        if at >= next {
            rig.gpio(at, primary, false, head);
            while next <= at {
                next += pins_us;
            }
        }
        let errors = rig.service.counts.publish_errors;
        let state = rig.step(at).state;
        let part = changed_part(&state);
        last = Some(state);
        // A Mac refuses a datagram over 2 KB, as a state holding two of the
        // head's events is; Linux takes it, and must say every change.
        let refused = rig.service.counts.publish_errors > errors;
        assert!(!refused || cfg!(target_os = "macos"), "refused at {at}");
        if !refused && heard.last.as_ref().is_some_and(|last| *last != part) {
            assert_eq!(latest_touch(rig).stamp.at_us, at, "changed at {at}");
        }
        heard.last = Some(part);
        heard.read();
    }
    last.unwrap()
}

fn latest_touch(rig: &Rig) -> TouchState {
    rig.bus
        .latest(TouchState::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap()
}

fn head(state: &TouchState) -> &TouchSourceState {
    state
        .sources
        .iter()
        .find(|source| source.source == TouchSource::Head)
        .unwrap()
}

/// Ten of each in a still second: the heartbeat, as declared.
fn still(counts: (usize, usize)) {
    assert!((9..=11).contains(&counts.0), "{} touch states", counts.0);
    assert!((9..=11).contains(&counts.1), "{} controls states", counts.1);
}

/// The first builder on #1456 (lamp-4ace): a still pad was said 50 times a
/// second on `controls/touch_state` and on `controls/state`, and each one
/// woke every reader. Now each is said when what a reader acts on changes,
/// and every 100 ms otherwise, as declared. A hand on the head, each reading
/// of it held and its going still go out in the step that read them. (The
/// button has a rig of its own: its events and the head's together make a
/// state longer than a Mac's 2 KB datagram, which the bus refuses there and
/// Linux takes.)
#[test]
fn a_still_pad_is_said_ten_times_a_second_and_a_hand_in_the_step_that_read_it() {
    let mut rig = Rig::new(true);
    let mut heard = Heard::new(&rig);
    // Settled and armed first.
    served(&mut rig, &mut heard, 0, 1_000_000, false, false);
    heard.take();
    served(&mut rig, &mut heard, 1_000_000, 2_000_000, false, false);
    still(heard.take());
    for at in (2_000_000..2_300_000).step_by(20_000) {
        rig.gpio(at, false, false, true);
        heard.last = Some(changed_part(&rig.step(at).state));
        assert_eq!(latest_touch(&rig).stamp.at_us, at, "the head at {at}");
        heard.read();
        served(&mut rig, &mut heard, at + 2_000, at + 20_000, false, true);
    }
    assert!(head(&latest_touch(&rig)).held_us.is_some());
    // Let go: every change on the way is said in its step, until the hand
    // is gone (it lasts through a short gap). The button's test counts the
    // still second after a release.
    let gone = served(&mut rig, &mut heard, 2_300_000, 3_000_000, false, false);
    assert!(
        head(&gone).held_us.is_none() && head(&gone).active == Some(false),
        "{:?}",
        head(&gone)
    );
}

/// The reviewer on #1469: the product's pins read every 5 ms, and saying a
/// held contact at each reading took the lamp to 200 states a second while
/// a hand stayed. What only grows while held is said at most every 20 ms,
/// the pace it always had; the press and the release still at once.
#[test]
fn a_hand_held_with_the_pins_read_every_5_ms_is_said_at_most_50_times_a_second() {
    let pins = 5_000;
    let mut rig = Rig::new(true);
    let mut heard = Heard::new(&rig);
    served_reading(&mut rig, &mut heard, (0, 1_000_000), pins, (false, false));
    heard.take();
    served_reading(
        &mut rig,
        &mut heard,
        (1_000_000, 2_000_000),
        pins,
        (false, true),
    );
    let (touch, _) = heard.take();
    assert!(
        (45..=55).contains(&touch),
        "{touch} head states in a held second"
    );
    assert!(head(&latest_touch(&rig)).held_us.is_some());
    // The button, on a lamp of its own (its events and the head's make a
    // state too long for a Mac's datagram).
    let mut rig = Rig::new(true);
    let mut heard = Heard::new(&rig);
    served_reading(&mut rig, &mut heard, (0, 1_000_000), pins, (false, false));
    heard.take();
    served_reading(
        &mut rig,
        &mut heard,
        (1_000_000, 2_000_000),
        pins,
        (true, false),
    );
    let (touch, legacy) = heard.take();
    assert!(
        (45..=55).contains(&touch),
        "{touch} touch states, the button held"
    );
    assert!(
        (45..=55).contains(&legacy),
        "{legacy} controls states, the button held"
    );
    assert!(rig.legacy().buttons[0].held_us.is_some());
    // Let go: said in the step that read it.
    rig.gpio(2_000_000, false, false, false);
    rig.step(2_000_000);
    assert_eq!(rig.legacy().stamp.at_us, 2_000_000);
    assert!(!rig.legacy().buttons[0].pressed);
}

/// The same for the primary button on `controls/state`: its press, each
/// reading while it is held (the hold's tiers count from it) and its
/// release are said in the step that read them.
#[test]
fn a_button_held_is_said_at_each_reading_and_its_release_at_once() {
    let mut rig = Rig::new(true);
    let mut heard = Heard::new(&rig);
    served(&mut rig, &mut heard, 0, 1_000_000, false, false);
    heard.take();
    for at in (1_000_000..1_300_000).step_by(20_000) {
        rig.gpio(at, true, false, false);
        rig.step(at);
        assert_eq!(rig.legacy().stamp.at_us, at, "the button at {at}");
        heard.last = None;
        served(&mut rig, &mut heard, at + 2_000, at + 20_000, true, false);
    }
    assert!(rig.legacy().buttons[0].held_us.is_some());
    rig.gpio(1_300_000, false, false, false);
    rig.step(1_300_000);
    let released = rig.legacy();
    assert_eq!(released.stamp.at_us, 1_300_000);
    assert!(!released.buttons[0].pressed);
    heard.last = None;
    served(&mut rig, &mut heard, 1_302_000, 2_000_000, false, false);
    heard.take();
    served(&mut rig, &mut heard, 2_000_000, 3_000_000, false, false);
    still(heard.take());
}
