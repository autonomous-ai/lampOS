use lamp_bus::Kind;
use lamp_msgs::{
    ClockStamp, SCHEDULE_MAX_ITEMS, SCHEDULE_MAX_UNIX_MS, ScheduleDue, ScheduleNotice,
    declaration_of, first_topics, schedule_topics,
};
use serde_json::{Value, json};

fn notice() -> ScheduleNotice {
    ScheduleNotice {
        id: 7,
        delivery: 11,
        due_unix_ms: 1_791_964_800_000,
        late_by_ms: Some(1_234),
        late: true,
    }
}

fn due() -> ScheduleDue {
    ScheduleDue {
        stamp: ClockStamp {
            clock_id: "schedule-test-boot".into(),
            at_us: 900_000,
            sequence: 3,
        },
        store_id: "0123456789abcdef0123456789abcdef".into(),
        revision: 5,
        enabled: true,
        ready: true,
        pending: 2,
        waiting_for_clock: 1,
        oldest: Some(notice()),
    }
}

#[test]
fn due_round_trip_carries_metadata_and_ignores_additive_fields_at_every_level() {
    let expected = due();
    expected.check().unwrap();
    let text = serde_json::to_string(&expected).unwrap();
    assert_eq!(
        text,
        r#"{"stamp":{"clock_id":"schedule-test-boot","at_us":900000,"sequence":3},"store_id":"0123456789abcdef0123456789abcdef","revision":5,"enabled":true,"ready":true,"pending":2,"waiting_for_clock":1,"oldest":{"id":7,"delivery":11,"due_unix_ms":1791964800000,"late_by_ms":1234,"late":true}}"#
    );
    assert_eq!(
        serde_json::from_str::<ScheduleDue>(&text).unwrap(),
        expected
    );
    let mut value: Value = serde_json::from_str(&text).unwrap();
    value["future"] = json!({"status": "new"});
    value["stamp"]["future"] = json!([1, 2]);
    value["oldest"]["future"] = json!("new notice field");
    assert_eq!(
        serde_json::from_value::<ScheduleDue>(value).unwrap(),
        expected
    );
}

#[test]
fn optional_unknown_lateness_and_withdrawn_notice_are_omitted_and_round_trip() {
    let mut unknown = notice();
    unknown.late_by_ms = None;
    let value = serde_json::to_value(&unknown).unwrap();
    assert!(value.get("late_by_ms").is_none());
    assert_eq!(
        serde_json::from_value::<ScheduleNotice>(value).unwrap(),
        unknown
    );

    let mut withdrawn = due();
    withdrawn.ready = false;
    withdrawn.oldest = None;
    withdrawn.check().unwrap();
    let value = serde_json::to_value(&withdrawn).unwrap();
    assert!(value.get("oldest").is_none());
    assert_eq!(
        serde_json::from_value::<ScheduleDue>(value).unwrap(),
        withdrawn
    );
}

#[test]
fn all_nonoptional_fields_are_required_including_nested_clock_and_notice() {
    for field in [
        "stamp",
        "store_id",
        "revision",
        "enabled",
        "ready",
        "pending",
        "waiting_for_clock",
    ] {
        let mut value = serde_json::to_value(due()).unwrap();
        value.as_object_mut().unwrap().remove(field).unwrap();
        assert!(
            serde_json::from_value::<ScheduleDue>(value).is_err(),
            "{field}"
        );
    }
    for field in ["clock_id", "at_us", "sequence"] {
        let mut value = serde_json::to_value(due()).unwrap();
        value["stamp"]
            .as_object_mut()
            .unwrap()
            .remove(field)
            .unwrap();
        assert!(
            serde_json::from_value::<ScheduleDue>(value).is_err(),
            "stamp.{field}"
        );
    }
    for field in ["id", "delivery", "due_unix_ms", "late"] {
        let mut value = serde_json::to_value(due()).unwrap();
        value["oldest"]
            .as_object_mut()
            .unwrap()
            .remove(field)
            .unwrap();
        assert!(
            serde_json::from_value::<ScheduleDue>(value).is_err(),
            "oldest.{field}"
        );
    }
}

#[test]
fn store_identity_is_exact_lowercase_hex_not_an_arbitrary_identifier() {
    for store in [
        "".to_owned(),
        "a".repeat(31),
        "a".repeat(33),
        "A".repeat(32),
        "g".repeat(32),
        "é".repeat(16),
        format!("{} ", "a".repeat(31)),
        format!("{}\n", "0".repeat(31)),
    ] {
        let mut value = due();
        value.store_id = store;
        assert!(value.check().is_err());
    }
    for store in ["0".repeat(32), "f".repeat(32)] {
        let mut value = due();
        value.store_id = store;
        value.check().unwrap();
    }
}

#[test]
fn clock_shape_uses_the_existing_contract_without_inventing_a_clock_or_sequence() {
    for clock in ["".to_owned(), "a".repeat(129), "boot\nother".into()] {
        let mut value = due();
        value.stamp.clock_id = clock;
        assert!(value.check().is_err());
    }
    let mut value = due();
    value.stamp.clock_id = "a".repeat(128);
    value.stamp.at_us = u64::MAX;
    value.stamp.sequence = 0;
    value.revision = 0;
    value.check().unwrap();
    // Structural validation neither reads a clock nor establishes freshness.
}

