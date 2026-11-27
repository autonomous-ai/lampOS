//! Offline head-pose candidate for #8847; no runtime or bus integration.
//!
//! Reuses the geometry and canonical points from the closed #8782 draft,
//! `3a9ae0692bcf1f9e38303db0c0865c84df9099e6`,
//! `lampOS/crates/eyes/src/facing.rs`.
//! The canonical values come from MediaPipe's Apache-2.0
//! `mediapipe/modules/face_geometry/data/canonical_face_model.obj`.
//!
//! Input is Face Mesh V2's 478 xyz landmarks in the rotated 256-pixel crop:
//! x right, y down, z away from the camera, with z assumed at x's scale.
//! The roll-compensated angles are a candidate measurement, not calibrated
//! confidence, gaze, a lamp/screen/phone decision, or permission to act.
//! Synthetic rigid-face tests establish mathematical behavior only; they do
//! not establish the model's depth scale or accuracy on a real person.

use lamp_eyes::mesh::LANDMARKS;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Head {
    /// Degrees toward the picture's right (the person's own left), zero
    /// toward the camera. The input must retain the model's landmark order.
    pub yaw_deg: f32,
    /// Degrees tipped down; negative is up.
    pub pitch_deg: f32,
}

/// Outer eye corners, inner eye corners, and nose sides: picture-left to
/// picture-right pairs on the canonical face. No iris or lip point is used.
const ACROSS: [(usize, usize); 3] = [(33, 263), (133, 362), (129, 358)];
/// Nose base to between the brows, and nose base to forehead.
const UP: [(usize, usize); 2] = [(2, 9), (94, 151)];
/// Canonical centimetres: x toward the person's left, y up, z out of the
/// face. Only the middle pairs' lean is used to set the pitch zero.
const CANONICAL: [(usize, [f32; 3]); 10] = [
    (33, [-4.445_859, 2.663_991, 3.173_422]),
    (263, [4.445_859, 2.663_991, 3.173_422]),
    (133, [-1.856_432, 2.585_245, 3.757_904]),
    (362, [1.856_432, 2.585_245, 3.757_904]),
    (129, [-1.785_794, -0.978_284, 4.850_47]),
    (358, [1.785_794, -0.978_284, 4.850_47]),
    (2, [0.0, -2.089_024, 6.058_267]),
    (9, [0.0, 4.885_979, 5.385_258]),
    (94, [0.0, -1.724_003, 6.601_39]),
    (151, [0.0, 6.545_39, 5.027_311]),
];

fn up_lean() -> f32 {
    let point = |index| {
        CANONICAL
            .iter()
            .find(|(at, _)| *at == index)
            .map_or([0.0; 3], |(_, point)| *point)
    };
    let (mut up, mut out) = (0.0f32, 0.0f32);
    for (from, to) in UP {
        let (from, to) = (point(from), point(to));
        up += to[1] - from[1];
        out += to[2] - from[2];
    }
    out.atan2(up)
}

/// Estimate roll-compensated yaw and pitch from one mesh, without allocation,
/// clocks, I/O, temporal smoothing, fallback, or a confidence claim.
///
/// Requires exactly 478 triples and finite values throughout. Missing points,
/// arithmetic overflow, degenerate directions, and normals outside the
/// camera-facing half-space return `None`; they never become zero angles.
/// Finite points outside the crop are not automatically rejected: crop
/// visibility and model confidence remain separate evaluation concerns.
pub fn mesh_head(landmarks: &[f32]) -> Option<Head> {
    if landmarks.len() != LANDMARKS * 3 || !landmarks.iter().all(|value| value.is_finite()) {
        return None;
    }
    let point = |index: usize| {
        [
            landmarks[index * 3],
            landmarks[index * 3 + 1],
            landmarks[index * 3 + 2],
        ]
    };
    let sum = |pairs: &[(usize, usize)]| {
        pairs.iter().fold([0.0; 3], |sum, &(from, to)| {
            plus(sum, minus(point(to), point(from)))
        })
    };
    let across = unit(sum(&ACROSS))?;
    let up = sum(&UP);
    let up = unit(minus(up, times(across, dot(up, across))))?;
    // Across times up points out of the face; correct the canonical pairs'
    // own lean before removing the image-plane roll along the eye line.
    let (lean_sin, lean_cos) = up_lean().sin_cos();
    let out = plus(times(cross(across, up), lean_cos), times(up, lean_sin));
    let flat = across[0].hypot(across[1]);
    let (tilt_sin, tilt_cos) = if flat > 1e-6 {
        (across[1] / flat, across[0] / flat)
    } else {
        (0.0, 1.0)
    };
    let out = [
        out[0] * tilt_cos + out[1] * tilt_sin,
        -out[0] * tilt_sin + out[1] * tilt_cos,
        out[2],
    ];
    if !out.iter().all(|value| value.is_finite()) || out[2] >= 0.0 {
        return None;
    }
    let head = Head {
        yaw_deg: out[0].atan2(-out[2]).to_degrees(),
        pitch_deg: out[1].atan2(out[0].hypot(out[2])).to_degrees(),
    };
    (head.yaw_deg.is_finite()
        && head.pitch_deg.is_finite()
        && (-90.0..=90.0).contains(&head.yaw_deg)
        && (-90.0..=90.0).contains(&head.pitch_deg))
    .then_some(head)
}

type Vector = [f32; 3];

