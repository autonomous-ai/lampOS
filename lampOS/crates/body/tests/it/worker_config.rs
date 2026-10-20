use lamp_body::runtime::{channel::Channels, config::*, wire::Control};
use lamp_turn_owner::{BootId, MonoTime};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
fn config() -> Config {
    Config {
        device: "/dev/explicit-test-only".into(),
        lock_directory: "/root/private-lock".into(),
        expected_device: Identity {
            filesystem_device: 1,
            inode: 2,
            character_device: 3,
            major: 4,
            minor: 5,
        },
        unit: "inert-unit".into(),
        calibration: std::array::from_fn(|i| Calibration {
            joint: i as u8 + 1,
            id: i as u16 + 1,
            drive_mode: 0,
            homing_offset: 0,
            range_min: 1000,
            range_max: 3000,
        }),
        register_binding: std::array::from_fn(|i| RegisterBinding {
            joint: i as u8 + 1,
            minimum: 1000,
            maximum: 3000,
            homing_offset: 0,
        }),
        qualification: Qualification {
            selected_profiles: None,
            record: [7; 32],
            valid_from_us: 1,
            expires_at_us: 1_000_000,
            joints: [Envelope {
                minimum: -70.0,
                maximum: 70.0,
                velocity: 100.0,
                acceleration: 1000.0,
                raw_acceleration: 10,
                raw_goal_time: 0,
                raw_speed: 500,
                maximum_load_raw: 100,
                maximum_velocity_raw: 1000,
                tracking_error: 10.0,
                settling_error: 0.5,
            }; 5],
        },
        timing: Bounds {
            transaction_us: 40_000,
            io_us: 5_000,
            readback_age_us: 200_000,
            placement_age_us: 500_000,
            interval_us: 10_000,
        },
        placement_source: BootId::new([3; 16]).unwrap(),
        maximum_runtime_us: 600_000_000,
    }
}
#[test]
fn explicit_head_only_policy_binds_inactive_zero_profiles_without_qualifying_them() {
    let mut value = serde_json::to_value(config()).unwrap();
    for i in 0..4 {
        value["qualification"]["joints"][i]["raw_speed"] = serde_json::json!(0);
        value["qualification"]["joints"][i]["raw_acceleration"] = serde_json::json!(0);
    }
    value["qualification"]["selected_profiles"] = serde_json::json!({"selection":16,"prepare_from":[
        {"acceleration":0,"goal_time":0,"speed":0},
        {"acceleration":0,"goal_time":0,"speed":0},
        {"acceleration":0,"goal_time":0,"speed":0},
        {"acceleration":0,"goal_time":0,"speed":0},
        {"acceleration":0,"goal_time":0,"speed":0}
    ]});
    let c: Config = serde_json::from_value(value.clone()).expect("explicit selected binding");
    c.validate()
        .expect("inactive zero binding is not an active profile");
    value["qualification"]
        .as_object_mut()
        .unwrap()
        .remove("selected_profiles");
    assert!(
        serde_json::from_value::<Config>(value)
            .unwrap()
            .validate()
            .is_err()
    );
}
#[test]
fn provisioning_rejects_hidden_defaults_unqualified_limits_and_narrowing() {
    let c = config();
    c.validate().unwrap();
    let mut value = serde_json::to_value(&c).unwrap();
    value["calibration"][0]["range_min"] = serde_json::json!(-1);
    assert!(
        serde_json::from_value::<Config>(value)
            .unwrap()
            .validate()
            .is_err()
    );
    let mut bad = c.clone();
    bad.qualification.joints[0].raw_speed = 0;
    assert!(bad.validate().is_err());
    bad = c.clone();
    bad.qualification.record = [0; 32];
    assert!(bad.validate().is_err());
    bad = c.clone();
    bad.calibration[0].id = 257;
    assert!(bad.validate().is_err());
    bad = c.clone();
    bad.timing.io_us = 20_001;
    assert!(bad.validate().is_err());
    bad = c;
    bad.device = PathBuf::from("relative");
    assert!(bad.validate().is_err());
    assert!(serde_json::from_str::<Config>("{}").is_err());
    assert!(serde_json::from_str::<Control>(r#"{"kind":"stop","torque":false}"#).is_err());
}
#[test]
fn bounded_private_file_is_required_before_any_device_open() {
    let path = PathBuf::from("/tmp").join(format!(
        "motor-config-{}",
        lamp_body::runtime::supervisor::boot_text(
            lamp_body::runtime::supervisor::new_boot().unwrap()
        )
    ));
    let bytes = serde_json::to_vec(&config()).unwrap();
    fs::write(&path, &bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    Config::read(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(Config::read(&path).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, vec![b' '; 16_385]).unwrap();
    assert!(Config::read(&path).is_err());
    fs::remove_file(path).unwrap();
}
#[test]
fn transport_preserves_original_sequence_age_and_boot() {
    let parent = BootId::new([1; 16]).unwrap();
    let worker = BootId::new([2; 16]).unwrap();
    let (mut p, mut w) = Channels::pair(parent, worker).unwrap();
    p.control
        .send(&Control::Stop {}, MonoTime::from_micros(1))
        .unwrap();
    assert!(
        w.control
            .receive::<Control>(MonoTime::from_micros(100_002))
            .is_err()
    );
    let (mut p, mut w) = Channels::pair(parent, worker).unwrap();
    p.control
        .send(&Control::Stop {}, MonoTime::from_micros(2))
        .unwrap();
    assert!(
        w.control
            .receive::<Control>(MonoTime::from_micros(1))
            .is_err()
    );
}

#[test]
fn receive_with_reads_the_clock_after_the_packet_arrives() {
    // #914: a packet sent after the receiver's clock read is not future-dated.
    let parent = BootId::new([1; 16]).unwrap();
    let worker = BootId::new([2; 16]).unwrap();
    let (mut p, mut w) = Channels::pair(parent, worker).unwrap();
    let read_before = MonoTime::from_micros(900);
    p.control
        .send(&Control::Stop {}, MonoTime::from_micros(1_000))
        .unwrap();
    let mut reads = 0;
    assert!(matches!(
        w.control.receive_with::<Control>(|| {
            reads += 1;
            Ok(MonoTime::from_micros(1_000))
        }),
        Ok(Some(Control::Stop {}))
    ));
    assert_eq!(reads, 1);
    assert!(
        w.control
            .receive_with::<Control>(|| panic!("no packet, no clock read"))
            .unwrap()
            .is_none()
    );
    let (mut p, mut w) = Channels::pair(parent, worker).unwrap();
    p.control
        .send(&Control::Stop {}, MonoTime::from_micros(1_000))
        .unwrap();
    assert!(w.control.receive::<Control>(read_before).is_err());
}

#[test]
fn oversized_serialization_does_not_consume_the_next_channel_sequence() {
    let parent = BootId::new([1; 16]).unwrap();
    let child = BootId::new([2; 16]).unwrap();
    let (mut p, mut w) = Channels::pair(parent, child).unwrap();
    assert!(
        p.control
            .send(&"x".repeat(8193), MonoTime::from_micros(1))
            .is_err()
    );
    assert!(
        w.control
            .receive::<Control>(MonoTime::from_micros(1))
            .unwrap()
            .is_none()
    );
    p.control
        .send(&Control::Stop {}, MonoTime::from_micros(2))
        .unwrap();
    assert!(matches!(
        w.control
            .receive::<Control>(MonoTime::from_micros(2))
            .unwrap(),
        Some(Control::Stop {})
    ));
}

fn matched_registers(
    joint: lamp_body::Joint,
    window: lamp_body::protocol::ReadWindow,
    bytes: &[u8],
) -> lamp_body::protocol::MatchedReply {
    use lamp_body::protocol::{ReadOnlyRequest, StatusParser};
    let mut frame = vec![255, 255, joint.id(), bytes.len() as u8 + 2, 0];
    frame.extend(bytes);
    frame.push(!frame[2..].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    let mut tracker = ReadOnlyRequest::read(joint, window).track_replies();
    let mut result = None;
    StatusParser::default().feed(&frame, |event| {
        result = Some(tracker.accept(event).unwrap())
    });
    tracker.finish().unwrap();
    result.unwrap()
}

#[test]
fn observed_4ace_shared_normalization_and_eeprom_are_distinct_explicit_inputs() {
    use lamp_body::{Joint, protocol::ReadWindow, units::EncoderCounts};
    // Recorded read-only on 4ace, 2026-10-11 23:44 UTC; no profile, travel,
    // velocity, acceleration, placement or mechanical safety claim follows.
    let shared = [(847, 3275), (0, 4095), (0, 4095), (621, 3254), (1464, 3071)];
    let observed = [
        (832, 3268, -1754),
        (0, 4095, 1434),
        (0, 4095, 1339),
        (823, 3057, -1823),
        (1657, 3098, -1880),
    ];
    let positions = [2015, 2048, 2047, 2039, 1765];
    let mut data = serde_json::to_value(config()).unwrap();
    data["calibration"] =
        serde_json::json!(shared.iter().enumerate().map(|(i,(min,max))| serde_json::json!({
        "joint":i+1,"id":i+1,"drive_mode":0,"homing_offset":0,"range_min":min,"range_max":max
    })).collect::<Vec<_>>());
    data["register_binding"] = serde_json::json!(
        observed
            .iter()
            .enumerate()
            .map(|(i, (min, max, homing))| serde_json::json!({
                "joint":i+1,"minimum":min,"maximum":max,"homing_offset":homing
            }))
            .collect::<Vec<_>>()
    );
    let config: Config = serde_json::from_value(data.clone())
        .expect("shared normalization requires explicit separate EEPROM binding");
    let validated = config.validate().unwrap();
    let bound = validated.calibration.bind(&validated.unit).unwrap();
    for joint in Joint::ALL {
        let i = joint.index();
        let (min, max, offset) = observed[i];
        let limits = matched_registers(
            joint,
            ReadWindow::POSITION_LIMITS,
            &[(min as u16).to_le_bytes(), (max as u16).to_le_bytes()].concat(),
        )
        .position_limits()
        .unwrap();
        let word = if offset < 0 {
            (-offset as u16) | 0x800
        } else {
            offset as u16
        };
        let homing = matched_registers(
            joint,
            ReadWindow::HOMING_AND_MODE,
            &[word as u8, (word >> 8) as u8, 0],
        )
        .homing_and_mode()
        .unwrap();
        let calibration = bound.joint(joint);
        calibration
            .compare_registers(limits, homing)
            .expect("actual EEPROM must not be compared to the shared JSON span or JSON0 homing");
        let expected =
            (f64::from(positions[i] - shared[i].0) / f64::from(shared[i].1 - shared[i].0)) * 200.0
                - 100.0;
        assert!(
            (calibration
                .normalize(EncoderCounts::new(positions[i]).unwrap())
                .unwrap()
                .get()
                - expected)
                .abs()
                < 1e-10
        );
        assert_eq!(
            calibration.homing_offset().get(),
            0,
            "source JSON retained, not rewritten"
        );
        let changed = matched_registers(
            joint,
            ReadWindow::HOMING_AND_MODE,
            &[(word ^ 1) as u8, ((word ^ 1) >> 8) as u8, 0],
        )
        .homing_and_mode()
        .unwrap();
        assert!(
            calibration.compare_registers(limits, changed).is_err(),
            "hardware changes remain rejected"
        );
    }
    data.as_object_mut().unwrap().remove("register_binding");
    assert!(
        serde_json::from_value::<Config>(data).is_err(),
        "no auto-probe or calibration fallback"
    );
}

#[test]
fn separate_register_binding_rejects_missing_duplicate_and_invalid_hardware() {
    let original = config();
    for (field, value) in [
        ("minimum", serde_json::json!(-1)),
        ("maximum", serde_json::json!(4096)),
        ("homing_offset", serde_json::json!(3000)),
    ] {
        let mut data = serde_json::to_value(&original).unwrap();
        data["register_binding"][0][field] = value;
        assert!(
            serde_json::from_value::<Config>(data)
                .unwrap()
                .validate()
                .is_err()
        );
    }
    let mut duplicate = original.clone();
    duplicate.register_binding[1].joint = 1;
    assert!(duplicate.validate().is_err());
    let mut swapped = original;
    swapped.register_binding.swap(0, 4);
    assert!(
        swapped.validate().is_ok(),
        "explicit typed keys, not array order, bind the observations"
    );
}
