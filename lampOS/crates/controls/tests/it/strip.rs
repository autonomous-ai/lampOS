use lamp_controls::base::strip::{
    FRESH_US, MAX_DISTANCE, MAX_EFFECTS, MAX_SPEED, RELEASE_US, SLEEP_US, SPEED_WINDOW_US,
    StripDirection, StripEffect, StripEffectKind, StripError, StripRecognizer, StripState,
};
use serde::Deserialize;

const ORDER: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const WORKING: [u8; 10] = [0, 1, 2, 3, 5, 6, 7, 9, 10, 11];

struct Rig {
    strip: StripRecognizer,
    at_us: u64,
    mask: u16,
    events: Vec<StripEffect>,
    states: Vec<StripState>,
}

impl Rig {
    fn new() -> Self {
        Self::ordered(ORDER)
    }

    fn ordered(order: impl AsRef<[u8]>) -> Self {
        let mut rig = Self {
            strip: StripRecognizer::new(order).unwrap(),
            at_us: 0,
            mask: 0,
            events: Vec::new(),
            states: Vec::new(),
        };
        rig.sample(0, 0);
        rig.sample(10_000, 0);
        assert!(rig.state().ready);
        rig
    }

    fn sample(&mut self, at_us: u64, mask: u16) {
        let update = self.strip.update(at_us, at_us, Some(mask)).unwrap();
        assert!(update.effects.len() <= MAX_EFFECTS);
        assert!(
            update
                .state
                .position
                .is_none_or(|p| (0.0..=1.0).contains(&p))
        );
        assert!((0.0..=MAX_DISTANCE).contains(&update.state.distance));
        assert!((0.0..=MAX_SPEED).contains(&update.state.speed));
        self.events.extend(update.effects);
        self.states.push(update.state);
        self.at_us = at_us;
        self.mask = mask;
    }

    /// Authored full acquisitions every 5 ms, with the requested edge at its
    /// exact time. This is neither a physical threshold nor hardware latency.
    fn to(&mut self, at_us: u64, mask: u16) {
        while self.at_us + 5_000 < at_us {
            self.sample(self.at_us + 5_000, self.mask);
        }
        self.sample(at_us, mask);
    }

    fn state(&self) -> StripState {
        self.strip.state(self.at_us)
    }

    fn count(&self, kind: StripEffectKind) -> usize {
        self.events
            .iter()
            .filter(|event| event.kind == kind)
            .count()
    }
}

fn near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 0.000_01,
        "{actual} != {expected}"
    );
}

fn mask(pads: &[u8]) -> u16 {
    pads.iter().fold(0, |mask, pad| mask | (1 << pad))
}

#[test]
fn working_sets_accept_three_through_twelve_unique_electrodes_in_both_directions() {
    for count in 3..=12 {
        let selected = &ORDER[..count];
        assert!(StripRecognizer::new(selected).is_ok());
        let reversed: Vec<_> = selected.iter().copied().rev().collect();
        assert!(StripRecognizer::new(reversed).is_ok());
    }
    assert!(StripRecognizer::new([0, 5, 11]).is_ok());
    assert!(StripRecognizer::new(WORKING).is_ok());
    // Existing complete non-numeric orders remain an explicit layout map.
    assert!(StripRecognizer::new([2, 0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11]).is_ok());
}

#[test]
fn sparse_layout_rejects_unknown_spacing_duplicate_ids_and_invalid_bounds() {
    for selected in [
        vec![],
        vec![0],
        vec![0, 1],
        vec![0, 0, 1],
        vec![0, 1, 12],
        vec![12, 1, 0],
        vec![0, 2, 1],
        vec![9, 11, 2],
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
    ] {
        assert!(matches!(
            StripRecognizer::new(&selected),
            Err(StripError::InvalidOrder)
        ));
    }
}

#[test]
fn sparse_coordinates_preserve_full_strip_slots_and_do_not_rescale_the_selected_extent() {
    for order in [vec![2, 5, 9], WORKING.to_vec(), ORDER.to_vec()] {
        for &pad in &order {
            let mut rig = Rig::ordered(&order);
            rig.to(20_000, 1 << pad);
            rig.to(40_000, 1 << pad);
            near(rig.state().position.unwrap(), pad as f32 / 11.0);
            assert!(rig.events.is_empty());
        }
    }
    let mut rig = Rig::ordered(WORKING);
    let chord = mask(&[3, 5, 6, 7, 9]);
    rig.to(20_000, chord);
    rig.to(40_000, chord);
    near(rig.state().position.unwrap(), 6.0 / 11.0);
    assert!(!rig.state().ambiguous);
}

#[test]
fn missing_electrode_crossings_keep_one_contact_and_physical_distance_without_taps() {
    let mut rig = Rig::ordered(WORKING);
    rig.to(20_000, 1 << 3);
    rig.to(40_000, 1 << 3);
    let contact = rig.state().contact_id;
    for (index, pad) in [5, 7, 9].into_iter().enumerate() {
        let at = 60_000 + index as u64 * 40_000;
        rig.to(at, 1 << pad);
        assert_eq!(rig.state().contact_id, contact);
        rig.to(at + 20_000, 1 << pad);
        assert_eq!(rig.state().contact_id, contact);
        assert!(!rig.state().ambiguous);
        near(rig.state().position.unwrap(), pad as f32 / 11.0);
        assert_eq!(rig.state().direction, Some(StripDirection::TowardEnd));
    }
    near(rig.state().distance, 6.0 / 11.0);
    rig.to(3_000_000, 1 << 9);
    assert_eq!(rig.count(StripEffectKind::Touch), 0);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    assert_eq!(rig.count(StripEffectKind::Slide), 3);
    assert!(rig.events.iter().all(|event| event.released_us.is_none()));
}

