use lamp_controls::base::{
    ALL_PADS, AMBIGUOUS_RELEASE_US, BaseError, BaseGesture, BaseGestureKind, BaseRecognizer,
    CONTACT_DEBOUNCE_US, DOUBLE_TAP_US, LONG_PRESS_US, MAX_SAMPLE_AGE_US, MAX_SAMPLE_GAP_US,
    RELEASE_US,
};

struct Rig {
    base: BaseRecognizer,
    now_us: u64,
    mask: u16,
    events: Vec<BaseGesture>,
}

impl Rig {
    fn new() -> Self {
        let mut rig = Self {
            base: BaseRecognizer::new(ALL_PADS).unwrap(),
            now_us: 0,
            mask: 0,
            events: Vec::new(),
        };
        assert_eq!(rig.base.update(0, 0, Some(0)), Ok(None));
        rig.sample(CONTACT_DEBOUNCE_US, 0);
        assert!(rig.base.ready());
        rig
    }

    fn sample(&mut self, at_us: u64, mask: u16) {
        if let Some(event) = self.base.update(at_us, at_us, Some(mask)).unwrap() {
            self.events.push(event);
        }
        self.now_us = at_us;
        self.mask = mask;
    }

    /// Keep the previous sampled mask between edges at the nominal 10 ms
    /// polling cadence; the explicit edge itself keeps its exact test time.
    fn to(&mut self, at_us: u64, mask: u16) {
        while self.now_us + 10_000 < at_us {
            self.sample(self.now_us + 10_000, self.mask);
        }
        self.sample(at_us, mask);
    }

    fn pulse(&mut self, began_us: u64, duration_us: u64) {
        self.to(began_us, 7);
        self.to(began_us + duration_us, 0);
        self.to(began_us + duration_us + RELEASE_US, 0);
    }

    fn kinds(&self) -> Vec<BaseGestureKind> {
        self.events.iter().map(|event| event.kind).collect()
    }
}

#[test]
fn three_filtered_pads_commit_once_after_full_release_without_waiting_for_double_tap() {
    let mut rig = Rig::new();
    rig.to(100_000, 7);
    rig.to(200_000, 0);
    rig.to(229_999, 0);
    assert!(rig.events.is_empty());
    rig.sample(230_000, 0);
    assert_eq!(
        rig.events,
        vec![BaseGesture {
            kind: BaseGestureKind::Tap,
            at_us: 230_000,
            released_us: 200_000,
            duration_us: 100_000,
        }]
    );
    rig.to(1_000_000, 0);
    assert_eq!(rig.events.len(), 1);
}

#[test]
fn a_two_second_hold_has_no_action_until_release_and_never_becomes_a_tap() {
    for duration in [LONG_PRESS_US, 5_000_000, 10_000_000, 15_000_000] {
        let mut rig = Rig::new();
        rig.to(100_000, 7);
        rig.to(100_000 + duration, 7);
        assert!(rig.events.is_empty());
        assert_eq!(rig.base.hold_us(100_000 + duration), Some(duration));
        rig.sample(100_001 + duration, 0);
        assert_eq!(rig.base.hold_us(100_002 + duration), Some(duration + 1));
        rig.to(100_001 + duration + RELEASE_US, 0);
        assert_eq!(rig.kinds(), vec![BaseGestureKind::LongPress]);
        assert_eq!(rig.events[0].duration_us, duration + 1);
        rig.to(700_000 + duration, 0);
        assert_eq!(rig.events.len(), 1);
    }
}

#[test]
fn release_filter_and_grace_cannot_promote_a_short_hold_over_two_seconds() {
    for duration in [LONG_PRESS_US - 1, LONG_PRESS_US] {
        let mut rig = Rig::new();
        rig.pulse(100_000, duration);
        assert_eq!(rig.events.len(), 1);
        assert_eq!(rig.events[0].duration_us, duration);
        assert_eq!(
            rig.events[0].kind,
            if duration < LONG_PRESS_US {
                BaseGestureKind::Tap
            } else {
                BaseGestureKind::LongPress
            }
        );
    }
}

