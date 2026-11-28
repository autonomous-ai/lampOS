//! What the lamp's head does for what the lamp is doing: a wanted pose for a
//! mood, a voice level and a time. Pure arithmetic. Limits, speed and whether
//! anything moves at all belong to the engine that asks.
//!
//! A pose is a share of the reach the engine allows around the head's home:
//! 1 is all of it, in pitch, roll and yaw. Positive pitch dips the head;
//! negative lifts it.
use serde::{Deserialize, Serialize};

/// What the lamp is doing, as far as its body is concerned. The same four as
/// its face.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mood {
    /// Nobody has its attention: at rest where it was found.
    #[default]
    Resting,
    /// Someone is speaking to it: head up a little and tilted, as a dog does.
    Listening,
    /// They have finished and it has not answered yet: it looks away and up.
    Thinking,
    /// It is speaking: back on the person, with small nods on its own voice.
    Speaking,
    /// A skill has it read what is on the desk in front of it: the head
    /// looks down and the arm comes forward a little, held for as long as
    /// the skill says so. The body sets the head's dip itself, in counts.
    Reading,
}

/// A wanted offset from home, each axis as a share of its reach, -1 to 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub pitch: f32,
    pub roll: f32,
    pub yaw: f32,
}

/// The voice level follows the syllables at this pace, and its own average
/// at the slower one; a nod is the difference between the two.
const VOICE_FAST_US: f32 = 120_000.0;
const VOICE_SLOW_US: f32 = 900_000.0;
const LONGEST_STEP_US: u64 = 100_000;

/// The look away while the lamp thinks begins as a glance and deepens only
/// if the thinking goes on: a glance at once ([`Gestures::glancing`]), held
/// for [`GLANCE_US`], and then the rest of the way over [`DEEPEN_US`].
///
/// A person asked something easy flicks their eyes aside and is back as
/// they begin to answer; asked something hard, they look away for as long
/// as it takes. The lamp used to start on its whole look away at once. On
/// lamp-4ace on 2026-10-14, with its joints read from the bus through
/// three short answers, it had thought for 1.2 to 1.4 s when its voice
/// began, its base was 8 to 12 degrees round by then, and it was back and
/// still 0.7 to 1.2 s later: it spent each answer turning back to the
/// person it was answering.
///
/// Only the head goes on: the base holds the glance. On lamp-4ace on 20 Oct
/// a slow first answer (3.5 s) let the base turn 82 counts more from about
/// 2 s in, and the microphone took the servo for someone speaking (the
/// first builder on #1722, 04:36). A person thinking a while looks away
/// with the eyes and the head, not the whole body, and the base is the
/// lamp's loud joint (Fourth and Thirteen, #1722 05:1x).
const GLANCE_US: f32 = 800_000.0;
const DEEPEN_US: f32 = 4_000_000.0;
/// The whole look away, as a share of the turn the engine allows.
pub const AWAY: f32 = 0.9;
/// The pose the lamp listens in, which thinking starts from and a glance
/// keeps: its lift, and its tilt.
const LISTENING_PITCH: f32 = -0.4;
const LISTENING_ROLL: f32 = 0.85;

