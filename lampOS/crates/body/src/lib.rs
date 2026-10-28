//! Feetech STS3215 protocol, per-unit calibration, and bounded motor control.
//!
//! Linux provides one finite read-only inspection with explicit device/unit/lock
//! paths. The control core emits selected position sync writes through an explicitly
//! owned nonblocking transport. The opt-in Linux motion adapter reuses the
//! serial owner; an explicit ID5-only profile preparation uses guarded SRAM
//! writes with ACK/readback. The Linux inspection API remains read-only.
//! The separate opt-in worker executable uses this same boundary. There is no
//! torque API, automatic gesture, or automatic recovery. Packet integrity,
//! authority and caller-supplied qualification are not physical safety evidence.

use std::fmt;

// Reuse the same inert protocol fixture in integration and supervisor unit tests.
#[cfg(test)]
extern crate self as lamp_body;

pub mod ask;
#[cfg(any(target_os = "linux", test))]
pub mod body;
#[cfg(any(target_os = "linux", test))]
pub mod bus_out;
#[cfg(any(target_os = "linux", test))]
pub mod exit_report;
#[cfg(any(target_os = "linux", test))]
pub mod expression_in;
pub mod hold_in;
#[cfg(any(target_os = "linux", test))]
pub mod inspection;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(any(target_os = "linux", test))]
pub mod name_in;
pub mod pause_in;
pub mod sleep_in;

pub mod calibration;
pub mod clips;
pub mod cog;
pub mod command;
pub mod control;
pub mod first_stand;
pub mod gaze;
pub mod gesture;
pub mod idle;
pub mod lease;
pub mod moves;
#[cfg(any(target_os = "linux", test))]
pub mod poses;
pub mod protocol;
pub mod readback;
pub mod runtime;
pub mod trajectory;
pub mod units;
pub mod unplug;

/// The most a lamp may raise its head's pitch stall bound to
/// (`--head-stall-load`, `body::Limits::head_stall_load`): under the head's
/// own load bound (240 for six steps), so a head pushed hard is still caught
/// by that. Here, outside the Linux-only `body` module, so the binary's option
/// is checked on every target. lamp-75c9's head stood up holds 140 short of
/// its goal, so at 100 it was told to stop pushing 20 counts low every time it
/// stood (28 Oct); at 230 it reached its goals with no stall in 18 minutes of
/// following a hand.
pub const HEAD_STALL_LOAD_MOST: u16 = 230;

pub const JOINT_COUNT: usize = 5;
/// A joint that gave way and then moved more than this many counts from
/// where it gave way, while in a hand, was moved by the hand (`body.rs`,
/// `in_hand_after`). Here, outside the Linux-only `body` module, so onboarding's
/// giving-way step (#1800) judges by the same rule.
pub const HAND_MOVED: u16 = 15;
/// Model number from the pinned STS3215 SDK table, not a device observation.
pub const STS3215_MODEL_NUMBER: u16 = 777;

/// Fixed Lamp wiring identity. These are bus IDs, not array offsets.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum Joint {
    BaseYaw = 1,
    BasePitch = 2,
    ElbowPitch = 3,
    WristRoll = 4,
    WristPitch = 5,
}

impl Joint {
    pub const ALL: [Self; JOINT_COUNT] = [
        Self::BaseYaw,
        Self::BasePitch,
        Self::ElbowPitch,
        Self::WristRoll,
        Self::WristPitch,
    ];

    pub const fn id(self) -> u8 {
        self as u8
    }

    pub const fn index(self) -> usize {
        self as usize - 1
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::BaseYaw => "base_yaw",
            Self::BasePitch => "base_pitch",
            Self::ElbowPitch => "elbow_pitch",
            Self::WristRoll => "wrist_roll",
            Self::WristPitch => "wrist_pitch",
        }
    }

    pub(crate) const fn mask(self) -> u8 {
        1 << self.index()
    }
}

impl TryFrom<u8> for Joint {
    type Error = UnknownJointId;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::BaseYaw),
            2 => Ok(Self::BasePitch),
            3 => Ok(Self::ElbowPitch),
            4 => Ok(Self::WristRoll),
            5 => Ok(Self::WristPitch),
            _ => Err(UnknownJointId(value)),
        }
    }
}

impl fmt::Display for Joint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownJointId(pub u8);

impl fmt::Display for UnknownJointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "servo ID {} is not one of Lamp's five joints", self.0)
    }
}

impl std::error::Error for UnknownJointId {}
