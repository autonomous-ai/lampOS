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
//! | a face in view | `humans/faces/tracked` (`Faces`) | `faces.len()` |
//! | awake | `expression/state` (`ExpressionState`) | `says_asleep()` |
//! | the opt-in off after a forget | `memory/forgot` (`MemoryForgot`) | any |
//!
//! `humans/familiar` is not read: there is no stranger check (the owner's
//! word on #6543), so whose face is in view never closes the gate. The one
//! face in view's passing id is still kept ([`OnTrack`]), for the keep's
//! rule that a sitting kept through a short closing goes on only for the
//! same face ([`crate::keep`]).
//!
//! Nothing here is the eyes' private files (`--presence-file`,
//! `--known-file`): the bus only.
use crate::gate::Inputs;
use lamp_msgs::{ExpressionState, Faces, MicrophonePrivacy, RoomPresence};

/// How long a `voice/privacy` lease is held after it came: its declared
/// lifespan.
pub const PRIVACY_LAST_US: u64 = 50_000;
/// `room/presence`'s declared lifespan.
pub const PRESENCE_LAST_US: u64 = 3_000_000;
/// `humans/faces/tracked` declares no lifespan; the eyes say it with each
/// look, at least about twice a second while they look.
pub const FACES_LAST_US: u64 = 2_000_000;
/// `expression/state`'s declared lifespan.
pub const EXPRESSION_LAST_US: u64 = ExpressionState::LASTS_US;

/// The one face in view: its passing id (`f3`, the eyes', gone with the
/// face) and its box (xmin, ymin, xmax, ymax, fractions of the picture).
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: String,
    pub edges: [f32; 4],
}

/// The one face in view, as the keep reads it ([`Readings::on_track`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OnTrack {
    /// The one face in view; `None` for no face, two, or not known.
    pub track: Option<Track>,
}

/// The newest reading of each topic, and when it came (this process's
/// monotonic microseconds).
#[derive(Clone, Debug, Default)]
pub struct Readings {
    privacy: Option<(Option<bool>, u64)>,
    presence: Option<(Option<bool>, u64)>,
    faces: Option<(Option<usize>, u64)>,
    /// The one face in view, by its passing id, and its box in the newest
    /// list; `None` while not exactly one valid face with a non-empty id.
    alone: Option<(String, [f32; 4])>,
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
            // An empty id is no one face's: it would match another.
            (Some(1), Some(face)) if !face.id.is_empty() => Some((
                face.id.clone(),
                [face.xmin, face.ymin, face.xmax, face.ymax],
            )),
            _ => None,
        };
    }

    /// Expression's state: asleep or not, when valid and enabled; an
    /// invalid or disabled state is not known.
    pub fn expression(&mut self, state: &ExpressionState, now_us: u64) {
        let asleep = (state.check().is_ok() && state.enabled).then(|| state.says_asleep());
        self.asleep = Some((asleep, now_us));
    }

    /// The one face in view, for the keep ([`crate::keep`]): its passing id
    /// and box while the newest list is fresh and has exactly one face.
    pub fn on_track(&self, now_us: u64) -> OnTrack {
        let one = fresh(self.faces, FACES_LAST_US, now_us).flatten() == Some(1);
        let track = self
            .alone
            .as_ref()
            .filter(|_| one)
            .map(|(id, edges)| Track {
                id: id.clone(),
                edges: *edges,
            });
        OnTrack { track }
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
            asleep: fresh(self.asleep, EXPRESSION_LAST_US, now_us).flatten(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{Closed, may_request};
    use lamp_msgs::{ClockStamp, Face, SeenBy};

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
            pitch: None,
            looking: None,
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
        readings.expression(&state(Some(false)), at);
        readings
    }

    #[test]
    fn a_face_with_an_empty_id_is_on_no_track() {
        // Twelve's P3 on #7635: an empty passing id would match another
        // empty one, so it is no one face's id; the count still opens the
        // gate as before.
        let mut readings = Readings::default();
        readings.faces(
            &Faces {
                faces: vec![face("")],
            },
            S,
        );
        assert_eq!(readings.on_track(S), OnTrack::default());
        assert_eq!(readings.inputs(true, true, S).faces, Some(1));
        readings.faces(
            &Faces {
                faces: vec![face("f1")],
            },
            2 * S,
        );
        assert_eq!(
            readings.on_track(2 * S).track.map(|track| track.id),
            Some("f1".to_owned())
        );
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
    fn nobody_is_read_from_presence_and_faces() {
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

        // Two faces, or a face nobody remembers: no stranger check, open.
        let mut readings = all_clear(S);
        readings.faces(
            &Faces {
                faces: vec![face("f1"), face("f2")],
            },
            S,
        );
        assert_eq!(may_request(&readings.inputs(true, true, S)), Ok(()));
        assert_eq!(readings.on_track(S).track, None);
    }

    #[test]
    fn a_guest_who_sits_down_is_seen_as_the_owner_was() {
        // The owner (f1) at 1 s; they leave, and a guest (f2) sits down.
        let mut readings = all_clear(S);
        readings.faces(&Faces { faces: Vec::new() }, S + 10);
        readings.faces(
            &Faces {
                faces: vec![face("f2")],
            },
            S + 20,
        );
        assert_eq!(may_request(&readings.inputs(true, true, S + 30)), Ok(()));
        assert_eq!(
            readings.on_track(S + 30).track.map(|track| track.id),
            Some("f2".to_owned())
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
