//! Whether two things said are the same voice (#829).
//!
//! A speaker embedding of each utterance, and the cosine between two. The
//! model is CAM++ from 3D-Speaker (Apache-2.0), as sherpa-onnx exports it:
//! 80 Kaldi filter banks a frame in, one vector out. The filter banks are
//! computed here as Kaldi computes them (25 ms frames every 10 ms, the
//! Povey window, pre-emphasis 0.97, the DC offset removed, a 512-point FFT,
//! 80 triangular bands from 20 Hz to 8 kHz on the mel scale, the log of
//! their power), with the utterance's mean taken off each band, as
//! 3D-Speaker does before the model.
//!
//! Measured on what the lamp's own microphone hears (`docs/same-voice.md`):
//! how often the same person is taken for another, and another for the
//! same. The voice runs it as a child of its own (`lamp-same-voice serve`,
//! [`serve`]) with `LAMP_SAME_VOICE_MODEL` set, and nothing else on the
//! lamp runs it.
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::{
    io::{self, Read, Write},
    path::Path,
    sync::Arc,
    time::Instant,
};
use tract_onnx::prelude::*;

/// Samples a second, the only rate taken.
pub const RATE: u32 = 16_000;
/// Filter banks a frame.
pub const BINS: usize = 80;
const FRAME: usize = 400;
const SHIFT: usize = 160;
const FFT_SIZE: usize = 512;
const PREEMPHASIS: f32 = 0.97;
const LOW_HZ: f32 = 20.0;

fn mel(hz: f32) -> f32 {
    1127.0 * (1.0 + hz / 700.0).ln()
}

/// Kaldi's filter banks for 16 kHz, as `torchaudio.compliance.kaldi.fbank`
/// computes them with its defaults and no dither.
pub struct Fbank {
    window: Vec<f32>,
    /// For each band, its first FFT bin and the weights from there.
    bands: Vec<(usize, Vec<f32>)>,
    fft: Arc<dyn Fft<f32>>,
}

impl Default for Fbank {
    fn default() -> Self {
        Self::new()
    }
}

impl Fbank {
    pub fn new() -> Self {
        let window = (0..FRAME)
            .map(|i| {
                let hann =
                    0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FRAME - 1) as f32).cos();
                hann.powf(0.85)
            })
            .collect();
        let nyquist = RATE as f32 / 2.0;
        let (low, high) = (mel(LOW_HZ), mel(nyquist));
        let step = (high - low) / (BINS + 1) as f32;
        let bins = FFT_SIZE / 2;
        let bands = (0..BINS)
            .map(|band| {
                let left = low + band as f32 * step;
                let center = left + step;
                let right = center + step;
                let weights: Vec<(usize, f32)> = (0..bins)
                    .filter_map(|bin| {
                        let m = mel(bin as f32 * RATE as f32 / FFT_SIZE as f32);
                        let weight = if m > left && m <= center {
                            (m - left) / (center - left)
                        } else if m > center && m < right {
                            (right - m) / (right - center)
                        } else {
                            0.0
                        };
                        (weight > 0.0).then_some((bin, weight))
                    })
                    .collect();
                let first = weights.first().map_or(0, |(bin, _)| *bin);
                let last = weights.last().map_or(0, |(bin, _)| *bin);
                let mut dense = vec![0.0; last + 1 - first];
                for (bin, weight) in weights {
                    dense[bin - first] = weight;
                }
                (first, dense)
            })
            .collect();
        Self {
            window,
            bands,
            fft: FftPlanner::new().plan_fft_forward(FFT_SIZE),
        }
    }

    /// The filter banks of `samples` (in [-1, 1]), one row a frame, the
    /// utterance's mean taken off each band. Empty when shorter than a
    /// frame.
    pub fn features(&self, samples: &[f32]) -> Vec<[f32; BINS]> {
        let mut rows = self.log_banks(samples);
        for band in 0..BINS {
            let mean = rows.iter().map(|row| row[band]).sum::<f32>() / rows.len().max(1) as f32;
            rows.iter_mut().for_each(|row| row[band] -= mean);
        }
        rows
    }

    /// The log filter banks, before the mean is taken off.
    fn log_banks(&self, samples: &[f32]) -> Vec<[f32; BINS]> {
        if samples.len() < FRAME {
            return Vec::new();
        }
        let frames = 1 + (samples.len() - FRAME) / SHIFT;
        let mut rows = Vec::with_capacity(frames);
        let mut buffer = vec![Complex::new(0.0f32, 0.0); FFT_SIZE];
        let mut frame = [0.0f32; FRAME];
        for index in 0..frames {
            frame.copy_from_slice(&samples[index * SHIFT..index * SHIFT + FRAME]);
            let mean = frame.iter().sum::<f32>() / FRAME as f32;
            frame.iter_mut().for_each(|sample| *sample -= mean);
            for i in (1..FRAME).rev() {
                frame[i] -= PREEMPHASIS * frame[i - 1];
            }
            frame[0] -= PREEMPHASIS * frame[0];
            for (slot, (sample, weight)) in buffer.iter_mut().zip(frame.iter().zip(&self.window)) {
                *slot = Complex::new(sample * weight, 0.0);
            }
            buffer[FRAME..].fill(Complex::new(0.0, 0.0));
            self.fft.process(&mut buffer);
            let mut row = [0.0f32; BINS];
            for (value, (first, weights)) in row.iter_mut().zip(&self.bands) {
                let energy: f32 = weights
                    .iter()
                    .zip(&buffer[*first..])
                    .map(|(weight, bin)| weight * bin.norm_sqr())
                    .sum();
                *value = energy.max(f32::EPSILON).ln();
            }
            rows.push(row);
        }
        rows
    }
}

