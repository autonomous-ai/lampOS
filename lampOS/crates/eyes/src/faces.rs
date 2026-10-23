//! Faces in a picture: the YuNet detector, run with tract, and its outputs
//! made into boxes.
use crate::picture::{HEIGHT, Picture, WIDTH};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, sync::Arc};
use tract_onnx::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// The exact model file: `face_detection_yunet_2023mar.onnx`, the detector
/// the old runtime used (`hal/drivers/tracking/models/`). Fetched separately
/// and never committed here.
pub const MODEL_FILE: &str = "face_detection_yunet_2023mar.onnx";
pub const MODEL_BYTES: u64 = 232_589;
pub const MODEL_SHA256: &str = "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4";

/// Under this a detection is not a face. The model's own example uses 0.9
/// on large pictures and the old runtime 0.35; in a 320 wide frame a face at
/// arm's length scored 0.92 and nothing in two rooms without one reached 0.6.
pub const LEAST_SCORE: f32 = 0.6;
/// Two boxes that overlap more than this are one face.
const SAME_FACE: f32 = 0.3;
const STRIDES: [usize; 3] = [8, 16, 32];

/// A face, in the picture's pixels: its box, and the eye on the left of the
/// picture, the eye on the right, the nose and the two corners of the mouth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub score: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub marks: [(f32, f32); 5],
}

fn overlap(a: &Face, b: &Face) -> f32 {
    let across = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
    let down = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
    let shared = across.max(0.0) * down.max(0.0);
    shared / (a.width * a.height + b.width * b.height - shared).max(1e-6)
}

/// The detector's twelve outputs, in its own order: face score, object
/// score, box and marks, each at strides 8, 16 and 32. Best first, one box a
/// face.
pub fn decode(outputs: &[Vec<f32>], least: f32) -> std::result::Result<Vec<Face>, &'static str> {
    if outputs.len() != 12 {
        return Err("the detector did not give twelve outputs");
    }
    let mut faces = Vec::new();
    for (level, stride) in STRIDES.into_iter().enumerate() {
        let (columns, rows) = (WIDTH / stride, HEIGHT / stride);
        let cells = columns * rows;
        let (face, object) = (&outputs[level], &outputs[3 + level]);
        let (boxes, marks) = (&outputs[6 + level], &outputs[9 + level]);
        if face.len() != cells
            || object.len() != cells
            || boxes.len() != cells * 4
            || marks.len() != cells * 10
        {
            return Err("a detector output of the wrong size");
        }
        let step = stride as f32;
        for cell in 0..cells {
            let score = (face[cell].clamp(0.0, 1.0) * object[cell].clamp(0.0, 1.0)).sqrt();
            if score.is_nan() || score < least {
                continue;
            }
            let (column, row) = ((cell % columns) as f32, (cell / columns) as f32);
            let width = boxes[cell * 4 + 2].exp() * step;
            let height = boxes[cell * 4 + 3].exp() * step;
            let found = Face {
                score,
                x: (column + boxes[cell * 4]) * step - width / 2.0,
                y: (row + boxes[cell * 4 + 1]) * step - height / 2.0,
                width,
                height,
                marks: std::array::from_fn(|mark| {
                    (
                        (marks[cell * 10 + 2 * mark] + column) * step,
                        (marks[cell * 10 + 2 * mark + 1] + row) * step,
                    )
                }),
            };
            // The model can give a box that is not a number, or one far off
            // the picture. Neither is a face to look at.
            let centre = (found.x + width / 2.0, found.y + height / 2.0);
            let sane = [found.x, found.y, width, height]
                .into_iter()
                .chain(found.marks.into_iter().flat_map(|(x, y)| [x, y]))
                .all(f32::is_finite)
                && (4.0..=WIDTH as f32).contains(&width)
                && (4.0..=HEIGHT as f32).contains(&height)
                && (0.0..=WIDTH as f32).contains(&centre.0)
                && (0.0..=HEIGHT as f32).contains(&centre.1);
            if sane {
                faces.push(found);
            }
        }
    }
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Face> = Vec::new();
    for face in faces {
        if kept.iter().all(|other| overlap(other, &face) <= SAME_FACE) {
            kept.push(face);
        }
    }
    Ok(kept)
}

pub struct Detector {
    plan: Arc<TypedRunnableModel>,
}

