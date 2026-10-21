use lamp_lean::{Angles, Clip, Geometry, Model, OffsetFrame, Step, Verdict};
use lamp_msgs::BodyOffsets;
use std::f64::consts::{FRAC_PI_2, PI};

fn geometry() -> Geometry {
    Geometry::from_json(include_str!("../../geometry.stand-in.json")).unwrap()
}
fn angles(values: [f64; 5]) -> Angles {
    Angles::new(values).unwrap()
}
fn pose(yaw: f64, shoulder: f64) -> Angles {
    angles([yaw, shoulder, 0., 0., 0.])
}
fn step(at_us: u64, angles: Angles) -> Step {
    Step { at_us, angles }
}
fn offset(yaw: f64) -> BodyOffsets {
    BodyOffsets {
        base_yaw: yaw,
        ..BodyOffsets::ZERO
    }
}
fn frame(at_us: u64, yaw: f64) -> OffsetFrame {
    OffsetFrame {
        at_us,
        offsets: offset(yaw),
    }
}
fn clip(frames: &[OffsetFrame]) -> Clip<'_> {
    Clip {
        posture: BodyOffsets::ZERO,
        frames,
        weight: 1.,
        limits: BodyOffsets::from_values([0.35; 5]),
        applied: [true; 5],
        repeat: false,
    }
}
fn rectangle(x: f64, y: f64) -> Geometry {
    let mut data = geometry();
    data.footprint_m = vec![[-x, -y], [x, -y], [x, y], [-x, y]];
    data
}

#[test]
fn every_planned_step_and_worst_time_are_reported_even_after_a_refusal() {
    let model = Model::new(geometry()).unwrap();
    let steps = [
        step(0, pose(0., 0.)),
        step(20_000, pose(0., PI / 4.)),
        step(40_000, pose(0., FRAC_PI_2)),
        step(60_000, pose(0., 0.)),
    ];
    let report = model.check_movement(&steps).unwrap();
    assert_eq!(report.checked_steps, 4);
    assert_eq!(report.evaluated_poses, 4);
    assert_eq!(report.verdict, Verdict::OutsideMargin);
    assert_eq!(report.worst_sample.at_us, 40_000.);
    assert!((report.worst_sample.check.clearance_m + 0.07).abs() < 1e-12);
}

#[test]
fn safe_endpoints_do_not_hide_an_outside_joint_linear_interior() {
    let model = Model::new(rectangle(0.1, 0.05)).unwrap();
    let a = pose(0., PI / 6.);
    let b = pose(PI, PI / 6.);
    assert!(model.check_pose(a).within_margin());
    assert!(model.check_pose(b).within_margin());
    // COM radius is .16*sin(pi/6)=.08 m. The y inset is .04 m.
    let report = model
        .check_movement(&[step(0, a), step(100_000, b)])
        .unwrap();
    assert_eq!(report.verdict, Verdict::OutsideMargin);
    assert_eq!(report.worst_sample.at_us, 50_000.);
    assert!((report.worst_sample.check.clearance_m + 0.04).abs() < 1e-12);
    assert!(report.evaluated_poses > 2);
}

#[test]
fn analytic_bound_is_below_a_dense_independent_static_sample() {
    let model = Model::new(geometry()).unwrap();
    for amplitude in [0.01, 0.1, 0.25, 0.45] {
        let report = model
            .check_movement(&[
                step(0, pose(0., 0.)),
                step(100_000, pose(amplitude, amplitude)),
            ])
            .unwrap();
        assert_eq!(report.verdict, Verdict::WithinMargin);
        let dense = (0..=1000)
            .map(|n| {
                let q = amplitude * f64::from(n) / 1000.;
                model.check_pose(pose(q, q)).clearance_m
            })
            .fold(f64::INFINITY, f64::min);
        assert!(report.minimum_clearance_bound_m <= dense + 1e-12);
        assert!(report.evaluated_poses <= 100_000);
    }
}

#[test]
fn interval_bound_uses_all_five_physical_radii_in_canonical_order() {
    let model = Model::new(geometry()).unwrap();
    let a = angles([0.; 5]);
    let b = angles([0.001, 0.002, 0.003, 0.004, 0.005]);
    // The five-rod fixture has canonical radii [.25,.16,.09,.01,.04].
    // Last two reverse the serial order. Weighted angular travel = .00108 m.
    let expected = model
        .check_pose(a)
        .clearance_m
        .min(model.check_pose(b).clearance_m)
        - 0.00054;
    let report = model
        .check_movement(&[step(0, a), step(100_000, b)])
        .unwrap();
    assert!(report.within_margin());
    assert_eq!(report.evaluated_poses, 2);
    assert!((report.minimum_clearance_bound_m - expected).abs() < 1e-12);
}

