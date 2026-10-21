//! Turning to the one the lamp answers (#1810, `LAMP_TURN_TO_ANSWERED=on`).
//!
//! The body looks at the face the eyes send it (`gaze.sock`), which is
//! otherwise the largest near one. With this on, when the voice begins to
//! speak, the eyes pick the face that was seen speaking (`speaking`, the
//! eyes' own judge) just before, and send that one for the whole answer
//! instead. It is picked once an answer and never again inside it: the
//! body's gaze turns to it in its own small, settled corrections, as a
//! person turns to whoever asked. When the answer ends, the choice is let
//! go and the body looks as before.
//!
//! Only a clear choice is made: exactly one face seen speaking in at least
//! [`LEAST_LOOKS`] looks over the [`BEFORE_US`] before the answer. Two
//! people talking, or nobody seen speaking, and nothing changes. A hand on
//! the lamp still wins: this only says which face the gaze is given, and
//! the body never moves a joint that meets resistance for a gaze.
//!
//! Nothing is kept: passing ids (`f3`) for [`BEFORE_US`], in memory only.
use crate::{
    faces::Face,
    sight::{Sight, choose},
};
use lamp_msgs::Voice;
use std::collections::VecDeque;

/// Where the eyes read whether to turn to the one answered: `on` only, and
/// it needs `LAMP_BUS=on` and `LAMP_EYES_SPEAKING=on` too.
pub const ENVIRONMENT: &str = "LAMP_TURN_TO_ANSWERED";
/// How far back from the answer's start faces seen speaking count: a line
/// and the lamp's thinking after it.
pub const BEFORE_US: u64 = 10_000_000;
/// Looks a face must read speaking in, over [`BEFORE_US`], to be the one
/// asking: under a second of talk at the eyes' four looks a second.
pub const LEAST_LOOKS: usize = 3;
/// A `voice/state` older than this says nothing: the voice says it once a
/// second.
pub const VOICE_FRESH_US: u64 = 3_000_000;

