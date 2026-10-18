//! The fast model: the pinned float model, rewritten to 8-bit weight
//! multiplications in memory ([`crate::eight_bit`]) and run by ONNX Runtime on
//! two threads.
//!
//! ONNX Runtime is a native (C++) library. It is loaded at run time, here only
//! in this worker process, from a file installed beside the worker whose size
//! and SHA256 are pinned here and checked before it is loaded. The only other
//! part that loads it is lamp-eyes, with `--mouth-runtime` (#720), checked
//! against this same pin; the interaction core never loads it.
use crate::{FRAMES, MELS, ModelAsset, Result, eight_bit, model::read_pinned};
use ort::{
    environment::Environment,
    logging::LogLevel,
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};
use std::path::Path;
use tract_onnx::prelude::Framework;

/// Threads the fast model uses unless told otherwise: one for each of the
/// lamp's two fast cores.
pub const FAST_THREADS: usize = 2;

/// Weight multiplications of the pinned model kept in 32-bit floats: the
/// third layer's projection back from 1,536 to 384 values, whose input (the
/// output of a GELU) has outliers that one 8-bit scale per request cannot
/// hold. Chosen on the development clips (shards 0 and 1 of the labelled
/// speech), where it alone took the disagreements with the exact model at 0.5
/// from 13 to 4, and confirmed on the held-out shards 4 and 5 (21 to 0)
/// (docs/endpoint-pilot.md, "The fast model"). It costs about 2 ms of 27.
pub const KEPT_IN_FLOATS: &[&str] = &["node_MatMul_223"];

/// A native library pinned by size and SHA256, fetched separately and never
/// committed (docs/endpoint-detection.md, "The fast model").
#[derive(Clone, Copy, Debug)]
pub struct RuntimeAsset {
    pub file_name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub version: &'static str,
    pub source: &'static str,
}

/// ONNX Runtime 1.30.0 as Microsoft publishes it for this target: the file
/// `lib/<file_name>` inside the release archive at `source`.
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub const ONNX_RUNTIME: Option<RuntimeAsset> = Some(RuntimeAsset {
    file_name: "libonnxruntime.so.1.30.0",
    bytes: 25_135_496,
    sha256: "64e903a43a041240fd6bcffe0ac6d4fea47ef87bf24b9d097801bd00a9612a4b",
    version: "1.30.0",
    source: "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-linux-aarch64-1.30.0.tgz",
});

/// The same release for development on Apple Silicon.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const ONNX_RUNTIME: Option<RuntimeAsset> = Some(RuntimeAsset {
    file_name: "libonnxruntime.1.30.0.dylib",
    bytes: 43_879_424,
    sha256: "bcc9110f9d638a119de2db7afb3ba9a1da8085f0cb3401e1c48ae1caf450b6fa",
    version: "1.30.0",
    source: "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-osx-arm64-1.30.0.tgz",
});

/// No ONNX Runtime is pinned for other targets; the fast model is refused.
#[cfg(not(all(target_arch = "aarch64", any(target_os = "linux", target_os = "macos"))))]
pub const ONNX_RUNTIME: Option<RuntimeAsset> = None;

/// The pinned library this build expects.
pub fn runtime_asset() -> Result<RuntimeAsset> {
    ONNX_RUNTIME.ok_or_else(|| "no ONNX Runtime is pinned for this target".into())
}

/// Checks the library's exact bytes. It is read once here and then loaded by
/// path; the install directory is the same one that holds the worker.
pub fn verify_runtime(path: &Path, asset: &RuntimeAsset) -> Result<()> {
    read_pinned(path, asset.bytes, asset.sha256).map(drop)
}

pub struct FastTurn {
    // Field order is drop order: the session goes before its environment.
    session: Session,
    _environment: Environment,
    rewrite: eight_bit::Rewrite,
}

impl FastTurn {
    /// Verifies the pinned library and model, rewrites the model to 8 bits
    /// and opens it with `threads` threads. ONNX Runtime can be loaded once
    /// per process; a second call fails.
    pub fn load(model: &Path, asset: &ModelAsset, runtime: &Path, threads: usize) -> Result<Self> {
        Self::load_with(model, asset, runtime, threads, true)
    }

    /// [`Self::load`], with ONNX Runtime's threads spinning while they wait
    /// for work (its default) or not. A worker the voice starts runs at the
    /// voice's real-time priority, and a spinning thread at that priority
    /// holds its core from everything at ordinary priority there, `lamp-name`
    /// among them on the same two fast cores (#1608). Not spinning, its
    /// threads sleep as soon as a verdict is done.
    pub fn load_with(
        model: &Path,
        asset: &ModelAsset,
        runtime: &Path,
        threads: usize,
        spin: bool,
    ) -> Result<Self> {
        if threads == 0 {
            return Err("the fast model needs at least one thread".into());
        }
        let library = runtime_asset()?;
        verify_runtime(runtime, &library)
            .map_err(|error| format!("ONNX Runtime library: {error}"))?;
        let bytes = read_pinned(model, asset.bytes, asset.sha256)?;
        let mut proto = tract_onnx::onnx().proto_model_for_read(&mut bytes.as_slice())?;
        drop(bytes);
        let rewrite = eight_bit::rewrite(&mut proto, KEPT_IN_FLOATS)?;
        let encoded = prost::Message::encode_to_vec(&proto);
        drop(proto);
        let environment = ort::init_from(runtime)
            .map_err(|error| format!("ONNX Runtime {}: {error}", runtime.display()))?
            .with_name("lamp-turn-end")
            .with_telemetry(false)
            .build()
            .map_err(|error| error.to_string())?;
        environment.set_log_level(LogLevel::Error);
        let session = Session::builder(&environment)
            .and_then(|builder| Ok(builder.with_log_level(LogLevel::Error)?))
            .and_then(|builder| Ok(builder.with_optimization_level(GraphOptimizationLevel::All)?))
            .and_then(|builder| Ok(builder.with_intra_threads(threads)?))
            .and_then(|builder| Ok(builder.with_inter_threads(1)?))
            .and_then(|builder| Ok(builder.with_parallel_execution(false)?))
            .and_then(|builder| {
                Ok(if spin {
                    builder
                } else {
                    builder
                        .with_intra_op_spinning(false)?
                        .with_inter_op_spinning(false)?
                })
            })
            .and_then(|mut builder| builder.commit_from_memory(&encoded))
            .map_err(|error| format!("ONNX Runtime session: {error}"))?;
        Ok(Self {
            session,
            _environment: environment,
            rewrite,
        })
    }

    /// What the in-memory rewrite changed.
    pub fn rewrite(&self) -> eight_bit::Rewrite {
        self.rewrite
    }

    /// Probability that the analyzed speech is a finished turn.
    pub fn probability(&mut self, features: Vec<f32>) -> Result<f32> {
        if features.len() != MELS * FRAMES || features.iter().any(|value| !value.is_finite()) {
            return Err("invalid Smart Turn features".into());
        }
        let input = Tensor::from_array(([1usize, MELS, FRAMES], features))?;
        let outputs = self.session.run(ort::inputs![input])?;
        let (_, values) = outputs[0].try_extract_tensor::<f32>()?;
        match values {
            [probability] if (0.0..=1.0).contains(probability) => Ok(*probability),
            _ => Err("Smart Turn returned an invalid probability".into()),
        }
    }
}
