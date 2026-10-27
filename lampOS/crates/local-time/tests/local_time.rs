//! #820: the hour and the day across daylight saving, and `--zone system`
//! following a zone that setup changes under a running reader.
use jiff::Timestamp;
use lamp_local_time::{LocalTime, SystemZone};
use std::{fs, os::unix::fs::symlink, path::Path};

fn ms(at: &str) -> u64 {
    u64::try_from(at.parse::<Timestamp>().unwrap().as_millisecond()).unwrap()
}

/// Zone data as `/usr/share/zoneinfo` has it, from jiff's own bundle.
fn zoneinfo(root: &Path) {
    for name in ["Asia/Ho_Chi_Minh", "America/New_York"] {
        let (_, data) = jiff_tzdb::get(name).unwrap();
        let file = root.join("zoneinfo").join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, data).unwrap();
    }
}

/// What setup does (`crates/setup/src/serve.rs`, `timezone`): a new link
/// renamed over the old one.
fn point(root: &Path, name: &str) {
    let link = root.join("localtime.lamp-setup-tmp");
    let _ = fs::remove_file(&link);
    symlink(root.join("zoneinfo").join(name), &link).unwrap();
    fs::rename(&link, root.join("localtime")).unwrap();
}

#[test]
fn new_york_changes_its_hour_and_keeps_its_day_across_both_changes() {
    let york = LocalTime::zone("America/New_York").unwrap();
    // 2026-03-08: 01:59 EST, then 03:00 EDT.
    assert_eq!(york.minute_of_day(ms("2026-03-08T06:59:00Z")), 60 + 59);
    assert_eq!(york.minute_of_day(ms("2026-03-08T07:00:00Z")), 3 * 60);
    assert_eq!(york.offset_minutes_at(ms("2026-03-08T06:59:00Z")), -300);
    assert_eq!(york.offset_minutes_at(ms("2026-03-08T07:00:00Z")), -240);
    // 2026-11-01: 01:59 EDT, then 01:00 EST again.
    assert_eq!(york.minute_of_day(ms("2026-11-01T05:59:00Z")), 60 + 59);
    assert_eq!(york.minute_of_day(ms("2026-11-01T06:00:00Z")), 60);
    // 23:00 in winter and in summer is the same hour of the day.
    assert_eq!(york.hour_of_day(ms("2026-01-15T04:00:00Z")), 23);
    assert_eq!(york.hour_of_day(ms("2026-07-15T03:00:00Z")), 23);
    // The 23-hour day: 2026-03-08 begins 05:00 UTC and ends 04:00 UTC.
    let march_8 = york.day(ms("2026-03-08T05:00:00Z"));
    assert_eq!(york.day(ms("2026-03-08T04:59:59.999Z")), march_8 - 1);
    assert_eq!(york.day(ms("2026-03-09T03:59:59.999Z")), march_8);
    assert_eq!(york.day(ms("2026-03-09T04:00:00Z")), march_8 + 1);
    // The 25-hour day: 2026-11-01 begins 04:00 UTC and ends 05:00 UTC.
    let november_1 = york.day(ms("2026-11-01T04:00:00Z"));
    assert_eq!(york.day(ms("2026-11-01T03:59:59.999Z")), november_1 - 1);
    assert_eq!(york.day(ms("2026-11-02T04:59:59.999Z")), november_1);
    assert_eq!(york.day(ms("2026-11-02T05:00:00Z")), november_1 + 1);
}

#[test]
fn vietnam_by_name_is_the_old_420_minutes_all_year() {
    let (named, offset) = (
        LocalTime::zone("Asia/Ho_Chi_Minh").unwrap(),
        LocalTime::Offset(420),
    );
    for at in [
        "2026-01-01T00:00:00Z",
        "2026-03-08T07:00:00Z",
        "2026-07-15T16:59:59.999Z",
        "2026-07-15T17:00:00Z",
        "2026-11-01T06:00:00Z",
    ] {
        assert_eq!(
            named.minute_of_day(ms(at)),
            offset.minute_of_day(ms(at)),
            "{at}"
        );
        assert_eq!(named.day(ms(at)), offset.day(ms(at)), "{at}");
    }
    assert_eq!(
        offset.day(ms("2026-07-15T16:59:59.999Z")) + 1,
        offset.day(ms("2026-07-15T17:00:00Z"))
    );
    assert_eq!(LocalTime::Offset(0).day(ms("1970-01-02T00:00:00Z")), 1);
    assert_eq!(LocalTime::Offset(-60).day(ms("1970-01-01T00:30:00Z")), -1);
}

#[test]
fn an_unknown_zone_is_refused_by_name() {
    let error = LocalTime::zone("Nowhere/City").unwrap_err();
    assert!(error.to_string().contains("Nowhere/City"), "{error}");
}

