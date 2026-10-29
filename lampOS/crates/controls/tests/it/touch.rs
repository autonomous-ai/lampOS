use lamp_controls::{input::Level, touch::*, touch_receiver::TouchReceiver};
use lamp_msgs::{ClockStamp, ControlFault, TouchEvent, TouchGesture, TouchIntent, TouchSource};

struct Rig {
    engine: TouchEngine,
    gpio_seq: u64,
    strip_seq: u64,
}
impl Rig {
    fn new(config: TouchConfig) -> Self {
        Self {
            engine: TouchEngine::new(config, "boot", "controls").unwrap(),
            gpio_seq: 0,
            strip_seq: 0,
        }
    }
    fn sample(&mut self, at: u64, primary: bool, reset: bool, head: u16) -> GpioSample {
        self.gpio_seq += 1;
        let config = self.engine.config();
        GpioSample {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: at,
                sequence: self.gpio_seq,
            },
            session_id: "gpio".into(),
            primary: config.primary.as_ref().map(|_| primary),
            reset: config.reset.as_ref().map(|_| reset),
            head_pads: config
                .head_pads
                .iter()
                .enumerate()
                .map(|(i, name)| Level {
                    name: name.clone(),
                    active: head & (1 << i) != 0,
                })
                .collect(),
        }
    }
    fn gpio(&mut self, at: u64, primary: bool, reset: bool, head: u16) -> Vec<TouchEvent> {
        let sample = self.sample(at, primary, reset, head);
        self.engine.ingest_gpio(at, sample).unwrap();
        self.take(at)
    }
    fn take(&mut self, at: u64) -> Vec<TouchEvent> {
        let output = self.engine.tick(at);
        output.state.check().unwrap();
        for event in &output.events {
            event.check().unwrap();
        }
        output.events
    }
    fn strip(&mut self, at: u64, mask: u16) -> Vec<TouchEvent> {
        self.strip_seq += 1;
        self.engine
            .ingest_strip(
                at,
                StripSample {
                    stamp: ClockStamp {
                        clock_id: "boot".into(),
                        at_us: at,
                        sequence: self.strip_seq,
                    },
                    session_id: "strip".into(),
                    mask,
                },
            )
            .unwrap();
        self.take(at)
    }
    /// Head levels every 10 ms from `from` up to, not including, `to`.
    fn run(&mut self, from: u64, to: u64, head: u16) -> Vec<TouchEvent> {
        (from..to)
            .step_by(10_000)
            .flat_map(|at| self.gpio(at, false, false, head))
            .collect()
    }
    fn head_ready(&mut self) {
        for at in (0..=500_000).step_by(50_000) {
            assert!(self.gpio(at, false, false, 0).is_empty());
        }
    }
    fn hold(
        &mut self,
        from: u64,
        through: u64,
        primary: bool,
        reset: bool,
        head: u16,
    ) -> Vec<TouchEvent> {
        (from..=through)
            .step_by(50_000)
            .flat_map(|at| self.gpio(at, primary, reset, head))
            .collect()
    }
}
fn head() -> TouchConfig {
    TouchConfig {
        enabled: true,
        head_pads: vec!["pad37".into(), "pad96".into()],
        ..TouchConfig::default()
    }
}
fn buttons() -> TouchConfig {
    TouchConfig {
        enabled: true,
        primary: Some("primary99".into()),
        reset: Some("reset100".into()),
        ..TouchConfig::default()
    }
}
fn strip() -> TouchConfig {
    TouchConfig {
        enabled: true,
        strip_pads: Some((0..12).collect()),
        ..TouchConfig::default()
    }
}

#[test]
fn configuration_is_bounded_unique_explicit_and_strict() {
    assert!(TouchEngine::new(TouchConfig::default(), "boot", "controls").is_err());
    let mut config = buttons();
    config.reset = config.primary.clone();
    assert!(config.check().is_err());
    let mut config = head();
    config.head_pads = vec!["a".into(); 5];
    assert!(config.check().is_err());
    let mut config = strip();
    config.strip_pads = Some(vec![0; 12]);
    assert!(config.check().is_err());
    let mut config = strip();
    config.strip_pads = Some(vec![0, 1]);
    assert!(config.check().is_err());
    let mut value = serde_json::to_value(head()).unwrap();
    value["privacy"] = serde_json::json!(true);
    assert!(serde_json::from_value::<TouchConfig>(value).is_err());
}

/// #1798: a pad read touched from the first acquisition, never released
/// (a line stuck active, or a pad resting high read as active-high), is
/// left out until it reads released. The other pad arms, and a touch on it
/// is told without the stuck one. Before, the whole head never armed and
/// nothing said so. Once it reads released, it counts again.
#[test]
fn a_pad_touched_from_the_start_is_left_out_and_the_other_arms() {
    let mut rig = Rig::new(head());
    assert!(rig.run(0, 600_000, 0b01).is_empty());
    assert_eq!(rig.engine.head_left_out(), ["pad37"]);
    let touched = rig.run(600_000, 800_000, 0b11);
    let touch = touched
        .iter()
        .find(|event| event.gesture == TouchGesture::Touch)
        .unwrap_or_else(|| panic!("pad96's touch: {touched:?}"));
    assert_eq!(
        (touch.active_mask, &touch.active_names[..]),
        (0b10, &["pad96".to_owned()][..])
    );
    let let_go = rig.run(800_000, 1_000_000, 0b01);
    assert!(
        let_go
            .iter()
            .any(|event| event.gesture == TouchGesture::Tap),
        "{let_go:?}"
    );
    assert_eq!(rig.engine.head_left_out(), ["pad37"]);
    assert!(rig.run(1_000_000, 1_200_000, 0).is_empty());
    assert!(rig.engine.head_left_out().is_empty());
    let again = rig.run(1_200_000, 1_400_000, 0b01);
    assert!(
        again
            .iter()
            .any(|event| event.gesture == TouchGesture::Touch && event.active_names == ["pad37"]),
        "{again:?}"
    );
    // With its only pad left out, the head is not armed: nothing on it can
    // count.
    let mut one = Rig::new(TouchConfig {
        head_pads: vec!["pad97".into()],
        ..head()
    });
    assert!(one.run(0, 600_000, 0b1).is_empty());
    assert_eq!(one.engine.head_left_out(), ["pad97"]);
    assert!(!one.engine.tick(600_000).state.sources[0].armed);
    // A hand that comes after the first acquisition is not left out: the
    // head waits for it to go, as before.
    let mut rig = Rig::new(head());
    assert!(rig.gpio(0, false, false, 0).is_empty());
    assert!(rig.run(10_000, 600_000, 0b01).is_empty());
    assert!(rig.engine.head_left_out().is_empty());
    assert!(rig.run(600_000, 800_000, 0b11).is_empty(), "not armed");
}

fn working_strip() -> TouchConfig {
    TouchConfig {
        strip_pads: Some(vec![0, 1, 2, 3, 5, 6, 7, 9, 10, 11]),
        ..strip()
    }
}

