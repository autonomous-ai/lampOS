use lamp_controls::{
    engine::{
        Config, Engine, HEAD_DECISION_US, HEAD_PAT_LONGEST_US, HEAD_SESSION_US,
        MAX_PENDING_GESTURES,
    },
    input::{INPUT_FRESH_US, Level, Observation, PinReader, ScriptedPins},
};
use lamp_msgs::{
    ClockStamp, ControlFault, ControlGesture, ControlIntent, ControlSource, Gesture, HoldTier,
};

fn config() -> Config {
    Config {
        enabled: true,
        buttons: vec!["button".into()],
        head_pads: vec![],
        base_selected_mask: None,
    }
}

fn observation(at_us: u64, sequence: u64, active: bool) -> Observation {
    Observation {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us,
            sequence,
        },
        session_id: "reader".into(),
        buttons: vec![Level {
            name: "button".into(),
            active,
        }],
        head_pads: vec![],
        base_mask: None,
    }
}

struct Rig {
    engine: Engine,
    now: u64,
    sequence: u64,
    buttons: Vec<Level>,
    head: Vec<Level>,
    base: Option<u16>,
    events: Vec<ControlGesture>,
}

impl Rig {
    fn new(config: Config) -> Self {
        let buttons = config
            .buttons
            .iter()
            .map(|name| Level {
                name: name.clone(),
                active: false,
            })
            .collect();
        let head = config
            .head_pads
            .iter()
            .map(|name| Level {
                name: name.clone(),
                active: false,
            })
            .collect();
        let base = config.base_selected_mask.map(|_| 0);
        let mut rig = Self {
            engine: Engine::new(config, "boot", "controls").unwrap(),
            now: 0,
            sequence: 0,
            buttons,
            head,
            base,
            events: vec![],
        };
        rig.sample(0);
        rig
    }

    fn sample(&mut self, at_us: u64) {
        self.sequence += 1;
        self.engine
            .ingest(
                at_us,
                Observation {
                    stamp: ClockStamp {
                        clock_id: "boot".into(),
                        at_us,
                        sequence: self.sequence,
                    },
                    session_id: "reader".into(),
                    buttons: self.buttons.clone(),
                    head_pads: self.head.clone(),
                    base_mask: self.base,
                },
            )
            .unwrap();
        self.now = at_us;
        let output = self.engine.tick(at_us);
        output.state.check().unwrap();
        for event in &output.gestures {
            event.check().unwrap();
        }
        self.events.extend(output.gestures);
    }

    fn before(&mut self, at_us: u64) {
        assert!(at_us > self.now);
        while self.now + 50_000 < at_us {
            self.sample(self.now + 50_000);
        }
    }

    fn to(&mut self, at_us: u64) {
        self.before(at_us);
        self.sample(at_us);
    }

    fn button(&mut self, at_us: u64, index: usize, active: bool) {
        self.before(at_us);
        self.buttons[index].active = active;
        self.sample(at_us);
    }

    fn head(&mut self, at_us: u64, index: usize, active: bool) {
        self.before(at_us);
        self.head[index].active = active;
        self.sample(at_us);
    }

    fn base(&mut self, at_us: u64, mask: u16) {
        self.before(at_us);
        self.base = Some(mask);
        self.sample(at_us);
    }

    fn kinds(&self) -> Vec<Gesture> {
        self.events.iter().map(|event| event.gesture).collect()
    }
}

#[test]
fn one_millisecond_tap_stops_at_release_without_double_click_wait() {
    let mut rig = Rig::new(config());
    rig.button(10_000, 0, true);
    assert!(rig.events.is_empty());
    rig.button(11_000, 0, false);
    assert_eq!(rig.kinds(), vec![Gesture::Tap]);
    assert_eq!(rig.events[0].stamp.at_us, 11_000);
    assert_eq!(rig.events[0].released_us, 11_000);
    assert_eq!(rig.events[0].duration_us, 1_000);
    assert_eq!(rig.events[0].intent, ControlIntent::Stop);
    rig.to(1_000_000);
    assert_eq!(rig.events.len(), 1);
}

#[test]
fn opposite_polarity_is_independent_but_same_polarity_bounce_does_not_retrigger() {
    let mut rig = Rig::new(config());
    for (at, level) in [
        (10_000, true),
        (20_000, false),
        (22_000, true),
        (24_000, false),
        (30_000, true),
        (40_000, false),
    ] {
        rig.button(at, 0, level);
    }
    assert_eq!(rig.kinds(), vec![Gesture::Tap]);
    rig.button(210_000, 0, true);
    rig.button(220_000, 0, false);
    assert_eq!(rig.kinds(), vec![Gesture::Tap, Gesture::DoubleTap]);
}

