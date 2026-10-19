use lamp_bus::{Header, Kind, Message};
use lamp_msgs::{ClockStamp, ControlFault, ControlsState, HoldTier, PrivacySwitch};
use lamp_turn_owner::MonoTime;
use lamp_voice::{
    privacy::{PrivacyGate, PrivacyState},
    privacy_bus::{SwitchFence, selected_root, worker_arguments},
};

fn state(at_us: u64, sequence: u64, muted: bool) -> ControlsState {
    ControlsState {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us,
            sequence,
        },
        session_id: "0123456789abcdef".into(),
        enabled: false,
        ready: false,
        fault: Some(ControlFault::Missing),
        buttons: vec![],
        head_active: false,
        base_ready: false,
        base_held_us: None,
        base_tier: HoldTier::None,
        latest_stop: None,
        latest_mute: None,
        event_sequence: 0,
        rejected_inputs: 0,
        privacy: Some(PrivacySwitch {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us,
                sequence,
            },
            reader_session_id: "slide".into(),
            muted: Some(muted),
            fault: None,
        }),
    }
}

fn message(value: &ControlsState) -> Message {
    Message {
        header: Header {
            topic: ControlsState::TOPIC.into(),
            kind: Kind::Latest,
            enc: lamp_bus::JSON.into(),
            type_name: Some(ControlsState::TYPE.into()),
            source: "controls".into(),
            boot: value.session_id.clone(),
            seq: value.stamp.sequence,
            at_us: value.stamp.at_us,
            at_unix_ms: 1000,
            expires_ms: Some(300),
        },
        payload: serde_json::to_vec(value).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

#[test]
fn off_selects_original_worker_and_touches_no_path() {
    for switch in [None, Some("off"), Some("true"), Some("")] {
        assert!(
            selected_root(switch, Some("invalid relative root"))
                .unwrap()
                .is_none()
        );
        assert!(worker_arguments(None).unwrap().is_empty());
    }
    assert_eq!(
        selected_root(Some("on"), None).unwrap().unwrap().to_str(),
        Some("/run/lampos-bus")
    );
    assert!(selected_root(Some("on"), Some("relative")).is_err());
    assert_eq!(
        worker_arguments(Some(std::path::Path::new("/tmp/test"))).unwrap(),
        ["--controls-root", "/tmp/test"]
    );
}

#[test]
fn startup_low_and_faulted_hand_source_are_independent() {
    let value = state(100_000, 1, true);
    let mut fence = SwitchFence::new("boot".into());
    let observed = fence
        .accept(&message(&value), 100_001, 1000)
        .unwrap()
        .unwrap();
    assert_eq!(observed.muted, Some(true));
    let mut gate = PrivacyGate::default();
    gate.observe(
        observed.stamp.sequence,
        MonoTime::from_micros(observed.stamp.at_us),
        MonoTime::from_micros(100_001),
        true,
    );
    assert_eq!(
        gate.state(MonoTime::from_micros(100_001)),
        PrivacyState::Muted
    );
}

#[test]
fn repeated_publication_does_not_refresh_or_rename_acquisition() {
    let first = state(100_000, 1, false);
    let mut fence = SwitchFence::new("boot".into());
    assert!(
        fence
            .accept(&message(&first), 100_000, 1000)
            .unwrap()
            .is_some()
    );
    let mut repeat = first.clone();
    repeat.stamp.at_us = 120_000;
    repeat.stamp.sequence = 2;
    assert!(
        fence
            .accept(&message(&repeat), 120_000, 1000)
            .unwrap()
            .is_none()
    );
    repeat.stamp.at_us = 150_000;
    repeat.stamp.sequence = 3;
    assert!(fence.accept(&message(&repeat), 150_000, 1000).is_err());
    let mut changed = first.clone();
    changed.stamp.at_us = 120_000;
    changed.stamp.sequence = 2;
    changed.privacy.as_mut().unwrap().muted = Some(true);
    let mut fence = SwitchFence::new("boot".into());
    fence.accept(&message(&first), 100_000, 1000).unwrap();
    assert!(fence.accept(&message(&changed), 120_000, 1000).is_err());
}

#[test]
fn rapid_low_high_must_complete_new_stable_open_interval() {
    let mut fence = SwitchFence::new("boot".into());
    let mut gate = PrivacyGate::default();
    for (i, (at_us, muted)) in [
        (100_000, false),
        (120_000, false),
        (140_000, false),
        (160_000, false),
        (161_000, true),
        (162_000, false),
        (182_000, false),
        (202_000, false),
        (222_000, false),
    ]
    .into_iter()
    .enumerate()
    {
        let value = state(at_us, i as u64 + 1, muted);
        let observed = fence
            .accept(&message(&value), at_us, 1000)
            .unwrap()
            .unwrap();
        gate.observe(
            observed.stamp.sequence,
            MonoTime::from_micros(observed.stamp.at_us),
            MonoTime::from_micros(at_us),
            observed.muted.unwrap(),
        );
        assert_eq!(
            gate.allowed(MonoTime::from_micros(at_us)),
            matches!(at_us, 160_000 | 222_000)
        );
    }
}

#[test]
fn malformed_foreign_missing_future_stale_and_discontinuous_messages_are_rejected() {
    for mutation in 0..16 {
        let mut value = state(100_000, 1, false);
        match mutation {
            0 => value.privacy = None,
            1 => value.privacy.as_mut().unwrap().stamp.clock_id = "other".into(),
            2 => value.privacy.as_mut().unwrap().stamp.at_us = 100_001,
            3 => value.privacy.as_mut().unwrap().stamp.at_us = 50_000,
            4 => {
                let privacy = value.privacy.as_mut().unwrap();
                privacy.muted = None;
                privacy.fault = Some(ControlFault::Missing);
            }
            5 => value.stamp.clock_id = "other".into(),
            _ => {}
        }
        let mut message = message(&value);
        match mutation {
            6 => message.header.source = "voice".into(),
            7 => message.header.type_name = None,
            8 => message.header.expires_ms = Some(500),
            9 => message.header.boot = "fedcba9876543210".into(),
            10 => message.header.at_us = 90_000,
            11 => message.header.seq = 0,
            12 => message.gap = 1,
            13 => message.publisher_restarted = true,
            14 => message.expired = true,
            15 => message.header.at_unix_ms = 100_000,
            _ => {}
        }
        assert!(
            SwitchFence::new("boot".into())
                .accept(&message, 100_000, 1000)
                .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn publisher_and_reader_restart_cannot_rebind_an_active_worker() {
    for mutation in 0..6 {
        let first = state(100_000, 1, false);
        let mut fence = SwitchFence::new("boot".into());
        fence.accept(&message(&first), 100_000, 1000).unwrap();
        let mut next = state(110_000, 2, false);
        match mutation {
            0 => next.session_id = "fedcba9876543210".into(),
            1 => next.privacy.as_mut().unwrap().reader_session_id = "replacement".into(),
            2 => next.privacy.as_mut().unwrap().stamp.sequence = 0,
            3 => next.privacy.as_mut().unwrap().stamp.at_us = 99_999,
            4 => next.stamp.sequence = 1,
            _ => next.stamp.at_us = 99_999,
        }
        assert!(
            fence.accept(&message(&next), 110_000, 1000).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn low_retires_capture_lease_even_after_slow_drain_with_old_allowed_controller() {
    use lamp_turn_owner::{BootId, Controller, Permission};
    use lamp_voice::privacy_bus::CaptureLeaseGate;
    let at = MonoTime::from_micros;
    let mut controller = Controller::new(BootId::new([1; 16]).unwrap(), at(100_000));
    controller
        .set_microphone_permission(at(100_000), Permission::Allowed)
        .unwrap();
    let allowed = controller.snapshot(at(100_000)).unwrap();
    let mut gate = CaptureLeaseGate::default();
    for (sequence, now) in [100_000, 120_000, 140_000, 160_000].into_iter().enumerate() {
        gate.observe(sequence as u64 + 1, now, now, false);
    }
    assert!(gate.permitted(allowed, 160_000));
    gate.observe(5, 161_000, 161_000, true);
    assert!(!gate.permitted(allowed, 161_000));
    // A long but continuously fresh drain observes LOW then stable HIGH before
    // the owner can acknowledge denial. Old Allowed must never authorize it.
    for (sequence, now) in [162_000, 182_000, 202_000, 222_000].into_iter().enumerate() {
        gate.observe(sequence as u64 + 6, now, now, false);
        assert!(!gate.permitted(allowed, now));
    }
    let mut next = CaptureLeaseGate::default();
    for (sequence, now) in [230_000, 250_000, 270_000, 290_000].into_iter().enumerate() {
        next.observe(sequence as u64 + 1, now, now, false);
    }
    let allowed = controller.snapshot(at(290_000)).unwrap();
    assert!(next.permitted(allowed, 290_000));
    controller
        .set_microphone_permission(at(290_001), Permission::Denied)
        .unwrap();
    assert!(!next.permitted(controller.snapshot(at(290_001)).unwrap(), 290_001));
}
