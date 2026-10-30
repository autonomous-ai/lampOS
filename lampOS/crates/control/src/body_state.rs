//! Where the body is, as the door's `body.state` event (docs/control.md,
//! "Where the body is"): each joint from `body/joint_states` (the body's
//! ROS JointState, radians from the middle of each servo's turn) in degrees
//! from its home, as `get` `body` counts them (`crate::body_info::from_home`,
//! from `body/limits`), and its temperature from the body's once-a-second
//! `diagnostics` report, under the protocol's joint names. Pure: each bus
//! message goes in and at most one event comes out, never more often than
//! [`EVERY_US`].

use serde_json::{Map, Value, json};

/// A `body.state` goes out at most this often, 25 a second. The body says
/// its joints every fourth step of its 20 ms loop (`lamp-body`'s
/// `bus_out::EVERY_STEPS`), so it is 12.5 a second today: the voice's
/// `body_motion` and the eyes' `full_look` judge moving by that 80 ms.
pub const EVERY_US: u64 = 40_000;
/// The bus's joint names and the protocol's, in the body's servo order
/// (`lamp-body`'s `bus_out::JOINTS`).
pub const JOINTS: [(&str, &str); 5] = [
    ("base_yaw", "yaw"),
    ("shoulder", "shoulder"),
    ("elbow", "elbow"),
    ("head_roll", "roll"),
    ("head_pitch", "pitch"),
];
/// The name the body's diagnostics come under (`lamp-body`'s `ask::SOURCE`).
pub const BODY_SOURCE: &str = "body";
/// A temperature fact in the body's report is this and the bus's joint
/// name: `temperature_base_yaw`.
pub const TEMPERATURE_FACT: &str = "temperature_";
/// A joint turning slower than this, in degrees a second, is still: a
/// held servo's reading wanders a count or two (0.09° each) from step to
/// step, which at 25 a second is under 5°/s.
pub const MOVING_DEG_S: f64 = 5.0;
/// A joint's temperature goes with this long gone and no report saying it
/// again, five of the body's once-a-second reports: an old one never passes
/// for the joint's now (the Reviewer-lead's P3 on #4564).
pub const TEMPERATURE_KEPT_US: u64 = 5_000_000;
/// A servo's turn is 4,096 counts, and its middle is the angle nothing.
const COUNTS_A_TURN: f64 = 4096.0;
const MIDDLE: f64 = 2048.0;

/// The newest of where the body is.
#[derive(Debug, Default)]
pub struct BodyState {
    /// Each joint's temperature, °C, and when the body's report said it,
    /// in [`JOINTS`]' order; none until a report says it.
    temperatures: [Option<(u64, u64)>; 5],
    told_us: Option<u64>,
}

impl BodyState {
    /// The body's report (`diagnostics` under [`BODY_SOURCE`]) at `now_us`:
    /// its temperatures are kept for the next `body.state`s, for
    /// [`TEMPERATURE_KEPT_US`]. Other parts' reports, and facts it doesn't
    /// have, leave them as they were.
    pub fn report(&mut self, payload: &Value, now_us: u64) {
        if payload["name"] != BODY_SOURCE {
            return;
        }
        let Some(values) = payload["values"].as_array() else {
            return;
        };
        for value in values {
            let Some(joint) = value["key"]
                .as_str()
                .and_then(|key| key.strip_prefix(TEMPERATURE_FACT))
            else {
                continue;
            };
            let Some(at) = JOINTS.iter().position(|(bus, _)| *bus == joint) else {
                continue;
            };
            if let Some(reading) = value["value"].as_str().and_then(|v| v.parse().ok()) {
                self.temperatures[at] = Some((reading, now_us));
            }
        }
    }