fn electrode_mask(pads: &[u8]) -> u16 {
    pads.iter().fold(0, |mask, pad| mask | (1 << pad))
}

#[test]
fn working_electrodes_accept_bounded_ordered_subsets_without_changing_full_layouts() {
    for pads in [
        vec![0, 5, 11],
        vec![11, 5, 0],
        (0..11).collect(),
        working_strip().strip_pads.unwrap(),
        vec![11, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    ] {
        let config = TouchConfig {
            strip_pads: Some(pads),
            ..strip()
        };
        config.check().unwrap();
        TouchEngine::new(config, "boot", "controls").unwrap();
    }
    for pads in [
        vec![],
        vec![0],
        vec![0, 1],
        (0..13).collect(),
        vec![0, 1, 12],
        vec![0, 1, 1],
        vec![0, 11, 5],
    ] {
        let config = TouchConfig {
            strip_pads: Some(pads),
            ..strip()
        };
        assert!(config.check().is_err());
        assert!(TouchEngine::new(config, "boot", "controls").is_err());
    }
}

#[test]
fn excluded_only_acquisitions_never_become_active_or_lifecycle_requests() {
    let excluded = electrode_mask(&[4, 8]);
    let mut rig = Rig::new(working_strip());
    let mut receiver = TouchReceiver::new("boot").unwrap();
    for at in (0..=2_100_000).step_by(50_000) {
        assert!(rig.strip(at, excluded).is_empty());
        let state = rig.engine.tick(at).state;
        let source = &state.sources[0];
        assert!(source.ready);
        assert_eq!(source.active_mask, Some(0));
        assert_eq!(source.active, Some(false));
        assert_eq!(source.held_us, None);
        assert_eq!(source.contact_id, None);
        assert_eq!(source.position, None);
        assert!(receiver.observe(at, &state).unwrap().is_empty());
    }
    assert!(rig.engine.tick(2_100_000).state.sources[0].armed);
    rig.strip(2_110_000, excluded | 7);
    rig.strip(2_115_000, excluded | 7);
    let events = rig.strip(2_125_000, excluded | 7);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].gesture, TouchGesture::Touch);
    assert_eq!(events[0].began_us, 2_110_000);
    assert_eq!(events[0].active_mask, 7);
    assert_eq!(events[0].when_asleep, Some(TouchIntent::Wake));
    let state = rig.engine.tick(2_125_000).state;
    assert_eq!(receiver.observe(2_125_000, &state).unwrap(), events);
}

#[test]
fn excluded_pad_noise_does_not_cancel_a_working_stationary_hold() {
    let mut rig = Rig::new(working_strip());
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    let mut events = rig.strip(20_000, 7);
    events.extend(rig.strip(25_000, 7 | (1 << 4)));
    events.extend(rig.strip(35_000, 7 | (1 << 8)));
    for (index, at) in (70_000..=2_070_000).step_by(50_000).enumerate() {
        let raw = 7 | if index % 2 == 0 { 1 << 4 } else { 1 << 8 };
        events.extend(rig.strip(at, raw));
        let state = rig.engine.tick(at).state;
        assert_eq!(state.sources[0].active_mask, Some(7));
        assert_eq!(state.sources[0].held_us, Some(at - 20_000));
    }
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].gesture, TouchGesture::Touch);
    assert_eq!(events[1].when_speaking, TouchIntent::Sleep);
    assert_eq!(events[1].observed_us, 2_020_000);
    assert_eq!(events[1].active_mask, 7);
    assert_eq!(events[0].contact_id, events[1].contact_id);
}

#[test]
fn excluded_electrodes_cannot_complete_the_three_working_pad_chord() {
    for working in [1, 3] {
        let mut rig = Rig::new(working_strip());
        let excluded = electrode_mask(&[4, 8]);
        rig.strip(0, excluded);
        rig.strip(10_000, excluded);
        for at in (20_000..=2_120_000).step_by(50_000) {
            assert!(rig.strip(at, excluded | working).is_empty());
        }
        let state = rig.engine.tick(2_120_000).state;
        assert_eq!(state.sources[0].active_mask, Some(working));
        assert_eq!(state.sources[0].active, Some(true));
        assert!(state.sources[0].position.is_some());
        assert!(state.latest.is_empty());
    }
}

#[test]
fn out_of_range_raw_bits_are_faults_even_when_no_working_electrode_is_active() {
    let mut rig = Rig::new(working_strip());
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    let raw = StripSample {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: 20_000,
            sequence: 3,
        },
        session_id: "strip".into(),
        mask: (1 << 12) | (1 << 4),
    };
    assert_eq!(
        rig.engine.ingest_strip(20_000, raw).unwrap_err().fault,
        ControlFault::Invalid
    );
    let output = rig.engine.tick(20_000);
    output.state.check().unwrap();
    assert!(output.events.is_empty());
    assert_eq!(output.state.sources[0].active, None);
    assert_eq!(output.state.sources[0].active_mask, None);
    assert_eq!(output.state.sources[0].fault, Some(ControlFault::Invalid));
}

#[test]
fn excluded_only_bits_release_a_working_contact_before_its_sleep_deadline() {
    let excluded = electrode_mask(&[4, 8]);
    let mut rig = Rig::new(working_strip());
    rig.strip(0, excluded);
    rig.strip(10_000, excluded);
    rig.strip(20_000, excluded | 7);
    rig.strip(25_000, excluded | 7);
    let first = rig.strip(35_000, excluded | 7).pop().unwrap();
    for at in (70_000..=1_970_000).step_by(50_000) {
        assert!(rig.strip(at, excluded | 7).is_empty());
    }
    for at in [2_019_999, 2_020_000, 2_049_999, 2_050_000] {
        assert!(rig.strip(at, excluded).is_empty());
        let state = rig.engine.tick(at).state;
        assert_eq!(state.sources[0].active, Some(false));
        assert_eq!(state.sources[0].active_mask, Some(0));
        assert_eq!(state.sources[0].held_us, None);
        assert_eq!(state.sources[0].position, None);
    }
    assert_eq!(rig.engine.tick(2_050_000).state.sources[0].contact_id, None);
    rig.strip(2_060_000, excluded | 7);
    rig.strip(2_065_000, excluded | 7);
    let next = rig.strip(2_075_000, excluded | 7).pop().unwrap();
    assert_eq!(next.gesture, TouchGesture::Touch);
    assert_ne!(next.contact_id, first.contact_id);
    assert_eq!(next.began_us, 2_060_000);
}

