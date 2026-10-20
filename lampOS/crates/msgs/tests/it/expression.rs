use lamp_bus::Kind;
use lamp_msgs::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::json;
use std::fmt::Debug;

fn stamp() -> ClockStamp {
    ClockStamp {
        clock_id: "boot-one".into(),
        at_us: 100_000,
        sequence: 1,
    }
}

fn lease() -> ExpressionLease {
    ExpressionLease {
        clock_id: "boot-one".into(),
        session_id: "expression-one".into(),
        voice_session_id: "voice-one".into(),
        sequence: 1,
        turn: 2,
        request: 3,
        issued_us: 100_000,
        apply_us: 100_000,
        valid_until_us: 300_000,
    }
}

fn roundtrip<T: Serialize + DeserializeOwned + Debug + PartialEq>(value: &T) {
    let mut wire = serde_json::to_value(value).unwrap();
    assert_eq!(&serde_json::from_value::<T>(wire.clone()).unwrap(), value);
    wire["future_optional_field"] = json!({"value":123});
    assert_eq!(&serde_json::from_value::<T>(wire).unwrap(), value);
}

#[test]
fn optional_expression_topics_have_own_types_and_do_not_replace_light_setting() {
    let topics = expression_topics();
    assert_eq!(topics.len(), 11);
    assert_eq!(first_topics().len(), 17);
    for (topic, declaration) in &topics {
        assert_eq!(declaration_of(topic).as_ref(), Some(declaration));
        assert_eq!(declaration.version, 1);
        assert!(declaration.type_name.starts_with("lamp_msgs/"));
    }
    assert_eq!(VoiceTiming::declaration().kind, Kind::Latest);
    assert_eq!(VoicePlayout::declaration().kind, Kind::Event);
    assert_eq!(ExpressionBody::declaration().lifespan_ms, Some(200));
    assert_eq!(ExpressionLight::declaration().lifespan_ms, Some(200));
    assert_eq!(ExpressionState::declaration().period_ms, Some(200));
    assert_eq!(
        declaration_of("light/command").unwrap().type_name,
        Light::TYPE
    );
    assert_eq!(Concentration::declaration().lifespan_ms, None);
}

#[test]
fn every_new_wire_type_round_trips_and_ignores_future_fields() {
    let timing = VoiceTiming {
        stamp: stamp(),
        session_id: "voice-one".into(),
        turn: 2,
        request: 3,
        state: Voice::Thinking,
        accepting_input: true,
        event: VoiceEvent::Scheduled,
        first_audio_us: Some(200_000),
    };
    timing.check().unwrap();
    roundtrip(&timing);
    let activity = PersonActivity {
        stamp: stamp(),
        session_id: "voice-one".into(),
        turn: 2,
        request: 3,
        speaking: true,
    };
    activity.check().unwrap();
    roundtrip(&activity);
    let played = VoicePlayout {
        stamp: stamp(),
        session_id: "voice-one".into(),
        turn: 2,
        request: 3,
        start_us: 110_000,
        end_us: 130_000,
        level: 0.7,
    };
    played.check().unwrap();
    roundtrip(&played);
    let concentration = Concentration {
        stamp: stamp(),
        concentrating: true,
    };
    concentration.check().unwrap();
    roundtrip(&concentration);
    let gaze = ExpressionGaze {
        face_id: "face-one".into(),
        x: -0.3,
        y: 0.2,
        observed_us: 90_000,
        valid_until_us: 200_000,
    };
    gaze.check().unwrap();
    roundtrip(&gaze);
    let body = ExpressionBody {
        lease: lease(),
        offsets: BodyOffsets::ZERO,
        hold: false,
        gaze: Some(gaze),
    };
    body.check().unwrap();
    roundtrip(&body);
    let light = ExpressionLight {
        lease: lease(),
        setting: Light::new(true, "warm", 3).unwrap(),
        face: ExpressionFace::Listening,
        brightness: 0.4,
    };
    light.check().unwrap();
    roundtrip(&light);
    let state = ExpressionState {
        stamp: stamp(),
        enabled: false,
        turn: Some(2),
        request: Some(3),
        behaviours: vec!["look away and back".into()],
        missing: vec!["measured body lag".into()],
        warnings: vec![],
        body: false,
        light: false,
    };
    state.check().unwrap();
    roundtrip(&state);
}

#[test]
fn required_identity_is_not_guessed_and_unknown_states_are_errors() {
    let value = json!({"stamp":stamp(),"session_id":"voice","turn":1,"request":2,"state":"listening","accepting_input":true,"event":"state"});
    for field in [
        "stamp",
        "session_id",
        "turn",
        "request",
        "state",
        "accepting_input",
        "event",
    ] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<VoiceTiming>(missing).is_err(),
            "{field}"
        );
    }
    let mut unknown = value.clone();
    unknown["event"] = "probably_finished".into();
    assert!(serde_json::from_value::<VoiceTiming>(unknown).is_err());
    let mut unknown = value;
    unknown["state"] = "dreaming".into();
    assert!(serde_json::from_value::<VoiceTiming>(unknown).is_err());
    assert!(serde_json::from_value::<ExpressionFace>(json!("maybe_listening")).is_err());
}

