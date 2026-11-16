//! The owner's recognised sitting, held while a close person is seen
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
//!   awake, and the owner's face, the one face in view, recognised.
//! - **Goes on without the face**: while held, a closing of the gate's
//!   presence or stranger rules (nobody in view, the face lost or misread,
//!   a new face not recognised, a stranger reading, a second face) does not
//!   close it, as long as the newest pose answer showed a close person
//!   ([`close`]): both shoulders (COCO-17's 5 and 6) at or above the pose
//!   model's own confidence threshold ([`CONFIDENCE`]) and at least
//!   [`CLOSE_SHARE`] of the frame's width apart. One answer without a close
//!   person is forgiven; the second in a row ends it ([`FAR_IN_ROW`]).
//!   Posture keeps its one frame every 30 s and activity its bursts, each
//!   behind the ring's mark as ever; the opt-in, the said-back line, the
//!   privacy and sleep must still hold (a lease or a state not known closes
//!   that step, and the hold stays).
//! - **Ends** (the gate closed as today, then [`crate::keep`]'s 120 s keep
//!   or wipe applies): two answers in a row with no close person, no pose
//!   close person's answer within [`FIRST_ANSWER_US`] of its beginning
//!   while the face is lost, `room/stood_up`, the
//!   privacy slide or capture muted, sleep, the opt-in off (or its line
//!   withdrawn), a forget, [`MOST_US`] since the owner's face was last
//!   recognised (the gate last open on it), or another person alone in the
//!   seat ([`STRANGER_ALONE_US`]).
//! - **The first answer** (lamp-06c1, run 4: the face lost seconds after
//!   every beginning, the first answer ~30 s away): a hold begins with the
//!   newest pose answer when its frame went under [`crate::pose::EVERY_US`]
//!   ago ([`crate::pose::Pose::recent_answer`]); else posture's next frame
//!   is already due (one each 30 s, as ever) and goes at the ring's next
//!   mark. Until a close person's answer comes, a lost face keeps the hold
//!   for at most [`FIRST_ANSWER_US`] since it began; no more frames go than
//!   posture's and activity's own rates.
//! - **Another person alone in the seat** (the first builder's change, the
//!   owner's yes): one face alone in view, close ([`close`] on the newest
//!   pose answer, the hold's own nearness), on a passing id not the
//!   owner's (the face the gate was last open on), and read as not the
//!   owner on every reading for [`STRANGER_ALONE_US`] ends the hold at
//!   once. A stranger reading on the owner's own id (the owner slumped or
//!   turned, lamp-06c1, run 4) is a misread: it counts nothing, and while
//!   held the pause and the keep never see it, so it wipes nothing. The
//!   eyes say only remembered or not (`humans/familiar` is a level, no
//!   name), so "not the owner" is a stranger reading on that face
//!   ([`OnTrack::stranger`]); someone else remembered reads as the owner.
//!   An owner reading, the face gone or a new id, a second face, or no
//!   close person starts the count again; a second face beside the seated
//!   person never ends the hold.
//! - **Nothing kept**: what the pose answer says is read for its two
//!   shoulders and dropped; no frame, no point and no id is kept.
//! - **The journal** ([`Note`]): began, goes on (with the newest answer's
//!   shoulders as a share of the width), and ended (why).
//!
//! Without posture (`LAMP_SEEN_POSTURE` off) no pose answer ever comes, so
//! a hold ends when the face is lost, with no wait, and nothing changes.
use crate::gate::{self, Closed, Inputs};
use crate::watch::OnTrack;
use crate::wire::Joint;

/// The switch: `on` exactly holds the owner's sitting; anything else and
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

/// The longest a hold goes on since the owner's face was last recognised.
pub const MOST_US: u64 = 600_000_000;

/// How long one close face alone in view reads as a stranger, on every
/// reading, before the hold ends: long enough that a 1 to 3 s misread of
/// the owner does not close the gate again (the first builder's 10 s).
pub const STRANGER_ALONE_US: u64 = 10_000_000;

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
    /// [`MOST_US`] since the owner's face was last recognised.
    TooLong,
    /// One close face alone in view read as a stranger for
    /// [`STRANGER_ALONE_US`].
    StrangerAlone,
}