#[test]
fn missing_working_electrodes_are_not_artificial_gaps_during_a_slide() {
    for reversed in [false, true] {
        let mut config = working_strip();
        if reversed {
            config.strip_pads.as_mut().unwrap().reverse();
        }
        let mut rig = Rig::new(config);
        rig.strip(0, 0);
        rig.strip(10_000, 0);
        let initial = electrode_mask(&[2, 3, 5]);
        rig.strip(20_000, initial);
        rig.strip(25_000, initial);
        let first = rig.strip(35_000, initial).pop().unwrap();
        let expected = if reversed { 23.0 / 33.0 } else { 10.0 / 33.0 };
        assert!((first.position.unwrap() - expected).abs() < 1e-6);
        let mut events = vec![first.clone()];
        let mut positions = vec![first.position.unwrap()];
        for (index, pads) in [[3, 5, 6], [5, 6, 7], [6, 7, 9], [7, 9, 10], [9, 10, 11]]
            .into_iter()
            .enumerate()
        {
            let at = 45_000 + index as u64 * 15_000;
            let mask = electrode_mask(&pads);
            events.extend(rig.strip(at, mask));
            events.extend(rig.strip(at + 5_000, mask));
            let state = rig.engine.tick(at + 5_000).state;
            assert_eq!(state.sources[0].active_mask, Some(mask));
            assert_eq!(state.sources[0].contact_id, Some(first.contact_id));
            positions.push(state.sources[0].position.unwrap());
        }
        assert!(
            positions
                .windows(2)
                .all(|p| if reversed { p[1] < p[0] } else { p[1] > p[0] })
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e.gesture == TouchGesture::Touch)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e.gesture == TouchGesture::Slide)
                .count(),
            5
        );
        for event in events.iter().skip(1) {
            assert_eq!(event.contact_id, first.contact_id);
            assert_eq!(event.toward_pad, Some(11));
            assert_eq!(event.active_mask & electrode_mask(&[4, 8]), 0);
        }
    }
}

