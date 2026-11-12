//! The agents (engines) the lamp's Harness has, for the phone app (#6240;
//! docs/cloud.md, "The lamp's agents"): said in `update/status`'s
//! `engines`, and the app's tap to install a missing one, heard on
//! `update/engine_install` (`lamp_msgs::EngineInstallAsk`). Both only with
//! [`SWITCH`] (`LAMP_APP_ENGINES`) `on` exactly in lamp-update's own unit;
//! off, `engines` is absent and nothing is subscribed to.
//!
//! Hermes ([`HERMES`]) is read from its install's
//! record ([`crate::hermes_install::record`]) as [`hermes`] maps it. A tap
//! only marks the install asked ([`Engines::pending`]): the next round's
//! Hermes step then opens [`crate::hermes_schedule::Window::Asked`], which
//! a recent failure does not hold (a round reached installs a missing
//! Hermes anyway, #5978), never while the setup walk is under way, and the
//! one-shot runs as it always does, with its memory and disk floors, its
//! freeze while someone speaks and its stop when the privacy reading is
//! lost.
//!
//! OpenClaw (#6240, beside Hermes; V2's ask of 12 Nov, its installer #6226
//! landed) is said and installed as Hermes is: always in `engines`
//! ([`openclaw`]), from its own install's record ([`OPENCLAW_RECORD`] in
//! the same records folder, none is `absent`), `off` with OpenClaw off on
//! this lamp (its switch, or Harness's). A tap marks it asked
//! ([`Engines::openclaw_pending`]): the next round's OpenClaw step opens
//! [`crate::hermes_schedule::Window::Asked`], which a recent failure does
//! not hold, takes its turn after Hermes's in Harness's one-shot, and runs
//! under the same watch, with the same freeze, stop and floors.
//!
//! Every other engine is Harness's own (the lead's rule of 12 Nov, V2:
//! "Harness has more than 10 agents", so nothing here is per agent): a
//! thread asks the lamp's Harness `harness machines engines <this machine>
//! --json` once a [`HARNESS_LOOK_EVERY`] ([`Engines::look_at_harness_every`]),
//! and each engine it says of the lamp's own machine is said beside Hermes
//! and OpenClaw, `installed` (with `version` when Harness gives one) or
//! `not_installed`. A look that fails keeps the last good one. Only Hermes
//! and OpenClaw can be installed from the app, as only they have an
//! installer in this build ([`INSTALLABLE`]).
use crate::hermes_install::{self, HermesInstall};
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::{EngineInstall, EngineInstallAsk};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// The engines this build's lamp-update can install when the app asks: the
/// only ones with an installer here (`hermes_install`, `openclaw_install`).
/// What the app is shown comes from Harness; this is only what a tap may
/// start. lamp-cloud's `update::INSTALLABLE` is the same list.
pub const INSTALLABLE: [&str; 2] = [HERMES, OPENCLAW];

/// How often lamp-update asks the lamp's Harness for its engines (#6240):
/// every five minutes, as lamp-tasks looks at the machines
/// (`MACHINES_EVERY_LOOKS`). Each look has Harness probe every engine.
pub const HARNESS_LOOK_EVERY: Duration = Duration::from_secs(300);
/// The longest `harness machines --json` may take, to find the lamp's own
/// machine: a cold start under `runuser` on a busy lamp.
pub const MACHINES_WITHIN: Duration = Duration::from_secs(30);
/// The longest `harness machines engines <machine> --json` may take:
/// lamp-tasks' `ENGINES_LOOK_TIMEOUT` (Harness's own probe waits 30 s).
pub const ENGINES_WITHIN: Duration = Duration::from_secs(35);
/// The most engines one look may give (lamp-tasks' `MOST_ENGINE_ROWS`).
pub const MOST_HARNESS_ENGINES: usize = 64;

/// One Harness CLI call: its arguments (after `harness`) and its bound, to
/// its stdout or why not. The real one is [`harness_run`]; a test's is a
/// fake Harness.
pub type HarnessRun<'a> = dyn FnMut(&[&str], Duration) -> Result<String, String> + 'a;

/// What the last good look at Harness said: the lamp's own machine id, and
/// each engine Harness says it has, as the app is told it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HarnessLook {
    pub machine: Option<String>,
    pub engines: BTreeMap<String, EngineInstall>,
}

