use super::*;
use lamp_bus::Kind;
use std::sync::atomic::{AtomicU64, Ordering};

/// The last message said on a real bus, `value` at `at_us`.
fn said(at_us: u64, value: serde_json::Value) -> Message {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "lamp-update-heat-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = Bus::new(&root).unwrap();
    let mut out = bus.publisher(TOPIC, Kind::Latest, "health").unwrap();
    out.publish_json(at_us, &value).unwrap();
    let message = bus.latest(TOPIC).unwrap().unwrap();
    let _ = std::fs::remove_dir_all(root);
    message
}

fn heat(level: &str) -> serde_json::Value {
    serde_json::json!({"level": level, "zone": "cpub_thermal_zone", "c": 89.0, "trip_c": 90.0})
}

#[test]
fn only_on_turns_it_on() {
    assert!(enabled(Some("on")));
    for value in [
        None,
        Some("off"),
        Some("1"),
        Some("ON"),
        Some(""),
        Some("on "),
    ] {
        assert!(!enabled(value), "{value:?}");
    }
}

/// A fresh, valid hot value is hot; an expired one, one from the future,
/// one that does not pass its check and one that is not heat read as not
/// hot (ok), so a stale reading never holds an install.
#[test]
fn a_stale_or_invalid_value_is_not_hot() {
    let at = 10_000_000;
    let lasts = BoardHeat::LASTS.as_micros() as u64;
    assert_eq!(level_in(&said(at, heat("hot")), at), HeatLevel::Hot);
    assert_eq!(level_in(&said(at, heat("hot")), at + lasts), HeatLevel::Hot);
    for (message, now, label) in [
        (said(at, heat("hot")), at + lasts + 1, "expired"),
        (said(at, heat("hot")), at - 1, "future"),
        (
            said(
                at,
                serde_json::json!({"level": "hot", "zone": "", "c": 95.0, "trip_c": 90.0}),
            ),
            at,
            "not valid",
        ),
        (
            said(at, serde_json::json!({"someone": true})),
            at,
            "not heat",
        ),
    ] {
        assert_eq!(level_in(&message, now), HeatLevel::Ok, "{label}");
        assert!(!holds(level_in(&message, now), true, true), "{label}");
    }
}

/// Only hot holds, only with the switch on, and only on OpenClaw's turn:
/// with the switch off a hot board is ignored, warm never holds, and
/// Hermes's turn in the same one-shot runs on.
#[test]
fn only_hot_with_the_switch_on_holds_openclaws_install() {
    assert!(holds(HeatLevel::Hot, true, true));
    assert!(
        !holds(HeatLevel::Hot, false, true),
        "the switch off ignores heat"
    );
    assert!(!holds(HeatLevel::Warm, true, true));
    assert!(!holds(HeatLevel::Ok, true, true));
    assert!(!holds(HeatLevel::Hot, true, false), "Hermes's turn");
}

/// No bus: never hot, and a read never waits.
#[test]
fn with_no_bus_the_board_is_never_hot() {
    let mut heat = HeatIn::on(None);
    assert_eq!(heat.level(), HeatLevel::Ok);
}

/// A bus whose kept value is hot and fresh reads hot.
#[test]
fn a_kept_hot_value_reads_hot() {
    let root = std::env::temp_dir().join(format!("lamp-update-heat-in-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let bus = Bus::new(&root).unwrap();
    let mut out = bus.publisher(TOPIC, Kind::Latest, "health").unwrap();
    out.publish_json(lamp_bus::monotonic_us(), &heat("hot"))
        .unwrap();
    let mut heat_in = HeatIn::on(Some(Bus::new(&root).unwrap()));
    assert_eq!(heat_in.level(), HeatLevel::Hot);
    let _ = std::fs::remove_dir_all(root);
}
