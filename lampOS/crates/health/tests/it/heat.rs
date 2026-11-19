//! The board's heat (`board/heat`, #7879): each line at which the level
//! changes, from both sides, at the board's reading of every five seconds.
use lamp_health::{
    board::{CpuCap, Reading, Zone},
    heat::{self, Change, Heat},
};
use lamp_msgs::{BoardHeat, HeatLevel};
use std::time::{Duration, Instant};

/// The lamp reads its board this often.
const STEP: Duration = Duration::from_secs(5);

/// A processor zone as the lamps have them: passive points at 70 and 90.
fn cpu(name: &str, milli_c: i64) -> Zone {
    Zone {
        name: name.into(),
        milli_c,
        passive_milli_c: vec![70_000, 90_000],
        critical_milli_c: Some(110_000),
    }
}

/// A heat, and the time of its next reading.
struct Walk {
    heat: Heat,
    now: Instant,
    changes: Vec<Change>,
}

impl Walk {
    fn new() -> Self {
        Self {
            heat: Heat::default(),
            now: Instant::now(),
            changes: Vec::new(),
        }
    }

    /// One reading of `zone`, then the next is a step later.
    fn read(&mut self, zone: &Zone, throttled: bool) -> HeatLevel {
        if let Some(change) = self.heat.observe(zone, throttled, self.now) {
            self.changes.push(change);
        }
        self.now += STEP;
        self.heat.level()
    }

    /// The zone at `milli_c` for `secs` seconds: the readings at 0, 5, ...,
    /// `secs` (so `secs` / 5 + 1 of them). The level after the last.
    fn hold(&mut self, milli_c: i64, secs: u64) -> HeatLevel {
        self.hold_zone(&cpu("cpul_thermal_zone", milli_c), secs)
    }

    fn hold_zone(&mut self, zone: &Zone, secs: u64) -> HeatLevel {
        let mut level = self.heat.level();
        for _ in 0..=secs / STEP.as_secs() {
            level = self.read(zone, false);
        }
        level
    }

    /// The levels it changed to, in order.
    fn went(&self) -> Vec<HeatLevel> {
        self.changes.iter().map(|change| change.to).collect()
    }
}

/// A walk that is warm: at 84 °C for 30 s.
fn warm() -> Walk {
    let mut walk = Walk::new();
    assert_eq!(walk.hold(84_000, 30), HeatLevel::Warm);
    walk
}

/// A walk that is hot: at 88 °C for 10 s.
fn hot() -> Walk {
    let mut walk = Walk::new();
    assert_eq!(walk.hold(88_000, 10), HeatLevel::Hot);
    walk
}

#[test]
fn the_trip_is_the_zones_second_passive_point_or_ninety() {
    assert_eq!(heat::trip_milli_c(&cpu("cpub_thermal_zone", 0)), 90_000);
    let own = Zone {
        passive_milli_c: vec![75_000, 95_000],
        ..cpu("cpub_thermal_zone", 0)
    };
    assert_eq!(heat::trip_milli_c(&own), 95_000);
    for passive in [vec![], vec![70_000]] {
        let zone = Zone {
            passive_milli_c: passive,
            critical_milli_c: None,
            ..cpu("cpub_thermal_zone", 0)
        };
        assert_eq!(heat::trip_milli_c(&zone), 90_000);
    }
    assert_eq!(heat::TRIP_WITHOUT_SECOND_C, 90);
    // The lines follow the zone's own trip: warm at 89 with a trip at 95.
    let mut walk = Walk::new();
    let at = |milli_c| Zone {
        milli_c,
        ..own.clone()
    };
    assert_eq!(walk.hold_zone(&at(88_999), 120), HeatLevel::Ok);
    assert_eq!(walk.hold_zone(&at(89_000), 30), HeatLevel::Warm);
}

#[test]
fn warm_is_six_under_the_trip_for_thirty_seconds() {
    // Under the line, however long: ok.
    let mut walk = Walk::new();
    assert_eq!(walk.hold(83_999, 600), HeatLevel::Ok);
    // On it, for 25 s: not yet; for 30 s: warm.
    let mut walk = Walk::new();
    assert_eq!(walk.hold(84_000, 25), HeatLevel::Ok);
    assert_eq!(
        walk.read(&cpu("cpul_thermal_zone", 84_000), false),
        HeatLevel::Warm
    );
    assert_eq!(walk.went(), [HeatLevel::Warm]);
    assert_eq!(walk.changes[0].held, Some(Duration::from_secs(30)));
    // One reading under the line starts its time again.
    let mut walk = Walk::new();
    assert_eq!(walk.hold(86_000, 25), HeatLevel::Ok);
    assert_eq!(walk.hold(83_999, 0), HeatLevel::Ok);
    assert_eq!(walk.hold(86_000, 25), HeatLevel::Ok);
    assert_eq!(walk.hold(86_000, 0), HeatLevel::Warm);
}