#[test]
fn a_suppressed_release_still_clears_held_feedback() {
    let mut rig = Rig::new(config());
    rig.button(10_000, 0, true);
    rig.button(190_000, 0, false);
    rig.button(210_000, 0, true);
    rig.button(220_000, 0, false);
    rig.to(3_000_000);
    let state = rig.engine.tick(rig.now).state;
    assert_eq!(state.buttons[0].held_us, None);
    assert!(!state.buttons[0].pressed);
    assert_eq!(rig.kinds(), vec![Gesture::Tap]);
}

#[test]
fn doubles_and_multiple_taps_have_only_the_stop_intent() {
    let mut rig = Rig::new(config());
    for (down, up) in [
        (10_000, 20_000),
        (210_000, 220_000),
        (410_000, 420_000),
        (610_000, 620_000),
    ] {
        rig.button(down, 0, true);
        rig.button(up, 0, false);
    }
    assert_eq!(
        rig.kinds(),
        vec![
            Gesture::Tap,
            Gesture::DoubleTap,
            Gesture::MultiTap,
            Gesture::MultiTap
        ]
    );
    assert!(
        rig.events
            .iter()
            .all(|event| event.intent == ControlIntent::Stop)
    );
    rig.button(1_200_000, 0, true);
    rig.button(1_210_000, 0, false);
    assert_eq!(rig.events.last().unwrap().gesture, Gesture::Tap);
}

#[test]
fn two_and_five_second_tiers_are_feedback_until_genuine_release() {
    let mut rig = Rig::new(config());
    rig.button(10_000, 0, true);
    for (at, tier) in [
        (2_009_999, HoldTier::None),
        (2_010_000, HoldTier::Mute),
        (5_010_000, HoldTier::ExtendedMute),
        (10_010_000, HoldTier::ExtendedMute),
    ] {
        rig.to(at);
        assert_eq!(rig.engine.tick(at).state.buttons[0].tier, tier);
        assert!(rig.events.is_empty());
    }
    rig.button(10_011_000, 0, false);
    assert_eq!(rig.kinds(), vec![Gesture::LongPress]);
    assert_eq!(rig.events[0].intent, ControlIntent::SoftwareMute);
    assert_eq!(rig.events[0].duration_us, 10_001_000);
}

#[test]
fn release_edge_not_receipt_time_sets_the_two_second_boundary() {
    for (duration, expected) in [
        (1_999_999, ControlIntent::Stop),
        (2_000_000, ControlIntent::SoftwareMute),
    ] {
        let mut rig = Rig::new(config());
        rig.button(10_000, 0, true);
        rig.to(10_000 + duration - 1);
        let at = 10_000 + duration;
        let mut released = observation(at, rig.sequence + 1, false);
        released.session_id = "reader".into();
        rig.engine.ingest(at + 90_000, released).unwrap();
        let output = rig.engine.tick(at + 90_000);
        assert_eq!(output.gestures.len(), 1);
        assert_eq!(output.gestures[0].intent, expected);
        assert_eq!(output.gestures[0].duration_us, duration);
        assert_eq!(output.gestures[0].stamp.at_us, at + 90_000);
    }
}

#[test]
fn buttons_keep_independent_debounce_and_hold_state() {
    let mut cfg = config();
    cfg.buttons.push("other".into());
    let mut rig = Rig::new(cfg);
    rig.button(10_000, 0, true);
    rig.button(11_000, 1, true);
    rig.button(12_000, 1, false);
    assert_eq!(rig.events[0].name, "other");
    rig.button(2_010_000, 0, false);
    assert_eq!(rig.events[1].name, "button");
    assert_eq!(rig.events[1].intent, ControlIntent::SoftwareMute);
}

#[test]
fn initial_held_button_and_orphan_release_do_not_act() {
    let mut engine = Engine::new(config(), "boot", "controls").unwrap();
    for (at, sequence, active) in [(0, 1, true), (50_000, 2, true), (60_000, 3, false)] {
        engine
            .ingest(at, observation(at, sequence, active))
            .unwrap();
        assert!(engine.tick(at).gestures.is_empty());
    }
    engine.ingest(70_000, observation(70_000, 4, true)).unwrap();
    engine
        .ingest(80_000, observation(80_000, 5, false))
        .unwrap();
    assert_eq!(engine.tick(80_000).gestures[0].intent, ControlIntent::Stop);
}

#[test]
fn ticking_never_fabricates_fresh_holding_or_a_long_release() {
    let mut rig = Rig::new(config());
    rig.button(10_000, 0, true);
    let output = rig.engine.tick(90_000);
    assert_eq!(output.state.buttons[0].held_us, Some(0));
    let stale = rig.engine.tick(110_000);
    assert_eq!(stale.state.fault, Some(ControlFault::Stale));
    assert_eq!(stale.state.buttons[0].held_us, None);
    assert!(stale.gestures.is_empty());
    rig.engine
        .ingest(3_000_000, observation(3_000_000, 3, false))
        .unwrap();
    assert!(rig.engine.tick(3_000_000).gestures.is_empty());
}