#[test]
fn first_tap_is_immediate_second_tap_has_an_exact_raw_edge_window() {
    for gap in [DOUBLE_TAP_US - 1, DOUBLE_TAP_US, DOUBLE_TAP_US + 1] {
        let mut rig = Rig::new();
        rig.pulse(100_000, 100_000);
        assert_eq!(rig.kinds(), vec![BaseGestureKind::Tap]);
        assert_eq!(rig.events[0].at_us, 200_000 + RELEASE_US);
        rig.pulse(200_000 + gap, 100_000);
        assert_eq!(
            rig.kinds(),
            vec![
                BaseGestureKind::Tap,
                if gap < DOUBLE_TAP_US {
                    BaseGestureKind::DoubleTap
                } else {
                    BaseGestureKind::Tap
                },
            ]
        );
    }
}

#[test]
fn triples_and_repeated_taps_only_produce_pairs_of_safe_classifications() {
    let mut rig = Rig::new();
    for start in [100_000, 300_000, 500_000, 700_000] {
        rig.pulse(start, 60_000);
    }
    assert_eq!(
        rig.kinds(),
        vec![
            BaseGestureKind::Tap,
            BaseGestureKind::DoubleTap,
            BaseGestureKind::Tap,
            BaseGestureKind::DoubleTap,
        ]
    );
}

#[test]
fn a_hold_consumes_any_previous_tap_pair() {
    let mut rig = Rig::new();
    rig.pulse(100_000, 60_000);
    rig.pulse(250_000, 2_000_000);
    rig.pulse(2_400_000, 60_000);
    assert_eq!(
        rig.kinds(),
        vec![
            BaseGestureKind::Tap,
            BaseGestureKind::LongPress,
            BaseGestureKind::Tap
        ]
    );
}

#[test]
fn one_or_two_pads_and_brief_third_pad_spikes_never_qualify() {
    for mask in [1, 3] {
        let mut rig = Rig::new();
        rig.to(100_000, mask);
        rig.to(3_000_000, mask);
        assert_eq!(rig.base.hold_us(3_000_000), None);
        rig.to(3_100_000, 0);
        rig.to(3_100_000 + AMBIGUOUS_RELEASE_US, 0);
        assert!(rig.events.is_empty());
    }
    let mut rig = Rig::new();
    for (at, mask) in [
        (20_000, 3),
        (25_000, 3),
        (30_000, 3),
        (50_000, 7),
        (55_000, 7),
        (60_000, 3),
        (100_000, 0),
        (150_000, 0),
        (220_000, 0),
    ] {
        rig.sample(at, mask);
    }
    assert!(rig.events.is_empty());
}

#[test]
fn qualified_contact_survives_staggered_lift_and_growth_is_not_a_swipe() {
    for changes in [
        vec![(100_000, 7), (180_000, 3), (200_000, 1), (220_000, 0)],
        vec![
            (100_000, 1),
            (160_000, 7),
            (200_000, 31),
            (240_000, 127),
            (300_000, 126),
            (320_000, 124),
            (340_000, 120),
            (360_000, 0),
        ],
    ] {
        let mut rig = Rig::new();
        for (at, mask) in changes {
            rig.to(at, mask);
        }
        let released_us = rig.now_us;
        rig.to(released_us + RELEASE_US, 0);
        assert_eq!(rig.kinds(), vec![BaseGestureKind::Tap]);
        assert_eq!(rig.events[0].released_us, released_us);
    }
}

