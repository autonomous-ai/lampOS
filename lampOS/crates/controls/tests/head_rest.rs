//! #711 and #1764: head pads that rest and read differently on each unit.
//! A pad the factory step measured as pulsing reads touched only while its
//! line keeps changing, so a line whose rest flips, as lamp-cb94's line 97
//! did for 77 s with no hand on it, is never a touch, a stroke or a sleep.
use lamp_controls::{input::Level, touch::*};
use lamp_msgs::{ClockStamp, TouchEvent, TouchGesture};

struct Rig {
    engine: TouchEngine,
    sequence: u64,
}

impl Rig {
    fn new(pulses: bool) -> Self {
        let config = TouchConfig {
            enabled: true,
            head_pads: vec!["head-1".into(), "head-2".into()],
            head_pulse_pads: if pulses {
                vec!["head-1".into()]
            } else {
                vec![]
            },
            ..TouchConfig::default()
        };
        Self {
            engine: TouchEngine::new(config, "boot", "controls").unwrap(),
            sequence: 0,
        }
    }
    /// One acquisition of head-1's line, `high`, with head-2 released.
    fn read(&mut self, at: u64, high: bool) -> Vec<TouchEvent> {
        self.sequence += 1;
        let sample = GpioSample {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: at,
                sequence: self.sequence,
            },
            session_id: "gpio".into(),
            primary: None,
            reset: None,
            head_pads: ["head-1", "head-2"]
                .into_iter()
                .zip([high, false])
                .map(|(name, active)| Level {
                    name: name.into(),
                    active,
                })
                .collect(),
        };
        self.engine.ingest_gpio(at, sample).unwrap();
        let output = self.engine.tick(at);
        output.state.check().unwrap();
        output.events
    }
    /// Every 10 ms from `from` to before `to`, the line as `line(at)` says.
    fn run(&mut self, from: u64, to: u64, line: impl Fn(u64) -> bool) -> Vec<TouchGesture> {
        (from..to)
            .step_by(10_000)
            .flat_map(|at| self.read(at, line(at)))
            .map(|event| event.gesture)
            .collect()
    }
}

/// A hand resting on a pulsing pad: 40 ms changed, 40 ms back, from `from`
/// for `for_us`.
fn pulsing(from: u64, for_us: u64, rest: bool) -> impl Fn(u64) -> bool {
    move |at| {
        if (from..from + for_us).contains(&at) {
            ((at - from) / 40_000).is_multiple_of(2) != rest
        } else {
            rest
        }
    }
}

#[test]
fn a_rest_that_flips_is_no_touch_on_a_pulse_pad_but_a_held_hand_on_a_level_pad() {
    // lamp-cb94's line 97 (#1764): low, then high for 77 s, nobody there.
    let flip = |at: u64| (2_000_000..79_000_000).contains(&at);
    assert_eq!(Rig::new(true).run(0, 80_000_000, flip), []);
    // Read by its level, as before, the same line is a hand held: a stroke
    // and a sleep.
    assert_eq!(
        Rig::new(false).run(0, 80_000_000, flip),
        [
            TouchGesture::Touch,
            TouchGesture::Stroke,
            TouchGesture::Hold
        ]
    );
}

#[test]
fn a_hand_resting_on_a_pulse_pad_strokes_and_holds_from_either_rest() {
    // lamp-c0a4's pads pulsed under a hand from a low rest (#1764, 08:21Z),
    // and a minute later rested high: both read the same.
    for rest in [false, true] {
        assert_eq!(
            Rig::new(true).run(0, 6_000_000, pulsing(1_000_000, 3_500_000, rest)),
            [
                TouchGesture::Touch,
                TouchGesture::Stroke,
                TouchGesture::Hold
            ],
            "rest high: {rest}"
        );
    }
}

#[test]
fn one_or_two_pulses_are_a_tap_from_either_rest() {
    for rest in [false, true] {
        for pulses in [1, 2] {
            let gestures = Rig::new(true).run(
                0,
                2_000_000,
                pulsing(1_000_000, pulses * 80_000 - 40_000, rest),
            );
            assert_eq!(
                gestures,
                [TouchGesture::Touch, TouchGesture::Tap],
                "rest high: {rest}, pulses: {pulses}"
            );
        }
    }
}

#[test]
fn a_bounce_or_a_line_that_settles_slowly_is_no_touch() {
    // Two changes 10 ms apart: a bounce, under the shortest pulse.
    let bounce = |at: u64| at == 1_000_000;
    assert_eq!(Rig::new(true).run(0, 2_000_000, bounce), []);
    // Two changes 400 ms apart: past the gap, not a pulse.
    let slow = |at: u64| (1_000_000..1_400_000).contains(&at);
    assert_eq!(Rig::new(true).run(0, 3_000_000, slow), []);
}

#[test]
fn a_pulse_pad_arms_on_a_line_resting_high_from_the_start() {
    // Read by its level, a line that rests active never arms the head.
    let mut level = Rig::new(false);
    level.run(0, 1_000_000, |_| true);
    assert!(!level.engine.tick(1_000_000).state.sources[0].armed);
    let mut pulse = Rig::new(true);
    pulse.run(0, 1_000_000, |_| true);
    assert!(pulse.engine.tick(1_000_000).state.sources[0].armed);
}

#[test]
fn pulse_pads_must_be_head_pads_named_once() {
    for pulse in [
        vec!["head-3".to_string()],
        vec!["head-1".into(), "head-1".into()],
    ] {
        let config = TouchConfig {
            enabled: true,
            head_pads: vec!["head-1".into(), "head-2".into()],
            head_pulse_pads: pulse,
            ..TouchConfig::default()
        };
        assert!(config.check().is_err());
    }
}
