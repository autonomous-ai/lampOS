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
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Condvar, Mutex, mpsc},
};
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
    /// With [`Hearer::with_wake`]: the first grid's name score that wakes
    /// the later grids, and how many more steps they stay awake.
    wake: Option<f32>,
    awake_steps: usize,
    /// Since the program started: how many times the later grids woke, and
    /// the steps they were awake for (for the minute line, #694).
    pub wakes: u64,
    pub woken_steps: u64,
    /// With a wake bar, the later grids on a thread of their own, begun at
    /// the first block heard ([`Woken`]); and whether each block waits for
    /// them ([`Hearer::waiting`]).
    woken: Option<Woken>,
    wait: bool,
    /// Samples the first grid was given since the last forget.
    fed: u64,
}

/// The later grids woken by the first grid (`--wake`, #694), on a thread of
/// their own. Waking makes up to 20 embeddings a grid at once, about a
/// second of one core on lamp-75c9 for seven grids: on lamp-name's own
/// loop that second held back the first grid's next steps, and so its own
/// hearing (22 Oct 07:36–08:31, three of four played calls lost). Here the
/// first grid is heard as each block comes, as at 80 ms alone, and the
/// later grids' scores go into the step that is next when they are done.
struct Woken {
    queue: Arc<Queue>,
    done: mpsc::Receiver<(u64, Result<Vec<Later>, String>)>,
    /// Bumped by each forget: what was worked out from before it is let go.
    epoch: u64,
    /// Blocks sent and not yet answered.
    pending: usize,
    /// Times the queue overran and the later grids started afresh.
    overruns: u64,
}

/// Blocks the later grids may have waiting: a second and a half of sound.
/// More, and they have fallen behind it for good: what waits is let go, and
/// they start afresh on what comes next, while the first grid hears on as
/// ever (reviewer-two on #2759: the queue was unbounded, and a forget waited
/// behind it). A wake's catch-up takes about a second on lamp-75c9: at half
/// a second, 55 overruns came of 35 wakes in 18 minutes (22 Oct 10:3x), so
/// the woken grids added nothing to most wakes.
pub const MOST_QUEUED: usize = 75;

/// The later grids' work, between the program's loop and their thread.
#[derive(Default)]
struct Queue {
    work: Mutex<(VecDeque<Work>, bool)>,
    ready: Condvar,
}

impl Queue {
    /// One more block. Over [`MOST_QUEUED`], what waits is let go and the
    /// grids start afresh first (`restart`): the blocks let go, and whether
    /// it overran. Never waits on the thread.
    fn push(&self, work: Work, restart: impl FnOnce() -> Work) -> (usize, bool) {
        let Ok(mut queue) = self.work.lock() else {
            return (0, false);
        };
        let blocks = |queue: &VecDeque<Work>| {
            queue
                .iter()
                .filter(|work| matches!(work, Work::Hear(..)))
                .count()
        };
        let overran = blocks(&queue.0) >= MOST_QUEUED;
        let mut let_go = 0;
        if overran {
            let_go = blocks(&queue.0);
            queue.0.clear();
            queue.0.push_back(restart());
        }
        queue.0.push_back(work);
        self.ready.notify_one();
        (let_go, overran)
    }

    /// Start afresh: what waits is of before, and is let go, `restart`
    /// first in line. The blocks let go.
    fn restart(&self, restart: Work) -> usize {
        let Ok(mut queue) = self.work.lock() else {
            return 0;
        };
        let let_go = queue
            .0
            .iter()
            .filter(|work| matches!(work, Work::Hear(..)))
            .count();
        queue.0.clear();
        queue.0.push_back(restart);
        self.ready.notify_one();
        let_go
    }

    /// The next work, waiting for it; `None` once closed.
    fn take(&self) -> Option<Work> {
        let mut queue = self.work.lock().ok()?;
        loop {
            if let Some(work) = queue.0.pop_front() {
                return Some(work);
            }
            if queue.1 {
                return None;
            }
            queue = self.ready.wait(queue).ok()?;
        }
    }

