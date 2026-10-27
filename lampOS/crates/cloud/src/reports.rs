//! Health and crash reports to the company, opt-in at setup (#1683,
//! `docs/cloud.md`, "Health and crash reports"). Three gates must all be open for anything to go:
//! `LAMP_CLOUD=on`, `LAMP_REPORTS=on`, and the person's yes, read at each
//! send.
//!
//! A report carries numbers, booleans, enums and names from fixed lists
//! only. Every field is typed so: a part's or a unit's name is one of
//! [`PARTS`] or [`UNITS`] (a `&'static str` taken from the list, never the
//! name as given), a build is hex digits and `+`, and no field of any type
//! here holds text that came in. So no message, reason or journal line can
//! reach a report, whatever a part writes on the bus or in an exit note; a
//! canary test plants secrets in every input to show it.
//!
//! It rides `lamp-cloud`'s own connection, on `<fd_channel>/report` at
//! QoS 1, kept until the broker acknowledges it, with the device's `id` as
//! `info` carries it. The backend agreed this shape on #1683 (the topic, the
//! `id`, `sent_at` set once, `stopped` sent until it is through and once
//! more), behind [`SCHEMA`].
use lamp_msgs::{BoardState, Health, Level, Online, SelfTest, Verdict};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod query;

/// `on` sends reports, with `LAMP_CLOUD=on` and the person's yes. Anything
/// else: nothing of this runs.
pub const ENVIRONMENT: &str = "LAMP_REPORTS";
/// Where reports go: below the lamp's own reply channel, so they are never
/// handed to the app's side as a reply (the backend, #1683).
pub fn topic(fd_channel: &str) -> String {
    format!("{fd_channel}/report")
}

/// The longest device id a report carries; room is kept for it in
/// [`MOST_BYTES`].
pub const ID_MOST: usize = 128;

/// The record's schema. Adding a field keeps `/1`; removing, renaming or
/// retyping one is `/2`.
pub const SCHEMA: &str = "lamp-report/1";
/// As every other reply says it: the backend routes a lamp by it.
pub const DEVICE: &str = "lamp";
/// Which Lamp it is, as every reply and setup's status say it (#1396).
pub const GENERATION: &str = crate::wire::GENERATION;
/// The setup question's wording, by version. A yes to an older wording
/// counts; a later wording that asks for more starts at no.
pub const ASKED: u64 = 1;

/// The parts a report may name: the product's `--expect` list
/// (`deploy/product/lamp-health.service`) and those `lamp-health show`
/// knows by name, plus this one. Any other is only counted.
pub const PARTS: &[&str] = &[
    "board",
    "voice",
    "eyes",
    "body",
    "policy",
    "name",
    "controls",
    "expression",
    "actions",
    "ears",
    "memory",
    "schedule",
    "air",
    "tasks",
    "selftest",
    "keeper",
    "skills",
    "cloud",
];

/// The parts the self-test names (`lamp_msgs::PartResult::part`).
pub const SELFTEST_PARTS: &[&str] = &[
    "voice_mic",
    "room_mic",
    "speaker",
    "servo_1",
    "servo_2",
    "servo_3",
    "servo_4",
    "servo_5",
    "ring",
    "camera",
    "touch",
    "button",
    "air",
    "board",
    "name",
    "body",
];

/// The units a report may name: the product's `lamp-*.service` files
/// (`deploy/product/`). A test holds the two the same.
pub const UNITS: &[&str] = &[
    "lamp-actions.service",
    "lamp-addons.service",
    "lamp-air-clock.service",
    "lamp-air.service",
    "lamp-body.service",
    "lamp-cloud.service",
    "lamp-control.service",
    "lamp-controls.service",
    "lamp-ears.service",
    "lamp-expression.service",
    "lamp-eyes-camera@.service",
    "lamp-eyes.service",
    "lamp-health.service",
    "lamp-keeper-restart@.service",
    "lamp-memory.service",
    "lamp-mood.service",
    "lamp-name.service",
    "lamp-policy.service",
    "lamp-records.service",
    "lamp-schedule.service",
    "lamp-selftest.service",
    "lamp-setup-change@.service",
    "lamp-setup-keys@.service",
    "lamp-setup-recover.service",
    "lamp-setup-reset.service",
    "lamp-setup-wifi-restore.service",
    "lamp-setup-wifi@.service",
    "lamp-setup.service",
    "lamp-skills-manage@.service",
    "lamp-skills.service",
    "lamp-tasks.service",
    "lamp-update-apply.service",
    "lamp-update-back.service",
    "lamp-update.service",
    "lamp-voice.service",
    "lamp-waking.service",
];

/// The signals a crash may name.
const SIGNALS: [&str; 16] = [
    "SIGHUP", "SIGINT", "SIGQUIT", "SIGILL", "SIGTRAP", "SIGABRT", "SIGBUS", "SIGFPE", "SIGKILL",
    "SIGUSR1", "SIGSEGV", "SIGUSR2", "SIGPIPE", "SIGALRM", "SIGTERM", "SIGSYS",
];

/// A health report every quarter of an hour while the yes stands.
pub const HEALTH_EVERY: Duration = Duration::from_secs(15 * 60);
/// And one five minutes after `lamp-cloud` starts, so a lamp that dies
/// soon after a boot is seen.
pub const FIRST_HEALTH: Duration = Duration::from_secs(5 * 60);
/// A part at `error` this long sends a health report at once...
pub const ERROR_HOLDS: Duration = Duration::from_secs(60);
/// ...at most once an hour for that part.
pub const ERROR_AGAIN: Duration = Duration::from_secs(60 * 60);
/// How often the exit notes are looked at.
pub const NOTES_EVERY: Duration = Duration::from_secs(10);
/// How long a send waits while the voice is in a turn.
pub const VOICE_WAIT: Duration = Duration::from_secs(60);
/// The most crash reports in an hour and in a day, across every unit.
pub const CRASHES_PER_HOUR: usize = 12;
pub const CRASHES_PER_DAY: usize = 50;
/// The most one report may be, in bytes.
pub const MOST_BYTES: usize = 4_096;
/// The most parts one health report names; the rest are counted.
pub const MOST_PARTS: usize = 32;
/// The queue's bounds: reports, bytes and age.
pub const QUEUE_MOST: usize = 64;
pub const QUEUE_BYTES: usize = 256 * 1_024;
pub const KEPT_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// The exit notes kept per unit, the newest.
pub const NOTES_PER_UNIT: usize = 8;
/// How long `systemctl show` has before it is killed.
pub const SYSTEMCTL_WITHIN: Duration = Duration::from_secs(2);

/// Whether the switch is on.
pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// [`switched_on`] as the environment says, with `LAMP_CLOUD` too: both
/// must be `on`.
pub fn on_from_environment() -> bool {
    gates(
        std::env::var("LAMP_CLOUD").ok().as_deref(),
        std::env::var(ENVIRONMENT).ok().as_deref(),
    )
}

/// The two switches' gate: `LAMP_CLOUD` and `LAMP_REPORTS` both `on`.
pub fn gates(cloud: Option<&str>, reports: Option<&str>) -> bool {
    switched_on(cloud) && switched_on(reports)
}

/// Where the reports' files are.
#[derive(Clone, Debug)]
pub struct Places {
    /// Setup's record of the person's answer, root's, 0644.
    pub setup: PathBuf,
    /// `lamp-cloud`'s own record of a later choice, 0600; it wins.
    pub own: PathBuf,
    /// The queue of crash reports, and the sequence.
    pub queue: PathBuf,
    /// The exit notes the units' `exit-note` writes.
    pub exits: PathBuf,
    /// The host's boot, which the notes' times are on.
    pub boot_id: PathBuf,
}

impl Places {
    /// The product's, with `lamp-cloud`'s state folder.
    pub fn product(state: &Path) -> Self {
        Self {
            setup: "/etc/lampOS/setup/reports.json".into(),
            own: state.join("reports.json"),
            queue: state.join("reports"),
            exits: EXITS.into(),
            boot_id: "/proc/sys/kernel/random/boot_id".into(),
        }
    }
}

/// Where `exit-note` writes.
pub const EXITS: &str = "/run/lampos-exits";

/// A build as `lamp-update` names it, `<sequence>+<commit>`: hex digits and
/// `+` only, at most 40. Anything else is no build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Build(String);

impl Build {
    pub fn of(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty()
            && text.len() <= 40
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'+'))
        .then(|| Self(text.to_owned()))
    }
}

/// A part's name from [`PARTS`].
fn part(name: &str) -> Option<&'static str> {
    PARTS.iter().copied().find(|known| *known == name)
}

/// A unit's name from [`UNITS`].
pub fn unit(name: &str) -> Option<&'static str> {
    UNITS.iter().copied().find(|known| *known == name)
}

/// The part a unit runs, if it is one: `lamp-voice.service` is `voice`.
fn part_of(unit: &str) -> Option<&'static str> {
    part(unit.strip_prefix("lamp-")?.strip_suffix(".service")?)
}

/// A number a report may carry: finite, rounded to `places` decimals.
fn rounded(value: f32, places: i32) -> Option<f32> {
    let scale = 10f32.powi(places);
    value.is_finite().then(|| (value * scale).round() / scale)
}

/// The lamp's own counts of what it did not send.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dropped {
    /// Put out of the queue to keep it in its bounds.
    pub queue_full: u32,
    /// Crashes past [`CRASHES_PER_HOUR`] or [`CRASHES_PER_DAY`].
    pub over_rate: u32,
    /// Reports over [`MOST_BYTES`].
    pub too_big: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Health,
    Crash,
    Stopped,
}

/// One report as sent.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    #[serde(rename = "type")]
    pub kind_of_message: &'static str,
    pub schema: &'static str,
    pub device: &'static str,
    pub generation: &'static str,
    pub seq: u64,
    /// RFC 3339 UTC, whole seconds.
    pub sent_at: String,
    pub clock_synced: Option<bool>,
    pub build: Option<Build>,
    pub uptime_s: u64,
    pub dropped: Dropped,
    #[serde(flatten)]
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Body {
    Health(Box<HealthBody>),
    Crash(CrashBody),
    /// The last, when the person says no: the backend deletes what it has.
    Stopped,
}

