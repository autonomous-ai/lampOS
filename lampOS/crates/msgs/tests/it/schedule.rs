use lamp_bus::Kind;
use lamp_msgs::{
    ClockStamp, SCHEDULE_MAX_ITEMS, SCHEDULE_MAX_UNIX_MS, ScheduleDue, ScheduleHorizon,
    ScheduleHorizonClock, ScheduleHorizonUnknown, ScheduleNotice, declaration_of, first_topics,
    schedule_topics,
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
        horizon: None,
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
fn absent_or_null_horizon_is_unsupported_and_is_never_promoted_to_empty() {
    let old = serde_json::to_value(due()).unwrap();
    assert!(old.get("horizon").is_none());
    let decoded: ScheduleDue = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(decoded.horizon, None);
    decoded.check().unwrap();

    let mut explicit_null = old.clone();
    explicit_null["horizon"] = Value::Null;
    let decoded: ScheduleDue = serde_json::from_value(explicit_null).unwrap();
    assert_eq!(decoded.horizon, None);
    assert_eq!(serde_json::to_value(decoded).unwrap(), old);
}

#[test]
fn horizon_wire_shapes_are_fixed_and_old_readers_ignore_the_new_field() {
    // This reader has exactly the fields supported before the extension.
    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct OldScheduleDue {
        stamp: ClockStamp,
        store_id: String,
        revision: u64,
        enabled: bool,
        ready: bool,
        pending: u32,
        waiting_for_clock: u32,
        #[serde(default)]
        oldest: Option<ScheduleNotice>,
    }

    for (horizon, expected) in [
        (
            ScheduleHorizon::Empty { sampled_boot_ms: 0 },
            json!({"state": "empty", "sampled_boot_ms": 0}),
        ),
        (
            ScheduleHorizon::Next {
                sampled_boot_ms: 900,
                in_ms: 15 * 60 * 1_000,
                clock: ScheduleHorizonClock::Elapsed,
            },
            json!({"state": "next", "sampled_boot_ms": 900, "in_ms": 900_000,
                "clock": {"basis": "elapsed"}}),
        ),
        (
            ScheduleHorizon::Next {
                sampled_boot_ms: 900,
                in_ms: 900_000,
                clock: ScheduleHorizonClock::Wall {
                    sampled_unix_ms: 1_791_964_800_000,
                },
            },
            json!({"state": "next", "sampled_boot_ms": 900, "in_ms": 900_000,
                "clock": {"basis": "wall", "sampled_unix_ms": 1_791_964_800_000i64}}),
        ),
        (
            ScheduleHorizon::Unknown {
                reason: ScheduleHorizonUnknown::Clock,
            },
            json!({"state": "unknown", "reason": "clock"}),
        ),
        (
            ScheduleHorizon::Unknown {
                reason: ScheduleHorizonUnknown::Unread,
            },
            json!({"state": "unknown", "reason": "unread"}),
        ),
        (
            ScheduleHorizon::Unknown {
                reason: ScheduleHorizonUnknown::RetainedStore,
            },
            json!({"state": "unknown", "reason": "retained_store"}),
        ),
        (
            ScheduleHorizon::Unknown {
                reason: ScheduleHorizonUnknown::Unavailable,
            },
            json!({"state": "unknown", "reason": "unavailable"}),
        ),
    ] {
        let mut value = due();
        value.pending = 0;
        value.waiting_for_clock = 0;
        value.oldest = None;
        let legacy: OldScheduleDue =
            serde_json::from_value(serde_json::to_value(&value).unwrap()).unwrap();
        value.horizon = Some(horizon);
        value.check().unwrap();
        let wire = serde_json::to_value(&value).unwrap();
        assert_eq!(wire["horizon"], expected);
        assert_eq!(
            serde_json::from_value::<OldScheduleDue>(wire.clone()).unwrap(),
            legacy
        );
        assert_eq!(serde_json::from_value::<ScheduleDue>(wire).unwrap(), value);
    }
}

#[test]
fn known_horizons_require_an_enabled_ready_owner_and_no_clock_blocked_work() {
    for enabled in [false, true] {
        for ready in [false, true] {
            for waiting in [0, 1] {
                for horizon in [
                    ScheduleHorizon::Empty { sampled_boot_ms: 1 },
                    ScheduleHorizon::Next {
                        sampled_boot_ms: 1,
                        in_ms: 0,
                        clock: ScheduleHorizonClock::Elapsed,
                    },
                    ScheduleHorizon::Next {
                        sampled_boot_ms: 1,
                        in_ms: 0,
                        clock: ScheduleHorizonClock::Wall { sampled_unix_ms: 0 },
                    },
                    ScheduleHorizon::Unknown {
                        reason: ScheduleHorizonUnknown::Unavailable,
                    },
                ] {
                    let mut value = due();
                    value.pending = 0;
                    value.oldest = None;
                    value.enabled = enabled;
                    value.ready = ready;
                    value.waiting_for_clock = waiting;
                    value.horizon = Some(horizon);
                    let expected = matches!(horizon, ScheduleHorizon::Unknown { .. })
                        || (enabled && ready && waiting == 0);
                    assert_eq!(
                        value.check().is_ok(),
                        expected,
                        "enabled={enabled}, ready={ready}, waiting={waiting}, {horizon:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn empty_and_next_horizons_cannot_hide_pending_work_or_overflow_signed_durations() {
    for pending in [0, 1, SCHEDULE_MAX_ITEMS] {
        let mut value = due();
        value.pending = pending;
        value.waiting_for_clock = 0;
        value.oldest = (pending > 0).then(notice);
        value.horizon = Some(ScheduleHorizon::Empty {
            sampled_boot_ms: u64::MAX,
        });
        assert_eq!(value.check().is_ok(), pending == 0);
        for in_ms in [0, 1, i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
            value.horizon = Some(ScheduleHorizon::Next {
                sampled_boot_ms: u64::MAX,
                in_ms,
                clock: ScheduleHorizonClock::Elapsed,
            });
            assert_eq!(
                value.check().is_ok(),
                in_ms <= i64::MAX as u64 && (pending == 0 || in_ms == 0),
                "pending={pending}, in_ms={in_ms}"
            );
        }
    }
    // The boot sample remains a u64. Validating freshness and correspondence
    // with the receiver's boot clock is deliberately not a structural check.
}

#[test]
fn malformed_or_incomplete_horizons_are_refused_without_guessing_a_state() {
    for malformed in [
        json!({}),
        json!("empty"),
        json!([]),
        json!({"state": "future"}),
        json!({"state": null}),
        json!({"state": "empty"}),
        json!({"state": "next", "sampled_boot_ms": 0, "clock": {"basis": "elapsed"}}),
        json!({"state": "next", "in_ms": 0, "clock": {"basis": "elapsed"}}),
        json!({"state": "next", "sampled_boot_ms": 0, "in_ms": 0}),
        json!({"state": "unknown"}),
        json!({"state": "unknown", "reason": "person's reminder"}),
        json!({"state": "unknown", "reason": null}),
        json!({"state": "unknown", "reason": 1}),
    ] {
        let mut value = serde_json::to_value(due()).unwrap();
        value["horizon"] = malformed;
        assert!(serde_json::from_value::<ScheduleDue>(value).is_err());
    }
    for bad in [
        json!(-1),
        json!(0.5),
        json!(1.0),
        json!("0"),
        json!(false),
        Value::Null,
    ] {
        for (state, field) in [
            ("empty", "sampled_boot_ms"),
            ("next", "sampled_boot_ms"),
            ("next", "in_ms"),
        ] {
            let mut value = serde_json::to_value(due()).unwrap();
            value["horizon"] = json!({"state": state, "sampled_boot_ms": 0, "in_ms": 0,
                "clock": {"basis": "elapsed"}});
            value["horizon"][field] = bad.clone();
            assert!(
                serde_json::from_value::<ScheduleDue>(value).is_err(),
                "{state}.{field}={bad}"
            );
        }
    }
    for field in ["sampled_boot_ms", "in_ms"] {
        let horizon =
            r#"{"state":"next","sampled_boot_ms":0,"in_ms":0,"clock":{"basis":"elapsed"}}"#
                .replace(
                    &format!("\"{field}\":0"),
                    &format!("\"{field}\":18446744073709551616"),
                );
        assert!(serde_json::from_str::<ScheduleHorizon>(&horizon).is_err());
    }
}

#[test]
fn next_horizons_require_a_known_clock_basis_and_complete_wall_sample() {
    for clock in [
        Value::Null,
        json!("elapsed"),
        json!(false),
        json!([]),
        json!(["elapsed"]),
        json!({}),
        json!({"basis": null}),
        json!({"basis": 0}),
        json!({"basis": 1, "sampled_unix_ms": 0}),
        json!({"basis": 2}),
        json!({"basis": -1}),
        json!({"basis": 0.0}),
        json!({"basis": false}),
        json!({"basis": {"elapsed": null}}),
        json!({"basis": "future"}),
        json!({"basis": "wall"}),
        json!({"basis": "wall", "sampled_unix_ms": null}),
    ] {
        assert!(
            serde_json::from_value::<ScheduleHorizonClock>(clock.clone()).is_err(),
            "accepted malformed clock value: {clock}"
        );
        assert!(
            serde_json::from_str::<ScheduleHorizonClock>(&clock.to_string()).is_err(),
            "accepted malformed clock JSON: {clock}"
        );
        let mut value = serde_json::to_value(due()).unwrap();
        value["horizon"] = json!({"state": "next", "sampled_boot_ms": 0,
            "in_ms": 0, "clock": clock});
        assert!(
            serde_json::from_value::<ScheduleDue>(value).is_err(),
            "accepted malformed clock: {clock}"
        );
    }
    for bad in [
        json!(0.5),
        json!(1.0),
        json!("0"),
        json!(false),
        json!(u64::MAX),
        Value::Null,
    ] {
        let value = json!({"state": "next", "sampled_boot_ms": 0, "in_ms": 0,
            "clock": {"basis": "wall", "sampled_unix_ms": bad}});
        assert!(
            serde_json::from_value::<ScheduleHorizon>(value).is_err(),
            "accepted malformed wall sample: {bad}"
        );
    }
}

#[test]
fn horizon_states_are_strings_not_numeric_variant_indexes() {
    for (index, named) in [
        (0, json!({"state": "empty", "sampled_boot_ms": 0})),
        (
            1,
            json!({"state": "next", "sampled_boot_ms": 0, "in_ms": 0,
                "clock": {"basis": "elapsed"}}),
        ),
        (2, json!({"state": "unknown", "reason": "clock"})),
    ] {
        assert!(
            serde_json::from_value::<ScheduleHorizon>(named.clone()).is_ok(),
            "invalid named control: {named}"
        );
        let mut numeric = named;
        numeric["state"] = json!(index);
        assert!(
            serde_json::from_value::<ScheduleHorizon>(numeric.clone()).is_err(),
            "accepted numeric state value: {numeric}"
        );
        assert!(
            serde_json::from_str::<ScheduleHorizon>(&numeric.to_string()).is_err(),
            "accepted numeric state JSON: {numeric}"
        );
    }
}

#[test]
fn strict_horizon_tags_preserve_field_order_and_duplicate_rejection() {
    let reordered = r#"{"clock":{"sampled_unix_ms":0,"future":true,"basis":"wall"},
        "in_ms":1,"sampled_boot_ms":0,"future":{},"state":"next"}"#;
    assert_eq!(
        serde_json::from_str::<ScheduleHorizon>(reordered).unwrap(),
        ScheduleHorizon::Next {
            sampled_boot_ms: 0,
            in_ms: 1,
            clock: ScheduleHorizonClock::Wall { sampled_unix_ms: 0 },
        }
    );
    for (case, value) in [
        (
            "duplicate state",
            r#"{"state":"empty","state":"empty","sampled_boot_ms":0}"#,
        ),
        (
            "duplicate clock basis",
            r#"{"state":"next","sampled_boot_ms":0,"in_ms":1,
                "clock":{"basis":"elapsed","basis":"elapsed"}}"#,
        ),
        (
            "duplicate wall sample",
            r#"{"state":"next","sampled_boot_ms":0,"in_ms":1,
                "clock":{"basis":"wall","sampled_unix_ms":0,"sampled_unix_ms":1}}"#,
        ),
        (
            "duplicate boot sample",
            r#"{"state":"empty","sampled_boot_ms":0,"sampled_boot_ms":1}"#,
        ),
    ] {
        assert!(
            serde_json::from_str::<ScheduleHorizon>(value).is_err(),
            "accepted {case}"
        );
    }
}

#[test]
fn wall_horizon_samples_share_the_existing_inclusive_unix_bounds() {
    for sampled_unix_ms in [
        i64::MIN,
        -1,
        0,
        1,
        SCHEDULE_MAX_UNIX_MS,
        SCHEDULE_MAX_UNIX_MS + 1,
        i64::MAX,
    ] {
        let mut value = due();
        value.pending = 0;
        value.waiting_for_clock = 0;
        value.oldest = None;
        value.horizon = Some(ScheduleHorizon::Next {
            sampled_boot_ms: 0,
            in_ms: 900_000,
            clock: ScheduleHorizonClock::Wall { sampled_unix_ms },
        });
        let expected = (0..=SCHEDULE_MAX_UNIX_MS).contains(&sampled_unix_ms);
        // The signed wire number parses; the message's structural validation
        // decides the calendar range rather than guessing another clock basis.
        let decoded: ScheduleDue =
            serde_json::from_value(serde_json::to_value(&value).unwrap()).unwrap();
        assert_eq!(decoded.check().is_ok(), expected, "{sampled_unix_ms}");
        assert_eq!(
            decoded
                .horizon
                .unwrap()
                .remaining_ms(0, Some(sampled_unix_ms)),
            expected.then_some(900_000),
            "{sampled_unix_ms}"
        );
    }
}

#[test]
fn elapsed_remaining_needs_no_wall_clock_and_saturates_without_overflow() {
    let horizon = ScheduleHorizon::Next {
        sampled_boot_ms: 1_000,
        in_ms: 900_000,
        clock: ScheduleHorizonClock::Elapsed,
    };
    assert_eq!(horizon.remaining_ms(1_000, None), Some(900_000));
    assert_eq!(horizon.remaining_ms(1_100, None), Some(899_900));
    assert_eq!(horizon.remaining_ms(999, None), None);
    assert_eq!(horizon.remaining_ms(901_000, None), Some(0));
    assert_eq!(horizon.remaining_ms(u64::MAX, None), Some(0));
    let maximum = ScheduleHorizon::Next {
        sampled_boot_ms: u64::MAX,
        in_ms: i64::MAX as u64,
        clock: ScheduleHorizonClock::Elapsed,
    };
    assert_eq!(maximum.remaining_ms(u64::MAX, None), Some(i64::MAX as u64));
}

#[test]
fn wall_remaining_handles_forward_and_backward_corrections_and_lost_trust() {
    let horizon = ScheduleHorizon::Next {
        sampled_boot_ms: 1_000,
        in_ms: 900_000,
        clock: ScheduleHorizonClock::Wall {
            sampled_unix_ms: 1_000_000,
        },
    };
    assert_eq!(horizon.remaining_ms(1_100, Some(1_000_100)), Some(899_900));
    assert_eq!(horizon.remaining_ms(1_100, Some(1_600_000)), Some(300_000));
    assert_eq!(horizon.remaining_ms(1_100, Some(999_000)), Some(899_900));
    assert_eq!(horizon.remaining_ms(1_100, Some(1_900_000)), Some(0));
    assert_eq!(
        horizon.remaining_ms(1_100, Some(SCHEDULE_MAX_UNIX_MS)),
        Some(0)
    );
    assert_eq!(horizon.remaining_ms(1_100, None), None);
    assert_eq!(horizon.remaining_ms(999, Some(1_000_100)), None);
    for current in [i64::MIN, -1, SCHEDULE_MAX_UNIX_MS + 1, i64::MAX] {
        assert_eq!(horizon.remaining_ms(1_100, Some(current)), None);
    }
}

#[test]
fn mixed_clock_remaining_is_a_lower_bound_even_when_the_timer_was_earliest() {
    // At the source sample, an elapsed timer is due in 900 s and a wall
    // reminder in 1,000 s. A later wall step can make the reminder first.
    let elapsed_deadline_ms = 900_000u64;
    let wall_deadline_ms = 2_000_000i64;
    let sampled_unix_ms = 1_000_000i64;
    let horizon = ScheduleHorizon::Next {
        sampled_boot_ms: 0,
        in_ms: elapsed_deadline_ms,
        clock: ScheduleHorizonClock::Wall { sampled_unix_ms },
    };
    for boot_age in [0, 100, 1_000, 200_000, 1_000_000] {
        for wall_change in [-500_000, -1, 0, 1, 600_000, 1_000_000] {
            let current_wall = sampled_unix_ms + wall_change;
            let remaining = horizon.remaining_ms(boot_age, Some(current_wall)).unwrap();
            let actual_timer = elapsed_deadline_ms.saturating_sub(boot_age);
            let actual_reminder = u64::try_from((wall_deadline_ms - current_wall).max(0)).unwrap();
            assert!(
                remaining <= actual_timer.min(actual_reminder),
                "boot_age={boot_age}, wall_change={wall_change}, remaining={remaining}"
            );
        }
    }
    assert_eq!(horizon.remaining_ms(1_000, Some(1_600_000)), Some(300_000));
    assert_eq!(horizon.remaining_ms(1_000, None), None);
}

#[test]
fn remaining_rejects_invalid_sources_and_never_turns_empty_or_unknown_into_a_duration() {
    for clock in [
        ScheduleHorizonClock::Elapsed,
        ScheduleHorizonClock::Wall { sampled_unix_ms: 0 },
    ] {
        for in_ms in [i64::MAX as u64 + 1, u64::MAX] {
            let horizon = ScheduleHorizon::Next {
                sampled_boot_ms: 0,
                in_ms,
                clock,
            };
            assert_eq!(horizon.remaining_ms(0, Some(0)), None);
        }
    }
    for sampled_unix_ms in [i64::MIN, -1, SCHEDULE_MAX_UNIX_MS + 1, i64::MAX] {
        let horizon = ScheduleHorizon::Next {
            sampled_boot_ms: 0,
            in_ms: 1,
            clock: ScheduleHorizonClock::Wall { sampled_unix_ms },
        };
        assert_eq!(horizon.remaining_ms(0, Some(0)), None);
    }
    let pending = ScheduleHorizon::Next {
        sampled_boot_ms: 0,
        in_ms: 0,
        clock: ScheduleHorizonClock::Wall { sampled_unix_ms: 0 },
    };
    assert_eq!(pending.remaining_ms(0, Some(0)), Some(0));
    assert_eq!(pending.remaining_ms(0, None), None);
    assert_eq!(
        ScheduleHorizon::Empty { sampled_boot_ms: 0 }.remaining_ms(0, Some(0)),
        None
    );
    assert_eq!(
        ScheduleHorizon::Unknown {
            reason: ScheduleHorizonUnknown::Clock,
        }
        .remaining_ms(0, Some(0)),
        None
    );
}

#[test]
fn additive_horizon_fields_are_compatible_but_cannot_be_republished_as_private_data() {
    let mut value = due();
    value.pending = 0;
    value.waiting_for_clock = 0;
    value.oldest = None;
    for horizon in [
        ScheduleHorizon::Empty { sampled_boot_ms: 0 },
        ScheduleHorizon::Next {
            sampled_boot_ms: 0,
            in_ms: 0,
            clock: ScheduleHorizonClock::Elapsed,
        },
        ScheduleHorizon::Next {
            sampled_boot_ms: 0,
            in_ms: 0,
            clock: ScheduleHorizonClock::Wall { sampled_unix_ms: 0 },
        },
        ScheduleHorizon::Unknown {
            reason: ScheduleHorizonUnknown::Unread,
        },
    ] {
        value.horizon = Some(horizon);
        let mut wire = serde_json::to_value(&value).unwrap();
        wire["horizon"]["future"] = json!({"private_canary": "DO_NOT_EXPORT_REMINDER_TEXT"});
        if let Some(clock) = wire["horizon"].get_mut("clock") {
            clock["future"] = json!({"private_canary": "DO_NOT_EXPORT_CLOCK_CONTEXT"});
        }
        let decoded: ScheduleDue = serde_json::from_value(wire).unwrap();
        decoded.check().unwrap();
        assert_eq!(decoded, value);
        assert!(
            !serde_json::to_string(&decoded)
                .unwrap()
                .contains("DO_NOT_EXPORT")
        );
    }
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