#[test]
fn irregular_falling_edge_closes_the_qualifying_interval_before_resetting_it() {
    for (falling, qualifies) in [(44_999, false), (45_000, true), (47_000, true)] {
        let mut rig = Rig::new();
        for (at, mask) in [
            (20_000, 1),
            (25_000, 1),
            (30_000, 7),
            (35_000, 7),
            (falling, 3),
            (60_000, 0),
            (90_000, 0),
            (150_000, 0),
            (180_000, 0),
        ] {
            rig.sample(at, mask);
        }
        assert_eq!(
            rig.events.len(),
            usize::from(qualifies),
            "fall at {falling}"
        );
        if qualifies {
            assert_eq!(rig.events[0].kind, BaseGestureKind::Tap);
        }
    }
}

#[test]
fn separated_short_qualification_intervals_do_not_add_up() {
    let mut rig = Rig::new();
    for (at, mask) in [
        (20_000, 1),
        (25_000, 1),
        (30_000, 7),
        (35_000, 7),
        (39_000, 1),
        (45_000, 1),
        (50_000, 7),
        (55_000, 7),
        (59_000, 1),
        (70_000, 0),
        (140_000, 0),
        (190_000, 0),
    ] {
        rig.sample(at, mask);
    }
    assert!(rig.events.is_empty());
}

#[test]
fn qualification_does_not_leak_to_a_following_small_contact() {
    let mut rig = Rig::new();
    rig.pulse(100_000, 80_000);
    rig.to(300_000, 1);
    rig.to(500_000, 0);
    rig.to(500_000 + AMBIGUOUS_RELEASE_US, 0);
    assert_eq!(rig.kinds(), vec![BaseGestureKind::Tap]);
    rig.pulse(700_000, 80_000);
    assert_eq!(
        rig.kinds(),
        vec![BaseGestureKind::Tap, BaseGestureKind::Tap]
    );
}

#[test]
fn moving_and_reversing_contacts_are_consumed_without_any_swipe_or_hold_action() {
    for masks in [vec![7, 14, 28, 56, 112], vec![7, 14, 28, 14, 7]] {
        let mut rig = Rig::new();
        for (index, mask) in masks.into_iter().enumerate() {
            rig.to(100_000 + index as u64 * 80_000, mask);
        }
        rig.to(3_000_000, 7);
        assert_eq!(rig.base.hold_us(3_000_000), None);
        rig.to(3_100_000, 0);
        rig.to(3_100_000 + AMBIGUOUS_RELEASE_US, 0);
        assert!(rig.events.is_empty());
        rig.pulse(3_400_000, 80_000);
        assert_eq!(rig.kinds(), vec![BaseGestureKind::Tap]);
    }
}

#[test]
fn brief_full_release_bounce_keeps_one_contact_and_its_original_hold_start() {
    let mut rig = Rig::new();
    rig.to(100_000, 7);
    rig.to(1_000_000, 0);
    rig.to(1_010_000, 7);
    rig.to(2_200_000, 0);
    rig.to(2_200_000 + RELEASE_US, 0);
    assert_eq!(rig.kinds(), vec![BaseGestureKind::LongPress]);
    assert_eq!(rig.events[0].duration_us, 2_100_000);
}

#[test]
fn a_new_touch_on_the_release_deadline_does_not_swallow_the_completed_tap() {
    let mut rig = Rig::new();
    rig.to(100_000, 7);
    rig.to(200_000, 0);
    rig.to(230_000, 7);
    assert_eq!(rig.kinds(), vec![BaseGestureKind::Tap]);
    rig.to(330_000, 0);
    rig.to(360_000, 0);
    assert_eq!(
        rig.kinds(),
        vec![BaseGestureKind::Tap, BaseGestureKind::DoubleTap]
    );
}