/// A thinking that goes on past its deep pose wanders around it (#7418,
/// rule 8; the first builder's yes on #8432, 24 Nov): on lamp-06c1 a make
/// thought on with no turn held one pose for 119 s, every joint still to
/// a tenth of a degree, and two minutes of that read as a machine stuck,
/// not a lamp thinking. A person thinking a while drifts and settles. The
/// wander is an offset in degrees ([`Gestures::wander`]), at most these
/// either way: pitch, roll, and the base's yaw. Big enough to see across a
/// room, small enough to stay thinking. Pitch and roll were 3 and 2 until
/// #8432's row on lamp-06c1 (25 Nov) saw the head all but still: the
/// first builder asked for a few degrees of both (17:35).
pub const WANDER_DEGREES: [f32; 3] = [3.5, 2.5, 4.0];
/// The wander is a chain of steps: an eased move to a new point around the
/// deep pose, then a short hold there, each step this long in seconds at
/// least and at most, the length drawn anew each time, so there is no
/// beat to it.
pub const WANDER_STEP_S: (f32, f32) = (2.0, 6.0);
/// The share of a step held still at its point: half a second to a second
/// and a half. With the eased ends of the moves on either side, the head is
/// never still for long (under [`WANDER_STILL_MOST_S`]).
const WANDER_HOLD_SHARE: f32 = 0.25;
/// The longest the wander leaves the head still, seconds (the first
/// builder's "no stillness past about 6 s", 25 Nov), held by the steps'
/// lengths and holds above. Tests and the docs check against it.
pub const WANDER_STILL_MOST_S: f32 = 6.0;
/// The fastest a move is asked to go at its middle, degrees a second: a
/// move that would go faster takes longer instead. Slow, so it drifts and
/// never jerks, and inside the body's own speed bounds (`FOLDED`'s 50
/// counts, 4.4 degrees, a second), which still apply.
pub const WANDER_PEAK_DEGREES_S: f32 = 3.0;
/// Each step one axis leads, drawn anew: it crosses to the other side of
/// the deep pose, to at least this share of its degrees, so every step is a
/// move the eye can see, and one the joint starts for. The others go
/// anywhere inside theirs.
const WANDER_LEAD_SHARE: f32 = 0.8;
/// The least an axis moves in a step, degrees: one that would move less
/// stays where it is. 2 degrees is 23 counts, over the body's
/// `WAKE_COUNTS` (20), so every move is one the joint starts for and is
/// kept awake through, never one it rests behind (#7418).
pub const WANDER_LEAST_MOVE_DEGREES: f32 = 2.0;

/// The wander at a time ([`Gestures::wander`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wander {
    /// The offset, degrees: pitch, roll and the base's yaw.
    pub degrees: [f32; 3],
    /// While a move is under way, its whole length on each axis, degrees,
    /// unsigned; 0 in a hold and with no wander. The body keeps a joint
    /// awake through a move it can see, rather than leaving it at rest
    /// until it is 20 counts behind ([`crate::body`]'s `WAKE_COUNTS`).
    pub moving: [f32; 3],
}

/// A number from 0 to under 1 from `seed`, the same for the same seed
/// (splitmix64): the wander's draws are from the exchange count and the
/// step, never chance.
fn draw(seed: u64) -> f32 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// Step `index` of an exchange's wander, starting at `from`: where it goes,
/// how long its move takes and how long it then holds, seconds.
fn wander_step(exchanges: u32, index: u32, from: [f32; 3]) -> ([f32; 3], f32, f32) {
    let seed = (u64::from(exchanges) << 32 | u64::from(index)) * 8;
    let lead = ((draw(seed) * 3.0) as usize).min(2);
    let to: [f32; 3] = std::array::from_fn(|axis| {
        let most = WANDER_DEGREES[axis];
        let share = draw(seed + 2 + axis as u64);
        if axis == lead {
            let side = if from[axis] > 0.0 {
                -1.0
            } else if from[axis] < 0.0 || draw(seed + 1) < 0.5 {
                1.0
            } else {
                -1.0
            };
            side * most * (WANDER_LEAD_SHARE + (1.0 - WANDER_LEAD_SHARE) * share)
        } else {
            let to = most * (2.0 * share - 1.0);
            if (to - from[axis]).abs() < WANDER_LEAST_MOVE_DEGREES {
                from[axis]
            } else {
                to
            }
        }
    });
    let (short, long) = WANDER_STEP_S;
    let length = short + (long - short) * draw(seed + 5);
    let hold = length * WANDER_HOLD_SHARE;
    let farthest = (0..3)
        .map(|axis| (to[axis] - from[axis]).abs())
        .fold(0.0, f32::max);
    // A smootherstep's middle is 1.875 times its average pace.
    let moving = (length - hold).max(1.875 * farthest / WANDER_PEAK_DEGREES_S);
    (to, moving, hold)
}

#[derive(Clone, Debug)]
pub struct Gestures {
    mood: Mood,
    /// Counts the times someone began to speak to it, so that a tilt is not
    /// always to the same side.
    exchanges: u32,
    voice: f32,
    voice_average: f32,
    last_us: Option<u64>,
    /// When the mood it is in began.
    since_us: u64,
    /// The glance a look away begins with, as a share of the turn.
    glance: f32,
}

impl Default for Gestures {
    /// With nothing said of a glance, the look away is whole from the
    /// start: within a reach of a few degrees the whole of it is a glance.
    fn default() -> Self {
        Self {
            mood: Mood::default(),
            exchanges: 0,
            voice: 0.0,
            voice_average: 0.0,
            last_us: None,
            since_us: 0,
            glance: AWAY,
        }
    }
}

