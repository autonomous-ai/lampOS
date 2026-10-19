//! What the person at the desk is doing, as far as their face shows it, on
//! `room/activity`, and when they stood up and left, on `room/stood_up`
//! (#1678, `docs/presence.md`). The eyes see a face's box and five marks,
//! no body: so each state is read against this person's own sitting, its
//! baseline the median of the first [`BASELINE_US`] of looks at the desk,
//! never against a fixed line (a seated face was 0.23 of the picture on
//! lamp-06c1 and 0.16 to 0.27 on lamp-4ace, #941). Pure arithmetic on the
//! looks it is given: the bus is `bus_out`'s.
use crate::faces;
use lamp_msgs::{Activity, RoomActivity, StoodUp};
use std::collections::VecDeque;

/// A face at least this share of the picture's height is at the desk:
/// Stretch break's `near`, seated on two lamps and never at 1.5 m (#941).
pub const NEAR: f32 = 0.14;
/// The sitting's baseline: the median of its looks over this long.
pub const BASELINE_US: u64 = 120_000_000;
/// And of at least this many looks.
const BASELINE_LOOKS: usize = 20;
/// No near face for this long is nobody at the desk: `room/presence`'s 20 s.
pub const AWAY_AFTER_US: u64 = 20_000_000;
/// No look for this long: the eyes are not looking, and nothing is said
/// (`away::BLIND_AFTER_US`).
pub const BLIND_AFTER_US: u64 = 5_000_000;
/// Leaning in: the face this much bigger than it sat; back, this much smaller.
const IN: f32 = 1.25;
const BACK: f32 = 0.8;
/// Head down: the face's centre lower by this share of its height, and the
/// nose lower between the eyes and mouth by this much.
const DOWN: f32 = 0.25;
const PITCH_DOWN: f32 = 0.12;
/// Sitting low: the centre lower by this share of its height.
const LOW: f32 = 0.5;
/// How long a look's state must hold before it is said.
const HOLD_US: [(Activity, u64); 5] = [
    (Activity::AtDesk, 5_000_000),
    (Activity::LeaningIn, 10_000_000),
    (Activity::LeaningBack, 10_000_000),
    (Activity::HeadDown, 20_000_000),
    (Activity::SittingLow, 120_000_000),
];
/// Standing up: within this long before the face was lost, it rose by
/// [`STOOD_RISE`] of its height and shrank to [`STOOD_SHRINK`] of it.
const STOOD_WITHIN_US: u64 = 2_000_000;
const STOOD_RISE: f32 = 0.5;
const STOOD_SHRINK: f32 = 0.85;
/// And was not seen again for this long.
const STOOD_GONE_US: u64 = 2_000_000;

/// One look's face, as shares of the picture, and how far down it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub height: f32,
    pub centre_y: f32,
    /// The nose between the eyes (0) and the mouth (1): more is lower,
    /// the head bowed.
    pub pitch: f32,
}

impl Look {
    /// The largest face of a look, in a picture filled to `filled` pixels.
    pub fn largest(found: &[faces::Face], filled: (usize, usize)) -> Option<Self> {
        let tall = filled.1.max(1) as f32;
        let face = found.iter().max_by(|a, b| a.height.total_cmp(&b.height))?;
        let [left_eye, right_eye, nose, mouth_left, mouth_right] = face.marks;
        let eyes = (left_eye.1 + right_eye.1) / 2.0;
        let mouth = (mouth_left.1 + mouth_right.1) / 2.0;
        let span = mouth - eyes;
        Some(Self {
            height: face.height / tall,
            centre_y: (face.y + face.height / 2.0) / tall,
            pitch: if span.abs() > f32::EPSILON {
                ((nose.1 - eyes) / span).clamp(-1.0, 2.0)
            } else {
                0.5
            },
        })
    }
}

