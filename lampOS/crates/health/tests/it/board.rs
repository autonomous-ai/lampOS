//! The board's reader against boards made of files, and each line at
//! which the board's report turns to a warning or an error, from both
//! sides.
use crate::tree;

use lamp_health::board::{self, Reading, Unread, Zone};
use lamp_msgs::{Diagnostic, Level};
use std::path::Path;
use tree::Tree;

fn value<'a>(report: &'a Diagnostic, key: &str) -> Option<&'a str> {
    report
        .values
        .iter()
        .find(|value| value.key == key)
        .map(|value| value.value.as_str())
}

fn what(reading: &Reading) -> Vec<&'static str> {
    reading.unread.iter().map(|unread| unread.what).collect()
}

/// A board with nothing the matter, to change one number of.
fn well() -> Reading {
    Reading {
        zones: vec![zone("skin_zone", 37_000, &[], None)],
        cpu_load: Some(0.31),
        memory_used: Some(0.44),
        disk_free: Some(0.13),
        uptime_s: Some(208_800),
        wifi_dbm: None,
        clock_synced: None,
        unread: Vec::new(),
        judged: Vec::new(),
        cpu_caps: Vec::new(),
        throttled_for: None,
        busiest: Vec::new(),
        processes: Vec::new(),
        cpu_ticks: None,
        cpufreq_cooling: false,
        hot_held: false,
    }
}

fn zone(name: &str, milli_c: i64, passive: &[i64], critical: Option<i64>) -> Zone {
    Zone {
        name: name.into(),
        milli_c,
        passive_milli_c: passive.to_vec(),
        critical_milli_c: critical,
    }
}

/// The level and the message of the board's report.
fn said(reading: &Reading) -> (Level, String) {
    let report = reading.report();
    assert_eq!(report.name, "board");
    (report.level, report.message)
}

/// A processor zone of the real board at `milli_c`.
fn cpu_at(milli_c: i64) -> Reading {
    Reading {
        zones: vec![
            zone("skin_zone", 37_000, &[], None),
            zone(
                "cpul_thermal_zone",
                milli_c,
                &[70_000, 90_000],
                Some(110_000),
            ),
        ],
        ..well()
    }
}

#[test]
fn the_real_boards_files_are_read_as_its_numbers() {
    let tree = Tree::real_board("real");
    let reading = board::read(&tree.places());
    assert!(reading.unread.is_empty(), "{:?}", reading.unread);
    // Eight zones in the order of their numbers, the two processor zones
    // with their trip points.
    assert_eq!(reading.zones.len(), 8);
    for (zone, (kind, milli_c)) in reading.zones.iter().zip(tree::ZONES) {
        assert_eq!((zone.name.as_str(), zone.milli_c), (kind, milli_c));
    }
    assert_eq!(
        reading.zones[0],
        zone(
            "cpul_thermal_zone",
            63_500,
            &[70_000, 90_000],
            Some(110_000)
        )
    );
    assert_eq!(reading.zones[1].passive_milli_c, [70_000, 90_000]);
    assert_eq!(reading.zones[7], zone("skin_zone", 37_000, &[], None));
    // 2.48 over eight processors; 8,960,000 of 16,000,000 kB available.
    assert!((reading.cpu_load.unwrap() - 0.31).abs() < 1e-6);
    assert!((reading.memory_used.unwrap() - 0.44).abs() < 1e-6);
    assert_eq!(reading.uptime_s, Some(208_800));
    // The disk is the machine's own: whatever is free on it, asked twice.
    let free = reading.disk_free.unwrap();
    assert!((0.0..=1.0).contains(&free), "{free}");
    assert!((free - tree::disk_free(&tree.path("disk"))).abs() < 0.01);
    // The real board has no file for the Wi-Fi and no timesync directory.
    assert_eq!((reading.wifi_dbm, reading.clock_synced), (None, None));

    let state = reading.state().expect("all four numbers are there");
    assert_eq!(state.check(), Ok(()));
    assert_eq!(state.temperature_c.len(), 8);
    assert_eq!(state.temperature_c["cpub_thermal_zone"], 64.25);
    assert_eq!(state.temperature_c["skin_zone"], 37.0);
    assert_eq!(
        (state.cpu_load, state.memory_used, state.uptime_s),
        (
            reading.cpu_load.unwrap(),
            reading.memory_used.unwrap(),
            208_800
        )
    );
    let json = serde_json::to_string(&state).unwrap();
    assert!(
        !json.contains("wifi_dbm") && !json.contains("clock_synced"),
        "{json}"
    );

    // The report: nothing the matter that these files could show, and the
    // numbers as text. The level is the disk's alone to spoil.
    let report = reading.report();
    assert_eq!(report.name, "board");
    if free >= board::DISK_FREE_WARN {
        assert_eq!(
            (report.level, report.message.as_str()),
            (Level::Ok, ""),
            "{report:?}"
        );
    } else {
        assert!(report.message.starts_with("the disk is "), "{report:?}");
    }
    assert_eq!(
        report
            .values
            .iter()
            .map(|value| value.key.as_str())
            .collect::<Vec<_>>(),
        [
            "cpu_load",
            "memory_used",
            "disk_free",
            "uptime_s",
            "hottest_zone",
            "hottest_c",
            "cpu_zone",
            "cpu_c",
            "cpu_next_trip_c"
        ]
    );
    assert_eq!(value(&report, "cpu_load"), Some("0.31"));
    assert_eq!(value(&report, "memory_used"), Some("0.440"));
    assert_eq!(value(&report, "uptime_s"), Some("208800"));
    assert_eq!(value(&report, "hottest_zone"), Some("ddr_thermal_zone"));
    assert_eq!(value(&report, "hottest_c"), Some("64.5"));
}

