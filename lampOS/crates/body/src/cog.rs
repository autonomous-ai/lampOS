//! V1's anti-tip rule (#641, #642 in the first lamp software): how far the
//! lamp's centre of gravity sits off its base's axis, by V1's own body model
//! (`hal/drivers/motors/recording_stability.py` on `robots/lamp/urdf/lamp.urdf`,
//! converted for `lamp-lean` as `deploy/lamp-4ace/lean-first-software.json`,
//! which agrees with V1's `cog_offset_mm` to 0.03 mm), and V1's two limits:
//!
//! - [`CLIP_MM`], 22 mm (V1's `SAFETY.md` `motion.max_cog_offset_mm`): a
//!   recording any frame of which takes the centre of gravity farther than
//!   this off the axis, in any direction, is refused whole before it plays
//!   (V1 `check_stable`), with a warning from 85% of it. A commanded move is
//!   cut short before it leans the lamp back past it (V1 `/servo/move`,
//!   `nudge`).
//! - [`LOOK_MM`], 16 mm (V1 `MAX_LEAN_BACK_MM`): a pose the lamp takes by
//!   itself, following a face or a mood, is cut short before it leans the
//!   lamp back past it (V1 `distribute_pitch`).
//!
//! Cut short as V1 cut it ([`limit_lean_back`]): the step from where the
//! goals were to where they are wanted is shortened, by halving ten times,
//! to the largest share of it that stays inside; a pose already past the
//! limit may only come forward.
//!
//! The masses are V1's estimates (1.49 kg against 7 kg on the spec), so the
//! millimetres are V1's, not a measurement of any lamp (docs/lean.md).

use crate::JOINT_COUNT;
use lamp_lean::{Angles, Geometry, Joint, Model};
use std::sync::LazyLock;

/// V1's tip-over ceiling for recordings and commanded moves, mm.
pub const CLIP_MM: f64 = 22.0;
/// V1's lean-back ceiling for what the lamp does by itself, mm.
pub const LOOK_MM: f64 = 16.0;
/// V1 warns of a recording from this share of [`CLIP_MM`] up.
pub const WARN_SHARE: f64 = 0.85;

struct Body {
    model: Model,
    /// Base yaw's angle at V1's zero: the arm's own heading is x there.
    yaw_zero: f64,
}

static BODY: LazyLock<Body> = LazyLock::new(|| {
    let geometry = Geometry::from_json(include_str!(
        "../../../deploy/lamp-4ace/lean-first-software.json"
    ))
    .expect("V1's body model parses");
    let yaw_zero = geometry
        .links
        .iter()
        .find(|link| link.joint == Joint::BaseYaw)
        .map_or(0.0, |link| link.angle_zero_rad);
    Body {
        model: Model::new(geometry).expect("V1's body model is valid"),
        yaw_zero,
    }
});

/// Whether every count is one a servo can be at, 0 to 4,095. A count
/// outside is refused, not clamped: clamped, lamp-4ace's elbow goal of
/// 37,639 read as 4,095 and was cut toward and written (2026-10-17).
fn in_range(counts: [f32; JOINT_COUNT]) -> bool {
    counts.iter().all(|count| (0.0..=4_095.0).contains(count))
}

/// Radians of encoder counts, servo IDs 1..5 (`Joint::index` order), each
/// 0 to 4,095.
fn radians(counts: [f32; JOINT_COUNT]) -> [f64; JOINT_COUNT] {
    counts.map(|count| (f64::from(count) - 2048.0) * std::f64::consts::TAU / 4096.0)
}

fn centre_mm(radians: [f64; JOINT_COUNT]) -> [f64; 2] {
    let angles = Angles::new(radians).expect("counts 0..4095 are within -pi..pi");
    let [x, y, _] = BODY.model.check_pose(angles).centre_of_mass_m;
    [x * 1000.0, y * 1000.0]
}

