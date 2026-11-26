//! Whether a face is turned toward the lamp, and whether someone has been
//! facing it for the last moment: the evidence a person gives, without
//! meaning to, that what they are saying is said to the lamp.
//!
//! V1 had this (`hal/drivers/tracking/gaze.py`) and used it when speech
//! began. It needs no model beyond the face detector: the detector already
//! gives the two eyes and the nose, and how far the nose sits from the
//! point between the eyes, along the line of the eyes, is the sine of how
//! far the head is turned. It says nothing of where the eyes themselves
//! look. With `LAMP_EYES_PITCH_GAZE=on` (#7442) the same marks also say
//! how far the head is tipped up or down ([`pitch_deg`]).
//!
//! Nothing here decides anything. It turns marks into an angle, an angle
//! into "facing or not" for one look, and a run of looks into a vote. The
//! numbers are V1's and were tuned on V1's lamps at twice this picture's
//! width; none has been checked on this lamp.
//!
//! At desk distance the marks are a few pixels apart, and a pixel's wobble
//! in the nose moves the turn 10 to 20 degrees (lamp-74ff, 26 Nov: 60% of
//! 1,807 reads `turned 60+ deg` with the owner facing the lamp). With
//! `LAMP_EYES_FACING_FROM_MESH=on` ([`FROM_MESH_ENVIRONMENT`]) the face the
//! mouth's Face Mesh V2 already meshed in a look is turned and tipped as
//! its mesh says instead ([`mesh_head`]): no second model run, and every
//! other face as the marks say.

/// A face turned further than this is not facing the lamp. V1's default
/// was 25 and its lamps were set to 60 after trying them with people: a
/// wide lens and a person looking at a screen beside the lamp.
pub const MAX_YAW_DEG: f32 = 60.0;
/// Toward the edge of the picture the lens shows a face turned even when
/// it looks straight at the lamp, so the allowance grows to this many
/// times `MAX_YAW_DEG` at the very edge (and is held to 90).
pub const EDGE_SCALE: f32 = 1.8;
/// A face narrower than this share of the picture's width has marks too
/// close together to measure a turn from. V1: 48 pixels of 640.
pub const LEAST_WIDTH: f32 = 0.075;
/// The vote looks this far back, needs this many looks, and this share of
/// them facing.
pub const WINDOW_US: u64 = 1_500_000;
pub const LEAST_LOOKS: usize = 2;
pub const LEAST_FACING: f32 = 0.6;

/// How far the head is turned, in degrees from straight at the camera, to
/// either side. `marks` are the eye on the left of the picture, the eye on
/// the right and the nose. None when the eyes are on top of one another or
/// a mark is not a number.
pub fn yaw_deg(marks: &[(f32, f32); 5]) -> Option<f32> {
    let (first, second, nose) = (marks[0], marks[1], marks[2]);
    if [first, second, nose]
        .iter()
        .any(|mark| !mark.0.is_finite() || !mark.1.is_finite())
    {
        return None;
    }
    let (across, down) = (second.0 - first.0, second.1 - first.1);
    let apart = across.hypot(down);
    if apart < 1e-6 {
        return None;
    }
    let between = ((first.0 + second.0) / 2.0, (first.1 + second.1) / 2.0);
    // The nose's distance from the middle, along the line of the eyes.
    let along = ((nose.0 - between.0) * across + (nose.1 - between.1) * down) / apart;
    // Past an eye the marks have nothing more to say.
    Some(
        (along / (apart / 2.0))
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees()
            .abs(),
    )
}

