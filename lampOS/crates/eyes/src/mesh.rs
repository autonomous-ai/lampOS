//! How open a face's mouth is, from Face Mesh V2's inner-lip points, for
//! #720: YuNet's corners alone can't tell a talking face from a still one
//! at desk distance (`speaking.rs`). With `--mouth-model`, the largest face
//! is read once a look (or, with `--mouth-on-heard on`, only while the voice
//! hears a line, [`Heard`]); the result goes to the journal, and with
//! `LAMP_EYES_SPEAKING=on` to that face's speaking judge.
use crate::{
    faces::Face,
    picture::{Frame, HEIGHT, Picture, WIDTH},
};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, sync::Arc};
use tract_onnx::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// `models/mouth/SOURCE.md`: made by `training/mouth/make.sh`, never
/// committed here.
pub const MODEL_FILE: &str = "face_mesh_v2.onnx";
pub const MODEL_BYTES: u64 = 4_915_552;
pub const MODEL_SHA256: &str = "09d24504cdc51240b6ccd8c457040c7cb61ab3485e0e08696dd44ce52ed8131a";
/// The model's input, square.
pub const SIDE: usize = 256;
/// Each side of the face box is widened by this share of it, as the model
/// was trained.
const MARGIN: f32 = 0.25;
/// The landmarks it gives, each x, y and z.
pub const LANDMARKS: usize = 478;
/// The inner lips' middle, upper and lower, and the face's top and chin.
pub(crate) const UPPER_LIP: usize = 13;
pub(crate) const LOWER_LIP: usize = 14;
pub(crate) const BROW: usize = 10;
pub(crate) const CHIN: usize = 152;

/// The eye line's angle in the frame, as its sine and cosine: what [`crop`]
/// turns the face by. `None` for eyes on one point.
pub fn eye_line(face: &Face) -> Option<(f32, f32)> {
    let (eye, other) = (face.marks[0], face.marks[1]);
    let (dx, dy) = (other.0 - eye.0, other.1 - eye.1);
    if !(dx.is_finite() && dy.is_finite()) || dx.hypot(dy) < 1e-3 {
        return None;
    }
    Some(dy.atan2(dx).sin_cos())
}

/// The face's box widened by the margin and turned so the eyes are level,
/// as the model's input: `SIDE` square, red, green and blue from 0 to 1,
/// row by row (NHWC). Outside the frame is black. `None` for a face whose
/// eyes sit on one point.
pub fn crop(picture: &Picture, face: &Face) -> Option<Vec<f32>> {
    let plane = WIDTH * HEIGHT;
    // The picture's planes are blue, green, red; the model's channels red,
    // green, blue.
    crop_with(face, picture.filled, |colour, u, v| {
        picture.planes[(2 - colour) * plane + v * WIDTH + u]
    })
}

/// As [`crop`], from the frame at its own size (`--full-look`): the face
/// the detector found in the picture, scaled back to the frame's pixels and
/// cut there, so a face seated at the desk, its eyes about 20 of the
/// picture's pixels apart at 1280 by 960, is meshed from about 80. `None`
/// for a frame whose bytes are not its size, or eyes on one point.
pub fn crop_full(frame: &Frame, face: &Face) -> Option<Vec<f32>> {
    if frame.width == 0 || frame.height == 0 || frame.rgb.len() != frame.width * frame.height * 3 {
        return None;
    }
    // A picture pixel's centre, in the frame's pixels (`who::align_full`).
    let back = |p: f32| (p + 0.5) / frame.scale - 0.5;
    let face = Face {
        x: back(face.x),
        y: back(face.y),
        width: face.width / frame.scale,
        height: face.height / frame.scale,
        marks: face.marks.map(|(x, y)| (back(x), back(y))),
        ..*face
    };
    crop_with(&face, (frame.width, frame.height), |colour, u, v| {
        f32::from(frame.rgb[(v * frame.width + u) * 3 + colour])
    })
}

/// [`crop_full`] when the look kept the frame at its own size for the
/// mouth, else [`crop`].
pub fn crop_of(picture: &Picture, frame: Option<&Frame>, face: &Face) -> Option<Vec<f32>> {
    match frame {
        Some(frame) => crop_full(frame, face),
        None => crop(picture, face),
    }
}

