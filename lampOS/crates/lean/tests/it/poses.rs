//! Analytic stand-ins, not measured lamp geometry or qualified hardware poses.

use lamp_lean::{Angles, Base, Geometry, Joint, Link, Measurement, Model};
use serde_json::Value;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

const EPS: f64 = 2e-12;

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < EPS,
        "expected {expected:.15}, got {actual:.15}"
    );
}

fn vector_close(actual: [f64; 3], expected: [f64; 3]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        close(actual, expected);
    }
}

fn angles(values: [f64; 5]) -> Angles {
    Angles::new(values).unwrap()
}

fn square(half_width: f64) -> Vec<[f64; 2]> {
    vec![
        [-half_width, -half_width],
        [half_width, -half_width],
        [half_width, half_width],
        [-half_width, half_width],
    ]
}

/// Five 0.2 m rods with a 0.1 kg mass at each midpoint, and a 0.5 kg
/// fixed base at the origin. Total mass is 1 kg. Physical order ends in
/// pitch then roll, deliberately different from the canonical input slots.
fn rods() -> Geometry {
    let joints = [
        Joint::BaseYaw,
        Joint::Shoulder,
        Joint::Elbow,
        Joint::HeadPitch,
        Joint::HeadRoll,
    ];
    let axes = [
        [0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
    ];
    Geometry {
        schema_version: 1,
        measurement: Measurement::StandIn,
        label: "Analytic five-rod stand-in; no lamp measurements".into(),
        footprint_m: square(0.1),
        margin_m: 0.01,
        base: Base {
            mass_kg: 0.5,
            com_m: [0.0; 3],
        },
        links: std::array::from_fn(|index| Link {
            joint: joints[index],
            origin_m: [0.0, 0.0, if index == 0 { 0.0 } else { 0.2 }],
            rpy_rad: [0.0; 3],
            axis: axes[index],
            angle_zero_rad: 0.0,
            mass_kg: 0.1,
            com_m: [0.0, 0.0, 0.1],
        }),
    }
}

fn only_fixed_base(com_m: [f64; 3]) -> Geometry {
    let mut geometry = rods();
    geometry.base = Base {
        mass_kg: 1.0,
        com_m,
    };
    for link in &mut geometry.links {
        link.mass_kg = 0.0;
    }
    geometry
}

#[test]
fn encoder_conversion_preserves_slots_and_uses_4096_counts_per_turn() {
    let pose = Angles::from_encoder_counts([0, 1024, 2048, 3072, 4095]).unwrap();
    let expected = [-PI, -FRAC_PI_2, 0.0, FRAC_PI_2, PI - TAU / 4096.0];
    for (actual, expected) in pose.radians().into_iter().zip(expected) {
        close(actual, expected);
    }
    let model = Model::new(rods()).unwrap();
    let encoder_pose = Angles::from_encoder_counts([2048, 3072, 2048, 2048, 2048]).unwrap();
    vector_close(
        model.check_pose(encoder_pose).centre_of_mass_m,
        [0.16, 0.0, 0.09],
    );
}

#[test]
fn encoder_counts_outside_single_turn_are_refused_in_every_slot() {
    for slot in 0..5 {
        for invalid in [4096, u16::MAX] {
            let mut counts = [2048; 5];
            counts[slot] = invalid;
            assert!(Angles::from_encoder_counts(counts).is_err());
        }
    }
}

#[test]
fn angles_refuse_nonfinite_values_degrees_and_legacy_normalized_units() {
    for slot in 0..5 {
        for invalid in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            90.0,
            -100.0,
            PI + 1e-6,
            -PI - 1e-6,
        ] {
            let mut values = [0.0; 5];
            values[slot] = invalid;
            assert!(Angles::new(values).is_err(), "slot {slot}, {invalid}");
        }
    }
    assert!(Angles::new([-PI, PI, 0.0, -FRAC_PI_2, FRAC_PI_2]).is_ok());
}

