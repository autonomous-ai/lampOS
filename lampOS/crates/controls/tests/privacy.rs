use lamp_bus::Bus;
use lamp_controls::{
    engine::Config,
    input::{Level, Observation, ScriptedPins},
    pins::{Lines, Pull},
    privacy::{self, LineReader, Monitor, Reader, Source},
    replay,
    runtime::{MissingPins, Service},
    strip_worker::Worker,
    touch_pins::{NoGpio, TouchPinsConfig},
    touch_runtime,
};
use lamp_msgs::{ClockStamp, ControlFault, ControlsState, PrivacySwitch};
use std::{
    cell::Cell,
    ffi::OsStr,
    fs, io,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

const CLOCK: &str = "privacy-test";
static NEXT: AtomicU64 = AtomicU64::new(1);

fn sample(at: u64, sequence: u64, muted: bool) -> PrivacySwitch {
    PrivacySwitch {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us: at,
            sequence,
        },
        reader_session_id: "privacy-reader".into(),
        muted: Some(muted),
        fault: None,
    }
}

struct Electrical {
    high: Rc<Cell<bool>>,
    fail: Rc<Cell<bool>>,
    reads: Rc<Cell<usize>>,
}
impl Lines for Electrical {
    fn high(&mut self, line: u32) -> io::Result<bool> {
        assert_eq!(line, 9);
        self.reads.set(self.reads.get() + 1);
        if self.fail.get() {
            Err(io::Error::other("lost chip"))
        } else {
            Ok(self.high.get())
        }
    }
}
type LineFixture = (Electrical, Rc<Cell<bool>>, Rc<Cell<bool>>, Rc<Cell<usize>>);

fn line(high: bool) -> LineFixture {
    let high = Rc::new(Cell::new(high));
    let fail = Rc::new(Cell::new(false));
    let reads = Rc::new(Cell::new(0));
    (
        Electrical {
            high: high.clone(),
            fail: fail.clone(),
            reads: reads.clone(),
        },
        high,
        fail,
        reads,
    )
}

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "controls-privacy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn bus(&self) -> Bus {
        Bus::new(self.0.join("bus")).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn only_exact_on_selects_the_separate_fixed_wiring() {
    for value in [None, Some("off"), Some("ON"), Some("true"), Some("")] {
        assert!(!privacy::selected(value.map(OsStr::new)));
    }
    assert!(privacy::selected(Some(OsStr::new("on"))));
    let pins: lamp_controls::pins::PinsConfig =
        serde_json::from_str(include_str!("../fixtures/privacy-pins.json")).unwrap();
    pins.check(&Config::default()).unwrap();
    let original = pins.privacy.unwrap();
    for edit in 0..4 {
        let mut pins = original.clone();
        match edit {
            0 => pins.chip = "/dev/gpiochip0".into(),
            1 => pins.line = 11,
            2 => pins.active_low = false,
            _ => pins.pull = Pull::Down,
        }
        assert!(pins.check().is_err());
    }
}

#[test]
fn the_privacy_line_cannot_also_be_a_button_or_head_pad() {
    for head in [false, true] {
        let mut pins: lamp_controls::pins::PinsConfig =
            serde_json::from_str(include_str!("../fixtures/privacy-pins.json")).unwrap();
        pins.chip = "/dev/gpiochip1".into();
        let pin = lamp_controls::pins::Pin {
            name: "hand".into(),
            line: 9,
            active_low: true,
            pull: Pull::Up,
        };
        let mut config = Config::default();
        if head {
            pins.head_pads.push(pin);
            config.head_pads.push("hand".into());
        } else {
            pins.buttons.push(pin);
            config.buttons.push("hand".into());
        }
        assert_eq!(
            pins.check(&config).unwrap_err(),
            "the privacy line cannot also be a hand input"
        );
        // Offsets on different chips are different electrical lines.
        pins.chip = "/dev/gpiochip0".into();
        pins.check(&config).unwrap();
    }
}

#[test]
fn startup_low_is_immediate_and_high_is_only_fresh_electrical_evidence() {
    for high in [false, true] {
        let (lines, _, fail, reads) = line(high);
        let mut reader = LineReader::new(lines, CLOCK, "privacy-reader");
        let first = reader.read(1_000).unwrap();
        assert_eq!(first.muted, Some(!high));
        assert_eq!(first.stamp.sequence, 1);
        assert!(reader.read(5_999).is_none());
        assert_eq!(reads.get(), 1);
        let second = reader.read(6_000).unwrap();
        assert_eq!(second.stamp.at_us, 6_000);
        assert_eq!(second.stamp.sequence, 2);
        fail.set(true);
        let missing = reader.read(11_000).unwrap();
        assert_eq!(missing.muted, None);
        assert_eq!(missing.fault, Some(ControlFault::Missing));
        assert!(!missing.fresh_at(CLOCK, 11_000));
    }
}

