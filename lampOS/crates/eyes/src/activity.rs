//! What the person at the desk is doing, as far as their face shows it, on
//! `room/activity`, and when they stood up and left, on `room/stood_up`
//! (#1678, `docs/presence.md`). The eyes see a face's box and five marks,
//! no body: so each state is read against this person's own sitting, its
//! baseline the median of the first [`BASELINE_US`] of looks at the desk,
//! never against a fixed line (a seated face was 0.23 of the picture on
//! lamp-06c1 and 0.16 to 0.27 on lamp-4ace, #941). The sitter is followed by
//! place, never taken as the largest face: in an office a colleague near the
//! desk is often the largest (lamp-75c9's scripted sitting, 19 Oct). Pure
//! arithmetic on the looks it is given: the bus is `bus_out`'s.
use crate::faces;
use crate::who::{SAME_PERSON, likeness};
use lamp_msgs::{Activity, RoomActivity, Side, StoodUp};
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
/// Leaning in: the face this much bigger than it sat (1.52 times, leaning
/// in at lamp-75c9).
const IN: f32 = 1.25;
/// Head down: the nose lower between the eyes and mouth by this much than
/// it sat. Not the face's centre: at lamp-75c9 a head bowed over the desk
/// came closer (1.21 times as tall) and only 0.06 of its height lower. This
/// line is the design's, not yet a run's: the journal says the pitch every
/// [`JOURNAL_US`] so a run can set it.
const PITCH_DOWN: f32 = 0.12;
/// How long a look's state must hold before it is said.
const HOLD_US: [(Activity, u64); 3] = [
    (Activity::AtDesk, 5_000_000),
    (Activity::LeaningIn, 10_000_000),
    (Activity::HeadDown, 20_000_000),
];
/// The sitter is the near face nearest their last place, within this many of
/// their face's heights of it; a near face further away is someone else.
pub(crate) const REACH: f32 = 1.0;
/// How often the journal says the sitter's look against the baseline.
pub const JOURNAL_US: u64 = 2_000_000;
/// With the eyes' recogniser (#1678's (d); the owner's yes, 19 Oct 15:57,
/// recorded on #1827; on a lamp only after Third's privacy read), the sitter
/// is told from others by face: a print of the sitter this often while the
/// sitting has fewer than [`MOST_PRINTS`], their mean the sitter's, held in
/// memory for that sitting only, never written, published or named.
const PRINT_EVERY_US: u64 = 20_000_000;
const MOST_PRINTS: usize = 6;
/// A face at the sitter's place after this long without them is the sitter
/// only once its print says so; one is taken at most every
/// [`CHECK_EVERY_US`]. Followed without such a gap, the sitter is never
/// printed again, so turning their head does not lose them.
const CHECK_AFTER_US: u64 = 2_000_000;
const CHECK_EVERY_US: u64 = 2_000_000;
/// Standing up: within this long before the face was lost, it rose by
/// [`STOOD_RISE`] of its height, or shrank to [`STOOD_AWAY`] of it. Which
/// one depends on the lamp's head: on lamp-52e6 resting (head low) the lead's
/// face rose 1.90 of its height at 1.02 as tall (row 14 try 4); with the
/// head up it fell 0.28 and shrank to 0.47 (try 5); on lamp-75c9 it rose
/// 0.84 and grew to 1.30 (Nine). A head turned while sitting shrank to
/// 0.84 at most, and leaning back sits at 0.68 or more (presence.md).
const STOOD_WITHIN_US: u64 = 2_000_000;
const STOOD_RISE: f32 = 0.5;
const STOOD_AWAY: f32 = 0.6;

/// Whether looks that went from `first` to `last` before the face was lost
/// are someone standing up: rising, or moving away.
fn standing(first: &Look, last: &Look) -> bool {
    first.centre_y - last.centre_y >= STOOD_RISE * first.height
        || last.height <= STOOD_AWAY * first.height
}
/// And was not seen again for this long.
const STOOD_GONE_US: u64 = 2_000_000;
/// The side of the picture someone left by: their last centre across it,
/// as a share of its width, short of this from an edge. Between the two,
/// near the middle, no side. Provisional: the arm hour sets it (#1669).
const LEFT_BY_LEFT: f32 = 0.35;
const LEFT_BY_RIGHT: f32 = 0.65;

/// One look's face, as shares of the picture's height, and how far down it
/// looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub height: f32,
    /// Across, in the same unit as `height`, so a distance is one measure.
    pub centre_x: f32,
    pub centre_y: f32,
    /// The centre across, as a share of the picture's width: 0 its left
    /// edge, 1 its right.
    pub across: f32,
    /// The nose between the eyes (0) and the mouth (1): more is lower,
    /// the head bowed.
    pub pitch: f32,
}

impl Look {
    /// Every face of a look, in a picture filled to `filled` pixels.
    pub fn all(found: &[faces::Face], filled: (usize, usize)) -> Vec<Self> {
        found.iter().map(|face| Self::of(face, filled)).collect()
    }

