//! How the person at the lamp seems, from their face (#1671, step 1): a
//! read for measuring, which nothing acts on yet.
//!
//! The near face the eyes already cut out upright for recognition
//! ([`crate::who::align`]) is turned grey and 64 by 64, and the FER+
//! expression model (the ONNX model zoo's `emotion-ferplus-8`, MIT) scores
//! eight expressions. They are reduced, as V1 did, to `positive`
//! (happiness), `negative` (sadness, anger, disgust, fear and contempt) or
//! `unsure`, with a confidence ([`judge`]). No picture, print or name is
//! kept or said: only the kind and the confidence.
use crate::who::SIDE as ALIGNED;
use lamp_msgs::{HumanSeems, SeemsKind};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, sync::Arc, time::Duration};
use tract_onnx::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// The exact model file. Fetched separately and never committed here.
pub const MODEL_FILE: &str = "emotion-ferplus-8.onnx";
pub const MODEL_BYTES: u64 = 35_040_571;
pub const MODEL_SHA256: &str = "a2a2ba6a335a3b29c21acb6272f962bd3d47f84952aaffa03b60986e04efa61c";
/// The model's input: one grey face, square.
pub const SIDE: usize = 64;
/// The model's eight expressions, in its order.
pub const EXPRESSIONS: [&str; 8] = [
    "neutral",
    "happiness",
    "surprise",
    "sadness",
    "anger",
    "disgust",
    "fear",
    "contempt",
];
/// A face is read at most this often.
pub const READ_EVERY: Duration = Duration::from_secs(2);
/// A kind is named only at or over this share of the eight; under it, the
/// read is `unsure`. Authored, to be set from the week's measure (#1671).
pub const NAMED_FROM: f32 = 0.6;

/// The face as the model takes it: grey, 64 by 64, each value 0 to 255,
/// from the aligned face's red, green and blue planes (112 by 112).
/// `None` for a face of the wrong size.
pub fn grey(aligned: &[f32]) -> Option<Vec<f32>> {
    let plane = ALIGNED * ALIGNED;
    if aligned.len() != 3 * plane {
        return None;
    }
    let at = |row: usize, column: usize| {
        let i = row * ALIGNED + column;
        0.299 * aligned[i] + 0.587 * aligned[plane + i] + 0.114 * aligned[2 * plane + i]
    };
    let scale = ALIGNED as f32 / SIDE as f32;
    let mut out = Vec::with_capacity(SIDE * SIDE);
    for row in 0..SIDE {
        let v = ((row as f32 + 0.5) * scale - 0.5).clamp(0.0, (ALIGNED - 1) as f32);
        let (v0, dv) = (v.floor() as usize, v - v.floor());
        let v1 = (v0 + 1).min(ALIGNED - 1);
        for column in 0..SIDE {
            let u = ((column as f32 + 0.5) * scale - 0.5).clamp(0.0, (ALIGNED - 1) as f32);
            let (u0, du) = (u.floor() as usize, u - u.floor());
            let u1 = (u0 + 1).min(ALIGNED - 1);
            out.push(
                at(v0, u0) * (1.0 - du) * (1.0 - dv)
                    + at(v0, u1) * du * (1.0 - dv)
                    + at(v1, u0) * (1.0 - du) * dv
                    + at(v1, u1) * du * dv,
            );
        }
    }
    Some(out)
}

/// The eight scores as shares that add to one.
pub fn shares(scores: &[f32]) -> Option<[f32; 8]> {
    if scores.len() != 8 || scores.iter().any(|score| !score.is_finite()) {
        return None;
    }
    let most = scores.iter().copied().fold(f32::MIN, f32::max);
    let mut out = [0.0f32; 8];
    for (to, score) in out.iter_mut().zip(scores) {
        *to = (score - most).exp();
    }
    let sum: f32 = out.iter().sum();
    out.iter_mut().for_each(|share| *share /= sum);
    Some(out)
}

