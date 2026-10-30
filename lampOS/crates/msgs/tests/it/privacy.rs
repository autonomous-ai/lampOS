use lamp_bus::{Header, Kind, Message};
use lamp_msgs::{ClockStamp, MicrophonePrivacy};

fn lease() -> MicrophonePrivacy {
    MicrophonePrivacy {
        stamp: ClockStamp {
            clock_id: "boot".into(),
            at_us: 100_000,
            sequence: 1,
        },
        session_id: "0123456789abcdef".into(),
        controller_boot: "0123456789abcdef0123456789abcdef".into(),
        microphone_generation: 1,
        observed_us: 95_000,
        expires_at_us: 145_000,
        permitted: true,
        physical_muted: Some(false),
        no_slide: false,
    }
}

/// A lamp with no slide: the Controls publication saying so stands for the
/// acquisition, and the physical state stays unknown.
fn no_slide() -> MicrophonePrivacy {
    MicrophonePrivacy {
        physical_muted: None,
        no_slide: true,
        ..lease()
    }
}

fn message(value: &MicrophonePrivacy) -> Message {
    Message {
        header: Header {
            topic: MicrophonePrivacy::TOPIC.into(),
            kind: Kind::Latest,
            enc: lamp_bus::JSON.into(),
            type_name: Some(MicrophonePrivacy::TYPE.into()),
            source: "voice".into(),
            boot: value.session_id.clone(),
            seq: value.stamp.sequence,
            at_us: value.stamp.at_us,
            at_unix_ms: 1000,
            expires_ms: Some(50),
            clock: None,
        },
        payload: serde_json::to_vec(value).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

#[test]
fn freshness_is_original_acquisition_bounded_and_boot_scoped() {
    let value = lease();
    value.check().unwrap();
    assert!(value.fresh_at("boot", 144_999));
    for now in [99_999, 145_000, 200_000] {
        assert!(!value.fresh_at("boot", now));
    }
    assert!(!value.fresh_at("other", 100_000));
    let mut stale = value.clone();
    stale.stamp.at_us = 144_000;
    stale.expires_at_us = 194_000;
    assert!(
        stale.check().is_err(),
        "renewal cannot relabel an old acquisition"
    );
}

#[test]
fn permission_requires_known_open_switch_and_bounded_valid_lineage() {
    let good = lease();
    for mutation in 0..12 {
        let mut value = good.clone();
        match mutation {
            0 => value.physical_muted = Some(true),
            1 => {
                value.physical_muted = None;
                value.observed_us = 0;
            }
            2 => value.session_id.clear(),
            3 => value.controller_boot = "g".repeat(32),
            4 => value.microphone_generation = 0,
            5 => value.stamp.sequence = 0,
            6 => value.observed_us = 100_001,
            7 => value.expires_at_us = 100_000,
            8 => value.expires_at_us = 150_001,
            9 => value.observed_us = 50_000,
            10 => value.observed_us = 0,
            _ => value.controller_boot = "0".repeat(32),
        }
        assert!(value.check().is_err(), "mutation {mutation}");
    }
    let mut unknown = good;
    unknown.permitted = false;
    unknown.physical_muted = None;
    unknown.observed_us = 0;
    unknown.check().unwrap();
}

#[test]
fn envelope_must_name_voice_and_match_payload_clock_time_session_and_sequence() {
    let good = message(&lease());
    MicrophonePrivacy::from_message(&good, "boot", 100_000, 1000).unwrap();
    for mutation in 0..12 {
        let mut value = good.clone();
        match mutation {
            0 => value.header.topic = "controls/state".into(),
            1 => value.header.source = "controls".into(),
            2 => value.header.type_name = None,
            3 => value.header.kind = Kind::Event,
            4 => value.header.enc = "none".into(),
            5 => value.header.seq = 2,
            6 => value.header.expires_ms = None,
            7 => value.expired = true,
            8 => value.header.at_unix_ms = 100_000,
            9 => value.header.boot = "fedcba9876543210".into(),
            10 => value.header.at_us += 1,
            _ => value.payload = b"{}".to_vec(),
        }
        assert!(
            MicrophonePrivacy::from_message(&value, "boot", 100_000, 1000).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn topic_is_additive_and_json_roundtrips() {
    let value = lease();
    assert_eq!(
        serde_json::from_slice::<MicrophonePrivacy>(&serde_json::to_vec(&value).unwrap()).unwrap(),
        value
    );
    assert_eq!(
        lamp_msgs::declaration_of(MicrophonePrivacy::TOPIC),
        Some(MicrophonePrivacy::declaration())
    );
    assert_eq!(
        lamp_msgs::declaration_of(lamp_msgs::CameraClosed::TOPIC),
        Some(lamp_msgs::CameraClosed::declaration())
    );
    assert!(
        !lamp_msgs::first_topics()
            .iter()
            .any(|(topic, _)| *topic == MicrophonePrivacy::TOPIC)
    );
    let mut json = serde_json::to_value(&value).unwrap();
    json["future_field"] = serde_json::json!({"ignored": true});
    assert_eq!(
        serde_json::from_value::<MicrophonePrivacy>(json.clone()).unwrap(),
        value
    );
    for field in [
        "stamp",
        "session_id",
        "controller_boot",
        "microphone_generation",
        "observed_us",
        "expires_at_us",
        "permitted",
    ] {
        let mut missing = json.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<MicrophonePrivacy>(missing).is_err(),
            "missing {field}"
        );
    }
}

#[test]
fn a_lamp_with_no_slide_permits_only_with_its_marker_and_a_fresh_observation() {
    let good = no_slide();
    good.check().unwrap();
    assert!(good.fresh_at("boot", 144_999));
    assert!(!good.fresh_at("boot", 145_000));
    MicrophonePrivacy::from_message(&message(&good), "boot", 100_000, 1000).unwrap();
    // Closed by the controller: still a valid lease with its observation.
    let mut denied = good.clone();
    denied.permitted = false;
    denied.check().unwrap();
    for mutation in 0..7 {
        let mut value = good.clone();
        match mutation {
            // The marker never comes with a physical level.
            0 => value.physical_muted = Some(false),
            1 => value.physical_muted = Some(true),
            // Its observation is fenced as a switch reading is.
            2 => value.observed_us = 0,
            3 => value.observed_us = 100_001,
            4 => value.observed_us = 50_000,
            5 => {
                value.observed_us = 90_000;
                value.expires_at_us = 140_001;
            }
            // Unknown physical state without the marker is no permit.
            _ => value.no_slide = false,
        }
        assert!(value.check().is_err(), "mutation {mutation}");
    }
    let mut unknown = good.clone();
    unknown.no_slide = false;
    unknown.observed_us = 0;
    assert!(unknown.check().is_err(), "a permit with nothing observed");
    unknown.permitted = false;
    unknown.check().unwrap();
    // A slide lamp's lease says nothing of it; a no-slide lease says so.
    let slide = serde_json::to_value(lease()).unwrap();
    assert!(slide.get("no_slide").is_none());
    let wire = serde_json::to_value(&good).unwrap();
    assert_eq!(wire["no_slide"], serde_json::json!(true));
    assert_eq!(wire["physical_muted"], serde_json::Value::Null);
    assert_eq!(
        serde_json::from_value::<MicrophonePrivacy>(wire).unwrap(),
        good
    );
}
