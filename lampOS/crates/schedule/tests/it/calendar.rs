use lamp_schedule::time::{
    ClockSample, Deadline, Fold, MAX_AFTER_SECONDS, MAX_UNIX_MS, MIN_UNIX_MS, Repeat, TimeError,
    Weekday, When, advance, remaining_ms, resolve, validate_zone,
};
use serde_json::json;

fn unix(raw: &str) -> i64 {
    raw.parse::<jiff::Timestamp>().unwrap().as_millisecond()
}

fn clock(raw: &str) -> ClockSample {
    ClockSample {
        unix_ms: unix(raw),
        boot_ms: 10_000,
        boot_id: "boot-1".into(),
        wall_trusted: true,
    }
}

fn at(time: &str, day: Option<&str>, zone: Option<&str>, fold: Option<Fold>) -> When {
    When::At {
        time: time.into(),
        day: day.map(str::to_owned),
        zone: zone.map(str::to_owned),
        fold,
    }
}

fn daily(time: &str, zone: &str) -> When {
    When::Daily {
        time: time.into(),
        zone: Some(zone.into()),
    }
}

fn weekly(weekday: Weekday, time: &str, zone: &str) -> When {
    When::Weekly {
        weekday,
        time: time.into(),
        zone: Some(zone.into()),
    }
}

fn assert_at(deadline: &Deadline, expected: &str) {
    assert_eq!(deadline.unix_ms, unix(expected), "{}", deadline.local);
    deadline.check().unwrap();
}

#[test]
fn duration_keeps_milliseconds_and_both_clock_anchors() {
    let now = clock("2026-10-14T08:00:00.123Z");
    let due = resolve(&When::After { seconds: 600 }, &now, "Asia/Ho_Chi_Minh").unwrap();
    assert_at(&due, "2026-10-14T08:10:00.123Z");
    assert_eq!(due.elapsed.as_ref().unwrap().at_boot_ms, 610_000);
    assert_eq!(due.elapsed.as_ref().unwrap().boot_id, "boot-1");
    assert_eq!(due.repeat, None);
    assert!(due.local.starts_with("2026-10-14T15:10:00.123+07:00["));
    assert_eq!(remaining_ms(&due, &now), Ok(Some(600_000)));
    assert_eq!(advance(&due), Ok(None));
}

#[test]
fn serialized_elapsed_timer_survives_process_restart_without_trusted_wall_time() {
    let now = clock("2026-10-14T08:00:00Z");
    let due = resolve(&When::After { seconds: 60 }, &now, "UTC").unwrap();
    let encoded = serde_json::to_vec(&due).unwrap();
    let restored: Deadline = serde_json::from_slice(&encoded).unwrap();
    let restarted = ClockSample {
        unix_ms: 0,
        boot_ms: 30_000,
        wall_trusted: false,
        ..now
    };
    assert_eq!(restored, due);
    assert_eq!(remaining_ms(&restored, &restarted), Ok(Some(40_000)));
}

#[test]
fn same_boot_wall_jumps_neither_end_nor_extend_a_duration_timer() {
    let now = clock("2026-10-14T08:00:00Z");
    let due = resolve(&When::After { seconds: 60 }, &now, "UTC").unwrap();
    for unix_ms in [0, now.unix_ms - 86_400_000, now.unix_ms + 86_400_000] {
        for wall_trusted in [false, true] {
            let jumped = ClockSample {
                unix_ms,
                boot_ms: 60_000,
                wall_trusted,
                ..now.clone()
            };
            assert_eq!(remaining_ms(&due, &jumped), Ok(Some(10_000)));
        }
    }
    let exact = ClockSample {
        boot_ms: 70_000,
        ..now.clone()
    };
    assert_eq!(remaining_ms(&due, &exact), Ok(Some(0)));
    assert_eq!(
        remaining_ms(
            &due,
            &ClockSample {
                boot_ms: 70_001,
                ..exact
            }
        ),
        Ok(Some(-1))
    );
}

