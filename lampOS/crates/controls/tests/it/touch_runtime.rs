use lamp_bus::Bus;
use lamp_controls::{
    engine::Config,
    input::{InputError, Level as PinLevel, Observation, PinReader},
    mpr121,
    mute::MuteRecord,
    pace::{Pace, rest_us},
    pins::{Lines, Pin, Pull},
    privacy::{LineReader, Source},
    receiver::Receiver,
    strip_worker::StripReader,
    touch::{StripSample, TouchOutput},
    touch_pins::{HEAD_ITEM, HeadLines, TouchPinsConfig},
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
        strip_qualify_ms: None,
        strip_release_ms: None,
        strip_stroke_ms: None,
        strip_sleep_ms: None,
        head_rest: None,
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

/// #1798: what the probe at start found on the head pads' lines goes on
/// `diagnostics` beside Controls' own report, once a second, where
/// lamp-health reads it. A pad read touched from the first acquisition is
/// named as left out, a warning, until it is let go. Without a probe there
/// is no item.
#[test]
fn controls_says_the_head_lines_and_a_pad_left_out_on_diagnostics() {
    let said = |reports: &mut lamp_bus::Subscriber| {
        let mut said = Vec::new();
        while let Ok(Some(message)) = reports.recv(Duration::from_millis(50)) {
            let report: Diagnostic = message.parse().unwrap();
            if report.name == HEAD_ITEM {
                said.push((report.level, report.message));
            }
        }
        said
    };
    let mut rig = Rig::new(true);
    let mut reports = rig
        .bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    rig.service.head_probed(HeadLines {
        wired: vec![37],
        bare: vec![],
        unprobed: vec![],
    });
    rig.gpio(0, false, false, true);
    rig.step(0);
    assert_eq!(
        said(&mut reports),
        [(
            Level::Warn,
            "wired on line 37; line 37 touched since the head was first read: left out until let go"
                .to_owned()
        )]
    );
    rig.gpio(500_000, false, false, false);
    rig.step(500_000);
    rig.gpio(1_000_000, false, false, false);
    rig.step(1_000_000);
    assert_eq!(
        said(&mut reports),
        [(Level::Ok, "wired on line 37".to_owned())]
    );
    let mut quiet = Rig::new(true);
    let mut reports = quiet
        .bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    quiet.gpio(0, false, false, false);
    quiet.step(0);
    assert_eq!(said(&mut reports), []);
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

/// #1798: exercise the retained body message through Service, not just its
/// freshness predicate or the recognizer's direct `set_head_pushed` input.
mod body_hand {
    use super::*;
    use lamp_msgs::{ControlIntent, Hand, TouchEvent};
    use std::ops::Range;

    fn body(rig: &Rig) -> lamp_bus::Publisher {
        let mut publisher = rig
            .bus
            .publisher_declared(Hand::TOPIC, &Hand::declaration(), "body")
            .unwrap();
        // GPIO uses a stepped clock, while bus headers use wall time. Keep
        // this fixture's fresh value stable under host load; the expiry case
        // explicitly ages the real retained header below. This does not model
        // Body's publication cadence or its one-second deadline.
        publisher.expires_after(None);
        publisher
    }

    fn say(body: &mut lamp_bus::Publisher, at: u64, pushed: bool) {
        body.publish_json(
            at,
            &Hand {
                pushed,
                // Body's monotonic origin is independent of Controls'.
                since_us: if pushed { 5_000_000 } else { 0 },
            },
        )
        .unwrap();
    }

    fn expire_push(rig: &Rig) {
        let mut message = rig.bus.latest(Hand::TOPIC).unwrap().unwrap();
        assert!(!message.expired && message.parse::<Hand>().unwrap().pushed);
        message.header.at_unix_ms = 0;
        message.header.expires_ms = Some(1_000);
        let mut bytes = serde_json::to_vec(&message.header).unwrap();
        bytes.push(b'\n');
        bytes.extend_from_slice(&message.payload);
        fs::write(rig.bus.root().join(Hand::TOPIC).join("latest"), bytes).unwrap();
        assert_expired_push(rig);
    }

    fn assert_expired_push(rig: &Rig) {
        let message = rig.bus.latest(Hand::TOPIC).unwrap().unwrap();
        assert_eq!(message.header.source, "body");
        assert!(message.expired && message.parse::<Hand>().unwrap().pushed);
    }

    fn run(rig: &mut Rig, times: Range<u64>, primary: bool, pressed: bool) -> Vec<TouchOutput> {
        times
            .step_by(10_000)
            .map(|at| {
                rig.gpio(at, primary, false, pressed);
                let output = rig.step(at);
                let head = source(&output, TouchSource::Head);
                assert!(head.ready && head.fault.is_none(), "at {at}: {head:?}");
                assert_eq!(head.observed_us, Some(at));
                assert_eq!(head.active, Some(pressed));
                if at >= 500_000 {
                    assert!(head.armed, "head was not armed at {at}");
                }
                assert_eq!(rig.service.counts.rejected, 0);
                assert_eq!(rig.service.counts.reader_errors, 0);
                assert_eq!(rig.service.counts.drain_overflows, 0);
                output
            })
            .collect()
    }

    fn events(outputs: &[TouchOutput]) -> Vec<&TouchEvent> {
        outputs.iter().flat_map(|output| &output.events).collect()
    }

    fn spoiled(outputs: &[TouchOutput], contact: u64) {
        for output in outputs {
            let head = source(output, TouchSource::Head);
            assert_eq!(head.contact_id, Some(contact));
            assert_eq!(head.held_us, None);
            assert!(
                output
                    .events
                    .iter()
                    .all(|event| event.source != TouchSource::Head),
                "a spoiled head emitted {:?}",
                output.events
            );
        }
    }

    fn recovery(expired: bool) {
        let mut rig = Rig::new(true);
        assert!(events(&run(&mut rig, 0..600_000, false, false)).is_empty());
        let mut body = body(&rig);
        say(&mut body, 600_000, true);
        let held = run(&mut rig, 600_000..1_000_000, false, true);
        let contact = source(&held[0], TouchSource::Head).contact_id.unwrap();
        spoiled(&held, contact);

        // A pushed head must not silence the independent primary Stop.
        let mut button = run(&mut rig, 1_000_000..1_030_000, true, true);
        button.extend(run(&mut rig, 1_030_000..1_100_000, false, true));
        spoiled(&button, contact);
        let stops = events(&button);
        assert_eq!(stops.len(), 1);
        assert_eq!(stops[0].source, TouchSource::Primary);
        assert_eq!(stops[0].intent(true, false), Some(TouchIntent::Stop));
        assert_eq!(stops[0].intent(false, true), Some(TouchIntent::Stop));
        let legacy_stop = rig.legacy().latest_stop.unwrap();
        assert_eq!(legacy_stop.intent, ControlIntent::Stop);
        assert_eq!(legacy_stop.released_us, stops[0].released_us.unwrap());
        spoiled(&run(&mut rig, 1_100_000..3_700_000, false, true), contact);

        if expired {
            expire_push(&rig);
        } else {
            say(&mut body, 3_700_000, false);
        }
        // Neither clear route rescues the old contact, even after another
        // complete sleep-hold interval. A short pad gap does not end it.
        spoiled(&run(&mut rig, 3_700_000..7_000_000, false, true), contact);
        spoiled(&run(&mut rig, 7_000_000..7_070_000, false, false), contact);
        spoiled(&run(&mut rig, 7_070_000..7_200_000, false, true), contact);

        // The gap is measured from the last active acquisition (7.19 s).
        let release = run(&mut rig, 7_200_000..7_400_000, false, false);
        assert!(events(&release).is_empty());
        spoiled(&release[..14], contact); // Last sample is 140 ms after active.
        assert_eq!(source(&release[14], TouchSource::Head).contact_id, None);

        let mut tap = run(&mut rig, 7_400_000..7_600_000, false, true);
        tap.extend(run(&mut rig, 7_600_000..7_800_000, false, false));
        let tap = events(&tap);
        assert_eq!(
            tap.iter()
                .map(|event| (event.source, event.gesture))
                .collect::<Vec<_>>(),
            [
                (TouchSource::Head, TouchGesture::Touch),
                (TouchSource::Head, TouchGesture::Tap)
            ]
        );
        assert_ne!(tap[0].contact_id, contact);
        assert_eq!(tap[0].contact_id, tap[1].contact_id);
        assert_eq!(tap[0].observed_us, 7_420_000);
        assert_eq!(tap[1].released_us, Some(7_600_000));
        assert_eq!(tap[1].observed_us, 7_740_000);
        assert_eq!(tap[1].intent(false, true), Some(TouchIntent::Wake));

        let hold = run(&mut rig, 7_800_000..10_830_000, false, true);
        let held = source(hold.last().unwrap(), TouchSource::Head);
        assert_eq!(held.held_us, Some(3_020_000));
        let hold = events(&hold);
        assert_eq!(
            hold.iter()
                .map(|event| (event.source, event.gesture))
                .collect::<Vec<_>>(),
            [
                (TouchSource::Head, TouchGesture::Touch),
                (TouchSource::Head, TouchGesture::Stroke),
                (TouchSource::Head, TouchGesture::Hold)
            ]
        );
        assert!(
            hold.iter()
                .all(|event| event.contact_id == hold[0].contact_id)
        );
        assert_ne!(hold[0].contact_id, tap[0].contact_id);
        assert_eq!(hold[2].observed_us, 10_800_000);
        assert_eq!(hold[2].intent(false, false), Some(TouchIntent::Sleep));
        assert!(events(&run(&mut rig, 10_830_000..11_050_000, false, false)).is_empty());
        if expired {
            // A later false publication must not conceal an expiry reader
            // that failed to clear its push state.
            assert_expired_push(&rig);
        }
    }

    #[test]
    fn a_body_release_requires_a_new_contact_before_tap_or_hold() {
        recovery(false);
    }

    #[test]
    fn an_expired_body_push_requires_a_new_contact_before_tap_or_hold() {
        recovery(true);
    }

    #[test]
    fn a_body_push_preserves_an_already_qualified_stop() {
        let mut rig = Rig::new(true);
        run(&mut rig, 0..600_000, false, false);
        let touch = run(&mut rig, 600_000..640_000, false, true);
        let touch = events(&touch);
        assert_eq!(touch.len(), 1);
        let stop = touch[0];
        assert_eq!(stop.source, TouchSource::Head);
        assert_eq!(stop.gesture, TouchGesture::Touch);
        assert_eq!(stop.intent(true, false), Some(TouchIntent::Stop));

        let mut body = body(&rig);
        say(&mut body, 640_000, true);
        let pushed = run(&mut rig, 640_000..700_000, false, true);
        spoiled(&pushed, stop.contact_id);
        for output in &pushed {
            assert!(output.state.latest.contains(stop));
        }
        say(&mut body, 700_000, false);
        let cleared = run(&mut rig, 700_000..800_000, false, true);
        spoiled(&cleared, stop.contact_id);
        for output in &cleared {
            assert!(output.state.latest.contains(stop));
        }
        spoiled(
            &run(&mut rig, 800_000..3_700_000, false, true),
            stop.contact_id,
        );
        assert!(events(&run(&mut rig, 3_700_000..3_900_000, false, false)).is_empty());
    }

    #[test]
    fn a_body_push_is_read_before_the_gpio_that_would_qualify_touch() {
        let mut rig = Rig::new(true);
        run(&mut rig, 0..600_000, false, false);
        let began = run(&mut rig, 600_000..620_000, false, true);
        assert!(events(&began).is_empty());
        let contact = source(&began[0], TouchSource::Head).contact_id.unwrap();
        let mut body = body(&rig);
        say(&mut body, 620_000, true);
        // 620 ms is both the 20 ms qualification boundary and a due body
        // poll. Reading body after GPIO would let one Stop escape here.
        spoiled(&run(&mut rig, 620_000..700_000, false, true), contact);
    }
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

/// The project lead, 19 Oct: the base strip isn't used. Without its switch,
/// a pins file's strip and the config's selected mask are set aside: the
/// program starts no strip worker (it starts one only for a `base`), the
/// touch state has no strip source, and the head pads and the button work
/// as before. Only `on` keeps the strip.
#[test]
fn without_its_switch_the_strip_is_off_and_the_head_pads_still_work() {
    use lamp_controls::touch_pins::{STRIP_ENV, serving};
    assert_eq!(STRIP_ENV, "LAMP_CONTROLS_STRIP");
    for switch in [None, Some("off"), Some("yes"), Some("ON")] {
        let (served, config, set_aside) = serving(wiring(), config(true), switch);
        assert!(set_aside, "{switch:?}");
        assert!(served.base.is_none(), "{switch:?}");
        assert_eq!(config.base_selected_mask, None);
        // Everything else as written: the head pads, the button, the reset.
        assert_eq!(
            served,
            TouchPinsConfig {
                base: None,
                ..wiring()
            }
        );
        let mut rig = Rig::with_config(config, served);
        for at in (0..=1_000_000).step_by(50_000) {
            rig.gpio(at, false, false, false);
            let output = rig.step(at);
            assert!(
                output
                    .state
                    .sources
                    .iter()
                    .all(|state| state.source != TouchSource::Strip),
                "{:?}",
                output.state.sources
            );
        }
        let mut events = Vec::new();
        for at in [1_020_000, 1_040_000] {
            rig.gpio(at, false, false, true);
            events.extend(rig.step(at).events);
        }
        assert!(
            events
                .iter()
                .any(|event| event.source == TouchSource::Head
                    && event.gesture == TouchGesture::Touch),
            "{events:?}"
        );
        let output = rig.step(1_060_000);
        let head = source(&output, TouchSource::Head);
        assert!(head.ready && head.fault.is_none(), "{head:?}");
        assert!(source(&output, TouchSource::Primary).fault.is_none());
    }
    // On, the strip is as configured, and the touch state has it.
    let (served, kept, set_aside) = serving(wiring(), config(true), Some("on"));
    assert!(!set_aside);
    assert_eq!(served, wiring());
    assert_eq!(kept.base_selected_mask, Some(4095));
    let mut rig = Rig::with_config(kept, served);
    rig.gpio(0, false, false, false);
    let output = rig.step(0);
    assert!(
        output
            .state
            .sources
            .iter()
            .any(|state| state.source == TouchSource::Strip)
    );
    // A pins file with no strip, as the product's: nothing set aside.
    let (served, _, set_aside) = serving(
        TouchPinsConfig {
            base: None,
            ..wiring()
        },
        Config {
            base_selected_mask: None,
            ..config(true)
        },
        None,
    );
    assert!(!set_aside);
    assert!(served.base.is_none());
}

/// A privacy switch that always reads open.
struct Open;
impl Lines for Open {
    fn high(&mut self, _: u32) -> std::io::Result<bool> {
        Ok(true)
    }
}

fn attach_open_privacy(rig: &mut Rig) {
    rig.service
        .attach_privacy(
            &rig.bus,
            Source::new(
                Box::new(LineReader::new(Open, CLOCK, "privacy-reader")),
                CLOCK,
            ),
        )
        .unwrap();
}

/// #2602 (the first builder, 20:2x): a touch reader that fails mid-run
/// holds nothing back from the privacy reading. Through 200 ms of failed
/// reads, with the reset not read, every `controls/state` carries a fresh
/// open reading, and one comes at least every 20 ms, as before it.
#[test]
fn a_touch_reader_failing_mid_run_leaves_the_privacy_reading_fresh() {
    let mut rig = Rig::new(true);
    attach_open_privacy(&mut rig);
    let mut states = rig
        .bus
        .subscribe(ControlsState::TOPIC, "test_state")
        .unwrap();
    let (mut said, mut not_read) = (Vec::new(), 0);
    for at in (0..400_000u64).step_by(1_000) {
        if at % 10_000 == 0 {
            if (150_000..350_000).contains(&at) {
                rig.gpio
                    .values
                    .borrow_mut()
                    .push_back(Err(InputError::new(ControlFault::Missing, "GPIO lost")));
            } else {
                rig.gpio(at, false, false, false);
            }
        }
        let output = rig.step(at);
        if source(&output, TouchSource::Reset).active.is_none() && at >= 150_000 {
            not_read += 1;
        }
        while let Some(message) = states.recv(Duration::ZERO).unwrap() {
            said.push(message.parse::<ControlsState>().unwrap());
        }
    }
    assert!(not_read > 0, "the reset was read throughout");
    let first = said
        .iter()
        .position(|state| state.privacy.as_ref().is_some_and(|p| p.fault.is_none()))
        .unwrap();
    for pair in said[first..].windows(2) {
        assert!(
            pair[1].stamp.at_us - pair[0].stamp.at_us <= 20_000,
            "nothing said from {} to {}",
            pair[0].stamp.at_us,
            pair[1].stamp.at_us
        );
    }
    for state in &said[first..] {
        let privacy = state.privacy.as_ref().unwrap();
        assert_eq!(privacy.fault, None, "at {}", state.stamp.at_us);
        assert_eq!(privacy.muted, Some(false));
        assert!(state.stamp.at_us - privacy.stamp.at_us < 50_000);
    }
    assert!(said.last().unwrap().stamp.at_us >= 380_000);
    assert_eq!(rig.service.pace.long_gaps, 0);
}

/// #2602: what lamp-75c9 showed is the loop not running. After 100 ms with
/// no step, the reset reads "not read" and the privacy reading has lapsed,
/// together, since they share the loop; the gap is counted (`long_gaps`).
#[test]
fn a_loop_that_stalls_for_100_ms_lapses_the_reset_and_the_privacy_reading_together() {
    let mut rig = Rig::new(true);
    attach_open_privacy(&mut rig);
    for at in (0..=100_000u64).step_by(1_000) {
        if at % 10_000 == 0 {
            rig.gpio(at, false, false, false);
        }
        rig.step(at);
    }
    assert_eq!(rig.legacy().privacy.unwrap().fault, None);
    assert_eq!(
        (rig.service.pace.long_gaps, rig.service.pace.longest_gap_us),
        (0, 1_000)
    );
    rig.gpio(200_000, false, false, false);
    let output = rig.step(200_000);
    assert_eq!(source(&output, TouchSource::Reset).active, None);
    assert_eq!(
        rig.legacy().privacy.unwrap().fault,
        Some(ControlFault::Stale)
    );
    assert_eq!(
        (rig.service.pace.long_gaps, rig.service.pace.longest_gap_us),
        (1, 100_000)
    );
}

/// #2602: a gap past the privacy reading's 50 ms is counted, and said in
/// the journal at most once a second, with how many more there were.
#[test]
fn a_long_gap_between_steps_is_counted_and_said_at_most_once_a_second() {
    let mut gaps = Pace::default();
    assert_eq!(gaps.step(0), None);
    assert_eq!(gaps.step(1_000), None);
    let line = gaps.step(61_000).unwrap();
    assert!(line.contains("didn't run for 60 ms"), "{line}");
    assert_eq!(gaps.step(62_000), None);
    assert_eq!(gaps.step(200_000), None, "within a second of the last line");
    let line = gaps.step(1_200_000).unwrap();
    assert!(line.contains("(1 more since the last line)"), "{line}");
    assert_eq!((gaps.long_gaps, gaps.longest_gap_us), (3, 1_000_000));
}

/// #2602 (the first builder's guard for Controls at priority 30): a step
/// whose elapsed time takes 1 ms or more is counted in health (`slow_steps`),
/// and the loop rests at least as long as its CPU work, so it never spins.
#[test]
fn a_slow_step_is_counted_and_the_loop_rests_at_least_its_work() {
    let mut rig = Rig::new(true);
    attach_open_privacy(&mut rig);
    rig.gpio(0, false, false, false);
    rig.step(0);
    assert_eq!(rig.service.pace.slow_steps, 0);
    // Each look at the clock 200 us after the last: a step of several.
    let clock = Cell::new(1_000);
    rig.service.step(|| {
        clock.set(clock.get() + 200);
        clock.get()
    });
    assert!(rig.service.counts.longest_step_us >= 1_000);
    assert_eq!(rig.service.pace.slow_steps, 1);
    assert_eq!(rest_us(100, 100, 1_000), 900);
    assert_eq!(rest_us(500, 500, 1_000), 500);
    assert_eq!(rest_us(800, 800, 1_000), 800);
    assert_eq!(rest_us(5_000, 5_000, 1_000), 5_000);
    assert_eq!(rest_us(30_000, 200, 1_000), 200);
}

/// #2602 (the first builder, 20:3x): Controls runs at real-time round-robin
/// priority 30 in the product unit and lamp-4ace's, above the voice's, so
/// its loop, which reads the privacy switch, never waits behind the voice's
/// real-time work.
#[test]
fn controls_runs_at_real_time_priority_above_the_voice() {
    fn setting<'a>(unit: &'a str, key: &str) -> Option<&'a str> {
        unit.lines().find_map(|line| line.strip_prefix(key))
    }
    fn priority(unit: &str) -> u32 {
        setting(unit, "CPUSchedulingPriority=")
            .unwrap()
            .parse()
            .unwrap()
    }
    for (controls, voice) in [
        (
            include_str!("../../../../deploy/product/lamp-controls.service"),
            include_str!("../../../../deploy/product/lamp-voice.service"),
        ),
        (
            include_str!("../../../../deploy/lamp-4ace/lamp-controls.service"),
            include_str!("../../../../deploy/lamp-4ace/lamp-voice.service"),
        ),
    ] {
        assert_eq!(setting(controls, "CPUSchedulingPolicy="), Some("rr"));
        assert_eq!(priority(controls), 30);
        // The hand's strip worker must not fall below the voice's priority.
        assert_eq!(setting(controls, "CPUSchedulingResetOnFork="), None);
        assert!(priority(controls) > priority(voice));
        // Niceness means nothing at real-time priority.
        assert_eq!(setting(controls, "Nice="), None);
    }
}

/// The first strip Touch from a hand laid on three pads at 40 ms, read every
/// 10 ms until `until`, and how many strip events were seen.
fn strip_touch(rig: &mut Rig, until: u64) -> (Option<u64>, usize) {
    let (mut first, mut events) = (None, 0);
    for at in (0..=until).step_by(10_000) {
        rig.gpio(at, false, false, false);
        rig.strip(at, if at >= 40_000 { 7 } else { 0 });
        for event in rig.step(at).events {
            if event.source == TouchSource::Strip {
                events += 1;
                if event.gesture == TouchGesture::Touch {
                    first.get_or_insert(at);
                }
            }
        }
    }
    (first, events)
}

/// #2858: with its switch off, a hand on the strip is no event at all, even
/// with its reader giving touched readings; on, the same hand is a Touch.
#[test]
fn the_strip_switch_off_means_no_strip_event_with_a_hand_on_it() {
    use lamp_controls::touch_pins::serving;
    for switch in [None, Some("off")] {
        let (served, config, _) = serving(wiring(), config(true), switch);
        let mut rig = Rig::with_config(config, served);
        assert_eq!(strip_touch(&mut rig, 300_000), (None, 0), "{switch:?}");
    }
    let (served, config, _) = serving(wiring(), config(true), Some("on"));
    let mut rig = Rig::with_config(config, served);
    assert!(strip_touch(&mut rig, 300_000).0.is_some());
}

/// #2858: the strip's timing is the board's pins file's; a board that gives
/// none keeps the defaults; a value out of bounds is refused.
#[test]
fn the_strip_timing_is_the_boards_and_defaults_without_it() {
    use lamp_controls::base::strip::StripTiming;
    let touch = wiring().check(&config(true)).unwrap();
    assert_eq!(touch.strip_timing(), StripTiming::default());
    let tuned = TouchPinsConfig {
        strip_qualify_ms: Some(150),
        strip_release_ms: Some(60),
        strip_stroke_ms: Some(500),
        strip_sleep_ms: Some(3_000),
        ..wiring()
    };
    let touch = tuned.check(&config(true)).unwrap();
    assert_eq!(
        touch.strip_timing(),
        StripTiming {
            qualify_us: 150_000,
            release_us: 60_000,
            stroke_us: 500_000,
            sleep_us: 3_000_000,
        }
    );
    // The board's qualify is what the strip waits for.
    let (default_at, _) = strip_touch(&mut Rig::new(true), 400_000);
    let (tuned_at, _) = strip_touch(&mut Rig::with(true, tuned), 400_000);
    let (default_at, tuned_at) = (default_at.unwrap(), tuned_at.unwrap());
    assert!(
        tuned_at >= default_at + 130_000,
        "{default_at} then {tuned_at}"
    );
    for wrong in [
        TouchPinsConfig {
            strip_qualify_ms: Some(5),
            ..wiring()
        },
        TouchPinsConfig {
            strip_sleep_ms: Some(60_000),
            ..wiring()
        },
        // Below the 2 s a strip Hold must be old enough for (TouchEvent::check).
        TouchPinsConfig {
            strip_sleep_ms: Some(1_999),
            ..wiring()
        },
    ] {
        assert!(wrong.check(&config(true)).is_err());
    }
}

/// #2896: a still hand held past the board's sleep is a Hold the message
/// check accepts, at the lowest sleep allowed and above it.
#[test]
fn a_strip_hold_at_the_boards_sleep_is_a_valid_event() {
    for sleep_ms in [2_000, 3_000] {
        let mut rig = Rig::with(
            true,
            TouchPinsConfig {
                strip_sleep_ms: Some(sleep_ms),
                ..wiring()
            },
        );
        let mut hold = None;
        for at in (0..=(sleep_ms + 200) * 1_000).step_by(10_000) {
            rig.gpio(at, false, false, false);
            rig.strip(at, if at >= 40_000 { 7 } else { 0 });
            // Rig::step checks every event against its message rules.
            for event in rig.step(at).events {
                if event.source == TouchSource::Strip && event.gesture == TouchGesture::Hold {
                    hold.get_or_insert(at);
                }
            }
        }
        let hold = hold.unwrap_or_else(|| panic!("no Hold at sleep {sleep_ms} ms"));
        assert!(hold >= 40_000 + sleep_ms * 1_000, "{sleep_ms}: {hold}");
    }
}

/// The product lamp serves `--touch` (queue #3997, C): its `controls/state`
/// says its monotonic clock as the plain runtime's does, only when its clock
/// is this boot's, whether the pins' primary button made its owner or the
/// privacy route did with no button at all.
#[test]
fn the_touch_runtimes_state_says_its_monotonic_clock_only_when_its_clock_is_this_boot() {
    for buttons in [true, false] {
        for (boot, said) in [
            (Some(CLOCK), Some(CLOCK)),
            (Some("another-boot"), None),
            (None, None),
        ] {
            let root = Root::new();
            let bus = Bus::new(&root.0)
                .unwrap()
                .rescan_every(Duration::ZERO)
                .boot_id(boot);
            let mut pins = wiring();
            if !buttons {
                pins.buttons.clear();
            }
            let mut config = config(true);
            if !buttons {
                config.buttons.clear();
            }
            let mut service =
                Service::new(&bus, config, pins, Gpio::default(), Strip::default(), CLOCK).unwrap();
            if !buttons {
                service.mark_no_slide(&bus, CLOCK).unwrap();
            }
            service.step(|| 1_000_000);
            let kept = bus.latest(ControlsState::TOPIC).unwrap().unwrap();
            assert_eq!(kept.header.clock.as_deref(), said, "{buttons} {boot:?}");
            let state: ControlsState = kept.parse().unwrap();
            assert_eq!(kept.header.at_us, state.stamp.at_us);
            // Its other topics say nothing of the kind.
            let touch = bus.latest(TouchState::TOPIC).unwrap().unwrap();
            assert!(touch.header.clock.is_none());
        }
    }
}