#[test]
fn qualified_chord_crosses_known_missing_pads_without_retouch_release_or_sleep() {
    let mut rig = Rig::ordered(WORKING);
    rig.to(20_000, mask(&[1, 2, 3]));
    rig.to(40_000, rig.mask);
    let contact = rig.state().contact_id;
    for (index, pads) in [[2, 3, 5], [3, 5, 6], [5, 6, 7], [6, 7, 9], [7, 9, 10]]
        .into_iter()
        .enumerate()
    {
        let at = 60_000 + index as u64 * 40_000;
        rig.to(at, mask(&pads));
        rig.to(at + 20_000, rig.mask);
        assert_eq!(rig.state().contact_id, contact);
        assert!(!rig.state().ambiguous);
        assert!(rig.state().moving);
    }
    rig.to(3_000_000, rig.mask);
    assert_eq!(rig.count(StripEffectKind::Touch), 1);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    assert_eq!(rig.count(StripEffectKind::Stroke), 0);
    assert_eq!(rig.count(StripEffectKind::Slide), 5);
    assert!(
        rig.events
            .iter()
            .all(|event| event.contact_id == contact.unwrap())
    );
    assert!(rig.events.iter().all(|event| event.released_us.is_none()));
}

#[test]
fn reversed_sparse_order_retains_missing_widths_and_reverses_slide_direction() {
    let order: Vec<_> = WORKING.into_iter().rev().collect();
    let mut rig = Rig::ordered(order);
    rig.to(20_000, 1 << 3);
    rig.to(40_000, 1 << 3);
    near(rig.state().position.unwrap(), 8.0 / 11.0);
    let contact = rig.state().contact_id;
    for (index, pad) in [5, 7, 9].into_iter().enumerate() {
        let at = 60_000 + index as u64 * 40_000;
        rig.to(at, 1 << pad);
        rig.to(at + 20_000, 1 << pad);
        near(rig.state().position.unwrap(), (11 - pad) as f32 / 11.0);
        assert_eq!(rig.state().direction, Some(StripDirection::TowardStart));
        assert_eq!(rig.state().contact_id, contact);
    }
    near(rig.state().distance, 6.0 / 11.0);
    assert_eq!(rig.count(StripEffectKind::Slide), 3);
}

#[test]
fn sparse_topology_skips_exclusions_but_inactive_working_electrodes_are_ambiguous() {
    let chord = mask(&[1, 3, 5]);
    let mut sparse = Rig::ordered(WORKING);
    sparse.to(20_000, chord);
    sparse.to(3_000_000, chord);
    assert!(sparse.state().ambiguous);
    assert_eq!(sparse.state().position, None);
    assert!(sparse.events.is_empty());

    // Full-set compatibility deliberately retains the original one-hole
    // tolerance. Explicit working sets are stricter about a live pad's gap.
    let mut complete = Rig::new();
    complete.to(20_000, chord);
    complete.to(40_000, chord);
    assert!(!complete.state().ambiguous);
    assert_eq!(complete.count(StripEffectKind::Touch), 1);
}

#[test]
fn three_pad_sparse_set_qualifies_only_all_three_selected_electrodes() {
    let selected = [0, 5, 11];
    let excluded = mask(&[1, 2, 3, 4, 6, 7, 8, 9, 10]);
    for (active, touches, sleeps) in [(mask(&[0, 5]), 0, 0), (mask(&selected), 1, 1)] {
        let mut rig = Rig::ordered(selected);
        rig.to(20_000, active | excluded);
        rig.to(2_020_000, active | excluded);
        assert_eq!(rig.state().active_mask, active);
        assert!(!rig.state().ambiguous);
        assert_eq!(rig.count(StripEffectKind::Touch), touches);
        assert_eq!(rig.count(StripEffectKind::Sleep), sleeps);
    }
}

#[test]
fn excluded_electrode_noise_cannot_change_selected_geometry_events_or_readiness() {
    let mut quiet = StripRecognizer::new(WORKING).unwrap();
    let mut noisy = StripRecognizer::new(WORKING).unwrap();
    let excluded = (1 << 4) | (1 << 8);
    for at in (0..=2_100_000).step_by(5_000) {
        let selected = if at < 20_000 { 0 } else { 7 };
        let noise = if at % 15_000 == 0 { excluded } else { 1 << 4 };
        let expected = quiet.update(at, at, Some(selected)).unwrap();
        let actual = noisy.update(at, at, Some(selected | noise)).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual.state.active_mask & excluded, 0);
    }
}