#[test]
fn another_boot_waits_for_trusted_wall_time_and_then_reports_lateness() {
    let now = clock("2026-10-14T08:00:00Z");
    let due = resolve(&When::After { seconds: 60 }, &now, "UTC").unwrap();
    let mut other = ClockSample {
        boot_id: "boot-10".into(),
        boot_ms: 5,
        unix_ms: due.unix_ms + 25_000,
        wall_trusted: false,
    };
    assert_eq!(remaining_ms(&due, &other), Ok(None));
    other.wall_trusted = true;
    assert_eq!(remaining_ms(&due, &other), Ok(Some(-25_000)));
    other.unix_ms = due.unix_ms - 25_000;
    assert_eq!(remaining_ms(&due, &other), Ok(Some(25_000)));
}

#[test]
fn calendar_deadlines_require_wall_trust_even_on_the_original_boot() {
    let mut now = clock("2026-10-14T08:00:00Z");
    let due = resolve(&at("09:00", Some("today"), None, None), &now, "UTC").unwrap();
    now.wall_trusted = false;
    now.boot_ms += 100_000_000;
    assert_eq!(remaining_ms(&due, &now), Ok(None));
    now.wall_trusted = true;
    now.unix_ms = due.unix_ms + 1;
    assert_eq!(remaining_ms(&due, &now), Ok(Some(-1)));
}

#[test]
fn creation_always_requires_a_trusted_wall_clock() {
    let mut now = clock("2026-10-14T08:00:00Z");
    now.wall_trusted = false;
    for when in [
        When::After { seconds: 1 },
        at("09:00", None, None, None),
        daily("09:00", "UTC"),
        weekly(Weekday::Monday, "09:00", "UTC"),
    ] {
        assert_eq!(resolve(&when, &now, "UTC"), Err(TimeError::UntrustedClock));
    }
}

#[test]
fn duration_bounds_are_inclusive_and_never_wrap() {
    let now = clock("2026-10-14T08:00:00Z");
    for seconds in [0, MAX_AFTER_SECONDS + 1, u64::MAX] {
        assert_eq!(
            resolve(&When::After { seconds }, &now, "UTC"),
            Err(TimeError::OutOfRange)
        );
    }
    for seconds in [1, MAX_AFTER_SECONDS] {
        let due = resolve(&When::After { seconds }, &now, "UTC").unwrap();
        assert_eq!(due.unix_ms - now.unix_ms, (seconds * 1_000) as i64);
        due.check().unwrap();
    }
    let mut overflow = now;
    overflow.boot_ms = u64::MAX - 999;
    assert_eq!(
        resolve(&When::After { seconds: 1 }, &overflow, "UTC"),
        Err(TimeError::OutOfRange)
    );
}

#[test]
fn utc_limits_match_jiff_and_arithmetic_rejects_the_next_millisecond() {
    assert_eq!(MAX_UNIX_MS, jiff::Timestamp::MAX.as_millisecond());
    let mut now = ClockSample {
        unix_ms: MIN_UNIX_MS,
        boot_ms: 0,
        boot_id: "boot".into(),
        wall_trusted: true,
    };
    now.check().unwrap();
    now.unix_ms = MAX_UNIX_MS;
    now.check().unwrap();
    for value in [i64::MIN, -1, MAX_UNIX_MS + 1, i64::MAX] {
        now.unix_ms = value;
        assert_eq!(now.check(), Err(TimeError::OutOfRange));
    }
    now.unix_ms = MAX_UNIX_MS - 1_000;
    let due = resolve(&When::After { seconds: 1 }, &now, "UTC").unwrap();
    assert_eq!(due.unix_ms, MAX_UNIX_MS);
    due.check().unwrap();
    now.unix_ms += 1;
    assert_eq!(
        resolve(&When::After { seconds: 1 }, &now, "UTC"),
        Err(TimeError::OutOfRange)
    );
}