#[test]
fn the_default_places_are_the_systems_own() {
    let places = board::Places::default();
    assert_eq!(
        (
            places.proc.as_path(),
            places.sys.as_path(),
            places.run.as_path(),
            places.disk.as_path()
        ),
        (
            Path::new("/proc"),
            Path::new("/sys"),
            Path::new("/run"),
            Path::new("/")
        )
    );
}

#[test]
fn a_number_whose_file_is_missing_is_not_read_and_the_report_says_which() {
    let tree = Tree::real_board("missing");
    // Each file alone: the number it holds is the one not read.
    for (file, number) in [
        ("proc/loadavg", "cpu_load"),
        ("proc/stat", "cpu_load"),
        ("proc/meminfo", "memory_used"),
        ("proc/uptime", "uptime_s"),
    ] {
        let kept = std::fs::read_to_string(tree.path(file)).unwrap();
        tree.remove(file);
        let reading = board::read(&tree.places());
        assert_eq!(what(&reading), [number], "{file}");
        let Unread { why, .. } = &reading.unread[0];
        assert!(
            why.starts_with(&format!("{}: ", tree.path(file).display())),
            "{why}"
        );
        // No numbers are published without all four, and the report says
        // which could not be read. The others are in it as before.
        assert_eq!(reading.state(), None, "{file}");
        let report = reading.report();
        if reading.disk_free.unwrap() >= board::DISK_FREE_WARN {
            assert_eq!(
                (report.level, report.message.clone()),
                (Level::Warn, format!("could not be read: {number}")),
                "{file}"
            );
        }
        assert_eq!(value(&report, number), None);
        // The four numbers but one, the hottest zone and the hottest processor
        // zone with its next trip.
        assert_eq!(report.values.len(), 8, "{file}");
        tree.file(file, &kept);
    }
    assert!(board::read(&tree.places()).unread.is_empty());

    // A disk that is not there.
    let mut places = tree.places();
    places.disk = tree.path("no-such-disk");
    let reading = board::read(&places);
    assert_eq!(what(&reading), ["disk_free"]);
    assert_eq!(reading.state(), None);
    assert_eq!(
        said(&reading),
        (Level::Warn, "could not be read: disk_free".into())
    );

    // Nothing at all, as on a machine that is no Linux: every number the
    // lamp cannot do without, in the order they are read, and no zone.
    let empty = Tree::empty("nothing");
    let mut places = empty.places();
    places.disk = empty.path("no-such-disk");
    let reading = board::read(&places);
    assert_eq!(
        what(&reading),
        ["cpu_load", "memory_used", "disk_free", "uptime_s"]
    );
    assert!(reading.zones.is_empty());
    assert_eq!(reading.state(), None);
    let report = reading.report();
    assert_eq!(
        (report.level, report.message.as_str()),
        (
            Level::Warn,
            "could not be read: cpu_load, memory_used, disk_free, uptime_s"
        )
    );
    assert!(report.values.is_empty());
    // Places that are not there at all are read the same way.
    let nowhere = board::read(&board::Places {
        proc: "/nonexistent/proc".into(),
        sys: "/nonexistent/sys".into(),
        run: "/nonexistent/run".into(),
        disk: "/nonexistent/disk".into(),
        judged: Vec::new(),
        clock: board::ClockSource::Timesync,
    });
    assert_eq!(what(&nowhere), what(&reading));
    assert!(nowhere.zones.is_empty() && nowhere.state().is_none());
    assert_eq!(
        nowhere.unread[0].why,
        "/nonexistent/proc/loadavg: No such file or directory (os error 2)"
    );
}

