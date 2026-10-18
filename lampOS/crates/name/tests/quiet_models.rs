//! `--quiet-below` with the real models (#705, heat): every score the
//! rested models give is the one they give never resting. The models are
//! not in the repository, so this runs only where they are installed:
//!
//!     LAMP_NAME_MODELS=/opt/lampOS/name/models LAMP_NAME_SOUND=a.wav:b.raw \
//!         cargo test -p lamp-name --test quiet_models -- --ignored
//!
//! Each file is 16 kHz, one channel, 16 bits, raw or WAV, given 20 ms at a
//! time as the voice sends it.
use lamp_name::{hear::Hearer, quiet::Quiet};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const BAR_DB: f32 = -50.0;

fn samples_of(path: &str) -> Vec<i16> {
    let bytes = std::fs::read(path).unwrap();
    let sound = if bytes.starts_with(b"RIFF") && bytes.len() >= 44 {
        &bytes[44..]
    } else {
        &bytes[..]
    };
    sound
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}

/// Each score by the step it is of, and the longest one block took: with
/// a rest, the start at the first sound.
fn scores(
    models: &Path,
    quiet_below: Option<f32>,
    samples: &[i16],
) -> (Vec<(usize, f32)>, Duration) {
    let mut quiet = Quiet::new(Hearer::load(models).unwrap(), quiet_below);
    let mut all = Vec::new();
    let mut longest = Duration::ZERO;
    for block in samples.chunks(320) {
        let began = Instant::now();
        let heard = quiet.hear(block).unwrap();
        longest = longest.max(began.elapsed());
        let first = quiet.steps() + 1 - heard.scores.len();
        all.extend((first..).zip(heard.scores));
    }
    (all, longest)
}

#[test]
#[ignore = "needs the installed models: LAMP_NAME_MODELS and LAMP_NAME_SOUND"]
fn the_real_models_give_the_same_scores_after_a_rest() {
    let models = std::env::var("LAMP_NAME_MODELS").expect("LAMP_NAME_MODELS");
    let sound = std::env::var("LAMP_NAME_SOUND").expect("LAMP_NAME_SOUND");
    for file in sound.split(':') {
        let samples = samples_of(file);
        let (always, block_always) = scores(Path::new(&models), None, &samples);
        let (rested, block_rested) = scores(Path::new(&models), Some(BAR_DB), &samples);
        for (step, score) in &rested {
            assert_eq!(always[step - 1], (*step, *score), "{file}: step {step}");
        }
        let highest = |scores: &[(usize, f32)]| scores.iter().map(|(_, s)| *s).fold(0f32, f32::max);
        println!(
            "{file}: {} steps, {} worked out at {BAR_DB} dBFS; highest {:.4} rested, {:.4} never resting; longest block {:.1} ms rested, {:.1} ms never resting",
            always.len(),
            rested.len(),
            highest(&rested),
            highest(&always),
            block_rested.as_secs_f64() * 1e3,
            block_always.as_secs_f64() * 1e3,
        );
    }
}
