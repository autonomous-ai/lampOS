use lamp_bus::{Header, Kind, Message};
use lamp_camera::frames::FrameMeta;
use lamp_msgs::{Audio, AudioCapturePermit, ClockStamp, MicrophonePrivacy};
use lamp_sensor5_recorder::{Admission, Warmup, consent};
use lamp_turn_owner::{BootId, Controller, MonoTime, Permission};
use serde_json::json;

fn lease(sequence: u64, now: u64, permitted: bool) -> Message {
    let value = MicrophonePrivacy {
        stamp: ClockStamp {
            clock_id: "test-clock".into(),
            at_us: now,
            sequence,
        },
        session_id: "1111111111111111".into(),
        controller_boot: "22222222222222222222222222222222".into(),
        microphone_generation: 1,
        observed_us: now,
        expires_at_us: now + 50_000,
        permitted,
        physical_muted: Some(!permitted),
        no_slide: false,
    };
    Message {
        header: Header {
            topic: MicrophonePrivacy::TOPIC.into(),
            kind: Kind::Latest,
            enc: "json".into(),
            type_name: Some(MicrophonePrivacy::TYPE.into()),
            source: "voice".into(),
            boot: value.session_id.clone(),
            seq: sequence,
            at_us: now,
            at_unix_ms: 1_000,
            expires_ms: Some(50),
            clock: None,
        },
        payload: serde_json::to_vec(&value).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

fn live() -> Admission {
    let mut admission = Admission::new("test-clock".into(), 100);
    admission
        .observe(&lease(1, 1_000, true), 1_000, 1_000)
        .unwrap();
    admission
}

fn frame(sequence: u64, at: u64) -> FrameMeta {
    let mut owner = Controller::new(BootId::new([1; 16]).unwrap(), MonoTime::from_micros(100));
    owner
        .set_camera_permission(MonoTime::from_micros(100), Permission::Allowed)
        .unwrap();
    FrameMeta {
        request: sequence,
        camera_worker: BootId::new([2; 16]).unwrap(),
        capture_epoch: 1,
        frame_sequence: sequence,
        grant: owner.camera_grant(MonoTime::from_micros(100)).unwrap(),
        source: "lamp-eyes".into(),
        format: *b"MJPG",
        width: 640,
        height: 480,
        driver_sequence: sequence as u32,
        driver_timestamp: None,
        dequeue_started_at_us: at - 10,
        dequeue_completed_at_us: at,
        copied_at_us: at + 1,
        derived: None,
    }
}

fn audio(sequence: u64, at: u64) -> Message {
    let a = Audio {
        sample_rate: 16_000,
        channels: 1,
        sample_format: Audio::S16LE.into(),
        sequence,
    };
    Message {
        header: Header {
            topic: "mic1/audio".into(),
            kind: Kind::Event,
            enc: Audio::ENCODING.into(),
            type_name: Some(Audio::TYPE.into()),
            source: "ears".into(),
            boot: "3333333333333333".into(),
            seq: sequence + 1,
            at_us: at,
            at_unix_ms: 1_000,
            expires_ms: Some(50),
            clock: None,
        },
        payload: a
            .block_with_capture_permit(
                &[0; 640],
                &AudioCapturePermit {
                    clock_id: "test-clock".into(),
                    epoch: 42,
                    until_us: at + 50_000,
                },
            )
            .unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

const JPEG: &[u8] = &[0xff, 0xd8, 0, 1, 0xff, 0xd9];

#[test]
fn missing_stale_revoked_and_later_lease_cannot_admit_old_ticket() {
    assert!(
        Admission::new("test-clock".into(), 100)
            .ticket(1_000)
            .is_err()
    );
    let mut admission = live();
    let original = admission.ticket(2_000).unwrap();
    admission
        .observe(&lease(2, 40_000, true), 40_000, 1_000)
        .unwrap();
    assert!(
        admission
            .final_check(original, 2_000, None, 51_000)
            .is_err()
    );
    assert!(
        admission
            .observe(&lease(3, 52_000, true), 52_000, 1_000)
            .is_err()
    );
    let mut admission = live();
    assert!(admission.ticket(51_000).is_err());
    let mut admission = live();
    let queued = admission.ticket(2_000).unwrap();
    assert!(
        admission
            .observe(&lease(2, 3_000, false), 3_000, 1_000)
            .is_err()
    );
    assert!(admission.final_check(queued, 2_000, None, 3_001).is_err());
}

#[test]
fn source_age_is_rechecked_after_encoding_even_under_a_fresh_renewed_lease() {
    for (age, allowed) in [(49_999, true), (50_000, false)] {
        let mut admission = live();
        admission.frame(&frame(1, 2_000), JPEG, 2_010).unwrap();
        admission
            .observe(&lease(2, 40_000, true), 40_000, 1_000)
            .unwrap();
        let ticket = admission.ticket(40_010).unwrap();
        assert_eq!(
            admission
                .final_check(ticket, 2_000, None, 2_000 + age)
                .is_ok(),
            allowed
        );
    }
}

fn warmed() -> Admission {
    let mut admission = live();
    let mut warmup = Warmup::new(0);
    assert_eq!(warmup.step(&mut admission, 1_000).unwrap(), None);
    admission
        .observe(&lease(2, 40_000, true), 40_000, 1_000)
        .unwrap();
    assert_eq!(warmup.step(&mut admission, 50_999).unwrap(), None);
    assert_eq!(warmup.step(&mut admission, 51_000).unwrap(), Some(51_000));
    admission
}

#[test]
fn continuous_warmup_sets_a_new_capture_boundary_and_rejects_older_completions() {
    let mut admission = warmed();
    let result = admission.frame(&frame(1, 51_000), JPEG, 51_010).unwrap();
    assert_eq!(result["capture_started_us"], 51_000);
    assert!(warmed().frame(&frame(1, 50_999), JPEG, 51_010).is_err());
    assert!(
        warmed()
            .audio(&audio(1, 50_999), "test-clock", 51_010)
            .is_err()
    );
}

#[test]
fn missing_initial_lease_stale_warmup_and_restart_never_retry() {
    let mut admission = Admission::new("test-clock".into(), 0);
    let mut warmup = Warmup::new(0);
    assert_eq!(warmup.step(&mut admission, 999_999).unwrap(), None);
    assert!(warmup.step(&mut admission, 1_000_000).is_err());
    assert!(
        admission
            .observe(&lease(1, 1_000_001, true), 1_000_001, 1_000)
            .is_err()
    );
    let mut admission = live();
    let mut warmup = Warmup::new(0);
    assert_eq!(warmup.step(&mut admission, 1_000).unwrap(), None);
    assert!(warmup.step(&mut admission, 51_000).is_err());
    assert!(
        admission
            .observe(&lease(2, 51_001, true), 51_001, 1_000)
            .is_err()
    );
    let mut admission = live();
    let mut warmup = Warmup::new(0);
    warmup.step(&mut admission, 1_000).unwrap();
    let mut restarted = lease(2, 2_000, true);
    restarted.publisher_restarted = true;
    assert!(admission.observe(&restarted, 2_000, 1_000).is_err());
    assert!(warmup.step(&mut admission, 2_001).is_err());
}

#[test]
fn privacy_reorder_gap_restart_fault_and_clock_regression_stop_clip() {
    for alteration in 0..5 {
        let mut admission = live();
        let mut next = lease(2, 2_000, true);
        match alteration {
            0 => next = lease(1, 2_000, true),
            1 => next.gap = 1,
            2 => next.publisher_restarted = true,
            3 => next.payload = b"{}".to_vec(),
            _ => next = lease(2, 999, true),
        }
        assert!(
            admission.observe(&next, 2_000, 1_000).is_err(),
            "case {alteration}"
        );
        assert!(admission.ticket(2_001).is_err());
    }
}

#[test]
fn originals_are_kept_and_native_gaps_are_explicit() {
    let mut admission = live();
    let first = frame(1, 2_000);
    let value = admission.frame(&first, JPEG, 2_010).unwrap();
    assert_eq!(value["frame"], serde_json::to_value(&first).unwrap());
    let value = admission.frame(&frame(4, 3_000), JPEG, 3_010).unwrap();
    assert_eq!(value["source_frames_between"], 2);
    let first_audio = audio(10, 4_000);
    let value = admission.audio(&first_audio, "test-clock", 4_010).unwrap();
    assert_eq!(
        value["header"],
        serde_json::to_value(&first_audio.header).unwrap()
    );
    assert_eq!(value["adc_timestamp"], false);
    let mut second = audio(13, 5_000);
    second.gap = 2;
    let value = admission.audio(&second, "test-clock", 5_010).unwrap();
    assert_eq!(value["audio_sequence_gap"], 2);
    assert_eq!(value["bus_gap"], 2);
}

#[test]
fn late_frame_wrong_worker_reopen_and_sequence_regression_are_rejected() {
    for alteration in 0..5 {
        let mut admission = live();
        admission.frame(&frame(1, 2_000), JPEG, 2_010).unwrap();
        let mut next = frame(2, 3_000);
        match alteration {
            0 => next.camera_worker = BootId::new([9; 16]).unwrap(),
            1 => next.capture_epoch = 2,
            2 => next.frame_sequence = 1,
            3 => next.dequeue_completed_at_us = 99,
            _ => next.copied_at_us = 9_000,
        }
        assert!(admission.frame(&next, JPEG, 3_010).is_err());
    }
    let mut admission = live();
    admission
        .observe(&lease(2, 40_000, true), 40_000, 1_000)
        .unwrap();
    assert!(admission.frame(&frame(1, 2_000), JPEG, 52_000).is_err());
}

#[test]
fn pcm_restart_clock_change_and_late_block_fail_closed() {
    for alteration in 0..4 {
        let mut admission = live();
        admission
            .audio(&audio(1, 2_000), "test-clock", 2_010)
            .unwrap();
        let mut next = audio(2, 3_000);
        match alteration {
            0 => next.header.boot = "4444444444444444".into(),
            1 => next.header.clock = Some("other-boot".into()),
            2 => next.header.at_us = 99,
            _ => next.publisher_restarted = true,
        }
        assert!(admission.audio(&next, "test-clock", 3_010).is_err());
        assert!(admission.ticket(3_020).is_err());
    }
}

fn with_source_permit(mut message: Message, permit: &AudioCapturePermit) -> Message {
    let (audio, samples) = Audio::split(&message.payload).unwrap();
    message.payload = audio.block_with_capture_permit(samples, permit).unwrap();
    message
}

#[test]
fn pcm_requires_original_ticket_and_its_source_epoch_is_not_the_recorders_epoch() {
    let mut legacy = audio(1, 2_000);
    let (header, samples) = Audio::split(&legacy.payload).unwrap();
    legacy.payload = header.block(samples).unwrap();
    assert!(live().audio(&legacy, "test-clock", 2_010).is_err());
    let mut admission = live();
    assert_ne!(admission.ticket(2_000).unwrap().epoch, 42);
    assert_eq!(
        admission
            .audio(&audio(1, 2_000), "test-clock", 2_010)
            .unwrap()["capture_permit"]["epoch"],
        42
    );
    let changed = with_source_permit(
        audio(2, 3_000),
        &AudioCapturePermit {
            clock_id: "test-clock".into(),
            epoch: 43,
            until_us: 53_000,
        },
    );
    assert!(admission.audio(&changed, "test-clock", 3_010).is_err());
    for permit in [
        AudioCapturePermit {
            clock_id: "other-clock".into(),
            epoch: 42,
            until_us: 52_000,
        },
        AudioCapturePermit {
            clock_id: "test-clock".into(),
            epoch: 0,
            until_us: 52_000,
        },
        AudioCapturePermit {
            clock_id: "test-clock".into(),
            epoch: 42,
            until_us: 2_000,
        },
        AudioCapturePermit {
            clock_id: "test-clock".into(),
            epoch: 42,
            until_us: 2_010,
        },
    ] {
        assert!(
            live()
                .audio(
                    &with_source_permit(audio(1, 2_000), &permit),
                    "test-clock",
                    2_010
                )
                .is_err()
        );
    }
}

#[test]
fn later_voice_lease_never_renews_the_original_ears_capture_expiry() {
    let mut admission = warmed();
    admission
        .observe(&lease(3, 80_000, true), 80_000, 1_000)
        .unwrap();
    let original = AudioCapturePermit {
        clock_id: "test-clock".into(),
        epoch: 42,
        until_us: 90_000,
    };
    let block = with_source_permit(audio(1, 85_000), &original);
    admission.audio(&block, "test-clock", 87_000).unwrap();
    let renewed = admission.ticket(87_000).unwrap();
    assert_eq!(renewed.until_us, 130_000);
    assert!(
        admission
            .final_check(renewed, 85_000, Some(&original), 94_002)
            .is_err()
    );
    let mut admission = warmed();
    admission
        .observe(&lease(3, 80_000, true), 80_000, 1_000)
        .unwrap();
    assert!(admission.audio(&block, "test-clock", 94_002).is_err());
}

fn consent_record(text: &[u8]) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema_version":1,"participant_alias":"P01","operator_alias":"O01",
        "accepted":true,"scope":consent::SCOPE,"accepted_at_utc":"2026-11-30T02:00:00Z","consent_text_sha256":consent::sha256(text)})).unwrap()
}