impl Body {
    fn kind(&self) -> Kind {
        match self {
            Self::Health(_) => Kind::Health,
            Self::Crash(_) => Kind::Crash,
            Self::Stopped => Kind::Stopped,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PartLevel {
    pub name: &'static str,
    pub level: Level,
    pub age_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Board {
    pub cpu_load: Option<f32>,
    pub memory_used: Option<f32>,
    pub disk_free: Option<f32>,
    /// The hottest zone's degrees; zones' names are never sent.
    pub hottest_c: Option<f32>,
    /// Only on the board's own diagnostic, which `lamp-cloud` does not read
    /// in step 1: always null for now.
    pub cpu_c: Option<f32>,
    pub throttled: Option<bool>,
    pub throttled_s: Option<u32>,
    pub wifi_dbm: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SelfTestSummary {
    pub verdict: Verdict,
    /// The parts that failed, by [`SELFTEST_PARTS`] name only.
    pub failed: Vec<&'static str>,
    /// Failed parts not in that list, counted.
    pub other_failed: u32,
    pub not_checked: u32,
    pub build: Option<Build>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HealthBody {
    pub level: Level,
    pub parts: Vec<PartLevel>,
    pub missing: Vec<&'static str>,
    /// Parts outside [`PARTS`], and parts past [`MOST_PARTS`].
    pub other_parts: u32,
    pub board: Option<Board>,
    pub online: Option<bool>,
    pub down_for_s: Option<u64>,
    pub selftest: Option<SelfTestSummary>,
    /// Each unit's restarts since boot, [`UNITS`] only.
    pub restarts: BTreeMap<&'static str, u32>,
    /// Units still in systemd's failed state, sorted and unique, [`UNITS`] only.
    pub failed_units: Vec<&'static str>,
    /// A unit that waits, and why, both fixed text ([`update_waits`]).
    /// Left out when nothing waits, so a report without it is unchanged.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub waits: BTreeMap<&'static str, &'static str>,
}

/// The self-update switch, as `lamp-update.service` reads it (#2709). Set
/// in `lamp-cloud`'s environment too, it asks whether lamp-update waits.
pub const UPDATE_SELF_ENVIRONMENT: &str = "LAMP_UPDATE_SELF";
/// The armed way back lamp-update requires before it switches itself.
pub const RECOVERY_TIMER: &str = "lamp-update-back.timer";
/// Why lamp-update waits, as its own journal line begins it (#2709).
pub const RECOVERY_NOT_READY: &str =
    "independent recovery is not ready: no armed lamp-update-back.timer";

/// Whether lamp-update's self-update waits, from `systemctl show -p
/// LoadState -p UnitFileState -p ActiveState` of [`RECOVERY_TIMER`]: unless
/// it is loaded, persistently enabled and active, lamp-update refuses to
/// hand over (`update/src/world.rs`, which also checks the fallback and its
/// service; this reads only the timer).
pub fn update_waits(show: &str) -> bool {
    let fields = shown(show);
    let is = |key: &str, value: &str| fields.get(key).is_some_and(|v| v == value);
    !(is("LoadState", "loaded") && is("UnitFileState", "enabled") && is("ActiveState", "active"))
}

/// systemd's `$SERVICE_RESULT`, past `success`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExitResult {
    ExitCode,
    Signal,
    CoreDump,
    Watchdog,
    Timeout,
    OomKill,
    StartLimitHit,
    /// Any other result systemd gives (`resources`, `protocol`, …).
    Other,
}

impl ExitResult {
    /// `None` for `success`: nothing to report.
    fn of(result: &str) -> Option<Self> {
        Some(match result {
            "success" | "" => return None,
            "exit-code" => Self::ExitCode,
            "signal" => Self::Signal,
            "core-dump" => Self::CoreDump,
            "watchdog" => Self::Watchdog,
            "timeout" => Self::Timeout,
            "oom-kill" => Self::OomKill,
            "start-limit-hit" => Self::StartLimitHit,
            _ => Self::Other,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CrashBody {
    pub unit: &'static str,
    pub result: ExitResult,
    pub exit_code: Option<i32>,
    pub signal: Option<&'static str>,
    pub ran_s: Option<u32>,
    pub restarts_since_boot: Option<u32>,
    pub level_before: Option<Level>,
    /// Step 2's panic hook (`{file, line}`), not built: always null.
    pub panic: (),
}

/// What `exit-note` keeps of one stop, as systemd handed it over.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitNote {
    pub unit: String,
    /// `$SERVICE_RESULT`, `$EXIT_CODE`, `$EXIT_STATUS`.
    pub result: String,
    pub code: String,
    pub status: String,
    /// The host's monotonic clock at the stop, and its boot.
    pub at_us: u64,
    pub boot: String,
    pub ran_s: Option<u32>,
    pub restarts: Option<u32>,
}

/// The note for one stop: `None` for a `success`, or a unit not lampOS's.
/// A listed template's instance becomes its fixed template name before
/// storage. The caller still asks systemd about the actual instance.
pub fn exit_note(
    unit_name: &str,
    result: &str,
    code: &str,
    status: &str,
    at_us: u64,
    boot: &str,
    shown: &BTreeMap<String, String>,
) -> Option<ExitNote> {
    ExitResult::of(result)?;
    let unit = if let Some((prefix, suffix)) = unit_name.split_once('@') {
        let instance = suffix.strip_suffix(".service")?;
        if instance.is_empty()
            || !instance
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b":_.\\-".contains(&byte))
        {
            return None;
        }
        UNITS
            .iter()
            .copied()
            .find(|name| name.strip_suffix("@.service") == Some(prefix))?
    } else {
        unit(unit_name)?
    };
    let number = |key: &str| shown.get(key).and_then(|v| v.trim().parse::<u64>().ok());
    let ran_s = match (
        number("ExecMainStartTimestampMonotonic"),
        number("ExecMainExitTimestampMonotonic"),
    ) {
        (Some(start), Some(exit)) if exit >= start && start > 0 => {
            u32::try_from((exit - start) / 1_000_000).ok()
        }
        _ => None,
    };
    Some(ExitNote {
        unit: unit.to_owned(),
        result: result.to_owned(),
        code: code.to_owned(),
        status: status.to_owned(),
        at_us,
        boot: boot.to_owned(),
        ran_s,
        restarts: number("NRestarts").and_then(|n| u32::try_from(n).ok()),
    })
}

/// `systemctl show`'s `Key=value` lines.
pub fn shown(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

/// Each unit's `NRestarts` from `systemctl show -p Id -p NRestarts`, whose
/// units are blocks parted by blank lines; [`UNITS`] only.
pub fn restarts(text: &str) -> BTreeMap<&'static str, u32> {
    let mut found = BTreeMap::new();
    for block in text.split("\n\n") {
        let fields = shown(block);
        if let (Some(unit), Some(n)) = (
            fields.get("Id").and_then(|id| unit(id)),
            fields.get("NRestarts").and_then(|n| n.trim().parse().ok()),
        ) {
            found.insert(unit, n);
        }
    }
    found
}

/// A failed unit is named only by one unambiguous Id/ActiveState record.
/// Unknown names and states are omitted; duplicate records cannot make a
/// recovered unit appear failed. Restart counts have their existing parser.
fn failed_units(text: &str) -> Vec<&'static str> {
    let mut found = BTreeMap::new();
    for block in text.split("\n\n") {
        let mut id = None;
        let mut state = None;
        let mut names = Vec::new();
        let mut malformed = false;
        for line in block.lines() {
            let Some((key, value)) = line.split_once('=') else {
                malformed = true;
                continue;
            };
            match key {
                "Id" => {
                    if let Some(name) = unit(value) {
                        names.push(name);
                    }
                    malformed |= id.replace(value).is_some();
                }
                "ActiveState" => malformed |= state.replace(value).is_some(),
                _ => {}
            }
        }
        let failed = !malformed && state == Some("failed");
        for name in names {
            found
                .entry(name)
                .and_modify(|failed| *failed = false)
                .or_insert(failed);
        }
    }
    found
        .into_iter()
        .filter_map(|(name, failed)| failed.then_some(name))
        .collect()
}

#[derive(Clone, Default)]
struct UnitStates {
    restarts: BTreeMap<&'static str, u32>,
    failed_units: Vec<&'static str>,
}

impl UnitStates {
    fn of(text: &str) -> Self {
        Self {
            restarts: restarts(text),
            failed_units: failed_units(text),
        }
    }
}

/// `program args…`'s complete UTF-8 output, at most 256 KiB, within
/// [`SYSTEMCTL_WITHIN`], or nothing. Failed queries stop their owned group.
pub fn bounded_output(program: &Path, args: &[&str]) -> Option<String> {
    bounded_output_within(program, args, SYSTEMCTL_WITHIN)
}

/// [`bounded_output`] within `within`.
pub fn bounded_output_within(program: &Path, args: &[&str], within: Duration) -> Option<String> {
    let until = Instant::now().checked_add(within)?;
    let bytes = query::output_within(Command::new(program).args(args), until).ok()?;
    String::from_utf8(bytes).ok()
}

/// What the lamp's bus says now, for a health report.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    /// `lamp/health`, and whether it is past its lifespan.
    pub health: Option<(Health, bool)>,
    pub board: Option<BoardState>,
    pub online: Option<Online>,
    pub selftest: Option<SelfTest>,
    /// `systemctl show`'s output for the units.
    pub restarts: BTreeMap<&'static str, u32>,
    /// Units observed failed; canonicalized again by [`health_body`].
    pub failed_units: Vec<&'static str>,
    /// What waits ([`HealthBody::waits`]).
    pub waits: BTreeMap<&'static str, &'static str>,
}

/// The health part of a report, from what was seen: names by the lists only.
pub fn health_body(seen: &Seen) -> HealthBody {
    let mut failed_units: Vec<_> = seen
        .failed_units
        .iter()
        .filter_map(|name| unit(name))
        .collect();
    failed_units.sort_unstable();
    failed_units.dedup();
    let mut parts = Vec::new();
    let mut other_parts = 0u32;
    let mut missing = Vec::new();
    let level = match &seen.health {
        Some((health, expired)) => {
            for item in &health.items {
                match part(&item.name) {
                    Some(name) if parts.len() < MOST_PARTS => parts.push(PartLevel {
                        name,
                        level: item.level,
                        age_ms: item.age_ms,
                    }),
                    _ => other_parts += 1,
                }
            }
            for name in &health.missing {
                match part(name) {
                    Some(name) => missing.push(name),
                    None => other_parts += 1,
                }
            }
            if *expired { Level::Stale } else { health.level }
        }
        None => Level::Stale,
    };
    let board = seen.board.as_ref().map(|board| Board {
        cpu_load: rounded(board.cpu_load, 2),
        memory_used: rounded(board.memory_used, 3),
        disk_free: rounded(board.disk_free, 3),
        hottest_c: board
            .temperature_c
            .values()
            .copied()
            .filter(|c| c.is_finite())
            .reduce(f32::max)
            .and_then(|c| rounded(c, 1)),
        cpu_c: None,
        throttled: None,
        throttled_s: None,
        wifi_dbm: board.wifi_dbm,
    });
    let selftest = seen.selftest.as_ref().map(|test| {
        let mut failed = Vec::new();
        let mut other_failed = 0;
        let mut not_checked = 0;
        for result in &test.parts {
            match result.verdict {
                Verdict::Fail => match SELFTEST_PARTS.iter().find(|p| **p == result.part) {
                    Some(name) => failed.push(*name),
                    None => other_failed += 1,
                },
                Verdict::NotChecked => not_checked += 1,
                _ => {}
            }
        }
        SelfTestSummary {
            verdict: test.verdict,
            failed,
            other_failed,
            not_checked,
            build: test.build.as_deref().and_then(Build::of),
        }
    });
    HealthBody {
        level,
        parts,
        missing,
        other_parts,
        board,
        online: seen.online.as_ref().and_then(|online| online.online),
        down_for_s: seen.online.as_ref().and_then(|online| online.down_for_s),
        selftest,
        restarts: seen.restarts.clone(),
        failed_units,
        waits: seen.waits.clone(),
    }
}

/// The crash part of a report, from a note: `None` for a note that is no
/// crash or names no unit of [`UNITS`].
pub fn crash_body(note: &ExitNote, level_before: Option<Level>) -> Option<CrashBody> {
    let result = ExitResult::of(&note.result)?;
    let unit = unit(&note.unit)?;
    let (exit_code, signal) = match note.code.as_str() {
        "exited" => (note.status.trim().parse().ok(), None),
        "killed" | "dumped" => (None, signal_name(&note.status)),
        _ => (None, None),
    };
    Some(CrashBody {
        unit,
        result,
        exit_code,
        signal,
        ran_s: note.ran_s,
        restarts_since_boot: note.restarts,
        level_before,
        panic: (),
    })
}

/// A signal as systemd says it (`SEGV`, `SIGSEGV` or `11`), by the list.
fn signal_name(status: &str) -> Option<&'static str> {
    let status = status.trim();
    if let Ok(number) = status.parse::<usize>() {
        // Linux's numbering, for the list's signals.
        const BY_NUMBER: [(usize, &str); 16] = [
            (1, "SIGHUP"),
            (2, "SIGINT"),
            (3, "SIGQUIT"),
            (4, "SIGILL"),
            (5, "SIGTRAP"),
            (6, "SIGABRT"),
            (7, "SIGBUS"),
            (8, "SIGFPE"),
            (9, "SIGKILL"),
            (10, "SIGUSR1"),
            (11, "SIGSEGV"),
            (12, "SIGUSR2"),
            (13, "SIGPIPE"),
            (14, "SIGALRM"),
            (15, "SIGTERM"),
            (31, "SIGSYS"),
        ];
        return BY_NUMBER
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, name)| *name);
    }
    let bare = status.strip_prefix("SIG").unwrap_or(status);
    SIGNALS
        .iter()
        .copied()
        .find(|name| name.strip_prefix("SIG") == Some(bare))
}

/// The person's choice as kept: `share`, since when, and who made it,
/// with whether a voice's stop came from a held line (#1880, the first
/// builder on #1722, 18:44: a held stop is made visible, never silent).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Consent {
    pub share: bool,
    pub since: Option<String>,
    /// None for a choice kept before this was.
    pub by: Option<lamp_msgs::ReportsBy>,
    pub held: bool,
}

/// `by` as kept: only the three names.
fn by_of(value: &Value) -> Option<lamp_msgs::ReportsBy> {
    serde_json::from_value(value.clone()).ok()
}

/// A time this lamp wrote, as RFC 3339: digits, `-`, `:`, `T` and `Z` only.
fn when(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|at| {
            at.len() <= 25
                && at
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b"-:TZ".contains(&b))
        })
        .map(str::to_owned)
}

/// A JSON error's kind and place, never its message, which can quote a
/// value from the file (#2061's rule): `Data at line 1 column 15`, or the
/// kind alone when there is no place (a `Value` read again).
fn json_why(error: &serde_json::Error) -> String {
    if error.line() == 0 {
        format!("{:?}", error.classify())
    } else {
        format!(
            "{:?} at line {} column {}",
            error.classify(),
            error.line(),
            error.column()
        )
    }
}

/// A small JSON file, or nothing.
fn read_json(path: &Path) -> Option<Value> {
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(4_096)
        .read_to_string(&mut text)
        .ok()?;
    serde_json::from_str(&text).ok()
}

/// A choice file as read: none there, one this build can't read (and
/// why), or a JSON object with a `share` that is a boolean.
enum Choice {
    Absent,
    Unreadable(String),
    Object(serde_json::Map<String, Value>),
}

/// The fields `lamp-cloud`'s own record has (`choose` writes them).
const OWN_FIELDS: [&str; 4] = ["share", "at", "by", "held"];
/// The fields setup's record has (`lamp-setup`'s `reports_record`).
const SETUP_FIELDS: [&str; 3] = ["share", "asked", "at"];

/// A choice file, read whole: only a JSON object whose `share` is a boolean
/// is one this build can read. A file that can't be opened for any reason
/// but its absence is unreadable, never absent (#2052): an own no that
/// can't be read must not let setup's older yes through.
fn read_choice(path: &Path) -> Choice {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Choice::Absent,
        Err(error) => return Choice::Unreadable(error.to_string()),
    };
    let mut text = String::new();
    if let Err(error) = file.take(4_097).read_to_string(&mut text) {
        return Choice::Unreadable(error.to_string());
    }
    if text.len() > 4_096 {
        return Choice::Unreadable("over 4 KiB".into());
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(choice)) if choice.get("share").is_some_and(Value::is_boolean) => {
            Choice::Object(choice)
        }
        Ok(_) => Choice::Unreadable("not an object with a boolean `share`".into()),
        Err(error) => Choice::Unreadable(json_why(&error)),
    }
}

/// The first field of `choice` this build doesn't know, if any.
fn unknown_field<'a>(
    choice: &'a serde_json::Map<String, Value>,
    known: &[&str],
) -> Option<&'a str> {
    choice
        .keys()
        .map(String::as_str)
        .find(|field| !known.contains(field))
}

/// The person's choice: `lamp-cloud`'s own record when there is one, else
/// setup's, else no. Only a JSON `true` is a yes, and setup's only for a
/// wording no newer than [`ASKED`]. Read at each send, never kept.
///
/// One release forward (#2052), for privacy: a record this build can't
/// read, a field it doesn't know or a `by` it doesn't know reads as **no**,
/// whatever its `share` says, so a newer build's field can never turn
/// sharing on here. Such a record is left as it is (a roll forward reads it
/// again); the line that says so comes back beside the choice.
fn read_consent(places: &Places) -> (Consent, Option<String>) {
    match read_choice(&places.own) {
        Choice::Absent => {}
        Choice::Unreadable(why) => {
            return (
                Consent::default(),
                Some(format!(
                    "the reports choice at {} can't be read ({why}): reports are off",
                    places.own.display()
                )),
            );
        }
        Choice::Object(own) => {
            let unknown = unknown_field(&own, &OWN_FIELDS).map(|field| format!("field `{field}`"));
            // As a `Value`, a field that isn't there is null.
            let own = Value::Object(own);
            let by = by_of(&own["by"]);
            let newer = unknown
                .or_else(|| {
                    (!own["by"].is_null() && by.is_none())
                        .then(|| "a `by` this build doesn't know".to_owned())
                })
                .or_else(|| {
                    (!own["held"].is_null() && !own["held"].is_boolean())
                        .then(|| "a `held` that isn't a boolean".to_owned())
                });
            let consent = Consent {
                share: newer.is_none() && own["share"] == Value::Bool(true),
                since: when(&own["at"]),
                held: by == Some(lamp_msgs::ReportsBy::Voice) && own["held"] == Value::Bool(true),
                by,
            };
            let note = newer.map(|what| {
                format!(
                    "the reports choice at {} is a newer build's ({what}): read as off, and kept",
                    places.own.display()
                )
            });
            return (consent, note);
        }
    }
    match read_choice(&places.setup) {
        Choice::Absent => (Consent::default(), None),
        Choice::Unreadable(why) => (
            Consent::default(),
            Some(format!(
                "setup's reports answer at {} can't be read ({why}): reports are off, and it is left there",
                places.setup.display()
            )),
        ),
        Choice::Object(setup) => {
            if let Some(field) = unknown_field(&setup, &SETUP_FIELDS) {
                return (
                    Consent::default(),
                    Some(format!(
                        "setup's reports answer at {} is a newer build's (field `{field}`): read as off, and left there",
                        places.setup.display()
                    )),
                );
            }
            let setup = Value::Object(setup);
            // A later wording (`asked` above ASKED) is a no until the person
            // is asked again, as designed: said by nothing.
            if setup["share"] == Value::Bool(true)
                && setup["asked"].as_u64().is_some_and(|asked| asked <= ASKED)
            {
                (
                    Consent {
                        share: true,
                        since: when(&setup["at"]),
                        by: Some(lamp_msgs::ReportsBy::Setup),
                        held: false,
                    },
                    None,
                )
            } else {
                (Consent::default(), None)
            }
        }
    }
}

/// [`read_consent`]'s choice, without its line.
pub fn consent(places: &Places) -> Consent {
    read_consent(places).0
}

/// Where a file this build can't read is kept whole: `<name>.unread-<s>`,
/// a name no reader here takes for its own.
fn set_aside(path: &Path, now_s: u64) -> io::Result<PathBuf> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".unread-{now_s}"));
    let aside = path.with_file_name(name);
    fs::rename(path, &aside)?;
    Ok(aside)
}

/// `bytes` at `path` with `mode`: a temporary file, synced, renamed, the
/// folder synced.
fn write_whole(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    let _ = fs::remove_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// One report waiting.
#[derive(Clone, Debug)]
struct Entry {
    seq: u64,
    kind: Kind,
    at_s: u64,
    bytes: String,
    /// Handed to the connection and not yet acknowledged.
    handed: bool,
    /// A `stopped` acknowledged once, waiting to go once more on the next
    /// connection (the backend, #1683: an ack is the broker's, not the
    /// store's, and a no lost there would last 90 days).
    once_more: bool,
    /// A `stopped` acknowledged twice is through.
    acks: u8,
}

/// The state the queue keeps beside the crashes. A field a newer build
/// adds is read and dropped at the next save; one this build has that a
/// record lacks is its default (#2052).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Kept {
    seq: u64,
    dropped: Dropped,
    /// The host's boot the notes below were on.
    boot: String,
    /// Each unit's newest note taken, on its monotonic clock.
    notes: BTreeMap<String, u64>,
    /// The crashes taken, wall seconds, for the rate.
    crashes: VecDeque<u64>,
}

/// The reports' engine: what waits, the counts and the bounds. Nothing in
/// it sends; the connection asks it for what is due.
pub struct Reports {
    places: Places,
    kept: Kept,
    queue: Vec<Entry>,
    /// Each part's newest level, for a crash's `level_before`.
    levels: BTreeMap<&'static str, Level>,
    /// Each part's `error` since, and its last report at once (wall s).
    error_since: BTreeMap<&'static str, u64>,
    error_told: BTreeMap<&'static str, u64>,
    /// Reports a newer build queued that this build doesn't send
    /// ([`Queued::Later`]): kept for it, and deleted by a no.
    later: Vec<u64>,
    /// The journal lines said, each once.
    said: RefCell<VecDeque<String>>,
}

/// The most journal lines remembered, so that each is said once.
const SAID_MOST: usize = 256;

/// One file of the queue folder, as read.
enum Queued {
    /// One this build sends.
    Sendable(Kind, u64, String),
    /// A newer build's: a `schema` past [`SCHEMA`], or a `kind` this build
    /// doesn't have. Kept where it is and not sent: the backend has agreed
    /// only [`SCHEMA`]'s kinds from this build, and a report it can't take
    /// would be acknowledged by the broker and lost all the same.
    Later(String),
    /// Not a report this build can read: set aside, never sent.
    Unreadable(String),
}