#[test]
fn working_subset_faults_remain_unknown_until_a_verified_release_baseline() {
    let mut rig = Rig::new(working_strip());
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    rig.strip(20_000, 7);
    rig.strip(25_000, 7);
    rig.strip(35_000, 7);
    rig.engine.strip_fault(40_000, ControlFault::Missing);
    let fault = rig.engine.tick(40_000);
    assert!(fault.events.is_empty());
    assert!(fault.state.latest.is_empty());
    assert_eq!(fault.state.sources[0].active, None);
    assert_eq!(fault.state.sources[0].active_mask, None);
    assert_eq!(fault.state.sources[0].held_us, None);
    for at in (45_000..=2_095_000).step_by(50_000) {
        assert!(rig.strip(at, 7).is_empty());
        let state = rig.engine.tick(at).state;
        assert!(!state.sources[0].armed);
        assert_eq!(state.sources[0].active, Some(true));
        assert_eq!(state.sources[0].held_us, None);
    }
    let excluded = electrode_mask(&[4, 8]);
    rig.strip(2_100_000, excluded);
    rig.strip(2_110_000, excluded);
    rig.strip(2_120_000, 7 | excluded);
    rig.strip(2_125_000, 7 | excluded);
    let event = rig.strip(2_135_000, 7 | excluded).pop().unwrap();
    assert_eq!(event.gesture, TouchGesture::Touch);
    assert_eq!(event.began_us, 2_120_000);
}
#[test]
fn default_disabled_reports_shadow_without_eligible_events() {
    let mut config = buttons();
    config.enabled = false;
    let mut rig = Rig::new(config);
    rig.gpio(0, false, false, 0);
    rig.gpio(10_000, true, false, 0);
    let events = rig.gpio(20_000, false, false, 0);
    assert_eq!(events.len(), 1);
    assert!(!events[0].enabled);
    assert!(!events[0].fresh_at("boot", 20_000));
    assert!(!rig.engine.tick(20_000).state.enabled);
}
#[test]
fn primary_short_release_is_immediate_stop_with_no_double_tap_wait() {
    let mut rig = Rig::new(buttons());
    rig.gpio(0, false, false, 0);
    assert!(rig.gpio(10_000, true, false, 0).is_empty());
    let e = rig.gpio(20_000, false, false, 0).pop().unwrap();
    assert_eq!(e.source, TouchSource::Primary);
    assert_eq!(e.gesture, TouchGesture::Tap);
    assert_eq!(
        (e.when_speaking, e.when_quiet),
        (TouchIntent::Stop, TouchIntent::Stop)
    );
    assert_eq!(e.stamp.at_us, 20_000);
    assert_eq!(e.released_us, Some(20_000));
}
#[test]
fn primary_hold_never_commits_before_release_and_never_factory_resets() {
    for duration in [2_000_000, 5_000_000] {
        let mut rig = Rig::new(buttons());
        rig.gpio(0, false, false, 0);
        assert!(
            rig.hold(50_000, duration + 50_000, true, false, 0)
                .is_empty()
        );
        let at = duration + 50_001;
        let events = rig.gpio(at, false, false, 0);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].when_quiet, TouchIntent::SoftwareMute);
    }
}
#[test]
fn reset_short_release_does_nothing_and_exact_five_seconds_is_reset_only() {
    let mut rig = Rig::new(buttons());
    // Released a full second first: the reset arms (RESET_ARM_US).
    rig.hold(0, 1_000_000, false, false, 0);
    rig.hold(1_050_000, 6_000_000, false, true, 0);
    assert!(rig.gpio(6_049_999, false, false, 0).is_empty());
    rig.hold(6_099_999, 6_299_999, false, false, 0);
    assert!(rig.hold(6_349_999, 11_299_999, false, true, 0).is_empty());
    let e = rig.gpio(11_349_999, false, false, 0).pop().unwrap();
    assert_eq!(e.source, TouchSource::Reset);
    assert_eq!(e.when_speaking, TouchIntent::FactoryReset);
    assert_eq!(e.observed_us - e.began_us, RESET_HOLD_US);
}
#[test]
fn startup_held_buttons_and_orphan_release_cannot_act() {
    let mut rig = Rig::new(buttons());
    assert!(rig.hold(0, 5_000_000, true, true, 0).is_empty());
    assert!(rig.gpio(5_000_001, false, false, 0).is_empty());
    rig.gpio(5_020_000, true, false, 0);
    assert_eq!(rig.gpio(5_030_000, false, false, 0).len(), 1);
}
#[test]
fn same_polarity_button_debounce_cancels_bounce_hold_evidence() {
    let mut rig = Rig::new(buttons());
    rig.gpio(0, false, false, 0);
    rig.gpio(10_000, true, false, 0);
    assert_eq!(rig.gpio(20_000, false, false, 0).len(), 1);
    rig.gpio(30_000, true, false, 0);
    assert!(rig.hold(80_000, 2_080_000, true, false, 0).is_empty());
    assert!(rig.gpio(2_100_000, false, false, 0).is_empty());
}
#[test]
fn head_needs_startup_settle_and_a_released_baseline() {
    let mut rig = Rig::new(head());
    assert!(rig.hold(0, 400_000, false, false, 1).is_empty());
    assert!(!rig.engine.tick(400_000).state.sources[0].armed, "settling");
    assert!(rig.hold(450_000, 1_000_000, false, false, 1).is_empty());
    // pad37, touched from the start, is left out; pad96 arms (#1798).
    assert!(rig.engine.tick(1_000_000).state.sources[0].armed);
    assert_eq!(rig.engine.head_left_out(), ["pad37"]);
    rig.gpio(1_010_000, false, false, 0);
    rig.gpio(1_020_000, false, false, 1);
    let e = rig.gpio(1_040_000, false, false, 1).pop().unwrap();
    assert_eq!(e.when_speaking, TouchIntent::Stop);
    assert_eq!(e.when_quiet, TouchIntent::Notice);
}
#[test]
fn head_qualification_uses_acquisition_time_not_receipt_or_ticks() {
    let mut rig = Rig::new(head());
    rig.head_ready();
    rig.gpio(510_000, false, false, 1);
    assert!(rig.take(529_999).is_empty());
    let sample = rig.sample(529_999, false, false, 1);
    rig.engine.ingest_gpio(550_000, sample).unwrap();
    assert!(rig.take(550_000).is_empty());
    let sample = rig.sample(530_000, false, false, 1);
    rig.engine.ingest_gpio(550_001, sample).unwrap();
    let e = rig.take(550_001).pop().unwrap();
    assert_eq!(e.began_us, 510_000);
    assert_eq!(e.observed_us, 530_000);
    assert_eq!(e.stamp.at_us, 550_001);
}
#[test]
fn configured_head_order_controls_mask_and_active_names_not_input_vector_order() {
    let mut rig = Rig::new(head());
    rig.head_ready();
    let mut sample = rig.sample(510_000, false, false, 2);
    sample.head_pads.reverse();
    rig.engine.ingest_gpio(510_000, sample).unwrap();
    let mut sample = rig.sample(530_000, false, false, 2);
    sample.head_pads.reverse();
    rig.engine.ingest_gpio(530_000, sample).unwrap();
    let e = rig.take(530_000).pop().unwrap();
    assert_eq!(e.active_mask, 2);
    assert_eq!(e.active_names, ["pad96"]);
}
#[test]
fn stationary_head_stroke_occurs_once_at_one_second() {
    let mut rig = Rig::new(head());
    rig.head_ready();
    let events = rig.hold(550_000, 2_550_000, false, false, 3);
    assert_eq!(
        events.iter().map(|e| e.gesture).collect::<Vec<_>>(),
        [TouchGesture::Touch, TouchGesture::Stroke]
    );
    assert_eq!(events[1].observed_us - events[1].began_us, HEAD_STROKE_US);
    assert_eq!(events[1].active_names, ["pad37", "pad96"]);
    assert!(
        rig.engine
            .tick(2_550_000)
            .state
            .latest
            .iter()
            .all(|e| e.gesture != TouchGesture::Stroke)
    );
}
#[test]
fn fast_mode_pulses_under_a_resting_hand_are_one_hand() {
    // #711: V1's TTP223 pads can pulse under a hand that stays; the owner's
    // hold needs that hand to last, so gaps under HEAD_GAP_US join it.
    let mut rig = Rig::new(head());
    rig.head_ready();
    let mut events = Vec::new();
    for start in (550_000..=1_950_000).step_by(100_000) {
        events.extend(rig.gpio(start, false, false, 1));
        events.extend(rig.gpio(start + 20_000, false, false, 1));
        events.extend(rig.gpio(start + 50_000, false, false, 0));
        let held = rig.engine.tick(start + 50_000).state.sources[0].held_us;
        assert_eq!(
            held,
            Some(start + 50_000 - 550_000),
            "counts through the gap"
        );
    }
    assert_eq!(
        events.iter().map(|e| e.gesture).collect::<Vec<_>>(),
        [TouchGesture::Touch, TouchGesture::Stroke]
    );
    assert!(events.iter().all(|e| e.contact_id == events[0].contact_id));
    // A hand gone for the gap ends it, with no tap: it was not short.
    assert!(rig.run(2_010_000, 2_200_000, 0).is_empty());
    assert_eq!(rig.engine.tick(2_190_000).state.sources[0].contact_id, None);
}
#[test]
fn authored_head_qualification_target_covers_first_and_repeated_sample_phases() {
    for cadence in [2_000, 5_000, 10_000, 20_000, 50_000] {
        let mut rig = Rig::new(head());
        rig.head_ready();
        for start in [550_000, 800_000] {
            let mut events = Vec::new();
            for at in (start..=start + 50_000).step_by(cadence as usize) {
                events.extend(rig.gpio(at, false, false, 1));
            }
            assert_eq!(events.len(), 1);
            let delay = events[0].observed_us - start;
            assert!((HEAD_QUALIFY_US..=HEAD_QUALIFY_US + cadence).contains(&delay));
            // Gone for longer than the gap before the next touch.
            rig.run(start + 60_000, start + 250_000, 0);
        }
    }
}
#[test]
fn acquisition_gap_disarms_hold_and_later_released_sample_recovers() {
    let mut rig = Rig::new(buttons());
    rig.gpio(0, false, false, 0);
    rig.gpio(10_000, true, true, 0);
    let sample = rig.sample(110_000, true, true, 0);
    assert_eq!(
        rig.engine.ingest_gpio(110_000, sample).unwrap_err().fault,
        ControlFault::Stale
    );
    assert!(rig.hold(160_000, 5_160_000, true, true, 0).is_empty());
    assert!(rig.gpio(5_170_000, false, false, 0).is_empty());
    rig.gpio(5_180_000, true, false, 0);
    assert_eq!(rig.gpio(5_190_000, false, false, 0).len(), 1);
}
#[test]
fn lost_acquisition_cannot_turn_hidden_release_repress_into_mute() {
    let mut rig = Rig::new(buttons());
    rig.gpio(0, false, false, 0);
    rig.gpio(10_000, true, false, 0);
    rig.gpio_seq += 1;
    let sample = rig.sample(20_000, true, false, 0);
    assert_eq!(
        rig.engine.ingest_gpio(20_000, sample).unwrap_err().fault,
        ControlFault::Overflow
    );
    assert!(rig.hold(70_000, 2_070_000, true, false, 0).is_empty());
    assert!(rig.gpio(2_080_000, false, false, 0).is_empty());
}
#[test]
fn source_ttl_withdraws_feedback_at_exact_boundary_without_synthetic_release() {
    let mut rig = Rig::new(buttons());
    rig.gpio(0, false, false, 0);
    rig.gpio(10_000, true, false, 0);
    let before = rig.engine.tick(109_999);
    assert_eq!(before.state.sources[0].held_us, Some(0));
    let expired = rig.engine.tick(110_000);
    assert!(expired.events.is_empty());
    for source in &expired.state.sources {
        assert_eq!(source.fault, Some(ControlFault::Stale));
        assert_eq!(source.active, None);
        assert_eq!(source.held_us, None);
    }
    expired.state.check().unwrap();
}
#[test]
fn malformed_future_duplicate_and_wrong_clock_inputs_clear_pending() {
    for fault in [
        ControlFault::Future,
        ControlFault::OutOfOrder,
        ControlFault::WrongClock,
        ControlFault::Missing,
        ControlFault::Invalid,
    ] {
        let mut rig = Rig::new(buttons());
        rig.gpio(0, false, false, 0);
        rig.gpio(10_000, true, false, 0);
        let mut sample = rig.sample(20_000, false, false, 0);
        match fault {
            ControlFault::Future => sample.stamp.at_us = 20_001,
            ControlFault::OutOfOrder => sample.stamp.sequence = 2,
            ControlFault::WrongClock => sample.stamp.clock_id = "other".into(),
            ControlFault::Missing => sample.reset = None,
            ControlFault::Invalid => sample.stamp.sequence = 0,
            _ => unreachable!(),
        }
        assert_eq!(
            rig.engine.ingest_gpio(20_000, sample).unwrap_err().fault,
            fault
        );
        assert!(rig.take(20_000).is_empty());
    }
}
#[test]
fn reader_restart_consumes_first_sample_then_requires_release_and_rejects_old_reader() {
    let mut rig = Rig::new(buttons());
    rig.gpio(0, false, false, 0);
    rig.gpio(10_000, true, false, 0);
    let mut sample = rig.sample(20_000, false, false, 0);
    sample.session_id = "gpio2".into();
    sample.stamp.sequence = 1;
    assert_eq!(
        rig.engine
            .ingest_gpio(20_000, sample.clone())
            .unwrap_err()
            .fault,
        ControlFault::ReaderChanged
    );
    sample.stamp.at_us = 30_000;
    sample.stamp.sequence = 2;
    rig.engine.ingest_gpio(30_000, sample).unwrap();
    assert!(rig.take(30_000).is_empty());
    let sample = rig.sample(40_000, true, false, 0);
    assert_eq!(
        rig.engine.ingest_gpio(40_000, sample).unwrap_err().fault,
        ControlFault::ReaderChanged
    );
}
#[test]
fn backwards_clock_is_terminal_and_near_max_timestamps_do_not_wrap() {
    let mut rig = Rig::new(buttons());
    rig.gpio(u64::MAX - 10_000, false, false, 0);
    rig.gpio(u64::MAX - 1, true, false, 0);
    assert_eq!(rig.gpio(u64::MAX, false, false, 0).len(), 1);
    let state = rig.engine.tick(0).state;
    state.check().unwrap();
    assert!(
        state
            .sources
            .iter()
            .all(|s| s.fault == Some(ControlFault::ClockReset))
    );
    let sample = rig.sample(u64::MAX, false, false, 0);
    assert_eq!(
        rig.engine.ingest_gpio(u64::MAX, sample).unwrap_err().fault,
        ControlFault::ClockReset
    );
}
#[test]
fn gpio_and_strip_faults_do_not_withdraw_the_other_group() {
    let mut config = buttons();
    config.strip_pads = strip().strip_pads;
    let mut rig = Rig::new(config);
    rig.gpio(0, false, false, 0);
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    rig.engine.strip_fault(20_000, ControlFault::Missing);
    rig.gpio(20_000, true, false, 0);
    let events = rig.gpio(30_000, false, false, 0);
    assert_eq!(events.len(), 1);
    rig.strip(30_000, 0);
    rig.strip(40_000, 0);
    rig.engine.gpio_fault(40_000, ControlFault::Missing);
    let state = rig.engine.tick(40_000).state;
    assert!(!state.sources[0].ready);
    assert!(state.sources[2].ready);
    assert!(state.sources[2].armed);
}
#[test]
fn strip_stationary_hold_requests_sleep_once_and_raw_release_is_not_a_tap() {
    let mut rig = Rig::new(strip());
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    rig.strip(20_000, 7);
    let mut events = rig.strip(25_000, 7);
    events.extend(rig.strip(35_000, 7));
    for at in (85_000..=2_035_000).step_by(50_000) {
        events.extend(rig.strip(at, 7));
    }
    assert_eq!(
        events.iter().map(|e| e.when_quiet).collect::<Vec<_>>(),
        [TouchIntent::Attend, TouchIntent::Sleep]
    );
    assert_eq!(events[1].began_us, 20_000);
    assert!(rig.strip(2_040_000, 0).is_empty());
    assert!(rig.strip(2_070_000, 0).is_empty());
}
#[test]
fn strip_brief_qualified_release_and_reversed_mapping_are_preserved() {
    let mut config = strip();
    config.strip_pads.as_mut().unwrap().reverse();
    let mut rig = Rig::new(config);
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    rig.strip(20_000, 7);
    rig.strip(25_000, 7);
    let e = rig.strip(35_000, 0).pop().unwrap();
    assert_eq!(e.released_us, Some(35_000));
    assert_eq!(e.active_mask, 0);
    assert_eq!(e.position, None);
    e.check().unwrap();
}
#[test]
fn strip_restart_and_expiry_cannot_reuse_old_hold() {
    let mut rig = Rig::new(strip());
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    rig.strip(20_000, 7);
    rig.strip(25_000, 7);
    rig.strip(35_000, 7);
    let sample = StripSample {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: 40_000,
            sequence: 1,
        },
        session_id: "strip2".into(),
        mask: 7,
    };
    assert_eq!(
        rig.engine.ingest_strip(40_000, sample).unwrap_err().fault,
        ControlFault::ReaderChanged
    );
    assert!(rig.engine.tick(40_000).state.latest.is_empty());
    for i in 2..50 {
        let at = 40_000 + (i - 1) * 50_000;
        rig.engine
            .ingest_strip(
                at,
                StripSample {
                    stamp: ClockStamp {
                        clock_id: "boot".into(),
                        at_us: at,
                        sequence: i,
                    },
                    session_id: "strip2".into(),
                    mask: 7,
                },
            )
            .unwrap();
        assert!(rig.take(at).is_empty());
    }
}
#[test]
fn output_queue_is_bounded_and_overflow_withdraws_undelivered_group() {
    let mut rig = Rig::new(buttons());
    let sample = rig.sample(0, false, false, 0);
    rig.engine.ingest_gpio(0, sample).unwrap();
    let mut overflow = None;
    for cycle in 0..65 {
        let start = cycle * 250_000 + 10_000;
        if cycle > 0 {
            for at in [
                start - 200_000,
                start - 150_000,
                start - 100_000,
                start - 50_000,
            ] {
                let s = rig.sample(at, false, false, 0);
                rig.engine.ingest_gpio(at, s).unwrap();
            }
        }
        let s = rig.sample(start, true, false, 0);
        rig.engine.ingest_gpio(start, s).unwrap();
        let s = rig.sample(start + 10_000, false, false, 0);
        if let Err(e) = rig.engine.ingest_gpio(start + 10_000, s) {
            overflow = Some(e.fault);
            break;
        }
    }
    assert_eq!(overflow, Some(ControlFault::Overflow));
    let output = rig.engine.tick(16_020_000);
    assert!(output.events.is_empty());
    assert!(output.state.latest.is_empty());
}
#[test]
fn reader_session_retirement_is_bounded_and_never_reopens_old_identity() {
    let mut rig = Rig::new(buttons());
    rig.gpio(0, false, false, 0);
    for i in 1..=32 {
        let mut sample = rig.sample(i * 1_000, false, false, 0);
        sample.session_id = format!("reader{i}");
        sample.stamp.sequence = 1;
        assert_eq!(
            rig.engine.ingest_gpio(i * 1_000, sample).unwrap_err().fault,
            ControlFault::ReaderChanged
        );
    }
    let mut sample = rig.sample(33_000, false, false, 0);
    sample.session_id = "reader33".into();
    sample.stamp.sequence = 1;
    assert_eq!(
        rig.engine.ingest_gpio(33_000, sample).unwrap_err().fault,
        ControlFault::Overflow
    );
}
#[test]
fn pending_event_and_feedback_do_not_gain_new_age_on_heartbeat() {
    let mut rig = Rig::new(buttons());
    rig.gpio(0, false, false, 0);
    rig.gpio(10_000, true, false, 0);
    let original = rig.gpio(20_000, false, false, 0).pop().unwrap();
    for at in (70_000..=570_000).step_by(50_000) {
        rig.gpio(at, false, false, 0);
    }
    let state = rig.engine.tick(570_000).state;
    assert_eq!(state.latest, std::slice::from_ref(&original));
    assert!(!state.latest[0].fresh_at("boot", 570_000));
}

