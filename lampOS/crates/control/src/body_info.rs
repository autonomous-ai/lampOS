//! `get` `body`: what the lamp's body may do, joint by joint, and where each
//! joint is now (docs/control.md, "The body's limits"), from the body's own
//! `body/limits` and `body/joint_states` on the bus.
//!
//! Each joint is named as `joints` names it (`yaw`, `pitch`, `roll`,
//! `shoulder`, `elbow`), and its degrees are counted from its home: where the
//! head and base were found when the body started, and the arm's folded pose.
//! The servos turn 4,096 counts a turn. The base turns right and the arm
//! rises with more counts, and the head looks up with fewer. So `yaw`,
//! `shoulder` and `elbow` are positive with more counts, `pitch` with fewer,
//! and `roll` with more. The counts are given too, as they are. Nothing here
//! can be raised: they are the body's bounds.
use serde_json::{Value, json};

/// Counts in a turn.
pub const COUNTS_A_TURN: f64 = 4_096.0;
/// A servo's middle, the zero of `body/joint_states`' radians.
const MIDDLE: f64 = 2_048.0;

/// The bus's name of each joint, the protocol's, and whether its degrees
/// grow with its counts.
const NAMES: [(&str, &str, bool); 5] = [
    ("base_yaw", "yaw", true),
    ("shoulder", "shoulder", true),
    ("elbow", "elbow", true),
    ("head_roll", "roll", true),
    ("head_pitch", "pitch", false),
];

fn degrees(counts: f64) -> f64 {
    (counts * 360.0 / COUNTS_A_TURN * 10.0).round() / 10.0
}

/// The one convention for a joint's degrees, for `get` `body`, `body.state`
/// and `joints`' targets: `counts` from `home` (the found home for the head
/// and base, the folded pose for the arm), to a tenth of a degree, positive
/// with more counts for `base_yaw`, `shoulder`, `elbow` and `head_roll`, and
/// with fewer for `head_pitch`, so up is positive. By the bus's joint name;
/// none for a name it doesn't know.
pub fn from_home(bus_name: &str, counts: f64, home: f64) -> Option<f64> {
    let (_, _, grows) = NAMES.iter().find(|(bus, _, _)| *bus == bus_name)?;
    Some(degrees(if *grows { counts - home } else { home - counts }))
}

/// The protocol's name for a joint the bus names `bus_name`: `yaw`,
/// `shoulder`, `elbow`, `roll`, `pitch`.
pub fn protocol_name(bus_name: &str) -> Option<&'static str> {
    NAMES
        .iter()
        .find(|(bus, _, _)| *bus == bus_name)
        .map(|(_, protocol, _)| *protocol)
}