#[test]
fn a_running_reader_takes_up_the_zone_setup_writes_at_its_next_reading() {
    let root = tempfile::tempdir().unwrap();
    zoneinfo(root.path());
    point(root.path(), "Asia/Ho_Chi_Minh");
    let reader = LocalTime::System(
        SystemZone::open(root.path().join("localtime"), root.path().join("timezone")).unwrap(),
    );
    let noon_utc = ms("2026-07-15T12:00:00Z");
    assert_eq!(reader.offset_minutes_at(noon_utc), 420);
    assert_eq!(reader.hour_of_day(noon_utc), 19);

    point(root.path(), "America/New_York");
    assert_eq!(reader.offset_minutes_at(noon_utc), -240);
    assert_eq!(reader.hour_of_day(noon_utc), 8);
    assert_eq!(reader.offset_minutes_at(ms("2026-01-15T12:00:00Z")), -300);

    point(root.path(), "Asia/Ho_Chi_Minh");
    assert_eq!(reader.hour_of_day(noon_utc), 19);
}

/// A part that keeps a zone by name (Schedule) is given the system's as it
/// is now: where the link points after setup moves it; the name file's text
/// for a zone file that is no link; nothing for a fixed offset.
#[test]
fn the_system_zone_is_named_as_setup_last_pointed_it() {
    let root = tempfile::tempdir().unwrap();
    zoneinfo(root.path());
    point(root.path(), "Asia/Ho_Chi_Minh");
    let reader = LocalTime::System(
        SystemZone::open(root.path().join("localtime"), root.path().join("timezone")).unwrap(),
    );
    assert_eq!(reader.name().as_deref(), Some("Asia/Ho_Chi_Minh"));
    point(root.path(), "America/New_York");
    assert_eq!(reader.name().as_deref(), Some("America/New_York"));

    let copied = tempfile::tempdir().unwrap();
    fs::write(
        copied.path().join("localtime"),
        jiff_tzdb::get("America/New_York").unwrap().1,
    )
    .unwrap();
    fs::write(copied.path().join("timezone"), "America/New_York\n").unwrap();
    let copy = LocalTime::System(
        SystemZone::open(
            copied.path().join("localtime"),
            copied.path().join("timezone"),
        )
        .unwrap(),
    );
    assert_eq!(copy.name().as_deref(), Some("America/New_York"));
    fs::write(copied.path().join("timezone"), "Middle_Earth/Rivendell\n").unwrap();
    assert_eq!(copy.name(), None);

    assert_eq!(LocalTime::Offset(420).name(), None);
    assert_eq!(
        LocalTime::zone("Asia/Ho_Chi_Minh")
            .unwrap()
            .name()
            .as_deref(),
        Some("Asia/Ho_Chi_Minh")
    );
}

#[test]
fn a_zone_file_rewritten_in_place_is_read_again() {
    let root = tempfile::tempdir().unwrap();
    let localtime = root.path().join("localtime");
    fs::write(&localtime, jiff_tzdb::get("Asia/Ho_Chi_Minh").unwrap().1).unwrap();
    let reader = SystemZone::open(&localtime, root.path().join("timezone")).unwrap();
    let local = LocalTime::System(reader);
    assert_eq!(local.offset_minutes_at(ms("2026-07-15T12:00:00Z")), 420);
    fs::write(&localtime, jiff_tzdb::get("America/New_York").unwrap().1).unwrap();
    assert_eq!(local.offset_minutes_at(ms("2026-07-15T12:00:00Z")), -240);
}

#[test]
fn without_a_zone_file_the_named_zone_counts_and_with_neither_nothing_starts() {
    let root = tempfile::tempdir().unwrap();
    let (localtime, timezone) = (root.path().join("localtime"), root.path().join("timezone"));
    let error = SystemZone::open(&localtime, &timezone).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("the system's time zone is unknown"),
        "{error}"
    );

    fs::write(&timezone, "America/New_York\n").unwrap();
    let local = LocalTime::System(SystemZone::open(&localtime, &timezone).unwrap());
    assert_eq!(local.offset_minutes_at(ms("2026-07-15T12:00:00Z")), -240);

    // The file, once there, is the zone.
    zoneinfo(root.path());
    point(root.path(), "Asia/Ho_Chi_Minh");
    assert_eq!(local.offset_minutes_at(ms("2026-07-15T12:00:00Z")), 420);
}

#[test]
fn a_change_that_cannot_be_read_keeps_the_last_zone_until_one_can() {
    let root = tempfile::tempdir().unwrap();
    zoneinfo(root.path());
    point(root.path(), "America/New_York");
    let local = LocalTime::System(
        SystemZone::open(root.path().join("localtime"), root.path().join("timezone")).unwrap(),
    );
    let at = ms("2026-07-15T12:00:00Z");
    assert_eq!(local.offset_minutes_at(at), -240);

    fs::write(root.path().join("zoneinfo/Broken"), b"not a zone file").unwrap();
    point(root.path(), "Broken");
    assert_eq!(local.offset_minutes_at(at), -240, "no silent UTC");
    fs::remove_file(root.path().join("localtime")).unwrap();
    assert_eq!(local.offset_minutes_at(at), -240, "no silent UTC");

    point(root.path(), "Asia/Ho_Chi_Minh");
    assert_eq!(local.offset_minutes_at(at), 420);
}