#[test]
fn strip_raw_handoff_is_active_even_while_filtered_geometry_is_empty() {
    let mut rig = Rig::new(strip());
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    rig.strip(20_000, 1);
    rig.strip(25_000, 1);
    rig.strip(30_000, 1);
    rig.strip(40_000, 2);
    let source = &rig.engine.tick(40_000).state.sources[0];
    assert_eq!(source.active_mask, Some(2));
    assert_eq!(source.active, Some(true));
    assert!(source.contact_id.is_some());
}

#[test]
fn reversed_strip_order_names_actual_destination_electrode() {
    let mut config = strip();
    config.strip_pads.as_mut().unwrap().reverse();
    let mut rig = Rig::new(config);
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    rig.strip(20_000, 7);
    rig.strip(25_000, 7);
    rig.strip(35_000, 7);
    let mut events = rig.strip(40_000, 14);
    events.extend(rig.strip(45_000, 14));
    let slide = events
        .iter()
        .find(|event| event.gesture == TouchGesture::Slide)
        .unwrap();
    assert_eq!(slide.toward_pad, Some(11));
    assert_eq!(slide.when_asleep, None);
}

#[test]
fn a_head_touch_stops_a_talking_lamp_and_only_a_short_tap_asks_it_to_listen_or_wake() {
    let mut rig = Rig::new(head());
    rig.head_ready();
    let touch = rig.run(510_000, 540_000, 1).pop().unwrap();
    assert_eq!(touch.gesture, TouchGesture::Touch);
    assert_eq!(touch.observed_us, 530_000);
    assert_eq!(touch.intent(true, false), Some(TouchIntent::Stop));
    assert_eq!(touch.intent(false, false), Some(TouchIntent::Notice));
    assert_eq!(touch.intent(false, true), None);
    // Last touched at 0.8 s, gone at 0.81 s: a tap, once the gap shows the
    // hand has gone.
    assert!(rig.run(540_000, 810_000, 1).is_empty());
    assert!(rig.run(810_000, 800_000 + HEAD_GAP_US, 0).is_empty());
    let tap = rig
        .gpio(800_000 + HEAD_GAP_US, false, false, 0)
        .pop()
        .unwrap();
    assert_eq!(tap.gesture, TouchGesture::Tap);
    assert_eq!((tap.began_us, tap.released_us), (510_000, Some(810_000)));
    assert_eq!(tap.contact_id, touch.contact_id);
    assert_eq!(tap.intent(true, false), Some(TouchIntent::Attend));
    assert_eq!(tap.intent(false, false), Some(TouchIntent::Attend));
    assert_eq!(tap.intent(false, true), Some(TouchIntent::Wake));
    // A second tap after the gap is its own contact.
    rig.run(960_000, 1_200_000, 0);
    assert_eq!(rig.run(1_200_000, 1_350_000, 2).len(), 1);
    let second = rig.run(1_350_000, 1_500_000, 0).pop().unwrap();
    assert_eq!(second.gesture, TouchGesture::Tap);
    assert!(second.contact_id > tap.contact_id);
}