/// How far (mm) the centre of gravity sits off the base's axis, in any
/// direction (V1 `cog_offset_mm`). Counts by servo ID 1..5; a pose with a
/// count outside 0..=4,095 is infinitely far.
pub fn off_axis_mm(counts: [f32; JOINT_COUNT]) -> f64 {
    if !in_range(counts) {
        return f64::INFINITY;
    }
    let [x, y] = centre_mm(radians(counts));
    x.hypot(y)
}

/// How far (mm) the centre of gravity sits behind the base's axis, along
/// the arm's own heading (V1 `lean_back_mm`): base yaw left out. A pose
/// with a count outside 0..=4,095 is infinitely far.
pub fn lean_back_mm(counts: [f32; JOINT_COUNT]) -> f64 {
    if !in_range(counts) {
        return f64::INFINITY;
    }
    let mut radians = radians(counts);
    radians[0] = BODY.yaw_zero;
    -centre_mm(radians)[0]
}

/// `target` cut short so the lamp leans back no farther than `ceiling_mm`,
/// and the share of the step from `start` that was kept (V1
/// `limit_lean_back`). A pose already past the ceiling may only come
/// forward. The whole step and 1.0 when it fits. A target with a count
/// outside 0..=4,095 is not stepped toward at all: `start` and 0.0.
pub fn limit_lean_back(
    start: [f32; JOINT_COUNT],
    target: [f32; JOINT_COUNT],
    ceiling_mm: f64,
) -> ([f32; JOINT_COUNT], f32) {
    let along = |share: f32| -> [f32; JOINT_COUNT] {
        std::array::from_fn(|index| start[index] + (target[index] - start[index]) * share)
    };
    if !in_range(target) {
        return (start, 0.0);
    }
    let limit = ceiling_mm.max(lean_back_mm(start));
    if lean_back_mm(target) <= limit {
        return (target, 1.0);
    }
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..10 {
        let middle = (low + high) / 2.0;
        if lean_back_mm(along(middle)) <= limit {
            low = middle;
        } else {
            high = middle;
        }
    }
    (along(low), low)
}

