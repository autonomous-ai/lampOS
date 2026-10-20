use lamp_controls::touch_receiver::TouchReceiver;
use lamp_msgs::*;

fn event(sequence: u64, began: u64, at: u64) -> TouchEvent {
    TouchEvent {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: at,
            sequence,
        },
        session_id: "controls".into(),
        reader_session_id: "reader".into(),
        enabled: true,
        source: TouchSource::Strip,
        name: "strip".into(),
        contact_id: sequence,
        gesture: TouchGesture::Touch,
        when_speaking: TouchIntent::Stop,
        when_quiet: TouchIntent::Attend,
        when_asleep: Some(TouchIntent::Wake),
        began_us: began,
        observed_us: at,
        released_us: None,
        active_mask: 7,
        active_names: Vec::new(),
        position: Some(1.0 / 11.0),
        toward_pad: None,
        distance: 0.0,
        speed: 0.0,
    }
}
fn sleep(sequence: u64, began: u64, at: u64) -> TouchEvent {
    TouchEvent {
        gesture: TouchGesture::Hold,
        when_speaking: TouchIntent::Sleep,
        when_quiet: TouchIntent::Sleep,
        ..event(sequence, began, at)
    }
}
fn state(at: u64, sequence: u64, cumulative: u64, latest: Vec<TouchEvent>) -> TouchState {
    TouchState {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: at,
            sequence,
        },
        session_id: "controls".into(),
        enabled: true,
        sources: vec![TouchSourceState {
            source: TouchSource::Strip,
            name: "strip".into(),
            reader_session_id: Some("reader".into()),
            ready: true,
            fault: None,
            observed_us: Some(at),
            armed: true,
            active: Some(false),
            contact_id: None,
            began_us: None,
            held_us: None,
            active_mask: Some(0),
            active_names: Vec::new(),
            position: None,
            toward_pad: None,
            distance: 0.0,
            speed: 0.0,
        }],
        latest,
        event_sequence: cumulative,
        rejected_inputs: 0,
    }
}
fn receiver() -> TouchReceiver {
    let mut value = TouchReceiver::new("boot").unwrap();
    assert!(
        value
            .observe(0, &state(0, 1, 0, Vec::new()))
            .unwrap()
            .is_empty()
    );
    value
}
fn heartbeat(value: &mut TouchReceiver, through: u64, retained: Vec<TouchEvent>) -> u64 {
    let mut sequence = 2;
    for at in (100_000..=through).step_by(100_000) {
        let cumulative = retained.iter().map(|e| e.stamp.sequence).max().unwrap_or(0);
        value
            .observe(at, &state(at, sequence, cumulative, retained.clone()))
            .unwrap();
        sequence += 1;
    }
    sequence
}