/// #3747 (the lead's table): no length between a tap and a stroke asks
/// nothing. A touch let go at 0.51 s or at 0.9 s is a tap, which listens and
/// wakes a sleeping lamp; a lone 110 ms edge, under the shortest tap, never
/// wakes it; and a 10 ms blip never qualified, so its letting go is nothing.
#[test]
fn a_touch_let_go_under_a_second_is_a_tap_and_a_blip_or_lone_edge_never_wakes() {
    let mut rig = Rig::new(head());
    rig.head_ready();
    // Each let go, then at rest until the next, read every 10 ms.
    for (began, held, next) in [
        (550_000, 510_000, 2_000_000),
        (2_000_000, 900_000, 4_000_000),
    ] {
        let mut events = rig.run(began, began + held, 1);
        events.extend(rig.run(began + held, next, 0));
        assert_eq!(
            events.iter().map(|e| e.gesture).collect::<Vec<_>>(),
            [TouchGesture::Touch, TouchGesture::Tap],
            "let go at {held}"
        );
        let (touch, tap) = (&events[0], &events[1]);
        assert_eq!(touch.intent(false, true), None, "its touch alone");
        assert_eq!((tap.began_us, tap.released_us), (began, Some(began + held)));
        assert_eq!(tap.intent(false, false), Some(TouchIntent::Attend));
        assert_eq!(tap.intent(false, true), Some(TouchIntent::Wake));
    }
    // A lone 110 ms edge: a touch that may stop speech, and nothing asleep.
    let mut events = rig.run(4_000_000, 4_110_000, 1);
    events.extend(rig.run(4_110_000, 5_000_000, 0));
    assert_eq!(
        events.iter().map(|e| e.gesture).collect::<Vec<_>>(),
        [TouchGesture::Touch]
    );
    assert!(events.iter().all(|e| e.intent(false, true).is_none()));
    // One 10 ms blip never qualified, so its letting go is nothing either.
    assert!(rig.run(5_000_000, 5_010_000, 1).is_empty());
    assert!(rig.run(5_010_000, 5_400_000, 0).is_empty());
}

/// A hand kept three seconds asks for sleep (#711; kept by the lead on
/// #1711), and its letting go asks nothing. Asleep when it began, the lamp
/// wakes at its stroke, a second in (#3747); its touch alone and its hold
/// never wake it.
#[test]
fn a_hand_kept_three_seconds_asks_for_sleep_and_its_letting_go_asks_nothing() {
    let mut rig = Rig::new(head());
    rig.head_ready();
    let events = rig.run(550_000, 3_560_000, 3);
    assert_eq!(
        events.iter().map(|e| e.gesture).collect::<Vec<_>>(),
        [
            TouchGesture::Touch,
            TouchGesture::Stroke,
            TouchGesture::Hold
        ]
    );
    assert_eq!(events[1].observed_us - events[1].began_us, HEAD_STROKE_US);
    let sleep = &events[2];
    assert_eq!(sleep.observed_us - sleep.began_us, HEAD_SLEEP_US);
    assert_eq!(sleep.intent(false, false), Some(TouchIntent::Sleep));
    assert_eq!(sleep.intent(true, false), Some(TouchIntent::Sleep));
    assert_eq!(sleep.intent(false, true), None);
    assert_eq!(events[0].intent(false, true), None, "the touch alone");
    assert_eq!(events[1].intent(false, false), Some(TouchIntent::Stroke));
    // The stroke keeps its old form, asking nothing asleep (Third's P2 on
    // #3767): its readers wake a sleeping lamp on it themselves.
    assert_eq!(events[1].intent(false, true), None);
    // The countdown between: the hand's time, at every acquisition.
    assert_eq!(
        rig.engine.tick(3_550_000).state.sources[0].held_us,
        Some(3_000_000)
    );
    assert!(rig.run(3_560_000, 4_000_000, 0).is_empty());
}

