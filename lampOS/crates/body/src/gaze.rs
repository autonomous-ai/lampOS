//! Looking at the person the eyes see: from where a face is in the camera's
//! picture to an offset of the head. Pure arithmetic; limits, speed and
//! whether anything moves at all belong to the engine that asks.
//!
//! The camera is in the head, so turning toward a face also moves the
//! picture. The gaze therefore works in small corrections with stillness
//! between them: it waits until the head has been still, takes a few sights,
//! corrects most of what they show, and waits again. It never searches: with
//! no face in the picture it does nothing, and after a while lets go.
use crate::gesture::Mood;
use serde::{Deserialize, Serialize};

/// Where a face is in the picture, as `lamp-eyes` sends it: its centre from
/// the middle (-1 left or top, 1 right or bottom), its height as a share of
/// the picture's, and how far its nose is from between its eyes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sight {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub turned: f32,
}

/// One datagram from the eyes: the face to look at, or that there is none.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seen {
    pub face: Option<Sight>,
}

/// How this lamp's joints move its camera's picture, and how far the gaze
/// may take them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Looking {
    /// Counts of base yaw, and of head pitch, that bring a face from the
    /// right edge of the picture, and from its bottom edge, to the middle.
    /// The sign is which way that joint moves the picture.
    pub turn: [f32; 2],
    /// The farthest the gaze takes yaw and pitch from the pose's own.
    pub reach: [f32; 2],
}

impl Looking {
    /// The most a whole half-picture may be worth, and the farthest reach:
    /// about 80 degrees of turn, and 44 and 18 degrees of reach. lamp-4ace's
    /// camera is a wide one: 727 counts of yaw and 553 of pitch. The yaw
    /// reach was 35 degrees until people stood further round than the lamp
    /// could turn to them; the old runtime's own recorded moves turn the
    /// base 42 degrees either way.
    pub const TURN_CAP: i16 = 900;
    pub const REACH_CAP: [u16; 2] = [500, 200];

    pub fn new(turn: [i16; 2], reach: [u16; 2]) -> Result<Self, &'static str> {
        if turn.iter().any(|counts| counts.abs() > Self::TURN_CAP)
            || reach
                .iter()
                .zip(Self::REACH_CAP)
                .any(|(counts, cap)| *counts > cap)
        {
            return Err("a gaze outside what this body may be given");
        }
        Ok(Self {
            turn: turn.map(f32::from),
            reach: reach.map(f32::from),
        })
    }
}

/// A picture counts only if the head has been still this long: one taken
/// while it moved, or just after, shows where the face was.
const STILL_US: u64 = 400_000;
/// Sights in a row, all with a face, before anything is corrected. Their
/// middle value is what counts: on lamp-4ace one look in sixty put a face
/// at the far corner of the picture, and the mean of three moved the head.
const SIGHTS: u8 = 3;
/// The share of what they show that one correction takes out. Under one, so
/// that a gain set too high still settles.
const SHARE: f32 = 0.7;
/// A face this near the middle is being looked at already: across, down.
const NEAR: [f32; 2] = [0.15, 0.20];
/// While the lamp speaks its head should be still, for its microphone. Only
/// a face well off the middle is worth a move then.
const NEAR_SPEAKING: [f32; 2] = [0.35, 0.40];
/// No face for this long and the gaze lets go.
const LOST_US: u64 = 6_000_000;
/// It goes back to the pose's own direction at this many counts a second.
const RETURN: f32 = 60.0;

#[derive(Clone, Debug)]
pub struct Gaze {
    looking: Looking,
    /// Yaw and pitch, in counts, added to what the mood wants.
    offset: [f32; 2],
    /// The last sights, across and down, of which the middle one is used:
    /// one stray face at the edge of a picture then moves nothing.
    recent: [[f32; 2]; SIGHTS as usize],
    sights: u8,
    face_at: Option<u64>,
    last_us: Option<u64>,
}

impl Gaze {
    pub fn new(looking: Looking) -> Self {
        Self {
            looking,
            offset: [0.0; 2],
            recent: [[0.0; 2]; SIGHTS as usize],
            sights: 0,
            face_at: None,
            last_us: None,
        }
    }