#[test]
fn excluded_only_input_releases_without_current_presence_or_extending_a_hold() {
    let excluded = (1 << 4) | (1 << 8);
    let mut rig = Rig::ordered(WORKING);
    rig.to(20_000, 7);
    rig.to(1_990_000, 7);
    let original = rig.state().contact_id;
    rig.to(2_000_000, excluded);
    assert_eq!(rig.state().active_mask, 0);
    assert_eq!(rig.state().position, None);
    assert_eq!(rig.state().held_us, 1_980_000);
    rig.to(2_029_999, excluded);
    assert_eq!(rig.state().held_us, 1_980_000);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    rig.sample(2_030_000, excluded);
    assert_eq!(rig.state().contact_id, None);
    rig.to(2_040_000, 7 | excluded);
    rig.to(2_060_000, 7 | excluded);
    assert_ne!(rig.state().contact_id, original);
    assert_eq!(rig.count(StripEffectKind::Touch), 2);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
}

#[test]
fn selected_held_at_startup_stays_unarmed_until_selected_release_despite_excluded_noise() {
    let mut strip = StripRecognizer::new(WORKING).unwrap();
    for at in (0..=2_100_000).step_by(20_000) {
        let out = strip.update(at, at, Some(7 | (1 << 4))).unwrap();
        assert!(!out.state.ready);
        assert!(out.effects.is_empty());
    }
    strip.update(2_120_000, 2_120_000, Some(1 << 4)).unwrap();
    let out = strip.update(2_130_000, 2_130_000, Some(1 << 4)).unwrap();
    assert!(out.state.ready);
    assert_eq!(out.state.active_mask, 0);
    assert_eq!(out.state.contact_id, None);
    assert!(out.effects.is_empty());
}

#[test]
fn sparse_raw_status_faults_and_unknown_acquisitions_disarm_instead_of_becoming_release() {
    for (now, observed, sample, error) in [
        (90_000, 90_000, None, StripError::UnknownSample),
        (90_000, 90_000, Some(1 << 12), StripError::InvalidMask),
        (
            90_000,
            90_000,
            Some(0x8000 | (1 << 4)),
            StripError::InvalidMask,
        ),
        (180_000, 180_000, Some(7), StripError::SampleGap),
        (190_000, 90_000, Some(7), StripError::StaleSample),
    ] {
        let mut rig = Rig::ordered(WORKING);
        rig.to(20_000, 7);
        rig.to(80_000, 7);
        assert_eq!(rig.strip.update(now, observed, sample), Err(error));
        assert!(!rig.strip.state(now).ready);
        assert_eq!(rig.strip.state(now).contact_id, None);
        for at in (now + 20_000..=now + 2_100_000).step_by(20_000) {
            let out = rig.strip.update(at, at, Some(7)).unwrap();
            assert!(!out.state.ready);
            assert!(out.effects.is_empty());
        }
    }
}

#[test]
fn known_hole_does_not_bridge_zero_acquisitions_past_the_existing_release_grace() {
    let mut rig = Rig::ordered(WORKING);
    rig.to(20_000, 1 << 3);
    rig.to(40_000, 1 << 3);
    let contact = rig.state().contact_id;
    rig.to(60_000, 0);
    assert_eq!(rig.state().active_mask, 0);
    assert_eq!(rig.state().position, None);
    rig.to(60_000 + RELEASE_US, 1 << 5);
    rig.to(110_000, 1 << 5);
    assert_ne!(rig.state().contact_id, contact);
    assert_eq!(rig.state().distance, 0.0);
    assert!(rig.events.is_empty());
}

#[test]
fn order_is_a_complete_permutation_and_preserves_weak_electrode_positions() {
    for order in [[0; 12], [12; 12]] {
        assert!(matches!(
            StripRecognizer::new(order),
            Err(StripError::InvalidOrder)
        ));
    }
    for pad in 0..12 {
        let mut rig = Rig::new();
        rig.to(20_000, 1 << pad);
        rig.to(40_000, 1 << pad);
        near(rig.state().position.unwrap(), pad as f32 / 11.0);
        assert!(rig.events.is_empty());
    }
}