#[test]
fn elapsed_difference_uses_checked_signed_conversion() {
    let now = clock("2026-10-14T08:00:00Z");
    let due = resolve(&When::After { seconds: 1 }, &now, "UTC").unwrap();
    let distant = ClockSample {
        boot_ms: u64::MAX,
        ..now
    };
    assert_eq!(remaining_ms(&due, &distant), Err(TimeError::OutOfRange));
}

#[test]
fn boot_clock_order_is_deliberately_the_owners_responsibility() {
    let now = clock("2026-10-14T08:00:00Z");
    let due = resolve(&When::After { seconds: 1 }, &now, "UTC").unwrap();
    let earlier = ClockSample {
        boot_ms: 9_000,
        ..now
    };
    assert_eq!(remaining_ms(&due, &earlier), Ok(Some(2_000)));
}

#[test]
fn boot_identity_is_bounded_and_matches_the_bus_alphabet() {
    let mut now = clock("2026-10-14T08:00:00Z");
    for bad in ["", "bad:id", "white space", "../a/b", "é", "a\n"] {
        now.boot_id = bad.into();
        assert_eq!(now.check(), Err(TimeError::InvalidClock));
    }
    now.boot_id = "x".repeat(129);
    assert_eq!(now.check(), Err(TimeError::InvalidClock));
    for good in ["boot_2026-10.14", &"x".repeat(128)] {
        now.boot_id = good.into();
        now.check().unwrap();
    }
}

#[test]
fn ho_chi_minh_local_request_resolves_without_model_date_arithmetic() {
    let now = clock("2026-10-14T07:00:00Z");
    let due = resolve(
        &at("15:00", Some("today"), None, None),
        &now,
        "Asia/Ho_Chi_Minh",
    )
    .unwrap();
    assert_at(&due, "2026-10-14T08:00:00Z");
    assert_eq!(due.local, "2026-10-14T15:00:00+07:00[Asia/Ho_Chi_Minh]");
    assert_eq!(due.elapsed, None);
    assert_eq!(advance(&due), Ok(None));
}

#[test]
fn explicit_zone_overrides_the_default_and_recurrence_freezes_its_zone() {
    let now = clock("2026-10-14T07:00:00Z");
    let due = resolve(&daily("15:00", "Asia/Ho_Chi_Minh"), &now, "not-a-zone").unwrap();
    assert_at(&due, "2026-10-14T08:00:00Z");
    let next = advance(&due).unwrap().unwrap();
    assert_at(&next, "2026-10-15T08:00:00Z");
    assert!(next.local.ends_with("[Asia/Ho_Chi_Minh]"));
    let explicit = resolve(&at("09:00", None, Some("UTC"), None), &now, "not-a-zone").unwrap();
    assert_at(&explicit, "2026-10-14T09:00:00Z");
}

#[test]
fn omitted_and_next_days_are_strictly_future_including_exact_equality() {
    for day in [None, Some("next")] {
        let before = clock("2026-10-14T08:59:59.999Z");
        assert_at(
            &resolve(&at("09:00", day, None, None), &before, "UTC").unwrap(),
            "2026-10-14T09:00:00Z",
        );
        for raw in [
            "2026-10-14T09:00:00Z",
            "2026-10-14T09:00:00.001Z",
            "2026-10-14T23:59:59Z",
        ] {
            assert_at(
                &resolve(&at("09:00", day, None, None), &clock(raw), "UTC").unwrap(),
                "2026-10-15T09:00:00Z",
            );
        }
    }
}

#[test]
fn today_and_explicit_dates_refuse_equal_or_past_requests() {
    let now = clock("2026-10-14T09:00:00Z");
    for day in [Some("today"), Some("2026-10-14")] {
        for time in ["08:59", "09:00"] {
            assert_eq!(
                resolve(&at(time, day, None, None), &now, "UTC"),
                Err(TimeError::NotFuture)
            );
        }
    }
    assert_eq!(
        resolve(&at("10:00", Some("2026-10-13"), None, None), &now, "UTC"),
        Err(TimeError::NotFuture)
    );
}

