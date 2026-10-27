//! A yawn, from the mesh of the largest face (#3709, V1's "yawning shortens
//! the break"): the inner lips held wide apart for [`HOLD_US`], with no smile
//! and the inner brows not raised. Nothing is kept but a count on
//! `room/activity` (`activity::Judge::yawned`): no picture, no score, no time.
//!
//! The mouth's gap alone is not a yawn. On the screen man's generated clips
//! (Ten on #3709, 27 Oct 20:0x, at 4 looks a second), a broad open smile held
//! `mouth_open` at 0.18 to 0.20 for 5 s and surprise at 0.10 to 0.14 for 5 s,
//! while his yawns peaked at 0.14 to 0.19. Two Blendshape V2 scores the
//! product already reads tell them apart: of the looks with the mouth at
//! [`OPEN_FROM`] or wider, 29 of his yawn's 30 had the smiles under
//! [`SMILE_UNDER`] and the inner brows under [`BROW_UP_UNDER`], and none of
//! the smile's 61, the surprise's 30 or the talk's 3. His yawns held that
//! open for 1 to 1.5 s, shorter than a person's: the hold is to be checked
//! with a person at a lamp.

/// `on` for the eyes to count yawns; with `--mouth-model` and
/// `--seems-blendshapes`, and only while the face-expressions yes is kept.
pub const ENVIRONMENT: &str = "LAMP_EYES_YAWN";
/// The inner lips' gap over the face's height, from which the mouth is open
/// as a yawn opens it (`mesh::mouth_open`).
pub const OPEN_FROM: f32 = 0.08;
/// Under this the mouth has closed, and the next yawn may count.
pub const CLOSED_UNDER: f32 = 0.03;
/// How long the mouth is held open for a yawn.
pub const HOLD_US: u64 = 1_500_000;
/// The looks a hold needs, so two looks far apart are not one.
pub const LEAST_LOOKS: usize = 4;
/// Looks in a hold that may fail it, as a look the mesh misreads.
pub const MOST_MISSES: usize = 1;
/// Looks further apart than this are not one hold.
pub const MOST_GAP_US: u64 = 1_000_000;
/// The smiles' mean, left and right, under which an open mouth is not a laugh.
pub const SMILE_UNDER: f32 = 0.3;
/// The inner brows raised under this: an open mouth that is not surprise.
pub const BROW_UP_UNDER: f32 = 0.5;

/// What one look shows of the largest face's mouth: its `mouth_open`, and,
/// when it was open enough for them to be read, the smiles' mean and the
/// inner brows raised.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mouth {
    pub open: f32,
    pub smile: Option<f32>,
    pub brow_up: Option<f32>,
}

impl Mouth {
    /// Wide open, with no smile and the brows not raised.
    fn yawning(&self) -> bool {
        self.open >= OPEN_FROM
            && self.smile.is_some_and(|smile| smile < SMILE_UNDER)
            && self.brow_up.is_some_and(|up| up < BROW_UP_UNDER)
    }
}

/// Whether a look's mouth is open enough for its scores to be read.
pub fn wants_scores(open: f32) -> bool {
    open >= OPEN_FROM
}

/// The yawn being held, if one is.
#[derive(Debug, Default)]
pub struct Yawns {
    /// When the hold began, its looks and its misses.
    hold: Option<(u64, usize, usize)>,
    /// The last look, so a gap ends a hold.
    last_us: Option<u64>,
    /// A yawn was counted, and the mouth has not closed since.
    counted: bool,
    /// Since when no face has been seen, if none is now.
    unseen_from: Option<u64>,
}

