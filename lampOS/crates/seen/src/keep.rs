//! The sitting kept through a short closing (#6543): when the gate closes
//! for a while that is not one of the closings that wipe at once, the
//! posture window and the doing timers are kept, in memory only and
//! unsaid, for at most [`KEEP_US`], and go on if the gate opens again
//! within it.
//!
//! On a live person the gate closed four times in eight minutes on brief
//! misreads and a face lost for a moment (lamp-06c1, run 3), and each
//! closing forgot the posture window, so with three samples wanted it was
//! almost never said. There is no stranger check any more (the owner's
//! word on #6543), so a misread, a second face or a guest closes nothing.
//!
//! - **Wiped at once** ([`Hard`]): `room/stood_up`, a forget
//!   (`memory/forgot`), the opt-in off, the privacy closed (the slide, or
//!   the voice muting capture), and the lamp asleep. The doing timers keep
//!   their own rule for the privacy, sleep and the opt-in: a sitting does
//!   not end on them (it never did).
//! - **Kept, for at most [`KEEP_US`]**: any other closing, so nobody in
//!   view, the face lost, and a reading gone stale (presence, the privacy
//!   lease, sleep not known).
//! - **Nothing is sent or said while kept.** The gate stays as it is: no
//!   frame is asked of the camera, a request under way is stopped and its
//!   answer dropped, and no `humans/posture`, `humans/doing` or
//!   `humans/doing_seen` is said from what is kept.
//! - **Goes on** only when the gate opens again, and only on the face it
//!   was kept for: the passing
//!   id of the one face in view the last step it was open is kept with it
//!   ([`Away`]), and the gate opening on any other id wipes it
//!   ([`Why::OtherFace`]), so a second remembered person within the 120 s
//!   never takes on the first one's window or timers (the Reviewer-lead's
//!   P2 on #7251). A face back under a new id (as the eyes give one coming
//!   back into view, `follows` included) starts afresh. Past [`KEEP_US`]
//!   closed, both are wiped as before.
//!   The posture window's samples older than its ten minutes still age out.
//! - **The journal** says when each is kept, goes on, or is wiped, and why
//!   (the gate's word): no id, no box.
//! - **The next sitting** ([`Seat`]) counts from `room/activity`'s arrival,
//!   as ever, but never from before the last time the chair emptied: 120 s
//!   closed ([`Why::Expired`]), or a held sitting ended on another face
//!   ([`crate::hold::End::OtherFace`]). `room/activity`'s `sitting_s` is
//!   the room's, forgives a gap under two minutes and knows no face, so on
//!   lamp-06c1's run 8 the next person's `break_s_ago` counted the first
//!   one's time (the first builder's rule on #6543). A stand-up, a forget,
//!   and the gate opening on a new id without the hold (most often the
//!   same person looking back up) begin from the arrival as before.
use crate::gate::{Closed, Inputs};
use crate::hold;
use crate::sitting::Sitting;
use crate::watch::OnTrack;

/// The longest a closing keeps the sitting's windows.
pub const KEEP_US: u64 = 120_000_000;

/// A closing that wipes at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hard {
    StoodUp,
    Forgot,
    OptInOff,
    PrivacyClosed,
    Asleep,
}

impl Hard {
    pub fn as_str(self) -> &'static str {
        match self {
            Hard::StoodUp => "stood_up",
            Hard::Forgot => "forgot",
            Hard::OptInOff => "no_opt_in",
            Hard::PrivacyClosed => "privacy_closed",
            Hard::Asleep => "asleep",
        }
    }

    /// Whether it ends the sitting's doing timers too: the privacy, sleep
    /// and the opt-in never ended a sitting on their own.
    fn ends_sitting(self) -> bool {
        matches!(self, Hard::StoodUp | Hard::Forgot)
    }
}

/// What one step of `serve` knows.
#[derive(Clone, Copy, Debug)]
pub struct Now<'a> {
    pub now_us: u64,
    /// The gate's inputs this step.
    pub inputs: &'a Inputs,
    /// The one face in view ([`crate::watch::Readings::on_track`]).
    pub seen: &'a OnTrack,
    pub stood_up: bool,
    pub forgot: bool,
    /// Posture's gate ([`crate::pose::gate`]).
    pub posture_gate: Result<(), Closed>,
    /// The desk's line (`serve`'s `desk`: presence only).
    pub desk: Result<(), Closed>,
    /// Posture's window is being watched ([`crate::pose::Pose::watching`]).
    pub watching: bool,
}

