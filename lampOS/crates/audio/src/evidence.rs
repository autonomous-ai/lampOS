//! How much of what the microphone picked up is what the speaker was given.
//!
//! One observer sits beside the echo canceller and sees the same three
//! streams in the same order: the PCM the speaker device accepted, the
//! microphone block before cancellation and the same block after it. For
//! every capture block it reports measurements, never a verdict:
//!
//! - whether accepted speaker PCM was recently non-silent;
//! - the level of the capture before and after cancellation;
//! - once an alignment between capture and render has been found, the share
//!   of the capture's energy, and of the cancelled signal's energy, that the
//!   render at that alignment explains.
//!
//! The second share is the one a speech detector cannot see for itself: a
//! detector fed the cancelled signal fires on leaked playback exactly as it
//! fires on a person. A high share says that what remains is still mostly
//! Lamp's own voice. A low share says little: on a device, what a canceller
//! leaves is mostly what no linear fit explains, playback included. The
//! floor reported beside it does not depend on fitting the cancelled signal
//! at all. Whether a person spoke, and whether Lamp was being addressed,
//! this module cannot know.
//!
//! No alignment, no claim: while the render is silent, or active but not yet
//! matched, the echo part of the evidence is absent rather than guessed.
//!
//! Everything is integer arithmetic on bounded rings at a 4 kHz analysis
//! rate. There is no allocation after construction, no I/O and no clock.
use crate::{CAPTURE_SAMPLES, CaptureBlock, RENDER_SAMPLES, RenderBlock};
use serde::{Deserialize, Serialize};

/// Analysis rate. Both streams are reduced to it by plain averaging, six
/// render samples or four capture samples to one, and each result is then
/// averaged with the one before it. The two streams get the same frequency
/// response, and little is left near half the analysis rate, where a delay
/// that is not a whole number of samples could not be followed.
pub const ANALYSIS_RATE: u32 = 4_000;
/// Analysis samples per 10 ms block of either stream.
const BLOCK: usize = 40;
/// Samples compared: the last 120 ms of capture.
const WINDOW: usize = 12 * BLOCK;
/// Render history kept, a little over one second.
const HISTORY: usize = 4_096;
/// Longest delay searched from accepted render to its capture: 400 ms.
const MAX_DELAY: i64 = 40 * BLOCK as i64;
/// Alignments tried per capture block while searching. With the three tried
/// around a known alignment this bounds the work of one block.
const SWEEP: i64 = 32;
/// A window quieter than this RMS, in 16-bit sample units, is silence
/// (-60 dB relative to full scale).
const SILENT_RMS: u64 = 33;
/// Render samples fitted together to one capture window. A speaker beside a
/// microphone is not one delay: on the first device recording the path's
/// energy was spread over about three milliseconds, and one delayed copy
/// explained 40 % of a capture that was all playback.
const TAPS: usize = 16;
/// Of those, how many lie at a shorter delay than the alignment itself.
const EARLIER: i64 = 5;
/// Render samples one fit reads: a window for each of its copies.
const SPAN: usize = WINDOW + TAPS - 1;
/// Rival alignments are fitted beside the held one, this far either side of
/// it and this far apart: 10 ms and 1.25 ms. A voice repeats every few
/// milliseconds, and an alignment one pitch period from the true one
/// explains a steady vowel as well. It fails where the voice changes, and a
/// rival that does not is taken instead.
const RIVAL_REACH: i64 = 40;
const RIVAL_STEP: i64 = 5;
/// Rivals fitted per capture block. All of them are visited in four blocks.
const RIVALS: i64 = 4;
/// Share of capture energy an alignment must explain to be adopted, and to
/// count as still holding, in percent.
pub const MATCH_PERCENT: u8 = 50;
/// How far a candidate must beat the current alignment to replace it.
const REPLACE_MARGIN_PERCENT: u8 = 15;
/// Blocks of recent render activity that are remembered: 500 ms.
pub const ACTIVITY_BLOCKS: u8 = 50;
/// The most [`EchoEvidence::render_quiet_blocks`] reports: 640 ms or longer.
pub const QUIET_BLOCKS_CEILING: u8 = 64;
/// Blocks over which [`EchoMatch::render_drop`] takes the render's peak:
/// 150 ms.
const RENDER_PEAK_BLOCKS: usize = 15;

/// RMS level in tenths of a decibel relative to full scale, measured in the
/// analysis band (below about 2 kHz) over the last 120 ms and about the mean.
/// Digital silence is reported as [`Level::FLOOR`], not as negative infinity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Level(i16);
impl Level {
    pub const FLOOR: Self = Self(-960);
    pub fn tenths_of_db(self) -> i16 {
        self.0
    }
    /// A level given in tenths of a decibel, no lower than [`Level::FLOOR`].
    pub fn from_tenths_of_db(tenths: i16) -> Self {
        Self(tenths.max(Self::FLOOR.0))
    }
    /// From the energy of one analysis window about its mean.
    fn of(energy: f64) -> Self {
        Self::of_samples(energy, WINDOW)
    }
    /// From the energy of `samples` analysis samples.
    fn of_samples(energy: f64, samples: usize) -> Self {
        if energy <= 0.0 {
            return Self::FLOOR;
        }
        let db = 10.0 * (energy / samples as f64 / (32_768.0 * 32_768.0)).log10();
        Self(((db * 10.0).round() as i16).max(Self::FLOOR.0))
    }
}