/// A tap of a sleeping lamp wakes it at its letting go, once: its touch
/// alone asks nothing asleep.
#[test]
fn a_tap_wakes_a_sleeping_lamp_once() {
    let mut rig = Rig::new(head());
    rig.head_ready();
    let mut events = rig.run(550_000, 800_000, 1);
    events.extend(rig.run(800_000, 1_400_000, 0));
    assert_eq!(
        events.iter().map(|e| e.gesture).collect::<Vec<_>>(),
        [TouchGesture::Touch, TouchGesture::Tap]
    );
    assert_eq!(events[0].intent(false, true), None);
    assert_eq!(events[1].intent(false, true), Some(TouchIntent::Wake));
    assert_eq!(events[1].intent(false, false), Some(TouchIntent::Attend));
}

#[test]
fn a_lone_short_edge_can_stop_speech_but_never_asks_to_listen() {
    // #711: lamp-4ace's journal showed a lone 117 ms head edge with nobody
    // known at the lamp. It may stop speech at 20 ms, as any touch does,
    // but under the shortest tap it never asks the lamp to listen or wake.
    let mut rig = Rig::new(head());
    rig.head_ready();
    let events = rig.run(550_000, 660_000, 1);
    assert_eq!(
        events.iter().map(|e| e.gesture).collect::<Vec<_>>(),
        [TouchGesture::Touch]
    );
    assert_eq!(
        events[0].intent(false, true),
        None,
        "never wakes it (#3747)"
    );
    assert!(rig.run(660_000, 1_000_000, 0).is_empty());
}

#[test]
fn the_head_timing_is_a_setting_with_bounds() {
    let mut config = head();
    (config.head_tap_min_ms, config.head_gap_ms) = (Some(50), Some(0));
    let mut rig = Rig::new(config);
    rig.head_ready();
    // A 110 ms tap counts with a 50 ms shortest tap.
    rig.run(550_000, 660_000, 1);
    let tap = rig.run(660_000, 700_000, 0).pop().unwrap();
    assert_eq!(tap.gesture, TouchGesture::Tap);
    // With no gap, a pulse's 30 ms gap ends the hand.
    rig.run(700_000, 800_000, 0);
    assert_eq!(rig.run(800_000, 830_000, 1).len(), 1);
    rig.run(830_000, 860_000, 0);
    assert_eq!(rig.run(860_000, 890_000, 1).len(), 1);
    for (qualify, tap_min, gap) in [
        (Some(10), None, None),
        (Some(201), None, None),
        (Some(100), Some(90), None),
        // The shortest tap keeps its range under half a second, although a
        // tap is now let go under a second (#3747).
        (None, Some(500), None),
        (None, None, Some(501)),
    ] {
        let mut config = head();
        (
            config.head_qualify_ms,
            config.head_tap_min_ms,
            config.head_gap_ms,
        ) = (qualify, tap_min, gap);
        assert!(TouchEngine::new(config, "boot", "controls").is_err());
    }
    assert_eq!(head().head_timing(), HeadTiming::default());
}

#[test]
fn a_head_pushed_by_hand_asks_for_nothing() {
    // #711: the body feels the head being moved; a touch then means nothing.
    let mut rig = Rig::new(head());
    rig.head_ready();
    rig.engine.set_head_pushed(true);
    assert!(rig.run(550_000, 4_000_000, 1).is_empty());
    assert_eq!(rig.engine.tick(3_990_000).state.sources[0].held_us, None);
    assert!(rig.run(4_000_000, 4_400_000, 0).is_empty());
    // Pushed after the touch: the stop stands, and nothing comes after it.
    rig.engine.set_head_pushed(false);
    assert_eq!(rig.run(4_400_000, 4_430_000, 1).len(), 1);
    rig.engine.set_head_pushed(true);
    rig.run(4_430_000, 4_440_000, 1);
    rig.engine.set_head_pushed(false);
    assert!(rig.run(4_440_000, 8_500_000, 1).is_empty());
    assert!(rig.run(8_500_000, 8_900_000, 0).is_empty());
    // A short tap once the push is over is a tap again.
    assert_eq!(rig.run(8_900_000, 9_050_000, 1).len(), 1);
    let tap = rig.run(9_050_000, 9_200_000, 0).pop().unwrap();
    assert_eq!(tap.gesture, TouchGesture::Tap);
}

#[test]
fn invalid_head_snapshot_cannot_omit_or_duplicate_configured_inputs() {
    for missing in [false, true] {
        let mut rig = Rig::new(head());
        rig.head_ready();
        rig.gpio(510_000, false, false, 1);
        let mut sample = rig.sample(530_000, false, false, 1);
        if missing {
            sample.head_pads.pop();
        } else {
            sample.head_pads[1] = sample.head_pads[0].clone();
        }
        assert!(rig.engine.ingest_gpio(530_000, sample).is_err());
        let output = rig.engine.tick(530_000);
        assert!(output.events.is_empty());
        assert_eq!(output.state.sources[0].active, None);
    }
}

#[test]
fn gpio_fault_preserves_independent_strip_request_identity() {
    let mut config = buttons();
    config.strip_pads = strip().strip_pads;
    let mut rig = Rig::new(config);
    rig.gpio(0, false, false, 0);
    rig.strip(0, 0);
    rig.strip(10_000, 0);
    rig.strip(20_000, 7);
    rig.strip(25_000, 7);
    let event = rig.strip(35_000, 7).pop().unwrap();
    rig.engine.gpio_fault(40_000, ControlFault::Missing);
    let output = rig.engine.tick(40_000);
    assert_eq!(output.state.latest, [event]);
    assert!(output.state.sources[2].ready);
}

#[test]
fn physical_button_events_preserve_stop_mute_and_reset_asleep() {
    let mut rig = Rig::new(buttons());
    rig.hold(0, 1_000_000, false, false, 0);
    rig.gpio(1_010_000, true, false, 0);
    let tap = rig.gpio(1_020_000, false, false, 0).pop().unwrap();
    assert_eq!(tap.intent(false, true), Some(TouchIntent::Stop));
    rig.hold(1_070_000, 1_270_000, false, false, 0);
    assert!(rig.hold(1_320_000, 6_320_000, true, true, 0).is_empty());
    let events = rig.gpio(6_320_001, false, false, 0);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].source, TouchSource::Primary);
    assert_eq!(
        events[0].intent(false, true),
        Some(TouchIntent::SoftwareMute)
    );
    assert_eq!(events[1].source, TouchSource::Reset);
    assert_eq!(
        events[1].intent(false, true),
        Some(TouchIntent::FactoryReset)
    );
    assert_eq!(events[1].released_us, Some(6_320_001));
}

