//! The lamp's software update for the app's Lamp 2 page (#2194, Mobile on
//! #1396), off unless `LAMP_APP_UPDATE=on`.
//!
//! lamp-update says it on `update/status` (`lamp_msgs::UpdateStatus`): the
//! build the lamp runs, what its round is doing now, and how the last one
//! ended, by the outcome's name only. lamp-cloud never reads lamp-update's
//! records, which are root's alone; it answers `update.status` from the
//! bus's kept value.
use lamp_msgs::UpdateStatus;
use serde_json::{Value, json};

/// The switch, in lamp-cloud's unit: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_APP_UPDATE";

pub fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// `update.status`: `{"running": {"sequence", "commit"} | null, "now":
/// "idle" | "checking" | "fetching" | "switching" | "watching" | "unknown",
/// "last": {"outcome", "sequence", "at_unix_ms"} | null, "stale": flag}`.
/// Before lamp-update has said anything, `now` is `unknown` and the rest
/// null; a value lamp-update could not have said is refused.
pub fn status(kept: Option<lamp_bus::Message>) -> Result<Option<Value>, String> {
    let Some(message) = kept else {
        return Ok(Some(json!({
            "running": null,
            "now": "unknown",
            "last": null,
            "stale": false,
        })));
    };
    let status = message
        .parse::<UpdateStatus>()
        .map_err(|_| "lamp-update said something unreadable".to_owned())?;
    if !UpdateStatus::NOW.contains(&status.now.as_str()) {
        return Err("lamp-update said something unreadable".into());
    }
    Ok(Some(json!({
        "running": status.running,
        "now": status.now,
        "last": status.last,
        "stale": message.expired,
    })))
}