#[test]
fn a_boot_held_contact_is_ignored_until_full_stable_release() {
    let mut base = BaseRecognizer::new(ALL_PADS).unwrap();
    for at in (0..=3_000_000).step_by(10_000) {
        assert_eq!(base.update(at, at, Some(7)), Ok(None));
        assert!(!base.ready());
        assert_eq!(base.hold_us(at), None);
    }
    for (at, mask) in [
        (3_010_000, 3),
        (3_020_000, 1),
        (3_030_000, 0),
        (3_035_000, 0),
    ] {
        assert_eq!(base.update(at, at, Some(mask)), Ok(None));
        assert!(!base.ready());
    }
    assert_eq!(base.update(3_040_000, 3_040_000, Some(0)), Ok(None));
    assert!(base.ready());
    let mut rig = Rig {
        base,
        now_us: 3_040_000,
        mask: 0,
        events: Vec::new(),
    };
    rig.pulse(3_100_000, 80_000);
    assert_eq!(rig.kinds(), vec![BaseGestureKind::Tap]);
}

#[test]
fn explicit_reset_cancels_contacts_and_tap_history_without_rolling_back_clocks() {
    let mut rig = Rig::new();
    rig.pulse(100_000, 80_000);
    rig.to(300_000, 7);
    rig.to(350_000, 7);
    rig.base.reset();
    assert!(!rig.base.ready());
    assert_eq!(
        rig.base.update(360_000, 349_000, Some(0)),
        Err(BaseError::ObservationOutOfOrder)
    );
    rig.now_us = 360_000;
    rig.to(400_000, 7);
    rig.to(450_000, 0);
    rig.to(500_000, 0);
    assert_eq!(rig.kinds(), vec![BaseGestureKind::Tap]);
    rig.pulse(550_000, 80_000);
    assert_eq!(
        rig.kinds(),
        vec![BaseGestureKind::Tap, BaseGestureKind::Tap]
    );
}

#[test]
fn unavailable_or_invalid_masks_are_faults_and_never_synthetic_releases() {
    for (mask, error) in [
        (None, BaseError::UnknownSample),
        (Some(0x8000), BaseError::InvalidMask),
        (Some(0x1000), BaseError::InvalidMask),
    ] {
        let mut rig = Rig::new();
        rig.to(100_000, 7);
        rig.to(2_200_000, 7);
        assert_eq!(rig.base.update(2_210_000, 2_210_000, mask), Err(error));
        rig.now_us = 2_210_000;
        assert!(!rig.base.ready());
        assert_eq!(rig.base.hold_us(2_210_000), None);
        rig.to(2_220_000, 7);
        rig.to(2_300_000, 0);
        rig.to(2_340_000, 0);
        assert!(rig.events.is_empty());
        rig.pulse(2_500_000, 80_000);
        assert_eq!(rig.kinds(), vec![BaseGestureKind::Tap]);
    }
}

#[test]
fn stale_future_duplicate_and_backward_time_disarm_without_any_release() {
    let mut base = BaseRecognizer::new(ALL_PADS).unwrap();
    assert_eq!(
        base.update(MAX_SAMPLE_AGE_US, 0, Some(0)),
        Err(BaseError::StaleSample)
    );
    assert!(!base.ready());
    let mut base = BaseRecognizer::new(ALL_PADS).unwrap();
    assert_eq!(
        base.update(10_000, 10_001, Some(0)),
        Err(BaseError::FutureSample)
    );
    assert!(!base.ready());
    for (arrival, observed, error) in [
        (9_999, 9_999, BaseError::ClockBackwards),
        (20_000, 10_000, BaseError::ObservationOutOfOrder),
        (20_000, 9_999, BaseError::ObservationOutOfOrder),
    ] {
        let mut rig = Rig::new();
        assert_eq!(rig.base.update(arrival, observed, Some(0)), Err(error));
        assert!(!rig.base.ready());
    }
}

#[test]
fn a_gap_over_one_hundred_ms_discards_the_contact_and_recovers_only_from_release() {
    let mut rig = Rig::new();
    rig.to(100_000, 7);
    rig.to(130_000, 7);
    let at = 130_000 + MAX_SAMPLE_GAP_US + 1;
    assert_eq!(rig.base.update(at, at, Some(0)), Err(BaseError::SampleGap));
    assert!(!rig.base.ready());
    rig.now_us = at;
    rig.mask = 0;
    rig.to(at + 10_000, 0);
    rig.to(at + 20_000, 0);
    assert!(rig.base.ready());
    assert!(rig.events.is_empty());
    rig.pulse(at + 100_000, 80_000);
    assert_eq!(rig.kinds(), vec![BaseGestureKind::Tap]);
}

