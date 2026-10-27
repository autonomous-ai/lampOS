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
//! - `--pitch-window`: the default (lamp-4ace's), checked to lie inside the
//!   pitch servo's own stored limits; `--home`'s pitch is brought inside the
//!   body's home range (`Limits::FOLDED.pitch_home`) when it rests within
//!   [`HOME_PITCH_TOLERANCE`] of it, or over it within the body's
//!   [`WAKE_REACH`] (a head moved off home by a hand, #3504), and refused
//!   farther out;
//! - `--gaze`: not here. It is measured with a face and the camera
//!   (body.md), so the file says it is not measured and the body runs
//!   without it.
use crate::body::{Limits, WAKE_REACH};
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
/// How far outside the body's home range a rest pitch may read and still be
/// a head resting on its stop (#2396: lamp-0c89 rests at 1755, five under
/// 1760): it is brought to the nearest edge. Farther out, the head is off
/// its stop and the read is refused, unless it is over the range (the side
/// the head falls with its weight) within [`WAKE_REACH`]: a head moved off
/// home by a hand, which the body wakes home (#3504).
pub const HOME_PITCH_TOLERANCE: u16 = 20;
/// How far the shoulder and the elbow may each stand from folded
/// (`body.rs`'s `TRAVEL_CAP`).
pub const TRAVEL_CAP: u16 = 1_400;

/// The five joints as read at rest, torque off, and each servo's own
/// stored limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestRead {
    pub yaw: u16,
    pub shoulder: u16,
    pub elbow: u16,
    pub roll: u16,
    pub pitch: u16,
    /// Each servo's own lower and upper limits, by bus ID less one: yaw,
    /// shoulder, elbow, roll, pitch (#3558).
    pub limits: [(u16, u16); 5],
}

/// The rest read from one completed inspection: every joint's position from
/// its telemetry, torque off on all five (so where it lies is where it rests,
/// not where it was held), and every servo's limits. Anything else is
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
    let mut limits = [None; 6];
    for reply in replies(Stage::Limits)? {
        if let Readback::Limits { min_raw, max_raw } = reply.data
            && let Some(slot) = limits.get_mut(usize::from(reply.joint_id))
        {
            *slot = Some((min_raw, max_raw));
        }
    }
    let limit =
        |joint: u8| limits[usize::from(joint)].ok_or(format!("joint {joint} has no limits"));
    let at =
        |joint: u8| position[usize::from(joint)].ok_or(format!("joint {joint} has no position"));
    Ok(RestRead {
        yaw: at(YAW)?,
        shoulder: at(SHOULDER)?,
        elbow: at(ELBOW)?,
        roll: at(ROLL)?,
        pitch: at(PITCH)?,
        limits: [
            limit(YAW)?,
            limit(SHOULDER)?,
            limit(ELBOW)?,
            limit(ROLL)?,
            limit(PITCH)?,
        ],
    })
}

/// `lamp.env`'s text for this lamp, from its rest read: the poses as the
/// body takes them (`BODY_POSES`), and comments saying where each came
/// from. Refused, with why, where the body itself would refuse them: a rest
/// pitch more than [`HOME_PITCH_TOLERANCE`] under the body's home range or
/// more than [`WAKE_REACH`] over it (nearer, brought to its edge and said), a window outside the
/// pitch servo's limits, a stand farther from folded than the body allows,
/// or a position past 4095.
pub fn lamp_env(unit_id: &str, read: &RestRead, measured_at: &str) -> Result<String, String> {
    let ((low, high), (min, max), pitch, clamp) = head_checked(read)?;
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
    let _ = writeln!(text, "{}", limits_line(read));
    let _ = writeln!(
        text,
        "# home, folded-arm: found at rest. stand: lamp-4ace's with this lamp's roll and yaw. pitch-window: the default, checked. gaze: NOT measured (body.md, with a face and the camera)."
    );
    if let Some(clamp) = clamp {
        let _ = writeln!(text, "# {clamp}");
    }
    let _ = writeln!(
        text,
        "BODY_POSES=\"--home {pitch},{},{} --folded-arm {},{} --stand {STAND_PITCH},{},{},{STAND_SHOULDER},{STAND_ELBOW} --pitch-window {low},{high} --torque on\"",
        read.roll, read.yaw, read.shoulder, read.elbow, read.roll, read.yaw
    );
    Ok(text)
}

