//! The session person (`LAMP_SESSION_PERSON`, the memory owner's decisions
//! of 23 Nov on the session-user proposal): one person per conversation
//! whose lines a passer-by's face does not take from them.
//!
//! It changes one thing only. While the session person P's own face is in
//! view through a line (recognised, or a face the eyes follow as P: the
//! eyes' held name, `LAMP_FACE_STICKY`), another face in view does not
//! narrow P's line to the room: the line is read, kept and journalled as
//! P's, with no forget, correction, rule or yes run on it unless their own
//! mouth is seen saying it (`ScopeLease::read_by_one_face`, as
//! `LAMP_PERSON_SCOPE_ONE_FACE_READS` does). The one exception: a face not
//! P's seen speaking the line ([`crate::speaking_faces::someone_elses`]).
//! That line goes to the room, and later lines stay P's.
//!
//! Every other rule is today's: who the line is admitted for (the face
//! cache, its 30 s hold only while no face is in view, the eyes' sticky
//! name, another enrolled person on their takeover looks), the forget
//! rules, and the private read-backs, which never count a session as alone.
//!
//! It starts on a line admitted for P by a real match (not held, not
//! sticky: a rematch is written held) with P's face in view. While a room's
//! conversation with a guest is going on and a face other than P's is
//! still in view, it starts only when P's own face is seen saying the line.
//! It ends at the first of: the conversation closed
//! (`thread::Thread::closed_at`); privacy or the camera changed (the face
//! cache's generation, [`FaceCache::generation`]); a line not admitted for
//! P with a face in view (P gone past today's hold, another face there);
//! [`NO_FACE_QUIET`] (2 min) past the lamp's last answer. No number of its
//! own.
use crate::memory_scope::{FaceCache, NO_FACE_QUIET};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

/// Whose session it is: a known person's (a memory id, never a name), or
/// the room's, a guest's conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Whose {
    Person(Arc<str>),
    Room,
}

struct Open {
    whose: Whose,
    /// The face cache's generation it began in: any other ends it.
    generation: u64,
    /// When it began: a close after it ends it.
    began: Instant,
    /// The lamp's last answer in it, or its start: [`NO_FACE_QUIET`] past
    /// it ends it.
    answered: Instant,
}

/// Why a session ended, for the journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// The conversation closed: a spoken stop, or a paired computer's turn
    /// end.
    Closed,
    /// Privacy, or the camera, changed (the face cache's generation).
    Privacy,
    /// A line not admitted for its person with a face in view: they are
    /// gone past today's hold, or another known person was taken.
    Gone,
    /// [`NO_FACE_QUIET`] past the lamp's last answer.
    Quiet,
}

impl Ended {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "the conversation closed",
            Self::Privacy => "privacy or the camera changed",
            Self::Gone => "a line not theirs with a face in view",
            Self::Quiet => "2 min quiet",
        }
    }
}

/// What the eyes saw of one heard line, as the second route reads it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Line<'a> {
    /// The person the line was admitted for (a memory id); none, the room's.
    pub person: Option<&'a str>,
    /// Admitted by a real match: not held by a gap, not a name the eyes
    /// hold (sticky, a rematch among them).
    pub matched: bool,
    /// The admitted person's own face (or the face the eyes follow as
    /// them) in at least one of the eyes' looks through it.
    pub in_view: bool,
    /// Their own face alone seen saying it
    /// ([`crate::speaking_faces::seen_saying`]).
    pub seen_saying: bool,
    /// A face not theirs seen speaking it
    /// ([`crate::speaking_faces::someone_elses`]).
    pub someone_elses: bool,
    /// A face other than the admitted person's in at least one look.
    pub others_in_view: bool,
    /// Any face at all in at least one look.
    pub any_face: bool,
}

/// What the session makes of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The session person's line, their face in view and no face not theirs
    /// seen speaking it: another face in view does not narrow it.
    Theirs,
    /// Judged by today's rules alone.
    Today,
}

/// A line's verdict, and what the journal is told of it: counts and words,
/// never a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judged {
    pub verdict: Verdict,
    pub told: Vec<&'static str>,
}

/// The one session of a second route's worker, beside its face cache.
pub struct SessionPerson {
    cache: Arc<FaceCache>,
    open: Mutex<Option<Open>>,
}

