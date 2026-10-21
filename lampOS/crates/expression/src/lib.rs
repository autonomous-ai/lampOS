//! Expression's deterministic planner. The clock and every observation are
//! supplied by the caller. Only the runtime module opens bus connections; the
//! planner opens no device and starts no background worker.

mod alive;
mod catalogue;
mod engine;
pub mod local_time;
pub mod replay;
pub mod runtime;
mod speaking;
mod unplug;

pub use alive::{
    Alive, BODY_QUIET_US, Breath, DAYTIME_STALE_US, Drift, EYES_QUIET_US, HAND_FRESH_US, LEAN,
    LEAN_IN_US, LEAN_OUT_US, LOOK_UP, LOOK_UP_FACE_US, LOOK_UP_FOR_US, LOOK_UP_IN_US,
    LOOK_UP_OUT_US, LOOK_UP_TOWARD, RING_BREATH_US, Sleep, Wake, WorkLight,
};
pub use catalogue::*;
pub use engine::*;
pub use speaking::Speaking;

/// The proposed planner cadence; this is not a measured motor or acoustic lag.
pub const TICK_US: u64 = 20_000;
/// No expression command may outlive this lease from its issue time.
pub const LEASE_US: u64 = lamp_msgs::EXPRESSION_LEASE_US;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<lamp_msgs::Invalid> for Error {
    fn from(value: lamp_msgs::Invalid) -> Self {
        Self(value.0)
    }
}