#[test]
fn reversed_and_non_numeric_orders_change_position_and_direction() {
    let order = [11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
    let mut rig = Rig::ordered(order);
    rig.to(20_000, 1);
    rig.to(40_000, 1);
    near(rig.state().position.unwrap(), 1.0);
    rig.to(60_000, 2);
    rig.to(80_000, 2);
    assert_eq!(rig.state().direction, Some(StripDirection::TowardStart));
    near(rig.state().position.unwrap(), 10.0 / 11.0);
    let mut custom = Rig::ordered([2, 0, 1, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    custom.to(20_000, 4);
    custom.to(40_000, 4);
    near(custom.state().position.unwrap(), 0.0);
}

#[test]
fn qualified_contact_emits_touch_once_and_nothing_new_on_release() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(34_999, 7);
    assert!(rig.events.is_empty());
    rig.sample(35_000, 7);
    assert_eq!(rig.count(StripEffectKind::Touch), 1);
    assert_eq!(rig.events[0].started_us, 20_000);
    assert_eq!(rig.events[0].observed_us, 35_000);
    assert_eq!(rig.events[0].released_us, None);
    rig.to(100_000, 0);
    assert!(rig.state().position.is_none());
    rig.to(500_000, 0);
    assert_eq!(rig.events.len(), 1);
    assert_eq!(rig.state().contact_id, None);
}

#[test]
fn qualification_closes_before_an_irregular_falling_edge() {
    for (last, expected) in [(34_999, 0), (35_000, 1)] {
        let mut rig = Rig::new();
        rig.sample(20_000, 7);
        rig.sample(25_000, 7);
        rig.sample(30_000, 7);
        rig.sample(last, 3);
        assert_eq!(rig.count(StripEffectKind::Touch), expected);
    }
}

#[test]
fn brief_qualified_touch_on_zero_has_explicit_release_and_no_current_position() {
    let mut rig = Rig::new();
    rig.sample(20_000, 7);
    rig.sample(25_000, 7);
    rig.sample(30_000, 7);
    rig.sample(35_000, 0);
    assert_eq!(rig.events.len(), 1);
    let event = rig.events[0];
    assert_eq!(event.kind, StripEffectKind::Touch);
    assert_eq!(event.released_us, Some(35_000));
    assert_eq!(event.active_mask, 0);
    assert_eq!(event.position, None);
    assert_eq!(rig.state().position, None);
    rig.to(35_000 + RELEASE_US, 0);
    assert_eq!(rig.events.len(), 1);
}

#[test]
fn qualifying_handoff_does_not_fabricate_a_zero_mask_touch_without_real_release() {
    let mut rig = Rig::new();
    rig.sample(20_000, 7);
    rig.sample(25_000, 7);
    rig.sample(30_000, 7);
    rig.sample(35_000, 56);
    assert!(rig.events.is_empty());
    assert_eq!(rig.state().active_mask, 0);
    rig.sample(40_000, 56);
    let touch = rig
        .events
        .iter()
        .find(|e| e.kind == StripEffectKind::Touch)
        .unwrap();
    assert_eq!(touch.active_mask, 56);
    assert_eq!(touch.released_us, None);
    assert_eq!(touch.started_us, 20_000);
}

#[test]
fn irregular_release_can_first_prove_both_contact_and_chord_debounce() {
    let mut rig = Rig::new();
    rig.sample(20_000, 7);
    rig.sample(25_000, 7);
    rig.sample(35_000, 0);
    assert_eq!(rig.events.len(), 1);
    let event = rig.events[0];
    assert_eq!(event.kind, StripEffectKind::Touch);
    assert_eq!(event.started_us, 20_000);
    assert_eq!(event.observed_us, 35_000);
    assert_eq!(event.released_us, Some(35_000));
    assert_eq!(event.active_mask, 0);
    assert_eq!(event.position, None);
    rig.to(100_000, 0);
    assert_eq!(rig.events.len(), 1);
}

#[test]
fn irregular_brief_disjoint_footprint_does_not_become_touch_on_release() {
    let mut rig = Rig::new();
    rig.sample(20_000, 3 | (1 << 11));
    rig.sample(25_000, 3 | (1 << 11));
    rig.sample(35_000, 0);
    rig.to(100_000, 0);
    assert!(rig.events.is_empty());
}

#[test]
fn separated_short_third_pad_spikes_do_not_accumulate() {
    let mut rig = Rig::new();
    rig.to(20_000, 3);
    for i in 0..20 {
        let start = 50_000 + i * 20_000;
        rig.to(start, 7);
        rig.to(start + 9_999, 7);
        rig.sample(start + 10_000, 3);
    }
    rig.to(3_000_000, 3);
    assert!(rig.events.is_empty());
}

#[test]
fn one_and_two_pads_have_position_but_cannot_touch_or_sleep() {
    for mask in [1, 3] {
        let mut rig = Rig::new();
        rig.to(20_000, mask);
        rig.to(3_000_000, mask);
        assert!(rig.state().position.is_some());
        assert!(!rig.state().sleep_sent);
        assert!(rig.events.is_empty());
    }
}

#[test]
fn sleep_emits_at_exact_observed_two_seconds_once_until_verified_repress() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(20_000 + SLEEP_US - 1, 7);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    rig.sample(20_000 + SLEEP_US, 7);
    assert_eq!(rig.count(StripEffectKind::Sleep), 1);
    assert!(rig.state().sleep_sent);
    let first = *rig.events.last().unwrap();
    assert_eq!(first.observed_us - first.started_us, SLEEP_US);
    assert_eq!(first.released_us, None);
    rig.to(6_000_000, 7);
    assert_eq!(rig.count(StripEffectKind::Sleep), 1);
    rig.to(6_010_000, 0);
    rig.to(6_050_000, 0);
    rig.to(6_060_000, 7);
    rig.to(8_060_000, 7);
    assert_eq!(rig.count(StripEffectKind::Sleep), 2);
    assert_ne!(rig.events.last().unwrap().contact_id, first.contact_id);
}

#[test]
fn release_grace_cannot_promote_a_short_hold_into_sleep() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(20_000 + SLEEP_US - 1, 0);
    rig.to(20_000 + SLEEP_US + RELEASE_US, 0);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    assert_eq!(rig.events.len(), 1);
}

#[test]
fn delayed_receipt_and_ticks_cannot_advance_observed_hold() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(2_019_998, 7);
    let update = rig.strip.update(2_109_999, 2_019_999, Some(7)).unwrap();
    assert!(update.effects.is_empty());
    assert_eq!(update.state.held_us, SLEEP_US - 1);
    assert_eq!(rig.strip.state(2_119_998).held_us, SLEEP_US - 1);
    assert!(!rig.strip.state(2_119_999).ready);
    let update = rig.strip.update(2_110_000, 2_020_000, Some(7)).unwrap();
    assert_eq!(update.effects.len(), 1);
    assert_eq!(update.effects[0].kind, StripEffectKind::Sleep);
    assert_eq!(update.effects[0].observed_us, 2_020_000);
}