#[test]
fn missing_and_stale_never_become_a_released_level_or_renew_the_stamp() {
    let mut monitor = Monitor::new(CLOCK);
    let missing = monitor.snapshot(1_000);
    missing.check().unwrap();
    assert_eq!(missing.fault, Some(ControlFault::Missing));
    monitor.observe(1_000, sample(1_000, 1, false)).unwrap();
    assert!(monitor.snapshot(50_999).fresh_at(CLOCK, 50_999));
    let stale = monitor.snapshot(51_000);
    assert_eq!(stale.muted, None);
    assert_eq!(stale.fault, Some(ControlFault::Stale));
    assert_eq!(stale.stamp, sample(1_000, 1, false).stamp);
    assert_eq!(
        monitor
            .observe(56_000, sample(56_000, 2, false))
            .unwrap_err()
            .fault,
        ControlFault::Stale
    );
    monitor.observe(61_000, sample(61_000, 3, false)).unwrap();
}

#[test]
fn malformed_future_wrong_clock_order_and_gaps_withdraw_only_privacy() {
    for case in 0..6 {
        let mut monitor = Monitor::new(CLOCK);
        monitor.observe(1_000, sample(1_000, 1, true)).unwrap();
        let mut next = sample(6_000, 2, false);
        let (now, expected) = match case {
            0 => (5_999, ControlFault::Future),
            1 => {
                next.stamp.clock_id = "other-boot".into();
                (6_000, ControlFault::WrongClock)
            }
            2 => {
                next.stamp.sequence = 1;
                (6_000, ControlFault::OutOfOrder)
            }
            3 => {
                next.stamp.sequence = 3;
                (6_000, ControlFault::Overflow)
            }
            4 => {
                next.muted = None;
                (6_000, ControlFault::Invalid)
            }
            _ => (56_000, ControlFault::Stale),
        };
        assert_eq!(monitor.observe(now, next).unwrap_err().fault, expected);
        let state = monitor.snapshot(now);
        state.check().unwrap();
        assert_eq!(state.muted, None);
        assert_eq!(state.fault, Some(expected));
    }
}

#[test]
fn restart_retirement_and_clock_reversal_cannot_reopen_evidence() {
    let mut monitor = Monitor::new(CLOCK);
    monitor.observe(1_000, sample(1_000, 1, true)).unwrap();
    let mut changed = sample(6_000, 1, false);
    changed.reader_session_id = "new-reader".into();
    assert_eq!(
        monitor.observe(6_000, changed.clone()).unwrap_err().fault,
        ControlFault::ReaderChanged
    );
    assert_eq!(monitor.snapshot(6_000).muted, None);
    changed.stamp.at_us = 11_000;
    changed.stamp.sequence = 2;
    monitor.observe(11_000, changed).unwrap();
    assert_eq!(
        monitor
            .observe(16_000, sample(16_000, 2, false))
            .unwrap_err()
            .fault,
        ControlFault::ReaderChanged
    );
    let reversed = monitor.snapshot(5_000);
    reversed.check().unwrap();
    assert_eq!(reversed.stamp.at_us, 11_000);
    assert_eq!(reversed.stamp.sequence, 2);
    assert_eq!(reversed.fault, Some(ControlFault::ClockReset));
    assert_eq!(
        monitor
            .observe(21_000, sample(21_000, 3, false))
            .unwrap_err()
            .fault,
        ControlFault::ClockReset
    );
}

#[test]
fn a_slow_acquisition_is_aged_after_the_read_returns() {
    let (lines, _, _, _) = line(true);
    let mut source = Source::new(
        Box::new(LineReader::new(lines, CLOCK, "privacy-reader")),
        CLOCK,
    );
    let calls = Cell::new(0);
    source.poll(|| {
        let call = calls.get();
        calls.set(call + 1);
        if call == 0 { 1_000 } else { 61_000 }
    });
    assert_eq!(
        source.monitor.snapshot(61_000).fault,
        Some(ControlFault::Stale)
    );
}

