//! How open a face's mouth is, from Face Mesh V2's inner-lip points, for
//! #720: YuNet's corners alone can't tell a talking face from a still one
//! at desk distance (`speaking.rs`). With `--mouth-model`, the largest face
//! is read once a look (or, with `--mouth-on-heard on`, only while the voice
//! hears a line, [`Heard`]); the result goes to the journal, and with
//! `LAMP_EYES_SPEAKING=on` to that face's speaking judge.
use crate::{
    faces::Face,
    picture::{HEIGHT, Picture, WIDTH},
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
const LANDMARKS: usize = 478;
/// The inner lips' middle, upper and lower, and the face's top and chin.
const UPPER_LIP: usize = 13;
const LOWER_LIP: usize = 14;
const BROW: usize = 10;
const CHIN: usize = 152;

/// The face's box widened by the margin and turned so the eyes are level,
/// as the model's input: `SIDE` square, red, green and blue from 0 to 1,
/// row by row (NHWC). Outside the frame is black. `None` for a face whose
/// eyes sit on one point.
pub fn crop(picture: &Picture, face: &Face) -> Option<Vec<f32>> {
    let (eye, other) = (face.marks[0], face.marks[1]);
    let (dx, dy) = (other.0 - eye.0, other.1 - eye.1);
    if !(dx.is_finite() && dy.is_finite()) || dx.hypot(dy) < 1e-3 {
        return None;
    }
    let (sin, cos) = dy.atan2(dx).sin_cos();
    let centre = (face.x + face.width / 2.0, face.y + face.height / 2.0);
    let side = face.width.max(face.height) * (1.0 + 2.0 * MARGIN);
    let step = side / SIDE as f32;
    let (filled_x, filled_y) = (picture.filled.0, picture.filled.1);
    let plane = WIDTH * HEIGHT;
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
            let at = |c: usize, uu: usize, vv: usize| picture.planes[c * plane + vv * WIDTH + uu];
            // The picture's planes are blue, green, red; the model's
            // channels red, green, blue.
            for (to, from) in [(0usize, 2usize), (1, 1), (2, 0)] {
                let value = at(from, u0, v0) * (1.0 - du) * (1.0 - dv)
                    + at(from, u1, v0) * du * (1.0 - dv)
                    + at(from, u0, v1) * (1.0 - du) * dv
                    + at(from, u1, v1) * du * dv;
                out[(row * SIDE + column) * 3 + to] = value / 255.0;
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
        let Some(input) = crop(picture, face) else {
            return Ok(None);
        };
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
        Ok(mouth_open(&landmarks))
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