#[test]
fn tomorrow_uses_local_dates_across_leap_month_and_year_boundaries() {
    for (now, expected) in [
        ("2024-02-28T23:00:00Z", "2024-02-29T09:00:00Z"),
        ("2024-02-29T23:00:00Z", "2024-03-01T09:00:00Z"),
        ("2026-04-30T23:00:00Z", "2026-05-01T09:00:00Z"),
        ("2026-12-31T23:00:00Z", "2027-01-01T09:00:00Z"),
    ] {
        assert_at(
            &resolve(
                &at("09:00", Some("tomorrow"), None, None),
                &clock(now),
                "UTC",
            )
            .unwrap(),
            expected,
        );
    }
    assert_at(
        &resolve(
            &at("09:00", Some("tomorrow"), None, None),
            &clock("2026-12-31T18:00:00Z"),
            "Asia/Ho_Chi_Minh",
        )
        .unwrap(),
        "2027-01-02T02:00:00Z",
    );
}

#[test]
fn exact_date_syntax_rejects_bad_dates_and_unrecognized_relative_words() {
    let now = clock("2024-01-01T00:00:00Z");
    assert_at(
        &resolve(&at("09:00", Some("2024-02-29"), None, None), &now, "UTC").unwrap(),
        "2024-02-29T09:00:00Z",
    );
    for bad in [
        "2023-02-29",
        "2100-02-29",
        "2024-04-31",
        "2024-00-01",
        "2024-13-01",
        "2024-1-01",
        "24-01-01",
        "2024-01-01Z",
        "2024/01/01",
        "Today",
        "tonight",
        "monday",
        "",
        " 2024-01-01",
    ] {
        assert_eq!(
            resolve(&at("09:00", Some(bad), None, None), &now, "UTC"),
            Err(TimeError::InvalidDay),
            "{bad}"
        );
    }
}

#[test]
fn clock_text_is_exact_ascii_hh_mm_for_every_calendar_kind() {
    let now = clock("2026-10-14T08:00:00Z");
    for bad in [
        "9:00",
        "09:0",
        "09:00:00",
        "24:00",
        "23:60",
        "-1:00",
        "+9:00",
        "09:00 ",
        " 09:00",
        "０９:００",
        "noon",
        "",
        "09\n00",
    ] {
        for when in [
            at(bad, None, None, None),
            daily(bad, "UTC"),
            weekly(Weekday::Friday, bad, "UTC"),
        ] {
            assert_eq!(
                resolve(&when, &now, "UTC"),
                Err(TimeError::InvalidTime),
                "{bad}"
            );
        }
    }
}

#[test]
fn zones_must_be_bounded_names_in_the_bundled_database() {
    for good in [
        "UTC",
        "Etc/UTC",
        "Asia/Ho_Chi_Minh",
        "Asia/Saigon",
        "America/New_York",
        "Pacific/Apia",
        "Etc/GMT+7",
    ] {
        validate_zone(good).unwrap();
    }
    for bad in [
        "",
        "+07:00",
        "UTC+07",
        "EST5EDT,M3.2.0,M11.1.0",
        "Not/A_Zone",
        "/etc/localtime",
        "../UTC",
        "Etc/Unknown",
        " UTC",
        "Asia/Ho_Chi_Minh\n",
    ] {
        assert_eq!(validate_zone(bad), Err(TimeError::InvalidZone), "{bad}");
    }
    assert_eq!(validate_zone(&"A".repeat(129)), Err(TimeError::InvalidZone));
    assert_eq!(
        resolve(
            &When::After { seconds: 1 },
            &clock("2026-10-14T08:00:00Z"),
            "+07:00"
        ),
        Err(TimeError::InvalidZone)
    );
}