#[test]
fn hot_is_two_under_the_trip_for_ten_seconds() {
    // Under the line, however long: warm, never hot.
    let mut walk = Walk::new();
    assert_eq!(walk.hold(87_999, 600), HeatLevel::Warm);
    assert_eq!(walk.went(), [HeatLevel::Warm]);
    // On it for 5 s: not yet; for 10 s: hot, straight from ok.
    let mut walk = Walk::new();
    assert_eq!(walk.hold(88_000, 5), HeatLevel::Ok);
    assert_eq!(walk.hold(88_000, 0), HeatLevel::Hot);
    assert_eq!(walk.went(), [HeatLevel::Hot]);
    assert_eq!(walk.changes[0].held, Some(Duration::from_secs(10)));
    // And from warm.
    let mut walk = warm();
    assert_eq!(walk.hold(88_000, 5), HeatLevel::Warm);
    assert_eq!(walk.hold(88_000, 0), HeatLevel::Hot);
    assert_eq!(walk.went(), [HeatLevel::Warm, HeatLevel::Hot]);
    // One reading under the line starts its time again.
    let mut walk = warm();
    assert_eq!(walk.hold(89_000, 5), HeatLevel::Warm);
    assert_eq!(walk.hold(87_999, 0), HeatLevel::Warm);
    assert_eq!(walk.hold(89_000, 5), HeatLevel::Warm);
    assert_eq!(walk.hold(89_000, 0), HeatLevel::Hot);
}

#[test]
fn the_board_holding_itself_back_is_hot_at_once_and_holds_it_hot() {
    let mut walk = Walk::new();
    assert_eq!(
        walk.read(&cpu("cpul_thermal_zone", 75_000), true),
        HeatLevel::Hot
    );
    assert_eq!(walk.changes[0].held, None);
    // Held back, it stays hot however cool the zone reads.
    for _ in 0..30 {
        assert_eq!(
            walk.read(&cpu("cpul_thermal_zone", 60_000), true),
            HeatLevel::Hot
        );
    }
    // Let go, the cool time starts then: 60 s under 80, then ok.
    assert_eq!(walk.hold(60_000, 55), HeatLevel::Hot);
    assert_eq!(walk.hold(60_000, 0), HeatLevel::Ok);
    assert_eq!(walk.went(), [HeatLevel::Hot, HeatLevel::Ok]);
    // From warm as well.
    let mut walk = warm();
    assert_eq!(
        walk.read(&cpu("cpul_thermal_zone", 84_000), true),
        HeatLevel::Hot
    );
}

#[test]
fn hot_ends_in_warm_five_under_the_trip_for_sixty_seconds() {
    // On the line, however long: still hot.
    let mut walk = hot();
    assert_eq!(walk.hold(85_000, 600), HeatLevel::Hot);
    // Under it for 55 s: not yet; for 60 s: warm.
    let mut walk = hot();
    assert_eq!(walk.hold(84_999, 55), HeatLevel::Hot);
    assert_eq!(walk.hold(84_999, 0), HeatLevel::Warm);
    assert_eq!(walk.went(), [HeatLevel::Hot, HeatLevel::Warm]);
    assert_eq!(walk.changes[1].held, Some(Duration::from_secs(60)));
    // One reading on the line starts its time again.
    let mut walk = hot();
    assert_eq!(walk.hold(82_000, 55), HeatLevel::Hot);
    assert_eq!(walk.hold(85_000, 0), HeatLevel::Hot);
    assert_eq!(walk.hold(82_000, 55), HeatLevel::Hot);
    assert_eq!(walk.hold(82_000, 0), HeatLevel::Warm);
    // Between the two lines, warm again is not hot again: hot needs its
    // own 10 s at 88.
    assert_eq!(walk.hold(87_000, 600), HeatLevel::Warm);
}

#[test]
fn warm_ends_in_ok_ten_under_the_trip_for_sixty_seconds() {
    // On the line, however long: still warm.
    let mut walk = warm();
    assert_eq!(walk.hold(80_000, 600), HeatLevel::Warm);
    // Under it for 55 s: not yet; for 60 s: ok.
    let mut walk = warm();
    assert_eq!(walk.hold(79_999, 55), HeatLevel::Warm);
    assert_eq!(walk.hold(79_999, 0), HeatLevel::Ok);
    assert_eq!(walk.went(), [HeatLevel::Warm, HeatLevel::Ok]);
    // One reading on the line starts its time again.
    let mut walk = warm();
    assert_eq!(walk.hold(70_000, 55), HeatLevel::Warm);
    assert_eq!(walk.hold(80_000, 0), HeatLevel::Warm);
    assert_eq!(walk.hold(70_000, 55), HeatLevel::Warm);
    assert_eq!(walk.hold(70_000, 0), HeatLevel::Ok);
    // Between the lines, ok stays ok: warm needs its own 30 s at 84.
    assert_eq!(walk.hold(83_000, 600), HeatLevel::Ok);
}