/// How far below the eye line the nose sits on a level face, as a share of
/// the distance between the eyes: the zero of [`pitch_deg`]. Set on this
/// lamp's YuNet: row-7442a, lamp-52e6, 17 Nov, a person facing the lamp
/// level read Q1-Q5 medians 7.8-17.6 degrees down (12.65, 9.1, 9.2, 7.75,
/// 17.65; 234 looks, pooled median 10.75) with the 0.57 of the five-point
/// template faces are aligned to for recognition (InsightFace's ArcFace
/// 112x112: eyes at y 51.70 and 51.50, 35.24 apart, nose at 71.74). YuNet
/// puts the nose lower than that template: the nose drop that the model
/// gives a face tipped 10.75 degrees, against the mouth's unchanged level,
/// is `1.15 (0.57 + 0.5 tan 10.75) / (1.15 + 0.2 tan 10.75)` = 0.644, so
/// 0.64, and that face now reads within a degree of 0. The template itself
/// then reads about 10 degrees up.
pub const LEVEL_NOSE_DROP: f32 = 0.64;
/// How far in front of the eyes the nose's tip stands, as a share of the
/// distance between them: a head tipped down swings it down, tipped up,
/// up. An anthropometric guess (about 30 mm against 63 mm), for the row.
pub const NOSE_DEPTH: f32 = 0.5;
/// How far below the eye line the mouth corners' midpoint sits on a level
/// face, in eye distances: the same ArcFace template (corners at y 92.37
/// and 92.20, 40.69 below the eyes' 51.60).
pub const LEVEL_MOUTH_DROP: f32 = 1.15;
/// How far in front of the eyes the mouth corners stand, in eye distances:
/// less than the nose's tip. A guess, for the row.
pub const MOUTH_DEPTH: f32 = 0.2;
/// Pitch is measured only up to this turn: past it the marks crowd into
/// one side of the face and say too little.
pub const PITCH_MOST_YAW_DEG: f32 = 60.0;
/// What [`pitch_deg`] gives for marks no head could make (a mark not a
/// number, the eyes on top of one another, the mouth above the eyes with
/// the nose below them): straight down, outside every window, so a lost
/// pitch never counts as facing. Not the same as not measured (`None`).
pub const LOST_DEG: f32 = 90.0;
/// A face tipped further down than this is not facing the lamp: a head
/// bent to a phone or the desk (#7442). Authored, to be set on the row.
pub const MAX_PITCH_DOWN_DEG: f32 = 25.0;
/// A face tipped further up than this is not facing the lamp (#7442).
/// Authored, to be set on the row.
pub const MAX_PITCH_UP_DEG: f32 = 20.0;

/// How far the head is tipped, in degrees: positive down, negative up, as
/// ROS has pitch (REP 103: about the head's left axis, nose down positive).
/// `marks` are YuNet's five: the eye on the left of the picture, the eye on
/// the right, the nose, and the two mouth corners.
///
/// The nose's drop `r` and the mouth corners' midpoint's drop `m` below the
/// point between the eyes, square to the eye line (so a head tilted over
/// reads the same). A head tipped by `t` gives, with `c = cos t` and
/// `s = sin t`, `r = LEVEL_NOSE_DROP c + NOSE_DEPTH s` and
/// `m = LEVEL_MOUTH_DROP c + MOUTH_DEPTH s`: two lines in `c` and `s`,
/// solved for them, and `t = atan2(s, c)`. That is one answer for every
/// bow, so the reading rises with it to straight down (the nose alone
/// folded back past about 41 degrees: the Reviewer-lead's P3 on #7450).
/// Only the ratio of `s` to `c` counts, so the face's size and the turn's
/// narrowing of it (both drops alike) fall out. A level face, `r` and `m`
/// at [`LEVEL_NOSE_DROP`] and [`LEVEL_MOUTH_DROP`] eye distances, reads 0.
///
/// [`LOST_DEG`] for marks no head could make, `c` not over 0 among them;
/// None, not measured, for a head turned past [`PITCH_MOST_YAW_DEG`].
pub fn pitch_deg(marks: &[(f32, f32); 5]) -> Option<f32> {
    let (first, second, nose) = (marks[0], marks[1], marks[2]);
    let (across, down) = (second.0 - first.0, second.1 - first.1);
    let apart = across.hypot(down);
    if marks
        .iter()
        .any(|mark| !mark.0.is_finite() || !mark.1.is_finite())
        || apart < 1e-6
    {
        return Some(LOST_DEG);
    }
    if yaw_deg(marks)? > PITCH_MOST_YAW_DEG {
        return None;
    }
    let between = ((first.0 + second.0) / 2.0, (first.1 + second.1) / 2.0);
    // Square to the eye line, toward the bottom of the picture.
    let drop = |at: (f32, f32)| (at.0 - between.0) * -down + (at.1 - between.1) * across;
    let mouth = (
        (marks[3].0 + marks[4].0) / 2.0,
        (marks[3].1 + marks[4].1) / 2.0,
    );
    let (r, m) = (drop(nose), drop(mouth));
    let det = LEVEL_NOSE_DROP * MOUTH_DEPTH - NOSE_DEPTH * LEVEL_MOUTH_DROP;
    let c = (r * MOUTH_DEPTH - NOSE_DEPTH * m) / det;
    let s = (LEVEL_NOSE_DROP * m - LEVEL_MOUTH_DROP * r) / det;
    if c <= 0.0 {
        return Some(LOST_DEG);
    }
    Some(s.atan2(c).to_degrees())
}

/// Whether a face tipped `pitch` degrees ([`pitch_deg`]) is inside the
/// window that counts as facing the lamp. A pitch not measured is not
/// judged here: that is the reader's to leave unknown.
pub fn pitch_within(pitch: f32) -> bool {
    (-MAX_PITCH_UP_DEG..=MAX_PITCH_DOWN_DEG).contains(&pitch)
}

