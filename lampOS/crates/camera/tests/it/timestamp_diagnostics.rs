use lamp_camera::{
    DriverTimestamp, TimestampDomain, TimestampPoint, TimestampPredicate, TimestampRejection,
};

fn largest_record() -> TimestampRejection {
    let timestamp = DriverTimestamp {
        micros: u64::MAX,
        domain: TimestampDomain::HostMonotonic,
        point: TimestampPoint::StartOfExposure,
    };
    TimestampRejection {
        capture_epoch: u64::MAX,
        driver_sequence: u32::MAX,
        timestamp,
        capture_started_at_us: u64::MAX,
        dequeue_started_at_us: u64::MAX,
        comparison_at_us: u64::MAX,
        maximum_age_us: u64::MAX,
        previous_timestamp: Some(timestamp),
        predicate: TimestampPredicate::BeforeCaptureStart,
    }
}

#[test]
fn timestamp_diagnostic_has_fixed_storage_and_bounded_closed_schema() {
    fn is_copy<T: Copy>() {}
    is_copy::<TimestampRejection>();
    assert!(!std::mem::needs_drop::<Option<TimestampRejection>>());
    assert!(std::mem::size_of::<Option<TimestampRejection>>() <= 160);
    let value = serde_json::to_value(largest_record()).unwrap();
    let keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "capture_epoch",
            "capture_started_at_us",
            "comparison_at_us",
            "dequeue_started_at_us",
            "driver_sequence",
            "maximum_age_us",
            "predicate",
            "previous_timestamp",
            "timestamp",
        ]
    );
    assert!(serde_json::to_vec(&value).unwrap().len() <= 1024);
    assert_eq!(
        serde_json::from_value::<TimestampRejection>(value).unwrap(),
        largest_record()
    );
}

#[test]
fn timestamp_diagnostic_cannot_carry_extra_image_or_credential_fields() {
    for field in ["jpeg", "pixels", "credential"] {
        let mut value = serde_json::to_value(largest_record()).unwrap();
        value[field] = serde_json::json!("synthetic-secret-marker");
        assert!(serde_json::from_value::<TimestampRejection>(value).is_err());
        let mut value = serde_json::to_value(largest_record()).unwrap();
        value["timestamp"][field] = serde_json::json!("synthetic-secret-marker");
        assert!(serde_json::from_value::<TimestampRejection>(value).is_err());
    }
}

#[test]
fn startup_discard_summary_is_fixed_bounded_metadata_not_image_storage() {
    use lamp_camera::{StartupDiscard, StartupDiscards};
    fn is_copy<T: Copy>() {}
    is_copy::<StartupDiscards>();
    assert!(!std::mem::needs_drop::<Option<StartupDiscards>>());
    assert!(std::mem::size_of::<Option<StartupDiscards>>() <= 320);
    let sample = StartupDiscard {
        comparison: largest_record(),
        age_us: u64::MAX,
    };
    let summary = StartupDiscards {
        count: u64::MAX,
        first: sample,
        latest: sample,
    };
    let value = serde_json::to_value(summary).unwrap();
    assert!(serde_json::to_vec(&value).unwrap().len() <= 2048);
    assert_eq!(
        serde_json::from_value::<StartupDiscards>(value.clone()).unwrap(),
        summary
    );
    for field in ["pixels", "jpeg", "credential"] {
        let mut bad = value.clone();
        bad["latest"][field] = serde_json::json!("private-marker");
        assert!(serde_json::from_value::<StartupDiscards>(bad).is_err());
    }
}
