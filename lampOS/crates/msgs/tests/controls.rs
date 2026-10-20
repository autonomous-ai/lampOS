use lamp_bus::Kind;
use lamp_msgs::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use std::fmt::Debug;

fn gesture() -> ControlGesture {
    ControlGesture {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: 100_000,
            sequence: 1,
        },
        session_id: "controls".into(),
        source: ControlSource::Button,
        name: "button".into(),
        gesture: Gesture::Tap,
        intent: ControlIntent::Stop,
        duration_us: 10_000,
        released_us: 100_000,
        enabled: true,
    }
}

fn state() -> ControlsState {
    ControlsState {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: 100_000,
            sequence: 1,
        },
        session_id: "controls".into(),
        enabled: true,
        ready: true,
        fault: None,
        buttons: vec![ButtonControlState {
            name: "button".into(),
            pressed: false,
            armed: true,
            held_us: None,
            tier: HoldTier::None,
        }],
        head_active: false,
        base_ready: false,
        base_held_us: None,
        base_tier: HoldTier::None,
        latest_stop: Some(gesture()),
        latest_mute: None,
        event_sequence: 1,
        rejected_inputs: 0,
        privacy: None,
        no_privacy_slide: false,
    }
}

fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let mut wire = serde_json::to_value(value).unwrap();
    assert_eq!(&serde_json::from_value::<T>(wire.clone()).unwrap(), value);
    wire["future_optional_field"] = json!({"enabled":false});
    assert_eq!(&serde_json::from_value::<T>(wire).unwrap(), value);
}

#[test]
fn controls_topics_are_additive_with_bounded_event_and_status_freshness() {
    assert_eq!(first_topics().len(), 17);
    assert_eq!(controls_topics().len(), 2);
    for (topic, declaration) in controls_topics() {
        assert_eq!(declaration_of(topic), Some(declaration.clone()));
        assert_eq!(declaration.version, 1);
    }
    assert_eq!(ControlGesture::declaration().kind, Kind::Event);
    assert_eq!(ControlGesture::declaration().lifespan_ms, Some(500));
    assert_eq!(ControlsState::declaration().kind, Kind::Latest);
    assert_eq!(ControlsState::declaration().period_ms, Some(100));
    assert_eq!(ControlsState::declaration().lifespan_ms, Some(300));
    assert_eq!(
        declaration_of("light/command").unwrap().type_name,
        Light::TYPE
    );
}

#[test]
fn controls_contracts_roundtrip_with_future_fields_without_privacy_grants() {
    gesture().check().unwrap();
    state().check().unwrap();
    roundtrip(&gesture());
    roundtrip(&state());
    roundtrip(&state().buttons[0]);
    let wire = serde_json::to_string(&state()).unwrap();
    for forbidden in [
        "microphone",
        "camera",
        "permit",
        "unmute",
        "shutdown",
        "reset_device",
    ] {
        assert!(!wire.contains(forbidden));
    }
}

#[test]
fn safe_source_gesture_intent_combinations_are_the_only_accepted_actions() {
    for source in [
        ControlSource::Button,
        ControlSource::Headpad,
        ControlSource::Base,
    ] {
        for kind in [
            Gesture::Tap,
            Gesture::DoubleTap,
            Gesture::MultiTap,
            Gesture::LongPress,
            Gesture::Pet,
        ] {
            for intent in [
                ControlIntent::Stop,
                ControlIntent::SoftwareMute,
                ControlIntent::Pet,
            ] {
                let mut event = gesture();
                event.source = source;
                event.gesture = kind;
                event.intent = intent;
                event.stamp.at_us = 3_000_000;
                event.released_us = 3_000_000;
                event.duration_us = if kind == Gesture::LongPress {
                    2_000_000
                } else {
                    10_000
                };
                let permitted = matches!(
                    (source, kind, intent),
                    (ControlSource::Headpad, Gesture::Pet, ControlIntent::Pet)
                        | (
                            ControlSource::Button | ControlSource::Base,
                            Gesture::Tap | Gesture::DoubleTap | Gesture::MultiTap,
                            ControlIntent::Stop
                        )
                        | (
                            ControlSource::Button | ControlSource::Base,
                            Gesture::LongPress,
                            ControlIntent::SoftwareMute
                        )
                );
                assert_eq!(
                    event.check().is_ok(),
                    permitted,
                    "{source:?}/{kind:?}/{intent:?}"
                );
            }
        }
    }
}