impl Yawns {
    /// One look of the largest face's mouth (`None`: no face, or no mesh).
    /// True once, as a yawn is counted.
    pub fn look(&mut self, mouth: Option<Mouth>, now_us: u64) -> bool {
        // A gap, or no face for longer than one: whatever was held is over,
        // and a face that left with its mouth open does not block the next
        // yawn (the reviewer-lead's P3 on #3842). One missed mesh mid-yawn
        // is not a face gone, so a long yawn still counts once (their P3 on
        // `eea699cd5`).
        if self
            .last_us
            .is_some_and(|last| now_us.saturating_sub(last) > MOST_GAP_US)
        {
            self.hold = None;
            self.counted = false;
        }
        self.last_us = Some(now_us);
        let Some(mouth) = mouth else {
            self.miss();
            let unseen = *self.unseen_from.get_or_insert(now_us);
            if now_us.saturating_sub(unseen) > MOST_GAP_US {
                self.counted = false;
            }
            return false;
        };
        self.unseen_from = None;
        if mouth.open < CLOSED_UNDER {
            self.counted = false;
            self.hold = None;
            return false;
        }
        if self.counted {
            return false;
        }
        if !mouth.yawning() {
            self.miss();
            return false;
        }
        let (from, looks, _) = self.hold.get_or_insert((now_us, 0, 0));
        *looks += 1;
        if *looks >= LEAST_LOOKS && now_us.saturating_sub(*from) >= HOLD_US {
            self.hold = None;
            self.counted = true;
            return true;
        }
        false
    }

