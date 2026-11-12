//! The "no frames" gate (#6543, phase 0's third line): no frame is even
//! asked of the camera unless every one of these holds, each read from the
//! bus and the opt-in, never from the eyes' private files:
//!
//! 1. the owner's opt-in is on ([`crate::opt_in`]);
//! 2. the camera's privacy is open: a fresh `voice/privacy` lease that
//!    permits capture (the camera looks exactly while the microphone may
//!    listen, docs/eyes.md); a frame's own grant is checked again when it
//!    comes ([`crate::frames`]);
//! 3. someone is at the desk: a fresh `room/presence` saying someone, and a
//!    face in view now on `humans/faces/tracked` (a voice alone shows
//!    nobody at the desk to see);
//! 4. no stranger is in view: exactly one face, and `humans/familiar`
//!    fresh with a level (someone who asked the lamp to remember them),
//!    said after that face, by its passing id, came into view alone
//!    ([`crate::watch`]): a level names no face, so one said before the
//!    face now in view came is someone else's;
//! 5. the lamp is awake: a fresh `expression/state` that does not say
//!    asleep.
//!
//! Whatever is not known is closed: a reading gone stale, never said, or
//! unreadable is never taken as all clear.

/// What the gate reads, each `None` while not known (stale, never said or
/// unreadable).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Inputs {
    /// The owner's opt-in is on (and no forget has come since it was given).
    pub opted_in: bool,
    /// `voice/privacy`: a fresh, valid lease, and whether it permits.
    pub privacy_open: Option<bool>,
    /// `room/presence`: someone at the lamp.
    pub someone: Option<bool>,
    /// `humans/faces/tracked`: how many faces are in view now.
    pub faces: Option<usize>,
    /// `humans/familiar`: the one at the lamp is remembered (a level, said
    /// after the one face now in view came into view alone), or not (none,
    /// or a level said before that face came).
    pub familiar: Option<bool>,
    /// `expression/state`: asleep or falling asleep.
    pub asleep: Option<bool>,
}

/// Why no frame is asked for: the first rule that does not hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closed {
    /// The owner has not opted in, or a forget came after.
    NoOptIn,
    /// The privacy slide is closed, or the voice muted capture.
    PrivacyClosed,
    /// No fresh privacy lease: not known to be open.
    PrivacyUnknown,
    /// Nobody is at the lamp, or no face is in view now.
    Nobody,
    /// Presence or faces not known.
    PresenceUnknown,
    /// More than one face, or the one face not someone remembered.
    Stranger,
    /// The lamp is asleep or falling asleep.
    Asleep,
    /// Whether the lamp is asleep is not known.
    SleepUnknown,
}

impl Closed {
    /// A short word for the journal and `diagnostics`.
    pub fn as_str(self) -> &'static str {
        match self {
            Closed::NoOptIn => "no_opt_in",
            Closed::PrivacyClosed => "privacy_closed",
            Closed::PrivacyUnknown => "privacy_unknown",
            Closed::Nobody => "nobody",
            Closed::PresenceUnknown => "presence_unknown",
            Closed::Stranger => "stranger",
            Closed::Asleep => "asleep",
            Closed::SleepUnknown => "sleep_unknown",
        }
    }
}

/// Whether a frame may be asked for now: `Ok` only when every rule holds,
/// else the first that does not, in the order of the module's list.
pub fn may_request(inputs: &Inputs) -> Result<(), Closed> {
    if !inputs.opted_in {
        return Err(Closed::NoOptIn);
    }
    match inputs.privacy_open {
        Some(true) => {}
        Some(false) => return Err(Closed::PrivacyClosed),
        None => return Err(Closed::PrivacyUnknown),
    }
    match (inputs.someone, inputs.faces) {
        (Some(false), _) | (_, Some(0)) => return Err(Closed::Nobody),
        (None, _) | (_, None) => return Err(Closed::PresenceUnknown),
        (Some(true), Some(_)) => {}
    }
    // One face, and that face someone remembered. A second face is anyone's.
    if inputs.faces != Some(1) || inputs.familiar != Some(true) {
        return Err(Closed::Stranger);
    }
    match inputs.asleep {
        Some(false) => Ok(()),
        Some(true) => Err(Closed::Asleep),
        None => Err(Closed::SleepUnknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear() -> Inputs {
        Inputs {
            opted_in: true,
            privacy_open: Some(true),
            someone: Some(true),
            faces: Some(1),
            familiar: Some(true),
            asleep: Some(false),
        }
    }

    #[test]
    fn all_clear_would_request() {
        assert_eq!(may_request(&clear()), Ok(()));
    }

    #[test]
    fn nothing_known_is_closed_and_the_opt_in_is_asked_first() {
        assert_eq!(may_request(&Inputs::default()), Err(Closed::NoOptIn));
    }

    #[test]
    fn no_opt_in_no_frame() {
        let inputs = Inputs {
            opted_in: false,
            ..clear()
        };
        assert_eq!(may_request(&inputs), Err(Closed::NoOptIn));
    }

    #[test]
    fn the_slide_on_no_frame() {
        let inputs = Inputs {
            privacy_open: Some(false),
            ..clear()
        };
        assert_eq!(may_request(&inputs), Err(Closed::PrivacyClosed));
    }

    #[test]
    fn privacy_not_known_no_frame() {
        let inputs = Inputs {
            privacy_open: None,
            ..clear()
        };
        assert_eq!(may_request(&inputs), Err(Closed::PrivacyUnknown));
    }

    #[test]
    fn nobody_at_the_desk_no_frame() {
        let inputs = Inputs {
            someone: Some(false),
            ..clear()
        };
        assert_eq!(may_request(&inputs), Err(Closed::Nobody));
    }

    #[test]
    fn someone_heard_but_no_face_in_view_no_frame() {
        let inputs = Inputs {
            faces: Some(0),
            ..clear()
        };
        assert_eq!(may_request(&inputs), Err(Closed::Nobody));
    }

    #[test]
    fn presence_not_known_no_frame() {
        for inputs in [
            Inputs {
                someone: None,
                ..clear()
            },
            Inputs {
                faces: None,
                ..clear()
            },
        ] {
            assert_eq!(may_request(&inputs), Err(Closed::PresenceUnknown));
        }
    }

    #[test]
    fn a_stranger_in_view_no_frame() {
        // The one face not remembered, or not known to be.
        for familiar in [Some(false), None] {
            let inputs = Inputs {
                familiar,
                ..clear()
            };
            assert_eq!(may_request(&inputs), Err(Closed::Stranger), "{familiar:?}");
        }
    }

    #[test]
    fn a_second_face_is_a_stranger_even_beside_a_remembered_one() {
        let inputs = Inputs {
            faces: Some(2),
            ..clear()
        };
        assert_eq!(may_request(&inputs), Err(Closed::Stranger));
    }

    #[test]
    fn asleep_no_frame() {
        let inputs = Inputs {
            asleep: Some(true),
            ..clear()
        };
        assert_eq!(may_request(&inputs), Err(Closed::Asleep));
    }

    #[test]
    fn sleep_not_known_no_frame() {
        let inputs = Inputs {
            asleep: None,
            ..clear()
        };
        assert_eq!(may_request(&inputs), Err(Closed::SleepUnknown));
    }
}
