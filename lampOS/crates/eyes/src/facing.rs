//! Whether a face is turned toward the lamp, and whether someone has been
//! facing it for the last moment: the evidence a person gives, without
//! meaning to, that what they are saying is said to the lamp.
//!
//! V1 had this (`hal/drivers/tracking/gaze.py`) and used it when speech
//! began. It needs no model beyond the face detector: the detector already
//! gives the two eyes and the nose, and how far the nose sits from the
//! point between the eyes, along the line of the eyes, is the sine of how
//! far the head is turned. It says nothing of where the eyes themselves
//! look. With `LAMP_EYES_PITCH_GAZE=on` (#7442) the same marks also say
//! how far the head is tipped up or down ([`pitch_deg`]).
//!
//! Nothing here decides anything. It turns marks into an angle, an angle
//! into "facing or not" for one look, and a run of looks into a vote. The
//! numbers are V1's and were tuned on V1's lamps at twice this picture's
//! width; none has been checked on this lamp.

/// A face turned further than this is not facing the lamp. V1's default
/// was 25 and its lamps were set to 60 after trying them with people: a
/// wide lens and a person looking at a screen beside the lamp.
pub const MAX_YAW_DEG: f32 = 60.0;
/// Toward the edge of the picture the lens shows a face turned even when
/// it looks straight at the lamp, so the allowance grows to this many
/// times `MAX_YAW_DEG` at the very edge (and is held to 90).
pub const EDGE_SCALE: f32 = 1.8;
/// A face narrower than this share of the picture's width has marks too
/// close together to measure a turn from. V1: 48 pixels of 640.
pub const LEAST_WIDTH: f32 = 0.075;
/// The vote looks this far back, needs this many looks, and this share of
/// them facing.
pub const WINDOW_US: u64 = 1_500_000;
pub const LEAST_LOOKS: usize = 2;
pub const LEAST_FACING: f32 = 0.6;

/// How far the head is turned, in degrees from straight at the camera, to
/// either side. `marks` are the eye on the left of the picture, the eye on
/// the right and the nose. None when the eyes are on top of one another or
/// a mark is not a number.
pub fn yaw_deg(marks: &[(f32, f32); 5]) -> Option<f32> {
    let (first, second, nose) = (marks[0], marks[1], marks[2]);
    if [first, second, nose]
        .iter()
        .any(|mark| !mark.0.is_finite() || !mark.1.is_finite())
    {
        return None;
    }
    let (across, down) = (second.0 - first.0, second.1 - first.1);
    let apart = across.hypot(down);
    if apart < 1e-6 {
        return None;
    }
    let between = ((first.0 + second.0) / 2.0, (first.1 + second.1) / 2.0);
    // The nose's distance from the middle, along the line of the eyes.
    let along = ((nose.0 - between.0) * across + (nose.1 - between.1) * down) / apart;
    // Past an eye the marks have nothing more to say.
    Some(
        (along / (apart / 2.0))
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees()
            .abs(),
    )
}

/// How far below the eye line the nose sits on a level face, as a share of
/// the distance between the eyes: the zero of [`pitch_deg`]. From the
/// five-point template faces are aligned to for recognition (InsightFace's
/// ArcFace 112x112: eyes at y 51.70 and 51.50, 35.24 apart, nose at 71.74),
/// itself a mean of frontal faces. Not yet checked on this lamp's YuNet.
pub const LEVEL_NOSE_DROP: f32 = 0.57;
/// How far in front of the eyes the nose's tip stands, as a share of the
/// distance between them: a head tipped down swings it down, tipped up,
/// up. An anthropometric guess (about 30 mm against 63 mm), for the row.
pub const NOSE_DEPTH: f32 = 0.5;
/// How far below the eye line the mouth corners' midpoint sits on a level
/// face, in eye distances: the same ArcFace template (corners at y 92.37
/// and 92.20, 40.69 below the eyes' 51.60).
pub const LEVEL_MOUTH_DROP: f32 = 1.15;
/// How far in front of the eyes the mouth corners stand, in eye distances:
/// less than the nose's tip. A guess, for the row.
pub const MOUTH_DEPTH: f32 = 0.2;
/// Pitch is measured only up to this turn: past it the marks crowd into
/// one side of the face and say too little.
pub const PITCH_MOST_YAW_DEG: f32 = 60.0;
/// What [`pitch_deg`] gives for marks no head could make (a mark not a
/// number, the eyes on top of one another, the mouth above the eyes with
/// the nose below them): straight down, outside every window, so a lost
/// pitch never counts as facing. Not the same as not measured (`None`).
pub const LOST_DEG: f32 = 90.0;
/// A face tipped further down than this is not facing the lamp: a head
/// bent to a phone or the desk (#7442). Authored, to be set on the row.
pub const MAX_PITCH_DOWN_DEG: f32 = 25.0;
/// A face tipped further up than this is not facing the lamp (#7442).
/// Authored, to be set on the row.
pub const MAX_PITCH_UP_DEG: f32 = 20.0;

