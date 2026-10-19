//! The body's per-lamp poses, as the factory writes them once per lamp
//! (#1682): `/etc/lampOS/body/lamp.env`'s `BODY_POSES`, which the product's
//! `lamp-body.service` passes to the body and without which it does not
//! start (`deploy/product/README.md`). Made from one read-only inspection
//! of the lamp at rest with torque off (`lamp-body-inspect`'s plan): nothing
//! here writes a register or moves a joint.
//!
//! What a rest read gives, and what it does not (docs/body.md, install.md):
//! - `--home` (pitch, roll, yaw) and `--folded-arm` (shoulder, elbow): where
//!   the joints lie at rest, read here;
//! - `--stand`: lamp-4ace's standing pitch, shoulder and elbow, with this
//!   lamp's own roll and yaw;
//! - `--pitch-window`: the default (lamp-4ace's), checked to hold the rest
//!   pitch and to lie inside the pitch servo's own stored limits;
//! - `--gaze`: not here. It is measured with a face and the camera
//!   (body.md), so the file says it is not measured and the body runs
//!   without it.
use crate::inspection::{InspectionReport, Readback, Stage};
use std::fmt::Write as _;

/// The bus IDs (`lib.rs`): base yaw, shoulder, elbow, head roll, head pitch.
const YAW: u8 = 1;
const SHOULDER: u8 = 2;
const ELBOW: u8 = 3;
const ROLL: u8 = 4;
const PITCH: u8 = 5;

/// lamp-4ace's standing pose's pitch, shoulder and elbow (install.md's
/// table, RUNBOOK.md): every lamp stands to these, with its own roll and yaw.
pub const STAND_PITCH: u16 = 1772;
pub const STAND_SHOULDER: u16 = 2657;
pub const STAND_ELBOW: u16 = 2602;
/// The body's default pitch window (`lamp-body --pitch-window`, lamp-4ace's).
pub const PITCH_WINDOW: (u16, u16) = (1740, 2030);
/// The head's floor, under which no window may reach (`lamp-body`).
pub const PITCH_FLOOR: u16 = 1670;
/// How far the shoulder and the elbow may each stand from folded
/// (`body.rs`'s `TRAVEL_CAP`).
pub const TRAVEL_CAP: u16 = 1_400;

/// The five joints as read at rest, torque off, and the pitch servo's own
/// stored limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestRead {
    pub yaw: u16,
    pub shoulder: u16,
    pub elbow: u16,
    pub roll: u16,
    pub pitch: u16,
    pub pitch_limits: (u16, u16),
}

/// The rest read from one completed inspection: every joint's position from
/// its telemetry, torque off on all five (so where it lies is where it rests,
/// not where it was held), and the pitch servo's limits. Anything else is
/// refused, with why.
pub fn rest_read(report: &InspectionReport) -> Result<RestRead, String> {
    if !report.completed || report.fault.is_some() {
        return Err("the inspection did not complete without a fault".into());
    }
    let replies = |stage: Stage| {
        report
            .transactions
            .iter()
            .find(|transaction| transaction.stage == stage && transaction.fault.is_none())
            .map(|transaction| &transaction.replies)
            .ok_or(format!("the inspection has no clean {stage:?} read"))
    };
    let mut position = [None; 6];
    for reply in replies(Stage::Telemetry)? {
        if let Readback::Telemetry { position_raw, .. } = reply.data
            && let Some(slot) = position.get_mut(usize::from(reply.joint_id))
        {
            *slot = Some(position_raw);
        }
    }
    for reply in replies(Stage::TorqueAndProfile)? {
        if let Readback::TorqueAndProfile {
            torque_enable_raw, ..
        } = reply.data
            && torque_enable_raw != 0
        {
            return Err(format!(
                "joint {} has torque on: read the lamp at rest with torque off on all five",
                reply.joint_id
            ));
        }
    }
    let pitch_limits = replies(Stage::Limits)?
        .iter()
        .find_map(|reply| match reply.data {
            Readback::Limits { min_raw, max_raw } if reply.joint_id == PITCH => {
                Some((min_raw, max_raw))
            }
            _ => None,
        })
        .ok_or("the inspection has no limits for the head's pitch")?;
    let at =
        |joint: u8| position[usize::from(joint)].ok_or(format!("joint {joint} has no position"));
    Ok(RestRead {
        yaw: at(YAW)?,
        shoulder: at(SHOULDER)?,
        elbow: at(ELBOW)?,
        roll: at(ROLL)?,
        pitch: at(PITCH)?,
        pitch_limits,
    })
}