/// A low and a high count.
type Span = (u16, u16);

/// The comment line with every servo's own stored limits, as the body reads
/// them at start with `LAMP_BODY_OWN_RANGES=on` (#3558). One storing
/// 0-4095 was never calibrated by V1 and needs the hand sweep.
fn limits_line(read: &RestRead) -> String {
    let [yaw, shoulder, elbow, roll, pitch] = read.limits.map(|(min, max)| format!("{min}-{max}"));
    format!(
        "# The servos' own limits: yaw {yaw}, shoulder {shoulder}, elbow {elbow}, roll {roll}, pitch {pitch}."
    )
}

/// The checks a seated and a standing lamp share: every position inside
/// 4095, `--home`'s pitch inside the body's home range (see
/// [`home_pitch`]), and the window inside the pitch servo's own limits.
/// Returns the window, the servo's limits, `--home`'s pitch and the clamp's
/// line when it was brought inside.
fn head_checked(read: &RestRead) -> Result<(Span, Span, u16, Option<String>), String> {
    let all = [read.yaw, read.shoulder, read.elbow, read.roll, read.pitch];
    if all.iter().any(|position| *position > 4095) {
        return Err(format!("a position past 4095: {all:?}"));
    }
    let (low, high) = PITCH_WINDOW;
    let (pitch, clamp) = home_pitch(read.pitch)?;
    let (min, max) = read.limits[usize::from(PITCH - 1)];
    if low < min || high > max {
        return Err(format!(
            "the window {low}-{high} is outside the pitch servo's own limits {min}-{max}: measure this lamp's own --pitch-window first (body.md)"
        ));
    }
    Ok(((low, high), (min, max), pitch, clamp))
}

/// `--home`'s pitch from the rest pitch, inside the body's home range
/// (`Limits::FOLDED.pitch_home`, where the body must find the head to
/// start, which lies inside `--pitch-window` and over the head's floor). A
/// rest pitch within [`HOME_PITCH_TOLERANCE`] outside it is the head on its
/// stop: brought to the nearest edge, with the line saying so. Over it (the
/// side the head falls with its weight) by more, but within [`WAKE_REACH`],
/// is a head moved off home by a hand (#3504: lamp-52e6 at 2213, lamp-3e83
/// at 2102 after handling, each staying where a hand left it): the high
/// edge is kept, and the body, waking, brings the head home from there.
/// Farther over, or under by more than the tolerance, it is refused.
fn home_pitch(pitch: u16) -> Result<(u16, Option<String>), String> {
    let (low, high) = Limits::FOLDED.pitch_home;
    let (edge, side, off) = if pitch < low {
        (low, "under", low - pitch)
    } else if pitch > high {
        (high, "over", pitch - high)
    } else {
        return Ok((pitch, None));
    };
    let line = if off <= HOME_PITCH_TOLERANCE {
        format!("home pitch {pitch} is {side} the body's home range {low}..{high}: {edge} kept")
    } else if pitch > high && off <= WAKE_REACH {
        format!(
            "home pitch {pitch} is {off} over the body's home range {low}..{high}: a head moved off home by a hand; {edge} kept, and the body wakes it home (within {WAKE_REACH})"
        )
    } else {
        let most = if pitch > high {
            WAKE_REACH
        } else {
            HOME_PITCH_TOLERANCE
        };
        return Err(format!(
            "home pitch {pitch} is {off} {side} the body's home range {low}..{high}, more than {most}: a head off its stop, not a rest reading. Fold the head down onto its stop and measure again; a head that still rests there is outside where this body starts (body.md)"
        ));
    };
    Ok((edge, Some(line)))
}