/// Whether this step's closing wipes at once, and why.
pub fn hard(now: &Now<'_>) -> Option<Hard> {
    let inputs = now.inputs;
    if now.stood_up {
        Some(Hard::StoodUp)
    } else if now.forgot {
        Some(Hard::Forgot)
    } else if !inputs.opted_in {
        Some(Hard::OptInOff)
    } else if inputs.privacy_open == Some(false) {
        Some(Hard::PrivacyClosed)
    } else if inputs.asleep == Some(true) {
        Some(Hard::Asleep)
    } else {
        None
    }
}

/// Why something kept was wiped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    Hard(Hard),
    /// Closed for [`KEEP_US`].
    Expired,
    /// The gate opened again on another face than the one it was kept for
    /// (another passing id, or none known when it was kept).
    OtherFace,
}

/// What to do with one thing kept this step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Open, or nothing to keep: as without this module.
    Open,
    /// The gate closed this step: kept, from now.
    Began,
    /// Still closed, still kept.
    Kept,
    /// Open again within [`KEEP_US`]: goes on.
    Resumed,
    /// Wipe it now.
    Wiped(Why),
}

impl Step {
    /// Held this step: nothing taken, sent or said from it.
    pub fn holds(self) -> bool {
        matches!(self, Step::Began | Step::Kept)
    }

    pub fn wipes(self) -> bool {
        matches!(self, Step::Wiped(_))
    }

    /// The journal's line for `what`, without the `lamp-seen: ` prefix,
    /// on a change only; `closed` is the gate's word.
    pub fn line(self, what: &str, closed: &str) -> Option<String> {
        let keep_s = KEEP_US / 1_000_000;
        match self {
            Step::Open | Step::Kept => None,
            Step::Began => Some(format!(
                "{what} kept unsaid for up to {keep_s} s: the gate closed ({closed})"
            )),
            Step::Resumed => Some(format!(
                "{what} goes on: the gate opened again within {keep_s} s"
            )),
            Step::Wiped(Why::Expired) => Some(format!(
                "{what} wiped: the gate stayed closed {keep_s} s ({closed})"
            )),
            Step::Wiped(Why::OtherFace) => Some(format!(
                "{what} wiped: the gate opened again on another face than the one it was kept for"
            )),
            Step::Wiped(Why::Hard(hard)) => {
                Some(format!("{what} wiped at once ({})", hard.as_str()))
            }
        }
    }
}

/// One thing kept through a closing: since when it is closed, and whose
/// it is: the passing id of the one face in view the last step it was open
/// (in memory only, never said).
#[derive(Clone, Debug, Default)]
pub struct Away {
    since_us: Option<u64>,
    face: Option<String>,
}

impl Away {
    /// One step at `now_us`: whether its gate is `closed`, a closing that
    /// wipes at once, whether there is anything to keep, and the passing id
    /// of the one face in view, if one is.
    pub fn step(
        &mut self,
        now_us: u64,
        closed: bool,
        hard: Option<Hard>,
        holding: bool,
        face: Option<&str>,
    ) -> Step {
        if !holding {
            self.since_us = None;
            self.face = None;
            return Step::Open;
        }
        if let Some(hard) = hard {
            self.since_us = None;
            self.face = None;
            return Step::Wiped(Why::Hard(hard));
        }
        if !closed {
            if self.since_us.take().is_some() {
                // Goes on only for the face it was kept for.
                if face.is_some() && face == self.face.as_deref() {
                    return Step::Resumed;
                }
                self.face = None;
                return Step::Wiped(Why::OtherFace);
            }
            if let Some(face) = face {
                self.face = Some(face.to_owned());
            }
            return Step::Open;
        }
        match self.since_us {
            None => {
                self.since_us = Some(now_us);
                Step::Began
            }
            Some(since) if now_us.saturating_sub(since) >= KEEP_US => {
                self.since_us = None;
                Step::Wiped(Why::Expired)
            }
            Some(_) => Step::Kept,
        }
    }
}

impl Away {
    /// A held sitting ended on another face ([`Keep::step`]): wiped
    /// as [`Why::OtherFace`], and kept from now for `face`, the one in
    /// view.
    fn other_face(&mut self, face: Option<&str>) -> Step {
        self.since_us = None;
        self.face = face.map(str::to_owned);
        Step::Wiped(Why::OtherFace)
    }
}

