//! What the lamp knows of each of the person's computers (`serve
//! --engines-from-machines`, #5199): whether it is offline, and which
//! engines it has and whether each is signed in, each with when it was
//! read. Learned on the Harness side only, never on a request's path:
//! `harness machines --json` gives each machine's presence, and `harness
//! machines engines <id> --json` (openharness#1888) its engines, asked on a
//! lane of its own. A `prepare` is then refused at once, with a reason the
//! voice can say, when the computer is known offline, or its engine known
//! not there or not signed in. Anything unknown hands over as before.
//!
//! Kept beside the targets in a file of its own, `targets.engines.json`,
//! never as a field of `targets.json`: an older build never reads it, so a
//! rollback keeps its hand-overs (#1505), as with `targets.names.json`.
use crate::harness::{AccountMachine, EngineKind, EngineRow, Target};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Read as _},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

/// The engines a computer's target may be given, in the order one is
/// chosen when the person's own isn't known there ([`Known::chosen`]).
pub const ENGINE_ORDER: [EngineKind; 3] =
    [EngineKind::Claude, EngineKind::Codex, EngineKind::Opencode];

/// How old a reading may be and still refuse or choose anything: three
/// looks at the machines (five minutes each on a lamp). An older one is
/// unknown, so a lamp whose looks stopped never refuses on old news.
pub const STALE_AFTER_MS: u64 = 15 * 60 * 1000;

/// The codes a `prepare` is refused with, early, from what is known.
pub const COMPUTER_OFFLINE: &str = "computer_offline";
pub const ENGINE_MISSING: &str = "engine_missing";
pub const ENGINE_NOT_SIGNED_IN: &str = "engine_not_signed_in";
/// Every early refusal, so the service can look again at once after one.
pub const EARLY_REFUSALS: [&str; 3] = [COMPUTER_OFFLINE, ENGINE_MISSING, ENGINE_NOT_SIGNED_IN];

/// One value as read, and when (Unix ms).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reading<T> {
    pub value: T,
    pub read_ms: u64,
}

/// What is known of one engine on one computer. `None` is unknown.
/// `installable` is the computer's own `!installed && command && recipe`
/// (openharness `engineProbe.ts`), passed through by the CLI, which turns
/// the computer's `installed: false` into `null` (`machineEngines.ts`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineKnown {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed: Option<Reading<bool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installable: Option<Reading<bool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_in: Option<Reading<bool>>,
}

/// What is known of one computer: its presence (`running`, `offline`,
/// `unknown` or `payment_pending`) and its engines, by Harness's name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Computer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Reading<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub engines: BTreeMap<String, EngineKnown>,
    /// The engines in the order the last look gave them (Harness's order,
    /// #6492): the lamp's own engines are offered in it after the default.
    /// Kept in memory only, never in the engines file.
    #[serde(skip)]
    pub order: Vec<String>,
}

impl EngineKnown {
    fn is_empty(&self) -> bool {
        self.installed.is_none() && self.installable.is_none() && self.signed_in.is_none()
    }

    /// Not found on the computer: Harness says it could install it there
    /// (`installable: true`) and doesn't say it is installed. `installable:
    /// false` with `installed: null` is unknown: the hand-over goes, and
    /// Harness's own `ENGINE_NOT_INSTALLED` decides. A computer whose shell
    /// took over 5 s reads so too, so it is said as "not found", never as
    /// "not installed".
    fn not_found(&self) -> bool {
        self.installable.as_ref().is_some_and(|r| r.value)
            && !self.installed.as_ref().is_some_and(|r| r.value)
    }
}

impl Computer {
    fn is_empty(&self) -> bool {
        self.status.is_none() && self.engines.is_empty()
    }

    /// Only what was read within [`STALE_AFTER_MS`] of `now_ms`, and not
    /// later than a minute after it (a clock set back): the rest unknown.
    pub fn fresh(&self, now_ms: u64) -> Self {
        let fresh = |read_ms: u64| {
            read_ms <= now_ms.saturating_add(60_000)
                && now_ms.saturating_sub(read_ms) <= STALE_AFTER_MS
        };
        let engines = self
            .engines
            .iter()
            .map(|(name, known)| {
                let known = EngineKnown {
                    installed: known.installed.clone().filter(|r| fresh(r.read_ms)),
                    installable: known.installable.clone().filter(|r| fresh(r.read_ms)),
                    signed_in: known.signed_in.clone().filter(|r| fresh(r.read_ms)),
                };
                (name.clone(), known)
            })
            .filter(|(_, known)| !known.is_empty())
            .collect();
        Self {
            status: self.status.clone().filter(|r| fresh(r.read_ms)),
            engines,
            order: self.order.clone(),
        }
    }

