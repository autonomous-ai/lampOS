//! How far apart the same voice and other voices are, as the voice's own
//! check prints them (#829, `docs/same-voice.md`): every clip of an index cut
//! into lines as the voice cuts them, each line printed as the worker asks
//! for its print, and the cosine of every pair of lines told as the same
//! voice or another, against the voice's two bars.
//!
//! `cargo run --release -p lamp-voice-eval --example same_voice_pairs --
//!  MODEL.onnx INDEX.tsv [--end-silence-ms 600] [--pairs]`
//!
//! `INDEX.tsv` is `file<TAB>voice` a line (a path relative to the index's
//! folder, then any label for whose voice it is), each a 16 kHz mono 16-bit
//! WAV. `MODEL.onnx` is the voice's CAM++ (`LAMP_SAME_VOICE_MODEL`).
//!
//! - **Lines:** each clip, with 600 ms of lead-in and 2.5 s of tail at a
//!   -63 dBFS noise floor (as `turn_end_replay` plays clips), goes through the
//!   voice's `TurnDetector` with its silence window alone (`--end-silence-ms`,
//!   the model's verdicts left out), so a clip with a long pause is several
//!   lines, as a cut line is on a lamp.
//! - **Prints:** each line's audio from its start, its pre-roll included,
//!   as the worker hears it (`same_voice::Voices::heard`): the first
//!   `same_voice::ASKED_AT` samples, or the whole line when it ends sooner and
//!   has at least `same_voice::LEAST_SAMPLES`. A shorter line has no print,
//!   and is counted.
//! - **Pairs:** every two printed lines. *Same voice, one clip* are the pieces
//!   of one cut line (the join's question, `same_voice::SAME_FROM`); *same
//!   voice* are two lines of one label; *another voice* two labels
//!   (`same_voice::DIFFERENT_BELOW`).
//!
//! Prints each group's count and spread, how many fall each side of the two
//! bars, and the bar that splits the same voice from other voices with the
//! fewest pairs on the wrong side, searched over the cosine's whole range.
//! Numbers only: no words, no audio, no file names. With `--pairs`, each
//! pair too, as `a<TAB>b<TAB>group<TAB>cosine`, a line's id being its clip's
//! place among the index's clips (from 1), its piece and its start
//! (`3.1@2410ms`).
use lamp_audio::{CAPTURE_SAMPLES, CaptureBlock};
use lamp_same_voice::{Voice, cosine, read_wav};
use lamp_voice::activity::{Activity, ObservedAudio, SpeechProbability, TurnDetector};
use lamp_voice::same_voice::{ASKED_AT, DIFFERENT_BELOW, LEAST_SAMPLES, SAME_FROM};
use std::path::Path;

type Error = Box<dyn std::error::Error>;

const BLOCK_US: u64 = 10_000;
const LEAD_BLOCKS: usize = 60;
const TAIL_BLOCKS: usize = 250;
/// -63 dBFS as the RMS of a uniform noise, as `turn_end_replay` adds it.
const NOISE_PEAK: f64 = 40.0;

/// One printed line: its clip, its voice's label, its place in the clip.
/// Its id is numbers only (`clip.piece`, the clip's place among the
/// index's clips from 1), so `--pairs` prints no file name.
struct Line {
    id: String,
    clip: usize,
    voice: String,
    print: Vec<f32>,
}

