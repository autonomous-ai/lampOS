//! The three models, run every 80 ms over what the microphone hears (or
//! on finer grids with `--step-ms`, [`Grids`]).
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
//!
//! 4. With `--stop-least`, `stop.onnx` (`models/stop/`, #828) on the same
//!    sixteen embeddings: how likely it is that "stop", "wait" or "hold on"
//!    has just been said (`docs/stop-word.md`). It costs about 24 µs a step
//!    beside the name's 9–10 ms, and is loaded only when asked for.
//! 5. With `--rescue-model`, a second name model of the same shape on the
//!    same sixteen embeddings, trained on the lamp's own recordings, its
//!    SHA-256 given beside it (`rescue.rs`, #1546). Loaded only when asked
//!    for.
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
/// The stop model, installed beside them, and the SHA-256 it must have
/// (`models/stop/SOURCE.md`): another file under its name is refused.
pub const STOP_FILE: &str = "stop.onnx";
pub const STOP_SHA256: &str = "5fe07569e3d7c5adcdf496770deb306701b057ab8adce1d03ce901db3ce72d41";

/// Whether `bytes` are the stop model: its pinned SHA-256.
pub fn is_the_stop_model(bytes: &[u8]) -> Result<(), String> {
    has_sha256(bytes, STOP_SHA256, "the stop model's")
}

/// Whether `bytes` have the SHA-256 `expected` (lower-case hex), said as
/// `whose` when they do not.
pub fn has_sha256(bytes: &[u8], expected: &str, whose: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let digest: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if digest == expected {
        Ok(())
    } else {
        Err(format!("SHA-256 {digest}, not {whose} {expected}"))
    }
}

/// One step's scores: the name's, and the stop and rescue models' when
/// they are loaded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Score {
    pub name: f32,
    pub stop: Option<f32>,
    pub rescue: Option<f32>,
}

type Plan = Arc<TypedRunnableModel>;

fn load(path: &Path, shape: &[usize]) -> Result<Plan, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    load_bytes(&bytes, path, shape)
}

/// A model from the bytes already read from `path`: a pinned file is run
/// from the bytes that were hashed, never opened again by its path.
fn load_bytes(bytes: &[u8], path: &Path, shape: &[usize]) -> Result<Plan, String> {
    let said = |error: TractError| format!("{}: {error}", path.display());
    tract_onnx::onnx()
        .with_ignore_value_info(true)
        .with_ignore_output_shapes(true)
        .model_for_read(&mut &bytes[..])
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

/// A model on the sixteen embeddings beside `name.onnx`: its file at
/// `path`, refused unless its SHA-256 is `sha256`, and checked by a score
/// from 0 to 1 for silence.
fn head(path: &Path, sha256: &str, whose: &str) -> Result<Plan, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    has_sha256(&bytes, sha256, whose).map_err(|why| format!("{}: {why}", path.display()))?;
    let plan = load_bytes(&bytes, path, &[1, CONTEXT, EMBEDDING])?;
    let what = path.display().to_string();
    let silence = run(
        &plan,
        &[1, CONTEXT, EMBEDDING],
        &[0.0; CONTEXT * EMBEDDING],
        &what,
    )?;
    if !silence
        .first()
        .is_some_and(|score| (0.0..=1.0).contains(score))
    {
        return Err(format!("{what}: no score from 0 to 1"));
    }
    Ok(plan)
}

/// The steps `--step-ms` takes, in ms: 80 is one grid, as always; 40 and
/// 20 or 10 add grids that start that much later (#694): the models
/// take frames of 10 ms, so only 10 tries every place a word can fall.
pub const STEPS_MS: [usize; 4] = [80, 40, 20, 10];

/// The listener: the models, and the little of the sound they still need.
pub struct Hearer {
    models: Models,
    /// One for each grid, the first on the steps as always and each other
    /// [`Hearer::with_step_ms`]'s step later than the one before it.
    streams: Vec<Stream>,
    /// The later grids' newest scores, waiting for the first grid's.
    grids: Grids,
}

