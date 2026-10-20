//! Whether anyone is at the lamp, as the eyes say it on `room/presence`
//! (#1677, `docs/presence.md`): someone while a face was seen or a voice
//! heard within [`RoomPresence::SOMEONE_WITHIN_S`], or nobody, and for how
//! long. The eyes own it because they own the larger part of the evidence,
//! the faces; the voice's lines come to them on `voice/heard`, as they do
//! for the mouth (`mesh::Heard`). Pure arithmetic on the times it is given:
//! the bus is `bus_out`'s.
//!
//! Nothing is said while the eyes are not looking (the camera closed by the
//! privacy switch, a mute or the voice gone, or stalled): nobody can be said
//! to be away then, so the last value runs out and a reader knows nothing.
//! Looks are 250 to 600 ms apart while the camera is open, so none for
//! [`BLIND_AFTER_US`] is not looking, as Expression counts the eyes quiet
//! (`crates/expression/src/alive.rs`, `EYES_QUIET_US`). When looks come back
//! after that, the count of nobody begins again from then: what happened
//! while the eyes were shut is not known.
use lamp_msgs::{RoomPresence, SeenBy};
use std::collections::{HashMap, VecDeque};

/// No look for this long: the eyes are not looking.
pub const BLIND_AFTER_US: u64 = 5_000_000;

/// With far faces judged ([`Away::far_moving`], `LAMP_PRESENCE_FAR=moving`,
/// #1678): a face this tall, as a share of the picture, is at the lamp, the
/// line the activity judge and the still places use (`activity::NEAR`,
/// `still::PERSON_HEIGHT`).
pub const NEAR_TALL: f32 = 0.14;
/// A smaller face counts while a near one was seen this lately: the sitter
/// the detector missed for a look, or someone walking away.
pub const NEAR_LATELY_US: u64 = 4_000_000;
/// Or while its own centre (by its passing id) has moved this far, across or
/// down, as a share of the picture, within [`MOVED_WITHIN_US`]: a false
/// face's box moves 0.01 to 0.02 look to look (`still`), and so does a
/// steady face far behind.
pub const MOVED: f32 = 0.05;
pub const MOVED_WITHIN_US: u64 = 10_000_000;

/// A face in a look: its passing id (`humans/faces/tracked`'s), its centre
/// across and down and its height, each as a share of the picture, and the
/// picture's width in its heights (4:3 is 1.33), so `across * wide` is in
/// the unit of `down` and `tall` and a distance is one measure.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub id: String,
    pub across: f32,
    pub down: f32,
    pub tall: f32,
    pub wide: f32,
}

impl Seen {
    /// Whether this face is the sitter's at `place`: within
    /// `activity::REACH` of its heights of its centre, as the activity judge
    /// follows its sitter (`sitter_in`), and at least [`AT_PLACE_TALL`] of
    /// the sitter's height there, so a small face far behind the chair, in
    /// line with the camera, is not (the reviewer on #2183).
    fn at(&self, place: &Place) -> bool {
        self.tall >= AT_PLACE_TALL * place.tall
            && (self.across * self.wide - place.x).hypot(self.down - place.down)
                <= crate::activity::REACH * place.tall
    }
}

/// A face at the place counts only this share of the sitter's height there
/// or more: leaning back at lamp-75c9 was 0.6 to 0.95 of it, sitting low 0.68.
pub const AT_PLACE_TALL: f32 = 0.5;

/// Where the sitter is: the last near face's centre (across in heights, as
/// [`Seen::at`] measures), following the sitter's face at it look to look,
/// with the near face's height, and when a face was last there.
#[derive(Clone, Copy, Debug)]
struct Place {
    x: f32,
    down: f32,
    tall: f32,
    seen_at: u64,
}

/// Where each far face has been, by its passing id, and when a near face
/// last was.
#[derive(Debug, Default)]
struct Far {
    near_at: Option<u64>,
    centres: HashMap<String, VecDeque<(u64, f32, f32)>>,
    /// The last near face's place, while a face stays at it (#1677's rerun:
    /// a sitter leaning back at lamp-75c9 is 0.12 to 0.14 tall, under
    /// [`NEAR_TALL`], at the place they sat, whatever their id).
    place: Option<Place>,
}

