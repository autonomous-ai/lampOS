//! How the person at the lamp seems, from their face's movements: the
//! product's reader for #1671 (the lead, 20 Oct 08:16), beside FER+
//! ([`crate::seems`]), which is only for the measure on our own lamps.
//!
//! Google's Blendshape V2 (Apache 2.0, from the same MediaPipe
//! FaceLandmarker download as the mouth's mesh) takes 146 of Face Mesh V2's
//! 478 points and scores 52 movements of the face: smiles, frowns, brows,
//! sneers. They are movements, not feelings, so how they are reduced to
//! `positive`, `negative` or `unsure` ([`judge`]) is ours, and is what the
//! measure tests. No picture, point or score is kept or said: only the kind
//! and the confidence, on `humans/seems` as FER+ says it.
use lamp_msgs::{HumanSeems, SeemsKind, SeemsReader};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, sync::Arc};
use tract_onnx::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// `models/blendshapes/SOURCE.md`: made by `training/mouth/make.sh`, never
/// committed here.
pub const MODEL_FILE: &str = "face_blendshapes.onnx";
pub const MODEL_BYTES: u64 = 1_882_583;
pub const MODEL_SHA256: &str = "68056086da2409d47891e88b7f44d331d60e3d0bc053f5ff0fd51b0915f11ddd";
/// The points the model takes, as MediaPipe's own blendshapes graph picks
/// them from the mesh's 478 (`face_blendshapes_graph.cc`,
/// `kLandmarksSubsetIdxs`).
pub const POINTS: [usize; 146] = [
    0, 1, 4, 5, 6, 7, 8, 10, 13, 14, 17, 21, 33, 37, 39, 40, 46, 52, 53, 54, 55, 58, 61, 63, 65,
    66, 67, 70, 78, 80, 81, 82, 84, 87, 88, 91, 93, 95, 103, 105, 107, 109, 127, 132, 133, 136,
    144, 145, 146, 148, 149, 150, 152, 153, 154, 155, 157, 158, 159, 160, 161, 162, 163, 168, 172,
    173, 176, 178, 181, 185, 191, 195, 197, 234, 246, 249, 251, 263, 267, 269, 270, 276, 282, 283,
    284, 285, 288, 291, 293, 295, 296, 297, 300, 308, 310, 311, 312, 314, 317, 318, 321, 323, 324,
    332, 334, 336, 338, 356, 361, 362, 365, 373, 374, 375, 377, 378, 379, 380, 381, 382, 384, 385,
    386, 387, 388, 389, 390, 397, 398, 400, 402, 405, 409, 415, 454, 466, 468, 469, 470, 471, 472,
    473, 474, 475, 476, 477,
];
/// The model's scores, 52 movements in MediaPipe's order (`_neutral`
/// first); these are the ones read here, each a left and a right. The
/// brows drawn down aren't among them: on MediaPipe's own smiling test
/// portrait they score 0.84, beside the smiles' 0.95.
pub const SCORES: usize = 52;
const MOUTH_FROWN: [usize; 2] = [30, 31];
const MOUTH_SMILE: [usize; 2] = [44, 45];
const NOSE_SNEER: [usize; 2] = [50, 51];
/// A kind is named only at or over this; under it, the read is `unsure`.
/// Authored, to be set from the week's measure (#1671).
pub const NAMED_FROM: f32 = 0.5;

/// The model's input from the mesh's landmarks (x, y and z each, in the
/// crop's pixels with the eyes level): the 146 points' x and y, turned back
/// by the eye line's angle (`eye_line`, its sine and cosine) to the frame's
/// own, as MediaPipe gives them. The model doesn't mind where the points
/// sit or how large the face is, but it does mind a turn. `None` if the
/// landmarks aren't all there or aren't numbers.
pub fn points(landmarks: &[f32], eye_line: (f32, f32)) -> Option<Vec<f32>> {
    if landmarks.len() != crate::mesh::LANDMARKS * 3 {
        return None;
    }
    let (sin, cos) = eye_line;
    let mut out = Vec::with_capacity(POINTS.len() * 2);
    for at in POINTS {
        let (x, y) = (landmarks[at * 3], landmarks[at * 3 + 1]);
        out.push(x * cos - y * sin);
        out.push(x * sin + y * cos);
    }
    out.iter().all(|value| value.is_finite()).then_some(out)
}