    fn of(face: &faces::Face, filled: (usize, usize)) -> Self {
        let tall = filled.1.max(1) as f32;
        let wide = filled.0.max(1) as f32;
        let [left_eye, right_eye, nose, mouth_left, mouth_right] = face.marks;
        let eyes = (left_eye.1 + right_eye.1) / 2.0;
        let mouth = (mouth_left.1 + mouth_right.1) / 2.0;
        let span = mouth - eyes;
        Self {
            height: face.height / tall,
            centre_x: (face.x + face.width / 2.0) / tall,
            centre_y: (face.y + face.height / 2.0) / tall,
            across: (face.x + face.width / 2.0) / wide,
            pitch: if span.abs() > f32::EPSILON {
                ((nose.1 - eyes) / span).clamp(-1.0, 2.0)
            } else {
                0.5
            },
        }
    }

    /// How far apart two faces' centres are.
    fn from(&self, other: &Look) -> f32 {
        (self.centre_x - other.centre_x).hypot(self.centre_y - other.centre_y)
    }
}

fn hold_of(state: Activity) -> u64 {
    HOLD_US
        .iter()
        .find(|(held, _)| *held == state)
        .map_or(0, |(_, us)| *us)
}

/// The mean of unit prints, itself of unit length.
fn mean(prints: &[Vec<f32>]) -> Vec<f32> {
    let mut sum = vec![0.0f32; prints.first().map_or(0, Vec::len)];
    for print in prints {
        for (total, value) in sum.iter_mut().zip(print) {
            *total += value;
        }
    }
    let length = sum.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length > f32::EPSILON {
        sum.iter_mut().for_each(|value| *value /= length);
    }
    sum
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
    /// The sitter's last look, where they are followed from.
    sitter: Option<Look>,
    /// The sitter's last looks, for standing up.
    recent: VecDeque<(u64, Look)>,
    /// A face lost as if standing up, when, the sitting it left, and the
    /// side of the picture it was last seen at.
    leaving: Option<(u64, u32, Option<Side>)>,
    said: Option<(Activity, u64)>,
    journaled_at: Option<u64>,
    /// The journal's line for a sitter's face lost, once a loss: how its
    /// last looks moved against what standing up needs (#1669 (e), row 14
    /// on lamp-52e6), and the look it was said for.
    lost_line: Option<String>,
    lost_said_for: Option<u64>,
    /// With the recogniser: the sitter's prints, and what is asked for.
    prints: Option<Prints>,
    /// This sitting's yawns (`yawn`, #3709): a count, nothing else.
    yawns: u32,
    /// The face-expressions yes is not kept: no yawn counts, and the count
    /// is 0 ([`Judge::yawns_allowed`]).
    yawns_refused: bool,
}

/// The sitter's prints, for this sitting only, and the print asked for.
#[derive(Default)]
struct Prints {
    taken: Vec<Vec<f32>>,
    asked_at: Option<u64>,
    asking: Option<Ask>,
}

/// Never the prints themselves, so no debug line can show one.
impl std::fmt::Debug for Prints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prints")
            .field("taken", &self.taken.len())
            .field("asked_at", &self.asked_at)
            .field("asking", &self.asking)
            .finish()
    }
}

/// A print asked of a face in the last look, by its place in that look.
#[derive(Clone, Copy, Debug)]
enum Ask {
    /// The sitter's own, for their print.
    Sitter(usize),
    /// A face at the sitter's place after a gap, to tell whether it is them.
    Check(usize, Look),
}

impl Judge {
    /// A judge that tells the sitter from others by face: the caller gives
    /// the prints it asks for ([`Judge::wants_print`], [`Judge::printed`]).
    pub fn with_prints() -> Self {
        Self {
            prints: Some(Prints::default()),
            ..Self::default()
        }
    }

    /// A look at `now_us`, with every face found in it.
    pub fn looked(&mut self, now_us: u64, faces: &[Look]) {
        self.looked_at = Some(now_us);
        let Some((index, face)) = self.sitter_in(now_us, faces) else {
            if let Some(prints) = self.prints.as_mut() {
                prints.asking = None;
            }
            self.lost(now_us);
            return;
        };
        let gap = |last: u64| now_us.saturating_sub(last);
        let after_gap = self.last_face.is_some_and(|last| {
            gap(last) >= CHECK_AFTER_US
                && gap(last) <= u64::from(RoomActivity::SITTING_GAP_S) * 1_000_000
        });
        if let Some(prints) = self.prints.as_mut() {
            prints.asking = None;
            if after_gap && self.sitter.is_some() && !prints.taken.is_empty() {
                prints.asking = Some(Ask::Check(index, face));
                self.lost(now_us);
                return;
            }
        }
        self.accept(now_us, face);
        if let Some(prints) = self.prints.as_mut()
            && prints.taken.len() < MOST_PRINTS
            && prints
                .asked_at
                .is_none_or(|at| now_us.saturating_sub(at) >= PRINT_EVERY_US)
        {
            prints.asking = Some(Ask::Sitter(index));
        }
    }