/// The crop of `face` from a frame `filled` across and down, read pixel by
/// pixel through `at(colour, x, y)`, colour 0 red, 1 green, 2 blue, each 0
/// to 255.
fn crop_with(
    face: &Face,
    filled: (usize, usize),
    at: impl Fn(usize, usize, usize) -> f32,
) -> Option<Vec<f32>> {
    let (sin, cos) = eye_line(face)?;
    let centre = (face.x + face.width / 2.0, face.y + face.height / 2.0);
    let side = face.width.max(face.height) * (1.0 + 2.0 * MARGIN);
    let step = side / SIDE as f32;
    let (filled_x, filled_y) = filled;
    let mut out = vec![0.0f32; SIDE * SIDE * 3];
    for row in 0..SIDE {
        for column in 0..SIDE {
            // From the crop's pixel to the frame's: offset from the middle,
            // turned by the eye line's angle.
            let (p, q) = (
                (column as f32 + 0.5) * step - side / 2.0,
                (row as f32 + 0.5) * step - side / 2.0,
            );
            let (u, v) = (centre.0 + p * cos - q * sin, centre.1 + p * sin + q * cos);
            if u < 0.0 || v < 0.0 || u > (filled_x - 1) as f32 || v > (filled_y - 1) as f32 {
                continue;
            }
            let (u0, v0) = (u.floor() as usize, v.floor() as usize);
            let (u1, v1) = ((u0 + 1).min(filled_x - 1), (v0 + 1).min(filled_y - 1));
            let (du, dv) = (u - u0 as f32, v - v0 as f32);
            for colour in 0..3 {
                let value = at(colour, u0, v0) * (1.0 - du) * (1.0 - dv)
                    + at(colour, u1, v0) * du * (1.0 - dv)
                    + at(colour, u0, v1) * (1.0 - du) * dv
                    + at(colour, u1, v1) * du * dv;
                out[(row * SIDE + column) * 3 + colour] = value / 255.0;
            }
        }
    }
    Some(out)
}

/// The inner lips' gap over the face's height, from the model's landmarks:
/// 0 shut, about 0.1 wide open. `None` if the landmarks are not all there
/// or the face has no height.
pub fn mouth_open(landmarks: &[f32]) -> Option<f32> {
    if landmarks.len() != LANDMARKS * 3 {
        return None;
    }
    let point = |at: usize| (landmarks[at * 3], landmarks[at * 3 + 1]);
    let apart = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0).hypot(b.1 - a.1);
    let tall = apart(point(BROW), point(CHIN));
    let gap = apart(point(UPPER_LIP), point(LOWER_LIP));
    (tall.is_finite() && gap.is_finite() && tall > 1e-3).then(|| gap / tall)
}

/// `on` exactly meshes up to [`MOST_MESHED_FACES`] faces a look, largest
/// first, instead of the largest only, so a second face's speaking reads its
/// inner lips too (the first builder's 08:49 on Queue #3126). Off: the
/// largest only, as before.
pub const ALL_FACES_ENVIRONMENT: &str = "LAMP_EYES_SPEAKING_ALL_FACES";
/// With this `on` and `--full-look`, the mouths are cut from the frame at its
/// own size ([`crop_full`]), so a seated face's eyes are counted in those
/// pixels (#1711). Off in the product: a silent person at the desk read
/// `yes` on those crops on lamp-52e6, and a false `yes` names a silent known
/// person as a line's speaker (#3764, docs/eyes.md).
pub const FULL_LOOK_ENVIRONMENT: &str = "LAMP_EYES_MOUTH_FULL_LOOK";
/// The faces meshed a look with [`ALL_FACES_ENVIRONMENT`] on.
pub const MOST_MESHED_FACES: usize = 3;
/// A face after the largest is meshed only if the look's meshing so far
/// plus the last mesh's time stays within this: under tract on lamp-52e6's
/// big core (85 ms a face) two faces, under ONNX Runtime (33 ms) three, on a
/// small core (290 ms) the largest only. Every face meshed is meshed on every
/// look, never in turn: a speaking window reads the mesh only when every one
/// of its looks has it (`speaking.rs`), so faces meshed alternately would be
/// judged on the corners. A face the budget skips on a look is judged on the
/// corners until its window is all mesh again.
pub const EXTRA_MESH_BUDGET_MS: u64 = 200;

