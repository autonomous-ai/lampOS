//! An isolated, caller-conditioned Personal VAD trial on 16 kHz PCM.
//! The pretrained ST model emits target, silence and other every 10 ms.
//! Only weights survive a child request. The simple voice policy never reads a vote.
mod features;
mod network;

use features::{Frontend, HOP, Moments, WINDOW};
use network::Weights;
use std::{
    collections::VecDeque,
    io::{self, BufRead, Read, Write},
    path::Path,
};

pub const MAX_REFERENCE: usize = 48_000;
pub const MAX_FOLLOWUP: usize = 192_000;
const CONTEXT: usize = 14;
const MIN_SPEECH_FRAMES: usize = 8;

/// The model's three posteriors in publisher order: target, silence, other.
pub type Posterior = [f32; 3];
pub struct Model {
    frontend: Frontend,
    weights: Weights,
}

#[derive(Debug, PartialEq)]
pub struct Decision {
    pub class: &'static str,
    /// Fraction of predicted speech frames assigned to the target.
    pub target_fraction: Option<f32>,
}

impl Model {
    pub fn load(directory: &Path) -> Result<Self, String> {
        Ok(Self {
            frontend: Frontend::new(),
            weights: Weights::load(directory)?,
        })
    }

    /// A silent or incomplete wake cannot supply an opening voice.
    pub fn enroll(&self, wake: &[f32]) -> Option<[f32; 256]> {
        if !usable_pcm(wake, MAX_REFERENCE) {
            return None;
        }
        let mut sum = Moments::zero();
        let mut previous = None;
        for mel in self.frontend.frames(wake) {
            let current: [f32; 40] = mel[..40].try_into().unwrap();
            let frame = Moments::frame(
                &mel,
                previous.as_ref().unwrap_or(&current),
                &self.weights.mel_mean,
                self.weights.threshold,
            );
            sum.accumulate(&frame, 1.0);
            previous = Some(current);
        }
        Some(self.weights.project(sum.summary()))
    }

    /// Every output uses only its 25 ms analysis window and earlier samples.
    pub fn posteriors(&self, caller: &[f32; 256], line: &[f32]) -> Vec<Posterior> {
        if !usable_pcm(line, MAX_FOLLOWUP) {
            return Vec::new();
        }
        let mut history = VecDeque::with_capacity(CONTEXT);
        let mut sum = Moments::zero();
        let mut previous = None;
        let mut state = Default::default();
        self.frontend
            .frames(line)
            .map(|mel| {
                let current: [f32; 40] = mel[..40].try_into().unwrap();
                let frame = Moments::frame(
                    &mel,
                    previous.as_ref().unwrap_or(&current),
                    &self.weights.mel_mean,
                    self.weights.threshold,
                );
                previous = Some(current);
                if history.len() == CONTEXT {
                    sum.accumulate(&history.pop_front().unwrap(), -1.0);
                }
                sum.accumulate(&frame, 1.0);
                history.push_back(frame);
                let speaker = self.weights.project(sum.summary());
                let cosine = speaker
                    .iter()
                    .zip(caller)
                    .map(|(a, b)| a * b)
                    .sum::<f32>()
                    .clamp(-1.0, 1.0);
                self.weights.predict(&mel, cosine, &mut state)
            })
            .collect()
    }
}

fn usable_pcm(pcm: &[f32], limit: usize) -> bool {
    // Reject invalid, clipped-format, silent and near-silent captures. A trace
    // needs at least 80 ms of audible material before it can represent a voice.
    pcm.len() <= limit
        && pcm.iter().all(|x| x.is_finite() && x.abs() <= 1.0)
        && pcm
            .windows(WINDOW)
            .step_by(HOP)
            .filter(|w| w.iter().map(|x| x * x).sum::<f32>() > WINDOW as f32 * 0.0015f32.powi(2))
            .take(MIN_SPEECH_FRAMES)
            .count()
            == MIN_SPEECH_FRAMES
}

pub fn decide(frames: &[Posterior]) -> Decision {
    let mut target = 0;
    let mut other = 0;
    for &[t, silence, o] in frames {
        if t > silence && t > o {
            target += 1;
        } else if o > silence && o > t {
            other += 1;
        }
    }
    let speech = target + other;
    if speech < MIN_SPEECH_FRAMES {
        return Decision {
            class: "unknown",
            target_fraction: None,
        };
    }
    let share = target as f32 / speech as f32;
    let class = if target * 5 >= speech * 3 {
        "same"
    } else if target * 5 <= speech * 2 {
        "different"
    } else {
        "unknown"
    };
    Decision {
        class,
        target_fraction: Some(share),
    }
}

fn malformed() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid frame pair header")
}

fn read_pcm(input: &mut impl Read, count: usize) -> io::Result<Vec<f32>> {
    let mut bytes = vec![0; count * 2];
    input.read_exact(&mut bytes)?;
    Ok(bytes
        .chunks_exact(2)
        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
        .collect())
}