#[test]
fn future_zero_identity_and_impossible_edge_durations_are_rejected() {
    for variant in 0..5 {
        let mut event = gesture();
        match variant {
            0 => event.stamp.sequence = 0,
            1 => event.released_us = event.stamp.at_us + 1,
            2 => event.duration_us = event.released_us + 1,
            3 => event.session_id.clear(),
            _ => event.name = "x".repeat(129),
        }
        assert!(event.check().is_err());
    }
}

#[test]
fn tap_and_hold_intents_enforce_the_release_duration_boundary() {
    let mut event = gesture();
    event.stamp.at_us = 3_000_000;
    event.released_us = 3_000_000;
    event.duration_us = 1_999_999;
    event.check().unwrap();
    event.duration_us = 2_000_000;
    assert!(event.check().is_err());
    event.gesture = Gesture::LongPress;
    event.intent = ControlIntent::SoftwareMute;
    event.check().unwrap();
    event.duration_us = 1_999_999;
    assert!(event.check().is_err());
}

#[test]
fn stop_cannot_refresh_itself_by_republishing_an_old_release() {
    let event = gesture();
    assert!(event.fresh_at("boot", 599_999));
    assert!(!event.fresh_at("boot", 600_000));
    assert!(!event.fresh_at("other-boot", 100_000));
    assert!(!event.fresh_at("boot", 99_999));
    let mut republished = event.clone();
    republished.stamp.at_us = 1_000_000;
    republished.check().unwrap();
    assert!(!republished.fresh_at("boot", 1_000_000));
    let mut shadow = event;
    shadow.enabled = false;
    assert!(!shadow.fresh_at("boot", 100_000));
}

#[test]
fn intentional_pet_decision_delay_does_not_make_a_new_pet_event_stale() {
    let mut event = gesture();
    event.source = ControlSource::Headpad;
    event.gesture = Gesture::Pet;
    event.intent = ControlIntent::Pet;
    event.stamp.at_us = event.released_us + 1_400_000;
    assert!(event.fresh_at("boot", event.stamp.at_us));
    assert!(!event.fresh_at("boot", event.stamp.at_us + CONTROL_EVENT_TTL_US));
}

#[test]
fn held_feedback_requires_fresh_armed_press_and_matching_tier() {
    for (duration, tier) in [
        (0, HoldTier::None),
        (1_999_999, HoldTier::None),
        (2_000_000, HoldTier::Mute),
        (5_000_000, HoldTier::ExtendedMute),
    ] {
        let good = ButtonControlState {
            name: "button".into(),
            pressed: true,
            armed: true,
            held_us: Some(duration),
            tier,
        };
        good.check().unwrap();
        let mut unarmed = good.clone();
        unarmed.armed = false;
        assert!(unarmed.check().is_err());
        let mut released = good.clone();
        released.pressed = false;
        assert!(released.check().is_err());
        let mut wrong = good;
        wrong.tier = if tier == HoldTier::None {
            HoldTier::Mute
        } else {
            HoldTier::None
        };
        assert!(wrong.check().is_err());
    }
}

#[test]
fn fault_status_cannot_claim_ready_or_retain_active_hold_feedback() {
    let mut value = state();
    value.fault = Some(ControlFault::Missing);
    assert!(value.check().is_err());
    value.ready = false;
    assert!(value.check().is_err());
    value.buttons[0].armed = false;
    value.check().unwrap();
    value.head_active = true;
    assert!(value.check().is_err());
    value.head_active = false;
    value.base_ready = true;
    assert!(value.check().is_err());
    value.base_ready = false;
    value.fault = None;
    assert!(value.check().is_err());
}

