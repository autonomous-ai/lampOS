//! The pause on a misread, read from the bus as `lamp-seen serve` reads it
//! ([`crate::watch::Readings`]), with the Reviewer-lead's conditions as
//! tests.
use super::*;
use crate::frames::{Step, Taker, tests::Fake};
use crate::gate::{Closed, may_request};
use crate::mark::{
    Marked,
    tests::{FakeRing, Inner},
};
use crate::watch::Readings;
use lamp_msgs::{
    ClockStamp, ExpressionState, Face, Faces, FamiliarLevel, HumanFamiliar, MicrophonePrivacy,
    RoomPresence, SeenBy,
};

pub(crate) const S: u64 = 1_000_000;
pub(crate) const MS: u64 = 1_000;

fn lease(permitted: bool) -> MicrophonePrivacy {
    MicrophonePrivacy {
        stamp: ClockStamp {
            clock_id: "test-boot".into(),
            at_us: 1_000,
            sequence: 1,
        },
        session_id: "0123456789abcdef".into(),
        controller_boot: "0123456789abcdef0123456789abcdef".into(),
        microphone_generation: 1,
        observed_us: 990,
        expires_at_us: 1_040,
        permitted,
        physical_muted: Some(!permitted),
        no_slide: false,
    }
}

fn state(asleep: bool) -> ExpressionState {
    ExpressionState {
        stamp: ClockStamp {
            clock_id: "test-boot".into(),
            at_us: 1_000,
            sequence: 1,
        },
        enabled: true,
        turn: None,
        request: None,
        behaviours: vec![if asleep { "asleep" } else { "breath" }.into()],
        missing: Vec::new(),
        warnings: Vec::new(),
        body: false,
        light: false,
        asleep: Some(asleep),
    }
}

/// A face `id` whose box is 0.2 across, its left edge at `x`.
fn face(id: &str, x: f32) -> Face {
    Face {
        id: id.into(),
        xmin: x,
        ymin: 0.3,
        xmax: x + 0.2,
        ymax: 0.6,
        confidence: 0.9,
        yaw: None,
        facing: None,
        speaking: None,
        follows: None,
    }
}

/// The desk as the bus says it, with the pause stepped as `serve` steps it.
pub(crate) struct Scene {
    pub readings: Readings,
    pub pause: Pause,
    pub opted_in: bool,
    pub privacy: bool,
    pub asleep: bool,
}

impl Scene {
    /// The owner (`f1`, at 0.4) at the desk at `at`, recognised: the gate
    /// open.
    pub fn owner_at(at: u64) -> Self {
        let mut scene = Self {
            readings: Readings::default(),
            pause: Pause::default(),
            opted_in: true,
            privacy: true,
            asleep: false,
        };
        scene.faces(&[("f1", 0.4)], at);
        scene.known(at);
        assert_eq!(scene.step(at), GO_ON);
        assert_eq!(scene.gate(at), Ok(()));
        scene
    }

    pub fn faces(&mut self, faces: &[(&str, f32)], at: u64) {
        let faces = Faces {
            faces: faces.iter().map(|(id, x)| face(id, *x)).collect(),
        };
        self.readings.faces(&faces, at);
    }

    pub fn known(&mut self, at: u64) {
        self.readings.familiar(
            &HumanFamiliar {
                level: Some(FamiliarLevel::Regular),
            },
            at,
        );
    }

    pub fn stranger(&mut self, at: u64) {
        self.readings.familiar(&HumanFamiliar { level: None }, at);
    }

    fn renew(&mut self, at: u64) {
        self.readings.privacy(&lease(self.privacy), at);
        self.readings.presence(
            &RoomPresence {
                someone: true,
                nobody_s: 0,
                by: Some(SeenBy::Face),
            },
            at,
        );
        self.readings.expression(&state(self.asleep), at);
    }

    pub fn gate(&self, at: u64) -> Result<(), Closed> {
        may_request(&self.readings.inputs(self.opted_in, true, at))
    }

    /// One step of `serve` at `at`, the leases and presence renewed.
    pub fn step(&mut self, at: u64) -> Verdict {
        self.renew(at);
        let inputs = self.readings.inputs(self.opted_in, true, at);
        self.pause.step(at, &inputs, &self.readings.on_track(at))
    }

    /// The owner looks down at `at`: the same face, a stranger reading.
    pub fn misread(&mut self, at: u64) -> Verdict {
        self.faces(&[("f1", 0.4)], at);
        self.stranger(at);
        self.step(at)
    }
}

/// A pause at 2 s, from an owner known at 1 s.
fn paused() -> Scene {
    let mut scene = Scene::owner_at(S);
    assert_eq!(scene.misread(2 * S), PAUSED);
    assert!(scene.pause.paused());
    scene
}

