//! The pause on a misread (#6543): a stranger reading on the same tracked
//! face pauses the gate instead of ending the sitting.
//!
//! On a live person `humans/familiar` flips from known to stranger and back
//! whenever the owner looks down, the recogniser's threshold on a tilted
//! face. Each flip closed the gate and forgot the posture window and the
//! doing timers, so a slouch was never scored and a drink seen was lost.
//! The Reviewer-lead's conditions, in this module and its callers:
//!
//! - **What pauses:** the gate was open on one face and closes only because
//!   a fresh `humans/familiar` says nobody remembered, said after that face
//!   came into view alone, while that same face (by its passing id) is the
//!   only one in view, its box where it was, and every other rule of the
//!   gate holds ([`crate::gate`]).
//! - **A pause sends nothing:** the gate stays closed while paused, so no
//!   frame is taken or sent and no mark is asked
//!   ([`crate::frames::Taker::step`]), and nothing kept is published
//!   (no `humans/posture`, `humans/doing` or `humans/doing_seen`).
//! - **Only a fresh known reading on the same id ends it:** the gate opens
//!   again only on a `humans/familiar` level said after the stranger
//!   reading, for the same passing id. Time alone never opens it.
//! - **On resume,** what was kept is used only when the known reading comes
//!   on the same id with its box near where it paused (the centre within
//!   the face's own width); otherwise it is wiped and the sitting starts
//!   fresh.
//! - **What a pause keeps:** the posture window and the doing timers, in
//!   memory only. They are wiped after [`MOST_US`] paused, or at once on a
//!   box jump between samples larger than the face's own width, a new id, a
//!   second face, no face, the privacy, sleep, the opt-in withdrawn, or
//!   anything else the gate does not know (a reading gone stale).
//! - **The familiar yes is never carried to a new id** (#6602's P2): a new
//!   id wipes, and the gate's own rule ([`crate::watch`]) wants a level
//!   said after the new face came.
//! - **The journal** gets counts only ([`Counts`]): no id, no box.
//!
//! The eyes' tracker follows a face by its box alone (`lamp-eyes`'
//! `bus_out::Ids`, IoU at least 0.2 with a box seen within 1.5 s), so an id
//! can pass from one person to another who takes the same place within
//! 1.5 s. A pause never sends a frame and its end wants the recogniser's
//! own known reading, so a stranger under a carried id never gets one sent;
//! what is left is the owner's kept window, unpublished, for at most 5 s.
use crate::gate::{self, Inputs};
use crate::watch::{OnTrack, Track};

/// The longest a pause keeps anything: past it, the window and timers are
/// wiped (the Reviewer-lead's 5 s).
pub const MOST_US: u64 = 5_000_000;

/// Saturating counts, for the journal and `diagnostics`: no ids.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Pauses begun.
    pub paused: u64,
    /// Stranger readings on the tracked face: the one that began each
    /// pause and each new one while paused.
    pub misread_on_track: u64,
}

/// What one step says to do with what the sitting keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Forget the posture window and the doing timers now.
    pub wipe: bool,
    /// Paused: keep them, in memory only, and publish nothing from them.
    /// The gate itself stays closed.
    pub paused: bool,
}

const GO_ON: Verdict = Verdict {
    wipe: false,
    paused: false,
};
const PAUSED: Verdict = Verdict {
    wipe: false,
    paused: true,
};
const WIPE: Verdict = Verdict {
    wipe: true,
    paused: false,
};

struct Paused {
    id: String,
    /// The box when the pause began.
    at: [f32; 4],
    /// The box at the last step.
    last: [f32; 4],
    since_us: u64,
    /// When the newest stranger reading counted came.
    reading_at: Option<u64>,
}

/// The pause's state: the face the gate was open on, and the pause.
#[derive(Default)]
pub struct Pause {
    open_on: Option<Track>,
    paused: Option<Paused>,
    counts: Counts,
}

fn centre(edges: &[f32; 4]) -> (f32, f32) {
    ((edges[0] + edges[2]) / 2.0, (edges[1] + edges[3]) / 2.0)
}

