//! The lamp's own speech in its head (#933): while it talks it should read
//! as a creature, not a speaker with a light on. Three small things, from
//! the voice's own sound as the body will meet it (the playout said ahead,
//! sampled at the body's lag):
//! - **a nod on a stressed word:** a short dip of the head when the level
//!   jumps over what it has just been, never closer than a gap to the last
//!   nod, each a little different in size;
//! - **a sway between sentences:** after a pause in its speech, as the next
//!   sentence begins, the head rolls a little to the other side and stays;
//! - **a settle as it finishes:** one soft nod when an answer of some length
//!   ends, then rest.
//!
//! Only the head, within the catalogue's limits, in any room: it is the
//! lamp's own speech. Off unless the catalogue has a `speaking` section.
use crate::Error;
use crate::catalogue::{add, ease};
use lamp_msgs::BodyOffsets;
use serde::{Deserialize, Serialize};

/// Under this the lamp is silent between words or sentences, for a voice at
/// `REFERENCE`; it scales with the voice's own level (below).
const QUIET_LEVEL: f32 = 0.06;
/// A sentence has begun again once the level is back over this, for a voice
/// at `REFERENCE`; it scales the same way.
const SPEAKING_LEVEL: f32 = 0.12;
/// The voice's running level these bars and `stress_level` were tuned at:
/// the sound of Gemini Live's answers (#955), as the engine tests model it,
/// averages about this over its pieces with sound. Another voice sits higher
/// or lower (the second route's ElevenLabs, #1712), so each bar is taken as
/// a share of the turn's own running level, and a quiet voice nods and
/// sways as a loud one does (the first builder, 19 Oct 21:34 on #1722).
const REFERENCE: f32 = 0.34;
/// The running level follows the voice's sound this slowly, over pieces
/// with sound in them only, so a pause does not pull it down.
const VOICE_US: f64 = 2_000_000.0;
/// A piece under this has no sound in it, whatever the voice.
const SOUND_FLOOR: f32 = 0.01;
/// How far the bars may scale from the reference, either way.
const SCALE: (f32, f32) = (0.25, 3.0);
/// The bars' least values, so that silence stays silence and a hiss is no
/// word, however quiet the voice.
const FLOORS: (f32, f32, f32) = (0.015, 0.03, 0.06);
/// The stress bar's most: the voice's meter stops at 1.0, so a voice that
/// fills it (a running level near 1.0, scaled past 1.0) would otherwise never
/// nod, where the fixed bars nodded as each sentence began (Seventh on
/// #1722, 19 Oct 22:25). `stress_rise` still asks the word to stand out from
/// the moment before it. A catalogue whose own `stress_level` is higher keeps
/// it.
const STRESS_CEILING: f32 = 0.9;
/// A sentence's first moments are its onset, not a stressed word: no nod
/// in them.
const ONSET_US: u64 = 300_000;
/// A stressed word stands at least this far over its own sentence's mean
/// level so far, on the meter's scale (about 5 dB). A voice loud all the
/// time, as the second route's ElevenLabs on lamp-4ace (a mean of 0.73,
/// over the stress bar in most frames), passed the bar on most pieces and
/// nodded 75 times in 150 s, one every 2 s: a nodding toy, not a lamp
/// leaning into a word (the first builder on #2030, 20 Oct 03:22; CLAUDE.md
/// rule 8).
const STRESS_STEP: f32 = 0.15;
/// At most one nod a sentence, and never two within this, whatever the
/// catalogue's `nod_gap_us`: people nod on a few stressed words and are
/// still in between.
const NOD_SPACING_US: u64 = 4_000_000;
/// The recent level a stressed word is measured against follows the sound
/// this fast.
const RECENT_US: f64 = 250_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Speaking {
    /// The nod on a stressed word at its deepest, how long the whole nod
    /// takes, and the least time from one nod's start to the next.
    pub nod: BodyOffsets,
    pub nod_us: u64,
    pub nod_gap_us: u64,
    /// A stressed word: a level of at least this, and at least `stress_rise`
    /// times the recent level.
    pub stress_level: f32,
    pub stress_rise: f32,
    /// The sway: the head's roll to each side, the pause in the lamp's
    /// speech that ends a sentence, and how long the head takes to move.
    pub sway_roll: f64,
    pub pause_us: u64,
    pub sway_us: u64,
    /// The settle as it finishes, after at least `settle_after_us` of speech.
    pub settle: BodyOffsets,
    pub settle_us: u64,
    pub settle_after_us: u64,
}

