use lamp_bus::Kind;
use lamp_msgs::*;
use serde_json::json;

fn event() -> TouchEvent {
    TouchEvent {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: 1_100_000,
            sequence: 1,
        },
        session_id: "controls".into(),
        reader_session_id: "gpio".into(),
        enabled: true,
        source: TouchSource::Head,
        name: "head".into(),
        contact_id: 1,
        gesture: TouchGesture::Touch,
        when_speaking: TouchIntent::Stop,
        when_quiet: TouchIntent::Notice,
        when_asleep: None,
        began_us: 1_000_000,
        observed_us: 1_100_000,
        released_us: None,
        active_mask: 3,
        active_names: vec!["pad37".into(), "pad96".into()],
        position: None,
        toward_pad: None,
        distance: 0.0,
        speed: 0.0,
    }
}
fn state() -> TouchState {
    TouchState {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: 1_100_000,
            sequence: 1,
        },
        session_id: "controls".into(),
        enabled: true,
        sources: vec![TouchSourceState {
            source: TouchSource::Head,
            name: "head".into(),
            reader_session_id: Some("gpio".into()),
            ready: true,
            fault: None,
            observed_us: Some(1_100_000),
            armed: true,
            active: Some(true),
            contact_id: Some(1),
            began_us: Some(1_000_000),
            held_us: Some(100_000),
            active_mask: Some(3),
            active_names: vec!["pad37".into(), "pad96".into()],
            position: None,
            toward_pad: None,
            distance: 0.0,
            speed: 0.0,
        }],
        latest: vec![event()],
        event_sequence: 1,
        rejected_inputs: 0,
    }
}
fn strip_event() -> TouchEvent {
    TouchEvent {
        source: TouchSource::Strip,
        name: "strip".into(),
        reader_session_id: "i2c".into(),
        active_mask: 7,
        active_names: Vec::new(),
        position: Some(1.0 / 11.0),
        when_quiet: TouchIntent::Attend,
        when_asleep: Some(TouchIntent::Wake),
        ..event()
    }
}