#[test]
fn boundary_is_unresolved_and_zero_motion_is_not_subdivided_forever() {
    let mut data = geometry();
    data.base.com_m = [0.18, 0., 0.];
    for link in &mut data.links {
        link.mass_kg = 0.;
    }
    // Put fixed COM exactly on the inset's right edge (.09).
    data.base.com_m[0] = 0.09;
    let model = Model::new(data).unwrap();
    let report = model
        .check_movement(&[step(0, pose(0., 0.)), step(1000, pose(0.1, 0.))])
        .unwrap();
    assert_eq!(report.verdict, Verdict::Unresolved);
    assert!(!report.within_margin());
    assert_eq!(report.evaluated_poses, 2);
}

#[test]
fn malformed_movement_is_never_an_empty_success() {
    let model = Model::new(geometry()).unwrap();
    let a = pose(0., 0.);
    for steps in [
        vec![],
        vec![step(1, a)],
        vec![step(0, a), step(0, a)],
        vec![step(0, a), step(2, a), step(1, a)],
        vec![step(0, a), step(600_000_001, a)],
        (0..10_001).map(|n| step(n, a)).collect(),
    ] {
        assert!(model.check_movement(&steps).is_err());
    }
    assert!(model.check_movement(&[step(0, a)]).unwrap().within_margin());
}

#[test]
fn whole_expression_clip_finds_unsafe_interior_with_safe_baseline_and_keys() {
    let model = Model::new(rectangle(0.169, 0.1)).unwrap();
    let base = pose(-0.2, FRAC_PI_2);
    let frames = [frame(0, 0.), frame(100_000, 0.35)];
    assert!(model.check_pose(base).within_margin());
    assert!(model.check_pose(pose(0.15, FRAC_PI_2)).within_margin());
    // Only two grid/key times; adaptive checking must discover the interior.
    // The COM's .16 m radius crosses x=.159 m despite safe endpoint yaw.
    let report = model.check_clip(base, clip(&frames), 100_000).unwrap();
    assert_eq!(report.checked_steps, 2);
    assert_eq!(report.verdict, Verdict::OutsideMargin);
    assert!(report.evaluated_poses > 2);
    assert!(report.worst_sample.at_us > 0. && report.worst_sample.at_us < 100_000.);
    assert!(report.worst_sample.check.clearance_m < -0.0009);
}

#[test]
fn clamped_clip_is_checked_on_its_actual_curve_not_between_clipped_endpoints() {
    let model = Model::new(rectangle(0.165, 0.1)).unwrap();
    let base = pose(0., 1.2);
    let frames = [
        OffsetFrame {
            at_us: 0,
            offsets: BodyOffsets::from_values([-0.35, 0.35, 0., 0., 0.]),
        },
        OffsetFrame {
            at_us: 100_000,
            offsets: BodyOffsets::from_values([0.35, -0.35, 0., 0., 0.]),
        },
    ];
    let mut value = clip(&frames);
    value.posture = BodyOffsets::from_values([0.05, 0.15, 0., 0., 0.]);
    value.limits = BodyOffsets::from_values([0.2; 5]);
    // Clipped endpoints are (-.2, 1.4) and (.2, 1.0). Their artificial
    // joint-linear path stays inside x=.155, but the actual smoothstep at
    // halfway is (.05, 1.35), whose x COM is .16*sin(1.35)*cos(.05)>.155.
    let artificial = model
        .check_movement(&[step(0, pose(-0.2, 1.4)), step(100_000, pose(0.2, 1.0))])
        .unwrap();
    assert!(artificial.within_margin());
    let report = model.check_clip(base, value, 100_000).unwrap();
    assert_eq!(report.verdict, Verdict::OutsideMargin);
    assert_eq!(report.worst_sample.at_us, 50_000.);
    let q = report.worst_sample.angles.radians();
    assert!((q[0] - 0.05).abs() < 1e-12);
    assert!((q[1] - 1.35).abs() < 1e-12);
    assert!(report.worst_sample.check.clearance_m < -0.0009);
}

#[test]
fn reported_clip_time_uses_smoothstep_rather_than_linear_offsets() {
    let model = Model::new(rectangle(0.1699, 0.1)).unwrap();
    let frames = [frame(0, 0.), frame(100_000, 0.35)];
    // smoothstep(.25)=.15625; .35*.15625=.0546875, canceling base yaw
    // exactly at the quarter-time sample, where COM reaches maximum x=.16.
    let report = model
        .check_clip(pose(-0.054_687_5, FRAC_PI_2), clip(&frames), 25_000)
        .unwrap();
    assert_eq!(report.verdict, Verdict::OutsideMargin);
    assert_eq!(report.worst_sample.at_us, 25_000.);
    assert_eq!(report.worst_sample.angles.radians()[0], 0.);
    assert!((report.worst_sample.check.clearance_m + 0.0001).abs() < 1e-12);
}

