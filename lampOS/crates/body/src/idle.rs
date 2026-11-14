//! When a lamp standing at rest shifts a little by itself, and how.
//!
//! Not on a clock: each wait is drawn at random, from 40 s to 3 min. A lamp
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

/// The small moves a lamp at rest shifts by: the old runtime's idle sway,
/// and four moves of the head alone inside the reach of a glance at rest
/// (`body::REST_GLANCE_REACH`): a tilt to either side, a look up and a dip.
pub const MOVES: [&Clip; 5] = [
    &clips::IDLE,
    &clips::TILT_LEFT,
    &clips::TILT_RIGHT,
    &clips::LOOK_UP,
    &clips::DIP,
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
}

impl Idler {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed,
            last: None,
            share: 1.0,
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
        let drawn = GAP_LEAST.as_secs_f64() + (GAP_MOST - GAP_LEAST).as_secs_f64() * share;
        Duration::from_secs_f64(drawn * self.share)
    }

    /// The next move, the last one a third as likely as each other. The two
    /// tilts only for a head that tilts when it stands (`--stand-roll` past
    /// `--reach`'s roll): elsewhere their roll is left out and both are the
    /// same small lift. Moves of the arm only for an arm no hand holds
    /// (`arm`): one held is not swayed into the hand.
    pub fn pick(&mut self, tilts: bool, arm: bool) -> &'static Clip {
        let open: Vec<&'static Clip> = MOVES
            .into_iter()
            .filter(|shift| tilts || !is_tilt(shift))
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
        for shift in MOVES {
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
}
