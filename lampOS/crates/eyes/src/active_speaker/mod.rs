//! Off-by-default Light-ASD component candidate for #8848.
//!
//! Used only by the standalone offline bench, never by voice admission or
//! a product service. Windows contain history; only their newest frame is
//! selected for a causal-window comparison. A full-window offline score
//! has future context and is not evidence of a live readiness deadline.
pub mod features;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs::File, io::Read, path::Path, sync::Arc};
use tract_onnx::prelude::*;

pub const ENVIRONMENT: &str = "LAMP_SPEAKING_FACE_SENSOR";
pub const SIDE: usize = 112;
pub const MAX_FACES: usize = 8;
pub const THRESHOLD: f32 = 0.5;
pub const MAX_MODEL_BYTES: u64 = 16 * 1024 * 1024;
pub const T25_SHA256: &str = "a2386f3d087af52809901a568df504ebdbdbe6161d6d958b3fccae31ec50352d";
pub const T50_SHA256: &str = "9c56974f3933cda59e5524e26aa9c5ef79e854ff78e43b09c0c1eb3adc5b1ef9";
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAsset {
    pub frames: usize,
    pub bytes: u64,
    pub sha256: String,
}

impl ModelAsset {
    fn valid(&self) -> bool {
        matches!(
            (self.frames, self.bytes, self.sha256.as_str()),
            (25, 4_103_414, T25_SHA256) | (50, 4_103_415, T50_SHA256)
        )
    }
}

/// The sole on-value; there is no implicit enable when a model is present.
pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

enum Engine {
    Tract(Arc<TypedRunnableModel>),
    Ort {
        session: ort::session::Session,
        _environment: ort::environment::Environment,
    },
}

pub struct Candidate {
    engine: Engine,
    asset: ModelAsset,
}