/// The body's state for `get`, from the last `body/limits` (`limits`) and the
/// last `body/joint_states` (`joints`), either `Null` when not heard yet.
pub fn state(limits: &Value, joints: &Value) -> Value {
    let Some(each) = limits["joints"].as_array() else {
        return json!({"known": false, "why": "the body has not said its limits yet"});
    };
    // Where each joint is now, in counts, by the bus's name.
    let now = |name: &str| -> Option<f64> {
        let at = joints["name"]
            .as_array()?
            .iter()
            .position(|joint| joint.as_str() == Some(name))?;
        let radians = joints["position"].as_array()?.get(at)?.as_f64()?;
        Some((radians * COUNTS_A_TURN / std::f64::consts::TAU + MIDDLE).round())
    };
    let mut out = serde_json::Map::new();
    for (bus, protocol, _) in NAMES {
        let Some(joint) = each
            .iter()
            .find(|joint| joint["name"].as_str() == Some(bus))
        else {
            continue;
        };
        let count = |key: &str| joint[key].as_f64();
        // The arm's home is its folded pose.
        let home = count("home").or_else(|| count("folded"));
        let from_home = |counts: f64| home.and_then(|home| from_home(bus, counts, home));
        let (min, max) = (count("min"), count("max"));
        let (low, high) = match (min.and_then(from_home), max.and_then(from_home)) {
            (Some(a), Some(b)) => (Some(a.min(b)), Some(a.max(b))),
            _ => (None, None),
        };
        let at = now(bus);
        out.insert(
            protocol.to_owned(),
            json!({
                "counts": at,
                "deg": at.and_then(from_home),
                "min_deg": low,
                "max_deg": high,
                "min_counts": min,
                "max_counts": max,
                "home_counts": home,
                "folded_counts": count("folded"),
                "stand_counts": count("stand"),
            }),
        );
    }
    let speed = limits["speed"].as_f64();
    json!({
        "known": true,
        "joints": out,
        "stands": limits["stands"],
        "speed_deg_s": speed.map(degrees),
        "acceleration_deg_s2": limits["acceleration"].as_f64().map(degrees),
        "load_limit": limits["force"].as_f64().map(|force| force / 1_000.0),
        "heat_stop_c": limits["heat_stop"],
        "counts_per_turn": COUNTS_A_TURN,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Value {
        json!({
            "joints": [
                {"name": "base_yaw", "min": 1975, "max": 2055, "home": 2015},
                {"name": "shoulder", "min": 1970, "max": 2730, "folded": 2050, "stand": 2650},
                {"name": "elbow", "min": 1967, "max": 2680, "folded": 2047, "stand": 2600},
                {"name": "head_roll", "min": 2014, "max": 2074, "home": 2044},
                {"name": "head_pitch", "min": 1955, "max": 2015, "home": 2000},
            ],
            "speed": 50.0, "acceleration": 400.0, "force": 600, "heat_stop": 62, "stands": true,
        })
    }

    fn radians(counts: f64) -> f64 {
        (counts - MIDDLE) * std::f64::consts::TAU / COUNTS_A_TURN
    }

    #[test]
    fn each_joint_is_in_degrees_from_its_home_with_its_bounds_and_counts() {
        let joints = json!({
            "name": ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"],
            "position": [radians(2_026.0), radians(2_050.0), radians(2_047.0), radians(2_044.0), radians(1_966.0)],
        });
        let body = state(&limits(), &joints);
        assert_eq!(body["known"], true);
        let yaw = &body["joints"]["yaw"];
        assert_eq!(
            (yaw["counts"].as_f64(), yaw["deg"].as_f64()),
            (Some(2_026.0), Some(1.0))
        );
        assert_eq!(
            (yaw["min_deg"].as_f64(), yaw["max_deg"].as_f64()),
            (Some(-3.5), Some(3.5))
        );
        // The head looks up with fewer counts: 34 fewer is 3.0 degrees up.
        let pitch = &body["joints"]["pitch"];
        assert_eq!(pitch["deg"].as_f64(), Some(3.0));
        assert_eq!(
            (pitch["min_deg"].as_f64(), pitch["max_deg"].as_f64()),
            (Some(-1.3), Some(4.0))
        );
        // The arm counts from its folded pose.
        let shoulder = &body["joints"]["shoulder"];
        assert_eq!(
            (shoulder["deg"].as_f64(), shoulder["home_counts"].as_f64()),
            (Some(0.0), Some(2_050.0))
        );
        assert_eq!(shoulder["stand_counts"].as_f64(), Some(2_650.0));
        assert_eq!(body["speed_deg_s"].as_f64(), Some(4.4));
        assert_eq!(body["load_limit"].as_f64(), Some(0.6));
        assert_eq!(body["heat_stop_c"], 62);
    }

    #[test]
    fn one_convention_for_every_joints_degrees() {
        assert_eq!(from_home("base_yaw", 2_026.0, 2_015.0), Some(1.0));
        assert_eq!(
            from_home("head_pitch", 1_966.0, 2_000.0),
            Some(3.0),
            "up is positive"
        );
        assert_eq!(from_home("head_roll", 2_044.0, 2_044.0), Some(0.0));
        assert_eq!(from_home("shoulder", 2_650.0, 2_050.0), Some(52.7));
        assert_eq!(from_home("knee", 1.0, 0.0), None);
        assert_eq!(protocol_name("head_pitch"), Some("pitch"));
    }

    #[test]
    fn before_the_body_says_its_limits_it_is_not_known_and_no_positions_are_none() {
        assert_eq!(state(&Value::Null, &Value::Null)["known"], false);
        let body = state(&limits(), &Value::Null);
        assert!(body["joints"]["yaw"]["counts"].is_null());
        assert_eq!(body["joints"]["yaw"]["min_counts"].as_f64(), Some(1_975.0));
    }
}
