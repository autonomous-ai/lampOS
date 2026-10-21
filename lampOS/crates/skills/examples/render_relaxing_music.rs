// SPDX-License-Identifier: Apache-2.0
//! Host-only renderer for the original relaxing-music example.
//! Run from the lampOS root. No runtime service, device or network access.
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

include!("../../../examples/relaxing-music/score.rs");

const RATE: u32 = 24_000;
const Q15: i64 = 32_768;
const PEAK: i64 = 900;
const WINDOW: usize = 240;
const DESTINATION: &str = "artifacts/skills/relaxing-music/staged/relaxing-music";
const TEXT_FILES: [(&str, &[u8]); 6] = [
    (
        "skill.toml",
        include_bytes!("../../../examples/relaxing-music/skill.toml"),
    ),
    (
        "SKILL.md",
        include_bytes!("../../../examples/relaxing-music/SKILL.md"),
    ),
    (
        "README.md",
        include_bytes!("../../../examples/relaxing-music/README.md"),
    ),
    (
        "PROVENANCE.md",
        include_bytes!("../../../examples/relaxing-music/PROVENANCE.md"),
    ),
    (
        "score.rs",
        include_bytes!("../../../examples/relaxing-music/score.rs"),
    ),
    (
        "LICENSE",
        include_bytes!("../../../examples/relaxing-music/LICENSE"),
    ),
];

#[derive(Clone, Copy)]
enum Timbre {
    Pad,
    Upper,
}

#[derive(Clone, Copy)]
struct Note {
    at_ms: u32,
    duration_ms: u32,
    milli_hz: u32,
    level: i64,
    attack_ms: u32,
    release_ms: u32,
    timbre: Timbre,
}

fn frames(ms: u32) -> usize {
    (u64::from(ms) * u64::from(RATE) / 1_000) as usize
}

fn notes() -> Vec<Note> {
    let mut notes = Vec::with_capacity(48);
    for (bar, chord) in PAD_CHORDS.iter().enumerate() {
        let at_ms = bar as u32 * BAR_MS;
        for (voice, &milli_hz) in chord.iter().enumerate() {
            notes.push(Note {
                at_ms,
                duration_ms: (BAR_MS + 1_000).min(DURATION_MS - at_ms),
                milli_hz,
                level: if voice == 0 { 220 } else { 150 },
                attack_ms: 1_200,
                release_ms: 2_000,
                timbre: Timbre::Pad,
            });
        }
    }
    for (at_ms, milli_hz) in UPPER_NOTES {
        notes.push(Note {
            at_ms,
            duration_ms: 2_400,
            milli_hz,
            level: 120,
            attack_ms: 180,
            release_ms: 2_200,
            timbre: Timbre::Upper,
        });
    }
    notes
}

/// One cycle is a wrapping u32. A parabolic sine approximation, then a
/// fixed integer correction: no platform libm or floating-point rounding.
fn sine(phase: u32) -> i64 {
    let half = phase >> 16;
    let x = i64::from(half & 0x7fff);
    let y = 4 * x * (Q15 - x) / Q15;
    let y = y * (775 * Q15 + 225 * y) / (1_000 * Q15);
    if half & 0x8000 == 0 { y } else { -y }
}

fn envelope(frame: usize, total: usize, attack: usize, release: usize) -> i64 {
    let remaining = total - 1 - frame;
    let in_gain = (frame as i64 * Q15 / attack as i64).min(Q15);
    let out_gain = (remaining as i64 * Q15 / release as i64).min(Q15);
    in_gain.min(out_gain)
}

fn render() -> Vec<i16> {
    let mut mix = vec![0i64; frames(DURATION_MS)];
    for note in notes() {
        let start = frames(note.at_ms);
        let total = frames(note.duration_ms);
        let step =
            ((u64::from(note.milli_hz) << 32) + u64::from(RATE) * 500) / (u64::from(RATE) * 1_000);
        let mut phase = 0u32;
        for (frame, sample) in mix[start..start + total].iter_mut().enumerate() {
            let wave = match note.timbre {
                Timbre::Pad => (3 * sine(phase) + sine(phase.wrapping_mul(2))) / 4,
                Timbre::Upper => (7 * sine(phase) + sine(phase.wrapping_mul(3))) / 8,
            };
            let gain = envelope(
                frame,
                total,
                frames(note.attack_ms),
                frames(note.release_ms),
            );
            *sample += wave * note.level * gain / (Q15 * Q15);
            phase = phase.wrapping_add(step as u32);
        }
    }
    let total = mix.len();
    for (frame, sample) in mix.iter_mut().enumerate() {
        *sample = *sample * envelope(frame, total, frames(2_000), frames(3_000)) / Q15;
    }
    // One gain for the whole piece. Scaling never clips or pumps a window;
    // a peak of 900 also bounds RMS in every possible window below -30 dBFS.
    let peak = mix.iter().map(|sample| sample.abs()).max().unwrap_or(1);
    mix.into_iter()
        .map(|sample| (sample * PEAK / peak.max(1)) as i16)
        .collect()
}

fn wav(samples: &[i16]) -> Vec<u8> {
    let data_size = u32::try_from(samples.len() * 2).expect("the fixed score fits a WAV");
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block alignment
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn largest_window_energy(samples: &[i16]) -> u64 {
    let square = |sample: i16| i64::from(sample).pow(2) as u64;
    let mut sum: u64 = samples[..WINDOW].iter().map(|&sample| square(sample)).sum();
    let mut largest = sum;
    for index in WINDOW..samples.len() {
        sum = sum + square(samples[index]) - square(samples[index - WINDOW]);
        largest = largest.max(sum);
    }
    largest
}

/// A byte fingerprint for reproducibility, not an authenticity check. The
/// package format uses its own SHA-256 for every staged file.
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
    })
}