    /// One `body/joint_states` at `now_us`: the joints as the protocol
    /// names them, or none when the last went out under [`EVERY_US`] ago
    /// or the message isn't one. `limits` is the body's last `body/limits`
    /// (`Null` when not heard), whose homes the degrees count from: a
    /// joint's `deg` and `goal_deg` are `null` until it is known. `lamp` is
    /// the lamp's own state the event carries beside them.
    pub fn joints(
        &mut self,
        payload: &Value,
        limits: &Value,
        lamp: Value,
        now_us: u64,
    ) -> Option<Value> {
        if self
            .told_us
            .is_some_and(|told| now_us.saturating_sub(told) < EVERY_US)
        {
            return None;
        }
        let names = payload["name"].as_array()?;
        let list = |key: &str| -> Vec<Option<f64>> {
            let values = payload[key].as_array().cloned().unwrap_or_default();
            (0..names.len())
                .map(|at| values.get(at).and_then(Value::as_f64))
                .collect()
        };
        let (position, velocity, effort, commanded) = (
            list("position"),
            list("velocity"),
            list("effort"),
            list("commanded"),
        );
        let mut joints = Map::new();
        for (at, name) in names.iter().enumerate() {
            let Some(index) = JOINTS
                .iter()
                .position(|(bus, _)| Some(*bus) == name.as_str())
            else {
                continue;
            };
            let Some(radians) = position[at].filter(|r| r.is_finite()) else {
                continue;
            };
            let bus = JOINTS[index].0;
            let home = crate::body_info::home(limits, bus);
            let from_home = |radians: f64| {
                home.and_then(|home| crate::body_info::from_home(bus, counts(radians) as f64, home))
            };
            let mut joint = Map::new();
            joint.insert("deg".to_owned(), json!(from_home(radians)));
            joint.insert("counts".to_owned(), json!(counts(radians)));
            if let Some(goal) = commanded[at].filter(|r| r.is_finite()) {
                joint.insert("goal_deg".to_owned(), json!(from_home(goal)));
            }
            if let Some(load) = effort[at].filter(|e| e.is_finite()) {
                joint.insert(
                    "load".to_owned(),
                    json!((load.clamp(0.0, 1.0) * 100.0).round() / 100.0),
                );
            }
            // No velocity (the body's first step) is no news of moving.
            joint.insert(
                "moving".to_owned(),
                json!(velocity[at].is_some_and(|v| v.to_degrees().abs() >= MOVING_DEG_S)),
            );
            if let Some((temperature, _)) = self.temperatures[index]
                .filter(|(_, said_us)| now_us.saturating_sub(*said_us) < TEMPERATURE_KEPT_US)
            {
                joint.insert("temp_c".to_owned(), json!(temperature));
            }
            joints.insert(JOINTS[index].1.to_owned(), Value::Object(joint));
        }
        if joints.is_empty() {
            return None;
        }
        self.told_us = Some(now_us);
        Some(json!({"joints": joints, "lamp": lamp}))
    }
}