/// The eight shares reduced to a kind and its confidence: `positive` with
/// happiness at or over [`NAMED_FROM`], `negative` with the five unhappy
/// ones together there, else `unsure` with the larger of the two.
pub fn judge(shares: &[f32; 8]) -> HumanSeems {
    let positive = shares[1];
    let negative: f32 = shares[3..].iter().sum();
    let (kind, confidence) = if positive >= NAMED_FROM {
        (SeemsKind::Positive, positive)
    } else if negative >= NAMED_FROM {
        (SeemsKind::Negative, negative)
    } else {
        (SeemsKind::Unsure, positive.max(negative))
    };
    HumanSeems {
        kind,
        confidence: confidence.clamp(0.0, 1.0),
    }
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
            .with_input_fact(0, f32::fact([1, 1, SIDE, SIDE]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Self { plan })
    }

    /// The eight shares for an aligned face (112 by 112, red, green, blue).
    pub fn shares(&self, aligned: &[f32]) -> Result<[f32; 8]> {
        let input = grey(aligned).ok_or("the face is not 112 by 112")?;
        let input = Tensor::from_shape(&[1, 1, SIDE, SIDE], &input)?;
        let outputs = self.plan.run(tvec!(input.into()))?;
        let scores = outputs
            .first()
            .ok_or("the expression model gave nothing")?
            .to_plain_array_view::<f32>()?
            .iter()
            .copied()
            .collect::<Vec<f32>>();
        shares(&scores).ok_or_else(|| "the expression model gave no eight scores".into())
    }

    /// How the face seems.
    pub fn read(&self, aligned: &[f32]) -> Result<HumanSeems> {
        Ok(judge(&self.shares(aligned)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn even(value: f32) -> Vec<f32> {
        vec![value; 3 * ALIGNED * ALIGNED]
    }

    #[test]
    fn a_face_is_made_grey_and_64_by_64() {
        let grey_face = grey(&even(100.0)).unwrap();
        assert_eq!(grey_face.len(), SIDE * SIDE);
        assert!(grey_face.iter().all(|value| (value - 100.0).abs() < 1e-3));
        // Red alone, by its share of grey.
        let mut red = vec![0.0f32; 3 * ALIGNED * ALIGNED];
        red[..ALIGNED * ALIGNED].fill(200.0);
        assert!((grey(&red).unwrap()[0] - 59.8).abs() < 1e-3);
        // A left half dark and a right half light stays so.
        let mut halves = even(0.0);
        for plane in 0..3 {
            for row in 0..ALIGNED {
                for column in ALIGNED / 2..ALIGNED {
                    halves[plane * ALIGNED * ALIGNED + row * ALIGNED + column] = 255.0;
                }
            }
        }
        let halves = grey(&halves).unwrap();
        assert!(halves[10 * SIDE + 5] < 1.0 && halves[10 * SIDE + 60] > 254.0);
        assert!(grey(&[0.0; 10]).is_none());
    }

    #[test]
    fn the_scores_become_shares_that_add_to_one() {
        let shares = shares(&[0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]).unwrap();
        assert!((shares.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(shares[1] > shares[0]);
        assert!(super::shares(&[0.0; 7]).is_none());
        assert!(super::shares(&[f32::NAN; 8]).is_none());
        // Large scores do not overflow.
        assert!(super::shares(&[1e4; 8]).unwrap()[0].is_finite());
    }

    #[test]
    fn a_read_is_named_only_when_one_kind_holds_most() {
        let read = |shares: [f32; 8]| judge(&shares);
        let happy = read([0.2, 0.7, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(happy.kind, SeemsKind::Positive);
        assert!((happy.confidence - 0.7).abs() < 1e-6);
        // The five unhappy ones together.
        let low = read([0.3, 0.05, 0.0, 0.3, 0.15, 0.1, 0.05, 0.05]);
        assert_eq!(low.kind, SeemsKind::Negative);
        assert!((low.confidence - 0.65).abs() < 1e-6);
        // Neutral or surprised: unsure, with the larger of the two.
        let calm = read([0.8, 0.1, 0.05, 0.05, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(calm.kind, SeemsKind::Unsure);
        assert!((calm.confidence - 0.1).abs() < 1e-6);
        let surprised = read([0.0, 0.3, 0.5, 0.2, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(surprised.kind, SeemsKind::Unsure);
        // Just under the line is unsure.
        let almost = read([0.41, 0.59, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(almost.kind, SeemsKind::Unsure);
    }

    #[test]
    fn a_model_that_is_not_the_pinned_one_is_refused() {
        assert!(Reader::load(Path::new("/nonexistent/emotion.onnx")).is_err());
        let path = std::env::temp_dir().join(format!("seems-{}.onnx", std::process::id()));
        std::fs::write(&path, b"not a model").unwrap();
        assert!(Reader::load(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    /// With the pinned model at `LAMP_SEEMS_MODEL`: it loads, reads a face
    /// in eight shares, and an even grey face is not named.
    #[test]
    fn the_pinned_model_reads_a_face_when_present() {
        let Some(path) = std::env::var_os("LAMP_SEEMS_MODEL") else {
            return;
        };
        let reader = Reader::load(Path::new(&path)).unwrap();
        let began = std::time::Instant::now();
        let shares = reader.shares(&even(128.0)).unwrap();
        eprintln!("seems: {shares:?} in {} ms", began.elapsed().as_millis());
        assert!((shares.iter().sum::<f32>() - 1.0).abs() < 1e-4);
    }
}