#[test]
fn touch_topics_are_explicit_and_leave_v1_inventory_unchanged() {
    assert_eq!(first_topics().len(), 17);
    assert_eq!(
        controls_topics()
            .iter()
            .map(|(topic, _)| *topic)
            .collect::<Vec<_>>(),
        ["controls/gesture", "controls/state"]
    );
    assert_eq!(touch_topics().len(), 2);
    for (topic, declaration) in touch_topics() {
        assert_eq!(declaration_of(topic), Some(declaration));
        assert!(!first_topics().iter().any(|(name, _)| *name == topic));
    }
    assert_eq!(TouchEvent::declaration().kind, Kind::Event);
    assert_eq!(TouchEvent::declaration().lifespan_ms, Some(500));
    assert_eq!(TouchState::declaration().kind, Kind::Latest);
    assert_eq!(TouchState::declaration().period_ms, Some(100));
    assert_eq!(TouchState::declaration().lifespan_ms, Some(300));
}
#[test]
fn roundtrip_new_contract_and_reject_unknown_actions_fields() {
    let value = state();
    value.check().unwrap();
    assert_eq!(
        serde_json::from_str::<TouchState>(&serde_json::to_string(&value).unwrap()).unwrap(),
        value
    );
    let mut wire = serde_json::to_value(event()).unwrap();
    wire["permit"] = json!(true);
    assert!(serde_json::from_value::<TouchEvent>(wire).is_err());
    let mut wire = serde_json::to_value(event()).unwrap();
    wire["when_quiet"] = json!("unmute");
    assert!(serde_json::from_value::<TouchEvent>(wire).is_err());
}
#[test]
fn contextual_intent_is_chosen_by_owner_and_contains_no_readiness() {
    assert_eq!(event().intent(true, false), Some(TouchIntent::Stop));
    assert_eq!(event().intent(false, false), Some(TouchIntent::Notice));
    assert_eq!(event().intent(true, true), None);
    assert_eq!(event().intent(false, true), None);
    let tap = head_tap(300_000);
    assert_eq!(tap.intent(true, false), Some(TouchIntent::Attend));
    assert_eq!(tap.intent(false, false), Some(TouchIntent::Attend));
    assert_eq!(tap.intent(false, true), Some(TouchIntent::Wake));
    let wire = serde_json::to_string(&event()).unwrap();
    for forbidden in [
        "microphone",
        "camera",
        "permission",
        "listening",
        "speaking_state",
    ] {
        assert!(!wire.contains(forbidden));
    }
}
#[test]
fn original_acquisition_and_publication_both_expire_exactly() {
    let value = event();
    assert!(value.fresh_at("boot", 1_199_999));
    assert!(!value.fresh_at("boot", 1_200_000));
    assert!(!value.fresh_at("other", 1_100_000));
    let mut late = value;
    late.stamp.at_us = 1_599_999;
    assert!(!late.fresh_at("boot", 1_600_000));
    late.enabled = false;
    assert!(!late.fresh_at("boot", 1_599_999));
}
#[test]
fn invalid_identities_and_temporal_order_are_refused() {
    for mutate in [
        |e: &mut TouchEvent| e.stamp.sequence = 0,
        |e: &mut TouchEvent| e.contact_id = 0,
        |e: &mut TouchEvent| e.observed_us = e.stamp.at_us + 1,
        |e: &mut TouchEvent| e.began_us = e.observed_us + 1,
        |e: &mut TouchEvent| e.reader_session_id.clear(),
        |e: &mut TouchEvent| e.session_id = "\n".into(),
    ] {
        let mut e = event();
        mutate(&mut e);
        assert!(e.check().is_err());
    }
}
#[test]
fn reset_is_a_distinct_five_second_release_only_request() {
    let mut e = event();
    e.source = TouchSource::Reset;
    e.name = "reset100".into();
    e.gesture = TouchGesture::Hold;
    e.when_speaking = TouchIntent::FactoryReset;
    e.when_quiet = TouchIntent::FactoryReset;
    e.when_asleep = Some(TouchIntent::FactoryReset);
    e.active_mask = 0;
    e.active_names.clear();
    e.began_us = 0;
    e.observed_us = 5_000_000;
    e.stamp.at_us = 5_000_000;
    e.released_us = Some(5_000_000);
    e.check().unwrap();
    e.began_us = 1;
    assert!(e.check().is_err());
    e.began_us = 0;
    e.released_us = None;
    assert!(e.check().is_err());
    e.released_us = Some(5_000_000);
    e.source = TouchSource::Primary;
    assert!(e.check().is_err());
}
#[test]
fn primary_preserves_stop_and_software_mute_release_tiers() {
    let mut e = event();
    e.source = TouchSource::Primary;
    e.name = "primary99".into();
    e.active_mask = 0;
    e.active_names.clear();
    e.gesture = TouchGesture::Tap;
    e.released_us = Some(e.observed_us);
    e.when_quiet = TouchIntent::Stop;
    e.when_asleep = Some(TouchIntent::Stop);
    e.check().unwrap();
    e.gesture = TouchGesture::Hold;
    e.when_speaking = TouchIntent::SoftwareMute;
    e.when_quiet = TouchIntent::SoftwareMute;
    e.when_asleep = Some(TouchIntent::SoftwareMute);
    e.began_us = 0;
    e.observed_us = 2_000_000;
    e.stamp.at_us = 2_000_000;
    e.released_us = Some(2_000_000);
    e.check().unwrap();
    e.active_mask = 1;
    assert!(e.check().is_err());
}
#[test]
fn head_qualifies_actual_continuous_observations_only() {
    let mut e = event();
    e.began_us = e.observed_us - 19_999;
    assert!(e.check().is_err());
    e.began_us -= 1;
    e.check().unwrap();
    e.gesture = TouchGesture::Stroke;
    e.when_speaking = TouchIntent::Stroke;
    e.when_quiet = TouchIntent::Stroke;
    e.when_asleep = None;
    e.began_us = e.observed_us - 1_000_000;
    e.check().unwrap();
    e.began_us += 1;
    assert!(e.check().is_err());
}