/// The indices of the faces to mesh this look, largest first: the largest
/// only unless `all_faces`, then up to [`MOST_MESHED_FACES`]. Each after the
/// first is meshed only while [`more_faces`] allows.
pub fn meshed_order(heights: &[f32], all_faces: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..heights.len()).collect();
    order.sort_by(|a, b| heights[*b].total_cmp(&heights[*a]));
    order.truncate(if all_faces { MOST_MESHED_FACES } else { 1 });
    order
}

/// Whether one more face is meshed this look: `spent` meshing so far, and
/// `last` the last mesh's time, as the next one's estimate.
pub fn more_faces(spent: std::time::Duration, last: std::time::Duration) -> bool {
    (spent + last).as_millis() <= u128::from(EXTRA_MESH_BUDGET_MS)
}

/// With `--mouth-on-heard on`: the mesh runs only while the voice hears a
/// line (`voice/heard`), from its first words until [`AFTER_LINE_US`] after
/// it is finished, so a quiet room costs it nothing (#705's heat). A line
/// whose end never comes is let go [`LINE_MOST_US`] after its last words.
#[derive(Debug, Default)]
pub struct Heard {
    until_us: Option<u64>,
}

/// How long after a line is finished the mouth is still read: the speaking
/// judge's window, so the line's last looks are judged on mesh looks.
pub const AFTER_LINE_US: u64 = crate::speaking::WINDOW_US;
/// A line still coming, with no word for this long, is taken as over.
pub const LINE_MOST_US: u64 = 15_000_000;

impl Heard {
    /// Words heard: still coming, or the whole of a line.
    pub fn words(&mut self, finished: bool, now_us: u64) {
        self.until_us = Some(
            now_us
                + if finished {
                    AFTER_LINE_US
                } else {
                    LINE_MOST_US
                },
        );
    }

    /// Whether the mouth is to be read now.
    pub fn hears(&self, now_us: u64) -> bool {
        self.until_us.is_some_and(|until| now_us <= until)
    }
}

/// What runs the mesh: tract, as YuNet runs, or ONNX Runtime, the pinned
/// library the voice's turn-end worker loads (`--mouth-runtime`): 33 ms a
/// face on lamp-52e6's big core against tract's 85 (#720, 10:3x).
enum Engine {
    Tract(Arc<TypedRunnableModel>),
    // Field order is drop order: the session goes before its environment.
    Ort {
        session: ort::session::Session,
        _environment: ort::environment::Environment,
    },
}

pub struct Mesh {
    engine: Engine,
}

impl Mesh {
    /// Under tract. Verifies the exact pinned bytes before the ONNX parser
    /// sees them.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = read_pinned(path)?;
        let plan = tract_onnx::onnx()
            .model_for_read(&mut bytes.as_slice())?
            .with_input_fact(0, f32::fact([1, SIDE, SIDE, 3]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self {
            engine: Engine::Tract(plan),
        })
    }