#[test]
fn positive_samples_cannot_turn_work_exhaustion_into_a_pass() {
    let mut data = geometry();
    data.footprint_m = vec![[-0.2, -0.1], [0.01, -0.1], [0.01, 0.1], [-0.2, 0.1]];
    data.base.com_m[0] = -2e-10;
    // Every part lies on the yaw axis. Its COM projection stays fixed just
    // inside the margin, but the general radius bound is conservative and
    // cannot establish the numerical reserve. Exercise the actual work cap.
    let model = Model::new(data).unwrap();
    let report = model
        .check_movement(&[step(0, pose(0., 0.)), step(100_000, pose(0.1, 0.))])
        .unwrap();
    assert!(report.worst_sample.check.clearance_m > 0.);
    assert_eq!(report.verdict, Verdict::Unresolved);
    assert!(!report.within_margin());
    assert_eq!(report.evaluated_poses, 100_000);
    assert!(report.minimum_clearance_bound_m <= report.worst_sample.check.clearance_m);
}

#[test]
fn depth_exhaustion_is_unresolved_even_when_every_sample_exceeds_the_reserve() {
    // At yaw zero the level arm projects x=.16, leaving just 2 nm before the
    // inset. A true sample passes the 1 nm reserve, but 20 subdivision levels
    // cannot establish the conservative interval bound near that extremum.
    let model = Model::new(rectangle(0.170_000_002, 0.1)).unwrap();
    let report = model
        .check_movement(&[
            step(0, pose(-0.01, FRAC_PI_2)),
            step(100_000, pose(0.01, FRAC_PI_2)),
        ])
        .unwrap();
    assert!(report.worst_sample.check.within_margin());
    assert!(report.evaluated_poses < 100_000);
    assert_eq!(report.verdict, Verdict::Unresolved);
    assert!(!report.within_margin());
}

#[test]
fn composition_matches_posture_weight_clamp_and_explicit_owner_mask() {
    let model = Model::new(geometry()).unwrap();
    let offsets = BodyOffsets::from_values([0.3, 0.1, -0.2, 0.3, -0.2]);
    let frames = [
        OffsetFrame { at_us: 0, offsets },
        OffsetFrame {
            at_us: 20_000,
            offsets,
        },
    ];
    let mut value = clip(&frames);
    value.posture = BodyOffsets::from_values([0.1, 0.1, 0.1, 0.1, 0.1]);
    value.weight = 0.5;
    value.limits = BodyOffsets::from_values([0.2; 5]);
    value.applied = [true, false, false, true, true];
    let expected = angles([0.2, 0., 0., 0.2, 0.]);
    let report = model.check_clip(pose(0., 0.), value, 20_000).unwrap();
    assert_eq!(report.worst_sample.angles, expected);
    assert_eq!(
        report.worst_sample.check.clearance_m,
        model.check_pose(expected).clearance_m
    );
}

#[test]
fn grid_always_includes_nonaligned_keyframes_and_final_time() {
    let model = Model::new(geometry()).unwrap();
    let frames = [frame(0, 0.), frame(15_001, 0.1), frame(39_999, 0.)];
    let report = model
        .check_clip(pose(0., 0.), clip(&frames), 20_000)
        .unwrap();
    assert_eq!(report.checked_steps, 4); // 0, 15001, 20000, 39999.
    assert!(report.within_margin());
}

#[test]
fn invalid_offset_on_an_ignored_joint_is_still_refused() {
    let model = Model::new(geometry()).unwrap();
    let mut frames = [frame(0, 0.), frame(20_000, 0.)];
    frames[1].offsets.shoulder = f64::NAN;
    let mut value = clip(&frames);
    value.applied = [true, false, false, true, true];
    assert!(model.check_clip(pose(0., 0.), value, 20_000).is_err());
}

#[test]
fn bad_clips_and_work_requests_fail_before_unbounded_sampling() {
    let model = Model::new(geometry()).unwrap();
    let base = pose(0., 0.);
    let good = [frame(0, 0.), frame(20_000, 0.1)];
    for frames in [
        vec![],
        vec![frame(0, 0.)],
        vec![frame(0, 0.), frame(0, 0.)],
        vec![frame(1, 0.), frame(2, 0.)],
        vec![frame(0, 0.), frame(10_000_001, 0.)],
        (0..65).map(|n| frame(n * 1000, 0.)).collect(),
    ] {
        assert!(model.check_clip(base, clip(&frames), 20_000).is_err());
    }
    for time in [0, 1, 100_001, u64::MAX] {
        assert!(model.check_clip(base, clip(&good), time).is_err());
    }
    for weight in [-1., 1.1, f64::NAN, f64::INFINITY] {
        let mut value = clip(&good);
        value.weight = weight;
        assert!(model.check_clip(base, value, 20_000).is_err());
    }
    let mut value = clip(&good);
    value.limits.head_pitch = -0.1;
    assert!(model.check_clip(base, value, 20_000).is_err());
    let mut value = clip(&good);
    value.repeat = true;
    assert!(model.check_clip(base, value, 20_000).is_err());
    assert!(model.check_clip(pose(PI, 0.), clip(&good), 20_000).is_err());
    let closed = [frame(0, 0.), frame(10_000, 0.1), frame(20_000, 0.)];
    let mut value = clip(&closed);
    value.repeat = true;
    assert!(
        model
            .check_clip(base, value, 20_000)
            .unwrap()
            .within_margin()
    );
}
