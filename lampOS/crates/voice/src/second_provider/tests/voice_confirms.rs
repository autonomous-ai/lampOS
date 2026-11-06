//! `LAMP_SAME_VOICE_CONFIRMS` (the memory owner's decision of 4 Nov: CAM++
//! one way only): the known person's own voice confirms their line as
//! seen said by them while their face is the one face in view, and a score
//! under the bar does nothing.
use super::*;
use crate::same_voice::{CONFIRM_BAR, OWNER_LEAST_PRINTS, OWNER_PRINTS, OwnerVoice};

const FACE: &str = "b1/f3";
const PERSON: &str = "person_owner";

/// Their own voice, another 0.95 like it, and one 0.20 like it.
const OWN: [f32; 3] = [1.0, 0.0, 0.0];
const NEAR: [f32; 3] = [0.95, 0.312, 0.0];
const FAR: [f32; 3] = [0.2, 0.98, 0.0];

/// A confirm at `bar` whose every line's print is `print`.
fn confirms(bar: f32, print: Option<[f32; 3]>) -> VoiceConfirms {
    VoiceConfirms {
        owner: Arc::new(OwnerVoice::new(bar)),
        print_of: Arc::new(move |_| print.map(|print| print.to_vec())),
        setup: None,
    }
}

/// `confirms` taught their voice by `OWNER_LEAST_PRINTS` lines their face
/// alone was seen speaking, the fewest any line is judged against.
fn taught(confirms: &VoiceConfirms) {
    for request in 101..101 + OWNER_LEAST_PRINTS as u64 {
        assert!(confirms.owner.learn(PERSON, request, OWN.to_vec()));
    }
}

fn confirm(
    confirms: Option<&VoiceConfirms>,
    request: u64,
    speakers: &[&str],
    in_view: bool,
) -> Vec<String> {
    confirmed_by_voice(
        confirms,
        request,
        PERSON,
        speakers.iter().map(|id| (*id).to_owned()).collect(),
        Some(FACE),
        |face| in_view && face == FACE,
    )
}

/// The known file's name for them, and their face.
fn known() -> Option<(String, String)> {
    Some(("Linh".to_owned(), FACE.to_owned()))
}

/// A high match, their face the one face in view: their face is seen
/// saying it, so every "seen saying" check reads it as theirs.
#[test]
fn a_high_match_with_their_face_in_view_is_seen_said_by_them() {
    let confirms = confirms(CONFIRM_BAR, Some(NEAR));
    taught(&confirms);
    let speakers = confirm(Some(&confirms), 7, &[], true);
    assert_eq!(speakers, vec![FACE.to_owned()]);
    // Read as the scope reads it, for the known file's person.
    let person = crate::known_faces::memory_person("Linh");
    assert!(crate::speaking_faces::seen_saying(
        &speakers,
        &person,
        known()
    ));
    assert!(!crate::speaking_faces::not_theirs(
        &speakers,
        &person,
        known(),
        true
    ));
    // Unconfirmed, the same line is not theirs with the scope seen.
    assert!(crate::speaking_faces::not_theirs(
        &[],
        &person,
        known(),
        true
    ));
}

/// A high match with no face in view, or another face in it, names nobody.
#[test]
fn a_high_match_with_no_face_in_view_does_nothing() {
    let confirms = confirms(CONFIRM_BAR, Some(NEAR));
    taught(&confirms);
    assert!(confirm(Some(&confirms), 7, &[], false).is_empty());
    // No face of theirs known: nothing either.
    assert!(confirmed_by_voice(Some(&confirms), 7, PERSON, Vec::new(), None, |_| true).is_empty());
    // Another face seen speaking it stays another face.
    assert_eq!(
        confirm(Some(&confirms), 7, &["b1/f9"], true),
        vec!["b1/f9".to_owned()]
    );
}

/// A score under the bar changes nothing: the speakers come back as they
/// went in, so the line stays as it was without the switch, never held or
/// ended by it (the voices judge nothing: `same_voice`'s own tests).
#[test]
fn a_low_match_does_nothing() {
    let confirms = confirms(CONFIRM_BAR, Some(FAR));
    taught(&confirms);
    assert!(confirm(Some(&confirms), 7, &[], true).is_empty());
    // No print, or none of theirs kept yet: nothing.
    let unprinted = self::confirms(CONFIRM_BAR, None);
    taught(&unprinted);
    assert!(confirm(Some(&unprinted), 7, &[], true).is_empty());
    let untaught = self::confirms(CONFIRM_BAR, Some(OWN));
    assert!(confirm(Some(&untaught), 7, &[], true).is_empty());
}

/// The switch off: the speakers are returned untouched, whatever they are.
#[test]
fn the_switch_off_changes_nothing() {
    for speakers in [&[][..], &[FACE][..], &["b1/f9"][..], &[FACE, "b1/f9"][..]] {
        for in_view in [false, true] {
            let expected: Vec<String> = speakers.iter().map(|id| (*id).to_owned()).collect();
            assert_eq!(confirm(None, 7, speakers, in_view), expected);
        }
    }
}

/// The bar is the one given, not a fixed one: 0.95 like theirs passes a
/// bar of 0.9 and fails one of 0.96.
#[test]
fn the_bar_given_is_the_bar() {
    let low = confirms(0.9, Some(NEAR));
    taught(&low);
    assert_eq!(confirm(Some(&low), 7, &[], true), vec![FACE.to_owned()]);
    let high = confirms(0.96, Some(NEAR));
    taught(&high);
    assert!(confirm(Some(&high), 7, &[], true).is_empty());
}

/// Their voice is learnt only from lines their face alone was seen
/// speaking, their face the one face in view, at most `OWNER_PRINTS`, and
/// never from a line the voice itself confirmed.
#[test]
fn their_voice_is_learnt_only_from_lines_seen_spoken_by_them_alone() {
    let confirms = confirms(CONFIRM_BAR, Some(OWN));
    // A face seen speaking it, but another face in view: not learnt.
    confirm(Some(&confirms), 1, &[FACE], false);
    assert_eq!(confirms.owner.known(PERSON), 0);
    // Two faces seen speaking: not learnt.
    confirm(Some(&confirms), 2, &[FACE, "b1/f9"], true);
    assert_eq!(confirms.owner.known(PERSON), 0);
    for request in 3..3 + OWNER_PRINTS as u64 + 2 {
        confirm(Some(&confirms), request, &[FACE], true);
    }
    assert_eq!(confirms.owner.known(PERSON), OWNER_PRINTS);
    // A confirmed line is not learnt from.
    let fresh = self::confirms(CONFIRM_BAR, Some(OWN));
    // Fewer than `OWNER_LEAST_PRINTS` kept: their own voice confirms nothing.
    for request in 1..OWNER_LEAST_PRINTS as u64 {
        assert!(fresh.owner.learn(PERSON, request, OWN.to_vec()));
    }
    assert!(confirm(Some(&fresh), 50, &[], true).is_empty());
    assert!(fresh.owner.learn(PERSON, 99, OWN.to_vec()));
    assert_eq!(confirm(Some(&fresh), 51, &[], true), vec![FACE.to_owned()]);
    assert_eq!(fresh.owner.known(PERSON), OWNER_LEAST_PRINTS);
}
