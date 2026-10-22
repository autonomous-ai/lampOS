//! `--step-ms` with the real models (#694): a word that the models hear
//! at its best place on the 80 ms steps is heard at 10 ms wherever it
//! falls. The models are not in the repository, so this runs only where
//! they are installed:
//!
//!     LAMP_NAME_MODELS=/opt/lampOS/name/models LAMP_NAME_SOUND=a.wav:b.raw \
//!         cargo test -p lamp-name --test it step_models:: -- --ignored
//!
//! Each file is 16 kHz, one channel, 16 bits, raw or WAV, with the lamp's
//! name in it after two seconds or more, given 20 ms at a time as the
//! voice sends it.
use lamp_name::hear::{Hearer, STEP};
use std::path::Path;

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

/// The highest score the models give `samples` at steps of `step_ms`.
fn highest(models: &Path, step_ms: usize, samples: &[i16]) -> f32 {
    highest_woken(models, step_ms, None, samples)
}

/// The same, the later grids woken by `wake` if given.
fn highest_woken(models: &Path, step_ms: usize, wake: Option<f32>, samples: &[i16]) -> f32 {
    let mut hearer = Hearer::load(models).unwrap().with_step_ms(step_ms).unwrap();
    if let Some(bar) = wake {
        hearer = hearer.with_wake(bar, 0.9).unwrap().waiting(true);
    }
    let mut best = 0f32;
    for block in samples.chunks(320) {
        for score in hearer.hear(block).unwrap() {
            best = best.max(score.name);
        }
    }
    best
}

#[test]
#[ignore = "needs the installed models: LAMP_NAME_MODELS and LAMP_NAME_SOUND"]
fn a_word_heard_at_its_best_place_is_heard_at_10_ms_wherever_it_falls() {
    let models = std::env::var("LAMP_NAME_MODELS").expect("LAMP_NAME_MODELS");
    let sound = std::env::var("LAMP_NAME_SOUND").expect("LAMP_NAME_SOUND");
    let models = Path::new(&models);
    for file in sound.split(':') {
        let samples = samples_of(file);
        // The word moved by 0 to 70 ms: where the 80 ms steps fall on it.
        let shifted = |by: usize| &samples[by * STEP / 8..];
        let at_80: Vec<f32> = (0..8).map(|by| highest(models, 80, shifted(by))).collect();
        let best = at_80.iter().copied().fold(0f32, f32::max);
        assert!(best >= 0.9, "{file}: not heard at any place: {at_80:?}");
        for by in 0..8 {
            let at_10 = highest(models, 10, shifted(by));
            assert!(
                at_10 >= best - 0.005,
                "{file} moved {} ms: {at_10:.4} at 10 ms, {best:.4} at its best place on 80",
                by * 10
            );
        }
        println!(
            "{file}: at 80 ms {}, so {best:.4} at its best place; heard at 10 ms wherever it falls",
            at_80
                .iter()
                .map(|score| format!("{score:.2}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
}

/// `--wake` (#694): the later grids woken by the first grid's score over
/// 0.03 hear every word they hear always awake (over 0.9), wherever the 80 ms steps
/// fall on it, and take far less time over the sound before it. Each file
/// is timed whole: always awake, woken, and the 80 ms grid alone.
#[test]
#[ignore = "needs the installed models: LAMP_NAME_MODELS and LAMP_NAME_SOUND"]
fn woken_grids_hear_what_the_grids_always_awake_hear() {
    let models = std::env::var("LAMP_NAME_MODELS").expect("LAMP_NAME_MODELS");
    let sound = std::env::var("LAMP_NAME_SOUND").expect("LAMP_NAME_SOUND");
    let models = Path::new(&models);
    let timed = |step_ms, wake, samples: &[i16]| {
        let started = std::time::Instant::now();
        let best = highest_woken(models, step_ms, wake, samples);
        (best, started.elapsed())
    };
    for file in sound.split(':') {
        let samples = samples_of(file);
        for by in 0..8 {
            let shifted = &samples[by * STEP / 8..];
            let (awake, awake_took) = timed(10, None, shifted);
            let (woken, woken_took) = timed(10, Some(0.03), shifted);
            let (alone, alone_took) = timed(80, None, shifted);
            // Heard wherever always awake hears it; a call the first grid
            // heard keeps its own score, its later grids' late ones let go.
            let heard_both = awake < 0.9 || woken >= 0.9;
            assert!(
                heard_both && (awake >= 0.9 || woken >= awake - 0.005),
                "{file} moved {} ms: {woken:.4} woken, {awake:.4} always awake",
                by * 10
            );
            println!(
                "{file} +{}ms: always awake {awake:.3} in {awake_took:?}, woken {woken:.3} in {woken_took:?}, 80 ms alone {alone:.3} in {alone_took:?}",
                by * 10
            );
        }
    }
}
