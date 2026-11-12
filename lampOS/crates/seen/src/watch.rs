//! What the bus says, kept for the gate (#6543): the newest of each topic
//! the gate reads, with when it came, so a reading is used only while it
//! lasts. The bus already passes over a message that has outlived its
//! declared lifespan; here each is also held only for that long after it
//! came (`humans/faces/tracked` declares none: [`FACES_LAST_US`]).
//!
//! | Gate rule | Topic | Field |
//! |---|---|---|
//! | privacy open | `voice/privacy` (`MicrophonePrivacy`) | a valid lease, `permitted` |
//! | someone at the desk | `room/presence` (`RoomPresence`) | `someone` |
//! | a face in view, one face | `humans/faces/tracked` (`Faces`) | `faces.len()` |
//! | not a stranger | `humans/familiar` (`HumanFamiliar`) | `level` is some, said after the one face in view came |
//! | awake | `expression/state` (`ExpressionState`) | `says_asleep()` |
//! | the opt-in off after a forget | `memory/forgot` (`MemoryForgot`) | any |
//!
//! `humans/familiar` names no face, so a level is taken only for the one
//! face that was already in view when it was said: it counts only when it
//! came at or after the moment the one face now in view, by its passing id,
//! came into view as the only face. Another id, a second face, no face or
//! an invalid list since then means the level was about someone else, and
//! the gate stays closed until the eyes say a level again (the
//! Reviewer-lead's P2 on #6602). A new id that `follows` an old one counts
//! as a new face.
//!
//! Nothing here is the eyes' private files (`--presence-file`,
//! `--known-file`): the bus only.
use crate::gate::Inputs;
use lamp_msgs::{ExpressionState, Faces, HumanFamiliar, MicrophonePrivacy, RoomPresence};

/// How long a `voice/privacy` lease is held after it came: its declared
/// lifespan.
pub const PRIVACY_LAST_US: u64 = 50_000;
/// `room/presence`'s declared lifespan.
pub const PRESENCE_LAST_US: u64 = 3_000_000;
/// `humans/faces/tracked` declares no lifespan; the eyes say it with each
/// look, at least about twice a second while they look.
pub const FACES_LAST_US: u64 = 2_000_000;
/// `humans/familiar`'s declared lifespan.
pub const FAMILIAR_LAST_US: u64 = 5_000_000;
/// `expression/state`'s declared lifespan.
pub const EXPRESSION_LAST_US: u64 = ExpressionState::LASTS_US;

/// The newest reading of each topic, and when it came (this process's
/// monotonic microseconds).
#[derive(Clone, Debug, Default)]
pub struct Readings {
    privacy: Option<(Option<bool>, u64)>,
    presence: Option<(Option<bool>, u64)>,
    faces: Option<(Option<usize>, u64)>,
    /// The one face in view, by its passing id, and when it came into view
    /// as the only face; `None` while not exactly one valid face.
    alone: Option<(String, u64)>,
    familiar: Option<(bool, u64)>,
    asleep: Option<(Option<bool>, u64)>,
}

fn fresh<T: Copy>(kept: Option<(T, u64)>, last_us: u64, now_us: u64) -> Option<T> {
    kept.filter(|(_, at)| now_us >= *at && now_us - at <= last_us)
        .map(|(value, _)| value)
}

impl Readings {
    /// A privacy lease: open only when valid and permitting; an invalid one
    /// is not known.
    pub fn privacy(&mut self, lease: &MicrophonePrivacy, now_us: u64) {
        let open = lease.check().is_ok().then_some(lease.permitted);
        self.privacy = Some((open, now_us));
    }

    pub fn presence(&mut self, presence: &RoomPresence, now_us: u64) {
        let someone = presence.check().is_ok().then_some(presence.someone);
        self.presence = Some((someone, now_us));
    }

    pub fn faces(&mut self, faces: &Faces, now_us: u64) {
        let count = faces.check().is_ok().then_some(faces.faces.len());
        self.faces = Some((count, now_us));
        self.alone = match (count, faces.faces.first()) {
            (Some(1), Some(face)) => match self.alone.take() {
                Some((id, since)) if id == face.id => Some((id, since)),
                _ => Some((face.id.clone(), now_us)),
            },
            _ => None,
        };
    }

    pub fn familiar(&mut self, familiar: &HumanFamiliar, now_us: u64) {
        self.familiar = Some((familiar.level.is_some(), now_us));
    }

    /// Expression's state: asleep or not, when valid and enabled; an
    /// invalid or disabled state is not known.
    pub fn expression(&mut self, state: &ExpressionState, now_us: u64) {
        let asleep = (state.check().is_ok() && state.enabled).then(|| state.says_asleep());
        self.asleep = Some((asleep, now_us));
    }

    /// Whether the newest `humans/familiar` came at or after the one face
    /// now in view came into view alone.
    fn familiar_after_alone(&self) -> bool {
        match (self.familiar, &self.alone) {
            (Some((_, said)), Some((_, since))) => said >= *since,
            _ => false,
        }
    }

