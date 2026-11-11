//! The agents (engines) the lamp's Harness has, for the phone app (#6240;
//! docs/cloud.md, "The lamp's agents"): said in `update/status`'s
//! `engines`, and the app's tap to install a missing one, heard on
//! `update/engine_install` (`lamp_msgs::EngineInstallAsk`). Both only with
//! [`SWITCH`] (`LAMP_APP_ENGINES`) `on` exactly in lamp-update's own unit;
//! off, `engines` is absent and nothing is subscribed to.
//!
//! Hermes is the only engine now ([`HERMES`]), read from its install's
//! record ([`crate::hermes_install::record`]) as [`hermes`] maps it. A tap
//! only marks the install asked ([`Engines::pending`]): the next round's
//! Hermes step then opens [`crate::hermes_schedule::Window::Asked`], past
//! the quiet stretch and the clock windows but never in the first round
//! after lamp-update starts nor while the setup walk is under way, and the
//! one-shot runs as it always does, with its memory and disk floors, its
//! freeze while someone speaks and its stop when the privacy reading is
//! lost.
use crate::hermes_install::{self, HermesInstall};
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::{EngineInstall, EngineInstallAsk};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// The switch, in lamp-update's own unit: `on` exactly, any other value or
/// none off.
pub const SWITCH: &str = "LAMP_APP_ENGINES";
/// Hermes's name among the engines, as the app reads it.
pub const HERMES: &str = "hermes";
/// The errors of Hermes's record the app is told by name; any other is
/// `failed`.
pub const SAID_REASONS: [&str; 5] = [
    "no_memory",
    "no_space",
    "no_userns",
    "interrupted",
    "privacy_lost",
];

/// [`SWITCH`]'s value: `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Hermes as the app is told it: `off` with Hermes off on this lamp (its
/// switch off, or Harness's); with its record (none is taken as `absent`),
/// `absent` is `not_installed`, `installing` is `installing`, or `frozen`
/// while the install's watch holds a freeze, `installed` is `installed`
/// with its version, and `failed` is `failed` with its error when the app
/// has a name for it ([`SAID_REASONS`]), else `failed`. A tap the next
/// round has not taken up yet (`pending`) makes a missing or failed one
/// `waiting`. `at_unix_ms` is the record's.
pub fn hermes(
    on: bool,
    record: Option<&HermesInstall>,
    frozen: bool,
    pending: bool,
) -> EngineInstall {
    let engine = |state: &str| EngineInstall {
        state: state.to_owned(),
        version: None,
        reason: None,
        at_unix_ms: record.map(|record| record.at_unix_ms),
    };
    if !on {
        return EngineInstall {
            at_unix_ms: None,
            ..engine("off")
        };
    }
    let install = record.map_or("absent", |record| record.install.as_str());
    match install {
        "absent" | "failed" if pending => engine("waiting"),
        "absent" => engine("not_installed"),
        "installing" if frozen => engine("frozen"),
        "installing" => engine("installing"),
        "installed" => EngineInstall {
            version: record.and_then(|record| record.version.clone()),
            ..engine("installed")
        },
        _ => {
            let error = record.and_then(|record| record.error.as_deref());
            EngineInstall {
                reason: Some(
                    error
                        .filter(|error| SAID_REASONS.contains(error))
                        .unwrap_or("failed")
                        .to_owned(),
                ),
                ..engine("failed")
            }
        }
    }
}

/// What `update/status`'s `engines` is read from, shared by the status
/// thread, the round's Hermes step and the app's asks.
#[derive(Clone, Debug, Default)]
pub struct Engines {
    /// Hermes is on for this `serve` (its switch and Harness's).
    pub hermes_on: bool,
    /// Set by the install's watch while it holds the one-shot frozen
    /// ([`crate::hermes_schedule::Watcher::frozen`]).
    pub frozen: Arc<AtomicBool>,
    /// The app asked for Hermes and no round has started its install yet.
    pub pending: Arc<AtomicBool>,
}