#[test]
fn exactly_one_hundred_millisecond_input_gap_disarms_before_processing_release() {
    let mut engine = Engine::new(config(), "boot", "controls").unwrap();
    engine.ingest(0, observation(0, 1, false)).unwrap();
    engine.ingest(10_000, observation(10_000, 2, true)).unwrap();
    engine
        .ingest(110_000, observation(110_000, 3, false))
        .unwrap();
    let output = engine.tick(110_000);
    assert!(output.gestures.is_empty());
    assert!(output.state.ready);
    assert!(output.state.buttons[0].armed);
}

#[test]
fn stale_queued_release_is_rejected_and_cannot_be_retimestamped() {
    let mut engine = Engine::new(config(), "boot", "controls").unwrap();
    engine.ingest(0, observation(0, 1, false)).unwrap();
    engine.ingest(10_000, observation(10_000, 2, true)).unwrap();
    assert_eq!(
        engine
            .ingest(120_000, observation(20_000, 3, false))
            .unwrap_err()
            .fault,
        ControlFault::Stale
    );
    assert!(engine.tick(120_000).gestures.is_empty());
    assert_eq!(
        engine
            .ingest(130_000, observation(130_000, 4, false))
            .unwrap_err()
            .fault,
        ControlFault::Overflow
    );
    engine
        .ingest(140_000, observation(140_000, 5, false))
        .unwrap();
    assert!(engine.tick(140_000).gestures.is_empty());
}

#[test]
fn fault_reset_requires_a_new_released_baseline_and_keeps_latched_requests() {
    let mut rig = Rig::new(config());
    rig.button(10_000, 0, true);
    rig.button(20_000, 0, false);
    let recorded = rig.events[0].clone();
    rig.button(210_000, 0, true);
    rig.engine.input_fault(220_000, ControlFault::Missing);
    let fault = rig.engine.tick(220_000);
    assert_eq!(fault.state.latest_stop, Some(recorded.clone()));
    assert!(!fault.state.ready);
    rig.now = 220_000;
    rig.to(230_000);
    assert!(!rig.engine.tick(230_000).state.buttons[0].armed);
    rig.button(240_000, 0, false);
    assert_eq!(rig.events.len(), 1);
    rig.button(250_000, 0, true);
    rig.button(260_000, 0, false);
    assert_eq!(rig.events.len(), 2);
    assert_eq!(recorded.stamp.sequence, 1);
    assert_eq!(rig.events[1].stamp.sequence, 2);
}

#[test]
fn missing_extra_duplicate_and_unknown_levels_are_faults() {
    for variant in 0..5 {
        let mut engine = Engine::new(config(), "boot", "controls").unwrap();
        engine.ingest(0, observation(0, 1, false)).unwrap();
        engine.ingest(10_000, observation(10_000, 2, true)).unwrap();
        let mut bad = observation(20_000, 3, false);
        match variant {
            0 => bad.buttons.clear(),
            1 => bad.buttons.push(Level {
                name: "extra".into(),
                active: false,
            }),
            2 => bad.buttons.push(bad.buttons[0].clone()),
            3 => bad.buttons[0].name = "unknown".into(),
            _ => bad.base_mask = Some(0),
        }
        assert!(engine.ingest(20_000, bad).is_err());
        let state = engine.tick(20_000);
        assert!(!state.state.ready);
        assert!(state.gestures.is_empty());
        assert_eq!(
            engine
                .ingest(30_000, observation(30_000, 4, false))
                .unwrap_err()
                .fault,
            ControlFault::Overflow
        );
        engine
            .ingest(40_000, observation(40_000, 5, false))
            .unwrap();
        assert!(engine.tick(40_000).gestures.is_empty());
    }
}

#[test]
fn future_wrong_boot_and_out_of_order_observations_cancel_pending_edges() {
    for variant in 0..4 {
        let mut engine = Engine::new(config(), "boot", "controls").unwrap();
        engine.ingest(0, observation(0, 1, false)).unwrap();
        engine.ingest(10_000, observation(10_000, 2, true)).unwrap();
        let mut bad = observation(20_000, 3, false);
        let expected = match variant {
            0 => {
                bad.stamp.at_us = 20_001;
                ControlFault::Future
            }
            1 => {
                bad.stamp.clock_id = "another-boot".into();
                ControlFault::WrongClock
            }
            2 => {
                bad.stamp.at_us = 10_000;
                ControlFault::OutOfOrder
            }
            _ => {
                bad.stamp.sequence = 2;
                ControlFault::OutOfOrder
            }
        };
        assert_eq!(engine.ingest(20_000, bad).unwrap_err().fault, expected);
        assert!(engine.tick(20_000).gestures.is_empty());
        assert_eq!(
            engine
                .ingest(30_000, observation(30_000, 4, false))
                .unwrap_err()
                .fault,
            ControlFault::Overflow
        );
        engine
            .ingest(40_000, observation(40_000, 5, false))
            .unwrap();
        assert!(engine.tick(40_000).gestures.is_empty());
    }
}