/// The 52 scores reduced to a kind and its confidence: `positive` with the
/// smiles at or over [`NAMED_FROM`] (and over the other), `negative` with
/// the frowns or the nose wrinkled there, else `unsure` with the larger of
/// the two. Each movement is its left and right's mean. `None` for scores
/// that aren't 52 numbers.
pub fn judge(scores: &[f32]) -> Option<HumanSeems> {
    if scores.len() != SCORES || scores.iter().any(|score| !score.is_finite()) {
        return None;
    }
    let both = |pair: [usize; 2]| (scores[pair[0]] + scores[pair[1]]) / 2.0;
    let positive = both(MOUTH_SMILE);
    let negative = both(MOUTH_FROWN).max(both(NOSE_SNEER));
    let (kind, confidence) = if positive >= NAMED_FROM && positive >= negative {
        (SeemsKind::Positive, positive)
    } else if negative >= NAMED_FROM {
        (SeemsKind::Negative, negative)
    } else {
        (SeemsKind::Unsure, positive.max(negative))
    };
    Some(HumanSeems {
        kind,
        confidence: confidence.clamp(0.0, 1.0),
        from: SeemsReader::Blendshapes,
    })
}

pub struct Reader {
    plan: Arc<TypedRunnableModel>,
}

impl Reader {
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
            .model_for_read(&mut bytes.as_slice())?
            .with_input_fact(0, f32::fact([1, POINTS.len(), 2]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self { plan })
    }

    /// The 52 scores for one face's [`points`].
    pub fn scores(&self, points: &[f32]) -> Result<Vec<f32>> {
        let input = Tensor::from_shape(&[1, POINTS.len(), 2], points)?;
        let outputs = self.plan.run(tvec!(input.into()))?;
        Ok(outputs
            .first()
            .ok_or("the model gave no output")?
            .to_plain_array_view::<f32>()?
            .iter()
            .copied()
            .collect())
    }