/// The niceness the child takes: a little below ordinary, as `lamp-name`
/// runs at 5, so that its long work gives way to the lamp's other parts.
pub const NICE: i32 = 10;

/// The voice runs at real-time round-robin priority (its units'
/// `CPUSchedulingPolicy=rr`), and a process it starts takes that policy.
/// A print is long work on the lamp (the model loads in 15.5 s on
/// lamp-4ace): at the voice's priority it shares the voice's own threads'
/// time and starves everything at ordinary priority, and on lamp-4ace on
/// 18 October the provider worker missed its controller's heartbeat while
/// the child loaded (#1445). So the child takes the ordinary policy at
/// [`NICE`] before anything else. Lowering its own priority needs no
/// privilege; one already nicer than [`NICE`] stays so. The only unsafe
/// calls here: the standard library and rustix do not wrap them.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub fn ordinary_priority() -> io::Result<()> {
    let ordinary = libc::sched_param { sched_priority: 0 };
    // SAFETY: the calling thread's own policy, with a valid parameter that
    // outlives the call.
    if unsafe { libc::sched_setscheduler(0, libc::SCHED_OTHER, &ordinary) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the calling thread's own niceness; no memory is passed.
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, NICE) } != 0 {
        let error = io::Error::last_os_error();
        // Already nicer than that: raising it again would need privilege,
        // and lower is no harm.
        if !matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)) {
            return Err(error);
        }
    }
    Ok(())
}

/// Elsewhere the voice does not run at real-time priority.
#[cfg(not(target_os = "linux"))]
pub fn ordinary_priority() -> io::Result<()> {
    Ok(())
}

const PINNED_MODEL_BYTES: u64 = 29_596_978;
const PINNED_MODEL_SHA256: &str =
    "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b";

