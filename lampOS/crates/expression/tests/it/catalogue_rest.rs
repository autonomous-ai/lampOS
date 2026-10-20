use lamp_expression::{Catalogue, Trigger};
use lamp_msgs::{BodyOffsets, Voice};

fn proposed() -> Catalogue {
    Catalogue::from_json(include_str!("../../rest-behaviours.json")).unwrap()
}

#[test]
fn existing_default_is_legacy_and_new_motion_needs_explicit_catalogue_selection() {
    let old = Catalogue::builtin();
    assert!(!old.resumable_voice);
    assert!(
        old.behaviours
            .iter()
            .all(|clip| clip.trigger != Trigger::Rest && clip.continue_in.is_empty())
    );
    let new = proposed();
    assert!(new.resumable_voice);
    let nod = new
        .behaviours
        .iter()
        .find(|clip| clip.trigger == Trigger::Pause)
        .unwrap();
    assert_eq!(nod.states, [Voice::Listening]);
    assert_eq!(nod.continue_in, [Voice::Thinking]);
    assert_eq!(nod.delay_us, 240_000);
    assert_eq!(nod.duration_us(), 1_200_000);
    assert_eq!(new.timing.body_lag_us, None);
    let rest = new
        .behaviours
        .iter()
        .find(|clip| clip.trigger == Trigger::Rest)
        .unwrap();
    assert_eq!(rest.delay_us, 45_000_000);
    assert_eq!(rest.cooldown_us, 90_000_000);
}

#[test]
fn a_continuation_is_not_permission_to_start_in_thinking_or_carry_into_speech() {
    let baseline = proposed();
    for states in [
        vec![Voice::Speaking],
        vec![Voice::Thinking, Voice::Thinking],
        vec![Voice::Resting],
    ] {
        let mut c = baseline.clone();
        c.behaviours
            .iter_mut()
            .find(|clip| clip.trigger == Trigger::Pause)
            .unwrap()
            .continue_in = states;
        assert!(c.check().is_err());
    }
    let mut c = baseline.clone();
    c.behaviours
        .iter_mut()
        .find(|clip| clip.trigger == Trigger::Pause)
        .unwrap()
        .states
        .push(Voice::Thinking);
    assert!(c.check().is_err());
    let mut c = baseline;
    c.behaviours
        .iter_mut()
        .find(|clip| clip.trigger == Trigger::Thinking)
        .unwrap()
        .continue_in = vec![Voice::Speaking];
    assert!(c.check().is_err());
}

#[test]
fn long_nod_must_have_a_bounded_approved_continuation() {
    let mut c = proposed();
    c.behaviours
        .iter_mut()
        .find(|clip| clip.trigger == Trigger::Pause)
        .unwrap()
        .continue_in
        .clear();
    assert!(c.check().is_err());
    let mut c = proposed();
    c.behaviours
        .iter_mut()
        .find(|clip| clip.trigger == Trigger::Pause)
        .unwrap()
        .keyframes
        .last_mut()
        .unwrap()
        .at_us = 2_000_001;
    assert!(c.check().is_err());
}

#[test]
fn rest_cannot_be_a_scan_unbounded_oscillation_or_light_pulse() {
    let baseline = proposed();
    for change in 0..13 {
        let mut c = baseline.clone();
        let clip = c
            .behaviours
            .iter_mut()
            .find(|clip| clip.trigger == Trigger::Rest)
            .unwrap();
        match change {
            0 => clip.quiet_only = false,
            1 => clip.stop_when_concentrating = false,
            2 => clip.repeat = true,
            3 => clip.hold = true,
            4 => clip.delay_us = 19_999_999,
            5 => clip.cooldown_us = 59_999_999,
            6 => clip.keyframes[1].offsets.base_yaw = 0.01,
            7 => clip.keyframes[1].offsets.shoulder = 0.01,
            8 => clip.keyframes[1].offsets.elbow = 0.01,
            9 => clip.keyframes[1].offsets.head_pitch = 0.061,
            10 => clip.keyframes[1].offsets.head_roll = -0.061,
            11 => clip.keyframes[1].brightness = 0.1,
            12 => clip.states = vec![Voice::Listening],
            _ => unreachable!(),
        }
        assert!(c.check().is_err(), "change {change}");
    }
}

#[test]
fn rest_has_zero_endpoints_and_no_more_than_one_author() {
    let mut c = proposed();
    let mut duplicate = c
        .behaviours
        .iter()
        .find(|clip| clip.trigger == Trigger::Rest)
        .unwrap()
        .clone();
    duplicate.name = "another rest".into();
    c.behaviours.push(duplicate);
    assert!(c.check().is_err());
    for end in [false, true] {
        let mut c = proposed();
        let clip = c
            .behaviours
            .iter_mut()
            .find(|clip| clip.trigger == Trigger::Rest)
            .unwrap();
        let frame = if end {
            clip.keyframes.last_mut().unwrap()
        } else {
            clip.keyframes.first_mut().unwrap()
        };
        frame.offsets = BodyOffsets {
            head_pitch: 0.01,
            ..BodyOffsets::ZERO
        };
        assert!(c.check().is_err());
    }
}

#[test]
fn a_long_rest_clip_cannot_hide_a_fast_step_between_adjacent_frames() {
    let mut c = proposed();
    let rest = c
        .behaviours
        .iter_mut()
        .find(|clip| clip.trigger == Trigger::Rest)
        .unwrap();
    rest.keyframes[1].at_us = 1;
    assert!(c.check().is_err());
    let mut c = proposed();
    let rest = c
        .behaviours
        .iter_mut()
        .find(|clip| clip.trigger == Trigger::Rest)
        .unwrap();
    // Speed alone would pass (0.045 rad/s); acceleration at the easing edge
    // is 0.18 rad/s² and must also be refused.
    rest.keyframes[1].at_us = 1_000_000;
    rest.keyframes[1].offsets.head_pitch = 0.03;
    assert!(c.check().is_err());
}
