//! How far apart the same voice and other voices are, as the voice's own
//! check prints them (#829, `docs/same-voice.md`): every clip of an index cut
//! into lines as the voice cuts them, each line printed as the worker asks
//! for its print, and the cosine of every pair of lines told as the same
//! voice or another, against the voice's two bars.
//!
//! `cargo run --release -p lamp-voice-eval --example same_voice_pairs --
//!  MODEL.onnx (INDEX.tsv | --bus RECORDING.jsonl --plays PLAYS.tsv
//!  [--offset-ms MS]) [--end-silence-ms 600] [--pairs]`
//!
//! `MODEL.onnx` is the voice's CAM++ (`LAMP_SAME_VOICE_MODEL`). The clips
//! come from one of two places:
//! - `INDEX.tsv`, `file<TAB>voice` a line (a path relative to the index's
//!   folder, then any label for whose voice it is), each a 16 kHz mono
//!   16-bit WAV;
//! - a microphone's recording, `lamp-bus record DIR mic2/audio`'s
//!   `mic2_audio.jsonl` (`--bus`), cut by when each line was played
//!   (`--plays`), by the player's wall clock: either
//!   `start_unix_ms<TAB>end_unix_ms<TAB>voice` a line, or the night kit's own
//!   round log (a `.json` whose `rows` each have `start` and `end` in Unix
//!   seconds and a `voice`). Each clip is the recording's blocks
//!   whose `at_unix_ms` falls from 300 ms before a play's start to 700 ms
//!   after its end, both moved by `--offset-ms` (the recording's clock minus
//!   the player's), and never past the play before's end or the play
//!   after's start. Blocks must be `lamp_msgs::Audio` at 16 kHz, one
//!   channel, `S16LE`; a lost block is left out, not filled.
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
/// Clips, each with its voice's label and its samples.
type Clips = Vec<(String, Vec<i16>)>;

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

/// Standard base64 with padding, as `lamp-bus record` writes `payload_b64`.
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let value = |byte: u8| match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let quartets = bytes.len() / 4;
    for (place, chunk) in bytes.chunks(4).enumerate() {
        let pad = chunk.iter().rev().take_while(|byte| **byte == b'=').count();
        // Padding ends the text: a padded quartet before the last one is
        // not base64 (`TQ==TQ==`).
        if pad > 2 || (pad > 0 && place + 1 != quartets) {
            return None;
        }
        let mut word = 0u32;
        for &byte in &chunk[..4 - pad] {
            word = word << 6 | u32::from(value(byte)?);
        }
        word <<= 6 * pad as u32;
        let [_, a, b, c] = word.to_be_bytes();
        out.extend_from_slice(&[a, b, c][..3 - pad]);
    }
    Some(out)
}

/// The clips of `INDEX.tsv`, each with its voice's label.
fn clips_from_index(index_path: &Path) -> Result<Clips, Error> {
    let folder = index_path.parent().unwrap_or(Path::new("."));
    let mut clips = Vec::new();
    for row in std::fs::read_to_string(index_path)?
        .lines()
        .filter(|row| !row.trim().is_empty() && !row.starts_with('#'))
    {
        let (file, label) = row
            .split_once('\t')
            .ok_or_else(|| format!("not file<TAB>voice: {row}"))?;
        let samples = read_wav(&folder.join(file))?
            .iter()
            .map(|value| (value * 32_768.0).clamp(-32_768.0, 32_767.0) as i16)
            .collect();
        clips.push((label.trim().to_owned(), samples));
    }
    Ok(clips)
}

/// One `lamp_msgs::Audio` payload's count and samples: 16 kHz, one channel,
/// `S16LE`, whole samples only.
fn samples_of(payload: &[u8]) -> Result<(u64, Vec<i16>), Error> {
    let (audio, pcm) = lamp_msgs::Audio::split(payload)?;
    if audio.sample_rate != 16_000
        || audio.channels != 1
        || audio.sample_format != lamp_msgs::Audio::S16LE
    {
        return Err(format!(
            "a block at {} Hz, {} channels, {}: not 16 kHz mono S16LE",
            audio.sample_rate, audio.channels, audio.sample_format
        )
        .into());
    }
    // `Audio::split` reads only the header: an odd byte left over is a
    // broken block, not a sample to drop.
    if !pcm.len().is_multiple_of(2) {
        return Err(format!("a block of {} bytes of S16LE: not whole samples", pcm.len()).into());
    }
    let samples = pcm
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Ok((audio.sequence, samples))
}