/// Whether the box moved from `from` to `to` by more than the face's own
/// width (`from`'s): another place, so perhaps another person.
pub fn jumped(from: &[f32; 4], to: &[f32; 4]) -> bool {
    let (a, b) = (centre(from), centre(to));
    let width = (from[2] - from[0]).abs();
    (a.0 - b.0).hypot(a.1 - b.1) > width
}

/// Every rule of the gate holds but the familiar reading, with one face.
fn all_but_familiar(inputs: &Inputs) -> bool {
    inputs.faces == Some(1)
        && gate::may_request(&Inputs {
            familiar: Some(true),
            ..*inputs
        })
        .is_ok()
}

/// `serve`'s wiring of one step's `verdict` (`main.rs`): `room/stood_up`
/// during a pause ends it and wipes what it kept; otherwise the verdict
/// stands.
pub fn after_stood_up(pause: &mut Pause, verdict: Verdict, stood_up: bool) -> Verdict {
    if verdict.paused && stood_up {
        pause.end();
        WIPE
    } else {
        verdict
    }
}

/// Whether `serve` ends the sitting this step: on `room/stood_up`, or the
/// desk nobody's or a stranger's while not paused. A pause keeps it.
pub fn ends_sitting(verdict: Verdict, stood_up: bool, desk: Result<(), gate::Closed>) -> bool {
    stood_up
        || (!verdict.paused && matches!(desk, Err(gate::Closed::Nobody | gate::Closed::Stranger)))
}

impl Pause {
    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// Whether a pause holds now.
    pub fn paused(&self) -> bool {
        self.paused.is_some()
    }

    /// End any pause with what it kept wiped by the caller (`room/stood_up`).
    pub fn end(&mut self) {
        self.paused = None;
        self.open_on = None;
    }

    /// One step at `now_us` with the gate's inputs and the one face in view.
    pub fn step(&mut self, now_us: u64, inputs: &Inputs, seen: &OnTrack) -> Verdict {
        let open = gate::may_request(inputs).is_ok();
        let Some(mut paused) = self.paused.take() else {
            return self.step_unpaused(now_us, open, inputs, seen);
        };
        let Some(track) = seen.track.as_ref() else {
            // No face, a second face, or the faces not known.
            return WIPE;
        };
        if track.id != paused.id
            || jumped(&paused.last, &track.edges)
            || now_us.saturating_sub(paused.since_us) >= MOST_US
        {
            return WIPE;
        }
        if open {
            // A known reading said after the stranger one, on this id.
            self.open_on = Some(track.clone());
            return if jumped(&paused.at, &track.edges) {
                WIPE
            } else {
                GO_ON
            };
        }
        if !all_but_familiar(inputs) {
            return WIPE;
        }
        if seen.stranger && seen.reading_at != paused.reading_at {
            self.counts.misread_on_track = self.counts.misread_on_track.saturating_add(1);
            paused.reading_at = seen.reading_at;
        }
        paused.last = track.edges;
        self.paused = Some(paused);
        PAUSED
    }

    fn step_unpaused(
        &mut self,
        now_us: u64,
        open: bool,
        inputs: &Inputs,
        seen: &OnTrack,
    ) -> Verdict {
        let was = self.open_on.take();
        if open {
            self.open_on = seen.track.clone();
            return GO_ON;
        }
        if let (Some(was), Some(track)) = (was, seen.track.as_ref())
            && seen.stranger
            && track.id == was.id
            && !jumped(&was.edges, &track.edges)
            && all_but_familiar(inputs)
        {
            self.counts.paused = self.counts.paused.saturating_add(1);
            self.counts.misread_on_track = self.counts.misread_on_track.saturating_add(1);
            self.paused = Some(Paused {
                id: track.id.clone(),
                at: was.edges,
                last: track.edges,
                since_us: now_us,
                reading_at: seen.reading_at,
            });
            return PAUSED;
        }
        GO_ON
    }
}

#[cfg(test)]
pub(crate) mod tests;