/// The evidence so far, and what was last said.
#[derive(Debug, Default)]
pub struct Away {
    /// Since when the eyes have been looking this time, and their last look.
    looking: Option<(u64, u64)>,
    /// When a face was last seen or a voice last heard, which, and the last
    /// face's height as a share of the picture.
    someone_at_us: Option<u64>,
    by: Option<SeenBy>,
    face_tall: Option<f32>,
    /// Whether someone was said to be there, and when it was said.
    said: Option<(bool, u64)>,
    /// With far faces judged, what they are judged from.
    far: Option<Far>,
}

impl Away {
    /// A face smaller than [`NEAR_TALL`] counts only while it moves, or just
    /// after a near one (#1678: a steady far face, someone behind in the
    /// office or a picture, held lamp-75c9 at someone ~37 s after its sitter
    /// left).
    pub fn far_moving() -> Self {
        Self {
            far: Some(Far::default()),
            ..Self::default()
        }
    }

    /// A look at `now_us` with the faces in it: the tallest that counts is
    /// someone ([`Self::looked`]). Every face counts unless far faces are
    /// judged ([`Self::far_moving`]).
    pub fn looked_at(&mut self, now_us: u64, faces: &[Seen]) {
        let tallest = |faces: &mut dyn Iterator<Item = &Seen>| {
            faces.map(|face| face.tall).max_by(|a, b| a.total_cmp(b))
        };
        let counted = match self.far.as_mut() {
            None => tallest(&mut faces.iter()),
            Some(far) => {
                if let Some(near) = tallest(&mut faces.iter().filter(|face| face.tall >= NEAR_TALL))
                {
                    far.near_at = Some(now_us);
                    far.centres.clear();
                    if let Some(face) = faces.iter().find(|face| face.tall == near) {
                        far.place = Some(Place {
                            x: face.across * face.wide,
                            down: face.down,
                            tall: face.tall,
                            seen_at: now_us,
                        });
                    }
                    Some(near)
                } else {
                    // Each far face's own centres, so two steady faces taking
                    // turns as the tallest, or a face found once elsewhere,
                    // never read as a move; a new id starts with none.
                    for centres in far.centres.values_mut() {
                        while centres
                            .front()
                            .is_some_and(|(at, ..)| now_us.saturating_sub(*at) > MOVED_WITHIN_US)
                        {
                            centres.pop_front();
                        }
                    }
                    far.centres.retain(|_, centres| !centres.is_empty());
                    let mut moving = Vec::new();
                    for face in faces {
                        let centres = far.centres.entry(face.id.clone()).or_default();
                        if centres.iter().any(|(_, across, down)| {
                            (face.across - across).abs() >= MOVED
                                || (face.down - down).abs() >= MOVED
                        }) {
                            moving.push(face);
                        }
                        centres.push_back((now_us, face.across, face.down));
                    }
                    // A smaller face at the last near face's place counts, for
                    // as long as a face stays there (gone for [`NEAR_LATELY_US`]
                    // and the place is forgotten).
                    let mut placed: Vec<&Seen> = Vec::new();
                    if let Some(place) = far.place.as_mut() {
                        placed.extend(faces.iter().filter(|face| face.at(place)));
                        // The place follows the sitter's face, as `sitter_in`
                        // follows its last look; its height stays the near
                        // face's, so the size guard holds.
                        if let Some(sitter) = placed.iter().max_by(|a, b| a.tall.total_cmp(&b.tall))
                        {
                            place.x = sitter.across * sitter.wide;
                            place.down = sitter.down;
                            place.seen_at = now_us;
                        }
                    }
                    if far
                        .place
                        .is_some_and(|place| now_us.saturating_sub(place.seen_at) >= NEAR_LATELY_US)
                    {
                        far.place = None;
                    }
                    let lately = far
                        .near_at
                        .is_some_and(|at| now_us.saturating_sub(at) < NEAR_LATELY_US);
                    if lately {
                        tallest(&mut faces.iter())
                    } else {
                        tallest(&mut moving.into_iter().chain(placed))
                    }
                }
            }
        };
        self.looked(now_us, counted);
    }