impl Gestures {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin every look away with this share of the turn, and go the rest
    /// of the way only if the thinking goes on. No more than the whole
    /// look away, which is what a lamp that is told nothing does at once.
    #[must_use]
    pub fn glancing(self, share: f32) -> Self {
        Self {
            glance: share.clamp(0.0, AWAY),
            ..self
        }
    }

    /// The pose wanted at `now_us`. `level` is how loud the voice in play is,
    /// 0 to 255: the lamp's own while it speaks.
    pub fn target(&mut self, mood: Mood, level: u8, now_us: u64) -> Pose {
        let step_us = self
            .last_us
            .map_or(0, |last| now_us.saturating_sub(last).min(LONGEST_STEP_US))
            as f32;
        self.last_us = Some(now_us);
        if mood != self.mood {
            if mood == Mood::Listening {
                self.exchanges = self.exchanges.wrapping_add(1);
            }
            self.mood = mood;
            self.since_us = now_us;
        }
        let heard = f32::from(level) / 255.0;
        self.voice += (heard - self.voice) * follow(step_us, VOICE_FAST_US);
        self.voice_average += (heard - self.voice_average) * follow(step_us, VOICE_SLOW_US);
        // Listening and the thinking that follows it share one side.
        let side = if self.exchanges % 2 == 1 { 1.0 } else { -1.0 };
        match mood {
            Mood::Resting | Mood::Reading => Pose::default(),
            Mood::Listening => Pose {
                pitch: LISTENING_PITCH,
                roll: LISTENING_ROLL * side,
                yaw: 0.0,
            },
            Mood::Thinking => {
                // How far into the whole look away it is: the glance for
                // the length of a glance, then all of it, eased at both
                // ends. The lift and the tilt go on; the base's turn stays
                // the glance.
                let thought_us = now_us.saturating_sub(self.since_us) as f32;
                let share = ((thought_us - GLANCE_US) / DEEPEN_US).clamp(0.0, 1.0);
                let eased = share * share * (3.0 - 2.0 * share);
                let glance = self.glance / AWAY;
                let deep = glance + (1.0 - glance) * eased;
                Pose {
                    pitch: LISTENING_PITCH - 0.3 * deep,
                    roll: (LISTENING_ROLL - 1.25 * deep) * side,
                    yaw: self.glance * side,
                }
            }
            Mood::Speaking => {
                // Louder than its recent average: the head dips a little.
                let nod = (self.voice - self.voice_average).clamp(-0.5, 0.5);
                Pose {
                    // Small: a head that swings while the lamp talks reverses
                    // hard at every syllable, and on lamp-4ace one such
                    // reversal read a load of 217 and ended the run.
                    pitch: -0.2 + 0.3 * nod,
                    roll: 0.0,
                    yaw: 0.0,
                }
            }
        }
    }
}

impl Gestures {
    /// The wander around a long thinking's deep pose at `now_us`, in
    /// degrees of pitch, roll and the base's yaw ([`WANDER_DEGREES`]): none
    /// but in thinking past its deep pose, as [`Gestures::target`] last saw
    /// the mood, so a person's voice (listening) ends it at the next step.
    /// None while the lamp keeps still for someone (`still`, the voice's
    /// care hold, #7689): that hold's still body wins. Its first move
    /// starts from the deep pose itself, eased, so the head never jumps.
    pub fn wander(&self, now_us: u64, still: bool) -> Wander {
        if self.mood != Mood::Thinking || still {
            return Wander::default();
        }
        let deep_us = GLANCE_US + DEEPEN_US;
        let past_us = now_us.saturating_sub(self.since_us) as f32 - deep_us;
        if past_us <= 0.0 {
            return Wander::default();
        }
        let past = past_us / 1_000_000.0;
        let (mut from, mut start, mut index) = ([0.0f32; 3], 0.0f32, 0u32);
        loop {
            let (to, moving, hold) = wander_step(self.exchanges, index, from);
            if past < start + moving + hold || index == u32::MAX {
                let share = ((past - start) / moving).clamp(0.0, 1.0);
                // Smootherstep: no jerk at either end of a move.
                let eased = share * share * share * (share * (share * 6.0 - 15.0) + 10.0);
                return Wander {
                    degrees: std::array::from_fn(|axis| {
                        from[axis] + (to[axis] - from[axis]) * eased
                    }),
                    moving: std::array::from_fn(|axis| {
                        if share < 1.0 {
                            (to[axis] - from[axis]).abs()
                        } else {
                            0.0
                        }
                    }),
                };
            }
            (from, start, index) = (to, start + moving + hold, index + 1);
        }
    }
}