impl SessionPerson {
    pub fn new(cache: Arc<FaceCache>) -> Self {
        Self {
            cache,
            open: Mutex::new(None),
        }
    }

    /// Judges `line`, heard by `now`, with the conversation last closed at
    /// `closed_at`: ends the session first if it is over, then starts one
    /// if the line may, then gives the line's verdict.
    pub fn line(&self, line: &Line<'_>, now: Instant, closed_at: Option<Instant>) -> Judged {
        let generation = self.cache.generation();
        let mut told = Vec::new();
        let Ok(mut open) = self.open.lock() else {
            return Judged {
                verdict: Verdict::Today,
                told,
            };
        };
        if let Some(ended) = open
            .as_ref()
            .and_then(|was| ended(was, line, generation, now, closed_at))
        {
            told.push(match ended {
                Ended::Closed => "memory session: ended: the conversation closed",
                Ended::Privacy => "memory session: ended: privacy or the camera changed",
                Ended::Gone => "memory session: ended: a line not theirs with a face in view",
                Ended::Quiet => "memory session: ended: 2 min quiet",
            });
            *open = None;
        }
        let begin = |whose| Open {
            whose,
            generation,
            began: now,
            answered: now,
        };
        let theirs = |line: &Line<'_>, told: &mut Vec<&'static str>| {
            if !line.in_view {
                Verdict::Today
            } else if line.someone_elses {
                told.push(
                    "memory session: the room's: a face not theirs seen speaking it; later lines stay theirs",
                );
                Verdict::Today
            } else {
                if line.others_in_view {
                    told.push(
                        "memory session: theirs: another face in view, none not theirs seen speaking it (LAMP_SESSION_PERSON)",
                    );
                }
                Verdict::Theirs
            }
        };
        let verdict = match (open.as_ref().map(|open| open.whose.clone()), line.person) {
            (Some(Whose::Person(of)), Some(person)) if *of == *person => theirs(line, &mut told),
            (Some(Whose::Room), Some(person))
                if line.matched && line.in_view && (line.seen_saying || !line.others_in_view) =>
            {
                told.push(if line.seen_saying {
                    "memory session: began for a known person, seen saying it in the room's conversation"
                } else {
                    "memory session: began for a known person, the guest gone"
                });
                *open = Some(begin(Whose::Person(Arc::from(person))));
                theirs(line, &mut told)
            }
            (None, Some(person)) if line.matched && line.in_view => {
                told.push("memory session: began for a known person");
                *open = Some(begin(Whose::Person(Arc::from(person))));
                theirs(line, &mut told)
            }
            (None, None) if line.any_face && !self.cache.is_cold(now) => {
                told.push("memory session: began for the room (a face not known in view)");
                *open = Some(begin(Whose::Room));
                Verdict::Today
            }
            _ => Verdict::Today,
        };
        Judged { verdict, told }
    }

    /// The lamp answered in the session at `at`: its quiet runs from there.
    pub fn answered(&self, at: Instant) {
        if let Ok(mut open) = self.open.lock()
            && let Some(open) = open.as_mut()
        {
            open.answered = open.answered.max(at);
        }
    }

    /// Whether a look asked now goes under `person` (proposal §8, for #8019's
    /// `look_scope` and `asker_still_alone`): the session is still theirs,
    /// in the same face cache generation, not closed and not quiet. No
    /// face-in-view check: a passer-by does not take it.
    pub fn looks_for(&self, person: &str, now: Instant, closed_at: Option<Instant>) -> bool {
        let generation = self.cache.generation();
        self.open.lock().is_ok_and(|open| {
            open.as_ref().is_some_and(|open| {
                open.whose == Whose::Person(Arc::from(person))
                    && ended(open, &Line::default(), generation, now, closed_at)
                        .is_none_or(|ended| ended == Ended::Gone)
            })
        })
    }
}