    /// A look at `now_us`, with the height of its largest face as a share
    /// of the picture if it found one.
    pub fn looked(&mut self, now_us: u64, face: Option<f32>) {
        let since = match self.looking {
            Some((since, last)) if now_us.saturating_sub(last) < BLIND_AFTER_US => since,
            _ => now_us,
        };
        self.looking = Some((since, now_us));
        if let Some(tall) = face {
            self.face_tall = Some(tall);
            self.seen(now_us, SeenBy::Face);
        }
    }

    /// A voice heard at `at_us` (when the voice heard the words, never
    /// later than now).
    pub fn heard(&mut self, at_us: u64) {
        self.seen(at_us, SeenBy::Voice);
    }

    fn seen(&mut self, at_us: u64, by: SeenBy) {
        if self.someone_at_us.is_none_or(|at| at_us >= at) {
            self.someone_at_us = Some(at_us);
            self.by = Some(by);
        }
    }

    /// Why someone is there, for the journal: a face and how tall, or a
    /// voice. Counts only, no words.
    pub fn cause(&self) -> String {
        match self.by {
            Some(SeenBy::Face) => format!(
                "face, {:.2} of the picture tall",
                self.face_tall.unwrap_or(0.0)
            ),
            Some(SeenBy::Voice) => "voice".into(),
            None => "nothing".into(),
        }
    }

    /// Whether anyone is at the lamp at `now_us`, while the eyes look.
    pub fn now(&self, now_us: u64) -> Option<RoomPresence> {
        let (since, last) = self.looking?;
        if now_us.saturating_sub(last) >= BLIND_AFTER_US {
            return None;
        }
        // Evidence from before the eyes began looking this time is not
        // counted: who came and went while they were shut is not known.
        let from = self.someone_at_us.filter(|at| *at >= since);
        let within = u64::from(RoomPresence::SOMEONE_WITHIN_S) * 1_000_000;
        if from.is_some_and(|at| now_us.saturating_sub(at) < within) {
            return Some(RoomPresence {
                someone: true,
                nobody_s: 0,
                by: self.by,
            });
        }
        let away_us = now_us.saturating_sub(from.unwrap_or(since));
        Some(RoomPresence {
            someone: false,
            nobody_s: u32::try_from(away_us / 1_000_000).unwrap_or(u32::MAX),
            by: None,
        })
    }