#[test]
fn startup_held_requires_full_release_before_any_contact_or_action() {
    let mut strip = StripRecognizer::new(ORDER).unwrap();
    for now in (0..=3_000_000).step_by(20_000) {
        let out = strip.update(now, now, Some(7)).unwrap();
        assert!(!out.state.ready);
        assert!(out.effects.is_empty());
        assert!(out.state.contact_id.is_none());
    }
    strip.update(3_020_000, 3_020_000, Some(0)).unwrap();
    assert!(!strip.state(3_020_000).ready);
    strip.update(3_030_000, 3_030_000, Some(0)).unwrap();
    assert!(strip.state(3_030_000).ready);
}

#[test]
fn short_zero_chatter_keeps_one_contact_and_cannot_rearm_sleep() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(2_100_000, 7);
    let id = rig.state().contact_id;
    rig.to(2_110_000, 0);
    rig.to(2_129_999, 7);
    rig.to(4_500_000, 7);
    assert_eq!(rig.state().contact_id, id);
    assert_eq!(rig.count(StripEffectKind::Touch), 1);
    assert_eq!(rig.count(StripEffectKind::Sleep), 1);
}

#[test]
fn exact_release_grace_then_press_is_a_new_contact() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(60_000, 0);
    rig.to(60_000 + RELEASE_US, 7);
    rig.to(120_000, 7);
    assert_eq!(rig.count(StripEffectKind::Touch), 2);
    assert_ne!(rig.events[0].contact_id, rig.events[1].contact_id);
}

#[test]
fn staggered_lift_and_palm_growth_do_not_create_slide_or_new_touch() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(60_000, 31);
    rig.to(100_000, 127);
    rig.to(140_000, 31);
    rig.to(180_000, 7);
    rig.to(220_000, 3);
    rig.to(260_000, 1);
    rig.to(300_000, 0);
    rig.to(350_000, 0);
    assert_eq!(rig.count(StripEffectKind::Touch), 1);
    assert_eq!(rig.count(StripEffectKind::Slide), 0);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
}

#[test]
fn recruitment_then_original_departure_does_not_slide_but_consumes_sleep() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(100_000, 31);
    rig.to(200_000, 24);
    rig.to(3_000_000, 24);
    assert_eq!(rig.count(StripEffectKind::Slide), 0);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
}

#[test]
fn subthreshold_travel_cannot_creep_into_sleep() {
    let mut rig = Rig::new();
    rig.to(20_000, 0x07df);
    rig.to(100_000, 0x07ef);
    rig.to(3_000_000, 0x07ef);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    assert_eq!(rig.count(StripEffectKind::Slide), 0);
    assert!(rig.state().position.is_some());
}

#[test]
fn raw_movement_at_sleep_boundary_suppresses_sleep_before_new_pad_filter_settles() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(20_000 + SLEEP_US - 1, 7);
    rig.sample(20_000 + SLEEP_US, 14);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    rig.to(3_000_000, 14);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    assert!(rig.state().moving);
}

#[test]
fn single_pad_travel_produces_continuous_positions_without_tap_or_sleep() {
    let mut rig = Rig::new();
    for pad in 0..12 {
        rig.to(20_000 + pad * 100_000, 1 << pad);
        rig.to(40_000 + pad * 100_000, 1 << pad);
        near(rig.state().position.unwrap(), pad as f32 / 11.0);
    }
    assert_eq!(rig.count(StripEffectKind::Touch), 0);
    assert!(rig.count(StripEffectKind::Slide) >= 11);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    rig.to(3_000_000, 1 << 11);
    assert!(rig.state().moving);
    rig.to(3_010_000, 0);
    let count = rig.events.len();
    rig.to(3_100_000, 0);
    assert_eq!(rig.events.len(), count);
}

#[test]
fn reversal_changes_direction_without_new_contact_and_pause_keeps_consumption() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(100_000, 14);
    rig.to(120_000, 14);
    assert_eq!(rig.state().direction, Some(StripDirection::TowardEnd));
    let id = rig.state().contact_id;
    rig.to(160_000, 28);
    rig.to(200_000, 14);
    rig.to(220_000, 14);
    assert_eq!(rig.state().direction, Some(StripDirection::TowardStart));
    let distance = rig.state().distance;
    rig.to(220_000 + SPEED_WINDOW_US, 14);
    assert_eq!(rig.state().speed, 0.0);
    assert_eq!(rig.state().direction, None);
    assert_eq!(rig.state().distance, distance);
    assert_eq!(rig.state().contact_id, id);
    rig.to(3_000_000, 14);
    assert_eq!(rig.count(StripEffectKind::Touch), 1);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    assert!(rig.state().moving);
}

#[test]
fn distance_and_speed_are_normalized_sampled_units_not_electrodes_or_millimetres() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(100_000, 14);
    rig.sample(105_000, 14);
    // First accepted centroid at 30 ms, one full-rank displacement confirmed
    // at 105 ms: 1/11 strip length over the observed 75 ms interval.
    near(rig.state().distance, 1.0 / 11.0);
    near(rig.state().speed, (1.0 / 11.0) / 0.075);
    near(rig.state().position.unwrap(), 2.0 / 11.0);
    assert_eq!(rig.state().direction, Some(StripDirection::TowardEnd));
}

