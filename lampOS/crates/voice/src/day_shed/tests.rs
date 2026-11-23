use super::*;

const DAY: u64 = 20_410 * DAY_MS;

/// Each threshold: under 0.8 open, from 0.8 near, at 1.0 spent.
#[test]
fn each_threshold_of_the_day() {
    let mut reading = Reading::default();
    for (header, level) in [
        ("0", Level::Open),
        ("0.79", Level::Open),
        ("0.8", Level::Near),
        (" 0.95 ", Level::Near),
        ("1", Level::Spent),
        ("1.0", Level::Spent),
    ] {
        reading.heard(DAY + 1_000, Some(header), true);
        assert_eq!(reading.level(DAY + 2_000), level, "{header}");
    }
    assert!(!Level::Open.sheds());
    assert!(Level::Near.sheds());
    assert!(Level::Spent.sheds());
}

/// As lamp-cloud's forwarder reads it: a value that isn't 0 to 1 is not
/// kept, a 2xx answer without the header says no day limit, a refusal
/// without it changes nothing, and another day's reading is no limit.
#[test]
fn a_missing_header_is_not_limited_and_a_bad_one_changes_nothing() {
    let mut reading = Reading::default();
    assert_eq!(reading.level(DAY), Level::Open, "nothing heard");
    assert!(reading.heard(DAY, Some("0.9"), true));
    for bad in ["1.5", "-0.1", "NaN", "inf", "", "most"] {
        assert!(!reading.heard(DAY + 1, Some(bad), true), "{bad:?}");
        assert_eq!(reading.level(DAY + 1), Level::Near, "{bad:?}");
    }
    assert!(!reading.heard(DAY + 2, None, false), "a refusal: kept");
    assert_eq!(reading.level(DAY + 2), Level::Near);
    assert!(reading.heard(DAY + 3, None, true), "2xx without: cleared");
    assert_eq!(reading.used, None);
    assert_eq!(reading.level(DAY + 3), Level::Open);
    // Yesterday's 0.9, read today: open, and a new day starts from nothing.
    reading.heard(DAY + 4, Some("0.9"), true);
    assert_eq!(reading.level(DAY + DAY_MS), Level::Open);
    assert!(reading.heard(DAY + DAY_MS, None, false), "rolled");
    assert_eq!(reading.used, None);
}

/// The worker keeps its reading in the file only when it changed, and the
/// coordinator's look reads it at most once a second; a missing,
/// unreadable or other day's file is open, and with the switch off the
/// look never reads.
#[test]
fn the_coordinator_reads_what_the_worker_kept() {
    let dir = std::env::temp_dir().join(format!("day-shed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("day-used.json");
    let now = wall_ms();
    let start = Instant::now();
    let mut look = Look::new(true, &path);
    assert_eq!(look.level(start, now), Level::Open, "no file");

    let worker = Worker::at(&path);
    worker.heard(Some("0.85"), true, now);
    assert_eq!(worker.level(now), Level::Near);
    // Within the second, the look keeps what it read.
    assert_eq!(look.level(start, now), Level::Open);
    let later = start + LOOKED_EVERY;
    assert_eq!(look.level(later, now), Level::Near);

    worker.heard(Some("1"), true, now);
    let later = later + LOOKED_EVERY;
    assert_eq!(look.level(later, now), Level::Spent);
    // Tomorrow, the same file: open.
    let later = later + LOOKED_EVERY;
    assert_eq!(look.level(later, now + DAY_MS), Level::Open);

    std::fs::write(&path, "not json").unwrap();
    let later = later + LOOKED_EVERY;
    assert_eq!(look.level(later, now), Level::Open, "unreadable");

    worker.heard(Some("0.9"), true, now);
    let mut off = Look::new(false, &path);
    assert_eq!(off.level(later + LOOKED_EVERY, now), Level::Open, "off");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The switch: `on` exactly, and the worker's flag only then.
#[test]
fn the_switch_is_on_exactly() {
    assert!(on(Some("on")));
    for value in [None, Some(""), Some("ON"), Some("1"), Some("off")] {
        assert!(!on(value), "{value:?}");
        assert!(arguments(value).is_empty(), "{value:?}");
    }
    assert_eq!(arguments(Some("on")), [FLAG]);
    let flags = [FLAG.to_owned(), "--rest".to_owned()];
    assert_eq!(take(&flags), (true, &flags[1..]));
    assert_eq!(take(&flags[1..]), (false, &flags[1..]));
}