#[test]
fn only_selected_electrodes_count_and_upper_status_bits_are_not_touch_bits() {
    for mask in [0, 1, 3, 0x1000, 0xffff] {
        assert_eq!(
            BaseRecognizer::new(mask).unwrap_err(),
            BaseError::InvalidSelectedMask
        );
    }
    let mut base = BaseRecognizer::new(7).unwrap();
    for at in (0..=100_000).step_by(10_000) {
        assert_eq!(base.update(at, at, Some(0x800)), Ok(None));
    }
    assert!(base.ready());
    for at in (110_000..=210_000).step_by(10_000) {
        assert_eq!(base.update(at, at, Some(0x803)), Ok(None));
    }
    for at in (220_000..=400_000).step_by(10_000) {
        assert_eq!(base.update(at, at, Some(0x800)), Ok(None));
    }
}

#[test]
fn arrival_delay_is_visible_without_becoming_hold_duration() {
    let mut base = BaseRecognizer::new(ALL_PADS).unwrap();
    let mut events = Vec::new();
    for observed in (0..=300_000).step_by(10_000) {
        let mask = if (100_000..200_000).contains(&observed) {
            7
        } else {
            0
        };
        if let Some(event) = base
            .update(observed + 50_000, observed, Some(mask))
            .unwrap()
        {
            events.push(event);
        }
    }
    assert_eq!(
        events,
        vec![BaseGesture {
            kind: BaseGestureKind::Tap,
            at_us: 280_000,
            released_us: 200_000,
            duration_us: 100_000
        }]
    );
}

#[test]
fn hold_feedback_uses_acquisition_time_and_does_not_advance_on_later_ticks() {
    let mut base = BaseRecognizer::new(ALL_PADS).unwrap();
    for observed in (0..=2_090_000).step_by(10_000) {
        let mask = if observed >= 100_000 { 7 } else { 0 };
        assert_eq!(
            base.update(observed + 50_000, observed, Some(mask)),
            Ok(None)
        );
    }
    assert_eq!(base.hold_us(2_140_000), Some(1_990_000));
    assert_eq!(base.hold_us(2_149_999), Some(1_990_000));
    // Crossing the two-second tier requires another actual held acquisition.
    assert_eq!(base.update(2_150_000, 2_100_000, Some(7)), Ok(None));
    assert_eq!(base.hold_us(2_150_000), Some(2_000_000));
    assert_eq!(base.hold_us(2_199_999), Some(2_000_000));
    assert_eq!(base.hold_us(2_200_000), None);
}

fn sampled_trace(changes: &[(u64, u16)], phase: u64) -> Vec<BaseGesture> {
    let mut base = BaseRecognizer::new(ALL_PADS).unwrap();
    let mut events = Vec::new();
    let mut index = 0;
    let mut mask = 0;
    let until = changes.last().unwrap().0 + 300_000;
    for at in (phase..=until).step_by(10_000) {
        while index < changes.len() && changes[index].0 <= at {
            mask = changes[index].1;
            index += 1;
        }
        if let Some(event) = base.update(at, at, Some(mask)).unwrap() {
            events.push(event);
        }
    }
    events
}

