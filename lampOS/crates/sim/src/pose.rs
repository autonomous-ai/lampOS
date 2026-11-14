//! Where the head points, from the servos' counts and the lamp's kinematic
//! model (`docs/lamp.model.json`, the same file the SDKs' twin draws with):
//! the rotation of the head, and what the camera in it sees relative to the
//! pose in which it faces the person.
use serde_json::Value;

/// Counts in a turn.
const COUNTS_A_TURN: f64 = 4096.0;

/// The found pose of lamp-fc4b, which the simulated servos start in (counts for
/// base yaw, shoulder, elbow, head roll, head pitch): the zero of each joint's
/// degrees, the body's own convention (`control/src/body_info.rs`).
pub const HOME: [f64; 5] = [2028.0, 2048.0, 1946.0, 2085.0, 1817.0];
/// The joints turn the way their counts grow, but the head's pitch (up is fewer).
const SIGN: [f64; 5] = [1.0, 1.0, 1.0, 1.0, -1.0];
/// The pose in which the lamp stands facing the person (shoulder and elbow raised
/// to their stand counts, the rest at home): the camera sees its normal picture.
pub const STAND: [f64; 5] = [2028.0, 2621.0, 2558.0, 2085.0, 1817.0];

/// Quaternion `[x, y, z, w]`.
type Quat = [f64; 4];

fn mul(a: Quat, b: Quat) -> Quat {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn axis_angle(axis: [f64; 3], radians: f64) -> Quat {
    let (s, c) = ((radians / 2.0).sin(), (radians / 2.0).cos());
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}

/// A 3x3 rotation matrix, row-major.
pub type Mat3 = [[f64; 3]; 3];

fn matrix(q: Quat) -> Mat3 {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let [x, y, z, w] = [q[0] / n, q[1] / n, q[2] / n, q[3] / n];
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

pub fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

pub fn transpose(a: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = a[j][i];
        }
    }
    out
}

/// The chain of joints from the model, base first.
pub struct Model {
    /// For each of yaw, shoulder, elbow, roll, pitch (the servo order): its
    /// place in the chain, rest rotation, axis, sign and zero.
    joints: Vec<Joint>,
    /// Chain order as indexes into the servo order.
    order: Vec<usize>,
}

struct Joint {
    rest: Quat,
    axis: [f64; 3],
    sign: f64,
    zero: f64,
}

impl Model {
    /// The model shipped in `docs/lamp.model.json`.
    pub fn shipped() -> Model {
        Model::parse(include_str!("../../../docs/lamp.model.json")).expect("the shipped model")
    }

    pub fn parse(text: &str) -> Option<Model> {
        let model: Value = serde_json::from_str(text).ok()?;
        let joints = model["joints"].as_array()?;
        // Servo order: yaw, shoulder, elbow, roll, pitch.
        let names = ["yaw", "shoulder", "elbow", "roll", "pitch"];
        let mut by_servo = Vec::new();
        for name in names {
            let j = joints.iter().find(|j| j["name"] == name)?;
            let f = |key: &str, k: usize| j[key][k].as_f64();
            by_servo.push((
                name,
                j["parent"].as_str()?.to_owned(),
                Joint {
                    rest: [
                        f("rest_quat", 0)?,
                        f("rest_quat", 1)?,
                        f("rest_quat", 2)?,
                        f("rest_quat", 3)?,
                    ],
                    axis: [f("axis", 0)?, f("axis", 1)?, f("axis", 2)?],
                    sign: j["sign"].as_f64()?,
                    zero: j["zero_deg"].as_f64()?,
                },
            ));
        }
        // Base first: each joint after its parent.
        let mut order = Vec::new();
        let mut parent = "base".to_owned();
        while order.len() < by_servo.len() {
            let at = by_servo.iter().position(|(_, p, _)| *p == parent)?;
            order.push(at);
            parent = by_servo[at].0.to_owned();
        }
        Some(Model {
            joints: by_servo.into_iter().map(|(_, _, j)| j).collect(),
            order,
        })
    }

    /// The head's rotation in the world, for servo `counts`.
    pub fn head(&self, counts: [f64; 5]) -> Mat3 {
        let mut q: Quat = [0.0, 0.0, 0.0, 1.0];
        for &i in &self.order {
            let joint = &self.joints[i];
            let deg = SIGN[i] * (counts[i] - HOME[i]) * 360.0 / COUNTS_A_TURN;
            let turn = axis_angle(joint.axis, (joint.sign * (deg - joint.zero)).to_radians());
            q = mul(q, mul(joint.rest, turn));
        }
        matrix(q)
    }
}