/// A queued report's file, `bytes` as read (`None` past [`MOST_BYTES`]).
fn queued(bytes: Option<&str>) -> Queued {
    let Some(bytes) = bytes else {
        return Queued::Unreadable(format!("over {MOST_BYTES} bytes"));
    };
    let report = match serde_json::from_str::<Value>(bytes) {
        Ok(Value::Object(report)) => report,
        Ok(_) => return Queued::Unreadable("not a JSON object".into()),
        Err(error) => return Queued::Unreadable(json_why(&error)),
    };
    match report.get("schema").and_then(Value::as_str) {
        Some(SCHEMA) => {}
        Some(schema)
            if schema
                .strip_prefix("lamp-report/")
                .and_then(|n| n.parse::<u64>().ok())
                .is_some_and(|n| n > 1) =>
        {
            return Queued::Later(format!("a schema past {SCHEMA}"));
        }
        _ => return Queued::Unreadable("no schema this build knows".into()),
    }
    let kind = match report.get("kind").and_then(Value::as_str) {
        Some("crash") => Kind::Crash,
        Some("stopped") => Kind::Stopped,
        Some(_) => return Queued::Later("a kind this build doesn't have".into()),
        None => return Queued::Unreadable("no kind".into()),
    };
    match report
        .get("sent_at")
        .and_then(Value::as_str)
        .and_then(seconds_of)
    {
        Some(at_s) => Queued::Sendable(kind, at_s, bytes.to_owned()),
        None => Queued::Unreadable("no `sent_at`".into()),
    }
}

impl Reports {
    /// The engine, with what the queue folder kept.
    pub fn open(places: Places) -> Self {
        let mut reports = Self {
            places,
            kept: Kept::default(),
            queue: Vec::new(),
            levels: BTreeMap::new(),
            error_since: BTreeMap::new(),
            error_told: BTreeMap::new(),
            later: Vec::new(),
            said: RefCell::new(VecDeque::new()),
        };
        let now = now_s();
        reports.own_choice(now);
        reports.kept = reports.kept_state(now);
        let mut highest = 0;
        if let Ok(entries) = fs::read_dir(&reports.places.queue) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(seq) = name
                    .to_str()
                    .and_then(|name| name.strip_suffix(".json"))
                    .and_then(|seq| seq.parse::<u64>().ok())
                else {
                    continue;
                };
                highest = highest.max(seq);
                let mut bytes = String::new();
                let read = fs::File::open(entry.path())
                    .and_then(|file| file.take(MOST_BYTES as u64 + 1).read_to_string(&mut bytes));
                let read = match read {
                    Ok(_) => queued((bytes.len() <= MOST_BYTES).then_some(bytes.as_str())),
                    Err(error) => Queued::Unreadable(error.to_string()),
                };
                match read {
                    Queued::Sendable(kind, at_s, bytes) => reports.queue.push(Entry {
                        seq,
                        kind,
                        at_s,
                        bytes,
                        handed: false,
                        once_more: false,
                        acks: 0,
                    }),
                    Queued::Later(what) => {
                        reports.later.push(seq);
                        reports.say(format!(
                            "queued report {} is a newer build's ({what}): kept, not sent",
                            entry.path().display()
                        ));
                    }
                    Queued::Unreadable(why) => reports.put_aside(&entry.path(), &why, now),
                }
            }
        }
        // Past every report on flash, even when the state was lost.
        reports.kept.seq = reports.kept.seq.max(highest);
        reports.queue.sort_by_key(|entry| entry.seq);
        reports
    }

    /// A line for the journal, said once.
    fn say(&self, line: String) {
        let mut said = self.said.borrow_mut();
        if said.contains(&line) {
            return;
        }
        eprintln!("lamp-cloud reports: {line}");
        if said.len() >= SAID_MOST {
            said.pop_front();
        }
        said.push_back(line);
    }

    /// The lines said, oldest first.
    pub fn said(&self) -> Vec<String> {
        self.said.borrow().iter().cloned().collect()
    }

    /// A file of the queue folder this build can't read, kept whole under
    /// another name, with a line.
    fn put_aside(&self, path: &Path, why: &str, now_s: u64) {
        match set_aside(path, now_s) {
            Ok(aside) => self.say(format!(
                "{} can't be read ({why}): kept as {}, not sent",
                path.display(),
                aside.display()
            )),
            Err(error) => self.say(format!(
                "{} can't be read ({why}), nor set aside ({error}): not sent",
                path.display()
            )),
        }
    }

    /// `lamp-cloud`'s own choice, when this build can't read it at all: set
    /// aside whole and an off kept in its place, said once. Off, since
    /// setup's older yes must not come back through the gap (#2052).
    fn own_choice(&self, now_s: u64) {
        let Choice::Unreadable(why) = read_choice(&self.places.own) else {
            return;
        };
        let off = json!({
            "share": false,
            "at": crate::online::rfc3339(UNIX_EPOCH + Duration::from_secs(now_s)),
            "by": null,
            "held": false,
        });
        let line = match set_aside(&self.places.own, now_s) {
            Ok(aside) => match write_whole(&self.places.own, off.to_string().as_bytes(), 0o600) {
                Ok(()) => format!(
                    "the reports choice at {} can't be read ({why}): kept as {}, and reports are off until the person says yes",
                    self.places.own.display(),
                    aside.display()
                ),
                // Gone and not replaced: put back, so that it still reads off.
                Err(error) => {
                    let _ = fs::rename(&aside, &self.places.own);
                    format!(
                        "the reports choice at {} can't be read ({why}), nor replaced ({error}): reports are off",
                        self.places.own.display()
                    )
                }
            },
            Err(error) => format!(
                "the reports choice at {} can't be read ({why}), nor set aside ({error}): reports are off",
                self.places.own.display()
            ),
        };
        self.say(line);
    }

    /// The queue's state, or a new one: one this build can't read is set
    /// aside whole with a line.
    fn kept_state(&self, now_s: u64) -> Kept {
        let path = self.places.queue.join("state.json");
        let mut text = String::new();
        match fs::File::open(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Kept::default(),
            Err(error) => {
                self.say(format!(
                    "{} can't be opened ({error}): begun again",
                    path.display()
                ));
                return Kept::default();
            }
            Ok(file) => {
                if let Err(error) = file.take(QUEUE_BYTES as u64).read_to_string(&mut text) {
                    self.put_aside(&path, &error.to_string(), now_s);
                    return Kept::default();
                }
            }
        }
        match serde_json::from_str::<Kept>(&text) {
            Ok(kept) => kept,
            Err(error) => {
                self.put_aside(&path, &json_why(&error), now_s);
                Kept::default()
            }
        }
    }

    /// Whether a no now has something to stop: a yes stands, or the
    /// record read is one this build can't settle (a newer build's or an
    /// unreadable one), which reads as off here but may be a yes to the
    /// build that wrote it. A no then is written and does all a no does
    /// (#2077's review), never taken for one already given.
    fn may_share(&self) -> bool {
        let (consent, line) = read_consent(&self.places);
        let unsettled = line.is_some();
        if let Some(line) = line {
            self.say(line);
        }
        consent.share || unsettled
    }

    /// The person's choice now.
    pub fn consent(&self) -> Consent {
        let (consent, line) = read_consent(&self.places);
        if let Some(line) = line {
            self.say(line);
        }
        consent
    }

    /// What `reports/state` says: whether they are sent, how many wait,
    /// and how and when the newest choice was made.
    pub fn state(&self) -> lamp_msgs::ReportsState {
        let consent = self.consent();
        lamp_msgs::ReportsState {
            share: consent.share,
            queued: self.queue.len() as u32,
            by: consent.by,
            held: consent.held,
            at_s: consent.since.as_deref().and_then(seconds_of),
        }
    }

    /// `reports.get`'s and `reports.set`'s answer: with `by` (`voice`,
    /// `app`, `setup`, or null), `held` and `at`, when the newest choice
    /// was made, so the app's row can say "Stopped by voice at 3 pm".
    pub fn answer(&self) -> Value {
        let consent = self.consent();
        json!({
            "share": consent.share,
            "since": consent.since,
            "queued": self.queue.len(),
            "by": consent.by,
            "held": consent.held,
            "at": consent.since,
        })
    }

    fn keep(&self) {
        if fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.places.queue)
            .is_ok()
            && let Ok(bytes) = serde_json::to_vec(&self.kept)
        {
            let _ = write_whole(&self.places.queue.join("state.json"), &bytes, 0o600);
        }
    }

    /// The app's `reports.set`: the person's own choice, from their
    /// account. A no deletes what waits and sends the last word.
    pub fn set(&mut self, share: bool, now_s: u64) -> io::Result<()> {
        self.choose(share, now_s, lamp_msgs::ReportsBy::App, false)
    }

    /// A choice kept with who made it and when, and for the voice's stop
    /// whether its line was held.
    fn choose(
        &mut self,
        share: bool,
        now_s: u64,
        by: lamp_msgs::ReportsBy,
        held: bool,
    ) -> io::Result<()> {
        let at = crate::online::rfc3339(UNIX_EPOCH + Duration::from_secs(now_s));
        let was = self.may_share();
        write_whole(
            &self.places.own,
            json!({"share": share, "at": at, "by": by, "held": held})
                .to_string()
                .as_bytes(),
            0o600,
        )?;
        if was && !share {
            self.stopped(now_s);
        }
        if share {
            // A yes again: a `stopped` already through once must not come
            // after the new reports and delete them.
            let through: Vec<u64> = self
                .queue
                .iter()
                .filter(|entry| entry.acks > 0)
                .map(|entry| entry.seq)
                .collect();
            for seq in through {
                self.forget(seq);
            }
        }
        Ok(())
    }

    /// A no from the bus (`reports/request`, the voice): the same as the
    /// app's, and only ever a no, kept as the voice's, with whether its
    /// line was held. When reports are already off it changes nothing, so
    /// the stop that stopped them is the one kept; a newer build's or an
    /// unreadable record is not an off already given, so the no is kept.
    pub fn stop(&mut self, now_s: u64, held: bool) -> io::Result<()> {
        if !self.may_share() {
            return Ok(());
        }
        self.choose(false, now_s, lamp_msgs::ReportsBy::Voice, held)
    }

    /// Everything waiting deleted, its files too, and one `stopped` put in
    /// their place: written to flash, kept until it is through, and sent
    /// once more on the next connection. Nothing else is sent after it.
    fn stopped(&mut self, now_s: u64) {
        let later = std::mem::take(&mut self.later);
        for seq in self.queue.drain(..).map(|entry| entry.seq).chain(later) {
            let _ = fs::remove_file(self.places.queue.join(format!("{seq}.json")));
        }
        self.error_since.clear();
        let report = self.report(now_s, None, &Seen::default(), Body::Stopped);
        self.put(report);
    }

    fn report(&mut self, now_s: u64, build: Option<&Build>, seen: &Seen, body: Body) -> Report {
        self.kept.seq += 1;
        Report {
            kind_of_message: "report",
            schema: SCHEMA,
            device: DEVICE,
            generation: GENERATION,
            seq: self.kept.seq,
            sent_at: crate::online::rfc3339(UNIX_EPOCH + Duration::from_secs(now_s)),
            clock_synced: seen.board.as_ref().and_then(|board| board.clock_synced),
            build: build.cloned(),
            uptime_s: seen.board.as_ref().map_or(0, |board| board.uptime_s),
            dropped: self.kept.dropped,
            body,
        }
    }

    /// Puts a report in the queue, bounded; a crash written to flash too.
    /// Nothing without the person's yes.
    fn put(&mut self, report: Report) -> Option<u64> {
        let kind = report.body.kind();
        let Ok(bytes) = serde_json::to_string(&report) else {
            return None;
        };
        if bytes.len() + ID_MOST > MOST_BYTES {
            self.kept.dropped.too_big += 1;
            self.keep();
            return None;
        }
        // `sent_at` is set here, once, and kept on every send again: the
        // backend drops a duplicate by `(device, seq, sent_at)`.
        let at_s = seconds_of(&report.sent_at).unwrap_or_default();
        if kind == Kind::Health {
            // Only the newest health waits, in memory.
            self.queue
                .retain(|entry| entry.kind != Kind::Health || entry.handed);
        } else if fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.places.queue)
            .is_ok()
        {
            let _ = write_whole(
                &self.places.queue.join(format!("{}.json", report.seq)),
                bytes.as_bytes(),
                0o600,
            );
        }
        self.queue.push(Entry {
            seq: report.seq,
            kind,
            at_s,
            bytes,
            handed: false,
            once_more: false,
            acks: 0,
        });
        self.bound(at_s);
        self.keep();
        Some(report.seq)
    }

    /// The queue in its bounds: nothing older than [`KEPT_FOR`]; past
    /// [`QUEUE_MOST`] or [`QUEUE_BYTES`] the oldest health goes first, and
    /// a crash only when nothing but crashes is left.
    fn bound(&mut self, now_s: u64) {
        let oldest = now_s.saturating_sub(KEPT_FOR.as_secs());
        let mut gone: Vec<u64> = self
            .queue
            .iter()
            .filter(|entry| entry.at_s < oldest && entry.kind != Kind::Stopped)
            .map(|entry| entry.seq)
            .collect();
        self.queue.retain(|entry| !gone.contains(&entry.seq));
        loop {
            let bytes: usize = self.queue.iter().map(|entry| entry.bytes.len()).sum();
            if self.queue.len() <= QUEUE_MOST && bytes <= QUEUE_BYTES {
                break;
            }
            let at = self
                .queue
                .iter()
                .position(|entry| entry.kind == Kind::Health)
                .or_else(|| {
                    self.queue
                        .iter()
                        .position(|entry| entry.kind == Kind::Crash)
                });
            let Some(at) = at else { break };
            gone.push(self.queue.remove(at).seq);
            self.kept.dropped.queue_full += 1;
        }
        for seq in gone {
            let _ = fs::remove_file(self.places.queue.join(format!("{seq}.json")));
        }
    }

    /// A health report, when the person's yes stands.
    pub fn health(&mut self, now_s: u64, build: Option<&Build>, seen: &Seen) -> Option<u64> {
        if !self.consent().share {
            return None;
        }
        self.look(now_s, seen);
        let body = Body::Health(Box::new(health_body(seen)));
        let report = self.report(now_s, build, seen, body);
        self.put(report)
    }

    /// The parts' levels from `seen`, for `level_before`, and whether one
    /// has been at `error` for [`ERROR_HOLDS`] and was not told of within
    /// [`ERROR_AGAIN`]: then a health report is due at once.
    pub fn look(&mut self, now_s: u64, seen: &Seen) -> bool {
        let Some((health, false)) = &seen.health else {
            return false;
        };
        let mut due = false;
        let mut erring = Vec::new();
        for item in &health.items {
            let Some(name) = part(&item.name) else {
                continue;
            };
            self.levels.insert(name, item.level);
            if item.level == Level::Error {
                erring.push(name);
                let since = *self.error_since.entry(name).or_insert(now_s);
                let told = self.error_told.get(name).copied();
                if now_s.saturating_sub(since) >= ERROR_HOLDS.as_secs()
                    && told.is_none_or(|told| now_s.saturating_sub(told) >= ERROR_AGAIN.as_secs())
                {
                    due = true;
                }
            }
        }
        self.error_since.retain(|name, _| erring.contains(name));
        if due {
            for name in erring {
                if now_s.saturating_sub(self.error_since[name]) >= ERROR_HOLDS.as_secs() {
                    self.error_told.insert(name, now_s);
                }
            }
        }
        due
    }

    /// A crash from an exit note, when the person's yes stands, within the
    /// rate.
    pub fn crash(&mut self, now_s: u64, build: Option<&Build>, note: &ExitNote) -> Option<u64> {
        if !self.consent().share {
            return None;
        }
        let level_before = part_of(&note.unit).and_then(|name| self.levels.get(name).copied());
        let body = crash_body(note, level_before)?;
        let hour = now_s.saturating_sub(3_600);
        let day = now_s.saturating_sub(86_400);
        self.kept.crashes.retain(|at| *at >= day);
        let in_hour = self.kept.crashes.iter().filter(|at| **at >= hour).count();
        if in_hour >= CRASHES_PER_HOUR || self.kept.crashes.len() >= CRASHES_PER_DAY {
            self.kept.dropped.over_rate += 1;
            self.keep();
            return None;
        }
        self.kept.crashes.push_back(now_s);
        let report = self.report(now_s, build, &Seen::default(), Body::Crash(body));
        self.put(report)
    }

    /// The notes in the exit folder not yet taken, oldest first, each taken
    /// now. A new boot starts the count again (the folder is in `/run`).
    pub fn new_notes(&mut self) -> Vec<ExitNote> {
        let boot = fs::read_to_string(&self.places.boot_id)
            .map(|boot| boot.trim().to_owned())
            .unwrap_or_default();
        if boot != self.kept.boot {
            self.kept.boot = boot.clone();
            self.kept.notes.clear();
        }
        // Only `<unit>@<at>.json`: `write_whole`'s `.tmp` is not a note yet.
        // A note this build can't read is skipped with a line, once, and
        // left whole where it is: the folder is root's, which `lamp-cloud`
        // can't write, and `/run` goes at the next boot.
        let mut notes: Vec<ExitNote> = fs::read_dir(&self.places.exits)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
            .filter_map(|entry| {
                let path = entry.path();
                let note = read_json(&path).ok_or_else(|| "not JSON".to_owned());
                match note.and_then(|note| {
                    serde_json::from_value::<ExitNote>(note).map_err(|error| json_why(&error))
                }) {
                    Ok(note) => Some(note),
                    // Pruned by the next `exit-note` while this read.
                    Err(_) if !path.exists() => None,
                    Err(why) => {
                        self.say(format!(
                            "exit note {} can't be read ({why}): skipped, and left there",
                            path.display()
                        ));
                        None
                    }
                }
            })
            .filter(|note| note.boot == boot)
            .filter(|note| {
                self.kept
                    .notes
                    .get(&note.unit)
                    .is_none_or(|newest| note.at_us > *newest)
            })
            .collect();
        notes.sort_by_key(|note| note.at_us);
        for note in &notes {
            self.kept.notes.insert(note.unit.clone(), note.at_us);
        }
        if !notes.is_empty() {
            self.keep();
        }
        notes
    }

    /// What is due to be handed to the connection, in order, each marked
    /// handed, each with the device's `id` as `info` carries it (the
    /// backend drops a message without one). Nothing without the person's
    /// yes but the last `stopped`; nothing for an id that is not one.
    pub fn due(&mut self, id: &str) -> Vec<(u64, String)> {
        if id.is_empty() || id.len() > ID_MOST || !id.bytes().all(|b| b.is_ascii_graphic()) {
            return Vec::new();
        }
        let share = self.consent().share;
        self.queue
            .iter_mut()
            .filter(|entry| {
                !entry.handed && !entry.once_more && (share || entry.kind == Kind::Stopped)
            })
            .filter_map(|entry| {
                let mut report: serde_json::Map<String, Value> =
                    serde_json::from_str(&entry.bytes).ok()?;
                report.insert("id".into(), json!(id));
                entry.handed = true;
                Some((entry.seq, Value::Object(report).to_string()))
            })
            .collect()
    }

    /// The broker acknowledged `seq`: it goes. A `stopped` goes after its
    /// second time through, on a later connection.
    pub fn acked(&mut self, seq: u64) {
        let Some(entry) = self.queue.iter_mut().find(|entry| entry.seq == seq) else {
            return;
        };
        entry.acks += 1;
        if entry.kind == Kind::Stopped && entry.acks < 2 {
            entry.once_more = true;
            entry.handed = false;
            return;
        }
        self.forget(seq);
    }

    fn forget(&mut self, seq: u64) {
        self.queue.retain(|entry| entry.seq != seq);
        let _ = fs::remove_file(self.places.queue.join(format!("{seq}.json")));
    }

    /// A new connection: a `stopped` through once goes once more on it.
    pub fn connected(&mut self) {
        for entry in &mut self.queue {
            entry.once_more = false;
        }
    }

    /// The connection went before acknowledging: everything handed is sent
    /// again, with the same `seq`, once it is back.
    pub fn unhand(&mut self, seq: Option<u64>) {
        for entry in &mut self.queue {
            if seq.is_none_or(|seq| seq == entry.seq) {
                entry.handed = false;
            }
        }
    }

    /// How many wait.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// The counts.
    pub fn dropped(&self) -> Dropped {
        self.kept.dropped
    }
}