/// A seated `lamp.env` for this lamp, from its rest read (#2163): `--home`,
/// the pitch window and `--torque on`, so the body commands the head and the
/// base alone and never writes a goal to the shoulder or the elbow, nor
/// torque except off, to let go as V1 does (no `--stand`, no
/// `--folded-arm`). `--torque on` takes hold only of the three it commands
/// when there is no stand (Fifth on #2228); without it the body, read with
/// torque off, refuses to start. For a lamp
/// whose first stand no person has confirmed yet; the standing file
/// ([`lamp_env`]) is written after that, at the factory's watched stand.
/// Refused, with why, as [`lamp_env`] is for the head.
pub fn seated_lamp_env(
    unit_id: &str,
    read: &RestRead,
    measured_at: &str,
) -> Result<String, String> {
    let ((low, high), (min, max), pitch, clamp) = head_checked(read)?;
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# {unit_id}, read at rest with torque off by lamp-body-measure --seated, {measured_at}."
    );
    let _ = writeln!(
        text,
        "# At rest: yaw {}, shoulder {}, elbow {}, roll {}, pitch {}; the pitch servo's limits {min}-{max}.",
        read.yaw, read.shoulder, read.elbow, read.roll, read.pitch
    );
    let _ = writeln!(text, "{}", limits_line(read));
    let _ = writeln!(
        text,
        "# SEATED: the first stand is not confirmed by a person yet, so no stand and no folded arm; torque is taken on the head and base only. home: found at rest. pitch-window: the default, checked. gaze: NOT measured."
    );
    if let Some(clamp) = clamp {
        let _ = writeln!(text, "# {clamp}");
    }
    let _ = writeln!(
        text,
        "BODY_POSES=\"--home {pitch},{},{} --pitch-window {low},{high} --torque on\"",
        read.roll, read.yaw
    );
    Ok(text)
}

/// One turn of a joint, in counts.
const TURN: i32 = 4096;
/// A read farther than this (counts, the short way round) from the read
/// after it is a single misread and dropped by the hand sweep (#3504):
/// 45 degrees, more than a hand turns a joint between two reads.
pub const SWEEP_OUTLIER: u16 = 512;
/// What starts the sweep's comment line in `lamp.env`.
pub const SWEPT_PREFIX: &str = "# Swept by hand:";
/// The joints' names in [`RestRead`]'s order, as the sweep says them.
pub const JOINT_NAMES: [&str; 5] = ["yaw", "shoulder", "elbow", "roll", "pitch"];

/// The signed step from one read to the next, the short way round: a jump
/// of more than half a turn is the 0/4095 seam crossed, not a move.
fn step(from: u16, to: u16) -> i32 {
    let forward = (i32::from(to) - i32::from(from)).rem_euclid(TURN);
    if forward > TURN / 2 {
        forward - TURN
    } else {
        forward
    }
}

/// The arc one joint travels in a hand sweep (#3504), from its reads in
/// order. Each read is unwrapped onto the last, so a joint crossing the
/// 0/4095 seam keeps counting instead of reading the whole turn; a read is
/// taken only once the read after it lies within [`SWEEP_OUTLIER`] of it.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArcTracker {
    pending: Option<u16>,
    last: Option<u16>,
    at: i32,
    low: i32,
    high: i32,
}

/// What a sweep found for one joint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swept {
    /// From `start` the positive way to `end`, wrapping past 4095 to 0 when
    /// `start > end` (elbow 3900-300 is 3900..=4095 and 0..=300).
    Arc { start: u16, end: u16, span: u16 },
    /// A whole turn or more.
    FullTurn,
}

impl ArcTracker {
    /// One read of the joint's position; past 4095 is no position and ignored.
    pub fn feed(&mut self, position: u16) {
        if position >= 4096 {
            return;
        }
        if let Some(pending) = self.pending.replace(position)
            && step(pending, position).unsigned_abs() <= u32::from(SWEEP_OUTLIER)
        {
            self.take(pending);
        }
    }