#[test]
fn disjoint_second_finger_invalidates_geometry_until_whole_release() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(100_000, 7 | (1 << 11));
    rig.to(120_000, 7 | (1 << 11));
    assert!(rig.state().ambiguous);
    assert_eq!(rig.state().position, None);
    rig.to(200_000, 7);
    rig.to(3_000_000, 7);
    assert!(rig.state().ambiguous);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    assert_eq!(rig.count(StripEffectKind::Slide), 0);
    rig.to(3_010_000, 0);
    rig.to(3_050_000, 0);
    assert!(!rig.state().ambiguous);
}

#[test]
fn missing_four_and_eight_remain_physical_holes_without_invalidating_trace_chord() {
    let mut rig = Rig::new();
    rig.to(20_000, (1 << 3) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 9));
    rig.to(80_000, rig.mask);
    assert!(!rig.state().ambiguous);
    near(rig.state().position.unwrap(), 6.0 / 11.0);
    assert_eq!(rig.count(StripEffectKind::Touch), 1);
}

#[test]
fn each_fault_disarms_without_a_release_sleep_or_new_contact() {
    let cases = [
        (90_000, 90_000, None, StripError::UnknownSample),
        (90_000, 90_000, Some(0x8000), StripError::InvalidMask),
        (79_999, 79_999, Some(0), StripError::ClockBackwards),
        (90_000, 80_000, Some(0), StripError::ObservationOutOfOrder),
        (90_000, 90_001, Some(0), StripError::FutureSample),
        (190_000, 90_000, Some(0), StripError::StaleSample),
        (180_000, 180_000, Some(0), StripError::SampleGap),
    ];
    for (now, observed, mask, expected) in cases {
        let mut rig = Rig::new();
        rig.to(20_000, 7);
        rig.to(80_000, 7);
        assert_eq!(rig.strip.update(now, observed, mask), Err(expected));
        let state = rig.strip.state(now.max(80_000));
        assert!(!state.ready);
        assert_eq!(state.position, None);
        assert_eq!(state.contact_id, None);
        assert_eq!(rig.events.len(), 1);
    }
}

#[test]
fn reset_keeps_timestamp_high_water_and_requires_release_before_another_sleep() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(2_100_000, 7);
    rig.strip.reset();
    assert_eq!(
        rig.strip.update(2_100_000, 2_100_000, Some(0)),
        Err(StripError::ObservationOutOfOrder)
    );
    for at in (2_120_000..=4_120_000).step_by(20_000) {
        let out = rig.strip.update(at, at, Some(7)).unwrap();
        assert!(!out.state.ready);
        assert!(out.effects.is_empty());
    }
    rig.strip.update(4_130_000, 4_130_000, Some(0)).unwrap();
    rig.strip.update(4_140_000, 4_140_000, Some(0)).unwrap();
    assert!(rig.strip.state(4_140_000).ready);
}

#[test]
fn freshness_and_gap_use_the_same_exclusive_hundred_millisecond_boundary() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(80_000, 7);
    assert!(rig.strip.state(80_000 + FRESH_US - 1).ready);
    assert!(!rig.strip.state(80_000 + FRESH_US).ready);
    assert_eq!(
        rig.strip
            .update(80_000 + FRESH_US, 80_000 + FRESH_US, Some(7)),
        Err(StripError::SampleGap)
    );
}

#[test]
fn long_reversing_contact_faults_at_bounded_distance_instead_of_saturating_silently() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(40_000, 7);
    let mut now = 40_000;
    let mut failed = false;
    for i in 0..2_000 {
        let mask = if i % 2 == 0 { 0xe00 } else { 7 };
        for _ in 0..2 {
            now += 10_000;
            match rig.strip.update(now, now, Some(mask)) {
                Ok(out) => assert!(out.effects.len() <= MAX_EFFECTS),
                Err(StripError::MotionBound) => {
                    failed = true;
                    break;
                }
                Err(error) => panic!("unexpected {error}"),
            }
        }
        if failed {
            break;
        }
    }
    assert!(failed);
    assert!(!rig.strip.state(now).ready);
}

#[test]
fn slow_sampled_travel_keeps_position_and_emits_one_stroke_after_motion_span() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    for step in 1..=6 {
        rig.to(100_000 + step * 700_000, 7 << step);
        rig.to(120_000 + step * 700_000, 7 << step);
        near(rig.state().position.unwrap(), (step + 1) as f32 / 11.0);
    }
    assert_eq!(rig.count(StripEffectKind::Touch), 1);
    assert_eq!(rig.count(StripEffectKind::Stroke), 1);
    assert_eq!(rig.count(StripEffectKind::Sleep), 0);
    assert!(rig.count(StripEffectKind::Slide) >= 6);
    let stroke = rig
        .events
        .iter()
        .find(|e| e.kind == StripEffectKind::Stroke)
        .unwrap();
    assert!(stroke.observed_us >= 1_500_000);
}

#[test]
fn initial_long_hold_is_excluded_from_stroke_speed_and_pause_alone_cannot_stroke() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(3_000_000, 7);
    assert_eq!(rig.count(StripEffectKind::Sleep), 1);
    for step in 1..=8 {
        rig.to(3_000_000 + step * 40_000, 7 << step);
        rig.to(3_010_000 + step * 40_000, 7 << step);
    }
    assert_eq!(rig.count(StripEffectKind::Stroke), 0);
    rig.to(12_000_000, rig.mask);
    assert_eq!(rig.count(StripEffectKind::Stroke), 0);
    assert_eq!(rig.count(StripEffectKind::Sleep), 1);
    // Further travel after a pause can satisfy the documented pattern
    // heuristic; it still does not prove continuous physical finger speed.
    rig.to(12_020_000, 7 << 7);
    rig.to(12_040_000, 7 << 7);
    assert_eq!(rig.count(StripEffectKind::Stroke), 1);
}