/// What one step does with the posture window and the doing timers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub hard: Option<Hard>,
    pub posture: Step,
    pub sitting: Step,
}

/// The posture window's keep and the doing timers', as `serve` steps them.
#[derive(Clone, Debug, Default)]
pub struct Keep {
    posture: Away,
    sitting: Away,
}

impl Keep {
    /// One step: `sitting`, a sitting is under way; `other_face`, a held
    /// sitting ended this step on another face
    /// ([`crate::hold::End::OtherFace`]), which wipes both as
    /// [`Why::OtherFace`].
    pub fn step(&mut self, now: &Now<'_>, sitting: bool, other_face: bool) -> Plan {
        let hard = hard(now);
        let face = now.seen.track.as_ref().map(|track| track.id.as_str());
        let posture_closed = now.posture_gate.is_err();
        let posture = if other_face && now.watching {
            self.posture.other_face(face)
        } else {
            self.posture
                .step(now.now_us, posture_closed, hard, now.watching, face)
        };
        // The sitting ends, as before, only on the desk's own line (nobody).
        let sitting_closed = matches!(now.desk, Err(Closed::Nobody));
        let sitting_hard = hard.filter(|hard| hard.ends_sitting());
        let sitting = if other_face && sitting {
            self.sitting.other_face(face)
        } else {
            self.sitting
                .step(now.now_us, sitting_closed, sitting_hard, sitting, face)
        };
        Plan {
            hard,
            posture,
            sitting,
        }
    }
}

/// Whether this step shows the chair emptied, so the next sitting never
/// counts from before it: the sitting's timers wiped after 120 s closed
/// ([`Why::Expired`]), or a held sitting ended on another face
/// (`held_other_face`). The keep's own [`Why::OtherFace`] (the gate open
/// on a new id, most often the same person looking back up), a stand-up
/// and a forget are not.
pub fn chair_emptied(sitting: Step, held_other_face: bool) -> bool {
    held_other_face || sitting == Step::Wiped(Why::Expired)
}

/// How a sitting begun this step is made: `serve`'s switches and
/// readings.
#[derive(Clone, Copy, Debug)]
pub struct Begin {
    /// `room/activity`'s arrival (a gap under two minutes forgiven), or now.
    pub arrived_us: u64,
    /// `LAMP_SEEN_MORE_DOINGS` on.
    pub more: bool,
    /// `LAMP_SEEN_BREAK_IS_STANDING` on.
    pub break_is_standing: bool,
    /// When the last `room/stood_up` came.
    pub stood_up_us: Option<u64>,
}

/// The sitting as `serve` steps it each step: the keep's plan, its wipe,
/// and the next sitting's beginning ([`crate::sitting::begins_us`]).
#[derive(Clone, Debug, Default)]
pub struct Seat {
    keep: Keep,
    sitting: Option<Sitting>,
    /// When the chair last emptied ([`chair_emptied`]).
    emptied_us: Option<u64>,
}

impl Seat {
    /// The sitting under way, if one is.
    pub fn sitting(&self) -> Option<&Sitting> {
        self.sitting.as_ref()
    }

    pub fn sitting_mut(&mut self) -> Option<&mut Sitting> {
        self.sitting.as_mut()
    }

    /// One step after the hold's (`hold_notes`, this step's): the keep's
    /// plan, the sitting wiped when it says so, and one begun when someone
    /// is at the desk (`now.desk`) and none is under way.
    pub fn step(&mut self, now: &Now<'_>, hold_notes: &[hold::Note], begin: &Begin) -> Plan {
        let held_other_face = hold::ended_on_other_face(hold_notes);
        let plan = self.keep.step(now, self.sitting.is_some(), held_other_face);
        if chair_emptied(plan.sitting, held_other_face) {
            self.emptied_us = Some(now.now_us);
        }
        if plan.sitting.wipes() {
            self.sitting = None;
        }
        if now.desk.is_ok() && self.sitting.is_none() {
            let begins = crate::sitting::begins_us(begin.arrived_us, self.emptied_us);
            let begun = Sitting::with_more(begins, begin.more);
            self.sitting = Some(if begin.break_is_standing {
                begun.break_is_standing(now.now_us, begin.stood_up_us)
            } else {
                begun
            });
        }
        plan
    }
}

#[cfg(test)]
mod tests;
