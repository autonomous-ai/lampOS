use super::*;
use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, Controller};

fn authority() -> (Controller, TurnOwner, Snapshot) {
    let now = MonoTime::from_micros(1_000);
    let mut controller = Controller::new(BootId::new([77; 16]).unwrap(), now);
    controller
        .set_microphone_permission(now, Permission::Allowed)
        .unwrap();
    let until = now.checked_add(100_000).unwrap();
    controller
        .set_capture(now, CaptureState::RetainingUntil(until))
        .unwrap();
    controller
        .set_admission(now, AdmissionState::OpenUntil(until))
        .unwrap();
    let owner = controller.admit(now, AdmittedInput::NewTurn).unwrap();
    let snapshot = controller.snapshot(now).unwrap();
    (controller, owner, snapshot)
}

#[test]
fn cut_drops_held_samples_before_ack_and_filters_late_old_chunks() {
    let (_, owner, snapshot) = authority();
    let mut cuts = SpeechCuts::default();
    // This models held device debt through the production drop seam. It is
    // not an ALSA or acoustic latency measurement.
    let mut accepted_unplayed = vec![123_i16; RENDER_SAMPLES * 4];
    let accepted_receipts = accepted_unplayed.len();
    let ack = cuts
        .apply(owner, 7, Some(snapshot), 1_001, || {
            accepted_unplayed.clear();
            Ok(1_002)
        })
        .unwrap();
    assert_eq!(ack, Some(1_002));
    assert!(accepted_unplayed.is_empty());
    assert_eq!(accepted_receipts, RENDER_SAMPLES * 4);
    assert!(cuts.discards(owner, 6)); // a partially written pending chunk
    assert!(cuts.discards(owner, 7)); // later delivery on the data socket
    assert!(!cuts.discards(owner, 8)); // fresh same-request answer
    assert_eq!(
        cuts.apply(owner, 7, Some(snapshot), 1_003, || panic!(
            "duplicate must not stop fresh PCM"
        ))
        .unwrap(),
        ack
    );
    assert_eq!(
        cuts.apply(owner, 6, Some(snapshot), 1_003, || panic!(
            "older cutoff must not stop fresh PCM"
        ))
        .unwrap(),
        ack
    );
    assert_eq!(
        cuts.apply(owner, 9, Some(snapshot), 1_004, || Ok(1_005))
            .unwrap(),
        Some(1_005)
    );
    assert!(cuts.discards(owner, 8));
    assert!(!cuts.discards(owner, 10));
}

#[test]
fn failed_physical_drop_has_no_ack_or_committed_cut() {
    let (_, owner, snapshot) = authority();
    let mut cuts = SpeechCuts::default();
    assert!(
        cuts.apply(owner, 7, Some(snapshot), 1_001, || Err(io::Error::other(
            "drop failed"
        )
        .into()))
            .is_err()
    );
    assert!(!cuts.discards(owner, 7));
    assert_eq!(
        cuts.apply(owner, 7, Some(snapshot), 1_002, || Ok(1_003))
            .unwrap(),
        Some(1_003)
    );
}

#[test]
fn stale_owner_or_authority_cannot_drop_successor_audio() {
    let (mut controller, owner, snapshot) = authority();
    let mut cuts = SpeechCuts::default();
    cuts.apply(owner, 7, Some(snapshot), 1_001, || Ok(1_002))
        .unwrap();
    let now = MonoTime::from_micros(1_003);
    let successor = controller.admit(now, AdmittedInput::NewTurn).unwrap();
    let fresh = controller.snapshot(now).unwrap();
    assert_ne!(owner, successor);
    assert_eq!(
        cuts.apply(owner, 8, Some(fresh), 1_003, || panic!(
            "old owner cannot drop successor"
        ))
        .unwrap(),
        None
    );
    assert!(!cuts.discards(successor, 7));
    assert_eq!(
        cuts.apply(successor, 9, Some(fresh), 1_002, || panic!(
            "future authority"
        ))
        .unwrap(),
        None
    );
    assert_eq!(
        cuts.apply(owner, 8, Some(snapshot), u64::MAX, || panic!(
            "expired authority"
        ))
        .unwrap(),
        None
    );
    controller
        .set_microphone_permission(now, Permission::Denied)
        .unwrap();
    let closed = controller.snapshot(now).unwrap();
    assert_eq!(
        cuts.apply(successor, 9, Some(closed), 1_003, || panic!(
            "privacy closed"
        ))
        .unwrap(),
        None
    );
    assert_eq!(
        cuts.apply(owner, 8, None, 1_003, || panic!("no authority"))
            .unwrap(),
        None
    );
}