#[test]
fn direction_hysteresis_ignores_sub_half_rank_retreat_without_losing_position() {
    let mut rig = Rig::new();
    rig.to(20_000, 0x001f);
    rig.to(100_000, 0x003e);
    rig.to(120_000, 0x003e);
    assert_eq!(rig.state().direction, Some(StripDirection::TowardEnd));
    let before = rig.state().position.unwrap();
    // [1,2,3,4,5] -> [0,2,3,4,5] retreats one fifth of a rank.
    rig.to(140_000, 0x003d);
    rig.to(160_000, 0x003d);
    assert!(rig.state().position.unwrap() < before);
    assert_eq!(rig.state().direction, Some(StripDirection::TowardEnd));
    rig.to(180_000, 0x001f);
    rig.to(200_000, 0x001f);
    assert_eq!(rig.state().direction, Some(StripDirection::TowardStart));
}

#[test]
fn implausibly_fast_sampled_motion_faults_instead_of_publishing_unbounded_speed() {
    let mut rig = Rig::new();
    rig.to(20_000, 7);
    rig.to(100_000, 14);
    rig.to(120_000, 14);
    rig.to(200_000, 0x0fff);
    rig.to(240_000, 0x0fff);
    for at in 240_001..=240_200 {
        rig.sample(at, 0x0fff);
    }
    // The retiring pads still need their full 5 ms filter. Dense acquisitions
    // deliberately exhaust the bounded speed window before that filtered edge.
    for at in 240_201..245_201 {
        let out = rig.strip.update(at, at, Some(1 << 11)).unwrap();
        assert!(out.effects.is_empty());
    }
    assert_eq!(
        rig.strip.update(245_201, 245_201, Some(1 << 11)),
        Err(StripError::MotionBound)
    );
    assert!(!rig.strip.state(245_201).ready);
}