/// What the render at the matched alignment accounts for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EchoMatch {
    /// Level of the accepted render over the matched window.
    pub render_level: Level,
    /// Percent of the capture's energy, before cancellation, explained by
    /// the render at this alignment.
    pub capture_explained: u8,
    /// The same for the signal after cancellation. On a device this is a
    /// lower figure than the truth and often a much lower one: what a
    /// canceller leaves is what a linear fit could not remove, and a second
    /// linear fit explains little of it. A high value says playback is left.
    /// A low value says nothing.
    pub residual_explained: u8,
    /// Percent of the cancelled signal's energy that has to be playback by
    /// accounting alone: the cancelled signal cannot hold more of anything
    /// else than the capture did, and all but `capture_explained` of the
    /// capture is the most that can be anything else. Zero when that much
    /// would cover the whole cancelled signal, or when it is silent. It
    /// holds only while nothing after the canceller amplifies.
    pub residual_playback_floor: u8,
    /// Capture blocks since this alignment last explained at least
    /// [`MATCH_PERCENT`] of the capture. Zero means this block.
    pub alignment_age_blocks: u16,
    /// Level of the render at the matched alignment over the newest capture
    /// block alone (10 ms), where `render_level` covers 120 ms.
    // #4035: logged with the tail guard's render_drop; Hearing::heard does not read it
    pub render_block_level: Level,
    /// How far `render_block_level` is under its own peak over the last
    /// 150 ms at the alignment held, in tenths of a decibel: high on the
    /// tail of a word the lamp just stopped, near zero while it holds.
    // #4035: Hearing::heard's tail guard (Hearing::tail_drop_tenths)
    pub render_drop: u16,
}

/// Measurements for one capture block. See the module text for what they do
/// and do not establish.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EchoEvidence {
    /// Capture blocks observed since construction or reset, this one included.
    pub blocks: u32,
    /// Of the last [`ACTIVITY_BLOCKS`] render blocks, how many were not silent.
    pub render_active_blocks: u8,
    /// Render blocks since the last one that was not silent, up to
    /// [`QUIET_BLOCKS_CEILING`]. Zero while the speaker is being given sound.
    pub render_quiet_blocks: u8,
    /// Capture level before cancellation, over the last 120 ms.
    pub capture_level: Level,
    /// Level after cancellation, over the same 120 ms.
    pub residual_level: Level,
    /// Present only while non-silent render is matched to the capture.
    /// Absent means "not determined", never "no echo".
    pub echo: Option<EchoMatch>,
    /// With an alignment held and the render at it silent, that alignment's
    /// age ([`EchoMatch::alignment_age_blocks`]): `echo` is absent because
    /// nothing of the lamp's is in the window its echo would come from, and
    /// not for want of an alignment (#828). None otherwise.
    #[serde(default)]
    pub silent_at_echo: Option<u16>,
}

/// What the skip of [`EchoObserver::skip_while_render_silent`] did: capture
/// blocks observed, of them skipped, and of those the ones whose search or
/// held alignment's fit would have run, which are the skips that saved work.
/// A block in a quiet room with nothing held is skipped, but would have cost
/// nothing anyway.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Skipped {
    pub observed: u64,
    pub skipped: u64,
    pub saved: u64,
}

/// Render history. The length is a power of two so indexing is a mask.
struct History {
    samples: [i16; HISTORY],
    /// Samples ever written.
    total: u64,
    /// One past the newest sample that was not zero: every sample from here
    /// on is zero. Zero if none ever was.
    sounded: u64,
}
impl History {
    fn push(&mut self, sample: i16) {
        self.samples[(self.total as usize) & (HISTORY - 1)] = sample;
        self.total += 1;
        if sample != 0 {
            self.sounded = self.total;
        }
    }
    /// The sample with absolute index `index`, which must still be held.
    fn at(&self, index: u64) -> i64 {
        debug_assert!(index < self.total && self.total - index <= HISTORY as u64);
        i64::from(self.samples[(index as usize) & (HISTORY - 1)])
    }
}

/// The last 120 ms of one capture stream, oldest first.
struct Window {
    samples: [i16; WINDOW],
}
impl Window {
    fn push(&mut self, block: [i16; BLOCK]) {
        self.samples.copy_within(BLOCK.., 0);
        self.samples[WINDOW - BLOCK..].copy_from_slice(&block);
    }
    fn sum(&self) -> i64 {
        self.samples.iter().map(|&sample| i64::from(sample)).sum()
    }
    /// Energy about the mean, so a microphone offset is not counted as signal.
    fn energy(&self) -> f64 {
        let squares: i64 = self
            .samples
            .iter()
            .map(|&sample| i64::from(sample) * i64::from(sample))
            .sum();
        centred(squares, self.sum(), self.sum())
    }
}

