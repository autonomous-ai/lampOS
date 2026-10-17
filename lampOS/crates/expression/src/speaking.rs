//! The lamp's own speech in its head (#933): while it talks it should read
//! as a creature, not a speaker with a light on. Three small things, from
//! the voice's own sound as the body will meet it (the playout said ahead,
//! sampled at the body's lag):
//! - **a nod on a stressed word:** a short dip of the head when the level
//!   jumps over what it has just been, never closer than a gap to the last
//!   nod, each a little different in size;
//! - **a sway between sentences:** after a pause in its speech, as the next
//!   sentence begins, now and then (about one sentence in two or three) the
//!   base turns a little to one side and eases back to centre, the side
//!   picked afresh each time (#955: head roll this small cannot be seen from
//!   the front, where the person sits; the base turning can);
//! - **a settle as it finishes:** one soft nod when an answer of some length
//!   ends, then rest.
//!
//! The head's pitch and the base's turn, within the catalogue's limits, in
//! any room: it is the lamp's own speech. Off unless the catalogue has a `speaking` section.
use crate::Error;
use crate::catalogue::{add, ease};
use lamp_msgs::BodyOffsets;
use serde::{Deserialize, Serialize};

/// Under this the lamp is silent between words or sentences.
const QUIET_LEVEL: f32 = 0.06;
/// A sentence has begun again once the level is back over this.
const SPEAKING_LEVEL: f32 = 0.12;
/// The recent level a stressed word is measured against follows the sound
/// this fast.
const RECENT_US: f64 = 250_000.0;
/// About this share of new sentences sways the base.
const SWAY_SHARE: f64 = 0.4;

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
    /// The sway: the base's turn out from centre at most, the pause in the
    /// lamp's speech that ends a sentence, and how long the base takes to
    /// turn out (and again to turn back). Missing, as in a section written
    /// for #955's head roll, it is 0: no sway.
    #[serde(default)]
    pub sway_yaw: f64,
    /// #955's head roll, which the front cannot see: read so a catalogue
    /// that still says it starts, and otherwise ignored.
    #[serde(default, skip_serializing)]
    pub sway_roll: Option<f64>,
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
        if !self.sway_yaw.is_finite() || !(0.0..=limits.base_yaw).contains(&self.sway_yaw) {
            return Err(Error(
                "speaking: sway_yaw must be within the base yaw limit".into(),
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
    quiet_since: Option<u64>,
    paused: bool,
    spoke_since: Option<u64>,
    nod: Option<(u64, f64)>,
    nods: u64,
    /// A sway out and back: when it began and its size and side, a share
    /// of `sway_yaw`, negative to the other side.
    sway: Option<(u64, f64)>,
    /// Back to centre from where a sway was when the answer ended.
    back: Option<(u64, f64)>,
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

    /// The base's turn from the sway now: out and back in `2 * sway_us`,
    /// eased at both ends; or, after the answer, eased back to centre.
    fn yaw(&self, rules: &Speaking, now_us: u64) -> f64 {
        let share = |at: u64, length: u64| now_us.saturating_sub(at) as f64 / length as f64;
        if let Some((at, from)) = self.back {
            from * (1.0 - ease(share(at, rules.sway_us)))
        } else if let Some((at, size)) = self.sway {
            size * rules.sway_yaw * bump(share(at, 2 * rules.sway_us))
        } else {
            0.0
        }
    }

    /// One tick. `speaking`: the lamp's answer is under way; `level`: its
    /// sound as the body meets it now (0 between pieces); `still`: the body
    /// is held or concentrating, or someone cut in. Returns the body's
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
            // the body, and nothing is owed when it ends.
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
            if level < QUIET_LEVEL {
                let since = *self.quiet_since.get_or_insert(now_us);
                self.paused |= now_us.saturating_sub(since) >= rules.pause_us;
            } else {
                self.quiet_since = None;
                if self.paused && level >= SPEAKING_LEVEL {
                    // A new sentence: now and then the base turns out to a
                    // side picked afresh and eases back, a little more or
                    // less each time; never while a turn is still under way,
                    // so it is back at centre between sways (#1067 review:
                    // every sentence, side to side, read as a metronome).
                    self.paused = false;
                    self.sways += 1;
                    if self.sway.is_none()
                        && self.back.is_none()
                        && fraction(self.sways, 3) < SWAY_SHARE
                    {
                        let side = if fraction(self.sways, 4) < 0.5 {
                            1.0
                        } else {
                            -1.0
                        };
                        let size = 0.75 + 0.25 * fraction(self.sways, 2);
                        self.sway = Some((now_us, side * size));
                    }
                }
            }
            let stressed = level >= rules.stress_level && level >= self.recent * rules.stress_rise;
            if stressed
                && self
                    .nod
                    .is_none_or(|(at, _)| now_us.saturating_sub(at) >= rules.nod_gap_us)
            {
                self.nods += 1;
                self.nod = Some((now_us, 0.7 + 0.3 * fraction(self.nods, 1)));
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
            if self.sway.is_some() {
                // The answer ended mid-sway: ease back to centre from there.
                self.back = Some((now_us, self.yaw(rules, now_us)));
                self.sway = None;
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
        let yaw = self.yaw(rules, now_us);
        // A sway is over once its time has passed, not when it is centred:
        // it starts centred.
        if self
            .sway
            .is_some_and(|(at, _)| now_us.saturating_sub(at) >= 2 * rules.sway_us)
        {
            self.sway = None;
        }
        if self
            .back
            .is_some_and(|(at, _)| now_us.saturating_sub(at) >= rules.sway_us)
        {
            self.back = None;
        }
        if yaw != 0.0 {
            out.base_yaw += yaw;
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
            sway_yaw: 0.08,
            sway_roll: None,
            pause_us: 350_000,
            sway_us: 800_000,
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

    #[test]
    fn nods_are_never_closer_than_the_gap_and_not_all_alike() {
        let mut motion = SpeechMotion::default();
        // A stressed word every 0.3 s for 4 s: quiet, then loud.
        let ticks = run(&mut motion, 1_000_000, 200, true, |at| {
            if (at / 20_000) % 15 < 5 { 0.1 } else { 0.7 }
        });
        let starts: Vec<u64> = ticks
            .windows(2)
            .filter(|w| w[0].1.head_pitch == 0.0 && w[1].1.head_pitch > 0.0)
            .map(|w| w[1].0)
            .collect();
        assert!(starts.len() >= 3, "{starts:?}");
        assert!(
            starts.windows(2).all(|w| w[1] - w[0] >= 900_000),
            "{starts:?}"
        );
        assert!(starts.len() <= 5, "{starts:?}");
        let sizes: Vec<_> = (1..=4)
            .map(|count| 0.7 + 0.3 * fraction(count, 1))
            .collect();
        assert!(
            sizes.windows(2).any(|w| (w[0] - w[1]).abs() > 0.02),
            "{sizes:?}"
        );
    }

    /// Ten sentences of 2.5 s, each after a 0.5 s pause, from 1 s, then 2 s
    /// at rest; `seed` sentences said before (the counter that picks).
    fn ten_sentences(seed: u64) -> Vec<(u64, BodyOffsets, Vec<&'static str>)> {
        let mut motion = SpeechMotion {
            sways: seed,
            ..SpeechMotion::default()
        };
        let mut ticks = run(&mut motion, 1_000_000, 1_500, true, |at| {
            if (at - 1_000_000) % 3_000_000 >= 2_500_000 {
                0.0
            } else {
                0.3
            }
        });
        ticks.extend(run(&mut motion, 31_000_000, 100, false, |_| 0.0));
        ticks
    }

    #[test]
    fn some_pauses_turn_the_base_out_to_either_side_and_back_to_centre() {
        let mut first_sides = Vec::new();
        let mut sides = Vec::new();
        for seed in 0..10 {
            let ticks = ten_sentences(seed);
            let yaw_at = |at: u64| ticks.iter().find(|(t, _, _)| *t >= at).unwrap().1.base_yaw;
            // Each sentence after a pause: how far out the base went.
            let outs: Vec<f64> = (1..10)
                .map(|k| {
                    let start = 1_000_000 + k * 3_000_000;
                    ticks
                        .iter()
                        .filter(|(t, _, _)| (start..start + 2_500_000).contains(t))
                        .map(|(_, out, _)| out.base_yaw)
                        .fold(
                            0.0,
                            |far: f64, yaw| if yaw.abs() > far.abs() { yaw } else { far },
                        )
                })
                .collect();
            let swayed: Vec<f64> = outs.iter().copied().filter(|out| *out != 0.0).collect();
            // Not every pause, and each one out by 75-100% of the 0.08.
            assert!(swayed.len() < 9, "{seed}: {outs:?}");
            assert!(
                swayed
                    .iter()
                    .all(|out| (0.75 * 0.08 - 1e-9..=0.08).contains(&out.abs())),
                "{seed}: {outs:?}"
            );
            first_sides.extend(swayed.first().map(|out| out.signum()));
            sides.extend(swayed.iter().map(|out| out.signum()));
            // Back at centre before each pause ends, and after the answer.
            for k in 1..10 {
                assert_eq!(yaw_at(k * 3_000_000 + 900_000), 0.0, "{seed} {k}");
            }
            assert_eq!(ticks.last().unwrap().1.base_yaw, 0.0, "{seed}");
            // Eased out and back, never a jump; only the base.
            let fastest = ticks
                .windows(2)
                .map(|w| (w[1].1.base_yaw - w[0].1.base_yaw).abs())
                .fold(0.0, f64::max);
            assert!(fastest < 0.004, "{seed}: {fastest}");
            assert!(ticks.iter().all(|(_, out, _)| out.head_roll == 0.0));
        }
        // Some do sway, to both sides, and the first side is not fixed.
        assert!(sides.len() >= 10, "{sides:?}");
        assert!(sides.contains(&1.0) && sides.contains(&-1.0), "{sides:?}");
        assert!(
            first_sides.contains(&1.0) && first_sides.contains(&-1.0),
            "{first_sides:?}"
        );
        // A short breath inside a sentence is not a pause.
        let mut breath = SpeechMotion::default();
        let ticks = run(&mut breath, 1_000_000, 150, true, |at| {
            if (2_000_000..2_200_000).contains(&at) {
                0.0
            } else {
                0.3
            }
        });
        assert!(ticks.iter().all(|(_, out, _)| out.base_yaw == 0.0));
    }

    #[test]
    fn an_answer_that_ends_mid_sway_eases_the_base_back_to_centre_and_no_further_out() {
        // Seed 0: the first sentence after the pause sways (fraction(1, 3)
        // is under the share). The answer ends 0.3 s into that sway, still
        // on its way out.
        let mut motion = SpeechMotion::default();
        run(&mut motion, 1_000_000, 125, true, |at| {
            if at >= 3_000_000 {
                0.3
            } else if at >= 2_500_000 {
                0.0
            } else {
                0.3
            }
        });
        let ended = run(&mut motion, 3_500_000, 1, true, |_| 0.3)[0].1.base_yaw;
        assert!(ended.abs() > 0.005, "{ended}");
        let after = run(&mut motion, 3_520_000, 60, false, |_| 0.0);
        assert!(
            after
                .windows(2)
                .all(|w| w[1].1.base_yaw.abs() <= w[0].1.base_yaw.abs()),
            "{after:?}"
        );
        // At centre within one sway_us of the end.
        assert!(
            after
                .iter()
                .filter(|(at, _, _)| *at >= 3_520_000 + 800_000)
                .all(|(_, out, _)| out.base_yaw == 0.0)
        );
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
        wide.sway_yaw = 0.11;
        assert!(wide.check(limits).is_err());
        let mut fast = rules();
        fast.nod_gap_us = 100_000;
        assert!(fast.check(limits).is_err());
    }
}