fn read_model(path: &Path, asset: &ModelAsset) -> Result<Vec<u8>> {
    if !asset.valid() || asset.bytes > MAX_MODEL_BYTES {
        return Err("invalid or unpinned active-speaker model specification".into());
    }
    let file = File::open(path).map_err(|_| "active-speaker model cannot be opened")?;
    if file.metadata()?.len() != asset.bytes {
        return Err("active-speaker model byte count differs".into());
    }
    let mut bytes = Vec::with_capacity(asset.bytes as usize);
    file.take(asset.bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != asset.bytes || format!("{:x}", Sha256::digest(&bytes)) != asset.sha256
    {
        return Err("active-speaker model SHA-256 differs".into());
    }
    Ok(bytes)
}

impl Candidate {
    /// Off returns before even validating paths/specification, so the default
    /// has no model load or media access. A bench runs in its own process.
    pub fn load(value: Option<&str>, path: &Path, asset: ModelAsset) -> Result<Option<Self>> {
        if !enabled(value) {
            return Ok(None);
        }
        let bytes = read_model(path, &asset)?;
        let frames = asset.frames;
        let plan = tract_onnx::onnx()
            .model_for_read(&mut bytes.as_slice())?
            .with_input_fact(0, f32::fact([1, frames * 4, features::COEFFICIENTS]).into())?
            .with_input_fact(1, f32::fact([1, frames, SIDE, SIDE]).into())?
            .into_optimized()?
            .into_runnable()?;
        Ok(Some(Self {
            engine: Engine::Tract(plan),
            asset,
        }))
    }

    /// Optional isolated board bench using the project's existing verified
    /// ARM64 ONNX Runtime library, one non-spinning CPU thread. No install.
    pub fn load_with_runtime(
        value: Option<&str>,
        path: &Path,
        asset: ModelAsset,
        runtime: &Path,
    ) -> Result<Option<Self>> {
        if !enabled(value) {
            return Ok(None);
        }
        let bytes = read_model(path, &asset)?;
        let library = lamp_turn_end::ONNX_RUNTIME.ok_or("no ONNX Runtime pin for this target")?;
        lamp_turn_end::fast::verify_runtime(runtime, &library)?;
        let environment = ort::init_from(runtime)?
            .with_name("lamp-speaking-face-bench")
            .with_telemetry(false)
            .build()?;
        environment.set_log_level(ort::logging::LogLevel::Error);
        let session = ort::session::Session::builder(&environment)
            .and_then(|b| Ok(b.with_log_level(ort::logging::LogLevel::Error)?))
            .and_then(|b| {
                Ok(b.with_optimization_level(ort::session::builder::GraphOptimizationLevel::All)?)
            })
            .and_then(|b| Ok(b.with_intra_threads(1)?))
            .and_then(|b| Ok(b.with_inter_threads(1)?))
            .and_then(|b| Ok(b.with_parallel_execution(false)?))
            .and_then(|b| Ok(b.with_intra_op_spinning(false)?))
            .and_then(|b| Ok(b.with_inter_op_spinning(false)?))
            .and_then(|mut b| b.commit_from_memory(&bytes))
            .map_err(|error| format!("active-speaker ONNX Runtime session: {error}"))?;
        Ok(Some(Self {
            engine: Engine::Ort {
                session,
                _environment: environment,
            },
            asset,
        }))
    }

    pub fn backend(&self) -> &'static str {
        match self.engine {
            Engine::Tract(_) => "tract-onnx-0.23.8",
            Engine::Ort { .. } => "onnxruntime-1.30.0-one-thread",
        }
    }

    pub fn asset(&self) -> &ModelAsset {
        &self.asset
    }

    /// The pinned export has logits, class-1 logit, and class-1 softmax
    /// probability, in that order. Pixel normalization is inside the graph.
    pub fn probabilities(&mut self, audio_mfcc: &[f32], visual_gray: &[f32]) -> Result<Vec<f32>> {
        let frames = self.asset.frames;
        if audio_mfcc.len() != frames * 4 * features::COEFFICIENTS
            || visual_gray.len() != frames * SIDE * SIDE
            || !audio_mfcc.iter().all(|v| v.is_finite())
            || !visual_gray
                .iter()
                .all(|v| v.is_finite() && (0.0..=255.0).contains(v))
        {
            return Err("invalid active-speaker input dimensions or numbers".into());
        }
        let probabilities: Vec<f32> = match &mut self.engine {
            Engine::Tract(plan) => {
                let audio =
                    Tensor::from_shape(&[1, frames * 4, features::COEFFICIENTS], audio_mfcc)?;
                let visual = Tensor::from_shape(&[1, frames, SIDE, SIDE], visual_gray)?;
                let outputs = plan.run(tvec!(audio.into(), visual.into()))?;
                if outputs.len() != 3 {
                    return Err("active-speaker export has unexpected outputs".into());
                }
                outputs[2]
                    .to_plain_array_view::<f32>()?
                    .iter()
                    .copied()
                    .collect()
            }
            Engine::Ort { session, .. } => {
                let audio = ort::value::Tensor::from_array((
                    [1, frames * 4, features::COEFFICIENTS],
                    audio_mfcc.to_vec(),
                ))?;
                let visual = ort::value::Tensor::from_array((
                    [1, frames, SIDE, SIDE],
                    visual_gray.to_vec(),
                ))?;
                let outputs = session.run(ort::inputs![audio, visual])?;
                if outputs.len() != 3 {
                    return Err("active-speaker export has unexpected outputs".into());
                }
                outputs[2].try_extract_tensor::<f32>()?.1.to_vec()
            }
        };
        if probabilities.len() != frames
            || !probabilities
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        {
            return Err("active-speaker export has invalid probabilities".into());
        }
        Ok(probabilities)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FaceScore {
    pub track_id: String,
    pub probability: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    FewerThanTwoFaces,
    TooManyFaces,
    MissingOrInvalidScore,
    NoActiveFace,
    MultipleActiveFaces,
    OneActiveFace,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Choice {
    /// A temporary track, `no_visible`, `unknown`, or `not_applicable`.
    pub output: String,
    pub reason: Reason,
}

/// Frozen before seeing benchmark predictions: 0.5 class-1 probability,
/// exactly one positive track. Missing evidence abstains; no largest-face,
/// identity, audio-only or top-score fallback can turn it into a person.
pub fn choose(faces: &[FaceScore]) -> Choice {
    let result = |output: &str, reason| Choice {
        output: output.to_owned(),
        reason,
    };
    if faces.len() < 2 {
        return result("not_applicable", Reason::FewerThanTwoFaces);
    }
    if faces.len() > MAX_FACES {
        return result("unknown", Reason::TooManyFaces);
    }
    let mut ids = BTreeSet::new();
    if faces.iter().any(|face| {
        face.track_id.is_empty()
            || face.track_id.len() > 128
            || matches!(
                face.track_id.as_str(),
                "unknown" | "no_visible" | "not_applicable"
            )
            || !ids.insert(&face.track_id)
            || !face
                .probability
                .is_some_and(|p| p.is_finite() && (0.0..=1.0).contains(&p))
    }) {
        return result("unknown", Reason::MissingOrInvalidScore);
    }
    let positive: Vec<_> = faces
        .iter()
        .filter(|face| face.probability.unwrap() >= THRESHOLD)
        .collect();
    match positive.as_slice() {
        [] => result("no_visible", Reason::NoActiveFace),
        [one] => result(&one.track_id, Reason::OneActiveFace),
        _ => result("unknown", Reason::MultipleActiveFaces),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(id: &str, probability: Option<f32>) -> FaceScore {
        FaceScore {
            track_id: id.into(),
            probability,
        }
    }

    #[test]
    fn off_does_not_open_a_model_or_validate_its_specification() {
        for setting in [None, Some("off"), Some("true"), Some("ON"), Some("")] {
            assert!(
                Candidate::load(
                    setting,
                    Path::new("/no/such/model"),
                    ModelAsset {
                        frames: 0,
                        bytes: u64::MAX,
                        sha256: String::new(),
                    }
                )
                .unwrap()
                .is_none()
            );
        }
    }

    #[test]
    fn only_two_or_more_faces_create_an_opportunity() {
        assert_eq!(choose(&[]).reason, Reason::FewerThanTwoFaces);
        assert_eq!(choose(&[face("a", Some(1.0))]).output, "not_applicable");
        assert_eq!(
            choose(&vec![face("a", Some(1.0)); MAX_FACES + 1]).reason,
            Reason::TooManyFaces
        );
    }

    #[test]
    fn unique_positive_track_wins_without_a_largest_face_fallback() {
        assert_eq!(
            choose(&[face("a", Some(0.1)), face("b", Some(0.8))]).output,
            "b"
        );
        assert_eq!(
            choose(&[face("a", Some(0.8)), face("b", Some(0.8))]).output,
            "unknown"
        );
        assert_eq!(
            choose(&[face("a", Some(0.1)), face("b", Some(0.1))]).output,
            "no_visible"
        );
    }

    #[test]
    fn missing_or_corrupt_evidence_never_chooses_the_other_face() {
        for bad in [None, Some(f32::NAN), Some(-1.0), Some(2.0)] {
            assert_eq!(
                choose(&[face("a", bad), face("b", Some(0.9))]).output,
                "unknown"
            );
        }
        assert_eq!(
            choose(&[face("a", Some(0.1)), face("a", Some(0.9))]).reason,
            Reason::MissingOrInvalidScore
        );
        assert_eq!(
            choose(&[face("unknown", Some(0.1)), face("b", Some(0.9))]).reason,
            Reason::MissingOrInvalidScore
        );
    }
}
