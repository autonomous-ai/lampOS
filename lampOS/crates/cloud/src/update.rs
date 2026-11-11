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
//!
//! The lamp's agents (#6240, `engines`), with `LAMP_APP_ENGINES=on` here
//! too ([`ENGINES_ENVIRONMENT`]): `update.status` gives lamp-update's
//! `engines` as it said them, and `engines.install` (`{"engine":
//! "hermes"}`) asks `lamp-update serve` to install a missing or failed one
//! at its next round: one `update/engine_install` on the bus
//! (`lamp_msgs::EngineInstallAsk`), at most one a minute. The install keeps
//! its own rules: never in lamp-update's first round nor in the setup walk,
//! its memory and disk floors, and its freeze while someone speaks.
use lamp_bus::{Bus, Publisher};
use lamp_msgs::{EngineInstallAsk, UpdateCheck, UpdateStatus};
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

/// The lamp's agents' switch (#6240), in lamp-cloud's unit: `on`
/// exactly, and only with [`ENVIRONMENT`] on as well.
pub const ENGINES_ENVIRONMENT: &str = "LAMP_APP_ENGINES";

/// `engines.install`, refused: lamp-update says no such agent (an older
/// lamp-update, its switch off, or a name it does not have).
pub const NO_ENGINE: &str = "no such agent on this lamp";
/// `engines.install`, refused: the agent is not missing or failed (there,
/// being installed, waiting for a round already, or off).
pub const NOT_MISSING: &str = "the agent is not missing or failed";