/// The frame of `frames`, laid over `pose` (head pitch, head roll, base
/// yaw, shoulder, elbow: a clip's order), that takes the centre of gravity
/// farthest off the axis, and how far (V1 `worst_frame`).
pub fn worst_frame(pose: [u16; 5], frames: &[[i16; 5]]) -> (usize, f64) {
    let mut worst = (0, 0.0);
    for (index, frame) in frames.iter().enumerate() {
        let at = |slot: usize| f32::from(pose[slot]) + f32::from(frame[slot]);
        let mm = off_axis_mm([at(2), at(3), at(4), at(1), at(0)]);
        if mm > worst.1 {
            worst = (index, mm);
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clips;

    /// Servo IDs 1..5 of a pose in a clip's order: head pitch, head roll,
    /// base yaw, shoulder, elbow.
    fn by_id(pose: [u16; 5]) -> [f32; JOINT_COUNT] {
        [pose[2], pose[3], pose[4], pose[1], pose[0]].map(f32::from)
    }

    /// The standing and folded poses the deployed lamps are given
    /// (`deploy/lamp-*/lamp-body.service`: `--stand`, and `--home` with
    /// `--folded-arm`), in a clip's order.
    const LAMPS: [(&str, [u16; 5], [u16; 5]); 2] = [
        (
            "lamp-4ace",
            [1772, 2043, 2015, 2657, 2602],
            [1950, 2043, 2015, 2048, 2047],
        ),
        (
            "lamp-52e6",
            [1772, 2035, 2035, 2657, 2602],
            [1771, 2035, 2035, 2053, 2069],
        ),
    ];

    #[test]
    fn the_centre_of_gravity_is_v1s_on_the_motor_readings_v1_was_checked_on() {
        // lamp-4ace at rest and standing, 14 October (crates/lean/tests/it/first_software.rs).
        assert!((off_axis_mm([2023.0, 2052.0, 2047.0, 2053.0, 1950.0]) - 17.851).abs() < 0.03);
        assert!((off_axis_mm([2023.0, 2558.0, 2503.0, 2053.0, 1820.0]) - 1.553).abs() < 0.03);
        // Folded, it sits behind its axis; base yaw turns the lean, not how far back.
        let rest = [2023.0, 2052.0, 2047.0, 2053.0, 1950.0];
        assert!((lean_back_mm(rest) - off_axis_mm(rest)).abs() < 0.1);
        assert!(
            (lean_back_mm([2600.0, 2052.0, 2047.0, 2053.0, 1950.0]) - lean_back_mm(rest)).abs()
                < 1e-9
        );
    }

    #[test]
    fn a_step_that_would_lean_back_past_the_ceiling_is_cut_short_as_v1_cut_it() {
        let stand = by_id(LAMPS[0].1);
        // The shoulder taken 400 counts back and the elbow 400 the other way lean it back past 16 mm.
        let back = [
            stand[0],
            stand[1] - 400.0,
            stand[2] + 400.0,
            stand[3],
            stand[4],
        ];
        assert!(lean_back_mm(back) > LOOK_MM, "{}", lean_back_mm(back));
        let (cut, share) = limit_lean_back(stand, back, LOOK_MM);
        assert!(share > 0.0 && share < 1.0, "{share}");
        assert!(lean_back_mm(cut) <= LOOK_MM && lean_back_mm(cut) > LOOK_MM - 0.1);
        // Under its ceiling, the whole step.
        assert_eq!(limit_lean_back(stand, back, 40.0), (back, 1.0));
        // Already past: it may come forward, never farther back.
        let further = [back[0], back[1] - 50.0, back[2], back[3], back[4]];
        assert!(lean_back_mm(further) > lean_back_mm(back));
        assert_eq!(limit_lean_back(back, further, LOOK_MM).1, 0.0);
        assert_eq!(limit_lean_back(back, stand, LOOK_MM), (stand, 1.0));
    }

    /// A count no servo can be at is refused, not clamped to the seam: a
    /// step toward it is not taken, and a pose with it is infinitely far.
    #[test]
    fn a_count_outside_the_servos_range_is_refused_not_clamped() {
        let folded = by_id(LAMPS[0].2);
        let mut wrapped = folded;
        wrapped[2] = 37_639.0;
        assert_eq!(lean_back_mm(wrapped), f64::INFINITY);
        assert_eq!(off_axis_mm(wrapped), f64::INFINITY);
        assert_eq!(limit_lean_back(folded, wrapped, CLIP_MM), (folded, 0.0));
        wrapped[2] = -1.0;
        assert_eq!(limit_lean_back(folded, wrapped, CLIP_MM), (folded, 0.0));
    }

    /// Every built-in clip, on each deployed lamp's standing pose, passes
    /// V1's rule, so no regenerated clip is refused at runtime: no frame
    /// within 85% of 22 mm off the axis (so not even said), and none leans
    /// the lamp back past 16 mm, with no clip let off. The folded poses
    /// are inside 22 mm.
    #[test]
    fn every_built_in_clip_passes_v1s_rule() {
        for (lamp, stand, folded) in LAMPS {
            assert!(off_axis_mm(by_id(folded)) < CLIP_MM, "{lamp}");
            for clip in clips::ALL
                .iter()
                .chain([&&clips::CALLED])
                .chain(crate::idle::MOVES.iter())
            {
                let (frame, mm) = worst_frame(stand, clip.frames);
                assert!(
                    mm < CLIP_MM * WARN_SHARE,
                    "{lamp} {} frame {frame}: {mm}",
                    clip.name
                );
                for offsets in clip.frames {
                    let pose: [u16; 5] = std::array::from_fn(|joint| {
                        (i32::from(stand[joint]) + i32::from(offsets[joint])) as u16
                    });
                    let back = lean_back_mm(by_id(pose));
                    assert!(back <= LOOK_MM, "{lamp} {}: {back}", clip.name);
                }
            }
        }
    }
}