#[test]
fn consent_requires_exact_pilot_alias_acceptance_and_both_hashes() {
    let text = b"Participant alias: P01\nConsent accepted: yes\nParticipant confirmation/signature (local only): synthetic-test-confirmation\n".to_vec();
    let record = consent_record(&text);
    let hash = consent::sha256(&record);
    assert!(consent::validate(record.clone(), text.clone(), &hash, "P01", "O01").is_ok());
    for (alias, operator) in [
        ("P05", "O01"),
        ("P01", "P01"),
        ("P01", "O02"),
        ("person", "O01"),
    ] {
        assert!(consent::validate(record.clone(), text.clone(), &hash, alias, operator).is_err());
    }
    assert!(consent::validate(record.clone(), b"changed".to_vec(), &hash, "P01", "O01").is_err());
    assert!(consent::validate(record.clone(), text, &"0".repeat(64), "P01", "O01").is_err());
    let no = b"Participant alias: P01\nConsent accepted: no\n".to_vec();
    let record = consent_record(&no);
    let hash = consent::sha256(&record);
    assert!(consent::validate(record, no, &hash, "P01", "O01").is_err());
}

#[test]
fn consent_needs_actual_confirmation_and_accepts_helpers_fractional_utc() {
    for confirmation in ["", "   ", "synthetic-test-confirmation"] {
        let text=format!("Participant alias: P01\nConsent accepted: yes\nParticipant confirmation/signature (local only): {confirmation}\n").into_bytes();
        let mut record: serde_json::Value = serde_json::from_slice(&consent_record(&text)).unwrap();
        record["accepted_at_utc"] = json!("2026-11-30T02:00:00.123Z");
        let bytes = serde_json::to_vec(&record).unwrap();
        let hash = consent::sha256(&bytes);
        assert_eq!(
            consent::validate(bytes, text, &hash, "P01", "O01").is_ok(),
            !confirmation.trim().is_empty()
        );
    }
}