    /// Under ONNX Runtime from `runtime`, checked against the voice's pin
    /// (`lamp_turn_end::ONNX_RUNTIME`) before it is loaded, on one thread.
    /// ONNX Runtime is loaded once a process.
    pub fn load_with_runtime(path: &Path, runtime: &Path) -> Result<Self> {
        let library =
            lamp_turn_end::ONNX_RUNTIME.ok_or("no ONNX Runtime is pinned for this target")?;
        lamp_turn_end::fast::verify_runtime(runtime, &library)
            .map_err(|error| format!("ONNX Runtime library: {error}"))?;
        let bytes = read_pinned(path)?;
        let environment = ort::init_from(runtime)
            .map_err(|error| format!("ONNX Runtime {}: {error}", runtime.display()))?
            .with_name("lamp-eyes-mouth")
            .with_telemetry(false)
            .build()
            .map_err(|error| error.to_string())?;
        environment.set_log_level(ort::logging::LogLevel::Error);
        let session = ort::session::Session::builder(&environment)
            .and_then(|builder| Ok(builder.with_log_level(ort::logging::LogLevel::Error)?))
            .and_then(|builder| {
                Ok(builder
                    .with_optimization_level(ort::session::builder::GraphOptimizationLevel::All)?)
            })
            .and_then(|builder| Ok(builder.with_intra_threads(1)?))
            .and_then(|builder| Ok(builder.with_inter_threads(1)?))
            .and_then(|mut builder| builder.commit_from_memory(&bytes))
            .map_err(|error| format!("ONNX Runtime session: {error}"))?;
        Ok(Self {
            engine: Engine::Ort {
                session,
                _environment: environment,
            },
        })
    }

    /// How open this face's mouth is in this picture.
    pub fn mouth_open(&mut self, picture: &Picture, face: &Face) -> Result<Option<f32>> {
        Ok(self
            .landmarks(picture, face)?
            .and_then(|landmarks| mouth_open(&landmarks)))
    }

    /// This face's 478 landmarks, x, y and z each, in the crop's pixels
    /// (0 to [`SIDE`], the eyes level). `None` for a face with no crop.
    pub fn landmarks(&mut self, picture: &Picture, face: &Face) -> Result<Option<Vec<f32>>> {
        match crop(picture, face) {
            Some(input) => self.landmarks_of(input).map(Some),
            None => Ok(None),
        }
    }

    /// The landmarks for one [`crop`], so the crop can be cut on the look
    /// and the mesh run elsewhere (the seems worker, [`crate::latest`]).
    pub fn landmarks_of(&mut self, input: Vec<f32>) -> Result<Vec<f32>> {
        let landmarks = match &mut self.engine {
            Engine::Tract(plan) => {
                let input = Tensor::from_shape(&[1, SIDE, SIDE, 3], &input)?;
                let outputs = plan.run(tvec!(input.into()))?;
                // The largest output is the landmarks; the others are scores.
                outputs
                    .iter()
                    .max_by_key(|output| output.len())
                    .ok_or("the model gave no output")?
                    .to_plain_array_view::<f32>()?
                    .iter()
                    .copied()
                    .collect::<Vec<f32>>()
            }
            Engine::Ort { session, .. } => {
                let input = ort::value::Tensor::from_array(([1usize, SIDE, SIDE, 3], input))?;
                let outputs = session.run(ort::inputs![input])?;
                let mut landmarks = Vec::new();
                for (_, output) in outputs.iter() {
                    let (_, values) = output.try_extract_tensor::<f32>()?;
                    if values.len() > landmarks.len() {
                        landmarks = values.to_vec();
                    }
                }
                landmarks
            }
        };
        Ok(landmarks)
    }
}