/// The matrix that carries a ray in the camera now to the same ray in the camera
/// at the pose that faces the person: `Bt Rref^T Rnow B`.
pub fn relative(model: &Model, now: [f64; 5], gain: f64) -> Mat3 {
    // The real joints move a few degrees: a gain makes the same movement a bigger turn.
    let now = std::array::from_fn(|i| STAND[i] + gain * (now[i] - STAND[i]));
    let b = camera_in_head();
    let r_now = model.head(now);
    let r_ref = model.head(STAND);
    mat_mul(
        &transpose(&b),
        &mat_mul(&transpose(&r_ref), &mat_mul(&r_now, &b)),
    )
}

/// Camera axes (columns) in the head frame: right = -x, down = +z, forward = +y
/// (sim-2's check of the render: with right = +x the picture was mirrored and
/// "down" pointed at the sky). Right x down = forward: a proper rotation.
pub fn camera_in_head() -> Mat3 {
    // column 0 = [-1,0,0]; column 1 = [0,0,1]; column 2 = [0,1,0]
    [[-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn angle_of(m: &Mat3) -> f64 {
        // The angle between the optical axes: the z axis of the camera mapped.
        m[2][2].clamp(-1.0, 1.0).acos().to_degrees()
    }

    #[test]
    fn at_the_stand_pose_the_camera_faces_the_person() {
        let model = Model::shipped();
        let m = relative(&model, STAND, 1.0);
        assert!(angle_of(&m) < 0.01, "{m:?}");
    }

    #[test]
    fn a_turn_of_the_base_turns_the_view_by_what_the_camera_height_allows() {
        // A turn about the vertical axis turns an optical axis that points up
        // by less than the turn: cos(angle) = sin^2 e + cos^2 e cos t, for elevation e.
        let model = Model::shipped();
        let b = camera_in_head();
        let forward = mat_mul(&model.head(STAND), &b);
        let elevation = forward[1][2].clamp(-1.0, 1.0).asin();
        let mut now = STAND;
        now[0] += 30.0 * COUNTS_A_TURN / 360.0;
        let turned = angle_of(&relative(&model, now, 1.0));
        let t = 30f64.to_radians();
        let want = (elevation.sin().powi(2) + elevation.cos().powi(2) * t.cos())
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees();
        assert!(
            (turned - want).abs() < 1.0,
            "turned {turned}, wanted {want}"
        );
        assert!(turned > 1.0);
    }

    #[test]
    fn folded_down_the_head_keeps_the_way_it_faced_standing() {
        // The model's shoulder turns one way and its elbow the other as the arm
        // folds (lamp-75c9, 11 Nov: the lower arm lies low over the base, the
        // two segments close by the elbow's 43 deg of counts, and the head ends up on
        // top), so the head comes down with its direction nearly the stand's: the
        // two arm turns cancel to a few degrees. (With the shoulder's old sign the
        // head tipped by about 95 deg and the shade went through the base.)
        let model = Model::shipped();
        let folded = angle_of(&relative(&model, HOME, 1.0));
        assert!(folded < 10.0, "{folded}");
    }

    #[test]
    fn the_picture_moves_the_way_the_head_turns() {
        // Print where the middle of the picture comes from when the base turns,
        // at the stand pose, as a reference for the JS twin (sim-2's reference).
        let model = Model::shipped();
        let mut now = STAND;
        now[0] += 30.0 * COUNTS_A_TURN / 360.0;
        let m = relative(&model, now, 1.0);
        let from = crate::warp::source_of(&m, 160.0, 90.0, 320, 180).unwrap();
        // The base turned by 30 degrees: the middle of the picture comes from the
        // left of the old middle, and nearly the same height. sim-2's JS reference
        // (the same maths, the model's own axes) gives (130.0, 94.8).
        assert!(
            from.0 < 160.0 && (from.0 - 160.0).abs() > 3.0 * (from.1 - 90.0).abs(),
            "{from:?}"
        );
        assert!(
            (from.0 - 130.0).abs() < 6.0 && (from.1 - 94.8).abs() < 6.0,
            "{from:?}"
        );
        // The head's pitch and yaw signs are the model's (a CAD guess until a photo of
        // a lamp checks them): no direction is asserted for them here.
    }
}