#[test]
fn standing_folded_and_level_arm_have_hand_worked_answers() {
    let model = Model::new(rods()).unwrap();
    // Standing moments: 0.1 * (0.1 + 0.3 + 0.5 + 0.7 + 0.9) = 0.25 kg m.
    // Folded COMs are (0,0,.1), (.1,0,.2), (.1,0,.2), (.1,0,.2), (.3,0,.2).
    // That inward-folded mathematical fixture is not a collision-free motion.
    let cases = [
        ([0.0; 5], [0.0, 0.0, 0.25], 0.09, true),
        (
            [0.0, FRAC_PI_2, -PI, 0.0, PI],
            [0.06, 0.0, 0.09],
            0.03,
            true,
        ),
        (
            [0.0, FRAC_PI_2, 0.0, 0.0, 0.0],
            [0.16, 0.0, 0.09],
            -0.07,
            false,
        ),
    ];
    for (pose, centre, clearance, inside) in cases {
        let result = model.check_pose(angles(pose));
        vector_close(result.centre_of_mass_m, centre);
        close(result.clearance_m, clearance);
        assert_eq!(result.within_margin(), inside);
    }
}

#[test]
fn every_joint_including_the_final_com_obeys_canonical_input_order() {
    let model = Model::new(rods()).unwrap();
    // Each individual quarter-turn changes the downstream mass moment. Yaw
    // needs an extended shoulder to distinguish its direction from standing.
    let cases = [
        (
            [FRAC_PI_2, FRAC_PI_2, 0.0, 0.0, 0.0],
            [0.0, 0.16, 0.09],
            -0.07,
        ),
        ([0.0, FRAC_PI_2, 0.0, 0.0, 0.0], [0.16, 0.0, 0.09], -0.07),
        ([0.0, 0.0, FRAC_PI_2, 0.0, 0.0], [0.09, 0.0, 0.16], 0.0),
        ([0.0, 0.0, 0.0, FRAC_PI_2, 0.0], [0.0, -0.01, 0.24], 0.08),
        ([0.0, 0.0, 0.0, 0.0, FRAC_PI_2], [0.04, 0.0, 0.21], 0.05),
    ];
    for (pose, centre, clearance) in cases {
        let result = model.check_pose(angles(pose));
        vector_close(result.centre_of_mass_m, centre);
        close(result.clearance_m, clearance);
    }
    let boundary = model.check_pose(angles([0.0, 0.0, FRAC_PI_2, 0.0, 0.0]));
    assert!(!boundary.within_margin());
}

#[test]
fn physical_pitch_then_roll_order_is_independent_of_canonical_pose_order() {
    let model = Model::new(rods()).unwrap();
    // All five quarter-turns put part COMs at (0,0,.1), (0,.1,.2),
    // (0,.2,.1), (0,.1,0), (.1,0,0). Their masses are each .1 kg;
    // total mass including the fixed base is exactly 1 kg.
    let result = model.check_pose(angles([FRAC_PI_2; 5]));
    vector_close(result.centre_of_mass_m, [0.01, 0.04, 0.04]);
    close(result.clearance_m, 0.05);
}

#[test]
fn fixed_rpy_precedes_local_joint_rotation_and_does_not_rotate_its_own_origin() {
    let mut geometry = rods();
    geometry.footprint_m = square(1.0);
    geometry.base.mass_kg = 1.0;
    for link in &mut geometry.links {
        link.origin_m = [0.0; 3];
        link.com_m = [0.0; 3];
        link.mass_kg = 1.0;
    }
    geometry.links[0].origin_m = [0.1, 0.2, 0.3];
    geometry.links[0].rpy_rad = [FRAC_PI_2; 3];
    geometry.links[4].com_m = [0.2, 0.3, 0.4];
    let model = Model::new(geometry).unwrap();
    // Rz(90) Ry(90) Rx(90) Rz(q) applied to (a,b,c):
    // q=0 -> (c,b,-a); q=90 -> (c,a,b). The translated origin
    // contributes 5*(.1,.2,.3), independently of the joint rotation.
    let at_zero = model.check_pose(angles([0.0; 5]));
    vector_close(at_zero.centre_of_mass_m, [0.9 / 6.0, 1.3 / 6.0, 1.3 / 6.0]);
    let rotated = model.check_pose(angles([FRAC_PI_2, 0.0, 0.0, 0.0, 0.0]));
    vector_close(rotated.centre_of_mass_m, [0.15, 0.2, 0.3]);
}