    /// The engines known installed here now, by Harness's name, in
    /// Harness's order (#6492): the order its last look gave, then any
    /// other known installed, by name.
    pub fn installed_in_order(&self) -> Vec<String> {
        let installed = |name: &String| {
            self.engines
                .get(name)
                .and_then(|known| known.installed.as_ref())
                .is_some_and(|r| r.value)
        };
        let mut names: Vec<String> = self
            .order
            .iter()
            .filter(|n| installed(n))
            .cloned()
            .collect();
        for name in self.engines.keys() {
            if installed(name) && !names.contains(name) {
                names.push(name.clone());
            }
        }
        names
    }

    fn installed(&self, engine: EngineKind) -> Option<bool> {
        self.engines
            .get(engine.argument())?
            .installed
            .as_ref()
            .map(|r| r.value)
    }

    /// Why a hand-over of `engine`'s work to this computer is refused now,
    /// from what is known: the code and its words. Only a known `offline`,
    /// an engine not found there ([`EngineKnown::not_found`]) or a known
    /// `signedIn: false` refuses;
    /// anything unknown never does. Devin Cloud works in Devin's cloud, not
    /// on the computer, and isn't one of Harness's engines: never refused.
    pub fn refusal(&self, engine: EngineKind) -> Option<(&'static str, &'static str)> {
        if engine.cloud() {
            return None;
        }
        if self.offline() {
            return Some((
                COMPUTER_OFFLINE,
                "That computer seems to be asleep or offline; nothing was handed over.",
            ));
        }
        let known = self.engines.get(engine.argument())?;
        if known.not_found() {
            return Some((
                ENGINE_MISSING,
                "That agent wasn't found on that computer; nothing was handed over.",
            ));
        }
        if known.signed_in.as_ref().is_some_and(|r| !r.value) {
            return Some((
                ENGINE_NOT_SIGNED_IN,
                "That agent isn't signed in on that computer; nothing was handed over.",
            ));
        }
        None
    }

    /// Whether the computer is known offline.
    pub fn offline(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s.value == "offline")
    }

    /// Whether `engine` wasn't found on this computer.
    pub fn missing(&self, engine: EngineKind) -> bool {
        !engine.cloud()
            && self
                .engines
                .get(engine.argument())
                .is_some_and(EngineKnown::not_found)
    }
}

/// A reading kept as the computer said it: a value replaces what was
/// known; a `null` forgets a value that refuses (`refusing`: a no for
/// `installed` and `signedIn`, a yes for `installable`), since it may no
/// longer hold and an unknown never refuses, but keeps one that doesn't,
/// so one shell that timed out doesn't move a computer's target to
/// another engine.
fn merged(
    before: Option<Reading<bool>>,
    said: Option<bool>,
    refusing: bool,
    now_ms: u64,
) -> Option<Reading<bool>> {
    match said {
        Some(value) => Some(Reading {
            value,
            read_ms: now_ms,
        }),
        None => before.filter(|r| r.value != refusing),
    }
}

/// Everything known, by machine id in lower case: kept while the part
/// runs, and written beside the targets ([`write_engines`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Known {
    machines: BTreeMap<String, Computer>,
}

impl Known {
    /// The presence `harness machines --json` gave each machine. A machine
    /// listed with no status, or a word Harness doesn't give, keeps what
    /// was known.
    pub fn listed(&mut self, machines: &[AccountMachine], now_ms: u64) {
        for machine in machines {
            let computer = self
                .machines
                .entry(machine.machine_id.to_lowercase())
                .or_default();
            if let Some(status) = &machine.status {
                computer.status = Some(Reading {
                    value: status.clone(),
                    read_ms: now_ms,
                });
            }
        }
    }

    /// Lets go of every machine but these (any case): the ones looked at.
    pub fn keep_only(&mut self, machines: &std::collections::BTreeSet<String>) {
        let kept: Vec<String> = machines.iter().map(|id| id.to_lowercase()).collect();
        self.machines.retain(|id, _| kept.contains(id));
    }

