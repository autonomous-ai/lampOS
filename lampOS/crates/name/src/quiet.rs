//! The models rest while the room is quiet (`--quiet-below`, off unless
//! given; #705, the lamp's heat).
//!
//! Run every 80 ms all day, the three models cost lamp-4ace 750 steps of
//! 9.3 ms a minute, about 12% of one core, with nobody speaking. A score is
//! of the newest [`WINDOW`] samples (2.03 s) and of nothing before them
//! (`hear.rs`), so a score whose 2.03 s held no sample at the bar or above
//! is of a quiet room, and the name cannot be in it: it is not worked out.
//! The newest 2.03 s are kept, and at the first sound the models start
//! afresh on them, from the step where the first score that hears that
//! sound begins: that score, and every one after it, is the number the
//! models would have given had they never rested. The cost is that start,
//! once a sound: at most 25 steps' first model and 16 of the second, under
//! a quarter of a second on lamp-4ace, before the next live step.
//!
//! Scores fall where they always did: on the steps counted from the last
//! [`Quiet::forget`] (a start, a gap, lost sound), never from where the
//! models last woke.
//!
//! A start or a forget takes [`LEAD`] samples of silence as heard before it
//! (#6114): a score needs 2.03 s of sound, so from nothing a "Lamp" said in
//! the first second and a half after the microphone's sound came back had
//! no score of its own (0.0000 against 0.95 at 2 s, on a clip; on 52e6, the
//! first "Lamp, …" after a restart with `heard=0`). With the silence before
//! it, the first 80 ms of sound already gives a score. Silence, not a guess
//! at the room: digital zeros never sound like the name. With no bar the
//! models' state after it is worked out once, at load
//! ([`Hearer::with_lead`]), and each start or forget copies it back
//! ([`Models::lead`]): no model runs on the loop for it (#6462; heard at
//! every forget, 1.95 s of zeros cost 0.3 to 0.7 s of lamp-4ace, and a
//! lost block forgets). With a bar it is kept, and heard with the sound.
use crate::hear::{Hearer, STEP, Score, WINDOW};
use std::collections::VecDeque;

/// What gives the scores: [`Hearer`], or a test's stand-in.
pub trait Models {
    /// More sound; a score for each step that became whole with it.
    fn hear(&mut self, samples: &[i16]) -> Result<Vec<Score>, String>;
    /// Start again, as if nothing had been heard.
    fn forget(&mut self);
    /// Start again, as if `samples` of silence (under a window) had just
    /// been heard: here by hearing them.
    fn lead(&mut self, samples: usize) -> Result<(), String> {
        self.forget();
        self.hear(&vec![0; samples]).map(|_| ())
    }
}

impl Models for Hearer {
    fn hear(&mut self, samples: &[i16]) -> Result<Vec<Score>, String> {
        Hearer::hear(self, samples)
    }

    fn forget(&mut self) {
        Hearer::forget(self)
    }

    /// The state after the silence, worked out once at load
    /// ([`Hearer::with_lead`]), copied back: no model run (#6462). A
    /// hearer without that lead hears it.
    fn lead(&mut self, samples: usize) -> Result<(), String> {
        if samples > 0 && self.led() == samples {
            self.forget_to_lead();
            Ok(())
        } else {
            self.forget();
            self.hear(&vec![0; samples]).map(|_| ())
        }
    }
}

/// The silence taken as heard before a start or a forget: a window less a
/// step, so the first step of sound is the first score's.
pub const LEAD: usize = WINDOW - STEP;

/// What is said of no sound at all, in dBFS.
pub const SILENCE_DB: f32 = -96.0;
/// The bars `--quiet-below` takes, in dBFS: above the 16-bit floor, and
/// well under any voice: a bar of -20 would rest through speech across
/// the room, so a mistyped one is refused at start, not made deaf.
pub const BARS_DB: std::ops::RangeInclusive<f32> = -90.0..=-30.0;
const FULL: f32 = 32_768.0;

/// A sample's size in dBFS, from [`SILENCE_DB`] to 0.
pub fn dbfs(peak: u16) -> f32 {
    if peak == 0 {
        SILENCE_DB
    } else {
        (20.0 * (f32::from(peak) / FULL).log10()).clamp(SILENCE_DB, 0.0)
    }
}

/// The smallest sample that is sound, for a bar in dBFS.
pub fn bar_of(db: f32) -> u16 {
    (FULL * 10f32.powf(db / 20.0))
        .ceil()
        .min(f32::from(u16::MAX)) as u16
}

fn loudest(samples: &[i16]) -> u16 {
    samples
        .iter()
        .map(|sample| sample.unsigned_abs())
        .max()
        .unwrap_or(0)
}