/// Wall seconds of an RFC 3339 UTC time `rfc3339` wrote.
fn seconds_of(at: &str) -> Option<u64> {
    let digits = |range: std::ops::Range<usize>| at.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    // Howard Hinnant's days-from-civil.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hour * 3_600 + minute * 60 + second).ok()
}

/// Wall seconds now.
pub fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The engine shared by the connection, the answers and the driver, with a
/// wake for the connection when something is due.
pub struct Shared {
    pub reports: Mutex<Reports>,
    pub wake: tokio::sync::Notify,
}

impl Shared {
    pub fn new(reports: Reports) -> Arc<Self> {
        Arc::new(Self {
            reports: Mutex::new(reports),
            wake: tokio::sync::Notify::new(),
        })
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut Reports) -> T) -> T {
        let mut reports = self
            .reports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut reports)
    }
}

/// `exit-note UNIT`: what systemd handed `ExecStopPost=` about one stop,
/// kept in `exits` as `<unit>@<at_us>.json` (0644), the newest
/// [`NOTES_PER_UNIT`] per unit. A `success` writes nothing. No journal is
/// read: only systemd's three variables, the unit's name and `systemctl
/// show`'s numbers.
pub fn write_exit_note(exits: &Path, note: &ExitNote) -> io::Result<PathBuf> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(exits)?;
    let path = exits.join(format!("{}@{}.json", note.unit, note.at_us));
    write_whole(&path, &serde_json::to_vec(note)?, 0o644)?;
    let prefix = format!("{}@", note.unit);
    let mut times: Vec<(u64, PathBuf)> = fs::read_dir(exits)?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let at = name
                .strip_prefix(&prefix)?
                .strip_suffix(".json")?
                .parse()
                .ok()?;
            Some((at, entry.path()))
        })
        .collect();
    times.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    for (_, old) in times.into_iter().skip(NOTES_PER_UNIT) {
        let _ = fs::remove_file(old);
    }
    Ok(path)
}

/// What the driver reads from the lamp.
pub trait Sources {
    /// The bus's values now, without the systemd observations.
    fn seen(&mut self) -> Seen;
    /// The units' restarts since boot, asked of systemd.
    fn restarts(&mut self) -> BTreeMap<&'static str, u32> {
        BTreeMap::new()
    }
    /// Units still failed, from the same observation as the restart counts.
    fn failed_units(&mut self) -> Vec<&'static str> {
        Vec::new()
    }
    /// Restart counts and failures for one health report. The lamp reads
    /// one cached snapshot, even at its expiry boundary; other sources
    /// retain the defaults or their existing field implementations.
    fn unit_health(&mut self) -> (BTreeMap<&'static str, u32>, Vec<&'static str>) {
        (self.restarts(), self.failed_units())
    }
    /// The units that wait, and why ([`HealthBody::waits`]).
    fn waits(&mut self) -> BTreeMap<&'static str, &'static str> {
        BTreeMap::new()
    }
    /// Whether the voice is resting (not in a turn); true when unknown.
    fn voice_resting(&mut self) -> bool;
    /// Asks taken from `reports/request` since the last look.
    fn requests(&mut self) -> Vec<lamp_msgs::ReportsRequest>;
    /// Says `reports/state`.
    fn tell(&mut self, state: &lamp_msgs::ReportsState);
}

/// The driver's clock: when each thing is next due.
#[derive(Clone, Debug)]
pub struct Timing {
    pub started: Instant,
    pub next_health: Instant,
    pub next_notes: Instant,
    pub next_state: Instant,
    /// A health waiting on the voice's turn, since.
    pub waiting_since: Option<Instant>,
}

impl Timing {
    pub fn new(now: Instant) -> Self {
        Self {
            started: now,
            next_health: now + FIRST_HEALTH,
            next_notes: now,
            next_state: now,
            waiting_since: None,
        }
    }
}

/// One second of the driver: asks to stop taken, the state said, notes
/// read, a health built when due. Returns whether something was put in the
/// queue, for the connection to be woken.
pub fn step(
    shared: &Shared,
    sources: &mut impl Sources,
    timing: &mut Timing,
    build: Option<&Build>,
    now: Instant,
    now_s: u64,
) -> bool {
    let mut put = false;
    // Only a no is ever taken from the bus. It is kept as held only when
    // every no taken was from a held line: one heard is the person's.
    let noes: Vec<_> = sources
        .requests()
        .into_iter()
        .filter(|request| !request.share)
        .collect();
    if !noes.is_empty() {
        let held = noes.iter().all(|request| request.held);
        let stopped = shared.with(|reports| {
            let was = reports.consent().share;
            let _ = reports.stop(now_s, held);
            was
        });
        put |= stopped;
        timing.next_state = now;
    }
    if now >= timing.next_notes {
        timing.next_notes = now + NOTES_EVERY;
        put |= shared.with(|reports| {
            let mut put = false;
            for note in reports.new_notes() {
                put |= reports.crash(now_s, build, &note).is_some();
            }
            put
        });
    }
    // Nothing is read without the person's yes.
    let share = shared.with(|reports| reports.consent().share);
    let mut seen = if share {
        sources.seen()
    } else {
        Seen::default()
    };
    let urgent = share && shared.with(|reports| reports.look(now_s, &seen));
    if !share && now >= timing.next_health {
        timing.next_health = now + HEALTH_EVERY;
    }
    if share && (now >= timing.next_health || urgent) {
        // A send waits while the voice is in a turn, up to a minute.
        let waited = timing.waiting_since.get_or_insert(now).elapsed_at(now);
        if sources.voice_resting() || waited >= VOICE_WAIT {
            timing.waiting_since = None;
            if now >= timing.next_health {
                timing.next_health = now + HEALTH_EVERY;
            }
            (seen.restarts, seen.failed_units) = sources.unit_health();
            seen.waits = sources.waits();
            put |= shared
                .with(|reports| reports.health(now_s, build, &seen))
                .is_some();
        }
    }
    if put || now >= timing.next_state {
        timing.next_state = now + Duration::from_secs(5);
        let state = shared.with(|reports| reports.state());
        sources.tell(&state);
    }
    put
}

trait ElapsedAt {
    fn elapsed_at(&self, now: Instant) -> Duration;
}

impl ElapsedAt for Instant {
    fn elapsed_at(&self, now: Instant) -> Duration {
        now.saturating_duration_since(*self)
    }
}

/// The driver: [`step`] once a second, forever, waking the connection
/// when something is due.
pub async fn run(shared: Arc<Shared>, mut sources: impl Sources, build: Option<Build>) {
    let mut timing = Timing::new(Instant::now());
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if step(
            &shared,
            &mut sources,
            &mut timing,
            build.as_ref(),
            Instant::now(),
            now_s(),
        ) {
            shared.wake.notify_one();
        }
    }
}

/// The lamp's bus and systemd, as the driver reads them.
/// The name `lamp-cloud` takes `reports/request` under.
const REQUESTS_NAME: &str = "cloud_reports";

pub struct Lamp {
    pub bus: Option<lamp_bus::Bus>,
    pub systemctl: PathBuf,
    /// How long each systemctl query has: [`SYSTEMCTL_WITHIN`]. A test
    /// whose stand-in shell may take longer on a loaded machine gives it
    /// more, so a late start isn't read as a failed query (#1569).
    pub systemctl_within: Duration,
    /// [`UPDATE_SELF_ENVIRONMENT`] is `on`.
    pub update_self: bool,
    requests: Option<lamp_bus::Subscriber>,
    requests_refused: bool,
    state: Option<lamp_bus::Publisher>,
    units_read: Option<(Instant, UnitStates)>,
}

impl Lamp {
    pub fn new(bus: Option<lamp_bus::Bus>) -> Self {
        Self {
            bus,
            systemctl: "/usr/bin/systemctl".into(),
            systemctl_within: SYSTEMCTL_WITHIN,
            update_self: switched_on(std::env::var(UPDATE_SELF_ENVIRONMENT).ok().as_deref()),
            requests: None,
            requests_refused: false,
            state: None,
            units_read: None,
        }
    }

    fn latest<T: serde::de::DeserializeOwned>(&self, topic: &str) -> Option<(T, bool)> {
        let message = self.bus.as_ref()?.latest(topic).ok().flatten()?;
        Some((message.parse().ok()?, message.expired))
    }

    /// Both fields share the existing one-minute success/failure cache and
    /// bounded systemctl query. No second query is added for failed units.
    fn unit_states(&mut self) -> UnitStates {
        if let Some((at, found)) = &self.units_read
            && at.elapsed() < Duration::from_secs(60)
        {
            return found.clone();
        }
        let mut args = vec!["show", "-p", "Id", "-p", "NRestarts", "-p", "ActiveState"];
        args.extend(UNITS.iter().copied());
        let found = bounded_output_within(&self.systemctl, &args, self.systemctl_within)
            .map(|text| UnitStates::of(&text))
            .unwrap_or_default();
        self.units_read = Some((Instant::now(), found.clone()));
        found
    }
}

impl Sources for Lamp {
    fn seen(&mut self) -> Seen {
        Seen {
            health: self.latest(Health::TOPIC),
            board: self
                .latest::<BoardState>(BoardState::TOPIC)
                .filter(|(_, expired)| !expired)
                .map(|(board, _)| board),
            online: self
                .latest::<Online>(Online::TOPIC)
                .filter(|(_, expired)| !expired)
                .map(|(online, _)| online),
            selftest: self
                .latest::<SelfTest>(SelfTest::TOPIC)
                .map(|(test, _)| test),
            restarts: BTreeMap::new(),
            failed_units: Vec::new(),
            waits: BTreeMap::new(),
        }
    }

    /// Only with the self-update switch on, asked of systemd within
    /// [`SYSTEMCTL_WITHIN`]; a query that fails says nothing.
    fn waits(&mut self) -> BTreeMap<&'static str, &'static str> {
        let mut waits = BTreeMap::new();
        if self.update_self
            && bounded_output_within(
                &self.systemctl,
                &[
                    "show",
                    "-p",
                    "LoadState",
                    "-p",
                    "UnitFileState",
                    "-p",
                    "ActiveState",
                    RECOVERY_TIMER,
                ],
                self.systemctl_within,
            )
            .is_some_and(|show| update_waits(&show))
        {
            waits.insert("lamp-update.service", RECOVERY_NOT_READY);
        }
        waits
    }