fn hold_of(state: Activity) -> u64 {
    HOLD_US
        .iter()
        .find(|(held, _)| *held == state)
        .map_or(0, |(_, us)| *us)
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

/// The evidence so far, and what was last said.
#[derive(Debug, Default)]
pub struct Judge {
    /// The eyes' last look.
    looked_at: Option<u64>,
    /// When this sitting began, and its last near face.
    sitting_from: Option<u64>,
    last_face: Option<u64>,
    /// The sitting's first looks, until the baseline is in.
    samples: Vec<Look>,
    baseline: Option<Look>,
    /// What is said, since when; and what the looks show, since when.
    state: Option<(Activity, u64)>,
    showing: Option<(Activity, u64)>,
    /// The last looks with a face, for standing up.
    recent: VecDeque<(u64, Look)>,
    /// A face lost as if standing up, when, and the sitting it left.
    leaving: Option<(u64, u32)>,
    said: Option<(Activity, u64)>,
}

impl Judge {
    /// A look at `now_us`, with its largest face if there was one.
    pub fn looked(&mut self, now_us: u64, face: Option<Look>) {
        self.looked_at = Some(now_us);
        let face = face.filter(|face| face.height >= NEAR);
        let Some(face) = face else {
            self.lost(now_us);
            return;
        };
        self.leaving = None;
        let new_sitting = self.last_face.is_none_or(|last| {
            now_us.saturating_sub(last) > u64::from(RoomActivity::SITTING_GAP_S) * 1_000_000
        });
        if new_sitting {
            self.sitting_from = Some(now_us);
            self.samples.clear();
            self.baseline = None;
            self.recent.clear();
        }
        self.last_face = Some(now_us);
        self.recent.push_back((now_us, face));
        while self
            .recent
            .front()
            .is_some_and(|(at, _)| now_us.saturating_sub(*at) > STOOD_WITHIN_US)
        {
            self.recent.pop_front();
        }
        let from = self.sitting_from.unwrap_or(now_us);
        if self.baseline.is_none() {
            self.samples.push(face);
            if now_us.saturating_sub(from) >= BASELINE_US && self.samples.len() >= BASELINE_LOOKS {
                let take =
                    |field: fn(&Look) -> f32| median(self.samples.iter().map(field).collect());
                self.baseline = Some(Look {
                    height: take(|look| look.height),
                    centre_y: take(|look| look.centre_y),
                    pitch: take(|look| look.pitch),
                });
                self.samples = Vec::new();
            }
        }
        let shows = self
            .baseline
            .map_or(Activity::AtDesk, |base| classify(face, base));
        if self.showing.is_none_or(|(state, _)| state != shows) {
            self.showing = Some((shows, now_us));
        }
        let (shows, since) = self.showing.unwrap_or((shows, now_us));
        let current = self.state.map(|(state, _)| state);
        if current != Some(shows)
            && (current.is_none_or(|state| state == Activity::Away)
                || now_us.saturating_sub(since) >= hold_of(shows))
        {
            self.state = Some((shows, now_us));
        }
    }

    /// A look with no face near: perhaps they stood up and left.
    fn lost(&mut self, now_us: u64) {
        if self.leaving.is_none()
            && self
                .last_face
                .is_some_and(|last| now_us.saturating_sub(last) <= STOOD_WITHIN_US)
            && let (Some((_, first)), Some((_, last))) = (self.recent.front(), self.recent.back())
            && first.centre_y - last.centre_y >= STOOD_RISE * first.height
            && last.height <= STOOD_SHRINK * first.height
        {
            let sitting = self
                .sitting_from
                .zip(self.last_face)
                .map_or(0, |(from, last)| ((last - from) / 1_000_000) as u32);
            self.leaving = Some((now_us, sitting));
        }
    }

    /// Someone stood up and left: once, [`STOOD_GONE_US`] after the face was
    /// lost rising, if it did not come back.
    pub fn stood_up(&mut self, now_us: u64) -> Option<StoodUp> {
        let (lost_at, sitting_s) = self.leaving?;
        if now_us.saturating_sub(lost_at) < STOOD_GONE_US {
            return None;
        }
        self.leaving = Some((u64::MAX / 2, 0));
        (lost_at < u64::MAX / 2).then_some(StoodUp { sitting_s })
    }

    /// What the face at the desk shows at `now_us`, while the eyes look.
    pub fn now(&mut self, now_us: u64) -> Option<RoomActivity> {
        let looked = self.looked_at?;
        if now_us.saturating_sub(looked) >= BLIND_AFTER_US {
            return None;
        }
        let away = self
            .last_face
            .is_none_or(|last| now_us.saturating_sub(last) >= AWAY_AFTER_US);
        if away && self.state.is_none_or(|(state, _)| state != Activity::Away) {
            let since = self
                .last_face
                .map_or(now_us, |last| last + AWAY_AFTER_US)
                .min(now_us);
            self.state = Some((Activity::Away, since));
            self.showing = None;
        }
        let (state, since) = self.state?;
        let sitting_s = if state == Activity::Away {
            0
        } else {
            self.sitting_from
                .map_or(0, |from| (now_us.saturating_sub(from) / 1_000_000) as u32)
        };
        Some(RoomActivity {
            state,
            for_s: (now_us.saturating_sub(since) / 1_000_000) as u32,
            sitting_s,
            baseline: self.baseline.is_some() && state != Activity::Away,
        })
    }

    /// What to say at `now_us`, if anything: once a second while the eyes
    /// look, and at once when the state changes.
    pub fn say(&mut self, now_us: u64) -> Option<RoomActivity> {
        let Some(now) = self.now(now_us) else {
            self.said = None;
            return None;
        };
        let every = RoomActivity::EVERY.as_micros() as u64;
        let due = self
            .said
            .is_none_or(|(state, at)| state != now.state || now_us.saturating_sub(at) >= every);
        due.then(|| {
            self.said = Some((now.state, now_us));
            now
        })
    }
}

/// One look against the sitting's baseline.
fn classify(face: Look, base: Look) -> Activity {
    let lower = (face.centre_y - base.centre_y) / base.height.max(f32::EPSILON);
    if face.height >= IN * base.height {
        Activity::LeaningIn
    } else if lower >= DOWN && face.pitch >= base.pitch + PITCH_DOWN {
        Activity::HeadDown
    } else if lower >= LOW {
        Activity::SittingLow
    } else if face.height <= BACK * base.height {
        Activity::LeaningBack
    } else {
        Activity::AtDesk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: u64 = 1_000_000;
    const SEATED: Look = Look {
        height: 0.22,
        centre_y: 0.45,
        pitch: 0.5,
    };

    /// Looks every 500 ms from `from` to `to` s, each `face`.
    fn looks(judge: &mut Judge, from: u64, to: u64, face: Option<Look>) {
        let mut at = from * S;
        while at < to * S {
            judge.looked(at, face);
            at += S / 2;
        }
    }

    fn state(judge: &mut Judge, at_s: u64) -> Option<Activity> {
        judge.now(at_s * S).map(|now| now.state)
    }

    /// Sat 2 minutes for the baseline, as `SEATED`.
    fn seated() -> Judge {
        let mut judge = Judge::default();
        looks(&mut judge, 0, 125, Some(SEATED));
        judge
    }

    #[test]
    fn at_the_desk_until_the_baseline_and_nothing_said_before_a_look_or_after_they_stop() {
        let mut judge = Judge::default();
        assert_eq!(judge.say(0), None);
        looks(&mut judge, 0, 60, Some(SEATED));
        let now = judge.now(60 * S).unwrap();
        assert_eq!(
            (now.state, now.baseline, now.sitting_s),
            (Activity::AtDesk, false, 60)
        );
        // A face leaning in before the baseline is in is still at the desk.
        looks(
            &mut judge,
            60,
            90,
            Some(Look {
                height: 0.4,
                ..SEATED
            }),
        );
        assert_eq!(state(&mut judge, 90), Some(Activity::AtDesk));
        looks(&mut judge, 90, 125, Some(SEATED));
        assert!(judge.now(125 * S).unwrap().baseline);
        // The eyes stop: nothing after 5 s.
        assert!(judge.now(129 * S).is_some());
        assert_eq!(judge.now(130 * S), None);
    }

    #[test]
    fn each_state_against_the_sitting_and_after_its_hold() {
        let cases = [
            (
                Look {
                    height: 0.30,
                    ..SEATED
                },
                Activity::LeaningIn,
                10,
            ),
            (
                Look {
                    height: 0.16,
                    ..SEATED
                },
                Activity::LeaningBack,
                10,
            ),
            (
                Look {
                    centre_y: 0.45 + 0.30 * 0.22,
                    pitch: 0.7,
                    ..SEATED
                },
                Activity::HeadDown,
                20,
            ),
            (
                Look {
                    centre_y: 0.45 + 0.6 * 0.22,
                    ..SEATED
                },
                Activity::SittingLow,
                120,
            ),
        ];
        for (face, expected, hold) in cases {
            let mut judge = seated();
            looks(&mut judge, 125, 125 + hold - 1, Some(face));
            assert_eq!(
                state(&mut judge, 125 + hold - 1),
                Some(Activity::AtDesk),
                "{expected:?}"
            );
            looks(&mut judge, 125 + hold - 1, 125 + hold + 1, Some(face));
            assert_eq!(
                state(&mut judge, 125 + hold + 1),
                Some(expected),
                "{expected:?}"
            );
            // Back to how they sat: at the desk after 5 s.
            looks(&mut judge, 125 + hold + 1, 125 + hold + 8, Some(SEATED));
            assert_eq!(
                state(&mut judge, 125 + hold + 8),
                Some(Activity::AtDesk),
                "{expected:?}"
            );
        }
        // Lower without looking down is not head down: sitting low, later.
        let mut judge = seated();
        let low_not_bowed = Look {
            centre_y: 0.45 + 0.30 * 0.22,
            ..SEATED
        };
        looks(&mut judge, 125, 160, Some(low_not_bowed));
        assert_eq!(state(&mut judge, 160), Some(Activity::AtDesk));
    }

    #[test]
    fn away_after_twenty_seconds_and_a_short_gap_keeps_the_sitting() {
        let mut judge = seated();
        looks(&mut judge, 125, 140, None);
        assert_eq!(
            state(&mut judge, 140),
            Some(Activity::AtDesk),
            "a look away"
        );
        looks(&mut judge, 140, 150, None);
        let away = judge.now(150 * S).unwrap();
        assert_eq!(
            (away.state, away.sitting_s, away.baseline),
            (Activity::Away, 0, false)
        );
        away.check().unwrap();
        // Back within 2 minutes: the same sitting, its baseline kept.
        looks(&mut judge, 150, 200, Some(SEATED));
        let back = judge.now(200 * S).unwrap();
        assert_eq!(
            (back.state, back.sitting_s, back.baseline),
            (Activity::AtDesk, 200, true)
        );
        // Gone past 2 minutes: a new sitting, its baseline again.
        looks(&mut judge, 200, 330, None);
        looks(&mut judge, 330, 340, Some(SEATED));
        let new = judge.now(340 * S).unwrap();
        assert_eq!((new.sitting_s, new.baseline), (10, false));
    }

    #[test]
    fn standing_up_is_said_once_when_the_face_rises_shrinks_and_is_gone() {
        let mut judge = seated();
        looks(&mut judge, 125, 130, Some(SEATED));
        // Rising out of the chair over a second, then gone.
        let steps = [0.45, 0.38, 0.30, 0.22];
        for (k, centre) in steps.iter().enumerate() {
            let shrink = 1.0 - 0.06 * k as f32;
            judge.looked(
                130 * S + k as u64 * 250_000,
                Some(Look {
                    centre_y: *centre,
                    height: 0.22 * shrink,
                    ..SEATED
                }),
            );
        }
        judge.looked(131 * S, None);
        assert_eq!(judge.stood_up(132 * S), None, "not gone 2 s yet");
        judge.looked(132 * S + S / 2, None);
        let up = judge.stood_up(133 * S).unwrap();
        assert_eq!(up.sitting_s, 130);
        assert_eq!(judge.stood_up(134 * S), None, "once");
        // A face that only looked away (no rise) is not standing up.
        let mut judge = seated();
        looks(&mut judge, 125, 130, Some(SEATED));
        judge.looked(131 * S, None);
        assert_eq!(judge.stood_up(140 * S), None);
        // Rising and back within 2 s: not standing up either.
        let mut judge = seated();
        for (k, centre) in steps.iter().enumerate() {
            judge.looked(
                130 * S + k as u64 * 250_000,
                Some(Look {
                    centre_y: *centre,
                    height: 0.22 * (1.0 - 0.06 * k as f32),
                    ..SEATED
                }),
            );
        }
        judge.looked(131 * S, None);
        judge.looked(132 * S, Some(SEATED));
        assert_eq!(judge.stood_up(134 * S), None);
    }

    #[test]
    fn it_is_said_once_a_second_and_at_once_on_a_change() {
        let mut judge = seated();
        let mut said = Vec::new();
        for turn in 1_250..1_600u64 {
            let at = turn * S / 10;
            if turn % 5 == 0 {
                let face = if turn >= 1_400 {
                    Look {
                        height: 0.30,
                        ..SEATED
                    }
                } else {
                    SEATED
                };
                judge.looked(at, Some(face));
            }
            if let Some(now) = judge.say(at) {
                now.check().unwrap();
                said.push((at, now.state));
            }
        }
        assert!(said.windows(2).all(|pair| pair[1].0 - pair[0].0 <= S));
        let leaned = said
            .iter()
            .position(|(_, state)| *state == Activity::LeaningIn)
            .unwrap();
        assert_eq!(
            said[leaned].0,
            150 * S,
            "said at once, 10 s after leaning in at 140 s"
        );
    }

    #[test]
    fn the_largest_face_gives_the_look_as_shares_of_the_picture() {
        let face = |y: f32, height: f32, nose: f32| faces::Face {
            score: 0.9,
            x: 100.0,
            y,
            width: height * 0.8,
            height,
            marks: [
                (120.0, y + 30.0),
                (160.0, y + 30.0),
                (140.0, nose),
                (125.0, y + 70.0),
                (155.0, y + 70.0),
            ],
        };
        let look = Look::largest(
            &[face(40.0, 60.0, 90.0), face(20.0, 100.0, 70.0)],
            (320, 240),
        )
        .unwrap();
        assert!((look.height - 100.0 / 240.0).abs() < 1e-6);
        assert!((look.centre_y - 70.0 / 240.0).abs() < 1e-6);
        // The nose 20 below the eyes of a 40 eyes-to-mouth span: 0.5.
        assert!((look.pitch - 0.5).abs() < 1e-6);
        assert_eq!(Look::largest(&[], (320, 240)), None);
    }
}