impl Detector {
    /// Verifies the exact pinned bytes before the ONNX parser sees them.
    pub fn load(path: &Path) -> Result<Self> {
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
        let plan = tract_onnx::onnx()
            .with_ignore_value_info(true)
            .with_ignore_output_shapes(true)
            .model_for_read(&mut bytes.as_slice())?
            .with_input_fact(0, f32::fact([1, 3, HEIGHT, WIDTH]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self { plan })
    }

    /// The faces in one picture, best first.
    pub fn find(&self, picture: &Picture) -> Result<Vec<Face>> {
        let input = Tensor::from_shape(&[1, 3, HEIGHT, WIDTH], &picture.planes)?;
        let outputs = self.plan.run(tvec!(input.into()))?;
        let mut plain = Vec::with_capacity(outputs.len());
        for output in outputs.iter() {
            plain.push(
                output
                    .to_plain_array_view::<f32>()?
                    .iter()
                    .copied()
                    .collect::<Vec<f32>>(),
            );
        }
        Ok(decode(&plain, LEAST_SCORE)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Twelve outputs that say nothing anywhere.
    fn nothing() -> Vec<Vec<f32>> {
        let mut outputs = Vec::new();
        for per_cell in [1usize, 1, 4, 10] {
            for stride in STRIDES {
                outputs.push(vec![0.0; (WIDTH / stride) * (HEIGHT / stride) * per_cell]);
            }
        }
        outputs
    }

    /// Put one detection in the cell at this column and row of a level.
    fn put(
        outputs: &mut [Vec<f32>],
        level: usize,
        column: usize,
        row: usize,
        score: f32,
        log_size: f32,
    ) {
        let cell = row * (WIDTH / STRIDES[level]) + column;
        outputs[level][cell] = score;
        outputs[3 + level][cell] = score;
        outputs[6 + level][cell * 4..cell * 4 + 4].copy_from_slice(&[0.5, 0.5, log_size, log_size]);
        // Eyes a quarter of a cell either side of the middle, the nose in it.
        outputs[9 + level][cell * 10..cell * 10 + 6]
            .copy_from_slice(&[0.25, 0.4, 0.75, 0.4, 0.5, 0.6]);
    }

    #[test]
    fn a_detection_becomes_a_box_where_its_cell_is() {
        let mut outputs = nothing();
        // Stride 16, column 10, row 5: centred at (168, 88), 64 pixels square.
        put(&mut outputs, 1, 10, 5, 0.9, 4.0f32.ln());
        let faces = decode(&outputs, LEAST_SCORE).unwrap();
        assert_eq!(faces.len(), 1);
        let face = faces[0];
        assert!((face.score - 0.9).abs() < 1e-5);
        assert!((face.x - 136.0).abs() < 1e-3 && (face.y - 56.0).abs() < 1e-3);
        assert!((face.width - 64.0).abs() < 1e-3 && (face.height - 64.0).abs() < 1e-3);
        assert_eq!(face.marks[0], (164.0, 86.4));
        assert_eq!(face.marks[2], (168.0, 89.6));
    }

    #[test]
    fn weak_and_duplicate_and_senseless_detections_are_dropped() {
        let mut outputs = nothing();
        put(&mut outputs, 1, 10, 5, 0.9, 4.0f32.ln());
        // The same face again from the next cell and from another level,
        // one too weak, one that is not a number, one far from here.
        put(&mut outputs, 1, 11, 5, 0.8, 4.0f32.ln());
        put(&mut outputs, 0, 20, 10, 0.7, 8.0f32.ln());
        put(&mut outputs, 1, 3, 12, 0.5, 4.0f32.ln());
        put(&mut outputs, 2, 2, 2, 0.95, f32::NAN);
        put(&mut outputs, 2, 8, 6, 0.75, 2.0f32.ln());
        let faces = decode(&outputs, LEAST_SCORE).unwrap();
        let scores: Vec<f32> = faces.iter().map(|face| face.score).collect();
        assert_eq!(scores.len(), 2, "{scores:?}");
        assert!((scores[0] - 0.9).abs() < 1e-5 && (scores[1] - 0.75).abs() < 1e-5);
    }

    #[test]
    fn outputs_of_another_shape_are_refused() {
        let mut outputs = nothing();
        outputs[7].pop();
        assert!(decode(&outputs, LEAST_SCORE).is_err());
        assert!(decode(&nothing()[..11], LEAST_SCORE).is_err());
        assert!(decode(&nothing(), LEAST_SCORE).unwrap().is_empty());
    }

    #[test]
    fn a_file_that_is_not_the_pinned_model_is_refused() {
        let path =
            std::env::temp_dir().join(format!("lamp-eyes-not-a-model-{}", std::process::id()));
        std::fs::write(&path, b"not a model").unwrap();
        let refused = Detector::load(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(refused.is_err());
        assert!(Detector::load(Path::new("/nonexistent/model.onnx")).is_err());
    }
}