    /// One computer's engines as it said them just now ([`merged`]). An
    /// engine it didn't list keeps what was known. A look that failed is
    /// never given here: what was known is kept.
    pub fn probed(&mut self, machine: &str, rows: &[EngineRow], now_ms: u64) {
        let computer = self.machines.entry(machine.to_lowercase()).or_default();
        for row in rows {
            let known = computer.engines.entry(row.engine.clone()).or_default();
            known.installed = merged(known.installed.take(), row.installed, false, now_ms);
            known.installable = merged(known.installable.take(), row.installable, true, now_ms);
            known.signed_in = merged(known.signed_in.take(), row.signed_in, false, now_ms);
            // Installable now: not installed, whatever was known before.
            if row.installable == Some(true) && row.installed != Some(true) {
                known.installed = None;
            }
        }
        computer.engines.retain(|_, known| !known.is_empty());
        computer.order = rows.iter().map(|row| row.engine.clone()).collect();
    }

    /// What is known of `machine`, fresh at `now_ms`.
    pub fn computer(&self, machine: &str, now_ms: u64) -> Option<Computer> {
        self.machines
            .get(&machine.to_lowercase())
            .map(|computer| computer.fresh(now_ms))
            .filter(|computer| !computer.is_empty())
    }

    /// The engine a target written from the machines gives `machine`:
    /// `wanted` (the person's, `--targets-from-machines ENGINE`) where it
    /// is known installed there, else the first of [`ENGINE_ORDER`] known
    /// installed, else `wanted`, as before anything was known.
    pub fn chosen(&self, machine: &str, wanted: EngineKind, now_ms: u64) -> EngineKind {
        let Some(computer) = self.computer(machine, now_ms) else {
            return wanted;
        };
        if computer.installed(wanted) == Some(true) {
            return wanted;
        }
        ENGINE_ORDER
            .into_iter()
            .find(|engine| computer.installed(*engine) == Some(true))
            .unwrap_or(wanted)
    }

    /// The engine [`Known::chosen`] gives each machine, by machine id: the
    /// lamp's own (`current`) never, as it is no computer of the person's;
    /// it is a target only with [`Known::lamp_engine`].
    pub fn choices(
        &self,
        machines: &[AccountMachine],
        wanted: EngineKind,
        now_ms: u64,
    ) -> BTreeMap<String, EngineKind> {
        machines
            .iter()
            .filter(|m| !m.current)
            .map(|m| {
                (
                    m.machine_id.clone(),
                    self.chosen(&m.machine_id, wanted, now_ms),
                )
            })
            .collect()
    }
}

impl Known {
    /// The engines the lamp's own machine (the one Harness marks
    /// `current`) is known to have installed now, in Harness's order
    /// ([`Computer::installed_in_order`], #6492). None when that machine
    /// is not listed or no fresh look at its engines is known.
    pub fn lamp_installed(&self, machines: &[AccountMachine], now_ms: u64) -> Option<Vec<String>> {
        let lamp = machines.iter().find(|m| m.current)?;
        let computer = self
            .computer(&lamp.machine_id, now_ms)
            .filter(|computer| !computer.engines.is_empty())?;
        Some(computer.installed_in_order())
    }

    /// `LAMP_OPENCLAW_ON_LAMP` and `LAMP_HERMES_ON_LAMP` (#5978, #6226):
    /// the lamp's own machine, the one Harness marks `current`, with the
    /// first engine of `order` (the lamp's engines switched on, the chosen
    /// one first) known installed there now (its engines' look said
    /// `installed: true`): its id and that engine. None otherwise: not
    /// looked at yet, none of them there, or the reading stale.
    pub fn lamp_engine(
        &self,
        machines: &[AccountMachine],
        order: &[EngineKind],
        now_ms: u64,
    ) -> Option<(String, EngineKind)> {
        let lamp = machines.iter().find(|m| m.current)?;
        let computer = self.computer(&lamp.machine_id, now_ms)?;
        order
            .iter()
            .find(|engine| computer.installed(**engine) == Some(true))
            .map(|engine| (lamp.machine_id.clone(), *engine))
    }
}

/// The file beside a targets file: `targets.engines.json` beside
/// `targets.json`.
pub fn engines_path(targets: &Path) -> PathBuf {
    targets.with_extension("engines.json")
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    computers: BTreeMap<String, Computer>,
}

/// The most computers the file names: one a target.
const MOST_COMPUTERS: usize = 8;
/// The most engines it names for one computer.
const MOST_ENGINES: usize = 16;
/// The file's bound, as the targets'.
const MOST_BYTES: u64 = 16_384;