#[test]
fn reader_restart_and_late_old_reader_cannot_join_a_hold() {
    let mut engine = Engine::new(config(), "boot", "controls").unwrap();
    engine.ingest(0, observation(0, 1, false)).unwrap();
    engine.ingest(10_000, observation(10_000, 2, true)).unwrap();
    let mut new = observation(20_000, 1, false);
    new.session_id = "new-reader".into();
    assert_eq!(
        engine.ingest(20_000, new.clone()).unwrap_err().fault,
        ControlFault::ReaderChanged
    );
    new.stamp.at_us = 30_000;
    new.stamp.sequence = 2;
    engine.ingest(30_000, new).unwrap();
    assert!(engine.tick(30_000).gestures.is_empty());
    assert_eq!(
        engine
            .ingest(40_000, observation(40_000, 3, false))
            .unwrap_err()
            .fault,
        ControlFault::ReaderChanged
    );
}

#[test]
fn backwards_arrival_clock_latches_failure_until_new_engine() {
    let mut engine = Engine::new(config(), "boot", "controls").unwrap();
    engine
        .ingest(10_000, observation(10_000, 1, false))
        .unwrap();
    assert_eq!(
        engine.tick(9_999).state.fault,
        Some(ControlFault::ClockReset)
    );
    engine.reset(20_000);
    assert_eq!(
        engine
            .ingest(30_000, observation(30_000, 2, false))
            .unwrap_err()
            .fault,
        ControlFault::ClockReset
    );
    assert!(!engine.tick(30_000).state.ready);
}

#[test]
fn near_overflow_clock_is_a_fault_not_a_wrapped_timer() {
    let mut engine = Engine::new(config(), "boot", "controls").unwrap();
    assert_eq!(
        engine
            .ingest(u64::MAX, observation(u64::MAX, 1, false))
            .unwrap_err()
            .fault,
        ControlFault::ClockReset
    );
    let output = engine.tick(u64::MAX);
    output.state.check().unwrap();
    assert!(!output.state.ready);
    assert!(output.gestures.is_empty());
}

#[test]
fn default_and_explicit_shadow_never_produce_enabled_requests() {
    let empty = Engine::new(Config::default(), "boot", "controls")
        .unwrap()
        .tick(0);
    assert!(!empty.state.enabled);
    assert!(!empty.state.ready);
    let mut cfg = config();
    cfg.enabled = false;
    let mut rig = Rig::new(cfg);
    rig.button(10_000, 0, true);
    rig.button(20_000, 0, false);
    assert_eq!(rig.events.len(), 1);
    assert!(!rig.events[0].enabled);
    assert!(!rig.events[0].fresh_at("boot", 20_000));
    let state = rig.engine.tick(20_000).state;
    assert!(!state.enabled);
    assert!(!state.latest_stop.unwrap().enabled);
}

fn head_config() -> Config {
    Config {
        enabled: true,
        buttons: vec![],
        head_pads: vec!["left".into(), "right".into()],
        base_selected_mask: None,
    }
}

#[test]
fn known_head_pads_accept_a_short_pulse_without_fabricating_a_hold() {
    let mut rig = Rig::new(head_config());
    rig.head(600_000, 0, true);
    rig.head(601_000, 0, false);
    let deadline = 601_000 + HEAD_SESSION_US + HEAD_DECISION_US;
    rig.to(deadline - 1);
    assert!(rig.events.is_empty());
    rig.to(deadline);
    assert_eq!(rig.kinds(), vec![Gesture::Pet]);
    assert_eq!(rig.events[0].source, ControlSource::Headpad);
    assert_eq!(rig.events[0].intent, ControlIntent::Pet);
    assert_eq!(rig.events[0].duration_us, 1_000);
    assert_eq!(rig.events[0].stamp.at_us, deadline);
    assert_eq!(rig.events[0].released_us, 601_000);
}

#[test]
fn startup_pad_contact_requires_settle_and_released_baseline() {
    let mut rig = Rig::new(head_config());
    rig.head(10_000, 0, true);
    rig.to(2_000_000);
    assert!(rig.events.is_empty());
    rig.head(2_010_000, 0, false);
    rig.to(3_500_000);
    assert!(rig.events.is_empty());
    rig.head(3_600_000, 0, true);
    rig.head(3_650_000, 0, false);
    rig.to(5_050_000);
    assert_eq!(rig.kinds(), vec![Gesture::Pet]);
}

#[test]
fn each_head_session_rearms_decision_and_release_is_not_another_pet() {
    let mut rig = Rig::new(head_config());
    rig.head(600_000, 0, true);
    rig.head(650_000, 0, false);
    rig.head(1_600_000, 1, true);
    rig.head(1_650_000, 1, false);
    rig.to(2_050_000);
    assert!(rig.events.is_empty());
    rig.to(3_049_999);
    assert!(rig.events.is_empty());
    rig.to(3_050_000);
    assert_eq!(rig.kinds(), vec![Gesture::Pet]);
    rig.to(4_000_000);
    assert_eq!(rig.events.len(), 1);
}