/// The Harness CLI as lamp-update runs it for its other Harness calls:
/// [`crate::install::HARNESS`] as [`crate::install::USER`], `HOME`
/// [`crate::install::HARNESS_HOME`], bounded, its group killed past it.
pub fn harness_run() -> impl FnMut(&[&str], Duration) -> Result<String, String> + Send {
    use crate::install::{self, HARNESS_HOME, USER};
    let mut uid: Option<String> = None;
    move |args, within| {
        let uid = match &uid {
            Some(uid) => uid.clone(),
            None => {
                let mut run = install::output_of;
                let found = install::uid_of(&mut run, USER)?;
                uid = Some(found.clone());
                found
            }
        };
        install::output_of_within(
            &install::harness_argv(USER, HARNESS_HOME, &uid, args),
            within,
        )
    }
}

/// A machine id or engine name as Harness gives one: plain, short.
fn plain(word: &str, max: usize) -> bool {
    !word.is_empty()
        && word.len() <= max
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The lamp's own machine in `harness machines --json` (one object a line):
/// the one `current`, whose id is plain.
pub fn lamp_machine(said: &str) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Machine {
        machine_id: String,
        #[serde(default)]
        current: bool,
    }
    said.lines()
        .filter_map(|line| serde_json::from_str::<Machine>(line).ok())
        .find(|machine| machine.current)
        .map(|machine| machine.machine_id.to_lowercase())
        .filter(|id| plain(id, 64))
}

/// What `harness machines engines <machine> --json` says of `machine` (one
/// object a line, `{machineId, engine, installed, installable, …}`, as
/// lamp-tasks reads it), as the app is told it: `installed: true` is
/// `installed` (with `version` when Harness gives a plain one), `installed:
/// false`, or `null` with `installable: true` (Harness's "not found
/// there"), `not_installed`. Any other is unknown: kept as `before` said
/// it, else left out. Rows of another machine are passed over. A line that
/// is not a row, an engine twice, more than [`MOST_HARNESS_ENGINES`], or no
/// row of `machine` is no answer.
pub fn harness_engines(
    said: &str,
    machine: &str,
    before: &BTreeMap<String, EngineInstall>,
) -> Result<BTreeMap<String, EngineInstall>, &'static str> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        machine_id: String,
        engine: String,
        #[serde(default)]
        installed: Option<bool>,
        #[serde(default)]
        installable: Option<bool>,
        #[serde(default)]
        version: Option<serde_json::Value>,
    }
    let invalid = "engines_invalid_reply";
    let mut rows = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    let mut engines = BTreeMap::new();
    for line in said.lines().filter(|line| !line.trim().is_empty()) {
        rows += 1;
        if rows > MOST_HARNESS_ENGINES {
            return Err(invalid);
        }
        let row: Row = serde_json::from_str(line).map_err(|_| invalid)?;
        if !row.machine_id.eq_ignore_ascii_case(machine) {
            continue;
        }
        if !plain(&row.engine, 32) || !seen.insert(row.engine.clone()) {
            return Err(invalid);
        }
        let state = match (row.installed, row.installable) {
            (Some(true), _) => {
                let installed = EngineInstall {
                    state: "installed".to_owned(),
                    version: row
                        .version
                        .as_ref()
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    reason: None,
                    at_unix_ms: None,
                };
                Some(if installed.check() {
                    installed
                } else {
                    EngineInstall {
                        version: None,
                        ..installed
                    }
                })
            }
            (Some(false), _) | (None, Some(true)) => Some(EngineInstall {
                state: "not_installed".to_owned(),
                version: None,
                reason: None,
                at_unix_ms: None,
            }),
            _ => before.get(&row.engine).cloned(),
        };
        if let Some(state) = state {
            engines.insert(row.engine, state);
        }
    }
    if seen.is_empty() {
        return Err("engines_none");
    }
    Ok(engines)
}

/// One look at the lamp's Harness: the lamp's own machine (found once, with
/// `harness machines --json`, and again after a look that failed), then its
/// engines. A failure keeps `kept` as it was, and is said back.
pub fn look_at_harness(kept: &Mutex<HarnessLook>, run: &mut HarnessRun) -> Result<(), String> {
    let (machine, before) = {
        let kept = kept.lock().unwrap_or_else(|e| e.into_inner());
        (kept.machine.clone(), kept.engines.clone())
    };
    let machine = match machine {
        Some(machine) => machine,
        None => lamp_machine(&run(&["machines", "--json"], MACHINES_WITHIN)?)
            .ok_or("Harness names no machine of its own (signed out?)")?,
    };
    let looked = run(&["machines", "engines", &machine, "--json"], ENGINES_WITHIN)
        .and_then(|said| harness_engines(&said, &machine, &before).map_err(str::to_owned));
    let mut kept = kept.lock().unwrap_or_else(|e| e.into_inner());
    match looked {
        Ok(engines) => {
            *kept = HarnessLook {
                machine: Some(machine),
                engines,
            };
            Ok(())
        }
        Err(why) => {
            // Found again next time: the lamp may have been signed in anew.
            kept.machine = None;
            Err(why)
        }
    }
}