/// The reviewer on #1167: a reset line that rests pressed and lifts briefly
/// never arms, so two short lifts 5 s apart are no factory reset. Released
/// a full second running, it arms, and a 5 s hold is one.
#[test]
fn a_reset_line_arms_only_after_a_full_released_second() {
    let mut rig = Rig::new(buttons());
    rig.hold(0, 500_000, false, false, 0);
    assert!(rig.hold(510_000, 5_610_000, false, true, 0).is_empty());
    assert!(rig.hold(5_620_000, 5_800_000, false, false, 0).is_empty());
    assert!(rig.hold(5_810_000, 10_910_000, false, true, 0).is_empty());
    rig.hold(10_920_000, 12_020_000, false, false, 0);
    assert!(rig.hold(12_030_000, 17_030_000, false, true, 0).is_empty());
    let e = rig.gpio(17_040_000, false, false, 0).pop().unwrap();
    assert_eq!(e.source, TouchSource::Reset);
    assert_eq!(e.when_speaking, TouchIntent::FactoryReset);
    assert!(!rig.engine.reset_refused());
}

/// The reviewer on #1176: a reset line read pressed through all of its
/// first second is not a reset button, and never arms in that run, even
/// after a full released second. Rests pressed, lifts 1.1 s (a hand kept on
/// a pad), pressed 5.2 s, a short release: no factory reset. The primary
/// button still works.
#[test]
fn a_reset_line_read_pressed_through_its_first_second_never_arms_in_that_run() {
    let mut rig = Rig::new(buttons());
    assert!(!rig.engine.reset_refused());
    rig.hold(0, 2_000_000, false, true, 0);
    assert!(rig.engine.reset_refused());
    rig.hold(2_010_000, 3_110_000, false, false, 0);
    assert!(rig.hold(3_120_000, 8_320_000, false, true, 0).is_empty());
    assert!(rig.hold(8_330_000, 8_500_000, false, false, 0).is_empty());
    rig.hold(8_510_000, 9_610_000, false, false, 0);
    assert!(rig.hold(9_620_000, 14_620_000, false, true, 0).is_empty());
    assert!(rig.gpio(14_630_000, false, false, 0).is_empty());
    let output = rig.engine.tick(14_630_000);
    let reset = output
        .state
        .sources
        .iter()
        .find(|source| source.source == TouchSource::Reset)
        .unwrap();
    assert!(!reset.armed);
    assert!(rig.engine.reset_refused());
    rig.gpio(14_700_000, true, false, 0);
    let tap = rig.gpio(14_710_000, false, false, 0).pop().unwrap();
    assert_eq!(tap.source, TouchSource::Primary);
}

/// The reviewer on #1209: one pressed read as Controls starts (a line still
/// charging through its pull-up, a hand leaving the button) does not turn
/// the reset off. Pressed for one read, released 2 s, then a 5 s hold: one
/// factory reset.
#[test]
fn a_reset_line_released_once_in_its_first_second_still_arms() {
    let mut rig = Rig::new(buttons());
    rig.gpio(0, false, true, 0);
    rig.hold(10_000, 2_010_000, false, false, 0);
    assert!(!rig.engine.reset_refused());
    assert!(rig.hold(2_020_000, 7_020_000, false, true, 0).is_empty());
    let e = rig.gpio(7_030_000, false, false, 0).pop().unwrap();
    assert_eq!(e.source, TouchSource::Reset);
    assert_eq!(e.when_speaking, TouchIntent::FactoryReset);
}

/// The primary button held `held` µs and released, on an engine with the
/// shutdown switch `on`: what it asks, and whether its state said the hold
/// past 5 s while it lasted (the ring's warning reads that `held_us`).
fn primary_hold(on: bool, held: u64) -> (Vec<TouchEvent>, bool) {
    let mut rig = Rig::new(buttons());
    rig.engine.set_shutdown(on);
    rig.gpio(0, false, false, 0);
    let mut warned = false;
    for at in (50_000..=50_000 + held).step_by(50_000) {
        let sample = rig.sample(at, true, false, 0);
        rig.engine.ingest_gpio(at, sample).unwrap();
        let output = rig.engine.tick(at);
        assert!(output.events.is_empty(), "nothing before the release");
        let primary = output
            .state
            .sources
            .iter()
            .find(|source| source.source == TouchSource::Primary)
            .unwrap();
        warned |= primary.held_us.is_some_and(lamp_msgs::shutdown_hold);
    }
    (rig.gpio(50_000 + held + 1, false, false, 0), warned)
}

/// #3697: 1 s is Stop, 3 s the mute, either way; with the switch on 6 s is
/// one shutdown and 20 s nothing (cancelled); off, both are the mute, as
/// before.
#[test]
fn a_primary_hold_turns_off_in_its_window_only_with_the_switch_on() {
    for (on, held, wanted, warned) in [
        (false, 1_000_000, Some(TouchIntent::Stop), false),
        (false, 3_000_000, Some(TouchIntent::SoftwareMute), false),
        (false, 6_000_000, Some(TouchIntent::SoftwareMute), true),
        (false, 20_000_000, Some(TouchIntent::SoftwareMute), true),
        (true, 1_000_000, Some(TouchIntent::Stop), false),
        (true, 3_000_000, Some(TouchIntent::SoftwareMute), false),
        (true, 6_000_000, Some(TouchIntent::Shutdown), true),
        (true, 20_000_000, None, true),
    ] {
        let (events, saw_window) = primary_hold(on, held);
        assert_eq!(
            events
                .iter()
                .map(|event| event.when_quiet)
                .collect::<Vec<_>>(),
            wanted.into_iter().collect::<Vec<_>>(),
            "on {on}, held {held}"
        );
        // The state says the hold's time either way; the ring warns only
        // with its own switch on.
        assert_eq!(saw_window, warned, "on {on}, held {held}");
        let requests = events
            .iter()
            .filter_map(shutdown_request)
            .collect::<Vec<_>>();
        if wanted == Some(TouchIntent::Shutdown) {
            assert_eq!(requests.len(), 1, "exactly one request");
            assert_eq!(requests[0].source, lamp_msgs::ShutdownSource::Button);
            assert_eq!(requests[0].stamp, events[0].stamp);
            assert!(
                requests[0]
                    .accept("controls", "boot", events[0].stamp.at_us)
                    .is_ok()
            );
        } else {
            assert!(requests.is_empty(), "on {on}, held {held}");
        }
    }
}

/// A Controls in shadow asks nothing of setup, even in the window.
#[test]
fn a_shadow_controls_shutdown_hold_is_no_request() {
    let (mut events, _) = primary_hold(true, 6_000_000);
    let mut event = events.pop().unwrap();
    event.enabled = false;
    assert!(shutdown_request(&event).is_none());
}