/// The models a step is worked out with, the same for every grid.
struct Models {
    mel: Plan,
    embedding: Plan,
    name: Plan,
    /// `stop.onnx`, only with `--stop-least` ([`Hearer::with_stop`]).
    stop: Option<Plan>,
    /// The rescue model, only with `--rescue-model` ([`Hearer::with_rescue`]).
    rescue: Option<Plan>,
}

/// One grid of steps: the sound it has not yet made into frames, and the
/// frames and embeddings it may still need.
struct Stream {
    /// Samples to pass over after a start, so this grid's steps fall
    /// later than the first grid's; 0 for the first.
    offset: usize,
    skip: usize,
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

/// A step's score is the highest of the first grid's for it and each
/// later grid's newest: that one is of the step before, `k` times
/// `--step-ms` later, so it is whole before the first grid's (#694). A
/// name that falls between two steps of 80 ms is scored where it lies.
/// Each step still has one score, every 80 ms, so the rest of the
/// program counts steps as before. The first step after a start has the
/// first grid's alone.
#[derive(Default)]
pub struct Grids {
    /// The first grid's scores so far.
    made: u64,
    /// For each later grid: how many scores it has given, and those the
    /// first grid has not yet taken, by number.
    later: Vec<(u64, VecDeque<(u64, Score)>)>,
}

impl Grids {
    /// For `count` grids in all.
    pub fn new(count: usize) -> Self {
        Self {
            made: 0,
            later: (1..count).map(|_| (0, VecDeque::new())).collect(),
        }
    }

    /// Later grid `grid` (from 1) gave `score`.
    pub fn later(&mut self, grid: usize, score: Score) {
        let (given, waiting) = &mut self.later[grid - 1];
        waiting.push_back((*given, score));
        *given += 1;
    }

    /// The first grid gave `score`: the step's score.
    pub fn first(&mut self, score: Score) -> Score {
        let step = self.made;
        self.made += 1;
        let mut best = score;
        for (_, waiting) in &mut self.later {
            while waiting.front().is_some_and(|(at, _)| at + 1 < step) {
                waiting.pop_front();
            }
            if let Some((at, later)) = waiting.front()
                && at + 1 == step
            {
                best = higher(best, *later);
            }
        }
        best
    }

    pub fn forget(&mut self) {
        *self = Self::new(self.later.len() + 1);
    }
}

/// Each score the higher of the two.
fn higher(one: Score, other: Score) -> Score {
    let max = |a: Option<f32>, b: Option<f32>| match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    Score {
        name: one.name.max(other.name),
        stop: max(one.stop, other.stop),
        rescue: max(one.rescue, other.rescue),
    }
}

impl Stream {
    fn new(offset: usize) -> Self {
        Self {
            offset,
            skip: offset,
            samples: Vec::with_capacity(CHUNK + STEP),
            frames: VecDeque::with_capacity(FRAMES + FRAMES_A_STEP),
            frames_gone: 0,
            made: 0,
            embeddings: VecDeque::with_capacity(CONTEXT),
        }
    }

    fn forget(&mut self) {
        *self = Self::new(self.offset);
    }