/// A head tap released `held` after it began, confirmed 150 ms later.
fn head_tap(held: u64) -> TouchEvent {
    let mut tap = event();
    tap.gesture = TouchGesture::Tap;
    tap.when_speaking = TouchIntent::Attend;
    tap.when_quiet = TouchIntent::Attend;
    tap.when_asleep = Some(TouchIntent::Wake);
    tap.began_us = 0;
    tap.released_us = Some(held);
    tap.observed_us = held + 150_000;
    tap.stamp.at_us = tap.observed_us;
    tap.active_mask = 0;
    tap.active_names = Vec::new();
    tap
}

#[test]
fn the_head_stops_at_a_touch_listens_at_a_short_tap_and_sleeps_at_three_seconds() {
    // #711: only a short tap, let go, asks it to listen or wake.
    for held in [20_000, 499_999] {
        head_tap(held).check().unwrap();
    }
    for held in [19_999, 500_000] {
        assert!(head_tap(held).check().is_err(), "{held}");
    }
    let mut held_tap = head_tap(300_000);
    held_tap.released_us = None;
    assert!(held_tap.check().is_err());
    let mut lit_tap = head_tap(300_000);
    lit_tap.active_mask = 1;
    lit_tap.active_names = vec!["pad37".into()];
    assert!(lit_tap.check().is_err());
    let mut no_wake = head_tap(300_000);
    no_wake.when_asleep = None;
    assert!(no_wake.check().is_err());
    // A touch asks nothing while quiet, and never wakes it.
    let mut attend = event();
    attend.when_quiet = TouchIntent::Attend;
    assert!(attend.check().is_err());
    let mut wake = event();
    wake.when_asleep = Some(TouchIntent::Wake);
    assert!(wake.check().is_err());
    // A hand kept three seconds is sleep, never anything while asleep.
    let mut hold = event();
    hold.gesture = TouchGesture::Hold;
    hold.when_speaking = TouchIntent::Sleep;
    hold.when_quiet = TouchIntent::Sleep;
    hold.began_us = 0;
    hold.observed_us = 3_000_000;
    hold.stamp.at_us = 3_000_000;
    hold.check().unwrap();
    assert_eq!(hold.intent(false, true), None);
    hold.began_us = 1;
    assert!(hold.check().is_err());
}
#[test]
fn brief_qualified_strip_release_has_no_invented_current_position() {
    let mut e = strip_event();
    e.active_mask = 0;
    e.position = None;
    e.released_us = Some(e.observed_us);
    e.check().unwrap();
    e.position = Some(0.1);
    assert!(e.check().is_err());
    e.position = None;
    e.released_us = None;
    assert!(e.check().is_err());
}
#[test]
fn slides_require_finite_normalized_geometry_and_explicit_destination() {
    let mut e = strip_event();
    e.gesture = TouchGesture::Slide;
    e.when_speaking = TouchIntent::Slide;
    e.when_quiet = TouchIntent::Slide;
    e.when_asleep = None;
    e.toward_pad = Some(11);
    e.distance = 0.5;
    e.speed = 1.0;
    e.check().unwrap();
    for mutate in [
        |e: &mut TouchEvent| e.toward_pad = None,
        |e: &mut TouchEvent| e.toward_pad = Some(12),
        |e: &mut TouchEvent| e.position = Some(f32::NAN),
        |e: &mut TouchEvent| e.position = Some(1.01),
        |e: &mut TouchEvent| e.speed = f32::INFINITY,
        |e: &mut TouchEvent| e.distance = 1024.01,
        |e: &mut TouchEvent| e.active_mask = 0x1000,
    ] {
        let mut bad = e.clone();
        mutate(&mut bad);
        assert!(bad.check().is_err());
    }
}
#[test]
fn head_names_mask_and_source_geometry_are_exact() {
    let mut e = event();
    e.active_names.pop();
    assert!(e.check().is_err());
    let mut e = event();
    e.active_names[1] = e.active_names[0].clone();
    assert!(e.check().is_err());
    let mut e = event();
    e.active_mask = 0x10;
    assert!(e.check().is_err());
    let mut e = event();
    e.position = Some(0.0);
    assert!(e.check().is_err());
}
#[test]
fn state_requires_current_acquisition_and_unknown_withdraws_contact() {
    let mut s = state();
    s.stamp.at_us += 99_999;
    s.check().unwrap();
    s.stamp.at_us += 1;
    assert!(s.check().is_err());
    let mut s = state();
    s.sources[0].ready = false;
    s.sources[0].fault = Some(ControlFault::Missing);
    assert!(s.check().is_err());
    let source = &mut s.sources[0];
    source.armed = false;
    source.active = None;
    source.contact_id = None;
    source.began_us = None;
    source.held_us = None;
    source.active_mask = None;
    source.active_names.clear();
    s.check().unwrap();
}
#[test]
fn retained_events_cannot_change_session_time_or_overwrite_motion() {
    for mutate in [
        |s: &mut TouchState| s.latest[0].session_id = "other".into(),
        |s: &mut TouchState| s.latest[0].reader_session_id = "other".into(),
        |s: &mut TouchState| s.latest[0].stamp.sequence = 2,
        |s: &mut TouchState| s.latest[0].stamp.at_us += 1,
        |s: &mut TouchState| s.latest.push(s.latest[0].clone()),
        |s: &mut TouchState| s.sources.push(s.sources[0].clone()),
    ] {
        let mut s = state();
        mutate(&mut s);
        assert!(s.check().is_err());
    }
    let mut s = state();
    let e = &mut s.latest[0];
    e.gesture = TouchGesture::Stroke;
    e.began_us = e.observed_us - 1_000_000;
    e.when_speaking = TouchIntent::Stroke;
    e.when_quiet = TouchIntent::Stroke;
    e.when_asleep = None;
    e.check().unwrap();
    assert!(s.check().is_err());
}

