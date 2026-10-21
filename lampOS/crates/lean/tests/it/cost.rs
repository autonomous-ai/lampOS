use lamp_expression::Catalogue;
use lamp_lean::{Angles, Clip, Geometry, Model, OffsetFrame, Step, Verdict};
use lamp_msgs::Voice;
use serde_json::{Value, json};
use std::hint::black_box;
use std::time::Instant;

const DATA: &str = include_str!("../../geometry.stand-in.json");
const REPEATS: usize = 30;

fn timed<T>(operation: impl FnOnce() -> T) -> (T, f64) {
    let start = Instant::now();
    let value = black_box(operation());
    (value, start.elapsed().as_secs_f64() * 1000.)
}

fn summary(mut values: Vec<f64>) -> Value {
    values.sort_by(f64::total_cmp);
    json!({
        "calls": values.len(),
        "median_ms": (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.,
        "p95_ms": values[(values.len() * 95).div_ceil(100) - 1],
        "worst_ms": values[values.len() - 1],
    })
}

#[test]
#[ignore = "finite release timing; host measurements are not lamp measurements"]
fn arithmetic_cost() {
    let (model, first_model_ms) = timed(|| Model::from_json(black_box(DATA)).unwrap());
    let pose = Angles::new([0.1, 0.1, -0.05, 0.02, 0.03]).unwrap();
    let (first, first_pose_ms) = timed(|| model.check_pose(black_box(pose)));
    assert!(first.within_margin());
    let steps: Vec<_> = (0..1000)
        .map(|n| {
            let t = f64::from(n) * 0.01;
            Step {
                at_us: n as u64 * 20_000,
                angles: Angles::new([
                    0.1 * t.sin(),
                    0.1 * t.sin(),
                    0.05 * t.cos(),
                    0.08 * (2. * t).cos(),
                    0.03 * (3. * t).sin(),
                ])
                .unwrap(),
            }
        })
        .collect();
    let (first, first_movement_ms) = timed(|| model.check_movement(black_box(&steps)).unwrap());
    assert!(first.within_margin());
    assert_eq!(first.checked_steps, 1000);
    let movement_evaluations = first.evaluated_poses;
    let catalogue = Catalogue::builtin();
    let behaviour = catalogue
        .behaviours
        .iter()
        .find(|behaviour| behaviour.repeat)
        .unwrap();
    let frames: Vec<_> = behaviour
        .keyframes
        .iter()
        .map(|frame| OffsetFrame {
            at_us: frame.at_us,
            offsets: frame.offsets,
        })
        .collect();
    let clip = Clip {
        posture: catalogue.postures.for_voice(Voice::Speaking),
        frames: &frames,
        weight: behaviour.weight,
        limits: catalogue.limits,
        applied: [true; 5],
        repeat: behaviour.repeat,
    };
    let (first, first_clip_ms) = timed(|| model.check_clip(pose, clip, 20_000).unwrap());
    assert!(first.within_margin());
    let clip_evaluations = first.evaluated_poses;
    let mut models = Vec::new();
    let mut poses = Vec::new();
    let mut movements = Vec::new();
    let mut clips = Vec::new();
    for _ in 0..REPEATS {
        models.push(timed(|| Model::from_json(black_box(DATA)).unwrap()).1);
        let (result, elapsed) = timed(|| model.check_pose(black_box(pose)));
        assert!(result.within_margin());
        poses.push(elapsed);
        let (result, elapsed) = timed(|| model.check_movement(black_box(&steps)).unwrap());
        assert!(result.within_margin());
        assert_eq!(result.evaluated_poses, movement_evaluations);
        movements.push(elapsed);
        let (result, elapsed) = timed(|| model.check_clip(pose, clip, 20_000).unwrap());
        assert!(result.within_margin());
        assert_eq!(result.evaluated_poses, clip_evaluations);
        clips.push(elapsed);
    }
    println!(
        "{}",
        json!({
            "fixture": "synthetic five rods; no hardware or disk I/O",
            "boundary": "Instant around each call, inputs already resident; no cache eviction",
            "first_model_json_and_validation_ms": first_model_ms,
            "repeated_model_json_and_validation": summary(models),
            "first_pose_ms": first_pose_ms,
            "repeated_pose": summary(poses),
            "movement_steps": steps.len(),
            "movement_evaluated_poses": movement_evaluations,
            "first_movement_ms": first_movement_ms,
            "repeated_movement": summary(movements),
            "clip_name": behaviour.name,
            "clip_grid_us": 20000,
            "clip_evaluated_poses": clip_evaluations,
            "first_clip_ms": first_clip_ms,
            "repeated_clip": summary(clips),
        })
    );
}

#[test]
#[ignore = "finite worst-work release timing; host measurements are not lamp measurements"]
fn inconclusive_cost() {
    let mut data = Geometry::from_json(DATA).unwrap();
    data.footprint_m = vec![[-0.2, -0.1], [0.01, -0.1], [0.01, 0.1], [-0.2, 0.1]];
    data.base.com_m[0] = -2e-10;
    let model = Model::new(data).unwrap();
    let steps = [
        Step {
            at_us: 0,
            angles: Angles::new([0.; 5]).unwrap(),
        },
        Step {
            at_us: 100_000,
            angles: Angles::new([0.1, 0., 0., 0., 0.]).unwrap(),
        },
    ];
    let check = || {
        let report = model.check_movement(black_box(&steps)).unwrap();
        assert_eq!(report.verdict, Verdict::Unresolved);
        assert_eq!(report.evaluated_poses, 100_000);
    };
    let (_, first_ms) = timed(check);
    let repeated: Vec<_> = (0..REPEATS).map(|_| timed(check).1).collect();
    println!(
        "{}",
        json!({
            "fixture": "near-boundary synthetic path, 100000 evaluations, unresolved",
            "boundary": "resident model and two input steps; includes two result assertions",
            "first_ms": first_ms,
            "repeated": summary(repeated),
        })
    );
}