#[test]
fn child_origins_are_translated_in_the_rotated_parent_frame() {
    let mut geometry = only_fixed_base([0.0; 3]);
    geometry.footprint_m = square(1.0);
    for link in &mut geometry.links {
        link.origin_m = [0.0; 3];
        link.com_m = [0.0; 3];
    }
    geometry.links[0].origin_m = [0.1, 0.2, 0.3];
    geometry.links[1].origin_m = [0.4, 0.0, 0.1];
    geometry.links[4].mass_kg = 1.0;
    geometry.links[4].com_m = [0.2, 0.0, 0.0];
    let model = Model::new(geometry).unwrap();
    // End mass is (.1,.2,.3) + Rz(90)*(.4,0,.1) + Rz(90)*(.2,0,0)
    // = (.1,.8,.4), with equal base/end masses.
    vector_close(
        model
            .check_pose(angles([FRAC_PI_2, 0.0, 0.0, 0.0, 0.0]))
            .centre_of_mass_m,
        [0.05, 0.4, 0.2],
    );
}

#[test]
fn geometric_zero_is_subtracted_from_encoder_angle_once() {
    let mut geometry = rods();
    geometry.links[1].angle_zero_rad = FRAC_PI_2;
    let model = Model::new(geometry).unwrap();
    // Encoder zero minus geometric-zero reading +pi/2 means -pi/2 rotation.
    vector_close(
        model.check_pose(angles([0.0; 5])).centre_of_mass_m,
        [-0.16, 0.0, 0.09],
    );
    // At the declared encoder reading the mechanical angle is exactly zero.
    vector_close(
        model
            .check_pose(Angles::from_encoder_counts([2048, 3072, 2048, 2048, 2048]).unwrap())
            .centre_of_mass_m,
        [0.0, 0.0, 0.25],
    );
}

#[test]
fn a_negative_measured_axis_reverses_the_encoder_rotation_direction() {
    let mut geometry = rods();
    geometry.links[1].axis = [0.0, -1.0, 0.0];
    let model = Model::new(geometry).unwrap();
    vector_close(
        model
            .check_pose(angles([0.0, FRAC_PI_2, 0.0, 0.0, 0.0]))
            .centre_of_mass_m,
        [-0.16, 0.0, 0.09],
    );
}

#[test]
fn base_mass_and_its_full_local_com_contribute_to_the_total_moment() {
    let mut geometry = rods();
    geometry.base.com_m = [0.02, -0.01, 0.04];
    let model = Model::new(geometry).unwrap();
    // The .5 kg base contributes (.01,-.005,.02) kg m to a 1 kg assembly.
    let result = model.check_pose(angles([0.0; 5]));
    vector_close(result.centre_of_mass_m, [0.01, -0.005, 0.27]);
    close(result.clearance_m, 0.08);
    close(model.total_mass_kg(), 1.0);
    close(model.margin_m(), 0.01);
}

#[test]
fn scaling_every_mass_preserves_com_and_clearance() {
    let geometry = rods();
    let original = Model::new(geometry.clone()).unwrap();
    let mut scaled = geometry;
    scaled.base.mass_kg *= 10.0;
    for link in &mut scaled.links {
        link.mass_kg *= 10.0;
    }
    let scaled = Model::new(scaled).unwrap();
    close(scaled.total_mass_kg(), 10.0);
    for pose in [[0.0; 5], [FRAC_PI_2; 5], [0.4, -0.2, 0.7, -0.3, 0.5]] {
        let before = original.check_pose(angles(pose));
        let after = scaled.check_pose(angles(pose));
        vector_close(after.centre_of_mass_m, before.centre_of_mass_m);
        close(after.clearance_m, before.clearance_m);
    }
}

#[test]
fn extending_forward_from_vertical_to_horizontal_never_improves_clearance() {
    let model = Model::new(rods()).unwrap();
    let mut previous = f64::INFINITY;
    for index in 0..=32 {
        let theta = FRAC_PI_2 * f64::from(index) / 32.0;
        // .16 kg m of downstream moment swings about the shoulder; .09 kg m
        // remains at fixed height. This formula is valid on this named range.
        let result = model.check_pose(angles([0.0, theta, 0.0, 0.0, 0.0]));
        vector_close(
            result.centre_of_mass_m,
            [0.16 * theta.sin(), 0.0, 0.09 + 0.16 * theta.cos()],
        );
        close(result.clearance_m, 0.09 - 0.16 * theta.sin());
        assert!(result.clearance_m <= previous + EPS);
        previous = result.clearance_m;
    }
}