#[test]
fn one_off_gap_is_rejected_even_when_a_fold_preference_is_supplied() {
    let now = clock("2026-03-08T05:00:00Z");
    for fold in [
        None,
        Some(Fold::Reject),
        Some(Fold::Earlier),
        Some(Fold::Later),
    ] {
        assert_eq!(
            resolve(
                &at("02:30", Some("2026-03-08"), Some("America/New_York"), fold),
                &now,
                "UTC"
            ),
            Err(TimeError::NonexistentTime)
        );
    }
}

#[test]
fn one_off_fold_requires_choice_and_resolves_both_instants() {
    let now = clock("2026-11-01T04:00:00Z");
    for fold in [None, Some(Fold::Reject)] {
        assert_eq!(
            resolve(
                &at("01:30", Some("today"), Some("America/New_York"), fold),
                &now,
                "UTC"
            ),
            Err(TimeError::AmbiguousTime)
        );
    }
    for (fold, expected) in [
        (Fold::Earlier, "2026-11-01T05:30:00Z"),
        (Fold::Later, "2026-11-01T06:30:00Z"),
    ] {
        assert_at(
            &resolve(
                &at("01:30", Some("today"), Some("America/New_York"), Some(fold)),
                &now,
                "UTC",
            )
            .unwrap(),
            expected,
        );
    }
}

#[test]
fn next_occurrence_distinguishes_remaining_fold_from_a_wholly_past_fold() {
    let between = clock("2026-11-01T05:45:00Z");
    assert_eq!(
        resolve(
            &at("01:30", None, Some("America/New_York"), None),
            &between,
            "UTC"
        ),
        Err(TimeError::AmbiguousTime)
    );
    assert_at(
        &resolve(
            &at("01:30", None, Some("America/New_York"), Some(Fold::Later)),
            &between,
            "UTC",
        )
        .unwrap(),
        "2026-11-01T06:30:00Z",
    );
    assert_at(
        &resolve(
            &at("01:30", None, Some("America/New_York"), Some(Fold::Earlier)),
            &between,
            "UTC",
        )
        .unwrap(),
        "2026-11-02T06:30:00Z",
    );
    assert_at(
        &resolve(
            &at("01:30", None, Some("America/New_York"), None),
            &clock("2026-11-01T07:00:00Z"),
            "UTC",
        )
        .unwrap(),
        "2026-11-02T06:30:00Z",
    );
}

#[test]
fn next_occurrence_rejects_an_upcoming_gap_but_can_pass_a_past_gap() {
    let when = at("02:30", None, Some("America/New_York"), None);
    assert_eq!(
        resolve(&when, &clock("2026-03-08T06:00:00Z"), "UTC"),
        Err(TimeError::NonexistentTime)
    );
    assert_at(
        &resolve(&when, &clock("2026-03-08T08:00:00Z"), "UTC").unwrap(),
        "2026-03-09T06:30:00Z",
    );
}

#[test]
fn daily_gap_moves_one_occurrence_forward_without_changing_requested_clock() {
    let first = resolve(
        &daily("02:30", "America/New_York"),
        &clock("2026-03-07T05:00:00Z"),
        "UTC",
    )
    .unwrap();
    assert_at(&first, "2026-03-07T07:30:00Z");
    let gap = advance(&first).unwrap().unwrap();
    assert_at(&gap, "2026-03-08T07:30:00Z");
    assert!(gap.local.starts_with("2026-03-08T03:30:00-04:00"));
    assert_eq!(
        gap.repeat,
        Some(Repeat::Daily {
            time: "02:30".into(),
            zone: "America/New_York".into(),
            date: "2026-03-08".into()
        })
    );
    let after = advance(&gap).unwrap().unwrap();
    assert_at(&after, "2026-03-09T06:30:00Z");
    assert!(after.local.starts_with("2026-03-09T02:30:00-04:00"));
}