/// `sum(a*b) - sum(a)*sum(b)/n` over one window: a covariance times `n`.
fn centred(products: i64, a: i64, b: i64) -> f64 {
    products as f64 - a as f64 * b as f64 / WINDOW as f64
}

struct Alignment {
    /// Render index minus capture index of matching samples, as a difference
    /// of absolute sample counts. Constant while both streams are continuous
    /// and their clocks agree; it moves by one when they drift.
    offset: i64,
    /// Capture blocks since it last explained [`MATCH_PERCENT`].
    age_blocks: u16,
}

/// Reduces one stream to the analysis rate. See [`ANALYSIS_RATE`].
#[derive(Default)]
struct Reducer {
    previous: i32,
}
impl Reducer {
    fn push(&mut self, group: &[i16]) -> i16 {
        let mean = group.iter().map(|&sample| i32::from(sample)).sum::<i32>() / group.len() as i32;
        let reduced = (mean + self.previous) / 2;
        self.previous = mean;
        reduced as i16
    }
}

pub struct EchoObserver {
    render: History,
    raw: Window,
    clean: Window,
    reducers: [Reducer; 3],
    /// Analysis samples of capture ever observed.
    captured: u64,
    blocks: u32,
    /// One bit per recent render block, newest in bit zero.
    activity: u64,
    alignment: Option<Alignment>,
    /// How far the search has got: steps behind the newest render while
    /// nothing is matched, steps outward from the alignment once one is.
    sweep: i64,
    /// Which rivals of the held alignment are fitted next.
    rival: i64,
    /// Skip the search while the render it would read is all zeros
    /// ([`EchoObserver::skip_while_render_silent`]).
    skip_silent: bool,
    /// What the skip did since construction ([`Skipped`]): kept through a
    /// reset, so a run can say how often it applied.
    counts: Skipped,
    latest: Option<EchoEvidence>,
    /// The held alignment's age when the render at it is silent this block
    /// ([`EchoEvidence::silent_at_echo`]).
    silent_at: Option<u16>,
    /// The render's level at the held alignment over the last
    /// [`RENDER_PEAK_BLOCKS`] capture blocks, oldest first, for
    /// [`EchoMatch::render_drop`].
    render_blocks: [Level; RENDER_PEAK_BLOCKS],
}
impl Default for EchoObserver {
    fn default() -> Self {
        Self::new()
    }
}

/// The render under one alignment of the capture window.
struct Compared {
    /// Render energy about its mean.
    render: f64,
    /// Percent of the capture's energy the render explains.
    explained: u8,
}

/// The render around one alignment, factored once and fitted to as many
/// capture windows as are compared with it.
struct Taps {
    /// The render read by the fit, oldest first: entry `index + tap` is what
    /// copy `tap` pairs with capture sample `index`.
    span: [i16; SPAN],
    sums: [i64; TAPS],
    lower: [[f64; TAPS]; TAPS],
    /// Render energy about its mean at the alignment itself.
    energy: f64,
}

/// One alignment fitted to the raw capture, kept so that the cancelled
/// signal can be fitted against the same render without factoring it again.
struct Fitted {
    offset: i64,
    raw: Compared,
    taps: Option<Taps>,
}

const SILENT_ENERGY: f64 = (SILENT_RMS * SILENT_RMS * WINDOW as u64) as f64;

impl EchoObserver {
    pub fn new() -> Self {
        Self {
            render: History {
                samples: [0; HISTORY],
                total: 0,
                sounded: 0,
            },
            raw: Window {
                samples: [0; WINDOW],
            },
            clean: Window {
                samples: [0; WINDOW],
            },
            reducers: Default::default(),
            captured: 0,
            blocks: 0,
            activity: 0,
            alignment: None,
            sweep: 0,
            rival: 0,
            skip_silent: false,
            counts: Skipped::default(),
            latest: None,
            silent_at: None,
            render_blocks: [Level::FLOOR; RENDER_PEAK_BLOCKS],
        }
    }
    /// Forget everything but [`Self::skip_while_render_silent`] and the
    /// counts of [`Self::skipped`]. Call wherever the echo canceller is reset.
    pub fn reset(&mut self) {
        let (skip_silent, counts) = (self.skip_silent, self.counts);
        *self = Self::new();
        (self.skip_silent, self.counts) = (skip_silent, counts);
    }

    /// What [`Self::skip_while_render_silent`] did since construction.
    pub fn skipped(&self) -> Skipped {
        self.counts
    }