/// Whether `open` is over as `line` is heard: the first end that holds.
fn ended(
    open: &Open,
    line: &Line<'_>,
    generation: u64,
    now: Instant,
    closed_at: Option<Instant>,
) -> Option<Ended> {
    if open.generation != generation {
        Some(Ended::Privacy)
    } else if closed_at.is_some_and(|closed| closed > open.began) {
        Some(Ended::Closed)
    } else if now.saturating_duration_since(open.answered) >= NO_FACE_QUIET {
        Some(Ended::Quiet)
    } else if let Whose::Person(of) = &open.whose
        && line.person != Some(&**of)
        && line.any_face
    {
        Some(Ended::Gone)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const P: &str = "person-p";
    const B: &str = "person-b";

    /// P's line by a real match, P's face alone in view, not read speaking.
    fn alone(person: &str) -> Line<'_> {
        Line {
            person: Some(person),
            matched: true,
            in_view: true,
            any_face: true,
            ..Line::default()
        }
    }

    /// A cache past its cold start, as on a lamp running a while.
    fn warm() -> Arc<FaceCache> {
        let cache = Arc::new(FaceCache::default());
        cache.cold_since(Instant::now() - Duration::from_secs(60));
        cache
    }

    fn session() -> (SessionPerson, Arc<FaceCache>, Instant) {
        let cache = warm();
        (SessionPerson::new(cache.clone()), cache, Instant::now())
    }

    #[test]
    fn p_alone_begins_the_session_and_is_theirs() {
        let (session, _, now) = session();
        let judged = session.line(&alone(P), now, None);
        assert_eq!(judged.verdict, Verdict::Theirs);
        assert_eq!(judged.told, ["memory session: began for a known person"]);
        assert!(session.looks_for(P, now, None));
    }

    #[test]
    fn a_silent_passerby_keeps_ps_line_theirs() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        let passer = Line {
            others_in_view: true,
            ..alone(P)
        };
        assert_eq!(session.line(&passer, now, None).verdict, Verdict::Theirs);
    }

    #[test]
    fn a_misread_on_ps_followed_face_stays_theirs() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        // The eyes hold P's name on their followed face (sticky), a
        // passer-by beside it.
        let covered = Line {
            matched: false,
            others_in_view: true,
            ..alone(P)
        };
        assert_eq!(session.line(&covered, now, None).verdict, Verdict::Theirs);
    }

    #[test]
    fn a_held_or_sticky_name_never_begins_a_session() {
        let (session, _, now) = session();
        let sticky = Line {
            matched: false,
            ..alone(P)
        };
        assert_eq!(session.line(&sticky, now, None).verdict, Verdict::Today);
        assert!(!session.looks_for(P, now, None));
    }

    #[test]
    fn another_face_seen_speaking_is_the_rooms_and_the_next_line_ps() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        let spoken = Line {
            someone_elses: true,
            others_in_view: true,
            ..alone(P)
        };
        let judged = session.line(&spoken, now, None);
        assert_eq!(judged.verdict, Verdict::Today);
        assert_eq!(
            judged.told,
            [
                "memory session: the room's: a face not theirs seen speaking it; later lines stay theirs"
            ]
        );
        let next = Line {
            others_in_view: true,
            ..alone(P)
        };
        assert_eq!(session.line(&next, now, None).verdict, Verdict::Theirs);
    }

    #[test]
    fn p_gone_and_an_unknown_face_is_the_rooms() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        let unknown = Line {
            any_face: true,
            ..Line::default()
        };
        let judged = session.line(&unknown, now, None);
        assert_eq!(judged.verdict, Verdict::Today);
        assert_eq!(
            judged.told,
            [
                "memory session: ended: a line not theirs with a face in view",
                "memory session: began for the room (a face not known in view)"
            ]
        );
        assert!(!session.looks_for(P, now, None));
    }

    #[test]
    fn p_gone_and_b_alone_is_bs_session() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        // The eyes took B on their takeover looks: the line is admitted B's.
        let judged = session.line(&alone(B), now, None);
        assert_eq!(judged.verdict, Verdict::Theirs);
        assert!(session.looks_for(B, now, None));
        assert!(!session.looks_for(P, now, None));
    }

    #[test]
    fn a_guests_room_session_holds_p_until_p_is_seen_speaking_or_the_guest_goes() {
        let (session, _, now) = session();
        let guest = Line {
            any_face: true,
            ..Line::default()
        };
        assert_eq!(session.line(&guest, now, None).verdict, Verdict::Today);
        // P comes in, the guest still in view, P not seen speaking.
        let with_guest = Line {
            others_in_view: true,
            ..alone(P)
        };
        assert_eq!(session.line(&with_guest, now, None).verdict, Verdict::Today);
        assert!(!session.looks_for(P, now, None));
        // P seen saying it: P's session.
        let seen = Line {
            seen_saying: true,
            ..with_guest
        };
        assert_eq!(session.line(&seen, now, None).verdict, Verdict::Theirs);
        assert!(session.looks_for(P, now, None));
        // Or, another room session, the guest gone.
        let (session, _, now) = self::session();
        session.line(&guest, now, None);
        let judged = session.line(&alone(P), now, None);
        assert_eq!(judged.verdict, Verdict::Theirs);
        assert_eq!(
            judged.told,
            ["memory session: began for a known person, the guest gone"]
        );
    }

    #[test]
    fn two_minutes_quiet_ends_it() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        session.answered(now + Duration::from_secs(10));
        let before = now + Duration::from_secs(10) + NO_FACE_QUIET - Duration::from_secs(1);
        assert!(session.looks_for(P, before, None));
        let after = now + Duration::from_secs(10) + NO_FACE_QUIET;
        assert!(!session.looks_for(P, after, None));
        let passer = Line {
            others_in_view: true,
            ..alone(P)
        };
        // The line after the quiet: ended, and a new session begins on it.
        let judged = session.line(&passer, after, None);
        assert_eq!(judged.told[0], "memory session: ended: 2 min quiet");
        assert_eq!(judged.told[1], "memory session: began for a known person");
    }

    #[test]
    fn privacy_on_ends_it() {
        let (session, cache, now) = session();
        session.line(&alone(P), now, None);
        cache.invalidate();
        assert!(!session.looks_for(P, now, None));
        // The cache is cold after the change: the next room line begins
        // nothing, and the ended session says why.
        let passer = Line {
            any_face: true,
            ..Line::default()
        };
        let judged = session.line(&passer, Instant::now(), None);
        assert_eq!(judged.verdict, Verdict::Today);
        assert_eq!(
            judged.told,
            ["memory session: ended: privacy or the camera changed"]
        );
    }

    #[test]
    fn a_closed_conversation_ends_it() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        let closed = now + Duration::from_secs(1);
        assert!(!session.looks_for(P, closed, Some(closed)));
        let later = now + Duration::from_secs(2);
        let judged = session.line(&alone(P), later, Some(closed));
        assert_eq!(
            judged.told,
            [
                "memory session: ended: the conversation closed",
                "memory session: began for a known person"
            ]
        );
        // A close before the new session began does not end it.
        assert!(session.looks_for(P, later, Some(closed)));
    }

    #[test]
    fn p_out_of_view_is_todays() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        // Held with no face in view (T1): today's sticky hold decides it.
        let held = Line {
            matched: false,
            in_view: false,
            any_face: false,
            ..alone(P)
        };
        assert_eq!(session.line(&held, now, None).verdict, Verdict::Today);
        assert!(session.looks_for(P, now, None), "the session goes on");
    }

    #[test]
    fn a_look_with_a_passerby_is_under_p() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        // #8019's look asks the session, not the faces in view.
        assert!(session.looks_for(P, now + Duration::from_secs(5), None));
        assert!(!session.looks_for(B, now, None));
    }

    /// The seat swap (the memory owner's choice "B with 5s", 23 Nov): with
    /// `LAMP_SESSION_PERSON` the eyes follow a new id in P's place only
    /// within `who::FOLLOW_GAP` (5 s) of P's last sighting
    /// (`who::tests::with_the_session_person_a_new_id_follows_only_within_the_follow_gap`).
    /// A face they follow as P is P's here; past the gap they don't name it,
    /// and its line is the room's.
    #[test]
    fn a_face_the_eyes_follow_as_p_is_ps_and_one_they_dont_is_the_rooms() {
        let (session, _, now) = session();
        session.line(&alone(P), now, None);
        // The new face, followed as P (held name), silent, P's own face gone.
        let swapped = Line {
            matched: false,
            ..alone(P)
        };
        assert_eq!(session.line(&swapped, now, None).verdict, Verdict::Theirs);
        // Past the eyes' follow gap: not named, the room's, and P's session
        // ends on it.
        let unnamed = Line {
            any_face: true,
            others_in_view: true,
            ..Line::default()
        };
        assert_eq!(session.line(&unnamed, now, None).verdict, Verdict::Today);
        assert!(!session.looks_for(P, now, None));
    }
}