/// How far the head is tipped, in degrees: positive down, negative up, as
/// ROS has pitch (REP 103: about the head's left axis, nose down positive).
/// `marks` are YuNet's five: the eye on the left of the picture, the eye on
/// the right, the nose, and the two mouth corners.
///
/// The nose's drop `r` and the mouth corners' midpoint's drop `m` below the
/// point between the eyes, square to the eye line (so a head tilted over
/// reads the same). A head tipped by `t` gives, with `c = cos t` and
/// `s = sin t`, `r = LEVEL_NOSE_DROP c + NOSE_DEPTH s` and
/// `m = LEVEL_MOUTH_DROP c + MOUTH_DEPTH s`: two lines in `c` and `s`,
/// solved for them, and `t = atan2(s, c)`. That is one answer for every
/// bow, so the reading rises with it to straight down (the nose alone
/// folded back past about 41 degrees: the Reviewer-lead's P3 on #7450).
/// Only the ratio of `s` to `c` counts, so the face's size and the turn's
/// narrowing of it (both drops alike) fall out. A level face, `r` and `m`
/// at [`LEVEL_NOSE_DROP`] and [`LEVEL_MOUTH_DROP`] eye distances, reads 0.
///
/// [`LOST_DEG`] for marks no head could make, `c` not over 0 among them;
/// None, not measured, for a head turned past [`PITCH_MOST_YAW_DEG`].
pub fn pitch_deg(marks: &[(f32, f32); 5]) -> Option<f32> {
    let (first, second, nose) = (marks[0], marks[1], marks[2]);
    let (across, down) = (second.0 - first.0, second.1 - first.1);
    let apart = across.hypot(down);
    if marks
        .iter()
        .any(|mark| !mark.0.is_finite() || !mark.1.is_finite())
        || apart < 1e-6
    {
        return Some(LOST_DEG);
    }
    if yaw_deg(marks)? > PITCH_MOST_YAW_DEG {
        return None;
    }
    let between = ((first.0 + second.0) / 2.0, (first.1 + second.1) / 2.0);
    // Square to the eye line, toward the bottom of the picture.
    let drop = |at: (f32, f32)| (at.0 - between.0) * -down + (at.1 - between.1) * across;
    let mouth = (
        (marks[3].0 + marks[4].0) / 2.0,
        (marks[3].1 + marks[4].1) / 2.0,
    );
    let (r, m) = (drop(nose), drop(mouth));
    let det = LEVEL_NOSE_DROP * MOUTH_DEPTH - NOSE_DEPTH * LEVEL_MOUTH_DROP;
    let c = (r * MOUTH_DEPTH - NOSE_DEPTH * m) / det;
    let s = (LEVEL_NOSE_DROP * m - LEVEL_MOUTH_DROP * r) / det;
    if c <= 0.0 {
        return Some(LOST_DEG);
    }
    Some(s.atan2(c).to_degrees())
}

/// Whether a face tipped `pitch` degrees ([`pitch_deg`]) is inside the
/// window that counts as facing the lamp. A pitch not measured is not
/// judged here: that is the reader's to leave unknown.
pub fn pitch_within(pitch: f32) -> bool {
    (-MAX_PITCH_UP_DEG..=MAX_PITCH_DOWN_DEG).contains(&pitch)
}

