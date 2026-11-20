//! The sitting at the desk, held while a close person is seen
//! (#6543, the owner's rule, c6908160252). Off unless
//! `LAMP_SEEN_HOLDS_SITTING=on` ([`ENVIRONMENT`]); off, `serve` is as
//! without this module.
//!
//! On a live person the face left view while the owner slumped, and brief
//! misreads closed the gate four times in eight minutes (lamp-06c1, run 3),
//! so posture was said once in seven and a half minutes. The rule:
//!
//! - **Begins** only when the gate is open as it always is
//!   ([`crate::gate`]): the opt-in on and said, the privacy open, the lamp
//!   awake, and someone at the desk with a face in view. There is no
//!   stranger check (the owner's word on #6543, "just remove it
//!   completely"): whose face it is is never asked.
//! - **Goes on without the face**: while held, a closing of the gate's
//!   presence rule (nobody in view, the face lost) does not close it, as
//!   long as the newest pose answer showed a close person
//!   ([`close`]): both shoulders (COCO-17's 5 and 6) at or above the pose
//!   model's own confidence threshold ([`CONFIDENCE`]) and at least
//!   [`CLOSE_SHARE`] of the frame's width apart. One answer without a close
//!   person is forgiven; the second in a row ends it ([`FAR_IN_ROW`]).
//!   Posture keeps its one frame every 30 s, even on a hot board
//!   ([`crate::heat::Pace::holding`]), and activity its bursts, each
//!   behind the ring's mark as ever; the opt-in, the said-back line, the
//!   privacy and sleep must still hold (a lease or a state not known closes
//!   that step, and the hold stays).
//! - **Ends** (the gate closed as today, then [`crate::keep`]'s 120 s keep
//!   or wipe applies): two answers in a row with no close person, no pose
//!   close person's answer within [`FIRST_ANSWER_US`] of its beginning
//!   while the face is lost, `room/stood_up`, the
//!   privacy slide or capture muted, sleep, the opt-in off (or its line
//!   withdrawn), a forget, [`MOST_US`] since the gate was last open on
//!   a face in view, or **another face once the chair emptied**
//!   ([`End::OtherFace`]): since the face was lost, at least one pose
//!   answer showed no close person, and then the one face in view comes
//!   back under another passing id than the one the hold was open on (or
//!   with none known, as when it began on two faces). `serve` then wipes
//!   the sitting and posture's window as the keep's
//!   [`crate::keep::Why::OtherFace`] does, and the next sitting counts
//!   from that wipe ([`crate::keep::Seat`]): someone sitting down after
//!   the first one left never takes on their time (lamp-06c1, run 8: the
//!   owner hid, one answer had no close person, another face came; the
//!   first builder's rule on #6543). With a close person in every answer
//!   it is the same body in the chair, and a new id is them looking back
//!   up (the eyes give a face turned away and back a new passing id as a
//!   rule, eyes.md): the hold goes on, on the new id (Heron's P1 on
//!   #7635). With two faces in view no id is matched and nothing ends. It
//!   compares two passing ids from the eyes and never asks whose face it
//!   is.
//! - **The first answer** (lamp-06c1, run 4: the face lost seconds after
//!   every beginning, the first answer ~30 s away): a hold begins with the
//!   newest pose answer when its frame went under posture's spacing
//!   ago ([`crate::pose::Pose::recent_answer`]); else posture's next frame
//!   is already due (one each 30 s, as ever) and goes at the ring's next
//!   mark. Until a close person's answer comes, a lost face keeps the hold
//!   for at most [`FIRST_ANSWER_US`] since it began; no more frames go than
//!   posture's and activity's own rates.
//! - **Nothing kept**: what the pose answer says is read for its two
//!   shoulders and dropped; no frame and no point is kept. The passing id
//!   of the one face in view the last step the gate was open on one is
//!   held in memory only, never said, as [`crate::keep::Away`] holds it.
//! - **The journal** ([`Note`]): began, goes on (with the newest answer's
//!   shoulders as a share of the width), and ended (why, e.g. `another
//!   face in view than the one it was held for`).
//!
//! Without posture (`LAMP_SEEN_POSTURE` off) no pose answer ever comes, so
//! a hold ends when the face is lost, with no wait, and nothing changes.
use crate::gate::{self, Closed, Inputs};
use crate::wire::Joint;

