use super::*;
use crate::{activity, pose};
use lamp_bus::Kind;
use std::sync::atomic::{AtomicU64, Ordering};

/// The last message said on a real bus, `value` at `at_us`.
fn said(at_us: u64, value: serde_json::Value) -> Message {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "lamp-seen-heat-{}-{}",
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
    serde_json::json!({"level": level, "zone": "cpub_thermal_zone", "c": 86.0, "trip_c": 90.0})
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

/// A fresh, valid value says its level; an old one, one from the future,
/// one that does not pass its check and one that is not heat are ok.
#[test]
fn a_fresh_valid_value_says_its_level_and_anything_else_is_ok() {
    let at = 10_000_000;
    let lasts = BoardHeat::LASTS.as_micros() as u64;
    assert_eq!(level_in(&said(at, heat("hot")), at), HeatLevel::Hot);
    assert_eq!(
        level_in(&said(at, heat("warm")), at + lasts),
        HeatLevel::Warm
    );
    assert_eq!(level_in(&said(at, heat("ok")), at), HeatLevel::Ok);
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
    }
}

#[test]
fn the_pace_at_each_level() {
    let ok = Pace::at(HeatLevel::Ok, true);
    assert_eq!(ok, Pace::FULL);
    assert_eq!(
        (ok.burst_every_us, ok.posture_every_us),
        (60_000_000, 30_000_000)
    );
    let warm = Pace::at(HeatLevel::Warm, true);
    assert_eq!(
        (warm.burst_every_us, warm.posture_every_us),
        (activity::EVERY_US * 2, pose::EVERY_US)
    );
    let hot = Pace::at(HeatLevel::Hot, true);
    assert_eq!(
        (hot.burst_every_us, hot.posture_every_us),
        (activity::EVERY_US * 4, 60_000_000)
    );
}

#[test]
fn the_switch_off_keeps_todays_pace_at_every_level() {
    for level in [HeatLevel::Ok, HeatLevel::Warm, HeatLevel::Hot] {
        assert_eq!(Pace::at(level, false), Pace::FULL, "{level:?}");
    }
}

#[test]
fn one_line_says_each_pace() {
    assert_eq!(
        Pace::at(HeatLevel::Hot, true).line(HeatLevel::Hot),
        "board hot: bursts every 240 s, posture one a minute"
    );
    assert_eq!(
        Pace::at(HeatLevel::Warm, true).line(HeatLevel::Warm),
        "board warm: bursts every 120 s"
    );
    assert_eq!(
        Pace::at(HeatLevel::Ok, true).line(HeatLevel::Ok),
        "board ok: bursts every 60 s"
    );
}

/// Heron's HOLD on #7927: at hot, posture's 60 s would outrun a held
/// sitting's 45 s first answer and double its two-far-in-a-row; while a
/// hold is open, posture keeps today's 30 s, and bursts keep the hot pace.
#[test]
fn a_held_sitting_keeps_posture_at_todays_pace_at_hot() {
    let hot = Pace::at(HeatLevel::Hot, true);
    let held = hot.holding(true);
    assert_eq!(held.posture_every_us, Pace::FULL.posture_every_us);
    assert!(held.posture_every_us < crate::hold::FIRST_ANSWER_US);
    assert_eq!(held.burst_every_us, hot.burst_every_us);
    assert_eq!(hot.holding(false), hot);
    for level in [HeatLevel::Ok, HeatLevel::Warm] {
        let pace = Pace::at(level, true);
        assert_eq!(pace.holding(true), pace, "{level:?}");
    }
}

/// #7401: warm and hot multiply whichever burst rate is in force, a
/// minute or 15 s with `LAMP_SEEN_BURST_15S=on`.
#[test]
fn the_heat_multiplies_the_burst_rate_in_force() {
    for base in [crate::activity::EVERY_US, crate::activity::FAST_EVERY_US] {
        let at = |level| Pace::at_every(level, true, base).burst_every_us;
        assert_eq!(
            (at(HeatLevel::Ok), at(HeatLevel::Warm), at(HeatLevel::Hot)),
            (base, 2 * base, 4 * base)
        );
        assert_eq!(
            Pace::at_every(HeatLevel::Hot, false, base).burst_every_us,
            base
        );
    }
    assert_eq!(
        Pace::at(HeatLevel::Hot, true),
        Pace::at_every(HeatLevel::Hot, true, crate::activity::EVERY_US)
    );
}
