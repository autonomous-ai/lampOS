//! A face counts as someone at the lamp only when it is a confirmed person
//! (#8971, piece 1: never invent a person).
//!
//! Today any face the detector scores 0.6 or more is someone at the lamp,
//! of any size, at any angle, clipped or not. On a day with nobody at the
//! desk, lamp-dbda's welcome-back started about 1,250 times on such faces
//! (#8964). With `LAMP_EYES_CONFIRMED_PERSON=on` a face is kept only when
//! it is a good face ([`poor`] finds nothing) and a confirmed person: its
//! place has moved [`crate::away::MOVED`] of the picture within
//! [`crate::away::MOVED_WITHIN_US`] (a false face's box wanders 0.01 to 0.02,
//! `still`). Once confirmed, a face stays confirmed while it is followed
//! look to look, so a person who then sits still stays seen. A face left
//! out is left out where the still faces are (`still`): of what the eyes
//! publish (`humans/faces/tracked`, `room/presence`, the presence file,
//! `room/activity`), of what the body is told and of recognition, so every
//! face-triggered skill and lamp-seen's gate follow the confirmed person
//! with no change of their own.
//!
//! With `=count` nothing is left out: the faces that would be are only
//! counted, by reason, in the journal's minute line, so one journal holds
//! today's gate and the new one on the same frames.
//!
//! The thresholds are a start, to be set from Builder #4's offline check
//! (#8971, piece 5). The NPU's body pose as a second confirmation is the
//! next piece. ponytail: movement is the only confirmation here, so a
//! person already sitting quite still when the camera opens is confirmed
//! only once they move; the pose lifts that.

use crate::away::{BLIND_AFTER_US, MOVED, MOVED_WITHIN_US};
use crate::faces::Face;
use crate::facing;

/// `on` leaves out what is not a confirmed person; `count` only counts it.
pub const ENVIRONMENT: &str = "LAMP_EYES_CONFIRMED_PERSON";
/// A face scored under this is not a person's: real faces at the desk score
/// about 0.91; lamp-4ace's night false face 0.60 to 0.79 (`still`).
pub const LEAST_SCORE: f32 = 0.8;
/// A face shorter than this, as a share of the picture's height, is too
/// small to tell; people at the desk are 0.12 to 0.27.
pub const LEAST_TALL: f32 = 0.06;
/// A box this close to the picture's edge, as a share of it, is clipped:
/// part of what made it is outside the picture.
pub const EDGE: f32 = 0.01;
/// A face's centre this far from the last one followed, as a share of the
/// picture, across or down, is another face.
pub const FOLLOW: f32 = 0.1;
/// Faces followed at once; the one seen longest ago goes first.
const MOST: usize = 8;

/// What the switch asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    On,
    Count,
}

impl Mode {
    pub fn from(value: Option<&str>) -> Option<Mode> {
        match value {
            Some("on") => Some(Mode::On),
            Some("count") => Some(Mode::Count),
            _ => None,
        }
    }
}

/// Why a face is not a confirmed person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    Faint,
    Small,
    Clipped,
    Turned,
    Unconfirmed,
}

/// Whether a face is too poor to judge, and why: faint, small, clipped, or
/// turned too far (when its angle can be measured) or with an eye or the
/// nose outside the picture. `filled` is how much of the detector's picture
/// the camera frame fills, in its units. None: a good face. Learning or
/// comparing a face uses the same (#8971, piece 2).
pub fn poor(face: &Face, filled: (usize, usize)) -> Option<Why> {
    let (width, height) = (filled.0.max(1) as f32, filled.1.max(1) as f32);
    if face.score < LEAST_SCORE {
        return Some(Why::Faint);
    }
    if face.height / height < LEAST_TALL {
        return Some(Why::Small);
    }
    if face.x < EDGE * width
        || face.y < EDGE * height
        || face.x + face.width > (1.0 - EDGE) * width
        || face.y + face.height > (1.0 - EDGE) * height
    {
        return Some(Why::Clipped);
    }
    if !facing::marks_seen(&face.marks, width, height) {
        return Some(Why::Turned);
    }
    if face.width / width >= facing::LEAST_WIDTH {
        let yaw = facing::yaw_deg(&face.marks);
        let pitch = facing::pitch_deg(&face.marks);
        if yaw.is_some_and(|yaw| yaw > facing::MAX_YAW_DEG)
            || pitch.is_some_and(|pitch| !facing::pitch_within(pitch))
        {
            return Some(Why::Turned);
        }
    }
    None
}