    /// Which face of the last look to print, if a print is wanted now: the
    /// caller prints it (or can't, a face side-on) and gives it to
    /// [`Judge::printed`].
    pub fn wants_print(&mut self, now_us: u64) -> Option<usize> {
        let prints = self.prints.as_mut()?;
        let index = match prints.asking? {
            Ask::Sitter(index) => index,
            Ask::Check(index, _)
                if prints
                    .asked_at
                    .is_none_or(|at| now_us.saturating_sub(at) >= CHECK_EVERY_US) =>
            {
                index
            }
            Ask::Check(..) => return None,
        };
        prints.asked_at = Some(now_us);
        Some(index)
    }

    /// The print asked for, or none if it could not be taken. A face checked
    /// at the sitter's place is the sitter only if it is alike enough
    /// (`who::SAME_PERSON`); what was decided is the journal's line, a
    /// number only.
    pub fn printed(&mut self, now_us: u64, print: Option<&[f32]>) -> Option<String> {
        let prints = self.prints.as_mut()?;
        match prints.asking.take()? {
            Ask::Sitter(_) => {
                prints.taken.extend(print.map(<[f32]>::to_vec));
                None
            }
            Ask::Check(_, face) => {
                let Some(print) = print else {
                    return Some(
                        "a face at the sitter's place could not be printed: not the sitter yet"
                            .into(),
                    );
                };
                let alike = likeness(print, &mean(&prints.taken));
                let same = alike >= SAME_PERSON;
                if same {
                    self.accept(now_us, face);
                }
                Some(format!(
                    "a face at the sitter's place, likeness {alike:.2}: {}",
                    if same { "the sitter" } else { "not the sitter" }
                ))
            }
        }
    }