#[test]
fn garbage_in_a_file_is_a_number_not_read_and_never_a_wrong_one() {
    let tree = Tree::real_board("garbage");
    let kept = |file: &str| std::fs::read_to_string(tree.path(file)).unwrap();
    for (file, number, garbage) in [
        (
            "proc/loadavg",
            "cpu_load",
            &[
                "",
                "\n",
                "busy 1 2\n",
                "-1.0 0 0\n",
                "nan 0 0\n",
                "inf 0 0\n",
            ][..],
        ),
        // No line for a processor: the first line is all of them together.
        (
            "proc/stat",
            "cpu_load",
            &["", "cpu  1 2 3 4\nintr 5\n", "cpus 4\n", "\u{0}\u{1}\u{2}"][..],
        ),
        (
            "proc/meminfo",
            "memory_used",
            &[
                "",
                "MemTotal: lots kB\nMemAvailable: 5 kB\n",
                "MemTotal: 16000000 kB\n",
                "MemAvailable: 5 kB\n",
                "MemTotal: 0 kB\nMemAvailable: 0 kB\n",
                "MemTotal 16000000 kB\nMemAvailable 5 kB\n",
            ][..],
        ),
        (
            "proc/uptime",
            "uptime_s",
            &["", "soon\n", "-5.0 1\n", "nan 1\n"][..],
        ),
    ] {
        let good = kept(file);
        for text in garbage {
            tree.file(file, text);
            let reading = board::read(&tree.places());
            assert_eq!(what(&reading), [number], "{file} {text:?}");
            assert_eq!(reading.state(), None);
        }
        tree.file(file, &good);
    }
    // Bytes that are no text at all.
    std::fs::write(tree.path("proc/loadavg"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    assert_eq!(what(&board::read(&tree.places())), ["cpu_load"]);
    tree.file("proc/loadavg", "2.48 2.51 2.60 3/1234 56789\n");
    // A directory where a file should be.
    tree.remove("proc/uptime");
    std::fs::create_dir(tree.path("proc/uptime")).unwrap();
    assert_eq!(what(&board::read(&tree.places())), ["uptime_s"]);
    tree.remove("proc/uptime");
    tree.file("proc/uptime", "12.9 1\n");
    // More available than there is, and a fraction of a second: neither
    // is out of range once read.
    tree.file("proc/meminfo", "MemTotal: 100 kB\nMemAvailable: 250 kB\n");
    let reading = board::read(&tree.places());
    assert_eq!(
        (reading.memory_used, reading.uptime_s),
        (Some(0.0), Some(12))
    );
    assert_eq!(reading.state().unwrap().check(), Ok(()));
    // A long file is read as far as its processors, which come first.
    let long = format!("{}{}", tree::STAT, "x".repeat(1_000_000));
    tree.file("proc/stat", &long);
    assert!((board::read(&tree.places()).cpu_load.unwrap() - 0.31).abs() < 1e-6);

    // A zone with garbage for its temperature or no type is no zone, and
    // the others are read as before.
    tree.file("sys/class/thermal/thermal_zone0/temp", "warm\n");
    tree.file("sys/class/thermal/thermal_zone2/temp", "");
    tree.remove("sys/class/thermal/thermal_zone4/type");
    tree.file("sys/class/thermal/thermal_zone5/type", "\n");
    // Not a temperature: colder than can be, and hotter than a board gets.
    tree.heat(6, -274_000);
    tree.heat(7, 2_147_483_647);
    let reading = board::read(&tree.places());
    assert_eq!(
        reading
            .zones
            .iter()
            .map(|zone| zone.name.as_str())
            .collect::<Vec<_>>(),
        ["cpub_thermal_zone", "cpub_idle_zone"]
    );
    assert!(reading.unread.is_empty());
    assert_eq!(reading.state().unwrap().temperature_c.len(), 2);
}

#[test]
fn a_board_with_no_thermal_zones_still_has_its_other_numbers() {
    let tree = Tree::real_board("no-zones");
    tree.remove("sys/class/thermal");
    let reading = board::read(&tree.places());
    assert!(reading.zones.is_empty() && reading.unread.is_empty());
    let state = reading.state().expect("the four numbers are there");
    assert!(state.temperature_c.is_empty());
    assert_eq!(state.check(), Ok(()));
    let report = reading.report();
    assert_eq!(
        (value(&report, "hottest_zone"), value(&report, "hottest_c")),
        (None, None)
    );
    assert_eq!(report.values.len(), 4);
    // Nothing is said of a temperature nobody measures.
    assert!(!report.message.contains("°C"), "{report:?}");
    // A thermal directory with other things in it and no zone.
    tree.file(
        "sys/class/thermal/cooling_device0/type",
        "thermal-cpufreq-0\n",
    );
    tree.file("sys/class/thermal/thermal_zone/type", "nameless\n");
    tree.file("sys/class/thermal/thermal_zoneX/temp", "50000\n");
    assert!(board::read(&tree.places()).zones.is_empty());
}

#[test]
fn trip_points_are_read_by_their_kind_and_the_lowest_passive_comes_first() {
    let tree = Tree::empty("trips");
    // Out of order, with kinds this program does nothing with.
    tree.zone(
        3,
        "soc_thermal",
        50_000,
        &[
            (95_000, "passive"),
            (60_000, "active"),
            (115_000, "critical"),
            (75_000, "passive"),
            (100_000, "hot"),
            (110_000, "critical"),
        ],
    );
    // No trip points at all.
    tree.zone(1, "skin_zone", 37_000, &[]);
    // Trip points that cannot be read: a kind with no temperature, a
    // temperature with garbage for its kind, a temperature that is none.
    tree.zone(7, "odd_zone", 41_000, &[(70_000, "passive")]);
    tree.remove("sys/class/thermal/thermal_zone7/trip_point_0_temp");
    tree.file(
        "sys/class/thermal/thermal_zone7/trip_point_1_temp",
        "80000\n",
    );
    tree.file(
        "sys/class/thermal/thermal_zone7/trip_point_1_type",
        "\u{0}\n",
    );
    tree.file("sys/class/thermal/thermal_zone7/trip_point_2_temp", "hot\n");
    tree.file(
        "sys/class/thermal/thermal_zone7/trip_point_2_type",
        "critical\n",
    );
    // A second zone of a type already seen is told apart by its number.
    tree.zone(12, "soc_thermal", 51_000, &[]);
    // Past a hole in the numbering nothing is looked for.
    tree.zone(20, "gap_zone", 30_000, &[(70_000, "passive")]);
    tree.file(
        "sys/class/thermal/thermal_zone20/trip_point_2_temp",
        "90000\n",
    );
    tree.file(
        "sys/class/thermal/thermal_zone20/trip_point_2_type",
        "passive\n",
    );
    let zones = board::read(&tree.places()).zones;
    assert_eq!(
        zones,
        [
            zone("skin_zone", 37_000, &[], None),
            zone("soc_thermal", 50_000, &[75_000, 95_000], Some(110_000)),
            zone("odd_zone", 41_000, &[], None),
            zone("soc_thermal_12", 51_000, &[], None),
            zone("gap_zone", 30_000, &[70_000], None),
        ]
    );
}

#[test]
fn a_zone_warns_from_its_first_passive_point_and_not_before() {
    // The real board's processor zones slow down from 70 °C, and read 63
    // to 66 with the lamp at rest: that is not a warning.
    for at_rest in [63_000, 65_000, 66_000, 69_999] {
        assert_eq!(said(&cpu_at(at_rest)), (Level::Ok, String::new()));
    }
    assert_eq!(
        said(&cpu_at(70_000)),
        (
            Level::Warn,
            "cpul_thermal_zone is at 70 °C: the board slows itself down from 70".into()
        )
    );
    assert_eq!(
        said(&cpu_at(76_400)),
        (
            Level::Warn,
            "cpul_thermal_zone is at 76 °C: the board slows itself down from 70".into()
        )
    );
    assert_eq!(said(&cpu_at(89_999)).0, Level::Warn);
    assert_eq!(board::WARN_BELOW_PASSIVE_C, 0);
}

#[test]
fn a_zone_is_an_error_at_its_second_passive_point_or_ten_degrees_under_critical() {
    assert_eq!(said(&cpu_at(89_999)).0, Level::Warn);
    assert_eq!(
        said(&cpu_at(90_000)),
        (
            Level::Error,
            "cpul_thermal_zone is at 90 °C: past 90, the second point at which the board slows itself down"
                .into()
        )
    );
    assert_eq!(
        said(&cpu_at(99_999)).1,
        "cpul_thermal_zone is at 100 °C: past 90, the second point at which the board slows itself down"
    );
    // From ten under the point where the board turns itself off, that is
    // what is said.
    assert_eq!(
        said(&cpu_at(100_000)),
        (
            Level::Error,
            "cpul_thermal_zone is at 100 °C: the board turns itself off at 110".into()
        )
    );
    assert_eq!(said(&cpu_at(125_000)).0, Level::Error);
    // A zone with a critical point and no second passive one: only the
    // critical line makes an error of it.
    let one = |milli_c| Reading {
        zones: vec![zone("soc_thermal", milli_c, &[85_000], Some(115_000))],
        ..well()
    };
    assert_eq!(said(&one(84_999)).0, Level::Ok);
    assert_eq!(said(&one(85_000)).0, Level::Warn);
    assert_eq!(said(&one(104_999)).0, Level::Warn);
    assert_eq!(
        said(&one(105_000)),
        (
            Level::Error,
            "soc_thermal is at 105 °C: the board turns itself off at 115".into()
        )
    );
    // Only a critical point: no warning comes before the error.
    let critical = |milli_c| Reading {
        zones: vec![zone("soc_thermal", milli_c, &[], Some(95_000))],
        ..well()
    };
    assert_eq!(said(&critical(84_999)).0, Level::Ok);
    assert_eq!(said(&critical(85_000)).0, Level::Error);
    // Only passive points: the second is the error.
    let passive = |milli_c| Reading {
        zones: vec![zone("soc_thermal", milli_c, &[60_000, 75_000], None)],
        ..well()
    };
    assert_eq!(said(&passive(59_999)).0, Level::Ok);
    assert_eq!(said(&passive(60_000)).0, Level::Warn);
    assert_eq!(said(&passive(74_999)).0, Level::Warn);
    assert_eq!(said(&passive(75_000)).0, Level::Error);
    // One passive point and nothing else never makes an error.
    let single = |milli_c| Reading {
        zones: vec![zone("soc_thermal", milli_c, &[60_000], None)],
        ..well()
    };
    assert_eq!(said(&single(54_999)).0, Level::Ok);
    assert_eq!(said(&single(200_000)).0, Level::Warn);
    assert_eq!(board::ERROR_BELOW_CRITICAL_C, 10);
}

#[test]
fn only_the_zones_named_are_judged_and_every_zone_is_still_published() {
    // The real board at rest: its graphics zone names a first passive
    // point of 60 °C and reads 64. Judged like every zone, it is a
    // warning all day.
    let board = |judged: &[&str]| Reading {
        zones: vec![
            zone(
                "cpub_thermal_zone",
                65_000,
                &[70_000, 90_000],
                Some(110_000),
            ),
            zone("gpu_thermal_zone", 64_000, &[60_000, 90_000], Some(110_000)),
            zone("skin_zone", 37_000, &[], Some(50_000)),
        ],
        judged: judged.iter().map(|begins| (*begins).to_owned()).collect(),
        ..well()
    };
    assert_eq!(
        said(&board(&[])),
        (
            Level::Warn,
            "gpu_thermal_zone is at 64 °C: the board slows itself down from 60".into()
        )
    );
    // With the processors and the skin named, the graphics zone's own
    // limit is nothing to the lamp.
    let watched = board(&["cpu", "skin"]);
    assert_eq!(said(&watched), (Level::Ok, String::new()));
    // Its temperature is still among the numbers, and still the hottest
    // if it is.
    assert_eq!(watched.state().unwrap().temperature_c.len(), 3);
    // A zone that is named is judged as before.
    let mut hot = board(&["cpu", "skin"]);
    hot.zones[0].milli_c = 72_000;
    assert_eq!(
        said(&hot).1,
        "cpub_thermal_zone is at 72 °C: the board slows itself down from 70"
    );
    let mut warm_case = board(&["cpu", "skin"]);
    warm_case.zones[2].milli_c = 41_000;
    assert_eq!(
        said(&warm_case),
        (
            Level::Error,
            "skin_zone is at 41 °C: the board turns itself off at 50".into()
        )
    );
    // A beginning that no zone has judges none of them.
    assert_eq!(said(&board(&["soc"])), (Level::Ok, String::new()));
}

#[test]
fn a_zone_that_names_no_trip_point_warns_from_80_and_is_an_error_from_90() {
    let at = |milli_c| Reading {
        zones: vec![zone("gpu_thermal_zone", milli_c, &[], None)],
        ..well()
    };
    assert_eq!(said(&at(79_999)), (Level::Ok, String::new()));
    assert_eq!(
        said(&at(80_000)),
        (
            Level::Warn,
            "gpu_thermal_zone is at 80 °C: hot, and the zone names no limit of its own".into()
        )
    );
    assert_eq!(said(&at(89_999)).0, Level::Warn);
    assert_eq!(
        said(&at(90_000)),
        (
            Level::Error,
            "gpu_thermal_zone is at 90 °C: too hot, and the zone names no limit of its own".into()
        )
    );
    assert_eq!(
        (board::WARN_WITHOUT_TRIPS_C, board::ERROR_WITHOUT_TRIPS_C),
        (80, 90)
    );
}

#[test]
fn the_disk_warns_under_a_tenth_free_and_is_an_error_under_three_hundredths() {
    let free = |free: f32| Reading {
        disk_free: Some(free),
        ..well()
    };
    assert_eq!(said(&free(1.0)), (Level::Ok, String::new()));
    assert_eq!(said(&free(0.10)), (Level::Ok, String::new()));
    assert_eq!(
        said(&free(0.0999)),
        (
            Level::Warn,
            "the disk is filling up: 10% of it is free".into()
        )
    );
    assert_eq!(
        said(&free(0.07)),
        (
            Level::Warn,
            "the disk is filling up: 7% of it is free".into()
        )
    );
    assert_eq!(said(&free(0.03)).0, Level::Warn);
    assert_eq!(
        said(&free(0.0299)),
        (
            Level::Error,
            "the disk is all but full: 3% of it is free".into()
        )
    );
    assert_eq!(said(&free(0.0)).0, Level::Error);
    assert_eq!(
        (board::DISK_FREE_WARN, board::DISK_FREE_ERROR),
        (0.10, 0.03)
    );
    assert_eq!(value(&free(0.0299).report(), "disk_free"), Some("0.030"));
    assert_eq!(value(&free(0.137).report(), "disk_free"), Some("0.137"));
}

#[test]
fn memory_warns_over_nine_tenths_used_and_load_over_one_and_a_half() {
    let used = |used: f32| Reading {
        memory_used: Some(used),
        ..well()
    };
    assert_eq!(said(&used(0.90)), (Level::Ok, String::new()));
    assert_eq!(
        said(&used(0.9001)),
        (Level::Warn, "90% of the memory is in use".into())
    );
    assert_eq!(
        said(&used(1.0)),
        (Level::Warn, "100% of the memory is in use".into())
    );
    let load = |load: f32| Reading {
        cpu_load: Some(load),
        ..well()
    };
    assert_eq!(said(&load(1.5)), (Level::Ok, String::new()));
    assert_eq!(said(&load(1.5001)).0, Level::Warn);
    assert_eq!(
        said(&load(1.83)),
        (
            Level::Warn,
            "the processors have more work than they can do: a load of 1.83 for each".into()
        )
    );
    assert_eq!((board::MEMORY_USED_WARN, board::CPU_LOAD_WARN), (0.90, 1.5));
}

#[test]
fn a_clock_that_is_known_not_to_be_set_is_a_warning() {
    let clock = |clock_synced| Reading {
        clock_synced,
        ..well()
    };
    assert_eq!(said(&clock(None)), (Level::Ok, String::new()));
    assert_eq!(said(&clock(Some(true))), (Level::Ok, String::new()));
    assert_eq!(
        said(&clock(Some(false))),
        (Level::Warn, "the clock is not set from the network".into())
    );
    // As the files say it: a timesync directory, and in it a file once
    // the clock is set.
    let tree = Tree::real_board("clock");
    assert_eq!(board::read(&tree.places()).clock_synced, None);
    std::fs::create_dir_all(tree.path("run/systemd/timesync")).unwrap();
    let reading = board::read(&tree.places());
    assert_eq!(reading.clock_synced, Some(false));
    assert_eq!(reading.state().unwrap().clock_synced, Some(false));
    tree.file("run/systemd/timesync/synchronized", "");
    let reading = board::read(&tree.places());
    assert_eq!(reading.clock_synced, Some(true));
    assert!(
        serde_json::to_string(&reading.state().unwrap())
            .unwrap()
            .contains("\"clock_synced\":true")
    );
}

#[test]
fn the_wifi_signal_is_the_level_of_the_first_device_that_has_one() {
    let tree = Tree::real_board("wifi");
    let wireless = |devices: &str| {
        tree.file(
            "proc/net/wireless",
            &format!(
                "Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE\n \
                 face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22\n{devices}"
            ),
        );
        board::read(&tree.places())
    };
    // The file is not there on the real board.
    assert_eq!(board::read(&tree.places()).wifi_dbm, None);
    let reading =
        wireless(" wlan0: 0000   54.  -56.  -256        0      0      0      0      0        0\n");
    assert_eq!(reading.wifi_dbm, Some(-56));
    assert_eq!(reading.state().unwrap().wifi_dbm, Some(-56));
    assert!(reading.unread.is_empty());
    // Without the full stops, and with a second device.
    assert_eq!(
        wireless("wlan1: 0000 70 -40 -256 0 0 0 0 0 0\nwlan0: 0000 54 -56 -256 0 0 0 0 0 0\n")
            .wifi_dbm,
        Some(-40)
    );
    // Headings and no device; a line cut short; a level that is no
    // number; one that is not in dBm. Each is no signal, and nothing else
    // about the board is spoiled by it.
    for devices in [
        "",
        "wlan0: 0000   54.\n",
        "wlan0: 0000   54.  strong  -256\n",
        "wlan0: 0000   54.  200.  -256\n",
        "wlan0: 0000   54.  -151.  -256\n",
    ] {
        let reading = wireless(devices);
        assert_eq!(reading.wifi_dbm, None, "{devices:?}");
        assert!(reading.unread.is_empty() && reading.state().is_some());
    }
    // A device with no level is passed over for one that has one.
    assert_eq!(
        wireless("wlan0: 0000 54. strong -256\nwlan1: 0000 54. -61. -256\n").wifi_dbm,
        Some(-61)
    );
    // A signal is never a reason for a warning.
    let weak = Reading {
        wifi_dbm: Some(-95),
        ..well()
    };
    assert_eq!(said(&weak), (Level::Ok, String::new()));
}

#[test]
fn the_report_is_the_worst_finding_and_of_two_as_bad_the_first() {
    // An error is said before any warning, wherever it is found.
    let mut reading = Reading {
        zones: vec![
            zone("skin_zone", 37_000, &[], None),
            zone(
                "cpul_thermal_zone",
                66_000,
                &[70_000, 90_000],
                Some(110_000),
            ),
            zone(
                "cpub_thermal_zone",
                72_000,
                &[70_000, 90_000],
                Some(110_000),
            ),
        ],
        cpu_load: Some(2.0),
        memory_used: Some(0.95),
        disk_free: Some(0.01),
        uptime_s: Some(12),
        wifi_dbm: None,
        clock_synced: Some(false),
        unread: Vec::new(),
        judged: Vec::new(),
        cpu_caps: Vec::new(),
        throttled_for: None,
        busiest: Vec::new(),
        processes: Vec::new(),
        cpu_ticks: None,
        cpufreq_cooling: false,
        hot_held: false,
    };
    assert_eq!(
        said(&reading),
        (
            Level::Error,
            "the disk is all but full: 1% of it is free".into()
        )
    );
    // Among warnings: the hottest zone, then the disk, the memory, the
    // load, the clock, and what could not be read.
    reading.disk_free = Some(0.05);
    assert_eq!(
        said(&reading).1,
        "cpub_thermal_zone is at 72 °C: the board slows itself down from 70"
    );
    // Without the zone that is slowing itself down. The other is at 66,
    // as the board is at rest, and is nothing to say.
    reading.zones.truncate(2);
    assert_eq!(said(&reading).1, "the disk is filling up: 5% of it is free");
    reading.disk_free = Some(0.5);
    assert_eq!(said(&reading).1, "95% of the memory is in use");
    reading.memory_used = Some(0.5);
    assert!(
        said(&reading)
            .1
            .starts_with("the processors have more work")
    );
    reading.cpu_load = Some(0.5);
    assert_eq!(said(&reading).1, "the clock is not set from the network");
    reading.clock_synced = Some(true);
    assert_eq!(said(&reading), (Level::Ok, String::new()));
    // What could not be read is a warning, after whatever else is one,
    // and an error found in what was read is still said first.
    reading.uptime_s = None;
    reading.unread.push(Unread {
        what: "uptime_s",
        why: "/proc/uptime: gone".into(),
    });
    assert_eq!(
        said(&reading),
        (Level::Warn, "could not be read: uptime_s".into())
    );
    reading.zones[0].milli_c = 95_000;
    assert_eq!(said(&reading).0, Level::Error);
    // The values are the numbers that were read, and the hottest zone;
    // of two as hot, the one read first.
    reading.zones = vec![
        zone("a_zone", 41_000, &[], None),
        zone("b_zone", 52_300, &[], None),
        zone("c_zone", 52_300, &[], None),
    ];
    let report = reading.report();
    assert_eq!(value(&report, "hottest_zone"), Some("b_zone"));
    assert_eq!(value(&report, "hottest_c"), Some("52.3"));
    assert_eq!(value(&report, "uptime_s"), None);
    assert_eq!(value(&report, "cpu_load"), Some("0.50"));
}

#[test]
fn the_kernel_is_asked_about_the_clock_only_when_named_and_the_marker_still_counts_first() {
    let tree = Tree::real_board("kernel-clock");
    // Today's default: no timesync directory, nobody asked.
    assert_eq!(board::read(&tree.places()).clock_synced, None);
    // Named: the kernel's answer, the same one `timedatectl` gives.
    let kernel = board::Places {
        clock: board::ClockSource::Kernel,
        ..tree.places()
    };
    assert_eq!(
        board::read(&kernel).clock_synced,
        board::kernel_clock_synced()
    );
    #[cfg(target_os = "linux")]
    assert!(board::kernel_clock_synced().is_some());
    // timesyncd's marker is believed before the kernel.
    tree.file("run/systemd/timesync/synchronized", "");
    assert_eq!(board::read(&kernel).clock_synced, Some(true));
    assert_eq!("kernel".parse(), Ok(board::ClockSource::Kernel));
    assert_eq!("timesync".parse(), Ok(board::ClockSource::Timesync));
    assert!("ntp".parse::<board::ClockSource>().is_err());
}

/// A processor zone whose trip is far off: only [`board::CPU_WARN_C`]
/// speaks for it.
fn processors_at(milli_c: i64) -> Reading {
    Reading {
        zones: vec![
            zone("skin_zone", 37_000, &[], None),
            zone("cpub_thermal_zone", milli_c, &[100_000], Some(110_000)),
        ],
        ..well()
    }
}

/// The hardware side (via the owner, 2026-10-16 16:53): the big cores run
/// around 85 °C, and from 95 it is worth saying, with the trip they are
/// nearest and the busiest processes; below, nothing.
#[test]
fn hot_processors_are_a_warning_from_95_naming_the_busiest() {
    assert_eq!(said(&processors_at(94_900)), (Level::Ok, String::new()));
    let mut hot = processors_at(96_000);
    hot.busiest = vec![
        ("lamp-eyes".into(), 0.44),
        ("lamp-voice".into(), 0.20),
        ("lamp-turn-end".into(), 0.14),
    ];
    assert_eq!(
        said(&hot),
        (
            Level::Warn,
            "the processors are hot: 96 °C at cpub_thermal_zone, 4 below the board's trip at 100; busiest: lamp-eyes 44%, lamp-voice 20%, lamp-turn-end 14%"
                .into()
        )
    );
    let report = hot.report();
    assert_eq!(value(&report, "cpu_zone"), Some("cpub_thermal_zone"));
    assert_eq!(value(&report, "cpu_c"), Some("96.0"));
    assert_eq!(value(&report, "cpu_next_trip_c"), Some("100"));
    assert_eq!(value(&report, "throttled"), None, "no cpufreq read");
}

/// A processor group held below what it can do for a minute is an error:
/// the board keeps slowing the lamp down. Held for less, it is only said in
/// the values; not held, `throttled` is false.
#[test]
fn processors_held_back_for_a_minute_are_an_error() {
    let mut reading = processors_at(93_000);
    reading.cpu_caps = vec![
        board::CpuCap {
            policy: "policy0".into(),
            max_khz: 1_794_000,
            hardware_khz: 1_794_000,
        },
        board::CpuCap {
            policy: "policy6".into(),
            max_khz: 1_792_000,
            hardware_khz: 2_002_000,
        },
    ];
    // A cap below the hardware's with nothing cooling and the zones under
    // their first trip is not heat (the reviewer on #1105): boost speeds left
    // out, or an image's own cap.
    reading.throttled_for = Some(board::THROTTLED_ERROR_AFTER);
    assert_eq!(
        said(&reading),
        (Level::Ok, String::new()),
        "no cooling, no heat"
    );
    assert_eq!(value(&reading.report(), "throttled"), Some("false"));
    // The board says it is cooling.
    reading.cpufreq_cooling = true;
    reading.throttled_for = Some(std::time::Duration::from_secs(30));
    assert_eq!(
        said(&reading),
        (Level::Ok, String::new()),
        "not yet a minute"
    );
    assert_eq!(value(&reading.report(), "throttled"), Some("true"));
    assert_eq!(value(&reading.report(), "throttled_s"), Some("30"));
    reading.throttled_for = Some(board::THROTTLED_ERROR_AFTER);
    reading.busiest = vec![("lamp-eyes".into(), 0.44)];
    assert_eq!(
        said(&reading),
        (
            Level::Error,
            "the processors have been held back to cool for 60 s: policy6 may run at 1792 of 2002 MHz, cpub_thermal_zone at 93 °C; busiest: lamp-eyes 44%"
                .into()
        )
    );
    let report = reading.report();
    assert_eq!(value(&report, "cpu_max_mhz"), Some("1792"));
    assert_eq!(value(&report, "cpu_hardware_mhz"), Some("2002"));
    reading.cpu_caps[1].max_khz = 2_002_000;
    reading.throttled_for = None;
    reading.zones[1].milli_c = 60_000;
    assert_eq!(said(&reading), (Level::Ok, String::new()));
    assert_eq!(value(&reading.report(), "throttled"), Some("false"));
}

/// The watch the lamp's reader keeps: how long the cap has held, from the
/// first reading that showed it until one that does not; and the processes'
/// share of one core between two readings, the busiest first, three at most.
#[test]
fn the_watch_counts_the_held_time_and_finds_the_busiest() {
    use std::time::{Duration, Instant};
    let capped = |held: bool, ticks: [(u32, &str, u64); 4], total: u64| Reading {
        cpu_caps: vec![board::CpuCap {
            policy: "policy6".into(),
            max_khz: if held { 1_792_000 } else { 2_002_000 },
            hardware_khz: 2_002_000,
        }],
        processes: ticks
            .iter()
            .map(|(pid, name, t)| (*pid, name.to_string(), *t))
            .collect(),
        cpu_ticks: Some((total, 8)),
        cpufreq_cooling: held,
        ..processors_at(90_000)
    };
    let mut watch = board::Watch::default();
    let start = Instant::now();
    let mut first = capped(
        true,
        [
            (1, "lamp-eyes", 100),
            (2, "lamp-voice", 50),
            (3, "lamp-turn-end", 10),
            (4, "sleepy", 5),
        ],
        8_000,
    );
    watch.observe(&mut first, start);
    assert_eq!(first.throttled_for, Some(Duration::ZERO));
    assert!(first.busiest.is_empty(), "nothing to compare with yet");
    // 800 ticks across 8 cores later: 100 a core.
    let mut second = capped(
        true,
        [
            (1, "lamp-eyes", 144),
            (2, "lamp-voice", 70),
            (3, "lamp-turn-end", 24),
            (4, "sleepy", 6),
        ],
        8_800,
    );
    watch.observe(&mut second, start + Duration::from_secs(70));
    assert_eq!(second.throttled_for, Some(Duration::from_secs(70)));
    let busiest: Vec<(&str, u32)> = second
        .busiest
        .iter()
        .map(|(name, share)| (name.as_str(), (share * 100.0).round() as u32))
        .collect();
    assert_eq!(
        busiest,
        [("lamp-eyes", 44), ("lamp-voice", 20), ("lamp-turn-end", 14)]
    );
    let mut third = capped(
        false,
        [
            (1, "lamp-eyes", 150),
            (2, "lamp-voice", 71),
            (3, "lamp-turn-end", 25),
            (4, "sleepy", 6),
        ],
        9_600,
    );
    watch.observe(&mut third, start + Duration::from_secs(75));
    assert_eq!(third.throttled_for, None, "no longer held");
}

/// Each process's CPU time is read from `N/stat`, its name kept whole even
/// with spaces and parentheses in it; the processors' ticks from `stat`.
#[test]
fn processes_and_the_processors_ticks_are_read_from_proc() {
    let tree = Tree::real_board("busiest");
    tree.file(
        "proc/123/stat",
        "123 (lamp (voice) x) S 1 1 1 0 -1 4194560 100 0 0 0 40 2 0 0 20 0 9 0 50 0 0\n",
    );
    tree.file("proc/124/stat", "garbage\n");
    let reading = board::read(&tree.places());
    assert!(
        reading
            .processes
            .contains(&(123, "lamp (voice) x".into(), 42))
    );
    assert!(reading.processes.iter().all(|(pid, _, _)| *pid != 124));
    let (total, cores) = reading.cpu_ticks.expect("the real board's stat");
    assert!(total > 0 && cores > 0);
}

/// The caps are read from `devices/system/cpu/cpufreq/policyN`, by number;
/// a policy missing a number is left out.
#[test]
fn processor_caps_are_read_from_each_cpufreq_policy() {
    let tree = Tree::real_board("cpu-caps");
    tree.file(
        "sys/devices/system/cpu/cpufreq/policy4/scaling_max_freq",
        "1792000\n",
    );
    tree.file(
        "sys/devices/system/cpu/cpufreq/policy4/cpuinfo_max_freq",
        "2016000\n",
    );
    tree.file(
        "sys/devices/system/cpu/cpufreq/policy0/scaling_max_freq",
        "1800000\n",
    );
    tree.file(
        "sys/devices/system/cpu/cpufreq/policy0/cpuinfo_max_freq",
        "1800000\n",
    );
    tree.file(
        "sys/devices/system/cpu/cpufreq/policy6/scaling_max_freq",
        "1000\n",
    );
    // The real board's cooling devices: idle injection, then cpufreq, at work.
    tree.file("sys/class/thermal/cooling_device0/type", "idle-cpu2\n");
    tree.file("sys/class/thermal/cooling_device0/cur_state", "5\n");
    tree.file("sys/class/thermal/cooling_device7/type", "cpufreq-cpu6\n");
    tree.file("sys/class/thermal/cooling_device7/cur_state", "0\n");
    assert!(
        !board::read(&tree.places()).cpufreq_cooling,
        "idle injection is not cpufreq"
    );
    tree.file("sys/class/thermal/cooling_device7/cur_state", "3\n");
    let reading = board::read(&tree.places());
    let caps: Vec<(&str, u64, u64)> = reading
        .cpu_caps
        .iter()
        .map(|cap| (cap.policy.as_str(), cap.max_khz, cap.hardware_khz))
        .collect();
    assert_eq!(
        caps,
        [
            ("policy0", 1_800_000, 1_800_000),
            ("policy4", 1_792_000, 2_016_000)
        ]
    );
    assert_eq!(reading.throttled().unwrap().policy, "policy4");
}

/// The reviewer on #1105: a zone at the line must not flip the lamp's health
/// on every reading. Once given, the warning holds until the zone is
/// [`board::CPU_WARN_CLEARS_BELOW_C`] under it.
#[test]
fn the_heat_warning_holds_until_the_processors_cool_a_little() {
    use std::time::{Duration, Instant};
    let mut watch = board::Watch::default();
    let start = Instant::now();
    let mut levels = Vec::new();
    for (step, milli_c) in [94_000, 95_000, 94_500, 93_000, 92_000, 91_900, 94_900]
        .into_iter()
        .enumerate()
    {
        let mut reading = processors_at(milli_c);
        watch.observe(&mut reading, start + Duration::from_secs(5 * step as u64));
        levels.push(said(&reading).0);
    }
    use Level::{Ok, Warn};
    assert_eq!(levels, [Ok, Warn, Warn, Warn, Warn, Ok, Ok]);
}