impl Engines {
    /// Each engine now, by name.
    pub fn read(&self, records: &Path) -> BTreeMap<String, EngineInstall> {
        BTreeMap::from([(
            HERMES.to_owned(),
            hermes(
                self.hermes_on,
                hermes_install::record(records).as_ref(),
                self.frozen.load(Ordering::Relaxed),
                self.pending(),
            ),
        )])
    }

    /// Whether a tap waits for a round's Hermes step.
    pub fn pending(&self) -> bool {
        self.pending.load(Ordering::Relaxed)
    }

    /// After a round's Hermes step: the tap is taken up once the record
    /// left `absent` and `failed` (the one-shot started, or Hermes found
    /// there). Still missing or failed (its window refused, or Harness not
    /// there yet), it keeps waiting for the next round.
    pub fn settle(&self, records: &Path) {
        if !missing(hermes_install::record(records).as_ref()) {
            self.pending.store(false, Ordering::Relaxed);
        }
    }
}

/// Hermes missing or failed as its record says (none is missing): what a
/// tap may install.
fn missing(record: Option<&HermesInstall>) -> bool {
    record.is_none_or(|record| matches!(record.install.as_str(), "absent" | "failed"))
}

/// `serve`'s side of the app's tap: its subscription to
/// `update/engine_install`, and when a tap was last taken.
pub struct Asks {
    asks: Option<Subscriber>,
    records: PathBuf,
    engines: Engines,
    last: Option<Instant>,
}

impl Asks {
    /// Subscribed on the bus at `root`. A bus or topic that cannot be used
    /// is said once, and no tap is ever heard.
    pub fn new(root: &Path, records: &Path, engines: Engines, say: &mut dyn FnMut(&str)) -> Self {
        let asks = Bus::new(root).and_then(|bus| {
            bus.subscribe_typed(
                EngineInstallAsk::TOPIC,
                crate::world::NAME,
                EngineInstallAsk::TYPE,
            )
        });
        let asks = match asks {
            Ok(asks) => Some(asks),
            Err(error) => {
                say(&format!(
                    "{} cannot be subscribed to: {error}",
                    EngineInstallAsk::TOPIC
                ));
                None
            }
        };
        Self {
            asks,
            records: records.to_owned(),
            engines,
            last: None,
        }
    }

    /// Whether it is subscribed.
    pub fn listens(&self) -> bool {
        self.asks.is_some()
    }