    /// The sitter's face in a look: follow them, start a sitting if none.
    fn accept(&mut self, now_us: u64, face: Look) {
        self.leaving = None;
        let new_sitting = self.last_face.is_none_or(|last| {
            now_us.saturating_sub(last) > u64::from(RoomActivity::SITTING_GAP_S) * 1_000_000
        });
        if new_sitting {
            self.sitting_from = Some(now_us);
            self.yawns = 0;
            self.samples.clear();
            self.baseline = None;
            self.recent.clear();
            self.forget_prints();
        }
        self.sitter = Some(face);
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
                    centre_x: take(|look| look.centre_x),
                    centre_y: take(|look| look.centre_y),
                    across: take(|look| look.across),
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

    /// The sitter in a look: within a sitting, the near face nearest their
    /// last place, within [`REACH`] of it or of where they sat (the
    /// baseline), whoever is larger; outside one, the largest near face, who
    /// starts it.
    fn sitter_in(&self, now_us: u64, faces: &[Look]) -> Option<(usize, Look)> {
        let near = faces
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, face)| face.height >= NEAR);
        let sitting = self.last_face.is_some_and(|last| {
            now_us.saturating_sub(last) <= u64::from(RoomActivity::SITTING_GAP_S) * 1_000_000
        });
        let base = self.baseline;
        let within = |face: &Look, last: &Look| {
            face.from(last) <= REACH * last.height
                || base.is_some_and(|base| face.from(&base) <= REACH * base.height)
        };
        match self.sitter.filter(|_| sitting) {
            Some(last) => near
                .filter(|(_, face)| within(face, &last))
                .min_by(|(_, a), (_, b)| a.from(&last).total_cmp(&b.from(&last))),
            None => near.max_by(|(_, a), (_, b)| a.height.total_cmp(&b.height)),
        }
    }

    /// The sitting is over, or the camera closed: the prints go.
    pub fn forget_prints(&mut self) {
        if let Some(prints) = self.prints.as_mut() {
            *prints = Prints::default();
        }
        // The camera closed, or the sitting's gap passed: what the face gave
        // goes with the prints (Reviewer-three on #3842).
        self.yawns = 0;
    }

    /// Whether the face-expressions yes is kept, as the eyes last read it:
    /// without it the sitting's yawns are forgotten at once and none counts
    /// until it is back (Fourteen and the reviewer-lead on #3842).
    pub fn yawns_allowed(&mut self, yes: bool) {
        self.yawns_refused = !yes;
        if !yes {
            self.yawns = 0;
        }
    }

    /// The journal's line, every [`JOURNAL_US`] while a sitting with its
    /// baseline is followed: the sitter's last look against it, numbers only,
    /// so a run can set the lines.
    pub fn journal(&mut self, now_us: u64) -> Option<String> {
        let (face, base) = self.sitter.zip(self.baseline)?;
        let (state, _) = self.state?;
        if state == Activity::Away
            || self
                .journaled_at
                .is_some_and(|at| now_us.saturating_sub(at) < JOURNAL_US)
        {
            return None;
        }
        self.journaled_at = Some(now_us);
        // Not seen for a while (until Away, `AWAY_AFTER_US` on): say so, not
        // the last look again, which reads as a face held still (lamp-75c9,
        // 20 Oct: ten equal lines after the sitter had gone, #1678).
        let unseen_us = self.last_face.map_or(0, |last| now_us.saturating_sub(last));
        if unseen_us >= JOURNAL_US {
            return Some(format!(
                "sitter not seen for {} s, {state:?}",
                unseen_us / 1_000_000
            ));
        }
        Some(format!(
            "sitter {:.2} as tall as sat, {:.2} of its height lower, pitch {:.2} (sat {:.2}), {state:?}",
            face.height / base.height.max(f32::EPSILON),
            (face.centre_y - base.centre_y) / base.height.max(f32::EPSILON),
            face.pitch,
            base.pitch,
        ))
    }

    /// A look with no face near: perhaps they stood up and left.
    fn lost(&mut self, now_us: u64) {
        if let (Some(last_at), Some((_, first)), Some((_, last))) =
            (self.last_face, self.recent.front(), self.recent.back())
            && self.lost_said_for != Some(last_at)
            && now_us.saturating_sub(last_at) <= STOOD_WITHIN_US
        {
            self.lost_said_for = Some(last_at);
            let height = first.height.max(f32::EPSILON);
            let (rise, size) = (
                (first.centre_y - last.centre_y) / height,
                last.height / height,
            );
            self.lost_line = Some(format!(
                "sitter's face lost: over its last {} looks it rose {rise:.2} of its height (standing up needs {STOOD_RISE:.2}) and was {size:.2} as tall (or {STOOD_AWAY:.2} or less), last {:.2} across, {:.2} tall: {}",
                self.recent.len(),
                last.across,
                last.height,
                if standing(first, last) {
                    "rising"
                } else {
                    "not standing up"
                }
            ));
        }
        if self.leaving.is_none()
            && self
                .last_face
                .is_some_and(|last| now_us.saturating_sub(last) <= STOOD_WITHIN_US)
            && let (Some((_, first)), Some((_, last))) = (self.recent.front(), self.recent.back())
            && standing(first, last)
        {
            let sitting = self
                .sitting_from
                .zip(self.last_face)
                .map_or(0, |(from, last)| ((last - from) / 1_000_000) as u32);
            let side = if last.across < LEFT_BY_LEFT {
                Some(Side::Left)
            } else if last.across > LEFT_BY_RIGHT {
                Some(Side::Right)
            } else {
                None
            };
            self.leaving = Some((now_us, sitting, side));
        }
    }

    /// The journal's line for the sitter's face lost, once a loss: numbers
    /// only, so a run can tell why a leave was or wasn't a standing up.
    pub fn lost_journal(&mut self) -> Option<String> {
        self.lost_line.take()
    }

    /// Someone stood up and left: once, [`STOOD_GONE_US`] after the face was
    /// lost rising, if it did not come back.
    pub fn stood_up(&mut self, now_us: u64) -> Option<StoodUp> {
        let (lost_at, sitting_s, side) = self.leaving?;
        if now_us.saturating_sub(lost_at) < STOOD_GONE_US {
            return None;
        }
        self.leaving = Some((u64::MAX / 2, 0, None));
        (lost_at < u64::MAX / 2).then_some(StoodUp { sitting_s, side })
    }

    /// A yawn seen at `now_us` (`yawn::Yawns`): counted in the sitting, if
    /// someone is at the desk.
    pub fn yawned(&mut self, now_us: u64) {
        if !self.yawns_refused
            && self
                .last_face
                .is_some_and(|last| now_us.saturating_sub(last) < AWAY_AFTER_US)
        {
            self.yawns = self.yawns.saturating_add(1);
        }
    }

    /// What the face at the desk shows at `now_us`, while the eyes look.
    pub fn now(&mut self, now_us: u64) -> Option<RoomActivity> {
        // Before anything else, so the prints go with the sitting whether or
        // not the eyes are looking (Third's read of #1827).
        if self.last_face.is_some_and(|last| {
            now_us.saturating_sub(last) > u64::from(RoomActivity::SITTING_GAP_S) * 1_000_000
        }) {
            self.forget_prints();
        }
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
            yawns: if state == Activity::Away {
                0
            } else {
                self.yawns
            },
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

/// One look against the sitting's baseline. Head down first: a head bowed
/// over the desk comes closer too, and is not leaning in.
fn classify(face: Look, base: Look) -> Activity {
    if face.pitch >= base.pitch + PITCH_DOWN {
        Activity::HeadDown
    } else if face.height >= IN * base.height {
        Activity::LeaningIn
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
        centre_x: 0.6,
        centre_y: 0.45,
        across: 0.45,
        pitch: 0.5,
    };

    /// Looks every 500 ms from `from` to `to` s, each `face`.
    fn looks(judge: &mut Judge, from: u64, to: u64, face: Option<Look>) {
        let mut at = from * S;
        while at < to * S {
            judge.looked(at, face.as_slice());
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

    /// A sitting's yawns (#3709): counted while someone sits, none while
    /// away, and a new sitting begins at none.
    #[test]
    fn yawns_are_the_sittings_and_go_with_it() {
        let mut judge = Judge::default();
        judge.yawned(0);
        looks(&mut judge, 0, 10, Some(SEATED));
        assert_eq!(judge.now(10 * S).unwrap().yawns, 0, "none before a face");
        judge.yawned(10 * S);
        judge.yawned(10 * S);
        assert_eq!(judge.now(10 * S).unwrap().yawns, 2);
        // Away: none said, and a yawn then is nobody's.
        looks(&mut judge, 10, 40, None);
        let away = judge.now(40 * S).unwrap();
        assert_eq!((away.state, away.yawns), (Activity::Away, 0));
        judge.yawned(40 * S);
        // Back within the gap: the same sitting, its two yawns.
        looks(&mut judge, 40, 50, Some(SEATED));
        assert_eq!(judge.now(50 * S).unwrap().yawns, 2);
        // Gone past the gap: a new sitting, with none.
        looks(&mut judge, 50, 200, None);
        looks(&mut judge, 200, 210, Some(SEATED));
        assert_eq!(judge.now(210 * S).unwrap().yawns, 0);
    }

    /// The face-expressions yes withdrawn mid-sitting: the count is 0 at
    /// once, a yawn then counts nothing, and with the yes back they count
    /// again from 0. A camera close forgets it too (#3842).
    #[test]
    fn yawns_go_with_the_yes_and_with_the_camera() {
        let mut judge = Judge::default();
        looks(&mut judge, 0, 10, Some(SEATED));
        judge.yawned(10 * S);
        judge.yawned(10 * S);
        assert_eq!(judge.now(10 * S).unwrap().yawns, 2);
        judge.yawns_allowed(false);
        assert_eq!(judge.now(10 * S).unwrap().yawns, 0);
        judge.yawned(11 * S);
        assert_eq!(judge.now(11 * S).unwrap().yawns, 0, "not while withdrawn");
        judge.yawns_allowed(true);
        judge.yawned(12 * S);
        assert_eq!(judge.now(12 * S).unwrap().yawns, 1);
        judge.forget_prints();
        assert_eq!(judge.now(12 * S).unwrap().yawns, 0, "the camera closed");
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
                    pitch: 0.7,
                    ..SEATED
                },
                Activity::HeadDown,
                20,
            ),
            // Bowed over the desk, closer too (1.21 times at lamp-75c9, and
            // more): head down, not leaning in.
            (
                Look {
                    height: 0.22 * 1.35,
                    centre_y: 0.45 + 0.06 * 0.22,
                    pitch: 0.7,
                    ..SEATED
                },
                Activity::HeadDown,
                20,
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
        // Lower, or further back, without looking down: at the desk (sitting
        // low and leaning back could not be seen from lamp-75c9's place).
        for other in [
            Look {
                centre_y: 0.45 + 0.6 * 0.22,
                ..SEATED
            },
            Look {
                height: 0.16,
                ..SEATED
            },
        ] {
            let mut judge = seated();
            looks(&mut judge, 125, 260, Some(other));
            assert_eq!(state(&mut judge, 260), Some(Activity::AtDesk));
        }
    }

    /// lamp-75c9's office: someone else near the desk, larger than the
    /// sitter, is not the sitter; the sitter is followed as they move.
    #[test]
    fn the_sitter_is_followed_by_place_and_a_larger_face_elsewhere_is_someone_else() {
        let colleague = Look {
            height: 0.40,
            centre_x: 1.2,
            ..SEATED
        };
        // Sat with a colleague in view: the colleague, larger, is not leaning in.
        let mut judge = seated();
        let mut at = 125 * S;
        while at < 160 * S {
            judge.looked(at, &[colleague, SEATED]);
            at += S / 2;
        }
        assert_eq!(state(&mut judge, 160), Some(Activity::AtDesk));
        // The sitter leaves; the colleague stays: away after 20 s, never them.
        while at < 190 * S {
            judge.looked(at, &[colleague]);
            at += S / 2;
        }
        assert_eq!(state(&mut judge, 190), Some(Activity::Away));
        // Leaning in and across, a step at a time: still the sitter, and
        // leaning in, while a colleague stands where they started.
        let mut judge = seated();
        let mut at = 125 * S;
        for step in 0..40u64 {
            let k = (step as f32 / 8.0).min(1.0);
            let leaning = Look {
                height: 0.22 * (1.0 + 0.5 * k),
                centre_x: 0.6 + 0.3 * k,
                ..SEATED
            };
            let faces: Vec<Look> = if step >= 8 {
                vec![SEATED, leaning]
            } else {
                vec![leaning]
            };
            judge.looked(at, &faces);
            at += S / 2;
        }
        assert_eq!(state(&mut judge, 145), Some(Activity::LeaningIn));
        // Outside a sitting, the largest near face starts one.
        let mut judge = Judge::default();
        judge.looked(0, &[SEATED, colleague]);
        assert_eq!(judge.sitter, Some(colleague));
    }

    const THEM: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
    const SOMEONE_ELSE: [f32; 4] = [0.0, 1.0, 0.0, 0.0];

    /// Looks every 500 ms from `from` to `to` s, each `faces`, every print
    /// asked for answered with `print`; the journal's lines.
    fn printed_looks(
        judge: &mut Judge,
        from: u64,
        to: u64,
        faces: &[Look],
        print: Option<&[f32]>,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        let mut at = from * S;
        while at < to * S {
            judge.looked(at, faces);
            if judge.wants_print(at).is_some() {
                lines.extend(judge.printed(at, print));
            }
            judge.now(at);
            at += S / 2;
        }
        lines
    }

    /// Sat 2 minutes, by face, as `SEATED`.
    fn seated_by_face() -> Judge {
        let mut judge = Judge::with_prints();
        printed_looks(&mut judge, 0, 125, &[SEATED], Some(&THEM));
        judge
    }

    /// #1678's (d): someone else in the sitter's own place
    /// is not the sitter, told by face; the sitter back is.
    #[test]
    fn someone_else_in_the_sitters_place_is_told_by_face() {
        let mut judge = seated_by_face();
        assert_eq!(judge.prints.as_ref().unwrap().taken.len(), MOST_PRINTS);
        // The sitter leaves; a colleague sits where they sat.
        printed_looks(&mut judge, 125, 130, &[], None);
        let lines = printed_looks(&mut judge, 130, 160, &[SEATED], Some(&SOMEONE_ELSE));
        assert_eq!(state(&mut judge, 160), Some(Activity::Away));
        assert!(
            lines.iter().all(|line| line.ends_with("not the sitter")),
            "{lines:?}"
        );
        assert_eq!(lines.len(), 15, "one check every 2 s: {lines:?}");
        // The sitter comes back: the same sitting, its baseline kept.
        let lines = printed_looks(&mut judge, 160, 170, &[SEATED], Some(&THEM));
        assert_eq!(
            lines.first().map(String::as_str),
            Some("a face at the sitter's place, likeness 1.00: the sitter")
        );
        let back = judge.now(170 * S).unwrap();
        assert_eq!(
            (back.state, back.sitting_s, back.baseline),
            (Activity::AtDesk, 170, true)
        );
        // A face that can't be printed (side-on) is not the sitter yet.
        let mut judge = seated_by_face();
        printed_looks(&mut judge, 125, 130, &[], None);
        let lines = printed_looks(&mut judge, 130, 160, &[SEATED], None);
        assert_eq!(state(&mut judge, 160), Some(Activity::Away));
        assert!(
            lines
                .iter()
                .all(|line| line.ends_with("not the sitter yet"))
        );
    }

    #[test]
    fn the_sitter_followed_without_a_gap_is_never_printed_and_the_prints_go_with_the_sitting() {
        let mut judge = seated_by_face();
        // Followed, with a gap under 2 s: no print asked for.
        let mut at = 125 * S;
        while at < 200 * S {
            let faces: &[Look] = if (150 * S..151 * S).contains(&at) {
                &[]
            } else {
                &[SEATED]
            };
            judge.looked(at, faces);
            assert_eq!(judge.wants_print(at), None, "at {at}");
            at += S / 2;
        }
        assert_eq!(state(&mut judge, 200), Some(Activity::AtDesk));
        // The sitting ends: its prints go, and whoever sits next starts a
        // new sitting without being checked against them.
        printed_looks(&mut judge, 200, 330, &[], None);
        assert!(judge.prints.as_ref().unwrap().taken.is_empty());
        let lines = printed_looks(&mut judge, 330, 340, &[SEATED], Some(&SOMEONE_ELSE));
        assert!(lines.is_empty(), "{lines:?}");
        let new = judge.now(340 * S).unwrap();
        assert_eq!((new.state, new.sitting_s), (Activity::AtDesk, 10));
        // Without the recogniser nothing is asked for.
        let mut judge = seated();
        assert_eq!(judge.wants_print(125 * S), None);
    }

    /// Third's read of #1827: the prints go when the sitting ends even with
    /// the eyes not looking (the camera muted), and when the camera closes;
    /// and no debug line shows one.
    #[test]
    fn the_prints_go_while_the_eyes_are_blind_and_when_the_camera_closes() {
        let taken = |judge: &Judge| judge.prints.as_ref().unwrap().taken.len();
        // Muted after sitting: no look at all, then past the sitting's gap.
        let mut judge = seated_by_face();
        assert_eq!(taken(&judge), MOST_PRINTS);
        // The last look was at 124.5 s.
        assert_eq!(judge.now(124 * S + 2 * 60 * S), None, "not looking");
        assert_eq!(taken(&judge), MOST_PRINTS, "within the sitting's gap");
        assert_eq!(judge.now(125 * S + 2 * 60 * S + S), None);
        assert_eq!(taken(&judge), 0);
        // The camera closed: gone at once.
        let mut judge = seated_by_face();
        judge.forget_prints();
        assert_eq!(taken(&judge), 0);
        // A print never shows in a debug line: only how many there are.
        let mut judge = Judge::with_prints();
        printed_looks(&mut judge, 0, 30, &[SEATED], Some(&[0.6, 0.8, 0.0, 0.0]));
        let shown = format!("{judge:?}");
        assert!(shown.contains("taken: 2"), "{shown}");
        assert!(!shown.contains("0.8"), "{shown}");
    }

    #[test]
    fn the_journal_says_the_sitters_look_against_the_baseline_every_two_seconds() {
        let mut judge = Judge::default();
        looks(&mut judge, 0, 60, Some(SEATED));
        assert_eq!(judge.journal(60 * S), None, "no baseline yet");
        let mut judge = seated();
        judge.looked(
            125 * S,
            &[Look {
                height: 0.22 * 1.1,
                pitch: 0.6,
                ..SEATED
            }],
        );
        assert_eq!(
            judge.journal(125 * S).as_deref(),
            Some(
                "sitter 1.10 as tall as sat, 0.00 of its height lower, pitch 0.60 (sat 0.50), AtDesk"
            )
        );
        assert_eq!(judge.journal(126 * S), None);
        // No look since 125 s: not seen, never the last look again (#1678).
        assert_eq!(
            judge.journal(127 * S).as_deref(),
            Some("sitter not seen for 2 s, AtDesk")
        );
        assert_eq!(
            judge.journal(139 * S).as_deref(),
            Some("sitter not seen for 14 s, AtDesk")
        );
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
    fn standing_up_is_said_once_when_the_face_rises_and_is_gone() {
        let mut judge = seated();
        looks(&mut judge, 125, 130, Some(SEATED));
        // Rising out of the chair over a second, then gone.
        let steps = [0.45, 0.38, 0.30, 0.22];
        for (k, centre) in steps.iter().enumerate() {
            let shrink = 1.0 - 0.06 * k as f32;
            judge.looked(
                130 * S + k as u64 * 250_000,
                &[Look {
                    centre_y: *centre,
                    height: 0.22 * shrink,
                    ..SEATED
                }],
            );
        }
        judge.looked(131 * S, &[]);
        let line = judge.lost_journal().unwrap();
        assert!(
            line.starts_with(
                "sitter's face lost: over its last 6 looks it rose 1.05 of its height"
            ) && line.ends_with(": rising"),
            "{line}"
        );
        assert_eq!(judge.lost_journal(), None, "once a loss");
        assert_eq!(judge.stood_up(132 * S), None, "not gone 2 s yet");
        judge.looked(132 * S + S / 2, &[]);
        let up = judge.stood_up(133 * S).unwrap();
        assert_eq!(up.sitting_s, 130);
        assert_eq!(up.side, None, "left from the middle: no side");
        assert_eq!(judge.stood_up(134 * S), None, "once");
        // A face that only looked away (no rise) is not standing up, and
        // the journal says by how much.
        let mut judge = seated();
        looks(&mut judge, 125, 130, Some(SEATED));
        judge.looked(131 * S, &[]);
        let line = judge.lost_journal().unwrap();
        assert!(
            line.contains("rose 0.00 of its height") && line.ends_with(": not standing up"),
            "{line}"
        );
        judge.looked(132 * S, &[]);
        assert_eq!(judge.lost_journal(), None, "said once, not every look");
        assert_eq!(judge.stood_up(140 * S), None);
        // Rising and back within 2 s: not standing up either.
        let mut judge = seated();
        for (k, centre) in steps.iter().enumerate() {
            judge.looked(
                130 * S + k as u64 * 250_000,
                &[Look {
                    centre_y: *centre,
                    height: 0.22 * (1.0 - 0.06 * k as f32),
                    ..SEATED
                }],
            );
        }
        judge.looked(131 * S, &[]);
        judge.looked(132 * S, &[SEATED]);
        assert_eq!(judge.stood_up(134 * S), None);
    }

    /// For watching them go (#1669): the side of the picture they were last
    /// seen at as they rose, a third of it or less from an edge; near the
    /// middle, none.
    #[test]
    fn standing_up_says_the_side_of_the_picture_they_left_by() {
        for (across, side) in [
            (0.2, Some(Side::Left)),
            (0.8, Some(Side::Right)),
            (0.5, None),
        ] {
            let sat = Look { across, ..SEATED };
            let mut judge = Judge::default();
            looks(&mut judge, 0, 130, Some(sat));
            for (k, centre) in [0.45, 0.38, 0.30, 0.22].iter().enumerate() {
                judge.looked(
                    130 * S + k as u64 * 250_000,
                    &[Look {
                        centre_y: *centre,
                        height: 0.22 * (1.0 - 0.06 * k as f32),
                        ..sat
                    }],
                );
            }
            judge.looked(131 * S, &[]);
            judge.looked(132 * S + S / 2, &[]);
            assert_eq!(judge.stood_up(133 * S).unwrap().side, side, "{across}");
        }
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
                judge.looked(at, &[face]);
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
    fn each_face_gives_its_look_as_shares_of_the_pictures_height() {
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
        let looks = Look::all(
            &[face(40.0, 60.0, 90.0), face(20.0, 100.0, 70.0)],
            (320, 240),
        );
        assert_eq!(looks.len(), 2);
        let look = looks[1];
        assert!((look.height - 100.0 / 240.0).abs() < 1e-6);
        assert!((look.centre_x - 140.0 / 240.0).abs() < 1e-6);
        assert!((look.centre_y - 70.0 / 240.0).abs() < 1e-6);
        // The nose 20 below the eyes of a 40 eyes-to-mouth span: 0.5.
        assert!((look.pitch - 0.5).abs() < 1e-6);
        assert!(Look::all(&[], (320, 240)).is_empty());
    }

    /// Row 14 try 4 on lamp-52e6 (#2196's line): the lead stood straight up
    /// and the face rose 1.90 of its height at the same size, out of the
    /// picture; on lamp-75c9 it rose and grew. Both are standing up; a face
    /// only turned (no rise) is not.
    #[test]
    fn a_face_rising_out_of_the_picture_at_the_same_size_is_standing_up() {
        let mut judge = seated();
        looks(&mut judge, 125, 130, Some(SEATED));
        for (k, centre) in [0.45, 0.35, 0.22, 0.10].iter().enumerate() {
            judge.looked(
                130 * S + k as u64 * 250_000,
                &[Look {
                    centre_y: *centre,
                    ..SEATED
                }],
            );
        }
        judge.looked(131 * S, &[]);
        let line = judge.lost_journal().unwrap();
        assert!(
            line.contains("as tall") && line.ends_with(": rising"),
            "{line}"
        );
        judge.looked(133 * S, &[]);
        assert!(judge.stood_up(133 * S).is_some());
        // lamp-75c9 (Nine, #1678): leaning toward the lamp first, then up,
        // the face rose 0.84 of its height and grew to 1.30 as tall as it
        // sat. Standing up too.
        let mut judge = seated();
        looks(&mut judge, 125, 130, Some(SEATED));
        for (k, (centre, grown)) in [(0.45, 1.0), (0.40, 1.15), (0.33, 1.25), (0.265, 1.30)]
            .iter()
            .enumerate()
        {
            judge.looked(
                130 * S + k as u64 * 250_000,
                &[Look {
                    centre_y: *centre,
                    height: SEATED.height * grown,
                    ..SEATED
                }],
            );
        }
        judge.looked(131 * S, &[]);
        let line = judge.lost_journal().unwrap();
        assert!(
            line.contains("rose 0.84 of its height")
                && line.contains("was 1.30 as tall")
                && line.ends_with(": rising"),
            "{line}"
        );
        judge.looked(133 * S, &[]);
        assert!(judge.stood_up(133 * S).is_some());
    }

    /// Row 14 try 5 on lamp-52e6, the lamp's head up: standing up, the face
    /// fell 0.28 of its height and shrank to 0.47 as tall, then was gone:
    /// moving away. A head turned while sitting (try 5 at 15:23:42: down
    /// 0.25, 0.84 as tall) is not.
    #[test]
    fn a_face_shrinking_away_is_standing_up_and_a_turned_head_is_not() {
        // As on 52e6, a face sat 0.30 tall, 0.14 at the end of the leave:
        // still a near face (`NEAR`) all the way.
        let tall = Look {
            height: 0.30,
            ..SEATED
        };
        let mut judge = Judge::default();
        looks(&mut judge, 0, 130, Some(tall));
        for (k, (centre, size)) in [(0.45, 1.0), (0.48, 0.80), (0.50, 0.62), (0.51, 0.47)]
            .iter()
            .enumerate()
        {
            judge.looked(
                130 * S + k as u64 * 250_000,
                &[Look {
                    centre_y: *centre,
                    height: 0.30 * size,
                    ..SEATED
                }],
            );
        }
        judge.looked(131 * S, &[]);
        let line = judge.lost_journal().unwrap();
        assert!(
            line.contains("was 0.47 as tall") && line.ends_with(": rising"),
            "{line}"
        );
        judge.looked(133 * S, &[]);
        assert!(judge.stood_up(133 * S).is_some());
        // A head turned while sitting: down a little, 0.84 as tall, gone.
        let mut judge = seated();
        looks(&mut judge, 125, 130, Some(SEATED));
        for (k, (centre, size)) in [(0.45, 1.0), (0.48, 0.92), (0.505, 0.84)]
            .iter()
            .enumerate()
        {
            judge.looked(
                130 * S + k as u64 * 250_000,
                &[Look {
                    centre_y: *centre,
                    height: SEATED.height * size,
                    ..SEATED
                }],
            );
        }
        judge.looked(131 * S, &[]);
        assert!(judge.lost_journal().unwrap().ends_with(": not standing up"));
        judge.looked(133 * S, &[]);
        assert_eq!(judge.stood_up(133 * S), None);
    }
}