/// The switch, read by `switches::update_switch`: `on` exactly, any other
/// value off; unset, [`PRODUCT_DEFAULT`] on a product lamp (#6607: a line
/// in lamp-update's own unit never reaches a lamp in a home).
pub const SWITCH: &str = "LAMP_APP_ENGINES";
/// Whether a lamp installed as the product tells the app its agents with
/// [`SWITCH`] unset: on (#6240, #6607); `off` turns it off.
pub const PRODUCT_DEFAULT: bool = true;
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

/// OpenClaw's name among the engines, as the app reads it.
pub const OPENCLAW: &str = "openclaw";
/// OpenClaw's install record, in lamp-update's records, next to Hermes's
/// [`hermes_install::RECORD`]. Written by OpenClaw's installer, not by this
/// build.
pub const OPENCLAW_RECORD: &str = "openclaw-install.json";
/// OpenClaw's switch, in lamp-update's own unit: on by the product's
/// default, `off` turns it off.
pub const OPENCLAW_SWITCH: &str = "LAMP_OPENCLAW_ON_LAMP";
/// Why OpenClaw's last try failed, as its record may say. Only those the
/// app also has a name for ([`SAID_REASONS`]) are said as they are; every
/// other is `failed`, so the app's list stays as it is.
pub const OPENCLAW_ERRORS: [&str; 13] = [
    "no_userns",
    "serve",
    "timeout",
    "fetch",
    "installer",
    "version",
    "old_node",
    "config",
    "entry",
    "gateway",
    "not_started",
    "privacy_lost",
    "interrupted",
];

/// Where OpenClaw's install is, as [`OPENCLAW_RECORD`] keeps it: Hermes's
/// shape, with `node` the Node pin beside `version` the OpenClaw pin, both
/// with `installed` only.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct OpenClawInstall {
    pub install: String,
    #[serde(default)]
    pub revision: u64,
    pub at_unix_ms: u64,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl OpenClawInstall {
    /// Whether every field is one its installer could have written: a
    /// state of [`HermesInstall::INSTALL`], an `error` only with `failed`
    /// (any code: one this build does not know is said as `failed`), and
    /// `version` and `node` plain, with `installed` only.
    pub fn check(&self) -> bool {
        let plain = |text: &Option<String>| {
            text.as_ref().is_none_or(|text| {
                self.install == "installed"
                    && !text.is_empty()
                    && text.len() <= 64
                    && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.')
            })
        };
        HermesInstall::INSTALL.contains(&self.install.as_str())
            && self.error.as_ref().is_none_or(|_| self.install == "failed")
            && plain(&self.version)
            && plain(&self.node)
    }
}

/// OpenClaw's record under `records`: none when the file is not there; a
/// file there that does not read, or holds what its installer could not
/// have written, is taken as a failed install.
pub fn openclaw_record(records: &Path) -> Option<OpenClawInstall> {
    let bytes = std::fs::read(records.join(OPENCLAW_RECORD)).ok()?;
    Some(
        serde_json::from_slice::<OpenClawInstall>(&bytes)
            .ok()
            .filter(OpenClawInstall::check)
            .unwrap_or(OpenClawInstall {
                install: "failed".to_owned(),
                revision: 0,
                at_unix_ms: 0,
                version: None,
                node: None,
                error: None,
            }),
    )
}

/// OpenClaw as the app is told it, as Hermes is ([`hermes`]): `off` with
/// OpenClaw off on this lamp; with its record (none is taken as `absent`),
/// `not_installed`, `installing` or `frozen` while the install's watch
/// holds a freeze, `installed` with its version, `failed` with its error
/// when the app has a name for it, else `failed`, and `waiting` for a
/// missing or failed one a tap asked for. `at_unix_ms` is the record's
/// (none for a record that did not read).
pub fn openclaw(
    on: bool,
    record: Option<&OpenClawInstall>,
    frozen: bool,
    pending: bool,
) -> EngineInstall {
    let engine = |state: &str| EngineInstall {
        state: state.to_owned(),
        version: None,
        reason: None,
        at_unix_ms: record.map(|record| record.at_unix_ms).filter(|at| *at > 0),
    };
    if !on {
        return EngineInstall {
            at_unix_ms: None,
            ..engine("off")
        };
    }
    match record.map_or("absent", |record| record.install.as_str()) {
        "absent" | "failed" if pending => engine("waiting"),
        "absent" => engine("not_installed"),
        "installing" if frozen => engine("frozen"),
        "installing" => engine("installing"),
        "installed" => EngineInstall {
            version: record.and_then(|record| record.version.clone()),
            ..engine("installed")
        },
        _ => EngineInstall {
            reason: Some(
                record
                    .and_then(|record| record.error.as_deref())
                    .filter(|error| SAID_REASONS.contains(error))
                    .unwrap_or("failed")
                    .to_owned(),
            ),
            ..engine("failed")
        },
    }
}

