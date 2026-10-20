use lamp_controls::{mute::MuteRecord, receiver::Receiver};
use lamp_msgs::{
    ClockStamp, ControlFault, ControlGesture, ControlIntent, ControlSource, ControlsState, Gesture,
};

fn gesture(intent: ControlIntent, session: &str, sequence: u64, at_us: u64) -> ControlGesture {
    let (source, gesture, duration_us) = match intent {
        ControlIntent::Stop => (ControlSource::Button, Gesture::Tap, 1_000),
        ControlIntent::SoftwareMute => (ControlSource::Button, Gesture::LongPress, 2_000_000),
        ControlIntent::Pet => (ControlSource::Headpad, Gesture::Pet, 50_000),
    };
    ControlGesture {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us,
            sequence,
        },
        session_id: session.into(),
        source,
        name: "primary".into(),
        gesture,
        intent,
        duration_us,
        released_us: at_us,
        enabled: true,
    }
}

fn state(session: &str, sequence: u64, at_us: u64) -> ControlsState {
    ControlsState {
        privacy: None,
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us,
            sequence,
        },
        session_id: session.into(),
        enabled: true,
        ready: true,
        fault: None,
        buttons: vec![],
        head_active: false,
        base_ready: false,
        base_held_us: None,
        base_tier: lamp_msgs::HoldTier::None,
        latest_stop: None,
        latest_mute: None,
        event_sequence: 0,
        rejected_inputs: 0,
        no_privacy_slide: false,
    }
}

fn receiver() -> Receiver {
    Receiver::new(MuteRecord::new("boot").unwrap()).unwrap()
}

#[test]
fn retained_stop_covers_lost_event_and_does_not_repeat_on_heartbeats() {
    let mut receiver = receiver();
    let mut sample = state("one", 1, 3_000_000);
    sample.latest_stop = Some(gesture(ControlIntent::Stop, "one", 1, 2_900_000));
    sample.event_sequence = 1;
    let effect = receiver.observe(3_000_000, &sample).unwrap();
    assert!(effect.stop && effect.stop_requested);
    assert!(!effect.mute_changed && !effect.clear_requested);
    assert!(!receiver.observe(3_000_001, &sample).unwrap().stop);
    sample.stamp.sequence += 1;
    sample.stamp.at_us += 100_000;
    assert!(!receiver.observe(3_100_000, &sample).unwrap().stop);
}

#[test]
fn delayed_stop_expires_but_a_retained_mute_can_still_close_input() {
    let mut receiver = receiver();
    let mut sample = state("one", 1, 4_000_000);
    sample.latest_stop = Some(gesture(ControlIntent::Stop, "one", 1, 3_000_000));
    sample.latest_mute = Some(gesture(ControlIntent::SoftwareMute, "one", 2, 3_100_000));
    sample.event_sequence = 2;
    let effect = receiver.observe(4_000_000, &sample).unwrap();
    assert!(effect.mute_changed && effect.software_muted && effect.stop);
    assert!(!effect.stop_requested && !effect.clear_requested);
    assert_eq!(receiver.mute_record().through_us, 3_100_000);
}

#[test]
fn stop_alone_cannot_clear_the_owners_existing_mute() {
    let mut record = MuteRecord::new("boot").unwrap();
    record.request_mute(2_500_000);
    let mut receiver = Receiver::new(record).unwrap();
    let mut sample = state("one", 1, 3_000_000);
    sample.latest_stop = Some(gesture(ControlIntent::Stop, "one", 1, 3_000_000));
    sample.event_sequence = 1;
    let effect = receiver.observe(3_000_000, &sample).unwrap();
    assert!(effect.stop && effect.stop_requested && effect.software_muted);
    assert!(!effect.mute_changed);
}

#[test]
fn fault_and_restart_never_mean_unmute() {
    let mut record = MuteRecord::new("boot").unwrap();
    record.request_mute(2_500_000);
    let mut receiver = Receiver::new(record).unwrap();
    let mut sample = state("one", 1, 3_000_000);
    sample.ready = false;
    sample.fault = Some(ControlFault::Missing);
    assert!(receiver.observe(3_000_000, &sample).unwrap().software_muted);
    let next = state("two", 1, 3_100_000);
    assert!(receiver.observe(3_100_000, &next).unwrap().software_muted);
}

#[test]
fn unknown_stale_future_wrong_clock_and_shadow_states_cannot_act() {
    for change in 0..5 {
        let mut receiver = receiver();
        let mut sample = state("one", 1, 3_000_000);
        match change {
            0 => sample.enabled = false,
            1 => sample.stamp.at_us = 2_700_000,
            2 => sample.stamp.at_us = 3_000_001,
            3 => sample.stamp.clock_id = "another-boot".into(),
            _ => sample.ready = false,
        }
        assert!(receiver.observe(3_000_000, &sample).is_err());
        assert!(!receiver.mute_record().muted);
    }
}