#[test]
fn first_and_repeated_short_pulses_at_four_poll_phases_have_bounded_sampled_latency() {
    // The boundary is the written raw-mask transition to emitted recognition;
    // this includes nominal sampling phase, not chip/ADC or physical latency.
    let mut shortest_us = u64::MAX;
    let mut longest_us = 0;
    let mut accepted = 0;
    for phase in [50, 2_550, 5_050, 7_550] {
        for width in [10_000, 25_000, 30_000, 40_000, 50_000, 80_000] {
            let first = 100_000 + phase;
            let second = 1_000_000 + phase;
            let changes = [
                (first, 7),
                (first + width, 0),
                (second, 7),
                (second + width, 0),
            ];
            let events = sampled_trace(&changes, 0);
            if width == 10_000 {
                assert!(events.is_empty(), "phase {phase}, width {width}");
                continue;
            }
            assert_eq!(events.len(), 2, "phase {phase}, width {width}");
            for (event, edge) in events.iter().zip([first + width, second + width]) {
                assert_eq!(event.kind, BaseGestureKind::Tap);
                assert!(
                    (RELEASE_US..RELEASE_US + 10_000).contains(&(event.at_us - edge)),
                    "phase {phase}, width {width}: {event:?}"
                );
                assert_eq!(event.at_us - event.released_us, RELEASE_US);
                shortest_us = shortest_us.min(event.at_us - edge);
                longest_us = longest_us.max(event.at_us - edge);
                accepted += 1;
            }
        }
    }
    assert_eq!((accepted, shortest_us, longest_us), (40, 32_450, 39_950));
}

#[test]
fn inherited_recorded_mask_traces_keep_stationary_taps_and_consume_travel() {
    // Source: hal/test/test_mpr121_swipe.py::test_recorded_hardware_traces_at_10ms_poll.
    // These historical V1 masks are replayed here, not measured on a lamp by
    // this test. Absolute trace time is shifted by 100 ms for startup release.
    let traces: &[(&[(u64, u16)], bool)] = &[
        (&[(0, 0x00f), (68_565, 0x00d), (80_512, 0)], true),
        (
            &[
                (0, 0xc00),
                (32_394, 0xe00),
                (108_233, 0xf00),
                (140_405, 0xd00),
                (151_704, 0x780),
                (194_663, 0x3c0),
                (223_980, 0x1c0),
                (235_201, 0x1e0),
                (246_512, 0x0f0),
                (268_403, 0x070),
                (279_260, 0x078),
                (290_454, 0x038),
                (301_521, 0x03c),
                (323_502, 0x01c),
                (338_959, 0x01e),
                (350_452, 0x00f),
                (372_375, 0x007),
                (384_148, 0x003),
                (395_206, 0x001),
                (407_554, 0),
            ],
            false,
        ),
        (
            &[
                (0, 1),
                (64_855, 3),
                (97_039, 7),
                (109_994, 6),
                (121_576, 0x00e),
                (132_669, 0x00c),
                (156_368, 0x01c),
                (169_716, 0x018),
                (181_015, 0x030),
                (192_587, 0x070),
                (203_921, 0x060),
                (215_169, 0x0c0),
                (226_225, 0x180),
                (247_988, 0x100),
                (259_447, 0),
            ],
            false,
        ),
        (
            &[
                (0, 0x300),
                (46_940, 0x100),
                (58_170, 0x180),
                (69_253, 0x080),
                (80_455, 0x0c0),
                (91_656, 0x060),
                (102_706, 0x020),
                (113_845, 0x030),
                (124_995, 0x018),
                (146_249, 0x006),
                (159_542, 0x003),
                (170_677, 0x001),
                (181_714, 0),
            ],
            false,
        ),
        (
            &[
                (0, 0x01e),
                (12_110, 0x01f),
                (55_090, 0x00f),
                (66_287, 0x00a),
                (77_446, 0),
            ],
            true,
        ),
    ];
    for phase in [0, 5_000] {
        for (trace, tap) in traces {
            let shifted: Vec<_> = trace
                .iter()
                .map(|(at, mask)| (at + 100_000, *mask))
                .collect();
            let events = sampled_trace(&shifted, phase);
            assert_eq!(
                events.len(),
                usize::from(*tap),
                "phase {phase}, trace {trace:?}"
            );
            assert!(
                events
                    .iter()
                    .all(|event| event.kind == BaseGestureKind::Tap)
            );
        }
    }
}