/// Read a bounded snapshot, then verify exactly the bytes the parser receives.
fn pinned_bytes(input: impl Read, length: u64, digest: &str) -> Result<Vec<u8>, String> {
    use sha2::{Digest, Sha256};
    let mut bytes = Vec::new();
    input
        .take(length + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 != length {
        return Err("pinned CAM++ model has an unexpected size".into());
    }
    if format!("{:x}", Sha256::digest(&bytes)) != digest {
        return Err("pinned CAM++ model does not match SHA256".into());
    }
    Ok(bytes)
}

/// The speaker model, ready to run on any number of frames.
pub struct Voice {
    plan: Arc<TypedRunnableModel>,
    fbank: Fbank,
}

impl Voice {
    pub fn load(path: &Path) -> Result<Self, String> {
        let said = |error: TractError| format!("{}: {error}", path.display());
        let model = tract_onnx::onnx()
            .with_ignore_value_info(true)
            .with_ignore_output_shapes(true)
            .model_for_path(path)
            .map_err(said)?;
        Self::from_model(model, path)
    }

    /// Identity prints require these exact bytes, checked on every child start.
    pub fn load_pinned(path: &Path) -> Result<Self, String> {
        let file =
            std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if file.metadata().map_err(|error| error.to_string())?.len() != PINNED_MODEL_BYTES {
            return Err("pinned CAM++ model has an unexpected size".into());
        }
        let bytes = pinned_bytes(file, PINNED_MODEL_BYTES, PINNED_MODEL_SHA256)?;
        let model = tract_onnx::onnx()
            .with_ignore_value_info(true)
            .with_ignore_output_shapes(true)
            .model_for_read(&mut bytes.as_slice())
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Self::from_model(model, path)
    }

    fn from_model(model: InferenceModel, path: &Path) -> Result<Self, String> {
        let said = |error: TractError| format!("{}: {error}", path.display());
        let frames = model.symbols.sym("T");
        let shape = [1.to_dim(), frames.to_dim(), (BINS as i64).to_dim()];
        let plan = model
            .with_input_fact(0, f32::fact(shape).into())
            .map_err(said)?
            .into_optimized()
            .map_err(said)?
            .into_runnable()
            .map_err(said)?;
        Ok(Self {
            plan,
            fbank: Fbank::new(),
        })
    }

    /// The embedding of `samples` (16 kHz, in [-1, 1]).
    pub fn embedding(&self, samples: &[f32]) -> Result<Vec<f32>, String> {
        let rows = self.fbank.features(samples);
        if rows.is_empty() {
            return Err("shorter than one frame".into());
        }
        let flat: Vec<f32> = rows.iter().flatten().copied().collect();
        let said = |error: TractError| error.to_string();
        let input = Tensor::from_shape(&[1, rows.len(), BINS], &flat).map_err(said)?;
        let output = self.plan.run(tvec!(input.into())).map_err(said)?;
        let first = output.first().ok_or("no output")?;
        Ok(first
            .to_plain_array_view::<f32>()
            .map_err(said)?
            .iter()
            .copied()
            .collect())
    }
}

/// The cosine between two embeddings: 1 for the same direction, whatever
/// their scale, summed in f64 so that neither very small nor very large
/// values lose it. None where it is not defined: vectors of different
/// lengths, an empty or all-zero one, or one with a value that is not
/// finite.
pub fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.iter().chain(b).any(|value| !value.is_finite()) {
        return None;
    }
    let (mut dot, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (f64::from(x), f64::from(y));
        dot += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa == 0.0 || bb == 0.0 {
        return None;
    }
    Some((dot / (aa.sqrt() * bb.sqrt())).clamp(-1.0, 1.0) as f32)
}

/// The most samples one request to [`serve`] may carry: 3 s.
pub const MOST_SAMPLES: usize = 3 * RATE as usize;
/// A request's header: its number (u64) and its count of samples (u32).
pub const HEADER_BYTES: usize = 12;

/// `lamp-same-voice serve MODEL`, the voice's child (#829): requests read
/// from `input`, one answer line written to `output` for each, until
/// `input` ends.
///
/// - **A request** is [`HEADER_BYTES`] bytes, the request's number (u64)
///   and its count of samples (u32), both little-endian, then that many
///   16-bit samples (16 kHz, one channel, little-endian).
/// - **Its answer** is one line of JSON: `{"request":N,"print":[...],"ms":M}`
///   with the print and how long it took, or `{"request":N,"error":"..."}`
///   when no print could be made of it.
/// - **First of all**, once the model is loaded: `{"ready":true,"loaded_ms":M}`.
///
/// A request of more than [`MOST_SAMPLES`] ends the serving with an error:
/// the voice never sends one.
pub fn serve(
    input: &mut impl Read,
    output: &mut impl Write,
    loaded_ms: u128,
    mut print: impl FnMut(&[f32]) -> Result<Vec<f32>, String>,
) -> io::Result<()> {
    writeln!(output, "{{\"ready\":true,\"loaded_ms\":{loaded_ms}}}")?;
    output.flush()?;
    loop {
        let mut header = [0u8; HEADER_BYTES];
        match input.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }
        let request = u64::from_le_bytes(header[..8].try_into().expect("8 bytes"));
        let count = u32::from_le_bytes(header[8..].try_into().expect("4 bytes")) as usize;
        if count > MOST_SAMPLES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("request {request}: {count} samples, more than {MOST_SAMPLES}"),
            ));
        }
        let mut bytes = vec![0u8; count * 2];
        input.read_exact(&mut bytes)?;
        let samples: Vec<f32> = bytes
            .chunks_exact(2)
            .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32_768.0)
            .collect();
        let started = Instant::now();
        let line = match print(&samples) {
            Ok(values) if !values.is_empty() && values.iter().all(|value| value.is_finite()) => {
                let values: Vec<String> = values.iter().map(f32::to_string).collect();
                format!(
                    "{{\"request\":{request},\"print\":[{}],\"ms\":{}}}",
                    values.join(","),
                    started.elapsed().as_millis()
                )
            }
            Ok(_) => format!("{{\"request\":{request},\"error\":\"no print\"}}"),
            Err(why) => format!("{{\"request\":{request},\"error\":\"{}\"}}", escaped(&why)),
        };
        writeln!(output, "{line}")?;
        output.flush()?;
    }
}