#[test]
fn weekly_gap_stays_on_the_requested_weekday_and_clock() {
    let first = resolve(
        &weekly(Weekday::Sunday, "02:30", "America/New_York"),
        &clock("2026-03-01T05:00:00Z"),
        "UTC",
    )
    .unwrap();
    assert_at(&first, "2026-03-01T07:30:00Z");
    let gap = advance(&first).unwrap().unwrap();
    assert_at(&gap, "2026-03-08T07:30:00Z");
    let after = advance(&gap).unwrap().unwrap();
    assert_at(&after, "2026-03-15T06:30:00Z");
    assert!(after.local.starts_with("2026-03-15T02:30:00"));
}

#[test]
fn repeating_fold_uses_earlier_instant_once_and_next_creation_skips_spent_fold() {
    let first = resolve(
        &daily("01:30", "America/New_York"),
        &clock("2026-10-31T04:00:00Z"),
        "UTC",
    )
    .unwrap();
    let fold = advance(&first).unwrap().unwrap();
    assert_at(&fold, "2026-11-01T05:30:00Z");
    assert_at(&advance(&fold).unwrap().unwrap(), "2026-11-02T06:30:00Z");
    assert_at(
        &resolve(
            &daily("01:30", "America/New_York"),
            &clock("2026-11-01T05:45:00Z"),
            "UTC",
        )
        .unwrap(),
        "2026-11-02T06:30:00Z",
    );
}

#[test]
fn half_hour_dst_gap_uses_database_rules_without_assuming_one_hour() {
    let first = resolve(
        &daily("02:15", "Australia/Lord_Howe"),
        &clock("2026-10-02T14:00:00Z"),
        "UTC",
    )
    .unwrap();
    assert_at(&first, "2026-10-02T15:45:00Z");
    let gap = advance(&first).unwrap().unwrap();
    assert_at(&gap, "2026-10-03T15:45:00Z");
    assert!(gap.local.starts_with("2026-10-04T02:45:00+11:00"));
    let after = advance(&gap).unwrap().unwrap();
    assert_at(&after, "2026-10-04T15:15:00Z");
}

#[test]
fn advancement_retains_every_past_occurrence_without_delivery_time_drift() {
    let mut due = resolve(
        &daily("09:00", "UTC"),
        &clock("2026-10-01T00:00:00Z"),
        "UTC",
    )
    .unwrap();
    let restarted = clock("2026-10-14T12:00:00Z");
    for day in 1..=14 {
        assert_eq!(due.local, format!("2026-10-{day:02}T09:00:00+00:00[UTC]"));
        assert!(remaining_ms(&due, &restarted).unwrap().unwrap() < 0);
        due = advance(&due).unwrap().unwrap();
    }
    assert_at(&due, "2026-10-15T09:00:00Z");
}

#[test]
fn weekly_initial_selection_covers_all_weekdays_and_exact_time_rollover() {
    let now = clock("2026-12-28T09:00:00Z"); // Monday.
    for (weekday, expected) in [
        (Weekday::Monday, "2027-01-04T09:00:00Z"),
        (Weekday::Tuesday, "2026-12-29T09:00:00Z"),
        (Weekday::Wednesday, "2026-12-30T09:00:00Z"),
        (Weekday::Thursday, "2026-12-31T09:00:00Z"),
        (Weekday::Friday, "2027-01-01T09:00:00Z"),
        (Weekday::Saturday, "2027-01-02T09:00:00Z"),
        (Weekday::Sunday, "2027-01-03T09:00:00Z"),
    ] {
        assert_at(
            &resolve(&weekly(weekday, "09:00", "UTC"), &now, "UTC").unwrap(),
            expected,
        );
    }
}

#[test]
fn recurrence_calendar_arithmetic_handles_leap_month_and_year_boundaries() {
    for (now, expected) in [
        ("2024-02-28T00:00:00Z", "2024-02-29T09:00:00Z"),
        ("2024-02-29T00:00:00Z", "2024-03-01T09:00:00Z"),
        ("2026-04-30T00:00:00Z", "2026-05-01T09:00:00Z"),
        ("2026-12-31T00:00:00Z", "2027-01-01T09:00:00Z"),
    ] {
        let first = resolve(&daily("09:00", "UTC"), &clock(now), "UTC").unwrap();
        assert_at(&advance(&first).unwrap().unwrap(), expected);
    }
    let first = resolve(
        &weekly(Weekday::Thursday, "09:00", "UTC"),
        &clock("2024-02-29T00:00:00Z"),
        "UTC",
    )
    .unwrap();
    assert_at(&advance(&first).unwrap().unwrap(), "2024-03-07T09:00:00Z");
}