#[test]
fn a_stranger_reading_on_the_track_sends_no_frame_and_asks_no_mark_until_known() {
    let mut scene = Scene::owner_at(0);
    let mut taker = Taker::new(Fake::default());
    let mut sink = Marked::new(FakeRing::default(), Inner::default());
    // Open: the mark is asked for the first frame.
    assert_eq!(taker.step(scene.gate(0), 0, &mut sink), Step::NotReady);
    let asked = sink.counts().asked;
    assert_eq!(scene.misread(100 * MS), PAUSED);
    let mut at = 100 * MS;
    while at < 4 * S {
        scene.faces(&[("f1", 0.41)], at);
        if at.is_multiple_of(S) {
            // Again and again: the owner still looking down.
            scene.stranger(at);
        }
        assert_eq!(scene.step(at), PAUSED, "{at}");
        let gate = scene.gate(at);
        assert_eq!(gate, Err(Closed::Stranger));
        assert!(matches!(
            taker.step(gate, at, &mut sink),
            Step::Idle | Step::Dropped(_)
        ));
        at += 100 * MS;
    }
    assert_eq!(taker.counts().requested, 0, "no frame asked of the camera");
    assert_eq!(sink.counts().asked, asked, "no mark asked while paused");
    assert_eq!(sink.counts().passed, 0);
    assert_eq!(scene.pause.counts().paused, 1);
    assert_eq!(
        scene.pause.counts().misread_on_track,
        4,
        "the first and 1, 2, 3 s"
    );
    // A known reading on the same id: open, kept, the mark asked again.
    scene.known(at);
    assert_eq!(scene.step(at), GO_ON);
    assert_eq!(scene.gate(at), Ok(()));
    taker.step(scene.gate(at), at, &mut sink);
    assert_eq!(sink.counts().asked, asked + 1);
}

#[test]
fn time_alone_never_ends_a_pause_and_past_5_s_it_wipes() {
    let mut scene = paused();
    scene.faces(&[("f1", 0.4)], 6 * S);
    assert_eq!(scene.step(6 * S), PAUSED, "4 s paused");
    assert_eq!(scene.gate(6 * S), Err(Closed::Stranger));
    scene.faces(&[("f1", 0.4)], 7 * S);
    assert_eq!(scene.step(7 * S), WIPE, "5 s paused");
    assert!(!scene.pause.paused());
    // The stranger reading went stale: still closed, no reading said since.
    scene.faces(&[("f1", 0.4)], 8 * S);
    assert_eq!(scene.step(8 * S), GO_ON);
    assert_ne!(scene.gate(8 * S), Ok(()));
    // A known reading after the wipe opens a fresh sitting, kept nothing.
    scene.known(8 * S + 1);
    assert_eq!(scene.step(8 * S + 1), GO_ON);
    assert_eq!(scene.gate(8 * S + 1), Ok(()));
}

#[test]
fn a_known_reading_past_5_s_still_wipes() {
    let mut scene = paused();
    scene.faces(&[("f1", 0.4)], 7 * S);
    scene.known(7 * S);
    assert_eq!(scene.step(7 * S), WIPE);
}

#[test]
fn the_privacy_during_a_pause_wipes_at_once() {
    let mut scene = paused();
    scene.privacy = false;
    assert_eq!(scene.step(2 * S + 100 * MS), WIPE);
    assert_eq!(scene.gate(2 * S + 100 * MS), Err(Closed::PrivacyClosed));
    // Open again with a known reading: no pause to resume, a fresh start.
    scene.privacy = true;
    scene.known(3 * S);
    assert_eq!(scene.step(3 * S), GO_ON);
    assert!(!scene.pause.paused());
}

#[test]
fn sleep_during_a_pause_wipes_at_once() {
    let mut scene = paused();
    scene.asleep = true;
    assert_eq!(scene.step(2 * S + 100 * MS), WIPE);
}

#[test]
fn the_opt_in_withdrawn_during_a_pause_wipes_at_once() {
    let mut scene = paused();
    scene.opted_in = false;
    assert_eq!(scene.step(2 * S + 100 * MS), WIPE);
    assert_eq!(scene.gate(2 * S + 100 * MS), Err(Closed::NoOptIn));
}

#[test]
fn a_new_id_during_a_pause_closes_and_wipes_and_no_yes_carries() {
    let mut scene = paused();
    scene.faces(&[("f2", 0.4)], 2 * S + 500 * MS);
    assert_eq!(scene.step(2 * S + 500 * MS), WIPE);
    // The owner's known reading, said before f2 came, is not f2's.
    assert_eq!(scene.gate(2 * S + 500 * MS), Err(Closed::Stranger));
    // A stranger reading on f2 never pauses: the gate was never open on it.
    scene.stranger(3 * S);
    assert_eq!(scene.step(3 * S), GO_ON);
    assert!(!scene.pause.paused());
}

#[test]
fn a_second_face_closes_at_once() {
    let mut scene = paused();
    scene.faces(&[("f1", 0.4), ("f2", 0.1)], 2 * S + 100 * MS);
    assert_eq!(scene.step(2 * S + 100 * MS), WIPE);
    assert_eq!(scene.gate(2 * S + 100 * MS), Err(Closed::Stranger));
}