impl End {
    pub fn as_str(self) -> &'static str {
        match self {
            End::NoClosePerson => "no close person in 2 answers in a row",
            End::NoAnswerYet => "the face lost and no close posture answer within 45 s",
            End::StoodUp => "stood_up",
            End::Forgot => "forgot",
            End::Gate(closed) => closed.as_str(),
            End::TooLong => "10 min since the owner's face was recognised",
            End::StrangerAlone => "another person alone in the seat for 10 s",
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
            Note::Began => "held sitting began: the gate open on the owner's face".to_owned(),
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
    /// The one face in view and what the newest `humans/familiar` says of
    /// it ([`crate::watch::Readings::on_track`]).
    pub seen: &'a OnTrack,
    /// The newest pose answer's [`share`], when its frame went under
    /// [`crate::pose::EVERY_US`] ago ([`crate::pose::Pose::recent_answer`]):
    /// a hold beginning this step begins with it.
    pub recent_answer: Option<Option<f32>>,
    /// Posture's frames go (`LAMP_SEEN_POSTURE`): without it no answer
    /// comes, and a lost face ends a hold with no [`FIRST_ANSWER_US`] wait.
    pub posture: bool,
}

struct Held {
    /// When it began.
    began_us: u64,
    /// When the gate was last open on the owner's face.
    recognised_us: u64,
    /// The passing id of the face the gate was last open on: the owner's.
    owner_id: Option<String>,
    /// A close person's answer came since it began.
    close_seen: bool,
    far_in_row: u32,
    /// The newest answer's [`share`].
    share: Option<f32>,
    /// Holding the gate open without the face at the last step.
    opening: bool,
    /// One close face alone in view, read as a stranger at every step
    /// since: its passing id, and since when.
    stranger_alone: Option<(String, u64)>,
}

/// The one face in view's passing id, when it is close, not the owner's
/// own id, and read as a stranger (not the owner) this step.
fn lone_stranger<'a>(now: &Now<'a>, held: &Held) -> Option<&'a str> {
    let seen = now.seen;
    let track = seen.track.as_ref()?;
    (now.inputs.faces == Some(1)
        && seen.stranger
        && now.inputs.familiar != Some(true)
        && held.owner_id.as_deref() != Some(track.id.as_str())
        && close(held.share))
    .then_some(track.id.as_str())
}

/// The hold's state.
#[derive(Default)]
pub struct Hold {
    held: Option<Held>,
    notes: Vec<Note>,
}

/// The inputs as if the presence and stranger rules held: one face in
/// view, someone at the desk, and that face remembered.
pub fn clear(inputs: &Inputs) -> Inputs {
    Inputs {
        someone: Some(true),
        faces: Some(1),
        familiar: Some(true),
        ..*inputs
    }
}

impl Hold {
    /// Whether a hold is under way.
    pub fn held(&self) -> bool {
        self.held.is_some()
    }

    /// Whether `seen` is a stranger reading on the owner's own passing id
    /// while held: a misread of the owner, not another person.
    fn owner_misread(&self, seen: &OnTrack) -> bool {
        let Some(held) = self.held.as_ref() else {
            return false;
        };
        seen.stranger
            && seen
                .track
                .as_ref()
                .is_some_and(|track| held.owner_id.as_deref() == Some(track.id.as_str()))
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
        } else if held
            .stranger_alone
            .as_ref()
            .is_some_and(|(_, since)| now.now_us.saturating_sub(*since) >= STRANGER_ALONE_US)
        {
            Some(End::StrangerAlone)
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
    /// presence or stranger rules do not hold. The gate open as it always
    /// is begins a hold or renews it, and opens nothing more.
    pub fn step(&mut self, now: &Now<'_>) -> bool {
        let open = gate::may_request(now.inputs).is_ok();
        if let Some(held) = self.held.as_mut() {
            held.stranger_alone = match (lone_stranger(now, held), held.stranger_alone.take()) {
                (Some(id), Some((was, since))) if was == id => Some((was, since)),
                (Some(id), _) => Some((id.to_owned(), now.now_us)),
                (None, _) => None,
            };
        }
        if let Some(end) = self.end(now, open) {
            self.held = None;
            self.notes.push(Note::Ended(end));
        }
        if open {
            let owner_id = now.seen.track.as_ref().map(|track| track.id.clone());
            match self.held.as_mut() {
                Some(held) => {
                    held.recognised_us = now.now_us;
                    held.opening = false;
                    if owner_id.is_some() {
                        held.owner_id = owner_id;
                    }
                }
                None => {
                    self.held = Some(Held {
                        began_us: now.now_us,
                        recognised_us: now.now_us,
                        owner_id,
                        close_seen: false,
                        far_in_row: 0,
                        share: None,
                        opening: false,
                        stranger_alone: None,
                    });
                    self.notes.push(Note::Began);
                    // The newest answer, when its frame went under 30 s
                    // ago: no wait for the next one.
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

/// `serve`'s wiring of one step (`main.rs`): the pose answers since the
/// last step taken, then the gate's inputs and the one face in view as the
/// gate, the pause and the keep read them (the face from `now.seen`).
/// With the switch off (`hold` none), or no hold opening the gate, they are
/// as they came; held open, the presence and stranger rules hold and no
/// stranger reading is seen. Held, a stranger reading on the owner's own
/// id is not seen either, so it pauses or wipes nothing.
pub fn wire(hold: Option<&mut Hold>, answers: &[Option<f32>], now: &Now<'_>) -> (Inputs, OnTrack) {
    let seen = now.seen.clone();
    let Some(hold) = hold else {
        return (*now.inputs, seen);
    };
    for share in answers {
        hold.answer(*share);
    }
    let opens = hold.step(now);
    let seen = OnTrack {
        stranger: seen.stranger && !opens && !hold.owner_misread(&seen),
        ..seen
    };
    if opens {
        (clear(now.inputs), seen)
    } else {
        (*now.inputs, seen)
    }
}

#[cfg(test)]
mod tests;