fn follow(step_us: f32, pace_us: f32) -> f32 {
    1.0 - (-step_us / pace_us).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No mood asks for more than the whole reach. The engine clamps again.
    const REACH: Pose = Pose {
        pitch: 1.0,
        roll: 1.0,
        yaw: 1.0,
    };

    #[test]
    fn every_pose_stays_within_a_few_degrees_of_where_the_head_was_found() {
        let mut gestures = Gestures::new();
        let mut now = 1_000u64;
        for round in 0..40u64 {
            for mood in [
                Mood::Resting,
                Mood::Listening,
                Mood::Thinking,
                Mood::Speaking,
            ] {
                for frame in 0..50u64 {
                    now += 20_000;
                    let level = ((frame * 37 + round * 11) % 256) as u8;
                    let pose = gestures.target(mood, level, now);
                    assert!(pose.pitch.abs() <= REACH.pitch, "{mood:?} {pose:?}");
                    assert!(pose.roll.abs() <= REACH.roll, "{mood:?} {pose:?}");
                    assert!(pose.yaw.abs() <= REACH.yaw, "{mood:?} {pose:?}");
                }
            }
        }
    }

    #[test]
    fn rest_is_exactly_where_it_was_found() {
        let mut gestures = Gestures::new();
        gestures.target(Mood::Speaking, 255, 1_000);
        assert_eq!(gestures.target(Mood::Resting, 255, 21_000), Pose::default());
    }

    #[test]
    fn listening_lifts_and_tilts_and_the_side_alternates_between_exchanges() {
        let mut gestures = Gestures::new();
        let first = gestures.target(Mood::Listening, 0, 1_000);
        assert!(first.pitch < 0.0 && first.roll != 0.0 && first.yaw == 0.0);
        let away = gestures.target(Mood::Thinking, 0, 21_000);
        // It looks away to the side its head was tilted from, and up.
        assert!(away.yaw * first.roll > 0.0 && away.pitch < first.pitch);
        // Told nothing of a glance, that is the whole look away at once.
        assert!((away.yaw.abs() - 0.9).abs() < 1e-6 && (away.pitch + 0.7).abs() < 1e-6);
        assert!((away.roll.abs() - 0.4).abs() < 1e-6);
        gestures.target(Mood::Speaking, 0, 41_000);
        gestures.target(Mood::Resting, 0, 61_000);
        let second = gestures.target(Mood::Listening, 0, 81_000);
        assert_eq!(second.roll, -first.roll);
    }

    #[test]
    fn the_look_away_is_a_glance_that_deepens_only_while_the_thinking_goes_on() {
        const GLANCE: f32 = 0.15;
        let mut gestures = Gestures::new().glancing(GLANCE);
        let start = 5_000_000u64;
        let listening = gestures.target(Mood::Listening, 0, start - 20_000);
        let at = |gestures: &mut Gestures, ms: u64| {
            gestures.target(Mood::Thinking, 0, start + ms * 1_000)
        };
        // At once, and for as long as an easy answer takes: a glance.
        let glance = at(&mut gestures, 0);
        assert!((glance.yaw.abs() - GLANCE).abs() < 1e-6, "{glance:?}");
        // The lift and the tilt have gone a sixth of their way with it.
        assert!(
            (glance.pitch - (listening.pitch - 0.05)).abs() < 1e-6,
            "{glance:?}"
        );
        assert!(glance.roll * listening.roll > 0.0 && glance.roll.abs() < listening.roll.abs());
        for ms in [100, 400, 800] {
            assert_eq!(at(&mut gestures, ms), glance, "{ms}");
        }
        // Then the head goes further, a little at first and never back,
        // while the base holds the glance: on lamp-4ace a base turning on
        // from 2 s into a slow answer was heard as someone speaking.
        let mut before = glance;
        for ms in [1_000, 1_400, 2_000, 2_800, 3_600, 4_400, 4_800] {
            let now = at(&mut gestures, ms);
            assert_eq!(now.yaw, glance.yaw, "{ms} {now:?}");
            assert!(now.pitch < before.pitch, "{ms} {now:?}");
            // The tilt goes on over to the other side.
            let side = listening.roll.signum();
            assert!(now.roll * side < before.roll * side, "{ms} {now:?}");
            before = now;
        }
        // After 4.8 s the head is all the way, and stays; the base is where
        // its glance put it.
        let whole = at(&mut gestures, 4_800);
        assert_eq!(whole.yaw, glance.yaw, "{whole:?}");
        assert!((whole.pitch + 0.7).abs() < 1e-6, "{whole:?}");
        assert!((whole.roll.abs() - 0.4).abs() < 1e-6 && whole.roll * glance.roll < 0.0);
        assert_eq!(at(&mut gestures, 30_000), whole);
        // A glance is never more than the whole look away, which a lamp
        // told nothing of a glance makes at once.
        let wide = Gestures::new().glancing(7.0).target(Mood::Thinking, 0, 1);
        assert!((wide.yaw.abs() - 0.9).abs() < 1e-6, "{wide:?}");
        let whole_at_once = Gestures::new().target(Mood::Thinking, 0, 1);
        assert!(
            (whole_at_once.yaw.abs() - 0.9).abs() < 1e-6,
            "{whole_at_once:?}"
        );
        // The next time it thinks, it begins with a glance again, to the
        // other side.
        gestures.target(Mood::Speaking, 0, start + 31_000_000);
        gestures.target(Mood::Listening, 0, start + 32_000_000);
        let again = gestures.target(Mood::Thinking, 0, start + 33_000_000);
        assert_eq!(again.yaw, -glance.yaw);
    }

    /// #7418, rule 8 (the first builder's yes on #8432): a thinking that
    /// goes on past its deep pose wanders around it, slowly and within its
    /// degrees; before the deep pose nothing changes; a person's voice ends
    /// it at the next step, and the care hold keeps the head still.
    #[test]
    fn a_long_thinking_wanders_around_its_deep_pose_and_a_short_one_never() {
        let start = 5_000_000u64;
        let deep_us = (GLANCE_US + DEEPEN_US) as u64;
        let mut gestures = Gestures::new().glancing(0.15);
        let mut today = Gestures::new().glancing(0.15);
        gestures.target(Mood::Listening, 0, start - 20_000);
        today.target(Mood::Listening, 0, start - 20_000);
        // Up to the deep pose: the pose is today's, with no wander at all.
        let mut at = start;
        while at <= start + deep_us {
            assert_eq!(
                gestures.target(Mood::Thinking, 0, at),
                today.target(Mood::Thinking, 0, at)
            );
            assert_eq!(gestures.wander(at, false), Wander::default(), "{at}");
            at += 20_000;
        }
        // It starts from the deep pose: no jump.
        let first = gestures.wander(start + deep_us + 20_000, false);
        assert!(
            first.degrees.iter().all(|degrees| degrees.abs() < 0.01),
            "{first:?}"
        );
        // Over the next minute: never past its degrees, and never faster
        // than its peak between steps 20 ms apart.
        let mut before = first;
        let mut at = start + deep_us + 20_000;
        while at <= start + deep_us + 60_000_000 {
            gestures.target(Mood::Thinking, 0, at);
            let wander = gestures.wander(at, false);
            for (axis, &degrees) in wander.degrees.iter().enumerate() {
                assert!(
                    degrees.abs() <= WANDER_DEGREES[axis] + 1e-4,
                    "{at} {wander:?}"
                );
                assert!(
                    (degrees - before.degrees[axis]).abs() <= WANDER_PEAK_DEGREES_S * 0.02 + 1e-3,
                    "{at} {before:?} {wander:?}"
                );
            }
            before = wander;
            at += 20_000;
        }
        // Kept still for someone (the care hold): no wander.
        assert_eq!(gestures.wander(at, true), Wander::default());
        // The same time, the same wander: from time alone, no chance.
        assert_eq!(gestures.wander(at, false), gestures.wander(at, false));
        assert_ne!(gestures.wander(at, false), Wander::default());
        // A person's voice: listening, and the wander is over at once.
        gestures.target(Mood::Listening, 0, at + 20_000);
        assert_eq!(gestures.wander(at + 20_000, false), Wander::default());
    }

    /// #7418, #8432's row on lamp-06c1 (25 Nov): over 115 s of thinking
    /// the head's pitch moved 1.14 degrees, its roll none, and the lamp was
    /// still for 13.1 s once. Over ten long thinkings of two minutes each,
    /// pitch and roll each go most of their degrees either way, every axis
    /// moves on in steps of no fixed length, and the wander is never still
    /// longer than `WANDER_STILL_MOST_S`.
    #[test]
    fn a_long_thinking_moves_the_heads_pitch_and_roll_and_is_never_still_for_long() {
        let deep_us = (GLANCE_US + DEEPEN_US) as u64;
        let mut lengths = Vec::new();
        for exchange in 0..10u64 {
            let mut gestures = Gestures::new().glancing(0.15);
            let start = 1_000_000 + exchange * 1_000_000_000;
            // Each listening is a new exchange.
            gestures.target(Mood::Listening, 0, start - 20_000);
            let mut low = [f32::MAX; 3];
            let mut high = [f32::MIN; 3];
            // Still: no axis a tenth of a degree from where it last was.
            let mut still_from = start + deep_us;
            let mut still_at = [0.0f32; 3];
            let mut longest_still = 0u64;
            let mut moving_from = None;
            let mut at = start + deep_us;
            while at <= start + deep_us + 120_000_000 {
                gestures.target(Mood::Thinking, 0, at);
                let wander = gestures.wander(at, false);
                for axis in 0..3 {
                    low[axis] = low[axis].min(wander.degrees[axis]);
                    high[axis] = high[axis].max(wander.degrees[axis]);
                }
                if (0..3).any(|axis| (wander.degrees[axis] - still_at[axis]).abs() >= 0.1) {
                    still_at = wander.degrees;
                    still_from = at;
                }
                longest_still = longest_still.max(at - still_from);
                // Every axis that moves goes far enough for its joint to
                // start: none rests behind a small move.
                for degrees in wander.moving {
                    assert!(
                        degrees == 0.0 || degrees >= WANDER_LEAST_MOVE_DEGREES - 1e-4,
                        "exchange {exchange}: {wander:?}"
                    );
                }
                // The length of each move, to see that they differ.
                let moving = wander.moving.iter().any(|degrees| *degrees > 0.0);
                match (moving, moving_from) {
                    (true, None) => moving_from = Some(at),
                    (false, Some(from)) => {
                        lengths.push(at - from);
                        moving_from = None;
                    }
                    _ => {}
                }
                at += 20_000;
            }
            assert!(
                longest_still as f32 <= WANDER_STILL_MOST_S * 1e6,
                "exchange {exchange}: still {longest_still} us"
            );
            for axis in 0..3 {
                assert!(
                    high[axis] >= 0.6 * WANDER_DEGREES[axis]
                        && low[axis] <= -0.6 * WANDER_DEGREES[axis],
                    "exchange {exchange}, axis {axis}: {low:?} {high:?}"
                );
            }
        }
        // No fixed rhythm: the moves' lengths spread over seconds.
        let shortest = *lengths.iter().min().unwrap();
        let longest = *lengths.iter().max().unwrap();
        assert!(longest - shortest >= 2_000_000, "{shortest} {longest}");
        lengths.sort_unstable();
        lengths.dedup_by(|a, b| a.abs_diff(*b) < 100_000);
        assert!(lengths.len() >= 10, "{lengths:?}");
    }

    #[test]
    fn speaking_nods_on_the_voice_and_settles_when_it_is_steady() {
        let mut gestures = Gestures::new();
        let mut now = 1_000u64;
        let mut quiet = Pose::default();
        for _ in 0..100 {
            now += 20_000;
            quiet = gestures.target(Mood::Speaking, 0, now);
        }
        // A syllable: the head dips within a tenth of a second.
        let mut loud = quiet;
        for _ in 0..5 {
            now += 20_000;
            loud = gestures.target(Mood::Speaking, 255, now);
        }
        assert!(loud.pitch > quiet.pitch + 0.1, "{quiet:?} {loud:?}");
        // Held loud, it comes back: a nod, not a new posture.
        let mut held = loud;
        for _ in 0..200 {
            now += 20_000;
            held = gestures.target(Mood::Speaking, 255, now);
        }
        assert!(
            (held.pitch - quiet.pitch).abs() < 0.03,
            "{quiet:?} {held:?}"
        );
        assert_eq!((held.roll, held.yaw), (0.0, 0.0));
    }
}