/// Whether the eyes and the nose were all inside the picture: a mark the
/// detector placed outside it was guessed, not seen.
pub fn marks_seen(marks: &[(f32, f32); 5], width: f32, height: f32) -> bool {
    marks[..3].iter().all(|mark| {
        mark.0.is_finite()
            && mark.1.is_finite()
            && (0.0..=width).contains(&mark.0)
            && (0.0..=height).contains(&mark.1)
    })
}

/// The turn allowed to a face this far from the middle of the picture:
/// 0 at the middle, 1 at an edge.
pub fn allowed_deg(from_middle: f32) -> f32 {
    (MAX_YAW_DEG * (1.0 + (EDGE_SCALE - 1.0) * from_middle.clamp(0.0, 1.0))).min(90.0)
}

/// One look at one face: is it facing the lamp. A face too small to
/// measure never counts, for or against, and that is the caller's to
/// leave out of the vote: here it is simply not facing.
pub fn facing(yaw: Option<f32>, width_share: f32, from_middle: f32) -> bool {
    yaw.is_some_and(|yaw| width_share >= LEAST_WIDTH && yaw <= allowed_deg(from_middle))
}

/// The looks of the last moment, for the vote taken when someone begins to
/// speak.
#[derive(Default)]
pub struct Recent {
    looks: std::collections::VecDeque<(u64, bool)>,
}

impl Recent {
    /// One look in which a face large enough to measure was seen.
    pub fn saw(&mut self, at_us: u64, facing: bool) {
        self.looks.push_back((at_us, facing));
        self.forget(at_us);
    }

    fn forget(&mut self, now_us: u64) {
        while self
            .looks
            .front()
            .is_some_and(|(at, _)| now_us.saturating_sub(*at) > WINDOW_US)
        {
            self.looks.pop_front();
        }
    }