impl Speaking {
    pub fn check(&self, limits: BodyOffsets) -> Result<(), Error> {
        let within = |pose: BodyOffsets| {
            pose.values()
                .into_iter()
                .zip(limits.values())
                .all(|(offset, limit)| offset.is_finite() && offset.abs() <= limit)
        };
        if !within(self.nod) || !within(self.settle) {
            return Err(Error("speaking: a pose exceeds configured limits".into()));
        }
        if !self.sway_roll.is_finite() || !(0.0..=limits.head_roll).contains(&self.sway_roll) {
            return Err(Error(
                "speaking: sway_roll must be within the head roll limit".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.stress_level) || !(1.0..=10.0).contains(&self.stress_rise) {
            return Err(Error(
                "speaking: stress_level must be 0..1 and stress_rise 1..10".into(),
            ));
        }
        for (name, value, low, high) in [
            ("nod_us", self.nod_us, 100_000, 1_500_000),
            ("nod_gap_us", self.nod_gap_us, 200_000, 10_000_000),
            ("pause_us", self.pause_us, 100_000, 2_000_000),
            ("sway_us", self.sway_us, 100_000, 3_000_000),
            ("settle_us", self.settle_us, 100_000, 2_000_000),
            ("settle_after_us", self.settle_after_us, 0, 30_000_000),
        ] {
            if !(low..=high).contains(&value) {
                return Err(Error(format!(
                    "speaking: {name} must be {low}..{high} microseconds"
                )));
            }
        }
        Ok(())
    }
}

/// What the head is doing with the lamp's speech, from tick to tick.
#[derive(Clone, Debug, Default)]
pub(crate) struct SpeechMotion {
    last_us: Option<u64>,
    recent: f32,
    /// The turn's running level, over pieces with sound (0: none yet).
    voice: f32,
    /// How many pieces with sound it is over, so far this turn.
    voiced: u32,
    /// When this sentence began, its mean level over pieces with sound so
    /// far and how many, and whether it has nodded.
    sentence_since: Option<u64>,
    sentence_level: f32,
    sentence_voiced: u32,
    sentence_nodded: bool,
    quiet_since: Option<u64>,
    paused: bool,
    spoke_since: Option<u64>,
    nod: Option<(u64, f64)>,
    nods: u64,
    side: f64,
    sway: Option<(u64, f64)>,
    sways: u64,
    settle: Option<u64>,
}

/// 0 at both ends and 1 in the middle, smoothly: one nod.
fn bump(share: f64) -> f64 {
    0.5 - 0.5 * (std::f64::consts::TAU * share.clamp(0.0, 1.0)).cos()
}

/// A share in 0..1 from a count: splitmix64, fixed, so a replay is the same.
fn fraction(count: u64, salt: u64) -> f64 {
    let mut z = count
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(salt.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1_u64 << 53) as f64
}

impl SpeechMotion {
    /// A lifetime fence (a new session, turn or audio generation, a hold, a
    /// cancellation, an interruption): nothing of the answer before goes on
    /// moving the head, and nothing is owed for it (#955 review).
    pub(crate) fn forget(&mut self) {
        *self = Self {
            nods: self.nods,
            sways: self.sways,
            ..Self::default()
        };
    }

    /// A sentence begins: its own level and its nod start again.
    fn new_sentence(&mut self, now_us: u64) {
        self.sentence_since = Some(now_us);
        self.sentence_level = 0.0;
        self.sentence_voiced = 0;
        self.sentence_nodded = false;
    }

    /// Whether an answer that ends now, as it should, has spoken long enough
    /// to be settled after.
    pub(crate) fn owes_settle(&self, rules: &Speaking, now_us: u64) -> bool {
        self.spoke_since
            .is_some_and(|since| now_us.saturating_sub(since) >= rules.settle_after_us)
    }

    /// Settle from `now_us`: what a normal finish owes, kept across the
    /// fence that ends its answer.
    pub(crate) fn settle_from(&mut self, now_us: u64) {
        self.settle = Some(now_us);
    }

    /// The head's roll from the sway now.
    fn roll(&self, rules: &Speaking, now_us: u64) -> f64 {
        let target = self.side * rules.sway_roll;
        match self.sway {
            Some((at, from)) => {
                let share = now_us.saturating_sub(at) as f64 / rules.sway_us as f64;
                from + (target - from) * ease(share.min(1.0))
            }
            None => target,
        }
    }

    fn sway_to(&mut self, rules: &Speaking, side: f64, now_us: u64) {
        let from = self.roll(rules, now_us);
        self.side = side;
        self.sway = Some((now_us, from));
    }

    /// One tick. `speaking`: the lamp's answer is under way; `level`: its
    /// sound as the body meets it now (0 between pieces); `still`: the body
    /// is held or concentrating, or someone cut in. Returns the head's
    /// offsets and names what it is doing.
    pub(crate) fn step(
        &mut self,
        rules: &Speaking,
        speaking: bool,
        level: f32,
        still: bool,
        now_us: u64,
        names: &mut Vec<&'static str>,
    ) -> BodyOffsets {
        let step = self
            .last_us
            .map_or(0, |last| now_us.saturating_sub(last))
            .min(200_000);
        self.last_us = Some(now_us);
        if still {
            // Held, concentrating or cut in on: nothing of its speech moves
            // the head, and nothing is owed when it ends.
            *self = Self {
                last_us: Some(now_us),
                nods: self.nods,
                sways: self.sways,
                ..Self::default()
            };
            return BodyOffsets::ZERO;
        }
        if speaking {
            self.spoke_since.get_or_insert(now_us);
            self.sentence_since.get_or_insert(now_us);
            // The voice's own level so far this turn: the mean of its first
            // pieces, then a slow follow.
            if level >= SOUND_FLOOR {
                self.voiced = self.voiced.saturating_add(1);
                let pace = (1.0 - (-(step as f64) / VOICE_US).exp()).max(1.0 / self.voiced as f64);
                self.voice += (level - self.voice) * pace as f32;
            }
            let scale = if self.voice > 0.0 {
                (self.voice / REFERENCE).clamp(SCALE.0, SCALE.1)
            } else {
                1.0
            };
            let quiet_level = (QUIET_LEVEL * scale).max(FLOORS.0);
            let speaking_level = (SPEAKING_LEVEL * scale).max(FLOORS.1);
            let stress_level = (rules.stress_level * scale)
                .min(STRESS_CEILING.max(rules.stress_level))
                .max(FLOORS.2);
            if level < quiet_level {
                let since = *self.quiet_since.get_or_insert(now_us);
                self.paused |= now_us.saturating_sub(since) >= rules.pause_us;
            } else {
                self.quiet_since = None;
                if self.paused && level >= speaking_level {
                    // A new sentence: the head rolls over to the other side,
                    // a little more or less each time.
                    self.paused = false;
                    self.new_sentence(now_us);
                    self.sways += 1;
                    let towards = if self.side > 0.0 { -1.0 } else { 1.0 };
                    let size = 0.75 + 0.25 * fraction(self.sways, 2);
                    self.sway_to(rules, towards * size, now_us);
                }
            }
            let stressed = level >= stress_level
                && level >= self.recent * rules.stress_rise
                && self.sentence_voiced > 0
                && level >= self.sentence_level + STRESS_STEP;
            let past_onset = self
                .sentence_since
                .is_some_and(|since| now_us.saturating_sub(since) >= ONSET_US);
            let gap = rules.nod_gap_us.max(NOD_SPACING_US);
            if stressed
                && past_onset
                && !self.sentence_nodded
                && self
                    .nod
                    .is_none_or(|(at, _)| now_us.saturating_sub(at) >= gap)
            {
                self.nods += 1;
                self.sentence_nodded = true;
                self.nod = Some((now_us, 0.7 + 0.3 * fraction(self.nods, 1)));
            }
            if level >= SOUND_FLOOR {
                self.sentence_voiced = self.sentence_voiced.saturating_add(1);
                self.sentence_level += (level - self.sentence_level) / self.sentence_voiced as f32;
            }
            let pace = 1.0 - (-(step as f64) / RECENT_US).exp();
            self.recent += (level - self.recent) * pace as f32;
        } else {
            if self
                .spoke_since
                .is_some_and(|since| now_us.saturating_sub(since) >= rules.settle_after_us)
            {
                self.settle = Some(now_us);
            }
            self.spoke_since = None;
            self.quiet_since = None;
            self.paused = false;
            self.recent = 0.0;
            self.voice = 0.0;
            self.voiced = 0;
            self.sentence_since = None;
            self.new_sentence(now_us);
            self.sentence_since = None;
            if self.side != 0.0 {
                self.sway_to(rules, 0.0, now_us);
            }
        }
        let mut out = BodyOffsets::ZERO;
        if let Some((at, size)) = self.nod {
            let share = now_us.saturating_sub(at) as f64 / rules.nod_us as f64;
            if share < 1.0 {
                out = add(out, rules.nod, size * bump(share));
                names.push("nodding on a stressed word");
            }
        }
        let roll = self.roll(rules, now_us);
        // A sway is over once its time has passed, not when it is level:
        // it starts level.
        if self
            .sway
            .is_some_and(|(at, _)| now_us.saturating_sub(at) >= rules.sway_us)
        {
            self.sway = None;
        }
        if roll != 0.0 {
            out.head_roll += roll;
            if speaking {
                names.push("swaying between sentences");
            }
        }
        if let Some(at) = self.settle {
            let share = now_us.saturating_sub(at) as f64 / rules.settle_us as f64;
            if share < 1.0 {
                out = add(out, rules.settle, bump(share));
                names.push("settling as it finishes");
            } else {
                self.settle = None;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Speaking {
        let head = |pitch: f64| BodyOffsets {
            head_pitch: pitch,
            ..BodyOffsets::ZERO
        };
        Speaking {
            nod: head(0.035),
            nod_us: 360_000,
            nod_gap_us: 900_000,
            stress_level: 0.35,
            stress_rise: 1.5,
            sway_roll: 0.025,
            pause_us: 350_000,
            sway_us: 600_000,
            settle: head(0.03),
            settle_us: 600_000,
            settle_after_us: 1_500_000,
        }
    }

    const TICK: u64 = 20_000;

    /// Speech at `level(t)` from `from_us` for `ticks` of 20 ms: each tick's
    /// time, offsets and names.
    fn run(
        motion: &mut SpeechMotion,
        from_us: u64,
        ticks: u64,
        speaking: bool,
        level: impl Fn(u64) -> f32,
    ) -> Vec<(u64, BodyOffsets, Vec<&'static str>)> {
        (0..ticks)
            .map(|tick| {
                let at = from_us + tick * TICK;
                let mut names = Vec::new();
                let out = motion.step(&rules(), speaking, level(at), false, at, &mut names);
                (at, out, names)
            })
            .collect()
    }

    #[test]
    fn a_stressed_word_nods_the_head_and_a_steady_voice_does_not() {
        let mut motion = SpeechMotion::default();
        // Half a second at a steady level, then a jump.
        let ticks = run(&mut motion, 1_000_000, 50, true, |at| {
            if at < 1_500_000 { 0.25 } else { 0.6 }
        });
        let before: Vec<_> = ticks.iter().filter(|(at, _, _)| *at < 1_500_000).collect();
        assert!(before.iter().all(|(_, out, _)| out.head_pitch == 0.0));
        let deepest = ticks
            .iter()
            .map(|(_, out, _)| out.head_pitch)
            .fold(0.0, f64::max);
        assert!((0.7 * 0.035 - 1e-9..=0.035).contains(&deepest), "{deepest}");
        assert!(
            ticks
                .iter()
                .any(|(_, _, names)| names.contains(&"nodding on a stressed word"))
        );
        // In and out smoothly, back to level within the nod's length.
        let after: Vec<_> = ticks.iter().filter(|(at, _, _)| *at >= 1_500_000).collect();
        assert!(
            after
                .windows(2)
                .all(|w| (w[1].1.head_pitch - w[0].1.head_pitch).abs() < 0.01)
        );
        assert_eq!(after.last().unwrap().1.head_pitch, 0.0);
    }

    fn nod_starts(ticks: &[(u64, BodyOffsets, Vec<&'static str>)]) -> Vec<u64> {
        ticks
            .windows(2)
            .filter(|w| w[0].1.head_pitch == 0.0 && w[1].1.head_pitch > 0.0)
            .map(|w| w[1].0)
            .collect()
    }

    /// The first builder on #2030, 20 Oct 03:22: a loud voice nodded every
    /// 2 s. One sentence of stressed words, one every 0.3 s for 4 s with
    /// no pause, nods once.
    #[test]
    fn one_sentence_nods_once_however_many_its_stressed_words() {
        let mut motion = SpeechMotion::default();
        let ticks = run(&mut motion, 1_000_000, 200, true, |at| {
            if (at / 20_000) % 15 < 5 { 0.1 } else { 0.7 }
        });
        assert_eq!(nod_starts(&ticks).len(), 1, "{:?}", nod_starts(&ticks));
    }

    /// Sentences of 1.5 s with half a second between, each with stressed
    /// words: never two nods within 4 s, at most one a sentence, and not all
    /// alike.
    #[test]
    fn nods_are_never_closer_than_four_seconds_and_not_all_alike() {
        let mut motion = SpeechMotion::default();
        let ticks = run(&mut motion, 1_000_000, 600, true, |at| {
            let into = (at - 1_000_000) % 2_000_000;
            if into >= 1_500_000 {
                0.0
            } else if (into / 20_000) % 15 < 5 {
                0.2
            } else {
                0.7
            }
        });
        let starts = nod_starts(&ticks);
        assert!(starts.len() >= 3, "{starts:?}");
        assert!(
            starts.windows(2).all(|w| w[1] - w[0] >= NOD_SPACING_US),
            "{starts:?}"
        );
        // Six sentences in 12 s: fewer nods than sentences.
        assert!(starts.len() <= 3, "{starts:?}");
        let sizes: Vec<_> = (1..=4)
            .map(|count| 0.7 + 0.3 * fraction(count, 1))
            .collect();
        assert!(
            sizes.windows(2).any(|w| (w[0] - w[1]).abs() > 0.02),
            "{sizes:?}"
        );
    }

    #[test]
    fn after_a_pause_the_next_sentence_sways_the_head_to_the_other_side() {
        let mut motion = SpeechMotion::default();
        // Speech, a 0.5 s pause, speech, a 0.5 s pause, speech.
        let level = |at: u64| {
            if (2_000_000..2_500_000).contains(&at) || (4_000_000..4_500_000).contains(&at) {
                0.0
            } else {
                0.3
            }
        };
        let ticks = run(&mut motion, 1_000_000, 300, true, level);
        let roll_at = |at: u64| ticks.iter().find(|(t, _, _)| *t >= at).unwrap().1.head_roll;
        assert_eq!(roll_at(1_900_000), 0.0);
        let first = roll_at(3_900_000);
        let second = roll_at(6_000_000);
        assert!(
            first.abs() >= 0.75 * 0.025 - 1e-9 && first.abs() <= 0.025,
            "{first}"
        );
        assert!(second.abs() >= 0.75 * 0.025 - 1e-9 && first.signum() != second.signum());
        // Eased over, never a jump.
        let fastest = ticks
            .windows(2)
            .map(|w| (w[1].1.head_roll - w[0].1.head_roll).abs())
            .fold(0.0, f64::max);
        assert!(fastest < 0.003, "{fastest}");
        // A short breath inside a sentence is not a pause.
        let mut breath = SpeechMotion::default();
        let ticks = run(&mut breath, 1_000_000, 150, true, |at| {
            if (2_000_000..2_200_000).contains(&at) {
                0.0
            } else {
                0.3
            }
        });
        assert!(ticks.iter().all(|(_, out, _)| out.head_roll == 0.0));
    }

    #[test]
    fn a_long_answer_ends_with_a_settle_and_the_head_comes_back_level() {
        let mut motion = SpeechMotion::default();
        run(&mut motion, 1_000_000, 100, true, |_| 0.3);
        let after = run(&mut motion, 3_000_000, 50, false, |_| 0.0);
        assert!(
            after
                .iter()
                .any(|(_, _, names)| names.contains(&"settling as it finishes"))
        );
        let deepest = after
            .iter()
            .map(|(_, out, _)| out.head_pitch)
            .fold(0.0, f64::max);
        assert!((deepest - 0.03).abs() < 0.002, "{deepest}");
        assert_eq!(after.last().unwrap().1, BodyOffsets::ZERO);
        // A short one does not.
        let mut short = SpeechMotion::default();
        run(&mut short, 1_000_000, 25, true, |_| 0.3);
        let after = run(&mut short, 1_500_000, 50, false, |_| 0.0);
        assert!(after.iter().all(|(_, out, _)| *out == BodyOffsets::ZERO));
    }

    #[test]
    fn held_or_cut_in_on_nothing_moves_and_nothing_is_owed() {
        let mut motion = SpeechMotion::default();
        run(&mut motion, 1_000_000, 100, true, |at| {
            if (at / 20_000) % 20 < 5 { 0.1 } else { 0.7 }
        });
        let mut names = Vec::new();
        let out = motion.step(&rules(), false, 0.0, true, 3_000_000, &mut names);
        assert_eq!(out, BodyOffsets::ZERO);
        let after = run(&mut motion, 3_020_000, 50, false, |_| 0.0);
        assert!(
            after
                .iter()
                .all(|(_, out, names)| *out == BodyOffsets::ZERO && names.is_empty())
        );
    }

    #[test]
    fn rules_outside_the_limits_are_refused() {
        let limits = BodyOffsets {
            base_yaw: 0.1,
            shoulder: 0.025,
            elbow: 0.025,
            head_roll: 0.065,
            head_pitch: 0.085,
        };
        assert!(rules().check(limits).is_ok());
        let mut deep = rules();
        deep.nod.head_pitch = 0.1;
        assert!(deep.check(limits).is_err());
        let mut wide = rules();
        wide.sway_roll = 0.07;
        assert!(wide.check(limits).is_err());
        let mut fast = rules();
        fast.nod_gap_us = 100_000;
        assert!(fast.check(limits).is_err());
    }
}