    /// How the face whose [`points`] these are seems.
    pub fn read(&self, points: &[f32]) -> Result<HumanSeems> {
        judge(&self.scores(points)?).ok_or_else(|| "the model gave no 52 scores".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scores(set: &[(usize, f32)]) -> Vec<f32> {
        let mut out = vec![0.0f32; SCORES];
        for &(at, score) in set {
            out[at] = score;
        }
        out
    }

    #[test]
    fn a_smile_seems_positive_and_a_frown_or_sneer_negative() {
        let positive = judge(&scores(&[(44, 0.8), (45, 0.6)])).unwrap();
        assert_eq!(positive.kind, SeemsKind::Positive);
        assert_eq!(positive.from, SeemsReader::Blendshapes);
        assert!((positive.confidence - 0.7).abs() < 1e-6);
        for pair in [MOUTH_FROWN, NOSE_SNEER] {
            let read = judge(&scores(&[(pair[0], 0.9), (pair[1], 0.7)])).unwrap();
            assert_eq!(read.kind, SeemsKind::Negative, "{pair:?}");
            assert!((read.confidence - 0.8).abs() < 1e-6, "{pair:?}");
        }
    }

    #[test]
    fn under_the_line_or_on_one_side_only_it_is_unsure() {
        // Both sides at 0.49: under the line, unsure with the larger.
        let read = judge(&scores(&[(44, 0.49), (45, 0.49), (30, 0.2), (31, 0.2)])).unwrap();
        assert_eq!(read.kind, SeemsKind::Unsure);
        assert!((read.confidence - 0.49).abs() < 1e-6);
        // A smile on one side only (a smirk) is half of one.
        let read = judge(&scores(&[(44, 0.9)])).unwrap();
        assert_eq!(read.kind, SeemsKind::Unsure);
        // Exactly on the line is named.
        let read = judge(&scores(&[(30, NAMED_FROM), (31, NAMED_FROM)])).unwrap();
        assert_eq!(read.kind, SeemsKind::Negative);
    }

    #[test]
    fn a_smile_with_a_stronger_frown_is_negative() {
        let read = judge(&scores(&[(44, 0.6), (45, 0.6), (30, 0.9), (31, 0.9)])).unwrap();
        assert_eq!(read.kind, SeemsKind::Negative);
    }

    /// MediaPipe's smiling test portrait, as its own pipeline scores it:
    /// the brows drawn down at 0.84 don't make a smile negative.
    #[test]
    fn a_smile_with_the_brows_drawn_down_is_positive() {
        let read = judge(&scores(&[(1, 0.839), (2, 0.818), (44, 0.960), (45, 0.934)])).unwrap();
        assert_eq!(read.kind, SeemsKind::Positive);
        let read = judge(&scores(&[(1, 0.9), (2, 0.9)])).unwrap();
        assert_eq!(read.kind, SeemsKind::Unsure);
    }

    #[test]
    fn scores_that_are_not_52_numbers_give_nothing() {
        assert!(judge(&[0.5; 51]).is_none());
        assert!(judge(&scores(&[(7, f32::NAN)])).is_none());
    }

    #[test]
    fn the_146_points_are_picked_and_turned_back_to_the_frames_angle() {
        let mut landmarks = vec![0.0f32; crate::mesh::LANDMARKS * 3];
        for (at, point) in landmarks.chunks_mut(3).enumerate() {
            point[0] = at as f32;
            point[1] = 1000.0 + at as f32;
            point[2] = -1.0;
        }
        // Level eyes: the points as they are, in MediaPipe's order.
        let level = points(&landmarks, (0.0, 1.0)).unwrap();
        assert_eq!(level.len(), 292);
        assert_eq!(&level[..4], &[0.0, 1000.0, 1.0, 1001.0]);
        assert_eq!(&level[290..], &[477.0, 1477.0]);
        assert_eq!(&level[24..26], &[33.0, 1033.0]);
        // The eye line a quarter turn down: (x, y) becomes (-y, x).
        let turned = points(&landmarks, (1.0, 0.0)).unwrap();
        assert_eq!(&turned[2..4], &[-1001.0, 1.0]);
        assert!(points(&landmarks[..30], (0.0, 1.0)).is_none());
        landmarks[0] = f32::INFINITY;
        assert!(points(&landmarks, (0.0, 1.0)).is_none());
    }

    #[test]
    fn the_points_are_distinct_and_in_order() {
        assert!(POINTS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(POINTS.iter().all(|&at| at < crate::mesh::LANDMARKS));
    }

    #[test]
    fn a_file_that_is_not_the_pinned_model_is_refused() {
        let path =
            std::env::temp_dir().join(format!("lamp-eyes-not-blendshapes-{}", std::process::id()));
        std::fs::write(&path, b"not a model").unwrap();
        let refused = Reader::load(&path);
        std::fs::remove_file(&path).unwrap();
        assert!(refused.is_err());
        assert!(Reader::load(Path::new("/nonexistent/face_blendshapes.onnx")).is_err());
    }

    /// With the model made by `training/mouth/make.sh` in
    /// `lampOS/.cache/mouth/`: tract runs it, and its scores for set points
    /// are the ones ONNX Runtime gives (`blendshapes_parity.py` compares
    /// that with TFLite).
    #[test]
    #[ignore = "needs face_blendshapes.onnx from training/mouth/make.sh"]
    fn the_pinned_model_scores_52_movements() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.cache/mouth")
            .join(MODEL_FILE);
        let reader = Reader::load(&path).unwrap();
        let points: Vec<f32> = (0..POINTS.len())
            .flat_map(|at| [(at % 13) as f32 * 15.0, (at / 13) as f32 * 18.0])
            .collect();
        let scores = reader.scores(&points).unwrap();
        assert_eq!(scores.len(), SCORES);
        assert!(scores.iter().all(|score| (0.0..=1.0).contains(score)));
        reader.read(&points).unwrap();
        if let Ok(expected) = std::env::var("LAMP_BLENDSHAPES_GRID") {
            let expected: Vec<f32> = expected
                .split(',')
                .map(|score| score.parse().unwrap())
                .collect();
            let worst = scores
                .iter()
                .zip(&expected)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(worst < 1e-4, "{worst}");
        }
    }
}