#[test]
fn notice_ids_are_positive_and_due_dates_have_explicit_inclusive_bounds() {
    for (id, delivery) in [(0, 1), (1, 0), (0, 0)] {
        let mut value = notice();
        value.id = id;
        value.delivery = delivery;
        assert!(value.check().is_err());
        let mut state = due();
        state.oldest = Some(value);
        assert!(state.check().is_err());
    }
    for at in [0, SCHEDULE_MAX_UNIX_MS] {
        let mut value = notice();
        value.id = u64::MAX;
        value.delivery = u64::MAX;
        value.due_unix_ms = at;
        value.check().unwrap();
    }
    for at in [i64::MIN, -1, SCHEDULE_MAX_UNIX_MS + 1, i64::MAX] {
        let mut value = notice();
        value.due_unix_ms = at;
        assert!(value.check().is_err());
    }
}

#[test]
fn counts_share_one_bound_and_extreme_unsigned_values_cannot_overflow_the_check() {
    for (pending, waiting) in [(0, 0), (0, 128), (128, 0), (64, 64), (1, 127)] {
        let mut value = due();
        value.pending = pending;
        value.waiting_for_clock = waiting;
        value.oldest = (pending > 0).then(notice);
        value.check().unwrap();
    }
    for (pending, waiting) in [
        (SCHEDULE_MAX_ITEMS + 1, 0),
        (0, SCHEDULE_MAX_ITEMS + 1),
        (128, 1),
        (1, 128),
        (u32::MAX, 1),
        (1, u32::MAX),
        (u32::MAX, u32::MAX),
    ] {
        let mut value = due();
        value.pending = pending;
        value.waiting_for_clock = waiting;
        assert!(value.check().is_err(), "{pending} + {waiting}");
    }
}

#[test]
fn oldest_is_present_if_and_only_if_enabled_ready_and_pending() {
    for enabled in [false, true] {
        for ready in [false, true] {
            for pending in [0, 1] {
                for present in [false, true] {
                    let mut value = due();
                    value.enabled = enabled;
                    value.ready = ready;
                    value.pending = pending;
                    value.oldest = present.then(notice);
                    let expected = present == (enabled && ready && pending > 0);
                    assert_eq!(
                        value.check().is_ok(),
                        expected,
                        "enabled={enabled}, ready={ready}, pending={pending}, present={present}"
                    );
                }
            }
        }
    }
}

#[test]
fn unknown_lateness_is_not_converted_to_an_on_time_claim() {
    for late in [false, true] {
        for late_by_ms in [None, Some(0), Some(u64::MAX)] {
            let mut value = notice();
            value.late = late;
            value.late_by_ms = late_by_ms;
            value.check().unwrap();
            assert_eq!(
                serde_json::from_str::<ScheduleNotice>(&serde_json::to_string(&value).unwrap())
                    .unwrap(),
                value
            );
        }
    }
}

#[test]
fn malformed_numeric_or_enum_like_boolean_values_are_not_guessed() {
    for (field, bad) in [
        ("revision", json!(-1)),
        ("revision", json!(1.5)),
        ("pending", json!(-1)),
        ("pending", json!(u64::MAX)),
        ("pending", json!("1")),
        ("waiting_for_clock", json!(0.5)),
        ("enabled", json!("enabled")),
        ("ready", json!("ready")),
        ("ready", json!(1)),
        ("store_id", json!(null)),
    ] {
        let mut value = serde_json::to_value(due()).unwrap();
        value[field] = bad;
        assert!(
            serde_json::from_value::<ScheduleDue>(value).is_err(),
            "{field}"
        );
    }
    for (field, bad) in [
        ("id", json!(-1)),
        ("delivery", json!(1.5)),
        ("due_unix_ms", json!(u64::MAX)),
        ("due_unix_ms", json!("15:00")),
        ("late_by_ms", json!(-1)),
        ("late_by_ms", json!(0.5)),
        ("late", json!("late")),
        ("late", json!(null)),
    ] {
        let mut value = serde_json::to_value(due()).unwrap();
        value["oldest"][field] = bad;
        assert!(
            serde_json::from_value::<ScheduleDue>(value).is_err(),
            "oldest.{field}"
        );
    }
    let oversized = serde_json::to_string(&due())
        .unwrap()
        .replace("\"revision\":5", "\"revision\":18446744073709551616");
    assert!(serde_json::from_str::<ScheduleDue>(&oversized).is_err());
}

#[test]
fn missing_optional_notice_still_fails_validation_when_due_work_is_offered() {
    let mut value = serde_json::to_value(due()).unwrap();
    value.as_object_mut().unwrap().remove("oldest");
    assert!(
        serde_json::from_value::<ScheduleDue>(value)
            .unwrap()
            .check()
            .is_err()
    );
    let mut value = serde_json::to_value(due()).unwrap();
    value["oldest"] = Value::Null;
    assert!(
        serde_json::from_value::<ScheduleDue>(value)
            .unwrap()
            .check()
            .is_err()
    );
}

#[test]
fn discovery_is_additive_and_declares_one_latest_topic_with_a_three_second_lifespan() {
    assert_eq!(ScheduleDue::TYPE, "lamp_msgs/ScheduleDue");
    assert_eq!(ScheduleDue::VERSION, 1);
    assert_eq!(ScheduleDue::TOPIC, "schedule/due");
    let declaration = ScheduleDue::declaration();
    assert_eq!(declaration.type_name, ScheduleDue::TYPE);
    assert_eq!(declaration.version, 1);
    assert_eq!(declaration.kind, Kind::Latest);
    assert_eq!(declaration.period_ms, Some(1_000));
    assert_eq!(declaration.lifespan_ms, Some(3_000));
    assert_eq!(
        schedule_topics(),
        vec![(ScheduleDue::TOPIC, declaration.clone())]
    );
    assert_eq!(declaration_of(ScheduleDue::TOPIC), Some(declaration));
    assert_eq!(first_topics().len(), 17);
    assert!(
        first_topics()
            .iter()
            .all(|(name, _)| *name != ScheduleDue::TOPIC)
    );
    assert!(declaration_of("schedule/unknown").is_none());
}