#[test]
fn retained_touch_sleep_and_reset_do_not_replay_on_subscribe() {
    for e in [event(1, 10, 20_010), sleep(1, 10, 2_000_010)] {
        let mut r = TouchReceiver::new("boot").unwrap();
        let at = e.observed_us;
        assert!(
            r.observe(at, &state(at, 1, 1, vec![e.clone()]))
                .unwrap()
                .is_empty()
        );
        assert!(r.event(at, &e).unwrap().is_none());
    }
    let mut e = event(1, 10, 5_000_010);
    e.source = TouchSource::Reset;
    e.name = "reset100".into();
    e.gesture = TouchGesture::Hold;
    e.when_speaking = TouchIntent::FactoryReset;
    e.when_quiet = TouchIntent::FactoryReset;
    e.when_asleep = Some(TouchIntent::FactoryReset);
    e.released_us = Some(e.observed_us);
    e.active_mask = 0;
    e.position = None;
    let mut s = state(e.observed_us, 1, 1, vec![e.clone()]);
    s.sources[0].source = TouchSource::Reset;
    s.sources[0].name = "reset100".into();
    let mut r = TouchReceiver::new("boot").unwrap();
    assert!(r.observe(e.observed_us, &s).unwrap().is_empty());
    assert!(r.event(e.observed_us, &e).unwrap().is_none());
}
#[test]
fn state_then_event_deduplicates_exact_identity_without_replaying() {
    let mut r = receiver();
    let e = event(1, 10_000, 30_000);
    let s = state(30_000, 2, 1, vec![e.clone()]);
    assert_eq!(r.observe(30_000, &s).unwrap(), std::slice::from_ref(&e));
    assert!(r.event(30_000, &e).unwrap().is_none());
    assert!(r.observe(30_000, &s).unwrap().is_empty());
    let mut next = s;
    next.stamp.sequence += 1;
    next.stamp.at_us += 1;
    next.sources[0].observed_us = Some(30_001);
    assert!(r.observe(30_001, &next).unwrap().is_empty());
}
#[test]
fn advisory_event_is_not_lost_to_state_cumulative_watermark() {
    let mut r = receiver();
    let mut e = event(2, 10_000, 30_000);
    e.gesture = TouchGesture::Slide;
    e.when_speaking = TouchIntent::Slide;
    e.when_quiet = TouchIntent::Slide;
    e.when_asleep = None;
    e.toward_pad = Some(11);
    e.distance = 0.2;
    e.speed = 1.0;
    r.observe(30_000, &state(30_000, 2, 2, Vec::new())).unwrap();
    assert_eq!(r.event(30_000, &e).unwrap(), Some(e.clone()));
    assert!(r.event(30_000, &e).unwrap().is_none());
}
#[test]
fn lifecycle_contact_binding_keeps_waking_hold_from_sleeping_again() {
    let touch = event(1, 10, 20_010);
    let held = sleep(2, 10, 2_000_010);
    assert_eq!(touch.intent(false, true), Some(TouchIntent::Wake));
    // The owner preserves asleep-at-contact-start after the first Wake.
    assert_eq!(held.intent(false, true), Some(TouchIntent::Wake));
    assert_eq!(held.intent(false, false), Some(TouchIntent::Sleep));
    assert_eq!(held.intent(true, false), Some(TouchIntent::Sleep));
}
#[test]
fn whole_new_hold_qualifies_after_bootstrap_and_is_only_delivered_once() {
    let mut r = receiver();
    let sequence = heartbeat(&mut r, 2_000_000, Vec::new());
    let e = sleep(1, 10, 2_000_010);
    let s = state(e.observed_us, sequence, 1, vec![e.clone()]);
    assert_eq!(
        r.observe(e.observed_us, &s).unwrap(),
        std::slice::from_ref(&e)
    );
    assert!(r.event(e.observed_us, &e).unwrap().is_none());
}
#[test]
fn stale_retained_hold_does_not_keep_advancing_new_contact_fence() {
    let mut r = receiver();
    let mut sequence = heartbeat(&mut r, 2_000_000, Vec::new());
    let old = sleep(1, 10, 2_000_010);
    r.observe(2_000_010, &state(2_000_010, sequence, 1, vec![old.clone()]))
        .unwrap();
    sequence += 1;
    for at in (2_100_000..=4_100_000).step_by(100_000) {
        assert!(
            r.observe(at, &state(at, sequence, 1, vec![old.clone()]))
                .unwrap()
                .is_empty()
        );
        sequence += 1;
    }
    let new = sleep(2, 2_100_001, 4_100_001);
    assert_eq!(
        r.observe(4_100_001, &state(4_100_001, sequence, 2, vec![new.clone()]))
            .unwrap(),
        [new]
    );
}
#[test]
fn original_event_acquisition_expires_even_with_fresh_state_and_republication() {
    let mut r = receiver();
    let mut e = event(1, 10_000, 30_000);
    e.stamp.at_us = 130_000;
    assert!(
        r.observe(130_000, &state(130_000, 2, 1, vec![e.clone()]))
            .unwrap()
            .is_empty()
    );
    assert!(r.event(130_000, &e).is_err());
}
#[test]
fn stale_current_acquisition_cannot_be_renewed_by_fresh_state_heartbeat() {
    let mut r = receiver();
    let e = event(1, 10_000, 30_000);
    let mut s = state(99_999, 2, 1, vec![e.clone()]);
    s.sources[0].observed_us = Some(30_000);
    s.check().unwrap();
    assert!(r.observe(130_000, &s).unwrap().is_empty());
    assert!(r.event(130_000, &e).is_err());
}
#[test]
fn event_before_qualifying_state_is_refused_without_consuming_it() {
    let mut r = receiver();
    let e = event(1, 10_000, 30_000);
    assert!(r.event(30_000, &e).is_err());
    assert_eq!(
        r.observe(30_000, &state(30_000, 2, 1, vec![e.clone()]))
            .unwrap(),
        [e]
    );
}
#[test]
fn out_of_order_state_cannot_erase_high_water_even_after_refusal() {
    let mut r = receiver();
    let e = event(1, 10_000, 30_000);
    let valid = state(30_000, 3, 1, vec![e]);
    r.observe(30_000, &valid).unwrap();
    assert!(r.observe(30_001, &state(20_000, 2, 0, Vec::new())).is_err());
    assert!(r.observe(30_002, &state(25_000, 2, 0, Vec::new())).is_err());
    assert!(r.event(30_002, &event(2, 10_000, 30_000)).is_err());
}
#[test]
fn repeated_state_identity_with_changed_content_is_refused() {
    let mut r = receiver();
    let mut s = state(0, 1, 0, Vec::new());
    s.rejected_inputs = 1;
    assert!(r.observe(0, &s).is_err());
}
#[test]
fn wrong_clock_and_disabled_cannot_establish_owner() {
    for wrong_clock in [false, true] {
        let mut r = TouchReceiver::new("boot").unwrap();
        let mut bad = state(0, 1, 0, Vec::new());
        if wrong_clock {
            bad.stamp.clock_id = "other".into();
        } else {
            bad.enabled = false;
        }
        assert!(r.observe(0, &bad).is_err());
        assert!(r.observe(1, &state(1, 1, 0, Vec::new())).is_ok());
    }
}
#[test]
fn changed_controls_session_retires_old_payload_and_resets_lifecycle_baseline() {
    let mut r = receiver();
    let mut fresh = state(10, 1, 0, Vec::new());
    fresh.session_id = "controls2".into();
    r.observe(10, &fresh).unwrap();
    assert!(r.observe(20, &state(20, 2, 0, Vec::new())).is_err());
}
#[test]
fn changed_source_reader_is_fenced_and_cannot_replay_held_sleep() {
    let mut r = receiver();
    let sequence = heartbeat(&mut r, 2_000_000, Vec::new());
    let mut e = sleep(1, 10, 2_000_010);
    e.reader_session_id = "reader2".into();
    let mut s = state(e.observed_us, sequence, 1, vec![e.clone()]);
    s.sources[0].reader_session_id = Some("reader2".into());
    assert!(r.observe(e.observed_us, &s).unwrap().is_empty());
    assert!(r.event(e.observed_us, &e).unwrap().is_none());
    let mut old = state(2_000_020, sequence + 1, 1, Vec::new());
    old.sources[0].reader_session_id = Some("reader".into());
    assert!(r.observe(2_000_020, &old).is_err());
}
#[test]
fn source_fault_withdraws_held_lifecycle_eligibility_even_after_fresh_recovery() {
    let mut r = receiver();
    let mut seq = heartbeat(&mut r, 1_000_000, Vec::new());
    let mut s = state(1_000_010, seq, 0, Vec::new());
    seq += 1;
    let source = &mut s.sources[0];
    source.ready = false;
    source.fault = Some(ControlFault::Missing);
    source.armed = false;
    source.active = None;
    source.active_mask = None;
    r.observe(1_000_010, &s).unwrap();
    for at in (1_100_000..=2_000_000).step_by(100_000) {
        r.observe(at, &state(at, seq, 0, Vec::new())).unwrap();
        seq += 1;
    }
    let e = sleep(1, 10, 2_000_010);
    assert!(
        r.observe(2_000_010, &state(2_000_010, seq, 1, vec![e]))
            .unwrap()
            .is_empty()
    );
}
#[test]
fn stalled_state_stream_suppresses_hold_that_crossed_unknown_lifecycle_interval() {
    let mut r = receiver();
    let e = sleep(1, 10, 2_000_010);
    assert!(
        r.observe(e.observed_us, &state(e.observed_us, 2, 1, vec![e]))
            .unwrap()
            .is_empty()
    );
}
#[test]
fn backwards_receipt_clock_latches_closed() {
    let mut r = receiver();
    r.observe(10, &state(10, 2, 0, Vec::new())).unwrap();
    assert!(r.observe(9, &state(9, 3, 0, Vec::new())).is_err());
    assert!(r.observe(20, &state(20, 4, 0, Vec::new())).is_err());
}
#[test]
fn source_configuration_is_immutable_within_controls_session() {
    let mut r = receiver();
    let mut s = state(10, 2, 0, Vec::new());
    s.sources[0].name = "other-strip".into();
    assert!(r.observe(10, &s).is_err());
}
#[test]
fn session_fences_are_bounded_without_evicting_old_owners() {
    let mut r = receiver();
    for i in 1..=32 {
        let mut s = state(i, 1, 0, Vec::new());
        s.session_id = format!("controls{i}");
        r.observe(i, &s).unwrap();
    }
    let mut s = state(33, 1, 0, Vec::new());
    s.session_id = "controls33".into();
    assert!(r.observe(33, &s).is_err());
    assert!(r.observe(34, &state(34, 2, 0, Vec::new())).is_err());
}