/// The clips a microphone's recording holds for each play of `PLAYS.tsv`,
/// each with its voice's label; and how many blocks were lost inside the
/// clips (a step of more than one in `lamp_msgs::Audio::sequence` between
/// two blocks of one clip). A clip runs from 300 ms before its play's start
/// (`afplay`'s start is taken before its sound, tens of ms early) to 700 ms
/// after its end, but never past the play before's end or the play after's
/// start, so a clip never holds another play's line (Fourteen's P2 on
/// #3125).
fn clips_from_bus(recording: &Path, plays: &Path, offset_ms: i64) -> Result<(Clips, u64), Error> {
    // Each block: when it was sent, its count, its samples.
    let mut blocks: Vec<(i64, u64, Vec<i16>)> = Vec::new();
    for line in std::fs::read_to_string(recording)?.lines() {
        let record: serde_json::Value = serde_json::from_str(line)?;
        // A topic's declaration has no header: it is not sound.
        let (Some(header), Some(payload)) = (
            record.get("header"),
            record.get("payload_b64").and_then(|value| value.as_str()),
        ) else {
            continue;
        };
        let at = header
            .get("at_unix_ms")
            .and_then(serde_json::Value::as_i64)
            .ok_or("a block without at_unix_ms")?;
        let (sequence, samples) =
            samples_of(&unbase64(payload).ok_or("payload_b64 is not base64")?)?;
        blocks.push((at, sequence, samples));
    }
    blocks.sort_by_key(|(at, _, _)| *at);
    let mut plays: Vec<(i64, i64, String)> = plays_of(plays)?
        .into_iter()
        .map(|(start, end, label)| (start + offset_ms, end + offset_ms, label))
        .collect();
    plays.sort_by_key(|(start, _, _)| *start);
    let mut clips = Vec::new();
    let mut lost = 0u64;
    for (index, (start, end, label)) in plays.iter().enumerate() {
        let before = index.checked_sub(1).map(|before| plays[before].1);
        let after = plays.get(index + 1).map(|(next, _, _)| *next);
        let from = (start - 300).max(before.unwrap_or(i64::MIN));
        let to = (end + 700).min(after.unwrap_or(i64::MAX));
        let mut last: Option<u64> = None;
        let mut samples: Vec<i16> = Vec::new();
        for (_, sequence, block) in blocks.iter().filter(|(at, _, _)| (from..=to).contains(at)) {
            if let Some(last) = last
                && *sequence > last + 1
            {
                lost += sequence - last - 1;
            }
            last = Some(*sequence);
            samples.extend_from_slice(block);
        }
        if samples.is_empty() {
            return Err(format!("no sound recorded for the play {start}-{end} {label}").into());
        }
        clips.push((label.clone(), samples));
    }
    Ok((clips, lost))
}

/// Each play's start and end in Unix ms and its voice: from a TSV of
/// `start_unix_ms<TAB>end_unix_ms<TAB>voice`, or from the night kit's round
/// log (`.json`, `rows` with `start` and `end` in Unix seconds and `voice`).
fn plays_of(plays: &Path) -> Result<Vec<(i64, i64, String)>, Error> {
    let text = std::fs::read_to_string(plays)?;
    if plays
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        let log: serde_json::Value = serde_json::from_str(&text)?;
        let rows = log
            .get("rows")
            .and_then(serde_json::Value::as_array)
            .ok_or("a round log without rows")?;
        return rows
            .iter()
            .map(|row| {
                let seconds = |key: &str| {
                    row.get(key)
                        .and_then(serde_json::Value::as_f64)
                        .map(|value| (value * 1000.0).round() as i64)
                        .ok_or_else(|| format!("a row without {key} in seconds: {row}"))
                };
                let voice = row
                    .get("voice")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| format!("a row without voice: {row}"))?;
                Ok((seconds("start")?, seconds("end")?, voice.to_owned()))
            })
            .collect();
    }
    text.lines()
        .filter(|row| !row.trim().is_empty() && !row.starts_with('#'))
        .map(|row| {
            let mut fields = row.split('\t');
            let (Some(start), Some(end), Some(label)) =
                (fields.next(), fields.next(), fields.next())
            else {
                return Err(format!("not start_unix_ms<TAB>end_unix_ms<TAB>voice: {row}").into());
            };
            Ok((
                start.trim().parse()?,
                end.trim().parse()?,
                label.trim().to_owned(),
            ))
        })
        .collect()
}