#[test]
fn apia_skipped_day_retains_two_distinct_civil_occurrences_at_one_instant() {
    let first = resolve(
        &daily("09:00", "Pacific/Apia"),
        &clock("2011-12-29T00:00:00Z"),
        "UTC",
    )
    .unwrap();
    assert_at(&first, "2011-12-29T19:00:00Z");
    let skipped = advance(&first).unwrap().unwrap();
    let following = advance(&skipped).unwrap().unwrap();
    assert_at(&skipped, "2011-12-30T19:00:00Z");
    assert_at(&following, "2011-12-30T19:00:00Z");
    assert_ne!(skipped.repeat, following.repeat);
    assert_eq!(
        skipped.repeat,
        Some(Repeat::Daily {
            time: "09:00".into(),
            zone: "Pacific/Apia".into(),
            date: "2011-12-30".into()
        })
    );
    assert_eq!(
        following.repeat,
        Some(Repeat::Daily {
            time: "09:00".into(),
            zone: "Pacific/Apia".into(),
            date: "2011-12-31".into()
        })
    );
    assert_at(
        &advance(&following).unwrap().unwrap(),
        "2011-12-31T19:00:00Z",
    );
}

#[test]
fn lower_utc_boundary_can_select_next_occurrence_from_a_previous_local_year() {
    let now = clock("1970-01-01T00:00:00Z");
    for when in [
        at("09:00", None, None, None),
        daily("09:00", "America/Los_Angeles"),
    ] {
        let due = resolve(&when, &now, "America/Los_Angeles").unwrap();
        assert_at(&due, "1970-01-01T17:00:00Z");
    }
    let due = resolve(&daily("18:00", "America/Los_Angeles"), &now, "UTC").unwrap();
    assert_at(&due, "1970-01-01T02:00:00Z");
    assert!(matches!(&due.repeat, Some(Repeat::Daily {date, ..}) if date == "1969-12-31"));
    assert_at(&advance(&due).unwrap().unwrap(), "1970-01-02T02:00:00Z");
}

#[test]
fn calendar_creation_and_advancement_fail_closed_at_the_upper_range() {
    let now = clock("9999-12-30T00:00:00Z");
    let due = resolve(&daily("09:00", "UTC"), &now, "UTC").unwrap();
    due.check().unwrap();
    assert_eq!(advance(&due), Err(TimeError::OutOfRange));
    assert_eq!(
        resolve(&at("09:00", Some("tomorrow"), None, None), &now, "UTC"),
        Err(TimeError::OutOfRange)
    );
}

#[test]
fn persisted_deadline_validation_rejects_inconsistent_or_unbounded_descriptors() {
    let now = clock("2026-10-14T08:00:00Z");
    let good = resolve(&daily("09:00", "UTC"), &now, "UTC").unwrap();
    let mut wrong = good.clone();
    wrong.unix_ms += 1;
    assert_eq!(wrong.check(), Err(TimeError::InvalidDeadline));
    wrong = good.clone();
    wrong.local = "x".repeat(193);
    assert_eq!(wrong.check(), Err(TimeError::InvalidDeadline));
    wrong = good.clone();
    wrong.local = "private source text".into();
    assert_eq!(wrong.check(), Err(TimeError::InvalidDeadline));
    wrong = good.clone();
    wrong.repeat = Some(Repeat::Daily {
        time: "10:00".into(),
        zone: "UTC".into(),
        date: "2026-10-14".into(),
    });
    assert_eq!(wrong.check(), Err(TimeError::InvalidDeadline));
    wrong = good.clone();
    wrong.repeat = Some(Repeat::Weekly {
        weekday: Weekday::Thursday,
        time: "09:00".into(),
        zone: "UTC".into(),
        date: "2026-10-14".into(),
    });
    assert_eq!(wrong.check(), Err(TimeError::InvalidDeadline));
    wrong = good.clone();
    wrong.elapsed = resolve(&When::After { seconds: 60 }, &now, "UTC")
        .unwrap()
        .elapsed;
    assert_eq!(wrong.check(), Err(TimeError::InvalidDeadline));
    assert_eq!(advance(&wrong), Err(TimeError::InvalidDeadline));
    assert_eq!(remaining_ms(&wrong, &now), Err(TimeError::InvalidDeadline));
}