#[test]
fn head_cooldown_is_extended_by_swallowed_session_and_accepts_exact_expiry() {
    let mut rig = Rig::new(head_config());
    rig.head(600_000, 0, true);
    rig.head(650_000, 0, false);
    rig.to(2_050_000);
    assert_eq!(rig.events.len(), 1);
    rig.head(3_000_000, 0, true);
    rig.head(3_050_000, 0, false);
    // Session ends at 3.25 s, extending cooldown from 3.55 s to 4.75 s.
    rig.to(3_650_000);
    assert_eq!(rig.events.len(), 1);
    rig.head(4_500_000, 0, true);
    rig.head(4_550_000, 0, false);
    rig.to(5_950_000);
    assert_eq!(rig.events.len(), 2);
    assert!(
        rig.events
            .iter()
            .all(|event| event.intent == ControlIntent::Pet)
    );
}

#[test]
fn a_long_head_contact_is_never_a_button_hold_or_an_early_action() {
    let mut rig = Rig::new(head_config());
    rig.head(600_000, 0, true);
    rig.to(6_000_000);
    assert!(rig.events.is_empty());
    // Over a pat's longest (#2239's P2), so no Pet at its release either.
    rig.head(6_010_000, 0, false);
    rig.to(7_410_000);
    assert!(rig.events.is_empty());
}

/// The reviewer's P2 on #2239 (rule 8): lamp-cb94's line 97 drifted high at
/// rest for 77 s and back (#1764). That's no pat, so no `Pet`, which would
/// wake a sleeping lamp with nobody there. A hand's 1 s pat still is one,
/// and so is a press of exactly the longest pat; the cycle after a long
/// press counts again.
#[test]
fn a_press_longer_than_a_pat_gives_no_pet_and_a_pat_still_does() {
    let decided = |release: u64| release + HEAD_SESSION_US + HEAD_DECISION_US;
    let mut drift = Rig::new(head_config());
    drift.head(600_000, 1, true);
    drift.head(77_600_000, 1, false);
    drift.to(decided(77_600_000) + 1_000_000);
    assert!(drift.events.is_empty(), "{:?}", drift.kinds());
    drift.head(81_000_000, 0, true);
    drift.head(82_000_000, 0, false);
    drift.to(decided(82_000_000));
    assert_eq!(drift.kinds(), vec![Gesture::Pet]);
    let mut pat = Rig::new(head_config());
    pat.head(600_000, 0, true);
    pat.head(1_600_000, 0, false);
    pat.to(decided(1_600_000));
    assert_eq!(pat.kinds(), vec![Gesture::Pet]);
    assert_eq!(pat.events[0].duration_us, 1_000_000);
    let mut longest = Rig::new(head_config());
    longest.head(600_000, 0, true);
    longest.head(600_000 + HEAD_PAT_LONGEST_US, 0, false);
    longest.to(decided(600_000 + HEAD_PAT_LONGEST_US));
    assert_eq!(longest.kinds(), vec![Gesture::Pet]);
}

#[test]
fn a_missing_input_cancels_a_pending_pet_decision() {
    let mut rig = Rig::new(head_config());
    rig.head(600_000, 0, true);
    rig.head(650_000, 0, false);
    rig.to(1_000_000);
    rig.engine.input_fault(1_010_000, ControlFault::Missing);
    rig.now = 1_010_000;
    rig.to(2_500_000);
    assert!(rig.events.is_empty());
}

#[test]
fn later_petting_does_not_overwrite_stop_or_mute_recovery_records() {
    let mut cfg = head_config();
    cfg.buttons = vec!["button".into()];
    let mut rig = Rig::new(cfg);
    rig.button(10_000, 0, true);
    rig.button(20_000, 0, false);
    rig.button(210_000, 0, true);
    rig.button(2_210_000, 0, false);
    let stop = rig.events[0].clone();
    let mute = rig.events[1].clone();
    rig.head(2_300_000, 0, true);
    rig.head(2_350_000, 0, false);
    rig.to(3_750_000);
    assert_eq!(
        rig.kinds(),
        vec![Gesture::Tap, Gesture::LongPress, Gesture::Pet]
    );
    let state = rig.engine.tick(rig.now).state;
    assert_eq!(state.latest_stop, Some(stop));
    assert_eq!(state.latest_mute, Some(mute.clone()));
    assert_eq!(state.event_sequence, 3);
    rig.engine.input_fault(rig.now, ControlFault::Missing);
    assert_eq!(rig.engine.tick(rig.now).state.latest_mute, Some(mute));
}

