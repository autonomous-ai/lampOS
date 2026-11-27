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
//!   id and box of the one face in view the last step it was open are kept
//!   with it ([`Away`]). The same id goes on ([`Step::Resumed`]). A new id
//!   goes on too ([`Step::Carried`]) only when all of these hold: exactly
//!   one face is in view, the gate was closed at most [`CARRY_GAP_US`]
//!   (4 s, the eyes' `KNOWN_HOLD`), and the two boxes' centres are apart
//!   at most the new box's width (the eyes' own follow rule). The eyes
//!   keep a passing id only 1.5 s and the gate reads nobody only after a
//!   2 s stale list, so the same face always comes back under a new id
//!   (lamp-06c1, 23 Nov: a slump wiped posture's window 2 s after "gate
//!   nobody"). Any other id wipes it ([`Why::OtherFace`]), so a second
//!   remembered person within the 120 s never takes on the first one's
//!   window or timers (the Reviewer-lead's P2 on #7251); only someone in
//!   the same place within 4 s, alone in view, is carried, which the first
//!   builder took for a timer on #6543. Past [`KEEP_US`] closed, both are
//!   wiped as before.
//!   The posture window's samples older than its `KEPT_US` (2 min) still age out.
//! - **Timers only.** The keep holds posture's window and the sitting's
//!   timers (break, drink, eating, desk time and the counts), never a
//!   name, an identity, a memory or a record about a person. Only an
//!   [`Away::timer`] carries onto a new id; an [`Away::default`] (the rule
//!   for anything kept about a person) goes on only on the same id (the
//!   first builder's condition on #6543).
//! - **The journal** says when each is kept, goes on, or is wiped, and why
//!   (the gate's word). On a carry it adds the old and new passing ids, the
//!   gap in ms and the centres' distance against the width; on a wipe for
//!   another face, what failed (the gap, the place, or the faces in view).
//!   Passing ids are the eyes', gone with the face, never a name.
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
use crate::watch::{OnTrack, Track};

/// The longest a closing keeps the sitting's windows.
pub const KEEP_US: u64 = 120_000_000;