    fn hear(&mut self, models: &Models, samples: &[i16]) -> Result<Vec<Score>, String> {
        let passed = self.skip.min(samples.len());
        self.skip -= passed;
        self.samples
            .extend(samples[passed..].iter().map(|sample| f32::from(*sample)));
        let mut scores = Vec::new();
        while self.samples.len() >= CHUNK {
            let mel = run(&models.mel, &[1, CHUNK], &self.samples[..CHUNK], FILES[0])?;
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
                let embedding = run(&models.embedding, &[1, FRAMES, BINS, 1], &flat, FILES[1])?;
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
                    let score = run(&models.name, &[1, CONTEXT, EMBEDDING], &flat, FILES[2])?;
                    let stop = match &models.stop {
                        Some(stop) => Some(
                            *run(stop, &[1, CONTEXT, EMBEDDING], &flat, STOP_FILE)?
                                .first()
                                .ok_or("stop.onnx: no score")?,
                        ),
                        None => None,
                    };
                    let rescue = match &models.rescue {
                        Some(rescue) => Some(
                            *run(rescue, &[1, CONTEXT, EMBEDDING], &flat, "the rescue model")?
                                .first()
                                .ok_or("the rescue model: no score")?,
                        ),
                        None => None,
                    };
                    scores.push(Score {
                        name: *score.first().ok_or("name.onnx: no score")?,
                        stop,
                        rescue,
                    });
                }
            }
        }
        Ok(scores)
    }
}

impl Hearer {
    /// The models from `directory`, checked by running each once on
    /// silence: a file that is not the model it is named for is found out
    /// here and not at the first word.
    pub fn load(directory: &Path) -> Result<Self, String> {
        let [mel, embedding, name] = FILES;
        let mut hearer = Self {
            models: Models {
                mel: load(&directory.join(mel), &[1, CHUNK])?,
                embedding: load(&directory.join(embedding), &[1, FRAMES, BINS, 1])?,
                name: load(&directory.join(name), &[1, CONTEXT, EMBEDDING])?,
                stop: None,
                rescue: None,
            },
            streams: vec![Stream::new(0)],
            grids: Grids::new(1),
        };
        // Enough silence for one score: 2.9 s.
        let scores = hearer.hear(&vec![0i16; STEP * 36])?;
        if scores.is_empty()
            || scores
                .iter()
                .any(|score| !(0.0..=1.0).contains(&score.name))
        {
            return Err(format!(
                "the models in {} do not give a score for silence",
                directory.display()
            ));
        }
        hearer.forget();
        Ok(hearer)
    }

    /// The stop model from `directory` as well, checked by its SHA-256 and
    /// by a score for silence. Its scores come beside the name's.
    pub fn with_stop(mut self, directory: &Path) -> Result<Self, String> {
        self.models.stop = Some(head(
            &directory.join(STOP_FILE),
            STOP_SHA256,
            "the stop model's",
        )?);
        Ok(self)
    }

    /// The rescue model at `path` as well (`rescue.rs`), checked by the
    /// SHA-256 given for it and by a score for silence. Its scores come
    /// beside the name's.
    pub fn with_rescue(mut self, path: &Path, sha256: &str) -> Result<Self, String> {
        self.models.rescue = Some(head(path, sha256, "the one given,")?);
        Ok(self)
    }

    /// Steps of `ms` (one of [`STEPS_MS`]) in place of 80: 80 / `ms` grids,
    /// each `ms` later than the one before, each step's score the highest
    /// of them ([`Grids`]). The models' work grows as many times (#694).
    pub fn with_step_ms(mut self, ms: usize) -> Result<Self, String> {
        if !STEPS_MS.contains(&ms) {
            return Err(format!("a step of {ms} ms: 80, 40, 20 or 10"));
        }
        let count = 80 / ms;
        let samples = STEP / count;
        self.streams = (0..count).map(|grid| Stream::new(grid * samples)).collect();
        self.grids = Grids::new(count);
        Ok(self)
    }

    /// Start again, as if nothing had been heard: after sound was lost,
    /// or the microphone was closed. What the models held is of before.
    pub fn forget(&mut self) {
        for stream in &mut self.streams {
            stream.forget();
        }
        self.grids.forget();
    }