#[test]
fn asymmetric_support_distinguishes_equal_radial_lean_at_different_yaw() {
    let mut geometry = rods();
    geometry.footprint_m = vec![[-0.1, -0.05], [0.1, -0.05], [0.1, 0.05], [-0.1, 0.05]];
    let model = Model::new(geometry).unwrap();
    let along_x = model.check_pose(angles([0.0, PI / 6.0, 0.0, 0.0, 0.0]));
    let along_y = model.check_pose(angles([FRAC_PI_2, PI / 6.0, 0.0, 0.0, 0.0]));
    // The radius is .16*sin(pi/6) = .08 m. Inset half-widths are .09/.04 m.
    close(along_x.clearance_m, 0.01);
    close(along_y.clearance_m, -0.04);
    assert!(along_x.within_margin());
    assert!(!along_y.within_margin());
}

#[test]
fn shifted_support_is_not_silently_recentred_on_the_yaw_axis() {
    let mut geometry = only_fixed_base([0.2, 0.0, 0.02]);
    geometry.footprint_m = vec![[0.1, -0.1], [0.3, -0.1], [0.3, 0.1], [0.1, 0.1]];
    let inside = Model::new(geometry.clone()).unwrap();
    close(inside.check_pose(angles([0.0; 5])).clearance_m, 0.09);
    geometry.base.com_m[0] = 0.0;
    let outside = Model::new(geometry).unwrap();
    close(outside.check_pose(angles([0.0; 5])).clearance_m, -0.11);
}

#[test]
fn projected_com_uses_true_signed_euclidean_distance_to_the_inset() {
    let cases = [
        ([0.0, 0.0, 0.0], 0.09),
        ([0.09, 0.0, 0.0], 0.0),
        ([0.1, 0.0, 0.0], -0.01),
        ([0.095, 0.095, 0.0], -0.005 * 2.0_f64.sqrt()),
        (
            [0.13, 0.14, 0.0],
            -(0.04_f64.powi(2) + 0.05_f64.powi(2)).sqrt(),
        ),
    ];
    for (com, expected) in cases {
        let model = Model::new(only_fixed_base(com)).unwrap();
        let result = model.check_pose(angles([0.0; 5]));
        vector_close(result.centre_of_mass_m, com);
        close(result.clearance_m, expected);
    }
}

#[test]
fn the_numerical_reserve_does_not_accept_boundary_roundoff_as_inside() {
    for (distance, expected) in [(0.0, false), (0.5e-9, false), (2e-9, true)] {
        let model = Model::new(only_fixed_base([0.09 - distance, 0.0, 0.0])).unwrap();
        let result = model.check_pose(angles([0.0; 5]));
        close(result.clearance_m, distance);
        assert_eq!(result.within_margin(), expected);
    }
}

#[test]
fn massless_moving_links_are_allowed_but_the_fixed_base_remains_in_the_com() {
    let model = Model::new(only_fixed_base([0.03, -0.02, 0.01])).unwrap();
    let result = model.check_pose(angles([FRAC_PI_2; 5]));
    close(model.total_mass_kg(), 1.0);
    vector_close(result.centre_of_mass_m, [0.03, -0.02, 0.01]);
    close(result.clearance_m, 0.06);
}

#[test]
fn measurement_provenance_is_preserved_without_changing_the_arithmetic() {
    let geometry = rods();
    let stand_in = Model::new(geometry.clone()).unwrap();
    assert!(!stand_in.has_measurements());
    let before = stand_in.check_pose(angles([0.0; 5]));
    assert_eq!(before.measurement, Measurement::StandIn);
    let mut tagged = geometry;
    // This only tests the metadata flag; the fixture is still invented data.
    tagged.measurement = Measurement::Measured;
    let tagged = Model::new(tagged).unwrap();
    assert!(tagged.has_measurements());
    let after = tagged.check_pose(angles([0.0; 5]));
    assert_eq!(after.measurement, Measurement::Measured);
    vector_close(after.centre_of_mass_m, before.centre_of_mass_m);
    close(after.clearance_m, before.clearance_m);
}

#[test]
fn duplicate_joint_identities_are_refused_regardless_of_serial_slot() {
    for duplicate in 1..5 {
        let mut geometry = rods();
        geometry.links[duplicate].joint = geometry.links[0].joint;
        assert!(Model::new(geometry).is_err());
    }
}