#[test]
fn retained_requests_cannot_change_session_boot_intent_time_or_enabled_state() {
    for variant in 0..6 {
        let mut value = state();
        let record = value.latest_stop.as_mut().unwrap();
        match variant {
            0 => record.session_id = "another-controls".into(),
            1 => record.stamp.clock_id = "another-boot".into(),
            2 => record.stamp.sequence = 2,
            3 => record.stamp.at_us += 1,
            4 => record.enabled = false,
            _ => {
                record.source = ControlSource::Headpad;
                record.gesture = Gesture::Pet;
                record.intent = ControlIntent::Pet;
            }
        }
        assert!(value.check().is_err());
    }
}

#[test]
fn an_old_one_way_mute_can_be_retained_during_input_fault() {
    let mut value = state();
    let mut mute = gesture();
    mute.gesture = Gesture::LongPress;
    mute.intent = ControlIntent::SoftwareMute;
    mute.stamp.at_us = 3_000_000;
    mute.released_us = 3_000_000;
    mute.duration_us = 2_000_000;
    mute.stamp.sequence = 2;
    value.latest_mute = Some(mute);
    value.event_sequence = 2;
    value.stamp.at_us = 60_000_000;
    value.ready = false;
    value.fault = Some(ControlFault::Missing);
    value.buttons[0].armed = false;
    value.check().unwrap();
    assert_eq!(value.latest_mute.as_ref().unwrap().stamp.at_us, 3_000_000);
}

#[test]
fn status_arrays_and_names_are_bounded_and_unknown_action_names_fail() {
    let mut value = state();
    value.buttons.push(value.buttons[0].clone());
    assert!(value.check().is_err());
    value.buttons = (0..9)
        .map(|i| ButtonControlState {
            name: i.to_string(),
            pressed: false,
            armed: false,
            held_us: None,
            tier: HoldTier::None,
        })
        .collect();
    assert!(value.check().is_err());
    let mut event = gesture();
    event.name = "line\nname".into();
    assert!(event.check().is_err());
    let mut wire = serde_json::to_value(gesture()).unwrap();
    wire["intent"] = "unmute".into();
    assert!(serde_json::from_value::<ControlGesture>(wire).is_err());
}

#[test]
fn base_feedback_needs_ready_observed_contact_and_matching_hold_tier() {
    let mut value = state();
    value.stamp.at_us = 10_000_000;
    value.base_ready = true;
    for (held, tier) in [
        (1_999_999, HoldTier::None),
        (2_000_000, HoldTier::Mute),
        (5_000_000, HoldTier::ExtendedMute),
    ] {
        value.base_held_us = Some(held);
        value.base_tier = tier;
        value.check().unwrap();
        value.base_ready = false;
        assert!(value.check().is_err());
        value.base_ready = true;
        value.base_tier = if tier == HoldTier::None {
            HoldTier::Mute
        } else {
            HoldTier::None
        };
        assert!(value.check().is_err());
    }
    value.base_held_us = Some(value.stamp.at_us + 1);
    value.base_tier = HoldTier::ExtendedMute;
    assert!(value.check().is_err());
    value.base_held_us = None;
    value.base_tier = HoldTier::Mute;
    assert!(value.check().is_err());
    value.base_tier = HoldTier::None;
    value.check().unwrap();
}

/// `LAMP_PRIVACY_CONTROLS=no-slide`: Controls says no slide is wired. The
/// marker is left out when false, so other states keep their bytes, and it
/// never comes with slide evidence.
#[test]
fn the_no_slide_marker_is_positive_omitted_when_false_and_never_beside_a_switch() {
    let plain = serde_json::to_value(state()).unwrap();
    assert!(plain.get("no_privacy_slide").is_none());
    let mut marked = state();
    marked.no_privacy_slide = true;
    marked.check().unwrap();
    roundtrip(&marked);
    assert_eq!(
        serde_json::to_value(&marked).unwrap()["no_privacy_slide"],
        json!(true)
    );
    let mut both = marked.clone();
    both.privacy = Some(PrivacySwitch {
        stamp: marked.stamp.clone(),
        reader_session_id: "slide".into(),
        muted: Some(false),
        fault: None,
    });
    assert!(both.check().is_err());
    both.no_privacy_slide = false;
    both.check().unwrap();
}