pub fn from_environment() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// [`ENGINES_ENVIRONMENT`] `on` exactly.
pub fn engines_from_environment() -> bool {
    std::env::var(ENGINES_ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// `update.status`: `{"running": {"sequence", "commit"} | null, "now":
/// "idle" | "checking" | "fetching" | "switching" | "watching" | "unknown",
/// "last": {"outcome", "sequence", "at_unix_ms", "reason"?,
/// "started_at_unix_ms"?} | null, "stale": flag}`, `last` as lamp-update said
/// it (`lamp_msgs::UpdateResult`).
/// Before lamp-update has said anything, `now` is `unknown` and the rest
/// null; a value lamp-update could not have said is refused. With
/// `harness` said (#5197), it is given as `lamp_msgs::HarnessInstall`.
/// While a round runs, `round` (#5473) gives `{"started_at_unix_ms",
/// "step_started_at_unix_ms", "target"?: {"sequence", "commit"}}`
/// (`lamp_msgs::UpdateRound`) as lamp-update said it, for the app's
/// Software update screen: absent when idle, and left out, not the page
/// refused, when its step began before its round. With `engines`
/// (`LAMP_APP_ENGINES=on`, #6240) and lamp-update saying them, `engines`
/// gives each agent by name as `lamp_msgs::EngineInstall` (`{"state",
/// "version"?, "reason"?, "at_unix_ms"?}`); one lamp-update could not
/// have said is left out.
pub fn status(kept: Option<lamp_bus::Message>, engines: bool) -> Result<Option<Value>, String> {
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
    let mut reply = json!({
        "running": status.running,
        "now": status.now,
        "last": status.last,
        "stale": message.expired,
    });
    // Harness's install (#5197), only when lamp-update says one (its
    // `LAMP_HARNESS_AT_ONBOARDING=on`): a value it could not have said is
    // left out, not the whole page refused.
    if let Some(harness) = status.harness.filter(lamp_msgs::HarnessInstall::check) {
        reply["harness"] = json!(harness);
    }
    if let Some(round) = status.round.filter(|round| {
        status.now != "idle" && round.step_started_at_unix_ms >= round.started_at_unix_ms
    }) {
        reply["round"] = json!(round);
    }
    // The lamp's agents (#6240), only with this side's switch on too.
    if let Some(said) = status.engines.filter(|_| engines) {
        let checked: serde_json::Map<String, Value> = said
            .into_iter()
            .filter(|(_, engine)| engine.check())
            .map(|(name, engine)| (name, json!(engine)))
            .collect();
        reply["engines"] = Value::Object(checked);
    }
    Ok(Some(reply))
}

/// Whether `engine` may be installed by the app, as `status` (as [`status`]
/// answers it) says: `not_installed` or `failed`. Refused with
/// [`NO_ENGINE`] when it is not said, and [`NOT_MISSING`] otherwise.
pub fn installable(status: &Value, engine: &str) -> Result<(), String> {
    match status["engines"][engine]["state"].as_str() {
        Some("not_installed" | "failed") => Ok(()),
        Some(_) => Err(NOT_MISSING.to_owned()),
        None => Err(NO_ENGINE.to_owned()),
    }
}

/// `engines.install`'s reply.
pub fn install_started() -> Value {
    json!({"install": "started"})
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
    /// [`ENGINES_ENVIRONMENT`] on (#6240).
    engines: bool,
    /// When the app's last install ask was taken, and its publisher on
    /// `update/engine_install`, made at first use.
    install_last: Option<Instant>,
    install_publisher: Option<Publisher>,
}

impl Checks {
    /// Checks with the lamp's agents' switch (#6240) as given.
    pub fn with_engines(engines: bool) -> Self {
        Self {
            engines,
            ..Self::default()
        }
    }

    /// Whether the lamp's agents' switch is on.
    pub fn engines(&self) -> bool {
        self.engines
    }

    /// Puts one install ask for `engine` on `bus` at `now` for lamp-update
    /// (#6240). Refused with [`TOO_SOON`] within
    /// [`EngineInstallAsk::AGAIN_AFTER`] of the last one taken, and with
    /// [`NOT_LISTENING`] when no running lamp-update reads it; that one
    /// does not count as asked.
    pub fn ask_install(
        &mut self,
        bus: Option<&Bus>,
        engine: &str,
        now: Instant,
    ) -> Result<(), String> {
        if self
            .install_last
            .is_some_and(|last| now.saturating_duration_since(last) < EngineInstallAsk::AGAIN_AFTER)
        {
            return Err(TOO_SOON.to_owned());
        }
        let bus = bus.ok_or(NOT_LISTENING)?;
        let publisher = match &mut self.install_publisher {
            Some(publisher) => publisher,
            empty => empty.insert(
                bus.publisher_declared(
                    EngineInstallAsk::TOPIC,
                    &EngineInstallAsk::declaration(),
                    crate::lamp::SOURCE,
                )
                .map_err(|_| NOT_LISTENING.to_owned())?,
            ),
        };
        let ask = EngineInstallAsk {
            engine: engine.to_owned(),
        };
        let published = publisher
            .publish_json(lamp_ipc::monotonic_us(), &ask)
            .map_err(|_| NOT_LISTENING.to_owned())?;
        if published.delivered == 0 {
            return Err(NOT_LISTENING.to_owned());
        }
        self.install_last = Some(now);
        Ok(())
    }

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

    fn kept(bus: &Bus) -> Option<lamp_bus::Message> {
        bus.latest(UpdateStatus::TOPIC).unwrap()
    }

    /// #6240: `engines` is given only with this side's switch on and
    /// lamp-update saying them; one it could not have said is left out.
    #[test]
    fn the_engines_are_given_only_with_the_switch_and_checked() {
        let root = tempfile::tempdir().unwrap();
        let bus = Bus::new(root.path()).unwrap();
        let mut says = bus
            .publisher_declared(UpdateStatus::TOPIC, &UpdateStatus::declaration(), "update")
            .unwrap();
        let engine = |state: &str, reason: Option<&str>| lamp_msgs::EngineInstall {
            state: state.into(),
            version: None,
            reason: reason.map(str::to_owned),
            at_unix_ms: Some(5),
        };
        let idle = UpdateStatus {
            running: None,
            now: "idle".into(),
            last: None,
            harness: None,
            round: None,
            engines: Some(std::collections::BTreeMap::from([
                ("hermes".into(), engine("failed", Some("no_memory"))),
                ("other".into(), engine("failed", Some("/var/lib/x"))),
            ])),
        };
        says.publish_json(0, &idle).unwrap();
        let off = status(kept(&bus), false).unwrap().unwrap();
        assert!(off.get("engines").is_none(), "{off}");
        let on = status(kept(&bus), true).unwrap().unwrap();
        assert_eq!(
            on["engines"],
            json!({"hermes": {"state": "failed", "reason": "no_memory", "at_unix_ms": 5}}),
            "{on}"
        );
        assert_eq!(installable(&on, "hermes"), Ok(()));
        assert_eq!(installable(&on, "other"), Err(NO_ENGINE.to_owned()));
        assert_eq!(installable(&off, "hermes"), Err(NO_ENGINE.to_owned()));
        for state in ["off", "waiting", "installing", "frozen", "installed"] {
            let told = json!({"engines": {"hermes": {"state": state}}});
            assert_eq!(installable(&told, "hermes"), Err(NOT_MISSING.to_owned()));
        }
        let missing = json!({"engines": {"hermes": {"state": "not_installed"}}});
        assert_eq!(installable(&missing, "hermes"), Ok(()));
        // None said: none given, with the switch on.
        says.publish_json(
            0,
            &UpdateStatus {
                engines: None,
                ..idle
            },
        )
        .unwrap();
        assert!(
            status(kept(&bus), true)
                .unwrap()
                .unwrap()
                .get("engines")
                .is_none()
        );
        assert_eq!(install_started(), json!({"install": "started"}));
    }

    /// #6240: one install ask a minute reaches lamp-update, with the engine
    /// named; a sooner one is refused, and one nobody reads is not counted.
    #[test]
    fn an_install_ask_reaches_lamp_update_at_most_once_a_minute() {
        let root = tempfile::tempdir().unwrap();
        let bus = Bus::new(root.path()).unwrap();
        let mut checks = Checks::with_engines(true);
        assert!(checks.engines() && !Checks::default().engines());
        let now = Instant::now();
        assert_eq!(
            checks.ask_install(None, "hermes", now),
            Err(NOT_LISTENING.to_owned())
        );
        assert_eq!(
            checks.ask_install(Some(&bus), "hermes", now),
            Err(NOT_LISTENING.to_owned())
        );
        let mut update = bus
            .subscribe_typed(EngineInstallAsk::TOPIC, "update", EngineInstallAsk::TYPE)
            .unwrap();
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(checks.ask_install(Some(&bus), "hermes", now), Ok(()));
        let heard = update.recv(Duration::from_secs(1)).unwrap().unwrap();
        assert_eq!(heard.parse::<EngineInstallAsk>().unwrap().engine, "hermes");
        let soon = now + EngineInstallAsk::AGAIN_AFTER - Duration::from_millis(1);
        assert_eq!(
            checks.ask_install(Some(&bus), "hermes", soon),
            Err(TOO_SOON.to_owned())
        );
        // The update check keeps its own minute.
        assert!(checks.last.is_none());
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