/// The lines the voice would hear in `samples`, each as the worker gathers
/// it for a print: from the line's first sample, pre-roll included, at most
/// `ASKED_AT`. With the start of each, in ms of the clip's own audio.
fn lines_of(samples: &[i16], end_silence_blocks: u16) -> Result<Vec<(i64, Vec<i16>)>, Error> {
    let mut detector = TurnDetector::default().with_end_silence(end_silence_blocks)?;
    let mut vad = SpeechProbability::default();
    let mut noise = 0x2545_f491_4f6c_dd1du64;
    let mut audio = vec![0i16; LEAD_BLOCKS * CAPTURE_SAMPLES];
    audio.extend_from_slice(samples);
    audio.resize(
        audio.len().div_ceil(CAPTURE_SAMPLES) * CAPTURE_SAMPLES + TAIL_BLOCKS * CAPTURE_SAMPLES,
        0,
    );
    let lead_ms = (LEAD_BLOCKS as i64) * (BLOCK_US as i64) / 1000;
    let mut lines = Vec::new();
    let mut open: Option<(i64, Vec<i16>)> = None;
    for (index, chunk) in audio.chunks_exact(CAPTURE_SAMPLES).enumerate() {
        let mut block: CaptureBlock = [0; CAPTURE_SAMPLES];
        for (out, sample) in block.iter_mut().zip(chunk) {
            noise ^= noise << 13;
            noise ^= noise >> 7;
            noise ^= noise << 17;
            let floor = ((noise >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * NOISE_PEAK;
            *out = (f64::from(*sample) + floor).clamp(-32768.0, 32767.0) as i16;
        }
        let probability = vad.analyze(&block);
        let frame = ObservedAudio {
            sequence: index as u64 + 1,
            captured_at_us: index as u64 * BLOCK_US,
            samples: block,
        };
        let keep = |line: &mut Vec<i16>, frame: &ObservedAudio| {
            let room = ASKED_AT.saturating_sub(line.len());
            line.extend_from_slice(&frame.samples[..frame.samples.len().min(room)]);
        };
        match detector.push(frame, probability) {
            Activity::Start(prefix) => {
                let start_ms = prefix.first().map_or(index as i64 * 10, |first| {
                    (first.captured_at_us / 1000) as i64
                });
                let mut line = Vec::with_capacity(ASKED_AT);
                for frame in &prefix {
                    keep(&mut line, frame);
                }
                open = Some((start_ms - lead_ms, line));
            }
            Activity::Continue(frame) => {
                if let Some((_, line)) = open.as_mut() {
                    keep(line, &frame);
                }
            }
            Activity::End(frame) => {
                if let Some((start, mut line)) = open.take() {
                    keep(&mut line, &frame);
                    lines.push((start, line));
                }
            }
            Activity::Fault(why) => return Err(why.into()),
            Activity::Quiet => {}
        }
    }
    Ok(lines)
}

/// The count, spread and the median of `values`, sorted in place.
fn spread(values: &mut [f32]) -> String {
    if values.is_empty() {
        return "n 0".into();
    }
    values.sort_by(f32::total_cmp);
    let at = |share: f32| values[((values.len() - 1) as f32 * share).round() as usize];
    format!(
        "n {}  min {:.2}  p05 {:.2}  median {:.2}  p95 {:.2}  max {:.2}",
        values.len(),
        values[0],
        at(0.05),
        at(0.5),
        at(0.95),
        values[values.len() - 1]
    )
}

/// The bar, in steps of 0.01 over the cosine's whole range (-1 to 1), under
/// which fewest same-voice pairs fall plus at or over which fewest
/// other-voice pairs rise; with those two counts.
fn best_bar(same: &[f32], other: &[f32]) -> Option<(f32, usize, usize)> {
    if same.is_empty() || other.is_empty() {
        return None;
    }
    (-100..=100)
        .map(|step| {
            let bar = step as f32 / 100.0;
            let under = same.iter().filter(|value| **value < bar).count();
            let over = other.iter().filter(|value| **value >= bar).count();
            (bar, under, over)
        })
        .min_by_key(|(_, under, over)| under + over)
}

fn main() -> Result<(), Error> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [model_path, index_path, rest @ ..] = &arguments[..] else {
        return Err("same_voice_pairs MODEL.onnx INDEX.tsv [--end-silence-ms MS] [--pairs]".into());
    };
    let mut end_silence_ms = 600u64;
    let mut each_pair = false;
    let mut flags = rest.iter();
    while let Some(flag) = flags.next() {
        match flag.as_str() {
            "--end-silence-ms" => {
                end_silence_ms = flags
                    .next()
                    .ok_or("--end-silence-ms needs a value")?
                    .parse()?;
                // The detector counts 10 ms blocks: a window that isn't a
                // whole number of them would run as a shorter one.
                if end_silence_ms == 0 || !end_silence_ms.is_multiple_of(10) {
                    return Err(format!(
                        "--end-silence-ms {end_silence_ms}: a whole number of 10 ms blocks, over 0"
                    )
                    .into());
                }
            }
            "--pairs" => each_pair = true,
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    let index_path = Path::new(index_path);
    let folder = index_path.parent().unwrap_or(Path::new("."));
    let index = std::fs::read_to_string(index_path)?;
    let voice = Voice::load(Path::new(model_path))?;
    let end_silence_blocks = u16::try_from(end_silence_ms / 10)?;
    let mut lines = Vec::new();
    let mut unprinted = 0usize;
    for (clip, row) in index
        .lines()
        .filter(|row| !row.trim().is_empty() && !row.starts_with('#'))
        .enumerate()
    {
        let (file, label) = row
            .split_once('\t')
            .ok_or_else(|| format!("not file<TAB>voice: {row}"))?;
        let samples: Vec<i16> = read_wav(&folder.join(file))?
            .iter()
            .map(|value| (value * 32_768.0).clamp(-32_768.0, 32_767.0) as i16)
            .collect();
        for (piece, (start_ms, heard)) in lines_of(&samples, end_silence_blocks)?
            .into_iter()
            .enumerate()
        {
            if heard.len() < LEAST_SAMPLES {
                unprinted += 1;
                continue;
            }
            let samples: Vec<f32> = heard
                .iter()
                .map(|value| f32::from(*value) / 32_768.0)
                .collect();
            lines.push(Line {
                id: format!("{}.{piece}@{start_ms}ms", clip + 1),
                clip,
                voice: label.trim().to_owned(),
                print: voice.embedding(&samples)?,
            });
        }
    }
    let (mut one_clip, mut same, mut other) = (Vec::new(), Vec::new(), Vec::new());
    for (i, a) in lines.iter().enumerate() {
        for b in &lines[i + 1..] {
            let value = cosine(&a.print, &b.print)
                .ok_or_else(|| format!("{} and {}: no cosine", a.id, b.id))?;
            let group = if a.clip == b.clip {
                one_clip.push(value);
                "same voice, one clip"
            } else if a.voice == b.voice {
                same.push(value);
                "same voice"
            } else {
                other.push(value);
                "another voice"
            };
            if each_pair {
                println!("{}\t{}\t{group}\t{value:.4}", a.id, b.id);
            }
        }
    }
    let voices: std::collections::BTreeSet<&str> =
        lines.iter().map(|line| line.voice.as_str()).collect();
    println!(
        "lines printed {} ({} voices), too short to print {unprinted}; end silence {end_silence_ms} ms",
        lines.len(),
        voices.len()
    );
    let same_all: Vec<f32> = same.iter().chain(&one_clip).copied().collect();
    println!(
        "same voice, one clip (pieces of one line): {}",
        spread(&mut one_clip)
    );
    println!(
        "same voice, two clips:                     {}",
        spread(&mut same)
    );
    println!(
        "another voice:                             {}",
        spread(&mut other)
    );
    let under = |values: &[f32], bar: f32| values.iter().filter(|value| **value < bar).count();
    println!(
        "DIFFERENT_BELOW {DIFFERENT_BELOW:.2}: same voice under it {} of {}, another voice at or over it {} of {}",
        under(&same_all, DIFFERENT_BELOW),
        same_all.len(),
        other.len() - under(&other, DIFFERENT_BELOW),
        other.len()
    );
    println!(
        "SAME_FROM {SAME_FROM:.2}: pieces of one line reaching it {} of {}, another voice reaching it {} of {}",
        one_clip.len() - under(&one_clip, SAME_FROM),
        one_clip.len(),
        other.len() - under(&other, SAME_FROM),
        other.len()
    );
    match best_bar(&same_all, &other) {
        Some((bar, wrong_same, wrong_other)) => println!(
            "fewest on the wrong side: a bar of {bar:.2}, same voice under it {wrong_same}, another voice at or over it {wrong_other}"
        ),
        None => println!("fewest on the wrong side: needs pairs of both kinds"),
    }
    Ok(())
}
