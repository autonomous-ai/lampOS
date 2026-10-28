//! Off the lamp (#1669): a WAV file's room sound scores, as `lamp-ears`
//! with `LAMP_EARS_CLASSES=on` would publish them, so a track or a line can
//! be checked on a Mac before a lamp run and the lamp's numbers read
//! against it. Run only when named:
//!
//!   cargo run --release -p lamp-ears --example score_wav -- MODEL.onnx FILE.wav...
//!
//! MODEL is the pinned `yamnet.onnx` with `yamnet.data` beside it
//! (`classes.rs`; neither is in the repository). Each FILE is 16 kHz mono
//! 16-bit. It prints each 500 ms window's speech, music, laughter, door (the
//! door group's highest, a knock among them) and footsteps, then,
//! per file, how many windows clear the skills' provisional bars and the
//! longest stretch music holds (Music sway asks for 20 s, with 5 s gaps).
//! The scores are the model's on the file's own sound, with no lamp, no
//! room and no microphone between.
use lamp_ears::classes::{Frontend, Model, PATCH_HOP, bounds};
use std::path::Path;

/// `skills/music-sway/skill.toml`'s `above`, `for` and `gap`.
const MUSIC_ABOVE: f32 = 0.6;
const MUSIC_FOR_S: f32 = 20.0;
const MUSIC_GAP_S: f32 = 5.0;
/// `skills/laugh-glance/skill.toml`'s `above`.
const LAUGHTER_ABOVE: f32 = 0.05;
const RATE: usize = 16_000;

fn main() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [model, files @ ..] = arguments.as_slice() else {
        return Err("usage: score_wav MODEL.onnx FILE.wav...".into());
    };
    if files.is_empty() {
        return Err("usage: score_wav MODEL.onnx FILE.wav...".into());
    }
    let model = Model::load(Path::new(model))?;
    let frontend = Frontend::new();
    println!("file\tend_s\tspeech\tmusic\tlaughter\tdoor\tfootsteps");
    for file in files {
        let pcm = read(file)?;
        let mut windows = Vec::new();
        let mut first_frame = 0;
        while bounds(first_frame).end_sample <= pcm.len() {
            let window = bounds(first_frame);
            let scores = model.score(&frontend.features(&pcm, 0, window)?)?;
            let end_s = window.end_sample as f32 / RATE as f32;
            println!(
                "{file}\t{end_s:.1}\t{:.2}\t{:.2}\t{:.2}\t{:.3}\t{:.3}",
                scores.speech, scores.music, scores.laughter, scores.door, scores.footsteps
            );
            windows.push((end_s, scores));
            first_frame += PATCH_HOP;
        }
        let music: Vec<f32> = windows
            .iter()
            .filter(|(_, scores)| scores.music > MUSIC_ABOVE)
            .map(|(at, _)| *at)
            .collect();
        let laughs = windows
            .iter()
            .filter(|(_, scores)| scores.laughter > LAUGHTER_ABOVE)
            .count();
        eprintln!(
            "{file}: {:.1} s, {} windows; music over {MUSIC_ABOVE} in {}, held at most {:.1} s \
             (sway at {MUSIC_FOR_S} s: {}); laughter over {LAUGHTER_ABOVE} in {laughs}",
            pcm.len() as f32 / RATE as f32,
            windows.len(),
            music.len(),
            longest(&music),
            if longest(&music) >= MUSIC_FOR_S {
                "yes"
            } else {
                "no"
            },
        );
    }
    Ok(())
}

/// The longest stretch of windows over the bar, a gap of up to
/// `MUSIC_GAP_S` between two of them not ending it.
fn longest(over: &[f32]) -> f32 {
    let mut best = 0.0_f32;
    let mut start = None;
    let mut last = 0.0;
    for &at in over {
        if start.is_none() || at - last > MUSIC_GAP_S {
            start = Some(at);
        }
        last = at;
        best = best.max(at - start.unwrap_or(at));
    }
    best
}

fn read(path: &str) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|error| format!("{path}: {error}"))?;
    let spec = reader.spec();
    if spec.sample_rate as usize != RATE || spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(format!("{path}: not 16 kHz mono 16-bit"));
    }
    reader
        .samples::<i16>()
        .map(|sample| {
            sample
                .map(|x| f32::from(x) / 32768.0)
                .map_err(|error| format!("{path}: {error}"))
        })
        .collect()
}

#[test]
fn a_gap_of_five_seconds_does_not_end_the_music_and_a_longer_one_does() {
    assert_eq!(longest(&[]), 0.0);
    assert_eq!(longest(&[1.0, 6.0, 11.0]), 10.0);
    assert_eq!(longest(&[1.0, 6.5, 7.0]), 0.5);
}