/// The model's exact bytes, checked by size and SHA-256 before any parser
/// sees them.
fn read_pinned(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    if file.metadata()?.len() != MODEL_BYTES {
        return Err(format!("{} has an unexpected size", path.display()).into());
    }
    let mut bytes = Vec::with_capacity(MODEL_BYTES as usize);
    file.take(MODEL_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != MODEL_BYTES {
        return Err(format!("{} changed while it was read", path.display()).into());
    }
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if digest != MODEL_SHA256 {
        return Err(format!("{} does not match the pinned SHA256", path.display()).into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(x: f32, y: f32, side: f32, eyes: [(f32, f32); 2]) -> Face {
        Face {
            score: 0.9,
            x,
            y,
            width: side,
            height: side,
            marks: [eyes[0], eyes[1], (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)],
        }
    }

    #[test]
    fn off_meshes_the_largest_face_only_and_on_up_to_the_most_largest_first() {
        let heights = [40.0, 90.0, 60.0, 20.0];
        assert_eq!(meshed_order(&heights, false), vec![1]);
        assert_eq!(meshed_order(&heights, true), vec![1, 2, 0]);
        assert_eq!(meshed_order(&[50.0], true), vec![0]);
        assert!(meshed_order(&[], true).is_empty());
    }

    /// The budget as measured on lamp-52e6: tract's big core meshes two
    /// faces, ONNX Runtime three, a small core the largest only.
    #[test]
    fn extra_faces_are_meshed_within_the_budget() {
        let count = |each_ms: u64| {
            let each = std::time::Duration::from_millis(each_ms);
            let mut spent = each;
            let mut meshed = 1;
            while meshed < MOST_MESHED_FACES && more_faces(spent, each) {
                spent += each;
                meshed += 1;
            }
            meshed
        };
        assert_eq!(count(85), 2);
        assert_eq!(count(33), 3);
        assert_eq!(count(290), 1);
        // Within a look's pace (250 ms), so the 2 s window keeps its five
        // looks at the following rate.
        let look_ms = 250;
        assert!(EXTRA_MESH_BUDGET_MS < look_ms);
        assert!(
            (crate::speaking::WINDOW_US / 1_000 / look_ms) as usize >= crate::speaking::LEAST_LOOKS
        );
    }

    /// Two faces, the smaller one talking: with every face meshed its
    /// speaking reads the mesh's `yes`; with the largest only it is never
    /// read from the mesh and never `yes` (its corners hold still). The
    /// largest face's read is the same either way.
    #[test]
    fn a_second_face_talking_reads_speaking_only_with_all_faces_meshed() {
        use crate::speaking::{LEAST_LOOKS, Mouths};
        use lamp_msgs::{SpeakingFrom, SpeakingState};
        // Still corners, 40 px between the eyes, level and facing.
        let marks = |x: f32| -> [(f32, f32); 5] {
            [
                (x, 100.0),
                (x + 40.0, 100.0),
                (x + 20.0, 120.0),
                (x + 6.0, 140.0),
                (x + 34.0, 140.0),
            ]
        };
        let heights = [120.0, 80.0];
        let run = |all_faces: bool| {
            let mut mouths = Mouths::default();
            let mut reads = Vec::new();
            for look in 0..12u64 {
                let order = meshed_order(&heights, all_faces);
                let gap = |index: usize| match index {
                    0 => 0.01,
                    _ => 0.04 + 0.03 * (look as f32 * 1.7).sin(),
                };
                let open = |index: usize| order.contains(&index).then(|| gap(index));
                let now = look * 250_000;
                let largest = mouths.judge(1, &marks(40.0), Some(0.0), open(0), now);
                let second = mouths.judge(2, &marks(180.0), Some(0.0), open(1), now);
                if look + 1 >= LEAST_LOOKS as u64 {
                    reads.push((largest, second));
                }
            }
            reads
        };
        let (on, off) = (run(true), run(false));
        for ((largest_on, second_on), (largest_off, second_off)) in on.iter().zip(&off) {
            assert_eq!(second_on.state, SpeakingState::Yes, "{second_on:?}");
            assert_eq!(second_on.from, Some(SpeakingFrom::Mesh));
            assert_ne!(second_off.state, SpeakingState::Yes, "{second_off:?}");
            assert_ne!(second_off.from, Some(SpeakingFrom::Mesh));
            assert_eq!(largest_on, largest_off);
            assert_eq!(largest_on.state, SpeakingState::No);
        }
    }

    /// A black 320 by 240 frame with one red pixel and one blue one.
    fn picture(red: (usize, usize), blue: (usize, usize)) -> Picture {
        let mut rgb = vec![0u8; 320 * 240 * 3];
        rgb[(red.1 * 320 + red.0) * 3] = 255;
        rgb[(blue.1 * 320 + blue.0) * 3 + 2] = 255;
        Picture::from_rgb(&rgb, 320, 240).unwrap()
    }

    /// Where in the crop a channel is brightest.
    fn brightest(crop: &[f32], channel: usize) -> (usize, usize) {
        let at = (0..SIDE * SIDE)
            .max_by(|a, b| crop[a * 3 + channel].total_cmp(&crop[b * 3 + channel]))
            .unwrap();
        (at % SIDE, at / SIDE)
    }

    #[test]
    fn a_level_face_is_cropped_with_its_margin_in_rgb() {
        // A 64-pixel box at (128, 88): the crop is 96 pixels of the frame,
        // centred at (160, 120), so the frame's pixel (160, 120) lands in
        // the middle and (120, 120), 40 to the left, about 107 crop
        // pixels left of it.
        let face = face(128.0, 88.0, 64.0, [(144.0, 110.0), (176.0, 110.0)]);
        let crop = crop(&picture((160, 120), (120, 120)), &face).unwrap();
        assert_eq!(crop.len(), SIDE * SIDE * 3);
        let (x, y) = brightest(&crop, 0);
        assert!(
            x.abs_diff(128) <= 2 && y.abs_diff(128) <= 2,
            "red at {x}, {y}"
        );
        let (x, y) = brightest(&crop, 2);
        assert!(
            x.abs_diff(128 - 107) <= 2 && y.abs_diff(128) <= 2,
            "blue at {x}, {y}"
        );
        assert!(crop.iter().all(|value| (0.0..=1.0).contains(value)));
    }

    #[test]
    fn a_tilted_face_is_turned_so_its_eyes_are_level() {
        // The eye line tipped 90 degrees (the right eye straight below the
        // left): a pixel 40 below the middle in the frame lies 40 to the
        // right of it along the eye line, so it lands right of the crop's
        // middle, on its row.
        let face = face(128.0, 88.0, 64.0, [(160.0, 100.0), (160.0, 140.0)]);
        let crop = crop(&picture((160, 160), (0, 0)), &face).unwrap();
        let (x, y) = brightest(&crop, 0);
        assert!(
            x.abs_diff(128 + 107) <= 2 && y.abs_diff(128) <= 2,
            "red at {x}, {y}"
        );
    }

    /// The same face cut from a 1280 by 960 frame, by the box the detector
    /// found in its 320 by 240 picture scaled back, covers the same part of
    /// it: a red mark at the face's middle and a blue one 40 picture pixels
    /// to its left land where the picture's own crop puts them.
    #[test]
    fn a_crop_from_the_full_frame_covers_the_face_the_picture_shows() {
        let (width, height) = (1_280, 960);
        let mut rgb = vec![0u8; width * height * 3];
        // Four by four frame pixels each: one picture pixel.
        for (x0, y0, colour) in [(640, 480, 0), (480, 480, 2)] {
            for y in y0..y0 + 4 {
                for x in x0..x0 + 4 {
                    rgb[(y * width + x) * 3 + colour] = 255;
                }
            }
        }
        let picture = Picture::from_rgb(&rgb, width, height).unwrap();
        assert_eq!(picture.filled, (320, 240));
        let frame = Frame {
            rgb,
            width,
            height,
            scale: 0.25,
        };
        let face = face(128.0, 88.0, 64.0, [(144.0, 110.0), (176.0, 110.0)]);
        let small = crop(&picture, &face).unwrap();
        let full = crop_of(&picture, Some(&frame), &face).unwrap();
        assert_eq!(crop_of(&picture, None, &face), Some(small.clone()));
        for (channel, expected) in [(0, (128, 128)), (2, (128 - 107, 128))] {
            let (a, b) = (brightest(&small, channel), brightest(&full, channel));
            for (x, y) in [a, b] {
                assert!(
                    x.abs_diff(expected.0) <= 3 && y.abs_diff(expected.1) <= 3,
                    "channel {channel}: picture {a:?}, frame {b:?}"
                );
            }
        }
        // A frame whose bytes are not its size gives none.
        let broken = Frame {
            rgb: vec![0; 10],
            width,
            height,
            scale: 0.25,
        };
        assert!(crop_full(&broken, &face).is_none());
    }

    #[test]
    fn eyes_on_one_point_give_no_crop() {
        let face = face(128.0, 88.0, 64.0, [(150.0, 110.0), (150.0, 110.0)]);
        assert!(crop(&picture((0, 0), (0, 0)), &face).is_none());
    }

    #[test]
    fn the_mouth_is_the_inner_lips_gap_over_the_face_height() {
        let mut landmarks = vec![0.0f32; LANDMARKS * 3];
        let mut put = |at: usize, x: f32, y: f32| {
            landmarks[at * 3] = x;
            landmarks[at * 3 + 1] = y;
        };
        put(BROW, 128.0, 40.0);
        put(CHIN, 128.0, 240.0);
        put(UPPER_LIP, 128.0, 180.0);
        put(LOWER_LIP, 128.0, 190.0);
        assert_eq!(mouth_open(&landmarks), Some(0.05));
        assert_eq!(mouth_open(&landmarks[..30]), None);
        assert_eq!(mouth_open(&vec![0.0; LANDMARKS * 3]), None);
    }

    #[test]
    fn the_mouth_is_read_from_a_lines_first_words_until_two_seconds_after_it_ends() {
        let mut heard = Heard::default();
        assert!(!heard.hears(0));
        heard.words(false, 1_000_000);
        assert!(heard.hears(1_000_000) && heard.hears(9_000_000));
        heard.words(true, 10_000_000);
        assert!(heard.hears(10_000_000 + AFTER_LINE_US));
        assert!(!heard.hears(10_000_001 + AFTER_LINE_US));
        // Words that never finish are let go after fifteen seconds.
        heard.words(false, 20_000_000);
        assert!(heard.hears(20_000_000 + LINE_MOST_US));
        assert!(!heard.hears(20_000_001 + LINE_MOST_US));
    }

    #[test]
    fn a_file_that_is_not_the_pinned_model_is_refused() {
        let path =
            std::env::temp_dir().join(format!("lamp-eyes-not-a-mesh-{}", std::process::id()));
        std::fs::write(&path, b"not a model").unwrap();
        let refused = Mesh::load(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(refused.is_err());
        assert!(Mesh::load(Path::new("/nonexistent/face_mesh_v2.onnx")).is_err());
    }

    /// With the model made by `training/mouth/make.sh` in
    /// `lampOS/.cache/mouth/`: it loads, and a crop of a black frame gives
    /// a number or nothing, never an error.
    #[test]
    #[ignore = "needs face_mesh_v2.onnx from training/mouth/make.sh"]
    fn the_pinned_model_loads_and_reads_a_crop() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.cache/mouth")
            .join(MODEL_FILE);
        let mut mesh = Mesh::load(&path).unwrap();
        let face = face(128.0, 88.0, 64.0, [(144.0, 110.0), (176.0, 110.0)]);
        mesh.mouth_open(&picture((0, 0), (0, 0)), &face).unwrap();
    }

    /// With the model and ONNX Runtime 1.30.0 for this machine in
    /// `LAMP_ONNX_RUNTIME` (checked against the voice's pin): the same
    /// crop gives the same `mouth_open` under both engines.
    #[test]
    #[ignore = "needs face_mesh_v2.onnx and LAMP_ONNX_RUNTIME"]
    fn onnx_runtime_reads_the_mouth_as_tract_does() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.cache/mouth")
            .join(MODEL_FILE);
        let runtime = std::env::var_os("LAMP_ONNX_RUNTIME").expect("LAMP_ONNX_RUNTIME");
        let mut tract = Mesh::load(&path).unwrap();
        let mut ort = Mesh::load_with_runtime(&path, Path::new(&runtime)).unwrap();
        // A frame with something in it: colour rising across and down.
        let rgb: Vec<u8> = (0..240usize)
            .flat_map(|y| {
                (0..320usize)
                    .flat_map(move |x| [(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
            })
            .collect();
        let frame = Picture::from_rgb(&rgb, 320, 240).unwrap();
        let face = face(128.0, 88.0, 64.0, [(144.0, 110.0), (176.0, 110.0)]);
        let (a, b) = (
            tract.mouth_open(&frame, &face).unwrap(),
            ort.mouth_open(&frame, &face).unwrap(),
        );
        assert_eq!(a.is_some(), b.is_some(), "{a:?} {b:?}");
        if let (Some(a), Some(b)) = (a, b) {
            assert!((a - b).abs() < 1e-3, "tract {a}, onnx runtime {b}");
        }
    }
}