#[derive(Debug)]
struct Followed {
    x: f32,
    y: f32,
    seen_us: u64,
    /// Where it was when its move was last measured from, and when.
    from: (f32, f32, u64),
    confirmed: bool,
}

/// What the minute line says.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub faces: u64,
    pub faint: u64,
    pub small: u64,
    pub clipped: u64,
    pub turned: u64,
    pub unconfirmed: u64,
}

impl Counts {
    pub fn line(&self) -> String {
        format!(
            "{} faces, not a confirmed person: {} faint, {} small, {} clipped, {} turned, {} unconfirmed",
            self.faces, self.faint, self.small, self.clipped, self.turned, self.unconfirmed
        )
    }
}

/// The faces followed, and whether each is a confirmed person.
#[derive(Debug, Default)]
pub struct Person {
    followed: Vec<Followed>,
    pub counts: Counts,
}

impl Person {
    /// One face seen now: why it is not a confirmed person, or None when it
    /// is. Every face is followed, a poor one too, so a face that turns
    /// for a moment keeps what it was.
    pub fn why_not(&mut self, face: &Face, filled: (usize, usize), now_us: u64) -> Option<Why> {
        let (width, height) = (filled.0.max(1) as f32, filled.1.max(1) as f32);
        let (x, y) = (
            (face.x + face.width / 2.0) / width,
            (face.y + face.height / 2.0) / height,
        );
        self.followed
            .retain(|one| now_us.saturating_sub(one.seen_us) <= BLIND_AFTER_US);
        let at = self
            .followed
            .iter()
            .position(|one| (x - one.x).abs() <= FOLLOW && (y - one.y).abs() <= FOLLOW);
        let at = at.unwrap_or_else(|| {
            if self.followed.len() >= MOST
                && let Some(oldest) =
                    (0..self.followed.len()).min_by_key(|i| self.followed[*i].seen_us)
            {
                self.followed.swap_remove(oldest);
            }
            self.followed.push(Followed {
                x,
                y,
                seen_us: now_us,
                from: (x, y, now_us),
                confirmed: false,
            });
            self.followed.len() - 1
        });
        let one = &mut self.followed[at];
        (one.x, one.y, one.seen_us) = (x, y, now_us);
        if (x - one.from.0).abs() >= MOVED || (y - one.from.1).abs() >= MOVED {
            one.confirmed = true;
        } else if now_us.saturating_sub(one.from.2) > MOVED_WITHIN_US {
            one.from = (x, y, now_us);
        }
        let why = poor(face, filled).or((!one.confirmed).then_some(Why::Unconfirmed));
        self.counts.faces += 1;
        match why {
            Some(Why::Faint) => self.counts.faint += 1,
            Some(Why::Small) => self.counts.small += 1,
            Some(Why::Clipped) => self.counts.clipped += 1,
            Some(Why::Turned) => self.counts.turned += 1,
            Some(Why::Unconfirmed) => self.counts.unconfirmed += 1,
            None => {}
        }
        why
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: u64 = 1_000_000;
    /// The detector's picture, as `bus_out`'s tests have it.
    const FILLED: (usize, usize) = (640, 480);

    /// A face at the desk, facing the lamp: 0.2 of the height, 0.91, its
    /// centre at (`across`, `down`) of the picture.
    fn face(across: f32, down: f32) -> Face {
        let (w, h) = (96.0, 96.0);
        let (x, y) = (across * 640.0 - w / 2.0, down * 480.0 - h / 2.0);
        Face {
            score: 0.91,
            x,
            y,
            width: w,
            height: h,
            marks: [
                (x + 0.3 * w, y + 0.4 * h),
                (x + 0.7 * w, y + 0.4 * h),
                // Level: the nose 0.64 and the mouth 1.15 of the eyes'
                // distance below them (`facing::LEVEL_NOSE_DROP`, `LEVEL_MOUTH_DROP`).
                (x + 0.5 * w, y + 0.656 * h),
                (x + 0.35 * w, y + 0.86 * h),
                (x + 0.65 * w, y + 0.86 * h),
            ],
        }
    }

    #[test]
    fn a_good_face_passes_the_face_checks() {
        assert_eq!(poor(&face(0.5, 0.5), FILLED), None);
    }

    #[test]
    fn a_faint_a_small_a_clipped_and_a_turned_face_are_poor() {
        let faint = Face {
            score: 0.79,
            ..face(0.5, 0.5)
        };
        assert_eq!(poor(&faint, FILLED), Some(Why::Faint));
        let mut small = face(0.5, 0.5);
        small.height = 0.05 * 480.0;
        assert_eq!(poor(&small, FILLED), Some(Why::Small));
        // Its box runs off the picture's left: the detector saw part of it.
        let clipped = face(0.05, 0.5);
        assert_eq!(poor(&clipped, FILLED), Some(Why::Clipped));
        // The nose far to one side of the eyes: turned well past 60 degrees.
        let mut turned = face(0.5, 0.5);
        turned.marks[2].0 = turned.marks[1].0 + 2.0;
        assert_eq!(poor(&turned, FILLED), Some(Why::Turned));
        // An eye guessed outside the picture.
        let mut guessed = face(0.5, 0.5);
        guessed.marks[0].0 = -1.0;
        assert_eq!(poor(&guessed, FILLED), Some(Why::Turned));
    }

    #[test]
    fn a_good_face_that_never_moves_is_never_confirmed() {
        // lamp-4ace's night: a face at one place, wandering 0.015, all night.
        let mut person = Person::default();
        for look in 0..600u64 {
            let wander = if look % 2 == 0 { 0.0 } else { 0.015 };
            let why = person.why_not(&face(0.5 + wander, 0.5), FILLED, look * SECOND / 2);
            assert_eq!(why, Some(Why::Unconfirmed), "look {look}");
        }
        assert_eq!(person.counts.unconfirmed, 600);
    }

    #[test]
    fn a_face_that_moves_is_confirmed_and_stays_so_sitting_still() {
        let mut person = Person::default();
        assert_eq!(
            person.why_not(&face(0.40, 0.5), FILLED, 0),
            Some(Why::Unconfirmed)
        );
        // Sitting down: 0.06 across in 2 s.
        assert_eq!(person.why_not(&face(0.46, 0.5), FILLED, 2 * SECOND), None);
        // Then still for a minute: still a person.
        for second in 3..63 {
            assert_eq!(
                person.why_not(&face(0.46, 0.5), FILLED, second * SECOND),
                None
            );
        }
        assert_eq!(person.counts.faces, 62);
        assert_eq!(person.counts.unconfirmed, 1);
    }

    #[test]
    fn a_move_spread_over_more_than_ten_seconds_does_not_confirm() {
        let mut person = Person::default();
        // 0.004 a second, a look a second: 0.04 in any 10 s, 0.08 in all.
        for second in 0..=20u64 {
            let across = 0.30 + 0.004 * second as f32;
            let why = person.why_not(&face(across, 0.5), FILLED, second * SECOND);
            assert_eq!(why, Some(Why::Unconfirmed), "second {second}");
        }
    }

    #[test]
    fn a_confirmed_face_lost_longer_than_the_blind_time_must_move_again() {
        let mut person = Person::default();
        person.why_not(&face(0.40, 0.5), FILLED, 0);
        assert_eq!(person.why_not(&face(0.46, 0.5), FILLED, SECOND), None);
        let back = SECOND + BLIND_AFTER_US + SECOND;
        assert_eq!(
            person.why_not(&face(0.46, 0.5), FILLED, back),
            Some(Why::Unconfirmed)
        );
    }

    #[test]
    fn a_confirmed_face_that_turns_for_a_moment_is_left_out_only_then() {
        let mut person = Person::default();
        person.why_not(&face(0.40, 0.5), FILLED, 0);
        assert_eq!(person.why_not(&face(0.46, 0.5), FILLED, SECOND), None);
        let mut turned = face(0.46, 0.5);
        turned.marks[2].0 = turned.marks[1].0 + 2.0;
        assert_eq!(
            person.why_not(&turned, FILLED, 2 * SECOND),
            Some(Why::Turned)
        );
        assert_eq!(person.why_not(&face(0.46, 0.5), FILLED, 3 * SECOND), None);
    }

    #[test]
    fn two_people_are_followed_apart() {
        let mut person = Person::default();
        person.why_not(&face(0.25, 0.5), FILLED, 0);
        person.why_not(&face(0.75, 0.5), FILLED, 0);
        // Only the one on the left moves.
        assert_eq!(person.why_not(&face(0.31, 0.5), FILLED, SECOND), None);
        assert_eq!(
            person.why_not(&face(0.75, 0.5), FILLED, SECOND),
            Some(Why::Unconfirmed)
        );
    }

    #[test]
    fn the_switch_reads_on_and_count_only() {
        assert_eq!(Mode::from(Some("on")), Some(Mode::On));
        assert_eq!(Mode::from(Some("count")), Some(Mode::Count));
        assert_eq!(Mode::from(Some("yes")), None);
        assert_eq!(Mode::from(None), None);
    }
}