    /// One step. `seen` is what arrived from the eyes since the last step,
    /// if anything did; `awake` is false for a lamp that is folded or
    /// folding, which looks at nobody; `still_us` is how long no joint has
    /// been asked to move. Returns the yaw and pitch offset in counts.
    pub fn step(
        &mut self,
        seen: Option<Seen>,
        mood: Mood,
        awake: bool,
        still_us: u64,
        now_us: u64,
    ) -> [f32; 2] {
        let step = self.last_us.map_or(0.0, |last| {
            now_us.saturating_sub(last).min(100_000) as f32 / 1e6
        });
        self.last_us = Some(now_us);
        let still = still_us >= STILL_US;
        match seen {
            Some(Seen { face: Some(sight) }) => {
                self.face_at = Some(now_us);
                if still && sight.x.is_finite() && sight.y.is_finite() {
                    self.recent[usize::from(self.sights) % usize::from(SIGHTS)] =
                        [sight.x.clamp(-1.0, 1.0), sight.y.clamp(-1.0, 1.0)];
                    self.sights = self.sights.saturating_add(1);
                }
            }
            Some(Seen { face: None }) => self.forget(),
            None => {}
        }
        let attending = matches!(mood, Mood::Listening | Mood::Speaking);
        // Sights are kept only for a correction that could be made now.
        if !still || !awake || !attending {
            self.forget();
        }
        if awake && attending && self.sights >= SIGHTS {
            let near = if mood == Mood::Speaking {
                NEAR_SPEAKING
            } else {
                NEAR
            };
            for (axis, offset) in self.offset.iter_mut().enumerate() {
                let mut seen = self.recent.map(|sight| sight[axis]);
                seen.sort_by(f32::total_cmp);
                let off = seen[seen.len() / 2];
                if off.abs() > near[axis] {
                    *offset = (*offset + self.looking.turn[axis] * off * SHARE)
                        .clamp(-self.looking.reach[axis], self.looking.reach[axis]);
                }
            }
            self.forget();
        }
        let lost = self
            .face_at
            .is_none_or(|at| now_us.saturating_sub(at) > LOST_US);
        if lost || !awake {
            for offset in &mut self.offset {
                *offset -= offset.clamp(-RETURN * step, RETURN * step);
            }
        }
        self.offset
    }

    fn forget(&mut self) {
        self.sights = 0;
    }