/// Whether the eyes and the nose were all inside the picture: a mark the
/// detector placed outside it was guessed, not seen.
pub fn marks_seen(marks: &[(f32, f32); 5], width: f32, height: f32) -> bool {
    marks[..3].iter().all(|mark| {
        mark.0.is_finite()
            && mark.1.is_finite()
            && (0.0..=width).contains(&mark.0)
            && (0.0..=height).contains(&mark.1)
    })
}

/// The turn allowed to a face this far from the middle of the picture:
/// 0 at the middle, 1 at an edge.
pub fn allowed_deg(from_middle: f32) -> f32 {
    (MAX_YAW_DEG * (1.0 + (EDGE_SCALE - 1.0) * from_middle.clamp(0.0, 1.0))).min(90.0)
}

/// One look at one face: is it facing the lamp. A face too small to
/// measure never counts, for or against, and that is the caller's to
/// leave out of the vote: here it is simply not facing.
pub fn facing(yaw: Option<f32>, width_share: f32, from_middle: f32) -> bool {
    yaw.is_some_and(|yaw| width_share >= LEAST_WIDTH && yaw <= allowed_deg(from_middle))
}

/// Where the eyes read whether a face meshed in a look is turned and tipped
/// as its Face Mesh V2 points say ([`mesh_head`]), not as its five marks
/// say: `on` exactly, off unless set. A face with no mesh that look (not
/// the largest, or `--mouth-on-heard` with no line heard), a mesh that
/// can't be read, or the switch off: the marks, unchanged. A face too small
/// for the marks to measure ([`LEAST_WIDTH`]) has no turn either way.
pub const FROM_MESH_ENVIRONMENT: &str = "LAMP_EYES_FACING_FROM_MESH";

/// Whether [`FROM_MESH_ENVIRONMENT`] is on in this process's environment.
pub fn from_mesh_on() -> bool {
    std::env::var(FROM_MESH_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Which way a face's turn was read in a look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TurnFrom {
    /// Its five marks ([`yaw_deg`]).
    #[default]
    Marks,
    /// Its mesh ([`mesh_head`]).
    Mesh,
}

impl TurnFrom {
    /// The word the journal says it with.
    pub fn word(self) -> &'static str {
        match self {
            Self::Marks => "marks",
            Self::Mesh => "mesh",
        }
    }
}

/// How a head is turned and tipped, from its mesh ([`mesh_head`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Head {
    /// Turned, in degrees: 0 facing the camera, positive toward the
    /// picture's right (the face's own left), as the bus's `yaw`.
    pub yaw_deg: f32,
    /// Tipped, in degrees: positive down, as [`pitch_deg`].
    pub pitch_deg: f32,
}

/// How far a face is turned in a look, either way, and which way that was
/// read: its mesh's turn when it has a [`Head`], else its marks'
/// ([`yaw_deg`]).
pub fn turn(marks: &[(f32, f32); 5], head: Option<Head>) -> (Option<f32>, TurnFrom) {
    match head {
        Some(head) => (Some(head.yaw_deg.abs()), TurnFrom::Mesh),
        None => (yaw_deg(marks), TurnFrom::Marks),
    }
}

/// How far a face is tipped in a look: its mesh's pitch when it has a
/// [`Head`], else its marks' ([`pitch_deg`]). Not measured, either way, for
/// a head turned past [`PITCH_MOST_YAW_DEG`].
pub fn tip(marks: &[(f32, f32); 5], head: Option<Head>) -> Option<f32> {
    match head {
        Some(head) => (head.yaw_deg.abs() <= PITCH_MOST_YAW_DEG).then_some(head.pitch_deg),
        None => pitch_deg(marks),
    }
}

/// The mesh's points across the face, each pair from the point on the
/// picture's left to its mirror image on the right: the outer eye corners,
/// the inner ones and the sides of the nose. Neither the lids nor the
/// irises (where the eyes look is not read), the brows, the lips or the
/// jaw: blinking, speaking and smiling move none of these.
const ACROSS: [(usize, usize); 3] = [(33, 263), (133, 362), (129, 358)];
/// The mesh's points up the face's middle, each pair from a point at the
/// nose's base to one above it: between the brows, and the forehead's
/// middle. Bone and cartilage the jaw doesn't move.
const UP: [(usize, usize); 2] = [(2, 9), (94, 151)];
/// Where [`ACROSS`]'s and [`UP`]'s points sit on MediaPipe's canonical face
/// (`mediapipe/modules/face_geometry/data/canonical_face_model.obj`,
/// Apache-2.0 as the model is), by their index in the mesh: centimetres, x
/// to the face's own left, y up, z out of the face. Only [`UP`]'s lean is
/// read from it ([`up_lean`]); each of [`ACROSS`]'s pairs is a mirror pair
/// there, as on any face the same on both sides.
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