/// [`OPENCLAW_SWITCH`]'s value: anything but `off` is on.
pub fn openclaw_on(value: Option<&str>) -> bool {
    value != Some("off")
}

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
    /// OpenClaw is on for this `serve` (its switch and Harness's).
    pub openclaw_on: bool,
    /// The app asked for OpenClaw and no round has started its install yet.
    pub openclaw_pending: Arc<AtomicBool>,
    /// What the lamp's Harness last said of its engines
    /// ([`Engines::look_at_harness_every`]): none until a look is good.
    pub harness: Arc<Mutex<HarnessLook>>,
}

impl Engines {
    /// Each engine now, by name: Hermes and OpenClaw from their records,
    /// then every other engine the lamp's Harness last said it has.
    pub fn read(&self, records: &Path) -> BTreeMap<String, EngineInstall> {
        let mut engines = BTreeMap::from([(
            HERMES.to_owned(),
            hermes(
                self.hermes_on,
                hermes_install::record(records).as_ref(),
                self.frozen.load(Ordering::Relaxed),
                self.pending(),
            ),
        )]);
        engines.insert(
            OPENCLAW.to_owned(),
            openclaw(
                self.openclaw_on,
                openclaw_record(records).as_ref(),
                self.frozen.load(Ordering::Relaxed),
                self.openclaw_pending(),
            ),
        );
        let harness = self.harness.lock().unwrap_or_else(|e| e.into_inner());
        for (name, engine) in &harness.engines {
            engines
                .entry(name.clone())
                .or_insert_with(|| engine.clone());
        }
        engines
    }

    /// Asks the lamp's Harness for its engines now and once every `every`
    /// after, on a thread of its own, so the status's 2 s look never waits
    /// on Harness ([`look_at_harness`], each call bounded). A look that
    /// fails is said once, and the last good one kept; a good one after it
    /// is said once too. The thread ends with the last [`Engines`].
    pub fn look_at_harness_every(
        &self,
        every: Duration,
        mut run: impl FnMut(&[&str], Duration) -> Result<String, String> + Send + 'static,
        mut say: impl FnMut(&str) + Send + 'static,
    ) {
        let kept = Arc::downgrade(&self.harness);
        let _ = std::thread::Builder::new()
            .name("update-engines".into())
            .spawn(move || {
                let mut good = None;
                loop {
                    let Some(kept) = kept.upgrade() else {
                        return;
                    };
                    let looked = look_at_harness(&kept, &mut run);
                    drop(kept);
                    match looked {
                        Ok(()) if good != Some(true) => {
                            say("the lamp's Harness said its engines");
                            good = Some(true);
                        }
                        Err(why) if good != Some(false) => {
                            say(&format!(
                                "the lamp's Harness did not say its engines, the last kept: {why}"
                            ));
                            good = Some(false);
                        }
                        _ => {}
                    }
                    std::thread::sleep(every);
                }
            });
    }

    /// Whether a tap waits for a round's OpenClaw step.
    pub fn openclaw_pending(&self) -> bool {
        self.openclaw_pending.load(Ordering::Relaxed)
    }

    /// After a round's OpenClaw step, as [`Engines::settle`] for Hermes.
    pub fn settle_openclaw(&self, records: &Path) {
        if !openclaw_missing(openclaw_record(records).as_ref()) {
            self.openclaw_pending.store(false, Ordering::Relaxed);
        }
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

/// OpenClaw missing or failed as its record says (none is missing).
fn openclaw_missing(record: Option<&OpenClawInstall>) -> bool {
    record.is_none_or(|record| matches!(record.install.as_str(), "absent" | "failed"))
}

/// `serve`'s side of the app's tap: its subscription to
/// `update/engine_install`, and when a tap was last taken.
pub struct Asks {
    asks: Option<Subscriber>,
    records: PathBuf,
    engines: Engines,
    last: Option<Instant>,
    last_openclaw: Option<Instant>,
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
            last_openclaw: None,
        }
    }

    /// Whether it is subscribed.
    pub fn listens(&self) -> bool {
        self.asks.is_some()
    }

