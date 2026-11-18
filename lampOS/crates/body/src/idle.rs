//! When a lamp standing at rest shifts a little by itself, and how.
//!
//! Not on a clock: each wait is drawn at random, from 40 s to 3 min, or
//! from 20 s to 60 s under `LAMP_BODY_PRESENCE` (#7301). A lamp
//! left alone folds (`--awake-seconds`) and folded never shifts, which is
//! how it is still when nobody is near. Each shift is one of a few small
//! moves, the one made last less likely than each other but not ruled out:
//! with only two to choose from, ruling it out would be strict turns, a
//! pattern of its own. Pure arithmetic: when the lamp may shift at all
//! (standing, at rest, nobody talking, no hand on it, no other gesture)
//! belongs to the body that asks.

use crate::clips::{self, Clip};
use std::time::Duration;

/// The shortest and the longest wait drawn.
pub const GAP_LEAST: Duration = Duration::from_secs(40);
pub const GAP_MOST: Duration = Duration::from_secs(180);

/// The same under `LAMP_BODY_PRESENCE` (#7301): someone at the desk sees
/// a lamp that is alive move at least about once a minute.
pub const ALIVE_GAP_LEAST: Duration = Duration::from_secs(20);
pub const ALIVE_GAP_MOST: Duration = Duration::from_secs(60);

/// The small moves a lamp at rest shifts by: the old runtime's idle sway,
/// and four moves of the head alone inside the reach of a glance at rest
/// (`body::REST_GLANCE_REACH`): a tilt to either side, a look up and a dip;
/// and under `LAMP_BODY_PRESENCE` only, the arm's settle (#7301).
pub const MOVES: [&Clip; 6] = [
    &clips::IDLE,
    &clips::TILT_LEFT,
    &clips::TILT_RIGHT,
    &clips::LOOK_UP,
    &clips::DIP,
    &clips::SETTLE,
];

/// How much likelier each other move is than the one made last.
const OTHERS_WEIGH: u64 = 3;

/// Whether this clip is one of the shifts at rest.
pub fn is_move(clip: &Clip) -> bool {
    is_move_name(clip.name)
}

/// Whether a move of this name is one of the shifts at rest.
pub fn is_move_name(name: &str) -> bool {
    MOVES.iter().any(|shift| shift.name == name)
}

/// Draws the waits and the moves, from a seed.
#[derive(Clone, Copy, Debug)]
pub struct Idler {
    state: u64,
    last: Option<&'static str>,
    /// Each wait drawn is scaled by this: 1 is today, an energy level's
    /// `idle_share` shortens them (`crate::energy`, #6893).
    pub share: f64,
    /// Under `LAMP_BODY_PRESENCE` (#7301): waits drawn from
    /// [`ALIVE_GAP_LEAST`] to [`ALIVE_GAP_MOST`], and [`clips::SETTLE`]
    /// among the moves. Off: as before.
    pub alive: bool,
}