fn plus(a: Vector, b: Vector) -> Vector {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn minus(a: Vector, b: Vector) -> Vector {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn times(a: Vector, by: f32) -> Vector {
    [a[0] * by, a[1] * by, a[2] * by]
}

fn dot(a: Vector, b: Vector) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: Vector, b: Vector) -> Vector {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(a: Vector) -> Option<Vector> {
    let length = dot(a, a).sqrt();
    (length.is_finite() && length > 1e-3).then(|| times(a, 1.0 / length))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rigid canonical points, not a model output or a labelled human face.
    /// Rotate pitch, then yaw, then image-plane roll; translate and scale.
    fn canonical_mesh(yaw: f32, pitch: f32, roll: f32, scale: f32, offset: Vector) -> Vec<f32> {
        let mut landmarks = vec![0.0; LANDMARKS * 3];
        let (ys, yc) = yaw.to_radians().sin_cos();
        let (ps, pc) = pitch.to_radians().sin_cos();
        let (rs, rc) = roll.to_radians().sin_cos();
        for (index, [x, y, z]) in CANONICAL {
            let (x, y, z) = (x, -y, -z);
            let (y, z) = (y * pc - z * ps, y * ps + z * pc);
            let (x, z) = (x * yc - z * ys, x * ys + z * yc);
            let (x, y) = (x * rc - y * rs, x * rs + y * rc);
            landmarks[index * 3..index * 3 + 3].copy_from_slice(&[
                offset[0] + scale * x,
                offset[1] + scale * y,
                offset[2] + scale * z,
            ]);
        }
        landmarks
    }

    fn square() -> Vec<f32> {
        canonical_mesh(0.0, 0.0, 0.0, 8.0, [128.0, 128.0, 0.0])
    }

    #[test]
    fn canonical_pairs_and_zero_preserve_the_reused_coordinate_convention() {
        let at = |index| CANONICAL.iter().find(|(at, _)| *at == index).unwrap().1;
        for (from, to) in ACROSS {
            let (left, right) = (at(from), at(to));
            assert!(left[0] < 0.0);
            assert_eq!([-left[0], left[1], left[2]], right);
        }
        for (from, to) in UP {
            assert_eq!((at(from)[0], at(to)[0]), (0.0, 0.0));
            assert!(at(to)[1] > at(from)[1]);
        }
        assert!((up_lean().to_degrees() + 8.385).abs() < 0.01);
        let head = mesh_head(&square()).unwrap();
        assert!(head.yaw_deg.abs() < 0.001 && head.pitch_deg.abs() < 0.001);
    }

    #[test]
    fn rigid_pose_signs_survive_roll_scale_and_translation() {
        for yaw in [-80.0, -30.0, 0.0, 30.0, 80.0] {
            for pitch in [-40.0, -20.0, 0.0, 20.0, 40.0] {
                for roll in [-15.0, 0.0, 10.0] {
                    for (scale, offset) in [(2.0, [128.0, 128.0, 0.0]), (8.0, [88.0, 153.0, 300.0])]
                    {
                        let head =
                            mesh_head(&canonical_mesh(yaw, pitch, roll, scale, offset)).unwrap();
                        assert!(
                            (head.yaw_deg - yaw).abs() < 0.01,
                            "{yaw} {pitch} {roll} {scale}: {head:?}"
                        );
                        assert!(
                            (head.pitch_deg - pitch).abs() < 0.01,
                            "{yaw} {pitch} {roll} {scale}: {head:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn length_is_exact_not_a_partial_or_trailing_mesh() {
        let mut mesh = square();
        for size in [0, 1, LANDMARKS * 3 - 3, LANDMARKS * 3 - 1] {
            assert_eq!(mesh_head(&mesh[..size]), None);
        }
        mesh.push(0.0);
        assert_eq!(mesh_head(&mesh), None);
    }

    #[test]
    fn nonfinite_values_anywhere_are_unknown_including_unused_landmarks() {
        for index in [33 * 3, 151 * 3 + 2, 13 * 3, LANDMARKS * 3 - 1] {
            for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut mesh = square();
                mesh[index] = invalid;
                assert_eq!(mesh_head(&mesh), None);
            }
        }
    }

    #[test]
    fn degenerate_and_overflowing_directions_are_unknown() {
        assert_eq!(mesh_head(&vec![0.0; LANDMARKS * 3]), None);
        assert_eq!(mesh_head(&vec![128.0; LANDMARKS * 3]), None);
        // Preserve across, but make both up pairs parallel to it.
        let mut parallel = square();
        for (from, to) in UP {
            parallel[from * 3..from * 3 + 3].copy_from_slice(&[0.0, 0.0, 0.0]);
            parallel[to * 3..to * 3 + 3].copy_from_slice(&[1.0, 0.0, 0.0]);
        }
        assert_eq!(mesh_head(&parallel), None);
        for huge in [1e30, f32::MAX] {
            let mut overflow = square();
            overflow[33 * 3] = -huge;
            overflow[263 * 3] = huge;
            assert_eq!(mesh_head(&overflow), None);
        }
    }

    #[test]
    fn reflected_landmark_order_or_a_back_facing_normal_is_unknown() {
        for axis in [0, 1] {
            let mut reflected = square();
            for point in reflected.chunks_exact_mut(3) {
                point[axis] = 256.0 - point[axis];
            }
            assert_eq!(mesh_head(&reflected), None);
        }
        let back = canonical_mesh(180.0, 0.0, 0.0, 8.0, [128.0, 128.0, 0.0]);
        assert_eq!(mesh_head(&back), None);
    }
}