/// How many steps have become whole by `fed` samples since the start.
fn steps_by(fed: usize) -> usize {
    if fed < WINDOW {
        0
    } else {
        (fed - WINDOW) / STEP + 1
    }
}

/// What one block gave.
#[derive(Debug, Default, PartialEq)]
pub struct Heard {
    /// The scores, oldest first, each of the step `Quiet::steps` counts
    /// back to: the last one is of the newest whole step. That holds for
    /// blocks of at most a step (`STEP`, 80 ms; the voice sends 20 ms): a
    /// longer block that rests may end past its last score.
    pub scores: Vec<Score>,
    /// The models went to rest with this block: a hearing still open is
    /// over.
    pub rested: bool,
}

/// The models, rested while the newest 2.03 s are quiet.
pub struct Quiet<M> {
    models: M,
    /// The smallest sample that is sound; `None` keeps the models running.
    bar: Option<u16>,
    /// The newest samples, enough to start the models afresh from.
    kept: VecDeque<i16>,
    /// Samples since the last forget, and where the newest sound ended.
    fed: usize,
    sound_until: Option<usize>,
    running: bool,
    /// Steps not worked out, since the program started.
    pub rested_steps: u64,
    /// The models are still to hear [`LEAD`]'s silence (only with no bar:
    /// with one it is kept, and heard from there at the first sound).
    lead_due: bool,
}

impl<M: Models> Quiet<M> {
    /// With `quiet_below` in dBFS, a block whose loudest sample is under it
    /// is quiet; without it the models always run.
    pub fn new(models: M, quiet_below: Option<f32>) -> Self {
        let bar = quiet_below.map(bar_of);
        let mut quiet = Self {
            models,
            bar,
            kept: VecDeque::with_capacity(WINDOW + 2 * STEP),
            fed: 0,
            sound_until: None,
            running: bar.is_none(),
            rested_steps: 0,
            lead_due: false,
        };
        quiet.lead();
        quiet
    }

    /// [`LEAD`]'s silence, as heard: counted, kept to start the models
    /// from with a bar, and without one the models are led
    /// ([`Models::lead`]) at the next block (so a start or a forget does no
    /// work of its own, and a hearer led at load none at all).
    fn lead(&mut self) {
        self.fed = LEAD;
        if self.bar.is_some() {
            self.kept.extend(std::iter::repeat_n(0, LEAD));
        } else {
            self.lead_due = true;
        }
    }

    /// The models it rests.
    pub fn models(&self) -> &M {
        &self.models
    }

    /// Steps whole since the last forget: the last score given is of this
    /// one, counted from 1.
    pub fn steps(&self) -> usize {
        steps_by(self.fed)
    }

    /// Start again, as if nothing had been heard.
    pub fn forget(&mut self) {
        self.models.forget();
        self.kept.clear();
        self.sound_until = None;
        self.running = self.bar.is_none();
        self.lead_due = false;
        self.lead();
    }

    /// One block of what the microphone hears.
    pub fn hear(&mut self, block: &[i16]) -> Result<Heard, String> {
        let Some(bar) = self.bar else {
            if self.lead_due {
                // A window less a step: never a score of its own. A copy
                // of the state after it, for a hearer led at load.
                self.models.lead(LEAD)?;
                self.lead_due = false;
            }
            self.fed += block.len();
            return Ok(Heard {
                scores: self.models.hear(block)?,
                rested: false,
            });
        };
        let start = self.fed;
        self.fed += block.len();
        let sound = loudest(block) >= bar;
        if sound {
            self.sound_until = Some(self.fed);
        }
        self.kept.extend(block);
        let keep = WINDOW + STEP.max(block.len());
        if self.kept.len() > keep {
            self.kept.drain(..self.kept.len() - keep);
        }
        if !self.running && !sound {
            self.rested_steps += (steps_by(self.fed) - steps_by(start)) as u64;
            return Ok(Heard::default());
        }
        let mut heard = Heard::default();
        if self.running {
            // The steps still to work out end where a window last holds
            // the sound: those whose 2.03 s begin after it are quiet.
            let until = self.sound_until.map_or(0, |until| until + WINDOW - 1);
            if until > start {
                let to = (until - start).min(block.len());
                heard.scores = self.models.hear(&block[..to])?;
            }
            if self.fed >= until {
                self.rested_steps += (steps_by(self.fed) - steps_by(until.max(start))) as u64;
                self.running = false;
                self.models.forget();
                heard.rested = true;
            }
            return Ok(heard);
        }
        // Sound after a rest: the first step whose 2.03 s hold this block
        // ends after the block began, and its window begins on a step.
        let first = if start < WINDOW {
            WINDOW
        } else {
            WINDOW + ((start - WINDOW) / STEP + 1) * STEP
        };
        let from = first - WINDOW;
        let kept_from = self.fed - self.kept.len();
        self.models.forget();
        self.running = true;
        let kept = self.kept.make_contiguous();
        heard.scores = self.models.hear(&kept[from - kept_from..])?;
        Ok(heard)
    }
}