fn main() -> Result<(), Error> {
    const USAGE: &str = "same_voice_pairs MODEL.onnx (INDEX.tsv | --bus RECORDING.jsonl --plays PLAYS.tsv [--offset-ms MS]) [--end-silence-ms MS] [--pairs]";
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [model_path, rest @ ..] = &arguments[..] else {
        return Err(USAGE.into());
    };
    let (index_path, rest) = match rest {
        [first, rest @ ..] if !first.starts_with("--") => (Some(Path::new(first)), rest),
        _ => (None, rest),
    };
    let mut end_silence_ms = 600u64;
    let mut each_pair = false;
    let (mut bus, mut plays, mut offset_ms) = (None, None, 0i64);
    let mut flags = rest.iter();
    while let Some(flag) = flags.next() {
        match flag.as_str() {
            "--bus" => bus = Some(Path::new(flags.next().ok_or("--bus needs a file")?)),
            "--plays" => plays = Some(Path::new(flags.next().ok_or("--plays needs a file")?)),
            "--offset-ms" => {
                offset_ms = flags.next().ok_or("--offset-ms needs a value")?.parse()?
            }
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
    let (clips, lost) = match (index_path, bus, plays) {
        (Some(index), None, None) => (clips_from_index(index)?, 0),
        (None, Some(bus), Some(plays)) => clips_from_bus(bus, plays, offset_ms)?,
        _ => return Err(format!("an index, or --bus with --plays: {USAGE}").into()),
    };
    let voice = Voice::load(Path::new(model_path))?;
    let end_silence_blocks = u16::try_from(end_silence_ms / 10)?;
    let mut lines = Vec::new();
    let mut unprinted = 0usize;
    // Clips in which no line was heard at all: with `--bus`, the first sign
    // that `--offset-ms` is wrong and a play's times miss its sound.
    let mut silent = 0usize;
    for (clip, (label, samples)) in clips.iter().enumerate() {
        let heard_lines = lines_of(samples, end_silence_blocks)?;
        if heard_lines.is_empty() {
            silent += 1;
        }
        for (piece, (start_ms, heard)) in heard_lines.into_iter().enumerate() {
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
        "clips {}, with no line heard {silent}, lines printed {} ({} voices), too short to print {unprinted}; end silence {end_silence_ms} ms; blocks lost inside the clips {lost}",
        clips.len(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_read_with_its_padding_only_at_the_end() {
        assert_eq!(unbase64("TWFu").as_deref(), Some(&b"Man"[..]));
        assert_eq!(unbase64("TWE=").as_deref(), Some(&b"Ma"[..]));
        assert_eq!(unbase64("TWFuTQ==").as_deref(), Some(&b"ManM"[..]));
        assert_eq!(unbase64(""), Some(Vec::new()));
        for broken in ["TQ==TQ==", "TQ=", "T===", "T=Q=", "TQ==TWFu", "TW!u"] {
            assert_eq!(unbase64(broken), None, "{broken}");
        }
    }

    #[test]
    fn a_block_gives_whole_samples_or_an_error() {
        let header = lamp_msgs::Audio {
            sample_rate: 16_000,
            channels: 1,
            sample_format: lamp_msgs::Audio::S16LE.into(),
            sequence: 7,
        };
        let whole = header.block(&[0x01, 0x00, 0xff, 0xff]).unwrap();
        assert_eq!(samples_of(&whole).unwrap(), (7, vec![1, -1]));
        // Any even length is a block: mic2 sends 640 bytes, a recording
        // need not.
        let short = header.block(&[0x02, 0x00]).unwrap();
        assert_eq!(samples_of(&short).unwrap(), (7, vec![2]));
        let odd = header.block(&[0x01, 0x00, 0xff]).unwrap();
        assert!(samples_of(&odd).is_err());
        let stereo = lamp_msgs::Audio {
            channels: 2,
            ..header
        }
        .block(&[0, 0, 0, 0])
        .unwrap();
        assert!(samples_of(&stereo).is_err());
    }
}