    fn close(&self) {
        if let Ok(mut queue) = self.work.lock() {
            queue.1 = true;
            self.ready.notify_all();
        }
    }
}

impl Drop for Woken {
    fn drop(&mut self) {
        self.queue.close();
    }
}

/// A later grid's score: the grid (from 1), its number and the score.
type Later = (usize, usize, Score);

enum Work {
    /// A block: the sound, and whether awake.
    Hear(Vec<i16>, bool),
    /// Start afresh: the grids pass over `skip` samples more, so each keeps
    /// its place against the first grid, and their scores count from
    /// `from` (the first grid's step that sound begins). What they work out
    /// from then on is of `epoch`; anything of an earlier one, still being
    /// worked on as this was sent, is let go when it comes.
    Restart { from: u64, skip: usize, epoch: u64 },
}

impl Woken {
    fn start(models: Models, mut streams: Vec<Stream>) -> Result<Self, String> {
        let queue = Arc::new(Queue::default());
        let taken = queue.clone();
        let (give, done) = mpsc::channel();
        std::thread::Builder::new()
            .name("name-woken".into())
            .spawn(move || {
                let (mut from, mut epoch) = (0, 0);
                while let Some(asked) = taken.take() {
                    match asked {
                        Work::Restart {
                            from: step,
                            skip,
                            epoch: now,
                        } => {
                            from = step as usize;
                            epoch = now;
                            for stream in &mut streams {
                                stream.forget();
                                stream.skip += skip;
                            }
                        }
                        Work::Hear(samples, awake) => {
                            let mut heard = Ok(Vec::new());
                            for (grid, stream) in streams.iter_mut().enumerate() {
                                match (stream.hear(&models, &samples, awake), heard.as_mut()) {
                                    (Ok(scores), Ok(all)) => all.extend(
                                        scores
                                            .into_iter()
                                            .map(|(n, score)| (grid + 1, from + n, score)),
                                    ),
                                    (Err(why), _) => {
                                        heard = Err(why);
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            if give.send((epoch, heard)).is_err() {
                                return;
                            }
                        }
                    }
                }
            })
            .map_err(|why| format!("the woken grids' thread: {why}"))?;
        Ok(Self {
            queue,
            done,
            epoch: 0,
            pending: 0,
            overruns: 0,
        })
    }
}
/// A woken grid stays awake this many steps after the first grid's name
/// score was last over the wake bar: the name's own rise and fall.
pub const AWAKE_STEPS: usize = 8;

/// The models a step is worked out with, the same for every grid.
#[derive(Clone)]
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
    /// How many embeddings have been made, and the newest of them: `None`
    /// for one passed over asleep ([`Hearer::with_wake`]), made on waking
    /// from the frames kept for it.
    made: usize,
    embeddings: VecDeque<Option<Vec<f32>>>,
    /// The score of the newest embedding made, if it was scored: one woken
    /// scores the [`BACK`] positions before as well, once.
    scored: usize,
}

/// Embeddings kept for a score: [`CONTEXT`], and [`BACK`] - 1 more so a
/// grid woken by the first grid's score can score the steps just before.
const KEPT: usize = CONTEXT + BACK - 1;
/// A grid woken scores this many of its newest positions: on lamp-75c9's
/// recorded calls the first grid's score crossed the wake bar up to three
/// steps after a later grid's best (warm1 moved 10 ms: its 0.96 was the
/// fourth position back), and five leave a step's margin.
pub const BACK: usize = 5;

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
    /// For each later grid: the scores it gave that the first grid has not
    /// yet taken, by number.
    later: Vec<VecDeque<(u64, Score)>>,
    /// With [`Hearer::with_wake`]: the score a call is heard at (`--least`),
    /// a stop word's (`--stop-least`, [`Grids::stop_heard_at`]), and the
    /// first grid's newest name and stop scores by step. A later grid's
    /// score given late for a step where the first grid heard a call, or a
    /// stop word, leaves that out, so it is not heard a second time; the
    /// other channel is kept (reviewer-two on #2759).
    least: Option<f32>,
    stop_least: Option<f32>,
    recent: VecDeque<(u64, f32, Option<f32>)>,
}

/// The first grid's steps kept to judge a late score by, 3.84 s. A later
/// grid's score for a step without [`BACK`] + 1 kept on each side, 3.36 s
/// back, is of a moment gone and is let go whole, whatever it was
/// (reviewer-two on #2759: judged as unheard once the history had moved
/// past it, one call was heard twice). A woken grid stays within about
/// three seconds of the sound: at most a second and a half queued
/// ([`MOST_QUEUED`]), about a second of catch-up on a lamp, and [`BACK`]
/// steps.
const RECENT: usize = 48;

impl Grids {
    /// For `count` grids in all.
    pub fn new(count: usize) -> Self {
        Self {
            made: 0,
            later: (1..count).map(|_| VecDeque::new()).collect(),
            least: None,
            stop_least: None,
            recent: VecDeque::with_capacity(RECENT),
        }
    }