/// The switch: `on` exactly holds the sitting; anything else and
/// `serve` is as without it.
pub const ENVIRONMENT: &str = "LAMP_SEEN_HOLDS_SITTING";

/// A shoulder counts at or above this confidence: the pose model's own
/// `confidence_threshold_2d` as the lamp configures it
/// ([`crate::pose::client::PoseConfig::v1`]).
pub const CONFIDENCE: f32 = 0.3;

/// A close person's shoulders are at least this share of the frame's width
/// apart (the owner's ~15%, to be tuned on a lamp): at the desk, not across
/// the room.
pub const CLOSE_SHARE: f32 = 0.15;

/// Answers in a row with no close person that end a hold.
pub const FAR_IN_ROW: u32 = 2;

/// The longest a hold goes on since the gate was last open on a face.
pub const MOST_US: u64 = 600_000_000;

/// How long since a hold began a lost face keeps it while no close
/// person's answer has come: the first answer may be one 30 s interval
/// and a request's time away.
pub const FIRST_ANSWER_US: u64 = 45_000_000;

/// The share of the frame's `width` (pixels, as sent) between the two
/// shoulders, when both are at or above [`CONFIDENCE`]; else none.
pub fn share(shoulders: Option<&[Joint; 2]>, width: u32) -> Option<f32> {
    let [left, right] = shoulders?;
    if width == 0 || left.conf < CONFIDENCE || right.conf < CONFIDENCE {
        return None;
    }
    let apart = (left.x - right.x).hypot(left.y - right.y);
    Some(apart / width as f32).filter(|share| share.is_finite())
}

/// Whether an answer's [`share`] shows a close person.
pub fn close(share: Option<f32>) -> bool {
    share.is_some_and(|share| share >= CLOSE_SHARE)
}

/// Why a hold ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// [`FAR_IN_ROW`] answers in a row with no close person.
    NoClosePerson,
    /// The face lost, and no close person's answer within
    /// [`FIRST_ANSWER_US`] of the hold's beginning.
    NoAnswerYet,
    StoodUp,
    Forgot,
    /// The gate's opt-in, said-back, privacy or sleep rule (its word).
    Gate(Closed),
    /// [`MOST_US`] since the gate was last open on a face.
    TooLong,
    /// Another face than the one it was held for came into view after
    /// the face was lost and an answer showed no close person: the
    /// sitting and posture's window are wiped
    /// ([`crate::keep::Why::OtherFace`]).
    OtherFace,
}

impl End {
    pub fn as_str(self) -> &'static str {
        match self {
            End::NoClosePerson => "no close person in 2 answers in a row",
            End::NoAnswerYet => "the face lost and no close posture answer within 45 s",
            End::StoodUp => "stood_up",
            End::Forgot => "forgot",
            End::Gate(closed) => closed.as_str(),
            End::TooLong => "10 min since the gate was open on a face",
            End::OtherFace => "another face in view than the one it was held for",
        }
    }
}

/// One line for the journal about the hold ([`Note::line`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Note {
    Began,
    /// Going on without the face: the newest answer's shoulders, and how
    /// many answers in a row had no close person.
    GoesOn {
        share: Option<f32>,
        far_in_row: u32,
    },
    Ended(End),
}

fn percent(share: f32) -> u32 {
    (share * 100.0).round().clamp(0.0, 1000.0) as u32
}

