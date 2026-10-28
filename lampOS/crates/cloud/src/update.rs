//! The lamp's software update for the app's Lamp 2 page (#2194, Mobile on
//! #1396), off unless `LAMP_APP_UPDATE=on`.
//!
//! lamp-update says it on `update/status` (`lamp_msgs::UpdateStatus`): the
//! build the lamp runs, what its round is doing now, and how the last one
//! ended, by the outcome's name only. lamp-cloud never reads lamp-update's
//! records, which are root's alone; it answers `update.status` from the
//! bus's kept value.
//!
//! The app's Check for update (#4153, `update.check`) asks `lamp-update
//! serve` for its next round now: one `update/check` on the bus
//! (`lamp_msgs::UpdateCheck`), read between rounds. lamp-cloud only asks; the
//! round keeps its own lock and quiet moment, so a tap never installs in
//! the middle of a conversation.
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{UpdateCheck, UpdateStatus};
use serde_json::{Value, json};
use std::time::Instant;

/// The switch, in lamp-cloud's unit: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_APP_UPDATE";

/// `update.check`, refused: lamp-update's round is under way.
pub const RUNNING: &str = "a round is running";
/// `update.check`, refused: one was taken within the last minute.
pub const TOO_SOON: &str = "asked less than a minute ago";
/// `update.check`, refused: no running lamp-update reads checks (it is
/// stopped, older than #4153, or a `serve` that does not act).
pub const NOT_LISTENING: &str = "lamp-update is not listening for a check";

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

/// Refused with [`RUNNING`] while `status` (as [`status`] answers it) says a
/// round is under way: anything but `idle` or `unknown`. A stale value
/// counts as it reads; lamp-update's own lock keeps two rounds apart anyway.
pub fn idle(status: &Value) -> Result<(), String> {
    match status["now"].as_str() {
        Some("idle" | "unknown") => Ok(()),
        _ => Err(RUNNING.to_owned()),
    }
}

/// `update.check`'s reply: `status` with `"check": "started"`.
pub fn started(mut status: Value) -> Value {
    status["check"] = json!("started");
    status
}

/// lamp-cloud's side of `update.check`: when it last asked, and its
/// publisher on `update/check`, made at first use.
#[derive(Default)]
pub struct Checks {
    last: Option<Instant>,
    publisher: Option<Publisher>,
}

impl Checks {
    /// Puts one check on `bus` at `now` for lamp-update. Refused with
    /// [`TOO_SOON`] within [`UpdateCheck::AGAIN_AFTER`] of the last one
    /// taken, and with [`NOT_LISTENING`] when no running lamp-update reads
    /// it; that one does not count as asked.
    pub fn ask(&mut self, bus: Option<&Bus>, now: Instant) -> Result<(), String> {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < UpdateCheck::AGAIN_AFTER)
        {
            return Err(TOO_SOON.to_owned());
        }
        let bus = bus.ok_or(NOT_LISTENING)?;
        let publisher = match &mut self.publisher {
            Some(publisher) => publisher,
            empty => empty.insert(
                bus.publisher_declared(
                    UpdateCheck::TOPIC,
                    &UpdateCheck::declaration(),
                    crate::lamp::SOURCE,
                )
                .map_err(|_| NOT_LISTENING.to_owned())?,
            ),
        };
        let published = publisher
            .publish_json(lamp_ipc::monotonic_us(), &UpdateCheck {})
            .map_err(|_| NOT_LISTENING.to_owned())?;
        if published.delivered == 0 {
            return Err(NOT_LISTENING.to_owned());
        }
        self.last = Some(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_check_is_refused_while_a_round_runs_and_taken_between_rounds() {
        for now in ["idle", "unknown"] {
            assert_eq!(idle(&json!({"now": now})), Ok(()), "{now}");
        }
        for now in ["checking", "fetching", "switching", "watching", "restoring"] {
            assert_eq!(idle(&json!({"now": now})), Err(RUNNING.to_owned()));
        }
        assert_eq!(
            started(json!({"now": "idle", "running": null})),
            json!({"now": "idle", "running": null, "check": "started"})
        );
    }

    /// One check a minute reaches lamp-update; a sooner ask is refused and
    /// sends nothing; an ask nobody reads is refused and not counted.
    #[test]
    fn a_check_reaches_lamp_update_at_most_once_a_minute() {
        let root = tempfile::tempdir().unwrap();
        let bus = Bus::new(root.path()).unwrap();
        let mut checks = Checks::default();
        let now = Instant::now();
        assert_eq!(checks.ask(None, now), Err(NOT_LISTENING.to_owned()));
        assert_eq!(checks.ask(Some(&bus), now), Err(NOT_LISTENING.to_owned()));
        let mut update = bus
            .subscribe_typed(UpdateCheck::TOPIC, "update", UpdateCheck::TYPE)
            .unwrap();
        // A publisher already made finds a new subscriber at its next look.
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(checks.ask(Some(&bus), now), Ok(()));
        let heard = update.recv(Duration::from_secs(1)).unwrap().unwrap();
        assert_eq!(heard.parse::<UpdateCheck>().unwrap(), UpdateCheck {});
        let soon = now + UpdateCheck::AGAIN_AFTER - Duration::from_millis(1);
        assert_eq!(checks.ask(Some(&bus), soon), Err(TOO_SOON.to_owned()));
        assert!(update.recv(Duration::from_millis(100)).unwrap().is_none());
        assert_eq!(
            checks.ask(Some(&bus), now + UpdateCheck::AGAIN_AFTER),
            Ok(())
        );
        assert!(update.recv(Duration::from_secs(1)).unwrap().is_some());
    }
}