/// Whether the environment asks for it.
pub fn from_environment() -> Option<Answered> {
    (std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")).then(Answered::default)
}

#[derive(Debug, Default)]
pub struct Answered {
    /// Looks in the last [`BEFORE_US`] with a face speaking: when, and the
    /// passing ids that read speaking.
    looks: VecDeque<(u64, Vec<u32>)>,
    /// Whether an answer is under way: from its first `speaking` until the
    /// voice listens or rests.
    answering: bool,
    /// The face this answer turns to.
    held: Option<u32>,
}

impl Answered {
    /// One look: the passing ids that read speaking in it. Nothing is kept
    /// while an answer is under way: talk over one answer is not asking for
    /// the next.
    pub fn looked(&mut self, now_us: u64, talking: &[u32]) {
        if self.answering {
            return;
        }
        while self
            .looks
            .front()
            .is_some_and(|(at, _)| now_us.saturating_sub(*at) > BEFORE_US)
        {
            self.looks.pop_front();
        }
        if !talking.is_empty() {
            self.looks.push_back((now_us, talking.to_vec()));
        }
    }

    /// What the voice is doing now (`None`: nothing fresh from it). As an
    /// answer begins, its face is picked, once, and returned for the
    /// journal. It is held through a pause inside the turn (`thinking`, as
    /// between a filler and the answer) and let go only when the voice
    /// listens or rests, or says nothing.
    pub fn voice(&mut self, state: Option<Voice>, now_us: u64) -> Option<u32> {
        match state {
            Some(Voice::Speaking) if !self.answering => {
                self.answering = true;
                self.held = self.asker(now_us);
                // This answer's asking is spent: the next answer counts
                // only what is said after this one.
                self.looks.clear();
                self.held
            }
            Some(Voice::Speaking | Voice::Thinking) => None,
            Some(Voice::Listening | Voice::Resting) | None => {
                self.answering = false;
                self.held = None;
                None
            }
        }
    }

    /// The sight to send this look: the held face's while it is in view and
    /// near, found at `index` of `faces` (the detector's order, from
    /// `BusOut::index_of`), else `otherwise`. `None` while a held face's
    /// passing id is gone (out of view, or back under a new id): nothing is
    /// sent, so the body keeps its last aim instead of turning to someone
    /// else for the rest of the answer.
    pub fn sight(
        &self,
        faces: &[Face],
        index: Option<usize>,
        filled: (usize, usize),
        otherwise: Option<Sight>,
    ) -> Option<Option<Sight>> {
        match (self.held, index) {
            (None, _) => Some(otherwise),
            (Some(_), None) => None,
            (Some(_), Some(index)) => Some(
                faces
                    .get(index)
                    .and_then(|face| choose(std::slice::from_ref(face), filled))
                    .or(otherwise),
            ),
        }
    }

    /// The face to look at while the answer lasts, if one was picked.
    pub fn held(&self) -> Option<u32> {
        self.held
    }

    /// The one face seen speaking in at least [`LEAST_LOOKS`] looks lately,
    /// none when there is no such face or more than one.
    fn asker(&self, now_us: u64) -> Option<u32> {
        let mut counts: Vec<(u32, usize)> = Vec::new();
        for id in self
            .looks
            .iter()
            .filter(|(at, _)| now_us.saturating_sub(*at) <= BEFORE_US)
            .flat_map(|(_, ids)| ids)
        {
            match counts.iter_mut().find(|(seen, _)| seen == id) {
                Some((_, count)) => *count += 1,
                None => counts.push((*id, 1)),
            }
        }
        let mut speakers = counts
            .into_iter()
            .filter(|(_, count)| *count >= LEAST_LOOKS);
        match (speakers.next(), speakers.next()) {
            (Some((id, _)), None) => Some(id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOK: u64 = 250_000;

    /// Looks every quarter second from `from`, with these faces speaking.
    fn talk(answered: &mut Answered, from: u64, looks: u64, talking: &[u32]) -> u64 {
        for n in 0..looks {
            answered.looked(from + n * LOOK, talking);
        }
        from + looks * LOOK
    }

    #[test]
    fn the_one_face_seen_speaking_is_turned_to_once_for_the_answer() {
        let mut answered = Answered::default();
        let now = talk(&mut answered, 0, 8, &[2]);
        // A quiet second of thinking, a face 5 in view and silent.
        let now = talk(&mut answered, now, 4, &[]);
        assert_eq!(answered.voice(Some(Voice::Listening), now), None);
        assert_eq!(answered.voice(Some(Voice::Speaking), now), Some(2));
        assert_eq!(answered.held(), Some(2));
        // Someone else talking over the answer does not take the gaze.
        let later = talk(&mut answered, now, 12, &[5]);
        assert_eq!(answered.voice(Some(Voice::Speaking), later), None);
        assert_eq!(answered.held(), Some(2));
        // The answer over, it lets go.
        assert_eq!(answered.voice(Some(Voice::Listening), later), None);
        assert_eq!(answered.held(), None);
    }

    /// A filler, a pause (`thinking`) and then the answer, all one turn
    /// (#2270, the reviewer-lead's P2): the face picked as the filler began
    /// is kept, though someone else talked over it, and nothing is picked
    /// again.
    #[test]
    fn a_pause_inside_the_answer_keeps_the_face_and_picks_none_again() {
        let mut answered = Answered::default();
        let now = talk(&mut answered, 0, 8, &[2]);
        assert_eq!(answered.voice(Some(Voice::Speaking), now), Some(2));
        let now = talk(&mut answered, now, 8, &[5]);
        assert_eq!(answered.voice(Some(Voice::Thinking), now), None);
        assert_eq!(answered.held(), Some(2));
        assert_eq!(answered.voice(Some(Voice::Speaking), now), None);
        assert_eq!(answered.held(), Some(2));
        // A voice that says nothing fresh ends it.
        assert_eq!(answered.voice(None, now), None);
        assert_eq!(answered.held(), None);
    }

    /// Someone talking over one answer, and the one who asked it, are not
    /// counted for the next (#2270, the reviewer-lead's P3): a third person
    /// asking within 10 s is turned to.
    #[test]
    fn talk_before_and_over_an_answer_is_not_counted_for_the_next() {
        let mut answered = Answered::default();
        let now = talk(&mut answered, 0, 8, &[2]);
        assert_eq!(answered.voice(Some(Voice::Speaking), now), Some(2));
        let now = talk(&mut answered, now, 8, &[5]);
        answered.voice(Some(Voice::Listening), now);
        let now = talk(&mut answered, now, 8, &[7]);
        assert_eq!(answered.voice(Some(Voice::Speaking), now), Some(7));
    }

    /// The held face is sent while it is in view, the usual one otherwise.
    #[test]
    fn the_held_face_is_sent_while_in_view() {
        let face = |x: f32| Face {
            score: 0.9,
            x,
            y: 60.0,
            width: 60.0,
            height: 60.0,
            marks: [
                (x + 18.0, 80.0),
                (x + 42.0, 80.0),
                (x + 30.0, 93.0),
                (x + 18.0, 108.0),
                (x + 42.0, 108.0),
            ],
        };
        let faces = [face(20.0), face(240.0)];
        let usual = choose(&faces[..1], (320, 240));
        let mut answered = Answered::default();
        let now = talk(&mut answered, 0, 8, &[7]);
        // Nothing held: the usual sight, whatever the index.
        assert_eq!(
            answered.sight(&faces, Some(1), (320, 240), usual),
            Some(usual)
        );
        assert_eq!(answered.sight(&faces, None, (320, 240), usual), Some(usual));
        answered.voice(Some(Voice::Speaking), now);
        let turned = answered
            .sight(&faces, Some(1), (320, 240), usual)
            .unwrap()
            .unwrap();
        assert!(turned.x > 0.5, "{turned:?}");
        // An index the faces don't hold: the usual one.
        assert_eq!(
            answered.sight(&faces, Some(9), (320, 240), usual),
            Some(usual)
        );
    }

    /// The asker's passing id is lost mid-answer (#2270, the reviewer's
    /// P3): nothing is sent, so the body keeps its last aim, and the largest
    /// face is not taken, though it is in view.
    #[test]
    fn the_askers_id_lost_mid_answer_sends_nothing() {
        let face = Face {
            score: 0.9,
            x: 20.0,
            y: 60.0,
            width: 90.0,
            height: 90.0,
            marks: [
                (47.0, 80.0),
                (83.0, 80.0),
                (65.0, 93.0),
                (47.0, 108.0),
                (83.0, 108.0),
            ],
        };
        // Someone else's face, the largest in view, is the usual sight.
        let faces = [face];
        let largest = choose(&faces, (320, 240));
        assert!(largest.is_some());
        let mut answered = Answered::default();
        let now = talk(&mut answered, 0, 8, &[7]);
        assert_eq!(answered.voice(Some(Voice::Speaking), now), Some(7));
        // f7's id is gone: no index for it this look.
        assert_eq!(answered.sight(&faces, None, (320, 240), largest), None);
        assert_eq!(answered.held(), Some(7));
    }

    #[test]
    fn off_unless_set() {
        if std::env::var_os(ENVIRONMENT).is_none() {
            assert!(from_environment().is_none());
        }
    }

    #[test]
    fn two_faces_speaking_or_too_little_of_one_picks_none() {
        let mut both = Answered::default();
        let now = talk(&mut both, 0, 8, &[2, 5]);
        assert_eq!(both.voice(Some(Voice::Speaking), now), None);
        assert_eq!(both.held(), None);

        let mut brief = Answered::default();
        let now = talk(&mut brief, 0, LEAST_LOOKS as u64 - 1, &[2]);
        assert_eq!(brief.voice(Some(Voice::Speaking), now), None);

        // One who spoke long before the answer is not the one asking.
        let mut old = Answered::default();
        talk(&mut old, 0, 8, &[2]);
        assert_eq!(
            old.voice(Some(Voice::Speaking), 8 * LOOK + BEFORE_US + 1),
            None
        );
    }

    #[test]
    fn each_answer_picks_its_own_asker() {
        let mut answered = Answered::default();
        let now = talk(&mut answered, 0, 8, &[2]);
        assert_eq!(answered.voice(Some(Voice::Speaking), now), Some(2));
        let now = talk(&mut answered, now, 8, &[]);
        answered.voice(Some(Voice::Listening), now);
        // The other person asks next, after the first's talk has gone by.
        let now = talk(&mut answered, now + BEFORE_US, 8, &[5]);
        assert_eq!(answered.voice(Some(Voice::Speaking), now), Some(5));
    }
}
