//! The three models, run every 80 ms over what the microphone hears.
//!
//! This is openWakeWord's design, with a last model of our own:
//!
//! 1. `melspectrogram.onnx`: the newest 1,760 samples become eight frames
//!    of 32 numbers, one frame for every 10 ms.
//! 2. `embedding_model.onnx` (Google's speech embedding): 76 of those
//!    frames, 0.76 s, become 96 numbers that say what kind of speech
//!    sound that was. A new one every 80 ms.
//! 3. `name.onnx`: the newest sixteen of those, 1.28 s, become one score:
//!    how likely it is that the lamp's name has just been said. This one
//!    is trained by us, on the name.
//!
//! The first two are fixed and know nothing of lamps. The files are not in
//! the repository: they are installed beside the program (`docs/name.md`).
use std::{collections::VecDeque, path::Path, sync::Arc};
use tract_onnx::prelude::*;

/// A step: 80 ms at 16 kHz. Every step gives one score once the models
/// have heard enough.
pub const STEP: usize = 1_280;
/// What the first model is given at each step: the step and the 30 ms
/// before it.
const CHUNK: usize = 1_760;
/// Numbers in a frame, frames a step makes, frames the second model
/// takes, numbers it gives, and how many of those the third takes.
const BINS: usize = 32;
const FRAMES_A_STEP: usize = 8;
const FRAMES: usize = 76;
const EMBEDDING: usize = 96;
const CONTEXT: usize = 16;

/// The sound one score is of: the 25 chunks whose frames make its sixteen
/// embeddings, 2.03 s. A score is of these samples and of nothing before
/// them, so the models may start afresh this far back of any score and
/// give the same one (`quiet.rs`).
pub const WINDOW: usize =
    STEP * ((FRAMES + FRAMES_A_STEP * (CONTEXT - 1)).div_ceil(FRAMES_A_STEP) - 1) + CHUNK;
const _: () = assert!(WINDOW == 32_480);

/// The three files, by the names they are installed under.
pub const FILES: [&str; 3] = ["melspectrogram.onnx", "embedding_model.onnx", "name.onnx"];

type Plan = Arc<TypedRunnableModel>;

fn load(path: &Path, shape: &[usize]) -> Result<Plan, String> {
    let said = |error: TractError| format!("{}: {error}", path.display());
    tract_onnx::onnx()
        .with_ignore_value_info(true)
        .with_ignore_output_shapes(true)
        .model_for_path(path)
        .map_err(said)?
        .with_input_fact(0, f32::fact(shape).into())
        .map_err(said)?
        .into_optimized()
        .map_err(said)?
        .into_runnable()
        .map_err(said)
}

fn run(plan: &Plan, shape: &[usize], values: &[f32], what: &str) -> Result<Vec<f32>, String> {
    let said = |error: TractError| format!("{what}: {error}");
    let input = Tensor::from_shape(shape, values).map_err(said)?;
    let output = plan.run(tvec!(input.into())).map_err(said)?;
    let first = output.first().ok_or_else(|| format!("{what}: no output"))?;
    Ok(first
        .to_plain_array_view::<f32>()
        .map_err(said)?
        .iter()
        .copied()
        .collect())
}

/// The listener: the models, and the little of the sound they still need.
pub struct Hearer {
    mel: Plan,
    embedding: Plan,
    name: Plan,
    /// Samples not yet made into frames, on the scale of 16-bit sound.
    samples: Vec<f32>,
    /// Frames the second model may still need, and how many came before
    /// the first of them.
    frames: VecDeque<[f32; BINS]>,
    frames_gone: usize,
    /// How many embeddings have been made, and the newest of them.
    made: usize,
    embeddings: VecDeque<Vec<f32>>,
}