    /// While every render sample any alignment would read is zero (the
    /// speaker given silence), the search finds nothing whatever the
    /// microphone hears: the render under every alignment is below
    /// [`SILENT_RMS`]. With this on, it is not run then, and the evidence is
    /// the same: no echo, the held alignment's age in `silent_at_echo`, and
    /// the search's place moved on as the search would have. Without it, a
    /// silent speaker still costs, every block: a room loud enough to search
    /// (a fan, an office) pays for the search, and once the lamp has spoken
    /// since the last reset, a room of any level pays for fitting the held
    /// alignment's copies. Off by default.
    pub fn skip_while_render_silent(&mut self, on: bool) {
        self.skip_silent = on;
    }
    /// The evidence of the most recent capture block, if any since reset.
    pub fn latest(&self) -> Option<EchoEvidence> {
        self.latest
    }

    /// How long after the newest render the echo of the alignment held
    /// arrives, in analysis samples (a quarter of a millisecond each), and
    /// the capture blocks since that alignment last explained
    /// [`MATCH_PERCENT`] of the capture. `None` while nothing is matched.
    pub fn echo_delay(&self) -> Option<(i64, u16)> {
        self.alignment.as_ref().map(|alignment| {
            (
                self.render.total as i64 - self.captured as i64 - alignment.offset,
                alignment.age_blocks,
            )
        })
    }

    /// One block of PCM the speaker device accepted, in acceptance order.
    pub fn render(&mut self, samples: &RenderBlock) {
        let mut energy = 0u64;
        for group in samples.chunks_exact(RENDER_SAMPLES / BLOCK) {
            let reduced = self.reducers[0].push(group);
            energy += (i32::from(reduced) * i32::from(reduced)) as u64;
            self.render.push(reduced);
        }
        let active = energy >= SILENT_RMS * SILENT_RMS * BLOCK as u64;
        if active && self.alignment.is_none() && self.activity & ((1 << ACTIVITY_BLOCKS) - 1) == 0 {
            // The speaker starts after a silence with nothing matched: its
            // echo is at the short delays first, so look there first.
            self.sweep = 0;
        }
        self.activity = (self.activity << 1) | u64::from(active);
    }

    /// One microphone block before and after cancellation.
    pub fn capture(&mut self, raw: &CaptureBlock, clean: &CaptureBlock) -> EchoEvidence {
        self.counts.observed += 1;
        let reduce = |reducer: &mut Reducer, block: &CaptureBlock| -> [i16; BLOCK] {
            let mut reduced = [0; BLOCK];
            for (target, group) in reduced
                .iter_mut()
                .zip(block.chunks_exact(CAPTURE_SAMPLES / BLOCK))
            {
                *target = reducer.push(group);
            }
            reduced
        };
        self.raw.push(reduce(&mut self.reducers[1], raw));
        self.clean.push(reduce(&mut self.reducers[2], clean));
        self.captured += BLOCK as u64;
        self.blocks = self.blocks.saturating_add(1);
        let (raw_energy, clean_energy) = (self.raw.energy(), self.clean.energy());
        let echo = self.matched(raw_energy);
        // #4035: render_drop, for Hearing::heard's tail guard
        let block = self.alignment.as_ref().map_or(Level::FLOOR, |alignment| {
            self.render_block(alignment.offset)
        });
        self.render_blocks.copy_within(1.., 0);
        self.render_blocks[RENDER_PEAK_BLOCKS - 1] = block;
        let peak = self
            .render_blocks
            .iter()
            .max()
            .copied()
            .unwrap_or(Level::FLOOR);
        let echo = echo.map(|echo| EchoMatch {
            render_block_level: block,
            render_drop: (peak.0 - block.0).unsigned_abs(),
            ..echo
        });
        let evidence = EchoEvidence {
            blocks: self.blocks,
            render_active_blocks: (self.activity & ((1 << ACTIVITY_BLOCKS) - 1)).count_ones() as u8,
            render_quiet_blocks: self.activity.trailing_zeros().min(64) as u8,
            capture_level: Level::of(raw_energy),
            residual_level: Level::of(clean_energy),
            echo,
            silent_at_echo: self.silent_at,
        };
        self.latest = Some(evidence);
        evidence
    }

    /// The render's level over the newest capture block at `offset`, or
    /// [`Level::FLOOR`] where that render is no longer, or not yet, held.
    fn render_block(&self, offset: i64) -> Level {
        let start = self.captured as i64 - BLOCK as i64 + offset;
        let oldest = self.render.total as i64 - HISTORY as i64;
        if start < oldest.max(0) || start + BLOCK as i64 > self.render.total as i64 {
            return Level::FLOOR;
        }
        let energy: i64 = (start..start + BLOCK as i64)
            .map(|index| self.render.at(index as u64).pow(2))
            .sum();
        Level::of_samples(energy as f64, BLOCK)
    }

    /// Offsets at which the whole capture window has render to compare with:
    /// from the longest delay searched up to the newest render (no delay).
    fn offsets(&self) -> Option<(i64, i64)> {
        if self.captured < WINDOW as u64 {
            return None;
        }
        let newest = self.render.total as i64 - self.captured as i64;
        let first = self.captured as i64 - WINDOW as i64;
        let oldest_held = (self.render.total as i64 - HISTORY as i64).max(0);
        let low = (newest - MAX_DELAY).max(oldest_held - first);
        (low <= newest).then_some((low, newest))
    }

