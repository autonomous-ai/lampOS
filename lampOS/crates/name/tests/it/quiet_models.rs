//! `--quiet-below` with the real models (#705, heat): every score the
//! rested models give is the one they give never resting. The models are
//! not in the repository, so this runs only where they are installed:
//!
//!     LAMP_NAME_MODELS=/opt/lampOS/name/models LAMP_NAME_SOUND=a.wav:b.raw \
//!         cargo test -p lamp-name --test it quiet_models:: -- --ignored
//!
//! Each file is 16 kHz, one channel, 16 bits, raw or WAV, given 20 ms at a
//! time as the voice sends it.
use lamp_name::{
    hear::Hearer,
    quiet::{LEAD, Quiet},
};
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
        all.extend((first..).zip(heard.scores.into_iter().map(|score| score.name)));
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

/// #6462: a hearer led at load ([`Hearer::with_lead`]) gives at each start
/// and forget the scores of one that hears the silence then, at the cost of
/// a copy: the first block after a forget is timed both ways. With
/// `--step-ms 10` and a wake bar too, the later grids on their own thread.
#[test]
#[ignore = "needs the installed models: LAMP_NAME_MODELS and LAMP_NAME_SOUND"]
fn a_lead_worked_out_once_scores_as_the_silence_heard_at_each_forget() {
    let models = std::env::var("LAMP_NAME_MODELS").expect("LAMP_NAME_MODELS");
    let sound = std::env::var("LAMP_NAME_SOUND").expect("LAMP_NAME_SOUND");
    let models = Path::new(&models);
    let hearer = |step_ms: usize, led: bool| {
        let mut hearer = Hearer::load(models).unwrap().with_step_ms(step_ms).unwrap();
        if step_ms < 80 {
            hearer = hearer.with_wake(0.1, 0.5).unwrap().waiting(true);
        }
        if led {
            hearer = hearer.with_lead(LEAD).unwrap();
        }
        Quiet::new(hearer, None)
    };
    for file in sound.split(':') {
        let samples = samples_of(file);
        for step_ms in [80, 10] {
            let mut ways = Vec::new();
            for led in [true, false] {
                let mut quiet = hearer(step_ms, led);
                let (mut all, mut after_forget) = (Vec::new(), Vec::new());
                for round in 0..3 {
                    if round > 0 {
                        quiet.forget();
                    }
                    for (at, block) in samples.chunks(320).enumerate() {
                        let began = Instant::now();
                        let heard = quiet.hear(block).unwrap();
                        if at == 0 {
                            after_forget.push(began.elapsed());
                        }
                        let first = quiet.steps() + 1 - heard.scores.len();
                        all.extend(
                            (first..)
                                .zip(heard.scores.into_iter().map(|score| (round, score.name))),
                        );
                    }
                }
                ways.push((all, after_forget));
            }
            let (led, heard) = (&ways[0], &ways[1]);
            assert!(!led.0.is_empty());
            assert_eq!(led.0, heard.0, "{file}: --step-ms {step_ms}");
            let ms = |times: &[Duration]| {
                times
                    .iter()
                    .map(|took| format!("{:.2}", took.as_secs_f64() * 1e3))
                    .collect::<Vec<_>>()
                    .join("/")
            };
            println!(
                "{file} --step-ms {step_ms}: {} scores the same; first block after start/forget/forget {} ms led at load, {} ms hearing the silence",
                led.0.len(),
                ms(&led.1),
                ms(&heard.1),
            );
        }
    }
}
