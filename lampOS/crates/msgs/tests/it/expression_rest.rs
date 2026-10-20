use lamp_bus::Kind;
use lamp_msgs::*;
use serde_json::{Value, json};

fn progress() -> VoiceProgress {
    VoiceProgress {
        timing: VoiceTiming {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: 100_000,
                sequence: 2,
            },
            session_id: "voice".into(),
            turn: 3,
            request: 4,
            state: Voice::Listening,
            accepting_input: true,
            event: VoiceEvent::State,
            first_audio_us: None,
        },
        audio_generation: 2,
        held: true,
    }
}

fn sound() -> VoiceSound {
    VoiceSound {
        playout: VoicePlayout {
            stamp: ClockStamp {
                clock_id: "boot".into(),
                at_us: 120_000,
                sequence: 3,
            },
            session_id: "voice".into(),
            turn: 3,
            request: 4,
            start_us: 130_000,
            end_us: 150_000,
            level: 0.5,
        },
        audio_generation: 2,
    }
}

#[test]
fn new_topics_have_distinct_types_and_original_contracts_are_unchanged() {
    assert_eq!(VoiceProgress::declaration().kind, Kind::Latest);
    assert_eq!(VoiceProgress::declaration().period_ms, Some(100));
    assert_eq!(VoiceProgress::declaration().lifespan_ms, Some(500));
    assert_eq!(VoiceSound::declaration().kind, Kind::Event);
    assert_eq!(VoiceSound::declaration().lifespan_ms, Some(200));
    assert_eq!(
        declaration_of(VoiceProgress::TOPIC),
        Some(VoiceProgress::declaration())
    );
    assert_eq!(
        declaration_of(VoiceSound::TOPIC),
        Some(VoiceSound::declaration())
    );
    assert_eq!(
        declaration_of("voice/timing"),
        Some(VoiceTiming::declaration())
    );
    assert_eq!(
        declaration_of("voice/playout"),
        Some(VoicePlayout::declaration())
    );
    assert_ne!(VoiceProgress::TYPE, VoiceTiming::TYPE);
    assert_ne!(VoiceSound::TYPE, VoicePlayout::TYPE);
}

#[test]
fn additive_wire_is_flat_and_future_fields_remain_compatible() {
    let value = progress();
    value.check().unwrap();
    let mut wire = serde_json::to_value(&value).unwrap();
    assert_eq!(wire["session_id"], "voice");
    assert_eq!(wire["audio_generation"], 2);
    assert!(wire.get("timing").is_none());
    wire["future_optional"] = json!({"unused":true});
    assert_eq!(
        serde_json::from_value::<VoiceProgress>(wire).unwrap(),
        value
    );
    let value = sound();
    value.check().unwrap();
    let mut wire = serde_json::to_value(&value).unwrap();
    assert_eq!(wire["start_us"], 130_000);
    assert!(wire.get("playout").is_none());
    wire["future_optional"] = Value::Null;
    assert_eq!(serde_json::from_value::<VoiceSound>(wire).unwrap(), value);
}

#[test]
fn required_fences_are_never_inferred_from_legacy_messages() {
    let good = serde_json::to_value(progress()).unwrap();
    for field in [
        "stamp",
        "session_id",
        "turn",
        "request",
        "state",
        "accepting_input",
        "event",
        "audio_generation",
        "held",
    ] {
        let mut wire = good.clone();
        wire.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<VoiceProgress>(wire).is_err(),
            "{field}"
        );
    }
    let mut wire = serde_json::to_value(sound()).unwrap();
    wire.as_object_mut().unwrap().remove("audio_generation");
    assert!(serde_json::from_value::<VoiceSound>(wire).is_err());
    let mut zero = progress();
    zero.audio_generation = 0;
    assert!(zero.check().is_err());
    let mut zero = sound();
    zero.audio_generation = 0;
    assert!(zero.check().is_err());
}

#[test]
fn held_progress_is_listening_but_readiness_remains_the_owners_fact() {
    let mut value = progress();
    value.timing.accepting_input = false;
    value.check().unwrap();
    for state in [Voice::Resting, Voice::Thinking, Voice::Speaking] {
        value.timing.state = state;
        assert!(value.check().is_err());
    }
    value.timing.state = Voice::Listening;
    for event in [
        VoiceEvent::Interrupted,
        VoiceEvent::Cancelled,
        VoiceEvent::Finished,
    ] {
        value.timing.event = event;
        assert!(value.check().is_err());
    }
    value.held = false;
    value.check().unwrap();
}

#[test]
fn generation_fences_do_not_relax_schedules_intervals_or_identity_validation() {
    let mut value = progress();
    value.held = false;
    value.timing.state = Voice::Thinking;
    value.timing.event = VoiceEvent::Scheduled;
    value.timing.first_audio_us = Some(200_000);
    value.check().unwrap();
    value.timing.first_audio_us = Some(2_100_001);
    assert!(value.check().is_err());
    value.timing.first_audio_us = Some(200_000);
    value.timing.session_id.clear();
    assert!(value.check().is_err());
    let mut value = sound();
    value.playout.end_us = value.playout.start_us + 200_001;
    assert!(value.check().is_err());
    value.playout.end_us = value.playout.start_us + 1;
    value.playout.level = f32::NAN;
    assert!(value.check().is_err());
}

#[test]
fn duplicate_generation_or_nested_identity_cannot_override_an_earlier_field() {
    let wire = serde_json::to_string(&progress()).unwrap();
    let duplicate = wire.replacen('{', "{\"audio_generation\":9,", 1);
    assert!(serde_json::from_str::<VoiceProgress>(&duplicate).is_err());
    let duplicate = wire.replacen('{', "{\"session_id\":\"other\",", 1);
    assert!(serde_json::from_str::<VoiceProgress>(&duplicate).is_err());
}