    /// A look that fails the hold: one may, a second ends it.
    fn miss(&mut self) {
        if let Some((_, _, misses)) = self.hold.as_mut() {
            *misses += 1;
            if *misses > MOST_MISSES {
                self.hold = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOK_US: u64 = 250_000;

    fn yawn(open: f32) -> Option<Mouth> {
        Some(Mouth {
            open,
            smile: Some(0.0),
            brow_up: Some(0.0),
        })
    }

    fn shut() -> Option<Mouth> {
        Some(Mouth {
            open: 0.005,
            smile: None,
            brow_up: None,
        })
    }

    /// The looks given, one every 250 ms from `from_us`; how many yawns
    /// they counted.
    fn counted(yawns: &mut Yawns, looks: &[Option<Mouth>], from_us: u64) -> usize {
        looks
            .iter()
            .enumerate()
            .filter(|(nth, mouth)| yawns.look(**mouth, from_us + *nth as u64 * LOOK_US))
            .count()
    }

    fn held(mouth: Option<Mouth>, looks: usize) -> Vec<Option<Mouth>> {
        let mut all = vec![shut(); 4];
        all.extend(std::iter::repeat_n(mouth, looks));
        all.extend(vec![shut(); 4]);
        all
    }

    #[test]
    fn a_mouth_held_wide_for_the_hold_is_one_yawn() {
        let mut yawns = Yawns::default();
        // 1.5 s from the first open look to the seventh.
        assert_eq!(counted(&mut yawns, &held(yawn(0.15), 7), 0), 1);
        // Held 4 s: still one.
        let mut yawns = Yawns::default();
        assert_eq!(counted(&mut yawns, &held(yawn(0.15), 17), 0), 1);
    }

    #[test]
    fn a_short_gape_is_no_yawn() {
        let mut yawns = Yawns::default();
        assert_eq!(counted(&mut yawns, &held(yawn(0.15), 6), 0), 0);
    }

    #[test]
    fn two_yawns_with_the_mouth_closed_between_are_two() {
        let mut yawns = Yawns::default();
        let mut looks = held(yawn(0.15), 8);
        looks.extend(held(yawn(0.12), 8));
        assert_eq!(counted(&mut yawns, &looks, 0), 2);
    }

    #[test]
    fn a_mouth_that_only_half_closes_is_still_the_same_yawn() {
        let mut yawns = Yawns::default();
        let mut looks = held(yawn(0.15), 8);
        looks.truncate(looks.len() - 4);
        looks.extend(vec![yawn(0.05); 2]);
        looks.extend(vec![yawn(0.15); 8]);
        assert_eq!(counted(&mut yawns, &looks, 0), 1);
    }

    #[test]
    fn one_misread_look_is_forgiven_and_two_end_the_hold() {
        let mut looks = vec![yawn(0.15); 3];
        looks.push(yawn(0.06));
        looks.extend(vec![yawn(0.15); 4]);
        assert_eq!(counted(&mut Yawns::default(), &looks, 0), 1);
        let mut looks = vec![yawn(0.15); 3];
        looks.extend(vec![yawn(0.06); 2]);
        looks.extend(vec![yawn(0.15); 4]);
        assert_eq!(counted(&mut Yawns::default(), &looks, 0), 0);
    }

    #[test]
    fn a_held_smile_or_surprise_is_no_yawn() {
        let smile = Some(Mouth {
            open: 0.19,
            smile: Some(0.75),
            brow_up: Some(0.0),
        });
        assert_eq!(counted(&mut Yawns::default(), &held(smile, 20), 0), 0);
        let surprise = Some(Mouth {
            open: 0.12,
            smile: Some(0.0),
            brow_up: Some(0.97),
        });
        assert_eq!(counted(&mut Yawns::default(), &held(surprise, 20), 0), 0);
        let unread = Some(Mouth {
            open: 0.15,
            smile: None,
            brow_up: None,
        });
        assert_eq!(counted(&mut Yawns::default(), &held(unread, 20), 0), 0);
    }

    /// Talk opens the mouth past the line now and then, never for long:
    /// the screen man's talk clip, 4 looks a second, its smiles as read.
    #[test]
    fn talk_is_no_yawn() {
        let talk = [
            0.008, 0.024, 0.055, 0.092, 0.116, 0.045, 0.056, 0.05, 0.042, 0.04, 0.045, 0.042, 0.04,
            0.031, 0.007, 0.011, 0.003, 0.035, 0.064, 0.003, 0.004, 0.041, 0.034, 0.058, 0.041,
            0.024, 0.032, 0.037, 0.067, 0.028, 0.017, 0.118, 0.076, 0.032,
        ];
        let looks: Vec<_> = talk
            .iter()
            .map(|&open| {
                Some(Mouth {
                    open,
                    smile: Some(0.0),
                    brow_up: Some(0.0),
                })
            })
            .collect();
        assert_eq!(counted(&mut Yawns::default(), &looks, 0), 0);
    }

    #[test]
    fn looks_far_apart_are_not_one_hold() {
        let mut yawns = Yawns::default();
        for nth in 0..6 {
            assert!(!yawns.look(yawn(0.15), nth * 1_200_000));
        }
    }

    /// The face leaves with its mouth open: the next yawn still counts.
    /// One missed mesh in a long yawn is not a face gone: still one.
    #[test]
    fn a_face_gone_with_its_mouth_open_does_not_block_the_next_yawn() {
        let mut looks = vec![yawn(0.15); 8];
        looks.push(None);
        looks.extend(vec![yawn(0.15); 8]);
        assert_eq!(
            counted(&mut Yawns::default(), &looks, 0),
            1,
            "one missed mesh"
        );
        // Gone: six looks with no face, 1.25 s from the first.
        let mut looks = vec![yawn(0.15); 8];
        looks.extend(vec![None; 6]);
        looks.extend(vec![yawn(0.15); 8]);
        assert_eq!(counted(&mut Yawns::default(), &looks, 0), 2, "a face gone");
        let mut yawns = Yawns::default();
        assert!(!yawns.look(yawn(0.15), 0));
        for nth in 1..7 {
            yawns.look(yawn(0.15), nth * LOOK_US);
        }
        // Two seconds unseen, and back with the mouth open.
        for nth in 0..7 {
            assert_eq!(yawns.look(yawn(0.15), 4_000_000 + nth * LOOK_US), nth == 6);
        }
    }

    #[test]
    fn scores_are_wanted_only_for_an_open_mouth() {
        assert!(wants_scores(OPEN_FROM) && wants_scores(0.2));
        assert!(!wants_scores(0.079));
    }
}