impl Note {
    /// The journal's line, without the `lamp-seen: ` prefix.
    pub fn line(&self) -> String {
        match *self {
            Note::Began => "held sitting began: the gate open on a face".to_owned(),
            Note::GoesOn {
                share: Some(share),
                far_in_row: 0,
            } => format!(
                "held sitting goes on (close person, shoulders {}% of width)",
                percent(share)
            ),
            Note::GoesOn { share, far_in_row } => format!(
                "held sitting goes on (no close person in {far_in_row} answer of {FAR_IN_ROW}{})",
                share
                    .map(|share| format!(", shoulders {}% of width", percent(share)))
                    .unwrap_or_default()
            ),
            Note::Ended(end) => format!("held sitting ended ({})", end.as_str()),
        }
    }
}

/// What one step of `serve` knows.
#[derive(Clone, Copy, Debug)]
pub struct Now<'a> {
    pub now_us: u64,
    /// The gate's inputs this step, as read.
    pub inputs: &'a Inputs,
    pub stood_up: bool,
    pub forgot: bool,
    /// The newest pose answer's [`share`], when its frame went under
    /// posture's spacing ago (30 s, or 60 s on a hot board before a hold) ([`crate::pose::Pose::recent_answer`]):
    /// a hold beginning this step begins with it.
    pub recent_answer: Option<Option<f32>>,
    /// Posture's frames go (`LAMP_SEEN_POSTURE`): without it no answer
    /// comes, and a lost face ends a hold with no [`FIRST_ANSWER_US`] wait.
    pub posture: bool,
    /// The passing id of the one face in view
    /// ([`crate::watch::Readings::on_track`]); `None` for no face, two, or
    /// not known.
    pub face: Option<&'a str>,
}

struct Held {
    /// When it began.
    began_us: u64,
    /// When the gate was last open on a face.
    recognised_us: u64,
    /// A close person's answer came since it began.
    close_seen: bool,
    far_in_row: u32,
    /// The newest answer's [`share`].
    share: Option<f32>,
    /// Holding the gate open without the face at the last step.
    opening: bool,
    /// The passing id of the one face in view the last step the gate was
    /// open on one (in memory only, never said), as [`crate::keep::Away`]
    /// keeps it.
    face: Option<String>,
    /// The gate's presence rule has not held since that step.
    away: bool,
    /// While away, an answer showed no close person: the chair may have
    /// emptied.
    far_while_away: bool,
}

/// The hold's state.
#[derive(Default)]
pub struct Hold {
    held: Option<Held>,
    notes: Vec<Note>,
}

/// The inputs as if the presence rule held: someone at the desk, and a
/// face in view.
pub fn clear(inputs: &Inputs) -> Inputs {
    Inputs {
        someone: Some(true),
        faces: Some(1),
        ..*inputs
    }
}

impl Hold {
    /// Whether a hold is under way.
    pub fn held(&self) -> bool {
        self.held.is_some()
    }

    /// The journal's notes since the last call, oldest first.
    pub fn notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// One pose answer's [`share`] (none: no confident shoulders, or no
    /// body). Taken only while held.
    pub fn answer(&mut self, share: Option<f32>) {
        let Some(held) = self.held.as_mut() else {
            return;
        };
        held.close_seen |= close(share);
        held.share = share;
        held.far_while_away |= held.away && !close(share);
        held.far_in_row = if close(share) {
            0
        } else {
            held.far_in_row.saturating_add(1)
        };
        if held.opening && held.far_in_row < FAR_IN_ROW {
            self.notes.push(Note::GoesOn {
                share,
                far_in_row: held.far_in_row,
            });
        }
    }