#[test]
fn schedules_require_a_future_device_clock_time_and_terminal_states_are_explicit() {
    let mut timing = VoiceTiming {
        stamp: stamp(),
        session_id: "voice".into(),
        turn: 1,
        request: 1,
        state: Voice::Thinking,
        accepting_input: true,
        event: VoiceEvent::Scheduled,
        first_audio_us: None,
    };
    assert!(timing.check().is_err());
    timing.first_audio_us = Some(99_999);
    assert!(timing.check().is_err());
    timing.first_audio_us = Some(2_100_001);
    assert!(timing.check().is_err());
    timing.first_audio_us = Some(2_100_000);
    assert!(timing.check().is_ok());
    timing.event = VoiceEvent::State;
    assert!(timing.check().is_err());
    timing.first_audio_us = None;
    assert!(timing.check().is_ok());
    timing.event = VoiceEvent::Interrupted;
    assert!(timing.check().is_err());
    timing.state = Voice::Listening;
    assert!(timing.check().is_ok());
    timing.event = VoiceEvent::Cancelled;
    timing.state = Voice::Speaking;
    assert!(timing.check().is_err());
}

#[test]
fn lease_expiry_is_half_open_and_cannot_be_extended_by_apply_time() {
    let original = lease();
    assert!(original.valid_at("boot-one", 100_000));
    assert!(original.valid_at("boot-one", 299_999));
    assert!(!original.valid_at("boot-one", 300_000));
    assert!(!original.valid_at("boot-one", 99_999));
    assert!(!original.valid_at("other-boot", 100_000));
    let mut long = original.clone();
    long.valid_until_us += 1;
    assert!(long.check().is_err());
    long.apply_us += 100_000;
    assert!(long.check().is_err());
    let mut reversed = original.clone();
    reversed.apply_us = 300_000;
    assert!(reversed.check().is_err());
    let mut invalid = original.clone();
    invalid.sequence = 0;
    assert!(invalid.check().is_err());
    let mut invalid = original.clone();
    invalid.voice_session_id.clear();
    assert!(invalid.check().is_err());
    let mut invalid = original;
    invalid.clock_id = "wrong\nclock".into();
    assert!(invalid.check().is_err());
}

#[test]
fn finite_si_offsets_and_normalized_gaze_are_checked_at_the_boundary() {
    for value in [
        f64::NAN,
        f64::INFINITY,
        -f64::INFINITY,
        0.350_001,
        -0.350_001,
    ] {
        assert!(
            BodyOffsets {
                base_yaw: value,
                ..BodyOffsets::ZERO
            }
            .check()
            .is_err()
        );
    }
    assert!(
        BodyOffsets {
            base_yaw: 0.35,
            head_pitch: -0.35,
            ..BodyOffsets::ZERO
        }
        .check()
        .is_ok()
    );
    let mut body = ExpressionBody {
        lease: lease(),
        offsets: BodyOffsets::ZERO,
        hold: false,
        gaze: Some(ExpressionGaze {
            face_id: "one".into(),
            x: 0.5,
            y: -0.5,
            observed_us: 0,
            valid_until_us: 99_999,
        }),
    };
    assert!(
        body.check().is_err(),
        "gaze already expired before the command applies"
    );
    body.gaze.as_mut().unwrap().valid_until_us = 200_000;
    body.check().unwrap();
    body.hold = true;
    assert!(body.check().is_err());
    body.hold = false;
    body.gaze.as_mut().unwrap().x = f32::NAN;
    assert!(body.check().is_err());
    body.gaze.as_mut().unwrap().x = 0.0;
    body.gaze.as_mut().unwrap().valid_until_us = 300_001;
    assert!(body.check().is_err());
}

#[test]
fn off_setting_stays_off_and_shadow_reports_cannot_claim_outputs() {
    let mut light = ExpressionLight {
        lease: lease(),
        setting: Light::new(false, "blue", 2).unwrap(),
        face: ExpressionFace::Speaking,
        brightness: 0.2,
    };
    assert!(light.check().is_err());
    light.brightness = 0.0;
    light.check().unwrap();
    light.setting.on = true;
    light.brightness = f32::NAN;
    assert!(light.check().is_err());
    let mut state = ExpressionState {
        stamp: stamp(),
        enabled: false,
        turn: Some(1),
        request: Some(2),
        behaviours: vec![],
        missing: vec![],
        warnings: vec![],
        body: true,
        light: false,
    };
    assert!(state.check().is_err());
    state.body = false;
    state.check().unwrap();
    state.request = None;
    assert!(state.check().is_err());
    state.request = Some(2);
    state.behaviours = vec!["many".into(); 33];
    assert!(state.check().is_err());
}

#[test]
fn invalid_playout_levels_intervals_and_identity_are_refused() {
    let valid = VoicePlayout {
        stamp: stamp(),
        session_id: "voice".into(),
        turn: 1,
        request: 1,
        start_us: 100_000,
        end_us: 120_000,
        level: 0.5,
    };
    for level in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
        assert!(
            VoicePlayout {
                level,
                ..valid.clone()
            }
            .check()
            .is_err()
        );
    }
    for end_us in [99_999, 100_000, 300_001] {
        assert!(
            VoicePlayout {
                end_us,
                ..valid.clone()
            }
            .check()
            .is_err()
        );
    }
    assert!(
        VoicePlayout {
            start_us: 2_100_001,
            end_us: 2_100_002,
            ..valid.clone()
        }
        .check()
        .is_err()
    );
    assert!(
        VoicePlayout {
            session_id: String::new(),
            ..valid
        }
        .check()
        .is_err()
    );
}