    /// Takes every ask waiting, at `now`: one for Hermes, with Hermes on,
    /// missing or failed, and not within [`EngineInstallAsk::AGAIN_AFTER`]
    /// of the last taken, marks it [`Engines::pending`], and true is
    /// returned (the wait between rounds ends). Any other is dropped with a
    /// line: an engine this lamp has not, Hermes off, or Hermes not
    /// missing.
    pub fn take(&mut self, now: Instant, say: &mut dyn FnMut(&str)) -> bool {
        let Some(asks) = &mut self.asks else {
            return false;
        };
        let mut taken = false;
        while let Ok(Some(message)) = asks.recv(Duration::ZERO) {
            let Ok(ask) = message.parse::<EngineInstallAsk>() else {
                continue;
            };
            if ask.engine != HERMES {
                say("the app asked to install an engine this lamp does not have: ignored");
                continue;
            }
            if !self.engines.hermes_on {
                say("the app asked to install Hermes: ignored, Hermes is off on this lamp");
                continue;
            }
            if !missing(hermes_install::record(&self.records).as_ref()) {
                say("the app asked to install Hermes: ignored, it is not missing or failed");
                continue;
            }
            if self.last.is_some_and(|last| {
                now.saturating_duration_since(last) < EngineInstallAsk::AGAIN_AFTER
            }) {
                continue;
            }
            self.last = Some(now);
            if !self.engines.pending.swap(true, Ordering::Relaxed) {
                say("the app asked to install Hermes: the next round may start it");
            }
            taken = true;
        }
        taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_bus::Publisher;

    fn record(install: &str, error: Option<&str>) -> HermesInstall {
        HermesInstall {
            install: install.to_owned(),
            revision: 1,
            at_unix_ms: 42,
            version: (install == "installed").then(|| "0.22.2".to_owned()),
            commit: None,
            error: error.map(str::to_owned),
        }
    }

    fn said(state: &str, reason: Option<&str>, version: Option<&str>) -> EngineInstall {
        EngineInstall {
            state: state.to_owned(),
            version: version.map(str::to_owned),
            reason: reason.map(str::to_owned),
            at_unix_ms: Some(42),
        }
    }

    /// Each record state, with the switch on and off, frozen or not, a tap
    /// pending or not: the app's state, and only what it could have said.
    #[test]
    fn each_record_state_maps_to_the_apps_state() {
        let absent = record("absent", None);
        let installing = record("installing", None);
        let installed = record("installed", None);
        let failed = record("failed", Some("installer"));
        for (record, frozen, pending, expected) in [
            (
                Some(&absent),
                false,
                false,
                said("not_installed", None, None),
            ),
            (
                Some(&absent),
                true,
                false,
                said("not_installed", None, None),
            ),
            (Some(&absent), false, true, said("waiting", None, None)),
            (Some(&absent), true, true, said("waiting", None, None)),
            (
                Some(&installing),
                false,
                false,
                said("installing", None, None),
            ),
            (
                Some(&installing),
                false,
                true,
                said("installing", None, None),
            ),
            (Some(&installing), true, false, said("frozen", None, None)),
            (Some(&installing), true, true, said("frozen", None, None)),
            (
                Some(&installed),
                false,
                false,
                said("installed", None, Some("0.22.2")),
            ),
            (
                Some(&installed),
                true,
                true,
                said("installed", None, Some("0.22.2")),
            ),
            (
                Some(&failed),
                false,
                false,
                said("failed", Some("failed"), None),
            ),
            (
                Some(&failed),
                true,
                false,
                said("failed", Some("failed"), None),
            ),
            (Some(&failed), false, true, said("waiting", None, None)),
        ] {
            let told = hermes(true, record, frozen, pending);
            assert_eq!(
                told, expected,
                "{record:?} frozen {frozen} pending {pending}"
            );
            assert!(told.check(), "{told:?}");
        }
        // No record yet reads as missing.
        let none = hermes(true, None, false, false);
        assert_eq!(none.state, "not_installed");
        assert_eq!(none.at_unix_ms, None);
        assert_eq!(hermes(true, None, false, true).state, "waiting");
        // Off: `off` whatever the record, the freeze or a tap.
        for record in [
            None,
            Some(&absent),
            Some(&installing),
            Some(&installed),
            Some(&failed),
        ] {
            for frozen in [false, true] {
                for pending in [false, true] {
                    let told = hermes(false, record, frozen, pending);
                    assert_eq!(
                        told,
                        EngineInstall {
                            state: "off".into(),
                            version: None,
                            reason: None,
                            at_unix_ms: None,
                        }
                    );
                    assert!(told.check());
                }
            }
        }
    }

    /// The errors the app has a name for are said as they are; every other
    /// of Hermes's is `failed`.
    #[test]
    fn a_failure_says_its_reason_only_when_the_app_has_a_name_for_it() {
        for error in HermesInstall::ERRORS {
            let told = hermes(true, Some(&record("failed", Some(error))), false, false);
            let expected = if SAID_REASONS.contains(&error) {
                error
            } else {
                "failed"
            };
            assert_eq!(told.reason.as_deref(), Some(expected), "{error}");
            assert!(told.check(), "{told:?}");
        }
        for reason in SAID_REASONS {
            assert!(HermesInstall::ERRORS.contains(&reason), "{reason}");
            assert!(EngineInstall::REASONS.contains(&reason), "{reason}");
        }
    }

    fn write_record(records: &Path, record: &HermesInstall) {
        std::fs::write(
            records.join(hermes_install::RECORD),
            serde_json::to_vec(record).unwrap(),
        )
        .unwrap();
    }

    /// The tap is taken up once the record left `absent` and `failed`.
    #[test]
    fn a_tap_is_settled_once_the_install_started() {
        let folder = tempfile::tempdir().unwrap();
        let engines = Engines {
            hermes_on: true,
            ..Engines::default()
        };
        engines.pending.store(true, Ordering::Relaxed);
        write_record(folder.path(), &record("absent", None));
        engines.settle(folder.path());
        assert!(engines.pending());
        assert_eq!(engines.read(folder.path())["hermes"].state, "waiting");
        write_record(folder.path(), &record("installing", None));
        engines.settle(folder.path());
        assert!(!engines.pending());
        assert_eq!(engines.read(folder.path())["hermes"].state, "installing");
        engines.frozen.store(true, Ordering::Relaxed);
        assert_eq!(engines.read(folder.path())["hermes"].state, "frozen");
    }

    fn cloud(bus: &Bus) -> Publisher {
        let publisher = bus
            .publisher_declared(
                EngineInstallAsk::TOPIC,
                &EngineInstallAsk::declaration(),
                "cloud",
            )
            .unwrap();
        // A publisher finds a new subscriber within 200 ms.
        std::thread::sleep(Duration::from_millis(250));
        publisher
    }

    fn ask(engine: &str) -> EngineInstallAsk {
        EngineInstallAsk {
            engine: engine.to_owned(),
        }
    }

    /// A tap for Hermes, on and missing, marks it pending at most once a
    /// minute; one for an engine the lamp has not, or with Hermes off or
    /// there, is dropped with a line.
    #[test]
    fn a_tap_for_a_missing_hermes_marks_it_pending_and_others_are_dropped() {
        let root = tempfile::tempdir().unwrap();
        let records = tempfile::tempdir().unwrap();
        write_record(records.path(), &record("failed", Some("no_memory")));
        let engines = Engines {
            hermes_on: true,
            ..Engines::default()
        };
        let mut lines = Vec::new();
        let mut asks = Asks::new(root.path(), records.path(), engines.clone(), &mut |line| {
            lines.push(line.to_owned())
        });
        assert!(asks.listens() && lines.is_empty(), "{lines:?}");
        let mut cloud = cloud(&Bus::new(root.path()).unwrap());
        let mut say = |line: &str| lines.push(line.to_owned());
        let now = Instant::now();

        cloud.publish_json(0, &ask("openclaw")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!asks.take(now, &mut say));
        assert!(!engines.pending());

        cloud.publish_json(0, &ask("hermes")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(asks.take(now, &mut say));
        assert!(engines.pending());
        assert_eq!(engines.read(records.path())["hermes"].state, "waiting");

        // Again within the minute: dropped.
        engines.pending.store(false, Ordering::Relaxed);
        cloud.publish_json(0, &ask("hermes")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!asks.take(now + Duration::from_secs(1), &mut say));
        assert!(!engines.pending());

        // Hermes there: dropped.
        write_record(records.path(), &record("installed", None));
        cloud.publish_json(0, &ask("hermes")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!asks.take(now + EngineInstallAsk::AGAIN_AFTER, &mut say));
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].contains("does not have"), "{lines:?}");
        assert!(
            lines[1].contains("the next round may start it"),
            "{lines:?}"
        );
        assert!(lines[2].contains("not missing"), "{lines:?}");

        // Hermes off: dropped.
        let other = tempfile::tempdir().unwrap();
        let mut off = Asks::new(
            other.path(),
            records.path(),
            Engines::default(),
            &mut |_| {},
        );
        let mut cloud = super::tests::cloud(&Bus::new(other.path()).unwrap());
        write_record(records.path(), &record("absent", None));
        cloud.publish_json(0, &ask("hermes")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let mut off_lines = Vec::new();
        assert!(!off.take(now, &mut |line| off_lines.push(line.to_owned())));
        assert!(off_lines[0].contains("Hermes is off"), "{off_lines:?}");
    }
}