    /// The base's turn toward a place `toward` across the picture (-1 its
    /// left edge, 1 its right), in counts of yaw: what a face there would be
    /// turned by, the same share of it and inside the same reach (#1679: a
    /// reading lamp turned toward the book it was asked to light).
    pub fn toward(&self, toward: f32) -> f32 {
        if !toward.is_finite() {
            return 0.0;
        }
        (self.looking.turn[0] * toward.clamp(-1.0, 1.0) * SHARE)
            .clamp(-self.looking.reach[0], self.looking.reach[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera 30 degrees wide either side, on a base that turns the
    /// picture the opposite way: 342 counts for the half-picture.
    const LOOKING: Looking = Looking {
        turn: [342.0, 200.0],
        reach: [300.0, 150.0],
    };

    /// A face at this bearing, in counts of yaw and pitch from the pose's
    /// own direction, as a camera turned by `offset` shows it.
    fn shown(bearing: [f32; 2], offset: [f32; 2]) -> Option<Seen> {
        let x = (bearing[0] - offset[0]) / LOOKING.turn[0];
        let y = (bearing[1] - offset[1]) / LOOKING.turn[1];
        Some(Seen {
            face: (x.abs() <= 1.0 && y.abs() <= 1.0).then_some(Sight {
                x,
                y,
                size: 0.2,
                turned: 0.0,
            }),
        })
    }

    /// Run for `seconds`, the eyes looking every 150 ms and the head taking
    /// half a second over each move. Returns the offset and how many moves.
    fn watch(
        gaze: &mut Gaze,
        bearing: Option<[f32; 2]>,
        mood: Mood,
        now: &mut u64,
        seconds: f32,
    ) -> ([f32; 2], u32) {
        let mut offset = gaze.step(None, mood, true, 10_000_000, *now);
        let (mut moved_at, mut moves) = (0u64, 0);
        for step in 0..(seconds * 50.0) as u64 {
            *now += 20_000;
            let moving = *now < moved_at + 500_000;
            let still_us = if moving {
                0
            } else {
                *now - (moved_at + 500_000).min(*now)
            };
            let seen = (step % 7 == 0).then(|| match bearing {
                // While it moves the picture is of somewhere on the way.
                Some(bearing) if !moving => shown(bearing, offset).unwrap(),
                Some(_) => Seen { face: None },
                None => Seen { face: None },
            });
            let next = gaze.step(seen, mood, true, still_us, *now);
            if next != offset && bearing.is_some() {
                moved_at = *now;
                moves += 1;
            }
            offset = next;
        }
        (offset, moves)
    }

    #[test]
    fn it_turns_to_a_face_in_a_few_corrections_and_then_keeps_still() {
        let mut gaze = Gaze::new(LOOKING);
        let mut now = 1_000_000;
        let (offset, moves) = watch(
            &mut gaze,
            Some([200.0, -80.0]),
            Mood::Listening,
            &mut now,
            8.0,
        );
        // Within the near band of the face, and it got there without hunting.
        assert!((offset[0] - 200.0).abs() < 342.0 * 0.15, "{offset:?}");
        assert!((offset[1] + 80.0).abs() < 200.0 * 0.20, "{offset:?}");
        assert!((2..=6).contains(&moves), "{moves}");
        // Nothing more happens while the face stays where it is.
        let (after, more) = watch(
            &mut gaze,
            Some([200.0, -80.0]),
            Mood::Listening,
            &mut now,
            5.0,
        );
        assert_eq!((after, more), (offset, 0));
    }

    #[test]
    fn a_gain_set_too_high_still_settles_and_the_reach_is_never_passed() {
        // The picture is in truth worth 342 counts; told 500, each
        // correction overshoots a little and the next is smaller.
        let mut gaze = Gaze::new(Looking {
            turn: [500.0, 200.0],
            reach: [300.0, 150.0],
        });
        let mut now = 1_000_000;
        let (offset, _) = watch(
            &mut gaze,
            Some([150.0, 0.0]),
            Mood::Listening,
            &mut now,
            12.0,
        );
        assert!((offset[0] - 150.0).abs() < 342.0 * 0.15 + 1.0, "{offset:?}");
        // A face further round than the gaze may go: it stops at its reach.
        let mut gaze = Gaze::new(LOOKING);
        let mut now = 1_000_000;
        let mut offset = [0.0; 2];
        for _ in 0..40 {
            now += 200_000;
            let seen = Some(Seen {
                face: Some(Sight {
                    x: 0.9,
                    y: 0.9,
                    size: 0.2,
                    turned: 0.0,
                }),
            });
            offset = gaze.step(seen, Mood::Listening, true, 1_000_000, now);
        }
        assert_eq!(offset, [300.0, 150.0]);
    }

    #[test]
    fn it_does_not_correct_from_a_moving_picture_or_while_it_thinks_or_rests() {
        let face = |x: f32| {
            Some(Seen {
                face: Some(Sight {
                    x,
                    y: 0.0,
                    size: 0.2,
                    turned: 0.0,
                }),
            })
        };
        for (mood, still_us) in [
            (Mood::Listening, 100_000),
            (Mood::Thinking, 5_000_000),
            (Mood::Resting, 5_000_000),
        ] {
            let mut gaze = Gaze::new(LOOKING);
            let mut now = 1_000_000;
            for _ in 0..30 {
                now += 150_000;
                assert_eq!(gaze.step(face(0.8), mood, true, still_us, now), [0.0; 2]);
            }
        }
        // Speaking, a face a little off the middle is left alone; one well
        // off it is turned to.
        let mut gaze = Gaze::new(LOOKING);
        let mut now = 1_000_000;
        for _ in 0..9 {
            now += 150_000;
            assert_eq!(
                gaze.step(face(0.3), Mood::Speaking, true, 5_000_000, now),
                [0.0; 2]
            );
        }
        let mut offset = [0.0; 2];
        for _ in 0..3 {
            now += 150_000;
            offset = gaze.step(face(0.6), Mood::Speaking, true, 5_000_000, now);
        }
        assert!((offset[0] - 342.0 * 0.6 * SHARE).abs() < 0.01, "{offset:?}");
    }

    #[test]
    fn with_nobody_there_it_does_nothing_and_after_a_while_lets_go() {
        let mut gaze = Gaze::new(LOOKING);
        let mut now = 1_000_000;
        // Never a face: never a move. It does not search.
        let (offset, _) = watch(&mut gaze, None, Mood::Listening, &mut now, 10.0);
        assert_eq!(offset, [0.0; 2]);
        // It found someone, and they left.
        let (found, _) = watch(
            &mut gaze,
            Some([200.0, 0.0]),
            Mood::Listening,
            &mut now,
            6.0,
        );
        assert!(found[0] > 100.0);
        let (held, _) = watch(&mut gaze, None, Mood::Listening, &mut now, 5.0);
        assert_eq!(held, found, "a few seconds without a face is not goodbye");
        let (gone, _) = watch(&mut gaze, None, Mood::Resting, &mut now, 8.0);
        assert_eq!(gone, [0.0; 2]);
        // A lamp that folds down looks at nobody, face or no face.
        let (again, _) = watch(
            &mut gaze,
            Some([200.0, 0.0]),
            Mood::Listening,
            &mut now,
            6.0,
        );
        assert!(again[0] > 100.0);
        let mut offset = again;
        for _ in 0..300 {
            now += 20_000;
            offset = gaze.step(
                shown([200.0, 0.0], offset),
                Mood::Resting,
                false,
                5_000_000,
                now,
            );
        }
        assert_eq!(offset, [0.0; 2]);
    }

    #[test]
    fn the_datagram_is_the_one_the_eyes_send_and_a_gaze_is_taken_only_inside_its_caps() {
        let text = r#"{"face":{"x":0.25,"y":-0.5,"size":0.2,"turned":0.0}}"#;
        let seen: Seen = serde_json::from_str(text).unwrap();
        assert_eq!(seen.face.unwrap().x, 0.25);
        assert_eq!(serde_json::to_string(&seen).unwrap(), text);
        assert_eq!(
            serde_json::from_str::<Seen>(r#"{"face":null}"#)
                .unwrap()
                .face,
            None
        );
        assert!(serde_json::from_str::<Seen>(r#"{"face":null,"goal":2100}"#).is_err());
        assert!(Looking::new([-342, 200], [300, 150]).is_ok());
        assert!(Looking::new([901, 200], [300, 150]).is_err());
        assert!(Looking::new([727, 553], [250, 120]).is_ok());
        assert!(Looking::new([342, 200], [500, 150]).is_ok());
        assert!(Looking::new([342, 200], [501, 150]).is_err());
        assert!(Looking::new([342, 200], [300, 201]).is_err());
    }
}

#[cfg(test)]
mod stray {
    use super::*;

    #[test]
    fn one_stray_sight_among_three_moves_nothing() {
        let sight = |x: f32, y: f32| {
            Some(Seen {
                face: Some(Sight {
                    x,
                    y,
                    size: 0.13,
                    turned: 0.0,
                }),
            })
        };
        let looking = Looking::new([727, 553], [250, 120]).unwrap();
        // As on lamp-4ace: a face near the middle, and once a "face" in the
        // far corner of the picture.
        let mut gaze = Gaze::new(looking);
        let mut now = 1_000_000;
        let mut offset = [0.0; 2];
        for seen in [sight(-0.06, 0.0), sight(0.83, -0.65), sight(-0.05, 0.05)] {
            now += 150_000;
            offset = gaze.step(seen, Mood::Listening, true, 5_000_000, now);
        }
        assert_eq!(offset, [0.0; 2]);
        // Two of three off to one side is a face off to one side.
        let mut gaze = Gaze::new(looking);
        for seen in [sight(-0.27, 0.0), sight(0.83, -0.65), sight(-0.27, 0.05)] {
            now += 150_000;
            offset = gaze.step(seen, Mood::Listening, true, 5_000_000, now);
        }
        assert!(
            (offset[0] + 727.0 * 0.27 * SHARE).abs() < 0.01,
            "{offset:?}"
        );
        assert_eq!(offset[1], 0.0);
    }

    /// #1679 (the review on #1730): a turn toward a place across the picture
    /// is the turn the gaze makes to a face seen there, the same way and as
    /// far, so "left" in the picture never turns the lamp right.
    #[test]
    fn a_turn_toward_a_place_is_the_turn_to_a_face_seen_there() {
        let face = |x: f32| {
            Some(Seen {
                face: Some(Sight {
                    x,
                    y: 0.0,
                    size: 0.2,
                    turned: 0.0,
                }),
            })
        };
        for x in [-0.6f32, 0.6] {
            let mut gaze = Gaze::new(Looking {
                turn: [342.0, 200.0],
                reach: [300.0, 150.0],
            });
            let mut now = 1_000_000;
            let mut offset = [0.0; 2];
            for _ in 0..3 {
                now += 150_000;
                offset = gaze.step(face(x), Mood::Speaking, true, 5_000_000, now);
            }
            assert!(offset[0] != 0.0, "the gaze turned to the face at {x}");
            assert!(
                (gaze.toward(x) - offset[0]).abs() < 0.01,
                "toward({x}) = {} against the face's {}",
                gaze.toward(x),
                offset[0]
            );
        }
    }
}