/// `text` as it may stand inside a JSON string.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out
}

/// A 16 kHz mono 16-bit WAV, in [-1, 1].
pub fn read_wav(path: &Path) -> Result<Vec<f32>, String> {
    let said = |error: hound::Error| format!("{}: {error}", path.display());
    let reader = hound::WavReader::open(path).map_err(said)?;
    let spec = reader.spec();
    if spec.sample_rate != RATE || spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(format!("{}: not 16 kHz mono 16-bit", path.display()));
    }
    reader
        .into_samples::<i16>()
        .map(|sample| {
            sample
                .map(|value| f32::from(value) / 32_768.0)
                .map_err(said)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_model_snapshot_rejects_size_and_digest_changes_before_parsing() {
        use sha2::{Digest, Sha256};
        let bytes = b"fixture model";
        let digest = format!("{:x}", Sha256::digest(bytes));
        assert_eq!(
            pinned_bytes(&bytes[..], bytes.len() as u64, &digest).unwrap(),
            bytes
        );
        for length in [bytes.len() as u64 - 1, bytes.len() as u64 + 1] {
            assert!(
                pinned_bytes(&bytes[..], length, &digest)
                    .unwrap_err()
                    .contains("size")
            );
        }
        assert!(
            pinned_bytes(&bytes[..], bytes.len() as u64, "wrong")
                .unwrap_err()
                .contains("SHA256")
        );
        let path = std::env::temp_dir().join(format!("lamp-pinned-model-{}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        assert!(Voice::load_pinned(&path).err().unwrap().contains("size"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_frame_is_25_ms_every_10_ms_and_each_band_is_centred() {
        let fbank = Fbank::new();
        assert!(fbank.features(&[0.1; 399]).is_empty());
        let one_second = vec![0.0; 16_000];
        assert_eq!(fbank.features(&one_second).len(), 98);
        // A tone lights the band around it most, and the mean is taken off.
        let tone: Vec<f32> = (0..16_000)
            .map(|i| (2.0 * std::f32::consts::PI * 1_000.0 * i as f32 / 16_000.0).sin() * 0.5)
            .collect();
        let raw = fbank.log_banks(&tone);
        let loudest = (0..BINS)
            .max_by(|a, b| raw[40][*a].total_cmp(&raw[40][*b]))
            .unwrap();
        let rows = fbank.features(&tone);
        let center_hz = |band: usize| {
            let (low, high) = (mel(LOW_HZ), mel(8_000.0));
            let m = low + (band + 1) as f32 * (high - low) / (BINS + 1) as f32;
            700.0 * ((m / 1127.0).exp() - 1.0)
        };
        assert!(
            (center_hz(loudest) - 1_000.0).abs() < 60.0,
            "{}",
            center_hz(loudest)
        );
        for band in 0..BINS {
            let mean: f32 = rows.iter().map(|row| row[band]).sum::<f32>() / rows.len() as f32;
            assert!(mean.abs() < 1e-3);
        }
    }

    /// #1445: the child leaves the voice's real-time policy for the
    /// ordinary one, at [`NICE`] or nicer. A test cannot take a real-time
    /// policy without privilege, so the step from it is not shown here.
    #[cfg(target_os = "linux")]
    #[test]
    #[allow(unsafe_code)]
    fn the_child_takes_the_ordinary_policy_a_little_below_ordinary_priority() {
        std::thread::spawn(|| {
            ordinary_priority().unwrap();
            // SAFETY: reads the calling thread's own policy.
            assert_eq!(unsafe { libc::sched_getscheduler(0) }, libc::SCHED_OTHER);
            // SAFETY: reads the calling thread's own niceness.
            let nice = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
            assert!(nice >= NICE, "{nice}");
        })
        .join()
        .unwrap();
    }

    /// A request as the voice writes it.
    fn request(number: u64, samples: &[i16]) -> Vec<u8> {
        let mut bytes = number.to_le_bytes().to_vec();
        bytes.extend((samples.len() as u32).to_le_bytes());
        bytes.extend(samples.iter().flat_map(|sample| sample.to_le_bytes()));
        bytes
    }

    #[test]
    fn serving_answers_each_request_with_its_print_or_why_not_until_its_input_ends() {
        let mut input = request(7, &[16_384, -16_384]);
        input.extend(request(8, &[]));
        input.extend(request(9, &[1]));
        let mut output = Vec::new();
        let mut seen = Vec::new();
        serve(&mut input.as_slice(), &mut output, 1_800, |samples| {
            seen.push(samples.to_vec());
            match samples.len() {
                2 => Ok(vec![0.5, -0.25]),
                0 => Err("shorter than \"one\" frame".into()),
                _ => Ok(vec![f32::NAN]),
            }
        })
        .unwrap();
        assert_eq!(seen, [vec![0.5, -0.5], vec![], vec![1.0 / 32_768.0]]);
        let lines: Vec<&str> = std::str::from_utf8(&output).unwrap().lines().collect();
        assert_eq!(lines[0], r#"{"ready":true,"loaded_ms":1800}"#);
        assert!(
            lines[1].starts_with(r#"{"request":7,"print":[0.5,-0.25],"ms":"#),
            "{}",
            lines[1]
        );
        assert_eq!(
            lines[2],
            r#"{"request":8,"error":"shorter than \"one\" frame"}"#
        );
        assert_eq!(lines[3], r#"{"request":9,"error":"no print"}"#);
        assert_eq!(lines.len(), 4);
        // More than 3 s ends it; a request cut short is an error too.
        let long = request(10, &vec![0; MOST_SAMPLES + 1]);
        assert!(serve(&mut long.as_slice(), &mut Vec::new(), 0, |_| Ok(vec![1.0])).is_err());
        let cut = &request(11, &[1, 2, 3])[..HEADER_BYTES + 2];
        assert!(serve(&mut &cut[..], &mut Vec::new(), 0, |_| Ok(vec![1.0])).is_err());
    }

    #[test]
    fn the_cosine_is_one_for_the_same_and_zero_for_a_right_angle() {
        assert!((cosine(&[1.0, 2.0], &[2.0, 4.0]).unwrap() - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 3.0]).unwrap().abs() < 1e-6);
        // The same at any scale (the reviewer's on #865).
        assert_eq!(cosine(&[0.0001, 0.0], &[0.0001, 0.0]), Some(1.0));
        assert_eq!(cosine(&[1e-30, 2e-30], &[2e-30, 4e-30]), Some(1.0));
        assert_eq!(cosine(&[1e20, 0.0], &[1e20, 0.0]), Some(1.0));
        assert_eq!(cosine(&[3e38, -3e38], &[-3e38, 3e38]), Some(-1.0));
        // Not defined: said so, never a number.
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), None);
        assert_eq!(cosine(&[], &[]), None);
        assert_eq!(cosine(&[1.0], &[1.0, 0.0]), None);
        assert_eq!(cosine(&[f32::NAN, 1.0], &[1.0, 1.0]), None);
        assert_eq!(cosine(&[f32::INFINITY, 1.0], &[1.0, 1.0]), None);
    }
}
