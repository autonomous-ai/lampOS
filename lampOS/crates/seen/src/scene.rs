//! The desk as the bus says it, read as `lamp-seen serve` reads it
//! ([`crate::watch::Readings`]), for the keep's, posture's and the hold's
//! tests.
use crate::gate::{Closed, may_request};
use crate::watch::Readings;
use lamp_msgs::{
    ClockStamp, ExpressionState, Face, Faces, MicrophonePrivacy, RoomPresence, SeenBy,
};

pub(crate) const S: u64 = 1_000_000;

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
        pitch: None,
        looking: None,
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

/// The desk as the bus says it.
pub(crate) struct Scene {
    pub readings: Readings,
    pub opted_in: bool,
    pub privacy: bool,
    pub asleep: bool,
}

impl Scene {
    /// The owner (`f1`, at 0.4) at the desk at `at`: the gate open.
    pub fn owner_at(at: u64) -> Self {
        let mut scene = Self {
            readings: Readings::default(),
            opted_in: true,
            privacy: true,
            asleep: false,
        };
        scene.faces(&[("f1", 0.4)], at);
        scene.renew(at);
        assert_eq!(scene.gate(at), Ok(()));
        scene
    }

    pub fn faces(&mut self, faces: &[(&str, f32)], at: u64) {
        let faces = Faces {
            faces: faces.iter().map(|(id, x)| face(id, *x)).collect(),
        };
        self.readings.faces(&faces, at);
    }

    /// The leases, presence and sleep renewed at `at`, as `serve` hears
    /// them each step.
    pub fn renew(&mut self, at: u64) {
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
}

#[test]
fn a_guest_or_a_second_face_keeps_the_gate_open() {
    // No stranger check (the owner's word on #6543): the owner turned
    // away, a guest in their place, or a second face beside them.
    let mut scene = Scene::owner_at(0);
    for (at, faces) in [
        (S, &[("f2", 0.4)][..]),
        (2 * S, &[("f2", 0.4), ("f3", 0.1)][..]),
        (3 * S, &[("f1", 0.4)][..]),
    ] {
        scene.faces(faces, at);
        scene.renew(at);
        assert_eq!(scene.gate(at), Ok(()), "{at}");
    }
}