impl Idler {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed,
            last: None,
            share: 1.0,
            alive: false,
        }
    }

    /// splitmix64: any seed, zero too, gives a full-period stream.
    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// The wait before the next shift.
    pub fn gap(&mut self) -> Duration {
        let share = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        let (least, most) = if self.alive {
            (ALIVE_GAP_LEAST, ALIVE_GAP_MOST)
        } else {
            (GAP_LEAST, GAP_MOST)
        };
        let drawn = least.as_secs_f64() + (most - least).as_secs_f64() * share;
        Duration::from_secs_f64(drawn * self.share)
    }

    /// The next move, the last one a third as likely as each other. The two
    /// tilts only for a head that tilts when it stands (`--stand-roll` past
    /// `--reach`'s roll): elsewhere their roll is left out and both are the
    /// same small lift. Moves of the arm only for an arm no hand holds
    /// (`arm`): one held is not swayed into the hand. The settle only while
    /// [`Idler::alive`].
    pub fn pick(&mut self, tilts: bool, arm: bool) -> &'static Clip {
        let open: Vec<&'static Clip> = MOVES
            .into_iter()
            .filter(|shift| tilts || !is_tilt(shift))
            .filter(|shift| self.alive || shift.name != clips::SETTLE.name)
            .filter(|shift| arm || shift.frames.iter().all(|frame| frame[3..] == [0; 2]))
            .collect();
        self.pick_from(&open)
    }

    fn pick_from(&mut self, open: &[&'static Clip]) -> &'static Clip {
        let last = self.last;
        let weight = |shift: &Clip| {
            if Some(shift.name) == last {
                1
            } else {
                OTHERS_WEIGH
            }
        };
        let total: u64 = open.iter().map(|shift| weight(shift)).sum();
        let mut left = self.next() % total;
        let mut chosen = open[0];
        for shift in open {
            if left < weight(shift) {
                chosen = shift;
                break;
            }
            left -= weight(shift);
        }
        self.last = Some(chosen.name);
        chosen
    }
}

/// Under `LAMP_BODY_PRESENCE` (#7301): a shift's arm `offset` from the
/// arm's `goal`, scaled by `size` and taken toward the fold whatever its
/// sign, and cut so the arm stays on its way between `folded` and `stood`,
/// as the lease's pose is: a shift only ever lowers a standing arm.
pub fn sag(offset: f32, size: f32, goal: f32, folded: f32, stood: f32) -> f32 {
    let toward = if folded < stood { -1.0 } else { 1.0 };
    (goal + offset.abs() * size * toward).clamp(folded.min(stood), folded.max(stood)) - goal
}

fn is_tilt(clip: &Clip) -> bool {
    clip.name == clips::TILT_LEFT.name || clip.name == clips::TILT_RIGHT.name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_are_irregular_and_inside_their_bounds() {
        let mut idler = Idler::new(7);
        let gaps: Vec<Duration> = (0..1_000).map(|_| idler.gap()).collect();
        assert!(gaps.iter().all(|gap| (GAP_LEAST..=GAP_MOST).contains(gap)));
        let mut sorted = gaps.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), gaps.len(), "two waits alike");
        // Spread over the whole range, not bunched: each fifth of it is hit.
        let span = (GAP_MOST - GAP_LEAST).as_secs_f64();
        for fifth in 0..5 {
            let hits = gaps
                .iter()
                .filter(|gap| {
                    ((gap.as_secs_f64() - GAP_LEAST.as_secs_f64()) / span * 5.0) as usize == fifth
                })
                .count();
            assert!((120..=280).contains(&hits), "fifth {fifth}: {hits}");
        }
        // Another seed, other waits.
        assert_ne!(Idler::new(8).gap(), gaps[0]);
    }

    #[test]
    fn a_move_is_seldom_the_one_before_and_all_of_them_come() {
        let mut idler = Idler::new(11);
        let picks: Vec<&str> = (0..1_300).map(|_| idler.pick(true, true).name).collect();
        // The last one weighs 1 against 3 for each of the other four: a
        // repeat about one time in 13.
        let repeats = picks.windows(2).filter(|pair| pair[0] == pair[1]).count();
        assert!((50..=150).contains(&repeats), "{repeats}");
        for shift in &MOVES[..5] {
            let count = picks.iter().filter(|name| **name == shift.name).count();
            assert!((200..=320).contains(&count), "{}: {count}", shift.name);
        }
        assert!(MOVES.iter().all(|shift| is_move(shift)));
        assert!(!is_move(&clips::CALLED) && !is_move(&clips::NOD));
        // A head that does not tilt standing: no tilts, the other three.
        let picks: Vec<&str> = (0..300).map(|_| idler.pick(false, true).name).collect();
        assert!(
            picks.iter().all(|name| !name.starts_with("tilt")),
            "{picks:?}"
        );
        for name in ["idle", "look_up", "dip"] {
            assert!(picks.contains(&name), "{name}");
        }
        // An arm held by a hand: the head alone.
        assert!((0..100).all(|_| idler.pick(true, false).name != clips::IDLE.name));
    }

    /// A share scales every wait and keeps their spread: level 5's half
    /// draws 20 s to 90 s, and 1 is the waits of today, exactly.
    #[test]
    fn a_share_scales_the_waits() {
        let today: Vec<Duration> = {
            let mut idler = Idler::new(3);
            (0..500).map(|_| idler.gap()).collect()
        };
        let mut lively = Idler::new(3);
        lively.share = 0.5;
        for gap in &today {
            let half = lively.gap();
            assert!((half.as_secs_f64() - gap.as_secs_f64() / 2.0).abs() < 1e-6);
            assert!((GAP_LEAST / 2..=GAP_MOST / 2).contains(&half), "{half:?}");
        }
        let mut same = Idler::new(3);
        same.share = 1.0;
        assert!(today.iter().all(|gap| *gap == same.gap()));
    }

    /// With only two moves, not strict turns: a repeat about one time in
    /// four, and no long run of either.
    #[test]
    fn two_moves_do_not_take_strict_turns() {
        let two = [&clips::IDLE, &clips::LOOK_UP];
        for seed in 0..20 {
            let mut idler = Idler::new(seed);
            let picks: Vec<&str> = (0..200).map(|_| idler.pick_from(&two).name).collect();
            let repeats = picks.windows(2).filter(|pair| pair[0] == pair[1]).count();
            assert!((25..=80).contains(&repeats), "seed {seed}: {repeats}");
            let longest = picks
                .chunk_by(|a, b| a == b)
                .map(|run| run.len())
                .max()
                .unwrap();
            assert!(longest <= 8, "seed {seed}: {longest}");
            assert_ne!(longest, 1, "seed {seed}: strict turns");
        }
    }

    /// Under `LAMP_BODY_PRESENCE` (#7301): waits of 20 s to 60 s, spread
    /// over all of it, and the arm's sag among the moves; off, never.
    #[test]
    fn alive_it_shifts_every_twenty_to_sixty_seconds_and_settles() {
        let mut idler = Idler::new(7);
        idler.alive = true;
        let gaps: Vec<Duration> = (0..1_000).map(|_| idler.gap()).collect();
        assert!(
            gaps.iter()
                .all(|gap| (Duration::from_secs(20)..=Duration::from_secs(60)).contains(gap)),
            "{:?}",
            &gaps[..5]
        );
        for fifth in 0..5 {
            let hits = gaps
                .iter()
                .filter(|gap| ((gap.as_secs_f64() - 20.0) / 40.0 * 5.0) as usize == fifth)
                .count();
            assert!((120..=280).contains(&hits), "fifth {fifth}: {hits}");
        }
        let picks: Vec<&str> = (0..600).map(|_| idler.pick(false, true).name).collect();
        let settles = picks
            .iter()
            .filter(|name| **name == clips::SETTLE.name)
            .count();
        assert!((100..=200).contains(&settles), "{settles}");
        assert!(is_move(&clips::SETTLE));
        // An arm a hand holds is never sagged into the hand.
        assert!((0..100).all(|_| idler.pick(true, false).name != clips::SETTLE.name));
        let mut today = Idler::new(7);
        assert!((0..600).all(|_| today.pick(true, true).name != clips::SETTLE.name));
    }

    /// Every shift's arm offset, at every size, goes only toward the fold
    /// and keeps the arm on its way, whichever way the fold lies.
    #[test]
    fn a_sag_only_lowers_the_arm_and_stays_on_its_way() {
        let mut idler = Idler::new(19);
        for (folded, stood) in [(2_050.0, 2_650.0), (2_600.0, 2_047.0)] {
            let toward = if folded < stood { -1.0 } else { 1.0 };
            for _ in 0..200 {
                let clip = idler.pick(true, true);
                let size = [1.0, 1.05, 1.1, 1.15, 1.2][(idler.next() % 5) as usize];
                // Anywhere on the way, the stand itself most often.
                let goal = if idler.next().is_multiple_of(2) {
                    stood
                } else {
                    folded + (stood - folded) * (idler.next() % 100) as f32 / 100.0
                };
                for frame in clip.frames {
                    for offset in &frame[3..] {
                        let offset = f32::from(*offset);
                        let sagged = sag(offset, size, goal, folded, stood);
                        let at = goal + sagged;
                        assert!(
                            (folded.min(stood)..=folded.max(stood)).contains(&at),
                            "{} {offset} {goal}: {at}",
                            clip.name
                        );
                        assert!(sagged * toward >= 0.0, "{} {offset}: {sagged}", clip.name);
                        assert!(sagged.abs() <= offset.abs() * size + 1e-3);
                        if (goal - stood).abs() < 1e-3 {
                            assert!(
                                (sagged.abs() - offset.abs() * size).abs() < 1e-3,
                                "{} {offset}: {sagged}",
                                clip.name
                            );
                        }
                    }
                }
            }
        }
    }
}
