//! What a body let go for its pose asks of a person (#2602, #2938), as its
//! diagnostic carries it, readable on any host: the voice says it aloud
//! (`lamp_voice::body_ask`).
use lamp_msgs::{Diagnostic, Level};

/// The name the body reports under on `diagnostics`.
pub const SOURCE: &str = "body";

/// What it says, no longer asking, once `crate::body::WAIT_ASKING` has
/// passed. Never said aloud.
pub const STOPPED_ASKING: &str = "Nobody has set me straight, so I have stopped asking. I am let go, and I will start by myself once I am set straight.";

/// What a body let go for its pose asks, if `report` is that: the body's
/// own error while it starts or is paused, never one it stopped with.
pub fn asks(report: &Diagnostic) -> Option<&str> {
    let starting = report.values.iter().any(|value| {
        value.key == lamp_keeper::STATE
            && (value.value == lamp_keeper::PartState::Starting.as_str()
                || value.value == lamp_keeper::PartState::Paused.as_str())
    });
    (report.name == SOURCE
        && report.level == Level::Error
        && starting
        && !report.message.is_empty())
    .then_some(report.message.as_str())
}