    /// A late score for a step the first grid heard a call at, at `least`
    /// or more within [`BACK`] + 1 steps of it, is let go.
    pub fn hearing_at(&mut self, least: f32) {
        self.least = Some(least);
    }

    /// The same for a stop word, at `least` (`--stop-least`).
    pub fn stop_heard_at(&mut self, least: f32) {
        self.stop_least = Some(least);
    }

    /// Whether the first grid's score was `heard` within [`BACK`] + 1 steps
    /// of `step`.
    fn heard_near(&self, step: u64, heard: impl Fn(f32, Option<f32>) -> bool) -> bool {
        let reach = BACK as u64 + 1;
        self.recent
            .iter()
            .any(|(at, name, stop)| at.abs_diff(step) <= reach && heard(*name, *stop))
    }

    /// A later grid's score given late for `step`: without what the first
    /// grid heard there already, or nothing, if the steps around it are no
    /// longer kept ([`RECENT`]).
    fn late(&self, step: u64, mut score: Score) -> Option<Score> {
        if self.made.saturating_sub(step) + BACK as u64 + 1 > RECENT as u64 {
            return None;
        }
        if let Some(least) = self.least
            && self.heard_near(step, |name, _| name >= least)
        {
            // The rescue is a second name model: its call is the same one.
            score.name = 0.0;
            score.rescue = None;
        }
        if let Some(least) = self.stop_least
            && self.heard_near(step, |_, stop| stop.is_some_and(|stop| stop >= least))
        {
            score.stop = None;
        }
        Some(score)
    }

    /// Later grid `grid` (from 1) gave `score`, its `number`-th.
    pub fn later(&mut self, grid: usize, number: u64, score: Score) {
        self.later[grid - 1].push_back((number, score));
    }

    /// The first grid gave `score`: the step's score. A later grid's score
    /// for the step before is in it, and so is any older one it gave late,
    /// woken ([`Hearer::with_wake`]): heard a few steps late, not lost.
    pub fn first(&mut self, score: Score) -> Score {
        let step = self.made;
        self.made += 1;
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back((step, score.name, score.stop));
        let mut best = score;
        for grid in 0..self.later.len() {
            while let Some((at, later)) = self.later[grid]
                .front()
                .copied()
                .filter(|(at, _)| *at < step)
            {
                // On time, the one for the step before; late, only what
                // the first grid didn't hear, and never once gone by.
                let taken = if at + 1 == step {
                    Some(later)
                } else {
                    self.late(at + 1, later)
                };
                if let Some(taken) = taken {
                    best = higher(best, taken);
                }
                self.later[grid].pop_front();
            }
        }
        best
    }

    pub fn forget(&mut self) {
        let (least, stop_least) = (self.least, self.stop_least);
        *self = Self::new(self.later.len() + 1);
        (self.least, self.stop_least) = (least, stop_least);
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
            embeddings: VecDeque::with_capacity(KEPT),
            scored: 0,
        }
    }