#[test]
fn base_tap_and_hold_share_safe_intents_but_not_gpio_timing() {
    let cfg = Config {
        enabled: true,
        buttons: vec![],
        head_pads: vec![],
        base_selected_mask: Some(7),
    };
    let mut rig = Rig::new(cfg);
    rig.to(10_000);
    assert!(rig.engine.tick(10_000).state.base_ready);
    rig.base(100_000, 7);
    rig.to(120_000);
    rig.base(200_000, 0);
    rig.to(229_999);
    assert!(rig.events.is_empty());
    rig.to(230_000);
    assert_eq!(rig.events[0].source, ControlSource::Base);
    assert_eq!(rig.events[0].intent, ControlIntent::Stop);
    assert_eq!(rig.events[0].released_us, 200_000);
    rig.base(500_000, 7);
    rig.to(2_500_000);
    assert_eq!(rig.events.len(), 1);
    rig.base(2_500_001, 0);
    rig.to(2_530_001);
    assert_eq!(rig.events[1].intent, ControlIntent::SoftwareMute);
    assert_eq!(rig.events[1].duration_us, 2_000_001);
}

#[test]
fn incomplete_or_invalid_base_snapshot_disarms_every_input() {
    let mut cfg = config();
    cfg.base_selected_mask = Some(7);
    let mut rig = Rig::new(cfg);
    rig.button(10_000, 0, true);
    let mut bad = observation(20_000, 3, false);
    bad.base_mask = Some(0x1000);
    assert_eq!(
        rig.engine.ingest(20_000, bad).unwrap_err().fault,
        ControlFault::Invalid
    );
    let output = rig.engine.tick(20_000);
    assert!(output.gestures.is_empty());
    assert!(!output.state.base_ready);
    assert!(output.state.latest_stop.is_none());
}

#[test]
fn bounded_output_queue_fails_closed_instead_of_dropping_transitions_silently() {
    let mut cfg = config();
    cfg.buttons = (0..8).map(|i| format!("button-{i}")).collect();
    let mut engine = Engine::new(cfg.clone(), "boot", "controls").unwrap();
    let mut sequence = 0;
    let mut now = 0;
    let mut level = false;
    let mut failure = None;
    'outer: for cycle in 0..10 {
        for step in 0..6 {
            if step == 1 {
                level = true;
            }
            if step == 2 {
                level = false;
            }
            sequence += 1;
            let input = Observation {
                stamp: ClockStamp {
                    clock_id: "boot".into(),
                    at_us: now,
                    sequence,
                },
                session_id: "reader".into(),
                buttons: cfg
                    .buttons
                    .iter()
                    .map(|name| Level {
                        name: name.clone(),
                        active: level,
                    })
                    .collect(),
                head_pads: vec![],
                base_mask: None,
            };
            if let Err(error) = engine.ingest(now, input) {
                failure = Some(error);
                break 'outer;
            }
            now += 50_000;
        }
        assert!(cycle < 9);
    }
    assert_eq!(failure.unwrap().fault, ControlFault::Overflow);
    let output = engine.tick(now);
    assert!(output.gestures.is_empty());
    assert!(!output.state.ready);
    assert_eq!(output.state.event_sequence, MAX_PENDING_GESTURES as u64);
    assert_eq!(output.state.rejected_inputs, 1);
}

#[test]
fn config_and_input_storage_limits_are_checked_without_pin_guessing() {
    assert!(Engine::new(config(), "", "controls").is_err());
    let mut cfg = config();
    cfg.buttons = vec!["same".into(); 2];
    assert!(cfg.check().is_err());
    cfg.buttons = (0..9).map(|n| n.to_string()).collect();
    assert_eq!(cfg.check().unwrap_err().fault, ControlFault::Overflow);
    cfg = config();
    cfg.head_pads = (0..5).map(|n| n.to_string()).collect();
    assert_eq!(cfg.check().unwrap_err().fault, ControlFault::Overflow);
    cfg = config();
    cfg.base_selected_mask = Some(3);
    assert!(cfg.check().is_err());
    let mut input = observation(0, 1, false);
    input.session_id = "x".repeat(129);
    assert!(input.check().is_err());
    let mut wire = serde_json::to_value(observation(0, 1, false)).unwrap();
    wire["privacy_switch"] = true.into();
    assert!(serde_json::from_value::<Observation>(wire).is_err());
}

#[test]
fn scripted_reader_preserves_acquisition_time_and_none_is_not_a_heartbeat() {
    let mut reader =
        ScriptedPins::new([observation(10_000, 1, false), observation(20_000, 2, true)]).unwrap();
    assert!(reader.read(9_999).unwrap().is_none());
    let first = reader.read(50_000).unwrap().unwrap();
    assert_eq!(first.stamp.at_us, 10_000);
    let second = reader.read(50_000).unwrap().unwrap();
    assert_eq!(second.stamp.at_us, 20_000);
    assert!(reader.read(50_000).unwrap().is_none());
    let mut engine = Engine::new(config(), "boot", "controls").unwrap();
    engine.ingest(50_000, first).unwrap();
    engine.ingest(50_000, second).unwrap();
    assert_eq!(
        engine.tick(20_000 + INPUT_FRESH_US).state.fault,
        Some(ControlFault::Stale)
    );
}

