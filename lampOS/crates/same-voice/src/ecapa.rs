//! Frozen SpeechBrain ECAPA-TDNN speaker embeddings for the isolated caller bench.
//! Input is one unpadded 16 kHz mono utterance. A fresh inference state is used
//! per call so reference and candidate audio never share activations.

use rustfft::{Fft, FftPlanner, num_complex::Complex};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, sync::Arc};
use tract_onnx::prelude::*;

pub const MODEL_FILE: &str = "ecapa-voxceleb.onnx";
pub const MODEL_SHA256: &str = "d8c32b0dbc17e31c7b13c9d7910f0adaca2ea0b0b3351da05041c9168e741994";
pub const DIMENSIONS: usize = 192;
const MODEL_BYTES: usize = 83_256_465;
const FFT: usize = 400;
const HOP: usize = 160;
const BANDS: usize = 80;
// The widest dilated convolution reflects four feature frames at each end.
const MIN_SAMPLES: usize = 4 * HOP;
const MAX_SAMPLES: usize = 12 * 16_000;

/// A loaded, read-only model. Preparing it once reuses the convolution weights
/// across calls. Each returned embedding is L2-normalized for cosine comparisons.
pub struct Ecapa {
    plan: Arc<TypedRunnableModel>,
    frontend: Filterbanks,
}

impl Ecapa {
    /// Open either the exported ONNX file or a local directory containing it.
    /// Only the frozen, checksummed graph is accepted.
    pub fn load(path: &Path) -> Result<Self, String> {
        let path = if path.is_dir() {
            path.join(MODEL_FILE)
        } else {
            path.to_path_buf()
        };
        let file = File::open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if file.metadata().map_err(|e| e.to_string())?.len() != MODEL_BYTES as u64 {
            return Err("wrong ECAPA model size".into());
        }
        let mut bytes = Vec::with_capacity(MODEL_BYTES);
        file.take(MODEL_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() != MODEL_BYTES || format!("{:x}", Sha256::digest(&bytes)) != MODEL_SHA256 {
            return Err("wrong ECAPA model SHA-256".into());
        }
        let tract_error = |error: TractError| format!("ECAPA: {error:#}");
        let model = tract_onnx::onnx()
            .model_for_read(&mut bytes.as_slice())
            .map_err(tract_error)?;
        drop(bytes);
        let frames = model.symbols.sym("T");
        let shape = [1.to_dim(), BANDS.to_dim(), frames.to_dim()];
        let plan = model
            .with_input_fact(0, f32::fact(shape).into())
            .map_err(tract_error)?
            .into_optimized()
            .map_err(tract_error)?
            .into_runnable()
            .map_err(tract_error)?;
        Ok(Self {
            plan,
            frontend: Filterbanks::new(),
        })
    }

    /// Embed the actual sample count, including short segments. Finite,
    /// normalized PCM is required. Digital silence produces no embedding.
    pub fn embed(&self, samples: &[f32]) -> Result<Vec<f32>, String> {
        if !(MIN_SAMPLES..=MAX_SAMPLES).contains(&samples.len()) {
            return Err("ECAPA expects 640 through 192000 samples at 16 kHz".into());
        }
        if !samples.iter().all(|s| (-1.0..=1.0).contains(s)) || samples.iter().all(|&s| s == 0.0) {
            return Err("ECAPA expects finite, non-silent 16 kHz PCM in [-1, 1]".into());
        }
        let (frames, features) = self.frontend.compute(samples);
        let tract_error = |error: TractError| format!("ECAPA: {error:#}");
        let input = Tensor::from_shape(&[1, BANDS, frames], &features).map_err(tract_error)?;
        let result = self.plan.run(tvec!(input.into())).map_err(tract_error)?;
        let vector = result[0]
            .to_plain_array_view::<f32>()
            .map_err(tract_error)?;
        if vector.len() != DIMENSIONS {
            return Err("invalid ECAPA output shape".into());
        }
        let norm = vector
            .iter()
            .map(|&x| f64::from(x).powi(2))
            .sum::<f64>()
            .sqrt();
        if !norm.is_finite() || norm == 0.0 {
            return Err("invalid ECAPA embedding".into());
        }
        Ok(vector
            .iter()
            .map(|&x| (f64::from(x) / norm) as f32)
            .collect())
    }
}

/// SpeechBrain Fbank uses centered, zero-padded 25 ms periodic Hamming
/// windows. Each triangular filter uses its *left* bandwidth on both sides.
struct Filterbanks {
    window: [f32; FFT],
    bands: Vec<(usize, Vec<f32>)>,
    fft: Arc<dyn Fft<f32>>,
}

impl Filterbanks {
    fn new() -> Self {
        let window = std::array::from_fn(|i| {
            (0.54 - 0.46 * (std::f64::consts::TAU * i as f64 / FFT as f64).cos()) as f32
        });
        let hz = |point: usize| {
            let mel_ratio = (1.0_f64 + 8000.0 / 700.0).log10() * point as f64 / (BANDS + 1) as f64;
            (700.0 * (10.0_f64.powf(mel_ratio) - 1.0)) as f32
        };
        let bands = (1..=BANDS)
            .map(|i| {
                let center = hz(i);
                let width = center - hz(i - 1);
                let weights: Vec<f32> = (0..=FFT / 2)
                    .map(|bin| (1.0 - ((40.0 * bin as f32 - center) / width).abs()).max(0.0))
                    .collect();
                let start = weights.iter().position(|&v| v > 0.0).unwrap_or(0);
                let end = weights
                    .iter()
                    .rposition(|&v| v > 0.0)
                    .map_or(start, |i| i + 1);
                (start, weights[start..end].to_vec())
            })
            .collect();
        Self {
            window,
            bands,
            fft: FftPlanner::new().plan_fft_forward(FFT),
        }
    }