/// `lamp.env`'s text for this lamp, from its rest read: the poses as the
/// body takes them (`BODY_POSES`), and comments saying where each came
/// from. Refused, with why, where the body itself would refuse them: a rest
/// pitch outside the window or under the head's floor, a window outside the
/// pitch servo's limits, a stand farther from folded than the body allows,
/// or a position past 4095.
pub fn lamp_env(unit_id: &str, read: &RestRead, measured_at: &str) -> Result<String, String> {
    let all = [read.yaw, read.shoulder, read.elbow, read.roll, read.pitch];
    if all.iter().any(|position| *position > 4095) {
        return Err(format!("a position past 4095: {all:?}"));
    }
    let (low, high) = PITCH_WINDOW;
    if read.pitch < PITCH_FLOOR || !(low..=high).contains(&read.pitch) {
        return Err(format!(
            "the head's pitch rests at {}, outside the window {low}-{high}: measure this lamp's own --pitch-window first (body.md)",
            read.pitch
        ));
    }
    let (min, max) = read.pitch_limits;
    if low < min || high > max {
        return Err(format!(
            "the window {low}-{high} is outside the pitch servo's own limits {min}-{max}: measure this lamp's own --pitch-window first (body.md)"
        ));
    }
    for (joint, folded, standing) in [
        ("shoulder", read.shoulder, STAND_SHOULDER),
        ("elbow", read.elbow, STAND_ELBOW),
    ] {
        if folded.abs_diff(standing) > TRAVEL_CAP {
            return Err(format!(
                "the {joint} folds at {folded}, more than {TRAVEL_CAP} from standing at {standing}: this body cannot stand it"
            ));
        }
    }
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# {unit_id}, read at rest with torque off by lamp-body-measure, {measured_at}."
    );
    let _ = writeln!(
        text,
        "# At rest: yaw {}, shoulder {}, elbow {}, roll {}, pitch {}; the pitch servo's limits {min}-{max}.",
        read.yaw, read.shoulder, read.elbow, read.roll, read.pitch
    );
    let _ = writeln!(
        text,
        "# home, folded-arm: found at rest. stand: lamp-4ace's with this lamp's roll and yaw. pitch-window: the default, checked. gaze: NOT measured (body.md, with a face and the camera)."
    );
    let _ = writeln!(
        text,
        "BODY_POSES=\"--home {},{},{} --folded-arm {},{} --stand {STAND_PITCH},{},{},{STAND_SHOULDER},{STAND_ELBOW} --pitch-window {low},{high} --torque on\"",
        read.pitch, read.roll, read.yaw, read.shoulder, read.elbow, read.roll, read.yaw
    );
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspection::{JointReadback, QuietReport, TransactionReport};

    fn transaction(stage: Stage, replies: Vec<JointReadback>) -> TransactionReport {
        TransactionReport {
            stage,
            started_us: 0,
            first_write_attempt_us: None,
            request_accepted_us: None,
            first_read_completed_us: None,
            ended_us: 0,
            elapsed_us: 0,
            request_bytes_accepted: 0,
            bytes_read: 0,
            io_operations: 0,
            pending_ids: Vec::new(),
            replies,
            fault: None,
        }
    }

    fn reply(joint_id: u8, data: Readback) -> JointReadback {
        JointReadback {
            joint_id,
            host_read_completed_us: 0,
            reply_status_raw: 0,
            data,
        }
    }

    fn telemetry(position_raw: u16) -> Readback {
        Readback::Telemetry {
            register_bytes: [0; 15],
            position_raw,
            velocity_word: 0,
            velocity_signed_raw: 0,
            load_word: 0,
            load_magnitude_raw: 0,
            load_direction_bit: false,
            load_uninterpreted_bits: 0,
            voltage_raw: 0,
            temperature_raw: 0,
            status_raw: 0,
            moving_raw: 0,
            current_raw: 0,
        }
    }

    fn torque(on: bool) -> Readback {
        Readback::TorqueAndProfile {
            register_bytes: [0; 8],
            torque_enable_raw: u8::from(on),
            acceleration_raw: 0,
            goal_position_raw: 0,
            goal_time_raw: 0,
            goal_speed_raw: 0,
        }
    }

    /// lamp-06c1 at rest (deploy/lamp-06c1/lamp.env): yaw 2034, shoulder
    /// 2050, elbow 2059, roll 2057, pitch 1772; lamp-52e6's pitch limits.
    fn inspection(positions: [u16; 5], torque_on: Option<u8>) -> InspectionReport {
        let ids = 1..=5u8;
        InspectionReport {
            completed: true,
            freshness: "fresh",
            transaction_deadline_us: 0,
            telemetry_target_us: 0,
            telemetry_target_met: Some(true),
            quiet: QuietReport::default(),
            transactions: vec![
                transaction(
                    Stage::Limits,
                    ids.clone()
                        .map(|id| {
                            let (min_raw, max_raw) = if id == 5 { (1451, 3067) } else { (0, 4095) };
                            reply(id, Readback::Limits { min_raw, max_raw })
                        })
                        .collect(),
                ),
                transaction(
                    Stage::TorqueAndProfile,
                    ids.clone()
                        .map(|id| reply(id, torque(torque_on == Some(id))))
                        .collect(),
                ),
                transaction(
                    Stage::Telemetry,
                    ids.map(|id| reply(id, telemetry(positions[usize::from(id) - 1])))
                        .collect(),
                ),
            ],
            fault: None,
        }
    }

    const LAMP_06C1: [u16; 5] = [2034, 2050, 2059, 2057, 1772];

    /// #1682: a rest read becomes the file the body takes, matching what was
    /// written by hand for lamp-06c1 (home, folded arm, stand), with the
    /// default window checked and the gaze said to be unmeasured.
    #[test]
    fn a_rest_read_becomes_the_lamps_own_poses() {
        let read = rest_read(&inspection(LAMP_06C1, None)).unwrap();
        assert_eq!(
            read,
            RestRead {
                yaw: 2034,
                shoulder: 2050,
                elbow: 2059,
                roll: 2057,
                pitch: 1772,
                pitch_limits: (1451, 3067),
            }
        );
        let text = lamp_env("lamp-06c1", &read, "2026-10-19T08:00Z").unwrap();
        assert!(text.contains(
            "BODY_POSES=\"--home 1772,2057,2034 --folded-arm 2050,2059 --stand 1772,2057,2034,2657,2602 --pitch-window 1740,2030 --torque on\""
        ));
        assert!(text.contains("gaze: NOT measured"));
        assert!(text.lines().filter(|line| !line.starts_with('#')).count() == 1);
    }

    /// A read with torque on, an unfinished one, or one missing a joint is
    /// never taken for the lamp at rest.
    #[test]
    fn only_a_complete_read_with_torque_off_is_taken() {
        assert!(
            rest_read(&inspection(LAMP_06C1, Some(2)))
                .unwrap_err()
                .contains("joint 2 has torque on")
        );
        let mut unfinished = inspection(LAMP_06C1, None);
        unfinished.completed = false;
        assert!(rest_read(&unfinished).is_err());
        let mut missing = inspection(LAMP_06C1, None);
        missing.transactions[2].replies.pop();
        assert!(rest_read(&missing).unwrap_err().contains("joint 5"));
    }

    /// Where the body itself would refuse, so does the tool: a rest pitch
    /// outside the window, a window outside the servo's limits, an arm that
    /// cannot stand that far.
    #[test]
    fn poses_the_body_would_refuse_are_refused_with_why() {
        let read = rest_read(&inspection(LAMP_06C1, None)).unwrap();
        let low = RestRead {
            pitch: 1700,
            ..read
        };
        assert!(
            lamp_env("x", &low, "t")
                .unwrap_err()
                .contains("--pitch-window")
        );
        let narrow = RestRead {
            pitch_limits: (1800, 3067),
            ..read
        };
        assert!(
            lamp_env("x", &narrow, "t")
                .unwrap_err()
                .contains("servo's own limits")
        );
        let far = RestRead {
            shoulder: STAND_SHOULDER - TRAVEL_CAP - 1,
            ..read
        };
        assert!(
            lamp_env("x", &far, "t")
                .unwrap_err()
                .contains("cannot stand")
        );
        let past = RestRead { yaw: 4096, ..read };
        assert!(lamp_env("x", &past, "t").is_err());
    }
}