#[test]
fn invalid_base_and_part_masses_are_refused() {
    for invalid in [-0.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 100.1] {
        let mut geometry = rods();
        geometry.base.mass_kg = invalid;
        assert!(Model::new(geometry).is_err(), "base mass {invalid}");
        for index in 0..5 {
            let mut geometry = rods();
            geometry.links[index].mass_kg = invalid;
            assert!(
                Model::new(geometry).is_err(),
                "part {index}, mass {invalid}"
            );
        }
    }
    let mut no_base = rods();
    no_base.base.mass_kg = 0.0;
    assert!(Model::new(no_base).is_err());
    let mut zero_total = only_fixed_base([0.0; 3]);
    zero_total.base.mass_kg = 0.0;
    assert!(Model::new(zero_total).is_err());
    let mut negligible_total = only_fixed_base([0.0; 3]);
    negligible_total.base.mass_kg = 0.0005;
    assert!(Model::new(negligible_total).is_err());
}

#[test]
fn model_positions_refuse_nonfinite_and_out_of_bounds_values_in_every_component() {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2.0001, -2.0001] {
        for component in 0..3 {
            let mut geometry = rods();
            geometry.base.com_m[component] = invalid;
            assert!(Model::new(geometry).is_err());
            for index in 0..5 {
                let mut origin = rods();
                origin.links[index].origin_m[component] = invalid;
                assert!(Model::new(origin).is_err());
                let mut com = rods();
                com.links[index].com_m[component] = invalid;
                assert!(Model::new(com).is_err());
            }
        }
    }
    let mut below_support = rods();
    below_support.base.com_m[2] = -0.01;
    assert!(Model::new(below_support).is_err());
    // Signed offsets are valid coordinates, unlike a negative mass.
    let mut signed_offsets = rods();
    signed_offsets.base.com_m = [-0.02, -0.01, 0.01];
    signed_offsets.links[1].origin_m = [-0.2, -0.1, 0.2];
    signed_offsets.links[4].com_m = [-0.1, -0.05, -0.01];
    assert!(Model::new(signed_offsets).is_ok());
}

#[test]
fn joint_axes_refuse_zero_nonunit_and_nonfinite_vectors() {
    for index in 0..5 {
        for axis in [[0.0; 3], [2.0, 0.0, 0.0], [0.5, 0.0, 0.0]] {
            let mut geometry = rods();
            geometry.links[index].axis = axis;
            assert!(Model::new(geometry).is_err());
        }
        for component in 0..3 {
            for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut geometry = rods();
                geometry.links[index].axis[component] = invalid;
                assert!(Model::new(geometry).is_err());
            }
        }
    }
}

#[test]
fn origin_rpy_and_zero_angles_reject_degrees_nonfinite_and_out_of_range_values() {
    for invalid in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        90.0,
        PI + 1e-6,
        -PI - 1e-6,
    ] {
        for index in 0..5 {
            let mut zero = rods();
            zero.links[index].angle_zero_rad = invalid;
            assert!(Model::new(zero).is_err());
            for component in 0..3 {
                let mut rpy = rods();
                rpy.links[index].rpy_rad[component] = invalid;
                assert!(Model::new(rpy).is_err());
            }
        }
    }
}

#[test]
fn schema_and_provenance_label_are_required_and_bounded_in_bytes() {
    for version in [0, 2, u32::MAX] {
        let mut geometry = rods();
        geometry.schema_version = version;
        assert!(Model::new(geometry).is_err());
    }
    for label in [
        String::new(),
        " \n\t".into(),
        "a".repeat(257),
        "é".repeat(129),
    ] {
        let mut geometry = rods();
        geometry.label = label;
        assert!(Model::new(geometry).is_err());
    }
    let mut at_bound = rods();
    at_bound.label = "é".repeat(128);
    assert!(Model::new(at_bound).is_ok());
}

fn rejects_json(value: &Value, context: &str) {
    let text = serde_json::to_string(value).unwrap();
    assert!(Model::from_json(&text).is_err(), "{context}");
}

#[test]
fn json_round_trip_preserves_named_chain_order_and_pose_answers() {
    let text = serde_json::to_string(&rods()).unwrap();
    let restored = Geometry::from_json(&text).unwrap();
    assert_eq!(restored.links[3].joint, Joint::HeadPitch);
    assert_eq!(restored.links[4].joint, Joint::HeadRoll);
    let model = Model::from_json(&text).unwrap();
    vector_close(
        model.check_pose(angles([FRAC_PI_2; 5])).centre_of_mass_m,
        [0.01, 0.04, 0.04],
    );
}