#[test]
fn serde_rejects_unknown_fields_wrong_case_and_unrecognized_words() {
    for value in [
        json!({"kind":"after","seconds":60,"extra":true}),
        json!({"kind":"at","time":"09:00","weekday":"monday"}),
        json!({"kind":"daily","time":"09:00","fold":"earlier"}),
        json!({"kind":"weekly","weekday":"monday","time":"09:00","extra":0}),
        json!({"kind":"weekly","weekday":"Monday","time":"09:00"}),
        json!({"kind":"weekly","weekday":"mon","time":"09:00"}),
        json!({"kind":"weekly","weekday":1,"time":"09:00"}),
        json!({"kind":"at","time":"09:00","fold":"first"}),
        json!({"kind":"At","time":"09:00"}),
        json!({"kind":"interval","seconds":60}),
    ] {
        assert!(
            serde_json::from_value::<When>(value.clone()).is_err(),
            "{value}"
        );
    }
    let now = clock("2026-10-14T08:00:00Z");
    let mut value = serde_json::to_value(&now).unwrap();
    value["extra"] = json!(true);
    assert!(serde_json::from_value::<ClockSample>(value).is_err());
    let due = resolve(&When::After { seconds: 1 }, &now, "UTC").unwrap();
    for where_extra in ["deadline", "elapsed"] {
        let mut value = serde_json::to_value(&due).unwrap();
        if where_extra == "deadline" {
            value["extra"] = json!(true);
        } else {
            value["elapsed"]["extra"] = json!(true);
        }
        assert!(serde_json::from_value::<Deadline>(value).is_err());
    }
    let mut value =
        serde_json::to_value(resolve(&daily("09:00", "UTC"), &now, "UTC").unwrap()).unwrap();
    value["repeat"]["extra"] = json!(true);
    assert!(serde_json::from_value::<Deadline>(value).is_err());
}

#[test]
fn omitted_optionals_and_full_lowercase_weekdays_round_trip() {
    let parsed: When = serde_json::from_value(json!({"kind":"at","time":"09:00"})).unwrap();
    assert_eq!(parsed, at("09:00", None, None, None));
    for weekday in [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ] {
        let value = json!({"kind":"weekly","weekday":weekday,"time":"09:00"});
        let parsed: When = serde_json::from_value(value).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap()["weekday"], weekday);
        let due = resolve(&parsed, &clock("2026-10-14T08:00:00Z"), "UTC").unwrap();
        let restored: Deadline =
            serde_json::from_slice(&serde_json::to_vec(&due).unwrap()).unwrap();
        assert_eq!(due, restored);
        restored.check().unwrap();
    }
}

#[test]
fn errors_have_static_messages_without_echoing_supplied_private_text() {
    let now = clock("2026-10-14T08:00:00Z");
    for when in [
        at("private text", None, None, None),
        at("09:00", Some("private text"), None, None),
        at("09:00", None, Some("private text"), None),
    ] {
        let error = resolve(&when, &now, "UTC").unwrap_err();
        assert!(!error.to_string().contains("private text"));
        assert!(!format!("{error:?}").contains("private text"));
    }
}
