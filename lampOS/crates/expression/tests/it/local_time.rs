//! #820: the local time of day across a daylight-saving change.
use lamp_expression::{local_time::LocalTime, runtime::minute_of_day};

fn ms(at: &str) -> u64 {
    at.parse::<jiff::Timestamp>().unwrap().as_millisecond() as u64
}

#[test]
fn a_zone_follows_new_yorks_clock_changes_of_2026() {
    let york = LocalTime::zone("America/New_York").unwrap();
    // 8 March: 01:59 EST, then 03:00 EDT a minute later.
    assert_eq!(york.minute_of_day(ms("2026-03-08T06:59:00Z")), 60 + 59);
    assert_eq!(york.minute_of_day(ms("2026-03-08T07:00:00Z")), 3 * 60);
    assert_eq!(york.offset_minutes_at(ms("2026-03-08T06:59:00Z")), -300);
    assert_eq!(york.offset_minutes_at(ms("2026-03-08T07:00:00Z")), -240);
    // 1 November: 01:59 EDT, then 01:00 EST a minute later.
    assert_eq!(york.minute_of_day(ms("2026-11-01T05:59:00Z")), 60 + 59);
    assert_eq!(york.minute_of_day(ms("2026-11-01T06:00:00Z")), 60);
    // A night's 23:00 is 23:00 local in winter and in summer.
    assert_eq!(york.minute_of_day(ms("2026-01-15T04:00:00Z")), 23 * 60);
    assert_eq!(york.minute_of_day(ms("2026-07-15T03:00:00Z")), 23 * 60);
}

#[test]
fn a_fixed_offset_is_what_it_was() {
    let vietnam = LocalTime::Offset(420);
    for at in [
        "2026-03-08T06:59:00Z",
        "2026-07-15T03:00:00Z",
        "2026-11-01T06:00:00Z",
    ] {
        assert_eq!(
            vietnam.minute_of_day(ms(at)),
            minute_of_day(ms(at), 420),
            "{at}"
        );
    }
    // Vietnam's zone keeps no summer time: the same as the fixed offset.
    let named = LocalTime::zone("Asia/Ho_Chi_Minh").unwrap();
    for at in ["2026-01-15T04:00:00Z", "2026-07-15T03:00:00Z"] {
        assert_eq!(
            named.minute_of_day(ms(at)),
            vietnam.minute_of_day(ms(at)),
            "{at}"
        );
    }
}

#[test]
fn an_unknown_zone_is_an_error_never_utc() {
    let error = LocalTime::zone("Nowhere/City").unwrap_err();
    assert!(error.to_string().contains("Nowhere/City"), "{error}");
    // This machine's own zone is found (the lamp's comes from setup).
    if std::path::Path::new("/etc/localtime").exists() {
        assert!(LocalTime::zone("system").is_ok());
    }
}