    /// Compare one capture window with the render at `offset`, which must be
    /// inside [`Self::offsets`].
    fn compare(&self, capture: &Window, energy: f64, offset: i64) -> Compared {
        let first = (self.captured as i64 - WINDOW as i64 + offset) as u64;
        let (mut cross, mut squares, mut sum) = (0i64, 0i64, 0i64);
        for (index, &sample) in capture.samples.iter().enumerate() {
            let reference = self.render.at(first + index as u64);
            cross += i64::from(sample) * reference;
            squares += reference * reference;
            sum += reference;
        }
        let render = centred(squares, sum, sum);
        let cross = centred(cross, capture.sum(), sum);
        let explained = if energy <= 0.0 || render <= 0.0 {
            0
        } else {
            (cross * cross / (energy * render) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8
        };
        Compared { render, explained }
    }

    /// The render around `offset` prepared for fitting: [`TAPS`] delayed
    /// copies, [`EARLIER`] of them at a shorter delay, moved inward where the
    /// searched range ends. `None` where the range is narrower than that or
    /// the render there cannot be solved for.
    fn taps(&self, offset: i64, low: i64, newest: i64) -> Option<Taps> {
        let top = (offset + EARLIER).min(newest);
        let bottom = top - (TAPS as i64 - 1);
        if bottom < low || offset < bottom {
            return None;
        }
        let first = (self.captured as i64 - WINDOW as i64 + bottom) as u64;
        let span: [i16; SPAN] =
            std::array::from_fn(|index| self.render.at(first + index as u64) as i16);
        let at = |index: usize| i64::from(span[index]);
        // Products of the window with itself at every pair of delays. The
        // first row is summed; each later entry follows from its diagonal
        // neighbour by dropping one product and adding one.
        let (mut sums, mut products) = ([0i64; TAPS], [[0i64; TAPS]; TAPS]);
        for index in 0..WINDOW {
            let sample = at(index);
            sums[0] += sample;
            for (column, product) in products[0].iter_mut().enumerate() {
                *product += sample * at(index + column);
            }
        }
        for row in 1..TAPS {
            sums[row] = sums[row - 1] - at(row - 1) + at(WINDOW + row - 1);
            for column in row..TAPS {
                products[row][column] = products[row - 1][column - 1]
                    - at(row - 1) * at(column - 1)
                    + at(WINDOW + row - 1) * at(WINDOW + column - 1);
            }
        }
        let mut render = [[0f64; TAPS]; TAPS];
        let mut trace = 0.0;
        for row in 0..TAPS {
            for column in row..TAPS {
                render[row][column] = centred(products[row][column], sums[row], sums[column]);
            }
            trace += render[row][row];
        }
        let own = (offset - bottom) as usize;
        let energy = render[own][own];
        // A touch of ridge keeps a nearly periodic render solvable.
        let ridge = trace * 1e-6 + 1.0;
        // Cholesky factor, in place in the lower triangle.
        let mut lower = [[0f64; TAPS]; TAPS];
        for row in 0..TAPS {
            for column in 0..=row {
                let sum: f64 = (0..column).map(|k| lower[row][k] * lower[column][k]).sum();
                if row == column {
                    let pivot = render[row][row] + ridge - sum;
                    if pivot <= 0.0 {
                        return None;
                    }
                    lower[row][column] = pivot.sqrt();
                } else {
                    lower[row][column] = (render[column][row] - sum) / lower[column][column];
                }
            }
        }
        Some(Taps {
            span,
            sums,
            lower,
            energy,
        })
    }

    /// Percent of `capture`'s energy that the prepared render accounts for
    /// when its copies are fitted to it together by least squares.
    fn share(taps: &Taps, capture: &Window, energy: f64) -> u8 {
        if energy <= 0.0 {
            return 0;
        }
        let mut cross = [0i64; TAPS];
        for (index, &sample) in capture.samples.iter().enumerate() {
            let sample = i64::from(sample);
            for (cross, &render) in cross.iter_mut().zip(&taps.span[index..]) {
                *cross += sample * i64::from(render);
            }
        }
        let capture_sum = capture.sum();
        // Forward substitution: |solve(lower, wanted)|^2 is the energy the
        // fit accounts for.
        let mut solved = [0f64; TAPS];
        let mut accounted = 0.0;
        for row in 0..TAPS {
            let wanted = centred(cross[row], capture_sum, taps.sums[row]);
            let sum: f64 = (0..row).map(|k| taps.lower[row][k] * solved[k]).sum();
            solved[row] = (wanted - sum) / taps.lower[row][row];
            accounted += solved[row] * solved[row];
        }
        (accounted / energy * 100.0).round().clamp(0.0, 100.0) as u8
    }

    /// The raw capture against the render around `offset`: the fit of
    /// [`Self::taps`] where there is room for it, one copy where there is not.
    fn fitted(&self, energy: f64, offset: i64, low: i64, newest: i64) -> Fitted {
        match self.taps(offset, low, newest) {
            Some(taps) => Fitted {
                offset,
                raw: Compared {
                    render: taps.energy,
                    explained: Self::share(&taps, &self.raw, energy),
                },
                taps: Some(taps),
            },
            None => Fitted {
                offset,
                raw: self.compare(&self.raw, energy, offset),
                taps: None,
            },
        }
    }

    /// The best of `offsets` at which the render is not silent.
    fn best(&self, energy: f64, offsets: impl Iterator<Item = i64>) -> Option<(i64, u8)> {
        offsets
            .map(|offset| (offset, self.compare(&self.raw, energy, offset)))
            .filter(|(_, compared)| compared.render >= SILENT_ENERGY)
            .map(|(offset, compared)| (offset, compared.explained))
            .max_by_key(|&(_, explained)| explained)
    }

    /// The `step`th alignment of the search. With nothing matched it walks
    /// back from the newest render. Around a held alignment it alternates
    /// outward, so one that drifted while the speaker was silent is found
    /// again at once. `None` once the whole range has been covered.
    fn candidate(&self, low: i64, newest: i64, step: i64) -> Option<Option<i64>> {
        let offset = match &self.alignment {
            None => newest - step,
            // The alignment and its two neighbours are always tried.
            Some(alignment) if step % 2 == 0 => alignment.offset - 2 - step / 2,
            Some(alignment) => alignment.offset + 2 + step / 2,
        };
        let exhausted = match &self.alignment {
            None => offset < low,
            Some(_) => 2 + step / 2 > newest - low,
        };
        (!exhausted).then(|| (low..=newest).contains(&offset).then_some(offset))
    }

    fn matched(&mut self, raw_energy: f64) -> Option<EchoMatch> {
        self.silent_at = None;
        let (low, newest) = self.offsets()?;
        if let Some(alignment) = self.alignment.as_mut() {
            alignment.age_blocks = alignment.age_blocks.saturating_add(1);
            if !(low..=newest).contains(&alignment.offset) {
                // The streams moved apart further than is searched.
                self.alignment = None;
                self.sweep = 0;
            }
        }
        if self.skip_silent
            && self.render.sounded as i64 <= self.captured as i64 - WINDOW as i64 + low
        {
            // Nothing the search reads is the lamp's: what it would find.
            if raw_energy >= SILENT_ENERGY {
                self.sweep = match self.candidate(low, newest, self.sweep + SWEEP) {
                    Some(_) => self.sweep + SWEEP,
                    None => 0,
                };
                self.rival = (self.rival + RIVALS) % (2 * RIVAL_REACH / RIVAL_STEP);
            }
            self.silent_at = self
                .alignment
                .as_ref()
                .map(|alignment| alignment.age_blocks);
            self.counts.skipped += 1;
            // Only a loud room is searched, and only a held alignment is
            // fitted: a quiet block with neither cost nothing to skip.
            if raw_energy >= SILENT_ENERGY || self.alignment.is_some() {
                self.counts.saved += 1;
            }
            return None;
        }
        let mut fitted = None;
        // A silent capture matches nothing and must not unseat an alignment.
        if raw_energy >= SILENT_ENERGY {
            // Follow slow clock drift around the alignment already held. One
            // delayed copy says where to look; whether the render there
            // explains the capture is decided by the fit.
            let near = self.alignment.as_ref().and_then(|alignment| {
                let near = alignment.offset - 1..=alignment.offset + 1;
                self.best(
                    raw_energy,
                    near.filter(|offset| (low..=newest).contains(offset)),
                )
            });
            let held = near.map(|(offset, _)| self.fitted(raw_energy, offset, low, newest));
            // Keep looking elsewhere as well: the first match may be a weaker
            // reflection, and a restarted stream moves the true alignment.
            let mut tried = [None; SWEEP as usize];
            for (slot, step) in tried.iter_mut().zip(self.sweep..) {
                match self.candidate(low, newest, step) {
                    Some(offset) => *slot = offset,
                    None => break,
                }
            }
            self.sweep = match self.candidate(low, newest, self.sweep + SWEEP) {
                Some(_) => self.sweep + SWEEP,
                None => 0,
            };
            let found = self
                .best(raw_energy, tried.into_iter().flatten())
                // A challenger has to beat the held alignment with one copy
                // before it is worth fitting.
                .filter(|&(_, one)| near.is_none_or(|(_, held)| one > held))
                .map(|(offset, _)| self.fitted(raw_energy, offset, low, newest));
            // Rivals near the held alignment, a few each block, while the
            // held one leaves room for a rival to be taken.
            let contestable =
                |held: &&Fitted| held.raw.explained.saturating_add(REPLACE_MARGIN_PERCENT) <= 100;
            let rival = held.as_ref().filter(contestable).and_then(|held| {
                let steps = 2 * RIVAL_REACH / RIVAL_STEP;
                (0..RIVALS)
                    .map(|index| {
                        let step = (self.rival + index) % steps;
                        // -reach, +reach, -reach + step, +reach - step, ...
                        let away = RIVAL_REACH - step / 2 * RIVAL_STEP;
                        held.offset + if step % 2 == 0 { -away } else { away }
                    })
                    .filter(|offset| (low..=newest).contains(offset))
                    .map(|offset| self.fitted(raw_energy, offset, low, newest))
                    .filter(|rival| rival.raw.render >= SILENT_ENERGY)
                    .max_by_key(|rival| rival.raw.explained)
            });
            self.rival = (self.rival + RIVALS) % (2 * RIVAL_REACH / RIVAL_STEP);
            let found = match (found, rival) {
                (Some(found), Some(rival)) if rival.raw.explained > found.raw.explained => {
                    Some(rival)
                }
                (found, rival) => found.or(rival),
            }
            .map(|found| {
                if found.raw.explained < MATCH_PERCENT {
                    return found;
                }
                // Record the match at its strongest single copy, so that
                // following it a sample at a time has a peak to follow.
                let span = found.offset - (TAPS as i64 - 1 - EARLIER)..=found.offset + EARLIER;
                match self.best(
                    raw_energy,
                    span.filter(|offset| (low..=newest).contains(offset)),
                ) {
                    Some((peak, _)) if peak != found.offset => {
                        self.fitted(raw_energy, peak, low, newest)
                    }
                    _ => found,
                }
            });
            let adopted = match (held, found) {
                (Some(held), Some(found))
                    if found.raw.explained >= MATCH_PERCENT
                        && found.raw.explained
                            >= held.raw.explained.saturating_add(REPLACE_MARGIN_PERCENT) =>
                {
                    Some(found)
                }
                (Some(held), _) if held.raw.explained >= MATCH_PERCENT => Some(held),
                (None, Some(found)) if found.raw.explained >= MATCH_PERCENT => Some(found),
                (held, _) => {
                    // Not matched this block. Its fit still serves the report
                    // if it is the alignment's own.
                    fitted = held;
                    None
                }
            };
            if let Some(adopted) = adopted {
                if self
                    .alignment
                    .as_ref()
                    .is_none_or(|held| (held.offset - adopted.offset).abs() > 1)
                {
                    self.sweep = 0;
                }
                self.alignment = Some(Alignment {
                    offset: adopted.offset,
                    age_blocks: 0,
                });
                fitted = Some(adopted);
            }
        }
        let (offset, age_blocks) = self
            .alignment
            .as_ref()
            .map(|alignment| (alignment.offset, alignment.age_blocks))?;
        let fitted = fitted
            .filter(|fitted| fitted.offset == offset)
            .unwrap_or_else(|| self.fitted(raw_energy, offset, low, newest));
        if fitted.raw.render < SILENT_ENERGY {
            // Nothing of the lamp's is in the window its echo would come
            // from: said, apart from having no alignment at all (#828).
            self.silent_at = Some(age_blocks);
            return None;
        }
        let clean_energy = self.clean.energy();
        let residual_explained = match &fitted.taps {
            Some(taps) => Self::share(taps, &self.clean, clean_energy),
            None => self.compare(&self.clean, clean_energy, offset).explained,
        };
        let other = raw_energy * f64::from(100 - fitted.raw.explained) / 100.0;
        let residual_playback_floor = if clean_energy < SILENT_ENERGY {
            0
        } else {
            ((1.0 - other / clean_energy) * 100.0).clamp(0.0, 100.0) as u8
        };
        Some(EchoMatch {
            render_level: Level::of(fitted.raw.render),
            capture_explained: fitted.raw.explained,
            residual_explained,
            residual_playback_floor,
            alignment_age_blocks: age_blocks,
            // Filled in by `capture`, which keeps the render's recent peak.
            render_block_level: Level::FLOOR,
            render_drop: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(seed: u64, index: u64) -> f64 {
        let mut x = index.wrapping_add(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((x ^ (x >> 31)) >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }

    /// The lamp's voice at `seconds`: it speaks from 0.5 s to 1.5 s and from
    /// 4 s to 4.5 s, and gives the speaker zeros otherwise.
    fn lamp(seconds: f64) -> f64 {
        let speaking = (0.5..1.5).contains(&seconds) || (4.0..4.5).contains(&seconds);
        if !speaking {
            return 0.0;
        }
        let index = (seconds * 4_000.0) as u64;
        8_000.0 * noise(1, index) * (0.4 + 0.6 * (seconds * 19.0).sin().abs())
    }

    /// A render held at one level for a second, then stopped at once, with
    /// its echo 50 ms late (five blocks). While it holds, the render at the
    /// alignment is about its own recent peak; once the alignment reaches the
    /// stop, every block still matched is 6 dB or more under it: the tail a
    /// word leaves in the room (#4035).
    #[test]
    fn a_render_that_stops_abruptly_drops_and_a_held_one_does_not() {
        let mut observer = EchoObserver::new();
        let stop = 100u64;
        let (mut held, mut tail) = (Vec::new(), Vec::new());
        for block in 0..stop + 30 {
            let render: RenderBlock = std::array::from_fn(|index| {
                let sample = block * RENDER_SAMPLES as u64 + index as u64;
                if block < stop {
                    (8_000.0 * noise(1, sample / 6)) as i16
                } else {
                    0
                }
            });
            let capture: CaptureBlock = std::array::from_fn(|index| {
                let sample = block * CAPTURE_SAMPLES as u64 + index as u64;
                let echo = if (5..stop + 5).contains(&block) {
                    -0.5 * 8_000.0 * noise(1, (sample - 5 * CAPTURE_SAMPLES as u64) / 4)
                } else {
                    0.0
                };
                (echo + 4.0 * noise(2, sample)).round() as i16
            });
            observer.render(&render);
            let Some(echo) = observer.capture(&capture, &capture).echo else {
                continue;
            };
            if (50..stop).contains(&block) {
                held.push(echo.render_drop);
            } else if block >= stop + 5 {
                tail.push(echo.render_drop);
            }
        }
        assert!(held.len() >= 40, "held matched {} blocks", held.len());
        // A 10 ms block of noise wanders a few dB about its level; all of
        // it stays under the 6 dB a tail is taken at, most of it near zero.
        assert!(held.iter().all(|&drop| drop < 50), "held {held:?}");
        let mean = held.iter().map(|&drop| u32::from(drop)).sum::<u32>() / held.len() as u32;
        assert!(mean <= 20, "held {held:?}");
        assert!(tail.len() >= 3, "tail matched {} blocks", tail.len());
        assert!(tail.iter().all(|&drop| drop >= 60), "tail {tail:?}");
    }

    /// The same scene through an observer that skips and one that does not:
    /// its echo 110 ms late, a quiet room, a loud one (a fan) from 1.5 s to
    /// 4 s with someone speaking in it from 2 s to 2.6 s, then quiet again.
    /// Every block's evidence, and where each search has got, are the same,
    /// and the skipping one skipped the loud silence.
    #[test]
    fn skipping_while_the_render_is_silent_changes_no_evidence() {
        let mut searching = EchoObserver::new();
        let mut skipping = EchoObserver::new();
        skipping.skip_while_render_silent(true);
        let mut matched = 0;
        for block in 0..600u64 {
            let render: RenderBlock = std::array::from_fn(|index| {
                lamp((block * RENDER_SAMPLES as u64 + index as u64) as f64 / 24_000.0) as i16
            });
            let capture: CaptureBlock = std::array::from_fn(|index| {
                let sample = block * CAPTURE_SAMPLES as u64 + index as u64;
                let seconds = sample as f64 / 16_000.0;
                let room = if (1.5..4.0).contains(&seconds) {
                    1_600.0
                } else {
                    4.0
                };
                let talker = if (2.0..2.6).contains(&seconds) {
                    3_000.0
                } else {
                    0.0
                };
                (-0.5 * lamp(seconds - 0.11)
                    + room * noise(2, sample)
                    + talker * noise(3, sample / 2))
                .round() as i16
            });
            if block == 300 {
                // A reset keeps the switch, and the counts.
                searching.reset();
                skipping.reset();
            }
            searching.render(&render);
            skipping.render(&render);
            let expected = searching.capture(&capture, &capture);
            assert_eq!(
                skipping.capture(&capture, &capture),
                expected,
                "block {block}"
            );
            assert_eq!(
                (skipping.sweep, skipping.rival, skipping.echo_delay()),
                (searching.sweep, searching.rival, searching.echo_delay()),
                "block {block}"
            );
            matched += usize::from(expected.echo.is_some());
        }
        assert!(skipping.skip_silent);
        assert!(matched > 50, "the echo was matched {matched} times");
        // The silences, each less the render's 520 ms reach (none after the
        // reset, or before the lamp first spoke): 324 of 600 blocks.
        let skipped = skipping.skipped();
        assert_eq!((skipped.observed, skipped.skipped), (600, 324));
        // All but the 39 of the quiet start, before the lamp first spoke and
        // so with nothing held, saved work.
        assert_eq!(skipped.saved, 324 - 39);
        assert_eq!(
            searching.skipped(),
            Skipped {
                observed: 600,
                ..Skipped::default()
            }
        );
    }

    /// A quiet room with nothing held: the speaker's silence is skipped, but
    /// no search or fit would have run, so the skip saved nothing (the
    /// reviewer on #1322).
    #[test]
    fn a_quiet_room_with_nothing_held_is_skipped_but_saves_nothing() {
        let mut skipping = EchoObserver::new();
        skipping.skip_while_render_silent(true);
        for block in 0..600u64 {
            skipping.render(&[0; RENDER_SAMPLES]);
            let capture: CaptureBlock = std::array::from_fn(|index| {
                (4.0 * noise(2, block * CAPTURE_SAMPLES as u64 + index as u64)).round() as i16
            });
            skipping.capture(&capture, &capture);
        }
        let skipped = skipping.skipped();
        assert_eq!((skipped.observed, skipped.saved), (600, 0));
        assert!(skipped.skipped > 580, "{skipped:?}");
    }
}
