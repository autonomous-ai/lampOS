//! Integration with Expression's public catalogue, using invented geometry.

use lamp_expression::{Behaviour, Catalogue};
use lamp_lean::{Angles, Clip, Model, OffsetFrame, Verdict};
use lamp_msgs::{BodyOffsets, Voice};

const MASKS: [[bool; 5]; 2] = [[true; 5], [true, false, false, true, true]];

fn model() -> Model {
    Model::from_json(include_str!("../../geometry.stand-in.json")).unwrap()
}

fn frames(behaviour: &Behaviour) -> Vec<OffsetFrame> {
    behaviour
        .keyframes
        .iter()
        .map(|frame| OffsetFrame {
            at_us: frame.at_us,
            offsets: frame.offsets,
        })
        .collect()
}

/// Independent catalogue composition; no call to lean's private sampler or
/// Expression's private sampler. This is a dense regression oracle, not a
/// substitute for the library's continuous conservative bound.
fn composed(
    behaviour: &Behaviour,
    posture: BodyOffsets,
    limits: BodyOffsets,
    base: Angles,
    applied: [bool; 5],
    at_us: f64,
) -> Angles {
    let pair = behaviour
        .keyframes
        .windows(2)
        .find(|pair| at_us <= pair[1].at_us as f64)
        .unwrap();
    let share = (at_us - pair[0].at_us as f64) / (pair[1].at_us - pair[0].at_us) as f64;
    let eased = 3.0 * share.powi(2) - 2.0 * share.powi(3);
    let left = pair[0].offsets.values();
    let right = pair[1].offsets.values();
    let posture = posture.values();
    let limits = limits.values();
    let base = base.radians();
    Angles::new(std::array::from_fn(|joint| {
        if !applied[joint] {
            return base[joint];
        }
        let sampled = left[joint] + (right[joint] - left[joint]) * eased;
        let total = posture[joint] + behaviour.weight * sampled;
        base[joint] + total.clamp(-limits[joint], limits[joint])
    }))
    .unwrap()
}

#[test]
fn builtin_catalogue_all_states_and_both_masks_match_independent_dense_composition() {
    let model = model();
    let catalogue = Catalogue::builtin();
    let base = Angles::from_encoder_counts([2048; 5]).unwrap();
    assert!(!catalogue.behaviours.is_empty());
    let expected_cases: usize = catalogue
        .behaviours
        .iter()
        .map(|behaviour| behaviour.states.len() * MASKS.len())
        .sum();
    assert!(!model.has_measurements());
    let mut cases = 0;
    for behaviour in &catalogue.behaviours {
        let frames = frames(behaviour);
        for &state in &behaviour.states {
            let posture = catalogue.postures.for_voice(state);
            for applied in MASKS {
                cases += 1;
                let result = model
                    .check_clip(
                        base,
                        Clip {
                            posture,
                            frames: &frames,
                            weight: behaviour.weight,
                            limits: catalogue.limits,
                            applied,
                            repeat: behaviour.repeat,
                        },
                        20_000,
                    )
                    .unwrap();
                assert_eq!(
                    result.verdict,
                    Verdict::WithinMargin,
                    "{} {state:?} {applied:?}",
                    behaviour.name
                );
                let mut dense_min = f64::INFINITY;
                for pair in behaviour.keyframes.windows(2) {
                    for index in 0..=256 {
                        let at_us = pair[0].at_us as f64
                            + (pair[1].at_us - pair[0].at_us) as f64 * f64::from(index) / 256.0;
                        let pose =
                            composed(behaviour, posture, catalogue.limits, base, applied, at_us);
                        dense_min = dense_min.min(model.check_pose(pose).clearance_m);
                    }
                }
                assert!(dense_min > 0.0);
                assert!(result.minimum_clearance_bound_m <= dense_min + 1e-12);
                // The reported worst sample's time also reproduces its exact
                // angles/COM through independently composed catalogue inputs.
                let independent = composed(
                    behaviour,
                    posture,
                    catalogue.limits,
                    base,
                    applied,
                    result.worst_sample.at_us,
                );
                for (expected, actual) in independent
                    .radians()
                    .into_iter()
                    .zip(result.worst_sample.angles.radians())
                {
                    assert!((expected - actual).abs() < 1e-12);
                }
                let independent = model.check_pose(independent);
                assert!(
                    (independent.clearance_m - result.worst_sample.check.clearance_m).abs() < 1e-12
                );
                assert!(result.evaluated_poses >= result.checked_steps);
                assert!(result.evaluated_poses <= 100_000);
            }
        }
    }
    assert_eq!(cases, expected_cases);
}

#[test]
fn current_body_mask_preserves_the_real_arm_posture_but_omits_expression_arm_offsets() {
    let model = model();
    let catalogue = Catalogue::builtin();
    let behaviour = catalogue
        .behaviours
        .iter()
        .find(|behaviour| behaviour.repeat)
        .unwrap();
    let base = Angles::new([0.0, 0.1, -0.15, 0.0, 0.0]).unwrap();
    let posture = catalogue.postures.for_voice(Voice::Speaking);
    let whole = composed(
        behaviour,
        posture,
        catalogue.limits,
        base,
        MASKS[0],
        300_000.0,
    );
    let today = composed(
        behaviour,
        posture,
        catalogue.limits,
        base,
        MASKS[1],
        300_000.0,
    );
    assert!((whole.radians()[1] - 0.104).abs() < 1e-12);
    assert!((whole.radians()[2] + 0.154).abs() < 1e-12);
    assert_eq!(today.radians()[1], 0.1);
    assert_eq!(today.radians()[2], -0.15);
    let whole_com = model.check_pose(whole).centre_of_mass_m;
    let today_com = model.check_pose(today).centre_of_mass_m;
    assert!((whole_com[0] - today_com[0]).abs() > 0.0001);
    let frames = frames(behaviour);
    let result = model
        .check_clip(
            base,
            Clip {
                posture,
                frames: &frames,
                weight: behaviour.weight,
                limits: catalogue.limits,
                applied: MASKS[1],
                repeat: behaviour.repeat,
            },
            20_000,
        )
        .unwrap();
    assert!(result.within_margin());
    assert_eq!(result.worst_sample.angles.radians()[1], 0.1);
    assert_eq!(result.worst_sample.angles.radians()[2], -0.15);
}

#[test]
fn a_catalogue_accepted_unclosed_repeat_is_rejected_by_lean_under_both_masks() {
    let mut catalogue = Catalogue::builtin();
    let repeated = catalogue
        .behaviours
        .iter_mut()
        .find(|behaviour| behaviour.repeat)
        .unwrap();
    repeated.keyframes.last_mut().unwrap().offsets.shoulder = 0.001;
    let catalogue = Catalogue::from_json(&serde_json::to_string(&catalogue).unwrap()).unwrap();
    let repeated = catalogue
        .behaviours
        .iter()
        .find(|behaviour| behaviour.repeat)
        .unwrap();
    let frames = frames(repeated);
    let model = model();
    for applied in MASKS {
        assert!(
            model
                .check_clip(
                    Angles::from_encoder_counts([2048; 5]).unwrap(),
                    Clip {
                        posture: catalogue.postures.for_voice(Voice::Speaking),
                        frames: &frames,
                        weight: repeated.weight,
                        limits: catalogue.limits,
                        applied,
                        repeat: repeated.repeat,
                    },
                    20_000,
                )
                .is_err()
        );
    }
}