#[test]
fn scripted_reader_rejects_unbounded_disordered_or_clock_reset_streams() {
    assert!(ScriptedPins::new([observation(1, 1, false), observation(1, 2, true)]).is_err());
    let samples = (0..4097).map(|n| observation(n, n + 1, false));
    assert_eq!(
        ScriptedPins::new(samples).err().unwrap().fault,
        ControlFault::Overflow
    );
    let mut reader = ScriptedPins::new([observation(100, 1, false)]).unwrap();
    assert!(reader.read(99).unwrap().is_none());
    assert_eq!(reader.read(98).unwrap_err().fault, ControlFault::ClockReset);
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn output_is_deterministic_for_identical_complete_acquisitions() {
    let inputs = [
        observation(0, 1, false),
        observation(10_000, 2, true),
        observation(20_000, 3, false),
    ];
    let run = || {
        let mut engine = Engine::new(config(), "boot", "controls").unwrap();
        inputs
            .iter()
            .map(|input| {
                engine.ingest(input.stamp.at_us, input.clone()).unwrap();
                serde_json::to_string(&engine.tick(input.stamp.at_us)).unwrap()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn lost_release_and_repress_cannot_extend_a_hold_into_software_mute() {
    let mut rig = Rig::new(config());
    rig.button(10_000, 0, true);
    rig.to(1_960_000);
    // Two lost acquisitions could be release and repress. The received level
    // still says pressed; continuity must fail despite the small time gap.
    let sequence = rig.sequence + 3;
    assert_eq!(
        rig.engine
            .ingest(1_970_000, observation(1_970_000, sequence, true))
            .unwrap_err()
            .fault,
        ControlFault::Overflow
    );
    rig.engine
        .ingest(1_980_000, observation(1_980_000, sequence + 1, true))
        .unwrap();
    rig.engine
        .ingest(2_010_000, observation(2_010_000, sequence + 2, false))
        .unwrap();
    let recovered = rig.engine.tick(2_010_000);
    assert!(recovered.gestures.is_empty());
    assert!(recovered.state.latest_mute.is_none());
    assert!(recovered.state.buttons[0].armed);
    rig.engine
        .ingest(2_020_000, observation(2_020_000, sequence + 3, true))
        .unwrap();
    rig.engine
        .ingest(2_030_000, observation(2_030_000, sequence + 4, false))
        .unwrap();
    assert_eq!(
        rig.engine.tick(2_030_000).gestures[0].intent,
        ControlIntent::Stop
    );
}

#[test]
fn reader_session_storage_is_bounded_and_never_reaccepts_a_retired_reader() {
    let mut engine = Engine::new(config(), "boot", "controls").unwrap();
    engine.ingest(0, observation(0, 1, false)).unwrap();
    for index in 1..=32 {
        let mut input = observation(index * 1_000, 1, false);
        input.session_id = format!("reader-{index}");
        assert_eq!(
            engine.ingest(index * 1_000, input).unwrap_err().fault,
            ControlFault::ReaderChanged
        );
    }
    let mut final_reader = observation(33_000, 1, false);
    final_reader.session_id = "reader-33".into();
    assert_eq!(
        engine.ingest(33_000, final_reader).unwrap_err().fault,
        ControlFault::Overflow
    );
    assert_eq!(
        engine.tick(33_000).state.fault,
        Some(ControlFault::Overflow)
    );
    assert!(
        engine
            .ingest(34_000, observation(34_000, 2, false))
            .is_err()
    );
}

#[test]
fn mechanical_double_window_uses_release_edges_with_an_inclusive_boundary() {
    for gap in [399_999, 400_000, 400_001] {
        let mut rig = Rig::new(config());
        rig.button(10_000, 0, true);
        rig.button(20_000, 0, false);
        rig.button(20_000 + gap - 10_000, 0, true);
        rig.button(20_000 + gap, 0, false);
        assert_eq!(
            rig.events[1].gesture,
            if gap <= 400_000 {
                Gesture::DoubleTap
            } else {
                Gesture::Tap
            }
        );
        assert_eq!(rig.events[1].intent, ControlIntent::Stop);
    }
}

#[test]
fn synthetic_first_and_repeated_taps_meet_the_two_millisecond_dispatch_budget() {
    let mut latencies = Vec::new();
    for phase_us in [0, 1, 200, 999, 1_000, 1_001, 1_200, 1_999] {
        let mut engine = Engine::new(config(), "boot", "controls").unwrap();
        let mut sequence = 1;
        engine.ingest(0, observation(0, sequence, false)).unwrap();
        let mut observed_us = 0;
        for press_us in [10_000 + phase_us, 600_000 + phase_us] {
            while observed_us + 50_000 < press_us {
                observed_us += 50_000;
                sequence += 1;
                engine
                    .ingest(observed_us, observation(observed_us, sequence, false))
                    .unwrap();
                engine.tick(observed_us);
            }
            sequence += 1;
            let press_arrival = press_us.div_ceil(2_000) * 2_000;
            engine
                .ingest(press_arrival, observation(press_us, sequence, true))
                .unwrap();
            assert!(engine.tick(press_arrival).gestures.is_empty());
            let released_us = press_us + 1_000;
            let receipt_us = released_us.div_ceil(2_000) * 2_000;
            sequence += 1;
            engine
                .ingest(receipt_us, observation(released_us, sequence, false))
                .unwrap();
            let output = engine.tick(receipt_us);
            assert_eq!(output.gestures.len(), 1);
            latencies.push(output.gestures[0].stamp.at_us - released_us);
            observed_us = released_us;
        }
    }
    assert_eq!(latencies.len(), 16);
    assert_eq!(latencies.iter().min(), Some(&0));
    assert_eq!(latencies.iter().max(), Some(&1_999));
    // These are injected acquisition-to-dispatch times on a nominal 2 ms poll
    // grid, not measured GPIO-to-speaker/motor latency or lamp measurements.
    assert!(latencies.into_iter().all(|latency| latency < 2_000));
}

#[test]
fn base_hold_feedback_uses_observations_and_waits_for_release_to_request_mute() {
    let cfg = Config {
        enabled: true,
        buttons: vec![],
        head_pads: vec![],
        base_selected_mask: Some(7),
    };
    let mut rig = Rig::new(cfg);
    rig.to(10_000);
    rig.base(100_000, 7);
    rig.to(120_000);
    for (at, tier) in [
        (2_099_999, HoldTier::None),
        (2_100_000, HoldTier::Mute),
        (5_100_000, HoldTier::ExtendedMute),
    ] {
        rig.to(at);
        let state = rig.engine.tick(at).state;
        assert_eq!(state.base_held_us, Some(at - 100_000));
        assert_eq!(state.base_tier, tier);
        assert!(rig.events.is_empty());
    }
    rig.base(5_100_001, 0);
    let state = rig.engine.tick(5_120_000).state;
    assert_eq!(state.base_held_us, Some(5_000_001));
    assert_eq!(state.base_tier, HoldTier::ExtendedMute);
    assert!(rig.events.is_empty());
    rig.to(5_130_001);
    assert_eq!(rig.events.len(), 1);
    assert_eq!(rig.events[0].intent, ControlIntent::SoftwareMute);
    assert_eq!(rig.engine.tick(rig.now).state.base_held_us, None);
}

#[test]
fn delayed_base_sample_and_ticks_cannot_cross_a_hold_tier_without_acquisition() {
    let cfg = Config {
        enabled: true,
        buttons: vec![],
        head_pads: vec![],
        base_selected_mask: Some(7),
    };
    let mut rig = Rig::new(cfg);
    rig.to(10_000);
    rig.base(100_000, 7);
    rig.to(120_000);
    rig.to(2_049_999);
    let at = 2_099_999;
    let input = Observation {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: at,
            sequence: rig.sequence + 1,
        },
        session_id: "reader".into(),
        buttons: vec![],
        head_pads: vec![],
        base_mask: Some(7),
    };
    rig.engine.ingest(at + 40_000, input).unwrap();
    let state = rig.engine.tick(at + 80_000).state;
    assert_eq!(state.base_held_us, Some(1_999_999));
    assert_eq!(state.base_tier, HoldTier::None);
    assert!(state.ready);
    let stale = rig.engine.tick(at + 100_000);
    assert_eq!(stale.state.fault, Some(ControlFault::Stale));
    assert!(!stale.state.base_ready);
    assert_eq!(stale.state.base_held_us, None);
    assert_eq!(stale.state.base_tier, HoldTier::None);
    assert!(stale.gestures.is_empty());
}

/// #3697: with the shutdown switch on, the legacy engine (the voice's mute
/// in touch mode) asks no mute for a hold of 5 s or more, which is the touch
/// path's shutdown or its cancel; under 5 s the mute is as before, and off,
/// every hold of 2 s or more is.
#[test]
fn with_the_shutdown_switch_a_hold_of_five_seconds_or_more_is_no_mute() {
    for (on, held, mute) in [
        (true, 1_000_000, false),
        (true, 3_000_000, true),
        (true, 6_000_000, false),
        (true, 20_000_000, false),
        (false, 6_000_000, true),
        (false, 20_000_000, true),
    ] {
        let mut rig = Rig::new(config());
        rig.engine.set_shutdown(on);
        rig.button(10_000, 0, true);
        rig.to(9_000 + held);
        rig.button(10_000 + held, 0, false);
        let mutes = rig
            .events
            .iter()
            .filter(|event| event.intent == ControlIntent::SoftwareMute)
            .count();
        assert_eq!(mutes, usize::from(mute), "on {on}, held {held}");
    }
}