#[test]
fn sampled_short_pulse_matrix_rejects_noise_and_does_not_delay_first_touch_for_a_double() {
    for phase in [50, 2_550, 5_050, 7_550] {
        for width in [10_000, 25_000, 30_000, 40_000, 50_000, 80_000] {
            let mut strip = StripRecognizer::new(ORDER).unwrap();
            let mut effects = Vec::new();
            for at in (phase..=500_000).step_by(10_000) {
                let mask = if (100_000..100_000 + width).contains(&at) {
                    7
                } else {
                    0
                };
                effects.extend(strip.update(at, at, Some(mask)).unwrap().effects);
            }
            let expected = usize::from(width >= 25_000);
            assert_eq!(effects.len(), expected, "phase {phase}, width {width}");
            assert!(effects.iter().all(|e| e.kind == StripEffectKind::Touch));
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceRow {
    at_us: u64,
    mask: u16,
    provenance: String,
    source_wall: Option<String>,
    pads: Option<Vec<u8>>,
    source_line: Option<String>,
}

fn rows(raw: &str) -> Vec<TraceRow> {
    raw.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn issue711_reconstruction_preserves_every_original_row_and_labels_all_fill_frames() {
    let changes = rows(include_str!("../../fixtures/strip-711-changes.jsonl"));
    let full = rows(include_str!(
        "../../fixtures/strip-711-reconstruction.jsonl"
    ));
    assert_eq!(changes.len(), 57);
    assert_eq!(full.len(), 1052);
    assert_eq!(
        full.iter()
            .filter(|r| r.provenance == "authored_constant")
            .count(),
        995
    );
    for source in &changes {
        assert_eq!(source.provenance, "recorded_change");
        let matching: Vec<_> = full.iter().filter(|r| r.at_us == source.at_us).collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].mask, source.mask);
        assert_eq!(matching[0].source_wall, source.source_wall);
        assert_eq!(matching[0].source_line, source.source_line);
        let mask = source
            .pads
            .as_ref()
            .unwrap()
            .iter()
            .fold(0u16, |m, p| m | (1 << p));
        assert_eq!(mask, source.mask);
    }
    let mut last_mask = 0;
    for row in &full {
        if row.provenance == "authored_constant" {
            assert_eq!(row.mask, last_mask);
            assert!(row.source_wall.is_none() && row.source_line.is_none() && row.pads.is_none());
        } else {
            assert_eq!(row.provenance, "recorded_change");
            last_mask = row.mask;
        }
    }
    assert!(
        full.windows(2)
            .all(|w| w[0].at_us < w[1].at_us && w[1].at_us - w[0].at_us <= 20_000)
    );
    assert_eq!(full.last().unwrap().at_us, 19_948_000);
}

#[test]
fn issue711_changed_rows_alone_cannot_prove_continuous_touch_hold_or_slide() {
    let mut strip = StripRecognizer::new(ORDER).unwrap();
    let mut faults = Vec::new();
    for row in rows(include_str!("../../fixtures/strip-711-changes.jsonl")) {
        match strip.update(row.at_us, row.at_us, Some(row.mask)) {
            Ok(out) => assert!(out.effects.is_empty()),
            Err(error) => faults.push(error),
        }
    }
    assert_eq!(faults, vec![StripError::SampleGap; 8]);
    assert!(!strip.state(19_908_000).ready);
}

#[test]
fn issue711_authored_reconstruction_is_one_hold_with_reversing_slides_not_multiple_taps() {
    let mut strip = StripRecognizer::new(ORDER).unwrap();
    let mut effects = Vec::new();
    let mut directions = Vec::new();
    let mut positions = Vec::new();
    for row in rows(include_str!(
        "../../fixtures/strip-711-reconstruction.jsonl"
    )) {
        let out = strip.update(row.at_us, row.at_us, Some(row.mask)).unwrap();
        assert!(!out.state.ambiguous);
        assert!(out.effects.len() <= MAX_EFFECTS);
        if let Some(position) = out.state.position {
            positions.push(position);
        }
        for effect in &out.effects {
            if effect.kind == StripEffectKind::Slide
                && directions.last() != effect.direction.as_ref()
            {
                directions.push(effect.direction.unwrap());
            }
        }
        effects.extend(out.effects);
    }
    let of_kind = |kind| effects.iter().filter(|event| event.kind == kind).count();
    assert_eq!(of_kind(StripEffectKind::Touch), 1);
    assert_eq!(of_kind(StripEffectKind::Sleep), 1);
    assert_eq!(of_kind(StripEffectKind::Slide), 52);
    assert_eq!(of_kind(StripEffectKind::Stroke), 0);
    assert!(effects.iter().all(|event| event.contact_id == 1));
    assert_eq!(
        directions,
        vec![
            StripDirection::TowardStart,
            StripDirection::TowardEnd,
            StripDirection::TowardStart,
            StripDirection::TowardEnd
        ]
    );
    assert!(positions.iter().copied().fold(1.0f32, f32::min) < 0.1);
    assert!(positions.iter().copied().fold(0.0f32, f32::max) > 0.95);
    assert!(strip.state(19_948_000).ready);
    assert_eq!(strip.state(19_948_000).contact_id, None);
}

#[test]
fn issue711_reconstruction_has_identical_geometry_and_effects_with_explicit_working_pads() {
    let mut complete = StripRecognizer::new(ORDER).unwrap();
    let mut sparse = StripRecognizer::new(WORKING).unwrap();
    for row in rows(include_str!(
        "../../fixtures/strip-711-reconstruction.jsonl"
    )) {
        let expected = complete
            .update(row.at_us, row.at_us, Some(row.mask))
            .unwrap();
        let actual = sparse.update(row.at_us, row.at_us, Some(row.mask)).unwrap();
        assert_eq!(actual, expected, "acquisition at {} us", row.at_us);
    }
}

#[test]
fn sparse_authored_first_and_repeated_touch_keep_the_existing_sampling_bounds() {
    for phase in [0, 5_000, 10_000, 15_000, 19_999] {
        let mut strip = StripRecognizer::new(WORKING).unwrap();
        let mut positions = [None, None];
        let mut touches = [None, None];
        for observed in (phase..=600_000).step_by(20_000) {
            let selected = if (100_000..300_000).contains(&observed) || observed >= 500_000 {
                mask(&[3, 5, 6])
            } else {
                0
            };
            let out = strip
                .update(observed + 1_700, observed, Some(selected | (1 << 4)))
                .unwrap();
            let index = usize::from(observed >= 500_000);
            if out.state.position.is_some() {
                positions[index].get_or_insert(observed + 1_700);
            }
            if out
                .effects
                .iter()
                .any(|effect| effect.kind == StripEffectKind::Touch)
            {
                touches[index].get_or_insert(observed + 1_700);
            }
        }
        for (index, press) in [100_000, 500_000].into_iter().enumerate() {
            // Authored 20 ms acquisitions, five phases and 1.7 ms transport,
            // including excluded-pad noise. This does not measure a lamp.
            assert!(positions[index].unwrap() - press <= 41_699);
            assert!(touches[index].unwrap() - press <= 61_699);
        }
    }
}

#[test]
fn authored_twenty_millisecond_sampling_bounds_position_and_touch_dispatch() {
    for phase in [0, 5_000, 10_000, 15_000, 19_999] {
        let mut strip = StripRecognizer::new(ORDER).unwrap();
        let mut positions = [None, None];
        let mut touches = [None, None];
        for observed in (phase..=600_000).step_by(20_000) {
            let mask = if (100_000..300_000).contains(&observed) || observed >= 500_000 {
                7
            } else {
                0
            };
            let out = strip
                .update(observed + 1_700, observed, Some(mask))
                .unwrap();
            let index = usize::from(observed >= 500_000);
            if out.state.position.is_some() {
                positions[index].get_or_insert(observed + 1_700);
            }
            if out.effects.iter().any(|e| e.kind == StripEffectKind::Touch) {
                touches[index].get_or_insert(observed + 1_700);
            }
        }
        for (index, press) in [100_000, 500_000].into_iter().enumerate() {
            // First and repeated contact on one recognizer; five authored
            // edge-to-sample phases (0..19.999 ms), one or two 20 ms samples,
            // and an authored 1.7 ms transport. No physical timing is measured.
            assert!(positions[index].unwrap() - press <= 41_699);
            assert!(touches[index].unwrap() - press <= 61_699);
        }
    }
}