    /// Takes every ask waiting, at `now`: one for Hermes (or OpenClaw),
    /// with it on, missing or failed, and not within
    /// [`EngineInstallAsk::AGAIN_AFTER`] of that engine's last taken, marks
    /// it [`Engines::pending`] (or [`Engines::openclaw_pending`]), and true
    /// is returned (the wait between rounds ends). Any other is dropped with
    /// a line: an engine this lamp has not, the engine off, or not missing.
    pub fn take(&mut self, now: Instant, say: &mut dyn FnMut(&str)) -> bool {
        let Some(asks) = &mut self.asks else {
            return false;
        };
        let mut taken = false;
        while let Ok(Some(message)) = asks.recv(Duration::ZERO) {
            let Ok(ask) = message.parse::<EngineInstallAsk>() else {
                continue;
            };
            if ask.engine == OPENCLAW {
                if !self.engines.openclaw_on {
                    say("the app asked to install OpenClaw: ignored, OpenClaw is off on this lamp");
                    continue;
                }
                if !openclaw_missing(openclaw_record(&self.records).as_ref()) {
                    say("the app asked to install OpenClaw: ignored, it is not missing or failed");
                    continue;
                }
                if self.last_openclaw.is_some_and(|last| {
                    now.saturating_duration_since(last) < EngineInstallAsk::AGAIN_AFTER
                }) {
                    continue;
                }
                self.last_openclaw = Some(now);
                if !self.engines.openclaw_pending.swap(true, Ordering::Relaxed) {
                    say("the app asked to install OpenClaw: the next round may start it");
                }
                taken = true;
                continue;
            }
            if ask.engine != HERMES {
                say("the app asked to install an engine this build cannot install: ignored");
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

    fn write_openclaw(records: &Path, text: &str) {
        std::fs::write(records.join(OPENCLAW_RECORD), text).unwrap();
    }

    /// OpenClaw is said as Hermes is (#6240, V2's ask of 12 Nov): always,
    /// `not_installed` with no record, `off` with OpenClaw off on this
    /// lamp, `frozen` while the watch holds a freeze, `waiting` for a tap
    /// not taken up yet; an error the app has no name for, or one this build
    /// does not know, is `failed`.
    #[test]
    fn openclaw_is_said_as_hermes_is() {
        let folder = tempfile::tempdir().unwrap();
        let engines = Engines {
            hermes_on: true,
            openclaw_on: true,
            ..Engines::default()
        };
        // No record: said, not installed.
        let told = engines.read(folder.path());
        assert_eq!(told.keys().collect::<Vec<_>>(), ["hermes", "openclaw"]);
        assert_eq!(
            told["openclaw"],
            EngineInstall {
                state: "not_installed".into(),
                version: None,
                reason: None,
                at_unix_ms: None,
            }
        );
        assert!(told["openclaw"].check());
        // A tap not taken up: waiting.
        engines.openclaw_pending.store(true, Ordering::Relaxed);
        assert_eq!(engines.read(folder.path())["openclaw"].state, "waiting");
        engines.openclaw_pending.store(false, Ordering::Relaxed);
        write_openclaw(
            folder.path(),
            r#"{"install":"installed","revision":2,"at_unix_ms":9,"version":"2026.11.2","node":"24.11.0"}"#,
        );
        let told = engines.read(folder.path());
        assert_eq!(
            told["openclaw"],
            EngineInstall {
                state: "installed".into(),
                version: Some("2026.11.2".into()),
                reason: None,
                at_unix_ms: Some(9),
            }
        );
        assert!(told["openclaw"].check());
        // OpenClaw off on this lamp: `off`, whatever the record.
        let off = Engines {
            openclaw_on: false,
            ..engines.clone()
        };
        assert_eq!(off.read(folder.path())["openclaw"].state, "off");
        assert!(off.read(folder.path())["openclaw"].check());
        for (text, frozen, state) in [
            (
                r#"{"install":"absent","revision":1,"at_unix_ms":3}"#,
                false,
                "not_installed",
            ),
            (
                r#"{"install":"installing","revision":1,"at_unix_ms":3}"#,
                false,
                "installing",
            ),
            (
                r#"{"install":"installing","revision":1,"at_unix_ms":3}"#,
                true,
                "frozen",
            ),
        ] {
            write_openclaw(folder.path(), text);
            engines.frozen.store(frozen, Ordering::Relaxed);
            assert_eq!(engines.read(folder.path())["openclaw"].state, state);
        }
        engines.frozen.store(false, Ordering::Relaxed);
        // Failed: a code the app knows is said, one it has no name for and
        // one no build knows are `failed`; a tap makes it `waiting`.
        for (error, reason) in [
            ("no_userns", "no_userns"),
            ("privacy_lost", "privacy_lost"),
            ("old_node", "failed"),
            ("gateway", "failed"),
            ("something_new", "failed"),
        ] {
            write_openclaw(
                folder.path(),
                &format!(r#"{{"install":"failed","revision":4,"at_unix_ms":7,"error":"{error}"}}"#),
            );
            let said = engines.read(folder.path())["openclaw"].clone();
            assert_eq!(
                (said.state.as_str(), said.reason.as_deref()),
                ("failed", Some(reason)),
                "{error}"
            );
            assert!(said.check(), "{said:?}");
        }
        engines.openclaw_pending.store(true, Ordering::Relaxed);
        assert_eq!(engines.read(folder.path())["openclaw"].state, "waiting");
        engines.openclaw_pending.store(false, Ordering::Relaxed);
        for error in OPENCLAW_ERRORS {
            let record = OpenClawInstall {
                install: "failed".into(),
                revision: 1,
                at_unix_ms: 1,
                version: None,
                node: None,
                error: Some(error.into()),
            };
            assert!(
                openclaw(true, Some(&record), false, false).check(),
                "{error}"
            );
        }
        // A file that does not read is a failed install, never a crash.
        write_openclaw(folder.path(), "not json");
        let said = engines.read(folder.path())["openclaw"].clone();
        assert_eq!(
            (said.state.as_str(), said.reason.as_deref(), said.at_unix_ms),
            ("failed", Some("failed"), None)
        );
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

    /// A tap for OpenClaw (#6240, V2's ask of 12 Nov), on and missing or
    /// failed, marks it pending at most once a minute, as Hermes's does,
    /// and the next round's OpenClaw step settles it; one with OpenClaw
    /// there is dropped with a line. Hermes's own tap is left as it was.
    #[test]
    fn a_tap_for_a_missing_openclaw_marks_it_pending() {
        let root = tempfile::tempdir().unwrap();
        let records = tempfile::tempdir().unwrap();
        let engines = Engines {
            hermes_on: true,
            openclaw_on: true,
            ..Engines::default()
        };
        let mut asks = Asks::new(root.path(), records.path(), engines.clone(), &mut |_| {});
        let mut cloud = cloud(&Bus::new(root.path()).unwrap());
        let mut lines = Vec::new();
        let mut say = |line: &str| lines.push(line.to_owned());
        let now = Instant::now();
        // No record: missing.
        cloud.publish_json(0, &ask("openclaw")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(asks.take(now, &mut say));
        assert!(engines.openclaw_pending() && !engines.pending());
        assert_eq!(engines.read(records.path())["openclaw"].state, "waiting");
        // Again within the minute: dropped.
        engines.openclaw_pending.store(false, Ordering::Relaxed);
        cloud.publish_json(0, &ask("openclaw")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!asks.take(now + Duration::from_secs(1), &mut say));
        assert!(!engines.openclaw_pending());
        // Failed, a minute on: taken; the round's step starts it, settled.
        write_openclaw(
            records.path(),
            r#"{"install":"failed","revision":2,"at_unix_ms":5,"error":"no_memory"}"#,
        );
        cloud.publish_json(0, &ask("openclaw")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(asks.take(now + EngineInstallAsk::AGAIN_AFTER, &mut say));
        engines.settle_openclaw(records.path());
        assert!(engines.openclaw_pending(), "still failed: still waiting");
        write_openclaw(
            records.path(),
            r#"{"install":"installing","revision":3,"at_unix_ms":6}"#,
        );
        engines.settle_openclaw(records.path());
        assert!(!engines.openclaw_pending());
        // There: dropped.
        write_openclaw(
            records.path(),
            r#"{"install":"installed","revision":4,"at_unix_ms":7,"version":"2026.11.2","node":"24.11.0"}"#,
        );
        cloud.publish_json(0, &ask("openclaw")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!asks.take(now + 2 * EngineInstallAsk::AGAIN_AFTER, &mut say));
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(
            lines[0].contains("the next round may start it"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("the next round may start it"),
            "{lines:?}"
        );
        assert!(lines[2].contains("not missing"), "{lines:?}");
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

        cloud.publish_json(0, &ask("picoclaw")).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!asks.take(now, &mut say));
        assert!(!engines.pending());
        // OpenClaw off on this lamp: dropped.
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
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(lines[0].contains("cannot install"), "{lines:?}");
        assert!(lines[1].contains("OpenClaw is off"), "{lines:?}");
        assert!(
            lines[2].contains("the next round may start it"),
            "{lines:?}"
        );
        assert!(lines[3].contains("not missing"), "{lines:?}");

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

    const LAMP: &str = "0f3c9a1e2b4d4c6e8a107d5e3b2a1c90";

    /// A fake Harness: `machines --json` names the lamp's own machine (and a
    /// computer), `machines engines <lamp> --json` says `engines`; every
    /// call is kept with its bound.
    fn fake_harness(
        engines: &str,
        calls: Arc<Mutex<Vec<(String, Duration)>>>,
    ) -> impl FnMut(&[&str], Duration) -> Result<String, String> + Send + 'static {
        let engines = engines.to_owned();
        move |args, within| {
            calls.lock().unwrap().push((args.join(" "), within));
            match args {
                ["machines", "--json"] => Ok(format!(
                    "{{\"machineId\":\"aa11\",\"current\":false,\"name\":\"a computer\"}}\n\
                     {{\"machineId\":\"{LAMP}\",\"current\":true}}\n"
                )),
                ["machines", "engines", machine, "--json"] if *machine == LAMP => {
                    Ok(engines.replace("LAMP", LAMP))
                }
                _ => Err("unexpected call".into()),
            }
        }
    }

    const FOUR: &str = r#"{"machineId":"LAMP","engine":"hermes","installed":true,"signedIn":null}
{"machineId":"LAMP","engine":"openclaw","installed":true}
{"machineId":"LAMP","engine":"codex","installed":true,"version":"0.58.0","signedIn":true}
{"machineId":"LAMP","engine":"cursor","installed":true}
{"machineId":"LAMP","engine":"claude","installed":null,"installable":true}
{"machineId":"LAMP","engine":"gemini","installed":null,"installable":false}
{"machineId":"aa11","engine":"opencode","installed":true}
"#;

    fn state(state: &str, version: Option<&str>) -> EngineInstall {
        EngineInstall {
            state: state.to_owned(),
            version: version.map(str::to_owned),
            reason: None,
            at_unix_ms: None,
        }
    }

    /// The lead's rule of 12 Nov (#6240): `engines` lists every engine the
    /// lamp's Harness says its own machine has, with no fixed list; Hermes
    /// and OpenClaw stay read from their own records (their richer states),
    /// whatever Harness says of them; a computer's engines are not the
    /// lamp's; one Harness cannot say yet (`null`, nothing installable) is
    /// left out, one not found there is `not_installed`.
    #[test]
    fn engines_lists_every_engine_harness_says_the_lamp_has() {
        let folder = tempfile::tempdir().unwrap();
        write_record(folder.path(), &record("installing", None));
        let engines = Engines {
            hermes_on: true,
            openclaw_on: true,
            ..Engines::default()
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut run = fake_harness(FOUR, calls.clone());
        look_at_harness(&engines.harness, &mut run).unwrap();
        let told = engines.read(folder.path());
        assert_eq!(
            told.keys().collect::<Vec<_>>(),
            ["claude", "codex", "cursor", "hermes", "openclaw"]
        );
        // Hermes and OpenClaw as their records say, not Harness's `true`.
        assert_eq!(told["hermes"].state, "installing");
        assert_eq!(told["openclaw"].state, "not_installed");
        assert_eq!(told["codex"], state("installed", Some("0.58.0")));
        assert_eq!(told["cursor"], state("installed", None));
        assert_eq!(told["claude"], state("not_installed", None));
        assert!(told.values().all(EngineInstall::check), "{told:?}");
        // Hermes off: still `off`, whatever Harness says.
        let off = Engines {
            hermes_on: false,
            ..engines.clone()
        };
        assert_eq!(off.read(folder.path())["hermes"].state, "off");
        // The machine is found once; the next look asks only its engines.
        look_at_harness(&engines.harness, &mut run).unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .map(|(call, within)| (call.replace(LAMP, "LAMP"), *within))
                .collect::<Vec<_>>(),
            [
                ("machines --json".to_owned(), MACHINES_WITHIN),
                ("machines engines LAMP --json".to_owned(), ENGINES_WITHIN),
                ("machines engines LAMP --json".to_owned(), ENGINES_WITHIN),
            ]
        );
    }

    /// Harness failing, timing out, signed out or saying nothing readable:
    /// Hermes and OpenClaw are said as before, no other engine, no error;
    /// after a good look, a failed one keeps it; an engine Harness cannot
    /// say this time keeps what it last said.
    #[test]
    fn a_harness_that_fails_leaves_hermes_and_openclaw_as_they_are() {
        let folder = tempfile::tempdir().unwrap();
        let engines = Engines {
            hermes_on: true,
            openclaw_on: true,
            ..Engines::default()
        };
        let mut failing = |_: &[&str], within: Duration| -> Result<String, String> {
            Err(format!(
                "harness machines engines: did not end within {} s",
                within.as_secs()
            ))
        };
        assert!(look_at_harness(&engines.harness, &mut failing).is_err());
        let told = engines.read(folder.path());
        assert_eq!(told.keys().collect::<Vec<_>>(), ["hermes", "openclaw"]);
        assert_eq!(told["hermes"].state, "not_installed");
        assert_eq!(told["openclaw"].state, "not_installed");
        for said in [
            "",
            "not json\n",
            "{\"machineId\":\"aa11\",\"engine\":\"codex\",\"installed\":true}\n",
        ] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let mut run = fake_harness(said, calls);
            assert!(
                look_at_harness(&engines.harness, &mut run).is_err(),
                "{said:?}"
            );
            assert_eq!(engines.read(folder.path()).len(), 2, "{said:?}");
        }
        // Signed out: no machine of its own.
        let mut signed_out = |_: &[&str], _: Duration| Ok::<_, String>(String::new());
        assert!(look_at_harness(&engines.harness, &mut signed_out).is_err());
        // A good look, then a failing one: the good one is kept.
        let calls = Arc::new(Mutex::new(Vec::new()));
        look_at_harness(&engines.harness, &mut fake_harness(FOUR, calls.clone())).unwrap();
        assert!(look_at_harness(&engines.harness, &mut failing).is_err());
        assert_eq!(engines.read(folder.path()).len(), 5);
        // A failed look finds the machine again next time.
        look_at_harness(&engines.harness, &mut fake_harness(FOUR, calls.clone())).unwrap();
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(call, _)| call == "machines --json")
                .count(),
            2
        );
        // Codex unknown this time (`null`, nothing installable): kept as
        // last said; cursor gone from Harness's list: gone.
        let unknown = r#"{"machineId":"LAMP","engine":"codex","installed":null}
"#;
        look_at_harness(&engines.harness, &mut fake_harness(unknown, calls)).unwrap();
        let told = engines.read(folder.path());
        assert_eq!(
            told.keys().collect::<Vec<_>>(),
            ["codex", "hermes", "openclaw"]
        );
        assert_eq!(told["codex"], state("installed", Some("0.58.0")));
    }

    /// What Harness says is read strictly: an engine twice, a name that is
    /// not plain, or more rows than [`MOST_HARNESS_ENGINES`] is no answer;
    /// a version that is not plain is left out, the engine kept.
    #[test]
    fn harness_engines_are_read_strictly() {
        let none = BTreeMap::new();
        let row = |engine: &str| {
            format!("{{\"machineId\":\"m1\",\"engine\":\"{engine}\",\"installed\":true}}\n")
        };
        assert!(harness_engines(&(row("codex") + &row("codex")), "m1", &none).is_err());
        assert!(harness_engines(&row("Codex"), "m1", &none).is_err());
        assert!(harness_engines(&row("co dex"), "m1", &none).is_err());
        let many: String = (0..=MOST_HARNESS_ENGINES)
            .map(|n| row(&format!("e{n}")))
            .collect();
        assert!(harness_engines(&many, "m1", &none).is_err());
        let odd =
            r#"{"machineId":"M1","engine":"codex","installed":true,"version":"1.0 beta; rm"}"#;
        assert_eq!(
            harness_engines(odd, "m1", &none).unwrap()["codex"],
            state("installed", None)
        );
        assert_eq!(
            lamp_machine(
                "{\"machineId\":\"a\",\"current\":false}\n{\"machineId\":\"../x\",\"current\":true}\n"
            ),
            None
        );
    }

    /// The thread looks at once, then once every `every`, and ends with
    /// the last [`Engines`].
    #[test]
    fn the_harness_look_runs_on_its_own_thread_and_ends_with_the_engines() {
        let folder = tempfile::tempdir().unwrap();
        let engines = Engines {
            hermes_on: true,
            openclaw_on: true,
            ..Engines::default()
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let lines = Arc::new(Mutex::new(Vec::new()));
        let said = lines.clone();
        engines.look_at_harness_every(
            Duration::from_millis(20),
            fake_harness(FOUR, calls.clone()),
            move |line: &str| said.lock().unwrap().push(line.to_owned()),
        );
        let began = Instant::now();
        while engines.read(folder.path()).len() < 5 && began.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(engines.read(folder.path()).len(), 5);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(lines.lock().unwrap().len(), 1, "said once: {lines:?}");
        drop(engines);
        std::thread::sleep(Duration::from_millis(100));
        let after = calls.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(calls.lock().unwrap().len(), after, "the thread ended");
    }

    /// Only Hermes and OpenClaw have an installer in this build.
    #[test]
    fn only_engines_with_an_installer_are_installable() {
        assert_eq!(INSTALLABLE, ["hermes", "openclaw"]);
    }
}