/// How far [`UP`]'s pairs, summed, lean out of the canonical face, in
/// radians: about -8.4 degrees, their tops set back.
fn up_lean() -> f32 {
    let at = |index: usize| {
        CANONICAL
            .iter()
            .find(|(at, _)| *at == index)
            .map_or([0.0; 3], |(_, point)| *point)
    };
    let (mut up, mut out) = (0.0f32, 0.0f32);
    for (from, to) in UP {
        let (from, to) = (at(from), at(to));
        up += to[1] - from[1];
        out += to[2] - from[2];
    }
    out.atan2(up)
}

/// How a head is turned and tipped, from its Face Mesh V2 points: the 478
/// of [`crate::mesh::Mesh::landmarks_of`], each x and y in the crop's
/// pixels (the crop turned so the detector's eyes are level) and z, depth
/// away from the camera, which the model gives at x's scale (MediaPipe's
/// own face geometry reads it so). `None` for points not all there or not
/// numbers, or that no head seen from the front could give.
///
/// A closed form from points nothing on the face moves (the product
/// owner's ask, 26 Nov):
/// - **across**, from the head's right to its left: the sum of three
///   pairs ([`ACROSS`]). On a face the same on both sides each pair runs
///   straight across it, whatever the face's shape, so no face model sets
///   the turn's zero;
/// - **up**: the sum of two pairs up the face's middle ([`UP`]), less its
///   part across;
/// - **out of the face**, toward the camera: across times up, leaned as
///   far as up leans on MediaPipe's canonical face ([`up_lean`]), so that
///   face, square to the camera, reads 0 and 0.
///
/// The face's tilt in the picture is taken out along its own eye line
/// (across), then the turn is how far out of the face points to the side
/// and the tip how far down. Exact for any turn and tip of a rigid face
/// at any size and place in the crop (the tests). A turn or a tip is read
/// from the whole of each pair, many crop pixels long: on a face with its
/// eyes 10 of the picture's pixels apart, meshed at 256 from about 50, one
/// picture pixel of error on every point, each its own way, moves the turn
/// about 5 degrees (a Monte Carlo of the canonical face), where one pixel
/// on the nose mark alone moves [`yaw_deg`]'s 11.5. The depth's scale is
/// the model's: read at half of x's, 70 degrees would read 54. Not yet
/// checked against a person on a lamp.
pub fn mesh_head(landmarks: &[f32]) -> Option<Head> {
    if landmarks.len() != crate::mesh::LANDMARKS * 3 {
        return None;
    }
    let point = |at: usize| {
        [
            landmarks[at * 3],
            landmarks[at * 3 + 1],
            landmarks[at * 3 + 2],
        ]
    };
    let sum = |pairs: &[(usize, usize)]| {
        pairs.iter().fold([0.0; 3], |sum, &(from, to)| {
            plus(sum, minus(point(to), point(from)))
        })
    };
    let (across, up) = (sum(&ACROSS), sum(&UP));
    if !across.iter().chain(&up).all(|value| value.is_finite()) {
        return None;
    }
    let across = unit(across)?;
    // Square to across.
    let up = unit(minus(up, times(across, dot(up, across))))?;
    // Out of the face, toward the camera (the crop's z is away from it).
    let (lean_sin, lean_cos) = up_lean().sin_cos();
    let out = plus(times(cross(across, up), lean_cos), times(up, lean_sin));
    // The face's tilt in the picture taken out, along its own eye line.
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
    // A face seen from the front looks into the camera's half.
    if out[2] >= 0.0 {
        return None;
    }
    Some(Head {
        yaw_deg: out[0].atan2(-out[2]).to_degrees(),
        pitch_deg: out[1].atan2(out[0].hypot(out[2])).to_degrees(),
    })
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

/// `a` made a unit long; none for one too short to have a direction.
fn unit(a: Vector) -> Option<Vector> {
    let length = dot(a, a).sqrt();
    (length > 1e-3).then(|| times(a, 1.0 / length))
}

/// Each face's [`Head`] from this look's meshes, by its index among the
/// faces found (`meshed`: each meshed face's index and points): none for a
/// face not meshed or whose mesh can't be read, and none at all with `on`
/// false ([`FROM_MESH_ENVIRONMENT`]).
pub fn mesh_heads(on: bool, faces: usize, meshed: &[(usize, Vec<f32>)]) -> Vec<Option<Head>> {
    let mut heads = vec![None; faces];
    if on {
        for (index, landmarks) in meshed {
            if let Some(head) = heads.get_mut(*index) {
                *head = mesh_head(landmarks);
            }
        }
    }
    heads
}

/// A mesh of the canonical face ([`CANONICAL`]'s points, every other point
/// 0) as the mouth's crop would hold it: tipped `pitch` degrees (down
/// positive), then turned `yaw` (toward the picture's right positive), then
/// tilted `roll` in the picture's plane, `scale` crop pixels a centimetre,
/// its middle at the crop's.
#[cfg(test)]
pub(crate) fn canonical_mesh(yaw: f32, pitch: f32, roll: f32, scale: f32) -> Vec<f32> {
    let mut landmarks = vec![0.0; crate::mesh::LANDMARKS * 3];
    let (yaw_sin, yaw_cos) = yaw.to_radians().sin_cos();
    let (pitch_sin, pitch_cos) = pitch.to_radians().sin_cos();
    let (roll_sin, roll_cos) = roll.to_radians().sin_cos();
    for (index, [x, y, z]) in CANONICAL {
        // Square to the camera, the crop's x is the face's own left, its y
        // down and its z away from the camera.
        let (x, y, z) = (x, -y, -z);
        let (y, z) = (y * pitch_cos - z * pitch_sin, y * pitch_sin + z * pitch_cos);
        let (x, z) = (x * yaw_cos - z * yaw_sin, x * yaw_sin + z * yaw_cos);
        let (x, y) = (x * roll_cos - y * roll_sin, x * roll_sin + y * roll_cos);
        landmarks[index * 3..index * 3 + 3].copy_from_slice(&[
            128.0 + scale * x,
            128.0 + scale * y,
            scale * z,
        ]);
    }
    landmarks
}

/// The looks of the last moment, for the vote taken when someone begins to
/// speak.
#[derive(Default)]
pub struct Recent {
    looks: std::collections::VecDeque<(u64, bool)>,
}

impl Recent {
    /// One look in which a face large enough to measure was seen.
    pub fn saw(&mut self, at_us: u64, facing: bool) {
        self.looks.push_back((at_us, facing));
        self.forget(at_us);
    }

    fn forget(&mut self, now_us: u64) {
        while self
            .looks
            .front()
            .is_some_and(|(at, _)| now_us.saturating_sub(*at) > WINDOW_US)
        {
            self.looks.pop_front();
        }
    }

    /// Whether someone has been facing the lamp: none when there were too
    /// few looks to say, which is not the same as nobody facing it.
    pub fn vote(&mut self, now_us: u64) -> Option<bool> {
        self.forget(now_us);
        if self.looks.len() < LEAST_LOOKS {
            return None;
        }
        let facing = self.looks.iter().filter(|(_, facing)| *facing).count();
        Some(facing as f32 / self.looks.len() as f32 >= LEAST_FACING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face 100 across with its eyes 40 apart and its nose `turn` of the
    /// way from the middle toward the right eye (1) or the left (-1).
    fn marks(turn: f32) -> [(f32, f32); 5] {
        let nose = 50.0 + 20.0 * turn;
        [
            (30.0, 40.0),
            (70.0, 40.0),
            (nose, 60.0),
            (35.0, 80.0),
            (65.0, 80.0),
        ]
    }

    #[test]
    fn the_nose_between_the_eyes_is_straight_and_at_an_eye_is_side_on() {
        assert!(yaw_deg(&marks(0.0)).unwrap() < 0.01);
        assert!((yaw_deg(&marks(0.5)).unwrap() - 30.0).abs() < 0.01);
        assert!((yaw_deg(&marks(-0.5)).unwrap() - 30.0).abs() < 0.01);
        assert!((yaw_deg(&marks(1.0)).unwrap() - 90.0).abs() < 0.01);
        // Past the eye is held at side on.
        assert!((yaw_deg(&marks(1.7)).unwrap() - 90.0).abs() < 0.01);
        // A head tilted over: the turn is measured along the tilted eyes.
        let tilted = [
            (30.0, 30.0),
            (70.0, 50.0),
            (45.0, 60.0),
            (0.0, 0.0),
            (0.0, 0.0),
        ];
        let straight = yaw_deg(&tilted).unwrap();
        assert!(straight < 12.0, "{straight}");
        // No eyes to measure from, or a mark that is no number.
        let mut none = marks(0.0);
        none[1] = none[0];
        assert_eq!(yaw_deg(&none), None);
        let mut broken = marks(0.0);
        broken[2].0 = f32::NAN;
        assert_eq!(yaw_deg(&broken), None);
    }

    /// Marks of a face with its eyes 40 apart, level, its head tipped
    /// `tip` degrees (down positive) by the model [`pitch_deg`] inverts:
    /// the nose and the mouth corners each dropped by their own depth.
    fn tipped(tip: f32) -> [(f32, f32); 5] {
        let (sin, cos) = tip.to_radians().sin_cos();
        let nose = 40.0 * (LEVEL_NOSE_DROP * cos + NOSE_DEPTH * sin);
        let mouth = 40.0 * (LEVEL_MOUTH_DROP * cos + MOUTH_DEPTH * sin);
        [
            (30.0, 40.0),
            (70.0, 40.0),
            (50.0, 40.0 + nose),
            (35.0, 40.0 + mouth),
            (65.0, 40.0 + mouth),
        ]
    }

    #[test]
    fn a_level_face_reads_no_pitch_and_a_nose_low_reads_head_down() {
        assert!(pitch_deg(&tipped(0.0)).unwrap().abs() < 0.01);
        // The ArcFace template, the zero before row-7442a, now reads about
        // 10 degrees up: YuNet on the lamp puts the nose lower.
        let template = [
            (38.29, 51.70),
            (73.53, 51.50),
            (56.03, 71.74),
            (41.55, 92.37),
            (70.73, 92.20),
        ];
        let level = pitch_deg(&template).unwrap();
        assert!((-12.0..-8.0).contains(&level), "{level}");
        for tip in [-30.0, -20.0, -10.0, 10.0, 20.0, 30.0] {
            let read = pitch_deg(&tipped(tip)).unwrap();
            assert!((read - tip).abs() < 0.01, "{tip}: {read}");
        }
        // A nose lower than level is down, higher is up.
        let mut low = tipped(0.0);
        low[2].1 += 6.0;
        assert!(pitch_deg(&low).unwrap() > 10.0);
        let mut high = tipped(0.0);
        high[2].1 -= 6.0;
        assert!(pitch_deg(&high).unwrap() < -10.0);
        // Bowed to a phone, or looking up at a shelf: outside the window.
        assert!(!pitch_within(pitch_deg(&tipped(35.0)).unwrap()));
        assert!(!pitch_within(pitch_deg(&tipped(-25.0)).unwrap()));
        assert!(pitch_within(pitch_deg(&tipped(15.0)).unwrap()));
        assert!(pitch_within(pitch_deg(&tipped(-15.0)).unwrap()));
    }

    /// Row-7442a on lamp-52e6 (17 Nov): a face facing the lamp level read a
    /// pooled median of 10.75 degrees down with the template's nose drop
    /// (0.57). Those marks, made by the model with 0.57 at 10.75 degrees,
    /// now read within 3 degrees of level.
    #[test]
    fn the_rows_level_face_reads_level() {
        let (sin, cos) = 10.75f32.to_radians().sin_cos();
        let nose = 40.0 * (0.57 * cos + NOSE_DEPTH * sin);
        let mouth = 40.0 * (LEVEL_MOUTH_DROP * cos + MOUTH_DEPTH * sin);
        let measured = [
            (30.0, 40.0),
            (70.0, 40.0),
            (50.0, 40.0 + nose),
            (35.0, 40.0 + mouth),
            (65.0, 40.0 + mouth),
        ];
        let read = pitch_deg(&measured).unwrap();
        assert!(read.abs() < 3.0, "{read}");
        assert!(pitch_within(read));
    }

    /// The Reviewer-lead's P3 on #7450: the nose alone folds past about 41
    /// degrees down, so a phone's deep bow read back inside the window. With
    /// the mouth too, the reading rises with the bow all the way down.
    #[test]
    fn a_deep_bow_reads_deeper_never_back_inside_the_window() {
        let tips = [0.0, 20.0, 45.0, 60.0, 75.0];
        let reads: Vec<f32> = tips
            .iter()
            .map(|&tip| pitch_deg(&tipped(tip)).unwrap())
            .collect();
        assert!(reads.windows(2).all(|pair| pair[0] < pair[1]), "{reads:?}");
        for (read, tip) in reads.iter().zip(tips) {
            assert!((read - tip).abs() < 0.01, "{tip}: {read}");
        }
        assert!(!pitch_within(reads[3]) && !pitch_within(reads[4]));
        // Head far back too: never back inside.
        let up = pitch_deg(&tipped(-60.0)).unwrap();
        assert!((up + 60.0).abs() < 0.01 && !pitch_within(up), "{up}");
    }

    /// Marks that can't be a face read as lost, outside the window: never
    /// facing. Only a turn past sixty is not measured at all.
    #[test]
    fn marks_that_cannot_be_read_are_lost_and_not_facing() {
        assert!(!pitch_within(LOST_DEG));
        let mut nan_nose = tipped(0.0);
        nan_nose[2].1 = f32::NAN;
        let mut nan_mouth = tipped(0.0);
        nan_mouth[4].0 = f32::NAN;
        let mut no_eyes = tipped(0.0);
        no_eyes[1] = no_eyes[0];
        // The mouth above the eyes and the nose below them: no head does it.
        let mut flipped = tipped(0.0);
        flipped[3].1 = 10.0;
        flipped[4].1 = 10.0;
        for marks in [nan_nose, nan_mouth, no_eyes, flipped] {
            assert_eq!(pitch_deg(&marks), Some(LOST_DEG), "{marks:?}");
        }
        // Turned past sixty: not measured.
        let mut side_on = tipped(0.0);
        side_on[2].0 = 69.0;
        assert_eq!(pitch_deg(&side_on), None);
    }

    #[test]
    fn pitch_is_the_same_with_the_head_tilted_over_or_turned_a_little() {
        // The whole face turned 30 degrees in the picture's plane.
        let (sin, cos) = 30f32.to_radians().sin_cos();
        let mut tilted = tipped(20.0);
        for mark in &mut tilted {
            let (x, y) = (mark.0 - 50.0, mark.1 - 40.0);
            *mark = (50.0 + x * cos - y * sin, 40.0 + x * sin + y * cos);
        }
        assert!((pitch_deg(&tilted).unwrap() - 20.0).abs() < 0.01);
        // A level head turned 40 degrees: its eyes look closer together,
        // which must not read as the nose dropping.
        let turn = 40f32.to_radians();
        let apart = 40.0 * turn.cos();
        let along = apart / 2.0 * turn.sin();
        let turned = [
            (30.0, 40.0),
            (30.0 + apart, 40.0),
            (30.0 + apart / 2.0 + along, 40.0 + 40.0 * LEVEL_NOSE_DROP),
            (30.0, 40.0 + 40.0 * LEVEL_MOUTH_DROP),
            (30.0 + apart, 40.0 + 40.0 * LEVEL_MOUTH_DROP),
        ];
        assert!((yaw_deg(&turned).unwrap() - 40.0).abs() < 0.01);
        let read = pitch_deg(&turned).unwrap();
        assert!(read.abs() < 0.01, "{read}");
    }

    #[test]
    fn a_small_face_or_a_guessed_mark_does_not_count_and_the_edge_allows_more() {
        // At the middle: up to sixty degrees is facing.
        assert!(facing(Some(59.0), 0.2, 0.0));
        assert!(!facing(Some(61.0), 0.2, 0.0));
        // At the edge of a wide lens the allowance grows, and stops at 90.
        assert!(facing(Some(80.0), 0.2, 1.0));
        assert!((allowed_deg(0.5) - 84.0).abs() < 0.01);
        assert_eq!(allowed_deg(1.0), 90.0);
        assert_eq!(allowed_deg(7.0), 90.0);
        // Too small to measure, or not measured at all.
        assert!(!facing(Some(0.0), 0.05, 0.0));
        assert!(!facing(None, 0.5, 0.0));
        // A nose the detector put outside the picture was not seen.
        assert!(marks_seen(&marks(0.0), 100.0, 100.0));
        let mut outside = marks(0.0);
        outside[2].0 = 104.0;
        assert!(!marks_seen(&outside, 100.0, 100.0));
        outside[2].0 = -1.0;
        assert!(!marks_seen(&outside, 100.0, 100.0));
        // The mouth is not used, so it may be anywhere.
        let mut mouth = marks(0.0);
        mouth[4] = (500.0, 500.0);
        assert!(marks_seen(&mouth, 100.0, 100.0));
    }

    #[test]
    fn the_vote_needs_two_looks_in_the_last_moment_and_most_of_them_facing() {
        const MS: u64 = 1_000;
        let mut recent = Recent::default();
        assert_eq!(recent.vote(0), None);
        recent.saw(100 * MS, true);
        assert_eq!(recent.vote(200 * MS), None);
        recent.saw(350 * MS, true);
        assert_eq!(recent.vote(400 * MS), Some(true));
        // Three of five facing is enough; two of five is not.
        recent.saw(600 * MS, false);
        recent.saw(850 * MS, false);
        recent.saw(1_100 * MS, true);
        assert_eq!(recent.vote(1_150 * MS), Some(true));
        let mut away = Recent::default();
        for (at, facing) in [
            (0, true),
            (250, false),
            (500, false),
            (750, false),
            (1_000, true),
        ] {
            away.saw(at * MS, facing);
        }
        assert_eq!(away.vote(1_050 * MS), Some(false));
        // Looks older than a second and a half are forgotten: a person who
        // faced the lamp and left is not still facing it.
        assert_eq!(recent.vote(1_100 * MS + WINDOW_US + 1), None);
    }

    fn at(index: usize) -> [f32; 3] {
        CANONICAL
            .iter()
            .find(|(at, _)| *at == index)
            .map(|(_, point)| *point)
            .unwrap()
    }

    #[test]
    fn the_canonical_face_reads_square_and_its_pairs_are_mirrors() {
        // A typo in the table would break a mirror or the middle.
        for (from, to) in ACROSS {
            let (from, to) = (at(from), at(to));
            assert!(from[0] < 0.0, "{from:?} is on the picture's left");
            assert_eq!([-from[0], from[1], from[2]], to);
        }
        for (from, to) in UP {
            assert_eq!((at(from)[0], at(to)[0]), (0.0, 0.0));
            assert!(at(to)[1] > at(from)[1], "{to} is above {from}");
        }
        let lean = up_lean().to_degrees();
        assert!((lean + 8.385).abs() < 0.01, "{lean}");
        let head = mesh_head(&canonical_mesh(0.0, 0.0, 0.0, 8.0)).unwrap();
        assert!(
            head.yaw_deg.abs() < 1e-3 && head.pitch_deg.abs() < 1e-3,
            "{head:?}"
        );
    }

    /// Any turn and tip of a rigid face reads back as made, tilted in the
    /// picture or not, small or large in the crop.
    #[test]
    fn a_mesh_reads_the_turn_and_tip_it_was_made_with() {
        for yaw in [-80.0, -60.0, -30.0, -10.0, 0.0, 10.0, 30.0, 60.0, 80.0] {
            for pitch in [-40.0, -20.0, 0.0, 20.0, 40.0] {
                for roll in [-15.0, 0.0, 10.0] {
                    for scale in [2.0, 8.0] {
                        let mut mesh = canonical_mesh(yaw, pitch, roll, scale);
                        // Anywhere in the crop, at any depth.
                        if scale > 4.0 {
                            for point in mesh.chunks_mut(3) {
                                point[0] -= 40.0;
                                point[1] += 25.0;
                                point[2] += 300.0;
                            }
                        }
                        let head = mesh_head(&mesh)
                            .unwrap_or_else(|| panic!("{yaw} {pitch} {roll} {scale}"));
                        assert!(
                            (head.yaw_deg - yaw).abs() < 0.01
                                && (head.pitch_deg - pitch).abs() < 0.01,
                            "made {yaw} {pitch} {roll} {scale}, read {head:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_mesh_no_head_seen_from_the_front_could_give_is_not_read() {
        let square = canonical_mesh(0.0, 0.0, 0.0, 8.0);
        assert!(mesh_head(&square[..square.len() - 3]).is_none());
        assert!(mesh_head(&[]).is_none());
        assert!(mesh_head(&vec![0.0; crate::mesh::LANDMARKS * 3]).is_none());
        let mut lost = square.clone();
        lost[263 * 3 + 2] = f32::NAN;
        assert!(mesh_head(&lost).is_none());
        // A point it doesn't read may be anything: the inner lips here.
        let mut lips = square.clone();
        lips[crate::mesh::UPPER_LIP * 3] = f32::NAN;
        assert!(mesh_head(&lips).is_some());
        // Mirrored, or upside down: no head seen from the front gives it.
        for flip in [0, 1] {
            let mut flipped = square.clone();
            for point in flipped.chunks_mut(3) {
                point[flip] = 256.0 - point[flip];
            }
            assert!(mesh_head(&flipped).is_none(), "{flip}");
        }
    }

    #[test]
    fn with_a_head_the_turn_and_tip_are_the_meshs_and_without_one_the_marks() {
        let marks = marks(0.5);
        let head = Head {
            yaw_deg: -40.0,
            pitch_deg: 10.0,
        };
        assert_eq!(turn(&marks, Some(head)), (Some(40.0), TurnFrom::Mesh));
        assert_eq!(turn(&marks, None), (yaw_deg(&marks), TurnFrom::Marks));
        assert_eq!(tip(&marks, Some(head)), Some(10.0));
        assert_eq!(tip(&marks, None), pitch_deg(&marks));
        // Turned past sixty: no pitch, as from the marks.
        let side_on = Head {
            yaw_deg: 70.0,
            ..head
        };
        assert_eq!(tip(&marks, Some(side_on)), None);
        assert_eq!(
            (TurnFrom::Marks.word(), TurnFrom::Mesh.word()),
            ("marks", "mesh")
        );
    }

    #[test]
    fn heads_come_only_with_the_switch_and_only_for_the_faces_meshed() {
        let mesh = canonical_mesh(20.0, 0.0, 0.0, 8.0);
        assert_eq!(mesh_heads(false, 2, &[(1, mesh.clone())]), vec![None, None]);
        let heads = mesh_heads(true, 2, &[(1, mesh.clone())]);
        assert!(heads[0].is_none());
        assert!((heads[1].unwrap().yaw_deg - 20.0).abs() < 0.01);
        // An index past the faces found, or a mesh that can't be read.
        assert_eq!(mesh_heads(true, 1, &[(3, mesh)]), vec![None]);
        assert_eq!(mesh_heads(true, 1, &[(0, vec![1.0; 9])]), vec![None]);
    }
}
