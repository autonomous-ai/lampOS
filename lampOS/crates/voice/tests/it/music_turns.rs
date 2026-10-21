//! Music the detector takes for speech ("Not spoken to", #2602): made-up
//! music, synthesised here from tones and chords with nothing recorded from
//! people, is fed block by block through the voice's own detector, and each
//! turn it opens is counted. Speech must still open one. On lamp-c0a4 a 6 s
//! tune of two-tone notes over a 65 Hz hum opened an admitted turn at 914 ms.
use std::f32::consts::TAU;

use lamp_voice::activity::{Activity, ObservedAudio, SpeechProbability, TurnDetector};
use lamp_voice::not_speech::SCRAP_BLOCKS;

const RATE: f32 = 16_000.0;
const BLOCK: usize = 160;

/// "Stop. Please stop talking.", Piper's generated voice (`activity.rs`).
const SPEECH: &[u8] = include_bytes!("../../fixtures/piper-stop-16k.s16");

/// Each turn the detector opens in `samples`: how many blocks of it scored
/// as speech, so a turn too short to have been said (`not_speech`) shows.
pub fn turns(samples: &[f32]) -> Vec<usize> {
    let mut vad = SpeechProbability::default();
    let mut detector = TurnDetector::v1_baseline();
    let mut opened = Vec::new();
    let mut speech = 0;
    let lead = vec![0.0; BLOCK * 50];
    let tail = vec![0.0; BLOCK * 200];
    let all = lead
        .iter()
        .chain(samples)
        .chain(&tail)
        .copied()
        .collect::<Vec<_>>();
    for (n, chunk) in all.chunks_exact(BLOCK).enumerate() {
        let mut block = [0i16; BLOCK];
        for (dst, src) in block.iter_mut().zip(chunk) {
            *dst = (src.clamp(-1.0, 1.0) * 32_767.0) as i16;
        }
        let probability = vad.analyze(&block);
        let frame = ObservedAudio {
            sequence: n as u64 + 1,
            captured_at_us: (n as u64 + 1) * 10_000,
            samples: block,
        };
        match detector.push(frame, probability) {
            Activity::Start(prefix) => {
                speech = prefix.len();
                opened.push(0);
            }
            Activity::Continue(_) if probability >= 0.35 => speech += 1,
            Activity::End(_) => *opened.last_mut().unwrap() = speech,
            Activity::Fault(reason) => panic!("{reason}"),
            _ => {}
        }
    }
    opened
}

fn piper() -> Vec<f32> {
    SPEECH
        .chunks_exact(2)
        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32_768.0)
        .collect()
}

fn hz(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

/// A note of `seconds` at `f`, with `harmonics` overtones falling as 1/k,
/// an attack of 10 ms and `decay` per second (0 holds it), and `vibrato`
/// cents at 5.5 Hz.
fn note(f: f32, seconds: f32, harmonics: usize, decay: f32, vibrato: f32) -> Vec<f32> {
    let n = (seconds * RATE) as usize;
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE;
            let bend = 2f32.powf(vibrato / 1200.0 * (TAU * 5.5 * t).sin());
            phase += TAU * f * bend / RATE;
            let tone: f32 = (1..=harmonics)
                .map(|k| (phase * k as f32).sin() / k as f32)
                .sum();
            let envelope =
                (t / 0.01).min(1.0) * (-decay * t).exp() * ((n - i) as f32 / 160.0).min(1.0);
            tone * envelope
        })
        .collect()
}

fn add(into: &mut Vec<f32>, at: usize, part: &[f32], gain: f32) {
    if into.len() < at + part.len() {
        into.resize(at + part.len(), 0.0);
    }
    for (dst, src) in into[at..].iter_mut().zip(part) {
        *dst += src * gain;
    }
}

fn level(samples: &mut [f32], peak: f32) {
    let top = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    samples.iter_mut().for_each(|s| *s *= peak / top);
}

/// A plain generator, so the noise is the same each run.
fn noise(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
}