    /// The gate's inputs at `now_us`, with the opt-in and whether its on
    /// line has begun as given.
    pub fn inputs(&self, opted_in: bool, said_on: bool, now_us: u64) -> Inputs {
        Inputs {
            opted_in,
            said_on,
            privacy_open: fresh(self.privacy, PRIVACY_LAST_US, now_us).flatten(),
            someone: fresh(self.presence, PRESENCE_LAST_US, now_us).flatten(),
            faces: fresh(self.faces, FACES_LAST_US, now_us).flatten(),
            familiar: fresh(self.familiar, FAMILIAR_LAST_US, now_us)
                .map(|level| level && self.familiar_after_alone()),
            asleep: fresh(self.asleep, EXPRESSION_LAST_US, now_us).flatten(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{Closed, may_request};
    use lamp_msgs::{ClockStamp, Face, FamiliarLevel, SeenBy};

    const S: u64 = 1_000_000;

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

    fn face(id: &str) -> Face {
        Face {
            id: id.into(),
            xmin: 0.3,
            ymin: 0.3,
            xmax: 0.5,
            ymax: 0.6,
            confidence: 0.9,
            yaw: None,
            facing: None,
            speaking: None,
            follows: None,
        }
    }

    fn state(asleep: Option<bool>) -> ExpressionState {
        ExpressionState {
            stamp: ClockStamp {
                clock_id: "test-boot".into(),
                at_us: 1_000,
                sequence: 1,
            },
            enabled: true,
            turn: None,
            request: None,
            behaviours: if asleep == Some(true) {
                vec!["asleep".into()]
            } else {
                vec!["breath".into()]
            },
            missing: Vec::new(),
            warnings: Vec::new(),
            body: false,
            light: false,
            asleep,
        }
    }

    /// Everything the gate wants, said at `at`.
    fn all_clear(at: u64) -> Readings {
        let mut readings = Readings::default();
        readings.privacy(&lease(true), at);
        readings.presence(
            &RoomPresence {
                someone: true,
                nobody_s: 0,
                by: Some(SeenBy::Face),
            },
            at,
        );
        readings.faces(
            &Faces {
                faces: vec![face("f1")],
            },
            at,
        );
        readings.familiar(
            &HumanFamiliar {
                level: Some(FamiliarLevel::Regular),
            },
            at,
        );
        readings.expression(&state(Some(false)), at);
        readings
    }

    #[test]
    fn nothing_heard_is_nothing_known() {
        assert_eq!(
            Readings::default().inputs(true, true, 5 * S),
            Inputs {
                opted_in: true,
                said_on: true,
                ..Inputs::default()
            }
        );
    }

    #[test]
    fn fresh_readings_open_the_gate() {
        assert_eq!(
            may_request(&all_clear(S).inputs(true, true, S + 10_000)),
            Ok(())
        );
    }

    #[test]
    fn each_reading_lapses_after_its_lifespan() {
        let readings = all_clear(S);
        // The privacy lease first, at 50 ms: then not known.
        assert_eq!(
            may_request(&readings.inputs(true, true, S + PRIVACY_LAST_US + 1)),
            Err(Closed::PrivacyUnknown)
        );
        let inputs = readings.inputs(true, true, S + 10 * S);
        assert_eq!(inputs.someone, None);
        assert_eq!(inputs.faces, None);
        assert_eq!(inputs.familiar, None);
        assert_eq!(inputs.asleep, None);
        // A reading from the future (another clock) is not taken.
        assert_eq!(readings.inputs(true, true, S - 1).privacy_open, None);
    }

    #[test]
    fn the_slide_closed_is_read_from_the_lease() {
        let mut readings = all_clear(S);
        readings.privacy(&lease(false), S);
        assert_eq!(
            may_request(&readings.inputs(true, true, S)),
            Err(Closed::PrivacyClosed)
        );
        // An invalid lease is not known, never open.
        let mut bad = lease(true);
        bad.controller_boot = "0".repeat(32);
        readings.privacy(&bad, S);
        assert_eq!(
            may_request(&readings.inputs(true, true, S)),
            Err(Closed::PrivacyUnknown)
        );
    }

    #[test]
    fn nobody_and_strangers_are_read_from_presence_faces_and_familiar() {
        let mut readings = all_clear(S);
        readings.presence(
            &RoomPresence {
                someone: false,
                nobody_s: 30,
                by: None,
            },
            S,
        );
        assert_eq!(
            may_request(&readings.inputs(true, true, S)),
            Err(Closed::Nobody)
        );

        let mut readings = all_clear(S);
        readings.faces(&Faces { faces: Vec::new() }, S);
        assert_eq!(
            may_request(&readings.inputs(true, true, S)),
            Err(Closed::Nobody)
        );

        let mut readings = all_clear(S);
        readings.faces(
            &Faces {
                faces: vec![face("f1"), face("f2")],
            },
            S,
        );
        assert_eq!(
            may_request(&readings.inputs(true, true, S)),
            Err(Closed::Stranger)
        );

        let mut readings = all_clear(S);
        readings.familiar(&HumanFamiliar { level: None }, S);
        assert_eq!(
            may_request(&readings.inputs(true, true, S)),
            Err(Closed::Stranger)
        );
    }

    fn one_face(readings: &mut Readings, id: &str, at: u64) {
        readings.faces(
            &Faces {
                faces: vec![face(id)],
            },
            at,
        );
    }

    fn regular(readings: &mut Readings, at: u64) {
        readings.familiar(
            &HumanFamiliar {
                level: Some(FamiliarLevel::Regular),
            },
            at,
        );
    }

    /// Fresh privacy, presence and awake at `at`, so only the faces and the
    /// familiar reading decide.
    fn renew(readings: &mut Readings, at: u64) {
        readings.privacy(&lease(true), at);
        readings.presence(
            &RoomPresence {
                someone: true,
                nobody_s: 0,
                by: Some(SeenBy::Face),
            },
            at,
        );
        readings.expression(&state(Some(false)), at);
    }

    #[test]
    fn the_owners_level_does_not_carry_over_to_a_guest_who_sits_down() {
        // The owner (f1) recognised at 1 s; they leave, and a guest (f2)
        // sits down within 1 s, or within 4 s, while the level is fresh.
        for gap in [S / 2, S, 4 * S] {
            let mut readings = all_clear(S);
            readings.faces(&Faces { faces: Vec::new() }, S + gap / 2);
            one_face(&mut readings, "f2", S + gap);
            renew(&mut readings, S + gap);
            let inputs = readings.inputs(true, true, S + gap);
            assert!(inputs.familiar.is_some(), "the level is still fresh");
            assert_eq!(may_request(&inputs), Err(Closed::Stranger), "{gap}");
        }
        // The owner's face swapped for the guest's in one look, no empty
        // list between.
        let mut readings = all_clear(S);
        one_face(&mut readings, "f2", S + S / 10);
        renew(&mut readings, S + S / 10);
        assert_eq!(
            may_request(&readings.inputs(true, true, S + S / 10)),
            Err(Closed::Stranger)
        );
    }

    #[test]
    fn a_second_face_since_the_level_closes_until_a_new_level() {
        // The owner, then a guest beside them, then the owner alone again:
        // the level said before the guest came is not taken.
        let mut readings = all_clear(S);
        readings.faces(
            &Faces {
                faces: vec![face("f1"), face("f2")],
            },
            S + S / 2,
        );
        one_face(&mut readings, "f1", S + S);
        renew(&mut readings, 2 * S);
        assert_eq!(
            may_request(&readings.inputs(true, true, 2 * S)),
            Err(Closed::Stranger)
        );
        regular(&mut readings, 2 * S + 1);
        assert_eq!(may_request(&readings.inputs(true, true, 2 * S + 1)), Ok(()));
    }

    #[test]
    fn the_same_person_staying_keeps_the_gate_open() {
        let mut readings = all_clear(S);
        for step in 1..=4 {
            let at = S + step * S / 2;
            one_face(&mut readings, "f1", at);
            renew(&mut readings, at);
            assert_eq!(
                may_request(&readings.inputs(true, true, at)),
                Ok(()),
                "{at}"
            );
        }
    }

    #[test]
    fn a_level_said_after_the_guest_reopens_the_gate() {
        let mut readings = all_clear(S);
        one_face(&mut readings, "f2", 2 * S);
        renew(&mut readings, 2 * S);
        assert_eq!(
            may_request(&readings.inputs(true, true, 2 * S)),
            Err(Closed::Stranger)
        );
        // The eyes recognise the one now in view as remembered.
        regular(&mut readings, 3 * S);
        renew(&mut readings, 3 * S);
        assert_eq!(may_request(&readings.inputs(true, true, 3 * S)), Ok(()));
    }

    #[test]
    fn a_level_with_no_face_seen_yet_is_not_taken() {
        let mut readings = Readings::default();
        regular(&mut readings, S);
        one_face(&mut readings, "f1", S + 1);
        renew(&mut readings, S + 1);
        assert_eq!(
            may_request(&readings.inputs(true, true, S + 1)),
            Err(Closed::Stranger)
        );
    }

    #[test]
    fn asleep_is_read_from_expression() {
        let mut readings = all_clear(S);
        readings.expression(&state(Some(true)), S);
        assert_eq!(
            may_request(&readings.inputs(true, true, S)),
            Err(Closed::Asleep)
        );
        // Expression planning no idle layer this step says nothing of sleep:
        // awake by its word.
        readings.expression(&state(None), S);
        assert_eq!(may_request(&readings.inputs(true, true, S)), Ok(()));
        // A disabled Expression says nothing of sleep: not known.
        let mut off = state(Some(false));
        off.enabled = false;
        readings.expression(&off, S);
        assert_eq!(
            may_request(&readings.inputs(true, true, S)),
            Err(Closed::SleepUnknown)
        );
    }
}