#[test]
fn no_face_closes_at_once() {
    let mut scene = paused();
    scene.faces(&[], 2 * S + 100 * MS);
    assert_eq!(scene.step(2 * S + 100 * MS), WIPE);
    assert_eq!(scene.gate(2 * S + 100 * MS), Err(Closed::Nobody));
}

#[test]
fn a_box_jump_wider_than_the_face_wipes_at_once() {
    // A move within the face's width (0.2) between samples is kept.
    let mut scene = paused();
    scene.faces(&[("f1", 0.55)], 2 * S + 500 * MS);
    assert_eq!(scene.step(2 * S + 500 * MS), PAUSED);
    // Then a jump of 0.25: another place, maybe another person.
    scene.faces(&[("f1", 0.8)], 3 * S);
    assert_eq!(scene.step(3 * S), WIPE);
    // At the start too: a stranger reading on a box that jumped never pauses.
    let mut scene = Scene::owner_at(S);
    scene.faces(&[("f1", 0.7)], 2 * S);
    scene.stranger(2 * S);
    assert_eq!(scene.step(2 * S), GO_ON);
    assert!(!scene.pause.paused());
}

#[test]
fn resuming_near_where_it_paused_keeps_and_far_wipes() {
    // Near: the box back within the face's width of where it paused.
    let mut scene = paused();
    scene.faces(&[("f1", 0.5)], 3 * S);
    scene.known(3 * S);
    assert_eq!(scene.step(3 * S), GO_ON, "kept");
    assert_eq!(scene.gate(3 * S), Ok(()));
    // Far: it drifted in small steps, each under the face's width, to 0.3
    // from where it paused: open on the same id, but fresh.
    let mut scene = paused();
    for (index, x) in [0.5, 0.6, 0.7].into_iter().enumerate() {
        let at = 2 * S + (index as u64 + 1) * 500 * MS;
        scene.faces(&[("f1", x)], at);
        assert_eq!(scene.step(at), PAUSED, "{x}");
    }
    scene.known(4 * S);
    scene.faces(&[("f1", 0.7)], 4 * S);
    assert_eq!(scene.step(4 * S), WIPE, "far from where it paused");
    assert_eq!(scene.gate(4 * S), Ok(()), "open, from a fresh start");
    assert!(!scene.pause.paused());
}

#[test]
fn a_stranger_reading_on_a_face_never_open_does_not_pause() {
    let mut scene = Scene::owner_at(S);
    scene.privacy = false;
    assert_eq!(scene.step(2 * S), GO_ON);
    scene.privacy = true;
    assert_eq!(
        scene.misread(3 * S),
        GO_ON,
        "the gate was not open the step before"
    );
    assert!(!scene.pause.paused());
}

#[test]
fn the_box_jump_rule() {
    let at = [0.4, 0.3, 0.6, 0.6];
    assert!(!jumped(&at, &[0.55, 0.3, 0.75, 0.6]));
    assert!(
        !jumped(&at, &[0.59, 0.3, 0.79, 0.6]),
        "just under its width"
    );
    assert!(jumped(&at, &[0.65, 0.3, 0.85, 0.6]));
    assert!(
        jumped(&at, &[0.4, 0.55, 0.6, 0.85]),
        "down as well as across"
    );
}

/// `serve`'s wiring (`main.rs`), the Reviewer-lead's P3 on 52aa7835a7: the
/// wipe on `room/stood_up` while paused, and the sitting kept while paused.
#[test]
fn serve_wipes_on_standing_up_while_paused_and_keeps_the_sitting_only_while_paused() {
    // Standing up during a pause ends it and wipes.
    let mut scene = paused();
    let verdict = scene.misread(3 * S);
    assert_eq!(verdict, PAUSED);
    let wiring = after_stood_up(&mut scene.pause, verdict, true);
    assert_eq!(wiring, WIPE);
    assert!(!scene.pause.paused(), "the pause ended");
    assert!(ends_sitting(wiring, true, scene.gate(3 * S)));
    // Not standing up, the pause and its verdict stand, and the sitting is
    // kept although the desk reads a stranger.
    let mut scene = paused();
    let verdict = scene.misread(3 * S);
    assert_eq!(after_stood_up(&mut scene.pause, verdict, false), PAUSED);
    assert!(scene.pause.paused());
    assert_eq!(scene.gate(3 * S), Err(Closed::Stranger));
    assert!(!ends_sitting(PAUSED, false, Err(Closed::Stranger)));
    assert!(!ends_sitting(PAUSED, false, Err(Closed::Nobody)));
    // Not paused, a stranger's or an empty desk ends the sitting, and
    // standing up always does; an open desk keeps it.
    assert!(ends_sitting(GO_ON, false, Err(Closed::Stranger)));
    assert!(ends_sitting(GO_ON, false, Err(Closed::Nobody)));
    assert!(ends_sitting(GO_ON, true, Ok(())));
    assert!(!ends_sitting(GO_ON, false, Ok(())));
    // Standing up while not paused leaves the verdict as it was.
    let mut scene = Scene::owner_at(S);
    assert_eq!(after_stood_up(&mut scene.pause, GO_ON, true), GO_ON);
}