    fn take(&mut self, position: u16) {
        self.at = match self.last {
            None => {
                self.low = i32::from(position);
                self.high = self.low;
                self.low
            }
            Some(last) => self.at + step(last, position),
        };
        self.last = Some(position);
        self.low = self.low.min(self.at);
        self.high = self.high.max(self.at);
    }

    /// The arc so far; none before two agreeing reads.
    pub fn swept(&self) -> Option<Swept> {
        self.last?;
        let span = self.high - self.low;
        if span >= TURN {
            return Some(Swept::FullTurn);
        }
        let end = |count: i32| u16::try_from(count.rem_euclid(TURN)).expect("inside a turn");
        Some(Swept::Arc {
            start: end(self.low),
            end: end(self.high),
            span: u16::try_from(span).expect("under a turn"),
        })
    }
}

impl std::fmt::Display for Swept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::FullTurn => write!(f, "a full turn"),
            Self::Arc { start, end, .. } if start > end => {
                write!(f, "{start}-{end} (across the seam)")
            }
            Self::Arc { start, end, .. } => write!(f, "{start}-{end}"),
        }
    }
}

impl Swept {
    /// The span in counts and degrees, for the person reading the sweep.
    pub fn span(&self) -> String {
        match *self {
            Self::FullTurn => "4096 counts or more, 360 degrees or more".into(),
            Self::Arc { span, .. } => format!(
                "{span} counts, {:.0} degrees",
                f64::from(span) * 360.0 / 4096.0
            ),
        }
    }
}