#[test]
fn privacy_only_state_exists_and_gesture_faults_do_not_hide_the_switch() {
    let root = Root::new();
    let bus = root.bus();
    let (lines, high, _, _) = line(false);
    let mut service = Service::new(&bus, Config::default(), MissingPins, CLOCK).unwrap();
    service.attach_privacy(Source::new(
        Box::new(LineReader::new(lines, CLOCK, "privacy-reader")),
        CLOCK,
    ));
    let first = service.step(|| 0);
    first.state.check().unwrap();
    assert_eq!(first.state.fault, Some(ControlFault::Missing));
    assert_eq!(first.state.privacy.unwrap().muted, Some(true));
    high.set(true);
    service.step(|| 6_000);
    let state: ControlsState = bus
        .latest(ControlsState::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(state.stamp.at_us, 6_000);
    assert_eq!(state.privacy.unwrap().muted, Some(false));
    service.step(|| 26_000);
    let state: ControlsState = bus
        .latest(ControlsState::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(state.stamp.at_us, 26_000);
}

#[test]
fn a_privacy_read_failure_does_not_cancel_healthy_hand_acquisition() {
    let root = Root::new();
    let bus = root.bus();
    let (lines, _, fail, _) = line(false);
    fail.set(true);
    let config = Config {
        buttons: vec!["primary".into()],
        ..Config::default()
    };
    let hands = ScriptedPins::new([Observation {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us: 0,
            sequence: 1,
        },
        session_id: "hands".into(),
        buttons: vec![Level {
            name: "primary".into(),
            active: false,
        }],
        head_pads: vec![],
        base_mask: None,
    }])
    .unwrap();
    let mut service = Service::new(&bus, config, hands, CLOCK).unwrap();
    service.attach_privacy(Source::new(
        Box::new(LineReader::new(lines, CLOCK, "privacy-reader")),
        CLOCK,
    ));
    let state = service.step(|| 0).state;
    state.check().unwrap();
    assert!(state.ready);
    assert_eq!(state.privacy.unwrap().fault, Some(ControlFault::Missing));
}

#[test]
fn base_only_touch_publishes_controls_state_and_strip_failure_is_independent() {
    let root = Root::new();
    let bus = root.bus();
    let (lines, _, _, _) = line(false);
    let wiring: TouchPinsConfig = serde_json::from_str(include_str!(
        "../fixtures/strip-711-working-pins-example.json"
    ))
    .unwrap();
    let config = Config {
        base_selected_mask: Some(
            wiring
                .base
                .as_ref()
                .unwrap()
                .pads
                .iter()
                .fold(0, |mask, &pad| mask | (1 << pad)),
        ),
        ..Config::default()
    };
    let mut service =
        touch_runtime::Service::new(&bus, config, wiring, NoGpio, None::<Worker>, CLOCK).unwrap();
    service
        .attach_privacy(
            &bus,
            Source::new(
                Box::new(LineReader::new(lines, CLOCK, "privacy-reader")),
                CLOCK,
            ),
        )
        .unwrap();
    service.strip_unavailable(0, "no strip");
    service.step(|| 0);
    let state: ControlsState = bus
        .latest(ControlsState::TOPIC)
        .unwrap()
        .unwrap()
        .parse()
        .unwrap();
    state.check().unwrap();
    assert_eq!(state.privacy.unwrap().muted, Some(true));
    assert!(!state.ready);
}

/// El on #1469: with the privacy switch attached, `controls/state` keeps
/// its 20 ms pace in a still second (the reading lasts 50 ms), while the
/// touch state, with nothing changing, goes out every 100 ms.
#[test]
fn with_privacy_attached_a_still_second_says_controls_state_fifty_times() {
    // Short: a subscriber's socket path must fit in 103 bytes, which the
    // Mac's temporary folder leaves no room for.
    let root = std::path::PathBuf::from(format!("/tmp/cpv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bus = Bus::new(&root).unwrap();
    let (lines, _, _, _) = line(false);
    let wiring: TouchPinsConfig = serde_json::from_str(include_str!(
        "../fixtures/strip-711-working-pins-example.json"
    ))
    .unwrap();
    let config = Config {
        base_selected_mask: Some(
            wiring
                .base
                .as_ref()
                .unwrap()
                .pads
                .iter()
                .fold(0, |mask, &pad| mask | (1 << pad)),
        ),
        ..Config::default()
    };
    let mut service =
        touch_runtime::Service::new(&bus, config, wiring, NoGpio, None::<Worker>, CLOCK).unwrap();
    service
        .attach_privacy(
            &bus,
            Source::new(
                Box::new(LineReader::new(lines, CLOCK, "privacy-reader")),
                CLOCK,
            ),
        )
        .unwrap();
    service.strip_unavailable(0, "no strip");
    let mut legacy = bus.subscribe(ControlsState::TOPIC, "test_legacy").unwrap();
    let mut touch = bus
        .subscribe(lamp_msgs::TouchState::TOPIC, "test_touch")
        .unwrap();
    let mut counts = (0, 0);
    for at in (0..2_000_000u64).step_by(2_000) {
        service.step(|| at);
        if at == 1_000_000 {
            counts = (0, 0);
        }
        while legacy.recv(std::time::Duration::ZERO).unwrap().is_some() {
            counts.0 += 1;
        }
        while touch.recv(std::time::Duration::ZERO).unwrap().is_some() {
            counts.1 += 1;
        }
    }
    drop((service, legacy, touch));
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        (45..=55).contains(&counts.0),
        "{} controls states",
        counts.0
    );
    assert!((9..=11).contains(&counts.1), "{} touch states", counts.1);
}

#[test]
fn legacy_bytes_omit_privacy_and_new_fields_reject_ambiguous_evidence() {
    let old = r#"{"stamp":{"clock_id":"privacy-test","at_us":0,"sequence":1},"session_id":"controls-replay","enabled":false,"ready":false,"fault":"missing","buttons":[],"head_active":false,"base_ready":false,"base_tier":"none","event_sequence":0,"rejected_inputs":0}"#;
    let state: ControlsState = serde_json::from_str(old).unwrap();
    state.check().unwrap();
    assert_eq!(serde_json::to_string(&state).unwrap(), old);
    let mut both = sample(0, 1, true);
    both.fault = Some(ControlFault::Missing);
    assert!(both.check().is_err());
    let mut value = serde_json::to_value(sample(0, 1, true)).unwrap();
    value["permission"] = true.into();
    assert!(serde_json::from_value::<PrivacySwitch>(value).is_err());
    let mut stale = state;
    stale.stamp.at_us = 50_000;
    stale.privacy = Some(sample(0, 1, false));
    assert!(stale.check().is_err());
}

#[test]
fn authored_replay_is_deterministic_keeps_faults_and_never_grants_hand_authority() {
    let records =
        replay::read(include_bytes!("../fixtures/privacy-replay.jsonl").as_slice()).unwrap();
    let mut first = vec![];
    let mut second = vec![];
    for bytes in [&mut first, &mut second] {
        replay::run_with_privacy(Config::default(), CLOCK, &records, None, bytes, true).unwrap();
    }
    assert_eq!(first, second);
    let frames: Vec<replay::Frame> = std::str::from_utf8(&first)
        .unwrap()
        .lines()
        .map(|row| serde_json::from_str(row).unwrap())
        .collect();
    assert_eq!(
        frames[0].output.state.privacy.as_ref().unwrap().muted,
        Some(true)
    );
    for frame in &frames {
        frame.output.state.check().unwrap();
        assert!(!frame.output.state.enabled);
        assert!(frame.output.gestures.is_empty());
    }
    assert_eq!(
        frames
            .last()
            .unwrap()
            .output
            .state
            .privacy
            .as_ref()
            .unwrap()
            .fault,
        Some(ControlFault::Stale)
    );
}

#[test]
fn reader_identity_storage_is_bounded_and_exhaustion_stays_unknown() {
    let mut monitor = Monitor::new(CLOCK);
    monitor.observe(0, sample(0, 1, false)).unwrap();
    for index in 1..=33 {
        let at = index * 10_000;
        let mut next = sample(at, 1, false);
        next.reader_session_id = format!("privacy-{index}");
        let expected = if index <= 32 {
            ControlFault::ReaderChanged
        } else {
            ControlFault::Overflow
        };
        assert_eq!(monitor.observe(at, next).unwrap_err().fault, expected);
    }
    assert_eq!(
        monitor.snapshot(340_000).fault,
        Some(ControlFault::Overflow)
    );
    assert_eq!(monitor.snapshot(340_000).muted, None);
}

#[test]
fn privacy_only_touch_recording_replays_the_actual_tagged_acquisitions() {
    let root = Root::new();
    let bus = root.bus();
    let (lines, high, _, _) = line(false);
    let mut service = Service::new(&bus, Config::default(), NoGpio, CLOCK).unwrap();
    service.attach_privacy(Source::new(
        Box::new(LineReader::new(lines, CLOCK, "privacy-reader")),
        CLOCK,
    ));
    let path = root.0.join("privacy-touch.jsonl");
    service.record_privacy_touch_to(&path).unwrap();
    for at in [0, 5_000, 10_000] {
        high.set(at != 0);
        service.step(|| at);
    }
    // The lamp's 100 ms finish window covers the writer's last `sync_all`,
    // which a busy test machine can take longer over: five seconds here,
    // still failing a writer that never finishes (#1542).
    let stats = service
        .finish_recording_within(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(stats.finished && !stats.failed);
    assert_eq!(stats.dropped, 0);
    let records = lamp_controls::touch_replay::read(fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(records.len(), 3);
    assert!(records.iter().all(|record| matches!(
        record.input,
        lamp_controls::touch_replay::Input::Privacy { .. }
    )));
    let mut output = vec![];
    lamp_controls::touch_replay::run_privacy_only(
        Config::default(),
        CLOCK,
        &records,
        None,
        &mut output,
    )
    .unwrap();
    let frames: Vec<replay::Frame> = std::str::from_utf8(&output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        frames[0].output.state.privacy.as_ref().unwrap().muted,
        Some(true)
    );
    assert_eq!(
        frames
            .last()
            .unwrap()
            .output
            .state
            .privacy
            .as_ref()
            .unwrap()
            .fault,
        Some(ControlFault::Stale)
    );
}

#[test]
#[ignore = "host measurement: injected GPIO read to retained Bus observation, no hardware"]
fn synthetic_line_to_bus_latency_with_stalled_strip() {
    use lamp_controls::{input::InputError, strip_worker::StripReader, touch::StripSample};
    use std::time::Instant;
    struct TimedLine {
        high: Rc<Cell<bool>>,
        began: Rc<Cell<Option<Instant>>>,
    }
    impl Lines for TimedLine {
        fn high(&mut self, _: u32) -> io::Result<bool> {
            self.began.set(Some(Instant::now()));
            Ok(self.high.get())
        }
    }
    struct StalledStrip(bool);
    impl StripReader for StalledStrip {
        fn read(&mut self, _: u64) -> Result<Option<StripSample>, InputError> {
            if std::mem::replace(&mut self.0, false) {
                Ok(Some(StripSample {
                    stamp: ClockStamp {
                        clock_id: CLOCK.into(),
                        at_us: 0,
                        sequence: 1,
                    },
                    session_id: "stalled-strip".into(),
                    mask: 0,
                }))
            } else {
                Ok(None)
            }
        }
    }
    let root = Root::new();
    let bus = root.bus();
    let wiring: TouchPinsConfig = serde_json::from_str(include_str!(
        "../fixtures/strip-711-working-pins-example.json"
    ))
    .unwrap();
    let config = Config {
        base_selected_mask: Some(
            wiring
                .base
                .as_ref()
                .unwrap()
                .pads
                .iter()
                .fold(0, |mask, &pad| mask | (1 << pad)),
        ),
        ..Config::default()
    };
    let high = Rc::new(Cell::new(false));
    let began = Rc::new(Cell::new(None));
    let mut service =
        touch_runtime::Service::new(&bus, config, wiring, NoGpio, StalledStrip(true), CLOCK)
            .unwrap();
    service
        .attach_privacy(
            &bus,
            Source::new(
                Box::new(LineReader::new(
                    TimedLine {
                        high: high.clone(),
                        began: began.clone(),
                    },
                    CLOCK,
                    "timed-privacy",
                )),
                CLOCK,
            ),
        )
        .unwrap();
    let mut times = Vec::new();
    for index in 0..101 {
        let at = index * 5_000;
        high.set(index % 2 == 1);
        let touch = service.step(|| at);
        let message = bus.latest(ControlsState::TOPIC).unwrap().unwrap();
        let state: ControlsState = message.parse().unwrap();
        state.check().unwrap();
        assert_eq!(state.stamp.at_us, message.header.at_us);
        assert_eq!(state.stamp.at_us, at);
        assert_eq!(state.privacy.unwrap().muted, Some(!high.get()));
        times.push(began.get().unwrap().elapsed().as_micros() as u64);
        if at >= 100_000 {
            assert_eq!(touch.state.sources[0].fault, Some(ControlFault::Stale));
        }
    }
    let first = times.remove(0);
    times.sort_unstable();
    eprintln!(
        "{}",
        serde_json::json!({"boundary":"injected GPIO high() entry through retained Bus read/parse/validation", "synthetic_clock_step_us":5000, "first_us":first, "repeated_count":times.len(), "repeated_p50_us":times[49], "repeated_p95_us":times[94], "repeated_max_us":times[99], "strip":"one initial sample then no acquisitions", "hardware_measured":false})
    );
}