#[test]
fn delayed_older_hold_cannot_follow_newer_touch_of_same_source() {
    let mut r = receiver();
    let sequence = heartbeat(&mut r, 2_000_000, Vec::new());
    let older = sleep(2, 10, 2_000_010);
    let newer = event(3, 2_020_000, 2_040_000);
    let s = state(2_040_000, sequence, 3, vec![newer.clone()]);
    assert_eq!(r.observe(2_040_000, &s).unwrap(), [newer]);
    assert!(r.event(2_040_001, &older).unwrap().is_none());
}

#[test]
fn retained_primary_stop_and_one_way_mute_are_not_lifecycle_replay() {
    for (gesture, intent, at) in [
        (TouchGesture::Tap, TouchIntent::Stop, 30_000),
        (TouchGesture::Hold, TouchIntent::SoftwareMute, 2_010_000),
    ] {
        let mut e = event(1, 10_000, at);
        e.source = TouchSource::Primary;
        e.name = "primary99".into();
        e.gesture = gesture;
        e.when_speaking = intent;
        e.when_quiet = intent;
        e.when_asleep = Some(intent);
        e.released_us = Some(at);
        e.active_mask = 0;
        e.position = None;
        let mut s = state(at, 1, 1, vec![e.clone()]);
        s.sources[0].source = TouchSource::Primary;
        s.sources[0].name = "primary99".into();
        let mut r = TouchReceiver::new("boot").unwrap();
        let accepted = r.observe(at, &s).unwrap();
        assert_eq!(accepted, std::slice::from_ref(&e));
        assert_eq!(accepted[0].intent(false, true), Some(intent));
        assert!(r.event(at, &e).unwrap().is_none());
    }
}