    /// More of what the microphone hears, 16 kHz, one channel. A score
    /// for every step that became whole with it, oldest first: none for
    /// about the first two seconds after a start, then one every 80 ms.
    pub fn hear(&mut self, samples: &[i16]) -> Result<Vec<Score>, String> {
        let Some((first, later)) = self.streams.split_first_mut() else {
            return Ok(Vec::new());
        };
        // The later grids first: each one's newest is of a step whole
        // before the first grid's next.
        for (grid, stream) in later.iter_mut().enumerate() {
            for score in stream.hear(&self.models, samples)? {
                self.grids.later(grid + 1, score);
            }
        }
        Ok(first
            .hear(&self.models, samples)?
            .into_iter()
            .map(|score| self.grids.first(score))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(name: f32) -> Score {
        Score {
            name,
            stop: None,
            rescue: None,
        }
    }

    /// A step's score is the highest of the first grid's for it and each
    /// later grid's for the step before; the first step after a start has
    /// the first grid's alone, and a later grid's older scores are let go.
    #[test]
    fn a_step_takes_the_highest_of_its_grids() {
        let mut grids = Grids::new(2);
        // Step 0: the later grid has given nothing yet.
        assert_eq!(grids.first(score(0.2)).name, 0.2);
        // The later grid's step 0 falls between the first grid's 0 and 1.
        grids.later(1, score(0.96));
        assert_eq!(grids.first(score(0.1)).name, 0.96);
        // Its step 1 is lower than the first grid's step 2.
        grids.later(1, score(0.3));
        assert_eq!(grids.first(score(0.5)).name, 0.5);
        // Two later scores at once (a long block): each meets its own step.
        grids.later(1, score(0.7));
        grids.later(1, score(0.05));
        assert_eq!(grids.first(score(0.0)).name, 0.7);
        assert_eq!(grids.first(score(0.0)).name, 0.05);
        // After a forget, the first step is the first grid's alone again.
        grids.later(1, score(0.99));
        grids.forget();
        assert_eq!(grids.first(score(0.4)).name, 0.4);
        // One grid: every score as it came.
        let mut one = Grids::new(1);
        assert_eq!(one.first(score(0.3)).name, 0.3);
    }

    /// The stop and rescue scores are each the higher of the grids', and a
    /// grid without one leaves the other's.
    #[test]
    fn each_score_is_the_higher_of_the_grids() {
        let one = Score {
            name: 0.2,
            stop: Some(0.7),
            rescue: None,
        };
        let other = Score {
            name: 0.9,
            stop: Some(0.1),
            rescue: Some(0.4),
        };
        assert_eq!(
            higher(one, other),
            Score {
                name: 0.9,
                stop: Some(0.7),
                rescue: Some(0.4),
            }
        );
    }

    /// The repository's `stop.onnx` is the pinned file; any other bytes are
    /// refused with their SHA-256.
    #[test]
    fn only_the_pinned_stop_model_is_taken() {
        let model = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/stop/stop.onnx"),
        )
        .unwrap();
        assert_eq!(is_the_stop_model(&model), Ok(()));
        let why = is_the_stop_model(b"not a model").unwrap_err();
        assert!(
            why.contains("not the stop model's") && why.contains(STOP_SHA256),
            "{why}"
        );
    }

    /// A head (the rescue model) loads only with the SHA-256 given for it,
    /// and must score silence from 0 to 1: the repository's `stop.onnx`
    /// stands in, being of the same shape.
    #[test]
    fn a_head_loads_only_with_the_sha256_given_for_it() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/stop/stop.onnx");
        assert!(head(&path, STOP_SHA256, "the one given,").is_ok());
        let wrong = "0".repeat(64);
        let why = head(&path, &wrong, "the one given,").err().unwrap();
        assert!(
            why.contains(STOP_SHA256) && why.contains(&format!("not the one given, {wrong}")),
            "{why}"
        );
        let missing = head(Path::new("/nonexistent/rescue.onnx"), STOP_SHA256, "x")
            .err()
            .unwrap();
        assert!(
            missing.starts_with("/nonexistent/rescue.onnx: "),
            "{missing}"
        );
    }
}