/// Made-up music, 6 s to 8 s each, named.
pub fn tunes() -> Vec<(&'static str, Vec<f32>)> {
    let s = RATE as usize;
    let mut out = Vec::new();

    // #2701's tune as Thirteen describes it: two-tone notes over a 65 Hz hum.
    let mut two_tone = note(65.0, 6.0, 3, 0.0, 0.0);
    for (k, m) in [
        72.0, 76.0, 79.0, 76.0, 74.0, 77.0, 81.0, 77.0, 72.0, 79.0, 76.0, 72.0,
    ]
    .iter()
    .enumerate()
    {
        add(
            &mut two_tone,
            k * s / 2,
            &note(hz(*m), 0.45, 1, 0.0, 0.0),
            0.5,
        );
        add(
            &mut two_tone,
            k * s / 2,
            &note(hz(*m - 12.0), 0.45, 1, 0.0, 0.0),
            0.4,
        );
    }
    out.push(("two-tone notes over a hum", two_tone));

    // A plucked arpeggio, falling away like a piano's.
    let mut arpeggio = Vec::new();
    for k in 0..32 {
        let m = [60.0, 64.0, 67.0, 72.0][k % 4] + [0.0, 5.0, 7.0, 0.0][k / 8];
        add(
            &mut arpeggio,
            k * s / 4,
            &note(hz(m), 0.9, 6, 4.0, 0.0),
            0.5,
        );
    }
    out.push(("a plucked arpeggio", arpeggio));

    // Held chords, a pad with slow vibrato.
    let mut pad = Vec::new();
    for (k, chord) in [
        [60.0, 64.0, 67.0],
        [57.0, 60.0, 64.0],
        [53.0, 57.0, 60.0],
        [55.0, 59.0, 62.0],
    ]
    .iter()
    .enumerate()
    {
        for m in chord {
            add(&mut pad, k * 2 * s, &note(hz(*m), 2.0, 4, 0.0, 12.0), 0.3);
        }
    }
    out.push(("held chords", pad));

    // A sung-like line: a bright tone in the voice's range with vibrato,
    // the nearest made-up music comes to a voice.
    let mut line = Vec::new();
    for (k, m) in [
        57.0, 59.0, 60.0, 62.0, 64.0, 62.0, 60.0, 59.0, 57.0, 60.0, 64.0, 57.0,
    ]
    .iter()
    .enumerate()
    {
        add(
            &mut line,
            k * s / 2,
            &note(hz(*m), 0.48, 12, 0.5, 30.0),
            0.5,
        );
    }
    out.push(("a bright line with vibrato", line));

    // A beat: a kick's falling sine and a hat's noise burst, with a bass.
    let mut beat = Vec::new();
    let mut seed = 7u32;
    for k in 0..16 {
        let kick = (0..s / 8)
            .map(|i| {
                let t = i as f32 / RATE;
                (TAU * (50.0 + 100.0 * (-30.0 * t).exp()) * t).sin() * (-12.0 * t).exp()
            })
            .collect::<Vec<_>>();
        let hat = (0..s / 25)
            .map(|i| noise(&mut seed) * (-(i as f32) / 120.0).exp())
            .collect::<Vec<_>>();
        add(&mut beat, k * s / 2, &kick, 0.8);
        add(&mut beat, k * s / 2 + s / 4, &hat, 0.3);
        add(
            &mut beat,
            k * s / 2,
            &note(hz([36.0, 36.0, 43.0, 41.0][k % 4]), 0.45, 3, 1.0, 0.0),
            0.4,
        );
    }
    out.push(("a beat with a bass", beat));

    // Everything at once: chords, the line and the beat.
    let mut band = out[2].1.clone();
    add(&mut band, 0, &out[3].1, 0.8);
    add(&mut band, 0, &out[4].1, 0.8);
    out.push(("a band: chords, line and beat", band));

    for (_, samples) in &mut out {
        level(samples, 0.5);
    }
    out
}

/// 16 kHz mono 16-bit WAV, to play a tune in a room or score it off the lamp.
fn wave(path: &std::path::Path, samples: &[f32]) {
    let data = samples
        .iter()
        .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes())
        .collect::<Vec<_>>();
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(16_000u32.to_le_bytes());
    out.extend(32_000u32.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(path, out).unwrap();
}

/// The count, printed: each tune at three levels, and the speech. With
/// `LAMP_MUSIC_TURNS_WAVS=DIR` each tune is written there as a WAV too.
#[test]
fn made_up_music_opens_turns_and_speech_still_does() {
    if let Some(dir) = std::env::var_os("LAMP_MUSIC_TURNS_WAVS") {
        for (k, (_, samples)) in tunes().iter().enumerate() {
            wave(
                &std::path::Path::new(&dir).join(format!("tune{k}.wav")),
                samples,
            );
        }
    }
    let speech = turns(&piper());
    assert_eq!(speech.len(), 1, "{speech:?}");
    assert!(speech[0] >= SCRAP_BLOCKS, "{speech:?}");
    println!("speech: {speech:?}");
    for (name, samples) in tunes() {
        for gain in [1.0, 0.3, 0.1] {
            let quieter = samples.iter().map(|s| s * gain).collect::<Vec<_>>();
            let opened = turns(&quieter);
            let said = opened.iter().filter(|&&n| n >= SCRAP_BLOCKS).count();
            println!(
                "{name} at {gain}: {} turns, {said} long enough to be said {opened:?}",
                opened.len()
            );
        }
    }
}