    fn forget(&mut self) {
        *self = Self::new(self.offset);
    }

    /// Scores for the steps that became whole, each with its number (the
    /// first after a start is 0). `awake`: the embeddings and scores are
    /// made; asleep, only the frames, the newest [`KEPT`] embeddings' worth
    /// kept, so waking makes what it passed over and scores the [`BACK`]
    /// newest positions it hadn't. The first grid is always awake.
    fn hear(
        &mut self,
        models: &Models,
        samples: &[i16],
        awake: bool,
    ) -> Result<Vec<(usize, Score)>, String> {
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
                if self.embeddings.len() == KEPT {
                    self.embeddings.pop_front();
                }
                self.embeddings.push_back(None);
                self.made += 1;
                // Frames before the oldest kept embedding's first are done
                // with: only one embedding's own when it is always made.
                let keep = if awake && self.scored + 1 >= self.made {
                    1
                } else {
                    KEPT
                };
                while self.frames_gone < FRAMES_A_STEP * self.made.saturating_sub(keep) {
                    self.frames.pop_front();
                    self.frames_gone += 1;
                }
                if awake {
                    scores.extend(self.score(models)?);
                }
            }
        }
        // Woken with no new step: what it passed over, now.
        if awake && self.scored < self.made {
            scores.extend(self.score(models)?);
        }
        Ok(scores)
    }

    /// Make the kept embeddings passed over, and score each position from
    /// the newest back that has its [`CONTEXT`] and wasn't scored, at most
    /// [`BACK`]: one, when it was awake for the one before.
    fn score(&mut self, models: &Models) -> Result<Vec<(usize, Score)>, String> {
        let first_kept = self.made - self.embeddings.len();
        for (at, embedding) in self.embeddings.iter_mut().enumerate() {
            if embedding.is_some() {
                continue;
            }
            let from = FRAMES_A_STEP * (first_kept + at) - self.frames_gone;
            let flat: Vec<f32> = self
                .frames
                .range(from..from + FRAMES)
                .flat_map(|frame| frame.iter().copied())
                .collect();
            let made = run(&models.embedding, &[1, FRAMES, BINS, 1], &flat, FILES[1])?;
            if made.len() != EMBEDDING {
                return Err(format!(
                    "{}: {} numbers, not {EMBEDDING}",
                    FILES[1],
                    made.len()
                ));
            }
            *embedding = Some(made);
        }
        // Position p (its newest embedding the p-th) is scored once its
        // CONTEXT embeddings are made; its score is number p - CONTEXT.
        let from = (self.scored + 1)
            .max(CONTEXT)
            .max(self.made.saturating_sub(BACK) + 1)
            .max(first_kept + CONTEXT);
        let mut scores = Vec::new();
        for position in from..=self.made {
            let end = position - first_kept;
            let flat: Vec<f32> = self
                .embeddings
                .range(end - CONTEXT..end)
                .flat_map(|embedding| embedding.iter().flatten().copied())
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
            scores.push((
                position - CONTEXT,
                Score {
                    name: *score.first().ok_or("name.onnx: no score")?,
                    stop,
                    rescue,
                },
            ));
        }
        self.scored = self.scored.max(self.made);
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
            wake: None,
            awake_steps: 0,
            wakes: 0,
            woken_steps: 0,
            woken: None,
            wait: false,
            fed: 0,
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

    /// The later grids of [`Self::with_step_ms`] run their models only
    /// while the first grid's name score is over `bar`, and for
    /// [`AWAKE_STEPS`] after, scoring the [`BACK`] steps before they woke
    /// as they wake (#694), `least` being the score a call is heard at, so
    /// a late score of a call already heard is let go: in quiet and talk with no name the work is
    /// about one grid's. They run on a thread of their own ([`Woken`]), so
    /// the first grid's steps never wait for them. A name a later grid hears
    /// best is heard when they are done: up to [`BACK`] steps (0.4 s) after
    /// it was said, and on a lamp the second or so a wake's catch-up takes.
    /// Needs more than one grid.
    pub fn with_wake(mut self, bar: f32, least: f32) -> Result<Self, String> {
        if self.streams.len() < 2 {
            return Err("a wake bar needs a step under 80 ms".to_owned());
        }
        if !(bar > 0.0 && bar < 1.0) {
            return Err(format!("a wake bar of {bar}: over 0 and under 1"));
        }
        self.wake = Some(bar);
        self.grids.hearing_at(least);
        Ok(self)
    }

    /// Start again, as if nothing had been heard: after sound was lost,
    /// or the microphone was closed. What the models held is of before.
    pub fn forget(&mut self) {
        for stream in &mut self.streams {
            stream.forget();
        }
        self.grids.forget();
        self.awake_steps = 0;
        self.fed = 0;
        if let Some(woken) = self.woken.as_mut() {
            woken.epoch += 1;
            woken.pending -= woken.queue.restart(Work::Restart {
                from: 0,
                skip: 0,
                epoch: woken.epoch,
            });
        }
    }

    /// With a wake bar and the stop model, a stop word the first grid heard
    /// at `least` (`--stop-least`) is not heard again from a later grid's
    /// late score ([`Grids::stop_heard_at`]).
    pub fn stop_heard_at(mut self, least: f32) -> Self {
        self.grids.stop_heard_at(least);
        self
    }

    /// Times the woken grids fell [`MOST_QUEUED`] blocks behind and started
    /// afresh, since the program started.
    pub fn overruns(&self) -> u64 {
        self.woken.as_ref().map_or(0, |woken| woken.overruns)
    }

    /// With a wake bar, each block waits for the later grids' work on it, so
    /// the scores are as they would be on one thread: for `check` and the
    /// tests. The program never waits.
    pub fn waiting(mut self, on: bool) -> Self {
        self.wait = on;
        self
    }

    /// The later grids' scores worked out so far, into [`Grids`]: all that
    /// were asked for, when waiting.
    fn take_woken(&mut self) -> Result<(), String> {
        let Some(woken) = self.woken.as_mut() else {
            return Ok(());
        };
        while woken.pending > 0 {
            let answer = if self.wait {
                woken
                    .done
                    .recv()
                    .map_err(|_| "the woken grids' thread ended".to_owned())?
            } else {
                match woken.done.try_recv() {
                    Ok(answer) => answer,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err("the woken grids' thread ended".to_owned());
                    }
                }
            };
            woken.pending -= 1;
            let (epoch, heard) = answer;
            for (grid, number, score) in heard? {
                if epoch == woken.epoch {
                    self.grids.later(grid, number as u64, score);
                }
            }
        }
        Ok(())
    }

    /// More of what the microphone hears, 16 kHz, one channel. A score
    /// for every step that became whole with it, oldest first: none for
    /// about the first two seconds after a start, then one every 80 ms.
    pub fn hear(&mut self, samples: &[i16]) -> Result<Vec<Score>, String> {
        let awake = self.wake.is_none() || self.awake_steps > 0;
        if self.wake.is_some() && self.streams.len() > 1 && self.woken.is_none() {
            let later = self.streams.split_off(1);
            self.woken = Some(Woken::start(self.models.clone(), later)?);
        }
        if let Some(woken) = self.woken.as_mut() {
            // The later grids on their own thread; the first is heard now.
            // Overrun, the later grids start at the first grid's next whole
            // step from this block on.
            // What was being worked on then is of before, and let go.
            let (fed, next) = (self.fed, woken.epoch + 1);
            let restart = || {
                let from = fed.div_ceil(STEP as u64);
                Work::Restart {
                    from,
                    skip: (from * STEP as u64 - fed) as usize,
                    epoch: next,
                }
            };
            let (let_go, overran) = woken
                .queue
                .push(Work::Hear(samples.to_vec(), awake), restart);
            woken.pending = woken.pending - let_go + 1;
            if overran {
                woken.overruns += 1;
                woken.epoch = next;
            }
        }
        let Some((first, later)) = self.streams.split_first_mut() else {
            return Ok(Vec::new());
        };
        // The later grids first: each one's newest is of a step whole
        // before the first grid's next.
        for (grid, stream) in later.iter_mut().enumerate() {
            for (number, score) in stream.hear(&self.models, samples, awake)? {
                self.grids.later(grid + 1, number as u64, score);
            }
        }
        let firsts = first.hear(&self.models, samples, true)?;
        self.fed += samples.len() as u64;
        self.take_woken()?;
        let scores: Vec<Score> = firsts
            .into_iter()
            .map(|(_, score)| {
                if let Some(bar) = self.wake {
                    let was = self.awake_steps > 0;
                    self.awake_steps = if score.name > bar {
                        AWAKE_STEPS
                    } else {
                        self.awake_steps.saturating_sub(1)
                    };
                    self.wakes += u64::from(!was && self.awake_steps > 0);
                    self.woken_steps += u64::from(was || self.awake_steps > 0);
                }
                self.grids.first(score)
            })
            .collect();
        Ok(scores)
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
    /// the first grid's alone, and a later grid's score given late goes
    /// into the next step.
    #[test]
    fn a_step_takes_the_highest_of_its_grids() {
        let mut grids = Grids::new(2);
        // Step 0: the later grid has given nothing yet.
        assert_eq!(grids.first(score(0.2)).name, 0.2);
        // The later grid's step 0 falls between the first grid's 0 and 1.
        grids.later(1, 0, score(0.96));
        assert_eq!(grids.first(score(0.1)).name, 0.96);
        // Its step 1 is lower than the first grid's step 2.
        grids.later(1, 1, score(0.3));
        assert_eq!(grids.first(score(0.5)).name, 0.5);
        // Two later scores at once (a long block): each meets its own step.
        grids.later(1, 2, score(0.7));
        grids.later(1, 3, score(0.05));
        assert_eq!(grids.first(score(0.0)).name, 0.7);
        assert_eq!(grids.first(score(0.0)).name, 0.05);
        // Given late, woken (#694): an older score goes into the next step
        // rather than being lost.
        grids.later(1, 3, score(0.8));
        grids.later(1, 5, score(0.2));
        assert_eq!(grids.first(score(0.1)).name, 0.8);
        assert_eq!(grids.first(score(0.1)).name, 0.2);
        // Woken, a late score for a step the first grid heard a call at is
        // let go; one for a call it missed is taken, once (reviewer-two on
        // #2759: one call heard twice, its fine score a second late).
        let mut woken = Grids::new(2);
        woken.hearing_at(0.9);
        assert_eq!(woken.first(score(0.95)).name, 0.95);
        for _ in 1..15 {
            assert_eq!(woken.first(score(0.0)).name, 0.0);
        }
        woken.later(1, 0, score(0.97));
        assert_eq!(woken.first(score(0.0)).name, 0.0, "heard already");
        for _ in 16..30 {
            woken.first(score(0.02));
        }
        woken.later(1, 20, score(0.97));
        assert_eq!(
            woken.first(score(0.02)).name,
            0.97,
            "missed by the first grid"
        );
        assert_eq!(woken.first(score(0.02)).name, 0.02);
        // Older than the first grid's kept steps: let go whole, unheard or
        // not (reviewer-two's 09:37).
        for _ in 31..90 {
            woken.first(score(0.02));
        }
        woken.later(1, 41, score(0.97));
        assert_eq!(woken.first(score(0.02)).name, 0.02, "gone by");
        // A stop word the first grid heard is let go too; the late score's
        // name, for a call it missed, is kept, and the other way round.
        let stop = |name, stop| Score {
            name,
            stop: Some(stop),
            rescue: None,
        };
        let mut both = Grids::new(2);
        both.hearing_at(0.9);
        both.stop_heard_at(0.9);
        assert_eq!(both.first(stop(0.95, 0.1)), stop(0.95, 0.1));
        for _ in 1..10 {
            both.first(stop(0.0, 0.0));
        }
        both.later(1, 0, stop(0.2, 0.99));
        assert_eq!(
            both.first(stop(0.0, 0.0)),
            stop(0.0, 0.99),
            "the name let go, the stop kept"
        );
        let mut stopped = Grids::new(2);
        stopped.hearing_at(0.9);
        stopped.stop_heard_at(0.9);
        stopped.first(stop(0.1, 0.95));
        for _ in 1..10 {
            stopped.first(stop(0.0, 0.0));
        }
        stopped.later(1, 0, stop(0.97, 0.99));
        assert_eq!(
            stopped.first(stop(0.0, 0.0)),
            stop(0.97, 0.0),
            "the stop let go, the name kept"
        );
        // A forget keeps the bars.
        woken.forget();
        assert_eq!(woken.least, Some(0.9));
        // After a forget, the first step is the first grid's alone again.
        grids.later(1, 0, score(0.99));
        grids.forget();
        assert_eq!(grids.first(score(0.4)).name, 0.4);
        // One grid: every score as it came.
        let mut one = Grids::new(1);
        assert_eq!(one.first(score(0.3)).name, 0.3);
    }

    /// Reviewer-two on #2759: the woken grids' queue holds at most
    /// [`MOST_QUEUED`] blocks. One more lets go what waits, and the grids
    /// start afresh at the first grid's next step before the new block; a
    /// forget does too, ahead of anything that waited. Pushing never waits
    /// on the thread.
    #[test]
    fn the_woken_queue_is_bounded_and_a_restart_goes_first() {
        let queue = Queue::default();
        let block = |n: i16| Work::Hear(vec![n; 320], true);
        let restart = || Work::Restart {
            from: 7,
            skip: 640,
            epoch: 3,
        };
        let is = |work: Option<Work>, n: i16| matches!(work, Some(Work::Hear(sound, _)) if sound[0] == n);
        for n in 0..MOST_QUEUED as i16 {
            assert_eq!(queue.push(block(n), restart), (0, false));
        }
        assert_eq!(queue.push(block(99), restart), (MOST_QUEUED, true));
        assert!(matches!(
            queue.take(),
            Some(Work::Restart {
                from: 7,
                skip: 640,
                epoch: 3
            })
        ));
        assert!(is(queue.take(), 99));
        // A forget's restart goes ahead of what waited, which is let go.
        queue.push(block(1), restart);
        queue.push(block(2), restart);
        assert_eq!(
            queue.restart(Work::Restart {
                from: 0,
                skip: 0,
                epoch: 4
            }),
            2
        );
        assert!(matches!(
            queue.take(),
            Some(Work::Restart {
                from: 0,
                skip: 0,
                epoch: 4
            })
        ));
        queue.push(block(3), restart);
        assert!(is(queue.take(), 3));
        // Closed and empty: the thread ends.
        queue.close();
        assert!(queue.take().is_none());
    }

    /// The first builder on #694: a wake's catch-up of 1.2 s, the blocks
    /// coming every 20 ms meanwhile with nothing taken, never overruns; the
    /// worker then takes them all, in order.
    #[test]
    fn a_wakes_catch_up_of_a_second_and_a_fifth_does_not_overrun() {
        let queue = Queue::default();
        let restart = || Work::Restart {
            from: 0,
            skip: 0,
            epoch: 1,
        };
        let blocks = 1_200 / 20;
        for n in 0..blocks {
            assert_eq!(
                queue.push(Work::Hear(vec![n; 320], true), restart),
                (0, false),
                "{n}"
            );
        }
        for n in 0..blocks {
            assert!(matches!(queue.take(), Some(Work::Hear(sound, _)) if sound[0] == n));
        }
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