/// Each step's loudest sample, for the minute line: what the room's quiet
/// and its calls measure, to set the bar by.
#[derive(Default)]
pub struct Loudness {
    step: u16,
    filled: usize,
    steps: Vec<u16>,
}

impl Loudness {
    pub fn add(&mut self, block: &[i16]) {
        for chunk in block.chunks(STEP) {
            let room = STEP - self.filled;
            let (now, rest) = chunk.split_at(room.min(chunk.len()));
            self.step = self.step.max(loudest(now));
            self.filled += now.len();
            if self.filled == STEP {
                self.steps.push(self.step);
                (self.step, self.filled) = (loudest(rest), rest.len());
            }
        }
    }

    /// The median and the 99th percentile of the steps' loudest samples,
    /// in dBFS, since the last call; none with no whole step.
    pub fn take(&mut self) -> Option<(f32, f32)> {
        let mut steps = std::mem::take(&mut self.steps);
        if steps.is_empty() {
            return None;
        }
        steps.sort_unstable();
        let at = |share: f32| dbfs(steps[((steps.len() - 1) as f32 * share).round() as usize]);
        Some((at(0.5), at(0.99)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The models as `hear.rs` cuts them: a score once `WINDOW` samples
    /// have come since a forget, then one every `STEP`, each a checksum of
    /// exactly its own 2.03 s, so a score of other samples is another
    /// number. It counts the scores worked out.
    #[derive(Default)]
    struct Sums {
        samples: Vec<i16>,
        worked: usize,
        /// Leads taken as a copy, and the longest sound given to `hear`.
        leads: usize,
        longest_heard: usize,
    }

    impl Models for Sums {
        fn hear(&mut self, samples: &[i16]) -> Result<Vec<Score>, String> {
            let before = steps_by(self.samples.len());
            self.longest_heard = self.longest_heard.max(samples.len());
            self.samples.extend(samples);
            let scores: Vec<Score> = (before..steps_by(self.samples.len()))
                .map(|step| {
                    let end = WINDOW + step * STEP;
                    let sum = self.samples[end - WINDOW..end].iter().enumerate().fold(
                        0u64,
                        |sum, (at, sample)| {
                            sum.wrapping_mul(31)
                                .wrapping_add(at as u64 ^ u64::from(sample.unsigned_abs()))
                        },
                    );
                    Score {
                        name: (sum % 1_000_003) as f32 / 1_000_003.0,
                        stop: None,
                        rescue: None,
                        late: 0,
                    }
                })
                .collect();
            self.worked += scores.len();
            Ok(scores)
        }

        fn forget(&mut self) {
            self.samples.clear();
        }

        /// As a hearer led at load: the state after the silence, never
        /// heard again.
        fn lead(&mut self, samples: usize) -> Result<(), String> {
            self.leads += 1;
            self.samples = vec![0; samples];
            Ok(())
        }
    }

    const BAR_DB: f32 = -50.0;

    /// 20 ms blocks as the voice sends them: quiet is a room's hiss under
    /// the bar, sound is well over it, both never the same twice.
    fn room(stretches: &[(bool, usize)]) -> Vec<(bool, Vec<i16>)> {
        let mut seed = 7u32;
        let mut blocks = Vec::new();
        for &(sound, count) in stretches {
            for _ in 0..count {
                let block = (0..320)
                    .map(|_| {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        let wobble = (seed >> 16) as i16 % 60;
                        if sound { wobble * 50 } else { wobble }
                    })
                    .collect();
                blocks.push((sound, block));
            }
        }
        blocks
    }

    /// Quiet, a call, quiet longer than 2.03 s, a short sound, quiet
    /// shorter than 2.03 s, sound, and quiet to the end.
    const STRETCHES: [(bool, usize); 7] = [
        (false, 160),
        (true, 50),
        (false, 200),
        (true, 3),
        (false, 60),
        (true, 25),
        (false, 150),
    ];

    /// Every score each way, by the step it is of.
    fn scores(quiet_below: Option<f32>, blocks: &[(bool, Vec<i16>)]) -> (Vec<(usize, f32)>, Sums) {
        let mut quiet = Quiet::new(Sums::default(), quiet_below);
        let mut all = Vec::new();
        for (_, block) in blocks {
            let heard = quiet.hear(block).unwrap();
            let last = quiet.steps();
            let count = heard.scores.len();
            all.extend(
                heard
                    .scores
                    .into_iter()
                    .enumerate()
                    .map(|(at, score)| (last + 1 + at - count, score.name)),
            );
        }
        (all, quiet.models)
    }

    /// (a) Every score the rested models give is the one they give never
    /// resting, on the same step.
    #[test]
    fn a_score_after_a_rest_is_the_score_without_one() {
        let blocks = room(&STRETCHES);
        let (always, _) = scores(None, &blocks);
        let (rested, _) = scores(Some(BAR_DB), &blocks);
        assert_eq!(always.len(), steps_by(LEAD + blocks.len() * 320));
        assert!(!rested.is_empty());
        for (step, score) in &rested {
            assert_eq!(always[step - 1], (*step, *score), "step {step}");
        }
    }

    /// (b) A step is worked out exactly when its 2.03 s held a block of
    /// sound, and the rest are counted as rested.
    #[test]
    fn exactly_the_steps_whose_window_held_sound_are_worked_out() {
        let blocks = room(&STRETCHES);
        // Counted from the start's silence ([`LEAD`]).
        let needed: Vec<usize> = (1..=steps_by(LEAD + blocks.len() * 320))
            .filter(|step| {
                let end = WINDOW + (step - 1) * STEP;
                blocks.iter().enumerate().any(|(at, (sound, _))| {
                    *sound && LEAD + at * 320 < end && LEAD + (at + 1) * 320 > end - WINDOW
                })
            })
            .collect();
        let (rested, models) = scores(Some(BAR_DB), &blocks);
        let worked: Vec<usize> = rested.iter().map(|(step, _)| *step).collect();
        assert_eq!(worked, needed);
        assert_eq!(models.worked, needed.len());
        let mut quiet = Quiet::new(Sums::default(), Some(BAR_DB));
        for (_, block) in &blocks {
            quiet.hear(block).unwrap();
        }
        assert!(quiet.rested_steps > 40, "{}", quiet.rested_steps);
        assert_eq!(
            quiet.rested_steps as usize + needed.len(),
            steps_by(LEAD + blocks.len() * 320)
        );
    }

    /// (c) The models rest once for each quiet longer than 2.03 s, when the
    /// last window holding the sound is over, so the program can end a
    /// hearing still open; a shorter quiet keeps them running.
    #[test]
    fn the_models_say_when_they_rest() {
        let blocks = room(&STRETCHES);
        let mut quiet = Quiet::new(Sums::default(), Some(BAR_DB));
        let mut rested_at = Vec::new();
        for (at, (_, block)) in blocks.iter().enumerate() {
            if quiet.hear(block).unwrap().rested {
                rested_at.push(at);
            }
        }
        // Each rests on the block holding the end of the sound's last
        // window: the sound's end plus 2.03 s, less a sample.
        let rest_after = |end_block: usize| (end_block * 320 + WINDOW - 1) / 320;
        assert_eq!(rested_at, [rest_after(210), rest_after(498)]);
    }

    /// With no bar the models are never rested, and a forget starts the
    /// steps again from nothing, as `Hearer::forget` does.
    #[test]
    fn without_a_bar_every_step_is_worked_out_and_a_forget_starts_again() {
        let blocks = room(&[(false, 300)]);
        let (always, models) = scores(None, &blocks);
        assert_eq!(always.len(), steps_by(LEAD + 300 * 320));
        assert_eq!(models.worked, always.len());
        let mut quiet = Quiet::new(Sums::default(), Some(BAR_DB));
        for (_, block) in &blocks[..200] {
            quiet.hear(block).unwrap();
        }
        quiet.forget();
        assert_eq!(quiet.steps(), 0);
        // Sound 1 s after the forget: the first score is of the first step
        // that ends after the sound began, counted from the forget's
        // silence ([`LEAD`]), as from a start.
        let mut first = None;
        for (at, (_, block)) in room(&[(false, 50), (true, 100)]).iter().enumerate() {
            if first.is_none() && !quiet.hear(block).unwrap().scores.is_empty() {
                first = Some((at, quiet.steps()));
            }
        }
        let steps_before = (LEAD + 50 * 320 - WINDOW) / STEP + 1;
        let end = WINDOW + steps_before * STEP;
        assert_eq!(
            first,
            Some(((end - LEAD).div_ceil(320) - 1, steps_before + 1))
        );
    }

    /// #6114: from nothing, a score needed 2.03 s of sound, so a "Lamp"
    /// said in the first second and a half after a start or a forget (the
    /// microphone's sound coming back after a restart) had no score of its
    /// own. With the silence taken as heard before it, the first step of
    /// sound gives one, with or without a bar, and it is the score of that
    /// silence and the sound.
    #[test]
    fn a_name_said_just_after_a_start_or_a_forget_is_scored() {
        let sound = room(&[(true, 20)]);
        for bar in [None, Some(BAR_DB)] {
            for forgot in [false, true] {
                let mut quiet = Quiet::new(Sums::default(), bar);
                if forgot {
                    for (_, block) in &room(&[(true, 200)]) {
                        quiet.hear(block).unwrap();
                    }
                    quiet.forget();
                }
                let mut first = None;
                for (at, (_, block)) in sound.iter().enumerate() {
                    let heard = quiet.hear(block).unwrap();
                    if first.is_none() && !heard.scores.is_empty() {
                        first = Some((at, quiet.steps(), heard.scores[0].name));
                    }
                }
                // The step's 1280 samples are four 20 ms blocks.
                let mut alone = Sums::default();
                let mut heard = vec![0i16; LEAD];
                heard.extend(
                    sound[..4]
                        .iter()
                        .flat_map(|(_, block)| block.iter().copied()),
                );
                let expected = alone.hear(&heard).unwrap()[0].name;
                assert_eq!(
                    first,
                    Some((3, 1, expected)),
                    "bar {bar:?}, after a forget: {forgot}"
                );
            }
        }
    }

    /// #6462: with no bar a start and each forget lead the models once, at
    /// the next block, and the silence is never heard on the loop: each
    /// block gives the models only itself, and the scores are those of the
    /// silence heard.
    #[test]
    fn a_forget_without_a_bar_leads_the_models_and_hears_no_silence() {
        let blocks = room(&STRETCHES);
        let mut led = Quiet::new(Sums::default(), None);
        let mut scores = Vec::new();
        for (at, (_, block)) in blocks.iter().enumerate() {
            if at > 0 && at % 150 == 0 {
                led.forget();
                led.forget();
            }
            scores.extend(led.hear(block).unwrap().scores);
        }
        assert_eq!(led.models().leads, 1 + (blocks.len() - 1) / 150);
        assert_eq!(led.models().longest_heard, 320);
        // The same as hearing the silence at each start.
        let mut heard = Vec::new();
        let mut alone = Sums::default();
        for (at, (_, block)) in blocks.iter().enumerate() {
            if at % 150 == 0 {
                alone.forget();
                alone.hear(&vec![0; LEAD]).unwrap();
            }
            heard.extend(alone.hear(block).unwrap());
        }
        assert_eq!(scores, heard);
    }

    /// A block whose loudest sample is exactly at the bar is sound; one a
    /// step under it is quiet.
    #[test]
    fn a_sample_at_the_bar_is_sound_and_one_under_it_is_not() {
        let bar = bar_of(BAR_DB);
        for (loudest, sound) in [(bar, true), (bar - 1, false)] {
            let mut quiet = Quiet::new(Sums::default(), Some(BAR_DB));
            let mut block = vec![0i16; 320];
            block[100] = -(loudest as i16);
            let mut scored = false;
            for at in 0..200 {
                let block = if at == 150 {
                    &block[..]
                } else {
                    &[0i16; 320][..]
                };
                scored |= !quiet.hear(block).unwrap().scores.is_empty();
            }
            assert_eq!(scored, sound, "{loudest}");
        }
    }

    #[test]
    fn the_bar_and_the_loudness_are_in_dbfs() {
        assert_eq!(bar_of(-50.0), 104);
        assert_eq!(bar_of(-30.0), 1_037);
        assert_eq!(dbfs(0), SILENCE_DB);
        assert!((dbfs(104) - -49.96).abs() < 0.01);
        assert!(dbfs(32_767) > -0.01);
        let mut loudness = Loudness::default();
        assert_eq!(loudness.take(), None);
        // 98 quiet steps and two loud ones, in blocks that cross the steps.
        let mut samples = vec![10i16; STEP * 98];
        samples.extend(vec![3_277i16; STEP * 2]);
        for block in samples.chunks(320 * 3) {
            loudness.add(block);
        }
        let (median, top) = loudness.take().unwrap();
        assert!((median - dbfs(10)).abs() < 1e-3, "{median}");
        assert!((top - dbfs(3_277)).abs() < 1e-3, "{top}");
        assert_eq!(loudness.take(), None);
    }
}