#[test]
fn the_previous_controls_process_cannot_return_after_a_replacement() {
    let mut receiver = receiver();
    receiver
        .observe(3_000_000, &state("old", 1, 3_000_000))
        .unwrap();
    receiver
        .observe(3_100_000, &state("new", 1, 3_100_000))
        .unwrap();
    assert!(
        receiver
            .observe(3_200_000, &state("old", 2, 3_200_000))
            .is_err()
    );
    assert!(
        receiver
            .observe(3_200_000, &state("new", 2, 3_200_000))
            .is_ok()
    );
}

#[test]
fn older_cross_session_state_cannot_retire_the_current_owner() {
    let mut receiver = receiver();
    receiver
        .observe(3_100_000, &state("current", 1, 3_100_000))
        .unwrap();
    assert!(
        receiver
            .observe(3_100_000, &state("late", 1, 3_000_000))
            .is_err()
    );
    assert!(
        receiver
            .observe(3_200_000, &state("current", 2, 3_200_000))
            .is_ok()
    );
}

#[test]
fn identity_tracking_is_bounded_and_fails_closed_at_exhaustion() {
    let mut receiver = receiver();
    for index in 0..33 {
        receiver
            .observe(
                3_000_000 + index,
                &state(&format!("session{index}"), 1, 3_000_000 + index),
            )
            .unwrap();
    }
    assert!(
        receiver
            .observe(3_100_000, &state("one-more", 1, 3_100_000))
            .is_err()
    );
    assert!(
        receiver
            .observe(3_200_000, &state("session32", 2, 3_200_000))
            .is_err()
    );
}

#[test]
fn clock_reset_does_not_revive_old_requests() {
    let mut receiver = receiver();
    receiver
        .observe(3_000_000, &state("one", 1, 3_000_000))
        .unwrap();
    assert!(
        receiver
            .observe(2_999_999, &state("one", 2, 2_999_999))
            .is_err()
    );
    assert!(
        receiver
            .observe(3_100_000, &state("one", 3, 3_100_000))
            .is_err()
    );
}

#[test]
fn malformed_or_backward_events_do_not_advance_a_valid_watermark() {
    let mut receiver = receiver();
    let mut sample = state("one", 1, 3_000_000);
    sample.event_sequence = 2;
    receiver.observe(3_000_000, &sample).unwrap();
    sample.stamp.sequence = 2;
    sample.stamp.at_us = 3_100_000;
    sample.event_sequence = 1;
    assert!(receiver.observe(3_100_000, &sample).is_err());
    sample.event_sequence = 3;
    sample.latest_stop = Some(gesture(ControlIntent::Stop, "one", 3, 3_100_000));
    assert!(receiver.observe(3_100_000, &sample).unwrap().stop);
}

#[test]
fn pet_needs_current_owner_and_is_deduplicated_without_ever_stopping() {
    let mut receiver = receiver();
    let pet = gesture(ControlIntent::Pet, "one", 1, 3_000_000);
    assert!(receiver.pet(3_000_000, &pet).is_err());
    receiver
        .observe(3_000_000, &state("one", 1, 3_000_000))
        .unwrap();
    assert!(receiver.pet(3_000_000, &pet).unwrap());
    assert!(!receiver.pet(3_000_001, &pet).unwrap());
    assert!(receiver.pet(3_300_000, &pet).is_err());
    assert!(!receiver.mute_record().muted);
}

#[test]
fn failed_explicit_clear_keeps_the_live_inhibit() {
    let mut record = MuteRecord::new("boot").unwrap();
    record.request_mute(2_500_000);
    let mut receiver = Receiver::new(record).unwrap();
    assert!(
        receiver
            .clear_after_explicit_user_request(3_000_000, std::path::Path::new("relative.json"))
            .is_err()
    );
    assert!(receiver.mute_record().muted);
    assert_eq!(receiver.mute_record().through_us, 2_500_000);
}

#[test]
fn delayed_publication_of_a_hold_released_before_clear_cannot_remute() {
    let mut record = MuteRecord::new("boot").unwrap();
    record.request_mute(2_000_000);
    record.clear_after_explicit_user_request(3_000_000).unwrap();
    let mut receiver = Receiver::new(record).unwrap();
    let mut sample = state("one", 1, 3_100_000);
    let mut held = gesture(ControlIntent::SoftwareMute, "one", 1, 3_100_000);
    held.released_us = 2_999_999;
    sample.latest_mute = Some(held);
    sample.event_sequence = 1;
    let effect = receiver.observe(3_100_000, &sample).unwrap();
    assert!(!effect.mute_changed && !effect.software_muted);
}

#[test]
fn explicit_clear_commits_private_state_before_changing_the_live_latch() {
    use std::os::unix::fs::PermissionsExt;
    let directory = std::path::PathBuf::from(format!("/tmp/ct-clear-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.join("mute.json");
    let mut record = MuteRecord::new("boot").unwrap();
    record.request_mute(2_500_000);
    record.save(&path).unwrap();
    let mut receiver = Receiver::new(record).unwrap();
    receiver
        .clear_after_explicit_user_request(3_000_000, &path)
        .unwrap();
    assert!(!receiver.mute_record().muted);
    assert_eq!(
        &MuteRecord::load(&path, "boot", 3_100_000).unwrap(),
        receiver.mute_record()
    );
    std::fs::remove_dir_all(directory).unwrap();
}