/// The sweep's `lamp.env` comment line (#3504), the joints in
/// [`JOINT_NAMES`]' order; a joint with no arc says "not read".
pub fn swept_line(swept: &[Option<Swept>; 5]) -> String {
    let joints: Vec<String> = JOINT_NAMES
        .iter()
        .zip(swept)
        .map(|(name, swept)| match swept {
            Some(swept) => format!("{name} {swept}"),
            None => format!("{name} not read"),
        })
        .collect();
    format!("{SWEPT_PREFIX} {}.", joints.join(", "))
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
                limits: [(0, 4095), (0, 4095), (0, 4095), (0, 4095), (1451, 3067)],
            }
        );
        let text = lamp_env("lamp-06c1", &read, "2026-10-19T08:00Z").unwrap();
        assert!(text.contains(
            "BODY_POSES=\"--home 1772,2057,2034 --folded-arm 2050,2059 --stand 1772,2057,2034,2657,2602 --pitch-window 1740,2030 --torque on\""
        ));
        assert!(text.contains("gaze: NOT measured"));
        assert!(text.contains(
            "# The servos' own limits: yaw 0-4095, shoulder 0-4095, elbow 0-4095, roll 0-4095, pitch 1451-3067."
        ));
        assert!(text.lines().filter(|line| !line.starts_with('#')).count() == 1);
    }

    /// #2163, Fifth's condition: the seated file holds only `--home`, the
    /// pitch window and `--torque on`, nothing that stands or folds the arm; an arm too
    /// far to stand is still seated, and a head the body would refuse is
    /// refused here too.
    #[test]
    fn a_seated_read_holds_only_the_head_and_its_window() {
        let read = rest_read(&inspection(LAMP_06C1, None)).unwrap();
        let text = seated_lamp_env("lamp-06c1", &read, "2026-10-20T08:00Z").unwrap();
        let poses: Vec<&str> = text.lines().filter(|line| !line.starts_with('#')).collect();
        assert_eq!(
            poses,
            ["BODY_POSES=\"--home 1772,2057,2034 --pitch-window 1740,2030 --torque on\""]
        );
        for standing in ["--stand", "--folded-arm"] {
            assert!(!poses[0].contains(standing), "{standing}");
        }
        assert!(text.contains("SEATED"));
        let far = RestRead {
            shoulder: STAND_SHOULDER - TRAVEL_CAP - 1,
            ..read
        };
        assert!(seated_lamp_env("x", &far, "t").is_ok());
        let low = RestRead {
            pitch: 1700,
            ..read
        };
        assert!(
            seated_lamp_env("x", &low, "t")
                .unwrap_err()
                .contains("off its stop")
        );
        assert!(seated_lamp_env("x", &RestRead { yaw: 4096, ..read }, "t").is_err());
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
                .contains("off its stop")
        );
        let mut narrow = read;
        narrow.limits[4] = (1800, 3067);
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

    /// #2396: lamp-0c89's head rests on its stop at 1755, five under the
    /// body's home range, so `--home` takes 1760 and the file says both.
    #[test]
    fn a_home_pitch_just_outside_the_body_s_range_is_brought_to_its_edge() {
        let read = rest_read(&inspection(LAMP_06C1, None)).unwrap();
        let under = lamp_env(
            "x",
            &RestRead {
                pitch: 1755,
                ..read
            },
            "t",
        )
        .unwrap();
        assert!(
            under
                .contains("# home pitch 1755 is under the body's home range 1760..2020: 1760 kept")
        );
        assert!(under.contains("--home 1760,2057,2034 "));
        assert!(under.contains("pitch 1755;"));
        let over = seated_lamp_env(
            "x",
            &RestRead {
                pitch: 2040,
                ..read
            },
            "t",
        )
        .unwrap();
        assert!(
            over.contains("# home pitch 2040 is over the body's home range 1760..2020: 2020 kept")
        );
        assert!(over.contains("--home 2020,2057,2034 "));
        // Neither file says --wake: lamp-body wakes unless given off, and the
        // product unit and the factory's stands say --wake themselves, where
        // a second one is refused. Waking is what lets the live 1755 start.
        assert!(!under.contains("--wake") && !over.contains("--wake"));
    }

    /// More than HOME_PITCH_TOLERANCE outside is a head off its stop.
    #[test]
    fn a_home_pitch_far_outside_the_body_s_range_is_refused() {
        let read = rest_read(&inspection(LAMP_06C1, None)).unwrap();
        let error = lamp_env(
            "x",
            &RestRead {
                pitch: 1739,
                ..read
            },
            "t",
        )
        .unwrap_err();
        assert!(error.contains("home pitch 1739 is 21 under"), "{error}");
        assert!(error.contains("a head off its stop, not a rest reading"));
        assert!(error.contains("measure again"), "{error}");
        let reach = 2020 + WAKE_REACH;
        let error = seated_lamp_env(
            "x",
            &RestRead {
                pitch: reach + 1,
                ..read
            },
            "t",
        )
        .unwrap_err();
        assert!(error.contains("401 over"), "{error}");
        assert!(
            error.contains("more than 400: a head off its stop"),
            "{error}"
        );
    }

    /// #3504: a head a hand moved off home, over the range (the side it falls
    /// with its weight) within the body's wake reach, keeps the high edge,
    /// which the body wakes it to; lamp-52e6 read 2213 after an SD card swap.
    #[test]
    fn a_home_pitch_moved_by_hand_within_the_wake_reach_keeps_the_edge() {
        let read = rest_read(&inspection(LAMP_06C1, None)).unwrap();
        let text = lamp_env(
            "x",
            &RestRead {
                pitch: 2213,
                ..read
            },
            "t",
        )
        .unwrap();
        assert!(text.contains(
            "# home pitch 2213 is 193 over the body's home range 1760..2020: a head moved off home by a hand; 2020 kept, and the body wakes it home (within 400)"
        ));
        assert!(text.contains("--home 2020,2057,2034 "));
        let reach = 2020 + WAKE_REACH;
        let seated = seated_lamp_env(
            "x",
            &RestRead {
                pitch: reach,
                ..read
            },
            "t",
        )
        .unwrap();
        assert!(seated.contains("--home 2020,2057,2034 "), "{seated}");
        assert!(seated.contains("400 over"), "{seated}");
    }

    /// A rest pitch inside the range, its edges too, is kept as read.
    #[test]
    fn a_home_pitch_inside_the_body_s_range_is_kept_as_read() {
        let read = rest_read(&inspection(LAMP_06C1, None)).unwrap();
        for pitch in [1760, 1772, 2020] {
            let text = lamp_env("x", &RestRead { pitch, ..read }, "t").unwrap();
            assert!(text.contains(&format!("--home {pitch},2057,2034 ")));
            assert!(!text.contains("kept"));
        }
    }

    fn swept(reads: &[u16]) -> Option<Swept> {
        let mut tracker = ArcTracker::default();
        for read in reads {
            tracker.feed(*read);
        }
        tracker.swept()
    }

    /// lamp-3e83's yaw (hw-first on #3558): a plain arc, end to end and back.
    #[test]
    fn a_plain_sweep_is_its_two_ends() {
        let up = (855..=3283).step_by(37).chain([3283, 3283]);
        let down = (855..=3283).rev().step_by(41).chain([855, 855]);
        let found = swept(&up.chain(down).collect::<Vec<u16>>()).unwrap();
        assert_eq!(
            found,
            Swept::Arc {
                start: 855,
                end: 3283,
                span: 2428
            }
        );
        assert_eq!(found.to_string(), "855-3283");
        assert_eq!(found.span(), "2428 counts, 213 degrees");
    }

    /// lamp-3e83's elbow crossed the 0/4095 seam, where a min and max read
    /// the whole turn: the arc keeps its two ends either side of the seam.
    #[test]
    fn a_sweep_across_the_seam_keeps_both_ends() {
        let up: Vec<u16> = (3900..4096)
            .step_by(13)
            .chain((0..=300).step_by(17))
            .chain([300, 300])
            .collect();
        let back: Vec<u16> = up.iter().rev().copied().collect();
        let found = swept(&[up, back].concat()).unwrap();
        assert_eq!(
            found,
            Swept::Arc {
                start: 3900,
                end: 300,
                span: 496
            }
        );
        assert_eq!(found.to_string(), "3900-300 (across the seam)");
    }

    #[test]
    fn a_single_misread_is_dropped() {
        let reads = [2000, 2010, 4000, 2020, 2030, 30, 2040, 2050, 2050];
        assert_eq!(
            swept(&reads),
            Some(Swept::Arc {
                start: 2000,
                end: 2050,
                span: 50
            })
        );
    }

    #[test]
    fn a_whole_turn_or_more_is_a_full_turn() {
        let turns: Vec<u16> = (0..2)
            .flat_map(|_| (0..4096).step_by(100))
            .map(|count| (count + 1000) % 4096)
            .collect();
        let found = swept(&turns).unwrap();
        assert_eq!(found, Swept::FullTurn);
        assert_eq!(found.to_string(), "a full turn");
        // Just short of one stays an arc, across the seam.
        let short: Vec<u16> = (1000..5000)
            .step_by(100)
            .chain([4999, 4999])
            .map(|count| count % 4096)
            .collect();
        assert_eq!(
            swept(&short),
            Some(Swept::Arc {
                start: 1000,
                end: 903,
                span: 3999
            })
        );
    }

    /// A joint left alone jitters a count or two, even sitting on the seam:
    /// a short arc, never a turn.
    #[test]
    fn jitter_at_rest_is_a_short_arc() {
        assert_eq!(
            swept(&[2050, 2052, 2049, 2051, 2050, 2050]),
            Some(Swept::Arc {
                start: 2049,
                end: 2052,
                span: 3
            })
        );
        assert_eq!(
            swept(&[4095, 0, 4094, 1, 0, 4095, 4095]),
            Some(Swept::Arc {
                start: 4094,
                end: 1,
                span: 3
            })
        );
        assert_eq!(swept(&[2050]), None);
    }

    #[test]
    fn the_swept_line_names_every_joint() {
        let arc = |start, end, span| Some(Swept::Arc { start, end, span });
        assert_eq!(
            swept_line(&[
                arc(855, 3283, 2428),
                arc(2056, 3989, 1933),
                arc(3900, 300, 496),
                Some(Swept::FullTurn),
                None,
            ]),
            "# Swept by hand: yaw 855-3283, shoulder 2056-3989, elbow 3900-300 (across the seam), roll a full turn, pitch not read."
        );
    }
}
