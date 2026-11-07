//! One WAV through the processor the voice gives mic2 (#5540), offline.
//!
//! The processor is built as `lamp-voice`'s capture worker builds it
//! (`EchoProcessor::with_reference(noise_suppression, Reference::AsGiven)`,
//! the linear output kept only when asked). There is no speaker here, so its
//! render reference is silence: two silent blocks as the speaker's prime,
//! then one silent 10 ms render block before each 10 ms capture block, the
//! render ahead of the capture as with the fixed margin. Nothing else stands
//! between ALSA and `mic2/audio` in software: no gain, no AGC. What the
//! board and the ALSA `plug` did before that is in the WAV or it is not.
use crate::{
    CAPTURE_RATE, CAPTURE_SAMPLES, CaptureBlock, EchoProcessor, RENDER_SAMPLES, Reference, Result,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The speaker's prime: 480 zero frames, two render blocks, after a reset.
const PRIME_BLOCKS: usize = 2;
/// The canceller's delay hint. With a silent reference there is no echo to
/// align, so the hint has nothing to act on; the lamp's own varies.
const DELAY_HINT_MS: u16 = 0;

/// What [`process`] did, for its one summary line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Processed {
    pub blocks: usize,
    pub samples: usize,
    pub input_dbfs: f64,
    pub output_dbfs: f64,
    /// Blocks that had no linear output (only with `linear`), written as
    /// silence.
    pub linear_missing: usize,
}

/// Reads `input` (16 kHz, mono, 16-bit PCM only), runs it through the mic2
/// processor with noise suppression `on` or off, and writes the final capture
/// output, or with `linear` the canceller's linear output, to `output` at the
/// input's length. Nothing is written if the input is refused.
pub fn process(
    input: &Path,
    output: &Path,
    noise_suppression: bool,
    linear: bool,
) -> Result<Processed> {
    let mut reader = hound::WavReader::open(input)?;
    let spec = reader.spec();
    if spec.sample_rate != CAPTURE_RATE
        || spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is {} Hz, {} channel(s), {}-bit {:?}; only 16 kHz mono 16-bit PCM is taken",
                input.display(),
                spec.sample_rate,
                spec.channels,
                spec.bits_per_sample,
                spec.sample_format
            ),
        )
        .into());
    }
    let samples = reader
        .samples::<i16>()
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut echo = EchoProcessor::with_reference(noise_suppression, Reference::AsGiven);
    if linear {
        echo = echo.with_linear_output();
    }
    let silence = [0i16; RENDER_SAMPLES];
    for _ in 0..PRIME_BLOCKS {
        echo.render(&silence)?;
    }
    let mut processed = Vec::with_capacity(samples.len().next_multiple_of(CAPTURE_SAMPLES));
    let mut linear_missing = 0;
    let blocks = samples.len().div_ceil(CAPTURE_SAMPLES);
    for chunk in samples.chunks(CAPTURE_SAMPLES) {
        // The tail is padded with silence; the output is trimmed back below.
        let mut block: CaptureBlock = [0; CAPTURE_SAMPLES];
        block[..chunk.len()].copy_from_slice(chunk);
        echo.render(&silence)?;
        let clean = echo.capture(&block, DELAY_HINT_MS)?;
        if linear {
            match echo.linear() {
                Some(block) => processed.extend_from_slice(block),
                None => {
                    linear_missing += 1;
                    processed.extend_from_slice(&[0; CAPTURE_SAMPLES]);
                }
            }
        } else {
            processed.extend_from_slice(&clean);
        }
    }
    processed.truncate(samples.len());

    write_replacing(output, &processed)?;
    Ok(Processed {
        blocks,
        samples: samples.len(),
        input_dbfs: rms_dbfs(&samples),
        output_dbfs: rms_dbfs(&processed),
        linear_missing,
    })
}

/// Written beside `output` first, then renamed over it: a failed run leaves
/// no partial file.
fn write_replacing(output: &Path, samples: &[i16]) -> Result<()> {
    let mut partial = PathBuf::from(output);
    partial.as_mut_os_string().push(".partial");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: CAPTURE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let written = (|| -> Result<()> {
        let mut writer = hound::WavWriter::create(&partial, spec)?;
        for &sample in samples {
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
        Ok(())
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    fs::rename(&partial, output)?;
    Ok(())
}

/// RMS in dBFS (full scale 32768); silence or nothing is negative infinity.
pub fn rms_dbfs(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return f64::NEG_INFINITY;
    }
    let power = samples
        .iter()
        .map(|&sample| (f64::from(sample) / 32768.0).powi(2))
        .sum::<f64>()
        / samples.len() as f64;
    10.0 * power.log10()
}