    /// What to say at `now_us`, if anything: once a second while the eyes
    /// look ([`RoomPresence::EVERY`]), and at once when `someone` changes.
    pub fn say(&mut self, now_us: u64) -> Option<RoomPresence> {
        let Some(now) = self.now(now_us) else {
            self.said = None;
            return None;
        };
        let every = RoomPresence::EVERY.as_micros() as u64;
        let due = self.said.is_none_or(|(someone, at)| {
            someone != now.someone || now_us.saturating_sub(at) >= every
        });
        due.then(|| {
            self.said = Some((now.someone, now_us));
            now
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;
    /// A 4:3 picture's width in its heights.
    const WIDE: f32 = 4.0 / 3.0;

    /// Looks every 500 ms from `from` to `to` (s), with a face or not.
    fn looks(away: &mut Away, from: u64, to: u64, face: bool) {
        let mut at = from * S;
        while at < to * S {
            away.looked(at, face.then_some(0.25));
            at += S / 2;
        }
    }

    /// Someone shown by a face, or nobody for so long.
    fn present(someone: bool, nobody_s: u32) -> Option<RoomPresence> {
        Some(RoomPresence {
            someone,
            nobody_s,
            by: someone.then_some(SeenBy::Face),
        })
    }

    #[test]
    fn nothing_is_said_before_the_eyes_look_or_once_they_stop() {
        let mut away = Away::default();
        assert_eq!(away.say(0), None);
        away.heard(0);
        assert_eq!(away.now(S), None, "a voice alone, with the eyes shut");
        looks(&mut away, 10, 20, false);
        assert_eq!(away.now(20 * S), present(false, 10));
        // The last look at 19.5 s: still looking at 24.4, not at 24.5.
        assert_eq!(away.now(24 * S + 400_000), present(false, 14));
        assert_eq!(away.now(24 * S + 500_000), None);
    }

    #[test]
    fn a_face_or_a_voice_is_someone_for_twenty_seconds_then_nobody_counts_from_it() {
        let mut away = Away::default();
        looks(&mut away, 0, 30, true);
        assert_eq!(away.now(30 * S), present(true, 0));
        // The last face at 29.5 s; then empty looks.
        looks(&mut away, 30, 60, false);
        assert_eq!(away.now(49 * S), present(true, 0), "a look away");
        assert_eq!(away.now(49 * S + 500_000), present(false, 20));
        assert_eq!(away.now(59 * S + 500_000), present(false, 30));
        // A voice heard out of the camera's view is someone too.
        away.heard(60 * S);
        looks(&mut away, 60, 90, false);
        let voiced = away.now(79 * S).unwrap();
        assert_eq!((voiced.someone, voiced.by), (true, Some(SeenBy::Voice)));
        assert_eq!(away.cause(), "voice");
        assert_eq!(away.now(89 * S), present(false, 29));
        // A face after it: the face is what showed someone, and how tall.
        looks(&mut away, 90, 91, true);
        assert_eq!(away.now(91 * S), present(true, 0));
        assert_eq!(away.cause(), "face, 0.25 of the picture tall");
    }

    #[test]
    fn after_the_eyes_were_shut_nobody_counts_again_from_when_they_look_again() {
        let mut away = Away::default();
        looks(&mut away, 0, 600, false);
        assert_eq!(away.now(600 * S), present(false, 600));
        // Shut for a minute (a mute), then looking again: who came and
        // went meanwhile is not known.
        looks(&mut away, 660, 700, false);
        assert_eq!(away.now(700 * S), present(false, 40));
        // A gap under five seconds (a stall, a camera reopened) counts on.
        looks(&mut away, 704, 710, false);
        assert_eq!(away.now(710 * S), present(false, 50));
        // A face seen just before the eyes shut is not someone after.
        let mut seen = Away::default();
        looks(&mut seen, 0, 10, true);
        looks(&mut seen, 16, 20, false);
        assert_eq!(seen.now(20 * S), present(false, 4));
    }

    /// #1678 (lamp-75c9, 20 Oct 10:17): the sitter leaves, and a steady far
    /// face behind (0.06 tall, its box moving 0.01 look to look) stays in
    /// view. Judged as far, it is nobody 20 s after the near face plus
    /// [`NEAR_LATELY_US`]; without, someone for as long as it stays.
    #[test]
    fn a_steady_far_face_with_nobody_near_ends_in_nobody() {
        let sitter = Seen {
            id: "f1".into(),
            across: 0.4,
            down: 0.45,
            tall: 0.16,
            wide: WIDE,
        };
        let behind = |at: u64| Seen {
            id: "f2".into(),
            across: 0.73 + if at.is_multiple_of(2) { 0.01 } else { 0.0 },
            down: 0.42,
            tall: 0.06,
            wide: WIDE,
        };
        for judged in [true, false] {
            let mut away = if judged {
                Away::far_moving()
            } else {
                Away::default()
            };
            let mut at = 0;
            // The sitter, with the far face behind, for a minute.
            while at < 60 * S {
                away.looked_at(at, &[sitter.clone(), behind(at / (S / 2))]);
                at += S / 2;
            }
            // The sitter gone at 60 s; the far face stays, for two minutes.
            while at < 180 * S {
                away.looked_at(at, &[behind(at / (S / 2))]);
                at += S / 2;
            }
            let now = away.now(at);
            if judged {
                // Last counted look 63 s (under 4 s after 59.5): nobody from 83 s.
                assert_eq!(now, present(false, 117), "{now:?}");
            } else {
                assert_eq!(now, present(true, 0), "{now:?}");
            }
        }
    }

    /// Judged as far, a small face that moves (someone walking past, or
    /// coming to the lamp) is someone, and a near face always is.
    #[test]
    fn a_far_face_that_moves_is_someone_and_a_near_one_always_is() {
        let mut away = Away::far_moving();
        let mut at = 0;
        // Nobody, then a small face walking across: 0.01 a look.
        while at < 30 * S {
            away.looked_at(at, &[]);
            at += S / 2;
        }
        let mut across = 0.2;
        while at < 40 * S {
            away.looked_at(
                at,
                &[Seen {
                    id: "f3".into(),
                    across,
                    down: 0.4,
                    tall: 0.07,
                    wide: WIDE,
                }],
            );
            across += 0.01;
            at += S / 2;
        }
        assert_eq!(away.now(at), present(true, 0));
        // The same face standing still for a minute: nobody, 20 s after it
        // last moved 0.05 within 10 s.
        while at < 100 * S {
            away.looked_at(
                at,
                &[Seen {
                    id: "f3".into(),
                    across,
                    down: 0.4,
                    tall: 0.07,
                    wide: WIDE,
                }],
            );
            at += S / 2;
        }
        assert!(!away.now(at).unwrap().someone);
        // A near face, however still, is someone at once.
        away.looked_at(
            at,
            &[Seen {
                id: "f4".into(),
                across: 0.5,
                down: 0.5,
                tall: NEAR_TALL,
                wide: WIDE,
            }],
        );
        assert_eq!(away.now(at), present(true, 0));
    }

    /// The reviewer on #2149: each far face's movement is its own, by its
    /// passing id. Two steady far faces taking turns as the tallest, or a
    /// face found for one look elsewhere every 25 s, are not a move: nobody.
    #[test]
    fn steady_far_faces_taking_turns_or_one_found_once_elsewhere_are_nobody() {
        let far = |id: &str, across: f32, tall: f32| Seen {
            id: id.into(),
            across,
            down: 0.4,
            tall,
            wide: WIDE,
        };
        let mut turns = Away::far_moving();
        let mut stray = Away::far_moving();
        let mut at = 0;
        let mut look = 0u64;
        while at < 180 * S {
            // ±0.001 of height a look, so the tallest changes every look.
            let wobble = if look.is_multiple_of(2) {
                0.001
            } else {
                -0.001
            };
            turns.looked_at(
                at,
                &[
                    far("f1", 0.30, 0.060 + wobble),
                    far("f2", 0.75, 0.060 - wobble),
                ],
            );
            let mut seen = vec![far("f3", 0.73, 0.06)];
            if look.is_multiple_of(50) {
                seen.push(far(&format!("f{}", 10 + look), 0.20, 0.07));
            }
            stray.looked_at(at, &seen);
            at += S / 2;
            look += 1;
        }
        assert_eq!(turns.now(at), present(false, 180), "taking turns");
        assert_eq!(stray.now(at), present(false, 180), "found once elsewhere");
    }

    /// #1677's rerun at lamp-75c9 (20 Oct 11:57–12:00), from its numbers:
    /// the sitter at 0.165 tall leans back to 0.136, under the near line,
    /// and their id changes (f1 to f10) while the place holds. Judged as far,
    /// they stay someone at the place they sat; the steady far face at 0.73
    /// still counts only if it moves, and once they walk out it's nobody
    /// 20 s after a face was last at the place.
    #[test]
    fn a_sitter_leaning_back_under_the_near_line_stays_someone_at_their_place() {
        let face = |id: &str, across: f32, down: f32, tall: f32| Seen {
            id: id.into(),
            across,
            down,
            tall,
            wide: WIDE,
        };
        let behind = || face("f2", 0.73, 0.42, 0.06);
        let mut away = Away::far_moving();
        let mut at = 0;
        let mut look = |away: &mut Away, to: u64, faces: &dyn Fn() -> Vec<Seen>| {
            while at < to * S {
                away.looked_at(at, &faces());
                at += S / 2;
            }
        };
        look(&mut away, 60, &|| {
            vec![face("f1", 0.55, 0.30, 0.165), behind()]
        });
        look(&mut away, 120, &|| {
            vec![face("f1", 0.53, 0.38, 0.136), behind()]
        });
        look(&mut away, 240, &|| {
            vec![face("f10", 0.53, 0.39, 0.136), behind()]
        });
        assert_eq!(away.now(240 * S), present(true, 0), "leaning back");
        // Walked out at 240 s: the far face alone, steady.
        look(&mut away, 330, &|| vec![behind()]);
        // A face last at the place at 239.5 s, the last counted look (the far
        // face isn't at it): nobody from 259.5 s.
        assert_eq!(away.now(330 * S), present(false, 90), "walked out");
        assert_eq!(away.now(259 * S), present(true, 0));
    }

    /// The reviewer on #2183: once the sitter walks out, a small steady face
    /// straight behind the chair (0.06 at nearly the sitter's place) is not
    /// the sitter: nobody 24 s after them. Sitting low, 0.68 of the seated
    /// height, still is; and the place follows a sitter shifting sideways.
    #[test]
    fn a_small_face_behind_the_chair_is_not_the_sitter_but_sitting_low_is() {
        let face = |id: &str, across: f32, down: f32, tall: f32| Seen {
            id: id.into(),
            across,
            down,
            tall,
            wide: WIDE,
        };
        let look = |away: &mut Away, at: &mut u64, to: u64, faces: &dyn Fn() -> Vec<Seen>| {
            while *at < to * S {
                away.looked_at(*at, &faces());
                *at += S / 2;
            }
        };
        let mut away = Away::far_moving();
        let mut at = 0;
        look(&mut away, &mut at, 60, &|| {
            vec![face("f1", 0.55, 0.30, 0.165)]
        });
        look(&mut away, &mut at, 300, &|| {
            vec![face("f2", 0.56, 0.32, 0.06)]
        });
        // The last counted look 63 s, under 4 s after the near face: 237 s of nobody.
        assert_eq!(away.now(300 * S), present(false, 237), "behind the chair");
        let mut low = Away::far_moving();
        let mut at = 0;
        look(&mut low, &mut at, 60, &|| {
            vec![face("f1", 0.55, 0.30, 0.165)]
        });
        look(&mut low, &mut at, 180, &|| {
            vec![face("f1", 0.55, 0.36, 0.112)]
        });
        assert_eq!(low.now(180 * S), present(true, 0), "sitting low");
        // Sliding sideways while low, 0.005 a look to 0.40 across: more than
        // a seated height from where they sat, still followed.
        let mut across = 0.55;
        while at < 240 * S {
            low.looked_at(at, &[face("f1", across, 0.36, 0.112)]);
            across = (across - 0.005).max(0.40);
            at += S / 2;
        }
        assert_eq!(low.now(240 * S), present(true, 0), "slid sideways");
    }

    #[test]
    fn it_is_said_once_a_second_and_at_once_when_someone_comes_or_goes() {
        let mut away = Away::default();
        let mut said = Vec::new();
        // The loop turns every 100 ms; a look every 500 ms; a face from 30 s.
        for turn in 0..600u64 {
            let at = turn * S / 10;
            if turn % 5 == 0 {
                away.looked(at, (300..310).contains(&turn).then_some(0.25));
            }
            if let Some(now) = away.say(at) {
                now.check().unwrap();
                said.push((at, now));
            }
        }
        // Once a second for a minute, and at once where someone came and went.
        let came = said
            .iter()
            .position(|(_, now)| now.someone)
            .expect("someone came");
        assert_eq!(said[came].0, 30 * S, "said at once");
        assert_eq!(
            said[came - 1],
            (
                29 * S,
                RoomPresence {
                    someone: false,
                    nobody_s: 29,
                    by: None,
                }
            )
        );
        // Someone until 20 s after the last face (30.5 s), nobody then.
        let went = said
            .iter()
            .rposition(|(_, now)| now.someone)
            .map(|at| said[at + 1])
            .unwrap();
        assert_eq!(
            went,
            (
                S * 505 / 10,
                RoomPresence {
                    someone: false,
                    nobody_s: 20,
                    by: None,
                }
            )
        );
        assert!(said.windows(2).all(|pair| pair[1].0 - pair[0].0 <= S));
        assert_eq!(said.len(), 61, "{said:?}");
    }
}