    fn compute(&self, samples: &[f32]) -> (usize, Vec<f32>) {
        let frames = 1 + samples.len() / HOP;
        let mut features = vec![0.0; BANDS * frames];
        let mut spectrum = [Complex::<f32>::default(); FFT];
        let mut power = [0.0; FFT / 2 + 1];
        let mut scratch = vec![Complex::default(); self.fft.get_inplace_scratch_len()];
        let mut peak = f32::NEG_INFINITY;
        for frame in 0..frames {
            let center = frame * HOP;
            for (index, (slot, &window)) in spectrum.iter_mut().zip(&self.window).enumerate() {
                let value = (center + index)
                    .checked_sub(FFT / 2)
                    .and_then(|i| samples.get(i))
                    .copied()
                    .unwrap_or(0.0);
                *slot = Complex::new(value * window, 0.0);
            }
            self.fft.process_with_scratch(&mut spectrum, &mut scratch);
            for (bin, value) in power.iter_mut().zip(&spectrum) {
                *bin = value.norm_sqr();
            }
            for (band, (start, weights)) in self.bands.iter().enumerate() {
                let energy = weights
                    .iter()
                    .zip(&power[*start..])
                    .map(|(a, b)| a * b)
                    .sum::<f32>();
                let db = 10.0 * energy.max(1e-10).log10();
                features[band * frames + frame] = db;
                peak = peak.max(db);
            }
        }
        for band in features.chunks_exact_mut(frames) {
            let floor = peak - 80.0;
            for value in band.iter_mut() {
                *value = value.max(floor);
            }
            let mean = (band.iter().map(|&x| f64::from(x)).sum::<f64>() / frames as f64) as f32;
            for value in band {
                *value -= mean;
            }
        }
        (frames, features)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_unrelated_model_files_are_rejected() {
        assert!(
            Ecapa::load(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("Cargo.toml")
                    .as_path()
            )
            .is_err()
        );
        assert!(Ecapa::load(Path::new("/no-such-speechbrain-ecapa-model")).is_err());
    }

    #[test]
    fn filterbanks_use_actual_utterance_length_and_zero_center_padding() {
        let features = Filterbanks::new();
        for n in [640, 1600, 4807, 20480] {
            let (frames, fbank) = features.compute(&vec![0.0; n]);
            assert_eq!(frames, n / HOP + 1);
            assert_eq!(fbank.len(), BANDS * frames);
            assert!(fbank.iter().all(|&v| v == 0.0));
        }
    }

    /// The optional offline exporter generates these independent full vectors
    /// with the original SpeechBrain Fbank, InputNormalization and ECAPA_TDNN.
    #[test]
    #[ignore = "set LAMP_ECAPA_MODEL and LAMP_ECAPA_REFERENCE to prepared local files"]
    fn pretrained_embeddings_match_speechbrain_at_variable_lengths() {
        use std::io::Write;
        use std::time::Instant;
        let path = std::env::var_os("LAMP_ECAPA_MODEL").unwrap();
        let reference = std::path::PathBuf::from(std::env::var_os("LAMP_ECAPA_REFERENCE").unwrap());
        let record = std::env::var_os("LAMP_ECAPA_NATIVE_OUTPUT").map(std::path::PathBuf::from);
        let began = Instant::now();
        let model = Ecapa::load(Path::new(&path)).unwrap();
        eprintln!("load_ms={:.3}", began.elapsed().as_secs_f64() * 1000.);
        assert!(model.embed(&[0.5; 639]).is_err());
        assert!(model.embed(&[0.0; 1600]).is_err());
        assert!(model.embed(&[f32::NAN; 1600]).is_err());
        assert!(model.embed(&vec![0.1; MAX_SAMPLES + 1]).is_err());
        assert_eq!(
            model.embed(&vec![0.1; MAX_SAMPLES]).unwrap().len(),
            DIMENSIONS
        );
        let floats = |file| {
            std::fs::read(file)
                .unwrap()
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect::<Vec<_>>()
        };
        for n in [640, 1600, 4807, 12003, 20480, 32061, 48000] {
            let pcm = std::fs::read(reference.join(format!("{n}.pcm"))).unwrap();
            let samples: Vec<f32> = pcm
                .chunks_exact(2)
                .map(|b| f32::from(i16::from_le_bytes(b.try_into().unwrap())) / 32768.)
                .collect();
            assert_eq!(samples.len(), n);
            let (_, features) = model.frontend.compute(&samples);
            let ref_features = floats(reference.join(format!("{n}.fbank")));
            assert_eq!(features.len(), ref_features.len());
            let feature_err = features
                .iter()
                .zip(&ref_features)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0, f32::max);
            let expected = floats(reference.join(format!("{n}.embedding")));
            let started = Instant::now();
            let actual = model.embed(&samples).unwrap();
            let elapsed = started.elapsed().as_secs_f64() * 1000.;
            let cosine = crate::cosine(&actual, &expected).unwrap();
            let error = actual
                .iter()
                .zip(&expected)
                .map(|(x, y)| (x - y).abs())
                .fold(0.0, f32::max);
            if let Some(dir) = &record {
                let mut file =
                    std::fs::File::create_new(dir.join(format!("{n}.embedding"))).unwrap();
                for value in &actual {
                    file.write_all(&value.to_le_bytes()).unwrap();
                }
            }
            eprintln!(
                "samples={n} max_fbank_db_error={feature_err:.6} cosine={cosine:.10} max_embedding_error={error:.8} embedding_ms={elapsed:.3}"
            );
            assert!(cosine > 0.99999 && error < 0.001 && feature_err < 0.02);
        }
    }
}