    /// Why the hold ends this step, if it does.
    fn end(&self, now: &Now<'_>, open: bool) -> Option<End> {
        let held = self.held.as_ref()?;
        let inputs = now.inputs;
        if now.forgot {
            Some(End::Forgot)
        } else if now.stood_up {
            Some(End::StoodUp)
        } else if !inputs.opted_in {
            Some(End::Gate(Closed::NoOptIn))
        } else if !inputs.said_on {
            Some(End::Gate(Closed::NotSaidYet))
        } else if inputs.privacy_open == Some(false) {
            Some(End::Gate(Closed::PrivacyClosed))
        } else if inputs.asleep == Some(true) {
            Some(End::Gate(Closed::Asleep))
        } else if held.away
            && held.far_while_away
            && now
                .face
                .is_some_and(|face| held.face.as_deref() != Some(face))
        {
            Some(End::OtherFace)
        } else if open {
            None
        } else if now.now_us.saturating_sub(held.recognised_us) >= MOST_US {
            Some(End::TooLong)
        } else if held.far_in_row >= FAR_IN_ROW {
            Some(End::NoClosePerson)
        } else if !held.close_seen
            && (!now.posture || now.now_us.saturating_sub(held.began_us) >= FIRST_ANSWER_US)
        {
            Some(End::NoAnswerYet)
        } else {
            None
        }
    }

    /// One step: whether the hold opens the gate this step, though its
    /// presence rule does not hold. The gate open as it always
    /// is begins a hold or renews it, and opens nothing more.
    pub fn step(&mut self, now: &Now<'_>) -> bool {
        let open = gate::may_request(now.inputs).is_ok();
        if let Some(end) = self.end(now, open) {
            self.held = None;
            self.notes.push(Note::Ended(end));
        }
        if open {
            let face = now.face.map(str::to_owned);
            match self.held.as_mut() {
                Some(held) => {
                    held.recognised_us = now.now_us;
                    held.opening = false;
                    // Back on one face (the same id, or a new one with a
                    // close person in every answer: them looking back
                    // up); with two in view, nothing to match yet.
                    if face.is_some() {
                        held.face = face;
                        held.away = false;
                        held.far_while_away = false;
                    }
                }
                None => {
                    self.held = Some(Held {
                        began_us: now.now_us,
                        recognised_us: now.now_us,
                        close_seen: false,
                        far_in_row: 0,
                        share: None,
                        opening: false,
                        face,
                        away: false,
                        far_while_away: false,
                    });
                    self.notes.push(Note::Began);
                    // The newest answer, when its frame went under
                    // posture's spacing ago: no wait for the next one.
                    if let Some(share) = now.recent_answer {
                        self.answer(share);
                    }
                }
            }
            return false;
        }
        let Some(held) = self.held.as_mut() else {
            return false;
        };
        held.away = true;
        // The privacy lease or the lamp's state not known this step:
        // closed as today, still held.
        if gate::may_request(&clear(now.inputs)).is_err() {
            held.opening = false;
            return false;
        }
        if !held.opening {
            held.opening = true;
            self.notes.push(Note::GoesOn {
                share: held.share,
                far_in_row: held.far_in_row,
            });
        }
        true
    }
}

/// Whether these notes end a hold on another face
/// ([`End::OtherFace`]): `serve` then wipes the sitting and posture's
/// window as the keep does, and the next sitting counts from then
/// ([`crate::keep::Seat::step`]).
pub fn ended_on_other_face(notes: &[Note]) -> bool {
    notes.contains(&Note::Ended(End::OtherFace))
}

/// `serve`'s wiring of one step (`main.rs`): the pose answers since the
/// last step taken, then the gate's inputs as the gate and the keep read
/// them. With the switch off (`hold` none), or no hold opening the gate,
/// they are as they came; held open, the presence rule holds.
pub fn wire(hold: Option<&mut Hold>, answers: &[Option<f32>], now: &Now<'_>) -> Inputs {
    let Some(hold) = hold else {
        return *now.inputs;
    };
    for share in answers {
        hold.answer(*share);
    }
    if hold.step(now) {
        clear(now.inputs)
    } else {
        *now.inputs
    }
}

#[cfg(test)]
mod tests;