impl Hearer {
    /// The models from `directory`, checked by running each once on
    /// silence: a file that is not the model it is named for is found out
    /// here and not at the first word.
    pub fn load(directory: &Path) -> Result<Self, String> {
        let [mel, embedding, name] = FILES;
        let mut hearer = Self {
            mel: load(&directory.join(mel), &[1, CHUNK])?,
            embedding: load(&directory.join(embedding), &[1, FRAMES, BINS, 1])?,
            name: load(&directory.join(name), &[1, CONTEXT, EMBEDDING])?,
            samples: Vec::with_capacity(CHUNK + STEP),
            frames: VecDeque::with_capacity(FRAMES + FRAMES_A_STEP),
            frames_gone: 0,
            made: 0,
            embeddings: VecDeque::with_capacity(CONTEXT),
        };
        // Enough silence for one score: 2.9 s.
        let scores = hearer.hear(&vec![0i16; STEP * 36])?;
        if scores.is_empty() || scores.iter().any(|score| !(0.0..=1.0).contains(score)) {
            return Err(format!(
                "the models in {} do not give a score for silence",
                directory.display()
            ));
        }
        hearer.forget();
        Ok(hearer)
    }

    /// Start again, as if nothing had been heard: after sound was lost,
    /// or the microphone was closed. What the models held is of before.
    pub fn forget(&mut self) {
        self.samples.clear();
        self.frames.clear();
        self.frames_gone = 0;
        self.made = 0;
        self.embeddings.clear();
    }

    /// More of what the microphone hears, 16 kHz, one channel. A score
    /// for every step that became whole with it, oldest first: none for
    /// about the first two seconds after a start, then one every 80 ms.
    pub fn hear(&mut self, samples: &[i16]) -> Result<Vec<f32>, String> {
        self.samples
            .extend(samples.iter().map(|sample| f32::from(*sample)));
        let mut scores = Vec::new();
        while self.samples.len() >= CHUNK {
            let mel = run(&self.mel, &[1, CHUNK], &self.samples[..CHUNK], FILES[0])?;
            if mel.len() != FRAMES_A_STEP * BINS {
                return Err(format!(
                    "{}: {} numbers for a step, not {}",
                    FILES[0],
                    mel.len(),
                    FRAMES_A_STEP * BINS
                ));
            }
            for frame in mel.chunks_exact(BINS) {
                let mut row = [0f32; BINS];
                // The scale the second model was trained on.
                for (to, from) in row.iter_mut().zip(frame) {
                    *to = from / 10.0 + 2.0;
                }
                self.frames.push_back(row);
            }
            self.samples.drain(..STEP);
            // Embedding k is of frames 8k to 8k + 76.
            while FRAMES_A_STEP * self.made + FRAMES <= self.frames_gone + self.frames.len() {
                let from = FRAMES_A_STEP * self.made - self.frames_gone;
                let flat: Vec<f32> = self
                    .frames
                    .range(from..from + FRAMES)
                    .flat_map(|frame| frame.iter().copied())
                    .collect();
                let embedding = run(&self.embedding, &[1, FRAMES, BINS, 1], &flat, FILES[1])?;
                if embedding.len() != EMBEDDING {
                    return Err(format!(
                        "{}: {} numbers, not {EMBEDDING}",
                        FILES[1],
                        embedding.len()
                    ));
                }
                if self.embeddings.len() == CONTEXT {
                    self.embeddings.pop_front();
                }
                self.embeddings.push_back(embedding);
                self.made += 1;
                // Frames before the next embedding's first are done with.
                while self.frames_gone < FRAMES_A_STEP * self.made {
                    self.frames.pop_front();
                    self.frames_gone += 1;
                }
                if self.embeddings.len() == CONTEXT {
                    let flat: Vec<f32> = self
                        .embeddings
                        .iter()
                        .flat_map(|embedding| embedding.iter().copied())
                        .collect();
                    let score = run(&self.name, &[1, CONTEXT, EMBEDDING], &flat, FILES[2])?;
                    scores.push(*score.first().ok_or("name.onnx: no score")?);
                }
            }
        }
        Ok(scores)
    }
}