/// The engines file as written: `{"version":1,"computers":{ALIAS:{
/// "status":{"value":WORD,"read_ms":MS},"engines":{ENGINE:{"installed":
/// {"value":BOOL,"read_ms":MS},"installable":…,"signed_in":…}}}}}`, each
/// field there only
/// when known. Read as strictly as the targets: an owned 0600 file of at
/// most 16384 bytes, version 1, nothing else in it, each alias of a target
/// alias's shape, at most 8 computers of at most 16 engines, each status
/// one Harness gives. Anything else is an error, and nothing is refused
/// from it: hand-overs go as before.
pub fn read_engines(path: &Path) -> io::Result<BTreeMap<String, Computer>> {
    let invalid = |message: &'static str| io::Error::new(io::ErrorKind::InvalidInput, message);
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.len() > MOST_BYTES
    {
        return Err(invalid(
            "engines must be an owned unlinked 0600 regular file at most 16384 bytes",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MOST_BYTES + 1).read_to_end(&mut bytes)?;
    let document: Document =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid engines JSON"))?;
    if document.version != 1 || document.computers.len() > MOST_COMPUTERS {
        return Err(invalid(
            "engines requires version 1 and at most 8 computers",
        ));
    }
    for (alias, computer) in &document.computers {
        let plain = |word: &str, max: usize, extra: u8| {
            !word.is_empty()
                && word.len() <= max
                && word
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == extra)
        };
        if !plain(alias, 40, b'_')
            || computer.engines.len() > MOST_ENGINES
            || computer
                .engines
                .keys()
                .any(|engine| !plain(engine, 32, b'-'))
            || computer.status.as_ref().is_some_and(|status| {
                !crate::harness::MACHINE_STATUSES.contains(&status.value.as_str())
            })
        {
            return Err(invalid("invalid engines entry"));
        }
    }
    Ok(document.computers)
}

/// What one write of the engines file did, for the journal.
#[derive(Debug, PartialEq, Eq)]
pub struct EnginesWritten {
    pub changed: bool,
    pub said: String,
}

/// Write the engines file beside `targets_file` from what is known of the
/// machines `targets` name, by alias. Written whole when it differs,
/// beside and renamed over, 0600; removed when nothing is known. A file
/// there that isn't one this part writes ([`read_engines`] refuses it) is
/// left as it is: never written over.
pub fn write_engines(
    targets_file: &Path,
    targets: &BTreeMap<String, Target>,
    known: &Known,
    now_ms: u64,
) -> io::Result<EnginesWritten> {
    let path = engines_path(targets_file);
    let present = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if present.is_some() && read_engines(&path).is_err() {
        return Ok(EnginesWritten {
            changed: false,
            said: "the engines file is not one this part writes; it is left as it is".into(),
        });
    }
    let computers: BTreeMap<String, Computer> = targets
        .iter()
        .filter_map(|(alias, target)| {
            let mut computer = known.computer(&target.machine_id, now_ms)?;
            computer.engines = computer.engines.into_iter().take(MOST_ENGINES).collect();
            Some((alias.clone(), computer))
        })
        .take(MOST_COMPUTERS)
        .collect();
    let count = computers.len();
    if computers.is_empty() {
        if present.is_some() {
            fs::remove_file(&path)?;
            return Ok(EnginesWritten {
                changed: true,
                said: "nothing known of the computers' engines: the engines file is removed".into(),
            });
        }
        return Ok(EnginesWritten {
            changed: false,
            said: "nothing known of the computers' engines yet".into(),
        });
    }
    let mut bytes = serde_json::to_vec(&Document {
        version: 1,
        computers,
    })
    .map_err(io::Error::other)?;
    bytes.push(b'\n');
    if present.as_deref() == Some(bytes.as_slice()) {
        return Ok(EnginesWritten {
            changed: false,
            said: format!("engines known for {count} computers, unchanged"),
        });
    }
    crate::runtime::write_atomically(&path, &bytes)?;
    Ok(EnginesWritten {
        changed: true,
        said: format!("engines known for {count} computers, written"),
    })
}

/// One look at a computer's engines, for the journal: each engine Harness
/// named and what it said, never the computer's name or id.
pub fn probe_line(rows: &Result<Vec<EngineRow>, String>, took_ms: u128) -> String {
    let word = |value: Option<bool>| match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    };
    match rows {
        Ok(rows) => {
            let each: Vec<String> = rows
                .iter()
                .map(|row| {
                    format!(
                        "{} installed {} installable {} signed in {}",
                        row.engine,
                        word(row.installed),
                        word(row.installable),
                        word(row.signed_in)
                    )
                })
                .collect();
            format!(
                "a computer's engines read in {took_ms} ms: {}",
                each.join(", ")
            )
        }
        Err(code) => format!(
            "a computer's engines not read in {took_ms} ms ({code}); what was known is kept"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn row(engine: &str, installed: Option<bool>, signed_in: Option<bool>) -> EngineRow {
        EngineRow {
            engine: engine.into(),
            installed,
            installable: None,
            signed_in,
        }
    }

    /// Harness's row for an engine not on the computer (openharness
    /// `machineEngines.ts`): `installed` null, `installable` true.
    fn not_found(engine: &str) -> EngineRow {
        EngineRow {
            installable: Some(true),
            ..row(engine, None, None)
        }
    }

    fn machine(id: char, status: Option<&str>) -> AccountMachine {
        AccountMachine {
            machine_id: id.to_string().repeat(32),
            label: "Mac".into(),
            current: false,
            status: status.map(str::to_owned),
        }
    }

    fn target(id: char, engine: EngineKind) -> Target {
        Target {
            machine_id: id.to_string().repeat(32),
            project: "~/harnesses/lamp-tasks".into(),
            engine,
        }
    }

    /// A null is unknown, never missing: it forgets a known no and keeps a
    /// known yes; a value replaces either. An engine not in a look keeps
    /// what was known.
    #[test]
    fn a_null_is_unknown_and_never_a_no() {
        let id = "a".repeat(32);
        let mut known = Known::default();
        known.probed(&id, &[row("claude", Some(true), Some(false))], NOW);
        let had = known.computer(&id, NOW).unwrap();
        assert_eq!(had.installed(EngineKind::Claude), Some(true));
        assert_eq!(
            had.refusal(EngineKind::Claude).map(|r| r.0),
            Some(ENGINE_NOT_SIGNED_IN)
        );
        // The computer couldn't say: the no is forgotten, the yes kept.
        known.probed(&id, &[row("claude", None, None)], NOW + 1);
        let now = known.computer(&id, NOW + 1).unwrap();
        assert_eq!(now.installed(EngineKind::Claude), Some(true));
        assert_eq!(now.refusal(EngineKind::Claude), None);
        // A look that lists only Codex leaves Claude Code as it was.
        known.probed(&id, &[row("codex", None, Some(false))], NOW + 2);
        assert_eq!(
            known
                .computer(&id, NOW + 2)
                .unwrap()
                .installed(EngineKind::Claude),
            Some(true)
        );
        // Not found, then a look that can't say: forgotten, so it goes.
        known.probed(&id, &[not_found("codex")], NOW + 3);
        assert!(
            known
                .computer(&id, NOW + 3)
                .unwrap()
                .missing(EngineKind::Codex)
        );
        known.probed(&id, &[row("codex", None, None)], NOW + 4);
        assert!(
            !known
                .computer(&id, NOW + 4)
                .unwrap()
                .missing(EngineKind::Codex)
        );
        // Installable once more forgets a yes known before.
        known.probed(&id, &[not_found("claude")], NOW + 5);
        let gone = known.computer(&id, NOW + 5).unwrap();
        assert_eq!(gone.installed(EngineKind::Claude), None);
        assert!(gone.missing(EngineKind::Claude));
        // Nothing known of an engine is no entry at all.
        let mut nothing = Known::default();
        nothing.probed(&id, &[row("codex", None, None)], NOW);
        assert_eq!(nothing.computer(&id, NOW), None);
    }

    /// Only a known offline, an engine not found (`installed` null with
    /// `installable` true) or a known not signed in refuses, in that order;
    /// unknown proceeds (`installable: false` with `installed` null among
    /// it), and Devin Cloud is never refused.
    #[test]
    fn each_early_refusal_and_unknown_proceeds() {
        let id = "a".repeat(32);
        let mut known = Known::default();
        assert_eq!(known.computer(&id, NOW), None);
        known.listed(&[machine('a', Some("running"))], NOW);
        let running = known.computer(&id, NOW).unwrap();
        assert_eq!(running.refusal(EngineKind::Codex), None);
        known.probed(
            &id,
            &[
                row("claude", Some(true), Some(false)),
                not_found("codex"),
                row("opencode", Some(true), None),
                EngineRow {
                    installable: Some(false),
                    ..row("hermes", None, None)
                },
            ],
            NOW,
        );
        let c = known.computer(&id, NOW).unwrap();
        assert_eq!(
            c.refusal(EngineKind::Claude).unwrap().0,
            ENGINE_NOT_SIGNED_IN
        );
        assert_eq!(c.refusal(EngineKind::Codex).unwrap().0, ENGINE_MISSING);
        assert!(c.missing(EngineKind::Codex) && !c.missing(EngineKind::Opencode));
        assert!(
            !c.missing(EngineKind::Hermes),
            "installable false is unknown"
        );
        assert_eq!(c.refusal(EngineKind::Opencode), None);
        assert_eq!(c.refusal(EngineKind::Hermes), None);
        assert_eq!(c.refusal(EngineKind::DevinCloud), None);
        // Offline before anything of its engines.
        known.listed(&[machine('a', Some("offline"))], NOW);
        let off = known.computer(&id, NOW).unwrap();
        for engine in [EngineKind::Claude, EngineKind::Opencode, EngineKind::Hermes] {
            assert_eq!(off.refusal(engine).unwrap().0, COMPUTER_OFFLINE);
        }
        assert_eq!(off.refusal(EngineKind::DevinCloud), None);
        // `unknown` and `payment_pending` refuse nothing; a listing with no
        // status keeps the one known.
        for word in ["unknown", "payment_pending"] {
            known.listed(&[machine('a', Some(word))], NOW);
            assert_eq!(
                known
                    .computer(&id, NOW)
                    .unwrap()
                    .refusal(EngineKind::Opencode),
                None
            );
        }
        known.listed(&[machine('a', Some("offline"))], NOW);
        known.listed(&[machine('a', None)], NOW + 5);
        assert_eq!(
            known
                .computer(&id, NOW + 5)
                .unwrap()
                .refusal(EngineKind::Opencode)
                .unwrap()
                .0,
            COMPUTER_OFFLINE
        );
        // Old news is unknown.
        let later = NOW + STALE_AFTER_MS + 1;
        assert_eq!(known.computer(&id, later), None);
        // A machine no longer looked at is let go.
        known.listed(&[machine('a', Some("offline"))], NOW);
        known.keep_only(&std::collections::BTreeSet::from(["B".repeat(32)]));
        assert_eq!(known.computer(&id, NOW), None);
    }

    /// The person's engine where it is there, else the first there of
    /// Claude Code, Codex and OpenCode, else the person's.
    #[test]
    fn the_engine_chosen_is_one_the_computer_has() {
        let id = "a".repeat(32);
        let mut known = Known::default();
        assert_eq!(
            known.chosen(&id, EngineKind::Claude, NOW),
            EngineKind::Claude
        );
        assert_eq!(known.chosen(&id, EngineKind::Codex, NOW), EngineKind::Codex);
        known.probed(
            &id,
            &[
                row("claude", None, None),
                row("codex", Some(true), Some(true)),
                row("opencode", Some(true), None),
            ],
            NOW,
        );
        assert_eq!(
            known.chosen(&id, EngineKind::Claude, NOW),
            EngineKind::Codex
        );
        assert_eq!(
            known.chosen(&id, EngineKind::Opencode, NOW),
            EngineKind::Opencode
        );
        known.probed(&id, &[row("claude", Some(true), None)], NOW);
        assert_eq!(
            known.chosen(&id, EngineKind::Claude, NOW),
            EngineKind::Claude
        );
        assert_eq!(known.chosen(&id, EngineKind::Codex, NOW), EngineKind::Codex);
        // Nothing installed known: the person's, as before.
        let mut none = Known::default();
        none.probed(&id, &[row("claude", Some(false), None)], NOW);
        assert_eq!(
            none.chosen(&id, EngineKind::Claude, NOW),
            EngineKind::Claude
        );
        let choices = known.choices(
            &[machine('a', None), machine('b', None)],
            EngineKind::Opencode,
            NOW,
        );
        assert_eq!(choices[&"a".repeat(32)], EngineKind::Opencode);
        assert_eq!(choices[&"b".repeat(32)], EngineKind::Opencode);
    }

    fn private_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// The engines file is written beside the targets by alias, read back
    /// as written, rewritten only when it differs, removed when nothing is
    /// known, and never written over when it isn't this part's.
    #[test]
    fn the_engines_file_is_written_beside_the_targets_and_read_back() {
        let dir = private_dir();
        let targets_file = dir.path().join("targets.json");
        let path = engines_path(&targets_file);
        assert_eq!(path, dir.path().join("targets.engines.json"));
        let targets = BTreeMap::from([
            ("office".to_owned(), target('a', EngineKind::Claude)),
            ("laptop".to_owned(), target('b', EngineKind::Codex)),
        ]);
        let mut known = Known::default();
        let nothing = write_engines(&targets_file, &targets, &known, NOW).unwrap();
        assert!(!nothing.changed && !path.exists());
        known.listed(&[machine('a', Some("offline")), machine('b', None)], NOW);
        known.probed(
            &"a".repeat(32),
            &[row("claude", Some(true), Some(false))],
            NOW,
        );
        let written = write_engines(&targets_file, &targets, &known, NOW).unwrap();
        assert!(written.changed, "{written:?}");
        let mode = fs::metadata(&path).unwrap().mode() & 0o7777;
        assert_eq!(mode, 0o600);
        let read = read_engines(&path).unwrap();
        assert_eq!(read.keys().collect::<Vec<_>>(), ["office"]);
        // Harness's order is kept in memory only (#6492).
        let mut computer = known.computer(&"a".repeat(32), NOW).unwrap();
        assert_eq!(computer.order, ["claude"]);
        computer.order.clear();
        assert_eq!(read["office"], computer);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
            serde_json::json!({"version":1,"computers":{"office":{
                "status":{"value":"offline","read_ms":NOW},
                "engines":{"claude":{"installed":{"value":true,"read_ms":NOW},
                    "signed_in":{"value":false,"read_ms":NOW}}}}}})
        );
        assert!(
            !write_engines(&targets_file, &targets, &known, NOW)
                .unwrap()
                .changed
        );
        // Gone stale: removed.
        let removed = write_engines(&targets_file, &targets, &known, NOW + STALE_AFTER_MS + 1);
        assert!(removed.unwrap().changed && !path.exists());
        // Someone else's file is never written over.
        fs::write(&path, b"{\"mine\":true}\n").unwrap();
        let left = write_engines(&targets_file, &targets, &known, NOW).unwrap();
        assert!(!left.changed, "{left:?}");
        assert_eq!(fs::read(&path).unwrap(), b"{\"mine\":true}\n");
    }

    /// Read one release back: a file this build doesn't know (a newer
    /// version, a field it lacks, a status Harness doesn't give, too many)
    /// is an error, and nothing is refused from it.
    #[test]
    fn the_engines_file_is_read_strictly() {
        use std::io::Write;
        let dir = private_dir();
        let path = dir.path().join("targets.engines.json");
        let write = |bytes: &str| {
            let _ = fs::remove_file(&path);
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .unwrap()
                .write_all(bytes.as_bytes())
                .unwrap();
        };
        write(r#"{"version":1,"computers":{"office":{"status":{"value":"running","read_ms":1}}}}"#);
        assert_eq!(
            read_engines(&path).unwrap()["office"]
                .status
                .as_ref()
                .unwrap()
                .value,
            "running"
        );
        for bad in [
            r#"{"version":2,"computers":{}}"#,
            r#"{"version":1,"computers":{},"more":1}"#,
            r#"{"version":1,"computers":{"office":{"status":{"value":"asleep","read_ms":1}}}}"#,
            r#"{"version":1,"computers":{"office":{"engines":{"claude":{"installed":{"value":true,"read_ms":1},"ready":true}}}}}"#,
            r#"{"version":1,"computers":{"bad alias":{}}}"#,
            r#"{"version":1,"computers":{"office":{"engines":{"Claude Code":{}}}}}"#,
            "not json",
        ] {
            write(bad);
            assert!(read_engines(&path).is_err(), "{bad}");
        }
        let many: BTreeMap<String, Computer> = (0..9)
            .map(|n| (format!("c{n}"), Computer::default()))
            .collect();
        write(
            &serde_json::to_string(&Document {
                version: 1,
                computers: many,
            })
            .unwrap(),
        );
        assert!(read_engines(&path).is_err());
        // Not 0600: refused.
        write(r#"{"version":1,"computers":{}}"#);
        fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
        assert!(read_engines(&path).is_err());
    }

    #[test]
    fn a_probe_line_names_engines_never_the_computer() {
        let line = probe_line(
            &Ok(vec![
                row("claude", Some(true), None),
                row("codex", None, Some(false)),
            ]),
            12,
        );
        assert_eq!(
            line,
            "a computer's engines read in 12 ms: claude installed yes installable unknown signed in unknown, codex installed unknown installable unknown signed in no"
        );
        assert_eq!(
            probe_line(&Err("unreachable".into()), 30_001),
            "a computer's engines not read in 30001 ms (unreachable); what was known is kept"
        );
    }

    /// #5978: the lamp's own machine is the lamp only where its look said
    /// Hermes is installed; never a choice for a computer's target.
    /// #6492: the lamp's own machine's engines known installed, in the
    /// order Harness's look gave them, not by name; a computer's never.
    #[test]
    fn the_lamps_installed_engines_keep_harness_s_order() {
        let lamp = AccountMachine {
            current: true,
            ..machine('a', Some("running"))
        };
        let listed = [lamp.clone(), machine('b', Some("running"))];
        let mut known = Known::default();
        known.listed(&listed, NOW);
        assert_eq!(known.lamp_installed(&listed, NOW), None, "not looked at");
        known.probed(
            &lamp.machine_id,
            &[
                row("openclaw", Some(true), None),
                not_found("codex"),
                row("hermes", Some(true), None),
                row("claude", Some(true), None),
            ],
            NOW,
        );
        assert_eq!(
            known.lamp_installed(&listed, NOW).unwrap(),
            ["openclaw", "hermes", "claude"]
        );
        known.probed(&"b".repeat(32), &[row("pi", Some(true), None)], NOW);
        assert!(
            !known
                .lamp_installed(&listed, NOW)
                .unwrap()
                .contains(&"pi".to_owned())
        );
        assert_eq!(
            known.lamp_installed(&listed, NOW + STALE_AFTER_MS + 1),
            None,
            "stale"
        );
    }

    #[test]
    fn the_lamp_is_kept_only_where_hermes_is_known_installed() {
        let lamp = AccountMachine {
            current: true,
            ..machine('a', Some("running"))
        };
        let listed = [lamp.clone(), machine('b', Some("running"))];
        let mut known = Known::default();
        known.listed(&listed, NOW);
        let hermes = [EngineKind::Hermes];
        assert_eq!(
            known.lamp_engine(&listed, &hermes, NOW),
            None,
            "not looked at"
        );
        known.probed(&lamp.machine_id, &[not_found("hermes")], NOW);
        assert_eq!(known.lamp_engine(&listed, &hermes, NOW), None, "not there");
        known.probed(&lamp.machine_id, &[row("hermes", Some(true), None)], NOW);
        assert_eq!(
            known.lamp_engine(&listed, &hermes, NOW),
            Some((lamp.machine_id.clone(), EngineKind::Hermes))
        );
        assert_eq!(
            known.lamp_engine(&listed, &hermes, NOW + STALE_AFTER_MS + 1),
            None,
            "stale"
        );
        // Only a computer's: the lamp's own machine gets no engine choice.
        let chosen = known.choices(&listed, EngineKind::Claude, NOW);
        assert_eq!(chosen.keys().collect::<Vec<_>>(), [&"b".repeat(32)]);
    }

    /// #6226: the lamp's engine is the first of the order installed there:
    /// Hermes by default with both (#6498), OpenClaw when it is chosen first, and
    /// the one installed when only one is; none when neither is switched on.
    #[test]
    fn the_lamps_engine_is_the_first_of_the_order_installed_there() {
        let lamp = AccountMachine {
            current: true,
            ..machine('a', Some("running"))
        };
        let listed = [lamp.clone(), machine('b', Some("running"))];
        let id = lamp.machine_id.clone();
        let default = [EngineKind::Hermes, EngineKind::Openclaw];
        let openclaw_first = [EngineKind::Openclaw, EngineKind::Hermes];
        let mut known = Known::default();
        known.listed(&listed, NOW);
        let both = [
            row("hermes", Some(true), None),
            row("openclaw", Some(true), None),
        ];
        known.probed(&id, &both, NOW);
        assert_eq!(
            known.lamp_engine(&listed, &default, NOW),
            Some((id.clone(), EngineKind::Hermes))
        );
        assert_eq!(
            known.lamp_engine(&listed, &openclaw_first, NOW),
            Some((id.clone(), EngineKind::Openclaw))
        );
        assert_eq!(known.lamp_engine(&listed, &[], NOW), None);
        known.probed(
            &id,
            &[row("hermes", Some(true), None), not_found("openclaw")],
            NOW,
        );
        assert_eq!(
            known.lamp_engine(&listed, &default, NOW),
            Some((id.clone(), EngineKind::Hermes))
        );
        assert_eq!(
            known.lamp_engine(&listed, &[EngineKind::Openclaw], NOW),
            None
        );
        known.probed(
            &id,
            &[not_found("hermes"), row("openclaw", Some(true), None)],
            NOW,
        );
        assert_eq!(
            known.lamp_engine(&listed, &default, NOW),
            Some((id, EngineKind::Openclaw))
        );
    }
}