/// What a package manager does: a new file renamed over the old one.
fn replace(path: &Path, text: &str) {
    let fresh = path.with_extension("new");
    fs::write(&fresh, text).unwrap();
    fs::rename(&fresh, path).unwrap();
}

#[test]
fn with_no_zone_file_a_new_zone_name_is_taken_up_at_the_next_reading() {
    // #844: the name file alone, changed while the reader runs.
    let root = tempfile::tempdir().unwrap();
    let (localtime, timezone) = (root.path().join("localtime"), root.path().join("timezone"));
    fs::write(&timezone, "America/New_York\n").unwrap();
    let local = LocalTime::System(SystemZone::open(&localtime, &timezone).unwrap());
    let at = ms("2026-07-15T12:00:00Z");
    assert_eq!(local.offset_minutes_at(at), -240);
    replace(&timezone, "Asia/Ho_Chi_Minh\n");
    assert_eq!(local.offset_minutes_at(at), 420);
    // A name that is not a zone keeps the last one.
    replace(&timezone, "Nowhere/City\n");
    assert_eq!(local.offset_minutes_at(at), 420);
}

#[test]
fn a_broken_link_moved_to_another_broken_one_with_a_new_name_is_taken_up() {
    let root = tempfile::tempdir().unwrap();
    let (localtime, timezone) = (root.path().join("localtime"), root.path().join("timezone"));
    symlink(root.path().join("zoneinfo/Gone/One"), &localtime).unwrap();
    fs::write(&timezone, "America/New_York\n").unwrap();
    let local = LocalTime::System(SystemZone::open(&localtime, &timezone).unwrap());
    let at = ms("2026-07-15T12:00:00Z");
    assert_eq!(local.offset_minutes_at(at), -240);
    // Setup's way: a new link renamed over the old, still pointing nowhere,
    // and the name file written beside it.
    let fresh = root.path().join("localtime.lamp-setup-tmp");
    symlink(root.path().join("zoneinfo/Gone/Two"), &fresh).unwrap();
    fs::rename(&fresh, &localtime).unwrap();
    replace(&timezone, "Asia/Ho_Chi_Minh\n");
    assert_eq!(local.offset_minutes_at(at), 420);
}

/// A zone file handed in: New York's, slim as the bundle keeps it, whose
/// listed changes stop in 2007 and leave the rest to its closing rule, read
/// across both 2026 changes; Ho Chi Minh's the same all year; no zone file,
/// nothing.
#[test]
fn a_zone_file_handed_in_is_read_past_its_listed_changes() {
    let new_york = jiff_tzdb::get("America/New_York").unwrap().1;
    let saigon = jiff_tzdb::get("Asia/Ho_Chi_Minh").unwrap().1;
    let at = |text: &str| i64::try_from(ms(text) / 1000).unwrap();
    for (when, offset) in [
        ("2026-03-08T06:59:59Z", -18_000),
        ("2026-03-08T07:00:00Z", -14_400),
        ("2026-11-01T05:59:59Z", -14_400),
        ("2026-11-01T06:00:00Z", -18_000),
        ("2061-07-01T12:00:00Z", -14_400),
    ] {
        assert_eq!(
            lamp_local_time::offset_seconds_in(new_york, at(when)),
            Some(offset),
            "{when}"
        );
    }
    for when in ["2026-01-15T12:00:00Z", "2026-07-15T12:00:00Z"] {
        assert_eq!(
            lamp_local_time::offset_seconds_in(saigon, at(when)),
            Some(25_200)
        );
    }
    assert_eq!(lamp_local_time::offset_seconds_in(b"", 0), None);
    assert_eq!(lamp_local_time::offset_seconds_in(&saigon[..60], 0), None);
}

/// The lamp's quiet hours, 22:00 to 07:00 local, one definition (#3504).
#[test]
fn the_quiet_hours_are_ten_at_night_to_seven_in_the_morning() {
    use lamp_local_time::quiet_hours;
    for (minute, quiet) in [
        (21 * 60 + 59, false),
        (22 * 60, true),
        (23 * 60 + 59, true),
        (0, true),
        (7 * 60 - 1, true),
        (7 * 60, false),
        (12 * 60, false),
    ] {
        assert_eq!(quiet_hours(minute), quiet, "{minute}");
    }
    // In Saigon (UTC+7), 22:30 local is 15:30 UTC.
    let saigon = LocalTime::Offset(7 * 60);
    assert!(quiet_hours(
        saigon.minute_of_day(ms("2026-10-28T15:30:00Z"))
    ));
    assert!(!quiet_hours(
        saigon.minute_of_day(ms("2026-10-28T05:00:00Z"))
    ));
}
