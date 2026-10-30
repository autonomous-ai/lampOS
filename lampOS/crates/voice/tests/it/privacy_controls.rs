use lamp_bus::{Header, Kind, Message};
use lamp_msgs::{ClockStamp, ControlFault, ControlsState, HoldTier, PrivacySwitch};
use lamp_turn_owner::MonoTime;
use lamp_voice::{
    privacy::{PrivacyGate, PrivacyState},
    privacy_bus::{
        NO_SLIDE_FLAG, ROOT_FLAG, Route, SwitchFence, refuse_no_slide_without_mute, selected,
        selected_root, unclocked_line, worker_arguments, worker_route,
    },
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
        no_privacy_slide: false,
    }
}

/// Controls on a lamp with no slide: no switch evidence, and its word that
/// none is wired.
fn marker(at_us: u64, sequence: u64) -> ControlsState {
    ControlsState {
        privacy: None,
        no_privacy_slide: true,
        ..state(at_us, sequence, false)
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
            clock: None,
        },
        payload: serde_json::to_vec(value).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

/// As Controls sends it when its clock is this boot's: the header says the
/// clock its `at_us` is (queue #3997, C).
fn clocked(value: &ControlsState) -> Message {
    let mut message = message(value);
    message.header.clock = Some(value.stamp.clock_id.clone());
    message
}

#[test]
fn a_wall_step_either_way_neither_expires_nor_refuses_a_clocked_controls_state() {
    // Sent at wall 30 000 and read a moment later by `at_us`, across chrony's
    // step of 20 s forward (lamp-06c1, about 46 s into every cold boot) or
    // back.
    for read_at_wall in [30_000 + 20_000, 30_000 - 20_000] {
        for no_slide in [false, true] {
            let value = if no_slide {
                marker(100_000, 1)
            } else {
                state(100_000, 1, false)
            };
            let mut message = clocked(&value);
            message.header.at_unix_ms = 30_000;
            let mut fence = if no_slide {
                SwitchFence::no_slide("boot".into())
            } else {
                SwitchFence::new("boot".into())
            };
            let observed = fence.observe(&message, 100_001, read_at_wall);
            assert!(
                observed.unwrap().is_some(),
                "wall {read_at_wall} no-slide {no_slide}"
            );
            assert_eq!(fence.unclocked(), None);
            // Said by another clock: expired, whatever the wall says.
            message.header.clock = Some("another-boot".into());
            let mut fence = SwitchFence::new("boot".into());
            assert!(fence.observe(&message, 100_001, 30_000).is_err());
        }
    }
    // Without a clock the wall judges as before: both steps are refused.
    for read_at_wall in [30_000 + 20_000, 30_000 - 20_000] {
        let mut message = message(&state(100_000, 1, false));
        message.header.at_unix_ms = 30_000;
        assert!(
            SwitchFence::new("boot".into())
                .accept(&message, 100_001, read_at_wall)
                .is_err()
        );
    }
}

#[test]
fn privacy_ask_c_a_controls_state_without_a_clock_is_taken_by_the_wall_and_said_once() {
    // An update round that mixes builds: a Controls from before it stamped
    // its clock. Visible, not refused.
    let mut fence = SwitchFence::new("boot".into());
    let first = state(100_000, 1, false);
    assert!(
        fence
            .accept(&message(&first), 100_001, 1000)
            .unwrap()
            .is_some()
    );
    let said = unclocked_line(&first.session_id);
    assert!(
        said.contains(&first.session_id) && said.contains("no clock"),
        "{said}"
    );
    assert_eq!(fence.unclocked(), Some(said.as_str()));
    // Once a fence, not with every state.
    let second = state(120_000, 2, true);
    assert!(
        fence
            .accept(&message(&second), 120_001, 1000)
            .unwrap()
            .is_some()
    );
    assert_eq!(fence.unclocked(), Some(said.as_str()));
    // A state that says its clock is said nothing of, on either route.
    let mut fence = SwitchFence::new("boot".into());
    assert!(
        fence
            .accept(&clocked(&first), 100_001, 1000)
            .unwrap()
            .is_some()
    );
    assert_eq!(fence.unclocked(), None);
    let mut fence = SwitchFence::no_slide("boot".into());
    fence
        .observe(&message(&marker(100_000, 1)), 100_001, 1000)
        .unwrap();
    assert!(fence.unclocked().is_some());
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
        worker_arguments(Some((std::path::Path::new("/tmp/test"), Route::Slide))).unwrap(),
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

/// #1569 row 45 (the first builder on #1722, 20 Oct 14:42): before the
/// worker's first datagram it reads Controls' kept value, and a state sent
/// while the publisher had not yet found the worker's socket reaches only
/// that file. What it reads until then, the first datagram included, is a
/// resync: a gap is not a discontinuity. From the second datagram on, a gap
/// fails closed, and a resync never takes the order back, another
/// publisher or a restarted one.
#[test]
fn up_to_the_first_datagram_a_fence_resyncs_and_after_it_a_gap_fails_closed() {
    let mut fence = SwitchFence::new("boot".into());
    // The kept value, then the next kept value with one sent in between.
    fence.resync();
    fence
        .accept(&message(&state(100_000, 3, false)), 100_000, 1000)
        .unwrap();
    let mut skipped = message(&state(110_000, 5, false));
    skipped.gap = 1;
    fence.resync();
    fence.accept(&skipped, 110_000, 1000).unwrap();
    // The first datagram, past another one only the file had.
    let mut first = message(&state(120_000, 7, false));
    first.gap = 1;
    fence.resync();
    fence.accept(&first, 120_000, 1000).unwrap();
    // From the second datagram on: in order is taken, a gap fails closed.
    fence
        .accept(&message(&state(130_000, 8, false)), 130_000, 1000)
        .unwrap();
    let mut later = message(&state(150_000, 10, false));
    later.gap = 1;
    assert!(fence.accept(&later, 150_000, 1000).is_err());
    // Without a resync asked, a first message's gap fails closed, as ever.
    let mut fresh = message(&state(100_000, 3, false));
    fresh.gap = 1;
    assert!(
        SwitchFence::new("boot".into())
            .accept(&fresh, 100_000, 1000)
            .is_err()
    );
    // A resync goes forward only, from the same publisher, not restarted.
    for mutation in 0..3 {
        let mut fence = SwitchFence::new("boot".into());
        fence.resync();
        fence
            .accept(&message(&state(100_000, 3, false)), 100_000, 1000)
            .unwrap();
        let mut next = state(110_000, 5, false);
        if mutation == 1 {
            next.session_id = "fedcba9876543210".into();
        }
        let mut next = message(&next);
        next.gap = 1;
        match mutation {
            0 => next.header.seq = 3,
            2 => next.publisher_restarted = true,
            _ => {}
        }
        fence.resync();
        assert!(
            fence.accept(&next, 110_000, 1000).is_err(),
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

/// `no-slide` selects the route for a lamp with no slide; any other value
/// but `on` is off, as builds before it read `no-slide`.
#[test]
fn no_slide_selects_the_route_and_reaches_the_worker_only_as_an_argument() {
    for switch in [
        None,
        Some("off"),
        Some("NO-SLIDE"),
        Some("no_slide"),
        Some(""),
    ] {
        assert_eq!(Route::from_value(switch), None, "{switch:?}");
        assert!(selected(switch, None).unwrap().is_none());
    }
    assert_eq!(Route::from_value(Some("on")), Some(Route::Slide));
    assert_eq!(Route::from_value(Some("no-slide")), Some(Route::NoSlide));
    let (root, route) = selected(Some("no-slide"), None).unwrap().unwrap();
    assert_eq!(
        (root.to_str(), route),
        (Some("/run/lampos-bus"), Route::NoSlide)
    );
    assert_eq!(
        selected_root(Some("no-slide"), Some("/tmp/b"))
            .unwrap()
            .unwrap()
            .to_str(),
        Some("/tmp/b")
    );
    assert!(selected(Some("no-slide"), Some("relative")).is_err());
    // The worker's environment is cleared: the route is its argument, and
    // the worker reads back exactly what it was given.
    let root = std::path::Path::new("/tmp/test");
    let arguments = worker_arguments(Some((root, Route::NoSlide))).unwrap();
    assert_eq!(arguments, [ROOT_FLAG, "/tmp/test", NO_SLIDE_FLAG]);
    assert_eq!(
        worker_route(&arguments).unwrap(),
        (root.into(), Route::NoSlide, false)
    );
    let arguments = worker_arguments(Some((root, Route::Slide))).unwrap();
    assert_eq!(
        worker_route(&arguments).unwrap(),
        (root.into(), Route::Slide, false)
    );
    for wrong in [
        vec![ROOT_FLAG, "relative"],
        vec![ROOT_FLAG, "/tmp/test", "--slide"],
        vec![ROOT_FLAG, "/tmp/test", NO_SLIDE_FLAG, NO_SLIDE_FLAG],
        vec![NO_SLIDE_FLAG, ROOT_FLAG, "/tmp/test"],
        vec![ROOT_FLAG],
    ] {
        let wrong: Vec<String> = wrong.into_iter().map(String::from).collect();
        assert!(worker_route(&wrong).is_err(), "{wrong:?}");
    }
}

#[test]
fn only_an_enabled_controls_conversation_passes_the_worker_its_gap_ride() {
    use lamp_voice::privacy_bus::{GAP_RIDE_FLAG, conversation_arguments};
    let root = std::path::Path::new("/tmp/test");
    for route in [Route::Slide, Route::NoSlide] {
        let selected = Some((root, route));
        let off = worker_arguments(selected).unwrap();
        assert_eq!(conversation_arguments(selected, false).unwrap(), off);
        let on = conversation_arguments(selected, true).unwrap();
        assert_eq!(on.last().unwrap(), GAP_RIDE_FLAG);
        assert_eq!(worker_route(&on).unwrap(), (root.into(), route, true));
        let mut duplicated = on;
        duplicated.push(GAP_RIDE_FLAG.into());
        assert!(worker_route(&duplicated).is_err());
    }
    assert!(conversation_arguments(None, true).unwrap().is_empty());
}

/// With no slide the software mute alone closes the microphones: without
/// its persisted record a restart would forget a mute, so the voice refuses.
#[test]
fn no_slide_without_the_persisted_mute_refuses_to_start() {
    let error = refuse_no_slide_without_mute(Some(Route::NoSlide), false).unwrap_err();
    assert!(error.to_string().contains("--controls-mute"), "{error}");
    refuse_no_slide_without_mute(Some(Route::NoSlide), true).unwrap();
    for route in [None, Some(Route::Slide)] {
        refuse_no_slide_without_mute(route, false).unwrap();
    }
}

/// Each fresh publication of the marker is an observation, read as not
/// muted: the lease is permitted by an Allowed controller after the usual
/// 60 ms of steady evidence, and never by a Denied one (the software mute).
#[test]
fn with_no_slide_a_fresh_marker_permits_only_with_an_allowed_controller() {
    use lamp_turn_owner::{BootId, Controller, Permission};
    use lamp_voice::privacy_bus::{CaptureLeaseGate, Observation, fresh, grants};
    let at = MonoTime::from_micros;
    for permission in [Permission::Allowed, Permission::Denied] {
        let mut controller = Controller::new(BootId::new([1; 16]).unwrap(), at(100_000));
        controller
            .set_microphone_permission(at(100_000), permission)
            .unwrap();
        let mut fence = SwitchFence::no_slide("boot".into());
        let mut gate = CaptureLeaseGate::default();
        for (i, now) in [100_000, 120_000, 140_000, 160_000].into_iter().enumerate() {
            let observed = fence
                .observe(&message(&marker(now, i as u64 + 1)), now + 1, 1000)
                .unwrap()
                .unwrap();
            assert_eq!(observed.physical_muted(), None);
            assert!(!observed.muted().unwrap());
            let Observation::NoSlide(stamp) = &observed else {
                panic!("a no-slide fence gave {observed:?}");
            };
            assert_eq!((stamp.at_us, stamp.sequence), (now, i as u64 + 1));
            gate.observe(stamp.sequence, stamp.at_us, now + 1, false);
            let snapshot = controller.snapshot(at(now + 1)).unwrap();
            let granted = now == 160_000 && permission == Permission::Allowed;
            assert_eq!(gate.permitted(snapshot, now + 1), granted);
            let observed = fresh(Some(&observed), now + 1);
            assert_eq!(
                grants(&gate, snapshot, observed, true, now + 1),
                granted,
                "{permission:?} {now}"
            );
            // An undrained queue or a stale observation grants nothing.
            assert!(!grants(&gate, snapshot, observed, false, now + 1));
            assert!(fresh(observed, now + 50_000).is_none());
        }
    }
}

/// A stale or future publication, a skipped one, a publisher restart or a
/// changed session, slide evidence, or a state that does not say no slide
/// is wired (a missing privacy section is never read as no slide): each
/// ends a no-slide worker, which says the switch is unreadable.
#[test]
fn with_no_slide_stale_restarted_or_unmarked_states_are_refused() {
    for mutation in 0..9 {
        let mut fence = SwitchFence::no_slide("boot".into());
        fence
            .accept_no_slide(&message(&marker(100_000, 1)), 100_000, 1000)
            .unwrap();
        let mut next = marker(120_000, 2);
        let mut now = 120_000;
        match mutation {
            0 => now = 170_000,
            1 => now = 119_999,
            2 => next.session_id = "fedcba9876543210".into(),
            // Slide evidence beside the marker, which its own check refuses.
            3 => next.privacy = state(120_000, 2, false).privacy,
            // Neither marker nor evidence: a missing section is not no slide.
            4 => next.no_privacy_slide = false,
            5 => next.stamp.sequence = 1,
            6 => next.stamp.at_us = 99_999,
            _ => {}
        }
        let mut envelope = message(&next);
        match mutation {
            7 => envelope.publisher_restarted = true,
            8 => envelope.header.seq = 3,
            _ => {}
        }
        assert!(
            fence.accept_no_slide(&envelope, now, 1000).is_err(),
            "mutation {mutation}"
        );
    }
    // The first publication must already be fresh and marked.
    let mut fence = SwitchFence::no_slide("boot".into());
    assert!(
        fence
            .accept_no_slide(&message(&marker(100_000, 1)), 150_000, 1000)
            .is_err()
    );
    assert!(
        fence
            .accept_no_slide(&message(&state(100_000, 1, false)), 100_000, 1000)
            .is_err()
    );
}

/// `on` reads the slide: a marker-only state fails closed as a state
/// without slide evidence always has. Neither fence reads the other kind.
#[test]
fn a_fence_never_reads_the_other_kind_of_state() {
    let mut slide = SwitchFence::new("boot".into());
    assert!(
        slide
            .accept(&message(&marker(100_000, 1)), 100_000, 1000)
            .is_err()
    );
    assert!(
        SwitchFence::new("boot".into())
            .accept_no_slide(&message(&marker(100_000, 1)), 100_000, 1000)
            .is_err()
    );
    assert!(
        SwitchFence::no_slide("boot".into())
            .accept(&message(&state(100_000, 1, false)), 100_000, 1000)
            .is_err()
    );
    assert!(
        SwitchFence::for_route("boot".into(), Route::Slide)
            .observe(&message(&marker(100_000, 1)), 100_000, 1000)
            .is_err()
    );
    assert!(
        SwitchFence::for_route("boot".into(), Route::NoSlide)
            .observe(&message(&state(100_000, 1, false)), 100_000, 1000)
            .is_err()
    );
}