#[test]
fn fresh_whole_five_second_reset_is_available_to_asleep_owner() {
    let mut r = TouchReceiver::new("boot").unwrap();
    for i in 0..=50 {
        let mut s = state(i * 100_000, i + 1, 0, Vec::new());
        s.sources[0].source = TouchSource::Reset;
        s.sources[0].name = "reset100".into();
        r.observe(i * 100_000, &s).unwrap();
    }
    let mut e = event(1, 10, 5_000_010);
    e.source = TouchSource::Reset;
    e.name = "reset100".into();
    e.gesture = TouchGesture::Hold;
    e.when_speaking = TouchIntent::FactoryReset;
    e.when_quiet = TouchIntent::FactoryReset;
    e.when_asleep = Some(TouchIntent::FactoryReset);
    e.released_us = Some(e.observed_us);
    e.active_mask = 0;
    e.position = None;
    let mut s = state(e.observed_us, 52, 1, vec![e.clone()]);
    s.sources[0].source = TouchSource::Reset;
    s.sources[0].name = "reset100".into();
    let accepted = r.observe(e.observed_us, &s).unwrap();
    assert_eq!(accepted, std::slice::from_ref(&e));
    assert_eq!(
        accepted[0].intent(false, true),
        Some(TouchIntent::FactoryReset)
    );
}