#[test]
fn json_unknown_and_missing_fields_do_not_get_silent_defaults() {
    let valid = serde_json::to_value(rods()).unwrap();
    for field in [
        "schema_version",
        "measurement",
        "label",
        "footprint_m",
        "margin_m",
        "base",
        "links",
    ] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        rejects_json(&missing, field);
    }
    for field in ["mass_kg", "com_m"] {
        let mut missing = valid.clone();
        missing["base"].as_object_mut().unwrap().remove(field);
        rejects_json(&missing, field);
    }
    for field in [
        "joint",
        "origin_m",
        "rpy_rad",
        "axis",
        "angle_zero_rad",
        "mass_kg",
        "com_m",
    ] {
        let mut missing = valid.clone();
        missing["links"][4].as_object_mut().unwrap().remove(field);
        rejects_json(&missing, field);
    }
    let mut top_extra = valid.clone();
    top_extra["unknown"] = Value::Bool(true);
    rejects_json(&top_extra, "unknown geometry field");
    let mut base_extra = valid.clone();
    base_extra["base"]["unknown"] = Value::Bool(true);
    rejects_json(&base_extra, "unknown base field");
    let mut link_extra = valid.clone();
    link_extra["links"][4]["unknown"] = Value::Bool(true);
    rejects_json(&link_extra, "unknown link field");
    for joint in ["wrist_pitch", "elbow_pitch", "unknown", ""] {
        let mut bad_name = valid.clone();
        bad_name["links"][4]["joint"] = Value::String(joint.into());
        rejects_json(
            &bad_name,
            "unknown joint name must not alias or become zero",
        );
    }
    let mut bad_measurement = valid;
    bad_measurement["measurement"] = Value::String("probably_measured".into());
    rejects_json(&bad_measurement, "unknown measurement provenance");
}

#[test]
fn json_requires_exactly_five_links_and_three_components_per_vector() {
    let valid = serde_json::to_value(rods()).unwrap();
    let mut fewer = valid.clone();
    fewer["links"].as_array_mut().unwrap().pop();
    rejects_json(&fewer, "missing physical link");
    let mut more = valid.clone();
    more["links"]
        .as_array_mut()
        .unwrap()
        .push(valid["links"][0].clone());
    rejects_json(&more, "extra physical link");
    for field in ["origin_m", "rpy_rad", "axis", "com_m"] {
        for count in [2, 4] {
            let mut malformed = valid.clone();
            malformed["links"][0][field] = serde_json::json!(vec![0.0; count]);
            rejects_json(&malformed, field);
        }
    }
    let mut duplicate = valid;
    duplicate["links"][4]["joint"] = Value::String("base_yaw".into());
    rejects_json(&duplicate, "duplicate joint, omitted head roll");
}

#[test]
fn json_size_is_bounded_before_parsing_and_malformed_text_is_refused() {
    let mut at_limit = serde_json::to_string(&rods()).unwrap();
    at_limit.push_str(&" ".repeat(65_536 - at_limit.len()));
    assert!(Model::from_json(&at_limit).is_ok());
    at_limit.push(' ');
    assert!(Model::from_json(&at_limit).is_err());
    for malformed in ["", "{", "null", "[]", "{\"base\":NaN}", "{} trailing"] {
        assert!(Model::from_json(malformed).is_err());
    }
}

#[test]
fn invalid_support_data_are_refused_through_the_public_model_constructor() {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01, 1.01] {
        let mut geometry = rods();
        geometry.margin_m = invalid;
        assert!(Model::new(geometry).is_err());
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2.01, -2.01] {
        for component in 0..2 {
            let mut geometry = rods();
            geometry.footprint_m[0][component] = invalid;
            assert!(Model::new(geometry).is_err());
        }
    }
    for count in [0, 1, 2, 33] {
        let mut geometry = rods();
        geometry.footprint_m = (0..count)
            .map(|index| {
                let theta = TAU * f64::from(index) / f64::from(count);
                [0.1 * theta.cos(), 0.1 * theta.sin()]
            })
            .collect();
        assert!(Model::new(geometry).is_err());
    }
    let mut no_inset = rods();
    no_inset.margin_m = 0.1;
    assert!(Model::new(no_inset).is_err());
}