    /// Whether someone has been facing the lamp: none when there were too
    /// few looks to say, which is not the same as nobody facing it.
    pub fn vote(&mut self, now_us: u64) -> Option<bool> {
        self.forget(now_us);
        if self.looks.len() < LEAST_LOOKS {
            return None;
        }
        let facing = self.looks.iter().filter(|(_, facing)| *facing).count();
        Some(facing as f32 / self.looks.len() as f32 >= LEAST_FACING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face 100 across with its eyes 40 apart and its nose `turn` of the
    /// way from the middle toward the right eye (1) or the left (-1).
    fn marks(turn: f32) -> [(f32, f32); 5] {
        let nose = 50.0 + 20.0 * turn;
        [
            (30.0, 40.0),
            (70.0, 40.0),
            (nose, 60.0),
            (35.0, 80.0),
            (65.0, 80.0),
        ]
    }

    #[test]
    fn the_nose_between_the_eyes_is_straight_and_at_an_eye_is_side_on() {
        assert!(yaw_deg(&marks(0.0)).unwrap() < 0.01);
        assert!((yaw_deg(&marks(0.5)).unwrap() - 30.0).abs() < 0.01);
        assert!((yaw_deg(&marks(-0.5)).unwrap() - 30.0).abs() < 0.01);
        assert!((yaw_deg(&marks(1.0)).unwrap() - 90.0).abs() < 0.01);
        // Past the eye is held at side on.
        assert!((yaw_deg(&marks(1.7)).unwrap() - 90.0).abs() < 0.01);
        // A head tilted over: the turn is measured along the tilted eyes.
        let tilted = [
            (30.0, 30.0),
            (70.0, 50.0),
            (45.0, 60.0),
            (0.0, 0.0),
            (0.0, 0.0),
        ];
        let straight = yaw_deg(&tilted).unwrap();
        assert!(straight < 12.0, "{straight}");
        // No eyes to measure from, or a mark that is no number.
        let mut none = marks(0.0);
        none[1] = none[0];
        assert_eq!(yaw_deg(&none), None);
        let mut broken = marks(0.0);
        broken[2].0 = f32::NAN;
        assert_eq!(yaw_deg(&broken), None);
    }

    /// Marks of a face with its eyes 40 apart, level, its head tipped
    /// `tip` degrees (down positive) by the model [`pitch_deg`] inverts:
    /// the nose and the mouth corners each dropped by their own depth.
    fn tipped(tip: f32) -> [(f32, f32); 5] {
        let (sin, cos) = tip.to_radians().sin_cos();
        let nose = 40.0 * (LEVEL_NOSE_DROP * cos + NOSE_DEPTH * sin);
        let mouth = 40.0 * (LEVEL_MOUTH_DROP * cos + MOUTH_DEPTH * sin);
        [
            (30.0, 40.0),
            (70.0, 40.0),
            (50.0, 40.0 + nose),
            (35.0, 40.0 + mouth),
            (65.0, 40.0 + mouth),
        ]
    }

    #[test]
    fn a_level_face_reads_no_pitch_and_a_nose_low_reads_head_down() {
        assert!(pitch_deg(&tipped(0.0)).unwrap().abs() < 0.01);
        // The ArcFace template itself reads level: the zero's source.
        let template = [
            (38.29, 51.70),
            (73.53, 51.50),
            (56.03, 71.74),
            (41.55, 92.37),
            (70.73, 92.20),
        ];
        let level = pitch_deg(&template).unwrap();
        assert!(level.abs() < 2.0, "{level}");
        for tip in [-30.0, -20.0, -10.0, 10.0, 20.0, 30.0] {
            let read = pitch_deg(&tipped(tip)).unwrap();
            assert!((read - tip).abs() < 0.01, "{tip}: {read}");
        }
        // A nose lower than level is down, higher is up.
        let mut low = tipped(0.0);
        low[2].1 += 6.0;
        assert!(pitch_deg(&low).unwrap() > 10.0);
        let mut high = tipped(0.0);
        high[2].1 -= 6.0;
        assert!(pitch_deg(&high).unwrap() < -10.0);
        // Bowed to a phone, or looking up at a shelf: outside the window.
        assert!(!pitch_within(pitch_deg(&tipped(35.0)).unwrap()));
        assert!(!pitch_within(pitch_deg(&tipped(-25.0)).unwrap()));
        assert!(pitch_within(pitch_deg(&tipped(15.0)).unwrap()));
        assert!(pitch_within(pitch_deg(&tipped(-15.0)).unwrap()));
    }

    /// The Reviewer-lead's P3 on #7450: the nose alone folds past about 41
    /// degrees down, so a phone's deep bow read back inside the window. With
    /// the mouth too, the reading rises with the bow all the way down.
    #[test]
    fn a_deep_bow_reads_deeper_never_back_inside_the_window() {
        let tips = [0.0, 20.0, 45.0, 60.0, 75.0];
        let reads: Vec<f32> = tips
            .iter()
            .map(|&tip| pitch_deg(&tipped(tip)).unwrap())
            .collect();
        assert!(reads.windows(2).all(|pair| pair[0] < pair[1]), "{reads:?}");
        for (read, tip) in reads.iter().zip(tips) {
            assert!((read - tip).abs() < 0.01, "{tip}: {read}");
        }
        assert!(!pitch_within(reads[3]) && !pitch_within(reads[4]));
        // Head far back too: never back inside.
        let up = pitch_deg(&tipped(-60.0)).unwrap();
        assert!((up + 60.0).abs() < 0.01 && !pitch_within(up), "{up}");
    }

    /// Marks that can't be a face read as lost, outside the window: never
    /// facing. Only a turn past sixty is not measured at all.
    #[test]
    fn marks_that_cannot_be_read_are_lost_and_not_facing() {
        assert!(!pitch_within(LOST_DEG));
        let mut nan_nose = tipped(0.0);
        nan_nose[2].1 = f32::NAN;
        let mut nan_mouth = tipped(0.0);
        nan_mouth[4].0 = f32::NAN;
        let mut no_eyes = tipped(0.0);
        no_eyes[1] = no_eyes[0];
        // The mouth above the eyes and the nose below them: no head does it.
        let mut flipped = tipped(0.0);
        flipped[3].1 = 10.0;
        flipped[4].1 = 10.0;
        for marks in [nan_nose, nan_mouth, no_eyes, flipped] {
            assert_eq!(pitch_deg(&marks), Some(LOST_DEG), "{marks:?}");
        }
        // Turned past sixty: not measured.
        let mut side_on = tipped(0.0);
        side_on[2].0 = 69.0;
        assert_eq!(pitch_deg(&side_on), None);
    }

    #[test]
    fn pitch_is_the_same_with_the_head_tilted_over_or_turned_a_little() {
        // The whole face turned 30 degrees in the picture's plane.
        let (sin, cos) = 30f32.to_radians().sin_cos();
        let mut tilted = tipped(20.0);
        for mark in &mut tilted {
            let (x, y) = (mark.0 - 50.0, mark.1 - 40.0);
            *mark = (50.0 + x * cos - y * sin, 40.0 + x * sin + y * cos);
        }
        assert!((pitch_deg(&tilted).unwrap() - 20.0).abs() < 0.01);
        // A level head turned 40 degrees: its eyes look closer together,
        // which must not read as the nose dropping.
        let turn = 40f32.to_radians();
        let apart = 40.0 * turn.cos();
        let along = apart / 2.0 * turn.sin();
        let turned = [
            (30.0, 40.0),
            (30.0 + apart, 40.0),
            (30.0 + apart / 2.0 + along, 40.0 + 40.0 * LEVEL_NOSE_DROP),
            (30.0, 40.0 + 40.0 * LEVEL_MOUTH_DROP),
            (30.0 + apart, 40.0 + 40.0 * LEVEL_MOUTH_DROP),
        ];
        assert!((yaw_deg(&turned).unwrap() - 40.0).abs() < 0.01);
        let read = pitch_deg(&turned).unwrap();
        assert!(read.abs() < 0.01, "{read}");
    }

    #[test]
    fn a_small_face_or_a_guessed_mark_does_not_count_and_the_edge_allows_more() {
        // At the middle: up to sixty degrees is facing.
        assert!(facing(Some(59.0), 0.2, 0.0));
        assert!(!facing(Some(61.0), 0.2, 0.0));
        // At the edge of a wide lens the allowance grows, and stops at 90.
        assert!(facing(Some(80.0), 0.2, 1.0));
        assert!((allowed_deg(0.5) - 84.0).abs() < 0.01);
        assert_eq!(allowed_deg(1.0), 90.0);
        assert_eq!(allowed_deg(7.0), 90.0);
        // Too small to measure, or not measured at all.
        assert!(!facing(Some(0.0), 0.05, 0.0));
        assert!(!facing(None, 0.5, 0.0));
        // A nose the detector put outside the picture was not seen.
        assert!(marks_seen(&marks(0.0), 100.0, 100.0));
        let mut outside = marks(0.0);
        outside[2].0 = 104.0;
        assert!(!marks_seen(&outside, 100.0, 100.0));
        outside[2].0 = -1.0;
        assert!(!marks_seen(&outside, 100.0, 100.0));
        // The mouth is not used, so it may be anywhere.
        let mut mouth = marks(0.0);
        mouth[4] = (500.0, 500.0);
        assert!(marks_seen(&mouth, 100.0, 100.0));
    }

    #[test]
    fn the_vote_needs_two_looks_in_the_last_moment_and_most_of_them_facing() {
        const MS: u64 = 1_000;
        let mut recent = Recent::default();
        assert_eq!(recent.vote(0), None);
        recent.saw(100 * MS, true);
        assert_eq!(recent.vote(200 * MS), None);
        recent.saw(350 * MS, true);
        assert_eq!(recent.vote(400 * MS), Some(true));
        // Three of five facing is enough; two of five is not.
        recent.saw(600 * MS, false);
        recent.saw(850 * MS, false);
        recent.saw(1_100 * MS, true);
        assert_eq!(recent.vote(1_150 * MS), Some(true));
        let mut away = Recent::default();
        for (at, facing) in [
            (0, true),
            (250, false),
            (500, false),
            (750, false),
            (1_000, true),
        ] {
            away.saw(at * MS, facing);
        }
        assert_eq!(away.vote(1_050 * MS), Some(false));
        // Looks older than a second and a half are forgotten: a person who
        // faced the lamp and left is not still facing it.
        assert_eq!(recent.vote(1_100 * MS + WINDOW_US + 1), None);
    }
}