#[test]
fn hot_that_cools_past_both_lines_is_ok_at_once_not_warm_first() {
    let mut walk = hot();
    assert_eq!(walk.hold(79_999, 55), HeatLevel::Hot);
    assert_eq!(walk.hold(79_999, 0), HeatLevel::Ok);
    assert_eq!(walk.went(), [HeatLevel::Hot, HeatLevel::Ok]);
}

/// A board whose two processor zones read `cpul` and `cpub`.
fn board(cpul: i64, cpub: i64) -> Reading {
    Reading {
        zones: vec![
            Zone {
                name: "gpu_thermal_zone".into(),
                milli_c: 99_000,
                passive_milli_c: vec![60_000],
                critical_milli_c: Some(110_000),
            },
            cpu("cpul_thermal_zone", cpul),
            cpu("cpub_thermal_zone", cpub),
            Zone {
                name: "skin_zone".into(),
                milli_c: 40_000,
                passive_milli_c: Vec::new(),
                critical_milli_c: Some(50_000),
            },
        ],
        ..Reading::default()
    }
}

#[test]
fn a_reading_is_heat_from_the_hotter_processor_zone_and_names_it() {
    let mut heat = Heat::default();
    let now = Instant::now();
    // The graphics zone is hotter and is not a processor's.
    let (value, change) = heat.read(&board(85_500, 84_000), now).unwrap();
    assert_eq!(
        value,
        BoardHeat {
            level: HeatLevel::Ok,
            zone: "cpul_thermal_zone".into(),
            c: 85.5,
            trip_c: 90.0,
        }
    );
    value.check().unwrap();
    assert_eq!(change, None);
    let mut reading = board(83_000, 86_000);
    reading.busiest = vec![("lamp-voice".into(), 0.78), ("lamp-eyes".into(), 0.24)];
    let (value, change) = heat.read(&reading, now + Duration::from_secs(30)).unwrap();
    assert_eq!(
        (value.level, value.zone.as_str()),
        (HeatLevel::Warm, "cpub_thermal_zone")
    );
    let change = change.unwrap();
    assert_eq!(
        change.to_string(),
        "the board is warm: cpub_thermal_zone 86 °C for 30 s (it slows itself at 90); \
         background work eases off; busiest: lamp-voice 78%, lamp-eyes 24%"
    );
    // A board with no processor zone has no heat.
    let mut none = board(0, 0);
    none.zones.retain(|zone| !zone.name.starts_with("cpu"));
    assert_eq!(heat.read(&none, now + Duration::from_secs(35)), None);
    assert_eq!(heat.level(), HeatLevel::Warm);
}

#[test]
fn a_reading_held_back_to_cool_is_hot_and_the_lines_say_so() {
    let mut heat = Heat::default();
    let now = Instant::now();
    let mut reading = board(86_000, 87_000);
    reading.cpufreq_cooling = true;
    reading.cpu_caps = vec![CpuCap {
        policy: "policy0".into(),
        max_khz: 1_608_000,
        hardware_khz: 1_800_000,
    }];
    let (value, change) = heat.read(&reading, now).unwrap();
    assert_eq!(value.level, HeatLevel::Hot);
    assert_eq!(
        change.unwrap().to_string(),
        "the board is hot: cpub_thermal_zone 87 °C and held back to cool (it slows itself at 90); \
         background work eases off"
    );
    // Let go and cool: ok after 60 s under 80.
    let cool = board(78_600, 79_000);
    assert_eq!(heat.read(&cool, now + STEP).unwrap().1, None);
    let (value, change) = heat.read(&cool, now + STEP + heat::ENDS_AFTER).unwrap();
    assert_eq!(value.level, HeatLevel::Ok);
    assert_eq!(
        change.unwrap().to_string(),
        "the board is cool again: cpub_thermal_zone 79 °C for 60 s; background work back at full pace"
    );
}

#[test]
fn the_lines_are_the_ones_the_design_names() {
    assert_eq!(heat::WARM_BELOW_TRIP_C, 6);
    assert_eq!(heat::WARM_AFTER, Duration::from_secs(30));
    assert_eq!(heat::HOT_BELOW_TRIP_C, 2);
    assert_eq!(heat::HOT_AFTER, Duration::from_secs(10));
    assert_eq!(heat::HOT_ENDS_BELOW_TRIP_C, 5);
    assert_eq!(heat::WARM_ENDS_BELOW_TRIP_C, 10);
    assert_eq!(heat::ENDS_AFTER, Duration::from_secs(60));
}