#[test]
fn sleep_and_wake_are_distinct_fixed_alternatives_not_toggle_or_mic_grants() {
    let mut hold = strip_event();
    hold.gesture = TouchGesture::Hold;
    hold.when_speaking = TouchIntent::Sleep;
    hold.when_quiet = TouchIntent::Sleep;
    hold.began_us = 0;
    hold.observed_us = 2_000_000;
    hold.stamp.at_us = 2_000_000;
    hold.check().unwrap();
    assert_eq!(hold.intent(false, false), Some(TouchIntent::Sleep));
    assert_eq!(hold.intent(false, true), Some(TouchIntent::Wake));
    hold.when_asleep = Some(TouchIntent::Sleep);
    assert!(hold.check().is_err());
    hold.when_asleep = None;
    assert!(hold.check().is_err());
    let mut bad = event();
    bad.when_quiet = TouchIntent::Wake;
    assert!(bad.check().is_err());
    let mut wire = serde_json::to_value(event()).unwrap();
    wire["when_asleep"] = json!("toggle");
    assert!(serde_json::from_value::<TouchEvent>(wire).is_err());
}

#[test]
fn primary_and_reset_keep_their_exact_one_way_intent_while_asleep() {
    for (source, gesture, intent, duration) in [
        (
            TouchSource::Primary,
            TouchGesture::Tap,
            TouchIntent::Stop,
            20_000,
        ),
        (
            TouchSource::Primary,
            TouchGesture::Hold,
            TouchIntent::SoftwareMute,
            2_000_000,
        ),
        (
            TouchSource::Reset,
            TouchGesture::Hold,
            TouchIntent::FactoryReset,
            5_000_000,
        ),
    ] {
        let mut e = event();
        e.source = source;
        e.gesture = gesture;
        e.when_speaking = intent;
        e.when_quiet = intent;
        e.when_asleep = Some(intent);
        e.began_us = 0;
        e.observed_us = duration;
        e.stamp.at_us = duration;
        e.released_us = Some(duration);
        e.active_mask = 0;
        e.active_names.clear();
        e.check().unwrap();
        assert_eq!(e.intent(false, true), Some(intent));
        assert_eq!(e.intent(true, true), Some(intent));
        e.when_asleep = None;
        assert!(e.check().is_err());
        e.when_asleep = Some(TouchIntent::Wake);
        assert!(e.check().is_err());
    }
}