fn write_identical_or_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && fs::read(path)? == bytes => Ok(()),
        Ok(_) => Err(io::Error::other(format!(
            "{} already exists and is not the identical generated file",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            File::create_new(path)?.write_all(bytes)
        }
        Err(error) => Err(error),
    }
}

fn run() -> io::Result<()> {
    if std::env::args_os().len() != 1
        || !Path::new("crates/skills/Cargo.toml").is_file()
        || !Path::new("examples/relaxing-music/score.rs").is_file()
    {
        return Err(io::Error::other(
            "run from the lampOS root: cargo run -p lamp-skills --release --example render_relaxing_music (no arguments)",
        ));
    }
    for folder in [
        "artifacts",
        "artifacts/skills",
        "artifacts/skills/relaxing-music",
        "artifacts/skills/relaxing-music/staged",
        DESTINATION,
    ] {
        match fs::create_dir(folder) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !fs::symlink_metadata(folder)?.is_dir() {
                    return Err(io::Error::other(format!("{folder} is not a plain folder")));
                }
            }
            Err(error) => return Err(error),
        }
    }
    let samples = render();
    let bytes = wav(&samples);
    for (name, content) in TEXT_FILES {
        write_identical_or_new(&Path::new(DESTINATION).join(name), content)?;
    }
    write_identical_or_new(&Path::new(DESTINATION).join("relaxing.wav"), &bytes)?;
    println!(
        "{DESTINATION}/relaxing.wav: rate={RATE} frames={} duration_ms={DURATION_MS} bytes={} peak={} largest_240_frame_sum_squares={} fnv1a64={:016x}",
        samples.len(),
        bytes.len(),
        samples
            .iter()
            .map(|sample| i32::from(*sample).abs())
            .max()
            .unwrap_or(0),
        largest_window_energy(&samples),
        fingerprint(&bytes),
    );
    Ok(())
}

fn main() -> io::Result<()> {
    run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_score_is_bounded_and_has_eight_changes_and_sixteen_upper_notes() {
        assert_eq!(DURATION_MS, 48_000);
        assert_eq!(notes().len(), 48);
        assert!(PAD_CHORDS.windows(2).all(|chords| chords[0] != chords[1]));
        assert_eq!(UPPER_NOTES.len(), 16);
        assert!(UPPER_NOTES.windows(2).all(|notes| notes[0].0 < notes[1].0));
        for note in notes() {
            assert!(note.duration_ms > 0);
            assert!(note.at_ms + note.duration_ms <= DURATION_MS);
            assert!((100_000..=600_000).contains(&note.milli_hz));
            assert!(note.attack_ms > 0 && note.release_ms > 0);
            assert!(note.attack_ms + note.release_ms <= note.duration_ms);
            assert!((1..=220).contains(&note.level));
        }
    }

    #[test]
    fn the_integer_oscillator_has_balanced_cycles_and_bounded_harmonics() {
        assert_eq!(sine(0), 0);
        assert_eq!(sine(1 << 30), Q15);
        assert_eq!(sine(1 << 31), 0);
        assert_eq!(sine(3 << 30), -Q15);
        for point in 0..65_536u32 {
            let phase = point << 16;
            assert!(sine(phase).abs() <= Q15);
            assert_eq!(sine(phase), -sine(phase.wrapping_add(1 << 31)));
        }
    }

    #[test]
    fn every_sliding_ten_millisecond_window_and_the_peak_meet_the_limits() {
        let samples = render();
        assert_eq!(samples.len(), 1_152_000);
        assert!(samples.len() <= frames(60_000));
        assert_eq!(samples.iter().map(|s| i32::from(*s).abs()).max(), Some(900));
        // floor(32767^2 / 1000): a conservative integer -30 dBFS RMS limit.
        assert!(largest_window_energy(&samples) <= WINDOW as u64 * 1_073_676);
        // floor(32767 * 10^(-12/20)): a conservative -12 dBFS peak limit.
        assert!(
            samples
                .iter()
                .all(|sample| i32::from(*sample).abs() <= 8_230)
        );
        assert_eq!(samples.first(), Some(&0));
        assert_eq!(samples.last(), Some(&0));
        let energy = |part: &[i16]| -> u64 {
            part.iter()
                .map(|sample| i64::from(*sample).pow(2) as u64)
                .sum()
        };
        let second = RATE as usize;
        assert!(energy(&samples[..second]) < energy(&samples[2 * second..3 * second]) / 4);
        assert!(energy(&samples[47 * second..]) < energy(&samples[43 * second..44 * second]) / 4);
        for bar in samples.chunks(frames(BAR_MS)) {
            assert!(energy(bar) > 1_000_000, "each phrase must have real audio");
        }
    }

    #[test]
    fn the_pcm_and_canonical_wav_bytes_are_reproducible() {
        let samples = render();
        assert_eq!(samples, render());
        let bytes = wav(&samples);
        assert_eq!(bytes.len(), 2_304_044);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(&bytes[20..24], &[1, 0, 1, 0]);
        assert_eq!(&bytes[24..28], &24_000u32.to_le_bytes());
        assert_eq!(&bytes[28..32], &48_000u32.to_le_bytes());
        assert_eq!(&bytes[32..40], &[2, 0, 16, 0, b'd', b'a', b't', b'a']);
        assert_eq!(&bytes[40..44], &2_304_000u32.to_le_bytes());
        assert_eq!(&bytes[4..8], &2_304_036u32.to_le_bytes());
        assert_eq!(fingerprint(&bytes), 0x2330_6445_3c39_34b7);
    }
}