pub fn serve(
    input: &mut impl BufRead,
    output: &mut impl Write,
    mut classify: impl FnMut(&[f32], &[f32]) -> Decision,
) -> io::Result<()> {
    writeln!(output, "ready")?;
    output.flush()?;
    let mut header = String::new();
    loop {
        header.clear();
        if input.take(97).read_line(&mut header)? == 0 {
            return Ok(());
        }
        if header.len() > 96 || !header.ends_with('\n') {
            return Err(malformed());
        }
        let mut fields = header.split_ascii_whitespace();
        if fields.next() != Some("pair") {
            return Err(malformed());
        }
        for _ in 0..2 {
            fields
                .next()
                .ok_or_else(malformed)?
                .parse::<u64>()
                .map_err(|_| malformed())?;
        }
        let mut count = |max: usize| -> io::Result<usize> {
            fields
                .next()
                .and_then(|s| s.parse().ok())
                .filter(|&n| n <= max)
                .ok_or_else(malformed)
        };
        let reference_len = count(MAX_REFERENCE)?;
        let line_len = count(MAX_FOLLOWUP)?;
        if fields.next().is_some() {
            return Err(malformed());
        }
        let reference = read_pcm(input, reference_len)?;
        let line = read_pcm(input, line_len)?;
        let result = classify(&reference, &line);
        match (result.class, result.target_fraction) {
            ("same" | "different", Some(score)) => writeln!(output, "{} {score:.7}", result.class)?,
            _ => writeln!(output, "unknown")?,
        }
        output.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_and_short_clips_offer_no_speaker_and_mixed_frames_abstain() {
        assert!(!usable_pcm(&vec![0.0; 16_000], MAX_REFERENCE));
        assert!(!usable_pcm(&[0.5; WINDOW], MAX_REFERENCE));
        assert_eq!(decide(&[[0.0, 1.0, 0.0]; 20]).target_fraction, None);
        let a = [0.8, 0.1, 0.1];
        let b = [0.1, 0.1, 0.8];
        for (n, class) in [
            (3, "different"),
            (4, "different"),
            (5, "unknown"),
            (6, "same"),
            (7, "same"),
        ] {
            let mut trace = vec![b; 10];
            trace[..n].fill(a);
            assert_eq!(decide(&trace).class, class);
        }
        assert_eq!(decide(&[a; 7]).class, "unknown");
    }

    /// Independent TensorFlow outputs for deterministic PCM, generated with the
    /// upstream ST checkpoint. This test needs the optional local trial weights.
    #[test]
    #[ignore = "set LAMP_CALLER_FRAME_MODEL to the prepared ST directory"]
    fn published_model_matches_reference_posteriors_and_uses_no_future_frames() {
        let path = std::env::var_os("LAMP_CALLER_FRAME_MODEL").unwrap();
        let model = Model::load(Path::new(&path)).unwrap();
        let waveform = |period: i32, size: i32| -> Vec<f32> {
            (0..size)
                .map(|i| {
                    let sample =
                        ((i % period) * 170 - 64 * 85 + (i % 31) * 40 - 620) * ((i / 640) % 3 + 1);
                    sample as f32 / 65536.0
                })
                .collect()
        };
        let caller = model.enroll(&waveform(67, 6400)).unwrap();
        let line = waveform(79, 8000);
        let full = model.posteriors(&caller, &line);
        assert_eq!(full.len(), 48);
        let prefix = model.posteriors(&caller, &line[..4000]);
        assert_eq!(prefix, full[..prefix.len()]);
        for (index, official) in [
            (0, [0.3400134, 0.367_252, 0.2927345]),
            (13, [0.9128506, 0.0208415, 0.0663078]),
            (34, [0.9743568, 0.0079733, 0.0176699]),
            (47, [0.9737777, 0.0091660, 0.0170562]),
        ] {
            for (a, b) in full[index].iter().zip(official) {
                assert!((a - b).abs() < 2e-4, "frame {index}: {a} != {b}");
            }
        }
    }

    #[test]
    fn protocol_binds_each_pair_and_caps_input_before_allocation() {
        let mut input = b"pair 8 9 1 1\n".to_vec();
        input.extend([0, 64, 0, 192]);
        input.extend(b"pair 11 12 0 0\n");
        let mut output = Vec::new();
        serve(&mut &input[..], &mut output, |reference, line| {
            if reference == [0.5] && line == [-0.5] {
                Decision {
                    class: "different",
                    target_fraction: Some(0.0),
                }
            } else {
                decide(&[])
            }
        })
        .unwrap();
        assert_eq!(output, b"ready\ndifferent 0.0000000\nunknown\n");
        for invalid in [
            "pair 1 2 48001 1\n",
            "pair 1 2 1 192001\n",
            "pair 0 1 4 1\n",
            "x",
        ] {
            assert!(
                serve(
                    &mut invalid.as_bytes(),
                    &mut Vec::new(),
                    |_, _| unreachable!()
                )
                .is_err()
            );
        }
    }
}