/// The longest closing a kept timer carries onto a new passing id in the
/// same place ([`Away::timer`]): the eyes' `KNOWN_HOLD`
/// (`crates/eyes/src/who.rs`, 4 s), the longest they hold a known person
/// through checks that find nobody. lamp-eyes is not a dependency of
/// lamp-seen, so it is named here. lamp-06c1's gaps were 2 s and 1 s.
pub const CARRY_GAP_US: u64 = 4_000_000;

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
    /// (another passing id not carried, or none known when it was kept).
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
    /// Open again within [`KEEP_US`] on the same passing id: goes on.
    Resumed,
    /// Open again within [`CARRY_GAP_US`] on a new passing id, the one face
    /// in view, in the same place: a timer goes on ([`Away::timer`]).
    Carried,
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
        self.line_with(what, closed, None)
    }

    /// [`Step::line`], with the keep's `detail` of this step
    /// ([`Plan::posture_detail`]) after it in brackets.
    pub fn line_with(self, what: &str, closed: &str, detail: Option<&str>) -> Option<String> {
        let line = self.bare_line(what, closed)?;
        Some(match detail {
            Some(detail) => format!("{line} ({detail})"),
            None => line,
        })
    }

    fn bare_line(self, what: &str, closed: &str) -> Option<String> {
        let keep_s = KEEP_US / 1_000_000;
        match self {
            Step::Open | Step::Kept => None,
            Step::Began => Some(format!(
                "{what} kept unsaid for up to {keep_s} s: the gate closed ({closed})"
            )),
            Step::Resumed => Some(format!(
                "{what} goes on: the gate opened again within {keep_s} s"
            )),
            Step::Carried => Some(format!(
                "{what} goes on: the gate opened again within {} s on a new passing id in the same place",
                CARRY_GAP_US / 1_000_000
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
/// it is: the passing id and box of the one face in view the last step it
/// was open (in memory only, never said).
///
/// [`Away::default`] goes on only on the same passing id: the rule for
/// anything kept about a person. [`Away::timer`] also carries onto a new
/// id in the same place after a short closing ([`Step::Carried`]): for
/// timers only (the first builder's condition on #6543).
#[derive(Clone, Debug, Default)]
pub struct Away {
    since_us: Option<u64>,
    face: Option<Track>,
    /// A timer: carried onto a new id in the same place.
    carries: bool,
    /// This step's journal detail: a carry, or why another face wiped it.
    detail: Option<String>,
}

impl Away {
    /// A timer's keep: carried onto a new passing id after a short closing
    /// in the same place, with one face in view.
    pub fn timer() -> Self {
        Self {
            carries: true,
            ..Self::default()
        }
    }

    /// This step's journal detail, if any: the old and new passing ids,
    /// the gap and the centres' distance on a carry, or why another face
    /// was not carried.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// One step at `now_us`: whether its gate is `closed`, a closing that
    /// wipes at once, whether there is anything to keep, the one face in
    /// view (its passing id and box), if one is, and how many faces are in
    /// view (the gate's `faces`).
    pub fn step(
        &mut self,
        now_us: u64,
        closed: bool,
        hard: Option<Hard>,
        holding: bool,
        face: Option<&Track>,
        faces: Option<usize>,
    ) -> Step {
        self.detail = None;
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
            if let Some(since) = self.since_us.take() {
                let kept = self.face.take();
                // Goes on for the face it was kept for.
                if let (Some(new), Some(old)) = (face, kept.as_ref())
                    && new.id == old.id
                {
                    self.face = kept;
                    return Step::Resumed;
                }
                if !self.carries {
                    return Step::Wiped(Why::OtherFace);
                }
                let gap_us = now_us.saturating_sub(since);
                return match carry(kept.as_ref(), face, faces, gap_us) {
                    Ok(detail) => {
                        self.face = face.cloned();
                        self.detail = Some(detail);
                        Step::Carried
                    }
                    Err(why) => {
                        self.detail = Some(why);
                        Step::Wiped(Why::OtherFace)
                    }
                };
            }
            if let Some(face) = face {
                self.face = Some(face.clone());
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

/// The centre of a box (xmin, ymin, xmax, ymax).
fn centre(edges: &[f32; 4]) -> (f32, f32) {
    ((edges[0] + edges[2]) / 2.0, (edges[1] + edges[3]) / 2.0)
}

/// Whether a timer kept for `kept` carries onto `new` after `gap_us`
/// closed: one face in view (`faces`), the gap at most [`CARRY_GAP_US`],
/// and the centres apart at most the new box's width (the eyes' follow
/// rule, `crates/eyes/src/bus_out.rs`, #4885). `Ok` is the journal's
/// detail of the carry; `Err` says what failed.
fn carry(
    kept: Option<&Track>,
    new: Option<&Track>,
    faces: Option<usize>,
    gap_us: u64,
) -> Result<String, String> {
    let Some(old) = kept else {
        return Err("no face known when it was kept".to_owned());
    };
    let new = match (faces, new) {
        (Some(1), Some(new)) => new,
        (Some(1), None) => return Err("the face in view has no passing id".to_owned()),
        (Some(count), _) => return Err(format!("{count} faces in view")),
        (None, _) => return Err("the faces in view not known".to_owned()),
    };
    let (old_id, new_id) = (&old.id, &new.id);
    let gap_ms = gap_us / 1_000;
    if gap_us > CARRY_GAP_US {
        return Err(format!(
            "{old_id} to {new_id} after {gap_ms} ms, over {} ms",
            CARRY_GAP_US / 1_000
        ));
    }
    let (old_x, old_y) = centre(&old.edges);
    let (new_x, new_y) = centre(&new.edges);
    let apart = (new_x - old_x).hypot(new_y - old_y);
    let width = new.edges[2] - new.edges[0];
    if apart > width {
        return Err(format!(
            "{old_id} to {new_id} after {gap_ms} ms: centres {apart:.3} apart, over the new box's width {width:.3}"
        ));
    }
    Ok(format!(
        "{old_id} to {new_id} after {gap_ms} ms: centres {apart:.3} apart, within the new box's width {width:.3}"
    ))
}

impl Away {
    /// A held sitting ended on another face ([`Keep::step`]): wiped
    /// as [`Why::OtherFace`], and kept from now for `face`, the one in
    /// view.
    fn other_face(&mut self, face: Option<&Track>) -> Step {
        self.since_us = None;
        self.face = face.cloned();
        self.detail = None;
        Step::Wiped(Why::OtherFace)
    }
}

/// What one step does with the posture window and the doing timers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub hard: Option<Hard>,
    pub posture: Step,
    pub sitting: Step,
    /// The journal's detail of `posture`'s step ([`Away::detail`]).
    pub posture_detail: Option<String>,
    /// The journal's detail of `sitting`'s step ([`Away::detail`]).
    pub sitting_detail: Option<String>,
}

impl Plan {
    /// Posture's journal line ([`Step::line_with`]).
    pub fn posture_line(&self, closed: &str) -> Option<String> {
        self.posture
            .line_with("posture's window", closed, self.posture_detail.as_deref())
    }

    /// The sitting's journal line ([`Step::line_with`]).
    pub fn sitting_line(&self, closed: &str) -> Option<String> {
        self.sitting.line_with(
            "the sitting's timers",
            closed,
            self.sitting_detail.as_deref(),
        )
    }
}

/// The posture window's keep and the doing timers', as `serve` steps them:
/// both timers ([`Away::timer`]).
#[derive(Clone, Debug)]
pub struct Keep {
    posture: Away,
    sitting: Away,
}

impl Default for Keep {
    fn default() -> Self {
        Self {
            posture: Away::timer(),
            sitting: Away::timer(),
        }
    }
}

impl Keep {
    /// One step: `sitting`, a sitting is under way; `other_face`, a held
    /// sitting ended this step on another face
    /// ([`crate::hold::End::OtherFace`]), which wipes both as
    /// [`Why::OtherFace`].
    pub fn step(&mut self, now: &Now<'_>, sitting: bool, other_face: bool) -> Plan {
        let hard = hard(now);
        let face = now.seen.track.as_ref();
        let faces = now.inputs.faces;
        let posture_closed = now.posture_gate.is_err();
        let posture = if other_face && now.watching {
            self.posture.other_face(face)
        } else {
            self.posture
                .step(now.now_us, posture_closed, hard, now.watching, face, faces)
        };
        // The sitting ends, as before, only on the desk's own line (nobody).
        let sitting_closed = matches!(now.desk, Err(Closed::Nobody));
        let sitting_hard = hard.filter(|hard| hard.ends_sitting());
        let sitting = if other_face && sitting {
            self.sitting.other_face(face)
        } else {
            self.sitting.step(
                now.now_us,
                sitting_closed,
                sitting_hard,
                sitting,
                face,
                faces,
            )
        };
        Plan {
            hard,
            posture,
            sitting,
            posture_detail: self.posture.detail().map(str::to_owned),
            sitting_detail: self.sitting.detail().map(str::to_owned),
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