    /// Asked of systemd at most once a minute, within
    /// [`SYSTEMCTL_WITHIN`].
    fn restarts(&mut self) -> BTreeMap<&'static str, u32> {
        self.unit_states().restarts
    }

    fn failed_units(&mut self) -> Vec<&'static str> {
        self.unit_states().failed_units
    }

    fn unit_health(&mut self) -> (BTreeMap<&'static str, u32>, Vec<&'static str>) {
        let found = self.unit_states();
        (found.restarts, found.failed_units)
    }

    fn voice_resting(&mut self) -> bool {
        self.latest::<lamp_msgs::VoiceState>(lamp_msgs::VoiceState::TOPIC)
            .filter(|(_, expired)| !expired)
            .is_none_or(|(state, _)| state.state == lamp_msgs::Voice::Resting)
    }

    fn requests(&mut self) -> Vec<lamp_msgs::ReportsRequest> {
        if self.requests.is_none()
            && let Some(bus) = &self.bus
        {
            // A bus name is `[a-z][a-z0-9_]*`: "cloud-reports" was refused,
            // and with the error dropped nothing took the voice's "stop
            // sending reports" (lamp-06c1, 19 Oct, #1755's trial).
            self.requests = match bus.subscribe_typed(
                lamp_msgs::ReportsRequest::TOPIC,
                REQUESTS_NAME,
                lamp_msgs::ReportsRequest::TYPE,
            ) {
                Ok(requests) => Some(requests),
                Err(error) => {
                    if !self.requests_refused {
                        self.requests_refused = true;
                        eprintln!(
                            "lamp-cloud reports: can't take {}: {error}",
                            lamp_msgs::ReportsRequest::TOPIC
                        );
                    }
                    None
                }
            };
        }
        let mut asked = Vec::new();
        if let Some(requests) = self.requests.as_mut() {
            while let Ok(Some(message)) = requests.recv(Duration::ZERO) {
                if let Ok(request) = message.parse::<lamp_msgs::ReportsRequest>() {
                    asked.push(request);
                }
            }
        }
        asked
    }

    fn tell(&mut self, state: &lamp_msgs::ReportsState) {
        if self.state.is_none()
            && let Some(bus) = &self.bus
        {
            self.state = bus
                .publisher_declared(
                    lamp_msgs::ReportsState::TOPIC,
                    &lamp_msgs::ReportsState::declaration(),
                    crate::lamp::SOURCE,
                )
                .ok();
        }
        if let Some(out) = self.state.as_mut() {
            let _ = out.publish_json(lamp_ipc::monotonic_us(), state);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lamp_msgs::{HealthItem, PartResult};

    #[test]
    fn failed_units_are_current_allowlisted_and_unambiguous() {
        let failed = "Id=lamp-update.service\nNRestarts=0\nActiveState=failed\n";
        let recovered = failed.replace("ActiveState=failed", "ActiveState=inactive");
        let before = UnitStates::of(failed);
        let after = UnitStates::of(&recovered);
        assert_eq!(before.failed_units, ["lamp-update.service"]);
        assert!(after.failed_units.is_empty());
        assert_eq!(before.restarts, after.restarts);
        assert_eq!(before.restarts.get("lamp-update.service"), Some(&0));

        let mixed = format!(
            "{failed}\nId=PRIVATE_FAILED_UNIT\nActiveState=failed\n\n\
             Id=lamp-voice.service\nNRestarts=7\nActiveState=active\n\n\
             Id=lamp-eyes.service\nNRestarts=2\nActiveState=failed\n"
        );
        assert_eq!(
            failed_units(&mixed),
            ["lamp-eyes.service", "lamp-update.service"]
        );

        for ambiguous in [
            "Id=lamp-update.service\nNRestarts=0\n".to_owned(),
            "ActiveState=failed\n".to_owned(),
            "Id=lamp-update.service\nActiveState=PRIVATE_FAILED_UNIT\n".to_owned(),
            "Id=lamp-update.service\nActiveState= failed\n".to_owned(),
            "Id=lamp-update.service \nActiveState=failed\n".to_owned(),
            format!("{failed}ActiveState=inactive\n"),
            format!("{failed}ActiveState=failed\n"),
            format!("{failed}Id=PRIVATE_FAILED_UNIT\n"),
            format!("{failed}Id=lamp-eyes.service\n"),
            format!("{failed}not a property\n"),
            format!("{failed}\n{recovered}"),
            format!("{recovered}\n{failed}"),
            format!("{failed}\n{failed}"),
        ] {
            assert!(failed_units(&ambiguous).is_empty(), "{ambiguous:?}");
            // A bad record cannot hide an unrelated, unambiguous failure.
            let with_other = format!("{ambiguous}\nId=lamp-actions.service\nActiveState=failed\n");
            assert_eq!(failed_units(&with_other), ["lamp-actions.service"]);
        }
    }

    /// The real production source calls a local command, never the host's
    /// systemd. Only the driver's clock and the cache's age are advanced;
    /// a oneshot's failure and recovery leave its restart count at zero.
    #[test]
    fn failed_units_share_the_gated_restart_query_and_cached_recovery() {
        use std::os::unix::fs::PermissionsExt;

        struct Observation {
            lamp: Lamp,
            resting: bool,
            seen: usize,
        }
        impl Sources for Observation {
            fn seen(&mut self) -> Seen {
                self.seen += 1;
                self.lamp.seen()
            }
            fn restarts(&mut self) -> BTreeMap<&'static str, u32> {
                self.lamp.restarts()
            }
            fn failed_units(&mut self) -> Vec<&'static str> {
                self.lamp.failed_units()
            }
            fn unit_health(&mut self) -> (BTreeMap<&'static str, u32>, Vec<&'static str>) {
                self.lamp.unit_health()
            }
            fn waits(&mut self) -> BTreeMap<&'static str, &'static str> {
                self.lamp.waits()
            }
            fn voice_resting(&mut self) -> bool {
                self.resting
            }
            fn requests(&mut self) -> Vec<lamp_msgs::ReportsRequest> {
                Vec::new()
            }
            fn tell(&mut self, _: &lamp_msgs::ReportsState) {}
        }

        let places = places("failed-cache");
        let root = places.own.parent().unwrap();
        let program = root.join("systemctl");
        let called = root.join("called");
        let states = root.join("states");
        let record_call = format!("printf '%s\\n' \"$@\" >> '{}'\n", called.display());
        fs::write(
            &program,
            format!("#!/bin/sh\n{record_call}cat '{}'\n", states.display()),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let failed = "Id=lamp-update.service\nNRestarts=0\nActiveState=failed\n\n\
                      Id=lamp-voice.service\nNRestarts=7\nActiveState=active\n\n\
                      Id=PRIVATE_FAILED_UNIT\nNRestarts=99\nActiveState=failed\n";
        fs::write(&states, failed).unwrap();
        let mut source = Observation {
            lamp: Lamp::new(None),
            resting: false,
            seen: 0,
        };
        source.lamp.systemctl = program.clone();
        // A stand-in shell can take seconds to start on a loaded machine.
        source.lamp.systemctl_within = Duration::from_secs(30);
        source.lamp.update_self = false;
        let shared = Shared::new(Reports::open(places.clone()));
        let start = Instant::now();
        let mut timing = Timing::new(start);

        // No consent, even when due: neither bus health nor systemd is read.
        assert!(!step(
            &shared,
            &mut source,
            &mut timing,
            None,
            start + FIRST_HEALTH,
            NOW
        ));
        assert_eq!(source.seen, 0);
        assert!(!called.exists());
        yes(&places);
        let due = timing.next_health;
        for at in [
            due - Duration::from_secs(1),
            due,
            due + VOICE_WAIT - Duration::from_secs(1),
        ] {
            assert!(!step(&shared, &mut source, &mut timing, None, at, NOW));
            assert!(
                !called.exists(),
                "no systemd query before the due voice wait ends"
            );
        }
        assert!(step(
            &shared,
            &mut source,
            &mut timing,
            None,
            due + VOICE_WAIT,
            NOW
        ));
        let first = shared.with(sent);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0]["failed_units"], json!(["lamp-update.service"]));
        assert_eq!(
            first[0]["restarts"],
            json!({"lamp-update.service": 0, "lamp-voice.service": 7})
        );
        assert!(!first[0].to_string().contains("PRIVATE_FAILED_UNIT"));
        shared.with(|reports| reports.acked(first[0]["seq"].as_u64().unwrap()));
        let mut args = vec!["show", "-p", "Id", "-p", "NRestarts", "-p", "ActiveState"];
        args.extend(UNITS.iter().copied());
        let invocation = format!("{}\n", args.join("\n"));
        assert_eq!(fs::read_to_string(&called).unwrap(), invocation);

        fs::write(
            &states,
            failed.replace("ActiveState=failed", "ActiveState=inactive"),
        )
        .unwrap();
        assert_eq!(source.lamp.failed_units(), ["lamp-update.service"]);
        assert_eq!(source.lamp.restarts().get("lamp-update.service"), Some(&0));
        assert_eq!(fs::read_to_string(&called).unwrap(), invocation);
        source.lamp.units_read.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(61);
        source.resting = true;
        let next = timing.next_health;
        assert!(step(
            &shared,
            &mut source,
            &mut timing,
            None,
            next,
            NOW + 900
        ));
        let recovered = shared.with(sent);
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0]["failed_units"], json!([]));
        assert_eq!(recovered[0]["restarts"], first[0]["restarts"]);
        assert_eq!(fs::read_to_string(&called).unwrap(), invocation.repeat(2));

        // A command failure is cached too; it does not retain a success
        // or start another process when the second field is read.
        fs::write(&program, format!("#!/bin/sh\n{record_call}exit 7\n")).unwrap();
        assert_eq!(source.lamp.restarts().get("lamp-voice.service"), Some(&7));
        source.lamp.units_read.as_mut().unwrap().0 = Instant::now() - Duration::from_secs(61);
        assert!(source.lamp.failed_units().is_empty());
        assert!(source.lamp.restarts().is_empty());
        assert_eq!(fs::read_to_string(&called).unwrap(), invocation.repeat(3));

        shared.with(|reports| reports.stop(NOW + 901, false).unwrap());
        let reads = source.seen;
        let next = timing.next_health;
        step(&shared, &mut source, &mut timing, None, next, NOW + 1_800);
        assert_eq!(source.seen, reads);
        assert_eq!(fs::read_to_string(&called).unwrap(), invocation.repeat(3));
        let _ = fs::remove_dir_all(root);
    }

    /// Through a real bus, as the voice's `reports_stop` sends it: a no on
    /// `reports/request` reaches `lamp-cloud` (lamp-06c1, 19 Oct: it didn't).
    #[test]
    fn a_no_on_the_bus_reaches_lamp_cloud() {
        // Short, as `places` is: a Unix socket's path holds 103 bytes on macOS.
        let root = PathBuf::from(format!("/tmp/lcrq-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut lamp = Lamp::new(Some(bus.clone()));
        assert_eq!(lamp.requests(), Vec::new());
        assert!(lamp.requests.is_some(), "lamp-cloud took reports/request");
        let mut out = bus
            .publisher_declared(
                lamp_msgs::ReportsRequest::TOPIC,
                &lamp_msgs::ReportsRequest::declaration(),
                "voice",
            )
            .unwrap();
        let sent = out
            .publish_json(1, &lamp_msgs::ReportsRequest::default())
            .unwrap();
        assert_eq!(sent.delivered, 1);
        let mut asked = Vec::new();
        for _ in 0..50 {
            asked = lamp.requests();
            if !asked.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(asked, vec![lamp_msgs::ReportsRequest::default()]);
        let _ = fs::remove_dir_all(&root);
    }

    /// The whole path on a real bus, as lamp-06c1 failed it (#1755's trial,
    /// check d): health is sent, the voice's no arrives on `reports/request`,
    /// `stopped` goes, and no health follows at the next 15 minutes.
    #[test]
    fn the_voices_no_on_the_bus_sends_stopped_and_ends_the_health() {
        let root = PathBuf::from(format!("/tmp/lcrs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let places = places("busno");
        yes(&places);
        let shared = Shared::new(Reports::open(places.clone()));
        let mut lamp = Lamp::new(Some(bus.clone()));
        lamp.systemctl = root.join("no-systemctl");
        let start = Instant::now();
        let mut timing = Timing::new(start);
        // The first health, as the lamp starts with a yes.
        let first = start + HEALTH_EVERY;
        step(&shared, &mut lamp, &mut timing, None, first, NOW);
        let kinds = shared.with(sent_kinds);
        assert_eq!(kinds, [json!("health")]);
        shared.with(|reports| {
            reports.acked(1);
            reports.unhand(None);
        });
        // The voice's "stop sending reports", as `reports_stop` sends it.
        let mut out = bus
            .publisher_declared(
                lamp_msgs::ReportsRequest::TOPIC,
                &lamp_msgs::ReportsRequest::declaration(),
                "voice",
            )
            .unwrap();
        // From a line whose answer was held (#1880).
        let sent = out
            .publish_json(
                1,
                &lamp_msgs::ReportsRequest {
                    share: false,
                    held: true,
                },
            )
            .unwrap();
        assert_eq!(sent.delivered, 1, "lamp-cloud is listening");
        let mut at = first + Duration::from_secs(1);
        for _ in 0..50 {
            step(&shared, &mut lamp, &mut timing, None, at, NOW + 1);
            if !shared.with(|reports| reports.consent().share) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            at += Duration::from_millis(10);
        }
        assert!(!shared.with(|reports| reports.consent().share));
        assert_eq!(shared.with(sent_kinds), [json!("stopped")]);
        let state = shared.with(|reports| reports.state());
        assert_eq!(
            (state.by, state.held, state.at_s),
            (Some(lamp_msgs::ReportsBy::Voice), true, Some(NOW + 1))
        );
        // The next 15 minutes: no health.
        step(
            &shared,
            &mut lamp,
            &mut timing,
            None,
            first + HEALTH_EVERY,
            NOW + 900,
        );
        step(
            &shared,
            &mut lamp,
            &mut timing,
            None,
            first + 2 * HEALTH_EVERY,
            NOW + 1800,
        );
        let kinds = shared.with(sent_kinds);
        assert!(kinds.iter().all(|kind| kind == "stopped"), "{kinds:?}");
        let _ = fs::remove_dir_all(&root);
    }

    /// The reports' records kept as fixtures, one folder for each change of a
    /// record's shape, each written by its day's builds: setup's answer
    /// (lamp-setup's `write_a_reports_answer_fixture`), and lamp-cloud's own
    /// choice, queue and the exit note its crash came from
    /// ([`write_a_reports_fixture`]). Every later build reads each as it was
    /// kept: the person's yes stands, with who gave it, the queued crash waits
    /// to be sent, nothing set aside, and the note it came from isn't taken
    /// again in the same boot (`/run` keeps it across an update), so an update
    /// never turns reports off, drops what waits or sends a crash twice (the
    /// build row "Updates cover the whole lamp, V3 to V5").
    #[test]
    fn each_kept_reports_record_reads_as_it_was_kept() {
        macro_rules! day {
            ($day:literal) => {
                (
                    $day,
                    include_str!(concat!(
                        "../tests/fixtures/stores/reports-",
                        $day,
                        "/setup.json"
                    )),
                    include_str!(concat!(
                        "../tests/fixtures/stores/reports-",
                        $day,
                        "/own.json"
                    )),
                    include_str!(concat!(
                        "../tests/fixtures/stores/reports-",
                        $day,
                        "/queue/state.json"
                    )),
                    include_str!(concat!(
                        "../tests/fixtures/stores/reports-",
                        $day,
                        "/queue/1.json"
                    )),
                    include_str!(concat!(
                        "../tests/fixtures/stores/reports-",
                        $day,
                        "/exits/lamp-voice.service@5521000000.json"
                    )),
                )
            };
        }
        let kept_days = [day!("2026-10-21")];
        for (day, setup, own, state, crash, exit) in kept_days {
            // Setup's answer alone, as on a lamp nobody has changed it on.
            let alone = places(&format!("kept-setup-{day}"));
            fs::write(&alone.setup, setup).unwrap();
            let consent = consent(&alone);
            assert!(consent.share, "{day}");
            assert_eq!(consent.by, Some(lamp_msgs::ReportsBy::Setup), "{day}");
            assert_eq!(
                consent.since.as_deref(),
                Some("2026-10-21T02:40:00Z"),
                "{day}"
            );
            let _ = fs::remove_dir_all(alone.own.parent().unwrap());
            // With lamp-cloud's own choice and its queue.
            let kept = places(&format!("kept-own-{day}"));
            fs::write(&kept.setup, setup).unwrap();
            fs::write(&kept.own, own).unwrap();
            fs::create_dir_all(&kept.queue).unwrap();
            fs::write(kept.queue.join("state.json"), state).unwrap();
            fs::write(kept.queue.join("1.json"), crash).unwrap();
            fs::create_dir_all(&kept.exits).unwrap();
            fs::write(kept.exits.join("lamp-voice.service@5521000000.json"), exit).unwrap();
            let mut reports = Reports::open(kept.clone());
            // The queue's state whole: `Kept` takes a field it can't find as
            // its default, so a renamed `crashes` would forget the crashes
            // already counted (the reviewer-lead on #2473).
            assert_eq!(
                missing(
                    &serde_json::from_str::<Value>(state).unwrap(),
                    &serde_json::to_value(&reports.kept).unwrap(),
                    "state",
                ),
                None,
                "{day}"
            );
            // The same boot: its note was taken for the crash that waits.
            assert_eq!(reports.new_notes(), Vec::new(), "{day}");
            let consent = reports.consent();
            assert!(consent.share, "{day}");
            assert_eq!(consent.by, Some(lamp_msgs::ReportsBy::App), "{day}");
            assert_eq!(
                consent.since.as_deref(),
                Some("2026-10-21T02:41:00Z"),
                "{day}"
            );
            assert_eq!(reports.state().queued, 1, "{day}");
            let due = reports.due(ID);
            assert_eq!(due.len(), 1, "{day}");
            let (seq, sent) = &due[0];
            let mut sent: Value = serde_json::from_str(sent).unwrap();
            assert_eq!(sent["id"], ID, "{day}");
            sent.as_object_mut().unwrap().remove("id");
            assert_eq!(
                (*seq, sent),
                (1, serde_json::from_str::<Value>(crash).unwrap()),
                "{day}"
            );
            assert_eq!(reports.said(), Vec::<String>::new(), "{day}");
            let _ = fs::remove_dir_all(kept.own.parent().unwrap());
        }
    }

    /// The first thing a kept record holds that `read` doesn't: a key of an
    /// object missing or different, by its path, an array of another length, or
    /// another value. A key `read` has beyond the kept one is a later build's
    /// addition, and passes: a kept day's test is never edited (#1505; the
    /// reviewer on #2486).
    fn missing(kept: &serde_json::Value, read: &serde_json::Value, path: &str) -> Option<String> {
        use serde_json::Value;
        match (kept, read) {
            (Value::Object(kept), Value::Object(read)) => kept.iter().find_map(|(key, value)| {
                let path = format!("{path}.{key}");
                match read.get(key) {
                    Some(got) => missing(value, got, &path),
                    None => Some(format!("{path}: not read")),
                }
            }),
            (Value::Array(kept), Value::Array(read)) if kept.len() == read.len() => kept
                .iter()
                .zip(read)
                .enumerate()
                .find_map(|(i, (value, got))| missing(value, got, &format!("{path}[{i}]"))),
            _ if kept == read => None,
            _ => Some(format!("{path}: kept {kept}, read {read}")),
        }
    }

    /// Writes lamp-cloud's own reports records to the folder in
    /// `LAMP_CLOUD_REPORTS_FIXTURE`: the app's yes (`own.json`), the queue
    /// with one crash (`queue/`), and the exit note it was taken from
    /// (`exits/`), as a lamp takes it. Setup's answer beside them comes from
    /// lamp-setup's own writer. Run by hand when a record's shape changes,
    /// keeping the folders kept.
    #[test]
    #[ignore = "writes a reports fixture: run by hand with LAMP_CLOUD_REPORTS_FIXTURE set"]
    fn write_a_reports_fixture() {
        let target = PathBuf::from(
            std::env::var_os("LAMP_CLOUD_REPORTS_FIXTURE").expect("LAMP_CLOUD_REPORTS_FIXTURE"),
        );
        // 21 Oct 2026, 02:40 UTC.
        const AT: u64 = 1_792_550_400;
        let places = places("fixture");
        let mut reports = Reports::open(places.clone());
        reports.set(true, AT + 60).unwrap();
        // As a lamp takes a crash: the unit's exit note, through `new_notes`.
        let note = write_exit_note(
            &places.exits,
            &planted_note("lamp-voice.service", 5_521_000_000),
        )
        .unwrap();
        let notes = reports.new_notes();
        assert_eq!(notes.len(), 1);
        let seq = reports.crash(AT + 120, Build::of("1712+9f3c1a2b7d4e").as_ref(), &notes[0]);
        assert_eq!(seq, Some(1));
        fs::create_dir_all(target.join("queue")).unwrap();
        fs::create_dir_all(target.join("exits")).unwrap();
        fs::copy(&places.own, target.join("own.json")).unwrap();
        for name in ["state.json", "1.json"] {
            fs::copy(places.queue.join(name), target.join("queue").join(name)).unwrap();
        }
        fs::copy(&note, target.join("exits").join(note.file_name().unwrap())).unwrap();
        let _ = fs::remove_dir_all(places.own.parent().unwrap());
    }

    pub(crate) fn places(name: &str) -> Places {
        let root = PathBuf::from(format!("/tmp/lcr-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("setup")).unwrap();
        fs::write(root.join("boot_id"), "boot-1\n").unwrap();
        Places {
            setup: root.join("setup/reports.json"),
            own: root.join("reports.json"),
            queue: root.join("reports"),
            exits: root.join("exits"),
            boot_id: root.join("boot_id"),
        }
    }

    fn yes(places: &Places) {
        fs::write(
            &places.setup,
            r#"{"share":true,"asked":1,"at":"2026-10-19T04:00:00Z"}"#,
        )
        .unwrap();
    }

    const NOW: u64 = 1_792_296_000; // 2026-10-18T04:00:00Z
    /// The device's id, as `info` carries it.
    const ID: &str = "d-42";

    /// Strings no report may ever carry, planted in every input.
    const SECRETS: [&str; 10] = [
        "Alice",
        "HomeWiFi-5G",
        "192.168.1.42",
        "lamp-7f72",
        "Asia/Ho_Chi_Minh",
        "s3cret",
        "remind me",
        "deadbeefcafe-Alice",
        "journal line",
        "PRIVATE_FAILED_UNIT",
    ];

    fn planted() -> Seen {
        let secret = "Alice said HomeWiFi-5G at 192.168.1.42: remind me, s3cret, journal line";
        let mut temperature_c = BTreeMap::new();
        temperature_c.insert("HomeWiFi-5G".to_owned(), 71.24);
        temperature_c.insert("cpu-thermal".to_owned(), 63.0);
        Seen {
            health: Some((
                Health {
                    level: Level::Error,
                    message: secret.into(),
                    items: vec![
                        HealthItem {
                            name: "Alice".into(),
                            level: Level::Error,
                            message: secret.into(),
                            age_ms: 10,
                        },
                        HealthItem {
                            name: "voice".into(),
                            level: Level::Warn,
                            message: secret.into(),
                            age_ms: 210,
                        },
                        HealthItem {
                            name: "lamp-7f72".into(),
                            level: Level::Ok,
                            message: "Asia/Ho_Chi_Minh".into(),
                            age_ms: 5,
                        },
                    ],
                    missing: vec!["s3cret".into(), "eyes".into()],
                    quiet_ms: 0,
                },
                false,
            )),
            board: Some(BoardState {
                temperature_c,
                cpu_load: 0.4123,
                memory_used: 0.51234,
                disk_free: 0.1382,
                uptime_s: 187_220,
                wifi_dbm: Some(-58),
                clock_synced: Some(true),
            }),
            online: Some(Online {
                online: Some(true),
                from: "Alice".into(),
                down_for_s: None,
            }),
            selftest: Some(SelfTest {
                occasion: lamp_msgs::Occasion::OnDemand,
                started: 0.0,
                took: 1.0,
                verdict: Verdict::Fail,
                parts: vec![
                    PartResult::new("ring", Verdict::Fail, secret).with("s3cret", secret),
                    PartResult::new("Alice", Verdict::Fail, secret),
                    PartResult::new("camera", Verdict::NotChecked, "HomeWiFi-5G"),
                ],
                build: Some("deadbeefcafe-Alice".into()),
                // The report's summary leaves the lamp's own name out.
                lamp: Some("lamp-Alice".into()),
            }),
            restarts: restarts(
                "Id=lamp-voice.service\nNRestarts=0\n\nId=Alice.service\nNRestarts=4\n\nId=lamp-eyes.service\nNRestarts=1\n",
            ),
            failed_units: vec![
                "PRIVATE_FAILED_UNIT",
                "lamp-update.service",
                "lamp-update.service",
            ],
            waits: BTreeMap::new(),
        }
    }

    fn planted_note(unit: &str, at_us: u64) -> ExitNote {
        let mut shown_ = BTreeMap::new();
        shown_.insert("NRestarts".to_owned(), "2".to_owned());
        shown_.insert(
            "ExecMainStartTimestampMonotonic".to_owned(),
            "1000000".to_owned(),
        );
        shown_.insert(
            "ExecMainExitTimestampMonotonic".to_owned(),
            "5521000000".to_owned(),
        );
        exit_note(unit, "signal", "killed", "SEGV", at_us, "boot-1", &shown_).unwrap()
    }

    fn sent(reports: &mut Reports) -> Vec<Value> {
        reports
            .due(ID)
            .into_iter()
            .map(|(_, bytes)| serde_json::from_str(&bytes).unwrap())
            .collect()
    }

    #[test]
    fn a_health_report_carries_numbers_enums_and_listed_names_only() {
        let places = places("health");
        yes(&places);
        let mut reports = Reports::open(places);
        let build = Build::of("1712+9f3c1a2b7d4e");
        reports.health(NOW, build.as_ref(), &planted()).unwrap();
        let sent = sent(&mut reports);
        assert_eq!(
            sent[0],
            json!({
                "type": "report", "schema": "lamp-report/1", "device": "lamp", "id": "d-42",
                "generation": "lamp-v2", "kind": "health", "seq": 1,
                "sent_at": "2026-10-18T04:00:00Z", "clock_synced": true,
                "build": "1712+9f3c1a2b7d4e", "uptime_s": 187220,
                "dropped": {"queue_full": 0, "over_rate": 0, "too_big": 0},
                "level": "error",
                "parts": [{"name": "voice", "level": "warn", "age_ms": 210}],
                "missing": ["eyes"], "other_parts": 3,
                "board": {"cpu_load": 0.41, "memory_used": 0.512, "disk_free": 0.138,
                          "hottest_c": 71.2, "cpu_c": null, "throttled": null,
                          "throttled_s": null, "wifi_dbm": -58},
                "online": true, "down_for_s": null,
                "selftest": {"verdict": "fail", "failed": ["ring"], "other_failed": 1,
                             "not_checked": 1, "build": null},
                "restarts": {"lamp-eyes.service": 1, "lamp-voice.service": 0},
                "failed_units": ["lamp-update.service"]
            })
        );
    }

    #[test]
    fn a_crash_report_is_systemds_numbers_by_the_lists() {
        let places = places("crash");
        yes(&places);
        let mut reports = Reports::open(places);
        let mut seen = planted();
        seen.health.as_mut().unwrap().0.items[1].name = "eyes".into();
        reports.look(NOW, &seen);
        reports
            .crash(NOW + 62, None, &planted_note("lamp-eyes.service", 9))
            .unwrap();
        let sent = sent(&mut reports);
        assert_eq!(
            sent[0],
            json!({
                "type": "report", "schema": "lamp-report/1", "device": "lamp", "id": "d-42",
                "generation": "lamp-v2", "kind": "crash", "seq": 1,
                "sent_at": "2026-10-18T04:01:02Z", "clock_synced": null, "build": null,
                "uptime_s": 0, "dropped": {"queue_full": 0, "over_rate": 0, "too_big": 0},
                "unit": "lamp-eyes.service", "result": "signal", "exit_code": null,
                "signal": "SIGSEGV", "ran_s": 5520, "restarts_since_boot": 2,
                "level_before": "warn", "panic": null
            })
        );
        // An exit with a code, and a unit not lampOS's.
        let mut code = planted_note("lamp-air.service", 10);
        code.result = "exit-code".into();
        code.code = "exited".into();
        code.status = "101".into();
        let body = crash_body(&code, None).unwrap();
        assert_eq!((body.exit_code, body.signal), (Some(101), None));
        let mut theirs = planted_note("lamp-air.service", 11);
        theirs.unit = "Alice.service".into();
        assert_eq!(crash_body(&theirs, None), None);
    }

    /// The canary: secrets planted in every input, the identity's included,
    /// reach no report of any kind, nor the app's answer.
    #[test]
    fn no_secret_planted_in_any_input_reaches_any_report() {
        let places = places("canary");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        // A build that is not one, and the identity's strings nowhere to go.
        let build = Build::of("Alice");
        assert_eq!(build, None);
        reports.health(NOW, build.as_ref(), &planted()).unwrap();
        let mut forged = planted_note("lamp-voice.service", 1);
        forged.result = "Alice said HomeWiFi-5G".into();
        forged.code = "killed".into();
        forged.status = "s3cret at 192.168.1.42".into();
        forged.boot = "boot-1".into();
        reports.crash(NOW, None, &forged).unwrap();
        let mut bytes: Vec<String> = reports.due(ID).into_iter().map(|(_, b)| b).collect();
        reports.stop(NOW + 1, false).unwrap();
        bytes.extend(reports.due(ID).into_iter().map(|(_, b)| b));
        bytes.push(reports.answer().to_string());
        assert_eq!(bytes.len(), 4);
        for report in &bytes {
            for secret in SECRETS {
                assert!(!report.contains(secret), "{secret} in {report}");
            }
        }
    }

    #[test]
    fn nothing_goes_without_each_of_the_three_gates() {
        // The two switches.
        assert!(gates(Some("on"), Some("on")));
        for (cloud, reports) in [
            (None, Some("on")),
            (Some("on"), None),
            (Some("off"), Some("on")),
            (Some("on"), Some("yes")),
            (None, None),
        ] {
            assert!(!gates(cloud, reports), "{cloud:?} {reports:?}");
        }
        // The person's yes: none, a no, an unreadable file, a JSON "true",
        // a newer wording.
        for setup in [
            None,
            Some(r#"{"share":false,"asked":1}"#),
            Some("nonsense"),
            Some(r#"{"share":"true","asked":1}"#),
            Some(r#"{"share":1,"asked":1}"#),
            Some(r#"{"share":true,"asked":2}"#),
            Some(r#"{"share":true}"#),
        ] {
            let places = places("gates");
            if let Some(setup) = setup {
                fs::write(&places.setup, setup).unwrap();
            }
            let mut reports = Reports::open(places.clone());
            assert_eq!(reports.health(NOW, None, &planted()), None, "{setup:?}");
            fs::create_dir_all(&places.exits).unwrap();
            write_exit_note(&places.exits, &planted_note("lamp-air.service", 5)).unwrap();
            for note in reports.new_notes() {
                assert_eq!(reports.crash(NOW, None, &note), None);
            }
            assert!(reports.due(ID).is_empty(), "{setup:?}");
            assert!(!places.queue.join("1.json").exists());
        }
        // A no of its own wins over setup's yes, and is read at each send.
        let places = places("gates-own");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        assert!(reports.health(NOW, None, &planted()).is_some());
        fs::write(&places.own, r#"{"share":false}"#).unwrap();
        assert!(reports.due(ID).is_empty());
        assert_eq!(reports.health(NOW, None, &planted()), None);
    }

    #[test]
    fn a_no_deletes_what_waits_and_nothing_but_stopped_goes_after() {
        let places = places("stop");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        reports.health(NOW, None, &planted()).unwrap();
        reports
            .crash(NOW, None, &planted_note("lamp-air.service", 3))
            .unwrap();
        assert!(places.queue.join("2.json").exists());
        reports.stop(NOW + 5, false).unwrap();
        assert!(!places.queue.join("2.json").exists());
        let sent = sent(&mut reports);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["kind"], "stopped");
        assert_eq!(sent[0]["seq"], 3);
        // Nothing after it.
        assert_eq!(reports.health(NOW + 6, None, &planted()), None);
        assert!(reports.due(ID).is_empty());
        assert_eq!(
            reports.answer(),
            json!({
                "share": false,
                "since": "2026-10-18T04:00:05Z",
                "queued": 1,
                "by": "voice",
                "held": false,
                "at": "2026-10-18T04:00:05Z",
            })
        );
        // A second no says nothing more.
        reports.stop(NOW + 7, false).unwrap();
        assert!(reports.due(ID).is_empty());
        // The app's yes turns it on again, and a `stopped` through once
        // never comes after the new reports.
        reports.acked(3);
        reports.set(true, NOW + 8).unwrap();
        assert_eq!(reports.queued(), 0);
        assert!(reports.health(NOW + 9, None, &planted()).is_some());
        reports.connected();
        let kinds: Vec<Value> = sent_kinds(&mut reports);
        assert_eq!(kinds, [json!("health")]);
    }

    fn sent_kinds(reports: &mut Reports) -> Vec<Value> {
        sent(reports)
            .into_iter()
            .map(|report| report["kind"].clone())
            .collect()
    }

    /// The backend on #1683: a no given offline is kept until it is sent,
    /// across a restart too, and goes once more on the next connection,
    /// since the broker's ack is not the store's.
    #[test]
    fn a_no_given_offline_goes_when_connected_and_once_more_on_the_next() {
        let places = places("stop-offline");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        reports.health(NOW, None, &planted()).unwrap();
        // Offline: nothing is handed. The no is kept on flash.
        reports.stop(NOW + 1, false).unwrap();
        assert!(places.queue.join("2.json").exists());
        drop(reports);
        let mut reports = Reports::open(places.clone());
        assert_eq!(reports.queued(), 1, "read back after a restart");
        // The connection comes: it goes, with the same seq and sent_at.
        reports.connected();
        let first = sent(&mut reports);
        assert_eq!(first.len(), 1);
        assert_eq!(
            (&first[0]["kind"], &first[0]["seq"]),
            (&json!("stopped"), &json!(2))
        );
        reports.acked(2);
        // Not again on the same connection.
        assert!(reports.due(ID).is_empty());
        assert!(places.queue.join("2.json").exists());
        // The next connection: once more, the same message.
        reports.unhand(None);
        reports.connected();
        let second = sent(&mut reports);
        assert_eq!(second, first);
        reports.acked(2);
        assert_eq!(reports.queued(), 0);
        assert!(!places.queue.join("2.json").exists());
        reports.connected();
        assert!(reports.due(ID).is_empty());
    }

    /// The backend drops a duplicate by `(device, seq, sent_at)`: a report
    /// sent again is the same message, and every one carries `info`'s id.
    #[test]
    fn a_report_sent_again_is_the_same_message_with_infos_id() {
        let places = places("again");
        yes(&places);
        let mut reports = Reports::open(places);
        reports.health(NOW, None, &planted()).unwrap();
        let first = reports.due(ID);
        reports.unhand(None);
        let again = reports.due(ID);
        assert_eq!(first, again);
        let report: Value = serde_json::from_str(&first[0].1).unwrap();
        assert_eq!(report["id"], ID);
        assert_eq!(report["sent_at"], "2026-10-18T04:00:00Z");
        // No id, or one that is not one: nothing is handed.
        reports.unhand(None);
        for id in ["", "a b", &"x".repeat(ID_MOST + 1)] {
            assert!(reports.due(id).is_empty(), "{id:?}");
        }
        assert_eq!(topic("fd/d-42"), "fd/d-42/report");
    }

    #[test]
    fn the_bus_can_only_turn_reports_off() {
        struct Bus {
            asked: Vec<lamp_msgs::ReportsRequest>,
            told: Vec<lamp_msgs::ReportsState>,
        }
        let ask = |share, held| lamp_msgs::ReportsRequest { share, held };
        impl Sources for Bus {
            fn seen(&mut self) -> Seen {
                Seen::default()
            }
            fn voice_resting(&mut self) -> bool {
                true
            }
            fn requests(&mut self) -> Vec<lamp_msgs::ReportsRequest> {
                std::mem::take(&mut self.asked)
            }
            fn tell(&mut self, state: &lamp_msgs::ReportsState) {
                self.told.push(*state);
            }
        }
        let places = places("bus");
        let shared = Shared::new(Reports::open(places.clone()));
        let now = Instant::now();
        let mut timing = Timing::new(now);
        let mut bus = Bus {
            asked: vec![ask(true, false)],
            told: Vec::new(),
        };
        step(&shared, &mut bus, &mut timing, None, now, NOW);
        assert!(!shared.with(|r| r.consent().share));
        assert_eq!(bus.told.last().map(|s| s.share), Some(false));
        yes(&places);
        bus.asked = vec![ask(true, false), ask(false, true)];
        assert!(step(&shared, &mut bus, &mut timing, None, now, NOW + 1));
        assert!(!shared.with(|r| r.consent().share));
        // A held line's no is said on `reports/state` as the voice's, held,
        // and when (#1880).
        assert_eq!(
            bus.told.last(),
            Some(&lamp_msgs::ReportsState {
                share: false,
                queued: 1,
                by: Some(lamp_msgs::ReportsBy::Voice),
                held: true,
                at_s: Some(NOW + 1),
            })
        );
        // A held no beside one heard is the person's: not kept as held.
        yes_again(&places);
        bus.asked = vec![ask(false, true), ask(false, false)];
        assert!(step(&shared, &mut bus, &mut timing, None, now, NOW + 2));
        assert_eq!(
            bus.told.last().map(|s| (s.by, s.held)),
            Some((Some(lamp_msgs::ReportsBy::Voice), false))
        );
    }

    /// A yes as the app would give it, over any choice kept before.
    fn yes_again(places: &Places) {
        let _ = fs::remove_file(&places.own);
        yes(places);
    }

    /// The first builder on #1722, 18:44, on Third's read of #1880: how and
    /// when reports were stopped is kept with the choice, said on
    /// `reports/state`, and returned by `reports.get`, so a stop from a
    /// held line is never one nobody can find.
    #[test]
    fn a_choice_is_kept_with_who_made_it_whether_held_and_when() {
        let places = places("who");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        let state = reports.state();
        assert_eq!(
            (state.by, state.held, state.at_s),
            (
                Some(lamp_msgs::ReportsBy::Setup),
                false,
                seconds_of("2026-10-19T04:00:00Z")
            )
        );
        // A held line's stop: the voice's, held.
        reports.stop(NOW + 5, true).unwrap();
        assert_eq!(
            reports.consent(),
            Consent {
                share: false,
                since: Some("2026-10-18T04:00:05Z".into()),
                by: Some(lamp_msgs::ReportsBy::Voice),
                held: true,
            }
        );
        assert_eq!(
            reports.state(),
            lamp_msgs::ReportsState {
                share: false,
                queued: 1,
                by: Some(lamp_msgs::ReportsBy::Voice),
                held: true,
                at_s: Some(NOW + 5),
            }
        );
        let answer = reports.answer();
        assert_eq!(
            (&answer["by"], &answer["held"], &answer["at"]),
            (
                &json!("voice"),
                &json!(true),
                &json!("2026-10-18T04:00:05Z")
            )
        );
        // Kept on flash: read again after a restart.
        drop(reports);
        let mut reports = Reports::open(places.clone());
        assert_eq!(reports.consent().by, Some(lamp_msgs::ReportsBy::Voice));
        assert!(reports.consent().held);
        // A stop when already off changes nothing: the one that stopped them
        // is kept.
        reports.stop(NOW + 9, false).unwrap();
        assert_eq!(reports.state().at_s, Some(NOW + 5));
        assert!(reports.state().held);
        // The app's yes and no are the app's, never held.
        reports.set(true, NOW + 10).unwrap();
        reports.set(false, NOW + 11).unwrap();
        let state = reports.state();
        assert_eq!(
            (state.by, state.held, state.at_s),
            (Some(lamp_msgs::ReportsBy::App), false, Some(NOW + 11))
        );
        let answer = reports.answer();
        assert_eq!(
            (&answer["by"], &answer["held"], &answer["at"]),
            (&json!("app"), &json!(false), &json!("2026-10-18T04:00:11Z"))
        );
        // A choice kept before this was says nobody; `held` is only ever
        // the voice's.
        for (kept, by) in [
            (r#"{"share":false,"at":"2026-10-18T04:00:00Z"}"#, None),
            (
                r#"{"share":false,"at":"2026-10-18T04:00:00Z","by":"app","held":true}"#,
                Some(lamp_msgs::ReportsBy::App),
            ),
            (
                r#"{"share":false,"at":"2026-10-18T04:00:00Z","by":"someone","held":true}"#,
                None,
            ),
        ] {
            fs::write(&places.own, kept).unwrap();
            let consent = reports.consent();
            assert_eq!((consent.by, consent.held), (by, false), "{kept}");
        }
    }

    #[test]
    fn the_queue_keeps_its_bounds_and_the_rate_its_limits() {
        let places = places("bounds");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        // 12 an hour, then counted.
        for n in 0..14 {
            let at = NOW + n;
            let got = reports.crash(at, None, &planted_note("lamp-air.service", n + 1));
            assert_eq!(got.is_some(), n < 12, "{n}");
        }
        assert_eq!(reports.dropped().over_rate, 2);
        // 50 a day: four more hours of twelve.
        let mut taken = 12;
        for hour in 1..5 {
            for n in 0..12 {
                let at = NOW + hour * 3_700 + n;
                if reports
                    .crash(at, None, &planted_note("lamp-air.service", 100 + at))
                    .is_some()
                {
                    taken += 1;
                }
            }
        }
        assert_eq!(taken, 50);
        // The queue: 64 at most, crashes kept over healths, healths one.
        assert!(reports.queued() <= QUEUE_MOST);
        assert_eq!(reports.dropped().queue_full, 0);
        let at = NOW + 2 * 86_400;
        let mut reports = Reports::open(places.clone());
        assert_eq!(reports.queued(), 50, "the crashes are read back from flash");
        for n in 0..20 {
            reports
                .crash(
                    at + n * 400,
                    None,
                    &planted_note("lamp-air.service", at + n),
                )
                .unwrap();
        }
        assert_eq!(reports.queued(), QUEUE_MOST);
        assert_eq!(reports.dropped().queue_full, 6);
        // Full of crashes: a health is what goes.
        reports.health(at + 9_000, None, &planted()).unwrap();
        assert_eq!(reports.queued(), QUEUE_MOST);
        assert_eq!(reports.dropped().queue_full, 7);
        assert!(reports.queue.iter().all(|e| e.kind == Kind::Crash));
        // A week on, everything older is gone, files too.
        reports.health(at + KEPT_FOR.as_secs() + 10_000, None, &planted());
        assert_eq!(reports.queued(), 1);
        let files = fs::read_dir(&places.queue)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".json")
                    && e.as_ref().unwrap().file_name() != "state.json"
            })
            .count();
        assert_eq!(files, 0);
    }

    #[test]
    fn a_report_over_four_kib_is_cut_or_counted() {
        let places = places("size");
        yes(&places);
        let mut reports = Reports::open(places);
        let mut seen = planted();
        let health = &mut seen.health.as_mut().unwrap().0;
        health.items = (0..64)
            .map(|n| HealthItem {
                name: PARTS[n % PARTS.len()].into(),
                level: Level::Ok,
                message: String::new(),
                age_ms: n as u64,
            })
            .collect();
        reports.health(NOW, None, &seen).unwrap();
        let sent = sent(&mut reports);
        assert_eq!(sent[0]["parts"].as_array().unwrap().len(), MOST_PARTS);
        assert_eq!(sent[0]["other_parts"], 64 - MOST_PARTS + 1);
        assert_eq!(sent[0]["failed_units"], json!(["lamp-update.service"]));
        assert!(sent[0].to_string().len() <= MOST_BYTES);
        reports.acked(sent[0]["seq"].as_u64().unwrap());
        seen.failed_units = UNITS.to_vec();
        seen.health.as_mut().unwrap().0.missing = vec!["voice".into(); MOST_BYTES];
        assert_eq!(reports.health(NOW + 1, None, &seen), None);
        assert_eq!(reports.dropped().too_big, 1);
        assert!(reports.due(ID).is_empty());
    }

    #[test]
    fn handed_reports_go_on_the_ack_and_again_after_a_drop_with_the_same_seq() {
        let places = places("acks");
        yes(&places);
        let mut reports = Reports::open(places);
        reports.health(NOW, None, &planted()).unwrap();
        reports
            .crash(NOW, None, &planted_note("lamp-air.service", 1))
            .unwrap();
        let first: Vec<u64> = reports.due(ID).into_iter().map(|(seq, _)| seq).collect();
        assert_eq!(first, [1, 2]);
        assert!(reports.due(ID).is_empty());
        reports.acked(1);
        reports.unhand(None);
        let again: Vec<u64> = reports.due(ID).into_iter().map(|(seq, _)| seq).collect();
        assert_eq!(again, [2]);
    }

    #[test]
    fn exit_notes_are_systemds_variables_and_success_writes_nothing() {
        let places = places("notes");
        let none = BTreeMap::new();
        assert_eq!(
            exit_note("lamp-air.service", "success", "exited", "0", 1, "b", &none),
            None
        );
        assert_eq!(
            exit_note("ssh.service", "signal", "killed", "SEGV", 1, "b", &none),
            None
        );
        for n in 0..10 {
            write_exit_note(&places.exits, &planted_note("lamp-air.service", n + 1)).unwrap();
        }
        let files = fs::read_dir(&places.exits).unwrap().count();
        assert_eq!(files, NOTES_PER_UNIT);
        let mut reports = Reports::open(places.clone());
        let notes = reports.new_notes();
        assert_eq!(notes.len(), NOTES_PER_UNIT);
        assert_eq!(notes[0].at_us, 3);
        assert!(reports.new_notes().is_empty(), "taken once");
        // Read again after a restart of lamp-cloud: still taken once.
        let mut reports = Reports::open(places.clone());
        assert!(reports.new_notes().is_empty());
        // A new boot starts again.
        fs::write(&places.boot_id, "boot-2\n").unwrap();
        let mut note = planted_note("lamp-air.service", 100);
        note.boot = "boot-2".into();
        write_exit_note(&places.exits, &note).unwrap();
        assert_eq!(reports.new_notes().len(), 1);
        assert_eq!(
            shown("NRestarts=3\nExecMainStartTimestampMonotonic=5\n")["NRestarts"],
            "3"
        );
    }

    #[test]
    fn template_exit_notes_keep_only_the_fixed_name_in_files_and_reports() {
        let places = places("template-notes");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        let shown = shown(
            "NRestarts=7\nExecMainStartTimestampMonotonic=1000000\nExecMainExitTimestampMonotonic=4000000\n",
        );
        let templates: Vec<_> = UNITS
            .iter()
            .copied()
            .filter(|name| name.ends_with("@.service"))
            .collect();
        for (n, template) in templates.iter().enumerate() {
            let actual = template.replace("@.service", "@private-instance-42.service");
            let note = exit_note(
                &actual,
                "signal",
                "killed",
                "SEGV",
                n as u64 + 1,
                "boot-1",
                &shown,
            )
            .unwrap();
            assert_eq!(note.unit, *template);
            assert_eq!((note.restarts, note.ran_s), (Some(7), Some(3)));
            let path = write_exit_note(&places.exits, &note).unwrap();
            assert!(
                !path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains("private-instance")
            );
            assert!(
                !fs::read_to_string(path)
                    .unwrap()
                    .contains("private-instance")
            );
        }
        let notes = reports.new_notes();
        assert_eq!(notes.len(), templates.len());
        for note in notes {
            reports.crash(NOW, None, &note).unwrap();
        }
        let sent = sent(&mut reports);
        assert_eq!(sent.len(), templates.len());
        for report in sent {
            assert!(!report.to_string().contains("private-instance"));
            assert!(UNITS.contains(&report["unit"].as_str().unwrap()));
            assert_eq!(report["restarts_since_boot"], 7);
            assert_eq!(report["ran_s"], 3);
        }
        // Different instances share one fixed name and its existing bound.
        for n in 0..NOTES_PER_UNIT + 2 {
            let actual =
                templates[0].replace("@.service", &format!("@private-instance-{n}.service"));
            let note = exit_note(
                &actual,
                "signal",
                "killed",
                "SEGV",
                100 + n as u64,
                "boot-1",
                &shown,
            )
            .unwrap();
            write_exit_note(&places.exits, &note).unwrap();
        }
        let files: Vec<_> = fs::read_dir(&places.exits)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(files.len(), templates.len() - 1 + NOTES_PER_UNIT);
        assert!(
            files
                .iter()
                .all(|name| !name.to_string_lossy().contains("private-instance"))
        );
        fs::remove_dir_all(places.own.parent().unwrap()).unwrap();
    }

    #[test]
    fn only_nonempty_instances_of_listed_templates_are_exit_notes() {
        let none = BTreeMap::new();
        for name in [
            "lamp-unlisted@private-instance.service",
            "lamp-voice@private-instance.service",
            "lamp-setup-change@.service",
            "lamp-setup-change@private-instance.timer",
            "lamp-setup-change@private@instance.service",
            "lamp-setup-change@../private-instance.service",
            "lamp-setup-change@private instance.service",
        ] {
            assert!(
                exit_note(name, "signal", "killed", "SEGV", 1, "b", &none).is_none(),
                "{name}"
            );
        }
        for name in UNITS.iter().filter(|name| !name.contains('@')) {
            assert_eq!(
                exit_note(name, "signal", "killed", "SEGV", 1, "b", &none)
                    .unwrap()
                    .unit,
                *name
            );
        }
        let actual = "lamp-setup-change@private-instance.service";
        assert!(exit_note(actual, "success", "exited", "0", 1, "b", &none).is_none());
        // Canonicalization is confined to exit notes; the general unit
        // allowlist and systemctl's caller keep taking the actual name.
        assert!(unit(actual).is_none());
        let escaped = r"lamp-setup-change@private\x20instance.service";
        assert_eq!(
            exit_note(escaped, "signal", "killed", "SEGV", 1, "b", &none)
                .unwrap()
                .unit,
            "lamp-setup-change@.service"
        );
    }

    #[test]
    fn a_local_template_exit_note_cannot_grant_consent_to_send() {
        let places = places("template-no");
        let note = exit_note(
            "lamp-setup-change@private-instance.service",
            "signal",
            "killed",
            "SEGV",
            1,
            "boot-1",
            &BTreeMap::new(),
        )
        .unwrap();
        write_exit_note(&places.exits, &note).unwrap();
        let mut reports = Reports::open(places.clone());
        let notes = reports.new_notes();
        assert_eq!(
            notes.len(),
            1,
            "local notes are consumed even without a yes"
        );
        assert!(reports.crash(NOW, None, &notes[0]).is_none());
        assert_eq!(reports.queued(), 0);
        assert!(reports.due(ID).is_empty());
        assert!(!reports.consent().share);
        fs::remove_dir_all(places.own.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_error_held_a_minute_sends_at_once_and_once_an_hour() {
        let places = places("urgent");
        let mut reports = Reports::open(places);
        let mut seen = planted();
        seen.health.as_mut().unwrap().0.items[0].name = "body".into();
        assert!(!reports.look(NOW, &seen));
        assert!(!reports.look(NOW + 59, &seen));
        assert!(reports.look(NOW + 60, &seen));
        assert!(!reports.look(NOW + 61, &seen));
        assert!(reports.look(NOW + 60 + 3_600, &seen));
    }

    #[test]
    fn the_lists_are_the_products() {
        let product = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
        let mut units: Vec<String> = fs::read_dir(product)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("lamp-") && name.ends_with(".service"))
            .collect();
        units.sort();
        assert_eq!(units, UNITS);
        let health = include_str!("../../../deploy/product/lamp-health.service");
        for word in health.split("--expect ").skip(1) {
            let name = word.split_whitespace().next().unwrap();
            assert!(PARTS.contains(&name), "{name}");
        }
    }

    #[test]
    fn the_exit_note_drop_in_runs_this_binary_for_the_unit_stopped() {
        let drop_in = include_str!("../../../deploy/product/exit-note.conf");
        let lines: Vec<&str> = drop_in
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .collect();
        assert_eq!(
            lines,
            [
                "[Service]",
                "ExecStopPost=-+/opt/lampOS/cloud/current/lamp-cloud exit-note %n"
            ]
        );
        let unit = include_str!("../../../deploy/product/lamp-cloud.service");
        assert!(unit.contains("ExecStart=/opt/lampOS/cloud/current/lamp-cloud "));
    }

    #[test]
    fn only_the_newest_health_waits() {
        let places = places("newest");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        reports.health(NOW, None, &planted()).unwrap();
        reports.health(NOW + 1, None, &planted()).unwrap();
        let sent = sent(&mut reports);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["seq"], 2);
        // A health is never written to flash.
        assert!(!places.queue.join("2.json").exists());
    }

    #[test]
    fn times_read_back_as_written() {
        assert_eq!(seconds_of("2026-10-18T04:00:00Z"), Some(NOW));
        assert_eq!(seconds_of("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(seconds_of("2000-02-29T00:00:00Z"), Some(951_782_400));
        assert_eq!(seconds_of("nonsense"), None);
        assert_eq!(signal_name("11"), Some("SIGSEGV"));
        assert_eq!(signal_name("SIGABRT"), Some("SIGABRT"));
        assert_eq!(signal_name("Alice"), None);
    }

    /// The file the queue folder holds for `seq`, as a report of `kind`.
    fn queued_file(places: &Places, seq: u64, text: &str) {
        fs::create_dir_all(&places.queue).unwrap();
        fs::write(places.queue.join(format!("{seq}.json")), text).unwrap();
    }

    /// The one file in `folder` whose name starts with `prefix`.
    fn aside(folder: &Path, prefix: &str) -> PathBuf {
        let found: Vec<PathBuf> = fs::read_dir(folder)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
            .map(|entry| entry.path())
            .collect();
        assert_eq!(
            found.len(),
            1,
            "{prefix} in {}: {found:?}",
            folder.display()
        );
        found[0].clone()
    }

    /// #2052, the privacy rule: a choice this build can't read is off, never
    /// on, and never lets setup's older yes through. Kept whole under
    /// another name, an off kept in its place, said once.
    #[test]
    fn a_choice_it_cant_read_is_off_kept_whole_and_said() {
        for unreadable in [
            "{\"share\":false,\"at\":\"2026-10-",
            "nonsense",
            "",
            r#"{"share":"no"}"#,
            r#"[true]"#,
        ] {
            let places = places("unread-own");
            yes(&places);
            fs::write(&places.own, unreadable).unwrap();
            assert!(!consent(&places).share, "{unreadable:?}");
            let mut reports = Reports::open(places.clone());
            assert!(!reports.consent().share, "{unreadable:?}");
            assert_eq!(reports.health(NOW, None, &planted()), None);
            assert!(reports.due(ID).is_empty());
            let kept = aside(places.own.parent().unwrap(), "reports.json.unread-");
            assert_eq!(fs::read_to_string(&kept).unwrap(), unreadable);
            let own: Value = serde_json::from_slice(&fs::read(&places.own).unwrap()).unwrap();
            assert_eq!(own["share"], false);
            let said = reports.said();
            assert_eq!(said.len(), 1, "{said:?}");
            assert!(
                said[0].contains("can't be read") && said[0].contains("off"),
                "{said:?}"
            );
            // A restart reads the off kept in its place: still off, nothing said.
            let reports = Reports::open(places.clone());
            assert!(!reports.consent().share);
            assert!(reports.said().is_empty(), "{:?}", reports.said());
        }
        // Spoiled while running: off at the next send, said once.
        let places = places("unread-own-live");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        assert!(reports.consent().share);
        fs::write(&places.own, "{\"share\":tr").unwrap();
        assert_eq!(reports.health(NOW, None, &planted()), None);
        assert!(!reports.consent().share);
        assert_eq!(reports.said().len(), 1, "{:?}", reports.said());
    }

    /// #2052: a newer build's choice, with a field or a `by` this build
    /// doesn't know, reads as off whatever its `share` says, is left as it
    /// is for a roll forward, and is said once.
    #[test]
    fn a_newer_builds_choice_reads_off_and_is_kept() {
        for newer in [
            r#"{"share":true,"at":"2026-10-19T04:00:00Z","by":"app","held":false,"scope":"more"}"#,
            r#"{"share":true,"at":"2026-10-19T04:00:00Z","by":"schedule","held":false}"#,
            r#"{"share":true,"at":"2026-10-19T04:00:00Z","by":"voice","held":"yes"}"#,
        ] {
            let places = places("newer-own");
            yes(&places);
            fs::write(&places.own, newer).unwrap();
            let mut reports = Reports::open(places.clone());
            assert!(!reports.consent().share, "{newer}");
            assert_eq!(reports.health(NOW, None, &planted()), None, "{newer}");
            assert!(reports.due(ID).is_empty());
            assert_eq!(fs::read_to_string(&places.own).unwrap(), newer);
            let said = reports.said();
            assert_eq!(said.len(), 1, "{newer}: {said:?}");
            assert!(said[0].contains("newer build"), "{said:?}");
        }
        // Setup's answer, a newer build's: off, left there, said.
        for newer in [
            r#"{"share":true,"asked":1,"at":"2026-10-19T04:00:00Z","scope":"more"}"#,
            "{\"share\":true,\"asked\":1,\"at\":\"2026",
        ] {
            let places = places("newer-setup");
            fs::write(&places.setup, newer).unwrap();
            let mut reports = Reports::open(places.clone());
            assert!(!reports.consent().share, "{newer}");
            assert_eq!(reports.health(NOW, None, &planted()), None, "{newer}");
            assert_eq!(fs::read_to_string(&places.setup).unwrap(), newer);
            assert_eq!(reports.said().len(), 1, "{:?}", reports.said());
        }
        // Today's records, setup's as `lamp-setup` writes it, read as before
        // and say nothing.
        let places = places("today");
        yes(&places);
        let reports = Reports::open(places.clone());
        assert!(reports.consent().share);
        fs::write(
            &places.own,
            r#"{"share":true,"at":"2026-10-19T04:00:00Z","by":"app","held":false}"#,
        )
        .unwrap();
        assert!(reports.consent().share);
        assert!(reports.said().is_empty(), "{:?}", reports.said());
    }

    /// #2052: a queued report from one release ahead never stops the part
    /// and is never sent where the backend may not take it. A newer field
    /// under [`SCHEMA`] goes as is; a newer schema or kind is kept and not
    /// sent (a no deletes it); one it can't read is kept whole under
    /// another name; none of them deletes the rest.
    #[test]
    fn queued_reports_read_one_release_forward() {
        let places = places("queue-forward");
        yes(&places);
        let today = |seq: u64, extra: &str| {
            format!(
                r#"{{"type":"report","schema":"lamp-report/1","device":"lamp","seq":{seq},"sent_at":"2026-10-18T04:00:00Z","kind":"crash"{extra}}}"#
            )
        };
        queued_file(&places, 3, &today(3, ""));
        queued_file(
            &places,
            4,
            &today(4, r#","panic":{"file":"a.rs","line":3}"#),
        );
        let newer_schema =
            r#"{"schema":"lamp-report/2","seq":5,"kind":"crash","sent_at":"2026-10-18T04:00:00Z"}"#;
        queued_file(&places, 5, newer_schema);
        let newer_kind = today(6, "").replace("\"crash\"", "\"panic\"");
        queued_file(&places, 6, &newer_kind);
        let truncated = &today(7, "")[..40];
        queued_file(&places, 7, truncated);
        queued_file(&places, 8, "\u{0}\u{1}garbage");
        let mut reports = Reports::open(places.clone());
        let went = sent(&mut reports);
        let seqs: Vec<&Value> = went.iter().map(|report| &report["seq"]).collect();
        assert_eq!(seqs, [&json!(3), &json!(4)]);
        assert_eq!(went[1]["panic"]["line"], 3, "a newer field goes as is");
        // Kept where they are, whole, for the newer build.
        assert_eq!(
            fs::read_to_string(places.queue.join("5.json")).unwrap(),
            newer_schema
        );
        assert_eq!(
            fs::read_to_string(places.queue.join("6.json")).unwrap(),
            newer_kind
        );
        // Set aside whole.
        assert!(!places.queue.join("7.json").exists());
        assert_eq!(
            fs::read_to_string(aside(&places.queue, "7.json.unread-")).unwrap(),
            truncated
        );
        assert_eq!(
            fs::read_to_string(aside(&places.queue, "8.json.unread-")).unwrap(),
            "\u{0}\u{1}garbage"
        );
        let said = reports.said();
        assert_eq!(said.len(), 4, "{said:?}");
        assert!(
            said.iter()
                .any(|line| line.contains("5.json") && line.contains("not sent"))
        );
        // Past every report on flash.
        assert_eq!(reports.health(NOW, None, &planted()), Some(9));
        // A restart reads the same and sets nothing more aside.
        drop(reports);
        let mut reports = Reports::open(places.clone());
        assert_eq!(reports.queued(), 2, "3 and 4; the health was in memory");
        assert!(
            sent(&mut reports)
                .iter()
                .all(|r| r["seq"] != 5 && r["seq"] != 6)
        );
        // A no deletes what waits, the newer build's too; what is set aside
        // stays.
        reports.set(false, NOW + 5).unwrap();
        for seq in [3, 4, 5, 6] {
            assert!(!places.queue.join(format!("{seq}.json")).exists(), "{seq}");
        }
        aside(&places.queue, "7.json.unread-");
        aside(&places.queue, "8.json.unread-");
    }

    /// #2052: the queue's state from one release ahead, a field added or
    /// one gone, reads; one it can't read is set aside whole, and the next
    /// seq goes past the reports still waiting (it was 1, a seq reused).
    #[test]
    fn the_queues_state_reads_forward_and_is_set_aside_when_it_cant() {
        let forward = places("state-forward");
        yes(&forward);
        fs::create_dir_all(&forward.queue).unwrap();
        fs::write(
            forward.queue.join("state.json"),
            r#"{"seq":7,"dropped":{"queue_full":2,"later":1},"boot":"boot-1","later":true}"#,
        )
        .unwrap();
        let mut reports = Reports::open(forward);
        assert_eq!(reports.dropped().queue_full, 2);
        assert_eq!(reports.health(NOW, None, &planted()), Some(8));
        assert!(reports.said().is_empty(), "{:?}", reports.said());
        for spoiled in ["{\"seq\":", r#"{"seq":"seven"}"#] {
            let places = places("state-unread");
            yes(&places);
            queued_file(
                &places,
                4,
                r#"{"type":"report","schema":"lamp-report/1","seq":4,"sent_at":"2026-10-18T04:00:00Z","kind":"crash"}"#,
            );
            fs::write(places.queue.join("state.json"), spoiled).unwrap();
            let mut reports = Reports::open(places.clone());
            assert_eq!(
                fs::read_to_string(aside(&places.queue, "state.json.unread-")).unwrap(),
                spoiled
            );
            assert_eq!(reports.queued(), 1, "{spoiled}");
            assert_eq!(reports.health(NOW, None, &planted()), Some(5), "{spoiled}");
            let said = reports.said();
            assert_eq!(said.len(), 1, "{said:?}");
            assert!(
                said[0].contains("state.json") && said[0].contains("kept as"),
                "{said:?}"
            );
        }
    }

    /// #2052: an exit note this build can't read is skipped with one line
    /// and left whole (the folder is root's); a newer field reads; the
    /// rest are taken.
    #[test]
    fn an_exit_note_it_cant_read_is_skipped_once_and_left_whole() {
        let places = places("notes-forward");
        yes(&places);
        let mut reports = Reports::open(places.clone());
        write_exit_note(&places.exits, &planted_note("lamp-air.service", 5)).unwrap();
        let mut newer = serde_json::to_value(planted_note("lamp-eyes.service", 6)).unwrap();
        newer["core"] = json!(true);
        fs::write(
            places.exits.join("lamp-eyes.service@6.json"),
            newer.to_string(),
        )
        .unwrap();
        fs::write(
            places.exits.join("lamp-ring.service@7.json"),
            "{\"unit\":\"lamp-r",
        )
        .unwrap();
        fs::write(
            places.exits.join("lamp-ring.service@8.json"),
            r#"{"unit":3}"#,
        )
        .unwrap();
        // `write_whole`'s temporary is not a note yet, and says nothing.
        fs::write(places.exits.join("lamp-ring.service@9.tmp"), "{").unwrap();
        let notes = reports.new_notes();
        let units: Vec<&str> = notes.iter().map(|note| note.unit.as_str()).collect();
        assert_eq!(units, ["lamp-air.service", "lamp-eyes.service"]);
        assert_eq!(reports.new_notes(), Vec::new());
        let said = reports.said();
        assert_eq!(said.len(), 2, "each said once: {said:?}");
        assert!(
            said.iter().all(|line| line.contains("exit note")),
            "{said:?}"
        );
        assert_eq!(
            fs::read_to_string(places.exits.join("lamp-ring.service@7.json")).unwrap(),
            "{\"unit\":\"lamp-r"
        );
    }

    /// The reviewer on #2077: a no given while the record reads off only
    /// because this build can't settle it (a newer build's yes, or one
    /// spoiled while running) is still a no. The voice's and the app's both
    /// write it, delete what waits, and queue `stopped`, sent twice.
    #[test]
    fn a_no_given_while_a_newer_yes_reads_off_is_kept_and_does_all_a_no_does() {
        let newer_yes =
            r#"{"share":true,"at":"2026-10-19T04:00:00Z","by":"app","held":false,"scope":"crash"}"#;
        for record in [newer_yes, "{\"share\":tr"] {
            for by in ["voice", "app"] {
                let places = places("no-over-newer");
                yes(&places);
                queued_file(
                    &places,
                    3,
                    r#"{"type":"report","schema":"lamp-report/1","seq":3,"sent_at":"2026-10-18T04:00:00Z","kind":"crash"}"#,
                );
                let mut reports = Reports::open(places.clone());
                fs::write(&places.own, record).unwrap();
                assert!(!reports.consent().share, "{record}");
                assert!(reports.due(ID).is_empty(), "nothing goes on it");
                reports.unhand(None);
                if by == "voice" {
                    reports.stop(NOW + 5, false).unwrap();
                } else {
                    reports.set(false, NOW + 5).unwrap();
                }
                let own: Value = serde_json::from_slice(&fs::read(&places.own).unwrap()).unwrap();
                assert_eq!(
                    (&own["share"], &own["by"]),
                    (&json!(false), &json!(by)),
                    "{by} over {record}: the no is written"
                );
                assert!(
                    !places.queue.join("3.json").exists(),
                    "{by} over {record}: what waits is deleted"
                );
                let first = sent(&mut reports);
                assert_eq!(first.len(), 1, "{by} over {record}");
                assert_eq!(
                    (&first[0]["kind"], &first[0]["seq"]),
                    (&json!("stopped"), &json!(4))
                );
                // Sent once more on the next connection, as designed.
                reports.acked(4);
                reports.unhand(None);
                reports.connected();
                assert_eq!(sent(&mut reports), first, "{by} over {record}");
            }
        }
    }

    /// #2061's rule, the reviewer on #2077: a line about a file names the
    /// error's kind and place, never a value from it.
    #[test]
    fn no_value_from_a_file_reaches_the_journal() {
        const PLANTED: &str = "Alice-s3cret";
        let places = places("no-values");
        yes(&places);
        fs::create_dir_all(&places.queue).unwrap();
        fs::write(
            places.queue.join("state.json"),
            format!(r#"{{"seq":"{PLANTED}"}}"#),
        )
        .unwrap();
        queued_file(
            &places,
            3,
            &format!(
                r#"{{"type":"report","schema":"lamp-report/1","seq":3,"sent_at":"2026-10-18T04:00:00Z","kind":"{PLANTED}"}}"#
            ),
        );
        queued_file(
            &places,
            4,
            &format!(r#"{{"schema":"lamp-report/2","seq":4,"note":"{PLANTED}"}}"#),
        );
        let mut reports = Reports::open(places.clone());
        let mut note = serde_json::to_value(planted_note("lamp-air.service", 5)).unwrap();
        note["at_us"] = json!(PLANTED);
        fs::create_dir_all(&places.exits).unwrap();
        fs::write(
            places.exits.join("lamp-air.service@5.json"),
            note.to_string(),
        )
        .unwrap();
        assert!(reports.new_notes().is_empty());
        fs::write(
            &places.own,
            format!(r#"{{"share":true,"at":"2026-10-19T04:00:00Z","by":"{PLANTED}"}}"#),
        )
        .unwrap();
        assert!(!reports.consent().share);
        let said = reports.said();
        assert_eq!(said.len(), 5, "{said:?}");
        for line in &said {
            assert!(
                !line.contains("Alice") && !line.contains("s3cret"),
                "a value reached the journal: {line}"
            );
        }
        assert!(
            said.iter()
                .any(|line| line.contains("state.json") && line.contains("Data at line 1 column")),
            "the kind and place are said: {said:?}"
        );
    }
}