/// The counts the body read, back from its radians.
fn counts(radians: f64) -> i64 {
    (radians * COUNTS_A_TURN / std::f64::consts::TAU + MIDDLE).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states(yaw: f64, velocity: f64) -> Value {
        json!({
            "name": ["base_yaw", "shoulder", "elbow", "head_roll", "head_pitch"],
            "position": [yaw, 0.5, -0.25, 0.0, 0.1],
            "velocity": [velocity, 0.0, 0.0, 0.0, 0.0],
            "effort": [0.12, 0.3, 0.2, 0.0, 0.05],
            "commanded": [yaw, 0.5, -0.25, 0.0, 0.2],
        })
    }

    /// The body's limits as `body/limits` says them: each joint's home in
    /// counts (the arm's, its folded pose).
    fn limits() -> Value {
        json!({"joints": [
            {"name": "base_yaw", "home": 2015},
            {"name": "head_pitch", "home": 2100},
            {"name": "head_roll", "home": 2048},
            {"name": "shoulder", "folded": 2400},
            {"name": "elbow", "folded": 1800},
        ]})
    }

    #[test]
    fn each_joint_goes_out_under_the_protocols_name_in_degrees_from_home_and_counts() {
        let mut body = BodyState::default();
        let told = body
            .joints(&states(-0.5, 0.0), &limits(), json!({}), 1_000_000)
            .unwrap();
        let joints = &told["joints"];
        assert_eq!(
            joints.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["elbow", "pitch", "roll", "shoulder", "yaw"]
        );
        // -0.5 rad is 326 counts under the middle, 1,722: 293 under the
        // found home, so -25.8°, as `get` `body` says it.
        assert_eq!(joints["yaw"]["counts"], 1722);
        assert_eq!(joints["yaw"]["deg"], -25.8);
        assert_eq!(
            Some(-25.8),
            crate::body_info::from_home("base_yaw", 1722.0, 2015.0)
        );
        // The head looks up with fewer counts: 2,113 is 13 past its home
        // of 2,100, so down, -1.1°, and its goal at 2,178 -6.9°.
        assert_eq!(joints["pitch"]["counts"], 2113);
        assert_eq!(joints["pitch"]["deg"], -1.1);
        assert_eq!(joints["pitch"]["goal_deg"], -6.9);
        // The arm from its folded pose: 0.5 rad is 2,374, 26 under 2,400.
        assert_eq!(joints["shoulder"]["deg"], -2.3);
        assert_eq!(joints["shoulder"]["load"], 0.3);
        assert_eq!(joints["yaw"]["moving"], false);
        assert!(joints["yaw"].get("temp_c").is_none());
    }

    /// Before the body has said its limits no home is known: the counts
    /// go out, and the degrees are `null`, as in `get` `body`.
    #[test]
    fn without_the_bodys_limits_the_degrees_are_null_and_the_counts_go_out() {
        let mut body = BodyState::default();
        let told = body
            .joints(&states(-0.5, 0.0), &Value::Null, json!({}), 1_000_000)
            .unwrap();
        assert_eq!(told["joints"]["yaw"]["counts"], 1722);
        assert!(told["joints"]["yaw"]["deg"].is_null());
        assert!(told["joints"]["pitch"]["goal_deg"].is_null());
    }

    #[test]
    fn at_most_twenty_five_a_second() {
        let mut body = BodyState::default();
        assert!(
            body.joints(&states(0.0, 0.0), &Value::Null, json!({}), 1_000_000)
                .is_some()
        );
        assert!(
            body.joints(&states(0.0, 0.0), &Value::Null, json!({}), 1_020_000)
                .is_none()
        );
        assert!(
            body.joints(&states(0.0, 0.0), &Value::Null, json!({}), 1_039_999)
                .is_none()
        );
        assert!(
            body.joints(&states(0.0, 0.0), &Value::Null, json!({}), 1_040_000)
                .is_some()
        );
    }

    #[test]
    fn a_turning_joint_is_moving_and_a_wandering_one_is_still() {
        let mut body = BodyState::default();
        let told = body
            .joints(&states(0.0, 0.2), &Value::Null, json!({}), 1_000_000)
            .unwrap();
        assert_eq!(told["joints"]["yaw"]["moving"], true);
        // Two counts a step at 25 a second: 4.4°/s.
        let wander = 2.0 * std::f64::consts::TAU / COUNTS_A_TURN * 25.0;
        let told = body
            .joints(&states(0.0, wander), &Value::Null, json!({}), 2_000_000)
            .unwrap();
        assert_eq!(told["joints"]["yaw"]["moving"], false);
    }

    #[test]
    fn the_bodys_report_gives_each_joint_its_temperature_and_others_dont() {
        let mut body = BodyState::default();
        body.report(
            &json!({"name": "eyes", "values": [{"key": "temperature_base_yaw", "value": "90"}]}),
            0,
        );
        body.report(
            &json!({"name": "body", "values": [
                {"key": "mood", "value": "resting"},
                {"key": "temperature_base_yaw", "value": "41"},
                {"key": "temperature_head_pitch", "value": "47"},
                {"key": "temperature_knee", "value": "30"},
            ]}),
            0,
        );
        let told = body
            .joints(&states(0.0, 0.0), &Value::Null, json!({}), 1_000_000)
            .unwrap();
        assert_eq!(told["joints"]["yaw"]["temp_c"], 41);
        assert_eq!(told["joints"]["pitch"]["temp_c"], 47);
        assert!(told["joints"]["elbow"].get("temp_c").is_none());
    }

    /// The Reviewer-lead's P3 on #4564: a report that stopped coming never
    /// leaves its last temperature passing for the joint's now.
    #[test]
    fn a_temperature_goes_once_no_report_has_said_it_for_five_seconds() {
        let mut body = BodyState::default();
        let report = |yaw: &str| json!({"name": "body", "values": [{"key": "temperature_base_yaw", "value": yaw}]});
        body.report(&report("41"), 1_000_000);
        let at = |body: &mut BodyState, now_us: u64| {
            body.joints(&states(0.0, 0.0), &Value::Null, json!({}), now_us)
                .unwrap()["joints"]["yaw"]
                .clone()
        };
        assert_eq!(at(&mut body, 5_900_000)["temp_c"], 41, "4.9 s on");
        assert!(
            at(&mut body, 6_000_000).get("temp_c").is_none(),
            "5 s on, no report since"
        );
        body.report(&report("43"), 7_000_000);
        assert_eq!(
            at(&mut body, 7_100_000)["temp_c"],
            43,
            "a new report brings it back"
        );
    }

    #[test]
    fn a_message_without_joints_or_with_unknown_ones_says_nothing() {
        let mut body = BodyState::default();
        assert!(
            body.joints(&json!({}), &Value::Null, json!({}), 1_000_000)
                .is_none()
        );
        assert!(
            body.joints(
                &json!({"name": ["knee"], "position": [0.1]}),
                &Value::Null,
                json!({}),
                1_000_000
            )
            .is_none()
        );
        // Nothing went out, so the next is not held back.
        assert!(
            body.joints(&states(0.0, 0.0), &Value::Null, json!({}), 1_000_001)
                .is_some()
        );
    }

    #[test]
    fn the_lamps_own_state_rides_along() {
        let mut body = BodyState::default();
        let lamp = json!({"light": {"on": true, "colour": "warm", "level": 3}, "talking": false, "private": false});
        let told = body
            .joints(&states(0.0, 0.0), &Value::Null, lamp.clone(), 1)
            .unwrap();
        assert_eq!(told["lamp"], lamp);
    }
}
