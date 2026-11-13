//! What a body let go for its pose asks of a person (#2602, #2938), or a
//! running body that will not stand (#6834), as its diagnostic carries it,
//! readable on any host: the voice says it aloud (`lamp_voice::body_ask`).
use lamp_msgs::{Diagnostic, Level};

/// The name the body reports under on `diagnostics`.
pub const SOURCE: &str = "body";

/// What it says, no longer asking, once `crate::body::WAIT_ASKING` has
/// passed. Never said aloud.
pub const STOPPED_ASKING: &str = "Nobody has set me straight, so I have stopped asking. I am let go, and I will start by myself once I am set straight.";

/// The fact a running body's report carries, whatever its level, when the
/// run will not stand (#6834): what its owner can do, one of the `STAND_`
/// lines. The same on every report of the run, so the voice says it once.
pub const ASKS: &str = "asks";

/// The run will not stand: an arm joint found off the way between folded
/// and stood up (a fold by hand that is not the body's own).
pub const STAND_OFF_WAY: &str = "My arm isn't folded all the way down, so I won't stand up. Please fold it down gently, then unplug me and plug me back in.";
/// The run will not stand: an arm joint's torque off, or a fault flag on it.
pub const STAND_FAULT: &str =
    "Something in my arm isn't right, so I won't stand up. Please unplug me and plug me back in.";
/// The run will not stand: an arm joint already at the heat it stops at.
pub const STAND_WARM: &str =
    "My arm is too warm to stand up. Once it has cooled, please unplug me and plug me back in.";
/// The run will not stand: it was given no home.
pub const STAND_NO_HOME: &str = "I don't know where my resting place is, so I won't stand up. I need someone to set me up again.";

/// What a body asks of a person, if `report` says so: a running body that
/// will not stand, its [`ASKS`] fact; or a body let go for its pose, the
/// body's own error while it starts or is paused. Never one it stopped with.
pub fn asks(report: &Diagnostic) -> Option<&str> {
    if report.name != SOURCE {
        return None;
    }
    if let Some(ask) = report.values.iter().find(|value| value.key == ASKS) {
        return Some(ask.value.as_str());
    }
    let starting = report.values.iter().any(|value| {
        value.key == lamp_keeper::STATE
            && (value.value == lamp_keeper::PartState::Starting.as_str()
                || value.value == lamp_keeper::PartState::Paused.as_str())
    });
    (report.level == Level::Error && starting && !report.message.is_empty())
        .then_some(report.message.as_str())
}
