//! A reminder every so many minutes or hours (`When::Every`,
//! `LAMP_SCHEDULE_EVERY`; the first builder's yes on #4710, 2 Nov 17:4x).
use lamp_schedule::time::{
    ClockSample, Deadline, Repeat, TimeError, When, advance, newest_due, resolve,
};

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

fn every(minutes: Option<u32>, hours: Option<u32>, window: Option<(&str, &str)>) -> When {
    When::Every {
        minutes,
        hours,
        from: window.map(|(from, _)| from.to_owned()),
        to: window.map(|(_, to)| to.to_owned()),
        zone: None,
    }
}

/// The local `YYYY-MM-DDTHH:MM` of a deadline.
fn local(deadline: &Deadline) -> String {
    deadline.local[..16].to_owned()
}

/// The first `count` occurrences, each checked and later than the one before.
fn walk(first: Deadline, count: usize) -> Vec<String> {
    let mut seen = vec![local(&first)];
    let mut at = first;
    for _ in 1..count {
        let next = advance(&at).unwrap().unwrap();
        next.check().unwrap();
        assert!(next.unix_ms > at.unix_ms, "{next:?} after {at:?}");
        seen.push(local(&next));
        at = next;
    }
    seen
}

#[test]
fn every_hour_starts_a_step_from_now_on_the_whole_minute() {
    let now = clock("2026-11-02T10:00:30Z");
    let first = resolve(&every(Some(60), None, None), &now, "Etc/UTC").unwrap();
    assert_eq!(
        walk(first.clone(), 3),
        ["2026-11-02T11:01", "2026-11-02T12:01", "2026-11-02T13:01"]
    );
    assert!(matches!(
        first.repeat,
        Some(Repeat::Every {
            minutes: 60,
            from: None,
            to: None,
            ..
        })
    ));
    // Hours are 60 minutes each; an hour and a half is 90 minutes.
    let two = resolve(&every(None, Some(2), None), &now, "Etc/UTC").unwrap();
    assert_eq!(local(&two), "2026-11-02T12:01");
    let ninety = resolve(&every(Some(90), None, None), &now, "Etc/UTC").unwrap();
    assert_eq!(walk(ninety, 2), ["2026-11-02T11:31", "2026-11-02T13:01"]);
}

#[test]
fn the_step_is_15_minutes_to_24_hours_in_minutes_or_hours_never_both() {
    let now = clock("2026-11-02T10:00:00Z");
    for when in [
        every(Some(14), None, None),
        every(Some(1441), None, None),
        every(None, Some(25), None),
        every(None, Some(0), None),
        every(Some(60), Some(1), None),
        every(None, None, None),
    ] {
        assert_eq!(
            resolve(&when, &now, "Etc/UTC").unwrap_err(),
            TimeError::InvalidInterval,
            "{when:?}"
        );
    }
    for when in [
        every(Some(15), None, None),
        every(Some(1440), None, None),
        every(None, Some(24), None),
    ] {
        resolve(&when, &now, "Etc/UTC").unwrap();
    }
    // A window needs both ends.
    let half = When::Every {
        minutes: Some(60),
        hours: None,
        from: Some("08:00".into()),
        to: None,
        zone: None,
    };
    assert_eq!(
        resolve(&half, &now, "Etc/UTC").unwrap_err(),
        TimeError::InvalidWindow
    );
}

#[test]
fn a_window_starts_at_its_from_and_its_next_start_follows_its_end() {
    // Every 4 hours from 08:00 to 18:00, asked at 17:00: the next start.
    let now = clock("2026-11-02T17:00:00Z");
    let first = resolve(
        &every(None, Some(4), Some(("08:00", "18:00"))),
        &now,
        "Etc/UTC",
    )
    .unwrap();
    assert_eq!(
        walk(first, 5),
        [
            "2026-11-03T08:00",
            "2026-11-03T12:00",
            "2026-11-03T16:00",
            "2026-11-04T08:00",
            "2026-11-04T12:00",
        ]
    );
    // Asked inside it, the next step of today's window.
    let now = clock("2026-11-02T09:10:00Z");
    let first = resolve(
        &every(Some(30), None, Some(("08:00", "18:00"))),
        &now,
        "Etc/UTC",
    )
    .unwrap();
    assert_eq!(local(&first), "2026-11-02T09:30");
}

#[test]
fn a_window_may_cross_midnight_and_equal_ends_are_the_whole_day() {
    let now = clock("2026-11-02T23:30:00Z");
    let first = resolve(
        &every(Some(60), None, Some(("22:00", "02:00"))),
        &now,
        "Etc/UTC",
    )
    .unwrap();
    assert_eq!(
        walk(first, 4),
        [
            "2026-11-03T00:00",
            "2026-11-03T01:00",
            "2026-11-03T22:00",
            "2026-11-03T23:00",
        ]
    );
    // From 09:00 to 09:00 is no window: a step from now.
    let whole = resolve(
        &every(Some(60), None, Some(("09:00", "09:00"))),
        &now,
        "Etc/UTC",
    )
    .unwrap();
    assert_eq!(local(&whole), "2026-11-03T00:30");
    assert!(matches!(
        whole.repeat,
        Some(Repeat::Every {
            from: None,
            to: None,
            ..
        })
    ));
}

#[test]
fn a_dst_gap_is_stepped_over_never_said_twice_or_backwards() {
    // New York springs forward at 02:00 on 8 March 2026: 02:00 to 02:59 never
    // happen. Every 30 minutes from 01:10 stays strictly increasing.
    let now = clock("2026-03-08T06:10:00Z"); // 01:10 EST
    let first = resolve(
        &When::Every {
            minutes: Some(30),
            hours: None,
            from: None,
            to: None,
            zone: Some("America/New_York".into()),
        },
        &now,
        "Etc/UTC",
    )
    .unwrap();
    let seen = walk(first, 5);
    assert_eq!(seen[0], "2026-03-08T01:40");
    assert!(!seen.iter().any(|at| at.contains("T02:")), "{seen:?}");
    let mut sorted = seen.clone();
    sorted.dedup();
    assert_eq!(sorted.len(), seen.len(), "{seen:?}");
}

#[test]
fn after_a_power_cut_only_the_newest_missed_one_is_due() {
    let now = clock("2026-11-02T10:00:00Z");
    let first = resolve(&every(Some(60), None, None), &now, "Etc/UTC").unwrap();
    let later = clock("2026-11-02T15:20:00Z");
    let newest = newest_due(&first, &later).unwrap();
    assert_eq!(local(&newest), "2026-11-02T15:00");
}

#[test]
fn a_damaged_every_is_refused_when_read() {
    let now = clock("2026-11-02T10:00:00Z");
    let first = resolve(
        &every(Some(60), None, Some(("08:00", "18:00"))),
        &now,
        "Etc/UTC",
    )
    .unwrap();
    first.check().unwrap();
    let mut too_short = first.clone();
    if let Some(Repeat::Every { minutes, .. }) = &mut too_short.repeat {
        *minutes = 5;
    }
    assert_eq!(too_short.check(), Err(TimeError::InvalidDeadline));
    let mut outside = first;
    if let Some(Repeat::Every { from, to, .. }) = &mut outside.repeat {
        *from = Some("12:00".into());
        *to = Some("13:00".into());
    }
    assert_eq!(outside.check(), Err(TimeError::InvalidDeadline));
}
